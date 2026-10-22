//! Validated local projects, timeline edits and explicit continuous capture.
//! Construction never opens hardware, starts inference or makes network calls.
pub mod ai;
pub mod capture;
pub mod encoding;
pub mod export;
pub mod insert;
pub mod media;
mod model;
mod overlay;
pub mod playback;
pub mod preview;
pub mod renderer;
mod storage;
mod timeline;
pub mod waveform;
pub mod youtube;

pub use catalyst_core::Result;
pub use insert::{
    InsertContent, InsertPlacement, InsertSource, SceneLayout, Viewport, ZoomEasing, ZoomPoint,
    ZoomSpec,
};
pub use model::*;
pub use overlay::{Highlight, HighlightStyle, OverlayBlock, OverlayContent, OverlayMotion};
pub use storage::{ProjectStore, SaveVersion};
pub use timeline::{Direction, Edit, EditOutcome, Editor, HISTORY_LIMIT, Nudge, TrimEdge};
