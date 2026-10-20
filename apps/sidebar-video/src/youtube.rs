//! Explicit YouTube actions. No credential access or HTTP on construction,
//! opening this panel or changing projects; slow operations stay on workers.
use crate::{Workspace, capture::CaptureUi, workspace::Store};
use eframe::egui;
use std::{
    path::PathBuf,
    sync::{Arc, Mutex, mpsc},
    time::Duration,
};
use video_core::{
    media::{Cancellation, ImportedMedia},
    youtube::{
        self, ConnectionId, ImportLimits, Privacy, PublishRequest, UploadPhase, UploadProgress,
        UploadedVideo, YoutubeClient, YoutubeImportPhase, YoutubeImportProgress,
        YoutubeImportRequest, YoutubeStatus,
    },
};

#[derive(Clone, Copy, Default, PartialEq, Eq)]
enum Pane {
    #[default]
    Connection,
    Upload,
    Import,
}
#[derive(Clone, Copy)]
enum AccountAction {
    Load,
    Configure,
    Connect,
    Disconnect,
    ForgetClient,
    ForgetPending,
}
#[derive(Clone)]
enum Progress {
    Upload(UploadProgress),
    Import(YoutubeImportProgress),
}
struct Published {
    video: UploadedVideo,
    title: String,
    requested_privacy: Privacy,
}
enum Outcome {
    Account(YoutubeStatus),
    ExportSelected(String),
    Published {
        result: Result<Published, String>,
        account: Option<YoutubeStatus>,
    },
    Imported(ImportedMedia),
    Cancelled,
}
struct Completion {
    client: Option<YoutubeClient>,
    result: Result<Outcome, String>,
}
struct Pending {
    receiver: mpsc::Receiver<Completion>,
    latest: Arc<Mutex<Option<Progress>>>,
    cancel: Cancellation,
    epoch: u64,
    project: Option<PathBuf>,
}

