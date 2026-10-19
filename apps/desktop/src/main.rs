use catalyst_core::{Project, Report, Review, SECTION_CATALOG, SavedChange};
use eframe::egui::{self, Color32, RichText};
use std::sync::mpsc::{self, Receiver, TryRecvError};

#[derive(Clone, Copy, PartialEq)]
enum View {
    Home,
    Pages,
    Source,
    History,
    Activity,
    Catalog,
}

struct Work {
    project: Project,
    report: Report,
    history: Vec<SavedChange>,
    saved: Option<SavedChange>,
}

struct Catalyst {
    folder: String,
    project: Option<Project>,
    report: Option<Report>,
    history: Vec<SavedChange>,
    activity: Vec<String>,
    view: View,
    path: Option<String>,
    original: String,
    source: String,
    summary: String,
    review: Option<Review>,
    worker: Option<Receiver<Result<Work, String>>>,
    error: Option<String>,
    catalog: serde_json::Value,
    confirm_close: bool,
    allow_close: bool,
}

impl Catalyst {
    fn new(folder: String) -> Self {
        let mut app = Self {
            folder,
            project: None,
            report: None,
            history: Vec::new(),
            activity: Vec::new(),
            view: View::Home,
            path: None,
            original: String::new(),
            source: String::new(),
            summary: "Update page content".into(),
            review: None,
            worker: None,
            error: None,
            catalog: serde_json::from_str(SECTION_CATALOG).expect("Bundled catalog is valid JSON"),
            confirm_close: false,
            allow_close: false,
        };
        if !app.folder.is_empty() {
            app.open();
        }
        app
    }

    fn dirty(&self) -> bool {
        self.path.is_some() && self.source != self.original
    }
    fn busy(&self) -> bool {
        self.worker.is_some()
    }

    fn open(&mut self) {
        if self.dirty() || self.busy() {
            return;
        }
        let folder = self.folder.clone();
        let (send, recv) = mpsc::channel();
        self.worker = Some(recv);
        self.error = None;
        std::thread::spawn(move || {
            let work = (|| -> catalyst_core::Result<Work> {
                let project = Project::open(folder)?;
                let report = project.scan()?;
                let history = project.history()?;
                Ok(Work {
                    project,
                    report,
                    history,
                    saved: None,
                })
            })()
            .map_err(|e| e.to_string());
            let _ = send.send(work);
        });
    }

    fn save(&mut self, review: Review) {
        let Some(project) = self.project.clone() else {
            return;
        };
        let (send, recv) = mpsc::channel();
        self.worker = Some(recv);
        self.review = None;
        self.error = None;
        std::thread::spawn(move || {
            let work = (|| -> catalyst_core::Result<Work> {
                let saved = project.apply(&review)?;
                let report = project.scan()?;
                let history = project.history()?;
                Ok(Work {
                    project,
                    report,
                    history,
                    saved: Some(saved),
                })
            })()
            .map_err(|e| e.to_string());
            let _ = send.send(work);
        });
    }

    fn poll(&mut self) {
        let Some(worker) = &self.worker else {
            return;
        };
        match worker.try_recv() {
            Ok(result) => {
                self.worker = None;
                match result {
                    Ok(work) => {
                        if let Some(saved) = work.saved {
                            self.activity
                                .push(format!("Saved {} · {}", saved.review.path, saved.commit));
                            self.path = Some(saved.review.path);
                            self.original = saved.review.after;
                            self.source = self.original.clone();
                        } else {
                            self.activity
                                .push(format!("Scanned {} pages", work.report.site.pages.len()));
                            self.path = None;
                            self.original.clear();
                            self.source.clear();
                        }
                        self.project = Some(work.project);
                        self.report = Some(work.report);
                        self.history = work.history;
                    }
                    Err(error) => {
                        self.activity.push(error.clone());
                        self.error = Some(error);
                    }
                }
            }
            Err(TryRecvError::Empty) => {}
            Err(TryRecvError::Disconnected) => {
                self.worker = None;
                self.error = Some("The background operation stopped unexpectedly. Reopen the folder to inspect its current state.".into());
            }
        }
    }

