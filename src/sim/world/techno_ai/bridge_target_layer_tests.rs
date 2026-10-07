//! Native passive AI -> scanner -> actual FireError/Evaluate -> assignment.
//! World admission and inter-step pose/list changes are supplied, as in the
//! original corpus. The separate retail-map test covers production composition.
use super::*;
use crate::map::resolved_terrain::{ResolvedTerrainGrid, test_flat_cell};
use crate::rules::art_data::ArtRegistry;
use crate::rules::retail_ini_fixture::retail_rules_and_art;
use crate::sim::components::Health;
use crate::sim::game_entity::GameEntity;
use crate::sim::house_state::HouseState;
use crate::sim::mission::{MissionId, MissionTimer};
use crate::sim::movement::locomotor::{LocomotorState, MovementLayer};
use crate::sim::occupancy::{CellListInsertion, OccupancyGrid};
use crate::sim::timer::CdTimer;
use crate::sim::world::bridge_test_evidence::restored_retail;
use crate::util::fixed_math::SimFixed;
use serde_json::{Value, json};

fn native() -> Value {
    serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/bridge_target_composed.json",
    ))
    .unwrap()
}

fn retail_rules() -> Option<RuleSet> {
    let (ini, art) = retail_rules_and_art()?;
    let mut rules = RuleSet::from_ini_with_fixed_art_for_test(&ini, &art).unwrap();
    rules.install_art_data(ArtRegistry::from_ini(&art));
    Some(rules)
}

fn supplied_world(rules: &RuleSet, case: &Value) -> Simulation {
    let input = &case["inputs"];
    let obj = rules.object("FV").unwrap();
    assert_eq!(obj.weapon_list[0].as_deref(), Some("HoverMissile"));
    let weapon = rules.weapon("HoverMissile").unwrap();
    assert_eq!(
        json!((weapon.range * SimFixed::from_num(256)).to_num::<i32>()),
        input["weapon_range"]
    );
    assert_eq!(
        json!((weapon.minimum_range * SimFixed::from_num(256)).to_num::<i32>()),
        input["weapon_minimum_range"]
    );
    assert_eq!(rules.combat_damage.max_damage, 10_000);
    assert_eq!(
        json!((obj.air_range_bonus.unwrap_or_default() * SimFixed::from_num(256)).to_num::<i32>()),
        input["air_range_bonus"]
    );
    assert_eq!(json!(obj.strength), input["strength"]);
    assert_eq!(
        json!(crate::sim::combat::armor_index(&obj.armor)),
        input["armor"]
    );
    assert_eq!(
        json!(rules.general.normal_targeting_delay),
        input["normal_delay"]
    );
    assert_eq!(
        json!(rules.general.guard_area_targeting_delay),
        input["area_delay"]
    );
    let mut sim = Simulation::with_seed(31);
    sim.intern_rule_type_ids(rules);
    let cells = (0..25)
        .flat_map(|y| {
            (0..25).map(move |x| {
                let mut cell = test_flat_cell(x, y);
                cell.level = 6;
                cell
            })
        })
        .collect();
    let mut terrain = ResolvedTerrainGrid::from_cells(25, 25, cells);
    // Same sparse real-cell extent as guided_step.create inherited by native.
    terrain.test_set_native_allocated_cells(
        &(16..25)
            .flat_map(|y| (6..25).map(move |x| (x, y)))
            .collect::<Vec<_>>(),
    );
    terrain.shared_cell_dummy().stamp_coord(111, -222);
    sim.install_resolved_terrain_for_new_map(terrain);
    sim.install_playfield_from_map_header(&crate::map::map_file::MapHeader {
        theater: "TEMPERATE".into(),
        fill: "Clear".into(),
        level: 6,
        width: 20,
        height: 20,
        local_left: 0,
        local_top: 0,
        local_width: 20,
        local_height: 20,
    });
    sim.session.game_mode_nonzero = true;
    sim.session.binary_frame = 173;
    for (id, owner, x) in [
        (1, "Americans", 10),
        (2, "Russians", 12),
        (3, "Russians", 12),
    ] {
        let owner = sim.intern(owner);
        sim.houses
            .entry(owner)
            .or_insert_with(|| HouseState::new(owner, 0, None, true, 0, 10));
        let mut entity = GameEntity::new_at_frame_zero_for_test(
            id,
            x,
            20,
            6,
            0,
            owner,
            Health {
                current: obj.strength,
            },
            sim.intern("FV"),
            EntityCategory::Unit,
            0,
            6,
            true,
        );
        entity.lifecycle.in_limbo = false;
        entity.lifecycle.cell_marked = true;
        entity.in_playfield = true;
        entity.locomotor = Some(LocomotorState::from_object_type(obj, 0));
        entity.passive_scan_timer = MissionTimer {
            start_frame: 0,
            duration: 0,
        };
        entity.last_target_scan_frame = 0;
        entity.position.exact_z_leptons = Some(624);
        sim.substrate.entities.insert(entity);
        sim.mission_assign_exact(id, MissionId::from_known(MissionType::Guard), 0)
            .unwrap();
    }
    sim.resolve_type_handles(rules);
    sim
}

