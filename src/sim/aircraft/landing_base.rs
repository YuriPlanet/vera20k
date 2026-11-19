//! IFlyControl's landing queries: the live landing base in leptons
//! (`0x0041B6A0`) and the landing direction (`0x0041B760`).

use crate::map::entities::EntityCategory;
use crate::rules::ruleset::RuleSet;
use crate::sim::entity_store::EntityStore;
use crate::sim::game_entity::GameEntity;
use crate::sim::intern::StringInterner;
use crate::sim::mission::{MissionId, MissionType};

/// Fly callers obtain this through Aircraft's auxiliary interface. Other Foot
/// categories fail that query and use zero, even with a Carryall rules field.
/// Cargo, sparse contacts and effective Mission remain their existing owners;
/// this value is recomputed, never stored as an independent altitude target.
/// Native comparisons: tools/spatial_oracle/fly_landing_base.{py,json}.
pub(crate) fn landing_base(
    entity: &GameEntity,
    entities: &EntityStore,
    rules_context: Option<(&RuleSet, &StringInterner)>,
) -> i32 {
    if entity.category() != EntityCategory::Aircraft {
        return 0;
    }
    let Some((rules, interner)) = rules_context else {
        return 0;
    };
    if !interner
        .object_type_in_category(entity.type_ref(), entity.category().into(), rules)
        .is_some_and(|object| object.carryall)
    {
        return 0;
    }
    let loaded = entity
        .passenger_role()
        .cargo()
        .is_some_and(|cargo| cargo.count() != 0);
    if !loaded && entity.mission.effective() == MissionId::from_known(MissionType::Enter) {
        //65AD40 reads raw slot0, not the first live contact. A hole must not
        // promote another dock to this special ground-level landing case.
        let docking_at_ground = entity
            .radio_contacts
            .slot(0)
            .and_then(|id| entities.get(id))
            .filter(|contact| contact.category() == EntityCategory::Structure)
            .and_then(|contact| {
                interner.object_type_in_category(
                    contact.type_ref(),
                    contact.category().into(),
                    rules,
                )
            })
            .is_some_and(|object| object.unit_repair || object.helipad);
        if docking_at_ground {
            return 0;
        }
    }
    //65AE30 scans every slot. It does not require the contact to be a building.
    if loaded || !entity.radio_contacts.is_empty() {
        100
    } else {
        0
    }
}

/// `AircraftClass::Landing_Direction @ 0x0041B760` (IFlyControl `+0x10`):
/// the direction of eight an aircraft lands facing. In radio contact
/// (`0x0065AE30`) it is the slot-0 contact's (`0x0065AD40`) PrimaryFacing
/// (`+0x388`) current value; with passengers (`+0x118`) the aircraft's own
/// Secondary (`+0x3A0`); otherwise Rules PoseDir (`+0x44`), returned raw.
/// A facing rounds as `((raw >> 12) + 1) >> 1`, low three bits.
///
/// A contact in a later slot with slot 0 empty would make native read a
/// facing at address `0x388`; aircraft radios hold one slot, so VERA reads
/// that case as no contact.
pub(crate) fn landing_direction(
    entity: &GameEntity,
    entities: &EntityStore,
    frame: u32,
    pose_dir: i32,
) -> i32 {
    let dir8 = |facing: u16| i32::from(crate::util::direction_tables::dir_from_facing16(facing));
    if !entity.radio_contacts.is_empty()
        && let Some(contact) = entity
            .radio_contacts
            .slot(0)
            .and_then(|id| entities.get(id))
    {
        return dir8(contact.body_facing.current(frame));
    }
    if entity
        .passenger_role()
        .cargo()
        .is_some_and(|cargo| cargo.count() != 0)
    {
        return entity
            .barrel_facing
            .as_ref()
            .map_or(0, |facing| dir8(facing.current(frame)));
    }
    pose_dir
}

