//! Fire-start owner, native prefix witnesses and connected frame regressions.
use super::*;
use crate::sim::movement::FacingClass;
use crate::sim::snapshot::GameSnapshot;
use crate::sim::world::Simulation;

fn pair() -> EntityStore {
    let mut store = EntityStore::new();
    let mut firer = make_infantry_entity(1, "E1", 5, 5, 125);
    firer.facing = 0x81;
    firer.body_facing = Some(FacingClass::new(0x8123, 5));
    store.insert(firer);
    store.insert(make_infantry_entity(2, "E2", 8, 5, 125));
    store
}

fn visit(store: &mut EntityStore, rules: &RuleSet, frame: u32) -> CombatTickResult {
    tick_combat(
        store,
        &mut OccupancyGrid::new(),
        rules,
        &mut test_interner(),
        u64::from(frame),
        67,
        frame,
        &mut SimRng::new(1),
    )
}

#[test]
fn infantry_orders_leave_facing_to_fire_start() {
    let rules = infantry_fire_frame_rules();
    for order in 0..2 {
        let mut store = pair();
        let interner = test_interner();
        match order {
            0 => assert!(issue_attack_command(
                &mut store,
                1,
                2,
                Some(&rules),
                &interner
            )),
            _ => assert!(issue_attack_cell_command(
                &mut store,
                1,
                8,
                5,
                Some(&rules),
                &interner
            )),
        }
        let entity = store.get(1).unwrap();
        assert_eq!(entity.facing, 0x81, "order {order}");
        assert_eq!(entity.body_facing.unwrap().current(100), 0x8123);
    }
}

#[test]
fn infantry_fire_start_heading_matches_original_code_vectors() {
    let rows: Vec<serde_json::Value> = serde_json::from_str(include_str!(
        "../../../tools/spatial_oracle/infantry_fire_start.json"
    ))
    .unwrap();
    assert_eq!(rows.len(), 59);
    let rules = infantry_fire_frame_rules();
    let mut compared = 0;
    // Compare the shared heading writer for standing, prone and deployed starts.
    // Secondary-action admission and the supplied native legality results are
    // outside this comparison; the corpus retains them as explicit witnesses.
    for row in rows.iter().filter(|r| {
        r["fire_error"] == 0
            && r["pending"] == 0
            && r["has_target"] == true
            && r["weapon_slot"] == 0
            && r["target_kind"] == "object"
    }) {
        let mut store = pair();
        for (id, key) in [(1, "source"), (2, "target")] {
            let pos = &mut store.get_mut(id).unwrap().position;
            let x = row[key][0].as_i64().unwrap() as i32;
            let y = row[key][1].as_i64().unwrap() as i32;
            pos.rx = (x / 256) as u16;
            pos.ry = (y / 256) as u16;
            pos.sub_x = SimFixed::from_num(x % 256);
            pos.sub_y = SimFixed::from_num(y % 256);
        }
        let firer = store.get_mut(1).unwrap();
        let mut body = FacingClass::new(
            row["initial_previous"].as_u64().unwrap() as u16,
            row["rot"].as_u64().unwrap() as u8,
        );
        if row["initial_duration"] != 0 {
            body.set(
                row["initial_facing"].as_u64().unwrap() as u16,
                row["initial_start"].as_u64().unwrap() as u32,
            );
        }
        firer.facing = (body.current(100) >> 8) as u8;
        firer.body_facing = Some(body);
        firer.infantry.as_mut().unwrap().is_prone = row["prone"] == 1;
        if row["deployed"] == true {
            firer.deploy_state = Some(crate::sim::deploy::DeployPhase::Deployed);
            firer.animation = Some(Animation::new(SequenceKind::Deployed));
        }
        firer.attack_target = Some(AttackTarget::new(2));
        let result = visit(&mut store, &rules, 100);
        let firer = store.get(1).unwrap();
        let expected = row["facing"].as_u64().unwrap() as u16;
        assert_eq!(
            firer.body_facing.unwrap().current(100),
            expected,
            "{}",
            row["name"]
        );
        assert_eq!(firer.facing, (expected >> 8) as u8);
        assert!(
            firer
                .attack_target
                .as_ref()
                .unwrap()
                .pending_infantry_fire
                .is_some()
        );
        assert!(result.consequences.fire_events().is_empty());
        compared += 1;
    }
    assert_eq!(compared, 31);
}

