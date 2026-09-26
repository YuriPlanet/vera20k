//! Rust regression tests for the crew survival owner. Expected draws are
//! replayed on a clone of the Scenario stream in the native order recorded on
//! each producer; they are not native goldens. Mapless fixtures stop Scatter
//! at its FNPC search, after its one `RandomRanged(0, 4)` draw.

use super::*;
use crate::map::entities::EntityCategory;
use crate::rules::ini_parser::IniFile;
use crate::sim::combat::combat_weapon::WeaponOverride;
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
Foundation=2x2
[GACNST]
Strength=1000
Cost=3000
Crewed=yes
Foundation=3x3
Factory=BuildingType
[YAPOWR]
Strength=750
Cost=600
Crewed=yes
Foundation=2x2
InfantryAbsorb=yes
Passengers=5
[GAREFN]
Strength=900
Cost=2000
Soylent=300
Crewed=yes
Foundation=4x3
[GASAND]
Strength=100
Cost=100
Foundation=1x1
[REDLAMP]
Strength=100
Cost=0
Crewed=yes
Foundation=1x1
[KILLWH]
Verses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%
";

fn rules() -> RuleSet {
    RuleSet::from_ini(&IniFile::from_str(RULES)).expect("crew survival rules")
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
            .insert(id, HouseState::new(id, side, None, human, 0, 10));
        sim.session.house_order.push(id);
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

/// For the stock 50% the x87 gate is exactly `r < 0x40000000`: only
/// `0x3FFFFFFF` depends on the chop control word, and under chop it escapes.
/// A 25% chance is the prefix below `0x20000000`, and an unordered compare
/// sets C0 like "less".
#[test]
fn crew_escape_is_a_prefix_of_the_draw_range() {
    let half = NativeF64Bits::from_bits(0x3fe0_0000_0000_0000);
    for roll in [0, 1, 0x3fff_fffe, 0x3fff_ffff] {
        assert!(crew_escapes(roll, half), "{roll:#x}");
    }
    for roll in [0x4000_0000, 0x4000_0001, 0x7fff_fffe] {
        assert!(!crew_escapes(roll, half), "{roll:#x}");
    }
    let quarter = NativeF64Bits::from_bits(0x3fd0_0000_0000_0000);
    assert!(crew_escapes(0x1fff_ffff, quarter));
    assert!(!crew_escapes(0x2000_0000, quarter));
    assert!(crew_escapes(
        0x7fff_fffe,
        NativeF64Bits::from_bits(0x7ff8_0000_0000_0000)
    ));
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
            .has_been_captured = captured;
        assert_eq!(
            sim.building_survivor_count(&rules, id, false),
            expected,
            "{kind} owned by {owner}, captured {captured}"
        );
        assert_eq!(
            sim.building_survivor_count(&rules, id, true),
            0,
            "NoSurvivor"
        );
    }
}

