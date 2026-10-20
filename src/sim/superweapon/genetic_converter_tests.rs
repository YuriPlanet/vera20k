//! The Genetic Mutator (`genetic_converter`): its native comparisons against
//! `tools/superweapon_oracle.json` (`genetic_launch`, `infantry_mutate_death`,
//! `make_infantry`; `--check` regenerates them), and the mutation through
//! the production receivers and frames on retail rules.

use super::{OBSERVED, Observed, launch};
use crate::map::bridge_facts::BRIDGE_FLAG_STRUCTURAL;
use crate::rules::{ini_parser::IniFile, ruleset::RuleSet};
use crate::sim::anim_class::{AnimRemap, AnimWorldCoord};
use crate::sim::combat::{EntityDamageEvent, RAD_NO_ATTACKER, ReceiverCallFlags};
use crate::sim::components::DriveCoord;
use crate::sim::house_state::HouseState;
use crate::sim::intern::InternedId;
use crate::sim::mission::{MissionId, MissionType};
use crate::sim::movement::locomotor::MovementLayer;
use crate::sim::occupancy::CellListInsertion;
use crate::sim::superweapon::chronosphere_tests::{
    charge_super, click, place, retail_rules_with_sequences, step, world_with,
};
use crate::sim::world::{SimSoundEvent, Simulation};
use serde_json::Value;
use std::collections::BTreeSet;

const MUTATOR: &str = "GeneticConverterSpecial";

/// The keys case 9, the InfDeath 9 arm and the MakeInfantry end read, with
/// an infantryman (E1), the Brute, a dog (NotHuman), a tank and a 1x1
/// building. The warheads have retail's `InfDeath=` and `Verses=`.
const RULES: &str = "\
[General]\nIonBlast=RING1\nInfantryMutate=GENDEATH\nAnimToInfantry=BRUTE\n\
[SpecialWeapons]\nMutateWarhead=Mutate\nMutateExplosionWarhead=MutateExplosion\n\
[InfantryTypes]\n0=E1\n1=BRUTE\n2=DOG\n\
[VehicleTypes]\n0=MTNK\n\
[AircraftTypes]\n[BuildingTypes]\n0=GAPILL\n\
[SuperWeaponTypes]\n0=GeneticConverterSpecial\n\
[GeneticConverterSpecial]\nType=GeneticConverter\nRechargeTime=5\n\
[E1]\nStrength=125\nArmor=none\nSpeed=4\n\
[BRUTE]\nStrength=200\nArmor=plate\nSpeed=4\n\
[DOG]\nStrength=100\nArmor=none\nSpeed=8\nNotHuman=yes\n\
[MTNK]\nStrength=300\nArmor=heavy\nSpeed=6\n\
Locomotor={4A582741-9839-11d1-B709-00A024DDAFD1}\n\
[GAPILL]\nStrength=400\nArmor=concrete\n\
[Warheads]\n0=Mutate\n1=MutateExplosion\n\
[Mutate]\nInfDeath=9\nVerses=100%,100%,100%,0%,0%,0%,0%,0%,0%,0%,0%\n\
[MutateExplosion]\nInfDeath=9\nCellSpread=5\nPercentAtMax=1\n\
Verses=100%,100%,100%,0%,0%,0%,0%,0%,0%,0%,0%\n";

/// [`RULES`] with `extra` merged over it, and RING1 and GENDEATH (retail's
/// `MakeInfantry=0`) bound.
fn rules(extra: &str) -> RuleSet {
    let mut ini = crate::sim::arena_fixture::clear_ground_ini(RULES);
    ini.merge(&IniFile::from_str(extra));
    let mut rules = RuleSet::from_ini(&ini).expect("genetic mutator fixture rules");
    let mut art = crate::rules::art_data::ArtRegistry::from_ini(&IniFile::from_str(
        "[RING1]\nRate=900\n[GENDEATH]\nRate=900\nMakeInfantry=0\n",
    ));
    art.bind_anim_frame_count_for_test("RING1", 15);
    art.bind_anim_frame_count_for_test("GENDEATH", 20);
    rules.replace_art_registry_for_test(art);
    rules
}

fn oracle() -> &'static Value {
    crate::test_fixture::json("tools/superweapon_oracle.json")
}

fn rows<'a>(oracle: &'a Value, section: &str) -> &'a [Value] {
    oracle[section].as_array().unwrap()
}

fn int(value: &Value) -> i32 {
    i32::try_from(value.as_i64().unwrap()).unwrap()
}

fn coord(value: &Value) -> [i32; 3] {
    [int(&value[0]), int(&value[1]), int(&value[2])]
}

/// The row's events named `name`, in order.
fn events<'a>(row: &'a Value, name: &'a str) -> impl Iterator<Item = &'a Value> + 'a {
    row["events"]
        .as_array()
        .unwrap()
        .iter()
        .filter(move |event| event[0] == name)
}

/// The anims of type `name`.
fn anims_of<'a>(sim: &'a Simulation, name: &str) -> Vec<&'a crate::sim::anim_class::AnimObject> {
    let Some(type_id) = sim.interner.get(name) else {
        return Vec::new();
    };
    sim.substrate
        .anims
        .iter()
        .map(|(_, anim)| anim)
        .filter(|anim| anim.type_id() == type_id)
        .collect()
}

