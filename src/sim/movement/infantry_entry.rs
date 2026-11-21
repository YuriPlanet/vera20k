//! Shared Infantry51BF90 entry inputs and the Foot4D9C60 height/list prelude.
//! The failure receiver consumes zero/nonzero, repair consumes seven, and the
//! Foot path wrapper distinguishes six. Keep those answers in one owner.

use crate::map::cell_index::NativeCellIdentity as Cell;
use crate::map::resolved_terrain::NativeCellQuery;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum InfantryEntryClass {
    Clear,
    SoftOther,
    Obstructed6,
    Impassable7,
}

impl InfantryEntryClass {
    pub(crate) fn from_raw(code: u8) -> Self {
        match code {
            0 => Self::Clear,
            1..=5 => Self::SoftOther,
            6 => Self::Obstructed6,
            7 => Self::Impassable7,
            _ => unreachable!("Infantry51BF90 result domain"),
        }
    }

    pub(crate) fn is_nonzero(self) -> bool {
        self != Self::Clear
    }
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct InfantryEntryArgs {
    pub(crate) direction: i32,
    pub(crate) height: i32,
    pub(crate) previous_cell: Option<Cell>,
}

impl InfantryEntryArgs {
    pub(crate) const REPAIR: Self = Self {
        direction: -1,
        height: -1,
        previous_cell: None,
    };
}

/// Fifth argument of class `Can_Enter_Cell` (+1AC). Foot4D9C10 calls the
/// installed locomotor's +1C only for `CheckLocomotor`; every active retail
/// interface uses the side-effect-free constant-zero55ABF0 leaf. Both modes
/// therefore seed the same class result without changing any simulation state.
/// Original48-call comparison: tools/spatial_oracle/foot_locomotor_entry.*.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum EntryQueryMode {
    SkipLocomotor,
    CheckLocomotor,
}

/// The cell fields the Foot4D9C60 prelude and its backstep read. A lookup off
/// the map stamps the shared Dummy, whose fields are read at each use; a real
/// cell's do not change while Can_Enter_Cell runs, so a caller may read them
/// from an earlier copy.
pub(crate) trait PreludeCells {
    fn coord(&self, cell: Cell) -> (i16, i16);
    fn lookup(&self, coord: (i16, i16)) -> Cell;
    /// The signed level and the slope.
    fn level_slope(&self, cell: Cell) -> (i32, u8);
    fn flags(&self, cell: Cell) -> u32;
    fn corrected_deck(&self, _cell: Cell) -> Option<i32> {
        None
    }
}

impl PreludeCells for NativeCellQuery<'_> {
    fn coord(&self, cell: Cell) -> (i16, i16) {
        NativeCellQuery::coord(self, cell)
    }

    fn lookup(&self, coord: (i16, i16)) -> Cell {
        NativeCellQuery::lookup(self, coord)
    }

    fn level_slope(&self, cell: Cell) -> (i32, u8) {
        let (level, slope) = self.ground_fields(cell);
        (i32::from(level as i8), slope)
    }

    fn flags(&self, cell: Cell) -> u32 {
        NativeCellQuery::flags(self, cell)
    }

    fn corrected_deck(&self, cell: Cell) -> Option<i32> {
        self.terrain()
            .corrected_high_bridge_deck_at(cell)
            .map(i32::from)
    }
}

pub(crate) fn backstep_cell(cells: &impl PreludeCells, target: Cell, direction: i32) -> Cell {
    let coord = cells.coord(target);
    let (dx, dy) = crate::util::direction::DIRECTION_DELTAS[((direction - 4) & 7) as usize];
    cells.lookup((
        coord.0.wrapping_add(dx as i16),
        coord.1.wrapping_add(dy as i16),
    ))
}

/// Foot+1B0. The caller has already performed its separate GetNeighbour /
/// GetTube query. A NULL previous Cell performs this second lookup, preserving
/// retained Dummy aliases and the exact mutable height/object-list outputs.
pub(crate) fn adjust_height_and_list(
    cells: &impl PreludeCells,
    target: Cell,
    direction: i32,
    previous: Option<Cell>,
    height: &mut i32,
    list_bridge: &mut bool,
) -> bool {
    let source = previous.unwrap_or_else(|| backstep_cell(cells, target, direction));
    let (target_level, target_slope) = cells.level_slope(target);
    let target_deck = cells.corrected_deck(target);
    if direction == -1 {
        if *height == -1 && cells.flags(target) & 0x100 != 0 {
            *height = target_deck.unwrap_or(target_level + 4);
        }
        return true;
    }
    let (mut source_level, mut source_slope) = cells.level_slope(source);
    let source_bridge = cells.flags(source) & 0x100 != 0;
    let source_deck = cells.corrected_deck(source);
    if *height == -1 && source_bridge {
        *height = source_deck.unwrap_or(source_level + 4);
        if cells.flags(target) & 0x200 == 0 {
            return false;
        }
    }
    // Foot4D9C60 normally infers the deck from a four-level terrain step.
    // A depressed lane admits bridge traffic at the span's actual elevation.
    let on_source_deck = source_bridge && *height == source_deck.unwrap_or(source_level + 4);
    if let Some(deck) = target_deck {
        if *height == deck && (!source_bridge || on_source_deck) {
            if cells.flags(target) & 0x300 != 0x300 {
                return false;
            }
            *list_bridge = true;
            return true;
        }
        if on_source_deck {
            return false;
        }
    }
    if on_source_deck && let Some(deck) = source_deck {
        source_level = deck - 4;
        source_slope = 0;
    }
    let difference = if source_bridge { source_level } else { *height } - target_level;
    match difference.abs() {
        0 => {
            if cells.flags(target) & 0x300 == 0x300 && cells.flags(source) & 0x100 != 0 {
                return true;
            }
            *height == -1 || *height == target_level
        }
        1 => {
            if difference > 0 {
                target_slope != 0
            } else {
                source_slope != 0
            }
        }
        4 => {
            if source_level == target_level - 4
                && (*height != target_level || cells.flags(source) & 0x100 == 0)
            {
                return false;
            }
            if target_level == source_level - 4 {
                if target_deck.is_some() || cells.flags(target) & 0x300 != 0x300 {
                    return false;
                }
                *list_bridge = true;
            }
            true
        }
        _ => false,
    }
}

