//! A spy's infiltration: `BuildingClass::Infiltrate @ 0x004571E0` against
//! its native execution (`tools/spy_infiltration_oracle.json`), the
//! What_Action arm that orders it and the PerCellProcess arm that runs it.

use serde_json::Value;

use crate::rules::ini_parser::IniFile;
use crate::rules::ruleset::RuleSet;
use crate::sim::combat::TargetKind;
use crate::sim::combat::veterancy::VeterancyRank;
use crate::sim::command::{Command, CommandEnvelope};
use crate::sim::house_state::HouseState;
use crate::sim::intern::InternedId;
use crate::sim::mission::{MissionId, MissionType};
use crate::sim::power_system::PowerState;
use crate::sim::spy_infiltration::InfiltrationEffect;
use crate::sim::superweapon::SuperWeaponInstance;
use crate::sim::world::{FrameEffects, SimSoundEvent, Simulation};

/// The oracle's `infiltrate` rows.
pub fn rows() -> Vec<Value> {
    let data: Value = serde_json::from_str(crate::test_fixture::text(
        "tools/spy_infiltration_oracle.json",
    ))
    .unwrap();
    data["infiltrate"].as_array().unwrap().clone()
}

/// The houses a row names: the building's owner, the spy's, and a third
/// that only watches.
pub const HOUSES: [(&str, &str); 3] = [("owner", "H0"), ("infiltrator", "H1"), ("bystander", "H2")];

/// A row after [`Simulation::infiltrate_building`].
pub struct Replay {
    pub sim: Simulation,
    pub rules: RuleSet,
    owner: InternedId,
    infiltrator: InternedId,
}

fn int(value: &Value) -> i32 {
    i32::try_from(value.as_i64().unwrap()).unwrap()
}

fn flag(value: &Value) -> bool {
    value.as_bool().unwrap()
}

/// The row's building type `B`, its Super `SW0`, its rules values, and three
/// types `T0`..`T2` that need the stolen-tech bytes `+0x2BC`..`+0x2BE`.
fn rules_for(row: &Value) -> RuleSet {
    let kind = &row["type"];
    let mut ini = format!(
        "[General]\nSpyPowerBlackout={}\nSpyMoneyStealPercent={}\n\
         [InfantryTypes]\n0=T0\n1=T1\n2=T2\n\
         [T0]\nRequiresStolenThirdTech=yes\n[T1]\nRequiresStolenSovietTech=yes\n\
         [T2]\nRequiresStolenAlliedTech=yes\n\
         [SuperWeaponTypes]\n0=SW0\n[SW0]\nType=MultiMissile\n\
         [BuildingTypes]\n0=B\n[B]\nStrength=800\nPower={}\nAIBasePlanningSide={}\nStorage={}\n",
        int(&row["rules"]["blackout"]),
        row["rules"]["percent"].as_str().unwrap(),
        int(&kind["power"]),
        int(&kind["side"]),
        int(&kind["storage"]),
    );
    if flag(&kind["radar"]) {
        ini += "Radar=yes\n";
    }
    if flag(&kind["super_weapon"]) {
        ini += "SuperWeapon=SW0\n";
    }
    if let Some(factory) = kind["factory"].as_str() {
        ini += &format!(
            "Factory={}Type\n",
            match factory {
                "unit" => "Unit",
                "infantry" => "Infantry",
                "aircraft" => "Aircraft",
                _ => "Building",
            }
        );
    }
    if flag(&kind["build_tech"]) {
        ini += "[AI]\nBuildTech=B\n";
    }
    let mut rules = RuleSet::from_ini(&IniFile::from_str(&ini)).unwrap();
    // The oracle writes the type's frame count (`+0xB0`) itself: a
    // `RechargeTime=` of 0 minutes would keep the constructor's.
    if let Some(recharge) = row["super"]["recharge"].as_i64() {
        for sw in rules.super_weapons.values_mut() {
            sw.recharge_time_frames = recharge as i32;
        }
    }
    rules
}

