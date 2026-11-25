//! Terrain deformation. gamemd still reads every warhead's `Deform=`
//! (`WarheadTypeClass+0x98`) and `DeformThreshhold=` (`+0x100`) but nothing
//! uses them. Tiberian Sun: Firestorm rolled them in its explosion damage to
//! dent the ground, and dented it where a meteor landed; this ports both and
//! Firestorm's dent (Firestorm 2.0 0x51E950), whose corner engine is the one
//! gamemd keeps for its random map generator (`map::corner_heights`). See
//! `docs/terrain-deformation.md`.

use crate::map::bridge_facts::{
    BRIDGE_FLAG_DESTROYED_OR_RAMP, BRIDGE_FLAG_DIRECTION_ZERO, BRIDGE_FLAG_STRUCTURAL,
};
use crate::map::bridge_pavement::DAMAGED_PAVEMENT;
use crate::map::cell_index::NativeCellIdentity;
use crate::map::corner_heights::{self, CornerCells, CornerGrid, FinalCell, FinalTiles};
use crate::map::entities::EntityCategory;
use crate::map::playfield::size_diamond_contains;
use crate::map::resolved_terrain::{ResolvedTerrainCell, ResolvedTerrainGrid};
use crate::rules::ruleset::RuleSet;
use crate::rules::warhead_type::WarheadType;
use crate::sim::entity_store::EntityStore;
use crate::sim::game_entity::GameEntity;
use crate::sim::movement::locomotor::MovementLayer;
use crate::sim::occupancy::OccupancyGrid;
use crate::sim::overlay_grid::OverlayGrid;
use crate::sim::projectile::ProjectileCoord;
use crate::sim::terrain_object::{TerrainObjectState, TerrainObjectStore};
use crate::sim::world::{FrameEffects, Simulation};
use crate::util::direction::DIRECTION_DELTAS;
use crate::util::lepton::BRIDGE_DECK_HEIGHT_LEPTONS;

/// Cells from a dent's cell to the edge of the corners Firestorm reads and
/// rewrites for it: the cell ±14 (Firestorm 2.0 0x5F8500, 0x5F8D30).
const DENT_WINDOW: i32 = 14;

/// VERA's window for a wider or deeper crater: room past the widest radius
/// for the ramps it pulls along.
const CRATER_WINDOW: i32 = 16;

/// The widest `DeformRadius=`: the widest band of gamemd's cell-spread table
/// (`sim::combat::cell_spread`), so a crater can be as wide as any blast.
const MAX_CRATER_RADIUS: i32 = 11;

/// Firestorm keeps a lowered corner at or below `0x90` (Firestorm 2.0
/// 0x5F9AB0), Level 12.
const TOP_LEVEL: i32 = 12;

/// Firestorm lowers the rest of a dented cell five frames later (Firestorm
/// 2.0 0x51E950 stores Frame + 5).
const SECOND_STEP_FRAMES: u32 = 5;

/// The rest of the last dent, Firestorm's one slot (Map `+0x140` mask,
/// `+0x144` cell, `+0x148` frame): the corners still to lower (bits `NW=0,
/// NE=1, SE=2, SW=3`), the cell and the frame they sink on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub(crate) struct PendingTerrainDent {
    cell: (i16, i16),
    mask: u8,
    frame: u32,
}

/// The live map as the corner engine reads it.
struct LiveCornerCells<'a> {
    terrain: &'a ResolvedTerrainGrid,
    /// The `In_Bounds` diamond's Size; None, a map without one, has every
    /// cell in bounds.
    size: Option<(i32, i32)>,
    overlays: Option<&'a OverlayGrid>,
    occupancy: &'a OccupancyGrid,
    entities: &'a EntityStore,
    terrain_objects: &'a TerrainObjectStore,
    /// VERA's wider crater (`DeformRadius=`, `DeformDepth=`) widens
    /// Firestorm's editable test twice: what the blast killed no longer
    /// holds the ground up ([`Self::holds_ground`]), and flat land digs
    /// whatever its tile (`ResolvedTerrainGrid::deformation_tile_diggable`).
    crater: bool,
}

impl LiveCornerCells<'_> {
    fn cell(&self, x: i32, y: i32) -> Option<&ResolvedTerrainCell> {
        let (Ok(x), Ok(y)) = (u16::try_from(x), u16::try_from(y)) else {
            return None;
        };
        self.terrain.cell(x, y)
    }

    /// Firestorm 2.0 0x510F20, gamemd's `MapClass::In_Bounds @ 0x00568300`.
    fn in_bounds(&self, x: i32, y: i32) -> bool {
        self.cell(x, y).is_some()
            && self
                .size
                .is_none_or(|(w, h)| size_diamond_contains(w, h, (x as i16, y as i16)))
    }

    /// The objects Firestorm's editable test lets hold a cell (Firestorm 2.0
    /// 0x5F83D0): any on its bridge list (`+0x74`), any on its ground list
    /// (`+0x70`) but a vehicle or a soldier, which the dent sets back on the
    /// ground, and a terrain object such as a tree. Firestorm skips an
    /// object no longer active (`+0x35`); one the blast killed stays on the
    /// list until its death is delivered, and holds the cell until then.
    /// VERA's crater lets it go: an object on the ground list with no Health
    /// left (`+0x6C`) holds nothing, so the crater digs under the buildings
    /// the blast killed.
    fn holds_ground(&self, (x, y): (u16, u16)) -> bool {
        self.occupancy.get(x, y).is_some_and(|cell| {
            cell.iter_layer(MovementLayer::Bridge).next().is_some()
                || cell.iter_layer(MovementLayer::Ground).any(|occupant| {
                    self.entities.get(occupant.entity_id).is_none_or(|entity| {
                        !is_foot(entity) && (entity.is_alive() || !self.crater)
                    })
                })
        }) || self.terrain_objects.members_at((x, y)).any(|id| {
            self.terrain_objects
                .get(id)
                .is_none_or(TerrainObjectState::is_live)
        })
    }
}

