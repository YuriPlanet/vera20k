//! Script37's original member walk and production lifecycle. The native
//! corpus excludes virtual lifecycle bodies; these regressions exercise their
//! existing world owners through the actual script consumer and late drain.

use super::*;
use crate::rules::ini_parser::IniFile;
use crate::rules::team_ai_ini::TeamAiDefinitionSource;
use crate::sim::house_state::HouseState;
use crate::sim::team_script_vm::TeamScriptDefinition;
use crate::sim::world::{LifecycleTestEvent, SimSoundEvent, UninitContext};

fn removal_rules() -> RuleSet {
    RuleSet::from_ini(&IniFile::from_str(&format!(
        "[VehicleTypes]\n0=VEHICLE\n[VEHICLE]\nStrength=200\nSpeed=6\nLocomotor={}\n",
        crate::rules::locomotor_type::LocomotorKind::Drive.clsid()
    )))
    .unwrap()
}

#[test]
fn script37_caches_native_next_and_uses_ordered_limbo_uninit_before_drain() {
    let rows: serde_json::Value =
        serde_json::from_str(crate::test_fixture::text("tools/team_delete_members.json")).unwrap();
    let rules = removal_rules();
    for row in rows.as_array().unwrap() {
        let mut sim = Simulation::with_seed(37);
        let owner = sim.interner.intern("Owner");
        sim.houses
            .register(HouseState::new(owner, 0, None, true, 0, 10));
        crate::sim::arena_fixture::flat_arena(&mut sim, &rules);
        let order = row["order"].as_array().unwrap();
        let members: Vec<_> = order
            .iter()
            .map(|index| {
                sim.spawn_object_at_height(
                    "VEHICLE",
                    "Owner",
                    10 + index.as_u64().unwrap() as u16,
                    10,
                    0,
                    0,
                    &rules,
                )
                .unwrap()
            })
            .collect();
        let script = sim.interner.intern("DELETE MEMBERS");
        sim.team_script_vm.register_script(TeamScriptDefinition {
            id: script,
            source: TeamAiDefinitionSource::Scenario,
            actions: vec![TeamScriptAction {
                action_id: 37,
                argument: 0,
            }],
        });
        let team = sim
            .team_script_vm
            .create_team(owner, script, members.clone(), 0);
        sim.clear_lifecycle_test_events_for_test();
        sim.sound_events.clear_for_test();
        sim.team_step_script(team, &rules, None, Default::default());
        let state = sim.team_script_vm.team(team).unwrap();
        assert_eq!(state.advance_pending(), row["advance"] == 1);
        assert_eq!(
            state.member_count(),
            0,
            "Limbo's existing detach removes each live member"
        );
        assert!(state.refusal().is_none());
        let native_uninit: Vec<_> = row["events"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|event| event[0] == "uninit")
            .map(|event| order.iter().position(|input| input == &event[1]).unwrap())
            .map(|ordinal| members[ordinal])
            .collect();
        let uninit: Vec<_> = sim
            .lifecycle_test_events_for_test()
            .iter()
            .filter_map(|event| match event {
                LifecycleTestEvent::UninitClassPre { stable_id } => Some(*stable_id),
                _ => None,
            })
            .collect();
        assert_eq!(
            uninit, native_uninit,
            "preserve original head order through member unlink callbacks"
        );
        let mut limbos = 0;
        let mut uninits = 0;
        for event in sim.lifecycle_test_events_for_test() {
            match event {
                LifecycleTestEvent::ConcealLimboSet => limbos += 1,
                LifecycleTestEvent::UninitClassPre { .. } => {
                    assert_eq!(
                        limbos,
                        uninits + 1,
                        "Script37's explicit Limbo must finish before this member's UnInit prelude"
                    );
                    uninits += 1;
                }
                _ => {}
            }
        }
        assert_eq!(limbos, members.len());
        for &member in &members {
            let entity = sim
                .substrate
                .entities
                .get(member)
                .expect("physical removal is deferred");
            assert!(entity.lifecycle.in_limbo() && !entity.lifecycle.object_alive());
            assert_eq!(
                entity.health.current(),
                200,
                "Script37 is not a damage/RecordKill caller"
            );
            assert!(!entity.lifecycle.cell_marked());
            assert!(!sim.team_script_vm.member_team.contains_key(&member));
            assert_eq!(sim.sound_events.iter().filter(|event|
                matches!(event, SimSoundEvent::ObjectSoundDetached { owner } if *owner == member)
            ).count(), 1, "FootLimbo detaches its sound once; UnInit's repeated Limbo does not");
        }
        assert_eq!(sim.houses[&owner].stats().units_lost(), 0);
        sim.process_pending_delete_with(&rules, None, Default::default(), 1, true);
        assert!(
            members
                .iter()
                .all(|member| !sim.substrate.entities.contains(*member))
        );
        assert_eq!(
            sim.houses[&owner].stats().units_lost(),
            0,
            "healthy scripted disposal creates no loss"
        );
    }
}

