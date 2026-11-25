//! Does a `Goal::Move` carry a unit across the map?
//!
//! The scout probe in status.md gave units a move and they stayed inside a
//! seventeen-cell strip around the base. Two explanations fit: the order never
//! travels — something downstream of admission drops it — or the policy re-sends
//! it every term and the unit restarts from wherever it has got to. This test is
//! the first half: one unit, one order, no policy. If the unit arrives, the fault
//! is in the policy's re-ordering and not in the order.

use vera20k_core::rules::ruleset::RuleSet;
use vera20k_core::sim::behavior::goal::Goal;
use vera20k_core::sim::behavior::order::Funnel;
use vera20k_core::sim::command::CommandEnvelope;
use vera20k_core::sim::control::observation::Observation;
use vera20k_core::sim::intern::InternedId;
use vera20k_core::sim::world::Simulation;

fn arena() -> Option<(Simulation, RuleSet)> {
    let retail = vera20k_core::rules::retail_ini_fixture::retail_battle_rules()?;
    let rules = retail.rules;
    let mut sim = vera20k_core::sim::arena_fixture::rival_arena(&rules, 0xA17);
    sim.intern_rule_type_ids(&rules);
    sim.resolve_type_handles(&rules);
    Some((sim, rules))
}

#[test]
fn a_single_move_order_carries_a_tank_across_the_arena() {
    let Some((mut sim, rules)) = arena() else {
        return;
    };
    let americans: InternedId = sim.intern("Americans");
    let tank = sim
        .spawn_object("MTNK", "Americans", 6, 6, 0, &rules)
        .expect("a tank");
    let target = (26, 26);

    // One order, through the same funnel a player's click and the policy both
    // use, and then the engine drives it. Nothing re-sends it.
    let funnel = Funnel::new(&sim, &rules, americans);
    let command = funnel
        .prepare(tank, Goal::Move { cell: target })
        .expect("a move is admissible");
    sim.queue_commands(vec![CommandEnvelope::new(
        americans,
        sim.session().tick(),
        command,
    )]);

    let mut closest = 999;
    let mut arrived_at = None;
    for frame in 0..1200 {
        let _ = sim.take_due_commands();
        sim.advance_tick(&[], Some(&rules), None, None, 33);
        let observation = Observation::new(&sim, americans);
        let Some(cell) = observation
            .own_entities()
            .find(|entity| entity.entity_id == tank)
            .map(|entity| entity.cell)
        else {
            break;
        };
        let away = i32::from(cell.0.abs_diff(target.0)).max(i32::from(cell.1.abs_diff(target.1)));
        closest = closest.min(away);
        if away <= 1 {
            arrived_at = Some(frame);
            break;
        }
    }
    assert!(
        arrived_at.is_some(),
        "a single Move must carry the tank to {target:?}; it got within {closest} cells"
    );
}

#[test]
fn a_single_attack_move_order_carries_a_tank_across_the_arena() {
    let Some((mut sim, rules)) = arena() else {
        return;
    };
    let americans: InternedId = sim.intern("Americans");
    let tank = sim
        .spawn_object("MTNK", "Americans", 6, 6, 0, &rules)
        .expect("a tank");
    let target = (26, 26);

    // The same single order as the test above, with the goal the policy uses for
    // a march to a cell.
    //
    // `Assault` on a `Target::Cell` maps to `Goal::AttackMove`, and three
    // measurements say the army does not move under it: five IFVs held
    // `job=Assault target=Cell((112, 52))` with `standing=7575` at `(91-95,
    // 26-32)`, only 67 of 4940 readings ever reached that destination, and
    // `MissionId(1)` appeared on 85 of 6041 readings — the army barely fired.
    //
    // This is the half that tells the two explanations apart: if one order, with
    // nothing re-sending it, does not carry the unit, the fault is in the goal and
    // not in the policy's re-ordering.
    let funnel = Funnel::new(&sim, &rules, americans);
    let command = funnel
        .prepare(tank, Goal::AttackMove { cell: target })
        .expect("an attack-move is admissible");
    sim.queue_commands(vec![CommandEnvelope::new(
        americans,
        sim.session().tick(),
        command,
    )]);

    let mut closest = 999;
    let mut arrived_at = None;
    for frame in 0..1200 {
        let _ = sim.take_due_commands();
        sim.advance_tick(&[], Some(&rules), None, None, 33);
        let observation = Observation::new(&sim, americans);
        let Some(cell) = observation
            .own_entities()
            .find(|entity| entity.entity_id == tank)
            .map(|entity| entity.cell)
        else {
            break;
        };
        let away = i32::from(cell.0.abs_diff(target.0)).max(i32::from(cell.1.abs_diff(target.1)));
        closest = closest.min(away);
        if away <= 1 {
            arrived_at = Some(frame);
            break;
        }
    }
    assert!(
        arrived_at.is_some(),
        "a single AttackMove must carry the tank to {target:?}; it got within {closest} cells"
    );
}

