//! Deciding where the ramps go: the pass that drives the carve routines.
//!
//! For every pair of adjacent regions that sit at different heights, this rolls
//! how many ramps to cut between them and then hunts along their shared border
//! for places one will fit.
//!
//! Two things are worth knowing before reading it:
//!
//! - **The connection count is not a uniform 1-to-3.** It is 1 whenever the
//!   accessibility roll fails and 2 or 3 when it succeeds, which is a very
//!   different distribution. An accessibility of zero does not switch the pass
//!   off — it makes every pair get exactly one ramp.
//! - **A pair is visited once, from the lower id.** Combined with the ascending
//!   neighbour order this fixes the visit order completely.
//!
//! Water-class regions take the active low-deck branch. That branch owns its
//! own seed rejection draws and conditional end-piece coins; dry regions keep
//! the ramp order described above.

use crate::map::rmg::rng::RmgRng;
use crate::map::rmg::x87::TruncF64;
use crate::util::native_x87::X87Chop53;

use super::adjacency;
use super::bridge_deck;
use super::carve::{CarveCtx, CarveRegions, try_carve_connector_at_cell};
use super::connector::LENIENCY_STEP;
use crate::map::rmg::RmgConstructionTrace;

/// `101 * (1 + 2^-32) * 2^-32` — the accessibility roll, pre-divided.
const ACCESS_ROLL_SCALE_BITS: u64 = 0x3E59_4000_0019_4000;
/// `2 * (1 + 2^-32) * 2^-32` — the extra-connections roll, which then has 1.0
/// added before truncation.
const EXTRA_ROLL_SCALE_BITS: u64 = 0x3E00_0000_0010_0000;
/// `(1 + 2^-32) * 2^-32` — the border-cell pick, which multiplies by the span
/// separately rather than folding it in.
const BORDER_PICK_SCALE_BITS: u64 = 0x3DF0_0000_0010_0000;

/// Highest value the accessibility roll may take.
const ACCESS_ROLL_MAX: i32 = 100;
/// Highest value the extra-connections roll may take.
const EXTRA_ROLL_MAX: i32 = 2;
/// Attempts allowed per region pair before giving up.
const MAX_ATTEMPTS: i32 = 100;

/// What the driver needs to know about each region.
#[derive(Debug, Clone)]
pub struct ConnectorRegion {
    pub id: i32,
    pub level: u8,
    /// The flood's class flag. Water-class regions take the bridge branch.
    pub waterish: bool,
    pub cell_count: i32,
    /// Ascending ids captured for every region before any connector mutates
    /// the grid/scratch state.
    pub neighbours: Vec<i32>,
}

/// How many ramps to cut between one pair.
///
/// One roll decides whether the pair is "accessible"; only on success is a
/// second roll taken, and it adds one or two. So the result is 1 on failure and
/// 2 or 3 on success — **never a flat 1-to-3**. Rewriting it as one uniform
/// would keep the range and change every map.
fn connection_count(rng: &mut RmgRng, accessibility: i32) -> i32 {
    let access_scale = TruncF64::from_f64(f64::from_bits(ACCESS_ROLL_SCALE_BITS));
    let roll = loop {
        let v = X87Chop53::ftol_f64_low_masked(
            TruncF64::from_f64(f64::from(rng.next_u32()))
                .mul(access_scale)
                .to_f64(),
        );
        if v <= ACCESS_ROLL_MAX {
            break v;
        }
    };

    // Signed compare: a non-positive accessibility fails every roll and the
    // pair still gets its one ramp.
    if roll >= accessibility {
        return 1;
    }

    let extra_scale = TruncF64::from_f64(f64::from_bits(EXTRA_ROLL_SCALE_BITS));
    let one = TruncF64::from_f64(1.0);
    let extra = loop {
        let v = X87Chop53::ftol_f64_low_masked(
            TruncF64::from_f64(f64::from(rng.next_u32()))
                .mul(extra_scale)
                .add(one)
                .to_f64(),
        );
        if v <= EXTRA_ROLL_MAX {
            break v;
        }
    };
    extra + 1
}

