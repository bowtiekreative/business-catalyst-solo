//! Explicit local AI and public evidence operations. Call only on workers after
//! a user action; proposal creation never mutates a project or approved slot.
mod evidence;
mod local;
mod proposal;
mod search;
pub use evidence::Evidence;
pub use local::{LocalModel, TokenUsage, usage};
pub use proposal::{HtmlExport, Proposal, export_html};
pub(crate) use search::PublicProxy;
pub use search::{SearchHit, search};

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        Edit, Editor, OverlayBlock, OverlayContent, OverlayKind, ProjectStore, SlotKey,
        media::Cancellation,
        renderer::{ActiveOverlay, OverlayAssets, OverlayRenderer, RenderOptions},
    };
    use std::{fs, io::Cursor};
    fn generated(store: &ProjectStore) -> std::path::PathBuf {
        let mut content = OverlayContent::new(OverlayKind::Quote, "THE LOCAL TEST");
        content.blocks.push(OverlayBlock {
            role: "body".into(),
            text: "Storyfold keeps words exact.".into(),
        });
        let frame = OverlayRenderer::new()
            .unwrap()
            .render_live(
                crate::Orientation::Horizontal.output_size(),
                &[ActiveOverlay {
                    content: &content,
                    elapsed_ms: 1000,
                    duration_ms: 5000,
                }],
                &OverlayAssets::new(),
                RenderOptions {
                    reduced_motion: true,
                },
            )
            .unwrap();
        let image = image::RgbaImage::from_raw(frame.width, frame.height, frame.pixels).unwrap();
        let mut png = Cursor::new(Vec::new());
        image.write_to(&mut png, image::ImageFormat::Png).unwrap();
        let path = store.root().join("buffer/generated-ai-source.png");
        fs::write(&path, png.into_inner()).unwrap();
        path
    }
    #[test]
    #[ignore = "Explicit local OCR/vision/HTML test; STORYFOLD_AI_TEST=1, generated image only"]
    fn generated_local_vision_review_html_and_usage() {
        assert_eq!(std::env::var("STORYFOLD_AI_TEST").as_deref(), Ok("1"));
        let temp = tempfile::tempdir().unwrap();
        let (mut store, studio) =
            ProjectStore::create(temp.path().join("project"), "Local AI test").unwrap();
        let original = fs::read(store.root().join("project.json")).unwrap();
        let token = Cancellation::default();
        let path = generated(&store);
        let evidence = Evidence::from_file(&store, &path, &token).unwrap();
        assert!(evidence.text().contains("Storyfold keeps words exact."));
        let proposal = LocalModel::default()
            .rebuild(&store, &evidence, &token)
            .unwrap();
        proposal.validate(&evidence).unwrap();
        assert_eq!(
            fs::read(store.root().join("project.json")).unwrap(),
            original
        );
        assert_eq!(fs::read_dir(store.root().join("media")).unwrap().count(), 0);
        let mut changed = proposal.clone();
        changed.blocks.push(OverlayBlock {
            role: "body".into(),
            text: "This statement was invented.".into(),
        });
        assert!(changed.validate(&evidence).is_err());
        let tokens = usage(&store).unwrap();
        assert_eq!(tokens.requests, 1);
        assert!(tokens.input > 0 && tokens.output > 0);
        let mut editor = Editor::new(studio).unwrap();
        let slot = SlotKey::try_from(1).unwrap();
        editor
            .apply(Edit::SetSlotContent {
                slot,
                content: proposal.content(),
            })
            .unwrap();
        store.save(editor.studio()).unwrap();
        let result = export_html(&mut store, editor.studio(), slot, &token).unwrap();
        assert!(result.version_error.is_none());
        let html = fs::read_to_string(store.root().join(result.relative_path)).unwrap();
        assert!(html.contains("data:image/png;base64,"));
        assert!(!html.contains("<script"));
        let original_card = evidence
            .approve_original(&store, "Original screenshot", &token)
            .unwrap();
        assert!(original_card.image.is_some());
        println!(
            "Generated local vision/OCR/review/HTML passed; tokens={}+{}",
            tokens.input, tokens.output
        );
    }
    #[test]
    #[ignore = "Explicit public search+isolated Chromium screenshot; STORYFOLD_AI_TEST=1"]
    fn public_search_and_screenshot_are_real_and_cancellable() {
        assert_eq!(std::env::var("STORYFOLD_AI_TEST").as_deref(), Ok("1"));
        let temp = tempfile::tempdir().unwrap();
        let (store, _) =
            ProjectStore::create(temp.path().join("project"), "Public evidence").unwrap();
        let cancel = Cancellation::default();
        let hits = search("Rust programming language official", 4, &cancel).unwrap();
        assert_eq!(hits.len(), 4);
        assert!(hits.iter().all(|h| h.url.starts_with("https://")));
        let evidence = Evidence::capture(&store, "https://example.org/", &cancel).unwrap();
        assert!(
            evidence.text().contains("documentation examples"),
            "Captured OCR: {}",
            evidence.text()
        );
        assert_eq!(evidence.source_url(), "https://example.org/");
        cancel.cancel();
        assert!(Evidence::capture(&store, "https://example.org/", &cancel).is_err());
        println!(
            "Public RSS returned{}realresults; isolated Chromium screenshot1440x900/OCR matched public page",
            hits.len()
        );
    }
}
