//! Read-only live-world fact adapter for behavior-3 Spark collision.
//!
//! The adapter mirrors the native query order without borrowing mutable
//! simulation state or consuming RNG. It returns an owned fact bundle so the
//! Spark owner can release all world borrows before advancing the Scenario RNG.

use glam::IVec3;
use thiserror::Error;

use super::spark::{
    SparkCollisionFacts, SparkKernelError, SparkMotionStep, bridge_collision_kind,
    classify_collision_kind, in_contact_band,
};
use crate::map::bridge_facts::BRIDGE_FLAG_STRUCTURAL;
use crate::map::entities::EntityCategory;
use crate::map::resolved_terrain::ResolvedTerrainGrid;
use crate::map::voxel_slope::slope_matrix;
use crate::rules::foundation::foundation_dimensions;
use crate::rules::ruleset::RuleSet;
use crate::sim::cell_rect::{CellRef, get_cellclass_fallback_leptons};
use crate::sim::movement::locomotor::MovementLayer;
use crate::sim::world::Simulation;
use crate::util::lepton::{UnsupportedGroundSlope, ground_height_leptons};
use crate::util::native_x87::NativeF32Bits;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum SparkWorldError {
    #[error("Spark collision requires resolved terrain")]
    MissingTerrain,
    #[error("Spark collision requires the mutable overlay grid")]
    MissingOverlayGrid,
    #[error("overlay state is unavailable at canonical cell ({rx}, {ry})")]
    UnavailableOverlayCell { rx: u16, ry: u16 },
    #[error("slope type {0} is outside the verified 0..=20 table")]
    UnsupportedSlope(u8),
    #[error("cell occupancy references missing entity {0}")]
    MissingOccupantEntity(u64),
    #[error("building entity {0} has no resolved ObjectType")]
    MissingObjectType(u64),
    #[error("building entity {0} needs unmodelled LaserFence connectivity state")]
    UnsupportedLaserFence(u64),
    #[error(transparent)]
    Kernel(#[from] SparkKernelError),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum SparkCellSelectionRole {
    Ground,
    Cell,
    Slope,
}

#[derive(Debug, Clone)]
struct SelectedSparkCell<'a> {
    role: SparkCellSelectionRole,
    cell: CellRef<'a>,
}

// gamemd-derived: MapClass::Get_CellClass_At_Coord @ 0x00565730 uses the
// fixed-512 real-or-shared-dummy selection and stamps a miss before continuing.
fn select_cell<'a>(
    terrain: &'a ResolvedTerrainGrid,
    role: SparkCellSelectionRole,
    world_x: i32,
    world_y: i32,
) -> SelectedSparkCell<'a> {
    SelectedSparkCell {
        role,
        cell: get_cellclass_fallback_leptons(Some(terrain), world_x, world_y),
    }
}

// gamemd-derived: CellClass::GetGroundHeight @ 0x00578080 evaluates the real
// or shared-dummy CellClass selected for the original world coordinate.
fn ground_height_for_selection(
    selection: &SelectedSparkCell<'_>,
    world_x: i32,
    world_y: i32,
) -> Result<i32, SparkWorldError> {
    debug_assert_eq!(selection.role, SparkCellSelectionRole::Ground);
    let (level, slope_type) = match &selection.cell {
        CellRef::Real(cell) => (cell.level, cell.slope_type),
        CellRef::Dummy { cell } => {
            let snapshot = cell.snapshot();
            (snapshot.level as u8, snapshot.slope_type)
        }
    };
    ground_height_leptons(level, slope_type, world_x, world_y)
        .map_err(|UnsupportedGroundSlope(slope)| SparkWorldError::UnsupportedSlope(slope))
}

/// Constructor-only terrain query. Missing terrain retains the pre-existing
/// no-floor policy; with terrain present, a native miss returns the live dummy
/// and therefore still produces a ground height.
///
/// gamemd-derived: ParticleClass::Constructor @ 0x0062B5E0 calls
/// CellClass::GetGroundHeight @ 0x00578080 once for the comparison and again
/// only when the input Z is at or below the first result.
pub(super) fn constructor_ground_height(
    sim: &Simulation,
    world_x: i32,
    world_y: i32,
) -> Result<Option<i32>, SparkWorldError> {
    let Some(terrain) = sim.resolved_terrain.as_ref() else {
        return Ok(None);
    };
    let selected = select_cell(terrain, SparkCellSelectionRole::Ground, world_x, world_y);
    ground_height_for_selection(&selected, world_x, world_y).map(Some)
}

