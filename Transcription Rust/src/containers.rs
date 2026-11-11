//! Additional pure-Rust media containers, bridged to the existing audio decoders.
use anyhow::{Context, Result, ensure};
use oxideav_core::{CodecId, CodecResolver, CodecTag, Demuxer, MediaType, ProbeContext};
use std::{fs::File, path::Path, sync::atomic::AtomicBool};
use symphonia::core::{
    audio::{Channels, SampleBuffer},
    codecs::{self, CodecParameters, DecoderOptions},
    formats::Packet,
};
struct Resolver;
impl CodecResolver for Resolver {
    fn resolve_tag(&self, ctx: &ProbeContext) -> Option<CodecId> {
        let name = match ctx.tag {
            CodecTag::WaveFormat(0x55) => "mp3",
            CodecTag::WaveFormat(0x50) => "mp2",
            CodecTag::WaveFormat(1) => "pcm_s16le",
            CodecTag::WaveFormat(3) => "pcm_f32le",
            CodecTag::WaveFormat(0xff | 0x1610) => "aac",
            CodecTag::WaveFormat(0x160) => "wmav1",
            CodecTag::WaveFormat(0x161) => "wma2",
            CodecTag::Fourcc(tag) if *tag == *b"AVC1" || *tag == *b"AVC3" => "h264",
            CodecTag::Fourcc(tag) if *tag == *b"HVC1" || *tag == *b"HEV1" => "hevc",
            CodecTag::Fourcc(tag) if *tag == *b"AV01" => "av1",
            CodecTag::Fourcc(tag) if *tag == *b"VP09" => "vp9",
            CodecTag::Fourcc(tag) if *tag == *b"MP4A" => "aac",
            CodecTag::Mp4ObjectType(0x40) => "aac",
            CodecTag::Mp4ObjectType(0x69 | 0x6b) => "mp3",
            _ => return None,
        };
        Some(CodecId::new(name))
    }
}
pub fn decode(path: &Path, cancel: &AtomicBool) -> Result<Vec<f32>> {
    use std::io::Read;
    let mut header = [0u8; 5];
    let _ = File::open(path)?.read(&mut header)?;
    if header[..4] == [0, 0, 1, 0xba] && header[4] & 0xf0 == 0x20 {
        return crate::mpeg1::decode(path, cancel);
    }
    let input = Box::new(File::open(path)?);
    let extension = path
        .extension()
        .unwrap_or_default()
        .to_string_lossy()
        .to_lowercase();
    let demux: Box<dyn Demuxer> = match extension.as_str() {
        "avi" => oxideav_avi::demuxer::open(input, &Resolver)?,
        "flv" => oxideav_flv::demuxer::open(input, &Resolver)?,
        "ts" | "mts" | "m2ts" => oxideav_mpegts::demuxer::open(input, &Resolver)?,
        "mpg" | "mpeg" | "vob" | "m2p" => oxideav_mpegts::ps::open(input, &Resolver)?,
        _ => anyhow::bail!("Unbekannter zusätzlicher Mediencontainer"),
    };
    decode_packets(demux, cancel)
}
fn decode_packets(mut demux: Box<dyn Demuxer>, cancel: &AtomicBool) -> Result<Vec<f32>> {
    let stream = demux
        .streams()
        .iter()
        .find(|s| s.params.media_type == MediaType::Audio)
        .context("Keine Audiospur im Video gefunden")?
        .clone();
    let p = &stream.params;
    let mut params = CodecParameters::new();
    params.codec = match p.codec_id.as_str() {
        "mp3" => codecs::CODEC_TYPE_MP3,
        "mp2" | "mpa" => codecs::CODEC_TYPE_MP2,
        "aac" => codecs::CODEC_TYPE_AAC,
        "pcm_s16le" => codecs::CODEC_TYPE_PCM_S16LE,
        "pcm_f32le" => codecs::CODEC_TYPE_PCM_F32LE,
        other => anyhow::bail!("Audiospur {other} wird noch nicht unterstützt"),
    };
    params.sample_rate = p.sample_rate;
    if let Some(count) = p.channels {
        ensure!(count > 0 && count <= 8, "Ungültige Kanalzahl");
        params.channels = Channels::from_bits((1u32 << count) - 1);
    }
    params.extra_data = (!p.extradata.is_empty()).then(|| p.extradata.clone().into_boxed_slice());
    let mut decoder = symphonia::default::get_codecs().make(&params, &DecoderOptions::default())?;
    let mut mono = vec![];
    let mut rate = p.sample_rate.unwrap_or(crate::audio::RATE);
    loop {
        crate::audio::check_cancel(cancel)?;
        let packet = match demux.next_packet() {
            Ok(p) => p,
            Err(oxideav_core::Error::Eof) => break,
            Err(e) => return Err(e.into()),
        };
        if packet.stream_index != stream.index || packet.flags.header {
            continue;
        }
        let mut data = packet.data;
        if params.codec == codecs::CODEC_TYPE_AAC
            && data.len() >= 7
            && data[0] == 0xff
            && data[1] & 0xf6 == 0xf0
        {
            let length = ((data[3] as usize & 3) << 11)
                | ((data[4] as usize) << 3)
                | ((data[5] as usize) >> 5);
            let header = if data[1] & 1 == 0 { 9 } else { 7 };
            ensure!(
                data.len() == length && length >= header,
                "Ungültiger ADTS-Audiorahmen"
            );
            data.drain(..header);
        }
        let packet = Packet::new_from_boxed_slice(0, 0, 0, data.into_boxed_slice());
        let decoded = decoder.decode(&packet)?;
        let spec = *decoded.spec();
        ensure!(
            mono.is_empty() || rate == spec.rate,
            "Wechselnde Abtastrate"
        );
        rate = spec.rate;
        let channels = spec.channels.count();
        ensure!(channels > 0, "Keine Audiokanäle");
        let mut samples = SampleBuffer::<f32>::new(decoded.capacity() as u64, spec);
        samples.copy_interleaved_ref(decoded);
        mono.extend(
            samples
                .samples()
                .chunks(channels)
                .map(|s| s.iter().sum::<f32>() / channels as f32),
        );
        ensure!(
            mono.len() < rate as usize * 3600 * 8,
            "Datei länger als acht Stunden"
        );
    }
    ensure!(!mono.is_empty(), "Leere Audiospur");
    crate::audio::resample(&mono, rate, cancel)
}

