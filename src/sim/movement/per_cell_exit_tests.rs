//! Foot PerCell4D8D67..4D8DA4: a promoted mobile outside the playfield
//! asks its class IsLeavingMap before UnInit, then skips the Techno tail.

use super::*;
use crate::rules::team_ai_ini::TeamAiDefinitionSource;
use crate::sim::combat::TargetKind;
use crate::sim::team_script_vm::{TeamScriptAction, TeamScriptDefinition};

const PLAYFIELD: crate::sim::cell_rect::PlayfieldBounds =
    crate::sim::cell_rect::PlayfieldBounds::from_normalized_local_size(20, 0, 0, 20, 8);

fn retail_rules() -> Option<RuleSet> {
    crate::rules::retail_ini_fixture::retail_battle_rules_for_map("XMP03T4.MAP")
        .map(|retail| retail.rules)
}

fn arena(rules: &RuleSet) -> Simulation {
    let mut sim = crate::sim::arena_fixture::rival_arena(rules, 0x4DC790);
    sim.intern_rule_type_ids(rules);
    sim.resolve_type_handles(rules);
    sim.session.game_options_mut_for_test().short_game = false;
    sim
}

fn grizzly(sim: &mut Simulation, rules: &RuleSet, cell: (u16, u16)) -> u64 {
    let id = sim
        .spawn_object_at_height("MTNK", "Russians", cell.0, cell.1, 0, 0, rules)
        .unwrap();
    assert!(sim.substrate.entities.get(id).unwrap().in_playfield());
    id
}

fn team(sim: &mut Simulation, id: u64, action: TeamScriptAction) -> u64 {
    let owner = sim.substrate.entities.get(id).unwrap().owner();
    let script = sim.interner.intern("EXIT-CONTROL");
    sim.team_script_vm.register_script(TeamScriptDefinition {
        id: script,
        source: TeamAiDefinitionSource::FixedAimd,
        actions: vec![action],
    });
    sim.team_script_vm
        .create_team(owner, script, vec![id], sim.session.binary_frame() as i32)
}

/// The stored membership is the earlier promotion, not a fresh geometry test.
/// Supply that valid exit entry state through Unlimbo in the open arena,
/// then install the bounded scene's playfield before its arrival callback.
#[test]
fn a_promoted_grizzly_leaving_the_playfield_uninitializes_and_expires_targets() {
    let Some(rules) = retail_rules() else {
        return;
    };
    let mut sim = arena(&rules);
    let id = grizzly(&mut sim, &rules, (20, 20));
    let observer = sim
        .spawn_object_at_height("E1", "Americans", 14, 14, 0, 0, &rules)
        .unwrap();
    sim.assign_target_represented(
        observer,
        Some(TargetKind::Entity(id)),
        Some(&rules),
        None,
        FrameEffects::default(),
    )
    .unwrap();
    assert_eq!(
        sim.substrate
            .entities
            .get(observer)
            .unwrap()
            .attack_target
            .target(),
        Some(TargetKind::Entity(id)),
    );
    sim.set_playfield_bounds_for_test(Some(PLAYFIELD));
    assert!(!crate::sim::cell_rect::cell_is_in_playfield_height_aware(
        (20, 20),
        sim.playfield_bounds(),
        sim.resolved_terrain.as_ref(),
    ));
    let health = sim.substrate.entities.get(id).unwrap().health.current();
    // Native PointerExpired7079D1..707A0D shortens a targeter's still-long
    // passive scan timer with the same Scenario(4,8) semantics as70F770.
    // Rust shares that timer owner. An elapsed timer here avoids that draw.
    let now = sim.session.binary_frame();
    sim.substrate
        .entities
        .get_mut(observer)
        .unwrap()
        .passive_scan
        .arm_timer(now, 0);
    let rng = sim.rng_state();

    sim.per_cell_process(
        id,
        PerCellReason::Arrival,
        Some(&rules),
        None,
        FrameEffects::default(),
    )
    .unwrap();

    let actor = sim.substrate.entities.get(id).unwrap();
    assert!(!actor.lifecycle.object_alive(), "native calls UnInit");
    assert!(actor.lifecycle.in_limbo());
    assert!(!actor.lifecycle.cell_marked());
    assert_eq!(actor.health.current(), health, "this is not a killing hit");
    assert_eq!(sim.substrate.pending_delete.object_ids(), [id]);
    assert_eq!(
        sim.substrate
            .entities
            .get(observer)
            .unwrap()
            .attack_target
            .target(),
        None,
    );
    assert_eq!(sim.rng_state(), rng);
}

#[test]
fn inside_ingress_guard_team_and_turn_completion_do_not_remove_the_grizzly() {
    let Some(rules) = retail_rules() else {
        return;
    };
    for (name, cell, promoted, guard_team, reason) in [
        ("inside", (12, 12), true, false, PerCellReason::Arrival),
        ("ingress", (20, 20), false, false, PerCellReason::Arrival),
        ("guard team", (20, 20), true, true, PerCellReason::Arrival),
        ("turn", (20, 20), true, false, PerCellReason::TurnComplete),
    ] {
        let mut sim = arena(&rules);
        let id = grizzly(&mut sim, &rules, cell);
        sim.set_playfield_bounds_for_test(Some(PLAYFIELD));
        sim.substrate
            .entities
            .get_mut(id)
            .unwrap()
            .set_in_playfield(promoted);
        if guard_team {
            team(
                &mut sim,
                id,
                TeamScriptAction {
                    action_id: 11,
                    argument: MissionType::Guard as i32,
                },
            );
        }
        let rng = sim.rng_state();
        sim.per_cell_process(id, reason, Some(&rules), None, FrameEffects::default())
            .unwrap();
        let actor = sim.substrate.entities.get(id).unwrap();
        assert!(actor.lifecycle.object_alive(), "{name}");
        assert!(
            sim.substrate.pending_delete.object_ids().is_empty(),
            "{name}"
        );
        assert_eq!(sim.rng_state(), rng, "{name}");
    }
}

