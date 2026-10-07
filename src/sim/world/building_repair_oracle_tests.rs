//! Replay of `tools/spatial_oracle/building_repair.json` against the repair's
//! Rust owner (`production::production_repair`):
//! - `cost` rows: `production::repair_step_cost` over the row's type keys
//!   (Cost, Strength, FreeUnit, the PadAircraft pair and its Dock=) and the
//!   `[General]` RepairStep=, RepairPercent= and SeparateAircraft=, read
//!   through the production reader;
//! - `toggle` rows: [`production::toggle_repair`] over the controls VERA
//!   issues (-1, 0 and 1): the repair byte, and for a row whose owner is the
//!   local player the EVA and the sound at the building's Location (the app
//!   applies `IsHumanPlayer`);
//! - `update` rows: [`production::update_repair_and_power`] on a building of
//!   the row's house: health and its estimate, the repair byte, the retained
//!   damage state, the smoke's done byte, the owner's balance and spending
//!   statistic, the auto-repair latch and its timer, the Scenario RNG cursors,
//!   the sounds and Sell_Back;
//! - `build` rows: the same, frame by frame through a build-up, with
//!   `Simulation::tick_building_up` after it;
//! - `release` rows: `HouseState::release_repair_latch`.
//!
//! Not compared: the wrench byte (`+0x6DE`, read only by the Building CRC),
//! ToggleRepair's flash and the redraw byte (presentation), and the
//! damage-state slot anims, which a building without art does not own: the
//! retained flag they follow is compared (`building_art_transition` replays
//! the anims), and the rows' anims change with it.

use crate::map::entities::EntityCategory;
use crate::rules::ini_parser::IniFile;
use crate::rules::ruleset::RuleSet;
use crate::sim::components::{BuildingUp, Health};
use crate::sim::estimated_health::EstimatedHealth;
use crate::sim::game_entity::GameEntity;
use crate::sim::house_state::{HouseDifficulty, HouseState};
use crate::sim::mission::state::MissionTestFixture;
use crate::sim::mission::{MissionDispatchTimer, MissionId, MissionType};
use crate::sim::production::{self, RepairControl};
use crate::sim::rng::SimRng;
use crate::sim::timer::CdTimer;
use crate::sim::world::{SimSoundEvent, Simulation};
use serde_json::{Value, json};

/// The oracle's sound indices: `[AudioVisual] GenericClick=` and
/// `ScoldSound=`.
const SOUNDS: [(i64, &str); 2] = [(42, "OracleClick"), (43, "OracleScold")];

fn corpus() -> Value {
    serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/building_repair.json",
    ))
    .unwrap()
}

fn constant(corpus: &Value, name: &str) -> u64 {
    u64::from_str_radix(corpus["constants"][name].as_str().unwrap(), 16).unwrap()
}

fn yes_no(value: bool) -> &'static str {
    if value { "yes" } else { "no" }
}

/// The building's sound events as the oracle records them: `EVA_Repairing`
/// and each `VocClass::PlayAt` with its sound index, coordinate and handle.
fn sounds(sim: &Simulation, owner: crate::sim::intern::InternedId) -> Vec<Value> {
    sim.sound_events
        .iter()
        .filter_map(|event| match event {
            SimSoundEvent::Repairing { owner: speaker } => {
                assert_eq!(*speaker, owner);
                Some(json!(["eva", "EVA_Repairing", -1, -1]))
            }
            SimSoundEvent::VocAt {
                sound_id,
                audible_to,
                rx,
                ry,
                sub_x,
                sub_y,
                world_z_leptons,
            } => {
                assert_eq!(*audible_to, Some([owner, owner]), "the owner's player");
                let index = SOUNDS
                    .iter()
                    .find(|(_, name)| name == sound_id)
                    .map(|(index, _)| *index)
                    .unwrap_or_else(|| panic!("sound {sound_id}"));
                Some(json!([
                    "play_at",
                    index,
                    [
                        i32::from(*rx) * 256 + sub_x.to_num::<i32>(),
                        i32::from(*ry) * 256 + sub_y.to_num::<i32>(),
                        world_z_leptons
                    ],
                    0
                ]))
            }
            _ => None,
        })
        .collect()
}

/// The oracle's EVA and PlayAt calls, in order.
fn native_sounds(row: &Value) -> Vec<Value> {
    row["events"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|event| event[0] == "eva" || event[0] == "play_at")
        .cloned()
        .collect()
}

