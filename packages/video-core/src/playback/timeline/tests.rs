use super::*;
use crate::{Edit, EditOutcome, Editor};

#[test]
fn edl_escapes_utf8_bytes_and_keeps_source_trim_duration_and_audio_layout() {
    let path = std::path::Path::new("/tmp/clip,;!é.mp4");
    let mut edl = String::new();
    append_edl(&mut edl, path, 1234, 5678, true).unwrap();
    assert_eq!(
        edl,
        format!(
            "%{}%{},1.234,5.678,layout=this\n",
            path.as_os_str().len(),
            path.display()
        )
    );
    assert!(edl.starts_with("%18%"));
}
#[test]
fn sequence_position_uses_ordered_source_trims_without_export_frame_rounding() {
    let plan = Plan {
        sources: vec![Source {
            relative: "media/a.mp4".into(),
            path: "a".into(),
            stamp: Stamp {
                len: 0,
                modified: SystemTime::UNIX_EPOCH,
                dev: 0,
                ino: 0,
            },
            info: media::MediaInfo {
                duration_ms: 10_000,
                width: 2,
                height: 2,
                has_audio: true,
            },
        }],
        segments: vec![
            Segment {
                source: 0,
                in_ms: 4000,
                start_ms: 0,
                end_ms: 1001,
            },
            Segment {
                source: 0,
                in_ms: 2000,
                start_ms: 1001,
                end_ms: 1501,
            },
        ],
        edl: String::new(),
    };
    assert_eq!(plan.at(0).1, 4000);
    assert_eq!(plan.at(1000).1, 5000);
    assert_eq!(plan.at(1001).1, 2000);
    assert_eq!(plan.at(u64::MAX).1, 2499);
}
#[test]
fn direct_preview_rejects_empty_legacy_cues_and_links_before_starting_player() {
    let temp = tempfile::tempdir().unwrap();
    let (store, _) =
        ProjectStore::create(temp.path().join("project"), "Direct preview boundary").unwrap();
    assert!(SequencePlayer::start_timeline(&store, &Studio::default(), true, false).is_err());
    let mut editor = Editor::new(Studio::default()).unwrap();
    let EditOutcome::MediaAdded(source) = editor
        .apply(Edit::AddMedia {
            path: "media/local.mp4".into(),
            duration_ms: 1000,
        })
        .unwrap()
    else {
        panic!()
    };
    let EditOutcome::ClipAdded(clip) = editor
        .apply(Edit::AddClip {
            source,
            name: "One".into(),
            in_ms: 0,
            out_ms: 1000,
        })
        .unwrap()
    else {
        panic!()
    };
    let slot = crate::SlotKey::try_from(2).unwrap();
    editor
        .apply(Edit::SetSlotKind {
            slot,
            kind: Some(crate::OverlayKind::Quote),
        })
        .unwrap();
    editor
        .apply(Edit::AddCue {
            clip,
            offset_ms: 0,
            slot,
            duration_ms: 100,
        })
        .unwrap();
    let error = Plan::prepare(&store, editor.studio(), &Cancellation::default())
        .err()
        .unwrap()
        .to_string();
    assert!(error.contains("approved"));
    let outside = temp.path().join("outside.mp4");
    fs::write(&outside, b"Link boundary; never decoded").unwrap();
    let linked = store.root().join("media/local.mp4");
    std::os::unix::fs::symlink(&outside, &linked).unwrap();
    editor.undo();
    assert!(Plan::prepare(&store, editor.studio(), &Cancellation::default()).is_err());
    assert_eq!(
        fs::read_dir(store.root().join("exports")).unwrap().count(),
        0
    );
}

