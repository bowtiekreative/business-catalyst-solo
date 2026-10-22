//! Approved manual cards and bounded worker rendering. Preview and export use
//! video-core's identical RGBA path; no font shaping or image decoding in paint.
use crate::{Workspace, capture::CaptureUi, inserts::InsertEditor, workspace::Store};
use eframe::egui;
use std::{
    sync::{Arc, Mutex, mpsc},
    time::{Duration, Instant},
};
use video_core::{
    Edit, Highlight, HighlightStyle, OverlayBlock, OverlayContent, OverlayKind, OverlayMotion,
    SlotKey, Studio,
    media::{self, Cancellation},
    renderer::{ActiveOverlay, OverlayAssets, OverlayRenderer, RenderOptions, RgbaFrame},
};

struct Fire {
    slot: SlotKey,
    content: OverlayContent,
    start_ms: u64,
    duration_ms: u64,
}
enum View {
    Timeline(u64),
    Live(Vec<(OverlayContent, u64, u64)>),
}
struct Job {
    epoch: u64,
    store: Option<Store>,
    root: Option<std::path::PathBuf>,
    studio: Studio,
    view: View,
    reduced: bool,
    camera: Option<Arc<video_core::capture::PreviewFrame>>,
    cancel: Cancellation,
}
struct Worker {
    sender: mpsc::SyncSender<Job>,
    latest: Arc<Mutex<Option<RenderResult>>>,
}
type RenderResult = (u64, Result<RgbaFrame, String>);
struct Draft {
    slot: SlotKey,
    content: OverlayContent,
    selection: Option<(usize, usize, usize)>,
    insert_mode: bool,
    insert_editor: InsertEditor,
}
struct ImagePending {
    receiver: mpsc::Receiver<Result<Option<String>, String>>,
    slot: SlotKey,
    cancel: Cancellation,
}
#[derive(Default)]
pub struct OverlaysUi {
    draft: Option<Draft>,
    worker: Option<Worker>,
    image: Option<ImagePending>,
    fires: Vec<Fire>,
    preview: Option<(OverlayContent, Instant)>,
    epoch: u64,
    last_studio: Option<Studio>,
    last_live: bool,
    last_reduced: bool,
    last_root: Option<std::path::PathBuf>,
    last_frame: Option<(u64, u64)>,
    render_cancel: Cancellation,
    pub texture: Option<egui::TextureHandle>,
    pub error: Option<String>,
    pub reduced_motion: bool,
}
impl OverlaysUi {
    pub fn busy(&self) -> bool {
        self.image.is_some() || self.draft.as_ref().is_some_and(|d| d.insert_editor.busy())
    }
    pub fn editing(&self) -> bool {
        self.draft.is_some()
    }
    fn invalidate(&mut self) {
        self.render_cancel.cancel();
        self.render_cancel = Cancellation::default();
        self.epoch = self.epoch.wrapping_add(1);
        self.last_frame = None;
        self.texture = None;
    }
    pub fn edit(&mut self, slot: SlotKey, content: Option<OverlayContent>) {
        let content =
            content.unwrap_or_else(|| OverlayContent::new(OverlayKind::LowerThird, "New card"));
        self.draft = Some(Draft {
            slot,
            insert_mode: content.insert.is_some(),
            content,
            selection: None,
            insert_editor: InsertEditor::default(),
        });
        self.error = None;
    }
    pub fn fire(
        &mut self,
        slot: SlotKey,
        project: &mut Workspace,
        capture: &mut CaptureUi,
        playhead: u64,
    ) {
        self.error = None;
        let Some(content) = project
            .studio()
            .slots()
            .iter()
            .find(|s| s.key() == slot)
            .and_then(|s| s.content())
            .cloned()
        else {
            self.error = Some("Edit and approve this card before showing it.".into());
            return;
        };
        if capture.camera_running() {
            let Some(at) = capture.media_ms() else {
                return;
            };
            if !capture.taking() {
                self.error = Some("Start a take before showing a timed card.".into());
                return;
            }
            if !capture.mark(slot, content.clone(), cue_duration(&content)) {
                return;
            }
            self.fires.retain(|f| f.slot != slot);
            self.fires.push(Fire {
                slot,
                duration_ms: cue_duration(&content),
                content,
                start_ms: at,
            });
            self.invalidate();
        } else if project
            .edit(Edit::AddCueAtPlayhead {
                sequence_ms: playhead,
                slot,
                duration_ms: cue_duration(&content),
            })
            .is_none()
        {
            self.error = project.error.clone();
        }
    }
    pub fn shortcuts(
        &mut self,
        project: &mut Workspace,
        capture: &mut CaptureUi,
        playhead: u64,
        ctx: &egui::Context,
    ) {
        if self.editing() || ctx.wants_keyboard_input() {
            return;
        }
        for (number, key) in [
            egui::Key::Num1,
            egui::Key::Num2,
            egui::Key::Num3,
            egui::Key::Num4,
            egui::Key::Num5,
            egui::Key::Num6,
            egui::Key::Num7,
            egui::Key::Num8,
            egui::Key::Num9,
        ]
        .into_iter()
        .enumerate()
        {
            if ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, key)) {
                self.fire(
                    SlotKey::try_from(number as u8 + 1).expect("nine slots"),
                    project,
                    capture,
                    playhead,
                );
            }
        }
    }
    pub fn tick(
        &mut self,
        project: &Workspace,
        capture: &CaptureUi,
        playhead: u64,
        ctx: &egui::Context,
    ) {
        let live = capture.camera_running();
        let root = project.folder.clone();
        if self.last_studio.as_ref() != Some(project.studio())
            || self.last_live != live
            || self.last_root != root
            || self.last_reduced != self.reduced_motion
        {
            self.invalidate();
            self.last_studio = Some(project.studio().clone());
            self.last_live = live;
            self.last_reduced = self.reduced_motion;
            if self.last_root != root {
                self.fires.clear();
                self.preview = None;
            }
            self.last_root = root;
        }
        let completed = self.worker.as_ref().and_then(|worker| {
            worker
                .latest
                .lock()
                .ok()
                .and_then(|mut latest| latest.take())
        });
        // The mailbox lock is released before conversion and texture upload.
        if let Some((epoch, result)) = completed
            && epoch == self.epoch
        {
            match result {
                Ok(frame) => {
                    let image = egui::ColorImage::from_rgba_unmultiplied(
                        [frame.width as usize, frame.height as usize],
                        &frame.pixels,
                    );
                    match &mut self.texture {
                        Some(t) => t.set(image, egui::TextureOptions::LINEAR),
                        None => {
                            self.texture = Some(ctx.load_texture(
                                "shared-overlay-renderer",
                                image,
                                egui::TextureOptions::LINEAR,
                            ))
                        }
                    }
                    self.error = None;
                }
                Err(e) => {
                    self.texture = None;
                    self.error = Some(e);
                }
            }
        }
        let at = capture.media_ms().unwrap_or(playhead);
        if live && !capture.taking() {
            self.fires.clear();
        }
        self.fires
            .retain(|f| at >= f.start_ms && at - f.start_ms < f.duration_ms);
        let view = if let Some((content, start)) = &self.preview {
            let elapsed = start.elapsed().as_millis().min(u128::from(u64::MAX)) as u64;
            let duration = cue_duration(content);
            if elapsed >= duration {
                self.preview = None;
                self.invalidate();
                View::Timeline(playhead)
            } else {
                View::Live(vec![(content.clone(), elapsed, duration)])
            }
        } else if live {
            View::Live(
                self.fires
                    .iter()
                    .map(|f| (f.content.clone(), at - f.start_ms, f.duration_ms))
                    .collect(),
            )
        } else {
            View::Timeline(playhead)
        };
        let active = match &view {
            View::Live(items) => !items.is_empty(),
            View::Timeline(ms) => project.studio().cues().iter().any(|c| {
                project
                    .studio()
                    .cue_sequence_time(c.id())
                    .is_ok_and(|start| *ms >= start && *ms - start < c.duration_ms())
            }),
        };
        if !active {
            self.texture = None;
            return;
        }
        if self.worker.is_none() {
            self.worker = Some(spawn_renderer(ctx.clone()));
        }
        let clock = if self.preview.is_some() {
            ctx.cumulative_pass_nr()
        } else {
            at / 33
        };
        if self.last_frame == Some((self.epoch, clock)) {
            return;
        }
        let job = Job {
            epoch: self.epoch,
            store: project.store(),
            root: project.folder.clone(),
            studio: project.studio().clone(),
            view,
            reduced: self.reduced_motion,
            camera: capture.preview_frame(),
            cancel: self.render_cancel.clone(),
        };
        if let Some(worker) = &self.worker {
            match worker.sender.try_send(job) {
                Ok(()) => self.last_frame = Some((self.epoch, clock)),
                Err(mpsc::TrySendError::Full(_)) => {
                    // A rendezvous receiver may still be finishing its prior
                    // frame. Retry even for a stationary timeline preview.
                    ctx.request_repaint_after(Duration::from_millis(16));
                }
                Err(mpsc::TrySendError::Disconnected(_)) => {
                    self.worker = None;
                    self.error = Some("Card renderer stopped. Reopen the card to retry.".into());
                }
            }
        }
        if live || self.preview.is_some() {
            ctx.request_repaint_after(Duration::from_millis(33));
        }
    }
    fn choose_image(&mut self, project: &Workspace, ctx: &egui::Context) {
        if self.busy() {
            return;
        }
        let Some(store) = project.store() else {
            self.error = Some("Save the project before adding an image.".into());
            return;
        };
        let Some(draft) = &self.draft else {
            return;
        };
        let slot = draft.slot;
        let (sender, receiver) = mpsc::channel();
        let cancel = Cancellation::default();
        let token = cancel.clone();
        let ctx = ctx.clone();
        match std::thread::Builder::new()
            .name("storyfold-image-import".into())
            .spawn(move || {
                let result = (|| {
                    let Some(path) = rfd::FileDialog::new()
                        .set_title("Choose screenshot or photo")
                        .add_filter("Image", &["png", "jpg", "jpeg", "webp"])
                        .pick_file()
                    else {
                        return Ok(None);
                    };
                    let store = store
                        .lock()
                        .map_err(|_| "Project worker failed".to_owned())?;
                    media::import_image(&store, &path, &token)
                        .map(Some)
                        .map_err(|e| e.to_string())
                })();
                let _ = sender.send(result);
                ctx.request_repaint();
            }) {
            Ok(_) => {
                self.image = Some(ImagePending {
                    receiver,
                    slot,
                    cancel,
                })
            }
            Err(e) => self.error = Some(e.to_string()),
        }
    }
    pub fn dialog(
        &mut self,
        project: &mut Workspace,
        camera: video_core::OutputSize,
        ctx: &egui::Context,
    ) {
        let image = self
            .image
            .as_ref()
            .and_then(|p| match p.receiver.try_recv() {
                Ok(r) => Some((p.slot, r)),
                Err(mpsc::TryRecvError::Empty) => None,
                Err(mpsc::TryRecvError::Disconnected) => {
                    Some((p.slot, Err("Image worker stopped".into())))
                }
            });
        if let Some((slot, result)) = image {
            let pending = self.image.take().expect("pending image outcome");
            if pending.cancel.is_cancelled() {
                // A published image can arrive before its worker sees Cancel.
                // Retain the file, but do not attach it to the current card.
                self.error = Some(
                    "Image import canceled. The card is unchanged; any imported image stays saved."
                        .into(),
                );
            } else {
                match result {
                    Ok(Some(path)) => {
                        if let Some(d) = self.draft.as_mut().filter(|d| d.slot == slot) {
                            d.content.image = Some(path);
                        }
                    }
                    Ok(None) => {}
                    Err(e) => self.error = Some(e),
                }
            }
        }
        if self.image.is_some() {
            ctx.request_repaint_after(Duration::from_millis(100));
        }
        let Some(mut draft) = self.draft.take() else {
            return;
        };
        let mut approve = false;
        let mut preview = false;
        let mut close = false;
        let mut image = false;
        let response=egui::Modal::new(egui::Id::new("manual-overlay")).show(ctx,|ui|{
            ui.set_width(560.);ui.heading(format!("Edit card {}",draft.slot.number()));
            if self.preview.is_some() && let Some(texture)=&self.texture{
                ui.add(egui::Image::new(texture).max_size(egui::vec2(240.,180.)));
            }
            egui::ScrollArea::vertical().max_height(460.).show(ui,|ui|{
                ui.add_enabled_ui(!draft.insert_editor.busy() && self.image.is_none(),|ui|{
                    let before=draft.insert_mode;
                    ui.horizontal(|ui|{ui.selectable_value(&mut draft.insert_mode,false,"Text / image card");ui.selectable_value(&mut draft.insert_mode,true,"Photo / video insert");});
                    if before!=draft.insert_mode{
                        draft.content=OverlayContent::new(if draft.insert_mode{OverlayKind::Photo}else{OverlayKind::LowerThird},if draft.insert_mode{"Media insert"}else{"New card"});
                        draft.selection=None;draft.insert_editor=InsertEditor::default();self.preview=None;self.invalidate();
                    }
                });
                ui.label("Title");ui.add(egui::TextEdit::singleline(&mut draft.content.title).char_limit(200));
                if draft.insert_mode{
                    draft.insert_editor.show(ui,project,&mut draft.content,camera);
                }else{
                    egui::ComboBox::from_label("Card type").selected_text(draft.content.kind.label()).show_ui(ui,|ui|{for kind in [OverlayKind::LowerThird,OverlayKind::Stat,OverlayKind::ScreenshotCard,OverlayKind::NumberedList,OverlayKind::Photo,OverlayKind::Quote]{ui.selectable_value(&mut draft.content.kind,kind,kind.label());}});
                    let mut remove=None;
                    for(index,block)in draft.content.blocks.iter_mut().enumerate(){
                        ui.horizontal(|ui|{egui::ComboBox::from_id_salt(("role",index)).selected_text(&block.role).show_ui(ui,|ui|{for role in ["heading","body","source","label"]{ui.selectable_value(&mut block.role,role.into(),role);}});if ui.button("Remove block").clicked(){remove=Some(index);}});
                        let output=egui::TextEdit::multiline(&mut block.text).desired_rows(3).char_limit(2000).show(ui);
                        if let Some(range)=output.cursor_range.filter(|r|!r.is_empty()){let r=range.as_sorted_char_range();draft.selection=Some((index,r.start,r.end));}
                    }
                    if let Some(index)=remove{draft.content.blocks.remove(index);draft.content.highlights.clear();draft.selection=None;}
                    if draft.content.blocks.len()<12 && ui.button("Add text block").clicked(){draft.content.blocks.push(OverlayBlock{role:"body".into(),text:String::new()});}
                    ui.horizontal(|ui|{if ui.add_enabled(draft.selection.is_some(),egui::Button::new("Highlight selected text")).clicked()&& let Some((block,start,end))=draft.selection.take(){draft.content.highlights.push(Highlight{block,start,end});}
if ui.button("Clear highlights").clicked(){draft.content.highlights.clear();}});
                    egui::ComboBox::from_label("Highlight style").selected_text(format!("{:?}",draft.content.highlight_style)).show_ui(ui,|ui|{for style in [HighlightStyle::MarkerSweep,HighlightStyle::UnderlineDraw,HighlightStyle::ZoomToLine,HighlightStyle::FadeRest]{ui.selectable_value(&mut draft.content.highlight_style,style,format!("{style:?}"));}});
                    ui.horizontal(|ui|{ui.selectable_value(&mut draft.content.motion,OverlayMotion::FadeUp,"Fade up");ui.selectable_value(&mut draft.content.motion,OverlayMotion::Still,"Still");});
                    if ui.add_enabled(self.image.is_none()&&project.store().is_some(),egui::Button::new("Choose screenshot / photo…")).clicked(){image=true;}
                    if draft.content.image.is_some(){ui.label("Project image attached.");if ui.button("Remove image").clicked(){draft.content.image=None;}}
                }
            });
            if self.image.is_some(){ui.horizontal(|ui|{ui.spinner();ui.label("Importing image…");if ui.button("Cancel image import").clicked()&&let Some(p)=&self.image{p.cancel.cancel();}});}
            if let Some(error)=&self.error{ui.colored_label(egui::Color32::from_rgb(160,30,45),error);}
            let busy=self.image.is_some()||draft.insert_editor.busy();
            ui.horizontal(|ui|{if ui.add_enabled(!busy,egui::Button::new("Approve card")).clicked(){approve=true;}
if ui.add_enabled(!busy,egui::Button::new("Preview card")).clicked(){preview=true;}
if ui.add_enabled(!busy,egui::Button::new("Cancel")).clicked(){close=true;}});
            ui.label("Approved content is copied into each cue. Later card edits keep existing cues unchanged.");
        });
        if preview || approve {
            let validation = if draft.insert_mode && draft.content.insert.is_none() {
                Err("Choose a photo or video before approving the insert".into())
            } else {
                draft.content.validate()
            };
            match validation {
                Ok(()) => {
                    if preview {
                        self.preview = Some((draft.content.clone(), Instant::now()));
                        self.invalidate();
                    }
                    if approve {
                        if project
                            .edit(Edit::SetSlotContent {
                                slot: draft.slot,
                                content: draft.content.clone(),
                            })
                            .is_some()
                        {
                            close = true;
                        } else {
                            self.error = project.error.clone();
                        }
                    }
                }
                Err(e) => self.error = Some(e.to_string()),
            }
        }
        if image {
            self.draft = Some(draft);
            self.choose_image(project, ctx);
        } else if !close
            && (!response.should_close() || self.image.is_some() || draft.insert_editor.busy())
        {
            self.draft = Some(draft);
        }
    }
}
fn cue_duration(content: &OverlayContent) -> u64 {
    content
        .insert
        .as_ref()
        .map_or(5000, |insert| insert.zoom.duration_ms)
}
impl Drop for OverlaysUi {
    fn drop(&mut self) {
        self.render_cancel.cancel();
        if let Some(p) = &self.image {
            p.cancel.cancel();
        }
    }
}
fn render_mailboxes() -> (Worker, mpsc::Receiver<Job>) {
    // No queued camera snapshot: accept only when rendering can begin now.
    let (sender, jobs) = mpsc::sync_channel::<Job>(0);
    let latest = Arc::new(Mutex::new(None));
    (Worker { sender, latest }, jobs)
}
fn publish_render(latest: &Mutex<Option<RenderResult>>, result: RenderResult) -> bool {
    let previous = match latest.lock() {
        Ok(mut latest) => latest.replace(result),
        Err(_) => return false,
    };
    // Free a replaced image outside the critical section.
    drop(previous);
    true
}
fn spawn_renderer(ctx: egui::Context) -> Worker {
    let (worker, jobs) = render_mailboxes();
    let latest = worker.latest.clone();
    std::thread::spawn(move || {
        let mut renderer = OverlayRenderer::new();
        let mut assets = OverlayAssets::new();
        let mut prepared_root = None;
        while let Ok(job) = jobs.recv() {
            let result = (|| {
                if job.cancel.is_cancelled() {
                    return Err("Card preview changed".to_owned());
                }
                let renderer = renderer.as_mut().map_err(|e| e.to_string())?;
                if prepared_root != job.root {
                    assets.clear();
                    prepared_root = job.root.clone();
                }
                let active = match &job.view {
                    View::Live(items) => items
                        .iter()
                        .map(|(content, elapsed_ms, duration_ms)| ActiveOverlay {
                            content,
                            elapsed_ms: *elapsed_ms,
                            duration_ms: *duration_ms,
                        })
                        .collect(),
                    View::Timeline(at) => video_core::renderer::active_overlays(&job.studio, *at)
                        .map_err(|e| e.to_string())?,
                };
                if active
                    .iter()
                    .any(|o| o.content.media_paths().next().is_some())
                {
                    let store = job
                        .store
                        .as_ref()
                        .ok_or("Save the project before preparing media")?
                        .lock()
                        .map_err(|_| "Project worker failed")?;
                    assets
                        .prepare_live(&store, &active, &job.cancel)
                        .map_err(|e| e.to_string())?;
                }
                let options = RenderOptions {
                    reduced_motion: job.reduced,
                };
                if active.iter().any(|o| o.content.insert.is_some()) {
                    let camera = job.camera.map_or_else(
                        || RgbaFrame {
                            width: 16,
                            height: 9,
                            pixels: [247, 246, 242, 255].repeat(16 * 9),
                        },
                        |f| RgbaFrame {
                            width: f.width,
                            height: f.height,
                            pixels: f.rgba.clone(),
                        },
                    );
                    renderer.render_scene_live(
                        job.studio.orientation().output_size(),
                        &camera,
                        &active,
                        &assets,
                        options,
                    )
                } else {
                    renderer.render_live(
                        job.studio.orientation().output_size(),
                        &active,
                        &assets,
                        options,
                    )
                }
                .map_err(|e| e.to_string())
            })();
            if !publish_render(&latest, (job.epoch, result)) {
                break;
            }
            ctx.request_repaint();
        }
    });
    worker
}

