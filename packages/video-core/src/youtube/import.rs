//! Explicit public URL clip import. The Linux downloader process group and its
//! FFmpeg children share one public-only proxy and bounded private output.
use crate::{
    ProjectStore, Result,
    media::{self, Cancellation, ImportedMedia},
};
use std::time::Duration;
use url::Url;

#[derive(Clone, Debug)]
pub struct YoutubeImportRequest {
    pub url: String,
    pub start_ms: u64,
    pub end_ms: u64,
}
#[derive(Clone, Copy, Debug)]
pub struct ImportLimits {
    /// Visible operation budget, not an estimated percentage of a video.
    pub max_bytes: u64,
    pub timeout: Duration,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum YoutubeImportPhase {
    Downloading,
    Importing,
    Complete,
}
#[derive(Clone, Copy, Debug)]
pub struct YoutubeImportProgress {
    pub phase: YoutubeImportPhase,
    /// Actual current private output bytes; may change during a merge/rename.
    pub written_bytes: u64,
    pub byte_limit: u64,
}
impl YoutubeImportRequest {
    pub fn canonical_url(&self) -> Result<String> {
        if self.end_ms <= self.start_ms || self.end_ms > i64::MAX as u64 / 90 {
            return Err(
                "Choose an end time after the start, within the media timestamp range".into(),
            );
        }
        if self.url.len() > 2048 {
            return Err("YouTube URL is too long".into());
        }
        let u = Url::parse(&self.url).map_err(|_| "Paste a direct HTTPS YouTube video URL")?;
        let authority = self
            .url
            .split_once("://")
            .map(|(_, rest)| rest.split(['/', '?', '#']).next().unwrap_or(""))
            .unwrap_or("");
        if u.scheme() != "https"
            || authority.contains(':')
            || u.port().is_some()
            || !u.username().is_empty()
            || u.password().is_some()
            || u.fragment().is_some()
        {
            return Err(
                "Use a direct HTTPS YouTube video URL without credentials, ports or fragments"
                    .into(),
            );
        }
        let mut video = None;
        let mut names = std::collections::HashSet::new();
        for (name, value) in u.query_pairs() {
            if !names.insert(name.to_string())
                || matches!(name.as_ref(), "list" | "index" | "playlist")
            {
                return Err("Playlists are not supported; paste one video's direct URL".into());
            }
            match name.as_ref() {
                "v" => video = Some(value.to_string()),
                "t" | "start" | "end" | "si" | "feature" => {}
                _ => {
                    return Err(
                        "Use a direct YouTube video link, without redirect or extra query fields"
                            .into(),
                    );
                }
            }
        }
        let id = match u.host_str() {
            Some("youtu.be") if video.is_none() => u.path().strip_prefix('/').map(str::to_owned),
            Some("youtube.com" | "www.youtube.com" | "m.youtube.com") => {
                if u.path() == "/watch" {
                    video
                } else if video.is_none() {
                    ["/shorts/", "/live/", "/embed/"]
                        .iter()
                        .find_map(|prefix| u.path().strip_prefix(prefix).map(str::to_owned))
                } else {
                    None
                }
            }
            _ => None,
        }
        .filter(|id| {
            id.len() == 11
                && id
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
        })
        .ok_or("Use a direct YouTube watch, Shorts, live-video or youtu.be link")?;
        Ok(format!("https://www.youtube.com/watch?v={id}"))
    }
}
impl ImportLimits {
    pub fn validate(self) -> Result<()> {
        if self.max_bytes == 0
            || self.max_bytes > 4 * 1024 * 1024 * 1024
            || self.timeout.is_zero()
            || self.timeout > Duration::from_secs(1800)
        {
            return Err("Set an explicit download budget of at most4GiB and30minutes".into());
        }
        Ok(())
    }
}
pub fn importer_available() -> bool {
    #[cfg(target_os = "linux")]
    {
        importer_tool().is_ok()
    }
    #[cfg(not(target_os = "linux"))]
    {
        false
    }
}
/// Prefer the app-owned, checksum-verified packaged tool. An invalid existing
/// bundle is an error, never a reason to execute a different system program.
#[cfg(target_os = "linux")]
fn importer_tool() -> Result<std::path::PathBuf> {
    if let Ok(executable) = std::env::current_exe()
        && let Some(prefix) = executable.parent().and_then(std::path::Path::parent)
        && let Some(tool) = bundled_importer(prefix)?
    {
        return Ok(tool);
    }
    media::tool("yt-dlp").map_err(|_| {
        "The packaged yt-dlp tool is missing. Reinstall Record studio or install a compatible system yt-dlp".into()
    })
}
/// The installer verifies the pinned checksum before publishing the tool. This
/// resolver reuses Catalyst confinement and rejects linked/non-executable files.
/// Taking a prefix explicitly lets local fixtures exercise packaged resolution.
#[cfg(target_os = "linux")]
fn bundled_importer(prefix: &std::path::Path) -> Result<Option<std::path::PathBuf>> {
    use catalyst_core::local::{self, FileMode, PathPolicy};
    use std::os::unix::fs::PermissionsExt;
    let root = prefix.canonicalize()?;
    let tool = match local::confined_file(
        &root,
        "share/storyfold-record/tools/yt-dlp",
        PathPolicy::LocalProject,
        FileMode::Existing,
    ) {
        Ok(tool) => tool,
        Err(error)
            if error
                .downcast_ref::<std::io::Error>()
                .is_some_and(|error| error.kind() == std::io::ErrorKind::NotFound) =>
        {
            return Ok(None);
        }
        Err(error) => return Err(error),
    };
    let metadata = std::fs::metadata(&tool)?;
    if metadata.len() == 0 || metadata.permissions().mode() & 0o111 == 0 {
        return Err(
            "The packaged yt-dlp tool is empty or not executable. Reinstall Record studio".into(),
        );
    }
    Ok(Some(tool))
}
pub fn import_clip(
    store: &ProjectStore,
    request: &YoutubeImportRequest,
    limits: ImportLimits,
    cancel: &Cancellation,
    progress: impl FnMut(YoutubeImportProgress),
) -> Result<ImportedMedia> {
    request.canonical_url()?;
    limits.validate()?;
    cancel.check()?;
    #[cfg(target_os = "linux")]
    {
        let tool = importer_tool()?;
        linux::import_with_tool(store, request, limits, cancel, progress, &tool)
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = (store, request, limits, cancel, progress);
        Err("YouTube clip import is available in the native Linux build; this platform has no safe downloader process supervisor yet".into())
    }
}
#[cfg(all(test, target_os = "linux", feature = "capture-test"))]
pub(super) fn import_test_tool(
    store: &ProjectStore,
    request: &YoutubeImportRequest,
    limits: ImportLimits,
    cancel: &Cancellation,
    progress: impl FnMut(YoutubeImportProgress),
    tool: &std::path::Path,
) -> Result<ImportedMedia> {
    linux::import_with_tool(store, request, limits, cancel, progress, tool)
}
#[cfg(target_os = "linux")]
mod linux {
    use super::*;
    use crate::ai::PublicProxy;
    use catalyst_core::local::{self, PrivateNamespace};
    use std::{
        fs,
        io::Read,
        path::Path,
        process::{Child, Command, Stdio},
        sync::{
            Arc,
            atomic::{AtomicBool, Ordering},
        },
        thread::{self, JoinHandle},
        time::Instant,
    };
    struct Running {
        child: Child,
        readers: Vec<JoinHandle<()>>,
    }
    impl Drop for Running {
        fn drop(&mut self) {
            // SAFETY: this child was put in its own process group immediately
            // at spawn; negative PID kills only that downloader/FFmpeg group.
            unsafe {
                libc::kill(-(self.child.id() as i32), libc::SIGKILL);
            }
            let _ = self.child.kill();
            let _ = self.child.wait();
            for reader in self.readers.drain(..) {
                let _ = reader.join();
            }
        }
    }
    impl Running {
        fn finished(&self) -> Result<Option<bool>> {
            let mut info = std::mem::MaybeUninit::<libc::siginfo_t>::zeroed();
            // SAFETY: this live Child owns the PID and no other thread reaps
            // it. WNOWAIT leaves an exited child unreaped, reserving its PID
            // until Drop kills its remaining group then waits. This prevents
            // signaling an unrelated reused PID after a successful exit.
            let status = unsafe {
                libc::waitid(
                    libc::P_PID,
                    self.child.id(),
                    info.as_mut_ptr(),
                    libc::WEXITED | libc::WNOHANG | libc::WNOWAIT,
                )
            };
            if status != 0 {
                return Err(std::io::Error::last_os_error().into());
            }
            // SAFETY: successful waitid initializes info, with si_pid=0 when
            // no child exit is available under WNOHANG.
            let info = unsafe { info.assume_init() };
            if unsafe { info.si_pid() } == 0 {
                return Ok(None);
            }
            Ok(Some(
                info.si_code == libc::CLD_EXITED && unsafe { info.si_status() } == 0,
            ))
        }
    }
    fn seconds(ms: u64) -> String {
        format!("{}.{:03}", ms / 1000, ms % 1000)
    }
    fn command(
        tool: &Path,
        ffmpeg: &Path,
        work: &Path,
        url: &str,
        request: &YoutubeImportRequest,
        limits: ImportLimits,
        proxy: std::net::SocketAddr,
    ) -> Command {
        use std::os::unix::process::CommandExt;
        let proxy_url = format!("http://{proxy}");
        let mut c = Command::new(tool);
        c.process_group(0).current_dir(work).env_clear()
            .env("PATH","/usr/local/bin:/usr/bin:/bin").env("LANG","C.UTF-8")
            .env("TMPDIR",work).env("PYTHONNOUSERSITE","1")
            .env("HTTP_PROXY",&proxy_url).env("http_proxy",&proxy_url)
            .env("HTTPS_PROXY",&proxy_url).env("https_proxy",&proxy_url)
            .env("NO_PROXY","").env("no_proxy","")
            .args(["--ignore-config","--no-plugin-dirs","--no-remote-components","--no-cache-dir","--no-cookies","--no-playlist","--no-mark-watched","--no-write-info-json","--no-write-thumbnail","--no-write-subs","--no-write-comments","--no-write-playlist-metafiles","--no-simulate","--quiet","--newline","--fixup","never","--socket-timeout","15","--retries","2","--fragment-retries","2","--proxy"])
            .arg(&proxy_url).arg("--ffmpeg-location").arg(ffmpeg)
            .args(["--downloader","ffmpeg","--format","bv[ext=mp4][protocol=https][width<=1920][height<=1920]+ba[ext=m4a][protocol=https]/b[ext=mp4][protocol=https][width<=1920][height<=1920]","--merge-output-format","mp4","--max-filesize"])
            .arg(limits.max_bytes.to_string()).arg("--download-sections")
            .arg(format!("*{}-{}",seconds(request.start_ms),seconds(request.end_ms)))
            .arg("--force-keyframes-at-cuts").arg("--downloader-args")
            .arg(format!("ffmpeg_i:-http_proxy {proxy_url} -protocol_whitelist https,tls,tcp,httpproxy -format_whitelist mov -rw_timeout 15000000"))
            .args(["--postprocessor-args","Merger+ffmpeg_i:-protocol_whitelist file,pipe -format_whitelist mov","--paths"])
            .arg(work).args(["--output","clip.%(ext)s","--"]).arg(url);
        c
    }
    fn output_bytes(work: &Path) -> Result<u64> {
        let mut count = 0;
        let mut total = 0_u64;
        for entry in fs::read_dir(work)? {
            count += 1;
            if count > 16 {
                return Err("Downloader created too many output files".into());
            }
            let path = entry?.path();
            local::reject_file_links(&path)?;
            let m = fs::symlink_metadata(path)?;
            if !m.is_file() {
                return Err("Downloader created an unexpected output directory".into());
            }
            total = total
                .checked_add(m.len())
                .ok_or("Download output size overflows")?;
        }
        Ok(total)
    }
    pub(super) fn import_with_tool(
        store: &ProjectStore,
        request: &YoutubeImportRequest,
        limits: ImportLimits,
        cancel: &Cancellation,
        mut progress: impl FnMut(YoutubeImportProgress),
        tool: &Path,
    ) -> Result<ImportedMedia> {
        let url = request.canonical_url()?;
        limits.validate()?;
        cancel.check()?;
        let private = local::private_dir(store.root(), PrivateNamespace::Sidebar, true)?;
        let work = tempfile::Builder::new()
            .prefix("youtube-import-")
            .tempdir_in(private)?;
        let ffmpeg = media::tool("ffmpeg")?;
        let proxy = PublicProxy::start_with_limits(limits.timeout, limits.max_bytes, 256)?;
        let mut c = command(
            tool,
            &ffmpeg,
            work.path(),
            &url,
            request,
            limits,
            proxy.address(),
        );
        let mut child = c
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|_| "Installed yt-dlp could not start")?;
        let overflow = Arc::new(AtomicBool::new(false));
        let mut readers = Vec::new();
        for stream in [
            child
                .stdout
                .take()
                .map(|s| Box::new(s) as Box<dyn Read + Send>),
            child
                .stderr
                .take()
                .map(|s| Box::new(s) as Box<dyn Read + Send>),
        ] {
            let mut stream = stream.ok_or("Missing downloader output stream")?;
            let overflow = overflow.clone();
            readers.push(thread::spawn(move || {
                let mut total = 0_usize;
                let mut buffer = [0_u8; 4096];
                while let Ok(n) = stream.read(&mut buffer) {
                    if n == 0 {
                        break;
                    }
                    total = total.saturating_add(n);
                    if total > 131072 {
                        overflow.store(true, Ordering::Release);
                    }
                }
            }));
        }
        let running = Running { child, readers };
        let start = Instant::now();
        let mut last_bytes = None;
        loop {
            cancel.check()?;
            let bytes = output_bytes(work.path())?;
            if bytes > limits.max_bytes {
                return Err("Clip exceeded the selected byte budget; no media was imported".into());
            }
            if start.elapsed() > limits.timeout {
                return Err("Clip exceeded the selected time budget; no media was imported".into());
            }
            if overflow.load(Ordering::Acquire) {
                return Err("Downloader exceeded its bounded output limit".into());
            }
            if last_bytes != Some(bytes) {
                progress(YoutubeImportProgress {
                    phase: YoutubeImportPhase::Downloading,
                    written_bytes: bytes,
                    byte_limit: limits.max_bytes,
                });
                last_bytes = Some(bytes);
            }
            match running.finished()? {
                Some(true)=>break,
                Some(_)=>return Err("Public clip download failed; check the installed yt-dlp version and the video's availability".into()),
                None=>thread::sleep(Duration::from_millis(25)),
            }
        }
        // Reap/stop all descendants before inspecting or publishing their output.
        drop(running);
        drop(proxy);
        cancel.check()?;
        let selected = work.path().join("clip.mp4");
        local::reject_file_links(&selected)?;
        let bytes = output_bytes(work.path())?;
        if bytes > limits.max_bytes {
            return Err("Clip exceeded the selected byte budget".into());
        }
        progress(YoutubeImportProgress {
            phase: YoutubeImportPhase::Importing,
            written_bytes: bytes,
            byte_limit: limits.max_bytes,
        });
        let media = media::import_media(store, &selected, cancel)?;
        progress(YoutubeImportProgress {
            phase: YoutubeImportPhase::Complete,
            written_bytes: bytes,
            byte_limit: limits.max_bytes,
        });
        Ok(media)
    }
    #[cfg(test)]
    mod tests {
        use super::*;
        #[test]
        fn fixed_downloader_arguments_disable_context_and_force_proxy_on_every_input() {
            let request = YoutubeImportRequest {
                url: "https://youtu.be/aB_cD-eF012".into(),
                start_ms: 1234,
                end_ms: 5678,
            };
            let c = command(
                Path::new("/usr/bin/yt-dlp"),
                Path::new("/usr/bin/ffmpeg"),
                Path::new("/tmp/owned output"),
                &request.canonical_url().unwrap(),
                &request,
                ImportLimits {
                    max_bytes: 1000000,
                    timeout: Duration::from_secs(60),
                },
                "127.0.0.1:23456".parse().unwrap(),
            );
            let args: Vec<_> = c.get_args().map(|s| s.to_str().unwrap()).collect();
            for flag in [
                "--ignore-config",
                "--no-plugin-dirs",
                "--no-remote-components",
                "--no-cookies",
                "--no-playlist",
                "--force-keyframes-at-cuts",
            ] {
                assert!(args.contains(&flag));
            }
            assert!(args.contains(&"*1.234-5.678"));
            assert!(args.contains(&"clip.%(ext)s"));
            assert!(args.iter().any(|v|v.starts_with("ffmpeg_i:-http_proxy http://127.0.0.1:23456 -protocol_whitelist https,tls,tcp,httpproxy -format_whitelist mov")));
            assert_eq!(
                args.last(),
                Some(&"https://www.youtube.com/watch?v=aB_cD-eF012")
            );
            assert!(
                c.get_envs().any(
                    |(key, value)| key == "NO_PROXY" && value == Some(std::ffi::OsStr::new(""))
                )
            );
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn direct_video_forms_canonicalize_but_playlists_redirects_and_bad_ranges_refuse() {
        for url in [
            "https://youtu.be/aB_cD-eF012",
            "https://www.youtube.com/watch?v=aB_cD-eF012&t=3",
            "https://youtube.com/shorts/aB_cD-eF012",
            "https://m.youtube.com/live/aB_cD-eF012",
        ] {
            let request = YoutubeImportRequest {
                url: url.into(),
                start_ms: 0,
                end_ms: 1000,
            };
            assert_eq!(
                request.canonical_url().unwrap(),
                "https://www.youtube.com/watch?v=aB_cD-eF012"
            );
        }
        for url in [
            "http://youtu.be/aB_cD-eF012",
            "https://youtube.com/playlist?list=aB_cD-eF012",
            "https://youtube.com/watch?v=aB_cD-eF012&list=other",
            "https://youtube.com/redirect?q=https://example.org",
            "https://youtube.com.evil.example/watch?v=aB_cD-eF012",
            "https://user@youtu.be/aB_cD-eF012",
            "https://youtu.be:443/aB_cD-eF012",
            "https://youtu.be/aB_cD-eF012/extra",
            "https://youtube.com/watch?v=aB_cD-eF012&v=other",
        ] {
            assert!(
                YoutubeImportRequest {
                    url: url.into(),
                    start_ms: 0,
                    end_ms: 1000
                }
                .canonical_url()
                .is_err(),
                "{url}"
            );
        }
        assert!(
            YoutubeImportRequest {
                url: "https://youtu.be/aB_cD-eF012".into(),
                start_ms: 1000,
                end_ms: 1000
            }
            .canonical_url()
            .is_err()
        );
        assert!(
            ImportLimits {
                max_bytes: 0,
                timeout: Duration::from_secs(1)
            }
            .validate()
            .is_err()
        );
        assert!(
            ImportLimits {
                max_bytes: 1,
                timeout: Duration::from_secs(1801)
            }
            .validate()
            .is_err()
        );
    }
}

#[cfg(all(test, target_os = "linux"))]
mod bundled_tests {
    use super::bundled_importer;
    use std::{
        fs,
        os::unix::fs::{PermissionsExt, symlink},
    };

