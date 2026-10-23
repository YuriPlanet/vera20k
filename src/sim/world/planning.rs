//! Active YR PlanMgr: shared nodes and per-Techno path tokens.
//! Native identities: append638120, commit636CE0, consume636590,
//! lookup636E60, connect639740/639800 and delete639BC0 (gamemd.exe).
//! The obsolete TS House path slots and Foot Formation+520 are unrelated.

use super::Simulation;
use crate::map::entities::EntityCategory;
use crate::sim::command::{
    Command, CommandEnvelope, CommandRecord, MegaMissionOrder, MegaMissionRecord, MegaMissionTarget,
};
use crate::sim::components::DriveCoord;
use crate::sim::intern::InternedId;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::hash::{Hash, Hasher};

#[derive(Debug, Clone, Default, Serialize)]
pub(super) struct PlanningState {
    next_node: u64,
    nodes: BTreeMap<u64, PlanningNode>,
    // Derived coordinate index; mutations are centralized in insert/remove_node.
    #[serde(skip)]
    locations: BTreeMap<((u16, u16), i32), Vec<u64>>,
    #[serde(skip)]
    dynamic_nodes: BTreeSet<u64>,
    paths: BTreeMap<u64, PlanningPath>,
    // AC4C7C: constructor / late-token registration order, independent of
    // node contents. Loaded actors may create tokens in any stable-ID order.
    token_order: Vec<u64>,
    #[serde(skip)]
    registered_tokens: BTreeSet<u64>,
    // AC4B84 is independently mutated accounting. Prune/node delete can
    // leave a charge behind even after a path becomes empty.
    // Retail has 24 slots; extend the same owner to VERA's 30 players plus
    // Neutral/Special, matching its 32-bit House masks.
    house_charges: [i32; 32],
    /// `0x00AC4CF6`: set while [`Simulation::planning_select_tail_node`]
    /// Selects partners, so their Selects (and any Select their tag events
    /// start) do not expand. Clear between calls; neither saved nor hashed.
    #[serde(skip)]
    select_expanding: bool,
}

impl<'de> Deserialize<'de> for PlanningState {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        struct StoredPlanningState {
            next_node: u64,
            nodes: BTreeMap<u64, PlanningNode>,
            paths: BTreeMap<u64, PlanningPath>,
            token_order: Vec<u64>,
            house_charges: [i32; 32],
        }
        let stored = StoredPlanningState::deserialize(deserializer)?;
        if crate::util::save_codec::layout::describing() {
            return Ok(Self::default());
        }
        let mut state = Self {
            next_node: stored.next_node,
            nodes: stored.nodes,
            locations: BTreeMap::new(),
            dynamic_nodes: BTreeSet::new(),
            paths: stored.paths,
            token_order: stored.token_order,
            registered_tokens: BTreeSet::new(),
            house_charges: stored.house_charges,
            select_expanding: false,
        };
        state.restore_index().map_err(serde::de::Error::custom)?;
        Ok(state)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct PlanningNode {
    id: u64,
    frame: i32,
    // Member order follows native event encounter order, not stable-ID sort.
    orders: BTreeMap<u64, VecDeque<PlanningOrder>>,
    member_order: Vec<u64>,
    #[serde(skip)]
    live_members: BTreeMap<usize, u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct PlanningOrder {
    // Stable encounter slot: native member removal shifts its vector; retained
    // slots preserve the same live order without shifting 20,000 entries.
    position: usize,
    record: CommandRecord,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct PlanningPath {
    nodes: Vec<u64>,
    pending: i32,
    loop_start: Option<usize>,
    loop_changed: bool,
    executing: bool,
    current: Option<CommandRecord>,
}

/// The refusals PlanMgr 0x0063A5A0 posts on a left press, each as its
/// literal CSF key with ScoldSound.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlanningPressRefusal {
    InvalidNode,
    NoAircraft,
    NoBuildings,
    NoUnknowns,
}

#[derive(Debug, Clone)]
pub struct PlanningPathView {
    pub actor_id: u64,
    pub cells: Vec<(u16, u16)>,
    pub loop_start: Option<usize>,
    pub editing: bool,
}

/// A fresh read projection of the authoritative shared graph. Pixel colors,
/// palette packing, line patterns and selection remain presentation-owned.
#[derive(Debug, Clone)]
pub struct PlanningDrawGraph {
    pub nodes: Vec<PlanningDrawNode>,
    pub actors: Vec<PlanningDrawActor>,
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) hovered_node: Option<u64>,
    pub highlighted_node: Option<u64>,
    pub hovered_coord: Option<DriveCoord>,
    pub highlighted_coord: Option<DriveCoord>,
    pub highlighted_members: Vec<u64>,
}

#[derive(Debug, Clone)]
pub struct PlanningDrawNode {
    /// Stable identity for client-only geometry history, not a second graph.
    pub id: u64,
    pub coord: DriveCoord,
    pub members: Vec<u64>,
    pub incoming: Vec<PlanningDrawSegment>,
    pub loop_segment: Option<PlanningDrawSegment>,
    pub marker_owner: InternedId,
    pub marker_bright: bool,
    pub badge: Option<PlanningBadgeKind>,
}

#[derive(Debug, Clone)]
pub struct PlanningDrawSegment {
    pub source: DriveCoord,
    pub target: DriveCoord,
    pub owner: InternedId,
    pub bright: bool,
}

#[derive(Debug, Clone)]
pub struct PlanningDrawActor {
    pub actor_id: u64,
    pub owner: InternedId,
    pub connection: Option<PlanningDrawSegment>,
    pub badge: Option<PlanningBadgeKind>,
    pub current_leg: Option<PlanningDrawSegment>,
    pub current_badge: Option<PlanningBadgeKind>,
    pub mega_leg: Option<PlanningDrawSegment>,
    pub mega_badge: Option<PlanningBadgeKind>,
    pub marker_only: Option<DriveCoord>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlanningBadgeKind {
    AreaGuard,
    AttackMove,
}

impl PlanningPath {
    /// Token636B50 removes the requested occurrence without touching the
    /// executing latch, current event or loop-edit flag.
    fn remove_occurrence(&mut self, index: usize) -> u64 {
        if self.nodes.len() == 1 {
            self.pending = -1;
        } else if self.pending > 0 && (index as i32) < self.pending {
            self.pending -= 1;
        }
        if let Some(start) = self.loop_start {
            if index < start {
                self.loop_start = Some(start - 1);
            } else if self.nodes.len() - start - 1 < 3 {
                self.loop_start = None;
            }
        }
        self.nodes.remove(index)
    }

    /// 636E60 indexes the remaining path; beyond its end it wraps only the loop.
    fn node_at(&self, index: i32) -> Option<u64> {
        let index = usize::try_from(index).ok()?;
        if index < self.nodes.len() {
            return self.nodes.get(index).copied();
        }
        let start = self.loop_start?;
        let length = self.nodes.len().checked_sub(start)?;
        if length == 0 {
            return None;
        }
        self.nodes.get(start + (index - start) % length).copied()
    }

    /// 636CE0 resets pending state, including loop edits. Drawing caches are
    /// presentation-owned in Rust, so no native pixel cache is serialized.
    fn commit(&mut self) {
        self.pending = -1;
        self.loop_changed = false;
    }