/// Case 9 against `genetic_launch`, through the production launch on a flat
/// 64-square map. Each row's cells carry its levels and bridge bits, and its
/// infantry stand on their cells' lists in the row's order, each of a type
/// with the row's Strength. Compared:
/// - the charge gate;
/// - the IonBlast anim and its constructor row;
/// - the launch event (EVA and sound, `launch_lines_match_native` in the
///   app) before radar event 13;
/// - under `MutateExplosion=`, the area damage's coordinate, damage, source,
///   warhead and house;
/// - otherwise each ReceiveDamage call with the cell whose list held the
///   object: the Strength, distance 0, MutateWarhead=, no source, the
///   IgnoreDefenses and sixth arguments and the house.
///
/// Every infantryman's InfDeath 9 arm takes it off its list inside the call,
/// as the unlink row's stub does to one object, so each list of several
/// objects checks that the walk reads the next one first (`0x006CD9E0`).
///
/// Left out:
/// - The mute rows: VERA has no `0x00A8B538` (module RESIDUAL).
/// - The WhatAmI row: one cell's list holds a unit, a building and infantry
///   with Strength 0, -5 and `0x7FFFFFFF`, which no VERA cell admits.
///   `a_walk_damages_only_the_infantry_of_the_block` runs the filter.
/// - The drop-next row: a fixture stub takes the next object off its list
///   inside the call, which no receiver here does. By reading,
///   `cell_grid::live_successor` then ends the list at that object, as for
///   the Iron Curtain's unlink row.
/// - An object at a negative cell: no VERA cell list holds one. Real gamemd
///   answers the shared dummy there, whose lists are empty.
#[test]
fn the_launch_matches_native() {
    let oracle = oracle();
    let mut compared = 0;
    for row in rows(&oracle, "genetic_launch") {
        let objects = rows(row, "objects");
        if row["mute"] == true
            || objects
                .iter()
                .any(|object| object["drop_next"] == true || int(&object["what"]) != 0xF)
        {
            continue;
        }
        let target = (int(&row["target"][0]), int(&row["target"][1]));
        let at = |offset: &Value| (target.0 + int(&offset[0]), target.1 + int(&offset[1]));
        let levels: Vec<((u16, u16), u8)> = rows(row, "levels")
            .iter()
            .map(|level| {
                let (x, y) = at(&level[0]);
                ((x as u16, y as u16), int(&level[1]) as u8)
            })
            .collect();
        let strengths: BTreeSet<i32> = objects
            .iter()
            .map(|object| int(&object["strength"]))
            .collect();
        let mut extra = String::from("[InfantryTypes]\n");
        for (n, strength) in strengths.iter().enumerate() {
            extra += &format!("{}=S{strength}\n", 10 + n);
        }
        for strength in &strengths {
            extra += &format!("[S{strength}]\nStrength={strength}\nArmor=none\nSpeed=4\n");
        }
        let mut rules = rules(&extra);
        rules.general.mutate_explosion = row["explosion"] == true;
        let (rules, mut sim, owner) = world_with(rules, 64, &levels);
        for bridge in rows(row, "bridges") {
            let (x, y) = at(bridge);
            let terrain = sim.resolved_terrain.as_mut().unwrap();
            let index = terrain.index(x as u16, y as u16).unwrap();
            terrain.cells_mut_for_test()[index].bridge_facts.raw_flags = BRIDGE_FLAG_STRUCTURAL;
        }
        // Prepended in reverse, each list ends in the row's order.
        let mut ids = vec![None; objects.len()];
        for (index, object) in objects.iter().enumerate().rev() {
            let (x, y) = at(&object["offset"]);
            if x < 0 || y < 0 {
                continue;
            }
            let cell = (x as u16, y as u16);
            let level = levels
                .iter()
                .find(|(at, _)| *at == cell)
                .map_or(0, |&(_, level)| level);
            let kind = format!("S{}", int(&object["strength"]));
            let bridge = object["bridge"] == true;
            ids[index] = Some(place(&mut sim, &rules, &kind, cell, level, bridge, false));
        }

        let sw_type = charge_super(&mut sim, owner, MUTATOR);
        let charged = row["charged"] == true;
        if !charged {
            sim.super_weapons
                .map_mut_for_test()
                .get_mut(&owner)
                .unwrap()
                .get_mut(&sw_type)
                .unwrap()
                .is_ready = false;
        }
        sim.sound_events.clear_for_test();
        let cell = (target.0 as u16, target.1 as u16);
        OBSERVED.set(Some(Vec::new()));
        let launched = launch(
            &mut sim,
            &rules,
            owner,
            cell.0,
            cell.1,
            sw_type,
            None,
            crate::sim::world::FrameEffects::default(),
        );
        let observed = OBSERVED.take().unwrap();
        assert_eq!(launched, charged, "{row}");

        let rings = anims_of(&sim, "RING1");
        let native_anims: Vec<&Value> = events(row, "anim").collect();
        assert_eq!(rings.len(), native_anims.len(), "{row}");
        if let (Some(anim), Some(native)) = (rings.first(), native_anims.first()) {
            let world = anim.world_coord();
            assert_eq!([world.x, world.y, world.z], coord(&native[2]), "{row}");
            // (delay, loopCount, drawFlags, zAdjust, reverse).
            assert_eq!(native[3], serde_json::json!([0, 1, 0x600, 0, 0]));
            assert_eq!((anim.draw_flags(), anim.z_adjust()), (0x600, 0), "{row}");
        }

        // PlayEVA and PlayAtCoord (one launch event), then
        // CreateRadarEvent(13, cell).
        let lines: Vec<String> = row["events"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|event| match event[0].as_str().unwrap() {
                "eva" => Some("launched".to_string()),
                "radar_event" => Some(format!(
                    "radar {} {},{}",
                    event[1], event[2][0], event[2][1]
                )),
                _ => None,
            })
            .collect();
        let pushed: Vec<String> = sim
            .sound_events
            .iter()
            .filter_map(|event| match event {
                SimSoundEvent::SuperWeaponLaunched {
                    owner: by,
                    sw_type: kind,
                    rx,
                    ry,
                } if *by == owner && *kind == sw_type && (*rx, *ry) == cell => {
                    Some("launched".to_string())
                }
                SimSoundEvent::RadarEvent { radar } => Some(format!(
                    "radar {} {},{}",
                    radar.event_type as i32, radar.rx, radar.ry
                )),
                _ => None,
            })
            .collect();
        assert_eq!(pushed, lines, "{row}");

        let warhead = sim.interner.intern("Mutate");
        let explosion = sim.interner.intern("MutateExplosion");
        let mut looked = (0, 0);
        let mut native = Vec::new();
        for event in row["events"].as_array().unwrap() {
            match event[0].as_str().unwrap() {
                "cell" => looked = (int(&event[1][0]) as i16, int(&event[1][1]) as i16),
                "area_damage" => {
                    // No source, MutateExplosionWarhead=, affectsTiberium 0
                    // (module RESIDUAL), the Super's house.
                    assert_eq!(int(&event[3]), 0);
                    assert_eq!(event[4], true);
                    assert_eq!(int(&event[5]), 0);
                    assert_eq!(event[6], true);
                    native.push(Observed::AreaDamage(
                        coord(&event[1]),
                        int(&event[2]),
                        (RAD_NO_ATTACKER, Some(owner), explosion),
                    ));
                }
                "damage" => {
                    // MutateWarhead=, no source, the Super's house.
                    assert_eq!(event[4], true);
                    assert_eq!(int(&event[5]), 0);
                    assert_eq!(event[8], true);
                    if let Some(id) = ids[event[1].as_u64().unwrap() as usize] {
                        native.push(Observed::ReceiveDamage(
                            looked,
                            EntityDamageEvent::direct_receiver(
                                id,
                                int(&event[2]),
                                int(&event[3]),
                                RAD_NO_ATTACKER,
                                Some(owner),
                                warhead,
                                ReceiverCallFlags {
                                    ignore_defenses: int(&event[6]) != 0,
                                    arg6: int(&event[7]) != 0,
                                },
                            ),
                        ));
                    }
                }
                _ => {}
            }
        }
        assert_eq!(observed, native, "{row}");
        compared += 1;
    }
    assert_eq!(compared, 14);
}

