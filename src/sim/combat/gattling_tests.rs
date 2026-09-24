//! Native parity for the Gattling stage owner (`gattling`): the original
//! bodies and accessors (`tools/spatial_oracle/gattling_stage.py`) and the
//! unit's per-frame firing update (`gattling_unit_fire.py`), replayed through
//! the production code.

use serde_json::Value;

use super::{GattlingState, UnitFireOutcome};
use crate::rules::gattling_type::GattlingStages;
use crate::sim::combat::fire_error::FireError;
use crate::sim::rng::SimRng;
use crate::util::native_x87::NativeF32Bits;

fn stage_payload() -> Value {
    serde_json::from_str(include_str!(
        "../../../tools/spatial_oracle/gattling_stage.json"
    ))
    .expect("gattling_stage.json parses")
}

fn int(value: &Value) -> i64 {
    value
        .as_i64()
        .unwrap_or_else(|| panic!("integer expected: {value}"))
}

fn i32_of(value: &Value) -> i32 {
    int(value) as i32
}

/// A history's veterancy float, as the payload writes it (a number or the
/// bit pattern of a special value), to the production elite test.
fn elite_of(value: &Value) -> bool {
    let bits = match value {
        Value::String(hex) => {
            u32::from_str_radix(hex.trim_start_matches("0x"), 16).expect("float bits")
        }
        other => (other.as_f64().expect("veterancy") as f32).to_bits(),
    };
    crate::sim::combat::veterancy::rank_of(NativeF32Bits::from_bits(bits))
        == crate::sim::combat::veterancy::VeterancyRank::Elite
}

/// One history's type: the stage block and both weapon arrays, each weapon's
/// `Report=` sound numbers.
struct Type {
    stages: GattlingStages,
    weapons: Vec<Option<Vec<i64>>>,
    elite_weapons: Vec<Option<Vec<i64>>>,
}

impl Type {
    fn of(payload: &Value, input: &Value) -> Self {
        let mut table = payload["types"][input["type"].as_str().unwrap()].clone();
        if let Some(overrides) = input.get("type_overrides").and_then(Value::as_object) {
            for (key, value) in overrides {
                table[key] = value.clone();
            }
        }
        let six = |key: &str| -> [i32; 6] {
            let values: Vec<i32> = table[key].as_array().unwrap().iter().map(i32_of).collect();
            values.try_into().unwrap()
        };
        let weapons = |key: &str| -> Vec<Option<Vec<i64>>> {
            table[key]
                .as_array()
                .unwrap()
                .iter()
                .map(|weapon| {
                    (!weapon.is_null()).then(|| {
                        // A `report_count` override stands for the Report
                        // vector's count word; the items stay as listed.
                        let items: Vec<i64> = weapon["report"]
                            .as_array()
                            .unwrap()
                            .iter()
                            .map(int)
                            .collect();
                        match weapon.get("report_count") {
                            Some(count) if int(count) <= 0 => Vec::new(),
                            _ => items,
                        }
                    })
                })
                .collect()
        };
        Self {
            stages: GattlingStages::from_fields(
                i32_of(&table["weapon_stages"]),
                six("stage"),
                six("elite_stage"),
                i32_of(&table["rate_up"]),
                i32_of(&table["rate_down"]),
            ),
            weapons: weapons("weapons"),
            elite_weapons: weapons("elite_weapons"),
        }
    }

    /// `GetWeapon(index)->WeaponType->Report`: the elite slot when elite and
    /// filled, else the base slot; `None` for an empty slot.
    fn report(&self, index: i32, elite: bool) -> Option<&Vec<i64>> {
        let slot = usize::try_from(index).ok()?;
        let elite_slot = elite
            .then(|| self.elite_weapons.get(slot).and_then(Option::as_ref))
            .flatten();
        elite_slot.or_else(|| self.weapons.get(slot).and_then(Option::as_ref))
    }
}