    /// 636590 consumes immediately. The current command is separate from the
    /// remaining vector; a loop head rotates, otherwise it is removed.
    fn consume(&mut self) -> Option<u64> {
        let node = self.node_at(0)?;
        self.nodes.remove(0);
        match self.loop_start {
            Some(0) => self.nodes.push(node),
            Some(start) => self.loop_start = Some(start - 1),
            None => {}
        }
        self.pending = self.pending.saturating_sub(1).max(-1);
        Some(node)
    }
}

impl PlanningState {
    /// The location index is derived from authoritative nodes. Reject broken
    /// graph references before restored plans can reach connect/consume.
    fn restore_index(&mut self) -> Result<(), &'static str> {
        self.registered_tokens = self.token_order.iter().copied().collect();
        if self.registered_tokens.len() != self.token_order.len()
            || self
                .paths
                .keys()
                .any(|actor| !self.registered_tokens.contains(actor))
        {
            return Err("invalid planning token registration");
        }
        let mut memberships = BTreeMap::<(u64, u64), usize>::new();
        for (&actor, path) in &self.paths {
            if path.pending < -1
                || usize::try_from(path.pending).is_ok_and(|pending| pending > path.nodes.len())
                || path
                    .loop_start
                    .is_some_and(|start| start >= path.nodes.len())
            {
                return Err("invalid planning path cursor");
            }
            if let Some(record) = &path.current
                && !valid_planning_record(record, actor, 2)
            {
                return Err("invalid planning current order");
            }
            for &id in &path.nodes {
                let node = self.nodes.get(&id).ok_or("missing planning node")?;
                if !node.orders.contains_key(&actor) {
                    return Err("invalid planning node membership");
                }
                *memberships.entry((id, actor)).or_default() += 1;
            }
        }
        for (&id, node) in &mut self.nodes {
            if node.id != id || id >= self.next_node || node.orders.is_empty() {
                return Err("invalid planning node identity");
            }
            node.live_members.clear();
            for (&actor, orders) in &node.orders {
                if orders.is_empty() || memberships.get(&(id, actor)) != Some(&orders.len()) {
                    return Err("orphan planning member");
                }
                let mut previous = None;
                for order in orders {
                    if node.member_order.get(order.position) != Some(&actor)
                        || previous.is_some_and(|position| position >= order.position)
                        || node.live_members.insert(order.position, actor).is_some()
                    {
                        return Err("invalid planning member occurrence");
                    }
                    previous = Some(order.position);
                    if !valid_planning_record(&order.record, actor, 1) {
                        return Err("invalid planning node order");
                    }
                }
            }
        }
        for id in self.nodes.keys().copied().collect::<Vec<_>>() {
            self.index_node(id);
        }
        Ok(())
    }

    pub(super) fn hash_state(&self, hasher: &mut impl Hasher) {
        // Keep existing no-plan hash controls stable while hashing every
        // authoritative field, including identity allocation after cleanup.
        if self.next_node == 0
            && self.nodes.is_empty()
            && self.paths.is_empty()
            && self.token_order.is_empty()
            && self.house_charges.iter().all(|charge| *charge == 0)
        {
            return;
        }
        b"planning-v1".hash(hasher);
        self.next_node.hash(hasher);
        self.nodes.len().hash(hasher);
        for node in self.nodes.values() {
            (node.id, node.frame).hash(hasher);
            node.orders.len().hash(hasher);
            node.member_order.hash(hasher);
            for (actor, orders) in &node.orders {
                actor.hash(hasher);
                orders.len().hash(hasher);
                for order in orders {
                    order.position.hash(hasher);
                    order.record.as_bytes().hash(hasher);
                }
            }
        }
        self.paths.len().hash(hasher);
        self.token_order.hash(hasher);
        self.house_charges.hash(hasher);
        for (actor, path) in &self.paths {
            actor.hash(hasher);
            (
                &path.nodes,
                path.pending,
                path.loop_start,
                path.loop_changed,
                path.executing,
            )
                .hash(hasher);
            path.current
                .as_ref()
                .map(CommandRecord::as_bytes)
                .hash(hasher);
        }
    }

    fn remove_member(&mut self, node_id: u64, actor: u64) {
        self.unindex_node(node_id);
        let Some(node) = self.nodes.get_mut(&node_id) else {
            return;
        };
        if let Some(orders) = node.orders.get_mut(&actor) {
            if let Some(order) = orders.pop_front() {
                node.live_members.remove(&order.position);
            }
            if orders.is_empty() {
                node.orders.remove(&actor);
            }
        }
        if node.orders.is_empty() {
            self.remove_node(node_id);
        } else {
            self.index_node(node_id);
        }
    }

    fn register_token(&mut self, actor: u64) {
        if self.registered_tokens.insert(actor) {
            self.token_order.push(actor);
        }
    }

    fn remove_node(&mut self, node_id: u64) {
        self.unindex_node(node_id);
        self.nodes.remove(&node_id);
    }

    fn index_node(&mut self, id: u64) {
        let node = &self.nodes[&id];
        match node.target() {
            Some(MegaMissionTarget::Cell { x, y }) => {
                self.locations
                    .entry(((x as u16, y as u16), node.frame))
                    .or_default()
                    .push(id);
            }
            Some(MegaMissionTarget::Object { .. }) => {
                self.dynamic_nodes.insert(id);
            }
            _ => {}
        }
    }

    fn unindex_node(&mut self, id: u64) {
        let Some(node) = self.nodes.get(&id) else {
            return;
        };
        if let Some(MegaMissionTarget::Cell { x, y }) = node.target() {
            let key = ((x as u16, y as u16), node.frame);
            if let Some(ids) = self.locations.get_mut(&key) {
                ids.retain(|node_id| *node_id != id);
                if ids.is_empty() {
                    self.locations.remove(&key);
                }
            }
        }
        self.dynamic_nodes.remove(&id);
    }
}

impl PlanningNode {
    fn target(&self) -> Option<MegaMissionTarget> {
        self.live_members
            .values()
            .next()
            .and_then(|actor| self.orders.get(actor))
            .and_then(|orders| orders.front())
            .map(|order| &order.record)
            .and_then(planning_record_target)
    }

    fn insert_order(&mut self, actor: u64, record: CommandRecord) {
        let position = self.member_order.len();
        self.member_order.push(actor);
        self.live_members.insert(position, actor);
        self.orders
            .entry(actor)
            .or_default()
            .push_back(PlanningOrder { position, record });
    }

    fn order(&self, actor: u64) -> Option<&CommandRecord> {
        self.orders.get(&actor)?.front().map(|order| &order.record)
    }
}

impl Simulation {
    /// Techno+514 / Token636DC0: only remaining occurrences count. A current
    /// event can still execute after the final node has been consumed.
    pub(crate) fn planning_has_remaining_nodes(&self, actor: u64) -> bool {
        self.planning
            .paths
            .get(&actor)
            .is_some_and(|path| !path.nodes.is_empty())
    }

    /// ClearScene6370B0 and TechnoLoad70BF79 discard active YR planning.
    /// This is a player-load boundary, separate from internal serialization.
    pub(crate) fn reset_planning_after_player_load(&mut self) {
        self.planning = PlanningState::default();
    }

    /// Normalize the unsaved planner in cross-crate state-only load fixtures,
    /// alongside their existing explicit Scenario RNG reset.
    #[cfg(any(test, feature = "test-support"))]
    pub fn reset_planning_after_player_load_for_test(&mut self) {
        self.reset_planning_after_player_load();
    }

    pub(crate) fn validate_planning_references(&self) -> Result<(), &'static str> {
        for actor in &self.planning.token_order {
            self.substrate
                .entities
                .get(*actor)
                .ok_or("missing planning token owner")?;
        }
        for (&actor, path) in &self.planning.paths {
            self.substrate
                .entities
                .get(actor)
                .ok_or("missing planning actor")?;
            for record in path.current.iter().chain(
                path.nodes
                    .iter()
                    .copied()
                    .collect::<BTreeSet<_>>()
                    .into_iter()
                    .flat_map(|id| {
                        self.planning
                            .nodes
                            .get(&id)
                            .and_then(|node| node.orders.get(&actor))
                            .into_iter()
                            .flatten()
                            .map(|order| &order.record)
                    }),
            ) {
                let issuer = usize::try_from(record.house_id())
                    .ok()
                    .and_then(|index| self.houses.order().get(index))
                    .ok_or("missing planning order house")?;
                if !self.houses.contains_key(issuer) {
                    return Err("missing planning order house");
                }
            }
        }
        Ok(())
    }

