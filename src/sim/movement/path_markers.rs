//! Search-scoped high-bridge passability markers.
//!
//! gamemd: `PathfinderClass::UpdateBridgePassability` 0x0042ACF0 and its
//! peer lookup `PathfinderClass::FindNearbyBridgePeer` 0x0042B080. The native
//! marker is an XOR of `CellClass+0x140` bit 0x40000 on persistent cell state,
//! search-scoped because the A* success and failure tails call the function
//! again; this module keeps the same XOR parity in an owned overlay so the
//! marks cannot outlive a search. The 24-entry replay cap, the direction-8
//! tube step, the (0,0) reset when `cell+0x116` is -1, and the 5x5 occupation
//! scan that cancels an occupied probe centre all follow the native body.
//!
//! Retail `PathfinderClass::UpdateBridgePassability` XORs a temporary bit into
//! selected cells immediately before an urgency 1/2 A* search and XORs the same
//! cells again on every normal search exit.  Rust represents that transaction
//! as an owned [`SearchMarkerOverlay`], so persistent map state is never
//! mutated and cleanup is automatic when the search returns.

use std::borrow::Cow;
use std::collections::BTreeMap;

use crate::map::entities::EntityCategory;
use crate::map::resolved_terrain::ResolvedTerrainGrid;
use crate::rules::locomotor_type::LocomotorKind;
use crate::sim::cell_rect::{PlayfieldBounds, cell_is_in_playfield_height_aware};
use crate::sim::components::FootPathQueue;
use crate::sim::entity_store::{EntityStore, OtherEntities};
use crate::sim::intern::{InternedId, StringInterner};
use crate::sim::movement::FacingClass;
use crate::sim::movement::locomotor::MovementLayer;
use crate::sim::occupancy::{OccupancyGrid, RawCellOccupationGrid};
use crate::sim::pathfinding::{PathGrid, SearchMarkerOverlay};
use crate::util::direction::{DIRECTION_DELTAS, TUBE_STEP_DIRECTION};
use crate::util::lepton::GROUND_LEVEL_HEIGHT_LEPTONS;

const PEER_MARKER_REPLAY_LIMIT: usize = 24;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct BridgeMarkerPeer {
    category: EntityCategory,
    foot_derived: bool,
    locomotor_kind: Option<LocomotorKind>,
    type_ref: InternedId,
    speed: i32,
    path_start: (i16, i16),
    path_directions: Vec<u8>,
    is_at_coord_track_cell: Option<(i16, i16)>,
    is_at_coord_head_cell: (i16, i16),
    /// Full retained head Z, never resampled from a later terrain state.
    is_at_coord_head_z: Option<i32>,
    current_height_leptons: i32,
}

/// Every entity's peer facts at one moment. Production reads peers live
/// ([`LiveBridgeMarkerPeers`]); this whole-world form is what tests build by
/// hand and what debug builds compare the live reads against.
///
/// Object-list order remains authoritative in [`OccupancyGrid`]; this map is
/// only an ID-to-facts lookup and therefore cannot reorder a native list.
#[derive(Debug, Clone, Default)]
pub(super) struct BridgeMarkerPeerSnapshot {
    peers: BTreeMap<u64, BridgeMarkerPeer>,
}

/// ID-to-facts lookup behind `UpdateBridgePassability`'s object-list walks.
pub(super) trait BridgeMarkerPeerLookup {
    fn peer(&self, entity_id: u64) -> Option<Cow<'_, BridgeMarkerPeer>>;
}

impl BridgeMarkerPeerLookup for BridgeMarkerPeerSnapshot {
    fn peer(&self, entity_id: u64) -> Option<Cow<'_, BridgeMarkerPeer>> {
        self.peers.get(&entity_id).map(Cow::Borrowed)
    }
}

/// Peer facts read from the entities themselves, as the native walk reads the
/// objects on a cell's list. The mover is the exception: it is mutated during
/// its own turn, and the facts that count are the ones it had when the turn
/// began, so those are captured once ([`bridge_marker_peer`]) and answered
/// from here whether or not the mover is lifted out of `others`.
#[derive(Clone, Copy)]
pub(super) struct LiveBridgeMarkerPeers<'a> {
    pub mover_id: u64,
    pub mover: Option<&'a BridgeMarkerPeer>,
    pub others: OtherEntities<'a>,
    pub rules: Option<&'a crate::rules::ruleset::RuleSet>,
    pub interner: &'a StringInterner,
    /// Debug builds: the whole-world snapshot taken at the same moment as
    /// `mover`. Every live read must equal its entry.
    #[cfg(debug_assertions)]
    pub check: Option<&'a BridgeMarkerPeerSnapshot>,
}

impl std::fmt::Debug for LiveBridgeMarkerPeers<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LiveBridgeMarkerPeers")
            .field("mover_id", &self.mover_id)
            .finish_non_exhaustive()
    }
}

impl BridgeMarkerPeerLookup for LiveBridgeMarkerPeers<'_> {
    fn peer(&self, entity_id: u64) -> Option<Cow<'_, BridgeMarkerPeer>> {
        let peer = if entity_id == self.mover_id {
            self.mover.map(Cow::Borrowed)
        } else {
            self.others
                .get(entity_id)
                .map(|entity| Cow::Owned(peer_from_entity(entity, self.rules, self.interner)))
        };
        #[cfg(debug_assertions)]
        if let Some(check) = self.check {
            debug_assert_eq!(
                peer.as_deref(),
                check.peers.get(&entity_id),
                "live marker peer {entity_id} diverged from the turn-start snapshot"
            );
        }
        peer
    }
}

