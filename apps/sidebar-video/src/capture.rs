//! Explicit device/session controls. Capture startup, import and recovery stay
//! off the paint thread; the core owns the media clock and recording gestures.
use crate::{Workspace, timecode};
use eframe::egui;
use std::{collections::VecDeque, sync::mpsc, time::Duration};
use video_core::{
    ClipId, OverlayContent, SlotKey,
    capture::{
        self, CaptureEvent, CapturePhase, CaptureSession, CaptureSettings, CompletedTake,
        DeviceInventory, RecoverySession,
    },
    media::{self, Cancellation, ImportedMedia},
};

enum Message {
    Devices(DeviceInventory),
    Started(CaptureSession),
    Imported(ImportedMedia),
    Recoveries(Vec<RecoverySession>),
    Recovered(Vec<CompletedTake>),
    Cancelled,
    Failed(String),
}

#[cfg(all(test, feature = "capture-test", target_os = "linux"))]
mod tests {
    use super::*;
    use crate::{Intent, SidebarVideo, configure};
    use std::time::Instant;
    fn paint(
        app: &mut SidebarVideo,
        ctx: &egui::Context,
        events: Vec<egui::Event>,
    ) -> egui::FullOutput {
        ctx.run(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(1480., 1000.),
                )),
                focused: true,
                events,
                ..Default::default()
            },
            |ctx| app.show(ctx),
        )
    }
    fn until(
        app: &mut SidebarVideo,
        ctx: &egui::Context,
        predicate: impl Fn(&SidebarVideo) -> bool,
    ) {
        let start = Instant::now();
        while !predicate(app) {
            paint(app, ctx, vec![]);
            assert!(
                start.elapsed() < Duration::from_secs(45),
                "Generated UI check timed out: {} {:?}",
                app.capture.timer(),
                app.capture.error
            );
            std::thread::sleep(Duration::from_millis(20));
        }
    }
    fn wait(app: &mut SidebarVideo, ctx: &egui::Context, millis: u64) {
        let start = Instant::now();
        while start.elapsed() < Duration::from_millis(millis) {
            paint(app, ctx, vec![]);
            std::thread::sleep(Duration::from_millis(20));
        }
    }
    fn pointer(pos: egui::Pos2, pressed: bool) -> Vec<egui::Event> {
        vec![
            egui::Event::PointerMoved(pos),
            egui::Event::PointerButton {
                pos,
                button: egui::PointerButton::Primary,
                pressed,
                modifiers: egui::Modifiers::NONE,
            },
        ]
    }
    fn space(pressed: bool) -> Vec<egui::Event> {
        vec![egui::Event::Key {
            key: egui::Key::Space,
            physical_key: Some(egui::Key::Space),
            pressed,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        }]
    }
    #[test]
    #[ignore = "Opt-in generated FFmpeg UI input integration; requires STORYFOLD_CAPTURE_TEST=1, no physical camera"]
    fn generated_pointer_space_capture_saves_newer_drafts_and_recovery_is_idempotent() {
        assert_eq!(std::env::var("STORYFOLD_CAPTURE_TEST").as_deref(), Ok("1"));
        let temp = tempfile::tempdir().unwrap();
        let ctx = egui::Context::default();
        configure(&ctx);
        ctx.enable_accesskit();
        let mut app = SidebarVideo::new();
        app.project = Workspace::test_project(&temp.path().join("project"));
        app.name_draft = app.project.studio().name().into();
        let output = paint(&mut app, &ctx, vec![]);
        let bounds = output
            .platform_output
            .accesskit_update
            .unwrap()
            .nodes
            .into_iter()
            .find_map(|(_, n)| {
                n.label()
                    .is_some_and(|l| l.starts_with("Record. Hold"))
                    .then(|| n.bounds().unwrap())
            })
            .unwrap();
        let pos = egui::pos2(
            ((bounds.x0 + bounds.x1) / 2.) as f32,
            ((bounds.y0 + bounds.y1) / 2.) as f32,
        );
        app.capture
            .enable_with(&app.project, &ctx, CaptureSession::start_generated);
        until(&mut app, &ctx, |a| {
            a.capture.can_record() && a.capture.texture.is_some()
        });
        let session = app.capture.session.as_ref().unwrap().status().session_id;
        app.request(Intent::New, &ctx);
        assert!(app.capture.camera_running());
        assert!(!app.project.loading());
        // A held pointer released outside the control is one complete clip.
        paint(&mut app, &ctx, pointer(pos, true));
        wait(&mut app, &ctx, 500);
        paint(&mut app, &ctx, pointer(egui::pos2(10., 120.), false));
        app.name_draft = "Typed while finishing".into();
        assert!(app.flush_name());
        until(&mut app, &ctx, |a| a.project.studio().clips().len() == 1);
        assert_eq!(app.project.studio().name(), "Typed while finishing");
        // Two short taps produce exactly one long take.
        paint(&mut app, &ctx, pointer(pos, true));
        wait(&mut app, &ctx, 60);
        paint(&mut app, &ctx, pointer(pos, false));
        until(&mut app, &ctx, |a| {
            a.capture
                .session
                .as_ref()
                .is_some_and(|s| s.status().phase == CapturePhase::LongTake)
        });
        wait(&mut app, &ctx, 400);
        paint(&mut app, &ctx, pointer(pos, true));
        wait(&mut app, &ctx, 60);
        paint(&mut app, &ctx, pointer(pos, false));
        until(&mut app, &ctx, |a| {
            a.project.studio().clips().len() == 2 && a.capture.can_record()
        });
        // Simultaneous pointer input cannot duplicate a Space-held gesture.
        paint(&mut app, &ctx, space(true));
        wait(&mut app, &ctx, 180);
        paint(&mut app, &ctx, pointer(pos, true));
        wait(&mut app, &ctx, 100);
        paint(&mut app, &ctx, pointer(pos, false));
        wait(&mut app, &ctx, 220);
        paint(&mut app, &ctx, space(false));
        until(&mut app, &ctx, |a| {
            a.project.studio().clips().len() == 3 && !a.project.busy()
        });
        // The real Preview button must also work while the session camera is on.
        app.output.muted = true;
        let bounds = paint(&mut app, &ctx, vec![])
            .platform_output
            .accesskit_update
            .unwrap()
            .nodes
            .into_iter()
            .find_map(|(_, node)| {
                (node.label() == Some("Stop camera & preview")).then(|| node.bounds().unwrap())
            })
            .unwrap();
        let preview = egui::pos2(
            ((bounds.x0 + bounds.x1) / 2.) as f32,
            ((bounds.y0 + bounds.y1) / 2.) as f32,
        );
        paint(&mut app, &ctx, pointer(preview, true));
        paint(&mut app, &ctx, pointer(preview, false));
        assert!(
            app.output.busy(),
            "Preview click should visibly queue camera stop and preparation"
        );
        until(&mut app, &ctx, |a| {
            a.capture.can_leave() && !a.project.busy() && !a.project.dirty() && !a.output.busy()
        });
        assert!(app.output.error.is_none(), "{:?}", app.output.error);
        assert!(app.output.status.contains("preview opened"));
        app.output.stop_preview();
        assert!(app.capture.texture.is_none());
        let studio = app.project.studio().clone();
        let folder = app.project.folder.clone().unwrap();
        assert_eq!(video_core::ProjectStore::open(&folder).unwrap().1, studio);
        app.capture.recover(&app.project, session, &ctx);
        until(&mut app, &ctx, |a| {
            a.capture.can_leave() && !a.project.busy()
        });
        assert_eq!(
            app.project.studio(),
            &studio,
            "Recovery must not duplicate existing takes"
        );
        assert_eq!(app.project.studio().clips().len(), 3);
    }
}
enum Append {
    Take(CompletedTake),
    Import(ImportedMedia),
}
struct Pending {
    receiver: mpsc::Receiver<Message>,
    cancellation: Cancellation,
}
#[derive(Default)]
pub struct CaptureUi {
    session: Option<CaptureSession>,
    pending: Option<Pending>,
    append: VecDeque<Append>,
    pub devices: Option<DeviceInventory>,
    pub settings: CaptureSettings,
    pub show_settings: bool,
    pub recoveries: Vec<RecoverySession>,
    pub message: String,
    pub error: Option<String>,
    pub texture: Option<egui::TextureHandle>,
    last_frame: Option<u64>,
    pointer_held: bool,
    space_held: bool,
    released_this_frame: bool,
    autosave: bool,
}
impl CaptureUi {
    pub fn queue_import(&mut self, media: ImportedMedia) {
        self.append.push_back(Append::Import(media));
    }
    pub fn preview_frame(&self) -> Option<std::sync::Arc<video_core::capture::PreviewFrame>> {
        self.session.as_ref().and_then(|s| s.latest_preview())
    }
    pub fn media_ms(&self) -> Option<u64> {
        self.session
            .as_ref()
            .and_then(|s| s.latest_preview())
            .map(|f| f.media_ms)
    }
    pub fn busy(&self) -> bool {
        self.pending.is_some() || !self.append.is_empty()
    }
    pub fn can_leave(&self) -> bool {
        !self.busy() && self.session.as_ref().is_none_or(|s| s.status().can_leave())
    }
    pub fn camera_running(&self) -> bool {
        self.session
            .as_ref()
            .is_some_and(|s| s.status().camera_running)
    }
    pub fn can_record(&self) -> bool {
        self.session.as_ref().is_some_and(|s| {
            matches!(
                s.status().phase,
                CapturePhase::Ready | CapturePhase::Holding | CapturePhase::LongTake
            )
        })
    }
    pub fn taking(&self) -> bool {
        self.session.as_ref().is_some_and(|s| {
            matches!(
                s.status().phase,
                CapturePhase::Holding | CapturePhase::LongTake
            )
        })
    }
    pub fn timer(&self) -> String {
        self.session.as_ref().map_or_else(
            || "Camera and microphone are off.".into(),
            |s| {
                let status = s.status();
                format!(
                    "{} · {} · {} finishing",
                    status.message,
                    timecode(status.active_ms),
                    status.pending_takes
                )
            },
        )
    }
    fn start(
        &mut self,
        ctx: &egui::Context,
        text: &str,
        task: impl FnOnce(Cancellation) -> Message + Send + 'static,
    ) {
        if self.busy() {
            return;
        }
        let (sender, receiver) = mpsc::channel();
        let cancellation = Cancellation::default();
        let token = cancellation.clone();
        let ctx = ctx.clone();
        match std::thread::Builder::new()
            .name("storyfold-capture-ui".into())
            .spawn(move || {
                let _ = sender.send(task(token));
                ctx.request_repaint();
            }) {
            Ok(_) => {
                self.pending = Some(Pending {
                    receiver,
                    cancellation,
                });
                self.message = text.into();
                self.error = None;
            }
            Err(e) => self.error = Some(format!("Could not start media worker: {e}")),
        }
    }
    pub fn refresh(&mut self, ctx: &egui::Context) {
        self.start(ctx, "Looking for cameras and microphones…", |_| {
            match capture::enumerate_devices() {
                Ok(devices) => Message::Devices(devices),
                Err(e) => Message::Failed(e.to_string()),
            }
        });
    }
    pub fn enable(&mut self, project: &Workspace, ctx: &egui::Context) {
        self.enable_with(project, ctx, CaptureSession::start);
    }
    fn enable_with(
        &mut self,
        project: &Workspace,
        ctx: &egui::Context,
        start: fn(&video_core::ProjectStore, CaptureSettings) -> video_core::Result<CaptureSession>,
    ) {
        if !self.can_leave() || project.busy() {
            return;
        }
        let Some(store) = project.store() else {
            self.error = Some("Create or save a project before enabling its camera.".into());
            return;
        };
        let settings = self.settings.clone();
        self.texture = None;
        self.last_frame = None;
        self.start(ctx, "Starting camera…", move |token| {
            if token.is_cancelled() {
                return Message::Cancelled;
            }
            match store
                .lock()
                .map_err(|_| "Project worker failed".to_owned())
                .and_then(|store| {
                    if token.is_cancelled() {
                        return Err("Camera start cancelled".to_owned());
                    }
                    start(&store, settings).map_err(|e| e.to_string())
                }) {
                Ok(session) => {
                    if token.is_cancelled() {
                        let _ = session.stop();
                        Message::Cancelled
                    } else {
                        Message::Started(session)
                    }
                }
                Err(e) => Message::Failed(e),
            }
        });
    }
    pub fn stop(&mut self) {
        if let Some(s) = &self.session
            && let Err(e) = s.stop()
        {
            self.error = Some(e.to_string());
        }
    }
    pub fn stop_take(&mut self) {
        if let Some(s) = &self.session
            && let Err(e) = s.stop_take()
        {
            self.error = Some(e.to_string());
        }
        self.pointer_held = false;
        self.space_held = false;
    }
    pub fn cancel_take(&mut self) {
        if let Some(s) = &self.session
            && let Err(e) = s.cancel_take()
        {
            self.error = Some(e.to_string());
        }
        self.pointer_held = false;
        self.space_held = false;
    }
    pub fn cancel_operation(&self) {
        if let Some(p) = &self.pending {
            p.cancellation.cancel();
        }
    }
    fn press(&mut self) -> bool {
        match self
            .session
            .as_ref()
            .ok_or("Enable the camera first".to_owned())
            .and_then(|s| s.press().map_err(|e| e.to_string()))
        {
            Ok(()) => {
                self.error = None;
                true
            }
            Err(e) => {
                self.error = Some(e);
                false
            }
        }
    }
    fn release(&mut self) {
        if let Some(s) = &self.session
            && let Err(e) = s.release()
        {
            self.error = Some(e.to_string());
        }
    }
    pub fn record_button(&mut self, response: &egui::Response, ctx: &egui::Context) {
        if !self.can_record() {
            return;
        }
        if response.is_pointer_button_down_on()
            && !self.pointer_held
            && !self.space_held
            && self.press()
        {
            self.pointer_held = true;
        }
        // Includes keyboard/AccessKit activation and a complete quick click
        // delivered in one paint pass. A pointer-up already handled in poll
        // must never become a second tap.
        if response.clicked()
            && !self.pointer_held
            && !self.space_held
            && !self.released_this_frame
            && self.press()
        {
            self.release();
        }
        if self.pointer_held {
            ctx.request_repaint_after(Duration::from_millis(16));
        }
    }
    pub fn space(&mut self, ctx: &egui::Context) {
        if self.can_record()
            && !self.space_held
            && !self.pointer_held
            && ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Space))
            && self.press()
        {
            self.space_held = true;
        }
    }
    pub fn mark(&mut self, slot: SlotKey, content: OverlayContent, duration_ms: u64) -> bool {
        if let Some(s) = &self.session {
            if let Err(e) = s.mark_slot(slot, content, duration_ms) {
                self.error = Some(e.to_string());
                return false;
            }
            return true;
        }
        false
    }
    pub fn import(&mut self, project: &Workspace, ctx: &egui::Context) {
        if !self.can_leave() || project.busy() {
            return;
        }
        let Some(store) = project.store() else {
            self.error = Some("Create or save a project before adding media.".into());
            return;
        };
        self.start(ctx, "Choose a local video to import…", move |token| {
            let Some(path) = rfd::FileDialog::new()
                .set_title("Add video to Storyfold")
                .add_filter("Video", &["mp4", "mkv", "mov", "webm", "avi", "m4v"])
                .pick_file()
            else {
                return Message::Cancelled;
            };
            if token.is_cancelled() {
                return Message::Cancelled;
            }
            match store
                .lock()
                .map_err(|_| "Project worker failed".to_owned())
                .and_then(|store| {
                    media::import_media(&store, &path, &token).map_err(|e| e.to_string())
                }) {
                Ok(media) => Message::Imported(media),
                Err(e) => Message::Failed(e),
            }
        });
    }
    pub fn find_recovery(&mut self, project: &Workspace, ctx: &egui::Context) {
        if !self.can_leave() || project.busy() {
            return;
        }
        let Some(store) = project.store() else {
            return;
        };
        self.start(
            ctx,
            "Checking this project's interrupted recordings…",
            move |_| match store
                .lock()
                .map_err(|_| "Project worker failed".to_owned())
                .and_then(|s| capture::list_recoverable(&s).map_err(|e| e.to_string()))
            {
                Ok(items) => Message::Recoveries(items),
                Err(e) => Message::Failed(e),
            },
        );
    }
    fn recover(&mut self, project: &Workspace, id: String, ctx: &egui::Context) {
        if !self.can_leave() || project.busy() {
            return;
        }
        let Some(store) = project.store() else {
            return;
        };
        self.start(
            ctx,
            "Recovering saved recording ranges…",
            move |token| match store
                .lock()
                .map_err(|_| "Project worker failed".to_owned())
                .and_then(|s| capture::recover_session(&s, &id, &token).map_err(|e| e.to_string()))
            {
                Ok(takes) => Message::Recovered(takes),
                Err(e) => Message::Failed(e),
            },
        );
    }
    pub fn poll(&mut self, project: &mut Workspace, ctx: &egui::Context) -> Option<ClipId> {
        self.released_this_frame = false;
        if !ctx.input(|i| i.focused) && (self.pointer_held || self.space_held) {
            self.stop_take();
        }
        if self.pointer_held && ctx.input(|i| i.pointer.any_released()) {
            self.release();
            self.pointer_held = false;
            self.released_this_frame = true;
        }
        if self.space_held && ctx.input(|i| i.key_released(egui::Key::Space)) {
            self.release();
            self.space_held = false;
            self.released_this_frame = true;
        }
        let message = self
            .pending
            .as_ref()
            .and_then(|p| match p.receiver.try_recv() {
                Ok(m) => Some(m),
                Err(mpsc::TryRecvError::Empty) => None,
                Err(mpsc::TryRecvError::Disconnected) => Some(Message::Failed(
                    "Media worker stopped. Saved media remains available.".into(),
                )),
            });
        if let Some(message) = message {
            let cancelled = self
                .pending
                .take()
                .is_some_and(|p| p.cancellation.is_cancelled());
            match message {
                Message::Devices(devices) => {
                    if !devices
                        .cameras
                        .iter()
                        .any(|c| c.id == self.settings.camera_id)
                    {
                        self.settings.camera_id = devices
                            .cameras
                            .first()
                            .map_or_else(String::new, |c| c.id.clone());
                        if let Some(mode) = devices.cameras.first().and_then(|c| c.modes.first()) {
                            self.settings.mode = *mode;
                        }
                    }
                    if self
                        .settings
                        .microphone_id
                        .as_ref()
                        .is_some_and(|id| !devices.microphones.iter().any(|m| &m.id == id))
                    {
                        self.settings.microphone_id = None;
                    }
                    self.message = devices.message.clone();
                    self.devices = Some(devices);
                }
                Message::Started(session) => {
                    if cancelled {
                        let _ = session.stop();
                        let _ = std::thread::spawn(move || drop(session));
                        self.message = "Camera start cancelled.".into();
                    } else {
                        self.session = Some(session);
                        self.show_settings = false;
                    }
                }
                Message::Imported(media) => {
                    self.append.push_back(Append::Import(media));
                }
                Message::Recoveries(items) => {
                    self.message = if items.is_empty() {
                        "No interrupted recordings found.".into()
                    } else {
                        "Choose a recording to recover.".into()
                    };
                    self.recoveries = items;
                }
                Message::Recovered(takes) => {
                    self.append.extend(takes.into_iter().map(Append::Take));
                    self.recoveries.clear();
                }
                Message::Cancelled => self.message = "Media selection cancelled.".into(),
                Message::Failed(e) => self.error = Some(e),
            }
        }
        if let Some(session) = &self.session {
            for event in session.poll_events() {
                match event {
                    CaptureEvent::TakeCompleted(take) => self.append.push_back(Append::Take(take)),
                    CaptureEvent::TakeFailed { ordinal, message } => {
                        self.error = Some(format!("Take {ordinal}: {message}"))
                    }
                    CaptureEvent::Error(e) => self.error = Some(e),
                    _ => {}
                }
            }
            if let Some(frame) = session
                .latest_preview()
                .filter(|f| self.last_frame != Some(f.sequence))
            {
                let image = egui::ColorImage::from_rgba_unmultiplied(
                    [frame.width as usize, frame.height as usize],
                    &frame.rgba,
                );
                match &mut self.texture {
                    Some(texture) => texture.set(image, egui::TextureOptions::LINEAR),
                    None => {
                        self.texture = Some(ctx.load_texture(
                            "continuous-camera",
                            image,
                            egui::TextureOptions::LINEAR,
                        ))
                    }
                }
                self.last_frame = Some(frame.sequence);
            }
            let status = session.status();
            self.message = status.message.clone();
            if status.can_leave() {
                // Join/reap even an already stopped sidecar away from painting.
                if let Some(session) = self.session.take() {
                    let _ = std::thread::spawn(move || drop(session));
                }
                self.pointer_held = false;
                self.space_held = false;
                self.texture = None;
                self.last_frame = None;
            }
        }
        let mut selected = None;
        if !project.loading()
            && let Some(store) = project.store()
            && let Ok(store) = store.try_lock()
        {
            while let Some(item) = self.append.pop_front() {
                let path = match &item {
                    Append::Take(take) => take.media().path(),
                    Append::Import(media) => media.path(),
                };
                if project.studio().media().iter().any(|m| m.path() == path) {
                    self.message =
                        "This take is already in the project. Existing clips are unchanged.".into();
                    continue;
                }
                let result = match item {
                    Append::Take(take) => take.append_to(&store, &mut project.editor),
                    Append::Import(media) => media.append_to(&store, &mut project.editor),
                };
                match result {
                    Ok(clip) => {
                        selected = Some(clip);
                        self.autosave = true;
                    }
                    Err(e) => {
                        self.error = Some(format!(
                            "Could not add media to the timeline: {e}. The source remains on disk for recovery."
                        ))
                    }
                }
            }
        }
        if self.autosave && !project.busy() {
            project.save(ctx, None);
            self.autosave = false;
        }
        if self.pending.is_some() || !self.append.is_empty() || self.session.is_some() {
            ctx.request_repaint_after(Duration::from_millis(16));
        }
        selected
    }
    pub fn settings_dialog(&mut self, project: &Workspace, ctx: &egui::Context) {
        if !self.show_settings {
            return;
        }
        let response=egui::Modal::new(egui::Id::new("capture-settings")).show(ctx,|ui|{
            ui.set_width(440.);ui.heading("Camera and microphone");
            ui.label("Enable starts one continuous local capture session. Record marks clips from its buffer.");
            ui.add_enabled_ui(self.can_leave(),|ui|{
                if let Some(devices)=&self.devices {
                    let old=self.settings.camera_id.clone();
                    let camera_label=devices.cameras.iter().find(|c|c.id==old).map_or("Choose a camera",|c|c.label.as_str());
                    egui::ComboBox::from_label("Camera").selected_text(camera_label).show_ui(ui,|ui|{
                        for c in &devices.cameras {ui.selectable_value(&mut self.settings.camera_id,c.id.clone(),&c.label);}
                    });
                    if let Some(camera)=devices.cameras.iter().find(|c|c.id==self.settings.camera_id) {
                        if (old!=self.settings.camera_id || !camera.modes.contains(&self.settings.mode))
                            && let Some(mode)=camera.modes.first(){self.settings.mode = *mode;}
                        egui::ComboBox::from_label("Capture quality").selected_text(format!("{}×{} · {}fps · {:?}",self.settings.mode.width,self.settings.mode.height,self.settings.mode.fps,self.settings.mode.format)).show_ui(ui,|ui|{
                            for mode in &camera.modes {ui.selectable_value(&mut self.settings.mode,*mode,format!("{}×{} · {}fps · {:?}",mode.width,mode.height,mode.fps,mode.format));}
                        });
                    }
                    let microphone_label=devices.microphones.iter().find(|m|Some(&m.id)==self.settings.microphone_id.as_ref()).map_or("Microphone off",|m|m.label.as_str());
                    egui::ComboBox::from_label("Microphone").selected_text(microphone_label).show_ui(ui,|ui|{
                        ui.selectable_value(&mut self.settings.microphone_id,None,"Microphone off");
                        for m in &devices.microphones {ui.selectable_value(&mut self.settings.microphone_id,Some(m.id.clone()),&m.label);}
                    });
                }
                ui.add(egui::Slider::new(&mut self.settings.tap_threshold_ms,50..=2000).text("Tap threshold (ms)"));
            });
            if !self.message.is_empty(){ui.label(&self.message);}
            if let Some(error)=&self.error{ui.colored_label(egui::Color32::from_rgb(160,30,45),error);}
            ui.horizontal(|ui|{
                if ui.add_enabled(self.can_leave(),egui::Button::new("Refresh devices")).clicked(){self.refresh(ctx);}
                if ui.add_enabled(self.can_leave() && !project.busy() && project.store().is_some() && !self.settings.camera_id.is_empty(),egui::Button::new("Enable camera")).clicked(){self.enable(project,ctx);}
                if ui.button("Close").clicked(){self.show_settings=false;}
            });
            if self.busy(){ui.spinner();}
        });
        if response.should_close() {
            self.show_settings = false;
        }
    }
    pub fn recovery_dialog(&mut self, project: &Workspace, ctx: &egui::Context) {
        if self.recoveries.is_empty() {
            return;
        }
        let mut recover = None;
        let mut close = false;
        egui::Modal::new(egui::Id::new("recording-recovery")).show(ctx, |ui| {
            ui.set_width(450.);
            ui.heading("Interrupted recordings");
            for item in &self.recoveries {
                ui.horizontal(|ui| {
                    ui.vertical(|ui| {
                        ui.label(&item.id);
                        ui.label(&item.message);
                    });
                    if ui
                        .add_enabled(self.can_leave(), egui::Button::new("Recover"))
                        .clicked()
                    {
                        recover = Some(item.id.clone());
                    }
                });
            }
            if ui.button("Close").clicked() {
                close = true;
            }
        });
        if close {
            self.recoveries.clear();
        }
        if let Some(id) = recover {
            self.recover(project, id, ctx);
        }
    }
}
