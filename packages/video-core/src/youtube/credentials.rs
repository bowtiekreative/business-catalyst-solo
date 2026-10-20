//! OS-keychain-only credentials. Two fixed bounded slots keep the previous
//! complete record readable if a staged write fails. Call under the per-user
//! process lock; this coordinates normal app instances, not hostile same-user code.
use super::protocol::{self, ClientConfig, MAX_SECRET_BYTES};
use super::{ConnectionId, PendingUpload, PublishRequest};
use crate::Result;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use zeroize::Zeroizing;

const SERVICE: &str = "com.bowtiekreative.storyfold.youtube.v1";
const ENTRY_BYTES: usize = 2000;
const DATA_BYTES: usize = ENTRY_BYTES - 44;
const MAX_CHUNKS: usize = MAX_SECRET_BYTES.div_ceil(DATA_BYTES) + 1;

pub(super) trait Keychain: Send + Sync {
    fn get(&self, name: &str) -> Result<Option<Vec<u8>>>;
    fn set(&self, name: &str, bytes: &[u8]) -> Result<()>;
    fn delete(&self, name: &str) -> Result<()>;
}
pub(super) struct OsKeychain;
fn store_error() -> Box<dyn std::error::Error + Send + Sync> {
    "Unlock the operating-system keyring and try again; no credential-file fallback is used".into()
}
impl Keychain for OsKeychain {
    fn get(&self, name: &str) -> Result<Option<Vec<u8>>> {
        #[cfg(any(target_os = "linux", target_os = "windows", target_os = "macos"))]
        {
            let entry = keyring::Entry::new(SERVICE, name).map_err(|_| store_error())?;
            match entry.get_secret() {
                Ok(bytes) => Ok(Some(bytes)),
                Err(keyring::Error::NoEntry) => Ok(None),
                Err(_) => Err(store_error()),
            }
        }
        #[cfg(not(any(target_os = "linux", target_os = "windows", target_os = "macos")))]
        {
            let _ = name;
            Err("YouTube keychain storage is unavailable on this platform".into())
        }
    }
    fn set(&self, name: &str, bytes: &[u8]) -> Result<()> {
        if bytes.len() > ENTRY_BYTES {
            return Err("YouTube keychain entry exceeds its storage bound".into());
        }
        #[cfg(any(target_os = "linux", target_os = "windows", target_os = "macos"))]
        {
            keyring::Entry::new(SERVICE, name)
                .map_err(|_| store_error())?
                .set_secret(bytes)
                .map_err(|_| store_error())
        }
        #[cfg(not(any(target_os = "linux", target_os = "windows", target_os = "macos")))]
        {
            let _ = (name, bytes);
            Err("YouTube keychain storage is unavailable on this platform".into())
        }
    }
    fn delete(&self, name: &str) -> Result<()> {
        #[cfg(any(target_os = "linux", target_os = "windows", target_os = "macos"))]
        {
            match keyring::Entry::new(SERVICE, name)
                .map_err(|_| store_error())?
                .delete_credential()
            {
                Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
                Err(_) => Err(store_error()),
            }
        }
        #[cfg(not(any(target_os = "linux", target_os = "windows", target_os = "macos")))]
        {
            let _ = name;
            Err("YouTube keychain storage is unavailable on this platform".into())
        }
    }
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Tokens {
    pub access: String,
    pub refresh: String,
    pub expires_unix_ms: u64,
    pub connection: ConnectionId,
}
impl std::fmt::Debug for Tokens {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Tokens([redacted])")
    }
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ResumeRecord {
    pub request: PublishRequest,
    pub project_binding: String,
    pub source: SourceFingerprint,
    pub session: String,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct SourceFingerprint {
    pub bytes: u64,
    pub modified_ns: u128,
    pub sha256: String,
    #[cfg(unix)]
    pub dev: u64,
    #[cfg(unix)]
    pub ino: u64,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Record {
    pub schema: u32,
    pub client: Option<ClientConfig>,
    pub client_generation: String,
    pub tokens: Option<Tokens>,
    pub pending: Option<ResumeRecord>,
}
impl Default for Record {
    fn default() -> Self {
        Self {
            schema: 1,
            client: None,
            client_generation: String::new(),
            tokens: None,
            pending: None,
        }
    }
}
impl Record {
    pub fn pending_summary(&self) -> Option<PendingUpload> {
        self.pending.as_ref().map(|p| PendingUpload {
            relative_path: p.request.relative_path.clone(),
            title: p.request.title.clone(),
            privacy: p.request.privacy,
            total_bytes: p.source.bytes,
        })
    }
    fn validate(&self) -> Result<()> {
        if self.schema != 1
            || self.client.as_ref().is_some_and(|c| {
                !protocol::bounded_secret(&c.id, 256)
                    || !protocol::bounded_secret(&c.secret, 1024)
                    || !protocol::bounded_secret(&self.client_generation, 64)
            })
            || self.tokens.as_ref().is_some_and(|t| {
                self.client.is_none()
                    || !protocol::bounded_secret(&t.access, 4096)
                    || !protocol::bounded_secret(&t.refresh, 4096)
                    || !protocol::bounded_secret(&t.connection.0, 64)
            })
        {
            return Err(
                "YouTube keychain record is invalid; reconnect or forget the client".into(),
            );
        }
        if let Some(p) = &self.pending {
            p.request.validate()?;
            protocol::validate_session_url(&p.session)?;
            if p.source.bytes == 0 || p.project_binding.len() != 43 || p.source.sha256.len() != 43 {
                return Err("YouTube resumable state is invalid; forget the pending upload".into());
            }
        }
        Ok(())
    }
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Manifest {
    slot: u8,
    generation: String,
    count: usize,
    length: usize,
    hash: String,
}
impl Manifest {
    fn validate(&self) -> Result<()> {
        if self.slot > 1
            || self.generation.len() != 43
            || !protocol::bounded_secret(&self.generation, 43)
            || self.count == 0
            || self.count > MAX_CHUNKS
            || self.length == 0
            || self.length > MAX_SECRET_BYTES
            || self.hash.len() != 43
        {
            return Err(
                "YouTube keychain manifest is invalid; forget the client to reset it".into(),
            );
        }
        Ok(())
    }
}
pub(super) struct Vault {
    backend: Arc<dyn Keychain>,
}
impl Vault {
    pub fn new(backend: Arc<dyn Keychain>) -> Self {
        Self { backend }
    }
    fn manifest(&self) -> Result<Option<Manifest>> {
        let Some(bytes) = self.backend.get("manifest")? else {
            return Ok(None);
        };
        let bytes = Zeroizing::new(bytes);
        if bytes.len() > ENTRY_BYTES {
            return Err("YouTube keychain manifest exceeds its storage bound".into());
        }
        let manifest: Manifest = serde_json::from_slice(&bytes)
            .map_err(|_| "YouTube keychain manifest is invalid; forget the client to reset it")?;
        manifest.validate()?;
        Ok(Some(manifest))
    }
    pub fn load(&self) -> Result<Record> {
        let Some(manifest) = self.manifest()? else {
            return Ok(Record::default());
        };
        let mut bytes = Zeroizing::new(Vec::with_capacity(manifest.length));
        let prefix = format!("{}\n", manifest.generation);
        for index in 0..manifest.count {
            let part = Zeroizing::new(
                self.backend
                    .get(&format!("slot-{}-{index:02}", manifest.slot))?
                    .ok_or(
                        "YouTube keychain record is incomplete; reconnect or forget the client",
                    )?,
            );
            if part.len() > ENTRY_BYTES || !part.starts_with(prefix.as_bytes()) {
                return Err("YouTube keychain record has mixed or corrupt chunks".into());
            }
            bytes.extend_from_slice(&part[prefix.len()..]);
            if bytes.len() > MAX_SECRET_BYTES {
                return Err("YouTube keychain record exceeds its storage bound".into());
            }
        }
        if bytes.len() != manifest.length || protocol::digest(&bytes) != manifest.hash {
            return Err("YouTube keychain record failed its integrity check".into());
        }
        let record: Record = serde_json::from_slice(&bytes)
            .map_err(|_| "YouTube keychain record is invalid; reconnect or forget the client")?;
        record.validate()?;
        Ok(record)
    }
    pub fn save(&self, record: &Record) -> Result<()> {
        record.validate()?;
        let bytes = Zeroizing::new(serde_json::to_vec(record)?);
        if bytes.len() > MAX_SECRET_BYTES {
            return Err("YouTube keychain record exceeds32KiB; shorten metadata and retry".into());
        }
        let text =
            std::str::from_utf8(&bytes).map_err(|_| "Invalid YouTube credential encoding")?;
        let slot = self.manifest()?.map_or(0, |m| 1 - m.slot);
        let generation = protocol::random_secret()?;
        let mut start = 0;
        let mut count = 0;
        while start < bytes.len() {
            let mut end = (start + DATA_BYTES).min(bytes.len());
            while !text.is_char_boundary(end) {
                end -= 1;
            }
            let part = Zeroizing::new(format!("{generation}\n{}", &text[start..end]));
            self.backend
                .set(&format!("slot-{slot}-{count:02}"), part.as_bytes())?;
            start = end;
            count += 1;
        }
        let manifest = Manifest {
            slot,
            generation,
            count,
            length: bytes.len(),
            hash: protocol::digest(&bytes),
        };
        manifest.validate()?;
        // A failed staging write cannot alter the previous active slot. The
        // per-user OS lock prevents normal app processes racing this manifest.
        self.backend
            .set("manifest", &serde_json::to_vec(&manifest)?)?;
        Ok(())
    }
    pub fn clear(&self) -> Result<()> {
        let mut error = self.backend.delete("manifest").err();
        for slot in 0..2 {
            for index in 0..MAX_CHUNKS {
                if let Err(e) = self.backend.delete(&format!("slot-{slot}-{index:02}")) {
                    error.get_or_insert(e);
                }
            }
        }
        match error {
            Some(e) => Err(e),
            None => Ok(()),
        }
    }
}
