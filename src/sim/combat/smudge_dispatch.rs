//! Smudge spawn dispatcher — fired from combat tick at explosion emission and
//! at building destruction. Mirrors AnimClass::Start, BuildingClass::DestructionEffects,
//! and BuildingClass::SpawnSurvivors smudge logic from gamemd.exe.
//!
//! Dependency rules: depends on rules/, map/, sim/. Never render/ui/audio/net.

use crate::sim::combat::inviso_scatter::{coord_to_cell_truncating, random_direction_coord};
use crate::sim::rng::SimRng;
use crate::sim::smudge_grid::SimCoord;

use crate::map::resolved_terrain::ResolvedTerrainGrid;
use crate::rules::art_data::ArtRegistry;
use crate::rules::locomotor_type::SpeedType;
use crate::rules::smudge_type::SmudgeTypeRegistry;
use crate::sim::combat::SmudgeSpawnRequest;
use crate::sim::intern::StringInterner;
use crate::sim::occupancy::{OccupancyGrid, RawCellOccupationGrid};
use crate::sim::ore_growth::OreGrowthState;
use crate::sim::overlay_grid::OverlayGrid;
use crate::sim::smudge_grid::{SmudgeGrid, SmudgeKind};
use crate::sim::tiberium::{ReduceTiberiumContext, reduce_tiberium};

/// Strict altitude gate from ledger #3: smudges only spawn when the anim
/// is within 30 leptons of the ground.
const SMUDGE_ALTITUDE_GATE_LEPTONS: i32 = 30;

/// Hardcoded ore-reduction amount on `AnimClass::Middle @ 0x00424F00`'s
/// crater branch (the particle/scorch/crater body; it plays no sound).
const CRATER_ORE_REDUCTION: u16 = 6;

/// Damage values passed to `SmudgeGrid::try_place` for building destruction
/// and survivor smudges (matches the Damage/Damage2 magnitudes seen in the
/// destruction effect path).
const BUILDING_SMUDGE_DMG: i32 = 100;

/// Lepton offset magnitude for survivor-smudge scatter — mirrors gamemd's
/// `SpawnSurvivors` call to `FUN_0049F420(magnitude=0x80, flag=0)`.
const SURVIVOR_OFFSET_MAGNITUDE: i32 = 0x80;

/// Mutable ore/tiberium state touched by crater smudge dispatch.
pub struct SmudgeTiberiumContext<'a> {
    pub overlay_grid: &'a mut OverlayGrid,
    pub ore_growth_state: &'a mut OreGrowthState,
    pub overlay_registry: Option<&'a crate::map::overlay_types::OverlayTypeRegistry>,
    pub tiberium_types: Option<&'a crate::rules::tiberium_type::TiberiumTypeRegistry>,
    pub source_object_cells: Option<&'a std::collections::BTreeSet<(u16, u16)>>,
    /// Live ground object lists for the crater reduction's neighbour reseed.
    pub live_objects: Option<crate::sim::tiberium::NativeCellObjectView<'a>>,
    pub binary_frame: u32,
    pub spread_enabled: bool,
    pub radar_dirty_cells: &'a mut Vec<(u16, u16)>,
    pub radar_dirty_generation: &'a mut u64,
    pub tactical_dirty_cells: &'a mut Vec<(u16, u16)>,
}

impl SmudgeTiberiumContext<'_> {
    fn overlay_grid(&self) -> &OverlayGrid {
        self.overlay_grid
    }

    fn reduce(
        &mut self,
        cell: (u16, u16),
        amount: u16,
        terrain: &mut ResolvedTerrainGrid,
        rng: &mut SimRng,
    ) {
        let mut ctx = ReduceTiberiumContext {
            overlay_grid: Some(&mut *self.overlay_grid),
            ore_growth_state: &mut *self.ore_growth_state,
            overlay_registry: self.overlay_registry,
            tiberium_types: self.tiberium_types,
            resolved_terrain: Some(terrain),
            source_object_cells: self.source_object_cells,
            live_objects: self.live_objects,
            rng: Some(rng),
            binary_frame: self.binary_frame,
            spread_enabled: self.spread_enabled,
            radar_dirty_cells: Some(&mut *self.radar_dirty_cells),
            radar_dirty_generation: Some(&mut *self.radar_dirty_generation),
            tactical_dirty_cells: Some(&mut *self.tactical_dirty_cells),
        };
        reduce_tiberium(&mut ctx, cell, i32::from(amount));
    }
}