#[test]
fn infantry_refused_or_reloading_does_not_snap() {
    let rules = infantry_fire_frame_rules();
    for out_of_range in [false, true] {
        let mut store = pair();
        let attack = AttackTarget::new(2);
        if out_of_range {
            store.get_mut(2).unwrap().position.rx = 50;
        } else {
            store.get_mut(1).unwrap().rearm_timer = crate::sim::timer::CdTimer::started(100, 20);
        }
        store.get_mut(1).unwrap().attack_target = Some(attack);
        assert!(
            visit(&mut store, &rules, 100)
                .consequences
                .fire_events()
                .is_empty()
        );
        assert_eq!(
            store.get(1).unwrap().body_facing.unwrap().current(100),
            0x8123
        );
    }
}

#[test]
fn infantry_fire_speed_refusal_matches_original_threshold() {
    let rows: Vec<serde_json::Value> = serde_json::from_str(include_str!(
        "../../../tools/spatial_oracle/infantry_fire_speed.json"
    ))
    .unwrap();
    let rules = infantry_fire_frame_rules();
    let mut compared = 0;
    for row in rows.iter().filter(|row| !row["fixed_bits"].is_null()) {
        let mut store = pair();
        let firer = store.get_mut(1).unwrap();
        firer.foot_speed.applied_fraction =
            SimFixed::from_bits(row["fixed_bits"].as_i64().unwrap() as i32);
        firer.attack_target = Some(AttackTarget::new(2));
        let result = visit(&mut store, &rules, 100);
        let firer = store.get(1).unwrap();
        let refused = row["refused"].as_bool().unwrap();
        assert_eq!(
            firer
                .attack_target
                .as_ref()
                .unwrap()
                .pending_infantry_fire
                .is_none(),
            refused,
            "{}",
            row["name"]
        );
        assert_eq!(
            firer.body_facing.unwrap().current(100),
            if refused { 0x8123 } else { 0x3FFF },
            "{}",
            row["name"]
        );
        assert!(result.consequences.fire_events().is_empty());
        compared += 1;
    }
    assert_eq!(compared, 8);
}

#[test]
fn infantry_speed_refusal_at_fire_frame_clears_pending_sequence() {
    let rules = infantry_fire_frame_rules();
    let mut store = pair();
    store.get_mut(1).unwrap().attack_target = Some(AttackTarget::new(2));
    visit(&mut store, &rules, 100);
    let body = store.get(1).unwrap().body_facing;
    set_anim_frame(&mut store, 1, 2);
    let firer = store.get_mut(1).unwrap();
    assert!(
        firer
            .attack_target
            .as_ref()
            .unwrap()
            .pending_infantry_fire
            .is_some()
    );
    // Isolate the live Foot predicate from the older movement-target shortcut.
    assert!(firer.movement_target.is_none());
    firer.foot_speed.applied_fraction = SimFixed::ONE;
    let result = visit(&mut store, &rules, 101);
    let firer = store.get(1).unwrap();
    assert!(result.consequences.fire_events().is_empty());
    assert!(
        firer
            .attack_target
            .as_ref()
            .unwrap()
            .pending_infantry_fire
            .is_none()
    );
    assert_eq!(
        firer.animation.as_ref().unwrap().sequence,
        SequenceKind::Stand
    );
    assert_eq!(firer.body_facing, body);
}