/// A vehicle or a soldier: Firestorm's Unit and Infantry (RTTI 1 and 15).
fn is_foot(entity: &GameEntity) -> bool {
    matches!(
        entity.category(),
        EntityCategory::Unit | EntityCategory::Infantry
    )
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

    /// Firestorm 2.0 0x5F83D0: in bounds, no object holding it
    /// ([`Self::holds_ground`]), no bridge over it or over the cell north or
    /// west of it along its span, no overlay, and a `Morphable=` tile (or,
    /// for VERA's crater, flat land). Its bridge test reads `+0xA4 & 0x5000`
    /// (gamemd's `0x500`); VERA keeps a cell with any bridge flag, because a
    /// stamp resets the cell's bridge facts, which Firestorm's dent never
    /// wrote.
    fn editable(&self, x: i32, y: i32) -> bool {
        let Some(cell) = self.cell(x, y) else {
            return false;
        };
        let under_bridge = |nx: i32, ny: i32, span_bit: bool| {
            self.cell(nx, ny).is_some_and(|neighbour| {
                let flags = neighbour.bridge_facts.raw_flags;
                flags & (BRIDGE_FLAG_STRUCTURAL | BRIDGE_FLAG_DESTROYED_OR_RAMP) != 0
                    && (flags & BRIDGE_FLAG_DIRECTION_ZERO != 0) == span_bit
            })
        };
        let coord = (x as u16, y as u16);
        self.in_bounds(x, y)
            && !cell.has_bridge_deck
            && cell.bridge_facts.raw_flags & !DAMAGED_PAVEMENT == 0
            && !under_bridge(x, y - 1, true)
            && !under_bridge(x - 1, y, false)
            && self
                .overlays
                .is_none_or(|grid| grid.cell(coord.0, coord.1).overlay_id.is_none())
            && !self.holds_ground(coord)
            && self.terrain.deformation_tile_diggable(cell, self.crater)
    }
}

/// The corner grid of `radius` cells around `cell`: its origin and stride.
fn window(cell: (i16, i16), radius: i32) -> ((i32, i32), i32) {
    (
        (i32::from(cell.0) - radius, i32::from(cell.1) - radius),
        2 * radius + 2,
    )
}

/// The cells of a [`window`], row by row, as Firestorm 2.0 0x5F8D30 visits
/// them.
fn window_cells(origin: (i32, i32), stride: i32) -> impl Iterator<Item = (i32, i32)> {
    (origin.1..origin.1 + stride - 1)
        .flat_map(move |y| (origin.0..origin.0 + stride - 1).map(move |x| (x, y)))
}

/// The heights a lowered corner may take: down to `DeformLowestLevel=`
/// (default -4; Firestorm's floor is Level 0) and Firestorm's Level 12 at
/// the top.
fn corner_heights(rules: &RuleSet) -> (i32, i32) {
    (
        rules.combat_damage.deform_lowest_level * corner_heights::LEVEL,
        TOP_LEVEL * corner_heights::LEVEL,
    )
}

impl Simulation {
    /// Firestorm's crater roll, from its explosion damage after the
    /// explosive-overlay branch (Firestorm 2.0 0x46006B..0x46011F in
    /// 0x45EE20): damage above `DeformThreshhold=` spends one Scenario
    /// `Random_Pick(0, 99)` draw and dents the hit cell when the draw is below
    /// `trunc(damage * 0.01 * Deform * 100.0)`. After the draw a blast at or
    /// above a high bridge's deck leaves the ground: the cell's bridge flag
    /// (`+0xA4 & 0x1000`, gamemd's `0x100`) and the ground under the impact
    /// (0x51E0A0) plus the deck height.
    ///
    /// Firestorm's Wide_Area_Damage (Firestorm 2.0 0x460440) rolls once more
    /// for its centre before its per-cell explosion damage. gamemd keeps that
    /// function (`0x0048A700`) without the roll and with no caller. Of the
    /// Firestorm blasts that called it, a dying `Explodes=` object fires its
    /// death weapon (Fire_Death_Weapon `0x0070D690`) and the nuke's ground
    /// zero (`0x004251F0`, reached in both games only when the detonation's
    /// anim cannot be allocated) is one Apply_area_damage, each rolling here
    /// once; a dying unit's tiberium (`UnitClass::Death_Explosion`
    /// `0x00738749`) is only summed.
    pub(crate) fn roll_terrain_deformation(
        &mut self,
        rules: &RuleSet,
        warhead: &WarheadType,
        impact: ProjectileCoord,
        cell: (u16, u16),
        damage: i32,
        effects: &mut FrameEffects<'_>,
    ) {
        if !rules.combat_damage.terrain_deformation || damage <= warhead.deform_threshold {
            return;
        }
        // IEEE double arithmetic, as TS's percent; `as` truncates like ftol.
        let chance = (f64::from(damage) * 0.01 * warhead.deform_f64 * 100.0) as i32;
        if (self.scenario_rng.next_range_u32_inclusive(0, 99) as i32) >= chance {
            return;
        }
        let on_deck = self
            .resolved_terrain
            .as_ref()
            .and_then(|terrain| terrain.cell(cell.0, cell.1))
            .is_some_and(|ground| {
                ground.bridge_facts.raw_flags & BRIDGE_FLAG_STRUCTURAL != 0
                    && crate::util::lepton::ground_height_leptons(
                        ground.level,
                        ground.slope_type,
                        impact.x,
                        impact.y,
                    )
                    .is_ok_and(|z| impact.z >= z + BRIDGE_DECK_HEIGHT_LEPTONS)
            });
        if !on_deck {
            self.dent_terrain(
                (cell.0 as i16, cell.1 as i16),
                rules,
                Some(warhead),
                effects,
            );
        }
    }

