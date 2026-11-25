//! Firestorm ice, world half (docs/ts-ice.md). The terrain half
//! (`map::resolved_terrain::ice`) knows the theater's ice tiles and writes
//! them; this owner runs Firestorm 2.0's chain over them with the Scenario
//! stream (cracking, breaking, the neighbour updates, growth and
//! solidifying) and publishes what changed to navigation, the radar and the
//! save. Every entry does nothing unless the map runs ice
//! ([`ResolvedTerrainGrid::ice_tiles`]), which stands in for Firestorm's
//! SNOW theater test (Scenario+0x7FC == 1): no draw, timer or hash input.
//!
//! [`ResolvedTerrainGrid::ice_tiles`]: crate::map::resolved_terrain::ResolvedTerrainGrid::ice_tiles

use super::{FrameEffects, SimSoundEvent, Simulation, UninitContext};
use crate::map::entities::EntityCategory;
use crate::map::resolved_terrain::IceTiles;
use crate::rules::locomotor_type::MovementZone;
use crate::rules::overlay_types::OverlayTypeRegistry;
use crate::rules::ruleset::RuleSet;
use crate::rules::warhead_type::WarheadType;
use crate::sim::anim_class::AnimWorldCoord;
use crate::sim::components::AnimClassSpawnDescriptor;
use crate::sim::movement::ground_pose::position_world_coord;
use crate::sim::movement::locomotor::MovementLayer;
use crate::sim::occupancy::CellObjectMember;
use crate::sim::projectile::ProjectileCoord;

/// A cracked cell waiting to freeze over: an entry of Firestorm 2.0
/// MapClass+0x14C, appended by Crack_Ice (0x52073d..0x52079a).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub(crate) struct CrackedIce {
    cell: (i16, i16),
    /// The crack's frame plus `IceSolidifyFrameTime=`; Solidify waits until
    /// the native frame has passed it.
    frame: i32,
}

/// Adjacent_Cell's offsets (Firestorm 2.0 0x4578e0, table 0x761128): north,
/// then clockwise.
const ADJACENT: [(i16, i16); 8] = [
    (0, -1),
    (1, -1),
    (1, 0),
    (1, 1),
    (0, 1),
    (-1, 1),
    (-1, 0),
    (-1, -1),
];

/// Edge_Ice's neighbours and mask bits (0x51fc12..0x51ffb2): north-east 1,
/// east 2, south-east 4, south 8, south-west 0x10, west 0x20, north-west
/// 0x40, north 0x80.
const EDGE_NEIGHBOURS: [((i16, i16), usize); 8] = [
    ((1, -1), 0x01),
    ((1, 0), 0x02),
    ((1, 1), 0x04),
    ((0, 1), 0x08),
    ((-1, 1), 0x10),
    ((-1, 0), 0x20),
    ((-1, -1), 0x40),
    ((0, -1), 0x80),
];

/// Edge_Ice's piece by solid-neighbour mask, Firestorm 2.0 table 0x6d3c04:
/// -1 open water, -2 piece 15.
#[rustfmt::skip]
const EDGE_PIECE: [i8; 256] = [
    -1, 33, 2, 2, 34, 37, 2, 2, 4, 26, 6, 6, 4, 26, 6, 6,
    35, 45, 17, 17, 38, 41, 17, 17, 4, 26, 6, 6, 4, 26, 6, 6,
    8, 21, 10, 10, 27, 31, 10, 10, 12, 23, 14, 14, 12, 23, 14, 14,
    8, 21, 10, 10, 27, 31, 10, 10, 12, 23, 14, 14, 12, 23, 14, 14,
    32, 36, 25, 25, 44, 40, 25, 25, 19, 30, 20, 20, 19, 30, 20, 20,
    39, 43, 29, 29, 42, 46, 29, 29, 19, 30, 20, 20, 19, 30, 20, 20,
    8, 21, 10, 10, 27, 31, 10, 10, 12, 23, 14, 14, 12, 23, 14, 14,
    8, 21, 10, 10, 27, 31, 10, 10, 12, 23, 14, 14, 12, 23, 14, 14,
    1, 1, 3, 3, 16, 16, 3, 3, 5, 5, 7, 7, 5, 5, 7, 7,
    24, 24, 18, 18, 28, 28, 18, 18, 5, 5, 7, 7, 5, 5, 7, 7,
    9, 9, 11, 11, 22, 22, 11, 11, 13, 13, -2, -2, 13, 13, -2, -2,
    9, 9, 11, 11, 22, 22, 11, 11, 13, 13, -2, -2, 13, 13, -2, -2,
    1, 1, 3, 3, 16, 16, 3, 3, 5, 5, 7, 7, 5, 5, 7, 7,
    24, 24, 18, 18, 28, 28, 18, 18, 5, 5, 7, 7, 5, 5, 7, 7,
    9, 9, 11, 11, 22, 22, 11, 11, 13, 13, -2, -2, 13, 13, -2, -2,
    9, 9, 11, 11, 22, 22, 11, 11, 13, 13, -2, -2, 13, 13, -2, -2,
];

