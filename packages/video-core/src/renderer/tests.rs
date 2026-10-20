use super::*;
use crate::{Edit, EditOutcome, Editor, Highlight, OverlayBlock, SlotKey};

fn block(role: &str, text: &str) -> OverlayBlock {
    OverlayBlock {
        role: role.into(),
        text: text.into(),
    }
}
fn content(kind: OverlayKind) -> OverlayContent {
    let (title, blocks) = match kind {
        OverlayKind::LowerThird => (
            "Ryan Perez",
            vec![
                block("body", "Documentary filmmaker"),
                block("source", "Bow Tie Kreative"),
            ],
        ),
        OverlayKind::Stat => (
            "84%",
            vec![
                block("body", "A clear statistic with its context."),
                block("source", "Generated example · 2026"),
            ],
        ),
        OverlayKind::ScreenshotCard => (
            "A clearer story",
            vec![
                block(
                    "body",
                    "Keep the original words. Highlight the line that matters.",
                ),
                block("source", "Generated design fixture"),
            ],
        ),
        OverlayKind::NumberedList => (
            "Three useful steps",
            vec![
                block("body", "Record the thought."),
                block("body", "Keep the best take."),
                block("body", "Share the story."),
            ],
        ),
        OverlayKind::Photo => (
            "A place to begin",
            vec![block("body", "Generated image — no private photograph.")],
        ),
        OverlayKind::Quote => (
            "On storytelling",
            vec![
                block("body", "The best stories make room for curiosity."),
                block("source", "Generated quotation fixture"),
            ],
        ),
    };
    let mut value = OverlayContent::new(kind, title);
    value.blocks = blocks;
    if matches!(kind, OverlayKind::Photo | OverlayKind::ScreenshotCard) {
        value.image = Some("media/generated.png".into());
    }
    value.highlights = vec![Highlight {
        block: 0,
        start: 0,
        end: value.blocks[0].text.chars().count().min(18),
    }];
    value
}
fn project_image() -> (tempfile::TempDir, ProjectStore) {
    let temp = tempfile::tempdir().unwrap();
    let (store, _) = ProjectStore::create(temp.path().join("project"), "Renderer tests").unwrap();
    image::RgbaImage::from_fn(640, 360, |x, y| {
        image::Rgba(if x < 320 {
            [120, (y % 180) as u8, 181, 255]
        } else {
            [24, 116, 132, 255]
        })
    })
    .save(store.root().join("media/generated.png"))
    .unwrap();
    (temp, store)
}
fn frame(
    renderer: &mut OverlayRenderer,
    content: &OverlayContent,
    shape: Orientation,
    ms: u64,
    assets: &OverlayAssets,
    reduce: bool,
) -> RgbaFrame {
    renderer
        .render_live(
            shape.output_size(),
            &[ActiveOverlay {
                content,
                elapsed_ms: ms,
                duration_ms: 5000,
            }],
            assets,
            RenderOptions {
                reduced_motion: reduce,
            },
        )
        .unwrap()
}
fn opaque_bounds(frame: &RgbaFrame) -> (u32, u32, u32, u32) {
    let (mut min_x, mut min_y, mut max_x, mut max_y) = (frame.width, frame.height, 0, 0);
    for (index, _) in frame
        .pixels
        .as_chunks::<4>()
        .0
        .iter()
        .enumerate()
        .filter(|(_, p)| p[3] > 0)
    {
        let (x, y) = (index as u32 % frame.width, index as u32 / frame.width);
        min_x = min_x.min(x);
        min_y = min_y.min(y);
        max_x = max_x.max(x);
        max_y = max_y.max(y);
    }
    (min_x, min_y, max_x, max_y)
}
#[test]
fn six_types_both_shapes_keep_every_text_line_readable_and_inside_safe_bounds() {
    let (_temp, store) = project_image();
    let mut renderer = OverlayRenderer::new().unwrap();
    let mut assets = OverlayAssets::new();
    let kinds = [
        OverlayKind::LowerThird,
        OverlayKind::Stat,
        OverlayKind::ScreenshotCard,
        OverlayKind::NumberedList,
        OverlayKind::Photo,
        OverlayKind::Quote,
    ];
    let proof = (std::env::var("STORYFOLD_RENDERER_PROOF").as_deref() == Ok("1")).then(|| {
        tempfile::Builder::new()
            .prefix("storyfold-overlay-proof-")
            .tempdir()
            .unwrap()
            .keep()
    });
    let mut gallery = image::RgbaImage::from_pixel(1080, 1440, image::Rgba([237, 239, 244, 255]));
    for (index, kind) in kinds.into_iter().enumerate() {
        let content = content(kind);
        assets.load_content(&store, &content).unwrap();
        for (shape_index, shape) in [Orientation::Horizontal, Orientation::Vertical]
            .into_iter()
            .enumerate()
        {
            let result = frame(&mut renderer, &content, shape, 1800, &assets, false);
            let (min_x, min_y, max_x, max_y) = opaque_bounds(&result);
            assert!(
                min_x >= 50
                    && min_y >= 50
                    && max_x < result.width - 50
                    && max_y < result.height - 50,
                "{kind:?} {shape:?}"
            );
            let image = assets.image(&content).unwrap();
            let layout = renderer
                .layout(shape.output_size(), &content, image)
                .unwrap();
            assert_eq!(layout.texts.len(), 1 + content.blocks.len());
            for item in &layout.texts {
                assert!(item.layout.buffer.metrics().font_size >= 26.);
                assert!(item.x >= layout.card.x() && item.y >= layout.card.y());
                assert!(item.y + item.layout.height <= layout.card.bottom() - 20.);
            }
            let raw =
                image::RgbaImage::from_raw(result.width, result.height, result.pixels).unwrap();
            if let Some(dir) = &proof {
                raw.save(dir.join(format!("{index}-{shape:?}.png")))
                    .unwrap();
            }
            let thumb = image::DynamicImage::ImageRgba8(raw)
                .thumbnail(540, 240)
                .to_rgba8();
            image::imageops::overlay(
                &mut gallery,
                &thumb,
                (shape_index * 540) as i64,
                (index * 240) as i64,
            );
        }
    }
    // Explicit developer-only proof artifact, never normal application behavior.
    if let Some(dir) = proof {
        gallery.save(dir.join("six-types-both-shapes.png")).unwrap();
        println!("Generated renderer proof: {}", dir.display());
    }
}
#[test]
fn unicode_scalar_highlights_map_across_wraps_and_explicit_lines_without_byte_drift() {
    let mut renderer = OverlayRenderer::new().unwrap();
    let text = "Café naïve words wrap across the narrow card.\nEncore déjà vu.";
    let end = text.chars().count() - 2;
    let layout = renderer.shape(text, 220., 32., false, &[(3, end)]).unwrap();
    assert!(layout.regions.len() >= 4);
    assert!(layout.regions[0].x() > 0.);
    for region in layout.regions {
        assert!(region.x() >= 0. && region.right() <= 220.5 && region.bottom() <= layout.height);
    }
    let newline = renderer.shape("A line\n", 300., 32., false, &[]).unwrap();
    assert!(newline.height >= 32.);
    assert!(
        !renderer
            .shape("Cafe\u{301}", 300., 32., false, &[(4, 5)])
            .unwrap()
            .regions
            .is_empty()
    );
    assert!(
        !renderer
            .shape("office", 300., 32., false, &[(2, 3)])
            .unwrap()
            .regions
            .is_empty()
    );
    let mut content = OverlayContent::new(OverlayKind::Quote, "Unicode");
    content.blocks = vec![block("body", "Café naïve déjà vu.")];
    content.highlights = vec![Highlight {
        block: 0,
        start: 5,
        end: 10,
    }];
    assert!(content.validate().is_ok());
    content.highlights[0].end = 99;
    assert!(content.validate().is_err());
}
#[test]
fn fonts_output_and_cached_layout_are_deterministic_without_host_fallback() {
    let mut first = OverlayRenderer::new().unwrap();
    let mut second = OverlayRenderer::new().unwrap();
    assert_eq!(first.fonts.db().faces().count(), 2);
    assert!(
        first
            .fonts
            .db()
            .faces()
            .all(|face| matches!(face.weight.0, 400 | 600))
    );
    let content = content(OverlayKind::LowerThird);
    let assets = OverlayAssets::new();
    let a = frame(
        &mut first,
        &content,
        Orientation::Horizontal,
        1500,
        &assets,
        false,
    );
    assert_eq!(
        a,
        frame(
            &mut second,
            &content,
            Orientation::Horizontal,
            1500,
            &assets,
            false
        )
    );
    assert_eq!(
        a,
        frame(
            &mut first,
            &content,
            Orientation::Horizontal,
            1500,
            &assets,
            false
        )
    );
    assert_eq!(first.layouts.len(), 1);
    let unsupported = OverlayContent::new(OverlayKind::Quote, "Unsupported: 🦄");
    assert!(
        first
            .render_live(
                Orientation::Horizontal.output_size(),
                &[ActiveOverlay {
                    content: &unsupported,
                    elapsed_ms: 1500,
                    duration_ms: 5000
                }],
                &assets,
                RenderOptions::default()
            )
            .is_err()
    );
}
#[test]
fn half_open_intervals_and_reduced_motion_preserve_static_content_and_all_highlight_styles() {
    let mut renderer = OverlayRenderer::new().unwrap();
    let assets = OverlayAssets::new();
    let mut content = content(OverlayKind::Quote);
    let size = Orientation::Horizontal.output_size();
    for ms in [5000, u64::MAX] {
        assert!(
            frame(
                &mut renderer,
                &content,
                Orientation::Horizontal,
                ms,
                &assets,
                false
            )
            .pixels
            .iter()
            .all(|b| *b == 0)
        );
    }
    assert!(
        frame(
            &mut renderer,
            &content,
            Orientation::Horizontal,
            0,
            &assets,
            false
        )
        .pixels
        .iter()
        .all(|b| *b == 0)
    );
    let mut finals = Vec::new();
    for style in [
        HighlightStyle::MarkerSweep,
        HighlightStyle::UnderlineDraw,
        HighlightStyle::ZoomToLine,
        HighlightStyle::FadeRest,
    ] {
        content.highlight_style = style;
        let initial = frame(
            &mut renderer,
            &content,
            Orientation::Horizontal,
            0,
            &assets,
            true,
        );
        assert!(initial.pixels.iter().any(|p| *p != 0));
        let later = frame(
            &mut renderer,
            &content,
            Orientation::Horizontal,
            3000,
            &assets,
            true,
        );
        assert_eq!(initial, later);
        content.motion = OverlayMotion::Still;
        assert_eq!(
            initial,
            frame(
                &mut renderer,
                &content,
                Orientation::Horizontal,
                1,
                &assets,
                false
            )
        );
        content.motion = OverlayMotion::FadeUp;
        let animated = frame(
            &mut renderer,
            &content,
            Orientation::Horizontal,
            180,
            &assets,
            false,
        );
        assert_ne!(animated, later);
        finals.push(frame(
            &mut renderer,
            &content,
            Orientation::Horizontal,
            1800,
            &assets,
            false,
        ));
    }
    assert_ne!(finals[0], finals[1]);
    assert_ne!(finals[0], finals[2]);
    assert_ne!(finals[0], finals[3]);
    assert!(ease(0.) == 0. && ease(1.) == 1. && ease(0.5) > 0.9);
    assert_eq!(enter_progress(89, 1, false), 0.);
    assert_eq!(enter_progress(90, 1, false), 0.);
    assert!(enter_progress(91, 1, false) > 0.);
    assert_eq!(enter_progress(690, 1, false), 1.);
    assert_eq!(enter_progress(0, 8, true), 1.);
    assert!(
        renderer
            .render_live(size, &[], &assets, RenderOptions::default())
            .unwrap()
            .pixels
            .iter()
            .all(|p| *p == 0)
    );
}
#[test]
fn straight_alpha_matches_rgba_consumers_and_transparent_pixels_have_no_color() {
    let mut pixmap = Pixmap::new(2, 1).unwrap();
    pixmap.data_mut()[..4].copy_from_slice(&[60, 48, 90, 128]);
    let frame = straight(pixmap);
    assert_eq!(&frame.pixels[..4], &[120, 96, 179, 128]);
    assert_eq!(&frame.pixels[4..], &[0, 0, 0, 0]);
    // Standard straight-alpha compositing reconstructs the original premultiplied color.
    for channel in 0..3 {
        assert!(
            ((u16::from(frame.pixels[channel]) * 128 + 127) / 255).abs_diff([60, 48, 90][channel])
                <= 1
        );
    }
}
#[test]
fn prepared_images_are_confined_cached_and_errors_never_drop_text_silently() {
    let (_temp, store) = project_image();
    let mut renderer = OverlayRenderer::new().unwrap();
    let mut assets = OverlayAssets::new();
    let content = content(OverlayKind::Photo);
    let render = |renderer: &mut OverlayRenderer, assets: &OverlayAssets| {
        renderer.render_live(
            Orientation::Horizontal.output_size(),
            &[ActiveOverlay {
                content: &content,
                elapsed_ms: 2000,
                duration_ms: 5000,
            }],
            assets,
            RenderOptions::default(),
        )
    };
    assert!(render(&mut renderer, &assets).is_err());
    assets.load_content(&store, &content).unwrap();
    let before = render(&mut renderer, &assets).unwrap();
    fs::remove_file(store.root().join("media/generated.png")).unwrap();
    assert_eq!(before, render(&mut renderer, &assets).unwrap());
    assets.clear();
    assert!(assets.load_content(&store, &content).is_err());
    let mut too_long = OverlayContent::new(OverlayKind::Quote, "Every word must remain");
    too_long.blocks = vec![block("body", &"Long copied sentence. ".repeat(70))];
    assert!(too_long.validate().is_ok());
    assert!(
        renderer
            .render_live(
                Orientation::Horizontal.output_size(),
                &[ActiveOverlay {
                    content: &too_long,
                    elapsed_ms: 2000,
                    duration_ms: 5000
                }],
                &assets,
                RenderOptions::default()
            )
            .unwrap_err()
            .to_string()
            .contains("does not fit")
    );
    #[cfg(unix)]
    {
        let source = store.root().join("media/source.png");
        fs::write(&source, b"private original").unwrap();
        let target = store.root().join("media/generated.png");
        std::os::unix::fs::symlink(&source, &target).unwrap();
        assert!(assets.load_content(&store, &content).is_err());
        fs::remove_file(&target).unwrap();
        fs::hard_link(&source, &target).unwrap();
        assert!(assets.load_content(&store, &content).is_err());
    }
}
#[test]
fn slot_two_four_seconds_into_take_three_follows_the_take_after_move() {
    let mut editor = Editor::new(Studio::default()).unwrap();
    let source = editor.studio().next_media_id();
    editor
        .apply(Edit::AddMedia {
            path: "media/kept.mp4".into(),
            duration_ms: 10000,
        })
        .unwrap();
    let mut clips = Vec::new();
    for number in 1..=3 {
        let clip = editor.studio().next_clip_id();
        editor
            .apply(Edit::AddClip {
                source,
                name: format!("Take {number}"),
                in_ms: 0,
                out_ms: 10000,
            })
            .unwrap();
        clips.push(clip);
    }
    let content = content(OverlayKind::LowerThird);
    let cue = match editor
        .apply(Edit::AddCueFromSnapshot {
            clip: clips[2],
            offset_ms: 4000,
            slot: SlotKey::try_from(2).unwrap(),
            content: content.clone(),
            duration_ms: 2000,
        })
        .unwrap()
    {
        EditOutcome::CueAdded(cue) => cue,
        _ => panic!("cue"),
    };
    let mut renderer = OverlayRenderer::new().unwrap();
    let assets = OverlayAssets::new();
    let options = RenderOptions {
        reduced_motion: true,
    };
    let original = renderer
        .render_timeline(editor.studio(), 24000, &assets, options)
        .unwrap();
    assert!(original.pixels.iter().any(|p| *p != 0));
    editor
        .apply(Edit::MoveClip {
            clip: clips[2],
            to_index: 0,
        })
        .unwrap();
    assert_eq!(editor.studio().cue_sequence_time(cue).unwrap(), 4000);
    editor
        .apply(Edit::SetSlotContent {
            slot: SlotKey::try_from(2).unwrap(),
            content: OverlayContent::new(OverlayKind::Stat, "A later slot edit"),
        })
        .unwrap();
    assert_eq!(
        original,
        renderer
            .render_timeline(editor.studio(), 4000, &assets, options)
            .unwrap()
    );
    for ms in [3999, 6000, 24000] {
        assert!(
            renderer
                .render_timeline(editor.studio(), ms, &assets, options)
                .unwrap()
                .pixels
                .iter()
                .all(|p| *p == 0)
        );
    }
    editor.apply(Edit::RemoveClip { clip: clips[2] }).unwrap();
    assert!(
        renderer
            .render_timeline(editor.studio(), 4000, &assets, options)
            .unwrap()
            .pixels
            .iter()
            .all(|p| *p == 0)
    );
    // Legacy kind-only content stays explicitly missing rather than invented.
    editor
        .apply(Edit::SetSlotKind {
            slot: SlotKey::try_from(1).unwrap(),
            kind: Some(OverlayKind::Quote),
        })
        .unwrap();
    editor
        .apply(Edit::AddCue {
            clip: clips[0],
            offset_ms: 1000,
            slot: SlotKey::try_from(1).unwrap(),
            duration_ms: 1000,
        })
        .unwrap();
    assert!(
        renderer
            .render_timeline(editor.studio(), 1000, &assets, options)
            .unwrap_err()
            .to_string()
            .contains("no approved")
    );
}
#[test]
fn old_content_roundtrips_with_motion_defaults_and_rejects_unknown_styles() {
    let old =
        r#"{"kind":"quote","title":"Keep exact words","blocks":[],"highlights":[],"image":null}"#;
    let value: OverlayContent = serde_json::from_str(old).unwrap();
    assert_eq!(value.motion, OverlayMotion::FadeUp);
    assert_eq!(value.highlight_style, HighlightStyle::MarkerSweep);
    assert_eq!(
        value,
        serde_json::from_str::<OverlayContent>(&serde_json::to_string(&value).unwrap()).unwrap()
    );
    assert!(
        serde_json::from_str::<OverlayContent>(&old.replace(
            "\"image\":null",
            "\"image\":null,\"highlight_style\":\"unknown\""
        ))
        .is_err()
    );
}

