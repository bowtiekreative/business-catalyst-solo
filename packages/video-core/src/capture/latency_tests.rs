//! Opt-in generated clock benchmark; never opens a physical device.
use super::*;
use std::{
    io::Write,
    process::{Child, Command, Stdio},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
struct Load(Child);
impl Drop for Load {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}
fn wait(session: &CaptureSession, ready: impl Fn(&CaptureStatus) -> bool) {
    let started = Instant::now();
    while !ready(&session.status()) {
        assert!(
            started.elapsed() < Duration::from_secs(15),
            "{:?}",
            session.status()
        );
        std::thread::sleep(Duration::from_millis(20));
    }
}
#[test]
#[ignore = "Generated HD clock benchmark; STORYFOLD_CAPTURE_TEST=1 and STORYFOLD_CAPTURE_CLOCK_TEST=1"]
fn generated_hd_preview_clock_age_and_freshness_under_encode_load() {
    assert_eq!(std::env::var("STORYFOLD_CAPTURE_TEST").as_deref(), Ok("1"));
    assert_eq!(
        std::env::var("STORYFOLD_CAPTURE_CLOCK_TEST").as_deref(),
        Ok("1")
    );
    let temp = tempfile::tempdir().unwrap();
    let (store, _) =
        ProjectStore::create(temp.path().join("project"), "Generated latency").unwrap();
    let load = Load(
        Command::new(crate::media::tool("ffmpeg").unwrap())
            .args([
                "-hide_banner",
                "-loglevel",
                "error",
                "-nostdin",
                "-f",
                "lavfi",
                "-i",
                "testsrc2=size=1920x1080:rate=30",
                "-an",
                "-c:v",
                "libx264",
                "-threads",
                "2",
                "-preset",
                "veryfast",
                "-f",
                "null",
                "-",
            ])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap(),
    );
    let session = CaptureSession::start_generated(
        &store,
        CaptureSettings {
            microphone_id: Some("generated-tone".into()),
            ..Default::default()
        },
    )
    .unwrap();
    wait(&session, |s| s.phase == CapturePhase::Ready);
    std::thread::sleep(Duration::from_millis(500));
    let started = Instant::now();
    let mut samples = Vec::new();
    for _ in 0..24 {
        let frame = session.latest_preview().unwrap();
        assert_eq!((frame.width, frame.height), (1280, 720));
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs_f64();
        samples.push((now, frame));
        std::thread::sleep(Duration::from_millis(250));
    }
    let elapsed = started.elapsed().as_secs_f64();
    session.stop().unwrap();
    wait(&session, |s| s.can_leave());
    drop(load);
    let fps = (samples.last().unwrap().1.sequence - samples.first().unwrap().1.sequence) as f64
        / (elapsed - 0.25);
    let mut ages = Vec::new();
    for (received, frame) in &samples {
        let rgba =
            image::RgbaImage::from_raw(frame.width, frame.height, frame.rgba.clone()).unwrap();
        let crop = image::imageops::crop_imm(&rgba, 0, 0, 740, 100).to_image();
        let mut png = std::io::Cursor::new(Vec::new());
        image::DynamicImage::ImageRgba8(crop)
            .write_to(&mut png, image::ImageFormat::Png)
            .unwrap();
        let mut child = Command::new(crate::media::tool("tesseract").unwrap())
            .args([
                "stdin",
                "stdout",
                "--psm",
                "7",
                "-c",
                "tessedit_char_whitelist=0123456789.",
            ])
            .env("OMP_THREAD_LIMIT", "1")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        child
            .stdin
            .take()
            .unwrap()
            .write_all(png.get_ref())
            .unwrap();
        let result = child.wait_with_output().unwrap();
        assert!(result.status.success());
        if let Ok(source) = String::from_utf8(result.stdout)
            .unwrap()
            .trim()
            .parse::<f64>()
        {
            let age = (received - source) * 1000.;
            if (-25.0..10000.).contains(&age) {
                ages.push(age);
            }
        }
    }
    assert!(
        ages.len() >= 22,
        "Generated clock OCR did not yield enough valid samples"
    );
    ages.sort_by(f64::total_cmp);
    let median = ages[ages.len() / 2];
    let p95 = ages[(ages.len() * 95 / 100).min(ages.len() - 1)];
    let proof = serde_json::json!({"syntheticOnly":true,"component":"Generated filter clock to latest native RGBA frame; excludes sensor/USB and GPU painting","competingLoad":"1920x1080 x264 veryfast, two threads","samples":ages.len(),"preview":"1280x720;1920x1080@30 capture unchanged","fps":fps,"ageMs":{"min":ages[0],"median":median,"p95":p95,"max":ages.last().unwrap()},"videoInputQueuePackets":2,"pulseInputQueuePackets":8,"physicalCameraOpened":false});
    if let Some(path) = std::env::var_os("STORYFOLD_CAPTURE_LATENCY_PROOF") {
        std::fs::write(path, serde_json::to_vec_pretty(&proof).unwrap()).unwrap();
    }
    println!("{proof}");
    assert!(fps >= 24., "Generated preview is not keeping up");
    assert!(
        median < 250. && p95 < 350.,
        "Generated software preview age exceeds the benchmark budget"
    );
}