    /// Firestorm's meteor craters, after a landing's children and only away
    /// from a bridge deck (Firestorm 2.0 0x4151B1..0x41530C for an SHP anim,
    /// 0x65E58D..0x65E6D4 for a voxel one): with `[General] CraterLevel=` not
    /// 0 a dent at the landing cell; above 1 also at its diagonal neighbours,
    /// above 2 at all eight, in compass order from north; above 3 the landing
    /// cell again. Each dent takes the one pending slot, so only the last
    /// dent's second step still comes. Only while `TerrainDeformation=` is on.
    pub(crate) fn dig_meteor_craters(
        &mut self,
        landing: glam::IVec3,
        rules: &RuleSet,
        effects: &mut FrameEffects<'_>,
    ) {
        let level = rules.general.crater_level;
        if !rules.combat_damage.terrain_deformation || level == 0 {
            return;
        }
        // Each axis divided by 256 toward zero, as a signed cell word.
        let centre = ((landing.x / 256) as i16, (landing.y / 256) as i16);
        self.dent_terrain(centre, rules, None, effects);
        if level > 1 {
            for (direction, &(dx, dy)) in DIRECTION_DELTAS.iter().enumerate() {
                if direction % 2 == 1 || level > 2 {
                    let cell = (
                        centre.0.wrapping_add(dx as i16),
                        centre.1.wrapping_add(dy as i16),
                    );
                    self.dent_terrain(cell, rules, None, effects);
                }
            }
        }
        if level > 3 {
            self.dent_terrain(centre, rules, None, effects);
        }
    }

    /// Firestorm's dent (Firestorm 2.0 0x51E950, force 0). With all eight
    /// neighbours of `cell` in bounds, one Scenario `Random_Pick(0, 3)` draw
    /// picks a corner of the cell, which sinks a level at once; the other
    /// three sink five frames later ([`Self::finish_terrain_dent`]). A
    /// `warhead` with VERA's `DeformRadius=` or `DeformDepth=` digs its
    /// crater instead ([`Self::dig_crater`]). Returns whether the dent took,
    /// which rewrites cells even where a corner stayed at its floor.
    pub(crate) fn dent_terrain(
        &mut self,
        cell: (i16, i16),
        rules: &RuleSet,
        warhead: Option<&WarheadType>,
        effects: &mut FrameEffects<'_>,
    ) -> bool {
        let (cx, cy) = (i32::from(cell.0), i32::from(cell.1));
        let admitted = self.live_corner_cells(false).is_some_and(|(cells, _)| {
            DIRECTION_DELTAS
                .iter()
                .all(|&(dx, dy)| cells.in_bounds(cx + dx, cy + dy))
        });
        if !admitted {
            return false;
        }
        let mask = 1 << self.scenario_rng.next_range_u32_inclusive(0, 3);
        let (radius, depth) = warhead.map_or((0, 1), |warhead| {
            (
                warhead.deform_radius.clamp(0, MAX_CRATER_RADIUS),
                warhead.deform_depth.clamp(1, CRATER_WINDOW),
            )
        });
        if radius > 0 || depth > 1 {
            return self.dig_crater(cell, mask, radius, depth, rules, effects);
        }
        if !self.lower_terrain_corners(cell, mask, rules, effects) {
            return false;
        }
        *self.terrain_dent = Some(PendingTerrainDent {
            cell,
            mask: (mask ^ 0xF) as u8,
            frame: self.session.binary_frame().wrapping_add(SECOND_STEP_FRAMES),
        });
        true
    }

    /// Firestorm 2.0 0x51EA50, each frame from `LogicClass::AI` (0x507060),
    /// where gamemd updates its crate timers: on the frame the last dent
    /// named, lower the rest of its cell, force 0, and empty the slot.
    pub(crate) fn finish_terrain_dent(&mut self, rules: &RuleSet, effects: &mut FrameEffects<'_>) {
        let Some(dent) = self
            .terrain_dent
            .filter(|dent| dent.frame == self.session.binary_frame())
        else {
            return;
        };
        self.lower_terrain_corners(dent.cell, u32::from(dent.mask), rules, effects);
        *self.terrain_dent = None;
    }

    /// Fold the pending dent into the world hash; nothing while the slot is
    /// empty, so a game without terrain deformation hashes as before.
    pub(super) fn hash_terrain_dent(&self, hasher: &mut impl std::hash::Hasher) {
        use std::hash::Hash;
        if let Some(dent) = &*self.terrain_dent {
            b"terrain-dent-v1".hash(hasher);
            dent.hash(hasher);
        }
    }

