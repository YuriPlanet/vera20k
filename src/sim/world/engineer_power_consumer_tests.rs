//! Bounded original House consumer comparisons from house_power_consumers.
//!
//! The admitted Building/House and frame195 Factory prior are supplied, as in
//! the native fixture. Sampling440042, repair450630, FactoryAI4C9B20 before
//! House4F8440, power508C30/rate4CA6E0/radar508DF0 and later advice4F8B08 are
//! compared through their existing owners. Full constructors, interposed
//! House AI, stock damage Anim allocation and nonlocal projection are outside
//! these controls; see tools/spatial_oracle/house_power_consumers.md.

use super::Simulation;
use crate::map::entities::EntityCategory;
use crate::rules::ini_parser::IniFile;
use crate::rules::ruleset::RuleSet;
use crate::sim::components::Health;
use crate::sim::estimated_health::EstimatedHealth;
use crate::sim::game_entity::GameEntity;
use crate::sim::house_eva::{self, EVA_LOW_POWER};
use crate::sim::house_state::HouseState;
use crate::sim::intern::InternedId;
use crate::sim::mission::state::MissionTestFixture;
use crate::sim::mission::{MissionDispatchTimer, MissionId, MissionType};
use crate::sim::power_system::{self, PowerState};
use crate::sim::production::{self, ProductionCategory};
use crate::sim::timer::CdTimer;
use crate::sim::world::SimSoundEvent;
use serde_json::{Value, json};

const PLANT: u64 = 1;
const DRAIN: u64 = 2;
const RADAR: u64 = 20;
const SECOND_RADAR: u64 = 10;
const PRODUCER: u64 = 30;

fn corpus() -> Value {
    let value: Value = serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/house_power_consumers.json",
    ))
    .unwrap();
    assert_eq!(
        value["native_sha256"],
        "1cdd1180e49024fbda8ad568caac2e86e856063ff67ab38f62b7d2c7bb84298c"
    );
    value
}

fn int(value: &Value) -> i32 {
    value.as_i64().expect("native signed scalar") as i32
}

fn flag(input: &Value, key: &str, default: bool) -> bool {
    input[key].as_bool().unwrap_or(default)
}

fn mission(entity: &mut GameEntity, current: i32, queued: i32) {
    entity.mission.apply_test_fixture(MissionTestFixture {
        current: MissionId::from_raw(current),
        suspended: MissionId::NONE,
        queued: MissionId::from_raw(queued),
        movement_bypass_latch: 0,
        handler_state: 0,
        mission_start_frame: 0,
        ai_counter: 0,
        dispatch_timer: MissionDispatchTimer::at_frame(0),
    });
}

fn building(sim: &mut Simulation, id: u64, owner: InternedId, kind: &str, health: i32) {
    let type_ref = sim.interner.intern(kind);
    let mut entity = GameEntity::new_at_frame_zero_for_test(
        id,
        id as u16,
        5,
        0,
        0,
        owner,
        Health { current: health },
        type_ref,
        EntityCategory::Structure,
        0,
        5,
        false,
    );
    entity.estimated_health = EstimatedHealth::from_raw(health);
    entity.lifecycle.object_alive = true;
    entity.lifecycle.in_limbo = false;
    entity.lifecycle.cell_marked = true;
    entity.in_playfield = true;
    entity.building_actually_placed = true;
    entity.finish_building_construction_for_test();
    mission(&mut entity, MissionType::Guard as i32, -1);
    sim.substrate.entities.insert(entity);
    sim.append_house_base_building_for_test(id);
    sim.sample_building_health_for_house_update(id);
}