    fn load(&mut self, path: &str) {
        if self.dirty() || self.busy() {
            return;
        }
        if let Some(project) = &self.project {
            match project.read_source(path) {
                Ok(source) => {
                    self.path = Some(path.into());
                    self.original = source.clone();
                    self.source = source;
                    self.view = View::Source;
                    self.error = None;
                }
                Err(error) => self.error = Some(error.to_string()),
            }
        }
    }

    fn show(&mut self, ctx: &egui::Context) {
        self.poll();
        egui::TopBottomPanel::top("header").show(ctx, |ui| {
            ui.add_space(10.0);
            ui.horizontal(|ui| {
                ui.heading("CATALYST / SOLO");
                ui.label("Rust starter");
                if self.busy() {
                    ui.spinner();
                    ui.label("Working…");
                }
            });
            ui.add_space(8.0);
        });
        egui::SidePanel::left("navigation")
            .exact_width(196.0)
            .show(ctx, |ui| {
                ui.add_space(18.0);
                ui.label(RichText::new("YOUR WEBSITE").color(Color32::from_rgb(222, 188, 119)));
                ui.add_space(12.0);
                for (view, label) in [
                    (View::Home, "Overview"),
                    (View::Pages, "Pages"),
                    (View::Source, "Source editor"),
                    (View::History, "History"),
                    (View::Activity, "Activity"),
                    (View::Catalog, "Section catalog"),
                ] {
                    if ui
                        .add_sized(
                            [174.0, 36.0],
                            egui::Button::new(label).selected(self.view == view),
                        )
                        .clicked()
                    {
                        self.view = view;
                    }
                }
                ui.add_space(24.0);
                ui.label("Changes stay in review until you save them.");
                ui.add_space(12.0);
                ui.label("AI helpers and publishing are on the roadmap.");
            });
        egui::TopBottomPanel::bottom("status").show(ctx, |ui| {
            ui.horizontal(|ui| {
                if self.dirty() {
                    ui.label(
                        RichText::new("Unsaved draft").color(Color32::from_rgb(255, 212, 135)),
                    );
                    if ui
                        .add_enabled(
                            !self.busy() && self.review.is_none(),
                            egui::Button::new("Discard draft"),
                        )
                        .clicked()
                    {
                        self.source = self.original.clone();
                    }
                } else {
                    ui.label("No unsaved changes");
                }
                ui.separator();
                ui.label("Local files · Local Git · No automatic push");
            });
        });
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.add_space(10.0);
            if let Some(error) = &self.error {
                ui.colored_label(Color32::from_rgb(255, 212, 135), error);
                ui.add_space(10.0);
            }
            match self.view {
                View::Home => self.home(ui),
                View::Pages => self.pages(ui),
                View::Source => self.editor(ui),
                View::History => self.history_view(ui),
                View::Activity => {
                    ui.heading("Activity");
                    egui::ScrollArea::vertical().show(ui, |ui| {
                        for event in self.activity.iter().rev() {
                            ui.label(event);
                            ui.separator();
                        }
                    });
                }
                View::Catalog => self.catalog_view(ui),
            }
        });
        self.review_window(ctx);
        if self.confirm_close {
            egui::Window::new("Close Catalyst?")
                .collapsible(false)
                .resizable(false)
                .show(ctx, |ui| {
                    ui.label(if self.busy() {
                        "A save or scan is running. Wait for it to finish before closing."
                    } else {
                        "Your unsaved draft will be discarded."
                    });
                    if ui.button("Keep working").clicked() {
                        self.confirm_close = false;
                    }
                    if ui
                        .add_enabled(!self.busy(), egui::Button::new("Discard draft and quit"))
                        .clicked()
                    {
                        self.allow_close = true;
                        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                    }
                });
        }
        if self.busy() {
            ctx.request_repaint_after(std::time::Duration::from_millis(50));
        }
    }

    fn home(&mut self, ui: &mut egui::Ui) {
        ui.heading("One website. A clear view.");
        ui.label("Open a local website folder to inspect its pages and review source changes.");
        ui.add_space(18.0);
        ui.label("Website folder");
        let can_open = !self.dirty() && !self.busy() && self.review.is_none();
        ui.horizontal(|ui| {
            ui.add_enabled(
                can_open,
                egui::TextEdit::singleline(&mut self.folder)
                    .desired_width(500.0)
                    .hint_text("Full path to your website"),
            );
            if ui
                .add_enabled(
                    can_open && !self.folder.trim().is_empty(),
                    egui::Button::new("Open / rescan"),
                )
                .clicked()
            {
                self.open();
            }
        });
        ui.add_space(18.0);
        if let Some(report) = &self.report {
            ui.heading(&report.site.name);
            ui.label(format!(
                "{} · {} pages · {} files",
                report.site.stack,
                report.site.pages.len(),
                report.files.len()
            ));
            if let Some(project) = &self.project {
                ui.label(project.root().display().to_string());
            }
            ui.add_space(18.0);
            ui.heading("Scan results");
            ui.label("Source checks only. Rendered accessibility, dynamic links and production readiness still need review.");
            egui::ScrollArea::vertical().show(ui, |ui| {
                if report.issues.is_empty() {
                    ui.label("No issues found by the starter checks.");
                }
                for issue in &report.issues {
                    ui.group(|ui| {
                        ui.strong(format!(
                            "{} · {}",
                            issue.severity.to_uppercase(),
                            issue.page
                        ));
                        ui.label(&issue.message);
                    });
                }
            });
        } else {
            ui.label("Try a disposable sample from your terminal:");
            ui.code("cargo run -p catalyst-cli -- demo ../catalyst-demo");
            ui.label("Then enter the sample's full path above.");
        }
    }

    fn pages(&mut self, ui: &mut egui::Ui) {
        ui.heading("Pages");
        ui.label(
            "Custom sections stay in their original source. Opening a file does not change it.",
        );
        let mut selected = None;
        if let Some(report) = &self.report {
            egui::ScrollArea::vertical().show(ui, |ui| {
                for page in &report.site.pages {
                    ui.group(|ui| {
                        ui.heading(&page.name);
                        ui.label(format!("{} · {} sections", page.route, page.secs.len()));
                        ui.label(format!("Title: {}", page.title));
                        ui.label(format!(
                            "Description: {}",
                            if page.desc.is_empty() {
                                "Missing"
                            } else {
                                &page.desc
                            }
                        ));
                        if let Some(file) = &page.file
                            && ui
                                .add_enabled(
                                    !self.dirty() && !self.busy() && self.review.is_none(),
                                    egui::Button::new("Open source"),
                                )
                                .clicked()
                        {
                            selected = Some(file.clone());
                        }
                    });
                }
            });
        } else {
            ui.label("Open a website folder from Overview first.");
        }
        if let Some(path) = selected {
            self.load(&path);
        }
    }

    fn editor(&mut self, ui: &mut egui::Ui) {
        ui.heading("Source editor");
        let Some(path) = self.path.clone() else {
            ui.label("Choose Open source on a page first.");
            return;
        };
        ui.label(&path);
        ui.label("Edits are held in memory. Review before saving one local Git commit.");
        ui.add_space(8.0);
        ui.label("Change description");
        ui.add_enabled(
            !self.busy() && self.review.is_none(),
            egui::TextEdit::singleline(&mut self.summary).desired_width(f32::INFINITY),
        );
        if ui
            .add_enabled(
                self.dirty() && !self.busy() && self.review.is_none(),
                egui::Button::new("Review changes"),
            )
            .clicked()
            && let Some(project) = &self.project
        {
            match project.review(&path, self.source.clone(), self.summary.clone()) {
                    Ok(review) if review.before == self.original => { self.review = Some(review); self.error = None; }
                    Ok(_) => self.error = Some("This file changed outside Catalyst. Keep a copy of your draft, discard it, then reopen the file.".into()),
                    Err(error) => self.error = Some(error.to_string()),
                }
        }
        ui.add_space(10.0);
        let enabled = !self.busy() && self.review.is_none();
        egui::ScrollArea::both().show(ui, |ui| {
            ui.add_enabled(
                enabled,
                egui::TextEdit::multiline(&mut self.source)
                    .code_editor()
                    .desired_width(f32::INFINITY)
                    .desired_rows(30),
            );
        });
    }

    fn history_view(&mut self, ui: &mut egui::Ui) {
        ui.heading("History");
        ui.label("Catalyst saves are local commits. Undo creates a new reviewed commit.");
        let mut undo = None;
        egui::ScrollArea::vertical().show(ui, |ui| {
            if self.history.is_empty() {
                ui.label("Your first save will appear here.");
            }
            for saved in &self.history {
                ui.group(|ui| {
                    ui.strong(&saved.review.summary);
                    ui.label(&saved.review.path);
                    ui.monospace(&saved.commit);
                    if ui
                        .add_enabled(
                            !self.dirty() && !self.busy() && self.review.is_none(),
                            egui::Button::new("Review undo"),
                        )
                        .clicked()
                    {
                        undo = Some(saved.clone());
                    }
                });
            }
        });
        if let (Some(saved), Some(project)) = (undo, &self.project) {
            match project.review_undo(&saved) {
                Ok(review) => self.review = Some(review),
                Err(error) => self.error = Some(error.to_string()),
            }
        }
    }

    fn catalog_view(&self, ui: &mut egui::Ui) {
        ui.heading("LAKA section catalog");
        ui.label("Original reference patterns. Sample copy is placeholder; replace and verify it before publishing.");
        ui.label("This starter browses the catalog. Inserting or rendering patterns needs a stack adapter.");
        egui::ScrollArea::vertical().show(ui, |ui| {
            if let Some(families) = self.catalog["families"].as_array() {
                for family in families {
                    ui.collapsing(
                        format!(
                            "{}  {}",
                            family["n"].as_str().unwrap_or(""),
                            family["t"].as_str().unwrap_or("")
                        ),
                        |ui| {
                            if let Some(items) = family["items"].as_array() {
                                for item in items {
                                    ui.strong(format!(
                                        "{} · {}",
                                        item["code"].as_str().unwrap_or(""),
                                        item["label"].as_str().unwrap_or("")
                                    ));
                                    ui.label(item["formula"].as_str().unwrap_or(""));
                                    ui.separator();
                                }
                            }
                        },
                    );
                }
            }
        });
    }

    fn review_window(&mut self, ctx: &egui::Context) {
        let Some(review) = self.review.clone() else {
            return;
        };
        let mut open = true;
        let mut apply = false;
        let mut cancel = false;
        egui::Window::new("Review changes").open(&mut open).collapsible(false).default_width(820.0).show(ctx, |ui| {
            ui.heading(&review.summary);
            ui.label(format!("File: {}", review.path));
            ui.label("Saving requires a clean repository and .catalyst/private/ in your committed .gitignore.");
            ui.columns(2, |columns| {
                columns[0].strong("Before");
                egui::ScrollArea::both().id_salt("before").max_height(360.0).show(&mut columns[0], |ui| { ui.monospace(&review.before); });
                columns[1].strong("After");
                egui::ScrollArea::both().id_salt("after").max_height(360.0).show(&mut columns[1], |ui| { ui.monospace(&review.after); });
            });
            ui.horizontal(|ui| {
                apply = ui.button("Save local Git commit").clicked();
                cancel = ui.button("Keep editing").clicked();
            });
        });
        if apply {
            self.save(review);
        } else if !open || cancel {
            self.review = None;
        }
    }
}