#[test]
fn a_tank_surrounded_by_its_own_base_still_reaches_the_field() {
    let Some((mut sim, rules)) = arena() else {
        return;
    };
    let americans: InternedId = sim.intern("Americans");
    // The yard, a war factory and two pillboxes where the match puts them: the
    // measured unit stood at `(91, 26)` wedged against `GAWEAP` at `(89, 27)`,
    // `GAREFN` at `(90, 32)` and `GAPILL` at `(86, 26)`, held `mission=MissionId(2)`
    // — Move — and did not change cell for 2775 frames.
    for (ty, x, y) in [
        ("GAWEAP", 20u16, 20u16),
        ("GAPILL", 17, 19),
        ("GAPOWR", 14, 24),
    ] {
        let _ = sim.spawn_object(ty, "Americans", x, y, 0, &rules);
    }
    sim.resolve_type_handles(&rules);
    // `(91, 26)` is occupied in this fixture — the spawn fails there, which is the
    // stronger version of the same fact — so the tank stands one cell clear of the
    // factory and has to walk out of a crowded corner.
    let tank = sim
        .spawn_object("MTNK", "Americans", 24, 24, 0, &rules)
        .expect("a tank beside the yard");
    let target = (26, 26);

    let funnel = Funnel::new(&sim, &rules, americans);
    let command = funnel
        .prepare(tank, Goal::AttackMove { cell: target })
        .expect("an attack-move is admissible");
    sim.queue_commands(vec![CommandEnvelope::new(
        americans,
        sim.session().tick(),
        command,
    )]);

    let mut closest = 999;
    let mut arrived_at = None;
    for frame in 0..1200 {
        let _ = sim.take_due_commands();
        sim.advance_tick(&[], Some(&rules), None, None, 33);
        let observation = Observation::new(&sim, americans);
        let Some(cell) = observation
            .own_entities()
            .find(|entity| entity.entity_id == tank)
            .map(|entity| entity.cell)
        else {
            break;
        };
        let away = i32::from(cell.0.abs_diff(target.0)).max(i32::from(cell.1.abs_diff(target.1)));
        closest = closest.min(away);
        if away <= 2 {
            arrived_at = Some(frame);
            break;
        }
    }
    assert!(
        arrived_at.is_some(),
        "a tank in its own yard must still leave it; it got within {closest} cells"
    );
}

#[test]
fn a_move_reissued_every_term_does_not_carry_the_tank() {
    let Some((mut sim, rules)) = arena() else {
        return;
    };
    let americans: InternedId = sim.intern("Americans");
    let tank = sim
        .spawn_object("MTNK", "Americans", 6, 6, 0, &rules)
        .expect("a tank");
    let target = (26, 26);

    // The policy re-decides every `TERM_FRAMES` (15) and, for a job whose mission
    // is not `Attack` or `Move`, it re-issues the order. Measured on Arena.mmx
    // seed 0x5EED, a unit at `(91, 26)` held `mission=MissionId(2)` — Move — for
    // 2775 frames without changing cell. One order walks the length of the arena
    // (the test above); this asks what a re-issued one does.
    let mut closest = 999;
    let mut arrived_at = None;
    for frame in 0..1200 {
        if frame % 15 == 0 {
            let funnel = Funnel::new(&sim, &rules, americans);
            if let Ok(command) = funnel.prepare(tank, Goal::AttackMove { cell: target }) {
                sim.queue_commands(vec![CommandEnvelope::new(
                    americans,
                    sim.session().tick(),
                    command,
                )]);
            }
        }
        let _ = sim.take_due_commands();
        sim.advance_tick(&[], Some(&rules), None, None, 33);
        let observation = Observation::new(&sim, americans);
        let Some(cell) = observation
            .own_entities()
            .find(|entity| entity.entity_id == tank)
            .map(|entity| entity.cell)
        else {
            break;
        };
        let away = i32::from(cell.0.abs_diff(target.0)).max(i32::from(cell.1.abs_diff(target.1)));
        closest = closest.min(away);
        if away <= 1 {
            arrived_at = Some(frame);
            break;
        }
    }
    println!("re-issued every 15 frames: arrived={arrived_at:?} closest={closest}");
}