fn fixture(radar: bool) -> (Simulation, RuleSet, InternedId) {
    // These reproduce the witness's supplied scalars through the production
    // reader. The extra zero-power producer realizes its factory counter1;
    // Begin_Production and producer admission are not compared here.
    let rules = RuleSet::from_ini(&IniFile::from_str(&format!(
        "[General]\nBuildSpeed=.7\nMultipleFactory=.8\nLowPowerPenaltyModifier=1\n\
         MinLowPowerProductionSpeed=.5\nMaxLowPowerProductionSpeed=.8\n\
         RepairStep=8\nRepairPercent=15%\nRepairRate=.016\n\
         SpeakDelayIsInAudioVisual=yes\n[AudioVisual]\nSpeakDelay=2\n\
         ConditionYellow=50%\nConditionRed=25%\n\
         [AI]\nCreditReserve=100\nBuildPower=GAPOWR,GAPOWR,GAPOWR\n\
         [IQ]\nRepairSell=1\nSellBack=2\n[Countries]\n0=Americans\n\
         [Americans]\nSide=GDI\n[VehicleTypes]\n0=PENDING\n\
         [PENDING]\nCost=600\nStrength=100\nTechLevel=1\n\
         Owner=Americans\nBuildTimeMultiplier=1\n\
         [BuildingTypes]\n0=GAPOWR\n1=DRAIN\n2=RADAR\n3=FACTORY\n\
         [GAPOWR]\nStrength=750\nCost=800\nPower=200\n\
         ClickRepairable=yes\nRepairable=yes\nFoundation=2x2\n\
         [DRAIN]\nStrength=500\nPower=-100\n\
         [RADAR]\nStrength=500\nPower=0\nRadar={}\n\
         [FACTORY]\nStrength=500\nPower=0\nFactory=UnitType\nOwner=Americans\n",
        if radar { "yes" } else { "no" },
    )))
    .unwrap();
    let mut sim = Simulation::with_seed(31);
    // The reused native fixture supplies an unseeded zero Main buffer and
    // seeds Scenario31 through the original seeder. Adopt the Main prior
    // through the existing serialized RNG owner, without a new RNG codec.
    sim.main_rng = serde_json::from_value(json!({
        "disabled": 0, "index_a": 0, "index_b": 0, "state": vec![0u32; 250],
    }))
    .unwrap();
    let owner = sim.interner.intern("Americans");
    let mut house = HouseState::new(owner, 0, Some(owner), true, 5000, 10);
    house.current_iq = 1;
    sim.houses.insert(owner, house);
    building(&mut sim, PLANT, owner, "GAPOWR", 374);
    building(&mut sim, DRAIN, owner, "DRAIN", 500);
    building(&mut sim, RADAR, owner, "RADAR", 500);
    building(&mut sim, PRODUCER, owner, "FACTORY", 500);
    (sim, rules, owner)
}

fn assert_rng(sim: &Simulation, native: &Value, boundary: &str) {
    assert_eq!(
        sim.main_rng.native_state_hex(),
        native["main"],
        "{boundary}: Main"
    );
    assert_eq!(
        sim.scenario_rng.native_state_hex(),
        native["scenario"],
        "{boundary}: Scenario"
    );
}

fn assert_power(sim: &Simulation, owner: InternedId, native: &Value, boundary: &str) {
    let state = &sim.power_states[&owner];
    assert_eq!(
        state.total_output,
        int(&native["output"]),
        "{boundary}: output"
    );
    assert_eq!(
        state.total_drain,
        int(&native["drain"]),
        "{boundary}: drain"
    );
    // Read the sole persisted state through its existing serialization seam;
    // the private dirty bits and timer are never externally mutated.
    let serialized = serde_json::to_value(state).unwrap();
    assert_eq!(
        serialized["power_dirty"],
        native["power_dirty"] == 1,
        "{boundary}: power dirty"
    );
    assert_eq!(
        serialized["radar_dirty"],
        native["radar_dirty"] == 1,
        "{boundary}: radar dirty"
    );
    assert_eq!(
        int(&serialized["blackout_timer"]["start_frame"]),
        int(&native["blackout_timer"][0]),
        "{boundary}: blackout start"
    );
    assert_eq!(
        int(&serialized["blackout_timer"]["duration"]),
        int(&native["blackout_timer"][2]),
        "{boundary}: blackout duration"
    );
}