/// SmokeMove62D3F0 queries the OLD cell and reads ground only if that
/// selected real/dummy cell has a live structural bridge. No overlay lookup.
pub(super) fn smoke_bridge_deck(
    sim: &Simulation,
    coords: IVec3,
) -> Result<Option<i32>, SparkWorldError> {
    let Some(terrain) = sim.resolved_terrain.as_ref() else {
        return Ok(None);
    };
    let cell = select_cell(terrain, SparkCellSelectionRole::Cell, coords.x, coords.y);
    if !SparkCollisionWorld::live_structural_bridge(&cell)? {
        return Ok(None);
    }
    let ground = select_cell(terrain, SparkCellSelectionRole::Ground, coords.x, coords.y);
    Ok(Some(
        ground_height_for_selection(&ground, coords.x, coords.y)?
            .wrapping_add(crate::util::lepton::BRIDGE_DECK_HEIGHT_LEPTONS),
    ))
}

/// Concrete read-only view over the live simulation state Spark collision reads.
pub struct SparkCollisionWorld<'a> {
    sim: &'a Simulation,
    rules: &'a RuleSet,
}

/// Selector calls in order: `(role, world x, world y)`.
#[cfg(test)]
type SparkSelectionTranscript = Vec<(SparkCellSelectionRole, i32, i32)>;

impl<'a> SparkCollisionWorld<'a> {
    pub fn new(sim: &'a Simulation, rules: &'a RuleSet) -> Result<Self, SparkWorldError> {
        if sim.resolved_terrain.is_none() {
            return Err(SparkWorldError::MissingTerrain);
        }
        Ok(Self { sim, rules })
    }

    /// Gather every native collision input before the owner borrows mutable RNG.
    pub fn query(&self, motion: SparkMotionStep) -> Result<SparkCollisionFacts, SparkWorldError> {
        let terrain = self
            .sim
            .resolved_terrain
            .as_ref()
            .ok_or(SparkWorldError::MissingTerrain)?;
        self.query_with_selector(motion, |role, x, y| select_cell(terrain, role, x, y))
    }

    #[cfg(test)]
    pub(super) fn query_with_transcript(
        &self,
        motion: SparkMotionStep,
    ) -> Result<(SparkCollisionFacts, SparkSelectionTranscript), SparkWorldError> {
        let terrain = self
            .sim
            .resolved_terrain
            .as_ref()
            .ok_or(SparkWorldError::MissingTerrain)?;
        let mut transcript = Vec::new();
        let facts = self.query_with_selector(motion, |role, x, y| {
            transcript.push((role, x, y));
            select_cell(terrain, role, x, y)
        })?;
        Ok((facts, transcript))
    }

    fn query_with_selector<F>(
        &self,
        motion: SparkMotionStep,
        mut select: F,
    ) -> Result<SparkCollisionFacts, SparkWorldError>
    where
        F: FnMut(SparkCellSelectionRole, i32, i32) -> SelectedSparkCell<'a>,
    {
        // gamemd-derived: behavior-3 ParticleClass AI @ 0x0062C6E0 orders
        // candidate ground, candidate cell, conditional old cell, gated
        // building/wall, then a collision-only candidate slope selection.
        let candidate_ground = select(
            SparkCellSelectionRole::Ground,
            motion.candidate_coords.x,
            motion.candidate_coords.y,
        );
        let ground_z = ground_height_for_selection(
            &candidate_ground,
            motion.candidate_coords.x,
            motion.candidate_coords.y,
        )?;

        let candidate_cell = select(
            SparkCellSelectionRole::Cell,
            motion.candidate_coords.x,
            motion.candidate_coords.y,
        );
        let candidate_has_structural_bridge = Self::live_structural_bridge(&candidate_cell)?;
        let old_has_structural_bridge = if candidate_has_structural_bridge {
            false
        } else {
            let old_cell = select(
                SparkCellSelectionRole::Cell,
                motion.old_coords.x,
                motion.old_coords.y,
            );
            Self::live_structural_bridge(&old_cell)?
        };

        let bridge_kind = bridge_collision_kind(
            motion,
            ground_z,
            old_has_structural_bridge,
            candidate_has_structural_bridge,
        );
        let mut accepted_building = false;
        let mut wall_overlay_id = None;
        if bridge_kind.is_none() && in_contact_band(motion, ground_z)? {
            accepted_building = self.accepted_building_for_selection(&candidate_cell)?;
            if !accepted_building {
                wall_overlay_id = self.wall_overlay_for_selection(&candidate_cell)?;
            }
        }

        let mut facts = SparkCollisionFacts {
            ground_z,
            slope_matrix: None,
            old_has_structural_bridge,
            candidate_has_structural_bridge,
            accepted_building,
            wall_overlay_id,
        };
        if classify_collision_kind(motion, facts)?.is_some() {
            let candidate_slope = select(
                SparkCellSelectionRole::Slope,
                motion.candidate_coords.x,
                motion.candidate_coords.y,
            );
            facts.slope_matrix = Some(self.slope_matrix_for_selection(&candidate_slope)?);
        }
        Ok(facts)
    }

