//! Authored production-owner regressions, not native-execution goldens.
//! Adapted from the author's ra2-mono action-56 primary-House/building-selector
//! scenario and charge/pair tests, contributed under VERA's GPLv3.

use serde_json::{Value, json};

use super::{ENEMY, OWNER, Replay, replay};
use crate::rules::ini_parser::IniFile;
use crate::rules::team_ai_ini::{TeamAiDefinitionSource, TeamAiIniRegistry};
use crate::sim::house_state::HouseState;
use crate::sim::superweapon::SuperWeaponInstance;
use crate::sim::superweapon::ai_fire::{AI_FIRE_LOG, AiFireEvent};
use crate::sim::team_script_vm::{
    TeamScriptAction, TeamScriptDefinition, TeamScriptVm, TeamTarget,
};
use crate::sim::world::FrameEffects;
use crate::util::save_codec;

const NEAREST_PLANT: i32 = 2 << 16;

fn scene(argument: i32, plant: bool) -> Replay {
    scene_with_members(
        argument,
        plant,
        json!([{"live": true, "joined": true, "aircraft": false, "rating": 0}]),
    )
}

fn scene_with_members(argument: i32, plant: bool, members: Value) -> Replay {
    let supers: Vec<_> = (0..6)
        .map(|kind| {
            json!({
                "kind": kind, "recharge": 900, "charged": kind == 3,
                "granted": true, "start": 4100, "left": 900, "custom": -1
            })
        })
        .collect();
    let row = json!({
        "frame": 5000, "output": 100, "drain": 50,
        "percent": 0.7_f32.to_bits(), "only_enemy": false,
        "centre": [3200, 3456, 0], "paired_chrono": true,
        "supers": supers,
        "members": members
    });
    let mut scene = replay(&row, plant);
    // Read an actual numbered ScriptType entry, then run it through the VM.
    let ini = IniFile::from_str(&format!(
        "[ScriptTypes]\n0=SCRIPT\n[SCRIPT]\n0=56,{argument}\n1=5,10\n"
    ));
    let parsed = TeamAiIniRegistry::from_sources(&ini, &IniFile::from_str(""), false);
    assert!(parsed.diagnostics.is_empty());
    let script = scene.sim.team_script_vm.teams[&scene.team].script_id;
    scene
        .sim
        .team_script_vm
        .register_script(TeamScriptDefinition {
            id: script,
            source: TeamAiDefinitionSource::Scenario,
            actions: parsed.scripts[0]
                .actions
                .iter()
                .map(|action| TeamScriptAction {
                    action_id: action.action_id,
                    argument: action.argument,
                })
                .collect(),
        });
    scene
        .sim
        .team_script_vm
        .teams
        .get_mut(&scene.team)
        .unwrap()
        .cursor = 0;
    scene
}

fn step(scene: &mut Replay) -> Vec<(String, (u16, u16))> {
    AI_FIRE_LOG.set(Some(Vec::new()));
    scene
        .sim
        .team_step_script(scene.team, &scene.rules, None, FrameEffects::default());
    AI_FIRE_LOG
        .take()
        .unwrap()
        .into_iter()
        .filter_map(|event| match event {
            AiFireEvent::Fire(id, cell) => Some((scene.sim.interner.resolve(id).to_string(), cell)),
            _ => None,
        })
        .collect()
}

fn source(scene: &mut Replay) -> &mut SuperWeaponInstance {
    let owner = scene.sim.interner.get(OWNER).unwrap();
    let id = scene.sim.interner.get("SW3").unwrap();
    scene
        .sim
        .super_weapons
        .get_mut(&owner)
        .unwrap()
        .get_mut(&id)
        .unwrap()
}

fn add_house(scene: &mut Replay, name: &str) -> crate::sim::intern::InternedId {
    let id = scene.sim.interner.intern(name);
    scene
        .sim
        .houses
        .register(HouseState::new(id, 2, None, false, 0, 10));
    id
}

fn plant(scene: &mut Replay, owner: &str, at: (u16, u16)) -> u64 {
    scene
        .sim
        .spawn_object_at_height("PLANT", owner, at.0, at.1, 0, 0, &scene.rules)
        .unwrap()
}

fn set_only_enemy(scene: &mut Replay, only_enemy: bool) {
    let team_type = scene.sim.team_script_vm.teams[&scene.team]
        .team_type_id
        .unwrap();
    scene
        .sim
        .team_script_vm
        .team_type_ini
        .get_mut(&team_type)
        .unwrap()
        .only_target_house_enemy = only_enemy;
}

