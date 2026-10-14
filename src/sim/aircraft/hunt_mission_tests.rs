//! Replays `tools/spatial_oracle/aircraft_hunt.json`: the original
//! `0x00414A80`, one call per row, through [`hunt_visit`].

use super::*;
use crate::sim::mission::{MissionId, MissionType};
use serde_json::{Value, json};

/// Answers each call from the row and logs it as the oracle logs the native
/// call it stands for; keeps the Target as Assign_Target leaves it and the
/// current mission as the idle mode leaves it.
struct Recorder {
    input: Value,
    scans: Vec<Value>,
    target: Option<String>,
    current: i64,
    calls: Vec<Value>,
}

impl HuntHost for Recorder {
    type Threat = String;

    fn target(&mut self) -> bool {
        self.calls.push(json!(["target"]));
        self.target.is_some()
    }

    fn greatest_threat(&mut self, mask: u32) -> Option<String> {
        self.calls.push(json!(["greatest_threat", mask, 0]));
        let answer = self.scans.remove(0);
        answer.as_str().map(str::to_owned)
    }

    fn assign_target(&mut self, threat: Option<String>) {
        self.calls.push(json!(["assign_target", threat]));
        self.target = threat;
    }

    fn queue(&mut self, mission: MissionType) {
        self.calls.push(json!(["queue", mission.id(), 0]));
    }

    fn enter_idle_mode(&mut self) {
        self.calls.push(json!(["enter_idle_mode", 0, 1]));
        if let Some(mission) = self.input["idle_commences"].as_i64() {
            self.current = mission;
        }
    }

    fn leave_team(&mut self) {
        self.calls.push(json!(["leave_team", "aircraft", -1, 0]));
    }

    fn epilogue(&mut self) -> i32 {
        self.calls.push(json!(["jitter", 0, 2]));
        let rate = self.input["rates"][self.current.to_string()]
            .as_i64()
            .unwrap();
        (rate + self.input["jitter"].as_i64().unwrap()) as i32
    }
}

#[test]
fn hunt_matches_the_original() {
    let oracle: Value = serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/aircraft_hunt.json",
    ))
    .expect("aircraft_hunt.json");
    let rows = oracle["mission_hunt"].as_array().unwrap();
    assert_eq!(rows.len(), 102);
    for row in rows {
        let input = &row["input"];
        let name = input["name"].as_str().unwrap();
        let facts = HuntFacts {
            ammo: input["ammo"].as_i64().unwrap() as i32,
            team: input["team"].as_bool().unwrap(),
            game_mode: input["game_mode"].as_i64().unwrap() != 0,
        };
        let mut host = Recorder {
            input: input.clone(),
            scans: input["scans"].as_array().unwrap().clone(),
            target: input["target"].as_bool().unwrap().then(|| "any".to_owned()),
            current: i64::from(MissionType::Hunt.id()),
            calls: Vec::new(),
        };
        let delay = hunt_visit(&facts, &mut host);
        assert_eq!(
            i64::from(delay),
            row["ret"].as_i64().unwrap(),
            "{name}: delay"
        );
        assert_eq!(Value::from(host.calls), row["calls"], "{name}: calls");
        assert_eq!(json!(host.target), row["target"], "{name}: Target");
        assert!(host.scans.is_empty(), "{name}: unused scan answers");
    }
}

const RULES: &str = "[General]\nFlightLevel=1500\n\
[VehicleTypes]\n0=TANK\n1=MINER\n[AircraftTypes]\n0=ORCA\n[BuildingTypes]\n0=POWER\n1=REFN\n\
[POWER]\nStrength=750\nPower=100\nFoundation=2x2\n\
[ORCA]\nStrength=150\nSpeed=14\nAmmo=1\nLandable=yes\nFighter=yes\nPrimary=TestGun\n\
Locomotor={4A582746-9839-11D1-B709-00A024DDAFD1}\n\
[TestGun]\nDamage=10\nRange=6\nROF=60\nProjectile=TestShot\nWarhead=TestWH\n\
[TestShot]\nROT=0\n[TestWH]\nVerses=100%\n\
[TANK]\nStrength=1000\nLocomotor={4A582741-9839-11D1-B709-00A024DDAFD1}\n\
[MINER]\nStrength=1000\nHarvester=yes\nStorage=40\n\
Locomotor={4A582741-9839-11D1-B709-00A024DDAFD1}\n\
[REFN]\nStrength=900\nRefinery=yes\nStorage=200\nFoundation=2x2\n";

