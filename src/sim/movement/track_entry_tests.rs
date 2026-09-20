//! Common runtime predicate integration, not fresh/chain response policy.
//! Native wall accumulation/caller evidence: .local/track-entry-native.txt;
//! the bounded Unit tail corpus lives in tools/spatial_oracle/cell_entry_crush_tail.
//! These fixtures exercise the production adapters, not full CanEnter parity.

use super::*;
use crate::map::overlay_types::OverlayTypeRegistry;
use crate::map::resolved_terrain::{ResolvedTerrainCell, zone_class};
use crate::rules::ini_parser::IniFile;
use crate::rules::terrain_rules::LandType;
use crate::sim::game_entity::GameEntity;
use crate::sim::intern::{test_intern, test_interner};
use crate::sim::movement::locomotor::LocomotorState;
use crate::sim::movement::movement_occupancy::{
    LiveBuildingEntrySkipMap, RuntimeCanEnterCellArgs,
    evaluate_runtime_can_enter_cell_with_transition,
};
use crate::sim::occupancy::CellListInsertion;
use crate::sim::overlay_grid::OverlayGrid;
use crate::sim::superweapon::invulnerability::{InvulnKind, InvulnerabilityState};

const MOVER: u64 = 1;
const CELL: (u16, u16) = (1, 1);

struct Fixture {
    entities: EntityStore,
    occupancy: OccupancyGrid,
    identity: CellOccupationGrid,
    raw: RawCellOccupationGrid,
    skips: LiveBuildingEntrySkipMap,
    alliances: HouseAllianceMap,
    terrain: ResolvedTerrainGrid,
    grid: PathGrid,
    overlays: OverlayGrid,
    registry: OverlayTypeRegistry,
    snapshot: MoverSnapshot,
}

impl Fixture {
    fn new(kind: LocomotorKind) -> Self {
        let mut mover = GameEntity::test_default(MOVER, "MOVER", "Americans", 0, 1);
        mover.category = EntityCategory::Unit;
        mover.lifecycle.object_alive = true;
        mover.lifecycle.in_limbo = false;
        let mut locomotor = LocomotorState::for_test_kind(kind);
        locomotor.movement_zone = if kind == LocomotorKind::Ship {
            MovementZone::Water
        } else {
            MovementZone::Normal
        };
        locomotor.speed_type = if kind == LocomotorKind::Ship {
            SpeedType::Float
        } else {
            SpeedType::Track
        };
        mover.locomotor = Some(locomotor);
        let mut entities = EntityStore::new();
        entities.insert(mover);
        let mut snapshot = snapshot_mover(&entities, MOVER, None, None, None).unwrap();
        // This predicate fixture supplies already-resolved weapon facts. The
        // production snapshot's type/weapon resolution is owned by its caller.
        snapshot.is_armed = true;
        snapshot.warhead_wall = true;
        let cells = (0..3)
            .flat_map(|y| {
                (0..3).map(move |x| {
                    let mut cell = ResolvedTerrainCell::clear_for_test(x, y);
                    cell.speed_costs.track = Some(100);
                    cell.speed_costs.float = Some(100);
                    if kind == LocomotorKind::Ship {
                        cell.zone_type = zone_class::WATER;
                        cell.is_water = true;
                        cell.ground_walk_blocked = true;
                        cell.land_type = LandType::Water.as_index();
                        cell.yr_cell_land_type = LandType::Water.as_index();
                    }
                    cell
                })
            })
            .collect();
        let terrain = ResolvedTerrainGrid::from_cells(3, 3, cells);
        let grid = PathGrid::from_resolved_terrain(&terrain);
        Self {
            entities,
            occupancy: OccupancyGrid::new(),
            identity: CellOccupationGrid::new(),
            raw: RawCellOccupationGrid::new(),
            skips: LiveBuildingEntrySkipMap::new(),
            alliances: HouseAllianceMap::new(),
            terrain,
            grid,
            overlays: OverlayGrid::new(3, 3),
            registry: OverlayTypeRegistry::from_ini(
                &IniFile::from_str("[OverlayTypes]\n0=GAWALL\n[GAWALL]\nWall=yes\n"),
                None,
            ),
            snapshot,
        }
    }