#[test]
fn a_column_of_tanks_all_leave_the_yard() {
    let Some((mut sim, rules)) = arena() else {
        return;
    };
    let americans: InternedId = sim.intern("Americans");
    let target = (26, 26);

    // The real case: not one tank but a column of them, packed into the yard the
    // way a match packs them. Measured on Arena.mmx seed 0x5EED, five IFVs held a
    // `Cell((112, 52))` order at `(91-95, 26-32)` and none of them moved for
    // thousands of frames, while a single tank given the same order in a clear
    // fixture walks the arena and even re-issued every term still arrives.
    //
    // So the crowding is the candidate: units do not path around each other.
    let mut tanks = Vec::new();
    for (x, y) in [(6u16, 6u16), (7, 6), (8, 6), (6, 7), (7, 7), (8, 7), (9, 6), (9, 7)] {
        if let Some(id) = sim.spawn_object("MTNK", "Americans", x, y, 0, &rules) {
            tanks.push(id);
        }
    }
    assert!(tanks.len() >= 4, "the column spawned: {} tanks", tanks.len());
    sim.resolve_type_handles(&rules);

    for tank in &tanks {
        let funnel = Funnel::new(&sim, &rules, americans);
        if let Ok(command) = funnel.prepare(*tank, Goal::AttackMove { cell: target }) {
            sim.queue_commands(vec![CommandEnvelope::new(
                americans,
                sim.session().tick(),
                command,
            )]);
        }
    }

    let nearest = |sim: &Simulation| -> i32 {
        let observation = Observation::new(sim, americans);
        observation
            .own_entities()
            .filter(|entity| tanks.contains(&entity.entity_id))
            .map(|entity| {
                i32::from(entity.cell.0.abs_diff(target.0))
                    .max(i32::from(entity.cell.1.abs_diff(target.1)))
            })
            .max()
            .unwrap_or(999)
    };

    let mut closest = 999;
    for _ in 0..1200 {
        let _ = sim.take_due_commands();
        sim.advance_tick(&[], Some(&rules), None, None, 33);
        closest = closest.min(nearest(&sim));
    }
    println!("column of {}: furthest tank got within {}", tanks.len(), closest);
}

#[test]
fn a_tank_crosses_a_hundred_cells_of_open_ground() {
    let Some(retail) = vera20k_core::rules::retail_ini_fixture::retail_battle_rules() else {
        return;
    };
    let rules = retail.rules;
    let mut sim = vera20k_core::sim::arena_fixture::rival_arena_with_size(&rules, 0xA18, 160);
    sim.intern_rule_type_ids(&rules);
    sim.resolve_type_handles(&rules);
    let americans: InternedId = sim.intern("Americans");

    // Arena.mmx is 160 cells square and its bases are about a hundred apart. The
    // probes above all run on a 32-cell arena, so this is the same question at the
    // size the match actually uses: can a tank walk a hundred cells of open ground
    // at all, or does the distance itself defeat the order?
    let tank = sim
        .spawn_object("MTNK", "Americans", 30, 30, 0, &rules)
        .expect("a tank");
    let target = (130, 30);

    let funnel = Funnel::new(&sim, &rules, americans);
    let command = funnel
        .prepare(tank, Goal::AttackMove { cell: target })
        .expect("an attack-move is admissible");
    sim.queue_commands(vec![CommandEnvelope::new(
        americans,
        sim.session().tick(),
        command,
    )]);

    let mut closest = 999;
    let mut arrived_at = None;
    for frame in 0..4000 {
        let _ = sim.take_due_commands();
        sim.advance_tick(&[], Some(&rules), None, None, 33);
        let observation = Observation::new(&sim, americans);
        let Some(cell) = observation
            .own_entities()
            .find(|entity| entity.entity_id == tank)
            .map(|entity| entity.cell)
        else {
            break;
        };
        let away = i32::from(cell.0.abs_diff(target.0)).max(i32::from(cell.1.abs_diff(target.1)));
        closest = closest.min(away);
        if away <= 2 {
            arrived_at = Some(frame);
            break;
        }
    }
    println!("100 cells: arrived={arrived_at:?} closest={closest}");
    assert!(
        arrived_at.is_some(),
        "a tank must cross a hundred cells of open ground; it got within {closest}"
    );
}