fn generated_fixture() -> (tempfile::TempDir, ProjectStore, Editor) {
    let temp = tempfile::tempdir().unwrap();
    let (store, studio) =
        ProjectStore::create(temp.path().join("project"), "Direct timeline").unwrap();
    for (name, color, audio) in [("red", "red", false), ("blue", "blue", true)] {
        let mut command = Command::new(media::tool("ffmpeg").unwrap());
        command
            .args(["-v", "error", "-nostdin", "-f", "lavfi", "-i"])
            .arg(format!("color=c={color}:s=160x90:r=30:d=2"));
        if audio {
            command.args([
                "-f",
                "lavfi",
                "-i",
                "sine=frequency=440:sample_rate=48000:duration=2",
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
                "-threads",
                "1",
                "-preset",
                "ultrafast",
                "-pix_fmt",
                "yuv420p",
            ])
            .arg(store.root().join(format!("media/{name}.mp4")));
        media::run_bounded(
            &mut command,
            Duration::from_secs(15),
            &Cancellation::default(),
            4096,
        )
        .unwrap();
    }
    let mut editor = Editor::new(studio).unwrap();
    for name in ["red", "blue"] {
        let source = editor.studio().next_media_id();
        editor
            .apply(Edit::AddMedia {
                path: format!("media/{name}.mp4"),
                duration_ms: 2000,
            })
            .unwrap();
        editor
            .apply(Edit::AddClip {
                source,
                name: name.into(),
                in_ms: 200,
                out_ms: 1200,
            })
            .unwrap();
    }
    (temp, store, editor)
}
fn input_fixture() {
    assert_eq!(std::env::var("STORYFOLD_INPUT_FIXTURE").as_deref(), Ok("1"));
    assert!(std::env::var("XAUTHORITY").is_ok_and(|path| path.contains("/xvfb-run.")));
}
#[test]
fn preview_frame_clock_keeps_rounded_thirty_fps_boundaries() {
    for frame in 0..300 {
        assert_eq!(
            crate::insert::frame_index(crate::capture::frame_ms(frame)).unwrap(),
            frame
        );
    }
    assert_eq!(crate::insert::frame_index(33).unwrap(), 1);
    assert_eq!(crate::insert::frame_index(133 - 67).unwrap(), 2);
}

fn completed_error_player() -> SequencePlayer {
    completed_error_player_with_exit(true)
}
fn completed_error_player_with_exit(success: bool) -> SequencePlayer {
    let worker = thread::spawn(|| Err("Synthetic overlay-add Broken pipe during shutdown".into()));
    let begin = Instant::now();
    while !worker.is_finished() {
        assert!(begin.elapsed() < Duration::from_secs(2));
        thread::sleep(Duration::from_millis(1));
    }
    let directory = tempfile::tempdir().unwrap();
    SequencePlayer {
        child: Command::new("/bin/sh")
            .args([
                "-c",
                if success {
                    "IFS= read -r release; sleep 0.08; exit 0"
                } else {
                    "IFS= read -r release; sleep 0.08; exit 7"
                },
            ])
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap(),
        _controls: tempfile::NamedTempFile::new().unwrap(),
        timeline: Some(Runtime {
            cancel: Cancellation::default(),
            worker: Some(worker),
            failure: None,
            socket: directory.path().join("unused.sock"),
            _directory: directory,
        }),
    }
}

#[test]
fn normal_child_exit_wins_over_a_shutdown_ipc_error_without_blocking_status() {
    let mut player = completed_error_player();
    // Keep the owned child alive until this first status check. This makes the
    // IPC-error/process-exit interleaving deterministic, independent of load.
    assert!(
        player
            .finished()
            .expect("Normal Quit must allow process-exit handoff after a shutdown IPC error")
            .is_none()
    );
    let mut input = player.child.stdin.take().unwrap();
    writeln!(input, "finish").unwrap();
    drop(input);
    let begin = Instant::now();
    loop {
        if let Some(success) = player.finished().unwrap() {
            assert!(success, "Normal native child exit must remain successful");
            break;
        }
        assert!(begin.elapsed() < Duration::from_secs(2));
        thread::sleep(Duration::from_millis(5));
    }
}

#[test]
fn live_child_composition_failure_still_surfaces_after_bounded_handoff() {
    let mut player = completed_error_player();
    let begin = Instant::now();
    assert!(player.finished().unwrap().is_none());
    loop {
        match player.finished() {
            Err(error) => {
                assert_eq!(
                    error.to_string(),
                    "Synthetic overlay-add Broken pipe during shutdown"
                );
                assert!(begin.elapsed() >= Duration::from_millis(250));
                assert!(begin.elapsed() < Duration::from_secs(2));
                break;
            }
            Ok(None) => {
                assert!(begin.elapsed() < Duration::from_secs(2));
                thread::sleep(Duration::from_millis(5));
            }
            Ok(Some(_)) => panic!("Unreleased owned child must still be alive"),
        }
    }
}

#[test]
fn nonzero_native_child_exit_remains_a_playback_failure() {
    let mut player = completed_error_player_with_exit(false);
    assert!(player.finished().unwrap().is_none());
    let mut input = player.child.stdin.take().unwrap();
    writeln!(input, "finish").unwrap();
    drop(input);
    let begin = Instant::now();
    loop {
        if let Some(success) = player.finished().unwrap() {
            assert!(!success, "Nonzero native child exit must remain a failure");
            break;
        }
        assert!(begin.elapsed() < Duration::from_secs(2));
        thread::sleep(Duration::from_millis(5));
    }
}

#[test]
#[ignore = "Generated direct timeline native window; fresh Xvfb and STORYFOLD_INPUT_FIXTURE=1; audio uses null output"]
fn generated_direct_timeline_opens_sources_with_audio_and_without_export_or_save() {
    input_fixture();
    let (_temp, store, editor) = generated_fixture();
    let before = fs::read(store.root().join("project.json")).unwrap();
    let head = catalyst_core::local::hardened_git(store.root(), &["rev-parse", "HEAD"])
        .unwrap()
        .stdout;
    let begin = Instant::now();
    let mut player = SequencePlayer::start_timeline(&store, editor.studio(), false, false).unwrap();
    let elapsed = begin.elapsed();
    assert!(
        elapsed < Duration::from_secs(8),
        "Direct preview must not encode the sequence first: {elapsed:?}"
    );
    let socket = player.timeline.as_ref().unwrap().socket.clone();
    let mut ipc = Ipc::new(UnixStream::connect(socket).unwrap()).unwrap();
    let duration = ipc.get("duration").unwrap().as_f64().unwrap();
    assert!((duration - 2.).abs() < 0.04);
    assert_ne!(ipc.get("aid").unwrap(), json!("no"));
    thread::sleep(Duration::from_millis(1200));
    assert!(
        ipc.get("audio-params").unwrap()["samplerate"]
            .as_u64()
            .is_some()
    );
    assert!(player.finished().unwrap().is_none());
    assert_eq!(fs::read(store.root().join("project.json")).unwrap(), before);
    assert_eq!(
        catalyst_core::local::hardened_git(store.root(), &["rev-parse", "HEAD"])
            .unwrap()
            .stdout,
        head
    );
    assert_eq!(
        fs::read_dir(store.root().join("exports")).unwrap().count(),
        0
    );
    println!(
        "Generated direct EDL first-frame/startup {elapsed:?}; duration2s, later source audio decoded, no export/save/Git version"
    );
    drop(ipc);
    drop(player);
    // Build the large VALIDATED fixture once, outside the measured startup.
    // Replaying editing/history validation for 1,000 edits measures fixture
    // construction rather than direct player startup in an unoptimized test.
    let mut serialized = serde_json::to_value(editor.studio()).unwrap();
    let first = editor.studio().next_clip_id().number();
    let source = editor.studio().media()[0].id().number();
    serialized["clips"].as_array_mut().unwrap().extend((0..1000).map(|index| {
        json!({"id":first+index,"source_id":source,"name":format!("Repeated {index}"),"in_ms":200,"out_ms":300})
    }));
    serialized["next_clip"] = json!(first + 1000);
    let long_snapshot: Studio = serde_json::from_value(serialized).unwrap();
    long_snapshot.validate().unwrap();
    let begin = Instant::now();
    let player = SequencePlayer::start_timeline(&store, &long_snapshot, true, false).unwrap();
    let long = begin.elapsed();
    assert!(long < Duration::from_secs(8));
    let mut ipc =
        Ipc::new(UnixStream::connect(player.timeline.as_ref().unwrap().socket.clone()).unwrap())
            .unwrap();
    assert!((ipc.get("duration").unwrap().as_f64().unwrap() - 102.).abs() < 0.04);
    assert_eq!(fs::read(store.root().join("project.json")).unwrap(), before);
    assert_eq!(
        fs::read_dir(store.root().join("exports")).unwrap().count(),
        0
    );
    println!(
        "{}",
        json!({"short_startup_ms":elapsed.as_secs_f64()*1000.,"thousand_extra_cuts_startup_ms":long.as_secs_f64()*1000.,"long_duration_ms":long_snapshot.duration_ms(),"unique_sources":2,"exports":0})
    );
}

fn paused_composition_matches(
    player: &mut SequencePlayer,
    store: &ProjectStore,
    snapshot: &Studio,
    time: f64,
    directory: &std::path::Path,
    label: &str,
) {
    let mut ipc =
        Ipc::new(UnixStream::connect(player.timeline.as_ref().unwrap().socket.clone()).unwrap())
            .unwrap();
    ipc.call(json!(["set_property", "pause", true])).unwrap();
    ipc.call(json!(["seek", time, "absolute+exact"])).unwrap();
    let plan = Plan::prepare(store, snapshot, &Cancellation::default()).unwrap();
    let mut renderer = OverlayRenderer::new().unwrap();
    let mut assets = OverlayAssets::new();
    let begin = Instant::now();
    for attempt in 0..50 {
        assert!(player.finished().unwrap().is_none());
        let ms = (ipc.get("time-pos").unwrap().as_f64().unwrap() * 1000.).round() as u64;
        let rect = VideoRect::parse(&ipc.get("osd-dimensions").unwrap())
            .unwrap()
            .unwrap();
        let (source, source_ms) = plan.at(ms);
        let mut camera =
            CameraCursor::new(&source.path, &source.info, &Cancellation::default()).unwrap();
        let camera = camera.frame(source_ms, &Cancellation::default()).unwrap();
        let active = crate::renderer::active_overlays(snapshot, ms).unwrap();
        assets
            .prepare_live(store, &active, &Cancellation::default())
            .unwrap();
        let expected = renderer
            .render_scene_live(
                snapshot.orientation().output_size(),
                camera,
                &active,
                &assets,
                RenderOptions {
                    reduced_motion: false,
                },
            )
            .unwrap();
        let expected = layers::display_bitmap(&expected, rect).unwrap();
        let shot = directory.join(format!("{label}-{attempt}.png"));
        ipc.call(json!([
            "screenshot-to-file",
            shot.to_str().unwrap(),
            "window"
        ]))
        .unwrap();
        let actual = image::open(&shot).unwrap().to_rgba8();
        let actual =
            image::imageops::crop_imm(&actual, rect.x, rect.y, rect.width, rect.height).to_image();
        let sum: u64 = actual
            .as_raw()
            .as_chunks::<4>()
            .0
            .iter()
            .zip(expected.as_chunks::<4>().0)
            .map(|(a, b)| {
                u64::from(a[0].abs_diff(b[2]))
                    + u64::from(a[1].abs_diff(b[1]))
                    + u64::from(a[2].abs_diff(b[0]))
            })
            .sum();
        let difference = sum as f64 / (f64::from(rect.width) * f64::from(rect.height) * 3.);
        if difference < 3. {
            println!(
                "Native/shared composition {label}: meanRGBdifference={difference:.4}, target={time}, clock_ms={ms}, rect={rect:?}"
            );
            return;
        }
        assert!(
            begin.elapsed() < Duration::from_secs(8),
            "Native/shared composition {label} differs by{difference}"
        );
        thread::sleep(Duration::from_millis(100));
    }
    panic!("Native/shared composition never matched {label}");
}
#[test]
#[ignore = "Generated card/photo/video/zoom direct preview fidelity; requires fresh Xvfb/STORYFOLD_INPUT_FIXTURE=1"]
fn generated_direct_timeline_cards_and_insert_placements_match_shared_renderer() {
    input_fixture();
    let (temp, store, base) = generated_fixture();
    image::RgbaImage::from_fn(160, 90, |x, y| {
        image::Rgba([(x * 3 % 256) as u8, (y * 5 % 256) as u8, 180, 255])
    })
    .save(store.root().join("media/photo.png"))
    .unwrap();
    media::run_bounded(
        Command::new(media::tool("ffmpeg").unwrap())
            .args([
                "-v",
                "error",
                "-nostdin",
                "-f",
                "lavfi",
                "-i",
                "testsrc2=s=160x90:r=30:d=2",
                "-an",
                "-c:v",
                "libx264",
                "-threads",
                "1",
                "-preset",
                "ultrafast",
                "-pix_fmt",
                "yuv420p",
            ])
            .arg(store.root().join("media/visual.mp4")),
        Duration::from_secs(15),
        &Cancellation::default(),
        4096,
    )
    .unwrap();
    let before = fs::read(store.root().join("project.json")).unwrap();
    for shape in [crate::Orientation::Horizontal, crate::Orientation::Vertical] {
        for (name, placement, video) in [
            ("fullscreen", crate::InsertPlacement::Fullscreen, false),
            ("beside", crate::InsertPlacement::Beside, false),
            ("behind", crate::InsertPlacement::Behind, false),
            ("video-zoom", crate::InsertPlacement::Behind, true),
        ] {
            let mut editor = Editor::new(base.studio().clone()).unwrap();
            editor.apply(Edit::SetOrientation(shape)).unwrap();
            editor
                .apply(Edit::AddCueFromSnapshot {
                    clip: editor.studio().clips()[0].id(),
                    offset_ms: 0,
                    slot: crate::SlotKey::try_from(2).unwrap(),
                    content: crate::OverlayContent::new(
                        crate::OverlayKind::Quote,
                        "Approved words in direct preview",
                    ),
                    duration_ms: 1000,
                })
                .unwrap();
            let insert = crate::OverlayContent::from_insert(
                "Generated visual",
                crate::InsertContent {
                    source: if video {
                        crate::InsertSource::Video {
                            path: "media/visual.mp4".into(),
                            source_in_ms: 0,
                        }
                    } else {
                        crate::InsertSource::Photo {
                            path: "media/photo.png".into(),
                        }
                    },
                    placement,
                    zoom: crate::ZoomSpec {
                        duration_ms: 1000,
                        end: crate::ZoomPoint {
                            center_x: 650,
                            center_y: 450,
                            percent: 200,
                        },
                        ..Default::default()
                    },
                },
            );
            editor
                .apply(Edit::AddCueFromSnapshot {
                    clip: editor.studio().clips()[1].id(),
                    offset_ms: 0,
                    slot: crate::SlotKey::try_from(1).unwrap(),
                    content: insert,
                    duration_ms: 1000,
                })
                .unwrap();
            let mut player =
                SequencePlayer::start_timeline(&store, editor.studio(), true, false).unwrap();
            paused_composition_matches(
                &mut player,
                &store,
                editor.studio(),
                0.25,
                temp.path(),
                &format!("{shape:?}-{name}-card"),
            );
            paused_composition_matches(
                &mut player,
                &store,
                editor.studio(),
                1.75,
                temp.path(),
                &format!("{shape:?}-{name}"),
            );
            assert_eq!(fs::read(store.root().join("project.json")).unwrap(), before);
            assert_eq!(
                fs::read_dir(store.root().join("exports")).unwrap().count(),
                0
            );
        }
    }
}

#[cfg(feature = "capture-test")]
#[test]
#[ignore = "Actual OS X11/XTest direct-preview OSC with active bitmap; fresh Xvfb/STORYFOLD_INPUT_FIXTURE=1"]
fn generated_direct_timeline_native_osc_and_clean_quit_with_active_card() {
    input_fixture();
    let (_temp, store, mut editor) = generated_fixture();
    let clips: Vec<_> = editor
        .studio()
        .clips()
        .iter()
        .map(|clip| clip.id())
        .collect();
    for clip in clips {
        editor
            .apply(Edit::AddCueFromSnapshot {
                clip,
                offset_ms: 0,
                slot: crate::SlotKey::try_from(2).unwrap(),
                content: crate::OverlayContent::new(
                    crate::OverlayKind::Quote,
                    "Native controls remain visible below this active card",
                ),
                duration_ms: 1000,
            })
            .unwrap();
    }
    let mut player = SequencePlayer::start_timeline(&store, editor.studio(), true, false).unwrap();
    let mut ipc =
        Ipc::new(UnixStream::connect(player.timeline.as_ref().unwrap().socket.clone()).unwrap())
            .unwrap();
    ipc.call(json!(["set_property", "pause", true])).unwrap();
    ipc.call(json!([
        "set_property",
        "title",
        "Storyfold generated native input fixture"
    ]))
    .unwrap();
    let input = super::super::native_tests::NativeInput {
        player_pid: player.child.id(),
    };
    let until = |ipc: &mut Ipc, name: &str, expected: bool| {
        let begin = Instant::now();
        loop {
            if ipc.get(name).unwrap().as_bool() == Some(expected) {
                break;
            }
            assert!(
                begin.elapsed() < Duration::from_secs(5),
                "Direct native property{name} must become{expected}"
            );
            thread::sleep(Duration::from_millis(20));
        }
    };
    input.action("play-pause");
    until(&mut ipc, "pause", false);
    thread::sleep(Duration::from_millis(150));
    input.action("play-pause");
    until(&mut ipc, "pause", true);
    let paused = ipc.get("time-pos").unwrap().as_f64().unwrap();
    thread::sleep(Duration::from_millis(150));
    assert!((ipc.get("time-pos").unwrap().as_f64().unwrap() - paused).abs() < 0.05);
    input.action("fullscreen");
    until(&mut ipc, "fullscreen", true);
    input.action("escape");
    until(&mut ipc, "fullscreen", false);
    input.action("play-pause");
    until(&mut ipc, "pause", false);
    until(&mut ipc, "eof-reached", true);
    until(&mut ipc, "pause", true);
    input.action("play-pause");
    until(&mut ipc, "pause", false);
    assert!(ipc.get("time-pos").unwrap().as_f64().unwrap() < 1.);
    let _ = ipc.call(json!(["quit", 0]));
    let begin = Instant::now();
    loop {
        match player
            .finished()
            .expect("Normal Quit must not become an IPC composition failure")
        {
            Some(success) => {
                assert!(success);
                break;
            }
            None => {
                assert!(begin.elapsed() < Duration::from_secs(5));
                thread::sleep(Duration::from_millis(10));
            }
        }
    }
    assert_eq!(
        fs::read_dir(store.root().join("exports")).unwrap().count(),
        0
    );
    println!(
        "Direct timeline active bitmap: real OS Play/Pause, Fullscreen/Escape and EOF/replay, plus normal Quit=>Some(true); own Xvfb only, no export or audio device"
    );
}
