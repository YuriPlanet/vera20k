//! The Iron Curtain (`iron_curtain`): its native comparisons against
//! `tools/superweapon_oracle.json` (`iron_curtain_launch`,
//! `curtain_overrides`; `--check` regenerates them), and the launch's
//! consequences through the production receivers and frames.

use super::{OBSERVED, Observed, launch};
use crate::map::bridge_facts::BRIDGE_FLAG_STRUCTURAL;
use crate::rules::{ini_parser::IniFile, ruleset::RuleSet};
use crate::sim::combat::{EntityDamageEvent, RAD_NO_ATTACKER, ReceiverCallFlags};
use crate::sim::intern::InternedId;
use crate::sim::movement::locomotor::MovementLayer;
use crate::sim::superweapon::chronosphere_tests::{
    charge_super, click, place, retail_rules_binding, world_with,
};
use crate::sim::superweapon::invulnerability::InvulnKind;
use crate::sim::world::{SimSoundEvent, Simulation};
use serde_json::Value;

const IRON_CURTAIN: &str = "IronCurtainSpecial";

/// The keys the launch and its overrides read, with an Iron Curtain, a Foot
/// (MTNK), a Teleport Foot the warp latch can hold (CMIN), an Organic Foot
/// (DLPH, on land), a Terror Drone (DRON), an infantryman (E1) and a 1x1
/// building (GAPILL). `C4Warhead=Super` has retail's `InfDeath=2`, so a
/// curtained infantryman dies into a corpse.
const RULES: &str = "\
[General]\nIronCurtainInvokeAnim=IRONBLST\n\
[CombatDamage]\nC4Warhead=Super\nIronCurtainDuration=750\n\
[InfantryTypes]\n0=E1\n\
[VehicleTypes]\n0=MTNK\n1=CMIN\n2=DLPH\n3=DRON\n\
[AircraftTypes]\n[BuildingTypes]\n0=GAPILL\n\
[SuperWeaponTypes]\n0=IronCurtainSpecial\n\
[IronCurtainSpecial]\nType=IronCurtain\nRechargeTime=5\n\
[E1]\nStrength=125\nArmor=none\nSpeed=4\n\
[MTNK]\nStrength=300\nArmor=heavy\nSpeed=6\n\
Locomotor={4A582741-9839-11d1-B709-00A024DDAFD1}\n\
[CMIN]\nStrength=1000\nArmor=heavy\nSpeed=6\n\
Locomotor={4A582747-9839-11d1-B709-00A024DDAFD1}\n\
[DLPH]\nStrength=200\nArmor=light\nSpeed=8\nOrganic=yes\n\
Locomotor={4A582741-9839-11d1-B709-00A024DDAFD1}\n\
[DRON]\nPrimary=DroneJump\nStrength=100\nArmor=special_1\nSpeed=10\n\
Locomotor={4A582741-9839-11d1-B709-00A024DDAFD1}\n\
[DroneJump]\nDamage=50\nROF=60\nRange=1.83\nProjectile=Invisible\nWarhead=Parasite\n\
[Invisible]\nInviso=yes\n\
[GAPILL]\nStrength=400\nArmor=concrete\n\
[Warheads]\n0=Super\n1=Parasite\n\
[Super]\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\nInfDeath=2\n\
[Parasite]\nVerses=100%,100%,100%,100%,100%,100%,0%,0%,0%,0%,0%\nParasite=yes\n";

/// [`RULES`] with `extra` merged over it and the invoke anim bound (12
/// frames, its retail `Report=`).
fn rules(extra: &str) -> RuleSet {
    let mut ini = IniFile::from_str(RULES);
    ini.merge(&IniFile::from_str(extra));
    let mut rules = RuleSet::from_ini(&ini).expect("iron curtain fixture rules");
    let mut art = crate::rules::art_data::ArtRegistry::from_ini(&IniFile::from_str(
        "[IRONBLST]\nRate=450\nReport=IronCurtainBlast\nTranslucent=yes\n",
    ));
    art.bind_anim_frame_count_for_test("IRONBLST", 12);
    rules.replace_art_registry_for_test(art);
    rules
}

