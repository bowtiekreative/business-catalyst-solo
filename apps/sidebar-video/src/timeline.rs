//! Visual sequence gestures are transient until release. The core owns each edit.
use crate::waveforms::{WaveformState, WaveformsUi};
use eframe::egui::{self, Color32, Pos2, Rect, Sense, Stroke, Vec2};
use video_core::{Clip, ClipId, Edit, MediaId, Studio};

const HANDLE: f32 = 18.;
const TRACK_Y: f32 = 23.;
const TRACK_HEIGHT: f32 = 46.;
const CANVAS_HEIGHT: f32 = 86.;
#[derive(Clone, Copy, PartialEq, Eq)]
enum Edge {
    Start,
    End,
}
#[derive(Clone, Copy)]
enum Kind {
    Trim {
        edge: Edge,
        original_in: u64,
        original_out: u64,
        in_ms: u64,
        out_ms: u64,
        source: MediaId,
        source_duration: u64,
        sequence_start: u64,
    },
    Move {
        to_index: usize,
    },
}
struct Gesture {
    clip: ClipId,
    baseline: Vec<Clip>,
    anchor_x: f64,
    kind: Kind,
}
#[derive(Default)]
pub struct TimelineResponse {
    pub selected: Option<ClipId>,
    pub seek: Option<u64>,
    pub edit: Option<Edit>,
    pub message: Option<String>,
}
pub struct TimelineUi {
    pixels_per_second: f64,
    gesture: Option<Gesture>,
    focus_selected: bool,
}
impl Default for TimelineUi {
    fn default() -> Self {
        Self {
            pixels_per_second: 64.,
            gesture: None,
            focus_selected: false,
        }
    }
}
impl TimelineUi {
    pub fn dragging(&self) -> bool {
        self.gesture.is_some()
    }
    pub fn cancel(&mut self) {
        self.gesture = None;
    }
    pub fn draft_source_preview(&self) -> Option<(MediaId, u64)> {
        match &self.gesture.as_ref()?.kind {
            Kind::Trim {
                edge,
                source,
                in_ms,
                out_ms,
                ..
            } => Some((
                *source,
                if *edge == Edge::Start {
                    *in_ms
                } else {
                    out_ms.saturating_sub(1)
                },
            )),
            _ => None,
        }
    }
    pub fn show(
        &mut self,
        ui: &mut egui::Ui,
        studio: &Studio,
        selected: Option<ClipId>,
        playhead: u64,
        enabled: bool,
        waveforms: &WaveformsUi,
    ) -> TimelineResponse {
        let mut result = TimelineResponse::default();
        if self
            .gesture
            .as_ref()
            .is_some_and(|g| g.baseline != studio.clips())
        {
            self.cancel();
            result.message = Some("The sequence changed; the unfinished drag was canceled.".into());
        }
        if !enabled
            || !ui.input(|i| i.focused)
            || ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape))
        {
            self.cancel();
        }
        let total = studio.duration_ms();
        let viewport_width = ui.available_width().max(100.);
        ui.scope(|ui| {
            ui.spacing_mut().interact_size.y = 24.;
            ui.horizontal(|ui| {
                ui.label(
                    egui::RichText::new("Drag edges to trim · Drag clips to reorder")
                        .size(12.)
                        .color(crate::BODY),
                );
                ui.add_enabled_ui(!self.dragging(), |ui| {
                    if ui.button("−").on_hover_text("Zoom timeline out").clicked() {
                        self.pixels_per_second = (self.pixels_per_second / 2.).max(0.02);
                    }
                    if ui.button("+").on_hover_text("Zoom timeline in").clicked() {
                        self.pixels_per_second = (self.pixels_per_second * 2.).min(3000.);
                    }
                    if ui
                        .button("Fit")
                        .on_hover_text("Fit the full sequence")
                        .clicked()
                    {
                        self.pixels_per_second = ((f64::from(viewport_width) - 24.) * 1000.
                            / total.max(1) as f64)
                            .clamp(0.02, 3000.);
                    }
                    if ui
                        .add_enabled(selected.is_some(), egui::Button::new("Focus clip"))
                        .on_hover_text("Zoom to selected clip; use this for very short cuts")
                        .clicked()
                        && let Some(clip) = studio.clips().iter().find(|c| Some(c.id()) == selected)
                    {
                        self.pixels_per_second = ((f64::from(viewport_width) * 0.6) * 1000.
                            / clip.duration_ms().max(1) as f64)
                            .clamp(0.02, 3000.);
                        self.focus_selected = true;
                    }
                });
                let mut value = playhead.min(total);
                let width = (ui.available_width() - 175.).max(90.);
                ui.spacing_mut().slider_width = width;
                if ui
                    .add_enabled(
                        enabled && !self.dragging(),
                        egui::Slider::new(&mut value, 0..=total)
                            .text("Playhead")
                            .show_value(false)
                            .clamping(egui::SliderClamping::Always),
                    )
                    .changed()
                {
                    result.seek = Some(snap(value).min(total));
                }
                ui.label(egui::RichText::new(time(playhead)).size(11.));
            });
        });
        let scale = self.pixels_per_second / 1000.;
        let width = (total as f64 * scale + 24.).max(f64::from(viewport_width)) as f32;
        egui::ScrollArea::horizontal()
            .id_salt("visual-sequence-track")
            .animated(false)
            .auto_shrink([false, true])
            .show(ui, |ui| {
                let (canvas, canvas_response) =
                    ui.allocate_exact_size(Vec2::new(width, CANVAS_HEIGHT), Sense::hover());
                let visible = ui.clip_rect();
                let ruler = Rect::from_min_max(
                    canvas.min,
                    Pos2::new(canvas.right(), canvas.top() + TRACK_Y - 2.),
                );
                let ruler_response =
                    ui.interact(ruler, ui.id().with("ruler"), Sense::click_and_drag());
                ruler_response.widget_info(|| {
                    egui::WidgetInfo::labeled(
                        egui::WidgetType::Slider,
                        enabled,
                        "Timeline ruler. Drag to seek",
                    )
                });
                if enabled
                    && (ruler_response.clicked() || ruler_response.dragged())
                    && let Some(p) = ruler_response.interact_pointer_pos()
                {
                    result.seek = Some(
                        snap(((f64::from(p.x - canvas.left()) / scale).max(0.) as u64).min(total))
                            .min(total),
                    );
                }
                draw_ruler(ui, canvas, visible, scale, total);
                let mut positions = Vec::with_capacity(studio.clips().len());
                let mut offset = 0u64;
                for clip in studio.clips() {
                    let x = canvas.left() + (offset as f64 * scale) as f32;
                    let end = canvas.left()
                        + ((offset.saturating_add(clip.duration_ms())) as f64 * scale) as f32;
                    positions.push((
                        clip.id(),
                        Rect::from_min_max(
                            Pos2::new(x, canvas.top() + TRACK_Y),
                            Pos2::new(end.max(x + 1.), canvas.top() + TRACK_Y + TRACK_HEIGHT),
                        ),
                        offset,
                    ));
                    offset = offset.saturating_add(clip.duration_ms());
                }
                if self.focus_selected {
                    if let Some((_, rect, _)) =
                        positions.iter().find(|(id, _, _)| Some(*id) == selected)
                    {
                        ui.scroll_to_rect(*rect, Some(egui::Align::Center));
                    }
                    self.focus_selected = false;
                }
                if let Some(gesture) = &mut self.gesture
                    && let Some(pointer) = ui.input(|i| i.pointer.interact_pos())
                {
                    let delta = f64::from(pointer.x - canvas.left()) - gesture.anchor_x;
                    match &mut gesture.kind {
                        Kind::Trim {
                            edge,
                            original_in,
                            original_out,
                            in_ms,
                            out_ms,
                            source_duration,
                            sequence_start,
                            ..
                        } => {
                            let minimum = 33.min(original_out.saturating_sub(*original_in)).max(1);
                            if *edge == Edge::Start {
                                let raw = (*original_in as f64 + delta / scale)
                                    .clamp(0., u64::MAX as f64)
                                    as u64;
                                *in_ms = snap(raw).min(original_out.saturating_sub(minimum));
                            } else {
                                let raw = (*original_out as f64 + delta / scale)
                                    .clamp(0., u64::MAX as f64)
                                    as u64;
                                *out_ms = snap(raw)
                                    .clamp(original_in.saturating_add(minimum), *source_duration);
                            }
                            let edge_ms = if *edge == Edge::Start {
                                *in_ms
                            } else {
                                out_ms.saturating_sub(1)
                            };
                            result.seek = Some(
                                sequence_start
                                    .saturating_add(edge_ms.saturating_sub(*original_in).min(
                                        original_out.saturating_sub(*original_in).saturating_sub(1),
                                    ))
                                    .min(total),
                            );
                        }
                        Kind::Move { to_index } => {
                            *to_index = positions
                                .iter()
                                .filter(|(id, rect, _)| {
                                    *id != gesture.clip && pointer.x > rect.center().x
                                })
                                .count();
                        }
                    }
                    if pointer.y >= canvas.top() && pointer.y <= canvas.bottom() {
                        if pointer.x > visible.right() - 26. {
                            ui.scroll_with_delta(Vec2::new(-20., 0.));
                        } else if pointer.x < visible.left() + 26. {
                            ui.scroll_with_delta(Vec2::new(20., 0.));
                        }
                    }
                    ui.ctx()
                        .request_repaint_after(std::time::Duration::from_millis(16));
                }
                for (index, clip) in studio.clips().iter().enumerate() {
                    let (_, base, sequence_start) = positions[index];
                    let mut rect = base;
                    let mut trimmed = false;
                    if let Some(g) = &self.gesture
                        && g.clip == clip.id()
                        && let Kind::Trim {
                            original_in,
                            original_out,
                            in_ms,
                            out_ms,
                            ..
                        } = &g.kind
                    {
                        ui.painter().rect_filled(base, 5., crate::SURFACE);
                        rect.min.x += ((*in_ms as f64 - *original_in as f64) * scale) as f32;
                        rect.max.x += ((*out_ms as f64 - *original_out as f64) * scale) as f32;
                        trimmed = true;
                    }
                    if !rect.intersects(visible)
                        && self.gesture.as_ref().is_none_or(|g| g.clip != clip.id())
                    {
                        continue;
                    }
                    let active = Some(clip.id()) == selected
                        || self.gesture.as_ref().is_some_and(|g| g.clip == clip.id());
                    ui.painter().rect_filled(
                        rect,
                        5.,
                        if active {
                            crate::ACCENT
                        } else {
                            Color32::from_rgb(225, 218, 239)
                        },
                    );
                    ui.painter().rect_stroke(
                        rect,
                        5.,
                        Stroke::new(if active { 2_f32 } else { 1_f32 }, crate::ACCENT),
                        egui::StrokeKind::Inside,
                    );
                    let waveform = waveforms.state(clip.source_id());
                    if matches!(waveform, WaveformState::Waiting) && waveforms.needs_wake() {
                        ui.ctx().request_repaint();
                    }
                    let (source_in, source_out) = match self.gesture.as_ref() {
                        Some(g) if g.clip == clip.id() => match g.kind {
                            Kind::Trim { in_ms, out_ms, .. } => (in_ms, out_ms),
                            _ => (clip.in_ms(), clip.out_ms()),
                        },
                        _ => (clip.in_ms(), clip.out_ms()),
                    };
                    draw_waveform(ui, rect, visible, source_in, source_out, &waveform, active);
                    let handles = rect.width() >= 52.;
                    let handle = if handles { HANDLE } else { 0. };
                    let body = Rect::from_min_max(
                        Pos2::new(rect.left() + handle, rect.top()),
                        Pos2::new(rect.right() - handle, rect.bottom()),
                    );
                    let response = ui.interact(
                        body,
                        ui.id().with(("clip", clip.id().number())),
                        Sense::click_and_drag(),
                    );
                    response.widget_info(|| {
                        egui::WidgetInfo::labeled(
                            egui::WidgetType::Button,
                            enabled,
                            format!(
                                "Timeline clip {}: {}. Drag to reorder",
                                index + 1,
                                clip.name()
                            ),
                        )
                    });
                    let response = response.on_hover_text(match &waveform {
                        WaveformState::Error(error) => {
                            format!("{error}\nRight-click this clip → Retry waveforms")
                        }
                        _ => waveform.label().to_owned(),
                    });
                    if matches!(waveform, WaveformState::Error(_)) {
                        response.context_menu(|ui| {
                            if ui.button("Retry waveforms").clicked() {
                                waveforms.retry();
                                ui.close();
                            }
                        });
                    }
                    if enabled && response.clicked() {
                        result.selected = Some(clip.id());
                        if let Some(p) = response.interact_pointer_pos() {
                            result.seek = Some(
                                sequence_start
                                    .saturating_add(snap(
                                        ((f64::from(p.x - base.left()) / scale).max(0.) as u64)
                                            .min(clip.duration_ms().saturating_sub(1)),
                                    ))
                                    .min(total),
                            );
                        }
                    }
                    if enabled && response.drag_started() && self.gesture.is_none() {
                        let p = ui
                            .input(|i| i.pointer.press_origin())
                            .unwrap_or(body.center());
                        self.gesture = Some(Gesture {
                            clip: clip.id(),
                            baseline: studio.clips().to_vec(),
                            anchor_x: f64::from(p.x - canvas.left()),
                            kind: Kind::Move { to_index: index },
                        });
                        result.selected = Some(clip.id());
                    }
                    if rect.width() > 65. {
                        let text_rect = rect.shrink2(Vec2::new(handle + 4., 4.));
                        let painter = ui.painter().with_clip_rect(text_rect.intersect(visible));
                        painter.text(
                            text_rect.left_top(),
                            egui::Align2::LEFT_TOP,
                            clip.name(),
                            egui::FontId::proportional(12.),
                            if active { crate::CARD } else { crate::INK },
                        );
                        // A solid duration badge keeps text readable above audio.
                        let duration_badge = Rect::from_min_max(
                            Pos2::new(text_rect.left() - 2., text_rect.bottom() - 12.),
                            Pos2::new(
                                (text_rect.left() + 71.).min(text_rect.right()),
                                text_rect.bottom() + 2.,
                            ),
                        );
                        painter.rect_filled(
                            duration_badge,
                            3.,
                            if active {
                                crate::ACCENT
                            } else {
                                Color32::from_rgb(225, 218, 239)
                            },
                        );
                        painter.text(
                            text_rect.left_bottom(),
                            egui::Align2::LEFT_BOTTOM,
                            time(if trimmed {
                                match &self.gesture.as_ref().unwrap().kind {
                                    Kind::Trim { in_ms, out_ms, .. } => out_ms - in_ms,
                                    _ => clip.duration_ms(),
                                }
                            } else {
                                clip.duration_ms()
                            }),
                            egui::FontId::proportional(10.),
                            if active { crate::CARD } else { crate::BODY },
                        );
                    }
                    if handles {
                        for edge in [Edge::Start, Edge::End] {
                            let x = if edge == Edge::Start {
                                rect.left()
                            } else {
                                rect.right() - HANDLE
                            };
                            let area = Rect::from_min_size(
                                Pos2::new(x, rect.top()),
                                Vec2::new(HANDLE, TRACK_HEIGHT),
                            );
                            ui.painter().rect_filled(
                                area,
                                4.,
                                if active {
                                    crate::ACCENT_HOVER
                                } else {
                                    Color32::from_rgb(194, 179, 222)
                                },
                            );
                            for dx in [-2., 2.] {
                                ui.painter().line_segment(
                                    [
                                        area.center() + Vec2::new(dx, -9.),
                                        area.center() + Vec2::new(dx, 9.),
                                    ],
                                    Stroke::new(
                                        1.5_f32,
                                        if active { crate::CARD } else { crate::BODY },
                                    ),
                                );
                            }
                            let response = ui
                                .interact(
                                    area,
                                    ui.id()
                                        .with(("edge", clip.id().number(), edge == Edge::Start)),
                                    Sense::click_and_drag(),
                                )
                                .on_hover_cursor(egui::CursorIcon::ResizeHorizontal);
                            response.widget_info(|| {
                                egui::WidgetInfo::labeled(
                                    egui::WidgetType::Button,
                                    enabled,
                                    format!(
                                        "Trim {} of {}",
                                        if edge == Edge::Start { "start" } else { "end" },
                                        clip.name()
                                    ),
                                )
                            });
                            if enabled && response.clicked() {
                                result.selected = Some(clip.id());
                            }
                            if enabled
                                && response.drag_started()
                                && self.gesture.is_none()
                                && let Some(source) =
                                    studio.media().iter().find(|m| m.id() == clip.source_id())
                            {
                                let p = ui
                                    .input(|i| i.pointer.press_origin())
                                    .unwrap_or(area.center());
                                self.gesture = Some(Gesture {
                                    clip: clip.id(),
                                    baseline: studio.clips().to_vec(),
                                    anchor_x: f64::from(p.x - canvas.left()),
                                    kind: Kind::Trim {
                                        edge,
                                        original_in: clip.in_ms(),
                                        original_out: clip.out_ms(),
                                        in_ms: clip.in_ms(),
                                        out_ms: clip.out_ms(),
                                        source: clip.source_id(),
                                        source_duration: source.duration_ms(),
                                        sequence_start,
                                    },
                                });
                                result.selected = Some(clip.id());
                            }
                        }
                    }
                }
                for cue in studio.cues() {
                    if let Ok(at) = studio.cue_sequence_time(cue.id()) {
                        let x = canvas.left() + (at as f64 * scale) as f32;
                        if x >= visible.left() && x <= visible.right() {
                            ui.painter().text(
                                Pos2::new(x, canvas.top() + 73.),
                                egui::Align2::LEFT_TOP,
                                format!("●{}", cue.slot().number()),
                                egui::FontId::proportional(10.),
                                crate::ACCENT,
                            );
                        }
                    }
                }
                let x = canvas.left() + (playhead.min(total) as f64 * scale) as f32;
                ui.painter().line_segment(
                    [
                        Pos2::new(x, canvas.top()),
                        Pos2::new(x, canvas.bottom() - 3.),
                    ],
                    Stroke::new(2_f32, Color32::from_rgb(166, 42, 67)),
                );
                if let Some(g) = &self.gesture
                    && let Kind::Move { to_index } = g.kind
                {
                    let others: Vec<_> = positions
                        .iter()
                        .filter(|(id, _, _)| *id != g.clip)
                        .collect();
                    let x = others.get(to_index).map_or_else(
                        || others.last().map_or(canvas.left(), |(_, r, _)| r.right()),
                        |(_, r, _)| r.left(),
                    );
                    ui.painter().line_segment(
                        [
                            Pos2::new(x, canvas.top() + TRACK_Y - 3.),
                            Pos2::new(x, canvas.top() + TRACK_Y + TRACK_HEIGHT + 3.),
                        ],
                        Stroke::new(4_f32, crate::ACCENT_HOVER),
                    );
                    if let Some(p) = ui.input(|i| i.pointer.interact_pos()) {
                        if let Some((_, original, _)) =
                            positions.iter().find(|(id, _, _)| *id == g.clip)
                        {
                            let ghost = original.translate(Vec2::new(
                                p.x - canvas.left() - g.anchor_x as f32,
                                p.y - original.center().y,
                            ));
                            ui.painter().rect_filled(
                                ghost,
                                5.,
                                Color32::from_rgba_unmultiplied(120, 96, 181, 220),
                            );
                            if let Some(clip) = studio.clips().iter().find(|c| c.id() == g.clip) {
                                ui.painter()
                                    .with_clip_rect(ghost.shrink(5.).intersect(visible))
                                    .text(
                                        ghost.left_center() + Vec2::new(8., 0.),
                                        egui::Align2::LEFT_CENTER,
                                        clip.name(),
                                        egui::FontId::proportional(12.),
                                        crate::CARD,
                                    );
                            }
                        }
                        ui.painter().text(
                            p + Vec2::new(12., -14.),
                            egui::Align2::LEFT_BOTTOM,
                            format!("Drop at position {}", to_index + 1),
                            egui::FontId::proportional(12.),
                            crate::INK,
                        );
                    }
                }
                if self.gesture.is_some() && ui.input(|i| i.pointer.any_released()) {
                    let inside = ui
                        .input(|i| i.pointer.interact_pos())
                        .is_some_and(|p| visible.contains(p));
                    let g = self.gesture.take().unwrap();
                    match g.kind {
                        Kind::Trim {
                            edge,
                            original_in,
                            original_out,
                            in_ms,
                            out_ms,
                            sequence_start,
                            ..
                        } => {
                            if (in_ms, out_ms) != (original_in, original_out) {
                                result.edit = Some(Edit::TrimClip {
                                    clip: g.clip,
                                    in_ms,
                                    out_ms,
                                });
                            }
                            result.seek =
                                Some(sequence_start.saturating_add(if edge == Edge::Start {
                                    0
                                } else {
                                    out_ms.saturating_sub(in_ms).saturating_sub(1)
                                }));
                        }
                        Kind::Move { to_index }
                            if inside
                                && studio.clips().iter().position(|c| c.id() == g.clip)
                                    != Some(to_index) =>
                        {
                            result.edit = Some(Edit::MoveClip {
                                clip: g.clip,
                                to_index,
                            });
                        }
                        _ => {}
                    }
                }
                // Preserve drag-and-drop from the existing take bin.
                if self.gesture.is_none()
                    && enabled
                    && let Some(payload) = canvas_response.dnd_release_payload::<ClipId>()
                {
                    let pointer = ui
                        .input(|i| i.pointer.interact_pos())
                        .unwrap_or(canvas.center());
                    let target = positions
                        .iter()
                        .filter(|(id, r, _)| *id != *payload && pointer.x > r.center().x)
                        .count();
                    if studio.clips().iter().any(|c| c.id() == *payload) {
                        result.edit = Some(Edit::MoveClip {
                            clip: *payload,
                            to_index: target,
                        });
                        result.selected = Some(*payload);
                    }
                }
            });
        result
    }
}
fn snap(ms: u64) -> u64 {
    let frame = (u128::from(ms) * 30 + 500) / 1000;
    ((frame * 1000 + 15) / 30).min(u128::from(u64::MAX)) as u64
}
fn time(ms: u64) -> String {
    format!("{:02}:{:02}.{:03}", ms / 60000, (ms / 1000) % 60, ms % 1000)
}
/// Map from a clip's full rectangle, never from its viewport crop. That keeps
/// peaks anchored to source time while scrolling, zooming or drafting a trim.
fn waveform_interval(
    rect: Rect,
    source_in: u64,
    source_out: u64,
    left: f32,
    right: f32,
) -> (u64, u64) {
    let duration = source_out.saturating_sub(source_in);
    let start = (f64::from(left - rect.left()) / f64::from(rect.width().max(1.))).clamp(0., 1.);
    let end = (f64::from(right - rect.left()) / f64::from(rect.width().max(1.))).clamp(0., 1.);
    (
        source_in
            .saturating_add((start * duration as f64).floor() as u64)
            .min(source_out),
        source_in
            .saturating_add((end * duration as f64).ceil() as u64)
            .min(source_out),
    )
}
fn draw_waveform(
    ui: &egui::Ui,
    rect: Rect,
    visible: Rect,
    source_in: u64,
    source_out: u64,
    state: &WaveformState<'_>,
    active: bool,
) {
    let lane = Rect::from_min_max(
        Pos2::new(rect.left() + 2., rect.top() + 19.),
        Pos2::new(rect.right() - 2., rect.bottom() - 3.),
    );
    let clipped = lane.intersect(visible);
    if clipped.width() <= 0. || clipped.height() <= 0. {
        return;
    }
    let painter = ui.painter().with_clip_rect(clipped);
    let color = if active {
        Color32::from_rgb(238, 229, 255)
    } else {
        crate::ACCENT_HOVER
    };
    if let WaveformState::Ready(envelope) = state {
        let stride = (clipped.width() / 4096.).max(2.5);
        let mut x = clipped.left();
        while x < clipped.right() {
            let right = (x + stride).min(clipped.right());
            let (start, end) = waveform_interval(rect, source_in, source_out, x, right);
            let height = (envelope.peak(start, end).sqrt() * lane.height() * 0.48).max(0.5);
            painter.line_segment(
                [
                    Pos2::new(x, lane.center().y - height),
                    Pos2::new(x, lane.center().y + height),
                ],
                Stroke::new(1.5_f32, color),
            );
            x += stride;
        }
    } else {
        painter.line_segment(
            [
                Pos2::new(clipped.left(), lane.center().y),
                Pos2::new(clipped.right(), lane.center().y),
            ],
            Stroke::new(1_f32, color.gamma_multiply(0.35)),
        );
        if clipped.width() > 110. {
            let text_color = if active { crate::CARD } else { crate::BODY };
            painter.text(
                Pos2::new(clipped.center().x, lane.top()),
                egui::Align2::CENTER_TOP,
                state.label(),
                egui::FontId::proportional(10.),
                text_color,
            );
        }
    }
}
fn draw_ruler(ui: &egui::Ui, canvas: Rect, visible: Rect, scale: f64, total: u64) {
    let target = 70. / scale;
    let steps = [
        33u64, 100, 500, 1000, 2000, 5000, 10000, 30000, 60000, 300000, 600000, 3600000, 36000000,
    ];
    let step = steps
        .into_iter()
        .find(|s| *s as f64 >= target)
        .unwrap_or(36000000);
    let first =
        (((f64::from(visible.left() - canvas.left()) / scale).max(0.) as u64) / step) * step;
    for i in 0..256u64 {
        let at = first.saturating_add(i.saturating_mul(step));
        if at > total {
            break;
        }
        let x = canvas.left() + (at as f64 * scale) as f32;
        if x > visible.right() {
            break;
        }
        ui.painter().line_segment(
            [
                Pos2::new(x, canvas.top() + 17.),
                Pos2::new(x, canvas.top() + 23.),
            ],
            Stroke::new(1_f32, crate::MUTED),
        );
        ui.painter().text(
            Pos2::new(x + 3., canvas.top() + 2.),
            egui::Align2::LEFT_TOP,
            time(at),
            egui::FontId::proportional(10.),
            crate::BODY,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use video_core::{Editor, OverlayContent, OverlayKind, SlotKey};
    struct Harness {
        ctx: egui::Context,
        ui: TimelineUi,
        editor: Editor,
        selected: Option<ClipId>,
        at: u64,
        waveforms: WaveformsUi,
    }
    impl Harness {
        fn new() -> Self {
            let mut editor = Editor::new(Studio::default()).unwrap();
            let source = editor.studio().next_media_id();
            editor
                .apply(Edit::AddMedia {
                    path: "media/generated.mp4".into(),
                    duration_ms: 30_000,
                })
                .unwrap();
            for (index, (start, end)) in [(1000, 6000), (6000, 10_000), (10_000, 14_000)]
                .into_iter()
                .enumerate()
            {
                editor
                    .apply(Edit::AddClip {
                        source,
                        name: format!("Clip {}", index + 1),
                        in_ms: start,
                        out_ms: end,
                    })
                    .unwrap();
            }
            let first = editor.studio().clips()[0].id();
            editor
                .apply(Edit::AddCueFromSnapshot {
                    clip: first,
                    offset_ms: 2000,
                    slot: SlotKey::try_from(1).unwrap(),
                    content: OverlayContent::new(OverlayKind::Quote, "Generated cue"),
                    duration_ms: 1000,
                })
                .unwrap();
            let ctx = egui::Context::default();
            crate::configure(&ctx);
            ctx.enable_accesskit();
            Self {
                ctx,
                ui: TimelineUi::default(),
                editor,
                selected: Some(first),
                at: 0,
                waveforms: WaveformsUi::default(),
            }
        }
        fn frame(&mut self, events: Vec<egui::Event>) -> egui::FullOutput {
            let mut response = None;
            let output = self.ctx.run(
                egui::RawInput {
                    screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(1160., 400.))),
                    focused: true,
                    events,
                    ..Default::default()
                },
                |ctx| {
                    egui::CentralPanel::default().show(ctx, |ui| {
                        response = Some(self.ui.show(
                            ui,
                            self.editor.studio(),
                            self.selected,
                            self.at,
                            true,
                            &self.waveforms,
                        ));
                    });
                },
            );
            let response = response.unwrap();
            if let Some(id) = response.selected {
                self.selected = Some(id);
            }
            if let Some(edit) = response.edit {
                self.editor.apply(edit).unwrap();
            }
            if let Some(at) = response.seek {
                self.at = at.min(self.editor.studio().duration_ms());
            }
            output
        }
        fn bounds(&mut self, label: &str) -> Rect {
            self.frame(vec![]);
            let out = self.frame(vec![]);
            let update = out.platform_output.accesskit_update.unwrap();
            let bounds = update
                .nodes
                .iter()
                .find(|(_, n)| n.label() == Some(label))
                .unwrap_or_else(|| panic!("Missing {label}"))
                .1
                .bounds()
                .unwrap();
            Rect::from_min_max(
                Pos2::new(bounds.x0 as f32, bounds.y0 as f32),
                Pos2::new(bounds.x1 as f32, bounds.y1 as f32),
            )
        }
        fn begin_drag(&mut self, pos: Pos2) {
            self.frame(pointer(pos, true));
            self.frame(vec![egui::Event::PointerMoved(pos + Vec2::new(7., 0.))]);
        }
    }
    fn pointer(pos: Pos2, pressed: bool) -> Vec<egui::Event> {
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
    #[test]
    fn source_waveform_alignment_survives_trim_split_scroll_zoom_and_draft() {
        let full = Rect::from_min_size(Pos2::new(0., 0.), Vec2::new(400., 46.));
        assert_eq!(
            waveform_interval(full, 1000, 5000, 100., 200.),
            (2000, 3000)
        );
        // A cropped viewport does not stretch the visible section back to zero.
        assert_eq!(
            waveform_interval(full.translate(Vec2::new(-200., 0.)), 1000, 5000, -100., 0.),
            (2000, 3000)
        );
        assert_eq!(
            waveform_interval(
                Rect::from_min_size(Pos2::ZERO, Vec2::new(800., 46.)),
                1000,
                5000,
                200.,
                400.
            ),
            (2000, 3000)
        );
        // The right split starts from the same original source millisecond.
        assert_eq!(
            waveform_interval(
                Rect::from_min_size(Pos2::new(500., 0.), Vec2::new(200., 46.)),
                3000,
                5000,
                550.,
                600.
            ),
            (3500, 4000)
        );
        // Extending the uncommitted end reveals source beyond the old out point.
        assert_eq!(
            waveform_interval(
                Rect::from_min_size(Pos2::ZERO, Vec2::new(500., 46.)),
                1000,
                6000,
                400.,
                500.
            ),
            (5000, 6000)
        );
    }
    #[test]
    fn visible_waveform_does_not_intercept_handle_or_body_pointer_gestures() {
        let mut h = Harness::new();
        let source = h.editor.studio().media()[0].clone();
        h.waveforms.test_ready(
            source,
            video_core::waveform::Analysis::Audio(
                video_core::waveform::Envelope::from_peaks(30_000, vec![0.1, 0.8, 0., 0.6])
                    .unwrap(),
            ),
        );
        h.frame(vec![]);
        let output = h.frame(vec![]);
        let wave_lines = output.shapes.iter().filter(|s| matches!(&s.shape,
            egui::Shape::LineSegment { stroke, .. } if stroke.color == Color32::from_rgb(238, 229, 255)
        )).count();
        assert!(
            wave_lines > 20,
            "Ready audio must actually paint its waveform"
        );
        let handle = h.bounds("Trim start of Clip 1").center();
        let history = h.editor.undo_len();
        h.begin_drag(handle);
        h.frame(pointer(handle + Vec2::new(64., 0.), false));
        assert_eq!(h.editor.studio().clips()[0].in_ms(), 2000);
        assert_eq!(h.editor.undo_len(), history + 1);
        let first = h.editor.studio().clips()[0].id();
        let body = h
            .bounds("Timeline clip 1: Clip 1. Drag to reorder")
            .center();
        let last = h.bounds("Timeline clip 3: Clip 3. Drag to reorder");
        h.begin_drag(body);
        h.frame(pointer(Pos2::new(last.right() - 4., body.y), false));
        assert_eq!(h.editor.studio().clips().last().unwrap().id(), first);
        assert_eq!(h.editor.undo_len(), history + 2);
    }
    #[test]
    fn pointer_trim_edges_commit_once_preserve_cues_and_undo() {
        for (edge, delta, expected_in, expected_out) in
            [("start", 64., 2000, 6000), ("end", -64., 1000, 5000)]
        {
            let mut h = Harness::new();
            let baseline = h.editor.studio().clone();
            let history = h.editor.undo_len();
            let bounds = h.bounds(&format!("Trim {edge} of Clip 1"));
            let start = bounds.center();
            h.begin_drag(start);
            h.frame(vec![egui::Event::PointerMoved(
                start + Vec2::new(delta, 0.),
            )]);
            assert!(h.ui.dragging());
            assert!(h.ui.draft_source_preview().is_some());
            assert_eq!(h.editor.studio(), &baseline, "Drag must remain a UI draft");
            h.frame(pointer(start + Vec2::new(delta, 0.), false));
            let clip = &h.editor.studio().clips()[0];
            assert_eq!((clip.in_ms(), clip.out_ms()), (expected_in, expected_out));
            assert_eq!(h.editor.undo_len(), history + 1);
            assert_eq!(h.editor.studio().media(), baseline.media());
            assert_eq!(clip.in_ms() + h.editor.studio().cues()[0].offset_ms(), 3000);
            h.editor.undo();
            assert_eq!(h.editor.studio(), &baseline);
        }
    }
    #[test]
    fn pointer_clip_reorder_moves_the_whole_unit_with_one_undo() {
        let mut h = Harness::new();
        let baseline = h.editor.studio().clone();
        let history = h.editor.undo_len();
        let start = h
            .bounds("Timeline clip 1: Clip 1. Drag to reorder")
            .center();
        let last = h.bounds("Timeline clip 3: Clip 3. Drag to reorder");
        let end = Pos2::new(last.right() - 22., start.y);
        h.begin_drag(start);
        h.frame(vec![egui::Event::PointerMoved(end)]);
        assert_eq!(h.editor.studio(), &baseline);
        h.frame(pointer(end, false));
        let clip = h.editor.studio().clips().last().unwrap();
        assert_eq!(clip.id(), baseline.clips()[0].id());
        assert_eq!((clip.in_ms(), clip.out_ms()), (1000, 6000));
        assert_eq!(h.editor.undo_len(), history + 1);
        assert_eq!(h.editor.studio().cues()[0].offset_ms(), 2000);
        assert_eq!(
            h.editor
                .studio()
                .cue_sequence_time(h.editor.studio().cues()[0].id())
                .unwrap(),
            10_000
        );
        h.editor.undo();
        assert_eq!(h.editor.studio(), &baseline);
    }
    #[test]
    fn escape_and_external_sequence_change_cancel_uncommitted_gestures() {
        let mut h = Harness::new();
        let baseline = h.editor.studio().clone();
        let history = h.editor.undo_len();
        let start = h.bounds("Trim start of Clip 1").center();
        h.begin_drag(start);
        h.frame(vec![egui::Event::PointerMoved(start + Vec2::new(64., 0.))]);
        h.frame(vec![egui::Event::Key {
            key: egui::Key::Escape,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        }]);
        h.frame(pointer(start + Vec2::new(64., 0.), false));
        assert!(!h.ui.dragging());
        assert_eq!(h.editor.studio(), &baseline);
        assert_eq!(h.editor.undo_len(), history);
        let start = h.bounds("Trim end of Clip 1").center();
        h.begin_drag(start);
        let clip = h.editor.studio().clips()[1].id();
        h.editor
            .apply(Edit::MoveClip { clip, to_index: 0 })
            .unwrap();
        let changed = h.editor.studio().clone();
        h.frame(pointer(start - Vec2::new(64., 0.), false));
        assert!(!h.ui.dragging());
        assert_eq!(h.editor.studio(), &changed);
    }
    #[test]
    fn ruler_seek_and_split_use_the_core_and_preserve_original_media() {
        let mut h = Harness::new();
        let original = h.editor.studio().media().to_vec();
        let ruler = h.bounds("Timeline ruler. Drag to seek");
        let pos = Pos2::new(ruler.left() + 128., ruler.center().y);
        h.frame(pointer(pos, true));
        h.frame(pointer(pos, false));
        assert_eq!(h.at, 2000);
        h.editor
            .apply(Edit::SplitAtPlayhead { sequence_ms: h.at })
            .unwrap();
        assert_eq!(h.editor.studio().clips().len(), 4);
        assert_eq!(h.editor.studio().media(), original);
        assert_eq!(h.editor.studio().cues()[0].offset_ms(), 0);
    }
    #[test]
    fn focus_clip_reveals_frame_length_handles_in_a_long_sequence() {
        let mut h = Harness::new();
        let source = h.editor.studio().media()[0].id();
        let short = h.editor.studio().next_clip_id();
        h.editor
            .apply(Edit::AddClip {
                source,
                name: "Short".into(),
                in_ms: 0,
                out_ms: 33,
            })
            .unwrap();
        h.selected = Some(short);
        let button = h.bounds("Focus clip").center();
        h.frame(pointer(button, true));
        h.frame(pointer(button, false));
        for _ in 0..3 {
            h.frame(vec![]);
        }
        for label in ["Trim start of Short", "Trim end of Short"] {
            let r = h.bounds(label);
            assert!(r.left() >= 0. && r.right() <= 1160., "{label}: {r:?}");
            assert!(r.width() >= 18.);
        }
    }
    #[test]
    fn trim_bounds_snap_and_never_remove_the_last_frame() {
        let mut h = Harness::new();
        let start = h.bounds("Trim start of Clip 1").center();
        h.begin_drag(start);
        h.frame(vec![egui::Event::PointerMoved(
            start + Vec2::new(10000., 0.),
        )]);
        h.frame(pointer(start + Vec2::new(10000., 0.), false));
        let c = &h.editor.studio().clips()[0];
        assert!(c.out_ms() - c.in_ms() >= 33);
        assert!(c.out_ms() <= 30_000);
        h.editor.studio().validate().unwrap();
        for ms in [0, 1, 32, 33, 66, 67, 999, 1000, 5999, 6000] {
            assert!(snap(ms).abs_diff(ms) <= 17);
        }
    }
}
