//! Serializable project data. Constructors/edits/load validate references and bounds.
use crate::{OverlayContent, Result};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::fmt;

pub const SLOT_COUNT: usize = 9;
pub const MAX_MEDIA: usize = 10_000;
pub const MAX_CLIPS: usize = 10_000;
pub const MAX_CUES: usize = 100_000;
pub const MAX_PROJECT_BYTES: usize = 8 * 1024 * 1024;
pub(crate) const MAX_ID: u64 = u64::MAX;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Orientation {
    #[default]
    Horizontal,
    Vertical,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OutputSize {
    pub width: u32,
    pub height: u32,
}
impl Orientation {
    pub const fn output_size(self) -> OutputSize {
        match self {
            Self::Horizontal => OutputSize {
                width: 1920,
                height: 1080,
            },
            Self::Vertical => OutputSize {
                width: 1080,
                height: 1920,
            },
        }
    }
    pub const fn label(self) -> &'static str {
        match self {
            Self::Horizontal => "Horizontal 16:9",
            Self::Vertical => "Vertical 9:16",
        }
    }
}
impl OutputSize {
    pub fn aspect_ratio(self) -> f32 {
        self.width as f32 / self.height as f32
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OverlayKind {
    LowerThird,
    Stat,
    ScreenshotCard,
    NumberedList,
    Photo,
    Quote,
}
impl OverlayKind {
    pub const fn label(self) -> &'static str {
        match self {
            Self::LowerThird => "Lower third",
            Self::Stat => "Statistic",
            Self::ScreenshotCard => "Screenshot card",
            Self::NumberedList => "Numbered list",
            Self::Photo => "Photo",
            Self::Quote => "Quote",
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(try_from = "u8", into = "u8")]
pub struct SlotKey(u8);
impl From<SlotKey> for u8 {
    fn from(key: SlotKey) -> Self {
        key.number()
    }
}
impl SlotKey {
    pub const fn number(self) -> u8 {
        self.0
    }
    fn index(self) -> usize {
        usize::from(self.0 - 1)
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InvalidSlot;
impl fmt::Display for InvalidSlot {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Choose an overlay slot from 1 to 9.")
    }
}
impl std::error::Error for InvalidSlot {}
impl TryFrom<u8> for SlotKey {
    type Error = InvalidSlot;
    fn try_from(number: u8) -> std::result::Result<Self, Self::Error> {
        if (1..=SLOT_COUNT as u8).contains(&number) {
            Ok(Self(number))
        } else {
            Err(InvalidSlot)
        }
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OverlaySlot {
    key: SlotKey,
    kind: Option<OverlayKind>,
    #[serde(default)]
    pub(crate) content: Option<OverlayContent>,
}
impl OverlaySlot {
    pub fn content(&self) -> Option<&OverlayContent> {
        self.content.as_ref()
    }
    pub const fn key(&self) -> SlotKey {
        self.key
    }
    pub const fn kind(&self) -> Option<OverlayKind> {
        self.kind
    }
}

macro_rules! identity {
    ($name:ident) => {
        #[derive(
            Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize,
        )]
        #[serde(try_from = "u64", into = "u64")]
        pub struct $name(pub(crate) u64);
        impl $name {
            pub const fn number(self) -> u64 {
                self.0
            }
        }
        impl From<$name> for u64 {
            fn from(id: $name) -> Self {
                id.number()
            }
        }
        impl TryFrom<u64> for $name {
            type Error = Box<dyn std::error::Error + Send + Sync>;
            fn try_from(value: u64) -> Result<Self> {
                if value == 0 || value == MAX_ID {
                    return Err("Invalid project identity".into());
                }
                Ok(Self(value))
            }
        }
    };
}
identity!(MediaId);
identity!(ClipId);
identity!(CueId);

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MediaSource {
    pub(crate) id: MediaId,
    pub(crate) path: String,
    pub(crate) duration_ms: u64,
}
impl MediaSource {
    pub const fn id(&self) -> MediaId {
        self.id
    }
    pub fn path(&self) -> &str {
        &self.path
    }
    pub const fn duration_ms(&self) -> u64 {
        self.duration_ms
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Clip {
    pub(crate) id: ClipId,
    pub(crate) name: String,
    pub(crate) source_id: MediaId,
    pub(crate) in_ms: u64,
    pub(crate) out_ms: u64,
}
impl Clip {
    pub const fn id(&self) -> ClipId {
        self.id
    }
    pub fn name(&self) -> &str {
        &self.name
    }
    pub const fn source_id(&self) -> MediaId {
        self.source_id
    }
    pub const fn in_ms(&self) -> u64 {
        self.in_ms
    }
    pub const fn out_ms(&self) -> u64 {
        self.out_ms
    }
    pub fn duration_ms(&self) -> u64 {
        self.out_ms - self.in_ms
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RemovedClip {
    pub(crate) clip: Clip,
    pub(crate) former_index: usize,
}
impl RemovedClip {
    pub fn clip(&self) -> &Clip {
        &self.clip
    }
    pub const fn former_index(&self) -> usize {
        self.former_index
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Cue {
    pub(crate) id: CueId,
    pub(crate) clip_id: ClipId,
    pub(crate) offset_ms: u64,
    pub(crate) slot: SlotKey,
    pub(crate) kind: OverlayKind,
    pub(crate) duration_ms: u64,
    #[serde(default)]
    pub(crate) content: Option<OverlayContent>,
}
impl Cue {
    pub const fn id(&self) -> CueId {
        self.id
    }
    pub const fn clip_id(&self) -> ClipId {
        self.clip_id
    }
    pub const fn offset_ms(&self) -> u64 {
        self.offset_ms
    }
    pub const fn slot(&self) -> SlotKey {
        self.slot
    }
    /// The cue keeps its own approved snapshot, independently of later slot edits.
    pub fn content(&self) -> Option<&OverlayContent> {
        self.content.as_ref()
    }
    pub const fn kind(&self) -> OverlayKind {
        self.kind
    }
    pub const fn duration_ms(&self) -> u64 {
        self.duration_ms
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Studio {
    pub(crate) version: u32,
    name: String,
    orientation: Orientation,
    slots: [OverlaySlot; SLOT_COUNT],
    pub(crate) media: Vec<MediaSource>,
    pub(crate) clips: Vec<Clip>,
    pub(crate) removed: Vec<RemovedClip>,
    pub(crate) cues: Vec<Cue>,
    pub(crate) next_media: u64,
    pub(crate) next_clip: u64,
    pub(crate) next_cue: u64,
}
impl Default for Studio {
    fn default() -> Self {
        Self {
            version: 1,
            name: "Untitled recording".into(),
            orientation: Orientation::Horizontal,
            slots: std::array::from_fn(|index| OverlaySlot {
                key: SlotKey(index as u8 + 1),
                kind: None,
                content: None,
            }),
            media: Vec::new(),
            clips: Vec::new(),
            removed: Vec::new(),
            cues: Vec::new(),
            next_media: 1,
            next_clip: 1,
            next_cue: 1,
        }
    }
}
impl Studio {
    pub fn media(&self) -> &[MediaSource] {
        &self.media
    }
    pub fn clips(&self) -> &[Clip] {
        &self.clips
    }
    pub fn removed_clips(&self) -> &[RemovedClip] {
        &self.removed
    }
    pub fn cues(&self) -> &[Cue] {
        &self.cues
    }
    pub const fn next_media_id(&self) -> MediaId {
        MediaId(self.next_media)
    }
    pub const fn next_clip_id(&self) -> ClipId {
        ClipId(self.next_clip)
    }
    pub const fn next_cue_id(&self) -> CueId {
        CueId(self.next_cue)
    }
    pub fn duration_ms(&self) -> u64 {
        self.clips
            .iter()
            .fold(0, |sum, clip| sum.saturating_add(clip.duration_ms()))
    }
    pub fn cue_sequence_time(&self, id: CueId) -> Result<u64> {
        let cue = self
            .cues
            .iter()
            .find(|cue| cue.id == id)
            .ok_or("Cue does not exist")?;
        self.clip_sequence_time(cue.clip_id)?
            .checked_add(cue.offset_ms)
            .ok_or_else(|| "Sequence time overflows milliseconds".into())
    }
    pub fn clip_sequence_time(&self, id: ClipId) -> Result<u64> {
        let mut start = 0_u64;
        for clip in &self.clips {
            if clip.id == id {
                return Ok(start);
            }
            start = start
                .checked_add(clip.duration_ms())
                .ok_or("Sequence time overflows milliseconds")?;
        }
        Err("Only active clips have sequence time".into())
    }
    pub fn to_json(&self) -> Result<String> {
        self.validate()?;
        let json = serde_json::to_string_pretty(self)?;
        if json.len() > MAX_PROJECT_BYTES {
            return Err("Project JSON is too large".into());
        }
        catalyst_core::check_secrets(&json)?;
        Ok(json)
    }
    pub fn from_json(json: &str) -> Result<Self> {
        if json.len() > MAX_PROJECT_BYTES {
            return Err("Project JSON is too large".into());
        }
        catalyst_core::check_secrets(json)?;
        let studio: Self = serde_json::from_str(json)
            .map_err(|_| "Project JSON is invalid or has unknown fields")?;
        studio.validate()?;
        Ok(studio)
    }
    pub fn validate(&self) -> Result<()> {
        if self.version != 1 {
            return Err("Unsupported project version".into());
        }
        validate_name(&self.name)?;
        if self.media.len() > MAX_MEDIA
            || self.clips.len() + self.removed.len() > MAX_CLIPS
            || self.cues.len() > MAX_CUES
        {
            return Err("Project exceeds supported item limits".into());
        }
        for (index, slot) in self.slots.iter().enumerate() {
            if let Some(content) = &slot.content {
                content.validate()?;
                if slot.kind != Some(content.kind) {
                    return Err("Overlay slot kind does not match content".into());
                }
            }
            if usize::from(slot.key.number()) != index + 1 {
                return Err("Invalid overlay slot identities".into());
            }
        }
        for next in [self.next_media, self.next_clip, self.next_cue] {
            if next == 0 || next == MAX_ID {
                return Err("Invalid next project identity".into());
            }
        }
        let mut ids = HashSet::new();
        let mut media_by_id = HashMap::new();
        let mut paths = HashSet::new();
        for media in &self.media {
            if media.id.0 == 0
                || media.id.0 >= self.next_media
                || !ids.insert(media.id)
                || !paths.insert(&media.path)
            {
                return Err("Duplicate or invalid media identity/path".into());
            }
            validate_media_path(&media.path)?;
            if media.duration_ms == 0 {
                return Err("Invalid media duration".into());
            }
            media_by_id.insert(media.id, media);
        }
        let mut clip_ids = HashSet::new();
        let mut clips_by_id = HashMap::new();
        for clip in self
            .clips
            .iter()
            .chain(self.removed.iter().map(|entry| &entry.clip))
        {
            let media = media_by_id
                .get(&clip.source_id)
                .ok_or("Clip references missing media")?;
            if clip.id.0 == 0 || clip.id.0 >= self.next_clip || !clip_ids.insert(clip.id) {
                return Err("Duplicate or invalid clip identity".into());
            }
            validate_name(&clip.name)?;
            if clip.in_ms >= clip.out_ms || clip.out_ms > media.duration_ms {
                return Err("Clip bounds exceed its source".into());
            }
            clips_by_id.insert(clip.id, clip);
        }
        if self
            .removed
            .iter()
            .any(|entry| entry.former_index >= MAX_CLIPS)
            || self
                .clips
                .iter()
                .try_fold(0_u64, |sum, clip| sum.checked_add(clip.duration_ms()))
                .is_none()
        {
            return Err("Sequence exceeds supported bounds".into());
        }
        let mut cue_ids = HashSet::new();
        for cue in &self.cues {
            let clip = clips_by_id
                .get(&cue.clip_id)
                .ok_or("Cue references missing clip")?;
            if cue.id.0 == 0 || cue.id.0 >= self.next_cue || !cue_ids.insert(cue.id) {
                return Err("Duplicate or invalid cue identity".into());
            }
            SlotKey::try_from(cue.slot.number())?;
            if let Some(content) = &cue.content {
                content.validate()?;
                if cue.kind != content.kind {
                    return Err("Cue kind does not match its immutable content".into());
                }
            }
            if cue.offset_ms >= clip.duration_ms()
                || cue.duration_ms == 0
                || cue.duration_ms > clip.duration_ms() - cue.offset_ms
            {
                return Err("Cue exceeds its clip bounds".into());
            }
        }
        if serde_json::to_vec(self)?.len() > MAX_PROJECT_BYTES {
            return Err("Project JSON is too large".into());
        }
        Ok(())
    }
    pub fn name(&self) -> &str {
        &self.name
    }
    pub const fn orientation(&self) -> Orientation {
        self.orientation
    }
    pub fn set_orientation(&mut self, orientation: Orientation) {
        self.orientation = orientation;
    }
    pub const fn slots(&self) -> &[OverlaySlot; SLOT_COUNT] {
        &self.slots
    }
    pub fn set_slot_kind(&mut self, key: SlotKey, kind: Option<OverlayKind>) {
        let slot = &mut self.slots[key.index()];
        slot.kind = kind;
        if slot.content.as_ref().is_some_and(|c| Some(c.kind) != kind) {
            slot.content = None;
        }
    }
    pub(crate) fn set_slot_content(&mut self, key: SlotKey, content: OverlayContent) -> Result<()> {
        content.validate()?;
        let slot = &mut self.slots[key.index()];
        slot.kind = Some(content.kind);
        slot.content = Some(content);
        Ok(())
    }
    pub fn rename(&mut self, name: &str) -> std::result::Result<(), NameError> {
        let name = name.trim();
        if name.is_empty() || name.chars().count() > 120 || name.chars().any(char::is_control) {
            return Err(NameError::Length);
        }
        catalyst_core::check_secrets(name).map_err(|_| NameError::Sensitive)?;
        self.name = name.into();
        Ok(())
    }
}
pub(crate) fn validate_name(name: &str) -> Result<()> {
    if name.trim() != name
        || name.is_empty()
        || name.chars().count() > 120
        || name.chars().any(char::is_control)
    {
        return Err("Use a name from 1 to 120 characters without control characters".into());
    }
    catalyst_core::check_secrets(name)
}
pub(crate) fn validate_media_path(path: &str) -> Result<()> {
    catalyst_core::local::validate_relative_path(path)?;
    if !path.starts_with("media/") || path.len() <= 6 {
        return Err("Media must stay inside the project's media folder".into());
    }
    catalyst_core::check_secrets(path)
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NameError {
    Length,
    Sensitive,
}
impl fmt::Display for NameError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Length => "Use a recording name from 1 to 120 characters.",
            Self::Sensitive => "Keep credentials out of the recording name.",
        })
    }
}
impl std::error::Error for NameError {}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn shape_changes_exact_output_dimensions_without_touching_slots_or_name() {
        let mut studio = Studio::default();
        let slots = studio.slots().clone();
        let name = studio.name().to_owned();
        assert_eq!(
            studio.orientation().output_size(),
            OutputSize {
                width: 1920,
                height: 1080
            }
        );
        studio.set_orientation(Orientation::Vertical);
        assert_eq!(
            studio.orientation().output_size(),
            OutputSize {
                width: 1080,
                height: 1920
            }
        );
        assert!(
            (studio.orientation().output_size().aspect_ratio() - 9. / 16.).abs() < f32::EPSILON
        );
        assert_eq!(studio.name(), name);
        assert_eq!(*studio.slots(), slots);
    }
    #[test]
    fn nine_empty_slots_keep_stable_one_based_keys_and_reject_outside_keys() {
        let mut studio = Studio::default();
        for (index, slot) in studio.slots().iter().enumerate() {
            assert_eq!(usize::from(slot.key().number()), index + 1);
            assert_eq!(slot.kind(), None);
        }
        for number in [0, 10, 255] {
            assert!(SlotKey::try_from(number).is_err());
        }
        let key = SlotKey::try_from(9).unwrap();
        studio.set_slot_kind(key, Some(OverlayKind::Quote));
        assert_eq!(studio.slots()[8].kind(), Some(OverlayKind::Quote));
        assert!(studio.slots()[..8].iter().all(|s| s.kind().is_none()));
    }
    #[test]
    fn invalid_name_cannot_replace_the_previous_value_and_secret_scan_is_reused() {
        let mut studio = Studio::default();
        studio.rename("  A fresh take  ").unwrap();
        assert_eq!(studio.name(), "A fresh take");
        for bad in [
            String::new(),
            "x".repeat(121),
            format!("OPENAI_API_KEY=sk-{}", "a".repeat(40)),
        ] {
            assert!(studio.rename(&bad).is_err());
            assert_eq!(studio.name(), "A fresh take");
        }
    }
    #[test]
    fn project_json_rejects_unknown_fields_bad_references_bounds_and_secrets() {
        let (editor, _, _, _) = crate::timeline::tests::populated();
        let original = serde_json::to_value(editor.studio()).unwrap();
        let mut variants = Vec::new();
        let mut bad = original.clone();
        bad["unknown"] = true.into();
        variants.push(bad);
        let mut bad = original.clone();
        bad["media"][0]["id"] = 0.into();
        variants.push(bad);
        let mut bad = original.clone();
        bad["media"][0]["path"] = "media/../../outside".into();
        variants.push(bad);
        let mut bad = original.clone();
        bad["clips"][1]["id"] = bad["clips"][0]["id"].clone();
        variants.push(bad);
        let mut bad = original.clone();
        bad["clips"][0]["source_id"] = 999.into();
        variants.push(bad);
        let mut bad = original.clone();
        bad["clips"][0]["out_ms"] = 0.into();
        variants.push(bad);
        let mut bad = original.clone();
        bad["clips"][0]["out_ms"] = 21_000.into();
        variants.push(bad);
        let mut bad = original.clone();
        bad["slots"][0]["key"] = 9.into();
        variants.push(bad);
        let mut bad = original.clone();
        bad["cues"][0]["clip_id"] = 999.into();
        variants.push(bad);
        let mut bad = original.clone();
        bad["cues"][0]["offset_ms"] = 10_000.into();
        variants.push(bad);
        let mut bad = original.clone();
        bad["cues"][0]["duration_ms"] = u64::MAX.into();
        variants.push(bad);
        let mut bad = original.clone();
        bad["next_clip"] = 1.into();
        variants.push(bad);
        let mut bad = original.clone();
        bad["name"] = format!("OPENAI_API_KEY=sk-{}", "a".repeat(40)).into();
        variants.push(bad);
        for variant in variants {
            assert!(Studio::from_json(&variant.to_string()).is_err());
        }
        assert!(serde_json::from_str::<SlotKey>("0").is_err());
        assert!(serde_json::from_str::<ClipId>("0").is_err());
        assert!(Studio::from_json(&" ".repeat(MAX_PROJECT_BYTES + 1)).is_err());
        assert_eq!(
            Studio::from_json(&editor.studio().to_json().unwrap()).unwrap(),
            *editor.studio()
        );
    }
    #[test]
    fn long_sources_are_supported_and_sequence_sum_overflow_is_rejected() {
        let mut editor = crate::Editor::new(Studio::default()).unwrap();
        let source = editor.studio().next_media_id();
        editor
            .apply(crate::Edit::AddMedia {
                path: "media/long.mp4".into(),
                duration_ms: u64::MAX,
            })
            .unwrap();
        editor
            .apply(crate::Edit::AddClip {
                source,
                name: "Long".into(),
                in_ms: 0,
                out_ms: u64::MAX,
            })
            .unwrap();
        assert_eq!(editor.studio().duration_ms(), u64::MAX);
        let before = editor.studio().clone();
        assert!(
            editor
                .apply(crate::Edit::AddClip {
                    source,
                    name: "Overflow".into(),
                    in_ms: 0,
                    out_ms: 1
                })
                .is_err()
        );
        assert_eq!(editor.studio(), &before);
        assert!(editor.studio().to_json().is_ok());
    }
}
