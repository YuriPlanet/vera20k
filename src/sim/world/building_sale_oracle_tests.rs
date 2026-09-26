//! Replay of `tools/spatial_oracle/building_sale.json`'s `crew` and `refund`
//! rows against the sale's Rust owners: Sell's stage 1
//! (`production::sell_stage_one`: the survivor count, the absorbed
//! passengers, the garrison and the crew in `sim::crew_survival`, then the
//! sounds) and the sale's credit (`production::building_type_refund`, `full`
//! clear). The `route` rows are replayed by `sim::building_construction`.
//!
//! The crew rows run on the slave_manager scene (`slave_manager_oracle_tests`:
//! the harvest_field world with a 2x2 YAREFN at NW (12, 12)) with the row's
//! type keys, the owner's side and the captured byte. The oracle's absorbed
//! passengers are prepared objects that drew nothing, so the Scenario RNG is
//! seeded after the Rust ones are constructed. The oracle's type does not
//! undeploy, so the scene's YAREFN loses `UndeploysInto=`. Each Scatter's
//! setter and first Walk Process run natively in the oracle (its heads and raw
//! spot bits steer the next crewman's placement and search); the oracle
//! answers only `Find_Path` with the one-step route to the neighbouring cell.
//!
//! Compared per crew row: the survivor count; each crewman's and passenger's
//! type, coordinate, NavCom and queued mission, in construction order (a
//! crewman whose cell had no free spot is deleted and leaves nothing); the
//! sounds at the building's Location; Sell's stage and `+0x6DD`; the Scenario
//! RNG cursors.
//!
//! Not compared: the OVER_OUT broadcast and Select, which the oracle answers
//! (VERA's selection does not lapse at a sale); the current mission, which
//! the oracle's answered FootClass::Unlimbo (`0x004D7170`) leaves unwritten
//! (TechnoClass::Unlimbo's Guard, `0x006F6E2A`, is the Unlimbo owner's); the
//! rows in `SKIPPED`.

use super::slave_manager_oracle_tests::{SlaveScene, row_scene_edited};
use crate::rules::ini_parser::IniFile;
use crate::rules::ruleset::RuleSet;
use crate::sim::components::NavTargetRef;
use crate::sim::house_state::HouseState;
use crate::sim::passenger::{PassengerCargo, PassengerRole};
use crate::sim::production;
use crate::sim::rng::SimRng;
use crate::sim::world::SimSoundEvent;
use serde_json::{Value, json};

/// Rows whose inputs a live sale cannot reach or VERA cannot express.
const SKIPPED: &[&str] = &[
    // NoSurvivor (`+0x6E0`) is written only by DestructionEffects: a
    // building on sale is alive.
    "c_no_survivor",
    "c_absorbed_no_survivor",
    // A country without `Side=` (HouseType `+0xBC == -1`): `side_index`
    // cannot express it (`crew_survival`'s residual; no stock country).
    "c_sideless_country",
];

/// The oracle's sound indices: `[AudioVisual] SellSound=` and the row's
/// `PackupSound=`.
const SOUNDS: [(i64, &str); 2] = [(41, "OracleSellSound"), (9, "OraclePackupSound")];

const WALK: &str = "{4A582744-9839-11D1-B709-00A024DDAFD1}";

fn corpus() -> Value {
    serde_json::from_str(include_str!(
        "../../../tools/spatial_oracle/building_sale.json"
    ))
    .unwrap()
}

fn skipped(row: &Value) -> bool {
    SKIPPED.contains(&row["input"]["name"].as_str().unwrap())
}

/// The oracle's crew types (Strength 125, Walk, MovementZone Infantry;
/// ENGINEER's `Engineer=`), the `[General]` crews, divisors and
/// RefundPercent, `SellSound=`, and the row's YAREFN keys: Cost, Crewed,
/// Factory (a yard's `+0xEB8`), PackupSound, InfantryAbsorb, Foundation and
/// a weapon for IsArmed.
fn edit_rules(input: &Value, text: &mut String) {
    *text = text.replacen(
        "0=SLAV\n",
        "0=SLAV\n1=E1\n2=E2\n3=INIT\n4=CTECH\n5=ENGINEER\n",
        1,
    );
    *text = text.replacen(
        "[General]\n",
        "[General]\nAlliedCrew=E1\nSovietCrew=E2\nThirdCrew=INIT\nTechnician=CTECH\n\
         Engineer=ENGINEER\nAlliedSurvivorDivisor=500\nSovietSurvivorDivisor=250\n\
         ThirdSurvivorDivisor=750\nRefundPercent=50%\n",
        1,
    );
    *text = text.replacen("UndeploysInto=SMIN\n", "", 1);
    let mut keys = format!(
        "Cost={}\nCrewed={}\n",
        input["cost"],
        if input["crewed"] == false {
            "no"
        } else {
            "yes"
        }
    );
    if input["yard"] == true {
        keys.push_str("Factory=BuildingType\n");
    }
    if input["packup_sound"].is_number() {
        keys.push_str(&format!("PackupSound={}\n", SOUNDS[1].1));
    }
    if input["passengers"].is_array() {
        keys.push_str("InfantryAbsorb=yes\n");
    }
    if input["foundation"] == 9 {
        keys.push_str("Foundation=3x3Refinery\n");
    }
    if input["armed"] == true {
        keys.push_str("Primary=OracleGun\n");
    }
    *text = text.replacen("[YAREFN]\n", &format!("[YAREFN]\n{keys}"), 1);
    for name in ["E1", "E2", "INIT", "CTECH", "ENGINEER"] {
        text.push_str(&format!(
            "[{name}]\nStrength=125\nSpeed=4\nMovementZone=Infantry\nLocomotor={WALK}\n{}",
            if name == "ENGINEER" {
                "Engineer=yes\n"
            } else {
                ""
            }
        ));
    }
    *text = text.replacen(
        "[AudioVisual]\n",
        &format!("[AudioVisual]\nSellSound={}\n", SOUNDS[0].1),
        1,
    );
    text.push_str("[OracleGun]\nDamage=10\nROF=10\nRange=5\nWarhead=KILLWH\n");
}