    /// The live map for the corner engine, with VERA's wider editable test
    /// for a `crater`, and the tiles its finalize picks between, while
    /// deformation is on.
    fn live_corner_cells(&self, crater: bool) -> Option<(LiveCornerCells<'_>, FinalTiles)> {
        let terrain = self.resolved_terrain.as_ref()?;
        let tiles = terrain.deformation_tiles()?;
        let cells = LiveCornerCells {
            terrain,
            size: self.map_size_diamond(),
            overlays: self.overlay_grid.as_ref(),
            occupancy: &self.substrate.occupancy,
            entities: &self.substrate.entities,
            terrain_objects: &self.terrain_objects,
            crater,
        };
        Some((cells, tiles))
    }

    /// Firestorm 2.0 0x5F9AB0 lowering `mask`'s corners of `cell` (bits
    /// `NW=0, NE=1, SE=2, SW=3`) with force 0: a corner grid of the cells
    /// within [`DENT_WINDOW`] (0x5F8500), one adjustment that rolls the
    /// whole call back when a corner it must move cannot, and on success
    /// every touched cell of the window rewritten (0x5F8D30,
    /// [`Self::stamp_terrain_dent`]), whether its shape changed or not.
    fn lower_terrain_corners(
        &mut self,
        cell: (i16, i16),
        mask: u32,
        rules: &RuleSet,
        effects: &mut FrameEffects<'_>,
    ) -> bool {
        let finals = {
            let Some((cells, tiles)) = self.live_corner_cells(false) else {
                return false;
            };
            let (origin, stride) = window(cell, DENT_WINDOW);
            let mut corners = CornerGrid::build(&cells, origin, stride, corner_heights(rules));
            if !corners.apply_or_discard(-1, i32::from(cell.0), i32::from(cell.1), mask) {
                return false;
            }
            corners.finalize(&cells, window_cells(origin, stride), tiles)
        };
        self.stamp_terrain_dent(&finals, rules, effects);
        true
    }

    /// VERA's wider or deeper crater (`DeformRadius=`, `DeformDepth=`), made
    /// of Firestorm's lowerings (Firestorm 2.0 0x5F9AB0) over one corner grid
    /// of [`CRATER_WINDOW`] cells: first the hit cell's dent, both steps at
    /// once, then level by level each cell within `(radius + 1/2) * (depth -
    /// k + 1) / depth` cells of the hit lowers its corners not yet `k` levels
    /// below where they started. Each lowering is all or nothing, so the
    /// crater bends around ground it cannot move, under the crater's wider
    /// editable test ([`LiveCornerCells::crater`]); the touched cells are
    /// rewritten once at the end. Returns whether any was.
    fn dig_crater(
        &mut self,
        cell: (i16, i16),
        first: u32,
        radius: i32,
        depth: i32,
        rules: &RuleSet,
        effects: &mut FrameEffects<'_>,
    ) -> bool {
        let finals = {
            let Some((cells, tiles)) = self.live_corner_cells(true) else {
                return false;
            };
            let (cx, cy) = (i32::from(cell.0), i32::from(cell.1));
            let (origin, stride) = window(cell, CRATER_WINDOW);
            let mut corners = CornerGrid::build(&cells, origin, stride, corner_heights(rules));
            let start = corners.clone();
            if corners.apply_or_discard(-1, cx, cy, first) {
                corners.apply_or_discard(-1, cx, cy, first ^ 0xF);
            }
            // The crater's cells row by row, those within radius + 1/2 cells
            // of the hit, with their squared distance from it. The half cell
            // gives the rim straight edges instead of a lone cell sticking
            // out on each axis.
            let area: Vec<_> = (cy - radius..=cy + radius)
                .flat_map(|y| (cx - radius..=cx + radius).map(move |x| (x, y)))
                .map(|(x, y)| (x, y, (x - cx).pow(2) + (y - cy).pow(2)))
                .filter(|&(_, _, d2)| 4 * d2 <= (2 * radius + 1).pow(2))
                .collect();
            for level in 1..=depth {
                // Twice the level's reach, times depth.
                let reach = (2 * radius + 1) * (depth - level + 1);
                for &(x, y, d2) in &area {
                    if 4 * d2 * depth * depth > reach * reach {
                        continue;
                    }
                    let floor = start
                        .cell_heights(x, y)
                        .map(|height| height - level * corner_heights::LEVEL);
                    let now = corners.cell_heights(x, y);
                    let mask = (0..4)
                        .filter(|&i| now[i] > floor[i])
                        .fold(0, |mask, i| mask | 1 << i);
                    if mask != 0 {
                        corners.apply_or_discard(-1, x, y, mask);
                    }
                }
            }
            corners.finalize(&cells, window_cells(origin, stride), tiles)
        };
        if finals.is_empty() {
            return false;
        }
        self.stamp_terrain_dent(&finals, rules, effects);
        true
    }

