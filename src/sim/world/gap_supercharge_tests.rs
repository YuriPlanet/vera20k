//! The Gap Generator super-charge on retail rules through the production
//! frame: Building What_Action's power rung (`0x00447286..0x004472E7`), the
//! self click's Unload (`0x004434FF..0x00443514`), Mission_Unload's toggle
//! (`0x0044E29D..0x0044E36B`), GetPowerDrain's extra drain (`0x0044E89F`)
//! and the operational-off reset (`0x00454B91..0x00454BEB`).
use super::*;
use crate::rules::ruleset::RuleSet;
use crate::sim::command::{Command, CommandEnvelope};
use crate::sim::house_state::HouseState;
use crate::sim::pathfinding::PathGrid;

fn arena(rules: &RuleSet) -> (Simulation, PathGrid, InternedId) {
    let mut sim = Simulation::with_seed(0x44E2BE);
    for (name, human) in [("Russians", false), ("Americans", true)] {
        let id = sim.interner.intern(name);
        sim.houses
            .register(HouseState::new(id, 0, None, human, 0, 10));
    }
    let grid = crate::sim::arena_fixture::flat_arena(&mut sim, rules);
    sim.intern_rule_type_ids(rules);
    sim.resolve_type_handles(rules);
    let owner = sim.interner.get("Americans").unwrap();
    (sim, grid, owner)
}

fn spawn(sim: &mut Simulation, rules: &RuleSet, kind: &str, rx: u16, ry: u16) -> u64 {
    sim.spawn_object_at_height(kind, "Americans", rx, ry, 0, 0, rules)
        .unwrap_or_else(|| panic!("{kind} spawns"))
}

fn run(sim: &mut Simulation, rules: &RuleSet, grid: &PathGrid, commands: &[CommandEnvelope]) {
    sim.advance_tick(commands, Some(rules), Some(grid), None, 33);
}

fn totals(sim: &Simulation, owner: InternedId) -> (i32, i32) {
    let state = &sim.power_states[&owner];
    (state.total_output(), state.total_drain())
}

fn click(sim: &mut Simulation, rules: &RuleSet, grid: &PathGrid, owner: InternedId, gap: u64) {
    let order = CommandEnvelope::new(
        owner,
        sim.session.tick(),
        Command::UnloadPassengers { transport_id: gap },
    );
    let result = sim.advance_tick(&[order], Some(rules), Some(grid), None, 33);
    assert_eq!(
        result.executed_commands, 1,
        "the gap generator's Unload is admitted"
    );
}

/// Runs frames until the super-charge reads `charged`.
fn run_until_charged(
    sim: &mut Simulation,
    rules: &RuleSet,
    grid: &PathGrid,
    gap: u64,
    charged: bool,
) {
    for _ in 0..60 {
        if sim.substrate.entities.get(gap).unwrap().gap_supercharged() == charged {
            return;
        }
        let commands = sim.take_due_commands();
        run(sim, rules, grid, &commands);
    }
    panic!("the super-charge never became {charged}");
}