/// Execute1096E3769..6E37DA uses ABD490[0..9], not Launch's B0C038
/// ordering, and never selects +E8 or skips +27C. Alias input proves its
/// Cell GetCoords uses the receiver's canonical coordinate before +5.
#[test]
fn waypoint_curtain_uses_ground_spread_order_including_warped_foot() {
    for waypoint_cell in [(40, 40), (552, 39)] {
        let (rules, mut sim, _) = world_with(rules(""), 64, &[((40, 40), 2)]);
        let terrain = sim.resolved_terrain.as_mut().unwrap();
        let index = terrain.index(40, 40).unwrap();
        terrain.cells_mut_for_test()[index].bridge_facts.raw_flags = BRIDGE_FLAG_STRUCTURAL;
        let offsets = [
            (0, 0),
            (1, -1),
            (0, -1),
            (-1, -1),
            (-1, 0),
            (1, 0),
            (-1, 1),
            (0, 1),
            (1, 1),
        ];
        let mut ordered = Vec::new();
        for &(dx, dy) in &offsets {
            let centre = dx == 0 && dy == 0;
            let id = place(
                &mut sim,
                &rules,
                if centre { "CMIN" } else { "MTNK" },
                ((40 + dx) as u16, (40 + dy) as u16),
                if centre { 2 } else { 0 },
                false,
                centre,
            );
            ordered.push(id);
        }
        let bridge = place(&mut sim, &rules, "MTNK", (40, 40), 2, true, false);
        sim.session
            .install_waypoints(&std::collections::HashMap::from([(
                1,
                crate::map::waypoints::Waypoint {
                    index: 1,
                    rx: waypoint_cell.0,
                    ry: waypoint_cell.1,
                },
            )]));
        sim.sound_events.clear_for_test();
        OBSERVED.set(Some(Vec::new()));
        assert!(super::at_waypoint(
            &mut sim,
            &rules,
            1,
            None,
            crate::sim::world::FrameEffects::default()
        ));
        let called: Vec<_> = OBSERVED
            .take()
            .unwrap()
            .into_iter()
            .filter_map(|event| match event {
                Observed::Curtain(_, id) => Some(id),
                _ => None,
            })
            .collect();
        assert_eq!(called, ordered);
        for &id in &ordered {
            assert_eq!(curtain(&sim, id).unwrap().1, 750);
        }
        assert!(curtain(&sim, bridge).is_none());
        assert!(sim.entities().get(ordered[0]).unwrap().chrono_warp_latch());
        let anim = sim.substrate.anims.iter().next().unwrap().1;
        assert_eq!(
            [
                anim.world_coord().x,
                anim.world_coord().y,
                anim.world_coord().z
            ],
            [10368, 10368, 213]
        );
        assert_eq!((anim.draw_flags(), anim.z_adjust()), (0x600, 0));
        assert!(!sim.sound_events.iter().any(|event| matches!(
            event,
            SimSoundEvent::SuperWeaponLaunched { .. } | SimSoundEvent::RadarEvent { .. }
        )));
        let saved = crate::sim::snapshot::GameSnapshot::save(&sim, 0, 0, "Waypoint IC", 0);
        let restored = crate::sim::snapshot::GameSnapshot::load(&saved)
            .unwrap()
            .sim;
        for id in ordered {
            assert_eq!(curtain(&restored, id), curtain(&sim, id));
        }
    }
}

#[test]
fn waypoint_curtain_calls_existing_infantry_and_organic_receivers_with_null_house() {
    let (rules, mut sim, _) = world_with(rules(""), 64, &[]);
    let infantry = place(&mut sim, &rules, "E1", (40, 40), 0, false, false);
    let organic = place(&mut sim, &rules, "DLPH", (41, 40), 0, false, false);
    let building = place(&mut sim, &rules, "GAPILL", (39, 40), 0, false, false);
    sim.session
        .install_waypoints(&std::collections::HashMap::from([(
            1,
            crate::map::waypoints::Waypoint {
                index: 1,
                rx: 40,
                ry: 40,
            },
        )]));
    OBSERVED.set(Some(Vec::new()));
    assert!(super::at_waypoint(
        &mut sim,
        &rules,
        1,
        None,
        crate::sim::world::FrameEffects::default()
    ));
    let damage: Vec<_> = OBSERVED
        .take()
        .unwrap()
        .into_iter()
        .filter_map(|event| match event {
            Observed::ReceiveDamage(event) => Some(event),
            _ => None,
        })
        .collect();
    assert_eq!(damage.len(), 2);
    for (event, id, strength, ignore_defenses) in [
        (&damage[0], infantry, 125, true),
        (&damage[1], organic, 200, false),
    ] {
        assert_eq!(event.target_id, id);
        assert_eq!(event.damage, strength);
        assert_eq!(event.attacker_id, RAD_NO_ATTACKER);
        assert_eq!(event.source_house, None);
        assert_eq!(event.receiver_flags.ignore_defenses, ignore_defenses);
        let entity = sim.entities().get(id).unwrap();
        assert_eq!(entity.health.current(), 0);
        assert!(entity.invulnerability.is_none());
    }
    // The infantryman is a corpse that keeps IsAlive; the dolphin's VERA-only
    // SHP death carries the dying mark.
    let corpse = sim.entities().get(infantry).unwrap();
    assert!(corpse.lifecycle.object_alive() && !corpse.death.is_dying());
    assert!(sim.entities().get(organic).unwrap().death.is_dying());
    assert_eq!(curtain(&sim, building).unwrap().1, 750);
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

/// The row's events named `name`, in order.
fn events<'a>(row: &'a Value, name: &'a str) -> impl Iterator<Item = &'a Value> + 'a {
    row["events"]
        .as_array()
        .unwrap()
        .iter()
        .filter(move |event| event[0] == name)
}

