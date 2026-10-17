//! Active YR PlanMgr: shared nodes and per-Techno path tokens.
//! Native identities: append638120, commit636CE0, consume636590,
//! lookup636E60, connect639740/639800 and delete639BC0 (gamemd.exe).
//! The obsolete TS House path slots and Foot Formation+520 are unrelated.

use super::Simulation;
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
}

impl<'de> Deserialize<'de> for PlanningState {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        struct StoredPlanningState {
            next_node: u64,
            nodes: BTreeMap<u64, PlanningNode>,
            paths: BTreeMap<u64, PlanningPath>,
        }
        let stored = StoredPlanningState::deserialize(deserializer)?;
        let mut state = Self {
            next_node: stored.next_node,
            nodes: stored.nodes,
            locations: BTreeMap::new(),
            dynamic_nodes: BTreeSet::new(),
            paths: stored.paths,
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
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) id: u64,
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
        let mut memberships = BTreeMap::<(u64, u64), usize>::new();
        for (&actor, path) in &self.paths {
            if path.nodes.is_empty() && !path.executing {
                return Err("empty planning path");
            }
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
        if self.next_node == 0 && self.nodes.is_empty() && self.paths.is_empty() {
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
    pub(crate) fn validate_planning_references(&self) -> Result<(), &'static str> {
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
                    .and_then(|index| self.session.house_order.get(index))
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
        let house = i8::try_from(
            self.session
                .house_order
                .iter()
                .position(|&id| id == owner)?,
        )
        .ok()?;
        CommandRecord::encode(
            opcode,
            i32::from(house),
            self.session.binary_frame as i32,
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

    /// PlanMgr639130, called by LeftUp4ABF7F before dispatch: a House may
    /// retain128 nonempty actor paths, including the newly selected actors.
    /// Every entity in this store is a Techno (+14 bit0); missing selection
    /// references do not contribute, and an existing nonempty path adds none.
    ///
    /// AC4B84 counts nonempty paths, not retained empty executing paths:
    /// append638470 increments empty->nonempty; consume636718..636760 and
    /// detach6386E0 decrement at the transition to empty. ChangeHouse701585
    /// detaches before replacing the owner. Derive that count from this owner
    /// using its existing effective-House projection, without a second counter.
    /// Source: original gamemd.exe / building_manual_primary.md and its
    /// executed planning_admission controls. Delayed old-issuer appends after
    /// capture remain a separate native accounting bound.
    pub fn planning_selection_has_capacity(&self, owner: InternedId, selected: &[u64]) -> bool {
        let retained = self
            .planning
            .paths
            .iter()
            .filter(|(actor, path)| {
                !path.nodes.is_empty() && self.planning_actor_house(**actor) == Some(owner)
            })
            .count();
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
        retained.saturating_add(additional) <= 0x80
    }

    pub fn encode_plan_commit_record(&self, owner: InternedId) -> Option<CommandRecord> {
        // Event constructor4C66C0: header only, no route packet.
        self.planning_header(owner, 0x2b)
    }

    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn encode_plan_connect_record(
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
        let node = self.planning_node_id_at_screen(owner, point)?;
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
        let node = self.planning_node_id_at_screen(owner, point)?;
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
    pub fn planning_draw_graph(
        &self,
        viewer: InternedId,
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
                    &self.house_alliances,
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
                #[cfg(any(test, feature = "test-support"))]
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
                        &self.substrate.entities,
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
            hover_point.and_then(|point| self.planning_node_id_at_screen(viewer, point));
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

    /// Node634220 projects634550's live XYZ; its half-open hit box is16x16.
    pub fn planning_node_at_screen(
        &self,
        owner: InternedId,
        point: [i32; 2],
    ) -> Option<(u16, u16)> {
        self.planning_node_coords(self.planning_node_id_at_screen(owner, point)?)
            .and_then(coord_cell)
    }

    fn planning_node_id_at_screen(&self, owner: InternedId, point: [i32; 2]) -> Option<u64> {
        self.planning
            .nodes
            .values()
            .filter(|node| self.planning_node_house(node.id) == Some(owner))
            .find_map(|node| {
                let coord = self.planning_node_coords(node.id)?;
                let (x, y) =
                    crate::util::lepton::absolute_leptons_to_screen(coord.x, coord.y, coord.z);
                let (x, y) = (x as i32, y as i32);
                (point[0] >= x - 8 && point[0] < x + 8 && point[1] >= y - 8 && point[1] < y + 8)
                    .then_some(node.id)
            })
    }

    /// Connect639740 expands each selected actor's tail group in encounter
    /// order. A selected actor without a path contributes only itself.
    pub fn planning_connect_actors(&self, owner: InternedId, selected: &[u64]) -> Vec<u64> {
        let mut actors = Vec::new();
        let mut seen = BTreeSet::new();
        for &actor in selected {
            let tail = self
                .planning
                .paths
                .get(&actor)
                .filter(|_| {
                    self.substrate
                        .entities
                        .get(actor)
                        .is_some_and(|entity| entity.owner() == owner)
                })
                .and_then(|path| path.nodes.last())
                .and_then(|id| self.planning.nodes.get(id));
            if let Some(node) = tail {
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
            self.session.current_house
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
            &self.substrate.entities,
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
        crate::sim::movement::ground_pose::target_get_coords(
            target,
            &self.substrate.entities,
            cells.as_ref(),
        )
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
        if let Some(path) = self.planning.paths.remove(&id) {
            for node in path.nodes {
                self.planning.remove_member(node, id);
            }
        }
    }

    /// DispatchLatch6386E0/638826: flag2 clears only the in-flight latch.
    pub(super) fn planning_acknowledge(&mut self, actor: u64) {
        if let Some(path) = self.planning.paths.get_mut(&actor) {
            path.executing = false;
            if path.nodes.is_empty() {
                self.planning.paths.remove(&actor);
            }
        }
    }

    pub(super) fn apply_planning_record(
        &mut self,
        owner_name: &str,
        record: &CommandRecord,
        rules: Option<&crate::rules::ruleset::RuleSet>,
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
                        self.planning_start_actor(actor);
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
                        path.nodes.remove(index);
                        if path.nodes.is_empty() {
                            path.pending = -1;
                        } else if path.pending > 0 && path.pending > index as i32 {
                            path.pending -= 1;
                        }
                        if let Some(start) = path.loop_start {
                            path.loop_start = if start > index {
                                Some(start - 1)
                            } else {
                                Some(start)
                            };
                            if path
                                .loop_start
                                .is_some_and(|start| path.nodes.len().saturating_sub(start) < 3)
                            {
                                path.loop_start = None;
                            }
                        }
                    }
                }
                self.planning.remove_node(node);
                self.planning
                    .paths
                    .retain(|_, path| !path.nodes.is_empty() || path.executing);
                self.lifecycle_outputs.push(
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
                        .push(super::SimSoundEvent::PlanningNodeAdded { owner });
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
        rules: Option<&crate::rules::ruleset::RuleSet>,
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
        if let Some(path) = self.planning.paths.get_mut(&actor)
            && let Some(index) = path.nodes.iter().position(|id| *id == node_id)
        {
            path.loop_start = Some(index);
            path.loop_changed = true;
            if let Some(rules) = rules {
                self.planning_promote_patrol(actor, rules);
            }
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

    /// Techno709A40 ->705D50 (!warp-in) ->6385C0. Called only after the
    /// existing RefreshMegaMission owner declined to resume attack-move.
    pub(crate) fn planning_enter_idle(&mut self, actor: u64) -> bool {
        if self
            .substrate
            .entities
            .get(actor)
            .is_none_or(|entity| entity.is_warping_in())
        {
            return false;
        }
        self.planning_start_actor(actor)
    }

    pub(crate) fn planning_start_actor(&mut self, actor: u64) -> bool {
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
        let Some(owner) = usize::try_from(record.house_id())
            .ok()
            .and_then(|index| self.session.house_order.get(index))
            .copied()
        else {
            return false;
        };
        record.payload_mut()[22] = 2;
        record.set_issue_header(
            4,
            i32::from(record.house_id()),
            self.session.binary_frame.wrapping_add(1) as i32,
        );
        let path = self.planning.paths.get_mut(&actor).unwrap();
        let keep_member = path.loop_start == Some(0);
        path.consume();
        path.current = Some(record.clone());
        path.executing = true;
        // 636590 retains the first member only for loop-head rotation. A
        // nonloop occurrence removes one member even if this node recurs later.
        if !keep_member {
            self.planning.remove_member(node_id, actor);
        }
        // Native638623 queues Deliberate without Commence, before staging the
        // planned event. Neither this owner nor its container consumes RNG.
        let _ = self.mission_queue_exact(
            actor,
            crate::sim::mission::MissionId::from_known(
                crate::sim::mission::MissionType::Deliberate,
            ),
            0,
            self.session.binary_frame,
            &crate::sim::mission::authority::EntityReadyInputProvider,
        );
        // 637550 ->652230: deferred publication stamps frame+1. Reverse
        // pending traversal plus ring prepend restores generation order.
        self.queue_command(CommandEnvelope::new(
            owner,
            self.session.tick.saturating_add(2),
            Command::PlanningRecord { record },
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
