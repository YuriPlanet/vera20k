//! The Phobos `AnimTypeExt` keys read at `Ext/AnimType/Body.cpp:93-125`:
//! `AttachedAnimPosition`, `WakeAnim`, `VisibleTo`,
//! `FireAnimDisallowedLandTypes`, `AttachFireAnimsToParent`,
//! `TheaterPalette`, `ConstrainFireAnimsToCellSpots`, `SmallFireCount`,
//! `SmallFireAnims`/`Chances`/`Distances`, `LargeFireCount` and
//! `LargeFireAnims`/`Chances`/`Distances`.
//!
//! The fire-spawn group feeds `AnimExt::SpawnFireAnims`
//! (`Ext/Anim/Body.cpp:309-405`, hook `0x425060`): its inputs live on
//! [`crate::rules::art_data::AnimTypeRuntimeConfig`] together with the stock
//! `[AudioVisual] SmallFire=`/`LargeFire=` fallbacks the Rules reader keeps.
//! The remaining keys (`AttachedAnimPosition`, `WakeAnim`, `VisibleTo`,
//! `TheaterPalette`) still have no anim-runtime consumer (the debris wake and
//! the cell-light palette are separate mechanisms); their reads land on the
//! same config so those paths need no second parse.
//!
//! ## Dependency rules
//! - Part of rules/ — no dependency on sim/, render/, ui/, etc.

use crate::rules::ini_parser::IniSection;
use crate::rules::phobos_shields::AffectedHouse;
use crate::rules::terrain_rules::LandType;

/// `Ext/AnimType/Body.h:11-16` `Valueable<AttachedAnimPosition>`; the
/// constructor value is `Default`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub enum AttachedAnimPosition {
    #[default]
    Default,
    Center,
    Ground,
}

impl AttachedAnimPosition {
    /// `EnumParser<AttachedAnimPosition>`'s name table. Anything else keeps
    /// the previous value (`Valueable::Read` returns false).
    pub fn parse(raw: &str) -> Option<Self> {
        let name = raw.trim();
        if name.eq_ignore_ascii_case("default") {
            Some(Self::Default)
        } else if name.eq_ignore_ascii_case("center") {
            Some(Self::Center)
        } else if name.eq_ignore_ascii_case("ground") {
            Some(Self::Ground)
        } else {
            None
        }
    }
}

/// Phobos `LandTypeFlags` (`Utilities/Enum.h:69-87`): one bit per `LandType`,
/// indexed by its `char` value.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub struct LandTypeFlags(u16);

impl LandTypeFlags {
    pub const NONE: Self = Self(0);
    /// `LandTypeFlags::DefaultDisallowed = Water | Rock | Ice | Beach`,
    /// the value `Ext/Anim/Body.cpp:379` falls back to for `Scorch=yes`.
    pub const DEFAULT_DISALLOWED: Self = Self::of(&[
        LandType::Water,
        LandType::Rock,
        LandType::Ice,
        LandType::Beach,
    ]);

    const fn of(types: &[LandType]) -> Self {
        let mut bits = 0u16;
        let mut index = 0;
        while index < types.len() {
            bits |= 1 << types[index].as_index();
            index += 1;
        }
        Self(bits)
    }

    /// `IsLandTypeInFlags` (`Utilities/Enum.h:91`).
    pub const fn contains(self, land: LandType) -> bool {
        self.0 & (1 << land.as_index()) != 0
    }
}

/// `Utilities/TemplateDef.h:681-704`'s `read<LandTypeFlags>`. It walks the
/// comma-separated tokens but looks each token's land type up from
/// `parser.value()` — the whole value — so only a value that is itself one
/// land-type name converts; any other non-`none` value fails the read and
/// keeps the previous nullable.
pub fn parse_land_type_flags(raw: &str) -> Option<LandTypeFlags> {
    let mut has_value_token = false;
    for token in raw.split(',') {
        let token = token.trim();
        if token.is_empty() || token.eq_ignore_ascii_case("none") {
            continue;
        }
        has_value_token = true;
    }
    if !has_value_token {
        return Some(LandTypeFlags::NONE);
    }
    let land = land_type_from_name(raw.trim())?;
    Some(LandTypeFlags(1 << land.as_index()))
}

/// `GroundType::GetLandTypeFromName` over the `Clear..=Weeds` range.
fn land_type_from_name(name: &str) -> Option<LandType> {
    LandType::ALL
        .into_iter()
        .find(|land| land.section_name().eq_ignore_ascii_case(name))
}

/// Reads the six keys from an AnimType's ARTMD section.
pub struct PhobosAnimTypeExt;

