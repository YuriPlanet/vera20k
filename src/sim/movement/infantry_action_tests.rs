use super::*;
use crate::rules::ini_parser::IniFile;
use crate::util::fixed_math::SimFixed;
use serde_json::Value;

#[test]
fn do_action_refuses_unchanged_and_uninterruptible_actions() {
    // Oracle astar_null rows: Doing 0 with request 0 leaves frame/timer alone.
    assert!(!do_action_admits(0, 0, false));
    assert!(do_action_admits(-1, 0, false));
    // Walk (3) is interruptible in the 0x7EAF7C table; Die1 (11) is not.
    assert!(do_action_admits(3, 0, false));
    assert!(!do_action_admits(11, 0, false));
    assert!(do_action_admits(11, 0, true));
    assert!(!do_action_admits(11, 11, true));
}

/// The retail `[RocketeerSequence]` (artmd.ini) with the Hover record's
/// value as given; `WALKJET` flies the Jumpjet locomotor without `JumpJet=`.
fn rocketeer_rules(hover: &str) -> RuleSet {
    let mut rules = RuleSet::from_ini(&IniFile::from_str(
        "[InfantryTypes]\n0=JUMPJET\n1=WALKJET\n\
         [JUMPJET]\nStrength=125\nJumpJet=yes\nBalloonHover=yes\nCrashable=yes\n\
         Locomotor={92612C46-F71F-11d1-AC9F-006008055BB5}\nSpeedType=Hover\n\
         MovementZone=Fly\nJumpjetSpeed=30\nJumpjetClimb=20\nJumpjetCrash=25\n\
         JumpjetHeight=500\nJumpjetNoWobbles=yes\n\
         [WALKJET]\nStrength=125\nImage=JUMPJET\nBalloonHover=yes\n\
         Locomotor={92612C46-F71F-11d1-AC9F-006008055BB5}\nSpeedType=Hover\n\
         MovementZone=Fly\nJumpjetSpeed=30\nJumpjetHeight=500\n",
    ))
    .unwrap();
    let art = IniFile::from_str(&format!(
        "[JUMPJET]\nSequence=RocketeerSequence\n\
         [RocketeerSequence]\nReady=0,1,1\nGuard=0,1,1\nProne=86,1,6\nWalk=8,6,6\n\
         FireUp=164,6,6\nDown=260,2,2\nCrawl=86,6,6\nUp=276,2,2\nFireProne=212,6,6\n\
         Idle1=56,15,0,S\nIdle2=71,15,0,E\nDie1=134,15,0\nDie2=149,15,0\nDie3=0,0,0\n\
         Die4=0,0,0\nDie5=0,0,0\nFly=292,6,6\nHover={hover}\nFireFly=370,6,6\n\
         Tumble=340,15,0\nAirDeathStart=340,8,0\nAirDeathFalling=348,1,0\n\
         AirDeathFinish=349,6,0\nParadrop=418,1,0\nCheer=419,8,0,E\nPanic=8,6,6\n"
    ));
    rules.replace_art_registry_for_test(crate::rules::art_data::ArtRegistry::from_ini(&art));
    let registry = crate::rules::infantry_sequence::parse_infantry_sequence_registry(&art);
    rules.replace_animation_sequences_for_test(
        crate::rules::animation_sequence::build_animation_sequence_catalog(&rules, Some(&registry)),
    );
    rules
}

