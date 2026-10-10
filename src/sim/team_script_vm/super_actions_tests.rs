//! `tools/superweapon_oracle.py`'s `team_super_actions` rows, script actions
//! 55 and 57 run natively, replayed through the production actions: the
//! row's Supers, power and charge times on the computer house, a team whose
//! centre stands at the row's coordinates, and Fire_SW's calls, the mission
//! target and the step in the oracle's terms.

use serde_json::{Value, json};

use super::super::{
    TeamMember, TeamScriptDefinition, TeamTarget, TeamTaskForceDefinition, TeamTypeDefinition,
};
use crate::rules::ini_parser::IniFile;
use crate::rules::locomotor_type::MovementZone;
use crate::rules::ruleset::RuleSet;
use crate::rules::superweapon_type::SuperWeaponKind;
use crate::rules::team_ai_ini::TeamAiDefinitionSource;
use crate::sim::combat::ScanMission;
use crate::sim::house_state::HouseState;
use crate::sim::power_system::PowerState;
use crate::sim::superweapon::SuperWeaponInstance;
use crate::sim::superweapon::ai_fire::{AI_FIRE_LOG, AiFireEvent};
use crate::sim::world::Simulation;
use crate::util::fixed_math::SimFixed;
use crate::util::native_x87::NativeF32Bits;

const OWNER: &str = "Computer";
const ENEMY: &str = "Human";

/// The rows' `Type=` values (`SuperWeaponTypeClass+0xB4`), in native order.
const KINDS: [SuperWeaponKind; 6] = [
    SuperWeaponKind::MultiMissile,
    SuperWeaponKind::IronCurtain,
    SuperWeaponKind::LightningStorm,
    SuperWeaponKind::ChronoSphere,
    SuperWeaponKind::ChronoWarp,
    SuperWeaponKind::ParaDrop,
];

/// The rows' default target cell (`TEAM_SW_TARGET_COORDS`), where the
/// replay's enemy power plant stands.
const TARGET_CELL: [i64; 2] = [20, 21];

fn rows() -> Vec<Value> {
    let oracle: Value =
        serde_json::from_str(crate::test_fixture::text("tools/superweapon_oracle.json")).unwrap();
    oracle["team_super_actions"].as_array().unwrap().clone()
}

fn int(value: &Value) -> i32 {
    i32::try_from(value.as_i64().unwrap()).unwrap()
}

fn flag(value: &Value) -> bool {
    value.as_bool().unwrap()
}

fn events<'a>(row: &'a Value, kind: &str) -> Vec<&'a Value> {
    row["events"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|event| event[0] == kind)
        .collect()
}

/// The row's `[SuperWeaponTypes]` as `SW<index>` with its `Type=` and
/// recharge frames; member `index` of type `M<index>` with its
/// `LeadershipRating=`, armed to scan; the centre object's `PROBE`; and the
/// enemy's power plant `PLANT`.
fn rules_for(row: &Value) -> RuleSet {
    let supers = row["supers"].as_array().unwrap();
    let armed = "Strength=100\nPrimary=M60\nSpeed=4\n\
                 Locomotor={4A582744-9839-11D1-B709-00A024DDAFD1}\n";
    let mut ini = format!(
        "[BuildingTypes]\n0=PLANT\n[PLANT]\nStrength=750\nPower=100\n\
         [M60]\nDamage=15\nROF=20\nRange=4\nWarhead=SA\n[SA]\nVerses=100%\n\
         [PROBE]\n{armed}"
    );
    ini += "[InfantryTypes]\n0=PROBE\n";
    let members = row["members"].as_array().unwrap();
    for index in 0..members.len() {
        ini += &format!("{}=M{index}\n", index + 1);
    }
    for (index, member) in members.iter().enumerate() {
        let rating = int(&member["rating"]);
        ini += &format!("[M{index}]\n{armed}LeadershipRating={rating}\n");
    }
    ini += "[SuperWeaponTypes]\n";
    for index in 0..supers.len() {
        ini += &format!("{index}=SW{index}\n");
    }
    for (index, sw) in supers.iter().enumerate() {
        ini += &format!("[SW{index}]\nType={:?}\n", KINDS[int(&sw["kind"]) as usize]);
    }
    let mut rules = RuleSet::from_ini(&IniFile::from_str(&ini)).expect("fixture rules");
    for (index, sw) in supers.iter().enumerate() {
        let name = format!("SW{index}");
        rules
            .super_weapons
            .values_mut()
            .find(|ty| ty.id.eq_ignore_ascii_case(&name))
            .expect("fixture super weapon")
            .recharge_time_frames = int(&sw["recharge"]);
    }
    rules.general.ai_minor_super_ready_percent =
        NativeF32Bits::from_bits(u32::try_from(row["percent"].as_u64().unwrap()).unwrap());
    rules
}

