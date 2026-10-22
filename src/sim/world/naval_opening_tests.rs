//! Grand_Opening44699A -> SetRallyPoint443860 -> ARCHIVE1E -> naval ExitObject.
//! These production regressions exercise the existing search and event owners;
//! supplied arena geometry is not a native whole-match comparison.

use super::Simulation;
use crate::rules::retail_ini_fixture::retail_battle_rules_for_map;
use crate::rules::ruleset::RuleSet;
use crate::sim::combat::TargetKind;
use crate::sim::command::Command;
use crate::sim::world::{FrameEffects, PlacementEvidence};

fn yard_fixture(rules: &RuleSet, name: &str, water: bool) -> (Simulation, u64) {
    let mut sim = if water {
        crate::sim::arena_fixture::rival_water_arena(rules, 17, 32)
    } else {
        crate::sim::arena_fixture::rival_arena(rules, 17)
    };
    sim.intern_rule_type_ids(rules);
    sim.resolve_type_handles(rules);
    let id = sim
        .construct_object_limbo_at_height(
            name,
            "Americans",
            12,
            12,
            0,
            0,
            rules,
            FrameEffects::default(),
        )
        .unwrap();
    assert!(
        sim.reveal_constructed_object_at_height(
            id,
            12,
            12,
            0,
            0,
            PlacementEvidence::EvaluateMark,
            rules,
            FrameEffects::default(),
        )
        .is_some()
    );
    sim.substrate
        .entities
        .get_mut(id)
        .unwrap()
        .finish_building_construction_for_test();
    assert_eq!(
        sim.substrate.entities.get(id).unwrap().archive_target(),
        None
    );
    assert!(sim.pending_command_snapshot().is_empty());
    (sim, id)
}

fn open(sim: &mut Simulation, id: u64, captured: bool, rules: &RuleSet) {
    sim.grand_opening(id, captured, false, rules, None, FrameEffects::default());
}

fn dispatch(sim: &mut Simulation, rules: &RuleSet) {
    let commands = sim.take_frame_commands_for_test();
    for command in commands {
        let owner = sim.interner.resolve(command.owner).to_owned();
        assert!(sim.apply_command(&owner, &command.payload, Some(rules)));
    }
}

#[test]
fn retail_shipyard_opening_queues_a_quiet_default_rally_until_the_command_tail() {
    let Some(retail) = retail_battle_rules_for_map("Hills.mmx") else {
        return;
    };
    let rules = &retail.rules;
    for name in ["GAYARD", "NAYARD", "YAYARD"] {
        let object = rules.object(name).unwrap();
        assert!(object.weapons_factory && object.naval);
        let (mut sim, id) = yard_fixture(rules, name, true);
        let sounds = sim.sound_events.len();
        open(&mut sim, id, false, rules);
        let pending = sim.pending_command_snapshot();
        assert_eq!(pending.len(), 1, "{name}: opening publishes ARCHIVE");
        let Command::SetRally {
            rx,
            ry,
            producer_ids,
        } = &pending[0].payload
        else {
            panic!("opening must use the shared rally event")
        };
        let destination = (*rx, *ry);
        assert_eq!(producer_ids, &[id]);
        assert_eq!(
            pending[0].owner,
            sim.substrate.entities.get(id).unwrap().owner()
        );
        assert_eq!(pending[0].execute_tick, sim.session.tick() + 1);
        assert_eq!(
            sim.substrate.entities.get(id).unwrap().archive_target(),
            None
        );
        assert_eq!(sim.sound_events.len(), sounds, "announce=false");
        dispatch(&mut sim, rules);
        assert_eq!(
            sim.substrate.entities.get(id).unwrap().rally_cell(),
            Some(destination)
        );
        assert!(sim.pending_command_snapshot().is_empty());
        open(&mut sim, id, false, rules);
        assert!(
            sim.pending_command_snapshot().is_empty(),
            "ordinary repeated opening returns early"
        );
        open(&mut sim, id, true, rules);
        assert!(
            sim.pending_command_snapshot().is_empty(),
            "capture retains an existing rally"
        );
    }
}

