//! Original Building ChangeOwner's capture-sale tail, 4492D7..449368.
//!
//! The joined native rows include an Engineer's event and arrival. Here their
//! Building transition is compared through `change_owner_with_rules`; the
//! separate retail world tests exercise the Rust Engineer command and Walk.
//! Tail rows start with supplied current ownership and mission state and call
//! only `sell_after_owner_change`. No Selling frame or host refund is advanced.

use super::*;
use crate::rules::ini_parser::IniFile;
use crate::sim::c4_plant::PendingC4Detonation;
use crate::sim::combat::Health;
use crate::sim::house_state::HouseState;
use crate::sim::intern::InternedId;
use crate::sim::mission::MissionDispatchTimer;
use crate::sim::mission::state::MissionTestFixture;
use crate::sim::power_system::PowerState;
use crate::sim::scenario_session::ScenarioGameMode;
use serde_json::{Value, json};
use std::collections::BTreeMap;

const BUILDING: u64 = 1;
const HOUSES: [&str; 3] = ["INITIAL", "PREVIOUS", "NEW"];

fn corpus() -> &'static Value {
    crate::test_fixture::json("tools/spatial_oracle/captured_building_sale.json")
}

fn signed(value: &Value) -> i32 {
    i32::try_from(value.as_i64().expect("signed native input")).unwrap()
}

fn boolean(value: &Value) -> bool {
    value.as_bool().expect("native boolean input")
}

fn name(value: &Value) -> &str {
    value.as_str().expect("native name")
}

fn scene(row: &Value) -> (Simulation, RuleSet, InternedId) {
    let input = &row["input"];
    let before = &row["before"];
    let native_type = &corpus()["native_type"];
    let host = name(&native_type["name"]);
    let initial = name(&before["initial_owner"]);
    assert_eq!(row["constructor"]["result_is_receiver"], true);
    assert_eq!(row["constructor"]["initial_owner"], before["initial_owner"]);
    let mut types = BTreeMap::from([(host.to_owned(), signed(&native_type["cost"]))]);
    for upgrade in input["upgrades"].as_array().unwrap() {
        let kind = name(&upgrade["name"]).to_owned();
        let cost = signed(&upgrade["cost"]);
        if let Some(prior) = types.insert(kind, cost) {
            assert_eq!(prior, cost, "one native type has one retained Cost");
        }
    }
    let mut text = String::from("[General]\nRefundPercent=50%\n[Countries]\n");
    for (index, owner) in HOUSES.iter().enumerate() {
        text.push_str(&format!("{index}=C_{owner}\n"));
    }
    for owner in HOUSES {
        text.push_str(&format!(
            "[C_{owner}]\nMultiplayPassive={}\n",
            if owner == initial && boolean(&input["initial_owner_passive"]) {
                "yes"
            } else {
                "no"
            }
        ));
    }
    text.push_str("[BuildingTypes]\n");
    for (index, kind) in types.keys().enumerate() {
        text.push_str(&format!("{index}={kind}\n"));
    }
    for (kind, cost) in &types {
        text.push_str(&format!(
            "[{kind}]\nStrength={}\nCost={cost}\n",
            signed(&native_type["strength"])
        ));
    }
    let mut rules = RuleSet::from_ini(&IniFile::from_str(&text)).unwrap();
    let object = rules.object(host).unwrap();
    assert_eq!(object.strength, signed(&native_type["strength"]));
    assert_eq!(object.cost, signed(&native_type["cost"]));
    for upgrade in corpus()["upgrade_types"].as_array().unwrap() {
        if let Some(object) = rules.object(name(&upgrade["name"])) {
            assert_eq!(object.cost, signed(&upgrade["cost"]));
            // The retained native types in this corpus carry no SuperWeapon;
            // removing a slot therefore has no separate grant/revoke effect.
            assert_eq!(upgrade["super_weapon"], -1);
            assert!(object.super_weapon.is_none());
            assert_eq!(upgrade["turret"], false);
            assert!(!object.has_turret);
        }
    }
    if boolean(&before["buildup"]) {
        rules.set_buildup_control_for_test(host, [0, 4, 1]);
    }
    let mut sim = Simulation::new();
    sim.intern_rule_type_ids(&rules);
    sim.resolve_type_handles(&rules);
    sim.session
        .set_game_mode(ScenarioGameMode::from_game_mode(signed(
            &input["game_mode"],
        )));
    for (index, owner) in HOUSES.iter().enumerate() {
        let id = sim.interner.intern(owner);
        let country_name = format!("C_{owner}");
        let country = sim.interner.intern(&country_name);
        let mut house = HouseState::new(
            id,
            index as i32,
            Some(country),
            *owner == "NEW" && boolean(&input["new_owner_human"]),
            signed(&before["credits"]),
            10,
        );
        house.set_player_control_for_test(*owner == "NEW" && boolean(&input["new_owner_player"]));
        house.resolve_multiplay_passive(&rules, Some(&country_name));
        sim.houses.register(house);
    }
    crate::sim::production::tests::spawn_structure(
        &mut sim, &rules, BUILDING, initial, host, 10, 10,
    );
    let prior = sim.interner.get(name(&before["owner"])).unwrap();
    // This installs the supplied pre-capture history through the shared
    // ownership writer. It has no rules, so it cannot run the capture-sale
    // tail being tested; the constructor identity remains untouched.
    sim.change_owner(BUILDING, prior);
    let slots = before["upgrade_slots"].as_array().unwrap();
    assert_eq!(slots.len(), 3);
    let slots = std::array::from_fn(|slot| {
        slots[slot].as_str().map(|kind| {
            sim.interner
                .get(kind)
                .expect("declared native upgrade type")
        })
    });
    let now = sim.session.binary_frame();
    let building = sim.substrate.entities.get_mut(BUILDING).unwrap();
    building.health = Health::new(signed(&native_type["strength"]));
    building.mission.apply_test_fixture(MissionTestFixture {
        current: MissionId::from_raw(signed(&before["mission"])),
        suspended: MissionId::NONE,
        queued: MissionId::from_raw(signed(&before["queued"])),
        movement_bypass_latch: 0,
        handler_state: u32::try_from(signed(&before["status"])).unwrap(),
        mission_start_frame: now,
        ai_counter: 0,
        dispatch_timer: MissionDispatchTimer::at_frame(now),
    });
    building.set_building_upgrades_for_test(
        i8::try_from(signed(&before["upgrade_count"])).unwrap(),
        slots,
    );
    if boolean(&before["c4"]) {
        building
            .pending_c4_detonation
            .set_for_test(Some(PendingC4Detonation::new(
                crate::sim::timer::CdTimer::started(now as i32, 100),
                None,
            )));
    }
    let new_owner = sim.interner.get("NEW").unwrap();
    let power = sim.power_states.house_mut(new_owner);
    *power = PowerState::settled_for_test();
    if boolean(&before["power_dirty"]) {
        power.invalidate(false);
    }
    if boolean(&before["radar_dirty"]) {
        power.recheck_radar();
    }
    (sim, rules, new_owner)
}