#[test]
fn all_nine_slots_use_snapshots_and_overlapping_work_has_a_visible_resource_bound() {
    let mut editor = Editor::new(Studio::default()).unwrap();
    for number in 1..=9 {
        editor
            .apply(Edit::SetSlotContent {
                slot: SlotKey::try_from(number).unwrap(),
                content: OverlayContent::new(OverlayKind::LowerThird, format!("Slot {number}")),
            })
            .unwrap();
    }
    let overlays: Vec<_> = editor
        .studio()
        .slots()
        .iter()
        .map(|slot| ActiveOverlay {
            content: slot.content().unwrap(),
            elapsed_ms: 2000,
            duration_ms: 5000,
        })
        .collect();
    let mut renderer = OverlayRenderer::new().unwrap();
    let result = renderer
        .render_live(
            Orientation::Horizontal.output_size(),
            &overlays,
            &OverlayAssets::new(),
            RenderOptions::default(),
        )
        .unwrap();
    assert!(result.pixels.iter().any(|b| *b != 0));
    let mut too_many = overlays.clone();
    too_many.push(overlays[0]);
    assert!(
        renderer
            .render_live(
                Orientation::Horizontal.output_size(),
                &too_many,
                &OverlayAssets::new(),
                RenderOptions::default()
            )
            .unwrap_err()
            .to_string()
            .contains("nine overlays")
    );
}

