//! Crash, the Fly fall and its impact, and the crash smoke against the
//! original executable (`tools/spatial_oracle/aircraft_crash.json`).

use super::lifecycle_tests::{insert_entity, install_common_raw_terrain};
use super::{PlacementEvidence, RevealOutcome, RevealPosition, RevealRequest, Simulation};
use crate::map::entities::EntityCategory;
use crate::rules::{ini_parser::IniFile, ruleset::RuleSet};
use crate::sim::components::DriveCoord;
use crate::sim::movement::FacingClass;
use crate::sim::movement::locomotor::LocomotorState;
use crate::sim::rng::SimRng;
use crate::util::fixed_math::SimFixed;

/// The oracle's aircraft stands at the centre of cell (52, 52).
const START: i32 = 52 * 256 + 128;

fn oracle() -> serde_json::Value {
    serde_json::from_str(include_str!(
        "../../../tools/spatial_oracle/aircraft_crash.json"
    ))
    .unwrap()
}

fn int(input: &serde_json::Value, name: &str, default: i64) -> i64 {
    input[name].as_i64().unwrap_or(default)
}

/// The oracle's sound indices, as named sounds: the type's land/water cues
/// (12, 33) and the `[AudioVisual]` fallbacks (71, 70).
fn sound_name(index: i64) -> &'static str {
    match index {
        12 => "TypeLand",
        33 => "TypeWater",
        70 => "RulesWater",
        71 => "RulesLand",
        other => panic!("unmapped oracle sound index {other}"),
    }
}

fn rules_for(input: &serde_json::Value) -> RuleSet {
    let mut sounds = String::new();
    let land = int(input, "land_sound", 12);
    if land != -1 {
        sounds += &format!("ImpactLandSound={}\n", sound_name(land));
    }
    let water = int(input, "water_sound", -1);
    if water != -1 {
        sounds += &format!("ImpactWaterSound={}\n", sound_name(water));
    }
    let mut rules = RuleSet::from_ini(&IniFile::from_str(&format!(
        "[General]\nFlightLevel=1500\nConditionRed=0.25\n\
         [AudioVisual]\nImpactLandSound=RulesLand\nImpactWaterSound=RulesWater\n\
         [AircraftTypes]\n0=TEST\n[VehicleTypes]\n0=VICTIM\n\
         [TEST]\nStrength={}\nSpeed={}\nLandable=yes\nPrimary=CrashGun\n\
         Locomotor={{4A582746-9839-11D1-B709-00A024DDAFD1}}\n{sounds}\
         [VICTIM]\nStrength=1000\nArmor=none\n\
         [CrashGun]\nDamage=150\nWarhead=CrashWH\n\
         [CrashWH]\nCellSpread=1\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n",
        int(input, "strength", 150),
        int(input, "ini_speed", 14),
    )))
    .unwrap();
    let mut art =
        crate::rules::art_data::ArtRegistry::from_ini(&IniFile::from_str("[SGRYSMK1]\nRate=100\n"));
    art.bind_anim_frame_count_for_test("SGRYSMK1", 20);
    rules.art_registry = art;
    rules
}

/// One aircraft in the oracle's airborne state over a flat 70x70 map whose
/// MapSize is 64x64 (the oracle's `In_Bounds` diamond).
fn fixture(input: &serde_json::Value) -> (Simulation, RuleSet) {
    let rules = rules_for(input);
    let mut sim = Simulation::with_seed(0);
    let level = int(input, "level", 0) as u8;
    install_common_raw_terrain(&mut sim, 70, 70, level, None);
    {
        let terrain = sim.resolved_terrain.as_mut().unwrap();
        for ry in 44..68 {
            for rx in 44..68 {
                let cell = terrain.cell_mut(rx, ry).unwrap();
                cell.yr_cell_land_type = int(input, "land_type", 0) as u8;
                if input["bridge"].as_bool() == Some(true) {
                    cell.bridge_facts.raw_flags |= 0x100;
                }
            }
        }
    }
    sim.playfield_bounds = Some(crate::sim::cell_rect::PlayfieldBounds {
        base: 64,
        off_fc: 0,
        off_100: 0,
        off_104: 64,
        off_108: 64,
    });
    sim.playfield_size_height = Some(64);
    sim.session.binary_frame = int(input, "frame", 1000) as u32;
    sim.scenario_rng = SimRng::new(int(input, "seed", 1) as u64);
    assert_eq!(sim.allocate_stable_id(), 1);
    insert_entity(&mut sim, 1, EntityCategory::Aircraft);
    sim.substrate.entities.get_mut(1).unwrap().locomotor = Some(LocomotorState::from_object_type(
        rules.object("TEST").unwrap(),
        0,
    ));
    assert!(matches!(
        sim.try_reveal_entity(
            1,
            RevealRequest {
                position: RevealPosition {
                    rx: 52,
                    ry: 52,
                    z: level,
                    sub_x: SimFixed::from_num(128),
                    sub_y: SimFixed::from_num(128)
                },
                placement: PlacementEvidence::MarkSucceeded,
                logic_eligible: true,
            }
        ),
        RevealOutcome::Revealed { .. }
    ));
    sim.remove_entity_occupancy(1);
    let xyz = input["xyz"].as_array().map_or([START, START, 1500], |a| {
        [0, 1, 2].map(|i| a[i].as_i64().unwrap() as i32)
    });
    assert_eq!((xyz[0], xyz[1]), (START, START));
    let frame = sim.session.binary_frame;
    let entity = sim.substrate.entities.get_mut(1).unwrap();
    entity.position.exact_z_leptons = Some(xyz[2]);
    entity.health.current = int(input, "health", 0) as i32;
    entity.crashing = int(input, "crashing", 1) != 0;
    let mut facing = FacingClass::new(
        int(input, "facing", 0x4000) as u16,
        int(input, "rot", 5) as i32,
    );
    if let Some(turn_to) = input["turn_to"].as_i64() {
        facing.set(turn_to as u16, frame - int(input, "turn_age", 0) as u32);
    }
    entity.body_facing = Some(facing);
    let loco = entity.locomotor.as_mut().unwrap();
    loco.set_fly_target_height(int(input, "target_height", 1500) as i32);
    loco.fly_current_speed = SimFixed::from_bits(int(input, "speed_bits", 65536) as i32);
    if int(input, "moving", 1) != 0 {
        let destination = input["destination"].as_array().map_or(
            DriveCoord {
                x: 64 * 256,
                y: 52 * 256,
                z: 1500,
            },
            |a| DriveCoord {
                x: a[0].as_i64().unwrap() as i32,
                y: a[1].as_i64().unwrap() as i32,
                z: a[2].as_i64().unwrap() as i32,
            },
        );
        loco.fly_runtime_mut()
            .unwrap()
            .retain_destination(destination, None, || 0);
    }
    sim.add_entity_occupancy(1);
    (sim, rules)
}