/// Every call of every original history, through `GattlingState`: the value,
/// the stage, the latch, each `g_MainRng` draw and the sound it picked, and the
/// stage-up/stage-down paths.
#[test]
fn original_stage_histories() {
    let payload = stage_payload();
    let initial = &payload["initial"];
    let histories = payload["histories"].as_array().unwrap();
    assert_eq!(histories.len(), 68);
    let mut calls_checked = 0;
    let mut draws_checked = 0;
    for history in histories {
        let input = &history["input"];
        let name = input["name"].as_str().unwrap();
        let ty = Type::of(&payload, input);
        let start = |key: &str| {
            input
                .get("initial")
                .and_then(|initial| initial.get(key))
                .unwrap_or(&initial[key])
                .clone()
        };
        let mut state = GattlingState::from_fields(
            i32_of(&start("stage")),
            i32_of(&start("value")),
            int(&start("latch")) != 0,
        );
        let mut elite = elite_of(&start("veterancy"));
        let mut rng = SimRng::new(int(&input["seed"]) as u64);
        for (index, call) in history["calls"].as_array().unwrap().iter().enumerate() {
            let at = format!("{name} call {index}");
            let op = call["op"].as_str().unwrap();
            let path = call["path"].as_str().unwrap_or("");
            let mut draws = Vec::new();
            let mut sounds = Vec::new();
            match op {
                "veterancy" => {
                    elite = elite_of(&call["arg"]);
                    continue;
                }
                "increase" => {
                    let before_latch = state.report_latch();
                    let effects = state.increase(
                        &ty.stages,
                        elite,
                        i32_of(&call["arg"]),
                        |weapon| ty.report(weapon, elite).map(|items| items.len() as i32),
                        || {
                            let draw = rng.next_u32();
                            draws.push(i64::from(draw));
                            draw
                        },
                    );
                    assert_eq!(effects.stage_up, path.contains("up"), "{at}: stage-up");
                    if let Some(report) = effects.report {
                        assert!(!before_latch || effects.stage_up, "{at}");
                        sounds.push(
                            ty.report(report.weapon_index, elite).unwrap()[report.item as usize],
                        );
                    }
                    assert_eq!(effects.report.is_some(), path.contains("report"), "{at}");
                }
                "update" => {
                    let (before_latch, before_stage) = (state.report_latch(), state.stage());
                    let released = state.update(&ty.stages, elite, i32_of(&call["arg"]));
                    assert_eq!(
                        state.stage() < before_stage,
                        path.contains("down"),
                        "{at}: stage-down"
                    );
                    // The release always runs; it has a loop to release
                    // exactly while the latch was set.
                    assert_eq!(released, before_latch, "{at}");
                }
                "set_stage" => state.set_stage(i32_of(&call["arg"])),
                "set_value" => state.set_value(i32_of(&call["arg"])),
                "decrease_value" => state.decrease_value(i32_of(&call["arg"])),
                "get_stage" => assert_eq!(i64::from(state.stage()), int(&call["ret"]), "{at}"),
                "get_value" => assert_eq!(i64::from(state.value()), int(&call["ret"]), "{at}"),
                other => panic!("{at}: unknown op {other}"),
            }
            assert_eq!(i64::from(state.value()), int(&call["value"]), "{at}: value");
            assert_eq!(i64::from(state.stage()), int(&call["stage"]), "{at}: stage");
            assert_eq!(
                state.report_latch(),
                int(&call["latch"]) != 0,
                "{at}: latch"
            );
            let native_draws: Vec<i64> =
                call["draws"].as_array().unwrap().iter().map(int).collect();
            assert_eq!(draws, native_draws, "{at}: g_MainRng draws");
            let native_sounds: Vec<i64> = call["plays"]
                .as_array()
                .unwrap()
                .iter()
                .map(|play| int(&play["sound"]))
                .collect();
            assert_eq!(sounds, native_sounds, "{at}: report");
            calls_checked += 1;
            draws_checked += draws.len();
        }
    }
    // 3,586 recorded calls, nine of them veterancy writes.
    assert_eq!(calls_checked, 3577);
    assert_eq!(draws_checked, 77);
}

