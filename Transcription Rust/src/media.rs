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
    #[serde(default, flatten)]
    pub metadata: std::collections::BTreeMap<String, serde_json::Value>,
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
    parse_feed(&bytes, url)
}
pub fn parse_feed(bytes: &[u8], url: &str) -> Result<Vec<Episode>> {
    let parsed = feed_rs::parser::parse(bytes)?;
    let show = parsed
        .title
        .as_ref()
        .map(|v| v.content.clone())
        .unwrap_or_default();
    let xml_text = std::str::from_utf8(bytes)?;
    let xml = roxmltree::Document::parse(xml_text)?;
    let channel = xml.descendants().find(|n| n.has_tag_name("channel"));
    let child = |node: roxmltree::Node<'_, '_>, name: &str| {
        node.children()
            .find(|n| n.is_element() && n.tag_name().name() == name)
            .and_then(|n| n.text())
            .map(str::to_owned)
    };
    let show_description = parsed.description.as_ref().map(|v| v.content.clone());
    let show_author = channel
        .and_then(|n| child(n, "author"))
        .or_else(|| parsed.authors.first().map(|a| a.name.clone()));
    let show_image = channel
        .and_then(|n| {
            n.children()
                .find(|n| n.has_tag_name(("http://www.itunes.com/dtds/podcast-1.0.dtd", "image")))
                .and_then(|n| n.attribute("href"))
                .map(str::to_owned)
        })
        .or_else(|| parsed.logo.as_ref().map(|i| i.uri.clone()));
    let show_categories = parsed
        .categories
        .iter()
        .map(|c| c.term.clone())
        .collect::<Vec<_>>();
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
            let item = xml.descendants().find(|n| {
                n.has_tag_name("item")
                    && (child(*n, "guid").as_deref() == Some(&entry.id)
                        || n.children().any(|e| {
                            e.has_tag_name("enclosure") && e.attribute("url") == Some(link.as_str())
                        }))
            });
            let title = entry
                .title
                .as_ref()
                .map(|v| v.content.clone())
                .unwrap_or_else(|| "Ohne Titel".into());
            let description = entry
                .content
                .as_ref()
                .and_then(|c| c.body.clone())
                .or_else(|| entry.summary.as_ref().map(|v| v.content.clone()))
                .unwrap_or_default();
            let mut metadata = std::collections::BTreeMap::new();
            let mut put = |key: &str, value: serde_json::Value| {
                if !value.is_null() {
                    metadata.insert(key.into(), value);
                }
            };
            put("show_title", serde_json::json!(show));
            put("episode_title", serde_json::json!(title));
            put("guid", serde_json::json!(entry.id));
            put(
                "author",
                serde_json::json!(
                    entry
                        .authors
                        .first()
                        .map(|a| a.name.clone())
                        .or_else(|| show_author.clone())
                ),
            );
            put("publisher", serde_json::json!(show_author));
            put(
                "language",
                serde_json::json!(entry.language.as_ref().or(parsed.language.as_ref())),
            );
            put(
                "published_at",
                serde_json::json!(entry.published.map(|v| v.to_rfc3339())),
            );
            put(
                "episode_url",
                serde_json::json!(
                    entry
                        .links
                        .iter()
                        .find(|l| l.rel.as_deref() != Some("enclosure"))
                        .map(|l| l.href.clone())
                ),
            );
            put("raw_show_description", serde_json::json!(show_description));
            put("show_description", serde_json::json!(show_description));
            put("raw_episode_description", serde_json::json!(description));
            put("episode_description", serde_json::json!(description));
            put(
                "categories",
                serde_json::json!(if entry.categories.is_empty() {
                    show_categories.clone()
                } else {
                    entry
                        .categories
                        .iter()
                        .map(|c| c.term.clone())
                        .collect::<Vec<_>>()
                }),
            );
            let image = item
                .and_then(|n| {
                    n.children()
                        .find(|n| n.is_element() && n.tag_name().name() == "image")
                        .and_then(|n| n.attribute("href"))
                        .map(str::to_owned)
                })
                .or_else(|| show_image.clone());
            put("image_url", serde_json::json!(image));
            if let Some(item) = item {
                for (tag, key) in [("episode", "episode_number"), ("season", "season_number")] {
                    put(
                        key,
                        serde_json::json!(child(item, tag).and_then(|v| v.parse::<u32>().ok())),
                    );
                }
                put(
                    "episode_type",
                    serde_json::json!(child(item, "episodeType")),
                );
                put(
                    "explicit",
                    serde_json::json!(child(item, "explicit").map(|v| matches!(
                        v.trim().to_lowercase().as_str(),
                        "yes" | "true" | "explicit"
                    ))),
                );
                let duration = child(item, "duration").and_then(|v| {
                    v.split(':').try_fold(0u64, |a, p| {
                        p.trim().parse::<f64>().ok().map(|p| a * 60 + p as u64)
                    })
                });
                put("duration_seconds", serde_json::json!(duration));
            }
            Some(Episode {
                metadata,
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
    let credentials = crate::credentials::load()?;
    let key = credentials.key;
    let secret = credentials.secret;
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
    let mut saved = episode.clone();
    saved.metadata.insert(
        "downloaded_at".into(),
        serde_json::json!(chrono::Utc::now().to_rfc3339()),
    );
    crate::storage::save(
        &path.with_extension(format!("{extension}.podcast.json")),
        &saved,
    )?;
    Ok(path)
}
/// YouTube visitor session and direct player streams run in Rust; no helper process.
pub fn youtube(
    url: &str,
    dir: &Path,
    video: bool,
    cancel: &AtomicBool,
    progress: &mut dyn FnMut(f32),
) -> Result<PathBuf> {
    let info = crate::youtube::player(url, cancel)?;
    crate::audio::check_cancel(cancel)?;
    let stream = info.formats.iter()
        .filter(|f| f.has_audio && ["mp4","webm"].contains(&f.container.as_str()))
        .filter(|f|reqwest::Url::parse(&f.url).is_ok_and(|u|u.scheme()=="https"))
        .max_by_key(|f| (!f.has_video,video&&f.container=="mp4",f.bitrate))
        .context("YouTube liefert keinen entschlüsselten Audiostream. Die aktuelle Rust-Extraktion unterstützt diesen Player noch nicht. Bitte die Datei lokal importieren.")?;
    std::fs::create_dir_all(dir)?;
    let temporary = dir.join(format!(".youtube-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir(&temporary)?;
    let result = (|| -> Result<PathBuf> {
        let extension = if stream.container == "webm" {
            "webm"
        } else {
            "m4a"
        };
        let audio = temporary.join(format!("source.{extension}"));
        models::download(&models::client()?, &stream.url, &audio, cancel, &mut |p| {
            progress(p * 0.45)
        })
        .context("YouTube-Audiostream laden")?;
        let samples = crate::audio::decode(&audio, cancel)?;
        progress(0.6);
        let path = destination(dir, &info.title, "mp3")?;
        crate::audio::write_mp3(&path, &samples, cancel)?;
        if video {
            let stream = info
                .formats
                .iter()
                .filter(|f| f.has_video && f.container == "mp4")
                .max_by_key(|f| (f.mime.contains("avc1"), f.bitrate))
                .context("MP3 gespeichert; keine MP4-Videospur verfügbar")?;
            let movie = destination(dir, &info.title, "mp4")?;
            if stream.has_audio {
                models::download(&models::client()?, &stream.url, &movie, cancel, &mut |p| {
                    progress(0.7 + p * 0.3)
                })?;
            } else {
                let picture = temporary.join("video.mp4");
                models::download(
                    &models::client()?,
                    &stream.url,
                    &picture,
                    cancel,
                    &mut |p| progress(0.7 + p * 0.25),
                )?;
                crate::containers::mux_mp4(&picture, &audio, &movie, cancel)?;
            }
        }
        progress(1.);
        Ok(path)
    })();
    let _ = std::fs::remove_dir_all(temporary);
    result
}
pub fn downloads_dir() -> PathBuf {
    data_dir().join("Downloads")
}
