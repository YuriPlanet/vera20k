//! A computer house's miner after a failed ore search: Mission_Harvest state 4
//! (`0x0073EEA6..0x0073EF71`) parks it on Guard, and Mission_Guard's AI arm
//! (`0x00740881..0x00740942`) keeps it there while `House+0x242` is set.

use super::ResourceType;
use super::pad_nudge_tests::{scene, spawn};
use crate::sim::mission::{MissionId, MissionType};
use crate::sim::tiberium::test_support::{
    bales_at, overlay_registry_with_land, place_tiberium_on_map,
};

/// No ore in reach: the scan miss sets the house's latch (`0x0073E911`) and
/// 105 frames later state 4 drives the miner off the refinery pad and queues
/// Guard. Ore that appears later stays untouched while the latch holds; with
/// the latch cleared, Guard re-queues Harvest (`0x00740922`).
#[test]
fn a_computer_miner_that_finds_no_ore_parks_on_guard() {
    let Some((mut sim, rules, grid)) = scene() else {
        return;
    };
    let russians = sim.interner.get("Russians").unwrap();
    sim.overlay_grid = Some(crate::sim::overlay_grid::OverlayGrid::new(40, 40));
    // The human house keeps a building, so the match does not end.
    spawn(&mut sim, &rules, "GAPOWR", "Americans", 30, 30);
    spawn(&mut sim, &rules, "GAREFN", "Russians", 10, 10);
    // Unlimbo marks the miner into the pad Cell, as docking leaves it.
    let miner = spawn(&mut sim, &rules, "HARV", "Russians", 13, 11);
    let _ =
        sim.substrate
            .mission_assign_exact(miner, MissionId::from_known(MissionType::Harvest), 100);
    let frames = |sim: &mut crate::sim::world::Simulation, count: u32| {
        for _ in 0..count {
            let commands = sim.take_due_commands();
            sim.advance_tick(
                &commands,
                Some(&rules),
                Some(&grid),
                Some(overlay_registry_with_land()),
                33,
            );
        }
    };
    let mission = |sim: &crate::sim::world::Simulation| {
        sim.substrate.entities.get(miner).unwrap().mission.current()
    };
    let guard = MissionId::from_known(MissionType::Guard);

    // The first state 4 (frame 205) drives the miner off the pad. Its stop
    // there runs Enter_Idle_Mode, whose harvester arm (`0x00738BD8`) queues
    // Harvest once more without the latch test; that scan misses too, and the
    // second state 4 parks it where it stands.
    frames(&mut sim, 500);
    assert!(sim.houses[&russians].harvester_no_ore());
    assert_eq!(mission(&sim), guard);
    let unit = sim.substrate.entities.get(miner).unwrap();
    assert_ne!(
        (unit.position.rx(), unit.position.ry()),
        (13, 11),
        "state 4 drives the miner off the refinery pad"
    );

    let ore = (unit.position.rx() + 2, unit.position.ry());
    place_tiberium_on_map(&mut sim, ore, ResourceType::Ore, 11);
    frames(&mut sim, 600);
    assert_eq!(mission(&sim), guard);
    assert_eq!(bales_at(&sim, ore.0, ore.1), 11);

    sim.houses
        .get_mut(&russians)
        .unwrap()
        .set_harvester_no_ore_for_test(false);
    let mut harvested = false;
    for _ in 0..400 {
        frames(&mut sim, 1);
        harvested |= mission(&sim) == MissionId::from_known(MissionType::Harvest);
    }
    assert!(
        harvested,
        "with the latch clear, Guard re-queues Harvest (0x00740922)"
    );
    assert!(
        bales_at(&sim, ore.0, ore.1) < 11,
        "the miner harvests the ore"
    );
}
