//! Validated local video projects and timeline edits. No device, inference or
//! network operations are implemented in this core foundation.
mod model;
mod storage;
mod timeline;

pub use catalyst_core::Result;
pub use model::*;
pub use storage::{ProjectStore, SaveVersion};
pub use timeline::{Direction, Edit, EditOutcome, Editor, HISTORY_LIMIT, Nudge, TrimEdge};