    fn planning_header(&self, owner: InternedId, opcode: u8) -> Option<CommandRecord> {
        if !self.houses.contains_key(&owner) {
            return None;
        }
        let house = i8::try_from(self.houses.index_of(owner)?).ok()?;
        CommandRecord::encode(
            opcode,
            i32::from(house),
            self.session.binary_frame() as i32,
            &[],
        )
        .ok()
    }

    pub fn encode_planning_order_record(
        &self,
        owner: InternedId,
        command: &Command,
    ) -> Option<CommandRecord> {
        let mut record = self.encode_megamission_record(owner, command)?;
        record.payload_mut()[22] = 1;
        Some(record)
    }

    /// PlanMgr639130, called by LeftUp4ABF7F: the current player's retained
    /// accounting plus selected Technos with empty/missing tokens must be
    /// <=128. Prune and node deletion do not release accounting; a connected
    /// path does not charge it. It cannot be recomputed from nonempty paths.
    pub fn planning_selection_has_capacity(&self, owner: InternedId, selected: &[u64]) -> bool {
        let player = self.session.current_house().unwrap_or(owner);
        let retained = self
            .houses
            .index_of(player)
            .and_then(|index| self.planning.house_charges.get(index))
            .copied()
            .unwrap_or(0);
        let additional = selected
            .iter()
            .filter(|actor| {
                self.substrate.entities.contains(**actor)
                    && self
                        .planning
                        .paths
                        .get(*actor)
                        .is_none_or(|path| path.nodes.is_empty())
            })
            .count();
        retained.wrapping_add(additional as i32) <= 0x80
    }