#[test]
fn open_ground_and_a_walled_corner_differ_for_reachability() {
    let Some(retail) = vera20k_core::rules::retail_ini_fixture::retail_battle_rules() else {
        return;
    };
    let rules = retail.rules;
    let mut sim = vera20k_core::sim::arena_fixture::rival_arena_with_size(&rules, 0xA19, 160);
    sim.intern_rule_type_ids(&rules);
    sim.resolve_type_handles(&rules);
    let americans: InternedId = sim.intern("Americans");

    // The question the probes could not answer: is the ground at the yard reachable
    // from the field at all. `can_reach` is the zone grid's answer, and a unit that
    // sits in a corner the grid calls unreachable would hold a Move order forever
    // without moving — exactly the reading, `mission=MissionId(2)` and the same
    // cell for 2775 frames.
    let obs = Observation::new(&sim, americans);
    println!(
        "open 160x160: (30,30) -> (130,30) reachable={}",
        obs.can_reach(
            vera20k_core::rules::locomotor_type::MovementZone::Normal,
            (30, 30),
            (130, 30)
        )
    );
    println!(
        "open 160x160: (91,26) -> (112,52) reachable={}",
        obs.can_reach(
            vera20k_core::rules::locomotor_type::MovementZone::Normal,
            (91, 26),
            (112, 52)
        )
    );
    println!("has_zone_grid={}", obs.has_zone_grid());
}

#[test]
fn a_long_march_reissued_every_term_still_arrives() {
    let Some(retail) = vera20k_core::rules::retail_ini_fixture::retail_battle_rules() else {
        return;
    };
    let rules = retail.rules;
    let mut sim = vera20k_core::sim::arena_fixture::rival_arena_with_size(&rules, 0xA20, 160);
    sim.intern_rule_type_ids(&rules);
    sim.resolve_type_handles(&rules);
    let americans: InternedId = sim.intern("Americans");
    let tank = sim
        .spawn_object("MTNK", "Americans", 30, 30, 0, &rules)
        .expect("a tank");
    let target = (130, 30);

    // The combination the two tests above do not cover: a hundred cells of ground
    // *and* the policy's fifteen-frame re-decide. A short walk survives a restart
    // every fifteen frames; a hundred-cell walk restarts the whole path, and the
    // measured army neither arrived nor moved at all.
    let mut closest = 999;
    let mut arrived_at = None;
    for frame in 0..4000 {
        if frame % 15 == 0 {
            let funnel = Funnel::new(&sim, &rules, americans);
            if let Ok(command) = funnel.prepare(tank, Goal::AttackMove { cell: target }) {
                sim.queue_commands(vec![CommandEnvelope::new(
                    americans,
                    sim.session().tick(),
                    command,
                )]);
            }
        }
        let _ = sim.take_due_commands();
        sim.advance_tick(&[], Some(&rules), None, None, 33);
        let observation = Observation::new(&sim, americans);
        let Some(cell) = observation
            .own_entities()
            .find(|entity| entity.entity_id == tank)
            .map(|entity| entity.cell)
        else {
            break;
        };
        let away = i32::from(cell.0.abs_diff(target.0)).max(i32::from(cell.1.abs_diff(target.1)));
        closest = closest.min(away);
        if away <= 2 {
            arrived_at = Some(frame);
            break;
        }
    }
    println!("long + re-issued: arrived={arrived_at:?} closest={closest}");
    assert!(
        arrived_at.is_some(),
        "a hundred-cell march re-issued every fifteen frames must still arrive; it got within {closest}"
    );
}

