//! Rust regression tests for the crew survival owner. Expected draws are
//! replayed on a clone of the Scenario stream in the native order recorded on
//! each producer; they are not native goldens. Mapless fixtures stop Scatter
//! at its FNPC search, after its one `RandomRanged(0, 4)` draw.

use super::*;
use crate::map::entities::EntityCategory;
use crate::rules::ini_parser::IniFile;

use crate::sim::combat::{EntityDamageEvent, RAD_NO_ATTACKER, ReceiverCallFlags, TargetKind};
use crate::sim::house_state::HouseState;
use crate::sim::passenger::{PassengerCargo, PassengerRole};
use crate::sim::rng::SimRng;

const RULES: &str = "\
[General]
AlliedCrew=E1
SovietCrew=E2
ThirdCrew=INIT
Technician=CTECH
Engineer=ENGINEER
AlliedSurvivorDivisor=500
SovietSurvivorDivisor=250
ThirdSurvivorDivisor=750
CrewEscape=50%
RefundPercent=50%
[InfantryTypes]
0=E1
1=E2
2=INIT
3=CTECH
4=ENGINEER
[VehicleTypes]
0=AMCV
1=IFV
2=APC
3=BOOMV
4=GUNV
[AircraftTypes]
[BuildingTypes]
0=GAPOWR
1=GACNST
2=YAPOWR
3=GAREFN
4=GASAND
5=REDLAMP
[Warheads]
0=KILLWH
1=BLASTWH
[E1]
Strength=125
Speed=4
Locomotor={4A582744-9839-11D1-B709-00A024DDAFD1}
[E2]
Strength=125
Speed=4
Locomotor={4A582744-9839-11D1-B709-00A024DDAFD1}
[INIT]
Strength=100
Speed=4
Locomotor={4A582744-9839-11D1-B709-00A024DDAFD1}
[CTECH]
Strength=50
Speed=4
Locomotor={4A582744-9839-11D1-B709-00A024DDAFD1}
[ENGINEER]
Strength=75
Speed=4
Locomotor={4A582744-9839-11D1-B709-00A024DDAFD1}
[AMCV]
Strength=1000
Cost=3000
Crewed=yes
[IFV]
Strength=200
Cost=600
Crewed=yes
Passengers=1
[APC]
Strength=200
Cost=900
Passengers=5
[BOOMV]
Strength=200
Cost=900
Passengers=2
Explodes=yes
[GUNV]
Strength=200
Cost=600
Passengers=1
Gunner=yes
TurretCount=8
WeaponCount=8
Weapon1=GUNGUN
Weapon3=GUNGUN
Weapon8=SUICIDEBLAST
DeathWeapon=SUICIDEBLAST
[GUNGUN]
Damage=10
Warhead=KILLWH
[SUICIDEBLAST]
Damage=50
Warhead=BLASTWH
Suicide=yes
[BLASTWH]
CellSpread=2
Verses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%
[GAPOWR]
Strength=750
Cost=800
Crewed=yes
[GACNST]
Strength=1000
Cost=3000
Crewed=yes
Factory=BuildingType
[YAPOWR]
Strength=750
Cost=600
Crewed=yes
InfantryAbsorb=yes
Passengers=5
[GAREFN]
Strength=900
Cost=2000
Soylent=300
Crewed=yes
[GASAND]
Strength=100
Cost=100
[REDLAMP]
Strength=100
Cost=0
Crewed=yes
[KILLWH]
Verses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%
";

const ART: &str = "[GAPOWR]\nFoundation=2x2\n[GACNST]\nFoundation=3x3\n\
    [YAPOWR]\nFoundation=2x2\n[GAREFN]\nFoundation=4x3\n\
    [GASAND]\nFoundation=1x1\n[REDLAMP]\nFoundation=1x1\n";

fn rules() -> RuleSet {
    RuleSet::from_ini_with_fixed_art_for_test(&IniFile::from_str(RULES), &IniFile::from_str(ART))
        .expect("crew survival rules")
}

/// Allied, Soviet and Yuri houses, computer and human, plus a Neutral one.
fn sim_with_houses(seed: u64) -> Simulation {
    let mut sim = Simulation::with_seed(seed);
    for (name, side, human) in [
        ("Americans", 0, false),
        ("AlliedHuman", 0, true),
        ("Russians", 1, false),
        ("SovietHuman", 1, true),
        ("YuriCountry", 2, false),
        ("YuriHuman", 2, true),
        ("Neutral", 3, false),
    ] {
        let id = sim.interner.intern(name);
        sim.houses
            .register(HouseState::new(id, side, None, human, 0, 10));
    }
    sim
}

fn spawn(sim: &mut Simulation, rules: &RuleSet, kind: &str, owner: &str, rx: u16, ry: u16) -> u64 {
    sim.spawn_object_at_height(kind, owner, rx, ry, 0, 0, rules)
        .unwrap_or_else(|| panic!("{kind} spawns"))
}

fn new_ids(sim: &Simulation, before: &[u64]) -> Vec<u64> {
    sim.substrate
        .entities
        .keys_sorted()
        .into_iter()
        .filter(|id| !before.contains(id))
        .collect()
}

#[test]
fn nominal_type_does_not_mark_ordinary_infantry_and_marker_changes_hash() {
    let Some(rules) = crate::rules::retail_ini_fixture::retail_unlayered_rules() else {
        return;
    };
    assert!(
        rules.object("CTECH").unwrap().nominal(),
        "physical retail Nominal input"
    );
    let mut sim = sim_with_houses(31);
    let house = sim.interner.intern("Americans");
    let id = spawn(&mut sim, &rules, "CTECH", "Americans", 10, 10);
    assert!(
        !sim.substrate
            .entities
            .get(id)
            .unwrap()
            .is_nominal_survivor()
    );
    let counts = {
        let tracking = sim.houses[&house].tracking();
        (tracking.active_for_test(), tracking.infantry())
    };
    let before = sim.state_hash();
    sim.substrate
        .entities
        .get_mut(id)
        .unwrap()
        .mark_nominal_survivor();
    assert_ne!(
        sim.state_hash(),
        before,
        "retained marker affects future admission/name"
    );
    let tracking = sim.houses[&house].tracking();
    assert_eq!(
        (tracking.active_for_test(), tracking.infantry()),
        counts,
        "marking does not retroactively change either counter family"
    );
}

/// Real producer timing and active-count goldens, not a replay of Rust's
/// crew selection/placement algorithm. Failed placement has separate controls.
#[test]
fn nominal_building_crew_matches_executed_first_admission() {
    let corpus = crate::test_fixture::json("tools/spatial_oracle/nominal_building_survivor.json");
    for row in corpus["producer_rows"].as_array().unwrap() {
        let input = &row["input"];
        if input["unlimbo_admitted"] != true {
            continue;
        }
        let text = RULES.replace("AlliedCrew=E1", "AlliedCrew=CTECH").replace(
            "[CTECH]\n",
            &format!("[CTECH]\nNominal={}\n", input["type_nominal"]),
        );
        let mut rules = RuleSet::from_ini_with_fixed_art_for_test(
            &IniFile::from_str(&text),
            &IniFile::from_str(ART),
        )
        .unwrap();
        if input["buildup"] == true {
            rules.set_buildup_control_for_test("GAPOWR", [0, 4, 1]);
        }
        let mut sim = sim_with_houses(31);
        let house = sim.interner.intern("Americans");
        let building = spawn(&mut sim, &rules, "GAPOWR", "Americans", 10, 10);
        let before = sim.substrate.entities.keys_sorted();
        let admitted = match input["route"].as_str().unwrap() {
            "death" => sim.spawn_building_survivor(
                &rules,
                None,
                "CTECH",
                building,
                (10, 10),
                None,
                Default::default(),
            ),
            "sale" => {
                sim.spawn_sale_crew(&rules, None, building, 1, &[(10, 10)], Default::default())
            }
            route => panic!("unknown native producer {route}"),
        };
        assert!(admitted, "{input}");
        let ids = new_ids(&sim, &before);
        assert_eq!(ids.len(), 1, "{input}");
        let survivor = sim.substrate.entities.get(ids[0]).unwrap();
        assert_eq!(
            survivor.is_nominal_survivor(),
            row["after"]["nominal"].as_u64().unwrap() != 0,
            "{input}"
        );
        let tracking = sim.houses[&house].tracking();
        assert_eq!(
            i64::from(tracking.active_count(EntityCategory::Infantry, survivor.type_ref())),
            row["after"]["houses"]["old"]["slot"].as_i64().unwrap(),
            "{input}"
        );
        assert_eq!(
            tracking.infantry(),
            1,
            "constructor tracking remains independent"
        );
    }
}

