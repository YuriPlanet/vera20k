//! Production composition witness: a retail Grizzly force-fires a loaded high
//! bridge through ordinary commands and bound runtime frames. Native scalar
//! comparisons live in bridge_launch_tests and bridge_damage_admission.json;
//! this test does not claim a native whole-flight or whole-match comparison.

use super::TargetKind;
use crate::headless_scenario::{HeadlessScenario, SIM_TICK_MS};
use crate::map::bridge_facts::BridgeStampSlot;
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
    let mut sites = Vec::new();
    for target in terrain.cells() {
        // A structural span body: neither the self-anchor nor the opposite slot.
        if !target.bridge_facts.has_structural_bridge()
            || target.slope_type != 0
            || target.bridge_facts.is_anchor_self()
            || target
                .bridge_facts
                .anchor
                .is_some_and(|relation| relation.slot == BridgeStampSlot::Opposite)
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

fn restore_saved_scenario(
    scenario: &HeadlessScenario,
    template: &crate::map::resolved_terrain::ResolvedTerrainGrid,
    bytes: &[u8],
) -> crate::sim::world::Simulation {
    let live = scenario.sim();
    let resources = &scenario.runtime.resources;
    let map_hash = scenario.map.ini.content_hash();
    let rules_hash = resources.rules.simulation_config_hash();
    let mut restored =
        GameSnapshot::load_validated(bytes, map_hash, rules_hash, &live.session.map_name)
            .expect("saved retail scenario validates")
            .sim;
    assert_eq!(
        bincode::serialize(&restored.projectiles).unwrap(),
        bincode::serialize(&live.projectiles).unwrap(),
        "serialized Bullet fields survive before native load fixups"
    );

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
        &resources.rules,
    );
    restored
        .restore_map_authority_after_snapshot_load(&resources.rules, &resources.overlay_registry)
        .expect("saved map and bridge authority restore");
    restored.resolve_type_handles(&resources.rules);
    restored.rebuild_lighting_sources_after_load(&resources.rules);
    restored
}

fn assert_collapsed_bridge_restores(
    scenario: &HeadlessScenario,
    template: &crate::map::resolved_terrain::ResolvedTerrainGrid,
    target: (u16, u16),
    attacker: u64,
) -> crate::sim::world::Simulation {
    let live = scenario.sim();
    assert!(
        template
            .cell(target.0, target.1)
            .unwrap()
            .bridge_facts
            .has_structural_bridge(),
        "restoration must start from the original intact map"
    );
    let bytes = GameSnapshot::save_validated(
        live,
        scenario.map.ini.content_hash(),
        scenario.runtime.resources.rules.simulation_config_hash(),
        "Retail bridge collapse",
        0,
    );
    let restored = restore_saved_scenario(scenario, template, &bytes);

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
    for (&id, original) in live.anims() {
        let loaded = restored
            .anim(id)
            .expect("every saved live AnimClass restores");
        assert_eq!(loaded.type_id, original.type_id);
        assert_eq!(loaded.world_coord, original.world_coord);
        assert_eq!(loaded.bounce, original.bounce, "BounceClass body {id}");
        assert_eq!(
            loaded.runtime, original.runtime,
            "AnimClass timer/lifecycle {id}"
        );
    }
    restored
}

