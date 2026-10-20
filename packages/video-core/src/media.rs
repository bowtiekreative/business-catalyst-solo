//! Explicit local media operations. Call blocking methods from a worker thread.
use crate::{ClipId, Edit, EditOutcome, Editor, ProjectStore, Result};
use catalyst_core::local::{self, FileMode, PathPolicy, PendingFile};
use serde::{Deserialize, Serialize};
use std::{
    fs,
    io::{Read, Write},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

#[derive(Clone, Default)]
pub struct Cancellation(Arc<AtomicBool>);
impl Cancellation {
    pub fn cancel(&self) {
        self.0.store(true, Ordering::Release);
    }
    pub fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::Acquire)
    }
    pub(crate) fn check(&self) -> Result<()> {
        if self.is_cancelled() {
            Err("Operation cancelled; existing media is unchanged".into())
        } else {
            Ok(())
        }
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MediaInfo {
    pub duration_ms: u64,
    pub width: u32,
    pub height: u32,
    pub has_audio: bool,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ImportedMedia {
    #[serde(skip)]
    root: PathBuf,
    relative: String,
    name: String,
    info: MediaInfo,
}
impl ImportedMedia {
    pub fn path(&self) -> &str {
        &self.relative
    }
    pub fn name(&self) -> &str {
        &self.name
    }
    pub fn info(&self) -> &MediaInfo {
        &self.info
    }
    pub(crate) fn published(
        root: &Path,
        relative: String,
        name: String,
        info: MediaInfo,
    ) -> Result<Self> {
        local::confined_file(
            root,
            &relative,
            PathPolicy::LocalProject,
            FileMode::Existing,
        )?;
        if !relative.starts_with("media/")
            || info.duration_ms == 0
            || info.width == 0
            || info.height == 0
        {
            return Err("Invalid published video metadata".into());
        }
        let mut validation = crate::Studio::default();
        validation.rename(&name)?;
        Ok(Self {
            root: root.to_owned(),
            relative,
            name,
            info,
        })
    }
    #[cfg(target_os = "linux")]
    pub(crate) fn rebind(self, root: &Path) -> Result<Self> {
        Self::published(root, self.relative, self.name, self.info)
    }
    pub fn append_to(&self, store: &ProjectStore, editor: &mut Editor) -> Result<ClipId> {
        let (clip, edits) = self.append_edits(store, editor)?;
        let outcome = editor.apply_batch(edits)?;
        if outcome.last() != Some(&EditOutcome::ClipAdded(clip)) {
            return Err("Media append did not create a clip".into());
        }
        Ok(clip)
    }
    pub(crate) fn append_edits(
        &self,
        store: &ProjectStore,
        editor: &Editor,
    ) -> Result<(ClipId, Vec<Edit>)> {
        if self.root != store.root() {
            return Err("This media belongs to another project".into());
        }
        local::confined_file(
            store.root(),
            &self.relative,
            PathPolicy::LocalProject,
            FileMode::Existing,
        )?;
        let source = editor.studio().next_media_id();
        let clip = editor.studio().next_clip_id();
        let edits = vec![
            Edit::AddMedia {
                path: self.relative.clone(),
                duration_ms: self.info.duration_ms,
            },
            Edit::AddClip {
                source,
                name: self.name.clone(),
                in_ms: 0,
                out_ms: self.info.duration_ms,
            },
        ];
        Ok((clip, edits))
    }
}

pub fn probe_file(source: impl AsRef<Path>, cancel: &Cancellation) -> Result<MediaInfo> {
    let source = selected_file(source.as_ref())?;
    probe_owned(&source, cancel)
}
pub fn import_media(
    store: &ProjectStore,
    source: impl AsRef<Path>,
    cancel: &Cancellation,
) -> Result<ImportedMedia> {
    cancel.check()?;
    let source = selected_file(source.as_ref())?;
    let before = fs::metadata(&source)?;
    let original = probe_owned(&source, cancel)?;
    let filename = source
        .file_stem()
        .and_then(|name| name.to_str())
        .unwrap_or("Imported video");
    let name: String = filename
        .chars()
        .filter(|c| !c.is_control())
        .take(120)
        .collect();
    let name = if name.trim().is_empty() {
        "Imported video".into()
    } else {
        name.trim().to_owned()
    };
    let mut name_check = crate::Studio::default();
    name_check.rename(&name)?;
    let extension = source
        .extension()
        .and_then(|part| part.to_str())
        .ok_or("Choose a supported video file")?
        .to_ascii_lowercase();
    let relative = format!("media/import-{}.{}", unique_id(), extension);
    let mut pending = PendingFile::prepare(store.root(), &relative)?;
    let mut input = fs::File::open(&source)?;
    let mut buffer = [0_u8; 65_536];
    let mut remaining = before.len();
    while remaining > 0 {
        cancel.check()?;
        let amount = usize::try_from(remaining.min(buffer.len() as u64))
            .map_err(|_| "Invalid media copy size")?;
        let bytes = input.read(&mut buffer[..amount])?;
        if bytes == 0 {
            return Err("Source became shorter during import; no media was published".into());
        }
        pending.file_mut().write_all(&buffer[..bytes])?;
        remaining -= bytes as u64;
    }
    let after = fs::metadata(&source)?;
    if before.len() != after.len() || before.modified()? != after.modified()? {
        return Err("Source changed during import; no media was published".into());
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if before.dev() != after.dev() || before.ino() != after.ino() {
            return Err("Source changed during import".into());
        }
    }
    let copied = probe_owned(pending.path(), cancel)?;
    if original != copied {
        return Err("Copied video metadata changed during import".into());
    }
    cancel.check()?;
    pending.publish()?;
    ImportedMedia::published(store.root(), relative, name, copied)
}
fn selected_file(source: &Path) -> Result<PathBuf> {
    let meta = fs::symlink_metadata(source)?;
    if !meta.is_file() || meta.file_type().is_symlink() {
        return Err("Choose a regular local video file".into());
    }
    local::reject_file_links(source)?;
    let extension = source
        .extension()
        .and_then(|part| part.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    if !matches!(
        extension.as_str(),
        "mp4" | "mov" | "mkv" | "webm" | "avi" | "m4v" | "ts"
    ) {
        return Err("Choose MP4, MOV, MKV, WebM, AVI, M4V or TS video; URLs and playlists are not supported".into());
    }
    Ok(source.canonicalize()?)
}
pub(crate) fn unique_id() -> String {
    static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let time = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    format!(
        "{time:x}-{}-{}",
        std::process::id(),
        COUNTER.fetch_add(1, Ordering::Relaxed)
    )
}
pub(crate) fn tool(name: &str) -> Result<PathBuf> {
    #[cfg(target_os = "linux")]
    for parent in ["/usr/bin", "/usr/local/bin", "/bin"] {
        let path = Path::new(parent).join(name);
        if path.is_file() {
            return Ok(path);
        }
    }
    #[cfg(not(target_os = "linux"))]
    if let Some(paths) = std::env::var_os("PATH") {
        for parent in std::env::split_paths(&paths) {
            let filename = if cfg!(windows) {
                format!("{name}.exe")
            } else {
                name.to_owned()
            };
            let path = parent.join(filename);
            if path.is_file() {
                return Ok(path);
            }
        }
    }
    Err("System FFmpeg and ffprobe must be installed; no automatic download was attempted".into())
}
pub(crate) struct BoundedOutput {
    pub stdout: Vec<u8>,
}
pub(crate) fn run_bounded(
    command: &mut Command,
    timeout: Duration,
    cancel: &Cancellation,
    stdout_limit: usize,
) -> Result<BoundedOutput> {
    cancel.check()?;
    let mut child = command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|_| "Media tool could not start")?;
    let stdout = child.stdout.take().ok_or("Missing media stdout")?;
    let stderr = child.stderr.take().ok_or("Missing media stderr")?;
    let overflow = Arc::new(AtomicBool::new(false));
    fn drain(mut stream: impl Read, limit: usize, overflow: Arc<AtomicBool>) -> Vec<u8> {
        let mut output = Vec::new();
        let mut bytes = [0; 8192];
        while let Ok(count) = stream.read(&mut bytes) {
            if count == 0 {
                break;
            }
            if output.len() + count > limit {
                overflow.store(true, Ordering::Release);
            } else {
                output.extend_from_slice(&bytes[..count]);
            }
        }
        output
    }
    let stdout_overflow = overflow.clone();
    let reader = thread::spawn(move || drain(stdout, stdout_limit, stdout_overflow));
    let stderr_overflow = overflow.clone();
    let errors = thread::spawn(move || drain(stderr, 65_536, stderr_overflow));
    let started = Instant::now();
    let result: Result<()> = loop {
        if cancel.is_cancelled() || started.elapsed() > timeout || overflow.load(Ordering::Acquire)
        {
            let _ = child.kill();
            let _ = child.wait();
            break Err("Media operation cancelled, timed out or exceeded its output limit".into());
        }
        match child.try_wait() {
            Ok(Some(status)) => {
                break if status.success() {
                    Ok(())
                } else {
                    Err("Media tool failed; source and buffered recordings were retained".into())
                };
            }
            Ok(None) => thread::sleep(Duration::from_millis(10)),
            Err(_) => {
                let _ = child.kill();
                let _ = child.wait();
                break Err("Media process could not be monitored".into());
            }
        }
    };
    let stdout = reader.join().map_err(|_| "Media reader failed")?;
    let _ = errors
        .join()
        .map_err(|_| "Media diagnostic reader failed")?;
    result?;
    if overflow.load(Ordering::Acquire) {
        return Err("Media tool exceeded its output limit".into());
    }
    Ok(BoundedOutput { stdout })
}
pub(crate) fn probe_owned(source: &Path, cancel: &Cancellation) -> Result<MediaInfo> {
    let output = run_bounded(
        Command::new(tool("ffprobe")?)
            .args([
                "-v",
                "error",
                "-protocol_whitelist",
                "file,pipe",
                "-show_entries",
                "format=duration:stream=codec_type,width,height,duration",
                "-of",
                "json",
            ])
            .arg(source),
        Duration::from_secs(30),
        cancel,
        2 * 1024 * 1024,
    )?;
    let value: serde_json::Value = serde_json::from_slice(&output.stdout)
        .map_err(|_| "Media probe returned invalid metadata")?;
    let streams = value["streams"]
        .as_array()
        .ok_or("Video streams are missing")?;
    let video = streams
        .iter()
        .find(|stream| stream["codec_type"] == "video")
        .ok_or("This file has no video track")?;
    let duration = video["duration"]
        .as_str()
        .or_else(|| value["format"]["duration"].as_str())
        .and_then(|value| value.parse::<f64>().ok())
        .ok_or("Video duration is unavailable")?;
    let millis = (duration * 1000.).round();
    let width = video["width"]
        .as_u64()
        .and_then(|width| u32::try_from(width).ok())
        .filter(|width| *width > 0)
        .ok_or("Video width is unavailable")?;
    let height = video["height"]
        .as_u64()
        .and_then(|height| u32::try_from(height).ok())
        .filter(|height| *height > 0)
        .ok_or("Video height is unavailable")?;
    if !duration.is_finite() || millis < 1. || millis >= u64::MAX as f64 {
        return Err("Video duration is invalid".into());
    }
    Ok(MediaInfo {
        duration_ms: millis as u64,
        width,
        height,
        has_audio: streams.iter().any(|stream| stream["codec_type"] == "audio"),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn subprocess_output_timeout_and_cancellation_are_bounded() {
        const CHILD: &str = "STORYFOLD_MEDIA_RUNNER_TEST_CHILD";
        if let Ok(mode) = std::env::var(CHILD) {
            if mode == "overflow" {
                for _ in 0..32 {
                    let _ = std::io::stdout().write_all(&[b'x'; 4096]);
                }
            } else {
                thread::sleep(Duration::from_secs(5));
            }
            return;
        }
        fn child(mode: &str) -> Command {
            let mut command = Command::new(std::env::current_exe().unwrap());
            command
                .args([
                    "--exact",
                    "media::tests::subprocess_output_timeout_and_cancellation_are_bounded",
                ])
                .env(CHILD, mode);
            command
        }
        let started = Instant::now();
        assert!(
            run_bounded(
                &mut child("overflow"),
                Duration::from_secs(2),
                &Cancellation::default(),
                1024
            )
            .is_err()
        );
        assert!(
            run_bounded(
                &mut child("sleep"),
                Duration::from_millis(50),
                &Cancellation::default(),
                1024
            )
            .is_err()
        );
        let cancellation = Cancellation::default();
        let delayed = cancellation.clone();
        let worker = thread::spawn(move || {
            thread::sleep(Duration::from_millis(50));
            delayed.cancel();
        });
        assert!(
            run_bounded(
                &mut child("sleep"),
                Duration::from_secs(2),
                &cancellation,
                1024
            )
            .is_err()
        );
        worker.join().unwrap();
        assert!(started.elapsed() < Duration::from_secs(3));
    }
    #[test]
    fn source_selection_rejects_playlists_devices_links_and_urls() {
        let temp = tempfile::tempdir().unwrap();
        let playlist = temp.path().join("remote.m3u8");
        fs::write(&playlist, b"https://example.invalid/video").unwrap();
        assert!(selected_file(&playlist).is_err());
        assert!(selected_file(Path::new("https://example.invalid/video.mp4")).is_err());
        assert!(selected_file(temp.path()).is_err());
        #[cfg(unix)]
        {
            use std::os::unix::fs::symlink;
            let video = temp.path().join("source.mp4");
            fs::write(&video, b"not decoded in a boundary test").unwrap();
            let linked = temp.path().join("linked.mp4");
            symlink(&video, &linked).unwrap();
            assert!(selected_file(&linked).is_err());
            fs::remove_file(&linked).unwrap();
            fs::hard_link(&video, &linked).unwrap();
            assert!(selected_file(&video).is_err());
        }
    }
}