/// All_To_Hunt (`0x00501400`), which a computer house's Strategy runs once
/// it has lost its last factory, queues Hunt on every Foot of the house.
/// In multiplayer a hunting computer aircraft goes for an enemy object with
/// `Storage=`, a harvester or a refinery, passing over a nearer tank, and
/// strikes it; outside multiplayer it finds nothing and its idle mode picks
/// again.
#[test]
fn a_hunting_computer_aircraft_strikes_storage_in_multiplayer_only() {
    use crate::rules::{ini_parser::IniFile, ruleset::RuleSet};
    use crate::sim::combat::TargetKind;
    use crate::sim::house_state::HouseState;
    use crate::sim::world::Simulation;
    let rules = RuleSet::from_ini(&IniFile::from_str(RULES)).expect("rules");
    for (game_mode, quarry) in [(true, "MINER"), (true, "REFN"), (false, "MINER")] {
        let mut sim = Simulation::new();
        sim.session.game_mode_nonzero = game_mode;
        let mut owners = Vec::new();
        // The hunter's house is a computer one, as All_To_Hunt's caller's,
        // and passive, so its own Strategy stays out of the run. The enemy
        // is human and not passive, which would keep its objects out of
        // every scan (`0x006F826E`).
        for (index, (name, human)) in [("Americans", false), ("Russians", true)]
            .into_iter()
            .enumerate()
        {
            let id = sim.interner.intern(name);
            let mut house = HouseState::new(id, index as u8, None, human, 0, 10);
            house.multiplay_passive = !human;
            sim.houses.insert(id, house);
            sim.session.house_order.push(id);
            owners.push(id);
            // Under `ShortGame=` a house without a building is defeated.
            sim.spawn_object("POWER", name, 2 + 40 * index as u16, 40, 0, &rules)
                .expect("power plant");
        }
        let orca = sim
            .spawn_object_at_height("ORCA", "Americans", 10, 10, 0, 0, &rules)
            .expect("aircraft");
        let tank = sim
            .spawn_object("TANK", "Russians", 14, 10, 0, &rules)
            .expect("tank");
        let storage = sim
            .spawn_object(quarry, "Russians", 34, 26, 0, &rules)
            .expect(quarry);
        crate::sim::house_strategy::all_to_hunt(
            &mut sim,
            &rules,
            owners[0],
            None,
            crate::sim::world::FrameEffects::default(),
        );
        let mut hunted = false;
        let mut targets = Vec::new();
        let mut fired = false;
        for _ in 0..1500 {
            let _ = sim.advance_tick(&[], Some(&rules), None, None, 33);
            let Some(entity) = sim.substrate.entities.get(orca) else {
                break;
            };
            hunted |= entity.mission.current() == MissionId::from_known(MissionType::Hunt);
            if let Some(attack) = entity.attack_target.target()
                && !targets.contains(&attack)
            {
                targets.push(attack);
            }
            fired |= entity
                .aircraft_ammo
                .as_ref()
                .is_some_and(|ammo| ammo.current() == 0);
            if fired {
                break;
            }
        }
        let case = format!("{quarry}, game mode {game_mode}");
        assert!(hunted, "{case}: the aircraft hunts");
        assert!(
            !targets.contains(&TargetKind::Entity(tank)),
            "{case}: never the tank"
        );
        if game_mode {
            assert_eq!(targets, [TargetKind::Entity(storage)], "{case}: its target");
            assert!(fired, "{case}: it strikes");
        } else {
            assert!(targets.is_empty(), "{case}: it finds nothing");
            assert!(!fired, "{case}");
        }
    }
}