/// The two inputs on which the original faults (`0x0070DF8A`, a NULL
/// WeaponType's Report count): VERA plays nothing and keeps the latch clear.
#[test]
fn original_fault_inputs_play_nothing() {
    let payload = stage_payload();
    let faults = payload["faults"].as_array().unwrap();
    assert_eq!(faults.len(), 2);
    for fault in faults {
        let input = &fault["input"]["history"];
        assert_eq!(fault["fault_at"], "0x0070DF8A");
        let ty = Type::of(&payload, input);
        let initial = &input["initial"];
        let mut state = GattlingState::from_fields(
            initial.get("stage").map_or(0, i32_of),
            initial.get("value").map_or(0, i32_of),
            false,
        );
        let effects = state.increase(
            &ty.stages,
            false,
            1,
            |weapon| ty.report(weapon, false).map(|items| items.len() as i32),
            || panic!("no draw without a WeaponType"),
        );
        assert!(effects.report.is_none());
        assert!(!state.report_latch());
    }
}

// ---- the unit's per-frame firing update, through the production tail ----

fn unit_fire_payload() -> Value {
    serde_json::from_str(include_str!(
        "../../../tools/spatial_oracle/gattling_unit_fire.json"
    ))
    .expect("gattling_unit_fire.json parses")
}

/// A `[YTNK]`-shaped vehicle: the retail stage tables and six weapons whose
/// `Report=` sounds carry the oracle's sound numbers (`S1`..`S6`, elite
/// `S101`..`S106`). `slot0 = false` leaves `Weapon1=` out.
fn unit_fire_rules(is_gattling: bool, slot0: bool) -> crate::rules::ruleset::RuleSet {
    let mut text =
        String::from("[VehicleTypes]\n0=GTNK\n[InfantryTypes]\n[BuildingTypes]\n[AircraftTypes]\n");
    text.push_str(&format!(
        "[GTNK]\nStrength=300\nArmor=light\nTurret=yes\nIsGattling={}\nTurretCount=1\n\
         WeaponCount=6\nWeaponStages=3\nStage1=200\nStage2=400\nStage3=600\n\
         EliteStage1=100\nEliteStage2=200\nEliteStage3=300\nRateUp=1\nRateDown=50\n",
        if is_gattling { "yes" } else { "no" }
    ));
    for slot in 0..6 {
        if slot > 0 || slot0 {
            text.push_str(&format!(
                "Weapon{0}=W{1}\nEliteWeapon{0}=E{1}\n",
                slot + 1,
                slot
            ));
        }
    }
    for slot in 0..6 {
        text.push_str(&format!(
            "[W{slot}]\nDamage=25\nROF=16\nRange=6\nWarhead=WH\nReport=S{}\n\
             [E{slot}]\nDamage=25\nROF=16\nRange=6\nWarhead=WH\nReport=S{}\n",
            slot + 1,
            101 + slot
        ));
    }
    text.push_str("[WH]\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n");
    crate::rules::ruleset::RuleSet::from_ini(&crate::rules::ini_parser::IniFile::from_str(&text))
        .expect("gattling unit rules")
}