/// A Rocketeer (`JumpJet=` on the Jumpjet locomotor, the retail
/// `[RocketeerSequence]`) hovering at 500 over (5, 5), firing at a GI three
/// cells off.
fn rocketeer_pair() -> (EntityStore, RuleSet) {
    let mut rules = RuleSet::from_ini(&IniFile::from_str(
        "[InfantryTypes]\n0=JJ\n1=E2\n\
         [JJ]\nStrength=125\nArmor=flak\nSpeed=9\nImage=ROCK\nPrimary=M60\nJumpJet=yes\n\
         BalloonHover=yes\nLocomotor={92612C46-F71F-11d1-AC9F-006008055BB5}\n\
         SpeedType=Hover\nMovementZone=Fly\nJumpjetSpeed=30\nJumpjetHeight=500\n\
         [E2]\nStrength=125\nArmor=flak\nSpeed=4\n\
         [M60]\nDamage=25\nROF=20\nRange=5\nWarhead=SA\n\
         [SA]\nVerses=100%,100%,100%,90%,70%,0%,100%,25%,25%,0%,0%\n",
    ))
    .unwrap();
    let art_ini = IniFile::from_str(
        "[ROCK]\nSequence=RocketeerSequence\nFireUp=2\n\
         [RocketeerSequence]\nReady=0,1,1\nGuard=0,1,1\nWalk=8,6,6\nFireUp=164,6,6\n\
         Fly=292,6,6\nHover=292,6,6\nFireFly=370,6,6\n",
    );
    rules.merge_art_data(&crate::rules::art_data::ArtRegistry::from_ini(&art_ini));
    let registry = crate::rules::infantry_sequence::parse_infantry_sequence_registry(&art_ini);
    rules.replace_animation_sequences_for_test(
        crate::rules::animation_sequence::build_animation_sequence_catalog(&rules, Some(&registry)),
    );
    let mut store = EntityStore::new();
    let mut firer = make_infantry_entity(1, "JJ", 5, 5, 125);
    firer.body_facing = Some(FacingClass::new(0x4000, 127));
    firer.position.exact_z_leptons = Some(500);
    let mut locomotor = crate::sim::movement::locomotor::LocomotorState::from_object_type(
        rules.object("JJ").unwrap(),
        0,
    );
    locomotor.altitude = SimFixed::from_num(500);
    let runtime = locomotor.jumpjet_runtime_mut().unwrap();
    runtime.phase = crate::sim::movement::jumpjet_flight::STATE_HOLD;
    runtime.moving = true;
    firer.locomotor = Some(locomotor);
    store.insert(firer);
    store.insert(make_infantry_entity(2, "E2", 8, 5, 125));
    (store, rules)
}

/// A Rocketeer's FireFly refused at its fire frame (here by I4, the speed
/// gate) takes the idle Do_Action (`0x00520A03..0x00520A51`): Ready, which
/// is Hover in the air, and its Hover sequence, not the walker's Stand.
#[test]
fn a_rocketeer_refused_at_its_fire_frame_hovers() {
    use crate::sim::movement::infantry_action::{DO_FIRE_FLY, DO_HOVER};
    let (mut store, rules) = rocketeer_pair();
    store.get_mut(1).unwrap().attack_target = Some(AttackTarget::new(2));
    visit(&mut store, &rules, 100);
    let doing = |store: &EntityStore| {
        store
            .get(1)
            .unwrap()
            .mission_leaf
            .as_infantry()
            .unwrap()
            .doing()
    };
    assert_eq!(doing(&store), DO_FIRE_FLY);
    assert_eq!(
        store.get(1).unwrap().animation.as_ref().unwrap().sequence,
        SequenceKind::FireFly
    );
    set_anim_frame(&mut store, 1, 2);
    store.get_mut(1).unwrap().foot_speed.applied_fraction = SimFixed::ONE;
    let result = visit(&mut store, &rules, 101);
    assert!(result.consequences.fire_events().is_empty());
    let firer = store.get(1).unwrap();
    assert!(
        firer
            .attack_target
            .as_ref()
            .unwrap()
            .pending_infantry_fire
            .is_none()
    );
    assert_eq!(doing(&store), DO_HOVER);
    assert_eq!(
        firer.animation.as_ref().unwrap().sequence,
        SequenceKind::Hover
    );
}

#[test]
fn pending_sequence_keeps_start_facing_when_target_moves() {
    let rules = infantry_fire_frame_rules();
    let mut store = pair();
    store.get_mut(1).unwrap().attack_target = Some(AttackTarget::new(2));
    visit(&mut store, &rules, 100);
    let facing = store.get(1).unwrap().body_facing.unwrap();
    store.get_mut(2).unwrap().position.rx = 2;
    set_anim_frame(&mut store, 1, 2);
    let result = visit(&mut store, &rules, 101);
    assert_eq!(result.consequences.fire_events().len(), 1);
    assert_eq!(store.get(1).unwrap().body_facing, Some(facing));
    assert_eq!(
        result.consequences.fire_events()[0].facing,
        (facing.current(101) >> 8) as u8
    );
    assert_eq!(store.get(2).unwrap().health.current, 100);
}

