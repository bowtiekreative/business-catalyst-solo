use super::DeviceInventory;
#[cfg(any(target_os = "linux", test))]
use super::MicrophoneDevice;
#[cfg(target_os = "linux")]
use super::{CameraDevice, CameraFormat, CameraMode};
use crate::Result;
#[cfg(target_os = "linux")]
use crate::media::{Cancellation, run_bounded};
#[cfg(target_os = "linux")]
pub(super) use linux::{FrameRateGuard, keep_frame_rate};
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
        let mut microphones = match run_bounded(
            Command::new("pactl").args(["-f", "json", "list", "sources"]),
            Duration::from_secs(3),
            &Cancellation::default(),
            2 * 1024 * 1024,
        ) {
            Ok(output) => Ok(parse_pulse(&output.stdout)?),
            Err(error) => Err(error),
        };
        if let Ok(microphones) = &mut microphones
            && !microphones.is_empty()
            && let Ok(default) = run_bounded(
                Command::new("pactl").arg("get-default-source"),
                Duration::from_secs(3),
                &Cancellation::default(),
                4096,
            )
        {
            prefer_default_microphone(microphones, &default.stdout);
        }
        inventory.message=if inventory.cameras.is_empty(){"No camera detected. Connect a camera and refresh devices. Camera and microphone stay off until Start camera."}else{"Devices are queried without streaming. Start camera explicitly opens the selected camera and microphone."}.into();
        apply_microphone_inventory(&mut inventory, microphones);
    }
    #[cfg(not(target_os = "linux"))]
    {
        inventory.message =
            "Camera capture is available on Linux. Local video import remains available.".into();
    }
    Ok(inventory)
}
#[cfg(any(target_os = "linux", test))]
fn apply_microphone_inventory(
    inventory: &mut DeviceInventory,
    microphones: Result<Vec<MicrophoneDevice>>,
) {
    match microphones {
        Ok(microphones) => inventory.microphones = microphones,
        Err(_) => inventory.message.push_str(
            " Microphone discovery is unavailable. Check PulseAudio/PipeWire and pulseaudio-utils, then refresh devices. Recording without a microphone is silent.",
        ),
    }
}
#[cfg(any(target_os = "linux", test))]
fn prefer_default_microphone(microphones: &mut [MicrophoneDevice], source: &[u8]) {
    let Ok(source) = std::str::from_utf8(source) else {
        return;
    };
    let source = source.trim();
    if valid_microphone(source)
        && let Some(index) = microphones
            .iter()
            .position(|microphone| microphone.id == source)
    {
        microphones[..=index].rotate_right(1);
    }
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
        fs::{File, OpenOptions},
        io,
        os::{fd::AsRawFd, unix::fs::OpenOptionsExt},
    };
    const EXPOSURE_AUTO_PRIORITY: u32 = 0x009a_0903;
    #[repr(C)]
    struct Control {
        id: u32,
        value: i32,
    }

    /// Holds the camera descriptor until capture has stopped. Restoration is
    /// best effort and never overwrites a subsequently changed control value.
    pub(crate) struct FrameRateGuard {
        file: File,
    }
    pub(crate) fn keep_frame_rate(id: &str) -> Option<FrameRateGuard> {
        if !valid_camera(id) {
            return None;
        }
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .custom_flags(libc::O_NONBLOCK)
            .open(id)
            .ok()?;
        if prefer_frame_rate(|value| exposure_priority(&file, value)) {
            Some(FrameRateGuard { file })
        } else {
            None
        }
    }
    impl Drop for FrameRateGuard {
        fn drop(&mut self) {
            restore_frame_rate(|value| exposure_priority(&self.file, value));
            // File closes after this method returns, including restore failure.
        }
    }
    fn exposure_priority(file: &File, value: Option<i32>) -> io::Result<i32> {
        let mut control = Control {
            id: EXPOSURE_AUTO_PRIORITY,
            value: value.unwrap_or_default(),
        };
        let request = if value.is_some() {
            libc::_IOWR::<Control>(b'V' as u32, 28)
        } else {
            libc::_IOWR::<Control>(b'V' as u32, 27)
        };
        // SAFETY: this initialized 8-byte v4l2_control matches the kernel ABI.
        // The live File owns the descriptor. G_CTRL/S_CTRL only access this
        // one boolean control; neither queues buffers nor starts streaming.
        if unsafe { libc::ioctl(file.as_raw_fd(), request, &mut control) } < 0 {
            Err(io::Error::last_os_error())
        } else {
            Ok(control.value)
        }
    }
    // A tiny injectable control operation keeps the real ioctl and restoration
    // decision on one path while tests never open physical camera hardware.
    fn prefer_frame_rate(mut control: impl FnMut(Option<i32>) -> io::Result<i32>) -> bool {
        matches!(control(None), Ok(1)) && control(Some(0)).is_ok()
    }
    fn restore_frame_rate(mut control: impl FnMut(Option<i32>) -> io::Result<i32>) {
        if matches!(control(None), Ok(0)) {
            let _ = control(Some(1));
        }
    }
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
            assert_eq!(std::mem::size_of::<Control>(), 8);
            assert_eq!(std::mem::offset_of!(Control, id), 0);
            assert_eq!(std::mem::offset_of!(Control, value), 4);
            assert_eq!(EXPOSURE_AUTO_PRIORITY, 0x009a_0903);
            #[cfg(any(target_arch = "x86_64", target_arch = "x86", target_arch = "aarch64"))]
            {
                assert_eq!(libc::_IOWR::<Control>(b'V' as u32, 27), 0xc008_561b);
                assert_eq!(libc::_IOWR::<Control>(b'V' as u32, 28), 0xc008_561c);
            }
        }
        #[test]
        fn frame_rate_priority_is_temporarily_changed_and_restored() {
            let mut value = 1;
            let mut operations = Vec::new();
            let mut control = |requested| {
                operations.push(requested);
                if let Some(requested) = requested {
                    value = requested;
                }
                Ok(value)
            };
            assert!(prefer_frame_rate(&mut control));
            restore_frame_rate(&mut control);
            assert_eq!(value, 1);
            assert_eq!(operations, [None, Some(0), None, Some(1)]);
        }
        #[test]
        fn existing_frame_rate_priority_and_unsupported_controls_are_untouched() {
            for original in [0, 2] {
                let mut writes = Vec::new();
                assert!(!prefer_frame_rate(|requested| {
                    if let Some(requested) = requested {
                        writes.push(requested);
                    }
                    Ok(original)
                }));
                assert!(writes.is_empty());
            }
            let mut reads = 0;
            assert!(!prefer_frame_rate(|requested| {
                assert!(requested.is_none());
                reads += 1;
                Err(io::Error::from_raw_os_error(libc::EINVAL))
            }));
            assert_eq!(reads, 1);
            assert!(!prefer_frame_rate(|requested| {
                if requested.is_none() {
                    Ok(1)
                } else {
                    Err(io::Error::from_raw_os_error(libc::EACCES))
                }
            }));
        }
        #[test]
        fn restoration_preserves_external_change_and_ignores_ioctl_errors() {
            for current in [1, 2] {
                restore_frame_rate(|requested| {
                    assert!(requested.is_none(), "Do not overwrite external change");
                    Ok(current)
                });
            }
            restore_frame_rate(|requested| {
                assert!(requested.is_none(), "Do not write after failed query");
                Err(io::Error::from_raw_os_error(libc::ENODEV))
            });
            let mut operations = Vec::new();
            restore_frame_rate(|requested| {
                operations.push(requested);
                if requested.is_none() {
                    Ok(0)
                } else {
                    Err(io::Error::from_raw_os_error(libc::ENODEV))
                }
            });
            assert_eq!(operations, [None, Some(1)]);
        }
        #[test]
        fn frame_rate_guard_rejects_non_camera_ids_before_opening() {
            for id in [
                "",
                "/dev/video",
                "/dev/video0/../video1",
                "/tmp/video0",
                "https://camera",
            ] {
                assert!(keep_frame_rate(id).is_none());
            }
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn preferred_source_only_reorders_detected_non_monitor_microphones() {
        let mut microphones = parse_pulse(
            br#"[{"name":"first"},{"name":"second"},{"name":"preferred"},{"name":"speaker.monitor"}]"#,
        )
        .unwrap();
        prefer_default_microphone(&mut microphones, b"preferred\n");
        let ids = |microphones: &[MicrophoneDevice]| {
            microphones
                .iter()
                .map(|microphone| microphone.id.clone())
                .collect::<Vec<_>>()
        };
        assert_eq!(ids(&microphones), ["preferred", "first", "second"]);
        for unavailable in [
            b"speaker.monitor".as_slice(),
            b"missing",
            b"first\nsecond",
            b"",
            &[0xff],
        ] {
            prefer_default_microphone(&mut microphones, unavailable);
            assert_eq!(ids(&microphones), ["preferred", "first", "second"]);
        }
    }
    #[test]
    fn microphone_discovery_failure_is_visible_and_keeps_camera_inventory() {
        let mut inventory = DeviceInventory {
            cameras: vec![super::super::CameraDevice {
                id: "/dev/video2".into(),
                label: "Generated inventory camera; never opened".into(),
                modes: Vec::new(),
            }],
            microphones: Vec::new(),
            message: "Devices queried without streaming.".into(),
        };
        apply_microphone_inventory(&mut inventory, Err("Media tool could not start".into()));
        assert!(
            inventory
                .message
                .contains("Microphone discovery is unavailable"),
            "A missing or failed pactl must not silently hide every microphone"
        );
        assert!(inventory.message.contains("pulseaudio-utils"));
        assert!(inventory.message.contains("silent"));
        assert!(inventory.microphones.is_empty());
        assert_eq!(inventory.cameras.len(), 1);
        assert_eq!(inventory.cameras[0].id, "/dev/video2");
    }
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
