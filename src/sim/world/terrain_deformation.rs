//! Terrain deformation. gamemd still reads every warhead's `Deform=`
//! (`WarheadTypeClass+0x98`) and `DeformThreshhold=` (`+0x100`) but nothing
//! uses them. Tiberian Sun rolled them in its explosion damage to lower the
//! ground; this ports that roll and reshapes the ground with gamemd's own
//! hills corner engine (`RandomMapGenerator::CreateHills @ 0x005A35F0`) over
//! a window of the live map. See `docs/terrain-deformation.md`.

use crate::map::corner_heights::{self, CornerCells, CornerGrid, FinalCell};
use crate::map::resolved_terrain::{ResolvedTerrainCell, ResolvedTerrainGrid};
use crate::rules::ruleset::RuleSet;
use crate::rules::warhead_type::WarheadType;
use crate::sim::overlay_grid::OverlayGrid;
use crate::sim::projectile::ProjectileCoord;
use crate::sim::tiberium::NativeCellObjectView;
use crate::sim::world::Simulation;

/// Cells from the hit cell to the corner window's edge. A reshape that would
/// move a corner on the edge rolls back, which bounds how far one hit can
/// pull a slope.
const WINDOW_RADIUS: i32 = 16;

/// TS `LEVEL_LEPTON_H`: one level in leptons, the close-to-ground tolerance.
const LEVEL_LEPTONS: i32 = 104;

/// The live map as the corner engine reads it: Level and slope, overlay
/// identity, FirstObject and the tile's `Morphable=`.
struct LiveCornerCells<'a> {
    terrain: &'a ResolvedTerrainGrid,
    overlays: Option<&'a OverlayGrid>,
    objects: NativeCellObjectView<'a>,
}

impl LiveCornerCells<'_> {
    fn cell(&self, x: i32, y: i32) -> Option<&ResolvedTerrainCell> {
        let (Ok(x), Ok(y)) = (u16::try_from(x), u16::try_from(y)) else {
            return None;
        };
        self.terrain.cell(x, y)
    }
}

impl CornerCells for LiveCornerCells<'_> {
    fn in_diamond(&self, x: i32, y: i32) -> bool {
        self.cell(x, y).is_some()
    }

    fn level_slope(&self, x: i32, y: i32) -> (i32, u8) {
        self.cell(x, y).map_or((0, 0), |cell| {
            // The engine's table stops at slope 18. A map's slopes 19 and 20
            // share 17's and 18's surface records in
            // `CellClass::ComputeGroundHeightAtCoord @ 0x0047B3A0`, so their
            // corners are those slopes'.
            let slope = match cell.slope_type {
                19 => 17,
                20 => 18,
                slope => slope,
            };
            (i32::from(cell.level as i8), slope)
        })
    }

    fn editable(&self, x: i32, y: i32) -> bool {
        let Some(cell) = self.cell(x, y) else {
            return false;
        };
        let coord = (x as u16, y as u16);
        // The RMG's protected flag has no live counterpart; the border outside
        // the playfield keeps the shape the map gave it.
        !cell.outside_playfield
            && self
                .overlays
                .is_none_or(|grid| grid.cell(coord.0, coord.1).overlay_id.is_none())
            && !self.objects.ground_object_present(coord)
            && self.terrain.deformation_tile_morphable(cell)
    }
}

impl Simulation {
    /// Tiberian Sun's crater roll, from its explosion damage after the
    /// explosive-overlay branch, inside the block it runs only for a blast
    /// within one level of the ground under it: damage above
    /// `DeformThreshhold=` spends one Scenario `Random_Pick(0, 99)` draw and
    /// lowers the hit cell when the draw is below
    /// `trunc(damage * 0.01 * Deform * 100.0)`. TS skips a hit on a high
    /// bridge after that draw; a bridge deck is four levels up, so the ground
    /// gate already excludes it.
    pub(crate) fn roll_terrain_deformation(
        &mut self,
        rules: &RuleSet,
        warhead: &WarheadType,
        impact: ProjectileCoord,
        cell: (u16, u16),
        damage: i32,
    ) {
        if !rules.combat_damage.terrain_deformation {
            return;
        }
        let ground = self.resolved_terrain.as_ref().and_then(|terrain| {
            let cell = terrain.cell(cell.0, cell.1)?;
            crate::util::lepton::ground_height_leptons(
                cell.level,
                cell.slope_type,
                impact.x,
                impact.y,
            )
            .ok()
        });
        let Some(ground) = ground else {
            return;
        };
        if (impact.z - ground).abs() >= LEVEL_LEPTONS || damage <= warhead.deform_threshold {
            return;
        }
        // IEEE double arithmetic, as TS's percent; `as` truncates like ftol.
        let chance = (f64::from(damage) * 0.01 * warhead.deform_f64 * 100.0) as i32;
        if (self.scenario_rng.next_range_u32_inclusive(0, 99) as i32) < chance {
            self.deform_terrain(cell, rules, warhead.deform_depth);
        }
    }

