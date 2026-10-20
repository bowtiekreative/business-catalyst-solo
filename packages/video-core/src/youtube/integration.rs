//! Generated local MP4 + fake Google transport only. No physical capture,
//! credentials, accounts, browser, external HTTP or real publishing.
use super::tests::{FakeHttp, MemoryKeychain, configured, services};
use super::*;
use crate::media;
use http::Response;
use std::{fs, io::Write, process::Command, sync::Arc, time::Duration};

pub(super) fn generated_mp4(path: &Path) {
    assert_eq!(
        std::env::var("STORYFOLD_CAPTURE_TEST").as_deref(),
        Ok("1"),
        "Explicit generated-only test opt-in required"
    );
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
        "-f",
        "lavfi",
        "-i",
        "color=c=0x7860b5:s=160x90:r=30:d=1",
        "-f",
        "lavfi",
        "-i",
        "sine=frequency=440:sample_rate=48000:duration=1",
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
    .arg(path);
    media::run_bounded(
        &mut c,
        Duration::from_secs(15),
        &Cancellation::default(),
        65536,
    )
    .unwrap();
}
#[test]
#[ignore = "explicit generated MP4 and fake resumable YouTube transport proof"]
fn generated_mp4_cancel_then_public_resume_preserves_media_and_reports_server_privacy() {
    let temp = tempfile::tempdir().unwrap();
    let (store, _) = ProjectStore::create(temp.path().join("project"), "Synthetic upload").unwrap();
    let path = store.root().join("exports/sequence.mp4");
    generated_mp4(&path);
    // A valid MP4 free box grows the opaque upload enough to exercise chunks.
    let mut file = fs::OpenOptions::new().append(true).open(&path).unwrap();
    let extra = 2 * protocol::CHUNK_BYTES + 17;
    file.write_all(&((extra + 8) as u32).to_be_bytes()).unwrap();
    file.write_all(b"free").unwrap();
    file.write_all(&vec![0; extra]).unwrap();
    drop(file);
    let original = fs::read(&path).unwrap();
    let project = fs::read(store.root().join("project.json")).unwrap();
    let memory = Arc::new(MemoryKeychain::default());
    let http = Arc::new(FakeHttp::default());
    let s = services(memory, http.clone(), temp.path());
    let record = configured();
    let connection = record.tokens.as_ref().unwrap().connection.clone();
    s.vault.save(&record).unwrap();
    let client = YoutubeClient {
        services: Arc::new(Mutex::new(s)),
    };
    http.replies.lock().unwrap().push_back(Ok(Response {
        status: 200,
        location: Some(
            "https://www.googleapis.com/upload/youtube/v3/videos?upload_id=synthetic".into(),
        ),
        range: None,
        body: vec![],
    }));
    http.push(308, Some("bytes=0-1048575"), "");
    let cancel = Cancellation::default();
    let stop = cancel.clone();
    let request = PublishRequest {
        relative_path: "exports/sequence.mp4".into(),
        title: "Synthetic generated upload".into(),
        description: "This fixture never contacts Google".into(),
        privacy: Privacy::Unlisted,
        reviewed_connection: connection.clone(),
    };
    assert!(
        client
            .publish(&store, request, &cancel, |p| if p.acknowledged_bytes > 0 {
                stop.cancel()
            })
            .is_err()
    );
    assert!(client.status().unwrap().pending.is_some());
    assert_eq!(http.requests.lock().unwrap().len(), 2);
    http.push(308, Some("bytes=0-1048575"), "");
    http.push(308, Some("bytes=0-2097151"), "");
    http.push(
        201,
        None,
        r#"{"id":"aB_cD-eF012","status":{"privacyStatus":"private"}}"#,
    );
    let mut progress = Vec::new();
    let done = client
        .resume(&store, &connection, &Cancellation::default(), |p| {
            progress.push(p)
        })
        .unwrap();
    assert_eq!(done.privacy, Privacy::Private);
    assert_eq!(
        progress.last().unwrap().acknowledged_bytes,
        original.len() as u64
    );
    let requests = http.requests.lock().unwrap();
    assert_eq!(requests.len(), 5);
    assert!(requests[2].body.is_empty());
    assert_eq!(
        [
            requests[1].body.as_slice(),
            requests[3].body.as_slice(),
            requests[4].body.as_slice()
        ]
        .concat(),
        original
    );
    assert_eq!(fs::read(&path).unwrap(), original);
    assert_eq!(
        fs::read(store.root().join("project.json")).unwrap(),
        project
    );
    assert!(client.status().unwrap().pending.is_none());
}

