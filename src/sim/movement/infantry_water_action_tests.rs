//! Original DoAction51D6F0 water remaps, pre-admission state and sound order.
//! The native corpus uses supplied valid cells/poses and a disabled audio
//! gate. These comparisons stop at class return, before water placement,
//! whole InfantryAI or audible playback. Wet death cleanup uses the existing
//! production sequencer's native20/21 UnInit arm, not a second action port.

use std::sync::{Arc, OnceLock};

use serde_json::{Value, json};

use super::*;
use crate::map::resolved_terrain::{ResolvedTerrainGrid, test_flat_cell};
use crate::rules::ini_parser::IniFile;
use crate::rules::process_owner::NativeRulesProcessOwner;
use crate::rules::sound_ini::SoundRegistry;
use crate::sim::components::DriveCoord;
use crate::sim::movement::ground_pose;
use crate::sim::rng::SimRng;
use crate::sim::stage::StageClass;
use crate::sim::timer::CdTimer;
use crate::sim::world::SimSoundEvent;

fn oracle() -> &'static [Value] {
    static NATIVE: OnceLock<Vec<Value>> = OnceLock::new();
    NATIVE.get_or_init(|| {
        let rows: Vec<Value> = serde_json::from_str(crate::test_fixture::text(
            "tools/spatial_oracle/infantry_water_action.json",
        ))
        .unwrap();
        assert_eq!(rows.len(), 356);
        rows
    })
}

fn signed(value: &Value) -> i32 {
    value.as_i64().expect("native signed dword") as i32
}

fn sections_text(sections: &Value) -> String {
    let mut text = String::new();
    for (section, fields) in sections.as_object().unwrap() {
        text.push_str(&format!("[{section}]\n"));
        for (key, value) in fields.as_object().unwrap() {
            text.push_str(&format!("{key}={}\n", value.as_str().unwrap()));
        }
    }
    text
}

fn sections_ini(sections: &Value) -> IniFile {
    IniFile::from_str(&sections_text(sections))
}

fn reader(name: &str) -> &'static Value {
    oracle()
        .iter()
        .find(|row| row["kind"] == "retail_reader" && row["type_name"] == name)
        .unwrap()
}

/// Native explicitly constructs these two type IDs, then reads the saved
/// physical sections across root/LANG/mode/map with fixed ART/SOUNDMD.
/// Transport those input bytes through the production process owner; the
/// observed record and sound outputs are asserted separately below.
fn physical_rules() -> RuleSet {
    let receipt = reader("GHOST");
    let history = receipt["history"].as_array().unwrap();
    let root = IniFile::from_str(&format!(
        "[InfantryTypes]\n0=GHOST\n1=TANY\n{}",
        sections_text(&history[0]["inputs"])
    ));
    assert_eq!(history[1]["file"], "LANGRULE.INI");
    assert_eq!(history[1]["absent"], true);
    let sounds = Arc::new(SoundRegistry::from_ini(&sections_ini(
        &receipt["selected_sound"],
    )));
    let mut owner = NativeRulesProcessOwner::from_cold_start_sources(
        root,
        None,
        sections_ini(&receipt["art"]),
        sounds,
    )
    .unwrap();
    let (mut rules, _, fixed_art, _) = owner
        .load_scenario(crate::rules::process_owner::NativeScenarioRulesPrefix::NonCampaign(
            Some(&sections_ini(&history[2]["inputs"]))),
            &sections_ini(&history[3]["inputs"]),
        )
        .unwrap()
        .into_parts();
    // The process owner returns fixed ART separately; the normal loader's
    // init2467 installs it, then binds its parsed signed42-record bank.
    rules.install_art_data(crate::rules::art_data::ArtRegistry::from_ini(&fixed_art));
    rules.bind_animation_sequences(
        &crate::rules::infantry_sequence::parse_infantry_sequence_registry(&fixed_art),
    );
    rules
}