    fn live_structural_bridge(selection: &SelectedSparkCell<'_>) -> Result<bool, SparkWorldError> {
        debug_assert_eq!(selection.role, SparkCellSelectionRole::Cell);
        match &selection.cell {
            CellRef::Dummy { cell } => {
                Ok(cell.snapshot().bridge_flags_0x1180 & BRIDGE_FLAG_STRUCTURAL != 0)
            }
            CellRef::Real(cell) => Ok(cell.bridge_facts.has_structural_bridge()),
        }
    }

    fn accepted_building_for_selection(
        &self,
        selection: &SelectedSparkCell<'_>,
    ) -> Result<bool, SparkWorldError> {
        match &selection.cell {
            CellRef::Dummy { .. } => Ok(false),
            CellRef::Real(cell) => self.accepted_building(cell.rx, cell.ry),
        }
    }

    fn wall_overlay_for_selection(
        &self,
        selection: &SelectedSparkCell<'_>,
    ) -> Result<Option<u8>, SparkWorldError> {
        match &selection.cell {
            CellRef::Dummy { .. } => Ok(None),
            CellRef::Real(cell) => {
                let (rx, ry) = (cell.rx, cell.ry);
                let overlay = self
                    .sim
                    .overlay_grid
                    .as_ref()
                    .ok_or(SparkWorldError::MissingOverlayGrid)?;
                if rx >= overlay.width() || ry >= overlay.height() {
                    return Err(SparkWorldError::UnavailableOverlayCell { rx, ry });
                }
                Ok(overlay.cell(rx, ry).overlay_id)
            }
        }
    }

    fn slope_matrix_for_selection(
        &self,
        selection: &SelectedSparkCell<'_>,
    ) -> Result<[NativeF32Bits; 12], SparkWorldError> {
        debug_assert_eq!(selection.role, SparkCellSelectionRole::Slope);
        let slope_type = match &selection.cell {
            CellRef::Real(cell) => cell.slope_type,
            CellRef::Dummy { cell } => cell.snapshot().slope_type,
        };
        slope_matrix(slope_type).ok_or(SparkWorldError::UnsupportedSlope(slope_type))
    }

    fn accepted_building(&self, rx: u16, ry: u16) -> Result<bool, SparkWorldError> {
        let Some(occupancy) = self.sim.occupancy().get(rx, ry) else {
            return Ok(false);
        };
        for occupant in occupancy.iter_layer(MovementLayer::Ground) {
            let entity = self
                .sim
                .entities()
                .get(occupant.entity_id)
                .ok_or(SparkWorldError::MissingOccupantEntity(occupant.entity_id))?;
            if entity.category() != EntityCategory::Structure {
                continue;
            }

            // Native stops at the first building in the selected object list.
            let object = self
                .sim
                .object_type(entity.type_ref(), self.rules)
                .ok_or(SparkWorldError::MissingObjectType(entity.stable_id()))?;
            if object.laser_fence {
                return Err(SparkWorldError::UnsupportedLaserFence(entity.stable_id()));
            }
            let undeploys_from_one_by_one = object
                .undeploys_into
                .as_deref()
                .and_then(|target| self.rules.object(target))
                .is_some()
                && foundation_dimensions(&object.foundation) == (1, 1);
            return Ok(!undeploys_from_one_by_one);
        }
        Ok(false)
    }
}

#[cfg(test)]
pub(super) mod tests {
    use super::*;
    use glam::IVec3;

    use crate::map::bridge_facts::BRIDGE_FLAG_STRUCTURAL;
    use crate::map::resolved_terrain::{ResolvedTerrainCell, ResolvedTerrainGrid};
    use crate::rules::ini_parser::IniFile;
    use crate::rules::terrain_rules::TerrainClass;
    use crate::sim::combat::Health;
    use crate::sim::game_entity::GameEntity;
    use crate::sim::occupancy::CellListInsertion;
    use crate::sim::overlay_grid::OverlayGrid;

