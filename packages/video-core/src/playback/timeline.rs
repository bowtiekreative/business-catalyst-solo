//! Read-only mpv EDL playback with clock-driven shared scene composition.
mod ipc;
mod layers;

use super::*;
use crate::{
    Studio,
    media::Cancellation,
    renderer::{OverlayAssets, OverlayRenderer, RenderOptions},
};
use ipc::Ipc;
use layers::{Bitmap, CameraCursor, VideoRect};
use serde_json::json;
use std::{
    collections::HashMap,
    fs,
    os::unix::net::UnixStream,
    path::PathBuf,
    sync::mpsc,
    thread::{self, JoinHandle},
    time::{Duration, Instant, SystemTime},
};

#[derive(Clone, Debug, PartialEq, Eq)]
struct Stamp {
    len: u64,
    modified: SystemTime,
    dev: u64,
    ino: u64,
}
impl Stamp {
    fn read(path: &std::path::Path) -> Result<Self> {
        let metadata = fs::metadata(path)?;
        use std::os::unix::fs::MetadataExt;
        Ok(Self {
            len: metadata.len(),
            modified: metadata.modified()?,
            dev: metadata.dev(),
            ino: metadata.ino(),
        })
    }
}
struct Source {
    relative: String,
    path: PathBuf,
    stamp: Stamp,
    info: media::MediaInfo,
}
struct Segment {
    source: usize,
    in_ms: u64,
    start_ms: u64,
    end_ms: u64,
}
struct Plan {
    sources: Vec<Source>,
    segments: Vec<Segment>,
    edl: String,
}
impl Plan {
    fn prepare(store: &ProjectStore, snapshot: &Studio, cancel: &Cancellation) -> Result<Self> {
        snapshot.validate()?;
        if snapshot.clips().is_empty() {
            return Err("Add a clip before previewing the sequence".into());
        }
        let clips: std::collections::HashSet<_> =
            snapshot.clips().iter().map(|clip| clip.id()).collect();
        for cue in snapshot
            .cues()
            .iter()
            .filter(|cue| clips.contains(&cue.clip_id()))
        {
            let content = cue.content().ok_or(
                "A sequence cue has no approved overlay content; choose it before previewing",
            )?;
            content.validate()?;
            for relative in content
                .image
                .iter()
                .map(String::as_str)
                .chain(content.insert.iter().map(|insert| insert.path()))
            {
                local::confined_file(
                    store.root(),
                    relative,
                    PathPolicy::LocalProject,
                    FileMode::Existing,
                )?;
            }
        }
        let mut plan = Self {
            sources: Vec::new(),
            segments: Vec::new(),
            edl: "# mpv EDL v0\n!no_chapters\n".into(),
        };
        let mut known = HashMap::new();
        let mut cursor = 0_u64;
        let mut audio_layout = false;
        for clip in snapshot.clips() {
            cancel.check()?;
            let source = if let Some(&index) = known.get(&clip.source_id()) {
                index
            } else {
                let source = snapshot
                    .media()
                    .iter()
                    .find(|source| source.id() == clip.source_id())
                    .ok_or("Clip source is missing")?;
                crate::model::validate_media_path(source.path())?;
                let path = local::confined_file(
                    store.root(),
                    source.path(),
                    PathPolicy::LocalProject,
                    FileMode::Existing,
                )?;
                let stamp = Stamp::read(&path)?;
                let info = media::probe_owned(&path, cancel)?;
                if stamp != Stamp::read(&path)? {
                    return Err("A source changed while opening preview".into());
                }
                let index = plan.sources.len();
                plan.sources.push(Source {
                    relative: source.path().into(),
                    path,
                    stamp,
                    info,
                });
                known.insert(clip.source_id(), index);
                index
            };
            let input = &plan.sources[source];
            if clip.out_ms() > input.info.duration_ms.saturating_add(34) {
                return Err("A clip extends beyond its source; trim it before previewing".into());
            }
            let end = cursor
                .checked_add(clip.duration_ms())
                .ok_or("Sequence time overflows")?;
            let layout = !audio_layout && input.info.has_audio;
            audio_layout |= layout;
            append_edl(
                &mut plan.edl,
                &input.path,
                clip.in_ms(),
                clip.duration_ms(),
                layout,
            )?;
            plan.segments.push(Segment {
                source,
                in_ms: clip.in_ms(),
                start_ms: cursor,
                end_ms: end,
            });
            cursor = end;
        }
        // mpv0.37's file EDL reader is bounded to1MB. Reject explicitly rather
        // than playing only a truncated prefix or silently exporting instead.
        if plan.edl.len() > 999_000 {
            return Err("This direct preview's source list exceeds mpv's1MB EDL limit".into());
        }
        Ok(plan)
    }
    fn at(&self, time_ms: u64) -> (&Source, u64) {
        let index = self
            .segments
            .partition_point(|segment| segment.end_ms <= time_ms)
            .min(self.segments.len() - 1);
        let segment = &self.segments[index];
        let local = time_ms
            .saturating_sub(segment.start_ms)
            .min(segment.end_ms - segment.start_ms - 1);
        (&self.sources[segment.source], segment.in_ms + local)
    }
    fn unchanged(&self, store: &ProjectStore) -> Result<()> {
        for source in &self.sources {
            local::confined_file(
                store.root(),
                &source.relative,
                PathPolicy::LocalProject,
                FileMode::Existing,
            )?;
            if Stamp::read(&source.path)? != source.stamp {
                return Err("A preview source changed; reopen the sequence".into());
            }
        }
        Ok(())
    }
}
fn append_edl(
    edl: &mut String,
    path: &std::path::Path,
    start_ms: u64,
    length_ms: u64,
    layout: bool,
) -> Result<()> {
    let path = path
        .to_str()
        .ok_or("Preview requires UTF8 local source paths")?;
    // Percent length is UTF8 BYTES, not characters. Filenames cannot introduce
    // extra segments, headers, protocols or options into this generated EDL.
    use std::fmt::Write;
    writeln!(
        edl,
        "%{}%{path},{}.{:03},{}.{:03}{}",
        path.len(),
        start_ms / 1000,
        start_ms % 1000,
        length_ms / 1000,
        length_ms % 1000,
        if layout { ",layout=this" } else { "" }
    )?;
    Ok(())
}

