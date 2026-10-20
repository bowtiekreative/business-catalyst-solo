//! Explicit generated-only media proof; no camera or network inputs.
use super::*;
use crate::{
    Edit, Editor, Orientation, OverlayContent, ProjectStore, SlotKey, Studio,
    export::{ExportOptions, export_sequence},
    media::{self, Cancellation},
    renderer::{ActiveOverlay, OverlayAssets, OverlayRenderer, RenderOptions, RgbaFrame},
};
use std::{
    fs,
    path::Path,
    process::Command,
    time::{Duration, Instant},
};
fn command() -> Command {
    let mut c = Command::new(media::tool("ffmpeg").unwrap());
    c.args([
        "-hide_banner",
        "-loglevel",
        "error",
        "-nostdin",
        "-y",
        "-threads",
        "2",
        "-filter_threads",
        "1",
    ]);
    c
}
fn run(c: &mut Command) {
    media::run_bounded(c, Duration::from_secs(30), &Cancellation::default(), 65536).unwrap();
}
fn fixture() -> (tempfile::TempDir, ProjectStore, Studio) {
    assert_eq!(
        std::env::var("STORYFOLD_CAPTURE_TEST").as_deref(),
        Ok("1"),
        "Generated proof requires explicit opt-in"
    );
    let temp = tempfile::Builder::new()
        .prefix("storyfold-insert-proof-")
        .tempdir()
        .unwrap();
    let (store, studio) =
        ProjectStore::create(temp.path().join("project with spaces"), "Insert proof").unwrap();
    let mut camera = command();
    camera
        .args([
            "-f",
            "lavfi",
            "-i",
            "color=c=0x14508c:s=320x240:r=30:d=6",
            "-f",
            "lavfi",
            "-i",
            "sine=frequency=440:sample_rate=48000:duration=6",
            "-vf",
            "drawbox=x=0:y=0:w=40:h=240:c=red:t=fill,drawbox=x=280:y=0:w=40:h=240:c=green:t=fill",
            "-c:v",
            "libx264",
            "-preset",
            "ultrafast",
            "-pix_fmt",
            "yuv420p",
            "-c:a",
            "aac",
            "-shortest",
        ])
        .arg(store.root().join("media/camera.mp4"));
    run(&mut camera);
    let mut insert = command();
    insert
        .args([
            "-f",
            "lavfi",
            "-i",
            "testsrc2=size=320x240:rate=24:duration=6.5",
            "-an",
            "-c:v",
            "libx264",
            "-preset",
            "ultrafast",
            "-pix_fmt",
            "yuv420p",
        ])
        .arg(temp.path().join("selected video.mp4"));
    run(&mut insert);
    image::RgbaImage::from_fn(480, 320, |x, y| {
        image::Rgba([(x / 2) as u8, (y / 2) as u8, 70, 255])
    })
    .save(store.root().join("media/photo.png"))
    .unwrap();
    let mut editor = Editor::new(studio).unwrap();
    let media = editor.studio().next_media_id();
    editor
        .apply(Edit::AddMedia {
            path: "media/camera.mp4".into(),
            duration_ms: 6000,
        })
        .unwrap();
    editor
        .apply(Edit::AddClip {
            source: media,
            name: "Narration".into(),
            in_ms: 0,
            out_ms: 6000,
        })
        .unwrap();
    (temp, store, editor.studio().clone())
}
fn decode(path: &Path, frame: u64, width: u32, height: u32) -> RgbaFrame {
    let mut c = command();
    c.args(["-i"])
        .arg(path)
        .arg("-vf")
        .arg(format!("select=eq(n\\,{frame})"))
        .args([
            "-frames:v",
            "1",
            "-pix_fmt",
            "rgba",
            "-f",
            "rawvideo",
            "pipe:1",
        ]);
    let pixels = media::run_bounded(
        &mut c,
        Duration::from_secs(30),
        &Cancellation::default(),
        16 * 1024 * 1024,
    )
    .unwrap()
    .stdout;
    assert_eq!(pixels.len(), width as usize * height as usize * 4);
    RgbaFrame {
        width,
        height,
        pixels,
    }
}
fn difference(a: &RgbaFrame, b: &RgbaFrame) -> f64 {
    assert_eq!((a.width, a.height), (b.width, b.height));
    let sum: u64 = a
        .pixels
        .as_chunks::<4>()
        .0
        .iter()
        .zip(b.pixels.as_chunks::<4>().0)
        .map(|(a, b)| (0..3).map(|c| u64::from(a[c].abs_diff(b[c]))).sum::<u64>())
        .sum();
    sum as f64 / (a.width as f64 * a.height as f64 * 3.)
}
fn content(source: InsertSource, placement: InsertPlacement) -> OverlayContent {
    OverlayContent::from_insert(
        "Approved insert",
        InsertContent {
            source,
            placement,
            zoom: ZoomSpec {
                end: ZoomPoint {
                    center_x: 650,
                    center_y: 400,
                    percent: 250,
                },
                ..ZoomSpec::default()
            },
        },
    )
}
#[test]
#[ignore = "explicit generated FFmpeg insert import, bounded cursor and rejection proof"]
fn generated_video_proxy_exact_seek_stale_frame_cancel_and_links() {
    let (temp, store, _) = fixture();
    let cancel = Cancellation::default();
    let original = fs::read(temp.path().join("selected video.mp4")).unwrap();
    let mut progress = Vec::new();
    let proxy = import_video(
        &store,
        &temp.path().join("selected video.mp4"),
        &cancel,
        |ms| progress.push(ms),
    )
    .unwrap();
    assert_eq!(
        fs::read(temp.path().join("selected video.mp4")).unwrap(),
        original
    );
    assert_eq!(proxy.frames, 195);
    assert_eq!((proxy.size.width, proxy.size.height), (320, 240));
    assert!(progress.last().is_some_and(|ms| *ms >= 6000));
    assert!(progress.windows(2).all(|v| v[0] <= v[1]));
    let mut cursor = VideoCursor::new(&store, &proxy.path, &cancel).unwrap();
    let mut frames = Vec::new();
    for frame in [0, 75, 150] {
        frames.push(cursor.frame_at(frame, &cancel).unwrap().clone());
    }
    for (index, frame) in [(1, 75), (0, 0), (2, 150)] {
        assert_eq!(cursor.frame_at(frame, &cancel).unwrap(), &frames[index]);
    }
    let exact_frames: Vec<_> = (0..28)
        .map(|frame| cursor.frame_at(frame, &cancel).unwrap().clone())
        .collect();
    assert!(cursor.frame_at(frame_index(6499).unwrap(), &cancel).is_ok());
    assert!(
        cursor
            .frame_at(frame_index(6500).unwrap(), &cancel)
            .is_err()
    );
    assert!(cursor.frame_at(proxy.frames, &cancel).is_err());
    let before = Instant::now();
    drop(cursor);
    assert!(before.elapsed() < Duration::from_secs(5));
    let content = content(
        InsertSource::Video {
            path: proxy.path.clone(),
            source_in_ms: 0,
        },
        InsertPlacement::Fullscreen,
    );
    let mut assets = OverlayAssets::new();
    // The shared asset path must decode the consecutive source frames even when
    // its elapsed clock came from subtracting two rounded camera timestamps.
    // Include fractional source offsets on both sides of a frame boundary.
    for (source_in_ms, first) in [(0, 0), (32, 0), (33, 1), (666, 20)] {
        let phased = self::content(
            InsertSource::Video {
                path: proxy.path.clone(),
                source_in_ms,
            },
            InsertPlacement::Fullscreen,
        );
        for phase in 0..3 {
            for frame in 0..8 {
                let rounded_ms = |index: u64| (index * 1000 + 15) / 30;
                let elapsed_ms = rounded_ms(phase + frame) - rounded_ms(phase);
                assets
                    .prepare_live(
                        &store,
                        &[ActiveOverlay {
                            content: &phased,
                            elapsed_ms,
                            duration_ms: 6000,
                        }],
                        &cancel,
                    )
                    .unwrap();
                assert_eq!(
                    assets.insert_preview(&phased, elapsed_ms).unwrap(),
                    exact_frames[(first + frame) as usize],
                    "source {source_in_ms}ms, cue phase {phase}, elapsed {elapsed_ms}ms"
                );
            }
        }
    }
    let active = [ActiveOverlay {
        content: &content,
        elapsed_ms: 0,
        duration_ms: 6000,
    }];
    assets.prepare_live(&store, &active, &cancel).unwrap();
    let camera = RgbaFrame {
        width: 16,
        height: 9,
        pixels: [20, 80, 140, 255].repeat(16 * 9),
    };
    let mut renderer = OverlayRenderer::new().unwrap();
    let later = [ActiveOverlay {
        elapsed_ms: 34,
        ..active[0]
    }];
    assert!(
        renderer
            .render_scene_live(
                Orientation::Horizontal.output_size(),
                &camera,
                &later,
                &assets,
                RenderOptions::default()
            )
            .is_err()
    );
    let cancelled = Cancellation::default();
    cancelled.cancel();
    let count = fs::read_dir(store.root().join("media")).unwrap().count();
    assert!(prepare_video(&store, &proxy.path, &cancelled, |_| {}).is_err());
    assert_eq!(
        count,
        fs::read_dir(store.root().join("media")).unwrap().count()
    );
    let cancelled = Cancellation::default();
    let stop = cancelled.clone();
    assert!(
        prepare_video(&store, &proxy.path, &cancelled, move |ms| if ms > 500 {
            stop.cancel()
        })
        .is_err()
    );
    assert_eq!(
        count,
        fs::read_dir(store.root().join("media")).unwrap().count()
    );
    #[cfg(unix)]
    {
        let link = store.root().join("media/linked.mp4");
        std::os::unix::fs::symlink(store.root().join(&proxy.path), &link).unwrap();
        assert!(probe_video(&store, "media/linked.mp4", &cancel).is_err());
        fs::remove_file(&link).unwrap();
        fs::hard_link(store.root().join(&proxy.path), &link).unwrap();
        assert!(probe_video(&store, &proxy.path, &cancel).is_err());
    }
}
#[test]
#[ignore = "explicit generated shared scene / encoded MP4 parity at0,2.5,5s"]
fn generated_photo_and_video_scene_exports_match_three_times_both_shapes() {
    let (temp, mut store, studio) = fixture();
    let cancel = Cancellation::default();
    let proxy = import_video(
        &store,
        &temp.path().join("selected video.mp4"),
        &cancel,
        |_| {},
    )
    .unwrap();
    let camera_bytes = fs::read(store.root().join("media/camera.mp4")).unwrap();
    let camera = decode(&store.root().join("media/camera.mp4"), 0, 320, 240);
    let mut renderer = OverlayRenderer::new().unwrap();
    let cases = [
        (Orientation::Horizontal, InsertPlacement::Fullscreen, false),
        (Orientation::Vertical, InsertPlacement::Beside, false),
        (Orientation::Horizontal, InsertPlacement::Behind, true),
        (Orientation::Vertical, InsertPlacement::Fullscreen, true),
    ];
    for (index, (shape, placement, video)) in cases.into_iter().enumerate() {
        let mut editor = Editor::new(studio.clone()).unwrap();
        editor.apply(Edit::SetOrientation(shape)).unwrap();
        let clip = editor.studio().clips()[0].id();
        let source = if video {
            InsertSource::Video {
                path: proxy.path.clone(),
                source_in_ms: 0,
            }
        } else {
            InsertSource::Photo {
                path: "media/photo.png".into(),
            }
        };
        let content = content(source, placement);
        editor
            .apply(Edit::AddCueFromSnapshot {
                clip,
                offset_ms: 0,
                slot: SlotKey::try_from(1).unwrap(),
                content: content.clone(),
                duration_ms: 6000,
            })
            .unwrap();
        let outcome = export_sequence(
            &mut store,
            editor.studio(),
            ExportOptions {
                file_name: Some(format!("scene-{index}.mp4")),
                ..ExportOptions::default()
            },
            &cancel,
            |_| {},
        )
        .unwrap();
        assert_eq!(outcome.artifact.frames, 180);
        assert!(outcome.project_saved && outcome.version_error.is_none());
        let output = outcome.artifact.path(&store).unwrap();
        if index == 0 {
            let mut command = command();
            command.arg("-i").arg(&output).args([
                "-ss", "0.2", "-t", "0.2", "-map", "0:a:0", "-vn", "-ac", "1", "-ar", "48000",
                "-f", "f32le", "pipe:1",
            ]);
            let audio = media::run_bounded(&mut command, Duration::from_secs(30), &cancel, 65536)
                .unwrap()
                .stdout;
            let samples: Vec<_> = audio
                .as_chunks::<4>()
                .0
                .iter()
                .map(|sample| f64::from(f32::from_le_bytes(*sample)))
                .collect();
            assert!(samples.len() > 9000);
            assert!(
                (samples.iter().map(|sample| sample * sample).sum::<f64>() / samples.len() as f64)
                    .sqrt()
                    > 0.02,
                "camera narration became silent in a visual-insert export"
            );
        }
        let mut assets = OverlayAssets::new();
        for ms in [0, 2500, 5000] {
            let active = [ActiveOverlay {
                content: &content,
                elapsed_ms: ms,
                duration_ms: 6000,
            }];
            assets.prepare_live(&store, &active, &cancel).unwrap();
            let expected = renderer
                .render_scene_live(
                    shape.output_size(),
                    &camera,
                    &active,
                    &assets,
                    RenderOptions::default(),
                )
                .unwrap();
            assert_eq!(
                expected,
                renderer
                    .render_scene_timeline(
                        editor.studio(),
                        ms,
                        &camera,
                        &assets,
                        RenderOptions::default()
                    )
                    .unwrap()
            );
            let actual = decode(&output, ms * 30 / 1000, expected.width, expected.height);
            let error = difference(&expected, &actual);
            eprintln!(
                "{shape:?} {placement:?} video={video} t={ms} scene/decode RGB MAE={error:.3}"
            );
            if std::env::var("STORYFOLD_INSERT_PROOF").as_deref() == Ok("1") {
                image::save_buffer(
                    temp.path().join(format!("scene-{index}-{ms}-expected.png")),
                    &expected.pixels,
                    expected.width,
                    expected.height,
                    image::ColorType::Rgba8,
                )
                .unwrap();
                image::save_buffer(
                    temp.path().join(format!("scene-{index}-{ms}-decoded.png")),
                    &actual.pixels,
                    actual.width,
                    actual.height,
                    image::ColorType::Rgba8,
                )
                .unwrap();
            }
            assert!(
                error < 7.,
                "scene/decode parity exceeds explicit lossy tolerance: {error}"
            );
        }
        assert_eq!(
            fs::read(store.root().join("media/camera.mp4")).unwrap(),
            camera_bytes
        );
    }
    if std::env::var("STORYFOLD_INSERT_PROOF").as_deref() == Ok("1") {
        eprintln!(
            "M7 generated scene proof retained at {}",
            temp.keep().display()
        );
    }
}