fn assert_joined(sim: &Simulation, rules: &RuleSet, owner: InternedId, native: &Value) {
    let boundary = format!("{}@{}", native["phase"], native["frame"]);
    let plant = sim.substrate.entities.get(PLANT).unwrap();
    assert_eq!(plant.health.current, int(&native["health"]), "{boundary}");
    assert_eq!(
        plant.building_power_health_sample(),
        Some(int(&native["sampled_health"])),
        "{boundary}"
    );
    assert_power(sim, owner, native, &boundary);
    assert_eq!(
        crate::sim::radar::has_radar_for_owner(sim, rules, "Americans"),
        native["radar_available"] == 1,
        "{boundary}: local derived radar projection"
    );
    assert_eq!(
        sim.houses[&owner].eva_low_power_guard,
        native["eva_guard"] == 1,
        "{boundary}: local EVA guard"
    );
    let factory = sim.production.factory_shadow.iter_insertion_ordered()[0];
    let expected = &native["factory"];
    assert_eq!(
        i32::from(factory.progress),
        int(&expected["stage"]),
        "{boundary}: stage"
    );
    assert_eq!(
        i32::from(factory.step_rate_frames),
        int(&expected["rate"]),
        "{boundary}: rate"
    );
    assert_eq!(
        factory.step_timer.start_frame(),
        int(&expected["timer_start"]),
        "{boundary}: timer start"
    );
    assert_eq!(
        factory.step_timer.duration(),
        int(&expected["timer_duration"]),
        "{boundary}: armed duration"
    );
    assert_eq!(
        factory.balance,
        int(&expected["balance"]),
        "{boundary}: balance"
    );
    assert_eq!(
        factory.on_hold,
        expected["on_hold"] == true,
        "{boundary}: cash hold"
    );
    assert_eq!(
        factory.suspended,
        expected["suspended"] == true,
        "{boundary}: suspended"
    );
    assert_eq!(
        sim.houses[&owner].economy.credits,
        int(&expected["credits"]),
        "{boundary}: credits"
    );
    assert_eq!(
        sim.houses[&owner].economy.spent_credits,
        int(&expected["spent"]),
        "{boundary}: spending"
    );
}

#[test]
fn settled_repair_native_health_sample_factory_cadence_radar_and_later_advice() {
    let corpus = corpus();
    assert_eq!(corpus["repair_timelines"].as_array().unwrap().len(), 2);
    let mut compared = 0;
    for row in corpus["repair_timelines"].as_array().unwrap() {
        assert_eq!(
            row["start_accepted"], false,
            "the supplied Factory was already live"
        );
        let (mut sim, rules, owner) = fixture(true);
        let prior = &row["trace"][1];
        sim.session.binary_frame = int(&prior["frame"]) as u32;
        let timer = &prior["blackout_timer"];
        sim.power_states
            .get_mut(&owner)
            .unwrap()
            .start_blackout(int(&timer[0]) as u32, int(&timer[2]) as u32);
        sim.assess_house_derived_state(owner, &rules);
        let type_ref = sim.interner.intern("PENDING");
        assert!(sim.production.factory_shadow.test_enqueue_kernel(
            owner,
            ProductionCategory::Vehicle,
            type_ref,
            1,
            600
        ));
        // Exact native-produced frame195 prior, not a Rust-derived start.
        let pending = &prior["factory"];
        let factory = sim.production.factory_shadow.test_first_mut().unwrap();
        factory.progress = int(&pending["stage"]) as u16;
        factory.step_rate_frames = int(&pending["rate"]) as u16;
        factory.step_timer = CdTimer::from_raw(
            int(&pending["timer_start"]),
            int(&pending["timer_duration"]),
        );
        factory.balance = int(&pending["balance"]);
        factory.on_hold = pending["on_hold"] == true;
        factory.suspended = pending["suspended"] == true;
        sim.houses.get_mut(&owner).unwrap().economy.credits = int(&pending["credits"]);
        sim.houses.get_mut(&owner).unwrap().economy.spent_credits = int(&pending["spent"]);
        sim.substrate.entities.get_mut(PLANT).unwrap().repairing = true;
        assert_joined(&sim, &rules, owner, prior);
        assert_rng(&sim, &row["receipt"]["rng_before"], "joined prior");
        compared += 1;

        for native in row["trace"].as_array().unwrap().iter().skip(2) {
            sim.session.binary_frame = int(&native["frame"]) as u32;
            match native["phase"].as_str().unwrap() {
                "plant_sample" => sim.sample_building_health_for_house_update(PLANT),
                "paid_repair" => {
                    if row["prior_power_dirty_at_repair"] == true {
                        sim.invalidate_house_power(owner, false);
                    }
                    production::update_repair_and_power(&mut sim, &rules, PLANT, None);
                }
                "actual_global_factory_then_house_prefix" => {
                    production::revalidate_and_step_factories(&mut sim, &rules);
                    sim.assess_house_derived_state(owner, &rules);
                }
                "later_local_house_advice" => house_eva::update_house_eva(&mut sim, &rules, owner),
                phase => panic!("unmapped native boundary {phase}"),
            }
            assert_joined(&sim, &rules, owner, native);
            assert_rng(
                &sim,
                &row["receipt"]["rng_after"],
                "joined no-draw boundary",
            );
            compared += 1;
        }
    }
    assert_eq!(compared, 106);
}

