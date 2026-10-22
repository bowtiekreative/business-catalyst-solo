//! Source scrubbing has one cancellable worker; intermediate seeks never queue.
use crate::Workspace;
use eframe::egui;
use std::{path::PathBuf, sync::mpsc, time::Duration};
use video_core::{
    media::Cancellation,
    preview::{self, FrameRequest},
    renderer::RgbaFrame,
};

type Key = (PathBuf, FrameRequest);
struct Pending {
    key: Key,
    cancel: Cancellation,
    receiver: mpsc::Receiver<Result<RgbaFrame, String>>,
}
#[derive(Default)]
pub struct ScrubUi {
    pending: Option<Pending>,
    requested: Option<Key>,
    pub texture: Option<egui::TextureHandle>,
    pub error: Option<String>,
}
impl Drop for ScrubUi {
    fn drop(&mut self) {
        if let Some(p) = &self.pending {
            p.cancel.cancel();
        }
    }
}
impl ScrubUi {
    pub fn busy(&self) -> bool {
        self.pending.is_some()
    }
    pub fn tick(
        &mut self,
        project: &Workspace,
        at: u64,
        live: bool,
        draft_source: Option<(video_core::MediaId, u64)>,
        ctx: &egui::Context,
    ) {
        let desired = (!live && !project.loading())
            .then(|| {
                let frame = match draft_source {
                    Some((source, ms)) => FrameRequest::source(project.studio(), source, ms),
                    None => FrameRequest::at(project.studio(), at),
                }?;
                Some((project.folder.clone()?, frame))
            })
            .flatten();
        if self
            .pending
            .as_ref()
            .is_some_and(|p| Some(&p.key) != desired.as_ref())
        {
            self.pending.as_ref().unwrap().cancel.cancel();
        }
        if self.requested.as_ref().map(|k| &k.0) != desired.as_ref().map(|k| &k.0)
            || desired.is_none()
        {
            self.texture = None;
        }
        if let Some(result) = self
            .pending
            .as_ref()
            .and_then(|p| match p.receiver.try_recv() {
                Ok(result) => Some(result),
                Err(mpsc::TryRecvError::Disconnected) => {
                    Some(Err("Editing preview worker stopped".into()))
                }
                Err(mpsc::TryRecvError::Empty) => None,
            })
        {
            let pending = self.pending.take().unwrap();
            if pending.cancel.is_cancelled() && self.requested.as_ref() == Some(&pending.key) {
                self.requested = None;
            }
            if !pending.cancel.is_cancelled() && Some(&pending.key) == desired.as_ref() {
                match result {
                    Ok(frame) => {
                        let image = egui::ColorImage::from_rgba_unmultiplied(
                            [frame.width as usize, frame.height as usize],
                            &frame.pixels,
                        );
                        match &mut self.texture {
                            Some(texture) => texture.set(image, egui::TextureOptions::LINEAR),
                            None => {
                                self.texture = Some(ctx.load_texture(
                                    "editing-source-preview",
                                    image,
                                    egui::TextureOptions::LINEAR,
                                ))
                            }
                        }
                        self.error = None;
                    }
                    Err(e) => {
                        self.error = Some(e);
                        self.texture = None;
                    }
                }
            }
        }
        if self.pending.is_none() && self.requested != desired {
            self.requested = desired.clone();
            self.error = None;
            if let Some(key) = desired
                && let Some(store) = project.store()
            {
                let (sender, receiver) = mpsc::channel();
                let cancel = Cancellation::default();
                let token = cancel.clone();
                let request = key.1.clone();
                let context = ctx.clone();
                match std::thread::Builder::new()
                    .name("storyfold-source-scrub".into())
                    .spawn(move || {
                        let result = (|| {
                            let store = store
                                .lock()
                                .map_err(|_| "Project preview worker failed".to_owned())?;
                            preview::decode(&store, &request, &token).map_err(|e| e.to_string())
                        })();
                        let _ = sender.send(result);
                        context.request_repaint();
                    }) {
                    Ok(_) => {
                        self.pending = Some(Pending {
                            key,
                            cancel,
                            receiver,
                        })
                    }
                    Err(_) => self.error = Some("Editing preview worker could not start".into()),
                }
            }
        }
        if self.pending.is_some() {
            ctx.request_repaint_after(Duration::from_millis(16));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn superseded_or_camera_enabled_seek_cannot_replace_preview() {
        let context = egui::Context::default();
        let request = FrameRequest {
            path: "media/take.mp4".into(),
            source_ms: 0,
            size: video_core::OutputSize {
                width: 2,
                height: 2,
            },
        };
        let key = (PathBuf::from("/old/project"), request);
        let (sender, receiver) = mpsc::channel();
        sender
            .send(Ok(RgbaFrame {
                width: 2,
                height: 2,
                pixels: vec![255; 16],
            }))
            .unwrap();
        let cancel = Cancellation::default();
        let mut scrub = ScrubUi {
            pending: Some(Pending {
                key: key.clone(),
                cancel: cancel.clone(),
                receiver,
            }),
            requested: Some(key),
            texture: None,
            error: None,
        };
        scrub.tick(&Workspace::default(), 0, true, None, &context);
        assert!(cancel.is_cancelled());
        assert!(!scrub.busy());
        assert!(scrub.texture.is_none());
        assert!(scrub.error.is_none());
    }
}
