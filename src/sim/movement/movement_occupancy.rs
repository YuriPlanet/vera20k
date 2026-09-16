//! Movement occupancy resolution — deferred cell entry checks for crush, scatter,
//! and infantry sub-cell allocation.
//!
//! When the movement tick detects that the next cell is occupied, it defers the resolution
//! to this module (outside the mutable entity borrow) so that immutable EntityStore lookups
//! can classify blockers and decide between crush, scatter, attack, or wait.

use std::collections::{BTreeMap, BTreeSet};

use crate::map::entities::EntityCategory;
use crate::map::houses::HouseAllianceMap;
use crate::map::resolved_terrain::ResolvedTerrainGrid;
use crate::rules::locomotor_type::LocomotorKind;
use crate::sim::combat::AttackTarget;
use crate::sim::components::Position;
use crate::sim::debug_event_log::DebugEventKind;
use crate::sim::entity_store::EntityStore;
use crate::sim::movement::bump_crush;
use crate::sim::movement::drive_track::DriveTrackState;
use crate::sim::movement::locomotor::MovementLayer;
use crate::sim::movement::movement_blocked::handle_blocked_tick;
use crate::sim::occupancy::{CellOccupationGrid, OccupancyGrid};
use crate::sim::pathfinding::cell_entry::{
    self, BuildingOccupantEntryDecision, CanEnterLayerContext, CellEntryResult,
    LiveVehicleBuildingEntry, VehicleBuildingEntryBranch,
};
use crate::sim::pathfinding::terrain_cost::TerrainCostGrid;
use crate::sim::pathfinding::{BridgeTraversalInput, PathGrid};
use crate::sim::rng::SimRng;

use super::{
    MovementConfig, MovementTickStats, MoverSnapshot, PATH_STUCK_INIT, PathfindingContext,
    PendingCrushKill,
};

pub(super) type LiveBuildingEntrySkipMap = BTreeMap<(u16, u16), BTreeSet<u64>>;