/// Builds the row's fixture as the oracle does (only the local house is
/// human, GameMode 1) and runs the infiltration.
pub fn replay(row: &Value) -> Replay {
    let rules = rules_for(row);
    let mut sim = Simulation::new();
    sim.intern_rule_type_ids(&rules);
    sim.resolve_type_handles(&rules);
    sim.session
        .set_game_mode(crate::sim::scenario_session::ScenarioGameMode::from_legacy_nonzero(true));
    let local = row["local"].as_str().unwrap();
    for (role, name) in HOUSES {
        let id = sim.interner.intern(name);
        let credits = match role {
            "owner" => int(&row["owner"]["balance"]),
            "infiltrator" => int(&row["infiltrator"]["balance"]),
            _ => 0,
        };
        sim.houses
            .register(HouseState::new(id, 0, None, role == local, credits, 10));
    }
    let owner = sim.interner.intern("H0");
    let infiltrator = sim.interner.intern("H1");
    let building = sim.spawn_object("B", "H0", 37, 52, 0, &rules).unwrap();
    let house = sim.houses.get_mut(&owner).unwrap();
    house.set_spy_sat_active_for_test(flag(&row["owner"]["spy_sat"]));
    house.set_map_is_clear_for_test(true);
    sim.power_states
        .map_mut_for_test()
        .insert(owner, PowerState::settled_for_test());
    if let Some(state) = row["super"].as_object() {
        let sw = sim.interner.intern("SW0");
        let mut instance = SuperWeaponInstance::new(sw, owner, 0);
        instance.set_is_active_for_test(true);
        instance.set_is_ready_for_test(flag(&state["ready"]));
        instance.set_charge_start_tick_for_test(int(&state["start"]));
        instance.set_charge_duration_for_test(int(&state["duration"]));
        sim.super_weapons
            .map_mut_for_test()
            .entry(owner)
            .or_default()
            .insert(sw, instance);
    }
    sim.session
        .set_binary_frame_for_test(int(&row["frame"]) as u32);
    sim.sound_events.clear_for_test();
    let spy = if flag(&row["own"]) {
        owner
    } else {
        infiltrator
    };
    if let Some(gains) = row["initial_gains"].as_array() {
        assert_eq!(gains.len(), 5);
        let house = sim.houses.get_mut(&infiltrator).unwrap();
        for (slot, side) in [(0, 2), (1, 1), (2, 0)] {
            if int(&gains[slot]) != 0 {
                house.steal_tech(side);
            }
        }
        if int(&gains[3]) != 0 {
            house.grant_veteran_infantry();
        }
        if int(&gains[4]) != 0 {
            house.grant_veteran_units();
        }
        for (slot, kind) in ["T0", "T1", "T2"].into_iter().enumerate() {
            assert_eq!(
                house.has_stolen_tech_for(rules.object(kind).unwrap()),
                int(&gains[slot]) != 0,
                "{}: supplied prior stolen gain",
                row["name"],
            );
        }
        assert_eq!(house.veteran_infantry(), int(&gains[3]) != 0);
        assert_eq!(house.veteran_units(), int(&gains[4]) != 0);
    }
    // Match the native settled input AFTER installing prior gains. Neither
    // constructor nor earlier gain requests may satisfy the measured write.
    for house in sim.houses.values_mut() {
        house.take_tech_recheck();
        assert!(!house.tech_recheck_pending());
    }
    sim.infiltrate_building(building, spy, &rules);
    Replay {
        sim,
        rules,
        owner,
        infiltrator,
    }
}