/// Every synthetic native action record has start/stride0 and signed count6,
/// except the declared absent record. Read those supplied inputs through the
/// same production ART/type/sound readers used by the physical controls.
fn supplied_rules(input: &Value) -> RuleSet {
    let records: String = crate::rules::infantry_sequence::NATIVE_SEQUENCE_NAMES
        .iter()
        .enumerate()
        .map(|(action, name)| {
            let count = if input["absent"].as_u64() == Some(action as u64) {
                0
            } else {
                6
            };
            format!("{name}=0,{count},0\n")
        })
        .collect();
    let zone = if input["zone"].as_i64() == Some(0) {
        "Normal"
    } else {
        "AmphibiousDestroyer"
    };
    let root = IniFile::from_str(&format!(
        "[InfantryTypes]\n0=WATER\n[WATER]\nStrength=100\nMovementZone={zone}\n\
         Locomotor={{4A582744-9839-11d1-B709-00A024DDAFD1}}\n\
         EnterWaterSound=WaterEnter\nLeaveWaterSound=WaterLeave\n"
    ));
    let art = IniFile::from_str(&format!(
        "[WATER]\nSequence=WaterFixture\nCrawls={}\n[WaterFixture]\n{records}",
        if input["extended"].as_bool().unwrap_or(false) && input["crawls"].as_bool().unwrap_or(true)
        {
            "yes"
        } else {
            "no"
        }
    ));
    let sounds = Arc::new(SoundRegistry::from_ini(&IniFile::from_str(
        "[SoundList]\n0=WaterEnter\n1=WaterLeave\n",
    )));
    let mut owner =
        NativeRulesProcessOwner::from_cold_start_sources(root, None, art, sounds).unwrap();
    let (mut rules, _, fixed_art, _) = owner
        .load_scenario(crate::rules::process_owner::NativeScenarioRulesPrefix::NonCampaign(None), &IniFile::from_str(""))
        .unwrap()
        .into_parts();
    rules.install_art_data(crate::rules::art_data::ArtRegistry::from_ini(&fixed_art));
    rules.bind_animation_sequences(
        &crate::rules::infantry_sequence::parse_infantry_sequence_registry(&fixed_art),
    );
    rules
}

fn fixture(row: &Value, rules: &RuleSet, type_name: &str) -> (Simulation, u64) {
    let input = &row["input"];
    let physical = row["kind"] == "retail_action";
    let mut sim = Simulation::with_seed(0);
    sim.session.binary_frame = 100;
    sim.session.game_options.game_speed = row["game_speed_index"].as_i64().unwrap_or(0) as i32;
    let house = sim.interner.intern("Americans");
    sim.houses.insert(
        house,
        crate::sim::house_state::HouseState::new(house, 0, None, true, 0, 10),
    );
    let xyz = if physical {
        row["physical_xyz"]
            .as_array()
            .unwrap()
            .iter()
            .map(signed)
            .collect::<Vec<_>>()
    } else {
        vec![2688, 2688, 0]
    };
    let cell = ((xyz[0] / 256) as u16, (xyz[1] / 256) as u16);
    let width = cell.0 + 1;
    let height = cell.1 + 1;
    let mut cells = (0..height)
        .flat_map(|y| (0..width).map(move |x| test_flat_cell(x, y)))
        .collect::<Vec<_>>();
    let terrain_cell = &mut cells[usize::from(cell.1) * usize::from(width) + usize::from(cell.0)];
    terrain_cell.yr_cell_land_type = input["land"].as_u64().unwrap_or(2) as u8;
    terrain_cell.base_yr_cell_land_type = terrain_cell.yr_cell_land_type;
    sim.install_resolved_terrain_for_new_map(ResolvedTerrainGrid::from_cells(width, height, cells));
    let id = sim
        .construct_object_limbo_at_height(type_name, "Americans", cell.0, cell.1, 0, 0, rules)
        .expect("water action constructor");
    let actor = sim.substrate.entities.get_mut(id).unwrap();
    assert_eq!(actor.mission_leaf.as_infantry().unwrap().water_state(), 2);
    // Synthetic rows supply +74=false; physical rows retain the constructor
    // in limbo. Neither premise claims wet/deck Unlimbo or occupation.
    actor.lifecycle.cell_marked = false;
    // These action controls supply Location after construction. Constructor
    // arguments do not place the fresh Object at this native cell/pose.
    ground_pose::put_location(
        &mut actor.position,
        DriveCoord {
            x: xyz[0],
            y: xyz[1],
            z: xyz[2],
        },
    );
    actor.on_bridge = input["bridge"].as_bool().unwrap_or(false);
    actor.health.current = row["before"]["health"]
        .as_i64()
        .or_else(|| input["health"].as_i64())
        .unwrap_or(100) as i32;
    actor
        .mission_leaf
        .set_infantry_doing_verified(input["current"].as_i64().unwrap_or(-1) as i32)
        .unwrap();
    actor
        .mission_leaf
        .install_infantry_water_state_fixture(input["old_water_state"].as_i64().unwrap_or(1) as i32);
    let infantry = actor.infantry.as_mut().unwrap();
    infantry.fear_level = input["fear"].as_u64().unwrap_or(0) as u16;
    infantry.is_prone = false;
    actor.install_native_stage_fixture(StageClass::from_native_fixture(
        7,
        0,
        CdTimer::from_raw(17, 91),
        92,
        1,
    ));
    sim.sound_events.clear();
    if row["rng_before"].is_object() {
        sim.main_rng = serde_json::from_value::<SimRng>(row["rng_before"]["main"].clone())
            .unwrap()
            .into();
        sim.scenario_rng =
            serde_json::from_value::<SimRng>(row["rng_before"]["scenario"].clone()).unwrap();
        sim.mapgen_rng =
            serde_json::from_value::<SimRng>(row["rng_before"]["mapgen"].clone()).unwrap();
    }
    (sim, id)
}