    pub(in crate::sim::particles) fn terrain_cell(rx: u16, ry: u16) -> ResolvedTerrainCell {
        ResolvedTerrainCell {
            tileset_index: None,
            base_terrain_class: TerrainClass::Clear,
            ..crate::map::resolved_terrain::test_flat_cell(rx, ry)
        }
    }

    fn motion_at(x: i32, y: i32) -> SparkMotionStep {
        motion_between(IVec3::new(x, y, 100), IVec3::new(x, y, 100))
    }

    fn motion_between(old_coords: IVec3, candidate_coords: IVec3) -> SparkMotionStep {
        SparkMotionStep {
            old_coords,
            candidate_coords,
            candidate_f32: [
                NativeF32Bits::from_bits((candidate_coords.x as f32).to_bits()),
                NativeF32Bits::from_bits((candidate_coords.y as f32).to_bits()),
                NativeF32Bits::from_bits((candidate_coords.z as f32).to_bits()),
            ],
            persistent_velocity: [NativeF32Bits::POSITIVE_ZERO; 3],
            probe_velocity: [NativeF32Bits::POSITIVE_ZERO; 3],
        }
    }

    pub(in crate::sim::particles) fn one_cell_sim(cell: ResolvedTerrainCell) -> Simulation {
        let mut sim = Simulation::new();
        sim.resolved_terrain = Some(ResolvedTerrainGrid::from_cells(1, 1, vec![cell]));
        sim.overlay_grid = Some(OverlayGrid::new(1, 1));
        sim
    }

    fn empty_rules() -> RuleSet {
        RuleSet::from_ini(&IniFile::from_str("")).unwrap()
    }

    fn add_building(sim: &mut Simulation, id: u64, type_name: &str) {
        let owner = sim.interner.intern("Neutral");
        let type_ref = sim.interner.intern(type_name);
        let entity = GameEntity::new_at_frame_zero_for_test(
            id,
            0,
            0,
            0,
            0,
            owner,
            Health::new(100),
            type_ref,
            EntityCategory::Structure,
            0,
            0,
            false,
        );
        sim.entities_mut().insert(entity);
        sim.occupancy_mut().add(
            0,
            0,
            id,
            MovementLayer::Ground,
            None,
            CellListInsertion::AppendBuilding,
        );
    }

    #[test]
    fn fixed_stride_lookup_preserves_native_aliasing() {
        let terrain = ResolvedTerrainGrid::from_cells(1, 1, vec![terrain_cell(0, 0)]);
        assert!(matches!(
            select_cell(&terrain, SparkCellSelectionRole::Cell, 512 * 256, -256).cell,
            CellRef::Real(_)
        ));
        assert!(matches!(
            select_cell(&terrain, SparkCellSelectionRole::Cell, -256, 0).cell,
            CellRef::Dummy { .. }
        ));
    }

    #[test]
    fn matrix_table_preserves_exact_aliases_and_identity_tail() {
        assert_eq!(slope_matrix(5), slope_matrix(9));
        assert_eq!(slope_matrix(6), slope_matrix(10));
        assert_eq!(slope_matrix(7), slope_matrix(11));
        assert_eq!(slope_matrix(8), slope_matrix(12));
        for slope in 17..=20 {
            assert_eq!(slope_matrix(slope), slope_matrix(0));
        }
        assert_eq!(slope_matrix(1).unwrap()[0].bits(), 0x3f5f3968);
        assert_eq!(slope_matrix(16).unwrap()[10].bits(), 0x3f5f397a);
        assert_eq!(slope_matrix(21), None);
    }

    #[test]
    fn live_bridge_fact_is_the_live_structural_flag() {
        let mut cell = terrain_cell(0, 0);
        cell.bridge_facts.raw_flags = BRIDGE_FLAG_STRUCTURAL;
        let mut sim = one_cell_sim(cell);
        let rules = empty_rules();

        let intact = SparkCollisionWorld::new(&sim, &rules)
            .unwrap()
            .query(motion_at(0, 0))
            .unwrap();
        assert!(!intact.old_has_structural_bridge);
        assert!(intact.candidate_has_structural_bridge);

        sim.resolved_terrain
            .as_mut()
            .unwrap()
            .cell_mut(0, 0)
            .unwrap()
            .bridge_facts
            .raw_flags &= !BRIDGE_FLAG_STRUCTURAL;
        let collapsed = SparkCollisionWorld::new(&sim, &rules)
            .unwrap()
            .query(motion_at(0, 0))
            .unwrap();
        assert!(!collapsed.old_has_structural_bridge);
        assert!(!collapsed.candidate_has_structural_bridge);
    }

