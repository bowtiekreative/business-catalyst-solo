//! Explicit native YouTube operations. Construction does not access credentials,
//! start a browser or send requests. Call each blocking method on a worker.
mod credentials;
mod http;
mod import;
pub use import::{
    ImportLimits, YoutubeImportPhase, YoutubeImportProgress, YoutubeImportRequest, import_clip,
    importer_available,
};
#[cfg(all(test, feature = "capture-test", target_os = "linux"))]
mod integration;
mod oauth;
mod protocol;
#[cfg(test)]
mod tests;
mod upload;

use crate::{ProjectStore, Result, media::Cancellation};
use catalyst_core::local::{self, PrivateNamespace, SaveLock};
use credentials::{OsKeychain, Record, Vault};
use http::{GoogleHttp, Transport};
use protocol::ClientConfig;
use serde::{Deserialize, Serialize};
#[cfg(test)]
use std::path::PathBuf;
use std::{
    fmt, fs,
    io::Read,
    net::TcpListener,
    path::Path,
    sync::{Arc, Mutex, MutexGuard},
    time::Instant,
};
use zeroize::Zeroizing;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ConnectionId(String);
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Privacy {
    Private,
    #[default]
    Unlisted,
    Public,
}
#[derive(Clone, Debug)]
pub struct YoutubeStatus {
    pub configured: bool,
    /// Identifies the reviewed credential set, not a verified channel name.
    pub connection: Option<ConnectionId>,
    pub pending: Option<PendingUpload>,
}
#[derive(Clone, Debug)]
pub struct PendingUpload {
    pub relative_path: String,
    pub title: String,
    pub privacy: Privacy,
    pub total_bytes: u64,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PublishRequest {
    pub relative_path: String,
    pub title: String,
    pub description: String,
    pub privacy: Privacy,
    pub reviewed_connection: ConnectionId,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UploadPhase {
    Preparing,
    Starting,
    Uploading,
    Recovering,
    Complete,
}
#[derive(Clone, Copy, Debug)]
pub struct UploadProgress {
    pub acknowledged_bytes: u64,
    pub total_bytes: u64,
    pub phase: UploadPhase,
}
#[derive(Clone, Debug)]
pub struct UploadedVideo {
    pub id: String,
    pub url: String,
    /// Actual returned status, including any enforced Private restriction.
    pub privacy: Privacy,
}
pub struct Authorization {
    listener: TcpListener,
    state: String,
    verifier: String,
    redirect: String,
    path: String,
    port: u16,
    config: ClientConfig,
    generation: String,
    browser_url: String,
    started: Instant,
}
impl Authorization {
    /// Open only after the user selects Connect, in the system browser. Never
    /// put this state-bearing URL in logs, task records or a project document.
    pub fn browser_url(&self) -> &str {
        &self.browser_url
    }
}
impl fmt::Debug for Authorization {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Authorization([redacted])")
    }
}
#[derive(Clone)]
pub struct YoutubeClient {
    services: Arc<Mutex<Services>>,
}
struct Services {
    vault: Vault,
    http: Arc<dyn Transport>,
    #[cfg(test)]
    state_root: Option<PathBuf>,
}
impl Services {
    fn process_lock(&self) -> Result<SaveLock> {
        #[cfg(test)]
        if let Some(root) = &self.state_root {
            return local::lock_named(root, PrivateNamespace::Sidebar, "youtube.lock")
                .map_err(|_| "Another Storyfold YouTube operation is running".into());
        }
        let root = local::user_state_directory("storyfold")?;
        local::lock_named(&root, PrivateNamespace::Sidebar, "youtube.lock")
            .map_err(|_| "Another Storyfold YouTube operation is running".into())
    }
    fn current(&self) -> Result<Record> {
        self.vault.load()
    }
    fn status(record: &Record) -> YoutubeStatus {
        YoutubeStatus {
            configured: record.client.is_some(),
            connection: record.tokens.as_ref().map(|t| t.connection.clone()),
            pending: record.pending_summary(),
        }
    }
    fn access(&self, record: &mut Record, cancel: &Cancellation) -> Result<String> {
        let tokens = record
            .tokens
            .as_ref()
            .ok_or("Connect YouTube before publishing")?;
        if tokens.expires_unix_ms <= oauth::now_ms()?.saturating_add(60_000) {
            let updated = oauth::refresh(
                record
                    .client
                    .as_ref()
                    .ok_or("Choose a Google Desktop client JSON first")?,
                tokens,
                self.http.as_ref(),
                cancel,
            )?;
            record.tokens = Some(updated);
            self.vault.save(record)?;
        }
        Ok(record
            .tokens
            .as_ref()
            .ok_or("Connect YouTube again")?
            .access
            .clone())
    }
}
impl YoutubeClient {
    /// No OS-keychain read/write, folder/lock creation, browser or HTTP request.
    pub fn system() -> Result<Self> {
        Ok(Self {
            services: Arc::new(Mutex::new(Services {
                vault: Vault::new(Arc::new(OsKeychain)),
                http: Arc::new(GoogleHttp::new()?),
                #[cfg(test)]
                state_root: None,
            })),
        })
    }
    fn services(&self) -> Result<MutexGuard<'_, Services>> {
        self.services
            .lock()
            .map_err(|_| "YouTube operation state was interrupted; reopen Storyfold".into())
    }
    pub fn status(&self) -> Result<YoutubeStatus> {
        let s = self.services()?;
        let _lock = s.process_lock()?;
        Ok(Services::status(&s.current()?))
    }
    pub fn configure_client(
        &self,
        selected_json: &Path,
        cancel: &Cancellation,
    ) -> Result<YoutubeStatus> {
        cancel.check()?;
        local::reject_file_links(selected_json)?;
        let before = fs::metadata(selected_json)?;
        if !before.is_file() || before.len() > 16 * 1024 {
            return Err("Choose the original Google Desktop client JSON smaller than16KiB".into());
        }
        let mut bytes = Zeroizing::new(Vec::new());
        fs::File::open(selected_json)?
            .take(16 * 1024 + 1)
            .read_to_end(&mut bytes)?;
        let config = protocol::parse_client(&bytes)?;
        local::reject_file_links(selected_json)?;
        let after = fs::metadata(selected_json)?;
        if before.len() != after.len() || before.modified()? != after.modified()? {
            return Err("Client JSON changed while reading; choose it again".into());
        }
        let s = self.services()?;
        let _lock = s.process_lock()?;
        let mut record = s.current()?;
        if record.pending.is_some() {
            return Err(
                "Resume or forget the pending upload before changing the Google client".into(),
            );
        }
        record.client = Some(config);
        record.client_generation = protocol::random_secret()?;
        record.tokens = None;
        cancel.check()?;
        s.vault.save(&record)?;
        Ok(Services::status(&record))
    }
    pub fn begin_connect(&self, cancel: &Cancellation) -> Result<Authorization> {
        cancel.check()?;
        let s = self.services()?;
        let _lock = s.process_lock()?;
        let mut record = s.current()?;
        if record.pending.is_some() {
            return Err(
                "Resume or forget the pending upload before connecting a different account".into(),
            );
        }
        let generation = protocol::random_secret()?;
        let authorization = oauth::begin(
            record
                .client
                .clone()
                .ok_or("Choose a Google Desktop client JSON first")?,
            generation.clone(),
        )?;
        // A returned handle spans operations/process-lock releases. Persist a
        // fresh attempt generation so an older consent cannot replace a newer
        // connection after two Connect actions finish out of order.
        record.client_generation = generation;
        cancel.check()?;
        s.vault.save(&record)?;
        Ok(authorization)
    }
    pub fn finish_connect(
        &self,
        authorization: Authorization,
        cancel: &Cancellation,
    ) -> Result<YoutubeStatus> {
        cancel.check()?;
        let s = self.services()?;
        let _lock = s.process_lock()?;
        let mut record = s.current()?;
        if record.client_generation != authorization.generation || record.pending.is_some() {
            return Err("Google client or upload changed; select Connect again".into());
        }
        let tokens = oauth::finish(authorization, s.http.as_ref(), cancel)?;
        cancel.check()?;
        record.tokens = Some(tokens);
        s.vault.save(&record)?;
        Ok(Services::status(&record))
    }
    pub fn disconnect(&self) -> Result<YoutubeStatus> {
        let s = self.services()?;
        let _lock = s.process_lock()?;
        let mut record = s.current()?;
        record.tokens = None;
        record.pending = None;
        // Disconnect also invalidates any not-yet-finished consent handle.
        record.client_generation = protocol::random_secret()?;
        s.vault.save(&record)?;
        Ok(Services::status(&record))
    }
    pub fn forget_client(&self) -> Result<()> {
        let s = self.services()?;
        let _lock = s.process_lock()?;
        s.vault.clear()
    }
    pub fn forget_pending_upload(&self) -> Result<()> {
        let s = self.services()?;
        let _lock = s.process_lock()?;
        let mut record = s.current()?;
        record.pending = None;
        s.vault.save(&record)
    }
    pub fn publish(
        &self,
        store: &ProjectStore,
        request: PublishRequest,
        cancel: &Cancellation,
        progress: impl FnMut(UploadProgress),
    ) -> Result<UploadedVideo> {
        request.validate()?;
        cancel.check()?;
        let s = self.services()?;
        let _lock = s.process_lock()?;
        upload::publish(&s, store, request, cancel, progress)
    }
    pub fn resume(
        &self,
        store: &ProjectStore,
        reviewed: &ConnectionId,
        cancel: &Cancellation,
        progress: impl FnMut(UploadProgress),
    ) -> Result<UploadedVideo> {
        cancel.check()?;
        let s = self.services()?;
        let _lock = s.process_lock()?;
        upload::resume(&s, store, reviewed, cancel, progress)
    }
}