    pub fn encode_plan_commit_record(&self, owner: InternedId) -> Option<CommandRecord> {
        // Event constructor4C66C0: header only, no route packet.
        self.planning_header(owner, 0x2b)
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn encode_plan_connect_record(
        &self,
        owner: InternedId,
        actor_id: u64,
        cell: (u16, u16),
    ) -> Option<CommandRecord> {
        self.substrate.entities.get(actor_id)?;
        let node = self.planning_node_id_at_cell(owner, cell)?;
        self.planning_connect_record_to_node(owner, actor_id, node)
    }

    pub fn encode_plan_connect_record_at_screen(
        &self,
        owner: InternedId,
        actor_id: u64,
        point: [i32; 2],
    ) -> Option<CommandRecord> {
        let node = self.planning_node_id_at_screen(Some(owner), point)?;
        self.planning_connect_record_to_node(owner, actor_id, node)
    }

    fn planning_connect_record_to_node(
        &self,
        owner: InternedId,
        actor_id: u64,
        node: u64,
    ) -> Option<CommandRecord> {
        self.substrate.entities.get(actor_id)?;
        let mut record = self.planning_header(owner, 0x2a)?;
        let (value, kind) = MegaMissionTarget::Object {
            id: i32::try_from(actor_id).ok()?,
        }
        .token()
        .ok()?;
        record.payload_mut()[0..4].copy_from_slice(&value.to_le_bytes());
        record.payload_mut()[4] = kind;
        let (value, kind) = self.planning.nodes[&node].target()?.token().ok()?;
        record.payload_mut()[5..9].copy_from_slice(&value.to_le_bytes());
        record.payload_mut()[9] = kind;
        Some(record)
    }

    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn encode_plan_node_delete_record(
        &self,
        owner: InternedId,
        cell: (u16, u16),
    ) -> Option<CommandRecord> {
        let node = self.planning_node_id_at_cell(owner, cell)?;
        self.planning_delete_record_for_node(owner, node)
    }

    pub fn encode_plan_node_delete_record_at_screen(
        &self,
        owner: InternedId,
        point: [i32; 2],
    ) -> Option<CommandRecord> {
        let node = self.planning_node_id_at_screen(Some(owner), point)?;
        self.planning_delete_record_for_node(owner, node)
    }

    fn planning_delete_record_for_node(
        &self,
        owner: InternedId,
        node: u64,
    ) -> Option<CommandRecord> {
        let mut record = self.planning_header(owner, 0x2c)?;
        let (value, kind) = self.planning.nodes[&node].target()?.token().ok()?;
        record.payload_mut()[0..4].copy_from_slice(&value.to_le_bytes());
        record.payload_mut()[4] = kind;
        Some(record)
    }

    pub fn planning_paths(&self, owner: InternedId) -> Vec<PlanningPathView> {
        self.planning
            .paths
            .iter()
            .filter(|(actor, path)| {
                self.substrate
                    .entities
                    .get(**actor)
                    .is_some_and(|entity| entity.owner() == owner)
                    && !path.nodes.is_empty()
            })
            .map(|(&actor_id, path)| PlanningPathView {
                actor_id,
                cells: path
                    .nodes
                    .iter()
                    .filter_map(|id| self.planning_node_coords(*id).and_then(coord_cell))
                    .collect(),
                loop_start: path.loop_start,
                editing: path.pending >= 0 || path.loop_changed,
            })
            .collect()
    }

    /// PlanMgr63B150 and node63CAE0 traverse shared nodes before actor links.
    /// Association634CC0/634E10 groups predecessor records by their live XYZ;
    /// the first remaining occurrence of a node supplies each actor's index.
    ///
    /// Only the presentation and input ask this. A target read looks its
    /// cell up, which stamps the fallback CellClass on a miss; the stamp
    /// stays private to the query, so drawing cannot change the simulation.
    pub fn planning_draw_graph(
        &self,
        viewer: InternedId,
        selected: &[u64],
        hover_point: Option<[i32; 2]>,
        retained_highlighted_node: Option<u64>,
    ) -> PlanningDrawGraph {
        self.effective_shared_cell_dummy().detached(|| {
            self.build_planning_draw_graph(viewer, selected, hover_point, retained_highlighted_node)
        })
    }

    fn build_planning_draw_graph(
        &self,
        viewer: InternedId,
        selected: &[u64],
        hover_point: Option<[i32; 2]>,
        retained_highlighted_node: Option<u64>,
    ) -> PlanningDrawGraph {
        let mut positions = BTreeMap::new();
        for (&actor, path) in &self.planning.paths {
            for (index, &node) in path.nodes.iter().enumerate() {
                positions.entry((actor, node)).or_insert(index);
            }
        }
        let allied = |actor: u64| {
            self.substrate.entities.get(actor).is_some_and(|entity| {
                crate::map::houses::is_allied_with(
                    self.house_alliances.as_map(),
                    self.interner.resolve(entity.owner()),
                    self.interner.resolve(viewer),
                )
            })
        };
        let record_coord = |record: &CommandRecord| {
            planning_record_target(record).and_then(|target| self.planning_target_coords(target))
        };
        let mut nodes = Vec::new();
        for node in self.planning.nodes.values() {
            let members: Vec<_> = node.live_members.values().copied().collect();
            let Some(&head) = members.first() else {
                continue;
            };
            if !allied(head) {
                continue;
            }
            let Some(coord) = self.planning_node_coords(node.id) else {
                continue;
            };
            let Some(marker_owner) = self.planning_actor_house(head) else {
                continue;
            };
            let mut incoming = Vec::<PlanningDrawSegment>::new();
            let mut groups = BTreeMap::new();
            let mut loop_segment: Option<PlanningDrawSegment> = None;
            for &actor in &members {
                let path = &self.planning.paths[&actor];
                let index = positions[&(actor, node.id)];
                let Some(owner) = self.planning_actor_house(actor) else {
                    continue;
                };
                let previous = path.nodes[(index + path.nodes.len() - 1) % path.nodes.len()];
                if previous != *path.nodes.last().unwrap()
                    && let Some(source) = self.planning.nodes[&previous]
                        .order(actor)
                        .and_then(record_coord)
                {
                    let key = (source.x, source.y, source.z);
                    let bright = path.pending != -1 && index as i32 >= path.pending;
                    if let Some(&group) = groups.get(&key) {
                        // 635A40 chooses the first committed member; otherwise
                        // the final eligible member supplies the bright color.
                        let segment: &mut PlanningDrawSegment = &mut incoming[group];
                        if segment.bright {
                            segment.owner = owner;
                            segment.bright = bright;
                        }
                    } else {
                        groups.insert(key, incoming.len());
                        incoming.push(PlanningDrawSegment {
                            source,
                            target: coord,
                            owner,
                            bright,
                        });
                    }
                }
                if path.loop_start == Some(index) {
                    let source = self.planning.nodes[path.nodes.last().unwrap()]
                        .order(actor)
                        .and_then(record_coord);
                    if let Some(source) = source {
                        if let Some(segment) = &mut loop_segment {
                            if segment.bright {
                                segment.owner = owner;
                                segment.bright = path.loop_changed;
                            }
                        } else {
                            loop_segment = Some(PlanningDrawSegment {
                                source,
                                target: coord,
                                owner,
                                bright: path.loop_changed,
                            });
                        }
                    }
                }
            }
            // 6349B0 suppresses a separate loop stroke when an incoming group
            // already resolves to the first looping member's predecessor XYZ.
            if loop_segment.as_ref().is_some_and(|segment| {
                groups.contains_key(&(segment.source.x, segment.source.y, segment.source.z))
            }) {
                loop_segment = None;
            }
            let first_path = &self.planning.paths[&head];
            let marker_bright = !incoming.is_empty()
                || (first_path.pending != -1
                    && positions[&(head, node.id)] as i32 >= first_path.pending);
            nodes.push(PlanningDrawNode {
                id: node.id,
                coord,
                members,
                incoming,
                loop_segment,
                marker_owner,
                marker_bright,
                badge: node.order(head).and_then(planning_badge),
            });
        }
        let mut actors = Vec::new();
        for (&actor_id, path) in &self.planning.paths {
            if !allied(actor_id) {
                continue;
            }
            let Some(&node_id) = path.nodes.first() else {
                continue;
            };
            let Some(record) = self.planning.nodes[&node_id].order(actor_id) else {
                continue;
            };
            let Some(target) = record_coord(record) else {
                continue;
            };
            let Some(owner) = self.planning_actor_house(actor_id) else {
                continue;
            };
            let entity = self.substrate.entities.get(actor_id).unwrap();
            let actor_coord = crate::sim::movement::ground_pose::object_get_coords(
                entity,
                self.resolved_terrain.as_ref(),
            );
            use crate::sim::mission::MissionType;
            let mission = entity.mission.effective().known();
            // 63BE10/63BF68 suppress Guard and an idle AreaGuard. These
            // paths already have a plan, satisfying63BF7E's route gate.
            let area_guard_idle = mission == Some(MissionType::AreaGuard)
                && (entity.category() == crate::map::entities::EntityCategory::Structure
                    || (entity.navigation.nav_com().is_none()
                        && entity.attack_target.target().is_none()));
            let current_leg = (mission != Some(MissionType::Guard) && !area_guard_idle)
                .then(|| {
                    crate::sim::movement::ground_pose::current_destination_coords(
                        entity,
                        self,
                        self.resolved_terrain.as_ref(),
                    )
                })
                .flatten()
                .map(|target| PlanningDrawSegment {
                    source: actor_coord,
                    target,
                    owner,
                    bright: false,
                });
            let (has_mega_target, mega_target) = entity.mega_mission.planning_draw_target();
            let in_limbo = entity.lifecycle.in_limbo();
            let marker_only = if in_limbo {
                current_leg.as_ref().map(|leg| leg.target)
            } else {
                None
            };
            let mega_leg = (!in_limbo && has_mega_target && current_leg.is_some())
                .then(|| mega_target.and_then(|target| self.planning_mega_target_coords(target)))
                .flatten()
                .map(|target| PlanningDrawSegment {
                    source: current_leg.as_ref().unwrap().target,
                    target,
                    owner,
                    bright: false,
                });
            let planned_source = current_leg.as_ref().map_or(actor_coord, |leg| leg.target);
            let current_badge = if entity.mega_mission.is_attack_move() && !has_mega_target {
                Some(PlanningBadgeKind::AttackMove)
            } else if mission == Some(MissionType::AreaGuard) {
                Some(PlanningBadgeKind::AreaGuard)
            } else {
                None
            };
            actors.push(PlanningDrawActor {
                actor_id,
                owner,
                connection: (!(in_limbo && current_leg.is_some())
                    && (!has_mega_target || current_leg.is_none()))
                .then_some(PlanningDrawSegment {
                    source: planned_source,
                    target,
                    owner,
                    bright: path.pending == 0,
                }),
                badge: planning_badge(record),
                current_leg: if marker_only.is_some() {
                    None
                } else {
                    current_leg
                },
                current_badge,
                mega_leg,
                mega_badge: has_mega_target.then_some(PlanningBadgeKind::AttackMove),
                marker_only,
            });
        }
        //63B2F0 draws retained/hover rectangles independently of the allied
        // node drawing pass.63D650 likewise checks full node membership.
        let hovered_node =
            hover_point.and_then(|point| self.planning_hover_node_id(viewer, selected, point));
        let highlighted_node =
            retained_highlighted_node.filter(|id| self.planning.nodes.contains_key(id));
        PlanningDrawGraph {
            nodes,
            actors,
            #[cfg(any(test, feature = "test-support"))]
            hovered_node,
            highlighted_node,
            hovered_coord: hovered_node.and_then(|id| self.planning_node_coords(id)),
            highlighted_coord: highlighted_node.and_then(|id| self.planning_node_coords(id)),
            highlighted_members: highlighted_node
                .into_iter()
                .flat_map(|id| self.planning.nodes[&id].live_members.values().copied())
                .collect(),
        }
    }

    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn planning_node_at_cell(&self, owner: InternedId, cell: (u16, u16)) -> bool {
        self.planning_node_id_at_cell(owner, cell).is_some()
    }

    /// PlanMgr 0x00638E60's hover AC4CCC: the node under `point` (Node634220
    /// projects634550's live XYZ; its half-open hit box is16x16), kept only
    /// when Planning__GetInvalidNodeReason admits it for `selected`. A refused
    /// node goes to AC4C3C, which nothing reads. Residuals: the press-held
    /// node AC4C38 gate is not kept (it only holds between an admitted press
    /// and its release); 638E60 re-runs the gate only when the node under the
    /// mouse changes, while this re-checks it each frame against the current
    /// selection; and it hit-tests the first node in AC4C18 before its owner
    /// test, while this finds the first own node (allied plans only).
    pub fn planning_node_at_screen(
        &self,
        owner: InternedId,
        selected: &[u64],
        point: [i32; 2],
    ) -> Option<(u16, u16)> {
        self.planning_node_coords(self.planning_hover_node_id(owner, selected, point)?)
            .and_then(coord_cell)
    }

    fn planning_hover_node_id(
        &self,
        owner: InternedId,
        selected: &[u64],
        point: [i32; 2],
    ) -> Option<u64> {
        self.planning_node_id_at_screen(Some(owner), point)
            .filter(|&node| self.planning_invalid_node_reason(selected, node).is_none())
    }

    /// Planning__GetInvalidNodeReason 0x0063AC70 over the selection in order.
    /// The first object holding a token (Techno+0x514, whatever its owner)
    /// refuses with 1 when its loop is set (token+0x94), 5 when its path lacks
    /// the node, and 2 or 3 when the node is its last or second-to-last.
    /// FindItemIndex (vt+0x10) answers the first occurrence.
    fn planning_invalid_node_reason(&self, selected: &[u64], node: u64) -> Option<u8> {
        for path in selected
            .iter()
            .filter_map(|actor| self.planning.paths.get(actor))
        {
            if path.loop_start.is_some() {
                return Some(1);
            }
            let Some(index) = path.nodes.iter().position(|id| *id == node) else {
                return Some(5);
            };
            if index + 1 == path.nodes.len() {
                return Some(2);
            }
            if index + 2 == path.nodes.len() {
                return Some(3);
            }
        }
        None
    }

    /// PlanMgr 0x0063A8E0 on the left release: a node under it that 63AC70
    /// refuses consumes the release (0x0063AA65..0x0063AA7E), which then
    /// neither connects nor orders.
    pub fn planning_release_refused(&self, selected: &[u64], point: [i32; 2]) -> bool {
        self.planning_node_id_at_screen(None, point)
            .is_some_and(|node| self.planning_invalid_node_reason(selected, node).is_some())
    }

    /// PlanMgr 0x0063A5A0, which ScrollClass's left press (0x00693187) runs
    /// ahead of the capture. A node under the press that 63AC70 refuses
    /// answers InvalidNode; an admitted node passes. With no node hit,
    /// 0x00639DA0 walks the selection in order: the first object that is not
    /// a Techno answers NoUnknowns, the first Techno that cannot use waypoints
    /// answers by WhatAmI (Aircraft, Building, else NoUnknowns).
    /// TechnoClass::CanUseWaypoint 0x0070F070 asks the type (vt+0xA0):
    /// AircraftType 0x0041CB50 and BuildingType 0x00465910 answer false, and
    /// Infantry and Unit types jump to the base 0x00711E80, true.
    pub fn planning_press_refusal(
        &self,
        selected: &[u64],
        point: [i32; 2],
    ) -> Option<PlanningPressRefusal> {
        if let Some(node) = self.planning_node_id_at_screen(None, point) {
            return self
                .planning_invalid_node_reason(selected, node)
                .map(|_| PlanningPressRefusal::InvalidNode);
        }
        self.planning_selection_refusal(selected)
    }

    /// PlanMgr639DA0's selection-only check, also used directly by radar
    /// input653CD1 without a tactical node hit test or refusal message.
    pub fn planning_selection_refusal(&self, selected: &[u64]) -> Option<PlanningPressRefusal> {
        selected.iter().find_map(|&id| {
            match self
                .substrate
                .entities
                .get(id)
                .map(|entity| entity.category())
            {
                Some(EntityCategory::Aircraft) => Some(PlanningPressRefusal::NoAircraft),
                Some(EntityCategory::Structure) => Some(PlanningPressRefusal::NoBuildings),
                Some(EntityCategory::Unit | EntityCategory::Infantry) => None,
                _ => Some(PlanningPressRefusal::NoUnknowns),
            }
        })
    }

    /// `None` hit-tests every house's nodes, as the press and release
    /// loops over AC4B34 do (0x0063A5A0, 0x0063A8E0).
    fn planning_node_id_at_screen(
        &self,
        owner: Option<InternedId>,
        point: [i32; 2],
    ) -> Option<u64> {
        self.planning
            .nodes
            .values()
            .filter(|node| owner.is_none() || self.planning_node_house(node.id) == owner)
            .find_map(|node| {
                let coord = self.planning_node_coords(node.id)?;
                let (x, y) =
                    crate::util::lepton::absolute_leptons_to_screen(coord.x, coord.y, coord.z);
                let (x, y) = (x as i32, y as i32);
                (point[0] >= x - 8 && point[0] < x + 8 && point[1] >= y - 8 && point[1] < y + 8)
                    .then_some(node.id)
            })
    }

    /// The last node of `actor`'s remaining path: its token's node vector at
    /// count-1 (`+0x8`, `+0x14`), with no loop wrap.
    fn planning_tail_node(&self, actor: u64) -> Option<&PlanningNode> {
        let id = self.planning.paths.get(&actor)?.nodes.last()?;
        self.planning.nodes.get(id)
    }

    /// The members a Select of `actor` pulls in through the planning-node add
    /// (`0x00637840`, [`Self::planning_select_tail_node`]) in Planning Mode
    /// (`0x00AC4CF4`): the members of `actor`'s tail node (token `+0x514` via
    /// `0x00705D20`, node vector `+0x8` at count-1 `+0x14`) in encounter
    /// order (node `+0x4`, `+0x10`), whatever their owner, `actor` included.
    /// Native skips a member whose IsSelected `+0x83` is already set. The
    /// input selection owner expands its own Selects and applies that skip,
    /// TechnoClass::Select's gate, Unselect_All and the `0x00AC4CF6` guard
    /// to that expansion only; a Select one of their tag events starts is
    /// the game's own (see the residual at `apply_selection_snapshot`).
    pub fn planning_select_partners(&self, actor: u64, planning_mode: bool) -> Vec<u64> {
        if !planning_mode {
            return Vec::new();
        }
        self.planning_tail_node(actor)
            .map(|node| node.live_members.values().copied().collect())
            .unwrap_or_default()
    }

    /// `0x00637840` (Ghidra's "PlayPlanningSelectionVoices"), the last call of
    /// every successful `TechnoClass::Select` (`0x006FC020`, its only caller),
    /// for the game's own Selects ([`Self::techno_select`]). With the guard
    /// `0x00AC4CF6` clear (`0x00637840..0x00637847`) it sets it
    /// (`0x00637853`, Planning Mode or not), Selects (vt+0x14C) each
    /// [`Self::planning_select_partners`] member not yet selected (`+0x83`,
    /// read as the loop reaches it) in the local player's Planning Mode, read
    /// through the frame's [`super::LocalPlanningBoundary`], and clears it
    /// (`0x0063789F`). The guard spans the whole loop, so neither the
    /// partners' Selects nor a Select one of their Selected tag events
    /// starts expands again. The voices come from the members' own Selects.
    /// Native reads the member count once and each member from the live
    /// vector; this reads the members once, which differs only if a
    /// partner's tag event changes the node's membership mid-loop.
    pub(super) fn planning_select_tail_node(
        &mut self,
        actor: u64,
        voice_latch: bool,
        context: super::UninitContext<'_>,
    ) {
        if self.planning.select_expanding {
            return;
        }
        self.planning.select_expanding = true;
        let planning_mode = context.effects().local_planning_active();
        for member in self.planning_select_partners(actor, planning_mode) {
            let selected = self
                .substrate
                .entities
                .get(member)
                .is_some_and(|entity| entity.selection.is_selected());
            if !selected {
                self.techno_select(member, voice_latch, context);
            }
        }
        self.planning.select_expanding = false;
    }

    /// `0x0063AD50` expands each selected Techno's tail group (token via
    /// `0x00705D20`, node vector at count-1) in encounter order, whatever its
    /// owner (`0x0063ADB0..0x0063AEBC`); a selected actor without a path or
    /// tail contributes only itself (`0x0063ADF0`, `0x0063AEC0`). The
    /// connect event's receiver checks only the node's house.
    pub fn planning_connect_actors(&self, selected: &[u64]) -> Vec<u64> {
        let mut actors = Vec::new();
        let mut seen = BTreeSet::new();
        for &actor in selected {
            if let Some(node) = self.planning_tail_node(actor) {
                for &member in node.live_members.values() {
                    if seen.insert(member) {
                        actors.push(member);
                    }
                }
            } else if seen.insert(actor) {
                actors.push(actor);
            }
        }
        actors
    }

    fn planning_node_coords(&self, id: u64) -> Option<DriveCoord> {
        self.planning_target_coords(self.planning.nodes.get(&id)?.target()?)
    }

    /// Node6339B0 obtains its group from the first live actor, not event bytes.
    fn planning_node_house(&self, id: u64) -> Option<InternedId> {
        let actor = *self.planning.nodes.get(&id)?.live_members.values().next()?;
        self.planning_actor_house(actor)
    }

    // Plan6339B0 supplies the same effective house to node grouping and
    // commit637E37; controlled campaign houses map to the current player.
    fn planning_actor_house(&self, actor: u64) -> Option<InternedId> {
        let owner = self.substrate.entities.get(actor)?.owner();
        if self.house_is_human_player(owner) {
            self.session.current_house().or(Some(owner))
        } else {
            Some(owner)
        }
    }

    fn planning_target_coords(&self, target: MegaMissionTarget) -> Option<DriveCoord> {
        let kind = match target {
            MegaMissionTarget::Cell { x, y } => {
                crate::sim::combat::TargetKind::Cell(u16::try_from(x).ok()?, u16::try_from(y).ok()?)
            }
            MegaMissionTarget::Object { id } => {
                crate::sim::combat::TargetKind::Entity(u64::try_from(id).ok()?)
            }
            MegaMissionTarget::Null => return None,
        };
        crate::sim::movement::ground_pose::target_get_target_coords(
            kind,
            self,
            self.resolved_terrain.as_ref(),
        )
    }

    fn planning_mega_target_coords(
        &self,
        target: crate::sim::combat::TargetKind,
    ) -> Option<DriveCoord> {
        let cells = self
            .resolved_terrain
            .as_ref()
            .map(crate::map::resolved_terrain::NativeCellQuery::canonical);
        crate::sim::movement::ground_pose::target_get_coords(target, self, cells.as_ref())
    }

    fn planning_candidates(
        &self,
        owner: InternedId,
        cell: (u16, u16),
    ) -> impl Iterator<Item = u64> + '_ {
        self.planning
            .locations
            .range((cell, i32::MIN)..=(cell, i32::MAX))
            .flat_map(|(_, ids)| ids.iter().copied())
            .chain(self.planning.dynamic_nodes.iter().copied())
            .filter(move |id| self.planning_node_house(*id) == Some(owner))
    }

