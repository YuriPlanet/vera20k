//! Foot path replay and search-scoped high-bridge passability markers.
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
//! mutated and cleanup is automatic when the search returns. The live
//! `FootClass::Find_Path` entry prepares the same overlay for A* and both
//! finishing passes. Queue consumers remain shared across locomotors.
//! Executed native inputs/costs: `tools/spatial_oracle/astar_hills_markers.json`.

use std::borrow::Cow;
#[cfg(test)]
use std::collections::BTreeMap;

use crate::map::entities::EntityCategory;
use crate::map::resolved_terrain::ResolvedTerrainGrid;
#[cfg(test)]
use crate::rules::locomotor_type::LocomotorKind;
use crate::sim::cell_rect::{PlayfieldBounds, cell_is_in_playfield_height_aware};
use crate::sim::components::FootPathQueue;
use crate::sim::entity_store::EntityStore;
use crate::sim::intern::{InternedId, StringInterner};
use crate::sim::movement::locomotor::MovementLayer;
use crate::sim::occupancy::{OccupancyGrid, RawCellOccupationGrid};
use crate::sim::pathfinding::{PathGrid, SearchMarkerOverlay};
use crate::util::direction::{DIRECTION_DELTAS, TUBE_STEP_DIRECTION};
use crate::util::lepton::GROUND_LEVEL_HEIGHT_LEPTONS;

const PEER_MARKER_REPLAY_LIMIT: usize = 24;

#[derive(Debug, Clone)]
pub(super) struct BridgeMarkerPeer {
    category: EntityCategory,
    foot_derived: bool,
    type_ref: InternedId,
    speed: i32,
    path_start: (i16, i16),
    path_directions: Vec<u8>,
    /// Retained value projection from the canonical locomotor query owner.
    at_coord: Option<super::at_coord::AtCoordQuery>,
}

/// Synthetic peer facts for focused marker tests. Production reads only the
/// live object-list peers; this ID lookup does not change list order.
#[cfg(test)]
#[derive(Debug, Clone, Default)]
pub(super) struct BridgeMarkerPeerSnapshot {
    peers: BTreeMap<u64, BridgeMarkerPeer>,
}

/// ID-to-facts lookup behind `UpdateBridgePassability`'s object-list walks.
pub(super) trait BridgeMarkerPeerLookup {
    fn peer(&self, entity_id: u64) -> Option<Cow<'_, BridgeMarkerPeer>>;
}

#[cfg(test)]
impl BridgeMarkerPeerLookup for BridgeMarkerPeerSnapshot {
    fn peer(&self, entity_id: u64) -> Option<Cow<'_, BridgeMarkerPeer>> {
        self.peers.get(&entity_id).map(Cow::Borrowed)
    }
}

/// Peer facts read from the entities themselves, as the native walk reads the
/// objects on a cell's list. The live Foot search captures the mover once
/// at the marker boundary ([`bridge_marker_peer`]); all other peer facts are
/// resolved by the entity store's canonical stable-ID lookup.
#[derive(Clone, Copy)]
pub(super) struct LiveBridgeMarkerPeers<'a> {
    pub mover_id: u64,
    pub mover: Option<&'a BridgeMarkerPeer>,
    pub entities: &'a EntityStore,
    pub rules: Option<&'a crate::rules::ruleset::RuleSet>,
    pub interner: &'a StringInterner,
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
        if entity_id == self.mover_id {
            self.mover.map(Cow::Borrowed)
        } else {
            self.entities
                .get(entity_id)
                .map(|entity| Cow::Owned(peer_from_entity(entity, self.rules, self.interner)))
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub(super) struct BridgeMarkerMover {
    pub current_cell: (u16, u16),
    /// The body heading (`+0x388`) at the search's frame.
    pub facing: u16,
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
    pub peers: LiveBridgeMarkerPeers<'a>,
    pub raw_occupation: &'a RawCellOccupationGrid,
    pub grid: &'a PathGrid,
    pub terrain: Option<&'a ResolvedTerrainGrid>,
    pub playfield_bounds: Option<PlayfieldBounds>,
}

/// Marker input awaiting its live peer lists and raw occupation plane.
/// The Foot search attaches its immutable entity store after Mark0; no
/// whole-world fact snapshot or split entity-store view is needed.
#[derive(Debug, Clone, Copy)]
pub(super) struct DeferredBridgeMarker<'a> {
    pub mover_id: u64,
    pub mover: Option<&'a BridgeMarkerPeer>,
    pub rules: Option<&'a crate::rules::ruleset::RuleSet>,
    pub interner: &'a StringInterner,
    pub grid: &'a PathGrid,
    pub terrain: Option<&'a ResolvedTerrainGrid>,
    pub playfield_bounds: Option<PlayfieldBounds>,
}

