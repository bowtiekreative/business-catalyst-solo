//! Explicit, cancellable sequence MP4 export. Call from a worker, retaining the
//! ProjectStore baseline from job start. No camera, inference, network or upload.
mod pipeline;
mod process;

use crate::{
    Orientation, OutputSize, ProjectStore, Result, SaveVersion, Studio,
    media::{self, Cancellation},
};
use catalyst_core::local::{self, FileMode, PathPolicy};
use std::{collections::HashSet, path::PathBuf};

#[derive(Clone, Debug, Default)]
pub struct ExportOptions {
    /// One normal .mp4 leaf inside the project's exports/ directory. None picks
    /// a unique generated name. Existing exports are never overwritten.
    pub file_name: Option<String>,
    pub reduced_motion: bool,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExportPhase {
    Preparing,
    PreparingClips,
    Encoding,
    Finalizing,
    SavingVersion,
    Completed,
}
#[derive(Clone, Debug)]
pub struct ExportProgress {
    pub phase: ExportPhase,
    pub fraction: f32,
    pub completed_ms: u64,
    pub total_ms: u64,
    pub message: String,
    pub cancellable: bool,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExportWarning {
    VerticalLongerThanThreeMinutes,
}
#[derive(Clone, Debug)]
pub struct ExportPlan {
    pub size: OutputSize,
    pub duration_ms: u64,
    pub frames: u64,
    pub clip_count: usize,
    pub warnings: Vec<ExportWarning>,
}
impl ExportPlan {
    pub fn new(studio: &Studio) -> Result<Self> {
        studio.validate()?;
        if studio.clips().is_empty() {
            return Err("Add a clip to the sequence before exporting".into());
        }
        let frames = frame_count(studio.duration_ms())?;
        let mut ms = 0_u64;
        let mut previous = 0_u64;
        for clip in studio.clips() {
            ms = ms
                .checked_add(clip.duration_ms())
                .ok_or("Sequence duration overflows milliseconds")?;
            let end = frame_count(ms)?;
            if end == previous {
                return Err(
                    "A clip is shorter than one output frame; extend it before exporting".into(),
                );
            }
            previous = end;
        }
        // Media PTS uses a 90kHz clock, 3000 ticks per 30fps frame.
        if frames == 0 || frames > i64::MAX as u64 / 3000 {
            return Err("Sequence exceeds the media timestamp range".into());
        }
        let active_clips: HashSet<_> = studio.clips().iter().map(|clip| clip.id()).collect();
        for cue in studio
            .cues()
            .iter()
            .filter(|cue| active_clips.contains(&cue.clip_id()))
        {
            cue.content().ok_or("A sequence cue has no approved overlay content; choose its content before exporting")?.validate()?;
        }
        let warnings =
            if studio.orientation() == Orientation::Vertical && frame_ms(frames) > 180_000 {
                vec![ExportWarning::VerticalLongerThanThreeMinutes]
            } else {
                Vec::new()
            };
        Ok(Self {
            size: studio.orientation().output_size(),
            duration_ms: frame_ms(frames),
            frames,
            clip_count: studio.clips().len(),
            warnings,
        })
    }
}
#[derive(Clone, Debug)]
pub struct ExportArtifact {
    pub relative_path: String,
    pub duration_ms: u64,
    pub frames: u64,
    pub size: OutputSize,
}
impl ExportArtifact {
    pub fn path(&self, store: &ProjectStore) -> Result<PathBuf> {
        if !self.relative_path.starts_with("exports/") {
            return Err("Invalid export location".into());
        }
        local::confined_file(
            store.root(),
            &self.relative_path,
            PathPolicy::LocalProject,
            FileMode::Existing,
        )
    }
}
#[derive(Clone, Debug)]
pub struct ExportOutcome {
    pub artifact: ExportArtifact,
    pub project_saved: bool,
    pub version: Option<SaveVersion>,
    pub version_error: Option<String>,
}
pub fn export_sequence(
    store: &mut ProjectStore,
    snapshot: &Studio,
    options: ExportOptions,
    cancel: &Cancellation,
    progress: impl FnMut(ExportProgress),
) -> Result<ExportOutcome> {
    pipeline::export(store, snapshot, options, cancel, progress)
}
fn frame_count(ms: u64) -> Result<u64> {
    u64::try_from((u128::from(ms) * 30 + 500) / 1000)
        .map_err(|_| "Sequence frame count overflows".into())
}
fn frame_ms(frame: u64) -> u64 {
    ((u128::from(frame) * 1000 + 15) / 30).min(u128::from(u64::MAX)) as u64
}
fn output_relative(options: &ExportOptions) -> Result<String> {
    let name = options
        .file_name
        .clone()
        .unwrap_or_else(|| format!("storyfold-{}.mp4", media::unique_id()));
    local::validate_relative_path(&name)?;
    if name.contains('/') || !name.to_ascii_lowercase().ends_with(".mp4") {
        return Err("Choose one MP4 filename inside this project's exports folder".into());
    }
    catalyst_core::check_secrets(&name)?;
    Ok(format!("exports/{name}"))
}

#[cfg(test)]
mod tests;