    #[cfg(any(test, feature = "test-support"))]
    fn planning_node_id_at_cell(&self, owner: InternedId, cell: (u16, u16)) -> Option<u64> {
        self.planning_candidates(owner, cell)
            .filter(|id| self.planning_node_coords(*id).and_then(coord_cell) == Some(cell))
            .min()
    }

    fn planning_node_at_coords(&self, owner: InternedId, coord: DriveCoord) -> Option<u64> {
        self.planning_candidates(owner, coord_cell(coord)?)
            .filter(|id| self.planning_node_coords(*id) == Some(coord))
            .min()
    }

    pub(super) fn planning_detach_actor(&mut self, id: u64) {
        if let Some(mut path) = self.planning.paths.remove(&id) {
            if !path.nodes.is_empty() {
                self.release_planning_charge(id);
                path.pending = -1;
                path.loop_start = None;
                path.loop_changed = false;
            }
            for node in std::mem::take(&mut path.nodes) {
                self.planning.remove_member(node, id);
            }
            path.executing = false;
            if path.current.is_some() || path.loop_changed {
                self.planning.paths.insert(id, path);
            }
        }
    }

    fn release_planning_charge(&mut self, actor: u64) {
        if let Some(index) = self
            .planning_actor_house(actor)
            .and_then(|house| self.houses.index_of(house))
            && let Some(charge) = self.planning.house_charges.get_mut(index)
        {
            *charge = charge.wrapping_sub(1);
            if *charge < 0 {
                *charge = 0;
            }
        }
    }