/// Where a marker search finds its peers.
#[derive(Debug, Clone, Copy)]
pub(super) enum BridgeMarkerPeers<'a> {
    Live(LiveBridgeMarkerPeers<'a>),
    #[cfg(test)]
    Snapshot(&'a BridgeMarkerPeerSnapshot),
}

impl BridgeMarkerPeerLookup for BridgeMarkerPeers<'_> {
    fn peer(&self, entity_id: u64) -> Option<Cow<'_, BridgeMarkerPeer>> {
        match self {
            Self::Live(live) => live.peer(entity_id),
            #[cfg(test)]
            Self::Snapshot(snapshot) => snapshot.peer(entity_id),
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub(super) struct BridgeMarkerMover {
    pub current_cell: (u16, u16),
    pub facing: u8,
    pub body_facing: Option<FacingClass>,
    pub on_bridge: bool,
    pub type_ref: InternedId,
    pub speed: i32,
}

#[derive(Debug, Default)]
pub(super) struct BridgeMarkerSearch {
    pub overlay: SearchMarkerOverlay,
    /// Urgency actually supplied to A*.  Retail downgrades urgency 1 to zero
    /// when no eligible peer path was replayed, before the raw-byte phase.
    pub effective_urgency: u8,
    #[cfg(test)]
    processed_peer_path: bool,
}

#[derive(Debug, Clone, Copy)]
pub(super) struct BridgeMarkerContext<'a> {
    pub enabled: bool,
    pub peers: BridgeMarkerPeers<'a>,
    pub raw_occupation: &'a RawCellOccupationGrid,
    pub grid: &'a PathGrid,
    pub terrain: Option<&'a ResolvedTerrainGrid>,
    pub playfield_bounds: Option<PlayfieldBounds>,
    pub native_frame: u32,
}

/// A marker context still missing its view of the other entities. The mover's
/// turn builds one at its start; each user attaches the entities it can read at
/// that point ([`Self::reading`]): the rest of the store while the mover is
/// lifted out of it, or the whole store when it is not. The raw occupation
/// plane arrives there too, because the turn mutates it between uses.
#[derive(Debug, Clone, Copy)]
pub(super) struct DeferredBridgeMarker<'a> {
    pub mover_id: u64,
    pub mover: Option<&'a BridgeMarkerPeer>,
    pub rules: Option<&'a crate::rules::ruleset::RuleSet>,
    pub interner: &'a StringInterner,
    #[cfg(debug_assertions)]
    pub check: Option<&'a BridgeMarkerPeerSnapshot>,
    pub grid: &'a PathGrid,
    pub terrain: Option<&'a ResolvedTerrainGrid>,
    pub playfield_bounds: Option<PlayfieldBounds>,
    pub native_frame: u32,
}

impl<'a> DeferredBridgeMarker<'a> {
    pub fn reading(
        self,
        others: OtherEntities<'a>,
        raw_occupation: &'a RawCellOccupationGrid,
    ) -> BridgeMarkerContext<'a> {
        BridgeMarkerContext {
            // PathfinderClass+0x03 is initialized to one by the process-static
            // constructor and has no active writer that clears it.
            enabled: true,
            peers: BridgeMarkerPeers::Live(LiveBridgeMarkerPeers {
                mover_id: self.mover_id,
                mover: self.mover,
                others,
                rules: self.rules,
                interner: self.interner,
                #[cfg(debug_assertions)]
                check: self.check,
            }),
            raw_occupation,
            grid: self.grid,
            terrain: self.terrain,
            playfield_bounds: self.playfield_bounds,
            native_frame: self.native_frame,
        }
    }
}

impl BridgeMarkerContext<'_> {
    pub fn build(
        self,
        occupancy: &OccupancyGrid,
        entity_id: u64,
        current_cell: (u16, u16),
        facing: u8,
        body_facing: Option<FacingClass>,
        on_bridge: bool,
        requested_urgency: u8,
    ) -> BridgeMarkerSearch {
        let Some(mover) = self.peers.peer(entity_id) else {
            return BridgeMarkerSearch {
                effective_urgency: requested_urgency,
                ..BridgeMarkerSearch::default()
            };
        };
        build_bridge_passability_search(
            self.enabled,
            &self.peers,
            occupancy,
            self.raw_occupation,
            self.grid,
            self.terrain,
            self.playfield_bounds,
            BridgeMarkerMover {
                current_cell,
                facing,
                body_facing,
                on_bridge,
                type_ref: mover.type_ref,
                speed: mover.speed,
            },
            requested_urgency,
            self.native_frame,
        )
    }
}

fn direction_from_step(from: (i16, i16), to: (u16, u16)) -> u8 {
    let dx = i32::from(to.0 as i16) - i32::from(from.0);
    let dy = i32::from(to.1 as i16) - i32::from(from.1);
    DIRECTION_DELTAS
        .iter()
        .position(|&delta| delta == (dx, dy))
        .map_or(TUBE_STEP_DIRECTION, |index| index as u8)
}

pub(super) fn install_path_replay(
    queue: &mut FootPathQueue,
    reference: (u16, u16),
    path: &[(u16, u16)],
    first_destination: usize,
) {
    let mut from = (reference.0 as i16, reference.1 as i16);
    queue.directions.clear();
    for &destination in path.iter().skip(first_destination) {
        queue
            .directions
            .push(direction_from_step(from, destination));
        from = (destination.0 as i16, destination.1 as i16);
    }
    queue.cursor = 0;
    queue.reference_cell = Some((reference.0 as i16, reference.1 as i16));
}

pub(super) fn accept_path_replay(
    queue: &mut FootPathQueue,
    endpoint: (i16, i16),
    consumed_directions: usize,
) {
    queue.reference_cell = Some(endpoint);
    consume_path_replay(queue, consumed_directions);
}

