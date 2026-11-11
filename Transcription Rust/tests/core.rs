use std::{path::Path, sync::atomic::AtomicBool};
use transcription_rust::{
    audio,
    document::{self, Document, Format, Segment},
    engine, speakers,
    storage::{self, History},
};
fn sample() -> Document {
    let mut d = Document::new(Path::new("Stimmen: „Grüße“.mp3"), "small", "de", "cpu");
    d.segments = vec![Segment::new(
        59.9996,
        62.4,
        "Grüße, Welt!\nEin zweiter Satz.".into(),
    )];
    d.segments[0].speaker = Some("S1".into());
    d.speaker_names.insert("S1".into(), "Jakob".into());
    d
}
#[test]
fn yaml_roundtrip_and_quoting() {
    let d = sample();
    let text = d.render(Format::Markdown).unwrap();
    let front = text
        .strip_prefix("---\n")
        .unwrap()
        .split("\n---\n")
        .next()
        .unwrap();
    let yaml: serde_yaml::Value = serde_yaml::from_str(front).unwrap();
    assert_eq!(yaml["source_file"].as_str(), Some(d.source_file.as_str()));
    for key in [
        "created",
        "model",
        "device",
        "source_file",
        "fps_timecode",
        "timecodes",
    ] {
        assert!(!yaml[key].is_null(), "{key}");
    }
    assert!(text.contains("**Jakob:**"));
    assert!(text.contains("[00:01:00:00]"));
}
#[test]
fn subtitle_roundtrip_with_multiline_unicode() {
    let d = sample();
    for format in [Format::Vtt, Format::Srt] {
        let rendered = d.render(format).unwrap();
        assert!(rendered.contains("00:01:00"));
        let parsed = document::import_subtitles(&rendered, Path::new("sample.wav")).unwrap();
        assert_eq!(parsed.segments.len(), 1);
        assert_eq!(parsed.segments[0].start, 60.);
        assert!(parsed.segments[0].text.contains("Grüße"));
    }
}
#[test]
fn parse_cue_settings_voice_tags_and_notes() {
    let text = "\u{feff}WEBVTT\r\n\r\nNOTE metadata\r\nignore\r\n\r\nid\r\n00:01.500 --> 00:03.000 align:start\r\n<v Anna>Grüße &amp; <b>Welt</b></v>\r\n";
    let d = document::import_subtitles(text, Path::new("demo.vtt")).unwrap();
    assert_eq!(d.segments[0].speaker.as_deref(), Some("Anna"));
    assert_eq!(d.segments[0].text, "Grüße & Welt");
    assert_eq!(d.segments[0].start, 1.5);
    assert!(document::import_subtitles("00:03.000 --> 00:01.000\nBad", Path::new("a")).is_err());
}
#[test]
fn export_preserves_existing_files() {
    let dir = tempfile::tempdir().unwrap();
    let mut d = sample();
    let formats = [Format::Markdown, Format::Csv];
    d.export_new(dir.path(), &formats).unwrap();
    let first = d.outputs.clone();
    std::fs::write(&first["md"], "Meine Änderungen").unwrap();
    d.export_new(dir.path(), &formats).unwrap();
    assert_ne!(first["md"], d.outputs["md"]);
    assert_eq!(
        std::fs::read_to_string(&first["md"]).unwrap(),
        "Meine Änderungen"
    );
    assert_eq!(
        std::fs::read_dir(dir.path())
            .unwrap()
            .filter(|e| !e
                .as_ref()
                .unwrap()
                .file_name()
                .to_string_lossy()
                .starts_with("._"))
            .count(),
        4
    );
}
#[test]
fn unicode_editing_undo_and_merge() {
    let mut d = sample();
    let original = d.clone();
    let mut history = History::default();
    history.record(&d);
    d.split(0, 61., 7).unwrap();
    assert_eq!(d.segments.len(), 2);
    assert_eq!(d.segments[0].text, "Grüße,");
    assert_eq!(d.segments[1].start, 61.);
    history.undo(&mut d);
    assert_eq!(d, original);
    history.redo(&mut d);
    assert_eq!(d.segments.len(), 2);
    d.merge(0).unwrap();
    assert_eq!(d.segments.len(), 1);
    assert_eq!(d.segments[0].end, 62.4);
    d.speaker_names.insert("S2".into(), "Zweite Stimme".into());
    d.merge_speakers("S1", "S2");
    assert_eq!(d.segments[0].speaker.as_deref(), Some("S2"));
}
#[test]
fn invalid_segments_are_rejected() {
    let mut d = sample();
    d.segments[0].start = f64::NAN;
    assert!(d.render(Format::Markdown).is_err());
    d = sample();
    d.fps_timecode = 0;
    assert!(d.render(Format::Vtt).is_err());
}
#[test]
fn csv_quotes_newlines_and_commas() {
    let d = sample();
    let csv = d.render(Format::Csv).unwrap();
    let mut reader = csv::Reader::from_reader(csv.as_bytes());
    let row = reader.records().next().unwrap().unwrap();
    assert_eq!(&row[2], "Jakob");
    assert_eq!(&row[4], d.segments[0].text.as_str());
}
#[test]
fn atomic_save_and_json_compatibility() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("a.json");
    let d = sample();
    storage::save(&path, &d).unwrap();
    let loaded: Document = storage::load(&path).unwrap();
    assert_eq!(loaded, d);
    storage::save(&path, &loaded).unwrap();
    assert_eq!(
        std::fs::read_dir(dir.path())
            .unwrap()
            .filter(|e| !e
                .as_ref()
                .unwrap()
                .file_name()
                .to_string_lossy()
                .starts_with("._"))
            .count(),
        1
    );
}
#[test]
fn rust_audio_decode_and_resample() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("test.wav");
    let rate = 44100;
    let mut wav = hound::WavWriter::create(
        &path,
        hound::WavSpec {
            channels: 2,
            sample_rate: rate,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        },
    )
    .unwrap();
    for n in 0..rate {
        let value =
            ((n as f32 * 440. * 2. * std::f32::consts::PI / rate as f32).sin() * 16000.) as i16;
        wav.write_sample(value).unwrap();
        wav.write_sample(value).unwrap();
    }
    wav.finalize().unwrap();
    let samples = audio::decode(&path, &AtomicBool::new(false)).unwrap();
    assert_eq!(samples.len(), 16000);
    let rms = (samples[1000..15000].iter().map(|v| v * v).sum::<f32>() / 14000.).sqrt();
    assert!((rms - 0.345).abs() < 0.02, "{rms}");
    assert!(audio::decode(&path, &AtomicBool::new(true)).is_err());
}
#[test]
fn chunking_covers_every_sample_once() {
    let samples = vec![0.1; 16000 * 65];
    let chunks = audio::chunks(&samples);
    assert!(chunks.len() >= 3);
    assert_eq!(chunks[0].0, 0);
    assert_eq!(chunks.last().unwrap().1, samples.len());
    for pair in chunks.windows(2) {
        assert_eq!(pair[0].1, pair[1].0);
    }
    for (a, b) in chunks {
        assert!(b > a && b - a <= 16000 * 30);
    }
}
#[test]
fn whisper_filters_match_openai_reference() {
    // Sampled peaks and neighbors from OpenAI Whisper assets/mel_filters.npz (MIT).
    let reference: std::collections::BTreeMap<String, Vec<(usize, f32)>> =
        serde_json::from_str(include_str!("fixtures/whisper-mel-reference.json")).unwrap();
    for bins in [80, 128] {
        let filters = engine::mel_filters(bins);
        assert_eq!(filters.len(), bins * 201);
        for &(index, expected) in &reference[&bins.to_string()] {
            assert!(
                (filters[index] - expected).abs() < 2e-7,
                "{bins} bins at {index}: {} != {expected}",
                filters[index]
            );
        }
    }
}
#[test]
fn speaker_features_and_clusters() {
    let samples: Vec<_> = (0..16000)
        .map(|n| (n as f32 * 440. * 2. * std::f32::consts::PI / 16000.).sin())
        .collect();
    let (f, frames) = speakers::fbank(&samples);
    assert_eq!(f.len(), 80 * frames);
    assert!(f.iter().all(|v| v.is_finite()));
    for row in f.chunks(frames) {
        assert!(row.iter().sum::<f32>().abs() / (frames as f32) < 0.001);
    }
    let vectors = vec![
        vec![1., 0.],
        vec![0.99, 0.01],
        vec![0., 1.],
        vec![0.01, 0.99],
    ];
    assert_eq!(speakers::cluster(&vectors, 2), vec![0, 0, 1, 1]);
    assert_eq!(speakers::cluster(&vectors, 0), vec![0, 0, 1, 1]);
}