struct Replay {
    sim: Simulation,
    rules: RuleSet,
    team: u64,
    members: Vec<u64>,
    plant: Option<u64>,
}

/// The computer house (its enemy `Human`) holding the row's Supers and
/// power, a team of the row's members from (10, 11) east whose centre is an
/// object
/// at the row's centre coordinates, and the enemy's one-cell power plant at
/// [`TARGET_CELL`] when `plant`.
fn replay(row: &Value, plant: bool) -> Replay {
    let rules = rules_for(row);
    let mut sim = Simulation::with_seed(0x5C57);
    crate::sim::arena_fixture::flat_ground(&mut sim, &rules);
    sim.intern_rule_type_ids(&rules);
    sim.resolve_type_handles(&rules);
    let owner = sim.interner.intern(OWNER);
    let enemy = sim.interner.intern(ENEMY);
    let mut computer = HouseState::new(owner, 0, None, false, 0, 10);
    computer.enemy_house = Some(enemy);
    sim.houses.insert(owner, computer);
    sim.houses
        .insert(enemy, HouseState::new(enemy, 1, None, true, 0, 10));
    sim.session.house_order = vec![owner, enemy];
    sim.session.binary_frame = u32::try_from(int(&row["frame"])).unwrap();
    let mut power = PowerState::default();
    power.total_output = int(&row["output"]);
    power.total_drain = int(&row["drain"]);
    sim.power_states.insert(owner, power);
    for (index, sw) in row["supers"].as_array().unwrap().iter().enumerate() {
        let id = sim.interner.intern(&format!("SW{index}"));
        let mut instance = SuperWeaponInstance::new(id, owner, 0);
        instance.is_ready = flag(&sw["charged"]);
        instance.is_active = flag(&sw["granted"]);
        instance.charge_start_tick = int(&sw["start"]);
        instance.charge_duration = int(&sw["left"]);
        sim.super_weapons
            .entry(owner)
            .or_default()
            .insert(id, instance);
    }

    let mut members = Vec::new();
    for (index, member) in row["members"].as_array().unwrap().iter().enumerate() {
        let rx = 10 + u16::try_from(index).unwrap();
        let id = sim
            .spawn_object_at_height(&format!("M{index}"), OWNER, rx, 11, 0, 0, &rules)
            .expect("member spawns");
        let entity = sim.substrate.entities.get_mut(id).unwrap();
        entity.lifecycle.object_alive = flag(&member["live"]);
        // What_Am_I (vt+0x2C) is all the leader test asks of an aircraft.
        if flag(&member["aircraft"]) {
            entity.category = crate::map::entities::EntityCategory::Aircraft;
        }
        members.push(id);
    }
    let centre = row["centre"].as_array().unwrap();
    let probe = sim
        .spawn_object_at_height("PROBE", OWNER, 5, 5, 0, 0, &rules)
        .expect("centre object spawns");
    // No object stands at negative coordinates; those rows stay out of the
    // full replay.
    let (x, y) = (int(&centre[0]), int(&centre[1]));
    if x >= 0 && y >= 0 {
        let position = &mut sim.substrate.entities.get_mut(probe).unwrap().position;
        position.rx = u16::try_from(x / 256).unwrap();
        position.ry = u16::try_from(y / 256).unwrap();
        position.sub_x = SimFixed::from_num(x % 256);
        position.sub_y = SimFixed::from_num(y % 256);
    }
    let plant = plant.then(|| {
        sim.spawn_object_at_height("PLANT", ENEMY, 20, 21, 0, 0, &rules)
            .expect("plant spawns")
    });

    let script = sim.interner.intern("SCRIPT");
    let task_force = sim.interner.intern("TASKFORCE");
    let team_type = sim.interner.intern("TEAMTYPE");
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
        entries: Vec::new(),
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
    vm.team_type_ini
        .get_mut(&team_type)
        .unwrap()
        .only_target_house_enemy = flag(&row["only_enemy"]);
    let team = vm.construct_team(team_type, owner, true, 0).unwrap();
    let state = vm.teams.get_mut(&team).unwrap();
    state.members = members
        .iter()
        .zip(row["members"].as_array().unwrap())
        .map(|(&id, member)| TeamMember {
            id,
            initiated: flag(&member["joined"]),
        })
        .collect();
    state.zone = Some(TeamTarget::Object(probe));
    state.advance_pending = false;
    Replay {
        sim,
        rules,
        team,
        members,
        plant,
    }
}

