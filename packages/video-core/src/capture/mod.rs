//! Continuous Linux camera session. Starting is explicit; tests use a separately
//! feature/environment-gated generated source, never a physical device.
#[cfg(target_os = "linux")]
mod buffer;
mod devices;
#[cfg(target_os = "linux")]
mod engine;
#[cfg(target_os = "linux")]
mod gesture;
#[cfg(all(test, target_os = "linux", feature = "capture-test"))]
mod latency_tests;
#[cfg(not(target_os = "linux"))]
mod unsupported;

use crate::{
    ClipId, Edit, Editor, OverlayContent, ProjectStore, Result, SlotKey, media::ImportedMedia,
};
pub use devices::enumerate_devices;
#[cfg(target_os = "linux")]
pub use engine::{CaptureSession, list_recoverable, recover_session};
use serde::{Deserialize, Serialize};
#[cfg(not(target_os = "linux"))]
pub use unsupported::{CaptureSession, list_recoverable, recover_session};

pub const FRAME_RATE: u64 = 30;
pub const PREVIEW_WIDTH: u32 = 1280;
pub const PREVIEW_HEIGHT: u32 = 720;
#[cfg(target_os = "linux")]
pub(crate) fn frame_ms(frame: u64) -> u64 {
    ((u128::from(frame) * 1000 + 15) / 30).min(u128::from(u64::MAX)) as u64
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CameraFormat {
    Mjpeg,
    Yuyv422,
    Nv12,
}
impl CameraFormat {
    #[cfg(target_os = "linux")]
    pub(crate) fn argument(self) -> &'static str {
        match self {
            Self::Mjpeg => "mjpeg",
            Self::Yuyv422 => "yuyv422",
            Self::Nv12 => "nv12",
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CameraMode {
    pub width: u32,
    pub height: u32,
    pub fps: u32,
    pub format: CameraFormat,
}
impl Default for CameraMode {
    fn default() -> Self {
        Self {
            width: 1920,
            height: 1080,
            fps: 30,
            format: CameraFormat::Mjpeg,
        }
    }
}
#[derive(Clone, Debug)]
pub struct CameraDevice {
    pub id: String,
    pub label: String,
    pub modes: Vec<CameraMode>,
}
#[derive(Clone, Debug)]
pub struct MicrophoneDevice {
    pub id: String,
    pub label: String,
}
#[derive(Clone, Debug)]
pub struct DeviceInventory {
    pub cameras: Vec<CameraDevice>,
    pub microphones: Vec<MicrophoneDevice>,
    pub message: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CaptureSettings {
    pub camera_id: String,
    pub microphone_id: Option<String>,
    pub mode: CameraMode,
    pub tap_threshold_ms: u64,
}
impl Default for CaptureSettings {
    fn default() -> Self {
        Self {
            camera_id: String::new(),
            microphone_id: None,
            mode: CameraMode::default(),
            tap_threshold_ms: 350,
        }
    }
}
impl CaptureSettings {
    #[cfg(target_os = "linux")]
    pub(crate) fn validate(&self, generated: bool) -> Result<()> {
        if self.mode.fps != 30
            || !matches!(
                (self.mode.width, self.mode.height),
                (1920, 1080) | (1280, 720) | (640, 480) | (640, 360)
            )
            || self.tap_threshold_ms < 50
            || self.tap_threshold_ms > 2000
        {
            return Err(
                "Choose a supported 30 fps camera mode and a tap threshold from 50 to 2000 ms."
                    .into(),
            );
        }
        if !generated
            && (!devices::valid_camera(&self.camera_id)
                || self
                    .microphone_id
                    .as_deref()
                    .is_some_and(|id| !devices::valid_microphone(id)))
        {
            return Err("Choose a detected camera and microphone".into());
        }
        Ok(())
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CapturePhase {
    Starting,
    Ready,
    Holding,
    LongTake,
    Stopping,
    Finishing,
    Stopped,
    Failed,
}
#[derive(Clone, Debug)]
pub struct CaptureStatus {
    pub session_id: String,
    pub phase: CapturePhase,
    pub media_ms: u64,
    pub active_ms: u64,
    pub pending_takes: usize,
    pub completed_takes: usize,
    pub camera_running: bool,
    pub message: String,
}
impl CaptureStatus {
    pub fn can_leave(&self) -> bool {
        !self.camera_running
            && self.pending_takes == 0
            && matches!(self.phase, CapturePhase::Stopped | CapturePhase::Failed)
    }
}
#[derive(Debug)]
pub struct PreviewFrame {
    pub sequence: u64,
    pub media_ms: u64,
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SlotMark {
    pub slot: SlotKey,
    pub offset_ms: u64,
    pub duration_ms: u64,
    pub content: OverlayContent,
}
#[derive(Clone, Debug)]
pub struct CompletedTake {
    pub(crate) ordinal: u64,
    pub(crate) media: ImportedMedia,
    pub(crate) marks: Vec<SlotMark>,
}
impl CompletedTake {
    pub const fn ordinal(&self) -> u64 {
        self.ordinal
    }
    pub fn media(&self) -> &ImportedMedia {
        &self.media
    }
    pub fn marks(&self) -> &[SlotMark] {
        &self.marks
    }
    pub fn append_to(&self, store: &ProjectStore, editor: &mut Editor) -> Result<ClipId> {
        let (clip, mut edits) = self.media.append_edits(store, editor)?;
        for mark in &self.marks {
            if mark.offset_ms >= self.media.info().duration_ms {
                continue;
            }
            edits.push(Edit::AddCueFromSnapshot {
                clip,
                offset_ms: mark.offset_ms,
                slot: mark.slot,
                content: mark.content.clone(),
                duration_ms: mark
                    .duration_ms
                    .min(self.media.info().duration_ms - mark.offset_ms),
            });
        }
        editor.apply_batch(edits)?;
        Ok(clip)
    }
}
#[derive(Clone, Debug)]
pub enum CaptureEvent {
    Ready,
    TakeQueued { ordinal: u64 },
    TakeCompleted(CompletedTake),
    TakeFailed { ordinal: u64, message: String },
    Stopped,
    Error(String),
}
#[derive(Clone, Debug)]
pub struct RecoverySession {
    pub id: String,
    pub pending_takes: usize,
    pub completed_takes: usize,
    pub message: String,
}