#[test]
fn infantry_uses_the_foot_exit_but_an_ordinary_aircraft_keeps_its_own_gate() {
    let Some(rules) = retail_rules() else {
        return;
    };
    for (kind, leaves) in [("E1", true), ("ORCA", false)] {
        let mut sim = arena(&rules);
        let id = sim
            .spawn_object_at_height(kind, "Russians", 20, 20, 0, 0, &rules)
            .unwrap();
        let actor = sim.substrate.entities.get(id).unwrap();
        assert!(actor.in_playfield());
        assert!(!actor.is_mission_only());
        sim.set_playfield_bounds_for_test(Some(PLAYFIELD));
        sim.per_cell_process(
            id,
            PerCellReason::Arrival,
            Some(&rules),
            None,
            FrameEffects::default(),
        )
        .unwrap();
        assert_eq!(
            sim.substrate
                .entities
                .get(id)
                .unwrap()
                .lifecycle
                .object_alive(),
            !leaves,
            "{kind}",
        );
    }
}

/// Rust partial-world compatibility, as in Aircraft's existing no-Map
/// removal control. Missing geometry is not evidence of an off-map position.
#[test]
fn a_partial_world_without_a_playfield_does_not_admit_exit() {
    let Some(rules) = retail_rules() else {
        return;
    };
    let mut sim = arena(&rules);
    let id = grizzly(&mut sim, &rules, (20, 20));
    sim.set_playfield_bounds_for_test(None);
    sim.per_cell_process(
        id,
        PerCellReason::Arrival,
        Some(&rules),
        None,
        FrameEffects::default(),
    )
    .unwrap();
    assert!(
        sim.substrate
            .entities
            .get(id)
            .unwrap()
            .lifecycle
            .object_alive()
    );
}

/// IsTrain is dormant in the selected retail MTNK section. An authored
/// scenario override exercises Foot4DC7BF..4DC7FD through the same reader
/// and destination owner, without giving the ordinary tank another policy.
#[test]
fn a_train_heading_back_to_the_playfield_stays_but_an_exiting_train_leaves() {
    let Some(retail) = crate::rules::retail_ini_fixture::retail_battle_rules_for_map_with_patch(
        "XMP03T4.MAP",
        &crate::rules::ini_parser::IniFile::from_str("[MTNK]\nIsTrain=yes\n"),
    ) else {
        return;
    };
    let rules = retail.rules;
    assert!(rules.object("MTNK").unwrap().is_train);
    for (destination, leaves) in [((12, 12), false), ((22, 22), true)] {
        let mut sim = arena(&rules);
        let id = grizzly(&mut sim, &rules, (20, 20));
        sim.set_playfield_bounds_for_test(Some(PLAYFIELD));
        sim.assign_destination_represented(
            id,
            Some(NavTargetRef::cell(destination.0, destination.1)),
            Some(&rules),
            None,
            FrameEffects::default(),
        )
        .unwrap();
        sim.per_cell_process(
            id,
            PerCellReason::Arrival,
            Some(&rules),
            None,
            FrameEffects::default(),
        )
        .unwrap();
        assert_eq!(
            sim.substrate
                .entities
                .get(id)
                .unwrap()
                .lifecycle
                .object_alive(),
            !leaves,
            "{destination:?}",
        );
    }
}

/// Start on the playfield; script3 supplies an off-playfield waypoint and
/// the existing Team/path/Drive owners carry the tank through normal ticks.
/// No test write supplies the exit position, membership or removal.
#[test]
fn a_retail_grizzly_team_walks_out_and_retires_through_the_production_tick() {
    let Some(rules) = retail_rules() else {
        return;
    };
    let mut sim = arena(&rules);
    sim.set_playfield_bounds_for_test(Some(PLAYFIELD));
    let id = grizzly(&mut sim, &rules, (12, 12));
    sim.spawn_object_at_height("E1", "Americans", 8, 16, 0, 0, &rules)
        .unwrap();
    sim.session
        .install_waypoints(&std::collections::HashMap::from([(
            0,
            crate::map::waypoints::Waypoint {
                index: 0,
                rx: 22,
                ry: 22,
            },
        )]));
    let team = team(
        &mut sim,
        id,
        TeamScriptAction {
            action_id: 3,
            argument: 0,
        },
    );
    sim.team_script_vm.seek_line_for_test(team, 0);
    let grid = sim.path_grid_snapshot().unwrap();
    let mut moved = false;
    let mut last = None;
    for _ in 0..600 {
        if !sim.substrate.entities.contains(id) {
            break;
        }
        let actor = sim.substrate.entities.get(id).unwrap();
        let cell = (actor.position.rx(), actor.position.ry());
        moved |= cell != (12, 12);
        last = Some((cell, actor.mission.current(), actor.navigation.nav_com()));
        let commands = sim.take_due_commands();
        sim.advance_tick(&commands, Some(&rules), Some(&grid), None, 33);
    }
    assert!(moved, "the Team's Move must run: {last:?}");
    assert!(
        !sim.substrate.entities.contains(id),
        "the exiting tank must retire: {last:?}",
    );
    assert!(sim.team_script_vm.team_for_member(id).is_none());
    assert!(
        sim.substrate
            .occupancy
            .validate_memberships(&sim.substrate.entities)
            .is_ok(),
    );
}
