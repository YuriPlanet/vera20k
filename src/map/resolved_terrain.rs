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
    BRIDGE_FLAG_TRANSITION, BridgeAnchorRelation, BridgeCellFacts, BridgeFlagStamp,
    BridgeStampFamily, BridgeStampSlot, MODELED_CELLCLASS_BRIDGE_FLAG_MASK,
    RETAINED_CELLCLASS_BRIDGE_FLAG_MASK,
};
use crate::map::cell_index::NativeCellIdentity;
use crate::map::lat;
use crate::map::map_file::{MapCell, MapFile};
use crate::map::overlay::OverlayEntry;
use crate::map::overlay_types::{
    OverlayTypeFlags, OverlayTypeRegistry, clears_tiberium_on_slope, retained_overlay_land,
    uses_early_recalc_land_branch,
};
use crate::map::playfield::PlayfieldBounds;
use crate::map::rmg::preview::Playfield;
use crate::map::theater::{self, TheaterData, TileKey, TilesetLookup};
use crate::map::tile_variant_selector::TileVariantSelectionContext;
use crate::map::tube_facts::{NativeTubeCellIndex, TubeFact, TubeId};
use crate::map::tubes::NativeMapTubeReceipt;
use crate::rules::terrain_object_type::TerrainObjectType;
use crate::rules::terrain_rules::{LandType, SpeedCostProfile, TerrainClass, TerrainRules};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::{
    Arc,
    atomic::{AtomicI16, AtomicU32, AtomicU64, Ordering},
};
#[cfg(test)]
pub(crate) use tests::{
    bridge_constructor_terrain, install_bridge_batch_test_catalog,
    install_ordinary_repair_test_catalog,
};

#[path = "resolved_terrain_mutation.rs"]
mod mutation;

#[path = "terrain_recalc_catalog.rs"]
mod recalc_catalog;
use recalc_catalog::BridgeRecalcCatalog;
pub(crate) use recalc_catalog::BridgeRecalcCatalogError;

#[cfg(test)]
#[path = "terrain_pavement_catalog_tests.rs"]
mod pavement_catalog_tests;

#[path = "terrain_pavement.rs"]
mod pavement;

pub const YR_CELL_LAND_TUNNEL: u8 = 10;

/// Identifies whether map overlays still require the ordinary authored-map
/// `OverlayClass::Mark` pass. RMG writes its complete overlay/data rectangles
/// directly into live cells; serializing those cells through `MapFile` must not
/// turn loading into a second Mark operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum OverlayLoadSource {
    Authored,
    GeneratedMaterialized,
}

/// Controls whether Fill stops at pristine CellClass materialization or also
/// performs the legacy eager projection retained for synthetic/generated
/// callers. Active authored loads execute LAT, OverlayClass::Mark, Recalc,
/// Terrain occupation, tile Anim construction, CliffBack, and Tube creation at
/// their native later seams, so none of those effects belong in Fill itself.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TerrainBuildProjection {
    EagerCompatibility,
    PendingAuthored,
}

impl TerrainBuildProjection {
    const fn is_eager(self) -> bool {
        matches!(self, Self::EagerCompatibility)
    }
}

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

/// Route-scoped bridge oracle dump for a resolved terrain cell.
#[derive(Debug, Clone, serde::Serialize, PartialEq, Eq)]
pub struct BridgeOracleCellFacts {
    pub rx: u16,
    pub ry: u16,
    pub source_tile_index: i32,
    pub source_sub_tile: u8,
    pub final_tile_index: i32,
    pub final_sub_tile: u8,
    pub level: u8,
    pub slope_type: u8,
    pub land_type: u8,
    pub yr_cell_land_type: u8,
    pub bridge_set_member: Option<bool>,
    pub wood_bridge_set_member: Option<bool>,
    pub bridge_raw_flags: u32,
    pub flag_0x80_anchor_self: bool,
    pub flag_0x100_structural: bool,
    pub flag_0x200_transition: bool,
    pub flag_0x400_destroyed_or_ramp: bool,
    pub state_byte: u8,
    pub overlay_id: Option<u8>,
    pub family: String,
    pub direction: Option<u8>,
    pub anchor: Option<BridgeOracleAnchor>,
    pub bridge_deck_level: u8,
    pub has_bridge_deck: bool,
    pub bridge_walkable: bool,
    pub bridge_transition: bool,
}

/// Anchor relation fields flattened for stable JSON diagnostics.
#[derive(Debug, Clone, serde::Serialize, PartialEq, Eq)]
pub struct BridgeOracleAnchor {
    pub rx: u16,
    pub ry: u16,
    pub slot: String,
    pub family: String,
    pub direction: u8,
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
    pub build_blocked: bool,
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
    /// Author-damaged anchor pre-classification: `Some(class)` if this
    /// cell's `final_tile_index` matches one of the 8 bridgehead anchor
    /// variant tile_ids in the current theater's BridgeAnchorVariantTable.
    /// `None` when not a variant tile (the common case for both
    /// non-bridge cells and pristine anchor cells).
    ///
    /// Sim's `BridgeRuntimeState::from_resolved_terrain` reads this to
    /// initialize `BridgeRuntimeCell.bridgehead_anchor_class` instead of
    /// the unconditional Variant0 default. None defaults to Variant0
    /// sim-side.
    pub bridgehead_anchor_class_at_load: Option<crate::map::bridge_facts::BridgeheadAnchorClass>,
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

impl BridgeOracleCellFacts {
    pub fn from_cell(cell: &ResolvedTerrainCell, theater_data: Option<&TheaterData>) -> Self {
        let final_tile_id = normalize_tile_id(cell.final_tile_index);
        let facts = cell.bridge_facts;
        Self {
            rx: cell.rx,
            ry: cell.ry,
            source_tile_index: cell.source_tile_index,
            source_sub_tile: cell.source_sub_tile,
            final_tile_index: cell.final_tile_index,
            final_sub_tile: cell.final_sub_tile,
            level: cell.level,
            slope_type: cell.slope_type,
            land_type: cell.land_type,
            yr_cell_land_type: cell.yr_cell_land_type,
            bridge_set_member: theater_data
                .map(|td| tile_in_first_16_of_set(td, td.bridge_set, final_tile_id)),
            wood_bridge_set_member: theater_data
                .map(|td| tile_in_first_16_of_set(td, td.wood_bridge_set, final_tile_id)),
            bridge_raw_flags: facts.raw_flags,
            flag_0x80_anchor_self: facts.has_flag(BRIDGE_FLAG_ANCHOR_SELF),
            flag_0x100_structural: facts.has_flag(BRIDGE_FLAG_STRUCTURAL),
            flag_0x200_transition: facts.has_flag(BRIDGE_FLAG_TRANSITION),
            flag_0x400_destroyed_or_ramp: facts.has_flag(BRIDGE_FLAG_DESTROYED_OR_RAMP),
            state_byte: facts.state_byte,
            overlay_id: facts.overlay_id,
            family: format!("{:?}", facts.family),
            direction: facts.direction,
            anchor: facts.anchor.map(|anchor| BridgeOracleAnchor {
                rx: anchor.anchor.0,
                ry: anchor.anchor.1,
                slot: format!("{:?}", anchor.slot),
                family: format!("{:?}", anchor.family),
                direction: anchor.direction,
            }),
            bridge_deck_level: cell.bridge_deck_level,
            has_bridge_deck: cell.has_bridge_deck,
            bridge_walkable: cell.bridge_walkable,
            bridge_transition: cell.bridge_transition,
        }
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

/// Non-Clone input adapter for `CellClass::RecalcAttributes`. Authored loading
/// owns its lazy asset cache and animation latch; the admitted runtime bridge
/// path borrows immutable resident inputs and supplies current playfield bounds.
///
/// The metadata/cache inputs are retained across calls so every Mark write and
/// both whole-cell sweeps observe the mutations made by earlier calls. The
/// animation latch is the load-local analogue of `CellClass+0x140 & 0x20000`;
/// it is deliberately not stored in the cloneable final terrain projection.
pub(crate) struct LoadCellRecalcState<'a> {
    theater_data: Option<&'a TheaterData>,
    asset_manager: Option<&'a crate::assets::asset_manager::AssetManager>,
    resident_bridge: Option<&'a BridgeRecalcCatalog>,
    terrain_rules: Option<&'a TerrainRules>,
    overlay_types: &'a OverlayTypeRegistry,
    playfield: Option<Playfield>,
    lat_config: Option<lat::LatConfig>,
    slope_config: Option<lat::SlopeFixupConfig>,
    cliff_back_impassability: u8,
    metadata_cache: HashMap<TileKey, TileMetadata>,
    warned_unknown_land_types: HashSet<u8>,
    terrain_anim_latched: Vec<bool>,
}

impl<'a> LoadCellRecalcState<'a> {
    pub(crate) fn for_authored_load(
        map: &MapFile,
        theater_data: &'a TheaterData,
        asset_manager: &'a crate::assets::asset_manager::AssetManager,
        terrain_rules: &'a TerrainRules,
        overlay_types: &'a OverlayTypeRegistry,
        lat_enabled: bool,
        cliff_back_impassability: u8,
        cell_count: usize,
    ) -> Self {
        let lat_config = lat_enabled
            .then(|| lat::parse_lat_config(&theater_data.ini_data, &theater_data.lookup));
        let slope_config = lat_enabled.then_some(lat::SlopeFixupConfig {
            ramp_base: theater_data.rmg_tiles.ramp_base.map_or(-1, i32::from),
            ramp_smooth: theater_data.rmg_tiles.ramp_smooth.map_or(-1, i32::from),
        });
        Self {
            theater_data: Some(theater_data),
            asset_manager: Some(asset_manager),
            resident_bridge: None,
            terrain_rules: Some(terrain_rules),
            overlay_types,
            playfield: Some(Playfield::from_header(&map.header)),
            lat_config,
            slope_config,
            cliff_back_impassability,
            metadata_cache: HashMap::new(),
            warned_unknown_land_types: HashSet::new(),
            terrain_anim_latched: vec![false; cell_count],
        }
    }

    /// Explicitly synthetic fallback for focused map-owner tests. Production
    /// authored loading must use `for_authored_load` so missing theater/TMP
    /// authority cannot silently become heuristic metadata.
    #[cfg(test)]
    pub(crate) fn synthetic(overlay_types: &'a OverlayTypeRegistry, cell_count: usize) -> Self {
        Self {
            theater_data: None,
            asset_manager: None,
            resident_bridge: None,
            terrain_rules: None,
            overlay_types,
            playfield: None,
            lat_config: None,
            slope_config: None,
            cliff_back_impassability: 0,
            metadata_cache: HashMap::new(),
            warned_unknown_land_types: HashSet::new(),
            terrain_anim_latched: vec![false; cell_count],
        }
    }

    /// Clear exactly the receiver latch immediately before that cell's final
    /// load Recalc. Native `InitCellAttributes(0)` interleaves this write with
    /// the anti-diagonal sweep; clearing the whole vector ahead of the sweep
    /// would expose future cells too early.
    pub(crate) fn clear_terrain_anim_latch(&mut self, index: usize) -> bool {
        let latched = self
            .terrain_anim_latched
            .get_mut(index)
            .expect("load Recalc latch shape must match the real terrain grid");
        let was_latched = *latched;
        *latched = false;
        was_latched
    }

    fn terrain_anim_is_latched(&self, index: usize) -> bool {
        if self.resident_bridge.is_some() {
            // Resident admission rejects every declared source attachment.
            // No per-call false latch is used to authorize an Anim constructor.
            return false;
        }
        *self
            .terrain_anim_latched
            .get(index)
            .expect("load Recalc latch shape must match the real terrain grid")
    }

    fn latch_terrain_anim(&mut self, index: usize) {
        *self
            .terrain_anim_latched
            .get_mut(index)
            .expect("load Recalc latch shape must match the real terrain grid") = true;
    }

    fn registered_tile_metadata<E>(
        &mut self,
        tile_index: i32,
        sub_tile: u8,
        synthetic_cell: &ResolvedTerrainCell,
    ) -> Result<Option<TileMetadata>, LoadCellRecalcError<E>> {
        if let Some(catalog) = self.resident_bridge {
            return catalog
                .metadata(tile_index, sub_tile)
                .map_err(LoadCellRecalcError::ResidentInput);
        }
        if tile_index < 0 || tile_index == 0xFFFF {
            return Ok(None);
        }
        if let Some(theater) = self.theater_data {
            if tile_index as usize >= theater.lookup.len() {
                return Ok(None);
            }
            let tile_id = tile_index as u16;
            return Ok(Some(cached_tile_metadata(
                &mut self.metadata_cache,
                self.theater_data,
                self.asset_manager,
                self.terrain_rules,
                TileKey {
                    tile_id,
                    sub_tile,
                    variant: 0,
                },
                &mut self.warned_unknown_land_types,
            )));
        }

        #[cfg(test)]
        {
            Ok(Some(TileMetadata::from_resolved_cell(synthetic_cell)))
        }
        #[cfg(not(test))]
        {
            let _ = synthetic_cell;
            unreachable!("production load Recalc requires theater/TMP authority")
        }
    }

    fn for_resident_bridge(
        catalog: &'a BridgeRecalcCatalog,
        overlay_types: &'a OverlayTypeRegistry,
        bounds: Option<PlayfieldBounds>,
    ) -> Self {
        Self {
            theater_data: None,
            asset_manager: None,
            resident_bridge: Some(catalog),
            terrain_rules: Some(&catalog.terrain_rules),
            overlay_types,
            playfield: bounds.map(|bounds| {
                Playfield::from_local_size(
                    bounds.base,
                    bounds.off_fc,
                    bounds.off_100,
                    bounds.off_104,
                    bounds.off_108,
                )
            }),
            lat_config: catalog.lat_config.clone(),
            slope_config: catalog.slope_config,
            cliff_back_impassability: catalog.cliff_back_impassability,
            metadata_cache: HashMap::new(),
            warned_unknown_land_types: HashSet::new(),
            terrain_anim_latched: Vec::new(),
        }
    }

    fn current_permissions(&self, tile: i32) -> Option<(bool, bool)> {
        self.resident_bridge
            .map(|catalog| catalog.current_permissions(tile))
            .or_else(|| {
                self.theater_data
                    .map(|theater| current_tile_permissions(&theater.lookup, tile))
            })
    }

    fn automatic_tube_direction(&self, tile: i32) -> Option<u8> {
        if let Some(catalog) = self.resident_bridge {
            catalog.automatic_tube_direction(tile)
        } else {
            auto_tube_direction_for_tile(tile, self.theater_data)
        }
    }

    fn wood_bridge_repair_tile(&self, tile: i32) -> bool {
        self.resident_bridge.map_or_else(
            || is_wood_bridge_repair_tile(self.theater_data, tile),
            |catalog| catalog.is_wood_bridge_repair_tile(tile),
        )
    }

    fn clear_land_metadata(&self) -> TileMetadata {
        let mut metadata = TileMetadata::default();
        if let Some(semantics) = self
            .terrain_rules
            .and_then(|rules| rules.semantics_for_land_type(LandType::Clear.as_index()))
        {
            metadata.land_type = LandType::Clear.as_index();
            metadata.yr_cell_land_type = LandType::Clear.as_index();
            metadata.terrain_class = semantics.terrain_class;
            metadata.speed_costs = semantics.speed_costs;
            metadata.is_water = semantics.water;
            metadata.is_cliff_like = semantics.cliff_like;
            metadata.is_rough = semantics.rough;
            metadata.is_road = semantics.road;
            metadata.ground_blocked = semantics.ground_blocked;
            metadata.build_blocked = !semantics.buildable;
        }
        metadata
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct AutomaticTubeRequest {
    pub(crate) cell: (u16, u16),
    pub(crate) direction: u8,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AutomaticTubeAllocation {
    AllocationNull,
    Allocated {
        native_unique_id: i32,
        /// Deterministic test seam for the post-ID vector-growth failure. The
        /// production host returns true and the map owner still uses
        /// `try_reserve` before publishing the registry entry.
        registry_append_allowed: bool,
    },
}

pub(crate) trait LoadCellRecalcEffects {
    type Error;

    /// Model the outer allocation and shared native-ID assignment. Registry
    /// append and cell-index publication remain map-owned and occur before the
    /// later terrain Anim callback.
    fn allocate_automatic_tube(
        &mut self,
        request: AutomaticTubeRequest,
    ) -> Result<AutomaticTubeAllocation, Self::Error>;

    /// Complete the terrain-attached Anim constructor/Middle side effects
    /// synchronously. The map owner sets its cell latch only after this call
    /// succeeds.
    fn construct_terrain_attached_anim(
        &mut self,
        request: &TerrainTileAnimation,
    ) -> Result<(), Self::Error>;
}

#[derive(Debug, thiserror::Error)]
pub(crate) enum LoadCellRecalcError<E> {
    #[error("runtime Recalc has no resident bridge inputs")]
    MissingResidentInputs,
    #[error(transparent)]
    ResidentInput(BridgeRecalcCatalogError),
    #[error("load Recalc received non-native overlay identity {identity}")]
    MalformedOverlayIdentity { identity: i32 },
    #[error("load Recalc could not resolve OverlayType index {overlay_id}")]
    MissingOverlayType { overlay_id: u8 },
    #[error("load Recalc cell index {index} is outside the live terrain grid")]
    CellIndexOutOfBounds { index: usize },
    #[error("terrain-attached animation construction failed during load Recalc")]
    Effect(E),
}

#[derive(Debug)]
pub(crate) struct LoadCellRecalcOutcome {
    pub(crate) finalized: FinalizedOverlayCell,
    pub(crate) zone_before: u8,
    pub(crate) zone_after: u8,
    pub(crate) navigation_changed: bool,
}

/// Leptons per cell along one map axis.
const LEPTONS_PER_CELL: i32 = crate::util::lepton::LEPTONS_PER_CELL_I32;
/// Cell-centre offset used by the tile-animation spawn coordinate.
const CELL_CENTRE_LEPTONS: i32 = LEPTONS_PER_CELL / 2;

/// Numerator of the exact screen-pixel → world-lepton scale used by the
/// tactical pixel-to-world transform, over `PIXEL_TO_LEPTON_DENOMINATOR`.
///
/// The native matrix row is `[ +s, 2s, 0, 0 ]` / `[ -s, 2s, 0, 0 ]` where `s` is
/// the single-precision constant `4.2667` — an authored decimal approximation of
/// 256/60, i.e. leptons per cell over the isometric diamond half-width in
/// pixels. The `Y` coefficient is exactly `2s` (same mantissa, exponent + 1),
/// so both output axes reduce to `s * k` for an integer `k` and the whole
/// transform is exact rational arithmetic followed by one truncation.
///
/// Working in integers rather than `f32` keeps this off the float path; the
/// equivalence over the reachable offset range is pinned by
/// `gsi_13_04_tile_anim_offset_matches_native_float_transform`.
const PIXEL_TO_LEPTON_NUMERATOR: i64 = 4_473_959;
const PIXEL_TO_LEPTON_DENOMINATOR: i64 = 1_048_576;

/// Live contents of MapClass's one process-global fallback `CellClass`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SharedCellDummySnapshot {
    pub coord: (i32, i32),
    pub level: i8,
    pub slope_type: u8,
    /// Exact modeled subset of `CellClass+0x140`; always masked to `0x1180`.
    pub bridge_flags_0x1180: u32,
}

/// One `MapClass::Get_CellClass` result as consumed by FNPC's isometric
/// projection helper.
///
/// Keeping the signed level and raw bridge subset in one value prevents callers
/// from accidentally performing two dummy-stamping lookups for one native probe.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct CellClassProjectionView {
    pub signed_level: i32,
    pub raw_flags_0x1180: u32,
}

const UNALLOCATED_REAL_CELL_BRIDGE_FLAGS: u16 = u16::MAX;

/// Serialized value authority for the exact `CellClass+0x140 & 0x1D80`
/// subset of every allocated real cell.
///
/// Native `CellClass::Load` restores real object flag words directly, while
/// `MapClass::Resize` reconstructs only the process-global dummy. Rust keeps
/// the derived terrain grid out of Scenario serialization, so this compact
/// aligned projection carries the five future-affecting bits without retaining
/// setter history. `u16::MAX` marks native-unallocated slots; every other
/// value is exactly one masked `0x1D80` subset. The historical type/field name
/// remains1180 for schema continuity; snapshot141 rejects pre-restamp saves.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Default, serde::Serialize, serde::Deserialize)]
pub(crate) struct RealCellBridgeFlags0x1180 {
    width: u16,
    height: u16,
    flags_or_unallocated: Vec<u16>,
}

impl RealCellBridgeFlags0x1180 {
    fn from_grid(grid: &ResolvedTerrainGrid) -> Self {
        let flags_or_unallocated = grid
            .cells
            .iter()
            .enumerate()
            .map(|(index, cell)| {
                if grid
                    .native_allocated
                    .as_deref()
                    .is_none_or(|mask| mask.get(index).copied().unwrap_or(false))
                {
                    (cell.bridge_facts.raw_flags & RETAINED_CELLCLASS_BRIDGE_FLAG_MASK) as u16
                } else {
                    UNALLOCATED_REAL_CELL_BRIDGE_FLAGS
                }
            })
            .collect();
        Self {
            width: grid.width,
            height: grid.height,
            flags_or_unallocated,
        }
    }

    fn matches_grid_shape(&self, grid: &ResolvedTerrainGrid) -> bool {
        self.width == grid.width
            && self.height == grid.height
            && self.flags_or_unallocated.len() == grid.cells.len()
    }

    pub(crate) fn set_allocated_cell(&mut self, index: usize, flags: u32) {
        let Some(slot) = self.flags_or_unallocated.get_mut(index) else {
            debug_assert!(false, "real-cell bridge flag update index must be in range");
            return;
        };
        debug_assert_ne!(*slot, UNALLOCATED_REAL_CELL_BRIDGE_FLAGS);
        *slot = (flags & RETAINED_CELLCLASS_BRIDGE_FLAG_MASK) as u16;
    }
}

/// Ephemeral mutable view of the represented live `CellClass+0x140` bridge
/// bits during one native state-machine transaction.
///
/// Ramp helpers recurse before their caller can project a returned outcome.
/// This value seam mirrors allocated real cells and retains the actual shared
/// dummy identity while the transaction is being constructed. Each native
/// setter call updates the real-cell mirror and synchronously stamps/mutates
/// that live dummy, so later recursive GetCell frames observe exact flag and
/// coordinate interleaving. The ordered setter transcript subsequently
/// commits only allocated real-cell values and serialized authority; replaying
/// it through the dummy would duplicate and reorder already-live effects.
#[derive(Debug, Clone)]
pub(crate) struct CellClassBridgeFlagState {
    width: u16,
    height: u16,
    native_allocated: Option<Vec<bool>>,
    flags: Vec<u16>,
    shared_cell_dummy: SharedCellDummy,
}

impl CellClassBridgeFlagState {
    fn from_grid(grid: &ResolvedTerrainGrid) -> Self {
        Self {
            width: grid.width,
            height: grid.height,
            native_allocated: grid.native_allocated.clone(),
            flags: grid
                .cells
                .iter()
                .map(|cell| {
                    (cell.bridge_facts.raw_flags & RETAINED_CELLCLASS_BRIDGE_FLAG_MASK) as u16
                })
                .collect(),
            shared_cell_dummy: grid.shared_cell_dummy.clone(),
        }
    }

    pub(crate) fn flags_at(&self, coord: (u16, u16)) -> u32 {
        native_resolved_cell_index(
            self.width,
            self.height,
            self.native_allocated.as_deref(),
            self.flags.len(),
            i32::from(coord.0),
            i32::from(coord.1),
        )
        .and_then(|index| self.flags.get(index).copied())
        .map(u32::from)
        .unwrap_or_else(|| self.shared_cell_dummy.bridge_flags_0x1180())
    }

    pub(crate) fn apply_stamp(&mut self, stamp: BridgeFlagStamp) {
        let Some(slots) = stamp.slots() else {
            return;
        };
        for (slot, requested) in slots {
            let Some((x, y)) = requested else {
                continue;
            };
            if let Some(index) = native_resolved_cell_index(
                self.width,
                self.height,
                self.native_allocated.as_deref(),
                self.flags.len(),
                x,
                y,
            ) {
                let mut flags = u32::from(self.flags[index]);
                crate::map::bridge_facts::apply_retained_cellclass_bridge_slot(
                    &mut flags,
                    slot,
                    stamp.set,
                    stamp.direction,
                );
                self.flags[index] = flags as u16;
            } else {
                self.shared_cell_dummy.stamp_coord(x, y);
                self.shared_cell_dummy.apply_retained_bridge_flag_slot(
                    slot,
                    stamp.set,
                    stamp.direction,
                );
            }
        }
    }
}

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
    pub build_blocked: bool,
    pub has_bridge_deck: bool,
    pub bridge_walkable: bool,
    pub bridge_transition: bool,
    pub bridge_deck_level: u8,
    pub bridge_layer: Option<BridgeLayer>,
    pub bridge_facts: BridgeCellFacts,
    pub radar_left: [u8; 3],
    pub radar_right: [u8; 3],
    pub has_damaged_data: bool,
    pub bridgehead_anchor_class_at_load: Option<crate::map::bridge_facts::BridgeheadAnchorClass>,
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
            build_blocked: cell.build_blocked,
            has_bridge_deck: cell.has_bridge_deck,
            bridge_walkable: cell.bridge_walkable,
            bridge_transition: cell.bridge_transition,
            bridge_deck_level: cell.bridge_deck_level,
            bridge_layer: cell.bridge_layer.clone(),
            bridge_facts: cell.bridge_facts,
            radar_left: cell.radar_left,
            radar_right: cell.radar_right,
            has_damaged_data: cell.has_damaged_data,
            bridgehead_anchor_class_at_load: cell.bridgehead_anchor_class_at_load,
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
        cell.build_blocked = self.build_blocked;
        cell.has_bridge_deck = self.has_bridge_deck;
        cell.bridge_walkable = self.bridge_walkable;
        cell.bridge_transition = self.bridge_transition;
        cell.bridge_deck_level = self.bridge_deck_level;
        cell.bridge_layer = self.bridge_layer.clone();
        cell.bridge_facts = self.bridge_facts;
        cell.radar_left = self.radar_left;
        cell.radar_right = self.radar_right;
        cell.has_damaged_data = self.has_damaged_data;
        cell.bridgehead_anchor_class_at_load = self.bridgehead_anchor_class_at_load;
    }
}

#[derive(Debug, Clone)]
struct DynamicTilePrototype {
    metadata: TileMetadata,
    accepts_smudge: bool,
    allows_tiberium: bool,
}

#[derive(Debug, Clone)]
struct SparseTileTemplate {
    tile_id: u16,
    width: u8,
    height: u8,
    entries: Vec<Option<DynamicTilePrototype>>,
}

#[derive(Debug, Clone)]
struct DestroyableCliffCatalog {
    destroyable_start: u16,
    old: [SparseTileTemplate; 2],
    replacements: [SparseTileTemplate; 4],
    lat_config: lat::LatConfig,
    slope_config: lat::SlopeFixupConfig,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DestroyableCliffFamily {
    A,
    B,
}

#[derive(Debug, Clone)]
pub(crate) struct DestroyableCliffMutation {
    pub family: DestroyableCliffFamily,
    pub origin: (i16, i16),
    pub original_footprint: Vec<(u16, u16)>,
    pub animation_cells: Vec<(i16, i16)>,
    pub changed_cells: Vec<(u16, u16)>,
}

/// Send-safe identity handle for MapClass's process-global fallback `CellClass`.
///
/// Active YR owns one object at `0x00ABDC50`. `MapClass::Get_CellClass` at
/// `0x005657A0` and `0x00565730` overwrite only its packed coordinate words at
/// `+0x24`; the independently writable level/slope bytes at `+0x11B/+0x11C`
/// survive those misses. Coordinate/level/slope and overlay identity/state
/// each occupy an atomic word; the full flag word has its own authority. Their
/// shared allocation keeps one live identity safe to carry through app loading
/// workers without copying native's global mutable object architecture.
#[derive(Debug, Clone)]
pub struct SharedCellDummy {
    state: Arc<SharedCellDummyState>,
}

#[derive(Debug)]
struct SharedCellDummyState {
    cell: AtomicU64,
    /// Complete CellClass+0x140, including the setter's bit0x10000.
    raw_flags: AtomicU32,
    /// CellClass+0x2C: 0=null, 1=shared dummy, real storage index+2 otherwise.
    native_anchor: AtomicU64,
    /// Low dword is signed Cell+0x44 identity; bits 32..39 are Cell+0x11E.
    overlay: AtomicU64,
    /// CellClass+0x116, written even when ReadTubesINI resolves a dummy cell.
    tube_index: AtomicI16,
}

/// One native map-query identity domain. Bulk terrain is borrowed; input
/// producers fork only the small fallback CellClass and retain it across every
/// nested miss. Cloning ResolvedTerrainGrid would share its canonical Dummy.
#[derive(Debug)]
pub struct NativeCellQuery<'a> {
    terrain: &'a ResolvedTerrainGrid,
    dummy: SharedCellDummy,
}

impl<'a> NativeCellQuery<'a> {
    /// Simulation queries retain the map's one live fallback identity. The
    /// same nested query APIs also serve isolated ordinary input producers.
    pub(crate) fn canonical(terrain: &'a ResolvedTerrainGrid) -> Self {
        Self {
            terrain,
            dummy: terrain.shared_cell_dummy.clone(),
        }
    }

    pub(crate) fn isolated(terrain: &'a ResolvedTerrainGrid) -> Self {
        Self {
            terrain,
            dummy: terrain.shared_cell_dummy.fork_query_identity(),
        }
    }

    pub(crate) fn terrain(&self) -> &'a ResolvedTerrainGrid {
        self.terrain
    }
    pub(crate) fn dummy(&self) -> SharedCellDummy {
        self.dummy.clone()
    }

    pub(crate) fn lookup(&self, coord: (i16, i16)) -> NativeCellIdentity {
        self.terrain
            .native_fixed_cell_index(coord.0, coord.1)
            .map_or_else(
                || {
                    self.dummy
                        .stamp_coord(i32::from(coord.0), i32::from(coord.1));
                    NativeCellIdentity::Dummy
                },
                NativeCellIdentity::Real,
            )
    }

    pub(crate) fn lookup_world(&self, x: i32, y: i32) -> NativeCellIdentity {
        let (x, y) = (x / 256, y / 256);
        let index = y.wrapping_mul(512).wrapping_add(x);
        if (0..0x40000).contains(&index)
            && let Some(real) = self
                .terrain
                .native_fixed_cell_index((index % 512) as i16, (index / 512) as i16)
        {
            return NativeCellIdentity::Real(real);
        }
        self.dummy
            .stamp_coord(i32::from(x as i16), i32::from(y as i16));
        NativeCellIdentity::Dummy
    }

    pub(crate) fn coord(&self, cell: NativeCellIdentity) -> (i16, i16) {
        match cell {
            NativeCellIdentity::Real(_) => self.terrain.native_cell_coord(cell),
            NativeCellIdentity::Dummy => {
                let (x, y) = self.dummy.snapshot().coord;
                (x as i16, y as i16)
            }
        }
    }

    pub(crate) fn ground_fields(&self, cell: NativeCellIdentity) -> (u8, u8) {
        match cell {
            NativeCellIdentity::Real(_) => self.terrain.native_cell_ground_fields(cell),
            NativeCellIdentity::Dummy => {
                let state = self.dummy.snapshot();
                (state.level as u8, state.slope_type)
            }
        }
    }

    pub(crate) fn flags(&self, cell: NativeCellIdentity) -> u32 {
        match cell {
            NativeCellIdentity::Real(_) => self.terrain.native_cell_flags(cell),
            NativeCellIdentity::Dummy => self.dummy.raw_flags(),
        }
    }

    pub(crate) fn projection_view(&self, x: i32, y: i32) -> CellClassProjectionView {
        let cell = self.lookup((x as i16, y as i16));
        CellClassProjectionView {
            signed_level: i32::from(self.ground_fields(cell).0 as i8),
            raw_flags_0x1180: self.flags(cell) & MODELED_CELLCLASS_BRIDGE_FLAG_MASK,
        }
    }
}

const SHARED_DUMMY_DEFAULT_OVERLAY: u64 = u32::MAX as u64;

impl Default for SharedCellDummy {
    fn default() -> Self {
        Self::fresh()
    }
}

impl SharedCellDummy {
    pub fn fresh() -> Self {
        Self {
            state: Arc::new(SharedCellDummyState {
                cell: AtomicU64::new(0),
                raw_flags: AtomicU32::new(0),
                native_anchor: AtomicU64::new(0),
                overlay: AtomicU64::new(SHARED_DUMMY_DEFAULT_OVERLAY),
                tube_index: AtomicI16::new(-1),
            }),
        }
    }

    /// Input queries borrow bulk map state but must not mutate the simulation's
    /// retained fallback identity. Copy every modeled byte, not just the
    /// coordinate/height snapshot, and keep one identity for the whole query.
    pub(crate) fn fork_query_identity(&self) -> Self {
        Self {
            state: Arc::new(SharedCellDummyState {
                cell: AtomicU64::new(self.state.cell.load(Ordering::Relaxed)),
                raw_flags: AtomicU32::new(self.state.raw_flags.load(Ordering::Relaxed)),
                native_anchor: AtomicU64::new(self.state.native_anchor.load(Ordering::Relaxed)),
                overlay: AtomicU64::new(self.state.overlay.load(Ordering::Relaxed)),
                tube_index: AtomicI16::new(self.state.tube_index.load(Ordering::Relaxed)),
            }),
        }
    }

    /// Reconstruct the modeled fields in place at the native Resize boundary.
    ///
    /// `MapClass::Resize @ 0x00565C10` unconditionally calls
    /// `CellClass::Constructor @ 0x0047BBF0` on the fixed dummy object at
    /// `0x00ABDC50` (`0x005670E7..0x005670F2`). The address survives, while
    /// coordinate `+0x24`, level `+0x11B`, slope `+0x11C`, and modeled
    /// `+0x140 & 0x1180` bridge bits return to zero, while overlay identity
    /// returns to signed `-1` and OverlayData to zero. Raw Tube index+0x116
    /// returns to-1 at47BC48 (PHASE3_TUBE_HIERARCHY_20260910.md). Other constructor-owned
    /// fields are not represented by this handle yet.
    pub(crate) fn reconstruct_for_map_resize(&self) {
        self.state.cell.store(0, Ordering::Relaxed);
        // Constructor47BBF0: AND FF800000 at47BCE1; preserve upper residue.
        self.state
            .raw_flags
            .fetch_and(0xff80_0000, Ordering::Relaxed);
        self.state.native_anchor.store(0, Ordering::Relaxed);
        self.state.tube_index.store(-1, Ordering::Relaxed);
        self.state
            .overlay
            .store(SHARED_DUMMY_DEFAULT_OVERLAY, Ordering::Relaxed);
    }

    pub fn snapshot(&self) -> SharedCellDummySnapshot {
        let packed = self.state.cell.load(Ordering::Relaxed);
        SharedCellDummySnapshot {
            coord: (
                i32::from((packed as u16) as i16),
                i32::from(((packed >> 16) as u16) as i16),
            ),
            level: ((packed >> 32) as u8) as i8,
            slope_type: (packed >> 40) as u8,
            bridge_flags_0x1180: self.raw_flags() & MODELED_CELLCLASS_BRIDGE_FLAG_MASK,
        }
    }

    pub(crate) fn raw_tube_index(&self) -> i16 {
        self.state.tube_index.load(Ordering::Relaxed)
    }

    pub(crate) fn write_raw_tube_index(&self, index: i16) {
        self.state.tube_index.store(index, Ordering::Relaxed);
    }

    /// Publish an already validated post-Resize candidate onto the retained
    /// process identity. Preserve its later lookup effects; do not reset twice.
    pub(crate) fn adopt_prepared_load_state(&self, prepared: &Self) {
        self.state.cell.store(
            prepared.state.cell.load(Ordering::Relaxed),
            Ordering::Relaxed,
        );
        self.state.overlay.store(
            prepared.state.overlay.load(Ordering::Relaxed),
            Ordering::Relaxed,
        );
        self.state
            .raw_flags
            .store(prepared.raw_flags(), Ordering::Relaxed);
        self.write_native_anchor(prepared.native_anchor());
        self.state
            .tube_index
            .store(prepared.raw_tube_index(), Ordering::Relaxed);
    }

    /// Stamp only CellClass+0x24, preserving the live level and slope bytes.
    pub fn stamp_coord(&self, x: i32, y: i32) {
        let coord = u64::from(x as i16 as u16) | (u64::from(y as i16 as u16) << 16);
        let _ = self
            .state
            .cell
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |current| {
                Some((current & !0xffff_ffff) | coord)
            });
    }

    pub fn same_identity(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.state, &other.state)
    }

    pub(crate) fn raw_flags(&self) -> u32 {
        self.state.raw_flags.load(Ordering::Relaxed)
    }

    pub(crate) fn write_raw_flags(&self, flags: u32) {
        self.state.raw_flags.store(flags, Ordering::Relaxed);
    }

    pub(crate) fn native_anchor(&self) -> Option<NativeCellIdentity> {
        match self.state.native_anchor.load(Ordering::Relaxed) {
            0 => None,
            1 => Some(NativeCellIdentity::Dummy),
            value => Some(NativeCellIdentity::Real((value - 2) as usize)),
        }
    }

    pub(crate) fn write_native_anchor(&self, anchor: Option<NativeCellIdentity>) {
        let encoded = match anchor {
            None => 0,
            Some(NativeCellIdentity::Dummy) => 1,
            Some(NativeCellIdentity::Real(index)) => index as u64 + 2,
        };
        self.state.native_anchor.store(encoded, Ordering::Relaxed);
    }

    /// Read the native signed overlay identity dword and independent state byte.
    pub(crate) fn overlay_identity_state(&self) -> (i32, u8) {
        let packed = self.state.overlay.load(Ordering::Relaxed);
        (packed as u32 as i32, (packed >> 32) as u8)
    }

    pub(crate) fn write_overlay_identity(&self, identity: i32) {
        let identity = u64::from(identity as u32);
        let _ = self
            .state
            .overlay
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |current| {
                Some((current & !u64::from(u32::MAX)) | identity)
            });
    }

    pub(crate) fn write_overlay_state(&self, state: u8) {
        const STATE_MASK: u64 = 0xff << 32;
        let state = u64::from(state) << 32;
        let _ = self
            .state
            .overlay
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |current| {
                Some((current & !STATE_MASK) | state)
            });
    }

    pub(crate) fn write_overlay_identity_state(&self, identity: i32, state: u8) {
        self.state.overlay.store(
            u64::from(identity as u32) | (u64::from(state) << 32),
            Ordering::Relaxed,
        );
    }

    /// Read the fallback CellClass overlay identity/data pair as the runtime
    /// crate/Mark writers see it. `None` is the native signed `-1` sentinel,
    /// so runtime overlay index zero stays distinct from an empty dummy.
    pub(crate) fn overlay_fields(&self) -> (Option<u8>, u8) {
        let (identity, state) = self.overlay_identity_state();
        (u8::try_from(identity).ok(), state)
    }

    /// Write only fallback `CellClass+0x44/+0x11E`, preserving every other
    /// process-global dummy field.
    pub(crate) fn set_overlay_fields(&self, overlay_id: Option<u8>, overlay_data: u8) {
        self.write_overlay_identity_state(overlay_id.map_or(-1, i32::from), overlay_data);
    }

    /// Apply one exact `SetBridgeDirection_*` slot to the modeled flag subset
    /// without disturbing coordinate, level, or slope writers.
    #[cfg(test)]
    pub(crate) fn apply_bridge_flag_slot(&self, slot: BridgeStampSlot, set: bool) {
        let _ =
            self.state
                .raw_flags
                .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |current| {
                    let mut flags = current;
                    crate::map::bridge_facts::apply_modeled_cellclass_bridge_slot(
                        &mut flags, slot, set,
                    );
                    Some(flags)
                });
    }

    pub(crate) fn bridge_flags_0x1180(&self) -> u32 {
        self.snapshot().bridge_flags_0x1180
    }

    pub(crate) fn retained_bridge_flags(&self) -> u32 {
        self.raw_flags() & RETAINED_CELLCLASS_BRIDGE_FLAG_MASK
    }

    #[cfg(test)]
    pub(crate) fn test_set_retained_bridge_flags(&self, flags: u32) {
        self.update_retained_bridge_flags(|_| flags);
    }

    fn update_retained_bridge_flags(&self, update: impl Fn(u32) -> u32) {
        let _ =
            self.state
                .raw_flags
                .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |current| {
                    let flags = update(current) & RETAINED_CELLCLASS_BRIDGE_FLAG_MASK;
                    Some((current & !RETAINED_CELLCLASS_BRIDGE_FLAG_MASK) | flags)
                });
    }

    pub(crate) fn apply_retained_bridge_flag_slot(
        &self,
        slot: BridgeStampSlot,
        set: bool,
        direction: u8,
    ) {
        self.update_retained_bridge_flags(|mut flags| {
            crate::map::bridge_facts::apply_retained_cellclass_bridge_slot(
                &mut flags, slot, set, direction,
            );
            flags
        });
    }

    fn apply_full_bridge_flag_slot(
        &self,
        slot: BridgeStampSlot,
        stamp: BridgeFlagStamp,
        family: BridgeStampFamily,
        anchor: NativeCellIdentity,
    ) {
        let mut facts = BridgeCellFacts {
            raw_flags: self.raw_flags(),
            ..Default::default()
        };
        crate::map::bridge_facts::apply_bridge_fact_slot(
            &mut facts,
            slot,
            BridgeAnchorRelation {
                anchor: (stamp.anchor.0 as u16, stamp.anchor.1 as u16),
                slot,
                family,
                direction: stamp.direction,
            },
            stamp.set,
        );
        self.write_raw_flags(facts.raw_flags);
        if slot.writes_native_anchor() {
            self.write_native_anchor(stamp.set.then_some(anchor));
        }
        if matches!(
            slot,
            BridgeStampSlot::Anchor
                | BridgeStampSlot::Forward1
                | BridgeStampSlot::Forward2
                | BridgeStampSlot::Opposite
        ) {
            self.write_overlay_state(facts.state_byte);
        }
    }

    #[cfg(test)]
    pub(crate) fn set_bridge_flags_0x1180(&self, flags: u32) {
        let _ =
            self.state
                .raw_flags
                .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |current| {
                    Some(
                        (current & !MODELED_CELLCLASS_BRIDGE_FLAG_MASK)
                            | (flags & MODELED_CELLCLASS_BRIDGE_FLAG_MASK),
                    )
                });
    }

    #[cfg(test)]
    pub(crate) fn set_level_slope(&self, level: i8, slope_type: u8) {
        const LEVEL_SLOPE_MASK: u64 = 0xffff << 32;
        let value = (u64::from(level as u8) << 32) | (u64::from(slope_type) << 40);
        let _ = self
            .state
            .cell
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |current| {
                Some((current & !LEVEL_SLOPE_MASK) | value)
            });
    }

    pub(crate) fn set_level(&self, level: i8) {
        const LEVEL_MASK: u64 = 0xff << 32;
        let value = u64::from(level as u8) << 32;
        let _ = self
            .state
            .cell
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |current| {
                Some((current & !LEVEL_MASK) | value)
            });
    }
}

fn native_resolved_cell_index(
    width: u16,
    height: u16,
    native_allocated: Option<&[bool]>,
    cell_count: usize,
    x: i32,
    y: i32,
) -> Option<usize> {
    let linear = crate::map::cell_index::cell_linear_index(x, y)?;
    let rx = (linear % crate::map::cell_index::CELL_ROW_STRIDE) as usize;
    let ry = (linear / crate::map::cell_index::CELL_ROW_STRIDE) as usize;
    if rx >= usize::from(width) || ry >= usize::from(height) {
        return None;
    }
    let index = ry * usize::from(width) + rx;
    (index < cell_count
        && native_allocated.is_none_or(|mask| mask.get(index).copied().unwrap_or(false)))
    .then_some(index)
}

/// Route one exact `SetBridgeDirection_*` transaction through native fixed
/// indexing. A real slot mutates its retained facts; every miss stamps and
/// mutates the same process-global dummy identity.
fn apply_native_bridge_flag_stamp_to_parts(
    cells: &mut [ResolvedTerrainCell],
    width: u16,
    height: u16,
    native_allocated: Option<&[bool]>,
    shared_cell_dummy: &SharedCellDummy,
    stamp: BridgeFlagStamp,
    map_family: Option<BridgeStampFamily>,
) -> Vec<(usize, u32)> {
    let Some(slots) = stamp.slots() else {
        return Vec::new();
    };
    let anchor = native_resolved_cell_index(
        width,
        height,
        native_allocated,
        cells.len(),
        i32::from(stamp.anchor.0),
        i32::from(stamp.anchor.1),
    )
    .map_or(NativeCellIdentity::Dummy, NativeCellIdentity::Real);
    let mut real_cell_updates = Vec::with_capacity(slots.len());
    for (slot, requested) in slots {
        let Some((x, y)) = requested else {
            continue;
        };
        if let Some(index) =
            native_resolved_cell_index(width, height, native_allocated, cells.len(), x, y)
        {
            let facts = &mut cells[index].bridge_facts;
            if let Some(family) = map_family {
                crate::map::bridge_facts::apply_bridge_fact_slot(
                    facts,
                    slot,
                    BridgeAnchorRelation {
                        anchor: (stamp.anchor.0 as u16, stamp.anchor.1 as u16),
                        slot,
                        family,
                        direction: stamp.direction,
                    },
                    stamp.set,
                );
                if slot.writes_native_anchor() {
                    facts.native_anchor = stamp.set.then_some(anchor);
                }
            } else {
                crate::map::bridge_facts::apply_retained_cellclass_bridge_slot(
                    &mut facts.raw_flags,
                    slot,
                    stamp.set,
                    stamp.direction,
                );
            }
            real_cell_updates.push((index, facts.raw_flags & RETAINED_CELLCLASS_BRIDGE_FLAG_MASK));
        } else {
            shared_cell_dummy.stamp_coord(x, y);
            if let Some(family) = map_family {
                shared_cell_dummy.apply_full_bridge_flag_slot(slot, stamp, family, anchor);
            } else {
                shared_cell_dummy.apply_retained_bridge_flag_slot(slot, stamp.set, stamp.direction);
            }
        }
    }
    real_cell_updates
}

/// Project a setter transcript that already executed against the live dummy
/// through [`CellClassBridgeFlagState`]. Only allocated real CellClass values
/// are deferred; missing slots must have no second lookup or dummy mutation.
fn apply_planned_bridge_flag_stamp_to_real_parts(
    cells: &mut [ResolvedTerrainCell],
    width: u16,
    height: u16,
    native_allocated: Option<&[bool]>,
    stamp: BridgeFlagStamp,
) -> Vec<(usize, u32)> {
    let Some(slots) = stamp.slots() else {
        return Vec::new();
    };
    let mut real_cell_updates = Vec::with_capacity(slots.len());
    for (slot, requested) in slots {
        let Some((x, y)) = requested else {
            continue;
        };
        let Some(index) =
            native_resolved_cell_index(width, height, native_allocated, cells.len(), x, y)
        else {
            continue;
        };
        let facts = &mut cells[index].bridge_facts;
        crate::map::bridge_facts::apply_retained_cellclass_bridge_slot(
            &mut facts.raw_flags,
            slot,
            stamp.set,
            stamp.direction,
        );
        real_cell_updates.push((index, facts.raw_flags & RETAINED_CELLCLASS_BRIDGE_FLAG_MASK));
    }
    real_cell_updates
}

fn refresh_runtime_bridge_projection(cell: &mut ResolvedTerrainCell, structural_removed: bool) {
    let facts = cell.bridge_facts;
    if facts.has_structural_bridge() {
        cell.has_bridge_deck = true;
        cell.bridge_walkable = !cell.terrain_object_blocks && !cell.overlay_blocks;
        cell.bridge_deck_level = cell.level.saturating_add(4);
        cell.build_blocked = cell.base_build_blocked
            || cell.terrain_object_blocks
            || cell.overlay_blocks
            || cell.bridge_walkable;
    } else if structural_removed
        || (facts.family != BridgeStampFamily::None
            && cell
                .bridge_layer
                .as_ref()
                .is_some_and(|layer| layer.direction != BridgeDirection::Low))
    {
        cell.has_bridge_deck = false;
        cell.bridge_walkable = false;
        cell.bridge_deck_level = cell.level;
        cell.build_blocked =
            cell.base_build_blocked || cell.terrain_object_blocks || cell.overlay_blocks;
    }
    if facts.has_transition_flag() {
        cell.bridge_transition = true;
    }
}

/// Convert a `Tile%02dXOffset` / `YOffset` screen-pixel pair into the world
/// lepton offset the animation spawn adds to the cell centre.
///
/// The native helper bails out to a zero coordinate when either offset is at or
/// beyond the tactical viewport extent; every stock offset is under 64 pixels,
/// so that guard is unreachable here and is deliberately not reproduced.
/// Truncation is toward zero, matching the native float-to-long conversion.
pub fn tile_anim_pixel_offset_to_leptons(x_offset: i32, y_offset: i32) -> (i32, i32) {
    let scale =
        |k: i64| -> i32 { ((PIXEL_TO_LEPTON_NUMERATOR * k) / PIXEL_TO_LEPTON_DENOMINATOR) as i32 };
    let px = i64::from(x_offset);
    let py = i64::from(y_offset);
    (scale(px + 2 * py), scale(2 * py - px))
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
    /// Active theater WaterSet cumulative tile base (global AA0738), not the
    /// sticky Fill cache. ReadTheater545150 resets to -1; Lunar also uses -1.
    projectile_water_set_base: i32,
    /// Active theater tile registry length. Positive out-of-range ids present
    /// as ClearTile while their stored semantic id remains untouched.
    tile_registry_len: Option<usize>,
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

/// Type barrier for the pristine authored Fill product. It intentionally does
/// not expose `ResolvedTerrainGrid`'s pathing/presentation surface: the inner
/// grid must first be consumed by the native Tube/overlay/Recalc transaction.
pub(crate) struct AuthoredTerrainFill(ResolvedTerrainGrid);

#[derive(Debug, thiserror::Error)]
pub(crate) enum NativeTubeRegistryBindError {
    #[error("pending authored terrain Tube registry is already populated")]
    RegistryAlreadyPopulated,
    #[error("pending authored terrain Tube registry could not reserve {entries} entries")]
    RegistryCapacity { entries: usize },
}

impl AuthoredTerrainFill {
    pub(crate) fn into_pending_grid(self) -> ResolvedTerrainGrid {
        self.0
    }

    pub(crate) fn dimensions(&self) -> (u16, u16) {
        (self.0.width(), self.0.height())
    }

    #[cfg(test)]
    pub(crate) fn pending_grid(&self) -> &ResolvedTerrainGrid {
        &self.0
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
        build_blocked: false,
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
        bridgehead_anchor_class_at_load: None,
    }
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
            projectile_water_set_base: -1,
            tile_registry_len: None,
            bridge_set_start: None,
            high_bridge_rim_tiles: None,
            wood_bridge_set_start: None,
            tile_animations: Vec::new(),
            destroyable_cliff_catalog: None,
            bridge_recalc_catalog: None,
        }
    }

    /// Terrain animations to spawn once at map load, in native creation order.
    pub fn tile_animations(&self) -> &[TerrainTileAnimation] {
        &self.tile_animations
    }

    pub fn width(&self) -> u16 {
        self.width
    }

    pub(crate) fn projectile_water_set_base(&self) -> i32 {
        self.projectile_water_set_base
    }

    #[cfg(test)]
    pub(crate) fn set_projectile_water_set_base(&mut self, base: i32) {
        self.projectile_water_set_base = base;
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

    pub(crate) fn shared_cell_dummy(&self) -> SharedCellDummy {
        self.shared_cell_dummy.clone()
    }

    /// Resolve one already-narrowed native fixed-grid coordinate to an
    /// allocated real CellClass slot. The caller owns dummy coordinate stamping
    /// on `None`, because some native probes test admission without GetCell.
    pub(crate) fn native_fixed_cell_index(&self, x: i16, y: i16) -> Option<usize> {
        native_resolved_cell_index(
            self.width,
            self.height,
            self.native_allocated.as_deref(),
            self.cells.len(),
            i32::from(x),
            i32::from(y),
        )
    }

    /// Native5657A0 lookup; retain the returned identity across callbacks.
    pub(crate) fn native_cell_identity(&self, coord: (i16, i16)) -> NativeCellIdentity {
        self.native_fixed_cell_index(coord.0, coord.1).map_or_else(
            || {
                self.shared_cell_dummy
                    .stamp_coord(i32::from(coord.0), i32::from(coord.1));
                NativeCellIdentity::Dummy
            },
            NativeCellIdentity::Real,
        )
    }

    pub(crate) fn native_cell_coord(&self, cell: NativeCellIdentity) -> (i16, i16) {
        match cell {
            NativeCellIdentity::Real(index) => {
                (self.cells[index].rx as i16, self.cells[index].ry as i16)
            }
            NativeCellIdentity::Dummy => {
                let (x, y) = self.shared_cell_dummy.snapshot().coord;
                (x as i16, y as i16)
            }
        }
    }

    pub(crate) fn native_cell_flags(&self, cell: NativeCellIdentity) -> u32 {
        match cell {
            NativeCellIdentity::Real(index) => self.cells[index].bridge_facts.raw_flags,
            NativeCellIdentity::Dummy => self.shared_cell_dummy.raw_flags(),
        }
    }

    /// Read retained CellClass+38 without another Map lookup. The shared
    /// fallback keeps the constructor's 0xFFFF tile, independently of its
    /// mutable coordinate, level, slope and overlay fields.
    pub(crate) fn native_cell_tile_index(&self, cell: NativeCellIdentity) -> i32 {
        match cell {
            NativeCellIdentity::Real(index) => self.cells[index].final_tile_index,
            NativeCellIdentity::Dummy => 0xFFFF,
        }
    }

    /// Cell47B3A0 samples the receiver's own signed level and slope; unlike
    /// Map578080 it neither resolves a new cell nor restamps the shared Dummy.
    pub(crate) fn native_cell_ground_fields(&self, cell: NativeCellIdentity) -> (u8, u8) {
        match cell {
            NativeCellIdentity::Real(index) => {
                let cell = &self.cells[index];
                (cell.level, cell.slope_type)
            }
            NativeCellIdentity::Dummy => {
                let state = self.shared_cell_dummy.snapshot();
                (state.level as u8, state.slope_type)
            }
        }
    }

    pub(crate) fn native_cell_state(&self, cell: NativeCellIdentity) -> u8 {
        match cell {
            NativeCellIdentity::Real(index) => self.cells[index].bridge_facts.state_byte,
            NativeCellIdentity::Dummy => self.shared_cell_dummy.overlay_identity_state().1,
        }
    }

    pub(crate) fn native_cell_anchor(
        &self,
        cell: NativeCellIdentity,
    ) -> Option<NativeCellIdentity> {
        match cell {
            NativeCellIdentity::Real(index) => self.cells[index].bridge_facts.native_anchor,
            NativeCellIdentity::Dummy => self.shared_cell_dummy.native_anchor(),
        }
    }

    /// Literal scalar writes do not dispatch Recalc, radar or fallout. The
    /// Simulation host publishes its serialized/read projections before effects.
    pub(crate) fn write_native_cell_flags(&mut self, cell: NativeCellIdentity, flags: u32) {
        match cell {
            NativeCellIdentity::Real(index) => {
                let previous = self.cells[index].bridge_facts.raw_flags;
                self.cells[index].bridge_facts.raw_flags = flags;
                // Runtime constructor47E040/47E470 writes the orientation in
                // bit0x800. Keep the derived direction used by span rebuilding
                // current even when this cell has no authored stamp receipt.
                if flags
                    & (BRIDGE_FLAG_ANCHOR_SELF
                        | BRIDGE_FLAG_STRUCTURAL
                        | BRIDGE_FLAG_DESTROYED_OR_RAMP)
                    != 0
                {
                    self.cells[index].bridge_facts.direction =
                        Some(if flags & 0x800 != 0 { 0 } else { 6 });
                }
                // 47E040 removes the deck from stamped non-anchor cells too;
                // those cells need not carry an overlay/bridge_layer. Preserve
                // unrelated ramp projections when F3/extra only change markers.
                let structural_removed =
                    previous & BRIDGE_FLAG_STRUCTURAL != 0 && flags & BRIDGE_FLAG_STRUCTURAL == 0;
                refresh_runtime_bridge_projection(&mut self.cells[index], structural_removed);
                if structural_removed
                    || (previous ^ flags) & crate::map::bridge_facts::BRIDGE_FLAG_TRANSITION != 0
                {
                    self.cells[index].bridge_transition =
                        self.cells[index].bridge_facts.has_transition_flag();
                }
            }
            NativeCellIdentity::Dummy => self.shared_cell_dummy.write_raw_flags(flags),
        }
    }

    pub(crate) fn write_native_cell_state(&mut self, cell: NativeCellIdentity, state: u8) {
        match cell {
            NativeCellIdentity::Real(index) => self.cells[index].bridge_facts.state_byte = state,
            NativeCellIdentity::Dummy => self.shared_cell_dummy.write_overlay_state(state),
        }
    }

    /// Native568E40/569760 raw Cell+11B write, without47D2B0 Recalc.
    /// Evidence: tools/spatial_oracle/bridge_repair.{py,json}. The world owns
    /// subsequent publication to current readers; retained zone inputs stay intact.
    pub(crate) fn write_native_cell_level(&mut self, cell: NativeCellIdentity, level: u8) {
        match cell {
            NativeCellIdentity::Real(index) => {
                self.cells[index].level = level;
                self.cells[index].refresh_current_height_projection();
            }
            NativeCellIdentity::Dummy => self.shared_cell_dummy.set_level(level as i8),
        }
    }

    pub(crate) fn write_native_cell_anchor(
        &mut self,
        cell: NativeCellIdentity,
        anchor: Option<NativeCellIdentity>,
    ) {
        match cell {
            NativeCellIdentity::Real(index) => {
                self.cells[index].bridge_facts.native_anchor = anchor
            }
            NativeCellIdentity::Dummy => self.shared_cell_dummy.write_native_anchor(anchor),
        }
    }

    pub(crate) fn clear_native_cell_overlay(&mut self, cell: NativeCellIdentity) {
        self.write_native_cell_overlay(cell, None);
    }

    /// Raw Cell+44 publication; derived Recalc fields remain untouched until
    /// the caller reaches47D2B0. Ordinary repair stores all three identities first.
    pub(crate) fn write_native_cell_overlay(
        &mut self,
        cell: NativeCellIdentity,
        overlay: Option<u8>,
    ) {
        match cell {
            NativeCellIdentity::Real(index) => self.cells[index].bridge_facts.overlay_id = overlay,
            NativeCellIdentity::Dummy => self
                .shared_cell_dummy
                .write_overlay_identity(overlay.map_or(-1, i32::from)),
        }
    }

    /// Execute one live authored-load `CellClass::RecalcAttributes(-1)`.
    ///
    /// gamemd-derived: `CellClass::RecalcAttributes @ 0x0047D2B0`. The shared
    /// dummy is filtered by the caller before this method; every real call
    /// consumes the current identity, mutates the one live terrain cell, sees
    /// earlier neighbor mutations, and may synchronously construct exactly one
    /// terrain-attached Anim before latching the cell.
    pub(crate) fn recalc_authored_load_cell<E: LoadCellRecalcEffects>(
        &mut self,
        state: &mut LoadCellRecalcState<'_>,
        index: usize,
        overlay: FinalizedOverlayCell,
        effects: &mut E,
    ) -> Result<LoadCellRecalcOutcome, LoadCellRecalcError<E::Error>> {
        self.recalc_cell_attributes(state, index, overlay, -1, effects)
    }

    /// Shared47D2B0 entry: only the valid normal-TMP branch consumes the
    /// level override, after Tube construction and before dimensions/Anim.
    /// Evidence: tools/spatial_oracle/terrain_recalc.
    pub(crate) fn recalc_cell_attributes<E: LoadCellRecalcEffects>(
        &mut self,
        state: &mut LoadCellRecalcState<'_>,
        index: usize,
        overlay: FinalizedOverlayCell,
        level_override: i32,
        effects: &mut E,
    ) -> Result<LoadCellRecalcOutcome, LoadCellRecalcError<E::Error>> {
        let Some(snapshot) = self.cells.get(index).cloned() else {
            return Err(LoadCellRecalcError::CellIndexOutOfBounds { index });
        };
        let zone_before = snapshot.zone_type;
        let navigation_before = load_navigation_signature(&snapshot);
        let source_overlay_id = match overlay.identity() {
            NO_OVERLAY_IDENTITY => None,
            identity @ 0..=254 => Some(identity as u8),
            identity => {
                return Err(LoadCellRecalcError::MalformedOverlayIdentity { identity });
            }
        };
        let source_flags = source_overlay_id
            .map(|overlay_id| {
                state
                    .overlay_types
                    .flags(overlay_id)
                    .ok_or(LoadCellRecalcError::MissingOverlayType { overlay_id })
            })
            .transpose()?;
        let early_overlay_branch = source_flags.is_some_and(uses_early_recalc_land_branch);
        let mut finalized = overlay;
        let mut current_land_ground_blocked;
        let mut current_land_build_blocked;
        let mut current_cliff_eligible;
        let mut base_cliff_eligible = cliff_back_normal_reclass_applies(snapshot.base_land_type);
        let run_lat;
        let mut anim_request = None;
        let mut animation_tile_id = None;
        let mut animation_sub_tile = 0;
        let mut valid_entry = false;
        let mut sparse_entry = false;
        let mut retained_height_in_pixels = snapshot.height_in_pixels;

        if early_overlay_branch {
            let flags = source_flags.expect("early branch has OverlayType flags");
            apply_load_land_to_cell(
                &mut self.cells[index],
                flags.land,
                flags.land_speed_costs,
                flags.land_ground_blocked,
            );
            current_land_ground_blocked = flags.land_ground_blocked;
            current_land_build_blocked = flags.land_build_blocked;
            // `CellClass::RecalcAttributes @ 0x0047D2B0` re-reads the current
            // pristine TMP slope before the early overlay branch validates a
            // sloped resource. A registered sparse/missing TMP yields flat 0;
            // an unregistered tile has no proved replacement and retains the
            // cached receiver byte.
            let refreshed_slope = state
                .registered_tile_metadata(
                    snapshot.final_tile_index,
                    snapshot.final_sub_tile,
                    &snapshot,
                )?
                .map_or(snapshot.slope_type, |metadata| metadata.slope_type);
            {
                let cell = &mut self.cells[index];
                cell.slope_type = refreshed_slope;
                cell.has_ramp = refreshed_slope != 0;
                cell.canonical_ramp = canonical_ramp_from_slope_type(refreshed_slope);
            }
            if clears_tiberium_on_slope(flags, refreshed_slope) {
                finalized = FinalizedOverlayCell::default();
            }
            current_cliff_eligible = true;
            if state.cliff_back_impassability != 0 {
                let behind_cliff = self.authored_load_cell_is_behind_cliff(index);
                if behind_cliff && state.cliff_back_impassability == 2 {
                    if let Some((ground_blocked, build_blocked)) = self
                        .apply_authored_load_cliff_back_rock(
                            state,
                            index,
                            base_cliff_eligible,
                            current_cliff_eligible,
                        )
                    {
                        current_land_ground_blocked = ground_blocked;
                        current_land_build_blocked = build_blocked;
                    }
                }
            }
            run_lat = true;
        } else {
            let current_tile = snapshot.final_tile_index;
            let current_sub_tile = snapshot.final_sub_tile;
            let registered =
                state.registered_tile_metadata(current_tile, current_sub_tile, &snapshot)?;
            sparse_entry = registered
                .as_ref()
                .is_some_and(|metadata| metadata.subtile_entry_valid == Some(false));
            valid_entry = registered.is_some() && !sparse_entry;
            let metadata = if valid_entry {
                registered.expect("valid registered tile metadata")
            } else {
                state.clear_land_metadata()
            };
            let tile_id = u16::try_from(current_tile).ok();
            if valid_entry {
                // `RecalcAttributes @ 0x0047D2B0` retains the registered
                // pristine tile receiver for the later AnimType/AttachesTo
                // checks even though LAT may replace Cell+0x38 in between.
                animation_tile_id = tile_id;
                animation_sub_tile = current_sub_tile;
                retained_height_in_pixels = metadata.height_in_pixels;
            }
            let permissions = state
                .current_permissions(current_tile)
                .unwrap_or((false, false));
            let accepts_smudge = valid_entry && permissions.0;
            let allows_tiberium = valid_entry && permissions.1;
            {
                let cell = &mut self.cells[index];
                if !valid_entry {
                    cell.final_tile_index = 0xFFFF;
                    if sparse_entry {
                        cell.final_sub_tile = 0;
                    }
                }
                apply_pristine_load_metadata(cell, &metadata, accepts_smudge, allows_tiberium);
                // Invalid/sparse paths preserve11D; the valid path updates it
                // only after its possible automatic Tube constructor.
                cell.height_in_pixels = snapshot.height_in_pixels;
                restore_load_base_land(cell);
            }
            base_cliff_eligible = if sparse_entry {
                self.cells[index].base_land_type == LandType::Clear.as_index()
            } else {
                cliff_back_normal_reclass_applies(self.cells[index].base_land_type)
            };
            current_cliff_eligible = if sparse_entry {
                self.cells[index].land_type == LandType::Clear.as_index()
            } else {
                cliff_back_normal_reclass_applies(self.cells[index].land_type)
            };
            current_land_ground_blocked = self.cells[index].base_ground_walk_blocked;
            current_land_build_blocked = self.cells[index].base_build_blocked;
            run_lat = valid_entry;
        }

        if run_lat {
            self.apply_authored_load_lat_slope(state, index);
        }

        // These are cached current-tile queries, unlike retained native Cell
        // attributes. CanPlaceTiberium4839C0 and smudge6B5F80 resolve live+38.
        if let Some(permissions) = state.current_permissions(self.cells[index].final_tile_index) {
            let cell = &mut self.cells[index];
            (cell.accepts_smudge, cell.allows_tiberium) = permissions;
        }

        if !early_overlay_branch {
            // 47D83C/47D8A3 and47D967 use retained EBP after47CA80.
            // LAT changes+38 and ensures final TMP residency; it does not
            // replace the pristine land/slope/dimensions receiver. The Tube
            // predicate separately reads the current tile identity.
            let refreshed_slope = self.cells[index].slope_type;
            if valid_entry
                && let Some(flags) = source_flags
                && clears_tiberium_on_slope(flags, refreshed_slope)
            {
                finalized = FinalizedOverlayCell::default();
            }
            let live_flags = finalized
                .overlay_id()
                .and_then(|overlay_id| state.overlay_types.flags(overlay_id));
            let retained_land = live_flags.and_then(|flags| {
                if valid_entry {
                    retained_overlay_land(flags, refreshed_slope)
                } else if sparse_entry {
                    // 47D5EF goes directly to the Clear fallback tail.
                    None
                } else {
                    // 47DB22 preserves an ordinary overlay's Land on the
                    // invalid/sentinel path. Early overlays exited above.
                    Some(flags.land)
                }
            });
            if let (Some(flags), Some(land)) = (live_flags, retained_land) {
                apply_load_land_to_cell(
                    &mut self.cells[index],
                    land,
                    flags.land_speed_costs,
                    flags.land_ground_blocked,
                );
                current_land_ground_blocked = flags.land_ground_blocked;
                current_land_build_blocked = flags.land_build_blocked;
            } else {
                current_land_ground_blocked = self.cells[index].base_ground_walk_blocked;
                current_land_build_blocked = self.cells[index].base_build_blocked;
            }
            base_cliff_eligible = if sparse_entry {
                self.cells[index].base_land_type == LandType::Clear.as_index()
            } else {
                cliff_back_normal_reclass_applies(self.cells[index].base_land_type)
            };
            current_cliff_eligible = if sparse_entry {
                self.cells[index].land_type == LandType::Clear.as_index()
            } else {
                cliff_back_normal_reclass_applies(self.cells[index].land_type)
            };

            if valid_entry {
                let cell = &self.cells[index];
                if cell.yr_cell_land_type == YR_CELL_LAND_TUNNEL
                    && self.native_tube_indices[index]
                        .admits_automatic_construction(self.tube_facts.len())
                    && let Some(direction) = state.automatic_tube_direction(cell.final_tile_index)
                {
                    let request = AutomaticTubeRequest {
                        cell: (cell.rx, cell.ry),
                        direction,
                    };
                    match effects
                        .allocate_automatic_tube(request)
                        .map_err(LoadCellRecalcError::Effect)?
                    {
                        AutomaticTubeAllocation::AllocationNull => {}
                        AutomaticTubeAllocation::Allocated {
                            native_unique_id,
                            registry_append_allowed,
                        } => {
                            let appended = registry_append_allowed
                                && self.tube_facts.try_reserve(1).is_ok()
                                && self.tube_native_ids.try_reserve(1).is_ok();
                            if appended {
                                let ordinal = self.tube_facts.len();
                                self.tube_facts.push(TubeFact::automatic_recalc_shell(
                                    request.cell,
                                    request.direction,
                                ));
                                self.tube_native_ids.push(Some(native_unique_id));
                                if request.cell != (0, 0) {
                                    let raw = NativeTubeCellIndex::from_registry_ordinal(ordinal);
                                    self.native_tube_indices[index] = raw;
                                    self.cells[index].tube_index =
                                        raw.validated_id(self.tube_facts.len());
                                }
                            } else if request.cell != (0, 0) {
                                self.native_tube_indices[index] = NativeTubeCellIndex::NONE;
                                self.cells[index].tube_index = None;
                            }
                        }
                    }
                }

                if level_override != -1 {
                    self.cells[index].level = level_override as u8;
                }
                self.cells[index].height_in_pixels = retained_height_in_pixels;

                if !state.terrain_anim_is_latched(index)
                    && live_flags.is_none_or(|flags| !uses_early_recalc_land_branch(flags))
                    && let (Some(theater), Some(tile_id)) = (state.theater_data, animation_tile_id)
                    && let Some(anim) = theater.lookup.tile_anim(tile_id)
                    && anim.attaches_to == i32::from(animation_sub_tile)
                {
                    let (offset_x, offset_y) =
                        tile_anim_pixel_offset_to_leptons(anim.x_offset, anim.y_offset);
                    let cell = &self.cells[index];
                    anim_request = Some(TerrainTileAnimation {
                        rx: cell.rx,
                        ry: cell.ry,
                        anim_name: anim.anim_name.clone(),
                        world_x: offset_x
                            .wrapping_add(i32::from(cell.rx).wrapping_mul(LEPTONS_PER_CELL))
                            .wrapping_add(CELL_CENTRE_LEPTONS),
                        world_y: offset_y
                            .wrapping_add(i32::from(cell.ry).wrapping_mul(LEPTONS_PER_CELL))
                            .wrapping_add(CELL_CENTRE_LEPTONS),
                        world_z: i32::from(cell.level as i8)
                            .wrapping_mul(crate::util::lepton::GROUND_LEVEL_HEIGHT_LEPTONS),
                        z_adjust: anim.z_adjust,
                    });
                }
            }
        }

        if let Some(request) = anim_request.as_ref() {
            effects
                .construct_terrain_attached_anim(request)
                .map_err(LoadCellRecalcError::Effect)?;
            state.latch_terrain_anim(index);
        }

        if !early_overlay_branch && state.cliff_back_impassability != 0 {
            let behind_cliff = self.authored_load_cell_is_behind_cliff(index);
            if behind_cliff && state.cliff_back_impassability == 2 {
                if let Some((ground_blocked, build_blocked)) = self
                    .apply_authored_load_cliff_back_rock(
                        state,
                        index,
                        base_cliff_eligible,
                        current_cliff_eligible,
                    )
                {
                    current_land_ground_blocked = ground_blocked;
                    current_land_build_blocked = build_blocked;
                }
            }
        }

        self.finish_authored_load_cell_projection(
            state,
            index,
            finalized,
            current_land_ground_blocked,
            current_land_build_blocked,
        );
        let cell = &self.cells[index];
        let navigation_after = load_navigation_signature(cell);
        Ok(LoadCellRecalcOutcome {
            finalized,
            zone_before,
            zone_after: cell.zone_type,
            navigation_changed: navigation_before != navigation_after,
        })
    }

    fn apply_authored_load_lat_slope(&mut self, state: &LoadCellRecalcState<'_>, index: usize) {
        let Some(cell) = self.cells.get(index) else {
            return;
        };
        let x = cell.rx as i16;
        let y = cell.ry as i16;
        let tile = cell.final_tile_index;
        let slope = cell.slope_type;
        let cardinal_coords = [
            (x, y.wrapping_sub(1)),
            (x.wrapping_add(1), y),
            (x, y.wrapping_add(1)),
            (x.wrapping_sub(1), y),
        ];
        let lat_tile = state.lat_config.as_ref().map_or(tile, |lat_config| {
            lat::lat_fixed_tile_live(tile, lat_config, || {
                cardinal_coords.map(|(nx, ny)| self.authored_load_neighbor_tile(nx, ny))
            })
        });
        let final_tile = state.slope_config.map_or(lat_tile, |slope_config| {
            lat::slope_fixed_tile_live(lat_tile, slope, slope_config, |slot| {
                let (nx, ny) = cardinal_coords[slot];
                self.authored_load_neighbor_slope(nx, ny)
            })
        });
        self.cells[index].final_tile_index = final_tile;
    }

    fn authored_load_neighbor_tile(&mut self, x: i16, y: i16) -> i32 {
        if let Some(index) = self.native_fixed_cell_index(x, y) {
            self.cells[index].final_tile_index
        } else {
            self.shared_cell_dummy
                .stamp_coord(i32::from(x), i32::from(y));
            0xFFFF
        }
    }

    fn authored_load_neighbor_slope(&mut self, x: i16, y: i16) -> u8 {
        if let Some(index) = self.native_fixed_cell_index(x, y) {
            self.cells[index].slope_type
        } else {
            self.shared_cell_dummy
                .stamp_coord(i32::from(x), i32::from(y));
            self.shared_cell_dummy.snapshot().slope_type
        }
    }

    fn authored_load_cell_is_behind_cliff(&self, index: usize) -> bool {
        const OFFSETS: [(i16, i16); 6] = [(0, -1), (-1, 0), (2, 2), (1, 1), (-1, 1), (1, -1)];
        let cell = &self.cells[index];
        let x = cell.rx as i16;
        let y = cell.ry as i16;
        let threshold = i16::from(cell.level as i8).wrapping_add(4);
        for (dx, dy) in OFFSETS {
            let nx = x.wrapping_add(dx);
            let ny = y.wrapping_add(dy);
            let neighbor_level = if let Some(neighbor_index) = self.native_fixed_cell_index(nx, ny)
            {
                self.cells[neighbor_index].level as i8
            } else {
                self.shared_cell_dummy
                    .stamp_coord(i32::from(nx), i32::from(ny));
                self.shared_cell_dummy.snapshot().level
            };
            if i16::from(neighbor_level) >= threshold {
                return true;
            }
        }
        false
    }

    fn apply_authored_load_cliff_back_rock(
        &mut self,
        state: &LoadCellRecalcState<'_>,
        index: usize,
        apply_base: bool,
        apply_current: bool,
    ) -> Option<(bool, bool)> {
        if state.cliff_back_impassability != 2 || (!apply_base && !apply_current) {
            return None;
        }
        let rock = state
            .terrain_rules
            .and_then(|rules| rules.semantics_for_land_type(LandType::Rock.as_index()))
            .copied();
        let cell = &mut self.cells[index];
        let terrain_class = rock.map_or(LandType::Rock.terrain_class(), |row| row.terrain_class);
        let mut speed_costs = rock.map_or_else(SpeedCostProfile::default, |row| row.speed_costs);
        if rock.is_none() {
            speed_costs.wheel = Some(0);
        }
        let ground_blocked = rock.is_none_or(|row| row.ground_blocked);
        let build_blocked = rock.is_none_or(|row| !row.buildable);
        if apply_base {
            cell.base_land_type = LandType::Rock.as_index();
            cell.base_yr_cell_land_type = LandType::Rock.as_index();
            cell.base_terrain_class = terrain_class;
            cell.base_speed_costs = speed_costs;
            cell.base_ground_walk_blocked = ground_blocked;
            cell.base_build_blocked = build_blocked;
        }
        if apply_current {
            cell.land_type = LandType::Rock.as_index();
            cell.yr_cell_land_type = LandType::Rock.as_index();
            cell.terrain_class = terrain_class;
            cell.speed_costs = speed_costs;
            cell.is_water = false;
            cell.is_cliff_like = true;
            cell.is_rough = false;
            cell.is_road = false;
        }
        apply_current.then_some((ground_blocked, build_blocked))
    }

    fn finish_authored_load_cell_projection(
        &mut self,
        state: &LoadCellRecalcState<'_>,
        index: usize,
        finalized: FinalizedOverlayCell,
        land_ground_blocked: bool,
        land_build_blocked: bool,
    ) {
        let overlay_id = finalized.overlay_id();
        let flags = overlay_id.and_then(|overlay_id| state.overlay_types.flags(overlay_id));
        let overlay_zone_type = overlay_reduced_zone_type(flags);
        let overlay_blocks = matches!(
            overlay_zone_type,
            Some(zone_class::WALL) | Some(zone_class::IMPASSABLE)
        );
        let overlay_name = overlay_id
            .and_then(|overlay_id| state.overlay_types.name(overlay_id))
            .unwrap_or("");
        let cell = &mut self.cells[index];
        cell.overlay_zone_type = overlay_zone_type;
        cell.overlay_blocks = overlay_blocks;
        cell.bridge_layer = overlay_id.and_then(|overlay_id| {
            crate::map::overlay_types::is_bridge_overlay_index(overlay_id).then(|| {
                let direction = match overlay_id {
                    24 | 237 => BridgeDirection::EastWest,
                    25 | 238 => BridgeDirection::NorthSouth,
                    _ => BridgeDirection::Low,
                };
                BridgeLayer {
                    overlay_id,
                    overlay_name: overlay_name.to_string(),
                    deck_level: match direction {
                        BridgeDirection::Low => cell.level,
                        BridgeDirection::EastWest | BridgeDirection::NorthSouth => {
                            cell.level.saturating_add(4)
                        }
                    },
                    direction,
                }
            })
        });
        let identity_bridge = cell.bridge_layer.is_some();
        let structural_bridge = cell.bridge_facts.has_structural_bridge();
        cell.has_bridge_deck = identity_bridge || structural_bridge;
        cell.refresh_current_height_projection();
        cell.bridge_walkable =
            structural_bridge && !cell.terrain_object_blocks && !cell.overlay_blocks;
        cell.bridge_transition = cell.bridge_facts.has_transition_flag();
        cell.ground_walk_blocked =
            land_ground_blocked || cell.terrain_object_blocks || cell.overlay_blocks;
        cell.build_blocked = land_build_blocked
            || cell.terrain_object_blocks
            || cell.overlay_blocks
            || cell.has_bridge_deck;
        cell.outside_playfield = state.playfield.is_some_and(|playfield| {
            !playfield.contains_raised(cell.rx, cell.ry, cell.level as i8, cell.slope_type)
        });
        cell.zone_type = recalc_zone_type(
            cell.outside_playfield,
            cell.overlay_zone_type,
            cell.land_type,
            cell.speed_costs.wheel,
            cell.terrain_object_occupation,
        );
        cell.bridge_facts.overlay_id = overlay_id;
        cell.bridge_facts.state_byte = finalized.state();
        cell.is_wood_bridge_repair_tile = state.wood_bridge_repair_tile(cell.final_tile_index);
    }

    /// Borrow the sparse native CellClass allocation plane used by fixed-grid
    /// map lookups. Load-local owners may snapshot this once so later overlay
    /// writes do not need to keep an immutable borrow of the live terrain grid.
    pub(crate) fn native_allocation_mask(&self) -> Option<&[bool]> {
        self.native_allocated.as_deref()
    }

    pub(crate) fn bridge_flag_execution_state(&self) -> CellClassBridgeFlagState {
        CellClassBridgeFlagState::from_grid(self)
    }

    pub(crate) fn bind_shared_cell_dummy(&mut self, shared_cell_dummy: SharedCellDummy) {
        self.shared_cell_dummy = shared_cell_dummy;
    }

    /// Apply one represented runtime `SetBridgeDirection_*` flag transaction
    /// through the same real-or-dummy seam used by map load.
    pub(crate) fn apply_runtime_bridge_flag_stamp(
        &mut self,
        stamp: BridgeFlagStamp,
    ) -> Vec<(usize, u32)> {
        apply_native_bridge_flag_stamp_to_parts(
            &mut self.cells,
            self.width,
            self.height,
            self.native_allocated.as_deref(),
            &self.shared_cell_dummy,
            stamp,
            None,
        )
    }

    /// Apply one authored high-bridge setter slot in native call order. The
    /// caller synchronizes the slot's `Cell+0x11E` write on the live overlay
    /// surface immediately after this flag/fact mutation, without repeating a
    /// fixed-grid lookup or restamping the shared dummy.
    pub(crate) fn apply_authored_bridge_flag_slot(
        &mut self,
        anchor: NativeCellIdentity,
        stamp: BridgeFlagStamp,
        family: crate::map::bridge_facts::BridgeStampFamily,
        slot: crate::map::bridge_facts::BridgeStampSlot,
        requested: Option<(i32, i32)>,
    ) -> Option<usize> {
        let (x, y) = requested?;
        if let Some(index) = native_resolved_cell_index(
            self.width,
            self.height,
            self.native_allocated.as_deref(),
            self.cells.len(),
            x,
            y,
        ) {
            crate::map::bridge_facts::apply_bridge_fact_slot(
                &mut self.cells[index].bridge_facts,
                slot,
                BridgeAnchorRelation {
                    anchor: (stamp.anchor.0 as u16, stamp.anchor.1 as u16),
                    slot,
                    family,
                    direction: stamp.direction,
                },
                stamp.set,
            );
            if slot.writes_native_anchor() {
                self.cells[index].bridge_facts.native_anchor = stamp.set.then_some(anchor);
            }
            Some(index)
        } else {
            self.shared_cell_dummy.stamp_coord(x, y);
            self.shared_cell_dummy
                .apply_full_bridge_flag_slot(slot, stamp, family, anchor);
            None
        }
    }

    /// Apply the high-anchor `OverlayClass::Mark` setter and refresh the
    /// Rust-native bridge projection carried beside the literal CellClass
    /// flag word. Unlike damage/repair runtime setters, a newly constructed
    /// anchor establishes its family/anchor relations at this call site.
    pub(crate) fn apply_runtime_bridge_mark_stamp(
        &mut self,
        stamp: BridgeFlagStamp,
        family: BridgeStampFamily,
    ) -> Vec<(usize, u32)> {
        let updates = apply_native_bridge_flag_stamp_to_parts(
            &mut self.cells,
            self.width,
            self.height,
            self.native_allocated.as_deref(),
            &self.shared_cell_dummy,
            stamp,
            Some(family),
        );
        for &(index, _) in &updates {
            if let Some(cell) = self.cells.get_mut(index) {
                refresh_runtime_bridge_projection(cell, false);
            }
        }
        updates
    }

    /// Project a direct runtime overlay-field write into the map-owned bridge
    /// cache. The literal identity/data remain owned by OverlayGrid; this is
    /// only the derived bridge metadata that pathing and bridge damage read.
    pub(crate) fn set_runtime_overlay_bridge_identity(
        &mut self,
        rx: u16,
        ry: u16,
        overlay_id: u8,
        overlay_data: u8,
        overlay_name: &str,
    ) {
        let Some(cell) = self.cell_mut(rx, ry) else {
            return;
        };
        cell.bridge_facts.overlay_id = Some(overlay_id);
        cell.bridge_facts.state_byte = overlay_data;
        if !crate::map::overlay_types::is_bridge_overlay_index(overlay_id) {
            return;
        }

        let direction = match overlay_id {
            0x18 | 0xED => BridgeDirection::EastWest,
            0x19 | 0xEE => BridgeDirection::NorthSouth,
            _ => BridgeDirection::Low,
        };
        let deck_level = match direction {
            BridgeDirection::EastWest | BridgeDirection::NorthSouth => cell.level.saturating_add(4),
            BridgeDirection::Low => cell.level,
        };
        cell.bridge_layer = Some(BridgeLayer {
            overlay_id,
            overlay_name: overlay_name.to_string(),
            deck_level,
            direction,
        });
        cell.has_bridge_deck = true;
        cell.bridge_walkable = direction != BridgeDirection::Low
            && !cell.terrain_object_blocks
            && !cell.overlay_blocks;
        cell.bridge_deck_level = deck_level;
        cell.build_blocked = cell.base_build_blocked
            || cell.terrain_object_blocks
            || cell.overlay_blocks
            || cell.has_bridge_deck;
    }

    /// Project a direct `CellClass::OverlayData` write into the derived bridge
    /// facts without inventing an overlay identity. The native high-bridge
    /// setters write this byte on four cells before `OverlayClass::Mark`
    /// decides whether the anchor itself receives an overlay.
    pub(crate) fn set_runtime_overlay_bridge_state_byte(
        &mut self,
        rx: u16,
        ry: u16,
        overlay_data: u8,
    ) -> bool {
        let Some(cell) = self.cell_mut(rx, ry) else {
            return false;
        };
        cell.bridge_facts.state_byte = overlay_data;
        true
    }

    /// Project the two literal field writes a crate removal performs.
    ///
    /// `CrateSlot__RemoveCrateOverlayFromCell @ 0x004A1AA0` stores
    /// `CellClass+0x44 = -1` at `0x004A1BE1` and `CellClass+0x11E = 0` at
    /// `0x004A1BE8` and touches nothing else — no `RecalcAttributes` tail and
    /// no bridge setter. The derived bridge layer/deck projection therefore
    /// stays as it is: the random placer only admits cells that already carry
    /// no overlay, so a removal can never be undoing a bridge identity in
    /// stock rules.
    pub(crate) fn clear_runtime_overlay_identity(&mut self, rx: u16, ry: u16) -> bool {
        let Some(cell) = self.cell_mut(rx, ry) else {
            return false;
        };
        cell.bridge_facts.overlay_id = None;
        cell.bridge_facts.state_byte = 0;
        true
    }

    /// Project only allocated real-cell values for a setter that already ran
    /// synchronously through [`CellClassBridgeFlagState`]. This performs no
    /// GetCell fallback and cannot stamp or mutate the shared dummy.
    pub(crate) fn apply_planned_bridge_flag_stamp_to_real_cells(
        &mut self,
        stamp: BridgeFlagStamp,
    ) -> Vec<(usize, u32)> {
        apply_planned_bridge_flag_stamp_to_real_parts(
            &mut self.cells,
            self.width,
            self.height,
            self.native_allocated.as_deref(),
            stamp,
        )
    }

    pub(crate) fn capture_real_cell_bridge_flags_0x1180(&self) -> RealCellBridgeFlags0x1180 {
        RealCellBridgeFlags0x1180::from_grid(self)
    }

    /// One original586BF0 write through packed fixed lookup. Missing slots
    /// publish their requested coordinate and mutate the retained shared dummy.
    /// Other real flag bits are preserved; no topology/zone rebuild is implied.
    pub(crate) fn apply_bridge_gap_flag_write(
        &mut self,
        coord: (i16, i16),
        horizontal: bool,
    ) -> (Option<usize>, u32) {
        let update = |flags: u32| {
            if horizontal {
                flags | 0xC00
            } else {
                (flags & !0x800) | 0x400
            }
        };
        if let Some(index) = self.native_fixed_cell_index(coord.0, coord.1) {
            let flags = &mut self.cells[index].bridge_facts.raw_flags;
            *flags = update(*flags);
            (Some(index), *flags)
        } else {
            self.shared_cell_dummy
                .stamp_coord(i32::from(coord.0), i32::from(coord.1));
            self.shared_cell_dummy.update_retained_bridge_flags(update);
            (None, self.shared_cell_dummy.retained_bridge_flags())
        }
    }

    pub(crate) fn bridge_flag_authority_matches_shape(
        &self,
        authority: &RealCellBridgeFlags0x1180,
    ) -> bool {
        authority.matches_grid_shape(self)
    }

    /// Restore serialized real CellClass values by aligned direct writes.
    /// This deliberately never performs a MapClass lookup or setter call, so
    /// native-unallocated slots cannot stamp or mutate the shared dummy.
    pub(crate) fn restore_real_cell_bridge_flags_0x1180(
        &mut self,
        authority: &RealCellBridgeFlags0x1180,
    ) -> bool {
        if !authority.matches_grid_shape(self)
            || self
                .native_allocated
                .as_ref()
                .is_some_and(|mask| mask.len() != self.cells.len())
        {
            return false;
        }

        for (index, &saved) in authority.flags_or_unallocated.iter().enumerate() {
            let allocated = self
                .native_allocated
                .as_deref()
                .is_none_or(|mask| mask[index]);
            if allocated == (saved == UNALLOCATED_REAL_CELL_BRIDGE_FLAGS)
                || (saved != UNALLOCATED_REAL_CELL_BRIDGE_FLAGS
                    && u32::from(saved) & !RETAINED_CELLCLASS_BRIDGE_FLAG_MASK != 0)
            {
                return false;
            }
        }

        for (cell, &saved) in self.cells.iter_mut().zip(&authority.flags_or_unallocated) {
            if saved != UNALLOCATED_REAL_CELL_BRIDGE_FLAGS {
                cell.bridge_facts.raw_flags = (cell.bridge_facts.raw_flags
                    & !RETAINED_CELLCLASS_BRIDGE_FLAG_MASK)
                    | u32::from(saved);
            }
        }
        true
    }

    /// One stamping lookup for FNPC's level-plus-flags projection probe.
    ///
    /// Native `MapClass::Get_CellClass @ 0x005657A0` returns one real CellClass
    /// or stamps `+0x24` on the process-global dummy before returning that same
    /// live object (`0x005657C8..0x005657D5`). `FUN_006D6410` then reads signed
    /// level `+0x11B` and, conditionally, flags `+0x140` from that one result.
    pub(crate) fn cellclass_projection_view(&self, x: i32, y: i32) -> CellClassProjectionView {
        let (x, y) = crate::map::cell_index::packed_cell_coord(x, y);
        if let Some(index) = native_resolved_cell_index(
            self.width,
            self.height,
            self.native_allocated.as_deref(),
            self.cells.len(),
            x,
            y,
        ) {
            let cell = &self.cells[index];
            return CellClassProjectionView {
                signed_level: i32::from(cell.level as i8),
                raw_flags_0x1180: cell.bridge_facts.raw_flags & MODELED_CELLCLASS_BRIDGE_FLAG_MASK,
            };
        }
        self.shared_cell_dummy.stamp_coord(x, y);
        let dummy = self.shared_cell_dummy.snapshot();
        CellClassProjectionView {
            signed_level: i32::from(dummy.level),
            raw_flags_0x1180: dummy.bridge_flags_0x1180,
        }
    }

    /// Stamping lookup view for later CellClass flag-only consumers.
    /// A miss updates the dummy coordinate before exposing its live `0x1180`.
    pub(crate) fn cellclass_bridge_flags_0x1180(&self, x: i32, y: i32) -> u32 {
        self.cellclass_projection_view(x, y).raw_flags_0x1180
    }

    pub(crate) fn dummy_cell_level_slope(&self) -> (i8, u8) {
        let snapshot = self.shared_cell_dummy.snapshot();
        (snapshot.level, snapshot.slope_type)
    }

    pub(crate) fn dummy_cell_requested_coord(&self) -> (i32, i32) {
        self.shared_cell_dummy.snapshot().coord
    }

    /// Stamp only the coordinate words of the shared fallback cell.
    ///
    /// Verified against `MapClass::Get_CellClass @ 0x005657A0` and the
    /// world/lepton overload at `0x00565730`: both miss paths overwrite
    /// CellClass+0x24 but preserve the independently mutable level/slope bytes.
    pub(crate) fn stamp_dummy_cell_requested_coord(&self, x: i32, y: i32) {
        self.shared_cell_dummy.stamp_coord(x, y);
    }

    /// Native `MapClass` allocation probe without dummy fallback side effects.
    ///
    /// `MapClass::Is_Cell_Allocated @ 0x005657E0` computes the signed packed
    /// `y*512+x` slot and directly tests its pointer. Its active save-table and
    /// EMP callers establish valid coordinates; Rust therefore returns `false`
    /// outside the fixed array instead of reproducing native out-of-array UB.
    pub(crate) fn cellclass_allocation_probe(&self, x: i32, y: i32) -> bool {
        let Some((rx, ry)) = crate::map::cell_index::canonical_cell_coord(x, y) else {
            return false;
        };
        self.cell(rx, ry).is_some()
    }

    /// Update the shared fallback level without exposing an unsupported runtime
    /// slope writer.
    #[cfg(test)]
    pub(crate) fn set_dummy_cell_level(&self, level: i8) {
        self.shared_cell_dummy.set_level(level);
    }

    #[cfg(test)]
    pub(crate) fn test_set_dummy_cell_level_slope(&self, level: i8, slope_type: u8) {
        self.shared_cell_dummy.set_level_slope(level, slope_type);
    }

    #[cfg(test)]
    pub(crate) fn test_set_native_allocated_cells(&mut self, allocated: &[(u16, u16)]) {
        let mut mask = vec![false; usize::from(self.width) * usize::from(self.height)];
        for &(rx, ry) in allocated {
            if rx < self.width && ry < self.height {
                mask[usize::from(ry) * usize::from(self.width) + usize::from(rx)] = true;
            }
        }
        self.native_allocated = Some(mask);
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

    pub(crate) fn is_destroyable_cliff(&self, rx: u16, ry: u16) -> bool {
        let Some(catalog) = self.destroyable_cliff_catalog.as_ref() else {
            return false;
        };
        self.cell(rx, ry).is_some_and(|cell| {
            cell.final_tile_index == i32::from(catalog.destroyable_start)
                || cell.final_tile_index == i32::from(catalog.destroyable_start) + 1
        })
    }

    #[cfg(test)]
    pub(crate) fn test_install_destroyable_cliff_catalog(&mut self, destroyable_start: u16) {
        fn prototype(template_height: u8, slope_type: u8) -> DynamicTilePrototype {
            DynamicTilePrototype {
                metadata: TileMetadata {
                    tileset_index: Some(1),
                    land_type: LandType::Clear.as_index(),
                    yr_cell_land_type: LandType::Clear.as_index(),
                    slope_type,
                    template_height,
                    terrain_class: TerrainClass::Clear,
                    radar_left: [slope_type, 1, 2],
                    radar_right: [slope_type, 3, 4],
                    ..TileMetadata::default()
                },
                accepts_smudge: true,
                allows_tiberium: false,
            }
        }
        fn template(
            tile_id: u16,
            width: u8,
            height: u8,
            template_height: u8,
            slope_type: u8,
            holes: &[usize],
        ) -> SparseTileTemplate {
            let mut entries = vec![
                Some(prototype(template_height, slope_type));
                usize::from(width) * usize::from(height)
            ];
            for &index in holes {
                entries[index] = None;
            }
            SparseTileTemplate {
                tile_id,
                width,
                height,
                entries,
            }
        }
        self.destroyable_cliff_catalog = Some(DestroyableCliffCatalog {
            destroyable_start,
            old: [
                template(destroyable_start, 6, 4, 1, 0, &[0, 5, 18, 23]),
                template(destroyable_start + 1, 4, 6, 1, 0, &[0, 3, 20, 23]),
            ],
            replacements: [
                template(destroyable_start + 100, 3, 4, 1, 1, &[0, 9]),
                template(destroyable_start + 101, 3, 4, 1, 2, &[2, 11]),
                template(destroyable_start + 102, 4, 3, 1, 3, &[8, 11]),
                template(destroyable_start + 103, 4, 3, 1, 4, &[0, 3]),
            ],
            lat_config: lat::LatConfig {
                grounds: Vec::new(),
            },
            slope_config: lat::SlopeFixupConfig {
                ramp_base: -1,
                ramp_smooth: -1,
            },
        });
    }

    /// gamemd-derived: `MapClass::CollapseDestroyableCliff @ 0x00581140`
    /// terrain half. The world owner surrounds this with zones, detach/dirty,
    /// and AnimClass construction so all non-grid authorities remain ordered.
    pub(crate) fn collapse_destroyable_cliff_terrain(
        &mut self,
        rx: u16,
        ry: u16,
        mut clear_replacement_cell: impl FnMut(u16, u16),
    ) -> Option<DestroyableCliffMutation> {
        let catalog = self.destroyable_cliff_catalog.clone()?;
        let selected = self.cell(rx, ry)?;
        let family = match selected.final_tile_index {
            tile if tile == i32::from(catalog.destroyable_start) => DestroyableCliffFamily::A,
            tile if tile == i32::from(catalog.destroyable_start) + 1 => DestroyableCliffFamily::B,
            _ => return None,
        };
        let old = &catalog.old[match family {
            DestroyableCliffFamily::A => 0,
            DestroyableCliffFamily::B => 1,
        }];
        let sub_tile = selected.final_sub_tile;
        let (sub_x, sub_y) = match family {
            DestroyableCliffFamily::A => (sub_tile % 6, sub_tile / 6),
            DestroyableCliffFamily::B => (sub_tile & 3, sub_tile >> 2),
        };
        let origin = (
            (rx as i16).wrapping_sub(i16::from(sub_x)),
            (ry as i16).wrapping_sub(i16::from(sub_y)),
        );

        let coord_for = |x: u8, y: u8| -> Option<(u16, u16)> {
            let x = origin.0.wrapping_add(i16::from(x));
            let y = origin.1.wrapping_add(i16::from(y));
            let (Ok(x), Ok(y)) = (u16::try_from(x), u16::try_from(y)) else {
                return None;
            };
            Some((x, y))
        };
        let mut original_footprint = Vec::new();
        for y in 0..old.height {
            for x in 0..old.width {
                let index = usize::from(y) * usize::from(old.width) + usize::from(x);
                let Some(prototype) = old.entries.get(index).and_then(Option::as_ref) else {
                    continue;
                };
                let Some(coord) = coord_for(x, y) else {
                    continue;
                };
                if !self
                    .cell(coord.0, coord.1)
                    .is_some_and(|cell| !cell.outside_playfield)
                {
                    continue;
                }
                original_footprint.push(coord);
                if let Some(cell) = self.cell_mut(coord.0, coord.1)
                    && cell.final_tile_index == i32::from(old.tile_id)
                    && usize::from(cell.final_sub_tile) == index
                {
                    cell.final_tile_index = 0xFFFF;
                    cell.final_sub_tile = 0;
                    cell.level = (cell.level as i8)
                        .wrapping_sub(prototype.metadata.template_height as i8)
                        as u8;
                }
            }
        }

        let stamps: [(usize, (i16, i16)); 2] = match family {
            DestroyableCliffFamily::A => [(0, origin), (1, (origin.0.wrapping_add(3), origin.1))],
            DestroyableCliffFamily::B => [(3, origin), (2, (origin.0, origin.1.wrapping_add(3)))],
        };
        let mut changed_cells = Vec::new();
        for (template_index, anchor) in stamps {
            let template = &catalog.replacements[template_index];
            for y in 0..template.height {
                for x in 0..template.width {
                    let index = usize::from(y) * usize::from(template.width) + usize::from(x);
                    let Some(prototype) = template.entries.get(index).and_then(Option::as_ref)
                    else {
                        continue;
                    };
                    let x = anchor.0.wrapping_add(i16::from(x));
                    let y = anchor.1.wrapping_add(i16::from(y));
                    let (Ok(x), Ok(y)) = (u16::try_from(x), u16::try_from(y)) else {
                        continue;
                    };
                    if !self.cell(x, y).is_some_and(|cell| !cell.outside_playfield) {
                        continue;
                    }
                    let cell_index = self.index(x, y).expect("validated real cell");
                    clear_replacement_cell(x, y);
                    stamp_dynamic_tile_identity(
                        &mut self.cells[cell_index],
                        template.tile_id,
                        index as u8,
                        prototype,
                    );
                    self.apply_runtime_lat_slope(x as i16, y as i16, &catalog, &mut changed_cells);
                    for (dx, dy) in [(0i16, -1i16), (1, 0), (0, 1), (-1, 0)] {
                        self.apply_runtime_lat_slope(
                            (x as i16).wrapping_add(dx),
                            (y as i16).wrapping_add(dy),
                            &catalog,
                            &mut changed_cells,
                        );
                    }
                    recalc_dynamic_tile_attributes(&mut self.cells[cell_index], prototype);
                    self.radar_color_valid[cell_index] = true;
                    self.damaged_radar_metadata[cell_index] = None;
                    if !changed_cells.contains(&(x, y)) {
                        changed_cells.push((x, y));
                    }
                }
            }
        }

        let (anim_width, anim_height) = match family {
            DestroyableCliffFamily::A => (5u8, 3u8),
            DestroyableCliffFamily::B => (3u8, 5u8),
        };
        let mut animation_cells = Vec::with_capacity(15);
        for y in 0..anim_height {
            for x in 0..anim_width {
                let x = origin.0.wrapping_add(i16::from(x));
                let y = origin.1.wrapping_add(i16::from(y));
                animation_cells.push((x, y));
            }
        }
        Some(DestroyableCliffMutation {
            family,
            origin,
            original_footprint,
            animation_cells,
            changed_cells,
        })
    }

    fn apply_runtime_lat_slope(
        &mut self,
        x: i16,
        y: i16,
        catalog: &DestroyableCliffCatalog,
        changed_cells: &mut Vec<(u16, u16)>,
    ) {
        let (Ok(rx), Ok(ry)) = (u16::try_from(x), u16::try_from(y)) else {
            self.shared_cell_dummy
                .stamp_coord(i32::from(x), i32::from(y));
            return;
        };
        let Some(index) = self.index(rx, ry) else {
            self.shared_cell_dummy
                .stamp_coord(i32::from(x), i32::from(y));
            return;
        };
        if self
            .native_allocated
            .as_ref()
            .is_some_and(|allocated| !allocated.get(index).copied().unwrap_or(false))
            || self.cells[index].outside_playfield
        {
            self.shared_cell_dummy
                .stamp_coord(i32::from(x), i32::from(y));
            return;
        }
        let cardinal_coords = [
            (x, y.wrapping_sub(1)),
            (x.wrapping_add(1), y),
            (x, y.wrapping_add(1)),
            (x.wrapping_sub(1), y),
        ];
        let cardinal_tiles = cardinal_coords.map(|(nx, ny)| {
            u16::try_from(nx)
                .ok()
                .zip(u16::try_from(ny).ok())
                .and_then(|(nx, ny)| self.cell(nx, ny))
                .map_or(0, |cell| match cell.final_tile_index {
                    tile if tile < 0 || tile == 0xFFFF => 0,
                    tile => tile,
                })
        });
        let cardinal_slopes = cardinal_coords.map(|(nx, ny)| {
            u16::try_from(nx)
                .ok()
                .zip(u16::try_from(ny).ok())
                .and_then(|(nx, ny)| self.cell(nx, ny))
                .map_or(0, |cell| cell.slope_type)
        });
        let old_tile = self.cells[index].final_tile_index;
        let lat_tile = lat::lat_fixed_tile(old_tile, cardinal_tiles, &catalog.lat_config);
        let new_tile = lat::slope_fixed_tile(
            lat_tile,
            self.cells[index].slope_type,
            cardinal_slopes,
            catalog.slope_config,
        );
        if new_tile != old_tile {
            self.cells[index].final_tile_index = new_tile;
            if !changed_cells.contains(&(rx, ry)) {
                changed_cells.push((rx, ry));
            }
        }
    }

    /// CellClass lookup used by the collapse animation producer. Invalid
    /// signed coordinates resolve through the one shared dummy; the caller
    /// still retains the signed coordinate for the world X/Y calculation.
    pub(crate) fn collapse_animation_level(&self, x: i16, y: i16) -> i8 {
        let cell = u16::try_from(x)
            .ok()
            .zip(u16::try_from(y).ok())
            .and_then(|(x, y)| self.cell(x, y));
        if let Some(cell) = cell {
            cell.level as i8
        } else {
            self.shared_cell_dummy
                .stamp_coord(i32::from(x), i32::from(y));
            self.dummy_cell_level_slope().0
        }
    }

    pub fn iter(&self) -> impl Iterator<Item = &ResolvedTerrainCell> {
        self.cells.iter().enumerate().filter_map(|(index, cell)| {
            self.native_allocated
                .as_ref()
                .is_none_or(|mask| mask.get(index).copied().unwrap_or(false))
                .then_some(cell)
        })
    }

    /// `CellIterator_Init @ 0x00578350` / `Next @ 0x00578290`: advance
    /// anti-diagonals and stop at the first null pointer, without dummy lookup.
    /// The signed Size width is independent of rectangular storage dimensions.
    pub(crate) fn native_cell_iterator(
        &self,
        size_width: i32,
    ) -> impl Iterator<Item = &ResolvedTerrainCell> {
        let (mut x, mut y, mut remaining) = (1i32, size_width, size_width.wrapping_sub(1));
        let mut ended = false;
        std::iter::from_fn(move || {
            if ended {
                return None;
            }
            let index = y.wrapping_mul(512).wrapping_add(x);
            let cell = (0..0x40000)
                .contains(&index)
                .then(|| self.cell((index % 512) as u16, (index / 512) as u16))
                .flatten();
            if remaining != 0 {
                x = x.wrapping_add(1);
                y = y.wrapping_sub(1);
                remaining = remaining.wrapping_sub(1);
            } else {
                (x, y) = (y, x);
                if x.wrapping_sub(size_width).wrapping_sub(1).wrapping_add(y) & 1 == 0 {
                    remaining = size_width.wrapping_sub(2);
                    x = x.wrapping_add(1);
                } else {
                    remaining = size_width.wrapping_sub(1);
                    y = y.wrapping_add(1);
                }
            }
            ended = cell.is_none();
            cell
        })
    }

    /// Recompute the playfield-derived part of every live CellClass cache.
    ///
    /// `FUN_006E21E0`, reached by TriggerAction kind 0x28 from
    /// `TriggerAction__Execute @ 0x006DD8B0`, runs
    /// `CellClass::RecalcAttributes(-1) @ 0x0047D2B0` over all cells after the
    /// LocalSize writer. Rust stores the two affected derived facts directly:
    /// `outside_playfield` and the reduced `zone_type` that consumes it.
    /// Other RecalcAttributes inputs did not change, so rewriting them would
    /// manufacture unrelated runtime behavior.
    pub(crate) fn recalc_playfield_attributes(
        &mut self,
        bounds: PlayfieldBounds,
    ) -> Vec<(u16, u16)> {
        let (cells, native_allocated) = (&mut self.cells, &self.native_allocated);
        let mut refreshed = Vec::with_capacity(cells.len());
        for (index, cell) in cells.iter_mut().enumerate() {
            if native_allocated
                .as_ref()
                .is_some_and(|mask| !mask.get(index).copied().unwrap_or(false))
            {
                continue;
            }
            let outside = !bounds.contains_height_aware_packed(
                i32::from(cell.rx),
                i32::from(cell.ry),
                cell.level as i8,
                cell.slope_type,
            );
            cell.outside_playfield = outside;
            cell.zone_type = recalc_zone_type(
                outside,
                cell.overlay_zone_type,
                cell.land_type,
                cell.speed_costs.wheel,
                cell.terrain_object_occupation,
            );
            refreshed.push((cell.rx, cell.ry));
        }
        refreshed
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

    pub fn tube_facts(&self) -> &[TubeFact] {
        &self.tube_facts
    }

    /// Consume the successful raw `[Tubes]` constructor receipt into the one
    /// live registry before authored OverlayClass Mark/Recalc begins.
    pub(crate) fn bind_native_map_tubes(
        &mut self,
        receipt: NativeMapTubeReceipt,
    ) -> Result<usize, NativeTubeRegistryBindError> {
        if !self.tube_facts.is_empty() || !self.tube_native_ids.is_empty() {
            return Err(NativeTubeRegistryBindError::RegistryAlreadyPopulated);
        }
        let entries = receipt.entries.len();
        self.tube_facts
            .try_reserve(entries)
            .map_err(|_| NativeTubeRegistryBindError::RegistryCapacity { entries })?;
        self.tube_native_ids
            .try_reserve(entries)
            .map_err(|_| NativeTubeRegistryBindError::RegistryCapacity { entries })?;

        for constructed in receipt.entries {
            let source_ordinal = constructed.native_init.source_entry_ordinal;
            let native_unique_id = constructed.native_init.native_unique_id;
            let entry = constructed.fact.entry;
            self.tube_facts.push(constructed.fact);
            self.tube_native_ids.push(Some(native_unique_id));

            let x = entry.0 as i16;
            let y = entry.1 as i16;
            if let Some(index) = self.native_fixed_cell_index(x, y) {
                let raw = NativeTubeCellIndex::from_registry_ordinal(source_ordinal);
                self.native_tube_indices[index] = raw;
                self.cells[index].tube_index = raw.validated_id(self.tube_facts.len());
            } else {
                self.shared_cell_dummy
                    .stamp_coord(i32::from(x), i32::from(y));
                // ReadTubesINI728515 writes through the returned pointer,
                // including the one retained shared dummy CellClass.
                self.shared_cell_dummy
                    .write_raw_tube_index(source_ordinal as i16);
            }
        }
        Ok(entries)
    }

    #[cfg(test)]
    pub(crate) fn tube_native_id(&self, tube_id: TubeId) -> Option<i32> {
        self.tube_native_ids
            .get(tube_id.as_usize())
            .copied()
            .flatten()
    }

    pub fn tube(&self, tube_id: TubeId) -> Option<&TubeFact> {
        self.tube_facts.get(tube_id.as_usize())
    }

    pub fn tube_at_cell(&self, rx: u16, ry: u16) -> Option<&TubeFact> {
        let index = self.index(rx, ry)?;
        let tube_id = self
            .native_tube_indices
            .get(index)
            .copied()?
            .validated_id(self.tube_facts.len())?;
        self.tube(tube_id)
    }

    /// Get_CellClass5657A0 followed by the raw Cell+116 read used by429780.
    pub(crate) fn raw_tube_index_at_native_coord(&self, coord: (u16, u16)) -> i16 {
        let (x, y) = (coord.0 as i16, coord.1 as i16);
        if let Some(index) = self.native_fixed_cell_index(x, y) {
            self.native_tube_indices[index].raw()
        } else {
            self.shared_cell_dummy
                .stamp_coord(i32::from(x), i32::from(y));
            self.shared_cell_dummy.raw_tube_index()
        }
    }

    /// GetTube484F20 validates the signed registry index, without a Land gate.
    pub(crate) fn tube_at_native_coord(&self, coord: (u16, u16)) -> Option<&TubeFact> {
        let index = NativeTubeCellIndex::from_raw(self.raw_tube_index_at_native_coord(coord));
        self.tube(index.validated_id(self.tube_facts.len())?)
    }

    /// Cell484F20 reads its retained receiver's signed index; unlike a map
    /// coordinate helper this performs no additional lookup or Dummy stamp.
    pub(crate) fn tube_for_native_cell(&self, cell: NativeCellIdentity) -> Option<&TubeFact> {
        let index = match cell {
            NativeCellIdentity::Real(index) => self.native_tube_indices[index],
            NativeCellIdentity::Dummy => {
                NativeTubeCellIndex::from_raw(self.shared_cell_dummy.raw_tube_index())
            }
        };
        self.tube(index.validated_id(self.tube_facts.len())?)
    }

    pub fn step_coord_by_direction(&self, coord: (u16, u16), direction: u8) -> Option<(u16, u16)> {
        if crate::util::direction::is_tube_step_direction(direction) {
            return Some(
                self.tube_at_cell(coord.0, coord.1)
                    .map_or((0, 0), |tube| tube.exit),
            );
        }
        let (dx, dy) = crate::util::direction::direction_delta(direction)?;
        let nx = coord.0 as i32 + dx;
        let ny = coord.1 as i32 + dy;
        if nx < 0 || ny < 0 || nx >= self.width as i32 || ny >= self.height as i32 {
            return None;
        }
        Some((nx as u16, ny as u16))
    }

    pub fn walk_directions_from(&self, start: (u16, u16), directions: &[u8]) -> Option<(u16, u16)> {
        let mut coord = start;
        for &direction in directions {
            coord = self.step_coord_by_direction(coord, direction)?;
        }
        Some(coord)
    }

    /// Dump selected cells for the bridge crossing oracle.
    ///
    /// This is deliberately read-only and route-scoped. `theater_data` is
    /// optional so tests can exercise bridge facts without retail theater data;
    /// when it is absent, theater membership fields are `None` and the
    /// comparator must keep that group `UNCHECKED`.
    pub fn bridge_oracle_cell_facts(
        &self,
        coords: &[(u16, u16)],
        theater_data: Option<&TheaterData>,
    ) -> Vec<BridgeOracleCellFacts> {
        coords
            .iter()
            .filter_map(|&(rx, ry)| self.cell(rx, ry))
            .map(|cell| BridgeOracleCellFacts::from_cell(cell, theater_data))
            .collect()
    }

    pub fn build(
        map: &MapFile,
        theater_data: Option<&TheaterData>,
        asset_manager: Option<&crate::assets::asset_manager::AssetManager>,
        terrain_rules: Option<&TerrainRules>,
        overlay_registry: Option<&OverlayTypeRegistry>,
        lat_enabled: bool,
        cliff_back_impassability: u8,
    ) -> Self {
        Self::build_inner(
            map,
            theater_data,
            asset_manager,
            terrain_rules,
            overlay_registry,
            None,
            lat_enabled,
            cliff_back_impassability,
            OverlayLoadSource::Authored,
            TerrainBuildProjection::EagerCompatibility,
            None,
            None,
            None,
        )
    }

    /// Selector-free generated-map load seam for focused construction tests.
    /// Production generated maps use `build_with_variant_selector_and_shared_dummy`.
    #[cfg(test)]
    pub(crate) fn build_generated_materialized_for_test(map: &MapFile) -> Self {
        Self::build_inner(
            map,
            None,
            None,
            None,
            None,
            None,
            false,
            0,
            OverlayLoadSource::GeneratedMaterialized,
            TerrainBuildProjection::EagerCompatibility,
            None,
            None,
            None,
        )
    }

    /// Production map-load path with the process-owned native variant selector.
    pub fn build_with_variant_selector(
        map: &MapFile,
        theater_data: Option<&TheaterData>,
        asset_manager: Option<&crate::assets::asset_manager::AssetManager>,
        terrain_rules: Option<&TerrainRules>,
        overlay_registry: Option<&OverlayTypeRegistry>,
        terrain_object_types: Option<&HashMap<String, TerrainObjectType>>,
        lat_enabled: bool,
        cliff_back_impassability: u8,
        scenario_fill_ranged: &mut dyn FnMut(u32, u32) -> u32,
        variant_selector: &mut TileVariantSelectionContext<'_, '_>,
    ) -> Self {
        Self::build_inner(
            map,
            theater_data,
            asset_manager,
            terrain_rules,
            overlay_registry,
            terrain_object_types,
            lat_enabled,
            cliff_back_impassability,
            OverlayLoadSource::Authored,
            TerrainBuildProjection::EagerCompatibility,
            Some(scenario_fill_ranged),
            Some(variant_selector),
            None,
        )
    }

    /// Production map-load path whose CellClass grid is born with the process
    /// dummy already attached. This is required before the post-Resize
    /// OverlayPack `SetBridgeDirection_*` pass; binding afterward loses every
    /// missing-neighbor write to the shared fallback object.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn build_with_variant_selector_and_shared_dummy(
        map: &MapFile,
        theater_data: Option<&TheaterData>,
        asset_manager: Option<&crate::assets::asset_manager::AssetManager>,
        terrain_rules: Option<&TerrainRules>,
        overlay_registry: Option<&OverlayTypeRegistry>,
        terrain_object_types: Option<&HashMap<String, TerrainObjectType>>,
        lat_enabled: bool,
        cliff_back_impassability: u8,
        scenario_fill_ranged: &mut dyn FnMut(u32, u32) -> u32,
        variant_selector: &mut TileVariantSelectionContext<'_, '_>,
        shared_cell_dummy: SharedCellDummy,
        overlay_load_source: OverlayLoadSource,
    ) -> Self {
        Self::build_inner(
            map,
            theater_data,
            asset_manager,
            terrain_rules,
            overlay_registry,
            terrain_object_types,
            lat_enabled,
            cliff_back_impassability,
            overlay_load_source,
            TerrainBuildProjection::EagerCompatibility,
            Some(scenario_fill_ranged),
            Some(variant_selector),
            Some(shared_cell_dummy),
        )
    }

    /// Active authored-map Fill boundary. This materializes the native Size
    /// diamond, pristine TMP metadata, and variant selection only. Executable
    /// map-load effects are deliberately deferred until the staged Simulation
    /// owns native identity and can run the consuming overlay transaction.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn build_pending_authored_with_variant_selector_and_shared_dummy(
        map: &MapFile,
        theater_data: Option<&TheaterData>,
        asset_manager: Option<&crate::assets::asset_manager::AssetManager>,
        terrain_rules: Option<&TerrainRules>,
        overlay_registry: Option<&OverlayTypeRegistry>,
        lat_enabled: bool,
        cliff_back_impassability: u8,
        scenario_fill_ranged: &mut dyn FnMut(u32, u32) -> u32,
        variant_selector: &mut TileVariantSelectionContext<'_, '_>,
        shared_cell_dummy: SharedCellDummy,
    ) -> AuthoredTerrainFill {
        AuthoredTerrainFill(Self::build_inner(
            map,
            theater_data,
            asset_manager,
            terrain_rules,
            overlay_registry,
            None,
            lat_enabled,
            cliff_back_impassability,
            OverlayLoadSource::Authored,
            TerrainBuildProjection::PendingAuthored,
            Some(scenario_fill_ranged),
            Some(variant_selector),
            Some(shared_cell_dummy),
        ))
    }

    fn build_inner(
        map: &MapFile,
        theater_data: Option<&TheaterData>,
        asset_manager: Option<&crate::assets::asset_manager::AssetManager>,
        terrain_rules: Option<&TerrainRules>,
        overlay_registry: Option<&OverlayTypeRegistry>,
        terrain_object_types: Option<&HashMap<String, TerrainObjectType>>,
        lat_enabled: bool,
        cliff_back_impassability: u8,
        overlay_load_source: OverlayLoadSource,
        projection: TerrainBuildProjection,
        mut scenario_fill_ranged: Option<&mut dyn FnMut(u32, u32) -> u32>,
        mut variant_selector: Option<&mut TileVariantSelectionContext<'_, '_>>,
        shared_cell_dummy: Option<SharedCellDummy>,
    ) -> Self {
        let shared_cell_dummy = shared_cell_dummy.unwrap_or_default();
        let clear_tile_id = theater_data
            .and_then(|td| td.rmg_tiles.clear_tile)
            .unwrap_or(0);
        let materialized_size_diamond = variant_selector.is_some();
        let playfield = materialized_size_diamond.then(|| Playfield::from_header(&map.header));
        let raw_cells = if let (Some(selector), Some(fill_ranged)) = (
            variant_selector.as_deref_mut(),
            scenario_fill_ranged.as_deref_mut(),
        ) {
            materialize_map_load_cells(
                map,
                theater_data.map(|theater| &theater.lookup),
                selector,
                fill_ranged,
                &shared_cell_dummy,
            )
        } else {
            // The selector-free constructor is retained for focused tests and
            // synthetic grids. Pack-derived fixtures still cross the native
            // compatibility seam, while manual/RMG actual IDs stay untouched.
            selector_free_map_cells(map, theater_data.map(|theater| &theater.lookup))
        };
        let (width, height) = grid_dimensions(&raw_cells);
        if width == 0 || height == 0 {
            return Self {
                width: 0,
                height: 0,
                cells: Vec::new(),
                native_tmp_draw_heights: HashMap::new(),
                shared_cell_dummy,
                mutation_epoch: std::cell::Cell::new(0),
                native_allocated: materialized_size_diamond.then(Vec::new),
                radar_color_valid: Vec::new(),
                damaged_radar_metadata: Vec::new(),
                tube_facts: Vec::new(),
                native_tube_indices: Vec::new(),
                tube_native_ids: Vec::new(),
                clear_tile_id,
                projectile_water_set_base: theater_data
                    .and_then(|td| td.rmg_tiles.water_set)
                    .map_or(-1, i32::from),
                tile_registry_len: theater_data.map(|td| td.lookup.len()),
                high_bridge_rim_tiles: theater_data
                    .map(super::bridge_rim_tiles::HighBridgeRimTiles::from_theater),
                bridge_set_start: theater_data.and_then(|td| {
                    td.bridge_set
                        .and_then(|set| td.lookup.bounds().get(set as usize))
                        .map(|bounds| bounds.start)
                }),
                wood_bridge_set_start: theater_data.and_then(|td| {
                    td.wood_bridge_set
                        .and_then(|set| td.lookup.bounds().get(set as usize))
                        .map(|bounds| bounds.start)
                }),
                tile_animations: Vec::new(),
                destroyable_cliff_catalog: None,
                bridge_recalc_catalog: None,
            };
        }

        let mut final_cells: Vec<MapCell> = raw_cells.clone();
        let mut metadata_cache: HashMap<TileKey, TileMetadata> = HashMap::new();
        let mut warned_unknown_land_types: HashSet<u8> = HashSet::new();
        let native_allocated = materialized_size_diamond.then(|| {
            let mut mask = vec![false; width as usize * height as usize];
            for cell in &raw_cells {
                let index = cell.ry as usize * width as usize + cell.rx as usize;
                if let Some(slot) = mask.get_mut(index) {
                    *slot = true;
                }
            }
            mask
        });
        let load_slope_states = if projection.is_eager() && lat_enabled {
            theater_data.map(|td| {
                let lat_config = lat::parse_lat_config(&td.ini_data, &td.lookup);
                let slope_config = lat::SlopeFixupConfig {
                    ramp_base: td.rmg_tiles.ramp_base.map_or(-1, i32::from),
                    ramp_smooth: td.rmg_tiles.ramp_smooth.map_or(-1, i32::from),
                };
                let mut pristine_slope = |tile_index: i32, sub_tile: u8| {
                    let Ok(tile_id) = u16::try_from(tile_index) else {
                        return 0;
                    };
                    if tile_index == theater::NO_TILE || usize::from(tile_id) >= td.lookup.len() {
                        return 0;
                    }
                    cached_tile_metadata(
                        &mut metadata_cache,
                        theater_data,
                        asset_manager,
                        terrain_rules,
                        TileKey {
                            tile_id,
                            sub_tile,
                            variant: 0,
                        },
                        &mut warned_unknown_land_types,
                    )
                    .slope_type
                };
                lat::apply_load_recalc_sweeps(
                    &mut final_cells,
                    &lat_config,
                    slope_config,
                    &mut pristine_slope,
                )
            })
        } else {
            None
        };
        let load_slope_lookup = load_slope_states.as_ref().map(|states| {
            final_cells
                .iter()
                .zip(states)
                .map(|(cell, &slope)| ((cell.rx, cell.ry), slope))
                .collect::<HashMap<_, _>>()
        });

        let raw_lookup: HashMap<(u16, u16), &MapCell> =
            raw_cells.iter().map(|c| ((c.rx, c.ry), c)).collect();
        let final_lookup: HashMap<(u16, u16), &MapCell> =
            final_cells.iter().map(|c| ((c.rx, c.ry), c)).collect();

        let snow_theater = map.header.theater.eq_ignore_ascii_case("SNOW");
        let terrain_objects: HashMap<(u16, u16), u8> = projection
            .is_eager()
            .then_some(map.terrain_objects.iter())
            .into_iter()
            .flatten()
            .map(|obj| {
                let occupation = terrain_object_types
                    .and_then(|types| types.get(&obj.name.to_ascii_uppercase()))
                    .map(|terrain_type| {
                        if snow_theater {
                            terrain_type.snow_occupation_bits
                        } else {
                            terrain_type.temperate_occupation_bits
                        }
                    })
                    .unwrap_or(7)
                    & 0x07;
                ((obj.rx, obj.ry), occupation)
            })
            .collect();

        let mut overlays_by_cell: HashMap<(u16, u16), Vec<&OverlayEntry>> = HashMap::new();
        for overlay in projection
            .is_eager()
            .then_some(map.overlays.iter())
            .into_iter()
            .flatten()
        {
            overlays_by_cell
                .entry((overlay.rx, overlay.ry))
                .or_default()
                .push(overlay);
        }

        let mut cells: Vec<ResolvedTerrainCell> =
            Vec::with_capacity(width as usize * height as usize);
        let mut radar_color_valid = Vec::with_capacity(cells.capacity());
        let mut damaged_radar_metadata = Vec::with_capacity(cells.capacity());
        let mut cliff_back_eligibility: Vec<CliffBackEligibility> =
            Vec::with_capacity(width as usize * height as usize);
        let mut tile_animations: Vec<TerrainTileAnimation> = Vec::new();
        let mut selector_calls = 0usize;
        let mut replacement_cells = 0usize;
        let mut high_suffix_cells = 0usize;
        let mut max_total_files = 1u8;

        for ry in 0..height {
            for rx in 0..width {
                let raw = raw_lookup.get(&(rx, ry)).copied();
                let final_cell = final_lookup.get(&(rx, ry)).copied();
                let (final_tile_index, final_sub_tile, level) = final_cell
                    .map(|cell| (cell.tile_index, cell.sub_tile, cell.z))
                    .unwrap_or((0, 0, 0));
                let tile_index_out_of_range = theater_data.is_some_and(|td| {
                    final_tile_index >= 0
                        && final_tile_index != 0xFFFF
                        && final_tile_index as usize >= td.lookup.len()
                });
                let uses_clear_fallback = raw.is_none()
                    || final_tile_index < 0
                    || final_tile_index == 0xFFFF
                    || tile_index_out_of_range;
                let (presentation_tile_id, presentation_sub_tile) = if uses_clear_fallback {
                    (clear_tile_id, 0)
                } else {
                    (normalize_tile_id(final_tile_index), final_sub_tile)
                };
                let pristine_key = TileKey {
                    tile_id: presentation_tile_id,
                    sub_tile: presentation_sub_tile,
                    variant: 0,
                };
                let pristine_metadata = cached_tile_metadata(
                    &mut metadata_cache,
                    theater_data,
                    asset_manager,
                    terrain_rules,
                    pristine_key,
                    &mut warned_unknown_land_types,
                );
                // The active RecalcAttributes caller validates this exact
                // registered pristine receiver. A failed entry check on an
                // otherwise valid tile takes the dedicated FFFF/0 fallback;
                // an already-invalid tile id instead stays on the later
                // sentinel branch.
                let pristine_subtile_entry_failed =
                    !uses_clear_fallback && pristine_metadata.subtile_entry_valid == Some(false);
                // Retail selects the independent tactical owner early, using
                // only pristine dimensions and the pristine damaged-data gate.
                // CellClass attributes remain owned by the registered pristine
                // head; selected-owner presentation fields are overlaid below.
                let mut variant = 0u8;
                if let (Some(td), Some(selector)) = (theater_data, variant_selector.as_mut()) {
                    let total_file_count = td.lookup.total_file_count(presentation_tile_id);
                    let outside_size_diamond = materialized_size_diamond && raw.is_none();
                    if !outside_size_diamond
                        && !pristine_subtile_entry_failed
                        && ordinary_variant_selection_enabled(
                            total_file_count,
                            uses_clear_fallback,
                            pristine_metadata.has_damaged_data,
                        )
                    {
                        max_total_files = max_total_files.max(total_file_count);
                        variant = selector.select_variant(
                            i32::from(rx),
                            i32::from(ry),
                            presentation_sub_tile,
                            pristine_metadata.template_width_cells,
                            pristine_metadata.template_height_cells,
                            total_file_count,
                        );
                        selector_calls += 1;
                        replacement_cells += usize::from(variant > 0);
                        high_suffix_cells += usize::from(variant > 4);
                    }
                }
                let selected_key = TileKey {
                    variant,
                    ..pristine_key
                };
                let mut metadata = pristine_metadata;
                if selected_key != pristine_key {
                    let selected_metadata = cached_tile_metadata(
                        &mut metadata_cache,
                        theater_data,
                        asset_manager,
                        terrain_rules,
                        selected_key,
                        &mut warned_unknown_land_types,
                    );
                    apply_selected_presentation_metadata(&mut metadata, &selected_metadata);
                }
                if !pristine_subtile_entry_failed {
                    if let Some(slope_type) = load_slope_lookup
                        .as_ref()
                        .and_then(|slopes| slopes.get(&(rx, ry)))
                        .copied()
                    {
                        metadata.slope_type = slope_type;
                        metadata.has_ramp = slope_type != 0;
                    }
                }
                let damaged_radar = damaged_variant_radar_metadata(
                    &mut metadata_cache,
                    theater_data,
                    asset_manager,
                    terrain_rules,
                    pristine_key,
                    &metadata,
                    &mut warned_unknown_land_types,
                );
                let terrain_object_occupation = terrain_objects.get(&(rx, ry)).copied();
                let terrain_object_blocks =
                    terrain_object_occupation.is_some_and(|occupation| occupation != 0);
                let overlay_effects = classify_overlay_effects(
                    overlays_by_cell.get(&(rx, ry)),
                    overlay_registry,
                    level,
                    metadata.slope_type,
                );
                let sparse_subtile_fallback =
                    pristine_subtile_entry_failed && !overlay_effects.claims_cell_attributes;
                if sparse_subtile_fallback {
                    // 0x0047D5E6..0x0047D5F9 materializes the failed valid-tile
                    // entry as a no-tile Cell before any valid-tile-only work.
                    metadata.tileset_index = None;
                    metadata.slope_type = 0;
                    metadata.has_ramp = false;
                }
                // Tile-attached animation. The engine spawns one AnimClass per
                // cell whose tile declares a `Tile%02dAnim` block and whose
                // sub-tile equals that block's `AttachesTo`, latching a per-cell
                // flag so later attribute recomputes never spawn a second one —
                // this single pass over each cell is that latch. Two earlier
                // returns exclude cells here too: a missing or out-of-range tile
                // id, and an overlay that claims the cell's attributes.
                if projection.is_eager()
                    && !uses_clear_fallback
                    && !sparse_subtile_fallback
                    && !overlay_effects.claims_cell_attributes
                {
                    if let Some(anim) =
                        theater_data.and_then(|td| td.lookup.tile_anim(presentation_tile_id))
                    {
                        if anim.attaches_to == i32::from(presentation_sub_tile) {
                            let (offset_x, offset_y) =
                                tile_anim_pixel_offset_to_leptons(anim.x_offset, anim.y_offset);
                            tile_animations.push(TerrainTileAnimation {
                                rx,
                                ry,
                                anim_name: anim.anim_name.clone(),
                                world_x: offset_x
                                    + i32::from(rx) * LEPTONS_PER_CELL
                                    + CELL_CENTRE_LEPTONS,
                                world_y: offset_y
                                    + i32::from(ry) * LEPTONS_PER_CELL
                                    + CELL_CENTRE_LEPTONS,
                                world_z: i32::from(level as i8)
                                    * crate::util::lepton::GROUND_LEVEL_HEIGHT_LEPTONS,
                                z_adjust: anim.z_adjust,
                            });
                        }
                    }
                }
                let canonical_ramp = canonical_ramp_from_slope_type(metadata.slope_type);
                // The registered pristine TMP owns the base snapshot. Overlay
                // land never feeds back into these restoration fields.
                let base_land_type = metadata.land_type;
                let base_yr_cell_land_type = metadata.yr_cell_land_type;
                let base_terrain_class = metadata.terrain_class;
                let base_speed_costs = metadata.speed_costs;
                let base_ground_walk_blocked = canonical_ramp.is_none() && metadata.ground_blocked;
                let base_build_blocked = metadata.build_blocked || canonical_ramp.is_some();
                if !sparse_subtile_fallback {
                    if let Some(land) = overlay_effects.effective_land {
                        apply_canonical_land_to_metadata(
                            &mut metadata,
                            land,
                            overlay_effects.effective_land_speed_costs,
                            overlay_effects.effective_land_ground_blocked,
                        );
                    }
                }
                let base_cliff_back_eligible = if sparse_subtile_fallback {
                    base_land_type == LandType::Clear.as_index()
                } else {
                    cliff_back_normal_reclass_applies(base_land_type)
                };
                let current_cliff_back_eligible = if overlay_effects.claims_cell_attributes {
                    true
                } else if sparse_subtile_fallback {
                    metadata.land_type == LandType::Clear.as_index()
                } else {
                    cliff_back_normal_reclass_applies(metadata.land_type)
                };
                cliff_back_eligibility.push(CliffBackEligibility {
                    current: current_cliff_back_eligible,
                    base: base_cliff_back_eligible,
                });
                let is_cliff_like = metadata.is_cliff_like;
                let outside = playfield.is_some_and(|playfield| {
                    !playfield.contains_raised(rx, ry, level as i8, metadata.slope_type)
                });
                let zone_type = recalc_zone_type(
                    outside,
                    overlay_effects.overlay_zone_type,
                    metadata.land_type,
                    metadata.speed_costs.wheel,
                    terrain_object_occupation,
                );
                // Same shape as `base_ground_walk_blocked`, but on the metadata
                // *after* any overlay land override. `base_*` stays pristine on
                // purpose — it is the restoration value for overlay removal —
                // so reading it here would pin a low bridge's deck to the
                // water underneath it.
                let ground_walk_blocked = (canonical_ramp.is_none() && metadata.ground_blocked)
                    || terrain_object_blocks
                    || overlay_effects.overlay_blocks;
                let bridge_walkable = overlay_effects.has_bridge_deck
                    && !overlay_effects.is_low_bridge
                    && !terrain_object_blocks
                    && !overlay_effects.overlay_blocks;
                // Smudges (craters, scorches) only place on tiles whose tileset has
                // Morphable=yes. Cells with no resolved tile (filled_clear) default
                // to false. Computed once at resolve time so the smudge dispatcher
                // reads a single bool.
                let accepts_smudge = if uses_clear_fallback || sparse_subtile_fallback {
                    false
                } else {
                    theater_data
                        .map(|td| td.lookup.is_morphable(presentation_tile_id))
                        .unwrap_or(false)
                };
                let allows_tiberium = if uses_clear_fallback || sparse_subtile_fallback {
                    false
                } else {
                    theater_data
                        .map(|td| td.lookup.allows_tiberium(presentation_tile_id))
                        .unwrap_or(false)
                };
                // Allow layer transitions on any bridge deck cell. High bridges over
                // water have ground_walk_blocked=true, but units still need to transition
                // from Ground→Bridge at the ramp/entry cells.
                // Only bridgehead ramp cells (detected below) allow layer
                // transitions. Deck cells must NOT be transitions — otherwise
                // the A* can switch Bridge→Ground mid-span and units clip
                // through the bridge.
                let bridge_transition = false;
                let build_blocked = base_build_blocked
                    || terrain_object_blocks
                    || overlay_effects.overlay_blocks
                    || overlay_effects.has_bridge_deck;
                let stored_final_tile_index = if sparse_subtile_fallback {
                    0xFFFF
                } else {
                    final_tile_index
                };
                let stored_final_sub_tile = if sparse_subtile_fallback {
                    0
                } else {
                    final_sub_tile
                };
                let is_wood_bridge_repair_tile =
                    is_wood_bridge_repair_tile(theater_data, stored_final_tile_index);
                radar_color_valid.push(metadata.subtile_entry_valid == Some(true));
                damaged_radar_metadata.push(damaged_radar.map(Ok));
                cells.push(ResolvedTerrainCell {
                    rx,
                    ry,
                    source_tile_index: raw.map(|c| c.tile_index).unwrap_or(theater::NO_TILE),
                    source_sub_tile: raw.map(|c| c.sub_tile).unwrap_or(0),
                    final_tile_index: stored_final_tile_index,
                    final_sub_tile: stored_final_sub_tile,
                    is_wood_bridge_repair_tile,
                    level,
                    filled_clear: raw.is_none(),
                    tileset_index: metadata.tileset_index,
                    land_type: metadata.land_type,
                    yr_cell_land_type: metadata.yr_cell_land_type,
                    slope_type: metadata.slope_type,
                    template_height: metadata.template_height,
                    height_in_pixels: metadata.height_in_pixels,
                    render_offset_x: metadata.render_offset_x,
                    render_offset_y: metadata.render_offset_y,
                    terrain_class: metadata.terrain_class,
                    speed_costs: metadata.speed_costs,
                    is_water: metadata.is_water,
                    is_cliff_like,
                    is_rough: metadata.is_rough,
                    is_road: metadata.is_road,
                    accepts_smudge,
                    allows_tiberium,
                    variant,
                    has_ramp: metadata.has_ramp,
                    canonical_ramp,
                    ground_walk_blocked,
                    terrain_object_blocks,
                    terrain_object_occupation,
                    overlay_blocks: overlay_effects.overlay_blocks,
                    overlay_zone_type: overlay_effects.overlay_zone_type,
                    outside_playfield: outside,
                    zone_type,
                    base_ground_walk_blocked,
                    base_build_blocked,
                    base_land_type,
                    base_yr_cell_land_type,
                    base_terrain_class,
                    base_speed_costs,
                    build_blocked,
                    has_bridge_deck: overlay_effects.has_bridge_deck,
                    bridge_walkable,
                    bridge_transition,
                    bridge_deck_level: overlay_effects
                        .bridge_layer
                        .as_ref()
                        .map(|layer| layer.deck_level)
                        .unwrap_or(level),
                    bridge_layer: overlay_effects.bridge_layer,
                    bridge_facts: BridgeCellFacts::default(),
                    tube_index: None,
                    radar_left: metadata.radar_left,
                    radar_right: metadata.radar_right,
                    has_damaged_data: metadata.has_damaged_data,
                    bridgehead_anchor_class_at_load: None,
                });
            }
        }
        // The engine reaches cells through an anti-diagonal iterator, so the
        // animations are constructed in that order. The loop above is row-major,
        // so restore the sweep order here: the spawner then assigns animation
        // identities the way a native load would.
        tile_animations.sort_by_key(|anim| (anim.rx as u32 + anim.ry as u32, anim.rx));
        if !tile_animations.is_empty() {
            log::info!(
                "ResolvedTerrain: {} terrain tile animations resolved",
                tile_animations.len(),
            );
        }
        if selector_calls > 0 {
            log::info!(
                "ResolvedTerrain variants: {} selector cells, {} replacements, {} e-or-later, max total files {}",
                selector_calls,
                replacement_cells,
                high_suffix_cells,
                max_total_files,
            );
        }

        // OverlayPack is decoded in fixed-grid row-major order. Each live
        // OverlayClass anchor is an allocated CellClass; its setter then walks
        // through MapClass lookups, so missing neighbors stamp/mutate the one
        // shared dummy rather than disappearing at the rectangular edge.
        for overlay in projection
            .is_eager()
            .then_some(map.overlays.iter())
            .into_iter()
            .flatten()
        {
            let anchor_index = native_resolved_cell_index(
                width,
                height,
                native_allocated.as_deref(),
                cells.len(),
                i32::from(overlay.rx),
                i32::from(overlay.ry),
            );
            if let Some(index) = anchor_index {
                cells[index].bridge_facts.overlay_id = Some(overlay.overlay_id);
            }
            if overlay_load_source == OverlayLoadSource::Authored
                && let Some((family, direction)) =
                    crate::map::bridge_facts::high_bridge_stamp_for_overlay(overlay.overlay_id)
                && anchor_index.is_some()
            {
                let stamp = BridgeFlagStamp::new((overlay.rx, overlay.ry), direction, true);
                let _ = apply_native_bridge_flag_stamp_to_parts(
                    &mut cells,
                    width,
                    height,
                    native_allocated.as_deref(),
                    &shared_cell_dummy,
                    stamp,
                    Some(family),
                );
            }
        }

        let raw_overlay_data = projection.is_eager().then(|| {
            map.overlay_data_pack()
                .expect("legacy terrain projection must precede authored pack consumption")
        });
        if let Some(raw_overlay_data) = raw_overlay_data.filter(|data| data.is_present()) {
            for (index, cell) in cells.iter_mut().enumerate() {
                if native_allocated
                    .as_deref()
                    .is_none_or(|mask| mask.get(index).copied().unwrap_or(false))
                {
                    cell.bridge_facts.state_byte = raw_overlay_data.byte_at(cell.rx, cell.ry);
                }
            }
        }

        for cell in projection
            .is_eager()
            .then_some(cells.iter_mut())
            .into_iter()
            .flatten()
        {
            let facts = cell.bridge_facts;
            if facts.has_structural_bridge() {
                cell.has_bridge_deck = true;
                cell.bridge_walkable = !cell.terrain_object_blocks && !cell.overlay_blocks;
                cell.bridge_deck_level = cell.level.saturating_add(4);
                cell.build_blocked = cell.base_build_blocked
                    || cell.terrain_object_blocks
                    || cell.overlay_blocks
                    || cell.bridge_walkable;
            } else if facts.family != crate::map::bridge_facts::BridgeStampFamily::None
                && cell
                    .bridge_layer
                    .as_ref()
                    .is_some_and(|bl| bl.direction != BridgeDirection::Low)
            {
                cell.has_bridge_deck = false;
                cell.bridge_walkable = false;
                cell.bridge_deck_level = cell.level;
                cell.build_blocked =
                    cell.base_build_blocked || cell.terrain_object_blocks || cell.overlay_blocks;
            }

            if facts.has_transition_flag() {
                cell.bridge_transition = true;
            }
        }

        if projection.is_eager()
            && let Some(td) = theater_data
        {
            if let (Some(bs_idx), Some(ramp_table)) = (
                td.bridge_set,
                crate::map::theater::BridgeRampTileTable::from_theater(td),
            ) {
                if let Some(bridge_set_bounds) = td.lookup.bounds().get(bs_idx as usize) {
                    let bridge_set_start = bridge_set_bounds.start;
                    let mut ramp_count = 0usize;
                    for cell in &mut cells {
                        if cell.final_tile_index < 0 {
                            continue;
                        }
                        let tile_id = normalize_tile_id(cell.final_tile_index);
                        let Some(ramp_tile) = ramp_table.match_tile_id(
                            tile_id,
                            bridge_set_start,
                            bridge_set_bounds.count,
                            cell.template_height,
                        ) else {
                            continue;
                        };
                        cell.bridge_facts.ramp_tile = Some(ramp_tile);
                        ramp_count += 1;
                    }
                    if ramp_count > 0 {
                        log::info!(
                            "ResolvedTerrain: {} exact high bridge ramp cells detected",
                            ramp_count,
                        );
                    }
                }
            }
        }

        {
            let mut high_deck: Vec<(u16, u16, u8, u32)> = cells
                .iter()
                .filter(|c| c.bridge_facts.has_structural_bridge())
                .map(|c| (c.rx, c.ry, c.bridge_deck_level, c.bridge_facts.raw_flags))
                .collect();
            high_deck.sort_by_key(|(rx, ry, _, _)| (*rx, *ry));
            if !high_deck.is_empty() {
                log::debug!(
                    "High bridge stamped structural cells ({} total):",
                    high_deck.len(),
                );
                for (rx, ry, dl, flags) in &high_deck {
                    log::debug!("  ({}, {}) deck_level={} flags=0x{:X}", rx, ry, dl, flags);
                }
            }
        }

        // Log bridge cell statistics for diagnostics.
        let bridge_cell_count: usize = cells.iter().filter(|c| c.has_bridge_deck).count();
        let low_bridge_count: usize = cells
            .iter()
            .filter(|c| {
                c.bridge_layer
                    .as_ref()
                    .map(|bl| bl.direction == BridgeDirection::Low)
                    .unwrap_or(false)
            })
            .count();
        let high_bridge_count: usize = bridge_cell_count - low_bridge_count;
        if bridge_cell_count > 0 {
            log::info!(
                "ResolvedTerrain: {} bridge deck cells ({} high, {} low)",
                bridge_cell_count,
                high_bridge_count,
                low_bridge_count,
            );
        }

        // CellClass::RecalcAttributes has three CliffBack writer gates: early
        // overlay is unconditional, unusable/sparse subtile is Clear-only, and
        // the normal path accepts Clear, Water, Beach, or Ice. Non-2 bytes have
        // no observable write, so this load resolver output-gates the pure scan.
        if projection.is_eager() && cliff_back_impassability == 2 {
            const CLIFF_BACK_HEIGHT_DIFF: i16 = 4;
            // 6 neighbor offsets in (dx, dy) matching gamemd.exe RecalcAttributes:
            // (X, Y-1), (X-1, Y), (X+2, Y+2), (X+1, Y+1), (X-1, Y+1), (X+1, Y-1)
            const NEIGHBOR_OFFSETS: [(i32, i32); 6] =
                [(0, -1), (-1, 0), (2, 2), (1, 1), (-1, 1), (1, -1)];
            let rock_lt = LandType::Rock.as_index();
            let mut rock_terrain_class = LandType::Rock.terrain_class();
            let mut rock_speed_costs = SpeedCostProfile::default();
            rock_speed_costs.wheel = Some(0);
            let mut rock_is_water = LandType::Rock.is_water();
            let mut rock_is_rough = LandType::Rock.is_rough();
            let mut rock_is_road = LandType::Rock.is_road();
            let mut rock_ground_blocked = true;
            let mut rock_build_blocked = true;
            if let Some(rock) = terrain_rules
                .and_then(|rules| rules.semantics_for_land_type(LandType::Rock.as_index()))
            {
                rock_terrain_class = rock.terrain_class;
                rock_speed_costs = rock.speed_costs;
                rock_is_water = rock.water;
                rock_is_rough = rock.rough;
                rock_is_road = rock.road;
                rock_ground_blocked = rock.ground_blocked;
                rock_build_blocked = !rock.buildable;
            }

            debug_assert_eq!(cliff_back_eligibility.len(), cells.len());
            let resolved_cell_count = cells.len();
            let fixed_stride_index = |x: i32, y: i32| {
                let native_index = crate::map::cell_index::cell_linear_index(x, y)?;
                let canonical_x = (native_index % crate::map::cell_index::CELL_ROW_STRIDE) as usize;
                let canonical_y = (native_index / crate::map::cell_index::CELL_ROW_STRIDE) as usize;
                if canonical_x >= usize::from(width) || canonical_y >= usize::from(height) {
                    return None;
                }
                let resolved_index = canonical_y * usize::from(width) + canonical_x;
                (resolved_index < resolved_cell_count
                    && native_allocated
                        .as_deref()
                        .is_none_or(|mask| mask.get(resolved_index).copied().unwrap_or(false)))
                .then_some(resolved_index)
            };

            let mut cliff_back_count: usize = 0;
            for idx in 0..cells.len() {
                let eligibility = cliff_back_eligibility[idx];
                if !eligibility.current && !eligibility.base {
                    continue;
                }
                let cell_level = i16::from(cells[idx].level as i8);
                let rx = cells[idx].rx as i16;
                let ry = cells[idx].ry as i16;

                let mut behind_cliff = false;
                for &(dx, dy) in &NEIGHBOR_OFFSETS {
                    let nx = rx.wrapping_add(dx as i16) as i32;
                    let ny = ry.wrapping_add(dy as i16) as i32;
                    if fixed_stride_index(nx, ny).is_some_and(|nidx| {
                        i16::from(cells[nidx].level as i8) >= cell_level + CLIFF_BACK_HEIGHT_DIFF
                    }) {
                        behind_cliff = true;
                        break;
                    }
                }
                if behind_cliff {
                    let cell = &mut cells[idx];
                    if eligibility.current {
                        cell.land_type = rock_lt;
                        cell.yr_cell_land_type = rock_lt;
                        cell.terrain_class = rock_terrain_class;
                        cell.speed_costs = rock_speed_costs;
                        cell.is_water = rock_is_water;
                        cell.is_cliff_like = true;
                        cell.is_rough = rock_is_rough;
                        cell.is_road = rock_is_road;
                        // RecalcZoneType runs after the native LandType write.
                        cell.ground_walk_blocked = rock_ground_blocked
                            || cell.terrain_object_blocks
                            || cell.overlay_blocks;
                        cell.build_blocked = rock_build_blocked
                            || cell.terrain_object_blocks
                            || cell.overlay_blocks
                            || cell.has_bridge_deck;
                        cell.zone_type = recalc_zone_type(
                            cell.outside_playfield,
                            cell.overlay_zone_type,
                            cell.land_type,
                            cell.speed_costs.wheel,
                            cell.terrain_object_occupation,
                        );
                        cliff_back_count += 1;
                    }
                    // Restoration follows the no-overlay branch. A claiming
                    // overlay on underlying Road must not persist Rock after
                    // that overlay is removed.
                    if eligibility.base {
                        cell.base_land_type = rock_lt;
                        cell.base_yr_cell_land_type = rock_lt;
                        cell.base_terrain_class = rock_terrain_class;
                        cell.base_speed_costs = rock_speed_costs;
                        cell.base_ground_walk_blocked = rock_ground_blocked;
                        cell.base_build_blocked = rock_build_blocked;
                    }
                }
            }
            if cliff_back_count > 0 {
                log::info!(
                    "ResolvedTerrain: {} cells marked impassable by CliffBackImpassability",
                    cliff_back_count,
                );
            }
        }

        let mut tube_facts = Vec::new();
        if projection.is_eager() {
            tube_facts = seed_explicit_map_tubes(&mut cells, width, height, &map.explicit_tubes);
            build_auto_low_bridge_tubes(&mut cells, width, height, theater_data, &mut tube_facts);
        }
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

        let mut grid = Self {
            width,
            height,
            cells,
            native_tmp_draw_heights: load_native_tmp_draw_heights(theater_data, asset_manager),
            shared_cell_dummy,
            mutation_epoch: std::cell::Cell::new(0),
            native_allocated,
            radar_color_valid,
            damaged_radar_metadata,
            tube_facts,
            native_tube_indices,
            tube_native_ids,
            clear_tile_id,
            projectile_water_set_base: theater_data
                .and_then(|td| td.rmg_tiles.water_set)
                .map_or(-1, i32::from),
            tile_registry_len: theater_data.map(|td| td.lookup.len()),
            high_bridge_rim_tiles: theater_data
                .map(super::bridge_rim_tiles::HighBridgeRimTiles::from_theater),
            bridge_set_start: theater_data.and_then(|td| {
                td.bridge_set
                    .and_then(|set| td.lookup.bounds().get(set as usize))
                    .map(|bounds| bounds.start)
            }),
            wood_bridge_set_start: theater_data.and_then(|td| {
                td.wood_bridge_set
                    .and_then(|set| td.lookup.bounds().get(set as usize))
                    .map(|bounds| bounds.start)
            }),
            tile_animations,
            destroyable_cliff_catalog: build_destroyable_cliff_catalog(
                theater_data,
                asset_manager,
                terrain_rules,
            ),
            bridge_recalc_catalog: match (theater_data, asset_manager, terrain_rules) {
                (Some(theater), Some(assets), Some(rules)) => {
                    Some(Arc::new(BridgeRecalcCatalog::for_runtime_bridges(
                        theater,
                        assets,
                        rules,
                        lat_enabled,
                        cliff_back_impassability,
                        variant_selector
                            .as_ref()
                            .and_then(|selector| selector.initialized_table()),
                    )))
                }
                _ => None,
            },
        };
        if projection.is_eager()
            && let Some(theater) = theater_data
        {
            grid.rebuild_bridgehead_anchor_classes_from_final_tiles(theater);
        }
        grid
    }

    /// Rebuild the authored bridgehead classification from the current,
    /// finalized tile surface. Pending authored Fill deliberately leaves this
    /// empty because LAT/Recalc has not established final tile authority yet.
    pub(crate) fn rebuild_bridgehead_anchor_classes_from_final_tiles(
        &mut self,
        theater: &TheaterData,
    ) {
        for cell in &mut self.cells {
            cell.bridgehead_anchor_class_at_load = None;
        }
        let Some(table) = crate::map::theater::BridgeAnchorVariantTable::from_theater(theater)
        else {
            return;
        };
        let Some(bridge_set) = theater.bridge_set else {
            return;
        };
        for cell in &mut self.cells {
            if cell.tileset_index != Some(bridge_set) || cell.final_tile_index < 0 {
                continue;
            }
            let tile_id = if cell.final_tile_index == 0xFFFF {
                0
            } else {
                cell.final_tile_index as u16
            };
            if let Some((_axis, class)) = table.match_tile_id(tile_id) {
                cell.bridgehead_anchor_class_at_load = Some(class);
            }
        }
    }

    pub fn build_height_map(&self) -> BTreeMap<(u16, u16), u8> {
        self.cells
            .iter()
            .map(|cell| ((cell.rx, cell.ry), cell.level))
            .collect()
    }

    /// Build a bridge deck height map — only HIGH bridge cells are included.
    /// Low bridges (LOBRDG/LOBRDB) are at ground level and don't need height
    /// correction for click resolution or debug overlays.
    pub fn build_bridge_height_map(&self) -> BTreeMap<(u16, u16), u8> {
        self.cells
            .iter()
            .filter(|cell| {
                cell.has_bridge_deck
                    && !cell
                        .bridge_layer
                        .as_ref()
                        .is_some_and(|bl| bl.direction == BridgeDirection::Low)
            })
            .map(|cell| ((cell.rx, cell.ry), cell.bridge_deck_level))
            .collect()
    }

    /// Build bridge metadata for the tactical screen-to-cell inverse.
    ///
    /// This keeps the existing deck-height map intact for render/debug users,
    /// while exposing the structural and direction-zero flags consumed by the
    /// verified gamemd tactical inverse branch.
    pub fn build_tactical_bridge_inverse_map(
        &self,
    ) -> BTreeMap<(u16, u16), crate::map::terrain::TacticalBridgeCell> {
        self.cells
            .iter()
            .filter(|cell| {
                cell.has_bridge_deck
                    && !cell
                        .bridge_layer
                        .as_ref()
                        .is_some_and(|bl| bl.direction == BridgeDirection::Low)
            })
            .map(|cell| {
                (
                    (cell.rx, cell.ry),
                    crate::map::terrain::TacticalBridgeCell {
                        structural: cell.bridge_facts.has_structural_bridge(),
                        direction_zero: cell
                            .bridge_facts
                            .has_flag(crate::map::bridge_facts::BRIDGE_FLAG_DIRECTION_ZERO),
                    },
                )
            })
            .collect()
    }
}

#[derive(Debug, Clone)]
struct TileMetadata {
    tileset_index: Option<u16>,
    has_tmp_metadata: bool,
    /// The independent TMP file parsed successfully. This stays true for a
    /// sparse/missing requested subimage, which native distinguishes from an
    /// absent damaged sibling chain.
    tmp_file_valid: bool,
    /// Result of the active CellClass subtile-entry check. `None` means this
    /// synthetic/tooling path had no TMP data source to check.
    subtile_entry_valid: Option<bool>,
    /// Pristine TMP grid dimensions used by ordinary variant normalization.
    template_width_cells: u32,
    template_height_cells: u32,
    /// Canonical YR LandType used for terrain semantics and CellClass Land
    /// derivation. Reduced `zone_type` selects the movement matrix column.
    land_type: u8,
    /// Retained CellClass LandType mirror for binary-derived predicates and
    /// compatibility consumers. This is not the raw TMP terrain_type byte.
    yr_cell_land_type: u8,
    /// Raw TMP terrain_type byte (0-15) for rules.ini semantic lookups.
    raw_land_type: u8,
    slope_type: u8,
    template_height: u8,
    height_in_pixels: i8,
    render_offset_x: i32,
    render_offset_y: i32,
    terrain_class: TerrainClass,
    speed_costs: SpeedCostProfile,
    is_water: bool,
    is_cliff_like: bool,
    is_rough: bool,
    is_road: bool,
    has_ramp: bool,
    ground_blocked: bool,
    build_blocked: bool,
    /// Per-tile radar minimap color (left half of isometric diamond), from TMP header.
    radar_left: [u8; 3],
    /// Per-tile radar minimap color (right half of isometric diamond), from TMP header.
    radar_right: [u8; 3],
    /// Mirrors `TmpTile.has_damaged_data` — set when the TMP sub-tile flag DWORD
    /// declares a baked damaged-variant pixel set.
    has_damaged_data: bool,
}

impl Default for TileMetadata {
    fn default() -> Self {
        Self {
            tileset_index: None,
            has_tmp_metadata: false,
            tmp_file_valid: false,
            subtile_entry_valid: None,
            template_width_cells: 0,
            template_height_cells: 0,
            land_type: 0,
            yr_cell_land_type: 0,
            raw_land_type: 0,
            slope_type: 0,
            template_height: 0,
            height_in_pixels: 0,
            render_offset_x: 0,
            render_offset_y: 0,
            terrain_class: TerrainClass::Unknown,
            speed_costs: SpeedCostProfile::default(),
            is_water: false,
            is_cliff_like: false,
            is_rough: false,
            is_road: false,
            has_ramp: false,
            ground_blocked: false,
            build_blocked: false,
            radar_left: [0, 0, 0],
            radar_right: [0, 0, 0],
            has_damaged_data: false,
        }
    }
}

impl TileMetadata {
    #[cfg(test)]
    fn from_resolved_cell(cell: &ResolvedTerrainCell) -> Self {
        Self {
            tileset_index: cell.tileset_index,
            has_tmp_metadata: true,
            tmp_file_valid: true,
            subtile_entry_valid: Some(true),
            template_width_cells: 1,
            template_height_cells: 1,
            land_type: cell.base_land_type,
            yr_cell_land_type: cell.base_yr_cell_land_type,
            raw_land_type: cell.base_land_type,
            slope_type: cell.slope_type,
            template_height: cell.template_height,
            height_in_pixels: cell.height_in_pixels,
            render_offset_x: cell.render_offset_x,
            render_offset_y: cell.render_offset_y,
            terrain_class: cell.base_terrain_class,
            speed_costs: cell.base_speed_costs,
            is_water: cell.base_terrain_class == TerrainClass::Water,
            is_cliff_like: cell.is_cliff_like,
            is_rough: cell.is_rough,
            is_road: cell.is_road,
            has_ramp: cell.has_ramp,
            ground_blocked: cell.base_ground_walk_blocked,
            build_blocked: cell.base_build_blocked,
            radar_left: cell.radar_left,
            radar_right: cell.radar_right,
            has_damaged_data: cell.has_damaged_data,
        }
    }
}

#[derive(Debug, Clone, Default)]
struct OverlayEffects {
    /// The overlay owns this cell's attributes, so `RecalcAttributes` takes its
    /// short branch: LAT/slope fixup and the zone recompute still run, but the
    /// tile-attached animation spawn and the ShadowCaster walk are skipped.
    claims_cell_attributes: bool,
    overlay_blocks: bool,
    overlay_zone_type: Option<u8>,
    has_bridge_deck: bool,
    bridge_layer: Option<BridgeLayer>,
    is_low_bridge: bool,
    effective_land: Option<LandType>,
    effective_land_speed_costs: Option<SpeedCostProfile>,
    /// Ground-blocking of `effective_land`'s rules row. Travels with
    /// `effective_land` so the replacement LandType brings its own passability
    /// instead of leaving the tile's behind. See `OverlayTypeFlags::land_ground_blocked`.
    effective_land_ground_blocked: bool,
}

#[derive(Debug, Clone, Copy, Default)]
struct CliffBackEligibility {
    /// Eligibility of the current overlay-adjusted Cell LandType branch.
    current: bool,
    /// Eligibility of the no-overlay restoration snapshot.
    base: bool,
}

fn grid_dimensions(cells: &[MapCell]) -> (u16, u16) {
    let mut max_rx: u16 = 0;
    let mut max_ry: u16 = 0;
    let mut found = false;
    for cell in cells {
        found = true;
        max_rx = max_rx.max(cell.rx);
        max_ry = max_ry.max(cell.ry);
    }
    if found {
        (max_rx.saturating_add(1), max_ry.saturating_add(1))
    } else {
        (0, 0)
    }
}

fn normalize_tile_id(tile_index: i32) -> u16 {
    if tile_index == 0xFFFF || tile_index < 0 {
        0
    } else {
        tile_index as u16
    }
}

fn presentation_tile_parts(tile_index: i32, sub_tile: u8, clear_tile_id: u16) -> (u16, u8) {
    if tile_index == 0xFFFF || tile_index < 0 {
        (clear_tile_id, 0)
    } else {
        (tile_index as u16, sub_tile)
    }
}

/// Land types the final normal `CliffBackImpassability` site reclasses to Rock.
///
/// The engine's filter on the ordinary (valid-tile, non-overlay) path is
/// exactly `Clear`, `Water`, `Beach` and `Ice`. Ice is reachable on Snow-theater
/// maps, where a frozen surface runs up to a cliff base; without it those cells
/// stay walkable and buildable where the engine blocks them.
///
/// The two sibling sites use different filters — the overlay-claimed path
/// reclasses unconditionally and the valid-tile sparse/OOB-subtile fallback
/// filters `Clear` only. An already-invalid/sentinel tile id reaches this
/// final-normal filter, not the sparse-entry copy.
fn cliff_back_normal_reclass_applies(land_type: u8) -> bool {
    use crate::rules::terrain_rules::LandType;
    land_type == LandType::Clear.as_index()
        || land_type == LandType::Water.as_index()
        || land_type == LandType::Beach.as_index()
        || land_type == LandType::Ice.as_index()
}

fn apply_load_land_to_cell(
    cell: &mut ResolvedTerrainCell,
    land: LandType,
    speed_costs: Option<SpeedCostProfile>,
    ground_blocked: bool,
) {
    cell.land_type = land.as_index();
    cell.yr_cell_land_type = land.as_index();
    cell.terrain_class = land.terrain_class();
    cell.speed_costs = speed_costs.unwrap_or_default();
    cell.is_water = land.is_water();
    cell.is_cliff_like = land.is_cliff_like();
    cell.is_rough = land.is_rough();
    cell.is_road = land.is_road();
    cell.ground_walk_blocked = ground_blocked;
}

fn restore_load_base_land(cell: &mut ResolvedTerrainCell) {
    cell.land_type = cell.base_land_type;
    cell.yr_cell_land_type = cell.base_yr_cell_land_type;
    cell.terrain_class = cell.base_terrain_class;
    cell.speed_costs = cell.base_speed_costs;
    cell.is_water = cell.base_land_type == LandType::Water.as_index();
    cell.is_cliff_like = matches!(
        cell.base_land_type,
        land if land == LandType::Rock.as_index() || land == LandType::Wall.as_index()
    );
    cell.is_rough = cell.base_land_type == LandType::Rough.as_index();
    cell.is_road = cell.base_land_type == LandType::Road.as_index();
}

/// Current Cell+38 queries, separate from Recalc's retained pristine metadata.
/// Native smudge6B601A..6B603A falls back to tile0 for an invalid signed index;
/// CanPlaceTiberium4839C0..4839E9 admits that index at its final tile gate.
/// Other placement gates are outside this projection. Original-block witnesses:
/// tools/spatial_oracle/terrain_tile_permissions.
fn current_tile_permissions(lookup: &TilesetLookup, tile: i32) -> (bool, bool) {
    if tile < 0 || tile as usize >= lookup.len() {
        (lookup.is_morphable(0), true)
    } else {
        // The registry admits at most65535 slots; compare before narrowing.
        let tile = tile as u16;
        (lookup.is_morphable(tile), lookup.allows_tiberium(tile))
    }
}

fn apply_pristine_load_metadata(
    cell: &mut ResolvedTerrainCell,
    metadata: &TileMetadata,
    accepts_smudge: bool,
    allows_tiberium: bool,
) {
    cell.tileset_index = metadata.tileset_index;
    cell.land_type = metadata.land_type;
    cell.yr_cell_land_type = metadata.yr_cell_land_type;
    cell.slope_type = metadata.slope_type;
    cell.template_height = metadata.template_height;
    cell.height_in_pixels = metadata.height_in_pixels;
    cell.terrain_class = metadata.terrain_class;
    cell.speed_costs = metadata.speed_costs;
    cell.is_water = metadata.is_water;
    cell.is_cliff_like = metadata.is_cliff_like;
    cell.is_rough = metadata.is_rough;
    cell.is_road = metadata.is_road;
    cell.accepts_smudge = accepts_smudge;
    cell.allows_tiberium = allows_tiberium;
    cell.has_ramp = metadata.has_ramp;
    cell.canonical_ramp = canonical_ramp_from_slope_type(metadata.slope_type);
    cell.base_land_type = metadata.land_type;
    cell.base_yr_cell_land_type = metadata.yr_cell_land_type;
    cell.base_terrain_class = metadata.terrain_class;
    cell.base_speed_costs = metadata.speed_costs;
    cell.base_ground_walk_blocked = cell.canonical_ramp.is_none() && metadata.ground_blocked;
    cell.base_build_blocked = metadata.build_blocked || cell.canonical_ramp.is_some();
}

fn load_navigation_signature(
    cell: &ResolvedTerrainCell,
) -> (
    bool,
    Option<u8>,
    u8,
    u8,
    TerrainClass,
    SpeedCostProfile,
    bool,
    bool,
) {
    (
        cell.overlay_blocks,
        cell.overlay_zone_type,
        cell.zone_type,
        cell.land_type,
        cell.terrain_class,
        cell.speed_costs,
        cell.ground_walk_blocked,
        cell.build_blocked,
    )
}

fn ordinary_variant_selection_enabled(
    total_file_count: u8,
    uses_clear_fallback: bool,
    has_damaged_data: bool,
) -> bool {
    !has_damaged_data && total_file_count != 0 && (uses_clear_fallback || total_file_count > 1)
}

fn is_wood_bridge_repair_tile(theater_data: Option<&TheaterData>, final_tile_index: i32) -> bool {
    if final_tile_index < 0 || final_tile_index == 0xFFFF {
        return false;
    }
    let Some(td) = theater_data else {
        return false;
    };
    let Some(wood_bridge_set) = td.wood_bridge_set else {
        return false;
    };
    let Some(bounds) = td.lookup.bounds().get(wood_bridge_set as usize) else {
        return false;
    };
    let tile_id = normalize_tile_id(final_tile_index) as u32;
    let start = bounds.start as u32;
    tile_id >= start && tile_id < start + 16
}

fn tile_in_first_16_of_set(td: &TheaterData, set_index: Option<u16>, tile_id: u16) -> bool {
    let Some(set_index) = set_index else {
        return false;
    };
    let Some(bounds) = td.lookup.bounds().get(set_index as usize) else {
        return false;
    };
    tile_id >= bounds.start && tile_id < bounds.start.saturating_add(16)
}

const AUTO_TUBE_DIRECTIONS: [u8; 4] = [2, 4, 6, 0];

fn build_auto_low_bridge_tubes(
    cells: &mut [ResolvedTerrainCell],
    width: u16,
    height: u16,
    theater_data: Option<&TheaterData>,
    tubes: &mut Vec<TubeFact>,
) {
    for cell in cells.iter_mut() {
        if cell.yr_cell_land_type != YR_CELL_LAND_TUNNEL || cell.tube_index.is_some() {
            continue;
        }
        let Some(direction) = auto_tube_direction_for_tile(cell.final_tile_index, theater_data)
        else {
            continue;
        };
        let Some(_idx) = (cell.rx < width && cell.ry < height).then_some(()) else {
            continue;
        };
        let Ok(raw_id) = u16::try_from(tubes.len()) else {
            log::warn!(
                "ResolvedTerrain: tube registry exceeded u16::MAX; skipping tube at ({}, {})",
                cell.rx,
                cell.ry
            );
            continue;
        };
        let tube_id = TubeId(raw_id);
        tubes.push(TubeFact::automatic_recalc_shell(
            (cell.rx, cell.ry),
            direction,
        ));
        cell.tube_index = Some(tube_id);
    }
}

fn seed_explicit_map_tubes(
    cells: &mut [ResolvedTerrainCell],
    width: u16,
    height: u16,
    explicit_tubes: &[TubeFact],
) -> Vec<TubeFact> {
    let mut tubes = Vec::with_capacity(explicit_tubes.len());
    for tube in explicit_tubes {
        let Ok(raw_id) = u16::try_from(tubes.len()) else {
            log::warn!(
                "ResolvedTerrain: explicit [Tubes] registry exceeded u16::MAX; skipping remaining tubes"
            );
            break;
        };
        let tube_id = TubeId(raw_id);
        tubes.push(tube.clone());
        let (rx, ry) = tube.entry;
        if rx >= width || ry >= height {
            log::warn!(
                "ResolvedTerrain: explicit [Tubes] entry cell ({}, {}) outside resolved grid",
                rx,
                ry
            );
            continue;
        }
        let idx = ry as usize * width as usize + rx as usize;
        if let Some(cell) = cells.get_mut(idx) {
            cell.tube_index = Some(tube_id);
        }
    }
    tubes
}

fn auto_tube_direction_for_tile(
    final_tile_index: i32,
    theater_data: Option<&TheaterData>,
) -> Option<u8> {
    let td = theater_data?;
    auto_tube_direction_from_bases(final_tile_index, td.automatic_tube_bases)
}

fn auto_tube_direction_from_bases(final_tile_index: i32, bases: [i32; 4]) -> Option<u8> {
    for base in bases {
        if final_tile_index >= base && final_tile_index <= base.wrapping_add(3) {
            let ordinal = final_tile_index.wrapping_sub(base);
            if ordinal != -1 {
                return usize::try_from(ordinal)
                    .ok()
                    .and_then(|ordinal| AUTO_TUBE_DIRECTIONS.get(ordinal).copied());
            }
        }
    }
    None
}

/// A small asset-derived type catalogue, independent of visited cell identities.
/// Include every registered pristine type so LAT, cliff collapse and restore can
/// change a cell's tile without a render-thread asset lookup or stale scalar.
/// Only headers are read here; the large color/depth planes are not decoded.
fn load_native_tmp_draw_heights(
    theater: Option<&TheaterData>,
    assets: Option<&crate::assets::asset_manager::AssetManager>,
) -> HashMap<u16, PristineTmpHeader> {
    let (Some(theater), Some(assets)) = (theater, assets) else {
        return HashMap::new();
    };
    let mut heights = HashMap::new();
    for id in 0..theater.lookup.len() {
        let Ok(id) = u16::try_from(id) else {
            break;
        };
        let Some(name) = theater.lookup.filename(i32::from(id)) else {
            log::warn!("TMP draw dimensions unavailable: tile {id} has no registered filename");
            continue;
        };
        let Some(bytes) = assets.get(name) else {
            log::warn!("TMP draw dimensions unavailable: tile {id} asset {name} is missing");
            continue;
        };
        match TmpFile::pristine_header_fields_from_bytes(&bytes) {
            Ok(rows) => {
                heights.insert(
                    id,
                    PristineTmpHeader {
                        subtiles: rows,
                        total_file_count: theater.lookup.total_file_count(id) as usize,
                    },
                );
            }
            Err(error) => log::warn!("TMP draw dimensions unavailable for {name}: {error}"),
        }
    }
    heights
}

fn cached_tile_metadata(
    cache: &mut HashMap<TileKey, TileMetadata>,
    theater_data: Option<&TheaterData>,
    asset_manager: Option<&crate::assets::asset_manager::AssetManager>,
    terrain_rules: Option<&TerrainRules>,
    key: TileKey,
    warned_unknown_land_types: &mut HashSet<u8>,
) -> TileMetadata {
    if let Some(metadata) = cache.get(&key) {
        return metadata.clone();
    }
    let metadata = load_tile_metadata(
        theater_data,
        asset_manager,
        terrain_rules,
        key,
        warned_unknown_land_types,
    );
    cache.insert(key, metadata.clone());
    metadata
}

/// Apply only fields owned by the selected independent tactical TMP. Pixel
/// bounds, Z, and extra planes stay atlas-owned; this resolver carries the
/// selected cell's radar pair and render-origin offsets. Every CellClass field
/// remains on `pristine`.
fn apply_selected_presentation_metadata(pristine: &mut TileMetadata, selected: &TileMetadata) {
    pristine.radar_left = selected.radar_left;
    pristine.radar_right = selected.radar_right;
    pristine.subtile_entry_valid = selected.subtile_entry_valid;
    pristine.render_offset_x = selected.render_offset_x;
    pristine.render_offset_y = selected.render_offset_y;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct RadarColorMetadata {
    pub left: [u8; 3],
    pub right: [u8; 3],
    pub valid: bool,
}

fn damaged_variant_radar_metadata(
    cache: &mut HashMap<TileKey, TileMetadata>,
    theater_data: Option<&TheaterData>,
    asset_manager: Option<&crate::assets::asset_manager::AssetManager>,
    terrain_rules: Option<&TerrainRules>,
    pristine_key: TileKey,
    pristine: &TileMetadata,
    warned_unknown_land_types: &mut HashSet<u8>,
) -> Option<RadarColorMetadata> {
    if !pristine.has_damaged_data
        || !theater_data
            .is_some_and(|theater| theater.lookup.total_file_count(pristine_key.tile_id) >= 2)
    {
        return None;
    }
    let damaged = cached_tile_metadata(
        cache,
        theater_data,
        asset_manager,
        terrain_rules,
        TileKey {
            variant: 1,
            ..pristine_key
        },
        warned_unknown_land_types,
    );
    // A missing/corrupt independent file never enters native's variant chain,
    // so the selector wraps to pristine. A parsed file with no requested
    // subimage does enter the chain and GetRadarColor returns fixed gray.
    retained_damaged_radar_metadata(pristine, Some(&damaged))
}

fn retained_damaged_radar_metadata(
    pristine: &TileMetadata,
    damaged: Option<&TileMetadata>,
) -> Option<RadarColorMetadata> {
    let damaged =
        damaged.filter(|metadata| pristine.has_damaged_data && metadata.tmp_file_valid)?;
    Some(RadarColorMetadata {
        left: damaged.radar_left,
        right: damaged.radar_right,
        valid: damaged.subtile_entry_valid == Some(true),
    })
}

fn build_destroyable_cliff_catalog(
    theater_data: Option<&TheaterData>,
    asset_manager: Option<&crate::assets::asset_manager::AssetManager>,
    terrain_rules: Option<&TerrainRules>,
) -> Option<DestroyableCliffCatalog> {
    let td = theater_data?;
    let assets = asset_manager?;
    let destroyable_start = td.cliff_ranges.destroyable_cliffs?;
    let slope_start = td
        .slope_set_pieces
        .and_then(|set| td.lookup.bounds().get(usize::from(set)))?
        .start;
    let mut warned_unknown_land_types = HashSet::new();
    let mut load_template = |tile_id: u16| -> Option<SparseTileTemplate> {
        let filename = td.lookup.filename(i32::from(tile_id))?;
        let bytes = assets.get(filename)?;
        let tmp = TmpFile::from_bytes(&bytes).ok()?;
        let width = u8::try_from(tmp.template_width).ok()?;
        let height = u8::try_from(tmp.template_height).ok()?;
        let entries = tmp
            .tiles
            .iter()
            .enumerate()
            .map(|(sub_tile, entry)| {
                entry.as_ref()?;
                let sub_tile = u8::try_from(sub_tile).ok()?;
                Some(DynamicTilePrototype {
                    metadata: load_tile_metadata(
                        Some(td),
                        Some(assets),
                        terrain_rules,
                        TileKey {
                            tile_id,
                            sub_tile,
                            variant: 0,
                        },
                        &mut warned_unknown_land_types,
                    ),
                    accepts_smudge: td.lookup.is_morphable(tile_id),
                    allows_tiberium: td.lookup.allows_tiberium(tile_id),
                })
            })
            .collect();
        Some(SparseTileTemplate {
            tile_id,
            width,
            height,
            entries,
        })
    };
    Some(DestroyableCliffCatalog {
        destroyable_start,
        old: [
            load_template(destroyable_start)?,
            load_template(destroyable_start.checked_add(1)?)?,
        ],
        replacements: [
            load_template(slope_start)?,
            load_template(slope_start.checked_add(1)?)?,
            load_template(slope_start.checked_add(2)?)?,
            load_template(slope_start.checked_add(3)?)?,
        ],
        lat_config: lat::parse_lat_config(&td.ini_data, &td.lookup),
        slope_config: lat::SlopeFixupConfig {
            ramp_base: td.rmg_tiles.ramp_base.map_or(-1, i32::from),
            ramp_smooth: td.rmg_tiles.ramp_smooth.map_or(-1, i32::from),
        },
    })
}

fn stamp_dynamic_tile_identity(
    cell: &mut ResolvedTerrainCell,
    tile_id: u16,
    sub_tile: u8,
    prototype: &DynamicTilePrototype,
) {
    cell.final_tile_index = i32::from(tile_id);
    cell.final_sub_tile = sub_tile;
    cell.is_wood_bridge_repair_tile = false;
    cell.slope_type = prototype.metadata.slope_type;
    cell.level = (cell.level as i8).wrapping_add(prototype.metadata.template_height as i8) as u8;
    cell.overlay_blocks = false;
    cell.overlay_zone_type = None;
}

fn recalc_dynamic_tile_attributes(
    cell: &mut ResolvedTerrainCell,
    prototype: &DynamicTilePrototype,
) {
    let metadata = &prototype.metadata;
    cell.filled_clear = false;
    cell.tileset_index = metadata.tileset_index;
    cell.land_type = metadata.land_type;
    cell.yr_cell_land_type = metadata.yr_cell_land_type;
    cell.template_height = metadata.template_height;
    cell.height_in_pixels = metadata.height_in_pixels;
    cell.render_offset_x = metadata.render_offset_x;
    cell.render_offset_y = metadata.render_offset_y;
    cell.terrain_class = metadata.terrain_class;
    cell.speed_costs = metadata.speed_costs;
    cell.is_water = metadata.is_water;
    cell.is_cliff_like = metadata.is_cliff_like;
    cell.is_rough = metadata.is_rough;
    cell.is_road = metadata.is_road;
    cell.accepts_smudge = prototype.accepts_smudge;
    cell.allows_tiberium = prototype.allows_tiberium;
    cell.variant = 0;
    cell.has_ramp = metadata.has_ramp;
    cell.canonical_ramp = canonical_ramp_from_slope_type(metadata.slope_type);
    cell.base_ground_walk_blocked = metadata.ground_blocked;
    cell.base_build_blocked = metadata.build_blocked;
    cell.base_land_type = metadata.land_type;
    cell.base_yr_cell_land_type = metadata.yr_cell_land_type;
    cell.base_terrain_class = metadata.terrain_class;
    cell.base_speed_costs = metadata.speed_costs;
    cell.ground_walk_blocked = metadata.ground_blocked || cell.terrain_object_blocks;
    cell.build_blocked = metadata.build_blocked || cell.terrain_object_blocks;
    cell.zone_type = recalc_zone_type(
        cell.outside_playfield,
        None,
        cell.land_type,
        cell.speed_costs.wheel,
        cell.terrain_object_occupation,
    );
    cell.has_bridge_deck = false;
    cell.bridge_walkable = false;
    cell.bridge_transition = false;
    cell.bridge_deck_level = cell.level;
    cell.bridge_layer = None;
    cell.bridge_facts = BridgeCellFacts::default();
    cell.radar_left = metadata.radar_left;
    cell.radar_right = metadata.radar_right;
    cell.has_damaged_data = metadata.has_damaged_data;
    cell.bridgehead_anchor_class_at_load = None;
}

fn load_tile_metadata(
    theater_data: Option<&TheaterData>,
    asset_manager: Option<&crate::assets::asset_manager::AssetManager>,
    terrain_rules: Option<&TerrainRules>,
    key: TileKey,
    warned_unknown_land_types: &mut HashSet<u8>,
) -> TileMetadata {
    let Some(td) = theater_data else {
        return TileMetadata::default();
    };
    let Some(asset_manager) = asset_manager else {
        return metadata_from_set_name(
            td.lookup
                .tileset_index(key.tile_id)
                .and_then(|idx| td.lookup.set_name(idx)),
            td.lookup.tileset_index(key.tile_id),
        );
    };
    let tileset_index = td.lookup.tileset_index(key.tile_id);
    let set_name = tileset_index.and_then(|idx| td.lookup.set_name(idx));
    let mut metadata = metadata_from_set_name(set_name, tileset_index);

    let Some(filename) = td.lookup.filename_for_variant(key.tile_id, key.variant) else {
        mark_invalid_subtile_metadata(&mut metadata, terrain_rules);
        return metadata;
    };
    let Some(bytes) = asset_manager.get(filename) else {
        mark_invalid_subtile_metadata(&mut metadata, terrain_rules);
        return metadata;
    };
    let Ok(tmp) = TmpFile::from_bytes(&bytes) else {
        mark_invalid_subtile_metadata(&mut metadata, terrain_rules);
        return metadata;
    };
    metadata.tmp_file_valid = true;
    merge_tmp_file_metadata(
        &mut metadata,
        &tmp,
        key.sub_tile,
        terrain_rules,
        warned_unknown_land_types,
    );
    metadata
}

fn merge_tmp_file_metadata(
    metadata: &mut TileMetadata,
    tmp: &TmpFile,
    sub_tile: u8,
    terrain_rules: Option<&TerrainRules>,
    warned_unknown_land_types: &mut HashSet<u8>,
) {
    metadata.template_width_cells = tmp.template_width;
    metadata.template_height_cells = tmp.template_height;
    // CellClass::RecalcAttributes validates the requested entry before asking
    // GetSubtileLandType to decode it. The latter's modulo is therefore not an
    // active fallback for a sparse or positive-OOB Cell subtile.
    let entry_count = tmp
        .template_width
        .checked_mul(tmp.template_height)
        .and_then(|count| usize::try_from(count).ok());
    let tile = entry_count
        .filter(|&count| usize::from(sub_tile) < count)
        .and_then(|_| tmp.tiles.get(usize::from(sub_tile)))
        .and_then(|tile| tile.as_ref());
    metadata.subtile_entry_valid = Some(tile.is_some());
    let relative_extra_y = tile.map_or(0, |tile| tile.relative_extra_y);
    match height_in_pixels_from_tmp(tmp.tile_height, relative_extra_y) {
        Some(height_in_pixels) => metadata.height_in_pixels = height_in_pixels,
        None => log::warn!(
            "ResolvedTerrain: TMP effective height overflow (header {}, relative extra-Y {})",
            tmp.tile_height,
            relative_extra_y,
        ),
    }
    let Some(tile) = tile else {
        mark_invalid_subtile_metadata(metadata, terrain_rules);
        return;
    };
    merge_tmp_metadata(metadata, tile);
    apply_land_type_semantics(metadata, terrain_rules, warned_unknown_land_types);
}

fn mark_invalid_subtile_metadata(
    metadata: &mut TileMetadata,
    terrain_rules: Option<&TerrainRules>,
) {
    metadata.subtile_entry_valid = Some(false);
    metadata.has_tmp_metadata = false;
    metadata.raw_land_type = 0;
    metadata.slope_type = 0;
    metadata.template_height = 0;
    metadata.has_ramp = false;
    apply_canonical_land_to_metadata(metadata, LandType::Clear, None, false);
    metadata.build_blocked = false;
    if let Some(clear) =
        terrain_rules.and_then(|rules| rules.semantics_for_land_type(LandType::Clear.as_index()))
    {
        metadata.terrain_class = clear.terrain_class;
        metadata.speed_costs = clear.speed_costs;
        metadata.is_water = clear.water;
        metadata.is_cliff_like = clear.cliff_like;
        metadata.is_rough = clear.rough;
        metadata.is_road = clear.road;
        metadata.ground_blocked = clear.ground_blocked;
        metadata.build_blocked = !clear.buildable;
    }
}

fn height_in_pixels_from_tmp(tile_height: u32, relative_extra_y: i32) -> Option<i8> {
    let header_height = i32::try_from(tile_height).ok()?;
    let effective_height = header_height.checked_sub(relative_extra_y)?;
    let numerator = effective_height.checked_sub(30)?;
    Some((numerator / 15) as i8)
}

fn metadata_from_set_name(set_name: Option<&str>, tileset_index: Option<u16>) -> TileMetadata {
    let lower = set_name.unwrap_or("").to_ascii_lowercase();
    let is_water = lower.contains("water");
    let is_cliff_like = lower.contains("cliff") || lower.contains("rock");
    let is_rough = lower.contains("rough");
    let is_road = lower.contains("road") || lower.contains("pavement") || lower.contains("pave");
    let land_type = if is_water {
        crate::rules::terrain_rules::LandType::Water.as_index()
    } else if is_road {
        crate::rules::terrain_rules::LandType::Road.as_index()
    } else if is_rough {
        crate::rules::terrain_rules::LandType::Rough.as_index()
    } else if is_cliff_like {
        crate::rules::terrain_rules::LandType::Rock.as_index()
    } else {
        crate::rules::terrain_rules::LandType::Clear.as_index()
    };
    let terrain_class = if is_water {
        TerrainClass::Water
    } else if lower.contains("cliff") {
        TerrainClass::Cliff
    } else if lower.contains("rock") {
        TerrainClass::Rock
    } else if is_road {
        TerrainClass::Road
    } else if is_rough {
        TerrainClass::Rough
    } else if !lower.is_empty() {
        TerrainClass::Clear
    } else {
        TerrainClass::Unknown
    };

    TileMetadata {
        tileset_index,
        land_type,
        yr_cell_land_type: land_type,
        terrain_class,
        is_water,
        is_cliff_like,
        is_rough,
        is_road,
        ground_blocked: is_water || is_cliff_like,
        build_blocked: is_water || is_cliff_like,
        ..TileMetadata::default()
    }
}

fn merge_tmp_metadata(metadata: &mut TileMetadata, tile: &TmpTile) {
    metadata.subtile_entry_valid = Some(true);
    metadata.raw_land_type = tile.terrain_type;
    metadata.yr_cell_land_type = yr_cell_land_type_from_tmp(tile.terrain_type);
    metadata.land_type =
        crate::rules::terrain_rules::tmp_terrain_to_land_type(tile.terrain_type).as_index();
    metadata.slope_type = tile.ramp_type;
    metadata.template_height = tile.height;
    metadata.render_offset_x = tile.offset_x;
    metadata.render_offset_y = tile.offset_y;
    metadata.has_ramp = tile.ramp_type != 0;
    metadata.has_tmp_metadata = true;
    metadata.radar_left = tile.radar_left;
    metadata.radar_right = tile.radar_right;
    metadata.has_damaged_data = tile.has_damaged_data;
}

fn yr_cell_land_type_from_tmp(tmp_terrain_type: u8) -> u8 {
    if tmp_terrain_type == 5 {
        YR_CELL_LAND_TUNNEL
    } else {
        crate::rules::terrain_rules::tmp_terrain_to_land_type(tmp_terrain_type).as_index()
    }
}

/// Maps TMP ramp_type byte to canonical direction.
/// Values from TS++ TIBSUN_DEFINES.H. Tilt matrix angles:
/// 270 deg=W, 180 deg=N, 90 deg=E, 0 deg=S for slope types 1-4.
fn canonical_ramp_from_slope_type(slope_type: u8) -> Option<RampDirection> {
    match slope_type {
        1 => Some(RampDirection::West),
        2 => Some(RampDirection::North),
        3 => Some(RampDirection::East),
        4 => Some(RampDirection::South),
        _ => None,
    }
}

fn apply_land_type_semantics(
    metadata: &mut TileMetadata,
    terrain_rules: Option<&TerrainRules>,
    warned_unknown_land_types: &mut HashSet<u8>,
) {
    let Some(terrain_rules) = terrain_rules else {
        return;
    };
    if !metadata.has_tmp_metadata {
        return;
    }
    // Rules rows use the canonical LandType index. The raw TMP byte remains
    // diagnostic context only after the fixed TMP-to-land conversion has run.
    let Some(semantics) = terrain_rules
        .semantics_for_land_type(metadata.land_type)
        .copied()
    else {
        if warned_unknown_land_types.insert(metadata.raw_land_type) {
            log::warn!(
                "No terrain-rules row for canonical LandType {} converted from TMP byte {}; \
                 falling back to tileset-name heuristics",
                metadata.land_type,
                metadata.raw_land_type,
            );
        }
        return;
    };

    metadata.terrain_class = semantics.terrain_class;
    metadata.speed_costs = semantics.speed_costs;
    metadata.is_water = semantics.water;
    metadata.is_cliff_like = semantics.cliff_like;
    metadata.is_rough = semantics.rough;
    metadata.is_road = semantics.road;
    metadata.ground_blocked = semantics.ground_blocked;
    metadata.build_blocked = !semantics.buildable;
}

/// Replace every land-derived attribute of a cell with the overlay's `Land=` row.
///
/// Overlay land override: `CellClass__RecalcAttributes` @ `0x0047D2B0`. Its
/// entry branch loads `OverlayTypeClass+0x298` into `Cell->LandType` and, when
/// that land is Wall(4)/Railroad(9) or `+0x2AC` (`NoUseTileLandType`) is set,
/// runs the LAT fixup and zone recompute and **returns** — the tile's own
/// subtile land type is never consulted. LandType is the only land attribute
/// gamemd stores, so nothing can survive the swap; passability is re-derived
/// from it. `ground_blocked` is VERA's cache of that derivation and must be
/// replaced here too, or a low bridge (`Land=Road`, `NoUseTileLandType=yes`)
/// over water keeps the water tile's block and rejects every ground unit.
fn apply_canonical_land_to_metadata(
    metadata: &mut TileMetadata,
    land: LandType,
    speed_costs: Option<SpeedCostProfile>,
    ground_blocked: bool,
) {
    metadata.land_type = land.as_index();
    metadata.yr_cell_land_type = land.as_index();
    metadata.terrain_class = land.terrain_class();
    metadata.speed_costs = speed_costs.unwrap_or_default();
    metadata.is_water = land.is_water();
    metadata.is_cliff_like = land.is_cliff_like();
    metadata.is_rough = land.is_rough();
    metadata.is_road = land.is_road();
    metadata.ground_blocked = ground_blocked;
}

fn classify_overlay_effects(
    overlays: Option<&Vec<&OverlayEntry>>,
    overlay_registry: Option<&OverlayTypeRegistry>,
    level: u8,
    slope_type: u8,
) -> OverlayEffects {
    let mut result = OverlayEffects::default();
    let Some(entries) = overlays else {
        return result;
    };
    for overlay in entries {
        let name = overlay_registry
            .and_then(|reg| reg.name(overlay.overlay_id))
            .unwrap_or("");
        // Bridge overlays identified by hardcoded index, matching original engine.
        let is_bridge = crate::map::overlay_types::is_bridge_overlay_index(overlay.overlay_id);

        let flags = overlay_registry.and_then(|reg| reg.flags(overlay.overlay_id));
        if let Some(flags) = flags {
            // The gate on the early authoritative-land branch, evaluated before
            // the resource-overlay removal so it matches the engine's order.
            if uses_early_recalc_land_branch(flags) {
                result.claims_cell_attributes = true;
            }
            // The early branch copies Land before it clears a sloped resource.
            // The overlay pointer and its zone effects disappear, but this
            // invocation retains that already-copied current Cell land.
            if let Some(land) = retained_overlay_land(flags, slope_type) {
                result.effective_land = Some(land);
                result.effective_land_speed_costs = flags.land_speed_costs;
                result.effective_land_ground_blocked = flags.land_ground_blocked;
            }
            if clears_tiberium_on_slope(flags, slope_type) {
                continue;
            }
            result.overlay_zone_type = merge_overlay_zone_type(
                result.overlay_zone_type,
                overlay_reduced_zone_type(Some(flags)),
            );
            result.overlay_blocks = matches!(
                result.overlay_zone_type,
                Some(zone_class::WALL) | Some(zone_class::IMPASSABLE)
            );
        }
        if is_bridge && result.bridge_layer.is_none() {
            result.has_bridge_deck = true;
            // Direction determined by index: 24/237=EW, 25/238=NS, rest=Low.
            let direction = match overlay.overlay_id {
                24 | 237 => BridgeDirection::EastWest,
                25 | 238 => BridgeDirection::NorthSouth,
                _ => BridgeDirection::Low,
            };
            // High bridges: deck 4 levels above ground (HighBridgeHeight=4).
            // Low bridges: deck at ground level (no elevation change).
            let deck_level = match direction {
                BridgeDirection::EastWest | BridgeDirection::NorthSouth => level.saturating_add(4),
                BridgeDirection::Low => level,
            };
            if direction == BridgeDirection::Low {
                result.is_low_bridge = true;
            }
            result.bridge_layer = Some(BridgeLayer {
                overlay_id: overlay.overlay_id,
                overlay_name: name.to_string(),
                deck_level,
                direction,
            });
        }
    }
    result
}

fn merge_overlay_zone_type(current: Option<u8>, candidate: Option<u8>) -> Option<u8> {
    fn priority(zone_type: u8) -> u8 {
        match zone_type {
            zone_class::CRUSHABLE => 0,
            zone_class::WALL => 1,
            zone_class::IMPASSABLE => 2,
            zone_class::GROUND => 3,
            _ => u8::MAX,
        }
    }

    match (current, candidate) {
        (None, candidate) => candidate,
        (current, None) => current,
        (Some(current), Some(candidate)) => Some(if priority(candidate) < priority(current) {
            candidate
        } else {
            current
        }),
    }
}

fn wheel_speed_at_or_below_one_percent(wheel: Option<u8>) -> bool {
    wheel.is_some_and(|speed| speed <= 1)
}

/// Reproduce the active map-load order: initialize every allocated Size-diamond
/// CellClass, then let IsoMapPack records overwrite those cells in stream order.
fn selector_free_map_cells(map: &MapFile, lookup: Option<&TilesetLookup>) -> Vec<MapCell> {
    let translate_raw_pack_cells = !map.iso_map_pack_lookups.is_empty();
    map.cells
        .iter()
        .cloned()
        .map(|mut cell| {
            if translate_raw_pack_cells && let Some(lookup) = lookup {
                cell.tile_index = lookup.translate_legacy_map_tile_index(cell.tile_index);
            }
            cell
        })
        .collect()
}

fn materialize_map_load_cells(
    map: &MapFile,
    lookup: Option<&TilesetLookup>,
    selector: &mut TileVariantSelectionContext<'_, '_>,
    scenario_fill_ranged: &mut dyn FnMut(u32, u32) -> u32,
    shared_cell_dummy: &SharedCellDummy,
) -> Vec<MapCell> {
    let n = map.header.width;
    let m = map.header.height;
    let Some(max_coord) = n.checked_add(m) else {
        return Vec::new();
    };
    // The native owner is a fixed 512x512 CellClass lookup. Retail maps fit
    // this boundary; reject malformed dimensions instead of iterating an
    // unbounded signed-bit-pattern value from the INI.
    if n == 0 || m == 0 || max_coord > 512 {
        return Vec::new();
    }

    let is_water = map
        .header
        .fill
        .trim_matches(|ch: char| ch <= ' ')
        .eq_ignore_ascii_case("Water");
    let expected_count = m
        .checked_mul(n.saturating_mul(2).saturating_sub(1))
        .and_then(|count| usize::try_from(count).ok())
        .unwrap_or(0);
    let mut cells = Vec::with_capacity(expected_count);
    let mut index_by_coord = HashMap::with_capacity(expected_count);
    let last_sum = n + m * 2;

    // This sum-major form is the native iterator's anti-diagonal order: each
    // diagonal advances x and retreats y, beginning at (1, Size.Width).
    for sum in (n + 1)..=last_sum {
        for rx in 0..=sum {
            let ry = sum - rx;
            if rx.abs_diff(ry) >= n {
                continue;
            }
            let rx = u16::try_from(rx).expect("validated Size diamond x fits u16");
            let ry = u16::try_from(ry).expect("validated Size diamond y fits u16");
            let index = cells.len();
            cells.push(MapCell {
                rx,
                ry,
                tile_index: selector.draw_fill_tile_index(is_water, scenario_fill_ranged),
                sub_tile: 0,
                z: 0u8.wrapping_add(map.header.level as u8),
            });
            index_by_coord.insert((rx, ry), index);
        }
    }
    debug_assert_eq!(cells.len(), expected_count);

    // gamemd.exe IsoMapPack5 decoder @ 0x0056BAC0: every OOB or null-slot
    // lookup stamps its exact raw header into the fixed dummy's CellStruct
    // before consuming (but not applying) the payload. Valid lookups never
    // touch the dummy, so replaying only misses here preserves stream order's
    // last writer while keeping the parser pure and payload authority real-only.
    if map.iso_map_pack_lookups.is_empty() {
        // Synthetic/RMG constructors have no decoder trace. Retain their
        // established manual-cell behavior while giving canonical misses the
        // same dummy-coordinate side effect as a native lookup.
        for explicit in &map.cells {
            if !index_by_coord.contains_key(&(explicit.rx, explicit.ry)) {
                shared_cell_dummy.stamp_coord(i32::from(explicit.rx), i32::from(explicit.ry));
            }
        }
    } else {
        for lookup in &map.iso_map_pack_lookups {
            let misses_real_slot = lookup
                .canonical
                .is_none_or(|coord| !index_by_coord.contains_key(&coord));
            if misses_real_slot {
                shared_cell_dummy.stamp_coord(i32::from(lookup.raw_x), i32::from(lookup.raw_y));
            }
        }
    }

    let translate_raw_pack_cells = !map.iso_map_pack_lookups.is_empty();
    for explicit in &map.cells {
        let Some(&index) = index_by_coord.get(&(explicit.rx, explicit.ry)) else {
            continue;
        };
        let mut materialized = explicit.clone();
        if translate_raw_pack_cells && let Some(lookup) = lookup {
            // Retail provenance: Pack5 callsite 0x0056BB68 reaches
            // `CalculateLegacyMapTileIndex @ 0x00544E30` only after the
            // destination CellClass was accepted, and before Cell+0x38 becomes
            // authoritative for variant/LAT/Recalc consumers. Fill, RMG, and
            // manual/runtime actual IDs never cross this seam.
            materialized.tile_index =
                lookup.translate_legacy_map_tile_index(materialized.tile_index);
        }
        cells[index] = materialized;
    }
    cells
}

#[cfg(test)]
#[path = "resolved_terrain_damaged_radar_tests.rs"]
mod damaged_radar_tests;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::assets::asset_manager::{AssetManager, MediaArchiveMode};
    use crate::assets::mix_hash::mix_hash;
    use crate::assets::tmp_file::TmpTile;
    use crate::map::overlay::{OverlayDataPack, TerrainObject};
    use crate::map::overlay_types::OverlayTypeRegistry;
    use crate::map::tube_facts::TubeSource;
    use crate::rules::ini_parser::IniFile;
    use crate::rules::ruleset::RuleSet;
    use crate::rules::terrain_rules::{TerrainClass, TerrainRules};
    use crate::sim::rng::SimRng;
    use std::collections::HashSet;
    use std::path::{Path, PathBuf};
    use std::time::{SystemTime, UNIX_EPOCH};

    struct Gsi0404AssetDirectory(PathBuf);

    impl Gsi0404AssetDirectory {
        fn new() -> Self {
            static NEXT_DIRECTORY: std::sync::atomic::AtomicU64 =
                std::sync::atomic::AtomicU64::new(0);
            let sequence = NEXT_DIRECTORY.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            let nonce = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("system clock follows Unix epoch")
                .as_nanos();
            let path = std::env::temp_dir().join(format!(
                "vera20k-gsi-04-04-{}-{nonce}-{sequence}",
                std::process::id()
            ));
            std::fs::create_dir(&path).expect("create GSI-04.04 asset directory");
            Self(path)
        }

        fn path(&self) -> &Path {
            &self.0
        }

        fn write(&self, name: &str, bytes: &[u8]) {
            std::fs::write(self.0.join(name), bytes).expect("write GSI-04.04 asset fixture");
        }
    }

    impl Drop for Gsi0404AssetDirectory {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn gsi_04_04_mix_bytes(entry_name: &str, body: &[u8]) -> Vec<u8> {
        let mut data = Vec::new();
        data.extend_from_slice(&0u16.to_le_bytes());
        data.extend_from_slice(&0u16.to_le_bytes());
        data.extend_from_slice(&1u16.to_le_bytes());
        data.extend_from_slice(&(body.len() as u32).to_le_bytes());
        data.extend_from_slice(&mix_hash(entry_name).to_le_bytes());
        data.extend_from_slice(&0u32.to_le_bytes());
        data.extend_from_slice(&(body.len() as u32).to_le_bytes());
        data.extend_from_slice(body);
        data
    }

    fn gsi_04_04_asset_manager_with_loose_tmp(
        tmp_filename: &str,
        tmp_bytes: &[u8],
    ) -> (Gsi0404AssetDirectory, AssetManager) {
        let directory = Gsi0404AssetDirectory::new();
        let empty_mix = gsi_04_04_mix_bytes("gsi0404.bin", b"fixture");
        for name in [
            "ra2md.mix",
            "ra2.mix",
            "cachemd.mix",
            "cache.mix",
            "localmd.mix",
            "local.mix",
            "conqmd.mix",
            "conquer.mix",
            "cameomd.mix",
            "cameo.mix",
            "mapsmd03.mix",
            "multimd.mix",
            "movmd03.mix",
        ] {
            directory.write(name, &empty_mix);
        }
        directory.write(tmp_filename, tmp_bytes);
        let manager = AssetManager::new_with_media_mode(
            directory.path(),
            MediaArchiveMode::Numbered { media_index: 2 },
        )
        .expect("construct synthetic retail archive stack");
        (directory, manager)
    }

    fn gsi_04_04_sparse_tmp_bytes() -> Vec<u8> {
        let mut data = Vec::new();
        data.extend_from_slice(&2u32.to_le_bytes());
        data.extend_from_slice(&1u32.to_le_bytes());
        data.extend_from_slice(&88u32.to_le_bytes());
        data.extend_from_slice(&45u32.to_le_bytes());
        data.extend_from_slice(&0u32.to_le_bytes());
        data.extend_from_slice(&0u32.to_le_bytes());
        data
    }

    fn gsi_04_02_last_tiles_tmp_bytes(
        terrain_type: u8,
        radar_left: [u8; 3],
        radar_right: [u8; 3],
    ) -> Vec<u8> {
        let mut data = Vec::new();
        data.extend_from_slice(&1u32.to_le_bytes());
        data.extend_from_slice(&1u32.to_le_bytes());
        data.extend_from_slice(&8u32.to_le_bytes());
        data.extend_from_slice(&4u32.to_le_bytes());
        data.extend_from_slice(&20u32.to_le_bytes());
        data.extend_from_slice(&[0u8; 40]);
        data.push(3);
        data.push(terrain_type);
        data.push(0);
        data.extend_from_slice(&radar_left);
        data.extend_from_slice(&radar_right);
        data.extend_from_slice(&[0u8; 3]);
        data.extend(1u8..=16);
        data
    }

    fn gsi_04_02_last_tiles_theater() -> TheaterData {
        synthetic_theater_from_ini(
            b"[General]\nRoughTile=2\nClearToRoughLat=3\n\
              [TileSet0000]\nTilesInSet=3\nFileName=base\nSetName=Clear\n\
              [TileSet0001]\nTilesInSet=5\nLastTilesInSet=2\nFileName=old\nSetName=Clear\n\
              [TileSet0002]\nTilesInSet=1\nFileName=rough\nSetName=Rough\n\
              [TileSet0003]\nTilesInSet=16\nFileName=lat\nSetName=Rough LAT\n",
        )
    }

    fn gsi_04_02_asset_manager_with_loose_tmps(
        files: &[(&str, &[u8])],
    ) -> (Gsi0404AssetDirectory, AssetManager) {
        let directory = Gsi0404AssetDirectory::new();
        let empty_mix = gsi_04_04_mix_bytes("gsi0402.bin", b"fixture");
        for name in [
            "ra2md.mix",
            "ra2.mix",
            "cachemd.mix",
            "cache.mix",
            "localmd.mix",
            "local.mix",
            "conqmd.mix",
            "conquer.mix",
            "cameomd.mix",
            "cameo.mix",
            "mapsmd03.mix",
            "multimd.mix",
            "movmd03.mix",
        ] {
            directory.write(name, &empty_mix);
        }
        for &(name, bytes) in files {
            directory.write(name, bytes);
        }
        let manager = AssetManager::new_with_media_mode(
            directory.path(),
            MediaArchiveMode::Numbered { media_index: 2 },
        )
        .expect("construct compatibility-translation asset stack");
        (directory, manager)
    }

    fn make_map(
        cells: Vec<MapCell>,
        overlays: Vec<OverlayEntry>,
        terrain_objects: Vec<TerrainObject>,
    ) -> MapFile {
        MapFile {
            header: crate::map::map_file::MapHeader {
                theater: "TEMPERATE".to_string(),
                fill: "Clear".to_string(),
                level: 0,
                width: 4,
                height: 4,
                local_left: 0,
                local_top: 0,
                local_width: 4,
                local_height: 4,
            },
            basic: crate::map::basic::BasicSection::default(),
            briefing: crate::map::briefing::BriefingSection::default(),
            preview: crate::map::preview::PreviewSection::default(),
            cells,
            iso_map_pack_lookups: Vec::new(),
            entities: Vec::new(),
            overlays,
            authored_overlay_packs: crate::map::map_file::AuthoredOverlayPackSlot::empty(),
            smudges: Vec::new(),
            terrain_objects,
            waypoints: HashMap::new(),
            cell_tags: HashMap::new(),
            tags: HashMap::new(),
            triggers: HashMap::new(),
            events: HashMap::new(),
            actions: HashMap::new(),
            local_variables: HashMap::new(),
            trigger_graph: crate::map::trigger_graph::TriggerGraph::default(),
            special_flags: crate::map::basic::SpecialFlagsSection::default(),
            explicit_tubes: Vec::new(),
            ini: IniFile::from_str(""),
        }
    }

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    struct ProductionBuildStats {
        fill_calls: usize,
        water_advances: usize,
        generated_selector_table: bool,
        main_draws: usize,
    }

    fn build_production_grid(
        map: &MapFile,
        theater_data: Option<&TheaterData>,
        cache: &mut crate::map::tile_variant_selector::TileVariantSelectorCache,
        main_draw: &mut dyn FnMut() -> u32,
    ) -> (ResolvedTerrainGrid, ProductionBuildStats) {
        let mut fill_calls = 0usize;
        let mut scenario_fill_ranged = |_low, _high| {
            fill_calls += 1;
            0
        };
        let mut selector = cache.begin_load(main_draw);
        let grid = ResolvedTerrainGrid::build_with_variant_selector(
            map,
            theater_data,
            None,
            None,
            None,
            None,
            false,
            0,
            &mut scenario_fill_ranged,
            &mut selector,
        );
        let stats = ProductionBuildStats {
            fill_calls,
            water_advances: selector.map_fill_scenario_advance_count(),
            generated_selector_table: selector.generated_table(),
            main_draws: selector.raw_draw_count(),
        };
        (grid, stats)
    }

    fn build_production_grid_without_theater(
        map: &MapFile,
        cache: &mut crate::map::tile_variant_selector::TileVariantSelectorCache,
    ) -> (ResolvedTerrainGrid, ProductionBuildStats) {
        let mut unused_main_draw = || 0;
        build_production_grid(map, None, cache, &mut unused_main_draw)
    }

    /// `MapClass::Resize @ 0x00565C10` owns a fixed 512x512 table and allocates
    /// exactly `M * (2N - 1)` Size-diamond cells. Rust deliberately rejects the
    /// first malformed dimension sum that would cross that native capacity.
    #[test]
    fn gsi_04_01_production_size_diamond_closes_exact_capacity_edge() {
        const N: u32 = 511;
        const M: u32 = 1;
        const HIGHEST_ALLOCATED: (i32, i32) = (2, 511);

        let mut cache = crate::map::tile_variant_selector::TileVariantSelectorCache::default();
        let mut edge = make_map(Vec::new(), Vec::new(), Vec::new());
        edge.header.width = N;
        edge.header.height = M;
        let (grid, stats) = build_production_grid_without_theater(&edge, &mut cache);

        let expected_count = M as usize * (2 * N as usize - 1);
        assert_eq!(expected_count, 1_021);
        let highest_index =
            crate::map::cell_index::cell_linear_index(HIGHEST_ALLOCATED.0, HIGHEST_ALLOCATED.1)
                .expect("capacity-edge coordinate has a fixed-stride slot");
        let highest_index_usize =
            usize::try_from(highest_index).expect("fixed-stride table index fits usize");
        let allocated = grid
            .native_allocated
            .as_ref()
            .expect("production build records native allocation membership");
        assert_eq!((grid.width(), grid.height()), (512, 512));
        assert_eq!(grid.iter().count(), expected_count);
        assert_eq!(stats.fill_calls, expected_count);
        assert_eq!(
            allocated.iter().filter(|&&slot| slot).count(),
            expected_count
        );
        let fixed_stride = usize::try_from(crate::map::cell_index::CELL_ROW_STRIDE)
            .expect("native cell stride fits usize");
        let fixed_slot = |x: u32, y: u32| {
            usize::try_from(y).expect("native y fits usize") * fixed_stride
                + usize::try_from(x).expect("native x fits usize")
        };
        let mut expected_slots = vec![false; fixed_stride * 512];
        for x in 1u32..=511 {
            let y = 512 - x;
            let slot = fixed_slot(x, y);
            assert!(!expected_slots[slot], "duplicate sum-512 slot ({x}, {y})");
            expected_slots[slot] = true;
        }
        for x in 2u32..=511 {
            let y = 513 - x;
            let slot = fixed_slot(x, y);
            assert!(!expected_slots[slot], "duplicate sum-513 slot ({x}, {y})");
            expected_slots[slot] = true;
        }
        assert_eq!(expected_slots.iter().filter(|&&slot| slot).count(), 1_021);
        for y in 0u32..512 {
            for x in 0u32..512 {
                let slot = fixed_slot(x, y);
                let expected = expected_slots[slot];
                assert_eq!(
                    allocated[slot], expected,
                    "native allocation membership at ({x}, {y}), fixed slot {slot}"
                );
                let cell_x = u16::try_from(x).expect("native x fits CellClass coordinate");
                let cell_y = u16::try_from(y).expect("native y fits CellClass coordinate");
                assert_eq!(
                    grid.cell(cell_x, cell_y).is_some(),
                    expected,
                    "grid lookup membership at ({x}, {y}), fixed slot {slot}"
                );
            }
        }
        assert_eq!(
            allocated.iter().rposition(|&slot| slot),
            Some(highest_index_usize)
        );
        assert_eq!(
            highest_index,
            511 * crate::map::cell_index::CELL_ROW_STRIDE + 2
        );
        assert!(grid.cell(2, 511).is_some());
        assert!(grid.cell(3, 511).is_none());

        let mut oversized = make_map(Vec::new(), Vec::new(), Vec::new());
        oversized.header.width = N + 1;
        oversized.header.height = M;
        let (rejected, rejected_stats) =
            build_production_grid_without_theater(&oversized, &mut cache);
        assert_eq!((rejected.width(), rejected.height()), (0, 0));
        assert!(rejected.cells.is_empty());
        assert_eq!(rejected.iter().count(), 0);
        assert!(
            rejected
                .native_allocated
                .as_ref()
                .is_some_and(Vec::is_empty)
        );
        assert_eq!(rejected_stats.fill_calls, 0);
        assert!(rejected.cell(0, 0).is_none());
    }

    /// `MapClass::Clear @ 0x00565B00` destroys and nulls every prior slot before
    /// an ordinary `MapClass::Resize @ 0x00565C10`, so a later smaller load must
    /// expose only its own cells and membership. Resize then reconstructs the
    /// fallback CellClass at its fixed `0x00ABDC50` address: the identity
    /// survives, but its constructor-owned contents do not.
    #[test]
    fn gsi_04_01_smaller_production_load_replaces_larger_grid_without_stale_state() {
        let mut cache = crate::map::tile_variant_selector::TileVariantSelectorCache::default();
        let mut larger = make_map(
            vec![MapCell {
                rx: 4,
                ry: 8,
                tile_index: theater::NO_TILE,
                sub_tile: 0,
                z: 0,
            }],
            Vec::new(),
            Vec::new(),
        );
        larger.header.width = 5;
        larger.header.height = 4;
        larger.header.fill = "Water".to_string();
        let theater = synthetic_theater_from_ini(
            b"[TileSet0000]\nTilesInSet=1\nFileName=clear\nSetName=Clear\n",
        );
        let mut main_rng = SimRng::new(0x0401_5EED);
        let mut main_draw = || main_rng.next_u32();
        let process_dummy = SharedCellDummy::fresh();

        process_dummy.reconstruct_for_map_resize();
        let (mut current, larger_stats) =
            build_production_grid(&larger, Some(&theater), &mut cache, &mut main_draw);
        current.bind_shared_cell_dummy(process_dummy.clone());
        assert_eq!(current.iter().count(), 4 * (2 * 5 - 1));
        assert_eq!(larger_stats.fill_calls, 36);
        assert_eq!(larger_stats.water_advances, 36);
        assert!(larger_stats.generated_selector_table);
        assert!(larger_stats.main_draws > 0);
        assert!(cache.is_initialized());
        assert!(current.cell(4, 8).is_some());
        current
            .cell_mut(4, 8)
            .expect("larger-only allocated cell")
            .level = 77;
        current.test_set_dummy_cell_level_slope(-7, 11);
        current.stamp_dummy_cell_requested_coord(-7, 11);
        let larger_dummy = current.shared_cell_dummy();

        let mut smaller = make_map(Vec::new(), Vec::new(), Vec::new());
        smaller.header.width = 2;
        smaller.header.height = 1;
        process_dummy.reconstruct_for_map_resize();
        let (mut smaller_grid, smaller_stats) =
            build_production_grid_without_theater(&smaller, &mut cache);
        smaller_grid.bind_shared_cell_dummy(process_dummy);
        assert!(larger_dummy.same_identity(&smaller_grid.shared_cell_dummy()));
        current = smaller_grid;

        let allocated_coords: Vec<_> = current.iter().map(|cell| (cell.rx, cell.ry)).collect();
        assert_eq!(allocated_coords, vec![(2, 1), (1, 2), (2, 2)]);
        assert_eq!((current.width(), current.height()), (3, 3));
        assert_eq!(current.cells.len(), 9);
        assert_eq!(current.iter().count(), 3);
        assert_eq!(
            current
                .native_allocated
                .as_ref()
                .expect("second production membership")
                .iter()
                .filter(|&&slot| slot)
                .count(),
            3
        );
        assert!(current.cell(1, 1).is_none());
        assert!(current.cell(4, 8).is_none());
        assert_eq!(current.dummy_cell_requested_coord(), (0, 0));
        assert_eq!(current.dummy_cell_level_slope(), (0, 0));

        assert_eq!(smaller_stats.fill_calls, 3);
        assert_eq!(smaller_stats.water_advances, 0);
        assert!(!smaller_stats.generated_selector_table);
        assert_eq!(smaller_stats.main_draws, 0);
        assert!(cache.is_initialized());
    }

    fn synthetic_theater_with_wood_bridge_set() -> TheaterData {
        let ini = b"[TileSet0000]\nTilesInSet=10\nFileName=clear\nSetName=Clear\n\n\
                    [TileSet0001]\nTilesInSet=20\nFileName=wood\nSetName=Wood Bridge\n";
        let lookup = crate::map::theater::parse_tileset_ini(ini, "tem").unwrap();
        let empty_palette = crate::assets::pal_file::Palette::from_bytes(&[0u8; 768])
            .expect("768-byte zero palette parses");
        TheaterData {
            lookup,
            iso_palette: empty_palette.clone(),
            unit_palette: empty_palette.clone(),
            tiberium_palette: empty_palette,
            extension: "tem",
            ini_data: Vec::new(),
            bridge_set: None,
            wood_bridge_set: Some(1),
            slope_set_pieces: None,
            slope_set_pieces2: None,
            bridge_top_left_1: None,
            bridge_top_left_2: None,
            bridge_bottom_right_1: None,
            bridge_bottom_right_2: None,
            bridge_top_right_1: None,
            bridge_top_right_2: None,
            bridge_bottom_left_1: None,
            bridge_bottom_left_2: None,
            bridge_middle_1: None,
            bridge_middle_2: None,
            tunnels: None,
            track_tunnels: None,
            dirt_tunnels: None,
            dirt_track_tunnels: None,
            automatic_tube_bases: [-1; 4],
            cliff_ranges: crate::map::theater::TheaterCliffRanges::default(),
            rmg_tiles: crate::map::theater::RmgTileKeys::default(),
        }
    }

    fn synthetic_theater_from_ini(ini: &[u8]) -> TheaterData {
        let lookup = crate::map::theater::parse_tileset_ini(ini, "tem").unwrap();
        let empty_palette = crate::assets::pal_file::Palette::from_bytes(&[0u8; 768])
            .expect("768-byte zero palette parses");
        TheaterData {
            lookup,
            iso_palette: empty_palette.clone(),
            unit_palette: empty_palette.clone(),
            tiberium_palette: empty_palette,
            extension: "tem",
            ini_data: ini.to_vec(),
            bridge_set: None,
            wood_bridge_set: None,
            slope_set_pieces: None,
            slope_set_pieces2: None,
            bridge_top_left_1: None,
            bridge_top_left_2: None,
            bridge_bottom_right_1: None,
            bridge_bottom_right_2: None,
            bridge_top_right_1: None,
            bridge_top_right_2: None,
            bridge_bottom_left_1: None,
            bridge_bottom_left_2: None,
            bridge_middle_1: None,
            bridge_middle_2: None,
            tunnels: None,
            track_tunnels: None,
            dirt_tunnels: None,
            dirt_track_tunnels: None,
            automatic_tube_bases: [-1; 4],
            cliff_ranges: crate::map::theater::TheaterCliffRanges::default(),
            rmg_tiles: crate::map::theater::RmgTileKeys::default(),
        }
    }

    #[test]
    fn native_tmp_catalog_covers_unused_pristine_types_and_live_subtile_replacements() {
        let mut theater = synthetic_theater_from_ini(
            b"[TileSet0000]\nTilesInSet=1\nFileName=clear\nSetName=Clear\n\
              [TileSet0001]\nTilesInSet=1\nFileName=unused\nSetName=Unused\n\
              [TileSet0002]\nTilesInSet=1\nFileName=bad\nSetName=Bad\n\
              [TileSet0003]\nTilesInSet=1\nFileName=missing\nSetName=Missing\n",
        );
        let clear = gsi_04_02_last_tiles_tmp_bytes(0, [1, 2, 3], [4, 5, 6]);
        // Three pristine slots: heights 36, sparse/base 30, and 37. There are
        // deliberately no image planes: this unused type only needs headers.
        let mut unused = vec![0u8; 28 + 2 * 52];
        for (offset, value) in [
            (0, 3u32),
            (4, 1),
            (8, 60),
            (12, 30),
            (16, 28),
            (24, 80),
            (28 + 4, 100),
            (28 + 24, 94),
            (28 + 36, 1),
            (80 + 4, 100),
            (80 + 24, 93),
            (80 + 36, 1),
        ] {
            unused[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
        }
        let mut variant = gsi_04_04_sparse_tmp_bytes();
        variant[12..16].copy_from_slice(&99u32.to_le_bytes());
        let (_directory, assets) = gsi_04_02_asset_manager_with_loose_tmps(&[
            ("clear01.tem", &clear),
            ("unused01.tem", &unused),
            ("unused01a.tem", &variant),
            ("bad01.tem", b"truncated"),
            ("unregistered01.tem", &variant),
        ]);
        crate::map::theater::resolve_contiguous_variant_chains_for_test(
            &mut theater.lookup,
            &assets,
        );
        assert_eq!(
            theater.lookup.variant_count(1),
            1,
            "fixture has a real suffix variant"
        );
        let map = make_map(
            vec![MapCell {
                rx: 1,
                ry: 1,
                tile_index: 0,
                sub_tile: 0,
                z: 0,
            }],
            vec![],
            vec![],
        );
        let mut grid =
            ResolvedTerrainGrid::build(&map, Some(&theater), Some(&assets), None, None, false, 0);
        assert!(grid.cells().iter().all(|cell| cell.final_tile_index != 1));
        assert_eq!(
            grid.native_tmp_draw_heights.len(),
            2,
            "only registered readable pristine headers are cached"
        );
        for (subtile, expected) in [(0, 36), (1, 30), (2, 37), (4, 30), (255, 36)] {
            assert_eq!(grid.native_tmp_draw_height(1, subtile), expected);
        }
        assert_eq!(
            grid.native_tmp_draw_height(2, 0),
            30,
            "corrupt type fallback"
        );
        assert_eq!(
            grid.native_tmp_draw_height(3, 0),
            30,
            "missing type fallback"
        );
        for sentinel in [-1, 0xff, 0xffff] {
            assert_eq!(
                grid.native_tmp_draw_height(sentinel, 5),
                4,
                "clear sentinel always selects pristine subtile zero"
            );
        }

        let cell = grid.cell_mut(1, 1).unwrap();
        cell.final_tile_index = 1;
        cell.final_sub_tile = 5;
        cell.variant = 1;
        let cell = grid.cell(1, 1).unwrap();
        assert_eq!(
            grid.native_tmp_draw_height(cell.final_tile_index, cell.final_sub_tile),
            37,
            "a live replacement resolves its unused pristine type and modulo, independently of selected variant 99"
        );
    }

    #[test]
    fn projectile_level_uses_loaded_theater_interval_through_runtime() {
        use crate::sim::projectile::*;
        use crate::sim::runtime::SimRuntime;
        use crate::sim::world::{Simulation, TickLane};
        let ini=b"[General]\nWaterSet=1\n[TileSet0000]\nTilesInSet=1\nFileName=wet\nSetName=Water\n[TileSet0001]\nTilesInSet=14\nFileName=band\nSetName=Clear\n";
        let palette = [0u8; 768];
        let wet = gsi_04_02_last_tiles_tmp_bytes(2, [1, 2, 3], [4, 5, 6]);
        let dry = gsi_04_02_last_tiles_tmp_bytes(0, [1, 2, 3], [4, 5, 6]);
        let (directory, mut assets) = gsi_04_02_asset_manager_with_loose_tmps(&[
            ("temperatmd.ini", ini),
            ("isotem.pal", &palette),
            ("unittem.pal", &palette),
            ("temperat.pal", &palette),
            ("wet01.tem", &wet),
            ("band01.tem", &dry),
        ]);
        let empty_mix = gsi_04_04_mix_bytes("fixture.bin", b"fixture");
        for name in ["temperat.mix", "tem.mix", "isotemmd.mix", "isotemp.mix"] {
            directory.write(name, &empty_mix);
        }
        let theater = crate::map::theater::load_theater(&mut assets, "TEMPERATE")
            .expect("production theater loader");
        assert_eq!(theater.rmg_tiles.water_set, Some(1));
        for (tile, semantic_water, admit) in [(0, true, true), (1, false, false)] {
            let map = make_map(
                vec![MapCell {
                    rx: 3,
                    ry: 3,
                    tile_index: tile,
                    sub_tile: 0,
                    z: 0,
                }],
                Vec::new(),
                Vec::new(),
            );
            let mut cache = crate::map::tile_variant_selector::TileVariantSelectorCache::default();
            let mut draw = || 0u32;
            let mut selector = cache.begin_load(&mut draw);
            let mut fill = |_: u32, _: u32| 0u32;
            let mut grid = ResolvedTerrainGrid::build_with_variant_selector(
                &map,
                Some(&theater),
                Some(&assets),
                None,
                None,
                None,
                false,
                0,
                &mut fill,
                &mut selector,
            );
            assert_eq!(grid.projectile_water_set_base(), 1);
            assert_eq!(grid.cell(3, 3).unwrap().is_water, semantic_water);
            assert_eq!(grid.cell(3, 3).unwrap().final_tile_index, tile);
            let mut sim = Simulation::new();
            sim.install_playfield_from_map_header(&map.header);
            grid.bind_shared_cell_dummy(sim.shared_cell_dummy.clone());
            sim.resolved_terrain = Some(grid);
            let key = sim.interner.intern("TEST");
            sim.admit_projectile(
                100,
                ProjectileSpawn {
                    source_id: 999,
                    origin: ProjectileCoord::new(896, 896, 500),
                    target: ProjectileTarget::None,
                    initial_target_position: ProjectileCoord::new(1408, 896, 0),
                    payload: ProjectilePayload {
                        base_damage: 0,
                        warhead: key,
                        weapon: key,
                        owner: key,
                    },
                    speed_leptons_per_frame: 16,
                    velocity: ProjectileVelocity::new(16, 0, 0),
                    trajectory: ProjectileTrajectory::Ballistic,
                    guidance: None,
                    visual: ProjectileVisualState::new(0, 0, 0),
                    arm_frames: 0,
                    fuse_frames: None,
                    ranged_fuse: false,
                    tracks_target: false,
                    target_expiry: TargetExpiryPolicy::Expire,
                    collision: ProjectileCollisionPolicy {
                        level_non_water: true,
                        ..ProjectileCollisionPolicy::NONE
                    },
                },
            );
            let mut runtime = SimRuntime::from_simulation(sim);
            let _ = runtime
                .advance_frame(&[], 16, TickLane::Ordinary)
                .expect("fixture frame must complete");
            assert_eq!(
                runtime.simulation.projectiles.get(100).is_none(),
                admit,
                "tile{tile}: semantic water is not the native Level predicate"
            );
        }
    }

    fn synthetic_automatic_shell_theater() -> TheaterData {
        let mut theater = synthetic_theater_from_ini(
            b"[TileSet0000]\nTilesInSet=5\nFileName=tunnel\nSetName=Tunnels\n\n\
              [TileSet0001]\nTilesInSet=5\nFileName=track\nSetName=Track Tunnels\n\n\
              [TileSet0002]\nTilesInSet=5\nFileName=dirt\nSetName=Dirt Tunnels\n\n\
              [TileSet0003]\nTilesInSet=5\nFileName=dtunn\nSetName=Dirt Track Tunnels\n",
        );
        theater.tunnels = Some(0);
        theater.track_tunnels = Some(1);
        theater.dirt_tunnels = Some(2);
        theater.dirt_track_tunnels = Some(3);
        theater.automatic_tube_bases = [0, 5, 10, 15];
        theater
    }

    fn make_test_cell(rx: u16, ry: u16) -> ResolvedTerrainCell {
        test_flat_cell(rx, ry)
    }

    #[derive(Default)]
    struct LoadRecalcTestEffects {
        tube_requests: Vec<AutomaticTubeRequest>,
        tube_allocation: Option<AutomaticTubeAllocation>,
        requests: Vec<TerrainTileAnimation>,
        fail: bool,
    }

    impl LoadCellRecalcEffects for LoadRecalcTestEffects {
        type Error = &'static str;

        fn allocate_automatic_tube(
            &mut self,
            request: AutomaticTubeRequest,
        ) -> Result<AutomaticTubeAllocation, Self::Error> {
            self.tube_requests.push(request);
            Ok(self
                .tube_allocation
                .unwrap_or(AutomaticTubeAllocation::AllocationNull))
        }

        fn construct_terrain_attached_anim(
            &mut self,
            request: &TerrainTileAnimation,
        ) -> Result<(), Self::Error> {
            self.requests.push(request.clone());
            if self.fail {
                Err("fixture failure")
            } else {
                Ok(())
            }
        }
    }

    #[test]
    fn gsi_04_02_last_tiles_pack_materialization_precedes_tmp_lat_and_final_state() {
        let mut theater = gsi_04_02_last_tiles_theater();
        let raw_legacy = gsi_04_02_last_tiles_tmp_bytes(6, [11, 12, 13], [14, 15, 16]);
        let rough = gsi_04_02_last_tiles_tmp_bytes(3, [1, 2, 3], [4, 5, 6]);
        let rough_suffix = gsi_04_02_last_tiles_tmp_bytes(9, [31, 32, 33], [34, 35, 36]);
        let lat = gsi_04_02_last_tiles_tmp_bytes(7, [21, 22, 23], [24, 25, 26]);
        let (_directory, assets) = gsi_04_02_asset_manager_with_loose_tmps(&[
            ("old03.tem", &raw_legacy),
            ("rough01.tem", &rough),
            ("rough01a.tem", &rough_suffix),
            ("lat16.tem", &lat),
        ]);
        crate::map::theater::resolve_contiguous_variant_chains_for_test(
            &mut theater.lookup,
            &assets,
        );
        assert_eq!(theater.lookup.translate_legacy_map_tile_index(5), 8);
        assert_eq!(theater.lookup.total_file_count(8), 2);
        let mut map = make_map(
            vec![MapCell {
                rx: 3,
                ry: 2,
                tile_index: 5,
                sub_tile: 0,
                z: 4,
            }],
            Vec::new(),
            Vec::new(),
        );
        map.header.width = 3;
        map.header.height = 2;
        map.iso_map_pack_lookups = vec![crate::map::map_file::IsoMapPackLookup {
            raw_x: -509,
            raw_y: 3,
            canonical: Some((3, 2)),
        }];

        let mut cache = crate::map::tile_variant_selector::TileVariantSelectorCache::default();
        cache.complete_theater_registry_load(Some(5), None);
        let mut deterministic_main = || 0;
        let mut fill = |_low, _high| 0;
        let without_lat = {
            let mut selector = cache.begin_load(&mut deterministic_main);
            ResolvedTerrainGrid::build_with_variant_selector(
                &map,
                Some(&theater),
                Some(&assets),
                None,
                None,
                None,
                false,
                0,
                &mut fill,
                &mut selector,
            )
        };
        let translated = without_lat.cell(3, 2).expect("accepted Pack5 cell");
        assert_eq!(translated.source_tile_index, 8);
        assert_eq!(translated.final_tile_index, 8);
        assert_eq!(translated.tileset_index, Some(2));
        assert!(translated.variant > 0);
        assert_eq!(translated.variant, 1);
        assert_eq!(translated.radar_left, [31, 32, 33]);
        assert_eq!(translated.radar_right, [34, 35, 36]);
        assert_eq!(translated.level, 4);

        let fill_cell = without_lat.cell(1, 3).expect("prefilled real CellClass");
        assert_eq!(fill_cell.source_tile_index, 5);
        assert_ne!(fill_cell.source_tile_index, 8);

        let with_lat = {
            let mut selector = cache.begin_load(&mut deterministic_main);
            ResolvedTerrainGrid::build_with_variant_selector(
                &map,
                Some(&theater),
                Some(&assets),
                None,
                None,
                None,
                true,
                0,
                &mut fill,
                &mut selector,
            )
        };
        let lat_fixed = with_lat.cell(3, 2).expect("LAT-fixed Pack5 cell");
        assert_eq!(lat_fixed.source_tile_index, 8);
        assert_eq!(lat_fixed.final_tile_index, 24);
        assert_eq!(lat_fixed.tileset_index, Some(3));
        assert_eq!(lat_fixed.radar_left, [21, 22, 23]);
        assert_eq!(lat_fixed.radar_right, [24, 25, 26]);
    }

    #[test]
    fn gsi_04_02_last_tiles_provenance_gates_both_materialization_paths() {
        let theater = gsi_04_02_last_tiles_theater();
        let mut pack = make_map(
            vec![MapCell {
                rx: 2,
                ry: 2,
                tile_index: 5,
                sub_tile: 0,
                z: 0,
            }],
            Vec::new(),
            Vec::new(),
        );
        pack.iso_map_pack_lookups = vec![crate::map::map_file::IsoMapPackLookup {
            raw_x: -510,
            raw_y: 3,
            canonical: Some((2, 2)),
        }];
        pack.header.width = 3;
        pack.header.height = 2;

        let selector_free_pack =
            ResolvedTerrainGrid::build(&pack, Some(&theater), None, None, None, false, 0);
        assert_eq!(
            selector_free_pack
                .cell(2, 2)
                .expect("selector-free Pack5 cell")
                .source_tile_index,
            8
        );

        let mut manual = make_map(pack.cells.clone(), Vec::new(), Vec::new());
        manual.header.width = 3;
        manual.header.height = 2;
        let selector_free_manual =
            ResolvedTerrainGrid::build(&manual, Some(&theater), None, None, None, false, 0);
        assert_eq!(
            selector_free_manual
                .cell(2, 2)
                .expect("manual actual-ID cell")
                .source_tile_index,
            5
        );

        let theaterless_pack = ResolvedTerrainGrid::build(&pack, None, None, None, None, false, 0);
        assert_eq!(
            theaterless_pack
                .cell(2, 2)
                .expect("theater-less synthetic Pack cell")
                .source_tile_index,
            5
        );

        let mut cache = crate::map::tile_variant_selector::TileVariantSelectorCache::default();
        cache.complete_theater_registry_load(Some(5), None);
        let mut forbidden_main = || panic!("compatibility fixture has no variants");
        let mut fill = |_low, _high| 0;
        let production_manual = {
            let mut selector = cache.begin_load(&mut forbidden_main);
            ResolvedTerrainGrid::build_with_variant_selector(
                &manual,
                Some(&theater),
                None,
                None,
                None,
                None,
                false,
                0,
                &mut fill,
                &mut selector,
            )
        };
        assert_eq!(
            production_manual
                .cell(2, 2)
                .expect("production manual actual-ID cell")
                .source_tile_index,
            5
        );
        let production_pack = {
            let mut selector = cache.begin_load(&mut forbidden_main);
            ResolvedTerrainGrid::build_with_variant_selector(
                &pack,
                Some(&theater),
                None,
                None,
                None,
                None,
                false,
                0,
                &mut fill,
                &mut selector,
            )
        };
        assert_eq!(
            production_pack
                .cell(2, 2)
                .expect("production Pack5 cell")
                .source_tile_index,
            8
        );
    }

    #[test]
    fn gsi_04_02_last_tiles_does_not_translate_fill_or_dummy_only_pack_misses() {
        let theater = gsi_04_02_last_tiles_theater();
        let mut map = make_map(
            vec![MapCell {
                rx: 99,
                ry: 99,
                tile_index: 5,
                sub_tile: 7,
                z: 9,
            }],
            Vec::new(),
            Vec::new(),
        );
        map.header.width = 3;
        map.header.height = 2;
        map.iso_map_pack_lookups = vec![crate::map::map_file::IsoMapPackLookup {
            raw_x: 99,
            raw_y: 99,
            canonical: None,
        }];
        let dummy = SharedCellDummy::fresh();
        let mut cache = crate::map::tile_variant_selector::TileVariantSelectorCache::default();
        cache.complete_theater_registry_load(Some(5), None);
        let mut forbidden_main = || panic!("Fill and misses do not draw Main");
        let mut selector = cache.begin_load(&mut forbidden_main);
        let mut fill = |_low, _high| 0;

        let cells = materialize_map_load_cells(
            &map,
            Some(&theater.lookup),
            &mut selector,
            &mut fill,
            &dummy,
        );

        assert!(!cells.is_empty());
        assert!(cells.iter().all(|cell| cell.tile_index == 5));
        assert!(cells.iter().all(|cell| cell.tile_index != 8));
        assert_eq!(dummy.snapshot().coord, (99, 99));
    }

    #[test]
    fn gsi_04_01_resize_clears_bridge_bits_without_replacing_dummy_identity() {
        let dummy = SharedCellDummy::fresh();
        let retained = dummy.clone();
        dummy.stamp_coord(-7, 11);
        dummy.set_level_slope(-3, 9);
        dummy.set_bridge_flags_0x1180(BRIDGE_FLAG_ANCHOR_SELF);
        assert_eq!(
            dummy.bridge_flags_0x1180(),
            BRIDGE_FLAG_ANCHOR_SELF,
            "the promoted live anchor bit is represented independently"
        );

        dummy.reconstruct_for_map_resize();

        assert!(dummy.same_identity(&retained));
        assert_eq!(
            dummy.snapshot(),
            SharedCellDummySnapshot {
                coord: (0, 0),
                level: 0,
                slope_type: 0,
                bridge_flags_0x1180: 0,
            }
        );
    }

    #[test]
    fn gsi_04_01_isomap_misses_stamp_raw_coord_without_payload_leak() {
        let mut map = make_map(
            vec![
                MapCell {
                    rx: 2,
                    ry: 1,
                    tile_index: 41,
                    sub_tile: 4,
                    z: 5,
                },
                MapCell {
                    rx: 3,
                    ry: 1,
                    tile_index: 99,
                    sub_tile: 9,
                    z: 10,
                },
            ],
            Vec::new(),
            Vec::new(),
        );
        map.header.width = 2;
        map.header.height = 1;
        map.iso_map_pack_lookups = vec![
            crate::map::map_file::IsoMapPackLookup {
                raw_x: -1,
                raw_y: 0,
                canonical: None,
            },
            crate::map::map_file::IsoMapPackLookup {
                raw_x: -510,
                raw_y: 2,
                canonical: Some((2, 1)),
            },
            crate::map::map_file::IsoMapPackLookup {
                raw_x: -509,
                raw_y: 2,
                canonical: Some((3, 1)),
            },
        ];

        let dummy = SharedCellDummy::fresh();
        dummy.stamp_coord(90, 91);
        dummy.set_level_slope(-7, 11);
        let mut cache = crate::map::tile_variant_selector::TileVariantSelectorCache::default();
        let mut forbidden_main = || panic!("materialization must not draw Main");
        let mut selector = cache.begin_load(&mut forbidden_main);
        let mut fill = |_low, _high| 0;

        let cells = materialize_map_load_cells(&map, None, &mut selector, &mut fill, &dummy);

        let real = cells
            .iter()
            .find(|cell| (cell.rx, cell.ry) == (2, 1))
            .expect("aliased lookup resolves to an allocated real cell");
        assert_eq!((real.tile_index, real.sub_tile, real.z), (41, 4, 5));
        assert!(
            cells.iter().all(|cell| cell.tile_index != 99),
            "null-slot payload must not leak into a real CellClass"
        );
        assert_eq!(
            dummy.snapshot(),
            SharedCellDummySnapshot {
                coord: (-509, 2),
                level: -7,
                slope_type: 11,
                bridge_flags_0x1180: 0,
            },
            "the last miss stamps its raw request and preserves non-coordinate bytes"
        );
    }

    #[test]
    fn gsi_04_01_valid_isomap_lookup_does_not_stamp_dummy() {
        let mut map = make_map(
            vec![MapCell {
                rx: 2,
                ry: 1,
                tile_index: 41,
                sub_tile: 4,
                z: 5,
            }],
            Vec::new(),
            Vec::new(),
        );
        map.header.width = 2;
        map.header.height = 1;
        map.iso_map_pack_lookups = vec![crate::map::map_file::IsoMapPackLookup {
            raw_x: -510,
            raw_y: 2,
            canonical: Some((2, 1)),
        }];

        let dummy = SharedCellDummy::fresh();
        dummy.stamp_coord(-7, 11);
        dummy.set_level_slope(-3, 9);
        dummy.set_bridge_flags_0x1180(BRIDGE_FLAG_ANCHOR_SELF);
        let expected_dummy = dummy.snapshot();
        let mut cache = crate::map::tile_variant_selector::TileVariantSelectorCache::default();
        let mut forbidden_main = || panic!("materialization must not draw Main");
        let mut selector = cache.begin_load(&mut forbidden_main);
        let mut fill = |_low, _high| 0;

        let cells = materialize_map_load_cells(&map, None, &mut selector, &mut fill, &dummy);

        assert_eq!(dummy.snapshot(), expected_dummy);
        assert_eq!(
            cells
                .iter()
                .find(|cell| (cell.rx, cell.ry) == (2, 1))
                .map(|cell| (cell.tile_index, cell.sub_tile, cell.z)),
            Some((41, 4, 5))
        );
    }

    #[test]
    fn gsi_04_01_runtime_setter_uses_native_real_or_dummy_order() {
        let cells = (0..3)
            .flat_map(|ry| (0..3).map(move |rx| make_test_cell(rx, ry)))
            .collect();
        let mut grid = ResolvedTerrainGrid::from_cells(3, 3, cells);
        grid.test_set_native_allocated_cells(&[(1, 1)]);
        let dummy = grid.shared_cell_dummy();
        let stamp = BridgeFlagStamp::new((1, 1), 0, true);

        let _ = grid.apply_runtime_bridge_flag_stamp(stamp);

        assert_eq!(
            grid.cell(1, 1).unwrap().bridge_facts.raw_flags & MODELED_CELLCLASS_BRIDGE_FLAG_MASK,
            MODELED_CELLCLASS_BRIDGE_FLAG_MASK,
            "the allocated anchor mutates the real CellClass"
        );
        assert_eq!(
            dummy.snapshot(),
            SharedCellDummySnapshot {
                coord: (1, 2),
                level: 0,
                slope_type: 0,
                bridge_flags_0x1180: BRIDGE_FLAG_STRUCTURAL,
            },
            "the missing opposite is the last native lookup/writer"
        );
        assert_eq!(
            grid.cellclass_bridge_flags_0x1180(9, 7),
            BRIDGE_FLAG_STRUCTURAL,
            "later candidate-filter lookups observe the live dummy flags"
        );
        assert_eq!(dummy.snapshot().coord, (9, 7));

        let _ = grid.apply_runtime_bridge_flag_stamp(BridgeFlagStamp {
            set: false,
            ..stamp
        });
        assert_eq!(dummy.bridge_flags_0x1180(), 0);
        assert_eq!(
            grid.cell(1, 1).unwrap().bridge_facts.raw_flags & MODELED_CELLCLASS_BRIDGE_FLAG_MASK,
            0
        );

        let _ = grid.apply_runtime_bridge_flag_stamp(stamp);
        assert_eq!(dummy.bridge_flags_0x1180(), BRIDGE_FLAG_STRUCTURAL);
    }

    #[test]
    fn bridge_publication_retains_allocation_identity_across_other_lookups() {
        let cells = (0..3)
            .flat_map(|y| (0..3).map(move |x| make_test_cell(x, y)))
            .collect();
        let mut terrain = ResolvedTerrainGrid::from_cells(3, 3, cells);
        terrain.test_set_native_allocated_cells(&[(1, 1)]);
        let real = terrain.native_cell_identity((-511, 2));
        assert_eq!(real, NativeCellIdentity::Real(4));
        let dummy = terrain.native_cell_identity((-1, -1));
        terrain.write_native_cell_flags(real, 0xa401_1380);
        terrain.write_native_cell_flags(dummy, 0xb502_0380);
        terrain.write_native_cell_anchor(real, Some(dummy));
        terrain.write_native_cell_anchor(dummy, Some(real));
        terrain.write_native_cell_state(dummy, 17);
        assert_eq!(terrain.native_cell_identity((8, 9)), dummy);
        assert_eq!(terrain.native_cell_coord(real), (1, 1));
        assert_eq!(terrain.native_cell_coord(dummy), (8, 9));
        assert_eq!(terrain.native_cell_flags(real), 0xa401_1380);
        assert_eq!(terrain.native_cell_flags(dummy), 0xb502_0380);
        assert_eq!(terrain.native_cell_state(dummy), 17);
        assert_eq!(terrain.native_cell_anchor(real), Some(dummy));
        assert_eq!(terrain.native_cell_anchor(dummy), Some(real));
        assert_eq!(
            terrain.shared_cell_dummy().snapshot().coord,
            (8, 9),
            "retained reads do not repeat lookup"
        );
    }

    #[test]
    fn bridge_publication_overlapping_mark_preserves_literal_anchor_pointer() {
        let cells = (0..7)
            .flat_map(|y| (0..7).map(move |x| make_test_cell(x, y)))
            .collect();
        let mut terrain = ResolvedTerrainGrid::from_cells(7, 7, cells);
        let first = terrain.native_cell_identity((3, 3));
        let second = terrain.native_cell_identity((4, 3));
        terrain.apply_runtime_bridge_mark_stamp(
            BridgeFlagStamp::new((3, 3), 6, true),
            BridgeStampFamily::Nesw,
        );
        assert_eq!(terrain.native_cell_anchor(second), Some(first));
        terrain.apply_runtime_bridge_mark_stamp(
            BridgeFlagStamp::new((4, 3), 6, true),
            BridgeStampFamily::Nesw,
        );
        assert_eq!(
            terrain
                .cell(4, 3)
                .unwrap()
                .bridge_facts
                .anchor
                .unwrap()
                .anchor,
            (4, 3)
        );
        assert_eq!(
            terrain.native_cell_anchor(second),
            Some(first),
            "native anchor slot preserves +2C despite its derived self relation"
        );
        terrain.apply_runtime_bridge_mark_stamp(
            BridgeFlagStamp::new((4, 3), 6, false),
            BridgeStampFamily::Nesw,
        );
        assert_eq!(terrain.native_cell_anchor(second), Some(first));
        assert_eq!(
            terrain.native_cell_anchor(first),
            None,
            "F1 clears its literal pointer"
        );
        let retained = DynamicTerrainCellState::capture(terrain.cell(4, 3).unwrap());
        let bytes = bincode::serialize(&retained).unwrap();
        let restored: DynamicTerrainCellState = bincode::deserialize(&bytes).unwrap();
        assert_eq!(restored, retained);
    }

    #[test]
    fn bridge_publication_dummy_masked_writers_and_resize_preserve_native_residue() {
        let dummy = SharedCellDummy::fresh();
        dummy.write_raw_flags(0xff81_ffff);
        dummy.set_bridge_flags_0x1180(0);
        assert_eq!(
            dummy.raw_flags(),
            0xff81_ffff & !MODELED_CELLCLASS_BRIDGE_FLAG_MASK
        );
        dummy.test_set_retained_bridge_flags(0);
        assert_eq!(
            dummy.raw_flags(),
            0xff81_ffff & !RETAINED_CELLCLASS_BRIDGE_FLAG_MASK
        );
        dummy.write_native_anchor(Some(NativeCellIdentity::Real(13)));
        dummy.write_overlay_identity_state(24, 15);
        dummy.stamp_coord(9, -4);
        let prepared = SharedCellDummy::fresh();
        prepared.adopt_prepared_load_state(&dummy);
        assert_eq!(prepared.raw_flags(), dummy.raw_flags());
        assert_eq!(prepared.native_anchor(), dummy.native_anchor());
        assert_eq!(prepared.overlay_identity_state(), (24, 15));
        dummy.reconstruct_for_map_resize();
        assert_eq!(
            dummy.raw_flags(),
            0xff80_0000,
            "original47BCE1 constructor AND"
        );
        assert_eq!(dummy.native_anchor(), None);
        assert_eq!(dummy.overlay_identity_state(), (-1, 0));
        assert_eq!(dummy.snapshot().coord, (0, 0));
    }

    #[test]
    fn authored_fill_defers_all_executable_projection() {
        let mut theater = synthetic_theater_from_ini(
            b"[General]\nBridgeSet=0\nBridgeMiddle1=1\nBridgeMiddle2=5\n\
              [TileSet0000]\nTilesInSet=8\nFileName=tunnel\nSetName=Tunnel FX\n\
              [Tunnel FX]\nTile01Anim=TUNNELFX\nTile01AttachesTo=0\n",
        );
        theater.bridge_set = Some(0);
        theater.bridge_middle_1 = Some(1);
        theater.bridge_middle_2 = Some(5);
        theater.tunnels = Some(0);
        let tmp_name = theater
            .lookup
            .filename_for_variant(0, 0)
            .expect("tunnel TMP name")
            .to_string();
        let suffix_name = tmp_name.replace(".tem", "a.tem");
        let tmp = gsi_04_02_last_tiles_tmp_bytes(5, [1, 2, 3], [4, 5, 6]);
        let suffix_tmp = gsi_04_02_last_tiles_tmp_bytes(5, [7, 8, 9], [10, 11, 12]);
        let (_directory, assets) = gsi_04_02_asset_manager_with_loose_tmps(&[
            (tmp_name.as_str(), tmp.as_slice()),
            (suffix_name.as_str(), suffix_tmp.as_slice()),
        ]);
        crate::map::theater::resolve_contiguous_variant_chains_for_test(
            &mut theater.lookup,
            &assets,
        );
        assert_eq!(theater.lookup.total_file_count(0), 2);
        let mut map = make_map(
            vec![MapCell {
                rx: 2,
                ry: 3,
                tile_index: 0,
                sub_tile: 0,
                z: 0,
            }],
            vec![OverlayEntry {
                rx: 2,
                ry: 3,
                overlay_id: 0x18,
                frame: 7,
            }],
            vec![TerrainObject {
                rx: 2,
                ry: 3,
                name: "TREE".to_string(),
            }],
        );
        map.explicit_tubes = vec![TubeFact::explicit((2, 3), (2, 3), 2, Vec::new())];
        map.replace_unconsumed_overlay_data(OverlayDataPack::from_cells([(2, 3, 99)]))
            .expect("fixture overlay data remains unconsumed");

        let mut eager_scenario_rng = SimRng::new(0x401);
        let mut eager_fill_calls = 0usize;
        let mut eager_fill = |low, high| {
            eager_fill_calls += 1;
            eager_scenario_rng.next_range_u32_inclusive(low, high)
        };
        let mut eager_main_rng = SimRng::new(0x402);
        let mut eager_main_draw = || eager_main_rng.next_u32();
        let mut eager_cache =
            crate::map::tile_variant_selector::TileVariantSelectorCache::default();
        let (eager, eager_selector_metrics) = {
            let mut selector = eager_cache.begin_load(&mut eager_main_draw);
            let grid = ResolvedTerrainGrid::build_with_variant_selector_and_shared_dummy(
                &map,
                Some(&theater),
                Some(&assets),
                None,
                None,
                None,
                true,
                2,
                &mut eager_fill,
                &mut selector,
                SharedCellDummy::fresh(),
                OverlayLoadSource::Authored,
            );
            let metrics = (
                selector.generated_table(),
                selector.map_fill_scenario_advance_count(),
                selector.raw_draw_count(),
            );
            (grid, metrics)
        };
        drop(eager_fill);
        drop(eager_main_draw);
        let eager_cell = eager.cell(2, 3).expect("eager fixture cell");
        assert!(eager_cell.bridge_facts.has_structural_bridge());
        assert_eq!(eager_cell.bridge_facts.state_byte, 99);
        assert_eq!(eager_cell.terrain_object_occupation, Some(7));
        assert_eq!(
            eager_cell.bridgehead_anchor_class_at_load,
            Some(crate::map::bridge_facts::BridgeheadAnchorClass::Variant0)
        );
        assert!(!eager.tile_animations().is_empty());
        assert!(!eager.tube_facts().is_empty());

        let mut pending_scenario_rng = SimRng::new(0x401);
        let mut pending_fill_calls = 0usize;
        let mut pending_fill = |low, high| {
            pending_fill_calls += 1;
            pending_scenario_rng.next_range_u32_inclusive(low, high)
        };
        let mut pending_main_rng = SimRng::new(0x402);
        let mut pending_main_draw = || pending_main_rng.next_u32();
        let mut pending_cache =
            crate::map::tile_variant_selector::TileVariantSelectorCache::default();
        let dummy = SharedCellDummy::fresh();
        let (pending, pending_selector_metrics) = {
            let mut selector = pending_cache.begin_load(&mut pending_main_draw);
            let fill =
                ResolvedTerrainGrid::build_pending_authored_with_variant_selector_and_shared_dummy(
                    &map,
                    Some(&theater),
                    Some(&assets),
                    None,
                    None,
                    true,
                    2,
                    &mut pending_fill,
                    &mut selector,
                    dummy.clone(),
                );
            let metrics = (
                selector.generated_table(),
                selector.map_fill_scenario_advance_count(),
                selector.raw_draw_count(),
            );
            (fill, metrics)
        };
        drop(pending_fill);
        drop(pending_main_draw);
        assert_eq!(pending_fill_calls, eager_fill_calls);
        assert_eq!(
            pending_scenario_rng.logical_state(),
            eager_scenario_rng.logical_state()
        );
        assert_eq!(
            pending_main_rng.logical_state(),
            eager_main_rng.logical_state()
        );
        assert_eq!(pending_selector_metrics, eager_selector_metrics);
        assert!(pending_selector_metrics.0);
        assert!(pending_selector_metrics.2 > 0);
        assert_eq!(
            pending
                .pending_grid()
                .iter()
                .map(|cell| (cell.rx, cell.ry, cell.variant))
                .collect::<Vec<_>>(),
            eager
                .iter()
                .map(|cell| (cell.rx, cell.ry, cell.variant))
                .collect::<Vec<_>>()
        );
        let pending_grid = pending.pending_grid();
        let pending_cell = pending_grid.cell(2, 3).expect("pending fixture cell");
        assert_eq!(
            pending_cell.final_tile_index,
            pending_cell.source_tile_index
        );
        assert_eq!(pending_cell.bridge_facts, BridgeCellFacts::default());
        assert_eq!(pending_cell.terrain_object_occupation, None);
        assert!(!pending_cell.terrain_object_blocks);
        assert!(pending_grid.tile_animations().is_empty());
        assert!(pending_grid.tube_facts().is_empty());
        assert_eq!(pending_cell.tube_index, None);
        assert_eq!(pending_cell.bridgehead_anchor_class_at_load, None);
        assert_eq!(dummy.snapshot().bridge_flags_0x1180, 0);
        assert_eq!(
            map.overlay_data_pack()
                .expect("pending Fill retains authored overlay data")
                .byte_at(2, 3),
            99
        );

        let consumed = pending.into_pending_grid();
        assert_eq!(consumed.cell(2, 3).unwrap().source_tile_index, 0);

        let lat_theater = synthetic_theater_from_ini(
            b"[General]\nRoughTile=1\nClearToRoughLat=2\n\
              [TileSet0000]\nSetName=Clear\nFileName=clear\nTilesInSet=5\n\
              [TileSet0001]\nSetName=Rough\nFileName=rough\nTilesInSet=5\n\
              [TileSet0002]\nSetName=ClearToRough\nFileName=crgh\nTilesInSet=16\n",
        );
        let lat_map = make_map(
            vec![
                anim_cell(5, 5, 5, 0, 0),
                anim_cell(5, 4, 0, 0, 0),
                anim_cell(4, 5, 6, 0, 0),
                anim_cell(6, 5, 1, 0, 0),
                anim_cell(5, 6, 7, 0, 0),
            ],
            Vec::new(),
            Vec::new(),
        );
        let pending_lat = ResolvedTerrainGrid::build_inner(
            &lat_map,
            Some(&lat_theater),
            None,
            None,
            None,
            None,
            true,
            0,
            OverlayLoadSource::Authored,
            TerrainBuildProjection::PendingAuthored,
            None,
            None,
            None,
        );
        assert_eq!(pending_lat.cell(5, 5).unwrap().final_tile_index, 5);

        let cliff_map = make_map(
            vec![anim_cell(1, 1, 0, 0, 0), anim_cell(1, 0, 0, 0, 4)],
            Vec::new(),
            Vec::new(),
        );
        let pending_cliff = ResolvedTerrainGrid::build_inner(
            &cliff_map,
            None,
            None,
            None,
            None,
            None,
            false,
            2,
            OverlayLoadSource::Authored,
            TerrainBuildProjection::PendingAuthored,
            None,
            None,
            None,
        );
        let cliff_cell = pending_cliff.cell(1, 1).unwrap();
        assert_eq!(cliff_cell.land_type, LandType::Clear.as_index());
        assert!(!cliff_cell.ground_walk_blocked);
    }

    #[test]
    fn gsi_04_01_production_overlaypack_stamps_two_anchors_in_row_major_order() {
        let mut map = make_map(
            Vec::new(),
            vec![
                OverlayEntry {
                    rx: 2,
                    ry: 1,
                    overlay_id: 0x18,
                    frame: 0,
                },
                OverlayEntry {
                    rx: 1,
                    ry: 2,
                    overlay_id: 0x18,
                    frame: 0,
                },
            ],
            Vec::new(),
        );
        map.header.width = 2;
        map.header.height = 1;
        map.header.local_width = 2;
        map.header.local_height = 1;

        let mut cache = crate::map::tile_variant_selector::TileVariantSelectorCache::default();
        let mut scenario_rng = SimRng::new(0x401);
        let mut fill = |low, high| scenario_rng.next_range_u32_inclusive(low, high);
        let mut main_rng = SimRng::new(0x401);
        let mut main_draw = || main_rng.next_u32();
        let dummy = SharedCellDummy::fresh();
        let grid = {
            let mut selector = cache.begin_load(&mut main_draw);
            ResolvedTerrainGrid::build_with_variant_selector_and_shared_dummy(
                &map,
                None,
                None,
                None,
                None,
                None,
                false,
                0,
                &mut fill,
                &mut selector,
                dummy.clone(),
                OverlayLoadSource::Authored,
            )
        };

        assert!(dummy.same_identity(&grid.shared_cell_dummy()));
        assert_eq!(
            dummy.snapshot(),
            SharedCellDummySnapshot {
                coord: (1, 3),
                level: 0,
                slope_type: 0,
                bridge_flags_0x1180: BRIDGE_FLAG_STRUCTURAL,
            },
            "the later row-major anchor owns the final dummy coord and bits"
        );
        assert_eq!(grid.cell(2, 1).unwrap().bridge_facts.overlay_id, Some(0x18));
        assert_eq!(grid.cell(1, 2).unwrap().bridge_facts.overlay_id, Some(0x18));
    }

    #[test]
    fn gsi_04_12_generated_materialized_overlays_never_replay_fixed_map_mark() {
        let map = make_map(
            vec![MapCell {
                rx: 4,
                ry: 4,
                tile_index: 0,
                sub_tile: 0,
                z: 0,
            }],
            vec![
                OverlayEntry {
                    rx: 2,
                    ry: 2,
                    overlay_id: 0x18,
                    frame: 0,
                },
                OverlayEntry {
                    rx: 1,
                    ry: 1,
                    overlay_id: 0x5C,
                    frame: 0,
                },
            ],
            Vec::new(),
        );

        let authored = ResolvedTerrainGrid::build_inner(
            &map,
            None,
            None,
            None,
            None,
            None,
            false,
            0,
            OverlayLoadSource::Authored,
            TerrainBuildProjection::EagerCompatibility,
            None,
            None,
            None,
        );
        let generated = ResolvedTerrainGrid::build_inner(
            &map,
            None,
            None,
            None,
            None,
            None,
            false,
            0,
            OverlayLoadSource::GeneratedMaterialized,
            TerrainBuildProjection::EagerCompatibility,
            None,
            None,
            None,
        );

        assert!(
            authored
                .cell(2, 2)
                .unwrap()
                .bridge_facts
                .has_structural_bridge(),
            "the ordinary authored-map source still runs fixed OverlayClass::Mark"
        );
        assert_eq!(
            generated.cell(2, 2).unwrap().bridge_facts.raw_flags
                & MODELED_CELLCLASS_BRIDGE_FLAG_MASK,
            0,
            "generated direct cell writes must not be expanded a second time"
        );
        assert_eq!(
            generated.cell(2, 2).unwrap().bridge_facts.overlay_id,
            Some(0x18),
            "source marking suppresses only Mark, not the materialized overlay"
        );
        assert_eq!(
            generated.cell(1, 1).unwrap().bridge_facts.overlay_id,
            Some(0x5C),
            "the generated low-deck rectangle remains present"
        );
    }

    #[test]
    fn gsi_04_03a_level_prefill_wraps_before_absolute_last_isomap_overwrite() {
        let mut map = make_map(
            vec![
                MapCell {
                    rx: 2,
                    ry: 2,
                    tile_index: 98,
                    sub_tile: 6,
                    z: 5,
                },
                MapCell {
                    rx: 2,
                    ry: 2,
                    tile_index: 99,
                    sub_tile: 7,
                    z: 9,
                },
            ],
            Vec::new(),
            Vec::new(),
        );
        map.header.width = 3;
        map.header.height = 2;
        map.header.fill = " \twAtEr\r\n".to_string();
        map.header.level = 260;

        let mut cache = crate::map::tile_variant_selector::TileVariantSelectorCache::default();
        assert_eq!(cache.cached_clear_tile_base(), 0);
        assert_eq!(cache.cached_water_set_base(), 0);

        // Stock RMG preview A publishes both resolved bases for the following
        // ordinary load B. Repeating preview A preserves the same values.
        cache.complete_theater_registry_load(Some(37), Some(50));
        assert_eq!(cache.cached_clear_tile_base(), 37);
        assert_eq!(cache.cached_water_set_base(), 50);
        cache.complete_theater_registry_load(Some(37), Some(50));
        assert_eq!(cache.cached_water_set_base(), 50);

        let seed = 0xA55A_1234;
        let mut expected_scenario = SimRng::new(seed);
        let expected_tiles: Vec<_> = (0..10)
            .map(|_| 50 + expected_scenario.next_range_u32_inclusive(0, 3) as i32)
            .collect();
        let mut scenario_rng = SimRng::new(seed);
        let mut scenario_calls = 0usize;
        let mut scenario_fill_ranged = |low, high| {
            scenario_calls += 1;
            scenario_rng.next_range_u32_inclusive(low, high)
        };
        let mut main_rng = SimRng::new(seed);
        let main_before = main_rng.logical_state();
        let mut main_draw = || main_rng.next_u32();
        {
            let mut selector = cache.begin_load(&mut main_draw);
            let cells = materialize_map_load_cells(
                &map,
                None,
                &mut selector,
                &mut scenario_fill_ranged,
                &SharedCellDummy::fresh(),
            );
            let coords: Vec<_> = cells.iter().map(|cell| (cell.rx, cell.ry)).collect();
            assert_eq!(
                coords,
                vec![
                    (1, 3),
                    (2, 2),
                    (3, 1),
                    (2, 3),
                    (3, 2),
                    (2, 4),
                    (3, 3),
                    (4, 2),
                    (3, 4),
                    (4, 3),
                ]
            );
            assert_eq!(selector.map_fill_scenario_advance_count(), 10);
            for (index, cell) in cells.iter().enumerate() {
                if index == 1 {
                    assert_eq!((cell.tile_index, cell.sub_tile, cell.z), (99, 7, 9));
                } else {
                    assert_eq!(
                        (cell.tile_index, cell.sub_tile, cell.z),
                        (expected_tiles[index], 0, 4)
                    );
                }
            }
            assert!(!coords.contains(&(0, 0)));
            assert!(!coords.contains(&(1, 4)));
            assert!(!selector.generated_table());
            assert_eq!(selector.raw_draw_count(), 0);
        }
        drop(main_draw);
        drop(scenario_fill_ranged);
        assert_eq!(scenario_calls, 10);
        assert_eq!(
            scenario_rng.logical_state(),
            expected_scenario.logical_state()
        );
        assert_eq!(main_rng.logical_state(), main_before);

        // A registry with neither role publishes native reset sentinels only
        // after its own Fill, for use by the following load.
        cache.complete_theater_registry_load(None, None);
        assert_eq!(cache.cached_clear_tile_base(), -1);
        assert_eq!(cache.cached_water_set_base(), -1);
    }

    #[test]
    fn gsi_04_02_clear_and_unknown_use_prior_cached_clear_and_equal_scenario_bounds() {
        for fill in ["Clear", "unknown"] {
            let mut map = make_map(Vec::new(), Vec::new(), Vec::new());
            map.header.width = 3;
            map.header.height = 2;
            map.header.fill = fill.to_string();

            let mut cache = crate::map::tile_variant_selector::TileVariantSelectorCache::default();
            // The already parsed current theater may resolve ClearTile=0, but
            // Fill snapshots the prior process-global value (37).
            cache.complete_theater_registry_load(Some(37), Some(50));
            let mut forbidden_main = || panic!("Fill must not draw Main");
            let mut selector = cache.begin_load(&mut forbidden_main);
            let mut scenario_rng = SimRng::new(0x1234_5678);
            let scenario_before = scenario_rng.logical_state();
            let mut scenario_calls = 0usize;
            let mut scenario_fill_ranged = |low, high| {
                scenario_calls += 1;
                assert_eq!((low, high), (0, 0));
                scenario_rng.next_range_u32_inclusive(low, high)
            };
            let cells = materialize_map_load_cells(
                &map,
                None,
                &mut selector,
                &mut scenario_fill_ranged,
                &SharedCellDummy::fresh(),
            );
            drop(scenario_fill_ranged);

            assert_eq!(cells.len(), 10);
            assert!(
                cells
                    .iter()
                    .all(|cell| { cell.tile_index == 37 && cell.sub_tile == 0 && cell.z == 0 })
            );
            assert_eq!(scenario_calls, 10);
            assert_eq!(scenario_rng.logical_state(), scenario_before);
            assert_eq!(selector.map_fill_scenario_advance_count(), 0);
            assert_eq!(selector.raw_draw_count(), 0);
        }
    }

    #[test]
    fn gsi_04_02_production_build_prefills_before_selector_and_skips_outside_slots() {
        let mut map = make_map(
            vec![MapCell {
                rx: 2,
                ry: 2,
                tile_index: theater::NO_TILE,
                sub_tile: 9,
                z: 4,
            }],
            Vec::new(),
            Vec::new(),
        );
        map.header.width = 3;
        map.header.height = 2;
        map.header.fill = "Water".to_string();
        let mut theater = synthetic_theater_from_ini(
            b"[TileSet0000]\nTilesInSet=64\nFileName=ground\nSetName=Ground\n",
        );
        theater.rmg_tiles.clear_tile = Some(0);
        // Preview A published 50, while this already-parsed ordinary theater B
        // resolves 1. B's Fill must retain A until B publishes afterward.
        theater.rmg_tiles.water_set = Some(1);

        let mut cache = crate::map::tile_variant_selector::TileVariantSelectorCache::default();
        cache.complete_theater_registry_load(Some(37), Some(50));
        let seed = 0xCAFE_BABE;
        let mut scenario_rng = SimRng::new(seed);
        let mut expected_scenario = scenario_rng.clone();
        let expected_first = 50 + expected_scenario.next_range_u32_inclusive(0, 3) as i32;
        for _ in 1..10 {
            let _ = expected_scenario.next_range_u32_inclusive(0, 3);
        }
        let mut scenario_fill_ranged = |low, high| scenario_rng.next_range_u32_inclusive(low, high);
        let mut main_rng = SimRng::new(seed);
        let main_before = main_rng.logical_state();
        let mut main_draw = || main_rng.next_u32();
        let main_draw_count;
        {
            let mut selector = cache.begin_load(&mut main_draw);
            let grid = ResolvedTerrainGrid::build_with_variant_selector(
                &map,
                Some(&theater),
                None,
                None,
                None,
                None,
                false,
                0,
                &mut scenario_fill_ranged,
                &mut selector,
            );

            let first = grid.cell(1, 3).expect("allocated first diamond cell");
            assert_eq!(first.source_tile_index, expected_first);
            assert_eq!(first.final_tile_index, expected_first);
            assert!(!first.filled_clear);

            let explicit = grid.cell(2, 2).expect("explicit overwrite cell");
            assert_eq!(explicit.source_tile_index, theater::NO_TILE);
            assert_eq!(explicit.final_sub_tile, 9);
            assert_eq!(explicit.level, 4);
            assert_eq!(grid.presentation_tile(explicit), (0, 0));

            assert_eq!(grid.iter().count(), 10);
            assert!(grid.cell(0, 0).is_none());
            assert_eq!(
                crate::sim::cell_rect::get_cellclass_fallback(Some(&grid), 0, 0).dummy_snapshot(),
                Some(SharedCellDummySnapshot {
                    coord: (0, 0),
                    level: 0,
                    slope_type: 0,
                    bridge_flags_0x1180: 0,
                })
            );
            let path_grid = crate::sim::pathfinding::PathGrid::from_resolved_terrain(&grid);
            assert!(!path_grid.is_walkable(0, 0));
            let terrain_cost =
                crate::sim::pathfinding::terrain_cost::TerrainCostGrid::from_resolved_terrain(
                    &grid,
                    crate::rules::locomotor_type::SpeedType::Track,
                );
            assert_eq!(terrain_cost.cost_at(0, 0), 0);
            assert_eq!(selector.map_fill_scenario_advance_count(), 10);
            assert!(selector.generated_table());
            assert!(selector.raw_draw_count() > 0);
            main_draw_count = selector.raw_draw_count();
        }
        drop(main_draw);
        drop(scenario_fill_ranged);
        assert_eq!(
            scenario_rng.logical_state(),
            expected_scenario.logical_state()
        );
        let mut expected_main = SimRng::new(seed);
        for _ in 0..main_draw_count {
            let _ = expected_main.next_u32();
        }
        assert_eq!(main_rng.logical_state(), expected_main.logical_state());
        assert_ne!(main_rng.logical_state(), main_before);

        // The production orchestrator publishes the current theater only
        // after Fill and variant resolution have released their snapshots.
        cache.complete_theater_registry_load(
            theater.rmg_tiles.clear_tile,
            theater.rmg_tiles.water_set,
        );
        assert_eq!(cache.cached_clear_tile_base(), 0);
        assert_eq!(cache.cached_water_set_base(), 1);
    }

    /// `CellClass::IsTubeCell` 0x00484AB0 requires BOTH a tube index inside
    /// `[0, g_TubeCount)` and `cell+0xEC` (LandType) == 10, but
    /// `CellClass::GetTubeAtCell` 0x00484F20 tests only the index and hands
    /// back the record regardless of land type. The asymmetry is deliberate in
    /// the binary and load-bearing for tube exits, whose facing is read from
    /// the record after the cell has already been left behind. Keep both sides
    /// of it.
    #[test]
    fn tube_at_cell_ignores_land_type_while_is_tube_cell_requires_it() {
        let mut cells = vec![make_test_cell(0, 0), make_test_cell(1, 0)];
        cells[0].tube_index = Some(TubeId(0));
        cells[0].yr_cell_land_type = YR_CELL_LAND_TUNNEL;
        cells[1].tube_index = Some(TubeId(0));
        // Land type deliberately NOT tunnel.
        let tube = TubeFact {
            entry: (0, 0),
            exit: (1, 0),
            direction: 2,
            path_steps: Vec::new(),
            source: TubeSource::AutoLowBridge,
        };
        let grid = ResolvedTerrainGrid::from_cells_with_tubes(2, 1, cells, vec![tube]);

        assert!(grid.cell(0, 0).expect("cell").is_low_bridge_tube_cell());
        assert!(
            !grid.cell(1, 0).expect("cell").is_low_bridge_tube_cell(),
            "0x00484AB0 requires LandType 10"
        );
        assert!(
            grid.tube_at_cell(1, 0).is_some(),
            "0x00484F20 applies no LandType test"
        );
    }

    #[test]
    fn automatic_shell_directions_are_indexed_by_tile_offset_for_every_family() {
        let theater = synthetic_automatic_shell_theater();
        let mut cells = Vec::new();
        for (rx, set_start) in [0i32, 5, 10, 15].into_iter().enumerate() {
            for offset in 0..4i32 {
                let mut cell = make_test_cell((rx * 4 + offset as usize) as u16, 0);
                cell.final_tile_index = set_start + offset;
                cell.yr_cell_land_type = YR_CELL_LAND_TUNNEL;
                cells.push(cell);
            }
        }
        let mut tubes = Vec::new();

        build_auto_low_bridge_tubes(&mut cells, 16, 1, Some(&theater), &mut tubes);

        assert_eq!(tubes.len(), 16);
        for (index, tube) in tubes.iter().enumerate() {
            let expected_direction = i32::from(AUTO_TUBE_DIRECTIONS[index % 4]);
            let coord = (index as u16, 0);
            assert_eq!(tube.entry, coord);
            assert_eq!(tube.exit, coord);
            assert_eq!(tube.direction, expected_direction);
            assert!(tube.path_steps.is_empty());
            assert_eq!(tube.source, TubeSource::AutoLowBridge);
            assert_eq!(cells[index].tube_index, Some(TubeId(index as u16)));
        }
    }

    #[test]
    fn automatic_shell_predicate_rejects_each_native_boundary_failure() {
        let theater = synthetic_automatic_shell_theater();
        assert_eq!(yr_cell_land_type_from_tmp(5), YR_CELL_LAND_TUNNEL);
        assert_ne!(yr_cell_land_type_from_tmp(4), YR_CELL_LAND_TUNNEL);
        assert_eq!(auto_tube_direction_for_tile(0, Some(&theater)), Some(2));
        assert_eq!(auto_tube_direction_for_tile(3, Some(&theater)), Some(0));
        assert_eq!(
            auto_tube_direction_for_tile(4, Some(&theater)),
            None,
            "the fifth tile of the configured band is excluded"
        );
        assert_eq!(auto_tube_direction_for_tile(0, None), None);

        let mut no_families = synthetic_automatic_shell_theater();
        no_families.tunnels = None;
        no_families.track_tunnels = None;
        no_families.dirt_tunnels = None;
        no_families.dirt_track_tunnels = None;
        no_families.automatic_tube_bases = [-1; 4];
        assert_eq!(
            auto_tube_direction_for_tile(0, Some(&no_families)),
            Some(4),
            "missing native bases remain signed -1 and admit tile zero as ordinal one"
        );

        let mut zero_length = synthetic_theater_from_ini(
            b"[TileSet0000]\nTilesInSet=0\nFileName=empty\nSetName=Empty Tunnel\n\n\
              [TileSet0001]\nTilesInSet=5\nFileName=next\nSetName=Following Set\n",
        );
        zero_length.tunnels = Some(0);
        zero_length.automatic_tube_bases[0] = 0;
        for (tile_id, direction) in AUTO_TUBE_DIRECTIONS.into_iter().enumerate() {
            assert_eq!(
                auto_tube_direction_for_tile(tile_id as i32, Some(&zero_length)),
                Some(direction),
                "native compares the zero-length set's cumulative base without its count"
            );
        }
        assert_eq!(auto_tube_direction_for_tile(4, Some(&zero_length)), None);

        let mut wrong_land = make_test_cell(0, 0);
        wrong_land.final_tile_index = 0;
        wrong_land.yr_cell_land_type = LandType::Road.as_index();
        let mut existing = make_test_cell(1, 0);
        existing.final_tile_index = 0;
        existing.yr_cell_land_type = YR_CELL_LAND_TUNNEL;
        existing.tube_index = Some(TubeId(0));
        let mut fifth_tile = make_test_cell(2, 0);
        fifth_tile.final_tile_index = 4;
        fifth_tile.yr_cell_land_type = YR_CELL_LAND_TUNNEL;
        let mut cells = vec![wrong_land, existing, fifth_tile];
        let mut tubes = vec![TubeFact::explicit((1, 0), (1, 0), 2, Vec::new())];

        build_auto_low_bridge_tubes(&mut cells, 3, 1, Some(&theater), &mut tubes);

        assert_eq!(tubes.len(), 1);
        assert_eq!(cells[0].tube_index, None);
        assert_eq!(cells[1].tube_index, Some(TubeId(0)));
        assert_eq!(cells[2].tube_index, None);
    }

    #[test]
    fn authored_recalc_constructs_and_retains_one_automatic_tube_inline() {
        let mut theater = synthetic_theater_from_ini(
            b"[TileSet0000]\nTilesInSet=1\nFileName=tunnel\nSetName=Tunnel\n",
        );
        theater.automatic_tube_bases = [0, -1, -1, -1];
        let tmp = gsi_04_02_last_tiles_tmp_bytes(5, [1, 2, 3], [4, 5, 6]);
        let (_directory, assets) = gsi_04_04_asset_manager_with_loose_tmp("tunnel01.tem", &tmp);
        let terrain_rules = TerrainRules::from_ini(&IniFile::from_str(""));
        let registry = OverlayTypeRegistry::from_ini(&IniFile::from_str(""), None);
        let map = make_map(Vec::new(), Vec::new(), Vec::new());
        let mut grid =
            ResolvedTerrainGrid::from_cells(2, 1, vec![make_test_cell(0, 0), make_test_cell(1, 0)]);
        let mut state = LoadCellRecalcState::for_authored_load(
            &map,
            &theater,
            &assets,
            &terrain_rules,
            &registry,
            false,
            0,
            grid.cells.len(),
        );
        let mut effects = LoadRecalcTestEffects {
            tube_allocation: Some(AutomaticTubeAllocation::Allocated {
                native_unique_id: 1_010_001,
                registry_append_allowed: true,
            }),
            ..LoadRecalcTestEffects::default()
        };

        grid.recalc_authored_load_cell(
            &mut state,
            1,
            FinalizedOverlayCell::default(),
            &mut effects,
        )
        .expect("first authored tunnel Recalc");

        assert_eq!(
            effects.tube_requests,
            vec![AutomaticTubeRequest {
                cell: (1, 0),
                direction: 2,
            }]
        );
        assert_eq!(grid.cells[1].tube_index, Some(TubeId(0)));
        assert_eq!(grid.tube_native_id(TubeId(0)), Some(1_010_001));
        assert_eq!(
            grid.tube_at_cell(1, 0).unwrap().source,
            TubeSource::AutomaticRecalcShell
        );

        grid.recalc_authored_load_cell(
            &mut state,
            1,
            FinalizedOverlayCell::default(),
            &mut effects,
        )
        .expect("later authored tunnel Recalc");
        assert_eq!(
            effects.tube_requests.len(),
            1,
            "the published in-range signed index suppresses duplicate construction"
        );
    }

    #[test]
    fn binding_explicit_native_tubes_publishes_identity_and_suppresses_rebind() {
        let mut grid =
            ResolvedTerrainGrid::from_cells(2, 1, vec![make_test_cell(0, 0), make_test_cell(1, 0)]);
        let receipt = crate::map::tubes::NativeMapTubeReceipt {
            entries: vec![crate::map::tubes::ConstructedMapTube {
                fact: TubeFact::explicit((1, 0), (0, 0), 6, vec![2, 4]),
                native_init: crate::map::tubes::TubeNativeInit {
                    source_entry_ordinal: 0,
                    native_unique_id: 1_010_000,
                },
            }],
        };

        assert_eq!(grid.bind_native_map_tubes(receipt).unwrap(), 1);
        assert_eq!(grid.cells[1].tube_index, Some(TubeId(0)));
        assert_eq!(grid.tube_native_id(TubeId(0)), Some(1_010_000));
        assert_eq!(grid.tube_at_cell(1, 0).unwrap().path_steps, vec![2, 4]);
        assert!(matches!(
            grid.bind_native_map_tubes(crate::map::tubes::NativeMapTubeReceipt::default()),
            Err(NativeTubeRegistryBindError::RegistryAlreadyPopulated)
        ));
    }

    #[test]
    fn direction_8_steps_through_cell_tube() {
        let mut cells = vec![
            make_test_cell(0, 0),
            make_test_cell(1, 0),
            make_test_cell(2, 0),
        ];
        cells[1].yr_cell_land_type = YR_CELL_LAND_TUNNEL;
        cells[1].tube_index = Some(TubeId(0));
        let tube = TubeFact {
            entry: (1, 0),
            exit: (2, 0),
            direction: 2,
            path_steps: Vec::new(),
            source: TubeSource::AutoLowBridge,
        };
        let grid = ResolvedTerrainGrid::from_cells_with_tubes(3, 1, cells, vec![tube]);

        assert_eq!(grid.step_coord_by_direction((1, 0), 8), Some((2, 0)));
        assert_eq!(grid.walk_directions_from((0, 0), &[2, 8]), Some((2, 0)));
    }

    #[test]
    fn direction_8_without_valid_tube_returns_zero_coord() {
        let grid = ResolvedTerrainGrid::from_cells(1, 1, vec![make_test_cell(0, 0)]);

        assert_eq!(grid.step_coord_by_direction((0, 0), 8), Some((0, 0)));
    }

    #[test]
    fn invalid_non_8_direction_does_not_wrap() {
        let cells = (0..3)
            .flat_map(|ry| (0..3).map(move |rx| make_test_cell(rx, ry)))
            .collect();
        let grid = ResolvedTerrainGrid::from_cells(3, 3, cells);

        assert_eq!(grid.step_coord_by_direction((1, 1), 9), None);
        assert_eq!(grid.step_coord_by_direction((1, 1), 255), None);
    }

    #[test]
    fn gsi_02_11_clear_fallback_calls_selector_even_with_pristine_only() {
        let map = make_map(
            vec![MapCell {
                rx: 1,
                ry: 4,
                tile_index: theater::NO_TILE,
                sub_tile: 7,
                z: 0,
            }],
            Vec::new(),
            Vec::new(),
        );
        let theater = synthetic_theater_from_ini(
            b"[General]\nClearTile=0\n[TileSet0000]\nTilesInSet=1\nFileName=clear\nSetName=Clear\n",
        );
        assert_eq!(theater.lookup.total_file_count(0), 1);

        let mut cache = crate::map::tile_variant_selector::TileVariantSelectorCache::default();
        let mut draws = 0u32;
        let mut raw_draw = || {
            let value = draws;
            draws = draws.wrapping_add(1);
            value
        };
        let mut scenario_fill_ranged = |_low, _high| 0;
        {
            let mut selector = cache.begin_load(&mut raw_draw);
            let grid = ResolvedTerrainGrid::build_with_variant_selector(
                &map,
                Some(&theater),
                None,
                None,
                None,
                None,
                false,
                0,
                &mut scenario_fill_ranged,
                &mut selector,
            );
            assert_eq!(grid.cell(1, 4).expect("fallback cell").variant, 0);
            assert!(selector.generated_table());
            assert!(selector.raw_draw_count() > 0);
        }
        drop(raw_draw);
        assert!(draws > 0);
    }

    #[test]
    fn gsi_02_11_positive_registry_oor_uses_clear_and_invokes_selector() {
        let map = make_map(
            vec![MapCell {
                rx: 1,
                ry: 4,
                tile_index: 1,
                sub_tile: 7,
                z: 2,
            }],
            Vec::new(),
            Vec::new(),
        );
        let theater = synthetic_theater_from_ini(
            b"[General]\nClearTile=0\n[TileSet0000]\nTilesInSet=1\nFileName=clear\nSetName=Clear\n",
        );
        assert_eq!(theater.lookup.len(), 1);
        assert_eq!(theater.lookup.total_file_count(0), 1);

        let mut cache = crate::map::tile_variant_selector::TileVariantSelectorCache::default();
        let mut draws = 0u32;
        let mut raw_draw = || {
            let value = draws;
            draws = draws.wrapping_add(1);
            value
        };
        let mut scenario_fill_ranged = |_low, _high| 0;
        {
            let mut selector = cache.begin_load(&mut raw_draw);
            let grid = ResolvedTerrainGrid::build_with_variant_selector(
                &map,
                Some(&theater),
                None,
                None,
                None,
                None,
                false,
                0,
                &mut scenario_fill_ranged,
                &mut selector,
            );
            let cell = grid.cell(1, 4).expect("positive out-of-range cell");
            assert_eq!(cell.final_tile_index, 1);
            assert_eq!(cell.final_sub_tile, 7);
            assert_eq!(grid.presentation_tile(cell), (0, 0));
            assert_eq!(cell.variant, 0);
            assert!(selector.generated_table());
            assert!(selector.raw_draw_count() > 0);
        }
        drop(raw_draw);
        assert!(draws > 0);
    }

    #[test]
    fn explicit_map_tubes_seed_resolved_grid() {
        let mut map = make_map(
            vec![
                MapCell {
                    rx: 0,
                    ry: 0,
                    tile_index: 0,
                    sub_tile: 0,
                    z: 0,
                },
                MapCell {
                    rx: 1,
                    ry: 0,
                    tile_index: 0,
                    sub_tile: 0,
                    z: 0,
                },
                MapCell {
                    rx: 2,
                    ry: 0,
                    tile_index: 0,
                    sub_tile: 0,
                    z: 0,
                },
            ],
            Vec::new(),
            Vec::new(),
        );
        map.explicit_tubes = vec![TubeFact::explicit((0, 0), (2, 0), 2, vec![2, 2])];

        let grid = ResolvedTerrainGrid::build(&map, None, None, None, None, false, 0);

        let cell = grid.cell(0, 0).expect("entry cell");
        assert_eq!(cell.tube_index, Some(TubeId(0)));
        assert_eq!(grid.tube_facts().len(), 1);
        assert_eq!(grid.tube_facts()[0].source, TubeSource::ExplicitMap);
        assert_eq!(grid.step_coord_by_direction((0, 0), 8), Some((2, 0)));
    }

    #[test]
    fn test_resolved_grid_preserves_raw_fields_and_fills_clear_cells() {
        let map = make_map(
            vec![MapCell {
                rx: 1,
                ry: 1,
                tile_index: 5,
                sub_tile: 3,
                z: 2,
            }],
            Vec::new(),
            Vec::new(),
        );
        let grid = ResolvedTerrainGrid::build(&map, None, None, None, None, false, 0);
        assert_eq!(grid.width(), 2);
        assert_eq!(grid.height(), 2);

        let cell = grid.cell(1, 1).expect("resolved cell");
        assert_eq!(cell.source_tile_index, 5);
        assert_eq!(cell.source_sub_tile, 3);
        assert_eq!(cell.final_tile_index, 5);
        assert_eq!(cell.final_sub_tile, 3);
        assert_eq!(cell.level, 2);
        assert!(!cell.filled_clear);

        let clear = grid.cell(0, 0).expect("filled clear");
        assert!(clear.filled_clear);
        assert_eq!(clear.final_tile_index, 0);
        assert_eq!(clear.level, 0);
    }

    #[test]
    fn no_tile_presentation_uses_theater_clear_tile_without_rewriting_semantics() {
        let ini = b"[TileSet0000]\n\
                    TilesInSet=37\n\
                    FileName=plain\n\
                    SetName=Plain\n\
                    \n\
                    [TileSet0001]\n\
                    TilesInSet=1\n\
                    FileName=clear\n\
                    SetName=Clear\n";
        let mut theater_data = synthetic_theater_from_ini(ini);
        theater_data.rmg_tiles.clear_tile = Some(37);
        let map = make_map(
            vec![MapCell {
                rx: 1,
                ry: 0,
                tile_index: theater::NO_TILE,
                sub_tile: 7,
                z: 3,
            }],
            Vec::new(),
            Vec::new(),
        );

        let grid =
            ResolvedTerrainGrid::build(&map, Some(&theater_data), None, None, None, false, 0);
        let sentinel = grid.cell(1, 0).expect("sentinel cell");

        assert_eq!(sentinel.final_tile_index, theater::NO_TILE);
        assert_eq!(sentinel.final_sub_tile, 7);
        assert_eq!(sentinel.level, 3);
        assert_eq!(grid.presentation_tile(sentinel), (37, 0));

        let presentation = crate::map::terrain::build_terrain_grid_from_resolved(&grid, None, None);
        let rendered = presentation
            .cells
            .iter()
            .find(|cell| (cell.rx, cell.ry) == (1, 0))
            .expect("sentinel reaches presentation grid");
        assert_eq!(rendered.tile_id, 37);
        assert_eq!(rendered.sub_tile, 0);
        assert_eq!(rendered.z, 3);

        let used = crate::map::theater::collect_used_tiles(
            &presentation
                .cells
                .iter()
                .map(|cell| (cell.tile_id as i32, cell.sub_tile))
                .collect::<Vec<_>>(),
        );
        assert!(used.contains(&TileKey {
            tile_id: 37,
            sub_tile: 0,
            variant: 0,
        }));
    }

    #[test]
    fn wood_bridge_repair_tile_uses_first_16_tiles_of_wood_bridge_set() {
        let theater = synthetic_theater_with_wood_bridge_set();
        let map = make_map(
            vec![
                MapCell {
                    rx: 0,
                    ry: 0,
                    tile_index: 9,
                    sub_tile: 0,
                    z: 0,
                },
                MapCell {
                    rx: 1,
                    ry: 0,
                    tile_index: 10,
                    sub_tile: 0,
                    z: 0,
                },
                MapCell {
                    rx: 2,
                    ry: 0,
                    tile_index: 25,
                    sub_tile: 0,
                    z: 0,
                },
                MapCell {
                    rx: 3,
                    ry: 0,
                    tile_index: 26,
                    sub_tile: 0,
                    z: 0,
                },
            ],
            Vec::new(),
            Vec::new(),
        );

        let grid = ResolvedTerrainGrid::build(&map, Some(&theater), None, None, None, false, 0);

        assert!(!grid.cell(0, 0).unwrap().is_wood_bridge_repair_tile);
        assert!(grid.cell(1, 0).unwrap().is_wood_bridge_repair_tile);
        assert!(grid.cell(2, 0).unwrap().is_wood_bridge_repair_tile);
        assert!(!grid.cell(3, 0).unwrap().is_wood_bridge_repair_tile);
    }

    #[test]
    fn gsi_04_03a_special_terrain_identity_is_not_promoted_to_hard_blocking() {
        let ini = b"[TileSet0000]\nTilesInSet=10\nFileName=clear\nSetName=Clear\n\n\
                    [TileSet0001]\nTilesInSet=10\nFileName=plain\nSetName=Plain Terrain\n";
        let mut theater = synthetic_theater_from_ini(ini);
        theater.cliff_ranges.cliff_set = Some(10);
        let map = make_map(
            vec![MapCell {
                rx: 0,
                ry: 0,
                tile_index: 10,
                sub_tile: 0,
                z: 0,
            }],
            Vec::new(),
            Vec::new(),
        );

        let grid = ResolvedTerrainGrid::build(&map, Some(&theater), None, None, None, false, 0);
        let cell = grid.cell(0, 0).expect("special-identity cell");

        assert!(theater.cliff_ranges.is_special_terrain_tile(10, 0));
        assert!(!cell.is_cliff_like);
        assert!(!cell.base_ground_walk_blocked);
        assert!(!cell.ground_walk_blocked);
        assert!(!cell.build_blocked);
        assert_eq!(cell.terrain_class, TerrainClass::Clear);
    }

    #[test]
    fn shore_set_name_alone_does_not_block_ground() {
        let ini = b"[TileSet0000]\nTilesInSet=10\nFileName=shore\nSetName=Shore Pieces\n";
        let theater = synthetic_theater_from_ini(ini);
        let map = make_map(
            vec![MapCell {
                rx: 0,
                ry: 0,
                tile_index: 0,
                sub_tile: 0,
                z: 0,
            }],
            Vec::new(),
            Vec::new(),
        );

        let grid = ResolvedTerrainGrid::build(&map, Some(&theater), None, None, None, false, 0);
        let cell = grid.cell(0, 0).expect("shore cell");

        assert!(!cell.is_cliff_like);
        assert!(!cell.base_ground_walk_blocked);
        assert!(!cell.ground_walk_blocked);
        assert_eq!(cell.terrain_class, TerrainClass::Clear);
    }

    #[test]
    fn resolved_terrain_allow_tiberium_uses_final_lat_tile() {
        let ini = b"[General]\n\
                    RoughTile=1\n\
                    ClearToRoughLat=2\n\
                    \n\
                    [TileSet0000]\n\
                    SetName=Clear\n\
                    FileName=clear\n\
                    TilesInSet=5\n\
                    \n\
                    [TileSet0001]\n\
                    SetName=Rough\n\
                    FileName=rough\n\
                    TilesInSet=5\n\
                    \n\
                    [TileSet0002]\n\
                    SetName=ClearToRough\n\
                    FileName=crgh\n\
                    TilesInSet=16\n\
                    AllowTiberium=true\n";
        let theater = synthetic_theater_from_ini(ini);
        let map = make_map(
            vec![
                MapCell {
                    rx: 5,
                    ry: 5,
                    tile_index: 5,
                    sub_tile: 0,
                    z: 0,
                },
                MapCell {
                    rx: 5,
                    ry: 4,
                    tile_index: 0,
                    sub_tile: 0,
                    z: 0,
                },
                MapCell {
                    rx: 4,
                    ry: 5,
                    tile_index: 6,
                    sub_tile: 0,
                    z: 0,
                },
                MapCell {
                    rx: 6,
                    ry: 5,
                    tile_index: 1,
                    sub_tile: 0,
                    z: 0,
                },
                MapCell {
                    rx: 5,
                    ry: 6,
                    tile_index: 7,
                    sub_tile: 0,
                    z: 0,
                },
            ],
            Vec::new(),
            Vec::new(),
        );

        let grid = ResolvedTerrainGrid::build(&map, Some(&theater), None, None, None, true, 0);
        let cell = grid.cell(5, 5).expect("center LAT cell");

        assert_eq!(cell.source_tile_index, 5);
        assert_eq!(cell.final_tile_index, 25);
        assert!(cell.allows_tiberium);
    }

    #[test]
    fn gsi_04_03c_height_in_pixels_uses_signed_effective_height_formula() {
        for (relative_extra_y, expected) in [(0, 0), (-1, 0), (-15, 1), (-30, 2), (15, -1)] {
            assert_eq!(
                height_in_pixels_from_tmp(30, relative_extra_y),
                Some(expected),
                "relative extra-Y {relative_extra_y}"
            );
        }
    }

    #[test]
    fn gsi_04_03c_sparse_pristine_subtile_uses_header_height() {
        let tmp = TmpFile {
            template_width: 1,
            template_height: 1,
            tile_width: 60,
            tile_height: 45,
            tiles: vec![None],
        };
        let mut metadata = TileMetadata::default();
        let mut warned = HashSet::new();

        merge_tmp_file_metadata(&mut metadata, &tmp, 0, None, &mut warned);

        assert_eq!(metadata.height_in_pixels, 1);
    }

    #[test]
    fn gsi_04_04_sparse_and_positive_oob_subtiles_clear_cell_land_and_slope() {
        let rock = TmpTile {
            height: 7,
            terrain_type: 7,
            ramp_type: 3,
            radar_left: [1, 2, 3],
            radar_right: [4, 5, 6],
            pixels: Vec::new(),
            depth: Vec::new(),
            pixel_width: 60,
            pixel_height: 30,
            relative_extra_y: -30,
            offset_x: -4,
            offset_y: -5,
            has_damaged_data: false,
        };
        let tmp = TmpFile {
            template_width: 2,
            template_height: 1,
            tile_width: 60,
            tile_height: 45,
            tiles: vec![Some(rock), None],
        };

        for sub_tile in [1, 2] {
            let mut metadata = metadata_from_set_name(Some("Road"), None);
            let mut warned = HashSet::new();
            merge_tmp_file_metadata(&mut metadata, &tmp, sub_tile, None, &mut warned);

            assert_eq!(metadata.subtile_entry_valid, Some(false));
            assert_eq!(metadata.land_type, LandType::Clear.as_index());
            assert_eq!(metadata.yr_cell_land_type, LandType::Clear.as_index());
            assert_eq!(metadata.slope_type, 0);
            assert!(!metadata.has_ramp);
            assert_eq!(
                metadata.height_in_pixels, 1,
                "header fallback remains Cell-owned for subtile {sub_tile}"
            );
        }
    }

    #[test]
    fn gsi_04_04_valid_tile_sparse_and_oob_entries_materialize_no_tile_cells() {
        let mut theater = synthetic_theater_from_ini(
            b"[General]\nClearTile=0\n\
              [TileSet0000]\nTilesInSet=1\nFileName=sparse\nSetName=Wood Bridge\n\
              Morphable=true\nAllowTiberium=true\nTile01Anim=SPARSEANIM\n\
              Tile01AttachesTo=1\n",
        );
        theater.wood_bridge_set = Some(0);
        let tmp_filename = theater
            .lookup
            .filename_for_variant(0, 0)
            .expect("synthetic tile filename")
            .to_string();
        let (_directory, assets) =
            gsi_04_04_asset_manager_with_loose_tmp(&tmp_filename, &gsi_04_04_sparse_tmp_bytes());
        let map = make_map(
            vec![
                MapCell {
                    rx: 1,
                    ry: 0,
                    tile_index: theater::NO_TILE,
                    sub_tile: 0,
                    z: 4,
                },
                MapCell {
                    rx: 3,
                    ry: 0,
                    tile_index: theater::NO_TILE,
                    sub_tile: 0,
                    z: 4,
                },
                MapCell {
                    rx: 1,
                    ry: 1,
                    tile_index: 0,
                    sub_tile: 1,
                    z: 0,
                },
                MapCell {
                    rx: 3,
                    ry: 1,
                    tile_index: 0,
                    sub_tile: 2,
                    z: 0,
                },
            ],
            Vec::new(),
            Vec::new(),
        );

        let without_cliff_back =
            ResolvedTerrainGrid::build(&map, Some(&theater), Some(&assets), None, None, false, 0);
        assert!(without_cliff_back.tile_animations().is_empty());
        for (rx, source_sub_tile) in [(1, 1), (3, 2)] {
            let cell = without_cliff_back
                .cell(rx, 1)
                .expect("materialized sparse/OOB cell");
            assert_eq!(cell.source_tile_index, 0);
            assert_eq!(cell.source_sub_tile, source_sub_tile);
            assert_eq!(cell.final_tile_index, 0xFFFF);
            assert_eq!(cell.final_sub_tile, 0);
            assert_eq!(cell.land_type, LandType::Clear.as_index());
            assert_eq!(cell.yr_cell_land_type, LandType::Clear.as_index());
            assert_eq!(cell.slope_type, 0);
            assert!(!cell.has_ramp);
            assert_eq!(cell.template_height, 0);
            assert_eq!(cell.height_in_pixels, 1);
            assert_eq!(cell.tileset_index, None);
            assert_eq!(cell.render_offset_x, 0);
            assert_eq!(cell.render_offset_y, 0);
            assert_eq!(cell.radar_left, [0, 0, 0]);
            assert_eq!(cell.radar_right, [0, 0, 0]);
            assert!(!cell.accepts_smudge);
            assert!(!cell.allows_tiberium);
            assert!(!cell.is_wood_bridge_repair_tile);
            assert_eq!(cell.variant, 0);
            assert_eq!(without_cliff_back.presentation_tile(cell), (0, 0));
        }

        let with_cliff_back =
            ResolvedTerrainGrid::build(&map, Some(&theater), Some(&assets), None, None, false, 2);
        for rx in [1, 3] {
            let cell = with_cliff_back.cell(rx, 1).expect("copy-2 CliffBack cell");
            assert_eq!(cell.final_tile_index, 0xFFFF);
            assert_eq!(cell.final_sub_tile, 0);
            assert_eq!(cell.slope_type, 0);
            assert_eq!(cell.height_in_pixels, 1);
            assert_eq!(cell.land_type, LandType::Rock.as_index());
            assert_eq!(cell.base_land_type, LandType::Rock.as_index());
            assert_eq!(cell.zone_type, zone_class::IMPASSABLE);
        }
    }

    #[test]
    fn gsi_04_04_merge_tmp_metadata_maps_raw_rock_to_canonical_land() {
        let mut metadata = TileMetadata::default();
        let tile = TmpTile {
            height: 4,
            terrain_type: 7,
            ramp_type: 2,
            radar_left: [100, 120, 80],
            radar_right: [90, 110, 70],
            pixels: Vec::new(),
            depth: Vec::new(),
            pixel_width: 60,
            pixel_height: 30,
            relative_extra_y: 0,
            offset_x: -5,
            offset_y: -6,
            has_damaged_data: false,
        };
        merge_tmp_metadata(&mut metadata, &tile);
        assert_eq!(metadata.land_type, LandType::Rock.as_index());
        assert_eq!(metadata.slope_type, 2);
        assert_eq!(metadata.template_height, 4);
        assert_eq!(metadata.render_offset_x, -5);
        assert_eq!(metadata.render_offset_y, -6);
        assert!(metadata.has_ramp);
        assert!(metadata.has_tmp_metadata);
        assert_eq!(metadata.radar_left, [100, 120, 80]);
        assert_eq!(metadata.radar_right, [90, 110, 70]);
    }

    #[test]
    fn gsi_04_03c_pristine_owns_height_in_pixels_across_selected_presentation() {
        let pristine = TmpFile {
            template_width: 1,
            template_height: 1,
            tile_width: 60,
            tile_height: 30,
            tiles: vec![Some(TmpTile {
                height: 2,
                terrain_type: 1,
                ramp_type: 1,
                radar_left: [1, 2, 3],
                radar_right: [4, 5, 6],
                pixels: Vec::new(),
                depth: Vec::new(),
                pixel_width: 60,
                pixel_height: 30,
                relative_extra_y: 0,
                offset_x: -3,
                offset_y: -4,
                has_damaged_data: false,
            })],
        };
        let divergent_selected = TmpFile {
            template_width: 2,
            template_height: 1,
            tile_width: 60,
            tile_height: 30,
            tiles: vec![
                Some(TmpTile {
                    height: 7,
                    terrain_type: 7,
                    ramp_type: 3,
                    radar_left: [101, 102, 103],
                    radar_right: [104, 105, 106],
                    pixels: Vec::new(),
                    depth: Vec::new(),
                    pixel_width: 64,
                    pixel_height: 35,
                    relative_extra_y: -30,
                    offset_x: -8,
                    offset_y: -9,
                    has_damaged_data: true,
                }),
                None,
            ],
        };
        let mut warned = HashSet::new();
        let mut metadata = TileMetadata::default();
        merge_tmp_file_metadata(&mut metadata, &pristine, 0, None, &mut warned);
        let pristine_metadata = metadata.clone();
        let mut selected_metadata = TileMetadata::default();
        merge_tmp_file_metadata(
            &mut selected_metadata,
            &divergent_selected,
            0,
            None,
            &mut warned,
        );
        assert_ne!(
            selected_metadata.raw_land_type,
            pristine_metadata.raw_land_type
        );
        assert_ne!(selected_metadata.slope_type, pristine_metadata.slope_type);
        assert_ne!(
            selected_metadata.template_height,
            pristine_metadata.template_height
        );
        assert_ne!(
            selected_metadata.has_damaged_data,
            pristine_metadata.has_damaged_data
        );
        assert_eq!(pristine_metadata.height_in_pixels, 0);
        assert_eq!(selected_metadata.height_in_pixels, 2);

        apply_selected_presentation_metadata(&mut metadata, &selected_metadata);

        assert_eq!(metadata.raw_land_type, pristine_metadata.raw_land_type);
        assert_eq!(metadata.land_type, pristine_metadata.land_type);
        assert_eq!(metadata.slope_type, pristine_metadata.slope_type);
        assert_eq!(metadata.template_height, pristine_metadata.template_height);
        assert_eq!(
            metadata.height_in_pixels,
            pristine_metadata.height_in_pixels
        );
        assert_eq!(
            metadata.template_width_cells,
            pristine_metadata.template_width_cells
        );
        assert_eq!(
            metadata.has_damaged_data,
            pristine_metadata.has_damaged_data
        );
        assert_eq!(metadata.radar_left, [101, 102, 103]);
        assert_eq!(metadata.radar_right, [104, 105, 106]);
        assert_eq!(metadata.render_offset_x, -8);
        assert_eq!(metadata.render_offset_y, -9);

        let sparse_suffix = TmpFile {
            template_width: 2,
            template_height: 1,
            tile_width: 60,
            tile_height: 30,
            tiles: vec![None, None],
        };
        let mut sparse_selected_metadata = TileMetadata::default();
        merge_tmp_file_metadata(
            &mut sparse_selected_metadata,
            &sparse_suffix,
            0,
            None,
            &mut warned,
        );
        let mut sparse_result = pristine_metadata.clone();
        apply_selected_presentation_metadata(&mut sparse_result, &sparse_selected_metadata);
        assert_eq!(sparse_result.raw_land_type, pristine_metadata.raw_land_type);
        assert_eq!(sparse_result.slope_type, pristine_metadata.slope_type);
        assert_eq!(
            sparse_result.template_height,
            pristine_metadata.template_height
        );
        assert_eq!(
            sparse_result.height_in_pixels,
            pristine_metadata.height_in_pixels
        );
        assert_eq!(
            sparse_result.has_damaged_data,
            pristine_metadata.has_damaged_data
        );
        assert_eq!(sparse_result.radar_left, [0, 0, 0]);
        assert_eq!(sparse_result.radar_right, [0, 0, 0]);
        assert_eq!(sparse_result.render_offset_x, 0);
        assert_eq!(sparse_result.render_offset_y, 0);
    }

    #[test]
    fn gsi_02_11_damaged_tmp_cell_bypasses_selector_without_table_generation() {
        let mut metadata = TileMetadata::default();
        let damaged_tile = TmpTile {
            height: 0,
            terrain_type: 0,
            ramp_type: 0,
            radar_left: [0, 0, 0],
            radar_right: [0, 0, 0],
            pixels: Vec::new(),
            depth: Vec::new(),
            pixel_width: 60,
            pixel_height: 30,
            relative_extra_y: 0,
            offset_x: 0,
            offset_y: 0,
            has_damaged_data: true,
        };
        merge_tmp_metadata(&mut metadata, &damaged_tile);
        assert!(metadata.has_damaged_data);

        let mut cache = crate::map::tile_variant_selector::TileVariantSelectorCache::default();
        let mut draws = 0usize;
        let mut raw_draw = || {
            draws += 1;
            0
        };
        let mut variant = 0u8;
        {
            let mut selector = cache.begin_load(&mut raw_draw);
            if ordinary_variant_selection_enabled(8, false, metadata.has_damaged_data) {
                variant = selector.select_variant(4, 4, 0, 1, 1, 8);
            }
            assert!(!selector.generated_table());
            assert_eq!(selector.raw_draw_count(), 0);
        }
        drop(raw_draw);
        assert_eq!(variant, 0);
        assert_eq!(draws, 0);
        assert!(!cache.is_initialized());
    }

    #[test]
    fn test_canonical_ramp_detection_only_marks_slope_types_one_to_four() {
        assert_eq!(canonical_ramp_from_slope_type(1), Some(RampDirection::West));
        assert_eq!(
            canonical_ramp_from_slope_type(4),
            Some(RampDirection::South)
        );
        assert_eq!(canonical_ramp_from_slope_type(0), None);
        assert_eq!(canonical_ramp_from_slope_type(7), None);
    }

    #[test]
    fn test_bridge_overlay_creates_upper_layer_without_ground_block() {
        // BRIDGE1 is hardcoded at overlay index 24 in the original engine.
        // Build a registry large enough so index 24 resolves to "BRIDGE1".
        let mut ini_str = String::from("[OverlayTypes]\n");
        for i in 0..24 {
            ini_str.push_str(&format!("{i}=FILLER{i}\n"));
        }
        ini_str.push_str("24=BRIDGE1\n");
        let ini = IniFile::from_str(&ini_str);
        let reg = OverlayTypeRegistry::from_ini(&ini, None);
        let effects = classify_overlay_effects(
            Some(&vec![&OverlayEntry {
                rx: 0,
                ry: 0,
                overlay_id: 24,
                frame: 0,
            }]),
            Some(&reg),
            3,
            0,
        );
        assert!(effects.has_bridge_deck);
        assert!(!effects.overlay_blocks);
        assert_eq!(
            effects
                .bridge_layer
                .as_ref()
                .map(|b| b.overlay_name.as_str()),
            Some("BRIDGE1")
        );
        // BRIDGE1 = EastWest high bridge: deck_level = ground(3) + HighBridgeHeight(4) = 7.
        assert_eq!(effects.bridge_layer.as_ref().map(|b| b.deck_level), Some(7));
        assert_eq!(
            effects.bridge_layer.as_ref().map(|b| b.direction),
            Some(BridgeDirection::EastWest)
        );
    }

    #[test]
    fn bridge_oracle_cell_facts_dump_stamped_flags_without_theater() {
        let mut ini_str = String::from("[OverlayTypes]\n");
        for i in 0..24 {
            ini_str.push_str(&format!("{i}=FILLER{i}\n"));
        }
        ini_str.push_str("24=BRIDGE1\n");
        let reg = OverlayTypeRegistry::from_ini(&IniFile::from_str(&ini_str), None);
        let map = make_map(
            vec![MapCell {
                rx: 5,
                ry: 5,
                tile_index: 0,
                sub_tile: 0,
                z: 0,
            }],
            vec![OverlayEntry {
                rx: 5,
                ry: 5,
                overlay_id: 0x18,
                frame: 0,
            }],
            Vec::new(),
        );

        let grid = ResolvedTerrainGrid::build(&map, None, None, None, Some(&reg), false, 0);
        let dump = grid.bridge_oracle_cell_facts(&[(5, 5)], None);

        assert_eq!(dump.len(), 1);
        assert_eq!(dump[0].rx, 5);
        assert!(dump[0].flag_0x80_anchor_self);
        assert!(dump[0].flag_0x100_structural);
        assert!(dump[0].flag_0x200_transition);
        assert_eq!(dump[0].bridge_set_member, None);
    }

    #[test]
    fn gsi_04_04_overlay_zone_type_priority_matches_recalc_zone_type_on_load() {
        let ini = IniFile::from_str(
            "\
[OverlayTypes]
0=SANDBAG
1=HARDWALL
2=ROCKOVL
3=RUBBLE
[Clear]
Wheel=100%
[Rock]
Wheel=0%
[SANDBAG]
Crushable=yes
Wall=yes
Land=Clear
[HARDWALL]
Wall=yes
Land=Clear
[ROCKOVL]
Land=Rock
[RUBBLE]
IsRubble=yes
",
        );
        let reg = OverlayTypeRegistry::from_ini(&ini, None);
        let map = make_map(
            vec![
                MapCell {
                    rx: 0,
                    ry: 0,
                    tile_index: 0,
                    sub_tile: 0,
                    z: 0,
                },
                MapCell {
                    rx: 1,
                    ry: 0,
                    tile_index: 0,
                    sub_tile: 0,
                    z: 0,
                },
                MapCell {
                    rx: 2,
                    ry: 0,
                    tile_index: 0,
                    sub_tile: 0,
                    z: 0,
                },
                MapCell {
                    rx: 3,
                    ry: 0,
                    tile_index: 0,
                    sub_tile: 0,
                    z: 0,
                },
            ],
            vec![
                OverlayEntry {
                    rx: 0,
                    ry: 0,
                    overlay_id: 0,
                    frame: 0,
                },
                OverlayEntry {
                    rx: 1,
                    ry: 0,
                    overlay_id: 1,
                    frame: 0,
                },
                OverlayEntry {
                    rx: 2,
                    ry: 0,
                    overlay_id: 2,
                    frame: 0,
                },
                OverlayEntry {
                    rx: 3,
                    ry: 0,
                    overlay_id: 3,
                    frame: 0,
                },
            ],
            (0..4)
                .map(|rx| TerrainObject {
                    rx,
                    ry: 0,
                    name: "DEFAULT_OCCUPATION".to_string(),
                })
                .collect(),
        );

        let grid = ResolvedTerrainGrid::build(&map, None, None, None, Some(&reg), false, 0);

        assert_eq!(grid.cell(0, 0).unwrap().zone_type, zone_class::CRUSHABLE);
        assert!(!grid.cell(0, 0).unwrap().overlay_blocks);
        assert_eq!(grid.cell(1, 0).unwrap().zone_type, zone_class::WALL);
        assert!(grid.cell(1, 0).unwrap().overlay_blocks);
        assert_eq!(grid.cell(2, 0).unwrap().zone_type, zone_class::IMPASSABLE);
        assert!(grid.cell(2, 0).unwrap().overlay_blocks);
        assert_eq!(grid.cell(3, 0).unwrap().zone_type, zone_class::GROUND);
        assert!(!grid.cell(3, 0).unwrap().overlay_blocks);
    }

    #[test]
    fn gsi_04_04_load_selects_current_theater_terrain_occupation() {
        let rules = RuleSet::from_ini(&IniFile::from_str(
            "\
[InfantryTypes]
[VehicleTypes]
[AircraftTypes]
[BuildingTypes]
[TerrainTypes]
0=TIBTRE03
1=ZEROTREE
[TIBTRE03]
TemperateOccupationBits=4
SnowOccupationBits=7
[ZEROTREE]
TemperateOccupationBits=0
SnowOccupationBits=0
",
        ))
        .expect("terrain rules");
        let mut map = make_map(
            vec![
                MapCell {
                    rx: 0,
                    ry: 0,
                    tile_index: 0,
                    sub_tile: 0,
                    z: 0,
                },
                MapCell {
                    rx: 1,
                    ry: 0,
                    tile_index: 0,
                    sub_tile: 0,
                    z: 0,
                },
            ],
            Vec::new(),
            vec![
                TerrainObject {
                    rx: 0,
                    ry: 0,
                    name: "TIBTRE03".to_string(),
                },
                TerrainObject {
                    rx: 1,
                    ry: 0,
                    name: "ZEROTREE".to_string(),
                },
            ],
        );

        let temperate = ResolvedTerrainGrid::build_inner(
            &map,
            None,
            None,
            None,
            None,
            Some(&rules.terrain_object_types),
            false,
            0,
            OverlayLoadSource::Authored,
            TerrainBuildProjection::EagerCompatibility,
            None,
            None,
            None,
        );
        let temperate_tree = temperate.cell(0, 0).unwrap();
        assert_eq!(temperate_tree.terrain_object_occupation, Some(4));
        assert!(temperate_tree.terrain_object_blocks);
        assert_eq!(temperate_tree.zone_type, zone_class::BUILDING);
        assert!(!temperate_tree.base_build_blocked);
        let zero_tree = temperate.cell(1, 0).unwrap();
        assert_eq!(zero_tree.terrain_object_occupation, Some(0));
        assert!(!zero_tree.terrain_object_blocks);
        assert_eq!(zero_tree.zone_type, zone_class::BUILDING);

        map.header.theater = "SNOW".to_string();
        let snow = ResolvedTerrainGrid::build_inner(
            &map,
            None,
            None,
            None,
            None,
            Some(&rules.terrain_object_types),
            false,
            0,
            OverlayLoadSource::Authored,
            TerrainBuildProjection::EagerCompatibility,
            None,
            None,
            None,
        );
        let snow_tree = snow.cell(0, 0).unwrap();
        assert_eq!(snow_tree.terrain_object_occupation, Some(7));
        assert!(snow_tree.terrain_object_blocks);
        assert_eq!(snow_tree.zone_type, zone_class::WALL);
    }

    #[test]
    fn gsi_04_04_outside_precedes_overlay_land_and_terrain_occupation() {
        let playfield = Playfield::from_local_size(34, 0, 0, 34, 42);
        let outside = !playfield.contains_raised(0, 0, 0, 0);
        assert!(outside);
        assert_eq!(
            recalc_zone_type(
                outside,
                Some(zone_class::CRUSHABLE),
                LandType::Water.as_index(),
                Some(0),
                Some(7),
            ),
            zone_class::OUTSIDE
        );
    }

    #[test]
    fn gsi_04_04_load_land_writer_retains_arbitrary_profiles_and_pristine_base() {
        let ini = IniFile::from_str(
            "\
[OverlayTypes]
0=ROADKEEP
1=ROADRESTORE
2=WALLLAND
3=RAILLAND
4=WATERKEEP
5=ROUGHKEEP
[Road]
Foot=80%
Wheel=55%
[Wall]
Wheel=40%
[Railroad]
Wheel=60%
[Water]
Float=66%
Wheel=44%
[Rough]
Foot=77%
Wheel=33%
[ROADKEEP]
Land=Road
NoUseTileLandType=yes
[ROADRESTORE]
Land=Road
NoUseTileLandType=no
[WALLLAND]
Land=Wall
NoUseTileLandType=no
[RAILLAND]
Land=Railroad
NoUseTileLandType=no
[WATERKEEP]
Land=Water
NoUseTileLandType=yes
[ROUGHKEEP]
Land=Rough
NoUseTileLandType=yes
",
        );
        let registry = OverlayTypeRegistry::from_ini(&ini, None);
        let cells = (0..6)
            .map(|rx| MapCell {
                rx,
                ry: 0,
                tile_index: 0,
                sub_tile: 0,
                z: 0,
            })
            .collect();
        let overlays = (0..6)
            .map(|overlay_id| OverlayEntry {
                rx: u16::from(overlay_id),
                ry: 0,
                overlay_id,
                frame: 0,
            })
            .collect();
        let map = make_map(cells, overlays, Vec::new());

        let grid = ResolvedTerrainGrid::build(&map, None, None, None, Some(&registry), false, 0);
        for (rx, overlay_id, land, class) in [
            (0, 0, LandType::Road, TerrainClass::Road),
            (2, 2, LandType::Wall, TerrainClass::Wall),
            (3, 3, LandType::Railroad, TerrainClass::Railroad),
            (4, 4, LandType::Water, TerrainClass::Water),
            (5, 5, LandType::Rough, TerrainClass::Rough),
        ] {
            let cell = grid.cell(rx, 0).expect("retained load cell");
            assert_eq!(cell.base_land_type, LandType::Clear.as_index());
            assert_eq!(cell.land_type, land.as_index(), "overlay {overlay_id}");
            assert_eq!(cell.terrain_class, class, "overlay {overlay_id}");
            assert_eq!(
                cell.speed_costs,
                registry
                    .flags(overlay_id)
                    .unwrap()
                    .land_speed_costs
                    .unwrap(),
                "overlay {overlay_id} profile"
            );
            assert_eq!(cell.is_water, land == LandType::Water);
            assert_eq!(cell.is_road, land == LandType::Road);
            assert_eq!(cell.is_rough, land == LandType::Rough);
        }

        let restored = grid.cell(1, 0).expect("ordinary restoration cell");
        assert_eq!(restored.land_type, restored.base_land_type);
        assert_eq!(restored.terrain_class, restored.base_terrain_class);
        assert_eq!(restored.speed_costs, restored.base_speed_costs);
    }

    #[test]
    fn low_bridge_overlay_land_replaces_the_tiles_ground_block_at_load() {
        // Low bridges (LOBRDG01..28 wood, LOBRDB01..28 concrete) are overlays
        // laid straight onto the water tiles they span, with `Land=Road` and
        // `NoUseTileLandType=yes`. `CellClass__RecalcAttributes` @ 0x0047D2B0
        // takes its early branch for exactly that pair and returns before the
        // tile's own subtile land type is read, so nothing of the water
        // survives — LandType is the only land attribute gamemd stores.
        let ini = IniFile::from_str(
            "[OverlayTypes]
0=LOBRDB10
1=WATERKEEP
[Road]
Foot=100%
Track=100%
Wheel=100%
[Water]
Float=100%
[LOBRDB10]
Land=Road
NoUseTileLandType=yes
[WATERKEEP]
Land=Water
NoUseTileLandType=yes
",
        );
        let registry = OverlayTypeRegistry::from_ini(&ini, None);

        // The blocked-ness of the replacement row travels with the land.
        assert!(!registry.flags(0).expect("bridge flags").land_ground_blocked);
        assert!(registry.flags(1).expect("water flags").land_ground_blocked);

        // Start from a water tile: blocked to ground, water-classed.
        let mut metadata = TileMetadata {
            has_tmp_metadata: true,
            land_type: LandType::Water.as_index(),
            yr_cell_land_type: LandType::Water.as_index(),
            terrain_class: TerrainClass::Water,
            is_water: true,
            ground_blocked: true,
            ..TileMetadata::default()
        };
        let canonical_ramp = canonical_ramp_from_slope_type(metadata.slope_type);
        let base_ground_walk_blocked = canonical_ramp.is_none() && metadata.ground_blocked;
        assert!(base_ground_walk_blocked);

        let overlay = OverlayEntry {
            rx: 0,
            ry: 0,
            overlay_id: 0,
            frame: 1,
        };
        let effects = classify_overlay_effects(Some(&vec![&overlay]), Some(&registry), 0, 0);
        let land = effects.effective_land.expect("bridge retains Land=Road");
        assert_eq!(land, LandType::Road);
        assert!(!effects.effective_land_ground_blocked);

        apply_canonical_land_to_metadata(
            &mut metadata,
            land,
            effects.effective_land_speed_costs,
            effects.effective_land_ground_blocked,
        );

        assert_eq!(metadata.land_type, LandType::Road.as_index());
        assert_eq!(metadata.yr_cell_land_type, LandType::Road.as_index());
        assert!(!metadata.is_water);
        // The regression: `ground_blocked` was the one land-derived field the
        // override skipped, so the deck stayed impassable while every other
        // attribute said Road.
        assert!(!metadata.ground_blocked);
        let ground_walk_blocked =
            (canonical_ramp.is_none() && metadata.ground_blocked) || effects.overlay_blocks;
        assert!(!ground_walk_blocked);
        // The pristine snapshot is untouched, so overlay removal still restores water.
        assert!(base_ground_walk_blocked);

        let theater = synthetic_automatic_shell_theater();
        let mut cell = make_test_cell(0, 0);
        cell.final_tile_index = 0;
        cell.yr_cell_land_type = metadata.yr_cell_land_type;
        let mut cells = vec![cell];
        let mut tubes = Vec::new();
        build_auto_low_bridge_tubes(&mut cells, 1, 1, Some(&theater), &mut tubes);
        assert!(
            tubes.is_empty() && cells[0].tube_index.is_none(),
            "the low Road early branch cannot reach automatic-shell construction"
        );
    }

    #[test]
    fn gsi_04_04_load_tiberium_slope_branches_retain_only_early_copied_land() {
        let ini = IniFile::from_str(
            "\
[OverlayTypes]
0=ORE
1=STOCKORE
2=WALLORE
3=RAILORE
[Tiberium]
Wheel=80%
[ORE]
Tiberium=yes
NoUseTileLandType=no
[STOCKORE]
Tiberium=yes
[WALLORE]
Tiberium=yes
Land=Wall
NoUseTileLandType=no
[RAILORE]
Tiberium=yes
Land=Railroad
NoUseTileLandType=no
",
        );
        let registry = OverlayTypeRegistry::from_ini(&ini, None);

        for (overlay_id, land, early_branch) in [
            (0, LandType::Tiberium, false),
            (1, LandType::Tiberium, true),
            (2, LandType::Wall, true),
            (3, LandType::Railroad, true),
        ] {
            let flags = registry.flags(overlay_id).expect("overlay flags");
            assert_eq!(uses_early_recalc_land_branch(flags), early_branch);
            for slope in [0, 1, 4, 5] {
                let overlay = OverlayEntry {
                    rx: 0,
                    ry: 0,
                    overlay_id,
                    frame: 7,
                };
                let entries = vec![&overlay];
                let effects = classify_overlay_effects(Some(&entries), Some(&registry), 0, slope);
                let clears = clears_tiberium_on_slope(flags, slope);
                assert_eq!(clears, slope != 0 && (early_branch || slope >= 5));
                let retains_current_land = !clears || early_branch;
                assert_eq!(
                    effects.effective_land,
                    retains_current_land.then_some(land),
                    "overlay={overlay_id} slope={slope}"
                );
                assert_eq!(effects.claims_cell_attributes, early_branch);

                let mut current = metadata_from_set_name(Some("Clear"), Some(0));
                if let Some(effective_land) = effects.effective_land {
                    apply_canonical_land_to_metadata(
                        &mut current,
                        effective_land,
                        effects.effective_land_speed_costs,
                        effects.effective_land_ground_blocked,
                    );
                }
                let expected_land = if retains_current_land {
                    land
                } else {
                    LandType::Clear
                };
                assert_eq!(current.land_type, expected_land.as_index());
                assert_eq!(current.yr_cell_land_type, expected_land.as_index());
                assert_eq!(current.terrain_class, expected_land.terrain_class());
                assert_eq!(current.is_water, expected_land.is_water());
                assert_eq!(current.is_cliff_like, expected_land.is_cliff_like());
                assert_eq!(current.is_rough, expected_land.is_rough());
                assert_eq!(current.is_road, expected_land.is_road());
                assert_eq!(
                    current.speed_costs,
                    if retains_current_land {
                        flags.land_speed_costs.unwrap_or_default()
                    } else {
                        SpeedCostProfile::default()
                    }
                );
                assert_eq!(
                    recalc_zone_type(
                        false,
                        effects.overlay_zone_type,
                        current.land_type,
                        current.speed_costs.wheel,
                        None,
                    ),
                    zone_class::GROUND
                );
                if clears {
                    assert_eq!(effects.overlay_zone_type, None);
                    assert!(!effects.overlay_blocks);
                }
            }
        }
    }

    #[test]
    fn gsi_04_04_rules_backed_land_type_uses_canonical_tmp_conversion() {
        let terrain_rules =
            TerrainRules::from_ini(&IniFile::from_str("[Rough]\nBuildable=yes\nTrack=75%\n"));
        let mut metadata = metadata_from_set_name(Some("Water"), Some(2));
        let tile = TmpTile {
            height: 0,
            terrain_type: 14,
            ramp_type: 0,
            radar_left: [0, 0, 0],
            radar_right: [0, 0, 0],
            pixels: Vec::new(),
            depth: Vec::new(),
            pixel_width: 60,
            pixel_height: 30,
            relative_extra_y: 0,
            offset_x: 0,
            offset_y: 0,
            has_damaged_data: false,
        };
        merge_tmp_metadata(&mut metadata, &tile);
        let mut warned = HashSet::new();
        apply_land_type_semantics(&mut metadata, Some(&terrain_rules), &mut warned);

        assert_eq!(metadata.terrain_class, TerrainClass::Rough);
        assert!(metadata.is_rough);
        assert!(!metadata.is_water);
        assert!(!metadata.ground_blocked);
        assert!(!metadata.build_blocked);
    }

    #[test]
    fn test_unknown_land_type_keeps_tileset_fallback() {
        // Use a LandType byte outside the 0-15 range (all 0-15 are now mapped).
        // Byte 200 is genuinely unknown and should fall back to tileset-name heuristics.
        let terrain_rules = TerrainRules::from_ini(&IniFile::from_str(""));
        let mut metadata = metadata_from_set_name(Some("Water Cliffs"), Some(5));
        let tile = TmpTile {
            height: 0,
            terrain_type: 200,
            ramp_type: 0,
            radar_left: [0, 0, 0],
            radar_right: [0, 0, 0],
            pixels: Vec::new(),
            depth: Vec::new(),
            pixel_width: 60,
            pixel_height: 30,
            relative_extra_y: 0,
            offset_x: 0,
            offset_y: 0,
            has_damaged_data: false,
        };
        merge_tmp_metadata(&mut metadata, &tile);
        let mut warned = HashSet::new();
        apply_land_type_semantics(&mut metadata, Some(&terrain_rules), &mut warned);

        assert_eq!(metadata.terrain_class, TerrainClass::Water);
        assert!(metadata.is_water);
        assert!(metadata.is_cliff_like);
        assert!(metadata.ground_blocked);
        assert_eq!(warned, HashSet::from([200]));
    }

    #[test]
    fn test_tileset_water_fallback_sets_water_land_type() {
        let metadata = metadata_from_set_name(Some("TEMPERATE WATER"), Some(5));
        assert!(metadata.is_water);
        assert_eq!(
            metadata.land_type,
            crate::rules::terrain_rules::LandType::Water.as_index()
        );
    }

    #[test]
    fn test_canonical_ramp_is_ground_passable_but_stays_non_buildable() {
        let map = make_map(
            vec![MapCell {
                rx: 0,
                ry: 0,
                tile_index: 0,
                sub_tile: 0,
                z: 0,
            }],
            Vec::new(),
            Vec::new(),
        );
        let terrain_rules = TerrainRules::from_ini(&IniFile::from_str("[Cliff]\nBuildable=no\n"));
        let mut metadata = TileMetadata {
            has_tmp_metadata: true,
            raw_land_type: 15,
            land_type: crate::rules::terrain_rules::tmp_terrain_to_land_type(15).as_index(),
            slope_type: 2,
            terrain_class: TerrainClass::Cliff,
            ground_blocked: true,
            build_blocked: true,
            is_cliff_like: true,
            has_ramp: true,
            ..TileMetadata::default()
        };
        let mut warned = HashSet::new();
        apply_land_type_semantics(&mut metadata, Some(&terrain_rules), &mut warned);

        let canonical_ramp = canonical_ramp_from_slope_type(metadata.slope_type);
        let base_ground_walk_blocked = canonical_ramp.is_none() && metadata.ground_blocked;
        assert!(!base_ground_walk_blocked);
        let grid = ResolvedTerrainGrid::from_cells(
            1,
            1,
            vec![ResolvedTerrainCell {
                rx: 0,
                ry: 0,
                source_tile_index: 0,
                source_sub_tile: 0,
                final_tile_index: 0,
                final_sub_tile: 0,
                is_wood_bridge_repair_tile: false,
                level: 0,
                filled_clear: false,
                tileset_index: Some(0),
                land_type: metadata.land_type,
                yr_cell_land_type: metadata.yr_cell_land_type,
                slope_type: metadata.slope_type,
                template_height: 0,
                render_offset_x: 0,
                render_offset_y: 0,
                terrain_class: metadata.terrain_class,
                speed_costs: metadata.speed_costs,
                is_water: false,
                is_cliff_like: true,
                is_rough: false,
                is_road: false,
                accepts_smudge: false,
                allows_tiberium: false,
                height_in_pixels: 0,
                variant: 0,
                has_ramp: true,
                canonical_ramp,
                ground_walk_blocked: false,
                terrain_object_blocks: false,
                terrain_object_occupation: None,
                overlay_blocks: false,
                overlay_zone_type: None,
                outside_playfield: false,
                zone_type: 0,
                base_ground_walk_blocked: false,
                base_build_blocked: true,
                base_land_type: 0,
                base_yr_cell_land_type: 0,
                base_terrain_class: Default::default(),
                base_speed_costs: Default::default(),
                build_blocked: true,
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
                bridgehead_anchor_class_at_load: None,
            }],
        );
        let cell = grid.cell(0, 0).expect("resolved ramp cell");
        assert_eq!(cell.canonical_ramp, Some(RampDirection::North));
        assert!(!cell.ground_walk_blocked);
        assert!(cell.build_blocked);
        assert_eq!(map.header.width, 4);
    }

    #[test]
    fn authored_load_lat_reads_live_cardinals_in_north_east_south_west_order() {
        let theater = synthetic_theater_from_ini(
            b"[General]\nRoughTile=0\nClearToRoughLat=1\n\
              [TileSet0000]\nTilesInSet=1\nFileName=base\nSetName=Rough\n\
              [TileSet0001]\nTilesInSet=16\nFileName=lat\nSetName=Rough LAT\n",
        );
        let lat_config = lat::parse_lat_config(&theater.ini_data, &theater.lookup);
        let registry = OverlayTypeRegistry::from_ini(&IniFile::from_str(""), None);
        let cardinals = [(1usize, 0usize), (2, 1), (1, 2), (0, 1)];

        for (bit, (nx, ny)) in cardinals.into_iter().enumerate() {
            let mut cells = (0..3)
                .flat_map(|ry| (0..3).map(move |rx| make_test_cell(rx, ry)))
                .collect::<Vec<_>>();
            cells[ny * 3 + nx].final_tile_index = 99;
            let mut grid = ResolvedTerrainGrid::from_cells(3, 3, cells);
            let mut state = LoadCellRecalcState::synthetic(&registry, grid.cells.len());
            state.lat_config = Some(lat_config.clone());
            state.slope_config = Some(lat::SlopeFixupConfig {
                ramp_base: -1,
                ramp_smooth: -1,
            });

            grid.apply_authored_load_lat_slope(&state, 4);

            assert_eq!(
                grid.cells[4].final_tile_index,
                1 + (1_i32 << bit),
                "cardinal slot {bit} must retain native N/E/S/W mask polarity"
            );
        }

        let mut edge = ResolvedTerrainGrid::from_cells(1, 1, vec![make_test_cell(0, 0)]);
        let mut state = LoadCellRecalcState::synthetic(&registry, 1);
        state.lat_config = Some(lat_config);
        state.slope_config = Some(lat::SlopeFixupConfig {
            ramp_base: -1,
            ramp_smooth: -1,
        });
        edge.apply_authored_load_lat_slope(&state, 0);
        assert_eq!(
            edge.cells[0].final_tile_index, 16,
            "all four missing neighbors must contribute through dummy tile 0xFFFF"
        );
        assert_eq!(
            edge.dummy_cell_requested_coord(),
            (-1, 0),
            "the final W lookup stamps the one shared dummy"
        );

        let mut guarded = ResolvedTerrainGrid::from_cells(1, 1, vec![make_test_cell(0, 0)]);
        guarded.cells[0].final_tile_index = 99;
        guarded.stamp_dummy_cell_requested_coord(7, 8);
        let mut state = LoadCellRecalcState::synthetic(&registry, 1);
        state.lat_config = Some(lat::parse_lat_config(&theater.ini_data, &theater.lookup));
        state.slope_config = Some(lat::SlopeFixupConfig {
            ramp_base: 100,
            ramp_smooth: 500,
        });
        guarded.apply_authored_load_lat_slope(&state, 0);
        assert_eq!(guarded.cells[0].final_tile_index, 99);
        assert_eq!(
            guarded.dummy_cell_requested_coord(),
            (7, 8),
            "failed LAT and ramp guards must issue no neighbor lookup"
        );

        let mut ramp = ResolvedTerrainGrid::from_cells(1, 1, vec![make_test_cell(0, 0)]);
        ramp.cells[0].final_tile_index = 100;
        ramp.cells[0].slope_type = 1;
        ramp.apply_authored_load_lat_slope(&state, 0);
        assert_eq!(ramp.cells[0].final_tile_index, 502);
        assert_eq!(
            ramp.dummy_cell_requested_coord(),
            (1, 0),
            "slope 1 must probe W then E and leave the E request stamped"
        );

        let mut sloped_neighbors =
            ResolvedTerrainGrid::from_cells(1, 1, vec![make_test_cell(0, 0)]);
        sloped_neighbors.cells[0].final_tile_index = 100;
        sloped_neighbors.cells[0].slope_type = 1;
        sloped_neighbors.test_set_dummy_cell_level_slope(0, 9);
        sloped_neighbors.apply_authored_load_lat_slope(&state, 0);
        assert_eq!(sloped_neighbors.cells[0].final_tile_index, 100);
        assert_eq!(sloped_neighbors.dummy_cell_requested_coord(), (1, 0));
    }

    #[test]
    fn authored_load_cliff_back_uses_exact_six_probes_and_signed_levels() {
        const OFFSETS: [(i16, i16); 6] = [(0, -1), (-1, 0), (2, 2), (1, 1), (-1, 1), (1, -1)];
        let center = (3i16, 3i16);
        let center_index = 3 * 7 + 3;

        for (dx, dy) in OFFSETS {
            let mut cells = (0..7)
                .flat_map(|ry| (0..7).map(move |rx| make_test_cell(rx, ry)))
                .collect::<Vec<_>>();
            let nx = usize::try_from(center.0 + dx).unwrap();
            let ny = usize::try_from(center.1 + dy).unwrap();
            cells[ny * 7 + nx].level = 4;
            let grid = ResolvedTerrainGrid::from_cells(7, 7, cells);
            assert!(
                grid.authored_load_cell_is_behind_cliff(center_index),
                "verified probe ({dx},{dy}) must reach the four-level boundary"
            );
        }

        let mut excluded = (0..7)
            .flat_map(|ry| (0..7).map(move |rx| make_test_cell(rx, ry)))
            .collect::<Vec<_>>();
        excluded[4 * 7 + 5].level = 4;
        let excluded = ResolvedTerrainGrid::from_cells(7, 7, excluded);
        assert!(
            !excluded.authored_load_cell_is_behind_cliff(center_index),
            "the adjacent but unprobed (+2,+1) cell must not affect CliffBack"
        );

        let mut signed = (0..7)
            .flat_map(|ry| (0..7).map(move |rx| make_test_cell(rx, ry)))
            .collect::<Vec<_>>();
        signed[center_index].level = u8::MAX;
        signed[2 * 7 + 3].level = 3;
        let signed = ResolvedTerrainGrid::from_cells(7, 7, signed);
        assert!(
            signed.authored_load_cell_is_behind_cliff(center_index),
            "signed -1 plus four reaches signed +3"
        );
    }

    #[test]
    fn authored_load_cliff_back_nonzero_modes_scan_but_only_two_writes() {
        let registry = OverlayTypeRegistry::from_ini(&IniFile::from_str(""), None);

        for mode in [0u8, 1, 3, u8::MAX] {
            let mut grid = ResolvedTerrainGrid::from_cells(1, 1, vec![make_test_cell(0, 0)]);
            grid.stamp_dummy_cell_requested_coord(7, 8);
            if mode != 0 {
                grid.test_set_dummy_cell_level_slope(4, 0);
            }
            let mut state = LoadCellRecalcState::synthetic(&registry, 1);
            state.cliff_back_impassability = mode;
            grid.recalc_authored_load_cell(
                &mut state,
                0,
                FinalizedOverlayCell::default(),
                &mut LoadRecalcTestEffects::default(),
            )
            .expect("mode scan Recalc");

            assert_eq!(grid.cells[0].land_type, LandType::Clear.as_index());
            assert_eq!(
                grid.dummy_cell_requested_coord(),
                if mode == 0 { (7, 8) } else { (0, -1) },
                "mode {mode} scan side effect"
            );
        }

        let mut grid = ResolvedTerrainGrid::from_cells(1, 1, vec![make_test_cell(0, 0)]);
        grid.test_set_dummy_cell_level_slope(4, 0);
        let mut state = LoadCellRecalcState::synthetic(&registry, 1);
        state.cliff_back_impassability = 2;
        grid.recalc_authored_load_cell(
            &mut state,
            0,
            FinalizedOverlayCell::default(),
            &mut LoadRecalcTestEffects::default(),
        )
        .expect("mode-2 writer Recalc");
        assert_eq!(grid.cells[0].land_type, LandType::Rock.as_index());
        assert_eq!(
            grid.dummy_cell_requested_coord(),
            (0, -1),
            "the first qualifying probe short-circuits the mode-2 scan"
        );
    }

    include!("terrain_recalc_native_tests.rs");

    #[test]
    fn authored_load_early_overlay_refreshes_tmp_slope_before_resource_clear() {
        let theater = synthetic_theater_from_ini(
            b"[TileSet0000]\nTilesInSet=1\nFileName=early\nSetName=Clear\n",
        );
        let mut tmp = gsi_04_02_last_tiles_tmp_bytes(0, [1, 2, 3], [4, 5, 6]);
        tmp[62] = 1;
        let (_directory, assets) = gsi_04_04_asset_manager_with_loose_tmp("early01.tem", &tmp);
        let terrain_rules = TerrainRules::from_ini(&IniFile::from_str(""));
        let registry = OverlayTypeRegistry::from_ini(
            &IniFile::from_str(
                "[OverlayTypes]\n0=EARLY_RESOURCE\n\
                 [EARLY_RESOURCE]\nTiberium=yes\nLand=Tiberium\nNoUseTileLandType=yes\n",
            ),
            None,
        );
        let map = make_map(Vec::new(), Vec::new(), Vec::new());
        let mut grid = ResolvedTerrainGrid::from_cells(1, 1, vec![make_test_cell(0, 0)]);
        assert_eq!(
            grid.cells[0].slope_type, 0,
            "fixture starts with stale flat cache"
        );
        let mut state = LoadCellRecalcState::for_authored_load(
            &map,
            &theater,
            &assets,
            &terrain_rules,
            &registry,
            false,
            0,
            1,
        );

        let outcome = grid
            .recalc_authored_load_cell(
                &mut state,
                0,
                FinalizedOverlayCell::from_parts(0, 7),
                &mut LoadRecalcTestEffects::default(),
            )
            .expect("early resource Recalc");

        assert_eq!(grid.cells[0].slope_type, 1);
        assert_eq!(outcome.finalized, FinalizedOverlayCell::default());
        assert_eq!(grid.cells[0].bridge_facts.overlay_id, None);
        assert_eq!(grid.cells[0].bridge_facts.state_byte, 0);
        assert_eq!(
            grid.cells[0].land_type,
            LandType::Tiberium.as_index(),
            "the early branch copies overlay Land before clearing its pair"
        );

        let mut unregistered = ResolvedTerrainGrid::from_cells(1, 1, vec![make_test_cell(0, 0)]);
        unregistered.cells[0].final_tile_index = 99;
        unregistered.cells[0].slope_type = 1;
        let outcome = unregistered
            .recalc_authored_load_cell(
                &mut state,
                0,
                FinalizedOverlayCell::from_parts(0, 9),
                &mut LoadRecalcTestEffects::default(),
            )
            .expect("unregistered early receiver Recalc");
        assert_eq!(unregistered.cells[0].slope_type, 1);
        assert_eq!(outcome.finalized, FinalizedOverlayCell::default());

        let sparse_theater = synthetic_theater_from_ini(
            b"[TileSet0000]\nTilesInSet=1\nFileName=absent\nSetName=Clear\n",
        );
        let mut sparse = ResolvedTerrainGrid::from_cells(1, 1, vec![make_test_cell(0, 0)]);
        sparse.cells[0].slope_type = 1;
        let mut sparse_state = LoadCellRecalcState::for_authored_load(
            &map,
            &sparse_theater,
            &assets,
            &terrain_rules,
            &registry,
            false,
            0,
            1,
        );
        let outcome = sparse
            .recalc_authored_load_cell(
                &mut sparse_state,
                0,
                FinalizedOverlayCell::from_parts(0, 11),
                &mut LoadRecalcTestEffects::default(),
            )
            .expect("registered missing-TMP early receiver Recalc");
        assert_eq!(sparse.cells[0].slope_type, 0);
        assert_eq!(outcome.finalized, FinalizedOverlayCell::from_parts(0, 11));
    }

    #[test]
    fn authored_load_recalc_keeps_three_cliff_back_writer_gates_distinct() {
        let overlay_ini = IniFile::from_str(
            "[OverlayTypes]\n0=EARLYROAD\n\
             [EARLYROAD]\nLand=Road\nNoUseTileLandType=yes\n",
        );
        let registry = OverlayTypeRegistry::from_ini(&overlay_ini, None);
        let make_grid = |land: LandType| {
            let mut cells = (0..3)
                .flat_map(|ry| (0..3).map(move |rx| make_test_cell(rx, ry)))
                .collect::<Vec<_>>();
            let center = &mut cells[4];
            center.land_type = land.as_index();
            center.yr_cell_land_type = land.as_index();
            center.terrain_class = land.terrain_class();
            center.is_road = land == LandType::Road;
            center.base_land_type = land.as_index();
            center.base_yr_cell_land_type = land.as_index();
            center.base_terrain_class = land.terrain_class();
            cells[1].level = 4;
            ResolvedTerrainGrid::from_cells(3, 3, cells)
        };

        let mut early = make_grid(LandType::Road);
        let mut early_state = LoadCellRecalcState::synthetic(&registry, early.cells.len());
        early_state.cliff_back_impassability = 2;
        early
            .recalc_authored_load_cell(
                &mut early_state,
                4,
                FinalizedOverlayCell::from_parts(0, 0),
                &mut LoadRecalcTestEffects::default(),
            )
            .expect("early overlay Recalc");
        assert_eq!(early.cells[4].land_type, LandType::Rock.as_index());
        assert_eq!(
            early.cells[4].base_land_type,
            LandType::Road.as_index(),
            "copy-1 reclasses the claimed current land without widening the base gate"
        );
        assert!(early.cells[4].ground_walk_blocked);
        assert!(early.cells[4].build_blocked);
        assert_eq!(early.cells[4].zone_type, zone_class::IMPASSABLE);
        assert!(!early.cells[4].base_build_blocked);

        let mut normal_clear = make_grid(LandType::Clear);
        let mut normal_clear_state =
            LoadCellRecalcState::synthetic(&registry, normal_clear.cells.len());
        normal_clear_state.cliff_back_impassability = 2;
        normal_clear
            .recalc_authored_load_cell(
                &mut normal_clear_state,
                4,
                FinalizedOverlayCell::default(),
                &mut LoadRecalcTestEffects::default(),
            )
            .expect("normal Clear Recalc");
        assert_eq!(normal_clear.cells[4].land_type, LandType::Rock.as_index());
        assert_eq!(
            normal_clear.cells[4].base_land_type,
            LandType::Rock.as_index()
        );

        let mut normal_road = make_grid(LandType::Road);
        let mut normal_road_state =
            LoadCellRecalcState::synthetic(&registry, normal_road.cells.len());
        normal_road_state.cliff_back_impassability = 2;
        normal_road
            .recalc_authored_load_cell(
                &mut normal_road_state,
                4,
                FinalizedOverlayCell::default(),
                &mut LoadRecalcTestEffects::default(),
            )
            .expect("normal Road Recalc");
        assert_eq!(normal_road.cells[4].land_type, LandType::Road.as_index());
        assert_eq!(
            normal_road.cells[4].base_land_type,
            LandType::Road.as_index()
        );

        let theater = synthetic_theater_from_ini(
            b"[TileSet0000]\nTilesInSet=1\nFileName=missing\nSetName=Clear\n",
        );
        let (_directory, assets) =
            gsi_04_04_asset_manager_with_loose_tmp("unrelated.tem", b"not a TMP");
        let terrain_rules = TerrainRules::from_ini(&IniFile::from_str(""));
        let map = make_map(Vec::new(), Vec::new(), Vec::new());
        let mut sparse = make_grid(LandType::Clear);
        sparse.cells[4].final_sub_tile = 7;
        let mut sparse_state = LoadCellRecalcState::for_authored_load(
            &map,
            &theater,
            &assets,
            &terrain_rules,
            &registry,
            false,
            2,
            sparse.cells.len(),
        );
        sparse
            .recalc_authored_load_cell(
                &mut sparse_state,
                4,
                FinalizedOverlayCell::default(),
                &mut LoadRecalcTestEffects::default(),
            )
            .expect("sparse-subtile Recalc");
        assert_eq!(sparse.cells[4].final_tile_index, 0xFFFF);
        assert_eq!(sparse.cells[4].final_sub_tile, 0);
        assert_eq!(sparse.cells[4].land_type, LandType::Rock.as_index());
        assert_eq!(sparse.cells[4].base_land_type, LandType::Rock.as_index());
    }

    #[test]
    fn gsi_04_04_normal_cliff_back_writer_accepts_clear_but_not_road() {
        let theater = synthetic_theater_from_ini(
            b"[TileSet0000]\nTilesInSet=1\nFileName=clear\nSetName=Clear\n\
              [TileSet0001]\nTilesInSet=1\nFileName=road\nSetName=Road\n",
        );
        let map = make_map(
            vec![
                MapCell {
                    rx: 1,
                    ry: 0,
                    tile_index: 0,
                    sub_tile: 0,
                    z: 4,
                },
                MapCell {
                    rx: 1,
                    ry: 1,
                    tile_index: 0,
                    sub_tile: 0,
                    z: 0,
                },
                MapCell {
                    rx: 3,
                    ry: 0,
                    tile_index: 0,
                    sub_tile: 0,
                    z: 4,
                },
                MapCell {
                    rx: 3,
                    ry: 1,
                    tile_index: 1,
                    sub_tile: 0,
                    z: 0,
                },
            ],
            Vec::new(),
            Vec::new(),
        );

        let grid = ResolvedTerrainGrid::build(&map, Some(&theater), None, None, None, false, 2);
        let clear = grid.cell(1, 1).expect("ordinary Clear cell");
        assert_eq!(clear.land_type, LandType::Rock.as_index());
        assert_eq!(clear.base_land_type, LandType::Rock.as_index());
        assert_eq!(clear.zone_type, zone_class::IMPASSABLE);

        let road = grid.cell(3, 1).expect("ordinary Road cell");
        assert_eq!(road.land_type, LandType::Road.as_index());
        assert_eq!(road.base_land_type, LandType::Road.as_index());
        assert_eq!(road.zone_type, zone_class::GROUND);
    }

    #[test]
    fn gsi_04_04_early_overlay_writer_does_not_bake_rock_into_road_base() {
        let theater = synthetic_theater_from_ini(
            b"[TileSet0000]\nTilesInSet=1\nFileName=road\nSetName=Road\n",
        );
        let overlay_ini = IniFile::from_str("[OverlayTypes]\n0=CLAIM\n[CLAIM]\nLand=Wall\n");
        let registry = OverlayTypeRegistry::from_ini(&overlay_ini, None);
        let map = make_map(
            vec![
                MapCell {
                    rx: 1,
                    ry: 0,
                    tile_index: 0,
                    sub_tile: 0,
                    z: 4,
                },
                MapCell {
                    rx: 1,
                    ry: 1,
                    tile_index: 0,
                    sub_tile: 0,
                    z: 0,
                },
            ],
            vec![OverlayEntry {
                rx: 1,
                ry: 1,
                overlay_id: 0,
                frame: 0,
            }],
            Vec::new(),
        );

        let mut grid =
            ResolvedTerrainGrid::build(&map, Some(&theater), None, None, Some(&registry), false, 2);
        let claimed = grid.cell(1, 1).expect("overlay-claimed Road cell");
        assert_eq!(claimed.land_type, LandType::Rock.as_index());
        assert_eq!(claimed.base_land_type, LandType::Road.as_index());
        assert_eq!(claimed.zone_type, zone_class::IMPASSABLE);

        let mut overlays = crate::sim::overlay_grid::OverlayGrid::from_overlay_entries(
            &map.overlays,
            grid.width,
            grid.height,
        );
        assert_eq!(overlays.clear_overlay(1, 1), Some(0));
        crate::sim::overlay_grid::recalc_overlay_passability(
            &mut overlays,
            &mut grid,
            &registry,
            1,
            1,
        );
        let restored = grid.cell(1, 1).expect("restored Road cell");
        assert_eq!(restored.land_type, LandType::Road.as_index());
        assert_eq!(restored.base_land_type, LandType::Road.as_index());
        assert_eq!(restored.zone_type, zone_class::GROUND);
    }

    #[test]
    fn gsi_04_04_cliff_back_neighbor_lookup_keeps_fixed_stride_alias() {
        let map = make_map(
            vec![
                MapCell {
                    rx: 0,
                    ry: 0,
                    tile_index: -1,
                    sub_tile: 0,
                    z: 0,
                },
                MapCell {
                    rx: 511,
                    ry: 0,
                    tile_index: -1,
                    sub_tile: 0,
                    z: 4,
                },
            ],
            Vec::new(),
            Vec::new(),
        );

        let grid = ResolvedTerrainGrid::build(&map, None, None, None, None, false, 2);
        assert_eq!(
            grid.cell(0, 0).expect("alias source").land_type,
            LandType::Rock.as_index(),
            "packed (-1,1) aliases canonical fixed-stride slot (511,0)"
        );
    }

    #[test]
    fn gsi_04_04_invalid_clear_cliff_back_writer_bakes_base() {
        // Cell (1,1) at level 0, cell (1,0) at level 4.
        // Neighbor offset (0,-1) means (1,0) is checked from (1,1).
        // Height diff = 4 >= 4 → cell (1,1) should be marked impassable.
        let map = make_map(
            vec![
                MapCell {
                    rx: 0,
                    ry: 0,
                    tile_index: -1,
                    sub_tile: 0,
                    z: 0,
                },
                MapCell {
                    rx: 1,
                    ry: 0,
                    tile_index: -1,
                    sub_tile: 0,
                    z: 4,
                },
                MapCell {
                    rx: 0,
                    ry: 1,
                    tile_index: -1,
                    sub_tile: 0,
                    z: 0,
                },
                MapCell {
                    rx: 1,
                    ry: 1,
                    tile_index: -1,
                    sub_tile: 0,
                    z: 0,
                },
            ],
            Vec::new(),
            Vec::new(),
        );
        let grid = ResolvedTerrainGrid::build(&map, None, None, None, None, false, 2);
        let cell = grid.cell(1, 1).unwrap();
        assert!(
            cell.ground_walk_blocked,
            "Cell at base of cliff should be blocked"
        );
        assert!(
            cell.is_cliff_like,
            "Cell at base of cliff should be cliff-like"
        );
        assert_eq!(
            cell.land_type,
            crate::rules::terrain_rules::LandType::Rock.as_index(),
            "Cell at base of cliff should have Rock land type"
        );
        // The reclass precedes zone derivation: the reduced zone becomes
        // Impassable, and the base (pre-overlay) snapshot carries the reclass
        // so an overlay add/remove cycle cannot restore the original clear
        // terrain (levels never change at runtime).
        assert_eq!(cell.zone_type, zone_class::IMPASSABLE);
        assert_eq!(
            cell.base_land_type,
            crate::rules::terrain_rules::LandType::Rock.as_index()
        );
        assert!(cell.base_ground_walk_blocked);
        assert!(cell.base_build_blocked);
        assert_eq!(
            cell.yr_cell_land_type,
            crate::rules::terrain_rules::LandType::Rock.as_index()
        );
    }

    #[test]
    fn gsi_04_04_sentinel_tile_uses_final_cliff_back_copy_for_overlay_water() {
        let overlay_ini =
            IniFile::from_str("[OverlayTypes]\n0=WATEROVERLAY\n[WATEROVERLAY]\nLand=Water\n");
        let registry = OverlayTypeRegistry::from_ini(&overlay_ini, None);
        let map = make_map(
            vec![
                MapCell {
                    rx: 1,
                    ry: 0,
                    tile_index: theater::NO_TILE,
                    sub_tile: 0,
                    z: 4,
                },
                MapCell {
                    rx: 1,
                    ry: 1,
                    tile_index: theater::NO_TILE,
                    sub_tile: 7,
                    z: 0,
                },
            ],
            vec![OverlayEntry {
                rx: 1,
                ry: 1,
                overlay_id: 0,
                frame: 0,
            }],
            Vec::new(),
        );

        let without_cliff_back =
            ResolvedTerrainGrid::build(&map, None, None, None, Some(&registry), false, 0);
        let water = without_cliff_back
            .cell(1, 1)
            .expect("sentinel overlay-water cell");
        assert_eq!(water.final_tile_index, theater::NO_TILE);
        assert_eq!(water.final_sub_tile, 7);
        assert_eq!(water.land_type, LandType::Water.as_index());

        let with_cliff_back =
            ResolvedTerrainGrid::build(&map, None, None, None, Some(&registry), false, 2);
        let rock = with_cliff_back
            .cell(1, 1)
            .expect("final-copy CliffBack cell");
        assert_eq!(rock.final_tile_index, theater::NO_TILE);
        assert_eq!(rock.final_sub_tile, 7);
        assert_eq!(rock.land_type, LandType::Rock.as_index());
        assert_eq!(rock.base_land_type, LandType::Rock.as_index());
        assert_eq!(rock.zone_type, zone_class::IMPASSABLE);
    }

    #[test]
    fn gsi_04_03b_cliff_back_compares_levels_as_signed_bytes() {
        let map = make_map(
            vec![
                MapCell {
                    rx: 0,
                    ry: 0,
                    tile_index: -1,
                    sub_tile: 0,
                    z: 0,
                },
                MapCell {
                    rx: 1,
                    ry: 0,
                    tile_index: -1,
                    sub_tile: 0,
                    z: 3,
                },
                MapCell {
                    rx: 0,
                    ry: 1,
                    tile_index: -1,
                    sub_tile: 0,
                    z: 0,
                },
                MapCell {
                    rx: 1,
                    ry: 1,
                    tile_index: -1,
                    sub_tile: 0,
                    z: 0xff,
                },
            ],
            Vec::new(),
            Vec::new(),
        );
        let grid = ResolvedTerrainGrid::build(&map, None, None, None, None, false, 2);
        let cell = grid.cell(1, 1).expect("signed -1 cell");
        assert!(
            cell.ground_walk_blocked,
            "signed -1 to +3 is the exact four-level CliffBack boundary"
        );
    }

    #[test]
    fn cliff_back_impassability_skips_when_disabled() {
        let map = make_map(
            vec![
                MapCell {
                    rx: 0,
                    ry: 0,
                    tile_index: -1,
                    sub_tile: 0,
                    z: 0,
                },
                MapCell {
                    rx: 1,
                    ry: 0,
                    tile_index: -1,
                    sub_tile: 0,
                    z: 4,
                },
                MapCell {
                    rx: 0,
                    ry: 1,
                    tile_index: -1,
                    sub_tile: 0,
                    z: 0,
                },
                MapCell {
                    rx: 1,
                    ry: 1,
                    tile_index: -1,
                    sub_tile: 0,
                    z: 0,
                },
            ],
            Vec::new(),
            Vec::new(),
        );
        // cliff_back_impassability = 0 → disabled
        let grid = ResolvedTerrainGrid::build(&map, None, None, None, None, false, 0);
        let cell = grid.cell(1, 1).unwrap();
        assert!(
            !cell.ground_walk_blocked,
            "Should NOT be blocked when disabled"
        );
    }

    #[test]
    fn cliff_back_impassability_ignores_small_height_diff() {
        let map = make_map(
            vec![
                MapCell {
                    rx: 0,
                    ry: 0,
                    tile_index: -1,
                    sub_tile: 0,
                    z: 0,
                },
                MapCell {
                    rx: 1,
                    ry: 0,
                    tile_index: -1,
                    sub_tile: 0,
                    z: 3,
                },
                MapCell {
                    rx: 0,
                    ry: 1,
                    tile_index: -1,
                    sub_tile: 0,
                    z: 0,
                },
                MapCell {
                    rx: 1,
                    ry: 1,
                    tile_index: -1,
                    sub_tile: 0,
                    z: 0,
                },
            ],
            Vec::new(),
            Vec::new(),
        );
        let grid = ResolvedTerrainGrid::build(&map, None, None, None, None, false, 2);
        let cell = grid.cell(1, 1).unwrap();
        assert!(
            !cell.ground_walk_blocked,
            "Height diff 3 should NOT trigger (threshold is 4)"
        );
    }

    /// `TheaterData` with a single tileset that declares tile animations in its
    /// `SetName`-named section, shaped like retail `[Waterfalls]` /
    /// `[Tunnel Floor]` blocks.
    fn theater_with_tile_anims() -> TheaterData {
        let ini = b"[TileSet0000]
TilesInSet=4
FileName=wf
SetName=Waterfalls

                    [Waterfalls]
                    Tile01Anim=WA01X
Tile01XOffset=-30
Tile01YOffset=59
                    Tile01AttachesTo=0
Tile01ZAdjust=0
                    Tile03Anim=TUNTOP01
Tile03XOffset=-48
Tile03YOffset=-37
                    Tile03AttachesTo=2
Tile03ZAdjust=-10
";
        let lookup = theater::parse_tileset_ini(ini, "tem").expect("synthetic theater parses");
        let empty_palette = crate::assets::pal_file::Palette::from_bytes(&[0u8; 768])
            .expect("768-byte zero palette parses");
        TheaterData {
            lookup,
            iso_palette: empty_palette.clone(),
            unit_palette: empty_palette.clone(),
            tiberium_palette: empty_palette,
            extension: "tem",
            ini_data: Vec::new(),
            bridge_set: None,
            wood_bridge_set: None,
            slope_set_pieces: None,
            slope_set_pieces2: None,
            bridge_top_left_1: None,
            bridge_top_left_2: None,
            bridge_bottom_right_1: None,
            bridge_bottom_right_2: None,
            bridge_top_right_1: None,
            bridge_top_right_2: None,
            bridge_bottom_left_1: None,
            bridge_bottom_left_2: None,
            bridge_middle_1: None,
            bridge_middle_2: None,
            tunnels: None,
            track_tunnels: None,
            dirt_tunnels: None,
            dirt_track_tunnels: None,
            automatic_tube_bases: [-1; 4],
            cliff_ranges: crate::map::theater::TheaterCliffRanges::default(),
            rmg_tiles: crate::map::theater::RmgTileKeys::default(),
        }
    }

    fn anim_cell(rx: u16, ry: u16, tile_index: i32, sub_tile: u8, z: u8) -> MapCell {
        MapCell {
            rx,
            ry,
            tile_index,
            sub_tile,
            z,
        }
    }

    #[test]
    fn authored_load_recalc_latches_tile_anim_only_after_synchronous_success() {
        let theater = theater_with_tile_anims();
        let tmp = gsi_04_02_last_tiles_tmp_bytes(0, [1, 2, 3], [4, 5, 6]);
        let (_directory, assets) = gsi_04_04_asset_manager_with_loose_tmp("wf01.tem", &tmp);
        let terrain_rules = TerrainRules::from_ini(&IniFile::from_str(""));
        let registry = OverlayTypeRegistry::from_ini(&IniFile::from_str(""), None);
        let map = make_map(Vec::new(), Vec::new(), Vec::new());
        let mut grid =
            ResolvedTerrainGrid::from_cells(2, 1, vec![make_test_cell(0, 0), make_test_cell(1, 0)]);
        let mut state = LoadCellRecalcState::for_authored_load(
            &map,
            &theater,
            &assets,
            &terrain_rules,
            &registry,
            false,
            0,
            grid.cells.len(),
        );
        let mut effects = LoadRecalcTestEffects {
            fail: true,
            ..LoadRecalcTestEffects::default()
        };

        let failed = grid.recalc_authored_load_cell(
            &mut state,
            0,
            FinalizedOverlayCell::default(),
            &mut effects,
        );
        assert!(matches!(
            failed,
            Err(LoadCellRecalcError::Effect("fixture failure"))
        ));
        assert_eq!(effects.requests.len(), 1);
        assert!(!state.terrain_anim_latched[0]);

        effects.fail = false;
        grid.recalc_authored_load_cell(
            &mut state,
            0,
            FinalizedOverlayCell::default(),
            &mut effects,
        )
        .expect("successful synchronous terrain-Anim construction");
        assert_eq!(effects.requests.len(), 2);
        assert!(state.terrain_anim_latched[0]);

        grid.recalc_authored_load_cell(
            &mut state,
            0,
            FinalizedOverlayCell::default(),
            &mut effects,
        )
        .expect("latched Recalc");
        assert_eq!(
            effects.requests.len(),
            2,
            "the post-data Recalc must not duplicate an inline successful Anim"
        );

        grid.recalc_authored_load_cell(
            &mut state,
            1,
            FinalizedOverlayCell::default(),
            &mut effects,
        )
        .expect("second cell terrain-Anim construction");
        assert_eq!(effects.requests.len(), 3);
        assert!(state.terrain_anim_latched[1]);

        assert!(state.clear_terrain_anim_latch(0));
        assert!(
            state.terrain_anim_latched[1],
            "clearing one final-sweep receiver must not expose a future cell"
        );
        grid.recalc_authored_load_cell(
            &mut state,
            0,
            FinalizedOverlayCell::default(),
            &mut effects,
        )
        .expect("post-object unlatched Recalc");
        assert_eq!(effects.requests.len(), 4);
        assert!(state.terrain_anim_latched[0]);

        assert!(state.clear_terrain_anim_latch(1));
        grid.recalc_authored_load_cell(
            &mut state,
            1,
            FinalizedOverlayCell::default(),
            &mut effects,
        )
        .expect("second post-object receiver Recalc");
        assert_eq!(effects.requests.len(), 5);
        assert!(state.terrain_anim_latched[1]);
    }

    #[test]
    fn gsi_13_04_tile_anim_offset_matches_native_float_transform() {
        // The engine converts the pixel offset through a float 3x4 matrix whose
        // first two rows are [+s, 2s, 0, 0] and [-s, 2s, 0, 0], with s the
        // single-precision constant 4.2667, then truncates toward zero. This
        // pins the integer form against that reference across a range that
        // comfortably contains every stock offset (max magnitude 60).
        const S: f32 = 4.2667;
        const S2: f32 = 8.5334;
        for px in -128i32..=128 {
            for py in -128i32..=128 {
                let reference_x = (S * px as f32 + S2 * py as f32).trunc() as i32;
                let reference_y = (S2 * py as f32 - S * px as f32).trunc() as i32;
                assert_eq!(
                    tile_anim_pixel_offset_to_leptons(px, py),
                    (reference_x, reference_y),
                    "pixel offset ({px}, {py})"
                );
            }
        }
    }

    #[test]
    fn gsi_13_04_tile_anim_spawns_only_on_the_attaches_to_subtile() {
        let theater = theater_with_tile_anims();
        // Tile 0 declares AttachesTo=0; tile 2 declares AttachesTo=2.
        let map = make_map(
            vec![
                anim_cell(0, 0, 0, 0, 0),
                anim_cell(1, 0, 0, 1, 0),
                anim_cell(0, 1, 2, 2, 0),
                anim_cell(1, 1, 2, 0, 0),
            ],
            Vec::new(),
            Vec::new(),
        );
        let grid = ResolvedTerrainGrid::build(&map, Some(&theater), None, None, None, false, 0);
        let anims = grid.tile_animations();
        assert_eq!(anims.len(), 2, "{anims:?}");
        assert_eq!(anims[0].rx, 0);
        assert_eq!(anims[0].ry, 0);
        assert_eq!(anims[0].anim_name, "WA01X");
        assert_eq!(anims[1].rx, 0);
        assert_eq!(anims[1].ry, 1);
        assert_eq!(anims[1].anim_name, "TUNTOP01");
        assert_eq!(anims[1].z_adjust, -10);
    }

    #[test]
    fn gsi_13_04_tile_anim_world_coord_adds_cell_centre_and_ground_height() {
        let theater = theater_with_tile_anims();
        let map = make_map(vec![anim_cell(2, 3, 0, 0, 5)], Vec::new(), Vec::new());
        let grid = ResolvedTerrainGrid::build(&map, Some(&theater), None, None, None, false, 0);
        let anim = &grid.tile_animations()[0];
        let (offset_x, offset_y) = tile_anim_pixel_offset_to_leptons(-30, 59);
        assert_eq!(anim.world_x, offset_x + 2 * 256 + 128);
        assert_eq!(anim.world_y, offset_y + 3 * 256 + 128);
        assert_eq!(
            anim.world_z,
            5 * crate::util::lepton::GROUND_LEVEL_HEIGHT_LEPTONS
        );
    }

    #[test]
    fn gsi_13_04_tile_anim_skipped_for_no_tile_cells() {
        // A no-tile cell takes the engine's early return before the animation
        // spawn, so ClearTile presentation never inherits an attachment.
        let theater = theater_with_tile_anims();
        let map = make_map(vec![anim_cell(0, 0, -1, 0, 0)], Vec::new(), Vec::new());
        let grid = ResolvedTerrainGrid::build(&map, Some(&theater), None, None, None, false, 0);
        assert!(grid.tile_animations().is_empty());
    }

    #[test]
    fn gsi_13_04_tile_anims_are_ordered_by_the_native_anti_diagonal_sweep() {
        let theater = theater_with_tile_anims();
        let map = make_map(
            vec![
                anim_cell(2, 0, 0, 0, 0),
                anim_cell(0, 1, 0, 0, 0),
                anim_cell(1, 0, 0, 0, 0),
                anim_cell(0, 0, 0, 0, 0),
            ],
            Vec::new(),
            Vec::new(),
        );
        let grid = ResolvedTerrainGrid::build(&map, Some(&theater), None, None, None, false, 0);
        let order: Vec<(u16, u16)> = grid
            .tile_animations()
            .iter()
            .map(|anim| (anim.rx, anim.ry))
            .collect();
        assert_eq!(order, vec![(0, 0), (0, 1), (1, 0), (2, 0)]);
    }

    #[test]
    fn gsi_04_04_normal_cliff_back_reclass_filter_includes_ice() {
        use crate::rules::terrain_rules::LandType;
        for land in [
            LandType::Clear,
            LandType::Water,
            LandType::Beach,
            LandType::Ice,
        ] {
            assert!(
                cliff_back_normal_reclass_applies(land.as_index()),
                "{land:?} must be reclassed to Rock behind a cliff"
            );
        }
        for land in [
            LandType::Road,
            LandType::Rock,
            LandType::Wall,
            LandType::Tiberium,
            LandType::Railroad,
            LandType::Rough,
            LandType::Tunnel,
            LandType::Weeds,
        ] {
            assert!(
                !cliff_back_normal_reclass_applies(land.as_index()),
                "{land:?} must keep its own land type behind a cliff"
            );
        }
    }

    #[test]
    fn destroyable_cliff_a_collapses_old_sparse_footprint_then_stamps_two_halves() {
        let mut cells = Vec::new();
        for ry in 0..4 {
            for rx in 0..6 {
                let mut cell = make_test_cell(rx, ry);
                let sub_tile = (ry * 6 + rx) as u8;
                if ![0, 5, 18, 23].contains(&usize::from(sub_tile)) {
                    cell.final_tile_index = 100;
                    cell.final_sub_tile = sub_tile;
                }
                cell.level = 1;
                cells.push(cell);
            }
        }
        let mut grid = ResolvedTerrainGrid::from_cells(6, 4, cells);
        grid.test_install_destroyable_cliff_catalog(100);

        let mutation = grid
            .collapse_destroyable_cliff_terrain(4, 1, |_, _| {})
            .expect("family-A destroyable cliff");

        assert_eq!(mutation.family, DestroyableCliffFamily::A);
        assert_eq!(mutation.origin, (0, 0));
        assert_eq!(mutation.original_footprint.len(), 20);
        assert_eq!(
            mutation.animation_cells,
            (0..3)
                .flat_map(|ry| (0..5).map(move |rx| (rx, ry)))
                .collect::<Vec<_>>(),
        );
        assert_eq!(mutation.changed_cells.len(), 20);
        for ry in 0..4 {
            for rx in 0..6 {
                let cell = grid.cell(rx, ry).unwrap();
                if [(0, 0), (5, 0), (0, 3), (5, 3)].contains(&(rx, ry)) {
                    assert_eq!(cell.final_tile_index, 0, "sparse holes stay untouched");
                    continue;
                }
                let (tile, sub_tile, slope) = if rx < 3 {
                    (200, ry * 3 + rx, 1)
                } else {
                    (201, ry * 3 + (rx - 3), 2)
                };
                assert_eq!(cell.final_tile_index, tile);
                assert_eq!(u16::from(cell.final_sub_tile), sub_tile);
                assert_eq!(cell.slope_type, slope);
                assert_eq!(cell.level, 1, "old height subtracts before replacement add");
                assert!(!cell.has_bridge_deck);
                assert_eq!(cell.bridge_facts, BridgeCellFacts::default());
            }
        }
    }

    #[test]
    fn destroyable_cliff_origin_recovery_accepts_every_present_a_and_b_subtile() {
        for (family, tile, width, height, holes) in [
            (
                DestroyableCliffFamily::A,
                100,
                6u16,
                4u16,
                &[0usize, 5, 18, 23][..],
            ),
            (
                DestroyableCliffFamily::B,
                101,
                4u16,
                6u16,
                &[0usize, 3, 20, 23][..],
            ),
        ] {
            for selected_subtile in 0..usize::from(width * height) {
                if holes.contains(&selected_subtile) {
                    continue;
                }
                let mut cells = Vec::new();
                for ry in 0..height {
                    for rx in 0..width {
                        let mut cell = make_test_cell(rx, ry);
                        let sub_tile = usize::from(ry * width + rx);
                        if !holes.contains(&sub_tile) {
                            cell.final_tile_index = tile;
                            cell.final_sub_tile = sub_tile as u8;
                        }
                        cell.level = 1;
                        cells.push(cell);
                    }
                }
                let mut grid = ResolvedTerrainGrid::from_cells(width, height, cells);
                grid.test_install_destroyable_cliff_catalog(100);
                let selected = (
                    (selected_subtile % usize::from(width)) as u16,
                    (selected_subtile / usize::from(width)) as u16,
                );

                let mutation = grid
                    .collapse_destroyable_cliff_terrain(selected.0, selected.1, |_, _| {})
                    .expect("present sparse subtile selects its family");

                assert_eq!(mutation.family, family);
                assert_eq!(mutation.origin, (0, 0));
                assert_eq!(mutation.original_footprint.len(), 20);
                assert_eq!(mutation.changed_cells.len(), 20);
                for ry in 0..height {
                    for rx in 0..width {
                        let index = usize::from(ry * width + rx);
                        let cell = grid.cell(rx, ry).unwrap();
                        if holes.contains(&index) {
                            assert_eq!(cell.final_tile_index, 0, "sparse hole {index}");
                            continue;
                        }
                        let (expected_tile, expected_subtile, expected_slope) = match family {
                            DestroyableCliffFamily::A if rx < 3 => {
                                (200, usize::from(ry * 3 + rx), 1)
                            }
                            DestroyableCliffFamily::A => (201, usize::from(ry * 3 + (rx - 3)), 2),
                            DestroyableCliffFamily::B if ry < 3 => {
                                (203, usize::from(ry * 4 + rx), 4)
                            }
                            DestroyableCliffFamily::B => (202, usize::from((ry - 3) * 4 + rx), 3),
                        };
                        assert_eq!(cell.final_tile_index, expected_tile);
                        assert_eq!(usize::from(cell.final_sub_tile), expected_subtile);
                        assert_eq!(cell.slope_type, expected_slope);
                        assert_eq!(cell.level, 1);
                    }
                }
            }
        }
    }
}

#[cfg(test)]
impl ResolvedTerrainCell {
    /// A clear, level, walkable cell for focused terrain fixtures. Mirrors the
    /// template the terrain-object tests build by hand; `zone_type` and the
    /// overlay fields are the knobs a test turns.
    pub(crate) fn clear_for_test(rx: u16, ry: u16) -> Self {
        let mut speed_costs = SpeedCostProfile::default();
        speed_costs.wheel = Some(100);
        Self {
            rx,
            ry,
            source_tile_index: 0,
            source_sub_tile: 0,
            final_tile_index: 0,
            final_sub_tile: 0,
            is_wood_bridge_repair_tile: false,
            level: 0,
            filled_clear: false,
            tileset_index: None,
            land_type: LandType::Clear.as_index(),
            yr_cell_land_type: LandType::Clear.as_index(),
            slope_type: 0,
            template_height: 0,
            render_offset_x: 0,
            render_offset_y: 0,
            terrain_class: TerrainClass::Clear,
            speed_costs,
            is_water: false,
            is_cliff_like: false,
            is_rough: false,
            is_road: false,
            accepts_smudge: false,
            allows_tiberium: false,
            height_in_pixels: 0,
            variant: 0,
            has_ramp: false,
            canonical_ramp: None,
            ground_walk_blocked: false,
            terrain_object_blocks: false,
            terrain_object_occupation: None,
            overlay_blocks: false,
            overlay_zone_type: None,
            outside_playfield: false,
            zone_type: zone_class::GROUND,
            base_ground_walk_blocked: false,
            base_build_blocked: false,
            base_land_type: LandType::Clear.as_index(),
            base_yr_cell_land_type: LandType::Clear.as_index(),
            base_terrain_class: TerrainClass::Clear,
            base_speed_costs: speed_costs,
            build_blocked: false,
            has_bridge_deck: false,
            bridge_walkable: false,
            bridge_transition: false,
            bridge_deck_level: 0,
            bridge_layer: None,
            bridge_facts: crate::map::bridge_facts::BridgeCellFacts::default(),
            tube_index: None,
            radar_left: [0; 3],
            radar_right: [0; 3],
            has_damaged_data: false,
            bridgehead_anchor_class_at_load: None,
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