/// Each row's house, power and Super state after `0x004571E0`. The radar
/// event and EVA lines are the app's (`sound_dispatch` replays them).
#[test]
fn infiltrate_matches_the_original() {
    for row in rows() {
        let name = row["name"].as_str().unwrap();
        let expected = &row["expected"];
        let Replay {
            sim,
            rules,
            owner,
            infiltrator,
        } = replay(&row);
        let (victim, spy) = (&sim.houses[&owner], &sim.houses[&infiltrator]);
        assert_eq!(
            victim.tech_recheck_pending(),
            int(&expected["owner"]["recheck_tech"]) != 0,
            "{name}: victim House tech request"
        );
        assert_eq!(
            spy.tech_recheck_pending(),
            int(&expected["infiltrator"]["recheck_tech"]) != 0,
            "{name}: infiltrator House tech request"
        );
        assert_eq!(
            [victim.economy().credits(), victim.economy().spent_credits()],
            [
                int(&expected["owner"]["balance"]),
                int(&expected["owner"]["spent"])
            ],
            "{name}: the owner's money"
        );
        assert_eq!(
            spy.economy().credits(),
            int(&expected["infiltrator"]["balance"]),
            "{name}: the spy's money"
        );
        let power = &sim.power_states[&owner];
        let blackout = power.blackout_timer_for_test();
        assert_eq!(
            [blackout.start_frame(), blackout.duration()],
            [
                int(&expected["owner"]["blackout"][0]),
                int(&expected["owner"]["blackout"][1])
            ],
            "{name}: the blackout timer"
        );
        assert_eq!(
            power.power_recheck_for_test(),
            int(&expected["owner"]["recheck_power"]) == 1,
            "{name}: RecheckPower"
        );
        let stolen: Vec<i32> = ["T0", "T1", "T2"]
            .into_iter()
            .map(|kind| i32::from(spy.has_stolen_tech_for(rules.object(kind).unwrap())))
            .collect();
        let expected_stolen: Vec<i32> = expected["infiltrator"]["stolen"]
            .as_array()
            .unwrap()
            .iter()
            .map(int)
            .collect();
        assert_eq!(stolen, expected_stolen, "{name}: the stolen technology");
        assert_eq!(
            [
                i32::from(spy.veteran_infantry()),
                i32::from(spy.veteran_units())
            ],
            [
                int(&expected["infiltrator"]["veteran"][0]),
                int(&expected["infiltrator"]["veteran"][1])
            ],
            "{name}: the veteran factories"
        );
        let reshrouded = expected["events"]
            .as_array()
            .unwrap()
            .iter()
            .any(|event| event[0] == "reset_shroud");
        assert_eq!(!victim.map_is_clear(), reshrouded, "{name}: Reset_Shroud");
        if let Some(state) = expected.get("super") {
            let sw = sim.interner.get("SW0").unwrap();
            let instance = &sim.super_weapons[&owner][&sw];
            assert_eq!(
                (
                    i32::from(instance.is_ready()),
                    instance.charge_start_tick(),
                    instance.charge_duration()
                ),
                (
                    int(&state["ready"]),
                    int(&state["timer"][0]),
                    int(&state["timer"][1])
                ),
                "{name}: the Super"
            );
        }
        // The sim announces whenever the owner or the spy's house is
        // human-controlled; only the local one is here.
        let announced = sim
            .sound_events
            .iter()
            .any(|event| matches!(event, SimSoundEvent::BuildingInfiltrated { .. }));
        assert_eq!(
            announced,
            !flag(&row["own"]) && row["local"] != "bystander",
            "{name}: the announcement"
        );
    }
}

/// Every spy gain changes the world hash.
#[test]
fn spy_gains_are_hashed() {
    let mut sim = Simulation::new();
    let house = sim.interner.intern("H1");
    sim.houses
        .register(HouseState::new(house, 0, None, true, 0, 10));
    let mut seen = vec![sim.state_hash()];
    for gain in [
        |house: &mut HouseState| house.steal_tech(0),
        |house: &mut HouseState| house.steal_tech(1),
        |house: &mut HouseState| house.steal_tech(2),
        |house: &mut HouseState| house.grant_veteran_infantry(),
        |house: &mut HouseState| house.grant_veteran_units(),
    ] {
        gain(sim.houses.get_mut(&house).unwrap());
        let hash = sim.state_hash();
        assert!(!seen.contains(&hash), "a spy gain left the hash unchanged");
        seen.push(hash);
    }
}