/// Launch with the walk observed: whether it launched, and what it called.
fn observed_launch(
    sim: &mut Simulation,
    rules: &RuleSet,
    owner: InternedId,
    cell: (u16, u16),
    sw_type: InternedId,
) -> (bool, Vec<Observed>) {
    OBSERVED.set(Some(Vec::new()));
    let launched = launch(
        sim,
        rules,
        owner,
        cell.0,
        cell.1,
        sw_type,
        None,
        crate::sim::world::FrameEffects::default(),
    );
    (launched, OBSERVED.take().unwrap())
}

/// The curtain's (start, duration, kind) on `id`.
fn curtain(sim: &Simulation, id: u64) -> Option<(i32, i32, InvulnKind)> {
    sim.substrate
        .entities
        .get(id)
        .and_then(|entity| entity.invulnerability.as_ref())
        .map(|state| {
            (
                state.timer().start_frame(),
                state.timer().duration(),
                state.kind(),
            )
        })
}

/// The paralysis timer's (start, duration) on `id`.
fn paralysis(sim: &Simulation, id: u64) -> (i32, i32) {
    let timer = sim.substrate.entities.get(id).unwrap().parasite.paralysis();
    (timer.start_frame(), timer.duration())
}

/// Case 1 against `iron_curtain_launch`, through the production launch on a
/// flat 64-square map. Each row's cells carry its levels and bridge bits, and
/// its objects stand on their cells' lists in the row's order. A Foot is a
/// tank, a latched Foot a Teleport tank under the warp latch, and any other
/// object a 1x1 building. Compared:
/// - the charge gate;
/// - the anim and its constructor row;
/// - the EVA (the launch event) before radar event 13;
/// - each IronCurtain call with the cell whose list held the object;
/// - each call's duration and the Iron Curtain's byte, through the curtain
///   and a Foot's paralysis timer.
///
/// Left out:
/// - The mute row: VERA has no `0x00A8B538` (module RESIDUAL).
/// - The unlink row: a fixture stub clears `+0x30` inside the call, which
///   ends that list and moves the walk to the next cell. No Rust test
///   replays it: by reading, `cell_grid::live_successor` ends the list once
///   the object has left every cell list (RemoveContent clears `+0x30`,
///   `0x0047EAF0`), and with retail `C4Warhead=Super` (`InfDeath=2`) a
///   killed infantryman stays on its list for its death sequence.
///   `iron_curtain_command_observes_native_deck_order_after_nested_bridge_drop_in`
///   (`cell_receiver_tests`) runs the other link change, an object moved
///   onto another list inside the call.
/// - An object at a negative cell: no VERA cell list holds one. Real gamemd
///   answers the shared dummy there, whose lists are empty.
///
/// The latched building is placed unlatched: only a Foot carries the latch
/// in VERA, and the row shows a building's latch is not read.
#[test]
fn the_launch_matches_native() {
    let oracle = oracle();
    let mut compared = 0;
    for row in rows(&oracle, "iron_curtain_launch") {
        let objects = rows(row, "objects");
        if row["mute"] == true || objects.iter().any(|object| object["unlink"] == true) {
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
        let mut rules = rules("");
        rules.general.iron_curtain_duration = int(&row["duration"]);
        let (rules, mut sim, owner) = world_with(rules, 64, &levels);
        for bridge in rows(row, "bridges") {
            let (x, y) = at(bridge);
            let terrain = sim.resolved_terrain.as_mut().unwrap();
            let index = terrain.index(x as u16, y as u16).unwrap();
            terrain.cells_mut_for_test()[index].bridge_facts.raw_flags = BRIDGE_FLAG_STRUCTURAL;
        }

        // Prepended in reverse, each list ends in the row's order; a building
        // is appended.
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
            let foot = object["foot"] == true;
            let latch = foot && object["latch"] == true;
            let kind = match (foot, latch) {
                (false, _) => "GAPILL",
                (true, true) => "CMIN",
                (true, false) => "MTNK",
            };
            let bridge = object["bridge"] == true;
            ids[index] = Some(place(&mut sim, &rules, kind, cell, level, bridge, latch));
        }
        for (index, object) in objects.iter().enumerate() {
            let Some(id) = ids[index] else { continue };
            let (x, y) = at(&object["offset"]);
            let layer = if object["bridge"] == true {
                MovementLayer::Bridge
            } else {
                MovementLayer::Ground
            };
            let listed: Vec<u64> = objects
                .iter()
                .zip(&ids)
                .filter(|(other, _)| {
                    other["offset"] == object["offset"] && other["bridge"] == object["bridge"]
                })
                .filter_map(|(_, id)| *id)
                .collect();
            assert_eq!(
                sim.substrate
                    .occupancy
                    .get(x as u16, y as u16)
                    .unwrap()
                    .snapshot_layer(layer),
                listed,
                "{row}: object {index} ({id})"
            );
        }

        let sw_type = charge_super(&mut sim, owner, IRON_CURTAIN);
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
        let frame = sim.session.binary_frame() as i32;
        let cell = (target.0 as u16, target.1 as u16);
        let paralysis_before: Vec<Option<(i32, i32)>> = ids
            .iter()
            .map(|id| id.map(|id| paralysis(&sim, id)))
            .collect();
        let (launched, observed) = observed_launch(&mut sim, &rules, owner, cell, sw_type);
        assert_eq!(launched, charged, "{row}");

        let invoke = sim.interner.intern("IRONBLST");
        let anims: Vec<_> = sim
            .substrate
            .anims
            .iter()
            .map(|(_, anim)| anim)
            .filter(|anim| anim.type_id() == invoke)
            .collect();
        let native_anims: Vec<&Value> = events(row, "anim").collect();
        assert_eq!(anims.len(), native_anims.len(), "{row}");
        if let (Some(anim), Some(native)) = (anims.first(), native_anims.first()) {
            let world = anim.world_coord();
            assert_eq!(
                [world.x, world.y, world.z],
                [int(&native[2][0]), int(&native[2][1]), int(&native[2][2])],
                "{row}"
            );
            // (delay, loopCount, drawFlags, zAdjust, reverse).
            assert_eq!(native[3], serde_json::json!([0, 1, 0x600, 0, 0]));
            assert_eq!((anim.draw_flags(), anim.z_adjust()), (0x600, 0), "{row}");
        }

        // PlayEVA, then CreateRadarEvent(13, cell).
        let lines: Vec<String> = row["events"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|event| match event[0].as_str().unwrap() {
                "eva" => Some(format!("eva {}", event[1].as_str().unwrap())),
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
                    world,
                } if *by == owner
                    && *kind == sw_type
                    && (world.to_cell_sub_z().0, world.to_cell_sub_z().1) == cell =>
                {
                    Some("eva EVA_IronCurtainActivated".to_string())
                }
                SimSoundEvent::RadarEvent { radar } => Some(format!(
                    "radar {} {},{}",
                    radar.event_type as i32, radar.rx, radar.ry
                )),
                _ => None,
            })
            .collect();
        assert_eq!(pushed, lines, "{row}");

        // Each call: the object and the cell whose list held it.
        let mut looked = (0, 0);
        let mut native = Vec::new();
        for event in row["events"].as_array().unwrap() {
            match event[0].as_str().unwrap() {
                "cell" => looked = (int(&event[1][0]) as i16, int(&event[1][1]) as i16),
                "curtain" => {
                    // IronCurtainDuration=, the Super's house, the Iron
                    // Curtain's byte.
                    assert_eq!(int(&event[2]), int(&row["duration"]));
                    assert_eq!((event[3].as_bool(), int(&event[4])), (Some(true), 0));
                    if let Some(id) = ids[event[1].as_u64().unwrap() as usize] {
                        native.push(Observed::Curtain(looked, id));
                    }
                }
                _ => {}
            }
        }
        assert_eq!(observed, native, "{row}");
        let duration = int(&row["duration"]);
        for (index, id) in ids.iter().enumerate() {
            let Some(id) = *id else { continue };
            let called = native
                .iter()
                .any(|call| matches!(call, Observed::Curtain(_, called) if *called == id));
            assert_eq!(
                curtain(&sim, id),
                called.then_some((frame, duration, InvulnKind::IronCurtain)),
                "{row}: object {index}"
            );
            if objects[index]["foot"] == true {
                // A skipped Foot (the warp latch, or no launch) keeps its timer.
                let expected = if called {
                    Some((frame, 0))
                } else {
                    paralysis_before[index]
                };
                assert_eq!(Some(paralysis(&sim, id)), expected, "{row}: object {index}");
            }
        }
        compared += 1;
    }
    assert_eq!(compared, 16);
}

