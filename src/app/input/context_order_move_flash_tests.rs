//! ClickedAction_Cell's MoveFlash (`0x004D7EB5`) on the click path: the
//! latch `0x00822CF2` admits only the selection's first receiver
//! (`0x004AE8CF`, `0x004AE935`, `0x004AE98A`), and only when its re-asked
//! Cell action is Move.

use super::*;
use crate::rules::overlay_types::OverlayTypeRegistry;
use crate::rules::ruleset::RuleSet;
use crate::sim::world::Simulation;

const WATER: (u16, u16) = (20, 20);

/// Two selected Americans tanks on revealed open ground beside a lake.
fn scene(rules: &RuleSet) -> (Simulation, [u64; 2]) {
    let mut sim = crate::sim::arena_fixture::rival_arena(rules, 0x4D7EB5);
    crate::sim::arena_fixture::flood(
        &mut sim,
        rules,
        32,
        (18..23).flat_map(|y| (18..23).map(move |x| (x, y))),
    );
    sim.intern_rule_type_ids(rules);
    sim.resolve_type_handles(rules);
    let owner = sim.interner().get("Americans").unwrap();
    sim.session_mut_for_test()
        .set_current_house_for_test(Some(owner));
    *sim.fog_mut_for_test() = crate::sim::vision::FogState::new(32, 32);
    sim.fog_mut_for_test().reveal_all_for_owner(owner);
    let tanks = [(10, 10), (11, 10)].map(|(rx, ry)| {
        let id = sim
            .spawn_object("MTNK", "Americans", rx, ry, 0, rules)
            .unwrap();
        sim.entities_mut_for_test()
            .get_mut(id)
            .unwrap()
            .selection
            .set_selected_for_test(true);
        id
    });
    (sim, tanks)
}

fn flashes(
    sim: &Simulation,
    rules: &RuleSet,
    overlays: &OverlayTypeRegistry,
    selection: &[u64],
    receivers: &[u64],
    cell: (u16, u16),
) -> Vec<CommandEnvelope> {
    let owner = sim.session().current_house().unwrap();
    let mut queued = Vec::new();
    for &receiver in receivers {
        push_move_flash(
            &mut queued,
            sim,
            rules,
            Some(overlays),
            owner,
            7,
            selection,
            receiver,
            cell,
            false,
        );
    }
    queued
}

#[test]
fn retail_move_click_flashes_once_for_the_first_receiver() {
    let Some(retail) = crate::rules::retail_ini_fixture::retail_battle_rules() else {
        return;
    };
    let overlays = OverlayTypeRegistry::from_ini(&retail.processed_rules, Some(&retail.fixed_art));
    let (sim, [first, second]) = scene(&retail.rules);
    let queued = flashes(
        &sim,
        &retail.rules,
        &overlays,
        &[first, second],
        &[second, first],
        (15, 12),
    );
    assert_eq!(queued.len(), 1);
    assert_eq!(queued[0].execute_tick, 7);
    assert_eq!(queued[0].payload, Command::MoveFlash { rx: 15, ry: 12 });
    // The second selected object alone never reaches the latch.
    assert!(
        flashes(
            &sim,
            &retail.rules,
            &overlays,
            &[second, first],
            &[first],
            (15, 12),
        )
        .is_empty()
    );
}

#[test]
fn retail_no_move_cell_click_spawns_no_flash() {
    let Some(retail) = crate::rules::retail_ini_fixture::retail_battle_rules() else {
        return;
    };
    let overlays = OverlayTypeRegistry::from_ini(&retail.processed_rules, Some(&retail.fixed_art));
    let (sim, [first, second]) = scene(&retail.rules);
    let owner = sim.session().current_house().unwrap();
    assert_eq!(
        sim.cell_move_action(
            owner,
            first,
            WATER,
            crate::sim::world::CellActionQuery {
                mouse_over: false,
                shift: false,
            },
            &retail.rules,
            Some(&overlays),
        ),
        Ok(crate::sim::world::MoveAction::NoMove)
    );
    assert!(
        flashes(
            &sim,
            &retail.rules,
            &overlays,
            &[first, second],
            &[first],
            WATER,
        )
        .is_empty()
    );
}

#[test]
fn the_flash_stays_ahead_of_the_first_receivers_order() {
    let owner = InternedId::default();
    let wrap = |payload| CommandEnvelope::new(owner, 42, payload);
    let flash = Command::MoveFlash { rx: 3, ry: 4 };
    let mut queued = vec![
        wrap(Command::Stop { entity_id: 20 }),
        wrap(Command::Stop { entity_id: 10 }),
        wrap(flash.clone()),
    ];
    restore_selection_dispatch_order(&mut queued, &[10, 20]);
    assert_eq!(
        queued.iter().map(|e| &e.payload).collect::<Vec<_>>(),
        vec![
            &flash,
            &Command::Stop { entity_id: 10 },
            &Command::Stop { entity_id: 20 },
        ]
    );
}