fn full_rng(sim: &Simulation) -> Value {
    let views = sim.rng_views();
    let stream = |rng: crate::sim::rng::SimRngLogicalView<'_>| {
        json!({"disabled": rng.disabled, "index_a": rng.index_a,
            "index_b": rng.index_b, "state": rng.words})
    };
    let main = crate::sim::rng::SimRngLogicalView {
        disabled: views.main.disabled,
        index_a: views.main.index_a,
        index_b: views.main.index_b,
        words: &views.main.words,
    };
    json!({"main":stream(main), "scenario":stream(views.scenario),
        "mapgen":stream(views.mapgen)})
}

fn compare(row: &Value, rules: &RuleSet, type_name: &str) -> (Simulation, u64) {
    let (mut sim, id) = fixture(row, rules, type_name);
    let rng_before = sim.rng_state();
    let input = &row["input"];
    let accepted = sim
        .infantry_do_action(
            id,
            signed(&input["request"]),
            input["force"].as_bool().unwrap_or(false),
            rules,
            crate::sim::world::FrameEffects::default(),
        )
        .unwrap();
    assert_eq!(accepted, row["accepted"] == 1, "{type_name}: {input}");
    let out = if row["kind"] == "retail_action" {
        &row["after"]
    } else {
        row
    };
    let actor = sim.substrate.entities.get(id).unwrap();
    let leaf = actor.mission_leaf.as_infantry().unwrap();
    assert_eq!(leaf.doing(), signed(&out["doing"]), "{input}: Doing");
    assert_eq!(
        leaf.water_state(),
        signed(&out["water_state"]),
        "{input}: +6E8"
    );
    if let Some(prone) = out["prone"].as_u64() {
        assert_eq!(
            actor.infantry.as_ref().unwrap().is_prone,
            prone != 0,
            "{input}: remapped action retains the actual prone byte"
        );
    }
    let stage = actor.native_stage();
    assert_eq!(
        stage.value(),
        signed(&out["frame"]),
        "{input}: signed Stage"
    );
    assert_eq!(
        [
            stage.timer().start_frame(),
            stage.timer().duration(),
            stage.rate()
        ],
        [
            signed(&out["timer_start"]),
            signed(&out["timer_duration"]),
            signed(&out["timer_repeat"])
        ],
        "{input}: Stage timer/rate; copied stack +104 excluded"
    );
    let sounds = row["sounds"].as_array().unwrap();
    assert_eq!(
        sim.sound_events.len(),
        sounds.len(),
        "{input}: sound requests"
    );
    for (actual, expected) in sim.sound_events.iter().zip(sounds) {
        let name = if let Some(name) = expected["name"].as_str() {
            name
        } else if expected["index"] == 101 {
            "WaterEnter"
        } else {
            assert_eq!(expected["index"], 102);
            "WaterLeave"
        };
        let SimSoundEvent::VocAt {
            sound_id,
            audible_to,
            rx,
            ry,
            sub_x,
            sub_y,
            world_z_leptons,
        } = actual
        else {
            panic!("{input}: expected the positional native Voc request");
        };
        assert_eq!(sound_id, name, "{input}: sound identity");
        assert!(audible_to.is_none(), "{input}: native context0");
        assert_eq!(
            [
                i32::from(*rx) * 256 + sub_x.to_num::<i32>(),
                i32::from(*ry) * 256 + sub_y.to_num::<i32>(),
                *world_z_leptons
            ],
            [
                signed(&expected["xyz"][0]),
                signed(&expected["xyz"][1]),
                signed(&expected["xyz"][2])
            ],
            "{input}: sound uses physical Location"
        );
    }
    assert_eq!(
        sim.rng_state(),
        rng_before,
        "{input}: all three RNG objects"
    );
    if row["rng_after"].is_object() {
        assert_eq!(full_rng(&sim), row["rng_after"], "{input}: full native RNG");
    }
    (sim, id)
}

