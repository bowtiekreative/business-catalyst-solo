//! Query capabilities only: this never starts a camera or microphone stream.
fn main() -> video_core::Result<()> {
    let devices = video_core::capture::enumerate_devices()?;
    let cameras: Vec<_> = devices
        .cameras
        .iter()
        .map(|camera| {
            serde_json::json!({
                "device":camera.id,
                "label":camera.label,
                "modes":camera.modes,
            })
        })
        .collect();
    println!(
        "{}",
        serde_json::to_string_pretty(&serde_json::json!({
            "stream_started":false,"cameras":cameras,
            "microphone_count":devices.microphones.len(),"message":devices.message,
        }))?
    );
    Ok(())
}