#[test]
#[ignore = "explicit generated scene cancellation and invalid cue interval proof"]
fn generated_scene_cancel_overlap_and_source_end_preserve_project() {
    let (temp, mut store, studio) = fixture();
    let cancel = Cancellation::default();
    let proxy = import_video(
        &store,
        &temp.path().join("selected video.mp4"),
        &cancel,
        |_| {},
    )
    .unwrap();
    let mut editor = Editor::new(studio.clone()).unwrap();
    let clip = studio.clips()[0].id();
    let insert = content(
        InsertSource::Video {
            path: proxy.path.clone(),
            source_in_ms: 0,
        },
        InsertPlacement::Beside,
    );
    editor
        .apply(Edit::AddCueFromSnapshot {
            clip,
            offset_ms: 0,
            slot: SlotKey::try_from(1).unwrap(),
            content: insert.clone(),
            duration_ms: 6000,
        })
        .unwrap();
    let original_json = fs::read(store.root().join("project.json")).unwrap();
    let stop = Cancellation::default();
    let control = stop.clone();
    let result = export_sequence(
        &mut store,
        editor.studio(),
        ExportOptions::default(),
        &stop,
        move |p| {
            if p.phase == crate::export::ExportPhase::Encoding && p.completed_ms > 100 {
                control.cancel()
            }
        },
    );
    assert!(result.is_err());
    assert_eq!(
        fs::read_dir(store.root().join("exports")).unwrap().count(),
        0
    );
    assert_eq!(
        fs::read(store.root().join("project.json")).unwrap(),
        original_json
    );
    editor
        .apply(Edit::AddCueFromSnapshot {
            clip,
            offset_ms: 100,
            slot: SlotKey::try_from(2).unwrap(),
            content: insert,
            duration_ms: 1000,
        })
        .unwrap();
    let error = export_sequence(
        &mut store,
        editor.studio(),
        ExportOptions::default(),
        &cancel,
        |_| {},
    )
    .unwrap_err();
    assert!(error.to_string().contains("overlap"));
    assert_eq!(
        fs::read_dir(store.root().join("exports")).unwrap().count(),
        0
    );
    let mut editor = Editor::new(studio).unwrap();
    let insert = content(
        InsertSource::Video {
            path: proxy.path,
            source_in_ms: 6000,
        },
        InsertPlacement::Fullscreen,
    );
    editor
        .apply(Edit::AddCueFromSnapshot {
            clip,
            offset_ms: 0,
            slot: SlotKey::try_from(1).unwrap(),
            content: insert,
            duration_ms: 6000,
        })
        .unwrap();
    let error = export_sequence(
        &mut store,
        editor.studio(),
        ExportOptions::default(),
        &cancel,
        |_| {},
    )
    .unwrap_err();
    assert!(error.to_string().contains("last frame"));
    assert_eq!(
        fs::read_dir(store.root().join("exports")).unwrap().count(),
        0
    );
    assert_eq!(
        fs::read(store.root().join("project.json")).unwrap(),
        original_json
    );
}

