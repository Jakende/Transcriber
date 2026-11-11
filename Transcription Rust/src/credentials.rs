use anyhow::{Result, bail, ensure};
use serde::{Deserialize, Serialize};
#[derive(Serialize, Deserialize)]
pub struct PodcastCredentials {
    pub key: String,
    pub secret: String,
}
impl PodcastCredentials {
    pub fn checked(key: &str, secret: &str) -> Result<Self> {
        let key = key.trim();
        let secret = secret.trim();
        ensure!(
            !key.is_empty() && !secret.is_empty(),
            "PodcastIndex benötigt API-Schlüssel und API-Geheimnis."
        );
        Ok(Self {
            key: key.into(),
            secret: secret.into(),
        })
    }
}
const SERVICE: &str = "de.jakende.transcription-rust.podcastindex";
fn read(service: &str, account: &str) -> Result<Option<Vec<u8>>> {
    match security_framework::passwords::get_generic_password(service, account) {
        Ok(bytes) => Ok(Some(bytes)),
        Err(error) if error.code() == -25300 => Ok(None),
        Err(error) => Err(anyhow::anyhow!(
            "Schlüsselbundzugriff für PodcastIndex fehlgeschlagen: {error}"
        )),
    }
}
pub fn load() -> Result<PodcastCredentials> {
    let env = |names: &[&str]| {
        names
            .iter()
            .find_map(|name| std::env::var(name).ok().filter(|v| !v.trim().is_empty()))
    };
    if let (Some(key), Some(secret)) = (
        env(&["PODCASTINDEX_API_KEY", "PODCAST_INDEX_KEY"]),
        env(&["PODCASTINDEX_API_SECRET", "PODCAST_INDEX_SECRET"]),
    ) {
        return PodcastCredentials::checked(&key, &secret);
    }
    if let Some(bytes) = read(SERVICE, "credentials")? {
        let credentials: PodcastCredentials = serde_json::from_slice(&bytes)?;
        return PodcastCredentials::checked(&credentials.key, &credentials.secret);
    }
    let old = "de.jakende.transcription-macos.podcastindex";
    if let (Some(key), Some(secret)) = (
        read(old, "PODCAST_INDEX_KEY")?,
        read(old, "PODCAST_INDEX_SECRET")?,
    ) {
        return PodcastCredentials::checked(
            std::str::from_utf8(&key)?,
            std::str::from_utf8(&secret)?,
        );
    }
    bail!(
        "PodcastIndex-Zugang fehlt. API-Schlüssel und API-Geheimnis unter „PodcastIndex-Zugang“ hinterlegen oder eine RSS-Adresse öffnen."
    )
}
pub fn save(key: &str, secret: &str) -> Result<()> {
    let credentials = PodcastCredentials::checked(key, secret)?;
    security_framework::passwords::set_generic_password(
        SERVICE,
        "credentials",
        &serde_json::to_vec(&credentials)?,
    )?;
    Ok(())
}
