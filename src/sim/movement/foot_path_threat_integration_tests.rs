//! Rust integration of the retained House threat producer with ordinary MTNK
//! Find_Path, its hierarchy/precheck, A* and both native finishing owners.
//! This contrast is not a native whole-route golden; exact arithmetic and
//! coefficient/Team reader/getter controls are pinned in astar_threat_inputs.
use super::*;
use crate::rules::ini_parser::IniFile;
use crate::rules::team_ai_ini::TeamAiIniRegistry;
use crate::sim::house_state::HouseState;
use crate::sim::snapshot::GameSnapshot;
use crate::sim::team_script_vm::TeamScriptVm;
use crate::util::native_x87::NativeF64Bits;

fn request_move(
    sim: &mut Simulation,
    mover: u64,
    rules: &RuleSet,
    registry: &OverlayTypeRegistry,
) -> (FindPathResult, Vec<(u16, u16)>, Vec<u8>) {
    let goal = (20, 15);
    sim.assign_destination_represented(
        mover,
        Some(crate::sim::components::NavTargetRef::cell(goal.0, goal.1)),
        Some(rules),
        Some(registry),
        crate::sim::world::FrameEffects::default(),
    )
    .unwrap();
    let destination = DriveCoord::cell(goal.0, goal.1, 0);
    let request = FootPathRequest::track(
        &sim.substrate.entities,
        mover,
        destination,
        0,
        sim.playfield_bounds,
    )
    .unwrap();
    let rng = sim.rng_state();
    let result = sim
        .foot_find_path(
            &request,
            rules,
            Some(registry),
            crate::sim::world::FrameEffects::default(),
        )
        .unwrap();
    assert_eq!(
        sim.rng_state(),
        rng,
        "ordinary search and finishing draw no RNG"
    );
    let actor = sim.substrate.entities.get(mover).unwrap();
    assert!(
        actor.lifecycle.cell_marked(),
        "Mark1 restores the live Foot"
    );
    assert!(sim.substrate.occupancy.contains_entity(13, 15, mover));
    // Find_Path installed the finished route as Foot+5E0 words from the
    // current cell; no layer is kept per route cell.
    (
        result,
        actor.navigation.path_replay.route_cells(),
        actor.navigation.path_replay.remaining_directions().to_vec(),
    )
}