/// Pick one cell from the shared border, uniformly.
fn pick_border_cell(rng: &mut RmgRng, count: i32) -> usize {
    let scale = TruncF64::from_f64(f64::from_bits(BORDER_PICK_SCALE_BITS));
    let span = TruncF64::from_f64(f64::from(count));
    loop {
        let v = X87Chop53::ftol_f64_low_masked(
            TruncF64::from_f64(f64::from(rng.next_u32()))
                .mul(span)
                .mul(scale)
                .to_f64(),
        );
        if v < count {
            return v as usize;
        }
    }
}

/// Cut ramps between `region` and each higher-id neighbour at a different
/// height.
///
/// Returns whether anything was carved for this region at all — the original
/// records that on the region and nothing has been found to read it back, so it
/// is handed to the caller rather than stored.
pub(crate) fn carve_connectors_for_region(
    ctx: &mut CarveCtx<'_>,
    regions: &[ConnectorRegion],
    region: &ConnectorRegion,
    accessibility: i32,
    structures: &mut Vec<(String, i16, i16)>,
    trace: &mut RmgConstructionTrace,
) -> bool {
    if region.waterish {
        let mut placed_any = false;
        for (first_id, second_id) in qualified_deck_pairs(regions, region) {
            placed_any |= bridge_deck::place_low_bridge_deck(
                ctx, region.id, first_id, second_id, structures, trace,
            );
        }
        return placed_any;
    }

    let mut carved_any = false;
    for &neighbour_id in &region.neighbours {
        let Some(neighbour) = regions.iter().find(|r| r.id == neighbour_id) else {
            continue;
        };
        // Each pair is visited once, from the lower id, and only where the two
        // sit at different heights — a ramp joins levels, so equal levels have
        // nothing to join.
        if region.id >= neighbour.id || region.level == neighbour.level {
            continue;
        }

        let connections = connection_count(ctx.rng, accessibility);

        // The uphill region owns the border cells the ramp is cut from; the
        // downhill one is what the carve reaches down into.
        let (uphill, downhill) = if region.level < neighbour.level {
            (neighbour, region)
        } else {
            (region, neighbour)
        };
        let border = adjacency::border_cells_of(ctx.scratch, uphill.id);
        if border.is_empty() {
            continue;
        }

        let carve_regions = CarveRegions {
            region: region.id,
            level: region.level,
            lower_region: downhill.id,
            lower_level: downhill.level,
        };

        let mut successes = 0;
        let mut attempt = 0;
        while attempt < MAX_ATTEMPTS && successes < connections {
            let cell = border[pick_border_cell(ctx.rng, border.len() as i32)];
            // Leniency runs 0.00, 0.01 ... 0.99 off the ZERO-based attempt
            // index, so the first try is the strictest.
            let leniency = attempt as f32 * LENIENCY_STEP;
            if ctx.playfield.contains(cell.0 as u16, cell.1 as u16)
                && try_carve_connector_at_cell(ctx, carve_regions, cell, leniency)
            {
                successes += 1;
            }
            attempt += 1;
        }
        carved_any |= successes > 0;
    }
    carved_any
}

fn qualified_deck_pairs(regions: &[ConnectorRegion], flood: &ConnectorRegion) -> Vec<(i32, i32)> {
    if !flood.waterish {
        return Vec::new();
    }
    let mut pairs = Vec::new();
    for first_index in 0..flood.neighbours.len().saturating_sub(1) {
        let Some(first) = regions
            .iter()
            .find(|candidate| candidate.id == flood.neighbours[first_index])
        else {
            continue;
        };
        // gamemd 0x00590648..0x0059065f checks only substantiality
        // for the first ordered slot. Its class field is not read here.
        if first.neighbours.len() <= 1 && first.cell_count <= 50 {
            continue;
        }
        for &second_id in &flood.neighbours[first_index + 1..] {
            let Some(second) = regions.iter().find(|candidate| candidate.id == second_id) else {
                continue;
            };
            if second.waterish
                || (second.neighbours.len() <= 1 && second.cell_count <= 50)
                || first.level != second.level
                || first.level != flood.level
            {
                continue;
            }
            pairs.push((first.id, second.id));
        }
    }
    pairs
}

#[cfg(test)]
mod tests {
    use super::*;

