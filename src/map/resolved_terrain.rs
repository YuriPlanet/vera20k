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
use crate::map::theater::{self, TheaterData, TileKey, TilesetLookup};
use crate::map::tile_variant_selector::TileVariantSelectionContext;
use crate::map::tube_facts::{NativeTubeCellIndex, TubeFact, TubeId};
use crate::map::tubes::NativeMapTubeReceipt;
use crate::rules::overlay_types::{
    OverlayTypeFlags, OverlayTypeRegistry, clears_tiberium_on_slope, retained_overlay_land,
    uses_early_recalc_land_branch,
};
use crate::rules::ruleset::RuleSet;
use crate::rules::terrain_object_type::TerrainObjectType;
use crate::rules::terrain_rules::{LandType, LandTypeSemantics, SpeedCostProfile, TerrainRules};
use crate::util::pixel_conversion::PixelConversionBounds;
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::sync::{
    Arc,
    atomic::{AtomicI16, AtomicI32, AtomicU8, AtomicU32, AtomicU64, Ordering},
};

mod allocated_order;
mod bridge_flags;
mod build;
mod cell_recalc;
mod cliffs;
mod mutation;
mod native_cell;
mod pavement;
mod recalc_catalog;
mod saved_terrain;
mod tubes;

#[cfg(test)]
mod damaged_radar_tests;
#[cfg(test)]
mod pavement_catalog_tests;
#[cfg(any(test, feature = "test-support"))]
mod tests;

