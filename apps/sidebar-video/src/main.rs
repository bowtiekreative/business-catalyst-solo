//! Storyfold native Record studio. UI forwards edits to video-core.
use eframe::egui::{self, Color32, FontFamily, FontId, RichText, Stroke, Vec2};
mod ai;
mod capture;
#[cfg(feature = "validation-screenshot")]
mod input_probe;
mod inserts;
mod output;
mod overlays;
mod scrub;
mod timeline;
mod waveforms;
use output::{Action, OutputUi};
use overlays::OverlaysUi;
mod workspace;
mod youtube;
use capture::CaptureUi;
use video_core::{ClipId, Direction, Edit, EditOutcome, Nudge, Orientation, TrimEdge};
use workspace::Workspace;

const CANVAS: Color32 = Color32::from_rgb(247, 246, 242);
const CARD: Color32 = Color32::WHITE;
const SURFACE: Color32 = Color32::from_rgb(248, 249, 252);
const INK: Color32 = Color32::from_rgb(16, 19, 26);
const BODY: Color32 = Color32::from_rgb(59, 63, 73);
const MUTED: Color32 = Color32::from_rgb(107, 111, 121);
const ACCENT: Color32 = Color32::from_rgb(120, 96, 181);
const ACCENT_HOVER: Color32 = Color32::from_rgb(102, 80, 157);
const HAIRLINE: Color32 = Color32::from_rgba_premultiplied(3, 3, 4, 41);
const REGULAR: &[u8] = video_core::renderer::INTER_REGULAR;
const SEMIBOLD: &[u8] = video_core::renderer::INTER_SEMIBOLD;

