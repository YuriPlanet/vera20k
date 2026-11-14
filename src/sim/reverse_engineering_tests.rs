//! Ares grinder reverse engineering (100498D0/1004C460 -> 10012E00 ->
//! 10022360) and the vehicle arm's hijacker refund, through the Eaten
//! arrival transaction.

use crate::rules::art_data::ArtRegistry;
use crate::rules::ini_parser::IniFile;
use crate::rules::object_type::ObjectCategory;
use crate::rules::ruleset::RuleSet;
use crate::sim::combat::TargetKind;
use crate::sim::mission::state::MissionTestFixture;
use crate::sim::mission::{MissionDispatchTimer, MissionId, MissionType};
use crate::sim::movement::locomotor::MovementLayer;
use crate::sim::world::{FrameEffects, SimSoundEvent, Simulation};

const TRACK: &str = "Locomotor={4A582741-9839-11D1-B709-00A024DDAFD1}\nSpeed=6\nSpeedType=Track\n";

fn fixture(grinder_keys: &str) -> (Simulation, RuleSet) {
    let ini = crate::sim::arena_fixture::clear_ground_ini(&format!(
        "[General]\nRefundPercent=50%\n\
         [Countries]\n0=Russians\n1=Americans\n\
         [InfantryTypes]\n0=CIV\n[VehicleTypes]\n0=TANK\n1=HEAVY\n2=PLAIN\n\
         [BuildingTypes]\n0=GRINDER\n\
         [CIV]\nImage=GI\nStrength=100\nCost=100\nSoylent=75\n\
         [TANK]\nStrength=300\nCost=600\nSoylent=300\nReversedAs=HEAVY\n{TRACK}\
         [HEAVY]\nStrength=600\nCost=1200\nPrerequisite=GRINDER\n{TRACK}\
         [PLAIN]\nStrength=300\nCost=600\nCanBeReversed=no\n{TRACK}\
         [GRINDER]\nStrength=1000\nGrinding=yes\n{grinder_keys}\n"
    ));
    let art = IniFile::from_str(&format!(
        "{}\n[GRINDER]\nFoundation=1x1\n",
        crate::rules::retail_ini_fixture::GI_ART_EXCERPT,
    ));
    let mut rules = RuleSet::from_ini_with_fixed_art_for_test(&ini, &art).unwrap();
    rules.install_art_data(ArtRegistry::from_ini(&art));
    let sim = crate::sim::arena_fixture::rival_arena(&rules, 60);
    (sim, rules)
}

fn spawn(sim: &mut Simulation, rules: &RuleSet, kind: &str, x: u16) -> u64 {
    sim.spawn_object_at_height(kind, "Americans", x, 10, 0, 0, rules)
        .unwrap()
}

/// Runs `foot`'s Eaten arrival at `grinder`, standing on its cell's ground
/// list as the arrival requires.
fn grind(sim: &mut Simulation, rules: &RuleSet, foot: u64, grinder: u64) {
    let (rx, ry) = {
        let entity = sim.substrate.entities.get(foot).unwrap();
        (entity.position.rx(), entity.position.ry())
    };
    sim.substrate.occupancy.add(
        rx,
        ry,
        grinder,
        MovementLayer::Ground,
        None,
        crate::sim::occupancy::CellListInsertion::AppendBuilding,
    );
    let entity = sim.substrate.entities.get_mut(foot).unwrap();
    entity.mission.apply_test_fixture(MissionTestFixture {
        current: MissionId::from_known(MissionType::Eaten),
        queued: MissionId::NONE,
        suspended: MissionId::NONE,
        movement_bypass_latch: 0,
        handler_state: 0,
        mission_start_frame: 0,
        ai_counter: 0,
        dispatch_timer: MissionDispatchTimer::at_frame(0),
    });
    entity
        .attack_target
        .set_for_test(Some(TargetKind::Entity(grinder)));
    assert!(sim.foot_per_cell_eaten(foot, rules, None, FrameEffects::default()));
}