pub struct YoutubeUi {
    pub open: bool,
    pub error: Option<String>,
    pane: Pane,
    client: Option<YoutubeClient>,
    account: Option<YoutubeStatus>,
    pending: Option<Pending>,
    epoch: u64,
    status: String,
    progress: Option<Progress>,
    export: Option<String>,
    title: String,
    description: String,
    privacy: Privacy,
    reviewed: Option<ConnectionId>,
    resume_reviewed: Option<ConnectionId>,
    published: Option<Published>,
    url: String,
    start: String,
    end: String,
    limit_mib: u32,
    limit_minutes: u32,
}
impl Default for YoutubeUi {
    fn default() -> Self {
        Self {
            open: false,
            error: None,
            pane: Pane::Connection,
            client: None,
            account: None,
            pending: None,
            epoch: 0,
            status: String::new(),
            progress: None,
            export: None,
            title: String::new(),
            description: String::new(),
            privacy: Privacy::Unlisted,
            reviewed: None,
            resume_reviewed: None,
            published: None,
            url: String::new(),
            start: "0".into(),
            end: "10".into(),
            limit_mib: 2048,
            limit_minutes: 30,
        }
    }
}
impl YoutubeUi {
    pub fn busy(&self) -> bool {
        self.pending.is_some()
    }
    pub fn show_window(&mut self) {
        self.open = true;
    }
    pub fn project_changed(&mut self) {
        self.epoch = self.epoch.wrapping_add(1);
        if let Some(pending) = &self.pending {
            pending.cancel.cancel();
        }
        self.account = None;
        self.export = None;
        self.title.clear();
        self.description.clear();
        self.url.clear();
        self.reviewed = None;
        self.resume_reviewed = None;
        self.published = None;
        self.progress = None;
        self.error = None;
        self.status = "Load the saved connection explicitly when you need it.".into();
    }
    fn start(
        &mut self,
        project: &mut Workspace,
        ctx: &egui::Context,
        status: &str,
        task: impl FnOnce(
            Option<YoutubeClient>,
            Cancellation,
            Arc<Mutex<Option<Progress>>>,
            egui::Context,
        ) -> Completion
        + Send
        + 'static,
    ) {
        if self.busy() || project.busy() {
            return;
        }
        let (sender, receiver) = mpsc::sync_channel(1);
        let latest = Arc::new(Mutex::new(None));
        let worker_latest = latest.clone();
        let cancel = Cancellation::default();
        let token = cancel.clone();
        let client = self.client.clone();
        let worker_ctx = ctx.clone();
        match std::thread::Builder::new()
            .name("storyfold-youtube".into())
            .spawn(move || {
                let completion = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    task(client, token, worker_latest, worker_ctx.clone())
                }))
                .unwrap_or_else(|_| Completion {
                    client: None,
                    result: Err(
                        "YouTube worker stopped unexpectedly. Your draft is still here.".into(),
                    ),
                });
                let _ = sender.send(completion);
                worker_ctx.request_repaint();
            }) {
            Ok(_) => {
                self.pending = Some(Pending {
                    receiver,
                    latest,
                    cancel,
                    epoch: self.epoch,
                    project: project.folder.clone(),
                });
                project.set_output_busy(true);
                self.error = None;
                self.status = status.into();
                self.progress = None;
            }
            Err(_) => self.error = Some("Could not start the YouTube worker. Try again.".into()),
        }
    }
    fn account_action(
        &mut self,
        action: AccountAction,
        project: &mut Workspace,
        ctx: &egui::Context,
    ) {
        let status = match action {
            AccountAction::Load => {
                "Loading the saved connection from the operating-system keychain…"
            }
            AccountAction::Configure => "Choose the Google Desktop client JSON in the file picker…",
            AccountAction::Connect => "Complete Google consent in your system browser…",
            AccountAction::Disconnect => "Disconnecting YouTube locally…",
            AccountAction::ForgetClient => "Forgetting the saved client and connection…",
            AccountAction::ForgetPending => "Forgetting the local pending-upload state…",
        };
        self.reviewed = None;
        self.resume_reviewed = None;
        self.start(project, ctx, status, move |cached, cancel, _, ctx| {
            if matches!(action, AccountAction::Configure) {
                let Some(file) = rfd::FileDialog::new()
                    .set_title("Choose Google Desktop OAuth client JSON")
                    .add_filter("Google Desktop client JSON", &["json"])
                    .pick_file()
                else {
                    return Completion {
                        client: cached,
                        result: Ok(Outcome::Cancelled),
                    };
                };
                return with_client(cached, |client| {
                    if cancel.is_cancelled() {
                        return Ok(Outcome::Cancelled);
                    }
                    client
                        .configure_client(&file, &cancel)
                        .map(Outcome::Account)
                        .map_err(|e| e.to_string())
                });
            }
            with_client(cached, |client| {
                if cancel.is_cancelled() {
                    return Ok(Outcome::Cancelled);
                }
                let account = match action {
                    AccountAction::Load => client.status(),
                    AccountAction::Connect => {
                        let authorization =
                            client.begin_connect(&cancel).map_err(|e| e.to_string())?;
                        if cancel.is_cancelled() {
                            return Ok(Outcome::Cancelled);
                        }
                        // Private state-bearing URL is never logged or kept in UI state.
                        ctx.open_url(egui::OpenUrl::new_tab(authorization.browser_url()));
                        ctx.request_repaint();
                        client.finish_connect(authorization, &cancel)
                    }
                    AccountAction::Disconnect => client.disconnect(),
                    AccountAction::ForgetClient => {
                        client.forget_client().and_then(|_| client.status())
                    }
                    AccountAction::ForgetPending => {
                        client.forget_pending_upload().and_then(|_| client.status())
                    }
                    AccountAction::Configure => unreachable!("handled file picker above"),
                };
                account.map(Outcome::Account).map_err(|e| e.to_string())
            })
        });
    }
    fn choose_export(&mut self, project: &mut Workspace, ctx: &egui::Context) {
        let Some(folder) = project.folder.clone() else {
            return;
        };
        self.start(
            project,
            ctx,
            "Choose a rendered MP4 from this project's exports folder…",
            move |client, cancel, _, _| {
                let result = (|| {
                    let Some(file) = rfd::FileDialog::new()
                        .set_title("Choose this project's rendered MP4")
                        .set_directory(folder.join("exports"))
                        .add_filter("Rendered MP4", &["mp4"])
                        .pick_file()
                    else {
                        return Ok(Outcome::Cancelled);
                    };
                    if cancel.is_cancelled() {
                        return Ok(Outcome::Cancelled);
                    }
                    let leaf = file
                        .strip_prefix(folder.join("exports"))
                        .ok()
                        .filter(|p| p.components().count() == 1)
                        .and_then(|p| p.to_str())
                        .filter(|name| name.ends_with(".mp4"))
                        .ok_or("Choose one MP4 directly inside this project's exports folder")?;
                    let meta = std::fs::symlink_metadata(&file)
                        .map_err(|_| "Selected MP4 is unavailable")?;
                    if !meta.is_file() || meta.file_type().is_symlink() {
                        return Err("Choose a normal rendered MP4 file".into());
                    }
                    // Core Publish validates confinement, links, container and source again.
                    Ok(Outcome::ExportSelected(format!("exports/{leaf}")))
                })();
                Completion { client, result }
            },
        );
    }
    fn publish_request(&self) -> Result<PublishRequest, String> {
        if self.account.as_ref().is_some_and(|a| a.pending.is_some()) {
            return Err("Resume or forget the pending upload before starting another.".into());
        }
        let connection = self
            .account
            .as_ref()
            .and_then(|a| a.connection.as_ref())
            .ok_or("Load and connect YouTube first.")?;
        if self.reviewed.as_ref() != Some(connection) {
            return Err(
                "Review the export and current connection, then check the confirmation box.".into(),
            );
        }
        let request = PublishRequest {
            relative_path: self
                .export
                .clone()
                .ok_or("Choose a rendered MP4 from this project.")?,
            title: self.title.clone(),
            description: self.description.clone(),
            privacy: self.privacy,
            reviewed_connection: connection.clone(),
        };
        request.validate().map_err(|e| e.to_string())?;
        Ok(request)
    }
    fn publish(&mut self, project: &mut Workspace, ctx: &egui::Context) {
        let Some(store) = project.store() else {
            return;
        };
        let request = match self.publish_request() {
            Ok(request) => request,
            Err(error) => {
                self.error = Some(error);
                return;
            }
        };
        self.reviewed = None;
        self.start(
            project,
            ctx,
            "Checking the rendered MP4 before uploading…",
            move |cached, cancel, latest, ctx| {
                with_client(cached, |client| {
                    let title = request.title.clone();
                    let requested_privacy = request.privacy;
                    let result = lock_store(&store)
                        .and_then(|store| {
                            client
                                .publish(&store, request, &cancel, |p| {
                                    progress(&latest, &ctx, Progress::Upload(p))
                                })
                                .map_err(|e| e.to_string())
                        })
                        .map(|video| Published {
                            video,
                            title,
                            requested_privacy,
                        });
                    let account = client.status().ok();
                    Ok(Outcome::Published { result, account })
                })
            },
        );
    }
    fn resume(&mut self, project: &mut Workspace, ctx: &egui::Context) {
        let Some(store) = project.store() else {
            return;
        };
        let Some(account) = &self.account else {
            return;
        };
        let (Some(connection), Some(pending)) = (&account.connection, &account.pending) else {
            return;
        };
        if self.resume_reviewed.as_ref() != Some(connection) {
            return;
        }
        let connection = connection.clone();
        let title = pending.title.clone();
        let requested_privacy = pending.privacy;
        self.resume_reviewed = None;
        self.start(
            project,
            ctx,
            "Checking the existing upload and its unchanged MP4…",
            move |cached, cancel, latest, ctx| {
                with_client(cached, |client| {
                    let result = lock_store(&store)
                        .and_then(|store| {
                            client
                                .resume(&store, &connection, &cancel, |p| {
                                    progress(&latest, &ctx, Progress::Upload(p))
                                })
                                .map_err(|e| e.to_string())
                        })
                        .map(|video| Published {
                            video,
                            title,
                            requested_privacy,
                        });
                    let account = client.status().ok();
                    Ok(Outcome::Published { result, account })
                })
            },
        );
    }
    fn import_request(&self) -> Result<(YoutubeImportRequest, ImportLimits), String> {
        let request = YoutubeImportRequest {
            url: self.url.clone(),
            start_ms: milliseconds(&self.start)?,
            end_ms: milliseconds(&self.end)?,
        };
        request.canonical_url().map_err(|e| e.to_string())?;
        let limits = ImportLimits {
            max_bytes: u64::from(self.limit_mib) * 1024 * 1024,
            timeout: Duration::from_secs(u64::from(self.limit_minutes) * 60),
        };
        limits.validate().map_err(|e| e.to_string())?;
        Ok((request, limits))
    }
    fn import(&mut self, project: &mut Workspace, ctx: &egui::Context) {
        let Some(store) = project.store() else {
            return;
        };
        let (request, limits) = match self.import_request() {
            Ok(value) => value,
            Err(error) => {
                self.error = Some(error);
                return;
            }
        };
        self.start(
            project,
            ctx,
            "Importing the selected public video clip…",
            move |client, cancel, latest, ctx| {
                let result = lock_store(&store).and_then(|store| {
                    youtube::import_clip(&store, &request, limits, &cancel, |p| {
                        progress(&latest, &ctx, Progress::Import(p))
                    })
                    .map(Outcome::Imported)
                    .map_err(|e| e.to_string())
                });
                Completion { client, result }
            },
        );
    }
    pub fn poll(&mut self, project: &mut Workspace, capture: &mut CaptureUi, ctx: &egui::Context) {
        let Some(pending) = &self.pending else {
            return;
        };
        if let Ok(latest) = pending.latest.try_lock()
            && let Some(latest) = &*latest
        {
            self.progress = Some(latest.clone());
        }
        let completion = match pending.receiver.try_recv() {
            Ok(completion) => completion,
            Err(mpsc::TryRecvError::Empty) => {ctx.request_repaint_after(Duration::from_millis(50)); return;}
            Err(mpsc::TryRecvError::Disconnected) => Completion { client: None, result: Err("YouTube worker stopped before completing its request. Your draft is still here.".into()) },
        };
        let pending = self.pending.take().expect("pending outcome");
        project.set_output_busy(false);
        if pending.epoch != self.epoch || pending.project != project.folder {
            self.status = "Project changed; the earlier result was not applied here.".into();
            self.progress = None;
            return;
        }
        if let Some(client) = completion.client {
            self.client = Some(client);
        }
        match completion.result {
            Ok(Outcome::Account(account)) => {
                self.account = Some(account);
                self.reviewed = None;
                self.resume_reviewed = None;
                self.status = "Connection state updated. Review it before uploading.".into();
            }
            Ok(Outcome::ExportSelected(relative)) => {
                self.export = Some(relative);
                self.reviewed = None;
                self.status =
                    "Rendered MP4 selected. Review the title, privacy and connection.".into();
            }
            Ok(Outcome::Published { result, account }) => {
                if let Some(account) = account {
                    self.account = Some(account);
                }
                self.reviewed = None;
                self.resume_reviewed = None;
                match result {
                    Ok(published) => {
                        self.status = format!(
                            "Upload complete. Actual privacy: {}.",
                            privacy_name(published.video.privacy)
                        );
                        self.published = Some(published);
                    }
                    Err(error) => {
                        self.error = Some(error);
                        self.status = "Load the saved connection to check pending-upload state if it is not shown.".into();
                    }
                }
            }
            Ok(Outcome::Imported(media)) => {
                if pending.cancel.is_cancelled() {
                    self.status = format!(
                        "The clip finished importing after cancellation. Its source is saved as {}. Use Add media later to add it to the sequence.",
                        media.path()
                    );
                } else {
                    capture.queue_import(media);
                    self.status =
                        "Clip imported. Adding it to the current sequence and saving…".into();
                }
            }
            Ok(Outcome::Cancelled) => {
                self.status = "Operation canceled. Your draft is unchanged.".into()
            }
            Err(error) => self.error = Some(error),
        }
    }
    pub fn dialog(&mut self, project: &mut Workspace, capture: &CaptureUi, ctx: &egui::Context) {
        if !self.open {
            return;
        }
        let mut close = false;
        let available = !self.busy() && !project.busy() && capture.can_leave();
        let modal = egui::Modal::new(egui::Id::new("youtube-studio")).show(ctx, |ui| {
            ui.set_width((ctx.content_rect().width() - 80.).clamp(400., 860.));
            ui.heading("YouTube");
            ui.label("Upload a rendered MP4 or import one public video clip.");
            ui.horizontal_wrapped(|ui| {
                ui.selectable_value(&mut self.pane, Pane::Connection, "Connection");
                ui.selectable_value(&mut self.pane, Pane::Upload, "Upload MP4");
                ui.selectable_value(&mut self.pane, Pane::Import, "Import clip");
            });
            ui.separator();
            egui::ScrollArea::vertical().max_height((ctx.content_rect().height() - 410.).clamp(180., 460.)).show(ui, |ui| {
                match self.pane {
                    Pane::Connection => self.connection_form(ui, project, ctx, available),
                    Pane::Upload => self.upload_form(ui, project, ctx, available),
                    Pane::Import => self.import_form(ui, project, ctx, available),
                }
            });
            ui.separator();
            if !capture.can_leave() { ui.label("Close this panel and stop the camera before starting a YouTube operation."); }
            if project.store().is_none() && self.pane != Pane::Connection { ui.label("Create or save a project first."); }
            self.primary_actions(ui, project, ctx, available);
            self.draw_progress(ui);
            if !self.status.is_empty() { ui.label(&self.status); }
            if let Some(error) = &self.error { ui.colored_label(egui::Color32::from_rgb(160, 30, 45), error); }
            ui.horizontal_wrapped(|ui| {
                if self.busy() {
                    ui.spinner();
                    if ui.button("Cancel operation").clicked() && let Some(pending) = &self.pending {
                        pending.cancel.cancel(); self.status = "Cancellation requested. Close any file picker; an in-flight request can take up to 45 seconds.".into();
                    }
                }
                if ui.add_enabled(!self.busy(), egui::Button::new("Close YouTube")).clicked() { close = true; }
            });
        });
        if close || modal.should_close() && !self.busy() {
            self.open = false;
        }
    }
    fn connection_label(&self, ui: &mut egui::Ui) {
        if self
            .account
            .as_ref()
            .is_some_and(|a| a.connection.is_some())
        {
            ui.label("Connected for uploads to the channel selected in Google consent.");
            ui.small(
                "Channel name is not verified here. Reconnect in Google to choose another channel.",
            );
        } else {
            ui.label("No YouTube connection loaded in this window.");
        }
    }
    fn connection_form(
        &mut self,
        ui: &mut egui::Ui,
        project: &mut Workspace,
        ctx: &egui::Context,
        available: bool,
    ) {
        self.connection_label(ui);
        ui.label("Opening this panel does not read credentials. Load the saved connection explicitly, or choose your Desktop client JSON.");
        ui.horizontal_wrapped(|ui| {
            if ui
                .add_enabled(available, egui::Button::new("Load saved connection"))
                .clicked()
            {
                self.account_action(AccountAction::Load, project, ctx);
            }
            if ui
                .add_enabled(available, egui::Button::new("Choose Desktop client JSON…"))
                .clicked()
            {
                self.account_action(AccountAction::Configure, project, ctx);
            }
        });
        let configured = self.account.as_ref().is_some_and(|a| a.configured);
        let pending = self.account.as_ref().is_some_and(|a| a.pending.is_some());
        ui.horizontal_wrapped(|ui| {
            if ui.add_enabled(available && configured && !pending, egui::Button::new("Connect in browser")).on_disabled_hover_text("Load or choose a Desktop client first. Resolve any pending upload before reconnecting.").clicked() { self.account_action(AccountAction::Connect, project, ctx); }
            let label = if pending { "Disconnect & forget pending upload" } else { "Disconnect locally" };
            if ui.add_enabled(available && self.account.as_ref().is_some_and(|a| a.connection.is_some()), egui::Button::new(label)).clicked() { self.account_action(AccountAction::Disconnect, project, ctx); }
        });
        if ui
            .add_enabled(
                available && configured,
                egui::Button::new(if pending {
                    "Forget client, connection & pending upload"
                } else {
                    "Forget client and connection"
                }),
            )
            .clicked()
        {
            self.account_action(AccountAction::ForgetClient, project, ctx);
        }
        ui.separator();
        ui.strong("One-time Google setup");
        ui.label("1. Enable YouTube Data API v3 in your Google Cloud project.");
        ui.label("2. Create an OAuth client of type Desktop app; configure consent and test users if needed.");
        ui.label("3. Download its JSON and select it above. Keep it outside your project.");
        ui.label(
            "4. Connect in your browser and choose the account/channel you intend to upload to.",
        );
        ui.hyperlink_to(
            "Open Google Cloud credentials",
            "https://console.cloud.google.com/apis/credentials",
        );
        ui.small("Only upload permission is requested. Client and tokens stay in the operating-system keychain.");
    }
    fn upload_form(
        &mut self,
        ui: &mut egui::Ui,
        project: &mut Workspace,
        ctx: &egui::Context,
        available: bool,
    ) {
        self.connection_label(ui);
        if let Some(pending) = self.account.as_ref().and_then(|a| a.pending.clone()) {
            ui.strong("Pending upload");
            ui.label(format!(
                "{} · {} · {}",
                pending.title,
                privacy_name(pending.privacy),
                bytes(pending.total_bytes)
            ));
            ui.label(&pending.relative_path);
            ui.label("Resume checks this project, the unchanged MP4 and the same Google connection. It does not create a new upload.");
            let shown_connection = self.account.as_ref().and_then(|a| a.connection.clone());
            let mut reviewed = self
                .resume_reviewed
                .as_ref()
                .is_some_and(|id| Some(id) == shown_connection.as_ref());
            if ui
                .add_enabled(
                    !self.busy() && shown_connection.is_some(),
                    egui::Checkbox::new(
                        &mut reviewed,
                        "I reviewed the pending upload and current Google connection.",
                    ),
                )
                .changed()
            {
                self.resume_reviewed = if reviewed {
                    shown_connection.clone()
                } else {
                    None
                };
            }
            ui.small("Forgetting only removes local resumption state. It does not delete a remote video.");
            return;
        }
        ui.horizontal_wrapped(|ui| {
            if ui
                .add_enabled(
                    available && project.store().is_some(),
                    egui::Button::new("Choose rendered MP4…"),
                )
                .clicked()
            {
                self.choose_export(project, ctx);
            }
            ui.label(self.export.as_deref().unwrap_or("No MP4 selected"));
        });
        let mut changed = false;
        let label = ui.label("Video title");
        changed |= ui
            .add(
                egui::TextEdit::singleline(&mut self.title)
                    .char_limit(100)
                    .desired_width(f32::INFINITY)
                    .hint_text("Title shown on YouTube"),
            )
            .labelled_by(label.id)
            .changed();
        let label = ui.label("Description");
        changed |= ui
            .add(
                egui::TextEdit::multiline(&mut self.description)
                    .char_limit(5000)
                    .desired_rows(3)
                    .desired_width(f32::INFINITY),
            )
            .labelled_by(label.id)
            .changed();
        ui.horizontal_wrapped(|ui| {
            ui.label("Privacy");
            for (value, name) in [
                (Privacy::Private, "Private"),
                (Privacy::Unlisted, "Unlisted"),
                (Privacy::Public, "Public"),
            ] {
                changed |= ui.radio_value(&mut self.privacy, value, name).changed();
            }
        });
        if changed {
            self.reviewed = None;
        }
        let shown_connection = self.account.as_ref().and_then(|a| a.connection.clone());
        let mut reviewed = self
            .reviewed
            .as_ref()
            .is_some_and(|id| Some(id) == shown_connection.as_ref());
        if ui
            .add_enabled(
                !self.busy() && shown_connection.is_some() && self.export.is_some(),
                egui::Checkbox::new(
                    &mut reviewed,
                    "I reviewed this export, its title/privacy and the Google connection.",
                ),
            )
            .changed()
        {
            self.reviewed = if reviewed { shown_connection } else { None };
        }
    }
    fn primary_actions(
        &mut self,
        ui: &mut egui::Ui,
        project: &mut Workspace,
        ctx: &egui::Context,
        available: bool,
    ) {
        match self.pane {
            Pane::Connection => {}
            Pane::Upload if self.account.as_ref().is_some_and(|a| a.pending.is_some()) => {
                let reviewed = self
                    .account
                    .as_ref()
                    .and_then(|a| a.connection.as_ref())
                    .is_some_and(|connection| self.resume_reviewed.as_ref() == Some(connection));
                ui.horizontal_wrapped(|ui| {
                    if ui
                        .add_enabled(
                            available && project.store().is_some() && reviewed,
                            egui::Button::new("Resume pending upload"),
                        )
                        .clicked()
                    {
                        self.resume(project, ctx);
                    }
                    if ui
                        .add_enabled(
                            available,
                            egui::Button::new("Forget pending upload locally"),
                        )
                        .clicked()
                    {
                        self.account_action(AccountAction::ForgetPending, project, ctx);
                    }
                });
            }
            Pane::Upload => {
                let validation = self.publish_request();
                if !self.busy()
                    && let Err(reason) = &validation
                {
                    ui.small(reason);
                }
                ui.horizontal_wrapped(|ui| {
                    if ui
                        .add_enabled(
                            available && project.store().is_some() && validation.is_ok(),
                            egui::Button::new("Publish video to YouTube"),
                        )
                        .clicked()
                    {
                        self.publish(project, ctx);
                    }
                    if let Some(published) = &self.published
                        && ui.button("Open uploaded video").clicked()
                    {
                        ctx.open_url(egui::OpenUrl::new_tab(&published.video.url));
                    }
                });
                if let Some(published) = &self.published {
                    ui.label(format!(
                        "Uploaded: {} · actual privacy: {}",
                        published.title,
                        privacy_name(published.video.privacy)
                    ));
                    if published.video.privacy != published.requested_privacy {
                        ui.small("Google returned a different privacy status than requested.");
                    }
                }
            }
            Pane::Import => {
                let validation = self.import_request();
                if !self.busy()
                    && let Err(reason) = &validation
                {
                    ui.small(reason);
                }
                if ui
                    .add_enabled(
                        available && project.store().is_some() && validation.is_ok(),
                        egui::Button::new("Import selected clip"),
                    )
                    .clicked()
                {
                    self.import(project, ctx);
                }
            }
        }
    }
    fn import_form(
        &mut self,
        ui: &mut egui::Ui,
        _project: &mut Workspace,
        _ctx: &egui::Context,
        _available: bool,
    ) {
        ui.label("Import one public YouTube clip. A Google connection is not required.");
        let label = ui.label("Video URL");
        ui.add(
            egui::TextEdit::singleline(&mut self.url)
                .char_limit(2048)
                .desired_width(f32::INFINITY)
                .hint_text("https://www.youtube.com/watch?v=…"),
        )
        .labelled_by(label.id);
        ui.horizontal_wrapped(|ui| {
            let label = ui.label("Start (seconds)");
            ui.add_sized(
                [120., 44.],
                egui::TextEdit::singleline(&mut self.start).char_limit(20),
            )
            .labelled_by(label.id);
            let label = ui.label("End (seconds)");
            ui.add_sized(
                [120., 44.],
                egui::TextEdit::singleline(&mut self.end).char_limit(20),
            )
            .labelled_by(label.id);
        });
        ui.horizontal_wrapped(|ui| {
            ui.label("Download limit");
            ui.add(
                egui::DragValue::new(&mut self.limit_mib)
                    .range(1..=4096)
                    .suffix(" MiB"),
            );
            ui.label("Time limit");
            ui.add(
                egui::DragValue::new(&mut self.limit_minutes)
                    .range(1..=30)
                    .suffix(" minutes"),
            );
        });
        ui.label(format!("This operation may use up to {} and {} minutes. Public clips only; no playlists or browser cookies.", bytes(u64::from(self.limit_mib)*1024*1024), self.limit_minutes));
        ui.small("Native Linux needs installed yt-dlp and FFmpeg. A failed import leaves the current sequence unchanged.");
    }
    fn draw_progress(&self, ui: &mut egui::Ui) {
        match &self.progress {
            Some(Progress::Upload(p)) => {
                let phase = match p.phase {
                    UploadPhase::Preparing => "Checking MP4",
                    UploadPhase::Starting => "Starting upload",
                    UploadPhase::Uploading => "Uploading",
                    UploadPhase::Recovering => "Checking saved upload",
                    UploadPhase::Complete => "Complete",
                };
                if p.total_bytes > 0 {
                    ui.add(
                        egui::ProgressBar::new(
                            (p.acknowledged_bytes as f64 / p.total_bytes as f64).clamp(0., 1.)
                                as f32,
                        )
                        .text(format!(
                            "{phase}: {} / {} acknowledged",
                            bytes(p.acknowledged_bytes),
                            bytes(p.total_bytes)
                        )),
                    );
                } else {
                    ui.label(phase);
                }
            }
            Some(Progress::Import(p)) => {
                let phase = match p.phase {
                    YoutubeImportPhase::Downloading => "Downloading",
                    YoutubeImportPhase::Importing => "Adding clip",
                    YoutubeImportPhase::Complete => "Imported",
                };
                ui.label(format!(
                    "{phase}: {} written · {} byte limit",
                    bytes(p.written_bytes),
                    bytes(p.byte_limit)
                ));
            }
            None => {}
        }
    }
}
fn with_client(
    cached: Option<YoutubeClient>,
    op: impl FnOnce(&YoutubeClient) -> Result<Outcome, String>,
) -> Completion {
    match cached
        .map(Ok)
        .unwrap_or_else(|| YoutubeClient::system().map_err(|e| e.to_string()))
    {
        Ok(client) => {
            let result = op(&client);
            Completion {
                client: Some(client),
                result,
            }
        }
        Err(error) => Completion {
            client: None,
            result: Err(error),
        },
    }
}
fn lock_store(
    store: &Store,
) -> Result<std::sync::MutexGuard<'_, video_core::ProjectStore>, String> {
    store
        .lock()
        .map_err(|_| "Project worker stopped; reopen the project before continuing.".into())
}
fn progress(latest: &Mutex<Option<Progress>>, ctx: &egui::Context, update: Progress) {
    if let Ok(mut latest) = latest.lock() {
        *latest = Some(update);
    }
    ctx.request_repaint();
}
fn privacy_name(privacy: Privacy) -> &'static str {
    match privacy {
        Privacy::Private => "Private",
        Privacy::Unlisted => "Unlisted",
        Privacy::Public => "Public",
    }
}
fn bytes(n: u64) -> String {
    if n >= 1024 * 1024 * 1024 {
        format!("{:.2} GiB", n as f64 / (1024. * 1024. * 1024.))
    } else if n >= 1024 * 1024 {
        format!("{:.1} MiB", n as f64 / (1024. * 1024.))
    } else if n >= 1024 {
        format!("{:.1} KiB", n as f64 / 1024.)
    } else {
        format!("{n} B")
    }
}
fn milliseconds(text: &str) -> Result<u64, String> {
    let text = text.trim();
    let (whole, fraction) = text.split_once('.').unwrap_or((text, ""));
    if whole.is_empty()
        || !whole.bytes().all(|b| b.is_ascii_digit())
        || fraction.len() > 3
        || !fraction.bytes().all(|b| b.is_ascii_digit())
    {
        return Err("Use seconds with up to three decimal places, such as 12.500.".into());
    }
    let whole = whole
        .parse::<u64>()
        .ok()
        .and_then(|v| v.checked_mul(1000))
        .ok_or("Clip time is too large.")?;
    let fraction = if fraction.is_empty() {
        0
    } else {
        fraction.parse::<u64>().map_err(|_| "Invalid clip time.")?
            * 10_u64.pow((3 - fraction.len()) as u32)
    };
    whole
        .checked_add(fraction)
        .ok_or_else(|| "Clip time is too large.".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use video_core::Edit;
    const REVIEW: &str = "I reviewed this export, its title/privacy and the Google connection.";
    fn connection(name: &str) -> ConnectionId {
        serde_json::from_value(serde_json::Value::String(name.into())).unwrap()
    }
    fn account() -> YoutubeStatus {
        YoutubeStatus {
            configured: true,
            connection: Some(connection("synthetic-ui-connection")),
            pending: None,
        }
    }
    fn ready() -> YoutubeUi {
        YoutubeUi {
            open: true,
            pane: Pane::Upload,
            account: Some(account()),
            export: Some("exports/fixture.mp4".into()),
            title: "Fixture title".into(),
            ..Default::default()
        }
    }
    fn context() -> egui::Context {
        let ctx = egui::Context::default();
        crate::configure(&ctx);
        ctx.enable_accesskit();
        ctx
    }
    fn frame(
        ui: &mut YoutubeUi,
        project: &mut Workspace,
        ctx: &egui::Context,
        events: Vec<egui::Event>,
    ) -> egui::FullOutput {
        ctx.run(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(1160., 800.),
                )),
                focused: true,
                events,
                ..Default::default()
            },
            |ctx| ui.dialog(project, &CaptureUi::default(), ctx),
        )
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
    fn pending(ui: &mut YoutubeUi, project: &mut Workspace) -> mpsc::SyncSender<Completion> {
        let (tx, rx) = mpsc::sync_channel(1);
        ui.pending = Some(Pending {
            receiver: rx,
            latest: Arc::new(Mutex::new(None)),
            cancel: Cancellation::default(),
            epoch: ui.epoch,
            project: project.folder.clone(),
        });
        project.set_output_busy(true);
        tx
    }
    #[test]
    fn default_open_and_project_change_do_not_load_credentials_or_start_workers() {
        let mut ui = YoutubeUi::default();
        assert_eq!(ui.privacy, Privacy::Unlisted);
        ui.show_window();
        let mut project = Workspace::default();
        let ctx = context();
        let output = frame(&mut ui, &mut project, &ctx, vec![]);
        assert!(
            !output
                .platform_output
                .commands
                .iter()
                .any(|command| matches!(command, egui::OutputCommand::OpenUrl(_)))
        );
        assert!(ui.client.is_none());
        assert!(ui.account.is_none());
        assert!(!ui.busy());
        assert!(!project.busy());
        ui.export = Some("exports/old.mp4".into());
        ui.reviewed = Some(connection("synthetic"));
        ui.project_changed();
        assert!(ui.export.is_none());
        assert!(ui.reviewed.is_none());
        assert!(ui.account.is_none());
        assert!(!ui.busy());
    }
    #[test]
    fn publication_requires_review_of_the_current_displayed_connection() {
        let mut ui = ready();
        assert!(ui.publish_request().is_err());
        ui.reviewed = Some(connection("earlier-ui-connection"));
        assert!(ui.publish_request().is_err());
        ui.reviewed = ui.account.as_ref().unwrap().connection.clone();
        assert_eq!(ui.publish_request().unwrap().privacy, Privacy::Unlisted);
        ui.account.as_mut().unwrap().connection = Some(connection("new-ui-connection"));
        assert!(ui.publish_request().is_err());
        assert_eq!(milliseconds("12.500").unwrap(), 12500);
        assert_eq!(milliseconds("0.001").unwrap(), 1);
        for text in ["-1", "NaN", "1.0001", "1.2.3", "18446744073709551615"] {
            assert!(milliseconds(text).is_err());
        }
    }
    #[test]
    fn upload_ack_preserves_newer_form_and_editor_and_worker_loss_releases_guard() {
        let mut ui = ready();
        let mut project = Workspace::default();
        let ctx = context();
        let mut capture = CaptureUi::default();
        let tx = pending(&mut ui, &mut project);
        ui.pending.as_ref().unwrap().cancel.cancel();
        ui.title = "Newer title".into();
        ui.description = "Newer description".into();
        ui.privacy = Privacy::Public;
        project.edit(Edit::Rename("Typed while uploading".into()));
        tx.send(Completion {
            client: None,
            result: Ok(Outcome::Published {
                result: Ok(Published {
                    video: UploadedVideo {
                        id: "synthetic-id".into(),
                        url: "https://www.youtube.com/watch?v=synthetic-id".into(),
                        privacy: Privacy::Private,
                    },
                    title: "Submitted title".into(),
                    requested_privacy: Privacy::Unlisted,
                }),
                account: Some(account()),
            }),
        })
        .unwrap();
        ui.poll(&mut project, &mut capture, &ctx);
        assert_eq!(ui.title, "Newer title");
        assert_eq!(ui.description, "Newer description");
        assert_eq!(ui.privacy, Privacy::Public);
        assert_eq!(project.studio().name(), "Typed while uploading");
        assert!(project.dirty());
        assert!(!project.busy());
        assert_eq!(
            ui.published.as_ref().unwrap().video.privacy,
            Privacy::Private
        );
        assert_eq!(ui.published.as_ref().unwrap().title, "Submitted title");
        let tx = pending(&mut ui, &mut project);
        drop(tx);
        ui.poll(&mut project, &mut capture, &ctx);
        assert!(!ui.busy());
        assert!(!project.busy());
        assert!(ui.error.as_deref().unwrap().contains("worker stopped"));
    }
    #[test]
    fn late_canceled_import_keeps_published_source_without_queueing_append() {
        let temp = tempfile::tempdir().unwrap();
        let source = temp.path().join("published.mp4");
        std::fs::write(&source, b"synthetic published-source fixture").unwrap();
        // This synthetic typed outcome never reaches media append/probing.
        // Generated core integrations separately verify actual MP4 publication.
        let media: ImportedMedia = serde_json::from_value(serde_json::json!({
            "relative": "media/published.mp4", "name": "Completed import",
            "info": {"duration_ms":1000,"width":160,"height":90,"has_audio":true}
        }))
        .unwrap();
        let mut ui = ready();
        let mut project = Workspace::default();
        let mut capture = CaptureUi::default();
        let ctx = context();
        let tx = pending(&mut ui, &mut project);
        ui.pending.as_ref().unwrap().cancel.cancel();
        tx.send(Completion {
            client: None,
            result: Ok(Outcome::Imported(media)),
        })
        .unwrap();
        ui.poll(&mut project, &mut capture, &ctx);
        assert!(!capture.busy());
        assert!(project.studio().clips().is_empty());
        assert!(!project.busy());
        assert!(!ui.busy());
        assert_eq!(
            std::fs::read(source).unwrap(),
            b"synthetic published-source fixture"
        );
        assert!(ui.status.contains("Add media later"));
    }
    #[test]
    fn earlier_project_picker_result_is_not_applied_to_new_project_review() {
        let mut ui = ready();
        let mut project = Workspace::default();
        let ctx = context();
        let tx = pending(&mut ui, &mut project);
        ui.project_changed();
        ui.title = "New project draft".into();
        tx.send(Completion {
            client: None,
            result: Ok(Outcome::ExportSelected("exports/earlier.mp4".into())),
        })
        .unwrap();
        ui.poll(&mut project, &mut CaptureUi::default(), &ctx);
        assert!(ui.export.is_none());
        assert_eq!(ui.title, "New project draft");
        assert!(!project.busy());
    }
    #[test]
    fn essential_controls_fit_minimum_window_and_primary_actions_stay_outside_scroll() {
        let ctx = context();
        let mut project = Workspace::default();
        for pane in [Pane::Connection, Pane::Upload, Pane::Import] {
            let mut ui = ready();
            ui.pane = pane;
            let output = frame(&mut ui, &mut project, &ctx, vec![]);
            let labels: &[&str] = match pane {
                Pane::Connection => &[
                    "Load saved connection",
                    "Choose Desktop client JSON…",
                    "Connect in browser",
                    "Close YouTube",
                ],
                Pane::Upload => &[
                    "Choose rendered MP4…",
                    "Publish video to YouTube",
                    "Close YouTube",
                ],
                Pane::Import => &["Import selected clip", "Close YouTube"],
            };
            let update = output.platform_output.accesskit_update.unwrap();
            for label in labels {
                let node = update
                    .nodes
                    .iter()
                    .find(|(_, n)| n.label() == Some(*label))
                    .unwrap_or_else(|| panic!("Missing {label}"));
                let bounds = node.1.bounds().unwrap();
                assert!(
                    bounds.x0 >= 0. && bounds.x1 <= 1160. && bounds.y0 >= 0. && bounds.y1 <= 800.,
                    "{label}: {bounds:?}"
                );
            }
        }
    }
    #[test]
    fn real_review_checkbox_requires_pointer_confirmation_without_opening_browser() {
        let mut ui = ready();
        let mut project = Workspace::default();
        let ctx = context();
        // A centered egui modal settles its position after measuring its first
        // frame. Use the settled accessibility bounds for the actual pointer.
        frame(&mut ui, &mut project, &ctx, vec![]);
        let output = frame(&mut ui, &mut project, &ctx, vec![]);
        let update = output.platform_output.accesskit_update.unwrap();
        let bounds = update
            .nodes
            .iter()
            .find(|(_, n)| n.label() == Some(REVIEW))
            .unwrap()
            .1
            .bounds()
            .unwrap();
        let pos = egui::pos2(
            ((bounds.x0 + bounds.x1) / 2.) as f32,
            ((bounds.y0 + bounds.y1) / 2.) as f32,
        );
        frame(&mut ui, &mut project, &ctx, pointer(pos, true));
        let output = frame(&mut ui, &mut project, &ctx, pointer(pos, false));
        assert!(ui.reviewed.is_some());
        assert!(ui.publish_request().is_ok());
        assert!(
            !output
                .platform_output
                .commands
                .iter()
                .any(|command| matches!(command, egui::OutputCommand::OpenUrl(_)))
        );
        assert!(ui.client.is_none());
        assert!(!ui.busy());
    }
}