#[test]
fn native_constructor_and_physical_readers_supply_water_state_and_raw_records() {
    let constructors = oracle()
        .iter()
        .find(|row| row["kind"] == "constructors")
        .unwrap();
    assert_eq!(
        constructors["native_sha256"],
        "1cdd1180e49024fbda8ad568caac2e86e856063ff67ab38f62b7d2c7bb84298c"
    );
    for receipt in constructors["constructors"].as_array().unwrap() {
        if receipt["entry"] == "0x517a50" {
            assert_eq!(receipt["after"]["0x6e8"], 2);
            assert!(
                receipt["writes"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|write| write["pc"] == "0x517ac2"
                        && write["offset"] == "0x6e8"
                        && write["new"] == 2)
            );
        } else {
            assert_eq!(receipt["entry"], "0x5236a0");
            assert_eq!(receipt["after"]["0xea4"], -1);
            assert_eq!(receipt["after"]["0xea8"], -1);
        }
    }
    assert_ne!(constructors["e1_after_ready"]["movement_zone"], 3);
    assert_eq!(constructors["e1_after_ready"]["water_state"], 2);
    assert_eq!(constructors["e1_after_ready"]["ready_returned_al"], 1);
    let rules = physical_rules();
    for name in ["GHOST", "TANY"] {
        let receipt = reader(name);
        let object = rules.object(name).unwrap();
        assert_eq!(object.movement_zone, MovementZone::AmphibiousDestroyer);
        assert_eq!(
            object.enter_water_sound.as_deref(),
            Some("TanyaEntersWater")
        );
        assert_eq!(
            object.leave_water_sound.as_deref(),
            Some("TanyaLeavesWater")
        );
        assert_eq!(
            receipt["defaults"],
            json!({"enter_sound":-1,"leave_sound":-1})
        );
        let records = rules.animation_sequence(name).unwrap();
        for (action, expected) in receipt["records"].as_array().unwrap().iter().enumerate() {
            let record = records.infantry_action(action as i32).unwrap();
            assert_eq!(
                [record.start_frame, record.frames_per_facing, record.facings],
                [
                    signed(&expected[0]),
                    signed(&expected[1]),
                    signed(&expected[2])
                ],
                "{name} raw action{action}"
            );
        }
    }
}

#[test]
fn native_supplied_water_actions_retain_pre_admission_writes_and_clock() {
    for row in oracle().iter().take(295) {
        let rules = supplied_rules(&row["input"]);
        compare(row, &rules, "WATER");
    }
}

#[test]
fn native_physical_water_fire_idle_and_bridge_contrasts_match() {
    let rules = physical_rules();
    let mut count = 0;
    for row in oracle()
        .iter()
        .filter(|row| row["kind"] == "retail_action" && row["input"]["health"].is_null())
    {
        compare(row, &rules, row["type_name"].as_str().unwrap());
        count += 1;
    }
    assert_eq!(count, 54);
}

