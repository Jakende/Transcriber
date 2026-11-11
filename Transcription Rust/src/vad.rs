//! Speech boundaries from the bundled Silero neural model; no runtime download.
use crate::audio::{RATE, check_cancel};
use anyhow::Result;
use std::sync::atomic::AtomicBool;
pub fn regions_from_probs(probabilities: &[f32], samples: usize) -> Vec<(usize, usize)> {
    let mut regions = vec![];
    let mut start = None;
    let mut silence = None;
    for (i, &p) in probabilities.iter().enumerate() {
        let at = i * 512;
        if p >= 0.5 {
            start.get_or_insert(at);
            silence = None;
        } else if p < 0.35
            && let Some(begin) = start
        {
            let quiet = *silence.get_or_insert(at);
            if at - quiet >= RATE as usize / 6 {
                if quiet - begin >= RATE as usize / 4 {
                    regions.push((begin.saturating_sub(480), (quiet + 480).min(samples)));
                }
                start = None;
                silence = None;
            }
        }
    }
    if let Some(begin) = start
        && samples.saturating_sub(begin) >= RATE as usize / 4
    {
        regions.push((begin.saturating_sub(480), samples));
    }
    regions
}
pub fn speech_regions(pcm: &[f32], cancel: &AtomicBool) -> Result<Vec<(usize, usize)>> {
    let mut vad = silero_vad_pure::SileroVad::new(silero_vad_pure::SampleRate::Hz16000)?;
    let mut probabilities = Vec::with_capacity(pcm.len().div_ceil(512));
    for samples in pcm.chunks(512) {
        check_cancel(cancel)?;
        let mut chunk = [0.; 512];
        chunk[..samples.len()].copy_from_slice(samples);
        probabilities.push(vad.process(&chunk)?);
    }
    Ok(regions_from_probs(&probabilities, pcm.len()))
}