fn f32_hex(value: &serde_json::Value) -> f64 {
    f64::from(f32::from_bits(
        u32::from_str_radix(value.as_str().unwrap(), 16).unwrap(),
    ))
}

/// `FootClass::Crash @ 0x004DEBB0`: the ground refusal, the live prefix's
/// Health 0, the latch and the three Scenario draws with their f32 rates and
/// the stream's continuation, over ten seeds.
#[test]
fn crash_matches_native_rows() {
    let rows = oracle()["crash"].as_array().unwrap().clone();
    assert_eq!(rows.len(), 15);
    let mut compared = 0;
    for row in &rows {
        let input = &row["input"];
        if int(input, "i_know", 0) != 0 {
            // `Unsorted::IKnowWhatImDoing` is raised only around building
            // placement scopes that never reach a crash; VERA reads it as 0.
            assert!(row["draws"].as_array().unwrap().is_empty());
            continue;
        }
        let mut fixture_input = input.clone();
        fixture_input["crashing"] = serde_json::json!(0);
        let (mut sim, rules) = fixture(&fixture_input);
        let returned = sim.foot_crash(1, None, &rules);
        let name = input["name"].as_str().unwrap();
        assert_eq!(returned, row["returned"].as_bool().unwrap(), "{name}");
        let entity = sim.substrate.entities.get(1).unwrap();
        assert_eq!(
            entity.health.current,
            row["health"].as_i64().unwrap() as i32,
            "{name}"
        );
        // The oracle's fixture pre-sets the latch only for rows that call
        // Crash on an already-crashing object; a refused Crash writes nothing.
        assert_eq!(entity.crashing, returned, "{name}");
        let draws = row["draws"].as_array().unwrap().len();
        if draws == 0 {
            assert!(entity.rocking.is_none(), "{name}");
        } else {
            let rocking = entity.rocking.as_ref().unwrap();
            for (actual, expected) in [
                (rocking.vel_sideways, &row["sideways_bits"]),
                (rocking.vel_forwards, &row["forwards_bits"]),
            ] {
                assert!(
                    (actual.to_num::<f64>() - f32_hex(expected)).abs() <= 1.0 / 65536.0,
                    "{name}: {actual} vs {}",
                    f32_hex(expected)
                );
            }
        }
        // Count and stream: the next raw draw continues where native's did.
        assert_eq!(
            sim.scenario_rng.next_u32(),
            row["next_random"].as_u64().unwrap() as u32,
            "{name}"
        );
        compared += 1;
    }
    assert_eq!(compared, 14);
}