    #[test]
    fn world_query_uses_fixed_stride_alias_for_real_cell_data() {
        let mut cell = terrain_cell(0, 0);
        cell.level = 2;
        let sim = one_cell_sim(cell);
        sim.resolved_terrain
            .as_ref()
            .unwrap()
            .shared_cell_dummy()
            .stamp_coord(77, -9);
        let rules = empty_rules();
        let aliased = SparkCollisionWorld::new(&sim, &rules)
            .unwrap()
            .query(motion_at(512 * 256, -256))
            .unwrap();
        assert_eq!(aliased.ground_z, 208);
        assert_eq!(
            sim.resolved_terrain
                .as_ref()
                .unwrap()
                .shared_cell_dummy()
                .snapshot()
                .coord,
            (77, -9),
            "a fixed-stride real alias leaves the dummy untouched"
        );
    }

    #[test]
    fn gsi_04_03_production_selection_seam_records_native_query_transcript() {
        let sim = one_cell_sim(terrain_cell(0, 0));
        let rules = empty_rules();
        let world = SparkCollisionWorld::new(&sim, &rules).unwrap();
        let terrain = sim.resolved_terrain.as_ref().unwrap();
        let motion = motion_between(IVec3::new(0, 0, 200), IVec3::new(256, 0, 200));
        let mut transcript = Vec::new();

        let facts = world
            .query_with_selector(motion, |role, x, y| {
                transcript.push((role, x, y));
                select_cell(terrain, role, x, y)
            })
            .unwrap();

        assert_eq!(
            transcript,
            vec![
                (SparkCellSelectionRole::Ground, 256, 0),
                (SparkCellSelectionRole::Cell, 256, 0),
                (SparkCellSelectionRole::Cell, 0, 0),
            ]
        );
        assert!(
            facts.slope_matrix.is_none(),
            "no collision performs no slope lookup"
        );
        assert_eq!(terrain.shared_cell_dummy().snapshot().coord, (1, 0));
    }

    #[test]
    fn gsi_04_03_collision_reselects_candidate_slope_after_old_cell_lookup() {
        let sim = one_cell_sim(terrain_cell(0, 0));
        let rules = empty_rules();
        let world = SparkCollisionWorld::new(&sim, &rules).unwrap();
        let terrain = sim.resolved_terrain.as_ref().unwrap();
        let motion = motion_between(IVec3::new(0, 0, 20), IVec3::new(256, 0, -100));
        let mut transcript = Vec::new();

        let facts = world
            .query_with_selector(motion, |role, x, y| {
                transcript.push((role, x, y));
                select_cell(terrain, role, x, y)
            })
            .unwrap();

        assert_eq!(
            transcript,
            vec![
                (SparkCellSelectionRole::Ground, 256, 0),
                (SparkCellSelectionRole::Cell, 256, 0),
                (SparkCellSelectionRole::Cell, 0, 0),
                (SparkCellSelectionRole::Slope, 256, 0),
            ]
        );
        assert!(facts.slope_matrix.is_some());
        assert_eq!(terrain.shared_cell_dummy().snapshot().coord, (1, 0));
    }

    #[test]
    fn gsi_04_03_dummy_collision_uses_exact_nonzero_slope_and_candidate_restamp() {
        let sim = one_cell_sim(terrain_cell(0, 0));
        let terrain = sim.resolved_terrain.as_ref().unwrap();
        terrain.test_set_dummy_cell_level_slope(0, 5);
        let rules = empty_rules();
        let world = SparkCollisionWorld::new(&sim, &rules).unwrap();
        let motion = motion_between(IVec3::new(512, 0, 20), IVec3::new(320, 192, -100));
        let (facts, transcript) = world.query_with_transcript(motion).unwrap();

        assert_eq!(
            transcript,
            vec![
                (SparkCellSelectionRole::Ground, 320, 192),
                (SparkCellSelectionRole::Cell, 320, 192),
                (SparkCellSelectionRole::Cell, 512, 0),
                (SparkCellSelectionRole::Slope, 320, 192),
            ]
        );
        assert_eq!(facts.slope_matrix, Some(slope_matrix(5).unwrap()));
        assert_eq!(
            terrain.shared_cell_dummy().snapshot().coord,
            (1, 0),
            "the collision-only slope selection must restamp the candidate after old"
        );
    }

