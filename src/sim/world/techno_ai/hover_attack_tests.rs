//! The HoverAttack re-anchor (`0x004D4DC0..0x004D4E07`,
//! `0x004D51E0..0x004D521F`) through the production Foot dispatcher.

use crate::sim::combat::TargetKind;
use crate::sim::components::NavTargetRef;
use crate::sim::house_state::HouseState;
use crate::sim::mission::state::MissionTestFixture;
use crate::sim::mission::{MissionDispatchTimer, MissionId, MissionType};
use crate::sim::world::{ObjectAiCtx, Simulation};

fn guard_with_target(sim: &mut Simulation, id: u64, target: u64, mission: MissionType) {
    let entity = sim.substrate.entities.get_mut(id).unwrap();
    entity.mission.apply_test_fixture(MissionTestFixture {
        current: MissionId::from_known(mission),
        queued: MissionId::NONE,
        suspended: MissionId::NONE,
        movement_bypass_latch: 0,
        handler_state: 0,
        mission_start_frame: 0,
        ai_counter: 0,
        dispatch_timer: MissionDispatchTimer::at_frame(0),
    });
    entity
        .attack_target
        .set_for_test(Some(TargetKind::Entity(target)));
}

/// On retail rules a landed Rocketeer (`JUMPJET`, `HoverAttack=yes`) holding
/// a Target takes NearbyLocation as its destination on its Guard dispatch, as
/// on Attack; a rifleman (`E1`) holding one does not, nor a Rocketeer aloft.
#[test]
fn a_landed_rocketeer_holding_a_target_re_anchors_on_guard_and_attack() {
    let Some(rules) = crate::rules::retail_ini_fixture::retail_unlayered_rules_with_art() else {
        return;
    };
    assert!(rules.object("JUMPJET").unwrap().hover_attack);
    assert!(!rules.object("E1").unwrap().hover_attack);
    let mut sim = Simulation::with_seed(5);
    sim.intern_rule_type_ids(&rules);
    sim.resolve_type_handles(&rules);
    for (name, side) in [("Americans", 0), ("Russians", 1)] {
        let id = sim.interner.intern(name);
        sim.houses
            .register(HouseState::new(id, side, None, false, 0, 10));
    }
    crate::sim::arena_fixture::flat_ground(&mut sim, &rules);
    let spawn = |sim: &mut Simulation, kind, owner, x, y| {
        sim.spawn_object_at_height(kind, owner, x, y, 0, 0, &rules)
            .unwrap()
    };
    let enemy = spawn(&mut sim, "HTNK", "Russians", 14, 10);
    for mission in [MissionType::Guard, MissionType::Attack] {
        let rocketeer = spawn(&mut sim, "JUMPJET", "Americans", 10, 12);
        let rifleman = spawn(&mut sim, "E1", "Americans", 10, 16);
        let expected = sim
            .techno_nearby_location(rocketeer, Some(rocketeer), &rules)
            .expect("an open arena has a nearby cell");
        for id in [rocketeer, rifleman] {
            guard_with_target(&mut sim, id, enemy, mission);
            crate::sim::world::dispatch_foot_mission(&mut sim, id, &rules, ObjectAiCtx::default());
        }
        let nav_com = |id| sim.substrate.entities.get(id).unwrap().navigation.nav_com();
        assert_eq!(
            nav_com(rocketeer),
            Some(NavTargetRef::cell(expected.0 as u16, expected.1 as u16)),
            "{mission:?}"
        );
        if mission == MissionType::Guard {
            assert_eq!(nav_com(rifleman), None);
        }
    }
    let aloft = spawn(&mut sim, "JUMPJET", "Americans", 20, 20);
    sim.substrate
        .entities
        .get_mut(aloft)
        .unwrap()
        .position
        .set_exact_z_leptons_for_test(Some(500));
    assert_ne!(
        crate::sim::movement::air_movement::current_fly_height(
            sim.substrate.entities.get(aloft).unwrap(),
            sim.resolved_terrain.as_ref(),
        ),
        0
    );
    guard_with_target(&mut sim, aloft, enemy, MissionType::Guard);
    crate::sim::world::dispatch_foot_mission(&mut sim, aloft, &rules, ObjectAiCtx::default());
    assert_eq!(
        sim.substrate
            .entities
            .get(aloft)
            .unwrap()
            .navigation
            .nav_com(),
        None,
        "aloft"
    );
}