impl PhobosAnimTypeExt {
    /// `AttachedAnimPosition` (`Valueable`, default `Default`): the attach
    /// anchor for an attached anim (`Ext/Anim/Hooks.cpp:377-409`).
    pub fn attached_anim_position(section: &IniSection) -> AttachedAnimPosition {
        section.read_string_with(
            "AttachedAnimPosition",
            2048,
            AttachedAnimPosition::Default,
            |current, raw| AttachedAnimPosition::parse(raw).unwrap_or(current),
        )
    }

    /// `WakeAnim` (`ValueableVector<AnimType*>`, empty): the anims a debris
    /// impact spawns in water (`Ext/Anim/Body.cpp:278-296`).
    pub fn wake_anim(section: &IniSection) -> Vec<String> {
        crate::rules::native_processing::read_nullable_type_vector(
            section,
            "WakeAnim",
            None,
            |name| Some(name.to_owned()),
        )
        .unwrap_or_default()
    }

    /// `VisibleTo` (`Valueable<AffectedHouse>`, constructed `All` at
    /// `Ext/AnimType/Body.h:64,116`): who may see the anim. `None` is the
    /// unset default (All); a present value assigns even `none`, and rejected
    /// text keeps the previous value.
    pub fn visible_to(section: &IniSection) -> Option<AffectedHouse> {
        section.read_string_with("VisibleTo", 2048, None, |current, raw| {
            AffectedHouse::parse(raw).or(current)
        })
    }

    /// `FireAnimDisallowedLandTypes` (`Nullable<LandTypeFlags>`), read with
    /// `<false, true>` so `none` clears rather than fails.
    pub fn fire_anim_disallowed_land_types(section: &IniSection) -> Option<LandTypeFlags> {
        section.read_phobos_nullable_with(
            "FireAnimDisallowedLandTypes",
            None,
            parse_land_type_flags,
        )
    }

    /// `AttachFireAnimsToParent` (`Nullable<bool>`): attach fire anims to the
    /// burning object instead of the cell (`Ext/Anim/Body.cpp:388`).
    pub fn attach_fire_anims_to_parent(section: &IniSection) -> Option<bool> {
        section.read_phobos_nullable_bool("AttachFireAnimsToParent", None)
    }

    /// `RestrictVisibilityIfCloaked` (`Valueable<bool>`, constructor `false`,
    /// `Ext/AnimType/Body.h:66`/`:118`): the anim attached to a cloaked or
    /// cloaking techno is hidden from a viewer that neither allies its owner
    /// nor senses its cell (`Ext/Anim/Hooks.cpp:524-570`, `0x423061`).
    pub fn restrict_visibility_if_cloaked(section: &IniSection) -> bool {
        section.read_bool("RestrictVisibilityIfCloaked", false)
    }

    /// `DetachOnCloak` (`Valueable<bool>`, constructor `true`,
    /// `Ext/AnimType/Body.h:67`/`:119`): a cloaking owner keeps this attached
    /// anim instead of letting `Detach_All(false)` remove it
    /// (`Ext/Anim/Hooks.cpp:663-684`, hook `0x425174`).
    pub fn detach_on_cloak(section: &IniSection) -> bool {
        section.read_bool("DetachOnCloak", true)
    }

    /// `TheaterPalette` (`Nullable<bool>`): use the theater/cell light
    /// convert for this anim (`Ext/Anim/Hooks.cpp:252,746`).
    pub fn theater_palette(section: &IniSection) -> Option<bool> {
        section.read_phobos_nullable_bool("TheaterPalette", None)
    }

    /// `ConstrainFireAnimsToCellSpots` (`Valueable<bool>`, constructor
    /// `true`, `Ext/AnimType/Body.h:122`): snap every spawned fire anim to the
    /// cell's infantry sublocation (`Ext/Anim/Body.cpp:328`).
    pub fn constrain_fire_anims_to_cell_spots(section: &IniSection) -> bool {
        section.read_bool("ConstrainFireAnimsToCellSpots", true)
    }

    /// `SmallFireCount` (`Nullable<int>`): overrides the
    /// `1 + Flamer` default of the small-fire loop (`Ext/Anim/Body.cpp:389`).
    pub fn small_fire_count(section: &IniSection) -> Option<i32> {
        section.read_phobos_nullable_int("SmallFireCount", None)
    }

    /// `SmallFireAnims` (`ValueableVector<AnimTypeClass*>`, empty): the small
    /// fire anims; an empty vector falls back to `[AudioVisual] SmallFire=`
    /// (`Ext/Anim/Body.cpp:384`).
    pub fn small_fire_anims(section: &IniSection) -> Vec<String> {
        crate::rules::native_processing::read_nullable_type_vector(
            section,
            "SmallFireAnims",
            None,
            |name| Some(name.to_owned()),
        )
        .unwrap_or_default()
    }

