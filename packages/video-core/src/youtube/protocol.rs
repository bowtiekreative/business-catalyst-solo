//! Pure bounded protocol validation. Never sends a request or opens a credential.
use super::{ConnectionId, Privacy, PublishRequest};
use crate::Result;
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashSet;
use url::Url;

pub(super) const AUTH_URL: &str = "https://accounts.google.com/o/oauth2/v2/auth";
pub(super) const TOKEN_URL: &str = "https://oauth2.googleapis.com/token";
pub(super) const UPLOAD_URL: &str =
    "https://www.googleapis.com/upload/youtube/v3/videos?uploadType=resumable&part=snippet,status";
pub(super) const SCOPE: &str = "https://www.googleapis.com/auth/youtube.upload";
pub(super) const MAX_SECRET_BYTES: usize = 32 * 1024;
pub(super) const MAX_RESPONSE: u64 = 64 * 1024;
pub(super) const CHUNK_BYTES: usize = 1024 * 1024;

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ClientConfig {
    pub id: String,
    pub secret: String,
}
impl std::fmt::Debug for ClientConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("ClientConfig([redacted])")
    }
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ClientFile {
    installed: Installed,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Installed {
    client_id: String,
    client_secret: String,
    auth_uri: String,
    token_uri: String,
    redirect_uris: Vec<String>,
    #[serde(default)]
    project_id: Option<String>,
    #[serde(default)]
    auth_provider_x509_cert_url: Option<String>,
}
pub(super) fn parse_client(bytes: &[u8]) -> Result<ClientConfig> {
    if bytes.is_empty() || bytes.len() > 16 * 1024 {
        return Err("Choose a Google Desktop OAuth client JSON file smaller than16KiB".into());
    }
    // Do not surface serde's error: an unexpected field may itself be a secret.
    let file: ClientFile = serde_json::from_slice(bytes)
        .map_err(|_| "Choose the original Google Desktop OAuth client JSON (installed client)")?;
    let c = file.installed;
    let prefix = c
        .client_id
        .strip_suffix(".apps.googleusercontent.com")
        .ok_or("Google Desktop client ID is invalid")?;
    if prefix.is_empty()
        || c.client_id.len() > 256
        || !prefix
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-')
        || !bounded_secret(&c.client_secret, 1024)
        || !matches!(
            c.auth_uri.as_str(),
            AUTH_URL | "https://accounts.google.com/o/oauth2/auth"
        )
        || c.token_uri != TOKEN_URL
        || c.auth_provider_x509_cert_url
            .as_deref()
            .is_some_and(|uri| uri != "https://www.googleapis.com/oauth2/v1/certs")
        || c.project_id.as_ref().is_some_and(|id| id.len() > 256)
        || c.redirect_uris.is_empty()
        || c.redirect_uris.len() > 8
        || !c.redirect_uris.iter().any(|uri| {
            Url::parse(uri).is_ok_and(|u| {
                u.scheme() == "http"
                    && matches!(u.host_str(), Some("localhost" | "127.0.0.1" | "[::1]"))
                    && u.username().is_empty()
                    && u.password().is_none()
                    && u.query().is_none()
                    && u.fragment().is_none()
                    && u.path() == "/"
            })
        })
    {
        return Err(
            "Client JSON must be an original Google Desktop app with loopback redirects".into(),
        );
    }
    Ok(ClientConfig {
        id: c.client_id,
        secret: c.client_secret,
    })
}
pub(super) fn bounded_secret(value: &str, max: usize) -> bool {
    !value.is_empty() && value.len() <= max && value.bytes().all(|b| b.is_ascii_graphic())
}
pub(super) fn challenge(verifier: &str) -> String {
    URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()))
}
pub(super) fn digest(bytes: &[u8]) -> String {
    URL_SAFE_NO_PAD.encode(Sha256::digest(bytes))
}
pub(super) fn random_secret() -> Result<String> {
    use rand::TryRngCore;
    let mut bytes = [0_u8; 32];
    rand::rngs::OsRng
        .try_fill_bytes(&mut bytes)
        .map_err(|_| "Secure randomness is unavailable; YouTube connection was not started")?;
    Ok(URL_SAFE_NO_PAD.encode(bytes))
}
pub(super) fn connection() -> Result<ConnectionId> {
    Ok(ConnectionId(random_secret()?))
}
pub(super) fn authorization_url(
    config: &ClientConfig,
    redirect: &str,
    state: &str,
    verifier: &str,
) -> Result<String> {
    let mut url = Url::parse(AUTH_URL)?;
    url.query_pairs_mut().extend_pairs([
        ("client_id", config.id.as_str()),
        ("redirect_uri", redirect),
        ("response_type", "code"),
        ("scope", SCOPE),
        ("state", state),
        ("code_challenge", challenge(verifier).as_str()),
        ("code_challenge_method", "S256"),
        ("access_type", "offline"),
        ("prompt", "consent select_account"),
    ]);
    Ok(url.into())
}
pub(super) fn validate_session_url(value: &str) -> Result<()> {
    let url = Url::parse(value).map_err(|_| "Google returned an invalid upload session")?;
    if value.len() > 4096
        || url.scheme() != "https"
        || !matches!(
            url.host_str(),
            Some("www.googleapis.com" | "youtube.googleapis.com")
        )
        || url.port_or_known_default() != Some(443)
        || !url.username().is_empty()
        || url.password().is_some()
        || url.fragment().is_some()
        || url.path() != "/upload/youtube/v3/videos"
    {
        return Err("Google returned an untrusted upload session; no video was sent".into());
    }
    let mut names = HashSet::new();
    let mut upload_id = false;
    for (key, value) in url.query_pairs() {
        if !names.insert(key.to_string()) || key.len() > 64 || value.len() > 3072 {
            return Err("Google returned an invalid upload session".into());
        }
        if key == "upload_id" {
            upload_id = !value.is_empty();
        }
    }
    if !upload_id {
        return Err("Google returned an upload session without an identifier".into());
    }
    Ok(())
}
/// Acknowledged prefix length; absent Range means no bytes persisted. Never
/// infer progress from bytes sent, a partial write or a client-side timer.
pub(super) fn acknowledged(range: Option<&str>, total: u64, sent_end: u64) -> Result<u64> {
    let Some(range) = range else { return Ok(0) };
    let value = range.strip_prefix("bytes=").unwrap_or(range);
    let end = value
        .strip_prefix("0-")
        .filter(|v| !v.is_empty() && v.bytes().all(|b| b.is_ascii_digit()))
        .ok_or("YouTube returned an invalid acknowledged byte range")?
        .parse::<u64>()
        .ok()
        .and_then(|n| n.checked_add(1))
        .ok_or("YouTube returned an invalid acknowledged byte range")?;
    if end > total || end > sent_end {
        return Err("YouTube acknowledged bytes outside this upload".into());
    }
    Ok(end)
}
impl PublishRequest {
    pub fn validate(&self) -> Result<()> {
        catalyst_core::local::validate_relative_path(&self.relative_path)?;
        if !self.relative_path.starts_with("exports/")
            || self.relative_path.split('/').count() != 2
            || !self.relative_path.ends_with(".mp4")
        {
            return Err("Choose a rendered MP4 in this project's exports folder".into());
        }
        if self.title.trim().is_empty()
            || self.title.chars().count() > 100
            || self
                .title
                .chars()
                .any(|c| c.is_control() || matches!(c, '<' | '>'))
            || self.description.len() > 5000
            || self.description.chars().any(|c| {
                matches!(c, '<' | '>') || (c.is_control() && !matches!(c, '\n' | '\r' | '\t'))
            })
        {
            return Err("Use a title of1–100characters and a description of at most5,000UTF-8bytes, without angle brackets".into());
        }
        if !bounded_secret(&self.reviewed_connection.0, 64) {
            return Err("Review the current YouTube connection before publishing".into());
        }
        Ok(())
    }
}
pub(super) fn metadata(request: &PublishRequest) -> serde_json::Value {
    serde_json::json!({
        "snippet": {"title":request.title,"description":request.description,"categoryId":"22"},
        "status": {"privacyStatus": match request.privacy {
            Privacy::Private => "private", Privacy::Unlisted => "unlisted", Privacy::Public => "public"
        }}
    })
}
pub(super) fn video_id(bytes: &[u8]) -> Result<String> {
    let value: serde_json::Value = serde_json::from_slice(bytes).map_err(
        |_| "YouTube returned an invalid completion response; check this upload's status",
    )?;
    let id = value["id"]
        .as_str()
        .filter(|id| {
            id.len() == 11
                && id
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
        })
        .ok_or("YouTube returned no valid video ID; resume to check completion")?;
    Ok(id.to_owned())
}