#[test]
#[ignore = "Explicit generated-only cached renderer performance measurement"]
fn cached_1080p_three_card_timing() {
    let (_temp, store) = project_image();
    let mut assets = OverlayAssets::new();
    let cards = [
        content(OverlayKind::ScreenshotCard),
        content(OverlayKind::LowerThird),
        content(OverlayKind::Quote),
    ];
    for card in &cards {
        assets.load_content(&store, card).unwrap();
    }
    let mut renderer = OverlayRenderer::new().unwrap();
    let mut active: Vec<_> = cards
        .iter()
        .map(|content| ActiveOverlay {
            content,
            elapsed_ms: 1200,
            duration_ms: 5000,
        })
        .collect();
    let size = Orientation::Horizontal.output_size();
    // Decode/load/shape/raster warmup happens before the timed filesystem-free loop.
    std::hint::black_box(
        renderer
            .render_live(size, &active, &assets, RenderOptions::default())
            .unwrap(),
    );
    let mut times = Vec::new();
    for frame in 0..30 {
        for overlay in &mut active {
            overlay.elapsed_ms = 1 + frame * 20;
        }
        let start = std::time::Instant::now();
        let rgba = renderer
            .render_live(size, &active, &assets, RenderOptions::default())
            .unwrap();
        std::hint::black_box(rgba);
        times.push(start.elapsed().as_secs_f64() * 1000.);
    }
    let total: f64 = times.iter().sum();
    times.sort_by(f64::total_cmp);
    println!(
        "cached-render profile={} size=1920x1080 cards=3 frames=30 mean_ms={:.3} median_ms={:.3} p95_ms={:.3} max_ms={:.3} fps={:.2}",
        if cfg!(debug_assertions) {
            "debug"
        } else {
            "release"
        },
        total / 30.,
        (times[14] + times[15]) / 2.,
        times[28],
        times[29],
        30000. / total
    );
    let start = std::time::Instant::now();
    for _ in 0..30 {
        for card in &cards {
            card.validate().unwrap();
        }
    }
    println!(
        "isolated-three-card-validation mean_ms={:.3}",
        start.elapsed().as_secs_f64() * 1000. / 30.
    );
}