fn supply_step(sim: &mut Simulation, rules: &RuleSet, input: &Value) {
    use crate::sim::combat::TargetKind;
    sim.session.binary_frame = input["frame"].as_u64().unwrap_or(173) as u32;
    sim.substrate.occupancy = OccupancyGrid::new();
    for (index, id, x) in [(0, 1, 10), (1, 2, 12)] {
        let deck = input["on_bridge"][index].as_i64().unwrap_or(0) != 0;
        let flags = input["flags"][index].as_u64().unwrap_or(0x100) as u32;
        let terrain = sim.resolved_terrain.as_mut().unwrap();
        let cell = crate::map::cell_index::NativeCellIdentity::Real(20 * 25 + x);
        terrain.write_native_cell_flags(cell, flags);
        let entity = sim.substrate.entities.get_mut(id).unwrap();
        entity.on_bridge = deck;
        entity.position.rx = x as u16;
        entity.position.ry = 20;
        entity.position.sub_x = SimFixed::from_num(128);
        entity.position.sub_y = SimFixed::from_num(128);
        entity.position.exact_z_leptons = Some(if deck { 1040 } else { 624 });
        sim.substrate.occupancy.add(
            x as u16,
            20,
            id,
            if deck {
                MovementLayer::Bridge
            } else {
                MovementLayer::Ground
            },
            None,
            CellListInsertion::PrependNonBuilding,
        );
    }
    if let Some(mode) = input["alternative"].as_str() {
        let y = if mode == "other_cell" { 21 } else { 20 };
        let alternate = sim.substrate.entities.get_mut(3).unwrap();
        alternate.position.ry = y;
        alternate.position.exact_z_leptons = Some(624);
        let terrain = sim.resolved_terrain.as_mut().unwrap();
        terrain.write_native_cell_flags(
            crate::map::cell_index::NativeCellIdentity::Real(usize::from(y) * 25 + 12),
            input["alternative_flags"].as_u64().unwrap_or(0x100) as u32,
        );
        // Deliberately mixed tail is a native supplied-list control, not an
        // ordinary movement/admission claim. Add alternate first, then head.
        if mode == "same_list" {
            sim.substrate.occupancy.remove(12, 20, 2);
            sim.substrate.occupancy.add(
                12,
                20,
                3,
                MovementLayer::Bridge,
                None,
                CellListInsertion::PrependNonBuilding,
            );
            sim.substrate.occupancy.add(
                12,
                20,
                2,
                MovementLayer::Bridge,
                None,
                CellListInsertion::PrependNonBuilding,
            );
        } else {
            sim.substrate.occupancy.add(
                12,
                y,
                3,
                MovementLayer::Ground,
                None,
                CellListInsertion::PrependNonBuilding,
            );
        }
    }
    if input["clear_target"] == true {
        sim.assign_target_represented(1, None::<TargetKind>, Some(rules))
            .unwrap();
    }
    let source = sim.substrate.entities.get_mut(1).unwrap();
    if let Some(timer) = input["rearm"].as_array() {
        source.rearm_timer = CdTimer::from_raw(
            timer[0].as_i64().unwrap() as i32,
            timer[1].as_i64().unwrap() as i32,
        );
    }
    if let Some(timer) = input["timer"].as_array() {
        source.passive_scan_timer = MissionTimer {
            start_frame: timer[0].as_i64().unwrap() as u32,
            duration: timer[2].as_i64().unwrap() as u32,
        };
    }
    if let Some(health) = input["health"].as_i64() {
        sim.substrate.entities.get_mut(2).unwrap().health.current = health as i32;
    }
}

