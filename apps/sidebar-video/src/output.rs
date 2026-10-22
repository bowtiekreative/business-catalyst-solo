//! Direct timeline preview and explicit MP4 export. Submitted snapshots and save
//! ACKs never replace newer drafts; export progress is actual encoded time.
use crate::{Workspace, capture::CaptureUi};
use eframe::egui;
use std::{
    sync::{Arc, Mutex, mpsc},
    time::Duration,
};
use video_core::{
    Studio,
    export::{
        self, ExportArtifact, ExportOptions, ExportOutcome, ExportPlan, ExportProgress,
        ExportWarning,
    },
    media::Cancellation,
    playback::SequencePlayer,
};

#[derive(Clone, Copy)]
pub enum Action {
    Preview,
    Export,
}
#[derive(Debug, PartialEq, Eq)]
enum Dispatch {
    Mp4Export,
    Player,
}
fn dispatch(action: Action) -> Dispatch {
    if matches!(action, Action::Preview) && cfg!(target_os = "linux") {
        Dispatch::Player
    } else {
        Dispatch::Mp4Export
    }
}
struct Built {
    snapshot: Studio,
    action: Action,
    reduced_motion: bool,
    result: Result<ExportOutcome, String>,
}
struct Pending {
    receiver: mpsc::Receiver<Built>,
    progress: Arc<Mutex<Option<ExportProgress>>>,
    cancel: Cancellation,
}
struct Opening {
    receiver: mpsc::Receiver<Result<SequencePlayer, String>>,
    snapshot: Studio,
    reduced_motion: bool,
    prepared_mp4: bool,
}
impl Opening {
    fn is_current(&self, project: &Workspace, reduced_motion: bool) -> bool {
        project.studio() == &self.snapshot && reduced_motion == self.reduced_motion
    }
}
#[derive(Default)]
pub struct OutputUi {
    waiting_for_camera: Option<(Action, bool)>,
    pending: Option<Pending>,
    player_pending: Option<Opening>,
    folder_pending: Option<mpsc::Receiver<Result<(), String>>>,
    player: Option<SequencePlayer>,
    cached: Option<(Studio, bool, ExportArtifact)>,
    progress: Option<ExportProgress>,
    pub status: String,
    pub error: Option<String>,
    notice: bool,
    pub muted: bool,
}
impl OutputUi {
    #[cfg(feature = "validation-screenshot")]
    pub fn probe_preview_state(&self) -> (bool, bool) {
        (self.player.is_some(), self.pending.is_some())
    }
    pub fn busy(&self) -> bool {
        self.waiting_for_camera.is_some() || self.pending.is_some() || self.player_pending.is_some()
    }
    pub fn request_from_button(
        &mut self,
        action: Action,
        project: &mut Workspace,
        capture: &mut CaptureUi,
        ctx: &egui::Context,
        reduced_motion: bool,
    ) {
        if self.busy() || project.busy() {
            return;
        }
        if capture.can_leave() {
            self.request(action, project, capture, ctx, reduced_motion);
        } else {
            self.stop_preview();
            self.error = None;
            self.progress = None;
            self.status = match action {
                Action::Preview => {
                    "Stopping the camera and saving your take before opening preview…"
                }
                Action::Export => "Stopping the camera and saving your take before exporting…",
            }
            .into();
            self.waiting_for_camera = Some((action, reduced_motion));
            capture.stop();
            ctx.request_repaint();
        }
    }
    /// Capture autosaves finish before preview/export takes its immutable snapshot.
    pub fn after_capture(
        &mut self,
        project: &mut Workspace,
        capture: &mut CaptureUi,
        ctx: &egui::Context,
    ) {
        if self.waiting_for_camera.is_some() && capture.camera_running() {
            // Also catches a camera-start request that completed after the click.
            capture.stop();
        }
        if self.waiting_for_camera.is_some() && capture.can_leave() && !project.busy() {
            let (action, reduced_motion) = self.waiting_for_camera.take().unwrap();
            self.request(action, project, capture, ctx, reduced_motion);
        }
    }
    pub fn request(
        &mut self,
        action: Action,
        project: &mut Workspace,
        capture: &CaptureUi,
        ctx: &egui::Context,
        reduced_motion: bool,
    ) {
        if self.busy() || project.busy() {
            return;
        }
        if !capture.can_leave() {
            self.error = Some(
                "Stop the camera and wait for takes to finish before preparing the sequence."
                    .into(),
            );
            return;
        }
        let Some(store) = project.store() else {
            self.error = Some("Create or save this project before preparing its sequence.".into());
            return;
        };
        let snapshot = project.studio().clone();
        if dispatch(action) == Dispatch::Player {
            self.play_sequence(project, snapshot, ctx, reduced_motion, None);
            return;
        }
        #[cfg(not(target_os = "linux"))]
        if matches!(action, Action::Preview)
            && let Some((saved, motion, artifact)) = &self.cached
            && saved == &snapshot
            && *motion == reduced_motion
        {
            self.play_sequence(
                project,
                snapshot,
                ctx,
                reduced_motion,
                Some(artifact.clone()),
            );
            return;
        }
        let plan = match ExportPlan::new(&snapshot) {
            Ok(plan) => plan,
            Err(e) => {
                self.error = Some(e.to_string());
                return;
            }
        };
        self.notice = plan
            .warnings
            .contains(&ExportWarning::VerticalLongerThanThreeMinutes);
        self.error = None;
        self.progress = None;
        self.status = match action {
            Action::Preview => "Preparing a rendered compatibility preview…",
            Action::Export => "Preparing MP4 export…",
        }
        .into();
        self.stop_preview();
        let (receiver, progress, cancel) = {
            let (sender, receiver) = mpsc::channel();
            let progress = Arc::new(Mutex::new(None));
            let latest = progress.clone();
            let cancel = Cancellation::default();
            let token = cancel.clone();
            let ctx = ctx.clone();
            let result = std::thread::Builder::new()
                .name("storyfold-sequence-export".into())
                .spawn(move || {
                    let result = store
                        .lock()
                        .map_err(|_| "Project worker failed".to_owned())
                        .and_then(|mut store| {
                            export::export_sequence(
                                &mut store,
                                &snapshot,
                                ExportOptions {
                                    file_name: None,
                                    reduced_motion,
                                },
                                &token,
                                |update| {
                                    if let Ok(mut latest) = latest.lock() {
                                        *latest = Some(update);
                                    }
                                    ctx.request_repaint();
                                },
                            )
                            .map_err(|e| e.to_string())
                        });
                    let _ = sender.send(Built {
                        snapshot,
                        action,
                        reduced_motion,
                        result,
                    });
                    ctx.request_repaint();
                });
            if let Err(e) = result {
                self.error = Some(format!("Could not start export worker: {e}"));
                return;
            }
            (receiver, progress, cancel)
        };
        self.pending = Some(Pending {
            receiver,
            progress,
            cancel,
        });
        project.set_output_busy(true);
    }
    fn play_sequence(
        &mut self,
        project: &mut Workspace,
        snapshot: Studio,
        ctx: &egui::Context,
        reduced_motion: bool,
        artifact: Option<ExportArtifact>,
    ) {
        let Some(store) = project.store() else {
            return;
        };
        self.stop_preview();
        self.error = None;
        self.progress = None;
        self.notice = false;
        let prepared_mp4 = artifact.is_some();
        self.status = if prepared_mp4 {
            "Opening rendered compatibility preview…"
        } else {
            "Opening timeline preview directly…"
        }
        .into();
        let (sender, receiver) = mpsc::channel();
        let ctx = ctx.clone();
        let muted = self.muted;
        let submitted = snapshot.clone();
        match std::thread::Builder::new()
            .name("storyfold-sequence-player".into())
            .spawn(move || {
                let result = store
                    .lock()
                    .map_err(|_| "Project worker failed".to_owned())
                    .and_then(|store| {
                        match artifact {
                            Some(artifact) => SequencePlayer::start(&store, &artifact.relative_path, muted).map_err(|e| e.to_string()),
                            None => {
                                #[cfg(target_os = "linux")]
                                { SequencePlayer::start_timeline(&store, &snapshot, muted, reduced_motion).map_err(|e| e.to_string()) }
                                #[cfg(not(target_os = "linux"))]
                                { Err("Direct timeline preview is available on Linux; use the rendered compatibility preview on this platform.".into()) }
                            }
                        }
                    });
                let _ = sender.send(result);
                ctx.request_repaint();
            }) {
            Ok(_) => {
                self.player_pending = Some(Opening {
                    receiver,
                    snapshot: submitted,
                    reduced_motion,
                    prepared_mp4,
                });
                project.set_output_busy(true);
            }
            Err(e) => {
                self.error = Some(e.to_string());
                self.status = "Could not open timeline preview.".into();
            }
        }
    }
    pub fn stop_preview(&mut self) {
        if let Some(player) = self.player.take() {
            let _ = std::thread::spawn(move || drop(player));
        }
    }
    pub fn encoding_changed(&mut self) {
        self.cached = None;
    }
    pub fn reset(&mut self) {
        self.waiting_for_camera = None;
        self.stop_preview();
        self.cached = None;
        self.status.clear();
        self.error = None;
        self.progress = None;
        self.notice = false;
        self.folder_pending = None;
    }
    pub fn poll(&mut self, project: &mut Workspace, ctx: &egui::Context, reduced_motion: bool) {
        if let Some(pending) = &self.pending
            && let Ok(mut progress) = pending.progress.lock()
            && let Some(progress) = progress.take()
        {
            self.progress = Some(progress);
        }
        let message = self
            .pending
            .as_ref()
            .and_then(|p| match p.receiver.try_recv() {
                Ok(m) => Some(m),
                Err(mpsc::TryRecvError::Empty) => None,
                Err(mpsc::TryRecvError::Disconnected) => Some(Built {
                    snapshot: project.studio().clone(),
                    action: Action::Export,
                    reduced_motion: false,
                    result: Err(
                        "Export worker stopped. Existing media and exports remain intact.".into(),
                    ),
                }),
            });
        if let Some(message) = message {
            self.pending = None;
            project.set_output_busy(false);
            match message.result {
                Ok(outcome) => {
                    if outcome.project_saved {
                        project.acknowledge_snapshot(message.snapshot.clone());
                    }
                    let current = project.studio() == &message.snapshot
                        && reduced_motion == message.reduced_motion;
                    self.status = format!("MP4 saved: {}", outcome.artifact.relative_path);
                    if !current {
                        self.status
                            .push_str(" · Your newer edits are still in the timeline.");
                    }
                    self.error = outcome.version_error.map(|e| {
                        format!(
                            "The MP4 is ready, but its local project version could not finish: {e}"
                        )
                    });
                    self.cached = Some((
                        message.snapshot.clone(),
                        message.reduced_motion,
                        outcome.artifact.clone(),
                    ));
                    if matches!(message.action, Action::Preview) {
                        if current {
                            let warning = self.error.clone();
                            self.play_sequence(
                                project,
                                message.snapshot,
                                ctx,
                                message.reduced_motion,
                                Some(outcome.artifact),
                            );
                            if self.error.is_none() {
                                self.error = warning;
                            }
                        } else {
                            self.error = Some("The rendered compatibility preview uses earlier edits. Your newer timeline is intact; press Preview sequence again.".into());
                        }
                    }
                }
                Err(e) => {
                    self.error = Some(e);
                    self.status = "Sequence preparation stopped.".into();
                }
            }
        }
        let player = self
            .player_pending
            .as_ref()
            .and_then(|p| match p.receiver.try_recv() {
                Ok(m) => Some(m),
                Err(mpsc::TryRecvError::Empty) => None,
                Err(mpsc::TryRecvError::Disconnected) => {
                    Some(Err("The preview worker stopped.".into()))
                }
            });
        if let Some(player) = player {
            let opening = self.player_pending.take().unwrap();
            project.set_output_busy(false);
            match player {
                Ok(player) if opening.is_current(project, reduced_motion) => {
                    self.player = Some(player);
                    self.status = if opening.prepared_mp4 {
                        "Rendered compatibility preview opened. Use Play/Pause, seek and fullscreen."
                    } else {
                        "Timeline preview opened without making an MP4. Use Play/Pause, seek and fullscreen. Export MP4 saves a finished video."
                    }.into();
                }
                Ok(player) => {
                    let _ = std::thread::spawn(move || drop(player));
                    self.error = Some("The timeline changed while preview was opening. Your newer edits are intact; press Preview sequence again to view them.".into());
                    self.status = "Earlier preview closed.".into();
                }
                Err(e) => {
                    if opening.prepared_mp4 {
                        self.cached = None;
                    }
                    self.error = Some(e);
                    self.status = "Could not open sequence preview.".into();
                }
            }
        }
        if let Some(player) = &mut self.player {
            match player.finished() {
                Ok(None) => {}
                Ok(Some(success)) => {
                    self.stop_preview();
                    if !success {
                        self.error=Some("The native player could not continue this timeline. Source media is unchanged; review the sequence and try Preview again.".into());
                    }
                }
                Err(e) => {
                    self.error = Some(e.to_string());
                    self.stop_preview();
                }
            }
        }
        let folder = self
            .folder_pending
            .as_ref()
            .and_then(|folder| match folder.try_recv() {
                Ok(result) => Some(result),
                Err(mpsc::TryRecvError::Empty) => None,
                Err(mpsc::TryRecvError::Disconnected) => {
                    Some(Err("The file-manager worker stopped.".into()))
                }
            });
        if let Some(result) = folder {
            self.folder_pending = None;
            if let Err(error) = result {
                self.error = Some(error);
            }
        }
        if self.busy() || self.player.is_some() || self.folder_pending.is_some() {
            ctx.request_repaint_after(Duration::from_millis(100));
        }
    }
    fn open_exports(&mut self, project: &Workspace, ctx: &egui::Context) {
        let Some(store) = project.store() else {
            return;
        };
        let (sender, receiver) = mpsc::channel();
        let ctx = ctx.clone();
        match std::thread::Builder::new()
            .name("storyfold-export-folder".into())
            .spawn(move || {
                let result = store
                    .lock()
                    .map_err(|_| "Project worker failed".to_owned())
                    .and_then(|store| {
                        video_core::playback::open_export_folder(&store).map_err(|e| e.to_string())
                    });
                let _ = sender.send(result);
                ctx.request_repaint();
            }) {
            Ok(_) => self.folder_pending = Some(receiver),
            Err(e) => self.error = Some(e.to_string()),
        }
    }
    pub fn banner(&mut self, project: &Workspace, ctx: &egui::Context) {
        if self.status.is_empty() && self.error.is_none() {
            return;
        }
        egui::Area::new(egui::Id::new("sequence-progress"))
            .anchor(egui::Align2::RIGHT_BOTTOM, egui::vec2(-20., -304.))
            .order(egui::Order::Foreground)
            .show(ctx, |ui| {
                crate::card(16).show(ui, |ui| {
                    ui.set_width(292.);
                    ui.horizontal(|ui| {
                        ui.label(crate::strong("Sequence", 15.));
                        if self.busy() {
                            ui.spinner();
                        }
                    });
                    if self.notice {
                        ui.label(
                            "This vertical sequence exceeds 180 seconds and will not be a Short.",
                        );
                    }
                    ui.label(&self.status);
                    if self.waiting_for_camera.is_some() && ui.button("Cancel preparation").clicked() {
                        self.waiting_for_camera = None;
                        self.status = "Sequence preparation cancelled. Any finishing takes remain in your project.".into();
                    }
                    if self.pending.is_some() {
                        if let Some(progress) = &self.progress {
                            ui.label(&progress.message);
                            ui.add(egui::ProgressBar::new(progress.fraction).show_percentage());
                        } else {
                            ui.label("Waiting for the export worker…");
                        }
                        if ui
                            .add_enabled(
                                self.progress.as_ref().is_none_or(|p| p.cancellable),
                                egui::Button::new("Cancel export"),
                            )
                            .clicked()
                            && let Some(p) = &self.pending
                        {
                            p.cancel.cancel();
                        }
                    }
                    if let Some(error) = &self.error {
                        ui.colored_label(egui::Color32::from_rgb(160, 30, 45), error);
                    }
                    if !self.busy() {
                        ui.horizontal(|ui| {
                            if self.cached.is_some()
                                && ui
                                    .add_enabled(
                                        self.folder_pending.is_none(),
                                        egui::Button::new("Open export folder"),
                                    )
                                    .clicked()
                            {
                                self.open_exports(project, ctx);
                            }
                            if self.player.is_some() && ui.button("Stop preview").clicked() {
                                self.stop_preview();
                            }
                            if ui.button("Dismiss").clicked() {
                                self.status.clear();
                                self.error = None;
                            }
                        });
                    }
                });
            });
    }
}
impl Drop for OutputUi {
    fn drop(&mut self) {
        if let Some(p) = &self.pending {
            p.cancel.cancel();
        }
        self.stop_preview();
        if let Some(opening) = self.player_pending.take() {
            // A queued successful startup owns a native player/runtime too.
            let _ = std::thread::spawn(move || drop(opening));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use video_core::{Edit, Orientation};
    #[cfg(target_os = "linux")]
    #[test]
    fn uncached_preview_dispatch_bypasses_mp4_export_and_save() {
        assert_eq!(
            dispatch(Action::Preview),
            Dispatch::Player,
            "Immediate preview omitted: an uncached Preview still enters MP4 export/save instead of the timeline player"
        );
    }
    #[test]
    fn export_dispatch_always_uses_explicit_mp4_pipeline() {
        assert_eq!(dispatch(Action::Export), Dispatch::Mp4Export);
    }
    #[cfg(not(target_os = "linux"))]
    #[test]
    fn non_linux_preview_uses_rendered_compatibility_pipeline() {
        assert_eq!(dispatch(Action::Preview), Dispatch::Mp4Export);
    }
    fn result() -> ExportOutcome {
        ExportOutcome {
            artifact: ExportArtifact {
                relative_path: "exports/generated.mp4".into(),
                duration_ms: 1000,
                frames: 30,
                size: Orientation::Horizontal.output_size(),
            },
            project_saved: true,
            version: None,
            version_error: None,
        }
    }
    fn pending(ui: &mut OutputUi, project: &mut Workspace) -> mpsc::Sender<Built> {
        let (sender, receiver) = mpsc::channel();
        ui.pending = Some(Pending {
            receiver,
            progress: Arc::new(Mutex::new(None)),
            cancel: Cancellation::default(),
        });
        project.set_output_busy(true);
        sender
    }
    #[test]
    fn export_ack_keeps_newer_draft_and_releases_project_guard() {
        let mut ui = OutputUi::default();
        let mut project = Workspace::default();
        let snapshot = project.studio().clone();
        let sender = pending(&mut ui, &mut project);
        project.edit(Edit::Rename("Typed during export".into()));
        sender
            .send(Built {
                snapshot,
                action: Action::Export,
                reduced_motion: false,
                result: Ok(result()),
            })
            .unwrap();
        ui.poll(&mut project, &egui::Context::default(), false);
        assert_eq!(project.studio().name(), "Typed during export");
        assert!(project.dirty());
        assert!(!project.busy());
        assert!(ui.cached.is_some());
    }
    #[test]
    fn encoding_change_invalidates_preview_without_resetting_status() {
        let mut ui = OutputUi::default();
        let project = Workspace::default();
        ui.cached = Some((project.studio().clone(), false, result().artifact));
        ui.status = "Preview is open".into();
        ui.encoding_changed();
        assert!(ui.cached.is_none());
        assert_eq!(ui.status, "Preview is open");
        assert!(!ui.busy());
    }
    #[test]
    fn preview_startup_binding_requires_submitted_snapshot_and_motion() {
        let mut project = Workspace::default();
        let snapshot = project.studio().clone();
        let (_sender, receiver) = mpsc::channel();
        let opening = Opening {
            receiver,
            snapshot,
            reduced_motion: false,
            prepared_mp4: false,
        };
        assert!(opening.is_current(&project, false));
        assert!(!opening.is_current(&project, true));
        project.edit(Edit::Rename("Typed while preview opens".into()));
        assert!(!opening.is_current(&project, false));
        assert!(project.dirty());
    }
    #[test]
    fn disconnected_worker_clears_loading_without_acknowledging_a_save() {
        let mut ui = OutputUi::default();
        let mut project = Workspace::default();
        project.edit(Edit::Rename("Unsaved work".into()));
        drop(pending(&mut ui, &mut project));
        ui.poll(&mut project, &egui::Context::default(), false);
        assert!(project.dirty());
        assert!(!project.busy());
        assert!(ui.error.is_some());
        assert!(ui.cached.is_none());
    }
    #[test]
    fn failed_direct_preview_start_keeps_export_cache_and_newer_draft() {
        for newer_draft in [false, true] {
            let mut ui = OutputUi::default();
            let mut project = Workspace::default();
            let source = project.studio().next_media_id();
            project.edit(Edit::AddMedia {
                path: "media/generated.mp4".into(),
                duration_ms: 1000,
            });
            project.edit(Edit::AddClip {
                source,
                name: "Generated take".into(),
                in_ms: 0,
                out_ms: 1000,
            });
            assert!(ExportPlan::new(project.studio()).is_ok());
            ui.cached = Some((project.studio().clone(), false, result().artifact));
            let submitted = project.studio().clone();
            if newer_draft {
                project.edit(Edit::Rename("Newer unsaved edits".into()));
            }
            let draft = project.studio().clone();
            let (sender, receiver) = mpsc::channel();
            ui.player_pending = Some(Opening {
                receiver,
                snapshot: submitted,
                reduced_motion: false,
                prepared_mp4: false,
            });
            project.set_output_busy(true);
            sender
                .send(Err("A timeline source is missing".into()))
                .unwrap();
            ui.poll(&mut project, &egui::Context::default(), false);
            assert!(!ui.busy() && !project.busy());
            assert!(ui.error.is_some());
            assert_eq!(project.studio(), &draft);
            assert!(project.dirty());
            assert!(
                ui.cached.is_some(),
                "Direct preview does not consume or invalidate an existing export"
            );
        }
    }
    #[test]
    fn disconnected_direct_preview_worker_releases_guard_without_save_ack() {
        let mut ui = OutputUi::default();
        let mut project = Workspace::default();
        let snapshot = project.studio().clone();
        let (sender, receiver) = mpsc::channel();
        ui.player_pending = Some(Opening {
            receiver,
            snapshot,
            reduced_motion: false,
            prepared_mp4: false,
        });
        project.set_output_busy(true);
        project.edit(Edit::Rename("Unchanged newer draft".into()));
        drop(sender);
        ui.poll(&mut project, &egui::Context::default(), false);
        assert!(!ui.busy() && !project.busy());
        assert!(ui.error.is_some());
        assert!(project.dirty());
        assert_eq!(project.studio().name(), "Unchanged newer draft");
        assert!(ui.pending.is_none());
    }
    #[test]
    fn failed_rendered_compatibility_preview_invalidates_its_export_cache() {
        let mut ui = OutputUi::default();
        let mut project = Workspace::default();
        let snapshot = project.studio().clone();
        ui.cached = Some((snapshot.clone(), false, result().artifact));
        let (sender, receiver) = mpsc::channel();
        ui.player_pending = Some(Opening {
            receiver,
            snapshot,
            reduced_motion: false,
            prepared_mp4: true,
        });
        project.set_output_busy(true);
        project.edit(Edit::Rename("Keep newer compatibility edits".into()));
        sender.send(Err("Prepared MP4 is missing".into())).unwrap();
        ui.poll(&mut project, &egui::Context::default(), false);
        assert!(!ui.busy() && !project.busy());
        assert!(ui.cached.is_none());
        assert!(project.dirty());
        assert_eq!(project.studio().name(), "Keep newer compatibility edits");
    }
    #[cfg(all(feature = "capture-test", target_os = "linux"))]
    #[test]
    #[ignore = "Generated-only export/cancel and direct timeline preview/no-write integration; run under Xvfb, muted"]
    fn generated_sequence_controls_cache_cancel_and_preserve_newer_edits() {
        use crate::{SidebarVideo, configure};
        use std::{fs, process::Command, time::Instant};
        assert!(
            std::env::var_os("DISPLAY").is_some(),
            "Use an isolated Xvfb display"
        );
        let temp = tempfile::tempdir().unwrap();
        let ctx = egui::Context::default();
        configure(&ctx);
        let mut app = SidebarVideo::new();
        app.project = Workspace::test_project(&temp.path().join("project"));
        app.name_draft = app.project.studio().name().into();
        app.output.muted = true;
        let folder = app.project.folder.clone().unwrap();
        let path = folder.join("media/generated.mp4");
        assert!(
            Command::new("/usr/bin/ffmpeg")
                .args([
                    "-hide_banner",
                    "-loglevel",
                    "error",
                    "-f",
                    "lavfi",
                    "-i",
                    "testsrc2=size=320x180:rate=30",
                    "-f",
                    "lavfi",
                    "-i",
                    "anullsrc=channel_layout=stereo:sample_rate=48000",
                    "-t",
                    "1",
                    "-c:v",
                    "libx264",
                    "-threads",
                    "1",
                    "-preset",
                    "ultrafast",
                    "-c:a",
                    "aac",
                    "-pix_fmt",
                    "yuv420p"
                ])
                .arg(&path)
                .status()
                .unwrap()
                .success()
        );
        let original = fs::read(&path).unwrap();
        let media = app.project.studio().next_media_id();
        app.project.edit(Edit::AddMedia {
            path: "media/generated.mp4".into(),
            duration_ms: 1000,
        });
        for (start, end) in [(0, 400), (400, 1000)] {
            app.project.edit(Edit::AddClip {
                source: media,
                name: "Generated source".into(),
                in_ms: start,
                out_ms: end,
            });
        }
        let clip = app.project.studio().clips()[1].id();
        let slot = video_core::SlotKey::try_from(2).unwrap();
        app.project.edit(Edit::AddCueFromSnapshot {
            clip,
            offset_ms: 100,
            slot,
            content: video_core::OverlayContent::new(
                video_core::OverlayKind::Quote,
                "Generated words",
            ),
            duration_ms: 300,
        });
        fn paint(app: &mut SidebarVideo, ctx: &egui::Context) {
            let _ = ctx.run(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(1480., 1000.),
                    )),
                    focused: true,
                    ..Default::default()
                },
                |ctx| app.show(ctx),
            );
        }
        fn until(
            app: &mut SidebarVideo,
            ctx: &egui::Context,
            ready: impl Fn(&SidebarVideo) -> bool,
        ) {
            let start = Instant::now();
            while !ready(app) {
                paint(app, ctx);
                assert!(
                    start.elapsed() < Duration::from_secs(45),
                    "Generated output check timed out: {} {:?}",
                    app.output.status,
                    app.output.error
                );
                std::thread::sleep(Duration::from_millis(20));
            }
        }
        let count = || {
            fs::read_dir(folder.join("exports"))
                .unwrap()
                .filter_map(Result::ok)
                .filter(|e| e.path().extension().is_some_and(|x| x == "mp4"))
                .count()
        };
        let before = app.project.studio().clone();
        app.output
            .request(Action::Export, &mut app.project, &app.capture, &ctx, false);
        assert!(app.output.busy() && app.project.busy());
        assert!(!app.output.status.is_empty());
        app.name_draft = "Newer local edits".into();
        assert!(app.flush_name());
        until(&mut app, &ctx, |a| !a.output.busy());
        assert!(app.output.error.is_none(), "{:?}", app.output.error);
        assert!(app.project.dirty());
        assert_eq!(app.project.studio().name(), "Newer local edits");
        assert_eq!(video_core::ProjectStore::open(&folder).unwrap().1, before);
        assert_eq!(count(), 1);
        let json_before_preview = fs::read(folder.join("project.json")).unwrap();
        // Fresh fixture projects initialize main; inspect its local ref without
        // introducing app shell/Git helpers solely for this no-write assertion.
        let head = || fs::read(folder.join(".git/refs/heads/main")).unwrap();
        let head_before_preview = head();
        app.output
            .request(Action::Preview, &mut app.project, &app.capture, &ctx, false);
        assert!(
            app.output.pending.is_none(),
            "Preview must never start an export worker"
        );
        assert!(app.output.player_pending.is_some());
        assert!(app.output.progress.is_none());
        assert!(app.output.status.contains("directly"));
        until(&mut app, &ctx, |a| {
            a.output.player.is_some() || a.output.error.is_some()
        });
        assert!(app.output.error.is_none(), "{:?}", app.output.error);
        assert_eq!(count(), 1, "Direct preview must not create an MP4");
        assert!(
            app.project.dirty(),
            "Preview must not acknowledge a project save"
        );
        app.output
            .request(Action::Preview, &mut app.project, &app.capture, &ctx, false);
        app.name_draft = "Typed while preview opens".into();
        assert!(app.flush_name());
        until(&mut app, &ctx, |a| !a.output.busy());
        assert!(
            app.output.player.is_none(),
            "A stale startup ACK must close its earlier preview"
        );
        assert!(
            app.output
                .error
                .as_deref()
                .is_some_and(|error| error.contains("timeline changed"))
        );
        assert_eq!(app.project.studio().name(), "Typed while preview opens");
        assert_eq!(count(), 1);
        app.output
            .request(Action::Preview, &mut app.project, &app.capture, &ctx, false);
        until(&mut app, &ctx, |a| {
            a.output.player.is_some() || a.output.error.is_some()
        });
        assert!(app.output.error.is_none(), "{:?}", app.output.error);
        app.project.edit(Edit::TrimClip {
            clip,
            in_ms: 450,
            out_ms: 1000,
        });
        app.output
            .request(Action::Preview, &mut app.project, &app.capture, &ctx, false);
        until(&mut app, &ctx, |a| {
            a.output.player.is_some() || a.output.error.is_some()
        });
        assert!(app.output.error.is_none(), "{:?}", app.output.error);
        assert_eq!(
            count(),
            1,
            "Changed trims preview directly without a fresh MP4"
        );
        assert_eq!(
            fs::read(folder.join("project.json")).unwrap(),
            json_before_preview
        );
        assert_eq!(
            head(),
            head_before_preview,
            "Preview must not create a local version"
        );
        app.output
            .request(Action::Export, &mut app.project, &app.capture, &ctx, false);
        app.output.pending.as_ref().unwrap().cancel.cancel();
        until(&mut app, &ctx, |a| !a.output.busy());
        assert!(app.output.error.is_some());
        assert_eq!(count(), 1);
        assert_eq!(
            fs::read(folder.join("project.json")).unwrap(),
            json_before_preview
        );
        assert_eq!(head(), head_before_preview);
        assert_eq!(fs::read(&path).unwrap(), original);
        app.output.stop_preview();
    }
}