/// The whole fall of a dead crashing aircraft, frame by frame, through the
/// production Fly transaction, to the impact frame: the XYZ after every frame,
/// then the death weapon's blast at the impact point (a 1000-strength victim
/// takes the 150-damage `Primary=`), the impact cue by LandType with its
/// `[AudioVisual]` fallback, and the UnInit.
#[test]
fn crash_fall_matches_native_frames_to_the_impact() {
    let rows = oracle()["fall"].as_array().unwrap().clone();
    assert_eq!(rows.len(), 15);
    for row in &rows {
        let input = &row["input"];
        let name = input["name"].as_str().unwrap();
        let (mut sim, rules) = fixture(input);
        let frames = row["frames"].as_array().unwrap();
        let start = sim.session.binary_frame;
        for (n, frame) in frames.iter().enumerate() {
            sim.session.binary_frame = start + n as u32;
            sim.sound_events.clear();
            let impact = n + 1 == frames.len();
            let expected: Vec<i32> = frame["xyz"]
                .as_array()
                .unwrap()
                .iter()
                .map(|v| v.as_i64().unwrap() as i32)
                .collect();
            if impact {
                // A victim under the impact point takes the death weapon.
                let victim = sim.allocate_stable_id();
                let owner = sim.interner.intern("Soviets");
                let type_ref = sim.interner.intern("VICTIM");
                let mut entity = crate::sim::game_entity::GameEntity::new_at_frame_zero_for_test(
                    victim,
                    (expected[0] / 256) as u16,
                    (expected[1] / 256) as u16,
                    int(input, "level", 0) as u8,
                    0,
                    owner,
                    crate::sim::components::Health { current: 1000 },
                    type_ref,
                    EntityCategory::Unit,
                    0,
                    5,
                    true,
                );
                entity.lifecycle.in_limbo = true;
                sim.substrate.entities.insert(entity);
                assert!(matches!(
                    sim.try_reveal_entity(
                        victim,
                        RevealRequest {
                            position: RevealPosition {
                                rx: (expected[0] / 256) as u16,
                                ry: (expected[1] / 256) as u16,
                                z: int(input, "level", 0) as u8,
                                sub_x: SimFixed::from_num(expected[0] % 256),
                                sub_y: SimFixed::from_num(expected[1] % 256),
                            },
                            placement: PlacementEvidence::MarkSucceeded,
                            logic_eligible: true,
                        }
                    ),
                    RevealOutcome::Revealed { .. }
                ));
                let stats = sim.tick_air_movement_with_cell_lists_one(1, Some(&rules));
                assert!(stats.impact, "{name}: impact frame {n}");
                let entity = sim.substrate.entities.get(1).unwrap();
                let xy = crate::sim::movement::ground_pose::position_world_xy(&entity.position);
                assert_eq!(
                    [xy[0], xy[1]],
                    [expected[0], expected[1]],
                    "{name} impact xy"
                );
                sim.fly_crash_impact(1, &rules, None);
                assert!(
                    sim.substrate
                        .entities
                        .get(1)
                        .is_none_or(|e| !e.lifecycle.object_alive),
                    "{name}: UnInit at the impact"
                );
                assert_eq!(
                    sim.substrate.entities.get(victim).unwrap().health.current,
                    1000 - 150,
                    "{name}: the death weapon detonates at the impact point"
                );
                let calls = frame["calls"].as_array().unwrap();
                let play = calls.iter().find(|c| c["call"] == "play_at").unwrap();
                let expected_sound = sound_name(play["sound"].as_i64().unwrap());
                let played: Vec<&str> = sim
                    .sound_events
                    .iter()
                    .filter_map(|event| match event {
                        super::SimSoundEvent::VocAt { sound_id, .. } => Some(sound_id.as_str()),
                        _ => None,
                    })
                    .collect();
                assert_eq!(played, [expected_sound], "{name}");
                assert!(calls.iter().any(|c| c["call"] == "fire_death_weapon"));
                assert!(calls.iter().any(|c| c["call"] == "uninit"));
            } else {
                let stats = sim.tick_air_movement_with_cell_lists_one(1, Some(&rules));
                assert!(!stats.impact, "{name}: early impact at frame {n}");
                let entity = sim.substrate.entities.get(1).unwrap();
                let xy = crate::sim::movement::ground_pose::position_world_xy(&entity.position);
                assert_eq!(
                    [xy[0], xy[1], entity.position.exact_z_leptons.unwrap()],
                    [expected[0], expected[1], expected[2]],
                    "{name} frame {n}"
                );
                let counter = entity
                    .locomotor
                    .as_ref()
                    .unwrap()
                    .fly_runtime()
                    .unwrap()
                    .fall_counter();
                assert_eq!(
                    counter,
                    frame["counter"].as_i64().unwrap() as i32,
                    "{name} frame {n}"
                );
            }
        }
    }
}

/// `AircraftClass::AI`'s smoke: strict red health (and no smoke at exactly
/// ConditionRed or on the ground), one Scenario `RandomRanged(0, 99)`, and the
/// 10/80 threshold, over three seeds.
#[test]
fn crash_smoke_matches_native_rows() {
    let rows = oracle()["smoke"].as_array().unwrap().clone();
    assert_eq!(rows.len(), 17);
    for row in &rows {
        let input = &row["input"];
        let name = input["name"].as_str().unwrap();
        let (mut sim, rules) = fixture(input);
        let anims_before = sim.substrate.anims.len();
        sim.aircraft_crash_smoke(1, &rules);
        let smoked = sim.substrate.anims.len() > anims_before;
        assert_eq!(smoked, row["smoke"].as_bool().unwrap(), "{name}");
        assert_eq!(
            sim.scenario_rng.next_u32(),
            row["next_random"].as_u64().unwrap() as u32,
            "{name}"
        );
    }
}