#[test]
fn nominal_survivor_capture_and_snapshot_retain_counter_drift() {
    use crate::sim::snapshot::GameSnapshot;
    let text = RULES.replace("[CTECH]\n", "[CTECH]\nNominal=yes\n");
    let mut rules = RuleSet::from_ini_with_fixed_art_for_test(
        &IniFile::from_str(&text),
        &IniFile::from_str(ART),
    )
    .unwrap();
    rules.set_buildup_control_for_test("GAPOWR", [0, 4, 1]);
    let mut sim = sim_with_houses(31);
    let first = sim.interner.intern("Americans");
    let second = sim.interner.intern("Russians");
    let building = spawn(&mut sim, &rules, "GAPOWR", "Americans", 10, 10);
    let before = sim.substrate.entities.keys_sorted();
    assert!(sim.spawn_building_survivor(
        &rules,
        None,
        "CTECH",
        building,
        (10, 10),
        None,
        Default::default()
    ));
    let id = new_ids(&sim, &before)[0];
    sim.change_owner(id, second);
    assert_eq!(sim.houses[&first].tracking().active_for_test().1, -1);
    assert_eq!(sim.houses[&second].tracking().active_for_test().1, 0);
    assert_eq!(sim.houses[&first].tracking().infantry(), 0);
    assert_eq!(sim.houses[&second].tracking().infantry(), 1);

    // The existing decoder resets Scenario RNG; compare the same prior.
    sim.scenario_rng = SimRng::new(0);
    let expected = sim.state_hash();
    let bytes = GameSnapshot::save(&sim, 0, 0, "nominal-survivor", 0);
    let mut restored = GameSnapshot::load(&bytes).unwrap().sim;
    restored.restore_after_snapshot_load().unwrap();
    assert_eq!(restored.state_hash(), expected);
    for candidate in [&mut sim, &mut restored] {
        assert!(
            candidate
                .substrate
                .entities
                .get(id)
                .unwrap()
                .is_nominal_survivor()
        );
        candidate.uninit_with_rules(id, &rules);
        assert_eq!(candidate.houses[&second].tracking().active_for_test().1, -1);
        candidate.flush_pending_delete();
        assert_eq!(candidate.houses[&second].tracking().infantry(), 0);
    }
    assert_eq!(sim.state_hash(), restored.state_hash());
}

/// The saved original control executes the building caller's481180 before
/// priority Infantry51DFF0. Migration must retain the first row draw and avoid
/// a second one, while using the shared class placement owner.
#[test]
fn building_crew_keeps_distinct_native_caller_placement_and_rng() {
    let corpus: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
        "src/sim/world/world_spawn/fixtures/infantry_unlimbo_placement_native.json",
    ))
    .unwrap();
    let row = corpus["rows"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| {
            row["input"]["name"].as_str() == Some("building_caller_place_then_priority_class")
        })
        .unwrap();
    let rules = rules();
    let mut sim = sim_with_houses(31);
    let owner = sim.interner.intern("Americans");
    let id = sim
        .construct_crew_limbo(&rules, "E1", owner, (15, 15), 0, Default::default())
        .unwrap();
    sim.scenario_rng = SimRng::from_native_state_hex_for_test(row["rng_before"].as_str().unwrap());
    let unlimbo = CrewUnlimbo::Place {
        cell: (15, 15),
        z: 0,
        raw_z: Some(0),
        request: (SimFixed::from_num(128), SimFixed::from_num(164)),
        caller_places_first: true,
    };
    assert!(sim.with_object_placement_scope(|sim| sim.unlimbo_crew(
        &rules,
        id,
        unlimbo,
        None,
        None,
        crate::sim::world::FrameEffects::default()
    )));
    let entity = sim.substrate.entities.get(id).unwrap();
    let foot = row["events"]
        .as_array()
        .unwrap()
        .iter()
        .find(|event| event[0].as_str() == Some("foot_unlimbo"))
        .unwrap();
    assert_eq!(
        serde_json::json!([
            i32::from(entity.position.rx()) * 256 + entity.position.sub_x().to_num::<i32>(),
            i32::from(entity.position.ry()) * 256 + entity.position.sub_y().to_num::<i32>(),
            0,
        ]),
        foot[2]
    );
    assert_eq!(
        sim.scenario_rng.native_state_hex(),
        row["rng_after"].as_str().unwrap()
    );
    assert!(!sim.object_placement_scope_active());
    assert_eq!(entity.sub_cell(), Some(4));
}

fn type_name(sim: &Simulation, id: u64) -> String {
    let entity = sim.substrate.entities.get(id).expect("entity");
    sim.interner.resolve(entity.type_ref()).to_string()
}

fn queued(sim: &Simulation, id: u64) -> MissionId {
    sim.substrate
        .entities
        .get(id)
        .expect("entity")
        .mission
        .queued()
}

/// The executed gate (`0x00738218..0x0073822D`) at the boundary draws of the
/// stock 50%, of 25% and of NaN: under chop `0x3FFFFFFF` escapes at 50%, and
/// an unordered compare lets every roll escape.
#[test]
fn crew_escape_takes_the_native_gate_draws() {
    for (band, roll, escapes) in crate::util::native_random::native_gate_rows("crew") {
        assert_eq!(
            crew_escapes(roll, NativeF64Bits::from_bits(band.to_bits())),
            escapes,
            "band {band:e}, roll {roll:#x}"
        );
    }
}

/// How_Many_Survivors over GetRefund: Soylent replaces the cost and skips
/// the human half, a zero refund still clamps to one, captured doubles the
/// divisor, and a house outside the three sides owes none.
#[test]
fn survivor_count_divides_the_refund_by_the_side_divisor() {
    let rules = rules();
    let mut sim = sim_with_houses(1);
    let cases: &[(&str, &str, bool, i32)] = &[
        ("GAPOWR", "Americans", false, 1),
        ("GAPOWR", "AlliedHuman", false, 1),
        ("GAPOWR", "Russians", false, 3),
        ("GAPOWR", "SovietHuman", false, 1),
        ("GAPOWR", "Neutral", false, 0),
        ("GACNST", "Americans", false, 5),
        ("GACNST", "AlliedHuman", false, 3),
        ("GACNST", "Russians", false, 5),
        ("GACNST", "YuriCountry", false, 4),
        ("GACNST", "YuriHuman", false, 2),
        ("GACNST", "Americans", true, 3),
        ("GACNST", "AlliedHuman", true, 1),
        ("GAREFN", "Americans", false, 1),
        ("GAREFN", "Russians", false, 1),
        ("GAREFN", "SovietHuman", false, 1),
        ("REDLAMP", "Americans", false, 1),
        ("GASAND", "Americans", false, 0),
    ];
    for (index, &(kind, owner, captured, expected)) in cases.iter().enumerate() {
        let id = spawn(&mut sim, &rules, kind, owner, 2 + 6 * index as u16, 10);
        sim.substrate
            .entities
            .get_mut(id)
            .unwrap()
            .has_been_captured
            .set_for_test(captured);
        assert_eq!(
            sim.building_survivor_count(&rules, id),
            expected,
            "{kind} owned by {owner}, captured {captured}"
        );
        sim.substrate
            .entities
            .get_mut(id)
            .unwrap()
            .no_survivor
            .set_for_test(true);
        assert_eq!(sim.building_survivor_count(&rules, id), 0, "NoSurvivor");
    }
}

/// `BuildingClass::ChangeOwner @ 0x00448723` marks the transferred building,
/// and its repair stops without a sound (`0x00448CE8`).
#[test]
fn a_captured_building_remembers_it() {
    let rules = rules();
    let mut sim = sim_with_houses(1);
    let id = spawn(&mut sim, &rules, "GAPOWR", "Americans", 10, 10);
    assert!(
        !sim.substrate
            .entities
            .get(id)
            .unwrap()
            .has_been_captured
            .is_set()
    );
    sim.substrate
        .entities
        .get_mut(id)
        .unwrap()
        .repairing
        .set_for_test(true);
    sim.sound_events.clear_for_test();
    let russians = sim.interner.intern("Russians");
    sim.change_owner_with_rules(
        id,
        russians,
        &rules,
        None,
        crate::sim::world::FrameEffects::default(),
    );
    let building = sim.substrate.entities.get(id).unwrap();
    assert!(building.has_been_captured.is_set());
    assert!(!building.repairing.is_on());
    assert!(
        !sim.sound_events
            .iter()
            .any(|event| matches!(event, crate::sim::world::SimSoundEvent::VocAt { .. })),
        "no ToggleRepair"
    );
}

/// Phase B on an uncaptured ConYard: per cell, the survivor roll (while any
/// is owed), then that cell's mark. A hit draws the Engineer roll, the
/// constructor word, the centre-row placement draw, the health roll and
/// Scatter's first draw, in that order.
#[test]
fn phase_b_interleaves_each_cells_roll_with_its_mark() {
    let rules = rules();
    let mut hits = 0;
    for seed in 1..=6 {
        let mut sim = sim_with_houses(seed);
        let yard = spawn(&mut sim, &rules, "GACNST", "Americans", 10, 10);
        let before = sim.substrate.entities.keys_sorted();
        let mut replay: SimRng = sim.scenario_rng.clone();
        let mut marks = Vec::new();
        sim.spawn_building_survivors(
            &rules,
            None,
            yard,
            |world, cell| {
                marks.push((cell, world.scenario_rng.state()));
            },
            crate::sim::world::FrameEffects::default(),
        );

        let mut expected_marks = Vec::new();
        let mut expected = Vec::new();
        let mut owed = 5;
        for cell in foundation_cells(10, 10, "3x3") {
            if owed > 0 && replay.next_range_u32_inclusive(0, 2) == 1 {
                let engineer = replay.next_range_u32_inclusive(0, 99) < 25;
                let _constructor = replay.next_u32();
                let _row = replay.next_range_u32(4);
                let strength = if engineer { 75 } else { 125 };
                let health = replay.next_range_i32_inclusive(5, strength);
                let _scatter = replay.next_range_u32_inclusive(0, 4);
                expected.push((if engineer { "ENGINEER" } else { "E1" }, cell, health));
                owed -= 1;
            }
            expected_marks.push((cell, replay.state()));
        }
        assert_eq!(marks, expected_marks, "seed {seed}");

        let survivors: Vec<_> = new_ids(&sim, &before)
            .into_iter()
            .map(|id| {
                let entity = sim.substrate.entities.get(id).unwrap();
                assert_eq!(sim.interner.resolve(entity.owner()), "Americans");
                assert_eq!(queued(&sim, id), MissionId::from_known(MissionType::Hunt));
                assert!(entity.sub_cell().is_some() && !entity.lifecycle.in_limbo());
                (
                    type_name(&sim, id),
                    (entity.position.rx(), entity.position.ry()),
                    entity.health.current(),
                )
            })
            .collect();
        let expected: Vec<_> = expected
            .into_iter()
            .map(|(kind, cell, health)| (kind.to_string(), cell, health))
            .collect();
        assert_eq!(survivors, expected, "seed {seed}");
        hits += survivors.len();
    }
    assert!(hits > 0, "the seeds exercise at least one survivor");
}

