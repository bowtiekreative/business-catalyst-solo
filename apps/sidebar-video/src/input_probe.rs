//! Opt-in, bounded diagnostics for this application's native input routing.
//! Counts/state flags and one static Project-control rectangle only. Never input
//! contents, pointer positions, project paths or window titles.
use eframe::egui;
use serde_json::json;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

pub(super) fn enabled() -> bool {
    static ENABLED: OnceLock<bool> = OnceLock::new();
    *ENABLED.get_or_init(|| std::env::var("STORYFOLD_RECORD_INPUT_PROBE").as_deref() == Ok("1"))
}

#[derive(Clone, Copy, PartialEq)]
pub(super) struct Flags {
    pub project_present: bool,
    pub project_busy: bool,
    pub project_loading: bool,
    pub output_busy: bool,
    pub confirm: bool,
    pub version: bool,
    pub trim: bool,
    pub overlay_editing: bool,
    pub overlay_busy: bool,
    pub ai_open: bool,
    pub ai_busy: bool,
    pub youtube_open: bool,
    pub youtube_busy: bool,
    pub camera_settings: bool,
    pub recovery_pending: bool,
    pub capture_busy: bool,
    pub camera_running: bool,
    pub allow_close: bool,
}

#[derive(Default)]
struct Counts {
    events: usize,
    buttons: usize,
    presses: usize,
    releases: usize,
    moves: usize,
    raw_moves: usize,
    gone: usize,
}
#[derive(Clone, Copy, PartialEq)]
struct Routing {
    focused: bool,
    viewport_focused: Option<bool>,
    root_viewport: bool,
    viewport_count: usize,
    minimized: Option<bool>,
    pointer_present: bool,
    pointer_down: bool,
    keyboard_focus: bool,
    egui_modal: bool,
    close_requested: bool,
}
#[derive(Clone, Copy, PartialEq)]
struct Menu {
    rect: [f32; 4],
    enabled: bool,
    clicked: bool,
    popup_open: bool,
}
static PROJECT_MENU: Mutex<Option<Menu>> = Mutex::new(None);

pub(super) fn project_menu(response: &egui::Response, popup_open: bool) {
    if !enabled() {
        return;
    }
    if let Ok(mut menu) = PROJECT_MENU.lock() {
        *menu = Some(Menu {
            rect: [
                response.rect.min.x,
                response.rect.min.y,
                response.rect.max.x,
                response.rect.max.y,
            ],
            enabled: response.enabled(),
            clicked: response.clicked(),
            popup_open,
        });
    }
}
#[derive(Default)]
struct Probe {
    lines: usize,
    paint_calls: usize,
    counts: Counts,
    previous: Option<(Flags, Routing)>,
    previous_menu: Option<Menu>,
    last_log: Option<Instant>,
}

pub(super) fn sample(ctx: &egui::Context, flags: Flags) {
    if !enabled() {
        return;
    }
    static PROBE: Mutex<Probe> = Mutex::new(Probe {
        lines: 0,
        paint_calls: 0,
        counts: Counts {
            events: 0,
            buttons: 0,
            presses: 0,
            releases: 0,
            moves: 0,
            raw_moves: 0,
            gone: 0,
        },
        previous: None,
        previous_menu: None,
        last_log: None,
    });
    let Ok(mut probe) = PROBE.lock() else { return };
    if probe.lines >= 100 {
        return;
    }
    let mut routing = ctx.input(|i| {
        probe.counts.events += i.raw.events.len();
        for event in &i.raw.events {
            match event {
                egui::Event::PointerButton { pressed, .. } => {
                    probe.counts.buttons += 1;
                    if *pressed {
                        probe.counts.presses += 1;
                    } else {
                        probe.counts.releases += 1;
                    }
                }
                egui::Event::PointerMoved(_) => probe.counts.moves += 1,
                egui::Event::MouseMoved(_) => probe.counts.raw_moves += 1,
                egui::Event::PointerGone => probe.counts.gone += 1,
                _ => {}
            }
        }
        Routing {
            focused: i.raw.focused,
            viewport_focused: i.viewport().focused,
            root_viewport: i.raw.viewport_id == egui::ViewportId::ROOT,
            viewport_count: i.raw.viewports.len(),
            minimized: i.viewport().minimized,
            pointer_present: i.pointer.hover_pos().is_some(),
            pointer_down: i.pointer.any_down(),
            keyboard_focus: false,
            egui_modal: false,
            close_requested: i.viewport().close_requested(),
        }
    });
    ctx.memory(|memory| {
        routing.keyboard_focus = memory.focused().is_some();
        routing.egui_modal = memory.top_modal_layer().is_some();
    });
    let menu = PROJECT_MENU.lock().ok().and_then(|m| *m);
    probe.paint_calls += 1;
    // Preserve samples for real clicks. Coalesce motion instead of exhausting
    // the 100-line cap while the user moves toward a button. Never force repaint:
    // doing so could hide a repaint/wake-up failure under investigation.
    let changed = probe.previous != Some((flags, routing)) || probe.previous_menu != menu;
    let elapsed = probe
        .last_log
        .is_none_or(|last| last.elapsed() >= Duration::from_millis(300));
    if probe.paint_calls > 4 && !changed && probe.counts.buttons == 0 && !elapsed {
        return;
    }
    probe.lines += 1;
    let mut record = json!({
        "probe":"storyfold-record-input", "sample":probe.lines,
        "sample_cap":100, "frame":ctx.cumulative_frame_nr(),
        "paint_calls":probe.paint_calls, "events":probe.counts.events,
        "pointer_buttons":probe.counts.buttons, "presses":probe.counts.presses,
        "releases":probe.counts.releases, "pointer_moves":probe.counts.moves,
        "raw_mouse_moves":probe.counts.raw_moves, "pointer_gone":probe.counts.gone,
        "pointer_present":routing.pointer_present, "pointer_down":routing.pointer_down,
        "focused":routing.focused, "viewport_focused":routing.viewport_focused,
        "root_viewport":routing.root_viewport, "viewports":routing.viewport_count,
        "minimized":routing.minimized, "keyboard_focus":routing.keyboard_focus,
        "egui_modal":routing.egui_modal, "wants_pointer":ctx.wants_pointer_input(),
        "close_requested":routing.close_requested, "allow_close":flags.allow_close,
        "pixels_per_point":ctx.pixels_per_point(),
        "project_menu":menu.map(|m|json!({"label":"Project","rect":m.rect,"enabled":m.enabled,"clicked":m.clicked,"popup_open":m.popup_open})),
    });
    let flags_record = json!({
        "project_present":flags.project_present, "project_busy":flags.project_busy,
        "project_loading":flags.project_loading, "output_busy":flags.output_busy,
        "confirm":flags.confirm, "version_dialog":flags.version, "trim_dialog":flags.trim,
        "overlay_editing":flags.overlay_editing, "overlay_busy":flags.overlay_busy,
        "ai_open":flags.ai_open, "ai_busy":flags.ai_busy,
        "youtube_open":flags.youtube_open, "youtube_busy":flags.youtube_busy,
        "camera_settings":flags.camera_settings, "recovery_pending":flags.recovery_pending,
        "capture_busy":flags.capture_busy, "camera_running":flags.camera_running,
    });
    if let (Some(record), serde_json::Value::Object(flags)) = (record.as_object_mut(), flags_record)
    {
        record.extend(flags);
    }
    eprintln!("{record}");
    probe.counts = Counts::default();
    probe.previous = Some((flags, routing));
    probe.previous_menu = menu;
    probe.last_log = Some(Instant::now());
}
