//! Explicit local camera check: no microphone, takes, image viewing or retained buffer.
use std::time::{Duration, Instant};
use video_core::{
    ProjectStore,
    capture::{self, CapturePhase, CaptureSession, CaptureSettings},
};

fn main() -> video_core::Result<()> {
    if std::env::var("STORYFOLD_CAMERA_STARTUP_CHECK").as_deref() != Ok("1") {
        return Err("Set STORYFOLD_CAMERA_STARTUP_CHECK=1 for this explicit camera check".into());
    }
    let id = std::env::args()
        .nth(1)
        .ok_or("Pass one detected /dev/video camera")?;
    let inventory = capture::enumerate_devices()?;
    let camera = inventory
        .cameras
        .iter()
        .find(|camera| camera.id == id)
        .ok_or("Camera is not in Storyfold's supported inventory")?;
    let mode = *camera.modes.first().ok_or("Camera has no supported mode")?;
    let temporary = tempfile::Builder::new()
        .prefix("storyfold-camera-check-")
        .tempdir()?;
    let (store, _) =
        ProjectStore::create(temporary.path().join("project"), "Camera startup check")?;
    let started = Instant::now();
    let session = CaptureSession::start(
        &store,
        CaptureSettings {
            camera_id: id.clone(),
            microphone_id: None,
            mode,
            ..CaptureSettings::default()
        },
    )?;
    let mut first = None;
    let mut latest = None;
    let mut error = None;
    while started.elapsed() < Duration::from_secs(12) {
        for event in session.poll_events() {
            if let capture::CaptureEvent::Error(message) = event {
                error = Some(message);
            }
        }
        if let Some(frame) = session.latest_preview() {
            first.get_or_insert_with(|| started.elapsed().as_millis());
            latest = Some((frame.sequence, frame.width, frame.height));
            if frame.sequence >= 60 {
                break;
            }
        }
        if session.status().phase == CapturePhase::Failed {
            break;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    session.stop()?;
    let stopping = Instant::now();
    while !session.status().can_leave() && stopping.elapsed() < Duration::from_secs(8) {
        std::thread::sleep(Duration::from_millis(20));
    }
    let stopped = session.status().can_leave();
    drop(session);
    drop(store);
    temporary.close()?;
    println!(
        "{}",
        serde_json::to_string_pretty(&serde_json::json!({
            "camera":id,"mode":mode,"microphone":"off","takes_created":0,
            "first_preview_ms":first,"last_frame":latest,"error":error,
            "stopped":stopped,"temporary_buffer_removed":true,
        }))?
    );
    if first.is_none() || error.is_some() || !stopped {
        return Err("Camera startup check failed; see the bounded status above".into());
    }
    Ok(())
}