/// Break_Ice's footprint step by the breaker's facing eighth, then 8 for an
/// explosion (table 0x8058e0). Rows 1, 3 and 6 repeat their neighbours and
/// none points north-west twice, as Firestorm fills it; with no breaker
/// every corner of the 2x2 footprint is the hit cell.
const BREAK_STEP: [(i16, i16); 9] = [
    (1, -1),
    (1, -1),
    (1, 1),
    (1, 1),
    (1, 1),
    (-1, 1),
    (-1, 1),
    (-1, -1),
    (0, 0),
];

fn offset((x, y): (i16, i16), (dx, dy): (i16, i16)) -> (i16, i16) {
    (x.wrapping_add(dx), y.wrapping_add(dy))
}

/// The four sides Update_Ice and Edge_Ice smooth, in their order: north,
/// south, east, west.
fn sides(coord: (i16, i16)) -> [(i16, i16); 4] {
    [(0, -1), (0, 1), (1, 0), (-1, 0)].map(|step| offset(coord, step))
}

/// A facing's eighth, `((facing >> 12) + 1) >> 1 & 7` (Break_Ice
/// 0x520962..0x52096f, UnitClass::AI 0x64e0a0..0x64e0ad).
pub(super) fn facing_eighth(facing: u16) -> usize {
    ((usize::from(facing >> 12) + 1) >> 1) & 7
}

/// One ice chain's records, from its caller's clear of the dirty list to
/// its publication.
struct IceRun {
    tiles: IceTiles,
    /// MapClass+0xDC, the coordinates Recalc_Ice recalculates. Update_Ice,
    /// Smooth and Edge move each one they touch to its end. Every caller
    /// that runs Recalc_Ice clears it first, so it never outlives a run.
    dirty: Vec<(i16, i16)>,
    /// Real cells whose tile changed.
    changed: Vec<(u16, u16)>,
    /// Real cells Recalc_Attributes (0x4531b0) ran on.
    recalced: Vec<(u16, u16)>,
    /// Recalc_Ice ran, with its whole zone rebuild.
    zones: bool,
}

impl IceRun {
    fn new(tiles: IceTiles) -> Self {
        Self {
            tiles,
            dirty: Vec::new(),
            changed: Vec::new(),
            recalced: Vec::new(),
            zones: false,
        }
    }

    /// DynamicVector Delete then Add: `coord` moves to the end.
    fn touch(&mut self, coord: (i16, i16)) {
        if let Some(index) = self.dirty.iter().position(|&dirty| dirty == coord) {
            self.dirty.remove(index);
        }
        self.dirty.push(coord);
    }

    fn note(cells: &mut Vec<(u16, u16)>, cell: (u16, u16)) {
        if !cells.contains(&cell) {
            cells.push(cell);
        }
    }
}