/// Whether `HouseClass::IsHumanPlayer @ 0x0050B6F0` admits the row's owner:
/// PlayerPtr in a multiplayer game, `+0x1EC` or `+0x1ED` in a campaign.
fn local_player(input: &Value) -> bool {
    if input["game_mode"].as_i64().unwrap_or(1) != 0 {
        input["player"] == true
    } else {
        input["human"] == true || input["player_control"] == true
    }
}

/// `BuildingTypeClass` repair step cost (`0x007120D0`) over the row's keys.
#[test]
fn the_repair_step_cost_matches_the_original() {
    let corpus = corpus();
    let percent_15 = constant(&corpus, "percent_15");
    let percent_25 = constant(&corpus, "percent_25");
    let mut compared = 0;
    for row in corpus["cost"].as_array().unwrap() {
        let input = &row["input"];
        let name = input["name"].as_str().unwrap();
        let int = |key: &str, default: i64| input[key].as_i64().unwrap_or(default);
        let percent = input["percent"].as_u64().unwrap_or(percent_15);
        let pads: Vec<i64> = input["pads"].as_array().map_or(vec![1000, 1200], |pads| {
            pads.iter().map(|cost| cost.as_i64().unwrap()).collect()
        });
        let mut text = format!(
            "[General]\nRepairStep={}\n{}PadAircraft=PADA,PADB\nSeparateAircraft={}\n\
             [InfantryTypes]\n[VehicleTypes]\n0=FREE\n[AircraftTypes]\n0=PADA\n1=PADB\n\
             [BuildingTypes]\n0=YAREFN\n1=OTHER\n\
             [PADA]\nCost={}\nDock={}\n[PADB]\nCost={}\n[FREE]\nCost={}\n[OTHER]\nStrength=100\n\
             [YAREFN]\nStrength={}\nCost={}\n",
            int("step", 8),
            if percent == percent_15 {
                "RepairPercent=15%\n"
            } else {
                assert_eq!(percent, percent_25, "{name}: the constructor's .25");
                ""
            },
            yes_no(input["separate_aircraft"] != false),
            pads[0],
            if input["pad_dock"] == true {
                "YAREFN"
            } else {
                "OTHER"
            },
            pads[1],
            int("free_unit", 0),
            int("strength", 1000),
            int("cost", 2500),
        );
        if input["free_unit"].is_number() {
            text.push_str("FreeUnit=FREE\n");
        }
        let rules = RuleSet::from_ini(&IniFile::from_str(&text)).unwrap();
        assert_eq!(rules.general.repair_percent.to_bits(), percent, "{name}");
        assert_eq!(
            i64::from(production::repair_step_cost(
                &rules,
                rules.object("YAREFN").unwrap()
            )),
            row["cost"].as_i64().unwrap(),
            "{name}"
        );
        compared += 1;
    }
    assert_eq!(compared, 25);
}

/// `BuildingClass::ToggleRepair @ 0x00446FF0` over the controls VERA issues.
#[test]
fn toggle_repair_matches_the_original() {
    let rules = RuleSet::from_ini(&IniFile::from_str(&format!(
        // VERA's rules reader reads `[AudioVisual]` sounds only with a
        // non-empty `[General]` section.
        "[General]\nFixtureOnly=1\n[AudioVisual]\nGenericClick={}\nScoldSound={}\n[InfantryTypes]\n\
         [VehicleTypes]\n[AircraftTypes]\n[BuildingTypes]\n0=YAREFN\n[YAREFN]\nStrength=1000\n",
        SOUNDS[0].1, SOUNDS[1].1
    )))
    .unwrap();
    let mut compared = 0;
    for row in corpus()["toggle"].as_array().unwrap() {
        let input = &row["input"];
        let name = input["name"].as_str().unwrap();
        let control = match input["control"].as_i64().unwrap() {
            -1 => RepairControl::Toggle,
            0 => RepairControl::Stop,
            1 => RepairControl::Start,
            // No caller passes another control (it keeps the byte and still
            // sounds, `0x00447007`).
            _ => continue,
        };
        let mut sim = Simulation::new();
        sim.session.game_mode_nonzero = input["game_mode"].as_i64().unwrap_or(1) != 0;
        let owner = sim.interner.intern("AI");
        let mut house = HouseState::new(owner, 0, None, input["human"] == true, 0, 10);
        house.player_control = input["player_control"] == true;
        sim.houses.insert(owner, house);
        let kind = sim.interner.intern("YAREFN");
        let mut building = GameEntity::new_at_frame_zero_for_test(
            1,
            12,
            12,
            0,
            0,
            owner,
            Health {
                current: input["health"].as_i64().unwrap() as i32,
            },
            kind,
            EntityCategory::Structure,
            0,
            5,
            false,
        );
        building.repairing = input["repairing"] == 1;
        sim.substrate.entities.insert(building);
        assert!(production::toggle_repair(&mut sim, &rules, 1, control));
        assert_eq!(
            u64::from(sim.substrate.entities.get(1).unwrap().repairing),
            row["repairing"].as_u64().unwrap(),
            "{name}: +0x6E8"
        );
        let native = native_sounds(row);
        if local_player(input) {
            assert_eq!(sounds(&sim, owner), native, "{name}: sounds");
        } else {
            assert!(native.is_empty(), "{name}: not the local player");
        }
        compared += 1;
    }
    assert_eq!(compared, 28);
}

