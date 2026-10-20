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
#[cfg(any(test, feature = "test-support"))]
use std::collections::BTreeMap;

use crate::map::entities::EntityCategory;
use crate::map::resolved_terrain::ResolvedTerrainGrid;
#[cfg(any(test, feature = "test-support"))]
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
#[cfg(any(test, feature = "test-support"))]
#[derive(Debug, Clone, Default)]
pub(super) struct BridgeMarkerPeerSnapshot {
    peers: BTreeMap<u64, BridgeMarkerPeer>,
}

/// ID-to-facts lookup behind `UpdateBridgePassability`'s object-list walks.
pub(super) trait BridgeMarkerPeerLookup {
    fn peer(&self, entity_id: u64) -> Option<Cow<'_, BridgeMarkerPeer>>;
}

#[cfg(any(test, feature = "test-support"))]
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
    /// The mover's id while Mark0 has not lifted it off its cell (a batched
    /// path search, `foot_path::MoverLift::Emulated`): the object lists
    /// skip it, as they would after Mark0 unlinked it.
    pub unlisted: Option<u64>,
}

#[derive(Debug, Default)]
pub(super) struct BridgeMarkerSearch {
    pub overlay: SearchMarkerOverlay,
    /// Urgency actually supplied to A*.  Retail downgrades urgency 1 to zero
    /// when no eligible peer path was replayed, before the raw-byte phase.
    pub effective_urgency: u8,
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
        unlisted: Option<u64>,
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
                unlisted,
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
    unlisted: Option<u64>,
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
        unlisted: Option<u64>,
    ) -> DeferredBridgeMarkerSearch<'a> {
        DeferredBridgeMarkerSearch {
            context: self,
            occupancy,
            entity_id,
            current_cell,
            facing,
            on_bridge,
            urgency,
            unlisted,
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
                self.unlisted,
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

/// Fixture queue for a found route, written by the production install from
/// `path[0]`.
#[cfg(any(test, feature = "test-support"))]
pub(crate) fn fixture_path_replay(path: &[(u16, u16)]) -> FootPathQueue {
    let mut queue = FootPathQueue::default();
    if let Some(&start) = path.first() {
        queue.install(start, path, 1);
    }
    queue
}

/// Walk75BD89..75BDB1 propagates a -1 head into the next word before
/// shifting. Retargeting an already-paid head must not expose its old suffix.
/// Original block comparisons: tools/spatial_oracle/walk_first_step.json.
pub(super) fn consume_walk_path_replay(queue: &mut FootPathQueue) {
    let invalidated = queue.remaining_directions().is_empty();
    queue.consume(1);
    if invalidated {
        queue.clear_live_head();
    }
}

/// The peer path `PathfinderClass::UpdateBridgePassability` (`0x0042ACF0`) replays:
/// Foot `+0x558` (`0x0042AE33`) and the directions at `+0x5E0` (`0x0042AE88`).
fn remaining_path_from_entity(
    entity: &crate::sim::game_entity::GameEntity,
) -> ((i16, i16), Vec<u8>) {
    let queue = entity.navigation.path_replay();
    // Foot+558 and its retained direction queue are the native source.
    let reference = queue
        .reference_cell()
        .unwrap_or((entity.position.rx() as i16, entity.position.ry() as i16));
    (reference, queue.remaining_directions().to_vec())
}

fn peer_from_entity(
    entity: &crate::sim::game_entity::GameEntity,
    rules: Option<&crate::rules::ruleset::RuleSet>,
    interner: &StringInterner,
) -> BridgeMarkerPeer {
    let (path_start, path_directions) = remaining_path_from_entity(entity);
    let speed = rules
        .and_then(|rules| interner.object_type(entity.type_ref(), rules))
        .map_or(0, |object| object.speed);
    let foot_derived = matches!(
        entity.category(),
        EntityCategory::Unit | EntityCategory::Infantry | EntityCategory::Aircraft
    );
    BridgeMarkerPeer {
        category: entity.category(),
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

/// The ids on `cell`'s `layer` list, in list order, without `unlisted`.
fn list_ids(
    occupancy: &OccupancyGrid,
    cell: (i16, i16),
    layer: MovementLayer,
    unlisted: Option<u64>,
) -> Vec<u64> {
    let cell = unsigned_cell(cell);
    occupancy
        .get(cell.0, cell.1)
        .map(|occupancy| {
            occupancy
                .iter_layer(layer)
                .map(|occupant| occupant.entity_id)
                .filter(|&id| Some(id) != unlisted)
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
    unlisted: Option<u64>,
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
            let list = list_ids(occupancy, candidate, layer, unlisted);
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
    let mut selected_list = list_ids(occupancy, probe, direct_layer, mover.unlisted);
    if selected_list.is_empty() {
        selected_list = find_nearby_bridge_peer_suffix(
            peers,
            occupancy,
            grid,
            probe,
            requested_height,
            mover.unlisted,
        );
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
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sim::occupancy::CellListInsertion;

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
        entity.position.put_location(current);
        entity.locomotor = Some(super::super::locomotor::LocomotorState::for_test_kind(
            family,
        ));
        if family == LocomotorKind::Drive {
            let loco = entity.locomotor.as_mut().unwrap();
            loco.ensure_installed_track_state();
            let mut track = loco
                .track_progress(super::super::track_process::TrackFamily::Drive)
                .unwrap();
            track.set_turn_index(input["turn_index"].as_i64().unwrap_or(-1) as i32);
            track.set_cursor(input["cursor"].as_i64().unwrap_or(0) as i32);
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
            find_nearby_bridge_peer_suffix(
                &peers,
                &occupancy,
                &PathGrid::new(12, 12),
                probe,
                0,
                None
            ),
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
}
