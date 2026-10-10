//! Native evidence for the Gattling calls of [`super`]:
//! `tools/spatial_oracle/building_gattling.json` replayed through
//! Mission_Attack's arms, Mission_Guard and the Update's Gattling block one
//! call at a time, and its engagements through the production frame.

use std::collections::BTreeMap;

use serde_json::Value;

use super::tests::{CODES, FRAME, aim_at, fixture, mission_of};
use super::*;
use crate::map::resolved_terrain::test_flat_ground_grid;
use crate::rules::ini_parser::IniFile;
use crate::sim::combat::gattling::{GattlingState, gattling_sound_owner};
use crate::sim::combat::veterancy::RANK_ELITE_U16;
use crate::sim::command::{Command, CommandEnvelope};
use crate::sim::mission::MissionDispatchTimer;
use crate::sim::mission::state::MissionTestFixture;
use crate::sim::rng::SimRng;
use crate::sim::world::SimSoundEvent;

/// The oracle weapons' ROF by slot (retail `[YAGGUN]`'s: the AA weapon of
/// each stage fires faster than the last).
const ROF: [i32; 6] = [16, 16, 16, 8, 16, 4];

fn golden() -> Value {
    serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/building_gattling.json",
    ))
    .unwrap()
}

fn rows<'a>(golden: &'a Value, set: &str) -> &'a [Value] {
    golden[set].as_array().unwrap()
}

/// The oracle's building as test types, none of them passive-acquiring:
/// `GAT` with retail `[YAGGUN]`'s stage table (`RateDown=` from the row) and
/// six weapon pairs whose one-item `Report=` names their slot (`R0`..`R5`,
/// elite `RE0`..`RE5`), as the oracle's do; `GATEMP`, an EMP cannon; `GATHUT`,
/// unarmed; `PLAIN`, armed and not Gattling. `SHED` is the unarmed enemy they
/// aim at. MissionControl and `GuardAreaTargetingDelay` as the oracle's (its
/// fixture leaves Area Guard's MissionControl entry zeroed); `reports: false`
/// empties every `Report=`.
fn rules(delay: i64, rate_down: i64, reports: bool) -> RuleSet {
    let stages = format!(
        "IsGattling=yes\nWeaponStages=3\nStage1=200\nStage2=400\nStage3=600\n\
         EliteStage1=100\nEliteStage2=200\nEliteStage3=300\nRateUp=1\nRateDown={rate_down}\n"
    );
    let base = "Strength=1000\nArmor=concrete\nFoundation=1x1\nCanPassiveAquire=no\n\
                HasStupidGuardMode=no\n";
    let mut weapons = String::from("TurretCount=1\nWeaponCount=6\n");
    let mut weapon_types = String::new();
    for (slot, rof) in ROF.iter().enumerate() {
        weapons += &format!(
            "Weapon{}=W{slot}\nEliteWeapon{}=E{slot}\n",
            slot + 1,
            slot + 1
        );
        for (name, report) in [
            (format!("W{slot}"), format!("R{slot}")),
            (format!("E{slot}"), format!("RE{slot}")),
        ] {
            weapon_types +=
                &format!("[{name}]\nDamage=1\nROF={rof}\nRange=6\nProjectile=Shell\nWarhead=AP\n");
            if reports {
                weapon_types += &format!("Report={report}\n");
            }
        }
    }
    RuleSet::from_ini(&IniFile::from_str(&format!(
        "[General]\nGuardAreaTargetingDelay={delay}\n\
         [Guard]\nRate=.030\nAARate=.016\n\
         [Sticky]\nRate=.016\nAARate=.016\n\
         [Attack]\nRate=.016\nAARate=.016\n\
         [Area Guard]\nRate=0\nAARate=0\n\
         [BuildingTypes]\n0=GAT\n1=GATEMP\n2=GATHUT\n3=PLAIN\n4=SHED\n\
         [GAT]\n{base}{stages}{weapons}\
         [GATEMP]\n{base}{stages}{weapons}EMPulseCannon=yes\n\
         [GATHUT]\n{base}{stages}\
         [PLAIN]\n{base}Primary=W0\n\
         [SHED]\nStrength=1000\nArmor=wood\nFoundation=1x1\n\
         {weapon_types}\
         [Shell]\nAG=yes\n\
         [AP]\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n"
    )))
    .unwrap()
}

