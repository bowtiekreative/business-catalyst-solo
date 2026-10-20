//! Confined, source-bound resumable MP4 upload. No background retry/new session.
use super::{
    ConnectionId, Privacy, PublishRequest, Services, UploadPhase, UploadProgress, UploadedVideo,
    credentials::{Record, ResumeRecord, SourceFingerprint},
    http::{Method, Request, Response},
    protocol::{self, CHUNK_BYTES, UPLOAD_URL},
};
use crate::{
    ProjectStore, Result,
    media::{self, Cancellation},
};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use catalyst_core::local::{self, FileMode, PathPolicy};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::{Read, Seek, SeekFrom},
    path::PathBuf,
    thread,
    time::{Duration, UNIX_EPOCH},
};

struct Source {
    root: PathBuf,
    relative: String,
    path: PathBuf,
    file: fs::File,
    fingerprint: SourceFingerprint,
}
impl Source {
    fn open(store: &ProjectStore, relative: &str, cancel: &Cancellation) -> Result<Self> {
        let path = local::confined_file(
            store.root(),
            relative,
            PathPolicy::LocalProject,
            FileMode::Existing,
        )?;
        let mut file = fs::File::open(&path)?;
        let before = stamp(&path)?;
        if before.bytes == 0 {
            return Err("Rendered MP4 is empty".into());
        }
        let mut header = [0_u8; 12];
        file.read_exact(&mut header)
            .map_err(|_| "Rendered MP4 has no complete container header")?;
        if &header[4..8] != b"ftyp" {
            return Err(
                "Choose the rendered MP4, rather than a renamed container or playlist".into(),
            );
        }
        file.seek(SeekFrom::Start(0))?;
        media::probe_owned(&path, cancel)?;
        let mut hash = Sha256::new();
        let mut buffer = [0_u8; 65_536];
        loop {
            cancel.check()?;
            let n = file.read(&mut buffer)?;
            if n == 0 {
                break;
            }
            hash.update(&buffer[..n]);
        }
        if stamp(&path)? != before {
            return Err("Rendered MP4 changed while preparing the upload".into());
        }
        let mut fingerprint = before;
        fingerprint.sha256 = URL_SAFE_NO_PAD.encode(hash.finalize());
        Ok(Self {
            root: store.root().into(),
            relative: relative.into(),
            path,
            file,
            fingerprint,
        })
    }
    fn unchanged(&self) -> Result<()> {
        let path = local::confined_file(
            &self.root,
            &self.relative,
            PathPolicy::LocalProject,
            FileMode::Existing,
        )?;
        let current = stamp(&path)?;
        let mut expected = self.fingerprint.clone();
        expected.sha256.clear();
        if path != self.path || current != expected {
            return Err(
                "Rendered MP4 changed; this upload cannot continue with different media".into(),
            );
        }
        let open = self.file.metadata()?;
        if open.len() != current.bytes
            || open.modified()?.duration_since(UNIX_EPOCH)?.as_nanos() != current.modified_ns
        {
            return Err(
                "Open upload source changed; local MP4 was not altered by Storyfold".into(),
            );
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            if open.dev() != current.dev || open.ino() != current.ino {
                return Err("Upload source was replaced".into());
            }
        }
        Ok(())
    }
    fn chunk(&mut self, offset: u64, cancel: &Cancellation) -> Result<Vec<u8>> {
        cancel.check()?;
        self.unchanged()?;
        let remaining = self
            .fingerprint
            .bytes
            .checked_sub(offset)
            .ok_or("Upload offset exceeds source")?;
        let size = usize::try_from(remaining.min(CHUNK_BYTES as u64))?;
        if size == 0 {
            return Err("No upload bytes remain; check the pending upload status".into());
        }
        self.file.seek(SeekFrom::Start(offset))?;
        let mut bytes = vec![0; size];
        self.file.read_exact(&mut bytes)?;
        self.unchanged()?;
        cancel.check()?;
        Ok(bytes)
    }
}
fn stamp(path: &std::path::Path) -> Result<SourceFingerprint> {
    local::reject_file_links(path)?;
    let m = fs::symlink_metadata(path)?;
    if !m.is_file() || m.file_type().is_symlink() {
        return Err("Choose a regular rendered MP4".into());
    }
    #[cfg(unix)]
    use std::os::unix::fs::MetadataExt;
    Ok(SourceFingerprint {
        bytes: m.len(),
        modified_ns: m.modified()?.duration_since(UNIX_EPOCH)?.as_nanos(),
        sha256: String::new(),
        #[cfg(unix)]
        dev: m.dev(),
        #[cfg(unix)]
        ino: m.ino(),
    })
}
fn binding(store: &ProjectStore) -> String {
    protocol::digest(store.root().as_os_str().as_encoded_bytes())
}
fn reviewed(record: &Record, connection: &ConnectionId) -> Result<()> {
    if record
        .tokens
        .as_ref()
        .is_none_or(|t| &t.connection != connection)
    {
        return Err(
            "YouTube connection changed; review the current connection before publishing".into(),
        );
    }
    Ok(())
}
pub(super) fn publish(
    s: &Services,
    store: &ProjectStore,
    request: PublishRequest,
    cancel: &Cancellation,
    mut progress: impl FnMut(UploadProgress),
) -> Result<UploadedVideo> {
    let mut record = s.current()?;
    reviewed(&record, &request.reviewed_connection)?;
    if record.pending.is_some() {
        return Err("Resume or forget the pending upload before starting another".into());
    }
    progress(UploadProgress {
        acknowledged_bytes: 0,
        total_bytes: 0,
        phase: UploadPhase::Preparing,
    });
    let source = Source::open(store, &request.relative_path, cancel)?;
    let total = source.fingerprint.bytes;
    let access = s.access(&mut record, cancel)?;
    progress(UploadProgress {
        acknowledged_bytes: 0,
        total_bytes: total,
        phase: UploadPhase::Starting,
    });
    cancel.check()?;
    source.unchanged()?;
    let response = s.http.send(Request {
        method: Method::Post,
        url: UPLOAD_URL.into(),
        headers: vec![
            ("Authorization", format!("Bearer {access}")),
            ("Content-Type", "application/json; charset=UTF-8".into()),
            ("X-Upload-Content-Type", "video/mp4".into()),
            ("X-Upload-Content-Length", total.to_string()),
        ],
        body: serde_json::to_vec(&protocol::metadata(&request))?,
    })?;
    if !matches!(response.status, 200 | 201) {
        return Err(
            "Google refused to start the upload; check connection, quota and account access".into(),
        );
    }
    let session = response
        .location
        .ok_or("Google returned no resumable upload location")?;
    protocol::validate_session_url(&session)?;
    record.pending = Some(ResumeRecord {
        request,
        project_binding: binding(store),
        source: source.fingerprint.clone(),
        session,
    });
    // Save the session before any MP4 bytes are sent. If cancellation happened
    // during session creation, explicit Resume can recover this same session.
    s.vault.save(&record)?;
    transfer(s, record, source, false, cancel, &mut progress)
}
pub(super) fn resume(
    s: &Services,
    store: &ProjectStore,
    connection: &ConnectionId,
    cancel: &Cancellation,
    mut progress: impl FnMut(UploadProgress),
) -> Result<UploadedVideo> {
    let record = s.current()?;
    reviewed(&record, connection)?;
    let pending = record
        .pending
        .as_ref()
        .ok_or("There is no pending upload to resume")?;
    if pending.project_binding != binding(store)
        || &pending.request.reviewed_connection != connection
    {
        return Err("Pending upload belongs to a different project or YouTube connection".into());
    }
    pending.request.validate()?;
    let source = Source::open(store, &pending.request.relative_path, cancel)?;
    if source.fingerprint != pending.source {
        return Err(
            "Rendered MP4 changed since the upload began; the pending session was not reused"
                .into(),
        );
    }
    transfer(s, record, source, true, cancel, &mut progress)
}
fn transfer(
    s: &Services,
    mut record: Record,
    mut source: Source,
    mut query_status: bool,
    cancel: &Cancellation,
    progress: &mut impl FnMut(UploadProgress),
) -> Result<UploadedVideo> {
    let pending = record.pending.as_ref().ok_or("Missing pending upload")?;
    let session = pending.session.clone();
    protocol::validate_session_url(&session)?;
    let total = source.fingerprint.bytes;
    let mut acknowledged = 0;
    let mut max_sent = if query_status { total } else { 0 };
    let mut failures = 0_u8;
    let mut refreshed = false;
    loop {
        cancel.check()?;
        source.unchanged()?;
        let access = s.access(&mut record, cancel)?;
        let was_query = query_status;
        let (body, range, sent_end) = if query_status {
            progress(UploadProgress {
                acknowledged_bytes: acknowledged,
                total_bytes: total,
                phase: UploadPhase::Recovering,
            });
            (Vec::new(), format!("bytes */{total}"), max_sent)
        } else {
            let body = source.chunk(acknowledged, cancel)?;
            let end = acknowledged
                .checked_add(body.len() as u64)
                .ok_or("Upload range overflows")?;
            max_sent = max_sent.max(end);
            (
                body,
                format!("bytes {}-{}/{total}", acknowledged, end - 1),
                end,
            )
        };
        cancel.check()?;
        let response = s.http.send(Request {
            method: Method::Put,
            url: session.clone(),
            headers: vec![
                ("Authorization", format!("Bearer {access}")),
                ("Content-Type", "video/mp4".into()),
                ("Content-Range", range),
                ("Content-Length", body.len().to_string()),
            ],
            body,
        });
        source.unchanged()?;
        let response = match response {
            Ok(r) => r,
            Err(_) => {
                retry(&mut failures, cancel)?;
                query_status = true;
                continue;
            }
        };
        match response.status {
            200 | 201 => {
                let uploaded = completion(&response)?;
                record.pending = None;
                s.vault.save(&record).map_err(|_| "Video uploaded, but local pending state could not be cleared; Resume checks its existing completion")?;
                progress(UploadProgress { acknowledged_bytes: total, total_bytes: total, phase: UploadPhase::Complete });
                return Ok(uploaded);
            }
            308 => {
                let next = protocol::acknowledged(response.range.as_deref(), total, sent_end)?;
                if next < acknowledged { return Err("YouTube lost previously acknowledged upload bytes; resume to check status".into()); }
                if next == total || (next == acknowledged && !was_query) {
                    retry(&mut failures, cancel)?;
                    query_status = true;
                } else {
                    if next > acknowledged { failures = 0; }
                    refreshed = false;
                    query_status = false;
                }
                acknowledged = next;
                progress(UploadProgress { acknowledged_bytes: acknowledged, total_bytes: total, phase: UploadPhase::Uploading });
            }
            401 if !refreshed => {
                record.tokens.as_mut().ok_or("Connect YouTube again")?.expires_unix_ms = 0;
                s.access(&mut record, cancel)?;
                refreshed = true;
                retry(&mut failures, cancel)?;
                query_status = true;
            }
            408 | 429 | 500 | 502 | 503 | 504 => {
                retry(&mut failures, cancel)?;
                query_status = true;
            }
            404 | 410 => return Err("Pending YouTube upload expired; forget it before explicitly starting a new upload".into()),
            _ => return Err("Google refused the upload; check permission, quota and channel access, then explicitly resume if appropriate".into()),
        }
    }
}
fn retry(failures: &mut u8, cancel: &Cancellation) -> Result<()> {
    *failures += 1;
    if *failures > 4 {
        return Err(
            "YouTube upload is still interrupted; use Resume to check the same pending upload"
                .into(),
        );
    }
    let delay = Duration::from_millis(200 * 2_u64.pow(u32::from(*failures - 1)));
    let started = std::time::Instant::now();
    while started.elapsed() < delay {
        cancel.check()?;
        thread::sleep(Duration::from_millis(20));
    }
    Ok(())
}
fn completion(response: &Response) -> Result<UploadedVideo> {
    let id = protocol::video_id(&response.body)?;
    #[derive(Deserialize)]
    struct Completed {
        status: CompletedStatus,
    }
    #[derive(Deserialize)]
    struct CompletedStatus {
        #[serde(rename = "privacyStatus")]
        privacy: Privacy,
    }
    let actual: Completed = serde_json::from_slice(&response.body).map_err(
        |_| "YouTube completion omitted actual privacy; resume to check the existing upload",
    )?;
    Ok(UploadedVideo {
        url: format!("https://www.youtube.com/watch?v={id}"),
        id,
        privacy: actual.status.privacy,
    })
}

#[cfg(test)]
mod tests;