/// A Rocketeer in the state one corpus row declares: flown by the Jumpjet
/// locomotor in `owner.phase` with its moving byte, at the row's height, with
/// its Doing, speed fraction, firing latch and Health.
fn rocketeer(input: &Value) -> (Simulation, RuleSet, u64) {
    let hover_start = input["hover_start"].as_i64().unwrap_or(292);
    let rules = rocketeer_rules(&format!("{hover_start},6,6"));
    let mut sim = Simulation::with_seed(0);
    // Original Actions/States supplies A8ED84=1000.
    sim.session.binary_frame = 1000;
    let house = sim.interner.intern("Americans");
    sim.houses.insert(
        house,
        crate::sim::house_state::HouseState::new(house, 0, None, true, 0, 10),
    );
    sim.session.game_options.game_speed = input["game_speed"].as_i64().unwrap_or(1) as i32;
    let type_id = if input["jumpjet"].as_bool().unwrap_or(true) {
        "JUMPJET"
    } else {
        "WALKJET"
    };
    let id = sim
        .construct_object_limbo_at_height(type_id, "Americans", 10, 10, 64, 0, &rules)
        .expect("rocketeer");
    let height = input["height"].as_i64().unwrap_or(500) as i32;
    let entity = sim.substrate.entities.get_mut(id).unwrap();
    entity.lifecycle.in_limbo = false;
    // jumpjet_infantry_actions.Actions supplies OWNER+74=1; Do_Action's
    // Object5F6B90 height predicate requires this independently of XYZ.
    entity.lifecycle.cell_marked = true;
    entity.health.current = input["health"].as_i64().unwrap_or(125) as _;
    entity.on_bridge = input["on_bridge"].as_bool().unwrap_or(false);
    entity.position.exact_z_leptons = Some(height);
    entity
        .mission_leaf
        .set_infantry_doing_verified(input["doing"].as_i64().unwrap_or(-1) as i32)
        .unwrap();
    entity
        .foot_speed
        .set_speed_fraction_native_bits(input["fraction"].as_f64().unwrap_or(0.0).to_bits());
    entity
        .mission_leaf
        .set_foot_firing_sequence(u8::from(input["firing"].as_bool().unwrap_or(false)));
    if entity.locomotor.is_none() {
        entity.locomotor = Some(
            crate::sim::movement::locomotor::LocomotorState::from_object_type(
                rules.object(type_id).unwrap(),
                0,
            ),
        );
    }
    // The supplied signed +0xF8 and native clock belong to the Stage owner.
    entity.install_native_stage_fixture(crate::sim::stage::StageClass::from_native_fixture(
        input["stage"].as_i64().unwrap_or(0) as i32,
        0,
        crate::sim::timer::CdTimer::from_raw(17, 91),
        92,
        1,
    ));
    let locomotor = entity.locomotor.as_mut().expect("Jumpjet locomotor");
    locomotor.altitude = SimFixed::from_num(height);
    let runtime = locomotor.jumpjet_runtime_mut().expect("Jumpjet runtime");
    *runtime = runtime
        .clone()
        .with_phase_for_test(input["owner"]["phase"].as_i64().unwrap_or(2) as i32)
        .with_moving_for_test(input["owner"]["moving"].as_bool().unwrap_or(true));
    (sim, rules, id)
}

/// Compare the retained native dwords, including a refused action's clock.
fn assert_stage(sim: &Simulation, id: u64, output: &Value, name: &str) {
    let entity = sim.substrate.entities.get(id).unwrap();
    let stage = entity.native_stage();
    let expected: Vec<i32> = output["timer"]
        .as_array()
        .unwrap()
        .iter()
        .map(|value| value.as_i64().unwrap() as i32)
        .collect();
    assert_eq!(
        stage.value(),
        output["stage"].as_i64().unwrap() as i32,
        "{name}: stage"
    );
    assert_eq!(
        [
            stage.timer().start_frame(),
            stage.timer().duration(),
            stage.rate()
        ],
        expected.as_slice(),
        "{name}: retained stage timer and rate"
    );
}

fn doing(sim: &Simulation, id: u64) -> i32 {
    sim.substrate
        .entities
        .get(id)
        .unwrap()
        .mission_leaf
        .as_infantry()
        .unwrap()
        .doing()
}

