//! Phobos `CreateUnit.*` animation-type keys (`Ext/AnimType/Body.cpp:130-149`
//! and `New/Type/Affiliated/CreateUnitTypeClass.cpp:3-22`; declarations and
//! constructor defaults `CreateUnitTypeClass.h:5-30`).
//!
//! `CreateUnit` itself is a `Nullable<TechnoTypeClass*>`: absent keeps the
//! previous state and `none`/`<none>` clears it. When it is set the same ART
//! section is read through `CreateUnitTypeClass::LoadFromINI`. The defaults
//! are the constructor's; note `RandomFacing` constructs **yes**
//! (`CreateUnitTypeClass.h:15`), unlike its first documentation.
//!
//! ## Dependency rules
//! - Part of rules/ — no dependency on sim/, render/, ui/, etc.

use crate::rules::ares_warhead::HouseSelector;
use crate::rules::ini_parser::IniSection;
use crate::rules::mission_data::MissionType;
use crate::rules::native_processing::read_nullable_type_vector;

/// One animation type's `CreateUnit` settings.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct CreateUnitSettings {
    /// The `CreateUnit=` TechnoType name (the `Nullable` was set).
    pub unit_type: String,
    /// `CreateUnit.Facing=`, `Valueable<DirType>`, default 0.
    pub facing: u8,
    /// `CreateUnit.InheritFacings=`, `Valueable<bool>`, default `no`.
    pub inherit_death_facings: bool,
    /// `CreateUnit.InheritTurretFacings=`, default `no`.
    pub inherit_turret_facings: bool,
    /// `CreateUnit.RandomFacing=`, `Valueable<bool>`, default **`yes`**.
    pub random_facing: bool,
    /// `CreateUnit.RemapAnim=`, default `no`.
    pub remap_anim: bool,
    /// `CreateUnit.Mission=`, `Valueable<Mission>`, default `Guard`.
    pub unit_mission: MissionType,
    /// `CreateUnit.AIMission=`, `Nullable<Mission>`.
    pub ai_unit_mission: Option<MissionType>,
    /// `CreateUnit.Owner=`, `Valueable<OwnerHouseKind>`, default `Victim`.
    pub owner: HouseSelector,
    /// `CreateUnit.RequireOwner=`, default `no`.
    pub require_owner: bool,
    /// `CreateUnit.AlwaysSpawnOnGround=`, default `no`.
    pub always_spawn_on_ground: bool,
    /// `CreateUnit.SpawnParachutedInAir=`, default `no`.
    pub spawn_parachuted_in_air: bool,
    /// `CreateUnit.ConsiderPathfinding=`, default `no`.
    pub consider_pathfinding: bool,
    /// `CreateUnit.SpawnAnim=`, `ValueableVector<AnimTypeClass*>`.
    pub spawn_anim: Vec<String>,
    /// `CreateUnit.SpawnHeight=`, `Valueable<int>`, default -1.
    pub spawn_height: i32,
}

impl CreateUnitSettings {
    pub fn from_section(section: &IniSection, unit_type: String) -> Self {
        Self {
            unit_type,
            facing: section.read_int("CreateUnit.Facing", 0) as u8,
            inherit_death_facings: section.read_bool("CreateUnit.InheritFacings", false),
            inherit_turret_facings: section.read_bool("CreateUnit.InheritTurretFacings", false),
            random_facing: section.read_bool("CreateUnit.RandomFacing", true),
            remap_anim: section.read_bool("CreateUnit.RemapAnim", false),
            unit_mission: section
                .read_name("CreateUnit.Mission", 0x80)
                .and_then(MissionType::from_map_name)
                .unwrap_or(MissionType::Guard),
            ai_unit_mission: read_nullable_mission(section, "CreateUnit.AIMission", None),
            owner: HouseSelector::Victim.read(section, "CreateUnit.Owner"),
            require_owner: section.read_bool("CreateUnit.RequireOwner", false),
            always_spawn_on_ground: section.read_bool("CreateUnit.AlwaysSpawnOnGround", false),
            spawn_parachuted_in_air: section.read_bool("CreateUnit.SpawnParachutedInAir", false),
            consider_pathfinding: section.read_bool("CreateUnit.ConsiderPathfinding", false),
            spawn_anim: read_nullable_type_vector(section, "CreateUnit.SpawnAnim", None, |name| {
                Some(name.to_owned())
            })
            .unwrap_or_default(),
            spawn_height: section.read_int("CreateUnit.SpawnHeight", -1),
        }
    }
}

/// `Nullable<Mission>` (`Utilities/Template.h:223`, `Utilities/TemplateDef.h:1833-1850`
/// with `allowNone=false`): `<default>`, `none` and `<none>` clear the field;
/// absent, empty and a rejected name keep `current`; a named `[Basic]`-style
/// mission replaces it (`Utilities/TemplateDef.h:510-528`).
pub(crate) fn read_nullable_mission(
    section: &IniSection,
    key: &str,
    current: Option<MissionType>,
) -> Option<MissionType> {
    section.read_phobos_nullable_with(key, current, MissionType::from_map_name)
}

#[cfg(all(test, tests_rules_o_z))]
#[path = "phobos_create_unit_tests.rs"]
mod tests;