const ARENA_RULES: &str = "[InfantryTypes]\n0=SPY\n1=THIEF\n2=GI\n\
    [VehicleTypes]\n0=TANK\n1=BOAT\n\
    [BuildingTypes]\n0=PLANT\n1=DEPOT\n2=YARD\n3=WALL\n\
    [SPY]\nStrength=100\nSpeed=4\nSpeedType=Foot\nInfiltrate=yes\nAgent=yes\n\
    Locomotor={4A582744-9839-11d1-B709-00A024DDAFD1}\n\
    [THIEF]\nStrength=100\nSpeed=4\nSpeedType=Foot\nInfiltrate=yes\n\
    Locomotor={4A582744-9839-11d1-B709-00A024DDAFD1}\n\
    [GI]\nStrength=125\nSpeed=4\nSpeedType=Foot\n\
    Locomotor={4A582744-9839-11d1-B709-00A024DDAFD1}\n\
    [TANK]\nStrength=300\nSpeed=6\nSpeedType=Track\n\
    Locomotor={4A582745-9839-11d1-B709-00A024DDAFD1}\n\
    [BOAT]\nStrength=300\nSpeed=6\nSpeedType=Track\nNaval=yes\n\
    Locomotor={4A582745-9839-11d1-B709-00A024DDAFD1}\n\
    [PLANT]\nStrength=750\nPower=100\nSpyable=yes\nCapturable=yes\n\
    [DEPOT]\nStrength=750\nPower=-20\nCapturable=yes\n\
    [YARD]\nStrength=1000\nUndeploysInto=TANK\nTrainable=yes\n\
    [WALL]\nStrength=300\nSpyable=yes\n\
    [General]\nSpyPowerBlackout=1000\n";

const ARENA_ART: &str = "[PLANT]\nFoundation=2x2\n[DEPOT]\nFoundation=2x2\n\
    [YARD]\nFoundation=2x2\n[WALL]\nFoundation=1x1\n";

fn arena_rules() -> RuleSet {
    RuleSet::from_ini_with_fixed_art_for_test(
        &crate::sim::arena_fixture::clear_ground_ini(ARENA_RULES),
        &IniFile::from_str(ARENA_ART),
    )
    .unwrap()
}

/// The flat arena with the human `Americans`, who are the local player,
/// and the computer `Russians`.
fn arena(rules: &RuleSet) -> (Simulation, crate::sim::pathfinding::PathGrid) {
    let mut sim = crate::sim::arena_fixture::rival_arena(rules, 0x4571E0);
    sim.intern_rule_type_ids(rules);
    sim.resolve_type_handles(rules);
    sim.set_input_delay_ticks(0);
    sim.session
        .set_game_mode(crate::sim::scenario_session::ScenarioGameMode::from_legacy_nonzero(true));
    sim.session
        .set_current_house_for_test(sim.interner.get("Americans"));
    let grid = sim
        .path_grid_snapshot()
        .map(|grid| (*grid).clone())
        .unwrap();
    (sim, grid)
}

fn spawn(sim: &mut Simulation, rules: &RuleSet, kind: &str, owner: &str, rx: u16, ry: u16) -> u64 {
    sim.spawn_object_at_height(kind, owner, rx, ry, 0, 0, rules)
        .unwrap_or_else(|| panic!("{kind} spawns"))
}