impl<'a> DeferredBridgeMarker<'a> {
    pub fn reading(
        self,
        entities: &'a EntityStore,
        raw_occupation: &'a RawCellOccupationGrid,
    ) -> BridgeMarkerContext<'a> {
        BridgeMarkerContext {
            // PathfinderClass+0x03 is initialized to one by the process-static
            // constructor and has no active writer that clears it.
            enabled: true,
            peers: LiveBridgeMarkerPeers {
                mover_id: self.mover_id,
                mover: self.mover,
                entities,
                rules: self.rules,
                interner: self.interner,
            },
            raw_occupation,
            grid: self.grid,
            terrain: self.terrain,
            playfield_bounds: self.playfield_bounds,
        }
    }
}

impl BridgeMarkerContext<'_> {
    pub fn build(
        self,
        occupancy: &OccupancyGrid,
        entity_id: u64,
        current_cell: (u16, u16),
        facing: u16,
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
                on_bridge,
                type_ref: mover.type_ref,
                speed: mover.speed,
            },
            requested_urgency,
        )
    }
}

/// One lazily prepared native marker transaction. Creating this input does
/// not query Cells; the admitted A* calls prepare at429C1A. The same overlay
/// is borrowed later by the finishing passes and discarded before Mark1.
pub(super) struct DeferredBridgeMarkerSearch<'a> {
    context: BridgeMarkerContext<'a>,
    occupancy: &'a OccupancyGrid,
    entity_id: u64,
    current_cell: (u16, u16),
    facing: u16,
    on_bridge: bool,
    urgency: u8,
    search: std::cell::RefCell<Option<BridgeMarkerSearch>>,
}

impl<'a> BridgeMarkerContext<'a> {
    pub(super) fn defer_search(
        self,
        occupancy: &'a OccupancyGrid,
        entity_id: u64,
        current_cell: (u16, u16),
        facing: u16,
        on_bridge: bool,
        urgency: u8,
    ) -> DeferredBridgeMarkerSearch<'a> {
        DeferredBridgeMarkerSearch {
            context: self,
            occupancy,
            entity_id,
            current_cell,
            facing,
            on_bridge,
            urgency,
            search: std::cell::RefCell::new(None),
        }
    }
}

impl DeferredBridgeMarkerSearch<'_> {
    pub(super) fn prepare(&self) -> (u8, std::cell::Ref<'_, SearchMarkerOverlay>) {
        if self.search.borrow().is_none() {
            let search = self.context.build(
                self.occupancy,
                self.entity_id,
                self.current_cell,
                self.facing,
                self.on_bridge,
                self.urgency,
            );
            *self.search.borrow_mut() = Some(search);
        }
        let urgency = self.search.borrow().as_ref().unwrap().effective_urgency;
        (
            urgency,
            self.prepared().expect("prepared marker transaction"),
        )
    }
    pub(super) fn prepared(&self) -> Option<std::cell::Ref<'_, SearchMarkerOverlay>> {
        std::cell::Ref::filter_map(self.search.borrow(), |search| {
            search.as_ref().map(|search| &search.overlay)
        })
        .ok()
    }
}