    #[test]
    fn gsi_04_03_candidate_and_old_dummy_no_collision_finishes_with_old_stamp() {
        let sim = one_cell_sim(terrain_cell(0, 0));
        let rules = empty_rules();
        let world = SparkCollisionWorld::new(&sim, &rules).unwrap();
        let terrain = sim.resolved_terrain.as_ref().unwrap();
        let motion = motion_between(IVec3::new(512, 0, 200), IVec3::new(256, 0, 200));
        let mut transcript = Vec::new();

        let facts = world
            .query_with_selector(motion, |role, x, y| {
                transcript.push((role, x, y));
                select_cell(terrain, role, x, y)
            })
            .unwrap();

        assert_eq!(
            transcript,
            vec![
                (SparkCellSelectionRole::Ground, 256, 0),
                (SparkCellSelectionRole::Cell, 256, 0),
                (SparkCellSelectionRole::Cell, 512, 0),
            ]
        );
        assert!(facts.slope_matrix.is_none());
        assert_eq!(terrain.shared_cell_dummy().snapshot().coord, (2, 0));
    }

    #[test]
    fn gsi_04_03_candidate_real_old_dummy_retains_real_candidate_view() {
        let sim = one_cell_sim(terrain_cell(0, 0));
        let rules = empty_rules();
        let world = SparkCollisionWorld::new(&sim, &rules).unwrap();
        let terrain = sim.resolved_terrain.as_ref().unwrap();
        let motion = motion_between(IVec3::new(256, 0, 200), IVec3::new(0, 0, 200));
        let mut transcript = Vec::new();

        let facts = world
            .query_with_selector(motion, |role, x, y| {
                transcript.push((role, x, y));
                select_cell(terrain, role, x, y)
            })
            .unwrap();

        assert_eq!(
            transcript,
            vec![
                (SparkCellSelectionRole::Ground, 0, 0),
                (SparkCellSelectionRole::Cell, 0, 0),
                (SparkCellSelectionRole::Cell, 256, 0),
            ]
        );
        assert!(!facts.candidate_has_structural_bridge);
        assert_eq!(terrain.shared_cell_dummy().snapshot().coord, (1, 0));
    }

    #[test]
    fn gsi_04_03_candidate_dummy_ignores_old_real_contact_contents() {
        let rules = RuleSet::from_ini(&IniFile::from_str(
            "[BuildingTypes]\n1=Solid\n[Solid]\nFoundation=2x2\n",
        ))
        .unwrap();
        let mut sim = one_cell_sim(terrain_cell(0, 0));
        add_building(&mut sim, 1, "Solid");
        sim.overlay_grid.as_mut().unwrap().cell_mut(0, 0).overlay_id = Some(0x02);
        let world = SparkCollisionWorld::new(&sim, &rules).unwrap();
        let terrain = sim.resolved_terrain.as_ref().unwrap();
        let motion = motion_between(IVec3::new(0, 0, 100), IVec3::new(256, 0, 100));
        let (facts, transcript) = world.query_with_transcript(motion).unwrap();

        assert_eq!(
            transcript,
            vec![
                (SparkCellSelectionRole::Ground, 256, 0),
                (SparkCellSelectionRole::Cell, 256, 0),
                (SparkCellSelectionRole::Cell, 0, 0),
            ]
        );
        assert!(!facts.accepted_building);
        assert_eq!(facts.wall_overlay_id, None);
        assert!(facts.slope_matrix.is_none());
        assert_eq!(terrain.shared_cell_dummy().snapshot().coord, (1, 0));
    }

    #[test]
    fn gsi_04_03_candidate_real_keeps_wall_after_old_dummy_restamp() {
        let mut sim = one_cell_sim(terrain_cell(0, 0));
        sim.overlay_grid.as_mut().unwrap().cell_mut(0, 0).overlay_id = Some(0x02);
        let rules = empty_rules();
        let world = SparkCollisionWorld::new(&sim, &rules).unwrap();
        let terrain = sim.resolved_terrain.as_ref().unwrap();
        let motion = motion_between(IVec3::new(256, 0, 100), IVec3::new(0, 0, 100));
        let (facts, transcript) = world.query_with_transcript(motion).unwrap();

        assert_eq!(
            transcript,
            vec![
                (SparkCellSelectionRole::Ground, 0, 0),
                (SparkCellSelectionRole::Cell, 0, 0),
                (SparkCellSelectionRole::Cell, 256, 0),
                (SparkCellSelectionRole::Slope, 0, 0),
            ]
        );
        assert!(!facts.accepted_building);
        assert_eq!(facts.wall_overlay_id, Some(0x02));
        assert!(facts.slope_matrix.is_some());
        assert_eq!(terrain.shared_cell_dummy().snapshot().coord, (1, 0));
    }