/// `BuildingClass::ChangeOwner @ 0x00448723` marks the transferred building.
#[test]
fn a_captured_building_remembers_it() {
    let rules = rules();
    let mut sim = sim_with_houses(1);
    let id = spawn(&mut sim, &rules, "GAPOWR", "Americans", 10, 10);
    assert!(!sim.substrate.entities.get(id).unwrap().has_been_captured);
    let russians = sim.interner.intern("Russians");
    sim.change_owner_with_rules(id, russians, &rules);
    assert!(sim.substrate.entities.get(id).unwrap().has_been_captured);
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
        sim.spawn_building_survivors(&rules, None, yard, false, |world, cell| {
            marks.push((cell, world.scenario_rng.state()));
        });

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
                assert!(entity.sub_cell.is_some() && !entity.lifecycle.in_limbo);
                (
                    type_name(&sim, id),
                    (entity.position.rx, entity.position.ry),
                    entity.health.current,
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
        .has_been_captured = true;
    let mut replay = sim.scenario_rng.clone();
    let mut marks = Vec::new();
    sim.spawn_building_survivors(&rules, None, yard, false, |world, cell| {
        marks.push((cell, world.scenario_rng.state()));
    });
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

/// No survivor owed means no Phase B at all: no roll and no per-cell mark.
#[test]
fn a_building_owing_no_survivor_draws_and_marks_nothing() {
    let rules = rules();
    let mut sim = sim_with_houses(5);
    let neutral = spawn(&mut sim, &rules, "GAPOWR", "Neutral", 10, 10);
    let uncrewed = spawn(&mut sim, &rules, "GASAND", "Americans", 20, 10);
    let allied = spawn(&mut sim, &rules, "GAPOWR", "Americans", 30, 10);
    for (id, no_survivor) in [(neutral, false), (uncrewed, false), (allied, true)] {
        let before = sim.scenario_rng.state();
        sim.spawn_building_survivors(&rules, None, id, no_survivor, |_, cell| {
            panic!("no mark expected at {cell:?}")
        });
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
                .pending_c4_detonation = Some(crate::sim::components::PendingC4Detonation {
                start_frame: 0,
                duration_frames: 100,
                source_entity_id: Some(planter),
            });
            let before = sim.substrate.entities.keys_sorted();
            let mut replay = sim.scenario_rng.clone();
            let mut marks = Vec::new();
            sim.spawn_building_survivors(&rules, None, plant, false, |world, cell| {
                marks.push((cell, world.scenario_rng.state()));
            });
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
                let target = survivor.attack_target.as_ref().map(|target| target.target);
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
        sim.spawn_building_survivors(&rules, None, plant, false, |_, _| {});
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
        .passenger_role = PassengerRole::Transport {
        cargo: PassengerCargo::new(5, 0),
    };
    (0..count)
        .map(|_| {
            let id = sim
                .construct_object_limbo_at_height("E1", "Americans", 10, 10, 0, 0, rules)
                .expect("absorbed infantry");
            sim.substrate.entities.get_mut(id).unwrap().passenger_role = PassengerRole::Inside {
                transport_id: building,
            };
            let cargo = sim
                .substrate
                .entities
                .get_mut(building)
                .unwrap()
                .passenger_role
                .cargo_mut()
                .unwrap();
            cargo.board_forced(id, 1);
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
    sim.spawn_building_survivors(&rules, None, reactor, false, |world, cell| {
        marks.push((cell, world.scenario_rng.state()));
    });

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
        assert!(matches!(passenger.passenger_role, PassengerRole::None));
        assert!(!passenger.lifecycle.in_limbo);
        assert_eq!((passenger.position.rx, passenger.position.ry), (10, 10));
        assert_eq!(queued(&sim, id), MissionId::from_known(MissionType::Hunt));
    }
}

/// NoSurvivor refuses the absorbed passengers' Unlimbo: they are removed,
/// though each infantryman's placement draw is still spent.
#[test]
fn no_survivor_uninits_the_absorbed_infantry() {
    let rules = rules();
    let mut sim = sim_with_houses(9);
    let reactor = spawn(&mut sim, &rules, "YAPOWR", "Americans", 10, 10);
    let absorbed = load_absorbed(&mut sim, &rules, reactor, 2);
    let mut replay = sim.scenario_rng.clone();
    sim.spawn_building_survivors(&rules, None, reactor, true, |_, cell| {
        panic!("no mark expected at {cell:?}")
    });
    for _ in &absorbed {
        let _overwritten_row = replay.next_range_u32(4);
    }
    assert_eq!(sim.scenario_rng.state(), replay.state());
    for id in absorbed {
        assert!(
            sim.substrate
                .entities
                .get(id)
                .is_none_or(|entity| !entity.lifecycle.object_alive),
            "absorbed passenger {id} is removed"
        );
    }
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
            sim.mark_up_dying_unit(mcv, UninitContext::with_rules(&rules));
            let before = sim.substrate.entities.keys_sorted();
            let mut replay = sim.scenario_rng.clone();
            sim.spawn_vehicle_crew(&rules, None, mcv, false, false);

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
                    assert_eq!(crewman.health.current, health);
                    assert_eq!((crewman.position.rx, crewman.position.ry), (10, 10));
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
            unit.on_bridge = true;
            unit.position.sub_x = sub_x;
            unit.position.sub_y = sub_y;
        }
        sim.mark_up_dying_unit(mcv, UninitContext::with_rules(&rules));
        let before = sim.substrate.entities.keys_sorted();
        let mut replay = sim.scenario_rng.clone();
        sim.spawn_vehicle_crew(&rules, None, mcv, false, false);

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
            assert!(crewman.on_bridge);
            assert_eq!((crewman.position.rx, crewman.position.ry), (10, 10));
            assert_eq!(
                (crewman.position.sub_x, crewman.position.sub_y),
                (sub_x, sub_y)
            );
            assert_eq!(crewman.sub_cell, Some(3), "the SW spot of its coordinate");
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
        sim.mark_up_dying_unit(id, UninitContext::with_rules(&rules));
        let before = sim.scenario_rng.state();
        sim.spawn_vehicle_crew(&rules, None, id, prevent, false);
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
    sim.commit_noncombat_aoe_hits(rules, registry, &[hit]);
}

const ORDINARY: ReceiverCallFlags = ReceiverCallFlags {
    ignore_defenses: false,
    arg6: false,
};

/// Through the production receiver: the building is removed in the same
/// call that spawns its crew, and an IgnoreDefenses kill (C4 expiry) spawns
/// none.
#[test]
fn a_killing_hit_spawns_the_building_crew_in_the_same_receiver_call() {
    let rules = rules();
    let mut spawned = 0;
    for seed in 1..=6 {
        for flags in [
            ORDINARY,
            ReceiverCallFlags {
                ignore_defenses: true,
                arg6: false,
            },
        ] {
            let mut sim = sim_with_houses(seed);
            let yard = spawn(&mut sim, &rules, "GACNST", "Americans", 10, 10);
            let before = sim.substrate.entities.keys_sorted();
            kill(&mut sim, &rules, yard, flags);
            assert!(
                sim.substrate
                    .entities
                    .get(yard)
                    .is_none_or(|entity| !entity.lifecycle.object_alive),
                "the yard died"
            );
            let crew: Vec<_> = new_ids(&sim, &before)
                .into_iter()
                .filter(|&id| {
                    sim.substrate.entities.get(id).unwrap().category == EntityCategory::Infantry
                })
                .collect();
            assert!(crew.len() <= 5);
            if flags.ignore_defenses {
                assert!(crew.is_empty(), "NoSurvivor");
            }
            for id in &crew {
                assert!(matches!(type_name(&sim, *id).as_str(), "E1" | "ENGINEER"));
            }
            spawned += crew.len();
        }
    }
    assert!(spawned > 0);
}

/// Through the production receiver: the dying vehicle leaves its cell before
/// the crewman is placed there; arg6 keeps the crew inside.
#[test]
fn a_killing_hit_lets_the_vehicle_crew_out_unless_arg6() {
    let rules = rules();
    let mut escaped = 0;
    for seed in 1..=8 {
        for flags in [
            ORDINARY,
            ReceiverCallFlags {
                ignore_defenses: false,
                arg6: true,
            },
        ] {
            let mut sim = sim_with_houses(seed);
            let mcv = spawn(&mut sim, &rules, "AMCV", "Americans", 10, 10);
            let before = sim.substrate.entities.keys_sorted();
            kill(&mut sim, &rules, mcv, flags);
            let crew = new_ids(&sim, &before);
            if flags.arg6 {
                assert!(crew.is_empty());
            }
            for id in &crew {
                let crewman = sim.substrate.entities.get(*id).unwrap();
                assert_eq!(type_name(&sim, *id), "E1");
                assert_eq!((crewman.position.rx, crewman.position.ry), (10, 10));
                assert!((5..=62).contains(&crewman.health.current));
            }
            escaped += crew.len();
        }
    }
    assert!(escaped > 0);
}

/// Through the production receiver: the fatal prelude leaves an absorbing
/// building's passengers for Phase A instead of killing them.
#[test]
fn a_killing_hit_releases_the_bio_reactors_infantry() {
    let rules = rules();
    let mut sim = sim_with_houses(4);
    let reactor = spawn(&mut sim, &rules, "YAPOWR", "Americans", 10, 10);
    let absorbed = load_absorbed(&mut sim, &rules, reactor, 3);
    kill(&mut sim, &rules, reactor, ORDINARY);
    for id in absorbed {
        let passenger = sim.substrate.entities.get(id).expect("released");
        assert!(passenger.lifecycle.object_alive && passenger.health.current > 0);
        assert!(!passenger.lifecycle.in_limbo);
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
    let (rx, ry) = {
        let entity = sim.substrate.entities.get(transport).unwrap();
        (entity.position.rx, entity.position.ry)
    };
    let passengers: Vec<u64> = (0..count)
        .map(|_| {
            sim.construct_object_limbo_at_height("E1", owner, rx, ry, 0, 0, rules)
                .expect("passenger")
        })
        .collect();
    for &id in passengers.iter().rev() {
        sim.substrate.entities.get_mut(id).unwrap().passenger_role = PassengerRole::Inside {
            transport_id: transport,
        };
        let cargo = sim
            .substrate
            .entities
            .get_mut(transport)
            .unwrap()
            .passenger_role
            .cargo_mut()
            .expect("a transport");
        assert!(cargo.board(id, 1));
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
    sim.commit_noncombat_aoe_hits(rules, None, &[hit]);
}

fn cargo_len(sim: &Simulation, transport: u64) -> usize {
    sim.substrate
        .entities
        .get(transport)
        .and_then(|entity| entity.passenger_role.cargo())
        .map_or(0, |cargo| cargo.passengers.len())
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
        assert!(sim.substrate.pending_delete.contains(&apc));
        assert_eq!(cargo_len(&sim, apc), 0);
        for &id in &passengers {
            let passenger = sim.substrate.entities.get(id).expect("escaped");
            assert!(passenger.lifecycle.object_alive && !passenger.lifecycle.in_limbo);
            assert!(matches!(passenger.passenger_role, PassengerRole::None));
            assert_eq!((passenger.position.rx, passenger.position.ry), (10, 10));
            assert_eq!(passenger.killed_by, None);
            assert!(!passenger.selected);
            assert_eq!(queued(&sim, id), mission, "{owner}");
            assert!(!sim.substrate.pending_delete.contains(&id));
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
        assert!(sim.substrate.pending_delete.contains(&id));
        assert!(!passenger.lifecycle.object_alive);
        assert_eq!(passenger.killed_by, Some(russians));
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
    let deleted = &sim.substrate.pending_delete;
    let order: Vec<usize> = passengers
        .iter()
        .chain([&boomer])
        .map(|id| deleted.iter().position(|d| d == id).expect("UnInit"))
        .collect();
    assert!(order[0] < order[1] && order[1] < order[2], "{deleted:?}");
    for id in passengers {
        assert_eq!(
            sim.substrate.entities.get(id).unwrap().killed_by,
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
        sim.substrate.entities.get_mut(ifv).unwrap().weapon_override =
            Some(WeaponOverride::IfvSlot(ifv_mode));
        kill_by(&mut sim, &rules, ifv, attacker, ORDINARY);
        let russians = sim.interner.intern("Russians");
        let passenger = sim.substrate.entities.get(passenger).unwrap();
        let bystander_hit = sim
            .substrate
            .entities
            .get(bystander)
            .unwrap()
            .health
            .current
            < 200;
        if explodes {
            assert!(!passenger.lifecycle.object_alive);
            assert_eq!(passenger.killed_by, Some(russians));
        } else {
            assert!(passenger.lifecycle.object_alive && !passenger.lifecycle.in_limbo);
            assert_eq!(passenger.killed_by, None);
        }
        assert_eq!(bystander_hit, explodes, "IFVMode {ifv_mode}");
    }
}

/// Retail `rulesmd.ini`: the stock IFV's death arm holds for exactly the
/// passengers whose IFVMode slot is a `Suicide=` weapon.
#[test]
fn retail_ifv_death_arm_follows_its_passengers_slot() {
    let Some((rules_ini, _)) = crate::rules::retail_ini_fixture::retail_rules_and_art() else {
        return;
    };
    let rules = RuleSet::from_ini(&rules_ini).unwrap();
    let fv = rules.object("FV").unwrap();
    let mut exploding: Vec<&str> = rules
        .infantry_ids
        .iter()
        .filter(|id| {
            let ifv_mode = i32::try_from(rules.object(id).unwrap().ifv_mode).unwrap();
            crate::sim::combat::death_arm_explodes(&rules, fv, 0, ifv_mode)
        })
        .map(String::as_str)
        .collect();
    exploding.sort_unstable();
    assert_eq!(exploding, ["CIVAN", "IVAN", "TERROR"]);
    assert!(!crate::sim::combat::death_arm_explodes(&rules, fv, 0, 0));
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
            sim.session.current_house = Some(owner);
            let apc = spawn(&mut sim, &rules, "APC", "AlliedHuman", 10, 10);
            let mcv = spawn(&mut sim, &rules, "AMCV", "AlliedHuman", 20, 10);
            let passengers = load_passengers(&mut sim, &rules, apc, "AlliedHuman", 2);
            for id in [apc, mcv] {
                sim.substrate.entities.get_mut(id).unwrap().selected = selected;
            }
            kill(&mut sim, &rules, apc, ORDINARY);
            let before = sim.substrate.entities.keys_sorted();
            kill(&mut sim, &rules, mcv, ORDINARY);
            for id in passengers.into_iter().chain(new_ids(&sim, &before)) {
                assert_eq!(sim.substrate.entities.get(id).unwrap().selected, selected);
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
    sim.session.current_house = Some(player);
    let apc = spawn(&mut sim, &rules, "APC", "Americans", 10, 10);
    let passengers = load_passengers(&mut sim, &rules, apc, "Americans", 2);
    sim.substrate.entities.get_mut(apc).unwrap().selected = true;
    kill(&mut sim, &rules, apc, ORDINARY);
    for id in passengers {
        let passenger = sim.substrate.entities.get(id).unwrap();
        assert!(passenger.lifecycle.object_alive && !passenger.lifecycle.in_limbo);
        assert!(!passenger.selected);
    }
}

/// Retail `rulesmd.ini` and `artmd.ini` (the local `ini/`): the stock crew
/// keys bind, and a power plant and an MCV killed through the production
/// receiver leave stock crew with native health ranges.
#[test]
fn retail_rules_crew_the_power_plant_and_the_mcv() {
    let Some((rules_ini, art_ini)) = crate::rules::retail_ini_fixture::retail_rules_and_art()
    else {
        return;
    };
    let mut rules = RuleSet::from_ini(&rules_ini).unwrap();
    rules.merge_art_data(&crate::rules::art_data::ArtRegistry::from_ini(&art_ini));
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
        assert_eq!(sim.building_survivor_count(&rules, plant, false), 1);
        assert_eq!(sim.building_survivor_count(&rules, soviet_plant, false), 2);
        assert_eq!(sim.building_survivor_count(&rules, yard, false), 5);
        let mcv = spawn(&mut sim, &rules, "AMCV", "Americans", 20, 10);
        let before = sim.substrate.entities.keys_sorted();
        kill_with(&mut sim, &rules, None, plant, "Super", ORDINARY);
        kill_with(&mut sim, &rules, None, mcv, "Super", ORDINARY);
        for id in new_ids(&sim, &before) {
            let entity = sim.substrate.entities.get(id).unwrap();
            if entity.category != EntityCategory::Infantry {
                continue;
            }
            assert_eq!(type_name(&sim, id), "E1");
            match (entity.position.rx, entity.position.ry) {
                (10..=11, 10..=11) => {
                    assert!((5..=125).contains(&entity.health.current));
                    plant_crew += 1;
                }
                (20, 10) => {
                    assert!((5..=62).contains(&entity.health.current));
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
            sim.spawn_object(
                "GAPOWR",
                "Americans",
                x,
                y,
                0,
                &resources.rules,
                &resources.height_map,
            )
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
            crate::util::config::GameConfig::load()
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
            .entry(owner)
            .or_insert_with(|| HouseState::new(owner, 0, None, false, 10_000, 10));
        if !sim.session.house_order.contains(&owner) {
            sim.session.house_order.push(owner);
        }
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
                let mcv = sim.spawn_object(
                    "AMCV",
                    "Americans",
                    x,
                    y,
                    0,
                    &resources.rules,
                    &resources.height_map,
                )?;
                let plant = sim.spawn_object(
                    "GAPOWR",
                    "Americans",
                    x - 3,
                    y,
                    0,
                    &resources.rules,
                    &resources.height_map,
                )?;
                Some((mcv, plant))
            })
            .expect("an MCV cell with room for a power plant");
        let (plant_cell, mcv_cell) = [plant, mcv]
            .map(|id| {
                let entity = sim.substrate.entities.get(id).unwrap();
                (entity.position.rx, entity.position.ry)
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
                sim.substrate.entities.get(id).unwrap().category == EntityCategory::Infantry
            })
            .map(|id| {
                let entity = sim.substrate.entities.get(id).unwrap();
                let spawn = (entity.position.rx, entity.position.ry);
                (
                    id,
                    spawn != mcv_cell,
                    spawn,
                    entity.navigation.nav_com.clone(),
                )
            })
            .collect();
        for _ in 0..45 {
            scenario.tick();
        }
        assert!(scenario.sim().substrate.entities.get(guard).is_some());
        assert!(!scenario.sim().houses[&owner].is_defeated);
        for (id, from_plant, spawn, destination) in crew {
            let entity = scenario
                .sim()
                .substrate
                .entities
                .get(id)
                .expect("crewman lives");
            let cell = (entity.position.rx, entity.position.ry);
            println!(
                "seed {seed:#x}: {} crewman {id} {} spawned {spawn:?} (plant {plant_cell:?}, MCV {mcv_cell:?}), Scatter to {destination:?}, at {cell:?} after 45 frames, health {}",
                if from_plant { "plant" } else { "MCV" },
                scenario.sim().interner.resolve(entity.type_ref()),
                entity.health.current
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
/// boarding, the transports die through the production receiver, and every GI
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
    use crate::sim::passenger::BoardingPhase;
    let dir = std::env::var("RA2_DIR")
        .ok()
        .filter(|path| !path.trim().is_empty())
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| {
            crate::util::config::GameConfig::load()
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
        .entry(owner)
        .or_insert_with(|| HouseState::new(owner, 0, None, false, 10_000, 10));
    if !sim.session.house_order.contains(&owner) {
        sim.session.house_order.push(owner);
    }
    // Each transport on the first central cell that admits it, with level
    // walkable ground around it for its GIs (the row south) and their
    // Scatter, four or more cells from the others.
    let mut loads: Vec<(&str, u64, (u16, u16), Vec<u64>)> = Vec::new();
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
                        terrain.cell(cx, cy).is_some_and(|cell| cell.level == level)
                            && grid.cell(cx, cy).is_some_and(|cell| cell.ground_walkable)
                    })
                }))
            };
            if !spaced || open() != Some(true) {
                continue;
            }
            if let Some(id) =
                sim.spawn_object(kind, "Americans", x, y, 0, rules, &resources.height_map)
            {
                placed = Some((id, (x, y)));
                break;
            }
        }
        let (transport, (tx, ty)) = placed.unwrap_or_else(|| panic!("{kind} spawns"));
        let gis = (0..count)
            .map(|i| {
                let id = sim
                    .spawn_object(
                        "E1",
                        "Americans",
                        tx - 1 + i % 3,
                        if i < 3 { ty + 1 } else { ty - 1 },
                        0,
                        rules,
                        &resources.height_map,
                    )
                    .expect("GI spawns");
                sim.substrate.entities.get_mut(id).unwrap().passenger_role =
                    PassengerRole::Boarding {
                        target_transport_id: transport,
                        phase: BoardingPhase::Entering,
                    };
                id
            })
            .collect();
        loads.push((kind, transport, (tx, ty), gis));
    }
    let guard = keep_undefeated(sim, resources, loads[0].2);
    sim.resolve_type_handles(rules);
    crate::sim::passenger::tick_passenger_system(sim, rules);
    for (kind, transport, _, gis) in &loads {
        assert_eq!(cargo_len(sim, *transport), gis.len(), "{kind} boarded");
    }
    assert!(
        sim.substrate
            .entities
            .get(loads[1].1)
            .unwrap()
            .weapon_override
            .is_some(),
        "the IFV took its gunner's weapon"
    );
    let registry = Some(&resources.overlay_registry);
    for (_, transport, _, _) in &loads {
        kill_with(sim, rules, registry, *transport, "Super", ORDINARY);
    }
    let mut destinations = Vec::new();
    for (kind, transport, cell, gis) in &loads {
        assert!(sim.substrate.pending_delete.contains(transport));
        for &id in gis {
            let gi = sim.substrate.entities.get(id).expect("GI lives");
            assert!(
                gi.lifecycle.object_alive && !gi.lifecycle.in_limbo,
                "{kind}"
            );
            assert!(matches!(gi.passenger_role, PassengerRole::None));
            assert_eq!(
                (gi.position.rx, gi.position.ry),
                *cell,
                "{kind}: out on its cell"
            );
            let Some(crate::sim::components::NavTargetRef::Cell { rx, ry }) =
                gi.navigation.nav_com.clone()
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
            left[index] |= (gi.position.rx, gi.position.ry) != *wreck;
        }
    }
    assert!(scenario.sim().substrate.entities.get(guard).is_some());
    assert!(!scenario.sim().houses[&owner].is_defeated);
    for ((kind, id, wreck, destination), left) in destinations.into_iter().zip(left) {
        println!("{kind} GI {id}: wreck {wreck:?}, Scatter to {destination:?}");
        assert!(left, "{kind}: GI {id} walked off the wreck");
    }
}
