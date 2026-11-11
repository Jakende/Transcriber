//! ASF audio payload extraction, based on Microsoft's ASF 1.20 file layout.
//! Media objects are reassembled before passing WMA packets to the Rust decoder.
use anyhow::{Context, Result, ensure};
use oxideav_core::{CodecId, CodecParameters, Decoder, Frame, Packet, TimeBase};
use std::{
    fs::File,
    io::{Read, Seek, SeekFrom},
    path::Path,
    sync::atomic::AtomicBool,
};
const HEADER: [u8; 16] = [
    0x30, 0x26, 0xb2, 0x75, 0x8e, 0x66, 0xcf, 0x11, 0xa6, 0xd9, 0, 0xaa, 0, 0x62, 0xce, 0x6c,
];
const DATA: [u8; 16] = [
    0x36, 0x26, 0xb2, 0x75, 0x8e, 0x66, 0xcf, 0x11, 0xa6, 0xd9, 0, 0xaa, 0, 0x62, 0xce, 0x6c,
];
const FILE_PROPERTIES: [u8; 16] = [
    0xa1, 0xdc, 0xab, 0x8c, 0x47, 0xa9, 0xcf, 0x11, 0x8e, 0xe4, 0, 0xc0, 0x0c, 0x20, 0x53, 0x65,
];
const STREAM_PROPERTIES: [u8; 16] = [
    0x91, 0x07, 0xdc, 0xb7, 0xb7, 0xa9, 0xcf, 0x11, 0x8e, 0xe6, 0, 0xc0, 0x0c, 0x20, 0x53, 0x65,
];
const AUDIO: [u8; 16] = [
    0x40, 0x9e, 0x69, 0xf8, 0x4d, 0x5b, 0xcf, 0x11, 0xa8, 0xfd, 0, 0x80, 0x5f, 0x5c, 0x44, 0x2b,
];
struct Bytes<'a> {
    data: &'a [u8],
    at: usize,
}
impl<'a> Bytes<'a> {
    fn take(&mut self, n: usize) -> Result<&'a [u8]> {
        let end = self.at.checked_add(n).context("ASF-Längenüberlauf")?;
        let out = self
            .data
            .get(self.at..end)
            .context("Abgeschnittene ASF-Daten")?;
        self.at = end;
        Ok(out)
    }
    fn value(&mut self, n: usize) -> Result<u64> {
        let mut out = 0;
        for (i, b) in self.take(n)?.iter().enumerate() {
            out |= u64::from(*b) << (i * 8);
        }
        Ok(out)
    }
    fn sized(&mut self, kind: u8) -> Result<usize> {
        Ok(self.value(match kind & 3 {
            0 => 0,
            1 => 1,
            2 => 2,
            _ => 4,
        })? as usize)
    }
}
fn number(data: &[u8], at: usize, n: usize) -> Result<u64> {
    Bytes { data, at }.value(n)
}
fn drain(decoder: &mut dyn Decoder, channels: usize, pcm: &mut Vec<f32>) -> Result<()> {
    loop {
        match decoder.receive_frame() {
            Ok(Frame::Audio(frame)) => {
                let data = frame.data.first().context("WMA-Frame ohne Samples")?;
                ensure!(data.len() % (channels * 4) == 0, "Ungültiger WMA-Frame");
                for frame in data.chunks_exact(channels * 4) {
                    let mut sum = 0.;
                    for sample in frame.chunks_exact(4) {
                        let value = f32::from_le_bytes(sample.try_into()?);
                        ensure!(value.is_finite(), "Ungültiger WMA-Pegel");
                        sum += value;
                    }
                    pcm.push(sum / channels as f32);
                }
            }
            Ok(_) => anyhow::bail!("WMA-Decoder lieferte Videodaten"),
            Err(oxideav_core::Error::NeedMore | oxideav_core::Error::Eof) => break,
            Err(error) => return Err(error.into()),
        }
    }
    Ok(())
}
pub fn decode(path: &Path, cancel: &AtomicBool) -> Result<Vec<f32>> {
    let mut file = File::open(path)?;
    let mut head = [0u8; 30];
    file.read_exact(&mut head)?;
    ensure!(head[..16] == HEADER, "Keine ASF-Datei");
    let header_len = number(&head, 16, 8)?;
    ensure!(
        (30..=16 * 1024 * 1024).contains(&header_len),
        "Ungültige ASF-Kopfgröße"
    );
    let mut body = vec![0; header_len as usize - 30];
    file.read_exact(&mut body)?;
    let mut cursor = Bytes { data: &body, at: 0 };
    let mut packet_size = 0;
    let mut stream = None;
    let mut declared_duration = None;
    while cursor.at < body.len() {
        let guid = cursor.take(16)?;
        let len = cursor.value(8)?;
        ensure!(
            (24..=16 * 1024 * 1024).contains(&len),
            "Ungültige ASF-Objektgröße"
        );
        let data = cursor.take(len as usize - 24)?;
        if guid == FILE_PROPERTIES {
            let min = number(data, 68, 4)?;
            let max = number(data, 72, 4)?;
            ensure!(
                min == max && max > 0 && max <= 1024 * 1024,
                "Variable oder ungültige ASF-Paketgröße"
            );
            packet_size = max as usize;
            let play = number(data, 40, 8)?;
            let preroll = number(data, 56, 8)?;
            declared_duration = Some((play as f64 / 10_000_000. - preroll as f64 / 1000.).max(0.));
        }
        if guid == STREAM_PROPERTIES && data.get(..16) == Some(AUDIO.as_slice()) && stream.is_none()
        {
            let flags = number(data, 48, 2)?;
            ensure!(flags & 0x8000 == 0, "Verschlüsselte ASF-Audiospur");
            let count = number(data, 40, 4)? as usize;
            let wave = data
                .get(54..54 + count)
                .context("ASF-Audiokonfiguration fehlt")?;
            let tag = number(wave, 0, 2)?;
            ensure!(
                [0x160, 0x161].contains(&tag),
                "ASF-Audiocodec {tag:#x} noch nicht unterstützt"
            );
            let mut p =
                CodecParameters::audio(CodecId::new(if tag == 0x160 { "wma1" } else { "wma2" }));
            let channels = number(wave, 2, 2)? as u16;
            ensure!(
                (1..=2).contains(&channels),
                "WMA erwartet ein oder zwei Kanäle"
            );
            p.channels = Some(channels);
            p.sample_rate = Some(number(wave, 4, 4)? as u32);
            p.bit_rate = Some(number(wave, 8, 4)? * 8);
            let align = number(wave, 12, 2)? as usize;
            ensure!(align > 0, "Ungültige WMA-Blockgröße");
            p.extradata = wave.get(18..).unwrap_or_default().to_vec();
            stream = Some(((flags & 0x7f) as usize, p, align));
        }
    }
    let (stream_id, params, align) = stream.context("Keine WMA-Audiospur gefunden")?;
    ensure!(packet_size > 0, "ASF-Paketgröße fehlt");
    loop {
        crate::audio::check_cancel(cancel)?;
        let mut obj = [0u8; 24];
        file.read_exact(&mut obj)?;
        let len = number(&obj, 16, 8)?;
        ensure!(len >= 24, "Ungültige ASF-Objektlänge");
        if obj[..16] == DATA {
            break;
        }
        file.seek(SeekFrom::Current(i64::try_from(len - 24)?))?;
    }
    let mut data_header = [0u8; 26];
    file.read_exact(&mut data_header)?;
    let packets = number(&data_header, 16, 8)?;
    ensure!(packets < 100_000_000, "Zu viele ASF-Pakete");
    let mut decoder = oxideav_wma::WmaDecoder::from_params(&params)?;
    let channels = params.channels.unwrap() as usize;
    let rate = params.sample_rate.unwrap();
    let mut pcm = vec![];
    let mut assembled = vec![];
    let mut expected = 0;
    let mut send = |bytes: Vec<u8>, pcm: &mut Vec<f32>| -> Result<()> {
        ensure!(
            bytes.len().is_multiple_of(align),
            "Unvollständiges WMA-Medienobjekt"
        );
        for chunk in bytes.chunks(align) {
            decoder.send_packet(&Packet::new(
                0,
                TimeBase::new(1, rate.into()),
                chunk.to_vec(),
            ))?;
            drain(&mut decoder, channels, pcm)?;
        }
        Ok(())
    };
    for _ in 0..packets {
        crate::audio::check_cancel(cancel)?;
        let mut packet = vec![0; packet_size];
        file.read_exact(&mut packet)?;
        let mut p = Bytes {
            data: &packet,
            at: 0,
        };
        let first = p.value(1)? as u8;
        let flags = if first & 0x80 != 0 {
            ensure!(first & 0x60 == 0, "Unbekannte ASF-Fehlerkorrektur");
            p.take((first & 15) as usize)?;
            p.value(1)? as u8
        } else {
            first
        };
        let properties = p.value(1)? as u8;
        let actual = p.sized(flags >> 5)?;
        let _sequence = p.sized(flags >> 1)?;
        let padding = p.sized(flags >> 3)?;
        p.take(6)?;
        ensure!(
            actual == 0 || actual == packet_size,
            "Abweichende ASF-Paketgröße"
        );
        ensure!(padding <= packet_size, "Ungültige ASF-Füllung");
        let end = packet_size - padding;
        let multi = flags & 1 != 0;
        let payload_flags = if multi { p.value(1)? as u8 } else { 1 };
        let count = if multi { payload_flags & 63 } else { 1 };
        for _ in 0..count {
            let stream = p.sized(properties >> 6)? & 0x7f;
            let _object = p.sized(properties >> 4)?;
            let offset = p.sized(properties >> 2)?;
            let replicated = p.sized(properties)?;
            let info = p.take(replicated)?;
            let length = if multi {
                p.sized(payload_flags >> 6)?
            } else {
                end.checked_sub(p.at).context("Ungültige ASF-Nutzlast")?
            };
            ensure!(p.at + length <= end, "ASF-Nutzlast über Paketgrenze");
            let payload = p.take(length)?;
            if stream != stream_id {
                continue;
            }
            if replicated == 1 {
                let mut sub = Bytes {
                    data: payload,
                    at: 0,
                };
                while sub.at < payload.len() {
                    let size = sub.value(1)? as usize;
                    send(sub.take(size)?.to_vec(), &mut pcm)?;
                }
                continue;
            }
            if offset == 0 {
                ensure!(assembled.is_empty(), "Unvollständiges ASF-Medienobjekt");
                expected = if replicated >= 8 {
                    number(info, 0, 4)? as usize
                } else {
                    length
                };
            }
            ensure!(offset == assembled.len(), "Lücke in ASF-Medienobjekt");
            ensure!(expected <= 16 * 1024 * 1024, "ASF-Medienobjekt zu groß");
            assembled.extend_from_slice(payload);
            ensure!(assembled.len() <= expected, "ASF-Medienobjekt zu lang");
            if assembled.len() == expected {
                send(std::mem::take(&mut assembled), &mut pcm)?;
            }
        }
        ensure!(
            pcm.len() < rate as usize * 3600 * 8,
            "Datei länger als acht Stunden"
        );
    }
    ensure!(assembled.is_empty(), "Abgeschnittenes ASF-Medienobjekt");
    decoder.flush()?;
    drain(&mut decoder, channels, &mut pcm)?;
    ensure!(!pcm.is_empty(), "Leere WMA-Audiospur");
    if let Some(seconds) = declared_duration {
        let length = (seconds * rate as f64).round() as usize;
        if length > 0 && length < pcm.len() {
            pcm.truncate(length);
        }
    }
    crate::audio::resample(&pcm, rate, cancel)
}
