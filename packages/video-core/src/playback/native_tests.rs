//! Generated-only check of the actual mpv on-screen mouse bindings.
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
    fn command(&mut self, command: Value) -> Value {
        self.sequence += 1;
        writeln!(
            self.writer,
            "{}",
            json!({"command":command,"request_id":self.sequence})
        )
        .unwrap();
        for _ in 0..100 {
            let mut line = String::new();
            assert!(self.reader.read_line(&mut line).unwrap() > 0);
            let response: Value = serde_json::from_str(&line).unwrap();
            if response["request_id"].as_u64() == Some(self.sequence) {
                assert_eq!(response["error"], "success", "{response}");
                return response["data"].clone();
            }
        }
        panic!("The generated player's IPC did not answer");
    }
    fn get(&mut self, name: &str) -> Value {
        self.command(json!(["get_property", name]))
    }
    fn click(&mut self, x: i64, y: i64) {
        self.command(json!(["mouse", x, y]));
        std::thread::sleep(Duration::from_millis(60));
        self.command(json!(["keydown", "MBTN_LEFT"]));
        std::thread::sleep(Duration::from_millis(60));
        self.command(json!(["keyup", "MBTN_LEFT"]));
        std::thread::sleep(Duration::from_millis(120));
    }
    fn controller_button(&mut self, fullscreen: bool) {
        let dimensions = self.get("osd-dimensions");
        let x = if fullscreen {
            dimensions["w"].as_i64().unwrap() - 14
        } else {
            14
        };
        self.click(x, dimensions["h"].as_i64().unwrap() - 10);
    }
}
#[test]
#[ignore = "Generated native player mouse-input check; run with an isolated Xvfb display"]
fn generated_native_player_mouse_play_pause_fullscreen_and_replay() {
    assert!(std::env::var_os("DISPLAY").is_some());
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
    let dimensions = loop {
        let dimensions = ipc.get("osd-dimensions");
        if dimensions["w"].as_i64().unwrap_or(0) > 0 {
            break dimensions;
        }
        assert!(started.elapsed() < Duration::from_secs(15));
        std::thread::sleep(Duration::from_millis(25));
    };
    assert!(dimensions["h"].as_i64().unwrap() > 0);
    assert!(
        ipc.get("input-bindings")
            .as_array()
            .unwrap()
            .iter()
            .any(|b| b.to_string().contains("osc/")),
        "Native controller did not load"
    );
    ipc.command(json!(["seek", 0, "absolute+exact"]));
    ipc.command(json!(["set_property", "pause", true]));
    // Fixed bottombar layout from the installed mpv0.37; these go through its
    // mouse input and Lua controller, not direct pause/fullscreen assignments.
    ipc.controller_button(false);
    assert_eq!(ipc.get("pause"), false);
    std::thread::sleep(Duration::from_millis(250));
    assert!(ipc.get("time-pos").as_f64().unwrap() > 0.1);
    ipc.controller_button(false);
    assert_eq!(ipc.get("pause"), true);
    let paused_at = ipc.get("time-pos").as_f64().unwrap();
    std::thread::sleep(Duration::from_millis(200));
    assert!((ipc.get("time-pos").as_f64().unwrap() - paused_at).abs() < 0.05);
    ipc.controller_button(true);
    assert_eq!(ipc.get("fullscreen"), true);
    ipc.controller_button(true);
    assert_eq!(ipc.get("fullscreen"), false);
    ipc.controller_button(true);
    assert_eq!(ipc.get("fullscreen"), true);
    ipc.command(json!(["keypress", "ESC"]));
    std::thread::sleep(Duration::from_millis(150));
    assert_eq!(ipc.get("fullscreen"), false);
    ipc.command(json!(["seek", 5, "absolute+exact"]));
    ipc.command(json!(["set_property", "pause", false]));
    std::thread::sleep(Duration::from_millis(400));
    assert_eq!(ipc.get("eof-reached"), true);
    assert!(
        player.finished().unwrap().is_none(),
        "Preview must stay open at the end"
    );
    ipc.controller_button(false);
    assert_eq!(ipc.get("pause"), false);
    assert!(ipc.get("time-pos").as_f64().unwrap() < 2.);
    assert_eq!(std::fs::read(&path).unwrap(), original);
    println!(
        "Native visible controls: Play/Pause, real clock pause, Fullscreen, Escape, EOF replay; original MP4 unchanged"
    );
}