/// Parity with `tools/spatial_oracle/jumpjet_infantry_actions.json`: the
/// original Do_Action, locomotion action tail, firing arm and sequencer on a
/// Rocketeer flown by the real Jumpjet locomotor, the sequencer's AirDeath
/// arms included (AirDeathFinish's end UnInits it). Not compared here: the
/// locomotor stop of the Health-0 Stop_Driver re-entry, which needs a map
/// (`world::jumpjet_infantry_tests` compares it through the crash); and the
/// firing arm's walker FireUp path, covered by the ground firing corpus.
#[test]
fn jumpjet_infantry_actions_match_the_native_bodies() {
    let corpus: Value = serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/jumpjet_infantry_actions.json",
    ))
    .expect("corpus parses");
    let mut compared = 0;
    let mut truncated = 0;
    for (index, row) in corpus["rows"].as_array().expect("rows").iter().enumerate() {
        let input = &row["input"];
        let output = &row["output"];
        let name = format!("row {index} {input}");
        let kind = input["kind"].as_str().unwrap();
        let native_doing = output["doing"].as_i64().unwrap() as i32;
        let (mut sim, rules, id) = rocketeer(input);
        match kind {
            "do_action" => {
                let request = input["request"].as_i64().unwrap() as i32;
                let force = input["force"].as_bool().unwrap_or(false);
                let accepted = sim.infantry_do_action(id, request, force, &rules).unwrap();
                assert_eq!(accepted, output["accepted"].as_bool().unwrap(), "{name}");
            }
            "movement" => sim.infantry_movement_actions(id, &rules, None),
            "sequencer" => {
                // The object turn (`infantry_action_turn`) follows the
                // sequencer with the locomotion actions, which this row does
                // not run.
                let removed = sim.infantry_sequencer(id, &rules);
                let native_removed = output["recorded"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|event| event == "uninit");
                assert_eq!(removed, native_removed, "{name}: UnInit");
                if removed {
                    assert!(
                        sim.substrate
                            .entities
                            .get(id)
                            .is_none_or(|entity| !entity.lifecycle.object_alive),
                        "{name}: removed"
                    );
                    compared += 1;
                    continue;
                }
            }
            "firing" => {
                if !input["jumpjet"].as_bool().unwrap() {
                    continue;
                }
                sim.infantry_do_action(id, DO_FIRE_FLY, false, &rules)
                    .unwrap();
            }
            other => panic!("unknown row kind {other}"),
        }
        // A fraction within 2^-16 above 0.8 truncates to 0.8 itself
        // (`set_speed_fraction_native_bits`): the tail reads it as a hover
        // where native flies. The corpus probes that boundary on purpose.
        let fraction = input["fraction"].as_f64().unwrap_or(0.0);
        if kind == "movement" && fraction > 0.8 && fraction * 65536.0 < 52429.0 {
            if native_doing == DO_FLY {
                assert_eq!(doing(&sim, id), DO_HOVER, "{name}: truncated fraction");
                truncated += 1;
                continue;
            }
        }
        assert_eq!(doing(&sim, id), native_doing, "{name}");
        assert_stage(&sim, id, output, &name);
        compared += 1;
    }
    assert_eq!((compared, truncated), (497, 30));
}

/// The truncation of `FootClass::SetSpeedFraction @ 0x004D3710`'s stored
/// double to `SimFixed`: a Jumpjet at speed 30 braking by 3 reaches 3/30 and
/// 24/30, which the truncating native division leaves just below 0.1 and 0.8.
/// The clamp itself is checked against the original setter in
/// `components::tests::speed_fraction_setter_matches_the_original`.
#[test]
fn a_native_speed_fraction_is_truncated_below_its_thresholds() {
    let mut speed = crate::sim::components::FootSpeedState::default();
    let mut set = |bits: u64| {
        speed.set_speed_fraction_native_bits(bits);
        speed.applied_fraction().to_bits()
    };
    // 3/30 and 24/30 divided with truncation: one ulp below 0.1 and 0.8.
    let tenth = 0x3FB9_9999_9999_9999;
    let eight_tenths = 0x3FE9_9999_9999_9999;
    assert!(f64::from_bits(tenth) < 0.1 && f64::from_bits(eight_tenths) < 0.8);
    assert_eq!(set(tenth), 6553);
    assert_eq!(set(eight_tenths), 52428);
    // Neither reads as above its threshold; the next step above does.
    let mut entity =
        crate::sim::game_entity::GameEntity::test_default(1, "JUMPJET", "Americans", 0, 0);
    entity.foot_speed.set_speed_fraction_native_bits(tenth);
    assert!(!entity.foot_speed.above_tenth());
    entity
        .foot_speed
        .set_speed_fraction_native_bits(eight_tenths);
    assert!(!entity.foot_speed.above_eight_tenths());
    entity
        .foot_speed
        .set_speed_fraction_native_bits((4.0f64 / 30.0).to_bits());
    assert!(entity.foot_speed.above_tenth());
    entity
        .foot_speed
        .set_speed_fraction_native_bits((26.0f64 / 30.0).to_bits());
    assert!(entity.foot_speed.above_eight_tenths());
}

