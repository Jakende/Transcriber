//! One-time, pure Rust migration of unquantized whisper.cpp weights.
use crate::{
    models::{self, ModelSize},
    storage,
};
use anyhow::{Context, Result, ensure};
use candle_core::{DType, Device, Tensor};
use std::{
    collections::HashMap,
    fs,
    io::{BufReader, Read, Seek, SeekFrom},
    path::{Path, PathBuf},
    sync::atomic::AtomicBool,
};
fn integer(reader: &mut impl Read) -> Result<u32> {
    let mut bytes = [0; 4];
    reader.read_exact(&mut bytes)?;
    Ok(u32::from_le_bytes(bytes))
}
pub fn tensor_name(name: &str) -> String {
    let mut name = name
        .replace(".blocks.", ".layers.")
        .replace(".cross_attn_ln.", ".encoder_attn_layer_norm.")
        .replace(".attn_ln.", ".self_attn_layer_norm.")
        .replace(".mlp_ln.", ".final_layer_norm.")
        .replace(".cross_attn.", ".encoder_attn.")
        .replace(".attn.", ".self_attn.")
        .replace(".query.", ".q_proj.")
        .replace(".key.", ".k_proj.")
        .replace(".value.", ".v_proj.")
        .replace(".out.", ".out_proj.")
        .replace(".mlp.0.", ".fc1.")
        .replace(".mlp.2.", ".fc2.")
        .replace(".ln_post.", ".layer_norm.")
        .replace(".ln.", ".layer_norm.")
        .replace(".token_embedding.", ".embed_tokens.");
    if name.ends_with(".positional_embedding") {
        name = name.replace(".positional_embedding", ".embed_positions.weight");
    }
    format!("model.{name}")
}
pub fn import(
    path: &Path,
    size: ModelSize,
    cancel: &AtomicBool,
    progress: &mut dyn FnMut(f32, String),
) -> Result<PathBuf> {
    let dir = models::model_root().join(size.name());
    fs::create_dir_all(&dir)?;
    let output = dir.join("model.safetensors");
    ensure!(
        !output.exists(),
        "Modell existiert bereits: {}",
        output.display()
    );
    let config_url = format!(
        "https://huggingface.co/openai/whisper-{}/resolve/{}/config.json",
        size.name(),
        size.revision()
    );
    let http = models::client()?;
    for (name, url) in [
        ("config.json", config_url),
        (
            "tokenizer.json",
            format!(
                "https://huggingface.co/openai/whisper-{}/resolve/{}/tokenizer.json",
                size.name(),
                size.revision()
            ),
        ),
    ] {
        if !dir.join(name).exists() {
            models::download(&http, &url, &dir.join(name), cancel, &mut |_| {})?;
        }
    }
    let config: candle_transformers::models::whisper::Config =
        serde_json::from_slice(&fs::read(dir.join("config.json"))?)?;
    let tensors = read_tensors(path, &config, cancel, progress)?;
    let part = dir.join(format!("{}.safetensors.part", uuid::Uuid::new_v4()));
    candle_core::safetensors::save(&tensors, &part)?;
    let hash = models::file_hash(&part, cancel)?;
    let provenance = serde_json::json!({"format":"whisper.cpp-f16-import","model":size.name(),"sha256":hash,"source_sha256":models::file_hash(path,cancel)?});
    // Publish metadata first, then the finished model; no incomplete model is ever visible.
    storage::save(&dir.join("import.json"), &provenance)?;
    fs::rename(part, output)?;
    Ok(dir)
}
pub fn read_tensors(
    path: &Path,
    config: &candle_transformers::models::whisper::Config,
    cancel: &AtomicBool,
    progress: &mut dyn FnMut(f32, String),
) -> Result<HashMap<String, Tensor>> {
    let mut reader = BufReader::new(fs::File::open(path)?);
    ensure!(
        integer(&mut reader)? == 0x67676d6c,
        "Keine Whisper-GGML-Datei."
    );
    let h = (0..11)
        .map(|_| integer(&mut reader))
        .collect::<Result<Vec<_>>>()?;
    ensure!(
        h[0] as usize == config.vocab_size
            && h[2] as usize == config.d_model
            && h[4] as usize == config.encoder_layers
            && h[8] as usize == config.decoder_layers
            && h[9] as usize == config.num_mel_bins,
        "GGML-Datei passt nicht zum gewählten Modell."
    );
    let mel = integer(&mut reader)?;
    let fft = integer(&mut reader)?;
    ensure!(mel <= 128 && fft <= 1000, "Ungültige Filterdimensionen.");
    reader.seek(SeekFrom::Current(mel as i64 * fft as i64 * 4))?;
    let vocab = integer(&mut reader)?;
    ensure!(vocab <= config.vocab_size as u32, "Ungültiges Vokabular.");
    for _ in 0..vocab {
        let len = integer(&mut reader)?;
        ensure!(len < 1_000_000, "Ungültiges Token.");
        reader.seek(SeekFrom::Current(len as i64))?;
    }
    let total = reader.get_ref().metadata()?.len();
    let mut tensors = HashMap::new();
    while reader.stream_position()? < total {
        crate::audio::check_cancel(cancel)?;
        let dims = integer(&mut reader)?;
        let name_len = integer(&mut reader)?;
        let kind = integer(&mut reader)?;
        ensure!(
            (1..=3).contains(&dims) && name_len < 1024,
            "Ungültiger Tensorheader."
        );
        let mut shape = (0..dims)
            .map(|_| integer(&mut reader).map(|v| v as usize))
            .collect::<Result<Vec<_>>>()?;
        shape.reverse();
        let mut name = vec![0; name_len as usize];
        reader.read_exact(&mut name)?;
        let name = String::from_utf8(name)?;
        let dtype = match kind {
            0 => DType::F32,
            1 => DType::F16,
            _ => DType::F32,
        };
        if name.ends_with(".bias") {
            shape.retain(|v| *v != 1);
        }
        let elements = shape
            .iter()
            .try_fold(1usize, |acc, v| acc.checked_mul(*v))
            .context("Tensor ist zu groß.")?;
        ensure!(elements < 1_000_000_000, "Tensor ist zu groß.");
        let tensor = if kind <= 1 {
            let mut bytes = vec![0; elements * dtype.size_in_bytes()];
            reader.read_exact(&mut bytes)?;
            Tensor::from_raw_buffer(&bytes, dtype, &shape, &Device::Cpu)?
        } else {
            use candle_core::quantized::{GgmlDType, ggml_file::qtensor_from_ggml};
            let dtype = match kind {
                2 => GgmlDType::Q4_0,
                3 => GgmlDType::Q4_1,
                6 => GgmlDType::Q5_0,
                7 => GgmlDType::Q5_1,
                8 => GgmlDType::Q8_0,
                _ => anyhow::bail!("GGML-Quantisierung {kind} wird nicht unterstützt."),
            };
            ensure!(elements % dtype.block_size() == 0, "Ungültige Blockgröße.");
            let mut bytes = vec![0; elements / dtype.block_size() * dtype.type_size()];
            reader.read_exact(&mut bytes)?;
            qtensor_from_ggml(dtype, &bytes, shape, &Device::Cpu)?.dequantize(&Device::Cpu)?
        };
        tensors.insert(tensor_name(&name), tensor);
        progress(
            reader.stream_position()? as f32 / total as f32,
            format!(
                "Vorhandenes Modell laden · {:.0} %",
                reader.stream_position()? as f32 / total as f32 * 100.
            ),
        );
    }
    Ok(tensors)
}

pub fn read_openai_pytorch(path: &Path, cancel: &AtomicBool) -> Result<HashMap<String, Tensor>> {
    let pth = candle_core::pickle::PthTensors::new(path, Some("model_state_dict"))?;
    let mut tensors = HashMap::new();
    for name in pth.tensor_infos().keys() {
        crate::audio::check_cancel(cancel)?;
        if let Some(tensor) = pth.get(name)? {
            tensors.insert(tensor_name(name), tensor);
        }
    }
    Ok(tensors)
}
