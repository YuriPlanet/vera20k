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

/// A lethal hit on a flying aircraft through the production receiver, then
/// whole frames through `advance_tick`: the aircraft stays alive at Health 0,
/// latched and spinning, trails smoke and plays its crash sound while it
/// falls, and at the impact its death weapon strikes the victim below before
/// it is UnInit. The killer's house holds the kill from the hit on.
#[test]
fn a_shot_down_aircraft_falls_and_detonates_through_advance_tick() {
    use crate::sim::combat::combat_aoe::AreaDamageReceiver;
    use std::collections::BTreeMap;
    let input = serde_json::json!({"health": 150, "crashing": 0});
    let (mut sim, rules) = fixture(&input);
    let soviets = sim.interner.intern("Soviets");
    let shooter = sim.allocate_stable_id();
    let shooter_type = sim.interner.intern("VICTIM");
    let mut entity = crate::sim::game_entity::GameEntity::new_at_frame_zero_for_test(
        shooter,
        40,
        40,
        0,
        0,
        soviets,
        crate::sim::components::Health { current: 1000 },
        shooter_type,
        EntityCategory::Unit,
        0,
        5,
        true,
    );
    entity.lifecycle.in_limbo = true;
    sim.substrate.entities.insert(entity);
    assert!(matches!(
        sim.try_reveal_entity(
            shooter,
            RevealRequest {
                position: RevealPosition {
                    rx: 40,
                    ry: 40,
                    z: 0,
                    sub_x: SimFixed::from_num(128),
                    sub_y: SimFixed::from_num(128),
                },
                placement: PlacementEvidence::MarkSucceeded,
                logic_eligible: true,
            }
        ),
        RevealOutcome::Revealed { .. }
    ));
    let warhead = sim.interner.intern("CrashWH");
    let hit =
        crate::sim::combat::EntityDamageEvent::area(1, 200, 0, shooter, Some(soviets), warhead);
    sim.commit_noncombat_aoe_receivers(&rules, None, &[AreaDamageReceiver::Entity(hit)]);
    let entity = sim
        .substrate
        .entities
        .get(1)
        .expect("a crashing aircraft stays represented");
    assert!(entity.crashing && entity.lifecycle.object_alive && !entity.dying);
    assert_eq!(entity.health.current, 0);
    assert_eq!(entity.killed_by, Some(soviets));
    assert!(
        entity
            .rocking
            .as_ref()
            .is_some_and(|r| r.vel_sideways != SimFixed::ZERO)
    );

    let grid = crate::sim::pathfinding::PathGrid::test_all_passable(70, 70);
    let mut smoke = 0;
    let mut crash_sound = false;
    let mut frames = 0;
    let mut last_xyz = None;
    while sim
        .substrate
        .entities
        .get(1)
        .is_some_and(|e| e.lifecycle.object_alive)
    {
        frames += 1;
        assert!(frames < 60, "the fall must reach the ground");
        let anims_before = sim.substrate.anims.len();
        sim.sound_events.clear();
        sim.advance_tick(&[], Some(&rules), &BTreeMap::new(), Some(&grid), None, 67);
        smoke += sim.substrate.anims.len().saturating_sub(anims_before);
        crash_sound |= sim.sound_events.iter().any(|event| {
            matches!(
                event,
                super::SimSoundEvent::AnimationStarted { anim_id: 1, .. }
            )
        });
        if let Some(entity) = sim.substrate.entities.get(1) {
            last_xyz = Some(crate::sim::movement::ground_pose::position_world_coord(
                &entity.position,
            ));
        }
    }
    let last = last_xyz.unwrap();
    assert!(
        frames > 30,
        "a 1500-lepton fall takes ~39 frames, took {frames}"
    );
    assert!(smoke > 0, "a dead airborne aircraft trails SGRYSMK1");
    // The type names no CrashingSound: the test only proves the edge ran once
    // with nothing to play.
    assert!(!crash_sound);
    assert!(last.z >= 0);
    assert!(
        sim.sound_events.iter().any(|event| matches!(
            event,
            super::SimSoundEvent::VocAt { sound_id, .. } if sound_id == "TypeLand"
        )),
        "the impact cue"
    );
    assert!(sim.substrate.entities.get(shooter).is_some());
}

/// A save in mid-fall restores the latch, its seen edge, the fall counter and
/// the spin: the loaded and the continuing worlds fall frame for frame to the
/// same impact.
#[test]
fn a_crash_saved_in_mid_fall_lands_like_the_original() {
    use crate::sim::combat::combat_aoe::AreaDamageReceiver;
    use crate::sim::snapshot::GameSnapshot;
    use std::collections::BTreeMap;
    let (mut sim, rules) = fixture(&serde_json::json!({"health": 150, "crashing": 0}));
    let soviets = sim.interner.intern("Soviets");
    let warhead = sim.interner.intern("CrashWH");
    let hit = crate::sim::combat::EntityDamageEvent::area(
        1,
        200,
        0,
        crate::sim::combat::RAD_NO_ATTACKER,
        Some(soviets),
        warhead,
    );
    sim.commit_noncombat_aoe_receivers(&rules, None, &[AreaDamageReceiver::Entity(hit)]);
    assert!(sim.substrate.entities.get(1).unwrap().crashing);
    let grid = crate::sim::pathfinding::PathGrid::test_all_passable(70, 70);
    let tick = |sim: &mut Simulation| {
        sim.advance_tick(&[], Some(&rules), &BTreeMap::new(), Some(&grid), None, 67);
    };
    for _ in 0..10 {
        tick(&mut sim);
    }
    assert!(sim.substrate.entities.get(1).unwrap().crashing_seen);

    sim.scenario_rng = SimRng::new(0);
    let bytes = GameSnapshot::save(&sim, 0, 0, "crash in mid-fall", 0);
    let mut restored = GameSnapshot::load(&bytes).unwrap().sim;
    restored.retain_in_scenario_process_state_from(&sim);
    restored.resolved_terrain = sim.resolved_terrain.clone();
    restored.restore_after_snapshot_load().unwrap();
    assert_eq!(restored.state_hash(), sim.state_hash());

    let alive = |sim: &Simulation| {
        sim.substrate
            .entities
            .get(1)
            .is_some_and(|entity| entity.lifecycle.object_alive)
    };
    let mut frames = 0;
    while alive(&sim) {
        frames += 1;
        assert!(frames < 60, "the fall reaches the ground");
        tick(&mut sim);
        tick(&mut restored);
        assert_eq!(restored.state_hash(), sim.state_hash(), "frame {frames}");
        assert_eq!(alive(&restored), alive(&sim));
    }
}