fn production_rules(fire_up: u32) -> RuleSet {
    // The receiver-only fixture has the default Sight=1. Give this full-frame
    // scenario the retail GI sight/locomotor inputs so its target three cells
    // away is visible through the ordinary fog update and firing gates.
    let mut rules = RuleSet::from_ini(&IniFile::from_str(
        "[InfantryTypes]\n0=E1\n1=E2\n[BuildingTypes]\n0=TARGET\n\
         [E1]\nStrength=125\nArmor=flak\nSpeed=4\nSight=5\nImage=GI\nPrimary=M60\n\
         Locomotor={4A582744-9839-11d1-B709-00A024DDAFD1}\nMovementZone=Infantry\n\
         [E2]\nStrength=125\nArmor=flak\nSpeed=4\nSight=5\n\
         [TARGET]\nStrength=125\nArmor=flak\nFoundation=2x3\n\
         [M60]\nDamage=25\nROF=20\nRange=5\nWarhead=SA\n\
         [SA]\nVerses=100%,100%,100%,90%,70%,0%,100%,25%,25%,0%,0%\n",
    ))
    .unwrap();
    let art_ini = IniFile::from_str(&format!(
        "[GI]\nFireUp={fire_up}\nPrimaryFireFLH=80,20,40\nSequence=GISequence\n\
         [GISequence]\nReady=0,1,1\nGuard=0,1,1\nFireUp=8,6,6\n",
    ));
    rules.merge_art_data(&crate::rules::art_data::ArtRegistry::from_ini(&art_ini));
    rules.bind_animation_sequences(
        &crate::rules::infantry_sequence::parse_infantry_sequence_registry(&art_ini),
    );
    rules
}

fn production_pair(rules: &RuleSet) -> (Simulation, u64, u64) {
    let mut sim = Simulation::new();
    sim.install_resolved_terrain_for_new_map(flat_level_zero_terrain(16, 16));
    let firer = sim
        .spawn_object("E1", "Americans", 5, 5, 0, rules, &BTreeMap::new())
        .unwrap();
    let target = sim
        .spawn_object("E2", "Russians", 8, 5, 0, rules, &BTreeMap::new())
        .unwrap();
    assert!(issue_attack_command(
        &mut sim.substrate.entities,
        firer,
        target,
        Some(rules),
        &sim.interner
    ));
    (sim, firer, target)
}

fn restore_production_pair(sim: &Simulation) -> Simulation {
    let bytes = GameSnapshot::save(sim, 0, 0, "fire_facing", 0);
    let mut restored = GameSnapshot::load(&bytes).unwrap().sim;
    restored.retain_in_scenario_process_state_from(sim);
    restored.restore_after_snapshot_load().unwrap();
    // Terrain is rebuilt from map inputs by the load workflow, not serialized
    // with the entities. Damage must see the same subcell/ground context.
    restored.rebuild_caches_after_load(
        flat_level_zero_terrain(16, 16),
        crate::sim::pathfinding::terrain_speed::TerrainSpeedConfig::default(),
        Vec::new(),
        Vec::new(),
    );
    restored
}

#[test]
fn production_zero_delay_shot_and_restore_use_new_heading() {
    let rules = production_rules(0);
    let (mut sim, firer, target) = production_pair(&rules);
    let source = &sim.substrate.entities.get(firer).unwrap().position;
    let source_x = i32::from(source.rx) * 256 + source.sub_x.to_num::<i32>();
    let source_y = i32::from(source.ry) * 256 + source.sub_y.to_num::<i32>();
    sim.advance_tick(&[], Some(&rules), &BTreeMap::new(), None, None, 67);
    let entity = sim.substrate.entities.get(firer).unwrap();
    assert!(sim.fog.is_cell_visible(entity.owner(), 8, 5));
    let facing = entity
        .body_facing
        .expect("retained fire-facing owner")
        .current(sim.session.binary_frame);
    assert_eq!(facing, 0x3FFF, "native eastward direction");
    let event = sim
        .fire_events
        .iter()
        .find(|event| event.attacker_id == firer)
        .expect("production shot");
    assert_eq!(event.facing, 63);
    assert_eq!(event.origin_snapshot.facing, 63);
    let delta = crate::util::flh_transform::native_flh_world_delta(
        80,
        20,
        40,
        0,
        crate::util::flh_transform::FlhFacings {
            aim: facing,
            body: facing,
            matrix: facing,
        },
        0,
    )
    .expect("bounded FLH");
    assert_eq!(event.fire_coord.x, source_x + delta.0);
    assert_eq!(event.fire_coord.y, source_y + delta.1);
    assert_eq!(
        sim.substrate.entities.get(target).unwrap().health.current,
        100
    );

    // Body FacingClass is already serialized/hashed; no second stored heading.
    let mut restored = restore_production_pair(&sim);
    // Native Scenario load reseeds its RNG; compare against that load state.
    sim.scenario_rng = SimRng::new(0);
    assert_eq!(sim.state_hash(), restored.state_hash());
    for _ in 0..4 {
        sim.advance_tick(&[], Some(&rules), &BTreeMap::new(), None, None, 67);
        restored.advance_tick(&[], Some(&rules), &BTreeMap::new(), None, None, 67);
        assert_eq!(sim.state_hash(), restored.state_hash());
    }
}