fn assert_state(sim: &Simulation, state: &Value, label: &str) {
    use crate::sim::combat::TargetKind;
    let source = sim.substrate.entities.get(1).unwrap();
    let name = match source.attack_target.as_ref().map(|a| a.target) {
        None => "null",
        Some(TargetKind::Entity(1)) => "source",
        Some(TargetKind::Entity(2)) => "target",
        Some(TargetKind::Entity(3)) => "alternative",
        other => panic!("unexpected {other:?}"),
    };
    assert_eq!(json!(name), state["target"], "{label}: target");
    assert_eq!(
        json!(u8::from(source.passively_acquired_target)),
        state["passive"],
        "{label}: passive"
    );
    assert_eq!(
        json!(source.last_target_scan_frame as i32),
        state["last_scan"],
        "{label}: last_scan"
    );
    assert_eq!(
        json!(source.passive_scan_timer.start_frame as i32),
        state["timer"][0],
        "{label}: timer anchor"
    );
    assert_eq!(
        json!(source.passive_scan_timer.duration as i32),
        state["timer"][2],
        "{label}: timer duration"
    );
    assert_eq!(
        json!(
            sim.resolved_terrain
                .as_ref()
                .unwrap()
                .shared_cell_dummy()
                .snapshot()
                .coord
        ),
        state["dummy_coord"],
        "{label}: shared cell query continuation"
    );
    // Native +184 is inactive scratch residue, not represented timer state.
    assert_eq!(
        json!(
            sim.substrate
                .entities
                .get(2)
                .unwrap()
                .estimated_health
                .get()
        ),
        state["estimated_health"],
        "{label}: estimate debit"
    );
    if !state["alternative_estimated_health"].is_null() {
        assert_eq!(
            json!(
                sim.substrate
                    .entities
                    .get(3)
                    .unwrap()
                    .estimated_health
                    .get()
            ),
            state["alternative_estimated_health"],
            "{label}: alternate debit"
        );
    }
}

// The supplied 0x100 -> 0x400 -> 0x100 history proves live flag reactivity.
// It does not execute an Engineer repair or prove ordinary rebuilding.
#[test]
fn native_passive_bridge_histories_preserve_timers_rng_assignment_and_cell_list_choice() {
    let Some(rules) = retail_rules() else { return };
    let corpus = native();
    let mut histories = 0;
    let mut steps = 0;
    for case in corpus["cases"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|c| c["direct_evaluate"] != true)
    {
        let mut sim = supplied_world(&rules, case);
        let label = case["name"].as_str().unwrap();
        for (index, step) in case["steps"].as_array().unwrap().iter().enumerate() {
            supply_step(&mut sim, &rules, &step["supplied"]);
            assert_state(&sim, &step["before"], &format!("{label}/{index} before"));
            assert_eq!(
                sim.scenario_rng.native_state_hex(),
                step["rng_before_hex"].as_str().unwrap(),
                "{label}/{index} before RNG"
            );
            let main = sim.main_rng.logical_state();
            let map = sim.mapgen_rng.logical_state();
            let ids = sim.native_unique_ids.as_ref().unwrap().current_raw();
            passive_acquire_step(&mut sim, 1, Some(&rules), ObjectAiCtx::default());
            assert_state(&sim, &step["after"], &format!("{label}/{index} after"));
            assert_eq!(
                sim.scenario_rng.native_state_hex(),
                step["rng_after_hex"].as_str().unwrap(),
                "{label}/{index} after RNG"
            );
            assert_eq!(sim.main_rng.logical_state(), main);
            assert_eq!(sim.mapgen_rng.logical_state(), map);
            assert_eq!(
                sim.native_unique_ids.as_ref().unwrap().current_raw(),
                ids,
                "scan allocates no native identity"
            );
            steps += 1;
        }
        histories += 1;
    }
    assert_eq!((histories, steps), (21, 33));
}

