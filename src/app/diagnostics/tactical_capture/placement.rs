//! Deterministic tactical-capture building placement search.
//!
//! The iterator owns only candidate order. The live simulation remains the
//! placement authority through `production::placement_preview_for_owner_with_overlays`.

use crate::map::entities::EntityCategory;
use crate::rules::overlay_types::OverlayTypeRegistry;
use crate::rules::ruleset::RuleSet;
use crate::sim::pathfinding::PathGrid;
use crate::sim::production;
use crate::sim::world::Simulation;

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub(crate) struct SquareRingCandidate {
    pub cell: (u16, u16),
    pub radius: u16,
    pub candidate_index: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub(crate) struct PlacementChoice {
    pub type_id: String,
    pub anchor_yard_id: u64,
    pub anchor_cell: (u16, u16),
    pub cell: (u16, u16),
    pub foundation: (u16, u16),
    pub radius: u16,
    pub candidate_index: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub(crate) enum PlacementSearchError {
    #[error("placement search requires a non-empty live PathGrid")]
    EmptyPathGrid,
    #[error(
        "placement anchor {anchor_yard_id} is not one live local structure at ({anchor_rx},{anchor_ry})"
    )]
    AnchorYardMissing {
        anchor_yard_id: u64,
        anchor_rx: u16,
        anchor_ry: u16,
    },
    #[error("building type '{type_id}' is not ready for owner '{owner}'")]
    TargetNotReady { owner: String, type_id: String },
    #[error("building type '{type_id}' has no live placement preview")]
    MissingPlacementType { type_id: String },
    #[error(
        "no valid placement for '{type_id}' within radius {max_radius} of yard {anchor_yard_id} at ({anchor_rx},{anchor_ry})"
    )]
    NoValidCell {
        type_id: String,
        max_radius: u16,
        anchor_yard_id: u64,
        anchor_rx: u16,
        anchor_ry: u16,
    },
}

/// Enumerate a clipped square-ring search in one stable order.
///
/// Radius zero visits the anchor once. For each larger radius, top then bottom
/// edges are visited with X ascending, followed by left then right interiors
/// with Y ascending. Signed bounds checks happen before conversion to `u16`.
pub(crate) fn ordered_square_ring_cells(
    center: (u16, u16),
    width: u16,
    height: u16,
    max_radius: u16,
) -> Vec<SquareRingCandidate> {
    if width == 0 || height == 0 {
        return Vec::new();
    }

    let center_x = i32::from(center.0);
    let center_y = i32::from(center.1);
    let width = i32::from(width);
    let height = i32::from(height);
    let mut cells = Vec::new();

    let mut push = |x: i32, y: i32, radius: u16| {
        if x < 0 || y < 0 || x >= width || y >= height {
            return;
        }
        let candidate_index = u32::try_from(cells.len()).unwrap_or(u32::MAX);
        cells.push(SquareRingCandidate {
            // Conversion is intentionally after the signed live-grid bounds
            // check, so negative candidates can never wrap.
            cell: (x as u16, y as u16),
            radius,
            candidate_index,
        });
    };

    push(center_x, center_y, 0);
    for radius in 1..=max_radius {
        let r = i32::from(radius);
        let min_x = center_x - r;
        let max_x = center_x + r;
        let min_y = center_y - r;
        let max_y = center_y + r;

        for x in min_x..=max_x {
            push(x, min_y, radius);
        }
        for x in min_x..=max_x {
            push(x, max_y, radius);
        }
        for y in (min_y + 1)..max_y {
            push(min_x, y, radius);
        }
        for y in (min_y + 1)..max_y {
            push(max_x, y, radius);
        }
    }

    cells
}