/// Five Nuclear Reactors (`Power=2000`) leave 9900 over the stock GAGAP's
/// `Power=-100`: above its `ExtraPower=-9000` threshold, so the self action
/// is offered; four (7900) are not enough. The click charges it and the next
/// House update adds 9000 to the drain; a second click discharges it.
#[test]
fn a_retail_gap_generator_supercharges_and_discharges_through_its_unload() {
    let Some(retail) = crate::rules::retail_ini_fixture::retail_battle_rules() else {
        return;
    };
    let rules = &retail.rules;
    let gagap = rules.object("GAGAP").unwrap();
    assert_eq!(
        (
            gagap.extra_power.output(),
            gagap.extra_power.drain(),
            gagap.gap_radius_in_cells,
            gagap.super_gap_radius_in_cells
        ),
        (0, 9000, 10, 10)
    );
    let (mut sim, grid, owner) = arena(rules);
    let gap = spawn(&mut sim, rules, "GAGAP", 16, 26);
    for x in [1, 6, 11, 16] {
        spawn(&mut sim, rules, "NANRCT", x, 2);
    }
    for _ in 0..4 {
        run(&mut sim, rules, &grid, &[]);
    }
    assert_eq!(totals(&sim, owner), (8000, 100));
    assert_eq!(sim.gap_generator_self_action(gap, rules), Some(false));

    spawn(&mut sim, rules, "NANRCT", 21, 2);
    for _ in 0..4 {
        run(&mut sim, rules, &grid, &[]);
    }
    assert_eq!(totals(&sim, owner), (10000, 100));
    assert_eq!(sim.gap_generator_self_action(gap, rules), Some(true));
    assert!(
        sim.substrate
            .entities
            .get(gap)
            .unwrap()
            .gap_generator
            .viewers()[&owner]
            .active(),
        "the operational generator holds its gap"
    );

    click(&mut sim, rules, &grid, owner, gap);
    run_until_charged(&mut sim, rules, &grid, gap, true);
    run(&mut sim, rules, &grid, &[]);
    assert_eq!(totals(&sim, owner), (10000, 9100));
    let deposit = sim
        .substrate
        .entities
        .get(gap)
        .unwrap()
        .gap_generator
        .viewers()[&owner];
    assert!(deposit.active());
    assert_eq!(deposit.radius(), 10, "SuperGapRadiusInCells");
    // Still offered while charged (`0x004472AA`), whatever the surplus.
    assert_eq!(sim.gap_generator_self_action(gap, rules), Some(true));

    click(&mut sim, rules, &grid, owner, gap);
    run_until_charged(&mut sim, rules, &grid, gap, false);
    run(&mut sim, rules, &grid, &[]);
    assert_eq!(totals(&sim, owner), (10000, 100));
    assert!(
        sim.substrate
            .entities
            .get(gap)
            .unwrap()
            .gap_generator
            .viewers()[&owner]
            .active()
    );
}

/// A charged generator over five reactors, settled.
fn charged_arena(rules: &RuleSet) -> (Simulation, PathGrid, InternedId, u64) {
    let (mut sim, grid, owner) = arena(rules);
    let gap = spawn(&mut sim, rules, "GAGAP", 16, 26);
    for x in [1, 6, 11, 16, 21] {
        spawn(&mut sim, rules, "NANRCT", x, 2);
    }
    for _ in 0..4 {
        run(&mut sim, rules, &grid, &[]);
    }
    click(&mut sim, rules, &grid, owner, gap);
    run_until_charged(&mut sim, rules, &grid, gap, true);
    run(&mut sim, rules, &grid, &[]);
    assert_eq!(totals(&sim, owner), (10000, 9100));
    (sim, grid, owner, gap)
}

/// Low power stops the charged generator; its off edge removes the gap and
/// drops the charge (`0x00454B9B..0x00454BCD`). Ten more generators
/// (drain 10100 over 10000) cause the outage; the discharged House (drain
/// 1100) has full power again and the generator resumes its gap.
#[test]
fn low_power_discharges_a_supercharged_gap_generator() {
    let Some(retail) = crate::rules::retail_ini_fixture::retail_battle_rules() else {
        return;
    };
    let rules = &retail.rules;
    let (mut sim, grid, owner, gap) = charged_arena(rules);
    for x in 0..10 {
        spawn(&mut sim, rules, "GAGAP", 2 + 2 * x, 20);
    }
    run_until_charged(&mut sim, rules, &grid, gap, false);
    for _ in 0..4 {
        run(&mut sim, rules, &grid, &[]);
    }
    assert_eq!(totals(&sim, owner), (10000, 1100));
    let deposit = sim
        .substrate
        .entities
        .get(gap)
        .unwrap()
        .gap_generator
        .viewers()[&owner];
    assert!(deposit.active());
    assert_eq!(deposit.radius(), 10, "GapRadiusInCells");
}