/// Copy encoded video and AAC packets into one MP4 without transcoding or a helper process.
pub fn mux_mp4(video: &Path, audio: &Path, output: &Path, cancel: &AtomicBool) -> Result<()> {
    let mut v = oxideav_mp4::demux::open(Box::new(File::open(video)?), &Resolver)?;
    let mut a = oxideav_mp4::demux::open(Box::new(File::open(audio)?), &Resolver)?;
    let mut vs = v
        .streams()
        .iter()
        .find(|s| s.params.media_type == MediaType::Video)
        .context("Videospur fehlt")?
        .clone();
    let mut aus = a
        .streams()
        .iter()
        .find(|s| s.params.media_type == MediaType::Audio)
        .context("Tonspur fehlt")?
        .clone();
    let (vi, ai) = (vs.index, aus.index);
    vs.index = 0;
    aus.index = 1;
    let streams = [vs, aus];
    let temporary = tempfile::Builder::new()
        .prefix(".video-")
        .suffix(".mp4")
        .tempfile_in(output.parent().context("Ausgabeordner fehlt")?)?;
    let mut mux = oxideav_mp4::muxer::open(Box::new(temporary.reopen()?), &streams)?;
    mux.write_header()?;
    fn next(
        demux: &mut dyn Demuxer,
        source: u32,
        target: u32,
    ) -> Result<Option<oxideav_core::Packet>> {
        loop {
            match demux.next_packet() {
                Ok(mut p) if p.stream_index == source => {
                    p.stream_index = target;
                    return Ok(Some(p));
                }
                Ok(_) => {}
                Err(oxideav_core::Error::Eof) => return Ok(None),
                Err(e) => return Err(e.into()),
            }
        }
    }
    let mut vp = next(&mut *v, vi, 0)?;
    let mut ap = next(&mut *a, ai, 1)?;
    while vp.is_some() || ap.is_some() {
        crate::audio::check_cancel(cancel)?;
        let time = |p: &oxideav_core::Packet| {
            let base = streams[p.stream_index as usize].time_base;
            (p.dts.or(p.pts).unwrap_or(0) as f64) * base.num() as f64 / base.den() as f64
        };
        let take_video = match (&vp, &ap) {
            (Some(v), Some(a)) => time(v) <= time(a),
            (Some(_), None) => true,
            _ => false,
        };
        if take_video {
            mux.write_packet(&vp.take().unwrap())?;
            vp = next(&mut *v, vi, 0)?;
        } else {
            mux.write_packet(&ap.take().unwrap())?;
            ap = next(&mut *a, ai, 1)?;
        }
    }
    mux.write_trailer()?;
    drop(mux);
    temporary.as_file().sync_all()?;
    crate::audio::check_cancel(cancel)?;
    let reservation = std::fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(output)?;
    drop(reservation);
    if let Err(error) = std::fs::rename(temporary.path(), output) {
        let _ = std::fs::remove_file(output);
        return Err(error.into());
    }
    Ok(())
}