/// A captured building skips the Engineer roll and widens the survivor roll
/// to `RandomRanged(0, 8)`.
#[test]
fn a_captured_yard_rolls_one_in_nine_without_the_engineer_draw() {
    let rules = rules();
    let mut sim = sim_with_houses(3);
    let yard = spawn(&mut sim, &rules, "GACNST", "Americans", 10, 10);
    sim.substrate
        .entities
        .get_mut(yard)
        .unwrap()
        .has_been_captured
        .set_for_test(true);
    let mut replay = sim.scenario_rng.clone();
    let mut marks = Vec::new();
    sim.spawn_building_survivors(
        &rules,
        None,
        yard,
        |world, cell| {
            marks.push((cell, world.scenario_rng.state()));
        },
        crate::sim::world::FrameEffects::default(),
    );
    let mut expected_marks = Vec::new();
    let mut owed = 3;
    for cell in foundation_cells(10, 10, "3x3") {
        if owed > 0 && replay.next_range_u32_inclusive(0, 8) == 1 {
            let _constructor = replay.next_u32();
            let _row = replay.next_range_u32(4);
            let _health = replay.next_range_i32_inclusive(5, 125);
            let _scatter = replay.next_range_u32_inclusive(0, 4);
            owed -= 1;
        }
        expected_marks.push((cell, replay.state()));
    }
    assert_eq!(marks, expected_marks);
}

/// Original44307A..443080 suppresses each survivor roll while House1F6
/// is pending, but the443292 suffix still visits every foundation cell.
#[test]
fn pending_house_death_skips_crew_rolls_but_retains_every_cell_mark() {
    let rules = rules();
    let corpus: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/pending_house_death.json",
    ))
    .unwrap();
    for row in corpus["crew"].as_array().unwrap() {
        let pending = row["pending"].as_bool().unwrap();
        let mut sim = sim_with_houses(5);
        let yard = spawn(&mut sim, &rules, "GACNST", "Americans", 10, 10);
        let owner = sim.interner.get("Americans").unwrap();
        let before_ids = sim.substrate.entities.keys_sorted();
        if pending {
            assert!(sim.flag_house_to_die(owner));
        }
        sim.scenario_rng =
            SimRng::from_native_state_hex_for_test(row["rng_before"].as_str().unwrap());
        let expected = SimRng::from_native_state_hex_for_test(row["rng_after"].as_str().unwrap())
            .logical_state();
        let mut marks = Vec::new();
        sim.spawn_building_survivors(
            &rules,
            None,
            yard,
            |world, cell| {
                if pending || marks.is_empty() {
                    // The native unflagged first roll is2, so it reaches the mark
                    // without a crew constructor; the pending case skips every roll.
                    assert_eq!(world.scenario_rng.logical_state(), expected);
                }
                marks.push(cell);
            },
            FrameEffects::default(),
        );
        assert_eq!(marks, foundation_cells(10, 10, "3x3"));
        if pending {
            assert_eq!(sim.substrate.entities.keys_sorted(), before_ids);
            assert_eq!(sim.scenario_rng.logical_state(), expected);
            assert!(
                sim.houses[&owner].is_to_die(),
                "crew reader does not consume House latch"
            );
        } else {
            assert_ne!(row["rng_before"], row["rng_after"]);
        }
    }
}

/// No survivor owed means no Phase B at all: no roll and no per-cell mark.
#[test]
fn a_building_owing_no_survivor_draws_and_marks_nothing() {
    let rules = rules();
    let mut sim = sim_with_houses(5);
    let neutral = spawn(&mut sim, &rules, "GAPOWR", "Neutral", 10, 10);
    let uncrewed = spawn(&mut sim, &rules, "GASAND", "Americans", 20, 10);
    let allied = spawn(&mut sim, &rules, "GAPOWR", "Americans", 30, 10);
    for (id, no_survivor) in [(neutral, false), (uncrewed, false), (allied, true)] {
        sim.substrate
            .entities
            .get_mut(id)
            .unwrap()
            .no_survivor
            .set_for_test(no_survivor);
        let before = sim.scenario_rng.state();
        sim.spawn_building_survivors(
            &rules,
            None,
            id,
            |_, cell| panic!("no mark expected at {cell:?}"),
            crate::sim::world::FrameEffects::default(),
        );
        assert_eq!(sim.scenario_rng.state(), before);
    }
}

/// A building carrying a C4 charge rolls one in two, and its crew attacks an
/// enemy planter; an allied planter only changes the odds.
#[test]
fn survivors_of_a_c4_charged_building_attack_an_enemy_planter() {
    let rules = rules();
    for (planter_owner, expect_attack) in [("Russians", true), ("Americans", false)] {
        let mut attacked = 0;
        for seed in 1..=8 {
            let mut sim = sim_with_houses(seed);
            let planter = spawn(&mut sim, &rules, "E2", planter_owner, 30, 30);
            let plant = spawn(&mut sim, &rules, "GACNST", "Americans", 10, 10);
            sim.substrate
                .entities
                .get_mut(plant)
                .unwrap()
                .pending_c4_detonation
                .set_for_test(Some(crate::sim::c4_plant::PendingC4Detonation::new(
                    crate::sim::timer::CdTimer::started(0, 100),
                    Some(planter),
                )));
            let before = sim.substrate.entities.keys_sorted();
            let mut replay = sim.scenario_rng.clone();
            let mut marks = Vec::new();
            sim.spawn_building_survivors(
                &rules,
                None,
                plant,
                |world, cell| {
                    marks.push((cell, world.scenario_rng.state()));
                },
                crate::sim::world::FrameEffects::default(),
            );
            let mut expected_marks = Vec::new();
            let mut owed = 5;
            for cell in foundation_cells(10, 10, "3x3") {
                if owed > 0 && replay.next_range_u32_inclusive(0, 1) == 1 {
                    let engineer = replay.next_range_u32_inclusive(0, 99) < 25;
                    let _constructor = replay.next_u32();
                    let _row = replay.next_range_u32(4);
                    let _health =
                        replay.next_range_i32_inclusive(5, if engineer { 75 } else { 125 });
                    let _scatter = replay.next_range_u32_inclusive(0, 4);
                    owed -= 1;
                }
                expected_marks.push((cell, replay.state()));
            }
            assert_eq!(marks, expected_marks, "seed {seed}");
            for id in new_ids(&sim, &before) {
                let survivor = sim.substrate.entities.get(id).unwrap();
                let target = survivor.attack_target.target();
                if expect_attack {
                    assert_eq!(queued(&sim, id), MissionId::from_known(MissionType::Attack));
                    assert_eq!(target, Some(TargetKind::Entity(planter)));
                    attacked += 1;
                } else {
                    assert_eq!(queued(&sim, id), MissionId::from_known(MissionType::Hunt));
                    assert_eq!(target, None);
                }
            }
        }
        if expect_attack {
            assert!(attacked > 0, "the seeds exercise the Attack arm");
        }
    }
}

/// A human owner's survivors Move; the computer's Hunt.
#[test]
fn a_human_owners_survivors_move() {
    let rules = rules();
    let mut moved = 0;
    for seed in 1..=6 {
        let mut sim = sim_with_houses(seed);
        let plant = spawn(&mut sim, &rules, "GACNST", "AlliedHuman", 10, 10);
        let before = sim.substrate.entities.keys_sorted();
        sim.spawn_building_survivors(
            &rules,
            None,
            plant,
            |_, _| {},
            crate::sim::world::FrameEffects::default(),
        );
        for id in new_ids(&sim, &before) {
            assert_eq!(queued(&sim, id), MissionId::from_known(MissionType::Move));
            moved += 1;
        }
    }
    assert!(moved > 0);
}

fn load_absorbed(sim: &mut Simulation, rules: &RuleSet, building: u64, count: usize) -> Vec<u64> {
    sim.substrate
        .entities
        .get_mut(building)
        .unwrap()
        .passenger_role = PassengerRole::transport(PassengerCargo::new(5, 0.0));
    (0..count)
        .map(|_| {
            let id = sim
                .construct_object_limbo_at_height(
                    "E1",
                    "Americans",
                    10,
                    10,
                    0,
                    0,
                    rules,
                    crate::sim::world::FrameEffects::default(),
                )
                .expect("absorbed infantry");
            sim.substrate.entities.get_mut(id).unwrap().passenger_role =
                PassengerRole::inside_for_test(building, false);
            let cargo = sim
                .substrate
                .entities
                .get_mut(building)
                .unwrap()
                .passenger_role
                .cargo_mut()
                .unwrap();
            cargo.board_forced_chain(&[(id, 1.0)]);
            id
        })
        .collect()
}

