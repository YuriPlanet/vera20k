//! Production composition witness: a retail Grizzly force-fires a loaded high
//! bridge through ordinary commands and bound runtime frames. Native scalar
//! comparisons live in bridge_launch_tests and bridge_damage_admission.json;
//! this test does not claim a native whole-flight or whole-match comparison.

use super::TargetKind;
use crate::headless_scenario::{HeadlessScenario, SIM_TICK_MS};
use crate::sim::bridge_state::BridgeCellRole;
use crate::sim::command::{Command, CommandEnvelope};
use crate::sim::projectile::{ProjectileCoord, ProjectileTarget};
use crate::sim::runtime::SimRuntime;
use crate::sim::snapshot::GameSnapshot;
use crate::sim::world::TickLane;
use std::collections::BTreeMap;

/// A flat bank at deck height, with two deck cells between it and the target.
/// The loaded geometry must provide this shot; no terrain or rules are edited.
fn firing_sites(scenario: &HeadlessScenario) -> Vec<((u16, u16), (u16, u16))> {
    let sim = scenario.sim();
    let terrain = sim.resolved_terrain.as_ref().unwrap();
    let navigation = sim.path_grid().unwrap();
    let bridges = sim.bridge_state.as_ref().unwrap();
    let mut sites = Vec::new();
    for target in terrain.cells() {
        if !target.bridge_facts.has_structural_bridge()
            || target.slope_type != 0
            || !bridges.is_bridge_walkable(target.rx, target.ry)
            || bridges.cell(target.rx, target.ry).unwrap().role != BridgeCellRole::Body
        {
            continue;
        }
        for (dx, dy) in [(1_i32, 0_i32), (-1, 0), (0, 1), (0, -1)] {
            let points = (1..=3)
                .map(|distance| {
                    Some((
                        u16::try_from(i32::from(target.rx) + dx * distance).ok()?,
                        u16::try_from(i32::from(target.ry) + dy * distance).ok()?,
                    ))
                })
                .collect::<Option<Vec<_>>>();
            let Some(points) = points else { continue };
            if !points[..2].iter().all(|&(x, y)| {
                terrain.cell(x, y).is_some_and(|cell| {
                    cell.bridge_facts.has_structural_bridge()
                        && cell.level == target.level
                        && cell.slope_type == 0
                        && bridges.is_bridge_walkable(x, y)
                })
            }) {
                continue;
            }
            let bank = points[2];
            if terrain.cell(bank.0, bank.1).is_some_and(|cell| {
                !cell.bridge_facts.has_structural_bridge()
                    && cell.slope_type == 0
                    && i32::from(cell.level as i8) == i32::from(target.level as i8) + 4
                    && navigation
                        .cell(bank.0, bank.1)
                        .is_some_and(|cell| cell.ground_walkable)
            }) {
                sites.push((bank, (target.rx, target.ry)));
            }
        }
    }
    sites
}