#[test]
fn capture_without_a_rally_generates_one_but_land_factories_do_not() {
    let Some(retail) = retail_battle_rules_for_map("Hills.mmx") else {
        return;
    };
    let rules = &retail.rules;
    let (mut sim, id) = yard_fixture(rules, "GAYARD", true);
    sim.substrate
        .entities
        .get_mut(id)
        .unwrap()
        .building_art
        .set_actually_placed_for_test(true);
    open(&mut sim, id, true, rules);
    assert_eq!(sim.pending_command_snapshot().len(), 1);
    let (mut sim, id) = yard_fixture(rules, "GAWEAP", false);
    open(&mut sim, id, false, rules);
    assert!(sim.pending_command_snapshot().is_empty());
}

#[test]
fn a_captured_shipyard_replaces_the_previous_owners_rally_through_the_event_tail() {
    let Some(retail) = retail_battle_rules_for_map("Hills.mmx") else {
        return;
    };
    let rules = &retail.rules;
    let (mut sim, id) = yard_fixture(rules, "GAYARD", true);
    open(&mut sim, id, false, rules);
    dispatch(&mut sim, rules);
    assert!(sim.apply_command(
        "Americans",
        &Command::SetRally {
            rx: 30,
            ry: 30,
            producer_ids: vec![id],
        },
        Some(rules)
    ));
    let owner = sim.intern("French");
    sim.houses
        .register(crate::sim::house_state::HouseState::new(
            owner, 0, None, true, 0, 10,
        ));
    sim.change_owner_with_rules(id, owner, rules, None, FrameEffects::default());
    assert_eq!(sim.entities().get(id).unwrap().owner(), owner);
    assert_eq!(sim.entities().get(id).unwrap().archive_target(), None);
    assert_eq!(sim.pending_command_snapshot().len(), 1);
    assert_eq!(sim.pending_command_snapshot()[0].owner, owner);
    dispatch(&mut sim, rules);
    let rally = sim.entities().get(id).unwrap().rally_cell();
    assert!(rally.is_some());
    assert_ne!(rally, Some((30, 30)));
}

#[test]
fn an_accepted_default_survives_capture_when_the_replacement_event_is_refused() {
    let Some(retail) = retail_battle_rules_for_map("Hills.mmx") else {
        return;
    };
    let rules = &retail.rules;
    let (mut sim, id) = yard_fixture(rules, "GAYARD", true);
    let old_owner = sim.entities().get(id).unwrap().owner();
    for _ in 0..127 {
        assert!(
            sim.issue_local_command(crate::sim::command::CommandEnvelope::new(
                old_owner,
                1,
                Command::Stop { entity_id: 999 },
            ))
        );
    }
    open(&mut sim, id, false, rules);
    let accepted = sim.pending_command_snapshot();
    assert_eq!(accepted.len(), 128);
    let expected = match accepted.last().unwrap().payload {
        Command::SetRally { rx, ry, .. } => Some((rx, ry)),
        _ => panic!("the last slot belongs to the default rally"),
    };
    let new_owner = sim.intern("French");
    sim.houses
        .register(crate::sim::house_state::HouseState::new(
            new_owner, 0, None, true, 0, 10,
        ));
    sim.change_owner_with_rules(id, new_owner, rules, None, FrameEffects::default());
    assert_eq!(
        sim.pending_command_snapshot(),
        accepted,
        "OutList refuses the replacement"
    );
    assert_eq!(sim.entities().get(id).unwrap().archive_target(), None);
    for command in sim.take_frame_commands_for_test() {
        let owner = sim.interner.resolve(command.owner).to_owned();
        let _ = sim.apply_command(&owner, &command.payload, Some(rules));
    }
    assert_eq!(sim.entities().get(id).unwrap().owner(), new_owner);
    assert_eq!(sim.entities().get(id).unwrap().rally_cell(), expected);
}

