//! Source-relative audio envelopes. Extraction is blocking and belongs on a worker.
//! PCM is reduced while streaming; neither source duration nor PCM is held in RAM.
use crate::{
    MediaSource, ProjectStore, Result,
    media::{self, Cancellation},
};
use catalyst_core::local::{self, FileMode, PathPolicy};
use std::{
    fs,
    io::Read,
    process::{Command, Stdio},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::{Duration, Instant},
};

pub const MAX_BINS: usize = 65_536;
const SAMPLE_RATE: u64 = 8_000;
const SILENCE: f32 = 0.0001;

#[derive(Debug)]
pub enum Analysis {
    NoAudio,
    Audio(Envelope),
}

/// Peak amplitudes over equal source-time intervals, including leading silence.
#[derive(Debug)]
pub struct Envelope {
    duration_ms: u64,
    peaks: Vec<f32>,
    silent: bool,
}
impl Envelope {
    /// Construct a validated bounded envelope, useful for deterministic consumers.
    pub fn from_peaks(duration_ms: u64, peaks: Vec<f32>) -> Result<Self> {
        if duration_ms == 0
            || peaks.is_empty()
            || peaks.len() > MAX_BINS
            || peaks
                .iter()
                .any(|p| !p.is_finite() || !(0. ..=1.).contains(p))
        {
            return Err("Invalid audio waveform".into());
        }
        let silent = peaks.iter().all(|p| *p <= SILENCE);
        Ok(Self {
            duration_ms,
            peaks,
            silent,
        })
    }
    pub fn duration_ms(&self) -> u64 {
        self.duration_ms
    }
    pub fn bins(&self) -> usize {
        self.peaks.len()
    }
    pub fn byte_len(&self) -> usize {
        self.peaks.len() * size_of::<f32>()
    }
    pub fn is_silent(&self) -> bool {
        self.silent
    }

    /// Maximum of bins intersecting [start,end), clamped to the original source.
    /// Callers map their visible pixels through the clip's current source range.
    pub fn peak(&self, start_ms: u64, end_ms: u64) -> f32 {
        let start = start_ms.min(self.duration_ms);
        let end = end_ms.min(self.duration_ms);
        if start >= end {
            return 0.;
        }
        let count = self.peaks.len() as u128;
        let first = (u128::from(start) * count / u128::from(self.duration_ms)) as usize;
        let last = (u128::from(end) * count).div_ceil(u128::from(self.duration_ms)) as usize;
        self.peaks[first..last.min(self.peaks.len())]
            .iter()
            .copied()
            .fold(0., f32::max)
    }
}

pub fn extract(
    store: &ProjectStore,
    source: &MediaSource,
    cancel: &Cancellation,
) -> Result<Analysis> {
    extract_with_bins(store, source, cancel, MAX_BINS)
}

/// The cache can reduce resolution for very large projects without a duration cap.
pub fn extract_with_bins(
    store: &ProjectStore,
    source: &MediaSource,
    cancel: &Cancellation,
    max_bins: usize,
) -> Result<Analysis> {
    cancel.check()?;
    if max_bins == 0 || max_bins > MAX_BINS {
        return Err("Invalid waveform resolution".into());
    }
    crate::model::validate_media_path(source.path())?;
    let path = local::confined_file(
        store.root(),
        source.path(),
        PathPolicy::LocalProject,
        FileMode::Existing,
    )?;
    let before = fs::metadata(&path)?;
    let info = media::probe_owned(&path, cancel)?;
    if !info.has_audio {
        return Ok(Analysis::NoAudio);
    }
    let samples = sample_count(source.duration_ms())?;
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
            "-protocol_whitelist",
            "file,pipe",
            "-format_whitelist",
            media::VIDEO_DEMUXERS,
            "-i",
        ])
        .arg(&path)
        .args(["-map", "0:a:0", "-vn", "-sn", "-dn", "-ac", "1", "-af"])
        .arg(format!(
            "aresample={SAMPLE_RATE}:async=1:first_pts=0,apad,atrim=end_sample={samples}"
        ))
        .args(["-ar", "8000", "-c:a", "pcm_f32le", "-f", "f32le", "pipe:1"]);
    let bins = source.duration_ms().div_ceil(20).min(max_bins as u64) as usize;
    let envelope = stream(&mut command, source.duration_ms(), samples, bins, cancel)?;
    cancel.check()?;
    local::confined_file(
        store.root(),
        source.path(),
        PathPolicy::LocalProject,
        FileMode::Existing,
    )?;
    let after = fs::metadata(path)?;
    if before.len() != after.len() || before.modified()? != after.modified()? {
        return Err("Source changed while preparing its waveform".into());
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if before.dev() != after.dev() || before.ino() != after.ino() {
            return Err("Source changed while preparing its waveform".into());
        }
    }
    Ok(Analysis::Audio(envelope))
}