#[test]
fn first_unit_limbo_uses_shared_stop_driver_before_hiding_the_actor() {
    use crate::sim::components::DriveCoord;
    use crate::sim::movement::track_process::TrackFamily;
    let rules = removal_rules();
    let mut sim = Simulation::new();
    let owner = sim.interner.intern("Owner");
    sim.houses
        .register(HouseState::new(owner, 0, None, true, 0, 10));
    crate::sim::arena_fixture::flat_arena(&mut sim, &rules);
    let member = sim
        .spawn_object_at_height("VEHICLE", "Owner", 10, 10, 0, 0, &rules)
        .unwrap();
    let target = DriveCoord::cell(15, 10, 0);
    // This fixture supplies a moving Drive prior. A fresh constructed Drive
    // retains no track payload until its normal movement owner installs one.
    assert!(
        sim.substrate
            .entities
            .get_mut(member)
            .unwrap()
            .locomotor
            .as_mut()
            .unwrap()
            .ensure_installed_track_state()
    );
    assert!(
        sim.substrate
            .entities
            .get_mut(member)
            .unwrap()
            .locomotor
            .as_mut()
            .unwrap()
            .store_track_destination(TrackFamily::Drive, Some(target))
    );
    assert_eq!(
        sim.substrate
            .entities
            .get(member)
            .unwrap()
            .locomotor
            .as_ref()
            .unwrap()
            .track_destination(TrackFamily::Drive),
        Some(target)
    );
    sim.techno_limbo_with_context(member, UninitContext::with_rules(&rules));
    let entity = sim.substrate.entities.get(member).unwrap();
    assert!(entity.lifecycle.in_limbo());
    assert_eq!(
        entity
            .locomotor
            .as_ref()
            .unwrap()
            .track_destination(TrackFamily::Drive),
        None,
        "Foot4DB2FB must dispatch Unit4D55C0 before occupation release/Lock"
    );
}

#[test]
fn retail_all01_return_programs_bind_script37_and_their_locomotor_types() {
    use crate::rules::locomotor_type::LocomotorKind;
    use crate::rules::process_owner::NativeScenarioRulesPrefix;
    let Some((_, assets)) = crate::rules::retail_ini_fixture::retail_assets() else {
        return;
    };
    let map =
        crate::map::map_file::MapFile::from_bytes(assets.get_ref("ALL01UMD.MAP").unwrap()).unwrap();
    let named = assets.resolve_ref("ALL01UMD.INI").map(|_| {
        crate::rules::retail_sources::select_ini(&assets, "ALL01UMD.INI")
            .unwrap()
            .ini
    });
    let mut owner = crate::rules::retail_ini_fixture::retail_rules_owner(&assets);
    let (rules, _, _, _) = owner
        .load_scenario(
            NativeScenarioRulesPrefix::Campaign(named.as_ref()),
            &map.ini,
        )
        .unwrap()
        .into_parts();
    let registry =
        crate::rules::team_ai_ini::TeamAiIniRegistry::load_retail(&assets, &map.ini, false)
            .unwrap();
    for id in ["0B7D63DC", "0AB4A90C", "0B1646CC", "0962056C"] {
        let script = registry
            .scripts
            .iter()
            .find(|script| script.id == id)
            .unwrap();
        assert_eq!(
            script.actions.last().unwrap().action_id,
            37,
            "{id}: selected return tail"
        );
    }
    assert_eq!(
        rules.object("YHVR").unwrap().locomotor,
        LocomotorKind::Hover
    );
    assert_eq!(
        rules.object("ZEP").unwrap().locomotor,
        LocomotorKind::Jumpjet
    );
    assert!(action_is_ported(37));
}