/// InfantryClass::IronCurtain and FootClass::IronCurtain against
/// `curtain_overrides`: each row's object alone at the target cell, with
/// the row's Strength, Organic flag and frame, through the production
/// launch. An eaten object holds a Terror Drone, which VERA attaches to
/// infantry and Organic types too. Compared:
/// - each ReceiveDamage call's arguments;
/// - the forced release of the drone (the ExitUnit call) and its
///   suppression timer;
/// - the paralysis timer and the curtain, and that a row with no Techno
///   call touches neither.
///
/// ReceiveDamage's own effects belong to the receiver, which the oracle
/// stubs, so a row with a damage call compares only the call. The oracle
/// stubs ExitUnit too: the drone's death through its running-suppression
/// arm is checked against VERA's ExitUnit only. The override's return value
/// is not compared: the walk drops it.
#[test]
fn the_overrides_match_native() {
    let oracle = oracle();
    for row in rows(&oracle, "curtain_overrides") {
        let infantry = row["kind"] == "infantry";
        let organic = row["organic"] == true;
        let kind = match (infantry, organic) {
            (true, _) => "E1",
            (false, true) => "DLPH",
            (false, false) => "MTNK",
        };
        let mut rules = rules(&format!(
            "[{kind}]\nStrength={}\nOrganic={}\n",
            int(&row["strength"]),
            if organic { "yes" } else { "no" }
        ));
        rules.general.iron_curtain_duration = int(&row["duration"]);
        let (rules, mut sim, owner) = world_with(rules, 64, &[]);
        let frame = int(&row["frame"]);
        sim.session.set_binary_frame_for_test(frame as u32);
        let victim = sim
            .spawn_object_at_height(kind, "Americans", 40, 40, 0, 0, &rules)
            .unwrap_or_else(|| panic!("{kind} stands for {row}"));
        let drone = (row["eaten"] == true).then(|| {
            let drone = sim
                .construct_object_limbo_at_height(
                    "DRON",
                    "Russians",
                    39,
                    40,
                    0,
                    0,
                    &rules,
                    crate::sim::world::FrameEffects::default(),
                )
                .unwrap();
            sim.parasite_attach(
                drone,
                Some(victim),
                &rules,
                crate::sim::world::FrameEffects::default(),
            );
            assert_eq!(
                sim.substrate.entities.get(victim).unwrap().parasite.eater(),
                Some(drone)
            );
            drone
        });
        let sw_type = charge_super(&mut sim, owner, IRON_CURTAIN);
        let paralysis_before = paralysis(&sim, victim);
        let (launched, observed) = observed_launch(&mut sim, &rules, owner, (40, 40), sw_type);
        assert!(launched);

        let c4 = sim.interner.intern("Super");
        let mut expected = vec![Observed::Curtain((40, 40), victim)];
        for event in row["events"].as_array().unwrap() {
            match event[0].as_str().unwrap() {
                "receive_damage" => {
                    // The damage, the distance, C4Warhead=, no attacker,
                    // ignoreDefenses, arg6 and the house.
                    assert_eq!((&event[3], int(&event[4])), (&Value::Bool(true), 0));
                    expected.push(Observed::ReceiveDamage(EntityDamageEvent::direct_receiver(
                        victim,
                        int(&event[1]),
                        int(&event[2]),
                        RAD_NO_ATTACKER,
                        (event[7] == true).then_some(owner),
                        c4,
                        ReceiverCallFlags {
                            ignore_defenses: int(&event[5]) != 0,
                            arg6: int(&event[6]) != 0,
                        },
                    )));
                }
                "exit_unit" => expected.push(Observed::ExitUnit(drone.unwrap())),
                _ => {}
            }
        }
        assert_eq!(observed, expected, "{row}");

        match events(row, "techno_curtain").next() {
            Some(call) => {
                // The duration, the Super's house and the Iron Curtain's byte.
                assert_eq!((&call[3], int(&call[4])), (&Value::Bool(true), 0));
                assert_eq!(
                    curtain(&sim, victim),
                    Some((frame, int(&call[2]), InvulnKind::IronCurtain)),
                    "{row}"
                );
                let timer = &row["foot_timer"];
                assert_eq!(
                    paralysis(&sim, victim),
                    (int(&timer[0]), int(&timer[1])),
                    "{row}"
                );
            }
            None => {
                assert_eq!(curtain(&sim, victim), None, "{row}");
                assert_eq!(row["foot_timer"], serde_json::json!([-7, -7]), "{row}");
                // The killed victim stays in the store for its death.
                assert_eq!(paralysis(&sim, victim), paralysis_before, "{row}");
            }
        }
        if let Some(drone) = drone.filter(|_| events(row, "exit_unit").next().is_some()) {
            let entity = sim.substrate.entities.get(drone).unwrap();
            let suppression = entity.parasite.class().unwrap().suppression();
            let timer = &row["parasite_timer"];
            assert_eq!(
                (suppression.start_frame(), suppression.duration()),
                (int(&timer[0]), int(&timer[1])),
                "{row}"
            );
            assert!(!entity.lifecycle.object_alive(), "{row}");
            assert_eq!(
                sim.substrate.entities.get(victim).unwrap().parasite.eater(),
                None
            );
        }
    }
}

