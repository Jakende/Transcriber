mod design;
mod ui;
use anyhow::{Result, ensure};
use clap::{Parser, Subcommand};
use std::{
    path::PathBuf,
    sync::{Arc, atomic::AtomicBool},
};
use transcription_rust::{
    audio,
    document::{self, Format},
    engine::Engine,
    models::{self, ModelSize},
    storage::{self, Library, Settings},
};

#[derive(Parser)]
#[command(
    version,
    about = "Lokale Transkription in Rust. Ohne Unterbefehl öffnet sich die macOS-App."
)]
struct Args {
    #[command(subcommand)]
    command: Option<Command>,
}
#[derive(Subcommand)]
enum Command {
    /// Modelle ausdrücklich in ein portables Offline-Bundle kopieren.
    BundleModel {
        #[arg(value_enum)]
        model: ModelSize,
        output: PathBuf,
    },
    /// Nur Konfigurationen und Tokenizer für einen Offline-App-Build vorbereiten.
    PrepareMetadata,
    /// Vorhandene Modelle ohne Download suchen.
    DiscoverModels,
    /// Vorhandene F16/F32-Whisper-GGML-Datei einmalig in Rust migrieren.
    ImportGgml {
        file: PathBuf,
        #[arg(value_enum)]
        model: ModelSize,
    },
    /// Modell für die spätere Offline-Nutzung herunterladen und prüfen.
    DownloadModel {
        #[arg(value_enum, default_value = "small")]
        model: ModelSize,
        #[arg(long)]
        speakers: bool,
    },
    /// Eine oder mehrere Dateien lokal transkribieren.
    Transcribe {
        #[arg(required = true)]
        files: Vec<PathBuf>,
        #[arg(long, value_enum, default_value = "small")]
        model: ModelSize,
        #[arg(long,default_value="auto",value_parser=["auto","de","en"])]
        language: String,
        #[arg(long)]
        cpu: bool,
        #[arg(long)]
        output: Option<PathBuf>,
        #[arg(long, default_value = "md")]
        formats: String,
        #[arg(long)]
        no_timecodes: bool,
        #[arg(long,default_value_t=25,value_parser=clap::value_parser!(u32).range(1..=120))]
        fps: u32,
        #[arg(long)]
        diarize: bool,
        #[arg(long,default_value_t=0,value_parser=clap::value_parser!(u8).range(0..=10))]
        speakers: u8,
        #[arg(long)]
        glossary: Option<PathBuf>,
        #[arg(long, default_value="auto",value_parser=["auto","2-2","2-4","4-6","5-8","6-10"])]
        speaker_range: String,
        #[arg(long, default_value_t = 0.60)]
        separation: f32,
    },
    /// VTT/SRT oder bisherige Swift-JSON-Dokumente importieren und exportieren.
    Import {
        file: PathBuf,
        #[arg(long)]
        audio: Option<PathBuf>,
        #[arg(long)]
        output: PathBuf,
        #[arg(long, default_value = "md,vtt,srt,txt,csv")]
        formats: String,
    },
    /// Erste unterstützte Audiospur als WAV extrahieren.
    Extract { file: PathBuf, output: PathBuf },
    /// Pfade, Modelle und verfügbare Rechengeräte prüfen.
    Doctor,
    /// Personen, Organisationen und Orte lokal analysieren (einmalig ca. 539 MB Modell).
    AnalyzeTerms {
        file: PathBuf,
        #[arg(long)]
        cpu: bool,
    },
    /// Rust-Sprecherinferenz prüfen und Stimmabdrücke als JSON ausgeben.
    SpeakerEmbedding {
        file: PathBuf,
        #[arg(long)]
        cpu: bool,
    },
}
fn formats(raw: &str) -> Result<Vec<Format>> {
    raw.split(',')
        .map(|s| {
            Format::ALL
                .into_iter()
                .find(|f| f.extension() == s.trim())
                .ok_or_else(|| anyhow::anyhow!("Unbekanntes Ausgabeformat: {s}"))
        })
        .collect()
}
fn main() -> Result<()> {
    let args = Args::parse();
    let cancel = Arc::new(AtomicBool::new(false));
    let mut progress = |p: f32, message: String| eprintln!("{:3.0}% {message}", p * 100.);
    match args.command {
        Some(Command::AnalyzeTerms { file, cpu }) => {
            let text = std::fs::read_to_string(file)?;
            println!(
                "{}",
                serde_json::to_string_pretty(&transcription_rust::terms::analyze(
                    &text,
                    cpu,
                    &cancel,
                    &mut progress
                )?)?
            );
            Ok(())
        }
        None => ui::run().map_err(|e| anyhow::anyhow!(e.to_string())),
        Some(Command::BundleModel { model, output }) => {
            let assets = models::resolve_model(model, &cancel, &mut progress)?;
            let target = output.join(model.name());
            std::fs::create_dir_all(&target)?;
            let name = match assets.format {
                transcription_rust::discovery::WeightFormat::Safetensors => {
                    "model.safetensors".into()
                }
                transcription_rust::discovery::WeightFormat::Ggml => {
                    format!("ggml-{}.bin", model.name())
                }
                transcription_rust::discovery::WeightFormat::OpenAiPytorch => {
                    format!("{}.pt", model.name())
                }
            };
            std::fs::copy(&assets.weights, target.join(name))?;
            for name in ["config.json", "tokenizer.json"] {
                std::fs::copy(assets.metadata.join(name), target.join(name))?;
            }
            if let Some(parent) = assets.weights.parent()
                && parent.join("import.json").exists()
            {
                std::fs::copy(parent.join("import.json"), target.join("import.json"))?;
            }
            let speaker = transcription_rust::speakers::ensure_weights(&cancel, &mut progress)?;
            std::fs::create_dir_all(output.join("ecapa"))?;
            std::fs::copy(speaker, output.join("ecapa/embedding_model.ckpt"))?;
            Ok(())
        }
        Some(Command::PrepareMetadata) => {
            for size in ModelSize::ALL {
                println!(
                    "{}",
                    models::ensure_metadata(size, None, &cancel, &mut progress)?.display()
                );
            }
            Ok(())
        }
        Some(Command::DiscoverModels) => {
            let found = ModelSize::ALL
                .into_iter()
                .flat_map(transcription_rust::discovery::discover)
                .collect::<Vec<_>>();
            println!("{}", serde_json::to_string_pretty(&found)?);
            Ok(())
        }
        Some(Command::ImportGgml { file, model }) => {
            println!(
                "{}",
                transcription_rust::ggml::import(&file, model, &cancel, &mut progress)?.display()
            );
            Ok(())
        }
        Some(Command::DownloadModel { model, speakers }) => {
            println!(
                "{}",
                models::ensure_model(model, &cancel, &mut progress)?.display()
            );
            if speakers {
                println!(
                    "{}",
                    transcription_rust::speakers::ensure_weights(&cancel, &mut progress)?.display()
                );
            }
            Ok(())
        }
        Some(Command::Transcribe {
            files,
            model,
            language,
            cpu,
            output,
            formats: raw,
            no_timecodes,
            fps,
            diarize,
            speakers,
            glossary,
            speaker_range,
            separation,
        }) => {
            let glossary = glossary
                .map(std::fs::read_to_string)
                .transpose()?
                .map(|s| {
                    s.lines()
                        .map(str::trim)
                        .filter(|s| !s.is_empty())
                        .map(String::from)
                        .collect()
                })
                .unwrap_or_default();
            let settings = Settings {
                model,
                language,
                cpu,
                output_dir: output,
                formats: formats(&raw)?,
                timecodes: !no_timecodes,
                fps,
                diarization: diarize,
                speakers: speakers as usize,
                speaker_range,
                separation,
                glossary,
                ..Default::default()
            };
            let mut engine = Engine::load(&settings, &cancel, &mut progress)?;
            let mut library = Library::open()?;
            let mut failed = 0;
            for file in files {
                let result = (|| -> Result<()> {
                    let mut doc = engine.transcribe(&file, &settings, &cancel, &mut progress)?;
                    if let Ok(episode) = storage::load::<transcription_rust::media::Episode>(
                        &file.with_extension(format!(
                            "{}.podcast.json",
                            file.extension().unwrap_or_default().to_string_lossy()
                        )),
                    ) {
                        doc.podcast = Some(serde_json::to_value(episode)?);
                    }
                    let dir = settings.output_dir.clone().unwrap_or_else(|| {
                        file.parent().unwrap_or(std::path::Path::new(".")).into()
                    });
                    doc.export_new(&dir, &settings.formats)?;
                    library.store(&doc)?;
                    println!("{}", serde_json::to_string(&doc)?);
                    Ok(())
                })();
                if let Err(e) = result {
                    failed += 1;
                    eprintln!("{}: {e:#}", file.display());
                }
            }
            ensure!(failed == 0, "{failed} Datei(en) fehlgeschlagen.");
            Ok(())
        }
        Some(Command::Import {
            file,
            audio,
            output,
            formats: raw,
        }) => {
            let mut doc = if file.extension().is_some_and(|e| e == "json") {
                storage::load(&file)?
            } else {
                document::import_subtitles(
                    &std::fs::read_to_string(&file)?,
                    audio.as_deref().unwrap_or(&file),
                )?
            };
            doc.export_new(&output, &formats(&raw)?)?;
            Library::open()?.store(&doc)?;
            println!("{}", serde_json::to_string_pretty(&doc)?);
            Ok(())
        }
        Some(Command::Extract { file, output }) => {
            ensure!(!output.exists(), "Zieldatei existiert bereits.");
            audio::write_wav(&output, &audio::decode(&file, &cancel)?)
        }
        Some(Command::Doctor) => {
            println!(
                "Transcription Rust {}\nmacOS {}\nDaten: {}\nModelle: {}\nGerät: {:?}",
                env!("CARGO_PKG_VERSION"),
                std::env::consts::ARCH,
                storage::data_dir().display(),
                models::model_root().display(),
                transcription_rust::engine::select_device(false)?
            );
            for model in ModelSize::ALL {
                let found = transcription_rust::discovery::discover(model);
                if found.is_empty() {
                    println!("{}: nicht gefunden", model.name());
                }
                for item in found {
                    println!("{}: {}", model.name(), item.label());
                }
            }
            for name in ["python3", "ffmpeg", "deno", "yt-dlp", "whisper-cli"] {
                println!(
                    "{name}: {} · für die Rust-App nicht erforderlich",
                    transcription_rust::discovery::existing_dependency(name)
                        .map(|p| p.display().to_string())
                        .unwrap_or_else(|| "nicht gefunden".into())
                );
            }
            Ok(())
        }
        Some(Command::SpeakerEmbedding { file, cpu }) => {
            let engine =
                transcription_rust::speakers::SpeakerEngine::load(cpu, &cancel, &mut progress)?;
            let samples = audio::decode(&file, &cancel)?;
            let vector =
                engine.embedding(&samples[..samples.len().min(3 * audio::RATE as usize)])?;
            println!("{}", serde_json::to_string(&vector)?);
            Ok(())
        }
    }
}