/// The row's scene: its seed, owner and type keys, the building on Sell's
/// stage 1 (Selling current, `+0xBC` = 1), its passengers loaded, and the
/// Scenario RNG seeded last. Returns the passengers in list order.
fn crew_scene(input: &Value) -> (SlaveScene, Vec<u64>) {
    let seed = input["seed"].as_u64().unwrap_or(1);
    let scene_input = json!({
        "name": input["name"],
        "manager_state": 0,
        "nodes": [],
        "ore": [],
        "seed": seed,
        "human": input["human"].as_bool().unwrap_or(true),
    });
    let mut s = row_scene_edited(&scene_input, |text| edit_rules(input, text));
    let refinery = s.refinery;
    let rules = &s.scene.rules;
    let sim = &mut s.scene.sim;
    let owner = sim.substrate.entities.get(refinery).unwrap().owner();
    sim.houses.get_mut(&owner).unwrap().side_index = input["side"].as_u64().unwrap() as u8;
    let building = sim.substrate.entities.get_mut(refinery).unwrap();
    building.has_been_captured = input["captured"] == true;
    building.selected = input["selected"] == true;
    let mut passengers = Vec::new();
    if let Some(names) = input["passengers"].as_array() {
        building.passenger_role = PassengerRole::Transport {
            cargo: PassengerCargo::new(5, 0),
        };
        for name in names {
            let id = sim
                .construct_object_limbo_at_height(
                    name.as_str().unwrap(),
                    "Americans",
                    10,
                    10,
                    0,
                    0,
                    rules,
                )
                .expect("absorbed infantry");
            sim.substrate.entities.get_mut(id).unwrap().passenger_role = PassengerRole::Inside {
                transport_id: refinery,
            };
            passengers.push(id);
        }
        // AddPassenger prepends: the list's head is the first passenger.
        for &id in passengers.iter().rev() {
            sim.substrate
                .entities
                .get_mut(refinery)
                .unwrap()
                .passenger_role
                .cargo_mut()
                .unwrap()
                .board_forced(id, 1);
        }
    }
    production::begin_selling(sim, rules, refinery, false);
    let building = sim.substrate.entities.get_mut(refinery).unwrap();
    assert!(building.building_down.is_some(), "the sale commenced");
    building.mission.set_handler_state(1);
    sim.scenario_rng = SimRng::new(seed);
    sim.sound_events.clear();
    (s, passengers)
}

fn compare_infantry(s: &SlaveScene, id: u64, expected: &Value, context: &str) {
    let sim = &s.scene.sim;
    let entity = sim.substrate.entities.get(id).unwrap();
    assert_eq!(
        sim.interner.resolve(entity.type_ref()),
        expected["type"].as_str().unwrap(),
        "{context}: type"
    );
    assert_eq!(
        sim.interner.resolve(entity.owner()) == "Americans",
        expected["owner"].as_bool().unwrap(),
        "{context}: owner"
    );
    assert_eq!(
        u64::from(entity.lifecycle.in_limbo),
        expected["limbo"].as_u64().unwrap(),
        "{context}: limbo"
    );
    let coord = crate::sim::movement::ground_pose::position_world_coord(&entity.position);
    assert_eq!(
        json!([coord.x, coord.y, coord.z]),
        expected["coord"],
        "{context}: coordinate"
    );
    let nav = match entity.navigation.nav_com {
        Some(NavTargetRef::Cell { rx, ry }) => json!([rx, ry]),
        None => Value::Null,
        Some(other) => panic!("{context}: NavCom {other:?}"),
    };
    assert_eq!(nav, expected["nav"], "{context}: NavCom");
    assert_eq!(
        entity.mission.queued().raw() as i64,
        expected["queued"].as_i64().unwrap(),
        "{context}: queued"
    );
}

/// Sell's stage 1 (`0x0044A2EE..0x0044A8DE`) on each row's building.
#[test]
fn sale_stage_one_matches_the_original_survivors_and_sounds() {
    let corpus = corpus();
    let rows: Vec<&Value> = corpus["crew"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|row| !skipped(row))
        .collect();
    for row in &rows {
        replay_crew_row(row);
    }
    assert_eq!(rows.len(), 23);
}

