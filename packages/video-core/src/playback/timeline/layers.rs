use super::Ipc;
use crate::{
    OutputSize, Result,
    insert::FrameReader,
    media::{self, Cancellation},
    renderer::RgbaFrame,
};
use serde_json::{Value, json};
use std::{
    fs::File,
    io::{Seek, SeekFrom, Write},
    os::fd::{AsRawFd, FromRawFd},
    os::unix::process::CommandExt,
    path::{Path, PathBuf},
    process::Command,
    time::Duration,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct VideoRect {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}
impl VideoRect {
    pub(super) fn parse(value: &Value) -> Result<Option<Self>> {
        let number = |name: &str| {
            value[name]
                .as_f64()
                .filter(|n| n.is_finite() && *n >= 0.)
                .map(|n| n.round() as u32)
        };
        let (Some(width), Some(height)) = (number("w"), number("h")) else {
            return Ok(None);
        };
        if width == 0 || height == 0 {
            return Ok(None);
        }
        let (x, y) = (number("ml").unwrap_or(0), number("mt").unwrap_or(0));
        let width = width
            .checked_sub(x)
            .and_then(|width| width.checked_sub(number("mr").unwrap_or(0)))
            .ok_or("Invalid native video margins")?;
        let height = height
            .checked_sub(y)
            .and_then(|height| height.checked_sub(number("mb").unwrap_or(0)))
            .ok_or("Invalid native video margins")?;
        if width == 0 || height == 0 {
            return Ok(None);
        }
        if width > 4096 || height > 4096 || u64::from(width) * u64::from(height) > 16 * 1024 * 1024
        {
            return Err("Native preview window exceeds its bitmap resource limit".into());
        }
        Ok(Some(Self {
            x,
            y,
            width,
            height,
        }))
    }
}
pub(super) struct Bitmap {
    file: File,
}
impl Bitmap {
    pub(super) fn new() -> Result<Self> {
        // SAFETY: fixed nul-terminated name; a successful descriptor is newly
        // owned, immediately wrapped once, and remains CLOEXEC in the parent.
        let fd = unsafe { libc::memfd_create(c"storyfold-preview".as_ptr(), libc::MFD_CLOEXEC) };
        if fd < 0 {
            return Err(std::io::Error::last_os_error().into());
        }
        Ok(Self {
            file: unsafe { File::from_raw_fd(fd) },
        })
    }
    pub(super) fn inherit(&self, command: &mut Command) {
        let fd = self.file.as_raw_fd();
        // SAFETY: pre_exec performs only the async-signal-safe fcntl syscall.
        // The live File owns fd through spawn. Clear CLOEXEC only in this mpv
        // child, so unrelated FFmpeg/jobs never inherit the bitmap descriptor.
        unsafe {
            command.pre_exec(move || {
                if libc::fcntl(fd, libc::F_SETFD, 0) < 0 {
                    Err(std::io::Error::last_os_error())
                } else {
                    Ok(())
                }
            });
        }
    }
    pub(super) fn show(&mut self, ipc: &mut Ipc, frame: &RgbaFrame, rect: VideoRect) -> Result<()> {
        let pixels = display_bitmap(frame, rect)?;
        self.file.set_len(pixels.len() as u64)?;
        self.file.seek(SeekFrom::Start(0))?;
        self.file.write_all(&pixels)?;
        // mpv0.37 copies the mapped bytes before this synchronous call returns.
        ipc.call(json!({"name":"overlay-add","id":0,"x":rect.x,"y":rect.y,"file":format!("@{}",self.file.as_raw_fd()),"offset":0,"fmt":"bgra","w":rect.width,"h":rect.height,"stride":rect.width*4}))?;
        Ok(())
    }
}
pub(super) fn display_bitmap(frame: &RgbaFrame, rect: VideoRect) -> Result<Vec<u8>> {
    if frame.pixels.len() != frame.width as usize * frame.height as usize * 4 {
        return Err("Incomplete preview composition".into());
    }
    let mut pixels = frame.pixels.clone();
    for pixel in pixels.as_chunks_mut::<4>().0 {
        let alpha = u16::from(pixel[3]);
        let red = ((u16::from(pixel[0]) * alpha + 127) / 255) as u8;
        pixel[0] = ((u16::from(pixel[2]) * alpha + 127) / 255) as u8;
        pixel[1] = ((u16::from(pixel[1]) * alpha + 127) / 255) as u8;
        pixel[2] = red;
    }
    let image = image::RgbaImage::from_raw(frame.width, frame.height, pixels)
        .ok_or("Incomplete preview bitmap")?;
    Ok(image::imageops::resize(
        &image,
        rect.width,
        rect.height,
        image::imageops::FilterType::Triangle,
    )
    .into_raw())
}

pub(super) struct CameraCursor {
    path: PathBuf,
    size: OutputSize,
    reader: Option<FrameReader>,
}
impl CameraCursor {
    pub(super) fn matches(&self, path: &Path) -> bool {
        self.path == path
    }
    pub(super) fn new(path: &Path, info: &media::MediaInfo, cancel: &Cancellation) -> Result<Self> {
        let display = display_size(path, info, cancel)?;
        let factor = (1280. / f64::from(display.width))
            .min(1280. / f64::from(display.height))
            .min(1.);
        let size = OutputSize {
            width: ((f64::from(display.width) * factor / 2.).floor() as u32 * 2).max(2),
            height: ((f64::from(display.height) * factor / 2.).floor() as u32 * 2).max(2),
        };
        Ok(Self {
            path: path.into(),
            size,
            reader: None,
        })
    }
    pub(super) fn frame(&mut self, source_ms: u64, cancel: &Cancellation) -> Result<&RgbaFrame> {
        let target = crate::insert::frame_index(source_ms)?;
        if self.reader.as_ref().is_none_or(|reader| {
            reader
                .last_index()
                .is_some_and(|last| target < last || target > last.saturating_add(30))
        }) {
            self.reader.take();
            let mut command = Command::new(media::tool("ffmpeg")?);
            command.args(["-hide_banner","-loglevel","error","-nostdin","-threads","2","-filter_threads","1","-protocol_whitelist","file,pipe","-format_whitelist",media::VIDEO_DEMUXERS,"-ss"])
                .arg((target/30).to_string()).arg("-i").arg(&self.path).args(["-map","0:v:0","-an","-vf"])
                .arg(format!("setpts=PTS-STARTPTS,fps=30:start_time=0,scale={}:{},setsar=1,trim=start_frame={},format=rgba",self.size.width,self.size.height,target%30))
                .args(["-threads","1","-f","rawvideo","pipe:1"]);
            self.reader = Some(FrameReader::spawn(&mut command, self.size, target)?);
        }
        self.reader
            .as_mut()
            .ok_or("Missing preview source decoder")?
            .frame_at(target, cancel)
    }
}
fn display_size(path: &Path, info: &media::MediaInfo, cancel: &Cancellation) -> Result<OutputSize> {
    let output = media::run_bounded(
        Command::new(media::tool("ffprobe")?)
            .args([
                "-v",
                "error",
                "-protocol_whitelist",
                "file,pipe",
                "-format_whitelist",
                media::VIDEO_DEMUXERS,
                "-select_streams",
                "v:0",
                "-show_entries",
                "stream=sample_aspect_ratio:stream_side_data=rotation",
                "-of",
                "json",
            ])
            .arg(path),
        Duration::from_secs(10),
        cancel,
        65536,
    )?;
    let value: Value = serde_json::from_slice(&output.stdout)?;
    let stream = &value["streams"][0];
    let sar = match stream["sample_aspect_ratio"].as_str() {
        None | Some("N/A" | "0:1") => 1.,
        Some(value) => {
            let (a, b) = value
                .split_once(':')
                .ok_or("Invalid preview pixel aspect")?;
            let (a, b) = (a.parse::<u32>()?, b.parse::<u32>()?);
            if a == 0 || b == 0 {
                return Err("Invalid preview pixel aspect".into());
            }
            f64::from(a) / f64::from(b)
        }
    };
    let rotation = stream["side_data_list"]
        .as_array()
        .and_then(|list| list.iter().find_map(|item| item["rotation"].as_i64()))
        .unwrap_or(0)
        .rem_euclid(360);
    let (mut width, mut height) = (
        (f64::from(info.width) * sar).round(),
        f64::from(info.height),
    );
    if rotation == 90 || rotation == 270 {
        std::mem::swap(&mut width, &mut height);
    } else if rotation != 0 && rotation != 180 {
        return Err("Normalize non-right-angle source rotation before previewing inserts".into());
    }
    if !(1.0..=16384.0).contains(&width) || !(1.0..=16384.0).contains(&height) {
        return Err("Preview source geometry exceeds its limit".into());
    }
    Ok(OutputSize {
        width: width as u32,
        height: height as u32,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn video_rectangle_excludes_controller_margins_and_premultiplies_bgra() {
        let rect = VideoRect::parse(&json!({"w":100,"h":100,"mt":10,"mb":30,"ml":5,"mr":5}))
            .unwrap()
            .unwrap();
        assert_eq!(
            rect,
            VideoRect {
                x: 5,
                y: 10,
                width: 90,
                height: 60
            }
        );
        let rgba = RgbaFrame {
            width: 1,
            height: 1,
            pixels: vec![200, 100, 50, 128],
        };
        let bytes = display_bitmap(
            &rgba,
            VideoRect {
                x: 0,
                y: 0,
                width: 1,
                height: 1,
            },
        )
        .unwrap();
        assert_eq!(bytes, [25, 50, 100, 128]);
        assert!(VideoRect::parse(&json!({"w":8000,"h":8000})).is_err());
    }
}
