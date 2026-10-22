use super::*;
use crate::{
    MediaId, OverlayContent,
    encoding::{self, VideoEncoder},
    insert::{self, InsertSource},
    renderer::{ActiveOverlay, OverlayAssets, OverlayRenderer, RenderOptions},
};
use catalyst_core::local::{PendingFile, PrivateNamespace, WriteExpected};
use std::{
    collections::HashMap,
    fs,
    io::Write,
    path::{Path, PathBuf},
    process::Command,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    time::Duration,
};

struct Reporter<F> {
    callback: F,
    last: f32,
    total: u64,
}
impl<F: FnMut(ExportProgress)> Reporter<F> {
    fn emit(
        &mut self,
        phase: ExportPhase,
        fraction: f32,
        completed_ms: u64,
        message: String,
        cancellable: bool,
    ) {
        self.last = self.last.max(fraction.clamp(0., 1.));
        (self.callback)(ExportProgress {
            phase,
            fraction: self.last,
            completed_ms: completed_ms.min(self.total),
            total_ms: self.total,
            message,
            cancellable,
        });
    }
}
#[derive(Clone)]
struct Scheduled {
    start: u64,
    end: u64,
    content: OverlayContent,
}
struct ClipInput {
    path: PathBuf,
    in_ms: u64,
    duration_ms: u64,
    frames: u64,
    has_audio: bool,
    source_size: OutputSize,
}
struct SourceInput {
    path: PathBuf,
    info: media::MediaInfo,
    size: OutputSize,
}
struct Prepared {
    inputs: Vec<ClipInput>,
    schedule: Vec<Scheduled>,
    fingerprints: HashMap<PathBuf, Fingerprint>,
}
#[derive(PartialEq)]
struct Fingerprint {
    len: u64,
    modified: std::time::SystemTime,
    #[cfg(unix)]
    dev: u64,
    #[cfg(unix)]
    ino: u64,
}
impl Fingerprint {
    fn read(path: &Path) -> Result<Self> {
        local::reject_file_links(path)?;
        let meta = fs::symlink_metadata(path)?;
        if !meta.is_file() || meta.file_type().is_symlink() {
            return Err("Export source must remain a regular local file".into());
        }
        #[cfg(unix)]
        use std::os::unix::fs::MetadataExt;
        Ok(Self {
            len: meta.len(),
            modified: meta.modified()?,
            #[cfg(unix)]
            dev: meta.dev(),
            #[cfg(unix)]
            ino: meta.ino(),
        })
    }
}
struct WorkDir {
    root: PathBuf,
    files: Vec<String>,
}
impl WorkDir {
    fn new(store: &ProjectStore) -> Result<Self> {
        let private = local::private_dir(store.root(), PrivateNamespace::Sidebar, false)?;
        let root = private.join(format!("export-{}", media::unique_id()));
        fs::create_dir(&root)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&root, fs::Permissions::from_mode(0o700))?;
        }
        Ok(Self {
            root: root.canonicalize()?,
            files: Vec::new(),
        })
    }
    fn file(&mut self, name: String) -> Result<PathBuf> {
        let path = local::confined_file(
            &self.root,
            &name,
            PathPolicy::LocalProject,
            FileMode::NewOrExisting,
        )?;
        self.files.push(name);
        Ok(path)
    }
    fn list(&mut self, name: &str, body: &str) -> Result<PathBuf> {
        let path = self.file(name.into())?;
        local::atomic_write(&path, body.as_bytes(), WriteExpected::Missing)?;
        Ok(path)
    }
}
impl Drop for WorkDir {
    fn drop(&mut self) {
        // Delete only generated registered files. Unknown/linked files keep the
        // private directory intact; never recursively remove a user's content.
        for name in self.files.iter().rev() {
            if let Ok(path) = local::confined_file(
                &self.root,
                name,
                PathPolicy::LocalProject,
                FileMode::Existing,
            ) {
                let _ = fs::remove_file(path);
            }
        }
        let _ = fs::remove_dir(&self.root);
    }
}