fn direction_from_step(from: (i16, i16), to: (u16, u16)) -> u8 {
    let dx = i32::from(to.0 as i16) - i32::from(from.0);
    let dy = i32::from(to.1 as i16) - i32::from(from.1);
    crate::util::direction::direction_from_delta(dx, dy).unwrap_or(TUBE_STEP_DIRECTION)
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

/// Fixture queue for a found route, written by the production install from
/// `path[0]`.
#[cfg(test)]
pub(crate) fn fixture_path_replay(path: &[(u16, u16)]) -> FootPathQueue {
    let mut queue = FootPathQueue::default();
    if let Some(&start) = path.first() {
        install_path_replay(&mut queue, start, path, 1);
    }
    queue
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

fn remaining_path_from_entity(
    entity: &crate::sim::game_entity::GameEntity,
) -> ((i16, i16), Vec<u8>) {
    let queue = &entity.navigation.path_replay;
    // Foot+558 and its retained direction queue are the native source.
    // MovementTarget is a scheduling adapter and must not synthesize a peer
    // path after its native queue has been exhausted or invalidated.
    let reference = queue
        .reference_cell
        .unwrap_or((entity.position.rx as i16, entity.position.ry as i16));
    (reference, queue.remaining_directions().to_vec())
}

fn peer_from_entity(
    entity: &crate::sim::game_entity::GameEntity,
    rules: Option<&crate::rules::ruleset::RuleSet>,
    interner: &StringInterner,
) -> BridgeMarkerPeer {
    let (path_start, path_directions) = remaining_path_from_entity(entity);
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
        type_ref: entity.type_ref(),
        speed,
        path_start,
        path_directions,
        at_coord: super::at_coord::AtCoordQuery::from_entity(entity),
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

/// O(entities), used only by focused tests.
#[cfg(test)]
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
    //42B08A..0AB: sign-extend cell words, shift8, add128, multiply
    //requested height by the native level step. For negative cells the
    //receiver still performs its own signed-truncated coordinate division.
    let center = crate::sim::cell_kernel::cell_center(
        crate::sim::cell_kernel::CellCoordinate {
            x: i32::from(probe.0),
            y: i32::from(probe.1),
        },
        i32::from(requested_height).wrapping_mul(GROUND_LEVEL_HEIGHT_LEPTONS),
    );
    let probe_coord = crate::sim::components::DriveCoord {
        x: center.x,
        y: center.y,
        z: center.z,
    };
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
                //42B172 dispatches ILocomotion+A0. Its existing owner
                //retains NullCoord rejection, the handoff/head OR, and the
                //wrapping native height comparison; do not flatten them.
                if !peer.foot_derived
                    || !peer
                        .at_coord
                        .is_some_and(|query| query.matches(probe_coord))
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
) -> BridgeMarkerSearch {
    if !enabled || requested_urgency == 0 {
        return BridgeMarkerSearch {
            effective_urgency: requested_urgency,
            ..BridgeMarkerSearch::default()
        };
    }

    let direction = crate::util::direction_tables::quantize::dir_from_facing16(mover.facing);
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
            if candidate != current && raw_occupation.ground_bits(cell.0, cell.1) != 0 {
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
        let kind = if category == EntityCategory::Infantry {
            LocomotorKind::Walk
        } else {
            LocomotorKind::Drive
        };
        let current = super::super::foot_path::cell_centre(start);
        BridgeMarkerPeer {
            category,
            foot_derived: true,
            type_ref,
            speed,
            path_start: start,
            path_directions: directions.to_vec(),
            at_coord: super::super::at_coord::AtCoordQuery::from_state(
                kind,
                current,
                Some(current),
                super::super::at_coord::AtCoordTrack::default(),
            ),
        }
    }

    fn mover(type_ref: InternedId) -> BridgeMarkerMover {
        BridgeMarkerMover {
            current_cell: (5, 5),
            facing: 0,
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
        );
        assert!(search.processed_peer_path);
        assert!(!search.overlay.contains((6, 4)), "duplicate visit cancels");
        assert!(search.overlay.contains((7, 4)));
        assert!(
            search.overlay.contains((5, 4)),
            "unoccupied probe center toggles"
        );
    }

    fn assert_native_peer_suffix(name: &str, probe: (i16, i16), occupied: (u16, u16)) {
        // Native slot40 executions preserve the complete head/handoff query.
        //42B08A..0AB supplies signed-cell center XYZ, then42B172 calls it.
        let data: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
            "tools/spatial_oracle/locomotor_at_coord.json",
        ))
        .unwrap();
        let case = data["cases"]
            .as_array()
            .unwrap()
            .iter()
            .find(|case| case["input"]["name"] == name)
            .unwrap();
        let input = &case["input"];
        let coord = |value: &serde_json::Value| crate::sim::components::DriveCoord {
            x: value[0].as_i64().unwrap() as i32,
            y: value[1].as_i64().unwrap() as i32,
            z: value[2].as_i64().unwrap() as i32,
        };
        let current = coord(&input["current"]);
        let head = coord(&input["stored_head"]);
        let family = match input["family"].as_str().unwrap() {
            "drive" => LocomotorKind::Drive,
            "hover" => LocomotorKind::Hover,
            _ => unreachable!(),
        };
        let mut entity =
            crate::sim::game_entity::GameEntity::test_default(2, "PEER", "Americans", 0, 0);
        super::super::ground_pose::put_location(&mut entity.position, current);
        entity.locomotor = Some(super::super::locomotor::LocomotorState::for_test_kind(
            family,
        ));
        if family == LocomotorKind::Drive {
            let loco = entity.locomotor.as_mut().unwrap();
            loco.ensure_installed_track_state();
            let mut track = loco
                .track_progress(super::super::track_process::TrackFamily::Drive)
                .unwrap();
            track.turn_index = input["turn_index"].as_i64().unwrap_or(-1) as i32;
            track.cursor = input["cursor"].as_i64().unwrap_or(0) as i32;
            loco.store_track_progress(super::super::track_process::TrackFamily::Drive, track);
            loco.store_track_head(super::super::track_process::TrackFamily::Drive, Some(head));
        } else {
            entity
                .locomotor
                .as_mut()
                .unwrap()
                .hover_runtime_mut()
                .unwrap()
                .set_head(Some(head));
        }
        let interner = StringInterner::new();
        let mut entities = EntityStore::new();
        entities.insert(entity);
        let peer = bridge_marker_peer(&entities, 2, None, &interner).unwrap();
        let mut peers = BridgeMarkerPeerSnapshot::default();
        peers.peers.insert(2, peer);
        let mut occupancy = OccupancyGrid::new();
        occupancy.add(
            occupied.0,
            occupied.1,
            2,
            MovementLayer::Ground,
            None,
            CellListInsertion::PrependNonBuilding,
        );
        let original_probe = &input["probes"][0];
        let center = crate::sim::cell_kernel::cell_center(
            crate::sim::cell_kernel::CellCoordinate {
                x: i32::from(probe.0),
                y: i32::from(probe.1),
            },
            0,
        );
        assert_eq!(
            [center.x, center.y, center.z],
            [
                original_probe[0].as_i64().unwrap() as i32,
                original_probe[1].as_i64().unwrap() as i32,
                original_probe[2].as_i64().unwrap() as i32,
            ]
        );
        let native = case["output"]["queries"]
            .as_array()
            .unwrap()
            .iter()
            .find(|row| row["probe"] == *original_probe)
            .unwrap()["at"]
            .as_bool()
            .unwrap();
        let expected = if native { vec![2] } else { vec![] };
        assert_eq!(
            find_nearby_bridge_peer_suffix(&peers, &occupancy, &PathGrid::new(12, 12), probe, 0),
            expected,
            "{name}: marker must use the original complete Is_At_Coord decision"
        );
    }

    #[test]
    fn native_peer_shared_cell_handoff_accepts_its_current_z() {
        assert_native_peer_suffix("marker_shared_cell_handoff_z", (5, 4), (5, 4));
    }

    #[test]
    fn native_peer_null_drive_head_is_rejected() {
        assert_native_peer_suffix("marker_null_head_center", (0, 0), (0, 0));
    }

    #[test]
    fn native_peer_signed_negative_cell_uses_center_coordinate() {
        assert_native_peer_suffix("marker_negative_signed_cell_center", (-1, -1), (0, 0));
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
        rejected.at_coord = super::super::at_coord::AtCoordQuery::from_state(
            LocomotorKind::Drive,
            super::super::foot_coordinate::NULL_COORD,
            None,
            super::super::at_coord::AtCoordTrack::default(),
        );
        peers.peers.insert(2, rejected);
        let accepted = peer(EntityCategory::Unit, accepted_type, 2, (5, 4), &[2, 2]);
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
        );
        assert!(search.overlay.contains((6, 4)), "accepted peer replayed");
        assert!(search.overlay.contains((5, 6)), "same-list suffix replayed");
    }

    #[test]
    fn live_peers_use_entity_store_and_captured_mover() {
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
        let live = |entities: &EntityStore| {
            [1, 2, 3].map(|id| {
                LiveBridgeMarkerPeers {
                    mover_id: 1,
                    mover: Some(&mover),
                    entities,
                    rules: None,
                    interner: &interner,
                }
                .peer(id)
                .map(Cow::into_owned)
            })
        };

        // Untouched world: live reads equal the independently collected facts.
        let read = live(&entities);
        assert_eq!(
            read[0].as_ref().unwrap().path_directions,
            snapshot.peers[&1].path_directions
        );
        assert_eq!(
            read[1].as_ref().unwrap().path_directions,
            snapshot.peers[&2].path_directions
        );
        assert!(read[2].is_none());

        // Both entities change. The peer is read as it is now; the mover keeps
        // the facts captured at the marker boundary.
        for id in [1, 2] {
            entities
                .get_mut(id)
                .unwrap()
                .navigation
                .path_replay
                .directions = vec![6, 6, 6];
        }
        let read = live(&entities);
        assert_eq!(read[0].as_ref().unwrap().path_directions, [2, 2]);
        assert_eq!(read[1].as_ref().unwrap().path_directions, [6, 6, 6]);
    }

    #[test]
    fn exhausted_native_peer_queue_is_not_rebuilt_from_scheduling_path() {
        //42ACF0 reads Foot+5E0 directly. A stale MovementTarget adapter must
        //not supply a different direction list after the queue reaches -1.
        let mut peer =
            crate::sim::game_entity::GameEntity::test_default(2, "PEER", "Americans", 4, 3);
        peer.navigation.path_replay.reference_cell = Some((5, 4));
        peer.navigation.path_replay.directions = vec![2, 2];
        peer.navigation.path_replay.cursor = 2;
        peer.movement_target = Some(crate::sim::components::MovementTarget {
            ..Default::default()
        });
        assert_eq!(remaining_path_from_entity(&peer), ((5, 4), vec![]));
        peer.navigation.path_replay.reference_cell = None;
        assert_eq!(remaining_path_from_entity(&peer), ((4, 3), vec![]));
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
        let loco = peer.locomotor.as_mut().unwrap();
        loco.ensure_installed_track_state();
        loco.store_track_head(
            super::super::track_process::TrackFamily::Drive,
            Some(crate::sim::components::DriveCoord::cell(6, 3, 4)),
        );
        loco.store_track_valid(super::super::track_process::TrackFamily::Drive, true);
        let mut track = loco
            .track_progress(super::super::track_process::TrackFamily::Drive)
            .unwrap();
        track.turn_index = 1;
        track.cursor = 12;
        loco.store_track_progress(super::super::track_process::TrackFamily::Drive, track);
        peer.navigation.path_replay.reference_cell = Some((5, 4));
        peer.navigation.path_replay.directions = vec![2, 2];
        // RawTrack 3 handoff point 22, transformed around head cell (6,3),
        // lies in probe cell (5,4).  A deck track deliberately owns no ground
        // occupation_head_to reservation, so that field cannot answer slot 40.
        entities.insert(peer);

        let peers = snapshot_bridge_marker_peers(&entities, None, &interner);
        let peer = peers.peers.get(&2).expect("Drive peer snapshot");
        assert_eq!(peer.at_coord.unwrap().cells(), (Some((5, 4)), (6, 3)));

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
            .and_then(|loco| loco.hover_runtime_mut())
            .unwrap()
            .set_head(Some(crate::sim::components::DriveCoord::cell(5, 4, 104)));
        peer.navigation.path_replay.reference_cell = Some((4, 3));
        peer.navigation.path_replay.directions = vec![3, 2, 2];
        entities.insert(peer);

        let peers = snapshot_bridge_marker_peers(&entities, None, &interner);
        let peer = peers.peers.get(&2).expect("Hover peer snapshot");
        assert_eq!(peer.at_coord.unwrap().cells().1, (5, 4));
        assert_eq!(peer.at_coord.unwrap().head_z(), 104);

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
        assert_eq!(peer.at_coord.unwrap().cells().1, (5, 4));
        assert_eq!(peer.at_coord.unwrap().head_z(), 120);
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
