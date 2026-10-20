use super::search;
use crate::{
    ProjectStore, Result,
    media::{self, Cancellation},
    renderer::RgbaFrame,
};
use catalyst_core::local::{self, PrivateNamespace};
use std::{
    fs,
    io::{Cursor, Read},
    path::Path,
    process::{Command, Stdio},
    thread,
    time::{Duration, Instant},
};

pub struct Evidence {
    pub(super) source_url: String,
    pub(super) text: String,
    pub(super) png: Vec<u8>,
    frame: RgbaFrame,
    directory: tempfile::TempDir,
}
impl Evidence {
    pub fn source_url(&self) -> &str {
        &self.source_url
    }
    pub fn text(&self) -> &str {
        &self.text
    }
    pub fn frame(&self) -> &RgbaFrame {
        &self.frame
    }
    pub fn from_file(store: &ProjectStore, path: &Path, cancel: &Cancellation) -> Result<Self> {
        cancel.check()?;
        local::reject_file_links(path)?;
        let before = fs::metadata(path)?;
        if !before.is_file() || before.len() > 16 * 1024 * 1024 {
            return Err("Choose a screenshot under 16 MiB".into());
        }
        let mut bytes = Vec::new();
        fs::File::open(path)?
            .take(16 * 1024 * 1024 + 1)
            .read_to_end(&mut bytes)?;
        local::reject_file_links(path)?;
        let after = fs::metadata(path)?;
        if before.len() != after.len()
            || before.modified()? != after.modified()?
            || bytes.len() as u64 != after.len()
        {
            return Err("Screenshot changed while reading; retry".into());
        }
        let directory = workspace(store)?;
        Self::decode(bytes, String::new(), directory, cancel)
    }
    pub fn capture(store: &ProjectStore, url: &str, cancel: &Cancellation) -> Result<Self> {
        cancel.check()?;
        search::public_url(url)?;
        cancel.check()?;
        let directory = workspace(store)?;
        let path = directory.path().join("capture.png");
        let profile = directory.path().join("browser");
        fs::create_dir(&profile)?;
        // An isolated fresh profile carries no existing login/cookies. All page
        // connections use the validating public-only HTTPS CONNECT proxy.
        let proxy = search::PublicProxy::start()?;
        let mut command = Command::new(chrome()?);
        command
            .args([
                "--headless=new",
                "--no-first-run",
                "--no-default-browser-check",
                "--disable-background-networking",
                "--disable-sync",
                "--disable-extensions",
                "--disable-component-update",
                "--disable-domain-reliability",
                "--disable-quic",
                "--force-webrtc-ip-handling-policy=disable_non_proxied_udp",
                "--hide-scrollbars",
                "--window-size=1440,900",
                "--timeout=15000",
            ])
            .arg(format!("--user-data-dir={}", profile.display()))
            .arg(format!("--proxy-server=http://{}", proxy.address()))
            .arg("--proxy-bypass-list=<-loopback>")
            .arg(format!("--screenshot={}", path.display()))
            .arg(url)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        #[cfg(unix)]
        {
            use std::os::unix::process::CommandExt;
            command.process_group(0);
        }
        let mut child = command
            .spawn()
            .map_err(|_| "Could not start installed Chromium/Chrome")?;
        let started = Instant::now();
        let result: Result<()> = loop {
            if cancel.is_cancelled() {
                break Err("Screenshot canceled".into());
            }
            match child.try_wait() {
                Ok(Some(status)) => {
                    break if status.success() {
                        Ok(())
                    } else {
                        Err("Chromium could not capture this public page; try another result or drop a screenshot".into())
                    };
                }
                Ok(None) => {}
                Err(e) => break Err(e.into()),
            }
            if started.elapsed() > Duration::from_secs(30) {
                break Err(
                    "Page screenshot timed out; try another result or drop a screenshot".into(),
                );
            }
            thread::sleep(Duration::from_millis(25));
        };
        #[cfg(target_os = "linux")]
        unsafe {
            libc::kill(-(child.id() as i32), libc::SIGKILL);
        }
        let _ = child.kill();
        let _ = child.wait();
        drop(proxy);
        result?;
        local::reject_file_links(&path)?;
        let meta = fs::metadata(&path)?;
        if !meta.is_file() || meta.len() > 16 * 1024 * 1024 {
            return Err("Page screenshot exceeds its size limit".into());
        }
        Self::decode(fs::read(&path)?, url.to_owned(), directory, cancel)
    }
    fn decode(
        bytes: Vec<u8>,
        source_url: String,
        directory: tempfile::TempDir,
        cancel: &Cancellation,
    ) -> Result<Self> {
        cancel.check()?;
        if bytes.len() > 16 * 1024 * 1024 {
            return Err("Screenshot exceeds 16 MiB".into());
        }
        let format = image::guess_format(&bytes)?;
        if !matches!(
            format,
            image::ImageFormat::Png | image::ImageFormat::Jpeg | image::ImageFormat::WebP
        ) {
            return Err("Choose a PNG, JPEG or WebP screenshot".into());
        }
        let mut reader = image::ImageReader::with_format(Cursor::new(bytes), format);
        let mut limits = image::Limits::default();
        limits.max_image_width = Some(4096);
        limits.max_image_height = Some(4096);
        limits.max_alloc = Some(64 * 1024 * 1024);
        reader.limits(limits);
        let image = reader.decode()?;
        let frame = RgbaFrame {
            width: image.width(),
            height: image.height(),
            pixels: image.to_rgba8().into_raw(),
        };
        let mut encoded = Cursor::new(Vec::new());
        image.write_to(&mut encoded, image::ImageFormat::Png)?;
        let png = encoded.into_inner();
        if png.len() > 16 * 1024 * 1024 {
            return Err("Normalized screenshot exceeds 16 MiB".into());
        }
        let path = directory.path().join("evidence.png");
        fs::write(&path, &png)?;
        let output =
            media::run_bounded(
                Command::new(media::tool("tesseract").map_err(
                    |_| "Tesseract OCR must be installed for screenshot text validation",
                )?)
                .arg(&path)
                .args(["stdout", "--psm", "11"]),
                Duration::from_secs(30),
                cancel,
                65536,
            )?;
        let text = String::from_utf8(output.stdout).map_err(|_| "OCR returned invalid text")?;
        if text.len() > 32000 {
            return Err("Screenshot has too much text; crop it to the needed passage".into());
        }
        catalyst_core::check_secrets(&text)?;
        cancel.check()?;
        Ok(Self {
            source_url,
            text,
            png,
            frame,
            directory,
        })
    }
    /// Explicit approval may retain the screenshot; before this point it stays
    /// only in a temporary ignored private directory and never in project JSON.
    pub fn approve_original(
        &self,
        store: &ProjectStore,
        title: &str,
        cancel: &Cancellation,
    ) -> Result<crate::OverlayContent> {
        let source = self.directory.path().join("evidence.png");
        local::reject_file_links(&source)?;
        if fs::metadata(&source)?.len() != self.png.len() as u64 || fs::read(&source)? != self.png {
            return Err("Screenshot changed after review; capture it again".into());
        }
        let path = media::import_image(store, &source, cancel)?;
        let published = local::confined_file(
            store.root(),
            &path,
            local::PathPolicy::LocalProject,
            local::FileMode::Existing,
        )?;
        if fs::metadata(&published)?.len() != self.png.len() as u64
            || fs::read(&published)? != self.png
        {
            return Err("Screenshot changed during approval; no card was approved".into());
        }
        let mut content = crate::OverlayContent::new(crate::OverlayKind::Photo, title);
        content.image = Some(path);
        if !self.source_url.is_empty() {
            content.blocks.push(crate::OverlayBlock {
                role: "source".into(),
                text: self.source_url.clone(),
            });
        }
        content.validate()?;
        Ok(content)
    }
}
fn workspace(store: &ProjectStore) -> Result<tempfile::TempDir> {
    Ok(tempfile::Builder::new()
        .prefix("ai-evidence-")
        .tempdir_in(local::private_dir(
            store.root(),
            PrivateNamespace::Sidebar,
            true,
        )?)?)
}
fn chrome() -> Result<std::path::PathBuf> {
    #[cfg(target_os = "linux")]
    for path in [
        "/usr/bin/google-chrome",
        "/usr/bin/chromium",
        "/usr/bin/chromium-browser",
    ] {
        if Path::new(path).is_file() {
            return Ok(path.into());
        }
    }
    Err(
        "Install Chromium or Google Chrome for page screenshots; dropping a screenshot still works"
            .into(),
    )
}