impl Simulation {
    /// The solidify list joins the world hash only while it holds a crack,
    /// so a game without ice hashes as before.
    pub(super) fn hash_ice_cracks(&self, hasher: &mut impl std::hash::Hasher) {
        use std::hash::Hash;
        if !self.ice_cracks.is_empty() {
            b"ice-cracks-v1".hash(hasher);
            self.ice_cracks.hash(hasher);
        }
    }

    fn ice_tiles(&self) -> Option<IceTiles> {
        self.resolved_terrain.as_ref()?.ice_tiles()
    }

    /// Cell+0x28 through the Map lookup.
    fn ice_tile(&self, coord: (i16, i16)) -> i32 {
        self.resolved_terrain
            .as_ref()
            .map_or(IceTiles::DUMMY_TILE, |terrain| terrain.ice_tile_at(coord))
    }

    /// RandomIceSet 0x4f89b0: one Scenario `Random_Pick(0, 2)` for an ice
    /// family's first tile.
    fn random_ice_set(&mut self, tiles: IceTiles) -> i32 {
        tiles.ice[self.scenario_rng.next_range_u32_inclusive(0, 2) as usize]
    }

    /// A tile write alone, as the shore arm, Grow and Solidify make it.
    fn write_ice_tile(&mut self, run: &mut IceRun, coord: (i16, i16), tile: i32) {
        if let Some(cell) = self
            .resolved_terrain
            .as_mut()
            .and_then(|terrain| terrain.set_ice_tile(coord, tile, false))
        {
            IceRun::note(&mut run.changed, cell);
        }
    }

    /// Recalc_Attributes 0x4531b0 on the cell at `coord`.
    fn recalc_ice_at(&mut self, run: &mut IceRun, coord: (i16, i16)) {
        if let Some(cell) = self
            .resolved_terrain
            .as_mut()
            .and_then(|terrain| terrain.recalc_ice_cell(coord))
        {
            IceRun::note(&mut run.recalced, cell);
        }
    }

    /// Tile, sub-tile 0 and Recalc_Attributes, as Smooth, Edge, Crack and
    /// Break stamp a cell.
    fn stamp_ice(&mut self, run: &mut IceRun, coord: (i16, i16), tile: i32) {
        if let Some(cell) = self
            .resolved_terrain
            .as_mut()
            .and_then(|terrain| terrain.set_ice_tile(coord, tile, true))
        {
            IceRun::note(&mut run.changed, cell);
            self.recalc_ice_at(run, coord);
        }
    }

    /// Smooth_Ice 0x51ef90 (its include-cracked argument is zero at every
    /// caller): solid or smoothed ice takes the piece for its open sides
    /// (north 1, east 2, south 4, west 8), base + 1 + mask, or a plain base
    /// with none open, from a random family unless it already has it.
    fn smooth_ice(&mut self, run: &mut IceRun, coord: (i16, i16)) {
        let tiles = run.tiles;
        let tile = self.ice_tile(coord);
        if !tiles.smooth_eligible(tile) {
            return;
        }
        let mask = [((0, -1), 1), ((1, 0), 2), ((0, 1), 4), ((-1, 0), 8)]
            .into_iter()
            .filter(|&(step, _)| tiles.smooth_open(self.ice_tile(offset(coord, step))))
            .fold(0, |mask, (_, bit)| mask | bit);
        if tiles.smooth_settled(tile, mask) {
            return;
        }
        let draw = self.random_ice_set(tiles);
        self.stamp_ice(run, coord, draw + mask + i32::from(mask != 0));
        run.touch(coord);
    }