pub(super) fn export<F: FnMut(ExportProgress)>(
    store: &mut ProjectStore,
    snapshot: &Studio,
    options: ExportOptions,
    cancel: &Cancellation,
    progress: F,
) -> Result<ExportOutcome> {
    cancel.check()?;
    let plan = ExportPlan::new(snapshot)?;
    snapshot.to_json()?;
    let relative = output_relative(&options)?;
    let mut report = Reporter {
        callback: progress,
        last: 0.,
        total: plan.duration_ms,
    };
    report.emit(
        ExportPhase::Preparing,
        0.,
        0,
        "Checking sequence and local media…".into(),
        true,
    );
    local::existing_directory(store.root(), "exports")?;
    let pending = PendingFile::prepare(store.root(), &relative)?;
    // A separate read-only root handle is captured now solely for asset loading
    // on the frame worker. Final save always uses the ORIGINAL store baseline.
    let (asset_store, _) = ProjectStore::open(store.root())?;
    let Prepared {
        inputs,
        schedule,
        fingerprints,
    } = preflight(store, snapshot, &plan, cancel)?;
    // Snapshot the process preference once for this job. Every intermediate and
    // the final MP4 use the same encoder; a runtime failure remains an error.
    let encoder = encoding::select(cancel)?;
    report.emit(
        ExportPhase::Preparing,
        0.,
        0,
        format!(
            "Video encoding: {}. Preparing your sequence…",
            encoder.label()
        ),
        true,
    );
    let mut work = WorkDir::new(store)?;
    let mut video_list = String::from("ffconcat version 1.0\n");
    let mut audio_list = String::from("ffconcat version 1.0\n");
    let scene = schedule.iter().any(|cue| cue.content.insert.is_some());
    let mut prepared_ms = 0;
    for (index, input) in inputs.iter().enumerate() {
        cancel.check()?;
        let video_name = format!("clip-{index:05}.mp4");
        let audio_name = format!("clip-{index:05}.wav");
        let video = work.file(video_name.clone())?;
        let audio = work.file(audio_name.clone())?;
        let mut command = normalize_command(input, plan.size, &video, &audio, scene, encoder)?;
        let duration = frame_ms(input.frames);
        process::run(
            &mut command,
            None,
            Arc::new(AtomicU64::new(0)),
            &video,
            timeout(duration),
            cancel,
            |ms| {
                let done = prepared_ms + ms.min(duration);
                report.emit(
                    ExportPhase::PreparingClips,
                    0.45 * done as f32 / plan.duration_ms as f32,
                    done,
                    format!(
                        "Preparing clip {} of {} · {}",
                        index + 1,
                        inputs.len(),
                        encoder.label()
                    ),
                    true,
                );
            },
        )?;
        prepared_ms += duration;
        video_list.push_str(&format!("file '{video_name}'\n"));
        audio_list.push_str(&format!("file '{audio_name}'\n"));
    }
    let video_list = work.list("video.ffconcat", &video_list)?;
    let audio_list = work.list("audio.ffconcat", &audio_list)?;
    let heartbeat = Arc::new(AtomicU64::new(0));
    let writer_heartbeat = heartbeat.clone();
    let writer_cancel = cancel.clone();
    let frame_snapshot = snapshot.clone();
    let frame_plan = plan.clone();
    let scene_video_list = video_list.clone();
    let camera_ranges: Vec<_> = if scene {
        inputs
            .iter()
            .map(|input| Ok((input.frames, camera_fit(input.source_size, plan.size)?)))
            .collect::<Result<_>>()?
    } else {
        Vec::new()
    };
    let writer: process::Writer = Box::new(move |mut input| {
        let mut renderer = OverlayRenderer::new()?;
        let mut assets = OverlayAssets::new();
        let mut last_active = Vec::new();
        let mut camera_decoder = if scene {
            Some(insert::FrameReader::spawn(
                &mut camera_decode_command(&scene_video_list)?,
                frame_plan.size,
                0,
            )?)
        } else {
            None
        };
        let (mut camera_index, mut camera_end) = (
            0_usize,
            camera_ranges.first().map_or(0, |(frames, _)| *frames),
        );
        for frame in 0..frame_plan.frames {
            writer_cancel.check()?;
            let ms = frame_ms(frame);
            let active: Vec<_> = schedule
                .iter()
                .enumerate()
                .filter(|(_, cue)| ms >= cue.start && ms < cue.end)
                .map(|(index, _)| index)
                .collect();
            if active != last_active {
                assets.clear();
                for &index in &active {
                    assets.load_content(&asset_store, &schedule[index].content)?;
                }
                last_active = active;
            }
            let render_options = RenderOptions {
                reduced_motion: options.reduced_motion,
            };
            let rgba = if let Some(decoder) = &mut camera_decoder {
                while frame >= camera_end {
                    camera_index += 1;
                    camera_end = camera_end
                        .checked_add(
                            camera_ranges
                                .get(camera_index)
                                .ok_or("Camera sequence ended before its output frame")?
                                .0,
                        )
                        .ok_or("Camera frame count overflows")?;
                }
                let camera = crop_camera(
                    decoder.frame_at(frame, &writer_cancel)?,
                    camera_ranges[camera_index].1,
                )?;
                let overlays: Vec<_> = last_active
                    .iter()
                    .map(|&index| ActiveOverlay {
                        content: &schedule[index].content,
                        elapsed_ms: ms - schedule[index].start,
                        duration_ms: schedule[index].end - schedule[index].start,
                    })
                    .collect();
                assets.prepare_live(&asset_store, &overlays, &writer_cancel)?;
                renderer.render_scene_live(
                    frame_plan.size,
                    &camera,
                    &overlays,
                    &assets,
                    render_options,
                )?
            } else {
                renderer.render_timeline(&frame_snapshot, ms, &assets, render_options)?
            };
            input
                .write_all(&rgba.pixels)
                .map_err(|_| "Export frame stream closed before all frames were written")?;
            writer_heartbeat.store(frame + 1, Ordering::Release);
        }
        Ok(())
    });
    let mut command = final_command(
        &video_list,
        &audio_list,
        pending.path(),
        &plan,
        scene,
        encoder,
    )?;
    process::run(
        &mut command,
        Some(writer),
        heartbeat,
        pending.path(),
        timeout(plan.duration_ms),
        cancel,
        |ms| {
            let done = ms.min(plan.duration_ms);
            report.emit(
                ExportPhase::Encoding,
                0.45 + 0.52 * done as f32 / plan.duration_ms as f32,
                done,
                format!("Making the MP4 with your overlays · {}", encoder.label()),
                true,
            );
        },
    )?;
    cancel.check()?;
    report.emit(
        ExportPhase::Finalizing,
        0.98,
        plan.duration_ms,
        format!("Checking the completed MP4 · {}", encoder.label()),
        true,
    );
    verify_output(pending.path(), &plan, cancel)?;
    for (path, before) in fingerprints {
        let relative = path
            .strip_prefix(store.root())
            .ok()
            .and_then(|p| p.to_str())
            .ok_or("Export source moved outside its project")?;
        local::confined_file(
            store.root(),
            relative,
            PathPolicy::LocalProject,
            FileMode::Existing,
        )?;
        if Fingerprint::read(&path)? != before {
            return Err("A source changed during export; no MP4 was published".into());
        }
    }
    cancel.check()?;
    pending.publish()?;
    let artifact = ExportArtifact {
        relative_path: relative,
        duration_ms: plan.duration_ms,
        frames: plan.frames,
        size: plan.size,
    };
    report.emit(
        ExportPhase::SavingVersion,
        0.99,
        plan.duration_ms,
        "MP4 saved. Saving its local project version…".into(),
        false,
    );
    let mut outcome = ExportOutcome {
        artifact,
        project_saved: false,
        version: None,
        version_error: None,
    };
    match store.save(snapshot) {
        Ok(()) => {
            outcome.project_saved = true;
            match store.save_version(snapshot, "Export MP4") {
                Ok(version) => outcome.version = Some(version),
                Err(error) => {
                    outcome.version_error = Some(format!(
                        "MP4 and project saved, but the local version failed: {error}"
                    ))
                }
            }
        }
        Err(error) => {
            outcome.version_error = Some(format!(
                "MP4 saved, but its project version could not be saved: {error}"
            ))
        }
    }
    report.emit(
        ExportPhase::Completed,
        1.,
        plan.duration_ms,
        if outcome.version_error.is_some() {
            "MP4 saved. Its project version needs attention.".into()
        } else {
            "MP4 and local project version saved.".into()
        },
        false,
    );
    Ok(outcome)
}