    /// What Firestorm's finalize (Firestorm 2.0 0x5F8D30) does to each cell
    /// it rewrites, through VERA's owners: the cell's smudge goes over its
    /// whole footprint (as `Map588AC0`), the cell takes its Level, slope and
    /// clear or ramp tile, and each object on its list is set back on the
    /// ground (`Set_Height_AGL(0)` after its Mark), the blast's dead too.
    /// Only vehicles and soldiers can be on a cell Firestorm rewrites; a
    /// building or aircraft VERA's crater digs under, dead, keeps its height
    /// while its death plays out. VERA then smooths the LAT and ramps around
    /// the stamped cells, as cliff collapse does, and publishes their
    /// lighting, navigation, radar and saved terrain.
    fn stamp_terrain_dent(
        &mut self,
        finals: &[FinalCell],
        rules: &RuleSet,
        effects: &mut FrameEffects<'_>,
    ) {
        if finals.is_empty() {
            return;
        }
        let feet: Vec<u64> = finals
            .iter()
            .filter_map(|f| self.substrate.occupancy.get(f.x as u16, f.y as u16))
            .flat_map(|cell| cell.iter_layer(MovementLayer::Ground))
            .map(|occupant| occupant.entity_id)
            .filter(|&id| self.substrate.entities.get(id).is_some_and(is_foot))
            .collect();
        if let (Some(grid), Some(terrain)) =
            (self.smudge_grid.as_mut(), self.resolved_terrain.as_ref())
        {
            for f in finals {
                // A smudge whose type is gone has nothing to clear.
                let _ = grid.clear_footprint_identity_at(
                    (f.x as i16, f.y as i16),
                    &rules.smudge_types,
                    terrain,
                    |identity, at, changed| {
                        if changed && matches!(identity, NativeCellIdentity::Real(_)) {
                            self.dirty_cells.mark_cell((at.0 as u16, at.1 as u16));
                        }
                    },
                );
            }
        }
        Self::flush_smudge_dirty(&mut self.smudge_grid, &mut self.dirty_cells);
        let Some(terrain) = self.resolved_terrain.as_mut() else {
            return;
        };
        let changed = terrain.stamp_deformation(finals);
        self.publish_cell_lighting_levels(changed.iter().copied());
        // Every stamped tile is a passable Clear or ramp tile, as the
        // Morphable land it replaced: zones stay, the path view republishes.
        self.publish_recalculated_cells(rules, &changed);
        for &coord in &changed {
            self.dirty_cells.mark_radar([coord]);
            self.dirty_cells.mark_tactical_once(coord);
        }
        let terrain = self.resolved_terrain.as_ref().expect("dented terrain");
        for &(x, y) in &changed {
            if let Some(cell) = terrain.cell(x, y) {
                self.dynamic_terrain_cells.record(cell);
            }
        }
        for id in feet {
            self.set_object_height(id, 0, rules, effects);
            let level = self.substrate.entities.get(id).and_then(|entity| {
                let terrain = self.resolved_terrain.as_ref()?;
                let cell = terrain.cell(entity.position.rx(), entity.position.ry())?;
                Some((cell.level, entity.on_bridge()))
            });
            if let (Some((level, on_bridge)), Some(entity)) =
                (level, self.substrate.entities.get_mut(id))
            {
                entity.position.store_cell_level(level, on_bridge);
            }
        }
    }
}

#[cfg(all(test, tests_sim_world_t_z))]
mod tests {
    use super::*;
    use crate::map::bridge_facts::BridgeCellFacts;
    use crate::map::resolved_terrain::{test_clear_cell, test_grid, test_smudge_cell};
    use crate::rules::ini_parser::IniFile;
    use crate::rules::terrain_rules::LandType;
    use crate::sim::occupancy::CellListInsertion;
    use crate::sim::snapshot::GameSnapshot;

    const RAMP_BASE: u16 = 600;
    /// The test ground's Level, above Firestorm's floor.
    const GROUND: u8 = 4;
    const GROUND_Z: i32 = GROUND as i32 * 104;

    /// `extra` follows the test warhead's keys: more of its keys, then any
    /// other sections.
    fn rules(enabled: bool, extra: &str) -> RuleSet {
        RuleSet::from_ini(&IniFile::from_str(&format!(
            "[InfantryTypes]\n[VehicleTypes]\n[AircraftTypes]\n[BuildingTypes]\n\
             [CombatDamage]\nTerrainDeformation={}\n\
             [Warheads]\n0=CRATERWH\n\
             [CRATERWH]\nDeform=100%\nDeformThreshhold=100\n\
             Verses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n{extra}",
            if enabled { "yes" } else { "no" },
        )))
        .expect("deformation rules")
    }

    /// Flat Morphable ground at Level 4.
    fn ground(rx: u16, ry: u16) -> ResolvedTerrainCell {
        ResolvedTerrainCell {
            level: GROUND,
            bridge_deck_level: GROUND,
            ..test_smudge_cell(rx, ry)
        }
    }

    /// [`ground`], 12 cells square, with one overlay.
    fn flat_sim(overlay: (u16, u16)) -> Simulation {
        sim_of(12, ground, overlay)
    }

    /// `size` cells square of `cell`s, with one overlay.
    fn sim_of(
        size: u16,
        cell: fn(u16, u16) -> ResolvedTerrainCell,
        overlay: (u16, u16),
    ) -> Simulation {
        let mut sim = Simulation::new();
        let mut terrain = test_grid(size, size, cell);
        terrain.test_install_deformation_catalog(0, RAMP_BASE);
        *sim.resolved_terrain = Some(terrain);
        let mut overlays = OverlayGrid::new(size, size);
        overlays.place_overlay(overlay.0, overlay.1, 0, 0);
        *sim.overlay_grid = Some(overlays);
        sim
    }