    /// Lower `cell` up to `depth` levels (VERA's `DeformDepth=`, read as
    /// `1..=16`) with the hills corner engine (mask `0x006B4240`, adjust
    /// `0x006B3E60` downward, finalize `0x006B3850`) over a window of the live
    /// map, no corner below `DeformLowestLevel=`, then stamp and publish the
    /// cells whose ground moved. Returns whether any did.
    pub(crate) fn deform_terrain(&mut self, cell: (u16, u16), rules: &RuleSet, depth: i32) -> bool {
        let Some(finals) = self.deformation_finals(cell, rules, depth) else {
            return false;
        };
        let Some(terrain) = self.resolved_terrain.as_mut() else {
            return false;
        };
        let changed = terrain.stamp_deformation(&finals);
        if changed.is_empty() {
            return false;
        }
        // A smudge lies flat; a cell the crater tilts loses it.
        if let Some(grid) = self.smudge_grid.as_mut() {
            for f in &finals {
                if f.slope_tile.is_some_and(|(slope, _)| slope != 0) {
                    let _ = grid.clear_cell_slot(f.x as u16, f.y as u16);
                }
            }
        }
        self.publish_cell_lighting_levels(changed.iter().copied());
        // Every stamped tile is a passable Clear or ramp tile, as the
        // Morphable tile it replaced: zones stay, the path view republishes.
        self.publish_recalculated_cells(rules, &changed);
        for &coord in &changed {
            self.dirty_cells.mark_radar([coord]);
            self.dirty_cells.mark_tactical_once(coord);
        }
        let terrain = self.resolved_terrain.as_ref().expect("deformed terrain");
        for &(x, y) in &changed {
            if let Some(cell) = terrain.cell(x, y) {
                self.dynamic_terrain_cells.record(cell);
            }
        }
        true
    }