pub(super) struct Runtime {
    cancel: Cancellation,
    worker: Option<JoinHandle<std::result::Result<(), String>>>,
    failure: Option<(Instant, String)>,
    _directory: tempfile::TempDir,
    #[cfg(test)]
    pub(super) socket: PathBuf,
}
impl Runtime {
    pub(super) fn cancel(&self) {
        self.cancel.cancel();
    }
    pub(super) fn check(&mut self) -> Result<()> {
        if self.worker.as_ref().is_some_and(JoinHandle::is_finished) {
            let result = self
                .worker
                .take()
                .ok_or("Missing preview worker")?
                .join()
                .unwrap_or_else(|_| Err("Preview composition worker stopped".into()));
            if let Err(error) = result {
                self.failure = Some((Instant::now(), error));
            }
        }
        // Quit closes IPC before process exit is observable. Keep polling the
        // child first without blocking; a still-live composition failure must
        // retain its original error after this bounded shutdown handoff.
        if let Some((since, error)) = &self.failure
            && since.elapsed() >= Duration::from_millis(250)
        {
            return Err(error.clone().into());
        }
        Ok(())
    }
}
impl Drop for Runtime {
    fn drop(&mut self) {
        self.cancel.cancel();
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

pub(super) fn start(
    store: &ProjectStore,
    snapshot: &Studio,
    muted: bool,
    reduced_motion: bool,
) -> Result<SequencePlayer> {
    let cancel = Cancellation::default();
    let plan = Plan::prepare(store, snapshot, &cancel)?;
    let directory = tempfile::Builder::new().prefix("sf-timeline-").tempdir()?;
    let socket = directory.path().join("p.sock");
    let edl = directory.path().join("sequence.edl");
    fs::write(&edl, &plan.edl)?;
    let mut bitmap = Bitmap::new()?;
    let (mut command, controls) = player_base_command(store, muted)?;
    let size = snapshot.orientation().output_size();
    let formats = media::VIDEO_DEMUXERS;
    command.args(["--demuxer=edl", "--pause=yes", "--autoload-files=no", "--ordered-chapters=no", "--audio-display=no"])
        .arg(format!("--demuxer-lavf-o=protocol_whitelist=file,format_whitelist=%{}%{formats},enable_drefs=0,use_absolute_path=0", formats.len()))
        .arg(format!("--vf=lavfi=[scale={}:{}:force_original_aspect_ratio=increase,crop={}:{},setsar=1]", size.width, size.height, size.width, size.height))
        .arg(format!("--input-ipc-server={}", socket.display()));
    #[cfg(test)]
    if std::env::var("STORYFOLD_INPUT_FIXTURE").as_deref() == Ok("1") {
        command.arg("--ao=null");
    }
    bitmap.inherit(&mut command);
    plan.unchanged(store)?;
    let (asset_store, _) = ProjectStore::open(store.root())?;
    let mut player = SequencePlayer {
        child: command
            .arg("--")
            .arg(&edl)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|_| "The native timeline player could not start")?,
        _controls: controls,
        timeline: None,
    };
    let begin = Instant::now();
    let stream = loop {
        if let Ok(stream) = UnixStream::connect(&socket) {
            break stream;
        }
        if player.child.try_wait()?.is_some() {
            return Err("The native timeline player exited during startup".into());
        }
        if begin.elapsed() > Duration::from_secs(10) {
            return Err("The native timeline player did not open its local control channel".into());
        }
        thread::sleep(Duration::from_millis(15));
    };
    let ipc = Ipc::with_cancel(stream, cancel.clone())?;
    let submitted = snapshot.clone();
    let token = cancel.clone();
    let (ready, receiver) = mpsc::sync_channel(1);
    let worker = thread::spawn(move || {
        let result = run(
            ipc,
            &asset_store,
            &submitted,
            &plan,
            &mut bitmap,
            (reduced_motion, &token),
            &ready,
        );
        if let Err(error) = &result {
            let _ = ready.try_send(Err(error.to_string()));
        }
        result.map_err(|error| error.to_string())
    });
    player.timeline = Some(Runtime {
        cancel,
        worker: Some(worker),
        failure: None,
        _directory: directory,
        #[cfg(test)]
        socket,
    });
    match receiver.recv_timeout(Duration::from_secs(10)) {
        Ok(Ok(())) => Ok(player),
        Ok(Err(error)) => Err(error.into()),
        Err(_) => Err("The first timeline preview frame could not be displayed".into()),
    }
}

fn run(
    mut ipc: Ipc,
    store: &ProjectStore,
    snapshot: &Studio,
    plan: &Plan,
    bitmap: &mut Bitmap,
    options: (bool, &Cancellation),
    ready: &mpsc::SyncSender<std::result::Result<(), String>>,
) -> Result<()> {
    let (reduced_motion, cancel) = options;
    let mut renderer = None;
    let mut assets = OverlayAssets::new();
    let mut active_content = Vec::new();
    let mut camera: Option<CameraCursor> = None;
    let mut previous = None;
    let mut announced = false;
    let mut visible = false;
    while !cancel.is_cancelled() {
        let Some(time) = ipc
            .get("time-pos")?
            .as_f64()
            .filter(|time| time.is_finite())
        else {
            thread::sleep(Duration::from_millis(15));
            continue;
        };
        let Some(rect) = VideoRect::parse(&ipc.get("osd-dimensions")?)? else {
            thread::sleep(Duration::from_millis(15));
            continue;
        };
        let ms =
            ((time.max(0.) * 1000.).floor() as u64).min(snapshot.duration_ms().saturating_sub(1));
        let key = (crate::insert::frame_index(ms)?, rect);
        if previous == Some(key) {
            thread::sleep(Duration::from_millis(10));
            continue;
        }
        let active = crate::renderer::active_overlays(snapshot, ms)?;
        if active.is_empty() {
            if visible {
                ipc.call(json!(["overlay-remove", 0]))?;
                visible = false;
            }
            assets.clear();
            active_content.clear();
            camera.take();
        } else {
            if renderer.is_none() {
                renderer = Some(OverlayRenderer::new()?);
            }
            let renderer = renderer.as_mut().ok_or("Missing preview scene renderer")?;
            let same = active_content.len() == active.len()
                && active_content
                    .iter()
                    .zip(&active)
                    .all(|(a, b)| a == b.content);
            if !same {
                assets.clear();
                active_content = active
                    .iter()
                    .map(|overlay| overlay.content.clone())
                    .collect();
            }
            assets.prepare_live(store, &active, cancel)?;
            let insert = active
                .iter()
                .find_map(|overlay| overlay.content.insert.as_ref());
            let rgba = if let Some(insert) = insert {
                let frame = if insert.placement == crate::InsertPlacement::Fullscreen {
                    crate::renderer::RgbaFrame {
                        width: 2,
                        height: 2,
                        pixels: vec![0; 16],
                    }
                } else {
                    let (source, source_ms) = plan.at(ms);
                    local::confined_file(
                        store.root(),
                        &source.relative,
                        PathPolicy::LocalProject,
                        FileMode::Existing,
                    )?;
                    if Stamp::read(&source.path)? != source.stamp {
                        return Err("A source changed during timeline playback".into());
                    }
                    if camera
                        .as_ref()
                        .is_none_or(|cursor| !cursor.matches(&source.path))
                    {
                        camera = Some(CameraCursor::new(&source.path, &source.info, cancel)?);
                    }
                    camera
                        .as_mut()
                        .ok_or("Missing source decoder")?
                        .frame(source_ms, cancel)?
                        .clone()
                };
                renderer.render_scene_live(
                    snapshot.orientation().output_size(),
                    &frame,
                    &active,
                    &assets,
                    RenderOptions { reduced_motion },
                )?
            } else {
                camera.take();
                renderer.render_timeline(snapshot, ms, &assets, RenderOptions { reduced_motion })?
            };
            bitmap.show(&mut ipc, &rgba, rect)?;
            visible = true;
        }
        previous = Some(key);
        if !announced {
            ipc.call(json!(["set_property", "pause", false]))?;
            ready
                .send(Ok(()))
                .map_err(|_| "Preview startup was abandoned")?;
            announced = true;
        }
        thread::sleep(Duration::from_millis(5));
    }
    Ok(())
}

#[cfg(test)]
mod tests;
