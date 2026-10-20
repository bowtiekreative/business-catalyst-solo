use super::{CaptureSettings, FRAME_RATE, SlotMark};
use crate::{
    Result,
    media::{ImportedMedia, unique_id},
};
use catalyst_core::local::{self, FileMode, PathPolicy, PrivateNamespace, WriteExpected};
use serde::{Deserialize, Serialize};
use std::{
    fs,
    io::{BufRead, BufReader, Read, Seek, SeekFrom},
    path::{Path, PathBuf},
};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ActiveRange {
    pub ordinal: u64,
    pub start: u64,
    pub marks: Vec<SlotMark>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct TakeRange {
    pub ordinal: u64,
    pub start: u64,
    pub end: u64,
    pub marks: Vec<SlotMark>,
    pub media: Option<ImportedMedia>,
    pub error: Option<String>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Manifest {
    version: u32,
    pub id: String,
    pub settings: CaptureSettings,
    pub closed_count: u64,
    pub frame_count: u64,
    pub pruned_before: u64,
    pub active: Option<ActiveRange>,
    pub takes: Vec<TakeRange>,
    pub next_ordinal: u64,
    pub stopped: bool,
}
impl Manifest {
    pub fn new(settings: CaptureSettings) -> Self {
        Self {
            version: 1,
            id: format!("capture-{}", unique_id()),
            settings,
            closed_count: 0,
            frame_count: 0,
            pruned_before: 0,
            active: None,
            takes: Vec::new(),
            next_ordinal: 1,
            stopped: false,
        }
    }
    pub fn relative(&self) -> String {
        format!("buffer/{}", self.id)
    }
    pub fn path(&self, root: &Path, name: &str, mode: FileMode) -> Result<PathBuf> {
        local::confined_file(
            root,
            &format!("{}/{name}", self.relative()),
            PathPolicy::LocalProject,
            mode,
        )
    }
    pub fn create(&self, root: &Path) -> Result<local::SaveLock> {
        local::existing_directory(root, "buffer")?;
        if !valid_id(&self.id) {
            return Err("Invalid capture session identity".into());
        }
        let directory = root.join(self.relative());
        fs::create_dir(&directory)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&directory, fs::Permissions::from_mode(0o700))?;
        }
        let guard = local::lock_named(
            root,
            PrivateNamespace::Sidebar,
            &format!("{}.lock", self.id),
        )?;
        self.save(root)?;
        Ok(guard)
    }
    pub fn save(&self, root: &Path) -> Result<()> {
        self.validate()?;
        let json = serde_json::to_vec(self)?;
        if json.len() > crate::MAX_PROJECT_BYTES {
            return Err("Capture recovery manifest reached its resource limit; existing marks were retained".into());
        }
        catalyst_core::check_secrets(std::str::from_utf8(&json)?)?;
        let path = self.path(root, "manifest.json", FileMode::NewOrExisting)?;
        local::atomic_write(&path, &json, WriteExpected::Any)
    }
    pub fn load(root: &Path, id: &str) -> Result<Self> {
        if !valid_id(id) {
            return Err("Invalid capture recovery identity".into());
        }
        let path = local::confined_file(
            root,
            &format!("buffer/{id}/manifest.json"),
            PathPolicy::LocalProject,
            FileMode::Existing,
        )?;
        if fs::metadata(&path)?.len() > crate::MAX_PROJECT_BYTES as u64 {
            return Err("Capture manifest is too large".into());
        }
        let bytes = fs::read(path)?;
        catalyst_core::check_secrets(std::str::from_utf8(&bytes)?)?;
        let manifest: Self =
            serde_json::from_slice(&bytes).map_err(|_| "Capture manifest is invalid")?;
        if manifest.id != id {
            return Err("Capture manifest identity does not match its folder".into());
        }
        manifest.validate()?;
        Ok(manifest)
    }
    pub fn validate(&self) -> Result<()> {
        if self.version != 1
            || !valid_id(&self.id)
            || self.takes.len() > crate::MAX_CLIPS
            || self.next_ordinal == 0
            || self.pruned_before > self.closed_count
        {
            return Err("Invalid capture recovery bounds".into());
        }
        self.settings.validate(true)?;
        let mut ids = std::collections::HashSet::new();
        for take in &self.takes {
            if take.ordinal == 0
                || take.ordinal >= self.next_ordinal
                || !ids.insert(take.ordinal)
                || take.start >= take.end
                || take.end > self.frame_count
            {
                return Err("Invalid captured take range".into());
            }
            validate_marks(&take.marks)?;
        }
        if let Some(active) = &self.active {
            if active.ordinal == 0
                || active.ordinal >= self.next_ordinal
                || ids.contains(&active.ordinal)
                || active.start > self.frame_count
            {
                return Err("Invalid active capture range".into());
            }
            validate_marks(&active.marks)?;
        }
        Ok(())
    }
    pub fn pending(&self) -> usize {
        self.takes
            .iter()
            .filter(|take| take.media.is_none())
            .count()
    }
    pub fn completed(&self) -> usize {
        self.takes
            .iter()
            .filter(|take| take.media.is_some())
            .count()
    }
    pub fn protected(&self, index: u64) -> bool {
        let start = index.saturating_mul(FRAME_RATE);
        let end = start.saturating_add(FRAME_RATE);
        self.active
            .as_ref()
            .is_some_and(|active| active.start < end)
            || self
                .takes
                .iter()
                .any(|take| take.media.is_none() && take.start < end && take.end > start)
    }
    pub fn prune(&mut self, root: &Path) -> Result<()> {
        let old_before = self.frame_count.saturating_sub(600 * FRAME_RATE);
        while self.pruned_before < self.closed_count {
            let index = self.pruned_before;
            if index.saturating_add(1).saturating_mul(FRAME_RATE) > old_before
                || self.protected(index)
            {
                break;
            }
            let path = self.path(root, &segment_name(index), FileMode::Existing)?;
            fs::remove_file(path)?;
            self.pruned_before += 1;
        }
        Ok(())
    }
}
fn validate_marks(marks: &[SlotMark]) -> Result<()> {
    if marks.len() > crate::MAX_CUES {
        return Err("Capture has too many overlay marks".into());
    }
    for mark in marks {
        mark.content.validate()?;
        if mark.duration_ms == 0 {
            return Err("Overlay mark duration must be positive".into());
        }
    }
    Ok(())
}
pub(crate) fn valid_id(id: &str) -> bool {
    id.starts_with("capture-")
        && id.len() <= 100
        && id[8..].chars().all(|c| c.is_ascii_hexdigit() || c == '-')
        && id.len() > 8
}
pub(crate) fn segment_name(index: u64) -> String {
    format!("segment-{index:09}.ts")
}
/// CSV is consumed incrementally, so an arbitrarily long session doesn't build
/// an ever-growing in-memory index. Only complete lines close a segment.
pub(crate) fn read_closed(root: &Path, manifest: &mut Manifest, cursor: &mut u64) -> Result<bool> {
    let path = manifest.path(root, "segments.csv", FileMode::NewOrExisting)?;
    let Ok(file) = fs::File::open(path) else {
        return Ok(false);
    };
    if file.metadata()?.len() < *cursor {
        *cursor = 0;
    }
    let mut reader = BufReader::new(file);
    reader.seek(SeekFrom::Start(*cursor))?;
    let mut changed = false;
    for _ in 0..4096 {
        let mut line = Vec::new();
        let bytes = reader.by_ref().take(513).read_until(b'\n', &mut line)?;
        if bytes == 0 {
            break;
        }
        if line.len() > 512 {
            return Err("Capture segment index line is too long".into());
        }
        if line.last() != Some(&b'\n') {
            break;
        }
        *cursor += bytes as u64;
        let line = std::str::from_utf8(&line).map_err(|_| "Capture segment index is invalid")?;
        let parts: Vec<_> = line.trim().split(',').collect();
        if parts.len() != 3 {
            return Err("Capture segment index is invalid".into());
        }
        let index = parts[0]
            .strip_prefix("segment-")
            .and_then(|name| name.strip_suffix(".ts"))
            .and_then(|index| index.parse::<u64>().ok())
            .ok_or("Capture segment filename is invalid")?;
        for time in &parts[1..] {
            if !time
                .parse::<f64>()
                .is_ok_and(|time| time.is_finite() && time >= 0.)
            {
                return Err("Capture segment time is invalid".into());
            }
        }
        if index < manifest.closed_count {
            continue;
        }
        if index != manifest.closed_count {
            return Err("Capture segment index has a gap".into());
        }
        manifest.path(root, &segment_name(index), FileMode::Existing)?;
        manifest.closed_count = manifest
            .closed_count
            .checked_add(1)
            .ok_or("Capture index overflow")?;
        changed = true;
    }
    Ok(changed)
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct CutPlan {
    pub first: u64,
    pub last: u64,
    pub first_offset: u64,
    pub last_end: u64,
    pub frames: u64,
}
impl CutPlan {
    pub fn new(start: u64, end: u64) -> Result<Self> {
        if start >= end {
            return Err("Take must contain at least one frame".into());
        }
        let first = start / FRAME_RATE;
        let last = (end - 1) / FRAME_RATE;
        Ok(Self {
            first,
            last,
            first_offset: start % FRAME_RATE,
            last_end: end - last * FRAME_RATE,
            frames: end - start,
        })
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn exact_cut_boundaries_never_include_the_next_segment() {
        assert_eq!(
            CutPlan::new(5, 30).unwrap(),
            CutPlan {
                first: 0,
                last: 0,
                first_offset: 5,
                last_end: 30,
                frames: 25
            }
        );
        assert_eq!(
            CutPlan::new(5, 91).unwrap(),
            CutPlan {
                first: 0,
                last: 3,
                first_offset: 5,
                last_end: 1,
                frames: 86
            }
        );
        assert!(CutPlan::new(30, 30).is_err());
    }
    #[test]
    fn pruning_keeps_pending_and_active_ranges_and_never_touches_unknown_files() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().canonicalize().unwrap();
        local::create_directory(&root, "buffer").unwrap();
        let mut manifest = Manifest::new(CaptureSettings::default());
        let _guard = manifest.create(&root).unwrap();
        manifest.closed_count = 4;
        manifest.frame_count = 605 * 30;
        for index in 0..4 {
            fs::write(
                root.join(manifest.relative()).join(segment_name(index)),
                b"segment",
            )
            .unwrap();
        }
        fs::write(root.join(manifest.relative()).join("user-file.ts"), b"keep").unwrap();
        manifest.next_ordinal = 2;
        manifest.takes.push(TakeRange {
            ordinal: 1,
            start: 30,
            end: 70,
            marks: Vec::new(),
            media: None,
            error: None,
        });
        manifest.prune(&root).unwrap();
        assert_eq!(manifest.pruned_before, 1);
        assert!(
            root.join(manifest.relative())
                .join(segment_name(1))
                .is_file()
        );
        manifest.takes.clear();
        manifest.active = Some(ActiveRange {
            ordinal: 1,
            start: 90,
            marks: Vec::new(),
        });
        manifest.prune(&root).unwrap();
        assert_eq!(manifest.pruned_before, 3);
        assert!(
            root.join(manifest.relative())
                .join("user-file.ts")
                .is_file()
        );
    }
}