/// The row's scene: the row's rules, its computer (or human) house, and the
/// refinery YAREFN with the row's health, estimate, mission, repair and AI
/// repair bytes, capture, retained damage state and smoke, with the Scenario
/// RNG seeded last. Current/queued Construction is represented by the same
/// MissionCom fields that native Get_Mission reads.
fn update_scene(corpus: &Value, input: &Value) -> (Simulation, RuleSet, Option<u64>) {
    let int = |key: &str, default: i64| input[key].as_i64().unwrap_or(default) as i32;
    let flag = |key: &str, default: bool| input[key].as_bool().unwrap_or(default);
    let rules = RuleSet::from_ini(&IniFile::from_str(&format!(
        "[General]\nRepairStep=8\nRepairPercent=15%\nRepairRate=.016\n[AI]\nCreditReserve={}\n\
         [IQ]\nRepairSell=1\nSellBack=2\n[AudioVisual]\nConditionYellow=50%\nConditionRed=25%\n\
         GenericClick={}\nScoldSound={}\n[InfantryTypes]\n[VehicleTypes]\n[AircraftTypes]\n\
         [BuildingTypes]\n0=YAREFN\n[YAREFN]\nStrength=1000\nCost={}\nClickRepairable={}\n\
         Repairable=yes\nHasStupidGuardMode=no\nFoundation=2x2\n[Particles]\n0=Smk\n[ParticleSystems]\n0=Sys\n\
         [Smk]\nBehavesLike=Smoke\nMaxEC=10\nMaxDC=4\nStartStateAI=0\nEndStateAI=10\n\
         StateAIAdvance=4\n[Sys]\nBehavesLike=Smoke\nHoldsWhat=Smk\nParticleCap=10\n\
         SpawnFrames=1\nLifetime=200\n",
        int("credit_reserve", 100),
        SOUNDS[0].1,
        SOUNDS[1].1,
        int("cost", 2500),
        yes_no(flag("click_repairable", true)),
    )))
    .unwrap();
    assert_eq!(
        rules.general.repair_percent.to_bits(),
        constant(corpus, "percent_15")
    );
    assert_eq!(
        rules.general.repair_rate_minutes.to_bits(),
        constant(corpus, "rate_016")
    );
    let mut sim = Simulation::new();
    sim.session.game_mode_nonzero = int("game_mode", 1) != 0;
    sim.session.binary_frame = int("frame", 196) as u32;
    let owner = sim.interner.intern("AI");
    let mut house = HouseState::new(
        owner,
        0,
        None,
        flag("human", false),
        int("balance", 5000),
        10,
    );
    house.player_control = flag("player_control", false);
    house.current_iq = int("current_iq", 2);
    house.authored_iq = 2;
    house.repair_start_latch = flag("latched", false);
    let timer = input["timer"].as_array().map_or([0, 0], |timer| {
        [timer[0].as_i64().unwrap(), timer[2].as_i64().unwrap()]
    });
    house.repair_latch_timer = CdTimer::from_raw(timer[0] as i32, timer[1] as i32);
    house.repair_delay = f64::from_bits(
        input["delay"]
            .as_u64()
            .unwrap_or_else(|| constant(corpus, "delay_02")),
    );
    sim.houses.insert(owner, house);
    assert_eq!(sim.allocate_stable_id(), 1);
    let kind = sim.interner.intern("YAREFN");
    let health = int("health", 300);
    let mut building = GameEntity::new_at_frame_zero_for_test(
        1,
        12,
        12,
        0,
        0,
        owner,
        Health { current: health },
        kind,
        EntityCategory::Structure,
        0,
        5,
        false,
    );
    building.lifecycle.in_limbo = false;
    building.in_playfield = true;
    building.estimated_health = EstimatedHealth::from_raw(int("estimate", health.into()));
    building.repairing = flag("repairing", false);
    building.has_been_captured = flag("captured", false);
    building.ai_repairable = flag("ai_repairable", true);
    building.was_attacked_by_enemy = flag("attacked", false);
    building.building_damage_state_active = flag("damaged", health * 2 <= 1000);
    let mission = |key: &str, default: &str| match input[key].as_str().unwrap_or(default) {
        "guard" => MissionId::from_known(MissionType::Guard),
        "selling" => MissionId::from_known(MissionType::Selling),
        "construction" => MissionId::from_known(MissionType::Construction),
        "none" => MissionId::NONE,
        other => panic!("mission {other}"),
    };
    building.mission.apply_test_fixture(MissionTestFixture {
        current: mission("mission", "guard"),
        suspended: MissionId::NONE,
        queued: mission("queue", "none"),
        movement_bypass_latch: 0,
        handler_state: 0,
        mission_start_frame: 0,
        ai_counter: 0,
        dispatch_timer: MissionDispatchTimer::at_frame(0),
    });
    sim.substrate.entities.insert(building);
    sim.add_entity_occupancy(1);
    let smoke = flag("smoke", true).then(|| {
        let smoke = sim
            .spawn_particle_system(
                crate::rules::particle_system_type::ParticleSystemTypeId(0),
                glam::IVec3::ZERO,
                Some(1),
                Some(1),
                glam::IVec3::ZERO,
                None,
                &rules,
            )
            .unwrap();
        sim.substrate
            .entities
            .get_mut(1)
            .unwrap()
            .damage_smoke_system_id = Some(smoke);
        smoke
    });
    sim.scenario_rng = SimRng::new(input["seed"].as_u64().unwrap_or(1));
    sim.sound_events.clear();
    (sim, rules, smoke)
}

