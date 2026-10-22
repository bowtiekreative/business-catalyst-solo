use super::{
    buffer::{ActiveRange, CutPlan, Manifest, TakeRange, segment_name},
    gesture::{Action, Gesture, GestureState, Stamp},
    *,
};
use crate::encoding::VideoEncoder;
use crate::media::{self, Cancellation, ImportedMedia, run_bounded};
use catalyst_core::local::{self, FileMode, PathPolicy, PendingFile, PrivateNamespace};
use ffmpeg_sidecar::{child::FfmpegChild, command::FfmpegCommand};
use std::{
    collections::{HashSet, VecDeque},
    fs,
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex, OnceLock,
        atomic::{AtomicBool, AtomicU64, Ordering},
        mpsc::{self, SyncSender, TryRecvError},
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

struct Shared {
    status: Mutex<CaptureStatus>,
    preview: Mutex<Option<(Arc<PreviewFrame>, Instant)>>,
    preview_waker: OnceLock<Arc<dyn Fn() + Send + Sync>>,
    events: Mutex<VecDeque<CaptureEvent>>,
    frame_count: AtomicU64,
    stop: AtomicBool,
    event_overflow: AtomicBool,
}
enum Control {
    Press(Stamp),
    Release(Stamp),
    StopTake(Stamp),
    CancelTake,
    Mark(Stamp, SlotKey, OverlayContent, u64),
    Stop,
    #[cfg(all(test, feature = "capture-test"))]
    InterruptGenerated,
}
pub struct CaptureSession {
    shared: Arc<Shared>,
    commands: SyncSender<Control>,
    epoch: Instant,
    cancel: Cancellation,
    worker: Option<JoinHandle<()>>,
}
impl CaptureSession {
    pub fn start(store: &ProjectStore, settings: CaptureSettings) -> Result<Self> {
        #[cfg(target_os = "linux")]
        {
            settings.validate(false)?;
            let devices = enumerate_devices()?;
            let camera = devices
                .cameras
                .iter()
                .find(|camera| camera.id == settings.camera_id)
                .ok_or("Selected camera is unavailable; refresh devices")?;
            if !camera.modes.contains(&settings.mode) {
                return Err("Selected camera mode is no longer available".into());
            }
            if settings
                .microphone_id
                .as_ref()
                .is_some_and(|id| !devices.microphones.iter().any(|mic| &mic.id == id))
            {
                return Err(
                    "Selected microphone is unavailable; refresh devices or turn it off".into(),
                );
            }
            Self::begin(store, settings, false)
        }
        #[cfg(not(target_os = "linux"))]
        {
            let _ = (store, settings);
            Err("Camera capture is supported on Linux; no device was opened".into())
        }
    }
    #[cfg(all(target_os = "linux", feature = "capture-test"))]
    pub fn start_generated(store: &ProjectStore, settings: CaptureSettings) -> Result<Self> {
        if std::env::var("STORYFOLD_CAPTURE_TEST").as_deref() != Ok("1") {
            return Err(
                "Generated capture requires the explicit STORYFOLD_CAPTURE_TEST=1 test environment"
                    .into(),
            );
        }
        settings.validate(true)?;
        Self::begin(store, settings, true)
    }
    fn begin(store: &ProjectStore, settings: CaptureSettings, generated: bool) -> Result<Self> {
        media::tool("ffmpeg")?;
        media::tool("ffprobe")?;
        let manifest = Manifest::new(settings);
        let guard = manifest.create(store.root())?;
        let shared = Arc::new(Shared {
            status: Mutex::new(CaptureStatus {
                session_id: manifest.id.clone(),
                phase: CapturePhase::Starting,
                media_ms: 0,
                active_ms: 0,
                pending_takes: 0,
                completed_takes: 0,
                camera_running: false,
                message: "Checking the video encoder before starting camera and microphone…".into(),
            }),
            preview: Mutex::new(None),
            preview_waker: OnceLock::new(),
            events: Mutex::new(VecDeque::new()),
            frame_count: AtomicU64::new(0),
            stop: AtomicBool::new(false),
            event_overflow: AtomicBool::new(false),
        });
        let (sender, receiver) = mpsc::sync_channel(128);
        let worker_shared = shared.clone();
        let root = store.root().to_owned();
        let cancel = Cancellation::default();
        let worker_cancel = cancel.clone();
        let worker = thread::spawn(move || {
            let _guard = guard;
            if let Err(_error) = run_capture(
                &root,
                manifest,
                generated,
                &worker_shared,
                receiver,
                &worker_cancel,
            ) {
                let mut status = worker_shared
                    .status
                    .lock()
                    .unwrap_or_else(|error| error.into_inner());
                status.phase = CapturePhase::Failed;
                status.camera_running = false;
                status.pending_takes = 0;
                status.message="Capture stopped. Saved takes and buffered segments remain available through recovery.".into();
                drop(status);
                emit(
                    &worker_shared,
                    CaptureEvent::Error(
                        "Capture stopped; recordings were retained for recovery.".into(),
                    ),
                );
            }
        });
        Ok(Self {
            shared,
            commands: sender,
            epoch: Instant::now(),
            cancel,
            worker: Some(worker),
        })
    }
    pub fn status(&self) -> CaptureStatus {
        self.shared
            .status
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .clone()
    }
    pub fn latest_preview(&self) -> Option<Arc<PreviewFrame>> {
        self.shared
            .preview
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .as_ref()
            .map(|(frame, _)| frame.clone())
    }
    /// Register one nonblocking frame-ready callback. It runs on the preview
    /// reader after publication, outside the latest-frame lock.
    pub fn set_preview_waker(&self, wake: impl Fn() + Send + Sync + 'static) -> bool {
        self.shared.preview_waker.set(Arc::new(wake)).is_ok()
    }
    pub fn poll_events(&self) -> Vec<CaptureEvent> {
        self.shared
            .events
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .drain(..)
            .collect()
    }
    fn stamp(&self, fresh: bool) -> Result<Stamp> {
        let preview = self
            .shared
            .preview
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        let (frame, received) = preview.as_ref().ok_or("Camera preview is not ready")?;
        if fresh && received.elapsed() > Duration::from_millis(250) {
            return Err(
                "Camera preview is stalled; wait or restart the camera before recording".into(),
            );
        }
        Ok(Stamp {
            frame: frame.sequence,
            host_ms: self.epoch.elapsed().as_millis().min(u128::from(u64::MAX)) as u64,
        })
    }
    fn send(&self, command: Control) -> Result<()> {
        self.commands
            .try_send(command)
            .map_err(|_| "Capture is busy or stopped; recordings remain available".into())
    }
    pub fn press(&self) -> Result<()> {
        if !matches!(
            self.status().phase,
            CapturePhase::Ready | CapturePhase::Holding | CapturePhase::LongTake
        ) {
            return Err("Start the camera and wait for its preview before recording".into());
        }
        self.send(Control::Press(self.stamp(true)?))
    }
    pub fn release(&self) -> Result<()> {
        let result = self.send(Control::Release(self.stamp(false)?));
        if result.is_err() {
            self.shared.stop.store(true, Ordering::Release);
        }
        result
    }
    pub fn stop_take(&self) -> Result<()> {
        let result = self.send(Control::StopTake(self.stamp(false)?));
        if result.is_err() {
            self.shared.stop.store(true, Ordering::Release);
        }
        result
    }
    pub fn cancel_take(&self) -> Result<()> {
        self.send(Control::CancelTake)
    }
    pub fn mark_slot(
        &self,
        slot: SlotKey,
        content: OverlayContent,
        duration_ms: u64,
    ) -> Result<()> {
        content.validate()?;
        if duration_ms == 0 {
            return Err("Overlay duration must be positive".into());
        }
        self.send(Control::Mark(self.stamp(true)?, slot, content, duration_ms))
    }
    pub fn stop(&self) -> Result<()> {
        self.shared.stop.store(true, Ordering::Release);
        let _ = self.commands.try_send(Control::Stop);
        Ok(())
    }
}
impl Drop for CaptureSession {
    fn drop(&mut self) {
        self.shared.stop.store(true, Ordering::Release);
        self.cancel.cancel();
        let _ = self.commands.try_send(Control::Stop);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}
fn emit(shared: &Shared, event: CaptureEvent) {
    let mut events = shared
        .events
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    if events.len() < 256 {
        events.push_back(event);
    } else {
        shared.event_overflow.store(true, Ordering::Release);
        shared.stop.store(true, Ordering::Release);
        let mut status = shared
            .status
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        status.message="Capture paused because its event queue is full. Completed recordings remain in recovery.".into();
    }
}

struct CameraProcess {
    child: FfmpegChild,
    readers: Vec<JoinHandle<()>>,
    stopped: bool,
    frame_rate_guard: Option<devices::FrameRateGuard>,
}
impl CameraProcess {
    fn finish(&mut self) {
        if !self.stopped {
            let _ = self.child.kill();
            let _ = self.child.as_inner_mut().wait();
            self.stopped = true;
        }
        for reader in self.readers.drain(..) {
            let _ = reader.join();
        }
        drop(self.frame_rate_guard.take());
    }
}
impl Drop for CameraProcess {
    fn drop(&mut self) {
        self.finish();
    }
}
struct CutWorker {
    worker: Option<JoinHandle<()>>,
    cancel: Cancellation,
}
impl Drop for CutWorker {
    fn drop(&mut self) {
        self.cancel.cancel();
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

// Capture settings validate positive, supported source dimensions before spawn.
// Keep the natural aspect and avoid upscaling smaller cameras or adding bars.
fn preview_dimensions(mode: CameraMode) -> (u32, u32) {
    let mut height = u64::from(mode.height.min(PREVIEW_HEIGHT));
    let mut width = u64::from(mode.width) * height / u64::from(mode.height);
    if width > u64::from(PREVIEW_WIDTH) {
        width = u64::from(PREVIEW_WIDTH);
        height = u64::from(mode.height) * width / u64::from(mode.width);
    }
    ((width as u32) & !1, (height as u32) & !1)
}
fn publish_preview(shared: &Shared, frame: Arc<PreviewFrame>) {
    shared
        .frame_count
        .store(frame.sequence.saturating_add(1), Ordering::Release);
    *shared
        .preview
        .lock()
        .unwrap_or_else(|error| error.into_inner()) = Some((frame, Instant::now()));
    if let Some(wake) = shared.preview_waker.get() {
        wake();
    }
}
fn spawn_camera(
    root: &Path,
    manifest: &Manifest,
    generated: bool,
    shared: Arc<Shared>,
) -> Result<CameraProcess> {
    let settings = &manifest.settings;
    let frame_rate_guard = if generated {
        None
    } else {
        devices::keep_frame_rate(&settings.camera_id)
    };
    let (preview_width, preview_height) = preview_dimensions(settings.mode);
    let directory = local::existing_directory(root, &manifest.relative())?;
    let mut command = FfmpegCommand::new_with_path(media::tool("ffmpeg")?);
    command.as_inner_mut().current_dir(&directory);
    command.args([
        "-hide_banner",
        "-loglevel",
        "error",
        "-nostats",
        "-n",
        "-filter_complex_threads",
        "1",
    ]);
    if generated {
        let source = format!(
            "testsrc2=size={}x{}:rate=30",
            settings.mode.width, settings.mode.height
        );
        #[cfg(feature = "capture-test")]
        let source = if std::env::var("STORYFOLD_CAPTURE_CLOCK_TEST").as_deref() == Ok("1") {
            format!(
                "{source},drawbox=x=0:y=0:w=1100:h=150:color=white:t=fill,drawtext=font=monospace:text='%{{localtime\\:%s.%3N}}':fontsize=90:fontcolor=black:x=24:y=24"
            )
        } else {
            source
        };
        command
            .args(["-re", "-thread_queue_size", "2", "-f", "lavfi", "-i"])
            .arg(source);
    } else {
        command
            .args([
                "-thread_queue_size",
                "2",
                "-f",
                "v4l2",
                "-input_format",
                settings.mode.format.argument(),
                "-video_size",
            ])
            .arg(format!("{}x{}", settings.mode.width, settings.mode.height))
            .args(["-framerate", "30", "-i", &settings.camera_id]);
    }
    if generated || settings.microphone_id.is_none() {
        let audio = if generated && settings.microphone_id.is_some() {
            "sine=frequency=440:sample_rate=48000"
        } else {
            "anullsrc=channel_layout=stereo:sample_rate=48000"
        };
        command.args(["-re", "-thread_queue_size", "8", "-f", "lavfi", "-i", audio]);
    } else if let Some(microphone) = &settings.microphone_id {
        command.args([
            "-thread_queue_size",
            "8",
            "-fragment_size",
            "3840",
            "-f",
            "pulse",
            "-i",
            microphone,
        ]);
    }
    command.arg("-filter_complex").arg(format!("[0:v]fps=30,setpts=N/(30*TB),split=2[vrec][vp];[vp]scale={preview_width}:{preview_height},format=rgba[preview];[1:a]asetpts=PTS-STARTPTS,aresample=async=1:first_pts=0[a]"));
    // Two encode threads fell behind in measured 1080p capture.
    let encode_threads = thread::available_parallelism()
        .map_or(2, |count| count.get().min(4))
        .to_string();
    command.args(["-map", "[vrec]", "-map", "[a]"]);
    configure_capture_video(&mut command, manifest.encoder, &encode_threads);
    command.args([
        "-pix_fmt",
        "yuv420p",
        "-profile:v",
        "high",
        "-g",
        "30",
        "-keyint_min",
        "30",
        "-sc_threshold",
        "0",
        "-bf",
        "0",
        "-c:a",
        "aac",
        "-b:a",
        "160k",
        "-ar",
        "48000",
        "-ac",
        "2",
        "-f",
        "segment",
        "-segment_format",
        "mpegts",
        "-segment_time",
        "1",
        "-segment_time_delta",
        "0.016667",
        "-reset_timestamps",
        "1",
        "-segment_list",
        "segments.csv",
        "-segment_list_type",
        "csv",
        "-segment_list_size",
        "0",
        "segment-%09d.ts",
        "-map",
        "[preview]",
        "-an",
        "-c:v",
        "rawvideo",
        "-threads",
        "1",
        "-pix_fmt",
        "rgba",
        "-f",
        "rawvideo",
        "pipe:1",
    ]);
    let mut child = command
        .spawn()
        .map_err(|_| "System FFmpeg could not start capture")?;
    let mut stdout = child.take_stdout().ok_or("Missing camera preview output")?;
    let mut stderr = child
        .take_stderr()
        .ok_or("Missing camera diagnostic output")?;
    let frames_shared = shared;
    let frames = thread::spawn(move || {
        let length = (preview_width * preview_height * 4) as usize;
        let mut sequence = 0_u64;
        loop {
            let mut rgba = vec![0_u8; length];
            if stdout.read_exact(&mut rgba).is_err() {
                break;
            }
            let frame = Arc::new(PreviewFrame {
                sequence,
                media_ms: frame_ms(sequence),
                width: preview_width,
                height: preview_height,
                rgba,
            });
            publish_preview(&frames_shared, frame);
            sequence = match sequence.checked_add(1) {
                Some(next) => next,
                None => break,
            };
            frames_shared.frame_count.store(sequence, Ordering::Release);
        }
    });
    // Drain without retaining unbounded FFmpeg output or reflecting local paths
    // into user errors. Exit status and explicit stall timers drive diagnostics.
    let errors = thread::spawn(move || {
        let mut bytes = [0; 4096];
        while let Ok(count) = stderr.read(&mut bytes) {
            if count == 0 {
                break;
            }
        }
    });
    Ok(CameraProcess {
        child,
        readers: vec![frames, errors],
        stopped: false,
        frame_rate_guard,
    })
}

fn stop_before_devices(root: &Path, manifest: &mut Manifest, shared: &Shared) -> Result<bool> {
    if !shared.stop.load(Ordering::Acquire) {
        return Ok(false);
    }
    manifest.stopped = true;
    manifest.save(root)?;
    let mut status = shared
        .status
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    status.phase = CapturePhase::Stopped;
    status.camera_running = false;
    status.message = "Camera is off. No capture devices were started.".into();
    drop(status);
    emit(shared, CaptureEvent::Stopped);
    Ok(true)
}

fn run_capture(
    root: &Path,
    mut manifest: Manifest,
    generated: bool,
    shared: &Arc<Shared>,
    commands: mpsc::Receiver<Control>,
    cancel: &Cancellation,
) -> Result<()> {
    cancel.check()?;
    if stop_before_devices(root, &mut manifest, shared)? {
        return Ok(());
    }
    // One backend per buffered session. Select and persist it before opening
    // capture devices; later cuts and explicit recovery reuse this choice.
    manifest.encoder = crate::encoding::select(cancel)?;
    manifest.save(root)?;
    cancel.check()?;
    if stop_before_devices(root, &mut manifest, shared)? {
        return Ok(());
    }
    // Declaration order matters on errors: drop/reap the camera before joining
    // the cutter, so slow postprocessing cannot leave hardware open invisibly.
    let mut cutter = CutWorker {
        worker: None,
        cancel: cancel.clone(),
    };
    let mut camera = spawn_camera(root, &manifest, generated, shared.clone())?;
    shared
        .status
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .camera_running = true;
    let (jobs, job_receiver) = mpsc::sync_channel::<TakeRange>(16);
    let (results, result_receiver) =
        mpsc::sync_channel::<(u64, std::result::Result<ImportedMedia, String>)>(16);
    let cut_root = root.to_owned();
    let cut_directory = manifest.relative();
    let cut_cancel = cancel.clone();
    let cut_encoder = manifest.encoder;
    let cutter_thread = thread::spawn(move || {
        while !cut_cancel.is_cancelled() {
            let take = match job_receiver.recv_timeout(Duration::from_millis(50)) {
                Ok(take) => take,
                Err(mpsc::RecvTimeoutError::Timeout) => continue,
                Err(mpsc::RecvTimeoutError::Disconnected) => break,
            };
            let result=cut_take(&cut_root,&cut_directory,&take,cut_encoder,&cut_cancel).map_err(|_|"Take preparation failed or was cancelled; source segments remain available through recovery.".into());
            if results.try_send((take.ordinal, result)).is_err() {
                break;
            }
        }
    });
    cutter.worker = Some(cutter_thread);
    let mut gesture = Gesture::new(manifest.settings.tap_threshold_ms);
    let mut cursor = 0;
    let mut in_flight = HashSet::new();
    let mut ready = false;
    let mut stopping: Option<Instant> = None;
    let mut camera_running = true;
    let mut failed = false;
    let startup = Instant::now();
    loop {
        manifest.frame_count = shared.frame_count.load(Ordering::Acquire);
        while let Ok(command) = commands.try_recv() {
            manifest.frame_count = shared.frame_count.load(Ordering::Acquire);
            let action = match command {
                Control::Press(at) => gesture.press(at),
                Control::Release(at) => gesture.release(at),
                Control::StopTake(at) => gesture.stop(at),
                Control::CancelTake => gesture.cancel(),
                Control::Stop => {
                    shared.stop.store(true, Ordering::Release);
                    Action::None
                }
                #[cfg(all(test, feature = "capture-test"))]
                Control::InterruptGenerated => {
                    let _ = camera.child.kill();
                    failed = true;
                    Action::None
                }
                Control::Mark(at, slot, content, duration_ms) => {
                    if let Some(active) = &mut manifest.active {
                        if active.marks.len() < crate::MAX_CUES {
                            active.marks.push(SlotMark {
                                slot,
                                content,
                                duration_ms,
                                offset_ms: frame_ms(at.frame.saturating_sub(active.start)),
                            });
                        } else {
                            emit(
                                shared,
                                CaptureEvent::Error(
                                    "This take reached its overlay mark resource limit.".into(),
                                ),
                            );
                        }
                    }
                    Action::None
                }
            };
            handle_action(&mut manifest, &gesture, action, shared)?;
            manifest.save(root)?;
        }
        if camera_running && shared.stop.load(Ordering::Acquire) && stopping.is_none() {
            let _ = camera.child.quit();
            stopping = Some(Instant::now());
        }
        if camera_running {
            let exited = camera.child.as_inner_mut().try_wait()?;
            if let Some(exit) = exited {
                camera.stopped = true;
                camera.finish();
                camera_running = false;
                manifest.frame_count = shared.frame_count.load(Ordering::Acquire);
                let at = Stamp {
                    frame: manifest.frame_count.saturating_sub(1),
                    host_ms: 0,
                };
                let action = gesture.stop(at);
                handle_action(&mut manifest, &gesture, action, shared)?;
                manifest.stopped = true;
                if !exit.success() || stopping.is_none() {
                    failed = true;
                    emit(shared,CaptureEvent::Error("Camera stopped unexpectedly. Completed takes were kept; remaining buffered video can be recovered.".into()));
                }
            } else if stopping.is_some_and(|started| started.elapsed() > Duration::from_secs(5)) {
                let _ = camera.child.kill();
            }
        }
        if buffer::read_closed(root, &mut manifest, &mut cursor)? {
            manifest.prune(root)?;
            manifest.save(root)?;
        }
        if !camera_running {
            for take in &mut manifest.takes {
                if take.media.is_none()
                    && take.error.is_none()
                    && !in_flight.contains(&take.ordinal)
                    && CutPlan::new(take.start, take.end)?.last >= manifest.closed_count
                {
                    let message="The last camera segment did not close. Buffered video was retained; use recovery to salvage complete frames.".to_owned();
                    take.error = Some(message.clone());
                    emit(
                        shared,
                        CaptureEvent::TakeFailed {
                            ordinal: take.ordinal,
                            message,
                        },
                    );
                }
            }
        }
        if !ready && manifest.frame_count > 0 {
            ready = true;
            emit(shared, CaptureEvent::Ready);
        }
        if camera_running && stopping.is_none() {
            let stalled = if let Some((_, received)) = &*shared
                .preview
                .lock()
                .unwrap_or_else(|error| error.into_inner())
            {
                received.elapsed() > Duration::from_secs(2)
            } else {
                startup.elapsed() > Duration::from_secs(10)
            };
            if stalled {
                failed = true;
                shared.stop.store(true, Ordering::Release);
                emit(
                    shared,
                    CaptureEvent::Error(
                        "Camera preview stalled. Stopping capture and preserving the buffer."
                            .into(),
                    ),
                );
            }
        }
        loop {
            let (ordinal, result) = match result_receiver.try_recv() {
                Ok(result) => result,
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    if in_flight.is_empty() {
                        break;
                    }
                    return Err("Take worker stopped; ranges were retained for recovery".into());
                }
            };
            in_flight.remove(&ordinal);
            if let Some(take) = manifest
                .takes
                .iter_mut()
                .find(|take| take.ordinal == ordinal)
            {
                match result {
                    Ok(media) => {
                        take.media = Some(media.clone());
                        take.error = None;
                        emit(
                            shared,
                            CaptureEvent::TakeCompleted(CompletedTake {
                                ordinal,
                                media,
                                marks: take.marks.clone(),
                            }),
                        );
                    }
                    Err(message) => {
                        take.error = Some(message.clone());
                        emit(shared, CaptureEvent::TakeFailed { ordinal, message });
                    }
                }
            }
            manifest.save(root)?;
        }
        if cancel.is_cancelled() {
            for take in manifest
                .takes
                .iter_mut()
                .filter(|take| take.media.is_none() && !in_flight.contains(&take.ordinal))
            {
                take.error =
                    Some("Preparation cancelled; buffered video remains available.".into());
            }
        } else {
            for take in &manifest.takes {
                if take.media.is_some() || take.error.is_some() || in_flight.contains(&take.ordinal)
                {
                    continue;
                }
                let plan = CutPlan::new(take.start, take.end)?;
                if plan.last >= manifest.closed_count {
                    continue;
                }
                match jobs.try_send(take.clone()) {
                    Ok(()) => {
                        in_flight.insert(take.ordinal);
                    }
                    Err(mpsc::TrySendError::Full(_)) => break,
                    Err(mpsc::TrySendError::Disconnected(_)) => {
                        return Err("Take worker stopped".into());
                    }
                }
            }
        }
        let pending = manifest
            .takes
            .iter()
            .filter(|take| take.media.is_none() && take.error.is_none())
            .count();
        failed |= shared.event_overflow.load(Ordering::Acquire);
        {
            let mut status = shared
                .status
                .lock()
                .unwrap_or_else(|error| error.into_inner());
            status.camera_running = camera_running;
            status.media_ms = frame_ms(manifest.frame_count);
            status.active_ms = gesture.active_start().map_or(0, |start| {
                frame_ms(manifest.frame_count.saturating_sub(start))
            });
            status.pending_takes = pending;
            status.completed_takes = manifest.completed();
            status.phase = if camera_running {
                if stopping.is_some() {
                    CapturePhase::Stopping
                } else {
                    match gesture.state {
                        GestureState::Pressed { .. } => CapturePhase::Holding,
                        GestureState::Long { .. } => CapturePhase::LongTake,
                        GestureState::Idle => {
                            if ready {
                                CapturePhase::Ready
                            } else {
                                CapturePhase::Starting
                            }
                        }
                    }
                }
            } else if pending > 0 || !in_flight.is_empty() {
                CapturePhase::Finishing
            } else if failed || manifest.takes.iter().any(|take| take.error.is_some()) {
                CapturePhase::Failed
            } else {
                CapturePhase::Stopped
            };
            let message = match status.phase {
                CapturePhase::Starting => "Waiting for the camera preview…",
                CapturePhase::Ready => {
                    "Camera is on. Hold Record for a clip, or tap for a long take."
                }
                CapturePhase::Holding => "Recording a clip. Release to finish.",
                CapturePhase::LongTake => {
                    "Recording a long take. Press Record again or Stop take to finish."
                }
                CapturePhase::Stopping => "Stopping camera and preserving the recording buffer…",
                CapturePhase::Finishing => "Camera is off. Preparing completed takes…",
                CapturePhase::Stopped => "Camera is off. Completed takes are available.",
                CapturePhase::Failed => {
                    "Capture stopped with an error. Saved takes and remaining segments are available in recovery."
                }
            };
            status.message = format!("{} · {message}", manifest.encoder.label());
        }
        if !camera_running && pending == 0 && in_flight.is_empty() {
            manifest.save(root)?;
            break;
        }
        thread::sleep(Duration::from_millis(10));
    }
    drop(jobs);
    if let Some(worker) = cutter.worker.take() {
        let _ = worker.join();
    }
    emit(shared, CaptureEvent::Stopped);
    Ok(())
}
fn handle_action(
    manifest: &mut Manifest,
    gesture: &Gesture,
    action: Action,
    shared: &Shared,
) -> Result<()> {
    match action {
        Action::None => {}
        Action::Started { frame } => {
            if manifest.takes.len() >= crate::MAX_CLIPS {
                return Err("Capture reached the project clip resource limit".into());
            }
            let ordinal = manifest.next_ordinal;
            manifest.next_ordinal = ordinal.checked_add(1).ok_or("Take identity overflow")?;
            manifest.active = Some(ActiveRange {
                ordinal,
                start: frame,
                marks: Vec::new(),
            });
        }
        Action::Finished { start, end } => {
            if let Some(active) = manifest.active.take() {
                let end = end.min(manifest.frame_count).max(start.saturating_add(1));
                manifest.takes.push(TakeRange {
                    ordinal: active.ordinal,
                    start,
                    end,
                    marks: active.marks,
                    media: None,
                    error: None,
                });
                emit(
                    shared,
                    CaptureEvent::TakeQueued {
                        ordinal: active.ordinal,
                    },
                );
            }
        }
        Action::Cancelled => {
            manifest.active = None;
        }
    }
    let _ = gesture;
    Ok(())
}

fn temporary(root: &Path, directory: &str, extension: &str) -> Result<PendingFile> {
    PendingFile::prepare(
        root,
        &format!("{directory}/work-{}.{}", media::unique_id(), extension),
    )
}
fn concat_list(root: &Path, directory: &str, paths: &[PathBuf]) -> Result<PendingFile> {
    let mut list = temporary(root, directory, "ffconcat")?;
    list.file_mut().write_all(b"ffconcat version 1.0\n")?;
    for path in paths {
        if path.parent() != Some(root.join(directory).as_path()) {
            return Err("Concat input left its capture folder".into());
        }
        let name = path
            .file_name()
            .and_then(|name| name.to_str())
            .ok_or("Invalid concat input")?;
        local::validate_relative_path(name)?;
        if name.contains('\'') {
            return Err("Invalid concat filename".into());
        }
        writeln!(list.file_mut(), "file '{name}'")?;
    }
    list.file_mut().flush()?;
    Ok(list)
}
fn ffmpeg_job(command: &mut FfmpegCommand, cancel: &Cancellation, timeout: Duration) -> Result<()> {
    run_bounded(command.as_inner_mut(), timeout, cancel, 4096)?;
    Ok(())
}
fn ffmpeg() -> Result<FfmpegCommand> {
    let mut command = FfmpegCommand::new_with_path(media::tool("ffmpeg")?);
    command.args([
        "-hide_banner",
        "-loglevel",
        "error",
        "-nostats",
        "-y",
        "-filter_threads",
        "1",
    ]);
    Ok(command)
}
fn configure_capture_video(command: &mut FfmpegCommand, encoder: VideoEncoder, threads: &str) {
    encoder.configure(command.as_inner_mut(), 20, true);
    command.args(["-threads", threads]);
}
fn encode_edge(
    root: &Path,
    directory: &str,
    source: &Path,
    range: std::ops::Range<u64>,
    encoder: VideoEncoder,
    cancel: &Cancellation,
) -> Result<PendingFile> {
    let output = temporary(root, directory, "ts")?;
    let mut command = ffmpeg()?;
    command
        .args(["-protocol_whitelist", "file,pipe", "-i"])
        .arg(source)
        .args(["-map", "0:v:0", "-an", "-vf"])
        .arg(format!(
            "trim=start_frame={}:end_frame={},setpts=PTS-STARTPTS",
            range.start, range.end
        ));
    configure_capture_video(&mut command, encoder, "2");
    command
        .args([
            "-pix_fmt",
            "yuv420p",
            "-profile:v",
            "high",
            "-g",
            "30",
            "-keyint_min",
            "30",
            "-sc_threshold",
            "0",
            "-bf",
            "0",
            "-f",
            "mpegts",
        ])
        .arg(output.path());
    ffmpeg_job(&mut command, cancel, Duration::from_secs(60))?;
    Ok(output)
}
fn cut_take(
    root: &Path,
    directory: &str,
    take: &TakeRange,
    encoder: VideoEncoder,
    cancel: &Cancellation,
) -> Result<ImportedMedia> {
    cancel.check()?;
    let plan = CutPlan::new(take.start, take.end)?;
    // The processing budget scales with the selected take, rather than imposing
    // an interview-length cap. Edge encodes are always at most one second each.
    let operation_timeout =
        Duration::from_millis(frame_ms(plan.frames)).saturating_add(Duration::from_secs(60));
    let mut originals = Vec::new();
    for index in plan.first..=plan.last {
        originals.push(local::confined_file(
            root,
            &format!("{directory}/{}", segment_name(index)),
            PathPolicy::LocalProject,
            FileMode::Existing,
        )?);
    }
    let mut pieces = Vec::new();
    let first_end = if plan.first == plan.last {
        plan.last_end
    } else {
        FRAME_RATE
    };
    pieces.push(encode_edge(
        root,
        directory,
        &originals[0],
        plan.first_offset..first_end,
        encoder,
        cancel,
    )?);
    if originals.len() > 2 {
        let middle_list = concat_list(root, directory, &originals[1..originals.len() - 1])?;
        let middle = temporary(root, directory, "ts")?;
        let mut command = ffmpeg()?;
        command
            .args([
                "-protocol_whitelist",
                "file,pipe",
                "-f",
                "concat",
                "-safe",
                "1",
                "-i",
            ])
            .arg(middle_list.path())
            .args(["-map", "0:v:0", "-an", "-c:v", "copy", "-f", "mpegts"])
            .arg(middle.path());
        ffmpeg_job(&mut command, cancel, operation_timeout)?;
        pieces.push(middle);
    }
    if originals.len() > 1 {
        pieces.push(encode_edge(
            root,
            directory,
            originals.last().ok_or("Missing final segment")?,
            0..plan.last_end,
            encoder,
            cancel,
        )?);
    }
    let video_paths: Vec<_> = pieces.iter().map(|piece| piece.path().to_owned()).collect();
    let video_list = concat_list(root, directory, &video_paths)?;
    let audio_list = concat_list(root, directory, &originals)?;
    let relative = format!("media/take-{}.mp4", media::unique_id());
    let output = PendingFile::prepare(root, &relative)?;
    let mut command = ffmpeg()?;
    let seconds = plan.frames as f64 / 30.;
    let audio_start = plan.first_offset as f64 / 30.;
    command
        .args([
            "-protocol_whitelist",
            "file,pipe",
            "-f",
            "concat",
            "-safe",
            "1",
            "-i",
        ])
        .arg(video_list.path())
        .args([
            "-protocol_whitelist",
            "file,pipe",
            "-f",
            "concat",
            "-safe",
            "1",
            "-i",
        ])
        .arg(audio_list.path())
        .args(["-map", "0:v:0", "-map", "1:a:0", "-c:v", "copy", "-af"])
        .arg(format!(
            "atrim=start={audio_start:.9}:duration={seconds:.9},asetpts=PTS-STARTPTS"
        ))
        .args([
            "-c:a", "aac", "-b:a", "160k", "-ar", "48000", "-ac", "2", "-t",
        ])
        .arg(format!("{seconds:.9}"))
        .args([
            "-movflags",
            "+faststart",
            "-video_track_timescale",
            "90000",
            "-f",
            "mp4",
        ])
        .arg(output.path());
    ffmpeg_job(&mut command, cancel, operation_timeout)?;
    let info = media::probe_owned(output.path(), cancel)?;
    let expected = frame_ms(plan.frames);
    if info.duration_ms.abs_diff(expected) > 34 || !info.has_audio {
        return Err(
            "Prepared take did not match its buffered frame range; segments retained".into(),
        );
    }
    cancel.check()?;
    output.publish()?;
    ImportedMedia::published(root, relative, format!("Take {}", take.ordinal), info)
}

pub fn list_recoverable(store: &ProjectStore) -> Result<Vec<RecoverySession>> {
    let directory = local::existing_directory(store.root(), "buffer")?;
    let mut sessions = Vec::new();
    for entry in fs::read_dir(directory)?.take(10_000) {
        let entry = entry?;
        let id = entry.file_name().to_string_lossy().to_string();
        if !buffer::valid_id(&id) || entry.file_type()?.is_symlink() || !entry.file_type()?.is_dir()
        {
            continue;
        }
        if let Ok(manifest) = Manifest::load(store.root(), &id) {
            if manifest.takes.is_empty() && manifest.active.is_none() {
                continue;
            }
            sessions.push(RecoverySession {
                id,
                pending_takes: manifest.pending() + usize::from(manifest.active.is_some()),
                completed_takes: manifest.completed(),
                message:
                    "Buffered or completed takes are available. Recovery never starts a camera."
                        .into(),
            });
        }
    }
    sessions.sort_by(|a, b| a.id.cmp(&b.id));
    Ok(sessions)
}
pub fn recover_session(
    store: &ProjectStore,
    id: &str,
    cancel: &Cancellation,
) -> Result<Vec<CompletedTake>> {
    cancel.check()?;
    if !buffer::valid_id(id) {
        return Err("Invalid recovery session identity".into());
    }
    let _guard = local::lock_named(
        store.root(),
        PrivateNamespace::Sidebar,
        &format!("{id}.lock"),
    )?;
    let mut manifest = Manifest::load(store.root(), id)?;
    if manifest.pending() == 0 && manifest.active.is_none() {
        return manifest
            .takes
            .into_iter()
            .filter_map(|take| take.media.map(|media| (take.ordinal, media, take.marks)))
            .map(|(ordinal, media, marks)| {
                Ok(CompletedTake {
                    ordinal,
                    media: media.rebind(store.root())?,
                    marks,
                })
            })
            .collect();
    }
    let previous_frame_count = manifest.frame_count;
    let mut cursor = 0;
    while buffer::read_closed(store.root(), &mut manifest, &mut cursor)? {}
    let mut available_frames = manifest
        .closed_count
        .checked_mul(FRAME_RATE)
        .ok_or("Recovery frame count overflow")?;
    // A stopped process can leave one final MPEG-TS file before its CSV close.
    // Probe only that exact known next filename; never scan arbitrary media.
    let tail = manifest.path(
        store.root(),
        &segment_name(manifest.closed_count),
        FileMode::NewOrExisting,
    )?;
    if tail.is_file() {
        if let Ok(frames) = count_frames(&tail, cancel)
            && frames > 0
            && frames <= FRAME_RATE
        {
            manifest.closed_count += 1;
            available_frames = (manifest.closed_count - 1)
                .checked_mul(FRAME_RATE)
                .and_then(|start| start.checked_add(frames))
                .ok_or("Recovery frame count overflow")?;
        }
    } else if manifest.closed_count > 0 {
        let last = manifest.path(
            store.root(),
            &segment_name(manifest.closed_count - 1),
            FileMode::Existing,
        )?;
        let frames = count_frames(&last, cancel)?;
        if frames == 0 || frames > FRAME_RATE {
            return Err("Recovery segment frame count is invalid".into());
        }
        available_frames = (manifest.closed_count - 1)
            .checked_mul(FRAME_RATE)
            .and_then(|start| start.checked_add(frames))
            .ok_or("Recovery frame count overflow")?;
    }
    manifest.frame_count = previous_frame_count.max(available_frames);
    if let Some(active) = manifest.active.take() {
        if active.start < available_frames {
            manifest.takes.push(TakeRange {
                ordinal: active.ordinal,
                start: active.start,
                end: available_frames,
                marks: active.marks,
                media: None,
                error: None,
            });
        } else {
            return Err("No complete buffered frame remains for the active take; its existing recovery metadata was retained".into());
        }
    }
    for take in manifest
        .takes
        .iter_mut()
        .filter(|take| take.media.is_none())
    {
        if take.start >= available_frames {
            return Err("No complete buffered frame remains for this take; existing recovery metadata was retained".into());
        }
        take.end = take.end.min(available_frames);
    }
    manifest.stopped = true;
    manifest.save(store.root())?;
    for index in 0..manifest.takes.len() {
        cancel.check()?;
        if manifest.takes[index].media.is_some() {
            continue;
        }
        if manifest.takes[index].start >= manifest.takes[index].end {
            manifest.takes[index].error =
                Some("No complete frame remains for this interrupted take.".into());
            continue;
        }
        let media = cut_take(
            store.root(),
            &manifest.relative(),
            &manifest.takes[index],
            manifest.encoder,
            cancel,
        )?;
        manifest.takes[index].media = Some(media);
        manifest.takes[index].error = None;
        manifest.save(store.root())?;
    }
    let mut completed = Vec::new();
    for take in manifest.takes {
        if let Some(media) = take.media {
            completed.push(CompletedTake {
                ordinal: take.ordinal,
                media: media.rebind(store.root())?,
                marks: take.marks,
            });
        }
    }
    Ok(completed)
}
fn count_frames(source: &Path, cancel: &Cancellation) -> Result<u64> {
    let output = run_bounded(
        std::process::Command::new(media::tool("ffprobe")?)
            .args([
                "-v",
                "error",
                "-protocol_whitelist",
                "file,pipe",
                "-select_streams",
                "v:0",
                "-count_frames",
                "-show_entries",
                "stream=nb_read_frames",
                "-of",
                "json",
            ])
            .arg(source),
        Duration::from_secs(30),
        cancel,
        65_536,
    )?;
    let value: serde_json::Value = serde_json::from_slice(&output.stdout)
        .map_err(|_| "Recovery probe returned invalid metadata")?;
    value["streams"][0]["nb_read_frames"]
        .as_str()
        .and_then(|count| count.parse().ok())
        .ok_or_else(|| "Recovery frame count is unavailable".into())
}

#[cfg(all(test, target_os = "linux", feature = "capture-test"))]
mod tests {
    use super::*;
    #[test]
    fn stop_during_preflight_keeps_capture_devices_off() {
        let temp = tempfile::tempdir().unwrap();
        let (store, _) =
            ProjectStore::create(temp.path().join("project"), "Stopped preflight").unwrap();
        let mut manifest = Manifest::new(CaptureSettings::default());
        let _guard = manifest.create(store.root()).unwrap();
        manifest.encoder = VideoEncoder::Nvidia;
        let shared = Shared {
            status: Mutex::new(CaptureStatus {
                session_id: manifest.id.clone(),
                phase: CapturePhase::Starting,
                media_ms: 0,
                active_ms: 0,
                pending_takes: 0,
                completed_takes: 0,
                camera_running: false,
                message: String::new(),
            }),
            preview: Mutex::new(None),
            preview_waker: OnceLock::new(),
            events: Mutex::new(VecDeque::new()),
            frame_count: AtomicU64::new(0),
            stop: AtomicBool::new(true),
            event_overflow: AtomicBool::new(false),
        };
        assert!(stop_before_devices(store.root(), &mut manifest, &shared).unwrap());
        let status = shared.status.lock().unwrap();
        assert_eq!(status.phase, CapturePhase::Stopped);
        assert!(status.can_leave());
        assert!(!status.camera_running);
        assert!(shared.preview.lock().unwrap().is_none());
        let saved = Manifest::load(store.root(), &manifest.id).unwrap();
        assert!(saved.stopped);
        assert_eq!(saved.encoder, VideoEncoder::Nvidia);
        assert!(matches!(
            shared.events.lock().unwrap().front(),
            Some(CaptureEvent::Stopped)
        ));
    }
    #[test]
    fn capture_encoder_configuration_honors_manifest_backend() {
        let mut command = FfmpegCommand::new_with_path("not-executed");
        configure_capture_video(&mut command, VideoEncoder::Nvidia, "2");
        let args: Vec<_> = command
            .as_inner_mut()
            .get_args()
            .filter_map(|arg| arg.to_str())
            .collect();
        assert!(
            args.windows(2).any(|pair| pair == ["-c:v", "h264_nvenc"]),
            "Capture must use the backend saved for this session"
        );
        assert!(!args.contains(&"libx264"));
        assert!(!args.contains(&"-crf"));
        assert!(args.windows(2).any(|pair| pair == ["-tune", "ull"]));
    }
    #[test]
    fn preview_publication_wakes_once_after_unlock_and_keeps_media_clock() {
        let (commands, _receiver) = mpsc::sync_channel(1);
        let session = CaptureSession {
            shared: Arc::new(Shared {
                status: Mutex::new(CaptureStatus {
                    session_id: "generated-waker".into(),
                    phase: CapturePhase::Starting,
                    media_ms: 0,
                    active_ms: 0,
                    pending_takes: 0,
                    completed_takes: 0,
                    camera_running: false,
                    message: String::new(),
                }),
                preview: Mutex::new(None),
                preview_waker: OnceLock::new(),
                events: Mutex::new(VecDeque::new()),
                frame_count: AtomicU64::new(0),
                stop: AtomicBool::new(false),
                event_overflow: AtomicBool::new(false),
            }),
            commands,
            epoch: Instant::now(),
            cancel: Cancellation::default(),
            worker: None,
        };
        let weak = Arc::downgrade(&session.shared);
        let observed = Arc::new(Mutex::new(Vec::new()));
        let frames = observed.clone();
        assert!(session.set_preview_waker(move || {
            let shared = weak.upgrade().unwrap();
            let latest = shared
                .preview
                .try_lock()
                .expect("Frame-ready callback must run outside the preview lock");
            let frame = &latest.as_ref().unwrap().0;
            assert_eq!(frame.media_ms, frame_ms(frame.sequence));
            assert_eq!(
                shared.frame_count.load(Ordering::Acquire),
                frame.sequence + 1
            );
            frames.lock().unwrap().push(frame.sequence);
        }));
        assert!(
            !session.set_preview_waker(|| panic!("A second callback must not replace the first"))
        );
        for sequence in 0..3 {
            publish_preview(
                &session.shared,
                Arc::new(PreviewFrame {
                    sequence,
                    media_ms: frame_ms(sequence),
                    width: 2,
                    height: 2,
                    rgba: vec![0; 16],
                }),
            );
            assert_eq!(
                observed.lock().unwrap().len(),
                sequence as usize + 1,
                "Publishing a preview must wake its listener immediately"
            );
        }
        assert_eq!(*observed.lock().unwrap(), vec![0, 1, 2]);
    }
    #[test]
    #[ignore = "Generated HD frame-ready versus16ms polling; STORYFOLD_CAPTURE_TEST=1, no devices"]
    fn generated_preview_waker_delivery_and_timer_phase() {
        assert_eq!(std::env::var("STORYFOLD_CAPTURE_TEST").as_deref(), Ok("1"));
        let temp = tempfile::tempdir().unwrap();
        let (store, _) =
            ProjectStore::create(temp.path().join("project"), "Generated wake").unwrap();
        let session =
            Arc::new(CaptureSession::start_generated(&store, CaptureSettings::default()).unwrap());
        let weak = Arc::downgrade(&session);
        let (sender, receiver) = mpsc::sync_channel(512);
        assert!(session.set_preview_waker(move || {
            let Some(session) = weak.upgrade() else {
                return;
            };
            let latest = session.shared.preview.lock().unwrap();
            let (frame, published) = latest.as_ref().unwrap();
            assert_eq!(frame.media_ms, frame_ms(frame.sequence));
            let sample = (frame.sequence, *published, Instant::now());
            drop(latest);
            sender
                .try_send(sample)
                .expect("Generated wake sample bound");
        }));
        wait(&session, Duration::from_secs(15), |s| {
            s.phase == CapturePhase::Ready
        });
        let started = Instant::now();
        let mut polled = Vec::new();
        let mut last = None;
        while started.elapsed() < Duration::from_secs(6) {
            if let Some(frame) = session.latest_preview()
                && last != Some(frame.sequence)
            {
                polled.push((frame.sequence, Instant::now()));
                last = Some(frame.sequence);
            }
            thread::sleep(Duration::from_millis(16));
        }
        session.stop().unwrap();
        wait(&session, Duration::from_secs(10), |s| s.can_leave());
        let wakes: Vec<_> = receiver.try_iter().collect();
        let mut direct = Vec::new();
        let mut timer = Vec::new();
        let mut matched = Vec::new();
        for (sequence, published, notified) in wakes {
            if let Some((_, observed)) = polled.iter().find(|(seq, _)| *seq == sequence) {
                direct.push(notified.duration_since(published).as_secs_f64() * 1000.);
                timer.push(observed.duration_since(published).as_secs_f64() * 1000.);
                matched.push((sequence, published));
            }
        }
        assert!(matched.len() >= 90, "Insufficient matched generated frames");
        let fps = (matched.last().unwrap().0 - matched[0].0) as f64
            / matched
                .last()
                .unwrap()
                .1
                .duration_since(matched[0].1)
                .as_secs_f64();
        direct.sort_by(f64::total_cmp);
        timer.sort_by(f64::total_cmp);
        let median = direct[direct.len() / 2];
        let p95 = direct[direct.len() * 95 / 100];
        let timer_median = timer[timer.len() / 2];
        let timer_p95 = timer[timer.len() * 95 / 100];
        let proof = serde_json::json!({
            "generated_only":true,"physical_devices_opened":false,
            "component":"Published normalized frame to callback versus16ms latest-frame polling; excludes source/camera and actual UI/GPU",
            "matched_frames":matched.len(),"fps_before_stop":fps,
            "callback_ms":{"median":median,"p95":p95},
            "timer_poll_ms":{"median":timer_median,"p95":timer_p95},
        });
        if let Some(path) = std::env::var_os("STORYFOLD_CAPTURE_WAKE_PROOF") {
            fs::write(path, serde_json::to_vec_pretty(&proof).unwrap()).unwrap();
        }
        println!("{proof}");
        assert!(fps >= 24., "Generated capture is not keeping up");
        assert!(
            p95 < 16.,
            "Frame-ready callback missed the polling interval"
        );
        assert!(
            median < timer_median,
            "Frame-ready delivery did not beat polling"
        );
    }
    #[test]
    fn preview_bounds_preserve_camera_aspect_without_upscaling() {
        for (width, height, expected) in [
            (1920, 1080, (1280, 720)),
            (1280, 720, (1280, 720)),
            (640, 480, (640, 480)),
            (640, 360, (640, 360)),
        ] {
            assert_eq!(
                preview_dimensions(CameraMode {
                    width,
                    height,
                    ..CameraMode::default()
                }),
                expected
            );
        }
    }
    #[cfg(feature = "capture-test")]
    #[test]
    #[ignore = "Explicit generated4:3 preview framing; requires STORYFOLD_CAPTURE_TEST=1, no hardware"]
    fn generated_preview_retains_natural_four_three_geometry() {
        assert_eq!(std::env::var("STORYFOLD_CAPTURE_TEST").as_deref(), Ok("1"));
        let temp = tempfile::tempdir().unwrap();
        let (store, _) = ProjectStore::create(temp.path().join("project"), "Generated4:3").unwrap();
        let session = CaptureSession::start_generated(
            &store,
            CaptureSettings {
                mode: CameraMode {
                    width: 640,
                    height: 480,
                    ..CameraMode::default()
                },
                ..CaptureSettings::default()
            },
        )
        .unwrap();
        wait(&session, Duration::from_secs(10), |s| {
            s.phase == CapturePhase::Ready
        });
        let frame = session.latest_preview().unwrap();
        assert_eq!((frame.width, frame.height), (640, 480));
        assert_eq!(frame.rgba.len(), 640 * 480 * 4);
        session.stop().unwrap();
        wait(&session, Duration::from_secs(10), |s| s.can_leave());
    }
    fn wait(
        session: &CaptureSession,
        timeout: Duration,
        mut predicate: impl FnMut(&CaptureStatus) -> bool,
    ) {
        let started = Instant::now();
        loop {
            let status = session.status();
            if predicate(&status) {
                return;
            }
            assert!(
                started.elapsed() < timeout,
                "Capture wait timed out: {status:?}"
            );
            thread::sleep(Duration::from_millis(20));
        }
    }
    fn content() -> OverlayContent {
        OverlayContent {
            kind: crate::OverlayKind::Quote,
            title: "Captured thought".into(),
            blocks: vec![crate::OverlayBlock {
                role: "body".into(),
                text: "Keep this exact thought.".into(),
            }],
            highlights: Vec::new(),
            image: None,
            insert: None,
            motion: crate::OverlayMotion::default(),
            highlight_style: crate::HighlightStyle::default(),
        }
    }
    fn frame_bytes(path: &Path, frame: u64) -> Vec<u8> {
        let mut command = ffmpeg().unwrap();
        command
            .args(["-protocol_whitelist", "file,pipe", "-i"])
            .arg(path)
            .args(["-an", "-vf"])
            .arg(format!("select=eq(n\\,{frame})"))
            .args([
                "-frames:v",
                "1",
                "-fps_mode",
                "passthrough",
                "-pix_fmt",
                "yuv420p",
                "-f",
                "rawvideo",
                "pipe:1",
            ]);
        run_bounded(
            command.as_inner_mut(),
            Duration::from_secs(30),
            &Cancellation::default(),
            8 * 1024 * 1024,
        )
        .unwrap()
        .stdout
    }
    fn generated_settings() -> CaptureSettings {
        CaptureSettings {
            microphone_id: Some("generated-tone".into()),
            ..Default::default()
        }
    }
    #[test]
    #[ignore = "Explicit generated1080p NVIDIA capture/edge/recovery; requires STORYFOLD_CAPTURE_TEST=1 and working NVENC"]
    fn generated_gpu_capture_cut_and_recovery_use_saved_encoder() {
        assert_eq!(std::env::var("STORYFOLD_CAPTURE_TEST").as_deref(), Ok("1"));
        let temp = tempfile::Builder::new()
            .prefix("storyfold-gpu-capture-")
            .tempdir()
            .unwrap();
        let (store, _) =
            ProjectStore::create(temp.path().join("project"), "Generated GPU recovery").unwrap();
        let session = CaptureSession::start_generated(&store, generated_settings()).unwrap();
        wait(&session, Duration::from_secs(20), |status| {
            status.phase == CapturePhase::Ready
        });
        assert!(
            session
                .status()
                .message
                .contains(VideoEncoder::Nvidia.label()),
            "This opt-in test requires actual GPU selection, not CPU fallback"
        );
        let id = session.status().session_id.clone();
        assert_eq!(
            Manifest::load(store.root(), &id).unwrap().encoder,
            VideoEncoder::Nvidia
        );
        session.press().unwrap();
        wait(&session, Duration::from_secs(10), |status| {
            status.active_ms >= 3200
        });
        session.send(Control::InterruptGenerated).unwrap();
        wait(&session, Duration::from_secs(20), CaptureStatus::can_leave);
        assert_eq!(session.status().phase, CapturePhase::Failed);
        drop(session);
        let manifest = Manifest::load(store.root(), &id).unwrap();
        assert_eq!(manifest.encoder, VideoEncoder::Nvidia);
        let originals: Vec<_> = fs::read_dir(store.root().join(manifest.relative()))
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .filter(|path| path.extension().is_some_and(|extension| extension == "ts"))
            .map(|path| {
                let bytes = fs::read(&path).unwrap();
                (path, bytes)
            })
            .collect();
        assert!(
            originals.len() >= 3,
            "Exercise copied interior segments and encoded edges"
        );
        let recovered = recover_session(&store, &id, &Cancellation::default()).unwrap();
        assert_eq!(recovered.len(), 1);
        let saved = Manifest::load(store.root(), &id).unwrap();
        assert_eq!(saved.encoder, VideoEncoder::Nvidia);
        let range = &saved.takes[0];
        let path = store.root().join(recovered[0].media().path());
        assert_eq!(
            count_frames(&path, &Cancellation::default()).unwrap(),
            range.end - range.start
        );
        assert!(
            audio_rms(&path, "0.2") > 0.02,
            "Generated microphone tone survives GPU recovery"
        );
        let plan = CutPlan::new(range.start, range.end).unwrap();
        assert!(plan.last > plan.first + 1);
        let interior = plan.first + 1;
        let source = store
            .root()
            .join(saved.relative())
            .join(segment_name(interior));
        assert_eq!(
            frame_bytes(&source, 0),
            frame_bytes(&path, interior * FRAME_RATE - range.start)
        );
        for (source, bytes) in originals {
            assert_eq!(
                fs::read(source).unwrap(),
                bytes,
                "Recovery preserves original GPU segments"
            );
        }
        println!(
            "Generated-only NVIDIA1080p capture/recovery: saved backend, exact frames, edge encodes/interior copies, real tone PCM and unchanged source segments; no camera/microphone opened"
        );
    }
    fn audio_rms(path: &Path, start: &str) -> f64 {
        let mut command = ffmpeg().unwrap();
        command
            .args(["-protocol_whitelist", "file,pipe", "-ss", start, "-i"])
            .arg(path)
            .args([
                "-t", "0.2", "-map", "0:a:0", "-vn", "-ac", "1", "-ar", "48000", "-f", "f32le",
                "pipe:1",
            ]);
        let output = run_bounded(
            command.as_inner_mut(),
            Duration::from_secs(15),
            &Cancellation::default(),
            128 * 1024,
        )
        .unwrap()
        .stdout;
        let samples = output.as_chunks::<4>().0;
        assert!(
            samples.len() > 9000,
            "Decode actual PCM, not stream metadata"
        );
        let energy = samples
            .iter()
            .map(|sample| f64::from(f32::from_le_bytes(*sample)).powi(2))
            .sum::<f64>();
        (energy / samples.len() as f64).sqrt()
    }
    #[test]
    #[ignore = "Generated microphone tone/off through take cuts and trimmed export; STORYFOLD_CAPTURE_TEST=1"]
    fn generated_microphone_signal_survives_take_trim_join_and_export() {
        assert_eq!(std::env::var("STORYFOLD_CAPTURE_TEST").as_deref(), Ok("1"));
        let temp = tempfile::Builder::new()
            .prefix("storyfold-capture-audio-")
            .tempdir()
            .unwrap();
        for microphone_on in [false, true] {
            let (mut store, studio) = ProjectStore::create(
                temp.path().join(if microphone_on { "tone" } else { "off" }),
                "Generated microphone signal",
            )
            .unwrap();
            let settings = CaptureSettings {
                microphone_id: microphone_on.then(|| "generated-tone".into()),
                mode: CameraMode {
                    width: 640,
                    height: 360,
                    ..Default::default()
                },
                ..Default::default()
            };
            let session = CaptureSession::start_generated(&store, settings).unwrap();
            wait(&session, Duration::from_secs(15), |status| {
                status.phase == CapturePhase::Ready
            });
            session.press().unwrap();
            wait(&session, Duration::from_secs(10), |status| {
                status.phase == CapturePhase::Holding && status.active_ms >= 1333
            });
            session.release().unwrap();
            wait(&session, Duration::from_secs(2), |status| {
                status.phase == CapturePhase::Ready
            });
            session.stop().unwrap();
            wait(&session, Duration::from_secs(30), CaptureStatus::can_leave);
            let takes: Vec<_> = session
                .poll_events()
                .into_iter()
                .filter_map(|event| match event {
                    CaptureEvent::TakeCompleted(take) => Some(take),
                    _ => None,
                })
                .collect();
            assert_eq!(takes.len(), 1, "{:?}", session.status());
            let take = &takes[0];
            let path = store.root().join(take.media().path());
            let original = fs::read(&path).unwrap();
            let take_rms = audio_rms(&path, "0.2");
            let mut editor = Editor::new(studio).unwrap();
            let first = take.append_to(&store, &mut editor).unwrap();
            editor
                .apply(Edit::TrimClip {
                    clip: first,
                    in_ms: 100,
                    out_ms: 500,
                })
                .unwrap();
            editor
                .apply(Edit::AddClip {
                    source: editor.studio().media()[0].id(),
                    name: "Later trimmed audio".into(),
                    in_ms: 700,
                    out_ms: 1100,
                })
                .unwrap();
            let outcome = crate::export::export_sequence(
                &mut store,
                editor.studio(),
                crate::export::ExportOptions::default(),
                &Cancellation::default(),
                |_| {},
            )
            .unwrap();
            assert_eq!(outcome.artifact.frames, 24);
            let export = outcome.artifact.path(&store).unwrap();
            let exported_rms = [audio_rms(&export, "0.1"), audio_rms(&export, "0.5")];
            for (stage, rms) in [
                ("prepared take", take_rms),
                ("first trimmed clip", exported_rms[0]),
                ("second joined clip", exported_rms[1]),
            ] {
                if microphone_on {
                    assert!(
                        rms > 0.02,
                        "Generated microphone signal lost at {stage}: {rms}"
                    );
                } else {
                    assert!(
                        rms < 0.001,
                        "Microphone Off must stay silent at {stage}: {rms}"
                    );
                }
            }
            assert_eq!(
                fs::read(&path).unwrap(),
                original,
                "Export must preserve the take"
            );
            println!(
                "{}",
                serde_json::json!({
                    "generated_only": true,
                    "physical_devices_opened": false,
                    "microphone_on": microphone_on,
                    "take_rms": take_rms,
                    "trimmed_export_rms": exported_rms,
                    "frames": outcome.artifact.frames,
                    "original_take_unchanged": true,
                })
            );
        }
    }
    #[test]
    fn generated_source_is_not_enabled_by_feature_alone() {
        if std::env::var("STORYFOLD_CAPTURE_TEST").as_deref() == Ok("1") {
            return;
        }
        let temp = tempfile::tempdir().unwrap();
        let (store, _) = ProjectStore::create(temp.path().join("project"), "Test gate").unwrap();
        assert!(CaptureSession::start_generated(&store, generated_settings()).is_err());
    }
    #[test]
    #[ignore = "Explicit generated-source FFmpeg integration; requires STORYFOLD_CAPTURE_TEST=1"]
    fn generated_continuous_capture_cut_import_and_recovery() {
        assert_eq!(std::env::var("STORYFOLD_CAPTURE_TEST").as_deref(), Ok("1"));
        let temp = tempfile::Builder::new()
            .prefix("storyfold-generated-capture-")
            .tempdir()
            .unwrap();
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let (store, studio) =
                ProjectStore::create(temp.path().join("project"), "Generated capture").unwrap();
            let session = CaptureSession::start_generated(&store, generated_settings()).unwrap();
            wait(&session, Duration::from_secs(15), |status| {
                status.phase == CapturePhase::Ready
            });
            assert!(
                recover_session(
                    &store,
                    &session.status().session_id,
                    &Cancellation::default()
                )
                .is_err(),
                "A live capture lock must prevent recovery"
            );
            let first = session.latest_preview().unwrap();
            assert_eq!(
                (first.width, first.height, first.rgba.len()),
                (1280, 720, 1280 * 720 * 4)
            );
            thread::sleep(Duration::from_millis(180));
            let next = session.latest_preview().unwrap();
            assert!(next.sequence > first.sequence);
            assert_ne!(first.rgba, next.rgba);
            session.press().unwrap();
            session
                .mark_slot(SlotKey::try_from(2).unwrap(), content(), 5000)
                .unwrap();
            thread::sleep(Duration::from_millis(450));
            session.release().unwrap();
            wait(&session, Duration::from_secs(2), |status| {
                status.phase == CapturePhase::Ready
            });
            session.press().unwrap();
            thread::sleep(Duration::from_millis(30));
            session.release().unwrap();
            wait(&session, Duration::from_secs(2), |status| {
                status.phase == CapturePhase::LongTake
            });
            thread::sleep(Duration::from_millis(3400));
            session.press().unwrap();
            thread::sleep(Duration::from_millis(20));
            session.release().unwrap();
            session.stop().unwrap();
            wait(&session, Duration::from_secs(60), CaptureStatus::can_leave);
            let events = session.poll_events();
            let mut takes: Vec<_> = events
                .iter()
                .filter_map(|event| match event {
                    CaptureEvent::TakeCompleted(take) => Some(take.clone()),
                    _ => None,
                })
                .collect();
            takes.sort_by_key(CompletedTake::ordinal);
            assert_eq!(takes.len(), 2, "{events:?}; {:?}", session.status());
            assert_eq!((takes[0].ordinal(), takes[1].ordinal()), (1, 2));
            let id = session.status().session_id.clone();
            drop(session);
            let manifest = Manifest::load(store.root(), &id).unwrap();
            for take in &takes {
                let source = store.root().join(take.media().path());
                let range = manifest
                    .takes
                    .iter()
                    .find(|range| range.ordinal == take.ordinal())
                    .unwrap();
                assert_eq!(
                    count_frames(&source, &Cancellation::default()).unwrap(),
                    range.end - range.start,
                    "Published take must have exactly the selected frame count"
                );
                assert_eq!(
                    (
                        take.media().info().width,
                        take.media().info().height,
                        take.media().info().has_audio
                    ),
                    (1920, 1080, true)
                );
                let metadata = run_bounded(
                    std::process::Command::new(media::tool("ffprobe").unwrap())
                        .args([
                            "-v",
                            "error",
                            "-show_entries",
                            "stream=codec_type,codec_name,duration",
                            "-of",
                            "json",
                        ])
                        .arg(&source),
                    Duration::from_secs(10),
                    &Cancellation::default(),
                    65_536,
                )
                .unwrap();
                let metadata: serde_json::Value = serde_json::from_slice(&metadata.stdout).unwrap();
                let streams = metadata["streams"].as_array().unwrap();
                let video = streams
                    .iter()
                    .find(|stream| stream["codec_type"] == "video")
                    .unwrap();
                let audio = streams
                    .iter()
                    .find(|stream| stream["codec_type"] == "audio")
                    .unwrap();
                assert_eq!(video["codec_name"], "h264");
                assert_eq!(audio["codec_name"], "aac");
                let video_duration = video["duration"].as_str().unwrap().parse::<f64>().unwrap();
                let audio_duration = audio["duration"].as_str().unwrap().parse::<f64>().unwrap();
                assert!(
                    (video_duration - audio_duration).abs() <= 1. / 30.,
                    "Generated take track durations must stay within one frame"
                );
            }
            let range = manifest
                .takes
                .iter()
                .find(|take| take.ordinal == 2)
                .unwrap();
            let plan = CutPlan::new(range.start, range.end).unwrap();
            assert!(
                plan.last > plan.first + 1,
                "Long take must exercise copied interior video"
            );
            let interior = plan.first + 1;
            let original = store
                .root()
                .join(manifest.relative())
                .join(segment_name(interior));
            let published = store.root().join(takes[1].media().path());
            assert_eq!(
                frame_bytes(&original, 0),
                frame_bytes(&published, interior * 30 - range.start),
                "Copied interior video must decode identically"
            );
            let mut editor = Editor::new(studio).unwrap();
            takes[0].append_to(&store, &mut editor).unwrap();
            assert_eq!(editor.undo_len(), 1);
            assert_eq!(editor.studio().cues().len(), 1);
            assert_eq!(editor.studio().cues()[0].content(), Some(&content()));
            assert!(editor.studio().cues()[0].duration_ms() <= takes[0].media().info().duration_ms);
            editor.undo();
            assert!(editor.studio().clips().is_empty());
            assert!(store.root().join(takes[0].media().path()).exists());
            let source = store.root().join(takes[1].media().path());
            let before = fs::read(&source).unwrap();
            let imported = media::import_media(&store, &source, &Cancellation::default()).unwrap();
            assert_ne!(imported.path(), takes[1].media().path());
            assert_eq!(fs::read(&source).unwrap(), before);
            imported.append_to(&store, &mut editor).unwrap();
            assert_eq!(editor.studio().clips().len(), 1);
            let cancelled = Cancellation::default();
            cancelled.cancel();
            assert!(media::import_media(&store, &source, &cancelled).is_err());
            let recovered = recover_session(&store, &id, &Cancellation::default()).unwrap();
            assert_eq!(recovered.len(), 2);
            assert!(
                list_recoverable(&store)
                    .unwrap()
                    .iter()
                    .any(|entry| entry.id == id)
            );
        }));
        if result.is_err() {
            let path = temp.keep();
            panic!(
                "Generated capture failed; artifacts retained at {}",
                path.display()
            );
        }
    }
    #[test]
    #[ignore = "Explicit generated-source interruption/recovery; requires STORYFOLD_CAPTURE_TEST=1"]
    fn generated_unplug_retains_a_partial_take_for_explicit_recovery() {
        assert_eq!(std::env::var("STORYFOLD_CAPTURE_TEST").as_deref(), Ok("1"));
        let temp = tempfile::Builder::new()
            .prefix("storyfold-generated-recovery-")
            .tempdir()
            .unwrap();
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let (store, _) =
                ProjectStore::create(temp.path().join("project"), "Interrupted capture").unwrap();
            let session = CaptureSession::start_generated(&store, generated_settings()).unwrap();
            wait(&session, Duration::from_secs(15), |status| {
                status.phase == CapturePhase::Ready
            });
            session.press().unwrap();
            thread::sleep(Duration::from_millis(30));
            session.release().unwrap();
            wait(&session, Duration::from_secs(2), |status| {
                status.phase == CapturePhase::LongTake
            });
            thread::sleep(Duration::from_millis(1600));
            session.send(Control::InterruptGenerated).unwrap();
            wait(&session, Duration::from_secs(20), CaptureStatus::can_leave);
            assert_eq!(session.status().phase, CapturePhase::Failed);
            let id = session.status().session_id.clone();
            drop(session);
            assert!(
                list_recoverable(&store)
                    .unwrap()
                    .iter()
                    .any(|entry| entry.id == id && entry.pending_takes == 1)
            );
            let recovered = recover_session(&store, &id, &Cancellation::default()).unwrap();
            assert_eq!(recovered.len(), 1);
            assert!(recovered[0].media().info().duration_ms > 500);
        }));
        if result.is_err() {
            let path = temp.keep();
            panic!(
                "Generated recovery failed; artifacts retained at {}",
                path.display()
            );
        }
    }
}
