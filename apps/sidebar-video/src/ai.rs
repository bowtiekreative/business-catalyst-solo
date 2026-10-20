//! Local model review. Worker results are drafts until an explicit approval.
use crate::{Workspace, workspace::Store};
use eframe::egui;
use std::{
    path::PathBuf,
    sync::{Arc, mpsc},
    time::Duration,
};
use video_core::{
    Edit, Orientation, OverlayContent, SlotKey, Studio,
    ai::{self, Evidence, LocalModel, Proposal, SearchHit, TokenUsage},
    media::Cancellation,
    renderer::{ActiveOverlay, OverlayAssets, OverlayRenderer, RenderOptions, RgbaFrame},
};
struct Hit {
    title: String,
    url: String,
    evidence: Option<Arc<Evidence>>,
    texture: Option<egui::TextureHandle>,
    error: Option<String>,
}
enum Event {
    Status(String),
    Hit(SearchHit, Result<Arc<Evidence>, String>),
    Rebuilt(Result<Proposal, String>, Result<TokenUsage, String>),
    Preview(Result<RgbaFrame, String>),
    Original(SlotKey, Result<OverlayContent, String>),
    Html(Box<Studio>, Result<ai::HtmlExport, String>),
    Usage(Result<TokenUsage, String>),
    Done,
}
struct Pending {
    receiver: mpsc::Receiver<Event>,
    cancel: Cancellation,
    html: bool,
}
pub struct AiUi {
    pub open: bool,
    pending: Option<Pending>,
    hits: Vec<Hit>,
    selected: Option<usize>,
    query: String,
    url: String,
    count: u8,
    model: String,
    slot: u8,
    json: String,
    preview: Option<egui::TextureHandle>,
    validated: Option<Proposal>,
    pub error: Option<String>,
    status: String,
    tokens: Option<TokenUsage>,
}
impl Default for AiUi {
    fn default() -> Self {
        Self {
            open: false,
            pending: None,
            hits: Vec::new(),
            selected: None,
            query: String::new(),
            url: String::new(),
            count: 4,
            model: LocalModel::default().name,
            slot: 1,
            json: String::new(),
            preview: None,
            validated: None,
            error: None,
            status: String::new(),
            tokens: None,
        }
    }
}
impl AiUi {
    pub fn busy(&self) -> bool {
        self.pending.is_some()
    }
    pub fn show_window(&mut self, project: &Workspace, ctx: &egui::Context) {
        self.open = true;
        self.error = None;
        if let Some(store) = project.store() {
            self.start(ctx, false, move |tx, _, ctx| {
                let result =
                    read_store(&store).and_then(|s| ai::usage(&s).map_err(|e| e.to_string()));
                let _ = tx.send(Event::Usage(result));
                ctx.request_repaint();
            });
        }
    }
    fn evidence(&self) -> Option<Arc<Evidence>> {
        self.selected
            .and_then(|i| self.hits.get(i))
            .and_then(|h| h.evidence.clone())
    }
    fn start(
        &mut self,
        ctx: &egui::Context,
        html: bool,
        task: impl FnOnce(mpsc::SyncSender<Event>, Cancellation, egui::Context) + Send + 'static,
    ) {
        if self.busy() {
            return;
        }
        let (tx, receiver) = mpsc::sync_channel(16);
        let done = tx.clone();
        let cancel = Cancellation::default();
        let token = cancel.clone();
        let ctx = ctx.clone();
        match std::thread::Builder::new()
            .name("storyfold-ai-review".into())
            .spawn(move || {
                task(tx, token, ctx.clone());
                let _ = done.send(Event::Done);
                ctx.request_repaint();
            }) {
            Ok(_) => {
                self.pending = Some(Pending {
                    receiver,
                    cancel,
                    html,
                });
                self.error = None;
            }
            Err(e) => self.error = Some(e.to_string()),
        }
    }
    fn search(&mut self, project: &Workspace, ctx: &egui::Context) {
        let Some(store) = project.store() else {
            return;
        };
        let query = self.query.clone();
        let count = self.count;
        self.clear_results();
        self.status = "Searching public pages…".into();
        self.start(ctx, false, move |tx, cancel, ctx| {
            let result = (|| {
                let store = read_store(&store)?;
                let hits = ai::search(&query, count, &cancel).map_err(|e| e.to_string())?;
                let total = hits.len();
                for (index, hit) in hits.into_iter().enumerate() {
                    if cancel.is_cancelled() {
                        break;
                    }
                    let _ = tx.send(Event::Status(format!(
                        "Capturing page {} of {total}…",
                        index + 1
                    )));
                    ctx.request_repaint();
                    let result = Evidence::capture(&store, &hit.url, &cancel)
                        .map(Arc::new)
                        .map_err(|e| e.to_string());
                    let _ = tx.send(Event::Hit(hit, result));
                    ctx.request_repaint();
                }
                Ok::<_, String>(())
            })();
            if let Err(e) = result {
                let _ = tx.send(Event::Status(e));
            }
        });
    }
    fn capture(&mut self, project: &Workspace, path: Option<PathBuf>, ctx: &egui::Context) {
        let Some(store) = project.store() else {
            return;
        };
        let url = self.url.clone();
        self.clear_results();
        self.status = "Preparing screenshot…".into();
        self.start(ctx, false, move |tx, cancel, ctx| {
            let title = if path.is_some() {
                "Dropped screenshot".to_owned()
            } else {
                url.clone()
            };
            let result = read_store(&store).and_then(|store| {
                if let Some(path) = path {
                    Evidence::from_file(&store, &path, &cancel)
                } else {
                    Evidence::capture(&store, &url, &cancel)
                }
                .map(Arc::new)
                .map_err(|e| e.to_string())
            });
            let _ = tx.send(Event::Hit(
                SearchHit {
                    title,
                    url,
                    description: String::new(),
                },
                result,
            ));
            ctx.request_repaint();
        });
    }
    fn choose(&mut self, project: &Workspace, ctx: &egui::Context) {
        let Some(store) = project.store() else {
            return;
        };
        self.clear_results();
        self.status = "Choose a screenshot…".into();
        self.start(ctx, false, move |tx, cancel, ctx| {
            if let Some(path) = rfd::FileDialog::new()
                .set_title("Choose screenshot")
                .add_filter("Screenshot", &["png", "jpg", "jpeg", "webp"])
                .pick_file()
            {
                let result = read_store(&store).and_then(|store| {
                    Evidence::from_file(&store, &path, &cancel)
                        .map(Arc::new)
                        .map_err(|e| e.to_string())
                });
                let _ = tx.send(Event::Hit(
                    SearchHit {
                        title: "Local screenshot".into(),
                        url: String::new(),
                        description: String::new(),
                    },
                    result,
                ));
                ctx.request_repaint();
            }
        });
    }
    fn rebuild(&mut self, project: &Workspace, ctx: &egui::Context) {
        let (Some(store), Some(evidence)) = (project.store(), self.evidence()) else {
            return;
        };
        let model = LocalModel {
            name: self.model.clone(),
        };
        self.preview = None;
        self.validated = None;
        self.json.clear();
        self.status = "Rebuilding with the installed local vision model…".into();
        self.start(ctx, false, move |tx, cancel, ctx| {
            match read_store(&store) {
                Ok(store) => {
                    let proposal = model
                        .rebuild(&store, &evidence, &cancel)
                        .map_err(|e| e.to_string());
                    let usage = ai::usage(&store).map_err(|e| e.to_string());
                    let _ = tx.send(Event::Rebuilt(proposal, usage));
                }
                Err(e) => {
                    let _ = tx.send(Event::Rebuilt(Err(e), Err("Usage unavailable".into())));
                }
            }
            ctx.request_repaint();
        });
    }
    fn validate(&mut self) -> Option<Proposal> {
        let evidence = self.evidence()?;
        match Proposal::parse(&self.json, &evidence) {
            Ok(p) => {
                self.error = None;
                Some(p)
            }
            Err(e) => {
                self.error = Some(e.to_string());
                None
            }
        }
    }
    fn preview(&mut self, orientation: Orientation, ctx: &egui::Context) {
        let Some(proposal) = self.validate() else {
            return;
        };
        let content = proposal.content();
        self.validated = Some(proposal);
        self.status = "Rendering proposed card…".into();
        self.start(ctx, false, move |tx, _, ctx| {
            let result = (|| {
                OverlayRenderer::new()?.render_live(
                    orientation.output_size(),
                    &[ActiveOverlay {
                        content: &content,
                        elapsed_ms: 1000,
                        duration_ms: 5000,
                    }],
                    &OverlayAssets::new(),
                    RenderOptions {
                        reduced_motion: true,
                    },
                )
            })();
            let _ = tx.send(Event::Preview(result.map_err(|e| e.to_string())));
            ctx.request_repaint();
        });
    }
    fn original(&mut self, project: &Workspace, ctx: &egui::Context) {
        let (Some(store), Some(evidence)) = (project.store(), self.evidence()) else {
            return;
        };
        let slot = SlotKey::try_from(self.slot).expect("bounded slot");
        self.status = "Approving original screenshot…".into();
        self.start(ctx, false, move |tx, cancel, ctx| {
            let result = read_store(&store).and_then(|s| {
                evidence
                    .approve_original(&s, "Original screenshot", &cancel)
                    .map_err(|e| e.to_string())
            });
            let _ = tx.send(Event::Original(slot, result));
            ctx.request_repaint();
        });
    }
    fn export_html(&mut self, project: &mut Workspace, ctx: &egui::Context) {
        let Some(store) = project.store() else {
            return;
        };
        let slot = SlotKey::try_from(self.slot).expect("bounded slot");
        let snapshot = project.studio().clone();
        self.status = "Exporting approved card HTML…".into();
        self.start(ctx, true, move |tx, cancel, ctx| {
            let result = store
                .lock()
                .map_err(|_| "Project worker failed".to_owned())
                .and_then(|mut s| {
                    ai::export_html(&mut s, &snapshot, slot, &cancel).map_err(|e| e.to_string())
                });
            let _ = tx.send(Event::Html(Box::new(snapshot), result));
            ctx.request_repaint();
        });
        if self.busy() {
            project.set_output_busy(true);
        }
    }
    fn clear_results(&mut self) {
        self.hits.clear();
        self.selected = None;
        self.json.clear();
        self.preview = None;
        self.validated = None;
    }
    fn select(&mut self, index: usize) {
        if self.selected != Some(index) {
            self.selected = Some(index);
            self.json.clear();
            self.preview = None;
            self.validated = None;
            self.error = None;
        }
    }
    pub fn poll(&mut self, project: &mut Workspace, ctx: &egui::Context) {
        let mut disconnected = false;
        let mut events = Vec::new();
        if let Some(p) = &self.pending {
            loop {
                match p.receiver.try_recv() {
                    Ok(event) => events.push(event),
                    Err(mpsc::TryRecvError::Empty) => break,
                    Err(mpsc::TryRecvError::Disconnected) => {
                        disconnected = true;
                        break;
                    }
                }
            }
        }
        let cancelled = self
            .pending
            .as_ref()
            .is_some_and(|p| p.cancel.is_cancelled());
        for event in events {
            if cancelled
                && matches!(
                    &event,
                    Event::Preview(_) | Event::Original(_, _) | Event::Rebuilt(_, _)
                )
            {
                self.status = "Operation canceled; no card was approved.".into();
                continue;
            }
            match event {
                Event::Status(s) => self.status = s,
                Event::Hit(hit, result) => {
                    let (evidence, texture, error) = match result {
                        Ok(e) => {
                            let f = e.frame();
                            let texture = ctx.load_texture(
                                "evidence-screenshot",
                                egui::ColorImage::from_rgba_unmultiplied(
                                    [f.width as usize, f.height as usize],
                                    &f.pixels,
                                ),
                                egui::TextureOptions::LINEAR,
                            );
                            (Some(e), Some(texture), None)
                        }
                        Err(e) => (None, None, Some(e)),
                    };
                    self.hits.push(Hit {
                        title: hit.title,
                        url: hit.url,
                        evidence,
                        texture,
                        error,
                    });
                    if self.selected.is_none()
                        && self.hits.last().is_some_and(|h| h.evidence.is_some())
                    {
                        self.selected = Some(self.hits.len() - 1);
                    }
                }
                Event::Rebuilt(result, usage) => {
                    match result {
                        Ok(p) => {
                            self.json = serde_json_string(&p);
                            self.validated = Some(p);
                            self.status =
                                "Review the source and exact copied text. Nothing is approved yet."
                                    .into();
                        }
                        Err(e) => self.error = Some(e),
                    }
                    if let Ok(tokens) = usage {
                        self.tokens = Some(tokens);
                    }
                }
                Event::Preview(result) => match result {
                    Ok(f) => {
                        self.preview = Some(ctx.load_texture(
                            "proposed-card",
                            egui::ColorImage::from_rgba_unmultiplied(
                                [f.width as usize, f.height as usize],
                                &f.pixels,
                            ),
                            egui::TextureOptions::LINEAR,
                        ));
                        self.status =
                            "Review the screenshot and rebuilt card before approval.".into();
                    }
                    Err(e) => self.error = Some(e),
                },
                Event::Original(slot, result) => {
                    match result {
                        Ok(content) => {
                            if project
                                .edit(Edit::SetSlotContent { slot, content })
                                .is_some()
                            {
                                self.status="Original screenshot approved. Save the project to keep this card.".into();
                                self.json.clear();
                                self.preview = None;
                            } else {
                                self.error = project.error.clone();
                            }
                        }
                        Err(e) => self.error = Some(e),
                    }
                }
                Event::Html(snapshot, result) => {
                    project.set_output_busy(false);
                    match result {
                        Ok(result) => {
                            if result.project_saved {
                                project.acknowledge_snapshot(*snapshot);
                            }
                            self.status = format!("Saved {}", result.relative_path);
                            self.error = result.version_error;
                        }
                        Err(e) => self.error = Some(e),
                    }
                }
                Event::Usage(result) => match result {
                    Ok(tokens) => self.tokens = Some(tokens),
                    Err(e) => self.error = Some(e),
                },
                Event::Done => {
                    if self.pending.as_ref().is_some_and(|p| p.html) {
                        project.set_output_busy(false);
                    }
                    self.pending = None;
                }
            }
        }
        if disconnected && self.pending.is_some() {
            if self.pending.take().is_some_and(|p| p.html) {
                project.set_output_busy(false);
            }
            self.error = Some("AI worker stopped before completing its request.".into());
        }
        if self.busy() {
            ctx.request_repaint_after(Duration::from_millis(100));
        }
    }
    pub fn dialog(
        &mut self,
        project: &mut Workspace,
        ctx: &egui::Context,
    ) -> Option<(SlotKey, OverlayContent)> {
        if !self.open {
            return None;
        }
        let mut handoff = None;
        let mut close = false;
        if !self.busy()
            && let Some(path) =
                ctx.input(|i| i.raw.dropped_files.iter().find_map(|f| f.path.clone()))
        {
            self.capture(project, Some(path), ctx);
        }
        let modal=egui::Modal::new(egui::Id::new("local-ai-review")).show(ctx,|ui|{
            ui.set_width(1000.);ui.heading("Build a card with local AI");
            ui.label("Public search: Bing RSS. Screenshot rebuild: installed Ollama vision model on this computer.");
            ui.add_enabled_ui(!self.busy(),|ui|{ui.horizontal(|ui|{ui.label("Local model");ui.add(egui::TextEdit::singleline(&mut self.model).char_limit(120));ui.label("Card slot");ui.add(egui::DragValue::new(&mut self.slot).range(1..=9));ui.label("Results");ui.add(egui::DragValue::new(&mut self.count).range(1..=12));});});
            ui.add_enabled_ui(!self.busy(),|ui|{
                ui.horizontal(|ui|{ui.add_sized([680.,44.],egui::TextEdit::singleline(&mut self.query).hint_text("Find public evidence…").char_limit(300));if ui.button("Search").clicked(){self.search(project,ctx);}});
                ui.horizontal(|ui|{ui.add_sized([580.,44.],egui::TextEdit::singleline(&mut self.url).hint_text("https://public-page.example/").char_limit(2048));if ui.button("Capture URL").clicked(){self.capture(project,None,ctx);}
if ui.button("Choose screenshot…").clicked(){self.choose(project,ctx);}});
            });
            egui::ScrollArea::vertical().max_height((ctx.content_rect().height()-470.).clamp(180.,470.)).show(ui,|ui|{
                let mut selected=None;let busy=self.busy();
                for(index,hit)in self.hits.iter().enumerate(){ui.horizontal(|ui|{if let Some(t)=&hit.texture{ui.add(egui::Image::new(t).max_size(egui::vec2(128.,80.)));}ui.vertical(|ui|{if ui.add_enabled(!busy,egui::Button::new(&hit.title).selected(self.selected==Some(index))).clicked(){selected=Some(index);}ui.label(&hit.url);if let Some(error)=&hit.error{ui.colored_label(crate::INK,error);}});});}
                if let Some(index)=selected{self.select(index);}
                if let Some(index)=self.selected{
                    ui.columns(2,|columns|{
                        columns[0].heading("Source screenshot");if let Some(t)=self.hits.get(index).and_then(|h|h.texture.as_ref()){columns[0].add(egui::Image::new(t).max_size(egui::vec2(470.,310.)));}
                        columns[1].heading("Rebuilt card");if let Some(t)=&self.preview{columns[1].add(egui::Image::new(t).max_size(egui::vec2(470.,310.)));}else{columns[1].label("Rebuild, then preview the proposed card.");}
                    });
                    if let Some(e)=self.evidence(){ui.collapsing("OCR evidence — check against the screenshot",|ui|{ui.label(e.text());});}
                }
                if !self.json.is_empty(){ui.label("Proposal JSON — editable; copied text must match the source OCR");if ui.add_enabled(!busy,egui::TextEdit::multiline(&mut self.json).desired_rows(7).char_limit(65536)).changed(){self.preview=None;self.validated=None;}}
            });
            ui.horizontal(|ui|{
                if ui.add_enabled(!self.busy()&&self.evidence().is_some(),egui::Button::new("Rebuild locally")).clicked(){self.rebuild(project,ctx);}
                if ui.add_enabled(!self.busy()&&!self.json.is_empty(),egui::Button::new("Preview proposal")).clicked(){self.preview(project.studio().orientation(),ctx);}
                if ui.add_enabled(!self.busy()&&self.preview.is_some(),egui::Button::new("Approve rebuilt")).clicked()&&let Some(proposal)=self.validate(){let slot=SlotKey::try_from(self.slot).expect("bounded slot");if project.edit(Edit::SetSlotContent{slot,content:proposal.content()}).is_some(){self.status="Card approved. Save the project to keep it.".into();}else{self.error=project.error.clone();}}
            });
            ui.horizontal(|ui|{
                if ui.add_enabled(!self.busy()&&self.evidence().is_some(),egui::Button::new("Approve original screenshot")).clicked(){self.original(project,ctx);}
                if ui.add_enabled(!self.busy()&&!self.json.is_empty(),egui::Button::new("Edit as manual draft")).clicked()&&let Some(proposal)=self.validate(){handoff=Some((SlotKey::try_from(self.slot).expect("bounded slot"),proposal.content()));close=true;}
                if ui.add_enabled(!self.busy(),egui::Button::new("Reject proposal")).clicked(){self.json.clear();self.preview=None;self.validated=None;self.status="Proposal rejected. Approved cards were kept.".into();}
            });
            ui.horizontal(|ui|{
                if ui.add_enabled(!self.busy()&&!project.busy(),egui::Button::new("Export approved slot as HTML")).clicked(){self.export_html(project,ctx);}
                if ui.add_enabled(!self.busy(),egui::Button::new("Close review")).clicked(){close=true;}
                if self.busy(){ui.spinner();if ui.button("Cancel operation").clicked()&&let Some(p)=&self.pending{p.cancel.cancel();self.status="Cancellation requested; waiting for the current local/tool request to finish.".into();}}
            });
            ui.label(&self.status);if let Some(error)=&self.error{ui.colored_label(egui::Color32::from_rgb(160,30,45),error);}
            if let Some(u)=&self.tokens{ui.label(format!("This project, UTC month: {} input + {} output tokens / {} local requests. No paid fallback.",u.input,u.output,u.requests));}
        });
        if close || modal.should_close() && !self.busy() {
            self.open = false;
            self.clear_results();
        }
        handoff
    }
}
fn read_store(store: &Store) -> Result<video_core::ProjectStore, String> {
    let root = store
        .lock()
        .map_err(|_| "Project worker failed")?
        .root()
        .to_owned();
    video_core::ProjectStore::open(root)
        .map(|(s, _)| s)
        .map_err(|e| e.to_string())
}
fn serde_json_string(value: &Proposal) -> String {
    serde_json::to_string_pretty(value).unwrap_or_default()
}
impl Drop for AiUi {
    fn drop(&mut self) {
        if let Some(p) = &self.pending {
            p.cancel.cancel();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn pending(ui: &mut AiUi, html: bool) -> mpsc::SyncSender<Event> {
        let (tx, receiver) = mpsc::sync_channel(16);
        ui.pending = Some(Pending {
            receiver,
            cancel: Cancellation::default(),
            html,
        });
        tx
    }
    #[test]
    fn cancelled_original_never_approves_a_late_card() {
        let mut ui = AiUi::default();
        let mut project = Workspace::default();
        let tx = pending(&mut ui, false);
        ui.pending.as_ref().unwrap().cancel.cancel();
        let slot = SlotKey::try_from(1).unwrap();
        tx.send(Event::Original(
            slot,
            Ok(OverlayContent::new(
                video_core::OverlayKind::Quote,
                "Late result",
            )),
        ))
        .unwrap();
        tx.send(Event::Done).unwrap();
        ui.poll(&mut project, &egui::Context::default());
        assert!(project.studio().slots()[0].content().is_none());
        assert!(!ui.busy());
    }
    #[test]
    fn html_ack_keeps_newer_draft_and_worker_loss_releases_the_guard() {
        let mut ui = AiUi::default();
        let mut project = Workspace::default();
        let snapshot = project.studio().clone();
        project.edit(Edit::Rename("Newer draft".into()));
        let tx = pending(&mut ui, true);
        project.set_output_busy(true);
        tx.send(Event::Html(
            Box::new(snapshot),
            Ok(ai::HtmlExport {
                project_saved: true,
                relative_path: "exports/card.html".into(),
                version_error: None,
            }),
        ))
        .unwrap();
        tx.send(Event::Done).unwrap();
        ui.poll(&mut project, &egui::Context::default());
        assert_eq!(project.studio().name(), "Newer draft");
        assert!(project.dirty());
        assert!(!project.busy());
        let tx = pending(&mut ui, true);
        project.set_output_busy(true);
        drop(tx);
        ui.poll(&mut project, &egui::Context::default());
        assert!(!ui.busy());
        assert!(!project.busy());
        assert!(ui.error.is_some());
    }
    #[test]
    #[allow(clippy::field_reassign_with_default)] // Drop cancels work, preventing struct-update construction.
    fn review_controls_fit_minimum_window_and_have_accessible_names() {
        let ctx = egui::Context::default();
        crate::configure(&ctx);
        ctx.enable_accesskit();
        let mut ui = AiUi::default();
        ui.open = true;
        let mut project = Workspace::default();
        let output = ctx.run(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(1160., 800.),
                )),
                ..Default::default()
            },
            |ctx| {
                let _ = ui.dialog(&mut project, ctx);
            },
        );
        let update = output.platform_output.accesskit_update.unwrap();
        for label in [
            "Rebuild locally",
            "Approve rebuilt",
            "Approve original screenshot",
            "Reject proposal",
            "Close review",
        ] {
            let node = update
                .nodes
                .iter()
                .find(|(_, n)| n.label() == Some(label))
                .unwrap_or_else(|| panic!("Missing {label}"));
            let bounds = node.1.bounds().unwrap();
            assert!(
                bounds.x0 >= 0. && bounds.x1 <= 1160. && bounds.y0 >= 0. && bounds.y1 <= 800.,
                "{label}: {bounds:?}"
            );
        }
    }
}