/// Every representable row of the original `0x00736DF0`, through
/// `Simulation::unit_fire_update_tail`: which call ran (charge, decay, or
/// neither), the stage, value, latch and `+0x148` afterwards, the `g_MainRng`
/// draw and the loop it started.
///
/// Not compared: codes the port cannot represent (-1, 12, `0x7FFFFFFF`: no
/// GetFireError returns them); the vt+0x4E4 return (a recorded residual,
/// dormant for every retail gattling type); and the arms' own effects
/// (`+0x68D`, SetTarget, uncloak, facing), which belong to the unit arm.
#[test]
fn original_unit_fire_update_rows() {
    let payload = unit_fire_payload();
    let defaults = &payload["defaults"];
    let rows = payload["rows"].as_array().unwrap();
    assert_eq!(rows.len(), 64);
    let hm = std::collections::BTreeMap::new();
    let mut compared = 0;
    for row in rows {
        let input = &row["input"];
        let name = input["name"].as_str().unwrap();
        let field = |group: &str, key: &str| -> Value {
            input
                .get(group)
                .and_then(|group| group.get(key))
                .unwrap_or(&defaults[group][key])
                .clone()
        };
        let scalar = |key: &str| input.get(key).unwrap_or(&defaults[key]).clone();
        let code = int(&scalar("code"));
        let deploy_fire = scalar("deploy_fire").as_bool().unwrap();
        let has_target = field("unit", "has_target").as_bool().unwrap();
        let outcome = if !has_target {
            UnitFireOutcome::NoTarget
        } else {
            let Some(code) = fire_error_of(code) else {
                continue;
            };
            if deploy_fire && matches!(code, FireError::Ok | FireError::Facing) {
                continue;
            }
            UnitFireOutcome::Code(code)
        };
        let is_gattling = int(&field("type", "is_gattling")) != 0;
        let slot0 = scalar("slot0").as_bool().unwrap();
        let rules = unit_fire_rules(is_gattling, slot0);
        let mut sim = crate::sim::world::Simulation::new();
        let id = sim
            .spawn_object("GTNK", "Americans", 10, 10, 0, &rules, &hm)
            .expect("spawn GTNK");
        {
            let entity = sim.substrate.entities.get_mut(id).unwrap();
            entity.gattling = GattlingState::from_fields(
                i32_of(&field("unit", "stage")),
                i32_of(&field("unit", "value")),
                int(&field("unit", "latch")) != 0,
            );
            entity.turret_anim_frame = i32_of(&field("unit", "turret_anim"));
            if elite_of(&field("unit", "veterancy")) {
                crate::sim::combat::veterancy::set_elite(entity);
            }
        }
        sim.main_rng = SimRng::new(0x2A61);
        sim.sound_events.clear();
        let mut probe = SimRng::new(0x2A61);
        sim.unit_fire_update_tail(id, outcome, &rules);
        let entity = sim.substrate.entities.get(id).unwrap();
        assert_eq!(
            i64::from(entity.gattling.value()),
            int(&row["value"]),
            "{name}: value"
        );
        assert_eq!(
            i64::from(entity.gattling.stage()),
            int(&row["stage"]),
            "{name}: stage"
        );
        assert_eq!(
            entity.gattling.report_latch(),
            int(&row["latch"]) != 0,
            "{name}: latch"
        );
        assert_eq!(
            i64::from(entity.turret_anim_frame),
            int(&row["turret_anim"]),
            "{name}: +0x148"
        );
        let draws = row["draws"].as_array().unwrap();
        for draw in draws {
            assert_eq!(i64::from(probe.next_u32()), int(draw), "{name}: draw");
        }
        assert_eq!(sim.main_rng.state(), probe.state(), "{name}: draw count");
        let plays: Vec<String> = sim
            .sound_events
            .iter()
            .filter_map(|event| match event {
                crate::sim::world::SimSoundEvent::GattlingLoop { sound_id, .. } => {
                    Some(sim.interner.resolve(*sound_id).to_string())
                }
                _ => None,
            })
            .collect();
        let native_plays: Vec<String> = row["plays"]
            .as_array()
            .unwrap()
            .iter()
            .map(|play| format!("S{}", int(&play["sound"])))
            .collect();
        assert_eq!(plays, native_plays, "{name}: report");
        compared += 1;
    }
    assert_eq!(compared, 54);
}

fn fire_error_of(code: i64) -> Option<FireError> {
    Some(match code {
        0 => FireError::Ok,
        1 => FireError::Ammo,
        2 => FireError::Facing,
        3 => FireError::Rearm,
        4 => FireError::Rotating,
        5 => FireError::Illegal,
        6 => FireError::Cant,
        7 => FireError::Moving,
        8 => FireError::Range,
        9 => FireError::Cloaked,
        10 => FireError::Busy,
        11 => FireError::MustDeploy,
        _ => return None,
    })
}

// ---- production regressions through `advance_tick` ----