#[test]
fn native_blackout_setter_replaces_shorter_longer_and_zero_durations() {
    let corpus = corpus();
    let rows = corpus["blackout_setter_controls"].as_array().unwrap();
    assert_eq!(rows.len(), 4);
    for row in rows {
        let (mut sim, rules, owner) = fixture(true);
        let input = &row["input"];
        let prior = &input["prior_timer"];
        let start = int(&prior[0]) as u32;
        sim.session.binary_frame = start;
        sim.power_states
            .get_mut(&owner)
            .unwrap()
            .start_blackout(start, int(&prior[2]) as u32);
        // Establish the supplied settled prior through the existing House
        // receiver. The full50BC90 comparison itself executes only the
        // setter; ForceShield/Spy activation and numeric inputs are outside.
        sim.assess_house_derived_state(owner, &rules);
        if input["prior_dirty"] == true {
            sim.invalidate_house_power(owner, input["prior_radar_dirty"] == true);
        }
        sim.session.binary_frame = int(&input["frame"]) as u32;
        assert_power(&sim, owner, &row["before"], "native setter prior");
        assert_rng(&sim, &row["receipt"]["rng_before"], "setter prior");
        sim.power_states
            .get_mut(&owner)
            .unwrap()
            .start_blackout(sim.session.binary_frame, int(&input["duration"]) as u32);
        assert_power(&sim, owner, &row["output"], "whole50BC90 setter");
        assert_rng(&sim, &row["receipt"]["rng_after"], "whole50BC90 setter");
        assert_eq!(row["setter_execution"]["unique_instructions"], 13);
        // Original50BCB0 copies entrySP-8 residue to unread timer+2A8.
        // It is preserved in native evidence, not a new Rust state owner.
        assert_eq!(input["stack_opaque"], row["output"]["blackout_timer"][1]);
    }
}

#[test]
fn native_blackout_zero_one_and_two_remaining_with_clean_or_dirty_house() {
    let corpus = corpus();
    let rows = corpus["blackout_controls"].as_array().unwrap();
    assert_eq!(rows.len(), 6);
    for row in rows {
        let (mut sim, rules, owner) = fixture(true);
        let input = &row["input"];
        let start = int(&input["start"]) as u32;
        sim.session.binary_frame = start;
        sim.power_states
            .get_mut(&owner)
            .unwrap()
            .start_blackout(start, int(&input["duration"]) as u32);
        // Reach a clean power assessment while the timer still has2 frames.
        // Local radar's separately supplied prior bool is not compared here.
        sim.assess_house_derived_state(owner, &rules);
        sim.session.binary_frame = int(&input["frame"]) as u32;
        if input["prior_dirty"] == true {
            sim.invalidate_house_power(owner, false);
        }
        assert_power(&sim, owner, &row["before"], "blackout prior");
        sim.assess_house_derived_state(owner, &rules);
        assert_power(&sim, owner, &row["output"], "native blackout receiver");
        assert_rng(&sim, &row["receipt"]["rng_after"], "blackout receiver");
    }
}