/// Try to dispatch a smudge for an animation that just spawned at `coord`.
///
/// Reads scorch/crater/force_big_craters bools from the AnimType's ArtEntry.
/// Performs the altitude gate, the 50/50 random pick when both flags are set,
/// the `reduce_tiberium(6)` side effect for crater path, and finally calls
/// `SmudgeGrid::try_place`.
#[allow(clippy::too_many_arguments)]
pub fn try_dispatch_anim_smudge(
    art: &ArtRegistry,
    smudge_types: &SmudgeTypeRegistry,
    anim_name: &str,
    coord: SimCoord,
    ground_z: i32,
    smudge_grid: &mut SmudgeGrid,
    occupancy: &OccupancyGrid,
    terrain: &mut ResolvedTerrainGrid,
    tiberium: &mut SmudgeTiberiumContext<'_>,
    rng: &mut SimRng,
) {
    let Some(entry) = art.get(anim_name) else {
        return;
    };

    if (coord.z - ground_z) >= SMUDGE_ALTITUDE_GATE_LEPTONS {
        return;
    }

    let dmg: i32 = entry.frame_width as i32;
    let dmg2: i32 = entry.frame_height as i32;

    if entry.scorch {
        if !entry.crater {
            smudge_grid.try_place(
                SmudgeKind::Burn,
                coord,
                dmg,
                dmg2,
                false,
                smudge_types,
                terrain,
                tiberium.overlay_grid(),
                occupancy,
                rng,
            );
            return;
        }
        if rng_below_half_normalized(rng) {
            smudge_grid.try_place(
                SmudgeKind::Burn,
                coord,
                dmg,
                dmg2,
                false,
                smudge_types,
                terrain,
                tiberium.overlay_grid(),
                occupancy,
                rng,
            );
            return;
        }
    }
    if entry.crater {
        let rx = (coord.x >> 8).clamp(0, smudge_grid.width() as i32 - 1) as u16;
        let ry = (coord.y >> 8).clamp(0, smudge_grid.height() as i32 - 1) as u16;
        tiberium.reduce((rx, ry), CRATER_ORE_REDUCTION, terrain, rng);

        if entry.force_big_craters {
            smudge_grid.try_place(
                SmudgeKind::Crater,
                coord,
                300,
                300,
                true,
                smudge_types,
                terrain,
                tiberium.overlay_grid(),
                occupancy,
                rng,
            );
        } else {
            smudge_grid.try_place(
                SmudgeKind::Crater,
                coord,
                dmg,
                dmg2,
                false,
                smudge_types,
                terrain,
                tiberium.overlay_grid(),
                occupancy,
                rng,
            );
        }
    }
}

/// gamemd's anim scorch-vs-crater 50/50: draw `RandomRanged(0, 0x7FFFFFFE)`,
/// then choose scorch when `roll * 2^-31 < 0.5`, i.e. `roll < 0x4000_0000`.
///
/// This is NOT a single high-bit test. The ranged draw masks to 31 bits and
/// rejects a masked `0x7FFF_FFFF` (one extra cursor advance), and the accepted
/// set is `[0, 0x4000_0000)` rather than "high bit clear". Both the draw count
/// and the accepted set must match the engine, or the shared scenario cursor
/// and the per-roll scorch/crater outcome drift.
const SMUDGE_5050_RANGED_HIGH: u32 = 0x7FFF_FFFE;
const SMUDGE_SCORCH_ACCEPT_BELOW: u32 = 0x4000_0000;
fn rng_below_half_normalized(rng: &mut SimRng) -> bool {
    rng.next_range_u32_inclusive(0, SMUDGE_5050_RANGED_HIGH) < SMUDGE_SCORCH_ACCEPT_BELOW
}

/// Building destruction center smudge — fires once per >=2x2 building.
/// Each dimension greater than two consumes one discarded ranged draw before
/// the scorch/crater roll; a 2x2 foundation consumes only the roll.
#[allow(clippy::too_many_arguments)]
pub fn try_dispatch_building_destruction_smudges(
    rx: u16,
    ry: u16,
    building_z: i32,
    foundation_w: u8,
    foundation_h: u8,
    art: &ArtRegistry,
    smudge_types: &SmudgeTypeRegistry,
    smudge_grid: &mut SmudgeGrid,
    occupancy: &OccupancyGrid,
    terrain: &mut ResolvedTerrainGrid,
    tiberium: &mut SmudgeTiberiumContext<'_>,
    rng: &mut SimRng,
) {
    let _ = art;
    if foundation_w < 2 || foundation_h < 2 {
        return;
    }
    if foundation_w > 2 {
        let _ = rng.next_range_u32(u32::from(foundation_w) - 1);
    }
    if foundation_h > 2 {
        let _ = rng.next_range_u32(u32::from(foundation_h) - 1);
    }
    let roll: u32 = rng.next_range_u32(100);
    let center = SimCoord {
        x: (rx as i32) * 256 + 128,
        y: (ry as i32) * 256 + 128,
        z: building_z,
    };
    if roll < 50 {
        smudge_grid.try_place(
            SmudgeKind::Burn,
            center,
            BUILDING_SMUDGE_DMG,
            BUILDING_SMUDGE_DMG,
            true,
            smudge_types,
            terrain,
            tiberium.overlay_grid(),
            occupancy,
            rng,
        );
    } else {
        smudge_grid.try_place(
            SmudgeKind::Crater,
            center,
            BUILDING_SMUDGE_DMG,
            BUILDING_SMUDGE_DMG,
            true,
            smudge_types,
            terrain,
            tiberium.overlay_grid(),
            occupancy,
            rng,
        );
    }
}

fn survivor_smudge_cell_passable(
    rx: u16,
    ry: u16,
    raw_occupation: &RawCellOccupationGrid,
    terrain: &ResolvedTerrainGrid,
    overlay: &OverlayGrid,
    overlay_registry: Option<&crate::map::overlay_types::OverlayTypeRegistry>,
) -> bool {
    if overlay
        .cell(rx, ry)
        .overlay_id
        .and_then(|id| overlay_registry.and_then(|registry| registry.flags(id)))
        .is_some_and(|flags| flags.wall)
    {
        return false;
    }

    let Some(cell) = terrain.cell(rx, ry) else {
        return false;
    };
    let structural_bridge = cell.bridge_facts.has_structural_bridge();
    let occupation = if structural_bridge {
        raw_occupation.deck_bits(rx, ry)
    } else {
        raw_occupation.ground_bits(rx, ry)
    };
    if occupation & 0x40 != 0 {
        return false;
    }

    structural_bridge
        || cell
            .speed_costs
            .cost_for_speed_type(SpeedType::Track)
            .is_some_and(|cost| cost != 0)
}

