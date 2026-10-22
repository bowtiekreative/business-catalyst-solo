//! Cancellable local source-frame preview for the editing playhead.
use crate::{
    OutputSize, ProjectStore, Result, Studio,
    media::{self, Cancellation},
    renderer::RgbaFrame,
};
use catalyst_core::local::{self, FileMode, PathPolicy};
use std::{process::Command, time::Duration};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FrameRequest {
    pub path: String,
    pub source_ms: u64,
    pub size: OutputSize,
}
impl FrameRequest {
    pub fn source(studio: &Studio, id: crate::MediaId, source_ms: u64) -> Option<Self> {
        let source = studio.media().iter().find(|m| m.id() == id)?;
        let full = studio.orientation().output_size();
        Some(Self {
            path: source.path().into(),
            source_ms: source_ms.min(source.duration_ms().checked_sub(1)?),
            size: OutputSize {
                width: full.width * 2 / 3,
                height: full.height * 2 / 3,
            },
        })
    }
    pub fn at(studio: &Studio, sequence_ms: u64) -> Option<Self> {
        let mut remaining = sequence_ms.min(studio.duration_ms().checked_sub(1)?);
        for clip in studio.clips() {
            if remaining < clip.duration_ms() {
                return Self::source(
                    studio,
                    clip.source_id(),
                    clip.in_ms().checked_add(remaining)?,
                );
            }
            remaining -= clip.duration_ms();
        }
        None
    }
}

pub fn decode(
    store: &ProjectStore,
    request: &FrameRequest,
    cancel: &Cancellation,
) -> Result<RgbaFrame> {
    cancel.check()?;
    crate::model::validate_media_path(&request.path)?;
    let source = local::confined_file(
        store.root(),
        &request.path,
        PathPolicy::LocalProject,
        FileMode::Existing,
    )?;
    let size = request.size;
    if size.width == 0 || size.height == 0 || size.width > 1280 || size.height > 1280 {
        return Err("Editing preview exceeds its1280px limit".into());
    }
    let bytes = size.width as usize * size.height as usize * 4;
    let mut command = Command::new(media::tool("ffmpeg")?);
    command.args(["-hide_banner", "-loglevel", "error", "-nostdin", "-threads", "2", "-filter_threads", "1", "-protocol_whitelist", "file,pipe", "-format_whitelist", media::VIDEO_DEMUXERS, "-ss"])
        .arg((request.source_ms / 1000).to_string())
        .arg("-i").arg(source)
        .args(["-map", "0:v:0", "-an", "-vf"])
        .arg(format!("setpts=PTS-STARTPTS,fps=30:start_time=0,scale={}:{}:force_original_aspect_ratio=increase,crop={}:{},setsar=1,tpad=stop_mode=clone:stop_duration=1,trim=start_frame={},format=rgba", size.width, size.height, size.width, size.height, (request.source_ms % 1000) * 30 / 1000))
        .args(["-frames:v", "1", "-threads", "1", "-f", "rawvideo", "pipe:1"]);
    let output = media::run_bounded(&mut command, Duration::from_secs(15), cancel, bytes)?;
    if output.stdout.len() != bytes {
        return Err("No video frame at this position".into());
    }
    Ok(RgbaFrame {
        width: size.width,
        height: size.height,
        pixels: output.stdout,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Edit, EditOutcome, Editor};
    #[test]
    fn playhead_follows_trim_split_reorder_and_end_without_changing_sources() {
        let mut editor = Editor::new(Studio::default()).unwrap();
        let EditOutcome::MediaAdded(source) = editor
            .apply(Edit::AddMedia {
                path: "media/take.mp4".into(),
                duration_ms: 5000,
            })
            .unwrap()
        else {
            panic!()
        };
        let EditOutcome::ClipAdded(a) = editor
            .apply(Edit::AddClip {
                source,
                name: "A".into(),
                in_ms: 1000,
                out_ms: 3000,
            })
            .unwrap()
        else {
            panic!()
        };
        editor
            .apply(Edit::SplitClip {
                clip: a,
                offset_ms: 1000,
            })
            .unwrap();
        let b = editor.studio().clips()[1].id();
        assert_eq!(
            FrameRequest::at(editor.studio(), 1000).unwrap().source_ms,
            2000
        );
        editor
            .apply(Edit::MoveClip {
                clip: b,
                to_index: 0,
            })
            .unwrap();
        assert_eq!(
            FrameRequest::at(editor.studio(), 0).unwrap().source_ms,
            2000
        );
        assert_eq!(
            FrameRequest::at(editor.studio(), u64::MAX)
                .unwrap()
                .source_ms,
            1999
        );
        assert_eq!(editor.studio().media()[0].duration_ms(), 5000);
        assert!(FrameRequest::at(&Studio::default(), 0).is_none());
    }

    #[test]
    #[ignore = "Generated local FFmpeg fixture; STORYFOLD_INPUT_FIXTURE=1"]
    fn generated_scrub_decodes_correct_clip_and_rejects_escape_and_cancel() {
        assert_eq!(std::env::var("STORYFOLD_INPUT_FIXTURE").as_deref(), Ok("1"));
        let temp = tempfile::tempdir().unwrap();
        let (store, _) =
            ProjectStore::create(temp.path().join("project"), "Scrub fixture").unwrap();
        let path = store.root().join("media/colors.mp4");
        let status = Command::new(media::tool("ffmpeg").unwrap())
            .args([
                "-v",
                "error",
                "-nostdin",
                "-f",
                "lavfi",
                "-i",
                "color=c=red:s=160x90:r=30:d=1",
                "-f",
                "lavfi",
                "-i",
                "color=c=blue:s=160x90:r=30:d=1",
                "-filter_complex",
                "[0:v][1:v]concat=n=2:v=1:a=0[v]",
                "-map",
                "[v]",
                "-c:v",
                "libx264",
                "-threads",
                "2",
                "-pix_fmt",
                "yuv420p",
            ])
            .arg(path)
            .status()
            .unwrap();
        assert!(status.success());
        let mut request = FrameRequest {
            path: "media/colors.mp4".into(),
            source_ms: 200,
            size: OutputSize {
                width: 160,
                height: 90,
            },
        };
        let red = decode(&store, &request, &Cancellation::default()).unwrap();
        assert!(red.pixels[0] > 200 && red.pixels[2] < 30);
        request.source_ms = 1200;
        let blue = decode(&store, &request, &Cancellation::default()).unwrap();
        assert!(blue.pixels[2] > 200 && blue.pixels[0] < 30);
        request.source_ms = 1999;
        let end = decode(&store, &request, &Cancellation::default()).unwrap();
        assert!(end.pixels[2] > 200 && end.pixels[0] < 30);
        let cancel = Cancellation::default();
        cancel.cancel();
        assert!(decode(&store, &request, &cancel).is_err());
        request.path = "../outside.mp4".into();
        assert!(decode(&store, &request, &Cancellation::default()).is_err());
        request.path = "https://example.com/video.mp4".into();
        assert!(decode(&store, &request, &Cancellation::default()).is_err());
    }
}