/// Follow the saved live Bullet through the same restoration transaction as
/// collapse/debris. Native load restarts Scenario RNG and the Bullet arm timer;
/// compare two restored futures rather than comparing with the unsaved future.
fn assert_live_projectile_restore_continuation(
    scenario: &mut HeadlessScenario,
    template: &crate::map::resolved_terrain::ResolvedTerrainGrid,
    projectile_id: u64,
) {
    use crate::sim::world::display_layers::DisplayLayer;

    let saved_shell = scenario
        .sim()
        .projectiles
        .get(projectile_id)
        .expect("save contains the already-moved live Bullet")
        .clone();
    assert_ne!(saved_shell.position, saved_shell.launch_origin);
    assert!(saved_shell.in_logic_vector);
    assert_eq!(
        scenario.sim().display_layers().layer_of(projectile_id),
        Some(DisplayLayer::AIR)
    );
    let saved_display = bincode::serialize(scenario.sim().display_layers()).unwrap();
    let live_hash = scenario.sim().state_hash();
    let rules_hash = scenario.runtime.resources.rules.simulation_config_hash();
    let bytes = GameSnapshot::save_validated(
        scenario.sim(),
        scenario.map.ini.content_hash(),
        rules_hash,
        "Retail Bullet in flight toward bridge",
        0,
    );
    let first = restore_saved_scenario(scenario, template, &bytes);
    let mut second = restore_saved_scenario(scenario, template, &bytes);
    for restored in [&first, &second] {
        assert_eq!(
            restored.native_unique_ids.as_ref().unwrap().current_raw(),
            scenario
                .sim()
                .native_unique_ids
                .as_ref()
                .unwrap()
                .current_raw(),
            "the restored Bullet's future siblings share the saved native cursor"
        );
        let mut retained = restored
            .projectiles
            .get(projectile_id)
            .expect("saved live Bullet restores")
            .clone();
        // Original BulletLoad46AE9C..46AEB0 restarts the timer. Every other
        // retained field, including binary64 motion, visual bytes and target,
        // must remain identical after the complete production restore.
        assert_eq!(
            retained.arm_timer,
            crate::sim::timer::CdTimer::started(restored.session.binary_frame as i32, 0)
        );
        retained.arm_timer = saved_shell.arm_timer;
        assert_eq!(retained, saved_shell);
        assert_eq!(
            bincode::serialize(restored.display_layers()).unwrap(),
            saved_display,
            "all five retained Display vectors preserve membership and order"
        );
        assert_eq!(
            restored.display_layers().layer_of(projectile_id),
            Some(DisplayLayer::AIR)
        );
    }
    assert_eq!(first.state_hash(), second.state_hash());

    // SimRuntime::advance_frame borrows every bound SimResources input
    // immutably. Swap only simulations; preserve the actual unsaved world so
    // the original force-fire/collapse witness resumes from this exact frame.
    let live = std::mem::replace(&mut scenario.runtime.simulation, first);
    let mut moved = false;
    let mut retired_at = None;
    for frame in 1..=300 {
        let first_output = scenario
            .runtime
            .advance_frame(&[], SIM_TICK_MS, TickLane::Ordinary)
            .expect("first restored Bullet continuation frame");
        assert!(first_output.tick.frame_committed);
        if let Some(shell) = scenario.sim().projectiles.get(projectile_id) {
            moved |= shell.position != saved_shell.position;
        }
        std::mem::swap(&mut scenario.runtime.simulation, &mut second);
        let second_output = scenario
            .runtime
            .advance_frame(&[], SIM_TICK_MS, TickLane::Ordinary)
            .expect("second restored Bullet continuation frame");
        assert!(second_output.tick.frame_committed);
        assert_eq!(
            scenario.sim().state_hash(),
            second.state_hash(),
            "restored Bullet continuation frame {frame}"
        );
        assert_eq!(
            scenario.sim().projectiles.get(projectile_id),
            second.projectiles.get(projectile_id),
            "same restored Bullet fields at frame {frame}"
        );
        if scenario.sim().projectiles.get(projectile_id).is_none() {
            for restored in [scenario.sim(), &second] {
                assert_eq!(restored.display_layers().layer_of(projectile_id), None);
                assert!(
                    !restored
                        .display_layers()
                        .ordered_ids()
                        .any(|&id| id == projectile_id),
                    "retired Bullet leaves every Display vector"
                );
            }
            retired_at = Some(frame);
            break;
        }
        std::mem::swap(&mut scenario.runtime.simulation, &mut second);
    }
    scenario.runtime.simulation = live;
    assert_eq!(scenario.sim().state_hash(), live_hash);
    assert_eq!(
        scenario.runtime.resources.rules.simulation_config_hash(),
        rules_hash,
        "restored continuations retain the bound rules"
    );
    assert!(moved, "restored Bullet must move beyond its saved position");
    let retired_at = retired_at.expect("restored Bullet must retire within 300 frames");
    println!(
        "saved Bullet {projectile_id} at {:?} resumed flight and retired after {retired_at} ordinary frames; both restored state-hash sequences matched, Display membership was removed, original live frame preserved",
        saved_shell.position
    );
}

