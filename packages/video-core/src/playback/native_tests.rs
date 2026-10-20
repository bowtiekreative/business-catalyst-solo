//! Generated-only OS X11/XTest check. IPC observes state; it never injects input.
use super::*;
use serde_json::{Value, json};
use std::{
    io::{BufRead, BufReader, Write},
    os::unix::net::UnixStream,
    time::{Duration, Instant},
};

struct Ipc {
    writer: UnixStream,
    reader: BufReader<UnixStream>,
    sequence: u64,
}
impl Ipc {
    fn get(&mut self, name: &str) -> Value {
        assert!(["pause", "time-pos", "fullscreen", "eof-reached"].contains(&name));
        self.sequence += 1;
        writeln!(
            self.writer,
            "{}",
            json!({"command":["get_property",name],"request_id":self.sequence})
        )
        .unwrap();
        let started = Instant::now();
        for _ in 0..100 {
            assert!(
                started.elapsed() < Duration::from_secs(4),
                "Player observation timed out"
            );
            let mut line = String::new();
            assert!(self.reader.read_line(&mut line).unwrap() > 0);
            let response: Value = serde_json::from_str(&line).unwrap();
            if response["request_id"].as_u64() == Some(self.sequence) {
                if response["error"] == "property unavailable" {
                    return Value::Null;
                }
                assert_eq!(response["error"], "success", "{response}");
                return response["data"].clone();
            }
        }
        panic!("The generated player's IPC did not answer");
    }
    fn until(&mut self, name: &str, matches: impl Fn(&Value) -> bool, timeout: Duration) -> Value {
        let started = Instant::now();
        loop {
            let value = self.get(name);
            if matches(&value) {
                return value;
            }
            assert!(
                started.elapsed() < timeout,
                "Player property {name} did not reach the expected state: {value}"
            );
            std::thread::sleep(Duration::from_millis(30));
        }
    }
}

