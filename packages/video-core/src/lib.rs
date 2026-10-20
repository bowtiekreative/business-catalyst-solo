//! Validated local projects, timeline edits and explicit continuous capture.
//! Construction never opens hardware, starts inference or makes network calls.
pub mod capture;
pub mod export;
pub mod media;
mod model;
mod overlay;
pub mod renderer;
mod storage;
mod timeline;

pub use catalyst_core::Result;
pub use model::*;
pub use overlay::{Highlight, HighlightStyle, OverlayBlock, OverlayContent, OverlayMotion};
pub use storage::{ProjectStore, SaveVersion};
pub use timeline::{Direction, Edit, EditOutcome, Editor, HISTORY_LIMIT, Nudge, TrimEdge};