/// Phase A: each absorbed infantryman spends a placement draw on the next
/// foundation cell, leaves at the building's own cell and Scatters; Phase B
/// then continues from the cell after the last passenger's.
#[test]
fn a_bio_reactor_releases_its_absorbed_infantry_before_its_crew() {
    let rules = rules();
    let mut sim = sim_with_houses(9);
    let reactor = spawn(&mut sim, &rules, "YAPOWR", "Americans", 10, 10);
    let absorbed = load_absorbed(&mut sim, &rules, reactor, 2);
    let mut replay = sim.scenario_rng.clone();
    let mut marks = Vec::new();
    sim.spawn_building_survivors(
        &rules,
        None,
        reactor,
        |world, cell| {
            marks.push((cell, world.scenario_rng.state()));
        },
        crate::sim::world::FrameEffects::default(),
    );

    for _ in &absorbed {
        let _overwritten_row = replay.next_range_u32(4);
        let _scatter = replay.next_range_u32_inclusive(0, 4);
    }
    let mut expected_marks = Vec::new();
    let mut owed = 1;
    for cell in foundation_cells(10, 10, "2x2")
        .into_iter()
        .skip(absorbed.len())
    {
        if owed > 0 && replay.next_range_u32_inclusive(0, 2) == 1 {
            let _engineer = replay.next_range_u32_inclusive(0, 99);
            let _constructor = replay.next_u32();
            let _row = replay.next_range_u32(4);
            let _health = replay.next_range_i32_inclusive(5, 125);
            let _scatter = replay.next_range_u32_inclusive(0, 4);
            owed -= 1;
        }
        expected_marks.push((cell, replay.state()));
    }
    assert_eq!(marks, expected_marks);

    let reactor_entity = sim.substrate.entities.get(reactor).unwrap();
    assert!(reactor_entity.passenger_role.cargo().unwrap().is_empty());
    for id in absorbed {
        let passenger = sim.substrate.entities.get(id).expect("released alive");
        assert!(passenger.passenger_role.is_none());
        assert!(!passenger.lifecycle.in_limbo());
        assert_eq!((passenger.position.rx(), passenger.position.ry()), (10, 10));
        assert_eq!(queued(&sim, id), MissionId::from_known(MissionType::Hunt));
    }
}

/// NoSurvivor refuses the absorbed passengers' Unlimbo: each is recorded as
/// killed by the building's C4 planter (SpawnSurvivors' argument,
/// `0x00442FE5`) and removed, though each infantryman's placement draw is
/// still spent.
#[test]
fn no_survivor_uninits_the_absorbed_infantry() {
    let rules = rules();
    let mut sim = sim_with_houses(9);
    let planter = spawn(&mut sim, &rules, "E2", "Russians", 30, 30);
    let reactor = spawn(&mut sim, &rules, "YAPOWR", "Americans", 10, 10);
    let absorbed = load_absorbed(&mut sim, &rules, reactor, 2);
    let mut replay = sim.scenario_rng.clone();
    let reactor_entity = sim.substrate.entities.get_mut(reactor).unwrap();
    reactor_entity.no_survivor.set_for_test(true);
    reactor_entity.pending_c4_detonation.set_for_test(Some(
        crate::sim::c4_plant::PendingC4Detonation::new(
            crate::sim::timer::CdTimer::started(0, 100),
            Some(planter),
        ),
    ));
    let stats = |sim: &Simulation, house: &str| {
        let id = sim.interner.get(house).unwrap();
        *sim.houses[&id].stats()
    };
    let (lost, killed) = (
        stats(&sim, "Americans").units_lost(),
        stats(&sim, "Russians").units_killed(),
    );
    sim.clear_lifecycle_test_events_for_test();
    sim.spawn_building_survivors(
        &rules,
        None,
        reactor,
        |_, cell| panic!("no mark expected at {cell:?}"),
        crate::sim::world::FrameEffects::default(),
    );
    for _ in &absorbed {
        let _overwritten_row = replay.next_range_u32(4);
    }
    assert_eq!(sim.scenario_rng.state(), replay.state());
    for &id in &absorbed {
        assert!(
            sim.substrate
                .entities
                .get(id)
                .is_none_or(|entity| !entity.lifecycle.object_alive()),
            "absorbed passenger {id} is removed"
        );
        assert!(
            sim.lifecycle_test_events_for_test()
                .iter()
                .any(|event| matches!(
                    event,
                    crate::sim::world::LifecycleTestEvent::RecordKillBookkeeping { stable_id }
                        if *stable_id == id
                )),
            "absorbed passenger {id} has its kill recorded"
        );
    }
    assert_eq!(stats(&sim, "Americans").units_lost(), lost + 2);
    assert_eq!(stats(&sim, "Russians").units_killed(), killed + 2);
}

/// The vehicle crew block: the CrewEscape draw, then for an escape the
/// constructor word, the centre-row placement at the vehicle's own spot, the
/// half-Strength health roll and Scatter's first draw.
#[test]
fn a_crewed_vehicle_rolls_crew_escape_then_places_its_crewman() {
    let rules = rules();
    let mut outcomes = [0; 2];
    for seed in 1..=8 {
        for (owner, mission) in [
            ("Americans", MissionType::Hunt),
            ("AlliedHuman", MissionType::Guard),
        ] {
            let mut sim = sim_with_houses(seed);
            let mcv = spawn(&mut sim, &rules, "AMCV", owner, 10, 10);
            sim.foot_mark_remove(
                mcv,
                &rules,
                None,
                crate::sim::world::FrameEffects::default(),
            );
            let before = sim.substrate.entities.keys_sorted();
            let mut replay = sim.scenario_rng.clone();
            sim.spawn_vehicle_crew(
                &rules,
                None,
                mcv,
                false,
                false,
                crate::sim::world::FrameEffects::default(),
            );

            let escaped = replay.next_range_u32_inclusive(0, 0x7fff_fffe) < 0x4000_0000;
            let health = escaped.then(|| {
                let _constructor = replay.next_u32();
                let _row = replay.next_range_u32(4);
                let health = replay.next_range_i32_inclusive(5, 62);
                let _scatter = replay.next_range_u32_inclusive(0, 4);
                health
            });
            assert_eq!(sim.scenario_rng.state(), replay.state(), "seed {seed}");
            let crew = new_ids(&sim, &before);
            match health {
                Some(health) => {
                    assert_eq!(crew.len(), 1);
                    let crewman = sim.substrate.entities.get(crew[0]).unwrap();
                    assert_eq!(type_name(&sim, crew[0]), "E1");
                    assert_eq!(crewman.health.current(), health);
                    assert_eq!((crewman.position.rx(), crewman.position.ry()), (10, 10));
                    // Unlimbo commits Guard (`0x006F6E2A`); the crew Queue
                    // then queues Hunt, or leaves Guard alone
                    // (`Queue_Mission 0x005B3601..0x005B3612`).
                    let crew_mission = &sim.substrate.entities.get(crew[0]).unwrap().mission;
                    assert_eq!(
                        crew_mission.current(),
                        MissionId::from_known(MissionType::Guard)
                    );
                    let expected_queue = if mission == MissionType::Guard {
                        MissionId::NONE
                    } else {
                        MissionId::from_known(mission)
                    };
                    assert_eq!(crew_mission.queued(), expected_queue);
                }
                None => assert!(crew.is_empty()),
            }
            outcomes[usize::from(escaped)] += 1;
        }
    }
    assert!(outcomes[0] > 0 && outcomes[1] > 0, "{outcomes:?}");
}

/// A vehicle on a bridge deck stands above the floor, so its crewman skips
/// PlaceInfantryInCell (`InfantryClass::Unlimbo`, `0x0051E01B`): no placement
/// draw, the vehicle's exact in-cell coordinate, and its OnBridge.
#[test]
fn a_crewman_leaving_a_bridge_deck_keeps_the_vehicle_coordinate() {
    let rules = rules();
    let (sub_x, sub_y) = (SimFixed::from_num(60), SimFixed::from_num(200));
    let mut escaped = 0;
    for seed in 1..=8 {
        let mut sim = sim_with_houses(seed);
        let mcv = spawn(&mut sim, &rules, "AMCV", "Americans", 10, 10);
        {
            let unit = sim.substrate.entities.get_mut(mcv).unwrap();
            unit.set_on_bridge(true);
            unit.position.set_sub_x_for_test(sub_x);
            unit.position.set_sub_y_for_test(sub_y);
        }
        sim.foot_mark_remove(
            mcv,
            &rules,
            None,
            crate::sim::world::FrameEffects::default(),
        );
        let before = sim.substrate.entities.keys_sorted();
        let mut replay = sim.scenario_rng.clone();
        sim.spawn_vehicle_crew(
            &rules,
            None,
            mcv,
            false,
            false,
            crate::sim::world::FrameEffects::default(),
        );

        let escapes = replay.next_range_u32_inclusive(0, 0x7fff_fffe) < 0x4000_0000;
        if escapes {
            let _constructor = replay.next_u32();
            let _health = replay.next_range_i32_inclusive(5, 62);
            let _scatter = replay.next_range_u32_inclusive(0, 4);
        }
        assert_eq!(sim.scenario_rng.state(), replay.state(), "seed {seed}");
        let crew = new_ids(&sim, &before);
        if escapes {
            let crewman = sim.substrate.entities.get(crew[0]).unwrap();
            assert!(crewman.on_bridge());
            assert_eq!((crewman.position.rx(), crewman.position.ry()), (10, 10));
            assert_eq!(
                (crewman.position.sub_x(), crewman.position.sub_y()),
                (sub_x, sub_y)
            );
            assert_eq!(crewman.sub_cell(), Some(3), "the SW spot of its coordinate");
            escaped += 1;
        } else {
            assert!(crew.is_empty());
        }
    }
    assert!(escaped > 0);
}

