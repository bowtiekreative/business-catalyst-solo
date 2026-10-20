//! Approved manual cards and bounded worker rendering. Preview and export use
//! video-core's identical RGBA path; no font shaping or image decoding in paint.
use crate::{Workspace, capture::CaptureUi, workspace::Store};
use eframe::egui;
use std::{
    sync::mpsc,
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
}
struct Worker {
    sender: mpsc::SyncSender<Job>,
    receiver: mpsc::Receiver<(u64, Result<RgbaFrame, String>)>,
}
struct Draft {
    slot: SlotKey,
    content: OverlayContent,
    selection: Option<(usize, usize, usize)>,
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
    pub texture: Option<egui::TextureHandle>,
    pub error: Option<String>,
    pub reduced_motion: bool,
}
impl OverlaysUi {
    pub fn busy(&self) -> bool {
        self.image.is_some()
    }
    pub fn editing(&self) -> bool {
        self.draft.is_some()
    }
    fn invalidate(&mut self) {
        self.epoch = self.epoch.wrapping_add(1);
        self.last_frame = None;
        self.texture = None;
    }
    pub fn edit(&mut self, slot: SlotKey, content: Option<OverlayContent>) {
        self.draft = Some(Draft {
            slot,
            content: content
                .unwrap_or_else(|| OverlayContent::new(OverlayKind::LowerThird, "New card")),
            selection: None,
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
            if !capture.mark(slot, content.clone()) {
                return;
            }
            self.fires.retain(|f| f.slot != slot);
            self.fires.push(Fire {
                slot,
                content,
                start_ms: at,
                duration_ms: 5000,
            });
            self.invalidate();
        } else if project
            .edit(Edit::AddCueAtPlayhead {
                sequence_ms: playhead,
                slot,
                duration_ms: 5000,
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
        if let Some(worker) = &self.worker {
            for (epoch, result) in worker.receiver.try_iter() {
                if epoch == self.epoch {
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
            if elapsed >= 5000 {
                self.preview = None;
                self.invalidate();
                View::Timeline(playhead)
            } else {
                View::Live(vec![(content.clone(), elapsed, 5000)])
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
        };
        if let Some(worker) = &self.worker {
            match worker.sender.try_send(job) {
                Ok(()) => self.last_frame = Some((self.epoch, clock)),
                Err(mpsc::TrySendError::Full(_)) => {}
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
    pub fn dialog(&mut self, project: &mut Workspace, ctx: &egui::Context) {
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
            self.image = None;
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
            ui.set_width(520.);ui.heading(format!("Edit card {}",draft.slot.number()));
            if self.preview.is_some() && let Some(texture)=&self.texture{ui.add(egui::Image::new(texture).fit_to_exact_size(egui::vec2(240.,135.)));}
            egui::ScrollArea::vertical().max_height(430.).show(ui,|ui|{
                egui::ComboBox::from_label("Card type").selected_text(draft.content.kind.label()).show_ui(ui,|ui|{for kind in [OverlayKind::LowerThird,OverlayKind::Stat,OverlayKind::ScreenshotCard,OverlayKind::NumberedList,OverlayKind::Photo,OverlayKind::Quote]{ui.selectable_value(&mut draft.content.kind,kind,kind.label());}});
                ui.label("Title");ui.add(egui::TextEdit::singleline(&mut draft.content.title).char_limit(200));
                let mut remove=None;
                for(index,block) in draft.content.blocks.iter_mut().enumerate(){
                    ui.horizontal(|ui|{egui::ComboBox::from_id_salt(("role",index)).selected_text(&block.role).show_ui(ui,|ui|{for role in ["heading","body","source","label"]{ui.selectable_value(&mut block.role,role.into(),role);}});if ui.button("Remove block").clicked(){remove=Some(index);}});
                    let output=egui::TextEdit::multiline(&mut block.text).desired_rows(3).char_limit(2000).show(ui);
                    if let Some(range)=output.cursor_range.filter(|r|!r.is_empty()){let r=range.as_sorted_char_range();draft.selection=Some((index,r.start,r.end));}
                }
                if let Some(index)=remove{draft.content.blocks.remove(index);draft.content.highlights.clear();draft.selection=None;}
                if draft.content.blocks.len()<12 && ui.button("Add text block").clicked(){draft.content.blocks.push(OverlayBlock{role:"body".into(),text:String::new()});}
                ui.horizontal(|ui|{if ui.add_enabled(draft.selection.is_some(),egui::Button::new("Highlight selected text")).clicked()&& let Some((block,start,end))=draft.selection.take(){draft.content.highlights.push(Highlight{block,start,end});}
                    if ui.button("Clear highlights").clicked(){draft.content.highlights.clear();}});
                egui::ComboBox::from_label("Highlight style").selected_text(format!("{:?}",draft.content.highlight_style)).show_ui(ui,|ui|{for s in [HighlightStyle::MarkerSweep,HighlightStyle::UnderlineDraw,HighlightStyle::ZoomToLine,HighlightStyle::FadeRest]{ui.selectable_value(&mut draft.content.highlight_style,s,format!("{s:?}"));}});
                ui.horizontal(|ui|{ui.selectable_value(&mut draft.content.motion,OverlayMotion::FadeUp,"Fade up");ui.selectable_value(&mut draft.content.motion,OverlayMotion::Still,"Still");});
                if ui.add_enabled(!self.busy() && project.store().is_some(),egui::Button::new("Choose screenshot / photo…")).clicked(){image=true;}
                if draft.content.image.is_some(){ui.label("Project image attached.");if ui.button("Remove image").clicked(){draft.content.image=None;}}
            });
            if self.busy(){ui.horizontal(|ui|{ui.spinner();ui.label("Importing image…");if ui.button("Cancel image import").clicked()&& let Some(p)=&self.image{p.cancel.cancel();}});}
            if let Some(error)=&self.error{ui.colored_label(egui::Color32::from_rgb(160,30,45),error);}
            ui.horizontal(|ui|{if ui.add_enabled(!self.busy(),egui::Button::new("Approve card")).clicked(){approve=true;}
if ui.button("Preview card").clicked(){preview=true;}
if ui.add_enabled(!self.busy(),egui::Button::new("Cancel")).clicked(){close=true;}});
            ui.label("Approved content is copied into each cue. Later card edits keep existing cues unchanged.");
        });
        if preview || approve {
            match draft.content.validate() {
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
        } else if !close && (!response.should_close() || self.busy()) {
            self.draft = Some(draft);
        }
    }
}
fn spawn_renderer(ctx: egui::Context) -> Worker {
    let (sender, jobs) = mpsc::sync_channel::<Job>(1);
    let (results, receiver) = mpsc::sync_channel(1);
    std::thread::spawn(move || {
        let mut renderer = OverlayRenderer::new();
        let mut assets = OverlayAssets::new();
        let mut prepared = None;
        while let Ok(job) = jobs.recv() {
            let result = (|| {
                let renderer = renderer.as_mut().map_err(|e| e.to_string())?;
                let contents: Vec<_> = match &job.view {
                    View::Live(items) => items.iter().map(|(c, _, _)| c.clone()).collect(),
                    View::Timeline(at) => job
                        .studio
                        .cues()
                        .iter()
                        .filter(|cue| {
                            job.studio
                                .cue_sequence_time(cue.id())
                                .is_ok_and(|start| *at >= start && *at - start < cue.duration_ms())
                        })
                        .filter_map(|c| c.content().cloned())
                        .collect(),
                };
                let mut images: Vec<_> = contents.iter().filter_map(|c| c.image.clone()).collect();
                images.sort();
                images.dedup();
                let key = (job.root.clone(), images);
                if prepared.as_ref() != Some(&key) {
                    prepared = None;
                    assets.clear();
                    if !key.1.is_empty() {
                        let store = job
                            .store
                            .as_ref()
                            .ok_or("Save the project before preparing its images")?
                            .lock()
                            .map_err(|_| "Project worker failed".to_owned())?;
                        for content in &contents {
                            assets
                                .load_content(&store, content)
                                .map_err(|e| e.to_string())?;
                        }
                    }
                    prepared = Some(key);
                }
                let options = RenderOptions {
                    reduced_motion: job.reduced,
                };
                match job.view {
                    View::Timeline(at) => {
                        renderer.render_timeline(&job.studio, at, &assets, options)
                    }
                    View::Live(items) => {
                        let active: Vec<_> = items
                            .iter()
                            .map(|(content, elapsed_ms, duration_ms)| ActiveOverlay {
                                content,
                                elapsed_ms: *elapsed_ms,
                                duration_ms: *duration_ms,
                            })
                            .collect();
                        renderer.render_live(
                            job.studio.orientation().output_size(),
                            &active,
                            &assets,
                            options,
                        )
                    }
                }
                .map_err(|e| e.to_string())
            })();
            if results.send((job.epoch, result)).is_err() {
                break;
            }
            ctx.request_repaint();
        }
    });
    Worker { sender, receiver }
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
        let (sender, _jobs) = mpsc::sync_channel(1);
        let (results, receiver) = mpsc::sync_channel(1);
        let mut ui = OverlaysUi {
            worker: Some(Worker { sender, receiver }),
            epoch: 2,
            last_studio: Some(project.studio().clone()),
            last_frame: Some((2, 1000 / 33)),
            ..Default::default()
        };
        let frame = || RgbaFrame {
            width: 1,
            height: 1,
            pixels: vec![120, 96, 181, 255],
        };
        results.send((1, Ok(frame()))).unwrap();
        ui.tick(&project, &capture, 1000, &ctx);
        assert!(ui.texture.is_none());
        results.send((2, Ok(frame()))).unwrap();
        ui.tick(&project, &capture, 1000, &ctx);
        assert!(ui.texture.is_some());
    }
}