    pub(super) fn initialize_planning_token(&mut self, actor: u64) {
        self.planning.register_token(actor);
    }

    /// Techno6F4507..6F4531, after Foot disposal and before voice/Anim tails.
    pub(super) fn destroy_planning_token(&mut self, actor: u64) {
        self.planning_detach_actor(actor);
        self.planning.paths.remove(&actor);
        self.planning.registered_tokens.remove(&actor);
        self.planning.token_order.retain(|id| *id != actor);
    }

    /// ChangeOwner638840 clears future nodes/cursors, preserving +1C/+1D.
    pub(super) fn planning_change_owner(&mut self, actor: u64) {
        if self.planning_has_remaining_nodes(actor) {
            self.release_planning_charge(actor);
        }
        let Some(path) = self.planning.paths.get_mut(&actor) else {
            return;
        };
        let nodes = std::mem::take(&mut path.nodes);
        path.pending = -1;
        path.loop_start = None;
        path.loop_changed = false;
        for node in nodes {
            self.planning.remove_member(node, actor);
        }
    }

    /// PlanMgr637270: after frame commit and deferred finalization, walk
    /// occurrences backwards. Both predicate and removal use the FIRST
    /// matching actor member, even when a path repeats the shared node.
    pub(super) fn prune_invalid_planning_nodes(&mut self) {
        let actors = self.planning.token_order.clone();
        for actor in actors {
            let Some(path) = self.planning.paths.get(&actor) else {
                continue;
            };
            let count = path.nodes.len();
            for index in (0..count).rev() {
                let node = self.planning.paths[&actor].nodes[index];
                let record = self
                    .planning
                    .nodes
                    .get(&node)
                    .and_then(|node| node.order(actor));
                let token = record
                    .filter(|record| record.opcode() == 4)
                    .and_then(|record| {
                        // event+E/+12 is primary; destination+13/+17 is chosen
                        // only when the primary KIND is zero, never on failure.
                        let offset = if record.payload()[11] != 0 { 7 } else { 12 };
                        read_target(record, offset)
                    });
                let target = match token {
                    Some(MegaMissionTarget::Cell { x, y }) => {
                        Some(crate::sim::combat::TargetKind::Cell(x as u16, y as u16))
                    }
                    Some(MegaMissionTarget::Object { id }) => u64::try_from(id)
                        .ok()
                        .map(crate::sim::combat::TargetKind::Entity),
                    _ => None,
                };
                if self.resolve_event_token(target).is_some() {
                    continue;
                }
                self.planning
                    .paths
                    .get_mut(&actor)
                    .unwrap()
                    .remove_occurrence(index);
                self.planning.remove_member(node, actor);
            }
        }
    }

    /// DispatchLatch6386E0/638826: flag2 clears only the in-flight latch.
    pub(super) fn planning_acknowledge(&mut self, actor: u64) {
        if let Some(path) = self.planning.paths.get_mut(&actor) {
            path.executing = false;
        }
    }