    fn wall(&mut self, owner: Option<&str>) {
        let cell = self.terrain.cell_mut(CELL.0, CELL.1).unwrap();
        cell.zone_type = zone_class::WALL;
        cell.overlay_zone_type = Some(zone_class::WALL);
        cell.overlay_blocks = true;
        self.grid = PathGrid::from_resolved_terrain(&self.terrain);
        let overlay = self.overlays.cell_mut(CELL.0, CELL.1);
        overlay.overlay_id = Some(0);
        overlay.wall_owner = owner.map(test_intern);
    }

    fn query(&self, layer: MovementLayer, height: i16) -> TrackEntryQuery {
        let entry = evaluate_runtime_can_enter_cell_with_transition(
            Some(&self.grid),
            layer,
            &mut Default::default(),
            false,
            RuntimeCanEnterCellArgs::runtime(CELL, 2, height),
        );
        assert!(entry.bridge_traversal_allowed);
        TrackEntryQuery {
            target_cell: CELL,
            layers: entry.layers,
            bridge_traversal_allowed: entry.bridge_traversal_allowed,
        }
    }

    fn classify(&self, query: TrackEntryQuery, frame: u32) -> CellEntryResult {
        let interner = test_interner();
        classify_track_entry(
            query,
            MOVER,
            &self.snapshot,
            Some(&self.grid),
            Some(&self.terrain),
            None,
            Some(cell_entry::WallArmTables {
                overlay_grid: Some(&self.overlays),
                overlay_registry: Some(&self.registry),
                alliances: Some(&self.alliances),
                interner: Some(&interner),
            }),
            &self.occupancy,
            &self.identity,
            &self.raw,
            frame,
            &self.skips,
            &self.entities,
            &self.alliances,
            &interner,
        )
    }

    fn insert(&mut self, id: u64, category: EntityCategory, owner: &str, layer: MovementLayer) {
        let mut entity = GameEntity::test_default(id, "BLOCKER", owner, CELL.0, CELL.1);
        entity.category = category;
        entity.lifecycle.object_alive = true;
        entity.lifecycle.in_limbo = false;
        entity.lifecycle.cell_marked = true;
        entity.sub_cell = (category == EntityCategory::Infantry).then_some(2);
        let sub_cell = entity.sub_cell;
        self.entities.insert(entity);
        self.occupancy.add(
            CELL.0,
            CELL.1,
            id,
            layer,
            sub_cell,
            CellListInsertion::AppendBuilding,
        );
    }
}

#[test]
fn track_entry_preserves_allied_enemy_and_unowned_wall_results_over_raw_code_two() {
    for (owner, expected) in [
        (Some("Americans"), CellEntryResult::FriendlyWall),
        (Some("Soviets"), CellEntryResult::EnemyWall),
        (None, CellEntryResult::EnemyWall),
    ] {
        let mut f = Fixture::new(LocomotorKind::Drive);
        f.wall(owner);
        f.raw.mark_ground(CELL.0, CELL.1, 0x20);
        let query = f.query(MovementLayer::Ground, 0);
        assert_eq!(f.classify(query, 100), expected);
        f.snapshot.is_armed = false;
        assert_eq!(f.classify(query, 100), CellEntryResult::Impassable);
        f.snapshot.is_armed = true;
        f.terrain
            .cell_mut(CELL.0, CELL.1)
            .unwrap()
            .speed_costs
            .track = Some(0);
        assert_eq!(f.classify(query, 100), CellEntryResult::Impassable);
    }
}

#[test]
fn track_entry_wall_accumulator_keeps_ordered_live_blockers_and_hard_refusals() {
    let mut f = Fixture::new(LocomotorKind::Drive);
    f.wall(Some("Americans"));
    f.insert(2, EntityCategory::Unit, "Soviets", MovementLayer::Ground);
    let query = f.query(MovementLayer::Ground, 0);
    assert_eq!(
        f.classify(query, 100),
        CellEntryResult::OccupiedEnemy { blocker_id: 2 }
    );
    f.wall(None);
    assert_eq!(
        f.classify(query, 100),
        CellEntryResult::EnemyWall,
        "an equal object code does not erase the earlier wall target"
    );
    f.insert(3, EntityCategory::Unit, "Americans", MovementLayer::Ground);
    assert_eq!(
        f.classify(query, 100),
        CellEntryResult::FriendlyStationary { blocker_id: 3 }
    );
    f.insert(
        4,
        EntityCategory::Structure,
        "Americans",
        MovementLayer::Ground,
    );
    assert_eq!(f.classify(query, 100), CellEntryResult::Impassable);
}