/// `Fire_Death_Weapon` hands its bullet straight to `DetonateAtCoord`
/// (`0x0070D782`). A BulletClass hit runs the cluster loop, which after every
/// cluster, the last included, draws the next cluster's coordinate
/// (`0x00469020..0x00469091`); the death weapon's single detonation draws
/// nothing there.
#[test]
fn a_death_weapon_detonates_once_without_cluster_draws() {
    use crate::sim::projectile::{
        ProjectileCoord, ProjectileDetonation, ProjectileDetonationReason, ProjectilePayload,
        ProjectileTarget,
    };
    let (mut sim, _) = fixture(&serde_json::json!({"health": 150}));
    let rules = RuleSet::from_ini(&IniFile::from_str(
        "[AircraftTypes]\n0=TEST\n[TEST]\nStrength=150\nPrimary=CrashGun\n\
         [CrashGun]\nDamage=150\nWarhead=CrashWH\nProjectile=CrashProj\n\
         [CrashProj]\nInviso=no\n\
         [CrashWH]\nCellSpread=1\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n",
    ))
    .unwrap();
    sim.resolve_type_handles(&rules);
    let payload = ProjectilePayload {
        base_damage: 150,
        warhead: sim.interner.intern("CrashWH"),
        weapon: sim.interner.intern("CrashGun"),
    };
    let detonation = |reason| ProjectileDetonation {
        projectile_id: 1,
        source_id: 1,
        target: ProjectileTarget::Cell { rx: 40, ry: 40 },
        impact: ProjectileCoord {
            x: 40 * 256 + 128,
            y: 40 * 256 + 128,
            z: 0,
        },
        payload,
        reason,
    };
    let before = sim.scenario_rng.state();
    sim.commit_logic_projectile_detonations(
        &rules,
        None,
        &[detonation(ProjectileDetonationReason::DeathWeapon)],
    );
    assert_eq!(sim.scenario_rng.state(), before, "no cluster draws");
    sim.commit_logic_projectile_detonations(
        &rules,
        None,
        &[detonation(ProjectileDetonationReason::ReachedTarget)],
    );
    assert_ne!(
        sim.scenario_rng.state(),
        before,
        "a bullet hit draws its next cluster"
    );
}

/// A crashable Jumpjet unit: `TEST` flies the Nighthawk's type block with
/// `BalloonHover=` as asked (a Kirov's impact fires its current weapon, any
/// other type's plays its `Explosion=` once more) and seats two `RIDER`s. The
/// type names an `ImpactLandSound=` the Jumpjet impact must not play.
fn jumpjet_rules(balloon: bool) -> RuleSet {
    let mut rules = RuleSet::from_ini(&IniFile::from_str(&format!(
        "[General]\nConditionRed=0.25\n\
         [AudioVisual]\nImpactLandSound=RulesLand\n\
         [VehicleTypes]\n0=TEST\n1=VICTIM\n\
         [InfantryTypes]\n0=RIDER\n\
         [TEST]\nStrength=300\nArmor=none\nCrashable=yes\nBalloonHover={}\nPassengers=2\n\
         Locomotor={{92612C46-F71F-11d1-AC9F-006008055BB5}}\nSpeedType=Hover\nMovementZone=Fly\n\
         JumpjetHeight=500\n\
         JumpjetClimb=10\nJumpjetCrash=40\nJumpjetSpeed=30\nJumpjetNoWobbles=yes\n\
         Primary=CrashGun\nExplosion=BOOM\nCrashingSound=JJDie\nImpactLandSound=TypeLand\n\
         [VICTIM]\nStrength=1000\nArmor=none\n\
         [RIDER]\nStrength=100\nArmor=none\n\
         [CrashGun]\nDamage=150\nWarhead=CrashWH\n\
         [CrashWH]\nCellSpread=1\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n",
        if balloon { "yes" } else { "no" },
    )))
    .unwrap();
    let mut art = crate::rules::art_data::ArtRegistry::from_ini(&IniFile::from_str(
        "[BOOM]\nRate=100\n[SGRYSMK1]\nRate=100\n",
    ));
    art.bind_anim_frame_count_for_test("BOOM", 20);
    art.bind_anim_frame_count_for_test("SGRYSMK1", 20);
    rules.art_registry = art;
    rules
}

