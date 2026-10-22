use super::*;
#[cfg(feature = "capture-test")]
use crate::OverlayContent;
use crate::{Edit, Editor, OverlayKind, SlotKey};
#[cfg(feature = "capture-test")]
use std::{fs, process::Command, time::Duration};

fn model() -> Editor {
    let mut editor = Editor::new(Studio::default()).unwrap();
    let source = editor.studio().next_media_id();
    editor
        .apply(Edit::AddMedia {
            path: "media/source.mp4".into(),
            duration_ms: 1_000_000,
        })
        .unwrap();
    editor
        .apply(Edit::AddClip {
            source,
            name: "One".into(),
            in_ms: 0,
            out_ms: 1001,
        })
        .unwrap();
    editor
        .apply(Edit::AddClip {
            source,
            name: "Two".into(),
            in_ms: 1001,
            out_ms: 2002,
        })
        .unwrap();
    editor
}
#[test]
fn plan_uses_absolute_frame_grid_and_reports_shape_warning_and_legacy_cues() {
    let mut editor = model();
    let plan = ExportPlan::new(editor.studio()).unwrap();
    assert_eq!(plan.frames, 60);
    assert_eq!(plan.duration_ms, 2000);
    assert!(plan.warnings.is_empty());
    let source = editor.studio().media()[0].id();
    editor
        .apply(Edit::AddClip {
            source,
            name: "Long interview".into(),
            in_ms: 0,
            out_ms: 181000,
        })
        .unwrap();
    editor
        .apply(Edit::SetOrientation(Orientation::Vertical))
        .unwrap();
    assert_eq!(
        ExportPlan::new(editor.studio()).unwrap().warnings,
        vec![ExportWarning::VerticalLongerThanThreeMinutes]
    );
    let slot = SlotKey::try_from(2).unwrap();
    editor
        .apply(Edit::SetSlotKind {
            slot,
            kind: Some(OverlayKind::Quote),
        })
        .unwrap();
    let clip = editor.studio().clips()[0].id();
    editor
        .apply(Edit::AddCue {
            clip,
            offset_ms: 0,
            slot,
            duration_ms: 100,
        })
        .unwrap();
    assert!(
        ExportPlan::new(editor.studio())
            .unwrap_err()
            .to_string()
            .contains("approved")
    );
    editor.apply(Edit::RemoveClip { clip }).unwrap();
    assert!(ExportPlan::new(editor.studio()).is_ok());
    assert!(ExportPlan::new(&Studio::default()).is_err());
    assert_eq!(frame_count(33).unwrap(), 1);
    assert_eq!(frame_count(67).unwrap(), 2);
}
#[test]
fn output_names_are_confined_and_never_accept_secrets_or_command_paths() {
    for name in [
        "../outside.mp4",
        "nested/file.mp4",
        ".private.mp4",
        "video.mkv",
        "file\\video.mp4",
        "api_key=sk-ant-very-private-key-value-do-not-save.mp4",
    ] {
        assert!(
            output_relative(&ExportOptions {
                file_name: Some(name.into()),
                reduced_motion: false
            })
            .is_err()
        );
    }
    assert_eq!(
        output_relative(&ExportOptions {
            file_name: Some("A complete story.mp4".into()),
            reduced_motion: false
        })
        .unwrap(),
        "exports/A complete story.mp4"
    );
    assert_ne!(
        output_relative(&ExportOptions::default()).unwrap(),
        output_relative(&ExportOptions::default()).unwrap()
    );
}