/// `InfantryClass::What_Action_OnObject`'s infiltration arm
/// (`0x0051EE3B..0x0051EEEF`): the local player's `Infiltrate=`
/// infantryman over another house's building that its flags admit.
#[test]
fn the_infiltration_arm_takes_only_admitted_buildings() {
    let rules = arena_rules();
    let (mut sim, _) = arena(&rules);
    let spy = spawn(&mut sim, &rules, "SPY", "Americans", 4, 4);
    let thief = spawn(&mut sim, &rules, "THIEF", "Americans", 5, 4);
    let gi = spawn(&mut sim, &rules, "GI", "Americans", 6, 4);
    let plant = spawn(&mut sim, &rules, "PLANT", "Russians", 10, 10);
    let depot = spawn(&mut sim, &rules, "DEPOT", "Russians", 14, 10);
    let own_plant = spawn(&mut sim, &rules, "PLANT", "Americans", 10, 16);
    let computer_spy = spawn(&mut sim, &rules, "SPY", "Russians", 20, 20);
    let takes =
        |sim: &Simulation, actor, target| sim.infiltrator_building_action(actor, target, &rules);

    // An Agent= spy needs Spyable=; any other infiltrator Capturable=.
    assert!(takes(&sim, spy, plant));
    assert!(!takes(&sim, spy, depot), "a building that is not Spyable=");
    assert!(takes(&sim, thief, depot));
    assert!(takes(&sim, thief, plant));
    assert!(!takes(&sim, gi, plant), "not Infiltrate=");
    assert!(!takes(&sim, spy, own_plant), "its own house's building");
    assert!(!takes(&sim, spy, gi), "not a building");
    assert!(
        !takes(&sim, computer_spy, own_plant),
        "a house other than the local player's"
    );

    // Is_Ally_ByObject asks the spy's house only (`0x004F9A90`).
    let alliances = sim.house_alliances_mut_for_test();
    alliances
        .entry("RUSSIANS".to_string())
        .or_default()
        .insert("AMERICANS".to_string());
    assert!(takes(&sim, spy, plant), "only the building's house allies");
    sim.house_alliances_mut_for_test()
        .entry("AMERICANS".to_string())
        .or_default()
        .insert("RUSSIANS".to_string());
    assert!(!takes(&sim, spy, plant), "the spy's house allies");
}

/// Runs the production frame until `done` holds, for at most `frames`
/// frames, keeping every sound event; answers whether it held.
fn run_until(
    sim: &mut Simulation,
    rules: &RuleSet,
    grid: &crate::sim::pathfinding::PathGrid,
    frames: u32,
    events: &mut Vec<SimSoundEvent>,
    mut done: impl FnMut(&Simulation) -> bool,
) -> bool {
    for _ in 0..frames {
        let commands = sim.take_due_commands();
        sim.advance_tick(&commands, Some(rules), Some(grid), None, 33);
        events.extend(sim.sound_events.take_for_test());
        if done(sim) {
            return true;
        }
    }
    false
}

fn capture(sim: &mut Simulation, actor: u64, building: u64) {
    let owner = sim.interner.get("Americans").unwrap();
    let tick = sim.session.tick();
    sim.queue_command(CommandEnvelope::new(
        owner,
        tick + 1,
        Command::CaptureBuilding {
            engineer_id: actor,
            target_building_id: building,
        },
    ));
}

/// The spy's Capture order walks it into the enemy power plant: its
/// PerCellProcess arm (`0x00519FF8..0x0051A02E`) infiltrates the building,
/// whose owner's power blacks out for `SpyPowerBlackout=`, and removes it.
#[test]
fn a_spy_sent_into_an_enemy_power_plant_blacks_it_out() {
    let rules = arena_rules();
    let (mut sim, grid) = arena(&rules);
    let spy = spawn(&mut sim, &rules, "SPY", "Americans", 6, 10);
    let plant = spawn(&mut sim, &rules, "PLANT", "Russians", 10, 10);
    let (americans, russians) = (
        sim.interner.get("Americans").unwrap(),
        sim.interner.get("Russians").unwrap(),
    );
    capture(&mut sim, spy, plant);
    let mut events = Vec::new();
    // The arm's UnInit (`0x0051A02E`); deleting the object is the frame
    // tail's.
    assert!(
        run_until(&mut sim, &rules, &grid, 300, &mut events, |sim| {
            sim.entities()
                .get(spy)
                .is_none_or(|spy| !spy.lifecycle.object_alive())
        }),
        "the spy never went in"
    );
    let infiltrations: Vec<_> = events
        .iter()
        .filter_map(|event| match event {
            SimSoundEvent::BuildingInfiltrated {
                owner,
                infiltrator,
                effect,
                radar,
            } => Some((*owner, *infiltrator, *effect, (radar.rx, radar.ry))),
            _ => None,
        })
        .collect();
    let plant_cell = {
        let plant = sim.entities().get(plant).unwrap();
        (plant.position.rx(), plant.position.ry())
    };
    assert_eq!(
        infiltrations,
        [(russians, americans, InfiltrationEffect::Power, plant_cell)]
    );
    let frame = sim.session.binary_frame();
    let blackout = sim.power_states[&russians].blackout_timer_for_test();
    assert_eq!(blackout.duration(), 1000);
    assert!(blackout.remaining(frame as i32) > 0);
    assert_eq!(sim.entities().get(plant).unwrap().owner(), russians);
}