fn row_rules(input: &Value) -> RuleSet {
    rules(
        input["delay"].as_i64().unwrap_or(36),
        input["table"]["rate_down"].as_i64().unwrap_or(50),
        input["report_count"] != 0,
    )
}

/// The row's building type.
fn kind(input: &Value) -> &'static str {
    let flag = |key: &str| input[key].as_bool();
    if flag("gattling") == Some(false) {
        "PLAIN"
    } else if flag("armed") == Some(false) {
        "GATHUT"
    } else if flag("emp_cannon") == Some(true) {
        "GATEMP"
    } else {
        "GAT"
    }
}

/// The row's input state: mission, queue, status, `+0xC4`, the Gattling
/// fields, veterancy, LastFireFrame and `+0x148`, at the oracle's frame.
fn prepare(sim: &mut Simulation, id: u64, input: &Value) {
    let state = &input["state"];
    let int = |key: &str, default: i64| state[key].as_i64().unwrap_or(default);
    let entity = sim.substrate.entities.get_mut(id).unwrap();
    entity.mission.apply_test_fixture(MissionTestFixture {
        current: mission_of(input["mission"].as_str().unwrap_or("guard")),
        suspended: MissionId::NONE,
        queued: mission_of(input["queued"].as_str().unwrap_or("none")),
        movement_bypass_latch: 0,
        handler_state: input["status"].as_u64().unwrap_or(0) as u32,
        mission_start_frame: FRAME,
        ai_counter: int("counter", 0) as u32,
        dispatch_timer: MissionDispatchTimer::from_raw(FRAME as i32, 0),
    });
    entity.mission_leaf.set_building_ready_latch(0);
    entity.gattling = GattlingState::from_fields(
        int("stage", 0) as i32,
        int("value", 0) as i32,
        int("latch", 0) == 1,
    );
    if state["veterancy"].as_f64().unwrap_or(0.0) >= 2.0 {
        entity.set_veterancy_rank(RANK_ELITE_U16);
    }
    entity.last_fire_frame = int("last_fire_frame", -100);
    entity.turret_anim_frame = int("turret_count", 0) as i32;
}

/// A sound the Gattling calls start, stop or release, by report name.
#[derive(Debug, PartialEq, Eq)]
enum Sound {
    Loop(String),
    Stop,
    Release,
}

/// The oracle's report sound `index`: weapon slot `index - 1`, elite from 7.
fn report_name(index: u64) -> String {
    if index <= 6 {
        format!("R{}", index - 1)
    } else {
        format!("RE{}", index - 7)
    }
}

/// The loop events a native call list leaves in VERA's model: a stage-up
/// stops the loop before the next one plays (StopAndClear `0x0070DF74` runs
/// natively and is not recorded), a play starts one, and a release matters
/// only for a live loop (the latch was set).
fn expected_sounds(events: &[Value], latch_before: bool, stage_up: bool) -> Vec<Sound> {
    let mut sounds = Vec::new();
    for event in events {
        match event[0].as_str().unwrap() {
            "stage_call" if event[1] == "increase" && stage_up => sounds.push(Sound::Stop),
            "play_at" => sounds.push(Sound::Loop(report_name(event[1].as_u64().unwrap()))),
            "release" if latch_before => sounds.push(Sound::Release),
            _ => {}
        }
    }
    sounds
}

fn sounds(sim: &Simulation, id: u64) -> Vec<Sound> {
    let owner = gattling_sound_owner(id);
    sim.sound_events
        .iter()
        .filter_map(|event| match event {
            SimSoundEvent::GattlingLoop {
                owner: o, sound_id, ..
            } if *o == owner => Some(Sound::Loop(sound_id.clone())),
            SimSoundEvent::GattlingLoopStop { owner: o } if *o == owner => Some(Sound::Stop),
            SimSoundEvent::GattlingLoopRelease { owner: o } if *o == owner => Some(Sound::Release),
            _ => None,
        })
        .collect()
}

fn count(events: &[Value], name: &str, stream: Option<&str>) -> usize {
    events
        .iter()
        .filter(|event| event[0] == name && stream.is_none_or(|stream| event[1] == stream))
        .count()
}