/// Save/load resets Scenario RNG in native. Compare two independently validated
/// restores through ordinary frames, not a loaded stream against an unsaved
/// live stream with intentionally different RNG. The saved flight body/timers
/// must survive the actual map/cache restoration before either continuation.
fn assert_debris_restore_continuation(
    scenario: &mut HeadlessScenario,
    template: &crate::map::resolved_terrain::ResolvedTerrainGrid,
    target: (u16, u16),
    attacker: u64,
) {
    let first = assert_collapsed_bridge_restores(scenario, template, target, attacker);
    let mut second = assert_collapsed_bridge_restores(scenario, template, target, attacker);
    let flying = first
        .anims()
        .filter_map(|(&id, anim)| anim.bounce.is_some().then_some(id))
        .collect::<Vec<_>>();
    assert!(!flying.is_empty(), "save must contain live Bouncer flight");
    assert_eq!(
        first.state_hash(),
        second.state_hash(),
        "independent validated restores"
    );
    scenario.runtime.simulation = first;
    let mut observed_motion = false;
    for frame in 0..200 {
        let before = flying
            .iter()
            .filter_map(|&id| {
                scenario
                    .sim()
                    .anim_absolute_coord(id)
                    .map(|coord| (id, coord))
            })
            .collect::<Vec<_>>();
        scenario
            .runtime
            .advance_frame(&[], SIM_TICK_MS, TickLane::Ordinary)
            .expect("first restored continuation frame");
        for (id, coord) in before {
            observed_motion |= scenario
                .sim()
                .anim_absolute_coord(id)
                .is_some_and(|after| after != coord);
        }
        std::mem::swap(&mut scenario.runtime.simulation, &mut second);
        scenario
            .runtime
            .advance_frame(&[], SIM_TICK_MS, TickLane::Ordinary)
            .expect("second restored continuation frame");
        assert_eq!(
            scenario.sim().state_hash(),
            second.state_hash(),
            "restored debris continuation frame {frame}"
        );
        std::mem::swap(&mut scenario.runtime.simulation, &mut second);
    }
    assert!(
        observed_motion,
        "restored Bouncer bodies must resume flight"
    );
    assert!(
        flying.iter().all(|&id| scenario.sim().anim(id).is_none()),
        "restored chunks must finish their flight/contact/landing lifecycle"
    );
    println!(
        "{} restored Bouncers flew and expired; both validated restores matched all 200 state hashes, final {:016x}",
        flying.len(),
        scenario.sim().state_hash()
    );
}

#[test]
#[ignore = "requires the configured retail install and stock Hills.mmx"]
fn retail_grizzly_forcefire_flies_damages_collapses_and_releases_bridge_target() {
    retail_bridge_forcefire_chain("MTNK", "105mm", "Cannon");
}

#[test]
#[ignore = "requires the configured retail install and stock Hills.mmx"]
fn retail_ifv_forcefire_guidance_survives_restore_and_collapses_live_bridge() {
    retail_bridge_forcefire_chain("FV", "HoverMissile", "AAHeatSeeker2");
}