/// The computer's way in: a computer house's spy handed an enemy power
/// plant on Attack, as a team's action 46 hands it one. `Assign_Target`'s
/// infiltration arm heads it for the plant, Mission_Attack's computer arm
/// puts it on Capture, and it walks in and blacks the plant's owner out.
#[test]
fn a_computer_spy_attacking_a_power_plant_goes_in() {
    let rules = arena_rules();
    let (mut sim, grid) = arena(&rules);
    // Off the sixteenth frame, whose AI would let go of a target the
    // unarmed spy cannot fire at before its mission runs
    // (`0x006FA472..0x006FA4CB`); a team's next update hands it back.
    sim.session.set_binary_frame_for_test(1);
    let spy = spawn(&mut sim, &rules, "SPY", "Russians", 6, 10);
    let plant = spawn(&mut sim, &rules, "PLANT", "Americans", 10, 10);
    let (americans, russians) = (
        sim.interner.get("Americans").unwrap(),
        sim.interner.get("Russians").unwrap(),
    );
    let now = sim.session.binary_frame();
    sim.substrate
        .mission_assign_exact(spy, MissionId::from_known(MissionType::Attack), now)
        .unwrap();
    sim.assign_target_represented(
        spy,
        Some(TargetKind::Entity(plant)),
        &rules,
        None,
        FrameEffects::default(),
    )
    .unwrap();
    let mut events = Vec::new();
    let mut captured = false;
    assert!(
        run_until(&mut sim, &rules, &grid, 300, &mut events, |sim| {
            let Some(spy) = sim.entities().get(spy) else {
                return true;
            };
            captured |= spy.mission.current() == MissionId::from_known(MissionType::Capture);
            !spy.lifecycle.object_alive()
        }),
        "the spy never went in"
    );
    assert!(captured, "the spy never took Capture");
    let infiltrations: Vec<_> = events
        .iter()
        .filter_map(|event| match event {
            SimSoundEvent::BuildingInfiltrated {
                owner,
                infiltrator,
                effect,
                ..
            } => Some((*owner, *infiltrator, *effect)),
            _ => None,
        })
        .collect();
    assert_eq!(
        infiltrations,
        [(americans, russians, InfiltrationEffect::Power)]
    );
    let frame = sim.session.binary_frame();
    assert!(sim.power_states[&americans].blackout_remaining(frame) > 0);
}

/// An `Infiltrate=` infantryman that is not an `Agent=` takes the Capture
/// arm over a `Capturable=` building, but its PerCellProcess arm only drops
/// its destination and scatters it from the building
/// (`0x0051A03E..0x0051A061`): no infiltration, no capture.
#[test]
fn an_infiltrator_that_is_no_agent_is_scattered_from_the_building() {
    let rules = arena_rules();
    let (mut sim, grid) = arena(&rules);
    let thief = spawn(&mut sim, &rules, "THIEF", "Americans", 6, 10);
    let depot = spawn(&mut sim, &rules, "DEPOT", "Russians", 10, 10);
    let russians = sim.interner.get("Russians").unwrap();
    capture(&mut sim, thief, depot);
    let mut events = Vec::new();
    let footprint = |sim: &Simulation, id: u64| {
        let entity = sim.entities().get(id).unwrap();
        (entity.position.rx(), entity.position.ry())
    };
    let mut entered = false;
    run_until(&mut sim, &rules, &grid, 300, &mut events, |sim| {
        let (rx, ry) = footprint(sim, thief);
        entered |= (10..12).contains(&rx) && (10..12).contains(&ry);
        entered
            && sim
                .entities()
                .get(thief)
                .unwrap()
                .navigation
                .nav_com()
                .is_none()
    });
    assert!(entered, "the infiltrator never reached the building");
    let entity = sim.entities().get(thief).expect("the infiltrator stays");
    assert!(entity.navigation.nav_com().is_none());
    assert!(
        !events
            .iter()
            .any(|event| matches!(event, SimSoundEvent::BuildingInfiltrated { .. }))
    );
    assert_eq!(sim.entities().get(depot).unwrap().owner(), russians);
}

