//! Phobos `TeamMember.ConsideredAs` (`Ext/TechnoType/Body.cpp:1160`,
//! `Ext/Team/Hooks.cpp:61-127`): a TechnoType may fill a TaskForce entry for
//! every type it lists, in the team add, liberate and recruit matches.

use super::*;
use crate::rules::ini_parser::IniFile;
use crate::rules::ruleset::RuleSet;
use crate::sim::world::Simulation;

fn rules(considered_as: Option<&str>) -> RuleSet {
    let key = considered_as.map_or_else(String::new, |list| {
        format!("TeamMember.ConsideredAs={list}\n")
    });
    RuleSet::from_ini(&IniFile::from_str(&format!(
        "[VehicleTypes]\n0=MTNK\n1=HTNK\n\
         [MTNK]\nStrength=300\nArmor=heavy\nSpeed=6\n{key}\
         [HTNK]\nStrength=400\nArmor=heavy\nSpeed=6\n"
    )))
    .expect("considered-as rules")
}

fn scene(list: Option<&str>) -> (Simulation, RuleSet, u64, u64) {
    let rules = rules(list);
    let mut sim = crate::sim::arena_fixture::rival_arena(&rules, 0x6E_AD73);
    sim.intern_rule_type_ids(&rules);
    sim.resolve_type_handles(&rules);
    let tank = sim
        .spawn_object("MTNK", "Americans", 10, 10, 0, &rules)
        .expect("MTNK spawns");
    let owner = sim.interner.get("Americans").expect("house");
    let script = sim.interner.intern("SCRIPT");
    let task_force = sim.interner.intern("TASKFORCE");
    let team_type = sim.interner.intern("TEAMTYPE");
    let member_type = TeamMemberTypeIdentity {
        category: ObjectCategory::Vehicle,
        id: sim.interner.intern("HTNK"),
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
            count: 1,
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
        .construct_team(
            team_type,
            owner,
            crate::rules::rule_policy::TeamLimitScope::PerHouse,
            0,
            false,
            None,
        )
        .expect("team");
    (sim, rules, team, tank)
}

#[test]
fn the_key_reads_a_type_name_list_and_an_unset_type_keeps_it_empty() {
    let rules = rules(Some("HTNK, SMCV"));
    assert_eq!(
        rules
            .object("MTNK")
            .unwrap()
            .phobos_team_member_considered_as,
        ["HTNK", " SMCV"],
        "parse_values keeps the token's leading space; the native lookup on an          unresolved token never matches"
    );
    assert!(
        rules
            .object("HTNK")
            .unwrap()
            .phobos_team_member_considered_as
            .is_empty()
    );
}

/// `TeamClass::Can_AddMember`'s slot search (`Ext/Team/Hooks.cpp:61-76`,
/// hook `0x6EA6BE`) accepts the member's considered types; without the key
/// the member finds no slot.
#[test]
fn a_considered_type_fills_the_task_force_slot() {
    let (mut sim, rules, team, tank) = scene(Some("HTNK"));
    assert_eq!(sim.team_can_add(team, tank, false, &rules), Some(0));

    let (mut sim, rules, team, tank) = scene(Some("htnk"));
    assert_eq!(
        sim.team_can_add(team, tank, false, &rules),
        Some(0),
        "native type lookup is case-insensitive"
    );

    let (mut sim, rules, team, tank) = scene(None);
    assert_eq!(sim.team_can_add(team, tank, false, &rules), None);
    let (mut sim, rules, team, tank) = scene(Some("SMCV"));
    assert_eq!(
        sim.team_can_add(team, tank, false, &rules),
        None,
        "an unrelated listed type does not fill the HTNK entry"
    );
}

/// `TeamClass::LiberateMember` (`0x6EA8E7`) frees the slot it found through
/// the member's considered types.
#[test]
fn liberating_a_considered_member_frees_its_slot() {
    let (mut sim, rules, team, tank) = scene(Some("HTNK"));
    sim.team_add_member(
        team,
        tank,
        false,
        &rules,
        crate::sim::world::FrameEffects::default(),
    );
    {
        let state = sim.team_script_vm.team(team).expect("team");
        assert_eq!(state.slot_counts[0], 1, "the add matched the HTNK slot");
    }
    sim.team_remove_member(
        team,
        tank,
        true,
        &rules,
        crate::sim::world::FrameEffects::default(),
    );
    let state = sim.team_script_vm.team(team).expect("team");
    assert_eq!(state.members().collect::<Vec<_>>(), Vec::<u64>::new());
    assert_eq!(
        state.slot_counts[0], 0,
        "the liberate scan found the HTNK slot"
    );
    assert_eq!(sim.team_can_add(team, tank, false, &rules), Some(0));
}

/// `TeamClass::Get_Needed_Types` (`0x6EF4D0`, hook `0x6EF57F`) subtracts a
/// member whose type considers itself as the entry type.
#[test]
fn the_missing_type_tally_uses_the_considered_type() {
    let tally = |considered_as: Option<&str>| {
        let (mut sim, rules, team, tank) = scene(considered_as);
        sim.team_script_vm
            .teams
            .get_mut(&team)
            .expect("team")
            .members
            .push(TeamMember {
                id: tank,
                initiated: false,
            });
        let team_state = sim.team_script_vm.team(team).expect("team").clone();
        sim.team_script_vm.needed_types(
            &team_state,
            |id| {
                sim.substrate
                    .entities
                    .get(id)
                    .map(crate::sim::team_script_vm::member_type_identity)
            },
            |id, entry_type| {
                sim.substrate.entities.get(id).is_some_and(|entity| {
                    crate::sim::team_script_vm::membership::member_fills_entry(
                        crate::sim::team_script_vm::member_type_identity(entity),
                        sim.interner.object_type(entity.type_ref(), &rules),
                        entry_type,
                        &sim.interner,
                    )
                })
            },
        )
    };
    assert!(
        tally(Some("HTNK")).is_empty(),
        "the considered member needs no HTNK"
    );
    let needed = tally(None);
    assert_eq!(needed.len(), 1);
    assert_eq!(needed[0].category, ObjectCategory::Vehicle);
}