fn preflight(
    store: &ProjectStore,
    snapshot: &Studio,
    plan: &ExportPlan,
    cancel: &Cancellation,
) -> Result<Prepared> {
    let active_clip_ids: std::collections::HashSet<_> =
        snapshot.clips().iter().map(|clip| clip.id()).collect();
    let has_scene = snapshot.cues().iter().any(|cue| {
        active_clip_ids.contains(&cue.clip_id())
            && cue
                .content()
                .is_some_and(|content| content.insert.is_some())
    });
    let mut fingerprints = HashMap::new();
    let mut sources: HashMap<MediaId, SourceInput> = HashMap::new();
    let mut inputs = Vec::new();
    let mut starts = HashMap::new();
    let mut ms = 0_u64;
    let mut previous = 0_u64;
    for clip in snapshot.clips() {
        cancel.check()?;
        starts.insert(clip.id(), ms);
        if let std::collections::hash_map::Entry::Vacant(entry) = sources.entry(clip.source_id()) {
            let source = snapshot
                .media()
                .iter()
                .find(|source| source.id() == clip.source_id())
                .ok_or("Clip source is missing")?;
            let path = local::confined_file(
                store.root(),
                source.path(),
                PathPolicy::LocalProject,
                FileMode::Existing,
            )?;
            fingerprints.insert(path.clone(), Fingerprint::read(&path)?);
            let info = media::probe_owned(&path, cancel)?;
            let size = if has_scene {
                camera_display_size(&path, &info, cancel)?
            } else {
                OutputSize {
                    width: info.width,
                    height: info.height,
                }
            };
            entry.insert(SourceInput { path, info, size });
        }
        let source = sources
            .get(&clip.source_id())
            .ok_or("Clip source was not prepared")?;
        if clip.out_ms() > source.info.duration_ms.saturating_add(34) {
            return Err(
                "A clip extends beyond its actual source video; trim it before exporting".into(),
            );
        }
        ms = ms
            .checked_add(clip.duration_ms())
            .ok_or("Sequence duration overflows")?;
        let end = frame_count(ms)?;
        inputs.push(ClipInput {
            path: source.path.clone(),
            in_ms: clip.in_ms(),
            duration_ms: clip.duration_ms(),
            frames: end - previous,
            has_audio: source.info.has_audio,
            source_size: source.size,
        });
        previous = end;
    }
    let mut schedule = Vec::new();
    let mut edges = Vec::new();
    let mut insert_edges = Vec::new();
    let mut renderer = OverlayRenderer::new()?;
    let mut assets = OverlayAssets::new();
    for cue in snapshot.cues() {
        let Some(start) = starts.get(&cue.clip_id()) else {
            continue;
        };
        cancel.check()?;
        let start = start
            .checked_add(cue.offset_ms())
            .ok_or("Cue time overflows")?;
        let end = start
            .checked_add(cue.duration_ms())
            .ok_or("Cue duration overflows")?;
        let content = cue
            .content()
            .ok_or("A sequence cue has no approved overlay content")?;
        for relative in content.media_paths() {
            let path = local::confined_file(
                store.root(),
                relative,
                PathPolicy::LocalProject,
                FileMode::Existing,
            )?;
            fingerprints
                .entry(path.clone())
                .or_insert(Fingerprint::read(&path)?);
        }
        assets.clear();
        assets.load_content(store, content)?;
        let active = [ActiveOverlay {
            content,
            elapsed_ms: 0,
            duration_ms: cue.duration_ms(),
        }];
        if let Some(insert) = &content.insert {
            if let InsertSource::Video { path, .. } = &insert.source {
                let video = insert::probe_video(store, path, cancel)?;
                if insert
                    .source_frame(cue.duration_ms() - 1)?
                    .ok_or("Missing insert source frame")?
                    >= video.frames
                {
                    return Err("A video insert extends beyond its last frame; shorten the cue before exporting".into());
                }
            }
            insert_edges.push((start, 1_i32));
            insert_edges.push((end, -1_i32));
            assets.prepare_live(store, &active, cancel)?;
            let camera = crate::renderer::RgbaFrame {
                width: 16,
                height: 9,
                pixels: [0, 0, 0, 255].repeat(16 * 9),
            };
            renderer.render_scene_live(
                plan.size,
                &camera,
                &active,
                &assets,
                RenderOptions {
                    reduced_motion: true,
                },
            )?;
        } else {
            renderer.render_live(
                plan.size,
                &active,
                &assets,
                RenderOptions {
                    reduced_motion: true,
                },
            )?;
        }
        schedule.push(Scheduled {
            start,
            end,
            content: content.clone(),
        });
        edges.push((start, 1_i32));
        edges.push((end, -1_i32));
    }
    edges.sort_unstable();
    let mut active = 0;
    for (_, change) in edges {
        active += change;
        if active > 9 {
            return Err(
                "More than nine sequence overlays overlap; shorten or move their cues".into(),
            );
        }
    }
    insert_edges.sort_unstable();
    let mut visible = 0;
    for (_, change) in insert_edges {
        visible += change;
        if visible > 1 {
            return Err("Two visual inserts overlap; move or shorten one before exporting".into());
        }
    }
    Ok(Prepared {
        inputs,
        schedule,
        fingerprints,
    })
}
fn base() -> Result<Command> {
    let mut command = Command::new(media::tool("ffmpeg")?);
    command.args([
        "-hide_banner",
        "-loglevel",
        "error",
        "-nostats",
        "-nostdin",
        "-xerror",
        "-stats_period",
        "0.1",
        "-progress",
        "pipe:1",
        "-filter_threads",
        "1",
        "-filter_complex_threads",
        "1",
    ]);
    Ok(command)
}
fn milliseconds(ms: u64) -> String {
    format!("{}.{:03}", ms / 1000, ms % 1000)
}
fn timeout(ms: u64) -> Duration {
    Duration::from_secs(120).saturating_add(Duration::from_millis(ms).saturating_mul(240))
}
fn normalize_command(
    input: &ClipInput,
    size: OutputSize,
    video: &Path,
    audio: &Path,
    scene: bool,
    encoder: VideoEncoder,
) -> Result<Command> {
    let mut command = base()?;
    command
        .args([
            "-n",
            "-protocol_whitelist",
            "file,pipe",
            "-format_whitelist",
            media::VIDEO_DEMUXERS,
            "-ss",
        ])
        .arg(milliseconds(input.in_ms))
        .arg("-t")
        .arg(milliseconds(input.duration_ms))
        .arg("-i")
        .arg(&input.path);
    if !input.has_audio {
        command.args(["-f", "lavfi", "-i", "anullsrc=r=48000:cl=stereo"]);
    }
    let audio_stream = if input.has_audio { "0:a:0" } else { "1:a:0" };
    let samples = input
        .frames
        .checked_mul(1600)
        .ok_or("Audio sample count overflows")?;
    let geometry = if scene {
        let fit = camera_fit(input.source_size, size)?;
        format!(
            "scale={}:{},pad={}:{}:{}:{}",
            fit.width, fit.height, size.width, size.height, fit.x, fit.y
        )
    } else {
        format!(
            "scale={}:{}:force_original_aspect_ratio=increase,crop={}:{}",
            size.width, size.height, size.width, size.height
        )
    };
    let filter = format!(
        "[0:v:0]setpts=PTS-STARTPTS,fps=30,{geometry},setsar=1,tpad=stop_mode=clone:stop_duration=1[v];[{audio_stream}]asetpts=PTS-STARTPTS,aresample=48000:async=1:first_pts=0,aformat=sample_rates=48000:channel_layouts=stereo,apad,atrim=end_sample={samples}[a]"
    );
    command
        .arg("-filter_complex")
        .arg(filter)
        .args(["-map", "[v]", "-an"]);
    encoder.configure(&mut command, 18, false);
    command
        .args([
            "-pix_fmt",
            "yuv420p",
            "-r",
            "30",
            "-bf",
            "0",
            "-threads",
            "2",
            "-frames:v",
        ])
        .arg(input.frames.to_string())
        .args(["-video_track_timescale", "90000", "-f", "mp4"])
        .arg(video)
        .args([
            "-map",
            "[a]",
            "-vn",
            "-c:a",
            "pcm_s16le",
            "-ar",
            "48000",
            "-ac",
            "2",
            "-rf64",
            "auto",
            "-f",
            "wav",
        ])
        .arg(audio);
    Ok(command)
}
fn final_command(
    video_list: &Path,
    audio_list: &Path,
    output: &Path,
    plan: &ExportPlan,
    scene: bool,
    encoder: VideoEncoder,
) -> Result<Command> {
    let mut command = base()?;
    command.arg("-y");
    if !scene {
        command
            .args([
                "-protocol_whitelist",
                "file,pipe",
                "-f",
                "concat",
                "-safe",
                "1",
                "-i",
            ])
            .arg(video_list);
    }
    command
        .args([
            "-protocol_whitelist",
            "file,pipe",
            "-f",
            "concat",
            "-safe",
            "1",
            "-i",
        ])
        .arg(audio_list);
    command
        .args(["-f", "rawvideo", "-pixel_format", "rgba", "-video_size"])
        .arg(format!("{}x{}", plan.size.width, plan.size.height))
        .args(["-framerate", "30", "-i", "pipe:0"]);
    if scene {
        command.args(["-map", "1:v:0", "-map", "0:a:0"]);
    } else {
        command.args([
            "-filter_complex",
            "[0:v:0][2:v:0]overlay=0:0:shortest=1:alpha=straight:format=auto,format=yuv420p[v]",
            "-map",
            "[v]",
            "-map",
            "1:a:0",
        ]);
    }
    encoder.configure(&mut command, 20, false);
    command
        .args([
            "-pix_fmt",
            "yuv420p",
            "-r",
            "30",
            "-bf",
            "0",
            "-threads",
            "2",
            "-frames:v",
        ])
        .arg(plan.frames.to_string())
        .args([
            "-c:a",
            "aac",
            "-b:a",
            "160k",
            "-ar",
            "48000",
            "-ac",
            "2",
            "-movflags",
            "+faststart",
            "-video_track_timescale",
            "90000",
            "-f",
            "mp4",
        ])
        .arg(output);
    Ok(command)
}
fn camera_display_size(
    path: &Path,
    info: &media::MediaInfo,
    cancel: &Cancellation,
) -> Result<OutputSize> {
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
        Duration::from_secs(30),
        cancel,
        65536,
    )?;
    let value: serde_json::Value =
        serde_json::from_slice(&output.stdout).map_err(|_| "Invalid camera display metadata")?;
    let stream = value["streams"]
        .as_array()
        .and_then(|v| v.first())
        .ok_or("Missing camera display metadata")?;
    let sar = match stream["sample_aspect_ratio"].as_str() {
        None | Some("N/A" | "0:1") => 1.,
        Some(value) => {
            let (a, b) = value.split_once(':').ok_or("Invalid camera pixel aspect")?;
            let a = a.parse::<u32>()?;
            let b = b.parse::<u32>()?;
            if a == 0 || b == 0 {
                return Err("Invalid camera pixel aspect".into());
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
        return Err("Camera video has a non-right-angle display rotation; normalize it before adding an insert".into());
    }
    if !(1.0..=16384.0).contains(&width) || !(1.0..=16384.0).contains(&height) {
        return Err("Camera display geometry exceeds its pixel resource limit".into());
    }
    Ok(OutputSize {
        width: width as u32,
        height: height as u32,
    })
}
#[derive(Clone, Copy, Debug)]
struct CameraFit {
    x: u32,
    y: u32,
    width: u32,
    height: u32,
}
fn camera_fit(source: OutputSize, output: OutputSize) -> Result<CameraFit> {
    insert::dimensions(source)?;
    let scale = (f64::from(output.width) / f64::from(source.width))
        .min(f64::from(output.height) / f64::from(source.height));
    let width = ((f64::from(source.width) * scale / 2.).floor() as u32 * 2)
        .max(2)
        .min(output.width);
    let height = ((f64::from(source.height) * scale / 2.).floor() as u32 * 2)
        .max(2)
        .min(output.height);
    Ok(CameraFit {
        x: (output.width - width) / 4 * 2,
        y: (output.height - height) / 4 * 2,
        width,
        height,
    })
}
fn crop_camera(
    frame: &crate::renderer::RgbaFrame,
    fit: CameraFit,
) -> Result<crate::renderer::RgbaFrame> {
    if fit.x + fit.width > frame.width || fit.y + fit.height > frame.height {
        return Err("Camera content lies outside the normalized frame".into());
    }
    let mut pixels = Vec::with_capacity(fit.width as usize * fit.height as usize * 4);
    for y in fit.y..fit.y + fit.height {
        let start = (y as usize * frame.width as usize + fit.x as usize) * 4;
        let end = start + fit.width as usize * 4;
        pixels.extend_from_slice(
            frame
                .pixels
                .get(start..end)
                .ok_or("Incomplete normalized camera frame")?,
        );
    }
    Ok(crate::renderer::RgbaFrame {
        width: fit.width,
        height: fit.height,
        pixels,
    })
}
fn camera_decode_command(list: &Path) -> Result<Command> {
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
            "-f",
            "concat",
            "-safe",
            "1",
            "-i",
        ])
        .arg(list)
        .args([
            "-map",
            "0:v:0",
            "-an",
            "-pix_fmt",
            "rgba",
            "-fps_mode",
            "passthrough",
            "-f",
            "rawvideo",
            "pipe:1",
        ]);
    Ok(command)
}
fn verify_output(path: &Path, plan: &ExportPlan, cancel: &Cancellation) -> Result<()> {
    let data = media::run_bounded(
        Command::new(media::tool("ffprobe")?)
            .args([
                "-v",
                "error",
                "-protocol_whitelist",
                "file,pipe",
                "-show_entries",
                "stream=codec_type,codec_name,width,height,nb_frames,r_frame_rate,duration",
                "-of",
                "json",
            ])
            .arg(path),
        Duration::from_secs(30),
        cancel,
        2 * 1024 * 1024,
    )?;
    let value: serde_json::Value =
        serde_json::from_slice(&data.stdout).map_err(|_| "Invalid completed export metadata")?;
    let streams = value["streams"]
        .as_array()
        .ok_or("Completed export streams are missing")?;
    let video = streams
        .iter()
        .find(|s| s["codec_type"] == "video")
        .ok_or("Completed export has no video")?;
    let audio = streams
        .iter()
        .find(|s| s["codec_type"] == "audio")
        .ok_or("Completed export has no audio")?;
    if video["codec_name"] != "h264"
        || audio["codec_name"] != "aac"
        || video["width"] != plan.size.width
        || video["height"] != plan.size.height
        || video["r_frame_rate"] != "30/1"
        || video["nb_frames"]
            .as_str()
            .and_then(|s| s.parse::<u64>().ok())
            != Some(plan.frames)
    {
        return Err("Completed MP4 does not match the requested video format/frame count".into());
    }
    let duration = |v: &serde_json::Value| {
        v["duration"]
            .as_str()
            .and_then(|s| s.parse::<f64>().ok())
            .filter(|s| s.is_finite() && *s > 0.)
    };
    let v = duration(video).ok_or("Completed video duration is missing")?;
    let a = duration(audio).ok_or("Completed audio duration is missing")?;
    if (v * 1000. - plan.duration_ms as f64).abs() > 34. || (v - a).abs() > 1. / 30. {
        return Err("Completed MP4 audio/video timing does not match the sequence".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn arguments(command: &Command) -> Vec<String> {
        command
            .get_args()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect()
    }
    fn option(args: &[String], name: &str, value: &str) -> bool {
        args.windows(2)
            .any(|pair| pair[0] == name && pair[1] == value)
    }
    fn input(has_audio: bool) -> ClipInput {
        ClipInput {
            path: PathBuf::from("/generated/source.mp4"),
            in_ms: 200,
            duration_ms: 1000,
            frames: 30,
            has_audio,
            source_size: OutputSize {
                width: 640,
                height: 480,
            },
        }
    }
    fn plan(size: OutputSize) -> ExportPlan {
        ExportPlan {
            size,
            duration_ms: 2000,
            frames: 60,
            clip_count: 2,
            warnings: Vec::new(),
        }
    }
    #[test]
    fn gpu_normalization_selection_preserves_cut_frame_and_audio_contract() {
        for shape in [Orientation::Horizontal, Orientation::Vertical] {
            for scene in [false, true] {
                for has_audio in [false, true] {
                    let args = arguments(
                        &normalize_command(
                            &input(has_audio),
                            shape.output_size(),
                            Path::new("/generated/video.mp4"),
                            Path::new("/generated/audio.wav"),
                            scene,
                            VideoEncoder::Nvidia,
                        )
                        .unwrap(),
                    );
                    assert!(
                        option(&args, "-c:v", "h264_nvenc"),
                        "GPU normalization omitted: expected selected h264_nvenc, command still uses software encoding"
                    );
                    assert!(option(&args, "-ss", "0.200"));
                    assert!(option(&args, "-cq", "18") && !args.iter().any(|arg| arg == "-crf"));
                    assert!(option(&args, "-t", "1.000"));
                    assert!(option(&args, "-frames:v", "30"));
                    assert!(option(&args, "-pix_fmt", "yuv420p") && option(&args, "-r", "30"));
                    assert!(option(&args, "-c:a", "pcm_s16le") && option(&args, "-rf64", "auto"));
                    assert!(option(&args, "-ar", "48000") && option(&args, "-ac", "2"));
                    let filter = &args[args
                        .iter()
                        .position(|arg| arg == "-filter_complex")
                        .unwrap()
                        + 1];
                    assert!(filter.contains("fps=30") && filter.contains("atrim=end_sample=48000"));
                    assert_eq!(filter.contains("[0:a:0]"), has_audio);
                    assert_eq!(
                        args.iter().any(|arg| arg == "anullsrc=r=48000:cl=stereo"),
                        !has_audio
                    );
                }
            }
        }
    }
    #[test]
    fn gpu_final_selection_preserves_exact_frames_audio_and_straight_overlay_contract() {
        for shape in [Orientation::Horizontal, Orientation::Vertical] {
            for scene in [false, true] {
                let args = arguments(
                    &final_command(
                        Path::new("/generated/video.ffconcat"),
                        Path::new("/generated/audio.ffconcat"),
                        Path::new("/generated/output.mp4"),
                        &plan(shape.output_size()),
                        scene,
                        VideoEncoder::Nvidia,
                    )
                    .unwrap(),
                );
                assert!(
                    option(&args, "-c:v", "h264_nvenc"),
                    "GPU final encoding omitted: expected selected h264_nvenc, command still uses software encoding"
                );
                assert!(option(&args, "-frames:v", "60") && option(&args, "-r", "30"));
                assert!(option(&args, "-cq", "20") && !args.iter().any(|arg| arg == "-crf"));
                assert!(option(&args, "-video_track_timescale", "90000"));
                assert!(option(&args, "-c:a", "aac") && option(&args, "-b:a", "160k"));
                assert!(option(&args, "-ar", "48000") && option(&args, "-ac", "2"));
                assert!(option(&args, "-framerate", "30"));
                assert_eq!(
                    args.iter().any(|arg| arg.contains("alpha=straight")),
                    !scene
                );
                assert_eq!(option(&args, "-map", "1:v:0"), scene);
                assert_eq!(option(&args, "-map", "0:a:0"), scene);
                assert!(
                    option(&args, "-safe", "1")
                        && option(&args, "-protocol_whitelist", "file,pipe")
                );
            }
        }
    }
    #[test]
    fn cpu_software_choice_remains_available_for_both_export_stages() {
        for shape in [Orientation::Horizontal, Orientation::Vertical] {
            for scene in [false, true] {
                let normalized = normalize_command(
                    &input(true),
                    shape.output_size(),
                    Path::new("/generated/video.mp4"),
                    Path::new("/generated/audio.wav"),
                    scene,
                    VideoEncoder::Cpu,
                )
                .unwrap();
                let final_video = final_command(
                    Path::new("/generated/video.ffconcat"),
                    Path::new("/generated/audio.ffconcat"),
                    Path::new("/generated/output.mp4"),
                    &plan(shape.output_size()),
                    scene,
                    VideoEncoder::Cpu,
                )
                .unwrap();
                for (command, quality) in [(&normalized, "18"), (&final_video, "20")] {
                    let args = arguments(command);
                    assert!(option(&args, "-c:v", "libx264"));
                    assert!(option(&args, "-preset", "veryfast") && option(&args, "-crf", quality));
                    assert!(
                        !args
                            .iter()
                            .any(|arg| matches!(arg.as_str(), "h264_nvenc" | "-cq"))
                    );
                    assert_eq!(args.iter().filter(|arg| arg.as_str() == "-c:v").count(), 1);
                    assert!(option(&args, "-bf", "0") && option(&args, "-pix_fmt", "yuv420p"));
                }
            }
        }
    }
    #[test]
    fn work_cleanup_removes_only_registered_files_and_keeps_unknown_content() {
        let temp = tempfile::tempdir().unwrap();
        let (store, _) = ProjectStore::create(temp.path().join("project"), "Cleanup").unwrap();
        let mut work = WorkDir::new(&store).unwrap();
        let root = work.root.clone();
        let known = work.file("clip-00000.mp4".into()).unwrap();
        fs::write(&known, b"generated").unwrap();
        fs::write(root.join("user-kept.txt"), b"preserve").unwrap();
        drop(work);
        assert!(!known.exists());
        assert_eq!(fs::read(root.join("user-kept.txt")).unwrap(), b"preserve");
    }
}
