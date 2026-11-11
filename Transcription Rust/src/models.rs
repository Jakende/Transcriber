use crate::{audio::check_cancel, storage::data_dir};
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::atomic::AtomicBool,
    time::Duration,
};

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq, clap::ValueEnum)]
pub enum ModelSize {
    Tiny,
    #[default]
    Small,
    Medium,
    Turbo,
}
impl ModelSize {
    pub const ALL: [Self; 4] = [Self::Tiny, Self::Small, Self::Medium, Self::Turbo];
    pub fn name(self) -> &'static str {
        match self {
            Self::Tiny => "tiny",
            Self::Small => "small",
            Self::Medium => "medium",
            Self::Turbo => "large-v3-turbo",
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            Self::Tiny => "Tiny · 151 MB",
            Self::Small => "Small · 967 MB",
            Self::Medium => "Medium · 3,1 GB",
            Self::Turbo => "Large v3 Turbo · 1,6 GB",
        }
    }
    pub fn revision(self) -> &'static str {
        match self {
            Self::Tiny => "169d4a4341b33bc18d8881c4b69c2e104e1cc0af",
            Self::Small => "973afd24965f72e36ca33b3055d56a652f456b4d",
            Self::Medium => "abdf7c39ab9d0397620ccaea8974cc764cd0953e",
            Self::Turbo => "41f01f3fe87f28c78e2fbf8b568835947dd65ed9",
        }
    }
    pub fn sha256(self) -> &'static str {
        match self {
            Self::Tiny => "7ebd0e69e78190ffe1438491fa05cc1f5c1aa3a4c4db3bc1723adbb551ea2395",
            Self::Small => "1d7734884874f1a1513ed9aa760a4f8e97aaa02fd6d93a3a85d27b2ae9ca596b",
            Self::Medium => "62f73550fa6db24b0c6f6c5962bd0dae80fa644e93cde9cd9c3792971b47fd28",
            Self::Turbo => "542566a422ae4f3fd23f1ba11add198fca01bbf82e66e6a2857b3f608b1eb9d1",
        }
    }
}
pub fn model_root() -> PathBuf {
    std::env::var_os("TRANSCRIPTION_MODEL_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| data_dir().join("models"))
}
pub fn bundled_dir(size: ModelSize) -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let p = exe
        .parent()?
        .parent()?
        .join("Resources/models")
        .join(size.name());
    p.join("model.safetensors").is_file().then_some(p)
}
pub fn client() -> Result<reqwest::blocking::Client> {
    Ok(reqwest::blocking::Client::builder()
        .user_agent(concat!("TranscriptionRust/", env!("CARGO_PKG_VERSION")))
        .connect_timeout(Duration::from_secs(20))
        .timeout(Duration::from_secs(60))
        .build()?)
}
#[derive(Clone, Debug)]
pub struct ModelAssets {
    pub weights: PathBuf,
    pub format: crate::discovery::WeightFormat,
    pub metadata: PathBuf,
}
fn metadata_locations(size: ModelSize, weights: Option<&Path>) -> Vec<PathBuf> {
    let mut locations = vec![];
    if let Some(parent) = weights.and_then(Path::parent) {
        locations.push(parent.into());
    }
    if let Ok(exe) = std::env::current_exe()
        && let Some(contents) = exe.parent().and_then(Path::parent)
    {
        locations.push(contents.join("Resources/model-metadata").join(size.name()));
    }
    locations.push(model_root().join(size.name()));
    locations
}
pub fn ensure_metadata(
    size: ModelSize,
    weights: Option<&Path>,
    cancel: &AtomicBool,
    progress: &mut dyn FnMut(f32, String),
) -> Result<PathBuf> {
    for dir in metadata_locations(size, weights) {
        if dir.join("config.json").is_file() && dir.join("tokenizer.json").is_file() {
            return Ok(dir);
        }
    }
    let dir = model_root().join(size.name());
    fs::create_dir_all(&dir)?;
    let http = client()?;
    for name in ["config.json", "tokenizer.json"] {
        let path = dir.join(name);
        if !path.exists() {
            let url = format!(
                "https://huggingface.co/openai/whisper-{}/resolve/{}/{name}",
                size.name(),
                size.revision()
            );
            download(&http, &url, &path, cancel, &mut |p| {
                progress(p, format!("Modellmetadaten: {name}"))
            })?;
        }
    }
    Ok(dir)
}
pub fn resolve_model(
    size: ModelSize,
    cancel: &AtomicBool,
    progress: &mut dyn FnMut(f32, String),
) -> Result<ModelAssets> {
    if let Some(found) = crate::discovery::discover(size).into_iter().next() {
        progress(0., format!("Vorhandenes {}-Modell verwenden", size.name()));
        let metadata = ensure_metadata(size, Some(&found.weights), cancel, progress)?;
        if found.format == crate::discovery::WeightFormat::Safetensors {
            let owned = model_root().join(size.name());
            let bundled = bundled_dir(size);
            if found
                .weights
                .parent()
                .is_some_and(|p| p == owned || bundled.as_ref().is_some_and(|d| p == d))
            {
                verify_model(size, found.weights.parent().unwrap(), cancel)?;
            }
        }
        return Ok(ModelAssets {
            weights: found.weights,
            format: found.format,
            metadata,
        });
    }
    let metadata = ensure_metadata(size, None, cancel, progress)?;
    let dir = model_root().join(size.name());
    let weights = dir.join("model.safetensors");
    if weights.exists() {
        anyhow::bail!(
            "Vorhandene Modelldatei ist unvollständig oder passt nicht zum Modell: {}. Sie wird nicht überschrieben oder erneut heruntergeladen.",
            weights.display()
        );
    }
    let url = format!(
        "https://huggingface.co/openai/whisper-{}/resolve/{}/model.safetensors",
        size.name(),
        size.revision()
    );
    download(&client()?, &url, &weights, cancel, &mut |p| {
        progress(
            p,
            format!("Modell {} laden · {:.0} %", size.name(), p * 100.),
        )
    })?;
    progress(1., "Modellintegrität prüfen …".into());
    verify_model(size, &dir, cancel)?;
    Ok(ModelAssets {
        weights,
        metadata,
        format: crate::discovery::WeightFormat::Safetensors,
    })
}
/// Compatibility helper for CLI downloads: reuse is preferred over downloading.
pub fn ensure_model(
    size: ModelSize,
    cancel: &AtomicBool,
    progress: &mut dyn FnMut(f32, String),
) -> Result<PathBuf> {
    Ok(resolve_model(size, cancel, progress)?.weights)
}
pub fn verify_model(size: ModelSize, dir: &Path, cancel: &AtomicBool) -> Result<()> {
    let expected = if dir.join("import.json").exists() {
        let provenance: serde_json::Value = crate::storage::load(&dir.join("import.json"))?;
        ensure!(
            provenance["format"] == "whisper.cpp-f16-import" && provenance["model"] == size.name(),
            "Ungültige Importmetadaten."
        );
        provenance["sha256"]
            .as_str()
            .context("Importprüfsumme fehlt.")?
            .to_string()
    } else {
        size.sha256().into()
    };
    ensure!(
        file_hash(&dir.join("model.safetensors"), cancel)? == expected,
        "Modell beschädigt: SHA-256 stimmt nicht. Modellordner prüfen: {}",
        dir.display()
    );
    let _: candle_transformers::models::whisper::Config =
        serde_json::from_slice(&fs::read(dir.join("config.json"))?)?;
    Ok(())
}
pub fn download(
    _http: &reqwest::blocking::Client,
    url: &str,
    path: &Path,
    cancel: &AtomicBool,
    progress: &mut dyn FnMut(f32),
) -> Result<()> {
    let parsed = reqwest::Url::parse(url)?;
    ensure!(
        ["https", "http"].contains(&parsed.scheme()),
        "Nur HTTP(S)-Adressen sind erlaubt."
    );
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let lock_path = path.with_file_name(format!(
        ".{}.download-lock",
        path.file_name().unwrap_or_default().to_string_lossy()
    ));
    let lock = fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(lock_path)?;
    lock.try_lock()
        .context("Für diese Datei läuft bereits ein Download.")?;
    ensure!(
        !path.exists(),
        "Zieldatei existiert bereits: {}",
        path.display()
    );
    // The OS releases the advisory lock even after a crash. Rename publishes only complete files.
    let part = path.with_extension(format!("{}.part", uuid::Uuid::new_v4()));
    check_cancel(cancel)?;
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    let result=runtime.block_on(async {
        let client=reqwest::Client::builder().user_agent(concat!("TranscriptionRust/",env!("CARGO_PKG_VERSION"))).connect_timeout(Duration::from_secs(20)).read_timeout(Duration::from_secs(30)).timeout(Duration::from_secs(3600)).build()?;
        let mut response=tokio::select! {
            response=client.get(url).send()=>response.context("Download konnte nicht gestartet werden.")?.error_for_status()?,
            ()=wait_cancel(cancel)=>anyhow::bail!("Abgebrochen"),
        };
        let length=response.content_length();let mut count=0u64;
        let mut file=fs::OpenOptions::new().create_new(true).write(true).open(&part)?;
        let mut last=std::time::Instant::now();
        loop {
            let chunk=tokio::select! {
                chunk=response.chunk()=>chunk?,
                ()=wait_cancel(cancel)=>anyhow::bail!("Abgebrochen"),
            };
            let Some(chunk)=chunk else{break;};
            file.write_all(&chunk)?;count+=chunk.len() as u64;
            if last.elapsed()>Duration::from_millis(150){progress(length.map_or(0.,|len|count as f32/len.max(1) as f32));last=std::time::Instant::now();}
        }
        check_cancel(cancel)?;
        ensure!(count>0&&length.is_none_or(|len|count==len),"Unvollständiger Download.");
        file.sync_all()?;drop(file);
        // Reserve without replacing a file created by another application during download.
        let reservation=fs::OpenOptions::new().create_new(true).write(true).open(path)?;drop(reservation);
        if let Err(error)=fs::rename(&part,path){let _=fs::remove_file(path);return Err(error.into());}
        progress(1.);Ok(())
    });
    let _ = fs::remove_file(part);
    result
}

pub fn file_hash(path: &Path, cancel: &AtomicBool) -> Result<String> {
    let mut f = fs::File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buf = vec![0; 1024 * 1024];
    loop {
        crate::audio::check_cancel(cancel)?;
        let n = f.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

pub async fn wait_cancel(cancel: &AtomicBool) {
    while !cancel.load(std::sync::atomic::Ordering::Relaxed) {
        tokio::time::sleep(Duration::from_millis(40)).await;
    }
}