    fn level_slope(sim: &Simulation, x: u16, y: u16) -> (i8, u8) {
        let cell = sim.resolved_terrain.as_ref().unwrap().cell(x, y).unwrap();
        (cell.level as i8, cell.slope_type)
    }

    fn crater(sim: &mut Simulation, rules: &RuleSet, cell: (i16, i16)) -> bool {
        let warhead = rules.warhead("CRATERWH").expect("crater warhead");
        sim.dent_terrain(cell, rules, Some(warhead), &mut FrameEffects::default())
    }

    fn place(sim: &mut Simulation, (x, y): (u16, u16), category: EntityCategory) -> u64 {
        let id = sim.substrate.allocate_stable_id();
        let entity = GameEntity::test_default_of_category(id, "OBJ", "Americans", x, y, category);
        sim.substrate.entities.insert(entity);
        sim.substrate.occupancy.add(
            x,
            y,
            id,
            MovementLayer::Ground,
            None,
            CellListInsertion::PrependNonBuilding,
        );
        id
    }

    #[test]
    fn a_dent_sinks_the_drawn_corner_and_the_other_three_five_frames_later() {
        let rules = rules(true, "");
        let mut sim = flat_sim((0, 0));
        sim.session.set_binary_frame_for_test(100);
        let mut expected = sim.scenario_rng.clone();
        let corner = expected.next_range_u32_inclusive(0, 3);
        let effects = &mut FrameEffects::default();
        assert!(sim.dent_terrain((5, 5), &rules, None, effects));
        assert_eq!(sim.scenario_rng.logical_state(), expected.logical_state());
        // One corner a level down: slopes 9..=12 hold NW..SW low.
        assert_eq!(level_slope(&sim, 5, 5), (3, 9 + corner as u8));
        assert_eq!(
            *sim.terrain_dent,
            Some(PendingTerrainDent {
                cell: (5, 5),
                mask: 0xF ^ (1 << corner),
                frame: 105,
            })
        );
        sim.session.set_binary_frame_for_test(104);
        sim.finish_terrain_dent(&rules, effects);
        assert_eq!(level_slope(&sim, 5, 5), (3, 9 + corner as u8));
        sim.session.set_binary_frame_for_test(105);
        sim.finish_terrain_dent(&rules, effects);
        assert_eq!(level_slope(&sim, 5, 5), (3, 0));
        assert_eq!(level_slope(&sim, 5, 4), (3, 4));
        assert_eq!(*sim.terrain_dent, None);
        assert_eq!(sim.scenario_rng.logical_state(), expected.logical_state());
    }

    #[test]
    fn the_roll_dents_under_an_air_burst_but_not_under_a_bridge_deck_after_its_draw() {
        let bridged = |rx, ry| ResolvedTerrainCell {
            bridge_facts: BridgeCellFacts {
                raw_flags: if (rx, ry) == (5, 5) {
                    BRIDGE_FLAG_STRUCTURAL
                } else {
                    0
                },
                ..Default::default()
            },
            ..ground(rx, ry)
        };
        // (switch, bridge, height above the ground, damage, draws)
        for (enabled, bridge, height, damage, draws) in [
            (false, false, 0, 500, 0),
            (true, false, 0, 100, 0),
            (true, false, BRIDGE_DECK_HEIGHT_LEPTONS, 101, 2),
            (true, true, BRIDGE_DECK_HEIGHT_LEPTONS, 101, 1),
            (true, true, 0, 101, 2),
        ] {
            let rules = rules(enabled, "");
            let warhead = rules.warhead("CRATERWH").expect("crater warhead");
            let mut sim = if bridge {
                sim_of(12, bridged, (0, 0))
            } else {
                flat_sim((0, 0))
            };
            let mut expected = sim.scenario_rng.clone();
            for draw in 0..draws {
                let _ = expected.next_range_u32_inclusive(0, if draw == 0 { 99 } else { 3 });
            }
            let impact = ProjectileCoord::new(5 * 256 + 128, 5 * 256 + 128, GROUND_Z + height);
            sim.roll_terrain_deformation(
                &rules,
                warhead,
                impact,
                (5, 5),
                damage,
                &mut FrameEffects::default(),
            );
            assert_eq!(
                sim.scenario_rng.logical_state(),
                expected.logical_state(),
                "draws for {enabled} {bridge} {height} {damage}"
            );
            // Deform=100% of 101 damage is a 101% chance; the bridge cell
            // keeps its ground.
            let dented = draws == 2 && !bridge;
            assert_eq!(level_slope(&sim, 5, 5).0, GROUND as i8 - i8::from(dented));
        }
    }

    #[test]
    fn a_meteor_dents_its_cell_and_its_diagonals_at_crater_level_two_only_when_switched_on() {
        assert_eq!(rules(true, "").general.crater_level, 4);
        let landing = glam::IVec3::new(5 * 256 + 128, 5 * 256 + 128, GROUND_Z);
        for enabled in [false, true] {
            let rules = rules(enabled, "[General]\nCraterLevel=2\n");
            let mut sim = flat_sim((0, 0));
            let mut expected = sim.scenario_rng.clone();
            let dents = if enabled { 5 } else { 0 };
            for _ in 0..dents {
                let _ = expected.next_range_u32_inclusive(0, 3);
            }
            sim.dig_meteor_craters(landing, &rules, &mut FrameEffects::default());
            assert_eq!(sim.scenario_rng.logical_state(), expected.logical_state());
            // The north-west dent is the last: its second step is to come.
            assert_eq!(
                sim.terrain_dent.map(|dent| dent.cell),
                enabled.then_some((4, 4))
            );
            assert_eq!(sim.dynamic_terrain_cells.is_empty(), !enabled);
        }
    }