#[cfg(test)]
mod tests {
    use super::*;
    fn project() -> Workspace {
        let mut project = Workspace::default();
        let source = project.studio().next_media_id();
        project.edit(Edit::AddMedia {
            path: "media/generated.mp4".into(),
            duration_ms: 20_000,
        });
        for (start, end) in [(0, 6000), (6000, 14000)] {
            project.edit(Edit::AddClip {
                source,
                name: "Generated take".into(),
                in_ms: start,
                out_ms: end,
            });
        }
        project
    }
    #[test]
    fn manual_show_requires_approved_content_and_copies_it_at_clip_relative_time() {
        let mut project = project();
        let mut ui = OverlaysUi::default();
        let mut capture = CaptureUi::default();
        let slot = SlotKey::try_from(2).unwrap();
        ui.fire(slot, &mut project, &mut capture, 10_000);
        assert!(project.studio().cues().is_empty());
        let content = OverlayContent::new(OverlayKind::Quote, "Exact approved words");
        project.edit(Edit::SetSlotContent {
            slot,
            content: content.clone(),
        });
        ui.fire(slot, &mut project, &mut capture, 20_000);
        assert!(project.studio().cues().is_empty());
        ui.fire(slot, &mut project, &mut capture, 10_000);
        let cue = project.studio().cues()[0].clone();
        let clip = project.studio().clips()[1].id();
        assert_eq!(cue.clip_id(), clip);
        assert_eq!(cue.offset_ms(), 4000);
        project.edit(Edit::MoveClip { clip, to_index: 0 });
        assert_eq!(project.studio().cue_sequence_time(cue.id()).unwrap(), 4000);
        project.edit(Edit::SetSlotContent {
            slot,
            content: OverlayContent::new(OverlayKind::Quote, "Later card edit"),
        });
        assert_eq!(project.studio().cues()[0].content(), Some(&content));
    }
    #[test]
    fn canceled_completed_image_import_preserves_the_existing_card_image() {
        let ctx = egui::Context::default();
        let mut project = Workspace::default();
        let mut ui = OverlaysUi::default();
        let slot = SlotKey::try_from(1).unwrap();
        let mut original = OverlayContent::new(OverlayKind::ScreenshotCard, "Original card");
        original.image = Some("media/original.png".into());
        ui.edit(slot, Some(original.clone()));
        let (sender, receiver) = mpsc::channel();
        ui.image = Some(ImagePending {
            receiver,
            slot,
            cancel: Cancellation::default(),
        });
        // Force the real ACK boundary without opening a picker, importing a
        // file, decoding an image or changing a user project.
        sender
            .send(Ok(Some("media/newly-published.png".into())))
            .unwrap();
        ui.image.as_ref().unwrap().cancel.cancel();
        let _ = ctx.run(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(1160., 800.),
                )),
                ..Default::default()
            },
            |ctx| {
                ui.dialog(
                    &mut project,
                    video_core::OutputSize {
                        width: 1280,
                        height: 720,
                    },
                    ctx,
                );
            },
        );
        assert_eq!(
            ui.draft.as_ref().unwrap().content.image,
            original.image,
            "Canceled image import changed the current card image"
        );
        assert_eq!(ui.draft.as_ref().unwrap().content.title, original.title);
        assert!(!ui.busy());
        assert!(project.studio().cues().is_empty());
    }
    #[test]
    #[allow(clippy::field_reassign_with_default)] // Drop cancels render work, so struct-update moves are unavailable.
    fn late_card_frame_from_a_previous_project_epoch_is_never_uploaded() {
        let mut project = project();
        let slot = SlotKey::try_from(1).unwrap();
        let clip = project.studio().clips()[0].id();
        project.edit(Edit::AddCueFromSnapshot {
            clip,
            offset_ms: 0,
            slot,
            content: OverlayContent::new(OverlayKind::Quote, "Generated card"),
            duration_ms: 5000,
        });
        let (ctx, capture) = (egui::Context::default(), CaptureUi::default());
        let (worker, _jobs) = render_mailboxes();
        let latest = worker.latest.clone();
        let mut ui = OverlaysUi::default();
        ui.worker = Some(worker);
        ui.epoch = 2;
        ui.last_studio = Some(project.studio().clone());
        ui.last_frame = Some((2, 1000 / 33));
        let frame = || RgbaFrame {
            width: 1,
            height: 1,
            pixels: vec![120, 96, 181, 255],
        };
        assert!(publish_render(&latest, (1, Ok(frame()))));
        ui.tick(&project, &capture, 1000, &ctx);
        assert!(ui.texture.is_none());
        assert!(publish_render(&latest, (2, Ok(frame()))));
        ui.tick(&project, &capture, 1000, &ctx);
        assert!(ui.texture.is_some());
    }
    #[test]
    fn render_mailbox_keeps_newest_result_and_never_queues_camera_jobs() {
        let (worker, jobs) = render_mailboxes();
        let frame = |value| RgbaFrame {
            width: 1,
            height: 1,
            pixels: vec![value, 96, 181, 255],
        };
        assert!(publish_render(&worker.latest, (1, Ok(frame(10)))));
        assert!(publish_render(&worker.latest, (2, Ok(frame(20)))));
        let (epoch, result) = worker.latest.lock().unwrap().take().unwrap();
        assert_eq!(epoch, 2);
        assert_eq!(result.unwrap().pixels[0], 20);
        assert!(worker.latest.lock().unwrap().is_none());

        let job = Job {
            epoch: 3,
            store: None,
            root: None,
            studio: Studio::default(),
            view: View::Timeline(0),
            reduced: false,
            camera: None,
            cancel: Cancellation::default(),
        };
        // Merely keeping the receiver alive cannot retain an old snapshot.
        let job = match worker.sender.try_send(job) {
            Err(mpsc::TrySendError::Full(job)) => job,
            _ => panic!("A busy renderer must not queue a camera job"),
        };
        drop(jobs);
        assert!(matches!(
            worker.sender.try_send(job),
            Err(mpsc::TrySendError::Disconnected(_))
        ));
    }
}
