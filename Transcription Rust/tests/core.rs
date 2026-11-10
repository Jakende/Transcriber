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
