//! Rust regression tests for `FootClass::Mission_Capture` (`0x004D4B20`).
//! Its delay is the shared rate epilogue, native-compared with the other
//! handlers' (`authority::rate_epilogue`).

use super::*;
use crate::rules::ini_parser::IniFile;
use crate::sim::house_state::HouseState;
use crate::sim::mission::timer::MissionDispatchTimer;
use crate::sim::world::ObjectAiCtx;

const RULES: &str = "\
[InfantryTypes]
0=ENGINEER
1=E1
2=E3
[VehicleTypes]
0=LTNK
[BuildingTypes]
0=GAPOWR
[Guard]
Rate=.030
[Capture]
Rate=.016
[ENGINEER]
Strength=75
Speed=4
Engineer=yes
Locomotor={4A582744-9839-11D1-B709-00A024DDAFD1}
[E1]
Strength=125
Speed=4
Occupier=yes
Locomotor={4A582744-9839-11D1-B709-00A024DDAFD1}
[E3]
Strength=100
Speed=4
Locomotor={4A582744-9839-11D1-B709-00A024DDAFD1}
[LTNK]
Strength=300
Speed=6
Locomotor={4A582741-9839-11D1-B709-00A024DDAFD1}
[GAPOWR]
Strength=750
";

fn rules() -> RuleSet {
    RuleSet::from_ini_with_fixed_art_for_test(
        &crate::sim::arena_fixture::clear_ground_ini(RULES),
        &IniFile::from_str("[GAPOWR]\nFoundation=2x2\n"),
    )
    .expect("capture rules")
}

/// The flat arena with the computer Russians and the human Americans, and
/// a Russian power plant at (16, 10).
fn arena(rules: &RuleSet) -> (Simulation, u64) {
    let mut sim = Simulation::with_seed(0x4D4B20);
    for (name, side, human) in [("Russians", 1, false), ("Americans", 0, true)] {
        let id = sim.interner.intern(name);
        sim.houses
            .insert(id, HouseState::new(id, side, None, human, 0, 10));
        sim.session.house_order.push(id);
    }
    crate::sim::arena_fixture::flat_arena(&mut sim, rules);
    let plant = spawn(&mut sim, rules, "GAPOWR", "Russians", 16, 10);
    (sim, plant)
}

fn spawn(sim: &mut Simulation, rules: &RuleSet, kind: &str, owner: &str, rx: u16, ry: u16) -> u64 {
    sim.spawn_object_at_height(kind, owner, rx, ry, 0, 0, rules)
        .unwrap_or_else(|| panic!("{kind} spawns"))
}

/// `kind` of `owner` on Capture at (10, `ry`).
fn capturer(sim: &mut Simulation, rules: &RuleSet, kind: &str, owner: &str, ry: u16) -> u64 {
    let id = spawn(sim, rules, kind, owner, 10, ry);
    let now = sim.session.binary_frame;
    sim.mission_assign_exact(id, MissionId::from_known(MissionType::Capture), now)
        .unwrap();
    id
}

fn head_for(sim: &mut Simulation, rules: &RuleSet, id: u64, building: u64) {
    sim.assign_destination_represented(
        id,
        Some(NavTargetRef::object(building)),
        Some(rules),
        None,
        FrameEffects::default(),
    )
    .unwrap();
}

fn heading(sim: &Simulation, id: u64) -> Option<u64> {
    match sim.substrate.entities.get(id)?.navigation.nav_com() {
        Some(NavTargetRef::Object { id }) => Some(id),
        _ => None,
    }
}

fn mission(sim: &Simulation, id: u64) -> (MissionId, MissionId) {
    let mission = &sim.substrate.entities.get(id).unwrap().mission;
    (mission.current(), mission.queued())
}

fn capture(sim: &mut Simulation, rules: &RuleSet, id: u64) -> i32 {
    mission_capture(sim, rules, None, id, FrameEffects::default())
}

const CAPTURE: MissionId = MissionId::from_known(MissionType::Capture);
const GUARD: MissionId = MissionId::from_known(MissionType::Guard);