/// Unit 1 hovers at `JumpjetHeight=` over cell (52, 52) of the aircraft
/// fixture's map, holding that cell's air slot with the moving byte set, as
/// the native corpus's kill leaves a hovering Jumpjet; its two riders sit in
/// its cargo, a `VICTIM` stands below and a Soviet shooter at (40, 40).
/// Answers the victim's and the shooter's ids.
fn jumpjet_fixture(balloon: bool) -> (Simulation, RuleSet, u64, u64) {
    use crate::sim::house_state::HouseState;
    use crate::sim::passenger::{PassengerCargo, PassengerRole};
    let rules = jumpjet_rules(balloon);
    let mut sim = Simulation::with_seed(0);
    install_common_raw_terrain(&mut sim, 70, 70, 0, None);
    sim.playfield_bounds = Some(crate::sim::cell_rect::PlayfieldBounds {
        base: 64,
        off_fc: 0,
        off_100: 0,
        off_104: 64,
        off_108: 64,
    });
    sim.playfield_size_height = Some(64);
    sim.session.binary_frame = 1000;
    for (name, human) in [("Americans", true), ("Soviets", false)] {
        let house = sim.interner.intern(name);
        sim.houses
            .insert(house, HouseState::new(house, 0, None, human, 0, 10));
    }
    let reveal = |sim: &mut Simulation, id: u64, rx: u16, ry: u16| {
        assert!(matches!(
            sim.try_reveal_entity(
                id,
                RevealRequest {
                    position: RevealPosition {
                        rx,
                        ry,
                        z: 0,
                        sub_x: SimFixed::from_num(128),
                        sub_y: SimFixed::from_num(128),
                    },
                    placement: PlacementEvidence::MarkSucceeded,
                    logic_eligible: true,
                }
            ),
            RevealOutcome::Revealed { .. }
        ));
    };
    assert_eq!(sim.allocate_stable_id(), 1);
    insert_entity(&mut sim, 1, EntityCategory::Unit);
    sim.substrate.entities.get_mut(1).unwrap().locomotor = Some(LocomotorState::from_object_type(
        rules.object("TEST").unwrap(),
        0,
    ));
    reveal(&mut sim, 1, 52, 52);
    sim.remove_entity_occupancy(1);
    {
        let entity = sim.substrate.entities.get_mut(1).unwrap();
        entity.health.current = 300;
        entity.position.exact_z_leptons = Some(500);
        entity.body_facing = Some(FacingClass::new(0x4000, 5));
        let loco = entity.locomotor.as_mut().unwrap();
        loco.altitude = SimFixed::from_num(500);
        let runtime = loco.jumpjet_runtime_mut().unwrap();
        runtime.phase = crate::sim::movement::jumpjet_flight::STATE_HOLD;
        runtime.moving = true;
        runtime.destination = DriveCoord {
            x: START,
            y: START,
            z: 0,
        };
        runtime.flight.facing.snap(0x4000, 1000);
        runtime.flight.target_height = 500;
        entity.passenger_role = PassengerRole::Transport {
            cargo: PassengerCargo::new(2, 0),
        };
    }
    sim.add_entity_occupancy(1);
    assert!(sim.substrate.air_slots.claim(52, 52, 1));
    for _ in 0..2 {
        let rider = sim
            .construct_object_limbo_at_height("RIDER", "Americans", 52, 52, 0, 0, &rules)
            .expect("rider");
        sim.substrate
            .entities
            .get_mut(rider)
            .unwrap()
            .passenger_role = PassengerRole::Inside { transport_id: 1 };
        sim.substrate
            .entities
            .get_mut(1)
            .unwrap()
            .passenger_role
            .cargo_mut()
            .unwrap()
            .board_forced(rider, 1);
    }
    let spawn = |sim: &mut Simulation, owner: &str, rx: u16, ry: u16| {
        let owner = sim.interner.intern(owner);
        let type_ref = sim.interner.intern("VICTIM");
        let id = sim.allocate_stable_id();
        let mut entity = crate::sim::game_entity::GameEntity::new_at_frame_zero_for_test(
            id,
            rx,
            ry,
            0,
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
        reveal(sim, id, rx, ry);
        id
    };
    let victim = spawn(&mut sim, "Americans", 52, 52);
    let shooter = spawn(&mut sim, "Soviets", 40, 40);
    (sim, rules, victim, shooter)
}

/// What a crashed Jumpjet's fall looked like through `advance_tick`.
struct JumpjetFall {
    /// The wreck's Z after each frame it survived.
    heights: Vec<i32>,
    /// The frame the impact UnInit it.
    impact_frame: usize,
    crash_sound_frames: Vec<usize>,
    impact_sounds: usize,
    /// `Explosion=` anims built in the impact frame.
    impact_booms: usize,
}

fn fall_to_the_impact(sim: &mut Simulation, rules: &RuleSet) -> JumpjetFall {
    use std::collections::BTreeMap;
    let grid = crate::sim::pathfinding::PathGrid::test_all_passable(70, 70);
    let boom = sim.interner.intern("BOOM");
    let mut fall = JumpjetFall {
        heights: Vec::new(),
        impact_frame: 0,
        crash_sound_frames: Vec::new(),
        impact_sounds: 0,
        impact_booms: 0,
    };
    for frame in 1..40 {
        sim.sound_events.clear();
        let booms_before = sim
            .substrate
            .anims
            .iter()
            .filter(|(_, anim)| anim.type_id == boom)
            .count();
        sim.advance_tick(&[], Some(rules), &BTreeMap::new(), Some(&grid), None, 67);
        if sim.sound_events.iter().any(|event| {
            matches!(
                event,
                super::SimSoundEvent::AnimationStarted { anim_id: 1, .. }
            )
        }) {
            fall.crash_sound_frames.push(frame);
        }
        fall.impact_sounds += sim
            .sound_events
            .iter()
            .filter(|event| {
                matches!(event, super::SimSoundEvent::VocAt { sound_id, .. }
                    if sound_id == "TypeLand" || sound_id == "RulesLand")
            })
            .count();
        match sim.substrate.entities.get(1) {
            Some(entity) if entity.lifecycle.object_alive => {
                assert!(entity.crashing && entity.health.current == 0);
                fall.heights
                    .push(entity.position.exact_z_leptons.expect("exact Z"));
            }
            _ => {
                fall.impact_frame = frame;
                fall.impact_booms = sim
                    .substrate
                    .anims
                    .iter()
                    .filter(|(_, anim)| anim.type_id == boom)
                    .count()
                    - booms_before;
                return fall;
            }
        }
    }
    panic!("the wreck never reached the ground: {:?}", fall.heights);
}

/// Kill unit 1 through the production receiver: one area hit from the
/// Soviet shooter.
fn shoot_down(sim: &mut Simulation, rules: &RuleSet, shooter: u64) {
    use crate::sim::combat::combat_aoe::AreaDamageReceiver;
    let soviets = sim.interner.intern("Soviets");
    let warhead = sim.interner.intern("CrashWH");
    let hit =
        crate::sim::combat::EntityDamageEvent::area(1, 400, 0, shooter, Some(soviets), warhead);
    sim.commit_noncombat_aoe_receivers(rules, None, &[AreaDamageReceiver::Entity(hit)]);
}

/// A hovering Nighthawk-like Jumpjet shot down through the production
/// receiver: `UnitClass::ReceiveDamage` kills its riders with the shooter's
/// credit above 0xD0 leptons and crashes it instead of its UnInit, and the
/// kill's Stun re-targets it through `Stop_Moving`. Through `advance_tick` it
/// then falls by `JumpjetClimb=` plus `JumpjetCrash=` a frame, plays
/// `CrashingSound=` on the edge, and at the ground releases its air slot,
/// plays its `Explosion=` once more and is UnInit, with no impact sound.
#[test]
fn a_shot_down_jumpjet_crashes_through_the_production_receiver() {
    let (mut sim, rules, victim, shooter) = jumpjet_fixture(false);
    let riders: Vec<u64> = sim
        .substrate
        .entities
        .get(1)
        .unwrap()
        .passenger_role
        .cargo()
        .unwrap()
        .passengers
        .clone();
    shoot_down(&mut sim, &rules, shooter);

    let entity = sim
        .substrate
        .entities
        .get(1)
        .expect("a crashing unit stays");
    assert!(entity.crashing && entity.lifecycle.object_alive && !entity.dying);
    assert_eq!(entity.health.current, 0);
    assert!(
        entity
            .rocking
            .as_ref()
            .is_some_and(|r| r.vel_sideways != SimFixed::ZERO),
        "Crash drew the spin"
    );
    assert!(
        entity.passenger_role.cargo().unwrap().passengers.is_empty(),
        "KillPassengers emptied the cargo"
    );
    for rider in riders {
        assert!(
            sim.substrate
                .entities
                .get(rider)
                .is_none_or(|rider| !rider.lifecycle.object_alive),
            "rider {rider} died with the transport"
        );
    }
    let soviets = sim.interner.intern("Soviets");
    assert_eq!(
        sim.houses[&soviets].stats.units_killed, 2,
        "the riders' kills go to the shooter; the wreck's comes at its UnInit"
    );

    // The kill's Stop_Moving searched from the hover cell, which the victim's
    // vehicle bit refuses (`CheckCellPassability @ 0x004834A0`), and
    // re-targeted a free neighbour.
    let entity = sim.substrate.entities.get(1).unwrap();
    let runtime = entity
        .locomotor
        .as_ref()
        .unwrap()
        .jumpjet_runtime()
        .unwrap();
    assert!(runtime.moving);
    assert_eq!(
        runtime.destination,
        DriveCoord {
            x: START - 256,
            y: START - 256,
            z: 0
        }
    );

    let fall = fall_to_the_impact(&mut sim, &rules);
    // Over a destination in another cell, the hold's Update reads the cell
    // top, which the victim lifts by 85: the first frame climbs 10 before
    // State 5 drops 40, then Update's descent and the crash take 50 a frame,
    // and near the ground Update climbs against the drop again.
    assert_eq!(
        fall.heights,
        vec![470, 420, 370, 320, 270, 220, 170, 120, 70, 40, 10]
    );
    assert_eq!(fall.impact_frame, 12);
    assert_eq!(
        fall.crash_sound_frames,
        vec![1],
        "CrashingSound= on the edge"
    );
    assert_eq!(fall.impact_sounds, 0, "a Jumpjet impact plays no sound");
    assert_eq!(
        fall.impact_booms, 1,
        "Death_Explosion once more at the impact"
    );
    assert_eq!(sim.substrate.air_slots.holder(52, 52), None);
    assert_eq!(
        sim.substrate.entities.get(victim).unwrap().health.current,
        1000,
        "an Explosion= anim deals no damage"
    );
    assert_eq!(sim.houses[&soviets].stats.units_killed, 3);
}

/// A `BalloonHover=` wreck (the Kirov) fires its current weapon as its death
/// weapon at the impact (`0x007461EF`), which strikes the unit below.
///
/// A balloon's Update keeps reading the cell top even over its own cell, and
/// the victim below lifts that by 85 (`CellClass @ 0x00485080`): the first
/// frame climbs 10 before State 5's drop, and near the ground Update climbs
/// against the drop again, so the fall takes a frame more than the
/// Nighthawk's.
#[test]
fn a_shot_down_balloon_jumpjet_bombs_its_impact_cell() {
    let (mut sim, rules, victim, shooter) = jumpjet_fixture(true);
    shoot_down(&mut sim, &rules, shooter);
    let fall = fall_to_the_impact(&mut sim, &rules);
    assert_eq!(
        fall.heights,
        vec![470, 420, 370, 320, 270, 220, 170, 120, 70, 40, 10]
    );
    assert_eq!(fall.impact_frame, 12);
    assert_eq!(fall.impact_booms, 0, "no second Explosion= for a balloon");
    assert_eq!(fall.impact_sounds, 0);
    assert_eq!(
        sim.substrate.entities.get(victim).unwrap().health.current,
        1000 - 150,
        "the death weapon's CrashGun hit the victim below"
    );
}

/// An order dropped in the cruise (an Attack order or the attack approach
/// dropping the goal) runs `Stop_Moving` through Foot's null arm, which keeps
/// the moving byte: shot down there, the wreck still latches into State 5 and
/// reaches the ground. (A hold without the moving byte never latches, natively
/// too; VERA no longer makes one out of a dropped order.)
#[test]
fn a_jumpjet_shot_down_after_its_order_dropped_reaches_the_ground() {
    let (mut sim, rules, _, shooter) = jumpjet_fixture(false);
    assert!(sim.issue_air_cell_destination(1, (60, 52), SimFixed::from_num(30), Some(&rules)));
    // Cruise until the owner has left the hover cell.
    let mut frame = 1001;
    while sim
        .substrate
        .entities
        .get(1)
        .is_some_and(|entity| (entity.position.rx, entity.position.ry) == (52, 52))
    {
        assert!(frame < 1200, "the cruise never left the hover cell");
        sim.session.binary_frame = frame;
        sim.tick_air_movement_with_cell_lists_one(1, Some(&rules));
        frame += 1;
    }
    let runtime = |sim: &Simulation| {
        sim.substrate
            .entities
            .get(1)
            .and_then(|entity| entity.locomotor.as_ref())
            .and_then(|locomotor| locomotor.jumpjet_runtime())
            .cloned()
            .expect("runtime")
    };
    assert_eq!(
        runtime(&sim).phase,
        crate::sim::movement::jumpjet_flight::STATE_TRANSLATE
    );
    let entity = sim.substrate.entities.get_mut(1).unwrap();
    let here = (entity.position.rx, entity.position.ry);
    entity.movement_target = None;
    sim.session.binary_frame = frame;
    sim.tick_air_movement_with_cell_lists_one(1, Some(&rules));
    let stopped = runtime(&sim);
    assert!(stopped.moving, "Stop_Moving keeps the moving byte");
    assert_eq!(
        stopped.destination,
        DriveCoord {
            x: i32::from(here.0) * 256 + 128,
            y: i32::from(here.1) * 256 + 128,
            z: 0,
        }
    );

    shoot_down(&mut sim, &rules, shooter);
    let fall = fall_to_the_impact(&mut sim, &rules);
    assert!(fall.impact_frame > 0, "the wreck reached the ground");
    assert!(fall.heights.windows(2).all(|pair| pair[1] < pair[0]));
}

/// Shot down one climb step above the ground in State 4, the kill's
/// `Stop_Moving` lifts the descent back into State 1 (`0x0054B455..0x0054B467`),
/// so the next frame's Update climbs and the latch still engages: the wreck
/// crashes instead of touching down (the native corpus's `SHAD` touchdown row).
#[test]
fn a_jumpjet_shot_down_touching_down_still_crashes() {
    let (mut sim, rules, _, shooter) = jumpjet_fixture(false);
    {
        let entity = sim.substrate.entities.get_mut(1).unwrap();
        entity.position.exact_z_leptons = Some(10);
        let loco = entity.locomotor.as_mut().unwrap();
        loco.altitude = SimFixed::from_num(10);
        let runtime = loco.jumpjet_runtime_mut().unwrap();
        runtime.phase = crate::sim::movement::jumpjet_flight::STATE_DESCEND;
        runtime.flight.target_height = 0;
        runtime.landing_latched = true;
    }
    shoot_down(&mut sim, &rules, shooter);
    let entity = sim.substrate.entities.get(1).unwrap();
    assert!(entity.crashing);
    let runtime = entity
        .locomotor
        .as_ref()
        .unwrap()
        .jumpjet_runtime()
        .unwrap();
    assert_eq!(
        runtime.phase,
        crate::sim::movement::jumpjet_flight::STATE_ASCEND
    );
    assert_eq!(runtime.flight.target_height, 500);
    assert!(runtime.moving && !runtime.landing_latched);

    let fall = fall_to_the_impact(&mut sim, &rules);
    assert_eq!(
        (fall.heights.len(), fall.impact_frame),
        (0, 1),
        "climbs to 20, then State 5's drop of 40 hits the ground"
    );
    assert_eq!(
        fall.impact_booms, 1,
        "Death_Explosion once more at the impact"
    );
}

/// On the ground a crashable unit's Crash refuses, and the receiver UnInits
/// it as before (`0x00738475..0x0073847F`).
#[test]
fn a_landed_jumpjet_dies_where_it_stands() {
    let (mut sim, rules, _, shooter) = jumpjet_fixture(false);
    {
        let entity = sim.substrate.entities.get_mut(1).unwrap();
        entity.position.exact_z_leptons = Some(0);
        let loco = entity.locomotor.as_mut().unwrap();
        loco.altitude = SimFixed::ZERO;
        loco.jumpjet_runtime_mut().unwrap().phase =
            crate::sim::movement::jumpjet_flight::STATE_GROUND;
    }
    shoot_down(&mut sim, &rules, shooter);
    assert!(
        sim.substrate
            .entities
            .get(1)
            .is_none_or(|entity| !entity.lifecycle.object_alive),
        "no crash on the ground"
    );
}

/// Retail Dustbowl runtime, end to end through production: a Harrier ordered
/// at three flak tracks takes off, their `FlakTrackAAGun` volleys shoot it
/// down, and it crashes. `VoiceCrashing=`/`CrashingSound=` play on the edge,
/// SGRYSMK1 smoke trails the spinning fall, whose frames are the original
/// executable's to the impact, and there its current weapon detonates as the
/// death weapon and `ImpactLandSound=` plays before it is UnInit. Ignored:
/// needs the retail install (`RA2_DIR` or `config.toml`).
#[test]
#[ignore = "requires a retail RA2/YR install (RA2_DIR or config.toml)"]
fn retail_dustbowl_flak_shoots_a_harrier_down() {
    use crate::sim::command::{Command, CommandEnvelope};
    use crate::sim::house_state::HouseState;
    use crate::sim::movement::air_movement::current_fly_height;
    use std::collections::BTreeSet;

    let dir = std::env::var("RA2_DIR")
        .ok()
        .filter(|path| !path.trim().is_empty())
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| {
            crate::util::config::GameConfig::load()
                .expect("set RA2_DIR or provide config.toml for this ignored test")
                .paths
                .ra2_dir
        });
    let mut scenario =
        crate::headless_scenario::load(&dir, "Dustbowl.mmx", 0x00C0_FFEE).expect("Dustbowl loads");
    let crate::sim::runtime::SimRuntime {
        simulation: sim,
        resources,
    } = &mut scenario.runtime;
    for (name, side, human) in [("Americans", 0, true), ("Russians", 1, false)] {
        let house = sim.interner.intern(name);
        sim.houses
            .entry(house)
            .or_insert_with(|| HouseState::new(house, side, None, human, 10_000, 10));
        if !sim.session.house_order.contains(&house) {
            sim.session.house_order.push(house);
        }
    }
    // A Harrier on open level ground with three flak tracks eight and nine
    // cells east: beyond their sight while it stands, inside their AA range
    // once it lifts off. Each side keeps a power plant out of the fight, so
    // neither house is defeated under the Battle mode's ShortGame.
    let (harrier, flak) = (40..100_u16)
        .flat_map(|y| (40..100_u16).map(move |x| (x, y)))
        .find_map(|(x, y)| {
            let grid = sim.path_grid()?;
            let terrain = sim.resolved_terrain.as_ref()?;
            let level = terrain.cell(x, y)?.level;
            let open = (x.checked_sub(4)?..=x + 13).all(|cx| {
                (y - 1..=y + 1).all(|cy| {
                    terrain.cell(cx, cy).is_some_and(|cell| cell.level == level)
                        && grid.cell(cx, cy).is_some_and(|cell| cell.ground_walkable)
                })
            });
            if !open {
                return None;
            }
            for (plant, owner, px) in [
                ("GAPOWR", "Americans", x - 4),
                ("NAPOWR", "Russians", x + 12),
            ] {
                sim.spawn_object(
                    plant,
                    owner,
                    px,
                    y - 1,
                    0,
                    &resources.rules,
                    &resources.height_map,
                )?;
            }
            let harrier = sim.spawn_object(
                "ORCA",
                "Americans",
                x,
                y,
                64,
                &resources.rules,
                &resources.height_map,
            )?;
            let flak = [(x + 8, y - 1), (x + 8, y + 1), (x + 9, y)]
                .into_iter()
                .map(|(fx, fy)| {
                    sim.spawn_object(
                        "HTK",
                        "Russians",
                        fx,
                        fy,
                        192,
                        &resources.rules,
                        &resources.height_map,
                    )
                })
                .collect::<Option<Vec<_>>>()?;
            Some((harrier, flak))
        })
        .expect("open level ground for the fight");
    sim.resolve_type_handles(&resources.rules);
    let americans = sim.interner.intern("Americans");
    let russians = sim.interner.intern("Russians");
    let smoke_type = sim.interner.intern("SGRYSMK1");
    // The Harrier's death weapon is its current weapon, Maverick (ORCAAP).
    let impact_anims: BTreeSet<_> = resources
        .rules
        .warhead("ORCAAP")
        .expect("retail ORCAAP")
        .anim_list
        .iter()
        .map(|name| sim.interner.intern(name))
        .collect();
    let order = CommandEnvelope::new(
        americans,
        sim.session.tick + 1,
        Command::Attack {
            attacker_id: harrier,
            target_id: flak[0],
        },
    );
    scenario
        .runtime
        .advance_frame(
            &[order],
            crate::headless_scenario::SIM_TICK_MS,
            super::TickLane::Ordinary,
        )
        .expect("the order frame");

    let mut lifted = false;
    let mut crash_frame = None;
    let mut impact_frame = None;
    let mut heights = Vec::new();
    let mut track = Vec::new();
    let mut spun = false;
    let mut smoke = BTreeSet::new();
    let mut sounds = Vec::new();
    let mut impact_explosions = Vec::new();
    for frame in 1..=900 {
        let anims_before: BTreeSet<_> = scenario
            .sim()
            .substrate
            .anims
            .iter()
            .map(|(id, _)| *id)
            .collect();
        let output = scenario
            .runtime
            .advance_frame(
                &[],
                crate::headless_scenario::SIM_TICK_MS,
                super::TickLane::Ordinary,
            )
            .expect("a retail frame");
        let sim = scenario.sim();
        for event in &output.sound_events {
            match event {
                super::SimSoundEvent::VocAt { sound_id, .. } => {
                    sounds.push((frame, sound_id.clone()));
                }
                super::SimSoundEvent::AnimationStarted {
                    anim_id, sound_id, ..
                } if *anim_id == harrier => {
                    sounds.push((frame, sim.interner.resolve(*sound_id).to_string()));
                }
                _ => {}
            }
        }
        let Some(entity) = sim
            .substrate
            .entities
            .get(harrier)
            .filter(|entity| entity.lifecycle.object_alive)
        else {
            assert!(crash_frame.is_some(), "the Harrier left before crashing");
            impact_frame = Some(frame);
            impact_explosions = sim
                .substrate
                .anims
                .iter()
                .filter(|(id, anim)| {
                    !anims_before.contains(id) && impact_anims.contains(&anim.type_id)
                })
                .map(|(_, anim)| sim.interner.resolve(anim.type_id).to_string())
                .collect();
            break;
        };
        let height = current_fly_height(entity, sim.resolved_terrain.as_ref());
        lifted |= height > 0;
        if !entity.crashing && frame % 15 == 0 {
            println!(
                "frame {frame}: height {height}, health {}, mission {:?}",
                entity.health.current, entity.aircraft_mission
            );
        }
        if entity.crashing {
            if crash_frame.is_none() {
                crash_frame = Some(frame);
                assert!(height > 0, "shot down in the air");
                assert_eq!(entity.health.current, 0);
                assert_eq!(entity.killed_by, Some(russians));
            }
            let rocking = entity.rocking.as_ref().expect("a crash spins");
            spun |= rocking.angle_sideways != SimFixed::ZERO;
            heights.push(height);
            let xy = crate::sim::movement::ground_pose::position_world_xy(&entity.position);
            track.push((xy[0], xy[1]));
            println!(
                "frame {frame}: height {height}, roll {:.4}, pitch {:.4}",
                rocking.angle_sideways.to_num::<f64>(),
                rocking.angle_forwards.to_num::<f64>()
            );
            for (id, anim) in sim.substrate.anims.iter() {
                if anim.type_id == smoke_type {
                    smoke.insert(*id);
                }
            }
        }
    }
    println!("sounds: {sounds:?}");
    let crash_frame = crash_frame.expect("the flak shoots the Harrier down");
    let impact_frame = impact_frame.expect("the crash reaches the ground");
    println!(
        "lifted, crashed at frame {crash_frame}, impact at frame {impact_frame}, {} smoke puffs, \
         impact explosions {impact_explosions:?}",
        smoke.len()
    );
    assert!(lifted);
    // Killed cruising at FlightLevel and full speed, it falls exactly as the
    // original executable's `cruise_1500_full_speed` row, to the impact.
    let native = oracle()["fall"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["input"]["name"] == "cruise_1500_full_speed")
        .expect("the oracle's cruise row")["frames"]
        .as_array()
        .unwrap()
        .iter()
        .map(|frame| frame["xyz"][2].as_i64().unwrap() as i32)
        .collect::<Vec<_>>();
    assert_eq!(heights[0], 1500);
    assert_eq!(heights[1..], native[..native.len() - 1], "the fall frames");
    assert_eq!(native.last(), Some(&0), "the native impact frame");
    assert_eq!(impact_frame - crash_frame, native.len() as i32);
    let steps: Vec<(i32, i32)> = track
        .windows(2)
        .map(|pair| (pair[1].0 - pair[0].0, pair[1].1 - pair[0].1))
        .collect();
    println!("fall steps: {steps:?}");
    // The paid step keeps the frozen cruise speed (the native row steps 35
    // leptons a frame); the heading finishes whatever turn was under way.
    assert!(
        steps.iter().all(|&(dx, dy)| {
            let length = f64::from(dx * dx + dy * dy).sqrt();
            (33.5..=36.5).contains(&length)
        }),
        "{steps:?}"
    );
    assert!(spun, "the crash spin turns the body");
    assert!(!smoke.is_empty(), "the falling Harrier trails SGRYSMK1");
    assert!(
        !impact_explosions.is_empty(),
        "its Maverick detonates at the impact"
    );
    let heard = |name: &str| {
        sounds
            .iter()
            .filter(|(_, sound)| sound == name)
            .map(|(frame, _)| *frame)
            .collect::<Vec<_>>()
    };
    // The crash edge runs in the Harrier's own AI after the hit latched it;
    // the impact cue on the frame its height reached zero.
    assert_eq!(heard("IntruderVoiceDie").len(), 1, "{sounds:?}");
    assert_eq!(heard("IntruderDie").len(), 1, "{sounds:?}");
    assert_eq!(heard("GenAircraftCrash"), vec![impact_frame], "{sounds:?}");
}