/// Runs `call` and answers its return, asserting the row's draws on both
/// streams: `g_MainRng` (the report pick) and the Scenario's
/// `RandomRanged(0, 2)`.
fn with_draws(sim: &mut Simulation, row: &Value, call: impl FnOnce(&mut Simulation) -> i32) -> i32 {
    let name = &row["input"]["name"];
    let events = row["events"].as_array().unwrap();
    let mut main = sim.main_rng.clone();
    for _ in 0..count(events, "rng", Some("main")) {
        main.next_u32();
    }
    let mut scenario = sim.scenario_rng.clone();
    for _ in 0..count(events, "rng", Some("scenario_ranged")) {
        scenario.next_range_u32_inclusive(0, 2);
    }
    let returns = call(sim);
    assert_eq!(
        sim.main_rng.logical_state(),
        main.logical_state(),
        "{name} g_MainRng draws"
    );
    assert_eq!(
        sim.scenario_rng.logical_state(),
        scenario.logical_state(),
        "{name} Scenario draws"
    );
    returns
}

/// The row's recorded Gattling state, `+0xC4` and `+0x148`, and the loop
/// events the calls made.
fn assert_gattling(sim: &Simulation, id: u64, row: &Value, latch_before: bool, stage_before: i32) {
    let name = &row["input"]["name"];
    let native = &row["gattling"];
    let entity = sim.substrate.entities.get(id).unwrap();
    let stage = entity.gattling.stage();
    assert_eq!(
        i64::from(stage),
        native["stage"].as_i64().unwrap(),
        "{name} stage"
    );
    assert_eq!(
        i64::from(entity.gattling.value()),
        native["value"].as_i64().unwrap(),
        "{name} value"
    );
    assert_eq!(
        entity.gattling.report_latch(),
        native["latch"] == 1,
        "{name} latch"
    );
    assert_eq!(
        i64::from(entity.mission.ai_counter()),
        native["counter"].as_i64().unwrap(),
        "{name} +0xC4"
    );
    assert_eq!(
        i64::from(entity.turret_anim_frame),
        native["turret_count"].as_i64().unwrap(),
        "{name} +0x148"
    );
    assert_eq!(
        sounds(sim, id),
        expected_sounds(
            row["events"].as_array().unwrap(),
            latch_before,
            stage > stage_before
        ),
        "{name} loop"
    );
}

/// Every `attack` row: Mission_Attack's Gattling call per GetFireError code
/// (answered from the row) with its ticks, its `+0xC4` and `+0x148` writes,
/// the FireAt before the charge with SelectWeapon's `2s` (the request carries
/// it), the report draw and loop, and the arms that make no call: the Wait
/// drop, the null target and codes 4, 7 and 11. A plain building keeps its
/// count and advances `+0x148` on OK and REARM. The combat phase then fires
/// each requested shot with the FireAt's weapon, the stage's own even when
/// the charge after it stepped the stage (`ok_stage_up`).
#[test]
fn gattling_attack_matches_the_original() {
    let golden = golden();
    let attack = rows(&golden, "attack");
    assert_eq!(attack.len(), 26);
    for row in attack {
        let input = &row["input"];
        let name = input["name"].as_str().unwrap();
        let rules = row_rules(input);
        let (mut sim, building, target) = fixture(&rules, kind(input), (8, 5));
        prepare(&mut sim, building, input);
        if input["target"] == true {
            aim_at(&mut sim, building, target);
        }
        let (latch_before, stage_before) = {
            let entity = sim.substrate.entities.get(building).unwrap();
            (entity.gattling.report_latch(), entity.gattling.stage())
        };
        let events = row["events"].as_array().unwrap();
        let returns = with_draws(&mut sim, row, |sim| {
            let Some((target, weapon)) = attack_prelude(sim, building, &rules) else {
                return 1;
            };
            let asked = events
                .iter()
                .find(|event| event[0] == "fire_error")
                .unwrap();
            assert_eq!(
                i64::from(weapon),
                asked[2].as_i64().unwrap(),
                "{name} SelectWeapon"
            );
            let code = CODES[input["errors"][0].as_u64().unwrap() as usize];
            attack_arm(sim, building, &rules, target, weapon, code)
        });
        assert_eq!(
            i64::from(returns),
            row["returns"].as_i64().unwrap(),
            "{name}"
        );
        let entity = sim.substrate.entities.get(building).unwrap();
        assert_eq!(
            entity.mission.current(),
            mission_of(row["mission"].as_str().unwrap()),
            "{name} mission"
        );
        assert_eq!(
            entity.mission.queued(),
            mission_of(row["queued"].as_str().unwrap()),
            "{name} queue"
        );
        assert_eq!(
            entity.attack_target.is_some(),
            row["target"].is_string(),
            "{name} target"
        );
        let fire_at = events.iter().find(|event| event[0] == "fire_at");
        assert_eq!(
            sim.fire_requests.buildings.get(&building).copied(),
            fire_at.map(|event| BuildingShot::Mission {
                weapon: event[2].as_i64().unwrap() as i32,
                target: TargetKind::Entity(target),
            }),
            "{name} FireAt"
        );
        assert_gattling(&sim, building, row, latch_before, stage_before);
        if let Some(event) = fire_at {
            let elite = state_is_elite(input);
            let shot = combat_phase_shot(&mut sim, &rules, building);
            assert_eq!(
                shot,
                Some((
                    weapon_name(event[2].as_u64().unwrap(), elite),
                    TargetKind::Entity(target)
                )),
                "{name} shot"
            );
        }
    }
}

