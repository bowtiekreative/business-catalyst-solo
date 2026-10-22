//! Encoder selection is an explicit bounded worker operation. Constructors and
//! UI painting only read the process preference and never initialize a GPU.
use crate::{
    Result,
    media::{self, Cancellation},
};
use serde::{Deserialize, Serialize};
use std::{
    process::Command,
    sync::atomic::{AtomicBool, Ordering},
    time::Duration,
};

static PREFER_GPU: AtomicBool = AtomicBool::new(true);

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VideoEncoder {
    #[default]
    Cpu,
    Nvidia,
}
impl VideoEncoder {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Cpu => "CPU (x264)",
            Self::Nvidia => "NVIDIA GPU (NVENC)",
        }
    }
    /// Append only encoder-specific options. Callers retain their existing
    /// frame clock, geometry, pixel format, GOP, audio and container settings.
    /// CRF and NVENC CQ are independent quality scales, not identical output.
    pub fn configure(self, command: &mut Command, quality: u8, low_latency: bool) {
        let quality = quality.min(51).to_string();
        match self {
            Self::Cpu => {
                command.args(["-c:v", "libx264", "-preset", "veryfast", "-crf", &quality]);
                if low_latency {
                    command.args(["-tune", "zerolatency"]);
                }
            }
            Self::Nvidia => {
                command.args([
                    "-c:v",
                    "h264_nvenc",
                    "-preset",
                    "p4",
                    "-tune",
                    if low_latency { "ull" } else { "hq" },
                    "-rc",
                    "vbr",
                    "-cq",
                    &quality,
                    "-b:v",
                    "0",
                    "-rc-lookahead",
                    "0",
                    "-zerolatency",
                    "1",
                ]);
            }
        }
    }
}

pub fn prefer_gpu() -> bool {
    PREFER_GPU.load(Ordering::Relaxed)
}
/// Applies to subsequently started jobs, never a running capture or export.
pub fn set_prefer_gpu(preferred: bool) {
    PREFER_GPU.store(preferred, Ordering::Relaxed);
}
/// Probe real encoder initialization with generated pixels, no camera, mic or
/// network. An unavailable GPU falls back before work begins. Do not reuse this
/// to switch codecs halfway through a recording or to mix recovery segments.
pub fn select(cancel: &Cancellation) -> Result<VideoEncoder> {
    select_with(prefer_gpu(), cancel, || {
        // Validate both modes sequentially: probing must not consume two NVENC
        // session slots when the actual job only needs one at a time.
        for (quality, low_latency) in [(20, true), (18, false)] {
            cancel.check()?;
            let mut command = Command::new(media::tool("ffmpeg")?);
            command.args([
                "-hide_banner",
                "-loglevel",
                "error",
                "-nostdin",
                "-f",
                "lavfi",
                "-i",
                "color=size=1920x1080:rate=30",
            ]);
            command.args(["-map", "0:v:0", "-frames:v", "3", "-an"]);
            VideoEncoder::Nvidia.configure(&mut command, quality, low_latency);
            command.args([
                "-pix_fmt",
                "yuv420p",
                "-profile:v",
                "high",
                "-g",
                "30",
                "-bf",
                "0",
                "-f",
                "null",
                "pipe:1",
            ]);
            media::run_bounded(&mut command, Duration::from_secs(4), cancel, 4096)?;
        }
        Ok(())
    })
}
fn select_with(
    preferred: bool,
    cancel: &Cancellation,
    probe: impl FnOnce() -> Result<()>,
) -> Result<VideoEncoder> {
    cancel.check()?;
    if !preferred {
        return Ok(VideoEncoder::Cpu);
    }
    let available = probe().is_ok();
    // A cancelled probe must never be converted into a CPU job.
    cancel.check()?;
    Ok(if available {
        VideoEncoder::Nvidia
    } else {
        VideoEncoder::Cpu
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn disabled_gpu_skips_probe_and_failed_probe_can_be_retried() {
        let cancel = Cancellation::default();
        assert_eq!(
            select_with(false, &cancel, || panic!(
                "Disabled GPU must not initialize"
            ))
            .unwrap(),
            VideoEncoder::Cpu
        );
        assert_eq!(
            select_with(true, &cancel, || Err("Unavailable GPU".into())).unwrap(),
            VideoEncoder::Cpu
        );
        assert_eq!(
            select_with(true, &cancel, || Ok(())).unwrap(),
            VideoEncoder::Nvidia
        );
    }
    #[test]
    fn cancellation_before_or_during_probe_never_falls_back_to_cpu() {
        let cancel = Cancellation::default();
        cancel.cancel();
        assert!(select_with(true, &cancel, || panic!("Already cancelled")).is_err());
        let cancel = Cancellation::default();
        assert!(
            select_with(true, &cancel, || {
                cancel.cancel();
                Err("Cancelled probe".into())
            })
            .is_err()
        );
    }
    #[test]
    fn encoder_options_keep_quality_modes_separate_without_constructing_hardware() {
        for encoder in [VideoEncoder::Cpu, VideoEncoder::Nvidia] {
            for low_latency in [false, true] {
                let mut command = Command::new("not-executed");
                encoder.configure(&mut command, 20, low_latency);
                let args: Vec<_> = command.get_args().filter_map(|arg| arg.to_str()).collect();
                match encoder {
                    VideoEncoder::Cpu => {
                        assert!(args.windows(2).any(|pair| pair == ["-c:v", "libx264"]));
                        assert!(args.windows(2).any(|pair| pair == ["-crf", "20"]));
                        assert_eq!(args.contains(&"zerolatency"), low_latency);
                        assert!(!args.contains(&"-cq"));
                    }
                    VideoEncoder::Nvidia => {
                        assert!(args.windows(2).any(|pair| pair == ["-c:v", "h264_nvenc"]));
                        assert!(args.windows(2).any(|pair| pair == ["-cq", "20"]));
                        assert!(
                            args.windows(2)
                                .any(|pair| pair
                                    == ["-tune", if low_latency { "ull" } else { "hq" }])
                        );
                        assert!(!args.contains(&"-crf"));
                    }
                }
                assert!(!args.contains(&"-i"));
                assert!(!args.contains(&"-frames:v"));
            }
        }
        assert_eq!(VideoEncoder::default(), VideoEncoder::Cpu);
        assert_eq!(
            serde_json::to_string(&VideoEncoder::Nvidia).unwrap(),
            "\"nvidia\""
        );
    }
}