#[test]
fn discovery_reuses_ggml_in_place_and_rejects_wrong_model() {
    use transcription_rust::{discovery, models::ModelSize};
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("ggml-small.bin");
    let header = [
        0x67676d6c_u32,
        51865,
        1500,
        768,
        12,
        12,
        448,
        768,
        12,
        12,
        80,
        1,
    ];
    let bytes = header
        .into_iter()
        .flat_map(u32::to_le_bytes)
        .collect::<Vec<_>>();
    std::fs::write(&path, &bytes).unwrap();
    let before = std::fs::metadata(&path).unwrap().len();
    let found = discovery::discover_in(ModelSize::Small, &[dir.path().into()]);
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].weights, path.canonicalize().unwrap());
    assert_eq!(found[0].format, discovery::WeightFormat::Ggml);
    assert!(discovery::discover_in(ModelSize::Medium, &[dir.path().into()]).is_empty());
    assert_eq!(std::fs::metadata(&path).unwrap().len(), before);
    assert!(!dir.path().join("model.safetensors").exists());
    std::fs::write(&path, b"partial download").unwrap();
    assert!(discovery::discover_in(ModelSize::Small, &[dir.path().into()]).is_empty());
}
#[test]
fn discovery_deduplicates_roots_and_resolves_huggingface_links() {
    use transcription_rust::{discovery, models::ModelSize};
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("models--openai--whisper-tiny");
    let snapshot = root.join("snapshots/revision");
    let blobs = root.join("blobs");
    std::fs::create_dir_all(&snapshot).unwrap();
    std::fs::create_dir_all(&blobs).unwrap();
    let header=serde_json::json!({"model.encoder.conv1.weight":{"shape":[384,80,3]},"model.encoder.layers.3.fc1.weight":{},"model.decoder.layers.3.fc1.weight":{}}).to_string();
    let mut bytes = (header.len() as u64).to_le_bytes().to_vec();
    bytes.extend(header.as_bytes());
    bytes.extend(vec![0; 1024]);
    std::fs::write(blobs.join("weights"), bytes).unwrap();
    std::fs::write(snapshot.join("model.safetensors"), "../../blobs/weights").unwrap();
    let found = discovery::discover_in(ModelSize::Tiny, &[dir.path().into(), dir.path().into()]);
    assert_eq!(found.len(), 1);
    assert_eq!(
        found[0].weights,
        blobs.join("weights").canonicalize().unwrap()
    );
}
#[test]
fn whisper_legacy_tensor_names_map_to_candle() {
    use transcription_rust::ggml::tensor_name;
    assert_eq!(
        tensor_name("decoder.blocks.2.cross_attn.query.weight"),
        "model.decoder.layers.2.encoder_attn.q_proj.weight"
    );
    assert_eq!(
        tensor_name("encoder.blocks.0.attn_ln.weight"),
        "model.encoder.layers.0.self_attn_layer_norm.weight"
    );
    assert_eq!(
        tensor_name("encoder.positional_embedding"),
        "model.encoder.embed_positions.weight"
    );
    assert_eq!(
        tensor_name("decoder.token_embedding.weight"),
        "model.decoder.embed_tokens.weight"
    );
}