/// The walk's WhatAmI test (`CMP EAX, 0xF`, `0x006CD9EE`): of an
/// infantryman, a tank and a building in the block, only the infantryman
/// receives its Strength.
#[test]
fn a_walk_damages_only_the_infantry_of_the_block() {
    let mut rules = rules("");
    rules.general.mutate_explosion = false;
    let (rules, mut sim, owner) = world_with(rules, 64, &[]);
    let infantry = place(&mut sim, &rules, "E1", (40, 40), 0, false, false);
    let tank = place(&mut sim, &rules, "MTNK", (41, 40), 0, false, false);
    let building = sim
        .spawn_object_at_height("GAPILL", "Americans", 39, 40, 0, 0, &rules)
        .unwrap();
    let sw_type = charge_super(&mut sim, owner, MUTATOR);
    OBSERVED.set(Some(Vec::new()));
    assert!(launch(
        &mut sim,
        &rules,
        owner,
        40,
        40,
        sw_type,
        None,
        crate::sim::world::FrameEffects::default()
    ));
    let observed = OBSERVED.take().unwrap();
    let damaged: Vec<u64> = observed
        .iter()
        .filter_map(|call| match call {
            Observed::ReceiveDamage(_, event) => Some(event.target_id),
            Observed::AreaDamage(..) => None,
        })
        .collect();
    assert_eq!(damaged, vec![infantry]);
    for id in [tank, building] {
        let object = sim.substrate.entities.get(id).unwrap();
        assert!(!object.death.is_dying() && object.health.current() > 0);
    }
}

