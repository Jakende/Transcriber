//! Bounded, read-only discovery. Models are referenced in place, never copied on launch.
use crate::{
    models::{self, ModelSize},
    storage,
};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeSet,
    fs,
    io::Read,
    path::{Path, PathBuf},
};
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum WeightFormat {
    Safetensors,
    Ggml,
    OpenAiPytorch,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct InstalledModel {
    pub model: ModelSize,
    pub weights: PathBuf,
    pub format: WeightFormat,
}
impl InstalledModel {
    pub fn label(&self) -> String {
        format!("{:?}: {}", self.format, self.weights.display())
    }
}
fn dimensions(size: ModelSize) -> (u32, u32, u32, u32) {
    match size {
        ModelSize::Tiny => (384, 4, 4, 80),
        ModelSize::Small => (768, 12, 12, 80),
        ModelSize::Medium => (1024, 24, 24, 80),
        ModelSize::Turbo => (1280, 32, 4, 128),
    }
}
pub fn ggml_matches(path: &Path, size: ModelSize) -> bool {
    let Ok(mut f) = fs::File::open(path) else {
        return false;
    };
    let mut bytes = [0; 48];
    if f.read_exact(&mut bytes).is_err() {
        return false;
    }
    let v = bytes
        .chunks_exact(4)
        .map(|b| u32::from_le_bytes(b.try_into().unwrap()))
        .collect::<Vec<_>>();
    let (state, enc, dec, mels) = dimensions(size);
    v[0] == 0x67676d6c
        && v[3] == state
        && v[5] == enc
        && v[9] == dec
        && v[10] == mels
        && v[1]
            == if size == ModelSize::Turbo {
                51866
            } else {
                51865
            }
}
/// Hugging Face snapshots may use symlinks, or text link placeholders on ExFAT.
pub fn resolve_hub_link(path: &Path) -> Option<PathBuf> {
    if !path.is_file() {
        return None;
    }
    if fs::metadata(path).ok()?.len() < 256
        && let Ok(text) = fs::read_to_string(path)
    {
        let text = text.trim();
        if text.starts_with("../../blobs/") && !text.contains('\n') {
            let target = path.parent()?.join(text).canonicalize().ok()?;
            if target.is_file() {
                return Some(target);
            }
        }
    }
    Some(path.to_path_buf())
}
pub fn standard_roots() -> Vec<PathBuf> {
    let mut roots = vec![models::model_root()];
    if let Some(home) = directories::BaseDirs::new().map(|d| d.home_dir().to_path_buf()) {
        for p in [
            ".cache/huggingface/hub",
            ".cache/whisper",
            ".cache/whisper.cpp",
            "Library/Caches/whisper",
            "Library/Caches/whisper.cpp",
            "Library/Caches/huggingface/hub",
            "Library/Application Support/whisper",
            "Library/Application Support/whisper.cpp",
            "Applications/Transcription macOS.app/Contents/Resources/models",
        ] {
            roots.push(home.join(p));
        }
    }
    roots.extend(
        [
            "/Applications/Transcription macOS.app/Contents/Resources/models",
            "/Applications/Transcription Rust.app/Contents/Resources/models",
            "/opt/homebrew/share/whisper-cpp/models",
            "/usr/local/share/whisper-cpp/models",
        ]
        .map(PathBuf::from),
    );
    for key in [
        "HF_HOME",
        "HUGGINGFACE_HUB_CACHE",
        "HF_HUB_CACHE",
        "WHISPER_MODEL_DIR",
        "WHISPER_CPP_MODEL_DIR",
    ] {
        if let Some(value) = std::env::var_os(key) {
            let p = PathBuf::from(value);
            roots.push(p.join("hub"));
            roots.push(p);
        }
    }
    if let Some(paths) = std::env::var_os("TRANSCRIPTION_MODEL_SEARCH_PATHS") {
        roots.extend(std::env::split_paths(&paths));
    }
    if let Ok(settings) =
        storage::load::<storage::Settings>(&storage::data_dir().join("settings.json"))
    {
        roots.extend(settings.model_search_dirs);
    }
    if let Ok(cwd) = std::env::current_dir() {
        for ancestor in cwd.ancestors().take(4) {
            roots.push(ancestor.join("Transcription macOS App/.bundle-cache/resources/models"));
            roots.push(ancestor.join(
                "Transcription macOS App/dist/Transcription macOS.app/Contents/Resources/models",
            ));
            roots.push(ancestor.join("Transcription macOS App/.bundle-cache/huggingface/hub"));
        }
    }
    if let Ok(exe) = std::env::current_exe()
        && let Some(contents) = exe.parent().and_then(Path::parent)
    {
        roots.insert(0, contents.join("Resources/models"));
    }
    roots
}
pub fn discover_in(size: ModelSize, roots: &[PathBuf]) -> Vec<InstalledModel> {
    let mut found = vec![];
    let mut seen = BTreeSet::new();
    for root in roots {
        let mut folders = vec![
            root.clone(),
            root.join(size.name()),
            root.join(format!("whisper-{}", size.name())),
        ];
        let hub = root
            .join(format!("models--openai--whisper-{}", size.name()))
            .join("snapshots");
        if let Ok(entries) = fs::read_dir(hub) {
            folders.extend(entries.flatten().take(100).map(|e| e.path()));
        }
        for folder in folders {
            if let Some(weights) = resolve_hub_link(&folder.join("model.safetensors"))
                && safetensors_matches(&weights, size)
            {
                add(
                    &mut found,
                    &mut seen,
                    size,
                    weights,
                    WeightFormat::Safetensors,
                );
            }
            for filename in [
                format!("{}.pt", size.name()),
                format!("whisper-{}.pt", size.name()),
            ] {
                let weights = folder.join(filename);
                if weights.is_file() {
                    let mut magic = [0; 4];
                    if fs::File::open(&weights)
                        .and_then(|mut f| f.read_exact(&mut magic))
                        .is_ok()
                        && magic[..2] == *b"PK"
                    {
                        add(
                            &mut found,
                            &mut seen,
                            size,
                            weights,
                            WeightFormat::OpenAiPytorch,
                        );
                    }
                }
            }
            if let Ok(entries) = fs::read_dir(&folder) {
                for entry in entries.flatten().take(500) {
                    let path = entry.path();
                    let name = entry.file_name().to_string_lossy().into_owned();
                    if name.starts_with(&format!("ggml-{}", size.name()))
                        && name.ends_with(".bin")
                        && ggml_matches(&path, size)
                    {
                        add(&mut found, &mut seen, size, path, WeightFormat::Ggml);
                    }
                }
            }
        }
    }
    found
}
fn add(
    found: &mut Vec<InstalledModel>,
    seen: &mut BTreeSet<PathBuf>,
    model: ModelSize,
    path: PathBuf,
    format: WeightFormat,
) {
    let path = path.canonicalize().unwrap_or(path);
    if seen.insert(path.clone()) {
        found.push(InstalledModel {
            model,
            weights: path,
            format,
        });
    }
}
pub fn discover(size: ModelSize) -> Vec<InstalledModel> {
    discover_in(size, &standard_roots())
}
pub fn safetensors_matches(path: &Path, size: ModelSize) -> bool {
    let Ok(mut file) = fs::File::open(path) else {
        return false;
    };
    let mut length = [0; 8];
    if file.read_exact(&mut length).is_err() {
        return false;
    }
    let length = u64::from_le_bytes(length);
    if !(2..=20_000_000).contains(&length) {
        return false;
    }
    let mut header = vec![0; length as usize];
    if file.read_exact(&mut header).is_err() {
        return false;
    }
    let Ok(header) = serde_json::from_slice::<serde_json::Value>(&header) else {
        return false;
    };
    let (state, enc, dec, _) = dimensions(size);
    let shape = &header["model.encoder.conv1.weight"]["shape"];
    shape[0].as_u64() == Some(state as u64)
        && header
            .get(format!("model.encoder.layers.{}.fc1.weight", enc - 1))
            .is_some()
        && header
            .get(format!("model.decoder.layers.{}.fc1.weight", dec - 1))
            .is_some()
}
pub fn existing_dependency(name: &str) -> Option<PathBuf> {
    std::env::var_os("PATH").and_then(|paths| {
        std::env::split_paths(&paths)
            .map(|p| p.join(name))
            .find(|p| p.is_file())
    })
}