/// Whether the row's building is elite (`+0x150` at 2.0 or more).
fn state_is_elite(input: &Value) -> bool {
    input["state"]["veterancy"].as_f64().unwrap_or(0.0) >= 2.0
}

/// The test types' weapon in slot `slot`: `W0`..`W5`, elite `E0`..`E5`.
fn weapon_name(slot: u64, elite: bool) -> String {
    format!("{}{slot}", if elite { 'E' } else { 'W' })
}

/// Runs the combat phase over the frame's requests and answers the
/// building's shot: the weapon it fired and what at.
fn combat_phase_shot(
    sim: &mut Simulation,
    rules: &RuleSet,
    building: u64,
) -> Option<(String, TargetKind)> {
    let requests = std::mem::take(&mut sim.fire_requests);
    let result = sim.tick_combat_with_fatal_lifecycle(
        rules,
        None,
        67,
        &[building],
        &Default::default(),
        &requests,
        &[],
    );
    result
        .consequences
        .fire_events()
        .iter()
        .find(|event| event.attacker_id == building)
        .map(|event| {
            (
                sim.interner.resolve(event.weapon_id).to_string(),
                event.target,
            )
        })
}

/// A later object retargeting the building after its visit (a bullet's
/// retaliation later in the Logic pass, say) leaves the visit's shot alone:
/// native's FireAt ran inside the visit (`0x0044B6D0`), at that visit's
/// TarCom with its weapon, so the combat phase fires the request's weapon at
/// the request's target.
#[test]
fn a_retarget_after_the_visit_keeps_the_visits_shot() {
    let rules = rules(36, 50, true);
    let (mut sim, building, target) = fixture(&rules, "GAT", (8, 5));
    let other = sim
        .spawn_object("SHED", "Russians", 5, 8, 0, &rules)
        .unwrap();
    aim_at(&mut sim, building, target);
    let (aimed, weapon) = attack_prelude(&mut sim, building, &rules).unwrap();
    assert_eq!(
        attack_arm(&mut sim, building, &rules, aimed, weapon, FireError::Ok),
        1
    );
    aim_at(&mut sim, building, other);
    assert_eq!(
        combat_phase_shot(&mut sim, &rules, building),
        Some(("W0".to_string(), TargetKind::Entity(target)))
    );
    let entity = sim.substrate.entities.get(building).unwrap();
    assert_eq!(
        entity.attack_target.as_ref().map(|attack| attack.target),
        Some(TargetKind::Entity(other))
    );
}