/// `SuperClass::Launch 0x006CCF09`: the invoke animation is a real
/// `AnimClass`, so its art `Report=` plays from `AnimClass::Start` (retail
/// `[IRONBLST] Report=IronCurtainBlast`, which no separate launch cue
/// carries).
#[test]
fn the_invoke_anim_plays_its_report() {
    let (rules, mut sim, owner) = world_with(rules(""), 64, &[]);
    let sw_type = charge_super(&mut sim, owner, IRON_CURTAIN);
    assert!(launch(
        &mut sim,
        &rules,
        owner,
        10,
        10,
        sw_type,
        None,
        crate::sim::world::FrameEffects::default()
    ));
    assert!(sim.sound_events.iter().any(|event| matches!(
        event,
        SimSoundEvent::AnimationStarted { sound_id, .. }
            if sound_id.eq_ignore_ascii_case("IronCurtainBlast")
    )));
}

/// The curtained tank's tint stage steps in its own Techno AI
/// (`0x006F9EAF`) and draws from the Scenario stream once, at its stage
/// 2 step: the AI visit ten frames after its first under the curtain.
/// The same idle tank uncurtained draws in the same other visits.
#[test]
fn a_curtained_tank_draws_its_tint_number_in_its_own_ai() {
    let drawing_visits = |curtain: bool| {
        let (rules, mut sim, owner) = world_with(rules(""), 64, &[]);
        let tank = sim
            .spawn_object_at_height("MTNK", "Americans", 10, 10, 0, 0, &rules)
            .unwrap();
        if curtain {
            let sw_type = charge_super(&mut sim, owner, IRON_CURTAIN);
            assert!(launch(
                &mut sim,
                &rules,
                owner,
                10,
                10,
                sw_type,
                None,
                crate::sim::world::FrameEffects::default()
            ));
        }
        let mut drawn = Vec::new();
        for visit in 1..=40 {
            let ((), draws) = crate::sim::rng::trace_draws(|| {
                sim.advance_tick(&[], Some(&rules), None, None, 67);
            });
            if draws.iter().any(|draw| draw["logic_object"] == tank) {
                drawn.push(visit);
            }
        }
        drawn
    };
    let mut expected = drawing_visits(false);
    assert!(!expected.contains(&11));
    expected.push(11);
    expected.sort();
    assert_eq!(drawing_visits(true), expected);
}

