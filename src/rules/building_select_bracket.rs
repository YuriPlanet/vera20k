//! Phobos `BuildingSelectBracketPosition` (`Enum.h:310..318`): the corner of
//! the building selection bracket a `TechnoExt::GetBuildingSelectBracketPosition`
//! caller anchors to.

/// Phobos `BuildingSelectBracketPosition`.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, Default, serde::Serialize, serde::Deserialize,
)]
pub enum BuildingSelectBracketPosition {
    #[default]
    Top,
    LeftTop,
    LeftBottom,
    Bottom,
    RightBottom,
    RightTop,
}

impl BuildingSelectBracketPosition {
    pub(crate) fn parse(raw: &str) -> Option<Self> {
        Some(match raw.to_ascii_lowercase().as_str() {
            "top" => Self::Top,
            "lefttop" => Self::LeftTop,
            "leftbottom" => Self::LeftBottom,
            "bottom" => Self::Bottom,
            "rightbottom" => Self::RightBottom,
            "righttop" => Self::RightTop,
            _ => return None,
        })
    }
}