    /// Edge_Ice 0x51fb30: water or an edge piece takes the piece table
    /// 0x6d3c04 gives for its solid neighbours. Open water (-1) is
    /// restamped from a random family even when it already is; another
    /// piece keeps an Ice1 or Ice2 tile that has it (not Ice3) and otherwise
    /// smooths the four sides after the stamp.
    fn edge_ice(&mut self, run: &mut IceRun, coord: (i16, i16)) {
        let tiles = run.tiles;
        let tile = self.ice_tile(coord);
        if !tiles.edge_eligible(tile) {
            return;
        }
        let mask = EDGE_NEIGHBOURS
            .into_iter()
            .filter(|&(step, _)| tiles.edge_solid(self.ice_tile(offset(coord, step))))
            .fold(0, |mask, (_, bit)| mask | bit);
        let piece = match EDGE_PIECE[mask] {
            -2 => 15,
            piece => i32::from(piece),
        };
        if piece < 0 {
            let draw = self.random_ice_set(tiles);
            self.stamp_ice(run, coord, draw + 0x3f);
            run.touch(coord);
            return;
        }
        let [ice1, ice2, _] = tiles.ice;
        if tile == ice1 + 0x10 + piece || tile == ice2 + 0x10 + piece {
            return;
        }
        let draw = self.random_ice_set(tiles);
        self.stamp_ice(run, coord, draw + 0x10 + piece);
        run.touch(coord);
        for side in sides(coord) {
            self.smooth_ice(run, side);
        }
    }

    /// Update_Ice 0x51eb40 (its RMG argument is zero at every caller, so
    /// the arm at 0x51f520 never runs): solid ice beside shore becomes
    /// Ice1's first edge piece (tile alone); the four sides are smoothed
    /// after the cell itself unless it is solid ice; then the cell is
    /// smoothed or edged and its eight neighbours edged.
    fn update_ice(&mut self, run: &mut IceRun, coord: (i16, i16)) {
        let tiles = run.tiles;
        let mut tile = self.ice_tile(coord);
        run.touch(coord);
        if tiles.is_base(tile)
            && [0, 2, 4, 6]
                .into_iter()
                .any(|dir| tiles.is_shore(self.ice_tile(offset(coord, ADJACENT[dir]))))
        {
            tile = tiles.ice[0] + 0x11;
            self.write_ice_tile(run, coord, tile);
        }
        let base = tiles.is_base(tile);
        if base {
            for side in sides(coord) {
                self.smooth_ice(run, side);
            }
        }
        if tiles.is_smoothed_family(tile) {
            self.smooth_ice(run, coord);
        } else if tiles.is_edge_piece(tile) {
            self.edge_ice(run, coord);
        }
        if !base {
            for side in sides(coord) {
                self.smooth_ice(run, side);
            }
        }
        for step in ADJACENT {
            self.edge_ice(run, offset(coord, step));
        }
    }

    /// Recalc_Ice 0x52c580: Recalc_Attributes on each dirty cell, newest
    /// first, then the zone rebuild (0x514a70) and its per-cell pass
    /// (0x52bdb0), which publication runs as one navigation rebuild.
    fn recalc_ice(&mut self, run: &mut IceRun) {
        let dirty = std::mem::take(&mut run.dirty);
        for &coord in dirty.iter().rev() {
            self.recalc_ice_at(run, coord);
        }
        run.zones = true;
    }

    /// Crack_Ice 0x520670: cracked ice breaks (Break_Ice); ice up to the
    /// last smoothed piece cracks to a random family's 0x10, waits
    /// `IceSolidifyFrameTime=` frames in the solidify list and plays one of
    /// `IceCrackSounds=` (a raw Scenario draw modulo their count) at full
    /// volume, not placed on the map. Returns Break_Ice's result; a crack
    /// alone returns false.
    fn crack_ice(
        &mut self,
        run: &mut IceRun,
        rules: &RuleSet,
        effects: &mut FrameEffects<'_>,
        cell: (u16, u16),
        breaker: Option<u64>,
    ) -> bool {
        if breaker.is_some_and(|id| {
            self.substrate
                .entities
                .get(id)
                .is_some_and(|entity| entity.on_bridge())
        }) {
            return false;
        }
        let tiles = run.tiles;
        let coord = (cell.0 as i16, cell.1 as i16);
        let tile = self.ice_tile(coord);
        if tiles.is_cracked(tile) {
            return self.break_ice(run, rules, effects, cell, breaker);
        }
        if !tiles.is_crackable(tile) {
            return false;
        }
        let draw = self.random_ice_set(tiles);
        self.stamp_ice(run, coord, draw + 0x10);
        self.update_ice(run, coord);
        self.recalc_ice_at(run, coord);
        let frame = (self.session.binary_frame() as i32)
            .wrapping_add(rules.general.ice.solidify_frame_time);
        self.ice_cracks.push(CrackedIce { cell: coord, frame });
        let sounds = &rules.general.ice.crack_sounds;
        if !sounds.is_empty() {
            let pick = self.scenario_rng.next_u32() % sounds.len() as u32;
            self.sound_events.emit(SimSoundEvent::VocCentered {
                sound_id: rules.sound_name(&sounds[pick as usize]),
            });
        }
        false
    }