/// `takes_default_arm` against the native dispatch: the arm
/// `DoType_Sequencer` takes for each Doing, read from the tables at
/// `0x00520F1C` and `0x00520EFC` in the corpus.
#[test]
fn the_default_arm_follows_the_native_sequencer_tables() {
    let corpus: Value = serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/jumpjet_infantry_actions.json",
    ))
    .expect("corpus parses");
    let arms = corpus["sequencer_arms"].as_array().expect("arms");
    assert_eq!(arms.len(), 43);
    for arm in arms {
        let doing = arm[0].as_i64().unwrap() as i32;
        let native_default = arm[1].as_str().unwrap() == "00520CE6";
        assert_eq!(takes_default_arm(doing), native_default, "Doing {doing}");
    }
}

/// One `infantry_movement_action` consumer row's actor: a Walk infantryman
/// with all 42 sequence records of six frames, the row's Doing and prone
/// byte, Health 100 and the old stage (image frame 7, timer start 17 and
/// duration 91, rate 92) at frame 100. A motion row's head (2880, 2624, 0)
/// went through Walk Process to `0x0075BD25`, which set +0x36 alone: the
/// destination and IsMoving byte (+0x34) keep the constructor's null and
/// false, as in a row without motion.
fn walk_consumer(input: &Value) -> (Simulation, RuleSet, u64) {
    let mut rules = RuleSet::from_ini(&IniFile::from_str(
        "[InfantryTypes]\n0=E1\n[E1]\nStrength=100\nSpeed=4\n\
         Locomotor={4A582744-9839-11D1-B709-00A024DDAFD1}\nMovementZone=Infantry\n",
    ))
    .unwrap();
    let mut text = String::from("[E1]\nSequence=SuppliedSequence\n[SuppliedSequence]\n");
    for name in crate::rules::infantry_sequence::NATIVE_SEQUENCE_NAMES {
        text.push_str(&format!("{name}=0,6,0\n"));
    }
    let art = IniFile::from_str(&text);
    rules.replace_art_registry_for_test(crate::rules::art_data::ArtRegistry::from_ini(&art));
    let registry = crate::rules::infantry_sequence::parse_infantry_sequence_registry(&art);
    rules.replace_animation_sequences_for_test(
        crate::rules::animation_sequence::build_animation_sequence_catalog(&rules, Some(&registry)),
    );
    let mut sim = Simulation::with_seed(0);
    sim.session.binary_frame = 100;
    let house = sim.interner.intern("Americans");
    sim.houses.insert(
        house,
        crate::sim::house_state::HouseState::new(house, 0, None, true, 0, 10),
    );
    let id = sim
        .construct_object_limbo_at_height("E1", "Americans", 10, 10, 0, 0, &rules)
        .expect("infantryman");
    let entity = sim.substrate.entities.get_mut(id).unwrap();
    entity.lifecycle.in_limbo = false;
    entity.health.current = 100;
    entity.infantry.as_mut().unwrap().is_prone = input["prone"] == true;
    entity
        .mission_leaf
        .set_infantry_doing_verified(input["current"].as_i64().unwrap() as i32)
        .unwrap();
    entity.install_native_stage_fixture(crate::sim::stage::StageClass::from_native_fixture(
        7,
        0,
        crate::sim::timer::CdTimer::from_raw(17, 91),
        92,
        0,
    ));
    if input["motion"] == true {
        let locomotor = entity.locomotor.as_mut().expect("Walk locomotor");
        locomotor.set_step_head(Some(crate::sim::components::DriveCoord {
            x: 2880,
            y: 2624,
            z: 0,
        }));
        locomotor.begin_walk_motion();
    }
    (sim, rules, id)
}

