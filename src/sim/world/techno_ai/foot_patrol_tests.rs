//! Patrol through production admission, frames, Infantry dispatch and save/load.
use super::*;
use crate::rules::ini_parser::IniFile;
use crate::sim::mission::state::MissionTestFixture;
use crate::sim::mission::{MissionDispatchTimer, MissionId};
use crate::sim::scenario_session::ScenarioGameMode;
/// Parsed MISSION column -> concrete scenario placement -> ordinary object-AI
/// frames. This cropped arena does not claim an original ScenarioLoad replay.
#[test]
fn map_authored_patrol_acquires_and_returns_during_ordinary_frames() {
    let Some(rules) = crate::rules::retail_ini_fixture::retail_unlayered_rules_with_art() else {
        return;
    };
    let mut sim = crate::sim::arena_fixture::rival_arena(&rules, 19);
    sim.session.set_game_mode(ScenarioGameMode::Skirmish);
    sim.session.game_options_mut_for_test().short_game = false;
    sim.intern_rule_type_ids(&rules);
    sim.resolve_type_handles(&rules);
    let placements = crate::map::entities::parse_map_entities(&IniFile::from_str(
        "[Units]\n0=Americans,MTNK,256,10,10,0,Patrol,None,0,-1,0,0,1,1\n",
    ));
    assert_eq!(placements[0].mission, Some(MissionType::Patrol));
    assert_eq!(sim.spawn_from_map(&placements, Some(&rules)), 1);
    let id = sim.substrate.entities.iter_sorted().next().unwrap().0;
    let enemy = sim
        .spawn_object("HTNK", "Russians", 15, 10, 0, &rules)
        .unwrap();
    // ShortGame survival and target expiry must not terminate this scene.
    sim.spawn_object("GAPOWR", "Americans", 3, 3, 0, &rules)
        .unwrap();
    sim.spawn_object("NAPOWR", "Russians", 26, 26, 0, &rules)
        .unwrap();
    let owner = sim.substrate.entities.get(id).unwrap().owner();
    sim.fog.mark_visible_for_owner(owner, 15, 10);
    // A retained rally destination makes the return chain observable when
    // the quarry expires. Patrol archives it through its ordinary receiver.
    assert!(
        sim.assign_destination_represented(
            id,
            Some(NavTargetRef::cell(12, 14)),
            &rules,
            None,
            Default::default()
        )
        .is_ok()
    );
    let path = sim.path_grid().unwrap().clone();
    let mut acquired = false;
    for _ in 0..90 {
        assert!(
            sim.advance_tick(&[], Some(&rules), Some(&path), None, 16)
                .frame_committed
        );
        let actor = sim.substrate.entities.get(id).unwrap();
        if actor.mission.current().known() == Some(MissionType::Patrol)
            && actor.mission.handler_state() == 1
        {
            assert_eq!(
                actor.attack_target.target(),
                Some(TargetKind::Entity(enemy))
            );
            assert!(actor.patrol_post.cell().is_some());
            assert_eq!(actor.archive_target(), Some(TargetKind::Cell(12, 14)));
            acquired = true;
            break;
        }
    }
    assert!(
        acquired,
        "authored Patrol reaches its due handler and acquires"
    );
    sim.uninit_with_rules(enemy, &rules);
    let mut returned = false;
    for _ in 0..600 {
        assert!(
            sim.advance_tick(&[], Some(&rules), Some(&path), None, 16)
                .frame_committed
        );
        let actor = sim.substrate.entities.get(id).unwrap();
        if actor.patrol_post.cell().is_none()
            && actor.archive_target().is_none()
            && actor.navigation.nav_com().is_none()
            && (actor.position.rx(), actor.position.ry()) == (12, 14)
        {
            assert_eq!(actor.attack_target.target(), None);
            assert!(matches!(actor.mission.handler_state(), 0 | 3));
            returned = true;
            break;
        }
    }
    assert!(
        returned,
        "ordinary Approach/return frames consume the saved post/archive"
    );
}

fn set_patrol(sim: &mut Simulation, id: u64, state: u32) {
    let now = sim.session.binary_frame();
    sim.substrate
        .entities
        .get_mut(id)
        .unwrap()
        .mission
        .apply_test_fixture(MissionTestFixture {
            current: MissionId::from_known(MissionType::Patrol),
            suspended: MissionId::NONE,
            queued: MissionId::NONE,
            movement_bypass_latch: 0,
            handler_state: state,
            mission_start_frame: now,
            ai_counter: 0,
            dispatch_timer: MissionDispatchTimer::at_frame(now),
        });
}

#[test]
fn stock_sniper_patrol_scans_then_approaches_through_the_existing_owners() {
    let Some(rules) = crate::rules::retail_ini_fixture::retail_unlayered_rules_with_art() else {
        return;
    };
    let (kind, enemy_kind) = ("SNIPE", "E1");
    {
        let mut sim = crate::sim::arena_fixture::rival_arena(&rules, 17);
        sim.session.set_game_mode(ScenarioGameMode::Skirmish);
        sim.session.game_options_mut_for_test().short_game = false;
        sim.intern_rule_type_ids(&rules);
        sim.resolve_type_handles(&rules);
        let id = sim
            .spawn_object(kind, "Americans", 10, 10, 0, &rules)
            .unwrap();
        let enemy = sim
            .spawn_object(enemy_kind, "Russians", 12, 10, 0, &rules)
            .unwrap();
        let owner = sim.substrate.entities.get(id).unwrap().owner();
        sim.fog.mark_visible_for_owner(owner, 12, 10);
        set_patrol(&mut sim, id, 0);
        super::super::mission_handlers::dispatch_foot_mission(
            &mut sim,
            id,
            &rules,
            ObjectAiCtx::default(),
        );
        let actor = sim.substrate.entities.get(id).unwrap();
        assert_eq!(actor.mission.handler_state(), 1, "{kind}");
        assert_eq!(
            actor.attack_target.target(),
            Some(TargetKind::Entity(enemy)),
            "{kind}"
        );
        let post = actor.patrol_post.cell();
        assert!(post.is_some());
        assert_eq!(actor.archive_target(), Some(TargetKind::Cell(10, 10)));
        set_patrol(&mut sim, id, 1);
        super::super::mission_handlers::dispatch_foot_mission(
            &mut sim,
            id,
            &rules,
            ObjectAiCtx::default(),
        );
        let actor = sim.substrate.entities.get(id).unwrap();
        assert_eq!(actor.mission.handler_state(), 1, "{kind}");
        assert_eq!(
            actor.attack_target.target(),
            Some(TargetKind::Entity(enemy)),
            "{kind}"
        );
        assert_eq!(actor.patrol_post.cell(), post);
    }
}

