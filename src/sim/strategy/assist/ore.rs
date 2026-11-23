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
//! finding a field is a search over it. A slow whole-map pass updates only
//! currently visible ore stages, retaining earlier sightings without looking
//! through the fog. The survey then filters the known tiles by the reachable
//! unloading bays, distance and local threat. Cached for a short term, so a
//! map-wide query never runs in every brain frame.

use std::collections::BTreeMap;

use crate::rules::ruleset::RuleSet;
use crate::rules::locomotor_type::MovementZone;
use crate::sim::control::observation::Observation;

use super::advance::cell_distance;
use super::census::Census;

/// How far from the base the ore search looks, in cells.
///
/// A field past this is a field the house would need a second base to work, and
/// that is `expand`'s question rather than this one.
pub(super) const SCAN_CELLS: i32 = 40;

/// Every second cell. An ore field is a blob, and reading each of its tiles adds
/// cost without changing the answer.
const SCAN_STEP: i32 = 2;

/// A full map ore survey every twenty seconds, not every brain frame.
///
/// A field can deplete while the trucks work it; two minutes is long enough to
/// buy multiple harvesters against ore that is no longer there.
pub(super) const SCAN_TERM: u64 = 300;

/// Approximate growth stages one miner can work through before a replacement
/// is useful. This is a *purchase heuristic*, not a second harvesting rule:
/// the engine remains the authority on cargo capacity and credits delivered.
const DENSITY_PER_HARVESTER: u32 = 55;
/// Bays unload serially. Beyond three trucks per bay the cost of another one
/// usually buys more useful army instead. The density survey may request a
/// second bay if the map has enough reachable ore to keep it busy.
pub(super) const HARVESTERS_PER_BAY: usize = 3;
const MAX_RESOURCE_BAYS: usize = 3;

/// The desired economic capacity for the ore the house can actually work.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(super) struct Targets {
    pub(super) harvesters: usize,
    pub(super) refineries: usize,
}

/// Produce an ore-bound target, allowing for longer collection trips without
/// assuming that a rich-looking map can support an infinite number of trucks.
/// `stock` is the sum of the native growth stages + 1 on reachable ore tiles.
/// It is not a currency value: mods can change the value of ore/gems.
pub(super) fn targets(stock: u32, mean_distance: u32, refineries: usize) -> Targets {
    if stock == 0 {
        // The first refinery is an opening prerequisite, not an expansion.
        return Targets { harvesters: 0, refineries: 1 };
    }
    let basic = stock.saturating_add(DENSITY_PER_HARVESTER - 1) / DENSITY_PER_HARVESTER;
    let demand = if mean_distance > 24 {
        basic.saturating_add((basic + 1) / 2)
    } else if mean_distance > 12 {
        basic.saturating_add((basic + 2) / 3)
    } else {
        basic
    };
    let demand = usize::try_from(demand).unwrap_or(usize::MAX);
    let refineries_wanted = demand
        .div_ceil(HARVESTERS_PER_BAY)
        .clamp(1, MAX_RESOURCE_BAYS);
    Targets {
        harvesters: demand.min(refineries.max(1).saturating_mul(HARVESTERS_PER_BAY)),
        refineries: refineries_wanted,
    }
}

/// Cached ore the AI has found and can reach, shared by mining dispatch and
/// the spending plan. A fresh scan is required after gaining/losing a refinery
/// even when the time limit has not elapsed.
#[derive(Debug, Clone)]
pub(super) struct Survey {
    pub(super) at: u64,
    pub(super) anchors: Vec<(u16, u16)>,
    pub(super) fields: Vec<(u16, u16)>,
    pub(super) stock: u32,
    pub(super) mean_distance: u32,
    pub(super) targets: Targets,
}

impl Survey {
    pub(super) fn read(
        rules: &RuleSet,
        obs: &Observation<'_>,
        census: &Census,
        anchors: Vec<(u16, u16)>,
        zone: MovementZone,
        known_ore: &BTreeMap<(u16, u16), u8>,
    ) -> Self {
        let space = super::terrain::Space::read(obs, rules, &census.home_cells, census.base_cell);
        // A refinery's anchor is the middle of its occupied building footprint,
        // *not* a pathing start. Find actual walkable ground beside each bay;
        // otherwise every path lookup from the refinery itself may fail.
        let entries: Vec<((u16, u16), (u16, u16))> = anchors.iter()
            .filter_map(|&anchor| {
                let entry = if obs.has_ground_grid() {
                    super::census::rally_near(obs, anchor, &census.home_cells)?
                } else {
                    anchor
                };
                Some((anchor, entry))
            })
            .collect();
        let mut fields = Vec::new();
        let mut stock: u32 = 0;
        let mut haul: u64 = 0;
        // Known ore is the *last observed* overlay stage. Reading an overlay
        // hidden by current fog here would leak its real-time depletion or
        // growth, even if its location was once explored.
        for (&cell, &stage) in known_ore {
            if !space.ground_at(cell).buildable() {
                continue;
            }
            let distance = entries.iter()
                .filter(|(_, entry)| obs.can_reach(zone, *entry, cell))
                .map(|(anchor, _)| cell_distance(*anchor, cell))
                .min();
            let Some(distance) = distance.filter(|distance| *distance <= SCAN_CELLS) else {
                continue;
            };
            // OverlayData=0 is a valid ore tile, not absence of ore.
            let amount = u32::from(stage) + 1;
            fields.push(cell);
            stock = stock.saturating_add(amount);
            haul = haul.saturating_add(u64::from(amount) * u64::try_from(distance).unwrap_or(0));
        }
        let mean_distance = if stock == 0 { 0 } else { (haul / u64::from(stock)) as u32 };
        let targets = targets(stock, mean_distance, census.refineries);
        Self { at: obs.tick(), anchors, fields, stock, mean_distance, targets }
    }
}


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

#[cfg(test)]
mod resource_target_tests {
    use super::*;

    #[test]
    fn exhausted_ore_does_not_buy_another_miner_or_bay() {
        assert_eq!(targets(0, 5, 2), Targets { harvesters: 0, refineries: 1 });
    }

    #[test]
    fn more_ore_supports_more_miners_and_more_unloading_bays() {
        assert_eq!(targets(55, 4, 1).harvesters, 1);
        assert_eq!(targets(110, 4, 1).harvesters, 2);
        assert_eq!(targets(550, 4, 1).harvesters, HARVESTERS_PER_BAY);
        assert_eq!(targets(550, 4, 2).harvesters, HARVESTERS_PER_BAY * 2);
        assert_eq!(targets(550, 4, 1).refineries, MAX_RESOURCE_BAYS);
    }

    #[test]
    fn long_haul_adds_demand_but_not_unbounded_purchase() {
        assert!(targets(110, 30, 3).harvesters > targets(110, 4, 3).harvesters);
        assert_eq!(targets(u32::MAX, 50, 3).harvesters, MAX_RESOURCE_BAYS * HARVESTERS_PER_BAY);
    }
}