#[test]
fn imported_swift_document_preserves_regions_and_unknown_metadata() {
    let mut value = serde_json::to_value(sample()).unwrap();
    value["speaker_regions"] = serde_json::json!([{"start":0.0,"end":2.0,"speaker":"S1"},{"start":2.1,"end":4.0,"speaker":"S2"}]);
    value["speaker_names"] = serde_json::json!({"S1":"Anna","S2":"Jakob"});
    value["future_metadata"] = serde_json::json!({"language_reviewed":true});
    let mut doc: Document = serde_json::from_value(value).unwrap();
    doc.merge_speakers("S1", "S2");
    assert_eq!(doc.speaker_regions.len(), 1);
    assert_eq!(doc.speaker_regions[0].end, 4.);
    assert_eq!(doc.speaker_regions[0].speaker, "S2");
    let stored = serde_json::to_value(&doc).unwrap();
    assert_eq!(stored["future_metadata"]["language_reviewed"], true);
    let before = doc.clone();
    doc.merge_speakers("S2", "S2");
    doc.merge_speakers("S2", "missing");
    assert_eq!(doc, before);
}
#[test]
fn vtt_voice_and_literal_markup_survive_roundtrip() {
    let mut doc = sample();
    doc.speaker_names.insert("S1".into(), "Anna & Jakob".into());
    doc.segments[0].text = "5 < 7 & 9 > 8".into();
    let encoded = doc.render(Format::Vtt).unwrap();
    let decoded = document::import_subtitles(&encoded, Path::new("audio.wav")).unwrap();
    assert_eq!(decoded.segments[0].text, doc.segments[0].text);
    assert_eq!(decoded.segments[0].speaker.as_deref(), Some("Anna & Jakob"));
}
#[test]
fn neural_vad_silence_and_hangover() {
    use transcription_rust::vad;
    assert!(
        vad::speech_regions(&vec![0.; 16000], &AtomicBool::new(false))
            .unwrap()
            .is_empty()
    );
    assert!(vad::speech_regions(&[0.; 512], &AtomicBool::new(true)).is_err());
    let mut probabilities = vec![0.; 10];
    probabilities.extend(vec![0.9; 20]);
    probabilities.extend(vec![0.1; 20]);
    let regions = vad::regions_from_probs(&probabilities, 50 * 512);
    assert_eq!(regions.len(), 1);
    assert_eq!(regions[0], (10 * 512 - 480, 30 * 512 + 480));
}
#[test]
fn pure_rust_mp3_is_decodable_and_never_replaces_existing_file() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("encoded.mp3");
    let pcm = (0..16000)
        .map(|i| (i as f32 * 440. * 2. * std::f32::consts::PI / 16000.).sin() * 0.35)
        .collect::<Vec<_>>();
    audio::write_mp3(&path, &pcm, &AtomicBool::new(false)).unwrap();
    let bytes = std::fs::read(&path).unwrap();
    let decoded = audio::decode(&path, &AtomicBool::new(false)).unwrap();
    assert!((decoded.len() as isize - 16000).abs() < 2500);
    assert!(decoded.iter().any(|v| v.abs() > 0.05));
    assert!(audio::write_mp3(&path, &pcm, &AtomicBool::new(false)).is_err());
    assert_eq!(std::fs::read(&path).unwrap(), bytes);
    let other = dir.path().join("cancelled.mp3");
    assert!(audio::write_mp3(&other, &pcm, &AtomicBool::new(true)).is_err());
    assert!(!other.exists());
}
#[test]
fn neural_vad_detects_real_speech_and_ignores_silence() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("speech.wav");
    std::fs::write(&path, include_bytes!("fixtures/jfk-speech.wav")).unwrap();
    let cancel = AtomicBool::new(false);
    let pcm = audio::decode(&path, &cancel).unwrap();
    let regions = transcription_rust::vad::speech_regions(&pcm, &cancel).unwrap();
    assert!(!regions.is_empty(), "Real JFK speech must not be discarded");
    assert!(regions.iter().map(|(a, b)| b - a).sum::<usize>() > 16000);
}
#[test]
fn glossary_replacements_preserve_word_boundaries_and_literal_dollars() {
    let edits = std::collections::BTreeMap::from([
        ("new york".into(), "New York City".into()),
        ("York".into(), "YORK".into()),
        ("grüße".into(), "Gruß $1".into()),
    ]);
    assert_eq!(
        transcription_rust::glossary::apply("NEW YORK, Yorker und Grüße!", &edits),
        "New YORK City, Yorker und Gruß $1!"
    );
}
#[test]
fn rss_feed_preserves_titles_unicode_and_audio_selection() {
    let rss=br#"<?xml version="1.0"?><rss version="2.0"><channel><title>Test &amp; Talk</title><item><title>Episode One</title><enclosure url="https://example.org/one.mp3" type="audio/mpeg" length="12"/></item><item><title>Missing enclosure</title></item></channel></rss>"#;
    let episodes = transcription_rust::media::parse_feed(rss, "https://example.org/feed").unwrap();
    assert_eq!(episodes.len(), 1);
    assert_eq!(episodes[0].show, "Test & Talk");
    assert_eq!(episodes[0].audio_url, "https://example.org/one.mp3");
}
#[test]
fn download_validates_completion_and_can_cancel_a_stalled_server() {
    use std::{
        io::{Read, Write},
        net::TcpListener,
        sync::{Arc, atomic::Ordering},
        time::{Duration, Instant},
    };
    let dir = tempfile::tempdir().unwrap();
    let client = transcription_rust::models::client().unwrap();
    let serve = |truncated: bool| {
        let server = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = server.local_addr().unwrap();
        let thread = std::thread::spawn(move || {
            let (mut stream, _) = server.accept().unwrap();
            let mut request = [0; 2048];
            let _ = stream.read(&mut request);
            let content = if truncated {
                "HTTP/1.1 200 OK\r\nContent-Length: 100\r\nConnection: close\r\n\r\npartial"
            } else {
                "HTTP/1.1 200 OK\r\nContent-Length: 5\r\nConnection: close\r\n\r\nhello"
            };
            stream.write_all(content.as_bytes()).unwrap();
        });
        (format!("http://{address}/audio"), thread)
    };
    let (url, thread) = serve(false);
    let path = dir.path().join("audio.bin");
    transcription_rust::models::download(
        &client,
        &url,
        &path,
        &AtomicBool::new(false),
        &mut |_| {},
    )
    .unwrap();
    thread.join().unwrap();
    assert_eq!(std::fs::read(&path).unwrap(), b"hello");
    let (url, thread) = serve(true);
    let bad = dir.path().join("bad.bin");
    assert!(
        transcription_rust::models::download(
            &client,
            &url,
            &bad,
            &AtomicBool::new(false),
            &mut |_| {}
        )
        .is_err()
    );
    thread.join().unwrap();
    assert!(!bad.exists());
    let server = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = server.local_addr().unwrap();
    let cancel = Arc::new(AtomicBool::new(false));
    let signal = cancel.clone();
    let thread = std::thread::spawn(move || {
        let (_stream, _) = server.accept().unwrap();
        std::thread::sleep(Duration::from_millis(150));
        signal.store(true, Ordering::Relaxed);
        std::thread::sleep(Duration::from_millis(500));
    });
    let start = Instant::now();
    let stalled = dir.path().join("stalled.bin");
    assert!(
        transcription_rust::models::download(
            &client,
            &format!("http://{address}/stalled"),
            &stalled,
            &cancel,
            &mut |_| {}
        )
        .is_err()
    );
    assert!(start.elapsed() < Duration::from_secs(2));
    thread.join().unwrap();
    assert!(!stalled.exists());
    assert!(
        !std::fs::read_dir(dir.path()).unwrap().any(|e| e
            .unwrap()
            .file_name()
            .to_string_lossy()
            .ends_with(".part"))
    );
}