/// Every `guard` row: Mission_Guard's head decays by the whole count and
/// zeroes it before the handler's own work, for Guard, Sticky and Area Guard,
/// armed or not, on the dispatch that commits Attack too, and before the
/// Scenario draw; a plain building keeps its count.
#[test]
fn gattling_guard_matches_the_original() {
    let golden = golden();
    let guard = rows(&golden, "guard");
    assert_eq!(guard.len(), 8);
    for row in guard {
        let input = &row["input"];
        let name = input["name"].as_str().unwrap();
        let rules = row_rules(input);
        let (mut sim, building, target) = fixture(&rules, kind(input), (8, 5));
        prepare(&mut sim, building, input);
        if input["target"] == true {
            aim_at(&mut sim, building, target);
        }
        // An unseeded oracle row draws from the fixture's state, whose first
        // draw is 1 like seed 1's.
        sim.scenario_rng = SimRng::new(input["seed"].as_u64().unwrap_or(1));
        let (latch_before, stage_before) = {
            let entity = sim.substrate.entities.get(building).unwrap();
            (entity.gattling.report_latch(), entity.gattling.stage())
        };
        let returns = with_draws(&mut sim, row, |sim| mission_guard(sim, building, &rules));
        assert_eq!(
            i64::from(returns),
            row["returns"].as_i64().unwrap(),
            "{name}"
        );
        let entity = sim.substrate.entities.get(building).unwrap();
        assert_eq!(
            entity.mission.current(),
            mission_of(row["mission"].as_str().unwrap()),
            "{name} mission"
        );
        assert_eq!(
            u64::from(entity.mission.handler_state()),
            row["status"].as_u64().unwrap(),
            "{name} status"
        );
        assert_gattling(&sim, building, row, latch_before, stage_before);
    }
}

/// Every `idle_decay` row through [`gattling_idle`]: `Frame - LastFireFrame`
/// against `GuardAreaTargetingDelay + 5`, strict and signed (a negative delay
/// is not clamped), the effective mission's Attack test, one `RateDown` and at
/// most one stage down (elite thresholds for an elite), the two `+0x148`
/// steps, no loop, latch or draw, and nothing for a plain or dead building.
#[test]
fn gattling_idle_decay_matches_the_original() {
    let golden = golden();
    let idle = rows(&golden, "idle_decay");
    assert_eq!(idle.len(), 22);
    for row in idle {
        let input = &row["input"];
        let name = input["name"].as_str().unwrap();
        let rules = row_rules(input);
        let (mut sim, building, _) = fixture(&rules, kind(input), (8, 5));
        prepare(&mut sim, building, input);
        let since = input["since"].as_i64().unwrap();
        let entity = sim.substrate.entities.get_mut(building).unwrap();
        entity.last_fire_frame = i64::from(FRAME) - since;
        if input["dead"] == true {
            entity.health.current = 0;
        }
        let dead = row["ended"].as_u64().unwrap() == 0x0044_0573;
        assert_eq!(dead, input["dead"] == true, "{name}");
        let latch_before = entity.gattling.report_latch();
        let stage_before = entity.gattling.stage();
        with_draws(&mut sim, row, |sim| {
            gattling_idle(sim, building, &rules);
            0
        });
        assert_gattling(&sim, building, row, latch_before, stage_before);
    }
}

/// One cadence row's event, applied before frame `k`.
enum Event {
    Acquire,
    Lose,
    /// A retaliation: `Override_Mission(Attack, target, 0)`.
    Override,
    /// GetFireError answers RANGE this frame: the building aims at a second
    /// target beyond its weapons' range just before it.
    OutOfRange,
}

