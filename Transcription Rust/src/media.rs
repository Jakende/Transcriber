use crate::{document::safe_name, models, storage::data_dir};
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use std::{
    io::Read,
    path::{Path, PathBuf},
    sync::atomic::AtomicBool,
};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Episode {
    pub title: String,
    pub audio_url: String,
    pub show: String,
    pub feed_url: String,
    pub published: String,
    pub description: String,
}
#[derive(Clone, Debug, Deserialize)]
pub struct Podcast {
    pub title: String,
    pub url: String,
    #[serde(default)]
    pub author: String,
}
pub fn feed(url: &str) -> Result<Vec<Episode>> {
    let response = models::client()?.get(url).send()?.error_for_status()?;
    ensure!(
        response.content_length().is_none_or(|n| n < 10_000_000),
        "RSS-Feed ist zu groß."
    );
    let mut bytes = vec![];
    response.take(10_000_001).read_to_end(&mut bytes)?;
    ensure!(bytes.len() <= 10_000_000, "RSS-Feed ist zu groß.");
    let parsed = feed_rs::parser::parse(bytes.as_slice())?;
    let show = parsed.title.map(|v| v.content).unwrap_or_default();
    Ok(parsed
        .entries
        .into_iter()
        .filter_map(|entry| {
            let link = entry
                .links
                .iter()
                .find(|l| l.rel.as_deref() == Some("enclosure"))
                .map(|l| l.href.clone())
                .or_else(|| {
                    entry
                        .media
                        .iter()
                        .flat_map(|m| m.content.iter())
                        .find_map(|m| m.url.as_ref().map(ToString::to_string))
                })?;
            Some(Episode {
                title: entry
                    .title
                    .map(|v| v.content)
                    .unwrap_or_else(|| "Ohne Titel".into()),
                audio_url: link,
                show: show.clone(),
                feed_url: url.into(),
                published: entry.published.map(|v| v.to_rfc3339()).unwrap_or_default(),
                description: entry.summary.map(|v| v.content).unwrap_or_default(),
            })
        })
        .collect())
}
pub fn search_podcasts(query: &str) -> Result<Vec<Podcast>> {
    use sha1::{Digest, Sha1};
    let key=std::env::var("PODCASTINDEX_API_KEY").context("PODCASTINDEX_API_KEY und PODCASTINDEX_API_SECRET als Umgebungsvariablen hinterlegen, oder direkt eine RSS-Adresse öffnen.")?;
    let secret = std::env::var("PODCASTINDEX_API_SECRET")?;
    let timestamp = chrono::Utc::now().timestamp().to_string();
    let signature = format!(
        "{:x}",
        Sha1::digest(format!("{key}{secret}{timestamp}").as_bytes())
    );
    #[derive(Deserialize)]
    struct Response {
        feeds: Vec<Podcast>,
    }
    let response: Response = models::client()?
        .get("https://api.podcastindex.org/api/1.0/search/byterm")
        .query(&[("q", query), ("max", "30")])
        .header("X-Auth-Key", key)
        .header("X-Auth-Date", timestamp)
        .header("Authorization", signature)
        .send()?
        .error_for_status()?
        .json()?;
    Ok(response.feeds)
}
fn destination(dir: &Path, title: &str, extension: &str) -> Result<PathBuf> {
    std::fs::create_dir_all(dir)?;
    for i in 0..100_000 {
        let name = if i == 0 {
            safe_name(title)
        } else {
            format!("{}_{i}", safe_name(title))
        };
        let path = dir.join(format!("{name}.{extension}"));
        if !path.exists() {
            return Ok(path);
        }
    }
    anyhow::bail!("Kein freier Dateiname gefunden.")
}
pub fn download_episode(
    episode: &Episode,
    dir: &Path,
    cancel: &AtomicBool,
    progress: &mut dyn FnMut(f32),
) -> Result<PathBuf> {
    let url = reqwest::Url::parse(&episode.audio_url)?;
    let extension = Path::new(url.path())
        .extension()
        .and_then(|e| e.to_str())
        .filter(|e| ["mp3", "m4a", "mp4", "ogg", "opus", "wav", "flac"].contains(e))
        .unwrap_or("mp3");
    let path = destination(dir, &episode.title, extension)?;
    models::download(
        &models::client()?,
        &episode.audio_url,
        &path,
        cancel,
        progress,
    )?;
    crate::storage::save(
        &path.with_extension(format!("{extension}.podcast.json")),
        episode,
    )?;
    Ok(path)
}
/// YouTube extraction and signature evaluation run in Rust (Boa); no yt-dlp/Deno.
pub fn youtube(
    url: &str,
    dir: &Path,
    video: bool,
    cancel: &AtomicBool,
    progress: &mut dyn FnMut(f32),
) -> Result<PathBuf> {
    use rusty_ytdl::{Video, VideoOptions, VideoQuality, VideoSearchOptions};
    let parsed = reqwest::Url::parse(url)?;
    let host = parsed.host_str().unwrap_or("");
    ensure!(
        host == "youtu.be" || host == "youtube.com" || host.ends_with(".youtube.com"),
        "Bitte eine YouTube-Adresse eingeben."
    );
    let options = VideoOptions {
        quality: VideoQuality::HighestAudio,
        filter: VideoSearchOptions::Audio,
        ..Default::default()
    };
    let source = Video::new_with_options(url, options)?;
    let runtime = tokio::runtime::Runtime::new()?;
    let info=runtime.block_on(source.get_info()).context("YouTube konnte nicht gelesen werden. YouTube kann Anfragen sperren; in diesem Fall eine lokale Mediendatei importieren.")?;
    crate::audio::check_cancel(cancel)?;
    let stream = info
        .formats
        .iter()
        .filter(|f| f.has_audio && !f.has_video && !f.is_live && f.mime_type.container == "mp4")
        .max_by_key(|f| f.audio_bitrate.unwrap_or(0))
        .context("Kein direkter Audiostream verfügbar.")?;
    let ext = if stream.mime_type.container.contains("webm") {
        "webm"
    } else {
        "m4a"
    };
    let path = destination(dir, &info.video_details.title, ext)?;
    models::download(&models::client()?, &stream.url, &path, cancel, progress)?;
    if video {
        let stream = info
            .formats
            .iter()
            .filter(|f| f.has_audio && f.has_video && !f.is_live)
            .max_by_key(|f| f.bitrate)
            .context("Audio geladen, aber kein Video mit integrierter Tonspur verfügbar.")?;
        let movie = destination(dir, &info.video_details.title, "mp4")?;
        models::download(&models::client()?, &stream.url, &movie, cancel, progress)?;
    }
    Ok(path)
}
pub fn downloads_dir() -> PathBuf {
    data_dir().join("Downloads")
}
