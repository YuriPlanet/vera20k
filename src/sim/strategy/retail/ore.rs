//! Where the ore is, read once and shared.
//!
//! A harvester's whole job is a route between two places, and the house had no
//! reading of either. It knew what a bay holds and how many trucks serve it, and
//! nothing about the ground those trucks drive to: the wishlist's "occupy the ore
//! field and put a second base by it" and "micro-manage the harvesters instead of
//! letting them wander into an enemy base" both start here.
//!
//! ## The reading
//!
//! Ore is an overlay, and an overlay is a type: `[OverlayTypes]` gives each id a
//! section, and the section's `Tiberium=` is what makes it ore rather than a
//! wall, a fence or a crate. So the question "is this cell ore" is answered the
//! way the rules answer it, with no id hard-coded — a mod that renames the ore
//! overlay or adds a second one is read correctly, and `ORE_OVERLAY_ID = 102` in
//! the pathfinding tests stays what it always was: one fixture's number.
//!
//! ## Why a scan and not a query
//!
//! The engine has no "nearest ore" query to ask: ore is a grid of overlay ids and
//! finding a field is a search over it. The search is bounded (a ring around the
//! base), stepped (every second cell, because an ore field is measured in cells,
//! not in single tiles) and cached (a field does not move, and a truck takes
//! seconds to cross one). Recomputed on a term, so the map cannot be read every
//! frame for the whole match.

use crate::rules::ruleset::RuleSet;
use crate::sim::control::observation::Observation;

use super::advance::cell_distance;

/// How far from the base the ore search looks, in cells.
///
/// A field past this is a field the house would need a second base to work, and
/// that is `expand`'s question rather than this one.
pub(super) const SCAN_CELLS: i32 = 40;

/// Every second cell. An ore field is a blob, and reading each of its tiles adds
/// cost without changing the answer.
const SCAN_STEP: i32 = 2;

/// How long an ore scan stands, in frames: two minutes.
///
/// Long enough that the scan is not the frame's cost, short enough that a field
/// worked out and a field opened by a crate both register.
pub(super) const SCAN_TERM: u64 = 1_800;

/// Whether this cell carries ore.
pub(super) fn at(rules: &RuleSet, obs: &Observation<'_>, cell: (u16, u16)) -> bool {
    let Some(id) = obs.overlay_id_at(cell.0, cell.1) else {
        return false;
    };
    rules
        .overlay_registry()
        .flags(id)
        .is_some_and(|flags| flags.tiberium)
}

/// Every ore cell within [`SCAN_CELLS`] of `center`, in a fixed order.
pub(super) fn cells_near(
    rules: &RuleSet,
    obs: &Observation<'_>,
    center: (u16, u16),
    extent: (u16, u16),
) -> Vec<(u16, u16)> {
    let mut found = Vec::new();
    let mut radius = SCAN_STEP;
    while radius <= SCAN_CELLS {
        for dx in (-radius..=radius).step_by(SCAN_STEP as usize) {
            for dy in (-radius..=radius).step_by(SCAN_STEP as usize) {
                // The ring only: a filled square would re-read every inner cell
                // once per ring.
                if dx.abs() != radius && dy.abs() != radius {
                    continue;
                }
                let (Some(x), Some(y)) = (
                    offset(center.0, dx, extent.0),
                    offset(center.1, dy, extent.1),
                ) else {
                    continue;
                };
                if at(rules, obs, (x, y)) {
                    found.push((x, y));
                }
            }
        }
        radius += SCAN_STEP;
    }
    found
}

/// How much ore stands near `center`: the count of ore cells in the scan.
///
/// A count rather than a total, because the overlay says only that a cell is ore
/// — how much is left in it is the engine's own bookkeeping, and inventing a
/// per-cell quantity here would be a second authority.
pub(super) fn richness(
    rules: &RuleSet,
    obs: &Observation<'_>,
    center: (u16, u16),
    extent: (u16, u16),
) -> usize {
    cells_near(rules, obs, center, extent).len()
}

/// `base + delta`, or `None` when that leaves the map.
fn offset(base: u16, delta: i32, extent: u16) -> Option<u16> {
    let value = i32::from(base).checked_add(delta)?;
    (0..i32::from(extent))
        .contains(&value)
        .then(|| u16::try_from(value).ok())
        .flatten()
}

/// The nearest ore cell to `from` among `cells`, or `None` when there is none.
pub(super) fn nearest(from: (u16, u16), cells: &[(u16, u16)]) -> Option<(u16, u16)> {
    cells
        .iter()
        .copied()
        .min_by_key(|cell| (cell_distance(from, *cell), cell.0, cell.1))
}