/// `InfantryClass::ReceiveDamage`'s InfDeath 9 arm against
/// `infantry_mutate_death`, through the production receiver: one
/// infantryman killed by an `InfDeath=9` warhead at the row's Location on a
/// flat map whose cell has the row's Foot land row, with the row's OnBridge
/// byte. The row's listed objects are put on the cell's ground list; a row
/// without a spot has a vehicle's raw bit on the cell, which refuses
/// PlaceInfantryInCell. The source is a Russian infantryman, the house
/// argument the Americans. Compared:
/// - mutated: the infantryman is UnInit and GENDEATH is built at the
///   Location with `(0, 1, 0x600, 0, 0)`, owned by the source's house, else
///   the house argument, with that house's remap;
/// - refused: Do_Action Die2, no anim, and the infantryman stays on its list
///   with its raw bit cleared (the unmark is not undone).
///
/// The oracle stubs the calls the arm makes (Unmark, Mark, the cell lookups,
/// PlaceInfantryInCell, the anim's constructor and its Mark); here they are
/// VERA's own, so only the arm's decisions and its writes are compared.
#[test]
fn the_mutation_arm_matches_native() {
    let oracle = oracle();
    let mut compared = 0;
    for row in rows(&oracle, "infantry_mutate_death") {
        let [x, y, z] = coord(&row["location"]);
        if x < 0 {
            // VERA stores no infantryman left of the map: its cell clamps.
            continue;
        }
        let cell = ((x / 256) as u16, (y / 256) as u16);
        let level = (z / 104) as u8;
        let (rules, mut sim, americans) = world_with(rules(""), 64, &[(cell, level)]);
        let russians = sim.interner.intern("Russians");
        {
            let terrain = sim.resolved_terrain.as_mut().unwrap();
            let index = terrain.index(cell.0, cell.1).unwrap();
            let foot = row["foot_cost"].as_f64().unwrap();
            terrain.cells_mut_for_test()[index].speed_costs.foot = Some((foot * 100.0) as u8);
        }
        let victim = sim
            .construct_object_limbo_at_height(
                "E1",
                "Americans",
                cell.0,
                cell.1,
                0,
                level,
                &rules,
                crate::sim::world::FrameEffects::default(),
            )
            .unwrap();
        {
            let entity = sim.substrate.entities.get_mut(victim).unwrap();
            entity.position.put_location(DriveCoord { x, y, z });
            entity.position.set_z_for_test(level);
        }
        assert!(matches!(
            sim.reveal(victim),
            crate::sim::world::RevealOutcome::Revealed { .. }
        ));
        sim.substrate
            .entities
            .get_mut(victim)
            .unwrap()
            .set_on_bridge(row["on_bridge"] == true);
        // The listed objects, each a real object of its kind elsewhere.
        for (n, what) in rows(row, "listed").iter().enumerate() {
            let (kind, insertion, sub_cell) = match int(what) {
                6 => ("GAPILL", CellListInsertion::AppendBuilding, None),
                1 => ("MTNK", CellListInsertion::PrependNonBuilding, None),
                _ => ("E1", CellListInsertion::PrependNonBuilding, Some(2)),
            };
            let other = sim
                .spawn_object_at_height(kind, "Russians", 10 + 2 * n as u16, 10, 0, 0, &rules)
                .unwrap();
            sim.substrate.occupancy.add(
                cell.0,
                cell.1,
                other,
                MovementLayer::Ground,
                sub_cell,
                insertion,
            );
        }
        if row["spot"] == false {
            sim.substrate.raw_cell_occupation.mark_ground(
                cell.0,
                cell.1,
                crate::sim::cell_kernel::INFANTRY_OCCUPATION_VEHICLE_BIT,
            );
        }
        let source = if row["source"] == true {
            sim.spawn_object_at_height("E1", "Russians", 20, 20, 0, 0, &rules)
                .unwrap()
        } else {
            RAD_NO_ATTACKER
        };
        let house = (row["house"] == true).then_some(americans);
        let bits_before = sim.substrate.raw_cell_occupation.bits_at(
            crate::sim::occupancy::RawCellKey::Real(cell.0, cell.1),
            MovementLayer::Ground,
        );

        let event = EntityDamageEvent::direct_receiver(
            victim,
            10_000,
            0,
            source,
            house,
            sim.interner.intern("Mutate"),
            ReceiverCallFlags {
                ignore_defenses: true,
                arg6: false,
            },
        );
        sim.commit_direct_damage_receiver(
            &rules,
            None,
            event,
            crate::sim::world::FrameEffects::default(),
        );

        let mutants = anims_of(&sim, "GENDEATH");
        let entity = sim.substrate.entities.get(victim);
        if row["mutated"] == true {
            assert!(
                entity.is_none_or(|entity| !entity.lifecycle.object_alive()),
                "{row}"
            );
            let [anim] = mutants[..] else {
                panic!("one InfantryMutate anim: {row}");
            };
            assert_eq!(anim.world_coord(), AnimWorldCoord { x, y, z }, "{row}");
            assert_eq!((anim.draw_flags(), anim.z_adjust()), (0x600, 0), "{row}");
            let owner = match row["anim_owner"].as_str() {
                Some("source") => Some(russians),
                Some("house") => Some(americans),
                _ => None,
            };
            assert_eq!(anim.owner_house(), owner, "{row}");
            assert_eq!(anim.remap(), owner.map(AnimRemap::House), "{row}");
            assert_eq!(events(row, "anim_mark").count(), 1);
        } else {
            assert_eq!(row["action"], serde_json::json!([12, 1, 0]));
            let entity = entity.expect("the dying infantryman stays");
            assert!(entity.lifecycle.object_alive(), "{row}");
            // A corpse at Health 0 (the fixture binds no Die2 frames, so
            // the zero-count refusal at 0x0051D70F leaves its Doing).
            assert!(!entity.death.is_dying(), "{row}");
            assert_eq!(entity.health.current(), 0, "{row}");
            assert!(mutants.is_empty(), "{row}");
            assert!(
                sim.substrate
                    .occupancy
                    .contains_entity(cell.0, cell.1, victim)
            );
            let bits = sim.substrate.raw_cell_occupation.bits_at(
                crate::sim::occupancy::RawCellKey::Real(cell.0, cell.1),
                MovementLayer::Ground,
            );
            assert_ne!(bits_before & 0x1F, 0, "{row}");
            assert_eq!(bits & 0x1F, 0, "{row}");
        }
        compared += 1;
    }
    assert_eq!(compared, 18);
}