/// Turning the generator off (DisableStuff `0x004524E9`) removes its gap
/// before the operational visit, so the off edge's `+0x269` gate skips the
/// discharge: the charge survives the power-off. Offline, GetPowerDrain
/// answers 0 before its `+0x669` addend (`0x0044E88F`). EVENT_POWERON
/// (GoOnline `0x00452260`) brings the gap and the 9100 drain back.
#[test]
fn powering_off_keeps_the_supercharge_without_its_drain() {
    let Some(retail) = crate::rules::retail_ini_fixture::retail_battle_rules() else {
        return;
    };
    let rules = &retail.rules;
    let (mut sim, grid, owner, gap) = charged_arena(rules);
    assert!(sim.go_building_offline(gap, rules, FrameEffects::default()));
    for _ in 0..4 {
        run(&mut sim, rules, &grid, &[]);
    }
    let entity = sim.substrate.entities.get(gap).unwrap();
    assert!(!entity.gap_generator.viewers()[&owner].active());
    assert!(entity.gap_supercharged());
    assert_eq!(totals(&sim, owner), (10000, 0));

    let on = CommandEnvelope::new(
        owner,
        sim.session.tick(),
        Command::SetBuildingOnLine {
            entity_id: gap,
            on: true,
        },
    );
    run(&mut sim, rules, &grid, &[on]);
    for _ in 0..4 {
        run(&mut sim, rules, &grid, &[]);
    }
    let entity = sim.substrate.entities.get(gap).unwrap();
    assert!(entity.building_online());
    assert!(entity.gap_supercharged());
    assert!(entity.gap_generator.viewers()[&owner].active());
    assert_eq!(totals(&sim, owner), (10000, 9100));
}

/// Mission_Unload's uncharged, not-operational arm (`0x0044E30E..0x0044E32C`):
/// `+0x26C` takes `GapRadiusInCells=`, the add deposits nothing, and neither
/// flag nor House+5778 is written. A map gives the Gap Generator a distinct
/// `SuperGapRadiusInCells=15`; with no reactor its House is in low power.
/// Once powered, the same arm caches 15 and charges.
#[test]
fn an_unpowered_gap_generator_unload_keeps_the_normal_radius_and_no_charge() {
    let patch = crate::rules::ini_parser::IniFile::from_str("[GAGAP]\nSuperGapRadiusInCells=15\n");
    let Some(retail) = crate::rules::retail_ini_fixture::retail_battle_rules_with_map_patch(&patch)
    else {
        return;
    };
    let rules = &retail.rules;
    let (mut sim, grid, owner) = arena(rules);
    let gap = spawn(&mut sim, rules, "GAGAP", 16, 26);
    for _ in 0..4 {
        run(&mut sim, rules, &grid, &[]);
    }
    assert_eq!(sim.building_operational_state(gap, rules), Some(false));
    assert!(!sim.power_states[&owner].power_recheck_for_test());

    sim.toggle_gap_supercharge(gap, rules);
    let entity = sim.substrate.entities.get(gap).unwrap();
    let deposit = entity.gap_generator.viewers()[&owner];
    assert_eq!((deposit.active(), deposit.radius()), (false, 10));
    assert!(!entity.gap_supercharged());
    assert!(!sim.power_states[&owner].power_recheck_for_test());

    for x in [1, 6, 11, 16, 21] {
        spawn(&mut sim, rules, "NANRCT", x, 2);
    }
    for _ in 0..4 {
        run(&mut sim, rules, &grid, &[]);
    }
    assert_eq!(sim.building_operational_state(gap, rules), Some(true));
    sim.toggle_gap_supercharge(gap, rules);
    let entity = sim.substrate.entities.get(gap).unwrap();
    let deposit = entity.gap_generator.viewers()[&owner];
    assert_eq!((deposit.active(), deposit.radius()), (true, 15));
    assert!(entity.gap_supercharged());
    assert!(sim.power_states[&owner].power_recheck_for_test());
}

