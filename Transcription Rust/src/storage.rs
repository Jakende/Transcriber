use crate::{
    document::{Document, Format},
    models::ModelSize,
};
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub model: ModelSize,
    pub model_search_dirs: Vec<PathBuf>,
    pub language: String,
    pub cpu: bool,
    pub timecodes: bool,
    pub fps: u32,
    pub formats: Vec<Format>,
    pub output_dir: Option<PathBuf>,
    pub diarization: bool,
    pub speakers: usize,
    pub glossary: Vec<String>,
    pub queue: Vec<PathBuf>,
    pub feeds: Vec<String>,
}
impl Default for Settings {
    fn default() -> Self {
        Self {
            model: ModelSize::Small,
            model_search_dirs: vec![],
            language: "auto".into(),
            cpu: false,
            timecodes: true,
            fps: 25,
            formats: vec![Format::Markdown],
            output_dir: None,
            diarization: false,
            speakers: 0,
            glossary: vec![],
            queue: vec![],
            feeds: vec![],
        }
    }
}
pub fn data_dir() -> PathBuf {
    if let Some(p) = std::env::var_os("TRANSCRIPTION_DATA_DIR") {
        return p.into();
    }
    directories::ProjectDirs::from("de", "Jakende", "Transcription Rust")
        .expect("Benutzerverzeichnis fehlt")
        .data_dir()
        .into()
}
pub fn load<T: DeserializeOwned>(path: &Path) -> Result<T> {
    Ok(serde_json::from_slice(
        &fs::read(path).with_context(|| format!("Lesen: {}", path.display()))?,
    )?)
}
pub fn atomic_write(path: &Path, bytes: &[u8]) -> Result<()> {
    let parent = path.parent().context("Übergeordnetes Verzeichnis fehlt.")?;
    fs::create_dir_all(parent)?;
    let temporary = parent.join(format!(".{}.tmp", uuid::Uuid::new_v4()));
    let result = (|| -> Result<()> {
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)?;
        file.write_all(bytes)?;
        file.sync_all()?;
        fs::rename(&temporary, path)?;
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(temporary);
    }
    result
}
pub fn save<T: Serialize>(path: &Path, value: &T) -> Result<()> {
    atomic_write(path, &serde_json::to_vec_pretty(value)?)
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Record {
    pub id: String,
    pub title: String,
    pub path: PathBuf,
    pub archived: bool,
    pub source: PathBuf,
}
#[derive(Default, Serialize, Deserialize)]
pub struct Library {
    pub records: Vec<Record>,
}
impl Library {
    pub fn open() -> Result<Self> {
        let p = data_dir().join("library.json");
        if p.exists() {
            load(&p)
        } else {
            Ok(Self::default())
        }
    }
    pub fn persist(&self) -> Result<()> {
        save(&data_dir().join("library.json"), self)
    }
    pub fn store(&mut self, doc: &Document) -> Result<()> {
        doc.validate()?;
        let path = data_dir()
            .join("documents")
            .join(format!("{}.json", doc.id));
        if path.exists() {
            let backups = data_dir().join("backups").join(&doc.id);
            fs::create_dir_all(&backups)?;
            let backup = backups.join(format!(
                "{}.json",
                chrono::Utc::now().format("%Y%m%dT%H%M%S%.9f")
            ));
            fs::copy(&path, backup)?;
            let mut entries = fs::read_dir(&backups)?
                .filter_map(|e| e.ok())
                .map(|e| e.path())
                .collect::<Vec<_>>();
            entries.sort();
            let remove_count = entries.len().saturating_sub(20);
            for old in entries.into_iter().take(remove_count) {
                fs::remove_file(old)?;
            }
        }
        save(&path, doc)?;
        if let Some(record) = self.records.iter_mut().find(|r| r.id == doc.id) {
            record.title = doc.source_file.clone();
            record.source = doc.source_path.clone();
        } else {
            self.records.insert(
                0,
                Record {
                    id: doc.id.clone(),
                    title: doc.source_file.clone(),
                    path,
                    archived: false,
                    source: doc.source_path.clone(),
                },
            );
        }
        self.persist()
    }
    pub fn trash(&mut self, index: usize) -> Result<()> {
        let record = self.records.get(index).context("Eintrag fehlt.")?;
        // Keep both internal document and exports recoverable; source media are untouched.
        let trash = data_dir().join("Papierkorb");
        fs::create_dir_all(&trash)?;
        if record.path.exists() {
            fs::rename(&record.path, trash.join(format!("{}.json", record.id)))?;
        }
        self.records.remove(index);
        self.persist()
    }
}
#[derive(Default)]
pub struct History {
    undo: Vec<Document>,
    redo: Vec<Document>,
}
impl History {
    pub fn record(&mut self, doc: &Document) {
        self.undo.push(doc.clone());
        if self.undo.len() > 100 {
            self.undo.remove(0);
        }
        self.redo.clear();
    }
    pub fn undo(&mut self, doc: &mut Document) {
        if let Some(previous) = self.undo.pop() {
            self.redo.push(doc.clone());
            *doc = previous;
        }
    }
    pub fn redo(&mut self, doc: &mut Document) {
        if let Some(next) = self.redo.pop() {
            self.undo.push(doc.clone());
            *doc = next;
        }
    }
}
