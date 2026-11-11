fn main() -> anyhow::Result<()> {
    let file = std::env::args()
        .nth(1)
        .ok_or_else(|| anyhow::anyhow!("Datei fehlt"))?;
    let pcm = transcription_rust::audio::decode(
        std::path::Path::new(&file),
        &std::sync::atomic::AtomicBool::new(false),
    )?;
    let mut model = silero_vad_pure::SileroVad::new(silero_vad_pure::SampleRate::Hz16000)?;
    let mut values = vec![];
    for chunk in pcm.chunks(512) {
        let mut padded = [0.; 512];
        padded[..chunk.len()].copy_from_slice(chunk);
        values.push(model.process(&padded)?);
    }
    println!(
        "samples={} rms={} max={} speech_chunks={} regions={:?}",
        pcm.len(),
        (pcm.iter().map(|v| v * v).sum::<f32>() / pcm.len() as f32).sqrt(),
        values.iter().copied().fold(0f32, f32::max),
        values.iter().filter(|&&p| p > 0.5).count(),
        transcription_rust::vad::regions_from_probs(&values, pcm.len())
    );
    Ok(())
}