/// AnimClass::AI's MakeInfantry end against `make_infantry`
/// ([`Simulation::anim_make_infantry`]), on a map whose cell under the
/// Location stands at level 2 (its GetCoords Z 208) and carries the row's
/// bridge bit. The houses are the row's (side, Defeated, IsHuman) in
/// HouseClass::Array order, each of the type `[Sides]` lists as its side
/// (HouseType `+0xBC`); where the row's Find_Index answers -1, side 3 keeps
/// its index under another name. `AnimToInfantry=` lists `count` types. A
/// refused Unlimbo comes from a vehicle's raw bit on the cell. Compared:
/// whether the anim ends or retries (and its stage), the infantryman created
/// (its type and house), the anim's owner, the bridge lift and the Hunt
/// queue.
///
/// Left out: the rows whose `MakeInfantry=` equals the count or is below -1,
/// which read outside the vector (the RESIDUAL), and the -1 row, which never
/// reaches the block.
#[test]
fn the_make_infantry_end_matches_native() {
    let oracle = oracle();
    let mut compared = 0;
    for row in rows(&oracle, "make_infantry") {
        let make_infantry = int(&row["make_infantry"]);
        let count = int(&row["count"]);
        if make_infantry == -1 || make_infantry == count || make_infantry < -1 {
            continue;
        }
        let [x, y, z] = coord(&row["location"]);
        let civilian = int(&row["civilian_side"]) != -1;
        let kinds: Vec<String> = (0..count.max(4)).map(|n| format!("INF{n}")).collect();
        let ini = format!(
            "[Countries]\n0=Americans\n1=Russians\n2=YuriCountry\n3=Neutral\n\
             [Americans]\n[Russians]\n[YuriCountry]\n[Neutral]\n\
             [Sides]\nAllied=Americans\nSoviet=Russians\nThirdSide=YuriCountry\n{}\
             [General]\n{}\n[InfantryTypes]\n{}\n[VehicleTypes]\n[AircraftTypes]\n\
             [BuildingTypes]\n{}",
            if civilian {
                "Civilian=Neutral\n"
            } else {
                "Other=Neutral\n"
            },
            if count > 0 {
                format!("AnimToInfantry={}", kinds[..count as usize].join(","))
            } else {
                String::new()
            },
            kinds
                .iter()
                .enumerate()
                .map(|(n, kind)| format!("{n}={kind}\n"))
                .collect::<String>(),
            kinds
                .iter()
                .map(|kind| format!("[{kind}]\nStrength=100\nArmor=none\nSpeed=4\n"))
                .collect::<String>(),
        );
        let mut rules = RuleSet::from_ini(&IniFile::from_str(&ini)).unwrap();
        let mut art = crate::rules::art_data::ArtRegistry::from_ini(&IniFile::from_str(
            "[GENDEATH]\nRate=900\nMakeInfantry=0\n",
        ));
        art.bind_anim_frame_count_for_test("GENDEATH", 20);
        rules.replace_art_registry_for_test(art);
        assert_eq!(
            rules.general.anim_to_infantry,
            kinds[..count as usize].to_vec()
        );

        let cell = ((x.max(0) / 256) as u16, (y / 256) as u16);
        let (rules, mut sim, _) = world_with(rules, 64, &[(cell, 2)]);
        sim.houses.clear_for_test();
        let houses: Vec<InternedId> = rows(row, "houses")
            .iter()
            .enumerate()
            .map(|(n, house)| {
                let id = sim.interner.intern(&format!("House{n}"));
                let side = int(&house[0]) as u8;
                let country =
                    ["Americans", "Russians", "YuriCountry", "Neutral"][usize::from(side)];
                let country = sim.interner.intern(country);
                let mut state = HouseState::new(id, side, Some(country), house[2] == true, 0, 10);
                state.set_is_defeated_for_test(house[1] == true);
                sim.houses.register(state);
                id
            })
            .collect();
        if row["bridge"] == true {
            let terrain = sim.resolved_terrain.as_mut().unwrap();
            let index = terrain.index(cell.0, cell.1).unwrap();
            terrain.cells_mut_for_test()[index].bridge_facts.raw_flags = BRIDGE_FLAG_STRUCTURAL;
        }
        if row["unlimbo"] == false {
            sim.substrate.raw_cell_occupation.mark_ground(
                cell.0,
                cell.1,
                crate::sim::cell_kernel::INFANTRY_OCCUPATION_VEHICLE_BIT,
            );
        }
        let anim = super::super::spawn_super_anim(&mut sim, &rules, "GENDEATH", [x, y, z])
            .expect("the MakeInfantry anim");
        if let Some(owner) = row["owner"].as_u64() {
            sim.substrate
                .set_anim_owner_house(anim, houses[owner as usize]);
        }
        sim.substrate
            .anims
            .get_mut(anim)
            .unwrap()
            .runtime_mut_for_test()
            .current_frame = int(&row["stage"]);
        let before: BTreeSet<u64> = sim.substrate.entities.keys_sorted().into_iter().collect();

        let ends = sim.anim_make_infantry(
            anim,
            make_infantry,
            &rules,
            None,
            crate::sim::world::FrameEffects::default(),
        );

        assert_eq!(ends, row["end"] == "uninit", "{row}");
        if !ends {
            assert_eq!(
                sim.anim(anim).unwrap().runtime().current_frame,
                int(&row["stage_after"]),
                "{row}"
            );
        }
        let created: Vec<u64> = sim
            .substrate
            .entities
            .keys_sorted()
            .into_iter()
            .filter(|id| !before.contains(id))
            .collect();
        let native_create: Vec<&Value> = events(row, "create").collect();
        assert_eq!(created.len(), native_create.len(), "{row}");
        if let (Some(&id), Some(create)) = (created.first(), native_create.first()) {
            let infantry = sim.substrate.entities.get(id).unwrap();
            assert_eq!(
                sim.interner.resolve(infantry.type_ref()),
                format!("INF{}", int(&create[1])),
                "{row}"
            );
            assert_eq!(infantry.owner(), houses[int(&create[2]) as usize], "{row}");
            if ends {
                assert!(!infantry.lifecycle.in_limbo(), "{row}");
                let lifted = events(row, "mark").count() == 2;
                assert_eq!(infantry.on_bridge(), lifted, "{row}");
                let hunt = events(row, "queue").next().is_some();
                assert_eq!(
                    infantry.mission.queued() == MissionId::from_known(MissionType::Hunt),
                    hunt,
                    "{row}"
                );
            } else {
                // The refused object stays in limbo, as natively.
                assert!(infantry.lifecycle.in_limbo(), "{row}");
            }
        }
        let owner = row["anim_owner"].as_u64().map(|n| houses[n as usize]);
        assert_eq!(
            sim.anim(anim).and_then(|anim| anim.owner_house()),
            owner,
            "{row}"
        );
        compared += 1;
    }
    assert_eq!(compared, 19);
}