fn retail_bridge_forcefire_chain(vehicle_name: &str, weapon_name: &str, projectile_name: &str) {
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
            vehicle_name,
            &owner_name,
            bank.0,
            bank.1,
            0,
            &resources.rules,
        ) {
            selected = Some((attacker, bank, target));
            break;
        }
    }
    let (attacker, bank, target) = selected.expect("place the selected vehicle on a loaded bank");
    let SimRuntime {
        simulation,
        resources,
    } = &mut scenario.runtime;
    simulation.resolve_type_handles(&resources.rules);
    let weapon = resources.rules.weapon(weapon_name).unwrap();
    assert_eq!(weapon.projectile.as_deref(), Some(projectile_name));
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
        "retail bridge shot: {vehicle_name} {attacker}, {bank:?} -> {target:?}, aim {expected_aim:?}, BridgeStrength {}",
        resources.rules.bridge_rules.strength
    );

    let mut shots = 0;
    let mut disappeared = 0;
    let mut flight_observed = false;
    let mut live_flight_restore_checked = false;
    let mut bridge_change_observed = false;
    let mut live: BTreeMap<u64, (u64, ProjectileCoord)> = BTreeMap::new();
    let mut first_launch = None;
    let mut issued = false;
    let mut last_state = None;
    let guided = resources.rules.projectile(projectile_name).unwrap().rot > 0;
    let mut trails = std::collections::BTreeSet::new();
    let mut detached_trails = 0;
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
        for event in &output.lifecycle_outputs {
            use crate::sim::world::LifecycleOutput;
            match event {
                LifecycleOutput::LineTrailConstructed { stable_id, .. }
                    if scenario
                        .sim()
                        .projectiles
                        .get(*stable_id)
                        .is_some_and(|bullet| bullet.source_id == attacker) =>
                {
                    assert!(trails.insert(*stable_id), "one trail per Bullet lifetime");
                }
                LifecycleOutput::LineTrailDetached { stable_id } if trails.contains(stable_id) => {
                    detached_trails += 1;
                    assert!(
                        scenario.sim().projectiles.get(*stable_id).is_none(),
                        "trail detach follows the physical deferred Bullet destructor"
                    );
                }
                _ => {}
            }
        }
        for fire in output
            .fire_events
            .iter()
            .filter(|fire| fire.attacker_id == attacker)
        {
            assert_eq!(fire.target, TargetKind::Cell(target.0, target.1));
            assert_eq!(scenario.sim().interner.resolve(fire.weapon_id), weapon_name);
            shots += 1;
        }
        for (&id, &(launched_at, previous)) in &live {
            if let Some(shell) = scenario.sim().projectiles.get(id) {
                flight_observed |= shell.position != previous;
                assert!(
                    frame - launched_at < 300,
                    "{projectile_name} remains in flight for 300 frames: id={id}, launch={:?}, aim={:?}, now={:?}, velocity={:?}, trajectory={:?}",
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
        if !live_flight_restore_checked {
            let moved_projectile = scenario.sim().projectiles.iter().find_map(|(&id, shell)| {
                (shell.source_id == attacker
                    && shell.position != shell.launch_origin
                    && scenario
                        .runtime
                        .resources
                        .rules
                        .weapon(scenario.sim().interner.resolve(shell.payload.weapon))
                        .and_then(|weapon| weapon.projectile.as_deref())
                        == Some(projectile_name))
                .then_some(id)
            });
            if let Some(id) = moved_projectile {
                assert_live_projectile_restore_continuation(&mut scenario, &pristine_terrain, id);
                live_flight_restore_checked = true;
            }
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
            assert!(live_flight_restore_checked);
            if guided {
                assert!(
                    trails.len() >= 2 && detached_trails > 0,
                    "both burst shots construct trails and completed bullets detach them"
                );
            }
            assert!(
                !scenario
                    .sim()
                    .resolved_terrain
                    .as_ref()
                    .unwrap()
                    .cell(target.0, target.1)
                    .unwrap()
                    .bridge_facts
                    .has_structural_bridge()
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
                "retail Hills ordinary {vehicle_name} chain reached frame {frame}: {shots} shots, {disappeared} ended bullets, flight + bridge mutation + collapse + target release; validating save/restore"
            );
            assert_debris_restore_continuation(&mut scenario, &pristine_terrain, target, attacker);
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