/// Accepted chain4B1DF7/6A143A and Drive tube4B1362..136E pop the queue
/// without rewriting Foot+558.
pub(super) fn consume_path_replay(queue: &mut FootPathQueue, consumed_directions: usize) {
    let cursor = usize::from(queue.cursor)
        .saturating_add(consumed_directions)
        .min(queue.directions.len());
    queue.cursor = cursor.min(u16::MAX as usize) as u16;
}

/// Walk75BD89..75BDB1 propagates a -1 head into the next word before
/// shifting. Retargeting an already-paid head must not expose its old suffix.
/// Original block comparisons: tools/spatial_oracle/walk_first_step.json.
pub(super) fn consume_walk_path_replay(queue: &mut FootPathQueue) {
    let invalidated = queue.remaining_directions().is_empty();
    consume_path_replay(queue, 1);
    if invalidated {
        queue.clear_live_head();
    }
}

/// Explicit owner abandonment, distinct from FootStop_Moving4DF0D0.
pub(super) fn exhaust_path_replay(queue: &mut FootPathQueue) {
    queue.cursor = queue.directions.len().min(u16::MAX as usize) as u16;
}

fn remaining_path_from_entity(
    entity: &crate::sim::game_entity::GameEntity,
) -> ((i16, i16), Vec<u8>) {
    let queue = &entity.navigation.path_replay;
    if let Some(reference) = queue.reference_cell {
        return (reference, queue.remaining_directions().to_vec());
    }

    let mut reference = (entity.position.rx as i16, entity.position.ry as i16);
    let path_start = reference;
    let mut directions = Vec::new();
    if let Some(target) = entity.movement_target.as_ref() {
        for &destination in target.path.iter().skip(target.next_index) {
            directions.push(direction_from_step(reference, destination));
            reference = (destination.0 as i16, destination.1 as i16);
        }
    }
    (path_start, directions)
}

fn is_at_coord_cells(
    entity: &crate::sim::game_entity::GameEntity,
) -> (Option<(i16, i16)>, (i16, i16), Option<i32>) {
    let current = super::ground_pose::position_world_coord(&entity.position);
    let Some(query) = super::at_coord::AtCoordQuery::from_entity(entity) else {
        return (
            None,
            ((current.x / 256) as i16, (current.y / 256) as i16),
            None,
        );
    };
    let (handoff, head) = query.cells();
    (handoff, head, Some(query.head_z()))
}

fn peer_from_entity(
    entity: &crate::sim::game_entity::GameEntity,
    rules: Option<&crate::rules::ruleset::RuleSet>,
    interner: &StringInterner,
) -> BridgeMarkerPeer {
    let (path_start, path_directions) = remaining_path_from_entity(entity);
    let (is_at_coord_track_cell, is_at_coord_head_cell, is_at_coord_head_z) =
        is_at_coord_cells(entity);
    let current_height_leptons = super::foot_coordinate::current_coordinate(entity).z;
    let speed = rules
        .and_then(|rules| rules.object(interner.resolve(entity.type_ref())))
        .map_or(0, |object| object.speed);
    let foot_derived = matches!(
        entity.category,
        EntityCategory::Unit | EntityCategory::Infantry | EntityCategory::Aircraft
    );
    BridgeMarkerPeer {
        category: entity.category,
        foot_derived,
        locomotor_kind: entity.locomotor.as_ref().map(|locomotor| locomotor.kind),
        type_ref: entity.type_ref(),
        speed,
        path_start,
        path_directions,
        is_at_coord_track_cell,
        is_at_coord_head_cell,
        is_at_coord_head_z,
        current_height_leptons,
    }
}

/// One entity's peer facts as they stand now: the mover's own entry, taken
/// before its turn mutates it.
pub(super) fn bridge_marker_peer(
    entities: &EntityStore,
    entity_id: u64,
    rules: Option<&crate::rules::ruleset::RuleSet>,
    interner: &StringInterner,
) -> Option<BridgeMarkerPeer> {
    entities
        .get(entity_id)
        .map(|entity| peer_from_entity(entity, rules, interner))
}

/// O(entities). Tests, and the debug-build check of the live reads.
#[cfg(any(test, debug_assertions))]
pub(super) fn snapshot_bridge_marker_peers(
    entities: &EntityStore,
    rules: Option<&crate::rules::ruleset::RuleSet>,
    interner: &StringInterner,
) -> BridgeMarkerPeerSnapshot {
    let peers = entities
        .values()
        .map(|entity| {
            (
                entity.stable_id(),
                peer_from_entity(entity, rules, interner),
            )
        })
        .collect();
    BridgeMarkerPeerSnapshot { peers }
}

fn signed_cell_add(cell: (i16, i16), delta: (i32, i32)) -> (i16, i16) {
    (
        cell.0.wrapping_add(delta.0 as i16),
        cell.1.wrapping_add(delta.1 as i16),
    )
}

fn unsigned_cell(cell: (i16, i16)) -> (u16, u16) {
    (cell.0 as u16, cell.1 as u16)
}

fn signed_ground_level(grid: &PathGrid, cell: (i16, i16)) -> i16 {
    let cell = unsigned_cell(cell);
    grid.cell(cell.0, cell.1)
        .map_or(0, |cell| cell.signed_level())
}

fn has_structural_bridge(grid: &PathGrid, cell: (i16, i16)) -> bool {
    let cell = unsigned_cell(cell);
    grid.cell(cell.0, cell.1)
        .is_some_and(|cell| cell.has_structural_bridge())
}

fn list_ids(occupancy: &OccupancyGrid, cell: (i16, i16), layer: MovementLayer) -> Vec<u64> {
    let cell = unsigned_cell(cell);
    occupancy
        .get(cell.0, cell.1)
        .map(|occupancy| {
            occupancy
                .iter_layer(layer)
                .map(|occupant| occupant.entity_id)
                .collect()
        })
        .unwrap_or_default()
}