/// The cadence rows through the production frame (`advance_tick`), frame by
/// frame: mission, `+0xC4`, stage, value, latch, `+0x148`, LastFireFrame, the
/// shot and its weapon (the stage's `2s`, fired before the visit's charge),
/// the loop events and the `g_MainRng` draws: a cannon charges RateUp per
/// frame it spends attacking, its stage rises at 200 and 400 with a new loop,
/// the drop tail and every Guard dispatch decay by their counts, and the idle
/// decay starts `GuardAreaTargetingDelay + 6` frames after the last shot.
/// A retaliation's first Attack dispatch charges the Guard's whole count.
///
/// Substitutions, as in `building_dispatch_cadence_matches_the_original`: the
/// oracle answers FireAt with a rearm of exactly the fired weapon's ROF, so
/// the replay overwrites the one each shot wrote (GetROF's own draw is
/// `combat::rof`'s), and it restores the Scenario stream to the oracle's
/// before each frame, since the oracle's FireAt draws nothing. The
/// turretless fixture never answers FACING, so `facing_stretch` (whose arm
/// the `attack` rows pin) is not replayed.
#[test]
fn gattling_cadence_matches_the_original() {
    let golden = golden();
    let cadence = rows(&golden, "cadence");
    assert_eq!(cadence.len(), 5);
    let mut shots_compared = 0;
    let mut loops_compared = 0;
    for row in cadence {
        let input = &row["input"];
        let name = input["name"].as_str().unwrap();
        if name == "facing_stretch" {
            continue;
        }
        assert_eq!(input["seed"], 1, "{name}");
        let rules = row_rules(input);
        let mut sim = Simulation::new();
        sim.install_resolved_terrain_for_new_map(test_flat_ground_grid(16));
        let building = sim
            .spawn_object("GAT", "Americans", 5, 5, 0, &rules)
            .unwrap();
        let target = sim
            .spawn_object("SHED", "Russians", 8, 5, 0, &rules)
            .unwrap();
        let far = sim
            .spawn_object("SHED", "Russians", 15, 5, 0, &rules)
            .unwrap();
        let events: BTreeMap<u64, Event> = input["events"]
            .as_array()
            .unwrap()
            .iter()
            .map(|event| {
                let kind = match event[1].as_str().unwrap() {
                    "acquire" => Event::Acquire,
                    "lose" => Event::Lose,
                    "override" => Event::Override,
                    "error:8" => Event::OutOfRange,
                    other => panic!("event {other}"),
                };
                (event[0].as_u64().unwrap(), kind)
            })
            .collect();
        sim.main_rng = SimRng::new(input["main_seed"].as_u64().unwrap_or(1));
        let mut scenario_draws = 0;
        // The object pass of the frame `advance_tick` commits as `start + k`
        // runs at `base + k`, the frame its timers record.
        let start = sim.session.binary_frame;
        let base = i64::from(start) - 1;
        let mut latch_before = false;
        let mut stage_before = 0;
        for frame in row["frames"].as_array().unwrap() {
            let k = frame["frame"].as_u64().unwrap();
            let at = format!("{name} frame {k}");
            match events.get(&k) {
                Some(Event::Acquire) => {
                    sim.assign_target_represented(
                        building,
                        Some(TargetKind::Entity(target)),
                        Some(&rules),
                    )
                    .unwrap();
                }
                Some(Event::Lose) => {
                    sim.assign_target_represented(building, None, Some(&rules))
                        .unwrap();
                }
                Some(Event::Override) => {
                    assert!(sim.override_mission_on_damage_response(building, target, &rules));
                }
                Some(Event::OutOfRange) => aim_at(&mut sim, building, far),
                None => {}
            }
            let mut scenario = SimRng::new(1);
            for _ in 0..scenario_draws {
                scenario.next_range_u32_inclusive(0, 2);
            }
            sim.scenario_rng = scenario;
            let native_events = frame["events"].as_array().unwrap();
            scenario_draws += count(native_events, "rng", Some("scenario_ranged"));
            let mut main = sim.main_rng.clone();
            for _ in 0..count(native_events, "rng", Some("main")) {
                main.next_u32();
            }
            sim.fire_events.clear();
            sim.sound_events.clear();
            sim.advance_tick(&[], Some(&rules), None, None, 67);
            assert_eq!(u64::from(sim.session.binary_frame - start), k, "{name}");

            let entity = sim.substrate.entities.get(building).unwrap();
            assert_eq!(
                entity.mission.current(),
                mission_of(frame["mission"].as_str().unwrap()),
                "{at} mission"
            );
            let native = &frame["gattling"];
            let lff = native["last_fire_frame"].as_i64().unwrap();
            assert_eq!(
                entity.last_fire_frame,
                if lff == -100 {
                    -100
                } else {
                    lff - i64::from(FRAME) + base
                },
                "{at} LastFireFrame"
            );
            assert_eq!(
                sim.main_rng.logical_state(),
                main.logical_state(),
                "{at} g_MainRng draws"
            );
            let shot = sim
                .fire_events
                .iter()
                .find(|event| event.attacker_id == building)
                .map(|event| {
                    assert_eq!(event.report_sound_id, None, "{at} per-shot report");
                    sim.interner.resolve(event.weapon_id).to_string()
                });
            let native_shot = native_events
                .iter()
                .find(|event| event[0] == "fire_at")
                .map(|event| event[2].as_u64().unwrap() as usize);
            assert_eq!(
                shot,
                native_shot.map(|weapon| format!("W{weapon}")),
                "{at} shot"
            );
            if let Some(weapon) = native_shot {
                shots_compared += 1;
                sim.substrate
                    .entities
                    .get_mut(building)
                    .unwrap()
                    .rearm_timer
                    .start((base + k as i64) as i32, ROF[weapon]);
            }
            loops_compared += count(native_events, "play_at", None);
            let frame_row = serde_json::json!({
                "input": {"name": at},
                "gattling": native,
                "events": native_events,
            });
            assert_gattling(&sim, building, &frame_row, latch_before, stage_before);
            latch_before = native["latch"] == 1;
            stage_before = native["stage"].as_i64().unwrap() as i32;
        }
    }
    // 25, 41, 17 and 3 shots with the stages' weapons; a loop at each first
    // charge and stage-up.
    assert_eq!((shots_compared, loops_compared), (86, 9));
}