#[test]
fn track_entry_ignores_only_checked_building_and_continues_the_live_list() {
    let mut f = Fixture::new(LocomotorKind::Drive);
    f.insert(
        2,
        EntityCategory::Structure,
        "Americans",
        MovementLayer::Ground,
    );
    f.insert(3, EntityCategory::Unit, "Soviets", MovementLayer::Ground);
    let query = f.query(MovementLayer::Ground, 0);
    assert_eq!(f.classify(query, 100), CellEntryResult::Impassable);
    f.skips.entry(CELL).or_default().insert(2);
    assert_eq!(
        f.classify(query, 100),
        CellEntryResult::OccupiedEnemy { blocker_id: 3 }
    );
}

#[test]
fn track_entry_keeps_bridge_object_list_independent_from_raw_plane_and_frame() {
    let mut f = Fixture::new(LocomotorKind::Drive);
    f.snapshot.regular_crusher = true;
    f.entities.get_mut(MOVER).unwrap().regular_crusher = true;
    f.grid.set_cell_for_test(0, 1, 4, true, false);
    f.grid.set_cell_for_test(CELL.0, CELL.1, 0, true, true);
    let query = f.query(MovementLayer::Bridge, 0);
    assert_eq!(query.layers.object_list_layer, MovementLayer::Bridge);
    assert_eq!(query.layers.occupancy_bits_layer, MovementLayer::Ground);
    f.insert(
        2,
        EntityCategory::Infantry,
        "Soviets",
        MovementLayer::Bridge,
    );
    f.entities.get_mut(2).unwrap().crushable = true;
    f.insert(3, EntityCategory::Unit, "Soviets", MovementLayer::Ground);
    {
        let unit = f.entities.get_mut(3).unwrap();
        unit.crushable = true;
        unit.invulnerability = Some(InvulnerabilityState {
            start_frame: 90,
            duration_frames: 20,
            kind: InvulnKind::IronCurtain,
        });
    }
    // No object-list skip: Unit3 is outside the selected bridge list, but
    // native GetUnit(false) must find it independently in the raw-bit tail.
    f.raw.mark_deck(CELL.0, CELL.1, 0x20);
    assert_eq!(
        f.classify(query, 100),
        CellEntryResult::Crushable { victims: vec![2] }
    );
    f.raw.mark_ground(CELL.0, CELL.1, 0x20);
    assert_eq!(f.classify(query, 100), CellEntryResult::TemporaryOccupation);
    assert_eq!(
        f.classify(query, 110),
        CellEntryResult::Crushable { victims: vec![2] }
    );
    for id in [2, 3] {
        let entity = f.entities.get(id).unwrap();
        assert!(entity.lifecycle.object_alive && entity.lifecycle.cell_marked);
        assert!(!entity.lifecycle.in_limbo);
    }
    assert_eq!(f.raw.ground_bits(CELL.0, CELL.1), 0x20);
    assert_eq!(f.raw.deck_bits(CELL.0, CELL.1), 0x20);
}

#[test]
fn drive_and_ship_track_entry_share_live_raw_occupation_without_identity_claims() {
    for kind in [LocomotorKind::Drive, LocomotorKind::Ship] {
        let mut f = Fixture::new(kind);
        let query = f.query(MovementLayer::Ground, 0);
        assert_eq!(f.classify(query, 100), CellEntryResult::Clear, "{kind:?}");
        f.raw.mark_deck(CELL.0, CELL.1, 0x20);
        assert_eq!(f.classify(query, 100), CellEntryResult::Clear, "{kind:?}");
        f.raw.mark_ground(CELL.0, CELL.1, 0x20);
        assert_eq!(
            f.classify(query, 100),
            CellEntryResult::TemporaryOccupation,
            "{kind:?}"
        );
    }
}