#[test]
fn action56_runs_from_parsed_script_through_the_real_chrono_pair() {
    let mut scene = scene(NEAREST_PLANT, true);
    assert_eq!(
        step(&mut scene),
        vec![("SW3".into(), (12, 13)), ("SW4".into(), (20, 21))]
    );
    let state = &scene.sim.team_script_vm.teams[&scene.team];
    assert!(state.refusal.is_none());
    assert!(state.advance_pending);
    assert_eq!(state.mission_target, scene.plant.map(TeamTarget::Object));
    assert!(
        !source(&mut scene).is_ready,
        "only the source charge is consumed"
    );
    let owner = scene.sim.interner.get(OWNER).unwrap();
    let warp = scene.sim.interner.get("SW4").unwrap();
    assert!(
        !scene.sim.super_weapons[&owner][&warp].is_ready,
        "an uncharged PostClick still executes"
    );
    assert_eq!(
        scene.sim.super_weapons[&owner][&warp].chrono_cell(),
        (12, 13)
    );
    assert!(
        step(&mut scene).is_empty(),
        "the next step must not fire the pair again"
    );
    assert_eq!(scene.sim.team_script_vm.teams[&scene.team].cursor, 1);
}

#[test]
fn action56_uses_the_leaders_house_for_both_supers_and_primary_enemy() {
    // ra2-mono's AiTeamChronosphereBuildingsAction_UsesOriginalBuildingSelectorAndPrimaryHouseLedger.
    let mut scene = scene(NEAREST_PLANT, true);
    let other = add_house(&mut scene, "Third");
    plant(&mut scene, "Third", (14, 15));
    scene
        .sim
        .team_script_vm
        .teams
        .get_mut(&scene.team)
        .unwrap()
        .owner = other;
    set_only_enemy(&mut scene, true);
    let events = step(&mut scene);
    assert_eq!(events.last(), Some(&("SW4".into(), (20, 21))));
    assert_eq!(
        scene.sim.team_script_vm.teams[&scene.team].mission_target,
        scene.plant.map(TeamTarget::Object)
    );
    assert!(!source(&mut scene).is_ready);
}

#[test]
fn action56_distance_modes_use_the_shared_building_selector() {
    for (mode, expected) in [(2, (14, 15)), (3, (20, 21))] {
        let mut scene = scene(mode << 16, true);
        let close = plant(&mut scene, ENEMY, (14, 15));
        assert_eq!(step(&mut scene).last(), Some(&("SW4".into(), expected)));
        let target = if mode == 2 {
            close
        } else {
            scene.plant.unwrap()
        };
        assert_eq!(
            scene.sim.team_script_vm.teams[&scene.team].mission_target,
            Some(TeamTarget::Object(target))
        );
    }
}

#[test]
fn action56_allied_buildings_are_skipped_unless_the_house_is_passive() {
    for passive in [false, true] {
        let mut scene = scene(NEAREST_PLANT, true);
        let ally = add_house(&mut scene, "Ally");
        let close = plant(&mut scene, "Ally", (14, 15));
        scene
            .sim
            .house_alliances
            .entry(OWNER.to_ascii_uppercase())
            .or_default()
            .insert("ALLY".into());
        scene.sim.houses.get_mut(&ally).unwrap().multiplay_passive = passive;
        step(&mut scene);
        let target = if passive { close } else { scene.plant.unwrap() };
        assert_eq!(
            scene.sim.team_script_vm.teams[&scene.team].mission_target,
            Some(TeamTarget::Object(target))
        );
    }
}

#[test]
fn action56_waits_or_finishes_before_looking_for_a_target() {
    for (charged, output, remaining, finished) in [
        (false, 100, 100, false),
        (false, 100, 800, true),
        (true, 0, 100, false),
        (true, 0, 800, true),
    ] {
        let mut scene = scene(NEAREST_PLANT, false);
        let owner = scene.sim.interner.get(OWNER).unwrap();
        scene.sim.power_states.get_mut(&owner).unwrap().total_output = output;
        let sw = source(&mut scene);
        sw.is_ready = charged;
        sw.charge_start_tick = 5000;
        sw.charge_duration = remaining;
        let rng = scene.sim.scenario_rng.native_state_hex();
        assert!(step(&mut scene).is_empty());
        let state = &scene.sim.team_script_vm.teams[&scene.team];
        assert_eq!(
            state.advance_pending, finished,
            "{charged} {output} {remaining}"
        );
        assert!(state.refusal.is_none());
        assert_eq!(scene.sim.scenario_rng.native_state_hex(), rng);
        assert_eq!(source(&mut scene).is_ready, charged);
    }
}

#[test]
fn action56_missing_building_type_target_or_super_finishes_without_spending() {
    for case in 0..6 {
        let argument = match case {
            1 => NEAREST_PLANT + 1,
            4 => 4 << 16,
            5 => -1,
            _ => NEAREST_PLANT,
        };
        let mut scene = scene(argument, case != 0);
        if matches!(case, 2 | 3) {
            let absent = if case == 2 { 3 } else { 4 };
            scene
                .rules
                .super_weapons
                .retain(|_, sw| sw.kind.native_index() != absent);
        }
        let rng = scene.sim.scenario_rng.native_state_hex();
        assert!(step(&mut scene).is_empty(), "case {case}");
        let state = &scene.sim.team_script_vm.teams[&scene.team];
        assert!(state.advance_pending, "case {case}");
        assert!(state.refusal.is_none());
        assert!(source(&mut scene).is_ready);
        assert_eq!(scene.sim.scenario_rng.native_state_hex(), rng);
    }
}