fn fake_downloader(temp: &Path, name: &str, body: &str) -> PathBuf {
    use std::os::unix::fs::PermissionsExt;
    let path = temp.join(name);
    fs::write(&path, format!("#!/usr/bin/python3\n{body}\n")).unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o700)).unwrap();
    path
}
#[test]
#[ignore = "explicit generated local MP4 and fake public downloader atomicity proof"]
fn generated_public_import_copies_exact_source_and_enforces_budget_cancel_and_process_group() {
    let temp = tempfile::tempdir().unwrap();
    let (store, studio) =
        ProjectStore::create(temp.path().join("project"), "Synthetic import").unwrap();
    let original_path = temp.path().join("generated.mp4");
    generated_mp4(&original_path);
    let original = fs::read(&original_path).unwrap();
    let source = serde_json::to_string(original_path.to_str().unwrap()).unwrap();
    let tool = fake_downloader(
        temp.path(),
        "fake-copy",
        &format!("import pathlib,shutil\nshutil.copyfile({source},pathlib.Path.cwd()/'clip.mp4')"),
    );
    let request = YoutubeImportRequest {
        url: "https://youtu.be/aB_cD-eF012".into(),
        start_ms: 0,
        end_ms: 1000,
    };
    let limits = ImportLimits {
        max_bytes: 1024 * 1024,
        timeout: Duration::from_secs(5),
    };
    let mut progress = Vec::new();
    let media = super::import::import_test_tool(
        &store,
        &request,
        limits,
        &Cancellation::default(),
        |p| progress.push(p),
        &tool,
    )
    .unwrap();
    assert_eq!(fs::read(store.root().join(media.path())).unwrap(), original);
    assert_eq!(fs::read(&original_path).unwrap(), original);
    assert_eq!(progress.last().unwrap().phase, YoutubeImportPhase::Complete);
    let mut editor = crate::Editor::new(studio).unwrap();
    media.append_to(&store, &mut editor).unwrap();
    assert_eq!(editor.studio().clips().len(), 1);
    let count = fs::read_dir(store.root().join("media")).unwrap().count();
    assert!(
        super::import::import_test_tool(
            &store,
            &request,
            ImportLimits {
                max_bytes: 1,
                ..limits
            },
            &Cancellation::default(),
            |_| {},
            &tool
        )
        .is_err()
    );
    assert_eq!(
        fs::read_dir(store.root().join("media")).unwrap().count(),
        count
    );
    let pid_path = temp.path().join("generated-child.pid");
    let pid_literal = serde_json::to_string(pid_path.to_str().unwrap()).unwrap();
    let sleeper = fake_downloader(
        temp.path(),
        "fake-group",
        &format!(
            "import pathlib,os,time\npid=os.fork()\nif pid==0:\n pathlib.Path({pid_literal}).write_text(str(os.getpid()))\n while True: time.sleep(1)\nelse:\n while not pathlib.Path({pid_literal}).exists(): time.sleep(0.01)\n pathlib.Path('started').write_text('generated')\n while True: time.sleep(1)"
        ),
    );
    let cancel = Cancellation::default();
    let stop = cancel.clone();
    let before = std::time::Instant::now();
    assert!(
        super::import::import_test_tool(
            &store,
            &request,
            limits,
            &cancel,
            |p| if p.written_bytes > 0 {
                stop.cancel()
            },
            &sleeper
        )
        .is_err()
    );
    assert!(before.elapsed() < Duration::from_secs(3));
    let pid: u32 = fs::read_to_string(pid_path).unwrap().parse().unwrap();
    let alive = fs::read_to_string(format!("/proc/{pid}/stat")).is_ok_and(|stat| {
        stat.rsplit_once(") ")
            .is_some_and(|(_, tail)| !tail.starts_with('Z'))
    });
    assert!(
        !alive,
        "generated downloader grandchild must not keep running"
    );
    assert_eq!(
        fs::read_dir(store.root().join("media")).unwrap().count(),
        count
    );
    assert_eq!(
        fs::read_dir(store.root().join(".sidebar/private"))
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| e
                .file_name()
                .to_string_lossy()
                .starts_with("youtube-import-"))
            .count(),
        0
    );
}
