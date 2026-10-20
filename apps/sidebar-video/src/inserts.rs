//! Media-insert drafts. All import/decode work runs off the UI thread.
use crate::{Workspace, workspace::Store};
use eframe::egui;
use std::{
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
        mpsc,
    },
    time::Duration,
};
use video_core::{
    InsertContent, InsertPlacement, InsertSource, OutputSize, OverlayContent, SceneLayout,
    Viewport, ZoomEasing, ZoomPoint, ZoomSpec,
    media::{self, Cancellation},
    renderer::{ActiveOverlay, OverlayAssets, RgbaFrame},
};

struct Prepared {
    content: OverlayContent,
    frame: RgbaFrame,
    duration_ms: Option<u64>,
}
struct Pending {
    receiver: mpsc::Receiver<Result<Option<Prepared>, String>>,
    cancel: Cancellation,
    progress: Arc<AtomicU64>,
    replace: bool,
}
#[derive(Default)]
pub struct InsertEditor {
    pending: Option<Pending>,
    texture: Option<egui::TextureHandle>,
    source: Option<OutputSize>,
    loaded_path: Option<String>,
    end_box: bool,
    duration_ms: Option<u64>,
    preview_at_ms: u64,
    pub error: Option<String>,
}
impl InsertEditor {
    pub fn busy(&self) -> bool {
        self.pending.is_some()
    }
    pub fn cancel(&self) {
        if let Some(p) = &self.pending {
            p.cancel.cancel();
        }
    }
    pub fn poll(&mut self, content: &mut OverlayContent, ctx: &egui::Context) {
        let result = self
            .pending
            .as_ref()
            .and_then(|p| match p.receiver.try_recv() {
                Ok(r) => Some(r),
                Err(mpsc::TryRecvError::Empty) => None,
                Err(mpsc::TryRecvError::Disconnected) => Some(Err("Insert worker stopped".into())),
            });
        if let Some(result) = result {
            let replace = self.pending.as_ref().is_some_and(|p| p.replace);
            self.pending = None;
            match result {
                Ok(Some(prepared)) => {
                    self.duration_ms = prepared.duration_ms;
                    self.preview_at_ms = match prepared.content.insert.as_ref().map(|i| &i.source) {
                        Some(InsertSource::Video { source_in_ms, .. }) => *source_in_ms,
                        _ => 0,
                    };
                    self.source = Some(OutputSize {
                        width: prepared.frame.width,
                        height: prepared.frame.height,
                    });
                    self.loaded_path = prepared
                        .content
                        .insert
                        .as_ref()
                        .map(|i| i.path().to_owned());
                    self.texture = Some(ctx.load_texture(
                        "insert-source",
                        egui::ColorImage::from_rgba_unmultiplied(
                            [
                                prepared.frame.width as usize,
                                prepared.frame.height as usize,
                            ],
                            &prepared.frame.pixels,
                        ),
                        egui::TextureOptions::LINEAR,
                    ));
                    if replace {
                        let title = content.title.clone();
                        *content = prepared.content;
                        content.title = title;
                    }
                    self.error = None;
                }
                Ok(None) => {}
                Err(e) => self.error = Some(e),
            }
        }
        if self.busy() {
            ctx.request_repaint_after(Duration::from_millis(100));
        }
    }
    pub fn choose(
        &mut self,
        project: &Workspace,
        content: &OverlayContent,
        video: bool,
        ctx: &egui::Context,
    ) {
        let Some(store) = project.store() else {
            self.error = Some("Save the project before adding media.".into());
            return;
        };
        let title = content.title.clone();
        self.start(ctx, true, move |cancel, progress| {
            let picker = rfd::FileDialog::new().set_title(if video {
                "Choose a video insert"
            } else {
                "Choose a photo insert"
            });
            let picker = if video {
                picker.add_filter("Video", &["mp4", "mkv", "mov", "webm", "avi", "m4v"])
            } else {
                picker.add_filter("Image", &["png", "jpg", "jpeg", "webp"])
            };
            let Some(path) = picker.pick_file() else {
                return Ok(None);
            };
            if cancel.is_cancelled() {
                return Ok(None);
            }
            let store = store.lock().map_err(|_| "Project worker failed")?;
            let (source, duration) = if video {
                let prepared = video_core::insert::import_video(&store, &path, cancel, |ms| {
                    progress.store(ms, Ordering::Relaxed);
                })?;
                (
                    InsertSource::Video {
                        path: prepared.path,
                        source_in_ms: 0,
                    },
                    prepared.duration_ms.min(5000),
                )
            } else {
                (
                    InsertSource::Photo {
                        path: media::import_image(&store, &path, cancel)?,
                    },
                    5000,
                )
            };
            let content = OverlayContent::from_insert(
                title,
                InsertContent {
                    source,
                    placement: InsertPlacement::Fullscreen,
                    zoom: ZoomSpec {
                        duration_ms: duration.max(1),
                        ..Default::default()
                    },
                },
            );
            prepared(&store, content, cancel).map(Some)
        });
    }
    pub fn load(&mut self, store: Store, content: OverlayContent, ctx: &egui::Context) {
        self.start(ctx, false, move |cancel, _| {
            let store = store.lock().map_err(|_| "Project worker failed")?;
            prepared(&store, content, cancel).map(Some)
        });
    }
    fn start(
        &mut self,
        ctx: &egui::Context,
        replace: bool,
        task: impl FnOnce(&Cancellation, &AtomicU64) -> video_core::Result<Option<Prepared>>
        + Send
        + 'static,
    ) {
        if self.busy() {
            return;
        }
        let (sender, receiver) = mpsc::channel();
        let cancel = Cancellation::default();
        let token = cancel.clone();
        let progress = Arc::new(AtomicU64::new(0));
        let counter = progress.clone();
        let ctx = ctx.clone();
        match std::thread::Builder::new()
            .name("storyfold-insert".into())
            .spawn(move || {
                let result = task(&token, &counter).map_err(|e| e.to_string());
                let _ = sender.send(result);
                ctx.request_repaint();
            }) {
            Ok(_) => {
                self.pending = Some(Pending {
                    receiver,
                    cancel,
                    progress,
                    replace,
                });
                self.error = None;
            }
            Err(e) => self.error = Some(e.to_string()),
        }
    }
    pub fn show(
        &mut self,
        ui: &mut egui::Ui,
        project: &Workspace,
        content: &mut OverlayContent,
        camera: OutputSize,
    ) {
        self.poll(content, ui.ctx());
        ui.horizontal(|ui| {
            if ui
                .add_enabled(!self.busy(), egui::Button::new("Choose photo…"))
                .clicked()
            {
                self.choose(project, content, false, ui.ctx());
            }
            if ui
                .add_enabled(!self.busy(), egui::Button::new("Choose video…"))
                .clicked()
            {
                self.choose(project, content, true, ui.ctx());
            }
        });
        if let Some(p) = &self.pending {
            ui.horizontal(|ui| {
                ui.spinner();
                let ms = p.progress.load(Ordering::Relaxed);
                ui.label(if ms == 0 {
                    "Preparing insert…".into()
                } else {
                    format!("Prepared {:.1} seconds", ms as f64 / 1000.)
                });
                if ui.button("Cancel import").clicked() {
                    p.cancel.cancel();
                }
            });
        }
        if self.busy() {
            return;
        }
        let Some(insert) = content.insert.as_mut() else {
            ui.label("Choose media to create an insert.");
            return;
        };
        if self.loaded_path.as_deref() != Some(insert.path())
            && !self.busy()
            && self.error.is_none()
            && let Some(store) = project.store()
        {
            self.load(store, content.clone(), ui.ctx());
            return;
        }
        let insert = content.insert.as_mut().expect("checked insert");
        ui.label("Video inserts are muted. Your recorded camera audio continues.");
        ui.horizontal_wrapped(|ui| {
            for (value, label) in [
                (InsertPlacement::Fullscreen, "Full screen"),
                (InsertPlacement::Beside, "Beside camera"),
                (InsertPlacement::Behind, "Behind camera card"),
            ] {
                ui.selectable_value(&mut insert.placement, value, label);
            }
        });
        if insert.placement == InsertPlacement::Behind {
            ui.label("The camera appears as an opaque card in front of the media.");
        }
        if let InsertSource::Video { source_in_ms, .. } = &mut insert.source {
            ui.horizontal(|ui| {
                ui.label("Video start (seconds)");
                let mut seconds = *source_in_ms as f64 / 1000.;
                if ui
                    .add(
                        egui::DragValue::new(&mut seconds)
                            .range(
                                0.0..=self
                                    .duration_ms
                                    .map_or(86400., |ms| ms.saturating_sub(34) as f64 / 1000.),
                            )
                            .speed(0.1),
                    )
                    .changed()
                {
                    *source_in_ms = (seconds * 1000.).round() as u64;
                }
            });
        }
        let available = match insert.source {
            InsertSource::Video { source_in_ms, .. } => self
                .duration_ms
                .map(|ms| ms.saturating_sub(source_in_ms).max(1)),
            _ => None,
        };
        if let Some(ms) = available {
            insert.zoom.duration_ms = insert.zoom.duration_ms.min(ms);
        }
        ui.horizontal(|ui| {
            ui.label("Zoom duration (seconds)");
            let mut seconds = insert.zoom.duration_ms as f64 / 1000.;
            if ui
                .add(
                    egui::DragValue::new(&mut seconds)
                        .range(0.001..=available.map_or(600., |ms| (ms as f64 / 1000.).min(600.)))
                        .speed(0.1),
                )
                .changed()
            {
                insert.zoom.duration_ms = (seconds * 1000.).round() as u64;
            }
        });
        egui::ComboBox::from_label("Easing")
            .selected_text(format!("{:?}", insert.zoom.easing))
            .show_ui(ui, |ui| {
                for (value, label) in [
                    (ZoomEasing::Smooth, "Smooth"),
                    (ZoomEasing::Steady, "Steady"),
                    (ZoomEasing::Snappy, "Snappy"),
                ] {
                    ui.selectable_value(&mut insert.zoom.easing, value, label);
                }
            });
        ui.horizontal(|ui| {
            ui.selectable_value(&mut self.end_box, false, "Start box");
            ui.selectable_value(&mut self.end_box, true, "End box");
        });
        let point = if self.end_box {
            &mut insert.zoom.end
        } else {
            &mut insert.zoom.start
        };
        ui.add(egui::Slider::new(&mut point.percent, 100..=250).text("Zoom %"));
        ui.horizontal(|ui| {
            ui.add(egui::Slider::new(&mut point.center_x, 0..=1000).text("Center X"));
            ui.add(egui::Slider::new(&mut point.center_y, 0..=1000).text("Center Y"));
        });
        if let (Some(texture), Some(source)) = (&self.texture, self.source) {
            match SceneLayout::new(
                project.studio().orientation().output_size(),
                camera,
                insert.placement,
            ) {
                Ok(layout) => {
                    let target = OutputSize {
                        width: layout.media.width.round() as u32,
                        height: layout.media.height.round() as u32,
                    };
                    if let Err(e) =
                        draw_boxes(ui, texture, source, target, &mut insert.zoom, self.end_box)
                    {
                        self.error = Some(e.to_string());
                    }
                }
                Err(e) => self.error = Some(e.to_string()),
            }
        }
        ui.label("Drag a box to move it; drag its lower-right handle to resize. Numeric controls work with the keyboard.");
        if self.duration_ms.is_some() {
            ui.label(format!("Source reference frame at {:.3} seconds. Preview card shows the chosen video timing.",self.preview_at_ms as f64/1000.));
        }

        if let Some(error) = &self.error {
            ui.colored_label(egui::Color32::from_rgb(160, 30, 45), error);
            if ui.button("Retry media preview").clicked() {
                self.error = None;
                self.loaded_path = None;
            }
        }
    }
}
fn prepared(
    store: &video_core::ProjectStore,
    content: OverlayContent,
    cancel: &Cancellation,
) -> video_core::Result<Prepared> {
    let duration_ms = match content.insert.as_ref().map(|i| &i.source) {
        Some(InsertSource::Video { path, .. }) => {
            Some(video_core::insert::inspect_video(store, path, cancel)?.duration_ms)
        }
        _ => None,
    };
    let mut assets = OverlayAssets::new();
    let active = [ActiveOverlay {
        content: &content,
        elapsed_ms: 0,
        duration_ms: content.insert.as_ref().map_or(5000, |i| i.zoom.duration_ms),
    }];
    assets.prepare_live(store, &active, cancel)?;
    let frame = assets.insert_preview(&content, 0)?;
    Ok(Prepared {
        content,
        frame,
        duration_ms,
    })
}
impl Drop for InsertEditor {
    fn drop(&mut self) {
        self.cancel();
    }
}
fn draw_boxes(
    ui: &mut egui::Ui,
    texture: &egui::TextureHandle,
    source: OutputSize,
    target: OutputSize,
    zoom: &mut ZoomSpec,
    end: bool,
) -> video_core::Result<()> {
    let width = ui.available_width().min(480.);
    let height = (width * source.height as f32 / source.width as f32).min(240.);
    let width = height * source.width as f32 / source.height as f32;
    let (rect, _) = ui.allocate_exact_size(egui::vec2(width, height), egui::Sense::hover());
    ui.painter().image(
        texture.id(),
        rect,
        egui::Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(1., 1.)),
        egui::Color32::WHITE,
    );
    let scale = rect.width() / source.width as f32;
    for (which, point) in [(false, zoom.start), (true, zoom.end)] {
        let view = ZoomSpec {
            start: point,
            end: point,
            duration_ms: 1,
            easing: ZoomEasing::Steady,
        }
        .viewport(source, target, 0, true)?;
        let bounds = egui::Rect::from_min_size(
            rect.min + egui::vec2(view.x as f32 * scale, view.y as f32 * scale),
            egui::vec2(view.width as f32 * scale, view.height as f32 * scale),
        );
        let selected = which == end;
        let color = if which {
            egui::Color32::from_rgb(65, 112, 134)
        } else {
            crate::ACCENT
        };
        ui.painter().rect_stroke(
            bounds,
            0,
            egui::Stroke::new(if selected { 3_f32 } else { 1_f32 }, color),
            egui::StrokeKind::Inside,
        );
        ui.painter().text(
            bounds.left_top() + egui::vec2(5., 5.),
            egui::Align2::LEFT_TOP,
            if which { "End" } else { "Start" },
            egui::FontId::proportional(12.),
            color,
        );
        if !selected {
            continue;
        }
        let handle = egui::Rect::from_center_size(
            bounds.right_bottom() - egui::vec2(6., 6.),
            egui::vec2(16., 16.),
        );
        ui.painter().rect_filled(handle, 3, color);
        let moved = ui.interact(bounds, ui.id().with("move-box"), egui::Sense::drag());
        let resized = ui.interact(handle, ui.id().with("resize-box"), egui::Sense::drag());
        moved.clone().widget_info(|| {
            egui::WidgetInfo::labeled(egui::WidgetType::Other, true, "Move selected zoom box")
        });
        resized.clone().widget_info(|| {
            egui::WidgetInfo::labeled(egui::WidgetType::Other, true, "Resize selected zoom box")
        });
        let pointer = if resized.dragged() {
            resized.interact_pointer_pos()
        } else if moved.dragged() {
            moved.interact_pointer_pos()
        } else {
            None
        };
        if let Some(pointer) = pointer {
            let x = f64::from((pointer.x - rect.left()) / scale);
            let y = f64::from((pointer.y - rect.top()) / scale);
            let proposed = if resized.dragged() {
                let base = ZoomSpec::default().viewport(source, target, 0, true)?;
                let w = ((x - (view.x + view.width / 2.)).abs() * 2.)
                    .clamp(base.width / 2.5, base.width);
                let h = w * f64::from(target.height) / f64::from(target.width);
                Viewport {
                    x: (view.x + view.width / 2. - w / 2.).clamp(0., f64::from(source.width) - w),
                    y: (view.y + view.height / 2. - h / 2.).clamp(0., f64::from(source.height) - h),
                    width: w,
                    height: h,
                }
            } else {
                Viewport {
                    x: (x - view.width / 2.).clamp(0., f64::from(source.width) - view.width),
                    y: (y - view.height / 2.).clamp(0., f64::from(source.height) - view.height),
                    ..view
                }
            };
            let point = ZoomPoint::from_viewport(source, target, proposed)?;
            if end {
                zoom.end = point
            } else {
                zoom.start = point
            };
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn content() -> OverlayContent {
        OverlayContent::from_insert(
            "Source",
            InsertContent {
                source: InsertSource::Photo {
                    path: "media/photo.png".into(),
                },
                placement: InsertPlacement::Fullscreen,
                zoom: ZoomSpec::default(),
            },
        )
    }
    #[test]
    fn thumbnail_ack_never_replaces_newer_draft_edits() {
        let ctx = egui::Context::default();
        let mut editor = InsertEditor::default();
        let (sender, receiver) = mpsc::channel();
        editor.pending = Some(Pending {
            receiver,
            cancel: Cancellation::default(),
            progress: Arc::new(AtomicU64::new(0)),
            replace: false,
        });
        let original = content();
        let mut draft = original.clone();
        draft.title = "Newer title".into();
        draft.insert.as_mut().unwrap().zoom.end.percent = 225;
        sender
            .send(Ok(Some(Prepared {
                content: original,
                frame: RgbaFrame {
                    width: 2,
                    height: 2,
                    pixels: [247, 246, 242, 255].repeat(4),
                },
                duration_ms: None,
            })))
            .unwrap();
        editor.poll(&mut draft, &ctx);
        assert_eq!(draft.title, "Newer title");
        assert_eq!(draft.insert.unwrap().zoom.end.percent, 225);
        assert!(!editor.busy());
        assert!(editor.texture.is_some());
    }
    #[test]
    fn pointer_moves_and_resizes_only_selected_zoom_box() {
        let ctx = egui::Context::default();
        ctx.enable_accesskit();
        let texture = ctx.load_texture(
            "generated",
            egui::ColorImage::new([2, 2], vec![egui::Color32::WHITE; 4]),
            egui::TextureOptions::LINEAR,
        );
        let source = OutputSize {
            width: 640,
            height: 480,
        };
        let target = OutputSize {
            width: 1920,
            height: 1080,
        };
        let mut zoom = ZoomSpec {
            start: ZoomPoint {
                percent: 150,
                ..Default::default()
            },
            ..Default::default()
        };
        let end = zoom.end;
        let draw = |zoom: &mut ZoomSpec, events| {
            ctx.run(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(600., 500.),
                    )),
                    events,
                    ..Default::default()
                },
                |ctx| {
                    egui::CentralPanel::default().show(ctx, |ui| {
                        draw_boxes(ui, &texture, source, target, zoom, false).unwrap()
                    });
                },
            )
        };
        let _ = draw(&mut zoom, vec![]);
        let press = |pos, pressed| egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        };
        let center = egui::pos2(168., 128.);
        let _ = draw(
            &mut zoom,
            vec![egui::Event::PointerMoved(center), press(center, true)],
        );
        let moved = center + egui::vec2(30., 0.);
        let output = draw(&mut zoom, vec![egui::Event::PointerMoved(moved)]);
        let _ = draw(&mut zoom, vec![press(moved, false)]);
        assert!(zoom.start.center_x > 550, "{zoom:?}");
        assert_eq!(zoom.end, end);
        let view = zoom.viewport(source, target, 0, true).unwrap();
        let handle = egui::pos2(
            8. + (view.x + view.width) as f32 * 0.5 - 6.,
            8. + (view.y + view.height) as f32 * 0.5 - 6.,
        );
        let _ = draw(
            &mut zoom,
            vec![egui::Event::PointerMoved(handle), press(handle, true)],
        );
        let smaller = handle - egui::vec2(30., 20.);
        let _ = draw(&mut zoom, vec![egui::Event::PointerMoved(smaller)]);
        let _ = draw(&mut zoom, vec![press(smaller, false)]);
        assert!(zoom.start.percent > 150, "{zoom:?}");
        assert_eq!(zoom.end, end);
        zoom.validate().unwrap();
        assert!(
            output
                .platform_output
                .accesskit_update
                .unwrap()
                .nodes
                .iter()
                .any(|(_, n)| n.label() == Some("Move selected zoom box"))
        );
    }
}
