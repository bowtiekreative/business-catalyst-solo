use super::*;
use crate::{Orientation, OverlayContent, OverlayKind};
fn size(w: u32, h: u32) -> OutputSize {
    OutputSize {
        width: w,
        height: h,
    }
}
#[test]
fn zoom_absolute_endpoints_clamp_easings_and_reduced_motion() {
    let mut zoom = ZoomSpec {
        start: ZoomPoint::default(),
        end: ZoomPoint {
            center_x: 1000,
            center_y: 0,
            percent: 250,
        },
        duration_ms: 5000,
        easing: ZoomEasing::Steady,
    };
    for easing in [ZoomEasing::Smooth, ZoomEasing::Steady, ZoomEasing::Snappy] {
        zoom.easing = easing;
        let start = zoom
            .viewport(size(1600, 1200), size(1920, 1080), 0, false)
            .unwrap();
        let end = zoom
            .viewport(size(1600, 1200), size(1920, 1080), 5000, false)
            .unwrap();
        assert_eq!(start.width, 1600.);
        assert_eq!(start.height, 900.);
        assert_eq!(end.width, 640.);
        assert_eq!(end.height, 360.);
        assert_eq!((end.x, end.y), (960., 0.));
        assert_eq!(
            zoom.viewport(size(1600, 1200), size(1920, 1080), u64::MAX, false)
                .unwrap(),
            end
        );
        assert_eq!(
            zoom.viewport(size(1600, 1200), size(1920, 1080), 5000, true)
                .unwrap(),
            start
        );
        let mut prior = start.width;
        for time in (0..=5000).step_by(125) {
            let view = zoom
                .viewport(size(1600, 1200), size(1920, 1080), time, false)
                .unwrap();
            assert!(view.width <= prior + 0.001);
            assert!(
                view.x >= 0.
                    && view.y >= 0.
                    && view.x + view.width <= 1600.001
                    && view.y + view.height <= 1200.001
            );
            prior = view.width;
        }
    }
    assert_eq!(ease(ZoomEasing::Steady, 0.5), 0.5);
    assert_eq!(ease(ZoomEasing::Smooth, 0.5), 0.5);
    assert!(ease(ZoomEasing::Snappy, 0.5) > 0.9);
}
#[test]
fn box_roundtrip_geometry_and_validation_are_core_owned() {
    let point = ZoomPoint {
        center_x: 500,
        center_y: 500,
        percent: 200,
    };
    let zoom = ZoomSpec {
        start: point,
        ..ZoomSpec::default()
    };
    let view = zoom
        .viewport(size(1600, 1200), size(1080, 1920), 0, false)
        .unwrap();
    assert_eq!(
        ZoomPoint::from_viewport(size(1600, 1200), size(1080, 1920), view).unwrap(),
        point
    );
    for orientation in [Orientation::Horizontal, Orientation::Vertical] {
        for placement in [
            InsertPlacement::Fullscreen,
            InsertPlacement::Beside,
            InsertPlacement::Behind,
        ] {
            let s = orientation.output_size();
            let layout = SceneLayout::new(s, size(640, 480), placement).unwrap();
            for b in std::iter::once(layout.media).chain(layout.camera) {
                assert!(
                    b.x >= 0.
                        && b.y >= 0.
                        && b.x + b.width <= f64::from(s.width) + 0.001
                        && b.y + b.height <= f64::from(s.height) + 0.001
                );
            }
        }
    }
    assert!(
        ZoomPoint {
            percent: 99,
            ..point
        }
        .validate()
        .is_err()
    );
    assert!(
        ZoomPoint {
            percent: 251,
            ..point
        }
        .validate()
        .is_err()
    );
    assert!(
        ZoomPoint {
            center_x: 1001,
            ..point
        }
        .validate()
        .is_err()
    );
    assert!(
        ZoomSpec {
            duration_ms: 0,
            ..ZoomSpec::default()
        }
        .validate()
        .is_err()
    );
    assert!(
        ZoomPoint::from_viewport(
            size(100, 100),
            size(100, 100),
            Viewport {
                x: f64::NAN,
                y: 0.,
                width: 50.,
                height: 50.
            }
        )
        .is_err()
    );
}
#[test]
fn inserts_roundtrip_exact_and_legacy_defaults_without_ambiguous_content() {
    let insert = InsertContent {
        source: InsertSource::Video {
            path: "media/proxy.mp4".into(),
            source_in_ms: 100,
        },
        placement: InsertPlacement::Behind,
        zoom: ZoomSpec::default(),
    };
    assert_eq!(insert.source_frame(2500).unwrap(), Some(78));
    assert!(insert.source_frame(u64::MAX).is_err());
    let content = OverlayContent::from_insert("My insert", insert.clone());
    assert!(content.validate().is_ok());
    let json = serde_json::to_string(&content).unwrap();
    assert_eq!(
        serde_json::from_str::<OverlayContent>(&json).unwrap(),
        content
    );
    assert_eq!(
        content.media_paths().collect::<Vec<_>>(),
        vec!["media/proxy.mp4"]
    );
    let mut invalid = content.clone();
    invalid.image = Some("media/photo.png".into());
    assert!(invalid.validate().is_err());
    let mut bad = insert;
    bad.source = InsertSource::Photo {
        path: "../outside.png".into(),
    };
    assert!(bad.validate().is_err());
    let old = OverlayContent::new(OverlayKind::Quote, "Original");
    let mut value = serde_json::to_value(&old).unwrap();
    value.as_object_mut().unwrap().remove("insert");
    assert_eq!(
        serde_json::from_value::<OverlayContent>(value).unwrap(),
        old
    );
    let bad = serde_json::json!({"source":{"type":"photo","path":"media/a.png","remote":"https://example.org"},"placement":"fullscreen","zoom":{"start":{"center_x":500,"center_y":500,"percent":100},"end":{"center_x":500,"center_y":500,"percent":100},"duration_ms":5000,"easing":"smooth"}});
    assert!(serde_json::from_value::<InsertContent>(bad).is_err());
}

#[test]
fn integer_millisecond_clock_preserves_every_output_frame_without_drift() {
    for frame in 0..30 * 600 {
        let ms = (frame * 1000 + 15) / 30;
        assert_eq!(frame_index(ms).unwrap(), frame);
    }
    assert_eq!(frame_index(5999).unwrap(), 179);
}