    /// Break_Ice 0x520870: the 2x2 footprint from the hit cell, stepping
    /// along the breaker's facing eighth (table 0x8058e0; an explosion's
    /// footprint is the hit cell four times), breaks when all four cells
    /// hold water or ice. Each cell is stamped with a random family's broken
    /// piece 0x12, then each one's occupants go under. A breaker on a
    /// bridge breaks nothing.
    fn break_ice(
        &mut self,
        run: &mut IceRun,
        rules: &RuleSet,
        effects: &mut FrameEffects<'_>,
        cell: (u16, u16),
        breaker: Option<u64>,
    ) -> bool {
        let row = match breaker {
            Some(id) => {
                let Some(entity) = self.substrate.entities.get(id) else {
                    return false;
                };
                // RESIDUAL: Techno vt+0xBC (0x5870a0) also refuses a unit
                // off a bridge whose centre stands over three levels above
                // its coordinate's ground on a bridge cell; OnBridge stands
                // in for it.
                if entity.on_bridge() {
                    return false;
                }
                facing_eighth(entity.body_facing_current(self.session.binary_frame()))
            }
            None => 8,
        };
        let (dx, dy) = BREAK_STEP[row];
        let origin = (cell.0 as i16, cell.1 as i16);
        let footprint: Vec<(i16, i16)> = (0..2i16)
            .flat_map(|i| {
                (0..2i16).map(move |j| offset(origin, (i.wrapping_mul(dx), j.wrapping_mul(dy))))
            })
            .collect();
        if !footprint
            .iter()
            .all(|&coord| run.tiles.breakable(self.ice_tile(coord)))
        {
            return false;
        }
        for &coord in &footprint {
            let draw = self.random_ice_set(run.tiles);
            self.stamp_ice(run, coord, draw + 0x12);
            self.update_ice(run, coord);
            self.recalc_ice_at(run, coord);
        }
        for &coord in &footprint {
            self.sink_ice_occupants(rules, effects, coord);
        }
        true
    }