/// The rows the full replay runs: one live joined member (the leader rows
/// replay below); the type's `RechargeTime=` (a per-Super override is the
/// module residual); a centre an object can stand at; and a target only
/// where the replay's power plant answers the scan as the stub did (quarry
/// 9, at the row's default cell). The skipped dimensions have their own
/// checks: the leader and the threat call's arguments below, the cell
/// rounding through the centre rows.
fn replayable(row: &Value) -> bool {
    let members = row["members"].as_array().unwrap();
    let default_member = members.len() <= 1
        && members.iter().all(|member| {
            flag(&member["live"]) && flag(&member["joined"]) && !flag(&member["aircraft"])
        })
        && members.iter().all(|member| int(&member["rating"]) == 0);
    let type_recharge = row["supers"]
        .as_array()
        .unwrap()
        .iter()
        .all(|sw| int(&sw["custom"]) == -1);
    let centre = row["centre"].as_array().unwrap();
    let standing = int(&centre[0]) >= 0 && int(&centre[1]) >= 0;
    let target = &row["target"];
    let plant_answers = target.is_null()
        || (int(&row["argument"]) == 9
            && target[0].as_i64().unwrap() / 256 == TARGET_CELL[0]
            && target[1].as_i64().unwrap() / 256 == TARGET_CELL[1]);
    default_member && type_recharge && standing && plant_answers
}

#[test]
fn the_superweapon_script_actions_match_native() {
    let rows = rows();
    assert_eq!(rows.len(), 195);
    let mut replayed = 0;
    for (number, row) in rows.iter().enumerate() {
        if !replayable(row) {
            continue;
        }
        replayed += 1;
        let Replay {
            mut sim,
            rules,
            team,
            plant,
            ..
        } = replay(row, !row["target"].is_null());
        if let Some(plant) = plant {
            assert_eq!(
                sim.team_target_cell_of_coord(TeamTarget::Object(plant)),
                Some(TeamTarget::Cell { x: 20, y: 21 }),
                "the plant's GetCoords cell"
            );
        }

        AI_FIRE_LOG.set(Some(Vec::new()));
        match int(&row["action"]) {
            55 => sim.team_action_iron_curtain(
                team,
                &rules,
                None,
                crate::sim::world::FrameEffects::default(),
            ),
            57 => sim.team_action_chronoshift(
                team,
                int(&row["argument"]),
                &rules,
                None,
                crate::sim::world::FrameEffects::default(),
            ),
            other => panic!("action {other}"),
        }
        let log = AI_FIRE_LOG.take().unwrap();

        let mut seen = Vec::new();
        for event in log {
            if let AiFireEvent::Fire(id, (x, y)) = event {
                let index = rules
                    .super_weapon_order
                    .iter()
                    .position(|name| sim.interner.get(name) == Some(id))
                    .unwrap();
                seen.push(json!(["fire", index, [x as i16, y as i16]]));
            }
        }
        let state = sim.team_script_vm.teams.get(&team).unwrap();
        if plant.is_some() && state.mission_target == plant.map(TeamTarget::Object) {
            seen.push(json!(["assign"]));
        }
        let expected: Vec<Value> = row["events"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|event| event[0] == "fire" || event[0] == "assign")
            .cloned()
            .collect();
        assert_eq!(seen, expected, "row {number}: {row}");
        assert_eq!(
            state.advance_pending,
            int(&row["complete"]) == 1,
            "row {number}: the step"
        );
    }
    assert_eq!(replayed, 163);
}