#[cfg(all(test, tests_sim_movement))]
mod tests {
    use super::*;

    /// One step from `source` at (3,3) to `target` east of it at (4,3), each
    /// `(level, slope, flags)`, at path `height`: the prelude's admission, the
    /// height it leaves and whether it selected the bridge list.
    fn step(source: (u8, u8, u32), target: (u8, u8, u32), height: i32) -> (bool, i32, bool) {
        let mut terrain = crate::map::resolved_terrain::test_flat_ground_grid(8);
        let [source, target] = [((3, 3), source), ((4, 3), target)].map(
            |(coord, (level, slope, flags)): ((i16, i16), (u8, u8, u32))| {
                let cell = terrain.native_cell_identity(coord);
                terrain.write_native_cell_level(cell, level);
                terrain.write_native_cell_flags(cell, flags);
                terrain
                    .cell_mut_for_test(coord.0 as u16, coord.1 as u16)
                    .unwrap()
                    .slope_type = slope;
                cell
            },
        );
        let (mut height, mut list_bridge) = (height, false);
        let admitted = adjust_height_and_list(
            &NativeCellQuery::canonical(&terrain),
            target,
            2,
            Some(source),
            &mut height,
            &mut list_bridge,
        );
        (admitted, height, list_bridge)
    }

    /// `0x004D9DD8..0x004D9E05`: a one-level step reads the slope of the lower
    /// cell only, from signed levels (`MOVSX` of `+0x11B`); two, three and
    /// five levels are refused whatever the slopes (`0x004D9D50`).
    #[test]
    fn the_lower_cells_slope_decides_a_one_level_step() {
        for (source, target, height, admitted) in [
            ((0, 0, 0), (1, 0, 0), 0, false),
            ((0, 0, 0), (1, 2, 0), 0, false),
            ((0, 2, 0), (1, 0, 0), 0, true),
            ((1, 2, 0), (0, 0, 0), 1, false),
            ((1, 0, 0), (0, 2, 0), 1, true),
            ((0xFF, 0, 0), (0, 0, 0), -1, false),
            ((0xFF, 0xFE, 0), (0, 0, 0), -1, true),
            ((0, 0, 0), (0xFF, 0, 0), 0, false),
            ((0, 0, 0), (0xFF, 0xFE, 0), 0, true),
            ((0, 2, 0), (2, 2, 0), 0, false),
            ((0, 2, 0), (3, 2, 0), 0, false),
            ((0, 2, 0), (5, 2, 0), 0, false),
        ] {
            assert_eq!(
                step(source, target, height).0,
                admitted,
                "{source:?} -> {target:?} at {height}"
            );
        }
    }

    /// `0x004D9E08..0x004D9E3B`: from a structural cell at deck height, an
    /// equal-level target needs both `0x100` and `0x200`; at the target's own
    /// level (the ground beneath the span) the step is admitted.
    #[test]
    fn a_deck_step_needs_the_targets_transition_flag() {
        for (target_flags, height, admitted) in
            [(0x100, 4, false), (0x300, 4, true), (0x100, 0, true)]
        {
            assert_eq!(
                step((0, 0, 0x100), (0, 0, target_flags), height),
                (admitted, height, false),
                "flags {target_flags:#x} at {height}"
            );
        }
    }

    /// `0x004D9D5C..0x004D9DD5`: four levels down onto a span needs the
    /// target's `0x300` and selects the bridge list; four levels up needs a
    /// structural source with the path at the target's level (the deck).
    #[test]
    fn a_four_level_step_meets_the_deck() {
        assert_eq!(step((4, 0, 0), (0, 0, 0x100), 4), (false, 4, false));
        assert_eq!(step((4, 0, 0), (0, 0, 0x300), 4), (true, 4, true));
        assert_eq!(step((0, 0, 0x100), (4, 0, 0), 4), (true, 4, false));
        assert_eq!(step((0, 0, 0x100), (4, 0, 0), 0), (false, 0, false));
        assert_eq!(step((0, 0, 0), (4, 0, 0), 0), (false, 0, false));
    }
}