/// arg6 and passenger capacity both skip the block before its draw.
#[test]
fn prevent_escape_and_passenger_capacity_skip_the_crew_draw() {
    let rules = rules();
    let mut sim = sim_with_houses(2);
    let mcv = spawn(&mut sim, &rules, "AMCV", "Americans", 10, 10);
    let ifv = spawn(&mut sim, &rules, "IFV", "Americans", 20, 10);
    for (id, prevent) in [(mcv, true), (ifv, false)] {
        sim.foot_mark_remove(id, &rules, None, crate::sim::world::FrameEffects::default());
        let before = sim.scenario_rng.state();
        sim.spawn_vehicle_crew(
            &rules,
            None,
            id,
            prevent,
            false,
            crate::sim::world::FrameEffects::default(),
        );
        assert_eq!(sim.scenario_rng.state(), before);
    }
}

fn kill(sim: &mut Simulation, rules: &RuleSet, id: u64, flags: ReceiverCallFlags) {
    kill_with(sim, rules, None, id, "KILLWH", flags);
}

fn kill_with(
    sim: &mut Simulation,
    rules: &RuleSet,
    registry: Option<&OverlayTypeRegistry>,
    id: u64,
    warhead: &str,
    flags: ReceiverCallFlags,
) {
    let warhead = sim.interner.intern(warhead);
    let hit =
        EntityDamageEvent::direct_receiver(id, 100_000, 0, RAD_NO_ATTACKER, None, warhead, flags);
    sim.commit_noncombat_aoe_hits(
        rules,
        registry,
        &[hit],
        crate::sim::world::FrameEffects::default(),
    );
}

const ORDINARY: ReceiverCallFlags = ReceiverCallFlags {
    ignore_defenses: false,
    arg6: false,
};

/// The production rules reader and death receiver must use each side's
/// `*Crew=` name, rather than the stock E1/E2/INIT assignment. Reusing the
/// fixture's registered infantry types keeps this test about the binding.
#[test]
fn renamed_general_crew_types_spawn_through_the_building_death_receiver() {
    let renamed = RULES
        .replace("AlliedCrew=E1", "AlliedCrew=E2")
        .replace("SovietCrew=E2", "SovietCrew=INIT")
        .replace("ThirdCrew=INIT", "ThirdCrew=E1");
    let rules = RuleSet::from_ini_with_fixed_art_for_test(
        &IniFile::from_str(&renamed),
        &IniFile::from_str(ART),
    )
    .expect("renamed crew rules");

    for (owner, expected) in [
        ("Americans", "E2"),
        ("Russians", "INIT"),
        ("YuriCountry", "E1"),
    ] {
        let mut spawned = 0;
        for seed in 1..=8 {
            let mut sim = sim_with_houses(seed);
            let plant = spawn(&mut sim, &rules, "GAPOWR", owner, 10, 10);
            let before = sim.substrate.entities.keys_sorted();
            kill(&mut sim, &rules, plant, ORDINARY);
            for id in new_ids(&sim, &before) {
                let crewman = sim.substrate.entities.get(id).unwrap();
                assert_eq!(crewman.category(), EntityCategory::Infantry);
                assert_eq!(type_name(&sim, id), expected, "owner {owner}, seed {seed}");
                spawned += 1;
            }
        }
        assert!(spawned > 0, "no {owner} crew reached the receiver");
    }
}

/// Board `count` limbo E1s of `owner` into `transport`, the first at the
/// cargo head.
fn load_passengers(
    sim: &mut Simulation,
    rules: &RuleSet,
    transport: u64,
    owner: &str,
    count: usize,
) -> Vec<u64> {
    let (rx, ry, open_topped) = {
        let entity = sim.substrate.entities.get(transport).unwrap();
        let open_topped = sim
            .interner
            .object_type(entity.type_ref(), rules)
            .is_some_and(|object| object.open_topped);
        (entity.position.rx(), entity.position.ry(), open_topped)
    };
    let passengers: Vec<u64> = (0..count)
        .map(|_| {
            sim.construct_object_limbo_at_height(
                "E1",
                owner,
                rx,
                ry,
                0,
                0,
                rules,
                crate::sim::world::FrameEffects::default(),
            )
            .expect("passenger")
        })
        .collect();
    for &id in passengers.iter().rev() {
        sim.substrate.entities.get_mut(id).unwrap().passenger_role =
            PassengerRole::inside_for_test(transport, open_topped);
        let cargo = sim
            .substrate
            .entities
            .get_mut(transport)
            .unwrap()
            .passenger_role
            .cargo_mut()
            .expect("a transport");
        cargo.board_forced_chain(&[(id, 1.0)]);
    }
    passengers
}

/// A dying unit's passenger must be able to enter its cell, so these
/// fixtures stand on the flat arena map.
fn sim_on_arena(seed: u64, rules: &RuleSet) -> Simulation {
    let mut sim = sim_with_houses(seed);
    crate::sim::arena_fixture::flat_arena(&mut sim, rules);
    sim
}

fn kill_by(
    sim: &mut Simulation,
    rules: &RuleSet,
    id: u64,
    attacker: u64,
    flags: ReceiverCallFlags,
) {
    let warhead = sim.interner.intern("KILLWH");
    let house = sim
        .substrate
        .entities
        .get(attacker)
        .map(|entity| entity.owner());
    let hit = EntityDamageEvent::direct_receiver(id, 100_000, 0, attacker, house, warhead, flags);
    sim.commit_noncombat_aoe_hits(
        rules,
        None,
        &[hit],
        crate::sim::world::FrameEffects::default(),
    );
}

fn cargo_len(sim: &Simulation, transport: u64) -> usize {
    sim.substrate
        .entities
        .get(transport)
        .and_then(|entity| entity.passenger_role.cargo())
        .map_or(0, |cargo| cargo.passengers().len())
}

/// Through the production receiver: a dying transport's passengers step out
/// onto its cell in cargo order, uncredited; a computer's Hunt and a human's
/// stay put.
#[test]
fn a_killing_hit_lets_the_passengers_out() {
    let rules = rules();
    for (owner, mission) in [
        ("Americans", MissionId::from_known(MissionType::Hunt)),
        ("AlliedHuman", MissionId::NONE),
    ] {
        let mut sim = sim_on_arena(3, &rules);
        let apc = spawn(&mut sim, &rules, "APC", owner, 10, 10);
        let attacker = spawn(&mut sim, &rules, "APC", "Russians", 14, 10);
        let passengers = load_passengers(&mut sim, &rules, apc, owner, 3);
        kill_by(&mut sim, &rules, apc, attacker, ORDINARY);
        assert!(sim.substrate.pending_delete.contains_object(&apc));
        assert_eq!(cargo_len(&sim, apc), 0);
        for &id in &passengers {
            let passenger = sim.substrate.entities.get(id).expect("escaped");
            assert!(passenger.lifecycle.object_alive() && !passenger.lifecycle.in_limbo());
            assert!(passenger.passenger_role.is_none());
            assert_eq!((passenger.position.rx(), passenger.position.ry()), (10, 10));
            assert_eq!(passenger.kill_record.killer(), None);
            assert!(!passenger.selection.is_selected());
            assert_eq!(queued(&sim, id), mission, "{owner}");
            assert!(!sim.substrate.pending_delete.contains_object(&id));
        }
    }
}

/// IgnoreDefenses (`0x007380A3`) kills every passenger instead, crediting
/// the attacker.
#[test]
fn an_ignore_defenses_kill_takes_the_passengers_along() {
    let rules = rules();
    let mut sim = sim_on_arena(3, &rules);
    let apc = spawn(&mut sim, &rules, "APC", "Americans", 10, 10);
    let attacker = spawn(&mut sim, &rules, "APC", "Russians", 14, 10);
    let passengers = load_passengers(&mut sim, &rules, apc, "Americans", 2);
    kill_by(
        &mut sim,
        &rules,
        apc,
        attacker,
        ReceiverCallFlags {
            ignore_defenses: true,
            arg6: false,
        },
    );
    let russians = sim.interner.intern("Russians");
    for id in passengers {
        let passenger = sim.substrate.entities.get(id).unwrap();
        assert!(sim.substrate.pending_delete.contains_object(&id));
        assert!(!passenger.lifecycle.object_alive());
        assert_eq!(passenger.kill_record.killer(), Some(russians));
    }
}

/// An `Explodes=` transport kills its passengers in the Techno death arm
/// (`0x00702603..0x00702667`), before its death weapon and its own UnInit,
/// crediting the attacker; its UnitClass arm then finds the cargo empty.
#[test]
fn an_exploding_transport_kills_its_passengers_first() {
    let rules = rules();
    let mut sim = sim_on_arena(3, &rules);
    let boomer = spawn(&mut sim, &rules, "BOOMV", "Americans", 10, 10);
    let attacker = spawn(&mut sim, &rules, "APC", "Russians", 14, 10);
    let passengers = load_passengers(&mut sim, &rules, boomer, "Americans", 2);
    kill_by(&mut sim, &rules, boomer, attacker, ORDINARY);
    let russians = sim.interner.intern("Russians");
    let deleted = sim.substrate.pending_delete.object_ids();
    let order: Vec<usize> = passengers
        .iter()
        .chain([&boomer])
        .map(|id| deleted.iter().position(|d| d == id).expect("UnInit"))
        .collect();
    assert!(order[0] < order[1] && order[1] < order[2], "{deleted:?}");
    for id in passengers {
        assert_eq!(
            sim.substrate.entities.get(id).unwrap().kill_record.killer(),
            Some(russians)
        );
    }
}