    #[test]
    fn packaged_tool_resolves_under_prefix_and_absence_allows_system_lookup() {
        let temp = tempfile::tempdir().unwrap();
        let prefix = temp.path();
        assert!(bundled_importer(prefix).unwrap().is_none());
        let tools = prefix.join("share/storyfold-record/tools");
        fs::create_dir_all(&tools).unwrap();
        assert!(bundled_importer(prefix).unwrap().is_none());
        let tool = tools.join("yt-dlp");
        // Resolver fixture only: no subprocess or downloaded code is executed.
        fs::write(&tool, b"synthetic packaged executable fixture").unwrap();
        fs::set_permissions(&tool, fs::Permissions::from_mode(0o755)).unwrap();
        assert_eq!(bundled_importer(prefix).unwrap(), Some(tool));
    }

    #[test]
    fn invalid_existing_bundle_is_an_error_instead_of_silent_system_fallback() {
        let temp = tempfile::tempdir().unwrap();
        let prefix = temp.path();
        let tools = prefix.join("share/storyfold-record/tools");
        fs::create_dir_all(&tools).unwrap();
        let tool = tools.join("yt-dlp");
        fs::write(&tool, b"fixture").unwrap();
        fs::set_permissions(&tool, fs::Permissions::from_mode(0o644)).unwrap();
        assert!(bundled_importer(prefix).is_err());
        fs::set_permissions(&tool, fs::Permissions::from_mode(0o755)).unwrap();
        fs::write(&tool, b"").unwrap();
        assert!(bundled_importer(prefix).is_err());
        fs::remove_file(&tool).unwrap();
        let outside = temp.path().join("outside");
        fs::write(&outside, b"fixture").unwrap();
        fs::set_permissions(&outside, fs::Permissions::from_mode(0o755)).unwrap();
        symlink(&outside, &tool).unwrap();
        assert!(bundled_importer(prefix).is_err());
        fs::remove_file(&tool).unwrap();
        fs::hard_link(&outside, &tool).unwrap();
        assert!(bundled_importer(prefix).is_err());
        fs::remove_file(&tool).unwrap();
        fs::remove_dir(&tools).unwrap();
        symlink(temp.path(), &tools).unwrap();
        assert!(bundled_importer(prefix).is_err());
    }
}