fn assert_collapsed_bridge_restores(
    scenario: &HeadlessScenario,
    template: &crate::map::resolved_terrain::ResolvedTerrainGrid,
    target: (u16, u16),
    attacker: u64,
) {
    let live = scenario.sim();
    let resources = &scenario.runtime.resources;
    assert!(
        template
            .cell(target.0, target.1)
            .unwrap()
            .bridge_facts
            .has_structural_bridge(),
        "restoration must start from the original intact map"
    );
    let map_hash = scenario.map.ini.content_hash();
    let rules_hash = resources.rules.simulation_config_hash();
    let bytes =
        GameSnapshot::save_validated(live, map_hash, rules_hash, "Retail bridge collapse", 0);
    let mut restored =
        GameSnapshot::load_validated(&bytes, map_hash, rules_hash, &live.session.map_name)
            .expect("saved retail scenario validates")
            .sim;

    // The simulation-owned portion of the app's PreparedLoad transaction:
    // detached Resize dummy, stable-reference fixups, pristine map caches,
    // then saved overlay/bridge authority and canonical navigation.
    restored.retain_in_scenario_process_state_from(live);
    restored.bind_shared_cell_dummy(crate::map::resolved_terrain::SharedCellDummy::fresh());
    restored.reconstruct_cellclass_dummy_for_map_resize();
    restored
        .restore_after_snapshot_load()
        .expect("saved object identities restore");
    crate::sim::production::validate_restored_factory_state(&restored, &resources.rules)
        .expect("saved production identities restore");
    restored.rebuild_caches_after_load(
        template.clone(),
        live.terrain_speed_config.clone(),
        live.bridge_explosions.clone(),
        live.metallic_debris.clone(),
    );
    restored
        .restore_map_authority_after_snapshot_load(&resources.rules, &resources.overlay_registry)
        .expect("saved map and bridge authority restore");
    restored.resolve_type_handles(&resources.rules);
    restored
        .restore_move_sound_handles_after_load(&resources.rules)
        .expect("saved movement sound handles restore");
    restored.rebuild_lighting_sources_after_load(&resources.rules);

    let terrain = live.resolved_terrain.as_ref().unwrap();
    let restored_terrain = restored.resolved_terrain.as_ref().unwrap();
    assert_eq!(
        restored_terrain
            .cell(target.0, target.1)
            .unwrap()
            .bridge_facts
            .raw_flags,
        terrain
            .cell(target.0, target.1)
            .unwrap()
            .bridge_facts
            .raw_flags,
        "restored target keeps the collapsed native flag word"
    );
    assert!(
        restored_terrain.capture_real_cell_bridge_flags_0x1180()
            == terrain.capture_real_cell_bridge_flags_0x1180(),
        "every allocated cell retains its current bridge flag authority"
    );
    let live_bridges = live.bridge_state.as_ref().unwrap();
    let restored_bridges = restored.bridge_state.as_ref().unwrap();
    for (coord, state) in live_bridges.iter_cells() {
        assert_eq!(
            restored_bridges.cell(coord.0, coord.1),
            Some(state),
            "bridge runtime {coord:?}"
        );
        assert_eq!(
            restored_bridges.is_bridge_walkable(coord.0, coord.1),
            live_bridges.is_bridge_walkable(coord.0, coord.1),
            "bridge surface {coord:?}"
        );
    }
    assert!(
        !restored
            .path_grid()
            .unwrap()
            .cell(target.0, target.1)
            .unwrap()
            .bridge_structural,
        "restored navigation must use the collapsed deck"
    );
    assert!(
        restored
            .entities()
            .get(attacker)
            .unwrap()
            .attack_target
            .is_none(),
        "save/load must not reacquire the released Cell target"
    );
    assert_eq!(
        crate::sim::projectile::cell_target_coord(Some(restored_terrain), target.0, target.1),
        crate::sim::projectile::cell_target_coord(Some(terrain), target.0, target.1),
        "future Cell-target fire uses the same surface after restore"
    );
}