/// A constructed stock-type terrain prestate: one clear landing cell with
/// the Unit's real ground occupation, surrounded by more than32 rings of
/// stock Rock (Hover0%). This is not a named retail map/touchdown history.
fn boxed_landed_unit(
    rules: &crate::rules::ruleset::RuleSet,
    kind: &str,
    missing_zero: bool,
) -> (Simulation, u64) {
    use crate::rules::terrain_rules::LandType;
    let mut sim = crate::sim::arena_fixture::rival_arena_with_size(rules, 1, 96);
    let actor = sim
        .spawn_object_at_height(kind, "Americans", 48, 48, 0, 0, rules)
        .unwrap();
    // Keep the rival house alive during continuation, outside both searches.
    sim.spawn_object_at_height("HTNK", "Russians", 84, 84, 0, 0, rules)
        .unwrap();
    assert_eq!(
        sim.substrate.raw_cell_occupation.ground_bits(48, 48) & 0x20,
        0x20
    );
    let rock = rules
        .terrain_rules
        .semantics_for_land_type(LandType::Rock.as_index())
        .unwrap();
    assert_eq!(rock.speed_costs.hover, Some(0));
    let terrain = sim.resolved_terrain.as_mut().unwrap();
    for y in 0..96 {
        for x in 0..96 {
            if [(48, 48), (84, 84)].contains(&(x, y)) {
                continue;
            }
            let cell = terrain.cell_mut_for_test(x, y).unwrap();
            cell.land_type = LandType::Rock.as_index();
            cell.base_land_type = cell.land_type;
            cell.speed_costs = rock.speed_costs;
            cell.base_speed_costs = cell.speed_costs;
            cell.ground_walk_blocked = rock.ground_blocked;
            cell.base_ground_walk_blocked = rock.ground_blocked;
            cell.base_build_blocked = !rock.buildable;
            cell.zone_type = crate::map::resolved_terrain::recalc_zone_type(
                false,
                None,
                cell.land_type,
                cell.speed_costs.wheel,
                None,
            );
        }
    }
    if missing_zero {
        let allocated: Vec<_> = (0..96)
            .flat_map(|y| (0..96).map(move |x| (x, y)))
            .filter(|&cell| cell != (0, 0))
            .collect();
        terrain.test_set_native_allocated_cells(&allocated);
    }
    assert!(sim.rebuild_dynamic_navigation(rules));
    sim.session.set_binary_frame_for_test(100);
    (sim, actor)
}

#[test]
fn failed_hover_attack_reanchor_keeps_the_original_jumpjet_handoff() {
    let Some(rules) = crate::rules::retail_ini_fixture::retail_unlayered_rules_with_art() else {
        return;
    };
    let native = crate::test_fixture::json("tools/spatial_oracle/hover_attack_reanchor.json");
    for kind in ["SCHP", "SHAD"] {
        let object = rules.object(kind).unwrap();
        assert!(object.hover_attack);
        for row in native.as_array().unwrap() {
            let missing_zero = row["input"]["missing_zero"].as_bool().unwrap();
            let mission = if row["input"]["mission"] == "guard" {
                MissionType::Guard
            } else {
                MissionType::Attack
            };
            let (mut sim, actor) = boxed_landed_unit(&rules, kind, missing_zero);
            guard_with_target(&mut sim, actor, actor, mission);
            let target = Some(TargetKind::Cell(50, 50));
            sim.substrate
                .entities
                .get_mut(actor)
                .unwrap()
                .attack_target
                .set_for_test(target);
            assert_eq!(
                sim.techno_nearby_location(actor, Some(actor), &rules),
                None,
                "{kind}: actual search must fail, including the occupied centre"
            );
            sim.scenario_rng = crate::sim::rng::SimRng::new(1);
            let rng = sim.scenario_rng.native_state_hex();
            // Compare exactly the native prefix's setter-return boundary,
            // before either mission performs later targeting/cadence work.
            super::hover_attack_reanchor(&mut sim, &rules, actor, ObjectAiCtx::default());
            let entity = sim.substrate.entities.get(actor).unwrap();
            let runtime = entity
                .locomotor
                .as_ref()
                .unwrap()
                .jumpjet_runtime()
                .unwrap();
            assert_eq!(
                entity.navigation.nav_com(),
                Some(NavTargetRef::cell(0, 0)),
                "{kind}: {row}"
            );
            let expected = &row["output"];
            assert_eq!(
                serde_json::to_value(runtime.destination()).unwrap(),
                serde_json::json!({
                "x": expected["destination"][0], "y": expected["destination"][1], "z": expected["destination"][2]})
            );
            assert_eq!(runtime.moving(), expected["moving"].as_bool().unwrap());
            assert_eq!(runtime.phase(), expected["phase"].as_i64().unwrap() as i32);
            assert_eq!(
                entity.navigation.path_runtime().movement_timer().duration(),
                expected["movement_timer"][1].as_i64().unwrap() as i32
            );
            assert_eq!(entity.attack_target.target(), target);
            assert!(expected["rng_unchanged"].as_bool().unwrap());
            assert_eq!(
                sim.scenario_rng.native_state_hex(),
                rng,
                "{kind}: Unit placement draws no subcell RNG"
            );
        }
    }
}

