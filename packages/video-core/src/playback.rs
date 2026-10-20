//! Explicit playback of a prepared local sequence. Reuse the system's FFmpeg
//! player for audio, seeking and fullscreen instead of another decoder stack.
use crate::{ProjectStore, Result, media};
use catalyst_core::local::{self, FileMode, PathPolicy};
use std::process::{Child, Command, Stdio};

pub struct SequencePlayer {
    child: Child,
}
impl SequencePlayer {
    /// Call from a worker. The only accepted input is an existing confined MP4
    /// under this project's exports directory, never an arbitrary path or URL.
    pub fn start(store: &ProjectStore, relative: &str, muted: bool) -> Result<Self> {
        let path = export_path(store, relative)?;
        let mut command = Command::new(media::tool("ffplay")?);
        command.args([
            "-hide_banner",
            "-loglevel",
            "error",
            "-autoexit",
            "-window_title",
            "Storyfold · Sequence preview",
            "-x",
            "960",
            "-y",
            "540",
            "-seek_interval",
            "1",
            "-protocol_whitelist",
            "file",
        ]);
        if muted {
            command.arg("-an");
        }
        let child=command.arg(path).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).spawn().map_err(|_|"The native sequence player could not start. Install system FFmpeg with ffplay.")?;
        Ok(Self { child })
    }
    /// Nonblocking status; Some(false) is surfaced as a playback failure.
    pub fn finished(&mut self) -> Result<Option<bool>> {
        Ok(self.child.try_wait()?.map(|status| status.success()))
    }
}
impl Drop for SequencePlayer {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}
fn export_path(store: &ProjectStore, relative: &str) -> Result<std::path::PathBuf> {
    if !relative.starts_with("exports/") || !relative.ends_with(".mp4") {
        return Err("Choose a prepared MP4 sequence from this project's exports".into());
    }
    local::confined_file(
        store.root(),
        relative,
        PathPolicy::LocalProject,
        FileMode::Existing,
    )
}
/// Open only the current project's exports directory. Slow OS integration stays
/// on a caller-owned worker; failures never affect project/media data.
pub fn open_export_folder(store: &ProjectStore) -> Result<()> {
    let directory = local::existing_directory(store.root(), "exports")?;
    #[cfg(target_os = "linux")]
    let executable = "/usr/bin/xdg-open";
    #[cfg(target_os = "windows")]
    let executable = "explorer.exe";
    #[cfg(target_os = "macos")]
    let executable = "/usr/bin/open";
    #[cfg(not(any(target_os = "linux", target_os = "windows", target_os = "macos")))]
    return Err("Open the project's exports folder using your file manager".into());
    #[cfg(any(target_os = "linux", target_os = "windows", target_os = "macos"))]
    {
        let mut child=Command::new(executable).arg(directory).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).spawn().map_err(|_|"The export folder could not open. Open the project's exports folder in your file manager.")?;
        let start = std::time::Instant::now();
        loop {
            if let Some(status) = child.try_wait()? {
                return if status.success() {
                    Ok(())
                } else {
                    Err("The file manager could not open the exports folder".into())
                };
            }
            if start.elapsed() > std::time::Duration::from_secs(5) {
                let _ = child.kill();
                let _ = child.wait();
                return Err(
                    "The file manager did not respond; your exports are still saved".into(),
                );
            }
            std::thread::sleep(std::time::Duration::from_millis(25));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn preview_rejects_outside_paths_urls_missing_files_and_links_before_spawning() {
        let temp = tempfile::tempdir().unwrap();
        let (store, _) =
            ProjectStore::create(temp.path().join("project"), "Playback check").unwrap();
        for path in [
            "../outside.mp4",
            "media/take.mp4",
            "https://example.invalid/video.mp4",
            "exports/missing.mp4",
            "exports/../../outside.mp4",
        ] {
            assert!(SequencePlayer::start(&store, path, true).is_err());
        }
        #[cfg(unix)]
        {
            let outside = temp.path().join("outside.mp4");
            std::fs::write(&outside, b"no playback attempted").unwrap();
            let linked = store.root().join("exports/linked.mp4");
            std::os::unix::fs::symlink(&outside, &linked).unwrap();
            assert!(SequencePlayer::start(&store, "exports/linked.mp4", true).is_err());
            std::fs::remove_file(&linked).unwrap();
            std::fs::hard_link(&outside, &linked).unwrap();
            assert!(SequencePlayer::start(&store, "exports/linked.mp4", true).is_err());
        }
    }
}
