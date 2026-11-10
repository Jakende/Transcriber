use anyhow::{Context, Result, bail, ensure};
use rubato::{FftFixedIn, Resampler};
use std::{
    fs::File,
    path::Path,
    sync::atomic::{AtomicBool, Ordering},
};
use symphonia::core::{
    audio::SampleBuffer, codecs::DecoderOptions, errors::Error, formats::FormatOptions,
    io::MediaSourceStream, meta::MetadataOptions, probe::Hint,
};

pub const RATE: u32 = 16_000;
pub fn check_cancel(cancel: &AtomicBool) -> Result<()> {
    ensure!(!cancel.load(Ordering::Relaxed), "Abgebrochen");
    Ok(())
}
/// Decode the first supported audio track. No ffmpeg process or native codec library.
pub fn decode(path: &Path, cancel: &AtomicBool) -> Result<Vec<f32>> {
    let file =
        File::open(path).with_context(|| format!("Datei nicht lesbar: {}", path.display()))?;
    let stream = MediaSourceStream::new(Box::new(file), Default::default());
    let mut hint = Hint::new();
    if let Some(ext) = path.extension().and_then(|s| s.to_str()) {
        hint.with_extension(ext);
    }
    let probe = symphonia::default::get_probe()
        .format(
            &hint,
            stream,
            &FormatOptions::default(),
            &MetadataOptions::default(),
        )
        .context(
            "Container nicht unterstützt. Bitte WAV, MP3, FLAC, M4A/MP4, OGG oder MKV verwenden.",
        )?;
    let mut format = probe.format;
    let (track_id, mut decoder, mut rate) = format
        .tracks()
        .iter()
        .find_map(|track| {
            symphonia::default::get_codecs()
                .make(&track.codec_params, &DecoderOptions { verify: true })
                .ok()
                .map(|d| (track.id, d, track.codec_params.sample_rate.unwrap_or(RATE)))
        })
        .context("Keine unterstützte Audiospur gefunden.")?;
    let mut mono = Vec::new();
    loop {
        check_cancel(cancel)?;
        let packet = match format.next_packet() {
            Ok(p) => p,
            Err(Error::IoError(e)) if e.kind() == std::io::ErrorKind::UnexpectedEof => break,
            Err(e) => return Err(e).context("Mediendatei beschädigt oder nicht unterstützt."),
        };
        if packet.track_id() != track_id {
            continue;
        }
        let decoded = decoder
            .decode(&packet)
            .context("Audiospur konnte nicht dekodiert werden.")?;
        let spec = *decoded.spec();
        if !mono.is_empty() && spec.rate != rate {
            bail!("Wechselnde Abtastraten werden nicht unterstützt.");
        }
        rate = spec.rate;
        let channels = spec.channels.count();
        ensure!(channels > 0, "Audiospur ohne Kanäle.");
        let mut samples = SampleBuffer::<f32>::new(decoded.capacity() as u64, spec);
        samples.copy_interleaved_ref(decoded);
        mono.extend(
            samples
                .samples()
                .chunks(channels)
                .map(|frame| frame.iter().copied().sum::<f32>() / channels as f32),
        );
        ensure!(
            mono.len() < rate as usize * 3600 * 8,
            "Datei ist länger als acht Stunden. Bitte aufteilen."
        );
    }
    ensure!(!mono.is_empty(), "Die Audiospur ist leer.");
    resample(&mono, rate, cancel)
}
pub fn resample(input: &[f32], rate: u32, cancel: &AtomicBool) -> Result<Vec<f32>> {
    ensure!(rate > 0, "Ungültige Abtastrate.");
    if rate == RATE {
        return Ok(input.to_vec());
    }
    let mut converter = FftFixedIn::<f32>::new(rate as usize, RATE as usize, 1024, 2, 1)?;
    let delay = converter.output_delay();
    let expected = (input.len() as f64 * RATE as f64 / rate as f64).round() as usize;
    let mut out = Vec::with_capacity(expected + 4096);
    let mut pos = 0;
    while pos < input.len() {
        check_cancel(cancel)?;
        let count = converter.input_frames_next();
        let end = (pos + count).min(input.len());
        let mut chunk = vec![0.; count];
        chunk[..end - pos].copy_from_slice(&input[pos..end]);
        out.extend_from_slice(&converter.process(&[chunk], None)?[0]);
        pos = end;
    }
    while out.len() < expected + delay {
        let count = converter.input_frames_next();
        out.extend_from_slice(&converter.process(&[vec![0.; count]], None)?[0]);
    }
    Ok(out[delay..delay + expected].to_vec())
}
pub fn write_wav(path: &Path, samples: &[f32]) -> Result<()> {
    let mut writer = hound::WavWriter::create(
        path,
        hound::WavSpec {
            channels: 1,
            sample_rate: RATE,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        },
    )?;
    for sample in samples {
        writer.write_sample((sample.clamp(-1., 1.) * i16::MAX as f32) as i16)?;
    }
    writer.finalize()?;
    Ok(())
}
/// Quiet boundaries around 20 seconds prevent chopping words at fixed 30s boundaries.
pub fn chunks(samples: &[f32]) -> Vec<(usize, usize)> {
    let mut ranges = vec![];
    let mut start = 0;
    while start < samples.len() {
        let mut end = (start + RATE as usize * 28).min(samples.len());
        if end < samples.len() {
            let search = start + RATE as usize * 20;
            if let Some((i, _)) = samples[search..end]
                .chunks(1600)
                .enumerate()
                .map(|(i, s)| (i, s.iter().map(|v| v * v).sum::<f32>() / s.len() as f32))
                .min_by(|a, b| a.1.total_cmp(&b.1))
            {
                end = search + i * 1600 + 800;
            }
        }
        ranges.push((start, end));
        start = end;
    }
    ranges
}