fn state(sim: &Simulation, rules: &RuleSet, new_owner: InternedId) -> Value {
    let building = sim.entities().get(BUILDING).unwrap();
    let power = &sim.power_states[&new_owner];
    json!({
        "owner": sim.interner.resolve(building.owner()),
        "initial_owner": sim.interner.resolve(building.initial_owner()),
        "mission": building.mission.current().raw(),
        "queued": building.mission.queued().raw(),
        "status": building.mission.handler_state(),
        "buildup": rules.has_buildup(sim.interner.resolve(building.type_ref())),
        "c4": building.pending_c4_detonation.pending().is_some(),
        "upgrade_count": building.building_upgrades().count(),
        "upgrade_slots": building.building_upgrades().types().map(|kind|
            kind.map(|kind| sim.interner.resolve(kind))),
        "credits": sim.houses[&new_owner].economy().credits(),
        "power_dirty": power.power_recheck_for_test(),
        "radar_dirty": power.radar_recheck_for_test(),
    })
}

fn compare(row: &Value, joined: bool) {
    let label = name(&row["input"]["name"]);
    let (mut sim, rules, new_owner) = scene(row);
    assert_eq!(
        state(&sim, &rules, new_owner),
        row["before"],
        "{label}: before"
    );
    let initial = sim.entities().get(BUILDING).unwrap().initial_owner();
    let next_id = sim.next_stable_object_id();
    let native_id = sim.native_identity_cursor();
    let rng = sim.scenario_rng().logical_state();
    if joined {
        sim.change_owner_with_rules(BUILDING, new_owner, &rules, None, FrameEffects::default());
    } else {
        sell_after_owner_change(&mut sim, &rules, BUILDING, None, FrameEffects::default());
    }
    assert_eq!(
        state(&sim, &rules, new_owner),
        row["after"],
        "{label}: after"
    );
    assert_eq!(
        sim.entities().get(BUILDING).unwrap().initial_owner() == initial,
        boolean(&row["initial_owner_unchanged"]),
        "{label}: constructor owner lifetime"
    );
    assert_eq!(
        sim.scenario_rng().logical_state() == rng,
        boolean(&row["rng_unchanged"]),
        "{label}: no temporary upgrade constructor draw"
    );
    let delta = signed(&row["object_id_counter_delta"]);
    assert_eq!(
        i64::try_from(sim.next_stable_object_id()).unwrap() - i64::try_from(next_id).unwrap(),
        i64::from(delta),
        "{label}: stable identity allocation"
    );
    assert_eq!(
        sim.native_identity_cursor()
            .unwrap()
            .wrapping_sub(native_id.unwrap()),
        u32::try_from(delta).unwrap(),
        "{label}: original Abstract identity allocation"
    );
}

#[test]
fn captured_building_sale_matches_original_joined_owner_transitions() {
    let rows = corpus()["joined_cases"].as_array().unwrap();
    assert!(!rows.is_empty());
    for row in rows {
        compare(row, true);
    }
}

#[test]
fn captured_building_sale_matches_original_tail_gates_and_upgrade_refunds() {
    let rows = corpus()["tail_cases"].as_array().unwrap();
    assert!(!rows.is_empty());
    for row in rows {
        compare(row, false);
    }
}
