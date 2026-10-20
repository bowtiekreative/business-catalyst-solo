use super::Evidence;
use crate::{
    Highlight, OverlayBlock, OverlayContent, OverlayKind, ProjectStore, Result, SlotKey, Studio,
    media::{self, Cancellation},
    renderer::{ActiveOverlay, OverlayAssets, OverlayRenderer, RenderOptions},
};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use catalyst_core::local::PendingFile;
use serde::{Deserialize, Serialize};
use std::io::{Cursor, Write};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Proposal {
    pub summary: String,
    pub kind: OverlayKind,
    pub title: String,
    pub source_url: String,
    pub blocks: Vec<OverlayBlock>,
    pub highlights: Vec<Highlight>,
}
impl Proposal {
    pub fn parse(json: &str, evidence: &Evidence) -> Result<Self> {
        if json.len() > 65536 {
            return Err("AI proposal exceeds its size limit".into());
        }
        let value: Self = serde_json::from_str(json)
            .map_err(|_| "Local model returned an invalid proposal; retry or edit manually")?;
        value.validate(evidence)?;
        Ok(value)
    }
    pub fn validate(&self, evidence: &Evidence) -> Result<()> {
        if self.summary.chars().count() > 800
            || self.blocks.len() > 11
            || self.source_url != evidence.source_url
            || self.kind == OverlayKind::Photo
        {
            return Err(
                "Proposal metadata is invalid; use the original screenshot option for a photo card"
                    .into(),
            );
        }
        if !evidence.text.contains(&self.title)
            || self.title.trim().is_empty()
            || self.blocks.iter().any(|b| !evidence.text.contains(&b.text))
        {
            return Err("Proposed text differs from the screenshot OCR evidence. Edit it to match before approval.".into());
        }
        catalyst_core::check_secrets(&serde_json::to_string(self)?)?;
        self.content().validate()
    }
    pub fn content(&self) -> OverlayContent {
        let mut content = OverlayContent::new(self.kind, &self.title);
        content.blocks = self.blocks.clone();
        content.highlights = self.highlights.clone();
        if !self.source_url.is_empty() {
            content.blocks.push(OverlayBlock {
                role: "source".into(),
                text: self.source_url.clone(),
            });
        }
        content
    }
    pub fn schema_for(evidence: &Evidence) -> Result<serde_json::Value> {
        if evidence.text.len() > 8000 {
            return Err(
                "Crop the screenshot to the passage you want to rebuild (up to 8000 OCR characters)"
                    .into(),
            );
        }
        let mut titles = Vec::new();
        let mut blocks = Vec::new();
        for text in evidence
            .text
            .lines()
            .chain(evidence.text.split("\n\n"))
            .map(str::trim)
            .filter(|s| !s.is_empty())
        {
            if text.chars().count() <= 200 && !titles.iter().any(|s| s == text) {
                titles.push(text.to_owned());
            }
            if text.chars().count() <= 2000 && !blocks.iter().any(|s| s == text) {
                blocks.push(text.to_owned());
            }
        }
        if titles.is_empty() || blocks.is_empty() {
            return Err(
                "No short readable passage was found; crop the screenshot or keep the original"
                    .into(),
            );
        }
        let mut schema = Self::schema();
        schema["properties"]["title"]["enum"] = serde_json::to_value(titles)?;
        schema["properties"]["blocks"]["items"]["properties"]["text"]["enum"] =
            serde_json::to_value(blocks)?;
        schema["properties"]["source_url"]["const"] = evidence.source_url.clone().into();
        Ok(schema)
    }
    pub fn schema() -> serde_json::Value {
        use serde_json::json;
        json!({"type":"object","additionalProperties":false,"required":["summary","kind","title","source_url","blocks","highlights"],"properties":{
            "summary":{"type":"string"},"kind":{"type":"string","enum":["lower_third","stat","screenshot_card","numbered_list","quote"]},"title":{"type":"string"},"source_url":{"type":"string"},
            "blocks":{"type":"array","maxItems":11,"items":{"type":"object","additionalProperties":false,"required":["role","text"],"properties":{"role":{"type":"string","enum":["heading","body","source","label"]},"text":{"type":"string"}}}},
            "highlights":{"type":"array","maxItems":32,"items":{"type":"object","additionalProperties":false,"required":["block","start","end"],"properties":{"block":{"type":"integer","minimum":0},"start":{"type":"integer","minimum":0},"end":{"type":"integer","minimum":0}}}}
        }})
    }
}
#[derive(Debug)]
pub struct HtmlExport {
    pub project_saved: bool,
    pub relative_path: String,
    pub version_error: Option<String>,
}
/// Only an already approved slot may export. HTML has no scripts/external assets;
/// its visible pixels are rendered by the shared renderer and its text is escaped.
pub fn export_html(
    store: &mut ProjectStore,
    studio: &Studio,
    slot: SlotKey,
    cancel: &Cancellation,
) -> Result<HtmlExport> {
    cancel.check()?;
    studio.validate()?;
    let content = studio
        .slots()
        .iter()
        .find(|s| s.key() == slot)
        .and_then(|s| s.content())
        .ok_or("Approve this card before exporting HTML")?;
    if content.insert.is_some() {
        return Err("Export media inserts as MP4; HTML exports support approved cards".into());
    }
    content.validate()?;
    let mut assets = OverlayAssets::new();
    assets.load_content(store, content)?;
    let frame = OverlayRenderer::new()?.render_live(
        studio.orientation().output_size(),
        &[ActiveOverlay {
            content,
            elapsed_ms: 1000,
            duration_ms: 5000,
        }],
        &assets,
        RenderOptions {
            reduced_motion: true,
        },
    )?;
    let image = image::RgbaImage::from_raw(frame.width, frame.height, frame.pixels)
        .ok_or("Invalid rendered card")?;
    let mut png = Cursor::new(Vec::new());
    image.write_to(&mut png, image::ImageFormat::Png)?;
    let text = content
        .blocks
        .iter()
        .map(|b| format!("<p>{}</p>", escape(&b.text)))
        .collect::<String>();
    let html = format!(
        "<!doctype html><html lang=\"en\"><meta charset=\"utf-8\"><meta name=\"viewport\" content=\"width=device-width,initial-scale=1\"><title>{}</title><style>body{{margin:0;background:#f7f6f2}}img{{display:block;width:100%;height:auto}}article{{position:absolute;width:1px;height:1px;overflow:hidden;clip-path:inset(50%)}}</style><img alt=\"{}\" src=\"data:image/png;base64,{}\"><article><h1>{}</h1>{text}</article></html>",
        escape(&content.title),
        escape(&content.title),
        STANDARD.encode(png.into_inner()),
        escape(&content.title)
    );
    cancel.check()?;
    let relative_path = format!("exports/card-{}-{}.html", slot.number(), media::unique_id());
    let mut pending = PendingFile::prepare(store.root(), &relative_path)?;
    pending.file_mut().write_all(html.as_bytes())?;
    cancel.check()?;
    pending.publish()?;
    let (project_saved, version_error) = match store.save(studio) {
        Ok(()) => (
            true,
            store
                .save_version(studio, "Export approved overlay HTML")
                .err()
                .map(|e| e.to_string()),
        ),
        Err(e) => (
            false,
            Some(format!(
                "HTML saved, but the project could not be saved: {e}"
            )),
        ),
    };
    Ok(HtmlExport {
        project_saved,
        relative_path,
        version_error,
    })
}
fn escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn html_escape_never_leaves_markup_or_attribute_breakouts() {
        assert_eq!(escape("<&\"'>"), "&lt;&amp;&quot;&#39;&gt;");
    }
    #[test]
    fn strict_nested_and_root_keys_reject_unknown_fields() {
        let base = serde_json::json!({"summary":"","kind":"quote","title":"A","source_url":"","blocks":[{"role":"body","text":"A"}],"highlights":[]});
        assert!(serde_json::from_value::<Proposal>(base.clone()).is_ok());
        let mut changed = base.clone();
        changed["command"] = true.into();
        assert!(serde_json::from_value::<Proposal>(changed).is_err());
        let mut changed = base;
        changed["blocks"][0]["command"] = true.into();
        assert!(serde_json::from_value::<Proposal>(changed).is_err());
    }
}
