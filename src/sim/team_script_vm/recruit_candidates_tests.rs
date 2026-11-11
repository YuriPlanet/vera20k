//! Which of its house's objects `Recruit` weighs in VERA (a cost; the search
//! itself replays the original in `recruit_oracle_tests`).

use super::*;
use crate::rules::ruleset::RuleSet;
use crate::sim::arena_fixture::{clear_ground_ini, rival_arena};
use crate::sim::world::{FrameEffects, Simulation};

/// A forming team of `Russians` whose TaskForce asks for two `E1`, beside
/// thirty `E2` of the house and two `E1`: `(sim, rules, team, the E1s)`.
fn forming_team() -> (Simulation, RuleSet, u64, [u64; 2]) {
    let rules = RuleSet::from_ini(&clear_ground_ini(
        "[InfantryTypes]\n0=E1\n1=E2\n[E1]\nStrength=100\nSpeed=4\n[E2]\nStrength=100\nSpeed=4\n",
    ))
    .unwrap();
    let mut sim = rival_arena(&rules, 0x6E_AA90);
    let mut spawn = |kind: &str, x: u16, y: u16| {
        sim.spawn_object(kind, "Russians", x, y, 0, &rules)
            .unwrap_or_else(|| panic!("{kind} spawns"))
    };
    for index in 0..30 {
        spawn("E2", 4 + index % 10, 4 + index / 10);
    }
    let named = [spawn("E1", 20, 20), spawn("E1", 12, 12)];
    sim.resolve_type_handles(&rules);
    let owner = sim.interner.get("Russians").unwrap();
    let script = sim.interner.intern("SCRIPT");
    let task_force = sim.interner.intern("TASKFORCE");
    let team_type = sim.interner.intern("TEAMTYPE");
    let member_type = TeamMemberTypeIdentity {
        category: ObjectCategory::Infantry,
        id: sim.interner.intern("E1"),
    };
    let vm = &mut sim.team_script_vm;
    vm.register_script(TeamScriptDefinition {
        id: script,
        source: TeamAiDefinitionSource::FixedAimd,
        actions: Vec::new(),
    });
    vm.register_task_force(TeamTaskForceDefinition {
        id: task_force,
        source: TeamAiDefinitionSource::FixedAimd,
        group: -1,
        entries: vec![TeamTaskForceEntry {
            member_type,
            count: 2,
        }],
    });
    vm.register_team_type(TeamTypeDefinition {
        id: team_type,
        script_id: script,
        task_force_id: task_force,
        priority: 0,
        is_base_defense: false,
        suicide: false,
        aggressive: false,
        combined_movement_zone: MovementZone::Normal,
        base_zone_relation_enforced: true,
        transport_crossing_required: false,
    });
    let team = vm
        .construct_team(team_type, owner, true, 0, false, None)
        .unwrap();
    (sim, rules, team, named)
}

/// A short entry weighs only its house's objects of a TaskForce type: each
/// other one is read once and never reaches `Can_Add`, and the nearest object
/// of the type joins as before.
#[test]
fn a_short_entry_weighs_only_objects_of_a_task_force_type() {
    let (mut sim, rules, team, [far, near]) = forming_team();
    let owned = 32;
    let before = sim.substrate.entities.lookups().0;

    assert!(sim.team_recruit(team, 0, &rules, None, FrameEffects::default()));

    let lookups = sim.substrate.entities.lookups().0 - before;
    assert!(
        lookups < 2 * owned,
        "{lookups} lookups for {owned} owned infantry"
    );
    let members: Vec<u64> = sim.team_script_vm.team(team).unwrap().members().collect();
    assert_eq!(
        members,
        [near],
        "the centreless team takes the E1 nearest the origin"
    );
    assert_eq!(sim.team_script_vm.team_for_member(far), None);
}