fn replay_crew_row(row: &Value) {
    let input = &row["input"];
    let context = input["name"].as_str().unwrap();
    let (mut s, passengers) = crew_scene(input);
    let refinery = s.refinery;
    let rules = &s.scene.rules;
    let sim = &mut s.scene.sim;
    assert_eq!(
        i64::from(sim.building_survivor_count(rules, refinery, false)),
        row["survivors"].as_i64().unwrap(),
        "{context}: How_Many_Survivors"
    );
    let before = sim.substrate.entities.keys_sorted();
    production::sell_stage_one(
        sim,
        Some(rules),
        Some(super::harvest_field_oracle_tests::registry()),
        refinery,
    );

    // The crew in construction order; a deleted crewman left nothing.
    let crew: Vec<u64> = sim
        .substrate
        .entities
        .keys_sorted()
        .into_iter()
        .filter(|id| !before.contains(id))
        .collect();
    let expected = &row["infantry"];
    let placed: Vec<&str> = expected
        .as_object()
        .unwrap()
        .iter()
        .filter(|(name, state)| name.starts_with("crew") && state["limbo"] == 0)
        .map(|(name, _)| name.as_str())
        .collect();
    assert_eq!(crew.len(), placed.len(), "{context}: crew on the map");
    for (id, name) in crew.iter().zip(&placed) {
        compare_infantry(&s, *id, &expected[*name], &format!("{context} {name}"));
    }
    for (index, id) in passengers.iter().enumerate() {
        let name = format!("passenger{index}");
        compare_infantry(&s, *id, &expected[&name], &format!("{context} {name}"));
    }

    let sim = &s.scene.sim;
    let owner = sim.interner.get("Americans").unwrap();
    let played: Vec<Value> = sim
        .sound_events
        .iter()
        .filter_map(|event| match event {
            SimSoundEvent::VocAt {
                sound_id,
                audible_to,
                rx,
                ry,
                sub_x,
                sub_y,
                world_z_leptons,
            } => {
                let index = SOUNDS
                    .iter()
                    .find(|(_, name)| name == sound_id)
                    .map(|(index, _)| *index)?;
                assert_eq!(*audible_to, Some([owner, owner]), "{context}: the owner's");
                Some(json!([
                    index,
                    [
                        i32::from(*rx) * 256 + sub_x.to_num::<i32>(),
                        i32::from(*ry) * 256 + sub_y.to_num::<i32>(),
                        world_z_leptons
                    ]
                ]))
            }
            _ => None,
        })
        .collect();
    // IsHumanPlayer (the app's gate) admits the owner's player only.
    if input["player"] != false {
        let native: Vec<Value> = row["events"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|event| event[0] == "play_at")
            .map(|event| json!([event[1], event[2]]))
            .collect();
        assert_eq!(played, native, "{context}: sounds");
    }

    let building = sim.substrate.entities.get(refinery).unwrap();
    assert_eq!(
        i64::from(building.mission.handler_state()),
        row["building"]["status"].as_i64().unwrap(),
        "{context}: Sell's stage"
    );
    assert_eq!(
        u64::from(building.building_down.unwrap().done),
        row["building"]["done"].as_u64().unwrap(),
        "{context}: +0x6DD"
    );
    assert_eq!(
        building
            .passenger_role
            .cargo()
            .map_or(0, |cargo| i64::from(cargo.count())),
        row["building"]["passengers"].as_i64().unwrap(),
        "{context}: passengers left"
    );
    let view = sim.scenario_rng.logical_view();
    assert_eq!(
        json!([view.index_a, view.index_b]),
        row["random_indices"]["after"],
        "{context}: Scenario RNG cursors"
    );
}

/// The sale's credit (`TechnoClass vt+0x2BC` = `0x0070ADA0` ->
/// `TechnoTypeClass::GetRefund 0x00711F60`, `full` clear) over cost, owner
/// and game mode.
#[test]
fn sale_refund_matches_the_original() {
    let corpus = corpus();
    let mut compared = 0;
    for row in corpus["refund"].as_array().unwrap() {
        let input = &row["input"];
        let context = input["name"].as_str().unwrap();
        let rules = RuleSet::from_ini(&IniFile::from_str(&format!(
            "[General]\nRefundPercent=50%\n[BuildingTypes]\n0=YAREFN\n\
             [YAREFN]\nCost={}\nStrength=2000\n",
            input["cost"]
        )))
        .unwrap();
        let owner = crate::sim::intern::test_intern("Americans");
        let mut house = HouseState::new(owner, 0, None, true, 0, 10);
        house.is_human = input["human"].as_bool().unwrap();
        house.player_control = input["player_control"] == true;
        let game_mode_nonzero = input["game_mode"].as_u64().unwrap() != 0;
        assert_eq!(
            i64::from(production::building_type_refund(
                &rules,
                rules.object("YAREFN").unwrap(),
                &house,
                game_mode_nonzero,
                false,
            )),
            row["refund"].as_i64().unwrap(),
            "{context}"
        );
        compared += 1;
    }
    assert_eq!(compared, 34);
}