/// `InfantryClass::ReceiveDamage`'s NotHuman rung (`0x005184F7`): a dog
/// dies with Die1 whatever the warhead's InfDeath, so the mutator leaves no
/// InfantryMutate anim over it, only a corpse at Health 0. The fixture binds
/// no Die1 frames; the retail test below sees the Die1 Doing.
#[test]
fn a_dog_dies_with_die1_instead_of_mutating() {
    let mut rules = rules("");
    rules.general.mutate_explosion = false;
    let (rules, mut sim, owner) = world_with(rules, 64, &[]);
    let dog = place(&mut sim, &rules, "DOG", (40, 40), 0, false, false);
    let sw_type = charge_super(&mut sim, owner, MUTATOR);
    assert!(launch(
        &mut sim,
        &rules,
        owner,
        40,
        40,
        sw_type,
        None,
        crate::sim::world::FrameEffects::default()
    ));
    let dog = sim.substrate.entities.get(dog).unwrap();
    assert!(!dog.death.is_dying() && dog.lifecycle.object_alive());
    assert_eq!(dog.health.current(), 0);
    assert!(anims_of(&sim, "GENDEATH").is_empty());
}

/// Retail rules with the launch's anims and the infantry sequences bound.
fn retail_rules() -> Option<RuleSet> {
    retail_rules_with_sequences(&[("RING1", 15), ("GENDEATH", 20)])
}

/// Steps until no GENDEATH anim is left, at most `frames` frames.
fn until_mutants_end(sim: &mut Simulation, rules: &RuleSet, frames: usize) {
    for _ in 0..frames {
        if anims_of(sim, "GENDEATH").is_empty() {
            return;
        }
        step(sim, rules);
    }
    panic!("the InfantryMutate anims outlived {frames} frames");
}

/// The Brutes standing in the store, with their cells.
fn brutes(sim: &Simulation) -> Vec<(u64, InternedId, (u16, u16))> {
    sim.substrate
        .entities
        .values()
        .filter(|entity| {
            sim.interner.resolve(entity.type_ref()) == "BRUTE" && !entity.lifecycle.in_limbo()
        })
        .map(|entity| {
            (
                entity.stable_id(),
                entity.owner(),
                (entity.position.rx(), entity.position.ry()),
            )
        })
        .collect()
}

