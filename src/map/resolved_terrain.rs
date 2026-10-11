//! Resolved terrain/topology stage built from raw map cells plus theater/TMP metadata.
//!
//! This module sits between `MapFile` parsing and downstream consumers such as
//! rendering, pathfinding, and building placement. It materializes raw
//! IsoMapPack5 tile values into theater-compatible actual IDs, then attaches
//! resolved per-cell metadata such as final LAT-adjusted tile choice,
//! land/slope bytes from TMP, and coarse blocking/buildability flags.

/// Zone classification constants matching gamemd.exe RecalcZoneType output.
/// These index columns of `MOVEMENT_ZONE_PASSABILITY` in pathfinding/passability.rs.
pub mod zone_class {
    pub const GROUND: u8 = 0;
    pub const CRUSHABLE: u8 = 1;
    pub const WALL: u8 = 2;
    pub const BEACH: u8 = 3;
    pub const WATER: u8 = 4;
    pub const BUILDING: u8 = 5;
    pub const IMPASSABLE: u8 = 6;
    pub const OUTSIDE: u8 = 7;
}

use crate::assets::tmp_file::{TmpFile, TmpTile};
use crate::map::authored_overlay::{FinalizedOverlayCell, NO_OVERLAY_IDENTITY};
use crate::map::bridge_facts::{
    BRIDGE_FLAG_ANCHOR_SELF, BRIDGE_FLAG_DESTROYED_OR_RAMP, BRIDGE_FLAG_STRUCTURAL,
    BridgeAnchorRelation, BridgeCellFacts, BridgeFlagStamp, BridgeStampFamily, BridgeStampSlot,
    MODELED_CELLCLASS_BRIDGE_FLAG_MASK, RETAINED_CELLCLASS_BRIDGE_FLAG_MASK,
};
use crate::map::cell_index::NativeCellIdentity;
use crate::map::lat;
use crate::map::map_file::{MapCell, MapFile};
use crate::map::overlay::OverlayEntry;
use crate::map::playfield::PlayfieldBounds;
use crate::map::rmg::preview::Playfield;
use crate::map::theater::{self, TheaterData, TileKey, TilesetLookup};
use crate::map::tile_variant_selector::TileVariantSelectionContext;
use crate::map::tube_facts::{NativeTubeCellIndex, TubeFact, TubeId};
use crate::map::tubes::NativeMapTubeReceipt;
use crate::rules::overlay_types::{
    OverlayTypeFlags, OverlayTypeRegistry, clears_tiberium_on_slope, retained_overlay_land,
    uses_early_recalc_land_branch,
};
use crate::rules::terrain_object_type::TerrainObjectType;
use crate::rules::terrain_rules::{LandType, SpeedCostProfile, TerrainClass, TerrainRules};
use crate::util::pixel_conversion::PixelConversionBounds;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::{
    Arc,
    atomic::{AtomicI16, AtomicI32, AtomicU8, AtomicU32, AtomicU64, Ordering},
};

mod bridge_flags;
mod build;
mod cell_recalc;
mod cliffs;
mod mutation;
mod native_cell;
mod pavement;
mod recalc_catalog;
mod tubes;

#[cfg(test)]
mod damaged_radar_tests;
#[cfg(test)]
mod pavement_catalog_tests;
#[cfg(test)]
mod tests;

pub(crate) use bridge_flags::RealCellBridgeFlags0x1180;
pub(crate) use build::{
    AuthoredTerrainFill, OverlayLoadSource, RadarColorMetadata, current_tile_permissions,
};
use build::{presentation_tile_parts, wheel_speed_at_or_below_one_percent};
pub(crate) use cell_recalc::{
    AutomaticTubeAllocation, AutomaticTubeRequest, LoadCellRecalcEffects, LoadCellRecalcError,
    LoadCellRecalcOutcome, LoadCellRecalcState,
};
use cliffs::DestroyableCliffCatalog;
pub(crate) use native_cell::CellClassProjectionView;
pub use native_cell::{NativeCellQuery, SharedCellDummy, SharedCellDummySnapshot};
use recalc_catalog::BridgeRecalcCatalog;
pub(crate) use recalc_catalog::BridgeRecalcCatalogError;
#[cfg(test)]
pub(crate) use tests::{
    bridge_constructor_terrain, install_bridge_batch_test_catalog,
    install_bridge_query_test_catalog, install_no_registered_tiles_test_catalog,
    install_ordinary_repair_test_catalog, install_tunnel_repair_test_catalog,
};
pub(crate) use tubes::NativeTubeRegistryBindError;

pub const YR_CELL_LAND_TUNNEL: u8 = 10;

/// Exact overlay portion of RecalcZoneType. `Some(GROUND)` is the terminal
/// IsRubble result and is intentionally distinct from no overlay result.
pub(crate) fn overlay_reduced_zone_type(flags: Option<&OverlayTypeFlags>) -> Option<u8> {
    let flags = flags?;
    if flags.crushable {
        Some(zone_class::CRUSHABLE)
    } else if flags.wall {
        Some(zone_class::WALL)
    } else if flags.land_wheel_speed_zero || flags.is_a_rock {
        Some(zone_class::IMPASSABLE)
    } else if flags.is_rubble {
        Some(zone_class::GROUND)
    } else {
        None
    }
}

