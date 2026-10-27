//! Installed Ares TypeExt1003C710 / LoadFromINI1003E232..1003E33A.
//! The process registry retains each read; ObjectType only projects it.
use super::ini_parser::IniSection;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub(crate) struct SpotlightTypeRules {
    start_height: i32,
    distance: i32,
    attached_to: u8,
    disable_red: bool,
    disable_green: bool,
    disable_blue: bool,
    disable_color: bool,
}

impl Default for SpotlightTypeRules {
    fn default() -> Self {
        //1003C7B1/B8,1003C871 and the packed +84 zero write1003C7BF.
        Self {
            start_height: 430,
            distance: 1024,
            attached_to: 0,
            disable_red: false,
            disable_green: false,
            disable_blue: false,
            disable_color: false,
        }
    }
}

impl SpotlightTypeRules {
    pub(crate) fn read_pass(&mut self, section: &IniSection) {
        self.start_height = section.read_ares_int("Spotlight.StartHeight", self.start_height);
        self.distance = section.read_ares_int("Spotlight.Distance", self.distance);
        //1003E260..1003E2E7: lookup-only enum; invalid/empty retains it.
        // The installed reader also accepts barrel, although the manual lists
        // body/turret only. Both non-body values select Secondary at1004EC52.
        self.attached_to = section.read_string_with(
            "Spotlight.AttachedTo",
            2048,
            self.attached_to,
            |current, value| {
                if value.eq_ignore_ascii_case("body") {
                    0
                } else if value.eq_ignore_ascii_case("turret") {
                    1
                } else if value.eq_ignore_ascii_case("barrel") {
                    2
                } else {
                    current
                }
            },
        );
        self.disable_red = section.read_ares_bool("Spotlight.DisableRed", self.disable_red);
        self.disable_green = section.read_ares_bool("Spotlight.DisableGreen", self.disable_green);
        self.disable_blue = section.read_ares_bool("Spotlight.DisableBlue", self.disable_blue);
        self.disable_color = section.read_ares_bool("Spotlight.DisableColor", self.disable_color);
    }

    pub(crate) fn start_height(self) -> i32 {
        self.start_height
    }

    pub(crate) fn distance(self) -> i32 {
        self.distance
    }

    pub(crate) fn follows_secondary(self) -> bool {
        self.attached_to != 0
    }

    /// BuildingLightDraw_Spotlight1004EAE3..1004EB1B, consumed by the
    /// existing CombatLight437410 owner rather than another blending path.
    pub(crate) fn glow_flags(self) -> u32 {
        u32::from(self.disable_color)
            | (u32::from(self.disable_red) << 1)
            | (u32::from(self.disable_green) << 2)
            | (u32::from(self.disable_blue) << 3)
    }
}