    /// The corner engine's run for [`Self::deform_terrain`]: the finalized
    /// cells whose Level or slope differ from the map's, or None when the
    /// first adjustment rolled back or nothing moved.
    fn deformation_finals(
        &self,
        cell: (u16, u16),
        rules: &RuleSet,
        depth: i32,
    ) -> Option<Vec<FinalCell>> {
        let terrain = self.resolved_terrain.as_ref()?;
        let tiles = terrain.deformation_tiles()?;
        let cells = LiveCornerCells {
            terrain,
            overlays: self.overlay_grid.as_ref(),
            objects: NativeCellObjectView::new(
                &self.substrate.occupancy,
                self.terrain_objects.cell_index(),
            ),
        };
        let (cx, cy) = (i32::from(cell.0), i32::from(cell.1));
        if !cells.editable(cx, cy) {
            return None;
        }
        let origin = (cx - WINDOW_RADIUS, cy - WINDOW_RADIUS);
        let stride = 2 * WINDOW_RADIUS + 2;
        let heights = (
            rules.combat_damage.deform_lowest_level * corner_heights::LEVEL,
            i32::from(i8::MAX) * corner_heights::LEVEL,
        );
        let mut corners = CornerGrid::build(&cells, origin, stride, heights);
        // One adjustment per level. The first that would move a corner it
        // cannot ends the crater at the depth the open ground allows. On flat
        // ground, WINDOW_RADIUS levels still fit the window.
        let mut sunk = 0;
        for _ in 0..depth.clamp(1, WINDOW_RADIUS) {
            let mask = corners.pick_mask(cx, cy, -1);
            if mask == 0 || !corners.apply(-1, cx, cy, mask) {
                break;
            }
            sunk += 1;
        }
        if sunk == 0 {
            return None;
        }
        let domain = (origin.1..origin.1 + stride - 1)
            .flat_map(|y| (origin.0..origin.0 + stride - 1).map(move |x| (x, y)));
        // The engine also rewrites touched cells whose corners came back where
        // they were; on a live map only cells whose ground moved change.
        let finals: Vec<_> = corners
            .finalize(&cells, domain, tiles)
            .into_iter()
            .filter(|f| {
                f.slope_tile
                    .is_some_and(|(slope, _)| cells.level_slope(f.x, f.y) != (f.level, slope))
            })
            .collect();
        (!finals.is_empty()).then_some(finals)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::map::resolved_terrain::{test_grid, test_smudge_cell};
    use crate::rules::ini_parser::IniFile;

    const RAMP_BASE: u16 = 600;

    /// `crater` adds keys to the test warhead's section.
    fn rules(enabled: bool, crater: &str) -> RuleSet {
        RuleSet::from_ini(&IniFile::from_str(&format!(
            "[InfantryTypes]\n[VehicleTypes]\n[AircraftTypes]\n[BuildingTypes]\n\
             [CombatDamage]\nTerrainDeformation={}\n\
             [Warheads]\n0=CRATERWH\n\
             [CRATERWH]\nDeform=100%\nDeformThreshhold=100\n{crater}\
             Verses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n",
            if enabled { "yes" } else { "no" },
        )))
        .expect("deformation rules")
    }

    /// Flat level-0 Morphable ground, 12 cells square, with one overlay.
    fn flat_sim(overlay: (u16, u16)) -> Simulation {
        let mut sim = Simulation::new();
        let mut terrain = test_grid(12, 12, test_smudge_cell);
        terrain.test_install_deformation_catalog(0, RAMP_BASE);
        sim.resolved_terrain = Some(terrain);
        let mut overlays = OverlayGrid::new(12, 12);
        overlays.place_overlay(overlay.0, overlay.1, 0, 0);
        sim.overlay_grid = Some(overlays);
        sim
    }

    fn level_slope(sim: &Simulation, x: u16, y: u16) -> (i8, u8) {
        let cell = sim.resolved_terrain.as_ref().unwrap().cell(x, y).unwrap();
        (cell.level as i8, cell.slope_type)
    }

    #[test]
    fn a_crater_sinks_below_level_zero_around_an_overlay_cell_it_cannot_move() {
        // The overlay north-east of the hit locks the hit cell's NE corner.
        let mut sim = flat_sim((6, 4));
        assert!(sim.deform_terrain((5, 5), &rules(true, ""), 1));

        // Three corners sank: [0, 15, 0, 0] above the lowest is slope 8.
        assert_eq!(level_slope(&sim, 5, 5), (-1, 8));
        let hit = sim.resolved_terrain.as_ref().unwrap().cell(5, 5).unwrap();
        assert_eq!(hit.final_tile_index, i32::from(RAMP_BASE) + 7);
        // The sides ramp down to it; the overlay cell keeps its ground.
        assert_eq!(level_slope(&sim, 5, 4), (-1, 12));
        assert_eq!(level_slope(&sim, 4, 5), (-1, 3));
        assert_eq!(level_slope(&sim, 6, 4), (0, 0));
        assert!(sim.dynamic_terrain_cells.get(&(6, 4)).is_none());
        assert_eq!(
            sim.dynamic_terrain_cells.get(&(5, 5)).unwrap().level as i8,
            -1
        );
        assert_eq!(sim.dynamic_terrain_cells.len(), 8);
    }

    #[test]
    fn the_crater_roll_draws_only_when_enabled_near_the_ground_above_the_threshold() {
        let centre = |z| ProjectileCoord::new(5 * 256 + 128, 5 * 256 + 128, z);
        // (switch, impact height, damage, draws)
        for (enabled, z, damage, draws) in [
            (false, 0, 500, 0),
            (true, 4 * LEVEL_LEPTONS, 500, 0),
            (true, 0, 100, 0),
            (true, LEVEL_LEPTONS - 1, 101, 1),
        ] {
            let rules = rules(enabled, "");
            let warhead = rules.warhead("CRATERWH").expect("crater warhead");
            let mut sim = flat_sim((0, 0));
            let mut expected = sim.scenario_rng.clone();
            for _ in 0..draws {
                let _ = expected.next_range_u32_inclusive(0, 99);
            }
            sim.roll_terrain_deformation(&rules, warhead, centre(z), (5, 5), damage);
            assert_eq!(
                sim.scenario_rng.logical_state(),
                expected.logical_state(),
                "draws for {enabled} {z} {damage}"
            );
            // Deform=100% of 101 damage is a 101% chance.
            assert_eq!(level_slope(&sim, 5, 5).0, -(draws as i8));
        }
    }

    #[test]
    fn a_deeper_crater_widens_a_cell_per_level_until_ground_it_cannot_move() {
        let rules = rules(true, "DeformDepth=3\n");
        let warhead = rules.warhead("CRATERWH").expect("crater warhead");
        let centre = ProjectileCoord::new(5 * 256 + 128, 5 * 256 + 128, 0);
        // Open ground: the hit cell sinks three levels and each ring around
        // it ramps up a level, so the fourth ring keeps its ground.
        let mut sim = flat_sim((0, 0));
        sim.roll_terrain_deformation(&rules, warhead, centre, (5, 5), 500);
        assert_eq!(level_slope(&sim, 5, 5), (-3, 0));
        assert_eq!(level_slope(&sim, 5, 4), (-3, 4));
        assert_eq!(level_slope(&sim, 5, 2), (-1, 4));
        assert_eq!(level_slope(&sim, 5, 1), (0, 0));
        // The third level would pull down the corners of the overlay cell
        // three cells east, so the crater stops two levels deep.
        let mut sim = flat_sim((8, 5));
        sim.roll_terrain_deformation(&rules, warhead, centre, (5, 5), 500);
        assert_eq!(level_slope(&sim, 5, 5), (-2, 0));
    }
}