#[test]
fn naval_default_rally_obeys_outlist_saturation_and_recorded_replay() {
    let Some(retail) = retail_battle_rules_for_map("Hills.mmx") else {
        return;
    };
    let rules = &retail.rules;
    let (mut sim, id) = yard_fixture(rules, "GAYARD", true);
    let owner = sim.substrate.entities.get(id).unwrap().owner();
    for _ in 0..128 {
        assert!(
            sim.issue_local_command(crate::sim::command::CommandEnvelope::new(
                owner,
                1,
                Command::Stop { entity_id: 999 },
            ))
        );
    }
    open(&mut sim, id, false, rules);
    assert_eq!(sim.pending_command_snapshot().len(), 128);
    assert_eq!(
        sim.substrate.entities.get(id).unwrap().archive_target(),
        None
    );
    let (mut live, id) = yard_fixture(rules, "GAYARD", true);
    open(&mut live, id, false, rules);
    let recorded = live.take_frame_commands_for_test();
    assert_eq!(recorded.len(), 1);
    let (mut replay, replay_id) = yard_fixture(rules, "GAYARD", true);
    assert_eq!(id, replay_id);
    let admitted = replay.take_due_replay_commands(recorded.clone());
    open(&mut replay, replay_id, false, rules);
    assert!(
        replay.pending_command_snapshot().is_empty(),
        "replay must not regenerate accepted or dropped records"
    );
    for command in admitted {
        let owner = replay.interner.resolve(command.owner).to_owned();
        assert!(replay.apply_command(&owner, &command.payload, Some(rules)));
    }
    assert!(
        replay
            .substrate
            .entities
            .get(id)
            .unwrap()
            .rally_cell()
            .is_some()
    );
}

#[test]
fn default_rally_reaches_the_first_paid_destroyer_exit_without_an_explicit_click() {
    use crate::sim::components::NavTargetRef;
    use crate::sim::mission::{MissionId, MissionType};
    use crate::sim::production::{self, ProductionCategory};

    let Some(retail) = retail_battle_rules_for_map("Hills.mmx") else {
        return;
    };
    let rules = &retail.rules;
    let (mut sim, id) = yard_fixture(rules, "GAYARD", true);
    open(&mut sim, id, false, rules);
    dispatch(&mut sim, rules);
    let destination = sim
        .substrate
        .entities
        .get(id)
        .unwrap()
        .rally_cell()
        .unwrap();
    let owner = sim.substrate.entities.get(id).unwrap().owner();
    sim.houses
        .get_mut(&owner)
        .unwrap()
        .economy_mut()
        .add_credits(50_000);
    assert!(production::enqueue_by_type(
        &mut sim,
        rules,
        "Americans",
        "DEST",
        FrameEffects::default(),
        false,
    ));
    let ship = sim
        .production
        .factories()
        .view(owner, ProductionCategory::Ship)
        .unwrap()
        .object
        .unwrap()
        .entity_id
        .unwrap();
    // Completion is a supplied prior; the real paid factory owns the identity,
    // and production publication, PLACE and ExitObject all run for real.
    assert!(
        sim.production
            .factories_mut()
            .test_arm_ready(owner, ProductionCategory::Ship)
    );
    assert!(production::dispatch_production_changes_for_tests(
        &mut sim, rules, None
    ));
    let ship = sim.substrate.entities.get(ship).unwrap();
    assert!(!ship.lifecycle.in_limbo());
    assert_eq!(
        ship.navigation.nav_com(),
        Some(NavTargetRef::cell(destination.0, destination.1))
    );
    assert_eq!(
        ship.mission.queued(),
        MissionId::from_known(MissionType::Move)
    );
}