/// `InfantryClass::IronCurtain @ 0x00522632` kills through `ReceiveDamage`
/// (`+0x16C`, `C4Warhead=`), so the death reaches `Death_Announcement`
/// (`+0x3B8`): each human-owned kill publishes the radar type-7 request
/// (`0x004D98FE`) whose client-side 8-cell dedupe limits "Unit lost". The
/// tank beside them is curtained.
///
/// The launch's line (`0x006CCF21`) and radar event (`0x006CCF2F`) come
/// before the walk (`0x006CCF39`), so before its kills' "Unit lost" lines:
/// the critical `EVA_IronCurtainActivated` reaches VoxClass first.
#[test]
fn curtained_infantry_die_and_announce_each_loss() {
    let (rules, mut sim, owner) = world_with(rules(""), 64, &[]);
    let spawn = |sim: &mut Simulation, kind: &str, at: u16| {
        sim.spawn_object_at_height(kind, "Americans", at, at, 0, 0, &rules)
            .unwrap()
    };
    let first = spawn(&mut sim, "E1", 10);
    let second = spawn(&mut sim, "E1", 11);
    let tank = spawn(&mut sim, "MTNK", 9);
    let sw_type = charge_super(&mut sim, owner, IRON_CURTAIN);
    sim.sound_events.clear_for_test();
    assert!(launch(
        &mut sim,
        &rules,
        owner,
        10,
        10,
        sw_type,
        None,
        crate::sim::world::FrameEffects::default()
    ));

    let lost: Vec<InternedId> = sim
        .sound_events
        .iter()
        .filter_map(|event| match event {
            SimSoundEvent::UnitLost { owner, .. } => Some(*owner),
            _ => None,
        })
        .collect();
    assert_eq!(lost, vec![owner, owner]);
    let position = |wanted: fn(&SimSoundEvent) -> bool| {
        sim.sound_events
            .iter()
            .position(wanted)
            .expect("the event was pushed")
    };
    let launched = position(|event| matches!(event, SimSoundEvent::SuperWeaponLaunched { .. }));
    let radar = position(|event| matches!(event, SimSoundEvent::RadarEvent { .. }));
    let first_loss = position(|event| matches!(event, SimSoundEvent::UnitLost { .. }));
    assert!(launched < radar && radar < first_loss);
    for id in [first, second] {
        let infantry = sim.substrate.entities.get(id).unwrap();
        assert!(infantry.lifecycle.object_alive() && infantry.health.current() == 0);
        assert!(infantry.invulnerability.is_none());
    }
    assert!(!sim.substrate.entities.get(tank).unwrap().death.is_dying());
    assert!(curtain(&sim, tank).is_some());
}