/// A Gattling Tank (`GTNK`, the retail `[YTNK]` tables, sight and locomotor)
/// whose stage weapons differ (`G0`..`G5`, each with its own report), a
/// transport it fits and a sturdy target.
fn spin_rules() -> crate::rules::ruleset::RuleSet {
    let mut text = String::from(
        "[VehicleTypes]\n0=GTNK\n1=POST\n2=HOVR\n[InfantryTypes]\n[BuildingTypes]\n\
         [AircraftTypes]\n\
         [GTNK]\nStrength=300\nArmor=light\nSpeed=6\nTurret=yes\nROT=10\nIsGattling=yes\n\
         TurretCount=1\nWeaponCount=6\nWeaponStages=3\nStage1=200\nStage2=400\nStage3=600\n\
         EliteStage1=100\nEliteStage2=200\nEliteStage3=300\nRateUp=1\nRateDown=50\nSize=3\n\
         Sight=10\nLocomotor={4A582741-9839-11d1-B709-00A024DDAFD1}\n",
    );
    for slot in 0..6 {
        text.push_str(&format!("Weapon{}=G{slot}\n", slot + 1));
    }
    text.push_str(
        "[POST]\nStrength=30000\nArmor=heavy\n\
         [HOVR]\nStrength=300\nArmor=light\nSpeed=6\nPassengers=5\nSizeLimit=6\n",
    );
    for slot in 0..6 {
        text.push_str(&format!(
            "[G{slot}]\nDamage=1\nROF=16\nRange=6\nWarhead=WH\nReport=Loop{slot}\n"
        ));
    }
    text.push_str("[WH]\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n");
    crate::rules::ruleset::RuleSet::from_ini(&crate::rules::ini_parser::IniFile::from_str(&text))
        .expect("spin-up rules")
}

/// The tank (YuriCountry, human) on the flat arena, where the ordinary fog
/// update gives it sight, and its orders as the player's commands.
struct Spin {
    sim: crate::sim::world::Simulation,
    rules: crate::rules::ruleset::RuleSet,
    grid: crate::sim::pathfinding::PathGrid,
    hm: std::collections::BTreeMap<(u16, u16), u8>,
    tank: u64,
}

impl Spin {
    /// The tank alone: nothing it could pick a target from.
    fn new() -> Self {
        let rules = spin_rules();
        let hm = std::collections::BTreeMap::new();
        let mut sim = crate::sim::world::Simulation::new();
        for (name, side, human) in [("YuriCountry", 2, true), ("Americans", 0, false)] {
            let id = sim.interner.intern(name);
            sim.houses.insert(
                id,
                crate::sim::house_state::HouseState::new(id, side, None, human, 0, 10),
            );
            sim.session.house_order.push(id);
        }
        let grid = crate::sim::arena_fixture::flat_arena(&mut sim, &rules);
        let tank = sim
            .spawn_object("GTNK", "YuriCountry", 10, 10, 0, &rules, &hm)
            .expect("spawn GTNK");
        Self {
            sim,
            rules,
            grid,
            hm,
            tank,
        }
    }

    /// An enemy post (Americans) three cells east of the tank.
    fn spawn_post(&mut self) -> u64 {
        self.sim
            .spawn_object("POST", "Americans", 13, 10, 0, &self.rules, &self.hm)
            .expect("spawn POST")
    }

    /// A player order and the frame it lands on. EventClass dispatch is the
    /// frame's tail rung, after the object walk and combat, so the tank first
    /// acts on the order the frame after this one.
    fn order(&mut self, command: crate::sim::command::Command) {
        let owner = self.sim.interner.intern("YuriCountry");
        self.sim
            .queue_command(crate::sim::command::CommandEnvelope::new(
                owner,
                self.sim.session.tick + 1,
                command,
            ));
        self.tick();
    }

    fn state(&self) -> GattlingState {
        self.sim.substrate.entities.get(self.tank).unwrap().gattling
    }

