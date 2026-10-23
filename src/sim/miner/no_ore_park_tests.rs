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

/// A refinery and its pad on a small Clear island, using stock Water to
/// separate the human house's island. Every remaining ground cell is filled
/// through Unlimbo, so NearbyLocation really exhausts its same-zone search.
fn blocked_pad() -> Option<(
    crate::sim::world::Simulation,
    crate::rules::ruleset::RuleSet,
    u64,
    u64,
)> {
    let (mut sim, rules, _) = scene()?;
    sim.session.game_options_mut_for_test().short_game = false;
    crate::sim::arena_fixture::flood(
        &mut sim,
        &rules,
        40,
        (0..40)
            .flat_map(|y| (0..40).map(move |x| (x, y)))
            .filter(|&(x, y)| {
                !((12..=16).contains(&x) && (13..=15).contains(&y)
                    || (22..=25).contains(&x) && (22..=25).contains(&y))
            }),
    );
    // The native 20x20 Size diamond in its 40x40 storage plane: (0,0)
    // names Dummy, while the two islands have ordinary allocated cells.
    let allocated: Vec<_> = crate::map::authored_overlay::NativeOverlayMapShape::new(20, 20)
        .recalc_cells()
        .into_iter()
        .map(|(x, y)| (x as u16, y as u16))
        .collect();
    let mut terrain = sim.resolved_terrain.take().unwrap();
    terrain.test_set_native_allocated_cells(&allocated);
    sim.install_resolved_terrain_for_new_map(terrain);
    sim.session.set_map_width_for_test(20);
    sim.session.set_map_height_for_test(20);
    sim.set_playfield_bounds_for_test(Some(
        crate::sim::cell_rect::PlayfieldBounds::from_normalized_local_size(20, 2, 2, 16, 12),
    ));
    sim.set_playfield_size_height_for_test(Some(20));
    assert!(sim.rebuild_dynamic_navigation(&rules));
    assert!(
        sim.resolved_terrain
            .as_ref()
            .unwrap()
            .native_fixed_cell_index(0, 0)
            .is_none()
    );
    sim.overlay_grid = Some(crate::sim::overlay_grid::OverlayGrid::new(40, 40));
    spawn(&mut sim, &rules, "GAPOWR", "Americans", 22, 22);
    let refinery = spawn(&mut sim, &rules, "GAREFN", "Russians", 12, 13);
    let miner = spawn(&mut sim, &rules, "HARV", "Russians", 15, 14);
    assert!(
        sim.techno_nearby_location(miner, Some(refinery), &rules)
            .is_some()
    );
    for y in 13..=15 {
        for x in 12..=16 {
            if let Some(blocker) =
                sim.spawn_object_at_height("HTNK", "Russians", x, y, 0, 0, &rules)
            {
                sim.substrate
                    .mission_assign_exact(blocker, MissionId::from_known(MissionType::Sleep), 100)
                    .unwrap();
            }
        }
    }
    assert_eq!(
        sim.techno_nearby_location(miner, Some(refinery), &rules),
        None
    );
    sim.substrate
        .mission_assign_exact(miner, MissionId::from_known(MissionType::Harvest), 100)
        .unwrap();
    Some((sim, rules, miner, refinery))
}

/// Mission_Harvest73EF42..73EF68 still passes fresh Map[0,0] to the Unit
/// setter on a failed departure search, then queues Guard. The no-ore entry
/// and its 105-frame timer run through the ordinary frame dispatcher.
#[test]
fn failed_harvest_departure_still_assigns_the_zero_cell() {
    use crate::sim::components::NavTargetRef;
    use crate::sim::world::{ObjectAiCtx, dispatch_foot_mission};

    let Some((mut sim, rules, miner, refinery)) = blocked_pad() else {
        return;
    };
    let grid = sim.path_grid_snapshot().unwrap();
    for _ in 0..105 {
        let commands = sim.take_due_commands();
        assert!(
            sim.advance_tick(
                &commands,
                Some(&rules),
                Some(&grid),
                Some(overlay_registry_with_land()),
                33,
            )
            .frame_committed
        );
    }
    let unit = sim.substrate.entities.get(miner).unwrap();
    assert_eq!(unit.mission.current().known(), Some(MissionType::Harvest));
    assert_eq!(unit.mission.handler_state(), 4);
    assert_eq!(unit.mission.dispatch_timer().delay(), 105);
    assert!(
        unit.mission
            .dispatch_timer()
            .due(sim.session.binary_frame())
    );
    assert_eq!(unit.navigation.nav_com(), None);
    assert_eq!(
        sim.techno_nearby_location(miner, Some(refinery), &rules),
        None
    );

    dispatch_foot_mission(
        &mut sim,
        miner,
        &rules,
        ObjectAiCtx {
            overlay_registry: Some(overlay_registry_with_land()),
            ..Default::default()
        },
    );
    let unit = sim.substrate.entities.get(miner).unwrap();
    assert_eq!(unit.navigation.nav_com(), Some(NavTargetRef::cell(0, 0)));
    assert_eq!(unit.mission.current().known(), Some(MissionType::Harvest));
    assert_eq!(unit.mission.queued().known(), Some(MissionType::Guard));
}

/// Unit AI73690A uses the same failed search but owns a different mission
/// suffix: break its contact, call the setter, then queue Move immediately.
#[test]
fn failed_pad_nudge_still_assigns_the_zero_cell() {
    use crate::sim::components::NavTargetRef;
    use crate::sim::radio::{RadioMessage, RadioPayload, RadioResponse, transmit};
    use crate::sim::world::FrameEffects;

    let Some((mut sim, rules, miner, refinery)) = blocked_pad() else {
        return;
    };
    assert_eq!(
        transmit(
            &mut sim,
            miner,
            refinery,
            RadioMessage::Hello,
            RadioPayload::default(),
            &rules,
            FrameEffects::default()
        ),
        RadioResponse::Roger
    );
    sim.session.set_binary_frame_for_test(401);
    super::miner_system::unit_ai_nudge_off_refinery_pad(
        &mut sim,
        miner,
        &rules,
        None,
        FrameEffects::default(),
    );
    let unit = sim.substrate.entities.get(miner).unwrap();
    assert!(unit.radio_contacts.is_empty());
    assert_eq!(unit.navigation.nav_com(), Some(NavTargetRef::cell(0, 0)));
    assert_eq!(unit.mission.effective().known(), Some(MissionType::Move));
}