#[test]
#[ignore = "requires the configured retail install and stock Hills.mmx"]
fn retail_grizzly_forcefire_flies_damages_collapses_and_releases_bridge_target() {
    let retail = std::env::var("RA2_DIR")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|_| {
            crate::util::config::GameConfig::load()
                .unwrap()
                .paths
                .ra2_dir
        });
    let mut scenario = crate::headless_scenario::load(&retail, "Hills.mmx", 0x0B21_D6E5)
        .expect("production retail Hills load");
    // The headless loader omits the app's retained restore template. Capture
    // the pristine production load before any spawn or damage, as the app does.
    let pristine_terrain = scenario.sim().resolved_terrain.as_ref().unwrap().clone();
    let owner = scenario
        .sim()
        .session
        .house_order
        .iter()
        .copied()
        .find(|id| {
            scenario
                .sim()
                .houses
                .get(id)
                .is_some_and(|house| house.is_human)
        })
        .expect("ordinary human launch house");
    let owner_name = scenario.sim().interner.resolve(owner).to_owned();
    let sites = firing_sites(&scenario);
    assert!(
        !sites.is_empty(),
        "stock map must supply a bank-to-deck shot"
    );
    let mut selected = None;
    for (bank, target) in sites {
        let SimRuntime {
            simulation,
            resources,
        } = &mut scenario.runtime;
        if let Some(attacker) = simulation.spawn_object(
            "MTNK",
            &owner_name,
            bank.0,
            bank.1,
            0,
            &resources.rules,
            &resources.height_map,
        ) {
            selected = Some((attacker, bank, target));
            break;
        }
    }
    let (attacker, bank, target) = selected.expect("place a Grizzly on a loaded bank");
    let SimRuntime {
        simulation,
        resources,
    } = &mut scenario.runtime;
    simulation.resolve_type_handles(&resources.rules);
    let vehicle = resources.rules.object("MTNK").unwrap();
    assert_eq!(vehicle.primary.as_deref(), Some("105mm"));
    let weapon = resources.rules.weapon("105mm").unwrap();
    assert_eq!(weapon.projectile.as_deref(), Some("Cannon"));
    assert!(
        resources
            .rules
            .warhead(weapon.warhead.as_deref().unwrap())
            .unwrap()
            .wall
    );
    let expected_aim = crate::sim::projectile::cell_target_coord(
        simulation.resolved_terrain.as_ref(),
        target.0,
        target.1,
    );
    println!(
        "retail bridge shot: MTNK {attacker}, {bank:?} -> {target:?}, aim {expected_aim:?}, BridgeStrength {}",
        resources.rules.bridge_rules.strength
    );

    let mut shots = 0;
    let mut disappeared = 0;
    let mut flight_observed = false;
    let mut bridge_change_observed = false;
    let mut live: BTreeMap<u64, (u64, ProjectileCoord)> = BTreeMap::new();
    let mut first_launch = None;
    let mut issued = false;
    let mut last_state = None;
    for frame in 0..18000_u64 {
        let commands = if !issued
            || scenario
                .sim()
                .entities()
                .get(attacker)
                .unwrap()
                .attack_target
                .is_none()
        {
            issued = true;
            vec![CommandEnvelope::new(
                owner,
                scenario.sim().session.tick + 1,
                Command::ForceAttackCell {
                    attacker_id: attacker,
                    target_rx: target.0,
                    target_ry: target.1,
                },
            )]
        } else {
            Vec::new()
        };
        let output = scenario
            .runtime
            .advance_frame(&commands, SIM_TICK_MS, TickLane::Ordinary)
            .expect("ordinary production frame");
        assert!(
            output.tick.frame_committed,
            "scenario exited before the bridge chain completed"
        );
        for fire in output
            .fire_events
            .iter()
            .filter(|fire| fire.attacker_id == attacker)
        {
            assert_eq!(fire.target, TargetKind::Cell(target.0, target.1));
            assert_eq!(scenario.sim().interner.resolve(fire.weapon_id), "105mm");
            shots += 1;
        }
        for (&id, &(launched_at, previous)) in &live {
            if let Some(shell) = scenario.sim().projectiles.get(id) {
                flight_observed |= shell.position != previous;
                assert!(
                    frame - launched_at < 300,
                    "Cannon remains in flight for 300 frames: id={id}, launch={:?}, aim={:?}, now={:?}, velocity={:?}, trajectory={:?}",
                    shell.launch_origin,
                    shell.launch_target,
                    shell.position,
                    shell.velocity,
                    shell.trajectory
                );
            } else {
                disappeared += 1;
            }
        }
        live.retain(|id, _| scenario.sim().projectiles.get(*id).is_some());
        for (id, shell) in scenario
            .sim()
            .projectiles
            .iter()
            .filter(|(_, shell)| shell.source_id == attacker)
        {
            first_launch.get_or_insert(frame);
            live.entry(*id).or_insert_with(|| {
                assert_eq!(
                    shell.target,
                    ProjectileTarget::Cell {
                        rx: target.0,
                        ry: target.1
                    }
                );
                assert_eq!(shell.launch_target, expected_aim);
                (frame, shell.position)
            });
        }
        assert!(
            frame < 300 || first_launch.is_some(),
            "the ordinary force-fire command never launched"
        );
        let cell = scenario
            .sim()
            .resolved_terrain
            .as_ref()
            .unwrap()
            .cell(target.0, target.1)
            .unwrap();
        let state = (
            cell.bridge_facts.raw_flags,
            cell.bridge_facts.state_byte,
            cell.final_tile_index,
        );
        if let Some(previous) = last_state {
            if previous != state {
                bridge_change_observed = true;
                println!(
                    "frame {frame}, shots {shots}, ended bullets {disappeared}: bridge {previous:?} -> {state:?}"
                );
            }
        }
        last_state = Some(state);
        if !cell.bridge_facts.has_structural_bridge() {
            assert!(
                output.tick.bridge_state_changed,
                "collapse must reach the frame notification"
            );
            assert!(shots > 0 && disappeared > 0 && flight_observed);
            assert!(bridge_change_observed);
            assert!(
                !scenario
                    .sim()
                    .bridge_state
                    .as_ref()
                    .unwrap()
                    .is_bridge_walkable(target.0, target.1)
            );
            assert!(
                scenario
                    .sim()
                    .entities()
                    .get(attacker)
                    .unwrap()
                    .attack_target
                    .is_none(),
                "successful bridge destruction must detach the force-fire Cell target"
            );
            println!(
                "retail Hills ordinary MTNK chain reached frame {frame}: {shots} shots, {disappeared} ended bullets, flight + bridge mutation + collapse + target release; validating save/restore"
            );
            assert_collapsed_bridge_restores(&scenario, &pristine_terrain, target, attacker);
            println!(
                "retail bridge save/restore preserves native flags, runtime surfaces, navigation, released target and future fire aim"
            );
            return;
        }
    }
    panic!(
        "retail bridge never collapsed: bank={bank:?}, target={target:?}, shots={shots}, ended={disappeared}, flight={flight_observed}, state={last_state:?}, live={live:?}"
    );
}
