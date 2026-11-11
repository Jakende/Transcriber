//! Manual network check: cargo run --release --example media_probe -- youtube URL OUTPUT
use std::{path::Path, sync::atomic::AtomicBool};
use transcription_rust::media;
fn main() -> anyhow::Result<()> {
    let args = std::env::args().collect::<Vec<_>>();
    match args.get(1).map(String::as_str) {
        Some("youtube") => {
            let url = args.get(2).ok_or_else(|| anyhow::anyhow!("URL fehlt"))?;
            let dir = args
                .get(3)
                .ok_or_else(|| anyhow::anyhow!("Ausgabeordner fehlt"))?;
            println!(
                "{}",
                media::youtube(
                    url,
                    Path::new(dir),
                    true,
                    &AtomicBool::new(false),
                    &mut |p| eprintln!("{:.0}%", p * 100.)
                )?
                .display()
            );
        }
        Some("podcasts") => {
            for podcast in
                media::search_podcasts(args.get(2).map(String::as_str).unwrap_or("NachWuchs"))?
            {
                println!("{}", podcast.title);
            }
        }
        Some("rss") => {
            let url = args.get(2).ok_or_else(|| anyhow::anyhow!("URL fehlt"))?;
            println!("{}", serde_json::to_string_pretty(&media::feed(url)?)?);
        }
        _ => anyhow::bail!("media_probe youtube URL OUTPUT | rss URL"),
    }
    Ok(())
}