    fn region(
        id: i32,
        level: u8,
        waterish: bool,
        cell_count: i32,
        neighbours: &[i32],
    ) -> ConnectorRegion {
        ConnectorRegion {
            id,
            level,
            waterish,
            cell_count,
            neighbours: neighbours.to_vec(),
        }
    }

    #[test]
    fn a_failed_accessibility_roll_still_gives_one_connection() {
        // Accessibility 0 fails every roll, and the pair gets exactly one ramp
        // rather than none. Treating 0 as "skip the pass" would strip ramps
        // off every map generated at the lowest setting.
        let mut rng = RmgRng::new(1);
        for _ in 0..16 {
            assert_eq!(connection_count(&mut rng, 0), 1);
        }
    }

    #[test]
    fn a_failed_roll_costs_one_draw_and_a_passed_roll_costs_two() {
        // The second roll is only taken on success. Taking it unconditionally
        // would spend a draw per pair that the original never spends.
        let mut rng = RmgRng::new(1);
        let _ = connection_count(&mut rng, 0);
        let mut probe = RmgRng::new(1);
        probe.next_u32();
        assert_eq!(rng.next_u32(), probe.next_u32(), "failure takes one draw");

        let mut rng = RmgRng::new(1);
        let _ = connection_count(&mut rng, 101);
        let mut probe = RmgRng::new(1);
        probe.next_u32();
        probe.next_u32();
        assert_eq!(rng.next_u32(), probe.next_u32(), "success takes two draws");
    }

    #[test]
    fn a_passed_roll_never_gives_one_and_never_gives_four() {
        // The distribution is the point: 2 or 3 on success, never a flat
        // 1-to-3. Accessibility 101 passes every roll.
        let mut rng = RmgRng::new(7);
        let mut seen = [false; 4];
        for _ in 0..64 {
            let n = connection_count(&mut rng, 101);
            assert!((2..=3).contains(&n), "got {n}");
            seen[n as usize] = true;
        }
        assert!(seen[2] && seen[3], "both outcomes occur");
    }

    #[test]
    fn the_border_pick_stays_inside_the_list() {
        let mut rng = RmgRng::new(3);
        for _ in 0..64 {
            assert!(pick_border_cell(&mut rng, 5) < 5);
        }
        assert_eq!(pick_border_cell(&mut rng, 1), 0, "a single cell is index 0");
    }

    #[test]
    fn flood_branch_visits_exact_qualified_ordered_slot_pairs_in_order() {
        let flood = region(0, 4, true, 90, &[1, 2, 3, 4, 5, 6]);
        let regions = vec![
            flood.clone(),
            // Exactly one neighbour and 50 cells: not substantial.
            region(1, 4, false, 50, &[0]),
            // Two neighbours qualifies at the count boundary.
            region(2, 4, false, 1, &[0, 9]),
            // 51 cells qualifies with one neighbour.
            region(3, 4, false, 51, &[0]),
            // Water is allowed in the first ordered slot, but excluded when it
            // appears as the second slot of an earlier pair.
            region(4, 4, true, 90, &[0, 9]),
            // Wrong level is excluded.
            region(5, 5, false, 90, &[0, 9]),
            region(6, 4, false, 90, &[0, 9]),
        ];
        assert_eq!(
            qualified_deck_pairs(&regions, &flood),
            vec![(2, 3), (2, 6), (3, 6), (4, 6)]
        );
        assert!(qualified_deck_pairs(&regions, &regions[2]).is_empty());
    }

    #[test]
    fn flood_pair_class_gate_is_second_neighbor_only() {
        let flood = region(0, 4, true, 90, &[1, 2]);
        let water = region(1, 4, true, 90, &[0, 9]);
        let land = region(2, 4, false, 90, &[0, 9]);
        let other_land = region(3, 4, false, 90, &[0, 9]);
        let regions = vec![flood.clone(), water, land, other_land];

        assert_eq!(qualified_deck_pairs(&regions, &flood), vec![(1, 2)]);

        let reverse = region(0, 4, true, 90, &[2, 1]);
        assert!(qualified_deck_pairs(&regions, &reverse).is_empty());

        let both_land = region(0, 4, true, 90, &[2, 3]);
        assert_eq!(qualified_deck_pairs(&regions, &both_land), vec![(2, 3)]);
    }
}
