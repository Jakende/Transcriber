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
    let samples = decode_inner(path, cancel)?;
    ensure!(
        samples.iter().all(|s| s.is_finite()),
        "Audiospur enthält ungültige Pegel"
    );
    Ok(samples)
}
fn decode_inner(path: &Path, cancel: &AtomicBool) -> Result<Vec<f32>> {
    let extension = path
        .extension()
        .unwrap_or_default()
        .to_string_lossy()
        .to_lowercase();
    if ["wma", "wmv", "asf"].contains(&extension.as_str()) {
        return crate::asf::decode(path, cancel);
    }
    if [
        "avi", "flv", "ts", "mts", "m2ts", "mpg", "mpeg", "vob", "m2p",
    ]
    .contains(&extension.as_str())
    {
        return crate::containers::decode(path, cancel);
    }

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
    if let Some(track) = format
        .tracks()
        .iter()
        .find(|t| t.codec_params.codec == symphonia::core::codecs::CODEC_TYPE_OPUS)
    {
        // Preserve the first audio track, rather than silently selecting a later supported track.
        let first_audio = format
            .tracks()
            .iter()
            .find(|t| t.codec_params.codec != symphonia::core::codecs::CODEC_TYPE_NULL);
        if first_audio.is_some_and(|t| t.id == track.id) {
            let id = track.id;
            let params = track.codec_params.clone();
            return decode_opus(&mut *format, id, &params, cancel);
        }
    }
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
    resample_to(input, rate, RATE, cancel)
}
pub fn resample_to(input: &[f32], rate: u32, target: u32, cancel: &AtomicBool) -> Result<Vec<f32>> {
    ensure!(rate > 0 && target > 0, "Ungültige Abtastrate.");
    if rate == target {
        return Ok(input.to_vec());
    }
    let mut converter = FftFixedIn::<f32>::new(rate as usize, target as usize, 1024, 2, 1)?;
    let delay = converter.output_delay();
    let expected = (input.len() as f64 * target as f64 / rate as f64).round() as usize;
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

/// Shorter phrase groups let automatic DE/EN detection follow language changes at pauses.
pub fn language_chunks(samples: &[f32], cancel: &AtomicBool) -> Result<Vec<(usize, usize)>> {
    let speech = crate::vad::speech_regions(samples, cancel)?;
    let mut ranges = vec![];
    let mut start = 0;
    for pair in speech.windows(2) {
        let gap = pair[1].0.saturating_sub(pair[0].1);
        let at = (pair[0].1 + pair[1].0) / 2;
        if gap >= RATE as usize / 4 && at - start >= RATE as usize * 6 {
            ranges.extend(
                chunks(&samples[start..at])
                    .into_iter()
                    .map(|(a, b)| (a + start, b + start)),
            );
            start = at;
        }
    }
    ranges.extend(
        chunks(&samples[start..])
            .into_iter()
            .map(|(a, b)| (a + start, b + start)),
    );
    Ok(ranges)
}

/// Encode a mono transcription buffer as a standard MP3 without an external encoder.
pub fn write_mp3(path: &Path, samples: &[f32], cancel: &AtomicBool) -> Result<()> {
    use audio2mp3_core::{Bitrate, EncoderConfig, Mp3Encoder};
    use std::io::{Seek, SeekFrom, Write};
    let pcm = resample_to(samples, RATE, 44100, cancel)?;
    let part = path.with_extension(format!("{}.part", uuid::Uuid::new_v4()));
    let result = (|| -> Result<()> {
        let mut encoder = Mp3Encoder::new(EncoderConfig {
            sample_rate: 44100,
            channels: 1,
            bitrate: Bitrate::Kbps128,
        })?;
        encoder.enable_info_frame()?;
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&part)?;
        let mut bytes = Vec::new();
        for chunk in pcm.chunks(44100) {
            check_cancel(cancel)?;
            let input = chunk
                .iter()
                .map(|v| (v.clamp(-1., 1.) * 32767.).round() as i16)
                .collect::<Vec<_>>();
            bytes.clear();
            encoder.encode(&input, &mut bytes)?;
            file.write_all(&bytes)?;
        }
        bytes.clear();
        encoder.flush(&mut bytes)?;
        file.write_all(&bytes)?;
        let mut info = vec![0; encoder.info_frame_size()];
        encoder.info_frame_into(&mut info)?;
        file.seek(SeekFrom::Start(0))?;
        file.write_all(&info)?;
        file.sync_all()?;
        drop(file);
        // Reserve destination without replacing another file, including concurrent exports.
        let reservation = std::fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(path)?;
        if let Err(e) = std::fs::rename(&part, path) {
            drop(reservation);
            let _ = std::fs::remove_file(path);
            return Err(e.into());
        }
        Ok(())
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(part);
    }
    result
}

fn decode_opus(
    format: &mut dyn symphonia::core::formats::FormatReader,
    id: u32,
    params: &symphonia::core::codecs::CodecParameters,
    cancel: &AtomicBool,
) -> Result<Vec<f32>> {
    let head = params.extra_data.as_deref().unwrap_or_default();
    let channels = params
        .channels
        .map(|v| v.count())
        .or_else(|| head.get(9).map(|&v| v as usize))
        .context("Opus-Kanalzahl fehlt.")?;
    ensure!(
        channels == 1 || channels == 2,
        "Opus unterstützt hier Mono oder Stereo."
    );
    let mut decoder = opus_decoder::OpusDecoder::new(48000, channels)?;
    let preskip = if head.starts_with(b"OpusHead") && head.len() >= 19 {
        u16::from_le_bytes([head[10], head[11]]) as usize
    } else {
        0
    };
    let gain = if head.starts_with(b"OpusHead") && head.len() >= 19 {
        10f32.powf(i16::from_le_bytes([head[16], head[17]]) as f32 / 256. / 20.)
    } else {
        1.
    };
    let mut pcm = vec![0.; decoder.max_frame_size_per_channel() * channels];
    let mut samples = vec![];
    loop {
        check_cancel(cancel)?;
        let packet = match format.next_packet() {
            Ok(packet) => packet,
            Err(Error::IoError(e)) if e.kind() == std::io::ErrorKind::UnexpectedEof => break,
            Err(e) => return Err(e.into()),
        };
        if packet.track_id() != id {
            continue;
        }
        let frames = decoder.decode_float(&packet.data, &mut pcm, false)?;
        samples.extend(
            pcm[..frames * channels]
                .chunks(channels)
                .map(|frame| gain * frame.iter().sum::<f32>() / channels as f32),
        );
        ensure!(
            samples.len() < 48000 * 3600 * 8,
            "Datei ist länger als acht Stunden."
        );
    }
    ensure!(samples.len() > preskip, "Opus-Audiospur ist leer.");
    resample(&samples[preskip..], 48000, cancel)
}