#[test]
fn validated_layout_cache_never_accepts_changed_untrusted_content() {
    let mut renderer = OverlayRenderer::new().unwrap();
    let assets = OverlayAssets::new();
    let mut card = content(OverlayKind::Quote);
    frame(
        &mut renderer,
        &card,
        Orientation::Horizontal,
        1800,
        &assets,
        false,
    );
    let original = card.clone();
    card.title = "api_key=sk-ant-very-private-key-value-do-not-save".into();
    assert!(
        renderer
            .render_live(
                Orientation::Horizontal.output_size(),
                &[ActiveOverlay {
                    content: &card,
                    elapsed_ms: 1800,
                    duration_ms: 5000
                }],
                &assets,
                RenderOptions::default()
            )
            .unwrap_err()
            .to_string()
            .contains("credential")
    );
    card = original.clone();
    card.highlights[0].end = 999;
    assert!(
        renderer
            .render_live(
                Orientation::Horizontal.output_size(),
                &[ActiveOverlay {
                    content: &card,
                    elapsed_ms: 1800,
                    duration_ms: 5000
                }],
                &assets,
                RenderOptions::default()
            )
            .is_err()
    );
    card = original;
    card.blocks[0].text.push('\0');
    assert!(
        renderer
            .render_live(
                Orientation::Horizontal.output_size(),
                &[ActiveOverlay {
                    content: &card,
                    elapsed_ms: 1800,
                    duration_ms: 5000
                }],
                &assets,
                RenderOptions::default()
            )
            .is_err()
    );
    assert_eq!(renderer.layouts.len(), 1);
}