    #[test]
    fn gsi_04_03_candidate_real_keeps_building_after_old_dummy_restamp() {
        let rules = RuleSet::from_ini(&IniFile::from_str(
            "[BuildingTypes]\n1=Solid\n[Solid]\nFoundation=2x2\n",
        ))
        .unwrap();
        let mut sim = one_cell_sim(terrain_cell(0, 0));
        add_building(&mut sim, 1, "Solid");
        let world = SparkCollisionWorld::new(&sim, &rules).unwrap();
        let terrain = sim.resolved_terrain.as_ref().unwrap();
        let motion = motion_between(IVec3::new(256, 0, 100), IVec3::new(0, 0, 100));
        let (facts, transcript) = world.query_with_transcript(motion).unwrap();

        assert_eq!(
            transcript,
            vec![
                (SparkCellSelectionRole::Ground, 0, 0),
                (SparkCellSelectionRole::Cell, 0, 0),
                (SparkCellSelectionRole::Cell, 256, 0),
                (SparkCellSelectionRole::Slope, 0, 0),
            ]
        );
        assert!(facts.accepted_building);
        assert_eq!(facts.wall_overlay_id, None);
        assert!(facts.slope_matrix.is_some());
        assert_eq!(terrain.shared_cell_dummy().snapshot().coord, (1, 0));
    }

    #[test]
    fn gsi_04_03_dummy_structural_no_crossing_skips_old_and_slope_selection() {
        let mut sim = one_cell_sim(terrain_cell(0, 0));
        sim.overlay_grid = None;
        let terrain = sim.resolved_terrain.as_ref().unwrap();
        terrain
            .shared_cell_dummy()
            .set_bridge_flags_0x1180(BRIDGE_FLAG_STRUCTURAL);
        let rules = empty_rules();
        let world = SparkCollisionWorld::new(&sim, &rules).unwrap();
        let motion = motion_between(IVec3::new(0, 0, 0), IVec3::new(256, 0, 300));
        let mut transcript = Vec::new();

        let facts = world
            .query_with_selector(motion, |role, x, y| {
                transcript.push((role, x, y));
                select_cell(terrain, role, x, y)
            })
            .unwrap();

        assert!(facts.candidate_has_structural_bridge);
        assert!(!facts.old_has_structural_bridge);
        assert_eq!(
            transcript,
            vec![
                (SparkCellSelectionRole::Ground, 256, 0),
                (SparkCellSelectionRole::Cell, 256, 0),
            ]
        );
        assert!(facts.slope_matrix.is_none());
    }

    #[test]
    fn gsi_04_03_overlay_and_slope_dependencies_are_lazy() {
        let mut sim = one_cell_sim(terrain_cell(0, 0));
        sim.overlay_grid = None;
        let rules = empty_rules();
        let world = SparkCollisionWorld::new(&sim, &rules).unwrap();

        let clear = world
            .query(motion_between(IVec3::new(0, 0, 200), IVec3::new(0, 0, 200)))
            .unwrap();
        assert!(clear.slope_matrix.is_none());

        assert_eq!(
            world.query(motion_between(IVec3::new(0, 0, 100), IVec3::new(0, 0, 100),)),
            Err(SparkWorldError::MissingOverlayGrid)
        );
    }

