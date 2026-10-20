//! Approved overlay content is copied into immutable cue definitions.
use crate::{OverlayKind, Result};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OverlayBlock {
    pub role: String,
    pub text: String,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Highlight {
    pub block: usize,
    /// Unicode scalar offsets into the block text, end exclusive.
    pub start: usize,
    pub end: usize,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OverlayMotion {
    #[default]
    FadeUp,
    Still,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HighlightStyle {
    #[default]
    MarkerSweep,
    UnderlineDraw,
    ZoomToLine,
    FadeRest,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OverlayContent {
    pub kind: OverlayKind,
    pub title: String,
    pub blocks: Vec<OverlayBlock>,
    pub highlights: Vec<Highlight>,
    /// A project-owned image under media/, never a URL or arbitrary file path.
    pub image: Option<String>,
    #[serde(default)]
    pub motion: OverlayMotion,
    #[serde(default)]
    pub highlight_style: HighlightStyle,
}
impl OverlayContent {
    pub fn new(kind: OverlayKind, title: impl Into<String>) -> Self {
        Self {
            kind,
            title: title.into(),
            blocks: Vec::new(),
            highlights: Vec::new(),
            image: None,
            motion: OverlayMotion::default(),
            highlight_style: HighlightStyle::default(),
        }
    }
    pub fn validate(&self) -> Result<()> {
        if self.title.trim().is_empty()
            || self.title.chars().count() > 200
            || self.blocks.len() > 12
            || self.highlights.len() > 32
        {
            return Err("Overlay title or content exceeds its limits".into());
        }
        let mut count = self.title.chars().count();
        for block in &self.blocks {
            if !matches!(block.role.as_str(), "heading" | "body" | "source" | "label")
                || block.text.trim().is_empty()
                || block.text.chars().count() > 2000
            {
                return Err("Invalid overlay text block".into());
            }
            count += block.text.chars().count();
        }
        if count > 8000 {
            return Err("Overlay text is too large".into());
        }
        for highlight in &self.highlights {
            let block = self
                .blocks
                .get(highlight.block)
                .ok_or("Highlight block does not exist")?;
            if highlight.start >= highlight.end || highlight.end > block.text.chars().count() {
                return Err("Highlight range must stay within its text block".into());
            }
        }
        if let Some(path) = &self.image {
            crate::model::validate_media_path(path)?;
        }
        if self.kind == OverlayKind::Photo && self.image.is_none() {
            return Err("Choose a project image for this photo overlay".into());
        }
        for text in
            std::iter::once(self.title.as_str()).chain(self.blocks.iter().map(|b| b.text.as_str()))
        {
            if text
                .chars()
                .any(|c| c.is_control() && !matches!(c, '\n' | '\t'))
            {
                return Err("Overlay text contains unsupported control characters".into());
            }
            catalyst_core::check_secrets(text)?;
        }
        Ok(())
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn approved_snapshot_validates_unicode_ranges_paths_and_credential_text() {
        let mut content = OverlayContent {
            kind: OverlayKind::Quote,
            title: "A quote".into(),
            blocks: vec![OverlayBlock {
                role: "body".into(),
                text: "Café ☕".into(),
            }],
            highlights: vec![Highlight {
                block: 0,
                start: 0,
                end: 6,
            }],
            image: None,
            motion: OverlayMotion::default(),
            highlight_style: HighlightStyle::default(),
        };
        assert!(content.validate().is_ok());
        content.highlights[0].end = 7;
        assert!(content.validate().is_err());
        content.highlights.clear();
        content.image = Some("../outside.png".into());
        assert!(content.validate().is_err());
        content.image = None;
        content.title = "api_key=sk-ant-very-private-key-value-do-not-save".into();
        assert!(content.validate().is_err());
    }
}