/// Phobos `IronCurtain.EffectOnOrganics`: the retail default kills an
/// infantryman and an Organic Foot with the curtain's kill warhead;
/// `invulnerable` curtains both; `ignore` leaves both untouched.
#[test]
fn iron_curtain_effect_on_organics_selects_kill_invulnerable_or_ignore() {
    for (effect, behaviour) in [
        ("", "kill"),
        ("invulnerable", "invulnerable"),
        ("ignore", "ignore"),
    ] {
        let extra = if effect.is_empty() {
            String::new()
        } else {
            format!("[CombatDamage]\nIronCurtain.EffectOnOrganics={effect}\n")
        };
        let (rules, mut sim, owner) = world_with(rules(&extra), 64, &[]);
        let spawn = |sim: &mut Simulation, kind: &str, x: u16| {
            sim.spawn_object_at_height(kind, "Americans", x, 10, 0, 0, &rules)
                .unwrap()
        };
        let infantry = spawn(&mut sim, "E1", 10);
        let dolphin = spawn(&mut sim, "DLPH", 11);
        let sw_type = charge_super(&mut sim, owner, IRON_CURTAIN);
        assert!(launch(
            &mut sim,
            &rules,
            owner,
            10,
            10,
            sw_type,
            None,
            crate::sim::world::FrameEffects::default()
        ));
        for id in [infantry, dolphin] {
            let entity = sim.substrate.entities.get(id).unwrap();
            match behaviour {
                "kill" => {
                    assert_eq!(entity.health.current(), 0, "{effect}: killed");
                    assert!(entity.invulnerability.is_none(), "{effect}: not curtained");
                }
                "invulnerable" => {
                    assert!(entity.health.current() > 0, "{effect}: survived");
                    assert!(entity.invulnerability.is_some(), "{effect}: curtained");
                }
                _ => {
                    assert!(entity.health.current() > 0, "ignore: survived");
                    assert!(entity.invulnerability.is_none(), "ignore: untouched");
                }
            }
        }
    }
}