/// The Scenario RNG's cursors as the oracle records them.
fn cursors(sim: &Simulation) -> Value {
    let view = sim.scenario_rng.logical_view();
    json!([view.index_a, view.index_b])
}

/// `BuildingClass::UpdateRepairAndPower @ 0x00450630` from its entry.
#[test]
fn update_repair_and_power_matches_the_original() {
    let corpus = corpus();
    let mut compared = 0;
    for row in corpus["update"].as_array().unwrap() {
        let input = &row["input"];
        let name = input["name"].as_str().unwrap();
        let (mut sim, rules, smoke) = update_scene(&corpus, input);
        assert_eq!(
            cursors(&sim),
            row["random_indices"]["before"],
            "{name}: seeded"
        );
        let damaged_before = sim
            .substrate
            .entities
            .get(1)
            .unwrap()
            .building_damage_state_active;
        production::update_repair_and_power(&mut sim, &rules, 1, None);
        let owner = sim.interner.get("AI").unwrap();
        let building = sim.substrate.entities.get(1).unwrap();
        let house = &sim.houses[&owner];
        assert_eq!(
            json!([
                building.health.current,
                building.estimated_health.get(),
                u8::from(building.repairing),
                u8::from(building.building_damage_state_active),
            ]),
            json!([
                row["health"],
                row["estimate"],
                row["repairing"],
                row["damaged"]
            ]),
            "{name}: health, estimate, +0x6E8, +0x6E6"
        );
        assert_eq!(
            json!([
                house.economy.credits,
                house.economy.spent_credits,
                u8::from(house.repair_start_latch),
                [
                    house.repair_latch_timer.start_frame(),
                    house.repair_latch_timer.duration()
                ],
            ]),
            json!([row["balance"], row["spent"], row["latched"], row["timer"]]),
            "{name}: balance, spending, latch, timer"
        );
        assert_eq!(
            cursors(&sim),
            row["random_indices"]["after"],
            "{name}: draws"
        );
        if let Some(smoke) = smoke {
            assert_eq!(
                u8::from(sim.particle_systems().get(smoke).unwrap().done_spawning),
                row["smoke_done"].as_u64().unwrap() as u8,
                "{name}: smoke"
            );
        }
        let native_anims = row["events"]
            .as_array()
            .unwrap()
            .iter()
            .any(|event| event[0] == "slot_anim");
        assert_eq!(
            native_anims,
            damaged_before != building.building_damage_state_active,
            "{name}: the slot anims follow the flag"
        );
        let selling = building.mission.effective().known() == Some(MissionType::Selling)
            && input["mission"] != "selling";
        assert_eq!(
            selling,
            row["events"]
                .as_array()
                .unwrap()
                .iter()
                .any(|event| event[0] == "sell_back"),
            "{name}: Sell_Back"
        );
        let native = native_sounds(row);
        if local_player(input) {
            assert_eq!(sounds(&sim, owner), native, "{name}: sounds");
        } else {
            assert!(native.is_empty(), "{name}: not the local player");
        }
        compared += 1;
    }
    assert_eq!(compared, 47);
}

