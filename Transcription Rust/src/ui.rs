use anyhow::{Context, Result};
use eframe::egui;
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    time::{Duration, Instant},
};
use transcription_rust::{
    audio,
    document::{self, Document, Format},
    engine::{self, Engine},
    media::{self, Episode, Podcast},
    models::{self, ModelSize},
    storage::{self, History, Library, Settings},
};

#[derive(Clone, Copy, PartialEq)]
enum Tab {
    Transcribe,
    Library,
    Glossary,
    Media,
    Diagnostics,
}
enum Event {
    Progress(f32, String),
    Log(String),
    Result(Box<Document>),
    Done,
    Feed(Vec<Episode>),
    Podcasts(Vec<Podcast>),
    Downloaded(PathBuf),
    Discovered(Vec<transcription_rust::discovery::InstalledModel>),
    Playback(Vec<f32>, String),
    Terms(String, Vec<transcription_rust::terms::Candidate>),
}
struct Editor {
    doc: Document,
    history: History,
    search: String,
    selected: usize,
    dirty: bool,
    last_edit: Instant,
    open: bool,
    merge_target: String,
    replacements: BTreeMap<String, String>,
    speed: f32,
    looping: bool,
}
impl Editor {
    fn new(doc: Document) -> Self {
        Self {
            doc,
            history: History::default(),
            search: String::new(),
            selected: 0,
            dirty: false,
            last_edit: Instant::now(),
            open: true,
            merge_target: String::new(),
            replacements: BTreeMap::new(),
            speed: 1.,
            looping: false,
        }
    }
}
struct Player {
    _stream: rodio::OutputStream,
    sink: rodio::Sink,
    source: String,
    start: f64,
    end: f64,
    looping: bool,
    samples: Arc<Vec<f32>>,
}
impl Player {
    fn new(samples: Vec<f32>, source: String) -> Result<Self> {
        let stream = rodio::OutputStreamBuilder::open_default_stream()?;
        let sink = rodio::Sink::connect_new(stream.mixer());
        sink.pause();
        Ok(Self {
            _stream: stream,
            sink,
            source,
            start: 0.,
            end: 0.,
            looping: false,
            samples: Arc::new(samples),
        })
    }
    fn play(&mut self, start: f64, end: f64, speed: f32, looping: bool) {
        use rodio::Source;
        self.sink.stop();
        self.sink = rodio::Sink::connect_new(self._stream.mixer());
        self.sink.set_speed(speed);
        self.start = start;
        self.end = end;
        self.looping = looping;
        let a = ((start * audio::RATE as f64) as usize).min(self.samples.len());
        let b = ((end * audio::RATE as f64) as usize).min(self.samples.len());
        if b <= a {
            return;
        }
        let samples =
            rodio::buffer::SamplesBuffer::new(1, audio::RATE, self.samples[a..b].to_vec());
        if looping {
            self.sink.append(samples.repeat_infinite());
        } else {
            self.sink.append(samples);
        }
        self.sink.play();
    }
}
struct App {
    settings: Settings,
    library: Library,
    tab: Tab,
    log: Vec<String>,
    progress: f32,
    status: String,
    busy: bool,
    cancel: Arc<AtomicBool>,
    tx: mpsc::Sender<Event>,
    rx: mpsc::Receiver<Event>,
    editors: BTreeMap<String, Editor>,
    player: Option<Player>,
    search: String,
    show_archive: bool,
    term: String,
    url: String,
    podcast_query: String,
    podcast_key: String,
    podcast_secret: String,
    episodes: Vec<Episode>,
    selected_episodes: std::collections::BTreeSet<String>,
    podcasts: Vec<Podcast>,
    download_video: bool,
    auto_start: bool,
    dirty_settings: bool,
    last_save: Instant,
    confirm_trash: Option<usize>,
    close_pending: bool,
    exit_after_cancel: bool,
    download_ready: bool,
    scan_pending: bool,
    installed_models: Vec<transcription_rust::discovery::InstalledModel>,
}
pub fn run() -> eframe::Result {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Transcription Rust")
            .with_inner_size([1100., 760.])
            .with_min_inner_size([880., 620.]),
        ..Default::default()
    };
    eframe::run_native(
        "Transcription Rust",
        options,
        Box::new(|cc| {
            crate::design::configure(&cc.egui_ctx);
            Ok(Box::new(App::new()?))
        }),
    )
}
impl App {
    fn new() -> Result<Self> {
        let path = storage::data_dir().join("settings.json");
        let settings = if path.exists() {
            storage::load(&path)
                .context("Einstellungen unlesbar; die Originaldatei wird erhalten.")?
        } else {
            Settings::default()
        };
        let (tx, rx) = mpsc::channel();
        Ok(Self {
            settings,
            library: Library::open()?,
            tab: Tab::Transcribe,
            log: vec!["Bereit. Audio bleibt auf diesem Mac.".into()],
            progress: 0.,
            status: "Bereit".into(),
            busy: false,
            cancel: Arc::new(AtomicBool::new(false)),
            tx,
            rx,
            editors: BTreeMap::new(),
            player: None,
            search: String::new(),
            show_archive: false,
            term: String::new(),
            url: String::new(),
            podcast_query: String::new(),
            podcast_key: String::new(),
            podcast_secret: String::new(),
            episodes: vec![],
            selected_episodes: Default::default(),
            podcasts: vec![],
            download_video: false,
            auto_start: false,
            dirty_settings: false,
            last_save: Instant::now(),
            confirm_trash: None,
            close_pending: false,
            exit_after_cancel: false,
            download_ready: false,
            scan_pending: true,
            installed_models: vec![],
        })
    }
    fn log(&mut self, message: impl Into<String>) {
        self.log.push(format!(
            "{}  {}",
            chrono::Local::now().format("%H:%M:%S"),
            message.into()
        ));
        if self.log.len() > 2000 {
            self.log.drain(..1000);
        }
    }
    fn add(&mut self, paths: Vec<PathBuf>) {
        for path in paths {
            if path.is_file() && !self.settings.queue.contains(&path) {
                self.settings.queue.push(path);
            }
        }
        self.dirty_settings = true;
    }
    fn task(
        &mut self,
        ctx: &egui::Context,
        work: impl FnOnce(mpsc::Sender<Event>, Arc<AtomicBool>) -> Result<()> + Send + 'static,
    ) {
        if self.busy {
            return;
        }
        self.busy = true;
        self.cancel = Arc::new(AtomicBool::new(false));
        let cancel = self.cancel.clone();
        let tx = self.tx.clone();
        let ctx = ctx.clone();
        self.progress = 0.;
        std::thread::spawn(move || {
            let result =
                std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| work(tx.clone(), cancel)));
            match result {
                Ok(Err(e)) => {
                    let _ = tx.send(Event::Log(format!("Fehler: {e:#}")));
                }
                Err(_) => {
                    let _ = tx.send(Event::Log(
                        "Hintergrundaufgabe fehlgeschlagen. Die App bleibt geöffnet.".into(),
                    ));
                }
                _ => {}
            }
            let _ = tx.send(Event::Done);
            ctx.request_repaint();
        });
    }
    fn start(&mut self, ctx: &egui::Context) {
        if self.settings.queue.is_empty() || self.settings.formats.is_empty() {
            self.log("Bitte Dateien und mindestens ein Ausgabeformat auswählen.");
            return;
        }
        let settings = self.settings.clone();
        self.task(ctx, move |tx, cancel| {
            let mut report = |p, message| {
                let _ = tx.send(Event::Progress(p, message));
            };
            let mut engine = Engine::load(&settings, &cancel, &mut report)?;
            let count = settings.queue.len();
            for (i, path) in settings.queue.iter().enumerate() {
                audio::check_cancel(&cancel)?;
                let mut report = |p: f32, message| {
                    let _ = tx.send(Event::Progress((i as f32 + p) / count as f32, message));
                };
                let result = (|| -> Result<Document> {
                    let mut doc = engine.transcribe(path, &settings, &cancel, &mut report)?;
                    if let Ok(episode) = storage::load::<Episode>(&path.with_extension(format!(
                        "{}.podcast.json",
                        path.extension().unwrap_or_default().to_string_lossy()
                    ))) {
                        doc.podcast = Some(serde_json::to_value(episode)?);
                    }
                    let dir = settings
                        .output_dir
                        .as_deref()
                        .unwrap_or_else(|| path.parent().unwrap_or(Path::new(".")));
                    doc.export_new(dir, &settings.formats)?;
                    Ok(doc)
                })();
                match result {
                    Ok(doc) => {
                        let _ = tx.send(Event::Result(Box::new(doc)));
                    }
                    Err(e) => {
                        let _ = tx.send(Event::Log(format!("{}: {e:#}", path.display())));
                    }
                }
            }
            Ok(())
        });
    }
    fn events(&mut self, ctx: &egui::Context) {
        while let Ok(event) = self.rx.try_recv() {
            match event {
                Event::Progress(p, s) => {
                    self.progress = p.clamp(0., 1.);
                    self.status = s;
                }
                Event::Log(s) => self.log(s),
                Event::Done => {
                    self.busy = false;
                    self.status = if self.cancel.load(Ordering::Relaxed) {
                        "Abgebrochen"
                    } else {
                        "Bereit"
                    }
                    .into();
                    let downloaded = std::mem::take(&mut self.download_ready);
                    if downloaded
                        && self.auto_start
                        && !self.cancel.load(Ordering::Relaxed)
                        && !self.settings.queue.is_empty()
                    {
                        self.start(ctx);
                    }
                }
                Event::Result(doc) => match self.library.store(&doc) {
                    Ok(()) => {
                        self.settings.queue.retain(|p| p != &doc.source_path);
                        self.dirty_settings = true;
                        self.log(format!(
                            "Gespeichert: {} · {} Segmente",
                            doc.source_file,
                            doc.segments.len()
                        ));
                    }
                    Err(e) => {
                        self.log(format!("Bibliothek konnte nicht gespeichert werden: {e:#}"));
                        self.editors.insert(doc.id.clone(), Editor::new(*doc));
                    }
                },
                Event::Discovered(models) => {
                    for size in ModelSize::ALL {
                        let count = models.iter().filter(|m| m.model == size).count();
                        if count > 0 {
                            self.log(format!(
                                "{} vorhanden · wird ohne Download verwendet",
                                size.name()
                            ));
                        }
                    }
                    if models.is_empty() {
                        self.log("Keine verwendbaren Whisper-Modelle gefunden. Ein Modell kann bei Bedarf geladen werden.");
                    }
                    self.installed_models = models;
                }
                Event::Feed(v) => {
                    self.selected_episodes.clear();
                    self.episodes = v;
                    self.podcasts.clear();
                }
                Event::Podcasts(v) => self.podcasts = v,
                Event::Terms(id, candidates) => {
                    if let Some(editor) = self.editors.values_mut().find(|e| e.doc.id == id) {
                        editor.history.record(&editor.doc);
                        editor.doc.candidates = candidates;
                        editor.dirty = true;
                        editor.last_edit = Instant::now();
                    }
                }
                Event::Downloaded(p) => {
                    self.download_ready = true;
                    self.log(format!("Geladen: {}", p.display()));
                    self.add(vec![p]);
                }
                Event::Playback(samples, source) => match Player::new(samples, source) {
                    Ok(p) => {
                        self.player = Some(p);
                        self.log("Audio bereit. Segment im Editor abspielen.");
                    }
                    Err(e) => self.log(format!("Audioausgabe: {e:#}")),
                },
            }
        }
    }
    fn import(&mut self) {
        if let Some(file) = rfd::FileDialog::new()
            .add_filter("Transkript", &["vtt", "srt", "json"])
            .pick_file()
        {
            let result = (|| -> Result<Document> {
                if file.extension().is_some_and(|e| e == "json") {
                    return storage::load(&file);
                }
                let mut doc = document::import_subtitles(&std::fs::read_to_string(&file)?, &file)?;
                if let Some(audio) = rfd::FileDialog::new()
                    .set_title("Optional: passende Audiodatei auswählen (Abbrechen überspringt)")
                    .pick_file()
                {
                    doc.source_file = audio
                        .file_name()
                        .unwrap_or_default()
                        .to_string_lossy()
                        .into();
                    doc.source_path = audio;
                }
                Ok(doc)
            })();
            match result {
                Ok(mut doc) => {
                    for segment in &mut doc.segments {
                        if segment.language.is_none() {
                            segment.language =
                                transcription_rust::glossary::language(&segment.text);
                        }
                    }
                    doc.id = uuid::Uuid::new_v4().to_string();
                    doc.outputs.clear();
                    if let Err(e) = self.library.store(&doc) {
                        self.log(format!("Import: {e:#}"));
                    }
                    self.editors.insert(doc.id.clone(), Editor::new(doc));
                }
                Err(e) => self.log(format!("Import: {e:#}")),
            }
        }
    }
    fn top(&mut self, ctx: &egui::Context) {
        egui::TopBottomPanel::top("navigation")
            .frame(
                egui::Frame::new()
                    .fill(ctx.style().visuals.window_fill)
                    .inner_margin(egui::Margin::symmetric(20, 12)),
            )
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    crate::design::mark(ui);
                    ui.label(egui::RichText::new("Transcription").size(16.).strong());
                    ui.add_space(18.);
                    for (tab, name) in [
                        (Tab::Transcribe, "Transkribieren"),
                        (Tab::Library, "Bibliothek"),
                        (Tab::Glossary, "Glossar"),
                        (Tab::Media, "Medien"),
                        (Tab::Diagnostics, "Diagnose"),
                    ] {
                        let label = if self.tab == tab {
                            egui::RichText::new(name).color(egui::Color32::WHITE)
                        } else {
                            egui::RichText::new(name)
                        };
                        ui.selectable_value(&mut self.tab, tab, label);
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let dark = ui.visuals().dark_mode;
                        if ui
                            .button(if dark { "Hell" } else { "Dunkel" })
                            .on_hover_text("Darstellung wechseln")
                            .clicked()
                        {
                            ui.ctx().set_theme(if dark {
                                egui::Theme::Light
                            } else {
                                egui::Theme::Dark
                            });
                        }
                    });
                });
            });
        egui::TopBottomPanel::bottom("status")
            .frame(
                egui::Frame::new()
                    .fill(ctx.style().visuals.window_fill)
                    .inner_margin(egui::Margin::symmetric(20, 10)),
            )
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    if self.busy {
                        ui.spinner();
                    } else {
                        let (rect, _) =
                            ui.allocate_exact_size(egui::vec2(8., 8.), egui::Sense::hover());
                        ui.painter().circle_filled(
                            rect.center(),
                            3.,
                            egui::Color32::from_rgb(52, 148, 105),
                        );
                    }
                    ui.label(&self.status);
                    if self.busy {
                        ui.add(
                            egui::ProgressBar::new(self.progress)
                                .desired_width(180.)
                                .show_percentage(),
                        );
                        if ui.button("Abbrechen").clicked() {
                            self.cancel.store(true, Ordering::Relaxed);
                            self.auto_start = false;
                        }
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        crate::design::secondary(ui, "Lokale Verarbeitung");
                    });
                });
            });
    }
    fn transcription(&mut self, ctx: &egui::Context) {
        egui::SidePanel::left("queue")
            .resizable(true)
            .default_width(256.)
            .width_range(220.0..=340.0)
            .frame(
                egui::Frame::new()
                    .fill(ctx.style().visuals.window_fill)
                    .inner_margin(20),
            )
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new("Warteschlange").size(16.).strong());
                    crate::design::secondary(ui, self.settings.queue.len().to_string());
                });
                ui.add_space(12.);
                ui.add_enabled_ui(!self.busy, |ui| {
                    if ui
                        .add_sized(
                            [ui.available_width(), 34.],
                            egui::Button::new("Dateien hinzufügen …"),
                        )
                        .clicked()
                        && let Some(files) = rfd::FileDialog::new().pick_files()
                    {
                        self.add(files);
                    }
                    if ui
                        .link("Transkript importieren …")
                        .on_hover_text("VTT, SRT oder vorhandenes JSON-Dokument")
                        .clicked()
                    {
                        self.import();
                    }
                });
                ui.add_space(18.);
                let mut remove = None;
                egui::ScrollArea::vertical()
                    .max_height(ui.available_height() - 100.)
                    .show(ui, |ui| {
                        if self.settings.queue.is_empty() {
                            ui.add_space(24.);
                            crate::design::secondary(ui, "Bereit für deine Aufnahme.");
                            ui.add_space(6.);
                            ui.label("Audio- oder Videodateien hier ablegen oder oben auswählen.");
                        }
                        for (i, path) in self.settings.queue.iter().enumerate() {
                            ui.push_id(i, |ui| {
                                ui.horizontal(|ui| {
                                    ui.vertical(|ui| {
                                        ui.set_max_width(ui.available_width() - 26.);
                                        ui.add(
                                            egui::Label::new(
                                                egui::RichText::new(
                                                    path.file_name()
                                                        .unwrap_or_default()
                                                        .to_string_lossy(),
                                                )
                                                .strong(),
                                            )
                                            .truncate(),
                                        )
                                        .on_hover_text(path.display().to_string());
                                        crate::design::secondary(
                                            ui,
                                            path.extension()
                                                .unwrap_or_default()
                                                .to_string_lossy()
                                                .to_uppercase(),
                                        );
                                    });
                                    if ui
                                        .add_enabled(
                                            !self.busy,
                                            egui::Button::new("×").frame(false),
                                        )
                                        .on_hover_text("Aus der Warteschlange entfernen")
                                        .clicked()
                                    {
                                        remove = Some(i);
                                    }
                                });
                                ui.add_space(8.);
                                ui.separator();
                                ui.add_space(4.);
                            });
                        }
                    });
                if let Some(i) = remove {
                    self.settings.queue.remove(i);
                    self.dirty_settings = true;
                }
                ui.with_layout(egui::Layout::bottom_up(egui::Align::Min), |ui| {
                    crate::design::secondary(ui, "Quelldateien bleiben erhalten.");
                    if ui
                        .add_enabled(
                            !self.busy && !self.settings.queue.is_empty(),
                            egui::Button::new("Liste leeren").frame(false),
                        )
                        .clicked()
                    {
                        self.settings.queue.clear();
                        self.dirty_settings = true;
                    }
                });
            });
        egui::CentralPanel::default()
            .frame(
                egui::Frame::new()
                    .fill(ctx.style().visuals.panel_fill)
                    .inner_margin(28),
            )
            .show(ctx, |ui| {
                egui::ScrollArea::vertical().show(ui, |ui| {
                    crate::design::title(
                        ui,
                        "Transkription",
                        "Aus Audio und Video wird bearbeitbarer Text. Lokal auf diesem Mac.",
                    );
                    let before = serde_json::to_string(&self.settings).unwrap_or_default();
                    ui.add_enabled_ui(!self.busy, |ui| {
                        crate::design::section(ui, "Erkennung", |ui| {
                            ui.columns(2, |cols| {
                                let ui = &mut cols[0];
                                crate::design::secondary(ui, "Whisper-Modell");
                                egui::ComboBox::from_id_salt("model")
                                    .width(ui.available_width())
                                    .selected_text(self.settings.model.label())
                                    .show_ui(ui, |ui| {
                                        for m in ModelSize::ALL {
                                            ui.selectable_value(
                                                &mut self.settings.model,
                                                m,
                                                m.label(),
                                            );
                                        }
                                    });
                                let available = self
                                    .installed_models
                                    .iter()
                                    .any(|m| m.model == self.settings.model);
                                crate::design::secondary(
                                    ui,
                                    if available {
                                        "Vorhanden · kein Download nötig"
                                    } else {
                                        "Wird einmalig heruntergeladen"
                                    },
                                );
                                let ui = &mut cols[1];
                                crate::design::secondary(ui, "Sprache");
                                egui::ComboBox::from_id_salt("language")
                                    .width(ui.available_width())
                                    .selected_text(match self.settings.language.as_str() {
                                        "de" => "Deutsch",
                                        "en" => "Englisch",
                                        _ => "Deutsch + Englisch",
                                    })
                                    .show_ui(ui, |ui| {
                                        for (value, label) in [
                                            ("auto", "Deutsch + Englisch · automatisch"),
                                            ("de", "Deutsch"),
                                            ("en", "Englisch"),
                                        ] {
                                            ui.selectable_value(
                                                &mut self.settings.language,
                                                value.into(),
                                                label,
                                            );
                                        }
                                    });
                                crate::design::secondary(
                                    ui,
                                    if self.settings.language == "auto" {
                                        "Sprache abschnittsweise erkennen"
                                    } else {
                                        "Gewählte Sprache verwenden"
                                    },
                                );
                            });
                            ui.add_space(8.);
                            ui.separator();
                            ui.add_space(6.);
                            ui.horizontal_wrapped(|ui| {
                                ui.checkbox(
                                    &mut self.settings.diarization,
                                    "Sprecher unterscheiden",
                                );
                                if self.settings.diarization {
                                    ui.add_space(12.);
                                    egui::ComboBox::from_id_salt("speaker-range")
                                        .selected_text(if self.settings.speaker_range == "auto" {
                                            "Automatisch"
                                        } else {
                                            &self.settings.speaker_range
                                        })
                                        .show_ui(ui, |ui| {
                                            for (value, label) in [
                                                ("auto", "Automatisch"),
                                                ("2-2", "Genau 2"),
                                                ("2-4", "2–4"),
                                                ("4-6", "4–6"),
                                                ("5-8", "5–8"),
                                                ("6-10", "6–10"),
                                            ] {
                                                if ui
                                                    .selectable_value(
                                                        &mut self.settings.speaker_range,
                                                        value.into(),
                                                        label,
                                                    )
                                                    .changed()
                                                {
                                                    self.settings.speakers = 0;
                                                }
                                            }
                                        });
                                    let selected = [
                                        (0.78, "Locker"),
                                        (0.60, "Normal"),
                                        (0.45, "Streng"),
                                        (0.30, "Sehr streng"),
                                    ]
                                    .into_iter()
                                    .min_by(|a, b| {
                                        (a.0 - self.settings.separation)
                                            .abs()
                                            .total_cmp(&(b.0 - self.settings.separation).abs())
                                    })
                                    .unwrap()
                                    .1;
                                    egui::ComboBox::from_id_salt("speaker-separation")
                                        .selected_text(selected)
                                        .show_ui(ui, |ui| {
                                            for (value, label) in [
                                                (0.78, "Locker"),
                                                (0.60, "Normal"),
                                                (0.45, "Streng"),
                                                (0.30, "Sehr streng"),
                                            ] {
                                                ui.selectable_value(
                                                    &mut self.settings.separation,
                                                    value,
                                                    label,
                                                );
                                            }
                                        });
                                }
                            });
                            egui::CollapsingHeader::new("Weitere Einstellungen").show(ui, |ui| {
                                ui.checkbox(&mut self.settings.cpu, "CPU verwenden (sonst Metal)");
                                ui.horizontal_wrapped(|ui| {
                                    ui.checkbox(
                                        &mut self.settings.timecodes,
                                        "Zeitangaben im Markdown",
                                    );
                                    ui.add(
                                        egui::DragValue::new(&mut self.settings.fps)
                                            .range(1..=120)
                                            .suffix(" fps"),
                                    );
                                });
                            });
                        });
                        ui.add_space(14.);
                        crate::design::section(ui, "Ausgabe", |ui| {
                            ui.horizontal_wrapped(|ui| {
                                for f in Format::ALL {
                                    let mut selected = self.settings.formats.contains(&f);
                                    if ui
                                        .checkbox(&mut selected, f.extension().to_uppercase())
                                        .changed()
                                    {
                                        if selected {
                                            self.settings.formats.push(f);
                                        } else {
                                            self.settings.formats.retain(|v| *v != f);
                                        }
                                    }
                                }
                            });
                            ui.add_space(5.);
                            ui.separator();
                            ui.add_space(4.);
                            ui.horizontal_wrapped(|ui| {
                                crate::design::secondary(ui, "Speichern");
                                let label = self
                                    .settings
                                    .output_dir
                                    .as_ref()
                                    .map(|p| {
                                        p.file_name()
                                            .unwrap_or_default()
                                            .to_string_lossy()
                                            .to_string()
                                    })
                                    .unwrap_or_else(|| "Neben der Quelldatei".into());
                                if ui
                                    .button(label)
                                    .on_hover_text(
                                        self.settings
                                            .output_dir
                                            .as_ref()
                                            .map(|p| p.display().to_string())
                                            .unwrap_or_else(|| {
                                                "Für jede Datei im jeweiligen Quellordner".into()
                                            }),
                                    )
                                    .clicked()
                                    && let Some(dir) = rfd::FileDialog::new().pick_folder()
                                {
                                    self.settings.output_dir = Some(dir);
                                }
                                if self.settings.output_dir.is_some()
                                    && ui.link("Zurücksetzen").clicked()
                                {
                                    self.settings.output_dir = None;
                                }
                            });
                        });
                    });
                    if before != serde_json::to_string(&self.settings).unwrap_or_default() {
                        self.dirty_settings = true;
                    }
                    ui.add_space(18.);
                    ui.horizontal_wrapped(|ui| {
                        let label = match self.settings.queue.len() {
                            0 => "Transkription starten".into(),
                            1 => "1 Datei transkribieren".into(),
                            n => format!("{n} Dateien transkribieren"),
                        };
                        if crate::design::action(
                            ui,
                            label,
                            !self.busy
                                && !self.settings.queue.is_empty()
                                && !self.settings.formats.is_empty(),
                        )
                        .clicked()
                        {
                            self.start(ctx);
                        }
                        if ui
                            .add_enabled(
                                !self.busy,
                                egui::Button::new("Modell vorbereiten").frame(false),
                            )
                            .on_hover_text(
                                "Vorhandene Gewichte verwenden oder fehlendes Modell herunterladen",
                            )
                            .clicked()
                        {
                            let size = self.settings.model;
                            let speakers = self.settings.diarization;
                            self.task(ctx, move |tx, cancel| {
                                let mut report = |p, s| {
                                    let _ = tx.send(Event::Progress(p, s));
                                };
                                models::ensure_model(size, &cancel, &mut report)?;
                                if speakers {
                                    transcription_rust::speakers::ensure_weights(
                                        &cancel,
                                        &mut report,
                                    )?;
                                }
                                Ok(())
                            });
                        }
                    });
                    if self.settings.formats.is_empty() {
                        ui.colored_label(
                            ui.visuals().error_fg_color,
                            "Bitte mindestens ein Ausgabeformat auswählen.",
                        );
                    }
                    ui.add_space(22.);
                    ui.separator();
                    ui.add_space(12.);
                    ui.label(egui::RichText::new("Letzte Transkripte").size(15.).strong());
                    let records = self
                        .library
                        .records
                        .iter()
                        .filter(|r| !r.archived)
                        .take(4)
                        .cloned()
                        .collect::<Vec<_>>();
                    if records.is_empty() {
                        crate::design::secondary(
                            ui,
                            "Fertige Transkripte erscheinen hier und in deiner Bibliothek.",
                        );
                    }
                    for record in records {
                        ui.horizontal(|ui| {
                            if ui.link(&record.title).clicked() {
                                self.open_editor(&record.path);
                            }
                            crate::design::secondary(ui, "Bearbeiten");
                        });
                    }
                    ui.add_space(14.);
                    egui::CollapsingHeader::new("Aktivität").show(ui, |ui| {
                        self.activity(ui);
                    });
                });
            });
    }
    fn activity(&self, ui: &mut egui::Ui) {
        ui.add_space(12.);
        ui.label("Aktivität");
        egui::ScrollArea::vertical()
            .id_salt("activity")
            .max_height(180.)
            .stick_to_bottom(true)
            .show(ui, |ui| {
                for line in &self.log {
                    ui.add(
                        egui::Label::new(egui::RichText::new(line).monospace().size(11.))
                            .selectable(true),
                    );
                }
            });
    }
    fn open_editor(&mut self, path: &Path) {
        match self.library.read_document(path) {
            Ok((doc, recovered)) => {
                if let Some(backup) = recovered {
                    self.log(format!(
                        "Dokument aus Sicherung wiederhergestellt: {}",
                        backup.display()
                    ));
                }
                if let Some(editor) = self.editors.get_mut(&doc.id) {
                    editor.open = true;
                } else {
                    self.editors.insert(doc.id.clone(), Editor::new(doc));
                }
            }
            Err(e) => self.log(format!("Dokument: {e:#}")),
        }
    }
    fn library(&mut self, ui: &mut egui::Ui) {
        crate::design::title(
            ui,
            "Bibliothek",
            "Deine Transkripte. Durchsuchen, weiterbearbeiten und organisieren.",
        );
        ui.horizontal(|ui| {
            ui.label("Suchen");
            ui.text_edit_singleline(&mut self.search);
            ui.checkbox(&mut self.show_archive, "Archiv anzeigen");
            if ui.button("Importieren …").clicked() {
                self.import();
            }
        });
        ui.separator();
        let records = self.library.records.clone();
        let mut trash = None;
        let mut archive = None;
        egui::ScrollArea::vertical().show(ui, |ui| {
            for (i, record) in records.iter().enumerate() {
                if record.archived != self.show_archive
                    || !record
                        .title
                        .to_lowercase()
                        .contains(&self.search.to_lowercase())
                {
                    continue;
                }
                ui.horizontal(|ui| {
                    if ui.button(&record.title).clicked() {
                        self.open_editor(&record.path);
                    }
                    if ui
                        .small_button(if record.archived {
                            "Wiederherstellen"
                        } else {
                            "Archivieren"
                        })
                        .clicked()
                    {
                        archive = Some(i);
                    }
                    if ui.small_button("Papierkorb …").clicked() {
                        trash = Some(i);
                    }
                    if ui.small_button("Im Finder").clicked() {
                        reveal(&record.path);
                    }
                });
                ui.small(record.source.display().to_string());
                ui.separator();
            }
        });
        if let Some(i) = archive {
            self.library.records[i].archived = !self.library.records[i].archived;
            if let Err(e) = self.library.persist() {
                self.log(format!("Archiv: {e:#}"));
            }
        }
        if let Some(i) = trash {
            self.confirm_trash = Some(i);
        }
    }
    fn glossary(&mut self, ui: &mut egui::Ui) {
        crate::design::title(
            ui,
            "Glossar",
            "Namen und Fachbegriffe für deine nächsten Aufnahmen.",
        );
        ui.label("Bestätigte Begriffe werden Whisper als Kontext übergeben. Kandidaten sind Vorschläge aus dem Transkript.");
        ui.horizontal(|ui| {
            ui.text_edit_singleline(&mut self.term);
            if ui.button("Begriff hinzufügen").clicked() {
                let term = self.term.trim().to_string();
                if !term.is_empty() && !self.settings.glossary.contains(&term) {
                    self.settings.glossary.push(term);
                    self.term.clear();
                    self.dirty_settings = true;
                }
            }
        });
        let mut remove = None;
        egui::ScrollArea::vertical().show(ui, |ui| {
            for (i, term) in self.settings.glossary.iter().enumerate() {
                ui.horizontal(|ui| {
                    ui.label(term);
                    if ui.small_button("Entfernen").clicked() {
                        remove = Some(i);
                    }
                });
            }
        });
        if let Some(i) = remove {
            self.settings.glossary.remove(i);
            self.dirty_settings = true;
        }
    }
    fn media(&mut self, ctx: &egui::Context, ui: &mut egui::Ui) {
        crate::design::title(
            ui,
            "Medien",
            "Podcast-Folgen und YouTube-Audio in deine Warteschlange übernehmen.",
        );
        ui.label("RSS-Feed oder YouTube-Adresse");
        ui.text_edit_singleline(&mut self.url);
        ui.horizontal(|ui| {
            if ui
                .add_enabled(!self.busy, egui::Button::new("RSS öffnen"))
                .clicked()
            {
                let url = self.url.clone();
                if !self.settings.feeds.contains(&url) {
                    self.settings.feeds.push(url.clone());
                    self.dirty_settings = true;
                }
                self.task(ctx, move |tx, _| {
                    tx.send(Event::Feed(media::feed(&url)?))?;
                    Ok(())
                });
            }
            if ui
                .add_enabled(!self.busy, egui::Button::new("YouTube herunterladen"))
                .clicked()
            {
                let url = self.url.clone();
                let video = self.download_video;
                self.task(ctx, move |tx, cancel| {
                    let path =
                        media::youtube(&url, &media::downloads_dir(), video, &cancel, &mut |p| {
                            let _ = tx.send(Event::Progress(p, "YouTube laden …".into()));
                        })?;
                    tx.send(Event::Downloaded(path))?;
                    Ok(())
                });
            }
        });
        ui.horizontal(|ui| {
            ui.checkbox(&mut self.download_video, "Zusätzliche Videodatei");
            ui.checkbox(
                &mut self.auto_start,
                "Nach Download Warteschlange transkribieren",
            );
        });
        ui.small("YouTube-Audio wird als MP3 gespeichert. Die zusätzliche MP4-Datei enthält Bild und Ton.");
        egui::ComboBox::from_id_salt("saved-feeds")
            .selected_text("Gespeicherte RSS-Feeds")
            .show_ui(ui, |ui| {
                for url in &self.settings.feeds {
                    if ui.selectable_label(false, url).clicked() {
                        self.url = url.clone();
                    }
                }
            });
        ui.separator();
        ui.horizontal(|ui| {
            ui.text_edit_singleline(&mut self.podcast_query);
            if ui
                .add_enabled(!self.busy, egui::Button::new("PodcastIndex durchsuchen"))
                .clicked()
            {
                let query = self.podcast_query.clone();
                self.task(ctx, move |tx, _| {
                    tx.send(Event::Podcasts(media::search_podcasts(&query)?))?;
                    Ok(())
                });
            }
        });
        egui::CollapsingHeader::new("PodcastIndex-Zugang").show(ui,|ui|{
            ui.label("Vorhandene Zugangsdaten der macOS-App werden automatisch gelesen. Neue Daten werden im Schlüsselbund gespeichert.");
            ui.label("API-Schlüssel");ui.add(egui::TextEdit::singleline(&mut self.podcast_key).password(true));
            ui.label("API-Geheimnis");ui.add(egui::TextEdit::singleline(&mut self.podcast_secret).password(true));
            if ui.button("Im Schlüsselbund speichern").clicked(){match transcription_rust::credentials::save(&self.podcast_key,&self.podcast_secret){Ok(())=>{self.podcast_key.clear();self.podcast_secret.clear();self.log("PodcastIndex-Zugang im Schlüsselbund gespeichert.");},Err(e)=>self.log(e.to_string())}}
        });
        let podcasts = self.podcasts.clone();
        for podcast in podcasts {
            if ui
                .button(format!("{} · {}", podcast.title, podcast.author))
                .clicked()
                && !self.busy
            {
                self.url = podcast.url.clone();
                self.task(ctx, move |tx, _| {
                    tx.send(Event::Feed(media::feed(&podcast.url)?))?;
                    Ok(())
                });
            }
        }
        if ui
            .add_enabled(
                !self.busy && !self.selected_episodes.is_empty(),
                egui::Button::new(format!(
                    "{} ausgewählte Folgen laden",
                    self.selected_episodes.len()
                )),
            )
            .clicked()
        {
            let episodes = self
                .episodes
                .iter()
                .filter(|e| self.selected_episodes.contains(&e.audio_url))
                .cloned()
                .collect::<Vec<_>>();
            self.task(ctx, move |tx, cancel| {
                let count = episodes.len();
                for (i, episode) in episodes.iter().enumerate() {
                    audio::check_cancel(&cancel)?;
                    match media::download_episode(
                        episode,
                        &media::downloads_dir(),
                        &cancel,
                        &mut |p| {
                            let _ = tx.send(Event::Progress(
                                (i as f32 + p) / count as f32,
                                format!("Folge {}/{} laden", i + 1, count),
                            ));
                        },
                    ) {
                        Ok(path) => {
                            tx.send(Event::Downloaded(path))?;
                        }
                        Err(e) => {
                            tx.send(Event::Log(format!("{}: {e:#}", episode.title)))?;
                        }
                    }
                }
                Ok(())
            });
        }
        let episodes = self.episodes.clone();
        egui::ScrollArea::vertical()
            .max_height(400.)
            .show(ui, |ui| {
                for episode in episodes {
                    ui.horizontal(|ui| {
                        let mut selected = self.selected_episodes.contains(&episode.audio_url);
                        if ui
                            .add_enabled(!self.busy, egui::Checkbox::without_text(&mut selected))
                            .on_hover_text("Folge auswählen")
                            .changed()
                        {
                            if selected {
                                self.selected_episodes.insert(episode.audio_url.clone());
                            } else {
                                self.selected_episodes.remove(&episode.audio_url);
                            }
                        }

                        if ui
                            .add_enabled(!self.busy, egui::Button::new("Laden"))
                            .clicked()
                        {
                            let episode = episode.clone();
                            self.task(ctx, move |tx, cancel| {
                                let path = media::download_episode(
                                    &episode,
                                    &media::downloads_dir(),
                                    &cancel,
                                    &mut |p| {
                                        let _ =
                                            tx.send(Event::Progress(p, "Podcast laden …".into()));
                                    },
                                )?;
                                tx.send(Event::Downloaded(path))?;
                                Ok(())
                            });
                        }
                        ui.label(format!("{} · {}", episode.title, episode.published));
                    });
                }
            });
        self.activity(ui);
    }
    fn diagnostics(&mut self, ui: &mut egui::Ui) {
        crate::design::title(
            ui,
            "Modelle und Diagnose",
            "Vorhandene Modelle erkennen, Speicherorte verwalten und den Status prüfen.",
        );
        ui.label(format!(
            "Version {} · Work in progress · {}",
            env!("CARGO_PKG_VERSION"),
            std::env::consts::ARCH
        ));
        ui.label(format!("Daten: {}", storage::data_dir().display()));
        ui.label(format!("Modelle: {}", models::model_root().display()));
        ui.label("Beim ersten Start und vor jedem Download werden vorhandene Modelle geprüft.");
        for model in ModelSize::ALL {
            let found = self
                .installed_models
                .iter()
                .filter(|m| m.model == model)
                .collect::<Vec<_>>();
            if found.is_empty() {
                ui.label(format!("{}: nicht gefunden", model.name()));
            }
            for item in found {
                ui.label(format!("{} · {}", model.name(), item.label()));
            }
        }
        ui.horizontal(|ui| {
            if ui
                .add_enabled(!self.busy, egui::Button::new("Erneut suchen"))
                .clicked()
            {
                self.scan_pending = true;
            }
            if ui
                .add_enabled(!self.busy, egui::Button::new("Modellordner hinzufügen …"))
                .clicked()
                && let Some(dir) = rfd::FileDialog::new().pick_folder()
            {
                if !self.settings.model_search_dirs.contains(&dir) {
                    self.settings.model_search_dirs.push(dir);
                }
                match storage::save(&storage::data_dir().join("settings.json"), &self.settings) {
                    Ok(()) => {
                        self.scan_pending = true;
                        self.dirty_settings = false;
                    }
                    Err(e) => self.log(format!("Modellordner speichern: {e:#}")),
                }
            }
        });
        for dir in &self.settings.model_search_dirs {
            ui.small(format!("Zusätzlicher Suchordner: {}", dir.display()));
        }
        for name in ["python3", "ffmpeg", "deno", "yt-dlp", "whisper-cli"] {
            let found = transcription_rust::discovery::existing_dependency(name)
                .map(|p| p.display().to_string())
                .unwrap_or_else(|| "nicht gefunden".into());
            ui.small(format!(
                "{name}: {found} · für diese App nicht erforderlich"
            ));
        }
        if ui.button("Datenordner öffnen").clicked() {
            reveal(&storage::data_dir());
        }
        ui.label("Rust: Candle + Metal/CPU · Silero · ECAPA-TDNN · Symphonia/OxideAV · egui");
        self.activity(ui);
    }
    fn save_editor(&mut self, editor: &mut Editor) -> bool {
        match self.library.store(&editor.doc) {
            Ok(()) => {
                editor.dirty = false;
                true
            }
            Err(e) => {
                self.log(format!("Speichern fehlgeschlagen: {e:#}"));
                false
            }
        }
    }
    fn editors(&mut self, ctx: &egui::Context) {
        let ids = self.editors.keys().cloned().collect::<Vec<_>>();
        for id in ids {
            let Some(mut editor) = self.editors.remove(&id) else {
                continue;
            };
            if editor.open {
                let title = format!("{} — Transkript", editor.doc.source_file);
                ctx.show_viewport_immediate(
                    egui::ViewportId::from_hash_of(&id),
                    egui::ViewportBuilder::default()
                        .with_title(title)
                        .with_inner_size([1020., 730.])
                        .with_min_inner_size([720., 520.]),
                    |ctx, _| {
                        if ctx.input(|i| i.viewport().close_requested()) {
                            if self.save_editor(&mut editor) {
                                editor.open = false;
                            } else {
                                ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
                            }
                        }
                        egui::CentralPanel::default()
                            .frame(
                                egui::Frame::new()
                                    .fill(ctx.style().visuals.panel_fill)
                                    .inner_margin(24),
                            )
                            .show(ctx, |ui| {
                                self.editor_ui(ctx, ui, &mut editor);
                            });
                    },
                );
                if editor.dirty && editor.last_edit.elapsed() > Duration::from_secs(2) {
                    self.save_editor(&mut editor);
                }
            }
            self.editors.insert(id, editor);
        }
    }
    fn editor_ui(&mut self, ctx: &egui::Context, ui: &mut egui::Ui, editor: &mut Editor) {
        ui.heading(&editor.doc.source_file);
        let old = editor.doc.clone();
        let mut historical = false;
        ui.horizontal_wrapped(|ui| {
            if ui.button("Speichern").clicked() {
                self.save_editor(editor);
            }
            if ui.button("Rückgängig").clicked() {
                editor.history.undo(&mut editor.doc);
                historical = true;
            }
            if ui.button("Wiederholen").clicked() {
                editor.history.redo(&mut editor.doc);
                historical = true;
            }
            if ui.button("Exportieren …").clicked()
                && let Some(dir) = rfd::FileDialog::new().pick_folder()
            {
                match editor.doc.export_new(&dir, &self.settings.formats) {
                    Ok(()) => {
                        self.log("Neue Exporte gespeichert.");
                    }
                    Err(e) => self.log(format!("Export: {e:#}")),
                }
            }
            if !editor.doc.outputs.is_empty() && ui.button("Exporte aktualisieren").clicked() {
                let result = (|| -> Result<()> {
                    for f in Format::ALL {
                        if let Some(path) = editor.doc.outputs.get(f.extension()) {
                            let path = Path::new(path);
                            if path.exists() {
                                let backup =
                                    storage::data_dir().join("export-backups").join(format!(
                                        "{}-{}.{}",
                                        editor.doc.id,
                                        chrono::Utc::now()
                                            .timestamp_nanos_opt()
                                            .unwrap_or_default(),
                                        f.extension()
                                    ));
                                if let Some(parent) = backup.parent() {
                                    std::fs::create_dir_all(parent)?;
                                }
                                std::fs::copy(path, backup)?;
                            }
                            storage::atomic_write(path, editor.doc.render(f)?.as_bytes())?;
                        }
                    }
                    Ok(())
                })();
                if let Err(e) = result {
                    self.log(format!("Export: {e:#}"));
                }
            }
            ui.label(if editor.dirty {
                "Änderungen werden automatisch gespeichert …"
            } else {
                "Gespeichert"
            });
        });
        if ctx.input_mut(|i| i.consume_key(egui::Modifiers::COMMAND, egui::Key::S)) {
            self.save_editor(editor);
        }
        if ctx.input_mut(|i| i.consume_key(egui::Modifiers::COMMAND, egui::Key::Z)) {
            editor.history.undo(&mut editor.doc);
            historical = true;
        }
        if ctx.input_mut(|i| {
            i.consume_key(
                egui::Modifiers::COMMAND | egui::Modifiers::SHIFT,
                egui::Key::Z,
            )
        }) {
            editor.history.redo(&mut editor.doc);
            historical = true;
        }
        ui.separator();
        ui.horizontal_wrapped(|ui| {
            if ui
                .add_enabled(!self.busy, egui::Button::new("Audio laden"))
                .clicked()
            {
                let path = editor.doc.source_path.clone();
                let source = path.display().to_string();
                self.task(ctx, move |tx, cancel| {
                    tx.send(Event::Playback(audio::decode(&path, &cancel)?, source))?;
                    Ok(())
                });
            }
            if ui.button("Audiodatei zuordnen …").clicked()
                && let Some(path) = rfd::FileDialog::new().pick_file()
            {
                editor.doc.source_path = path;
            }
            ui.add(egui::Slider::new(&mut editor.speed, 0.5..=2.).text("Geschwindigkeit"));
            ui.checkbox(&mut editor.looping, "Schleife");
            if let Some(player) = &mut self.player
                && player.source == editor.doc.source_path.display().to_string()
            {
                player.sink.set_speed(editor.speed);
                if ui
                    .button(if player.sink.empty() {
                        "Wiedergeben"
                    } else if player.sink.is_paused() {
                        "Weiter"
                    } else {
                        "Pause"
                    })
                    .clicked()
                {
                    if player.sink.empty() {
                        if let Some(segment) = editor.doc.segments.get(editor.selected) {
                            player.play(segment.start, segment.end, editor.speed, editor.looping);
                        }
                    } else if player.sink.is_paused() {
                        player.sink.play();
                    } else {
                        player.sink.pause();
                    }
                }
                if ui.button("Stopp").clicked() {
                    player.sink.stop();
                    player.sink.pause();
                    player.start = 0.;
                    player.end = 0.;
                }
                let elapsed = player.sink.get_pos().as_secs_f64();
                let position = if player.looping && player.end > player.start {
                    player.start + elapsed % (player.end - player.start)
                } else {
                    (player.start + elapsed).min(player.end)
                };
                ui.label(document::subtitle_time(position, '.'));
            }
        });
        ui.horizontal(|ui| {
            ui.label("Segment suchen");
            ui.text_edit_singleline(&mut editor.search);
            ui.label(format!("{} Segmente", editor.doc.segments.len()));
        });
        egui::CollapsingHeader::new("Sprecher benennen und zusammenführen").show(ui, |ui| {
            let ids = editor
                .doc
                .speaker_names
                .keys()
                .cloned()
                .chain(editor.doc.speaker_regions.iter().map(|r| r.speaker.clone()))
                .chain(editor.doc.segments.iter().filter_map(|s| s.speaker.clone()))
                .collect::<std::collections::BTreeSet<_>>();
            for id in &ids {
                ui.horizontal(|ui| {
                    ui.label(id);
                    let name = editor
                        .doc
                        .speaker_names
                        .entry(id.clone())
                        .or_insert_with(|| id.clone());
                    ui.text_edit_singleline(name);
                    if ui.button("Hörprobe").clicked()
                        && let (Some(s), Some(player)) = (
                            editor
                                .doc
                                .segments
                                .iter()
                                .find(|s| s.speaker.as_ref() == Some(id)),
                            &mut self.player,
                        )
                        && player.source == editor.doc.source_path.display().to_string()
                    {
                        player.play(s.start, s.end.min(s.start + 5.), editor.speed, false);
                    }
                });
            }
            let selected = editor
                .doc
                .segments
                .get(editor.selected)
                .and_then(|s| s.speaker.clone());
            if let Some(from) = selected {
                ui.horizontal(|ui| {
                    ui.label(format!("Stimme des gewählten Segments ({from}) →"));
                    egui::ComboBox::from_id_salt("merge-speaker")
                        .selected_text(&editor.merge_target)
                        .show_ui(ui, |ui| {
                            for id in &ids {
                                if id != &from {
                                    ui.selectable_value(&mut editor.merge_target, id.clone(), id);
                                }
                            }
                        });
                    if ui.button("Zusammenführen").clicked()
                        && ids.contains(&editor.merge_target)
                        && editor.merge_target != from
                    {
                        editor.doc.merge_speakers(&from, &editor.merge_target);
                    }
                });
            }
        });
        egui::CollapsingHeader::new("Begriffsvorschläge").show(ui, |ui| {
            ui.label("Personen, Organisationen und Orte lokal erkennen. Das Sprachmodell benötigt beim ersten Mal etwa 539 MB; danach arbeitet die Analyse offline.");
            if ui.add_enabled(!self.busy,egui::Button::new("Sprachanalyse starten")).clicked() {
                let id=editor.doc.id.clone();
                let text=editor.doc.segments.iter().map(|s|s.text.as_str()).collect::<Vec<_>>().join("\n");
                let cpu=self.settings.cpu;
                self.task(ctx,move |tx,cancel| {
                    let candidates=transcription_rust::terms::analyze(&text,cpu,&cancel,&mut |p,m|{let _=tx.send(Event::Progress(p,m));})?;
                    tx.send(Event::Terms(id,candidates))?;Ok(())
                });
            }
            let candidates=if editor.doc.candidates.is_empty() {engine::term_candidates(&editor.doc,&self.settings.glossary)} else {editor.doc.candidates.iter().filter(|c|!self.settings.glossary.iter().any(|g|g.to_lowercase()==c.term.to_lowercase())).map(|c|(c.term.clone(),c.count)).collect()};
            for (term,count) in candidates.into_iter().take(100) {
                ui.horizontal(|ui| {
                    ui.label(format!("{term} ({count})"));
                    ui.add(
                        egui::TextEdit::singleline(
                            editor.replacements.entry(term.clone()).or_default(),
                        )
                        .hint_text("Korrekte Schreibweise"),
                    );
                    if ui.small_button("Ins Glossar").clicked() {
                        self.settings.glossary.push(term.clone());
                        self.dirty_settings = true;
                    }
                });
            }
        });
        if editor.replacements.values().any(|v| !v.trim().is_empty())
            && ui.button("Bestätigte Korrekturen anwenden").clicked()
        {
            for segment in &mut editor.doc.segments {
                segment.text =
                    transcription_rust::glossary::apply(&segment.text, &editor.replacements);
            }
            for term in editor
                .replacements
                .values()
                .filter(|v| !v.trim().is_empty())
            {
                if !self.settings.glossary.contains(term) {
                    self.settings.glossary.push(term.clone());
                    self.dirty_settings = true;
                }
            }
            editor.replacements.clear();
        }
        ui.separator();
        let speaker_ids = editor
            .doc
            .speaker_names
            .keys()
            .cloned()
            .chain(editor.doc.speaker_regions.iter().map(|r| r.speaker.clone()))
            .chain(editor.doc.segments.iter().filter_map(|s| s.speaker.clone()))
            .chain((1..=10).map(|i| format!("SPEAKER_{i:02}")))
            .collect::<std::collections::BTreeSet<_>>();
        let mut split = None;
        let mut merge = None;
        let mut delete = None;
        egui::ScrollArea::vertical()
            .id_salt("segments")
            .show(ui, |ui| {
                for (index, segment) in editor.doc.segments.iter_mut().enumerate() {
                    if !editor.search.is_empty()
                        && !segment
                            .text
                            .to_lowercase()
                            .contains(&editor.search.to_lowercase())
                    {
                        continue;
                    }
                    ui.push_id(&segment.id, |ui| {
                        ui.horizontal_wrapped(|ui| {
                            if ui
                                .selectable_label(
                                    editor.selected == index,
                                    format!(
                                        "{} – {}",
                                        document::subtitle_time(segment.start, '.'),
                                        document::subtitle_time(segment.end, '.')
                                    ),
                                )
                                .clicked()
                            {
                                editor.selected = index;
                            }
                            ui.add(
                                egui::DragValue::new(&mut segment.start)
                                    .speed(0.05)
                                    .range(0.0..=segment.end.max(0.))
                                    .prefix("Von ")
                                    .suffix(" s"),
                            );
                            ui.add(
                                egui::DragValue::new(&mut segment.end)
                                    .speed(0.05)
                                    .range(segment.start..=f64::MAX)
                                    .prefix("Bis ")
                                    .suffix(" s"),
                            );
                            egui::ComboBox::from_id_salt("speaker")
                                .selected_text(
                                    segment.speaker.as_deref().unwrap_or("Ohne Sprecher"),
                                )
                                .show_ui(ui, |ui| {
                                    ui.selectable_value(
                                        &mut segment.speaker,
                                        None,
                                        "Ohne Sprecher",
                                    );
                                    for id in &speaker_ids {
                                        ui.selectable_value(
                                            &mut segment.speaker,
                                            Some(id.clone()),
                                            id,
                                        );
                                    }
                                });
                            egui::ComboBox::from_id_salt("segment-language")
                                .selected_text(segment.language.as_deref().unwrap_or("Sprache"))
                                .show_ui(ui, |ui| {
                                    for lang in ["de", "en"] {
                                        ui.selectable_value(
                                            &mut segment.language,
                                            Some(lang.into()),
                                            lang,
                                        );
                                    }
                                });
                            if ui.button("Anhören").clicked() {
                                editor.selected = index;
                                if let Some(player) = &mut self.player
                                    && player.source == editor.doc.source_path.display().to_string()
                                {
                                    player.play(
                                        segment.start,
                                        segment.end,
                                        editor.speed,
                                        editor.looping,
                                    );
                                }
                            }
                        });
                        let text = egui::TextEdit::multiline(&mut segment.text)
                            .desired_width(f32::INFINITY)
                            .desired_rows(2)
                            .show(ui);
                        if text.response.has_focus() {
                            editor.selected = index;
                        }
                        ui.horizontal(|ui| {
                            if ui.small_button("Teilen").clicked() {
                                let cursor = text
                                    .cursor_range
                                    .map(|r| r.primary.index)
                                    .filter(|n| *n > 0 && *n < segment.text.chars().count());
                                split = Some((index, cursor));
                            }
                            if ui.small_button("Mit nächstem verbinden").clicked() {
                                merge = Some(index);
                            }
                            if ui.small_button("Segment entfernen").clicked() {
                                delete = Some(index);
                            }
                        });
                        ui.separator();
                    });
                }
            });
        if let Some((index, cursor)) = split
            && let Err(e) = editor.doc.split_words(index, cursor)
        {
            self.log(e.to_string());
        }
        if let Some(index) = merge
            && let Err(e) = editor.doc.merge(index)
        {
            self.log(e.to_string());
        }
        if let Some(index) = delete {
            editor.doc.segments.remove(index);
        }
        if old != editor.doc {
            if !historical {
                editor.history.record(&old);
            }
            editor.dirty = true;
            editor.last_edit = Instant::now();
        }
    }
}
impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        ctx.request_repaint_after(Duration::from_millis(150));
        self.events(ctx);
        if self.scan_pending && !self.busy {
            self.scan_pending = false;
            self.status = "Vorhandene Modelle erkennen …".into();
            self.task(ctx, move |tx, cancel| {
                let mut found = vec![];
                for model in ModelSize::ALL {
                    audio::check_cancel(&cancel)?;
                    found.extend(transcription_rust::discovery::discover(model));
                }
                tx.send(Event::Discovered(found))?;
                Ok(())
            });
        }
        if ctx.input(|i| i.viewport().close_requested()) && self.busy {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            self.close_pending = true;
        }
        let files = ctx.input(|i| {
            i.raw
                .dropped_files
                .iter()
                .filter_map(|f| f.path.clone())
                .collect::<Vec<_>>()
        });
        if !self.busy && !files.is_empty() {
            self.add(files);
        }
        self.top(ctx);
        if self.tab == Tab::Transcribe {
            self.transcription(ctx);
        } else {
            egui::CentralPanel::default()
                .frame(
                    egui::Frame::new()
                        .fill(ctx.style().visuals.panel_fill)
                        .inner_margin(28),
                )
                .show(ctx, |ui| match self.tab {
                    Tab::Library => self.library(ui),
                    Tab::Glossary => self.glossary(ui),
                    Tab::Media => self.media(ctx, ui),
                    Tab::Diagnostics => self.diagnostics(ui),
                    _ => {}
                });
        }
        self.editors(ctx);
        if self.exit_after_cancel && !self.busy {
            self.exit_after_cancel = false;
            self.close_pending = false;
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
        if ctx.input(|i| i.viewport().close_requested()) && !self.busy {
            let mut failed = false;
            for editor in self.editors.values_mut().filter(|e| e.dirty) {
                match self.library.store(&editor.doc) {
                    Ok(()) => editor.dirty = false,
                    Err(_) => failed = true,
                }
            }
            if let Err(e) =
                storage::save(&storage::data_dir().join("settings.json"), &self.settings)
            {
                self.log(format!("Speichern vor dem Schließen fehlgeschlagen: {e:#}"));
                failed = true;
            }
            if failed {
                ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
                self.log("Die App bleibt geöffnet, damit Änderungen erhalten bleiben.");
            }
        }
        if let Some(index) = self.confirm_trash {
            egui::Window::new("Transkript in den Papierkorb verschieben?").collapsible(false).resizable(false).show(ctx,|ui|{
                ui.label("Der Bibliothekseintrag bleibt im App-Papierkorb wiederherstellbar. Quelldatei und Exporte bleiben erhalten.");
                ui.horizontal(|ui|{if ui.button("Verschieben").clicked(){if let Err(e)=self.library.trash(index){self.log(format!("Papierkorb: {e:#}"));}self.confirm_trash=None;}
                if ui.button("Abbrechen").clicked(){self.confirm_trash=None;}});
            });
        }
        if self.close_pending {
            egui::Window::new("Laufende Aufgabe")
                .collapsible(false)
                .show(ctx, |ui| {
                    ui.label("Aufgabe abbrechen und danach schließen?");
                    ui.horizontal(|ui| {
                        if ui.button("Abbrechen und schließen").clicked() {
                            self.cancel.store(true, Ordering::Relaxed);
                            self.auto_start = false;
                            self.exit_after_cancel = true;
                            if !self.busy {
                                self.close_pending = false;
                                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                            }
                        }
                        if ui.button("Weiterarbeiten").clicked() {
                            self.close_pending = false;
                        }
                    });
                });
        }
        if self.dirty_settings && self.last_save.elapsed() > Duration::from_secs(1) {
            match storage::save(&storage::data_dir().join("settings.json"), &self.settings) {
                Ok(()) => self.dirty_settings = false,
                Err(e) => self.log(format!("Einstellungen: {e:#}")),
            };
            self.last_save = Instant::now();
        }
    }
    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        self.cancel.store(true, Ordering::Relaxed);
        let _ = storage::save(&storage::data_dir().join("settings.json"), &self.settings);
        for editor in self.editors.values() {
            if editor.dirty {
                let _ = self.library.store(&editor.doc);
            }
        }
    }
}
fn reveal(path: &Path) {
    let _ = std::process::Command::new("/usr/bin/open")
        .arg("-R")
        .arg(path)
        .spawn();
}
