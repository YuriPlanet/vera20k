//! Phobos `Harvester.CanGuardArea` (`Ext/Unit/Hooks.Harvester.cpp:71-90`)
//! through the Foot Area Guard dispatcher.

use crate::rules::ini_parser::IniFile;
use crate::rules::ruleset::RuleSet;
use crate::sim::house_state::HouseState;
use crate::sim::mission::state::MissionTestFixture;
use crate::sim::mission::{MissionDispatchTimer, MissionId, MissionType};
use crate::sim::world::{ObjectAiCtx, Simulation};

fn rules(miner_extra: &str) -> RuleSet {
    RuleSet::from_ini(&crate::sim::arena_fixture::clear_ground_ini(&format!(
        "[VehicleTypes]\n0=MINER\n\
         [MINER]\nStrength=300\nSight=8\nSpeed=4\nHarvester=yes\nPrimary=MinerGun\n{miner_extra}\
         [MinerGun]\nDamage=10\nRange=6\nROF=20\nProjectile=Invisible\nWarhead=GUARDWH\n\
         [Warheads]\n0=GUARDWH\n\
         [GUARDWH]\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n"
    )))
    .expect("harvester guard rules")
}

fn world(rules: &RuleSet, human: bool) -> Simulation {
    let mut sim = Simulation::new();
    crate::sim::arena_fixture::flat_ground(&mut sim, rules);
    let id = sim.intern("Americans");
    sim.houses
        .register(HouseState::new(id, 0, None, human, 0, 10));
    sim
}

fn area_guard(sim: &mut Simulation, id: u64) {
    sim.substrate
        .entities
        .get_mut(id)
        .unwrap()
        .mission
        .apply_test_fixture(MissionTestFixture {
            current: MissionId::from_known(MissionType::AreaGuard),
            suspended: MissionId::NONE,
            queued: MissionId::NONE,
            movement_bypass_latch: 0,
            handler_state: 0,
            mission_start_frame: 0,
            ai_counter: 0,
            dispatch_timer: MissionDispatchTimer::at_frame(0),
        });
}

/// One dispatcher visit's committed mission and the frame its passive scan
/// last stamped, for a harvester set on Area Guard. The visit runs at frame
/// 5, so a stamp of 5 proves a scan ran and a stamp of 0 proves none did.
fn visited_mission(miner_extra: &str, human: bool) -> (MissionType, u32) {
    let rules = rules(miner_extra);
    let mut sim = world(&rules, human);
    let miner = sim
        .spawn_object("MINER", "Americans", 10, 10, 0, &rules)
        .expect("MINER spawns");
    area_guard(&mut sim, miner);
    sim.session.set_binary_frame_for_test(5);
    crate::sim::world::dispatch_foot_mission(&mut sim, miner, &rules, ObjectAiCtx::default())
        .expect("the visit runs");
    let entity = sim.substrate.entities.get(miner).unwrap();
    (
        entity
            .mission
            .current()
            .known()
            .expect("a committed mission"),
        entity.passive_scan.last_scan_frame(),
    )
}

/// `0x4D6D34`: only a human-owned harvester whose type sets the key skips
/// the stock `Queue_Mission(Harvest, 0)` switch.
#[test]
fn a_human_harvester_with_the_key_keeps_area_guard() {
    assert_eq!(
        visited_mission("Harvester.CanGuardArea=yes\n", true),
        (MissionType::AreaGuard, 0),
        "keeping Area Guard must not scan"
    );
    assert_eq!(
        visited_mission("", true),
        (MissionType::Harvest, 0),
        "without the key the stock switch runs without a scan"
    );
    assert_eq!(
        visited_mission("Harvester.CanGuardArea=yes\n", false),
        (MissionType::Harvest, 0),
        "a computer harvester keeps the stock switch"
    );
}

/// `RequireTarget`: with no threat in range the gate refuses and the stock
/// Harvest switch runs. The synthetic fixture cannot form a scanner
/// candidate; the retail-rules test below covers the positive "threat keeps
/// Area Guard" arm.
#[test]
fn require_target_without_a_threat_falls_back_to_harvest() {
    assert_eq!(
        visited_mission(
            "Harvester.CanGuardArea=yes\nHarvester.CanGuardArea.RequireTarget=yes\n",
            true
        ),
        (MissionType::Harvest, 5),
        "no threat: the scan ran and refused, then Harvest started"
    );
}

/// The positive arm on retail rules (skipped when the checkout has no retail
/// INIs): an armed `MTNK` patched into a harvester with the key keeps Area
/// Guard while an enemy is inside the scan.
#[test]
fn require_target_keeps_area_guard_with_a_threat_on_retail_rules() {
    let patch = IniFile::from_str(
        "[MTNK]\nHarvester=yes\nHarvester.CanGuardArea=yes\n\
         Harvester.CanGuardArea.RequireTarget=yes\n",
    );
    let Some(retail) = crate::rules::retail_ini_fixture::retail_battle_rules_with_map_patch(&patch)
    else {
        return;
    };
    let rules = retail.rules;
    let mut sim = Simulation::new();
    crate::sim::arena_fixture::flat_ground(&mut sim, &rules);
    for (name, side, human) in [("Americans", 0, true), ("Russians", 1, false)] {
        let id = sim.intern(name);
        sim.houses
            .register(HouseState::new(id, side, None, human, 0, 10));
    }
    sim.spawn_object("HTNK", "Russians", 12, 10, 0, &rules)
        .expect("HTNK spawns");
    let miner = sim
        .spawn_object("MTNK", "Americans", 10, 10, 0, &rules)
        .expect("MTNK spawns");
    area_guard(&mut sim, miner);
    crate::sim::world::dispatch_foot_mission(&mut sim, miner, &rules, ObjectAiCtx::default())
        .expect("the visit runs");
    assert_eq!(
        sim.substrate
            .entities
            .get(miner)
            .unwrap()
            .mission
            .current()
            .known(),
        Some(MissionType::AreaGuard),
        "the enemy in range keeps Area Guard"
    );
}
