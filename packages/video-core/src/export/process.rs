//! Bounded progress/cancel transport for fixed export commands, never a shell.
use crate::{Result, media::Cancellation};
use std::{
    io::{BufRead, BufReader, Read},
    process::{Child, ChildStdin, Command, Stdio},
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

pub(crate) type Writer = Box<dyn FnOnce(ChildStdin) -> Result<()> + Send + 'static>;
#[derive(Default)]
struct State {
    out_us: AtomicU64,
    serial: AtomicU64,
    overflow: AtomicBool,
}
struct Running {
    child: Child,
    readers: Vec<JoinHandle<()>>,
    writer: Option<JoinHandle<Result<()>>>,
}
impl Drop for Running {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        for reader in self.readers.drain(..) {
            let _ = reader.join();
        }
        if let Some(writer) = self.writer.take() {
            let _ = writer.join();
        }
    }
}
pub(crate) fn run(
    command: &mut Command,
    writer: Option<Writer>,
    heartbeat: Arc<AtomicU64>,
    output_activity: &std::path::Path,
    timeout: Duration,
    cancel: &Cancellation,
    mut emit: impl FnMut(u64),
) -> Result<()> {
    cancel.check()?;
    let state = Arc::new(State::default());
    let mut child = command
        .stdin(if writer.is_some() {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|_| "Export media tool could not start")?;
    let stdout = child
        .stdout
        .take()
        .ok_or("Missing export progress stream")?;
    let stderr = child
        .stderr
        .take()
        .ok_or("Missing export diagnostic stream")?;
    let input = child.stdin.take();
    let stdout_state = state.clone();
    let progress_reader = thread::spawn(move || read_progress(stdout, stdout_state));
    let stderr_state = state.clone();
    let error_reader = thread::spawn(move || {
        let mut reader = stderr;
        let mut bytes = [0; 4096];
        let mut total = 0_usize;
        while let Ok(count) = reader.read(&mut bytes) {
            if count == 0 {
                break;
            }
            total = total.saturating_add(count);
            if total > 65_536 {
                stderr_state.overflow.store(true, Ordering::Release);
            }
        }
    });
    let writer = match (writer, input) {
        (Some(write), Some(input)) => Some(thread::spawn(move || write(input))),
        (None, _) => None,
        _ => return Err("Missing export frame input".into()),
    };
    let mut running = Running {
        child,
        readers: vec![progress_reader, error_reader],
        writer,
    };
    let start = Instant::now();
    let mut last_activity = Instant::now();
    let mut last_out = 0;
    let mut last_heartbeat = 0;
    let mut last_serial = 0;
    let mut last_file_check = Instant::now();
    let mut file_stamp = None;
    let result: Result<()> = loop {
        if cancel.is_cancelled() {
            break Err("Export cancelled; source media and existing exports were retained".into());
        }
        if state.overflow.load(Ordering::Acquire) {
            break Err("Export tool exceeded its bounded progress or diagnostic limit".into());
        }
        if running.writer.as_ref().is_some_and(JoinHandle::is_finished) {
            let finished = running
                .writer
                .take()
                .ok_or("Export frame worker disappeared")?
                .join()
                .map_err(|_| "Export frame worker failed")?;
            if let Err(error) = finished {
                break Err(error);
            }
        }
        let out = state.out_us.load(Ordering::Acquire);
        let serial = state.serial.load(Ordering::Acquire);
        let wrote = heartbeat.load(Ordering::Acquire);
        if out > last_out || wrote > last_heartbeat {
            last_activity = Instant::now();
        }
        // MP4 fast-start can spend time moving existing bytes after the last
        // encoded frame. Observe real output writes for liveness only; never
        // turn elapsed time/file size into invented encoding percentages.
        if last_file_check.elapsed() >= Duration::from_secs(1) {
            if let Ok(meta) = std::fs::metadata(output_activity) {
                let stamp = (meta.len(), meta.modified().ok());
                if file_stamp.as_ref() != Some(&stamp) {
                    last_activity = Instant::now();
                    file_stamp = Some(stamp);
                }
            }
            last_file_check = Instant::now();
        }
        if serial != last_serial {
            emit(out / 1000);
            last_serial = serial;
        }
        last_out = out;
        last_heartbeat = wrote;
        if start.elapsed() > timeout || last_activity.elapsed() > Duration::from_secs(60) {
            break Err("Export stopped after the media tool stopped making progress; source files were retained".into());
        }
        match running.child.try_wait() {
            Ok(Some(status)) => {
                break if status.success() {
                    Ok(())
                } else {
                    Err(
                        "Export media tool failed; source media and existing exports were retained"
                            .into(),
                    )
                };
            }
            Ok(None) => thread::sleep(Duration::from_millis(15)),
            Err(_) => break Err("Export process could not be monitored".into()),
        }
    };
    if result.is_err() {
        let _ = running.child.kill();
        let _ = running.child.wait();
    }
    let writer_result = if let Some(writer) = running.writer.take() {
        writer.join().map_err(|_| "Export frame worker failed")?
    } else {
        Ok(())
    };
    for reader in running.readers.drain(..) {
        reader.join().map_err(|_| "Export progress reader failed")?;
    }
    result?;
    writer_result?;
    if state.overflow.load(Ordering::Acquire) {
        return Err("Export tool exceeded its output limits".into());
    }
    if state.serial.load(Ordering::Acquire) == 0 {
        return Err("Export tool did not report actual progress".into());
    }
    emit(state.out_us.load(Ordering::Acquire) / 1000);
    Ok(())
}
fn read_progress(stream: impl Read, state: Arc<State>) {
    let mut reader = BufReader::new(stream);
    let mut current = 0;
    loop {
        let mut line = Vec::new();
        let Ok(count) = reader.by_ref().take(1025).read_until(b'\n', &mut line) else {
            state.overflow.store(true, Ordering::Release);
            break;
        };
        if count == 0 {
            break;
        }
        if line.len() > 1024 || line.last() != Some(&b'\n') {
            state.overflow.store(true, Ordering::Release);
            break;
        }
        let Ok(line) = std::str::from_utf8(&line) else {
            state.overflow.store(true, Ordering::Release);
            break;
        };
        let Some((key, value)) = line.trim_end().split_once('=') else {
            state.overflow.store(true, Ordering::Release);
            break;
        };
        if key.len() > 80 || !key.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
            state.overflow.store(true, Ordering::Release);
            break;
        }
        if key == "out_time_us" {
            if value == "N/A" {
                continue;
            }
            if let Ok(us) = value.parse::<i64>() {
                current = current.max(us.max(0) as u64);
            } else {
                state.overflow.store(true, Ordering::Release);
                break;
            }
        } else if key == "progress" {
            if !matches!(value, "continue" | "end") {
                state.overflow.store(true, Ordering::Release);
                break;
            }
            state.out_us.store(current, Ordering::Release);
            state.serial.fetch_add(1, Ordering::Release);
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn ffmpeg_progress_is_bounded_and_never_moves_backwards() {
        let state = Arc::new(State::default());
        read_progress(&b"frame=1\nout_time_us=-20000\nprogress=continue\nout_time_us=500000\nprogress=continue\nout_time_us=400000\nprogress=end\n"[..],state.clone());
        assert_eq!(state.serial.load(Ordering::Acquire), 3);
        assert_eq!(state.out_us.load(Ordering::Acquire), 500000);
        assert!(!state.overflow.load(Ordering::Acquire));
        for invalid in [
            b"out_time_us=invalid\n".to_vec(),
            b"progress=bad\n".to_vec(),
            vec![b'x'; 1025],
            b"out_time_us=10".to_vec(),
        ] {
            let state = Arc::new(State::default());
            read_progress(invalid.as_slice(), state.clone());
            assert!(state.overflow.load(Ordering::Acquire));
        }
    }
}