fn eva_lines(sim: &mut Simulation) -> Vec<&'static str> {
    sim.take_sound_events_for_test()
        .into_iter()
        .filter_map(|event| match event {
            SimSoundEvent::HouseEva { event, .. } => Some(event),
            _ => None,
        })
        .collect()
}

/// The first `ReversedAs=HEAVY` tank ground records HEAVY (not TANK) for
/// the grinder's owner, sets House+1FC and speaks the two lines for its
/// player; the second finds HEAVY listed and does neither.
// Runs again once building entry's grinder step moves to the switch.
#[cfg(feature = "ares")]
#[test]
fn a_grinder_reverse_engineers_a_vehicle_once_as_its_reversed_type() {
    let _ares = crate::rules::ares_switch::read_ares_way(true);
    let (mut sim, rules) = fixture("ReverseEngineersVictims=yes");
    let owner = sim.interner.get("Americans").unwrap();
    let grinder = spawn(&mut sim, &rules, "GRINDER", 14);
    let first = spawn(&mut sim, &rules, "TANK", 10);
    let second = spawn(&mut sim, &rules, "TANK", 11);
    sim.houses.get_mut(&owner).unwrap().take_tech_recheck();
    sim.take_sound_events_for_test();

    grind(&mut sim, &rules, first, grinder);
    let house = &sim.houses[&owner];
    assert!(house.has_ares_reverse_engineered(ObjectCategory::Vehicle, "HEAVY"));
    assert!(!house.has_ares_reverse_engineered(ObjectCategory::Vehicle, "TANK"));
    assert!(house.tech_recheck_pending());
    let spoken: &[&str] = if sim.house_is_human_player(owner) {
        &["EVA_ReverseEngineeredVehicle", "EVA_NewTechnologyAcquired"]
    } else {
        &[]
    };
    assert_eq!(eva_lines(&mut sim), spoken);

    sim.houses.get_mut(&owner).unwrap().take_tech_recheck();
    grind(&mut sim, &rules, second, grinder);
    assert!(!sim.houses[&owner].tech_recheck_pending());
    assert!(eva_lines(&mut sim).is_empty());
}

/// 10012E00 needs `ReverseEngineersVictims=` and 10022360 `CanBeReversed=`.
#[test]
fn a_plain_grinder_or_an_irreversible_victim_records_nothing() {
    let _ares = crate::rules::ares_switch::read_ares_way(true);
    for (grinder_keys, victim) in [("", "TANK"), ("ReverseEngineersVictims=yes", "PLAIN")] {
        let (mut sim, rules) = fixture(grinder_keys);
        let owner = sim.interner.get("Americans").unwrap();
        let grinder = spawn(&mut sim, &rules, "GRINDER", 14);
        let foot = spawn(&mut sim, &rules, victim, 10);
        grind(&mut sim, &rules, foot, grinder);
        let house = &sim.houses[&owner];
        for kind in ["TANK", "HEAVY", "PLAIN"] {
            assert!(!house.has_ares_reverse_engineered(ObjectCategory::Vehicle, kind));
        }
    }
}

/// #368: a vehicle with a captor InfantryType (Techno+0x338) also refunds
/// that type (CIV, Soylent 75) to the grinder's owner, after its own 300.
// Runs again once building entry's grinder step moves to the switch.
#[cfg(feature = "ares")]
#[test]
fn a_ground_hijacked_vehicle_refunds_its_hijacker() {
    let _ares = crate::rules::ares_switch::read_ares_way(true);
    for (captor, refund) in [(-1, 300), (0, 375)] {
        let (mut sim, rules) = fixture("");
        let owner = sim.interner.get("Americans").unwrap();
        let grinder = spawn(&mut sim, &rules, "GRINDER", 14);
        let tank = spawn(&mut sim, &rules, "TANK", 10);
        sim.substrate
            .entities
            .get_mut(tank)
            .unwrap()
            .capture_infantry_type_index
            .record(captor);
        let before = sim.houses[&owner].economy().credits();
        grind(&mut sim, &rules, tank, grinder);
        assert_eq!(sim.houses[&owner].economy().credits() - before, refund);
    }
}