/// The `build` rows: a damaged building's build-up on each route, frame by
/// frame through the ordinary object visit: body and Construction precede
/// [`production::update_repair_and_power`]. VERA completes on the frame native calls
/// Grand_Opening, and the repair starts on that frame, not before (native
/// Get_Mission reads Guard there): after each frame the health, repair byte,
/// balance, latch and its timer, the draws and the local player's sounds.
#[test]
fn a_build_up_holds_the_repair_until_its_completion_frame() {
    let corpus = corpus();
    let mut compared = 0;
    for row in corpus["build"].as_array().unwrap() {
        let input = &row["input"];
        let name = input["name"].as_str().unwrap();
        let (mut sim, rules, _smoke) = update_scene(&corpus, input);
        // Original build() calls prepare_update with mission=none before entry.
        sim.mission_assign_exact(1, MissionId::NONE, sim.session.binary_frame)
            .unwrap();
        let owner = sim.interner.get("AI").unwrap();
        let control: [i32; 3] = serde_json::from_value(input["control"].clone()).unwrap();
        let start = input["frame"].as_i64().unwrap() as i32;
        sim.substrate
            .entities
            .get_mut(1)
            .unwrap()
            .install_building_up(
                match input["route"].as_str().unwrap() {
                    "deploy" => BuildingUp::deployed(control, start),
                    "computer" => BuildingUp::placed_by_computer(control, start),
                    "player" => BuildingUp::placed_by_player(control, start),
                    other => panic!("route {other}"),
                },
                start,
            );
        if input["route"] == "player" {
            // The original inherited route supplies the successful yard BREAK.
            sim.substrate
                .entities
                .get_mut(1)
                .unwrap()
                .begin_building_body(
                    crate::sim::building_construction::BuildingBodyMode::Idle,
                    start,
                );
        }
        for frame in row["frames"].as_array().unwrap() {
            let now = frame["frame"].as_u64().unwrap();
            sim.session.binary_frame = now as u32;
            sim.sound_events.clear();
            let placed_before = sim
                .substrate
                .entities
                .get(1)
                .unwrap()
                .building_actually_placed;
            sim.object_ai_visit_one(1, Some(&rules), super::techno_ai::ObjectAiCtx::default());
            let after = cursors(&sim);
            let completed = !placed_before
                && sim
                    .substrate
                    .entities
                    .get(1)
                    .unwrap()
                    .building_actually_placed;
            assert_eq!(
                completed, frame["grand_opening"],
                "{name} {now}: the build-up completes"
            );
            let building = sim.substrate.entities.get(1).unwrap();
            let house = &sim.houses[&owner];
            assert_eq!(
                json!([
                    building.health.current,
                    u8::from(building.repairing),
                    house.economy.credits,
                    u8::from(house.repair_start_latch),
                    [
                        house.repair_latch_timer.start_frame(),
                        house.repair_latch_timer.duration()
                    ],
                ]),
                json!([
                    frame["health"],
                    frame["repairing"],
                    frame["balance"],
                    frame["latched"],
                    frame["timer"]
                ]),
                "{name} {now}: health, +0x6E8, balance, latch, timer"
            );
            // The Guard mission's draws the fixture runs after completion
            // precede later frames' visits; the start's draw is the row's
            // first.
            // Native's recorded before cursor is after the mission pieces.
            // The production visit includes those pieces and repair, so its
            // final cursor is the matching boundary, including Guard draws.
            assert_eq!(after, frame["random_indices"]["after"], "{name} {now}");
            let native = native_sounds(frame);
            if local_player(input) {
                assert_eq!(sounds(&sim, owner), native, "{name} {now}: sounds");
            } else {
                assert!(native.is_empty(), "{name} {now}: not the local player");
            }
        }
        compared += 1;
    }
    assert_eq!(compared, 5);
}