#[test]
#[ignore = "explicit generated disguised-playlist confinement and pixel-aspect proof"]
fn generated_containers_reject_disguised_playlists_and_preserve_display_aspect() {
    let (temp, mut store, studio) = fixture();
    let cancel = Cancellation::default();
    let selected = temp.path().join("playlist.mp4");
    fs::write(&selected, b"ffconcat version 1.0\nfile 'linked.mp4'\n").unwrap();
    #[cfg(unix)]
    std::os::unix::fs::symlink(
        store.root().join("media/camera.mp4"),
        temp.path().join("linked.mp4"),
    )
    .unwrap();
    #[cfg(not(unix))]
    fs::copy(
        store.root().join("media/camera.mp4"),
        temp.path().join("linked.mp4"),
    )
    .unwrap();
    let before = fs::read_dir(store.root().join("media")).unwrap().count();
    assert!(media::probe_file(&selected, &cancel).is_err());
    assert!(media::import_media(&store, &selected, &cancel).is_err());
    assert_eq!(
        before,
        fs::read_dir(store.root().join("media")).unwrap().count()
    );
    fs::write(
        store.root().join("media/disguised.mp4"),
        b"ffconcat version 1.0\nfile 'linked.mp4'\n",
    )
    .unwrap();
    #[cfg(unix)]
    std::os::unix::fs::symlink(
        temp.path().join("selected video.mp4"),
        store.root().join("media/linked.mp4"),
    )
    .unwrap();
    #[cfg(not(unix))]
    fs::copy(
        temp.path().join("selected video.mp4"),
        store.root().join("media/linked.mp4"),
    )
    .unwrap();
    assert!(inspect_video(&store, "media/disguised.mp4", &cancel).is_err());
    assert!(prepare_video(&store, "media/disguised.mp4", &cancel, |_| {}).is_err());
    let mut editor = Editor::new(studio).unwrap();
    let media = editor.studio().next_media_id();
    editor
        .apply(Edit::AddMedia {
            path: "media/disguised.mp4".into(),
            duration_ms: 6000,
        })
        .unwrap();
    editor
        .apply(Edit::AddClip {
            source: media,
            name: "Disguised".into(),
            in_ms: 0,
            out_ms: 1000,
        })
        .unwrap();
    assert!(
        export_sequence(
            &mut store,
            editor.studio(),
            ExportOptions::default(),
            &cancel,
            |_| {}
        )
        .is_err()
    );
    assert_eq!(
        fs::read_dir(store.root().join("exports")).unwrap().count(),
        0
    );
    let original = temp.path().join("wide pixels.mp4");
    let mut c = command();
    c.args([
        "-f",
        "lavfi",
        "-i",
        "color=c=blue:s=320x240:r=30:d=0.2",
        "-vf",
        "setsar=2",
        "-an",
        "-c:v",
        "libx264",
        "-preset",
        "ultrafast",
        "-pix_fmt",
        "yuv420p",
    ])
    .arg(&original);
    run(&mut c);
    let imported = import_video(&store, &original, &cancel, |_| {}).unwrap();
    assert_eq!((imported.size.width, imported.size.height), (640, 240));
    assert_eq!(imported.frames, 6);
}
