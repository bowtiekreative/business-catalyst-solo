//! Sidebar Video milestone 1. This native layout does not open devices or project files.
use eframe::egui::{self, Color32, FontFamily, FontId, RichText, Stroke, Vec2};
use video_core::{Orientation, Studio};

const CANVAS: Color32 = Color32::from_rgb(247, 246, 242);
const CARD: Color32 = Color32::WHITE;
const SURFACE: Color32 = Color32::from_rgb(248, 249, 252);
const INK: Color32 = Color32::from_rgb(16, 19, 26);
const BODY: Color32 = Color32::from_rgb(59, 63, 73);
const MUTED: Color32 = Color32::from_rgb(107, 111, 121);
const ACCENT: Color32 = Color32::from_rgb(120, 96, 181);
const ACCENT_HOVER: Color32 = Color32::from_rgb(102, 80, 157);
const HAIRLINE: Color32 = Color32::from_rgba_premultiplied(3, 3, 4, 41);
const REGULAR: &[u8] = include_bytes!("../assets/fonts/Inter-Regular.ttf");
const SEMIBOLD: &[u8] = include_bytes!("../assets/fonts/Inter-SemiBold.ttf");

fn strong(text: impl Into<String>, size: f32) -> RichText {
    RichText::new(text).font(FontId::new(size, FontFamily::Name("Inter 600".into())))
}
fn hint(ui: &mut egui::Ui, text: &str) {
    ui.label(RichText::new(text).size(12.).color(MUTED));
}
fn card(radius: u8) -> egui::Frame {
    egui::Frame::new()
        .fill(CARD)
        .corner_radius(radius)
        .stroke(Stroke::new(1.0_f32, HAIRLINE))
        .inner_margin(16)
}
fn pending(ui: &mut egui::Ui, label: &str, reason: &str) -> egui::Response {
    ui.add_enabled(
        false,
        egui::Button::new(label).min_size(egui::vec2(44., 44.)),
    )
    .on_disabled_hover_text(reason)
}
fn configure(ctx: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();
    fonts.font_data.insert(
        "Inter 400".into(),
        egui::FontData::from_static(REGULAR).into(),
    );
    fonts.font_data.insert(
        "Inter 600".into(),
        egui::FontData::from_static(SEMIBOLD).into(),
    );
    fonts
        .families
        .insert(FontFamily::Proportional, vec!["Inter 400".into()]);
    fonts
        .families
        .insert(FontFamily::Monospace, vec!["Inter 400".into()]);
    fonts.families.insert(
        FontFamily::Name("Inter 600".into()),
        vec!["Inter 600".into()],
    );
    ctx.set_fonts(fonts);
    ctx.set_visuals(egui::Visuals::light());
    ctx.style_mut(|s| {
        s.animation_time = 0.; // Static foundation also respects reduced-motion preferences.
        s.spacing.item_spacing = egui::vec2(10., 8.);
        s.spacing.button_padding = egui::vec2(14., 10.);
        s.spacing.interact_size = egui::vec2(44., 18.); // Every interactive control sets its own 44 px minimum.
        s.visuals.override_text_color = Some(BODY);
        s.visuals.weak_text_color = Some(MUTED);
        s.visuals.panel_fill = CANVAS;
        s.visuals.window_fill = CARD;
        s.visuals.disabled_alpha = 1.; // Keep pending-feature labels readable; AccessKit marks them disabled.
        s.visuals.selection.bg_fill = ACCENT;
        s.visuals.selection.stroke = Stroke::new(1.0_f32, CARD);
        for w in [
            &mut s.visuals.widgets.noninteractive,
            &mut s.visuals.widgets.inactive,
        ] {
            w.bg_fill = SURFACE;
            w.weak_bg_fill = SURFACE;
            w.bg_stroke = Stroke::new(1.0_f32, HAIRLINE);
            w.fg_stroke = Stroke::new(1.0_f32, BODY);
            w.corner_radius = 8.into();
        }
        s.visuals.widgets.hovered.bg_fill = CANVAS;
        s.visuals.widgets.hovered.weak_bg_fill = CANVAS;
        s.visuals.widgets.hovered.fg_stroke = Stroke::new(1.0_f32, BODY);
        s.visuals.widgets.hovered.bg_stroke = Stroke::new(1.0_f32, ACCENT_HOVER);
        s.visuals.widgets.hovered.corner_radius = 8.into();
        s.visuals.widgets.active.bg_fill = ACCENT;
        s.visuals.widgets.active.weak_bg_fill = ACCENT;
        s.visuals.widgets.active.fg_stroke = Stroke::new(1.0_f32, CARD);
        s.visuals.widgets.active.corner_radius = 8.into();
        for style in [
            egui::TextStyle::Body,
            egui::TextStyle::Button,
            egui::TextStyle::Monospace,
        ] {
            s.text_styles.insert(style, FontId::proportional(14.));
        }
        s.text_styles
            .insert(egui::TextStyle::Small, FontId::proportional(12.));
        s.text_styles.insert(
            egui::TextStyle::Heading,
            FontId::new(22., FontFamily::Name("Inter 600".into())),
        );
    });
}