/// `HouseClass::Update`'s latch release (`0x004F9302..0x004F9338`).
#[test]
fn the_auto_repair_latch_releases_on_the_original_timer() {
    let mut compared = 0;
    for row in corpus()["release"].as_array().unwrap() {
        let input = &row["input"];
        let mut house = HouseState::new(Default::default(), 0, None, false, 0, 10);
        house.repair_start_latch = input["latched"] == 1;
        house.repair_latch_timer = CdTimer::from_raw(
            input["timer"][0].as_i64().unwrap() as i32,
            input["timer"][1].as_i64().unwrap() as i32,
        );
        house.release_repair_latch(input["frame"].as_u64().unwrap() as u32);
        assert_eq!(
            u8::from(house.repair_start_latch),
            row["latched"].as_u64().unwrap() as u8,
            "{}",
            input["name"]
        );
        compared += 1;
    }
    assert_eq!(compared, 20);
}

/// The retail keys through the production reader: `RepairStep=8`,
/// `RepairPercent=15%`, `RepairRate=.016`, the difficulty rows'
/// `RepairDelay=` and `ScoldSound=`/`GenericClick=` as the oracle's rows write
/// them; and ReadDifficulty's default for a row without the key.
#[test]
fn retail_repair_keys_read_as_the_oracle_writes_them() {
    let corpus = corpus();
    let bits = |values: [f64; 3]| values.map(f64::to_bits);
    let without_key = RuleSet::from_ini(&IniFile::from_str(
        "[General]\nFixtureOnly=1\n[Easy]\nROF=1\n[Normal]\n[Difficult]\n",
    ))
    .unwrap();
    assert_eq!(
        bits(without_key.general.difficulty_repair_delay),
        [constant(&corpus, "delay_02_default"); 3]
    );
    assert_eq!(
        without_key.general.repair_percent.to_bits(),
        constant(&corpus, "percent_25")
    );
    assert_eq!(without_key.general.repair_step, 5);
    let Some(ini) = crate::rules::retail_ini_fixture::retail_ini("rulesmd.ini") else {
        return;
    };
    let rules = RuleSet::from_ini(&ini).unwrap();
    assert_eq!(rules.general.repair_step, 8);
    assert_eq!(
        rules.general.repair_percent.to_bits(),
        constant(&corpus, "percent_15")
    );
    assert_eq!(
        rules.general.repair_rate_minutes.to_bits(),
        constant(&corpus, "rate_016")
    );
    assert_eq!(
        bits(rules.general.difficulty_repair_delay),
        [
            constant(&corpus, "delay_02"),
            constant(&corpus, "delay_02"),
            constant(&corpus, "delay_05")
        ]
    );
    assert_eq!(rules.general.scold_sound.as_deref(), Some("MenuScold"));
    assert_eq!(
        rules.general.generic_click_sound.as_deref(),
        Some("MenuClick")
    );
    // SetDifficulty copies the house's row (HouseDifficulty order: 0 is a
    // Hard AI reading [Easy]).
    let mut house = HouseState::new(Default::default(), 0, None, false, 0, 10);
    house.set_difficulty(HouseDifficulty::Easy, &rules.general, 1.0, true, 0, 0);
    assert_eq!(house.repair_delay.to_bits(), constant(&corpus, "delay_05"));
}