/// Every `report_gate` row through
/// [`crate::sim::combat::world_receiver::per_shot_report`]: a Gattling type
/// plays no per-shot report whatever its list; any other plays
/// `Report[(u16)+0x3C8 % Count]` unless the list is empty.
#[test]
fn per_shot_report_matches_the_original() {
    let golden = golden();
    let gate = rows(&golden, "report_gate");
    assert_eq!(gate.len(), 12);
    for row in gate {
        let input = &row["input"];
        let name = input["name"].as_str().unwrap();
        let count = input["count"].as_u64().unwrap() as usize;
        let ini = IniFile::from_str(&format!(
            "[W]\nDamage=1\nReport={}\n",
            ["S11", "S12", "S13"][..count].join(",")
        ));
        let weapon =
            crate::rules::weapon_type::WeaponType::from_ini_section("W", ini.section("W").unwrap());
        let played = crate::sim::combat::world_receiver::per_shot_report(
            &weapon,
            input["gattling"] == true,
            input["sequence"].as_u64().unwrap() as u16,
        );
        let native = row["events"]
            .as_array()
            .unwrap()
            .iter()
            .find(|event| event[0] == "play_at")
            .map(|event| format!("S{}", event[1]));
        assert_eq!(played.map(str::to_string), native, "{name}");
    }
}