/// A human's engineer on its way, through the dispatcher: the body changes
/// nothing and the timer takes Capture's rate, `ftol(.016 × 900)` = 14
/// frames, plus one `RandomRanged(0, 2)` on the Scenario stream.
#[test]
fn a_capturer_on_its_way_takes_captures_cadence() {
    let rules = rules();
    let (mut sim, plant) = arena(&rules);
    let engineer = capturer(&mut sim, &rules, "ENGINEER", "Americans", 10);
    head_for(&mut sim, &rules, engineer, plant);
    let mut rng = sim.scenario_rng.clone();
    let delay = 14 + rng.next_range_u32_inclusive(0, 2) as i32;
    let now = sim.session.binary_frame;

    sim.object_ai_visit_one(engineer, Some(&rules), ObjectAiCtx::default());

    assert_eq!(heading(&sim, engineer), Some(plant));
    assert_eq!(mission(&sim, engineer), (CAPTURE, MissionId::NONE));
    assert_eq!(
        sim.substrate
            .entities
            .get(engineer)
            .unwrap()
            .mission
            .dispatch_timer(),
        MissionDispatchTimer::from_raw(now as i32, delay)
    );
    assert_eq!(sim.scenario_rng.logical_state(), rng.logical_state());
}

/// Original E1 Capture8 through Mission5B3060 -> Foot4D4B20, with physical
/// RULESMD/LANGRULE/MPBattleMD/XMP03T4 reads. The three native controls include
/// RandomRanged65C7E0 rejection retries and both dispatcher timer writes
/// (5B331B/5B3324). Mission prior and nonnull Building NavCom are supplied;
/// mouse scheduling, native construction and walking are separate coverage.
#[test]
fn retail_occupier_capture_dispatch_matches_native_rng_and_timer() {
    let Some(retail) = crate::rules::retail_ini_fixture::retail_battle_rules_for_map("XMP03T4.MAP")
    else {
        return;
    };
    let native: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/anytown_damage/foot_capture.json",
    ))
    .unwrap();
    let rules = &retail.rules;
    assert!(rules.object("E1").unwrap().occupier);
    let controls = native["controls"].as_array().unwrap();
    assert_eq!(controls.len(), 3);
    for control in controls {
        let name = control["name"].as_str().unwrap();
        let (mut sim, building) = arena(rules);
        let gi = capturer(&mut sim, rules, "E1", "Americans", 10);
        head_for(&mut sim, rules, gi, building);
        sim.session.binary_frame = control["frame"].as_u64().unwrap() as u32;
        let timer_before = control["timer_before"].as_array().unwrap();
        sim.substrate
            .entities
            .get_mut(gi)
            .unwrap()
            .mission
            .apply_test_fixture(crate::sim::mission::state::MissionTestFixture {
                current: MissionId::from_raw(control["mission"].as_i64().unwrap() as i32),
                suspended: MissionId::NONE,
                queued: MissionId::from_raw(control["queued"].as_i64().unwrap() as i32),
                movement_bypass_latch: 0,
                handler_state: 0,
                mission_start_frame: 0,
                ai_counter: 0,
                dispatch_timer: MissionDispatchTimer::from_raw(
                    timer_before[0].as_i64().unwrap() as i32,
                    timer_before[1].as_i64().unwrap() as i32,
                ),
            });
        sim.scenario_rng = crate::sim::rng::SimRng::from_native_state_hex_for_test(
            control["rng_before"]["native_state_hex"].as_str().unwrap(),
        );
        let main_before = sim.main_rng.logical_state();
        let mapgen_before = sim.mapgen_rng.logical_state();
        let (_, draws) = crate::sim::rng::trace_draws(|| {
            crate::sim::world::dispatch_foot_mission(&mut sim, gi, rules, ObjectAiCtx::default())
        });

        assert_eq!(heading(&sim, gi), Some(building), "{name}");
        assert_eq!(mission(&sim, gi), (CAPTURE, MissionId::NONE), "{name}");
        assert!(
            sim.substrate
                .entities
                .get(gi)
                .unwrap()
                .attack_target
                .target()
                .is_none(),
            "{name}"
        );
        let timer_after = control["timer_after"].as_array().unwrap();
        assert_eq!(
            sim.substrate
                .entities
                .get(gi)
                .unwrap()
                .mission
                .dispatch_timer(),
            MissionDispatchTimer::from_raw(
                timer_after[0].as_i64().unwrap() as i32,
                timer_after[1].as_i64().unwrap() as i32,
            ),
            "{name}",
        );
        assert_eq!(
            draws
                .iter()
                .map(|draw| draw["value"].clone())
                .collect::<Vec<_>>(),
            *control["raw_draws"].as_array().unwrap(),
            "{name}: ordered Scenario raw words include rejection retries",
        );
        assert_eq!(
            sim.scenario_rng.native_state_hex(),
            control["rng_after"]["native_state_hex"].as_str().unwrap(),
            "{name}: complete Scenario state",
        );
        assert_eq!(
            sim.main_rng.logical_state(),
            main_before,
            "{name}: Main unchanged"
        );
        assert_eq!(
            sim.mapgen_rng.logical_state(),
            mapgen_before,
            "{name}: MapGen unchanged"
        );
    }
}