/// Return the first live-authority-valid placement in the documented order.
///
/// The caller supplies the resolved local construction-yard identity and cell.
/// This helper does not reserve cells, move blockers, or invoke the placement
/// mutator; it only observes `placement_preview_for_owner_with_overlays`.
pub(crate) fn first_valid_placement(
    sim: &Simulation,
    rules: &RuleSet,
    owner: &str,
    type_id: &str,
    anchor_yard_id: u64,
    anchor_cell: (u16, u16),
    max_radius: u16,
    path_grid: &PathGrid,
    overlay_registry: Option<&OverlayTypeRegistry>,
) -> Result<PlacementChoice, PlacementSearchError> {
    if path_grid.width() == 0 || path_grid.height() == 0 {
        return Err(PlacementSearchError::EmptyPathGrid);
    }
    let anchor_valid = sim
        .substrate
        .entities
        .get(anchor_yard_id)
        .is_some_and(|entity| {
            entity.is_active()
                && entity.category() == EntityCategory::Structure
                && sim.interner.resolve(entity.owner()) == owner
                && (entity.position.rx, entity.position.ry) == anchor_cell
        });
    if !anchor_valid {
        return Err(PlacementSearchError::AnchorYardMissing {
            anchor_yard_id,
            anchor_rx: anchor_cell.0,
            anchor_ry: anchor_cell.1,
        });
    }
    let ready_count = production::ready_buildings_for_owner(sim, rules, owner)
        .iter()
        .filter(|ready| {
            sim.interner
                .resolve(ready.type_id)
                .eq_ignore_ascii_case(type_id)
        })
        .count();
    if ready_count == 0 {
        return Err(PlacementSearchError::TargetNotReady {
            owner: owner.to_string(),
            type_id: type_id.to_string(),
        });
    }

    for candidate in ordered_square_ring_cells(
        anchor_cell,
        path_grid.width(),
        path_grid.height(),
        max_radius,
    ) {
        let preview = production::placement_preview_for_owner_with_overlays(
            sim,
            rules,
            owner,
            type_id,
            candidate.cell.0,
            candidate.cell.1,
            overlay_registry,
        )
        .ok_or_else(|| PlacementSearchError::MissingPlacementType {
            type_id: type_id.to_string(),
        })?;
        if preview.valid {
            return Ok(PlacementChoice {
                type_id: type_id.to_string(),
                anchor_yard_id,
                anchor_cell,
                cell: candidate.cell,
                foundation: (preview.width, preview.height),
                radius: candidate.radius,
                candidate_index: candidate.candidate_index,
            });
        }
    }

    Err(PlacementSearchError::NoValidCell {
        type_id: type_id.to_string(),
        max_radius,
        anchor_yard_id,
        anchor_rx: anchor_cell.0,
        anchor_ry: anchor_cell.1,
    })
}

#[cfg(test)]
mod placement_tests {
    use super::*;
    use crate::rules::ini_parser::IniFile;
    use std::collections::BTreeSet;

    fn placement_rules() -> RuleSet {
        let ini = IniFile::from_str(
            "\
[BuildingTypes]
0=GACNST
1=NAPOWR
[GACNST]
Strength=1000
Foundation=4x3
BaseNormal=yes
ConstructionYard=yes
[NAPOWR]
Strength=750
Foundation=2x2
Adjacent=2
[Clear]
Buildable=yes
",
        );
        RuleSet::from_ini(&ini).expect("placement rules")
    }

    fn yard_fixture(
        width: u16,
        height: u16,
        yard_cell: (u16, u16),
    ) -> (Simulation, RuleSet, PathGrid, u64) {
        let rules = placement_rules();
        let mut sim = Simulation::new();
        crate::sim::arena_fixture::flat_ground(&mut sim, &rules);
        let grid = PathGrid::new(width, height);
        let yard_id = sim
            .spawn_object("GACNST", "Russians", yard_cell.0, yard_cell.1, 0, &rules)
            .expect("yard");
        (sim, rules, grid, yard_id)
    }

    /// The yard fixture with a completed NAPOWR waiting in its owner's
    /// building factory.
    fn ready_fixture(
        width: u16,
        height: u16,
        yard_cell: (u16, u16),
    ) -> (Simulation, RuleSet, PathGrid, u64) {
        let (mut sim, rules, grid, yard_id) = yard_fixture(width, height, yard_cell);
        let owner_id = sim.interner.intern("Russians");
        let target_id = sim.interner.intern("NAPOWR");
        crate::sim::production::complete_held_building_for_test(
            &mut sim, &rules, owner_id, target_id,
        );
        (sim, rules, grid, yard_id)
    }

