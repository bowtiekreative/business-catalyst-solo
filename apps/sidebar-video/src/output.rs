//! Explicit sequence builds. Immutable submitted snapshots and save ACKs never
//! replace a newer draft; progress is the core's actual encoded time.
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
#[derive(Default)]
pub struct OutputUi {
    pending: Option<Pending>,
    player_pending: Option<mpsc::Receiver<Result<SequencePlayer, String>>>,
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
    pub fn busy(&self) -> bool {
        self.pending.is_some() || self.player_pending.is_some()
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
        self.status = "Preparing your sequence…".into();
        if matches!(action, Action::Preview)
            && let Some((saved, motion, artifact)) = &self.cached
            && saved == &snapshot
            && *motion == reduced_motion
        {
            self.play(project, artifact.clone(), ctx);
            return;
        }
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
    fn play(&mut self, project: &mut Workspace, artifact: ExportArtifact, ctx: &egui::Context) {
        let Some(store) = project.store() else {
            return;
        };
        self.stop_preview();
        let (sender, receiver) = mpsc::channel();
        let ctx = ctx.clone();
        let muted = self.muted;
        match std::thread::Builder::new()
            .name("storyfold-sequence-player".into())
            .spawn(move || {
                let result = store
                    .lock()
                    .map_err(|_| "Project worker failed".to_owned())
                    .and_then(|store| {
                        SequencePlayer::start(&store, &artifact.relative_path, muted)
                            .map_err(|e| e.to_string())
                    });
                let _ = sender.send(result);
                ctx.request_repaint();
            }) {
            Ok(_) => {
                self.player_pending = Some(receiver);
                project.set_output_busy(true);
                self.status = "Opening sequence preview…".into();
            }
            Err(e) => self.error = Some(e.to_string()),
        }
    }
    pub fn stop_preview(&mut self) {
        if let Some(player) = self.player.take() {
            let _ = std::thread::spawn(move || drop(player));
        }
    }
    pub fn reset(&mut self) {
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
                        message.snapshot,
                        message.reduced_motion,
                        outcome.artifact.clone(),
                    ));
                    if matches!(message.action, Action::Preview) {
                        if current {
                            self.play(project, outcome.artifact, ctx);
                        } else {
                            self.error=Some("Preview was built from earlier edits. Press Preview sequence again to prepare your latest timeline.".into());
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
            .and_then(|p| match p.try_recv() {
                Ok(m) => Some(m),
                Err(mpsc::TryRecvError::Empty) => None,
                Err(mpsc::TryRecvError::Disconnected) => {
                    Some(Err("The preview worker stopped.".into()))
                }
            });
        if let Some(player) = player {
            self.player_pending = None;
            project.set_output_busy(false);
            match player {
                Ok(player) => {
                    self.player = Some(player);
                    self.status="Sequence preview opened. Space pauses · arrows seek · F expands · Escape closes.".into();
                }
                Err(e) => {
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
                        self.error=Some("The native player could not decode this MP4. Export again to prepare a fresh sequence.".into());
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
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use video_core::{Edit, Orientation};
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
    fn changed_motion_preference_prevents_automatic_old_preview() {
        let mut ui = OutputUi::default();
        let mut project = Workspace::default();
        let snapshot = project.studio().clone();
        let sender = pending(&mut ui, &mut project);
        sender
            .send(Built {
                snapshot,
                action: Action::Preview,
                reduced_motion: false,
                result: Ok(result()),
            })
            .unwrap();
        ui.poll(&mut project, &egui::Context::default(), true);
        assert!(ui.player_pending.is_none());
        assert!(
            ui.error
                .as_deref()
                .is_some_and(|e| e.contains("earlier edits"))
        );
        assert!(!project.busy());
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
    #[cfg(all(feature = "capture-test", target_os = "linux"))]
    #[test]
    #[ignore = "Generated-only export/controller/native player integration; run under Xvfb, muted"]
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
        app.output
            .request(Action::Preview, &mut app.project, &app.capture, &ctx, false);
        until(&mut app, &ctx, |a| {
            a.output.player.is_some() || a.output.error.is_some()
        });
        assert!(app.output.error.is_none(), "{:?}", app.output.error);
        assert_eq!(count(), 2);
        app.output
            .request(Action::Preview, &mut app.project, &app.capture, &ctx, false);
        until(&mut app, &ctx, |a| {
            a.output.player.is_some() && !a.output.busy()
        });
        assert_eq!(count(), 2, "Current preview reuses its prepared MP4");
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
        assert_eq!(count(), 3, "Changed trim requires a fresh preview");
        app.output
            .request(Action::Export, &mut app.project, &app.capture, &ctx, false);
        app.output.pending.as_ref().unwrap().cancel.cancel();
        until(&mut app, &ctx, |a| !a.output.busy());
        assert!(app.output.error.is_some());
        assert_eq!(count(), 3);
        assert_eq!(fs::read(&path).unwrap(), original);
        app.output.stop_preview();
    }
}
