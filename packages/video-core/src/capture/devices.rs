use super::DeviceInventory;
#[cfg(any(target_os = "linux", test))]
use super::MicrophoneDevice;
#[cfg(target_os = "linux")]
use super::{CameraDevice, CameraFormat, CameraMode};
use crate::Result;
#[cfg(target_os = "linux")]
use crate::media::{Cancellation, run_bounded};
#[cfg(target_os = "linux")]
use std::{fs, process::Command, time::Duration};

pub fn enumerate_devices() -> Result<DeviceInventory> {
    let mut inventory = DeviceInventory {
        cameras: Vec::new(),
        microphones: Vec::new(),
        message: String::new(),
    };
    #[cfg(target_os = "linux")]
    {
        if let Ok(nodes) = fs::read_dir("/dev") {
            for node in nodes.flatten() {
                let id = node.path().to_string_lossy().to_string();
                if !valid_camera(&id) {
                    continue;
                }
                if let Ok(Some(camera)) = linux::query(&id) {
                    inventory.cameras.push(camera);
                }
            }
        }
        inventory.cameras.sort_by(|a, b| a.id.cmp(&b.id));
        if let Ok(output) = run_bounded(
            Command::new("pactl").args(["-f", "json", "list", "sources"]),
            Duration::from_secs(3),
            &Cancellation::default(),
            2 * 1024 * 1024,
        ) {
            inventory.microphones = parse_pulse(&output.stdout)?;
        }
        inventory.message=if inventory.cameras.is_empty(){"No camera detected. Connect a camera and refresh devices. Camera and microphone stay off until Start camera."}else{"Devices are queried without streaming. Start camera explicitly opens the selected camera and microphone."}.into();
    }
    #[cfg(not(target_os = "linux"))]
    {
        inventory.message =
            "Camera capture is available on Linux. Local video import remains available.".into();
    }
    Ok(inventory)
}
#[cfg(any(target_os = "linux", test))]
pub(crate) fn valid_camera(id: &str) -> bool {
    id.strip_prefix("/dev/video").is_some_and(|suffix| {
        !suffix.is_empty() && suffix.len() <= 8 && suffix.chars().all(|c| c.is_ascii_digit())
    })
}
#[cfg(any(target_os = "linux", test))]
pub(crate) fn valid_microphone(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 256
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.'))
}
#[cfg(any(target_os = "linux", test))]
fn parse_pulse(bytes: &[u8]) -> Result<Vec<MicrophoneDevice>> {
    let value: serde_json::Value =
        serde_json::from_slice(bytes).map_err(|_| "Microphone inventory is invalid")?;
    let sources = value.as_array().ok_or("Microphone inventory is invalid")?;
    let mut microphones = Vec::new();
    for source in sources.iter().take(256) {
        let Some(id) = source["name"].as_str().filter(|id| valid_microphone(id)) else {
            continue;
        };
        if id.ends_with(".monitor") {
            continue;
        }
        let label = source["description"]
            .as_str()
            .unwrap_or(id)
            .chars()
            .filter(|c| !c.is_control())
            .take(120)
            .collect();
        microphones.push(MicrophoneDevice {
            id: id.into(),
            label,
        });
    }
    Ok(microphones)
}