const RUNTIME_CAN_ENTER_ARG5: i32 = 1;
/// Deck height above the cell's own terrain, in level indices.
///
/// `FootClass::Set_Height_On_Bridge` @ `0x005F5FA0` adds
/// `g_nFootOnBridgeDeckOffsetLeptons` `[0x00AC13BC]` to its height argument when
/// and only when the object's `+0x8C` OnBridge byte is set. That global is a
/// one-shot initializer computing `4 * LevelStep` at `0x005F3860`
/// (`LEA ECX,[EAX*4]` @ `0x005F3866`), so in level-index space the delta is 4.
/// `movement_bridge` shares this constant so the height model has exactly one
/// definition.
pub(super) const BRIDGE_DECK_LEVEL_DELTA: i16 = 4;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum DeferredCellCheck {
    Infantry((u16, u16), CanEnterLayerContext),
    Vehicle((u16, u16), CanEnterLayerContext),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct RuntimeCanEnterCellArgs {
    pub target_cell: (u16, u16),
    pub direction: i8,
    pub height: i16,
    pub parent_current_cell: Option<(u16, u16)>,
    pub arg5: i32,
}

impl RuntimeCanEnterCellArgs {
    pub(super) fn runtime(
        target_cell: (u16, u16),
        direction: i8,
        current_effective_height: i16,
    ) -> Self {
        Self {
            target_cell,
            direction,
            height: current_effective_height,
            parent_current_cell: None,
            arg5: RUNTIME_CAN_ENTER_ARG5,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct RuntimeCanEnterCellEvaluation {
    pub args: RuntimeCanEnterCellArgs,
    pub layers: CanEnterLayerContext,
    pub bridge_traversal_allowed: bool,
}

pub(super) fn runtime_can_enter_direction(current_cell: (u16, u16), target_cell: (u16, u16)) -> i8 {
    let dx = (target_cell.0 as i32 - current_cell.0 as i32).signum();
    let dy = (target_cell.1 as i32 - current_cell.1 as i32).signum();
    match (dx, dy) {
        (0, -1) => 0,
        (1, -1) => 1,
        (1, 0) => 2,
        (1, 1) => 3,
        (0, 1) => 4,
        (-1, 1) => 5,
        (-1, 0) => 6,
        (-1, -1) => 7,
        _ => -1,
    }
}

pub(super) fn runtime_current_effective_height(
    path_grid: Option<&PathGrid>,
    current_cell: (u16, u16),
    on_bridge: bool,
    fallback_z: u8,
) -> i16 {
    path_grid
        .and_then(|grid| grid.cell(current_cell.0, current_cell.1))
        .map_or(fallback_z as i16, |cell| {
            cell.signed_level()
                + if on_bridge {
                    BRIDGE_DECK_LEVEL_DELTA
                } else {
                    0
                }
        })
}

pub(super) fn runtime_can_enter_cell_args(
    path_grid: Option<&PathGrid>,
    current_cell: (u16, u16),
    target_cell: (u16, u16),
    on_bridge: bool,
    fallback_z: u8,
) -> RuntimeCanEnterCellArgs {
    RuntimeCanEnterCellArgs::runtime(
        target_cell,
        runtime_can_enter_direction(current_cell, target_cell),
        runtime_current_effective_height(path_grid, current_cell, on_bridge, fallback_z),
    )
}

pub(super) fn evaluate_runtime_can_enter_cell_with_transition(
    path_grid: Option<&PathGrid>,
    next_layer: MovementLayer,
    bridge_state: &mut super::movement_bridge::RuntimeBridgeTransitionState,
    current_on_bridge: bool,
    args: RuntimeCanEnterCellArgs,
) -> RuntimeCanEnterCellEvaluation {
    let base = CanEnterLayerContext::single(next_layer);
    let Some(grid) = path_grid else {
        return RuntimeCanEnterCellEvaluation {
            args,
            layers: base,
            bridge_traversal_allowed: true,
        };
    };
    let Some(candidate) = grid.cell(args.target_cell.0, args.target_cell.1) else {
        return RuntimeCanEnterCellEvaluation {
            args,
            layers: base,
            bridge_traversal_allowed: true,
        };
    };
    let runtime_policy = super::movement_bridge::evaluate_runtime_bridge_transition(
        bridge_state,
        candidate.has_structural_bridge(),
        current_on_bridge,
        || super::movement_bridge::RuntimeBridgePolicyResult::Continue,
    );
    if runtime_policy == super::movement_bridge::RuntimeBridgePolicyResult::Reject {
        return RuntimeCanEnterCellEvaluation {
            args,
            layers: base,
            bridge_traversal_allowed: false,
        };
    }
    let explicit_parent = args
        .parent_current_cell
        .and_then(|coord| grid.cell(coord.0, coord.1).map(|cell| (cell, coord)));
    let resolved_parent = crate::sim::pathfinding::resolve_parent_for_bridge_traversal(
        grid,
        args.target_cell,
        args.direction,
        explicit_parent,
    );

    let mut object_list_layer = if candidate.has_structural_bridge()
        && (args.height == -1 || (args.height - candidate.signed_level()).abs() >= 2)
    {
        MovementLayer::Bridge
    } else {
        MovementLayer::Ground
    };

    // Mirror the A* neighbor-expansion gate: structural→structural bridge
    // edges with no bridgehead involved (plain deck driving — ramp→body,
    // body→body) are planned WITHOUT the traversal legality check, so the
    // runtime crossing must not re-run it there either (the original's
    // locomotor relink re-checks nothing at the boundary; this evaluation is
    // a stricter-than-native safety net and must at least accept every edge
    // the plan legally produced).
    let skip_traversal_check = args.height >= 0
        && resolved_parent.is_some_and(|(parent, _)| {
            !crate::sim::pathfinding::needs_bridge_traversal_for_edge(
                args.height as u8,
                parent,
                candidate,
            )
        });

    let path_height = if skip_traversal_check {
        args.height
    } else {
        let bridge_traversal = crate::sim::pathfinding::check_bridge_traversal(
            grid,
            BridgeTraversalInput {
                candidate,
                candidate_coord: args.target_cell,
                direction: args.direction,
                path_height: args.height,
                parent: resolved_parent,
            },
        );
        if !bridge_traversal.allowed {
            return RuntimeCanEnterCellEvaluation {
                args,
                layers: base,
                bridge_traversal_allowed: false,
            };
        }
        if bridge_traversal.force_bridge_list {
            object_list_layer = MovementLayer::Bridge;
        }
        bridge_traversal.path_height
    };

    let layers = crate::sim::pathfinding::can_enter_layer_context(
        next_layer,
        object_list_layer,
        candidate,
        path_height,
    );
    RuntimeCanEnterCellEvaluation {
        args,
        layers,
        bridge_traversal_allowed: true,
    }
}

#[cfg(test)]
pub(super) fn evaluate_runtime_can_enter_cell(
    path_grid: Option<&PathGrid>,
    next_layer: MovementLayer,
    args: RuntimeCanEnterCellArgs,
) -> RuntimeCanEnterCellEvaluation {
    let mut state = super::movement_bridge::RuntimeBridgeTransitionState::default();
    evaluate_runtime_can_enter_cell_with_transition(path_grid, next_layer, &mut state, false, args)
}

pub(super) fn detect_deferred_cell_check(
    mover_category: EntityCategory,
    mover_id: u64,
    _mover_bypass_grid: bool,
    layer_context: CanEnterLayerContext,
    next_cell: (u16, u16),
    current_cell: (u16, u16),
    current_object_list_layer: MovementLayer,
    occupancy: &OccupancyGrid,
    cell_occupation: &CellOccupationGrid,
    live_building_entry_skips: &LiveBuildingEntrySkipMap,
) -> Option<DeferredCellCheck> {
    let object_list_layer = layer_context.object_list_layer;
    let occupancy_bits_layer = layer_context.occupancy_bits_layer;
    let is_self_cell = (next_cell.0, next_cell.1, object_list_layer)
        == (current_cell.0, current_cell.1, current_object_list_layer);
    if is_self_cell {
        return None;
    }

    let bit_only_block =
        cell_occupation.occupied_by_other(next_cell.0, next_cell.1, occupancy_bits_layer, mover_id);

    // Static path-grid bypass is not a live-object exception. Only the skip map
    // can suppress specific building occupants such as refinery bib pads and
    // stable-open gates while preserving later blockers in the same cell list.
    let cell_occ = occupancy.get(next_cell.0, next_cell.1);
    if mover_category == EntityCategory::Infantry {
        if bit_only_block
            || cell_occ.is_some_and(|o| {
                has_unignored_blocker_on(o, object_list_layer, next_cell, live_building_entry_skips)
                    || has_unignored_blocker_on(
                        o,
                        occupancy_bits_layer,
                        next_cell,
                        live_building_entry_skips,
                    )
                    || o.infantry(object_list_layer).next().is_some()
                    || o.infantry(occupancy_bits_layer).next().is_some()
            })
        {
            return Some(DeferredCellCheck::Infantry(next_cell, layer_context));
        }
    } else if bit_only_block
        || cell_occ.is_some_and(|o| {
            has_unignored_blocker_on(o, object_list_layer, next_cell, live_building_entry_skips)
                || o.infantry(object_list_layer).next().is_some()
                || has_unignored_blocker_on(
                    o,
                    occupancy_bits_layer,
                    next_cell,
                    live_building_entry_skips,
                )
                || o.infantry(occupancy_bits_layer).next().is_some()
        })
    {
        return Some(DeferredCellCheck::Vehicle(next_cell, layer_context));
    }

    None
}

fn has_unignored_blocker_on(
    occ: &crate::sim::occupancy::CellOccupancy,
    layer: MovementLayer,
    cell: (u16, u16),
    live_building_entry_skips: &LiveBuildingEntrySkipMap,
) -> bool {
    let ignored = live_building_entry_skips.get(&cell);
    occ.iter_layer(layer).any(|occupant| {
        occupant.sub_cell.is_none() && !ignored.is_some_and(|ids| ids.contains(&occupant.entity_id))
    })
}

pub(super) fn has_unignored_runtime_occupants_on_layers(
    occupancy: &OccupancyGrid,
    cell: (u16, u16),
    layer_context: CanEnterLayerContext,
    live_building_entry_skips: &LiveBuildingEntrySkipMap,
) -> bool {
    let Some(occ) = occupancy.get(cell.0, cell.1) else {
        return false;
    };
    has_unignored_occupant_on(
        occ,
        layer_context.object_list_layer,
        cell,
        live_building_entry_skips,
    ) || (layer_context.occupancy_bits_layer != layer_context.object_list_layer
        && has_unignored_occupant_on(
            occ,
            layer_context.occupancy_bits_layer,
            cell,
            live_building_entry_skips,
        ))
}

fn has_unignored_occupant_on(
    occ: &crate::sim::occupancy::CellOccupancy,
    layer: MovementLayer,
    cell: (u16, u16),
    live_building_entry_skips: &LiveBuildingEntrySkipMap,
) -> bool {
    let ignored = live_building_entry_skips.get(&cell);
    occ.iter_layer(layer)
        .any(|occupant| !ignored.is_some_and(|ids| ids.contains(&occupant.entity_id)))
}

pub(super) fn build_live_building_entry_skip_map(
    entities: &crate::sim::entity_store::EntityStore,
    mover_id: u64,
    interner: &crate::sim::intern::StringInterner,
    rules: Option<&crate::rules::ruleset::RuleSet>,
) -> LiveBuildingEntrySkipMap {
    let Some(rules) = rules else {
        return LiveBuildingEntrySkipMap::new();
    };
    let Some(mover) = entities.get(mover_id) else {
        return LiveBuildingEntrySkipMap::new();
    };
    let vehicle_row_helpers = mover.category == EntityCategory::Unit;
    let gate_helpers = matches!(
        mover.category,
        EntityCategory::Unit | EntityCategory::Infantry
    );
    if !vehicle_row_helpers && !gate_helpers {
        return LiveBuildingEntrySkipMap::new();
    }

    let mut skips = LiveBuildingEntrySkipMap::new();
    for building in entities.values() {
        // A Dying building corpse no longer offers gate/bunker/bib entry cells.
        if building.dying || !building.lifecycle.cell_marked {
            continue;
        }
        if building.category != EntityCategory::Structure {
            continue;
        }
        let Some(obj) = rules.object(interner.resolve(building.type_ref())) else {
            continue;
        };
        let gate_skip = gate_helpers
            && obj.gate
            && building
                .building_gate
                .is_some_and(|state| state.can_garrison_passable());
        let infantry_entry_target = mover.category == EntityCategory::Infantry
            && (mover.capture_target == Some(building.stable_id())
                || mover
                    .c4_plant
                    .is_some_and(|plant| plant.target_building_id == building.stable_id()));
        let has_contact = vehicle_row_helpers && mover.has_live_contact_with(building.stable_id());
        let has_vehicle_exception =
            vehicle_row_helpers && (has_contact || obj.unit_repair || obj.bunker || obj.bib);
        if !has_vehicle_exception && !gate_skip && !infantry_entry_target {
            continue;
        }
        let is_bunker_occupied = obj.bunker
            && (building.bunker_occupant.is_some()
                || building
                    .passenger_role
                    .cargo()
                    .is_some_and(|cargo| cargo.count() > 0));
        let foundation_cells = crate::sim::production::building_base_foundation_cells(
            building.position.rx,
            building.position.ry,
            &obj.foundation,
        );
        let foundation_cell_set: BTreeSet<(u16, u16)> = foundation_cells.iter().copied().collect();
        for (cx, cy) in foundation_cells {
            let (contact_skip, second_callsite_skip) = if vehicle_row_helpers {
                let input = LiveVehicleBuildingEntry {
                    mover_category: mover.category,
                    branch: VehicleBuildingEntryBranch::RadioContact {
                        mover_has_contact: has_contact,
                    },
                    checked_building_id: building.stable_id(),
                    candidate_building_id: Some(building.stable_id()),
                    candidate_x: cx,
                    building_origin_x: building.position.rx,
                    number_impassable_rows: obj.number_impassable_rows,
                    is_unit_repair: obj.unit_repair,
                    is_bunker: obj.bunker,
                    bunker_occupied: is_bunker_occupied,
                };
                (
                    cell_entry::decide_live_vehicle_building_entry(input),
                    cell_entry::decide_live_vehicle_building_entry(LiveVehicleBuildingEntry {
                        branch: VehicleBuildingEntryBranch::UnitRepairOrBunker,
                        ..input
                    }),
                )
            } else {
                (
                    BuildingOccupantEntryDecision::KeepBlocker,
                    BuildingOccupantEntryDecision::KeepBlocker,
                )
            };
            let bib_skip = vehicle_row_helpers
                && obj.bib
                && cx
                    .checked_add(1)
                    .is_some_and(|east_x| !foundation_cell_set.contains(&(east_x, cy)));
            if matches!(contact_skip, BuildingOccupantEntryDecision::SkipBlocker)
                || matches!(
                    second_callsite_skip,
                    BuildingOccupantEntryDecision::SkipBlocker
                )
                || bib_skip
                || gate_skip
                || infantry_entry_target
            {
                skips
                    .entry((cx, cy))
                    .or_default()
                    .insert(building.stable_id());
            }
        }
    }
    skips
}

pub(super) fn snap_motion_to_cell_center(
    position: &mut Position,
    drive_track: &mut Option<DriveTrackState>,
) {
    position.sub_x = crate::util::lepton::CELL_CENTER_LEPTON;
    position.sub_y = crate::util::lepton::CELL_CENTER_LEPTON;
    *drive_track = None;
}

pub(super) fn naval_terrain_diag(
    terrain: Option<&ResolvedTerrainGrid>,
    cell: (u16, u16),
) -> String {
    let Some(terrain) = terrain else {
        return "terrain=<none>".into();
    };
    let Some(t) = terrain.cell(cell.0, cell.1) else {
        return format!("terrain=OOB({},{})", cell.0, cell.1);
    };
    format!(
        "terrain[water={} land_type={} cliff={} overlay_blocks={} terrain_blocks={} bridge_walkable={} bridge_deck={} level={}]",
        t.is_water,
        t.land_type,
        t.is_cliff_like,
        t.overlay_blocks,
        t.terrain_object_blocks,
        t.bridge_walkable,
        t.bridge_deck_level,
        t.level,
    )
}

pub(super) fn naval_occ_diag(
    occupancy: &OccupancyGrid,
    layer: MovementLayer,
    cell: (u16, u16),
) -> String {
    match occupancy.get(cell.0, cell.1) {
        Some(occ) => format!(
            "occ[blockers={} infantry={}]",
            occ.blockers(layer).count(),
            occ.infantry(layer).count(),
        ),
        None => "occ[empty]".into(),
    }
}

fn bridge_marker_context_with_peers<'a>(
    context: crate::sim::movement::path_markers::BridgeMarkerContext<'a>,
    peers: &'a crate::sim::movement::path_markers::BridgeMarkerPeerSnapshot,
) -> crate::sim::movement::path_markers::BridgeMarkerContext<'a> {
    crate::sim::movement::path_markers::BridgeMarkerContext { peers, ..context }
}

/// Handle the deferred occupancy check — runs outside the mutable entity borrow
/// so `classify_occupied_cell()` can do immutable EntityStore lookups for blocker
/// properties. Returns debug events to be pushed onto the entity.
#[allow(clippy::too_many_arguments)]
pub(super) fn handle_deferred_occupancy(
    entities: &mut EntityStore,
    check: DeferredCellCheck,
    entity_id: u64,
    snap: &MoverSnapshot,
    active_layer: MovementLayer,
    ctx: PathfindingContext<'_>,
    mcfg: MovementConfig,
    entity_cost_grid: Option<&TerrainCostGrid>,
    mover_entity_blocks: Option<&BTreeSet<(u16, u16)>>,
    mover_entity_block_map: Option<&crate::sim::pathfinding::LayeredEntityBlockMap>,
    occupancy: &mut OccupancyGrid,
    cell_occupation: &mut CellOccupationGrid,
    live_building_entry_skips: &LiveBuildingEntrySkipMap,
    alliances: &HouseAllianceMap,
    path_grid: Option<&PathGrid>,
    resolved_terrain: Option<&ResolvedTerrainGrid>,
    rng: &mut SimRng,
    stats: &mut MovementTickStats,
    finished_entities: &mut Vec<u64>,
    crush_kills: &mut Vec<PendingCrushKill>,
    already_scattered: &mut BTreeSet<u64>,
    sim_tick: u64,
    interner: &crate::sim::intern::StringInterner,
    rules: Option<&crate::rules::ruleset::RuleSet>,
    marker_context: Option<crate::sim::movement::path_markers::BridgeMarkerContext<'_>>,
    slave_bindings: Option<&std::collections::BTreeMap<u64, Vec<u64>>>,
) -> (Vec<(u32, DebugEventKind)>, bool) {
    let mut debug_events: Vec<(u32, DebugEventKind)> = Vec::new();
    let (nx, ny, layer_context) = match check {
        DeferredCellCheck::Infantry((nx, ny), layers)
        | DeferredCellCheck::Vehicle((nx, ny), layers) => (nx, ny, layers),
    };
    let object_list_layer = layer_context.object_list_layer;
    let occupancy_bits_layer = layer_context.occupancy_bits_layer;
    let mover_loco_kind = snap
        .locomotor
        .as_ref()
        .map_or(LocomotorKind::Drive, |l| l.kind);
    let crush_capability = snap.crush_capability();
    let mover_is_crusher = crush_capability.can_crush_units();
    let is_infantry = snap.category == EntityCategory::Infantry;
    if let Some(rules) = rules {
        crate::sim::gate_runtime::request_gate_open_for_cell(
            entities,
            occupancy,
            (nx, ny),
            object_list_layer,
            entity_id,
            interner.resolve(snap.owner),
            rules,
            alliances,
            interner,
        );
    }
    let slave_query = rules
        .zip(resolved_terrain)
        .zip(slave_bindings)
        .filter(|_| is_infantry)
        .map(
            |((rules, terrain), bindings)| crate::sim::slave_deposit::SlaveDepositQuery {
                entities,
                bindings,
                occupancy,
                terrain,
                rules,
                interner,
            },
        );
    let entry_result = cell_entry::classify_occupied_cell_with_occupation_and_slave_query(
        (nx, ny),
        layer_context,
        entity_id,
        crush_capability,
        interner.resolve(snap.owner),
        mover_loco_kind,
        snap.bypass_grid,
        live_building_entry_skips.get(&(nx, ny)),
        occupancy,
        cell_occupation,
        entities,
        alliances,
        interner,
        slave_query.as_ref(),
    );
    if snap.movement_zone.is_water_mover() {
        log::info!(
            "NAVAL occupancy block: entity={} cur=({},{}) next=({},{}) object_layer={:?} occupancy_layer={:?} result={:?} blocked_delay={} path_blocked={} {} {}",
            entity_id,
            entities.get(entity_id).map(|e| e.position.rx).unwrap_or(nx),
            entities.get(entity_id).map(|e| e.position.ry).unwrap_or(ny),
            nx,
            ny,
            object_list_layer,
            occupancy_bits_layer,
            entry_result,
            entities
                .get(entity_id)
                .map(|e| e
                    .navigation
                    .path_runtime
                    .blocked_timer
                    .remaining(mcfg.binary_frame as i32))
                .unwrap_or(0),
            entities
                .get(entity_id)
                .map(|e| e.navigation.path_runtime.path_blocked)
                .unwrap_or(false),
            naval_terrain_diag(resolved_terrain, (nx, ny)),
            naval_occ_diag(occupancy, occupancy_bits_layer, (nx, ny)),
        );
    }

    // BACKED OUT — the full-infantry-cell force-scatter used to live here.
    //
    // The instruction range it was built on is real: the infantry per-cell entry
    // body does test the three infantry occupancy bits and then call the cell
    // scatter with force = 1. But the whole block is dominated by an earlier
    // byte test on the infantryman himself, and every path into that block goes
    // through it — when the byte is zero the code jumps clean past the scatter
    // call. That byte is the radio tether: cleared by the `TechnoClass`
    // constructor, raised on one radio message and cleared on its partner. So
    // the original force-scatters a cell only for a *tethered* infantryman, and
    // VERA models no tether term at all.
    //
    // It was also re-sited. The cell the original scatters is the man's OWN
    // cell, resolved at the top of the per-cell entry body, so his own sub-cell
    // bit is one of the three that satisfy the test. VERA tested the
    // DESTINATION cell of a blocked step and explicitly excluded the arriver —
    // a fourth man stopped before entry, which is a different situation.
    //
    // Consequence of leaving it in: any ordinary infantryman routed into a cell
    // already holding three force-scattered all of them and drew from the
    // scenario RNG where the original makes no draw, bounded only by a
    // VERA-invented wait. That is deterministic state, and it fires on every
    // squad funnelling through a chokepoint. Re-siting it faithfully means
    // moving it to the arrival body and modelling the tether byte first, so the
    // clause is out until both exist rather than half-gated.

    let accepted = matches!(&entry_result, CellEntryResult::Clear);
    match entry_result {
        CellEntryResult::Clear => {
            // Locomotor override (JumpJet) can clear a lower native code before
            // we reach this branch.
            if let Some(entity) = entities.get_mut(entity_id) {
                if mover_loco_kind != LocomotorKind::Walk {
                    snap_motion_to_cell_center(&mut entity.position, &mut entity.drive_track);
                }
                entity.navigation.path_runtime.start_blocked(
                    mcfg.binary_frame,
                    0,
                    mover_loco_kind == LocomotorKind::Walk,
                );
                entity.navigation.path_runtime.path_blocked = false;
            }
        }
        CellEntryResult::ScatterRequired { .. } => {
            // Native code 3 is a soft blocked result for allied gate/building
            // contact. The opener request has already been issued above; the
            // mover must wait/repath instead of entering this occupied cell.
            if let Some(entity) = entities.get_mut(entity_id) {
                if mover_loco_kind != LocomotorKind::Walk {
                    snap_motion_to_cell_center(&mut entity.position, &mut entity.drive_track);
                }
                let cur_pos = (entity.position.rx, entity.position.ry);
                let body_facing = entity.body_facing;
                if let Some(ref mut target) = entity.movement_target {
                    let mut aborted_for_stuck = false;
                    let evts = handle_blocked_tick(
                        &mut entity.navigation.path_replay,
                        target,
                        &mut entity.navigation.path_runtime,
                        &mut entity.facing,
                        body_facing,
                        &snap.locomotor,
                        &mut entity.drive_locomotion,
                        &mut entity.ship_locomotion,
                        entity_id,
                        cur_pos,
                        active_layer,
                        snap.on_bridge,
                        stats,
                        finished_entities,
                        &mut aborted_for_stuck,
                        ctx,
                        entity_cost_grid,
                        mover_entity_blocks,
                        mover_entity_block_map,
                        snap.too_big_to_fit_under_bridge,
                        mcfg,
                        rng,
                        sim_tick,
                        PATH_STUCK_INIT,
                        mover_is_crusher,
                        is_infantry,
                        snap.allow_zone_hierarchy,
                        false,
                        true,
                        marker_context,
                        occupancy,
                    );
                    debug_events.extend(evts);
                }
            }
        }
        CellEntryResult::Crushable { victims } => {
            let crusher_cell = (i32::from(nx), i32::from(ny));
            let crusher_lepton = (i32::from(nx) * 256 + 128, i32::from(ny) * 256 + 128);
            let eligibility = bump_crush::ScatterEligibility::from_rules(rules);
            // The entering-cell scatter walks the whole selected cell list, not
            // just the crushable subset, and it runs before any kill filter.
            let cell_occupants = occupancy
                .get(nx, ny)
                .map_or_else(Vec::new, |occ| occ.snapshot_layer(object_list_layer));
            let victims = match bump_crush::classify_drive_crush_phase(
                bump_crush::DriveCrushPhase::FullyInCell,
                &victims,
                entities,
                entity_id,
                alliances,
                interner,
                crusher_lepton,
                crush_capability,
                eligibility,
                sim_tick as u32,
            ) {
                bump_crush::DriveCrushOutcome::Kill { victims } => victims,
                _ => Vec::new(),
            };
            let kill_set: BTreeSet<u64> = victims.iter().copied().collect();
            if let bump_crush::DriveCrushOutcome::Scatter { blockers } =
                bump_crush::classify_drive_crush_phase(
                    bump_crush::DriveCrushPhase::EnteringCell,
                    &cell_occupants,
                    entities,
                    entity_id,
                    alliances,
                    interner,
                    crusher_lepton,
                    crush_capability,
                    eligibility,
                    sim_tick as u32,
                )
            {
                for blocker_id in blockers {
                    if kill_set.contains(&blocker_id) {
                        continue;
                    }
                    if !already_scattered.contains(&blocker_id)
                        && bump_crush::scatter_blocker(
                            entities,
                            blocker_id,
                            path_grid,
                            resolved_terrain,
                            occupancy,
                            object_list_layer,
                            rng,
                            rules,
                            interner,
                            crate::sim::movement::DestinationTiming::new(
                                mcfg.binary_frame,
                                mcfg.blockage_path_delay_ticks,
                            ),
                        )
                    {
                        already_scattered.insert(blocker_id);
                        stats.scatter_successes = stats.scatter_successes.saturating_add(1);
                    }
                }
            }
            // Remove crush victims from occupancy immediately (matches gamemd's
            // PerCellProcess which calls RemoveFromGame before continuing).
            for &vid in &victims {
                let victim_cell = entities
                    .get(vid)
                    .map(|victim| (victim.position.rx, victim.position.ry));
                if let Some((rx, ry)) = victim_cell {
                    occupancy.remove(rx, ry, vid);
                    if let Some(victim) = entities.get_mut(vid) {
                        // UNCHECKED: movement still owns this pre-UnInit unmark
                        // until the unified per-object scheduler can place the
                        // exact native Mark(0) boundary.
                        if let Some(drive) = victim.drive_locomotion.as_mut() {
                            crate::sim::occupancy::clear_drive_head_to_occupation_for_remove(
                                drive,
                                cell_occupation,
                                vid,
                            );
                        }
                        victim.lifecycle.cell_marked = false;
                        cell_occupation.reconcile_entity(victim);
                    }
                }
            }
            crush_kills.extend(victims.into_iter().map(|victim_id| PendingCrushKill {
                victim_id,
                crusher_id: entity_id,
                crush_coord: crusher_cell,
            }));
            if let Some(entity) = entities.get_mut(entity_id) {
                if mover_loco_kind != LocomotorKind::Walk {
                    snap_motion_to_cell_center(&mut entity.position, &mut entity.drive_track);
                }
                entity.navigation.path_runtime.start_blocked(
                    mcfg.binary_frame,
                    0,
                    mover_loco_kind == LocomotorKind::Walk,
                );
                entity.navigation.path_runtime.path_blocked = false;
            }
        }
        CellEntryResult::FriendlyStationary { blocker_id } => {
            // An allied occupant that is NOT moving and IS a building is a hard
            // block in gamemd: the ally-and-not-moving branch of
            // `UnitClass::Can_Enter_Cell` returns 7 outright before the running
            // code can be raised to 6. Code 7 gets no scatter attempt and no
            // BlockagePathDelay grace — the mover stops and repaths at once.
            let blocker_is_structure = entities
                .get(blocker_id)
                .is_some_and(|blocker| blocker.category == EntityCategory::Structure);
            // Scatter the stationary friendly blocker out of the way.
            // Matches original engine: CellClass::Scatter_Objects with force=1
            // tells the BLOCKER to move, not the mover. The blocker receives a
            // movement command to walk to an adjacent cell.
            let mut scattered = false;
            if !blocker_is_structure && !already_scattered.contains(&blocker_id) {
                scattered = bump_crush::scatter_blocker(
                    entities,
                    blocker_id,
                    path_grid,
                    resolved_terrain,
                    occupancy,
                    object_list_layer,
                    rng,
                    rules,
                    interner,
                    crate::sim::movement::DestinationTiming::new(
                        mcfg.binary_frame,
                        mcfg.blockage_path_delay_ticks,
                    ),
                );
                if scattered {
                    already_scattered.insert(blocker_id);
                    stats.scatter_successes = stats.scatter_successes.saturating_add(1);
                }
            }
            // Mover waits — blocker is walking away. If scatter failed,
            // fall through to handle_blocked_tick for repath.
            if let Some(entity) = entities.get_mut(entity_id) {
                if mover_loco_kind != LocomotorKind::Walk {
                    snap_motion_to_cell_center(&mut entity.position, &mut entity.drive_track);
                }
                let cur_pos = (entity.position.rx, entity.position.ry);
                let body_facing = entity.body_facing;
                if let Some(ref mut target) = entity.movement_target {
                    if scattered {
                        // Blocker is walking away. The original writes its
                        // hardcoded 10-frame post-scatter wait here, not
                        // `[AI] BlockagePathDelay`, which is a separate timer.
                        if mover_loco_kind != LocomotorKind::Walk
                            && !entity.navigation.path_runtime.path_blocked
                        {
                            entity.navigation.path_runtime.path_blocked = true;
                            entity.navigation.path_runtime.start_blocked(
                                mcfg.binary_frame,
                                bump_crush::POST_SCATTER_WAIT_FRAMES,
                                mover_loco_kind == LocomotorKind::Walk,
                            );
                        }
                        // Walk code 6 returns after CellScatter (0x75B891),
                        // without installing Drive's fixed ten-frame wait.
                    } else {
                        let mut aborted_for_stuck = false;
                        let evts = handle_blocked_tick(
                            &mut entity.navigation.path_replay,
                            target,
                            &mut entity.navigation.path_runtime,
                            &mut entity.facing,
                            body_facing,
                            &snap.locomotor,
                            &mut entity.drive_locomotion,
                            &mut entity.ship_locomotion,
                            entity_id,
                            cur_pos,
                            active_layer,
                            snap.on_bridge,
                            stats,
                            finished_entities,
                            &mut aborted_for_stuck,
                            ctx,
                            entity_cost_grid,
                            mover_entity_blocks,
                            mover_entity_block_map,
                            snap.too_big_to_fit_under_bridge,
                            mcfg,
                            rng,
                            sim_tick,
                            PATH_STUCK_INIT,
                            mover_is_crusher,
                            is_infantry,
                            snap.allow_zone_hierarchy,
                            // Allied building occupant is native code 7 (no
                            // grace); any other stationary friendly keeps the
                            // code-2 grace.
                            blocker_is_structure,
                            true,
                            marker_context,
                            occupancy,
                        );
                        debug_events.extend(evts);
                    }
                }
            }
        }
        CellEntryResult::OccupiedEnemy { blocker_id } => {
            // Native cell-entry class 5, body arm: the next step is blocked by
            // an object the mover is not allied with, so the mover stops and
            // fights it. The Override archives the destination and the current
            // target, so once something releases the mover — the target dies,
            // or detaches alive — it goes back to the order it was carrying.
            //
            // Class 5 also covers an *enemy wall* natively, and that arm targets
            // a cell rather than an object, which neither detach sweep can see;
            // an object overridden onto a wall would strand. It cannot arrive
            // here: `OccupiedEnemy` is built at exactly one place, inside the
            // blocker classifier, which has already resolved a live entity, and
            // `FriendlyWall` has no producer at all — not because VERA
            // ignores walls (the `cell_rect` wall gate is live and
            // MovementZone-keyed) but because that gate answers a hard block
            // where native answers 4 or 5. So this site is the body arm and only
            // the body arm. A future wall-overlay producer must NOT route into
            // this arm without a Restore path for cell targets.
            //
            // TWO LOCOMOTORS TAKE THIS ARM IN THE ORIGINAL: Walk and HOVER. An
            // exhaustive search of the Override vtable slot returns ten call
            // sites. Walk owns two of them (a blocking *object* and a wall).
            // Hover's movement processor `FUN_00514F70` — reached from
            // `HoverLocomotionClass__Move` 0x00514499/0x00514636 and
            // `__SpeedUpdate` 0x00516309 — has its own `case 4: case 5:` pair:
            // the object arm runs `CellClass__Find_Blocking_Object` 0x00515BB8
            // -> `HouseClass__Is_Ally_ByObject` 0x00515BEF -> the Override at
            // 0x00515C2C, and the wall arm sits at 0x00515C9C. Walk's run the
            // same way: object at 0x0075BAEB, wall at 0x0075BB49 — both
            // locomotors are object-first in address order. The claim that Drive
            // and ship "have no blocking-object arm at all, so a blocked tank
            // still repaths rather than overriding" was false and is corrected
            // here (2026-09-16, from the binary): `0x004B3A97` splits codes 4/5
            // out of the shared entry, and the arm at `0x004B3B03` runs
            // `CellClass::Find_Blocking_Object 0x0047C5A0` ->
            // `Is_Ally_ByObject 0x004F9A90` -> `+0x1F4(1, object)` exactly as
            // Walk and Hover do, falling to the wall-cell `+0x1F4(1, cell)` at
            // `0x004B3B94` only when no object is found. Drive therefore owns
            // two Override sites, not one. VERA still routes a blocked vehicle
            // to a repath — that is the unported half of ledger row I9b, a
            // recorded gap rather than native behaviour.
            //
            // Hover is live stock, not dead data. `Locomotor={4A582742-…}` has
            // exactly four uncommented users in rulesmd.ini: [ROBO] the Robot
            // Tank, and three transports — [LCRF] Landing Craft, [SAPC] Armored
            // Transport, [YHVR] Hover Transport Yuri. (The Floating Disc is
            // [DISK] and uses the Jumpjet locomotor, so it never reaches this
            // arm.) Gating on Walk alone left all four repathing where retail
            // stops them, fights the blocker, and Restores the order afterwards
            // — losing the whole Suspend/Restore pair this row owns.
            //
            // The Mech locomotor `FUN_005B01C0` has the arm too and is
            // deliberately excluded: no CLSID installs it, so it is dormant.
            //
            // The two arms are NOT step-for-step identical, and the tail below
            // describes Walk only. DRIFT, recorded not fixed: both Hover
            // Override sites return straight to the function epilogue —
            // 0x00515C32 and the epilogue after 0x00515C9C — with no
            // path clear, no speed zero and no `Stop_Moving`. Hover reaches an
            // equivalent stop by another route: every live caller passes
            // `param_2 = 1`, so `case 4/5` first zeroes the speed fraction, sets
            // `+0x5E0 = 0xFFFFFFFF`, calls the repath helper `FUN_005164D0`
            // (which runs `FootClass__Find_Path`), and only then recurses with
            // `param_2 = 0` to reach the Override. So a hovering mover ends the
            // tick with a freshly computed path where a walking one ends with
            // none, and VERA gives Hover the walk tail — a VERA-internal stop
            // for that locomotor. Trigger: a Robot Tank or one of the three
            // hover transports blocked by an enemy. Player effect: it re-paths a
            // tick later than retail once the fight resolves. Frequency: only
            // those four types, so occasional rather than constant — Robot Tanks
            // are the one combat user. Downstream risk: none — the once-per-block
            // Override is identical on both, and both end with no committed
            // destination.
            //
            // After the body, the original falls straight into its "not class 1,
            // not class 7" tail and RETURNS: it clears the stored path array,
            // drives the applied speed fraction to zero, and calls the walk
            // locomotor's own Stop_Moving. Together with the Override's NULL
            // destination that leaves the mover with no destination and no path,
            // so the walk step cannot re-enter this arm — the Override fires
            // ONCE per block. Reproducing that stop is what makes the trigger
            // safe: without it VERA re-entered every tick, and because a second
            // Override with an empty queue archives the *current* mission, tick
            // two overwrote the archived Move with Attack and every later
            // Restore returned the unit to Attack instead of its order. The
            // clobber is native and must not be gated away; the re-entry is the
            // defect.
            //
            // Class 5 also covers an *enemy wall* natively, and that arm targets
            // a cell rather than an object, which neither detach sweep can see;
            // an object overridden onto a wall would strand. It cannot arrive
            // here: `OccupiedEnemy` is built at exactly one place, inside the
            // blocker classifier, which has already resolved a live entity, and
            // `FriendlyWall` has no producer at all — not because VERA
            // ignores walls (the `cell_rect` wall gate is live and
            // MovementZone-keyed) but because that gate answers a hard block
            // where native answers 4 or 5. So this site is the body arm and only
            // the body arm. A future wall-overlay producer must NOT route into
            // this arm without a Restore path for cell targets.
            if matches!(mover_loco_kind, LocomotorKind::Walk | LocomotorKind::Hover) {
                crate::sim::mission::authority::override_mission_on_blocked_step(
                    entities, alliances, interner, entity_id, blocker_id,
                );
                if let Some(entity) = entities.get_mut(entity_id) {
                    if mover_loco_kind != LocomotorKind::Walk {
                        snap_motion_to_cell_center(&mut entity.position, &mut entity.drive_track);
                    }
                }
                // The tail. `finalize_finished_entities` is VERA's single stop
                // path: it drops the path executor (the stored path array), puts
                // the locomotor back in its idle phase and returns the drive
                // runtime — including its speed fraction — to rest.
                if !finished_entities.contains(&entity_id) {
                    finished_entities.push(entity_id);
                }
            } else if let Some(entity) = entities.get_mut(entity_id) {
                if mover_loco_kind != LocomotorKind::Walk {
                    snap_motion_to_cell_center(&mut entity.position, &mut entity.drive_track);
                }
                if entity.attack_target.is_none() {
                    entity.attack_target = Some(AttackTarget::new(blocker_id));
                }
                let cur_pos = (entity.position.rx, entity.position.ry);
                let body_facing = entity.body_facing;
                if let Some(ref mut target) = entity.movement_target {
                    let mut aborted_for_stuck = false;
                    let evts = handle_blocked_tick(
                        &mut entity.navigation.path_replay,
                        target,
                        &mut entity.navigation.path_runtime,
                        &mut entity.facing,
                        body_facing,
                        &snap.locomotor,
                        &mut entity.drive_locomotion,
                        &mut entity.ship_locomotion,
                        entity_id,
                        cur_pos,
                        active_layer,
                        snap.on_bridge,
                        stats,
                        finished_entities,
                        &mut aborted_for_stuck,
                        ctx,
                        entity_cost_grid,
                        mover_entity_blocks,
                        mover_entity_block_map,
                        snap.too_big_to_fit_under_bridge,
                        mcfg,
                        rng,
                        sim_tick,
                        PATH_STUCK_INIT,
                        mover_is_crusher,
                        is_infantry,
                        snap.allow_zone_hierarchy,
                        false, // enemy blocker (code-5): keep code-2-style grace
                        true,
                        marker_context,
                        occupancy,
                    );
                    debug_events.extend(evts);
                }
            }
        }
        temporary @ (CellEntryResult::TemporaryBlock { .. }
        | CellEntryResult::TemporaryOccupation) => {
            let blocker_id = match temporary {
                CellEntryResult::TemporaryBlock { blocker_id } => Some(blocker_id),
                CellEntryResult::TemporaryOccupation => None,
                _ => unreachable!(),
            };
            // Drive moving-friendly response: scatter the BLOCKER, then wait.
            // Walk bypasses that block below and only waits/repaths.
            // Original Drive locomotor calls CellClass::Scatter_Objects with
            // force=1 regardless of whether blocker is moving or stationary,
            // and writes its 10-frame wait into the mover *immediately after*
            // that call. So the nudge comes first and the wait is what follows
            // it; arming the wait before the first scatter would delay the first
            // nudge by the whole wait, which has no source in the original.
            let mut has_target = false;
            let mut grace_expired = false;
            let mut first_block = false;
            if let Some(entity) = entities.get_mut(entity_id) {
                if mover_loco_kind != LocomotorKind::Walk {
                    snap_motion_to_cell_center(&mut entity.position, &mut entity.drive_track);
                }
                if entity.movement_target.is_some() {
                    first_block = !entity.navigation.path_runtime.path_blocked;
                    if mover_loco_kind != LocomotorKind::Walk {
                        entity.navigation.path_runtime.path_blocked = true;
                    }
                    has_target = true;
                    grace_expired = entity
                        .navigation
                        .path_runtime
                        .blocked_timer
                        .expired(mcfg.binary_frame as i32);
                }
            }
            if has_target {
                // The grace timer selects the repath URGENCY; it never
                // suppresses the repath. gamemd's code-2 dispatch falls through
                // to `FootClass::Find_Path(dest, 0, expired ? 2 : 1)` on every
                // tick the movement-delay rate limiter allows, and that limiter
                // is permanently open for Drive movers (the only writers of the
                // tick field are the FootClass constructor and
                // Set_Destination_Internal, both of which store zero). Only the
                // blocker scatter and the peer refresh below wait for the timer.
                let mut refreshed_marker_peers = None;
                // Walk ProcessMovement 0x75B8A0..0x75B9F9 (code 2) only waits/repaths.
                // The blocker scatter and ten-frame wait below belong to Drive.
                // Native timer cases: tools/infantry_scatter_oracle.py.
                if mover_loco_kind != LocomotorKind::Walk && (first_block || grace_expired) {
                    // Scatter first — on the tick the block is detected, and
                    // again once the wait has run out.
                    if let Some(blocker_id) = blocker_id
                        && !already_scattered.contains(&blocker_id)
                    {
                        let scattered = bump_crush::scatter_blocker(
                            entities,
                            blocker_id,
                            path_grid,
                            resolved_terrain,
                            occupancy,
                            object_list_layer,
                            rng,
                            rules,
                            interner,
                            crate::sim::movement::DestinationTiming::new(
                                mcfg.binary_frame,
                                mcfg.blockage_path_delay_ticks,
                            ),
                        );
                        if scattered {
                            already_scattered.insert(blocker_id);
                            stats.scatter_successes = stats.scatter_successes.saturating_add(1);
                        }
                    }
                    if let Some(entity) = entities.get_mut(entity_id) {
                        // The wait the original writes right after the scatter
                        // call, on EVERY pass through the block: the store sits
                        // straight-line after the scatter with no branch between
                        // them. Re-arming does not pin the repath urgency at 1 —
                        // the wait expires again after its full span, so urgency
                        // escalates to 2 once per span exactly as it does in the
                        // original. Gating this on entry instead left the timer
                        // at zero forever once it first expired, which made the
                        // blocker scatter — and its scenario-stream draw — fire
                        // every tick instead of once per span.
                        entity.navigation.path_runtime.start_blocked(
                            mcfg.binary_frame,
                            bump_crush::POST_SCATTER_WAIT_FRAMES,
                            mover_loco_kind == LocomotorKind::Walk,
                        );
                    }
                    // Retail reads peer paths immediately before A*. Refresh
                    // only at this seam because the scatter attempt above is
                    // the sole mutation between the tick snapshot and this
                    // immediate blocked repath.
                    refreshed_marker_peers = marker_context.map(|_| {
                        crate::sim::movement::path_markers::snapshot_bridge_marker_peers(
                            entities, rules, interner,
                        )
                    });
                }
                let effective_marker_context = match refreshed_marker_peers.as_ref() {
                    Some(peers) => marker_context
                        .map(|context| bridge_marker_context_with_peers(context, peers)),
                    None => marker_context,
                };
                // Re-borrow the mover since scatter_blocker and the live
                // marker snapshot both released it.
                if let Some(entity) = entities.get_mut(entity_id) {
                    let cur_pos = (entity.position.rx, entity.position.ry);
                    let body_facing = entity.body_facing;
                    if let Some(ref mut target) = entity.movement_target {
                        let mut aborted_for_stuck = false;
                        let evts = handle_blocked_tick(
                            &mut entity.navigation.path_replay,
                            target,
                            &mut entity.navigation.path_runtime,
                            &mut entity.facing,
                            body_facing,
                            &snap.locomotor,
                            &mut entity.drive_locomotion,
                            &mut entity.ship_locomotion,
                            entity_id,
                            cur_pos,
                            active_layer,
                            snap.on_bridge,
                            stats,
                            finished_entities,
                            &mut aborted_for_stuck,
                            ctx,
                            entity_cost_grid,
                            mover_entity_blocks,
                            mover_entity_block_map,
                            snap.too_big_to_fit_under_bridge,
                            mcfg,
                            rng,
                            sim_tick,
                            PATH_STUCK_INIT,
                            mover_is_crusher,
                            is_infantry,
                            snap.allow_zone_hierarchy,
                            false, // temp block (moving friendly): keep grace
                            // Native code 2 has no CloseEnough give-up.
                            false,
                            effective_marker_context,
                            occupancy,
                        );
                        debug_events.extend(evts);
                    }
                }
            }
        }
        CellEntryResult::FriendlyWall | CellEntryResult::Impassable => {
            // Shouldn't reach here from NeedsBlockerCheck, but handle gracefully.
            if let Some(entity) = entities.get_mut(entity_id) {
                if mover_loco_kind != LocomotorKind::Walk {
                    snap_motion_to_cell_center(&mut entity.position, &mut entity.drive_track);
                }
                let cur_pos = (entity.position.rx, entity.position.ry);
                let body_facing = entity.body_facing;
                if let Some(ref mut target) = entity.movement_target {
                    let mut aborted_for_stuck = false;
                    let evts = handle_blocked_tick(
                        &mut entity.navigation.path_replay,
                        target,
                        &mut entity.navigation.path_runtime,
                        &mut entity.facing,
                        body_facing,
                        &snap.locomotor,
                        &mut entity.drive_locomotion,
                        &mut entity.ship_locomotion,
                        entity_id,
                        cur_pos,
                        active_layer,
                        snap.on_bridge,
                        stats,
                        finished_entities,
                        &mut aborted_for_stuck,
                        ctx,
                        entity_cost_grid,
                        mover_entity_blocks,
                        mover_entity_block_map,
                        snap.too_big_to_fit_under_bridge,
                        mcfg,
                        rng,
                        sim_tick,
                        PATH_STUCK_INIT,
                        mover_is_crusher,
                        is_infantry,
                        snap.allow_zone_hierarchy,
                        true, // wall/impassable (code-7): skip grace
                        true,
                        marker_context,
                        occupancy,
                    );
                    debug_events.extend(evts);
                }
            }
        }
    }

    (debug_events, accepted)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sim::components::MovementTarget;
    use crate::sim::game_entity::GameEntity;
    use crate::sim::intern::test_interner;
    use crate::sim::occupancy::{CellListInsertion, RawCellOccupationGrid};

    #[test]
    fn gsi_04_12_marker_deferred_repath_refreshes_same_tick_scatter_path() {
        let ini = crate::rules::ini_parser::IniFile::from_str(
            "[VehicleTypes]\n0=MOVER\n1=PEER\n\
             [MOVER]\nName=Mover\nSpeed=8\n\
             [PEER]\nName=Peer\nSpeed=4\n",
        );
        let rules = crate::rules::ruleset::RuleSet::from_ini(&ini).expect("marker rules");
        let mut entities = EntityStore::new();

        let mut mover = GameEntity::test_default(1, "MOVER", "Americans", 5, 5);
        mover.category = EntityCategory::Unit;
        mover.facing = 0;
        entities.insert(mover);

        let mut blocker = GameEntity::test_default(2, "PEER", "Americans", 5, 4);
        blocker.category = EntityCategory::Unit;
        blocker.movement_target = Some(MovementTarget {
            path: vec![(5, 4), (6, 4)],
            path_layers: vec![MovementLayer::Ground; 2],
            next_index: 1,
            ..MovementTarget::default()
        });
        entities.insert(blocker);
        let interner = test_interner();

        let mut occupancy = OccupancyGrid::new();
        occupancy.add(
            5,
            4,
            2,
            MovementLayer::Ground,
            None,
            CellListInsertion::PrependNonBuilding,
        );
        let grid = PathGrid::new(12, 12);
        let raw_occupation = RawCellOccupationGrid::new();
        let playfield = crate::sim::cell_rect::PlayfieldBounds {
            base: 0,
            off_fc: -32,
            off_100: -32,
            off_104: 64,
            off_108: 64,
        };
        let stale_peers = crate::sim::movement::path_markers::snapshot_bridge_marker_peers(
            &entities,
            Some(&rules),
            &interner,
        );
        let stale_context = crate::sim::movement::path_markers::BridgeMarkerContext {
            enabled: true,
            peers: &stale_peers,
            raw_occupation: &raw_occupation,
            grid: &grid,
            terrain: None,
            playfield_bounds: Some(playfield),
            native_frame: 0,
        };

        let stale_search = stale_context.build(&occupancy, 1, (5, 5), 0, None, false, 1);
        assert_eq!(
            stale_search.effective_urgency, 0,
            "the old one-direction path cannot satisfy the Unit peer gate"
        );

        // Model the live path replacement this seam must observe. The current
        // TemporaryBlock classifier makes a successful scatter mutation
        // unreachable, so write the same live field directly without changing
        // scatter semantics just to manufacture the test trigger.
        entities.get_mut(2).unwrap().movement_target = Some(MovementTarget {
            path: vec![(5, 4), (4, 4), (3, 4)],
            path_layers: vec![MovementLayer::Ground; 3],
            next_index: 1,
            ..MovementTarget::default()
        });
        let refreshed_peers = crate::sim::movement::path_markers::snapshot_bridge_marker_peers(
            &entities,
            Some(&rules),
            &interner,
        );
        let refreshed_context = bridge_marker_context_with_peers(stale_context, &refreshed_peers);
        let refreshed_search = refreshed_context.build(&occupancy, 1, (5, 5), 0, None, false, 1);

        assert_eq!(refreshed_search.effective_urgency, 1);
        assert!(refreshed_search.overlay.contains((4, 4)));
        assert!(refreshed_search.overlay.contains((3, 4)));
        assert!(!refreshed_search.overlay.contains((6, 4)));
    }

    #[test]
    fn runtime_layers_keep_bridge_object_list_with_ground_occupancy_bits() {
        let mut grid = PathGrid::new(2, 1);
        grid.set_cell_for_test(0, 0, 4, true, false);
        grid.set_cell_for_test(1, 0, 0, true, true);

        let eval = evaluate_runtime_can_enter_cell(
            Some(&grid),
            MovementLayer::Bridge,
            RuntimeCanEnterCellArgs::runtime((1, 0), 2, 0),
        );
        let layers = eval.layers;

        assert!(eval.bridge_traversal_allowed);
        assert_eq!(layers.terrain_layer, MovementLayer::Bridge);
        assert_eq!(layers.object_list_layer, MovementLayer::Bridge);
        assert_eq!(layers.occupancy_bits_layer, MovementLayer::Ground);
    }

    #[test]
    fn runtime_layers_resnapshot_bridge_occupancy_bits_at_deck_height() {
        let mut grid = PathGrid::new(2, 1);
        grid.set_cell_for_test(0, 0, 4, true, false);
        grid.set_cell_for_test(1, 0, 0, true, true);

        let eval = evaluate_runtime_can_enter_cell(
            Some(&grid),
            MovementLayer::Bridge,
            RuntimeCanEnterCellArgs::runtime((1, 0), 2, 4),
        );
        let layers = eval.layers;

        assert!(eval.bridge_traversal_allowed);
        assert_eq!(layers.object_list_layer, MovementLayer::Bridge);
        assert_eq!(layers.occupancy_bits_layer, MovementLayer::Bridge);
    }

    #[test]
    fn runtime_height_uses_current_cell_level_plus_on_bridge() {
        let mut grid = PathGrid::new(1, 1);
        grid.set_cell_for_test(0, 0, 2, false, false);

        assert_eq!(
            runtime_current_effective_height(Some(&grid), (0, 0), true, 99),
            6
        );
        assert_eq!(
            runtime_current_effective_height(Some(&grid), (0, 0), false, 99),
            2
        );
    }

    #[test]
    fn runtime_null_parent_reconstructs_predecessor_from_target_and_direction() {
        let mut grid = PathGrid::new(3, 1);
        grid.set_cell_for_test(0, 0, 0, false, false);
        grid.set_cell_for_test(1, 0, 4, true, false);
        grid.set_cell_for_test(2, 0, 0, true, true);

        let runtime_null_parent = evaluate_runtime_can_enter_cell(
            Some(&grid),
            MovementLayer::Bridge,
            RuntimeCanEnterCellArgs::runtime((2, 0), 2, 0),
        );
        let explicit_wrong_parent = evaluate_runtime_can_enter_cell(
            Some(&grid),
            MovementLayer::Bridge,
            RuntimeCanEnterCellArgs {
                parent_current_cell: Some((0, 0)),
                ..RuntimeCanEnterCellArgs::runtime((2, 0), 2, 0)
            },
        );

        assert!(runtime_null_parent.bridge_traversal_allowed);
        assert_eq!(
            runtime_null_parent.layers.object_list_layer,
            MovementLayer::Bridge
        );
        assert!(explicit_wrong_parent.bridge_traversal_allowed);
        assert_eq!(
            explicit_wrong_parent.layers.object_list_layer,
            MovementLayer::Ground
        );
    }

    #[test]
    fn deferred_detection_uses_occupancy_bits_layer_not_object_list_layer() {
        let mut occupancy = OccupancyGrid::new();
        occupancy.add(
            1,
            0,
            10,
            MovementLayer::Ground,
            None,
            CellListInsertion::PrependNonBuilding,
        );

        let layers = CanEnterLayerContext {
            terrain_layer: MovementLayer::Bridge,
            object_list_layer: MovementLayer::Bridge,
            occupancy_bits_layer: MovementLayer::Ground,
        };
        let check = detect_deferred_cell_check(
            EntityCategory::Unit,
            1,
            false,
            layers,
            (1, 0),
            (0, 0),
            MovementLayer::Ground,
            &occupancy,
            &CellOccupationGrid::new(),
            &LiveBuildingEntrySkipMap::new(),
        );

        assert_eq!(check, Some(DeferredCellCheck::Vehicle((1, 0), layers)));
    }

    #[test]
    fn deferred_detection_uses_object_list_layer_for_selected_blockers() {
        let mut occupancy = OccupancyGrid::new();
        occupancy.add(
            5,
            5,
            10,
            MovementLayer::Bridge,
            None,
            CellListInsertion::PrependNonBuilding,
        );

        let layers = CanEnterLayerContext {
            terrain_layer: MovementLayer::Bridge,
            object_list_layer: MovementLayer::Bridge,
            occupancy_bits_layer: MovementLayer::Ground,
        };
        let check = detect_deferred_cell_check(
            EntityCategory::Unit,
            1,
            false,
            layers,
            (5, 5),
            (4, 5),
            MovementLayer::Ground,
            &occupancy,
            &CellOccupationGrid::new(),
            &LiveBuildingEntrySkipMap::new(),
        );

        assert_eq!(check, Some(DeferredCellCheck::Vehicle((5, 5), layers)));
    }

    #[test]
    fn deferred_detection_ignores_only_live_skipped_building_blocker() {
        let mut occupancy = OccupancyGrid::new();
        occupancy.add(
            3,
            3,
            10,
            MovementLayer::Ground,
            None,
            CellListInsertion::AppendBuilding,
        );
        let layers = CanEnterLayerContext::single(MovementLayer::Ground);
        let mut skips = LiveBuildingEntrySkipMap::new();
        skips.entry((3, 3)).or_default().insert(10);

        let check = detect_deferred_cell_check(
            EntityCategory::Unit,
            1,
            false,
            layers,
            (3, 3),
            (2, 3),
            MovementLayer::Ground,
            &occupancy,
            &CellOccupationGrid::new(),
            &skips,
        );

        assert_eq!(check, None);
    }

    #[test]
    fn infantry_deferred_detection_ignores_only_live_skipped_gate_blocker() {
        let mut occupancy = OccupancyGrid::new();
        occupancy.add(
            3,
            3,
            10,
            MovementLayer::Ground,
            None,
            CellListInsertion::AppendBuilding,
        );
        let layers = CanEnterLayerContext::single(MovementLayer::Ground);
        let mut skips = LiveBuildingEntrySkipMap::new();
        skips.entry((3, 3)).or_default().insert(10);

        let check = detect_deferred_cell_check(
            EntityCategory::Infantry,
            1,
            false,
            layers,
            (3, 3),
            (2, 3),
            MovementLayer::Ground,
            &occupancy,
            &CellOccupationGrid::new(),
            &skips,
        );

        assert_eq!(check, None);
    }

    #[test]
    fn deferred_detection_bypass_grid_still_checks_unskipped_structure() {
        let mut occupancy = OccupancyGrid::new();
        occupancy.add(
            3,
            3,
            10,
            MovementLayer::Ground,
            None,
            CellListInsertion::AppendBuilding,
        );
        let layers = CanEnterLayerContext::single(MovementLayer::Ground);

        let check = detect_deferred_cell_check(
            EntityCategory::Unit,
            1,
            true,
            layers,
            (3, 3),
            (2, 3),
            MovementLayer::Ground,
            &occupancy,
            &CellOccupationGrid::new(),
            &LiveBuildingEntrySkipMap::new(),
        );

        assert_eq!(check, Some(DeferredCellCheck::Vehicle((3, 3), layers)));
    }

    #[test]
    fn deferred_detection_keeps_unrelated_blocker_in_live_skipped_cell() {
        let mut occupancy = OccupancyGrid::new();
        occupancy.add(
            3,
            3,
            10,
            MovementLayer::Ground,
            None,
            CellListInsertion::AppendBuilding,
        );
        occupancy.add(
            3,
            3,
            20,
            MovementLayer::Ground,
            None,
            CellListInsertion::PrependNonBuilding,
        );
        let layers = CanEnterLayerContext::single(MovementLayer::Ground);
        let mut skips = LiveBuildingEntrySkipMap::new();
        skips.entry((3, 3)).or_default().insert(10);

        let check = detect_deferred_cell_check(
            EntityCategory::Unit,
            1,
            false,
            layers,
            (3, 3),
            (2, 3),
            MovementLayer::Ground,
            &occupancy,
            &CellOccupationGrid::new(),
            &skips,
        );

        assert_eq!(check, Some(DeferredCellCheck::Vehicle((3, 3), layers)));
    }

    #[test]
    fn refinery_live_skip_map_opens_bib_east_edge_not_interior() {
        use crate::rules::ini_parser::IniFile;
        use crate::rules::ruleset::RuleSet;
        use crate::sim::entity_store::EntityStore;
        use crate::sim::game_entity::GameEntity;

        let ini = IniFile::from_str(
            "[VehicleTypes]\n0=HARV\n[BuildingTypes]\n0=GAREFN\n\
             [HARV]\nName=Harvester\nSpeed=4\n\
             [GAREFN]\nName=Refinery\nFoundation=4x3\nBib=yes\nNumberImpassableRows=3\n",
        );
        let rules = RuleSet::from_ini(&ini).expect("refinery rules");
        let mut entities = EntityStore::new();
        let mut mover = GameEntity::test_default(1, "HARV", "Americans", 14, 11);
        mover.category = EntityCategory::Unit;
        entities.insert(mover);
        let mut refinery = GameEntity::test_default(100, "GAREFN", "Americans", 10, 10);
        refinery.category = EntityCategory::Structure;
        refinery.lifecycle.in_limbo = false;
        refinery.lifecycle.cell_marked = true;
        entities.insert(refinery);
        let interner = crate::sim::intern::test_interner();

        let skips = build_live_building_entry_skip_map(&entities, 1, &interner, Some(&rules));

        assert!(skips.get(&(13, 11)).is_some_and(|ids| ids.contains(&100)));
        assert!(!skips.get(&(12, 11)).is_some_and(|ids| ids.contains(&100)));
    }

    #[test]
    fn refinery_contact_number_rows_opens_first_clear_column_only() {
        use crate::rules::ini_parser::IniFile;
        use crate::rules::ruleset::RuleSet;
        use crate::sim::entity_store::EntityStore;
        use crate::sim::game_entity::GameEntity;

        let ini = IniFile::from_str(
            "[VehicleTypes]\n0=HARV\n[BuildingTypes]\n0=GAREFN\n\
             [HARV]\nName=Harvester\nSpeed=4\n\
             [GAREFN]\nName=Refinery\nFoundation=4x3\nBib=no\nNumberImpassableRows=3\n",
        );
        let rules = RuleSet::from_ini(&ini).expect("refinery rules");
        let mut entities = EntityStore::new();
        let mut mover = GameEntity::test_default(1, "HARV", "Americans", 14, 11);
        mover.category = EntityCategory::Unit;
        mover.mark_live_contact_with(100);
        entities.insert(mover);
        let mut refinery = GameEntity::test_default(100, "GAREFN", "Americans", 10, 10);
        refinery.category = EntityCategory::Structure;
        refinery.lifecycle.in_limbo = false;
        refinery.lifecycle.cell_marked = true;
        entities.insert(refinery);
        let interner = crate::sim::intern::test_interner();

        let skips = build_live_building_entry_skip_map(&entities, 1, &interner, Some(&rules));

        assert!(skips.get(&(13, 11)).is_some_and(|ids| ids.contains(&100)));
        assert!(!skips.get(&(12, 11)).is_some_and(|ids| ids.contains(&100)));
    }

    #[test]
    fn open_gate_skip_map_requires_mission_18_and_stable_open() {
        use crate::rules::ini_parser::IniFile;
        use crate::rules::ruleset::RuleSet;
        use crate::sim::entity_store::EntityStore;
        use crate::sim::game_entity::{BuildingGatePhase, BuildingGateRuntime, GameEntity};

        let ini = IniFile::from_str(
            "[VehicleTypes]\n0=MTNK\n[BuildingTypes]\n0=GAGATE_A\n\
             [MTNK]\nName=Tank\nSpeed=4\n\
             [GAGATE_A]\nName=Allied Gate\nFoundation=3x1\nGate=yes\n",
        );
        let rules = RuleSet::from_ini(&ini).expect("gate rules");

        let mut entities = EntityStore::new();
        let mut mover = GameEntity::test_default(1, "MTNK", "Americans", 8, 10);
        mover.category = EntityCategory::Unit;
        entities.insert(mover);
        let mut gate = GameEntity::test_default(100, "GAGATE_A", "Americans", 10, 10);
        gate.category = EntityCategory::Structure;
        gate.lifecycle.in_limbo = false;
        gate.lifecycle.cell_marked = true;
        gate.building_gate = Some(BuildingGateRuntime {
            mission_18_active: true,
            phase: BuildingGatePhase::OpenStable,
            ..Default::default()
        });
        entities.insert(gate);
        let interner = crate::sim::intern::test_interner();

        let skips = build_live_building_entry_skip_map(&entities, 1, &interner, Some(&rules));
        assert!(skips.get(&(10, 10)).is_some_and(|ids| ids.contains(&100)));
        assert!(skips.get(&(11, 10)).is_some_and(|ids| ids.contains(&100)));
        assert!(skips.get(&(12, 10)).is_some_and(|ids| ids.contains(&100)));

        entities.get_mut(100).unwrap().building_gate = Some(BuildingGateRuntime {
            mission_18_active: true,
            phase: BuildingGatePhase::Opening,
            ..Default::default()
        });
        let skips = build_live_building_entry_skip_map(&entities, 1, &interner, Some(&rules));
        assert!(!skips.get(&(10, 10)).is_some_and(|ids| ids.contains(&100)));

        entities.get_mut(100).unwrap().building_gate = Some(BuildingGateRuntime {
            mission_18_active: false,
            phase: BuildingGatePhase::OpenStable,
            ..Default::default()
        });
        let skips = build_live_building_entry_skip_map(&entities, 1, &interner, Some(&rules));
        assert!(!skips.get(&(10, 10)).is_some_and(|ids| ids.contains(&100)));
    }

    #[test]
    fn infantry_uses_open_gate_skip_without_vehicle_row_helpers() {
        use crate::rules::ini_parser::IniFile;
        use crate::rules::ruleset::RuleSet;
        use crate::sim::entity_store::EntityStore;
        use crate::sim::game_entity::{BuildingGatePhase, BuildingGateRuntime, GameEntity};

        let ini = IniFile::from_str(
            "[InfantryTypes]\n0=E1\n[BuildingTypes]\n0=GAGATE_A\n\
             [E1]\nName=GI\nSpeed=4\n\
             [GAGATE_A]\nName=Allied Gate\nFoundation=3x1\nGate=yes\nNumberImpassableRows=0\n",
        );
        let rules = RuleSet::from_ini(&ini).expect("gate rules");
        let mut entities = EntityStore::new();
        let mut mover = GameEntity::test_default(1, "E1", "Americans", 8, 10);
        mover.category = EntityCategory::Infantry;
        entities.insert(mover);
        let mut gate = GameEntity::test_default(100, "GAGATE_A", "Americans", 10, 10);
        gate.category = EntityCategory::Structure;
        gate.lifecycle.in_limbo = false;
        gate.lifecycle.cell_marked = true;
        gate.building_gate = Some(BuildingGateRuntime {
            mission_18_active: true,
            phase: BuildingGatePhase::OpenStable,
            ..Default::default()
        });
        entities.insert(gate);
        let interner = crate::sim::intern::test_interner();

        let skips = build_live_building_entry_skip_map(&entities, 1, &interner, Some(&rules));

        assert!(skips.get(&(10, 10)).is_some_and(|ids| ids.contains(&100)));
    }

    #[test]
    fn empty_tank_bunker_is_passable_occupied_blocks() {
        use crate::rules::ini_parser::IniFile;
        use crate::rules::ruleset::RuleSet;
        use crate::sim::entity_store::EntityStore;
        use crate::sim::game_entity::GameEntity;

        let ini = IniFile::from_str(
            "[VehicleTypes]\n0=TANK\n[BuildingTypes]\n0=NATBNK\n\
             [TANK]\nName=Tank\nSpeed=6\n\
             [NATBNK]\nName=Tank Bunker\nFoundation=1x1\nBunker=yes\nNumberImpassableRows=0\n",
        );
        let rules = RuleSet::from_ini(&ini).expect("bunker rules");
        let mut entities = EntityStore::new();
        let mut mover = GameEntity::test_default(1, "TANK", "Americans", 14, 11);
        mover.category = EntityCategory::Unit;
        entities.insert(mover);
        let mut bunker = GameEntity::test_default(100, "NATBNK", "Americans", 10, 10);
        bunker.category = EntityCategory::Structure;
        bunker.lifecycle.in_limbo = false;
        bunker.lifecycle.cell_marked = true;
        entities.insert(bunker);
        let interner = crate::sim::intern::test_interner();

        // Empty bunker: the footprint cell is row-exempt (vehicle exception),
        // so a vehicle may path through it.
        let skips = build_live_building_entry_skip_map(&entities, 1, &interner, Some(&rules));
        assert!(
            skips.get(&(10, 10)).is_some_and(|ids| ids.contains(&100)),
            "empty bunker footprint is passable"
        );

        // Occupied bunker: the gate that was previously dead (bunker_occupant
        // read-but-never-set) is now live — the footprint blocks again.
        entities.get_mut(100).unwrap().bunker_occupant = Some(1);
        let skips = build_live_building_entry_skip_map(&entities, 1, &interner, Some(&rules));
        assert!(
            !skips.get(&(10, 10)).is_some_and(|ids| ids.contains(&100)),
            "occupied bunker footprint blocks"
        );
    }
}