#[test]
fn word_split_uses_whole_words_and_preserves_speaker_on_merge() {
    let mut d = sample();
    d.segments[0].text = "Äpfel Birnen schöne Grüße".into();
    d.segments[0].start = 0.;
    d.segments[0].end = 8.;
    d.split_words(0, None).unwrap();
    assert_eq!(d.segments[0].text, "Äpfel Birnen");
    assert_eq!(d.segments[1].text, "schöne Grüße");
    assert_eq!(d.segments[0].end, 4.);
    d.segments[0].speaker = None;
    d.merge(0).unwrap();
    assert_eq!(d.segments[0].speaker.as_deref(), Some("S1"));
    d.segments[0].text = "Einzelwort".into();
    assert!(d.split_words(0, None).is_err());
}
#[test]
fn speaker_ranges_separation_and_overlapping_windows() {
    let cancel = AtomicBool::new(false);
    let vectors = vec![
        vec![1., 0.],
        vec![0.98, 0.02],
        vec![0., 1.],
        vec![0.02, 0.98],
    ];
    assert_eq!(
        speakers::cluster_bounded(&vectors, 2, 2, 0.6, &cancel).unwrap(),
        vec![0, 0, 1, 1]
    );
    assert_eq!(
        speakers::cluster_bounded(&vectors, 1, 1, 0.6, &cancel).unwrap(),
        vec![0, 0, 0, 0]
    );
    assert_eq!(
        speakers::cluster_bounded(&vectors, 3, 4, 0.6, &cancel)
            .unwrap()
            .iter()
            .collect::<std::collections::BTreeSet<_>>()
            .len(),
        3
    );
    assert_eq!(speakers::speaker_bounds("2-4", 0).unwrap(), (2, 4));
    assert!(speakers::speaker_bounds("4-2", 0).is_err());
    assert_eq!(
        speakers::speech_windows(0, 48000),
        vec![(0, 24000), (12000, 36000), (24000, 48000)]
    );
    assert!(speakers::speech_windows(0, 100).is_empty());
}
#[test]
fn library_save_backup_archive_reopen_and_trash_preserve_source_and_exports() {
    let dir = tempfile::tempdir().unwrap();
    let mut library = storage::Library::open_at(dir.path().join("state")).unwrap();
    let mut d = sample();
    let source = dir.path().join("source.wav");
    std::fs::write(&source, b"original").unwrap();
    d.source_path = source.clone();
    d.export_new(&dir.path().join("exports"), &[Format::Markdown])
        .unwrap();
    let export = d.outputs["md"].clone();
    library.store(&d).unwrap();
    d.segments[0].text = "Überarbeitet".into();
    library.store(&d).unwrap();
    assert_eq!(
        std::fs::read_dir(dir.path().join("state/backups").join(&d.id))
            .unwrap()
            .filter_map(Result::ok)
            .filter(|e| !e.file_name().to_string_lossy().starts_with("._"))
            .count(),
        1
    );
    library.records[0].archived = true;
    library.persist().unwrap();
    let mut reopened = storage::Library::open_at(dir.path().join("state")).unwrap();
    assert!(reopened.records[0].archived);
    assert_eq!(
        storage::load::<Document>(&reopened.records[0].path)
            .unwrap()
            .segments[0]
            .text,
        "Überarbeitet"
    );
    let damaged = reopened.records[0].path.clone();
    std::fs::write(&damaged, b"corrupted").unwrap();
    let (recovered, backup) = reopened.read_document(&damaged).unwrap();
    assert!(backup.is_some());
    assert_eq!(recovered.segments[0].text, sample().segments[0].text);
    assert_eq!(std::fs::read(&damaged).unwrap(), b"corrupted");
    reopened.trash(0).unwrap();
    assert!(reopened.records.is_empty());
    assert!(
        dir.path()
            .join("state/Papierkorb")
            .join(format!("{}.json", d.id))
            .is_file()
    );
    assert_eq!(std::fs::read(source).unwrap(), b"original");
    assert!(Path::new(&export).is_file());
}
#[test]
fn podcast_itunes_metadata_survives_import_and_markdown_export() {
    let rss=br#"<rss version="2.0" xmlns:itunes="http://www.itunes.com/dtds/podcast-1.0.dtd"><channel><title>Show</title><language>de</language><itunes:author>Anna</itunes:author><description>Show description</description><item><guid>unique-42</guid><title>Episode</title><description>Episode description</description><itunes:episode>42</itunes:episode><itunes:season>3</itunes:season><itunes:duration>01:02:03</itunes:duration><itunes:explicit>yes</itunes:explicit><enclosure url="https://example.org/a.mp3" type="audio/mpeg" length="12"/></item></channel></rss>"#;
    let episodes = transcription_rust::media::parse_feed(rss, "https://example.org/feed").unwrap();
    let e = &episodes[0];
    assert_eq!(e.metadata["guid"], "unique-42");
    assert_eq!(e.metadata["episode_number"], 42);
    assert_eq!(e.metadata["season_number"], 3);
    assert_eq!(e.metadata["duration_seconds"], 3723);
    assert_eq!(e.metadata["explicit"], true);
    assert_eq!(e.metadata["language"], "de");
    let mut d = sample();
    d.podcast = Some(serde_json::to_value(e).unwrap());
    let md = d.render(Format::Markdown).unwrap();
    assert!(md.contains("episode_number: 42"));
    assert!(md.contains("duration_seconds: 3723"));
}