    /// `SmallFireChances` (`ValueableVector<double>`, empty).
    pub fn small_fire_chances(section: &IniSection) -> Vec<f64> {
        section
            .read_phobos_double_list("SmallFireChances")
            .unwrap_or_default()
    }

    /// `SmallFireDistances` (`ValueableVector<double>`, empty): cell radii,
    /// each converted with `LeptonsPerCell` at spawn (`Ext/Anim/Body.cpp:369`).
    pub fn small_fire_distances(section: &IniSection) -> Vec<f64> {
        section
            .read_phobos_double_list("SmallFireDistances")
            .unwrap_or_default()
    }

    /// `LargeFireCount` (`Valueable<int>`, constructor `1`,
    /// `Ext/AnimType/Body.h:135`).
    pub fn large_fire_count(section: &IniSection) -> i32 {
        section.read_string_with("LargeFireCount", 2048, 1, |current, raw| {
            crate::rules::ini_value::parse_ares_int(raw).unwrap_or(current)
        })
    }

    /// `LargeFireAnims` (`ValueableVector<AnimTypeClass*>`, empty): the
    /// `Flamer=` type's second loop (`Ext/Anim/Body.cpp:395`).
    pub fn large_fire_anims(section: &IniSection) -> Vec<String> {
        crate::rules::native_processing::read_nullable_type_vector(
            section,
            "LargeFireAnims",
            None,
            |name| Some(name.to_owned()),
        )
        .unwrap_or_default()
    }

    /// `LargeFireChances` (`ValueableVector<double>`, empty).
    pub fn large_fire_chances(section: &IniSection) -> Vec<f64> {
        section
            .read_phobos_double_list("LargeFireChances")
            .unwrap_or_default()
    }

    /// `LargeFireDistances` (`ValueableVector<double>`, empty).
    pub fn large_fire_distances(section: &IniSection) -> Vec<f64> {
        section
            .read_phobos_double_list("LargeFireDistances")
            .unwrap_or_default()
    }
}

#[cfg(all(test, tests_rules_o_z))]
mod tests {
    use super::*;
    use crate::rules::ini_parser::IniFile;

    /// The six keys keep their `Body.h` constructor values when unset.
    #[test]
    fn the_anim_type_ext_keys_default_to_the_constructor_values() {
        let ini = IniFile::from_str("[ANIM]\nStart=0\n");
        let empty = ini.section("ANIM").unwrap();
        assert_eq!(
            PhobosAnimTypeExt::attached_anim_position(empty),
            AttachedAnimPosition::Default
        );
        assert!(PhobosAnimTypeExt::wake_anim(empty).is_empty());
        assert_eq!(PhobosAnimTypeExt::visible_to(empty), None);
        assert_eq!(
            PhobosAnimTypeExt::fire_anim_disallowed_land_types(empty),
            None
        );
        assert_eq!(PhobosAnimTypeExt::attach_fire_anims_to_parent(empty), None);
        assert_eq!(PhobosAnimTypeExt::theater_palette(empty), None);
        assert!(PhobosAnimTypeExt::detach_on_cloak(empty));
        assert!(!PhobosAnimTypeExt::restrict_visibility_if_cloaked(empty));
        assert!(PhobosAnimTypeExt::constrain_fire_anims_to_cell_spots(empty));
        assert_eq!(PhobosAnimTypeExt::small_fire_count(empty), None);
        assert!(PhobosAnimTypeExt::small_fire_anims(empty).is_empty());
        assert!(PhobosAnimTypeExt::small_fire_chances(empty).is_empty());
        assert!(PhobosAnimTypeExt::small_fire_distances(empty).is_empty());
        assert_eq!(PhobosAnimTypeExt::large_fire_count(empty), 1);
        assert!(PhobosAnimTypeExt::large_fire_anims(empty).is_empty());
        assert!(PhobosAnimTypeExt::large_fire_chances(empty).is_empty());
        assert!(PhobosAnimTypeExt::large_fire_distances(empty).is_empty());
    }