#[test]
fn cell_and_building_fire_headings_match_original_coordinate_getters() {
    let rows: Vec<serde_json::Value> = serde_json::from_str(include_str!(
        "../../../tools/spatial_oracle/infantry_fire_start.json"
    ))
    .unwrap();
    let rules = production_rules(2);
    let mut compared = 0;
    for row in rows.iter().filter(|row| row["target_kind"] != "object") {
        let mut store = EntityStore::new();
        let mut firer = make_infantry_entity(1, "E1", 5, 5, 125);
        firer.position.sub_x = SimFixed::from_num(row["source"][0].as_i64().unwrap() as i32 % 256);
        firer.position.sub_y = SimFixed::from_num(row["source"][1].as_i64().unwrap() as i32 % 256);
        firer.attack_target = Some(if row["target_kind"] == "cell" {
            AttackTarget::for_cell(8, 5)
        } else {
            store.insert(make_structure_entity(2, "TARGET", 8, 5, 125, 125));
            AttackTarget::new(2)
        });
        store.insert(firer);
        let result = visit(&mut store, &rules, 100);
        assert!(result.consequences.fire_events().is_empty());
        assert_eq!(
            u64::from(store.get(1).unwrap().body_facing.unwrap().current(100)),
            row["facing"].as_u64().unwrap(),
            "{}",
            row["name"]
        );
        compared += 1;
    }
    assert_eq!(compared, 2);
}

#[test]
fn production_pending_fire_restores_heading_and_reaches_emission() {
    let rules = production_rules(2);
    let (mut sim, firer, target) = production_pair(&rules);
    sim.advance_tick(&[], Some(&rules), &BTreeMap::new(), None, None, 67);
    let entity = sim.substrate.entities.get(firer).unwrap();
    assert!(
        entity
            .attack_target
            .as_ref()
            .unwrap()
            .pending_infantry_fire
            .is_some()
    );
    let body = entity
        .body_facing
        .expect("facing written at sequence start");
    assert!(sim.fire_events.is_empty());
    let mut restored = restore_production_pair(&sim);
    sim.scenario_rng = SimRng::new(0); // Native Scenario load reseed.
    assert_eq!(sim.state_hash(), restored.state_hash());
    for _ in 0..24 {
        sim.advance_tick(&[], Some(&rules), &BTreeMap::new(), None, None, 67);
        restored.advance_tick(&[], Some(&rules), &BTreeMap::new(), None, None, 67);
        assert_eq!(sim.state_hash(), restored.state_hash());
        assert_eq!(
            restored.substrate.entities.get(firer).unwrap().body_facing,
            Some(body)
        );
        if restored
            .fire_events
            .iter()
            .any(|event| event.attacker_id == firer)
        {
            assert_eq!(
                restored
                    .substrate
                    .entities
                    .get(target)
                    .unwrap()
                    .health
                    .current,
                100
            );
            return;
        }
    }
    panic!("restored production fire sequence did not emit");
}

#[test]
fn production_attack_during_paid_walk_step_waits_before_turning_and_firing() {
    production_attack_during_paid_walk_step_case(false);
}

#[test]
fn live_speed_crate_survives_walk_attack_and_production_save_restore() {
    production_attack_during_paid_walk_step_case(true);
}