/// Retail Dustbowl runtime, end to end through production: three flak tracks
/// shoot down an Allied Nighthawk carrying two GIs and a Kirov left with 1
/// Health as both fly over them. The Nighthawk's riders die with the flak's
/// credit, and each wreck spins down under the Jumpjet State 5, a Nighthawk
/// by `JumpjetClimb=` + `JumpjetCrash=` (50) a frame and a Kirov by 18,
/// playing `CrashingSound=` and the owner's `VoiceCrashing=` on the edge.
/// At the ground the Nighthawk plays its `Explosion=` once more and the Kirov
/// drops its `BlimpBomb` as its death weapon; neither plays an impact sound.
/// Ignored: needs the retail install (`RA2_DIR` or `config.toml`).
#[test]
#[ignore = "requires a retail RA2/YR install (RA2_DIR or config.toml)"]
fn retail_dustbowl_flak_shoots_down_a_nighthawk_and_a_kirov() {
    use crate::sim::command::{Command, CommandEnvelope};
    use crate::sim::house_state::HouseState;
    use crate::sim::movement::air_movement::current_fly_height;
    use crate::sim::passenger::PassengerRole;
    use std::collections::{BTreeMap, BTreeSet};

    let dir = std::env::var("RA2_DIR")
        .ok()
        .filter(|path| !path.trim().is_empty())
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| {
            crate::util::config::GameConfig::load()
                .expect("set RA2_DIR or provide config.toml for this ignored test")
                .paths
                .ra2_dir
        });
    let mut scenario =
        crate::headless_scenario::load(&dir, "Dustbowl.mmx", 0x00C0_FFEE).expect("Dustbowl loads");
    let crate::sim::runtime::SimRuntime {
        simulation: sim,
        resources,
    } = &mut scenario.runtime;
    for (name, side, human) in [("Americans", 0, true), ("Russians", 1, false)] {
        let house = sim.interner.intern(name);
        sim.houses
            .entry(house)
            .or_insert_with(|| HouseState::new(house, side, None, human, 10_000, 10));
        if !sim.session.house_order.contains(&house) {
            sim.session.house_order.push(house);
        }
    }
    // Open level ground: the Nighthawk and the Kirov land west of three flak
    // tracks and are ordered past them. Each side keeps a power plant out of
    // the fight, so neither house is defeated under the Battle mode's
    // ShortGame.
    let (nighthawk, kirov, destination) = (40..100_u16)
        .flat_map(|y| (40..100_u16).map(move |x| (x, y)))
        .find_map(|(x, y)| {
            let grid = sim.path_grid()?;
            let terrain = sim.resolved_terrain.as_ref()?;
            let level = terrain.cell(x, y)?.level;
            let open = (x - 4..=x + 16).all(|cx| {
                (y - 2..=y + 3).all(|cy| {
                    terrain.cell(cx, cy).is_some_and(|cell| cell.level == level)
                        && grid.cell(cx, cy).is_some_and(|cell| cell.ground_walkable)
                })
            });
            if !open {
                return None;
            }
            for (plant, owner, px) in [
                ("GAPOWR", "Americans", x - 4),
                ("NAPOWR", "Russians", x + 14),
            ] {
                sim.spawn_object(
                    plant,
                    owner,
                    px,
                    y - 2,
                    0,
                    &resources.rules,
                    &resources.height_map,
                )?;
            }
            let nighthawk = sim.spawn_object(
                "SHAD",
                "Americans",
                x,
                y,
                64,
                &resources.rules,
                &resources.height_map,
            )?;
            let kirov = sim.spawn_object(
                "ZEP",
                "Americans",
                x,
                y + 2,
                64,
                &resources.rules,
                &resources.height_map,
            )?;
            for (fx, fy) in [(x + 8, y - 1), (x + 8, y + 1), (x + 9, y)] {
                sim.spawn_object(
                    "HTK",
                    "Russians",
                    fx,
                    fy,
                    192,
                    &resources.rules,
                    &resources.height_map,
                )?;
            }
            Some((nighthawk, kirov, (x + 12, y)))
        })
        .expect("open level ground for the fight");
    let riders: Vec<u64> = (0..2)
        .map(|_| {
            let rider = sim
                .construct_object_limbo_at_height("E1", "Americans", 0, 0, 0, 0, &resources.rules)
                .expect("a GI");
            sim.substrate
                .entities
                .get_mut(rider)
                .unwrap()
                .passenger_role = PassengerRole::Inside {
                transport_id: nighthawk,
            };
            sim.substrate
                .entities
                .get_mut(nighthawk)
                .unwrap()
                .passenger_role
                .cargo_mut()
                .expect("the Nighthawk carries")
                .board_forced(rider, 1);
            rider
        })
        .collect();
    sim.substrate
        .entities
        .get_mut(kirov)
        .unwrap()
        .health
        .current = 1;
    sim.resolve_type_handles(&resources.rules);
    let americans = sim.interner.intern("Americans");
    let russians = sim.interner.intern("Russians");
    let intern_all = |sim: &mut Simulation, names: &[String]| -> BTreeSet<_> {
        names.iter().map(|name| sim.interner.intern(name)).collect()
    };
    let explosions = intern_all(
        sim,
        &resources
            .rules
            .object("SHAD")
            .expect("retail SHAD")
            .explosion_anims,
    );
    let bomb_anims = intern_all(
        sim,
        &resources
            .rules
            .warhead("BlimpHE")
            .expect("retail BlimpHE")
            .anim_list,
    );
    let orders: Vec<_> = [nighthawk, kirov]
        .into_iter()
        .map(|entity_id| {
            CommandEnvelope::new(
                americans,
                sim.session.tick + 1,
                Command::Move {
                    entity_id,
                    target_rx: destination.0,
                    target_ry: destination.1,
                    queue: false,
                    group_id: None,
                },
            )
        })
        .collect();
    scenario
        .runtime
        .advance_frame(
            &orders,
            crate::headless_scenario::SIM_TICK_MS,
            super::TickLane::Ordinary,
        )
        .expect("the order frame");

    #[derive(Default)]
    struct Wreck {
        crash_frame: Option<i32>,
        impact_frame: Option<i32>,
        heights: Vec<i32>,
        impact_anims: Vec<String>,
    }
    let mut wrecks: BTreeMap<u64, Wreck> = BTreeMap::new();
    let mut sounds = Vec::new();
    let mut riders_killed_by_crash = None;
    for frame in 1..=1500 {
        let anims_before: BTreeSet<_> = scenario
            .sim()
            .substrate
            .anims
            .iter()
            .map(|(id, _)| *id)
            .collect();
        let output = scenario
            .runtime
            .advance_frame(
                &[],
                crate::headless_scenario::SIM_TICK_MS,
                super::TickLane::Ordinary,
            )
            .expect("a retail frame");
        let sim = scenario.sim();
        for event in &output.sound_events {
            match event {
                super::SimSoundEvent::VocAt { sound_id, .. } => {
                    sounds.push((frame, sound_id.clone()));
                }
                super::SimSoundEvent::AnimationStarted {
                    anim_id, sound_id, ..
                } if *anim_id == nighthawk || *anim_id == kirov => {
                    sounds.push((frame, sim.interner.resolve(*sound_id).to_string()));
                }
                _ => {}
            }
        }
        for id in [nighthawk, kirov] {
            let wreck = wrecks.entry(id).or_default();
            if wreck.impact_frame.is_some() {
                continue;
            }
            let Some(entity) = sim
                .substrate
                .entities
                .get(id)
                .filter(|entity| entity.lifecycle.object_alive)
            else {
                assert!(wreck.crash_frame.is_some(), "{id} left before crashing");
                wreck.impact_frame = Some(frame);
                wreck.impact_anims = sim
                    .substrate
                    .anims
                    .iter()
                    .filter(|(anim_id, _)| !anims_before.contains(anim_id))
                    .map(|(_, anim)| sim.interner.resolve(anim.type_id).to_string())
                    .collect();
                continue;
            };
            if !entity.crashing {
                continue;
            }
            let height = current_fly_height(entity, sim.resolved_terrain.as_ref());
            if wreck.crash_frame.is_none() {
                wreck.crash_frame = Some(frame);
                assert!(height > 0, "{id} shot down in the air");
                assert_eq!(entity.health.current, 0);
                assert_eq!(entity.killed_by, Some(russians));
                if id == nighthawk {
                    riders_killed_by_crash = Some(sim.houses[&russians].stats.units_killed);
                    assert!(
                        entity
                            .passenger_role
                            .cargo()
                            .is_some_and(|cargo| cargo.passengers.is_empty()),
                        "the riders died with the Nighthawk"
                    );
                }
            }
            wreck.heights.push(height);
        }
        if wrecks.values().all(|wreck| wreck.impact_frame.is_some()) {
            break;
        }
    }
    println!("sounds: {sounds:?}");
    let sim = scenario.sim();
    for rider in &riders {
        assert!(
            sim.substrate
                .entities
                .get(*rider)
                .is_none_or(|rider| !rider.lifecycle.object_alive)
        );
    }
    assert!(
        riders_killed_by_crash.is_some_and(|kills| kills >= 2),
        "the flak holds both riders' kills at the Nighthawk's death"
    );
    for (id, drop, crashing, voice, impact_cue) in [
        (
            nighthawk,
            50,
            "BlackOpsDie",
            "BlackOpsVoiceDie",
            "GenAircraftCrash",
        ),
        (kirov, 18, "KirovDie", "KirovVoiceDie", "KirovCrash"),
    ] {
        let wreck = &wrecks[&id];
        println!(
            "{crashing}: crashed at {:?}, impact at {:?}, heights {:?}, impact anims {:?}",
            wreck.crash_frame, wreck.impact_frame, wreck.heights, wreck.impact_anims
        );
        let crash_frame = wreck.crash_frame.expect("shot down");
        let impact_frame = wreck.impact_frame.expect("reached the ground");
        // After the frame the latch engages, every frame falls by climb plus
        // crash until the impact takes the wreck.
        for pair in wreck.heights[1..].windows(2) {
            assert_eq!(pair[0] - pair[1], drop, "{crashing}: {:?}", wreck.heights);
        }
        assert!(wreck.heights.last().is_some_and(|&last| last <= drop));
        let heard = |name: &str| {
            sounds
                .iter()
                .filter(|(_, sound)| sound == name)
                .map(|(frame, _)| *frame)
                .collect::<Vec<_>>()
        };
        // The flak's bullet kills the wreck after the wreck's own AI ran this
        // frame; its next AI sees the latch rise.
        assert_eq!(heard(crashing), vec![crash_frame + 1], "{sounds:?}");
        assert_eq!(heard(voice), vec![crash_frame + 1], "{sounds:?}");
        assert!(heard(impact_cue).is_empty(), "no Jumpjet impact sound");
        assert!(impact_frame > crash_frame);
    }
    let impact_anims = |id: u64, set: &BTreeSet<crate::sim::intern::InternedId>| {
        wrecks[&id]
            .impact_anims
            .iter()
            .filter(|name| {
                sim.interner
                    .get(name)
                    .is_some_and(|name| set.contains(&name))
            })
            .count()
    };
    assert!(
        impact_anims(nighthawk, &explosions) >= 1,
        "the Nighthawk explodes once more"
    );
    assert!(
        impact_anims(kirov, &bomb_anims) >= 1,
        "the Kirov's BlimpBomb detonates at the impact"
    );
}