    #[test]
    fn radius_zero_visits_center_once() {
        assert_eq!(
            ordered_square_ring_cells((4, 5), 10, 10, 0),
            vec![SquareRingCandidate {
                cell: (4, 5),
                radius: 0,
                candidate_index: 0,
            }]
        );
    }

    #[test]
    fn each_ring_uses_top_bottom_then_left_right_order() {
        let cells = ordered_square_ring_cells((3, 3), 8, 8, 1);
        let actual: Vec<(u16, u16)> = cells.into_iter().map(|cell| cell.cell).collect();
        assert_eq!(
            actual,
            vec![
                (3, 3),
                (2, 2),
                (3, 2),
                (4, 2),
                (2, 4),
                (3, 4),
                (4, 4),
                (2, 3),
                (4, 3),
            ]
        );
    }

    #[test]
    fn bounds_are_clipped_before_conversion_and_no_cell_is_duplicated() {
        let cells = ordered_square_ring_cells((0, 0), 3, 2, 4);
        assert!(cells.iter().all(|cell| cell.cell.0 < 3 && cell.cell.1 < 2));
        let unique: BTreeSet<(u16, u16)> = cells.iter().map(|cell| cell.cell).collect();
        assert_eq!(unique.len(), cells.len());
        assert_eq!(unique.len(), 6);
    }

    #[test]
    fn first_live_preview_valid_candidate_wins_stably() {
        let (sim, rules, grid, yard_id) = ready_fixture(24, 24, (10, 10));
        let ordered = ordered_square_ring_cells((10, 10), grid.width(), grid.height(), 16);
        let expected = ordered
            .iter()
            .find(|candidate| {
                production::placement_preview_for_owner_with_overlays(
                    &sim,
                    &rules,
                    "Russians",
                    "NAPOWR",
                    candidate.cell.0,
                    candidate.cell.1,
                    None,
                )
                .is_some_and(|preview| preview.valid)
            })
            .copied()
            .expect("at least one valid placement");

        let first = first_valid_placement(
            &sim,
            &rules,
            "Russians",
            "NAPOWR",
            yard_id,
            (10, 10),
            16,
            &grid,
            None,
        )
        .expect("placement");
        let repeat = first_valid_placement(
            &sim,
            &rules,
            "Russians",
            "NAPOWR",
            yard_id,
            (10, 10),
            16,
            &grid,
            None,
        )
        .expect("repeat placement");

        assert_eq!(first.cell, expected.cell);
        assert_eq!(first.radius, expected.radius);
        assert_eq!(first.candidate_index, expected.candidate_index);
        assert_eq!(repeat, first);
    }

    #[test]
    fn target_must_be_ready_before_search() {
        let (sim, rules, grid, yard_id) = yard_fixture(24, 24, (10, 10));
        assert!(matches!(
            first_valid_placement(
                &sim,
                &rules,
                "Russians",
                "NAPOWR",
                yard_id,
                (10, 10),
                16,
                &grid,
                None,
            ),
            Err(PlacementSearchError::TargetNotReady { .. })
        ));
    }

    #[test]
    fn anchor_must_still_be_the_live_local_structure() {
        let (mut sim, rules, grid, yard_id) = ready_fixture(24, 24, (10, 10));
        sim.substrate.entities.remove(yard_id);
        assert!(matches!(
            first_valid_placement(
                &sim,
                &rules,
                "Russians",
                "NAPOWR",
                yard_id,
                (10, 10),
                16,
                &grid,
                None,
            ),
            Err(PlacementSearchError::AnchorYardMissing { .. })
        ));
    }

    #[test]
    fn no_valid_candidate_fails_without_fallback() {
        // Radius zero visits only the live yard's occupied anchor. A failed
        // search must not fall back to a cell beyond the requested radius.
        let (sim, rules, grid, yard_id) = ready_fixture(24, 24, (10, 10));
        assert!(matches!(
            first_valid_placement(
                &sim,
                &rules,
                "Russians",
                "NAPOWR",
                yard_id,
                (10, 10),
                0,
                &grid,
                None,
            ),
            Err(PlacementSearchError::NoValidCell { .. })
        ));
    }
}