fn find_nearby_bridge_peer_suffix(
    peers: &impl BridgeMarkerPeerLookup,
    occupancy: &OccupancyGrid,
    grid: &PathGrid,
    probe: (i16, i16),
    requested_height: i16,
) -> Vec<u64> {
    let requested_height_leptons =
        i32::from(requested_height).wrapping_mul(GROUND_LEVEL_HEIGHT_LEPTONS);
    // Retail helper nesting is dy outer, dx inner.
    for dy in -2..=2 {
        for dx in -2..=2 {
            let candidate = signed_cell_add(probe, (dx, dy));
            let candidate_level = signed_ground_level(grid, candidate);
            let layer = if has_structural_bridge(grid, candidate)
                && (candidate_level - requested_height).abs() > 2
            {
                MovementLayer::Bridge
            } else {
                MovementLayer::Ground
            };
            let list = list_ids(occupancy, candidate, layer);
            for (index, entity_id) in list.iter().copied().enumerate() {
                let Some(peer) = peers.peer(entity_id) else {
                    continue;
                };
                if !peer.foot_derived
                    || !matches!(
                        peer.locomotor_kind,
                        Some(
                            LocomotorKind::Drive
                                | LocomotorKind::Ship
                                | LocomotorKind::Walk
                                | LocomotorKind::Hover
                        )
                    )
                    || (peer.is_at_coord_track_cell != Some(probe)
                        && peer.is_at_coord_head_cell != probe)
                {
                    continue;
                }
                let receiver_height_leptons = if peer.is_at_coord_head_cell == probe {
                    peer.is_at_coord_head_z
                        .unwrap_or(peer.current_height_leptons)
                } else {
                    peer.current_height_leptons
                };
                if (receiver_height_leptons - requested_height_leptons).abs()
                    > GROUND_LEVEL_HEIGHT_LEPTONS
                {
                    continue;
                }
                // Caller receives this node and follows its +0x30 suffix only.
                return list[index..].to_vec();
            }
        }
    }
    Vec::new()
}

fn replay_peer_path(
    overlay: &mut SearchMarkerOverlay,
    peer: &BridgeMarkerPeer,
    terrain: Option<&ResolvedTerrainGrid>,
) {
    let mut replay = peer.path_start;
    for &direction in peer.path_directions.iter().take(PEER_MARKER_REPLAY_LIMIT) {
        replay = match direction {
            0..=7 => signed_cell_add(replay, DIRECTION_DELTAS[direction as usize]),
            TUBE_STEP_DIRECTION => {
                let current = unsigned_cell(replay);
                terrain
                    .and_then(|terrain| terrain.tube_at_cell(current.0, current.1))
                    .map_or((0, 0), |tube| (tube.exit.0 as i16, tube.exit.1 as i16))
            }
            _ => continue,
        };
        overlay.toggle(unsigned_cell(replay));
    }
}