fn move_retail_fv(
    scenario: &mut crate::headless_scenario::HeadlessScenario,
    owner: crate::sim::intern::InternedId,
    start: (u16, u16),
    destination: (u16, u16),
    stop_at: (u16, u16),
    deck: bool,
) -> u64 {
    use crate::sim::command::{Command, CommandEnvelope};
    let runtime = &mut scenario.runtime;
    let owner_name = runtime.simulation.interner.resolve(owner).to_owned();
    let id = runtime
        .simulation
        .spawn_object(
            "FV",
            &owner_name,
            start.0,
            start.1,
            0,
            &runtime.resources.rules,
        )
        .expect("ordinary FV placement on bank/road");
    // Keep the isolated movement preparation from acquiring hostiles. The
    // targeting entry state is restored explicitly after both arrivals.
    runtime
        .simulation
        .substrate
        .entities
        .get_mut(id)
        .unwrap()
        .passive_scan_timer =
        MissionTimer::armed(runtime.simulation.session.binary_frame, 1_000_000);
    let command = CommandEnvelope::new(
        owner,
        runtime.simulation.session.tick + 1,
        Command::Move {
            entity_id: id,
            target_rx: destination.0,
            target_ry: destination.1,
            queue: false,
        },
    );
    let mut stop_sent = false;
    for frame in 0..1200 {
        // A cell-target Move on the footprint intentionally selects the deck.
        // Ground admission instead crosses between exterior road cells; Stop
        // finishes the paid movement head after entering the desired cell.
        let stop = (!stop_sent
            && destination != stop_at
            && runtime
                .simulation
                .substrate
                .entities
                .get(id)
                .is_some_and(|e| (e.position.rx, e.position.ry) == stop_at))
        .then(|| {
            stop_sent = true;
            CommandEnvelope::new(
                owner,
                runtime.simulation.session.tick + 1,
                Command::Stop { entity_id: id },
            )
        });
        runtime
            .advance_frame(
                if frame == 0 {
                    std::slice::from_ref(&command)
                } else if let Some(stop) = stop.as_ref() {
                    std::slice::from_ref(stop)
                } else {
                    &[]
                },
                crate::headless_scenario::SIM_TICK_MS,
                crate::sim::world::TickLane::Ordinary,
            )
            .unwrap();
        let entity = runtime.simulation.substrate.entities.get(id).unwrap();
        if (entity.position.rx, entity.position.ry) == stop_at && entity.movement_target.is_none() {
            assert_eq!(
                entity.on_bridge, deck,
                "ordinary Move selected wrong layer at {stop_at:?}"
            );
            assert_eq!(
                entity.position.exact_z_leptons,
                Some(if deck { 1040 } else { 624 })
            );
            assert!(
                runtime
                    .simulation
                    .substrate
                    .occupancy
                    .get(stop_at.0, stop_at.1)
                    .unwrap()
                    .iter_layer(if deck {
                        MovementLayer::Bridge
                    } else {
                        MovementLayer::Ground
                    })
                    .any(|entry| entry.entity_id == id)
            );
            println!(
                "FV {id} moved {start:?}->{destination:?}, stopped {stop_at:?}, deck={deck}, frames={}",
                frame + 1
            );
            return id;
        }
    }
    panic!(
        "ordinary FV Move failed {start:?}->{destination:?}, stop {stop_at:?}: {:?}",
        runtime.simulation.substrate.entities.get(id)
    );
}