fn sample_count(duration_ms: u64) -> Result<u64> {
    let samples = (u128::from(duration_ms) * u128::from(SAMPLE_RATE)).div_ceil(1000);
    if samples == 0 || samples > i64::MAX as u128 {
        return Err("Source duration exceeds the audio tool's timestamp range".into());
    }
    Ok(samples as u64)
}

struct Accumulator {
    peaks: Vec<f32>,
    expected: u64,
    count: u64,
}
impl Accumulator {
    fn sample(&mut self, sample: f32) -> Result<()> {
        if !sample.is_finite() || self.count >= self.expected {
            return Err("Audio decoder exceeded the source sample bounds".into());
        }
        let bin = (u128::from(self.count) * self.peaks.len() as u128 / u128::from(self.expected))
            as usize;
        self.peaks[bin] = self.peaks[bin].max(sample.abs().min(1.));
        self.count += 1;
        Ok(())
    }
    fn finish(self, duration_ms: u64) -> Result<Envelope> {
        if self.count != self.expected {
            return Err("Audio waveform ended before the source duration".into());
        }
        Envelope::from_peaks(duration_ms, self.peaks)
    }
}

fn stream(
    command: &mut Command,
    duration_ms: u64,
    samples: u64,
    bins: usize,
    cancel: &Cancellation,
) -> Result<Envelope> {
    cancel.check()?;
    let mut child = command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|_| "Audio waveform decoder could not start")?;
    let mut output = child.stdout.take().expect("piped audio stdout");
    let mut errors = child.stderr.take().expect("piped audio stderr");
    let failed = Arc::new(AtomicBool::new(false));
    let reader_failed = failed.clone();
    let reader_cancel = cancel.clone();
    let reader = thread::spawn(move || -> Result<Envelope> {
        let mut accumulator = Accumulator {
            peaks: vec![0.; bins],
            expected: samples,
            count: 0,
        };
        let mut bytes = [0_u8; 8192];
        let mut remainder = 0;
        let result = (|| {
            loop {
                reader_cancel.check()?;
                let count = output.read(&mut bytes[remainder..])?;
                if count == 0 {
                    break;
                }
                let available = remainder + count;
                for sample in bytes[..available / 4 * 4].as_chunks::<4>().0 {
                    accumulator.sample(f32::from_le_bytes(*sample))?;
                }
                remainder = available % 4;
                bytes.copy_within(available - remainder..available, 0);
            }
            if remainder != 0 {
                return Err("Audio decoder returned an incomplete sample".into());
            }
            accumulator.finish(duration_ms)
        })();
        if result.is_err() {
            reader_failed.store(true, Ordering::Release);
        }
        result
    });
    let error_failed = failed.clone();
    let diagnostics = thread::spawn(move || {
        let mut bytes = [0_u8; 4096];
        let mut total = 0usize;
        while let Ok(count) = errors.read(&mut bytes) {
            if count == 0 {
                break;
            }
            total = total.saturating_add(count);
            if total > 65_536 {
                error_failed.store(true, Ordering::Release);
            }
        }
    });
    // Decode has at least30s startup budget and otherwise up to real-time source
    // duration. This bounds stalled work without rejecting a long interview.
    let timeout = Duration::from_millis(duration_ms.saturating_add(30_000));
    let started = Instant::now();
    let result: Result<()> = loop {
        if cancel.is_cancelled() || failed.load(Ordering::Acquire) || started.elapsed() > timeout {
            let _ = child.kill();
            let _ = child.wait();
            break Err("Waveform preparation cancelled, timed out or exceeded its bounds".into());
        }
        match child.try_wait() {
            Ok(Some(status)) => {
                break if status.success() {
                    Ok(())
                } else {
                    Err("Audio waveform decoder failed; source media is unchanged".into())
                };
            }
            Ok(None) => thread::sleep(Duration::from_millis(10)),
            Err(_) => {
                let _ = child.kill();
                let _ = child.wait();
                break Err("Audio waveform decoder could not be monitored".into());
            }
        }
    };
    let envelope = reader.join().map_err(|_| "Audio waveform reader stopped")?;
    diagnostics
        .join()
        .map_err(|_| "Audio waveform diagnostic reader stopped")?;
    result?;
    cancel.check()?;
    envelope
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn source_ranges_preserve_silence_trim_split_and_large_timestamps() {
        let envelope = Envelope::from_peaks(2000, vec![0., 0.7, 0., 0.9]).unwrap();
        assert_eq!(envelope.peak(0, 500), 0.);
        assert_eq!(envelope.peak(500, 1000), 0.7);
        assert_eq!(envelope.peak(1000, 1500), 0.);
        assert_eq!(envelope.peak(1500, u64::MAX), 0.9);
        assert_eq!(envelope.peak(2000, u64::MAX), 0.);
        assert_eq!(envelope.peak(500, 501), 0.7);
        let huge = Envelope::from_peaks(u64::MAX, vec![0.2, 0.8]).unwrap();
        assert_eq!(huge.peak(u64::MAX - 1, u64::MAX), 0.8);
        assert!(Envelope::from_peaks(10, vec![f32::NAN]).is_err());
        assert!(Envelope::from_peaks(10, vec![0.; MAX_BINS + 1]).is_err());
        assert!(Envelope::from_peaks(10, vec![0.]).unwrap().is_silent());
    }

    #[test]
    fn streaming_accumulator_is_bounded_and_keeps_signed_peaks() {
        let mut accumulator = Accumulator {
            peaks: vec![0.; 4],
            expected: 8,
            count: 0,
        };
        for sample in [0., 0., -0.7, 0.1, 0., 0., 0.9, 0.] {
            accumulator.sample(sample).unwrap();
        }
        assert!(accumulator.sample(0.).is_err());
        let envelope = accumulator.finish(1000).unwrap();
        assert_eq!(envelope.peak(250, 500), 0.7);
        assert_eq!(envelope.peak(750, 1000), 0.9);
        assert!(sample_count(0).is_err());
        assert!(sample_count(u64::MAX).is_err());
    }

    #[test]
    #[ignore = "Generated local audio fixtures only; STORYFOLD_WAVEFORM_TEST=1"]
    fn generated_tone_silence_no_track_and_cancel_are_distinct() {
        assert_eq!(std::env::var("STORYFOLD_WAVEFORM_TEST").as_deref(), Ok("1"));
        let temp = tempfile::tempdir().unwrap();
        let (store, _) =
            ProjectStore::create(temp.path().join("project"), "Waveform check").unwrap();
        for (name, audio) in [
            (
                "pattern",
                Some("aevalsrc=0.6*sin(2*PI*440*t)*between(mod(t\\,1)\\,0.5\\,0.9999):s=8000:d=2"),
            ),
            ("silent", Some("anullsrc=r=8000:cl=mono:d=2")),
            ("no-audio", None),
        ] {
            let path = format!("media/{name}.mkv");
            let mut command = Command::new(media::tool("ffmpeg").unwrap());
            command.args([
                "-v",
                "error",
                "-nostdin",
                "-f",
                "lavfi",
                "-i",
                "color=c=black:s=64x64:r=30:d=2",
            ]);
            if let Some(audio) = audio {
                command.args([
                    "-f",
                    "lavfi",
                    "-i",
                    audio,
                    "-map",
                    "0:v",
                    "-map",
                    "1:a",
                    "-c:a",
                    "pcm_s16le",
                ]);
            }
            command
                .args(["-c:v", "libx264", "-threads", "1", "-pix_fmt", "yuv420p"])
                .arg(store.root().join(&path));
            assert!(command.status().unwrap().success());
            let source = MediaSource {
                id: crate::MediaId::try_from(1).unwrap(),
                path,
                duration_ms: 2000,
            };
            let analysis = extract(&store, &source, &Cancellation::default()).unwrap();
            match (name, analysis) {
                ("pattern", Analysis::Audio(e)) => {
                    assert!(e.peak(100, 300) < 0.001);
                    assert!(e.peak(600, 800) > 0.5);
                    assert!(e.peak(1100, 1300) < 0.001);
                    assert!(e.peak(1600, 1800) > 0.5);
                    assert!(e.byte_len() <= MAX_BINS * 4);
                }
                ("silent", Analysis::Audio(e)) => assert!(e.is_silent()),
                ("no-audio", Analysis::NoAudio) => {}
                _ => panic!("Audio states were confused"),
            }
            let cancel = Cancellation::default();
            cancel.cancel();
            assert!(extract(&store, &source, &cancel).is_err());
        }
        let source = MediaSource {
            id: crate::MediaId::try_from(1).unwrap(),
            path: "../outside.mp4".into(),
            duration_ms: 2000,
        };
        assert!(extract(&store, &source, &Cancellation::default()).is_err());
    }
}
