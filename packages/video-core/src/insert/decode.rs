//! Capacity-one raw-frame transport shared by insert preparation and export.
use crate::{OutputSize, Result, media::Cancellation, renderer::RgbaFrame};
use std::{
    io::Read,
    process::{Child, Command, Stdio},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver, RecvTimeoutError},
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

type Message = std::result::Result<(u64, RgbaFrame), String>;
pub(crate) struct FrameReader {
    child: Child,
    frames: Option<Receiver<Message>>,
    readers: Vec<JoinHandle<()>>,
    overflow: Arc<AtomicBool>,
    last: Option<(u64, RgbaFrame)>,
}
impl FrameReader {
    pub(crate) fn spawn(command: &mut Command, size: OutputSize, first_frame: u64) -> Result<Self> {
        super::dimensions(size)?;
        let count = usize::try_from(u64::from(size.width) * u64::from(size.height) * 4)
            .map_err(|_| "Decoded frame size overflows")?;
        if count > 16 * 1024 * 1024 {
            return Err("Video frame exceeds the16 MiB decode limit".into());
        }
        let mut child = command
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|_| "Video frame decoder could not start")?;
        let mut output = child.stdout.take().ok_or("Missing decoded video stream")?;
        let mut diagnostics = child
            .stderr
            .take()
            .ok_or("Missing decoder diagnostic stream")?;
        let (sender, receiver) = mpsc::sync_channel(1);
        let frames = thread::spawn(move || {
            let mut index = first_frame;
            loop {
                let mut pixels = vec![0_u8; count];
                if let Err(error) = output.read_exact(&mut pixels) {
                    if error.kind() != std::io::ErrorKind::UnexpectedEof {
                        let _ = sender.send(Err("Video frame stream could not be read".into()));
                    }
                    break;
                }
                if sender
                    .send(Ok((
                        index,
                        RgbaFrame {
                            width: size.width,
                            height: size.height,
                            pixels,
                        },
                    )))
                    .is_err()
                {
                    break;
                }
                let Some(next) = index.checked_add(1) else {
                    break;
                };
                index = next;
            }
        });
        let overflow = Arc::new(AtomicBool::new(false));
        let diagnostic_overflow = overflow.clone();
        let errors = thread::spawn(move || {
            let mut bytes = [0; 4096];
            let mut total = 0_usize;
            while let Ok(count) = diagnostics.read(&mut bytes) {
                if count == 0 {
                    break;
                }
                total = total.saturating_add(count);
                if total > 65536 {
                    diagnostic_overflow.store(true, Ordering::Release);
                }
            }
        });
        Ok(Self {
            child,
            frames: Some(receiver),
            readers: vec![frames, errors],
            overflow,
            last: None,
        })
    }
    pub(crate) fn last_index(&self) -> Option<u64> {
        self.last.as_ref().map(|(i, _)| *i)
    }
    pub(crate) fn frame_at(&mut self, target: u64, cancel: &Cancellation) -> Result<&RgbaFrame> {
        if self
            .last
            .as_ref()
            .is_some_and(|(index, _)| *index == target)
        {
            return Ok(&self.last.as_ref().ok_or("Missing decoded frame")?.1);
        }
        if self.last.as_ref().is_some_and(|(index, _)| *index > target) {
            return Err("Reopen the video decoder before seeking backwards".into());
        }
        let begin = Instant::now();
        loop {
            cancel.check()?;
            if self.overflow.load(Ordering::Acquire) {
                return Err("Video decoder exceeded its diagnostic limit".into());
            }
            if begin.elapsed() > Duration::from_secs(30) {
                return Err("Video frame decoder stopped making progress".into());
            }
            match self
                .frames
                .as_ref()
                .ok_or("Video decoder is closed")?
                .recv_timeout(Duration::from_millis(40))
            {
                Ok(Ok((index, frame))) => {
                    if index > target {
                        return Err("Video decoder skipped the requested frame".into());
                    }
                    self.last = Some((index, frame));
                    if index == target {
                        return Ok(&self.last.as_ref().ok_or("Missing decoded frame")?.1);
                    }
                }
                Ok(Err(error)) => return Err(error.into()),
                Err(RecvTimeoutError::Disconnected) => {
                    return Err(
                        "Video ended before the requested frame; shorten this insert".into(),
                    );
                }
                Err(RecvTimeoutError::Timeout) => {
                    if self
                        .child
                        .try_wait()?
                        .is_some_and(|status| !status.success())
                    {
                        return Err("Video decoder failed; no stale frame was substituted".into());
                    }
                }
            }
        }
    }
}
impl Drop for FrameReader {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        // Drop the receiver BEFORE joining: a producer can be blocked sending
        // its single pending frame even after the decoder has exited.
        self.frames.take();
        for reader in self.readers.drain(..) {
            let _ = reader.join();
        }
    }
}