#[test]
fn default_rally_keeps_all_rng_streams_and_survives_a_pending_command_save() {
    let Some(retail) = retail_battle_rules_for_map("Hills.mmx") else {
        return;
    };
    let rules = &retail.rules;
    let (mut sim, id) = yard_fixture(rules, "GAYARD", true);
    let rng = (
        sim.main_rng.logical_state(),
        sim.scenario_rng.logical_state(),
        sim.mapgen_rng.logical_state(),
    );
    sim.initialize_naval_rally_point(id, rules);
    assert_eq!(
        (
            sim.main_rng.logical_state(),
            sim.scenario_rng.logical_state(),
            sim.mapgen_rng.logical_state()
        ),
        rng
    );
    let pending = sim.pending_command_snapshot();
    assert_eq!(pending.len(), 1);
    let bytes = crate::sim::snapshot::GameSnapshot::save(&sim, 0, 0, "naval arena", 0);
    let loaded = crate::sim::snapshot::GameSnapshot::load_unchecked(&bytes).unwrap();
    assert_eq!(loaded.sim.pending_command_snapshot(), pending);
    assert_eq!(
        loaded.sim.entities().get(id).unwrap().archive_target(),
        None
    );
}

#[test]
fn a_failed_water_search_still_snaps_the_offset_null_cell() {
    let Some(retail) = retail_battle_rules_for_map("Hills.mmx") else {
        return;
    };
    let rules = &retail.rules;
    let (mut sim, id) = yard_fixture(rules, "GAYARD", true);
    // Change only the supplied map prior: no water can satisfy the first
    // Float/Water search. The original caller still sends(2,2) to443860.
    crate::sim::arena_fixture::flat_ground(&mut sim, rules);
    let expected = sim
        .factory_rally_cell_input(id, (2, 2), rules)
        .unwrap()
        .unwrap();
    sim.initialize_naval_rally_point(id, rules);
    dispatch(&mut sim, rules);
    assert_eq!(
        sim.substrate.entities.get(id).unwrap().archive_target(),
        Some(TargetKind::Cell(expected.0, expected.1))
    );
}

#[test]
fn two_searches_and_event_birth_match_the_original_supplied_map_control() {
    use super::super::native_cell_input_test_fixture::{bounds, native_fixture};
    use crate::rules::ini_parser::IniFile;
    use crate::sim::combat::Health;
    use crate::sim::game_entity::GameEntity;
    use serde_json::json;

    // Original44699A -> real56DC20 -> real443860 -> real56DC20 -> real4C6780,
    // with the same map/zone prior as the existing rally_input native corpus.
    // At frames1/100 the first result(5,5) becomes(7,7); zero speed rows
    // produce two misses and no event. Only OS timestamp is supplied natively.
    let rules = RuleSet::from_ini(&IniFile::from_str(
        "[BuildingTypes]\n0=YARD\n[YARD]\nStrength=1000\nFactory=UnitType\nWeaponsFactory=yes\nNaval=yes\n",
    )).unwrap();
    for (frame, zero_speeds) in [(1, false), (100, false), (100, true)] {
        let (terrain, zones, raw) = native_fixture(&json!({"zero_speed_table":zero_speeds}));
        let mut sim = Simulation::new();
        let owner = sim.intern("Local");
        let type_ref = sim.intern("YARD");
        let mut actor = GameEntity::new_at_frame_zero_for_test(
            1,
            5,
            5,
            0,
            0,
            owner,
            Health::new(1000),
            type_ref,
            crate::map::entities::EntityCategory::Structure,
            0,
            5,
            false,
        );
        actor.lifecycle.set_in_limbo_for_test(false);
        actor.position.put_world_xy([1344, 1344]);
        sim.substrate.entities.insert(actor);
        sim.resolved_terrain = Some(terrain);
        sim.zone_grid = Some(zones);
        sim.substrate.raw_cell_occupation = raw;
        sim.playfield_bounds = Some(bounds());
        sim.playfield_size_height = Some(8);
        sim.session.set_binary_frame_for_test(frame);
        sim.initialize_naval_rally_point(1, &rules);
        let pending = sim.pending_command_snapshot();
        assert_eq!(pending.len(), usize::from(!zero_speeds));
        if !zero_speeds {
            assert_eq!(
                pending[0].payload,
                Command::SetRally {
                    rx: 7,
                    ry: 7,
                    producer_ids: vec![1]
                }
            );
        }
        assert_eq!(sim.entities().get(1).unwrap().archive_target(), None);
    }
}
