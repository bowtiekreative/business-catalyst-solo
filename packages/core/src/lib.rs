mod changes;
mod demo;
pub mod local;
pub mod model;
mod project;
mod scan;

pub use changes::{Review, SavedChange};
pub use demo::create_demo;
pub use project::{Project, Result, check_secrets};
pub use scan::{Issue, Report};

pub const SECTION_CATALOG: &str = include_str!("../../../handoff/prototype/laka-sections.json");