/// Shared RecalcZoneType priority writer for load and runtime attribute changes.
pub(crate) fn recalc_zone_type(
    outside: bool,
    overlay_zone_type: Option<u8>,
    land_type: u8,
    wheel_speed: Option<u8>,
    terrain_object_occupation: Option<u8>,
) -> u8 {
    if outside {
        zone_class::OUTSIDE
    } else if let Some(zone_type) = overlay_zone_type {
        zone_type
    } else if land_type == LandType::Water.as_index() {
        zone_class::WATER
    } else if land_type == LandType::Beach.as_index() {
        zone_class::BEACH
    } else if wheel_speed_at_or_below_one_percent(wheel_speed) {
        zone_class::IMPASSABLE
    } else if terrain_object_occupation == Some(7) {
        zone_class::WALL
    } else if terrain_object_occupation.is_some() {
        zone_class::BUILDING
    } else {
        zone_class::GROUND
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
/// Canonical ramp direction from TS++ TIBSUN_DEFINES.H (slope types 1-4).
/// These are the four basic full-edge ramps where two adjacent corners are raised.
///
/// Names are in **map coordinates** (as defined by TS++). In the isometric view,
/// map-North appears as screen upper-right. The actual tilt angles used for VXL
/// rendering come from the slope_type number (1-16) indexed into a pre-computed
/// matrix table — they don't depend on these labels.
pub enum RampDirection {
    West,
    North,
    East,
    South,
}

/// Bridge direction as expressed by the map overlay class. Do not derive high
/// bridge SHP body frames directly from these labels; rendering follows the
/// runtime bridge state-byte family (`Axis::NS => 0..=8`, `Axis::EW => 9..=17`).
/// Low bridges have no height offset.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum BridgeDirection {
    /// BRIDGE1, BRIDGEB1 — EW direction. Height offset = CellHeight + 1 = 16px.
    EastWest,
    /// BRIDGE2, BRIDGEB2 — NS direction. Height offset = CellHeight * 2 + 1 = 31px.
    NorthSouth,
    /// LOBRDG*, LOBRDB* — ground-level bridge. No height offset.
    Low,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct BridgeLayer {
    pub overlay_id: u8,
    pub overlay_name: String,
    /// Bridge deck height level (ground level + offset).
    pub deck_level: u8,
    /// Bridge direction — determines height offset and rendering.
    pub direction: BridgeDirection,
}

#[derive(Debug, Clone)]
pub struct ResolvedTerrainCell {
    pub rx: u16,
    pub ry: u16,
    pub source_tile_index: i32,
    pub source_sub_tile: u8,
    pub final_tile_index: i32,
    pub final_sub_tile: u8,
    /// True when `final_tile_index` falls in the first 16 tiles of the
    /// theater's `WoodBridgeSet`. This is the CellClass+0x38 predicate used
    /// by Engineer CABHUT bridge-repair dispatch.
    pub is_wood_bridge_repair_tile: bool,
    pub level: u8,
    pub filled_clear: bool,
    pub tileset_index: Option<u16>,
    /// Canonical effective LandType after TMP and overlay derivation.
    pub land_type: u8,
    /// Retained CellClass LandType mirror for binary-derived predicates and
    /// compatibility consumers. Reduced `zone_type`, not either Land field,
    /// selects the movement matrix column.
    pub yr_cell_land_type: u8,
    pub slope_type: u8,
    /// Registered pristine TMP +0x28 height field (CellClass-owned).
    pub template_height: u8,
    /// Signed low-byte result of the pristine TMP effective-height formula.
    pub height_in_pixels: i8,
    /// Tactical render origin owned by the selected independent TMP.
    pub render_offset_x: i32,
    pub render_offset_y: i32,
    pub terrain_class: TerrainClass,
    pub speed_costs: SpeedCostProfile,
    pub is_water: bool,
    pub is_cliff_like: bool,
    pub is_rough: bool,
    pub is_road: bool,
    /// True when this cell's tileset has `Morphable=yes`. Smudge placement
    /// requires this gate (matches gamemd IsoTileTypeClass+0x2E0).
    pub accepts_smudge: bool,
    /// True when this cell's final resolved tile has `AllowTiberium=yes`.
    /// TIBTRE placement validation uses the current tile type, not the source
    /// map tile, matching gamemd IsoTileTypeClass+0x306.
    pub allows_tiberium: bool,
    /// Tile visual variant index: 0 = pristine, positive = suffix sibling.
    pub variant: u8,
    pub has_ramp: bool,
    pub canonical_ramp: Option<RampDirection>,
    pub ground_walk_blocked: bool,
    pub terrain_object_blocks: bool,
    /// Selected current-theater occupation byte for a present terrain object.
    /// `Some(0)` preserves presence without physical blockage.
    pub terrain_object_occupation: Option<u8>,
    pub overlay_blocks: bool,
    /// Explicit terminal overlay result used by RecalcZoneType. In particular,
    /// `Some(GROUND)` preserves IsRubble priority across later mutations.
    pub overlay_zone_type: Option<u8>,
    /// Production CellClass playfield result. Synthetic `from_cells` grids set
    /// this false so focused rectangular fixtures remain in-playfield.
    pub outside_playfield: bool,
    /// Cached zone classification (0-7) matching gamemd.exe RecalcZoneType (0x483C80).
    /// Indexes columns of `MOVEMENT_ZONE_PASSABILITY` in pathfinding/passability.rs.
    ///
    /// 0=Ground, 1=Crushable overlay, 2=Wall, 3=Beach, 4=Water,
    /// 5=Building/TerrainObject, 6=Impassable, 7=Outside.
    ///
    /// Dynamic building footprints are never inferred from `PathGrid` here or
    /// during zone construction. Their object-loop class writer belongs to the
    /// occupancy lifecycle and must update this byte before connectivity repair.
    /// Overlay mutation updates it through `recalc_overlay_passability`.
    pub zone_type: u8,
    /// Terrain-only walk block flag — true when the base terrain (rock, cliff) is
    /// impassable, EXCLUDING overlay and terrain-object contributions.
    /// Needed by `recalc_overlay_passability` to re-derive zone_type after overlay
    /// removal without the conflated `ground_walk_blocked` field.
    pub base_ground_walk_blocked: bool,
    pub base_build_blocked: bool,
    /// Underlying `land_type` before any overlay override (tiberium / road).
    /// Used by `recalc_overlay_passability` to restore the pre-overlay value
    /// when an ore overlay is fully harvested (or any other overlay removed).
    /// Same pattern as `base_ground_walk_blocked`.
    pub base_land_type: u8,
    /// Underlying `yr_cell_land_type` (binary CellClass+0xEC) before overlay
    /// overrides.
    pub base_yr_cell_land_type: u8,
    /// Underlying terrain classification before overlay overrides.
    pub base_terrain_class: TerrainClass,
    /// Underlying terrain speed-cost profile before overlay overrides. Restored
    /// on overlay removal so harvested ore cells revert to the original
    /// terrain's per-locomotor speed table.
    pub base_speed_costs: SpeedCostProfile,
    pub has_bridge_deck: bool,
    pub bridge_walkable: bool,
    pub bridge_transition: bool,
    pub bridge_deck_level: u8,
    pub bridge_layer: Option<BridgeLayer>,
    pub bridge_facts: BridgeCellFacts,
    /// Compatibility projection of an in-range nonnegative
    /// `CellClass+0x116` value. The exact signed authority is retained by the
    /// owning `ResolvedTerrainGrid`.
    pub tube_index: Option<TubeId>,
    /// Selected tactical owner's radar color for the diamond's left half.
    pub radar_left: [u8; 3],
    /// Selected tactical owner's radar color for the diamond's right half.
    pub radar_right: [u8; 3],
    /// True if this cell's registered pristine TMP sub-tile carries a baked
    /// damaged-variant pixel set. Drives the kickoff gate of the bridge damage flood-fill (only
    /// cells with baked damage art may initiate propagation) and the render-side
    /// substitution that swaps in variant=1 when the bridge sim flags the cell.
    pub has_damaged_data: bool,
}

impl ResolvedTerrainCell {
    /// Refresh byte-encoded height views of the current Cell, independently of
    /// its Recalc-owned class, flags and cached topology. Preserve the encoded
    /// signed-level +4 result: raw252 (-4) has a structural deck at level0.
    /// Native47B3A0/5F5FA0: tools/ramp_height_{oracle.py,vectors.json}; this
    /// byte projection does not establish signed world-coordinate consumers.
    fn refresh_current_height_projection(&mut self) {
        if let Some(layer) = self.bridge_layer.as_mut() {
            layer.deck_level = match layer.direction {
                BridgeDirection::Low => self.level,
                _ => self.level.wrapping_add(4),
            };
        }
        self.bridge_deck_level = if self.bridge_facts.has_structural_bridge()
            || self
                .bridge_layer
                .as_ref()
                .is_some_and(|layer| layer.direction != BridgeDirection::Low)
        {
            self.level.wrapping_add(4)
        } else {
            self.level
        };
    }

    pub fn is_walkable(&self) -> bool {
        !self.ground_walk_blocked
    }

    pub fn is_bridge_transition_cell(&self) -> bool {
        self.bridge_transition
    }

    pub fn is_elevated_bridge_cell(&self) -> bool {
        self.bridge_walkable && self.bridge_deck_level > self.level
    }

    pub fn bridge_deck_level_if_any(&self) -> Option<u8> {
        self.has_bridge_deck.then_some(self.bridge_deck_level)
    }

    pub fn bridge_flags(&self) -> u32 {
        self.bridge_facts.raw_flags
    }

    pub fn is_low_bridge_tube_cell(&self) -> bool {
        self.tube_index.is_some() && self.yr_cell_land_type == YR_CELL_LAND_TUNNEL
    }
}

/// One terrain-attached animation the map load resolved for a cell.
///
/// `CellClass::RecalcAttributes` spawns one AnimClass per cell whose tile
/// declares a `Tile%02dAnim` block and whose sub-tile equals that block's
/// `AttachesTo`, then latches a per-cell flag so no later attribute recompute
/// spawns a second one. Waterfalls and tunnel mouths are the stock users.
///
/// This is the load-time descriptor only: the animation's frame set and cadence
/// come from the named AnimType's `art(md).ini` row, and the spawner owns the
/// AnimClass lifecycle.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TerrainTileAnimation {
    pub rx: u16,
    pub ry: u16,
    /// AnimType id from `Tile%02dAnim` (e.g. `WA01X`, `TUNTOP01`).
    pub anim_name: String,
    /// Absolute world leptons: pixel offset converted to world, plus the cell
    /// centre.
    pub world_x: i32,
    pub world_y: i32,
    /// Cell ground height in leptons. The pixel-offset conversion contributes
    /// nothing here — it is a 2D screen-to-world transform.
    pub world_z: i32,
    /// `Tile%02dZAdjust`, forwarded to the spawned animation's sort bias.
    pub z_adjust: i32,
}

/// Leptons per cell along one map axis.
const LEPTONS_PER_CELL: i32 = crate::util::lepton::LEPTONS_PER_CELL_I32;
/// Cell-centre offset used by the tile-animation spawn coordinate.
const CELL_CENTRE_LEPTONS: i32 = LEPTONS_PER_CELL / 2;

/// Serialized behavior authority for a CellClass whose isometric tile was
/// replaced after map construction. The live ResolvedTerrainGrid is derived
/// and skipped by Simulation snapshots, so collapse projects these exact
/// fields into Simulation and reapplies them after map reconstruction.
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub(crate) struct DynamicTerrainCellState {
    pub final_tile_index: i32,
    pub final_sub_tile: u8,
    pub is_wood_bridge_repair_tile: bool,
    pub level: u8,
    pub filled_clear: bool,
    pub tileset_index: Option<u16>,
    pub land_type: u8,
    pub yr_cell_land_type: u8,
    pub slope_type: u8,
    pub template_height: u8,
    pub height_in_pixels: i8,
    pub render_offset_x: i32,
    pub render_offset_y: i32,
    pub terrain_class: TerrainClass,
    pub speed_costs: SpeedCostProfile,
    pub is_water: bool,
    pub is_cliff_like: bool,
    pub is_rough: bool,
    pub is_road: bool,
    pub accepts_smudge: bool,
    pub allows_tiberium: bool,
    pub variant: u8,
    pub has_ramp: bool,
    pub canonical_ramp: Option<RampDirection>,
    pub ground_walk_blocked: bool,
    pub overlay_blocks: bool,
    pub overlay_zone_type: Option<u8>,
    pub zone_type: u8,
    pub base_ground_walk_blocked: bool,
    pub base_build_blocked: bool,
    pub base_land_type: u8,
    pub base_yr_cell_land_type: u8,
    pub base_terrain_class: TerrainClass,
    pub base_speed_costs: SpeedCostProfile,
    pub has_bridge_deck: bool,
    pub bridge_walkable: bool,
    pub bridge_transition: bool,
    pub bridge_deck_level: u8,
    pub bridge_layer: Option<BridgeLayer>,
    pub bridge_facts: BridgeCellFacts,
    pub radar_left: [u8; 3],
    pub radar_right: [u8; 3],
    pub has_damaged_data: bool,
}

impl DynamicTerrainCellState {
    pub(crate) fn capture(cell: &ResolvedTerrainCell) -> Self {
        Self {
            final_tile_index: cell.final_tile_index,
            final_sub_tile: cell.final_sub_tile,
            is_wood_bridge_repair_tile: cell.is_wood_bridge_repair_tile,
            level: cell.level,
            filled_clear: cell.filled_clear,
            tileset_index: cell.tileset_index,
            land_type: cell.land_type,
            yr_cell_land_type: cell.yr_cell_land_type,
            slope_type: cell.slope_type,
            template_height: cell.template_height,
            height_in_pixels: cell.height_in_pixels,
            render_offset_x: cell.render_offset_x,
            render_offset_y: cell.render_offset_y,
            terrain_class: cell.terrain_class,
            speed_costs: cell.speed_costs,
            is_water: cell.is_water,
            is_cliff_like: cell.is_cliff_like,
            is_rough: cell.is_rough,
            is_road: cell.is_road,
            accepts_smudge: cell.accepts_smudge,
            allows_tiberium: cell.allows_tiberium,
            variant: cell.variant,
            has_ramp: cell.has_ramp,
            canonical_ramp: cell.canonical_ramp,
            ground_walk_blocked: cell.ground_walk_blocked,
            overlay_blocks: cell.overlay_blocks,
            overlay_zone_type: cell.overlay_zone_type,
            zone_type: cell.zone_type,
            base_ground_walk_blocked: cell.base_ground_walk_blocked,
            base_build_blocked: cell.base_build_blocked,
            base_land_type: cell.base_land_type,
            base_yr_cell_land_type: cell.base_yr_cell_land_type,
            base_terrain_class: cell.base_terrain_class,
            base_speed_costs: cell.base_speed_costs,
            has_bridge_deck: cell.has_bridge_deck,
            bridge_walkable: cell.bridge_walkable,
            bridge_transition: cell.bridge_transition,
            bridge_deck_level: cell.bridge_deck_level,
            bridge_layer: cell.bridge_layer.clone(),
            bridge_facts: cell.bridge_facts,
            radar_left: cell.radar_left,
            radar_right: cell.radar_right,
            has_damaged_data: cell.has_damaged_data,
        }
    }

    pub(crate) fn apply(&self, cell: &mut ResolvedTerrainCell) {
        cell.final_tile_index = self.final_tile_index;
        cell.final_sub_tile = self.final_sub_tile;
        cell.is_wood_bridge_repair_tile = self.is_wood_bridge_repair_tile;
        cell.level = self.level;
        cell.filled_clear = self.filled_clear;
        cell.tileset_index = self.tileset_index;
        cell.land_type = self.land_type;
        cell.yr_cell_land_type = self.yr_cell_land_type;
        cell.slope_type = self.slope_type;
        cell.template_height = self.template_height;
        cell.height_in_pixels = self.height_in_pixels;
        cell.render_offset_x = self.render_offset_x;
        cell.render_offset_y = self.render_offset_y;
        cell.terrain_class = self.terrain_class;
        cell.speed_costs = self.speed_costs;
        cell.is_water = self.is_water;
        cell.is_cliff_like = self.is_cliff_like;
        cell.is_rough = self.is_rough;
        cell.is_road = self.is_road;
        cell.accepts_smudge = self.accepts_smudge;
        cell.allows_tiberium = self.allows_tiberium;
        cell.variant = self.variant;
        cell.has_ramp = self.has_ramp;
        cell.canonical_ramp = self.canonical_ramp;
        cell.ground_walk_blocked = self.ground_walk_blocked;
        cell.overlay_blocks = self.overlay_blocks;
        cell.overlay_zone_type = self.overlay_zone_type;
        cell.zone_type = self.zone_type;
        cell.base_ground_walk_blocked = self.base_ground_walk_blocked;
        cell.base_build_blocked = self.base_build_blocked;
        cell.base_land_type = self.base_land_type;
        cell.base_yr_cell_land_type = self.base_yr_cell_land_type;
        cell.base_terrain_class = self.base_terrain_class;
        cell.base_speed_costs = self.base_speed_costs;
        cell.has_bridge_deck = self.has_bridge_deck;
        cell.bridge_walkable = self.bridge_walkable;
        cell.bridge_transition = self.bridge_transition;
        cell.bridge_deck_level = self.bridge_deck_level;
        cell.bridge_layer = self.bridge_layer.clone();
        cell.bridge_facts = self.bridge_facts;
        cell.radar_left = self.radar_left;
        cell.radar_right = self.radar_right;
        cell.has_damaged_data = self.has_damaged_data;
    }
}

#[derive(Debug, Clone)]
struct PristineTmpHeader {
    subtiles: Vec<(i32, bool)>,
    total_file_count: usize,
}

impl PristineTmpHeader {
    fn damaged_data(&self, sub: u8) -> bool {
        self.subtiles[usize::from(sub) % self.subtiles.len()].1
    }
}

#[derive(Debug, Clone)]
pub struct ResolvedTerrainGrid {
    width: u16,
    height: u16,
    /// Immutable projection of the scenario's TerrainRules speed rows. Real
    /// cells retain their resolved row; the shared Dummy retains only Land
    /// (+EC), so CheckCellPassability4834A0 indexes this same rule source.
    /// Rebuilt with the grid on each scenario load, including empty grids;
    /// independent of whether a theater/bridge asset catalog was available.
    land_speed_costs: [SpeedCostProfile; LandType::ALL.len()],
    /// Pristine TMP dimensions used by Techno draw depth (0x547150/0x704350).
    /// Derived asset data, keyed by tile identity rather than mutable cell:
    /// never serialized or included in simulation hashes.
    native_tmp_draw_heights: HashMap<u16, PristineTmpHeader>,
    #[cfg(test)]
    pub(crate) cells: Vec<ResolvedTerrainCell>,
    #[cfg(not(test))]
    cells: Vec<ResolvedTerrainCell>,
    /// One live identity handle, normally bound to the process owner before
    /// scenario construction. Synthetic constructors own a fresh detached
    /// handle; derived clones share it.
    shared_cell_dummy: SharedCellDummy,
    /// Bumped on every `cell_mut` access. Many internal writers index `cells`
    /// directly and do not bump it; the one production writer of
    /// `terrain_object_occupation` (the movement blocker plane's only terrain
    /// input) goes through `cell_mut`. A bump without a change only costs a
    /// rebuild.
    mutation_epoch: std::cell::Cell<u64>,
    /// Production-only membership for native Size-diamond CellClass slots.
    /// `None` keeps synthetic/from_cells grids rectangular for focused tests.
    native_allocated: Option<Vec<bool>>,
    /// Selected TMP subimage-pointer validity, aligned with `cells`. Black
    /// header RGB remains valid and therefore cannot be used as the sentinel.
    radar_color_valid: Vec<bool>,
    /// Parsed first sibling TMP radar metadata for pristine cells whose
    /// subimage advertises damaged data. `None` means native VariantCount is
    /// below two (or the sibling could not be loaded), so bit 0x2000 wraps to
    /// the pristine chain head instead of inventing a damaged color.
    damaged_radar_metadata: Vec<Option<Result<RadarColorMetadata, BridgeRecalcCatalogError>>>,
    tube_facts: Vec<TubeFact>,
    /// Exact signed `CellClass+0x116` authority aligned with `cells`.
    native_tube_indices: Vec<NativeTubeCellIndex>,
    /// Load-only native identities aligned with `tube_facts`. Explicit rows and
    /// automatic Recalc shells share this one registry order.
    tube_native_ids: Vec<Option<i32>>,
    /// Theater `[General] ClearTile` resolved to a flat tile id.
    ///
    /// Presentation consumers use this for `NO_TILE` cells while
    /// `ResolvedTerrainCell::final_tile_index` retains the sentinel for sim.
    clear_tile_id: u16,
    /// Active theater tile-family bases: WaterSet (global AA0738, not the
    /// sticky Fill cache), ShorePieces (ABAD28) and the four waterfall sets.
    /// ReadTheater545150 resets each to -1; Lunar keeps -1 for them.
    water_tiles: crate::map::rmg::tiles::TileIds,
    /// Active theater tile registry length. Positive out-of-range ids present
    /// as ClearTile while their stored semantic id remains untouched.
    tile_registry_len: Option<usize>,
    /// Smudge CanPlace's Morphable read of the shared dummy's constructor
    /// tile 0xFFFF, derived once from the active theater registry: every
    /// index below 0 or at least the count reads theater tile 0. False
    /// without a theater.
    dummy_accepts_smudge: bool,
    /// First flat tile id of the active theater's concrete high-bridge set.
    bridge_set_start: Option<u16>,
    /// Immutable signed ReadTheater545150 identities for high-rim selection.
    high_bridge_rim_tiles: Option<super::bridge_rim_tiles::HighBridgeRimTiles>,
    /// First flat tile id of the active theater's wooden high-bridge set.
    wood_bridge_set_start: Option<u16>,
    /// Terrain animations the load resolved, in the native anti-diagonal cell
    /// order so a spawner reproduces the engine's animation creation order.
    tile_animations: Vec<TerrainTileAnimation>,
    /// Immutable active-theater sparse TMP authority used only by the two
    /// verified destroyable-cliff callers.
    destroyable_cliff_catalog: Option<DestroyableCliffCatalog>,
    /// Immutable pristine/policy inputs, shared by derived clones and snapshot
    /// templates. Mutable terrain, bounds and overlays remain separate owners.
    bridge_recalc_catalog: Option<Arc<BridgeRecalcCatalog>>,
}

/// A `size` x `size` flat level-0 grid whose land rows admit Foot, Track and
/// Wheel at 100%: the inputs the native Foot `Can_Enter_Cell` reads.
#[cfg(test)]
pub(crate) fn test_flat_ground_grid(size: u16) -> ResolvedTerrainGrid {
    let costs = SpeedCostProfile {
        foot: Some(100),
        track: Some(100),
        wheel: Some(100),
        ..Default::default()
    };
    test_grid(size, size, |rx, ry| ResolvedTerrainCell {
        speed_costs: costs,
        base_speed_costs: costs,
        ..test_flat_cell(rx, ry)
    })
}

/// A `width` x `height` grid of `cell(rx, ry)`, in row-major order.
#[cfg(test)]
pub(crate) fn test_grid(
    width: u16,
    height: u16,
    mut cell: impl FnMut(u16, u16) -> ResolvedTerrainCell,
) -> ResolvedTerrainGrid {
    let mut cells = Vec::with_capacity(usize::from(width) * usize::from(height));
    for ry in 0..height {
        for rx in 0..width {
            cells.push(cell(rx, ry));
        }
    }
    ResolvedTerrainGrid::from_cells(width, height, cells)
}

/// Every SpeedType at 100%.
#[cfg(test)]
pub(crate) const TEST_OPEN_SPEED_COSTS: SpeedCostProfile = SpeedCostProfile {
    foot: Some(100),
    track: Some(100),
    wheel: Some(100),
    float: Some(100),
    amphibious: Some(100),
    float_beach: Some(100),
    hover: Some(100),
};

/// [`test_flat_cell`] whose base terrain class is Clear.
#[cfg(test)]
pub(crate) fn test_clear_cell(rx: u16, ry: u16) -> ResolvedTerrainCell {
    ResolvedTerrainCell {
        base_terrain_class: TerrainClass::Clear,
        ..test_flat_cell(rx, ry)
    }
}

/// [`test_clear_cell`] that accepts smudges.
#[cfg(test)]
pub(crate) fn test_smudge_cell(rx: u16, ry: u16) -> ResolvedTerrainCell {
    ResolvedTerrainCell {
        accepts_smudge: true,
        ..test_clear_cell(rx, ry)
    }
}

/// [`test_smudge_cell`] that also allows tiberium.
#[cfg(test)]
pub(crate) fn test_tiberium_cell(rx: u16, ry: u16) -> ResolvedTerrainCell {
    ResolvedTerrainCell {
        allows_tiberium: true,
        ..test_smudge_cell(rx, ry)
    }
}

/// [`test_flat_cell`] that is `filled_clear`, has an Unknown terrain class
/// (`TerrainClass`'s default) and accepts smudges.
#[cfg(test)]
pub(crate) fn test_unclassified_cell(rx: u16, ry: u16) -> ResolvedTerrainCell {
    ResolvedTerrainCell {
        filled_clear: true,
        terrain_class: TerrainClass::default(),
        accepts_smudge: true,
        ..test_flat_cell(rx, ry)
    }
}

/// [`test_smudge_cell`] as the map loader fills a clear cell: `filled_clear`
/// and no tileset.
#[cfg(test)]
pub(crate) fn test_loader_clear_cell(rx: u16, ry: u16) -> ResolvedTerrainCell {
    ResolvedTerrainCell {
        filled_clear: true,
        tileset_index: None,
        ..test_smudge_cell(rx, ry)
    }
}

/// A fully-populated flat `ResolvedTerrainCell`, for tests in other modules that
/// need a grid and should not carry a 47-field copy of this literal.
#[cfg(test)]
pub(crate) fn test_flat_cell(rx: u16, ry: u16) -> ResolvedTerrainCell {
    ResolvedTerrainCell {
        rx,
        ry,
        source_tile_index: 0,
        source_sub_tile: 0,
        final_tile_index: 0,
        final_sub_tile: 0,
        is_wood_bridge_repair_tile: false,
        level: 0,
        filled_clear: false,
        tileset_index: Some(0),
        land_type: 0,
        yr_cell_land_type: 0,
        slope_type: 0,
        template_height: 0,
        render_offset_x: 0,
        render_offset_y: 0,
        terrain_class: TerrainClass::Clear,
        speed_costs: SpeedCostProfile::default(),
        is_water: false,
        is_cliff_like: false,
        height_in_pixels: 0,
        variant: 0,
        is_rough: false,
        is_road: false,
        accepts_smudge: false,
        allows_tiberium: false,
        has_ramp: false,
        canonical_ramp: None,
        ground_walk_blocked: false,
        terrain_object_blocks: false,
        terrain_object_occupation: None,
        overlay_blocks: false,
        overlay_zone_type: None,
        outside_playfield: false,
        zone_type: 0,
        base_ground_walk_blocked: false,
        base_build_blocked: false,
        base_land_type: 0,
        base_yr_cell_land_type: 0,
        base_terrain_class: Default::default(),
        base_speed_costs: Default::default(),
        has_bridge_deck: false,
        bridge_walkable: false,
        bridge_transition: false,
        bridge_deck_level: 0,
        bridge_layer: None,
        bridge_facts: crate::map::bridge_facts::BridgeCellFacts::default(),
        tube_index: None,
        radar_left: [0, 0, 0],
        radar_right: [0, 0, 0],
        has_damaged_data: false,
    }
}

fn land_speed_cost_profiles(
    rules: Option<&TerrainRules>,
) -> [SpeedCostProfile; LandType::ALL.len()] {
    std::array::from_fn(|index| {
        rules
            .and_then(|rules| rules.semantics_for_land_type(LandType::ALL[index].as_index()))
            .map_or_else(SpeedCostProfile::default, |row| row.speed_costs)
    })
}

impl ResolvedTerrainGrid {
    pub fn from_cells(width: u16, height: u16, cells: Vec<ResolvedTerrainCell>) -> Self {
        Self::from_cells_with_tubes(width, height, cells, Vec::new())
    }

    pub fn from_cells_with_tubes(
        width: u16,
        height: u16,
        cells: Vec<ResolvedTerrainCell>,
        tube_facts: Vec<TubeFact>,
    ) -> Self {
        let tube_native_ids = vec![None; tube_facts.len()];
        let native_tube_indices = cells
            .iter()
            .map(|cell| {
                cell.tube_index
                    .map_or(NativeTubeCellIndex::NONE, |tube_id| {
                        NativeTubeCellIndex::from_registry_ordinal(tube_id.as_usize())
                    })
            })
            .collect();
        let radar_color_valid = cells
            .iter()
            .map(|cell| cell.radar_left != [0, 0, 0] || cell.radar_right != [0, 0, 0])
            .collect();
        let damaged_radar_metadata = vec![None; cells.len()];
        Self {
            width,
            height,
            cells,
            land_speed_costs: land_speed_cost_profiles(None),
            native_tmp_draw_heights: HashMap::new(),
            shared_cell_dummy: SharedCellDummy::fresh(),
            mutation_epoch: std::cell::Cell::new(0),
            native_allocated: None,
            radar_color_valid,
            damaged_radar_metadata,
            tube_facts,
            native_tube_indices,
            tube_native_ids,
            clear_tile_id: 0,
            water_tiles: crate::map::rmg::tiles::TileIds::from_keys(&Default::default()),
            tile_registry_len: None,
            dummy_accepts_smudge: false,
            bridge_set_start: None,
            high_bridge_rim_tiles: None,
            wood_bridge_set_start: None,
            tile_animations: Vec::new(),
            destroyable_cliff_catalog: None,
            bridge_recalc_catalog: None,
        }
    }

    /// Original4835D5..4835DE indexes the 89EA40 table using retained Land.
    /// The existing TerrainRules owner supplies these rows; this grid does
    /// not parse or independently mutate rules. Missing/invalid rows keep the
    /// existing unbound-profile compatibility policy, also used by real cells.
    pub(crate) fn land_speed_costs(&self, land: i32) -> SpeedCostProfile {
        usize::try_from(land)
            .ok()
            .and_then(|index| self.land_speed_costs.get(index))
            .copied()
            .unwrap_or_default()
    }

    #[cfg(test)]
    pub(crate) fn set_land_speed_rules_for_test(&mut self, rules: &TerrainRules) {
        self.land_speed_costs = land_speed_cost_profiles(Some(rules));
    }

    /// Terrain animations to spawn once at map load, in native creation order.
    pub fn tile_animations(&self) -> &[TerrainTileAnimation] {
        &self.tile_animations
    }

    pub fn width(&self) -> u16 {
        self.width
    }

    #[cfg(test)]
    pub(crate) fn projectile_water_set_base(&self) -> i32 {
        self.water_tiles.water_base
    }

    #[cfg(test)]
    pub(crate) fn set_projectile_water_set_base(&mut self, base: i32) {
        self.water_tiles.water_base = base;
    }

    /// `CellClass @ 0x004865D0` (the database's HasBridgeOverlay), which
    /// reads only the cell's tile (`+0x38`): a WaterSet, ShorePieces or
    /// waterfall tile ([`TileIds::is_water_shore_or_waterfall`], the one
    /// port). The shared dummy's tile is 0xFFFF.
    ///
    /// RESIDUAL: that port skips a family whose base is -1; native compares
    /// against the -1 base too, so on Lunar (no WaterSet or ShorePieces)
    /// tiles 0..40 answer true. Trigger: a Chrono Warp landing on a Lunar
    /// cell its object cannot enter (PostWarpValidation). Effect: VERA
    /// destroys the object where native sinks it.
    ///
    /// [`TileIds::is_water_shore_or_waterfall`]:
    /// crate::map::rmg::tiles::TileIds::is_water_shore_or_waterfall
    pub(crate) fn native_cell_is_water_tile(&self, cell: NativeCellIdentity) -> bool {
        self.water_tiles
            .is_water_shore_or_waterfall(self.native_cell_tile_index(cell))
    }

    /// `CellClass @ 0x00485060`: the cell's tile (`+0x38`) is one of the 14
    /// open-water tiles from the theater's WaterSet base (`0x00AA0738`), a
    /// signed window that the -1 base of a theater without one (Lunar) does
    /// not disable: there tiles -1..12 answer true. CellClass vt+0x50
    /// (`0x004867E0`) answers its negation, and Is_Clear_To_Build inlines it
    /// for a Naval building (`0x0047CA0F..0x0047CA25`).
    pub(crate) fn native_cell_is_water_set_tile(&self, cell: NativeCellIdentity) -> bool {
        self.tile_is_water_set(self.native_cell_tile_index(cell))
    }

    /// [`Self::native_cell_is_water_set_tile`] on a tile already read from a
    /// cell's `+0x38`.
    pub(crate) fn tile_is_water_set(&self, tile: i32) -> bool {
        let base = self.water_tiles.water_base;
        tile >= base && tile < base.wrapping_add(14)
    }

    pub(crate) fn current_tile_radar_metadata(
        &self,
        rx: u16,
        ry: u16,
    ) -> Option<RadarColorMetadata> {
        let index = self.index(rx, ry)?;
        let cell = self.cells.get(index)?;
        if cell.bridge_facts.raw_flags & super::bridge_pavement::DAMAGED_PAVEMENT != 0
            && let Some(metadata) = self
                .damaged_radar_metadata
                .get(index)
                .and_then(Option::as_ref)
        {
            // A non-null invalid native RGB pointer has no available color.
            // Preserve that boundary instead of displaying pristine or gray.
            return metadata.as_ref().ok().copied();
        }
        Some(RadarColorMetadata {
            left: cell.radar_left,
            right: cell.radar_right,
            valid: self.radar_color_valid.get(index).copied().unwrap_or(false),
        })
    }

    #[cfg(test)]
    pub(crate) fn test_set_damaged_radar_metadata(
        &mut self,
        rx: u16,
        ry: u16,
        metadata: RadarColorMetadata,
    ) {
        let index = self.index(rx, ry).expect("test damaged-radar cell exists");
        self.damaged_radar_metadata[index] = Some(Ok(metadata));
    }

    pub fn height(&self) -> u16 {
        self.height
    }

    /// Native GetSubtileDimensions height, not the union image canvas height.
    /// 0x704350 uses the pristine TMP even when a damaged sibling is displayed.
    pub(crate) fn native_tmp_draw_height(&self, tile: i32, sub_tile: u8) -> i32 {
        let key = if matches!(tile, -1 | 0xFF | 0xFFFF) {
            (self.clear_tile_id, 0)
        } else if let Ok(tile) = u16::try_from(tile) {
            (tile, sub_tile)
        } else {
            return 30;
        };
        if let Some(heights) = self.native_tmp_draw_heights.get(&key.0) {
            // 0x547165..0x547174 divides by the complete template cell count.
            return heights.subtiles[usize::from(key.1) % heights.subtiles.len()].0;
        }
        // VERA fallback for synthetic/no-assets grids or a missing/invalid
        // registered header (diagnosed once at load). This is not a claim of
        // native behavior for unavailable data; valid production types use
        // their pristine header above, including future live replacements.
        30
    }

    /// Original5471F0 uses the registered pristine head and wraps unsigned11A
    /// modulo its complete template size. No sentinel/ClearTile substitution:
    /// the56E990 caller owns its two explicit sentinel checks. None diagnoses
    /// missing resident input; it is not native's sparse-entry false result.
    pub(crate) fn native_tmp_has_damaged_data(&self, tile: i32, sub: u8) -> Option<bool> {
        let tile = u16::try_from(tile).ok()?;
        self.native_tmp_draw_heights
            .get(&tile)
            .map(|head| head.damaged_data(sub))
    }

    /// Tactical480350 pins the pristine file when the native file count is
    /// below two. Keep this distinct from the pristine damaged-data predicate.
    pub(crate) fn native_tmp_file_count(&self, tile: i32) -> Option<usize> {
        let tile = u16::try_from(tile).ok()?;
        self.native_tmp_draw_heights
            .get(&tile)
            .map(|head| head.total_file_count)
    }

    pub(crate) fn concrete_bridge_set_base(&self) -> i32 {
        self.bridge_set_start.map_or(-1, i32::from)
    }

    /// Cell486750 selects the concrete bridge's first sixteen tile IDs.
    /// It reads neither the bridge occupancy flags nor the wooden tile set.
    /// Mirage746997 uses this predicate for its neighbour Infantry list.
    pub(crate) fn native_cell_is_concrete_bridge(&self, cell: NativeCellIdentity) -> bool {
        let base = self.concrete_bridge_set_base();
        let tile = self.native_cell_tile_index(cell);
        base != -1 && tile >= base && tile < base.wrapping_add(16)
    }

    pub(crate) fn wood_bridge_set_base(&self) -> i32 {
        self.wood_bridge_set_start.map_or(-1, i32::from)
    }

    pub(crate) fn high_bridge_rim_tiles(
        &self,
    ) -> Option<super::bridge_rim_tiles::HighBridgeRimTiles> {
        self.high_bridge_rim_tiles
    }

    #[cfg(test)]
    pub(crate) fn test_set_high_bridge_rim_tiles(
        &mut self,
        tiles: super::bridge_rim_tiles::HighBridgeRimTiles,
    ) {
        self.high_bridge_rim_tiles = Some(tiles);
    }

    /// Resolve a cell's presentation tile without rewriting its semantic tile.
    ///
    /// Active YR substitutes the theater ClearTile and sub-tile zero when the
    /// stored IsoTileTypeIndex is absent, the no-tile sentinel, or outside the
    /// active theater registry.
    pub fn presentation_tile(&self, cell: &ResolvedTerrainCell) -> (u16, u8) {
        let positive_out_of_range = self.tile_registry_len.is_some_and(|len| {
            cell.final_tile_index >= 0
                && cell.final_tile_index != 0xFFFF
                && cell.final_tile_index as usize >= len
        });
        if cell.filled_clear || positive_out_of_range {
            return (self.clear_tile_id, 0);
        }
        presentation_tile_parts(
            cell.final_tile_index,
            cell.final_sub_tile,
            self.clear_tile_id,
        )
    }

    pub fn index(&self, rx: u16, ry: u16) -> Option<usize> {
        if rx < self.width && ry < self.height {
            let index = ry as usize * self.width as usize + rx as usize;
            if self
                .native_allocated
                .as_ref()
                .is_some_and(|mask| !mask.get(index).copied().unwrap_or(false))
            {
                None
            } else {
                Some(index)
            }
        } else {
            None
        }
    }

    pub fn cell(&self, rx: u16, ry: u16) -> Option<&ResolvedTerrainCell> {
        self.index(rx, ry).and_then(|i| self.cells.get(i))
    }

    pub fn radar_color_valid(&self, rx: u16, ry: u16) -> bool {
        self.index(rx, ry)
            .and_then(|index| self.radar_color_valid.get(index))
            .copied()
            .unwrap_or(false)
    }

    #[cfg(test)]
    pub(crate) fn test_set_radar_color_valid(&mut self, rx: u16, ry: u16, valid: bool) {
        if let Some(index) = self.index(rx, ry)
            && let Some(slot) = self.radar_color_valid.get_mut(index)
        {
            *slot = valid;
        }
    }

    /// Immutable cells in the grid's established storage order.
    pub fn cells(&self) -> &[ResolvedTerrainCell] {
        &self.cells
    }

    /// Mutable cell access that moves `mutation_epoch`. The terrain-object
    /// occupation writer (`set_terrain_object_occupation`) goes through here,
    /// and that occupation flag is the only terrain field the movement
    /// blocker-plane cache (`MovementPassCache`) reads, so the epoch is a
    /// sound key for that cache; other internal writers index `cells`
    /// directly and do not move it (see the field note), which stays fine
    /// only while the plane reads no other terrain field. The bump has to
    /// run in game builds as well as tests: a tree destroyed mid-match must
    /// invalidate the plane in the shipped binary. An earlier revision bumped
    /// only a `cfg(test)` body, which the suite could not detect.
    pub(crate) fn cell_mut(&mut self, rx: u16, ry: u16) -> Option<&mut ResolvedTerrainCell> {
        let idx = self.index(rx, ry)?;
        self.mutation_epoch
            .set(self.mutation_epoch.get().wrapping_add(1));
        self.cells.get_mut(idx)
    }

    /// Epoch of mutable cell access; see the field note.
    pub fn mutation_epoch(&self) -> u64 {
        self.mutation_epoch.get()
    }

    pub(crate) fn apply_dynamic_cell_state(
        &mut self,
        rx: u16,
        ry: u16,
        state: &DynamicTerrainCellState,
    ) -> bool {
        let Some(index) = self.index(rx, ry) else {
            return false;
        };
        // Scalar-only updates (including56E990 pavement) keep the exact
        // resident TMP entry and its sparse/valid damaged sibling metadata.
        // A changed entry reconstructs independent presentation inputs from
        // the resident catalog, including sparse/error damaged radar results.
        let same_entry = self.cells[index].final_tile_index == state.final_tile_index
            && self.cells[index].final_sub_tile == state.final_sub_tile
            && self.cells[index].variant == state.variant;
        let before = (!same_entry && self.bridge_recalc_catalog.is_some())
            .then(|| self.cells[index].clone());
        state.apply(&mut self.cells[index]);
        if !same_entry {
            if let Some(before) = before {
                if let Err(error) = self.refresh_resident_bridge_presentation(index) {
                    self.cells[index] = before;
                    log::error!("terrain snapshot presentation unavailable at {rx},{ry}: {error}");
                    return false;
                }
            } else {
                self.radar_color_valid[index] = true;
                self.damaged_radar_metadata[index] = None;
            }
        }
        true
    }

    pub fn iter(&self) -> impl Iterator<Item = &ResolvedTerrainCell> {
        self.cells.iter().enumerate().filter_map(|(index, cell)| {
            self.native_allocated
                .as_ref()
                .is_none_or(|mask| mask.get(index).copied().unwrap_or(false))
                .then_some(cell)
        })
    }

    /// Return the cell's zero-based slot in the first sixteen tiles of either
    /// active high-bridge set. Concrete wins if malformed theater data makes
    /// the two ranges overlap, matching the engine's predicate order.
    pub(crate) fn high_bridge_tile_offset(&self, cell: &ResolvedTerrainCell) -> Option<usize> {
        let tile_id = u16::try_from(cell.final_tile_index).ok()?;
        [self.bridge_set_start, self.wood_bridge_set_start]
            .into_iter()
            .flatten()
            .find_map(|start| {
                let offset = u32::from(tile_id).checked_sub(u32::from(start))?;
                (offset < 16).then_some(offset as usize)
            })
    }

    #[cfg(test)]
    pub(crate) fn test_set_high_bridge_set_starts(
        &mut self,
        bridge_set_start: Option<u16>,
        wood_bridge_set_start: Option<u16>,
    ) {
        self.bridge_set_start = bridge_set_start;
        self.wood_bridge_set_start = wood_bridge_set_start;
    }
}

impl crate::map::terrain::TacticalBridgeLookup for ResolvedTerrainGrid {
    fn bridge_cell(&self, rx: u16, ry: u16) -> Option<crate::map::terrain::TacticalBridgeCell> {
        let facts = &self.cell(rx, ry)?.bridge_facts;
        // Original6D6760/6D6793 reads raw100/800, independent of inferred
        // topology or deck-walkability. Collapse and restore already update
        // this authoritative cell word; retaining a second map makes it stale.
        // Executed reader: tools/bridge_click_state_oracle/README.md.
        Some(crate::map::terrain::TacticalBridgeCell {
            structural: facts.has_structural_bridge(),
            direction_zero: facts.has_flag(crate::map::bridge_facts::BRIDGE_FLAG_DIRECTION_ZERO),
        })
    }
}

#[cfg(test)]
impl ResolvedTerrainCell {
    /// A clear, level, walkable cell for focused terrain fixtures. Mirrors the
    /// template the terrain-object tests build by hand; `zone_type` and the
    /// overlay fields are the knobs a test turns.
    pub(crate) fn clear_for_test(rx: u16, ry: u16) -> Self {
        let speed_costs = SpeedCostProfile {
            wheel: Some(100),
            ..Default::default()
        };
        Self {
            tileset_index: None,
            speed_costs,
            base_speed_costs: speed_costs,
            ..test_clear_cell(rx, ry)
        }
    }
}

#[cfg(test)]
mod mutation_epoch_tests {
    use super::*;

    /// The production terrain-object writer must move the epoch the movement
    /// blocker-plane cache keys on. `set_terrain_object_occupation` is compiled
    /// identically in game and test builds, so this covers the shipped path.
    #[test]
    fn terrain_object_occupation_writer_moves_the_mutation_epoch() {
        let mut grid =
            ResolvedTerrainGrid::from_cells(1, 1, vec![ResolvedTerrainCell::clear_for_test(0, 0)]);
        let before = grid.mutation_epoch();
        grid.set_terrain_object_occupation((0, 0), Some(1));
        assert!(
            grid.mutation_epoch() > before,
            "occupation write must bump the epoch"
        );
        assert!(grid.cell(0, 0).unwrap().terrain_object_blocks);
        let marked = grid.mutation_epoch();
        grid.set_terrain_object_occupation((0, 0), None);
        assert!(
            grid.mutation_epoch() > marked,
            "occupation clear must bump the epoch"
        );
        // A miss leaves the epoch alone.
        let stable = grid.mutation_epoch();
        grid.set_terrain_object_occupation((5, 5), Some(1));
        assert_eq!(grid.mutation_epoch(), stable);
    }
}