/// The locomotion action tail of `0x00520F40` asks Walk's
/// `Is_Really_Moving_Now` (`0x0075CB20`, its +0x36), not `Is_Moving_Now`: a
/// head admitted with a null destination and a clear IsMoving byte still
/// walks or crawls. Compares every original consumer row's Doing, prone byte,
/// +0x36, image frame and stage clock.
#[test]
fn walk_locomotion_actions_match_original_consumer_rows() {
    let corpus: Vec<Value> = serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/infantry_movement_action.json",
    ))
    .unwrap();
    let mut compared = 0;
    for row in corpus.iter().filter(|row| row["input"]["consumer"] == true) {
        let name = row["input"].to_string();
        let (mut sim, rules, id) = walk_consumer(&row["input"]);
        sim.infantry_movement_actions(id, &rules, None);
        let entity = sim.substrate.entities.get(id).unwrap();
        assert_eq!(
            entity.mission_leaf.as_infantry().unwrap().doing(),
            row["doing"].as_i64().unwrap() as i32,
            "{name}: Doing"
        );
        assert_eq!(
            entity.infantry.as_ref().unwrap().is_prone,
            row["prone"] != 0,
            "{name}: prone"
        );
        assert_eq!(
            entity.locomotor.as_ref().unwrap().walk_animation_moving(),
            Some(row["motion"] != 0),
            "{name}: Walk +0x36"
        );
        let stage = entity.native_stage();
        assert_eq!(
            [
                stage.value(),
                stage.timer().start_frame(),
                stage.timer().duration(),
                stage.rate()
            ],
            [
                row["frame"].as_i64().unwrap() as i32,
                row["timer_start"].as_i64().unwrap() as i32,
                row["timer_duration"].as_i64().unwrap() as i32,
                row["timer_repeat"].as_i64().unwrap() as i32,
            ],
            "{name}: image frame and stage clock"
        );
        compared += 1;
    }
    assert_eq!(compared, 20);
}

