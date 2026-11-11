//! Anonymous YouTube player requests with a shared visitor session, entirely in Rust.
use anyhow::{Context, Result, ensure};
use serde::Deserialize;
use serde_json::{Value, json};
use std::{sync::atomic::AtomicBool, time::Duration};
const AGENT: &str = "Mozilla/5.0 (Macintosh; Intel Mac OS X 15_7_3) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/26.0 Safari/605.1.15";
#[derive(Clone, Debug)]
pub struct Stream {
    pub url: String,
    pub container: String,
    pub has_audio: bool,
    pub has_video: bool,
    pub bitrate: u64,
    pub mime: String,
}
pub struct Video {
    pub title: String,
    pub formats: Vec<Stream>,
}
pub fn video_id(url: &str) -> Result<String> {
    let parsed = reqwest::Url::parse(url)?;
    let host = parsed.host_str().unwrap_or_default();
    ensure!(
        host == "youtu.be"
            || host == "youtube.com"
            || host.ends_with(".youtube.com")
            || host == "youtube-nocookie.com"
            || host.ends_with(".youtube-nocookie.com"),
        "Bitte eine YouTube-Adresse eingeben"
    );
    let id = if host == "youtu.be" {
        parsed
            .path()
            .trim_start_matches('/')
            .split('/')
            .next()
            .map(str::to_owned)
    } else {
        parsed
            .query_pairs()
            .find(|(k, _)| k == "v")
            .map(|(_, v)| v.into_owned())
            .or_else(|| {
                let mut p = parsed.path_segments()?;
                let kind = p.next()?;
                ["shorts", "embed", "live", "v"]
                    .contains(&kind)
                    .then(|| p.next().map(str::to_owned))
                    .flatten()
            })
    }
    .context("Video-ID fehlt in der YouTube-Adresse")?;
    ensure!(
        id.len() == 11
            && id
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-'),
        "Ungültige YouTube-Video-ID"
    );
    Ok(id)
}
pub fn configuration(page: &str) -> Value {
    let mut result = json!({});
    for tail in page.split("ytcfg.set(").skip(1) {
        let mut decoder = serde_json::Deserializer::from_str(tail);
        if let Ok(Value::Object(values)) = Value::deserialize(&mut decoder)
            && let Some(object) = result.as_object_mut()
        {
            object.extend(values);
        }
    }
    result
}
pub fn parse_player(value: &Value) -> Result<Video> {
    let status = value["playabilityStatus"]["status"]
        .as_str()
        .unwrap_or_default();
    ensure!(
        status == "OK",
        "YouTube: {}",
        value["playabilityStatus"]["reason"]
            .as_str()
            .unwrap_or("Video nicht verfügbar")
    );
    ensure!(
        !value["videoDetails"]["isLiveContent"]
            .as_bool()
            .unwrap_or(false)
            || value["videoDetails"]["isLive"].as_bool() != Some(true),
        "Livestream bitte nach Ende herunterladen"
    );
    let mut formats = vec![];
    for group in ["formats", "adaptiveFormats"] {
        for item in value["streamingData"][group]
            .as_array()
            .into_iter()
            .flatten()
        {
            let Some(url) = item["url"]
                .as_str()
                .filter(|u| reqwest::Url::parse(u).is_ok_and(|u| u.scheme() == "https"))
            else {
                continue;
            };
            let mime = item["mimeType"].as_str().unwrap_or_default();
            let container = if mime.contains("/mp4") {
                "mp4"
            } else if mime.contains("/webm") {
                "webm"
            } else {
                continue;
            };
            formats.push(Stream {
                url: url.into(),
                container: container.into(),
                has_video: mime.starts_with("video/"),
                has_audio: mime.starts_with("audio/") || item.get("audioQuality").is_some(),
                bitrate: item["bitrate"].as_u64().unwrap_or(0),
                mime: mime.into(),
            });
        }
    }
    ensure!(
        !formats.is_empty(),
        "YouTube liefert keine direkt nutzbaren Streams"
    );
    Ok(Video {
        title: value["videoDetails"]["title"]
            .as_str()
            .unwrap_or("YouTube-Audio")
            .into(),
        formats,
    })
}
pub fn player(url: &str, cancel: &AtomicBool) -> Result<Video> {
    let id = video_id(url)?;
    let runtime = tokio::runtime::Runtime::new()?;
    runtime.block_on(async {
  let request=async {
   let client=reqwest::Client::builder().user_agent(AGENT).cookie_store(true).connect_timeout(Duration::from_secs(20)).timeout(Duration::from_secs(45)).build()?;
   let page=client.get(format!("https://www.youtube.com/watch?v={id}&hl=en")).send().await?.error_for_status()?.text().await?;
   let config=configuration(&page);
   let visitor=config["VISITOR_DATA"].as_str().or_else(||config["INNERTUBE_CONTEXT"]["client"]["visitorData"].as_str()).unwrap_or_default();
   let body=json!({"context":{"client":{"clientName":"VISIONOS","clientVersion":"1.02","userAgent":AGENT,"deviceMake":"Apple","deviceModel":"RealityDevice17,1","osName":"visionOS","osVersion":"26.5.23O471","hl":"en","gl":"US","visitorData":visitor}},"videoId":id,"contentCheckOk":true,"racyCheckOk":true,"playbackContext":{"contentPlaybackContext":{"html5Preference":"HTML5_PREF_WANTS"}}});
   let mut request=client.post("https://www.youtube.com/youtubei/v1/player?prettyPrint=false").header("Origin","https://www.youtube.com").header("X-YouTube-Client-Name","101").header("X-YouTube-Client-Version","1.02");
   if !visitor.is_empty(){request=request.header("X-Goog-Visitor-Id",visitor);}
   let response:Value=request.json(&body).send().await?.error_for_status()?.json().await?;parse_player(&response)
  };
  tokio::select!{result=request=>result,()=crate::models::wait_cancel(cancel)=>anyhow::bail!("Abgebrochen"),()=tokio::time::sleep(Duration::from_secs(90))=>anyhow::bail!("Zeitüberschreitung bei YouTube")}
 })
}
