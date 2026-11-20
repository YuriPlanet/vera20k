//! `CreateUnit.*` reads (`Ext/AnimType/Body.cpp:130-149`,
//! `New/Type/Affiliated/CreateUnitTypeClass.cpp:3-22`).
use super::CreateUnitSettings;
use crate::rules::ares_warhead::HouseSelector;
use crate::rules::ini_parser::IniFile;
use crate::rules::mission_data::MissionType;

fn read(text: &str) -> CreateUnitSettings {
    CreateUnitSettings::from_section(IniFile::from_str(text).section_or_empty("Q"), "MTNK".into())
}

#[test]
fn create_unit_defaults_and_keys() {
    // `CreateUnitTypeClass.h:5-30`: `RandomFacing` constructs **yes**.
    let defaults = read("[Q]\n");
    assert_eq!(defaults.unit_type, "MTNK");
    assert_eq!(defaults.facing, 0);
    assert!(!defaults.inherit_death_facings);
    assert!(!defaults.inherit_turret_facings);
    assert!(defaults.random_facing);
    assert!(!defaults.remap_anim);
    assert_eq!(defaults.unit_mission, MissionType::Guard);
    assert_eq!(defaults.ai_unit_mission, None);
    assert_eq!(defaults.owner, HouseSelector::Victim);
    assert!(!defaults.require_owner);
    assert!(!defaults.always_spawn_on_ground);
    assert!(!defaults.spawn_parachuted_in_air);
    assert!(!defaults.consider_pathfinding);
    assert!(defaults.spawn_anim.is_empty());
    assert_eq!(defaults.spawn_height, -1);

    let set = read(
        "[Q]\n\
         CreateUnit.Facing=192\n\
         CreateUnit.InheritFacings=yes\n\
         CreateUnit.InheritTurretFacings=yes\n\
         CreateUnit.RandomFacing=no\n\
         CreateUnit.RemapAnim=yes\n\
         CreateUnit.Mission=Hunt\n\
         CreateUnit.AIMission=Sleep\n\
         CreateUnit.Owner=Killer\n\
         CreateUnit.RequireOwner=yes\n\
         CreateUnit.AlwaysSpawnOnGround=yes\n\
         CreateUnit.SpawnParachutedInAir=yes\n\
         CreateUnit.ConsiderPathfinding=yes\n\
         CreateUnit.SpawnAnim=GAANIM,GBANIM\n\
         CreateUnit.SpawnHeight=96\n",
    );
    assert_eq!(set.facing, 192);
    assert!(set.inherit_death_facings);
    assert!(set.inherit_turret_facings);
    assert!(!set.random_facing);
    assert!(set.remap_anim);
    assert_eq!(set.unit_mission, MissionType::Hunt);
    assert_eq!(set.ai_unit_mission, Some(MissionType::Sleep));
    assert_eq!(set.owner, HouseSelector::Killer);
    assert!(set.require_owner);
    assert!(set.always_spawn_on_ground);
    assert!(set.spawn_parachuted_in_air);
    assert!(set.consider_pathfinding);
    assert_eq!(set.spawn_anim, ["GAANIM", "GBANIM"]);
    assert_eq!(set.spawn_height, 96);

    // `Nullable<Mission>`: `none` clears; an unknown name keeps the default.
    let cleared = read("[Q]\nCreateUnit.AIMission=<none>\n");
    assert_eq!(cleared.ai_unit_mission, None);
    let unknown = read("[Q]\nCreateUnit.Mission=Nonsense\nCreateUnit.AIMission=Nonsense\n");
    assert_eq!(unknown.unit_mission, MissionType::Guard);
    assert_eq!(unknown.ai_unit_mission, None);
}