fn assert_retail_target_state_restored(live: &Simulation, restored: &Simulation, ids: [u64; 2]) {
    // Native Scenario load (683564) reseeds its RNG and Map resize recreates
    // the shared Dummy. An uninterrupted future is therefore not expected to
    // match a restored future. Compare the saved actors directly, then compare
    // the two restored continuations below without normalizing either state.
    assert_eq!(
        restored.scenario_rng.logical_state(),
        crate::sim::rng::SimRng::new(0).logical_state()
    );
    for id in ids {
        let before = live.substrate.entities.get(id);
        let after = restored.substrate.entities.get(id);
        assert_eq!(before.is_some(), after.is_some(), "restored actor {id}");
        if let (Some(before), Some(after)) = (before, after) {
            assert_eq!(json!(before.position), json!(after.position));
            assert_eq!(before.on_bridge, after.on_bridge);
            assert_eq!(before.passive_scan_timer, after.passive_scan_timer);
            assert_eq!(before.rearm_timer, after.rearm_timer);
            assert_eq!(json!(before.attack_target), json!(after.attack_target));
            assert_eq!(
                before.passively_acquired_target,
                after.passively_acquired_target
            );
            assert_eq!(before.estimated_health, after.estimated_health);
            assert_eq!(before.health.current, after.health.current);
            assert_eq!(before.lifecycle.object_alive, after.lifecycle.object_alive);
            assert_eq!(before.lifecycle.in_limbo, after.lifecycle.in_limbo);
            assert_eq!(before.dying, after.dying);
        }
    }
    for xy in [(64, 69), (66, 69)] {
        let before = live
            .resolved_terrain
            .as_ref()
            .unwrap()
            .cell(xy.0, xy.1)
            .unwrap();
        let after = restored
            .resolved_terrain
            .as_ref()
            .unwrap()
            .cell(xy.0, xy.1)
            .unwrap();
        assert_eq!(before.level, after.level);
        assert_eq!(before.bridge_facts.raw_flags, after.bridge_facts.raw_flags);
    }
}

#[test]
#[ignore = "requires physical retail Hills and production movement/restore; no native whole-scene claim"]
fn retail_hills_passive_bridge_layers_survive_movement_and_restore() {
    retail_hills_collapsed_scene(|_, _| {});
}