/// The death arm's Suicide test reads `GetWeapon(+0x138)`, which a Gunner
/// transport takes from its passenger's IFVMode (SetGunnerWeapon
/// `0x0070DC70`): the stock IFV carrying a Crazy Ivan (IFVMode=7, CRNuke
/// `Suicide=yes`) kills its passenger and fires its death weapon; carrying a
/// GI (IFVMode=2) it lets the GI out and fires nothing.
#[test]
fn a_gunner_transport_on_its_suicide_weapon_explodes() {
    let rules = rules();
    for (ifv_mode, explodes) in [(7, true), (2, false)] {
        let mut sim = sim_on_arena(3, &rules);
        let ifv = spawn(&mut sim, &rules, "GUNV", "Americans", 10, 10);
        let bystander = spawn(&mut sim, &rules, "APC", "Americans", 11, 10);
        let attacker = spawn(&mut sim, &rules, "APC", "Russians", 14, 10);
        let passenger = load_passengers(&mut sim, &rules, ifv, "Americans", 1)[0];
        sim.substrate
            .entities
            .get_mut(ifv)
            .unwrap()
            .set_gunner_selection_for_test(ifv_mode, -1);
        kill_by(&mut sim, &rules, ifv, attacker, ORDINARY);
        let russians = sim.interner.intern("Russians");
        let passenger = sim.substrate.entities.get(passenger).unwrap();
        let bystander_hit = sim
            .substrate
            .entities
            .get(bystander)
            .unwrap()
            .health
            .current()
            < 200;
        if explodes {
            assert!(!passenger.lifecycle.object_alive());
            assert_eq!(passenger.kill_record.killer(), Some(russians));
        } else {
            assert!(passenger.lifecycle.object_alive() && !passenger.lifecycle.in_limbo());
            assert_eq!(passenger.kill_record.killer(), None);
        }
        assert_eq!(bystander_hit, explodes, "IFVMode {ifv_mode}");
    }
}

/// Retail `rulesmd.ini`: the stock IFV's death arm holds for exactly the
/// passengers whose IFVMode slot is a `Suicide=` weapon.
#[test]
fn retail_ifv_death_arm_follows_its_passengers_slot() {
    let Some(rules) = crate::rules::retail_ini_fixture::retail_rulesmd_rules() else {
        return;
    };
    let fv = rules.object("FV").unwrap();
    let mut exploding: Vec<&str> = rules
        .infantry_ids
        .iter()
        .filter(|id| {
            let ifv_mode = rules.object(id).unwrap().ifv_mode;
            let numbered = crate::sim::combat::combat_weapon::weapon_for_index(fv, 0, ifv_mode)
                .and_then(|(weapon_id, _)| rules.weapon(weapon_id));
            crate::sim::combat::death_arm_explodes(fv, 0, numbered)
        })
        .map(String::as_str)
        .collect();
    exploding.sort_unstable();
    assert_eq!(exploding, ["CIVAN", "IVAN", "TERROR"]);
    let slot0 = crate::sim::combat::combat_weapon::weapon_for_index(fv, 0, 0)
        .and_then(|(weapon_id, _)| rules.weapon(weapon_id));
    assert!(!crate::sim::combat::death_arm_explodes(fv, 0, slot0));
}

/// A transport the local player had selected (`0x00737C98..0x00737CB6`)
/// selects each escapee (`0x00738174`) and its crewman (`0x00738352`).
#[test]
fn a_selected_vehicle_selects_its_escapees_and_crewman() {
    let rules = rules();
    let mut crewmen = 0;
    for seed in 1..=8 {
        for selected in [true, false] {
            let mut sim = sim_on_arena(seed, &rules);
            let owner = sim.interner.intern("AlliedHuman");
            sim.session.set_current_house_for_test(Some(owner));
            let apc = spawn(&mut sim, &rules, "APC", "AlliedHuman", 10, 10);
            let mcv = spawn(&mut sim, &rules, "AMCV", "AlliedHuman", 20, 10);
            let passengers = load_passengers(&mut sim, &rules, apc, "AlliedHuman", 2);
            for id in [apc, mcv] {
                sim.substrate
                    .entities
                    .get_mut(id)
                    .unwrap()
                    .selection
                    .set_selected_for_test(selected);
            }
            kill(&mut sim, &rules, apc, ORDINARY);
            let before = sim.substrate.entities.keys_sorted();
            kill(&mut sim, &rules, mcv, ORDINARY);
            for id in passengers.into_iter().chain(new_ids(&sim, &before)) {
                assert_eq!(
                    sim.substrate
                        .entities
                        .get(id)
                        .unwrap()
                        .selection
                        .is_selected(),
                    selected
                );
            }
            crewmen += new_ids(&sim, &before).len();
        }
    }
    assert!(crewmen > 0);
}

/// A selected transport the local player does not own hands no selection on
/// (`HouseClass::IsHumanPlayer @ 0x0050B6F0`, skirmish arm: the owner is the
/// local player).
#[test]
fn a_selected_foreign_vehicle_selects_nothing() {
    let rules = rules();
    let mut sim = sim_on_arena(1, &rules);
    let player = sim.interner.intern("AlliedHuman");
    sim.session.set_current_house_for_test(Some(player));
    let apc = spawn(&mut sim, &rules, "APC", "Americans", 10, 10);
    let passengers = load_passengers(&mut sim, &rules, apc, "Americans", 2);
    sim.substrate
        .entities
        .get_mut(apc)
        .unwrap()
        .selection
        .set_selected_for_test(true);
    kill(&mut sim, &rules, apc, ORDINARY);
    for id in passengers {
        let passenger = sim.substrate.entities.get(id).unwrap();
        assert!(passenger.lifecycle.object_alive() && !passenger.lifecycle.in_limbo());
        assert!(!passenger.selection.is_selected());
    }
}

/// Retail `rulesmd.ini` and `artmd.ini` (the local `ini/`): the stock crew
/// keys bind, and a power plant and an MCV killed through the production
/// receiver leave stock crew with native health ranges.
#[test]
fn retail_rules_crew_the_power_plant_and_the_mcv() {
    let Some(rules) = crate::rules::retail_ini_fixture::retail_unlayered_rules_with_art() else {
        return;
    };
    let general = &rules.general;
    assert_eq!(
        [
            general.allied_crew.as_deref(),
            general.soviet_crew.as_deref(),
            general.third_crew.as_deref(),
            general.technician.as_deref(),
            general.engineer_infantry.as_deref(),
        ],
        [
            Some("E1"),
            Some("E2"),
            Some("INIT"),
            Some("CTECH"),
            Some("ENGINEER")
        ]
    );
    assert_eq!(general.crew_escape.bits(), 0x3fe0_0000_0000_0000);
    assert_eq!(general.refund_percent.bits(), 0x3fe0_0000_0000_0000);
    assert_eq!(rules.object("NAREFN").unwrap().soylent, 300);

    let mut plant_crew = 0;
    let mut mcv_crew = 0;
    for seed in 1..=8 {
        let mut sim = sim_with_houses(seed);
        let plant = spawn(&mut sim, &rules, "GAPOWR", "Americans", 10, 10);
        let soviet_plant = spawn(&mut sim, &rules, "NAPOWR", "Russians", 30, 10);
        let yard = spawn(&mut sim, &rules, "GACNST", "Americans", 40, 10);
        assert_eq!(sim.building_survivor_count(&rules, plant), 1);
        assert_eq!(sim.building_survivor_count(&rules, soviet_plant), 2);
        assert_eq!(sim.building_survivor_count(&rules, yard), 5);
        let mcv = spawn(&mut sim, &rules, "AMCV", "Americans", 20, 10);
        let before = sim.substrate.entities.keys_sorted();
        kill_with(&mut sim, &rules, None, plant, "Super", ORDINARY);
        kill_with(&mut sim, &rules, None, mcv, "Super", ORDINARY);
        for id in new_ids(&sim, &before) {
            let entity = sim.substrate.entities.get(id).unwrap();
            if entity.category() != EntityCategory::Infantry {
                continue;
            }
            assert_eq!(type_name(&sim, id), "E1");
            match (entity.position.rx(), entity.position.ry()) {
                (10..=11, 10..=11) => {
                    assert!((5..=125).contains(&entity.health.current()));
                    plant_crew += 1;
                }
                (20, 10) => {
                    assert!((5..=62).contains(&entity.health.current()));
                    mcv_crew += 1;
                }
                cell => panic!("crewman {id} at {cell:?}"),
            }
        }
    }
    assert!(plant_crew > 0 && mcv_crew > 0, "{plant_crew} {mcv_crew}");
}