#[test]
fn subframe_clips_and_actual_vertical_duration_boundaries_are_explicit() {
    let mut editor = Editor::new(Studio::default()).unwrap();
    let source = editor.studio().next_media_id();
    editor
        .apply(Edit::AddMedia {
            path: "media/long.mp4".into(),
            duration_ms: u64::MAX,
        })
        .unwrap();
    let clip = editor.studio().next_clip_id();
    editor
        .apply(Edit::AddClip {
            source,
            name: "Boundary".into(),
            in_ms: 0,
            out_ms: 1,
        })
        .unwrap();
    assert!(ExportPlan::new(editor.studio()).is_err());
    editor
        .apply(Edit::TrimClip {
            clip,
            in_ms: 0,
            out_ms: 180001,
        })
        .unwrap();
    editor
        .apply(Edit::SetOrientation(Orientation::Vertical))
        .unwrap();
    let plan = ExportPlan::new(editor.studio()).unwrap();
    assert_eq!(plan.duration_ms, 180000);
    assert!(plan.warnings.is_empty());
    editor
        .apply(Edit::TrimClip {
            clip,
            in_ms: 0,
            out_ms: 180017,
        })
        .unwrap();
    assert_eq!(
        ExportPlan::new(editor.studio()).unwrap().warnings,
        vec![ExportWarning::VerticalLongerThanThreeMinutes]
    );
    editor
        .apply(Edit::TrimClip {
            clip,
            in_ms: 0,
            out_ms: u64::MAX,
        })
        .unwrap();
    assert!(ExportPlan::new(editor.studio()).is_err());
}