/// Phobos `TechnoTypeExt`: `IronCurtain.Effect` overrides the global per
/// type, and `IronCurtain.KillWarhead`/`KillOrganicsWarhead` choose the
/// killing warhead (the type's wins).
#[test]
fn type_effect_and_kill_warheads_override_the_globals() {
    let extra = "[CombatDamage]\nIronCurtain.KillOrganicsWarhead=GlobalWH\n\
                 [Warheads]\n2=GlobalWH\n3=TypeWH\n\
                 [GlobalWH]\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n\
                 [TypeWH]\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n\
                 [DLPH]\nIronCurtain.KillWarhead=TypeWH\n";
    let (rules, mut sim, owner) = world_with(rules(extra), 64, &[]);
    let infantry = sim
        .spawn_object_at_height("E1", "Americans", 10, 10, 0, 0, &rules)
        .unwrap();
    let dolphin = sim
        .spawn_object_at_height("DLPH", "Americans", 11, 10, 0, 0, &rules)
        .unwrap();
    let sw_type = charge_super(&mut sim, owner, IRON_CURTAIN);
    let (_, observed) = observed_launch(&mut sim, &rules, owner, (10, 10), sw_type);
    let warheads: Vec<String> = observed
        .iter()
        .filter_map(|call| match call {
            Observed::ReceiveDamage(event) => {
                Some(sim.interner.resolve(event.warhead_ref).to_owned())
            }
            _ => None,
        })
        .collect();
    assert_eq!(warheads.len(), 2, "both organics are killed");
    assert!(warheads.iter().any(|name| name == "GlobalWH"));
    assert!(warheads.iter().any(|name| name == "TypeWH"));
    for id in [infantry, dolphin] {
        assert_eq!(sim.substrate.entities.get(id).unwrap().health.current(), 0);
    }

    // The type's Invulnerable effect suppresses its kill entirely.
    let invulnerable_rules = self::rules("[E1]\nIronCurtain.Effect=invulnerable\n");
    let (invulnerable_rules, mut sim, owner) = world_with(invulnerable_rules, 64, &[]);
    let infantry = sim
        .spawn_object_at_height("E1", "Americans", 10, 10, 0, 0, &invulnerable_rules)
        .unwrap();
    let sw_type = charge_super(&mut sim, owner, IRON_CURTAIN);
    let (_, observed) = observed_launch(&mut sim, &invulnerable_rules, owner, (10, 10), sw_type);
    assert!(
        !observed
            .iter()
            .any(|call| matches!(call, Observed::ReceiveDamage(_))),
        "the invulnerable type takes no kill"
    );
    let entity = sim.substrate.entities.get(infantry).unwrap();
    assert!(entity.health.current() > 0 && entity.invulnerability.is_some());
}

/// Retail rules through the production reader and frames:
/// `IronCurtainDuration=750`, `IronCurtainInvokeAnim=IRONBLST` (its `;`
/// comment dropped) and `C4Warhead=Super`. A click launches on the next
/// frame: the line, then radar event 13 at the cell; the Rhino curtained for
/// 750 frames; the conscript beside it killed by its Strength.
#[test]
fn retail_iron_curtain_through_production_frames() {
    let Some(rules) = retail_rules_binding(&[]) else {
        return;
    };
    assert_eq!(rules.general.iron_curtain_duration, 750);
    assert_eq!(rules.general.iron_curtain_invoke_anim, "IRONBLST");
    assert_eq!(rules.bridge_warheads.c4_name, "Super");
    let (rules, mut sim, _) = world_with(rules, 64, &[]);
    let russians = sim.interner.intern("Russians");
    // The Russians click as a human player: the DoList runs only human or
    // PlayerControl Houses' events (0x0064C4FB..0x0064C50D), and SPECIAL_PLACE
    // fires the sender House's own Super (0x004C78D6).
    sim.houses
        .get_mut(&russians)
        .unwrap()
        .set_is_human_for_test(true);
    let rhino = sim
        .spawn_object_at_height("HTNK", "Russians", 40, 40, 0, 0, &rules)
        .unwrap();
    let conscript = sim
        .spawn_object_at_height("E2", "Russians", 41, 40, 0, 0, &rules)
        .unwrap();
    let sw_type = charge_super(&mut sim, russians, IRON_CURTAIN);
    let launch_frame = sim.session.binary_frame() as i32;
    click(&mut sim, &rules, russians, IRON_CURTAIN, (40, 40));

    let launch: Vec<&SimSoundEvent> = sim
        .sound_events
        .iter()
        .filter(|event| {
            matches!(
                event,
                SimSoundEvent::SuperWeaponLaunched { .. } | SimSoundEvent::RadarEvent { .. }
            )
        })
        .collect();
    assert!(matches!(
        launch.as_slice(),
        [
            SimSoundEvent::SuperWeaponLaunched { owner, sw_type: kind, world },
            SimSoundEvent::RadarEvent { radar },
        ] if *owner == russians
            && *kind == sw_type
            && (world.to_cell_sub_z().0, world.to_cell_sub_z().1) == (40, 40)
            && radar.event_type == crate::sim::radar::RadarEventType::ImpactSilent
            && (radar.rx, radar.ry) == (40, 40)
    ));
    assert_eq!(
        curtain(&sim, rhino),
        Some((launch_frame, 750, InvulnKind::IronCurtain))
    );
    let conscript = sim.substrate.entities.get(conscript).unwrap();
    assert!(conscript.lifecycle.object_alive() && conscript.health.current() == 0);
    assert!(!conscript.death.is_dying());
}