#[allow(clippy::too_many_arguments)]
pub(super) fn build_bridge_passability_search(
    enabled: bool,
    peers: &impl BridgeMarkerPeerLookup,
    occupancy: &OccupancyGrid,
    raw_occupation: &RawCellOccupationGrid,
    grid: &PathGrid,
    terrain: Option<&ResolvedTerrainGrid>,
    playfield_bounds: Option<PlayfieldBounds>,
    mover: BridgeMarkerMover,
    requested_urgency: u8,
    native_frame: u32,
) -> BridgeMarkerSearch {
    if !enabled || requested_urgency == 0 {
        return BridgeMarkerSearch {
            effective_urgency: requested_urgency,
            ..BridgeMarkerSearch::default()
        };
    }

    let facing = mover
        .body_facing
        .map_or(u16::from(mover.facing) << 8, |facing| {
            facing.current(native_frame)
        });
    let direction = crate::util::direction_tables::quantize::dir_from_facing16(facing);
    let current = (mover.current_cell.0 as i16, mover.current_cell.1 as i16);
    let probe = signed_cell_add(current, DIRECTION_DELTAS[direction as usize]);
    let current_level = signed_ground_level(grid, current);
    let probe_level = signed_ground_level(grid, probe);
    let direct_deck = has_structural_bridge(grid, probe)
        && ((current_level - probe_level).abs() > 3 || mover.on_bridge);
    let direct_layer = if direct_deck {
        MovementLayer::Bridge
    } else {
        MovementLayer::Ground
    };
    let requested_height = probe_level + if direct_deck { 4 } else { 0 };
    let mut selected_list = list_ids(occupancy, probe, direct_layer);
    if selected_list.is_empty() {
        selected_list =
            find_nearby_bridge_peer_suffix(peers, occupancy, grid, probe, requested_height);
    }

    let mut overlay = SearchMarkerOverlay::new();
    let mut processed_peer_path = false;
    for entity_id in selected_list {
        let Some(peer) = peers.peer(entity_id) else {
            continue;
        };
        if !peer.foot_derived
            || !matches!(
                peer.category,
                EntityCategory::Unit | EntityCategory::Infantry
            )
        {
            continue;
        }
        if requested_urgency != 2 {
            if peer.type_ref == mover.type_ref || mover.speed <= peer.speed {
                continue;
            }
            if !cell_is_in_playfield_height_aware(
                (i32::from(peer.path_start.0), i32::from(peer.path_start.1)),
                playfield_bounds,
                terrain,
            ) {
                continue;
            }
        }
        let minimum_directions = if peer.category == EntityCategory::Infantry {
            3
        } else {
            2
        };
        if peer.path_directions.len() < minimum_directions {
            continue;
        }
        processed_peer_path = true;
        replay_peer_path(&mut overlay, &peer, terrain);
    }

    if requested_urgency == 1 && !processed_peer_path {
        return BridgeMarkerSearch {
            overlay: SearchMarkerOverlay::new(),
            effective_urgency: 0,
            #[cfg(test)]
            processed_peer_path,
        };
    }

    // Retail occupation nesting is dx outer, dy inner and always reads the
    // full ground byte, regardless of the direct-list layer selected above.
    for dx in -2..=2 {
        for dy in -2..=2 {
            let candidate = signed_cell_add(probe, (dx, dy));
            let cell = unsigned_cell(candidate);
            if candidate != current && raw_occupation.ground_is_occupied(cell.0, cell.1) {
                overlay.toggle(cell);
            }
        }
    }
    // Unconditional center toggle makes an occupied probe cancel itself.
    overlay.toggle(unsigned_cell(probe));

    BridgeMarkerSearch {
        overlay,
        effective_urgency: requested_urgency,
        #[cfg(test)]
        processed_peer_path,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sim::occupancy::CellListInsertion;

    fn peer(
        category: EntityCategory,
        type_ref: InternedId,
        speed: i32,
        start: (i16, i16),
        directions: &[u8],
    ) -> BridgeMarkerPeer {
        BridgeMarkerPeer {
            category,
            foot_derived: true,
            locomotor_kind: Some(if category == EntityCategory::Infantry {
                LocomotorKind::Walk
            } else {
                LocomotorKind::Drive
            }),
            type_ref,
            speed,
            path_start: start,
            path_directions: directions.to_vec(),
            is_at_coord_track_cell: None,
            is_at_coord_head_cell: start,
            is_at_coord_head_z: None,
            current_height_leptons: 0,
        }
    }

    fn mover(type_ref: InternedId) -> BridgeMarkerMover {
        BridgeMarkerMover {
            current_cell: (5, 5),
            facing: 0,
            body_facing: None,
            on_bridge: false,
            type_ref,
            speed: 8,
        }
    }

    fn test_playfield() -> PlayfieldBounds {
        PlayfieldBounds {
            base: 0,
            off_fc: -32,
            off_100: -32,
            off_104: 64,
            off_108: 64,
        }
    }

    #[test]
    fn gsi_04_12_marker_urgency_zero_is_inert_and_urgency_one_without_peer_downgrades() {
        let interner = StringInterner::new();
        let mover_type = interner.get("MOVER").unwrap_or_default();
        let peers = BridgeMarkerPeerSnapshot::default();
        let occupancy = OccupancyGrid::new();
        let mut raw = RawCellOccupationGrid::new();
        raw.mark_ground(5, 4, 0x20);
        let grid = PathGrid::new(12, 12);

        let zero = build_bridge_passability_search(
            true,
            &peers,
            &occupancy,
            &raw,
            &grid,
            None,
            Some(test_playfield()),
            mover(mover_type),
            0,
            0,
        );
        assert!(zero.overlay.is_empty());
        assert_eq!(zero.effective_urgency, 0);

        let one = build_bridge_passability_search(
            true,
            &peers,
            &occupancy,
            &raw,
            &grid,
            None,
            Some(test_playfield()),
            mover(mover_type),
            1,
            0,
        );
        assert!(one.overlay.is_empty(), "raw phase is skipped on downgrade");
        assert_eq!(one.effective_urgency, 0);
    }

    #[test]
    fn gsi_04_12_marker_direct_list_uses_facing_layer_and_replay_xor() {
        let mut interner = StringInterner::new();
        let mover_type = interner.intern("FAST");
        let peer_type = interner.intern("SLOW");
        let mut peers = BridgeMarkerPeerSnapshot::default();
        peers.peers.insert(
            2,
            peer(EntityCategory::Unit, peer_type, 4, (5, 4), &[2, 2, 6]),
        );
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
        let raw = RawCellOccupationGrid::new();

        let search = build_bridge_passability_search(
            true,
            &peers,
            &occupancy,
            &raw,
            &grid,
            None,
            Some(test_playfield()),
            mover(mover_type),
            1,
            0,
        );
        assert!(search.processed_peer_path);
        assert!(!search.overlay.contains((6, 4)), "duplicate visit cancels");
        assert!(search.overlay.contains((7, 4)));
        assert!(
            search.overlay.contains((5, 4)),
            "unoccupied probe center toggles"
        );
    }

    #[test]
    fn gsi_04_12_marker_fallback_returns_first_is_at_coord_list_suffix() {
        let mut interner = StringInterner::new();
        let mover_type = interner.intern("FAST");
        let rejected_type = interner.intern("SAME");
        let accepted_type = interner.intern("SLOW");
        let suffix_type = interner.intern("SLOWER");
        let mut peers = BridgeMarkerPeerSnapshot::default();
        let mut rejected = peer(EntityCategory::Unit, rejected_type, 1, (3, 2), &[2, 2]);
        rejected.is_at_coord_head_cell = (0, 0);
        peers.peers.insert(2, rejected);
        let mut accepted = peer(EntityCategory::Unit, accepted_type, 2, (5, 4), &[2, 2]);
        accepted.is_at_coord_head_cell = (5, 4);
        peers.peers.insert(3, accepted);
        peers.peers.insert(
            4,
            peer(EntityCategory::Unit, suffix_type, 3, (5, 4), &[4, 4]),
        );
        let mut occupancy = OccupancyGrid::new();
        // Probe (5,4) is empty. Candidate (3,2) is first in dy/dx order.
        for id in [4, 3, 2] {
            occupancy.add(
                3,
                2,
                id,
                MovementLayer::Ground,
                None,
                CellListInsertion::PrependNonBuilding,
            );
        }
        let grid = PathGrid::new(12, 12);
        let raw = RawCellOccupationGrid::new();
        let search = build_bridge_passability_search(
            true,
            &peers,
            &occupancy,
            &raw,
            &grid,
            None,
            Some(test_playfield()),
            mover(mover_type),
            1,
            0,
        );
        assert!(search.overlay.contains((6, 4)), "accepted peer replayed");
        assert!(search.overlay.contains((5, 6)), "same-list suffix replayed");
    }

    #[test]
    fn live_peers_read_the_others_now_and_the_mover_as_its_turn_began() {
        let interner = StringInterner::new();
        let mut entities = EntityStore::new();
        for id in [1, 2] {
            let mut entity =
                crate::sim::game_entity::GameEntity::test_default(id, "UNIT", "Americans", 4, 4);
            entity.navigation.path_replay.reference_cell = Some((4, 4));
            entity.navigation.path_replay.directions = vec![2, 2];
            entities.insert(entity);
        }
        let snapshot = snapshot_bridge_marker_peers(&entities, None, &interner);
        let mover = bridge_marker_peer(&entities, 1, None, &interner).expect("mover is stored");
        let live = |entities: &EntityStore, check| {
            [1, 2, 3].map(|id| {
                LiveBridgeMarkerPeers {
                    mover_id: 1,
                    mover: Some(&mover),
                    others: OtherEntities::whole(entities),
                    rules: None,
                    interner: &interner,
                    #[cfg(debug_assertions)]
                    check,
                }
                .peer(id)
                .map(Cow::into_owned)
            })
        };

        // Untouched world: every live read is the whole-world snapshot's entry,
        // which the debug check asserts as well.
        let read = live(&entities, Some(&snapshot));
        assert_eq!(read[0].as_ref(), snapshot.peers.get(&1));
        assert_eq!(read[1].as_ref(), snapshot.peers.get(&2));
        assert_eq!(read[2], None);

        // Both entities change. The peer is read as it is now; the mover keeps
        // the facts captured when its turn began.
        for id in [1, 2] {
            entities
                .get_mut(id)
                .unwrap()
                .navigation
                .path_replay
                .directions = vec![6, 6, 6];
        }
        let read = live(&entities, None);
        assert_eq!(read[0].as_ref().unwrap().path_directions, [2, 2]);
        assert_eq!(read[1].as_ref().unwrap().path_directions, [6, 6, 6]);

        // Lifted out of the store, the mover is still answered from its capture.
        let mut turn = entities.take_turn(1).expect("mover is stored");
        let (_, others) = turn.split();
        let lifted = LiveBridgeMarkerPeers {
            mover_id: 1,
            mover: Some(&mover),
            others,
            rules: None,
            interner: &interner,
            #[cfg(debug_assertions)]
            check: None,
        };
        assert_eq!(lifted.peer(1).unwrap().path_directions, [2, 2]);
        assert_eq!(lifted.peer(2).unwrap().path_directions, [6, 6, 6]);
    }

    #[test]
    fn gsi_04_12_marker_drive_deck_track_fallback_uses_unconsumed_handoff() {
        let mut interner = StringInterner::new();
        let mover_type = interner.intern("MOVER");
        let mut entities = EntityStore::new();
        let mut peer =
            crate::sim::game_entity::GameEntity::test_default(2, "PEER", "Americans", 4, 4);
        peer.locomotor = Some(
            crate::sim::movement::locomotor::LocomotorState::for_test_kind(LocomotorKind::Drive),
        );
        peer.position.z = 4;
        peer.on_bridge = true;
        let mut drive = crate::sim::components::DriveLocomotionRuntime::default();
        drive.head_to = Some(crate::sim::components::DriveCoord::cell(6, 3, 4));
        drive.track_valid = true;
        drive.track.turn_index = 1;
        drive.track.cursor = 12;
        peer.navigation.path_replay.reference_cell = Some((5, 4));
        peer.navigation.path_replay.directions = vec![2, 2];
        peer.drive_locomotion = Some(drive);
        // RawTrack 3 handoff point 22, transformed around head cell (6,3),
        // lies in probe cell (5,4).  A deck track deliberately owns no ground
        // occupation_head_to reservation, so that field cannot answer slot 40.
        entities.insert(peer);

        let peers = snapshot_bridge_marker_peers(&entities, None, &interner);
        let peer = peers.peers.get(&2).expect("Drive peer snapshot");
        assert_eq!(peer.is_at_coord_track_cell, Some((5, 4)));
        assert_eq!(peer.is_at_coord_head_cell, (6, 3));

        let mut occupancy = OccupancyGrid::new();
        occupancy.add(
            4,
            4,
            2,
            MovementLayer::Bridge,
            None,
            CellListInsertion::PrependNonBuilding,
        );
        let mut grid = PathGrid::new(12, 12);
        grid.set_cell_for_test(5, 4, 0, true, false);
        grid.set_cell_for_test(4, 4, 0, true, false);
        let raw = RawCellOccupationGrid::new();
        let mut bridge_mover = mover(mover_type);
        bridge_mover.on_bridge = true;
        let search = build_bridge_passability_search(
            true,
            &peers,
            &occupancy,
            &raw,
            &grid,
            None,
            Some(test_playfield()),
            bridge_mover,
            2,
            0,
        );

        assert!(search.processed_peer_path);
        assert!(search.overlay.contains((6, 4)));
        assert!(search.overlay.contains((7, 4)));
    }

    #[test]
    fn gsi_04_12_marker_hover_fallback_uses_live_head_to_and_inclusive_height() {
        let mut interner = StringInterner::new();
        let mover_type = interner.intern("MOVER");
        let hover_type = interner.intern("HOVER");
        let mut entities = EntityStore::new();
        let mut peer =
            crate::sim::game_entity::GameEntity::test_default(2, "HOVER", "Americans", 4, 3);
        peer.type_ref = hover_type;
        peer.position.z = 7;
        peer.locomotor = Some(
            crate::sim::movement::locomotor::LocomotorState::for_test_kind(LocomotorKind::Hover),
        );
        peer.locomotor
            .as_mut()
            .unwrap()
            .set_step_head(Some(crate::sim::components::DriveCoord::cell(5, 4, 104)));
        peer.movement_target = Some(crate::sim::components::MovementTarget {
            path: vec![(5, 4), (6, 4), (7, 4)],
            path_layers: vec![
                MovementLayer::Bridge,
                MovementLayer::Ground,
                MovementLayer::Ground,
            ],
            next_index: 0,
            ..crate::sim::components::MovementTarget::default()
        });
        entities.insert(peer);

        let peers = snapshot_bridge_marker_peers(&entities, None, &interner);
        let peer = peers.peers.get(&2).expect("Hover peer snapshot");
        assert_eq!(peer.is_at_coord_head_cell, (5, 4));
        assert_eq!(peer.is_at_coord_head_z, Some(104));

        let mut occupancy = OccupancyGrid::new();
        // The probe itself is empty; only the nearby list can expose Hover.
        occupancy.add(
            4,
            3,
            2,
            MovementLayer::Ground,
            None,
            CellListInsertion::PrependNonBuilding,
        );
        let flat = crate::sim::pathfinding::PathCell {
            ground_walkable: true,
            bridge_walkable: false,
            bridge_structural: false,
            bridge_marker_0x80: false,
            transition: false,
            ground_level: 0,
            bridge_deck_level: 0,
            slope_type: 0,
            tube_index: None,
            low_bridge_tube_cell: false,
        };
        let mut cells = vec![flat; 12 * 12];
        // One level is the Rust map-height equivalent of the native inclusive
        // 0x68-lepton receiver/query Z tolerance. The current Foot Z above is
        // deliberately outside it, proving Head_To Z stays paired with XY.
        cells[4 * 12 + 5] = crate::sim::pathfinding::PathCell {
            bridge_walkable: true,
            bridge_structural: true,
            transition: true,
            bridge_deck_level: 1,
            ..flat
        };
        let grid = PathGrid::from_cells(cells, 12, 12);
        let raw = RawCellOccupationGrid::new();
        let search = build_bridge_passability_search(
            true,
            &peers,
            &occupancy,
            &raw,
            &grid,
            None,
            Some(test_playfield()),
            mover(mover_type),
            1,
            0,
        );

        assert_eq!(search.effective_urgency, 1);
        assert!(search.processed_peer_path);
        assert!(search.overlay.contains((6, 4)));
        assert!(search.overlay.contains((7, 4)));
    }

    #[test]
    fn gsi_04_12_marker_idle_hover_fallback_keeps_exact_current_altitude() {
        let mut interner = StringInterner::new();
        let hover_type = interner.intern("HOVER");
        let mut entities = EntityStore::new();
        let mut peer =
            crate::sim::game_entity::GameEntity::test_default(2, "HOVER", "Americans", 5, 4);
        peer.type_ref = hover_type;
        let mut locomotor =
            crate::sim::movement::locomotor::LocomotorState::for_test_kind(LocomotorKind::Hover);
        locomotor.altitude = crate::util::fixed_math::SimFixed::from_num(120);
        peer.locomotor = Some(locomotor);
        entities.insert(peer);

        let mut occupancy = OccupancyGrid::new();
        occupancy.add(
            4,
            3,
            2,
            MovementLayer::Ground,
            None,
            CellListInsertion::PrependNonBuilding,
        );
        let grid = PathGrid::new(12, 12);
        let above_tolerance = snapshot_bridge_marker_peers(&entities, None, &interner);
        let peer = above_tolerance.peers.get(&2).expect("idle Hover snapshot");
        assert_eq!(peer.is_at_coord_head_cell, (5, 4));
        assert_eq!(peer.is_at_coord_head_z, Some(120));
        assert_eq!(peer.current_height_leptons, 120);
        assert!(
            find_nearby_bridge_peer_suffix(&above_tolerance, &occupancy, &grid, (5, 4), 0,)
                .is_empty(),
            "120-lepton idle hover height exceeds the 104-lepton tolerance"
        );

        entities
            .get_mut(2)
            .unwrap()
            .locomotor
            .as_mut()
            .unwrap()
            .altitude = crate::util::fixed_math::SimFixed::from_num(104);
        let inclusive_boundary = snapshot_bridge_marker_peers(&entities, None, &interner);
        assert_eq!(
            find_nearby_bridge_peer_suffix(&inclusive_boundary, &occupancy, &grid, (5, 4), 0,),
            vec![2],
            "the native 104-lepton boundary is inclusive"
        );
    }

    #[test]
    fn gsi_04_12_marker_urgency_two_raw_phase_cancels_occupied_probe() {
        let mut interner = StringInterner::new();
        let mover_type = interner.intern("MOVER");
        let peers = BridgeMarkerPeerSnapshot::default();
        let occupancy = OccupancyGrid::new();
        let mut raw = RawCellOccupationGrid::new();
        raw.mark_ground(5, 4, 0x01);
        raw.mark_ground(4, 4, 0x80);
        raw.mark_ground(5, 5, 0x20);
        let grid = PathGrid::new(12, 12);
        let search = build_bridge_passability_search(
            true,
            &peers,
            &occupancy,
            &raw,
            &grid,
            None,
            Some(test_playfield()),
            mover(mover_type),
            2,
            0,
        );
        assert_eq!(search.effective_urgency, 2);
        assert!(
            !search.overlay.contains((5, 4)),
            "occupied probe toggles twice"
        );
        assert!(search.overlay.contains((4, 4)));
        assert!(!search.overlay.contains((5, 5)), "mover current is skipped");
    }

    #[test]
    fn gsi_04_12_marker_direct_layer_uses_strict_level_four_or_on_bridge() {
        let mut interner = StringInterner::new();
        let mover_type = interner.intern("FAST");
        let ground_type = interner.intern("GROUND");
        let deck_type = interner.intern("DECK");
        let mut peers = BridgeMarkerPeerSnapshot::default();
        peers.peers.insert(
            2,
            peer(EntityCategory::Unit, ground_type, 2, (5, 4), &[2, 2]),
        );
        peers
            .peers
            .insert(3, peer(EntityCategory::Unit, deck_type, 2, (5, 4), &[6, 6]));
        let mut occupancy = OccupancyGrid::new();
        occupancy.add(
            5,
            4,
            2,
            MovementLayer::Ground,
            None,
            CellListInsertion::PrependNonBuilding,
        );
        occupancy.add(
            5,
            4,
            3,
            MovementLayer::Bridge,
            None,
            CellListInsertion::PrependNonBuilding,
        );
        let mut grid = PathGrid::new(12, 12);
        grid.set_cell_for_test(5, 4, 0, true, false);
        grid.set_cell_for_test(5, 5, 3, false, false);
        let raw = RawCellOccupationGrid::new();

        let strict_three = build_bridge_passability_search(
            true,
            &peers,
            &occupancy,
            &raw,
            &grid,
            None,
            Some(test_playfield()),
            mover(mover_type),
            1,
            0,
        );
        assert!(strict_three.overlay.contains((7, 4)));
        assert!(!strict_three.overlay.contains((3, 4)));

        grid.set_cell_for_test(5, 5, 4, false, false);
        let level_four = build_bridge_passability_search(
            true,
            &peers,
            &occupancy,
            &raw,
            &grid,
            None,
            Some(test_playfield()),
            mover(mover_type),
            1,
            0,
        );
        assert!(level_four.overlay.contains((3, 4)));
        assert!(!level_four.overlay.contains((7, 4)));

        grid.set_cell_for_test(5, 5, 0, false, false);
        let mut bridge_mover = mover(mover_type);
        bridge_mover.on_bridge = true;
        let on_bridge = build_bridge_passability_search(
            true,
            &peers,
            &occupancy,
            &raw,
            &grid,
            None,
            Some(test_playfield()),
            bridge_mover,
            1,
            0,
        );
        assert!(on_bridge.overlay.contains((3, 4)));
    }

    #[test]
    fn gsi_04_12_marker_urgency_gates_and_path_prerequisites_are_exact() {
        let mut interner = StringInterner::new();
        let mover_type = interner.intern("FAST");
        let other_type = interner.intern("OTHER");
        let mut peers = BridgeMarkerPeerSnapshot::default();
        peers.peers.insert(
            2,
            peer(EntityCategory::Unit, mover_type, 1, (5, 4), &[2, 2]),
        );
        peers.peers.insert(
            3,
            peer(EntityCategory::Unit, other_type, 8, (5, 4), &[4, 4]),
        );
        peers
            .peers
            .insert(4, peer(EntityCategory::Unit, other_type, 1, (5, 4), &[6]));
        peers.peers.insert(
            5,
            peer(EntityCategory::Infantry, other_type, 1, (5, 4), &[0, 0]),
        );
        let mut occupancy = OccupancyGrid::new();
        for id in 2..=5 {
            occupancy.add(
                5,
                4,
                id,
                MovementLayer::Ground,
                None,
                CellListInsertion::PrependNonBuilding,
            );
        }
        let grid = PathGrid::new(12, 12);
        let raw = RawCellOccupationGrid::new();

        let urgency_one = build_bridge_passability_search(
            true,
            &peers,
            &occupancy,
            &raw,
            &grid,
            None,
            Some(test_playfield()),
            mover(mover_type),
            1,
            0,
        );
        assert_eq!(urgency_one.effective_urgency, 0);
        assert!(!urgency_one.processed_peer_path);

        let urgency_two = build_bridge_passability_search(
            true,
            &peers,
            &occupancy,
            &raw,
            &grid,
            None,
            None,
            mover(mover_type),
            2,
            0,
        );
        assert!(urgency_two.processed_peer_path);
        assert!(
            urgency_two.overlay.contains((7, 4)),
            "same-type Unit bypassed"
        );
        assert!(
            urgency_two.overlay.contains((5, 6)),
            "equal-speed Unit bypassed"
        );
        assert!(
            !urgency_two.overlay.contains((4, 4)),
            "Unit needs dir[0] and dir[1] non-(-1) (0x0042AE90)"
        );
        assert!(
            !urgency_two.overlay.contains((5, 2)),
            "Infantry needs dir[0..2] non-(-1) (0x0042AED8)"
        );
    }

    #[test]
    fn gsi_04_12_marker_replay_caps_at_24_and_missing_tube_continues_from_zero() {
        let mut interner = StringInterner::new();
        let peer_type = interner.intern("PEER");
        let mut overlay = SearchMarkerOverlay::new();
        replay_peer_path(
            &mut overlay,
            &peer(EntityCategory::Unit, peer_type, 1, (0, 0), &vec![2; 25]),
            None,
        );
        assert!(overlay.contains((24, 0)));
        assert!(!overlay.contains((25, 0)));

        let mut tube_overlay = SearchMarkerOverlay::new();
        replay_peer_path(
            &mut tube_overlay,
            &peer(
                EntityCategory::Unit,
                peer_type,
                1,
                (8, 8),
                &[TUBE_STEP_DIRECTION, 2],
            ),
            None,
        );
        assert!(tube_overlay.contains((0, 0)));
        assert!(tube_overlay.contains((1, 0)));
    }
}