/// Per-foundation-cell scattered smudges. For each cell that's passable,
/// a 50/50 scorch/crater is rolled and placed at a random-offset cell within
/// 1 cell of the foundation (mirrors `SpawnSurvivors` magnitude 0x80).
#[allow(clippy::too_many_arguments)]
pub(crate) fn try_dispatch_building_survivor_smudges(
    foundation_cells: &[(u16, u16)],
    art: &ArtRegistry,
    smudge_types: &SmudgeTypeRegistry,
    smudge_grid: &mut SmudgeGrid,
    occupancy: &OccupancyGrid,
    terrain: &mut ResolvedTerrainGrid,
    raw_occupation: &RawCellOccupationGrid,
    tiberium: &mut SmudgeTiberiumContext<'_>,
    rng: &mut SimRng,
) {
    let _ = art;
    for &(cell_rx, cell_ry) in foundation_cells {
        if !survivor_smudge_cell_passable(
            cell_rx,
            cell_ry,
            raw_occupation,
            terrain,
            tiberium.overlay_grid(),
            tiberium.overlay_registry,
        ) {
            continue;
        }
        let roll: u32 = rng.next_range_u32(100);
        let base_x = (cell_rx as i32) * 256 + 128;
        let base_y = (cell_ry as i32) * 256 + 128;
        let (off_x, off_y) = random_direction_coord(rng, base_x, base_y, SURVIVOR_OFFSET_MAGNITUDE);
        let snap_rx = coord_to_cell_truncating(off_x) as u16;
        let snap_ry = coord_to_cell_truncating(off_y) as u16;
        let coord = SimCoord {
            x: (snap_rx as i32) * 256 + 128,
            y: (snap_ry as i32) * 256 + 128,
            z: 0,
        };
        if roll < 50 {
            smudge_grid.try_place(
                SmudgeKind::Burn,
                coord,
                BUILDING_SMUDGE_DMG,
                BUILDING_SMUDGE_DMG,
                false,
                smudge_types,
                terrain,
                tiberium.overlay_grid(),
                occupancy,
                rng,
            );
        } else {
            // gamemd `BuildingClass::SpawnSurvivors @ 0x00442D90` calls
            // Debris_Smoke directly; unlike AnimClass::Start, this branch does
            // not reduce tiberium before attempting the crater.
            smudge_grid.try_place(
                SmudgeKind::Crater,
                coord,
                BUILDING_SMUDGE_DMG,
                BUILDING_SMUDGE_DMG,
                false,
                smudge_types,
                terrain,
                tiberium.overlay_grid(),
                occupancy,
                rng,
            );
        }
    }
}