    /// One frame: the weapons the tank fired and the gattling sound events.
    fn tick(&mut self) -> (Vec<String>, Vec<crate::sim::world::SimSoundEvent>) {
        self.sim.fire_events.clear();
        self.sim.sound_events.clear();
        let commands = self.sim.take_due_commands();
        self.sim.advance_tick(
            &commands,
            Some(&self.rules),
            &self.hm,
            Some(&self.grid),
            None,
            67,
        );
        let fired = self
            .sim
            .fire_events
            .iter()
            .filter(|event| event.attacker_id == self.tank)
            .map(|event| {
                assert!(
                    event.report_sound_id.is_none(),
                    "a gattling type plays no per-shot report"
                );
                self.sim.interner.resolve(event.weapon_id).to_string()
            })
            .collect();
        let sounds = self
            .sim
            .sound_events
            .iter()
            .filter(|event| {
                matches!(
                    event,
                    crate::sim::world::SimSoundEvent::GattlingLoop { .. }
                        | crate::sim::world::SimSoundEvent::GattlingLoopStop { .. }
                        | crate::sim::world::SimSoundEvent::GattlingLoopRelease { .. }
                )
            })
            .cloned()
            .collect();
        (fired, sounds)
    }
}

/// A Gattling Tank ordered to attack charges one point every frame it fires,
/// reloads or turns, and steps up on the 201st and 401st such frames, as the
/// original does (`gattling_stage.py`: 201/401, cap 600). Each stage fires
/// its own weapon from the next frame on; the loop starts on the first frame
/// (one `g_MainRng` draw) and again at each stage-up, after a hard stop.
#[test]
fn a_gattling_tank_spins_up_while_it_fights() {
    let mut spin = Spin::new();
    let post = spin.spawn_post();
    spin.order(crate::sim::command::Command::Attack {
        attacker_id: spin.tank,
        target_id: post,
    });
    let mut stage_ups = Vec::new();
    let mut weapons_by_stage: std::collections::BTreeMap<i32, std::collections::BTreeSet<String>> =
        Default::default();
    let mut loops = Vec::new();
    let mut previous = spin.state();
    for frame in 0..620 {
        let stage_before = spin.state().stage();
        let (fired, sounds) = spin.tick();
        for weapon in fired {
            weapons_by_stage
                .entry(stage_before)
                .or_default()
                .insert(weapon);
        }
        let now = spin.state();
        assert_eq!(
            now.value(),
            (previous.value() + 1).min(600),
            "frame {frame}: one point a frame up to the cap"
        );
        if now.stage() != previous.stage() {
            stage_ups.push((now.stage(), now.value()));
        }
        for sound in sounds {
            match sound {
                crate::sim::world::SimSoundEvent::GattlingLoop { sound_id, .. } => {
                    loops.push(spin.sim.interner.resolve(sound_id).to_string());
                }
                crate::sim::world::SimSoundEvent::GattlingLoopStop { .. } => {
                    loops.push("stop".to_string());
                }
                _ => panic!("frame {frame}: no release while charging"),
            }
        }
        previous = now;
    }
    assert_eq!(stage_ups, [(1, 201), (2, 401)]);
    assert_eq!(previous.value(), 600);
    let stage_weapons =
        |stage: i32| -> Vec<String> { weapons_by_stage[&stage].iter().cloned().collect() };
    assert_eq!(stage_weapons(0), ["G0"]);
    assert_eq!(stage_weapons(1), ["G2"]);
    assert_eq!(stage_weapons(2), ["G4"]);
    assert_eq!(loops, ["Loop0", "stop", "Loop2", "stop", "Loop4"]);
}