#[test]
#[ignore = "Explicit full-resolution cropped/full reference equivalence proof"]
fn cropped_layers_are_identical_to_full_frame_reference() {
    let (_temp, store) = project_image();
    let mut assets = OverlayAssets::new();
    let mut cropped = OverlayRenderer::new().unwrap();
    let mut full = OverlayRenderer::new().unwrap();
    full.full_layer_reference = true;
    for kind in [
        OverlayKind::LowerThird,
        OverlayKind::Stat,
        OverlayKind::ScreenshotCard,
        OverlayKind::NumberedList,
        OverlayKind::Photo,
        OverlayKind::Quote,
    ] {
        let mut card = content(kind);
        assets.load_content(&store, &card).unwrap();
        for style in [
            HighlightStyle::MarkerSweep,
            HighlightStyle::UnderlineDraw,
            HighlightStyle::ZoomToLine,
            HighlightStyle::FadeRest,
        ] {
            card.highlight_style = style;
            for shape in [Orientation::Horizontal, Orientation::Vertical] {
                for ms in [0, 90, 400, 900] {
                    let expected = frame(&mut full, &card, shape, ms, &assets, false);
                    let actual = frame(&mut cropped, &card, shape, ms, &assets, false);
                    assert!(
                        actual == expected,
                        "Cropped layer changed bytes: {kind:?} {style:?} {shape:?} {ms}ms"
                    );
                }
            }
        }
    }
}