/// Original Teleport constructor/Move_To/Stop and whole Infantry sequencer
/// controls from `jumpjet_infantry_actions.py --default-motion` (54 rows).
/// Rules/ART use the production retail readers and sequence binder. The
/// ordinary Teleport request is produced through its existing move/stop owner;
/// subcell resolution uses the same placement owner and is compared separately
/// in teleport_cell_destination_tests. This corpus bounds the subsequent
/// sequencer, rather than full Teleport lifetime or Chronosphere behaviour.
#[test]
fn retail_teleport_default_action_matches_the_native_sequencer() {
    let Some((retail_rules, retail_art)) = crate::rules::retail_ini_fixture::retail_rules_and_art()
    else {
        return;
    };
    let mut rules = RuleSet::from_ini(&retail_rules).unwrap();
    rules.install_art_data(crate::rules::art_data::ArtRegistry::from_ini(&retail_art));
    rules.bind_animation_sequences(
        &crate::rules::infantry_sequence::parse_infantry_sequence_registry(&retail_art),
    );
    let corpus: Value = serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/infantry_default_motion.json",
    ))
    .unwrap();
    // These values come from original523D00 on physical ClegSequence, not
    // from the Rust binder. Confirm the retained bank used by the sequencer.
    let records = rules.animation_sequence("CLEG").unwrap();
    for (action, native) in corpus["records"].as_array().unwrap().iter().enumerate() {
        let record = records.infantry_action(action as i32).unwrap();
        assert_eq!(record.start_frame, native[0].as_i64().unwrap() as i32);
        assert_eq!(record.frames_per_facing, native[1].as_i64().unwrap() as i32);
        assert_eq!(record.facings, native[2].as_i64().unwrap() as i32);
    }
    let rows = corpus["rows"].as_array().unwrap();
    assert_eq!(rows.len(), 54);
    let clear_costs = rules
        .terrain_rules
        .semantics_by_name("Clear")
        .unwrap()
        .speed_costs;
    for (index, row) in rows.iter().enumerate() {
        let input = &row["input"];
        let name = format!("Teleport row {index} {input}");
        let mut sim = Simulation::with_seed(31);
        sim.resolved_terrain = Some(
            crate::map::resolved_terrain::ResolvedTerrainGrid::from_cells(
                32,
                32,
                (0..32)
                    .flat_map(|y| {
                        (0..32).map(move |x| {
                            let mut cell =
                                crate::sim::world::common_raw_test_terrain_cell(x, y, 0, false);
                            cell.speed_costs = clear_costs;
                            cell.base_speed_costs = cell.speed_costs;
                            cell
                        })
                    })
                    .collect(),
            ),
        );
        sim.session.binary_frame = 1000;
        sim.session.game_options.game_speed = 1;
        let house = sim.interner.intern("Americans");
        sim.houses.insert(
            house,
            crate::sim::house_state::HouseState::new(house, 0, None, true, 0, 10),
        );
        let id = sim
            .construct_object_limbo_at_height("CLEG", "Americans", 10, 10, 0, 0, &rules)
            .unwrap();
        let actor = sim.substrate.entities.get_mut(id).unwrap();
        actor.lifecycle.in_limbo = false;
        actor.lifecycle.cell_marked = true;
        actor.health.current = 125;
        actor.position.exact_z_leptons = Some(0);
        actor.infantry.as_mut().unwrap().is_prone = input["prone"].as_bool().unwrap();
        assert_eq!(
            actor.locomotor.as_ref().unwrap().active_kind(),
            crate::rules::locomotor_type::LocomotorKind::Teleport,
        );
        let producer = input["producer"].as_str().unwrap();
        if producer != "ctor" {
            assert!(
                sim.teleport_move_to(id, (12, 10), &rules, false, None,)
                    .unwrap()
            );
        }
        let actor = sim.substrate.entities.get_mut(id).unwrap();
        if producer == "move_stop" {
            super::super::teleport_movement::teleport_stop_moving(actor);
        }
        actor
            .mission_leaf
            .set_infantry_doing_verified(input["doing"].as_i64().unwrap() as i32)
            .unwrap();
        actor
            .foot_speed
            .set_speed_fraction_native_bits(input["fraction"].as_f64().unwrap().to_bits());
        actor.install_native_stage_fixture(crate::sim::stage::StageClass::from_native_fixture(
            input["stage"].as_i64().unwrap() as i32,
            0,
            crate::sim::timer::CdTimer::from_raw(17, 91),
            92,
            1,
        ));
        let native_moving = row["before"]["request_byte"].as_u64().unwrap() != 0;
        assert_eq!(
            super::super::motion_query::is_moving(actor),
            Some(native_moving),
            "{name}: original ILocomotion+10 input"
        );
        let rng_before = sim.rng_state();
        assert!(
            !sim.infantry_sequencer(id, &rules),
            "{name}: retained owner"
        );
        assert_eq!(
            doing(&sim, id),
            row["after"]["doing"].as_i64().unwrap() as i32,
            "{name}"
        );
        assert_stage(&sim, id, &row["after"], &name);
        assert_eq!(
            sim.rng_state(),
            rng_before,
            "{name}: all three complete RNG states"
        );
        for stream in ["main", "scenario", "mapgen"] {
            assert_eq!(row["rng"][stream]["unchanged"], true);
            assert_eq!(
                row["rng"][stream]["before_hex"],
                row["rng"][stream]["after_hex"]
            );
        }
    }
}
