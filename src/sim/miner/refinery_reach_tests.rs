//! The refinery scanner's zone gate (`FUN_004DEE80`, `0x004DEF29..0x004DEFDD`):
//! `MapClass::Can_Reach_Zone` from the cell of the miner's `vt+0x4C`
//! coordinate to the cell of the refinery's GetCoords.

use super::ResourceType;
use super::miner_system::find_docking_bay;
use super::pad_nudge_tests::{scene, scene_with_size, spawn};
use crate::sim::tiberium::test_support::place_tiberium_on_map;
use crate::sim::world::FrameEffects;

/// A War Miner standing in an ore field, a refinery at NW (22, 10) and a
/// farther one at NW (4, 24) on the miner's ground, with the cells `water`
/// flooded after placement. Answers the refinery the scanner picks and the
/// two candidates.
fn pick(water: impl IntoIterator<Item = (u16, u16)>) -> Option<(Option<u64>, u64, u64)> {
    let (mut sim, rules, _) = scene()?;
    let near = spawn(&mut sim, &rules, "NAREFN", "Russians", 22, 10);
    let far = spawn(&mut sim, &rules, "NAREFN", "Russians", 4, 24);
    let miner = spawn(&mut sim, &rules, "HARV", "Russians", 14, 12);
    crate::sim::arena_fixture::flood(&mut sim, &rules, 40, water);
    sim.overlay_grid = Some(crate::sim::overlay_grid::OverlayGrid::new(40, 40));
    for y in 11..=13 {
        for x in 13..=15 {
            place_tiberium_on_map(&mut sim, (x, y), ResourceType::Ore, 5);
        }
    }
    assert!(sim.rebuild_dynamic_navigation(&rules));
    let found = find_docking_bay(
        &mut sim,
        &rules,
        miner,
        &rules.object("HARV").unwrap().dock,
        false,
        false,
        &mut crate::sim::world::FrameEffects::default(),
    );
    Some((found, near, far))
}

/// Across water the nearer refinery's zone is unreachable and the scanner
/// takes the farther one. The miner's own Cell is ore, which keeps its zone.
#[test]
fn a_miner_skips_a_refinery_across_water() {
    let Some((found, _, far)) = pick((0..40).flat_map(|y| (18..=20).map(move |x| (x, y)))) else {
        return;
    };
    assert_eq!(found, Some(far));
}

/// The gate measures to the refinery's GetCoords Cell, its foundation centre
/// (`0x00447AC0`: NW + (W·128 − 128, H·128 − 128) leptons, so (24, 11) for
/// the 4x3 NAREFN), not its dock pad (25, 11). Water under that one Cell
/// alone puts the nearer refinery out of reach.
#[test]
fn the_zone_gate_reads_the_refinerys_centre_cell() {
    let Some((found, _, far)) = pick([(24, 11)]) else {
        return;
    };
    assert_eq!(found, Some(far));
}

/// VERA's map-size extension accepts width plus height up to 1024, past the
/// original's 512. On a 300x300 map the 230-cell separation below wraps a
/// signed 32-bit squared distance negative, so the farther refinery reads as
/// nearer. The scanner must take the truly nearest one on a map the original
/// cannot load.
#[test]
fn an_extended_map_picks_the_true_nearest_refinery() {
    let Some((mut sim, rules, _)) = scene_with_size(300) else {
        return;
    };
    // The session stores `[Map] Size` width plus height as the canonical
    // cell-array extent; 300 + 300 is past the original's 512.
    sim.session.set_map_width_for_test(600);
    sim.session.set_map_height_for_test(600);
    // The shared arena's fixed playfield stops short of an extended map's
    // eastern and southern cells, which would refuse the far refinery.
    sim.set_playfield_bounds_for_test(Some(crate::map::playfield::PlayfieldBounds {
        base: 0,
        off_fc: -1000,
        off_100: -1000,
        off_104: 2000,
        off_108: 2000,
    }));
    let near = spawn(&mut sim, &rules, "NAREFN", "Russians", 20, 20);
    let _far = spawn(&mut sim, &rules, "NAREFN", "Russians", 235, 235);
    let miner = spawn(&mut sim, &rules, "HARV", "Russians", 5, 5);
    assert!(sim.rebuild_dynamic_navigation(&rules));
    let found = find_docking_bay(
        &mut sim,
        &rules,
        miner,
        &rules.object("HARV").unwrap().dock,
        false,
        false,
        &mut FrameEffects::default(),
    );
    assert_eq!(found, Some(near));
}

/// A map the original reads (width plus height 512 or less) keeps the native
/// 32-bit distance, even where it wraps: stock play stays as gamemd plays it.
/// On a 256x256 map the same far separation wraps negative, so the scanner
/// still picks the farther refinery.
#[test]
fn a_native_readable_map_keeps_the_original_wrapped_distance() {
    let Some((mut sim, rules, _)) = scene_with_size(256) else {
        return;
    };
    // 256 + 256 is exactly the original's 512 sum boundary.
    sim.session.set_map_width_for_test(512);
    sim.session.set_map_height_for_test(512);
    sim.set_playfield_bounds_for_test(Some(crate::map::playfield::PlayfieldBounds {
        base: 0,
        off_fc: -1000,
        off_100: -1000,
        off_104: 2000,
        off_108: 2000,
    }));
    let _near = spawn(&mut sim, &rules, "NAREFN", "Russians", 20, 20);
    let far = spawn(&mut sim, &rules, "NAREFN", "Russians", 251, 251);
    let miner = spawn(&mut sim, &rules, "HARV", "Russians", 5, 5);
    assert!(sim.rebuild_dynamic_navigation(&rules));
    let found = find_docking_bay(
        &mut sim,
        &rules,
        miner,
        &rules.object("HARV").unwrap().dock,
        false,
        false,
        &mut FrameEffects::default(),
    );
    assert_eq!(found, Some(far));
}