#[test]
fn native_local_radar_projection_uses_represented_provider_predicates_and_house_order() {
    let corpus = corpus();
    assert_eq!(corpus["radar_controls"].as_array().unwrap().len(), 16);
    let mut compared = 0;
    for row in corpus["radar_controls"].as_array().unwrap() {
        let input = &row["input"];
        // EMP and independent online/warp are not represented. Native
        // nonlocal output deliberately stays local.
        if !flag(input, "local", true)
            || !flag(input, "online", true)
            || flag(input, "warped", false)
            || input["emp"].as_i64().unwrap_or(0) != 0
        {
            continue;
        }
        let (mut sim, rules, owner) = fixture(flag(input, "radar_type", true));
        sim.session.free_radar = flag(input, "free", false);
        if flag(input, "second", false) {
            building(&mut sim, SECOND_RADAR, owner, "RADAR", 500);
            assert_eq!(
                sim.houses[&owner].base_projection.buildings(),
                &[PLANT, DRAIN, RADAR, PRODUCER, SECOND_RADAR]
            );
            assert_eq!(
                sim.substrate.entities.keys_sorted(),
                vec![PLANT, DRAIN, SECOND_RADAR, RADAR, PRODUCER]
            );
        }
        let provider = sim.substrate.entities.get_mut(RADAR).unwrap();
        mission(
            provider,
            input["mission"].as_i64().unwrap_or(5) as i32,
            input["queued"].as_i64().unwrap_or(-1) as i32,
        );
        provider.lifecycle.cell_marked = flag(input, "marked", true);
        provider.lifecycle.in_limbo = flag(input, "limbo", false);
        let power = input["power"].as_array();
        let mut state = PowerState::default();
        state.total_output = power.map_or(101, |v| int(&v[0]));
        state.total_drain = power.map_or(100, |v| int(&v[1]));
        state.is_low_power = state.total_output < state.total_drain;
        // House+2B0's timer (start, an unused dword, duration) at the row's
        // frame.
        sim.session.binary_frame = int(&row["output"]["frame"]) as u32;
        if let Some(outage) = input["outage"].as_array() {
            state.set_radar_outage_for_test(crate::sim::timer::CdTimer::from_raw(
                int(&outage[0]),
                int(&outage[2]),
            ));
        }
        let order = sim.houses[&owner].base_projection.buildings();
        power_system::assess_house_radar_projection(
            &mut state,
            &sim.substrate.entities,
            order,
            &rules,
            owner,
            &sim.interner,
            sim.session.free_radar,
            sim.session.binary_frame,
        );
        sim.power_states.insert(owner, state);
        assert_eq!(
            crate::sim::radar::has_radar_for_owner(&sim, &rules, "Americans"),
            row["output"]["radar_available"] == 1,
            "{input}"
        );
        assert_eq!(
            serde_json::to_value(&sim.power_states[&owner]).unwrap()["radar_dirty"],
            row["output"]["radar_dirty"] == 1,
            "{input}"
        );
        assert_rng(&sim, &row["receipt"]["rng_after"], "radar receiver");
        compared += 1;
    }
    assert_eq!(compared, 10);
}

#[test]
fn native_local_low_power_advice_guard_and_eight_speed_delay_values() {
    let corpus = corpus();
    assert_eq!(corpus["advice_controls"].as_array().unwrap().len(), 13);
    let mut compared = 0;
    for row in corpus["advice_controls"].as_array().unwrap() {
        let input = &row["input"];
        if !flag(input, "local", true) {
            continue;
        }
        let (mut sim, rules, owner) = fixture(true);
        sim.session.binary_frame = 196;
        sim.session.game_options.game_speed = input["speed"].as_i64().unwrap_or(1) as i32;
        sim.houses.get_mut(&owner).unwrap().eva_low_power_guard = int(&input["guard"]) != 0;
        if input["count"] == 0 {
            // Supply absence of the counted BuildPower type, independently
            // of the retained House power totals. No removal path is claimed.
            sim.substrate.entities.remove(PLANT);
        }
        let mut state = PowerState::default();
        state.total_output = int(&input["power"][0]);
        state.total_drain = int(&input["power"][1]);
        state.is_low_power = state.total_output < state.total_drain;
        sim.power_states.insert(owner, state);
        house_eva::update_house_eva(&mut sim, &rules, owner);
        assert_eq!(
            sim.houses[&owner].eva_low_power_guard,
            row["output"]["eva_guard"] == 1,
            "{input}"
        );
        let lines: Vec<_> = sim
            .sound_events
            .iter()
            .filter_map(|event| match event {
                SimSoundEvent::HouseEva {
                    owner: speaker,
                    event,
                } if *speaker == owner => Some(*event),
                _ => None,
            })
            .collect();
        let native_lines: Vec<_> = row["events"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|event| event[0] == "eva")
            .map(|event| event[1].as_str().unwrap())
            .collect();
        assert_eq!(lines, native_lines, "{input}: local EVA requests");
        if native_lines.contains(&EVA_LOW_POWER) {
            assert_eq!(
                house_eva::speak_delay_frames(&rules, &sim.session.game_options),
                int(&row["output"]["low_power_timer"][2]),
                "{input}: original advice's delay input"
            );
        }
        // The native57BC write has no represented reader; no new timer is
        // introduced merely to echo the corpus's observed write.
        assert_rng(&sim, &row["receipt"]["rng_after"], "later advice");
        compared += 1;
    }
    assert_eq!(compared, 12);
}