/// A second power plant well away from the scene keeps the fixture's
/// Americans in the game: with the retail `ShortGame=yes`, a house left with
/// no counted building and no `BaseUnit=` vehicle is defeated on its next
/// update, and `HouseClass::Blowup_All` kills everything it still owns
/// (`sim::world::house_defeat`).
fn keep_undefeated(
    sim: &mut Simulation,
    resources: &crate::sim::runtime::SimResources,
    scene: (u16, u16),
) -> u64 {
    (40..100_u16)
        .flat_map(|y| (40..100_u16).map(move |x| (x, y)))
        .filter(|&(x, y)| x.abs_diff(scene.0) + y.abs_diff(scene.1) >= 16)
        .find_map(|(x, y)| {
            let grid = sim.path_grid()?;
            let open = (x..=x + 1).all(|cx| {
                (y..=y + 1).all(|cy| grid.cell(cx, cy).is_some_and(|cell| cell.ground_walkable))
            });
            if !open {
                return None;
            }
            sim.spawn_object("GAPOWR", "Americans", x, y, 0, &resources.rules)
        })
        .expect("room for a power plant away from the scene")
}

/// Retail Dustbowl runtime: a power plant and an MCV die through the
/// production receiver on real terrain, and their crews Scatter through FNPC
/// and walk off the wreck. Ignored: needs the retail install (`RA2_DIR` or
/// `config.toml`).
#[test]
#[ignore = "requires a retail RA2/YR install (RA2_DIR or config.toml)"]
fn retail_dustbowl_crews_scatter_off_their_wrecks() {
    let dir = std::env::var("RA2_DIR")
        .ok()
        .filter(|path| !path.trim().is_empty())
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| {
            crate::test_fixture::game_config()
                .expect("set RA2_DIR or provide config.toml for this ignored test")
                .paths
                .ra2_dir
        });
    let mut crews = [0; 2];
    for run in 0..8_u32 {
        let seed = 0x00C0_FFEE_u32.wrapping_add(run.wrapping_mul(0x9E37_79B9));
        let mut scenario =
            crate::headless_scenario::load(&dir, "Dustbowl.mmx", seed).expect("Dustbowl loads");
        let crate::sim::runtime::SimRuntime {
            simulation: sim,
            resources,
        } = &mut scenario.runtime;
        let owner = sim.interner.intern("Americans");
        sim.houses
            .get_or_register_for_test(owner, || HouseState::new(owner, 0, None, false, 10_000, 10));
        // The first central cell that admits the MCV, with level walkable
        // ground under and around it and a power plant three cells west, as a
        // player's base would stand on.
        let (mcv, plant) = (40..100_u16)
            .flat_map(|y| (40..100_u16).map(move |x| (x, y)))
            .find_map(|(x, y)| {
                let grid = sim.path_grid()?;
                let terrain = sim.resolved_terrain.as_ref()?;
                let level = terrain.cell(x.checked_sub(3)?, y)?.level;
                let (x0, y0) = (x.checked_sub(4)?, y.checked_sub(1)?);
                let open = (x0..=x + 1).all(|cx| {
                    (y0..=y + 2).all(|cy| {
                        terrain.cell(cx, cy).is_some_and(|cell| cell.level == level)
                            && grid.cell(cx, cy).is_some_and(|cell| cell.ground_walkable)
                    })
                });
                if !open {
                    return None;
                }
                let mcv = sim.spawn_object("AMCV", "Americans", x, y, 0, &resources.rules)?;
                let plant =
                    sim.spawn_object("GAPOWR", "Americans", x - 3, y, 0, &resources.rules)?;
                Some((mcv, plant))
            })
            .expect("an MCV cell with room for a power plant");
        let (plant_cell, mcv_cell) = [plant, mcv]
            .map(|id| {
                let entity = sim.substrate.entities.get(id).unwrap();
                (entity.position.rx(), entity.position.ry())
            })
            .into();
        let guard = keep_undefeated(sim, resources, mcv_cell);
        sim.resolve_type_handles(&resources.rules);
        let registry = Some(&resources.overlay_registry);
        let before = sim.substrate.entities.keys_sorted();
        kill_with(sim, &resources.rules, registry, plant, "Super", ORDINARY);
        kill_with(sim, &resources.rules, registry, mcv, "Super", ORDINARY);
        let crew: Vec<_> = new_ids(sim, &before)
            .into_iter()
            .filter(|&id| {
                sim.substrate.entities.get(id).unwrap().category() == EntityCategory::Infantry
            })
            .map(|id| {
                let entity = sim.substrate.entities.get(id).unwrap();
                let spawn = (entity.position.rx(), entity.position.ry());
                (id, spawn != mcv_cell, spawn, entity.navigation.nav_com())
            })
            .collect();
        for _ in 0..45 {
            scenario.tick();
        }
        assert!(scenario.sim().substrate.entities.get(guard).is_some());
        assert!(!scenario.sim().houses[&owner].is_defeated());
        for (id, from_plant, spawn, destination) in crew {
            let entity = scenario
                .sim()
                .substrate
                .entities
                .get(id)
                .expect("crewman lives");
            let cell = (entity.position.rx(), entity.position.ry());
            println!(
                "seed {seed:#x}: {} crewman {id} {} spawned {spawn:?} (plant {plant_cell:?}, MCV {mcv_cell:?}), Scatter to {destination:?}, at {cell:?} after 45 frames, health {}",
                if from_plant { "plant" } else { "MCV" },
                scenario.sim().interner.resolve(entity.type_ref()),
                entity.health.current()
            );
            assert_eq!(scenario.sim().interner.resolve(entity.type_ref()), "E1");
            let Some(crate::sim::components::NavTargetRef::Cell { rx, ry }) = destination else {
                panic!("crewman {id} has no Scatter destination");
            };
            assert_ne!((rx, ry), spawn, "Scatter leads off the spawn cell");
            assert_ne!(cell, spawn, "crewman {id} walked off its spawn cell");
            crews[usize::from(from_plant)] += 1;
        }
    }
    assert!(
        crews[0] > 0 && crews[1] > 0,
        "MCV {} plant {}",
        crews[0],
        crews[1]
    );
}

/// Retail Dustbowl runtime: GIs board a full Battle Fortress (`OpenTopped=`,
/// five), an IFV (`Gunner=`) and a Flak Track through the production
/// attach, the transports die through the production receiver, and every GI
/// steps out onto its transport's cell, Scatters and walks off the wreck. The
/// Battle Fortress's fourth and fifth GIs are admitted because the earlier
/// ones already move: Scatter's Set_Destination reaches the Walk's MoveTo
/// (`0x004D965D`), which sets its IsMoving byte (`0x0075AD5A`), and
/// Can_Enter_Cell counts only stationary allied infantry
/// (`0x0051C6E7..0x0051C70B`). Ignored: needs the retail install (`RA2_DIR`
/// or `config.toml`).
#[test]
#[ignore = "requires a retail RA2/YR install (RA2_DIR or config.toml)"]
fn retail_dustbowl_passengers_leave_their_destroyed_transports() {
    let dir = std::env::var("RA2_DIR")
        .ok()
        .filter(|path| !path.trim().is_empty())
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| {
            crate::test_fixture::game_config()
                .expect("set RA2_DIR or provide config.toml for this ignored test")
                .paths
                .ra2_dir
        });
    let mut scenario =
        crate::headless_scenario::load(&dir, "Dustbowl.mmx", 0x00C0_FFEE).expect("Dustbowl loads");
    let crate::sim::runtime::SimRuntime {
        simulation: sim,
        resources,
    } = &mut scenario.runtime;
    let rules = &resources.rules;
    let owner = sim.interner.intern("Americans");
    sim.houses
        .get_or_register_for_test(owner, || HouseState::new(owner, 0, None, false, 10_000, 10));
    // Each transport on the first central cell that admits it, with level
    // walkable ground around it for its GIs (the row south) and their
    // Scatter, four or more cells from the others. Check the shared playfield
    // owner for the entire neighborhood: fresh Unit +3D5=false admission can
    // succeed outside it (original73F34C; unit_entry_boundary rows24/30), so
    // transport placement does not establish that its GIs can reveal there.
    type TransportLoad = (&'static str, u64, (u16, u16), Vec<u64>);
    let mut loads: Vec<TransportLoad> = Vec::new();
    for (kind, count) in [("BFRT", 5_u16), ("FV", 1), ("HTK", 3)] {
        let mut placed = None;
        for (x, y) in (40..100_u16).flat_map(|y| (40..100_u16).map(move |x| (x, y))) {
            let spaced = loads
                .iter()
                .all(|(_, _, (lx, ly), _)| x.abs_diff(*lx).max(y.abs_diff(*ly)) >= 4);
            let open = || {
                let (grid, terrain) = (sim.path_grid()?, sim.resolved_terrain.as_ref()?);
                let level = terrain.cell(x, y)?.level;
                Some((x - 2..=x + 2).all(|cx| {
                    (y - 2..=y + 2).all(|cy| {
                        crate::sim::cell_rect::cell_is_in_playfield_height_aware(
                            (i32::from(cx), i32::from(cy)),
                            sim.playfield_bounds(),
                            Some(terrain),
                        ) && terrain.cell(cx, cy).is_some_and(|cell| cell.level == level)
                            && grid.cell(cx, cy).is_some_and(|cell| cell.ground_walkable)
                    })
                }))
            };
            if !spaced || open() != Some(true) {
                continue;
            }
            if let Some(id) = sim.spawn_object(kind, "Americans", x, y, 0, rules) {
                placed = Some((id, (x, y)));
                break;
            }
        }
        let (transport, (tx, ty)) = placed.unwrap_or_else(|| panic!("{kind} spawns"));
        let gis = (0..count)
            .map(|i| {
                let id = sim
                    .spawn_object_with_overlay_registry(
                        "E1",
                        "Americans",
                        tx - 1 + i % 3,
                        if i < 3 { ty + 1 } else { ty - 1 },
                        0,
                        rules,
                        &resources.overlay_registry,
                    )
                    .unwrap_or_else(|| {
                        panic!(
                            "{kind} GI {i} rejected at ({}, {}) beside transport ({tx}, {ty})",
                            tx - 1 + i % 3,
                            if i < 3 { ty + 1 } else { ty - 1 },
                        )
                    });
                id
            })
            .collect();
        loads.push((kind, transport, (tx, ty), gis));
    }
    let guard = keep_undefeated(sim, resources, loads[0].2);
    sim.resolve_type_handles(rules);
    // The Infantry transport arm's attach once CAN_LOAD said ROGER
    // (`0x0051A3FC..0x0051A485`); the walk in has its own tests.
    for (_, transport, _, gis) in &loads {
        for &gi in gis {
            assert!(crate::sim::passenger::board_passenger(
                sim,
                rules,
                gi,
                *transport,
                crate::sim::passenger::BoardingArm::InfantryTransport,
                Some(&resources.overlay_registry),
                crate::sim::world::FrameEffects::default(),
            ));
        }
    }
    for (kind, transport, _, gis) in &loads {
        assert_eq!(cargo_len(sim, *transport), gis.len(), "{kind} boarded");
    }
    assert!(
        sim.substrate
            .entities
            .get(loads[1].1)
            .unwrap()
            .current_weapon_number()
            == rules.object("E1").unwrap().ifv_mode,
        "the IFV took its gunner's weapon"
    );
    let registry = Some(&resources.overlay_registry);
    for (_, transport, _, _) in &loads {
        kill_with(sim, rules, registry, *transport, "Super", ORDINARY);
    }
    let mut destinations = Vec::new();
    for (kind, transport, cell, gis) in &loads {
        assert!(sim.substrate.pending_delete.contains_object(transport));
        for &id in gis {
            let gi = sim.substrate.entities.get(id).expect("GI lives");
            assert!(
                gi.lifecycle.object_alive() && !gi.lifecycle.in_limbo(),
                "{kind}"
            );
            assert!(gi.passenger_role.is_none());
            assert_eq!(
                (gi.position.rx(), gi.position.ry()),
                *cell,
                "{kind}: out on its cell"
            );
            let Some(crate::sim::components::NavTargetRef::Cell { rx, ry }) =
                gi.navigation.nav_com()
            else {
                panic!("{kind}: GI {id} has no Scatter destination");
            };
            assert_ne!((rx, ry), *cell, "{kind}: Scatter leads off the wreck");
            destinations.push((*kind, id, *cell, (rx, ry)));
        }
    }
    // Each GI walks off the wreck; a computer's Hunt may lead it back later.
    let mut left = vec![false; destinations.len()];
    for _ in 0..45 {
        scenario.tick();
        for (index, (_, id, wreck, _)) in destinations.iter().enumerate() {
            let gi = scenario
                .sim()
                .substrate
                .entities
                .get(*id)
                .expect("GI lives");
            left[index] |= (gi.position.rx(), gi.position.ry()) != *wreck;
        }
    }
    assert!(scenario.sim().substrate.entities.get(guard).is_some());
    assert!(!scenario.sim().houses[&owner].is_defeated());
    for ((kind, id, wreck, destination), left) in destinations.into_iter().zip(left) {
        println!("{kind} GI {id}: wreck {wreck:?}, Scatter to {destination:?}");
        assert!(left, "{kind}: GI {id} walked off the wreck");
    }
}