#[test]
fn action56_empty_team_finishes_without_firing() {
    let mut scene = scene(NEAREST_PLANT, true);
    scene
        .sim
        .team_script_vm
        .teams
        .get_mut(&scene.team)
        .unwrap()
        .members
        .clear();
    assert!(step(&mut scene).is_empty());
    assert!(scene.sim.team_script_vm.teams[&scene.team].advance_pending);
    assert!(source(&mut scene).is_ready);
}

#[test]
fn action56_a_waiting_script_roundtrips_without_extra_action_state() {
    let mut scene = scene(NEAREST_PLANT, true);
    source(&mut scene).is_ready = false;
    source(&mut scene).charge_start_tick = 5000;
    source(&mut scene).charge_duration = 100;
    assert!(step(&mut scene).is_empty());
    assert!(!scene.sim.team_script_vm.teams[&scene.team].advance_pending);
    let bytes = save_codec::serialize(&scene.sim.team_script_vm).unwrap();
    let restored: TeamScriptVm = save_codec::deserialize(&bytes).unwrap();
    assert_eq!(
        restored.team(scene.team),
        scene.sim.team_script_vm.team(scene.team)
    );
    scene.sim.team_script_vm = restored;
    source(&mut scene).is_ready = true;
    assert_eq!(step(&mut scene).len(), 2);
    assert!(scene.sim.team_script_vm.teams[&scene.team].advance_pending);
}

#[test]
fn action56_selects_the_highest_eligible_leader_with_first_tie() {
    for (first_rating, second_rating, joined, aircraft, foreign_member) in [
        (2, 9, true, false, 0),
        (9, 9, true, false, 1),
        (2, 9, false, false, 1),
        (2, 9, false, true, 0),
    ] {
        let mut scene = scene_with_members(
            NEAREST_PLANT,
            true,
            json!([
                {"live": true, "joined": true, "aircraft": false, "rating": first_rating},
                {"live": true, "joined": joined, "aircraft": aircraft, "rating": second_rating}
            ]),
        );
        let foreign = add_house(&mut scene, "Foreign");
        scene
            .sim
            .substrate
            .entities
            .get_mut(scene.members[foreign_member])
            .unwrap()
            .set_owner_for_test(foreign);
        set_only_enemy(&mut scene, true);
        assert_eq!(step(&mut scene).last(), Some(&("SW4".into(), (20, 21))));
        assert!(!source(&mut scene).is_ready);
    }
}

#[test]
fn action56_threat_modes_reuse_the_house_spatial_threat_owner() {
    for (mode, expected) in [(0, (14, 15)), (1, (20, 21))] {
        let mut scene = scene(mode << 16, true);
        plant(&mut scene, ENEMY, (14, 15));
        let owner = scene.sim.interner.get(OWNER).unwrap();
        let house = scene.sim.houses.get_mut(&owner).unwrap();
        house.clear_spatial_threat();
        house.adjust_spatial_threat((20, 21), 400).unwrap();
        assert_eq!(step(&mut scene).last(), Some(&("SW4".into(), expected)));
    }
}

#[test]
fn action56_does_not_require_a_weapon_or_revealed_target() {
    let mut scene = scene_with_members(
        NEAREST_PLANT,
        true,
        json!([{"live": true, "joined": true, "aircraft": false, "rating": 0, "unarmed": true}]),
    );
    scene.sim.fog = crate::sim::vision::FogState::new(32, 32);
    assert_eq!(step(&mut scene).last(), Some(&("SW4".into(), (20, 21))));
    assert_eq!(
        scene.sim.team_script_vm.teams[&scene.team].mission_target,
        scene.plant.map(TeamTarget::Object)
    );
}

#[test]
fn action56_missing_centre_or_primary_enemy_finishes_safely() {
    for missing_centre in [false, true] {
        let mut scene = scene(NEAREST_PLANT, true);
        if missing_centre {
            scene
                .sim
                .team_script_vm
                .teams
                .get_mut(&scene.team)
                .unwrap()
                .zone = None;
        } else {
            let owner = scene.sim.interner.get(OWNER).unwrap();
            scene.sim.houses.get_mut(&owner).unwrap().enemy_house = None;
            set_only_enemy(&mut scene, true);
        }
        assert!(step(&mut scene).is_empty());
        assert!(scene.sim.team_script_vm.teams[&scene.team].advance_pending);
        assert!(source(&mut scene).is_ready);
    }
}
