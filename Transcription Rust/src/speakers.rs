//! ECAPA-TDNN inference in Candle, with the original SpeechBrain weights.
use crate::{
    audio::{RATE, check_cancel},
    document::Document,
    models,
};
use anyhow::{Context, Result, ensure};
use candle_core::{DType, Device, Module, ModuleT, Tensor};
use candle_nn::{BatchNorm, Conv1d, Conv1dConfig, VarBuilder};
use sha2::{Digest, Sha256};
use std::{path::PathBuf, sync::atomic::AtomicBool};
const REVISION: &str = "0f99f2d0ebe89ac095bcc5903c4dd8f72b367286";
const HASH: &str = "0575cb64845e6b9a10db9bcb74d5ac32b326b8dc90352671d345e2ee3d0126a2";

struct Conv {
    layer: Conv1d,
    padding: usize,
}
impl Conv {
    fn load(
        input: usize,
        output: usize,
        kernel: usize,
        dilation: usize,
        vb: VarBuilder,
    ) -> Result<Self> {
        Ok(Self {
            layer: candle_nn::conv1d(
                input,
                output,
                kernel,
                Conv1dConfig {
                    dilation,
                    ..Default::default()
                },
                vb,
            )?,
            padding: (kernel - 1) * dilation / 2,
        })
    }
    fn forward(&self, x: &Tensor) -> candle_core::Result<Tensor> {
        let padded = if self.padding == 0 {
            x.clone()
        } else {
            let len = x.dim(2)?;
            let indices: Vec<u32> = (1..=self.padding)
                .rev()
                .chain(0..len)
                .chain((len - self.padding - 1..len - 1).rev())
                .map(|n| n as u32)
                .collect();
            x.index_select(&Tensor::new(indices.as_slice(), x.device())?, 2)?
        };
        self.layer.forward(&padded.contiguous()?)
    }
}
struct Tdnn {
    conv: Conv,
    norm: BatchNorm,
}
impl Tdnn {
    fn load(input: usize, output: usize, k: usize, d: usize, vb: VarBuilder) -> Result<Self> {
        Ok(Self {
            conv: Conv::load(input, output, k, d, vb.pp("conv.conv"))?,
            norm: candle_nn::batch_norm(output, 1e-5, vb.pp("norm.norm"))?,
        })
    }
    fn forward(&self, x: &Tensor) -> candle_core::Result<Tensor> {
        self.norm.forward_t(&self.conv.forward(x)?.relu()?, false)
    }
}
struct Residual {
    first: Tdnn,
    parts: Vec<Tdnn>,
    last: Tdnn,
    se1: Conv,
    se2: Conv,
}
impl Residual {
    fn load(dilation: usize, vb: VarBuilder) -> Result<Self> {
        let parts = (0..7)
            .map(|i| {
                Tdnn::load(
                    128,
                    128,
                    3,
                    dilation,
                    vb.pp(format!("res2net_block.blocks.{i}")),
                )
            })
            .collect::<Result<_>>()?;
        Ok(Self {
            first: Tdnn::load(1024, 1024, 1, 1, vb.pp("tdnn1"))?,
            parts,
            last: Tdnn::load(1024, 1024, 1, 1, vb.pp("tdnn2"))?,
            se1: Conv::load(1024, 128, 1, 1, vb.pp("se_block.conv1.conv"))?,
            se2: Conv::load(128, 1024, 1, 1, vb.pp("se_block.conv2.conv"))?,
        })
    }
    fn forward(&self, input: &Tensor) -> candle_core::Result<Tensor> {
        let x = self.first.forward(input)?;
        let mut parts = vec![x.narrow(1, 0, 128)?];
        for i in 1..8 {
            let mut part = x.narrow(1, i * 128, 128)?;
            if i > 1 {
                part = (part + &parts[i - 1])?;
            }
            parts.push(self.parts[i - 1].forward(&part.contiguous()?)?);
        }
        let x = self.last.forward(&Tensor::cat(&parts, 1)?)?;
        let gate = candle_nn::ops::sigmoid(
            &self
                .se2
                .forward(&self.se1.forward(&x.mean_keepdim(2)?)?.relu()?)?,
        )?;
        x.broadcast_mul(&gate)? + input
    }
}
pub struct SpeakerEngine {
    first: Tdnn,
    blocks: Vec<Residual>,
    mfa: Tdnn,
    attention: Tdnn,
    attention_conv: Conv,
    norm: BatchNorm,
    fc: Conv,
    device: Device,
}
impl SpeakerEngine {
    pub fn load(
        cpu: bool,
        cancel: &AtomicBool,
        progress: &mut dyn FnMut(f32, String),
    ) -> Result<Self> {
        let path = ensure_weights(cancel, progress)?;
        let device = crate::engine::select_device(cpu)?;
        let vb = VarBuilder::from_pth(path, DType::F32, &device)?;
        Ok(Self {
            first: Tdnn::load(80, 1024, 5, 1, vb.pp("blocks.0"))?,
            blocks: (1..4)
                .map(|i| Residual::load(i + 1, vb.pp(format!("blocks.{i}"))))
                .collect::<Result<_>>()?,
            mfa: Tdnn::load(3072, 3072, 1, 1, vb.pp("mfa"))?,
            attention: Tdnn::load(9216, 128, 1, 1, vb.pp("asp.tdnn"))?,
            attention_conv: Conv::load(128, 3072, 1, 1, vb.pp("asp.conv.conv"))?,
            norm: candle_nn::batch_norm(6144, 1e-5, vb.pp("asp_bn.norm"))?,
            fc: Conv::load(6144, 192, 1, 1, vb.pp("fc.conv"))?,
            device,
        })
    }
    pub fn embedding(&self, samples: &[f32]) -> Result<Vec<f32>> {
        let (features, frames) = fbank(samples);
        let input = Tensor::from_vec(features, (1, 80, frames), &self.device)?;
        let mut x = self.first.forward(&input)?;
        let mut outputs = vec![];
        for block in &self.blocks {
            x = block.forward(&x)?;
            outputs.push(x.clone());
        }
        let x = self.mfa.forward(&Tensor::cat(&outputs, 1)?)?;
        let mean = x.mean_keepdim(2)?;
        let std = x
            .broadcast_sub(&mean)?
            .sqr()?
            .mean_keepdim(2)?
            .clamp(1e-12, f64::INFINITY)?
            .sqrt()?;
        let context = Tensor::cat(
            &[
                &x,
                &mean.broadcast_as(x.shape())?,
                &std.broadcast_as(x.shape())?,
            ],
            1,
        )?;
        let weights = candle_nn::ops::softmax(
            &self
                .attention_conv
                .forward(&self.attention.forward(&context)?.tanh()?)?,
            2,
        )?;
        let mean = (&weights * &x)?.sum_keepdim(2)?;
        let std = (&weights * x.broadcast_sub(&mean)?.sqr()?)?
            .sum_keepdim(2)?
            .clamp(1e-12, f64::INFINITY)?
            .sqrt()?;
        let pooled = self.norm.forward_t(&Tensor::cat(&[mean, std], 1)?, false)?;
        let vector = self.fc.forward(&pooled)?.flatten_all()?.to_vec1::<f32>()?;
        Ok(normalize(vector))
    }
}
pub fn ensure_weights(
    cancel: &AtomicBool,
    progress: &mut dyn FnMut(f32, String),
) -> Result<PathBuf> {
    let bundled = std::env::current_exe().ok().and_then(|p| {
        p.parent()?
            .parent()
            .map(|p| p.join("Resources/models/ecapa/embedding_model.ckpt"))
    });
    let path = bundled
        .filter(|p| p.exists())
        .or_else(|| discover_weights().into_iter().next())
        .unwrap_or_else(|| models::model_root().join("ecapa/embedding_model.ckpt"));
    if !path.exists() {
        models::download(
            &models::client()?,
            &format!(
                "https://huggingface.co/speechbrain/spkrec-ecapa-voxceleb/resolve/{REVISION}/embedding_model.ckpt"
            ),
            &path,
            cancel,
            &mut |p| progress(p, "Sprechermodell laden …".into()),
        )?;
    }
    use std::io::Read;
    let mut f = std::fs::File::open(&path)?;
    let mut hasher = Sha256::new();
    let mut buf = vec![0; 1024 * 1024];
    loop {
        check_cancel(cancel)?;
        let n = f.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    ensure!(
        format!("{:x}", hasher.finalize()) == HASH,
        "Sprechermodell ist beschädigt."
    );
    Ok(path)
}
/// SpeechBrain-compatible Hamming STFT, HTK mel triangles, log power, sentence mean.
pub fn fbank(samples: &[f32]) -> (Vec<f32>, usize) {
    use rustfft::{FftPlanner, num_complex::Complex};
    let mut padded = samples.to_vec();
    padded.resize(padded.len().max(800), 0.);
    let frames = padded.len() / 160 + 1;
    let mut features = vec![0f32; 80 * frames];
    let edges: Vec<_> = (0..82)
        .map(|i| {
            700. * (10f64.powf((2595. * (1. + 8000. / 700f64).log10()) * i as f64 / 81. / 2595.)
                - 1.)
        })
        .collect();
    let mut planner = FftPlanner::<f32>::new();
    let fft = planner.plan_fft_forward(400);
    let mut buffer = vec![Complex::default(); 400];
    for frame in 0..frames {
        for (i, value) in buffer.iter_mut().enumerate() {
            let index = frame as isize * 160 + i as isize - 200;
            let sample = if index < 0 || index as usize >= padded.len() {
                0.
            } else {
                padded[index as usize]
            };
            let window = 0.54 - 0.46 * (2. * std::f32::consts::PI * i as f32 / 400.).cos();
            *value = Complex::new(sample * window, 0.);
        }
        fft.process(&mut buffer);
        for mel in 0..80 {
            let mut energy = 0f64;
            for (k, v) in buffer[..201].iter().enumerate() {
                let hz = k as f64 * 40.;
                let weight =
                    (1. - ((hz - edges[mel + 1]) / (edges[mel + 1] - edges[mel])).abs()).max(0.);
                energy += v.norm_sqr() as f64 * weight;
            }
            features[mel * frames + frame] = 10. * energy.max(1e-10).log10() as f32;
        }
    }
    let floor = features.iter().copied().fold(f32::NEG_INFINITY, f32::max) - 80.;
    for channel in features.chunks_mut(frames) {
        for v in channel.iter_mut() {
            *v = v.max(floor);
        }
        let mean = channel.iter().sum::<f32>() / frames as f32;
        for v in channel {
            *v -= mean;
        }
    }
    (features, frames)
}
fn normalize(mut v: Vec<f32>) -> Vec<f32> {
    let norm = v.iter().map(|x| x * x).sum::<f32>().sqrt().max(1e-12);
    for x in &mut v {
        *x /= norm;
    }
    v
}
fn cosine(a: &[f32], b: &[f32]) -> f32 {
    a.iter().zip(b).map(|(a, b)| a * b).sum()
}
pub fn cluster(vectors: &[Vec<f32>], requested: usize) -> Vec<usize> {
    if vectors.is_empty() {
        return vec![];
    }
    let mut centers = vec![vectors[0].clone()];
    let k = requested.clamp(1, 10).min(vectors.len());
    if requested > 0 {
        while centers.len() < k {
            let candidate = vectors
                .iter()
                .max_by(|a, b| {
                    let similarity =
                        |v: &Vec<f32>| centers.iter().map(|c| cosine(v, c)).fold(-1., f32::max);
                    similarity(b).total_cmp(&similarity(a))
                })
                .unwrap();
            centers.push(candidate.clone());
        }
    } else {
        for vector in vectors.iter().skip(1) {
            let score = centers
                .iter()
                .map(|c| cosine(vector, c))
                .fold(-1., f32::max);
            if score < 0.65 && centers.len() < 10 {
                centers.push(vector.clone());
            }
        }
    }
    let mut labels = vec![0; vectors.len()];
    for _ in 0..12 {
        for (i, v) in vectors.iter().enumerate() {
            labels[i] = centers
                .iter()
                .enumerate()
                .max_by(|a, b| cosine(v, a.1).total_cmp(&cosine(v, b.1)))
                .unwrap()
                .0;
        }
        let mut sums = vec![vec![0.; vectors[0].len()]; centers.len()];
        let mut counts = vec![0; centers.len()];
        for (v, &label) in vectors.iter().zip(&labels) {
            counts[label] += 1;
            for (sum, value) in sums[label].iter_mut().zip(v) {
                *sum += value;
            }
        }
        for i in 0..centers.len() {
            if counts[i] > 0 {
                centers[i] = normalize(sums[i].clone());
            }
        }
    }
    // Stable labels in order of first appearance.
    let mut order = vec![];
    labels
        .into_iter()
        .map(|label| {
            if !order.contains(&label) {
                order.push(label);
            }
            order.iter().position(|v| *v == label).unwrap()
        })
        .collect()
}
pub fn assign(
    pcm: &[f32],
    doc: &mut Document,
    requested: usize,
    cancel: &AtomicBool,
    progress: &mut dyn FnMut(f32, String),
) -> Result<()> {
    let engine = SpeakerEngine::load(doc.device == "cpu", cancel, progress)?;
    let mut vectors = vec![];
    for (i, segment) in doc.segments.iter().enumerate() {
        check_cancel(cancel)?;
        let start = (segment.start * RATE as f64) as usize;
        let end = ((segment.end * RATE as f64) as usize)
            .min(pcm.len())
            .min(start + RATE as usize * 3);
        let sample = pcm
            .get(start..end)
            .filter(|s| !s.is_empty())
            .context("Segment liegt außerhalb der Audiodatei.")?;
        vectors.push(engine.embedding(sample)?);
        progress(
            i as f32 / doc.segments.len() as f32,
            format!("Stimmabdruck {}/{}", i + 1, doc.segments.len()),
        );
    }
    for (segment, label) in doc.segments.iter_mut().zip(cluster(&vectors, requested)) {
        let id = format!("SPEAKER_{:02}", label + 1);
        segment.speaker = Some(id.clone());
        doc.speaker_names
            .entry(id)
            .or_insert_with(|| format!("Sprecher {}", label + 1));
    }
    doc.diarization = true;
    doc.speaker_model = Some("speechbrain/ecapa-tdnn · candle-rust".into());
    Ok(())
}

pub fn discover_weights() -> Vec<PathBuf> {
    let mut found = vec![];
    for root in crate::discovery::standard_roots() {
        for suffix in [
            "ecapa/embedding_model.ckpt",
            "speaker/spkrec-ecapa-voxceleb/embedding_model.ckpt",
            "embedding_model.ckpt",
        ] {
            if let Some(path) = crate::discovery::resolve_hub_link(&root.join(suffix)) {
                found.push(path);
            }
        }
        let snapshots = root.join("models--speechbrain--spkrec-ecapa-voxceleb/snapshots");
        if let Ok(entries) = std::fs::read_dir(snapshots) {
            for entry in entries.flatten().take(100) {
                if let Some(path) =
                    crate::discovery::resolve_hub_link(&entry.path().join("embedding_model.ckpt"))
                {
                    found.push(path);
                }
            }
        }
    }
    found
}