/// After a spy's factory infiltration, the house's later objects start
/// veteran where InitFromType asks the flag: `Trainable=` infantry for a
/// barracks; `Trainable=` units that are not `Naval=`, and buildings that
/// undeploy, for a war factory.
#[test]
fn factory_infiltration_makes_later_objects_veteran() {
    let rules = arena_rules();
    let (mut sim, _) = arena(&rules);
    let americans = sim.interner.get("Americans").unwrap();
    let rank = |sim: &mut Simulation, kind: &str, rx: u16| {
        let id = spawn(sim, &rules, kind, "Americans", rx, 20);
        sim.entities().get(id).unwrap().veterancy.rank_kind()
    };
    assert_eq!(rank(&mut sim, "GI", 2), VeterancyRank::Rookie);
    assert_eq!(rank(&mut sim, "TANK", 4), VeterancyRank::Rookie);

    let house = sim.houses.get_mut(&americans).unwrap();
    house.grant_veteran_infantry();
    assert_eq!(rank(&mut sim, "GI", 6), VeterancyRank::Veteran);
    assert_eq!(rank(&mut sim, "TANK", 8), VeterancyRank::Rookie);

    sim.houses
        .get_mut(&americans)
        .unwrap()
        .grant_veteran_units();
    assert_eq!(rank(&mut sim, "TANK", 10), VeterancyRank::Veteran);
    assert_eq!(rank(&mut sim, "YARD", 12), VeterancyRank::Veteran);
    assert_eq!(rank(&mut sim, "PLANT", 16), VeterancyRank::Rookie);
    assert_eq!(rank(&mut sim, "BOAT", 20), VeterancyRank::Rookie);
}

/// Retail rules through the production reader: the Spy is the one
/// `Agent=` infiltrator, the `Spyable=` buildings, `[AI] BuildTech=`, and
/// the two `[General]` values its effects read.
#[test]
fn retail_rules_name_the_spy_and_what_it_enters() {
    let Some(retail) = crate::rules::retail_ini_fixture::retail_battle_rules() else {
        return;
    };
    let rules = &retail.rules;
    let spy = rules.object("SPY").unwrap();
    assert!(spy.infiltrate && spy.agent);
    assert!(!rules.object("E1").unwrap().infiltrate);
    for kind in [
        "ENGINEER",
        "SENGINEER",
        "YENGINEER",
        "GHOST",
        "PTROOP",
        "TANY",
    ] {
        let object = rules.object(kind).unwrap();
        assert!(object.infiltrate && !object.agent, "{kind}");
    }
    for kind in [
        "GAPOWR", "NAPOWR", "NANRCT", "GAPILE", "NAWEAP", "YATECH", "AMRADR",
    ] {
        assert!(rules.object(kind).unwrap().spyable, "{kind}");
    }
    for kind in ["GACNST", "NAFLAK", "GAYARD"] {
        assert!(!rules.object(kind).unwrap().spyable, "{kind}");
    }
    for kind in ["GATECH", "NATECH", "YATECH"] {
        assert!(rules.is_build_tech(kind), "{kind}");
    }
    assert_eq!(rules.general.spy_power_blackout, 1000);
    assert_eq!(rules.general.spy_money_steal_percent.bits(), 0x3F00_0000);
}

/// Without the keys, the two `[General]` values are the RulesClass
/// constructor's: `SpyPowerBlackout` 0 (`0x0066680A`) and
/// `SpyMoneyStealPercent` 0.5f (`0x00666810`).
#[test]
fn spy_rules_default_to_the_constructor_values() {
    let rules = RuleSet::from_ini(&IniFile::from_str("")).unwrap();
    assert_eq!(rules.general.spy_power_blackout, 0);
    assert_eq!(rules.general.spy_money_steal_percent.bits(), 0x3F00_0000);
}