    #[test]
    fn gsi_04_03_building_and_wall_queries_obey_native_lazy_gates() {
        let rules = RuleSet::from_ini(&IniFile::from_str(
            "[BuildingTypes]\n1=Solid\n[Solid]\nFoundation=2x2\n",
        ))
        .unwrap();

        let mut corrupt = one_cell_sim(terrain_cell(0, 0));
        corrupt.occupancy_mut().add(
            0,
            0,
            999,
            MovementLayer::Ground,
            None,
            CellListInsertion::AppendBuilding,
        );
        corrupt.overlay_grid = None;
        let world = SparkCollisionWorld::new(&corrupt, &rules).unwrap();
        assert!(
            world
                .query(motion_between(IVec3::new(0, 0, 200), IVec3::new(0, 0, 200),))
                .is_ok()
        );
        assert_eq!(
            world.query(motion_between(IVec3::new(0, 0, 100), IVec3::new(0, 0, 100),)),
            Err(SparkWorldError::MissingOccupantEntity(999))
        );

        let mut accepted = one_cell_sim(terrain_cell(0, 0));
        add_building(&mut accepted, 1, "Solid");
        accepted.overlay_grid = None;
        let facts = SparkCollisionWorld::new(&accepted, &rules)
            .unwrap()
            .query(motion_between(IVec3::new(0, 0, 100), IVec3::new(0, 0, 100)))
            .unwrap();
        assert!(facts.accepted_building);
        assert_eq!(
            facts.wall_overlay_id, None,
            "accepted building suppresses wall read"
        );
        assert!(facts.slope_matrix.is_some());
    }

    #[test]
    fn gsi_04_03_bridge_crossing_skips_contact_dependencies_and_selects_slope() {
        let mut cell = terrain_cell(0, 0);
        cell.bridge_facts.raw_flags = BRIDGE_FLAG_STRUCTURAL;
        let mut sim = one_cell_sim(cell);
        sim.occupancy_mut().add(
            0,
            0,
            999,
            MovementLayer::Ground,
            None,
            CellListInsertion::AppendBuilding,
        );
        sim.overlay_grid = None;
        let rules = empty_rules();
        let world = SparkCollisionWorld::new(&sim, &rules).unwrap();
        let motion = motion_between(IVec3::new(0, 0, 500), IVec3::new(0, 0, 100));
        let (facts, transcript) = world.query_with_transcript(motion).unwrap();

        assert_eq!(
            transcript,
            vec![
                (SparkCellSelectionRole::Ground, 0, 0),
                (SparkCellSelectionRole::Cell, 0, 0),
                (SparkCellSelectionRole::Slope, 0, 0),
            ]
        );
        assert!(facts.candidate_has_structural_bridge);
        assert!(!facts.accepted_building);
        assert_eq!(facts.wall_overlay_id, None);
        assert!(facts.slope_matrix.is_some());
    }

    #[test]
    fn gsi_04_03_null_mask_and_invalid_linear_cells_are_normal_dummy_results() {
        let mut terrain =
            ResolvedTerrainGrid::from_cells(2, 1, vec![terrain_cell(0, 0), terrain_cell(1, 0)]);
        terrain.test_set_native_allocated_cells(&[(0, 0)]);
        let mut sim = Simulation::new();
        sim.resolved_terrain = Some(terrain);
        let rules = empty_rules();
        let world = SparkCollisionWorld::new(&sim, &rules).unwrap();

        for x in [256, -256, i32::MAX] {
            let facts = world
                .query(motion_between(IVec3::new(x, 0, 200), IVec3::new(x, 0, 200)))
                .unwrap();
            assert!(facts.slope_matrix.is_none(), "x={x}");
        }
    }

    #[test]
    fn first_building_exclusion_does_not_scan_later_buildings() {
        let rules = RuleSet::from_ini(&IniFile::from_str(
            "[VehicleTypes]\n1=Vehicle\n\
             [BuildingTypes]\n1=Deployable\n2=Solid\n3=Fence\n\
             [Vehicle]\nFixtureOnly=1\n\
             [Deployable]\nUndeploysInto=Vehicle\nFoundation=1x1\n\
             [Solid]\nFoundation=2x2\n\
             [Fence]\nLaserFence=yes\nFoundation=1x1\n",
        ))
        .unwrap();

        let mut ordered = one_cell_sim(terrain_cell(0, 0));
        add_building(&mut ordered, 1, "Deployable");
        add_building(&mut ordered, 2, "Solid");
        let world = SparkCollisionWorld::new(&ordered, &rules).unwrap();
        assert_eq!(world.accepted_building(0, 0), Ok(false));

        let mut solid = one_cell_sim(terrain_cell(0, 0));
        add_building(&mut solid, 2, "Solid");
        let world = SparkCollisionWorld::new(&solid, &rules).unwrap();
        assert_eq!(world.accepted_building(0, 0), Ok(true));

        let mut fence = one_cell_sim(terrain_cell(0, 0));
        add_building(&mut fence, 3, "Fence");
        let world = SparkCollisionWorld::new(&fence, &rules).unwrap();
        assert_eq!(
            world.accepted_building(0, 0),
            Err(SparkWorldError::UnsupportedLaserFence(3))
        );
    }
}