    /// Break_Ice's occupant pass (0x520b16..0x520cf3) over one cell's
    /// ground objects: infantry and aircraft spring their Tag's events 48
    /// then 29 while alive and are deleted with no kill; another Foot sinks
    /// (Unit+0x1F4, Stun) unless it is amphibious (MovementZone 3, 4 or 5),
    /// which skips it. Each one not skipped leaves a `Wake` anim at its
    /// centre (delay 0, loop 1, flags 0x600).
    fn sink_ice_occupants(
        &mut self,
        rules: &RuleSet,
        effects: &mut FrameEffects<'_>,
        coord: (i16, i16),
    ) {
        // The Dummy's object list is empty.
        let Some(cell) = self
            .resolved_terrain
            .as_ref()
            .and_then(|terrain| terrain.ice_cell_index(coord))
            .map(|_| (coord.0 as u16, coord.1 as u16))
        else {
            return;
        };
        let occupants: Vec<u64> = self
            .cell_objects(cell, MovementLayer::Ground)
            .filter_map(|member| match member {
                CellObjectMember::Entity(id) => Some(id),
                CellObjectMember::Terrain(_) => None,
            })
            .collect();
        for id in occupants {
            let Some(entity) = self.substrate.entities.get(id) else {
                continue;
            };
            // A dead object is already off its cell's list in Firestorm.
            if !entity.category().is_foot() || !entity.lifecycle.object_alive() {
                continue;
            }
            let category = entity.category();
            let zone = self
                .object_type(entity.type_ref(), rules)
                .map(|object| object.movement_zone);
            let centre = position_world_coord(&entity.position);
            let mut context = UninitContext::new(rules).with_effects(effects);
            match category {
                EntityCategory::Infantry | EntityCategory::Aircraft => {
                    for event in [48, 29] {
                        if self
                            .substrate
                            .entities
                            .get(id)
                            .is_some_and(|entity| entity.lifecycle.object_alive())
                        {
                            self.dispatch_object_tag_event(
                                id,
                                event,
                                Some(id),
                                None,
                                context.reborrow(),
                            );
                        }
                    }
                    self.uninit_with_context(id, context);
                }
                _ if matches!(
                    zone,
                    Some(
                        MovementZone::AmphibiousDestroyer
                            | MovementZone::AmphibiousCrusher
                            | MovementZone::Amphibious
                    )
                ) =>
                {
                    continue;
                }
                _ => self.begin_ice_sinking(id, context),
            }
            let wake = AnimWorldCoord {
                x: centre.x,
                y: centre.y,
                z: centre.z,
            };
            let type_id = self.interner.intern(&rules.general.wake.name);
            let (rx, ry, sub_x, sub_y, z) = wake.to_cell_sub_z();
            let descriptor = AnimClassSpawnDescriptor {
                draw_flags: 0x600,
                ..AnimClassSpawnDescriptor::new(type_id, rx, ry, sub_x, sub_y, z)
            };
            if let Err(error) = self.spawn_anim_at_world(rules, descriptor, wake) {
                log::debug!(
                    "ice wake [{}] did not construct: {error}",
                    rules.general.wake.name
                );
            }
        }
    }

    /// Grow_Ice 0x520d20, with `IceGrowthEnabled=`: walking the map with
    /// its cell iterator (0x51e120), flag each cell its map lets grow ice
    /// (Cell+0x91, the IsoMapPack5 byte) that holds Ice1 or Ice2 past solid
    /// ice and not cracked; then, in the same order, freeze each flagged
    /// cell whose tile is still past Ice1's cracked tile but at most Ice2's
    /// first, or past Ice2's cracked tile, to Ice1's solid tile (tile alone)
    /// and update it. Returns whether any cell froze.
    fn grow_ice(&mut self, run: &mut IceRun) -> bool {
        if !self.session.ice_growth_enabled() {
            return false;
        }
        let tiles = run.tiles;
        let Some(terrain) = self.resolved_terrain.as_ref() else {
            return false;
        };
        let flag = |cell: &crate::map::resolved_terrain::ResolvedTerrainCell| {
            (cell.legacy_ice_byte != 0 && tiles.grow_candidate(cell.final_tile_index))
                .then_some((cell.rx as i16, cell.ry as i16))
        };
        let flagged: Vec<(i16, i16)> = match self.map_size_diamond() {
            Some((width, _)) => terrain
                .native_cell_iterator(width)
                .filter_map(flag)
                .collect(),
            // Rectangular test grids have no Map Size; walk them row by row.
            None => terrain.iter().filter_map(flag).collect(),
        };
        let mut changed = false;
        for coord in flagged {
            if tiles.grow_refreezes(self.ice_tile(coord)) {
                self.write_ice_tile(run, coord, tiles.ice[0]);
                self.update_ice(run, coord);
                changed = true;
            }
        }
        changed
    }

    /// Firestorm 2.0 LogicClass::AI 0x506eb7..0x506f6b, before the tiberium
    /// updates: with `IceGrowthRate=` neither zero nor NaN, each time the
    /// Scenario ice timer runs out it restarts at `ftol(rate * 900.0)`
    /// frames, the dirty list clears and Grow runs, then Recalc_Ice when
    /// anything froze.
    pub(crate) fn tick_ice_growth(
        &mut self,
        rules: &RuleSet,
        registry: Option<&OverlayTypeRegistry>,
    ) {
        let Some(tiles) = self.ice_tiles() else {
            return;
        };
        let rate = rules.general.ice.growth_rate;
        if rate == 0.0
            || rate.is_nan()
            || !self
                .session
                .poll_ice_growth(crate::rules::ruleset::native_minutes_to_frames(rate))
        {
            return;
        }
        let mut run = IceRun::new(tiles);
        if self.grow_ice(&mut run) {
            self.recalc_ice(&mut run);
        }
        self.publish_ice_run(run, rules, registry);
    }