    #[test]
    fn the_anim_type_ext_keys_read_their_authored_values() {
        let ini = IniFile::from_str(
            "[ANIM]\nAttachedAnimPosition=Ground\nWakeAnim=WAKE1,WAKE2\nVisibleTo=owner,allies\n\
             FireAnimDisallowedLandTypes=Rock\nAttachFireAnimsToParent=yes\n\
             TheaterPalette=no\nDetachOnCloak=no\nRestrictVisibilityIfCloaked=yes\n",
        );
        let set = ini.section("ANIM").unwrap();
        assert_eq!(
            PhobosAnimTypeExt::attached_anim_position(set),
            AttachedAnimPosition::Ground
        );
        assert_eq!(PhobosAnimTypeExt::wake_anim(set), ["WAKE1", "WAKE2"]);
        assert_eq!(
            PhobosAnimTypeExt::visible_to(set),
            Some(AffectedHouse::TEAM)
        );
        let none = IniFile::from_str(
            "[ANIM]
VisibleTo=none
",
        );
        assert_eq!(
            PhobosAnimTypeExt::visible_to(none.section("ANIM").unwrap()),
            Some(AffectedHouse::NONE),
            "none is a valid AffectedHouse value, not a clearing token"
        );
        let rejected = IniFile::from_str(
            "[ANIM]
VisibleTo=bogus
",
        );
        assert_eq!(
            PhobosAnimTypeExt::visible_to(rejected.section("ANIM").unwrap()),
            None,
            "rejected text keeps the unset default"
        );
        assert_eq!(
            PhobosAnimTypeExt::fire_anim_disallowed_land_types(set),
            Some(LandTypeFlags(1 << LandType::Rock.as_index()))
        );
        assert_eq!(
            PhobosAnimTypeExt::attach_fire_anims_to_parent(set),
            Some(true)
        );
        assert_eq!(PhobosAnimTypeExt::theater_palette(set), Some(false));
        assert!(!PhobosAnimTypeExt::detach_on_cloak(set));
        assert!(PhobosAnimTypeExt::restrict_visibility_if_cloaked(set));
    }

    /// The fire-spawn keys keep their `Ext/AnimType/Body.h` constructor values
    /// (`ConstrainFireAnimsToCellSpots` true, `SmallFireCount` unset,
    /// `LargeFireCount` 1) and read the authored vectors verbatim.
    #[test]
    fn the_fire_spawn_keys_read_their_vectors_and_counts() {
        let ini = IniFile::from_str(
            "[FIRE]\nConstrainFireAnimsToCellSpots=no\nSmallFireCount=3\n\
             SmallFireAnims=SMALL1,SMALL2\nSmallFireChances=0.5,0.25\n\
             SmallFireDistances=0.25,1\nLargeFireCount=4\nLargeFireAnims=LARGE\n\
             LargeFireChances=0.75\nLargeFireDistances=2\n",
        );
        let set = ini.section("FIRE").unwrap();
        assert!(!PhobosAnimTypeExt::constrain_fire_anims_to_cell_spots(set));
        assert_eq!(PhobosAnimTypeExt::small_fire_count(set), Some(3));
        assert_eq!(
            PhobosAnimTypeExt::small_fire_anims(set),
            ["SMALL1", "SMALL2"]
        );
        assert_eq!(PhobosAnimTypeExt::small_fire_chances(set), [0.5, 0.25]);
        assert_eq!(PhobosAnimTypeExt::small_fire_distances(set), [0.25, 1.0]);
        assert_eq!(PhobosAnimTypeExt::large_fire_count(set), 4);
        assert_eq!(PhobosAnimTypeExt::large_fire_anims(set), ["LARGE"]);
        assert_eq!(PhobosAnimTypeExt::large_fire_chances(set), [0.75]);
        assert_eq!(PhobosAnimTypeExt::large_fire_distances(set), [2.0]);
        // `<default>` clears the nullable count like every Phobos nullable.
        let cleared = IniFile::from_str("[FIRE]\nSmallFireCount=<default>\n");
        assert_eq!(
            PhobosAnimTypeExt::small_fire_count(cleared.section("FIRE").unwrap()),
            None
        );
    }

    /// `read<LandTypeFlags>` looks the land type up from the whole value, so a
    /// multi-token list fails instead of ORing the tokens. `none` is the
    /// empty mask and succeeds.
    #[test]
    fn the_land_type_flags_reader_matches_the_whole_value_lookup() {
        assert_eq!(
            parse_land_type_flags("rock"),
            Some(LandTypeFlags(1 << LandType::Rock.as_index()))
        );
        assert_eq!(parse_land_type_flags("none"), Some(LandTypeFlags::NONE));
        assert_eq!(parse_land_type_flags("Rock,Water"), None);
        assert_eq!(parse_land_type_flags("Mud"), None);
        assert!(
            LandTypeFlags::DEFAULT_DISALLOWED.contains(LandType::Water)
                && LandTypeFlags::DEFAULT_DISALLOWED.contains(LandType::Rock)
                && LandTypeFlags::DEFAULT_DISALLOWED.contains(LandType::Ice)
                && LandTypeFlags::DEFAULT_DISALLOWED.contains(LandType::Beach)
        );
        assert!(!LandTypeFlags::DEFAULT_DISALLOWED.contains(LandType::Clear));
    }

    /// An unknown position name keeps the current value.
    #[test]
    fn the_attached_anim_position_keeps_the_current_value_on_a_bad_name() {
        assert_eq!(
            AttachedAnimPosition::parse("center"),
            Some(AttachedAnimPosition::Center)
        );
        assert_eq!(AttachedAnimPosition::parse("sideways"), None);
    }
}