/// The player's Genetic Mutator on retail rules (`MutateExplosion=yes`,
/// `MutateExplosionWarhead=MutateExplosion`, `InfantryMutate=GENDEATH`,
/// `AnimToInfantry=BRUTE`) through the production reader and frames. A
/// click launches on the next frame: the line, then radar event 13 at the
/// cell. Each enemy conscript in the blast is removed under a GENDEATH owned
/// by the Americans, two of them sharing a cell; the dog plays Die1. When
/// each anim ends, a Brute of the Americans stands at its conscript's cell,
/// on Guard (a human house queues nothing).
#[test]
fn retail_mutator_turns_conscripts_into_the_players_brutes() {
    let Some(rules) = retail_rules() else {
        return;
    };
    assert!(rules.general.mutate_explosion);
    assert_eq!(rules.general.mutate_explosion_warhead, "MutateExplosion");
    assert_eq!(rules.general.infantry_death_anim(9), Some("GENDEATH"));
    assert_eq!(rules.general.anim_to_infantry, vec!["BRUTE".to_string()]);
    let (rules, mut sim, americans) = world_with(rules, 64, &[]);
    let conscripts: Vec<u64> = [(40, 40), (40, 40), (41, 40), (40, 42)]
        .into_iter()
        .map(|(x, y)| {
            sim.spawn_object_at_height("E2", "Russians", x, y, 0, 0, &rules)
                .unwrap()
        })
        .collect();
    let dog = sim
        .spawn_object_at_height("DOG", "Russians", 39, 41, 0, 0, &rules)
        .unwrap();
    let cells: Vec<(u16, u16)> = conscripts
        .iter()
        .map(|&id| {
            let entity = sim.substrate.entities.get(id).unwrap();
            (entity.position.rx(), entity.position.ry())
        })
        .collect();
    charge_super(&mut sim, americans, MUTATOR);
    click(&mut sim, &rules, americans, MUTATOR, (40, 40));

    assert!(sim.sound_events.iter().any(|event| matches!(
        event,
        SimSoundEvent::SuperWeaponLaunched { owner, rx: 40, ry: 40, .. } if *owner == americans
    )));
    for &id in &conscripts {
        assert!(
            sim.substrate
                .entities
                .get(id)
                .is_none_or(|entity| !entity.lifecycle.object_alive())
        );
    }
    let mutants = anims_of(&sim, "GENDEATH");
    assert_eq!(mutants.len(), 4);
    assert!(
        mutants
            .iter()
            .all(|anim| anim.owner_house() == Some(americans))
    );
    let dog = sim.substrate.entities.get(dog).unwrap();
    assert_eq!(dog.health.current(), 0);
    assert_eq!(dog.mission_leaf.as_infantry().unwrap().doing(), 11);

    until_mutants_end(&mut sim, &rules, 600);
    let brutes = brutes(&sim);
    assert_eq!(brutes.len(), 4);
    let mut standing: Vec<(u16, u16)> = brutes.iter().map(|&(_, _, at)| at).collect();
    standing.sort_unstable();
    let mut expected = cells.clone();
    expected.sort_unstable();
    assert_eq!(standing, expected);
    for (id, owner, _) in brutes {
        assert_eq!(owner, americans);
        let brute = sim.substrate.entities.get(id).unwrap();
        assert_ne!(
            brute.mission.queued(),
            MissionId::from_known(MissionType::Hunt)
        );
        assert_ne!(
            brute.mission.current(),
            MissionId::from_known(MissionType::Hunt)
        );
    }
}

/// The computer's Genetic Mutator on retail rules, through AI_TryFireSW's
/// AI_Fire_GenMutator arm: it fires at the Americans' infantry, and the
/// Brutes its anims leave are its own and hunt.
#[test]
fn retail_computer_mutator_leaves_hunting_brutes() {
    let Some(rules) = retail_rules() else {
        return;
    };
    let (rules, mut sim, americans) = world_with(rules, 64, &[]);
    let russians = sim.interner.intern("Russians");
    sim.houses
        .get_mut(&russians)
        .unwrap()
        .set_enemy_house_for_test(Some(americans));
    for (x, y) in [(30, 30), (31, 30), (30, 31), (31, 31)] {
        sim.spawn_object_at_height("E1", "Americans", x, y, 0, 0, &rules)
            .unwrap();
    }
    let sw_type = charge_super(&mut sim, russians, MUTATOR);

    super::super::ai_fire::try_fire(
        &mut sim,
        &rules,
        russians,
        None,
        crate::sim::world::FrameEffects::default(),
    );

    assert!(!sim.super_weapons[&russians][&sw_type].is_ready);
    assert!(!anims_of(&sim, "GENDEATH").is_empty());
    until_mutants_end(&mut sim, &rules, 600);
    let brutes = brutes(&sim);
    assert!(!brutes.is_empty());
    for (id, owner, _) in brutes {
        assert_eq!(owner, russians);
        let brute = sim.substrate.entities.get(id).unwrap();
        let hunt = MissionId::from_known(MissionType::Hunt);
        assert!(brute.mission.queued() == hunt || brute.mission.current() == hunt);
    }
}