#[cfg(all(test, feature = "capture-test", target_os = "linux"))]
mod live_ui_tests {
    use super::*;
    use std::{fs, thread, time::Instant};
    fn frame(
        ui: &mut AiUi,
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
                events,
                ..Default::default()
            },
            |ctx| {
                ui.poll(project, ctx);
                let _ = ui.dialog(project, ctx);
            },
        )
    }
    fn wait(ui: &mut AiUi, project: &mut Workspace, ctx: &egui::Context) {
        let start = Instant::now();
        while ui.busy() {
            let _ = frame(ui, project, ctx, vec![]);
            assert!(
                start.elapsed() < Duration::from_secs(180),
                "AI UI timed out"
            );
            thread::sleep(Duration::from_millis(30));
        }
        assert!(ui.error.is_none(), "{:?}", ui.error);
    }
    fn click(ui: &mut AiUi, project: &mut Workspace, ctx: &egui::Context, label: &str) {
        let _ = frame(ui, project, ctx, vec![]);
        let output = frame(ui, project, ctx, vec![]);
        let tree = output.platform_output.accesskit_update.unwrap();
        let bounds = tree
            .nodes
            .iter()
            .find(|(_, n)| n.label() == Some(label))
            .unwrap_or_else(|| panic!("Missing {label}"))
            .1
            .bounds()
            .unwrap();
        let pos = egui::pos2(
            ((bounds.x0 + bounds.x1) / 2.) as f32,
            ((bounds.y0 + bounds.y1) / 2.) as f32,
        );
        for pressed in [true, false] {
            let _ = frame(
                ui,
                project,
                ctx,
                vec![
                    egui::Event::PointerMoved(pos),
                    egui::Event::PointerButton {
                        pos,
                        button: egui::PointerButton::Primary,
                        pressed,
                        modifiers: egui::Modifiers::NONE,
                    },
                ],
            );
        }
    }
    #[test]
    #[ignore = "Explicit egui source/rebuild/preview/approve/HTML button flow; local Qwen and generated image only, STORYFOLD_AI_TEST=1"]
    #[allow(clippy::field_reassign_with_default)] // Drop cancels worker work; struct-update moves are unavailable.
    fn generated_review_buttons_roundtrip_local_model_and_preserve_newer_edits() {
        assert_eq!(std::env::var("STORYFOLD_AI_TEST").as_deref(), Ok("1"));
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("project");
        let mut project = Workspace::test_project(&root);
        let original = fs::read(root.join("project.json")).unwrap();
        let source = root.join("buffer/generated-ui-source.png");
        let bytes = include_bytes!("../tests/fixtures/local-vision.png");
        fs::write(&source, bytes).unwrap();
        let ctx = egui::Context::default();
        crate::configure(&ctx);
        ctx.enable_accesskit();
        let mut ui = AiUi::default();
        ui.open = true;
        ui.capture(&project, Some(source.clone()), &ctx);
        wait(&mut ui, &mut project, &ctx);
        assert!(ui.evidence().is_some());
        click(&mut ui, &mut project, &ctx, "Rebuild locally");
        wait(&mut ui, &mut project, &ctx);
        assert!(!ui.json.is_empty());
        assert_eq!(fs::read(root.join("project.json")).unwrap(), original);
        assert!(
            project
                .studio()
                .slots()
                .iter()
                .all(|s| s.content().is_none())
        );
        assert_eq!(fs::read_dir(root.join("media")).unwrap().count(), 0);
        click(&mut ui, &mut project, &ctx, "Preview proposal");
        wait(&mut ui, &mut project, &ctx);
        assert!(ui.preview.is_some());
        click(&mut ui, &mut project, &ctx, "Approve rebuilt");
        assert!(project.studio().slots()[0].content().is_some());
        assert!(project.dirty());
        click(&mut ui, &mut project, &ctx, "Export approved slot as HTML");
        assert!(ui.busy());
        project.edit(Edit::Rename("Newer UI draft".into()));
        wait(&mut ui, &mut project, &ctx);
        assert_eq!(project.studio().name(), "Newer UI draft");
        assert!(project.dirty());
        assert_eq!(fs::read(&source).unwrap(), bytes);
        let html: Vec<_> = fs::read_dir(root.join("exports"))
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| e.path().extension().is_some_and(|e| e == "html"))
            .collect();
        assert_eq!(html.len(), 1);
        assert!(
            fs::read_to_string(html[0].path())
                .unwrap()
                .contains("data:image/png;base64,")
        );
        assert_eq!(ui.tokens.as_ref().unwrap().requests, 1);
        println!(
            "Actual egui AI buttons passed: generated source -> local model -> shared preview -> approval -> HTML; newer draft retained"
        );
    }
}
