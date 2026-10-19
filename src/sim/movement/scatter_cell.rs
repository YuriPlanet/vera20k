//! The away-from-a-coordinate start direction and the eight-neighbour walk
//! that both Foot Scatter bodies compile from one source: Infantry
//! `0x0051D258..0x0051D2D4` then `0x0051D487..0x0051D6BA`, Unit
//! `0x00743D62..0x00743E04` then `0x00743E08..0x00744048`. They call the
//! same functions in the same order; only the start jitter, the receiver's
//! own `Can_Enter_Cell` (vt+0x1AC) and each translation unit's copies of the
//! NullCell and level-height constants differ.
//!
//! Admission belongs to the class receiver. Its candidate query distinguishes
//! illegal cells, legal fallback cells, and preferred cells. The first legal
//! cell is retained while the search continues for a preferred surface.

use super::infantry_entry::InfantryEntryArgs;
use crate::map::resolved_terrain::{NativeCellQuery, ResolvedTerrainGrid};
use crate::rules::overlay_types::OverlayTypeRegistry;
use crate::rules::ruleset::RuleSet;
use crate::sim::world::Simulation;

/// How far a class's Scatter jitters its start octant either way: one
/// Scenario `RandomRanged(0, 2n)` less `n` (Infantry `0x0051D2BA` and
/// `0x0051D385`; Unit `0x00743DC5`).
#[derive(Clone, Copy, Debug)]
pub(super) enum OctantJitter {
    Infantry = 2,
    Unit = 1,
}

/// The away heading from `source` to the physical coordinate (Object+9C, not
/// Foot+4C), rounded to an octant, plus the class's jitter. The heading goes
/// through the shared wide coordinate subtraction before native f32
/// narrowing.
pub(super) fn source_start_direction(
    current: (i32, i32),
    source: (i32, i32),
    jitter: OctantJitter,
    rng: &mut crate::sim::rng::SimRng,
) -> i32 {
    let away = crate::util::direction_tables::facing16_between(
        [source.0, source.1],
        [current.0, current.1],
    );
    octant_start(away, jitter, rng)
}

/// Infantry51D2D9..51D390, the null-coordinate arm: the body facing when the
/// physical coordinate (vt+0x48) sits at its cell centre, else the heading
/// from the centre to it; rounded to an octant, then RandomRanged(0,4)-2.
/// The draw precedes the arm's nearby-cell search, so it is paid whether
/// or not the fallback runs.
pub(super) fn null_start_direction(
    current: (i32, i32),
    facing: u16,
    rng: &mut crate::sim::rng::SimRng,
) -> i32 {
    let within = [current.0 & 0xFF, current.1 & 0xFF];
    let heading = if within == [0x80, 0x80] {
        facing
    } else {
        crate::util::direction_tables::facing16_between([0x80, 0x80], within)
    };
    octant_start(heading, OctantJitter::Infantry, rng)
}

fn octant_start(heading: u16, jitter: OctantJitter, rng: &mut crate::sim::rng::SimRng) -> i32 {
    let jitter = jitter as i32;
    let octant = i32::from(crate::util::direction_tables::dir_from_facing16(heading));
    rng.next_range_u32_inclusive(0, 2 * jitter as u32) as i32 - jitter + octant
}

/// `None` refuses entry, `Some(false)` retains a fallback, `Some(true)` selects
/// immediately. Native NullCell is (0,0), so a legal zero cell is not a result.
pub(super) fn select_neighbor<E>(
    seed: (i16, i16),
    start_direction: i32,
    mut inspect: impl FnMut((i16, i16), i32) -> Result<Option<bool>, E>,
) -> Result<Option<(i16, i16)>, E> {
    let mut fallback = (0, 0);
    for offset in 0..8 {
        let direction = (start_direction + offset) & 7;
        let (dx, dy) = crate::util::direction::DIRECTION_DELTAS[direction as usize];
        let candidate = (
            seed.0.wrapping_add(dx as i16),
            seed.1.wrapping_add(dy as i16),
        );
        let Some(preferred) = inspect(candidate, direction)? else {
            continue;
        };
        if fallback == (0, 0) {
            fallback = candidate;
        }
        if preferred {
            return Ok((candidate != (0, 0))
                .then_some(candidate)
                .or((fallback != (0, 0)).then_some(fallback)));
        }
    }
    Ok((fallback != (0, 0)).then_some(fallback))
}