/// A retail Gattling Cannon (`[YAGGUN]`: Stage1..3 = 200/400/600, RateUp 1,
/// RateDown 50) through the production frame on Dustbowl, against an unarmed
/// enemy MCV five cells east:
/// - From its Attack commit it charges one per frame (every dispatch passes
///   the frames since the last): the value is the frames since the commit,
///   or one less between the dispatches a REARM return spaces out.
/// - Its stage rises once the value it starts a charge from reaches 200 and
///   400, each with a new loop (`GattlingGunAttackLoop1`..`3`, a hard stop
///   before the next), and every shot fires the stage's ground weapon as the
///   dispatch found it (`AGGattling`, `AGGattling2`, `AGGattling3`) with no
///   per-shot report.
/// - The MCV drives off. Once the cannon drops it, the drop tail or the first
///   Guard dispatch releases the loop, and each call decays 50 a frame and at
///   most one stage: stage 0 and value 0 within three Guard dispatches.
#[test]
#[ignore = "requires a retail RA2/YR install (RA2_DIR or config.toml)"]
fn retail_dustbowl_gattling_cannon_spins_up_and_winds_down() {
    use super::tests::{retail_distance, retail_dustbowl_defence, retail_frame};
    let (mut scenario, cannon, truck, (x, y)) =
        retail_dustbowl_defence("YAGGUN", "Russians", "Americans", "AMCV", 2);
    {
        let rules = &scenario.runtime.resources.rules;
        let obj = rules.object("YAGGUN").unwrap();
        let table = &obj.gattling_stages;
        assert!(obj.is_gattling);
        assert_eq!(
            (1..=3)
                .map(|stage| table.threshold(stage, false))
                .collect::<Vec<_>>(),
            [200, 400, 600]
        );
        assert_eq!((table.rate_up(), table.rate_down()), (1, 50));
    }
    let ground = ["AGGattling", "AGGattling2", "AGGattling3"];
    let owner = gattling_sound_owner(cannon);
    let state = |scenario: &crate::headless_scenario::HeadlessScenario| {
        let entity = scenario.sim().substrate.entities.get(cannon).unwrap();
        (
            entity.mission.current().known(),
            entity.attack_target.is_some(),
            entity.gattling.stage(),
            entity.gattling.value(),
        )
    };
    let loops = |output: &crate::sim::world::SimFrameOutput| {
        output
            .sound_events
            .iter()
            .filter_map(|event| match event {
                SimSoundEvent::GattlingLoop {
                    owner: o, sound_id, ..
                } if *o == owner => Some(Sound::Loop(sound_id.clone())),
                SimSoundEvent::GattlingLoopStop { owner: o } if *o == owner => Some(Sound::Stop),
                SimSoundEvent::GattlingLoopRelease { owner: o } if *o == owner => {
                    Some(Sound::Release)
                }
                _ => None,
            })
            .collect::<Vec<_>>()
    };

    let mut sounds = Vec::new();
    let mut stage_ups = Vec::new();
    let mut shots = [0_usize; 3];
    let mut attacking = 0;
    for frame in 0..700_u32 {
        let (_, _, stage_before, _) = state(&scenario);
        let output = retail_frame(&mut scenario, Vec::new());
        let (mission, _, stage, value) = state(&scenario);
        if mission == Some(MissionType::Attack) {
            // The commit frame counts, and its count went to the Guard head.
            attacking += 1;
            if value < 600 {
                assert!(
                    (1..=2).contains(&(attacking - value)),
                    "frame {frame}: value {value} after {attacking} frames in Attack"
                );
            }
        }
        if stage != stage_before {
            assert_eq!(stage, stage_before + 1, "frame {frame}");
            let threshold = [200, 400][stage_before as usize];
            assert!(
                (threshold + 1..=threshold + 3).contains(&value),
                "frame {frame}: stage {stage} at value {value}"
            );
            stage_ups.push(frame);
        }
        for event in output
            .fire_events
            .iter()
            .filter(|event| event.attacker_id == cannon)
        {
            assert_eq!(event.target, TargetKind::Entity(truck));
            assert_eq!(
                event.report_sound_id, None,
                "frame {frame}: per-shot report"
            );
            assert_eq!(
                scenario.sim().interner.resolve(event.weapon_id),
                ground[stage_before as usize],
                "frame {frame}: the stage's ground weapon"
            );
            shots[stage_before as usize] += 1;
        }
        sounds.extend(loops(&output));
    }
    println!("YAGGUN: stage-ups at {stage_ups:?}, shots per stage {shots:?}");
    assert_eq!(stage_ups.len(), 2, "two stage-ups");
    assert!(shots.iter().all(|&count| count > 0), "{shots:?}");
    assert_eq!(
        sounds,
        [
            Sound::Loop("GattlingGunAttackLoop1".to_string()),
            Sound::Stop,
            Sound::Loop("GattlingGunAttackLoop2".to_string()),
            Sound::Stop,
            Sound::Loop("GattlingGunAttackLoop3".to_string()),
        ]
    );

    let enemy_house = scenario.sim().interner.get("Americans").unwrap();
    let tick = scenario.sim().session.tick;
    let order = retail_frame(
        &mut scenario,
        vec![CommandEnvelope::new(
            enemy_house,
            tick + 1,
            Command::Move {
                entity_id: truck,
                target_rx: x + 17,
                target_ry: y,
                queue: false,
            },
        )],
    );
    let mut sounds = loops(&order);
    let dropped = (0..600_u32)
        .find(|_| {
            let output = retail_frame(&mut scenario, Vec::new());
            sounds.extend(loops(&output));
            !state(&scenario).1
        })
        .expect("the cannon drops the MCV");
    println!(
        "YAGGUN: dropped the MCV {dropped} frames after the order, {} leptons out",
        retail_distance(&scenario, cannon, truck)
    );
    let wound_down = (0..60_u32).find(|_| {
        let output = retail_frame(&mut scenario, Vec::new());
        sounds.extend(loops(&output));
        let (mission, _, stage, value) = state(&scenario);
        mission == Some(MissionType::Guard) && (stage, value) == (0, 0)
    });
    assert!(wound_down.is_some(), "stage and value back to 0 on Guard");
    assert_eq!(sounds, [Sound::Release], "one release, the loop's");
}