#[test]
fn native_physical_ghost_tanya_wet_death_uses_raw_doing_and_stage() {
    let rules = physical_rules();
    let mut count = 0;
    for row in oracle()
        .iter()
        .filter(|row| row["kind"] == "retail_action" && row["input"]["health"] == 0)
    {
        let (mut sim, id) = compare(row, &rules, row["type_name"].as_str().unwrap());
        let doing = sim
            .substrate
            .entities
            .get(id)
            .unwrap()
            .mission_leaf
            .as_infantry()
            .unwrap()
            .doing();
        assert!(matches!(doing, 20 | 21));
        let frames = rules
            .animation_sequence(row["type_name"].as_str().unwrap())
            .unwrap()
            .infantry_action(doing)
            .unwrap()
            .frames_per_facing;
        // The native520AE0 switch's WetDie20/21 arm owns eventual UnInit.
        // Supply its completed signed Stage through the clock owner; this
        // does not claim a full original wet AI/occupancy/audio replay.
        sim.substrate
            .entities
            .get_mut(id)
            .unwrap()
            .set_native_stage_value(frames);
        assert!(sim.infantry_sequencer(id, &rules, crate::sim::world::FrameEffects::default()));
        assert!(
            sim.substrate
                .entities
                .get(id)
                .is_none_or(|actor| !actor.lifecycle.object_alive)
        );
        count += 1;
    }
    assert_eq!(count, 4);
}

/// Bounded Rust production integration of the critic's stuck-wet-corpse
/// trigger. The native receiver comparison establishes the raw action/count;
/// this checks ordinary frame visits advance that one Stage to eventual
/// UnInit. It does not claim whole native wet AI/placement/audio parity.
#[test]
fn physical_wet_death_advances_in_logic_and_uninitializes_without_animation() {
    let rules = physical_rules();
    for row in oracle()
        .iter()
        .filter(|row| row["kind"] == "retail_action" && row["input"]["health"] == 0)
    {
        let name = row["type_name"].as_str().unwrap();
        let (mut sim, id) = fixture(row, &rules, name);
        // This integration extends the constructor/DoAction receipt with an
        // admitted stationary Unlimbo, including Techno+508 publication. The
        // original action-only rows above still exclude water placement.
        assert!(matches!(
            sim.reveal_entity_with_rules(id, &rules),
            crate::sim::world::RevealOutcome::Revealed { .. }
        ));
        assert!(
            sim.substrate
                .entities
                .get(id)
                .unwrap()
                .cached_spatial_threat()
                .is_some()
        );
        sim.set_logic_order_for_test(vec![id]);
        sim.begin_infantry_death_sequence(
            id,
            if row["input"]["request"] == 11 {
                crate::sim::world::InfantryDeathSequence::Die1
            } else {
                crate::sim::world::InfantryDeathSequence::Die2
            },
            &rules,
            crate::sim::world::FrameEffects::default(),
        );
        let doing = signed(&row["after"]["doing"]);
        let frames = rules
            .animation_sequence(name)
            .unwrap()
            .infantry_action(doing)
            .unwrap()
            .frames_per_facing;
        let mut progressed = false;
        for _ in 0..frames + 2 {
            sim.advance_tick(&[], Some(&rules), None, None, 67);
            let Some(actor) = sim.substrate.entities.get(id) else {
                break;
            };
            assert_eq!(
                actor.mission_leaf.as_infantry().unwrap().doing(),
                doing,
                "{name}: wet death retained"
            );
            assert_eq!(
                actor.health.current, 0,
                "{name}: retained native death Health0"
            );
            assert!(
                actor.animation.is_none(),
                "{name}: the sole Stage drives Infantry"
            );
            progressed |= actor.native_stage().value() > 0;
        }
        assert!(
            progressed,
            "{name}: production Stage advances before UnInit"
        );
        assert!(
            !sim.substrate.entities.contains(id),
            "{name}: finished wet death leaves no stuck entity"
        );
        assert!(
            !sim.live_object_order_snapshot().contains(&id),
            "{name}: UnInit removes Logic membership"
        );
    }
}