struct NativeInput {
    player_pid: u32,
}
impl NativeInput {
    fn action(&self, action: &str) {
        assert!(["play-pause", "fullscreen", "escape"].contains(&action));
        let helper = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../scripts/test-native-player-input.py");
        let mut child = Command::new("python3")
            .arg("-I")
            .arg(helper)
            .args(["--pid", &self.player_pid.to_string(), "--action", action])
            .stdin(Stdio::null())
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit())
            .spawn()
            .expect("Start the isolated X11/XTest fixture helper");
        let started = Instant::now();
        loop {
            if let Some(status) = child.try_wait().unwrap() {
                assert!(status.success(), "Owned Xvfb player input failed");
                break;
            }
            if started.elapsed() > Duration::from_secs(10) {
                let _ = child.kill();
                let _ = child.wait();
                panic!("Owned Xvfb player input timed out");
            }
            std::thread::sleep(Duration::from_millis(20));
        }
    }
}
#[test]
#[ignore = "Generated OS X11/XTest input check; requires STORYFOLD_INPUT_FIXTURE=1 inside fresh xvfb-run"]
fn generated_native_player_mouse_play_pause_fullscreen_and_replay() {
    assert_eq!(std::env::var("STORYFOLD_INPUT_FIXTURE").as_deref(), Ok("1"));
    assert!(std::env::var_os("DISPLAY").is_some());
    assert!(
        std::env::var("XAUTHORITY").is_ok_and(|s| s.contains("/xvfb-run.")),
        "Use fresh xvfb-run, never the desktop display"
    );
    let temp = tempfile::Builder::new()
        .prefix("sf-player-")
        .tempdir()
        .unwrap();
    let (store, _) = ProjectStore::create(temp.path().join("project"), "Generated player").unwrap();
    let path = store.root().join("exports/generated.mp4");
    assert!(
        Command::new(media::tool("ffmpeg").unwrap())
            .args([
                "-hide_banner",
                "-loglevel",
                "error",
                "-f",
                "lavfi",
                "-i",
                "testsrc2=size=640x360:rate=30",
                "-t",
                "5",
                "-an",
                "-c:v",
                "libx264",
                "-threads",
                "1",
                "-preset",
                "ultrafast",
                "-pix_fmt",
                "yuv420p"
            ])
            .arg(&path)
            .status()
            .unwrap()
            .success()
    );
    let original = std::fs::read(&path).unwrap();
    let socket = temp.path().join("control.sock");
    let (mut command, controls) = player_command(&store, true).unwrap();
    command.arg(format!("--input-ipc-server={}", socket.display()));
    command.args([
        "--pause=yes",
        "--start=0",
        "--title=Storyfold generated native input fixture",
    ]);
    command
        .env_remove("WAYLAND_DISPLAY")
        .env_remove("WAYLAND_SOCKET")
        .env("XDG_SESSION_TYPE", "x11");
    let mut player = SequencePlayer {
        _controls: controls,
        child: command
            .arg("--")
            .arg(&path)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap(),
    };
    let input = NativeInput {
        player_pid: player.child.id(),
    };
    let started = Instant::now();
    let stream = loop {
        if let Ok(stream) = UnixStream::connect(&socket) {
            break stream;
        }
        assert!(
            player.finished().unwrap().is_none(),
            "Player exited during startup"
        );
        assert!(started.elapsed() < Duration::from_secs(10));
        std::thread::sleep(Duration::from_millis(25));
    };
    stream
        .set_read_timeout(Some(Duration::from_secs(3)))
        .unwrap();
    stream
        .set_write_timeout(Some(Duration::from_secs(3)))
        .unwrap();
    let mut ipc = Ipc {
        reader: BufReader::new(stream.try_clone().unwrap()),
        writer: stream,
        sequence: 0,
    };
    ipc.until("time-pos", Value::is_number, Duration::from_secs(10));
    assert_eq!(ipc.get("pause"), true);
    // The helper locates only this child PID/title on its fresh Xvfb display,
    // then clicks the installed mpv bottombar through the OS input pipeline.
    input.action("play-pause");
    ipc.until(
        "pause",
        |v| v.as_bool() == Some(false),
        Duration::from_secs(2),
    );
    std::thread::sleep(Duration::from_millis(250));
    assert!(ipc.get("time-pos").as_f64().unwrap() > 0.1);
    input.action("play-pause");
    ipc.until(
        "pause",
        |v| v.as_bool() == Some(true),
        Duration::from_secs(2),
    );
    let paused_at = ipc.get("time-pos").as_f64().unwrap();
    std::thread::sleep(Duration::from_millis(200));
    assert!((ipc.get("time-pos").as_f64().unwrap() - paused_at).abs() < 0.05);
    input.action("fullscreen");
    ipc.until(
        "fullscreen",
        |v| v.as_bool() == Some(true),
        Duration::from_secs(2),
    );
    input.action("fullscreen");
    ipc.until(
        "fullscreen",
        |v| v.as_bool() == Some(false),
        Duration::from_secs(2),
    );
    input.action("fullscreen");
    ipc.until(
        "fullscreen",
        |v| v.as_bool() == Some(true),
        Duration::from_secs(2),
    );
    input.action("escape");
    ipc.until(
        "fullscreen",
        |v| v.as_bool() == Some(false),
        Duration::from_secs(2),
    );
    assert_eq!(ipc.get("pause"), true);
    input.action("play-pause");
    ipc.until(
        "eof-reached",
        |v| v.as_bool() == Some(true),
        Duration::from_secs(8),
    );
    ipc.until(
        "pause",
        |v| v.as_bool() == Some(true),
        Duration::from_secs(2),
    );
    assert!(
        player.finished().unwrap().is_none(),
        "Preview must stay open at the end"
    );
    input.action("play-pause");
    ipc.until(
        "pause",
        |v| v.as_bool() == Some(false),
        Duration::from_secs(2),
    );
    ipc.until(
        "time-pos",
        |v| v.as_f64().is_some_and(|t| t > 0.05 && t < 2.),
        Duration::from_secs(2),
    );
    assert_eq!(std::fs::read(&path).unwrap(), original);
    println!(
        "Real OS X11/XTest controls on owned Xvfb: Play/Pause, clock pause, Fullscreen, Escape, natural EOF/replay; IPC observation only; original MP4 unchanged"
    );
}