pub(crate) use allocated_order::{AllocatedCellOrder, ROW_NEIGHBORS};
pub(crate) use bridge_flags::RealCellBridgeFlags0x1180;
pub use build::OverlayLoadSource;
pub use build::{AuthoredTerrainFill, RadarColorMetadata, current_tile_permissions};
use build::{presentation_tile_parts, wheel_speed_at_or_below_one_percent};
pub(crate) use cell_recalc::{
    AutomaticTubeAllocation, AutomaticTubeRequest, LoadCellRecalcEffects, LoadCellRecalcError,
    LoadCellRecalcOutcome, LoadCellRecalcState,
};
use cliffs::DestroyableCliffCatalog;
pub(crate) use native_cell::{
    CellClassProjectionView, NativeAllocatedCellCursor, NativeLocalCellCursor,
};
pub use native_cell::{NativeCellQuery, SharedCellDummy, SharedCellDummySnapshot};
use recalc_catalog::BridgeRecalcCatalog;
pub(crate) use recalc_catalog::BridgeRecalcCatalogError;
pub use saved_terrain::{SavedCellLandValues, SavedTerrainError, SavedTerrainTemplate};
#[cfg(any(test, feature = "test-support"))]
pub use tests::{
    bridge_constructor_terrain, install_bridge_batch_test_catalog,
    install_bridge_query_test_catalog, install_no_registered_tiles_test_catalog,
    install_ordinary_repair_test_catalog, install_tunnel_repair_test_catalog,
};
pub(crate) use tubes::NativeTubeRegistryBindError;

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
/// CellClass::RecalcZoneType483C80, original instructions483DFB/483E12:
/// compare the full theater-selected type dword with7, not its low-bit mask.
pub(crate) fn recalc_zone_type(
    outside: bool,
    overlay_zone_type: Option<u8>,
    land_type: u8,
    wheel_speed: Option<u8>,
    terrain_object_occupation: Option<i32>,
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

impl BridgeDirection {
    /// The direction of a bridge overlay class: BRIDGE1/BRIDGEB1 (24/237) run
    /// EW, BRIDGE2/BRIDGEB2 (25/238) NS, and every other bridge overlay is low.
    pub fn from_overlay_id(overlay_id: u8) -> Self {
        match overlay_id {
            24 | 237 => Self::EastWest,
            25 | 238 => Self::NorthSouth,
            _ => Self::Low,
        }
    }
}

/// The deck a bridge overlay identity projects onto its cell. OverlayGrid owns
/// the identity itself.
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct BridgeLayer {
    /// Bridge deck height level (ground level + offset).
    pub deck_level: u8,
    /// Bridge direction — determines height offset and rendering.
    pub direction: BridgeDirection,
}

impl BridgeLayer {
    /// A high deck sits four levels above `level` (HighBridgeHeight=4); a low
    /// deck sits at ground level.
    pub fn for_overlay(overlay_id: u8, level: u8) -> Self {
        let direction = BridgeDirection::from_overlay_id(overlay_id);
        let deck_level = match direction {
            BridgeDirection::Low => level,
            BridgeDirection::EastWest | BridgeDirection::NorthSouth => level.saturating_add(4),
        };
        Self {
            deck_level,
            direction,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
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
    /// `CellClass+0xEC` LandType after TMP and overlay derivation: the one
    /// land attribute gamemd stores per cell (constructor `0x0047BC93`,
    /// RecalcAttributes `0x0047D2B0`). Water, cliff and the rest are asked of
    /// it, never cached beside it. Reduced `zone_type`, not this field, selects
    /// the movement matrix column. Only this module's load, Recalc,
    /// CliffBack, overlay and snapshot writers change it: the grid hands out
    /// no mutable cell outside this module.
    pub land_type: u8,
    pub slope_type: u8,
    /// Registered pristine TMP +0x28 height field (CellClass-owned).
    pub template_height: u8,
    /// Signed low-byte result of the pristine TMP effective-height formula.
    pub height_in_pixels: i8,
    /// Tactical render origin owned by the selected independent TMP.
    pub render_offset_x: i32,
    pub render_offset_y: i32,
    pub speed_costs: SpeedCostProfile,
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
    /// Full current-theater occupation dword for a present terrain object.
    /// Low bits1/2/4 decide blockage; only raw7 selects the WALL zone.
    pub terrain_object_occupation: Option<i32>,
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
    /// Underlying `land_type` before any overlay override (tiberium / road):
    /// the pristine TMP subtile land (`GetSubtileLandType 0x00544BE0`) or the
    /// CliffBack Rock reclass. Written with the pristine metadata by this
    /// module's load, Recalc, CliffBack, tile-replacement and snapshot writers;
    /// `recalc_overlay_passability` restores `land_type` from it when an
    /// overlay goes (an ore overlay fully harvested), where RecalcAttributes
    /// would re-read the TMP. Same pattern as `base_ground_walk_blocked`.
    pub base_land_type: u8,
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

    pub fn bridge_deck_level_if_any(&self) -> Option<u8> {
        self.has_bridge_deck.then_some(self.bridge_deck_level)
    }

    /// The deck level of a high bridge; low bridges (LOBRDG/LOBRDB) sit at
    /// ground level and answer `None`.
    pub fn high_bridge_deck_level(&self) -> Option<u8> {
        self.bridge_deck_level_if_any().filter(|_| {
            !self
                .bridge_layer
                .as_ref()
                .is_some_and(|layer| layer.direction == BridgeDirection::Low)
        })
    }

    pub fn bridge_flags(&self) -> u32 {
        self.bridge_facts.raw_flags
    }

    pub fn is_low_bridge_tube_cell(&self) -> bool {
        self.tube_index.is_some() && self.land_type == LandType::Tunnel.as_index()
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
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
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

/// Scheduler asset roots required by this map's surviving runtime objects.
///
/// Damage-fire roots remain part of the established closure. Terrain animation
/// names come exclusively from resolved theater `Tile##Anim` data, so custom
/// names take the same path without a WA/TUNTOP name table in Rust.
pub fn scheduler_anim_roots(
    rules: &RuleSet,
    overlay_registry: &OverlayTypeRegistry,
    tile_animations: &[TerrainTileAnimation],
) -> Vec<String> {
    let mut roots = BTreeSet::new();
    roots.extend(
        rules
            .general
            .damage_fire_types
            .iter()
            .map(|anim| anim.name.trim().to_ascii_uppercase())
            .filter(|name| !name.is_empty()),
    );
    roots.extend(
        tile_animations
            .iter()
            .map(|anim| anim.anim_name.trim().to_ascii_uppercase())
            .filter(|name| !name.is_empty()),
    );
    // [General] Wake= is constructed as an ordinary AnimClass by the drive
    // locomotor's process (0x004B0823 region), so it needs the same loader
    // bounds as every other scheduler-owned anim or its constructor refuses it.
    {
        let wake = rules.general.wake.name.trim().to_ascii_uppercase();
        if !wake.is_empty() {
            roots.insert(wake);
        }
    }
    roots.extend(
        [
            rules.crate_rules.wood_crate_img.as_deref(),
            rules.crate_rules.crate_img.as_deref(),
            rules.crate_rules.water_crate_img.as_deref(),
        ]
        .into_iter()
        .flatten()
        .filter_map(|name| overlay_registry.id_for_name(name))
        .filter_map(|overlay_id| overlay_registry.flags(overlay_id))
        .filter_map(|flags| flags.cell_anim.as_deref())
        .map(|name| name.trim().to_ascii_uppercase())
        .filter(|name| !name.is_empty()),
    );
    //AnimAI42413B can create ore on a map that started without any. Its
    //Overlay Mark's CellAnim must already use the ordinary bound Anim owner.
    roots.extend(
        (0..overlay_registry.len())
            .filter_map(|index| u8::try_from(index).ok())
            .filter_map(|id| overlay_registry.flags(id))
            .filter(|flags| flags.tiberium)
            .filter_map(|flags| flags.cell_anim.as_deref())
            .map(str::to_ascii_uppercase),
    );
    roots.into_iter().collect()
}

/// Leptons per cell along one map axis.
const LEPTONS_PER_CELL: i32 = crate::util::lepton::LEPTONS_PER_CELL_I32;
/// Cell-centre offset used by the tile-animation spawn coordinate.
const CELL_CENTRE_LEPTONS: i32 = LEPTONS_PER_CELL / 2;

/// Serialized behavior authority for a CellClass whose isometric tile was
/// replaced after map construction. The live ResolvedTerrainGrid is derived
/// and skipped by Simulation snapshots, so collapse projects these exact
/// fields into Simulation and reapplies them after map reconstruction.
///
/// [`DynamicTerrainCells`] keeps one per replaced cell: only
/// [`Self::capture`] (or a saved game) makes one, and only
/// [`ResolvedTerrainGrid::apply_dynamic_cell_state`] writes it back.
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct DynamicTerrainCellState {
    pub final_tile_index: i32,
    pub final_sub_tile: u8,
    pub is_wood_bridge_repair_tile: bool,
    pub level: u8,
    pub filled_clear: bool,
    pub tileset_index: Option<u16>,
    pub land_type: u8,
    pub slope_type: u8,
    pub template_height: u8,
    pub height_in_pixels: i8,
    pub render_offset_x: i32,
    pub render_offset_y: i32,
    pub speed_costs: SpeedCostProfile,
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
    pub fn capture(cell: &ResolvedTerrainCell) -> Self {
        Self {
            final_tile_index: cell.final_tile_index,
            final_sub_tile: cell.final_sub_tile,
            is_wood_bridge_repair_tile: cell.is_wood_bridge_repair_tile,
            level: cell.level,
            filled_clear: cell.filled_clear,
            tileset_index: cell.tileset_index,
            land_type: cell.land_type,
            slope_type: cell.slope_type,
            template_height: cell.template_height,
            height_in_pixels: cell.height_in_pixels,
            render_offset_x: cell.render_offset_x,
            render_offset_y: cell.render_offset_y,
            speed_costs: cell.speed_costs,
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

    fn apply(&self, cell: &mut ResolvedTerrainCell) {
        cell.final_tile_index = self.final_tile_index;
        cell.final_sub_tile = self.final_sub_tile;
        cell.is_wood_bridge_repair_tile = self.is_wood_bridge_repair_tile;
        cell.level = self.level;
        cell.filled_clear = self.filled_clear;
        cell.tileset_index = self.tileset_index;
        cell.land_type = self.land_type;
        cell.slope_type = self.slope_type;
        cell.template_height = self.template_height;
        cell.height_in_pixels = self.height_in_pixels;
        cell.render_offset_x = self.render_offset_x;
        cell.render_offset_y = self.render_offset_y;
        cell.speed_costs = self.speed_costs;
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

/// Every CellClass whose isometric tile was replaced after map construction,
/// as its [`DynamicTerrainCellState`], keyed by cell. Tile replacement records
/// a cell when it changes it; snapshot restore reapplies every record onto the
/// rebuilt grid before overlay, bridge and navigation reconstruction.
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(transparent)]
pub struct DynamicTerrainCells(BTreeMap<(u16, u16), DynamicTerrainCellState>);

impl DynamicTerrainCells {
    /// Record `cell`'s current replacement state, superseding an earlier one.
    pub fn record(&mut self, cell: &ResolvedTerrainCell) {
        self.0
            .insert((cell.rx, cell.ry), DynamicTerrainCellState::capture(cell));
    }

    /// Forget every record: a newly constructed map has replaced no tile.
    pub(crate) fn clear(&mut self) {
        self.0.clear();
    }

    /// Reapply every record onto `grid`; the error names the first recorded
    /// cell the grid does not allocate.
    pub(crate) fn apply_to(&self, grid: &mut ResolvedTerrainGrid) -> Result<(), (u16, u16)> {
        for (&(rx, ry), state) in &self.0 {
            if !grid.apply_dynamic_cell_state(rx, ry, state) {
                return Err((rx, ry));
            }
        }
        Ok(())
    }

    pub fn get(&self, cell: &(u16, u16)) -> Option<&DynamicTerrainCellState> {
        self.0.get(cell)
    }

    pub fn iter(&self) -> impl Iterator<Item = (&(u16, u16), &DynamicTerrainCellState)> {
        self.0.iter()
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
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
    /// Immutable projection of the scenario's `[OverlayTypes]` `Wall=` and
    /// `Crushable=`, in registry order. CheckCellPassability4834A0 reads the
    /// type through the live `CellClass+0x44` identity (`OverlayGrid` for a
    /// real cell, the shared Dummy's own). Bound beside `land_speed_costs`
    /// from the same scenario's registry; empty when no registry was bound.
    overlay_passability: Vec<OverlayPassability>,
    /// Pristine TMP dimensions used by Techno draw depth (0x547150/0x704350).
    /// Derived asset data, keyed by tile identity rather than mutable cell:
    /// never serialized or included in simulation hashes.
    native_tmp_draw_heights: HashMap<u16, PristineTmpHeader>,
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
    mutation_epoch: u64,
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
    /// Derived from the allocation; see [`Self::allocated_cell_order`] and
    /// [`Self::interior_cells`].
    allocation_caches: allocated_order::AllocationCaches,
}

/// A `size` x `size` flat level-0 grid whose land rows admit Foot, Track and
/// Wheel at 100%: the inputs the native Foot `Can_Enter_Cell` reads.
#[cfg(any(test, feature = "test-support"))]
pub fn test_flat_ground_grid(size: u16) -> ResolvedTerrainGrid {
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

/// Retail `rulesmd.ini` `[Clear]` speed keys; a resolved-terrain test pins
/// them to the retail file through the production land reader.
#[cfg(any(test, feature = "test-support"))]
pub(crate) const TEST_RETAIL_CLEAR_INI: &str = "[Clear]\nFoot=100%\nTrack=100%\nWheel=100%\nFloat=0%\n\
    Hover=50%\nAmphibious=80%\nFloatBeach=0%\n";

/// The row the production land reader makes of retail `[Clear]`.
#[cfg(any(test, feature = "test-support"))]
pub(crate) const TEST_RETAIL_CLEAR_SPEED_COSTS: SpeedCostProfile = SpeedCostProfile {
    foot: Some(100),
    track: Some(100),
    wheel: Some(100),
    float: Some(0),
    amphibious: Some(80),
    float_beach: Some(0),
    hover: Some(50),
};

/// Retail `[Clear]` read by the production land reader, for binding a test
/// grid's land rows as a scenario load binds its rules.
#[cfg(any(test, feature = "test-support"))]
pub fn test_retail_clear_rules() -> TerrainRules {
    TerrainRules::from_ini(&crate::rules::ini_parser::IniFile::from_str(
        TEST_RETAIL_CLEAR_INI,
    ))
}

/// [`test_clear_cell`] carrying the retail Clear row.
#[cfg(any(test, feature = "test-support"))]
pub(crate) fn test_retail_clear_cell(rx: u16, ry: u16) -> ResolvedTerrainCell {
    ResolvedTerrainCell {
        speed_costs: TEST_RETAIL_CLEAR_SPEED_COSTS,
        base_speed_costs: TEST_RETAIL_CLEAR_SPEED_COSTS,
        ..test_clear_cell(rx, ry)
    }
}

/// A grid of [`test_retail_clear_cell`] with [`test_retail_clear_rules`]
/// bound as its land rows: the Clear row reaches the shared Dummy and a wall
/// a mover passes, as on a loaded map.
#[cfg(any(test, feature = "test-support"))]
pub(crate) fn test_retail_clear_grid(width: u16, height: u16) -> ResolvedTerrainGrid {
    let mut grid = test_grid(width, height, test_retail_clear_cell);
    grid.set_land_speed_rules_for_test(&test_retail_clear_rules());
    grid
}

/// A `width` x `height` grid of `cell(rx, ry)`, in row-major order.
#[cfg(any(test, feature = "test-support"))]
pub fn test_grid(
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
#[cfg(any(test, feature = "test-support"))]
pub const TEST_OPEN_SPEED_COSTS: SpeedCostProfile = SpeedCostProfile {
    foot: Some(100),
    track: Some(100),
    wheel: Some(100),
    float: Some(100),
    amphibious: Some(100),
    float_beach: Some(100),
    hover: Some(100),
};

/// [`test_flat_cell`], named for the Clear land it carries.
#[cfg(any(test, feature = "test-support"))]
pub(crate) fn test_clear_cell(rx: u16, ry: u16) -> ResolvedTerrainCell {
    test_flat_cell(rx, ry)
}

/// [`test_clear_cell`] that accepts smudges.
#[cfg(any(test, feature = "test-support"))]
pub fn test_smudge_cell(rx: u16, ry: u16) -> ResolvedTerrainCell {
    ResolvedTerrainCell {
        accepts_smudge: true,
        ..test_clear_cell(rx, ry)
    }
}

/// [`test_smudge_cell`] that also allows tiberium.
#[cfg(any(test, feature = "test-support"))]
pub(crate) fn test_tiberium_cell(rx: u16, ry: u16) -> ResolvedTerrainCell {
    ResolvedTerrainCell {
        allows_tiberium: true,
        ..test_smudge_cell(rx, ry)
    }
}

/// [`test_flat_cell`] that is `filled_clear` and accepts smudges.
#[cfg(any(test, feature = "test-support"))]
pub(crate) fn test_filled_smudge_cell(rx: u16, ry: u16) -> ResolvedTerrainCell {
    ResolvedTerrainCell {
        filled_clear: true,
        accepts_smudge: true,
        ..test_flat_cell(rx, ry)
    }
}

/// [`test_smudge_cell`] as the map loader fills a clear cell: `filled_clear`
/// and no tileset.
#[cfg(any(test, feature = "test-support"))]
pub fn test_loader_clear_cell(rx: u16, ry: u16) -> ResolvedTerrainCell {
    ResolvedTerrainCell {
        filled_clear: true,
        tileset_index: None,
        ..test_smudge_cell(rx, ry)
    }
}

/// A fully-populated flat `ResolvedTerrainCell`, for tests in other modules that
/// need a grid and should not carry a copy of this whole literal.
#[cfg(any(test, feature = "test-support"))]
pub fn test_flat_cell(rx: u16, ry: u16) -> ResolvedTerrainCell {
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
        slope_type: 0,
        template_height: 0,
        render_offset_x: 0,
        render_offset_y: 0,
        speed_costs: SpeedCostProfile::default(),
        height_in_pixels: 0,
        variant: 0,
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

/// The rules row a cell of `land` takes: the scenario's (the zero row for an
/// unauthored section), or, with no rules table bound at all (test fixtures
/// only; every production builder binds one), the unbound row: no SpeedType
/// rows, not blocked, buildable.
fn land_row(rules: Option<&TerrainRules>, land: LandType) -> LandTypeSemantics {
    rules.map_or(
        LandTypeSemantics {
            buildable: true,
            ground_blocked: false,
            speed_costs: SpeedCostProfile::default(),
        },
        |rules| *rules.row(land),
    )
}

fn land_speed_cost_profiles(
    rules: Option<&TerrainRules>,
) -> [SpeedCostProfile; LandType::ALL.len()] {
    LandType::ALL.map(|land| land_row(rules, land).speed_costs)
}

/// The two `OverlayTypeClass` fields `CellClass::CheckCellPassability @
/// 0x004834A0` reads through `CellClass+0x44`: `Wall=` (`+0x2A8`,
/// `0x00483598`) and `Crushable=` (`+0x22D`, `0x004835BB`), as
/// [`OverlayTypeRegistry::from_ini`] parses them.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct OverlayPassability {
    pub wall: bool,
    pub crushable: bool,
}

fn overlay_passability_flags(registry: Option<&OverlayTypeRegistry>) -> Vec<OverlayPassability> {
    let Some(registry) = registry else {
        return Vec::new();
    };
    // `OverlayCell` stores the `+0x44` identity as a byte.
    (0..=u8::MAX)
        .map_while(|id| registry.flags(id))
        .map(|flags| OverlayPassability {
            wall: flags.wall,
            crushable: flags.crushable,
        })
        .collect()
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
            overlay_passability: Vec::new(),
            native_tmp_draw_heights: HashMap::new(),
            shared_cell_dummy: SharedCellDummy::fresh(),
            mutation_epoch: 0,
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
            allocation_caches: Default::default(),
        }
    }

    /// Original4835D5..4835DE indexes the 89EA40 table using retained Land.
    /// The existing TerrainRules owner supplies these rows (the zero row for
    /// an unauthored section); this grid does not parse or independently
    /// mutate rules. A value outside the twelve land types is the unbound
    /// profile, every cost absent.
    pub(crate) fn land_speed_costs(&self, land: i32) -> SpeedCostProfile {
        usize::try_from(land)
            .ok()
            .and_then(|index| self.land_speed_costs.get(index))
            .copied()
            .unwrap_or_default()
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn set_land_speed_rules_for_test(&mut self, rules: &TerrainRules) {
        self.land_speed_costs = land_speed_cost_profiles(Some(rules));
    }

    /// `Wall=`/`Crushable=` of the overlay type a `CellClass+0x44` identity
    /// names; `None` for `-1` (`0x00483589`) or a type the bound registry
    /// does not hold.
    pub(crate) fn overlay_passability(&self, overlay: i32) -> Option<OverlayPassability> {
        usize::try_from(overlay)
            .ok()
            .and_then(|index| self.overlay_passability.get(index))
            .copied()
    }

    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn set_overlay_types_for_test(&mut self, registry: &OverlayTypeRegistry) {
        self.overlay_passability = overlay_passability_flags(Some(registry));
    }

    /// Terrain animations to spawn once at map load, in native creation order.
    pub fn tile_animations(&self) -> &[TerrainTileAnimation] {
        &self.tile_animations
    }

    pub fn width(&self) -> u16 {
        self.width
    }

    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn projectile_water_set_base(&self) -> i32 {
        self.water_tiles.water_base
    }

    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn set_projectile_water_set_base(&mut self, base: i32) {
        self.water_tiles.water_base = base;
    }

    /// `CellClass @ 0x004865D0` (the database's HasBridgeOverlay), which
    /// reads only the cell's tile (`+0x38`): a WaterSet, ShorePieces or
    /// waterfall tile ([`TileIds::is_water_shore_or_waterfall`], the one
    /// port). The shared dummy's tile is 0xFFFF.
    ///
    /// Absent bases still describe signed windows: Lunar ShorePieces=-1
    /// admits tiles0..40. PostWarpValidation718AA9 uses this decision after
    /// an entry refusal to choose living sinking rather than C4 destruction.
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

    pub fn current_tile_radar_metadata(&self, rx: u16, ry: u16) -> Option<RadarColorMetadata> {
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

    #[cfg(any(test, feature = "test-support"))]
    pub fn test_set_damaged_radar_metadata(
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
    pub fn native_tmp_draw_height(&self, tile: i32, sub_tile: u8) -> i32 {
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
    pub fn native_tmp_file_count(&self, tile: i32) -> Option<usize> {
        let tile = u16::try_from(tile).ok()?;
        self.native_tmp_draw_heights
            .get(&tile)
            .map(|head| head.total_file_count)
    }

    pub fn concrete_bridge_set_base(&self) -> i32 {
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

    #[cfg(any(test, feature = "test-support"))]
    pub fn test_set_high_bridge_rim_tiles(
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
            self.is_allocated(index).then_some(index)
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

    #[cfg(any(test, feature = "test-support"))]
    pub fn test_set_radar_color_valid(&mut self, rx: u16, ry: u16, valid: bool) {
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

    /// Mutable cell access that moves `mutation_epoch`, private to this
    /// module: no other module writes a cell. The terrain-object
    /// occupation writer (`set_terrain_object_occupation`) goes through here,
    /// and that occupation flag is the only terrain field the movement
    /// blocker-plane cache (`MovementPassCache`) reads, so the epoch is a
    /// sound key for that cache; other internal writers index `cells`
    /// directly and do not move it (see the field note), which stays fine
    /// only while the plane reads no other terrain field. The bump has to
    /// run in game builds as well as tests: a tree destroyed mid-match must
    /// invalidate the plane in the shipped binary. An earlier revision bumped
    /// only a `cfg(test)` body, which the suite could not detect.
    fn cell_mut(&mut self, rx: u16, ry: u16) -> Option<&mut ResolvedTerrainCell> {
        let idx = self.index(rx, ry)?;
        self.mutation_epoch = self.mutation_epoch.wrapping_add(1);
        self.cells.get_mut(idx)
    }

    /// [`Self::cell_mut`] for fixtures outside this module.
    #[cfg(any(test, feature = "test-support"))]
    pub fn cell_mut_for_test(&mut self, rx: u16, ry: u16) -> Option<&mut ResolvedTerrainCell> {
        self.cell_mut(rx, ry)
    }

    /// The cell storage, for fixtures that reshape a grid. Does not move
    /// `mutation_epoch`.
    #[cfg(any(test, feature = "test-support"))]
    pub fn cells_mut_for_test(&mut self) -> &mut Vec<ResolvedTerrainCell> {
        &mut self.cells
    }

    /// Epoch of mutable cell access; see the field note.
    pub fn mutation_epoch(&self) -> u64 {
        self.mutation_epoch
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
                if let Err(error) = self.refresh_saved_cell_presentation(index) {
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
        self.cells
            .iter()
            .enumerate()
            .filter_map(|(index, cell)| self.is_allocated(index).then_some(cell))
    }

    /// Whether the cell at storage `index` is allocated, one [`Self::iter`]
    /// yields.
    pub(crate) fn is_allocated(&self, index: usize) -> bool {
        self.native_allocated
            .as_ref()
            .is_none_or(|mask| mask.get(index).copied().unwrap_or(false))
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

    #[cfg(any(test, feature = "test-support"))]
    pub fn test_set_high_bridge_set_starts(
        &mut self,
        bridge_set_start: Option<u16>,
        wood_bridge_set_start: Option<u16>,
    ) {
        self.bridge_set_start = bridge_set_start;
        self.wood_bridge_set_start = wood_bridge_set_start;
    }
}

impl crate::map::terrain::TacticalCellLookup for ResolvedTerrainGrid {
    fn cell_level(&self, rx: u16, ry: u16) -> Option<u8> {
        // Original6D6753 reads the current candidate, not a load-time level.
        self.cell(rx, ry).map(|cell| cell.level)
    }

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

#[cfg(any(test, feature = "test-support"))]
impl ResolvedTerrainCell {
    /// A clear, level, walkable cell for focused terrain fixtures. Mirrors the
    /// template the terrain-object tests build by hand; `zone_type` and the
    /// overlay fields are the knobs a test turns.
    pub fn clear_for_test(rx: u16, ry: u16) -> Self {
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