#[cfg(target_os = "linux")]
mod linux {
    use super::*;
    use std::{
        fs::OpenOptions,
        os::{fd::AsRawFd, unix::fs::OpenOptionsExt},
    };
    #[repr(C)]
    #[derive(Default)]
    struct Capability {
        driver: [u8; 16],
        card: [u8; 32],
        bus_info: [u8; 32],
        version: u32,
        capabilities: u32,
        device_caps: u32,
        reserved: [u32; 3],
    }
    #[repr(C)]
    #[derive(Default)]
    struct FrameInterval {
        index: u32,
        pixel_format: u32,
        width: u32,
        height: u32,
        kind: u32,
        interval: [u32; 6],
        reserved: [u32; 2],
    }
    pub(super) fn query(id: &str) -> Result<Option<CameraDevice>> {
        if !valid_camera(id) {
            return Err("Choose a detected camera".into());
        }
        let file = OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NONBLOCK)
            .open(id)?;
        let mut capability = Capability::default();
        // SAFETY: QUERYCAP initializes this kernel ABI-sized structure; it never
        // queues a buffer or starts streaming. The live file owns the descriptor.
        if unsafe {
            libc::ioctl(
                file.as_raw_fd(),
                libc::_IOR::<Capability>(b'V' as u32, 0),
                &mut capability,
            )
        } < 0
        {
            return Err(std::io::Error::last_os_error().into());
        }
        let flags = if capability.capabilities & 0x8000_0000 != 0 {
            capability.device_caps
        } else {
            capability.capabilities
        };
        if flags & (0x1 | 0x1000) == 0 {
            return Ok(None);
        }
        let end = capability
            .card
            .iter()
            .position(|value| *value == 0)
            .unwrap_or(32);
        let label = String::from_utf8_lossy(&capability.card[..end])
            .chars()
            .filter(|c| !c.is_control())
            .collect();
        let mut modes = Vec::new();
        for (format, fourcc) in [
            (CameraFormat::Mjpeg, *b"MJPG"),
            (CameraFormat::Yuyv422, *b"YUYV"),
            (CameraFormat::Nv12, *b"NV12"),
        ] {
            for (width, height) in [(1920, 1080), (1280, 720), (640, 480), (640, 360)] {
                for index in 0..64 {
                    let mut interval = FrameInterval {
                        index,
                        pixel_format: u32::from_le_bytes(fourcc),
                        width,
                        height,
                        ..Default::default()
                    };
                    // SAFETY: ENUM_FRAMEINTERVALS only queries this initialized
                    // structure; no camera format is changed or stream started.
                    if unsafe {
                        libc::ioctl(
                            file.as_raw_fd(),
                            libc::_IOWR::<FrameInterval>(b'V' as u32, 75),
                            &mut interval,
                        )
                    } < 0
                    {
                        break;
                    }
                    let [numerator, denominator, ..] = interval.interval;
                    if interval.kind == 1
                        && numerator > 0
                        && denominator > 0
                        && numerator.checked_mul(30) == Some(denominator)
                    {
                        let mode = CameraMode {
                            width,
                            height,
                            fps: 30,
                            format,
                        };
                        if !modes.contains(&mode) {
                            modes.push(mode);
                        }
                    }
                    if interval.kind != 1 {
                        break;
                    }
                }
            }
        }
        // No unqueried "default" fallback: an unsupported/slow mode must not be
        // presented as a clear 30fps recording source.
        if modes.is_empty() {
            return Ok(None);
        }
        modes.sort_by_key(|mode| {
            std::cmp::Reverse((mode.width * mode.height, mode.format == CameraFormat::Mjpeg))
        });
        Ok(Some(CameraDevice {
            id: id.into(),
            label,
            modes,
        }))
    }
    #[cfg(test)]
    mod tests {
        use super::*;
        #[test]
        fn v4l2_query_structs_match_kernel_abi() {
            assert_eq!(std::mem::size_of::<Capability>(), 104);
            assert_eq!(std::mem::offset_of!(Capability, device_caps), 88);
            assert_eq!(std::mem::size_of::<FrameInterval>(), 52);
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn only_local_video_nodes_and_pulse_sources_are_accepted() {
        for bad in [
            "/dev/video",
            "/dev/video0/../x",
            "http://camera",
            "/dev/video1;cmd",
        ] {
            assert!(!valid_camera(bad));
        }
        assert!(valid_camera("/dev/video12"));
        let sources=parse_pulse(br#"[{"name":"alsa_input.usb-device","description":"Mic"},{"name":"speaker.monitor"},{"name":"bad source"}]"#).unwrap();
        assert_eq!(sources.len(), 1);
        assert_eq!(sources[0].id, "alsa_input.usb-device");
    }
}