/// Stopped at the cap after firing at the ground, with nothing to pick
/// another target from: from the first frame without its target the tank
/// loses 50 a frame, drops to stage 1 on the 5th frame and to 0 on the 9th,
/// and is empty on the 12th (the original's rookie decay,
/// `gattling_stage.py`); the loop is released once.
#[test]
fn a_gattling_tank_winds_down_without_a_target() {
    use crate::sim::command::Command;
    let mut spin = Spin::new();
    spin.order(Command::ForceAttackCell {
        attacker_id: spin.tank,
        target_rx: 13,
        target_ry: 10,
    });
    for _ in 0..620 {
        spin.tick();
    }
    assert_eq!((spin.state().stage(), spin.state().value()), (2, 600));
    spin.order(Command::Stop {
        entity_id: spin.tank,
    });
    let mut timeline = Vec::new();
    let mut releases = 0;
    for _ in 0..13 {
        let (fired, sounds) = spin.tick();
        assert!(fired.is_empty());
        releases += sounds
            .iter()
            .filter(|sound| {
                matches!(
                    sound,
                    crate::sim::world::SimSoundEvent::GattlingLoopRelease { .. }
                )
            })
            .count();
        timeline.push((spin.state().stage(), spin.state().value()));
    }
    // Frame n is `timeline[n - 1]`.
    assert_eq!((timeline[3].0, timeline[4].0), (2, 1));
    assert_eq!((timeline[7].0, timeline[8].0), (1, 0));
    assert_eq!((timeline[10].1, timeline[11].1), (50, 0));
    assert_eq!(releases, 1);
}

/// A tank boarding a transport enters with its spin reset: `+0xC4`, the
/// value and the stage are zeroed ahead of the captive release
/// (`0x0073A6FC..0x0073A70F`), and Limbo releases its loop.
#[test]
fn a_gattling_tank_boards_with_its_spin_reset() {
    use crate::sim::passenger::{BoardingPhase, PassengerRole};
    let mut spin = Spin::new();
    let hover = spin
        .sim
        .spawn_object("HOVR", "YuriCountry", 11, 10, 0, &spin.rules, &spin.hm)
        .expect("spawn HOVR");
    {
        let tank = spin.sim.substrate.entities.get_mut(spin.tank).unwrap();
        tank.gattling = GattlingState::from_fields(2, 600, true);
        tank.passenger_role = PassengerRole::Boarding {
            target_transport_id: hover,
            phase: BoardingPhase::Entering,
        };
    }
    spin.sim.sound_events.clear();
    crate::sim::passenger::tick_passenger_system(&mut spin.sim, &spin.rules);
    let tank = spin.sim.substrate.entities.get(spin.tank).unwrap();
    assert!(tank.passenger_role.is_inside_transport());
    assert_eq!((tank.gattling.stage(), tank.gattling.value()), (0, 0));
    assert_eq!(tank.mission.ai_counter(), 0);
    assert!(!tank.gattling.report_latch());
    let owner = super::gattling_sound_owner(spin.tank);
    assert!(spin.sim.sound_events.iter().any(|event| matches!(
        event,
        crate::sim::world::SimSoundEvent::GattlingLoopRelease { owner: released }
            if *released == owner
    )));
}

/// Stage and value survive a save and are hashed; the latch does not survive
/// (`TechnoClass::Load` clears it, `0x0070C20E`), so the first charge after a
/// load starts the loop again.
#[test]
fn gattling_state_round_trips_and_is_hashed() {
    let mut spin = Spin::new();
    spin.sim
        .substrate
        .entities
        .get_mut(spin.tank)
        .unwrap()
        .gattling = GattlingState::from_fields(1, 321, true);
    let bytes = crate::sim::snapshot::GameSnapshot::save(&spin.sim, 0, 0, "gattling", 0);
    let mut restored = crate::sim::snapshot::GameSnapshot::load(&bytes)
        .expect("snapshot")
        .sim;
    restored
        .restore_after_snapshot_load()
        .expect("gattling snapshot restores");
    let tank = restored.substrate.entities.get(spin.tank).unwrap();
    assert_eq!((tank.gattling.stage(), tank.gattling.value()), (1, 321));
    assert!(!tank.gattling.report_latch());
    let hashed = restored.state_hash();
    restored
        .substrate
        .entities
        .get_mut(spin.tank)
        .unwrap()
        .gattling = GattlingState::from_fields(1, 322, false);
    assert_ne!(restored.state_hash(), hashed, "the value is hashed");
}