    #[test]
    fn a_dent_moves_vehicles_with_the_ground_but_not_a_building_or_a_tile_that_is_not_morphable() {
        let mut sim = sim_of(
            12,
            |rx, ry| {
                if (rx, ry) == (8, 5) {
                    ResolvedTerrainCell {
                        level: GROUND,
                        bridge_deck_level: GROUND,
                        ..test_clear_cell(rx, ry)
                    }
                } else {
                    ground(rx, ry)
                }
            },
            (0, 0),
        );
        let rules = rules(true, "");
        let tank = place(&mut sim, (5, 5), EntityCategory::Unit);
        sim.substrate
            .entities
            .get_mut(tank)
            .unwrap()
            .position
            .set_exact_z_leptons_for_test(Some(GROUND_Z));
        place(&mut sim, (2, 5), EntityCategory::Structure);
        let effects = &mut FrameEffects::default();
        sim.session.set_binary_frame_for_test(0);
        assert!(sim.dent_terrain((5, 5), &rules, None, effects));
        sim.session.set_binary_frame_for_test(SECOND_STEP_FRAMES);
        sim.finish_terrain_dent(&rules, effects);
        assert_eq!(level_slope(&sim, 5, 5), (GROUND as i8 - 1, 0));
        let position = sim.substrate.entities.get(tank).unwrap().position;
        assert_eq!(position.exact_z_leptons(), Some(GROUND_Z - 104));
        assert_eq!(position.z(), GROUND - 1);
        // The building's cell and the plain clear tile keep their ground.
        assert!(!sim.dent_terrain((2, 5), &rules, None, effects));
        assert!(!sim.dent_terrain((8, 5), &rules, None, effects));
        assert_eq!(level_slope(&sim, 2, 5), (GROUND as i8, 0));
        assert_eq!(level_slope(&sim, 8, 5), (GROUND as i8, 0));
    }

    #[test]
    fn a_dent_sinks_below_level_zero_around_an_overlay_cell_it_cannot_move() {
        // Level 0 ground and the rules' own floor, as a map that sets only
        // TerrainDeformation=yes.
        let rules = rules(true, "");
        let mut sim = sim_of(12, test_smudge_cell, (0, 0));
        let corner = sim.scenario_rng.clone().next_range_u32_inclusive(0, 3) as usize;
        // An overlay on the diagonal cell beyond the far corner locks it.
        let (dx, dy) = [(-1, -1), (1, -1), (1, 1), (-1, 1)][(corner + 2) % 4];
        let overlay = ((5 + dx) as u16, (5 + dy) as u16);
        let overlays = sim.overlay_grid.as_mut().expect("overlays");
        overlays.place_overlay(overlay.0, overlay.1, 0, 0);
        let effects = &mut FrameEffects::default();
        sim.session.set_binary_frame_for_test(0);
        assert!(sim.dent_terrain((5, 5), &rules, None, effects));
        assert_eq!(level_slope(&sim, 5, 5), (-1, 9 + corner as u8));
        // The second step would pull the locked corner down too, so it
        // leaves the ground as it is.
        sim.session.set_binary_frame_for_test(SECOND_STEP_FRAMES);
        sim.finish_terrain_dent(&rules, effects);
        assert_eq!(*sim.terrain_dent, None);
        assert_eq!(level_slope(&sim, 5, 5), (-1, 9 + corner as u8));
        assert_eq!(level_slope(&sim, overlay.0, overlay.1), (0, 0));
    }

    #[test]
    fn a_crater_digs_under_what_the_blast_killed_but_not_the_living() {
        let mut sim = flat_sim((0, 0));
        // The blast killed the building on the hit cell and the tank three
        // cells east; their deaths are still to be delivered.
        let building = place(&mut sim, (5, 5), EntityCategory::Structure);
        let tank = place(&mut sim, (8, 5), EntityCategory::Unit);
        for id in [building, tank] {
            let entity = sim.substrate.entities.get_mut(id).unwrap();
            entity.health.set_for_test(0);
            entity.position.set_exact_z_leptons_for_test(Some(GROUND_Z));
        }
        place(&mut sim, (2, 5), EntityCategory::Structure);
        let rules = rules(true, "DeformRadius=3\n");
        assert!(crater(&mut sim, &rules, (5, 5)));
        assert_eq!(level_slope(&sim, 5, 5), (GROUND as i8 - 1, 0));
        assert_eq!(level_slope(&sim, 8, 5), (GROUND as i8 - 1, 0));
        // The dead tank is set back on the sunken ground; the living
        // building keeps its cell.
        let position = sim.substrate.entities.get(tank).unwrap().position;
        assert_eq!(position.exact_z_leptons(), Some(GROUND_Z - 104));
        assert_eq!(level_slope(&sim, 2, 5), (GROUND as i8, 0));
    }

