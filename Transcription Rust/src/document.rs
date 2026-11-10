use anyhow::{Context, Result, bail, ensure};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
};
use uuid::Uuid;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Segment {
    #[serde(default = "new_id")]
    pub id: String,
    pub start: f64,
    pub end: f64,
    pub text: String,
    #[serde(default)]
    pub speaker: Option<String>,
    #[serde(default)]
    pub language: Option<String>,
}
fn new_id() -> String {
    Uuid::new_v4().to_string()
}
impl Segment {
    pub fn new(start: f64, end: f64, text: String) -> Self {
        Self {
            id: new_id(),
            start,
            end,
            text,
            speaker: None,
            language: None,
        }
    }
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Document {
    pub id: String,
    pub source_path: PathBuf,
    pub source_file: String,
    pub language: String,
    pub model: String,
    pub device: String,
    #[serde(default = "engine_name")]
    pub engine: String,
    pub created: String,
    #[serde(default = "default_fps")]
    pub fps_timecode: u32,
    pub timecodes: bool,
    #[serde(default)]
    pub diarization: bool,
    #[serde(default)]
    pub speaker_model: Option<String>,
    #[serde(default)]
    pub speaker_names: BTreeMap<String, String>,
    pub segments: Vec<Segment>,
    #[serde(default)]
    pub outputs: BTreeMap<String, String>,
    #[serde(default)]
    pub podcast: Option<serde_json::Value>,
}
fn engine_name() -> String {
    "candle-whisper-rust".into()
}
fn default_fps() -> u32 {
    25
}
impl Document {
    pub fn new(source: &Path, model: &str, language: &str, device: &str) -> Self {
        Self {
            id: new_id(),
            source_path: source.into(),
            source_file: source
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .into(),
            language: language.into(),
            model: model.into(),
            device: device.into(),
            engine: engine_name(),
            created: Utc::now().to_rfc3339(),
            fps_timecode: 25,
            timecodes: true,
            diarization: false,
            speaker_model: None,
            speaker_names: BTreeMap::new(),
            segments: vec![],
            outputs: BTreeMap::new(),
            podcast: None,
        }
    }
    pub fn validate(&self) -> Result<()> {
        ensure!(
            self.fps_timecode > 0 && self.fps_timecode <= 120,
            "Bildrate muss zwischen 1 und 120 liegen."
        );
        for s in &self.segments {
            ensure!(
                s.start.is_finite() && s.end.is_finite() && s.start >= 0. && s.end >= s.start,
                "Ungültiger Segmentzeitraum."
            );
        }
        Ok(())
    }
    pub fn speaker_label<'a>(&'a self, speaker: &'a str) -> &'a str {
        self.speaker_names
            .get(speaker)
            .map(String::as_str)
            .unwrap_or(speaker)
    }
    pub fn split(&mut self, index: usize, at: f64, char_index: usize) -> Result<()> {
        let s = self.segments.get(index).context("Segment fehlt.")?;
        ensure!(
            at > s.start && at < s.end,
            "Trennpunkt muss im Segment liegen."
        );
        let byte = s
            .text
            .char_indices()
            .nth(char_index)
            .map(|(n, _)| n)
            .unwrap_or(s.text.len());
        let mut right = s.clone();
        right.id = new_id();
        right.start = at;
        right.text = s.text[byte..].trim().into();
        let left = &mut self.segments[index];
        left.end = at;
        left.text = left.text[..byte].trim().into();
        self.segments.insert(index + 1, right);
        Ok(())
    }
    pub fn merge(&mut self, index: usize) -> Result<()> {
        ensure!(
            index + 1 < self.segments.len(),
            "Kein Folgesegment vorhanden."
        );
        let next = self.segments.remove(index + 1);
        let s = &mut self.segments[index];
        s.end = s.end.max(next.end);
        s.text = format!("{} {}", s.text.trim(), next.text.trim());
        Ok(())
    }
    pub fn merge_speakers(&mut self, from: &str, to: &str) {
        for s in &mut self.segments {
            if s.speaker.as_deref() == Some(from) {
                s.speaker = Some(to.into());
            }
        }
        self.speaker_names.remove(from);
    }
    pub fn render(&self, format: Format) -> Result<String> {
        self.validate()?;
        let mut out = String::new();
        match format {
            Format::Markdown => {
                let metadata = serde_json::json!({"created": self.created, "model": self.model, "device": self.device, "source_file": self.source_file, "fps_timecode": self.fps_timecode, "timecodes": self.timecodes, "language": self.language, "engine": self.engine, "diarization": self.diarization, "speaker_model": self.speaker_model, "podcast": self.podcast});
                out.push_str("---\n");
                out.push_str(&serde_yaml::to_string(&metadata)?);
                out.push_str("---\n\n");
                out.push_str(&format!(
                    "# {}\n\n",
                    self.source_file.replace(['\n', '\r'], " ")
                ));
                for s in &self.segments {
                    if self.timecodes {
                        out.push_str(&format!("[{}] ", frame_time(s.start, self.fps_timecode)));
                    }
                    if let Some(speaker) = &s.speaker {
                        out.push_str(&format!("**{}:** ", self.speaker_label(speaker)));
                    }
                    out.push_str(&s.text);
                    out.push_str("\n\n");
                }
            }
            Format::Vtt | Format::Srt => {
                if format == Format::Vtt {
                    out.push_str("WEBVTT\n\n");
                }
                for (i, s) in self.segments.iter().enumerate() {
                    if format == Format::Srt {
                        out.push_str(&format!("{}\n", i + 1));
                    }
                    let separator = if format == Format::Srt { ',' } else { '.' };
                    out.push_str(&format!(
                        "{} --> {}\n",
                        subtitle_time(s.start, separator),
                        subtitle_time(s.end, separator)
                    ));
                    if let Some(speaker) = &s.speaker {
                        out.push_str(&format!("{}: ", self.speaker_label(speaker)));
                    }
                    // A blank line terminates a cue; keep edited paragraphs inside one cue.
                    out.push_str(
                        &s.text
                            .replace("\r\n", "\n")
                            .split('\n')
                            .filter(|l| !l.trim().is_empty())
                            .collect::<Vec<_>>()
                            .join("\n"),
                    );
                    out.push_str("\n\n");
                }
            }
            Format::Text => {
                for s in &self.segments {
                    if let Some(speaker) = &s.speaker {
                        out.push_str(&format!("{}: ", self.speaker_label(speaker)));
                    }
                    out.push_str(&s.text);
                    out.push('\n');
                }
            }
            Format::Csv => {
                let mut w = csv::Writer::from_writer(Vec::new());
                w.write_record(["start", "end", "speaker", "language", "text"])?;
                for s in &self.segments {
                    w.write_record([
                        format!("{:.3}", s.start),
                        format!("{:.3}", s.end),
                        s.speaker
                            .as_deref()
                            .map(|v| self.speaker_label(v))
                            .unwrap_or("")
                            .into(),
                        s.language.clone().unwrap_or_default(),
                        s.text.clone(),
                    ])?;
                }
                out = String::from_utf8(w.into_inner()?)?;
            }
        }
        Ok(out)
    }
    /// Reserve a unique basename before writing; never replace someone else's transcript.
    pub fn export_new(&mut self, dir: &Path, formats: &[Format]) -> Result<()> {
        ensure!(
            !formats.is_empty(),
            "Mindestens ein Ausgabeformat auswählen."
        );
        fs::create_dir_all(dir)?;
        let stem = safe_name(
            Path::new(&self.source_file)
                .file_stem()
                .unwrap_or_default()
                .to_string_lossy()
                .as_ref(),
        );
        for suffix in 0..100_000 {
            let base = if suffix == 0 {
                stem.clone()
            } else {
                format!("{stem}_{suffix}")
            };
            let paths: Vec<_> = formats
                .iter()
                .map(|f| (f, dir.join(format!("{base}.{}", f.extension()))))
                .collect();
            let lock = dir.join(format!(".{base}.transcription-lock"));
            let reservation = match fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&lock)
            {
                Ok(f) => f,
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(e) => return Err(e.into()),
            };
            if paths.iter().any(|(_, p)| p.exists()) {
                drop(reservation);
                fs::remove_file(&lock)?;
                continue;
            }
            let mut written = vec![];
            let result: Result<()> = (|| {
                use std::io::Write;
                for (format, path) in &paths {
                    let content = self.render(**format)?;
                    let mut file = fs::OpenOptions::new()
                        .create_new(true)
                        .write(true)
                        .open(path)?;
                    written.push(path.clone());
                    file.write_all(content.as_bytes())?;
                    file.sync_all()?;
                }
                Ok(())
            })();
            drop(reservation);
            let _ = fs::remove_file(&lock);
            if let Err(e) = result {
                for p in written {
                    let _ = fs::remove_file(p);
                }
                return Err(e);
            }
            self.outputs = paths
                .into_iter()
                .map(|(f, p)| (f.extension().into(), p.to_string_lossy().into()))
                .collect();
            return Ok(());
        }
        bail!("Kein freier Dateiname gefunden.")
    }
}
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum Format {
    Markdown,
    Vtt,
    Srt,
    Text,
    Csv,
}
impl Format {
    pub const ALL: [Self; 5] = [Self::Markdown, Self::Vtt, Self::Srt, Self::Text, Self::Csv];
    pub fn extension(self) -> &'static str {
        match self {
            Self::Markdown => "md",
            Self::Vtt => "vtt",
            Self::Srt => "srt",
            Self::Text => "txt",
            Self::Csv => "csv",
        }
    }
}
pub fn safe_name(name: &str) -> String {
    let s: String = name
        .chars()
        .map(|c| {
            if c.is_control() || "/\\:*?\"<>|".contains(c) {
                '_'
            } else {
                c
            }
        })
        .take(120)
        .collect();
    let s = s.trim_matches([' ', '.']);
    if s.is_empty() {
        "Transkript".into()
    } else {
        s.into()
    }
}
pub fn subtitle_time(seconds: f64, sep: char) -> String {
    let ms = (seconds.max(0.) * 1000.).round() as u64;
    format!(
        "{:02}:{:02}:{:02}{sep}{:03}",
        ms / 3_600_000,
        ms / 60_000 % 60,
        ms / 1000 % 60,
        ms % 1000
    )
}
pub fn frame_time(seconds: f64, fps: u32) -> String {
    let fps = fps.max(1) as u64;
    let n = (seconds.max(0.) * fps as f64).round() as u64;
    format!(
        "{:02}:{:02}:{:02}:{:02}",
        n / fps / 3600,
        n / fps / 60 % 60,
        n / fps % 60,
        n % fps
    )
}
fn parse_time(raw: &str) -> Result<f64> {
    let values = raw
        .replace(',', ".")
        .split(':')
        .map(str::parse::<f64>)
        .collect::<std::result::Result<Vec<_>, _>>()?;
    ensure!(
        values.len() == 2 || values.len() == 3,
        "Ungültige Untertitelzeit: {raw}"
    );
    ensure!(
        values.iter().all(|v| v.is_finite() && *v >= 0.)
            && values[values.len() - 1] < 60.
            && values[values.len() - 2] < 60.,
        "Ungültige Untertitelzeit: {raw}"
    );
    Ok(values.into_iter().fold(0., |a, v| a * 60. + v))
}
pub fn import_subtitles(text: &str, source: &Path) -> Result<Document> {
    let normalized = text
        .trim_start_matches('\u{feff}')
        .replace("\r\n", "\n")
        .replace('\r', "\n");
    let mut doc = Document::new(source, "Untertitelimport", "auto", "cpu");
    for block in normalized.split("\n\n") {
        let lines: Vec<_> = block.lines().collect();
        if lines
            .first()
            .is_some_and(|s| s.starts_with("NOTE") || *s == "STYLE" || *s == "REGION")
        {
            continue;
        }
        let Some(i) = lines.iter().position(|l| l.contains("-->")) else {
            continue;
        };
        let (start, end) = lines[i]
            .split_once("-->")
            .context("Ungültiger Zeitbereich.")?;
        let start = parse_time(start.trim())?;
        let end = parse_time(end.split_whitespace().next().context("Endzeit fehlt.")?)?;
        ensure!(end >= start, "Endzeit liegt vor Beginn.");
        let raw = lines[i + 1..].join("\n");
        let speaker = raw
            .strip_prefix("<v ")
            .and_then(|s| s.split_once('>'))
            .map(|(s, _)| s.to_string());
        let mut stripped = String::new();
        let mut in_tag = false;
        for c in raw.chars() {
            match c {
                '<' => in_tag = true,
                '>' => in_tag = false,
                _ if !in_tag => stripped.push(c),
                _ => {}
            }
        }
        let mut s = Segment::new(
            start,
            end,
            stripped
                .replace("&lt;", "<")
                .replace("&gt;", ">")
                .replace("&nbsp;", " ")
                .replace("&amp;", "&")
                .trim()
                .into(),
        );
        s.speaker = speaker;
        doc.segments.push(s);
    }
    ensure!(
        !doc.segments.is_empty(),
        "Keine VTT-/SRT-Segmente gefunden."
    );
    doc.validate()?;
    Ok(doc)
}