/// Phobos `AircraftExt::GetLandingDir` (`Ext/Aircraft/Body.cpp:128-200`) for
/// the hooks that replace a stock `Landing_Direction` use with
/// `DirStruct(GetLandingDir(...)).Raw`: the landing face in 16-bit raw units.
///
/// Every VERA caller passes a null dock and `isProduction=false`; a linked
/// building comes from the aircraft's slot-0 radio contact. Order: the
/// spawner's facing; a linked building's per-dock `AircraftDockingDir`
/// (`BuildingTypeExt`, slot = `FindLinkIndex`, an out-of-range index falling
/// back to slot 0's entry); the building's facing unless
/// `AircraftDockingDir.DefaultToPoseDir`; a non-AirportBound non-building
/// link's facing; the type's `LandingDir` (AirportBound masks `& 0xFF`,
/// otherwise a negative value keeps its own facing); the pose dir when the
/// aircraft is linked; otherwise `PoseDir.Field`'s low byte (`PoseDir * 32`
/// when unset). A `DirType` result scales as `<< 8`; the stock fallback (no
/// rules context) keeps the stock direction of eight, scaled `<< 13`.
///
/// The BuildingClass `pDock` argument of the ExitObject/GrandOpening hooks
/// has no VERA owner; those sites are not modelled.
pub(crate) fn get_landing_dir(
    entity: &GameEntity,
    entities: &EntityStore,
    frame: u32,
    pose_dir: i32,
    rules_context: Option<(&RuleSet, &StringInterner)>,
) -> u16 {
    let Some((rules, interner)) = rules_context else {
        return (landing_direction(entity, entities, frame, pose_dir) as u16).wrapping_shl(13);
    };
    #[cfg(not(feature = "phobos"))]
    {
        let _ = (rules, interner);
        return (landing_direction(entity, entities, frame, pose_dir) as u16).wrapping_shl(13);
    }
    #[cfg(feature = "phobos")]
    {
        let dir8 =
            |facing: u16| i32::from(crate::util::direction_tables::dir_from_facing16(facing));
        let byte_raw = |byte: i32| (byte as u16).wrapping_shl(8);
        // If this is a spawnee, use the spawner's facing.
        if let Some(owner_id) = entity.spawn_owner_id.owner()
            && let Some(owner) = entities.get(owner_id)
        {
            return byte_raw(dir8(owner.body_facing.current(frame)));
        }
        let object = interner.object_type(entity.type_ref(), rules);
        let link = entity
            .radio_contacts
            .slot(0)
            .and_then(|id| entities.get(id));
        let has_dock_or_link = link.is_some();
        if let Some(link) = link {
            if link.category() == EntityCategory::Structure {
                if let Some(building_type) = interner.object_type(link.type_ref(), rules) {
                    let docks = building_type.number_of_docks;
                    let slot = match link.radio_contacts.find_slot(entity.stable_id()) {
                        Some(index) if (index as i32) < docks => index,
                        _ => 0,
                    };
                    if docks > 0
                        && let Some(dir) = building_type
                            .aircraft_docking_dirs()
                            .get(slot)
                            .copied()
                            .flatten()
                    {
                        return byte_raw(i32::from(dir));
                    }
                    let default_to_pose = building_type
                        .phobos_aircraft_docking_dir_default_to_pose_dir
                        .unwrap_or(
                            rules
                                .phobos_globals
                                .aircraft_docking_dir_default_to_pose_dir,
                        );
                    if !default_to_pose {
                        return byte_raw(dir8(link.body_facing.current(frame)));
                    }
                }
            } else if object.is_some_and(|object| !object.airport_bound) {
                return byte_raw(dir8(link.body_facing.current(frame)));
            }
        }
        let Some(object) = object else {
            return (landing_direction(entity, entities, frame, pose_dir) as u16).wrapping_shl(13);
        };
        if let Some(landing_dir) = object.phobos_landing_dir {
            if object.airport_bound || landing_dir >= 0 {
                return byte_raw(landing_dir & 0xFF);
            }
            return byte_raw(dir8(entity.body_facing.current(frame)));
        }
        if has_dock_or_link {
            return byte_raw(pose_dir);
        }
        let field = rules
            .phobos_globals
            .pose_dir_field
            .unwrap_or(pose_dir.wrapping_mul(32));
        let _ = interner;
        return byte_raw(field & 0xFF);
    }
    #[cfg(not(feature = "phobos"))]
    unreachable!("the non-Phobos arm returns before the GetLandingDir arms")
}

#[cfg(test)]
#[cfg(all(test, tests_sim_a_b))]
mod tests {
    use super::*;
    use crate::rules::ini_parser::IniFile;