impl eframe::App for Catalyst {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        if !self.allow_close
            && ctx.input(|i| i.viewport().close_requested())
            && (self.dirty() || self.busy() || self.review.is_some())
        {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            self.confirm_close = true;
        }
        self.show(ctx);
    }
}

fn main() -> eframe::Result {
    let folder = std::env::args().nth(1).unwrap_or_default();
    eframe::run_native(
        "Catalyst Solo",
        eframe::NativeOptions {
            viewport: egui::ViewportBuilder::default()
                .with_inner_size([1180.0, 800.0])
                .with_min_inner_size([800.0, 560.0]),
            ..Default::default()
        },
        Box::new(move |cc| {
            cc.egui_ctx.set_visuals(egui::Visuals::dark());
            cc.egui_ctx.style_mut(|style| {
                style.visuals.override_text_color = Some(Color32::from_rgb(240, 242, 248));
                style.spacing.item_spacing = egui::vec2(10.0, 10.0);
                style.spacing.button_padding = egui::vec2(14.0, 9.0);
            });
            Ok(Box::new(Catalyst::new(folder)))
        }),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_view_draws_without_a_window() {
        let ctx = egui::Context::default();
        let mut app = Catalyst::new(String::new());
        for view in [
            View::Home,
            View::Pages,
            View::Source,
            View::History,
            View::Activity,
            View::Catalog,
        ] {
            app.view = view;
            let _ = ctx.run(egui::RawInput::default(), |ctx| app.show(ctx));
        }
    }
}