    pub(super) fn apply_planning_record(
        &mut self,
        owner_name: &str,
        record: &CommandRecord,
        rules: &crate::rules::ruleset::RuleSet,
    ) -> bool {
        let Some(owner) = self.interner.get(owner_name) else {
            return false;
        };
        match record.opcode() {
            0x2b => {
                // Stable IDs are allocated in construction order. Logic order
                // changes on limbo/re-entry and is not Techno vector order.
                let actors: Vec<_> = self.planning.paths.keys().copied().collect();
                for actor in actors {
                    if self.planning_actor_house(actor) != Some(owner) {
                        continue;
                    }
                    let Some(path) = self.planning.paths.get_mut(&actor) else {
                        continue;
                    };
                    let before = !path.nodes.is_empty() && (path.pending == -1 || path.pending > 0);
                    path.commit();
                    if !before && self.planning_commit_starts(actor) {
                        self.planning_start_actor(actor, rules);
                    }
                }
                true
            }
            0x2a => self.planning_connect(owner, record, rules),
            0x2c => {
                let Some(coord) = self.planning_target_coords(
                    read_target(record, 0).unwrap_or(MegaMissionTarget::Null),
                ) else {
                    return false;
                };
                let Some(node) = self.planning_node_at_coords(owner, coord) else {
                    return false;
                };
                // NodeDelete6380A6 captures the FIRST member path successor
                // before deletion.6380EE hands it to the local presentation
                // owner; hover AC4C68 and retained highlight AC4BF0 differ.
                let next_node = self.planning.nodes[&node]
                    .live_members
                    .values()
                    .next()
                    .and_then(|actor| self.planning.paths.get(actor))
                    .and_then(|path| {
                        let index = path.nodes.iter().position(|id| *id == node)?;
                        path.node_at(i32::try_from(index + 1).ok()?)
                    });
                let members: Vec<_> = self.planning.nodes[&node]
                    .live_members
                    .values()
                    .rev()
                    .copied()
                    .collect();
                for actor in members {
                    let Some(path) = self.planning.paths.get_mut(&actor) else {
                        continue;
                    };
                    if let Some(index) = path.nodes.iter().position(|id| *id == node) {
                        path.remove_occurrence(index);
                    }
                }
                self.planning.remove_node(node);
                self.lifecycle_outputs.emit(
                    super::lifecycle::LifecycleOutput::PlanningNodeDeleted {
                        owner,
                        node_id: node,
                        next_node,
                    },
                );
                true
            }
            4 if record.payload()[22] == 1 => {
                let accepted = self.planning_append(owner, record);
                if accepted {
                    self.sound_events
                        .emit(super::SimSoundEvent::PlanningNodeAdded { owner });
                }
                accepted
            }
            _ => false,
        }
    }

    fn planning_append(&mut self, owner: InternedId, record: &CommandRecord) -> bool {
        let mut ordinary = record.clone();
        ordinary.payload_mut()[22] = 0;
        let Some(order) = MegaMissionRecord::decode(&ordinary) else {
            return false;
        };
        let Ok(actor) = u64::try_from(order.source_id) else {
            return false;
        };
        let Some(entity) = self.substrate.entities.get(actor) else {
            return false;
        };
        //638120 requires a resolved actor; ordinary-event admission is deferred
        // until its generated flag2 is dispatched, even after capture.
        // Append63821F accepts only disposition -1 from638CE0. A Cell
        // Attack/AreaGuard tail is terminal even while it is still being edited.
        if self
            .planning
            .paths
            .get(&actor)
            .and_then(|path| path.nodes.last())
            .and_then(|id| self.planning.nodes.get(id))
            .and_then(|node| node.order(actor))
            .is_some_and(|record| {
                let mut ordinary = record.clone();
                ordinary.payload_mut()[22] = 0;
                matches!(
                    MegaMissionRecord::decode(&ordinary).map(|order| order.order),
                    Some(MegaMissionOrder::Attack {
                        target: MegaMissionTarget::Cell { .. }
                    }) | Some(MegaMissionOrder::AreaGuard {
                        post: MegaMissionTarget::Cell { .. } | MegaMissionTarget::Null
                    })
                )
            })
        {
            return false;
        }
        let Some(target) = planning_record_target(record) else {
            return false;
        };
        let Some(coord) = self.planning_target_coords(target) else {
            return false;
        };
        let Some(cell) = coord_cell(coord) else {
            return false;
        };
        // PlanMgr638B70 rejects AreaGuard with no target or the actor's
        // current CellStruct; this is separate from commit admission.
        if matches!(order.order, MegaMissionOrder::AreaGuard { .. })
            && cell == (entity.position.rx(), entity.position.ry())
        {
            return false;
        }
        // 6384DC compares 10h directly in this active append path.
        self.planning.register_token(actor);
        if self
            .planning
            .paths
            .get(&actor)
            .is_some_and(|path| path.nodes.len() == 16 || path.loop_start.is_some())
        {
            return false;
        }
        let frame = record.frame_stamp();
        let shared = self
            .planning_candidates(owner, cell)
            .filter(|id| {
                let node = &self.planning.nodes[id];
                node.frame == frame
                    && !node.orders.contains_key(&actor)
                    && self.planning_node_coords(*id) == Some(coord)
            })
            .min();
        let id = if let Some(id) = shared {
            id
        } else {
            let id = self.planning.next_node;
            self.planning.next_node += 1;
            self.planning.nodes.insert(
                id,
                PlanningNode {
                    id,
                    frame,
                    orders: BTreeMap::new(),
                    member_order: Vec::new(),
                    live_members: BTreeMap::new(),
                },
            );
            id
        };
        self.planning.unindex_node(id);
        if !self.planning_has_remaining_nodes(actor)
            && let Ok(index) = usize::try_from(record.house_id())
            && let Some(charge) = self.planning.house_charges.get_mut(index)
        {
            *charge = charge.wrapping_add(1);
        }
        let node = self.planning.nodes.get_mut(&id).unwrap();
        node.insert_order(actor, record.clone());
        self.planning.index_node(id);
        let path = self
            .planning
            .paths
            .entry(actor)
            .or_insert_with(|| PlanningPath {
                nodes: Vec::new(),
                pending: 0,
                loop_start: None,
                loop_changed: false,
                executing: false,
                current: None,
            });
        if path.pending == -1 {
            path.pending = path.nodes.len() as i32;
        }
        path.nodes.push(id);
        true
    }

    fn planning_connect(
        &mut self,
        owner: InternedId,
        record: &CommandRecord,
        rules: &crate::rules::ruleset::RuleSet,
    ) -> bool {
        let Some(MegaMissionTarget::Object { id }) = read_target(record, 0) else {
            return false;
        };
        let Ok(actor) = u64::try_from(id) else {
            return false;
        };
        //639800 resolves the actor; owner/lifetime are not receiver gates.
        if self.substrate.entities.get(actor).is_none() {
            return false;
        }
        let Some(coord) =
            read_target(record, 5).and_then(|target| self.planning_target_coords(target))
        else {
            return false;
        };
        let Some(node_id) = self.planning_node_at_coords(owner, coord) else {
            return false;
        };
        self.planning.register_token(actor);
        if let Some(path) = self.planning.paths.get_mut(&actor)
            && let Some(index) = path.nodes.iter().position(|id| *id == node_id)
        {
            path.loop_start = Some(index);
            path.loop_changed = true;
            self.planning_promote_patrol(actor, rules);
            return true;
        }
        let node = &self.planning.nodes[&node_id];
        let Some(source_actor) = node.live_members.values().next().copied() else {
            return false;
        };
        let Some(source) = self.planning.paths.get(&source_actor).cloned() else {
            return false;
        };
        let index = source.nodes.iter().position(|id| *id == node_id).unwrap();
        let mut suffix = source.nodes[index..].to_vec();
        if let Some(start) = source.loop_start
            && index >= start
        {
            suffix.extend_from_slice(&source.nodes[start..index]);
        }
        for node_id in &suffix {
            self.planning.unindex_node(*node_id);
            let node = self.planning.nodes.get_mut(node_id).unwrap();
            //639800 copies each suffix member for the selected source path,
            // not the node's unrelated first member.634290 finds its first.
            let Some(template) = node.order(source_actor) else {
                continue;
            };
            let mut order = template.clone();
            let Ok(id) = i32::try_from(actor) else {
                return false;
            };
            order.payload_mut()[0..4].copy_from_slice(&id.to_le_bytes());
            node.insert_order(actor, order);
            self.planning.index_node(*node_id);
        }
        let path = self
            .planning
            .paths
            .entry(actor)
            .or_insert_with(|| PlanningPath {
                nodes: Vec::new(),
                pending: 0,
                loop_start: None,
                loop_changed: false,
                executing: false,
                current: None,
            });
        if path.pending == -1 {
            path.pending = path.nodes.len() as i32;
        }
        let clicked_index = path.nodes.len();
        path.nodes.extend(suffix);
        path.loop_start = source.loop_start.and_then(|start| {
            if index >= start {
                Some(clicked_index)
            } else {
                path.nodes.iter().position(|id| *id == source.nodes[start])
            }
        });
        path.loop_changed = true;
        true
    }

