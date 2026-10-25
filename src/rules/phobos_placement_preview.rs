//! Phobos building placement preview settings.
//!
//! Native identity: Phobos `RulesExt` placement fields and `BuildingTypeExt`
//! (`src/Ext/Rules/Body.{h,cpp}`, `src/Ext/BuildingType/Body.{h,cpp}`), consumed
//! by `TacticalClass_DrawPlacement_PlacementPreview @ 0x6D528A` and
//! `CellClass_Draw_It_SetPlacementGridTranslucency @ 0x47EFAE`.
//!
//! This module owns the shared translucency value and the `[AudioVisual]`
//! globals. Per-BuildingType fields live on `ObjectType`; the client
//! `[Phobos] ShowPlacementPreview` switch lives in
//! `app::persistence::phobos_settings`.

use crate::rules::art_data::{
    ANIM_DRAW_BITS_OPAQUE, ANIM_DRAW_BITS_TRANSLUCENT_25, ANIM_DRAW_BITS_TRANSLUCENT_50,
    ANIM_DRAW_BITS_TRANSLUCENT_75, anim_translucency_source_alpha,
};
use crate::rules::ini_parser::IniFile;

/// Phobos `TranslucencyLevel` (`Utilities/Constructs.h:459..510`), held as the
/// `CC_Draw_Shape` translucency bits it selects: only the exact 0/25/50/75
/// assignments select a blitter; every other integer maps to the opaque
/// default.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, Default, serde::Serialize, serde::Deserialize,
)]
pub struct Translucency(u32);

impl Translucency {
    pub const OPAQUE: Self = Self(ANIM_DRAW_BITS_OPAQUE);
    pub const T25: Self = Self(ANIM_DRAW_BITS_TRANSLUCENT_25);
    pub const T50: Self = Self(ANIM_DRAW_BITS_TRANSLUCENT_50);
    pub const T75: Self = Self(ANIM_DRAW_BITS_TRANSLUCENT_75);

    /// `TranslucencyLevel& operator=(int)`: an exact match, else opaque.
    pub fn from_int(value: i32) -> Self {
        match value {
            25 => Self::T25,
            50 => Self::T50,
            75 => Self::T75,
            _ => Self::OPAQUE,
        }
    }

    /// The blitter's source weight, from the shared anim translucency table.
    pub fn opacity(self) -> f32 {
        anim_translucency_source_alpha(self.0)
    }
}

/// `RulesExt`'s placement preview globals.
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct PlacementPreviewGlobals {
    /// `[AudioVisual] PlacementPreview=`, constructor default `false`.
    pub enabled: bool,
    /// `[AudioVisual] PlacementPreview.Translucency=`, default 75.
    pub translucency: Translucency,
    /// `[AudioVisual] PlacementGrid.Translucency=`, default 0.
    pub grid_translucency: Translucency,
    /// `PlacementGrid.TranslucencyWithPreview`: unset inherits
    /// `grid_translucency` (`Nullable<TranslucencyLevel>`).
    pub grid_translucency_with_preview: Option<Translucency>,
}

impl Default for PlacementPreviewGlobals {
    fn default() -> Self {
        // RulesExt constructor: PlacementPreview false, its translucency 75,
        // the grid 0, and the with-preview override unset.
        Self {
            enabled: false,
            translucency: Translucency::T75,
            grid_translucency: Translucency::OPAQUE,
            grid_translucency_with_preview: None,
        }
    }
}

impl PlacementPreviewGlobals {
    /// The active grid translucency: the with-preview override applies only
    /// while the rules feature and the client switch are both on.
    pub fn effective_grid_translucency(&self, client_switch: bool) -> Translucency {
        if self.enabled && client_switch {
            self.grid_translucency_with_preview
                .unwrap_or(self.grid_translucency)
        } else {
            self.grid_translucency
        }
    }

    pub(crate) fn from_ini(ini: &IniFile) -> Self {
        let section = ini.section_or_empty("AudioVisual");
        Self {
            enabled: section.read_bool("PlacementPreview", false),
            translucency: Translucency::from_int(
                section.read_int("PlacementPreview.Translucency", 75),
            ),
            grid_translucency: Translucency::from_int(
                section.read_int("PlacementGrid.Translucency", 0),
            ),
            grid_translucency_with_preview: section
                .read_phobos_nullable_int("PlacementGrid.TranslucencyWithPreview", None)
                .map(Translucency::from_int),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn translucency_accepts_only_the_native_levels() {
        assert_eq!(Translucency::from_int(0), Translucency::OPAQUE);
        assert_eq!(Translucency::from_int(25), Translucency::T25);
        assert_eq!(Translucency::from_int(50), Translucency::T50);
        assert_eq!(Translucency::from_int(75), Translucency::T75);
        assert_eq!(Translucency::from_int(30), Translucency::OPAQUE);
        assert_eq!(Translucency::from_int(-1), Translucency::OPAQUE);
    }

    #[test]
    fn building_type_keys_use_the_source_defaults_and_overrides() {
        use crate::rules::ruleset::RuleSet;

        let rules = RuleSet::from_ini(&IniFile::from_str(
            "[BuildingTypes]\n0=DEFAULT\n1=CUSTOM\n\
             [DEFAULT]\nStrength=100\n\
             [CUSTOM]\nStrength=100\nPlacementPreview=no\n\
             PlacementPreview.Shape=~~~/PREV.SHP\nPlacementPreview.ShapeFrame=3\n\
             PlacementPreview.Offset=1,-2,3\nPlacementPreview.Remap=no\n\
             PlacementPreview.Palette=custom.pal\nPlacementPreview.Translucency=50\n",
        ))
        .unwrap();
        let default = rules.object("DEFAULT").unwrap();
        assert!(default.placement_preview());
        assert_eq!(default.placement_preview_shape(), None);
        assert_eq!(default.placement_preview_shape_frame(), None);
        assert_eq!(default.placement_preview_offset(), [0, -15, 1]);
        assert!(default.placement_preview_remap());
        assert_eq!(default.placement_preview_palette(), None);
        assert_eq!(default.placement_preview_translucency(), None);

        let custom = rules.object("CUSTOM").unwrap();
        assert!(!custom.placement_preview());
        assert_eq!(custom.placement_preview_shape(), Some("~~~/PREV.SHP"));
        assert_eq!(custom.placement_preview_shape_frame(), Some(3));
        assert_eq!(custom.placement_preview_offset(), [1, -2, 3]);
        assert!(!custom.placement_preview_remap());
        assert_eq!(custom.placement_preview_palette(), Some("custom.pal"));
        assert_eq!(
            custom.placement_preview_translucency(),
            Some(Translucency::T50)
        );
    }

    #[test]
    fn grid_override_needs_both_switches_and_falls_back_to_the_base() {
        let ini = IniFile::from_str(
            "[AudioVisual]\nPlacementPreview=yes\nPlacementGrid.Translucency=25\n\
             PlacementGrid.TranslucencyWithPreview=75\n\
             PlacementPreview.Translucency=50\n",
        );
        let globals = PlacementPreviewGlobals::from_ini(&ini);
        assert_eq!(globals.translucency, Translucency::T50);
        assert_eq!(globals.effective_grid_translucency(true), Translucency::T75);
        assert_eq!(
            globals.effective_grid_translucency(false),
            Translucency::T25
        );

        let inherited = PlacementPreviewGlobals::from_ini(&IniFile::from_str(
            "[AudioVisual]\nPlacementPreview=yes\nPlacementGrid.Translucency=25\n",
        ));
        assert_eq!(
            inherited.effective_grid_translucency(true),
            Translucency::T25
        );
    }
}
