use std::{fs::File, path::Path, sync::atomic::AtomicBool};
fn main() -> anyhow::Result<()> {
    let args = std::env::args().collect::<Vec<_>>();
    let p = Path::new(&args[1]);
    if p.extension().is_some_and(|e| e == "ts") {
        let d = oxideav_mpegts::demuxer::open(
            Box::new(File::open(p)?),
            &oxideav_core::NullCodecResolver,
        )?;
        for s in d.streams() {
            eprintln!(
                "codec={} rate={:?} channels={:?} extra={:?}",
                s.params.codec_id, s.params.sample_rate, s.params.channels, s.params.extradata
            );
        }
    }
    let pcm = transcription_rust::audio::decode(p, &AtomicBool::new(false))?;
    println!(
        "samples={} finite={} rms={}",
        pcm.len(),
        pcm.iter().filter(|s| s.is_finite()).count(),
        (pcm.iter().map(|s| s * s).sum::<f32>() / pcm.len() as f32).sqrt()
    );
    Ok(())
}