/// InfantryClass::ReceiveDamage's InfDeath 9 head (`0x00517FF5..
/// 0x00518010`): an infantryman above the ground (GetHeight, vt+0x1C8) takes
/// no damage from an `InfDeath=9` warhead. The walk leaves one standing a
/// lepton up as it was; the one on the ground beside it mutates.
#[test]
fn an_infantryman_above_the_ground_is_not_mutated() {
    let mut rules = rules("");
    rules.general.mutate_explosion = false;
    let (rules, mut sim, owner) = world_with(rules, 64, &[]);
    let grounded = place(&mut sim, &rules, "E1", (40, 40), 0, false, false);
    let raised = place(&mut sim, &rules, "E1", (41, 40), 0, false, false);
    {
        let entity = sim.substrate.entities.get_mut(raised).unwrap();
        entity.position.put_location(DriveCoord {
            x: 41 * 256 + 128,
            y: 40 * 256 + 128,
            z: 1,
        });
    }
    let sw_type = charge_super(&mut sim, owner, MUTATOR);
    assert!(launch(
        &mut sim,
        &rules,
        owner,
        40,
        40,
        sw_type,
        None,
        crate::sim::world::FrameEffects::default()
    ));

    let entity = sim.substrate.entities.get(raised).unwrap();
    assert!(entity.lifecycle.object_alive() && !entity.death.is_dying());
    assert_eq!(entity.health.current(), 125);
    assert!(
        sim.substrate
            .entities
            .get(grounded)
            .is_none_or(|entity| !entity.lifecycle.object_alive())
    );
    assert_eq!(anims_of(&sim, "GENDEATH").len(), 1);
}

/// Each anim arm of InfantryClass::ReceiveDamage ends in UnInit inside the
/// receiver (`0x00518B9A`), before Apply_area_damage dispatches its next
/// record. Three infantrymen hold spots 2 to 4 of a cell, and a fourth,
/// listed after them, stands at its centre (no spot preference). Under
/// `MutateExplosion=` each of the three mutates and leaves its spot, so
/// PlaceInfantryInCell finds one for the fourth, which mutates too.
#[test]
fn a_fourth_infantryman_at_a_full_cells_centre_mutates() {
    use crate::sim::movement::ground_pose;
    let mut rules = rules("");
    rules.general.mutate_explosion = true;
    let (rules, mut sim, owner) = world_with(rules, 64, &[]);
    let spawn = |sim: &mut Simulation, (x, y): (u16, u16)| {
        sim.spawn_object_at_height("E1", "Americans", x, y, 0, 0, &rules)
            .unwrap()
    };
    let fourth = spawn(&mut sim, (41, 40));
    let three: Vec<u64> = (0..3).map(|_| spawn(&mut sim, (40, 40))).collect();
    let spots: BTreeSet<Option<u8>> = three
        .iter()
        .map(|&id| sim.substrate.entities.get(id).unwrap().sub_cell())
        .collect();
    assert_eq!(spots, BTreeSet::from([Some(2), Some(3), Some(4)]));

    // The fourth moves to the centre of (40, 40), behind the three on its list.
    let [x, y] =
        ground_pose::position_world_xy(&sim.substrate.entities.get(fourth).unwrap().position);
    assert!(sim.object_raw_receiver_at(fourth, DriveCoord { x, y, z: 0 }, false));
    let occupancy = &mut sim.substrate.occupancy;
    occupancy.move_entity(
        41,
        40,
        40,
        40,
        fourth,
        MovementLayer::Ground,
        Some(0),
        CellListInsertion::PrependNonBuilding,
    );
    for &id in &three {
        let spot = sim.substrate.entities.get(id).unwrap().sub_cell();
        let occupancy = &mut sim.substrate.occupancy;
        occupancy.remove(40, 40, id);
        occupancy.add(
            40,
            40,
            id,
            MovementLayer::Ground,
            spot,
            CellListInsertion::PrependNonBuilding,
        );
    }
    let centre = DriveCoord {
        x: 40 * 256 + 128,
        y: 40 * 256 + 128,
        z: 0,
    };
    {
        let entity = sim.substrate.entities.get_mut(fourth).unwrap();
        entity.position.put_location(centre);
        entity.set_sub_cell(Some(0));
    }
    assert!(sim.object_raw_receiver_at(fourth, centre, true));
    let listed: Vec<u64> = sim
        .substrate
        .occupancy
        .get(40, 40)
        .unwrap()
        .iter_layer(MovementLayer::Ground)
        .map(|occupant| occupant.entity_id)
        .collect();
    assert_eq!(listed.len(), 4);
    assert_eq!(listed.last(), Some(&fourth));

    let sw_type = charge_super(&mut sim, owner, MUTATOR);
    assert!(launch(
        &mut sim,
        &rules,
        owner,
        40,
        40,
        sw_type,
        None,
        crate::sim::world::FrameEffects::default()
    ));

    let doing = sim
        .substrate
        .entities
        .get(fourth)
        .and_then(|entity| entity.mission_leaf.as_infantry())
        .map(|leaf| leaf.doing());
    assert_eq!(anims_of(&sim, "GENDEATH").len(), 4, "fourth: {doing:?}");
    for &id in three.iter().chain([&fourth]) {
        assert!(
            sim.substrate
                .entities
                .get(id)
                .is_none_or(|entity| !entity.lifecycle.object_alive()),
            "{id} mutated and is gone"
        );
    }
}
