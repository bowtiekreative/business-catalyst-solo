//! Sidebar Video milestone1: in-memory studio settings, output geometry and slot identities.
//! No capture, persistence, inference, network or subprocesses are implemented here yet.
use std::fmt;

pub const SLOT_COUNT: usize = 9;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Orientation {
    #[default]
    Horizontal,
    Vertical,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
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
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct SlotKey(u8);
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
    fn try_from(number: u8) -> Result<Self, Self::Error> {
        if (1..=SLOT_COUNT as u8).contains(&number) {
            Ok(Self(number))
        } else {
            Err(InvalidSlot)
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OverlaySlot {
    key: SlotKey,
    kind: Option<OverlayKind>,
}
impl OverlaySlot {
    pub const fn key(self) -> SlotKey {
        self.key
    }
    pub const fn kind(self) -> Option<OverlayKind> {
        self.kind
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Studio {
    name: String,
    orientation: Orientation,
    slots: [OverlaySlot; SLOT_COUNT],
}
impl Default for Studio {
    fn default() -> Self {
        Self {
            name: "Untitled recording".into(),
            orientation: Orientation::Horizontal,
            slots: std::array::from_fn(|index| OverlaySlot {
                key: SlotKey(index as u8 + 1),
                kind: None,
            }),
        }
    }
}
impl Studio {
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
        self.slots[key.index()].kind = kind;
    }
    pub fn rename(&mut self, name: &str) -> Result<(), NameError> {
        let name = name.trim();
        if name.is_empty() || name.chars().count() > 120 {
            return Err(NameError::Length);
        }
        catalyst_core::check_secrets(name).map_err(|_| NameError::Sensitive)?;
        self.name = name.into();
        Ok(())
    }
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
        let slots = *studio.slots();
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
}