#[test]
fn an_attack_move_overrides_an_area_guard() {
    let Some((mut sim, rules)) = arena() else {
        return;
    };
    let americans: InternedId = sim.intern("Americans");
    let tank = sim
        .spawn_object("MTNK", "Americans", 6, 6, 0, &rules)
        .expect("a tank");
    let post = (6, 6);
    let target = (26, 26);

    // Every probe so far gave an idle unit its first order. The match does not:
    // measured on Arena.mmx seed 0x5EED, the units that would not move held
    // `mission=MissionId(11)` — AreaGuard — for 11880 frames at `(83, 37)`, five
    // cells from their own base, while their job read `Assault target=Cell((112,
    // 52))`. The policy had given the order; the unit was guarding.
    //
    // So the question is whether a move order replaces a guard post at all.
    let funnel = Funnel::new(&sim, &rules, americans);
    let command = funnel
        .prepare(tank, Goal::GuardAt { cell: post })
        .expect("a guard is admissible");
    sim.queue_commands(vec![CommandEnvelope::new(
        americans,
        sim.session().tick(),
        command,
    )]);
    for _ in 0..60 {
        let _ = sim.take_due_commands();
        sim.advance_tick(&[], Some(&rules), None, None, 33);
    }
    let mission = Observation::new(&sim, americans)
        .own_entities()
        .find(|entity| entity.entity_id == tank)
        .map(|entity| entity.mission);
    println!("after guard: mission={mission:?}");

    let funnel = Funnel::new(&sim, &rules, americans);
    let command = funnel
        .prepare(tank, Goal::AttackMove { cell: target })
        .expect("an attack-move is admissible");
    sim.queue_commands(vec![CommandEnvelope::new(
        americans,
        sim.session().tick(),
        command,
    )]);

    let mut closest = 999;
    let mut arrived_at = None;
    for frame in 0..1200 {
        let _ = sim.take_due_commands();
        sim.advance_tick(&[], Some(&rules), None, None, 33);
        let observation = Observation::new(&sim, americans);
        let Some(cell) = observation
            .own_entities()
            .find(|entity| entity.entity_id == tank)
            .map(|entity| entity.cell)
        else {
            break;
        };
        let away = i32::from(cell.0.abs_diff(target.0)).max(i32::from(cell.1.abs_diff(target.1)));
        closest = closest.min(away);
        if away <= 1 {
            arrived_at = Some(frame);
            break;
        }
    }
    assert!(
        arrived_at.is_some(),
        "a move order must replace a guard post; it got within {closest}"
    );
}

#[test]
fn a_rifleman_walks_to_its_destination() {
    let Some((mut sim, rules)) = arena() else {
        return;
    };
    let americans: InternedId = sim.intern("Americans");
    // Every probe in this file has used MTNK, a tank. The units that would not
    // move in the match are E1 and ADOG - infantry - and the distinction was never
    // varied: `stuck at 47400: E1 id=1154 job=Scout target=Cell((149, 149))
    // cell=(88, 34) still=47400`, forty-seven thousand frames in one cell.
    let soldier = sim
        .spawn_object("E1", "Americans", 6, 6, 0, &rules)
        .expect("a rifleman");
    let target = (26, 26);

    let funnel = Funnel::new(&sim, &rules, americans);
    let command = funnel
        .prepare(soldier, Goal::Move { cell: target })
        .expect("a move is admissible");
    sim.queue_commands(vec![CommandEnvelope::new(
        americans,
        sim.session().tick(),
        command,
    )]);

    let mut closest = 999;
    let mut arrived_at = None;
    for frame in 0..1600 {
        let _ = sim.take_due_commands();
        sim.advance_tick(&[], Some(&rules), None, None, 33);
        let observation = Observation::new(&sim, americans);
        let Some(cell) = observation
            .own_entities()
            .find(|entity| entity.entity_id == soldier)
            .map(|entity| entity.cell)
        else {
            break;
        };
        let away = i32::from(cell.0.abs_diff(target.0)).max(i32::from(cell.1.abs_diff(target.1)));
        closest = closest.min(away);
        if away <= 1 {
            arrived_at = Some(frame);
            break;
        }
    }
    assert!(
        arrived_at.is_some(),
        "a rifleman must walk to its destination; it got within {closest}"
    );
}