/// What each threat call asks natively, `Quarry_To_Threat`'s mask of the
/// script argument and the TeamType's `OnlyTargetHouseEnemy=`, is what
/// [`Simulation::team_quarry_mission`] asks for the replayed team. Native
/// scans around the leader's own Location; the leader rows below check the
/// leader.
#[test]
fn the_chronoshift_scan_arguments_match_native() {
    let mut calls = 0;
    for row in rows() {
        let events = events(&row, "threat");
        if events.is_empty() {
            continue;
        }
        let Replay { sim, team, .. } = replay(&row, false);
        let mission = sim.team_quarry_mission(team, int(&row["argument"]));
        for event in events {
            calls += 1;
            let native = ScanMission::Quarry {
                mask: u32::try_from(event[2].as_i64().unwrap()).unwrap(),
                only_target_house_enemy: event[4].as_u64().unwrap() == 1,
            };
            assert_eq!(mission, native, "{row}");
            assert_eq!(event[3], row["location"], "the leader's Location");
        }
    }
    assert_eq!(calls, 41);
}

/// The leader each threat call was made by: the highest `LeadershipRating=`
/// among live members that joined or are aircraft, the first on a tie, else
/// the head ([`Simulation::team_leader`]).
#[test]
fn the_team_leader_matches_native() {
    let mut checked = 0;
    for row in rows() {
        let Some(event) = events(&row, "threat").first().copied() else {
            continue;
        };
        checked += 1;
        let Replay {
            sim,
            rules,
            team,
            members,
            ..
        } = replay(&row, false);
        let leader = sim.team_leader(team, &rules).expect("a leader");
        let index = members.iter().position(|&id| id == leader).unwrap();
        assert_eq!(index as u64, event[1].as_u64().unwrap(), "{row}");
    }
    assert_eq!(checked, 41);
}

/// Retail data the actions rely on, through the production readers: each
/// `[SuperWeaponTypes]` entry sits at the index of its `Type=`, so the index
/// Fire_SW takes names the Super the action checked; and the four retail
/// scripts naming action 55 or 57 run only ported actions, so the nine
/// TeamTypes running them no longer idle on an unported action.
#[test]
fn retail_superweapon_scripts_run_ported_actions() {
    use crate::rules::retail_ini_fixture::{retail_ini, retail_rules_and_art};
    use crate::rules::team_ai_ini::TeamAiIniRegistry;
    let Some((rules_ini, art_ini)) = retail_rules_and_art() else {
        return;
    };
    let rules = RuleSet::from_ini_with_fixed_art_for_test(&rules_ini, &art_ini).unwrap();
    assert_eq!(rules.super_weapon_order.len(), 12);
    for (index, name) in rules.super_weapon_order.iter().enumerate() {
        let kind = rules.super_weapon(name).unwrap().kind;
        assert_eq!(usize::try_from(kind.native_index()), Ok(index), "{name}");
    }
    let aimd = retail_ini("aimd.ini").expect("retail AIMD");
    let registry = TeamAiIniRegistry::from_sources(&aimd, &IniFile::from_str(""), true);
    let mut sim = Simulation::new();
    sim.intern_rule_type_ids(&rules);
    sim.resolve_type_handles(&rules);
    sim.install_team_ai_registry(&registry, &rules)
        .expect("retail AIMD installs");
    let vm = &sim.team_script_vm;
    let scripts: Vec<_> = vm
        .scripts
        .values()
        .filter(|script| {
            script
                .actions
                .iter()
                .any(|action| matches!(action.action_id, 55 | 57))
        })
        .collect();
    assert_eq!(scripts.len(), 4);
    for script in &scripts {
        for action in &script.actions {
            assert!(
                super::super::actions::action_is_ported(action.action_id),
                "{} runs action {}",
                sim.interner.resolve(script.id),
                action.action_id
            );
        }
    }
    let running = vm
        .team_types
        .values()
        .filter(|team_type| {
            scripts
                .iter()
                .any(|script| script.id == team_type.script_id)
        })
        .count();
    assert_eq!(running, 9);
}