/// No destination left: the capturer idles (`0x004D4BD1..0x004D4BD9`),
/// which queues Guard.
#[test]
fn a_capturer_without_a_destination_idles() {
    let rules = rules();
    let (mut sim, _) = arena(&rules);
    let engineer = capturer(&mut sim, &rules, "ENGINEER", "Americans", 10);

    let _ = capture(&mut sim, &rules, engineer);

    assert_eq!(mission(&sim, engineer), (CAPTURE, GUARD));
    assert_eq!(heading(&sim, engineer), None);
}

/// An `Engineer=` or `Occupier=` infantryman holding a building target and
/// no destination takes the target as its destination
/// (`0x004D4B43..0x004D4BC1`) and does not idle; a plain infantryman and a
/// vehicle holding one idle instead.
#[test]
fn a_capturing_infantryman_heads_for_its_building_target() {
    let rules = rules();
    let (mut sim, plant) = arena(&rules);
    let engineer = capturer(&mut sim, &rules, "ENGINEER", "Americans", 10);
    let occupier = capturer(&mut sim, &rules, "E1", "Americans", 12);
    let rifleman = capturer(&mut sim, &rules, "E3", "Americans", 14);
    let tank = capturer(&mut sim, &rules, "LTNK", "Americans", 16);
    let all = [engineer, occupier, rifleman, tank];
    for id in all {
        sim.assign_target_represented(
            id,
            Some(TargetKind::Entity(plant)),
            Some(&rules),
            None,
            crate::sim::world::FrameEffects::default(),
        )
        .unwrap();
    }

    for id in all {
        let _ = capture(&mut sim, &rules, id);
    }

    for id in [engineer, occupier] {
        assert_eq!(heading(&sim, id), Some(plant));
        assert_eq!(mission(&sim, id), (CAPTURE, MissionId::NONE));
    }
    // Neither of the others takes the target as its destination: the tank
    // idles into Guard, the rifleman, an infantryman holding a target, back
    // into Capture (Infantry Enter_Idle_Mode, `0x0051CBCE..0x0051CD9D`).
    assert_eq!(heading(&sim, rifleman), None);
    assert_eq!(mission(&sim, rifleman), (CAPTURE, MissionId::NONE));
    assert_eq!(heading(&sim, tank), None);
    assert_eq!(mission(&sim, tank), (CAPTURE, GUARD));
}

/// Left without a destination on a building's cell, a capturer also
/// scatters off it (vt+0x174(NULL, 1, 0), `0x004D4BDC..0x004D4C0E`), which
/// gives it a cell to walk to; one off the building only idles.
#[test]
fn a_capturer_left_on_a_building_scatters_off_it() {
    let rules = rules();
    let (mut sim, _) = arena(&rules);
    let _hut = spawn(&mut sim, &rules, "GAPOWR", "Russians", 10, 14);
    // Supply the native initial-map infantry prior on the building's cell.
    // ScenarioFullInit686B4F..687C44 holds A8E7AC through InfantryRead51FB00;
    // ordinary runtime building placement correctly rejects occupied ground.
    let on =
        sim.with_object_placement_scope(|sim| capturer(sim, &rules, "ENGINEER", "Americans", 14));
    let off = capturer(&mut sim, &rules, "ENGINEER", "Americans", 18);

    let _ = capture(&mut sim, &rules, on);
    let _ = capture(&mut sim, &rules, off);

    let nav_com = |id| sim.substrate.entities.get(id).unwrap().navigation.nav_com();
    assert!(matches!(nav_com(on), Some(NavTargetRef::Cell { .. })));
    assert_eq!(nav_com(off), None);
    assert_eq!(mission(&sim, off), (CAPTURE, GUARD));
}

/// A computer's capturer with no target drops its destination and queues
/// Hunt (`0x004D4C49..0x004D4C6B`); an occupier keeps on.
#[test]
fn a_computer_capturer_without_a_target_hunts_unless_an_occupier() {
    let rules = rules();
    let (mut sim, plant) = arena(&rules);
    let engineer = capturer(&mut sim, &rules, "ENGINEER", "Russians", 10);
    let occupier = capturer(&mut sim, &rules, "E1", "Russians", 12);
    for id in [engineer, occupier] {
        head_for(&mut sim, &rules, id, plant);
    }

    let _ = capture(&mut sim, &rules, engineer);
    let _ = capture(&mut sim, &rules, occupier);

    assert_eq!(heading(&sim, engineer), None);
    assert_eq!(
        mission(&sim, engineer),
        (CAPTURE, MissionId::from_known(MissionType::Hunt))
    );
    assert_eq!(heading(&sim, occupier), Some(plant));
    assert_eq!(mission(&sim, occupier), (CAPTURE, MissionId::NONE));
}
