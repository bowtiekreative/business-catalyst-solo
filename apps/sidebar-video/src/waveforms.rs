//! One cancellable local decoder and a bounded source cache; paint only reads it.
use crate::Workspace;
use eframe::egui;
use std::{
    cell::{Cell, RefCell},
    collections::{BTreeMap, BTreeSet},
    path::PathBuf,
    sync::mpsc,
    time::Duration,
};
use video_core::{
    MediaId, MediaSource, ProjectStore,
    media::Cancellation,
    waveform::{self, Analysis, Envelope},
};

const CACHE_BYTES: usize = 16 * 1024 * 1024;

pub enum WaveformState<'a> {
    Waiting,
    Loading,
    Ready(&'a Envelope),
    Silent,
    NoAudio,
    Error(&'a str),
}
impl WaveformState<'_> {
    pub fn label(&self) -> &'static str {
        match self {
            Self::Waiting => "Waveform queued",
            Self::Loading => "Loading waveform…",
            Self::Ready(_) => "Audio waveform",
            Self::Silent => "Silent audio",
            Self::NoAudio => "No audio track",
            Self::Error(_) => "Waveform error",
        }
    }
}
struct Entry {
    source: MediaSource,
    analysis: Result<Analysis, String>,
}
struct Pending {
    root: PathBuf,
    source: MediaSource,
    bins: usize,
    cancel: Cancellation,
    receiver: mpsc::Receiver<Result<Analysis, String>>,
}
pub struct WaveformsUi {
    root: Option<PathBuf>,
    bins: usize,
    cache: BTreeMap<MediaId, Entry>,
    requested: RefCell<BTreeSet<MediaId>>,
    retry_requested: Cell<bool>,
    pending: Option<Pending>,
}
impl Default for WaveformsUi {
    fn default() -> Self {
        Self {
            root: None,
            bins: waveform::MAX_BINS,
            cache: BTreeMap::new(),
            requested: RefCell::default(),
            retry_requested: Cell::new(false),
            pending: None,
        }
    }
}
impl Drop for WaveformsUi {
    fn drop(&mut self) {
        if let Some(pending) = &self.pending {
            pending.cancel.cancel();
        }
    }
}
impl WaveformsUi {
    /// Merely records current visible demand; no IO, channel send or decoding.
    pub fn state(&self, id: MediaId) -> WaveformState<'_> {
        self.requested.borrow_mut().insert(id);
        if let Some(entry) = self.cache.get(&id) {
            match &entry.analysis {
                Ok(Analysis::NoAudio) => WaveformState::NoAudio,
                Ok(Analysis::Audio(envelope)) if envelope.is_silent() => WaveformState::Silent,
                Ok(Analysis::Audio(envelope)) => WaveformState::Ready(envelope),
                Err(error) => WaveformState::Error(error),
            }
        } else if self
            .pending
            .as_ref()
            .is_some_and(|p| p.source.id() == id && Some(&p.root) == self.root.as_ref())
        {
            WaveformState::Loading
        } else {
            WaveformState::Waiting
        }
    }
    pub fn retry(&self) {
        self.retry_requested.set(true);
    }
    /// Visible demand is collected after tick. Wake once so a still window can
    /// start it; waiting workers and unattached projects must not spin at idle.
    pub fn needs_wake(&self) -> bool {
        self.root.is_some()
            && self.pending.is_none()
            && self
                .requested
                .borrow()
                .iter()
                .any(|id| !self.cache.contains_key(id))
    }

    pub fn tick(&mut self, project: &Workspace, ctx: &egui::Context) {
        let desired = self.requested.take();
        let root = (!project.loading())
            .then(|| project.folder.clone())
            .flatten();
        let sources: BTreeSet<_> = project
            .studio()
            .clips()
            .iter()
            .map(|c| c.source_id())
            .collect();
        let current: BTreeMap<_, _> = project
            .studio()
            .media()
            .iter()
            .filter(|source| sources.contains(&source.id()))
            .map(|source| (source.id(), source))
            .collect();
        // All active source envelopes fit together, even at the10,000-source
        // model limit; density adapts instead of capping interview duration.
        let bins = (CACHE_BYTES / size_of::<f32>() / sources.len().max(1)).min(waveform::MAX_BINS);
        if self.root != root || self.bins != bins {
            self.cache.clear();
            self.root = root.clone();
            self.bins = bins;
        }
        self.cache.retain(|id, entry| {
            current
                .get(id)
                .is_some_and(|source| **source == entry.source)
        });
        if self.retry_requested.take() {
            self.cache.retain(|_, entry| entry.analysis.is_ok());
        }
        if let Some(pending) = &self.pending
            && (Some(&pending.root) != root.as_ref()
                || pending.bins != bins
                || !desired.contains(&pending.source.id())
                || !sources.contains(&pending.source.id())
                || current
                    .get(&pending.source.id())
                    .is_none_or(|source| **source != pending.source))
        {
            pending.cancel.cancel();
        }
        self.accept_ready(project, ctx);
        if self.pending.is_none()
            && let Some(root) = root
            && let Some(source) = desired
                .iter()
                .filter(|id| sources.contains(id) && !self.cache.contains_key(id))
                .find_map(|id| current.get(id).copied())
                .cloned()
        {
            let (sender, receiver) = mpsc::sync_channel(1);
            let cancel = Cancellation::default();
            let worker_cancel = cancel.clone();
            let worker_root = root.clone();
            let worker_source = source.clone();
            let wake = ctx.clone();
            match std::thread::Builder::new()
                .name("audio-waveform".into())
                .spawn(move || {
                    let result = (|| {
                        if worker_cancel.is_cancelled() {
                            return Err("Waveform preparation cancelled".into());
                        }
                        // A separate read-only handle avoids keeping the UI's save
                        // mutex locked during a long source decode.
                        let (store, _) = ProjectStore::open(worker_root)?;
                        waveform::extract_with_bins(&store, &worker_source, &worker_cancel, bins)
                    })()
                    .map_err(|error: Box<dyn std::error::Error + Send + Sync>| error.to_string());
                    let _ = sender.send(result);
                    wake.request_repaint();
                }) {
                Ok(_) => {
                    self.pending = Some(Pending {
                        root,
                        source,
                        bins,
                        cancel,
                        receiver,
                    })
                }
                Err(_) => {
                    self.cache.insert(
                        source.id(),
                        Entry {
                            source,
                            analysis: Err(
                                "Waveform worker could not start. Right-click the clip to retry."
                                    .into(),
                            ),
                        },
                    );
                }
            }
        }
        if self.pending.is_some() {
            ctx.request_repaint_after(Duration::from_millis(50));
        }
    }
    fn accept_ready(&mut self, project: &Workspace, ctx: &egui::Context) {
        let result = self
            .pending
            .as_ref()
            .and_then(|pending| match pending.receiver.try_recv() {
                Ok(result) => Some(result),
                Err(mpsc::TryRecvError::Disconnected) => Some(Err(
                    "Waveform worker stopped. Right-click the clip to retry.".into(),
                )),
                Err(mpsc::TryRecvError::Empty) => None,
            });
        if let Some(result) = result {
            let pending = self.pending.take().unwrap();
            if !pending.cancel.is_cancelled()
                && Some(&pending.root) == self.root.as_ref()
                && pending.bins == self.bins
                && project
                    .studio()
                    .media()
                    .iter()
                    .any(|source| source == &pending.source)
            {
                let result = result.map_err(|error| error.chars().take(240).collect());
                self.cache.insert(
                    pending.source.id(),
                    Entry {
                        source: pending.source,
                        analysis: result,
                    },
                );
            }
            ctx.request_repaint();
        }
    }
    #[cfg(test)]
    pub(crate) fn test_ready(&mut self, source: MediaSource, analysis: Analysis) {
        self.cache.insert(
            source.id(),
            Entry {
                source,
                analysis: Ok(analysis),
            },
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use video_core::Edit;
    fn project() -> Workspace {
        let mut project = Workspace::default();
        let source = project.studio().next_media_id();
        project.edit(Edit::AddMedia {
            path: "media/generated.mp4".into(),
            duration_ms: 2000,
        });
        project.edit(Edit::AddClip {
            source,
            name: "Generated".into(),
            in_ms: 0,
            out_ms: 2000,
        });
        project
    }
    #[test]
    fn silence_no_track_error_and_loading_are_distinct_without_io() {
        let project = project();
        let source = project.studio().media()[0].clone();
        let id = source.id();
        let mut ui = WaveformsUi::default();
        assert_eq!(ui.state(id).label(), "Waveform queued");
        ui.test_ready(
            source.clone(),
            Analysis::Audio(Envelope::from_peaks(2000, vec![0.]).unwrap()),
        );
        assert_eq!(ui.state(id).label(), "Silent audio");
        ui.test_ready(source.clone(), Analysis::NoAudio);
        assert_eq!(ui.state(id).label(), "No audio track");
        ui.cache.insert(
            id,
            Entry {
                source: source.clone(),
                analysis: Err("A decoder error".into()),
            },
        );
        assert_eq!(ui.state(id).label(), "Waveform error");
        let root = PathBuf::from("/generated-fixture");
        ui.root = Some(root.clone());
        ui.cache.clear();
        let (_sender, receiver) = mpsc::sync_channel(1);
        ui.pending = Some(Pending {
            root,
            source,
            bins: ui.bins,
            cancel: Cancellation::default(),
            receiver,
        });
        assert_eq!(ui.state(id).label(), "Loading waveform…");
    }
    #[test]
    fn changed_project_cancels_old_worker_and_discards_its_late_result() {
        let project = project();
        let source = project.studio().media()[0].clone();
        let mut ui = WaveformsUi::default();
        let (sender, receiver) = mpsc::sync_channel(1);
        let cancel = Cancellation::default();
        ui.root = Some(PathBuf::from("/old-project"));
        ui.pending = Some(Pending {
            root: ui.root.clone().unwrap(),
            source,
            bins: ui.bins,
            cancel: cancel.clone(),
            receiver,
        });
        ui.tick(&project, &egui::Context::default());
        assert!(cancel.is_cancelled());
        assert!(
            ui.pending.is_some(),
            "Old worker must drain before another decoder starts"
        );
        sender.send(Ok(Analysis::NoAudio)).unwrap();
        ui.tick(&project, &egui::Context::default());
        assert!(ui.pending.is_none());
        assert!(ui.cache.is_empty());
    }
    #[test]
    fn queued_demand_wakes_idle_attached_window_without_idle_or_error_loop() {
        let project = project();
        let source = project.studio().media()[0].clone();
        let id = source.id();
        let mut ui = WaveformsUi::default();
        ui.state(id);
        assert!(!ui.needs_wake(), "No decode can start without a project");
        ui.root = Some(PathBuf::from("/generated-project"));
        assert!(ui.needs_wake(), "Paint-time demand needs a follow-up tick");
        ui.test_ready(source.clone(), Analysis::NoAudio);
        assert!(!ui.needs_wake());
        ui.cache.insert(
            id,
            Entry {
                source,
                analysis: Err("Decoder unavailable".into()),
            },
        );
        assert!(!ui.needs_wake(), "An error must wait for explicit retry");
    }
    #[test]
    fn failures_stay_visible_until_explicit_retry_and_metadata_changes_invalidate() {
        let project = project();
        let source = project.studio().media()[0].clone();
        let id = source.id();
        let mut ui = WaveformsUi::default();
        ui.cache.insert(
            id,
            Entry {
                source,
                analysis: Err("Unavailable decoder".into()),
            },
        );
        ui.state(id);
        ui.tick(&project, &egui::Context::default());
        assert!(matches!(ui.state(id), WaveformState::Error(_)));
        ui.retry();
        ui.tick(&project, &egui::Context::default());
        assert!(matches!(ui.state(id), WaveformState::Waiting));
        ui.test_ready(project.studio().media()[0].clone(), Analysis::NoAudio);
        let mut replacement = Workspace::default();
        replacement.edit(Edit::AddMedia {
            path: "media/different.mp4".into(),
            duration_ms: 2000,
        });
        replacement.edit(Edit::AddClip {
            source: id,
            name: "Different".into(),
            in_ms: 0,
            out_ms: 2000,
        });
        ui.tick(&replacement, &egui::Context::default());
        assert!(ui.cache.is_empty());
    }
    #[test]
    fn adaptive_cache_budget_handles_model_limit_without_duration_cap() {
        for count in [1usize, 65, video_core::MAX_MEDIA] {
            let bins = (CACHE_BYTES / size_of::<f32>() / count).min(waveform::MAX_BINS);
            assert!(bins > 0);
            assert!(bins * size_of::<f32>() * count <= CACHE_BYTES);
        }
    }

    #[cfg(all(feature = "capture-test", target_os = "linux"))]
    #[test]
    #[ignore = "Generated local tone/worker/timeline integration; STORYFOLD_WAVEFORM_TEST=1, no hardware or speakers"]
    fn generated_tone_worker_reaches_ready_from_idle_timeline_demand() {
        use std::{
            fs,
            process::{Command, Stdio},
            time::Instant,
        };
        assert_eq!(std::env::var("STORYFOLD_WAVEFORM_TEST").as_deref(), Ok("1"));
        let temp = tempfile::tempdir().unwrap();
        let mut project = Workspace::test_project(&temp.path().join("project"));
        let folder = project.folder.clone().unwrap();
        let source_path = folder.join("media/tone.mp4");
        let mut generator = Command::new("/usr/bin/ffmpeg")
            .args([
                "-hide_banner",
                "-loglevel",
                "error",
                "-nostdin",
                "-f",
                "lavfi",
                "-i",
                "color=c=black:s=160x90:r=30:d=1",
                "-f",
                "lavfi",
                "-i",
                "sine=frequency=440:sample_rate=8000:duration=1",
                "-t",
                "1",
                "-c:v",
                "libx264",
                "-threads",
                "1",
                "-preset",
                "ultrafast",
                "-pix_fmt",
                "yuv420p",
                "-c:a",
                "aac",
            ])
            .arg(&source_path)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            if let Some(status) = generator.try_wait().unwrap() {
                assert!(status.success(), "Generated tone fixture failed");
                break;
            }
            if Instant::now() >= deadline {
                let _ = generator.kill();
                let _ = generator.wait();
                panic!("Generated tone fixture exceeded10s");
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        let source = project.studio().next_media_id();
        project.edit(Edit::AddMedia {
            path: "media/tone.mp4".into(),
            duration_ms: 1000,
        });
        project.edit(Edit::AddClip {
            source,
            name: "Generated tone".into(),
            in_ms: 0,
            out_ms: 1000,
        });
        project
            .store()
            .unwrap()
            .lock()
            .unwrap()
            .save(project.studio())
            .unwrap();
        let snapshot = project.studio().clone();
        let json_before = fs::read(folder.join("project.json")).unwrap();
        let media_before = fs::read(&source_path).unwrap();
        let ctx = egui::Context::default();
        crate::configure(&ctx);
        let mut waveforms = WaveformsUi::default();
        let mut timeline = crate::timeline::TimelineUi::default();
        let selected = project.studio().clips()[0].id();
        let mut paint = |waveforms: &mut WaveformsUi| {
            ctx.run(
                egui::RawInput {
                    focused: true,
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(1160., 400.),
                    )),
                    ..Default::default()
                },
                |ctx| {
                    waveforms.tick(&project, ctx);
                    egui::CentralPanel::default().show(ctx, |ui| {
                        let response =
                            timeline.show(ui, project.studio(), Some(selected), 0, true, waveforms);
                        assert!(
                            response.edit.is_none(),
                            "Analysis must never edit the timeline"
                        );
                    });
                },
            )
        };
        // No input events or active camera: paint itself must ask for a tick.
        let first = paint(&mut waveforms);
        assert!(waveforms.pending.is_none());
        assert!(waveforms.needs_wake());
        assert_eq!(
            first.viewport_output[&egui::ViewportId::ROOT].repaint_delay,
            Duration::ZERO
        );
        let deadline = Instant::now() + Duration::from_secs(15);
        let ready_frame = loop {
            let frame = paint(&mut waveforms);
            match waveforms.state(source) {
                WaveformState::Ready(envelope) => {
                    assert!(
                        envelope.peak(100, 900) > 0.05,
                        "The cache must contain real tone peaks"
                    );
                    assert_eq!(envelope.duration_ms(), 1000);
                    break frame;
                }
                WaveformState::Error(error) => panic!("Real waveform worker failed: {error}"),
                _ => {}
            }
            assert!(
                Instant::now() < deadline,
                "Real waveform worker exceeded15s"
            );
            std::thread::sleep(Duration::from_millis(10));
        };
        assert!(waveforms.pending.is_none());
        assert!(ready_frame.shapes.iter().any(|shape| matches!(&shape.shape,
            egui::Shape::LineSegment { stroke, .. } if stroke.color == egui::Color32::from_rgb(238, 229, 255)
        )), "Ready cache must be drawn by the actual timeline");
        paint(&mut waveforms);
        assert!(
            waveforms.pending.is_none(),
            "A cached source must not restart decoding"
        );
        assert_eq!(project.studio(), &snapshot);
        assert_eq!(fs::read(folder.join("project.json")).unwrap(), json_before);
        assert_eq!(fs::read(source_path).unwrap(), media_before);
    }
}