pub(super) fn retail_hills_collapsed_scene(
    mut observe: impl FnMut(&Simulation, &str),
) -> (
    crate::headless_scenario::HeadlessScenario,
    ResolvedTerrainGrid,
) {
    use crate::sim::snapshot::GameSnapshot;
    let retail = std::env::var("RA2_DIR")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|_| {
            crate::util::config::GameConfig::load()
                .unwrap()
                .paths
                .ra2_dir
        });
    let mut scenario = crate::headless_scenario::load(&retail, "Hills.mmx", 0x0B21_D6E5).unwrap();
    let pristine = scenario.sim().resolved_terrain.as_ref().unwrap().clone();
    observe(scenario.sim(), "initial_loaded");
    for xy in [(64, 69), (66, 69)] {
        let cell = pristine.cell(xy.0, xy.1).unwrap();
        assert_eq!(
            (cell.level, cell.slope_type, cell.base_yr_cell_land_type),
            (6, 0, 7)
        );
        assert_eq!(cell.base_speed_costs.track, Some(100));
        assert!(!cell.base_ground_walk_blocked);
        assert_ne!(cell.bridge_facts.raw_flags & 0x100, 0);
    }
    let owner = scenario.sim().session.current_house.unwrap();
    let source = move_retail_fv(&mut scenario, owner, (64, 72), (64, 69), (64, 69), true);
    // Initially same owner so movement preparation cannot cause combat. The
    // hostile House link is a supplied acquisition-entry boundary below.
    let target = move_retail_fv(&mut scenario, owner, (62, 69), (68, 69), (66, 69), false);
    observe(scenario.sim(), "after_ordinary_fv_moves");
    let enemy = scenario.runtime.simulation.intern("Russians");
    scenario
        .runtime
        .simulation
        .houses
        .entry(enemy)
        .or_insert_with(|| HouseState::new(enemy, 0, None, true, 0, 10));
    scenario
        .runtime
        .simulation
        .substrate
        .entities
        .change_owner(target, enemy);
    {
        let runtime = &mut scenario.runtime;
        runtime
            .simulation
            .mission_assign_exact(
                source,
                MissionId::from_known(MissionType::Guard),
                runtime.simulation.session.binary_frame,
            )
            .unwrap();
        let entity = runtime
            .simulation
            .substrate
            .entities
            .get_mut(source)
            .unwrap();
        entity.passive_scan_timer.clear();
        // Native composed deck-to-ground rearm boundary: ready weapons
        // reject earlier in T58; the busy deck source reaches Evaluate's G27.
        // The reverse direction rejects in InRange before G27.
        entity.rearm_timer = CdTimer::started(runtime.simulation.session.binary_frame as i32, 10);
        passive_acquire_step(
            &mut runtime.simulation,
            source,
            Some(&runtime.resources.rules),
            ObjectAiCtx {
                overlay_registry: Some(&runtime.resources.overlay_registry),
                ..Default::default()
            },
        );
        assert!(
            runtime
                .simulation
                .substrate
                .entities
                .get(source)
                .unwrap()
                .attack_target
                .is_none()
        );
        assert!(
            !runtime
                .simulation
                .substrate
                .entities
                .get(source)
                .unwrap()
                .passively_acquired_target
        );
    }
    let bytes = GameSnapshot::save_validated(
        scenario.sim(),
        scenario.map.ini.content_hash(),
        scenario.runtime.resources.rules.simulation_config_hash(),
        "Retail bridge layer scan",
        0,
    );
    let mut first = restored_retail(&scenario, &pristine, &bytes);
    let second = restored_retail(&scenario, &pristine, &bytes);
    assert_retail_target_state_restored(scenario.sim(), &first, [source, target]);
    assert_eq!(first.state_hash(), second.state_hash());
    for sim in [&first, &second] {
        for (id, deck, xy) in [(source, true, (64, 69)), (target, false, (66, 69))] {
            let e = sim.substrate.entities.get(id).unwrap();
            assert_eq!(e.on_bridge, deck);
            assert_eq!(
                e.position.exact_z_leptons,
                Some(if deck { 1040 } else { 624 })
            );
            assert!(
                sim.substrate
                    .occupancy
                    .get(xy.0, xy.1)
                    .unwrap()
                    .iter_layer(if deck {
                        MovementLayer::Bridge
                    } else {
                        MovementLayer::Ground
                    })
                    .any(|entry| entry.entity_id == id)
            );
        }
    }
    scenario.runtime.simulation = second;
    for frame in 0..40 {
        let runtime = &mut scenario.runtime;
        runtime
            .advance_frame(
                &[],
                crate::headless_scenario::SIM_TICK_MS,
                crate::sim::world::TickLane::Ordinary,
            )
            .unwrap();
        std::mem::swap(&mut runtime.simulation, &mut first);
        runtime
            .advance_frame(
                &[],
                crate::headless_scenario::SIM_TICK_MS,
                crate::sim::world::TickLane::Ordinary,
            )
            .unwrap();
        assert_eq!(
            runtime.simulation.state_hash(),
            first.state_hash(),
            "restored acquisition continuation frame{frame}"
        );
        assert!(
            runtime
                .simulation
                .substrate
                .entities
                .get(source)
                .unwrap()
                .attack_target
                .is_none()
        );
        std::mem::swap(&mut runtime.simulation, &mut first);
    }
    println!(
        "retail Hills: legal opposite layers, busy/ready passive scans and two restored futures matched 40 frames"
    );
    observe(scenario.sim(), "before_collapse_after_restored_40_frames");
    collapse_retail_scene(&mut scenario, &pristine, [source, target], &mut observe);
    (scenario, pristine)
}