#[test]
fn live_avoid_threats_membership_reads_admitted_house_grid_through_search_and_snapshot() {
    let (mut sim, rules, registry) = crate::sim::world::entry_test_fixture::fixture_with_rules(
        "[VehicleTypes]\n0=MTNK\n[MTNK]\nStrength=400\nSpeed=6\nSpeedType=Track\nMovementZone=Normal\nThreatPosed=100\nLocomotor={4A582741-9839-11D1-B709-00A024DDAFD1}\n",
    );
    let american = sim.interner.intern("Americans");
    let russian = sim.interner.intern("Russians");
    for owner in [american, russian] {
        sim.houses
            .register(HouseState::new(owner, 0, None, true, 0, 10));
    }
    sim.houses.reorder_for_test(&[american, russian]);
    sim.session
        .set_game_mode(crate::sim::scenario_session::ScenarioGameMode::from_legacy_nonzero(true));
    let mover = sim
        .spawn_object("MTNK", "Americans", 13, 15, 0, &rules)
        .unwrap();
    // Real enemy admission supplies the retained map, without injecting padded
    // values or replacing the production House getter. It is off the direct
    // Move corridor, so occupancy alone cannot explain the route contrast.
    let enemy = sim
        .spawn_object("MTNK", "Russians", 16, 19, 0, &rules)
        .unwrap();
    assert_eq!(
        sim.substrate
            .entities
            .get(enemy)
            .unwrap()
            .cached_spatial_threat(),
        Some(100)
    );
    assert!(sim.house_threat_at_cell(american, (16, 15)).unwrap() > 0);
    assert_eq!(
        sim.substrate
            .entities
            .get(mover)
            .unwrap()
            .navigation
            .path_threat_coefficient(),
        NativeF64Bits::POSITIVE_ZERO
    );
    let aimd = IniFile::from_str(
        "[TeamTypes]\n0=AVOID\n[AVOID]\nScript=MOVE\nTaskForce=TANK\nAvoidThreats=yes\n[ScriptTypes]\n0=MOVE\n[MOVE]\n0=0,0\n[TaskForces]\n0=TANK\n[TANK]\n0=1,MTNK\n",
    );
    let team_registry = TeamAiIniRegistry::from_sources(&aimd, &IniFile::from_str(""), true);
    let (vm, diagnostics) =
        TeamScriptVm::from_ini_registry(&team_registry, &mut sim.interner, &rules);
    assert!(diagnostics.is_empty(), "{diagnostics:?}");
    sim.team_script_vm = vm;
    let team_type = sim.interner.get("AVOID").unwrap();
    let team = sim.create_team(team_type, Some(american), &rules).unwrap();
    assert!(sim.team_add_member(
        team,
        mover,
        false,
        &rules,
        Some(&registry),
        crate::sim::world::FrameEffects::default()
    ));
    assert!(sim.team_script_vm.member_avoids_threats(mover));
    assert_eq!(
        sim.substrate
            .entities
            .get(mover)
            .unwrap()
            .navigation
            .path_threat_coefficient_for_team(sim.team_script_vm.member_avoids_threats(mover)),
        NativeF64Bits::ONE
    );
    let terrain = sim.resolved_terrain.as_ref().unwrap().clone();
    let grids = sim
        .houses
        .iter()
        .map(|(&id, h)| (id, h.spatial_threat_values().to_vec()))
        .collect::<Vec<_>>();
    let bytes = GameSnapshot::save(&sim, 0, 0, "AvoidThreats Move prestate", 0);
    let mut restored = GameSnapshot::load(&bytes).unwrap().sim;
    restored.restore_after_snapshot_load().unwrap();
    restored.rebuild_caches_after_load(
        terrain,
        crate::sim::pathfinding::terrain_speed::TerrainSpeedConfig::default(),
        &rules,
    );
    restored
        .restore_map_authority_after_snapshot_load(&rules, &registry, None)
        .unwrap();
    assert_eq!(
        restored
            .houses
            .iter()
            .map(|(&id, h)| (id, h.spatial_threat_values().to_vec()))
            .collect::<Vec<_>>(),
        grids
    );
    assert!(restored.team_script_vm.member_avoids_threats(mover));
    let positive = request_move(&mut sim, mover, &rules, &registry);
    assert_eq!(positive.0, FindPathResult::Route);
    assert_eq!(
        request_move(&mut restored, mover, &rules, &registry),
        positive,
        "restored retained map/cache/team membership produces the same finished path"
    );
    sim.team_remove_member(
        team,
        mover,
        true,
        Some(&rules),
        crate::sim::world::FrameEffects::default(),
    );
    assert!(!sim.team_script_vm.member_avoids_threats(mover));
    assert_eq!(
        sim.substrate
            .entities
            .get(mover)
            .unwrap()
            .navigation
            .path_threat_coefficient(),
        NativeF64Bits::POSITIVE_ZERO,
        "Team getter never overwrites Foot+530"
    );
    let ordinary = request_move(&mut sim, mover, &rules, &registry);
    assert_eq!(ordinary.0, FindPathResult::Route);
    assert_ne!(
        positive.1, ordinary.1,
        "live Team override must affect the real search/finishing: {positive:?}"
    );
    assert_eq!(ordinary.1.first().copied(), Some((13, 15)));
    assert_eq!(ordinary.1.last().copied(), Some((20, 15)));
    assert_eq!(
        sim.houses
            .iter()
            .map(|(&id, h)| (id, h.spatial_threat_values().to_vec()))
            .collect::<Vec<_>>(),
        grids,
        "Mark0/1 and Team removal do not rebuild spatial threat"
    );
}