fn strong(text: impl Into<String>, size: f32) -> RichText {
    RichText::new(text).font(FontId::new(size, FontFamily::Name("Inter 600".into())))
}
fn hint(ui: &mut egui::Ui, text: &str) {
    ui.label(RichText::new(text).size(12.).color(MUTED));
}
fn compact_row(ui: &mut egui::Ui, draw: impl FnOnce(&mut egui::Ui)) {
    ui.scope(|ui| {
        ui.spacing_mut().interact_size.y = 18.;
        ui.horizontal(draw);
    });
}
fn card(radius: u8) -> egui::Frame {
    egui::Frame::new()
        .fill(CARD)
        .corner_radius(radius)
        .stroke(Stroke::new(1.0_f32, HAIRLINE))
        .inner_margin(16)
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
        s.spacing.interact_size = egui::vec2(44., 44.);
        s.visuals.override_text_color = None;
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

#[derive(Clone, Copy)]
enum Intent {
    New,
    Open,
    Close,
}
struct TrimDraft {
    clip: ClipId,
    name: String,
    start: u64,
    end: u64,
}
#[derive(Default)]
struct SidebarVideo {
    ai: ai::AiUi,
    youtube: youtube::YoutubeUi,
    project: Workspace,
    capture: CaptureUi,
    cards: OverlaysUi,
    output: OutputUi,
    timeline: timeline::TimelineUi,
    waveforms: waveforms::WaveformsUi,
    scrub: scrub::ScrubUi,
    name_draft: String,
    selected: Option<ClipId>,
    playhead_ms: u64,
    show_removed: bool,
    confirm: Option<Intent>,
    version_summary: Option<String>,
    trim: Option<TrimDraft>,
    allow_close: bool,
}
impl SidebarVideo {
    fn new() -> Self {
        let project = Workspace::default();
        Self {
            name_draft: project.studio().name().into(),
            project,
            ..Self::default()
        }
    }
    fn dirty(&self) -> bool {
        self.project.dirty() || self.name_draft != self.project.studio().name()
    }
    fn flush_name(&mut self) -> bool {
        if self.name_draft == self.project.studio().name() {
            return true;
        }
        self.project
            .edit(Edit::Rename(self.name_draft.clone()))
            .is_some()
    }
    fn request(&mut self, intent: Intent, ctx: &egui::Context) {
        if self.timeline.dragging() {
            self.project.status =
                "Release the timeline drag or press Escape before continuing.".into();
        } else if self.youtube.open || self.youtube.busy() {
            self.project.status =
                "Finish or close YouTube before replacing the project or closing.".into();
        } else if self.ai.open || self.ai.busy() {
            self.project.status =
                "Finish or close the AI review before replacing the project or closing.".into();
        } else if self.cards.editing() {
            self.cards.error =
                Some("Approve or cancel this card before replacing the project or closing.".into());
        } else if !self.capture.can_leave() || self.cards.busy() {
            self.capture.error = Some("Stop the camera and wait for takes and media operations to finish before opening another project or closing.".into());
        } else if self.project.busy() || self.output.busy() {
            self.project.status = "Wait for the current project operation to finish.".into();
        } else if self.dirty() {
            self.confirm = Some(intent);
        } else {
            self.perform(intent, ctx);
        }
    }
    fn perform(&mut self, intent: Intent, ctx: &egui::Context) {
        match intent {
            Intent::New => self.project.new_project(ctx),
            Intent::Open => self.project.open_project(ctx),
            Intent::Close => {
                self.allow_close = true;
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
        }
    }
    fn show_camera_settings(&mut self, ctx: &egui::Context) {
        self.capture.show_settings = true;
        if self.capture.devices.is_none() {
            self.capture.refresh(ctx);
        }
    }
    fn save(&mut self, ctx: &egui::Context, version: bool) {
        if self.project.busy() || !self.flush_name() {
            return;
        }
        if version {
            self.version_summary = Some(String::new());
        } else {
            self.project.save(ctx, None);
        }
    }
    fn select(&mut self, clip: ClipId) {
        self.selected = Some(clip);
        if let Ok(time) = self.project.studio().clip_sequence_time(clip) {
            self.playhead_ms = time;
        }
    }
    fn edit(&mut self, edit: Edit) {
        self.timeline.cancel();
        if let Some(EditOutcome::ClipSplit(clip)) = self.project.edit(edit) {
            self.selected = Some(clip);
        }
        self.sanitize_view();
    }
    fn sanitize_view(&mut self) {
        if self
            .selected
            .is_some_and(|id| !self.project.studio().clips().iter().any(|c| c.id() == id))
        {
            self.selected = None;
        }
        self.playhead_ms = self.playhead_ms.min(self.project.studio().duration_ms());
    }
    fn history(&mut self, redo: bool) {
        self.timeline.cancel();
        if self.project.loading() {
            return;
        }
        if redo {
            self.project.editor.redo();
        } else {
            self.project.editor.undo();
        }
        self.name_draft = self.project.studio().name().into();
        self.project.error = None;
        self.sanitize_view();
    }
    fn header(&mut self, ctx: &egui::Context) {
        egui::TopBottomPanel::top("sidebar_header")
            .exact_height(112.)
            .frame(
                egui::Frame::new()
                    .fill(SURFACE)
                    .inner_margin(egui::Margin::symmetric(20, 12)),
            )
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.vertical(|ui| {
                        ui.set_width(126.);
                        ui.label(strong("Storyfold", 24.).color(INK));
                        hint(ui, "RECORD STUDIO");
                        #[cfg(feature = "validation-screenshot")]
                        if input_probe::enabled() {
                            ui.label(RichText::new("Input check").size(12.).color(ACCENT));
                        }
                    });
                    let mut intent = None;
                    let mut save = None;
                    ui.add_enabled_ui(!self.project.busy(), |ui| {
                        let _project_menu = ui.menu_button("Project", |ui| {
                            if ui.button("New project…").clicked() {
                                intent = Some(Intent::New);
                                ui.close();
                            }
                            if ui.button("Open project…").clicked() {
                                intent = Some(Intent::Open);
                                ui.close();
                            }
                            if ui
                                .add_enabled(
                                    !self.output.busy()
                                        && !self.ai.open
                                        && !self.cards.busy()
                                        && !self.cards.editing(),
                                    egui::Button::new("YouTube…"),
                                )
                                .clicked()
                            {
                                self.youtube.show_window();
                                ui.close();
                            }
                            if ui
                                .add_enabled(
                                    self.capture.can_leave() && self.project.store().is_some(),
                                    egui::Button::new("Recover interrupted recordings…"),
                                )
                                .clicked()
                            {
                                self.capture.find_recovery(&self.project, ctx);
                                ui.close();
                            }
                            ui.separator();
                            let mut prefer_gpu = video_core::encoding::prefer_gpu();
                            if ui
                                .add_enabled(
                                    self.capture.can_leave() && !self.output.busy(),
                                    egui::Checkbox::new(
                                        &mut prefer_gpu,
                                        "Prefer NVIDIA GPU encoding",
                                    ),
                                )
                                .on_hover_text("Uses NVENC when available for the next recording or render. Audio, decoding and some video filters still use the CPU.")
                                .changed()
                            {
                                video_core::encoding::set_prefer_gpu(prefer_gpu);
                                self.output.encoding_changed();
                            }
                            ui.separator();
                            if ui.button("Save    Ctrl+S").clicked() {
                                save = Some(false);
                                ui.close();
                            }
                            if ui.button("Save version…    Ctrl+Shift+S").clicked() {
                                save = Some(true);
                                ui.close();
                            }
                        });
                        #[cfg(feature = "validation-screenshot")]
                        input_probe::project_menu(
                            &_project_menu.response,
                            _project_menu.inner.is_some(),
                        );
                    });
                    if let Some(intent) = intent {
                        self.request(intent, ctx);
                    }
                    if let Some(version) = save {
                        self.save(ctx, version);
                    }
                    if ui
                        .button(if self.capture.camera_running() {
                            "Camera on"
                        } else {
                            "Camera…"
                        })
                        .clicked()
                    {
                        self.show_camera_settings(ctx);
                    }
                    ui.add_enabled_ui(!self.project.loading(), |ui| {
                        ui.vertical(|ui| {
                            ui.set_width(220.);
                            let label = ui.label(
                                RichText::new(if self.dirty() {
                                    "Recording name · unsaved"
                                } else {
                                    "Recording name"
                                })
                                .size(11.)
                                .color(MUTED),
                            );
                            let response = ui.add_sized(
                                [220., 44.],
                                egui::TextEdit::singleline(&mut self.name_draft)
                                    .char_limit(120)
                                    .hint_text("Untitled recording"),
                            );
                            let commit = response.lost_focus()
                                || (response.has_focus()
                                    && ui.input(|i| i.key_pressed(egui::Key::Enter)));
                            response.labelled_by(label.id);
                            if commit {
                                self.flush_name();
                            }
                        });
                    });
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        self.output_button(ui, Action::Export);
                        ui.add_enabled_ui(
                            !self.project.loading() && self.capture.can_leave(),
                            |ui| {
                                for orientation in [Orientation::Vertical, Orientation::Horizontal]
                                {
                                    let selected =
                                        self.project.studio().orientation() == orientation;
                                    let mut button = egui::Button::new(
                                        strong(orientation.label(), 13.).color(if selected {
                                            CARD
                                        } else {
                                            BODY
                                        }),
                                    )
                                    .min_size(egui::vec2(152., 44.));
                                    if selected {
                                        button = button.fill(ACCENT);
                                    }
                                    if ui.add(button).clicked() {
                                        self.edit(Edit::SetOrientation(orientation));
                                    }
                                }
                            },
                        );
                    });
                });
                let status = self
                    .project
                    .error
                    .as_deref()
                    .unwrap_or(&self.project.status);
                let label = ui.add(
                    egui::Label::new(RichText::new(status).size(12.).color(
                        if self.project.error.is_some() {
                            INK
                        } else {
                            BODY
                        },
                    ))
                    .truncate(),
                );
                label.on_hover_text(self.project.folder.as_ref().map_or_else(
                    || status.to_owned(),
                    |p| format!("{status}\n{}", p.display()),
                ));
            });
    }
    fn clip_button(
        &mut self,
        ui: &mut egui::Ui,
        clip: &video_core::Clip,
        index: usize,
        width: f32,
    ) {
        let text = format!(
            "{}  {}\n{}",
            index + 1,
            clip.name(),
            timecode(clip.duration_ms())
        );
        let response = ui.add_sized(
            [width, 48.],
            egui::Button::new(text)
                .selected(self.selected == Some(clip.id()))
                .sense(egui::Sense::click_and_drag()),
        );
        response.dnd_set_drag_payload(clip.id());
        if response.clicked() {
            self.select(clip.id());
        }
        if let Some(payload) = response.dnd_release_payload::<ClipId>() {
            self.edit(Edit::MoveClip {
                clip: *payload,
                to_index: index,
            });
        }
        response.on_hover_text(
            "Select a clip or drag it to another position. The bin and sequence share this order.",
        );
    }
    fn takes(&mut self, ui: &mut egui::Ui) {
        card(16).show(ui, |ui| {
            ui.set_min_height((ui.available_height()-2.).max(0.));
            egui::ScrollArea::vertical().id_salt("sidebar_takes_content").auto_shrink([false,false]).show(ui, |ui| {
                ui.horizontal(|ui| { ui.label(strong("Your takes",14.).color(INK)); hint(ui,&format!("{} clips",self.project.studio().clips().len())); });
                ui.add_enabled_ui(!self.project.loading(), |ui| {
                    ui.horizontal(|ui| {
                        ui.selectable_value(&mut self.show_removed,false,"Sequence");
                        ui.selectable_value(&mut self.show_removed,true,"Removed");
                    });
                    ui.add_space(10.);
                    if self.show_removed {
                        let removed=self.project.studio().removed_clips().to_vec();
                        if removed.is_empty() { hint(ui,"Removed clips will stay here for restoration."); }
                        for item in removed {
                            ui.label(strong(item.clip().name(),13.));
                            hint(ui,&timecode(item.clip().duration_ms()));
                            if ui.button("Restore to sequence").clicked() {
                                self.edit(Edit::RestoreClip {clip:item.clip().id(),to_index:item.former_index().min(self.project.studio().clips().len())});
                            }
                            ui.separator();
                        }
                    } else {
                        let clips=self.project.studio().clips().to_vec();
                        if clips.is_empty() {
                            egui::Frame::new().fill(SURFACE).corner_radius(12).inner_margin(16).show(ui, |ui| {
                                ui.set_min_height(130.);
                                ui.vertical_centered(|ui| {
                                    ui.add_space(16.); camera_icon(ui,egui::vec2(52.,40.),MUTED);
                                    ui.add_space(12.); ui.label(strong("Your first take goes here.",13.).color(INK));
                                    hint(ui,"No clips in this project yet.");
                                });
                            });
                        }
                        for (index,clip) in clips.iter().enumerate() { self.clip_button(ui,clip,index,ui.available_width()); }
                    }
                });
                ui.add_space(12.);
                if ui.add_enabled(self.capture.can_leave() && !self.project.busy() && self.project.store().is_some(),egui::Button::new("Add media…")).on_disabled_hover_text("Save a project and stop its camera before importing local media.").clicked(){ self.capture.import(&self.project,ui.ctx()); }
                if self.capture.busy(){ui.horizontal(|ui|{ui.spinner();ui.label(&self.capture.message);if ui.button("Cancel media operation").clicked(){self.capture.cancel_operation();}});}
                ui.add_space(12.);ui.separator();ui.add_space(8.);
                ui.label(strong("Saved on your computer",12.).color(INK));
                hint(ui,"Project → Save stores your timeline. Save version adds a local Git checkpoint. Media files stay intact when you trim or remove clips.");
            });
        });
    }
    fn overlays(&mut self, ui: &mut egui::Ui) {
        card(16).show(ui, |ui| {
            ui.set_min_height((ui.available_height()-2.).max(0.));
            ui.horizontal(|ui| {ui.label(strong("On-screen cards",14.).color(INK));hint(ui,"1–9");});
            hint(ui,"Edit and approve a card. Show adds a cue at the playhead or in your active take; inserts use their chosen duration.");
            ui.checkbox(&mut self.cards.reduced_motion,"Reduce card motion");
            let slots=self.project.studio().slots().to_vec();
            egui::ScrollArea::vertical().id_salt("sidebar_slots").max_height((ui.available_height()-112.).max(100.)).show(ui, |ui| {
                for slot in slots {
                    ui.horizontal(|ui| {
                        let text=format!("{}  {}",slot.key().number(),slot.kind().map_or("Empty card",|kind| kind.label()));
                        if ui.add_sized([ui.available_width()-72.,44.],egui::Button::new(text)).clicked(){self.cards.edit(slot.key(),slot.content().cloned());}
                        if ui.add_enabled(slot.content().is_some(),egui::Button::new("Show").min_size(egui::vec2(62.,44.))).clicked(){self.cards.fire(slot.key(),&mut self.project,&mut self.capture,self.playhead_ms);}
                    });
                }
            });
            if ui.add_enabled(!self.project.busy()&&self.project.store().is_some(),egui::Button::new("Build with local AI")).clicked(){self.ai.show_window(&self.project,ui.ctx());}
            if let Some(error)=&self.cards.error{ui.colored_label(Color32::from_rgb(160,30,45),error);}
        });
    }
    fn output_button(&mut self, ui: &mut egui::Ui, action: Action) {
        let label = match (action, self.capture.camera_running()) {
            (Action::Preview, true) => "Stop camera & preview",
            (Action::Export, true) => "Stop camera & export",
            (Action::Preview, false) => "Preview sequence",
            (Action::Export, false) => "Export MP4",
        };
        let response = ui
            .add_enabled(
                !self.project.busy()
                    && !self.output.busy()
                    && !self.cards.busy()
                    && !self.ai.open
                    && !self.ai.busy()
                    && !self.youtube.open
                    && !self.youtube.busy()
                    && !self.cards.editing()
                    && self.project.store().is_some()
                    && (!self.project.studio().clips().is_empty() || self.capture.taking()),
                egui::Button::new(label).min_size(egui::vec2(44., 44.)),
            )
            .on_hover_text(match action {
                Action::Preview => "Play your current cuts without exporting an MP4.",
                Action::Export => "Render the finished MP4 and save a local project version.",
            })
            .on_disabled_hover_text(
                "Create a project and record or import a clip before previewing or exporting.",
            );
        #[cfg(feature = "validation-screenshot")]
        if matches!(action, Action::Preview) {
            input_probe::preview_control(&response);
        }
        if response.clicked() && self.flush_name() {
            self.output.request_from_button(
                action,
                &mut self.project,
                &mut self.capture,
                ui.ctx(),
                self.cards.reduced_motion,
            );
        }
    }
    fn stage(&mut self, ui: &mut egui::Ui) {
        let geometry = self.project.studio().orientation().output_size();
        compact_row(ui, |ui| {
            ui.label(strong("Your recording", 15.).color(INK));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let response = ui.add_enabled(
                    self.capture.can_record() && !self.project.loading(),
                    egui::Button::new(if self.capture.taking() {
                        "Stop & save"
                    } else {
                        "Start recording"
                    }),
                );
                #[cfg(feature = "validation-screenshot")]
                input_probe::record_toggle(&response);
                if response.clicked() {
                    self.capture.toggle_recording();
                }
                hint(ui, &format!("{} × {}", geometry.width, geometry.height));
            });
        });
        if let Some(error) = &self.capture.error {
            ui.colored_label(Color32::from_rgb(160, 30, 45), error);
        }
        if !self.capture.camera_running() {
            if let Some(error) = &self.scrub.error {
                ui.colored_label(Color32::from_rgb(160, 30, 45), error);
            } else if self.scrub.busy() {
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.label("Preparing source preview…");
                });
            }
        }
        ui.add_space(12.);
        let stage_height = (ui.available_height() - 230.).max(90.);
        card(24).show(ui, |ui| {
            ui.set_min_size(egui::vec2(ui.available_width(), stage_height));
            ui.set_max_height(stage_height);
            let available = ui.available_size();
            let aspect = geometry.aspect_ratio();
            let width = available.x.min(available.y * aspect);
            let stage = egui::Rect::from_center_size(
                ui.max_rect().center(),
                egui::vec2(width, width / aspect),
            );
            ui.painter().rect_filled(stage, 16, CANVAS);
            if self.project.store().is_none() {
                ui.allocate_ui_with_layout(
                    available,
                    egui::Layout::top_down(egui::Align::Center),
                    |ui| {
                        ui.add_space(((available.y - 104.) / 2.).max(0.));
                        ui.label(strong("Start a recording project", 18.).color(INK));
                        hint(
                            ui,
                            if self.project.busy() {
                                "Finish or cancel the folder selection in the system dialog."
                            } else {
                                "Create or open a project to enable recording."
                            },
                        );
                        ui.horizontal(|ui| {
                            ui.add_space(((ui.available_width() - 306.) / 2.).max(0.));
                            if ui
                                .add_enabled(
                                    !self.project.busy(),
                                    egui::Button::new("Create project…")
                                        .min_size(egui::vec2(148., 44.)),
                                )
                                .clicked()
                            {
                                self.request(Intent::New, ui.ctx());
                            }
                            if ui
                                .add_enabled(
                                    !self.project.busy(),
                                    egui::Button::new("Open project…")
                                        .min_size(egui::vec2(148., 44.)),
                                )
                                .clicked()
                            {
                                self.request(Intent::Open, ui.ctx());
                            }
                        });
                    },
                );
            } else if let Some(texture) = self.capture.texture.as_ref().or_else(|| {
                if !self.capture.camera_running() {
                    self.scrub.texture.as_ref()
                } else {
                    None
                }
            }) {
                let source = texture.size_vec2();
                let ratio = (stage.width() / source.x).max(stage.height() / source.y);
                let size = stage.size() / ratio / source;
                let uv = egui::Rect::from_center_size(egui::pos2(0.5, 0.5), size);
                ui.painter().image(texture.id(), stage, uv, Color32::WHITE);
            } else {
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
                let waiting = self.capture.waiting_for_preview();
                let busy = self.capture.busy();
                ui.painter().text(
                    label,
                    egui::Align2::CENTER_CENTER,
                    if waiting {
                        "Starting camera…"
                    } else if busy {
                        "Working…"
                    } else {
                        "Camera is off"
                    },
                    FontId::new(
                        18_f32.min(stage.width() / 9.),
                        FontFamily::Name("Inter 600".into()),
                    ),
                    INK,
                );
                ui.painter().text(
                    label + egui::vec2(0., 29.),
                    egui::Align2::CENTER_CENTER,
                    if waiting {
                        "Record becomes available when live video is ready."
                    } else if busy {
                        &self.capture.message
                    } else {
                        "Choose camera below, then Enable camera."
                    },
                    FontId::proportional(12.),
                    BODY,
                );
                ui.allocate_rect(ui.max_rect(), egui::Sense::hover())
                    .widget_info(|| {
                        egui::WidgetInfo::labeled(
                            egui::WidgetType::Label,
                            false,
                            if waiting {
                                "Camera preview starting. Record is not ready yet."
                            } else {
                                "Camera stage. Camera is off."
                            },
                        )
                    });
                if waiting || busy {
                    ui.place(
                        egui::Rect::from_center_size(
                            label + egui::vec2(0., 60.),
                            egui::vec2(22., 22.),
                        ),
                        egui::Spinner::new(),
                    );
                }
            }
            if let Some(texture) = &self.cards.texture {
                ui.painter().image(
                    texture.id(),
                    stage,
                    egui::Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(1., 1.)),
                    Color32::WHITE,
                );
            }
            if self.project.store().is_some() {
                let silent = self.capture.settings.microphone_id.is_none();
                let area = egui::Rect::from_min_size(
                    ui.max_rect().left_bottom() + egui::vec2(8., -38.),
                    egui::vec2((ui.max_rect().width() - 16.).clamp(64., 320.), 30.),
                );
                let audio = ui.place(area, egui::Button::new(RichText::new(self.capture.audio_label()).size(13.).color(if silent {Color32::WHITE} else {INK})).fill(if silent {Color32::from_rgb(160,30,45)} else {CARD}).truncate())
                    .on_hover_text("Choose the microphone in Camera settings. Stop the camera before changing audio input.");
                if audio.clicked() { self.show_camera_settings(ui.ctx()); }
            }
        });
        ui.add_space(16.);
        ui.vertical_centered(|ui| {
            ui.add_enabled_ui(self.capture.can_record() && !self.project.loading(), |ui| {
                let (rect, _) = ui.allocate_exact_size(egui::vec2(88., 88.), egui::Sense::hover());
                // Camera/error widgets change during a gesture; keep its input identity.
                let response = ui.interact(
                    rect,
                    egui::Id::new("live-record-button"),
                    egui::Sense::click(),
                );
                ui.painter().circle_filled(rect.center(), 43., CARD);
                ui.painter()
                    .circle_stroke(rect.center(), 43., Stroke::new(2.0_f32, ACCENT));
                ui.painter().circle_filled(
                    rect.center(),
                    38.,
                    if self.capture.taking() {
                        Color32::from_rgb(176, 34, 48)
                    } else if !self.capture.can_record() {
                        MUTED
                    } else {
                        ACCENT
                    },
                );
                ui.painter().circle_filled(rect.center(), 13., CARD);
                response.widget_info(|| {
                    egui::WidgetInfo::labeled(
                        egui::WidgetType::Button,
                        self.capture.can_record(),
                        "Record. Hold for a clip; tap to start or stop a long take.",
                    )
                });
                #[cfg(feature = "validation-screenshot")]
                input_probe::record_control(&response, rect);
                self.capture.record_button(&response, ui.ctx());
            });
            ui.add_space(8.);
            ui.label(
                strong(
                    if self.capture.taking() {
                        "Recording · Tap again to stop"
                    } else {
                        "Record · Hold = clip · Tap = long take"
                    },
                    13.,
                )
                .color(BODY),
            );
            ui.horizontal(|ui| {
                if self.capture.camera_running() {
                    if ui.button("Stop camera").clicked() {
                        self.capture.stop();
                    }
                    if ui
                        .add_enabled(
                            self.capture.taking(),
                            egui::Button::new("Stop and save take"),
                        )
                        .clicked()
                    {
                        self.capture.stop_take();
                    }
                    if ui
                        .add_enabled(self.capture.taking(), egui::Button::new("Cancel take"))
                        .clicked()
                    {
                        self.capture.cancel_take();
                    }
                } else if self.project.store().is_some() {
                    if self.capture.busy() || self.capture.waiting_for_preview() {
                        ui.spinner();
                    } else if ui
                        .add_enabled(
                            !self.project.busy() && self.capture.can_leave(),
                            egui::Button::new("Choose camera…"),
                        )
                        .clicked()
                    {
                        self.show_camera_settings(ui.ctx());
                    }
                }
                let timer = self.capture.timer();
                ui.add(egui::Label::new(RichText::new(&timer).size(12.).color(MUTED)).truncate())
                    .on_hover_text(timer);
            });
        });
    }
    fn sequence(&mut self, ctx: &egui::Context) {
        egui::TopBottomPanel::bottom("sidebar_sequence")
            .exact_height(288.)
            .frame(
                egui::Frame::new()
                    .fill(CANVAS)
                    .inner_margin(egui::Margin::symmetric(20, 10)),
            )
            .show(ctx, |ui| {
                card(16).show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.label(strong("Your sequence", 14.).color(INK));
                        hint(
                            ui,
                            &format!(
                                "{} clips · {}",
                                self.project.studio().clips().len(),
                                timecode(self.project.studio().duration_ms())
                            ),
                        );
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui
                                .add_enabled(
                                    !self.project.loading() && self.project.editor.can_redo(),
                                    egui::Button::new("Redo"),
                                )
                                .clicked()
                            {
                                self.history(true);
                            }
                            if ui
                                .add_enabled(
                                    !self.project.loading() && self.project.editor.can_undo(),
                                    egui::Button::new("Undo"),
                                )
                                .clicked()
                            {
                                self.history(false);
                            }
                            self.output_button(ui, Action::Preview);
                        });
                    });
                    let interaction = self.timeline.show(
                        ui,
                        self.project.studio(),
                        self.selected,
                        self.playhead_ms,
                        !self.project.loading(),
                        &self.waveforms,
                    );
                    if let Some(clip) = interaction.selected {
                        self.selected = Some(clip);
                    }
                    if let Some(edit) = interaction.edit {
                        self.edit(edit);
                    }
                    if let Some(at) = interaction.seek {
                        self.playhead_ms = at.min(self.project.studio().duration_ms());
                    }
                    if let Some(message) = interaction.message {
                        self.project.status = message;
                    }
                    ui.add_enabled_ui(!self.project.loading() && !self.timeline.dragging(), |ui| {
                        ui.horizontal(|ui| {
                            let position = self.selected.and_then(|id| {
                                self.project
                                    .studio()
                                    .clips()
                                    .iter()
                                    .position(|c| c.id() == id)
                            });
                            if ui
                                .add_enabled(
                                    position.is_some_and(|i| i > 0),
                                    egui::Button::new("Move left"),
                                )
                                .clicked()
                            {
                                self.edit(Edit::MoveClip {
                                    clip: self.selected.unwrap(),
                                    to_index: position.unwrap() - 1,
                                });
                            }
                            if ui
                                .add_enabled(
                                    position.is_some_and(|i| {
                                        i + 1 < self.project.studio().clips().len()
                                    }),
                                    egui::Button::new("Move right"),
                                )
                                .clicked()
                            {
                                self.edit(Edit::MoveClip {
                                    clip: self.selected.unwrap(),
                                    to_index: position.unwrap() + 1,
                                });
                            }
                            if ui
                                .add_enabled(
                                    !self.project.studio().clips().is_empty(),
                                    egui::Button::new("Split at playhead"),
                                )
                                .clicked()
                            {
                                self.edit(Edit::SplitAtPlayhead {
                                    sequence_ms: self.playhead_ms,
                                });
                            }
                            if ui
                                .add_enabled(position.is_some(), egui::Button::new("Trim / name…"))
                                .clicked()
                            {
                                let clip = &self.project.studio().clips()[position.unwrap()];
                                self.trim = Some(TrimDraft {
                                    clip: clip.id(),
                                    name: clip.name().into(),
                                    start: clip.in_ms(),
                                    end: clip.out_ms(),
                                });
                            }
                            if ui
                                .add_enabled(position.is_some(), egui::Button::new("Remove"))
                                .clicked()
                            {
                                self.edit(Edit::RemoveClip {
                                    clip: self.selected.unwrap(),
                                });
                            }
                        });
                    });
                });
            });
    }
    fn dialogs(&mut self, ctx: &egui::Context) {
        if let Some(intent) = self.confirm {
            let mut discard = false;
            let mut cancel = false;
            let response=egui::Modal::new(egui::Id::new("unsaved_project")).show(ctx, |ui| {
                ui.set_width(410.);ui.heading("Keep your unsaved edits?");
                ui.label("Save your project before continuing, or discard the changes in this window. Saved projects and media files will remain on disk.");
                ui.horizontal(|ui| {
                    cancel=ui.button("Keep editing").clicked();
                    discard=ui.button("Discard changes").clicked();
                });
            });
            if cancel || response.should_close() {
                self.confirm = None;
            }
            if discard {
                self.confirm = None;
                self.perform(intent, ctx);
            }
        } else if let Some(mut summary) = self.version_summary.take() {
            let mut save = false;
            let mut cancel = false;
            let response = egui::Modal::new(egui::Id::new("save_version")).show(ctx, |ui| {
                ui.set_width(440.);
                ui.heading("Save a local version");
                let label = ui.label("What changed?");
                ui.add_sized(
                    [440., 44.],
                    egui::TextEdit::singleline(&mut summary).char_limit(200),
                )
                .labelled_by(label.id);
                hint(
                    ui,
                    "The checkpoint stays in this project’s local Git history.",
                );
                ui.horizontal(|ui| {
                    cancel = ui.button("Cancel").clicked();
                    save = ui
                        .add_enabled(
                            !summary.trim().is_empty(),
                            egui::Button::new("Save version"),
                        )
                        .clicked();
                });
            });
            if save {
                self.project.save(ctx, Some(summary));
            } else if !cancel && !response.should_close() {
                self.version_summary = Some(summary);
            }
        } else if let Some(mut draft) = self.trim.take() {
            let mut done = false;
            let mut apply = false;
            let mut nudge = None;
            let response=egui::Modal::new(egui::Id::new("trim_clip")).show(ctx, |ui| {
                ui.set_width(460.);ui.heading("Trim and name clip");
                let label=ui.label("Clip name");ui.add_sized([460.,44.],egui::TextEdit::singleline(&mut draft.name).char_limit(120)).labelled_by(label.id);
                ui.horizontal(|ui| {
                    ui.label("Source in (ms)");ui.add_sized([125.,44.],egui::DragValue::new(&mut draft.start).range(0..=u64::MAX));
                    ui.label("Out (ms)");ui.add_sized([125.,44.],egui::DragValue::new(&mut draft.end).range(0..=u64::MAX));
                });
                hint(ui,"Source media stays intact. Cue timing follows trims.");
                for (edge,label) in [(TrimEdge::Start,"Start"),(TrimEdge::End,"End")] {
                    ui.horizontal(|ui| {ui.label(label);
                        for (text,direction,amount) in [("−1 s",Direction::Earlier,Nudge::Second),("−1 frame",Direction::Earlier,Nudge::Frame),("+1 frame",Direction::Later,Nudge::Frame),("+1 s",Direction::Later,Nudge::Second)] {
                            let response=ui.button(text);response.widget_info(||egui::WidgetInfo::labeled(egui::WidgetType::Button,true,format!("{label} {text}")));
                            if response.clicked(){nudge=Some(Edit::NudgeTrim{clip:draft.clip,edge,direction,amount});}
                        }
                    });
                }
                hint(ui,"Nudge buttons apply immediately. Apply fields commits the name and typed in/out values.");
                if let Some(error)=&self.project.error{ui.label(error);}
                ui.horizontal(|ui| {done=ui.button("Done").clicked();apply=ui.button("Apply fields").clicked();});
            });
            if let Some(edit) = nudge {
                self.edit(edit);
                if let Some(clip) = self
                    .project
                    .studio()
                    .clips()
                    .iter()
                    .find(|c| c.id() == draft.clip)
                {
                    draft.start = clip.in_ms();
                    draft.end = clip.out_ms();
                }
            }
            if apply {
                self.project.edit_batch(vec![
                    Edit::RenameClip {
                        clip: draft.clip,
                        name: draft.name.clone(),
                    },
                    Edit::TrimClip {
                        clip: draft.clip,
                        in_ms: draft.start,
                        out_ms: draft.end,
                    },
                ]);
                self.sanitize_view();
            }
            if !done && !response.should_close() {
                self.trim = Some(draft);
            }
        }
    }
    fn shortcuts(&mut self, ctx: &egui::Context) {
        if self.timeline.dragging() {
            if ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape)) {
                self.timeline.cancel();
            }
            return;
        }
        if self.confirm.is_some()
            || self.trim.is_some()
            || self.version_summary.is_some()
            || self.project.loading()
            || self.youtube.open
            || self.cards.editing()
            || self.capture.show_settings
            || !self.capture.recoveries.is_empty()
        {
            return;
        }
        let save_version = ctx.input_mut(|i| {
            i.consume_key(
                egui::Modifiers::COMMAND | egui::Modifiers::SHIFT,
                egui::Key::S,
            )
        });
        let save = ctx.input_mut(|i| i.consume_key(egui::Modifiers::COMMAND, egui::Key::S));
        if save_version || save {
            self.save(ctx, save_version);
        }
        if !ctx.wants_keyboard_input() {
            self.capture.space(ctx);
            self.cards
                .shortcuts(&mut self.project, &mut self.capture, self.playhead_ms, ctx);
            if ctx.input_mut(|i| {
                i.consume_key(
                    egui::Modifiers::COMMAND | egui::Modifiers::SHIFT,
                    egui::Key::Z,
                ) || i.consume_key(egui::Modifiers::COMMAND, egui::Key::Y)
            }) {
                self.history(true);
            } else if ctx.input_mut(|i| i.consume_key(egui::Modifiers::COMMAND, egui::Key::Z)) {
                self.history(false);
            }
            if let Some(clip) = self.selected.filter(|_| {
                ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Delete))
            }) {
                self.edit(Edit::RemoveClip { clip });
            }
        }
    }
    fn show(&mut self, ctx: &egui::Context) {
        if self.project.poll(ctx) {
            self.output.reset();
            self.timeline = timeline::TimelineUi::default();
            self.youtube.project_changed();
            self.name_draft = self.project.studio().name().into();
            self.selected = None;
            self.playhead_ms = 0;
            self.show_removed = false;
        }
        self.output
            .poll(&mut self.project, ctx, self.cards.reduced_motion);
        self.youtube.poll(&mut self.project, &mut self.capture, ctx);
        if let Some(clip) = self.capture.poll(&mut self.project, ctx) {
            self.select(clip);
        }
        self.scrub.tick(
            &self.project,
            self.playhead_ms,
            self.capture.camera_running(),
            self.timeline.draft_source_preview(),
            ctx,
        );
        self.output
            .after_capture(&mut self.project, &mut self.capture, ctx);
        self.ai.poll(&mut self.project, ctx);
        if !self.ai.open && !self.youtube.open {
            self.shortcuts(ctx);
        }
        self.cards
            .tick(&self.project, &self.capture, self.playhead_ms, ctx);
        self.header(ctx);
        self.waveforms.tick(&self.project, ctx);
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
        self.dialogs(ctx);
        self.cards.dialog(
            &mut self.project,
            video_core::OutputSize {
                width: self.capture.settings.mode.width,
                height: self.capture.settings.mode.height,
            },
            ctx,
        );
        self.capture.settings_dialog(&self.project, ctx);
        self.capture.recovery_dialog(&self.project, ctx);
        self.output.banner(&self.project, ctx);
        if let Some((slot, content)) = self.ai.dialog(&mut self.project, ctx) {
            self.cards.edit(slot, Some(content));
        }
        self.youtube.dialog(&mut self.project, &self.capture, ctx);
    }
}
fn timecode(ms: u64) -> String {
    format!(
        "{:02}:{:02}.{:03}",
        ms / 60_000,
        (ms / 1000) % 60,
        ms % 1000
    )
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
impl eframe::App for SidebarVideo {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        if ctx.input(|i| i.viewport().close_requested())
            && !self.allow_close
            && (self.dirty()
                || self.project.busy()
                || self.output.busy()
                || !self.capture.can_leave()
                || self.cards.busy()
                || self.cards.editing()
                || self.ai.open
                || self.ai.busy()
                || self.youtube.open
                || self.youtube.busy()
                || self.timeline.dragging())
        {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            self.request(Intent::Close, ctx);
        }
        self.show(ctx);
        #[cfg(feature = "validation-screenshot")]
        input_probe::sample(
            ctx,
            input_probe::Flags {
                project_present: self.project.store().is_some(),
                project_busy: self.project.busy(),
                project_loading: self.project.loading(),
                output_busy: self.output.busy(),
                sequence_preview_open: self.output.probe_preview_state().0,
                export_pending: self.output.probe_preview_state().1,
                confirm: self.confirm.is_some(),
                version: self.version_summary.is_some(),
                trim: self.trim.is_some(),
                overlay_editing: self.cards.editing(),
                overlay_busy: self.cards.busy(),
                ai_open: self.ai.open,
                ai_busy: self.ai.busy(),
                youtube_open: self.youtube.open,
                youtube_busy: self.youtube.busy(),
                camera_settings: self.capture.show_settings,
                recovery_pending: !self.capture.recoveries.is_empty(),
                capture_busy: self.capture.busy(),
                camera_running: self.capture.camera_running(),
                can_record: self.capture.can_record(),
                taking: self.capture.taking(),
                preview_present: self.capture.texture.is_some(),
                clip_count: self.project.studio().clips().len(),
                capture_error: self.capture.error.is_some(),
                allow_close: self.allow_close,
            },
        );
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
    let mut app = SidebarVideo::new();
    if minimum_preview {
        app.edit(Edit::SetOrientation(Orientation::Vertical));
    }
    #[cfg(feature = "validation-screenshot")]
    if std::env::var_os("SIDEBAR_VIDEO_VALIDATION_TIMELINE").is_some() {
        validation_timeline(&mut app);
    }
    #[cfg(feature = "validation-screenshot")]
    if std::env::var_os("SIDEBAR_VIDEO_VALIDATION_YOUTUBE").is_some() {
        app.youtube.show_window();
    }
    let title = format!("Storyfold · Record studio ({})", env!("CARGO_PKG_VERSION"));
    #[cfg(feature = "validation-screenshot")]
    let title = if input_probe::enabled() {
        format!("{title} · Input check")
    } else {
        title
    };
    eframe::run_native(
        &title,
        eframe::NativeOptions {
            viewport: egui::ViewportBuilder::default()
                .with_app_id("storyfold-record")
                .with_icon(
                    eframe::icon_data::from_png_bytes(include_bytes!(
                        "../../../packaging/record-studio/storyfold-record.png"
                    ))
                    .expect("Valid bundled Storyfold icon"),
                )
                .with_inner_size(size)
                .with_min_inner_size([1160., 800.]),
            ..Default::default()
        },
        Box::new(move |cc| {
            configure(&cc.egui_ctx);
            #[cfg(all(
                feature = "capture-test",
                feature = "validation-screenshot",
                target_os = "linux"
            ))]
            if let Some(folder) = std::env::var_os("STORYFOLD_RECORD_GENERATED_FIXTURE") {
                assert_eq!(std::env::var("STORYFOLD_INPUT_FIXTURE").as_deref(), Ok("1"));
                assert_eq!(std::env::var("STORYFOLD_CAPTURE_TEST").as_deref(), Ok("1"));
                let folder = std::path::PathBuf::from(folder);
                assert!(folder.is_absolute() && !folder.exists());
                app.project = Workspace::test_project(&folder);
                app.name_draft = app.project.studio().name().into();
                app.output.muted = true;
                app.capture.enable_generated(&app.project, &cc.egui_ctx);
            }
            app.project
                .set_folder_dialog(rfd::FileDialog::new().set_parent(cc));
            Ok(Box::new(app))
        }),
    )
}
#[cfg(any(test, feature = "validation-screenshot"))]
fn validation_timeline(app: &mut SidebarVideo) {
    let source = app.project.studio().next_media_id();
    app.project.edit(Edit::AddMedia {
        path: "media/generated-fixture.mp4".into(),
        duration_ms: 20_000,
    });
    for (name, start, end) in [
        ("Opening", 0, 5_000),
        ("Key idea", 5_000, 12_000),
        ("Closing", 12_000, 20_000),
    ] {
        app.project.edit(Edit::AddClip {
            source,
            name: name.into(),
            in_ms: start,
            out_ms: end,
        });
    }
    let clip = app.project.studio().clips()[1].id();
    app.select(clip);
    app.playhead_ms = 8_000;
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn project_menu_exposes_gpu_preference_without_starting_devices() {
        let ctx = egui::Context::default();
        configure(&ctx);
        ctx.enable_accesskit();
        let mut app = SidebarVideo::new();
        let mut draw = |events| {
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
                |ctx| app.header(ctx),
            )
        };
        let initial = draw(vec![]);
        let bounds = initial
            .platform_output
            .accesskit_update
            .unwrap()
            .nodes
            .into_iter()
            .find_map(|(_, n)| (n.label() == Some("Project")).then(|| n.bounds().unwrap()))
            .unwrap();
        let pos = egui::pos2(
            ((bounds.x0 + bounds.x1) / 2.) as f32,
            ((bounds.y0 + bounds.y1) / 2.) as f32,
        );
        let pointer = |pressed| {
            vec![
                egui::Event::PointerMoved(pos),
                egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers: egui::Modifiers::NONE,
                },
            ]
        };
        draw(pointer(true));
        let opened = draw(pointer(false));
        assert!(
            opened
                .platform_output
                .accesskit_update
                .unwrap()
                .nodes
                .iter()
                .any(|(_, n)| n.label() == Some("Prefer NVIDIA GPU encoding")),
            "Project menu must expose the GPU encoding preference"
        );
        assert!(!app.capture.camera_running());
        assert!(!app.capture.busy());
        assert!(!app.output.busy());
    }
    #[test]
    fn both_shapes_expose_accessible_controls_and_keep_record_in_bounds() {
        let ctx = egui::Context::default();
        configure(&ctx);
        ctx.enable_accesskit();
        let mut app = SidebarVideo::new();
        validation_timeline(&mut app);
        for (orientation, size) in [
            (Orientation::Horizontal, [1480., 1000.]),
            (Orientation::Vertical, [1160., 800.]),
        ] {
            app.edit(Edit::SetOrientation(orientation));
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
                .expect("Native accessibility tree");
            let labels: Vec<_> = update
                .nodes
                .iter()
                .filter_map(|(_, node)| node.label())
                .collect();
            for text in [
                "Export MP4",
                "Record. Hold",
                "Empty card",
                "Split at playhead",
                "Playhead",
            ] {
                assert!(
                    labels.iter().any(|label| label.contains(text)),
                    "Missing {text}"
                );
            }
            let bounds = update
                .nodes
                .iter()
                .find(|(_, node)| node.label().is_some_and(|s| s.starts_with("Record. Hold")))
                .unwrap()
                .1
                .bounds()
                .unwrap();
            assert!(
                bounds.y0 >= 112. && bounds.y1 <= f64::from(size[1] - 288.),
                "Record must fit above sequence: {bounds:?}"
            );
            assert!(bounds.x0 >= 0. && bounds.x1 <= f64::from(size[0]));
            for action in ["Split at playhead", "Trim / name…", "Remove"] {
                let b = update
                    .nodes
                    .iter()
                    .find(|(_, node)| node.label() == Some(action))
                    .unwrap()
                    .1
                    .bounds()
                    .unwrap();
                assert!(
                    b.y0 >= f64::from(size[1] - 288.) && b.y1 <= f64::from(size[1] - 10.),
                    "Sequence action must fit in its panel: {action} {b:?}"
                );
            }
            for action in ["Create project…", "Open project…"] {
                let action_bounds = update
                    .nodes
                    .iter()
                    .find(|(_, node)| node.label() == Some(action))
                    .unwrap_or_else(|| panic!("Missing startup action {action}"))
                    .1
                    .bounds()
                    .unwrap();
                assert!(action_bounds.x0 >= 250. && action_bounds.x1 <= f64::from(size[0] - 304.));
                assert!(
                    action_bounds.y0 >= 112. && action_bounds.y1 <= bounds.y0,
                    "Startup action must fit above Record: {action_bounds:?}, Record {bounds:?}"
                );
            }
        }
    }
    #[test]
    fn sequence_actions_use_core_history_and_removed_restoration() {
        let mut app = SidebarVideo::new();
        validation_timeline(&mut app);
        let selected = app.selected.unwrap();
        app.edit(Edit::MoveClip {
            clip: selected,
            to_index: 0,
        });
        assert_eq!(app.project.studio().clips()[0].id(), selected);
        app.playhead_ms = 3_000;
        app.edit(Edit::SplitAtPlayhead {
            sequence_ms: app.playhead_ms,
        });
        assert_eq!(app.project.studio().clips().len(), 4);
        app.edit(Edit::RemoveClip { clip: selected });
        assert_eq!(
            app.project.studio().removed_clips()[0].clip().id(),
            selected
        );
        app.history(false);
        assert!(app.project.studio().removed_clips().is_empty());
        app.history(true);
        app.edit(Edit::RestoreClip {
            clip: selected,
            to_index: 0,
        });
        assert_eq!(app.project.studio().clips()[0].id(), selected);
        assert!(app.project.studio().validate().is_ok());
    }
    #[test]
    fn navigation_guards_unsaved_name_without_discarding_it() {
        let ctx = egui::Context::default();
        let mut app = SidebarVideo::new();
        app.name_draft = "Uncommitted title".into();
        app.request(Intent::Open, &ctx);
        assert!(matches!(app.confirm, Some(Intent::Open)));
        assert!(!app.project.busy());
        assert_eq!(app.name_draft, "Uncommitted title");
        app.confirm = None;
        app.youtube.show_window();
        app.request(Intent::Open, &ctx);
        assert!(app.confirm.is_none());
        assert!(!app.project.busy());
        assert_eq!(app.name_draft, "Uncommitted title");
        assert!(app.project.status.contains("close YouTube"));
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
