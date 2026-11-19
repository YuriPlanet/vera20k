//! Phobos `XDrawOffset.*` / `YDrawOffset.*` (`Ext/AnimType/Body.cpp:82-90`).
//!
//! The eight selection-bracket keys extend `AnimTypeExt`
//! (`Ext/AnimType/Body.h:37-45`, constructor defaults `:89-97`). Consumer:
//! `AnimClass_DrawIt_DrawOffset` (`Ext/Anim/Hooks.cpp:278-345`, hook
//! `0x423122`, alternate `0x422CD8`). The plain `XDrawOffset` shifts DrawIt's
//! location; when the animation's `OwnerObject` is a Techno, the bracket keys
//! overwrite X and/or Y from that Techno's selection bracket.
//!
//! ## Dependency rules
//! - Part of rules/ — no dependency on sim/, render/, ui/, etc.

use crate::rules::ini_parser::IniSection;

/// `AnimTypeExt` fields `XDrawOffset` .. `YDrawOffset_BracketAdjust_Buildings`
/// with the constructor defaults (`Ext/AnimType/Body.h:89-97`): every gate
/// false, every plain adjust 0 and both building overrides unset.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct PhobosAnimDrawOffset {
    /// `XDrawOffset=`: pixels added to DrawIt's X before the bracket arms.
    /// The sprite atlas already bakes it into the animation's frame offset,
    /// so the bracket X assignment subtracts it to reproduce the native
    /// overwrite.
    pub x_draw_offset: i32,
    /// `XDrawOffset.ApplyBracketWidth=`.
    pub x_apply_bracket_width: bool,
    /// `XDrawOffset.InvertBracketShift=`.
    pub x_invert_bracket_shift: bool,
    /// `XDrawOffset.BracketAdjust=`.
    pub x_bracket_adjust: i32,
    /// `XDrawOffset.BracketAdjust.Buildings=`: `Nullable<int>`, unset falls
    /// back to [`Self::x_bracket_adjust`].
    pub x_bracket_adjust_buildings: Option<i32>,
    /// `YDrawOffset.ApplyBracketHeight=`.
    pub y_apply_bracket_height: bool,
    /// `YDrawOffset.InvertBracketShift=`.
    pub y_invert_bracket_shift: bool,
    /// `YDrawOffset.BracketAdjust=`.
    pub y_bracket_adjust: i32,
    /// `YDrawOffset.BracketAdjust.Buildings=`: `Nullable<int>`, unset falls
    /// back to [`Self::y_bracket_adjust`].
    pub y_bracket_adjust_buildings: Option<i32>,
}

impl PhobosAnimDrawOffset {
    /// Read the keys in `AnimTypeExt::LoadFromINIFile` order. Every value is a
    /// Phobos `Valueable<T>` (Ares `Parser<T>`), except the two `Nullable<int>`
    /// building overrides, whose `Get(default)` keeps them unset on absence.
    pub fn from_section(section: &IniSection) -> Self {
        Self {
            x_draw_offset: section.read_ares_int("XDrawOffset", 0),
            x_apply_bracket_width: section.read_ares_bool("XDrawOffset.ApplyBracketWidth", false),
            x_invert_bracket_shift: section.read_ares_bool("XDrawOffset.InvertBracketShift", false),
            x_bracket_adjust: section.read_ares_int("XDrawOffset.BracketAdjust", 0),
            x_bracket_adjust_buildings: section
                .read_phobos_nullable_int("XDrawOffset.BracketAdjust.Buildings", None),
            y_apply_bracket_height: section.read_ares_bool("YDrawOffset.ApplyBracketHeight", false),
            y_invert_bracket_shift: section.read_ares_bool("YDrawOffset.InvertBracketShift", false),
            y_bracket_adjust: section.read_ares_int("YDrawOffset.BracketAdjust", 0),
            y_bracket_adjust_buildings: section
                .read_phobos_nullable_int("YDrawOffset.BracketAdjust.Buildings", None),
        }
    }

    /// Whether either bracket arm can replace the DrawIt location at all.
    pub fn applies_bracket(&self) -> bool {
        self.x_apply_bracket_width || self.y_apply_bracket_height
    }

    /// `XDrawOffset_BracketAdjust_Buildings.Get(XDrawOffset_BracketAdjust)`.
    pub fn x_adjust(&self, building: bool) -> i32 {
        match (building, self.x_bracket_adjust_buildings) {
            (true, Some(value)) => value,
            _ => self.x_bracket_adjust,
        }
    }

    /// `YDrawOffset_BracketAdjust_Buildings.Get(YDrawOffset_BracketAdjust)`.
    pub fn y_adjust(&self, building: bool) -> i32 {
        match (building, self.y_bracket_adjust_buildings) {
            (true, Some(value)) => value,
            _ => self.y_bracket_adjust,
        }
    }
}

#[cfg(all(test, tests_rules_o_z))]
#[path = "phobos_anim_draw_offset_tests.rs"]
mod tests;