struct SidebarVideo {
    studio: Studio,
    name_draft: String,
    name_error: Option<String>,
}
impl Default for SidebarVideo {
    fn default() -> Self {
        let studio = Studio::default();
        Self {
            name_draft: studio.name().into(),
            studio,
            name_error: None,
        }
    }
}
impl SidebarVideo {
    fn header(&mut self, ctx: &egui::Context) {
        egui::TopBottomPanel::top("sidebar_header")
            .exact_height(112.)
            .frame(
                egui::Frame::new()
                    .fill(SURFACE)
                    .inner_margin(egui::Margin::symmetric(24, 16)),
            )
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.vertical(|ui| {
                        ui.set_width(165.);
                        ui.label(strong("Storyfold", 24.).color(INK));
                        hint(ui, "RECORD STUDIO");
                    });
                    ui.add_space(12.);
                    ui.vertical(|ui| {
                        ui.set_width(260.);
                        let label =
                            ui.label(RichText::new("Recording name").size(11.).color(MUTED));
                        let response = ui.add_sized(
                            [260., 44.],
                            egui::TextEdit::singleline(&mut self.name_draft)
                                .char_limit(120)
                                .hint_text("Untitled recording"),
                        );
                        let changed = response.changed();
                        response.labelled_by(label.id);
                        if changed {
                            self.name_error = self
                                .studio
                                .rename(&self.name_draft)
                                .err()
                                .map(|e| e.to_string());
                        }
                    });
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        pending(
                            ui,
                            "Export MP4",
                            "MP4 export is not available in this layout preview.",
                        );
                        ui.add_space(10.);
                        for orientation in [Orientation::Vertical, Orientation::Horizontal] {
                            let selected = self.studio.orientation() == orientation;
                            let text = strong(orientation.label(), 13.).color(if selected {
                                CARD
                            } else {
                                BODY
                            });
                            let mut button =
                                egui::Button::new(text).min_size(egui::vec2(162., 44.));
                            if selected {
                                button = button.fill(ACCENT);
                            }
                            if ui.add(button).clicked() {
                                self.studio.set_orientation(orientation);
                            }
                        }
                    });
                });
                if let Some(error) = &self.name_error {
                    ui.label(RichText::new(error).size(11.).color(BODY));
                }
            });
    }
    fn takes(&self, ui: &mut egui::Ui) {
        card(16).show(ui,|ui| {
            ui.set_min_height((ui.available_height()-2.).max(0.));
            egui::ScrollArea::vertical().id_salt("sidebar_takes_content").auto_shrink([false,false]).show(ui,|ui| {
            ui.horizontal(|ui| {ui.label(strong("Your takes",14.).color(INK));ui.with_layout(egui::Layout::right_to_left(egui::Align::Center),|ui|{hint(ui,"0 clips");});});
            ui.add_space(18.);
            egui::Frame::new().fill(SURFACE).corner_radius(12).inner_margin(16).show(ui,|ui| {
                ui.set_min_height(148.);
                ui.vertical_centered(|ui| {
                    ui.add_space(21.);camera_icon(ui,egui::vec2(52.,40.),MUTED);
                    ui.add_space(15.);ui.label(strong("Your first take goes here.",13.).color(INK));
                    hint(ui,"No clips have been recorded or imported.");
                });
            });
            ui.add_space(16.);
            pending(ui,"Add media","Adding media is not available in this layout preview.");
            ui.add_space(12.);hint(ui,"Recording and clip editing are not available in this layout preview.");
            ui.add_space(15.);ui.separator();ui.add_space(12.);
            ui.label(strong("Local by design",12.).color(INK));
            hint(ui,"No camera, microphone or AI model is running. Name and shape changes last only for this window.");
            });
        });
    }
    fn overlays(&self, ui: &mut egui::Ui) {
        card(16).show(ui,|ui| {
            ui.set_min_height((ui.available_height()-2.).max(0.));
            ui.horizontal(|ui|{ui.label(strong("On-screen cards",14.).color(INK));ui.with_layout(egui::Layout::right_to_left(egui::Align::Center),|ui|{hint(ui,"1–9");});});
            hint(ui,"Nine empty slots. Cards and timing are not available in this layout preview.");ui.add_space(12.);
            egui::ScrollArea::vertical().id_salt("sidebar_slots").max_height((ui.available_height()-104.).max(120.)).show(ui,|ui| {
                for slot in self.studio.slots() {
                    let number=slot.key().number();
                    let text=format!("{number}     Empty overlay slot");
                    ui.add_enabled(false,egui::Button::new(RichText::new(text).size(13.)).min_size(egui::vec2(ui.available_width(),44.)))
                        .on_disabled_hover_text(format!("Overlay slot {number}. Cards and keyboard triggers are not available in this layout preview."));
                }
            });
            ui.add_space(12.);
            pending(ui,"Build with local AI","The local AI builder is not available in this layout preview.");
            ui.add_space(8.);hint(ui,"Lower third · stat · screenshot · list · photo · quote");
        });
    }
    fn stage(&self, ui: &mut egui::Ui) {
        let geometry = self.studio.orientation().output_size();
        ui.horizontal(|ui| {
            ui.label(strong("Your recording", 15.).color(INK));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                hint(ui, &format!("{} × {}", geometry.width, geometry.height));
            });
        });
        ui.add_space(12.);
        let stage_height = (ui.available_height() - 214.).max(160.);
        card(24).show(ui, |ui| {
            ui.set_min_size(egui::vec2(ui.available_width(), stage_height));
            ui.set_max_height(stage_height);
            let available = ui.available_size();
            let aspect = geometry.aspect_ratio();
            let width = available.x.min(available.y * aspect);
            let size = egui::vec2(width, width / aspect);
            let stage = egui::Rect::from_center_size(ui.max_rect().center(), size);
            ui.painter().rect_filled(stage, 16, CANVAS);
            // Empty framing guide; no illustrative person or simulated camera picture.
            for fraction in [1. / 3., 2. / 3.] {
                let x = egui::lerp(stage.x_range(), fraction);
                let y = egui::lerp(stage.y_range(), fraction);
                ui.painter().line_segment(
                    [egui::pos2(x, stage.top()), egui::pos2(x, stage.bottom())],
                    Stroke::new(1.0_f32, Color32::from_white_alpha(150)),
                );
                ui.painter().line_segment(
                    [egui::pos2(stage.left(), y), egui::pos2(stage.right(), y)],
                    Stroke::new(1.0_f32, Color32::from_white_alpha(150)),
                );
            }
            let label = stage.center() + egui::vec2(0., -8.);
            ui.painter().text(
                label,
                egui::Align2::CENTER_CENTER,
                "No camera open",
                FontId::new(
                    18_f32.min(stage.width() / 9.),
                    FontFamily::Name("Inter 600".into()),
                ),
                INK,
            );
            ui.painter().text(
                label + egui::vec2(0., 29.),
                egui::Align2::CENTER_CENTER,
                "Layout preview",
                FontId::proportional(12.),
                BODY,
            );
            ui.allocate_rect(ui.max_rect(), egui::Sense::hover())
                .widget_info(|| {
                    egui::WidgetInfo::labeled(
                        egui::WidgetType::Label,
                        false,
                        "Camera stage. No camera open. Layout preview.",
                    )
                });
        });
        ui.add_space(16.);
        ui.vertical_centered(|ui| {
            ui.add_enabled_ui(false,|ui| {
                let (rect,response)=ui.allocate_exact_size(egui::vec2(88.,88.),egui::Sense::click());
                ui.painter().circle_filled(rect.center(),43.,CARD);
                ui.painter().circle_stroke(rect.center(),43.,Stroke::new(2.0_f32,ACCENT));
                ui.painter().circle_filled(rect.center(),38.,ACCENT);
                ui.painter().circle_filled(rect.center(),13.,CARD);
                response.widget_info(||egui::WidgetInfo::labeled(egui::WidgetType::Button,false,"Record. Recording is not available in this layout preview."));
                response.on_disabled_hover_text("Recording is not available in this layout preview. The camera and microphone are off.");
            });
            ui.add_space(8.);ui.label(strong("Hold = clip · Tap = long take",13.).color(BODY));
            ui.label(RichText::new("Recording is not available in this layout preview.").size(12.).color(BODY));
        });
    }
    fn sequence(&self, ctx: &egui::Context) {
        egui::TopBottomPanel::bottom("sidebar_sequence")
            .exact_height(248.)
            .frame(
                egui::Frame::new()
                    .fill(CANVAS)
                    .inner_margin(egui::Margin::symmetric(20, 12)),
            )
            .show(ctx, |ui| {
                card(16).show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.label(strong("Your sequence", 14.).color(INK));
                        hint(ui, "0 clips · 00:00");
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            pending(
                                ui,
                                "Redo",
                                "Undo and redo are not available in this layout preview.",
                            );
                            pending(
                                ui,
                                "Undo",
                                "Undo and redo are not available in this layout preview.",
                            );
                        });
                    });
                    ui.add_space(6.);
                    track(
                        ui,
                        "CLIPS",
                        "Your takes will appear here in their recording order.",
                    );
                    ui.add_space(8.);
                    track(
                        ui,
                        "OVERLAYS",
                        "Overlay cues will follow their clips. No cues are present.",
                    );
                    ui.add_space(8.);
                    ui.horizontal(|ui| {
                        for label in [
                            "Move left",
                            "Move right",
                            "Split at playhead",
                            "Trim start",
                            "Trim end",
                            "Remove",
                        ] {
                            pending(
                                ui,
                                label,
                                "Clip editing is not available in this layout preview.",
                            );
                        }
                    });
                });
            });
    }
    fn show(&mut self, ctx: &egui::Context) {
        self.header(ctx);
        self.sequence(ctx);
        egui::SidePanel::left("sidebar_takes")
            .exact_width(250.)
            .resizable(false)
            .frame(
                egui::Frame::new()
                    .fill(CANVAS)
                    .inner_margin(egui::Margin::symmetric(12, 14)),
            )
            .show(ctx, |ui| self.takes(ui));
        egui::SidePanel::right("sidebar_cards")
            .exact_width(304.)
            .resizable(false)
            .frame(
                egui::Frame::new()
                    .fill(CANVAS)
                    .inner_margin(egui::Margin::symmetric(12, 14)),
            )
            .show(ctx, |ui| self.overlays(ui));
        egui::CentralPanel::default()
            .frame(
                egui::Frame::new()
                    .fill(CANVAS)
                    .inner_margin(egui::Margin::symmetric(8, 14)),
            )
            .show(ctx, |ui| self.stage(ui));
    }
}
fn camera_icon(ui: &mut egui::Ui, size: Vec2, color: Color32) {
    let (rect, _) = ui.allocate_exact_size(size, egui::Sense::hover());
    let body = rect.shrink2(egui::vec2(6., 6.));
    ui.painter().rect_stroke(
        body,
        8,
        Stroke::new(1.5_f32, color),
        egui::StrokeKind::Inside,
    );
    ui.painter()
        .circle_stroke(body.center(), 8., Stroke::new(1.5_f32, color));
}
fn track(ui: &mut egui::Ui, label: &str, message: &str) {
    egui::Frame::new()
        .fill(SURFACE)
        .corner_radius(8)
        .inner_margin(10)
        .show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            ui.horizontal(|ui| {
                ui.add_sized(
                    [80., 18.],
                    egui::Label::new(RichText::new(label).size(10.).color(MUTED)),
                );
                hint(ui, message);
            });
        });
}
impl eframe::App for SidebarVideo {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.show(ctx);
        #[cfg(feature = "validation-screenshot")]
        if ctx.cumulative_pass_nr() < 3 {
            ctx.request_repaint();
        }
    }
}
fn main() -> eframe::Result {
    #[cfg(feature = "validation-screenshot")]
    let minimum_preview = std::env::var_os("SIDEBAR_VIDEO_VALIDATION_MIN").is_some();
    #[cfg(not(feature = "validation-screenshot"))]
    let minimum_preview = false;
    let size = if minimum_preview {
        [1160., 800.]
    } else {
        [1480., 1000.]
    };
    let mut app = SidebarVideo::default();
    if minimum_preview {
        app.studio.set_orientation(Orientation::Vertical);
    }
    eframe::run_native(
        "Storyfold · Record studio foundation",
        eframe::NativeOptions {
            viewport: egui::ViewportBuilder::default()
                .with_inner_size(size)
                .with_min_inner_size([1160., 800.]),
            ..Default::default()
        },
        Box::new(move |cc| {
            configure(&cc.egui_ctx);
            Ok(Box::new(app))
        }),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn both_shapes_draw_with_bundled_fonts_and_accessible_pending_controls() {
        let ctx = egui::Context::default();
        configure(&ctx);
        ctx.enable_accesskit();
        let mut app = SidebarVideo::default();
        for (orientation, size) in [
            (Orientation::Horizontal, [1480., 1000.]),
            (Orientation::Vertical, [1160., 800.]),
        ] {
            app.studio.set_orientation(orientation);
            let output = ctx.run(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(size[0], size[1]),
                    )),
                    ..Default::default()
                },
                |ctx| app.show(ctx),
            );
            assert!(!output.shapes.is_empty());
            let update = output
                .platform_output
                .accesskit_update
                .expect("AccessKit must emit a native accessibility tree");
            let labels: Vec<_> = update
                .nodes
                .iter()
                .filter_map(|(_, node)| node.label())
                .collect();
            assert!(labels.iter().any(|label| label.contains("Export MP4")));
            assert!(
                labels
                    .iter()
                    .any(|label| label.contains("Record. Recording is not available"))
            );
            assert!(
                labels
                    .iter()
                    .any(|label| label.contains("Empty overlay slot"))
            );
            let record = update
                .nodes
                .iter()
                .find(|(_, node)| {
                    node.label()
                        .is_some_and(|label| label.starts_with("Record. Recording"))
                })
                .unwrap()
                .1
                .bounds()
                .expect("The Record control must expose bounds");
            assert!(
                record.y0 >= 112. && record.y1 <= f64::from(size[1] - 248.),
                "Record must fit above the sequence at each supported size"
            );
            assert!(record.x0 >= 0. && record.x1 <= f64::from(size[0]));
        }
    }
    #[test]
    fn design_text_colors_meet_contrast_on_their_actual_surfaces() {
        fn luminance(c: Color32) -> f32 {
            let linear = |value: u8| {
                let s = f32::from(value) / 255.;
                if s <= 0.04045 {
                    s / 12.92
                } else {
                    ((s + 0.055) / 1.055).powf(2.4)
                }
            };
            0.2126 * linear(c.r()) + 0.7152 * linear(c.g()) + 0.0722 * linear(c.b())
        }
        for (text, surface) in [
            (MUTED, CARD),
            (MUTED, SURFACE),
            (BODY, CANVAS),
            (INK, CANVAS),
            (CARD, ACCENT),
        ] {
            let a = luminance(text);
            let b = luminance(surface);
            assert!((a.max(b) + 0.05) / (a.min(b) + 0.05) >= 4.5);
        }
    }
}