/// The off edge reads the running client's own `+0x269` (`0x00454B9B`); in
/// skirmish that is the current House's copy. A skirmish with the human
/// (current) House and two AIs: the AI owner of a charged generator allies
/// with the human under retail `AllyReveal=yes`. The remove bracket
/// (`0x004F9B9A`, before the ally bit) takes the gap arm on the human's
/// client and the re-add bracket (`0x004F9F4F`) the allied-Building arm, so
/// the human's copy stays 0 while the owner's is 1. The outage then skips
/// the removal and the super-charge reset: the charge and its drain survive.
#[test]
fn an_allied_current_house_without_the_gap_keeps_the_charge_through_the_outage() {
    let Some(retail) = crate::rules::retail_ini_fixture::retail_battle_rules() else {
        return;
    };
    let rules = &retail.rules;
    assert!(rules.general.ally_reveal, "retail AllyReveal=yes");
    let mut sim = Simulation::with_seed(0x454B9B);
    sim.session
        .set_game_mode(crate::sim::scenario_session::ScenarioGameMode::Skirmish);
    for (name, human) in [("Russians", false), ("Americans", true), ("British", false)] {
        let id = sim.interner.intern(name);
        sim.houses
            .register(HouseState::new(id, 0, None, human, 0, 10));
    }
    let owner = sim.interner.get("Russians").unwrap();
    let player = sim.interner.get("Americans").unwrap();
    sim.session.set_current_house_for_test(Some(player));
    let grid = crate::sim::arena_fixture::flat_arena(&mut sim, rules);
    sim.intern_rule_type_ids(rules);
    sim.resolve_type_handles(rules);
    let spawn = |sim: &mut Simulation, kind: &str, rx: u16, ry: u16| {
        sim.spawn_object_at_height(kind, "Russians", rx, ry, 0, 0, rules)
            .unwrap_or_else(|| panic!("{kind} spawns"))
    };
    let gap = spawn(&mut sim, "GAGAP", 16, 26);
    for x in [1, 6, 11, 16, 21] {
        spawn(&mut sim, "NANRCT", x, 2);
    }
    for _ in 0..4 {
        run(&mut sim, rules, &grid, &[]);
    }
    // The charge is a supplied initial AI posture, not a player click from
    // this computer House. Use the existing class receiver to order it.
    assert!(sim.apply_command_with_overlays(
        "Russians",
        &Command::UnloadPassengers { transport_id: gap },
        rules,
        None,
        FrameEffects::default(),
    ));
    run_until_charged(&mut sim, rules, &grid, gap, true);
    run(&mut sim, rules, &grid, &[]);
    assert_eq!(totals(&sim, owner), (10000, 9100));
    let viewers = |sim: &Simulation| {
        sim.substrate
            .entities
            .get(gap)
            .unwrap()
            .gap_generator
            .viewers()
            .clone()
    };
    assert!(viewers(&sim)[&player].active(), "the human's copy starts 1");

    sim.make_ally(owner, player, rules, None, FrameEffects::default(), false);
    let deposits = viewers(&sim);
    assert!(deposits[&owner].active(), "the owner's copy is re-added");
    assert!(!deposits[&player].active(), "the new ally's copy stays 0");

    for x in 0..10 {
        spawn(&mut sim, "GAGAP", 2 + 2 * x, 20);
    }
    for _ in 0..8 {
        run(&mut sim, rules, &grid, &[]);
    }
    let entity = sim.substrate.entities.get(gap).unwrap();
    assert!(
        entity.gap_supercharged(),
        "native skirmish keeps the charge"
    );
    // The kept 9000 drain holds the House in low power.
    assert!(
        !entity.building_last_operational.is_operational(),
        "the outage ran the off edge"
    );
    assert_eq!(totals(&sim, owner), (10000, 10100));
}