    /// gamemd.exe 639CB0, called by self-connect 639740 at 6397E5:
    /// any attack-move-capable member of the FIRST node enables Move ->
    /// AttackMove for every membership in the loop suffix, including shared
    /// actors and repeated occurrences. Other missions and the prefix stay put.
    /// The native AC4CC8 local feedback latch is not simulation state here;
    /// applying this idempotent conversion per received connect keeps replay
    /// independent of the local player's selection and presentation updates.
    fn planning_promote_patrol(&mut self, actor: u64, rules: &crate::rules::ruleset::RuleSet) {
        let Some(path) = self.planning.paths.get(&actor) else {
            return;
        };
        let Some(start) = path.loop_start else {
            return;
        };
        let enabled = path
            .nodes
            .first()
            .and_then(|id| self.planning.nodes.get(id))
            .is_some_and(|node| {
                node.live_members
                    .values()
                    .any(|member| self.techno_can_attack_move(*member, rules))
            });
        if !enabled {
            return;
        }
        for id in &path.nodes[start..] {
            let node = self.planning.nodes.get_mut(id).unwrap();
            for orders in node.orders.values_mut() {
                for order in orders {
                    if order.record.payload()[5] == 2 {
                        order.record.payload_mut()[5] = 29;
                    }
                }
            }
        }
    }

    /// PlanMgr638C70 tests effective mission and the TarCom token type.
    fn planning_commit_starts(&self, actor: u64) -> bool {
        use crate::sim::{combat::TargetKind, mission::MissionType};
        let Some(entity) = self.substrate.entities.get(actor) else {
            return false;
        };
        match entity.mission.effective().known() {
            Some(MissionType::Guard) => true,
            Some(MissionType::Attack) => entity
                .attack_target
                .target()
                .is_some_and(|target| matches!(target, TargetKind::Cell(..))),
            Some(MissionType::AreaGuard) => entity
                .attack_target
                .target()
                .is_none_or(|target| matches!(target, TargetKind::Cell(..))),
            _ => false,
        }
    }

    /// Techno709A40 -> vt+0x430 -> 6385C0. Called only after the existing
    /// RefreshMegaMission owner declined to resume attack-move. vt+0x430 is
    /// the not-warping-in test (`0x00705D50`) for every class but Aircraft,
    /// whose override (`0x0041B9E0`) answers its `+0x6D5` byte,
    /// `aircraft_arrival` here (clear only inside Mission_Attack state 10's
    /// idle call, [`crate::sim::aircraft::enter_idle_mode_after_attack`]).
    pub(crate) fn planning_enter_idle(
        &mut self,
        actor: u64,
        rules: &crate::rules::ruleset::RuleSet,
        aircraft_arrival: bool,
    ) -> bool {
        let Some(entity) = self.substrate.entities.get(actor) else {
            return false;
        };
        let admitted = if entity.category() == crate::map::entities::EntityCategory::Aircraft {
            aircraft_arrival
        } else {
            !entity.is_warping_in()
        };
        admitted && self.planning_start_actor(actor, rules)
    }

    pub(crate) fn planning_start_actor(
        &mut self,
        actor: u64,
        rules: &crate::rules::ruleset::RuleSet,
    ) -> bool {
        let Some(path) = self.planning.paths.get(&actor) else {
            return false;
        };
        if path.executing || path.nodes.is_empty() || path.pending == 0 {
            return false;
        }
        let node_id = path.nodes[0];
        let Some(mut record) = self
            .planning
            .nodes
            .get(&node_id)
            .and_then(|node| node.order(actor).cloned())
        else {
            return false;
        };
        let Some(_owner) = usize::try_from(record.house_id())
            .ok()
            .and_then(|index| self.houses.order().get(index))
            .copied()
        else {
            return false;
        };
        // `0x00638602..0x0063860C`: Queue_Mission(Deliberate, 1) through
        // vt+0x1E8 commences it at once when the object is ready, before
        // 0x00636590 consumes the node and the planned event is staged.
        // Neither this owner nor its container consumes RNG.
        let _ = self.mission_queue_exact(
            actor,
            crate::sim::mission::MissionId::from_known(
                crate::sim::mission::MissionType::Deliberate,
            ),
            1,
            self.session.binary_frame(),
            &crate::sim::mission::authority::LiveReadyInputProvider { rules },
        );
        record.payload_mut()[22] = 2;
        record.set_issue_header(
            4,
            i32::from(record.house_id()),
            self.session.binary_frame().wrapping_add(1) as i32,
        );
        let path = self.planning.paths.get_mut(&actor).unwrap();
        let keep_member = path.loop_start == Some(0);
        path.consume();
        path.current = Some(record.clone());
        path.executing = true;
        if path.nodes.is_empty() {
            self.release_planning_charge(actor);
        }
        // 636590 retains the first member only for loop-head rotation. A
        // nonloop occurrence removes one member even if this node recurs later.
        if !keep_member {
            self.planning.remove_member(node_id, actor);
        }
        // 637550 ->652230: deferred publication stamps frame+1. Reverse
        // pending traversal plus ring prepend restores generation order.
        self.queue_command(CommandEnvelope::synchronized(
            record,
            self.session.tick().saturating_add(2),
        ));
        true
    }
}

fn valid_planning_record(record: &CommandRecord, actor: u64, flag: u8) -> bool {
    if record.opcode() != 4 || record.payload()[22] != flag {
        return false;
    }
    let mut ordinary = record.clone();
    ordinary.payload_mut()[22] = 0;
    MegaMissionRecord::decode(&ordinary)
        .is_some_and(|order| u64::try_from(order.source_id) == Ok(actor))
}

fn planning_badge(record: &CommandRecord) -> Option<PlanningBadgeKind> {
    match record.payload()[5] {
        11 => Some(PlanningBadgeKind::AreaGuard),
        29 => Some(PlanningBadgeKind::AttackMove),
        _ => None,
    }
}

/// Event63B050 chooses secondary, auxiliary, then destination, independent
/// of mission. Null tokens do not acquire the auxiliary value's identity.
fn planning_record_target(record: &CommandRecord) -> Option<MegaMissionTarget> {
    [7, 17, 12].into_iter().find_map(|offset| {
        read_target(record, offset).filter(|target| *target != MegaMissionTarget::Null)
    })
}

fn coord_cell(coord: DriveCoord) -> Option<(u16, u16)> {
    Some((
        u16::try_from(coord.x / 256).ok()?,
        u16::try_from(coord.y / 256).ok()?,
    ))
}

fn read_target(record: &CommandRecord, offset: usize) -> Option<MegaMissionTarget> {
    let payload = record.payload();
    MegaMissionTarget::from_token(
        i32::from_le_bytes(payload.get(offset..offset + 4)?.try_into().ok()?),
        *payload.get(offset + 4)?,
    )
}

#[cfg(test)]
#[path = "planning_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "planning_mission_tests.rs"]
mod mission_tests;