#[test]
#[ignore = "Downloads the pinned 539 MB NER model once; run explicitly for release validation"]
fn multilingual_neural_terms_recognize_people_places_and_organizations() {
    let text = "Angela Merkel besuchte Berlin und sprach mit Microsoft. Barack Obama visited New York and met employees of Google. Angela Merkel traf Barack Obama in Berlin.";
    let result =
        transcription_rust::terms::analyze(text, false, &AtomicBool::new(false), &mut |_, _| {})
            .unwrap();
    for (term, kind, count) in [
        ("Angela Merkel", "Person", 2),
        ("Barack Obama", "Person", 2),
        ("Berlin", "Ort", 2),
        ("New York", "Ort", 1),
        ("Google", "Organisation", 1),
        ("Microsoft", "Organisation", 1),
    ] {
        assert!(
            result
                .iter()
                .any(|c| c.term == term && c.kind == kind && c.count == count),
            "Missing {term}: {result:?}"
        );
    }
    assert!(
        !result
            .iter()
            .any(|c| ["und", "besuchte", "met"].contains(&c.term.as_str()))
    );
}

#[test]
fn additional_media_containers_decode_real_speech_with_correct_duration_and_signal() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let cancel = AtomicBool::new(false);
    let original = audio::decode(&root.join("jfk-speech.wav"), &cancel).unwrap();
    for name in [
        "jfk.avi", "jfk.flv", "jfk.ts", "jfk.mpg", "jfk.wma", "jfk.opus", "jfk.webm", "jfk.m4a",
    ] {
        let pcm =
            audio::decode(&root.join(name), &cancel).unwrap_or_else(|e| panic!("{name}: {e:#}"));
        assert!(
            (pcm.len() as f64 / 16000. - 4.).abs() < 0.12,
            "{name} duration: {}",
            pcm.len()
        );
        let mut best = 0f64;
        for lag in -2048isize..2048 {
            let (mut dot, mut aa, mut bb) = (0., 0., 0.);
            for i in (3000..original.len().min(pcm.len()).min(30000) - 3000).step_by(16) {
                let a = original[i] as f64;
                let b = pcm[(i as isize + lag) as usize] as f64;
                dot += a * b;
                aa += a * a;
                bb += b * b;
            }
            best = best.max(dot / (aa * bb).sqrt().max(1e-20));
        }
        assert!(best > 0.90, "{name}: signal correlation {best}");
    }
}
#[test]
fn rust_mp4_mux_keeps_video_and_audio_without_overwriting_existing_output() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let dir = tempfile::tempdir().unwrap();
    let output = dir.path().join("combined.mp4");
    let cancel = AtomicBool::new(false);
    transcription_rust::containers::mux_mp4(
        &root.join("picture.mp4"),
        &root.join("jfk.m4a"),
        &output,
        &cancel,
    )
    .unwrap();
    let pcm = audio::decode(&output, &cancel).unwrap();
    assert!((pcm.len() as f64 / 16000. - 4.).abs() < 0.12);
    let before = std::fs::read(&output).unwrap();
    assert!(
        transcription_rust::containers::mux_mp4(
            &root.join("picture.mp4"),
            &root.join("jfk.m4a"),
            &output,
            &cancel
        )
        .is_err()
    );
    assert_eq!(std::fs::read(output).unwrap(), before);
}
#[test]
fn youtube_urls_visitor_configuration_and_stream_selection() {
    use transcription_rust::youtube;
    for url in [
        "https://youtu.be/jNQXAC9IVRw",
        "https://www.youtube.com/watch?v=jNQXAC9IVRw",
        "https://youtube.com/shorts/jNQXAC9IVRw",
        "https://www.youtube-nocookie.com/embed/jNQXAC9IVRw",
    ] {
        assert_eq!(youtube::video_id(url).unwrap(), "jNQXAC9IVRw");
    }
    assert!(youtube::video_id("https://youtube.com.evil.example/watch?v=jNQXAC9IVRw").is_err());
    assert!(youtube::video_id("https://youtube.com/watch?v=invalid").is_err());
    let config = youtube::configuration(
        r#"<script>ytcfg.set({"VISITOR_DATA":"anonymous-test"});ytcfg.set({"OTHER":42});</script>"#,
    );
    assert_eq!(config["VISITOR_DATA"], "anonymous-test");
    assert_eq!(config["OTHER"], 42);
    let info=youtube::parse_player(&serde_json::json!({"playabilityStatus":{"status":"OK"},"videoDetails":{"title":"Title"},"streamingData":{"adaptiveFormats":[{"url":"https://example.org/a","mimeType":"audio/mp4; codecs=mp4a","bitrate":128000},{"url":"https://example.org/v","mimeType":"video/mp4; codecs=avc1","bitrate":1000000},{"url":"file:///private","mimeType":"audio/mp4"}]}})).unwrap();
    assert_eq!(info.formats.len(), 2);
    assert!(info.formats[0].has_audio);
    assert!(!info.formats[1].has_audio);
    assert!(youtube::parse_player(&serde_json::json!({"playabilityStatus":{"status":"LOGIN_REQUIRED","reason":"Please sign in"}})).is_err());
}

#[test]
fn detected_voice_changes_split_phrase_text_at_whole_words() {
    use transcription_rust::document::SpeakerRegion;
    let mut d = sample();
    d.segments = vec![Segment::new(
        0.,
        10.,
        "Hallo schöne Welt good morning everyone".into(),
    )];
    d.speaker_regions = vec![
        SpeakerRegion {
            start: 0.,
            end: 5.,
            speaker: "A".into(),
        },
        SpeakerRegion {
            start: 5.,
            end: 10.,
            speaker: "B".into(),
        },
    ];
    speakers::split_at_speaker_changes(&mut d).unwrap();
    assert_eq!(d.segments.len(), 2);
    assert_eq!(d.segments[0].text, "Hallo schöne Welt");
    assert_eq!(d.segments[1].text, "good morning everyone");
    assert_eq!(d.segments[1].start, 5.);
}