    /// Solidify_Ice 0x520eb0, called by LogicClass::AI 0x506f84 after the
    /// tiberium updates, with `IceGrowthEnabled=`: newest first, each crack
    /// whose frame has passed and whose cell still holds offsets 1..=0x10
    /// of a family freezes to a random family's solid tile, as does each
    /// cracked side (north, east, south, west, a draw each, tiles alone);
    /// then the cell is updated and recalculated and each side updated, and
    /// the entry leaves the list. Any other entry stays, so a crack that
    /// broke stays listed for good (RESIDUAL in docs/ts-ice.md).
    pub(crate) fn solidify_ice(&mut self, rules: &RuleSet, registry: Option<&OverlayTypeRegistry>) {
        let Some(tiles) = self.ice_tiles() else {
            return;
        };
        if self.ice_cracks.is_empty() || !self.session.ice_growth_enabled() {
            return;
        }
        let now = self.session.binary_frame() as i32;
        let mut run = IceRun::new(tiles);
        for index in (0..self.ice_cracks.len()).rev() {
            let CrackedIce { cell: coord, frame } = self.ice_cracks[index];
            if frame >= now || !tiles.solidifies(self.ice_tile(coord)) {
                continue;
            }
            let draw = self.random_ice_set(tiles);
            self.write_ice_tile(&mut run, coord, draw);
            let compass = [0, 2, 4, 6].map(|dir| offset(coord, ADJACENT[dir]));
            for side in compass {
                if tiles.is_cracked(self.ice_tile(side)) {
                    let draw = self.random_ice_set(tiles);
                    self.write_ice_tile(&mut run, side, draw);
                }
            }
            self.update_ice(&mut run, coord);
            self.recalc_ice_at(&mut run, coord);
            for side in compass {
                self.update_ice(&mut run, side);
            }
            self.ice_cracks.remove(index);
        }
        self.publish_ice_run(run, rules, registry);
    }

    /// Firestorm 2.0 UnitClass::Per_Cell_Process 0x651890..0x651975, on
    /// arrival after the mission promotion: the dirty list clears; a unit
    /// whose `Weight=` reaches `IceBreakingWeight=` breaks the ice at its
    /// centre, else one reaching `IceCrackingWeight=` cracks it. When the
    /// ice broke the unit sinks (Unit+0x1F4, Stun) whatever its
    /// MovementZone, and Recalc_Ice runs.
    ///
    /// VERA deviation: Firestorm cracks and breaks from whatever cell the
    /// unit reached, so a heavy enough ship or hovercraft would turn open
    /// water into broken ice and go under. Here only a unit on ice (offsets
    /// 0..=0x10, the only Ice land the theater patch leaves an ice family)
    /// does. Firestorm reaches no other case: its only unit at
    /// `IceBreakingWeight=` is a ground unit, and none that can leave Ice
    /// land reaches `IceCrackingWeight=`.
    pub(crate) fn unit_arrival_ice(
        &mut self,
        id: u64,
        rules: &RuleSet,
        registry: Option<&OverlayTypeRegistry>,
        effects: &mut FrameEffects<'_>,
    ) {
        let Some(tiles) = self.ice_tiles() else {
            return;
        };
        let Some(entity) = self.substrate.entities.get(id) else {
            return;
        };
        let Some(weight) = self
            .object_type(entity.type_ref(), rules)
            .map(|object| object.weight)
        else {
            return;
        };
        let cell = (entity.position.rx(), entity.position.ry());
        if !tiles.is_smoothed_family(self.ice_tile((cell.0 as i16, cell.1 as i16))) {
            return;
        }
        let ice = &rules.general.ice;
        let mut run = IceRun::new(tiles);
        let broke = if weight >= ice.breaking_weight {
            self.break_ice(&mut run, rules, effects, cell, Some(id))
        } else if weight >= ice.cracking_weight {
            self.crack_ice(&mut run, rules, effects, cell, Some(id))
        } else {
            return;
        };
        if broke {
            self.begin_ice_sinking(id, UninitContext::new(rules).with_effects(effects));
            self.recalc_ice(&mut run);
        }
        self.publish_ice_run(run, rules, registry);
    }