/// Commit one producer-ordered batch of `SmudgeSpawnRequest` events, mutating
/// `SmudgeGrid` and tiberium state before the producer returns.
#[allow(clippy::too_many_arguments)]
pub(crate) fn drain_smudge_spawn_requests(
    requests: &[SmudgeSpawnRequest],
    art: &ArtRegistry,
    smudge_types: &SmudgeTypeRegistry,
    interner: &StringInterner,
    smudge_grid: &mut SmudgeGrid,
    occupancy: &OccupancyGrid,
    terrain: &mut ResolvedTerrainGrid,
    raw_occupation: &RawCellOccupationGrid,
    tiberium: &mut SmudgeTiberiumContext<'_>,
    rng: &mut SimRng,
) {
    for req in requests {
        match req {
            SmudgeSpawnRequest::Anim {
                anim_name,
                rx,
                ry,
                sub_x,
                sub_y,
                world_z_leptons,
            } => {
                let coord = SimCoord {
                    x: (*rx as i32) * 256 + sub_x.to_num::<i32>(),
                    y: (*ry as i32) * 256 + sub_y.to_num::<i32>(),
                    z: *world_z_leptons,
                };
                let Some(cell) = terrain.cell(*rx, *ry) else {
                    continue;
                };
                let Ok(ground_z) = crate::util::lepton::ground_height_leptons(
                    cell.level,
                    cell.slope_type,
                    coord.x,
                    coord.y,
                ) else {
                    continue;
                };
                let name = interner.resolve(*anim_name);
                try_dispatch_anim_smudge(
                    art,
                    smudge_types,
                    name,
                    coord,
                    ground_z,
                    smudge_grid,
                    occupancy,
                    terrain,
                    tiberium,
                    rng,
                );
            }
            SmudgeSpawnRequest::BuildingCenter {
                rx,
                ry,
                building_z,
                foundation_w,
                foundation_h,
            } => {
                try_dispatch_building_destruction_smudges(
                    *rx,
                    *ry,
                    *building_z,
                    *foundation_w,
                    *foundation_h,
                    art,
                    smudge_types,
                    smudge_grid,
                    occupancy,
                    terrain,
                    tiberium,
                    rng,
                );
            }
            SmudgeSpawnRequest::BuildingSurvivor { cell_rx, cell_ry } => {
                try_dispatch_building_survivor_smudges(
                    &[(*cell_rx, *cell_ry)],
                    art,
                    smudge_types,
                    smudge_grid,
                    occupancy,
                    terrain,
                    raw_occupation,
                    tiberium,
                    rng,
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn smudge_5050_uses_ranged_draw_with_2pow30_threshold() {
        // gamemd: RandomRanged(0, 0x7FFFFFFE) then scorch when roll < 0x40000000.
        // seed=1 draw1 = 0x78B76ED5 (=2_025_287_381); masked to 31 bits it is
        // itself (bit31 clear) and <= 0x7FFFFFFE -> accepted in ONE draw. It is
        // >= 0x40000000 -> crater (false). The OLD high-bit test (< 0x80000000)
        // would have returned true here, so this pins the corrected threshold.
        let mut rng = SimRng::new(1);
        assert!(
            !rng_below_half_normalized(&mut rng),
            "draw1 in [2^30, 2^31) selects crater, not scorch"
        );
        // Exactly one raw draw consumed (no 0x7FFFFFFF rejection on draw1).
        let mut reference = SimRng::new(1);
        reference.next_u32();
        assert_eq!(rng.state(), reference.state());
        // draw2 = 0x275D74AE (=660_436_142) < 0x40000000 -> scorch (true).
        assert!(
            rng_below_half_normalized(&mut rng),
            "draw2 below 2^30 selects scorch"
        );
    }
}

#[cfg(test)]
mod dispatch_tests {
    use super::*;
    use crate::map::resolved_terrain::ResolvedTerrainCell;
    use crate::sim::ore_growth::OreGrowthState;
    use crate::util::fixed_math::SimFixed;

    fn tiberium_ctx<'a>(
        overlay_grid: &'a mut OverlayGrid,
        ore_growth_state: &'a mut OreGrowthState,
        radar_dirty_cells: &'a mut Vec<(u16, u16)>,
        radar_dirty_generation: &'a mut u64,
        tactical_dirty_cells: &'a mut Vec<(u16, u16)>,
    ) -> SmudgeTiberiumContext<'a> {
        SmudgeTiberiumContext {
            overlay_grid,
            ore_growth_state,
            overlay_registry: None,
            tiberium_types: None,
            source_object_cells: None,
            live_objects: None,
            binary_frame: 0,
            spread_enabled: false,
            radar_dirty_cells,
            radar_dirty_generation,
            tactical_dirty_cells,
        }
    }

    fn native_tiberium_registries() -> (
        crate::map::overlay_types::OverlayTypeRegistry,
        crate::rules::tiberium_type::TiberiumTypeRegistry,
    ) {
        let ini = crate::rules::ini_parser::IniFile::from_bytes(
            b"[Tiberiums]\n0=Riparius\n\
              [Riparius]\nImage=1\nValue=25\n\
              [OverlayTypes]\n0=ORE\n\
              [ORE]\nTiberium=yes\n",
        )
        .unwrap();
        (
            crate::map::overlay_types::OverlayTypeRegistry::from_ini(&ini, None),
            crate::rules::tiberium_type::TiberiumTypeRegistry::from_ini(&ini),
        )
    }

    fn make_art(scorch: bool, crater: bool, force_big: bool) -> ArtRegistry {
        let scorch_str = if scorch { "yes" } else { "no" };
        let crater_str = if crater { "yes" } else { "no" };
        let big_str = if force_big { "yes" } else { "no" };
        let ini_text = format!(
            "[ANIM]\nScorch={}\nCrater={}\nForceBigCraters={}\n",
            scorch_str, crater_str, big_str,
        );
        let ini = crate::rules::ini_parser::IniFile::from_bytes(ini_text.as_bytes()).unwrap();
        ArtRegistry::from_ini(&ini)
    }

    fn make_smudge_registry() -> SmudgeTypeRegistry {
        let ini = crate::rules::ini_parser::IniFile::from_bytes(
            b"[SmudgeTypes]\n1=CR1\n2=BURN1\n\
              [CR1]\nCrater=yes\nWidth=1\nHeight=1\n\
              [BURN1]\nBurn=yes\nWidth=1\nHeight=1\n",
        )
        .unwrap();
        SmudgeTypeRegistry::from_rules_ini(&ini)
    }

    fn flat_terrain(w: u16, h: u16) -> ResolvedTerrainGrid {
        let mut cells: Vec<ResolvedTerrainCell> = Vec::with_capacity((w * h) as usize);
        for ry in 0..h {
            for rx in 0..w {
                cells.push(test_default_cell(rx, ry));
            }
        }
        ResolvedTerrainGrid::from_cells(w, h, cells)
    }

    fn test_default_cell(rx: u16, ry: u16) -> ResolvedTerrainCell {
        // Reuse Task 7's defaults via copy-paste; intentionally not extracted to
        // a shared helper to keep tasks self-contained.
        ResolvedTerrainCell {
            rx,
            ry,
            source_tile_index: 0,
            source_sub_tile: 0,
            final_tile_index: 0,
            final_sub_tile: 0,
            is_wood_bridge_repair_tile: false,
            level: 0,
            filled_clear: true,
            tileset_index: Some(0),
            land_type: 0,
            yr_cell_land_type: 0,
            slope_type: 0,
            template_height: 0,
            render_offset_x: 0,
            render_offset_y: 0,
            terrain_class: Default::default(),
            speed_costs: crate::rules::terrain_rules::SpeedCostProfile {
                track: Some(100),
                ..Default::default()
            },
            is_water: false,
            is_cliff_like: false,
            is_rough: false,
            is_road: false,
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
            radar_left: [0; 3],
            radar_right: [0; 3],
            accepts_smudge: true,
            allows_tiberium: false,
            has_damaged_data: false,
            bridgehead_anchor_class_at_load: None,
        }
    }

    #[test]
    fn altitude_gate_blocks_above_30_leptons() {
        let art = make_art(false, true, false);
        let smudge_reg = make_smudge_registry();
        let mut grid = SmudgeGrid::new(8, 8);
        let mut terrain = flat_terrain(8, 8);
        let mut overlay = OverlayGrid::new(8, 8);
        let occupancy = OccupancyGrid::new();
        let mut rng = SimRng::new(1);
        let mut growth = OreGrowthState::new(8, 8);
        let mut radar_dirty = Vec::new();
        let mut radar_generation = 0;
        let mut tactical_dirty = Vec::new();
        let coord = SimCoord {
            x: 4 * 256 + 128,
            y: 4 * 256 + 128,
            z: 100,
        };
        let mut tiberium = tiberium_ctx(
            &mut overlay,
            &mut growth,
            &mut radar_dirty,
            &mut radar_generation,
            &mut tactical_dirty,
        );
        try_dispatch_anim_smudge(
            &art,
            &smudge_reg,
            "ANIM",
            coord,
            0,
            &mut grid,
            &occupancy,
            &mut terrain,
            &mut tiberium,
            &mut rng,
        );
        assert!(grid.iter_occupied().count() == 0);
    }

    #[test]
    fn altitude_gate_strict_less_than_30() {
        let art = make_art(false, true, false);
        let smudge_reg = make_smudge_registry();
        let mut grid = SmudgeGrid::new(8, 8);
        let mut terrain = flat_terrain(8, 8);
        let mut overlay = OverlayGrid::new(8, 8);
        let occupancy = OccupancyGrid::new();
        let mut rng = SimRng::new(1);
        let mut growth = OreGrowthState::new(8, 8);
        let mut radar_dirty = Vec::new();
        let mut radar_generation = 0;
        let mut tactical_dirty = Vec::new();
        let mut tiberium = tiberium_ctx(
            &mut overlay,
            &mut growth,
            &mut radar_dirty,
            &mut radar_generation,
            &mut tactical_dirty,
        );
        // z - ground_z = 30 exactly -> must FAIL (strict <)
        let coord = SimCoord {
            x: 4 * 256 + 128,
            y: 4 * 256 + 128,
            z: 30,
        };
        try_dispatch_anim_smudge(
            &art,
            &smudge_reg,
            "ANIM",
            coord,
            0,
            &mut grid,
            &occupancy,
            &mut terrain,
            &mut tiberium,
            &mut rng,
        );
        assert!(grid.iter_occupied().count() == 0);
        // z - ground_z = 29 -> must PASS
        let coord = SimCoord {
            x: 4 * 256 + 128,
            y: 4 * 256 + 128,
            z: 29,
        };
        try_dispatch_anim_smudge(
            &art,
            &smudge_reg,
            "ANIM",
            coord,
            0,
            &mut grid,
            &occupancy,
            &mut terrain,
            &mut tiberium,
            &mut rng,
        );
        assert_eq!(grid.iter_occupied().count(), 1);
    }

    #[test]
    fn gsi_04_11_elevated_ground_uses_absolute_lepton_altitude_gate() {
        let art = make_art(false, true, false);
        let smudge_reg = make_smudge_registry();
        let mut grid = SmudgeGrid::new(8, 8);
        let mut terrain = flat_terrain(8, 8);
        let cell = terrain.cell_mut(4, 4).unwrap();
        cell.level = 3;
        cell.slope_type = 0;
        let world_x = 4 * 256 + 96;
        let world_y = 4 * 256 + 160;
        let ground_z = crate::util::lepton::ground_height_leptons(
            cell.level,
            cell.slope_type,
            world_x,
            world_y,
        )
        .unwrap();
        let mut overlay = OverlayGrid::new(8, 8);
        let occupancy = OccupancyGrid::new();
        let raw_occupation = RawCellOccupationGrid::new();
        let mut interner = StringInterner::new();
        let anim_name = interner.intern("ANIM");
        let mut rng = SimRng::new(41);
        let before_reject = rng.logical_state();
        let mut growth = OreGrowthState::new(8, 8);
        let mut radar_dirty = Vec::new();
        let mut radar_generation = 0;
        let mut tactical_dirty = Vec::new();
        let mut tiberium = tiberium_ctx(
            &mut overlay,
            &mut growth,
            &mut radar_dirty,
            &mut radar_generation,
            &mut tactical_dirty,
        );

        drain_smudge_spawn_requests(
            &[SmudgeSpawnRequest::Anim {
                anim_name,
                rx: 4,
                ry: 4,
                sub_x: SimFixed::from_num(96),
                sub_y: SimFixed::from_num(160),
                world_z_leptons: ground_z + 30,
            }],
            &art,
            &smudge_reg,
            &interner,
            &mut grid,
            &occupancy,
            &mut terrain,
            &raw_occupation,
            &mut tiberium,
            &mut rng,
        );
        assert_eq!(rng.logical_state(), before_reject);
        assert_eq!(grid.iter_occupied().count(), 0);

        drain_smudge_spawn_requests(
            &[SmudgeSpawnRequest::Anim {
                anim_name,
                rx: 4,
                ry: 4,
                sub_x: SimFixed::from_num(96),
                sub_y: SimFixed::from_num(160),
                world_z_leptons: ground_z,
            }],
            &art,
            &smudge_reg,
            &interner,
            &mut grid,
            &occupancy,
            &mut terrain,
            &raw_occupation,
            &mut tiberium,
            &mut rng,
        );
        assert_eq!(
            rng.logical_state(),
            before_reject,
            "the sole placeable crater candidate uses RandomRanged(0, 0)"
        );
        assert_eq!(grid.iter_occupied().count(), 1);
    }

    #[test]
    fn gsi_04_11_parachute_object_height_rejects_smudge_without_rng() {
        let art = make_art(false, true, false);
        let smudge_reg = make_smudge_registry();
        let mut grid = SmudgeGrid::new(8, 8);
        let mut terrain = flat_terrain(8, 8);
        let mut entity =
            crate::sim::game_entity::GameEntity::test_default(7, "E1", "Americans", 4, 4);
        entity.parachute_state = Some(
            crate::sim::movement::parachute_descent::ParachuteDescentState {
                rate: -1,
                altitude: SimFixed::from_num(64),
            },
        );
        let world_z_leptons = crate::sim::combat::object_world_z_leptons(&entity, Some(&terrain));
        assert_eq!(world_z_leptons, 64);

        let mut overlay = OverlayGrid::new(8, 8);
        let occupancy = OccupancyGrid::new();
        let raw_occupation = RawCellOccupationGrid::new();
        let mut interner = StringInterner::new();
        let anim_name = interner.intern("ANIM");
        let mut rng = SimRng::new(17);
        let before_reject = rng.logical_state();
        let mut growth = OreGrowthState::new(8, 8);
        let mut radar_dirty = Vec::new();
        let mut radar_generation = 0;
        let mut tactical_dirty = Vec::new();
        let mut tiberium = tiberium_ctx(
            &mut overlay,
            &mut growth,
            &mut radar_dirty,
            &mut radar_generation,
            &mut tactical_dirty,
        );

        drain_smudge_spawn_requests(
            &[SmudgeSpawnRequest::Anim {
                anim_name,
                rx: entity.position.rx,
                ry: entity.position.ry,
                sub_x: entity.position.sub_x,
                sub_y: entity.position.sub_y,
                world_z_leptons,
            }],
            &art,
            &smudge_reg,
            &interner,
            &mut grid,
            &occupancy,
            &mut terrain,
            &raw_occupation,
            &mut tiberium,
            &mut rng,
        );

        assert_eq!(rng.logical_state(), before_reject);
        assert_eq!(grid.iter_occupied().count(), 0);
    }

    #[test]
    fn crater_path_reduces_tiberium_even_when_can_place_fails() {
        // Seed the authoritative overlay byte with 10 density levels (raw 9),
        // more than the 6-unit reduction, so the cell stays present after the
        // native partial reduction.
        let art = make_art(false, true, false);
        let smudge_reg = make_smudge_registry();
        let mut grid = SmudgeGrid::new(8, 8);
        let mut terrain = flat_terrain(8, 8);
        let mut overlay = OverlayGrid::new(8, 8);
        let (overlay_registry, tiberium_types) = native_tiberium_registries();
        let ore_id = overlay_registry.id_for_name("ORE").unwrap();
        overlay.place_overlay(4, 4, ore_id, 9);
        let occupancy = OccupancyGrid::new();
        let mut rng = SimRng::new(1);
        let mut growth = OreGrowthState::new(8, 8);
        let mut radar_dirty = Vec::new();
        let mut radar_generation = 0;
        let mut tactical_dirty = Vec::new();
        let coord = SimCoord {
            x: 4 * 256 + 128,
            y: 4 * 256 + 128,
            z: 0,
        };
        {
            let mut tiberium = tiberium_ctx(
                &mut overlay,
                &mut growth,
                &mut radar_dirty,
                &mut radar_generation,
                &mut tactical_dirty,
            );
            tiberium.overlay_registry = Some(&overlay_registry);
            tiberium.tiberium_types = Some(&tiberium_types);
            try_dispatch_anim_smudge(
                &art,
                &smudge_reg,
                "ANIM",
                coord,
                0,
                &mut grid,
                &occupancy,
                &mut terrain,
                &mut tiberium,
                &mut rng,
            );
        }
        // Smudge NOT placed (overlay blocks) but ore reduced by 6 density levels.
        assert_eq!(grid.iter_occupied().count(), 0);
        assert_eq!(overlay.cell(4, 4).overlay_id, Some(ore_id));
        assert_eq!(overlay.cell(4, 4).overlay_data, 3);
    }

    #[test]
    fn gsi_04_11_anim_crater_reduces_ore_before_zero_zero_sentinel_rejection() {
        let art = make_art(false, true, false);
        let smudge_reg = make_smudge_registry();
        let mut grid = SmudgeGrid::new(8, 8);
        let mut terrain = flat_terrain(8, 8);
        let mut overlay = OverlayGrid::new(8, 8);
        let (overlay_registry, tiberium_types) = native_tiberium_registries();
        let ore_id = overlay_registry.id_for_name("ORE").unwrap();
        overlay.place_overlay(0, 0, ore_id, 9);
        let occupancy = OccupancyGrid::new();
        let mut rng = SimRng::new(1);
        let mut growth = OreGrowthState::new(8, 8);
        let mut radar_dirty = Vec::new();
        let mut radar_generation = 0;
        let mut tactical_dirty = Vec::new();
        {
            let mut tiberium = tiberium_ctx(
                &mut overlay,
                &mut growth,
                &mut radar_dirty,
                &mut radar_generation,
                &mut tactical_dirty,
            );
            tiberium.overlay_registry = Some(&overlay_registry);
            tiberium.tiberium_types = Some(&tiberium_types);
            try_dispatch_anim_smudge(
                &art,
                &smudge_reg,
                "ANIM",
                SimCoord {
                    x: 128,
                    y: 128,
                    z: 0,
                },
                0,
                &mut grid,
                &occupancy,
                &mut terrain,
                &mut tiberium,
                &mut rng,
            );
        }
        assert_eq!(grid.iter_occupied().count(), 0);
        assert_eq!(overlay.cell(0, 0).overlay_id, Some(ore_id));
        assert_eq!(overlay.cell(0, 0).overlay_data, 3);
    }

    #[test]
    fn scorch_only_anim_spawns_burn() {
        let art = make_art(true, false, false);
        let smudge_reg = make_smudge_registry();
        let mut grid = SmudgeGrid::new(8, 8);
        let mut terrain = flat_terrain(8, 8);
        let mut overlay = OverlayGrid::new(8, 8);
        let occupancy = OccupancyGrid::new();
        let mut rng = SimRng::new(1);
        let mut growth = OreGrowthState::new(8, 8);
        let mut radar_dirty = Vec::new();
        let mut radar_generation = 0;
        let mut tactical_dirty = Vec::new();
        let mut tiberium = tiberium_ctx(
            &mut overlay,
            &mut growth,
            &mut radar_dirty,
            &mut radar_generation,
            &mut tactical_dirty,
        );
        let coord = SimCoord {
            x: 4 * 256 + 128,
            y: 4 * 256 + 128,
            z: 0,
        };
        try_dispatch_anim_smudge(
            &art,
            &smudge_reg,
            "ANIM",
            coord,
            0,
            &mut grid,
            &occupancy,
            &mut terrain,
            &mut tiberium,
            &mut rng,
        );
        let placed = grid.cell(4, 4).type_id.unwrap();
        // BURN1 is index 1 in the registry above.
        assert_eq!(placed, 1);
    }

    // Building destruction + survivor dispatcher tests live inside
    // `dispatch_tests` so they can reuse the helpers defined above
    // (`make_smudge_registry`, `flat_terrain`, `test_default_cell`).
    mod building_dispatch_tests {
        use super::*;

        #[test]
        fn destruction_skipped_for_1x1_foundation() {
            let smudge_reg = make_smudge_registry();
            let mut grid = SmudgeGrid::new(8, 8);
            let art = ArtRegistry::empty();
            let mut terrain = flat_terrain(8, 8);
            let mut overlay = OverlayGrid::new(8, 8);
            let occupancy = OccupancyGrid::new();
            let mut rng = SimRng::new(1);
            let mut growth = OreGrowthState::new(8, 8);
            let mut radar_dirty = Vec::new();
            let mut radar_generation = 0;
            let mut tactical_dirty = Vec::new();
            let mut tiberium = tiberium_ctx(
                &mut overlay,
                &mut growth,
                &mut radar_dirty,
                &mut radar_generation,
                &mut tactical_dirty,
            );
            try_dispatch_building_destruction_smudges(
                4,
                4,
                0,
                1,
                1, // 1x1 foundation
                &art,
                &smudge_reg,
                &mut grid,
                &occupancy,
                &mut terrain,
                &mut tiberium,
                &mut rng,
            );
            assert_eq!(grid.iter_occupied().count(), 0);
        }

        fn run_center_smudge(foundation_w: u8, foundation_h: u8) -> SimRng {
            let smudge_reg = make_smudge_registry();
            let mut grid = SmudgeGrid::new(8, 8);
            let art = ArtRegistry::empty();
            let mut terrain = flat_terrain(8, 8);
            let mut overlay = OverlayGrid::new(8, 8);
            let occupancy = OccupancyGrid::new();
            let mut growth = OreGrowthState::new(8, 8);
            let mut radar_dirty = Vec::new();
            let mut radar_generation = 0;
            let mut tactical_dirty = Vec::new();
            let mut tiberium = tiberium_ctx(
                &mut overlay,
                &mut growth,
                &mut radar_dirty,
                &mut radar_generation,
                &mut tactical_dirty,
            );
            let mut rng = SimRng::new(42);
            try_dispatch_building_destruction_smudges(
                4,
                4,
                0,
                foundation_w,
                foundation_h,
                &art,
                &smudge_reg,
                &mut grid,
                &occupancy,
                &mut terrain,
                &mut tiberium,
                &mut rng,
            );
            assert_eq!(grid.iter_occupied().count(), 1);
            rng
        }

        #[test]
        fn gsi_04_11_center_smudge_2x2_roll_only_3x3_discards_both_dimensions() {
            let actual_2x2 = run_center_smudge(2, 2);
            let mut expected_2x2 = SimRng::new(42);
            expected_2x2.next_range_u32(100);
            assert_eq!(actual_2x2.logical_state(), expected_2x2.logical_state());

            let actual_3x3 = run_center_smudge(3, 3);
            let mut expected_3x3 = SimRng::new(42);
            expected_3x3.next_range_u32(2);
            expected_3x3.next_range_u32(2);
            expected_3x3.next_range_u32(100);
            assert_eq!(actual_3x3.logical_state(), expected_3x3.logical_state());
        }

        #[test]
        fn gsi_04_11_survivor_gate_uses_native_mask_wall_track_and_bridge_order() {
            let mut terrain = flat_terrain(8, 8);
            let mut overlay = OverlayGrid::new(8, 8);
            let registry_ini = crate::rules::ini_parser::IniFile::from_str(
                "[OverlayTypes]\n0=TESTWALL\n[TESTWALL]\nWall=yes\n",
            );
            let overlay_registry =
                crate::map::overlay_types::OverlayTypeRegistry::from_ini(&registry_ini, None);
            let mut raw = RawCellOccupationGrid::new();

            raw.mark_ground(1, 1, 0x80);
            assert!(survivor_smudge_cell_passable(
                1,
                1,
                &raw,
                &terrain,
                &overlay,
                Some(&overlay_registry),
            ));

            raw.mark_ground(1, 2, 0x40);
            assert!(!survivor_smudge_cell_passable(
                1,
                2,
                &raw,
                &terrain,
                &overlay,
                Some(&overlay_registry),
            ));

            terrain.cell_mut(1, 3).unwrap().speed_costs.track = None;
            assert!(!survivor_smudge_cell_passable(
                1,
                3,
                &raw,
                &terrain,
                &overlay,
                Some(&overlay_registry),
            ));
            terrain.cell_mut(1, 4).unwrap().speed_costs.track = Some(0);
            assert!(!survivor_smudge_cell_passable(
                1,
                4,
                &raw,
                &terrain,
                &overlay,
                Some(&overlay_registry),
            ));

            let bridge = terrain.cell_mut(1, 5).unwrap();
            bridge.speed_costs.track = Some(0);
            bridge.bridge_facts.raw_flags = crate::map::bridge_facts::BRIDGE_FLAG_STRUCTURAL;
            raw.mark_ground(1, 5, 0x40);
            assert!(
                survivor_smudge_cell_passable(
                    1,
                    5,
                    &raw,
                    &terrain,
                    &overlay,
                    Some(&overlay_registry),
                ),
                "a structural bridge selects the empty deck byte and bypasses Track=0"
            );
            raw.mark_deck(1, 5, 0x40);
            assert!(!survivor_smudge_cell_passable(
                1,
                5,
                &raw,
                &terrain,
                &overlay,
                Some(&overlay_registry),
            ));

            let wall_bridge = terrain.cell_mut(1, 6).unwrap();
            wall_bridge.speed_costs.track = Some(0);
            wall_bridge.bridge_facts.raw_flags = crate::map::bridge_facts::BRIDGE_FLAG_STRUCTURAL;
            overlay.place_overlay(1, 6, 0, 0);
            assert!(!survivor_smudge_cell_passable(
                1,
                6,
                &raw,
                &terrain,
                &overlay,
                Some(&overlay_registry),
            ));

            let smudge_reg = make_smudge_registry();
            let art = ArtRegistry::empty();
            let occupancy = OccupancyGrid::new();
            let mut grid = SmudgeGrid::new(8, 8);
            let mut growth = OreGrowthState::new(8, 8);
            let mut radar_dirty = Vec::new();
            let mut radar_generation = 0;
            let mut tactical_dirty = Vec::new();
            let mut rng = SimRng::new(17);
            let before = rng.logical_state();
            let mut tiberium = SmudgeTiberiumContext {
                overlay_grid: &mut overlay,
                ore_growth_state: &mut growth,
                overlay_registry: Some(&overlay_registry),
                tiberium_types: None,
                source_object_cells: None,
                live_objects: None,
                binary_frame: 0,
                spread_enabled: false,
                radar_dirty_cells: &mut radar_dirty,
                radar_dirty_generation: &mut radar_generation,
                tactical_dirty_cells: &mut tactical_dirty,
            };
            try_dispatch_building_survivor_smudges(
                &[(1, 6)],
                &art,
                &smudge_reg,
                &mut grid,
                &occupancy,
                &mut terrain,
                &raw,
                &mut tiberium,
                &mut rng,
            );
            assert_eq!(
                rng.logical_state(),
                before,
                "a rejected cell consumes no RNG"
            );
        }

        #[test]
        fn gsi_05_15_survivor_crater_leaves_ore_untouched() {
            let crater_seed = (1_u64..100)
                .find(|seed| {
                    let mut probe = SimRng::new(*seed);
                    probe.next_range_u32(100) >= 50
                })
                .expect("fixture has a crater-selecting seed");
            let smudge_reg = make_smudge_registry();
            let art = ArtRegistry::empty();
            let occupancy = OccupancyGrid::new();
            let mut survivor_grid = SmudgeGrid::new(8, 8);
            let mut survivor_terrain = flat_terrain(8, 8);
            let mut survivor_overlay = OverlayGrid::new(8, 8);
            let (overlay_registry, tiberium_types) = native_tiberium_registries();
            let ore_id = overlay_registry.id_for_name("ORE").unwrap();
            // Cover every possible snapped destination with authoritative ore,
            // then clear fixture dirties so any mutation below is observable.
            for ry in 0..8 {
                for rx in 0..8 {
                    survivor_overlay.place_overlay(rx, ry, ore_id, 9);
                }
            }
            survivor_overlay.take_dirty_cells();
            let raw = RawCellOccupationGrid::new();
            let mut survivor_growth = OreGrowthState::new(8, 8);
            let mut survivor_radar = Vec::new();
            let mut survivor_generation = 0;
            let mut survivor_tactical = Vec::new();
            let mut survivor_rng = SimRng::new(crater_seed);
            let mut expected_rng = SimRng::new(crater_seed);
            assert!(expected_rng.next_range_u32(100) >= 50);
            let _ = random_direction_coord(
                &mut expected_rng,
                4 * 256 + 128,
                4 * 256 + 128,
                SURVIVOR_OFFSET_MAGNITUDE,
            );
            {
                let mut tiberium = tiberium_ctx(
                    &mut survivor_overlay,
                    &mut survivor_growth,
                    &mut survivor_radar,
                    &mut survivor_generation,
                    &mut survivor_tactical,
                );
                tiberium.overlay_registry = Some(&overlay_registry);
                tiberium.tiberium_types = Some(&tiberium_types);
                try_dispatch_building_survivor_smudges(
                    &[(4, 4)],
                    &art,
                    &smudge_reg,
                    &mut survivor_grid,
                    &occupancy,
                    &mut survivor_terrain,
                    &raw,
                    &mut tiberium,
                    &mut survivor_rng,
                );
            }
            for ry in 0..8 {
                for rx in 0..8 {
                    let cell = survivor_overlay.cell(rx, ry);
                    assert_eq!(cell.overlay_id, Some(ore_id), "cell ({rx},{ry})");
                    assert_eq!(cell.overlay_data, 9, "cell ({rx},{ry})");
                }
            }
            assert_eq!(survivor_grid.iter_occupied().count(), 0);
            assert!(survivor_overlay.take_dirty_cells().is_empty());
            assert!(
                survivor_growth
                    .native_tiberium_state()
                    .classes
                    .iter()
                    .all(|class| class.growth.is_empty() && class.spread.is_empty())
            );
            assert!(survivor_radar.is_empty());
            assert_eq!(survivor_generation, 0);
            assert!(survivor_tactical.is_empty());
            assert_eq!(survivor_rng.logical_state(), expected_rng.logical_state());
        }
    }
}
