//! ISO/IEC 11172-1 program-stream audio extraction into an automatically removed scratch file.
use anyhow::{Context, Result, ensure};
use std::{
    fs::File,
    io::{Read, Seek, SeekFrom, Write},
    path::Path,
    sync::atomic::AtomicBool,
};
pub fn decode(path: &Path, cancel: &AtomicBool) -> Result<Vec<f32>> {
    let mut input = File::open(path)?;
    let mut output = tempfile::Builder::new()
        .prefix("transcription-mpeg-")
        .suffix(".mp2")
        .tempfile()?;
    let mut chosen = None;
    let mut written = 0u64;
    loop {
        crate::audio::check_cancel(cancel)?;
        let mut prefix = [0u8; 4];
        match input.read_exact(&mut prefix) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => break,
            Err(e) => return Err(e.into()),
        };
        ensure!(prefix[..3] == [0, 0, 1], "Ungültiger MPEG-Programmstrom");
        let id = prefix[3];
        if id == 0xb9 {
            break;
        }
        if id == 0xba {
            let mut pack = [0u8; 8];
            input.read_exact(&mut pack)?;
            ensure!(pack[0] & 0xf0 == 0x20, "Kein MPEG-1-Packheader");
            continue;
        }
        let mut length = [0u8; 2];
        input.read_exact(&mut length)?;
        let length = u16::from_be_bytes(length) as usize;
        ensure!(length > 0, "MPEG-Paket ohne Länge");
        if !(0xc0..=0xdf).contains(&id) || chosen.is_some_and(|c| c != id) {
            input.seek(SeekFrom::Current(length as i64))?;
            continue;
        }
        chosen = Some(id);
        let mut packet = vec![0; length];
        input.read_exact(&mut packet)?;
        let mut start = 0;
        while packet.get(start) == Some(&0xff) {
            start += 1;
        }
        let byte = *packet.get(start).context("MPEG-Audioheader fehlt")?;
        if byte & 0xc0 == 0x40 {
            start += 2;
        }
        let byte = *packet.get(start).context("MPEG-Zeitstempel fehlt")?;
        start += match byte >> 4 {
            2 => 5,
            3 => 10,
            _ if byte == 0x0f => 1,
            _ => anyhow::bail!("Ungültiger MPEG-1-Audioheader"),
        };
        let samples = packet
            .get(start..)
            .context("Abgeschnittener MPEG-Audioheader")?;
        output.write_all(samples)?;
        written += samples.len() as u64;
        ensure!(written < 4 * 1024 * 1024 * 1024, "MPEG-Audiospur zu groß");
    }
    ensure!(written > 0, "Keine MPEG-Audiospur gefunden");
    output.flush()?;
    crate::audio::decode(output.path(), cancel)
}