    /// Firestorm 2.0 Explosion_Damage 0x4601b4..0x46024f, after the
    /// warhead's particle system: a `Wall=` or `Fire=` warhead cracks the
    /// ice of the cell it hits (an explosion breaks cracked ice), unless it
    /// went off at or above a bridge deck over that cell; Recalc_Ice runs
    /// when the ice broke.
    pub(crate) fn explosion_ice(
        &mut self,
        rules: &RuleSet,
        registry: Option<&OverlayTypeRegistry>,
        effects: &mut FrameEffects<'_>,
        warhead: &WarheadType,
        impact: ProjectileCoord,
        cell: (u16, u16),
    ) {
        let Some(tiles) = self.ice_tiles() else {
            return;
        };
        if !warhead.wall && !warhead.fire {
            return;
        }
        // RESIDUAL: Firestorm tests its cell flag 0x1000 (0x4601d7); the
        // bridge head flag stands in for it.
        if self.cell_has_bridge_head(cell) {
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
            if ground.is_some_and(|ground| {
                impact.z >= ground + crate::util::lepton::BRIDGE_DECK_HEIGHT_LEPTONS
            }) {
                return;
            }
        }
        let mut run = IceRun::new(tiles);
        if self.crack_ice(&mut run, rules, effects, cell, None) {
            self.recalc_ice(&mut run);
        }
        self.publish_ice_run(run, rules, registry);
    }

    /// Publish one run: the overlay land each Recalc reset, the path view
    /// of each recalculated cell (gamemd's pathfinder reads cells live),
    /// the radar and tactical redraw of each changed tile, and each changed
    /// or recalculated cell for the save. Ice tiles keep their cell's
    /// level, so lighting stays.
    ///
    /// Sim-scale deviation: Recalc_Ice rebuilds the zones (0x514a70) at
    /// once, each time ice breaks; here the frame tail rebuilds them once
    /// (`finalize_frame_overlays_and_navigation`), so a frame with many
    /// breaks pays one rebuild. Until then the path view already holds the
    /// new land; only zone connectivity lags, within the frame.
    fn publish_ice_run(
        &mut self,
        run: IceRun,
        rules: &RuleSet,
        registry: Option<&OverlayTypeRegistry>,
    ) {
        if run.changed.is_empty() && run.recalced.is_empty() {
            return;
        }
        if let (Some(grid), Some(terrain), Some(registry)) = (
            self.overlay_grid.as_mut(),
            self.resolved_terrain.as_mut(),
            registry,
        ) {
            for &(x, y) in &run.recalced {
                crate::sim::overlay_grid::recalc_overlay_passability(grid, terrain, registry, x, y);
            }
        }
        self.publish_recalculated_cells(rules, &run.recalced);
        *self.ice_zones_stale |= run.zones;
        for &cell in &run.changed {
            self.dirty_cells.mark_radar([cell]);
            self.dirty_cells.mark_tactical_once(cell);
        }
        if let Some(terrain) = self.resolved_terrain.as_ref() {
            for &(x, y) in run.changed.iter().chain(&run.recalced) {
                if let Some(cell) = terrain.cell(x, y) {
                    self.dynamic_terrain_cells.record(cell);
                }
            }
        }
    }
}

#[cfg(all(test, tests_sim_world_c_o))]
#[path = "ice_tests.rs"]
mod tests;