/// SpawnSurvivors' Unlimbo direction (`0x00442F3C..0x00442F63`, which the
/// survivor spawn above reads through `body_facing_dir`): the building's
/// `+0x388` Current() rounded to a DirType, wrapping 0xFF80.. to 0. Rows are
/// the original instructions' outputs (`tools/spatial_oracle/survivor_facing.py`).
#[test]
fn survivor_direction_matches_the_original() {
    let rows: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/survivor_facing.json",
    ))
    .unwrap();
    let rows = rows.as_array().unwrap();
    assert_eq!(rows.len(), 42);
    for row in rows {
        let input = &row["input"];
        let number = |key: &str| input[key].as_i64().unwrap();
        let initial = number("initial") as u16;
        let mut facing = crate::sim::movement::FacingClass::new(initial, number("rot") as i32);
        facing.snap(initial, number("start") as u32);
        if let Some(target) = input["target"].as_u64() {
            facing.set(target as u16, number("set_frame") as u32);
        }
        let mut building =
            crate::sim::game_entity::GameEntity::test_default(1, "GAPILE", "Americans", 5, 5);
        building.body_facing = facing;
        let frame = number("sample_frame") as u32;
        assert_eq!(
            u64::from(building.body_facing_current(frame)),
            row["current"].as_u64().unwrap(),
            "{input}"
        );
        assert_eq!(
            u64::from(building.body_facing_dir(frame)),
            row["direction"].as_u64().unwrap(),
            "{input}"
        );
    }
}

/// Retail: the crewman of a selected MCV is selected through
/// `TechnoClass::Select @ 0x006FBFA0`, which speaks his `VoiceSelect=`
/// (`0x006FC018`, the latch set as every Logic pass leaves it).
#[test]
fn a_selected_mcvs_crewman_speaks_his_select_voice() {
    let Some(retail) = crate::rules::retail_ini_fixture::retail_battle_rules() else {
        return;
    };
    let rules = retail.rules;
    let gi_select = &rules.object("E1").unwrap().voice_select;
    assert!(!gi_select.is_empty(), "retail E1 VoiceSelect binds");
    let mut crewmen = 0;
    for seed in 1..=8 {
        let mut sim = sim_on_arena(seed, &rules);
        let owner = sim.interner.intern("AlliedHuman");
        sim.session.set_current_house_for_test(Some(owner));
        let mcv = spawn(&mut sim, &rules, "AMCV", "AlliedHuman", 20, 10);
        sim.substrate
            .entities
            .get_mut(mcv)
            .unwrap()
            .selection
            .set_selected_for_test(true);
        let before = sim.substrate.entities.keys_sorted();
        sim.sound_events.clear_for_test();
        kill_with(&mut sim, &rules, None, mcv, "Super", ORDINARY);
        for crewman in new_ids(&sim, &before) {
            if type_name(&sim, crewman) != "E1" {
                continue;
            }
            crewmen += 1;
            assert!(
                sim.substrate
                    .entities
                    .get(crewman)
                    .unwrap()
                    .selection
                    .is_selected()
            );
            assert!(sim.sound_events.iter().any(|event| matches!(
                event,
                crate::sim::world::SimSoundEvent::UnitVoiceRequested { owner, sound_id }
                    if *owner == crewman && gi_select.contains(sound_id)
            )));
        }
    }
    assert!(crewmen > 0);
}

#[test]
fn signed_country_crew_type_and_rng_match_original() {
    let rows: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/crew_type.json",
    ))
    .unwrap();
    let rules = RuleSet::from_ini(&IniFile::from_str(RULES)).unwrap();
    for row in rows.as_array().unwrap() {
        let mut sim = Simulation::with_seed(1);
        sim.scenario_rng =
            SimRng::from_native_state_hex_for_test(row["rng_before"].as_str().unwrap());
        assert_eq!(
            techno_crew_type(
                &mut sim.scenario_rng,
                &rules,
                row["country_side"].as_i64().unwrap() as i32,
                row["armed"].as_bool().unwrap()
            )
            .as_deref(),
            row["result"].as_str()
        );
        assert_eq!(
            sim.scenario_rng.native_state_hex(),
            row["rng_after"].as_str().unwrap()
        );
    }
}

/// A sale's refused absorbed passenger (Mission_Selling `0x0044A56D`) is
/// recorded with a NULL source (`0x0044A56F`): its house books the loss and no
/// house is credited the kill.
#[test]
fn a_refused_passenger_of_a_sale_books_a_loss_with_no_kill() {
    let rules = rules();
    let mut sim = sim_with_houses(9);
    let reactor = spawn(&mut sim, &rules, "YAPOWR", "Americans", 10, 10);
    let absorbed = load_absorbed(&mut sim, &rules, reactor, 2);
    sim.substrate
        .entities
        .get_mut(reactor)
        .unwrap()
        .no_survivor
        .set_for_test(true);
    let stats = |sim: &Simulation| {
        sim.houses
            .order()
            .iter()
            .map(|id| *sim.houses[id].stats())
            .collect::<Vec<_>>()
    };
    let americans = sim.interner.get("Americans").unwrap();
    let lost = sim.houses[&americans].stats().units_lost();
    let before = stats(&sim);
    sim.clear_lifecycle_test_events_for_test();
    sim.eject_absorbed_passengers(
        &rules,
        None,
        reactor,
        None,
        &foundation_cells(10, 10, "2x2"),
        crate::sim::world::FrameEffects::default(),
    );
    for &id in &absorbed {
        assert!(
            sim.lifecycle_test_events_for_test()
                .iter()
                .any(|event| matches!(
                    event,
                    crate::sim::world::LifecycleTestEvent::RecordKillBookkeeping { stable_id }
                        if *stable_id == id
                )),
            "absorbed passenger {id} has its kill recorded"
        );
    }
    assert_eq!(sim.houses[&americans].stats().units_lost(), lost + 2);
    for (after, before) in stats(&sim).iter().zip(&before) {
        assert_eq!(after.units_killed(), before.units_killed());
        assert_eq!(after.buildings_killed(), before.buildings_killed());
    }
}
