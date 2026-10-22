//! Honest non-Linux stub: no substitute/fake device and no hardware invocation.
use super::*;
use crate::media::Cancellation;
use std::sync::Arc;
pub struct CaptureSession;
fn unsupported<T>() -> Result<T> {
    Err("Camera capture and buffer recovery are supported on Linux; local video import remains available".into())
}
impl CaptureSession {
    pub fn start(_store: &ProjectStore, _settings: CaptureSettings) -> Result<Self> {
        unsupported()
    }
    pub fn status(&self) -> CaptureStatus {
        CaptureStatus {
            session_id: String::new(),
            phase: CapturePhase::Failed,
            media_ms: 0,
            active_ms: 0,
            pending_takes: 0,
            completed_takes: 0,
            camera_running: false,
            message: "Camera capture is supported on Linux.".into(),
        }
    }
    pub fn latest_preview(&self) -> Option<Arc<PreviewFrame>> {
        None
    }
    pub fn set_preview_waker(&self, _wake: impl Fn() + Send + Sync + 'static) -> bool {
        false
    }
    pub fn poll_events(&self) -> Vec<CaptureEvent> {
        Vec::new()
    }
    pub fn press(&self) -> Result<()> {
        unsupported()
    }
    pub fn release(&self) -> Result<()> {
        unsupported()
    }
    pub fn stop_take(&self) -> Result<()> {
        unsupported()
    }
    pub fn cancel_take(&self) -> Result<()> {
        unsupported()
    }
    pub fn mark_slot(
        &self,
        _slot: SlotKey,
        _content: OverlayContent,
        _duration_ms: u64,
    ) -> Result<()> {
        unsupported()
    }
    pub fn stop(&self) -> Result<()> {
        Ok(())
    }
}
pub fn list_recoverable(_store: &ProjectStore) -> Result<Vec<RecoverySession>> {
    unsupported()
}
pub fn recover_session(
    _store: &ProjectStore,
    _id: &str,
    _cancel: &Cancellation,
) -> Result<Vec<CompletedTake>> {
    unsupported()
}
