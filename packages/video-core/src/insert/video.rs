use super::FrameReader;
use crate::{
    OutputSize, ProjectStore, Result,
    media::{self, Cancellation},
    renderer::RgbaFrame,
};
use catalyst_core::local::{self, FileMode, PathPolicy, PendingFile};
use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
    sync::{Arc, atomic::AtomicU64},
    time::Duration,
};

#[derive(Clone, Debug)]
pub struct InsertVideo {
    pub path: String,
    pub duration_ms: u64,
    pub frames: u64,
    pub size: OutputSize,
}
#[derive(Clone, Debug, PartialEq, Eq)]
struct Stamp {
    len: u64,
    modified: std::time::SystemTime,
    #[cfg(unix)]
    identity: (u64, u64),
}
impl Stamp {
    fn read(path: &Path) -> Result<Self> {
        let meta = fs::metadata(path)?;
        Ok(Self {
            len: meta.len(),
            modified: meta.modified()?,
            #[cfg(unix)]
            identity: {
                use std::os::unix::fs::MetadataExt;
                (meta.dev(), meta.ino())
            },
        })
    }
}
pub fn import_video(
    store: &ProjectStore,
    source: &Path,
    cancel: &Cancellation,
    progress: impl FnMut(u64),
) -> Result<InsertVideo> {
    let imported = media::import_media(store, source, cancel)?;
    prepare_video(store, imported.path(), cancel, progress)
}
pub fn prepare_video(
    store: &ProjectStore,
    relative: &str,
    cancel: &Cancellation,
    mut progress: impl FnMut(u64),
) -> Result<InsertVideo> {
    cancel.check()?;
    crate::model::validate_media_path(relative)?;
    let source = local::confined_file(
        store.root(),
        relative,
        PathPolicy::LocalProject,
        FileMode::Existing,
    )?;
    let before = Stamp::read(&source)?;
    let info = media::probe_owned(&source, cancel)?;
    let path = format!("media/insert-{}.mp4", media::unique_id());
    let pending = PendingFile::prepare(store.root(), &path)?;
    let mut command = Command::new(media::tool("ffmpeg")?);
    command.args(["-hide_banner","-loglevel","error","-nostdin","-nostats","-progress","pipe:1","-stats_period","0.1","-y","-threads","2","-filter_threads","1","-protocol_whitelist","file,pipe","-format_whitelist",media::VIDEO_DEMUXERS,"-i"]).arg(&source)
      .args(["-map","0:v:0","-an","-vf","scale=w='if(gte(dar,1),trunc(min(1920,iw*sar)/2)*2,trunc(min(1920,ih)*dar/2)*2)':h='if(gte(dar,1),trunc(min(1920,iw*sar)/dar/2)*2,trunc(min(1920,ih)/2)*2)',fps=30:start_time=0,setpts=N/(30*TB),setsar=1","-c:v","libx264","-preset","veryfast","-crf","18","-pix_fmt","yuv420p","-r","30","-bf","0","-threads","2","-video_track_timescale","90000","-movflags","+faststart","-f","mp4"]).arg(pending.path());
    crate::export::process::run(
        &mut command,
        None,
        Arc::new(AtomicU64::new(0)),
        pending.path(),
        Duration::from_secs(120_u64.saturating_add((info.duration_ms / 1000).saturating_mul(10))),
        cancel,
        |ms| progress(ms.min(info.duration_ms)),
    )?;
    let mut prepared = probe_video_path(pending.path(), &path, cancel)?;
    local::confined_file(
        store.root(),
        relative,
        PathPolicy::LocalProject,
        FileMode::Existing,
    )?;
    if Stamp::read(&source)? != before {
        return Err("Video changed during insert preparation; no proxy was published".into());
    }
    cancel.check()?;
    pending.publish()?;
    prepared.path = path;
    Ok(prepared)
}
pub fn inspect_video(
    store: &ProjectStore,
    relative: &str,
    cancel: &Cancellation,
) -> Result<InsertVideo> {
    crate::model::validate_media_path(relative)?;
    let path = local::confined_file(
        store.root(),
        relative,
        PathPolicy::LocalProject,
        FileMode::Existing,
    )?;
    probe_video_path(&path, relative, cancel)
}
fn probe_video_path(path: &Path, relative: &str, cancel: &Cancellation) -> Result<InsertVideo> {
    let output=media::run_bounded(Command::new(media::tool("ffprobe")?).args(["-v","error","-protocol_whitelist","file,pipe","-f","mov","-show_entries","stream=codec_type,codec_name,width,height,avg_frame_rate,r_frame_rate,nb_frames,start_time,sample_aspect_ratio,duration","-of","json"]).arg(path),Duration::from_secs(30),cancel,65536)?;
    let value: serde_json::Value =
        serde_json::from_slice(&output.stdout).map_err(|_| "Invalid video insert metadata")?;
    let streams = value["streams"]
        .as_array()
        .ok_or("Missing video insert streams")?;
    let video = streams
        .iter()
        .find(|s| s["codec_type"] == "video")
        .ok_or("Insert has no video track")?;
    let size = OutputSize {
        width: video["width"]
            .as_u64()
            .and_then(|v| v.try_into().ok())
            .ok_or("Invalid insert width")?,
        height: video["height"]
            .as_u64()
            .and_then(|v| v.try_into().ok())
            .ok_or("Invalid insert height")?,
    };
    let frames = video["nb_frames"]
        .as_str()
        .and_then(|v| v.parse::<u64>().ok())
        .filter(|v| *v > 0)
        .ok_or("Prepare this video insert before use: its frame count is unavailable")?;
    let start = video["start_time"]
        .as_str()
        .and_then(|v| v.parse::<f64>().ok())
        .unwrap_or(f64::NAN);
    if size.width == 0
        || size.height == 0
        || size.width > 1920
        || size.height > 1920
        || !size.width.is_multiple_of(2)
        || !size.height.is_multiple_of(2)
        || video["codec_name"] != "h264"
        || video["avg_frame_rate"] != "30/1"
        || video["r_frame_rate"] != "30/1"
        || video["sample_aspect_ratio"] != "1:1"
        || !start.is_finite()
        || start.abs() > 0.000001
        || streams.len() != 1
        || streams.iter().any(|s| s["codec_type"] != "video")
    {
        return Err("Prepare this video insert first: it needs a muted, square-pixel30fps proxy within1920px".into());
    }
    if frames > i64::MAX as u64 / 3000 {
        return Err("Video insert exceeds the media timestamp range".into());
    }
    let duration_ms = u64::try_from((u128::from(frames) * 1000 + 15) / 30)
        .map_err(|_| "Video insert duration overflows")?;
    Ok(InsertVideo {
        path: relative.into(),
        duration_ms,
        frames,
        size,
    })
}
pub(crate) struct VideoCursor {
    root: PathBuf,
    path: String,
    source: PathBuf,
    stamp: Stamp,
    info: InsertVideo,
    reader: Option<FrameReader>,
}
impl VideoCursor {
    pub(crate) fn new(store: &ProjectStore, path: &str, cancel: &Cancellation) -> Result<Self> {
        let source = local::confined_file(
            store.root(),
            path,
            PathPolicy::LocalProject,
            FileMode::Existing,
        )?;
        let stamp = Stamp::read(&source)?;
        let info = inspect_video(store, path, cancel)?;
        if Stamp::read(&source)? != stamp {
            return Err("Video insert changed during preparation".into());
        }
        Ok(Self {
            root: store.root().into(),
            path: path.into(),
            source,
            stamp,
            info,
            reader: None,
        })
    }
    pub(crate) fn matches(&self, store: &ProjectStore, path: &str) -> bool {
        self.root == store.root() && self.path == path
    }
    pub(crate) fn frame_at(&mut self, frame: u64, cancel: &Cancellation) -> Result<&RgbaFrame> {
        cancel.check()?;
        if frame >= self.info.frames {
            return Err(
                "Video insert extends beyond its last frame; shorten the cue or change its start"
                    .into(),
            );
        }
        local::confined_file(
            &self.root,
            &self.path,
            PathPolicy::LocalProject,
            FileMode::Existing,
        )?;
        if Stamp::read(&self.source)? != self.stamp {
            return Err("Video insert changed; prepare it again before playback".into());
        }
        let restart = self.reader.as_ref().is_none_or(|r| {
            r.last_index()
                .is_some_and(|last| frame < last || frame > last.saturating_add(90))
        });
        if restart {
            self.reader.take();
            let mut command = Command::new(media::tool("ffmpeg")?);
            command
                .args([
                    "-hide_banner",
                    "-loglevel",
                    "error",
                    "-nostdin",
                    "-threads",
                    "2",
                    "-filter_threads",
                    "1",
                    "-ss",
                ])
                .arg((frame / 30).to_string())
                .args([
                    "-protocol_whitelist",
                    "file,pipe",
                    "-f",
                    "mov",
                    "-noautorotate",
                    "-i",
                ])
                .arg(&self.source)
                .args(["-map", "0:v:0", "-an", "-vf"])
                .arg(format!(
                    "trim=start_frame={},setpts=N/(30*TB),format=rgba",
                    frame % 30
                ))
                .args(["-fps_mode", "passthrough", "-f", "rawvideo", "pipe:1"]);
            self.reader = Some(FrameReader::spawn(&mut command, self.info.size, frame)?);
        }
        self.reader
            .as_mut()
            .ok_or("Missing video insert decoder")?
            .frame_at(frame, cancel)
    }
}
