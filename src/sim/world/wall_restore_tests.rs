//! Stock blocking-wall history and explicit Cell-collection boundary controls.

use crate::sim::combat::TargetKind;
use crate::sim::command::{Command, CommandEnvelope};
use crate::sim::components::NavTargetRef;
use crate::sim::mission::MissionType;

/// Drive4B3BE9 suspends the Move to attack an uncrushable wall. The bullet's
/// AreaDamage4896BA restores it through Foot4D8F99 before returning, after
/// the firer's Process has already run in that frame.
#[test]
fn retail_blocking_wall_restores_the_move_in_the_detonation_frame() {
    let Some(retail) = crate::rules::retail_ini_fixture::retail_battle_rules_for_map("XMP03T4.MAP")
    else {
        return;
    };
    let rules = &retail.rules;
    let registry = crate::rules::overlay_types::OverlayTypeRegistry::from_ini(
        &retail.processed_rules,
        Some(&retail.fixed_art),
    );
    let mut sim = crate::sim::arena_fixture::rival_arena(rules, 43);
    sim.set_input_delay_ticks(0);
    sim.intern_rule_type_ids(rules);
    sim.resolve_type_handles(rules);
    sim.overlay_grid = Some(crate::sim::overlay_grid::OverlayGrid::new(32, 32));
    let wall = registry.id_for_name("GAWALL").unwrap();
    assert!(!registry.flags(wall).unwrap().crushable);
    // A complete enemy wall line makes shooting the wall the admitted route;
    // there is no cheaper gap for the native A* class-5 cost to choose.
    for y in 0..32 {
        assert!(
            sim.spawn_object_with_overlay_registry(
                "GAWALL", "Russians", 16, y, 0, rules, &registry,
            )
            .is_some()
        );
    }
    let tank = sim
        .spawn_object_with_overlay_registry("MTNK", "Americans", 12, 16, 64, rules, &registry)
        .unwrap();
    let owner = sim.substrate.entities.get(tank).unwrap().owner();
    let goal = NavTargetRef::cell(20, 16);
    sim.queue_command(CommandEnvelope::new(
        owner,
        sim.session.tick() + 1,
        Command::Move {
            entity_id: tank,
            target_rx: 20,
            target_ry: 16,
        },
    ));
    let mut blocker = None;
    for _ in 0..3600 {
        let commands = sim.take_due_commands();
        let grid = sim.path_grid_snapshot().unwrap();
        sim.advance_tick(&commands, Some(rules), Some(&grid), Some(&registry), 33);
        let actor = sim.substrate.entities.get(tank).unwrap();
        if blocker.is_none()
            && actor.mission.current().known() == Some(MissionType::Attack)
            && actor.mission.suspended().known() == Some(MissionType::Move)
            && let Some(TargetKind::Cell(x, y)) = actor.attack_target.target()
        {
            assert_eq!(x, 16);
            assert_eq!(actor.navigation.suspended_nav_com(), Some(goal));
            assert_eq!(
                sim.overlay_grid.as_ref().unwrap().cell(x, y).overlay_id,
                Some(wall)
            );
            blocker = Some((x, y));
        }
        if let Some((x, y)) = blocker
            && sim
                .overlay_grid
                .as_ref()
                .unwrap()
                .cell(x, y)
                .overlay_id
                .is_none()
        {
            assert_eq!(actor.mission.current().known(), Some(MissionType::Move));
            assert_eq!(actor.navigation.nav_com(), Some(goal));
            assert!(
                !actor.navigation.pending_destination_setter(),
                "Foot Restore pays the class setter inside AreaDamage, before the next Process"
            );
            return;
        }
    }
    let actor = sim.substrate.entities.get(tank).unwrap();
    panic!(
        "ordinary Move must breach the real concrete wall: blocker={blocker:?}, position={:?}, mission={:?}, nav={:?}, target={:?}",
        actor.position,
        actor.mission.current(),
        actor.navigation.nav_com(),
        actor.attack_target.target()
    );
}

/// AreaDamage489614 admits the original overlay before ReduceTiberium;
/// 4896B2 still detaches when that non-wall overlay becomes empty.
/// The scene enables ChainReaction through the production overlay reader;
/// stock TIB01 leaves this gate disabled. Units and warhead remain stock.
#[test]
fn ore_removal_runs_the_outer_sweep_but_an_empty_cell_does_not() {
    let Some(retail) = crate::rules::retail_ini_fixture::retail_battle_rules_for_map("XMP03T4.MAP")
    else {
        return;
    };
    let rules = &retail.rules;
    let mut overlay_ini = retail.processed_rules.as_ref().clone();
    overlay_ini.merge(&crate::rules::ini_parser::IniFile::from_str(
        "[TIB01]\nChainReaction=yes\n",
    ));
    let registry = crate::rules::overlay_types::OverlayTypeRegistry::from_ini(
        &overlay_ini,
        Some(&retail.fixed_art),
    );
    let mut sim = crate::sim::arena_fixture::rival_arena(rules, 43);
    sim.intern_rule_type_ids(rules);
    sim.resolve_type_handles(rules);
    sim.overlay_grid = Some(crate::sim::overlay_grid::OverlayGrid::new(32, 32));
    let ore = registry.id_for_name("TIB01").unwrap();
    let flags = registry.flags(ore).unwrap();
    assert!(flags.tiberium && flags.chain_reaction && !flags.wall);
    let warhead = rules.warhead("HE").unwrap();
    assert!(warhead.tiberium);
    let tank = sim
        .spawn_object_with_overlay_registry("MTNK", "Americans", 12, 16, 64, rules, &registry)
        .unwrap();
    let target = Some(TargetKind::Cell(16, 16));
    let effects = crate::sim::world::FrameEffects::default();
    let he = sim.interner.intern("HE");
    sim.overlay_grid
        .as_mut()
        .unwrap()
        .place_overlay(16, 16, ore, 0);
    sim.assign_target_represented(tank, target, rules, Some(&registry), effects)
        .unwrap();
    let removed = crate::sim::combat::world_receiver::collect_area(
        &mut sim,
        rules,
        Some(&registry),
        (16, 16),
        10,
        warhead,
        (crate::sim::combat::RAD_NO_ATTACKER, None, he),
        None,
        0,
        effects,
    );
    assert_eq!(
        sim.overlay_grid.as_ref().unwrap().cell(16, 16).overlay_id,
        None
    );
    assert_eq!(removed.cell_target_detaches.len(), 1);
    assert_eq!(removed.cell_target_detaches[0].listener_id, tank);
    assert_eq!(
        sim.substrate
            .entities
            .get(tank)
            .unwrap()
            .attack_target
            .target(),
        None
    );

    sim.assign_target_represented(tank, target, rules, Some(&registry), effects)
        .unwrap();
    let empty = crate::sim::combat::world_receiver::collect_area(
        &mut sim,
        rules,
        Some(&registry),
        (16, 16),
        10,
        warhead,
        (crate::sim::combat::RAD_NO_ATTACKER, None, he),
        None,
        0,
        effects,
    );
    assert!(empty.cell_target_detaches.is_empty());
    assert_eq!(
        sim.substrate
            .entities
            .get(tank)
            .unwrap()
            .attack_target
            .target(),
        target
    );
}