    #[test]
    fn carryall_reader_matches_original() {
        let vectors: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
            "tools/spatial_oracle/fly_landing_base.json",
        ))
        .unwrap();
        let rows = vectors["rules"].as_array().unwrap();
        assert_eq!(rows.len(), 9);
        for row in rows {
            let field = row["raw"]
                .as_str()
                .map_or(String::new(), |raw| format!("Carryall={raw}\n"));
            let rules = RuleSet::from_ini(&IniFile::from_str(&format!(
                "[AircraftTypes]\n0=TEST\n[TEST]\nStrength=100\n{field}"
            )))
            .unwrap();
            assert_eq!(
                rules.object("TEST").unwrap().carryall,
                row["carryall"].as_bool().unwrap(),
                "{row}"
            );
        }
    }
    /// Phobos `AircraftExt::GetLandingDir` (`Ext/Aircraft/Body.cpp:128-200`)
    /// at `FlyLocomotionClass::FlightUpdate` (`0x4CF31C`): the spawner, the
    /// linked building's docking dir and facing fallback, the type
    /// `LandingDir`, and the pose/field fallbacks. Results are DirTypes.
    #[cfg(feature = "phobos")]
    #[test]
    fn get_landing_dir_follows_the_native_branch_order() {
        use crate::map::entities::EntityCategory;
        use crate::rules::art_data::ArtRegistry;
        use crate::sim::entity_store::EntityStore;
        use crate::sim::game_entity::GameEntity;
        use crate::sim::intern::test_interner;

        fn rules(aircraft: &str, building: &str, audio_visual: &str) -> RuleSet {
            RuleSet::from_ini(&IniFile::from_str(&format!(
                "[AudioVisual]\nPoseDir=3\n{audio_visual}\
                 [AircraftTypes]\n0=HELI\n[HELI]\nStrength=100\n{aircraft}\
                 [BuildingTypes]\n0=PAD\n[PAD]\nStrength=100\nNumberOfDocks=1\n{building}"
            )))
            .unwrap()
        }

        let mut interner = test_interner();
        let heli_ref = interner.intern("HELI");
        let pad_ref = interner.intern("PAD");
        let mut entities = EntityStore::new();
        let mut heli = GameEntity::test_default_of_category(
            1,
            "HELI",
            "Americans",
            10,
            10,
            EntityCategory::Aircraft,
        );
        heli.set_type_ref_for_test(heli_ref);
        heli.body_facing.snap(5u16 << 13, 0);
        entities.insert(heli);
        let mut pad = GameEntity::test_default_of_category(
            2,
            "PAD",
            "Americans",
            12,
            10,
            EntityCategory::Structure,
        );
        pad.set_type_ref_for_test(pad_ref);
        pad.body_facing.snap(6u16 << 13, 0);
        entities.insert(pad);

        let landing = |rules: &RuleSet, entities: &EntityStore, id: u64| {
            get_landing_dir(
                entities.get(id).unwrap(),
                entities,
                0,
                rules.general.pose_dir,
                Some((rules, &interner)),
            )
        };
        let raw = |byte: u16| byte.wrapping_shl(8);

        // The type key: AirportBound masks & 0xFF; a negative value on an
        // unbound aircraft keeps its own facing.
        let bound = rules("LandingDir=64\nAirportBound=yes\n", "", "");
        assert_eq!(landing(&bound, &entities, 1), raw(64));
        let unbound = rules("LandingDir=-1\n", "", "");
        assert_eq!(landing(&unbound, &entities, 1), raw(5));
        let plain = rules("", "", "");
        // No link and no type key: the field pose dir, `PoseDir * 32` masked.
        assert_eq!(landing(&plain, &entities, 1), raw(96));
        let field = rules("", "", "PoseDir.Field=200\n");
        assert_eq!(landing(&field, &entities, 1), raw(200));

        // A linked building's docking dir wins; without one the default
        // `DefaultToPoseDir=yes` falls through to the pose dir.
        let mut linked = EntityStore::new();
        linked.insert(entities.get(1).unwrap().clone());
        linked.insert(entities.get(2).unwrap().clone());
        if let Some(heli) = linked.get_mut(1) {
            heli.radio_contacts.insert(2);
        }
        let docked = rules("", "AircraftDockingDir=32\n", "");
        assert_eq!(landing(&docked, &linked, 1), raw(32));
        assert_eq!(
            docked.object("PAD").unwrap().aircraft_docking_dirs(),
            [Some(32u8)]
        );
        // The base key seeds the running value; a missing later key keeps it,
        // `none` clears it, and an out-of-range integer keeps the current.
        let multi = rules(
            "",
            "NumberOfDocks=3\nAircraftDockingDir=32\nAircraftDockingDir0=64\n\
             AircraftDockingDir2=999\n",
            "",
        );
        assert_eq!(
            multi.object("PAD").unwrap().aircraft_docking_dirs(),
            [Some(64u8), Some(64), Some(64)],
            "missing and out-of-range keys keep the running DirType"
        );
        let cleared = rules("", "AircraftDockingDir=32\nAircraftDockingDir0=none\n", "");
        assert_eq!(
            cleared.object("PAD").unwrap().aircraft_docking_dirs(),
            [Some(32u8)],
            "none clears the running value; index 0 keeps the seeded dir"
        );
        assert_eq!(
            landing(&plain, &linked, 1),
            raw(3),
            "the pose dir when linked"
        );

        // `DefaultToPoseDir=no` (global or building ART) returns the pad's
        // facing instead.
        let facing = rules("", "", "AircraftDockingDir.DefaultToPoseDir=no\n");
        assert_eq!(landing(&facing, &linked, 1), raw(6));
        let mut art_rules = rules("", "", "");
        art_rules.install_art_data(ArtRegistry::from_ini(&IniFile::from_str(
            "[PAD]\nAircraftDockingDir.DefaultToPoseDir=no\n",
        )));
        assert_eq!(
            art_rules
                .object("PAD")
                .unwrap()
                .phobos_aircraft_docking_dir_default_to_pose_dir,
            Some(false),
            "the building ART key projects onto the type"
        );
        assert_eq!(landing(&art_rules, &linked, 1), raw(6));

        // A spawner's facing outranks every key.
        let mut spawned = EntityStore::new();
        let mut heli = entities.get(1).unwrap().clone();
        heli.spawn_owner_id.set_for_test(Some(2));
        spawned.insert(heli);
        spawned.insert(entities.get(2).unwrap().clone());
        let keyed = rules("LandingDir=64\nAirportBound=yes\n", "", "");
        assert_eq!(landing(&keyed, &spawned, 1), raw(6));
    }
}