#[test]
fn failed_reanchor_on_guard_lifts_the_stock_unit_and_survives_save() {
    use crate::sim::snapshot::GameSnapshot;
    use crate::sim::world::SnapshotProcessState;
    let Some(rules) = crate::rules::retail_ini_fixture::retail_unlayered_rules_with_art() else {
        return;
    };
    let (mut sim, actor) = boxed_landed_unit(&rules, "SCHP", true);
    guard_with_target(&mut sim, actor, actor, MissionType::Guard);
    sim.substrate
        .entities
        .get_mut(actor)
        .unwrap()
        .attack_target
        .set_for_test(Some(TargetKind::Cell(50, 50)));
    crate::sim::world::dispatch_foot_mission(&mut sim, actor, &rules, ObjectAiCtx::default());
    assert_eq!(
        sim.substrate
            .entities
            .get(actor)
            .unwrap()
            .navigation
            .nav_com(),
        Some(NavTargetRef::cell(0, 0))
    );
    let bytes = GameSnapshot::save_validated(&sim, 17, 23, "reanchor", 0);
    let mut restored = GameSnapshot::load_validated(&bytes, 17, 23, "")
        .unwrap()
        .sim;
    restored.retain_process_state_for_load(SnapshotProcessState::Live(&sim));
    restored.resolved_terrain = sim.resolved_terrain.clone();
    restored.restore_after_snapshot_load().unwrap();
    restored.rebuild_caches_after_load(
        sim.resolved_terrain.as_ref().unwrap().clone(),
        sim.terrain_speed_config().clone(),
        &rules,
    );
    assert!(restored.rebuild_dynamic_navigation(&rules));
    let control_dummy = sim.effective_shared_cell_dummy();
    restored.bind_shared_cell_dummy(control_dummy.fork_query_identity());
    assert!(
        !restored
            .effective_shared_cell_dummy()
            .same_identity(&control_dummy)
    );
    sim.scenario_rng = crate::sim::rng::SimRng::new(0);
    for id in sim.substrate.entities.keys_sorted() {
        sim.substrate.restore_move_sound_state_after_load(id);
    }
    assert_eq!(restored.state_hash(), sim.state_hash());
    for _ in 0..4 {
        for world in [&mut sim, &mut restored] {
            let result = world.advance_tick(&[], Some(&rules), None, None, 66);
            assert!(result.frame_committed, "{result:?}");
        }
        assert_eq!(restored.state_hash(), sim.state_hash());
    }
    let entity = sim.substrate.entities.get(actor).unwrap();
    assert!(
        crate::sim::movement::air_movement::current_fly_height(
            entity,
            sim.resolved_terrain.as_ref()
        ) > 0,
        "failed relocation still starts the Jumpjet lift"
    );
}