    #[test]
    fn a_crater_digs_flat_road_except_beside_water() {
        // Road tiles are not Morphable; one water cell east of the crater.
        let mut sim = sim_of(
            12,
            |rx, ry| ResolvedTerrainCell {
                level: GROUND,
                bridge_deck_level: GROUND,
                land_type: if (rx, ry) == (9, 5) {
                    LandType::Water
                } else {
                    LandType::Road
                }
                .as_index(),
                ..test_clear_cell(rx, ry)
            },
            (0, 0),
        );
        let rules = rules(true, "DeformRadius=3\n");
        assert!(crater(&mut sim, &rules, (5, 5)));
        assert_eq!(level_slope(&sim, 5, 5), (GROUND as i8 - 1, 0));
        assert_eq!(level_slope(&sim, 2, 5), (GROUND as i8 - 1, 0));
        // The road beside the water keeps its ground, as a shore cell would.
        assert_eq!(level_slope(&sim, 8, 5), (GROUND as i8, 0));
    }

    #[test]
    fn a_deeper_crater_widens_a_cell_per_level_until_ground_it_cannot_move() {
        let rules = rules(true, "DeformDepth=3\n");
        // Open ground: the hit cell sinks three levels and each ring around
        // it ramps up a level, so the fourth ring keeps its ground.
        let mut sim = flat_sim((0, 0));
        assert!(crater(&mut sim, &rules, (5, 5)));
        assert_eq!(level_slope(&sim, 5, 5), (1, 0));
        assert_eq!(level_slope(&sim, 5, 4), (1, 4));
        assert_eq!(level_slope(&sim, 5, 2), (3, 4));
        assert_eq!(level_slope(&sim, 5, 1), (4, 0));
        assert_eq!(*sim.terrain_dent, None);
        // The third level would pull down the corners of the overlay cell
        // three cells east, so the crater stops two levels deep.
        let mut sim = flat_sim((8, 5));
        assert!(crater(&mut sim, &rules, (5, 5)));
        assert_eq!(level_slope(&sim, 5, 5), (2, 0));
    }

    #[test]
    fn a_crater_radius_digs_a_terraced_bowl_around_ground_it_cannot_move() {
        let rules = rules(true, "DeformDepth=2\nDeformRadius=3\n");
        // Open ground: the middle 3x3 sinks two levels, the rest of the
        // radius one, and a ring of ramps climbs back to the ground.
        let mut sim = flat_sim((0, 0));
        assert!(crater(&mut sim, &rules, (5, 5)));
        assert_eq!(level_slope(&sim, 5, 5), (2, 0));
        assert_eq!(level_slope(&sim, 5, 3), (2, 4));
        assert_eq!(level_slope(&sim, 5, 2), (3, 0));
        assert_eq!(level_slope(&sim, 5, 1), (3, 4));
        assert_eq!(level_slope(&sim, 5, 0), (4, 0));
        // The rim reaches half a cell past the radius, so the cell (3, 1) off
        // the hit sinks too.
        assert_eq!(level_slope(&sim, 8, 6), (3, 0));
        // An overlay on the hit cell keeps its ground, and the crater forms
        // around it.
        let mut sim = flat_sim((5, 5));
        assert!(crater(&mut sim, &rules, (5, 5)));
        assert_eq!(level_slope(&sim, 5, 5), (4, 0));
        assert_eq!(level_slope(&sim, 5, 3), (3, 0));
        assert_eq!(level_slope(&sim, 5, 2), (3, 0));
    }

    #[test]
    fn a_crater_radius_reaches_the_widest_cell_spread_and_no_further() {
        let rules = rules(true, "DeformRadius=12\n");
        let mut sim = sim_of(30, ground, (0, 0));
        assert!(crater(&mut sim, &rules, (15, 15)));
        // Radius 11: the cell 11 east sinks, 12 east ramps back up, 13 east
        // keeps its ground.
        assert_eq!(level_slope(&sim, 26, 15), (3, 0));
        assert_eq!(level_slope(&sim, 28, 15), (4, 0));
    }

    #[test]
    fn a_dent_and_its_second_step_survive_a_save_and_load() {
        let rules = rules(true, "");
        let mut sim = flat_sim((0, 0));
        // The map's own terrain, as a load rebuilds from it: switched off, as
        // every new grid starts.
        let mut pristine = (*sim.resolved_terrain).clone().expect("terrain");
        pristine.set_terrain_deformation(false);
        // Scenario start's capture, which a load checks against the grid.
        *sim.real_cell_bridge_flags_0x1180 = pristine.capture_real_cell_bridge_flags_0x1180();
        assert!(sim.dent_terrain((5, 5), &rules, None, &mut FrameEffects::default()));

        let bytes = GameSnapshot::save(&sim, 0, 0, "crater", 0);
        let mut restored = GameSnapshot::load(&bytes).expect("load").sim;
        restored.restore_after_snapshot_load().expect("restore");
        restored.rebuild_caches_after_load(pristine, Default::default(), &rules);
        restored
            .restore_map_authority_after_snapshot_load(&rules, rules.overlay_registry(), None)
            .expect("map authority");

        assert_eq!(level_slope(&restored, 5, 5), level_slope(&sim, 5, 5));
        assert_eq!(*restored.dynamic_terrain_cells, *sim.dynamic_terrain_cells);
        assert!(restored.terrain_dent.is_some());
        assert_eq!(*restored.terrain_dent, *sim.terrain_dent);
        assert!(
            restored
                .resolved_terrain
                .as_ref()
                .expect("restored terrain")
                .terrain_deformation()
        );
    }
}