/// Scatter51D62A uses the same 6D6410 projection as FNPC. Input Z is ignored
/// by that native projection. Only a direct candidate without structural
/// bridge flag100 is preferred; neither failure invalidates the fallback.
pub(super) fn preferred_surface(terrain: &ResolvedTerrainGrid, candidate: (i16, i16)) -> bool {
    let cells = NativeCellQuery::canonical(terrain);
    let coord = (i32::from(candidate.0), i32::from(candidate.1));
    if crate::sim::find_nearby_cell::project_world_coordinate_with_lookup(
        coord.0 * 256 + 128,
        coord.1 * 256 + 128,
        |x, y| cells.projection_view(x, y),
    ) != coord
    {
        return false;
    }
    let cell = cells.lookup(candidate);
    cells.flags(cell) & 0x100 == 0
}

impl Simulation {
    /// The eight-neighbour walk both Foot Scatter bodies share, from the
    /// navigation cell (Foot+4C): a candidate in the height-aware playfield
    /// (`0x00578460`) is legal only when the receiver's own `Can_Enter_Cell`
    /// ([`Self::foot_can_enter`]) answers zero, with the object's effective
    /// cell level (`0x005F5F00`), so a soft obstruction is not a passable
    /// destination; then [`preferred_surface`].
    /// Evidence: tools/spatial_oracle/infantry_scatter_entry and
    /// unit_source_scatter.{py,json,meta.json}.
    ///
    /// Missing map state keeps headless fixtures compatible: no cell is found.
    pub(super) fn foot_scatter_neighbor(
        &self,
        id: u64,
        start: i32,
        rules: &RuleSet,
        registry: Option<&OverlayTypeRegistry>,
    ) -> Result<Option<(u16, u16)>, String> {
        let Some(foot) = self.substrate.entities.get(id) else {
            return Ok(None);
        };
        let current = super::foot_coordinate::current_coordinate(foot);
        let Some(terrain) = self.resolved_terrain.as_ref() else {
            return Ok(None);
        };
        let Some(bounds) = self.playfield_bounds else {
            return Ok(None);
        };
        let navigation = super::foot_coordinate::navigation_coordinate(foot, Some(terrain))?;
        let seed = ((navigation.x / 256) as i16, (navigation.y / 256) as i16);
        let on_bridge = foot.on_bridge();
        let destination = select_neighbor(seed, start, |candidate, direction| {
            let cells = NativeCellQuery::canonical(terrain);
            // Preserve the retained Cell identity across the height-aware
            // playfield lookup and Object5F5F00's current-cell lookup. A dummy
            // identity is shared and can change its coordinate between reads.
            let cell = cells.lookup(candidate);
            if !crate::sim::cell_rect::cell_is_in_playfield_height_aware(
                (i32::from(candidate.0), i32::from(candidate.1)),
                Some(bounds),
                Some(terrain),
            ) {
                return Ok(None);
            }
            let height = super::ground_pose::query_object_cell_height(&cells, current, on_bridge);
            let args = InfantryEntryArgs {
                direction,
                height,
                previous_cell: None,
            };
            if self.foot_can_enter(id, cell, args, rules, registry)? != 0 {
                return Ok(None);
            }
            Ok::<_, String>(Some(preferred_surface(terrain, candidate)))
        })?;
        Ok(destination.map(|destination| (destination.0 as u16, destination.1 as u16)))
    }
}

#[cfg(test)]
#[path = "scatter_cell_tests.rs"]
mod tests;
