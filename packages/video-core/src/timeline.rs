use crate::{model::validate_media_path, model::validate_name, *};
use std::collections::VecDeque;

pub const HISTORY_LIMIT: usize = 100;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TrimEdge {
    Start,
    End,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Direction {
    Earlier,
    Later,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Nudge {
    Frame,
    Second,
}
#[derive(Clone, Debug)]
pub enum Edit {
    Rename(String),
    SetOrientation(Orientation),
    SetSlotKind {
        slot: SlotKey,
        kind: Option<OverlayKind>,
    },
    AddMedia {
        path: String,
        duration_ms: u64,
    },
    AddClip {
        source: MediaId,
        name: String,
        in_ms: u64,
        out_ms: u64,
    },
    RenameClip {
        clip: ClipId,
        name: String,
    },
    MoveClip {
        clip: ClipId,
        to_index: usize,
    },
    SplitClip {
        clip: ClipId,
        offset_ms: u64,
    },
    SplitAtPlayhead {
        sequence_ms: u64,
    },
    TrimClip {
        clip: ClipId,
        in_ms: u64,
        out_ms: u64,
    },
    NudgeTrim {
        clip: ClipId,
        edge: TrimEdge,
        direction: Direction,
        amount: Nudge,
    },
    RemoveClip {
        clip: ClipId,
    },
    RestoreClip {
        clip: ClipId,
        to_index: usize,
    },
    AddCue {
        clip: ClipId,
        offset_ms: u64,
        slot: SlotKey,
        duration_ms: u64,
    },
    RemoveCue {
        cue: CueId,
    },
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EditOutcome {
    Changed,
    Unchanged,
    MediaAdded(MediaId),
    ClipAdded(ClipId),
    ClipSplit(ClipId),
    CueAdded(CueId),
}

pub struct Editor {
    studio: Studio,
    undo: VecDeque<Studio>,
    redo: VecDeque<Studio>,
}
impl Editor {
    pub fn new(studio: Studio) -> Result<Self> {
        studio.validate()?;
        Ok(Self {
            studio,
            undo: VecDeque::new(),
            redo: VecDeque::new(),
        })
    }
    pub fn studio(&self) -> &Studio {
        &self.studio
    }
    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }
    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }
    pub fn undo_len(&self) -> usize {
        self.undo.len()
    }
    pub fn redo_len(&self) -> usize {
        self.redo.len()
    }
    pub fn apply(&mut self, edit: Edit) -> Result<EditOutcome> {
        Ok(self.apply_batch(vec![edit])?.remove(0))
    }
    /// One atomic edit transaction and one history step. IDs returned by the
    /// current document's next_*_id() may reference earlier additions in a batch.
    pub fn apply_batch(&mut self, edits: Vec<Edit>) -> Result<Vec<EditOutcome>> {
        let mut candidate = self.studio.clone();
        let mut outcomes = Vec::with_capacity(edits.len());
        for edit in edits {
            outcomes.push(candidate.apply_one(edit)?);
            candidate.validate()?;
        }
        if candidate != self.studio {
            push_snapshot(&mut self.undo, self.studio.clone());
            self.redo.clear();
            self.studio = candidate;
        }
        Ok(outcomes)
    }
    pub fn undo(&mut self) -> bool {
        if let Some(mut previous) = self.undo.pop_back() {
            previous.keep_identity_high_water(&self.studio);
            push_snapshot(
                &mut self.redo,
                std::mem::replace(&mut self.studio, previous),
            );
            true
        } else {
            false
        }
    }
    pub fn redo(&mut self) -> bool {
        if let Some(mut next) = self.redo.pop_back() {
            next.keep_identity_high_water(&self.studio);
            push_snapshot(&mut self.undo, std::mem::replace(&mut self.studio, next));
            true
        } else {
            false
        }
    }
}
fn push_snapshot(history: &mut VecDeque<Studio>, studio: Studio) {
    if history.len() == HISTORY_LIMIT {
        history.pop_front();
    }
    history.push_back(studio);
}
impl Studio {
    fn keep_identity_high_water(&mut self, other: &Self) {
        self.next_media = self.next_media.max(other.next_media);
        self.next_clip = self.next_clip.max(other.next_clip);
        self.next_cue = self.next_cue.max(other.next_cue);
    }
    fn clip_index(&self, id: ClipId) -> Result<usize> {
        self.clips
            .iter()
            .position(|clip| clip.id == id)
            .ok_or_else(|| "Active clip does not exist".into())
    }
    fn apply_one(&mut self, edit: Edit) -> Result<EditOutcome> {
        let before = self.clone();
        match edit {
            Edit::Rename(name) => {
                self.rename(&name)?;
            }
            Edit::SetOrientation(orientation) => self.set_orientation(orientation),
            Edit::SetSlotKind { slot, kind } => self.set_slot_kind(slot, kind),
            Edit::AddMedia { path, duration_ms } => {
                validate_media_path(&path)?;
                if duration_ms == 0 || self.media.len() >= MAX_MEDIA {
                    return Err("Invalid media duration or item limit".into());
                }
                if self.media.iter().any(|media| media.path == path) {
                    return Err("Media is already in this project".into());
                }
                let id = self.next_media_id();
                self.next_media += 1;
                self.media.push(MediaSource {
                    id,
                    path,
                    duration_ms,
                });
                return Ok(EditOutcome::MediaAdded(id));
            }
            Edit::AddClip {
                source,
                name,
                in_ms,
                out_ms,
            } => {
                validate_name(&name)?;
                let id = self.next_clip_id();
                self.next_clip += 1;
                self.clips.push(Clip {
                    id,
                    name,
                    source_id: source,
                    in_ms,
                    out_ms,
                });
                return Ok(EditOutcome::ClipAdded(id));
            }
            Edit::RenameClip { clip, name } => {
                validate_name(&name)?;
                let index = self.clip_index(clip)?;
                self.clips[index].name = name;
            }
            Edit::MoveClip { clip, to_index } => {
                let index = self.clip_index(clip)?;
                if to_index >= self.clips.len() {
                    return Err("Clip position is outside the sequence".into());
                }
                let clip = self.clips.remove(index);
                self.clips.insert(to_index, clip);
            }
            Edit::SplitClip { clip, offset_ms } => {
                let index = self.clip_index(clip)?;
                let original = self.clips[index].clone();
                if offset_ms == 0 || offset_ms >= original.duration_ms() {
                    return Err("Split must be inside the clip".into());
                }
                let id = self.next_clip_id();
                self.next_clip += 1;
                let split_source = original.in_ms + offset_ms;
                self.clips[index].out_ms = split_source;
                self.clips.insert(
                    index + 1,
                    Clip {
                        id,
                        in_ms: split_source,
                        ..original
                    },
                );
                for cue in self.cues.iter_mut().filter(|cue| cue.clip_id == clip) {
                    if cue.offset_ms >= offset_ms {
                        cue.clip_id = id;
                        cue.offset_ms -= offset_ms;
                        cue.duration_ms = cue
                            .duration_ms
                            .min(self.clips[index + 1].duration_ms() - cue.offset_ms);
                    } else {
                        cue.duration_ms = cue.duration_ms.min(offset_ms - cue.offset_ms);
                    }
                }
                return Ok(EditOutcome::ClipSplit(id));
            }
            Edit::SplitAtPlayhead { sequence_ms } => {
                let mut start = 0_u64;
                let mut resolved = None;
                for clip in &self.clips {
                    let end = start
                        .checked_add(clip.duration_ms())
                        .ok_or("Sequence duration overflow")?;
                    if sequence_ms >= start && sequence_ms < end {
                        resolved = Some((clip.id, sequence_ms - start));
                        break;
                    }
                    start = end;
                }
                let (clip, offset_ms) = resolved.ok_or("Playhead must be inside an active clip")?;
                return self.apply_one(Edit::SplitClip { clip, offset_ms });
            }
            Edit::TrimClip {
                clip,
                in_ms,
                out_ms,
            } => self.trim(clip, in_ms, out_ms)?,
            Edit::NudgeTrim {
                clip,
                edge,
                direction,
                amount,
            } => {
                let index = self.clip_index(clip)?;
                let current = &self.clips[index];
                let value = match edge {
                    TrimEdge::Start => current.in_ms,
                    TrimEdge::End => current.out_ms,
                };
                let next = match amount {
                    Nudge::Second => match direction {
                        Direction::Earlier => value.checked_sub(1000),
                        Direction::Later => value.checked_add(1000),
                    },
                    Nudge::Frame => {
                        let frame = (u128::from(value) * 30 + 500) / 1000;
                        let next_frame = match direction {
                            Direction::Earlier => frame.checked_sub(1),
                            Direction::Later => frame.checked_add(1),
                        };
                        next_frame.and_then(|frame| u64::try_from((frame * 1000 + 15) / 30).ok())
                    }
                }
                .ok_or("Trim cannot move before its source")?;
                let (start, end) = match edge {
                    TrimEdge::Start => (next, current.out_ms),
                    TrimEdge::End => (current.in_ms, next),
                };
                self.trim(clip, start, end)?;
            }
            Edit::RemoveClip { clip } => {
                let index = self.clip_index(clip)?;
                self.removed.push(RemovedClip {
                    clip: self.clips.remove(index),
                    former_index: index,
                });
            }
            Edit::RestoreClip { clip, to_index } => {
                if to_index > self.clips.len() {
                    return Err("Restore position is outside the sequence".into());
                }
                let index = self
                    .removed
                    .iter()
                    .position(|entry| entry.clip.id == clip)
                    .ok_or("Removed clip does not exist")?;
                self.clips.insert(to_index, self.removed.remove(index).clip);
            }
            Edit::AddCue {
                clip,
                offset_ms,
                slot,
                duration_ms,
            } => {
                self.clip_index(clip)?;
                let kind = self.slots()[usize::from(slot.number() - 1)]
                    .kind()
                    .ok_or("Choose a card type before adding a cue")?;
                let id = self.next_cue_id();
                self.next_cue += 1;
                self.cues.push(Cue {
                    id,
                    clip_id: clip,
                    offset_ms,
                    slot,
                    kind,
                    duration_ms,
                });
                return Ok(EditOutcome::CueAdded(id));
            }
            Edit::RemoveCue { cue } => {
                let index = self
                    .cues
                    .iter()
                    .position(|item| item.id == cue)
                    .ok_or("Cue does not exist")?;
                self.cues.remove(index);
            }
        }
        Ok(if self == &before {
            EditOutcome::Unchanged
        } else {
            EditOutcome::Changed
        })
    }
    fn trim(&mut self, id: ClipId, in_ms: u64, out_ms: u64) -> Result<()> {
        let index = self.clip_index(id)?;
        let old = &self.clips[index];
        let source = self
            .media
            .iter()
            .find(|source| source.id == old.source_id)
            .ok_or("Missing media")?;
        if in_ms >= out_ms || out_ms > source.duration_ms {
            return Err("Trim must leave a positive clip inside its source".into());
        }
        let old_in = old.in_ms;
        let duration = out_ms - in_ms;
        self.clips[index].in_ms = in_ms;
        self.clips[index].out_ms = out_ms;
        for cue in self.cues.iter_mut().filter(|cue| cue.clip_id == id) {
            let source_time = old_in + cue.offset_ms;
            cue.offset_ms = source_time.saturating_sub(in_ms).min(duration - 1);
            cue.duration_ms = cue.duration_ms.min(duration - cue.offset_ms);
        }
        Ok(())
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    pub(crate) fn populated() -> (Editor, ClipId, ClipId, CueId) {
        let mut editor = Editor::new(Studio::default()).unwrap();
        let media = editor.studio().next_media_id();
        let first = editor.studio().next_clip_id();
        editor
            .apply_batch(vec![
                Edit::SetSlotKind {
                    slot: SlotKey::try_from(2).unwrap(),
                    kind: Some(OverlayKind::Quote),
                },
                Edit::AddMedia {
                    path: "media/interview.mp4".into(),
                    duration_ms: 20_000,
                },
                Edit::AddClip {
                    source: media,
                    name: "First".into(),
                    in_ms: 1000,
                    out_ms: 11_000,
                },
            ])
            .unwrap();
        let second = editor.studio().next_clip_id();
        editor
            .apply(Edit::AddClip {
                source: media,
                name: "Second".into(),
                in_ms: 0,
                out_ms: 5000,
            })
            .unwrap();
        let cue = editor.studio().next_cue_id();
        editor
            .apply(Edit::AddCue {
                clip: first,
                offset_ms: 4000,
                slot: SlotKey::try_from(2).unwrap(),
                duration_ms: 3000,
            })
            .unwrap();
        (editor, first, second, cue)
    }
    #[test]
    fn moving_changes_sequence_time_but_cue_stays_with_its_clip() {
        let (mut editor, first, _, cue) = populated();
        assert_eq!(editor.studio().cue_sequence_time(cue).unwrap(), 4000);
        editor
            .apply(Edit::MoveClip {
                clip: first,
                to_index: 1,
            })
            .unwrap();
        assert_eq!(editor.studio().cue_sequence_time(cue).unwrap(), 9000);
        assert_eq!(editor.studio().cues()[0].clip_id(), first);
        editor
            .apply(Edit::SetSlotKind {
                slot: SlotKey::try_from(2).unwrap(),
                kind: Some(OverlayKind::Stat),
            })
            .unwrap();
        assert_eq!(editor.studio().cues()[0].kind(), OverlayKind::Quote);
    }
    #[test]
    fn split_moves_boundary_and_later_cues_and_clamps_crossing_duration() {
        let (mut editor, first, _, cue) = populated();
        editor
            .apply(Edit::AddCue {
                clip: first,
                offset_ms: 2000,
                slot: SlotKey::try_from(2).unwrap(),
                duration_ms: 5000,
            })
            .unwrap();
        let right = match editor
            .apply(Edit::SplitClip {
                clip: first,
                offset_ms: 4000,
            })
            .unwrap()
        {
            EditOutcome::ClipSplit(id) => id,
            _ => unreachable!(),
        };
        let first_cue = editor
            .studio()
            .cues()
            .iter()
            .find(|item| item.id() == cue)
            .unwrap();
        assert_eq!(
            (
                first_cue.clip_id(),
                first_cue.offset_ms(),
                first_cue.duration_ms()
            ),
            (right, 0, 3000)
        );
        assert_eq!(editor.studio().cues()[1].duration_ms(), 2000);
        assert_eq!(editor.studio().duration_ms(), 15_000);
        assert_eq!(editor.studio().cue_sequence_time(cue).unwrap(), 4000);
    }
    #[test]
    fn trim_remaps_source_positions_clamps_both_ends_and_keeps_positive_cues() {
        let (mut editor, first, _, _) = populated();
        editor
            .apply(Edit::TrimClip {
                clip: first,
                in_ms: 3000,
                out_ms: 6000,
            })
            .unwrap();
        assert_eq!(
            (
                editor.studio().cues()[0].offset_ms(),
                editor.studio().cues()[0].duration_ms()
            ),
            (2000, 1000)
        );
        editor
            .apply(Edit::TrimClip {
                clip: first,
                in_ms: 7000,
                out_ms: 8000,
            })
            .unwrap();
        assert_eq!(
            (
                editor.studio().cues()[0].offset_ms(),
                editor.studio().cues()[0].duration_ms()
            ),
            (0, 1000)
        );
        editor
            .apply(Edit::TrimClip {
                clip: first,
                in_ms: 0,
                out_ms: 1,
            })
            .unwrap();
        assert_eq!(
            (
                editor.studio().cues()[0].offset_ms(),
                editor.studio().cues()[0].duration_ms()
            ),
            (0, 1)
        );
    }
    #[test]
    fn removal_retains_sources_and_cues_and_restore_recovers_timing() {
        let (mut editor, first, _, cue) = populated();
        let media = editor.studio().media().to_vec();
        let cues = editor.studio().cues().to_vec();
        editor.apply(Edit::RemoveClip { clip: first }).unwrap();
        assert_eq!(editor.studio().media(), media);
        assert_eq!(editor.studio().cues(), cues);
        assert_eq!(editor.studio().removed_clips()[0].clip().id(), first);
        assert!(editor.studio().cue_sequence_time(cue).is_err());
        editor
            .apply(Edit::RestoreClip {
                clip: first,
                to_index: 1,
            })
            .unwrap();
        assert_eq!(editor.studio().cue_sequence_time(cue).unwrap(), 9000);
    }
    #[test]
    fn grouped_edits_are_atomic_history_is_bounded_and_new_edits_clear_redo() {
        let mut editor = Editor::new(Studio::default()).unwrap();
        let media = editor.studio().next_media_id();
        let clip = editor.studio().next_clip_id();
        editor
            .apply_batch(vec![
                Edit::AddMedia {
                    path: "media/a.mp4".into(),
                    duration_ms: 1000,
                },
                Edit::AddClip {
                    source: media,
                    name: "Take".into(),
                    in_ms: 0,
                    out_ms: 1000,
                },
            ])
            .unwrap();
        assert_eq!(editor.undo_len(), 1);
        assert!(editor.undo());
        assert!(editor.studio().clips().is_empty());
        assert!(editor.studio().next_clip_id().number() > clip.number());
        assert!(editor.redo());
        let before = editor.studio().clone();
        assert!(
            editor
                .apply_batch(vec![
                    Edit::Rename("Unsaved".into()),
                    Edit::TrimClip {
                        clip,
                        in_ms: 1000,
                        out_ms: 0
                    }
                ])
                .is_err()
        );
        assert_eq!(editor.studio(), &before);
        for index in 0..110 {
            editor
                .apply(Edit::Rename(format!("Version {index}")))
                .unwrap();
        }
        assert_eq!(editor.undo_len(), 100);
        for _ in 0..100 {
            assert!(editor.undo());
        }
        assert!(!editor.undo());
        assert_eq!(editor.redo_len(), 100);
        editor.apply(Edit::Rename("Branch".into())).unwrap();
        assert!(!editor.can_redo());
        let history = editor.undo_len();
        assert_eq!(
            editor.apply(Edit::Rename("Branch".into())).unwrap(),
            EditOutcome::Unchanged
        );
        assert_eq!(editor.undo_len(), history);
    }
    #[test]
    fn frame_nudges_use_absolute_thirty_fps_grid_and_seconds_are_exact() {
        let (mut editor, first, _, _) = populated();
        editor
            .apply(Edit::TrimClip {
                clip: first,
                in_ms: 0,
                out_ms: 10_000,
            })
            .unwrap();
        for _ in 0..30 {
            editor
                .apply(Edit::NudgeTrim {
                    clip: first,
                    edge: TrimEdge::Start,
                    direction: Direction::Later,
                    amount: Nudge::Frame,
                })
                .unwrap();
        }
        assert_eq!(editor.studio().clips()[0].in_ms(), 1000);
        editor
            .apply(Edit::NudgeTrim {
                clip: first,
                edge: TrimEdge::End,
                direction: Direction::Earlier,
                amount: Nudge::Second,
            })
            .unwrap();
        assert_eq!(editor.studio().clips()[0].out_ms(), 9000);
    }
    #[test]
    fn sequence_playhead_resolves_in_core_and_boundary_split_is_atomic() {
        let (mut editor, first, second, _) = populated();
        assert_eq!(editor.studio().clip_sequence_time(first).unwrap(), 0);
        assert_eq!(editor.studio().clip_sequence_time(second).unwrap(), 10_000);
        let before = editor.studio().clone();
        assert!(
            editor
                .apply(Edit::SplitAtPlayhead {
                    sequence_ms: 10_000
                })
                .is_err()
        );
        assert_eq!(editor.studio(), &before);
        editor
            .apply(Edit::SplitAtPlayhead {
                sequence_ms: 12_000,
            })
            .unwrap();
        assert_eq!(editor.studio().clips()[1].duration_ms(), 2000);
        assert_eq!(editor.studio().clips()[2].duration_ms(), 3000);
        assert_eq!(editor.studio().duration_ms(), 15_000);
    }
}