/// The repair step's `ADD`s wrap and its clamp compares signed against the
/// type's live Strength, whatever sign it has (`0x004508A8..0x004508D0`).
#[test]
fn the_repair_step_keeps_signed_adds_and_the_live_strength() {
    for (strength, actual, estimate, expected_actual, expected_estimate, repairing) in [
        (100_000, 70_000, -20, 70_004, -16, true),
        (
            i32::MAX,
            i32::MAX - 1,
            i32::MAX - 2,
            i32::MIN + 2,
            i32::MIN + 1,
            true,
        ),
        (-10, -20, -50, -16, -46, true),
        (-10, -12, -50, -10, -10, false),
        (100, 100, -50, 100, 100, false),
    ] {
        let rules = RuleSet::from_ini(&IniFile::from_str(&format!(
            "[General]\nRepairStep=4\n[InfantryTypes]\n[VehicleTypes]\n[AircraftTypes]\n\
             [BuildingTypes]\n0=GAPOWR\n[GAPOWR]\nStrength={strength}\nCost=800\n",
        )))
        .unwrap();
        let mut sim = Simulation::new();
        let owner = sim.interner.intern("Americans");
        sim.houses
            .insert(owner, HouseState::new(owner, 0, None, true, 1000, 10));
        let kind = sim.interner.intern("GAPOWR");
        let mut building = GameEntity::new_at_frame_zero_for_test(
            1,
            10,
            10,
            0,
            0,
            owner,
            Health { current: actual },
            kind,
            EntityCategory::Structure,
            0,
            5,
            false,
        );
        building.estimated_health = EstimatedHealth::from_raw(estimate);
        building.repairing = true;
        sim.substrate.entities.insert(building);
        production::update_repair_and_power(&mut sim, &rules, 1, None);
        let building = sim.substrate.entities.get(1).unwrap();
        assert_eq!(
            (
                building.health.current,
                building.estimated_health.get(),
                building.repairing
            ),
            (expected_actual, expected_estimate, repairing),
            "strength={strength} actual={actual}"
        );
    }
}

/// Each Selling visit stops a repair first (`BuildingClass::Mission_Selling`'s
/// `ToggleRepair(0)`, `0x00449C41`): on the refinery dock scene's second
/// refinery, damaged and paid for, a repair running when its owner's sale
/// order arrives keeps running through the order's frame and stops at the
/// sale's first visit, the frame after, with ToggleRepair's click at the
/// building; the same visit of a building not repairing plays no click.
#[test]
fn a_sale_s_first_visit_stops_the_repair() {
    let overlay = crate::sim::tiberium::test_support::overlay_registry();
    for repairing in [true, false] {
        let mut s = super::refinery_dock_oracle_tests::scene(&json!({
            "mission": "guard",
            "balance": 100_000,
        }));
        s.rules.general.generic_click_sound = Some(SOUNDS[0].1.to_string());
        // The `route` rows' Buildup (`building_sale_oracle_tests`).
        s.rules.set_buildup_control_for_test("GAREFN", [0, 3, 2]);
        let building = s.other;
        let strength = s
            .sim
            .object_type(
                s.sim.substrate.entities.get(building).unwrap().type_ref(),
                &s.rules,
            )
            .unwrap()
            .strength;
        let entity = s.sim.substrate.entities.get_mut(building).unwrap();
        entity.health.current = strength / 2;
        entity.repairing = repairing;
        let sale_order = crate::sim::command::CommandEnvelope::new(
            s.sim.substrate.entities.get(building).unwrap().owner(),
            s.sim.session.tick + 1,
            crate::sim::command::Command::SellBuilding {
                entity_id: building,
            },
        );
        let mut frames = Vec::new();
        for index in 0..2 {
            s.sim.sound_events.clear();
            let grid = s.sim.path_grid_snapshot();
            let commands = if index == 0 {
                std::slice::from_ref(&sale_order)
            } else {
                &[]
            };
            let tick =
                s.sim
                    .advance_tick(commands, Some(&s.rules), grid.as_deref(), Some(overlay), 67);
            if index == 0 {
                assert_eq!(tick.executed_commands, 1);
            }
            let entity = s.sim.substrate.entities.get(building).unwrap();
            let clicks = s
                .sim
                .sound_events
                .iter()
                .filter(|event| {
                    matches!(event, SimSoundEvent::VocAt { sound_id, .. } if sound_id == SOUNDS[0].1)
                })
                .count();
            frames.push((entity.mission.handler_state(), entity.repairing, clicks));
        }
        let (order, visit) = (frames[0], frames[1]);
        assert_eq!(
            order,
            (0, repairing, 0),
            "repairing {repairing}: the order's frame"
        );
        assert_eq!(
            visit,
            (1, false, usize::from(repairing)),
            "repairing {repairing}: the first visit"
        );
    }
}