fn continuation_world(state: u32) -> (Simulation, RuleSet, u64, u64) {
    let rules=RuleSet::from_ini(&crate::sim::arena_fixture::clear_ground_ini(
        "[VehicleTypes]\n0=MOVER\n[MOVER]\nStrength=100\nSpeed=6\nSpeedType=Wheel\nMovementZone=Normal\nLocomotor={4A582741-9839-11d1-B709-00A024DDAFD1}\n"
    )).unwrap();
    let mut sim = crate::sim::arena_fixture::rival_arena(&rules, 1);
    sim.session.set_game_mode(ScenarioGameMode::Skirmish);
    sim.session.game_options_mut_for_test().short_game = false;
    sim.intern_rule_type_ids(&rules);
    sim.resolve_type_handles(&rules);
    let id = sim
        .spawn_object("MOVER", "Americans", 10, 10, 0, &rules)
        .unwrap();
    let target = sim
        .spawn_object("MOVER", "Russians", 12, 10, 0, &rules)
        .unwrap();
    let post = sim
        .resolved_terrain
        .as_ref()
        .unwrap()
        .native_fixed_cell_index(10, 10)
        .map(NativeCellIdentity::Real);
    set_patrol(&mut sim, id, state);
    let actor = sim.substrate.entities.get_mut(id).unwrap();
    actor.patrol_post.cell = post;
    actor
        .attack_target
        .set_for_test((state == 1).then_some(TargetKind::Entity(target)));
    actor.set_archive_target(Some(TargetKind::Cell(15, 15)));
    actor.navigation.set_nav_com_for_test(None);
    actor
        .locomotor
        .as_mut()
        .unwrap()
        .install_drive_state_for_test(Some(
            crate::sim::movement::DriveLocomotionRuntime::default()
                .with_destination_for_test(None)
                .with_head_to_for_test(None),
        ));
    (sim, rules, id, target)
}
#[test]
fn patrol_saved_pursuit_and_return_continue_in_independent_worlds() {
    for state in [1, 2] {
        let (mut live, rules, id, _) = continuation_world(state);
        // Native Load689470 resets Scenario RNG to Seed(0). The reference
        // side starts at that same post-load cursor, not its saved Seed(1).
        live.scenario_rng = crate::sim::rng::SimRng::new(0);
        let terrain = live.resolved_terrain.as_ref().unwrap().clone();
        let bytes = crate::sim::snapshot::GameSnapshot::save(&live, 0, 0, "patrol.map", 0);
        let mut restored = crate::sim::snapshot::GameSnapshot::load(&bytes)
            .unwrap()
            .sim;
        restored.restore_after_snapshot_load().unwrap();
        restored.rebuild_caches_after_load(terrain, Default::default(), &rules);
        assert!(restored.rebuild_dynamic_navigation(&rules));
        assert_eq!(
            restored.substrate.entities.get(id).unwrap().patrol_post,
            live.substrate.entities.get(id).unwrap().patrol_post
        );
        assert!(
            !live
                .shared_cell_dummy
                .same_identity(&restored.shared_cell_dummy)
        );
        // State1 uses actual Approach on the retained quarry; state2 scans
        // an unarmed actor, then hands back the archive. No query seam.
        for _ in 0..3 {
            super::super::mission_handlers::dispatch_foot_mission(
                &mut live,
                id,
                &rules,
                ObjectAiCtx::default(),
            );
            super::super::mission_handlers::dispatch_foot_mission(
                &mut restored,
                id,
                &rules,
                ObjectAiCtx::default(),
            );
            let a = live.substrate.entities.get(id).unwrap();
            let b = restored.substrate.entities.get(id).unwrap();
            assert!(
                b.mission.dispatch_timer().delay() > 0,
                "the due Patrol handler must actually run"
            );
            assert_eq!(a.patrol_post, b.patrol_post, "state{state}");
            assert_eq!(a.attack_target.target(), b.attack_target.target());
            assert_eq!(a.archive_target(), b.archive_target());
            assert_eq!(a.navigation.nav_com(), b.navigation.nav_com());
            assert_eq!(a.mission.handler_state(), b.mission.handler_state());
            assert_eq!(a.mission.dispatch_timer(), b.mission.dispatch_timer());
            assert_eq!(
                live.scenario_rng.logical_state(),
                restored.scenario_rng.logical_state()
            );
            // Advance only the mission clock here; physical tick continuation
            // is covered separately by the authored-placement frame test.
            let frame = live.session.binary_frame().wrapping_add(60);
            live.session.set_binary_frame_for_test(frame);
            restored.session.set_binary_frame_for_test(frame);
        }
    }
}