#[cfg(feature = "capture-test")]
fn generated() -> (tempfile::TempDir, ProjectStore, Studio) {
    assert_eq!(
        std::env::var("STORYFOLD_CAPTURE_TEST").as_deref(),
        Ok("1"),
        "Generated integration requires explicit STORYFOLD_CAPTURE_TEST=1"
    );
    let temp = tempfile::Builder::new()
        .prefix("storyfold-export-generated-")
        .tempdir()
        .unwrap();
    let (store, studio) = ProjectStore::create(
        temp.path().join("project with spaces"),
        "Export generated fixture",
    )
    .unwrap();
    for (name, color, audio) in [("red", "red", false), ("blue", "blue", true)] {
        let mut command = Command::new(media::tool("ffmpeg").unwrap());
        command
            .args([
                "-hide_banner",
                "-loglevel",
                "error",
                "-nostdin",
                "-f",
                "lavfi",
                "-i",
            ])
            .arg(format!("color=c={color}:s=320x180:r=30:d=0.8"));
        if audio {
            command.args([
                "-f",
                "lavfi",
                "-i",
                "sine=frequency=440:sample_rate=48000:duration=0.8",
                "-c:a",
                "aac",
            ]);
        } else {
            command.arg("-an");
        }
        command
            .args([
                "-c:v",
                "libx264",
                "-preset",
                "ultrafast",
                "-crf",
                "18",
                "-pix_fmt",
                "yuv420p",
                "-threads",
                "2",
            ])
            .arg(store.root().join(format!("media/{name}.mp4")));
        media::run_bounded(
            &mut command,
            Duration::from_secs(30),
            &Cancellation::default(),
            1024,
        )
        .unwrap();
    }
    let mut editor = Editor::new(studio).unwrap();
    let red = editor.studio().next_media_id();
    editor
        .apply(Edit::AddMedia {
            path: "media/red.mp4".into(),
            duration_ms: 800,
        })
        .unwrap();
    let blue = editor.studio().next_media_id();
    editor
        .apply(Edit::AddMedia {
            path: "media/blue.mp4".into(),
            duration_ms: 800,
        })
        .unwrap();
    editor
        .apply(Edit::AddClip {
            source: red,
            name: "Red trim".into(),
            in_ms: 200,
            out_ms: 600,
        })
        .unwrap();
    let second = editor.studio().next_clip_id();
    editor
        .apply(Edit::AddClip {
            source: blue,
            name: "Blue trim".into(),
            in_ms: 0,
            out_ms: 400,
        })
        .unwrap();
    editor
        .apply(Edit::AddCueFromSnapshot {
            clip: second,
            offset_ms: 100,
            slot: SlotKey::try_from(2).unwrap(),
            content: OverlayContent::new(OverlayKind::LowerThird, "Exact approved words"),
            duration_ms: 200,
        })
        .unwrap();
    (temp, store, editor.studio().clone())
}
#[cfg(feature = "capture-test")]
fn git(root: &std::path::Path, args: &[&str]) -> String {
    let output = catalyst_core::local::hardened_git(root, args).unwrap();
    assert!(output.status.success());
    String::from_utf8(output.stdout).unwrap()
}
#[cfg(feature = "capture-test")]
fn decoded_frame(path: &std::path::Path, frame: u64) -> Vec<u8> {
    media::run_bounded(
        Command::new(media::tool("ffmpeg").unwrap())
            .args(["-hide_banner", "-loglevel", "error", "-threads", "2", "-i"])
            .arg(path)
            .args(["-vf"])
            .arg(format!("select=eq(n\\,{frame})"))
            .args([
                "-frames:v",
                "1",
                "-pix_fmt",
                "rgba",
                "-f",
                "rawvideo",
                "pipe:1",
            ]),
        Duration::from_secs(30),
        &Cancellation::default(),
        16 * 1024 * 1024,
    )
    .unwrap()
    .stdout
}
#[cfg(feature = "capture-test")]
fn audio_rms(path: &std::path::Path, start: &str) -> f64 {
    let output = media::run_bounded(
        Command::new(media::tool("ffmpeg").unwrap())
            .args(["-hide_banner", "-loglevel", "error", "-ss", start, "-i"])
            .arg(path)
            .args([
                "-t", "0.2", "-map", "0:a:0", "-vn", "-ac", "1", "-ar", "48000", "-f", "f32le",
                "pipe:1",
            ]),
        Duration::from_secs(30),
        &Cancellation::default(),
        128 * 1024,
    )
    .unwrap()
    .stdout;
    let samples: Vec<_> = output
        .as_chunks::<4>()
        .0
        .iter()
        .map(|p| f64::from(f32::from_le_bytes(*p)))
        .collect();
    assert!(samples.len() > 9000);
    (samples.iter().map(|v| v * v).sum::<f64>() / samples.len() as f64).sqrt()
}
#[cfg(feature = "capture-test")]
#[test]
#[ignore = "Explicit generated-only full sequence MP4 export/overlay/progress integration"]
fn generated_sequence_both_shapes_has_exact_frames_audio_overlay_and_local_version() {
    generated_sequence_contract(false);
}
#[cfg(feature = "capture-test")]
#[test]
#[ignore = "Explicit generated-only GPU-required export of both shapes/cuts/audio/shared overlays; STORYFOLD_GPU_EXPORT_TEST=1"]
fn generated_gpu_sequence_both_shapes_keeps_cut_audio_overlay_and_version_contract() {
    assert_eq!(
        std::env::var("STORYFOLD_GPU_EXPORT_TEST").as_deref(),
        Ok("1")
    );
    assert!(
        crate::encoding::prefer_gpu(),
        "GPU-required fixture needs the GPU preference enabled"
    );
    generated_sequence_contract(true);
}
#[cfg(feature = "capture-test")]
fn generated_sequence_contract(require_gpu: bool) {
    let (temp, mut store, studio) = generated();
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let originals = [
            fs::read(store.root().join("media/red.mp4")).unwrap(),
            fs::read(store.root().join("media/blue.mp4")).unwrap(),
        ];
        let mut previous_head = git(store.root(), &["rev-parse", "HEAD"]);
        for shape in [Orientation::Horizontal, Orientation::Vertical] {
            let mut editor = Editor::new(studio.clone()).unwrap();
            editor.apply(Edit::SetOrientation(shape)).unwrap();
            let snapshot = editor.studio();
            let reduced_motion = shape == Orientation::Horizontal;
            let mut progress = Vec::new();
            let outcome = export_sequence(
                &mut store,
                snapshot,
                ExportOptions {
                    file_name: Some(format!("{shape:?}.mp4")),
                    reduced_motion,
                },
                &Cancellation::default(),
                |event| progress.push(event),
            )
            .unwrap();
            if require_gpu {
                let backend = crate::encoding::VideoEncoder::Nvidia.label();
                let encoding_events: Vec<_> = progress
                    .iter()
                    .filter(|p| {
                        matches!(p.phase, ExportPhase::PreparingClips | ExportPhase::Encoding)
                    })
                    .collect();
                assert!(
                    !encoding_events.is_empty()
                        && encoding_events
                            .iter()
                            .all(|event| event.message.contains(backend)),
                    "GPU-required export must actually select NVENC in both stages, not silently pass on CPU"
                );
            }
            let path = outcome.artifact.path(&store).unwrap();
            assert!(
                audio_rms(&path, "0") < 0.001,
                "No-audio clip must stay silent"
            );
            assert!(
                audio_rms(&path, "0.5") > 0.02,
                "The second clip's real generated audio must survive the join"
            );
            assert!(
                outcome.project_saved
                    && outcome.version.is_some()
                    && outcome.version_error.is_none()
            );
            assert_eq!(outcome.artifact.frames, 24);
            let head = git(store.root(), &["rev-parse", "HEAD"]);
            assert_ne!(head, previous_head);
            previous_head = head;
            assert!(progress.windows(2).all(|p| p[0].fraction <= p[1].fraction));
            assert_eq!(progress.last().unwrap().fraction, 1.);
            assert!(
                progress
                    .iter()
                    .any(|p| p.phase == ExportPhase::Encoding && p.completed_ms > 0)
            );
            let before = decoded_frame(&path, 0);
            assert!(before[0] > 200 && before[2] < 20);
            for frame in [13, 22] {
                let pixels = decoded_frame(&path, frame);
                assert!(pixels[0] < 20 && pixels[2] > 200);
            }
            let pixels = decoded_frame(&path, 17);
            let size = shape.output_size();
            assert_eq!(pixels.len(), size.width as usize * size.height as usize * 4);
            let mut renderer = crate::renderer::OverlayRenderer::new().unwrap();
            let overlay = renderer
                .render_timeline(
                    snapshot,
                    frame_ms(17),
                    &crate::renderer::OverlayAssets::new(),
                    crate::renderer::RenderOptions { reduced_motion },
                )
                .unwrap();
            let mut difference = 0_u64;
            let mut samples = 0_u64;
            for (actual, expected) in pixels
                .as_chunks::<4>()
                .0
                .iter()
                .zip(overlay.pixels.as_chunks::<4>().0.iter())
                .filter(|(_, p)| p[3] > 0)
            {
                for channel in 0..3 {
                    let base = if channel == 2 { 254 } else { 0 };
                    let value = (u32::from(expected[channel]) * u32::from(expected[3])
                        + base * (255 - u32::from(expected[3]))
                        + 127)
                        / 255;
                    difference += u64::from(actual[channel].abs_diff(value as u8));
                    samples += 1;
                }
            }
            let error = difference as f64 / samples as f64;
            println!("{shape:?} shared overlay/decode mean absolute RGB error={error:.3}");
            assert!(samples > 10000 && error < 6.);
            assert!(git(store.root(), &["status", "--porcelain"]).is_empty());
            let bytes = fs::read(&path).unwrap();
            assert!(
                export_sequence(
                    &mut store,
                    snapshot,
                    ExportOptions {
                        file_name: Some(format!("{shape:?}.mp4")),
                        reduced_motion
                    },
                    &Cancellation::default(),
                    |_| {}
                )
                .is_err()
            );
            assert_eq!(fs::read(&path).unwrap(), bytes);
        }
        assert_eq!(
            fs::read(store.root().join("media/red.mp4")).unwrap(),
            originals[0]
        );
        assert_eq!(
            fs::read(store.root().join("media/blue.mp4")).unwrap(),
            originals[1]
        );
    }));
    if let Err(error) = result {
        eprintln!(
            "Retained generated export evidence: {}",
            temp.keep().display()
        );
        std::panic::resume_unwind(error);
    }
}
#[cfg(feature = "capture-test")]
#[test]
#[ignore = "Explicit generated-only cancellation/stale-save/version-failure preservation"]
fn generated_cancel_and_version_failures_preserve_sources_and_valid_outputs() {
    let (temp, mut store, studio) = generated();
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let head = git(store.root(), &["rev-parse", "HEAD"]);
        let json = fs::read(store.root().join("project.json")).unwrap();
        let cancel = Cancellation::default();
        let trigger = cancel.clone();
        assert!(
            export_sequence(
                &mut store,
                &studio,
                ExportOptions {
                    file_name: Some("cancel.mp4".into()),
                    reduced_motion: false
                },
                &cancel,
                |p| if p.phase == ExportPhase::Encoding {
                    trigger.cancel()
                }
            )
            .is_err()
        );
        assert!(!store.root().join("exports/cancel.mp4").exists());
        assert_eq!(git(store.root(), &["rev-parse", "HEAD"]), head);
        assert_eq!(fs::read(store.root().join("project.json")).unwrap(), json);
        git(store.root(), &["config", "--local", "user.name", ""]);
        let failed = export_sequence(
            &mut store,
            &studio,
            ExportOptions {
                file_name: Some("version-failed.mp4".into()),
                reduced_motion: true,
            },
            &Cancellation::default(),
            |_| {},
        )
        .unwrap();
        assert!(failed.project_saved && failed.version.is_none() && failed.version_error.is_some());
        assert!(failed.artifact.path(&store).unwrap().is_file());
        assert_eq!(git(store.root(), &["rev-parse", "HEAD"]), head);
        git(
            store.root(),
            &["config", "--local", "user.name", "Storyfold"],
        );
        let (mut outside, _) = ProjectStore::open(store.root()).unwrap();
        let mut newer = studio.clone();
        newer.rename("Changed outside this window").unwrap();
        let mut changed = false;
        let stale = export_sequence(
            &mut store,
            &studio,
            ExportOptions {
                file_name: Some("stale.mp4".into()),
                reduced_motion: true,
            },
            &Cancellation::default(),
            |p| {
                if p.phase == ExportPhase::Encoding && !changed {
                    outside.save(&newer).unwrap();
                    changed = true;
                }
            },
        )
        .unwrap();
        assert!(!stale.project_saved && stale.version_error.is_some());
        assert!(stale.artifact.path(&store).unwrap().is_file());
        assert_eq!(ProjectStore::open(store.root()).unwrap().1, newer);
        assert!(
            store.root().join("media/red.mp4").is_file()
                && store.root().join("media/blue.mp4").is_file()
        );
        let collision = store.root().join("exports/race.mp4");
        let mut injected = false;
        assert!(
            export_sequence(
                &mut store,
                &studio,
                ExportOptions {
                    file_name: Some("race.mp4".into()),
                    reduced_motion: true
                },
                &Cancellation::default(),
                |p| if p.phase == ExportPhase::Encoding && !injected {
                    fs::write(&collision, b"preserve an existing export").unwrap();
                    injected = true;
                }
            )
            .is_err()
        );
        assert!(injected);
        assert_eq!(
            fs::read(&collision).unwrap(),
            b"preserve an existing export"
        );
        assert_eq!(ProjectStore::open(store.root()).unwrap().1, newer);
        #[cfg(unix)]
        {
            let linked = store.root().join("exports/linked.mp4");
            std::os::unix::fs::symlink(&collision, &linked).unwrap();
            assert!(
                export_sequence(
                    &mut store,
                    &studio,
                    ExportOptions {
                        file_name: Some("linked.mp4".into()),
                        reduced_motion: true
                    },
                    &Cancellation::default(),
                    |_| {}
                )
                .is_err()
            );
            assert_eq!(
                fs::read(&collision).unwrap(),
                b"preserve an existing export"
            );
        }
    }));
    if let Err(error) = result {
        eprintln!(
            "Retained generated export evidence: {}",
            temp.keep().display()
        );
        std::panic::resume_unwind(error);
    }
}