/// Continue the physical scene through existing world publication; damage is
/// supplied at the bridge receiver boundary, not claimed as an FV launch.
fn collapse_retail_scene(
    scenario: &mut crate::headless_scenario::HeadlessScenario,
    pristine: &ResolvedTerrainGrid,
    actors: [u64; 2],
    observe: &mut impl FnMut(&Simulation, &str),
) {
    use crate::sim::bridge_state::BridgeDamageEvent;
    use crate::sim::snapshot::GameSnapshot;
    let runtime = &mut scenario.runtime;
    let warhead_ref = runtime.simulation.intern("HE");
    let event = BridgeDamageEvent {
        rx: 64,
        ry: 69,
        damage: runtime.resources.rules.bridge_rules.strength + 1,
        warhead_ref,
        is_ion_cannon: false,
        impact_z_leptons: 1040,
    };
    let mut collapsed = false;
    for _ in 0..8 {
        collapsed|=crate::sim::world::bridge_orchestrator::apply_bridge_damage_events_with_overlay_registry(&mut runtime.simulation,&runtime.resources.rules,&[event],Some(&runtime.resources.overlay_registry));
        if !runtime
            .simulation
            .resolved_terrain
            .as_ref()
            .unwrap()
            .cell(64, 69)
            .unwrap()
            .bridge_facts
            .has_structural_bridge()
        {
            break;
        }
    }
    assert!(collapsed, "real bridge dispatcher must publish collapse");
    assert!(
        !runtime
            .simulation
            .resolved_terrain
            .as_ref()
            .unwrap()
            .cell(64, 69)
            .unwrap()
            .bridge_facts
            .has_structural_bridge()
    );
    observe(&runtime.simulation, "after_receiver_collapse_before_load");
    let bytes = GameSnapshot::save_validated(
        &runtime.simulation,
        scenario.map.ini.content_hash(),
        runtime.resources.rules.simulation_config_hash(),
        "Collapsed passive scan scene",
        0,
    );
    let mut first = restored_retail(scenario, pristine, &bytes);
    let second = restored_retail(scenario, pristine, &bytes);
    assert_retail_target_state_restored(scenario.sim(), &first, actors);
    assert_eq!(first.state_hash(), second.state_hash());
    for restored in [&first, &second] {
        assert!(
            !restored
                .resolved_terrain
                .as_ref()
                .unwrap()
                .cell(64, 69)
                .unwrap()
                .bridge_facts
                .has_structural_bridge()
        );
    }
    scenario.runtime.simulation = second;
    for frame in 0..4 {
        let runtime = &mut scenario.runtime;
        runtime
            .advance_frame(
                &[],
                crate::headless_scenario::SIM_TICK_MS,
                crate::sim::world::TickLane::Ordinary,
            )
            .unwrap();
        std::mem::swap(&mut runtime.simulation, &mut first);
        runtime
            .advance_frame(
                &[],
                crate::headless_scenario::SIM_TICK_MS,
                crate::sim::world::TickLane::Ordinary,
            )
            .unwrap();
        assert_eq!(
            runtime.simulation.state_hash(),
            first.state_hash(),
            "collapsed target/lifecycle restore frame{frame}"
        );
        std::mem::swap(&mut runtime.simulation, &mut first);
    }
    println!("retail Hills bridge receiver collapse and four restored continuation frames matched");
    observe(scenario.sim(), "after_collapsed_restore_and_4_frames");
}
