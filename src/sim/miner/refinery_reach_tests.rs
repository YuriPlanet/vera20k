//! The refinery scanner's zone gate (`FUN_004DEE80`, `0x004DEF29..0x004DEFDD`):
//! `MapClass::Can_Reach_Zone` from the cell of the miner's `vt+0x4C`
//! coordinate to the cell of the refinery's GetCoords.

use super::ResourceType;
use super::miner_system::find_docking_bay;
use super::pad_nudge_tests::{scene, spawn};
use crate::sim::tiberium::test_support::place_tiberium_on_map;

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
        crate::sim::world::FrameEffects::default(),
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