fn production_attack_during_paid_walk_step_case(boosted: bool) {
    use crate::sim::command::Command;

    let rules = production_rules(0);
    let (mut sim, firer, target) = production_pair(&rules);
    assert!(sim.rebuild_dynamic_navigation(&rules));
    let command = |sim: &mut Simulation, command: Command| {
        let grid = sim.path_grid_snapshot();
        assert!(sim.apply_command_with_overlays(
            "Americans",
            &command,
            Some(&rules),
            grid.as_deref(),
            &BTreeMap::new(),
            None,
        ));
    };
    let frame = |sim: &mut Simulation| {
        let grid = sim.path_grid_snapshot();
        sim.advance_tick(
            &[],
            Some(&rules),
            &BTreeMap::new(),
            grid.as_deref(),
            None,
            67,
        );
    };
    // Walk south, then attack the enemy east of us. The turn is observable.
    command(
        &mut sim,
        Command::Move {
            entity_id: firer,
            target_rx: 5,
            target_ry: 10,
            queue: false,
            group_id: None,
        },
    );
    for _ in 0..50 {
        frame(&mut sim);
        if sim
            .substrate
            .entities
            .get(firer)
            .unwrap()
            .locomotor
            .as_ref()
            .unwrap()
            .step_head()
            .is_some()
        {
            break;
        }
    }
    let entity = sim.substrate.entities.get(firer).unwrap();
    let head = entity
        .locomotor
        .as_ref()
        .unwrap()
        .step_head()
        .expect("accepted Walk step");
    let body = entity.body_facing.expect("Walk's heading owner");
    assert_eq!(entity.foot_speed.applied_fraction, SimFixed::ONE);
    if boosted {
        assert!(
            sim.substrate
                .entities
                .get_mut(firer)
                .unwrap()
                .foot_speed
                .accept_speed_crate(crate::util::native_x87::NativeF64Bits::from_bits(
                    1.2_f64.to_bits()
                ))
        );
    }
    command(
        &mut sim,
        Command::Attack {
            attacker_id: firer,
            target_id: target,
        },
    );
    let entity = sim.substrate.entities.get(firer).unwrap();
    assert_eq!(entity.locomotor.as_ref().unwrap().step_head(), Some(head));
    assert_eq!(
        entity.body_facing,
        Some(body),
        "order keeps the paid-step heading"
    );
    assert_eq!(entity.foot_speed.applied_fraction, SimFixed::ONE);
    let mut resumed = restore_production_pair(&sim);
    sim.scenario_rng = SimRng::new(0); // Native Scenario load reseeds this stream.
    assert_eq!(sim.state_hash(), resumed.state_hash());
    let mut refused_frames = 0;
    for _ in 0..100 {
        let position = &sim.substrate.entities.get(firer).unwrap().position;
        let current = crate::sim::movement::ground_pose::position_world_xy(position);
        let step_heading = crate::util::direction_tables::facing16_from_delta(
            head.x - current[0],
            head.y - current[1],
        );
        frame(&mut sim);
        frame(&mut resumed);
        assert_eq!(
            sim.state_hash(),
            resumed.state_hash(),
            "paid Walk save/restore continuation"
        );
        let entity = sim.substrate.entities.get(firer).unwrap();
        let fired = sim
            .fire_events
            .iter()
            .any(|event| event.attacker_id == firer);
        if entity.foot_speed.applied_fraction > SimFixed::ONE / SimFixed::from_num(10) {
            if boosted {
                assert_eq!(
                    entity.foot_speed.cached_current_speed, 11,
                    "live native crate speed"
                );
            }
            assert!(!fired, "a retained paid step cannot fire");
            assert_eq!(
                entity
                    .body_facing
                    .unwrap()
                    .current(sim.session.binary_frame),
                step_heading,
                "movement may correct toward its head; fire must not turn toward the enemy"
            );
            assert!(
                entity
                    .attack_target
                    .as_ref()
                    .unwrap()
                    .pending_infantry_fire
                    .is_none()
            );
            refused_frames += 1;
        } else if fired {
            assert!(
                refused_frames > 0,
                "exercise a frame with the step still live"
            );
            assert!(entity.locomotor.as_ref().unwrap().step_head().is_none());
            assert_ne!(
                entity.body_facing,
                Some(body),
                "face target at accepted fire start"
            );
            assert_eq!(
                sim.substrate.entities.get(target).unwrap().health.current,
                100
            );
            return;
        }
    }
    panic!("paid-step completion never reached a production shot");
}
