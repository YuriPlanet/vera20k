//! VERA ferry order: a transport carries units from a pick-up spot to a drop
//! spot and comes back for more, until it is given something else to do.
//!
//! The player selects transports that stand at the pick-up spot, presses the
//! Ferry key and clicks the drop spot. Each transport then repeats four legs:
//!
//! - **Loading:** it waits at the pick-up spot. Own units that come to a stop
//!   inside the pick-up circle are told to board.
//! - **To the drop:** once it is full, or it holds someone and nobody has been
//!   boarding for [`QUIET_TICKS`], it moves to the free cell nearest the drop
//!   spot within the drop zone.
//! - **Unloading:** the transport's ordinary self-click unload.
//! - **To the pick-up:** empty again, it moves back.
//!
//! Like AutoLoad (`input::autoload`) this is input policy. It only emits the
//! orders a click produces: Enter (Foot `ClickedAction` `0x004D74E0`), a Cell
//! Move through the click's own goal resolver
//! ([`ordinary_cell_move_goal`]) and the mobile self-click Unload
//! (`0x004D75E1`). They travel through the ordinary scheduler, so a replay or
//! another client sees the same commands, and nothing here mutates
//! simulation state. The engine's own admission
//! ([`can_enter_transport`], `UnitClass::Receive_Radio @ 0x00737430`) decides
//! every boarding.
//!
//! A route is local input state: it is not saved, the computer does not use
//! it, and it ends when its transport is lost, is stopped (the Stop key),
//! gets a new ferry order or is found away from its route.

use crate::app::AppState;
use crate::app::input::commands::{
    ordinary_cell_move_goal, preferred_local_owner_name, try_schedule_command,
};
use crate::app::input::dispatch::selected_stable_ids_in_order;
use crate::app::input::transport_orders::unload_command;
use crate::map::entities::EntityCategory;
use crate::rules::object_type::ObjectType;
use crate::rules::overlay_types::OverlayTypeRegistry;
use crate::rules::ruleset::RuleSet;
use crate::sim::combat::TargetKind;
use crate::sim::command::Command;
use crate::sim::game_entity::GameEntity;
use crate::sim::intern::InternedId;
use crate::sim::mission::MissionType;
use crate::sim::movement::locomotor::MovementLayer;
use crate::sim::occupancy::CellObjectMember;
use crate::sim::passenger::{PassengerCargo, can_enter_transport};
use crate::sim::world::Simulation;

/// A map cell `(rx, ry)`.
type Cell = (u16, u16);

/// Logic frames between two looks at the routes.
const CHECK_INTERVAL: u64 = 8;
/// Radius of the pick-up circle in cells, measured between cell centres.
pub(crate) const PICKUP_RADIUS_CELLS: i64 = 4;
/// Radius of the drop zone in cells: a transport leaving for the drop spot
/// heads for the free cell nearest the spot within it.
pub(crate) const DROP_RADIUS_CELLS: i64 = 3;
/// A loading transport farther than this from its pick-up spot has been
/// driven away.
const LEASH_CELLS: i64 = PICKUP_RADIUS_CELLS + 3;
/// A drop spot this close to the pick-up spot is refused: units set down
/// inside the circle would board again.
const MIN_ROUTE_CELLS: i64 = PICKUP_RADIUS_CELLS + 3;
/// A transport that stops within this many cells of a leg's end has arrived.
const ARRIVE_CELLS: i64 = 3;
/// Logic frames with nobody boarding before a partly loaded transport leaves.
const QUIET_TICKS: u64 = 75;
/// Logic frames an order may take to show on the transport before it is sent
/// again.
const RESEND_TICKS: u64 = 60;
/// Logic frames a unit on its way aboard is given to get there: it holds its
/// transport that long, and is not told to board again before.
const BOARDING_TICKS: u64 = 300;
/// Logic frames a fresh boarding order counts as boarding before the unit's
/// own mission shows it.
const ORDER_GRACE_TICKS: u64 = 30;
/// A crossing may land this many cells outside the drop zone when every cell
/// in it is taken.
const DROP_OVERFLOW_CELLS: i64 = 4;
/// Upper bounds on the routes kept and the boarding orders of one look.
const MAX_ROUTES: usize = 64;
const MAX_BOARD_ORDERS: usize = 32;

fn cell_dist_sq(a: Cell, b: Cell) -> i64 {
    let dx = i64::from(a.0) - i64::from(b.0);
    let dy = i64::from(a.1) - i64::from(b.1);
    dx * dx + dy * dy
}

fn within(a: Cell, b: Cell, cells: i64) -> bool {
    cell_dist_sq(a, b) <= cells * cells
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Leg {
    Loading,
    ToDrop,
    Unloading,
    ToPickup,
}

/// What one look reads from a route's live transport.
#[derive(Debug, Clone, Copy)]
pub(crate) struct TransportFacts {
    pub cell: Cell,
    /// On Guard with no destination and a locomotor at rest.
    pub idle: bool,
    /// Passengers aboard.
    pub passengers: u32,
    /// The hold has no free `Size=` unit left.
    pub full: bool,
    /// Someone is on the way aboard.
    pub boarding: bool,
}

/// The order a look sends for a route's transport.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FerryAction {
    None,
    MoveTo(Cell),
    Unload,
    /// The route is over.
    End,
}

/// One transport's route and where it is on it.
#[derive(Debug, Clone)]
pub(crate) struct FerryRoute {
    transport: u64,
    pickup: Cell,
    drop: Cell,
    /// Where the current Move leg ends: the pick-up spot, or the cell of the
    /// drop zone chosen for this crossing.
    target: Cell,
    leg: Leg,
    /// Logic frame the current leg's order was last sent.
    ordered_at: u64,
    /// The transport has been seen busy since `ordered_at`.
    seen_busy: bool,
    /// Loading: the last logic frame someone was boarding or the hold changed.
    last_boarding: u64,
    last_count: u32,
}

impl FerryRoute {
    pub(crate) fn new(transport: u64, pickup: Cell, drop: Cell, passengers: u32, now: u64) -> Self {
        Self {
            transport,
            pickup,
            drop,
            target: drop,
            leg: Leg::Loading,
            ordered_at: now,
            seen_busy: false,
            last_boarding: now,
            last_count: passengers,
        }
    }

    fn begin(&mut self, leg: Leg, now: u64) {
        self.leg = leg;
        self.ordered_at = now;
        self.seen_busy = false;
    }

    /// Advance the route by one look and name the order to send.
    pub(crate) fn step(&mut self, transport: &TransportFacts, now: u64) -> FerryAction {
        if !transport.idle {
            self.seen_busy = true;
        }
        match self.leg {
            Leg::Loading => {
                if !within(transport.cell, self.pickup, LEASH_CELLS) {
                    return FerryAction::End;
                }
                if transport.boarding || transport.passengers != self.last_count {
                    self.last_boarding = now;
                    self.last_count = transport.passengers;
                }
                let quiet = now.saturating_sub(self.last_boarding) >= QUIET_TICKS;
                if transport.passengers > 0 && (transport.full || (!transport.boarding && quiet)) {
                    self.begin(Leg::ToDrop, now);
                    self.target = self.drop;
                    return FerryAction::MoveTo(self.drop);
                }
                FerryAction::None
            }
            Leg::ToDrop => match self.arrival(transport, self.target, now) {
                Arrival::Travelling => FerryAction::None,
                Arrival::Arrived => {
                    self.begin(Leg::Unloading, now);
                    FerryAction::Unload
                }
                Arrival::Resend => FerryAction::MoveTo(self.drop),
                Arrival::Astray => FerryAction::End,
            },
            Leg::Unloading => {
                // The Unload mission runs to its end (it queues Guard), and a
                // transport on water first drives to land.
                if !transport.idle || now.saturating_sub(self.ordered_at) < RESEND_TICKS {
                    return FerryAction::None;
                }
                if transport.passengers == 0 {
                    self.begin(Leg::ToPickup, now);
                    self.target = self.pickup;
                    return FerryAction::MoveTo(self.pickup);
                }
                // Still loaded and at rest: the passengers left aboard found
                // no way out. Cross again, to a cell with room for them.
                if !within(transport.cell, self.drop, LEASH_CELLS) {
                    return FerryAction::End;
                }
                self.begin(Leg::ToDrop, now);
                self.target = self.drop;
                FerryAction::MoveTo(self.drop)
            }
            Leg::ToPickup => match self.arrival(transport, self.pickup, now) {
                Arrival::Travelling => FerryAction::None,
                Arrival::Arrived => {
                    self.begin(Leg::Loading, now);
                    self.last_boarding = now;
                    self.last_count = transport.passengers;
                    FerryAction::None
                }
                Arrival::Resend => FerryAction::MoveTo(self.pickup),
                Arrival::Astray => FerryAction::End,
            },
        }
    }

    /// Where a Move leg stands. A transport that moved and then stopped away
    /// from the leg's end was stopped, sent elsewhere or could not get there.
    fn arrival(&mut self, transport: &TransportFacts, end: Cell, now: u64) -> Arrival {
        if !transport.idle {
            return Arrival::Travelling;
        }
        let near = within(transport.cell, end, ARRIVE_CELLS);
        if self.seen_busy {
            return if near {
                Arrival::Arrived
            } else {
                Arrival::Astray
            };
        }
        // Never seen moving: the order is still on its way, was refused, or
        // the transport already stands at the end.
        if now.saturating_sub(self.ordered_at) < RESEND_TICKS {
            Arrival::Travelling
        } else if near {
            Arrival::Arrived
        } else {
            self.ordered_at = now;
            Arrival::Resend
        }
    }
}

enum Arrival {
    Travelling,
    Arrived,
    Resend,
    Astray,
}

/// A unit on its way aboard a route's transport: one this helper told to
/// board, or one the player sent, from the look that first saw it.
#[derive(Debug, Clone, Copy)]
struct Boarder {
    unit: u64,
    transport: u64,
    at: u64,
}

impl Boarder {
    /// Whether the transport still waits for this unit: its order is fresh,
    /// or it is on its way and has not used up [`BOARDING_TICKS`]. A unit
    /// that never gets aboard stops holding the ferry.
    fn holds(&self, sim: &Simulation, now: u64) -> bool {
        let age = now.saturating_sub(self.at);
        age < ORDER_GRACE_TICKS || (age < BOARDING_TICKS && on_the_way_aboard(sim, self.unit))
    }
}

/// The local player's ferry routes and the armed drop click.
#[derive(Debug, Default)]
pub(crate) struct FerryState {
    /// The Ferry key was pressed: the next left click names the drop spot.
    armed: bool,
    routes: Vec<FerryRoute>,
    boarders: Vec<Boarder>,
}

impl FerryState {
    pub(crate) fn armed(&self) -> bool {
        self.armed
    }

    /// Drop the armed click; whether there was one.
    pub(crate) fn disarm(&mut self) -> bool {
        std::mem::take(&mut self.armed)
    }

    /// End the routes of these transports, as the Stop key does.
    pub(crate) fn end_for(&mut self, transports: &[u64]) {
        self.routes
            .retain(|route| !transports.contains(&route.transport));
    }

    /// Add a route; a transport's earlier route is replaced.
    pub(crate) fn add(&mut self, route: FerryRoute) {
        self.end_for(&[route.transport]);
        if self.routes.len() < MAX_ROUTES {
            self.routes.push(route);
        }
    }

    /// One look at every route: the orders to send, in issue order. Routes
    /// that ended are dropped.
    pub(crate) fn check(
        &mut self,
        sim: &Simulation,
        rules: &RuleSet,
        registry: Option<&OverlayTypeRegistry>,
        owner: InternedId,
        now: u64,
    ) -> Vec<Command> {
        let Self {
            routes, boarders, ..
        } = self;
        // A unit still on its way is remembered past its time, so that it is
        // not counted as a new boarder.
        boarders.retain(|boarder| {
            now.saturating_sub(boarder.at) < BOARDING_TICKS || on_the_way_aboard(sim, boarder.unit)
        });
        let ferrying: Vec<u64> = routes.iter().map(|route| route.transport).collect();
        // The drop cells other transports are crossing to or unloading on.
        let mut claimed: Vec<(u64, Cell)> = routes
            .iter()
            .filter(|route| matches!(route.leg, Leg::ToDrop | Leg::Unloading))
            .map(|route| (route.transport, route.target))
            .collect();
        let mut orders = Vec::new();
        routes.retain_mut(|route| {
            let Some((entity, object)) = ferry_transport(sim, rules, route.transport, owner) else {
                return false;
            };
            let cargo = entity.passenger_role.cargo();
            // A unit the player clicked aboard shows as the transport's
            // boarding contact.
            for id in entity.radio_contacts.iter_live() {
                if on_the_way_aboard(sim, id) && !boarders.iter().any(|boarder| boarder.unit == id)
                {
                    boarders.push(Boarder {
                        unit: id,
                        transport: route.transport,
                        at: now,
                    });
                }
            }
            let mut boarding = boarders
                .iter()
                .any(|boarder| boarder.transport == route.transport && boarder.holds(sim, now));
            let mut board = Vec::new();
            if route.leg == Leg::Loading && orders.len() < MAX_BOARD_ORDERS {
                board = board_orders(
                    sim,
                    rules,
                    owner,
                    route,
                    entity,
                    object,
                    &ferrying,
                    boarders.as_slice(),
                    now,
                );
                board.truncate(MAX_BOARD_ORDERS - orders.len());
                boarding |= !board.is_empty();
            }
            let facts = TransportFacts {
                cell: (entity.position.rx(), entity.position.ry()),
                idle: at_rest(entity),
                passengers: cargo.map_or(0, PassengerCargo::count),
                full: cargo.map_or(0, PassengerCargo::total_size) >= object.passengers,
                boarding,
            };
            match route.step(&facts, now) {
                FerryAction::None => {}
                FerryAction::MoveTo(cell) => {
                    // A crossing ends on a free cell of the drop zone that no
                    // other route is heading for.
                    let cell = if route.leg == Leg::ToDrop {
                        // With nowhere better, a transport already at the
                        // drop stays where it is.
                        let fallback = if within(facts.cell, route.drop, LEASH_CELLS) {
                            facts.cell
                        } else {
                            route.drop
                        };
                        let cell =
                            drop_cell(sim, rules, registry, owner, route, &claimed, fallback);
                        claimed.push((route.transport, cell));
                        cell
                    } else {
                        cell
                    };
                    route.target = cell;
                    let Some(goal) = ordinary_cell_move_goal(
                        sim,
                        rules,
                        registry,
                        owner,
                        route.transport,
                        cell,
                        false,
                    ) else {
                        return false;
                    };
                    orders.push(Command::Move {
                        entity_id: route.transport,
                        target_rx: goal.0,
                        target_ry: goal.1,
                    });
                }
                FerryAction::Unload => {
                    orders.extend(unload_command(entity, Some(object), Some(owner)));
                }
                FerryAction::End => return false,
            }
            for unit in board {
                boarders.push(Boarder {
                    unit,
                    transport: route.transport,
                    at: now,
                });
                orders.push(Command::MegaMission {
                    entity_id: unit,
                    mission: MissionType::Enter,
                    target: None,
                    destination: Some(TargetKind::Entity(route.transport)),
                });
            }
            true
        });
        orders
    }
}

/// On Guard with no destination and a locomotor at rest: a unit that has
/// come to a stop.
fn at_rest(entity: &GameEntity) -> bool {
    entity.mission.current().known() == Some(MissionType::Guard)
        && entity.navigation.nav_com().is_none()
        && crate::sim::movement::motion_query::is_moving(entity) != Some(true)
}

/// A living unit outside any transport whose mission is Enter.
fn on_the_way_aboard(sim: &Simulation, id: u64) -> bool {
    sim.entities().get(id).is_some_and(|entity| {
        entity.is_alive()
            && !entity.passenger_role.is_inside_transport()
            && entity.mission.current().known() == Some(MissionType::Enter)
    })
}

/// The live transport of a route: the owner's own controllable vehicle or
/// aircraft of a type with `Passengers=`.
fn ferry_transport<'a>(
    sim: &'a Simulation,
    rules: &'a RuleSet,
    id: u64,
    owner: InternedId,
) -> Option<(&'a GameEntity, &'a ObjectType)> {
    let entity = sim.entities().get(id)?;
    if !entity.is_alive()
        || entity.death.is_dying()
        || entity.owner() != owner
        || !matches!(
            entity.category(),
            EntityCategory::Unit | EntityCategory::Aircraft
        )
        || entity.passenger_role.is_inside_transport()
    {
        return None;
    }
    let object = sim.interner().object_type(entity.type_ref(), rules)?;
    (object.passengers > 0 && sim.techno_player_controllable(id, rules)).then_some((entity, object))
}

/// The cell this crossing heads for: the one nearest the drop spot that holds
/// nothing on the ground, is not `claimed`, lies clear of the pick-up circle,
/// is where a Move click on it would send the transport and has an empty
/// cell beside it for the passengers. Cells of the drop zone come first; when
/// all are taken, cells up to [`DROP_OVERFLOW_CELLS`] outside it. `fallback`
/// when no cell qualifies.
fn drop_cell(
    sim: &Simulation,
    rules: &RuleSet,
    registry: Option<&OverlayTypeRegistry>,
    owner: InternedId,
    route: &FerryRoute,
    claimed: &[(u64, Cell)],
    fallback: Cell,
) -> Cell {
    // Nearest first, so a cell inside the zone wins while one is free.
    let reach = DROP_RADIUS_CELLS + DROP_OVERFLOW_CELLS;
    let mut cells: Vec<(i64, Cell)> = Vec::new();
    for dy in -reach..=reach {
        for dx in -reach..=reach {
            let distance = dx * dx + dy * dy;
            if distance > reach * reach {
                continue;
            }
            if let (Ok(rx), Ok(ry)) = (
                u16::try_from(i64::from(route.drop.0) + dx),
                u16::try_from(i64::from(route.drop.1) + dy),
            ) {
                cells.push((distance, (rx, ry)));
            }
        }
    }
    cells.sort();
    let empty = |cell: Cell| {
        sim.cell_objects(cell, MovementLayer::Ground)
            .all(|member| member == CellObjectMember::Entity(route.transport))
    };
    // Empty cells beside `cell`, for the passengers to step onto.
    let room = |cell: Cell| {
        (-1i32..=1)
            .flat_map(|dy| (-1i32..=1).map(move |dx| (dx, dy)))
            .filter(|&step| step != (0, 0))
            .filter(|&(dx, dy)| {
                match (
                    u16::try_from(i32::from(cell.0) + dx),
                    u16::try_from(i32::from(cell.1) + dy),
                ) {
                    (Ok(rx), Ok(ry)) => empty((rx, ry)),
                    _ => false,
                }
            })
            .count()
    };
    let free: Vec<(Cell, usize)> = cells
        .into_iter()
        .map(|(_, cell)| cell)
        .filter(|&cell| {
            !claimed
                .iter()
                .any(|&(other, taken)| other != route.transport && taken == cell)
                && !within(cell, route.pickup, PICKUP_RADIUS_CELLS + 2)
                && empty(cell)
                && ordinary_cell_move_goal(
                    sim,
                    rules,
                    registry,
                    owner,
                    route.transport,
                    cell,
                    false,
                ) == Some(cell)
        })
        .map(|cell| (cell, room(cell)))
        .filter(|&(_, room)| room > 0)
        .collect();
    free.first().map_or(fallback, |&(cell, _)| cell)
}

/// The units to tell aboard this look: the owner's own infantry and vehicles
/// at rest inside the pick-up circle, nearest the transport first, while the
/// planned hold has room and the engine's admission answers yes.
#[allow(clippy::too_many_arguments)]
fn board_orders(
    sim: &Simulation,
    rules: &RuleSet,
    owner: InternedId,
    route: &FerryRoute,
    transport: &GameEntity,
    transport_object: &ObjectType,
    ferrying: &[u64],
    boarders: &[Boarder],
    now: u64,
) -> Vec<u64> {
    let size_of = |entity: &GameEntity| {
        sim.interner()
            .object_type(entity.type_ref(), rules)
            .map(|object| object.size)
    };
    // The hold as it will be once everyone already on the way is aboard.
    let mut planned = transport
        .passenger_role
        .cargo()
        .cloned()
        .unwrap_or_else(|| {
            PassengerCargo::new(
                transport_object.passengers as u32,
                transport_object.size_limit,
            )
        });
    for boarder in boarders {
        if boarder.transport == route.transport
            && boarder.holds(sim, now)
            && let Some(entity) = sim.entities().get(boarder.unit)
            && !entity.passenger_role.is_inside_transport()
            && let Some(size) = size_of(entity)
        {
            planned.board_forced_chain(&[(boarder.unit, size)]);
        }
    }

    let transport_cell = (transport.position.rx(), transport.position.ry());
    let mut waiting: Vec<(i64, u64, f64)> = Vec::new();
    for dy in -PICKUP_RADIUS_CELLS..=PICKUP_RADIUS_CELLS {
        for dx in -PICKUP_RADIUS_CELLS..=PICKUP_RADIUS_CELLS {
            if dx * dx + dy * dy > PICKUP_RADIUS_CELLS * PICKUP_RADIUS_CELLS {
                continue;
            }
            let (Ok(rx), Ok(ry)) = (
                u16::try_from(i64::from(route.pickup.0) + dx),
                u16::try_from(i64::from(route.pickup.1) + dy),
            ) else {
                continue;
            };
            for member in sim.cell_objects((rx, ry), MovementLayer::Ground) {
                let CellObjectMember::Entity(id) = member else {
                    continue;
                };
                let Some(entity) = sim.entities().get(id) else {
                    continue;
                };
                let category = entity.category();
                if entity.owner() != owner
                    || !entity.is_alive()
                    || entity.death.is_dying()
                    || !matches!(category, EntityCategory::Infantry | EntityCategory::Unit)
                    || entity.passenger_role.is_inside_transport()
                    || !at_rest(entity)
                    || ferrying.contains(&id)
                    || boarders.iter().any(|boarder| boarder.unit == id)
                    || waiting.iter().any(|&(_, seen, _)| seen == id)
                    // A loaded carrier does not board, as in AutoLoad.
                    || !entity
                        .passenger_role
                        .cargo()
                        .is_none_or(PassengerCargo::is_empty)
                    || !sim.techno_player_controllable(id, rules)
                {
                    continue;
                }
                let Some(size) = size_of(entity) else {
                    continue;
                };
                waiting.push((cell_dist_sq((rx, ry), transport_cell), id, size));
            }
        }
    }
    waiting.sort_by(|a, b| (a.0, a.1).cmp(&(b.0, b.1)));

    let mut board = Vec::new();
    for (_, id, size) in waiting {
        if PassengerCargo::size_fits_transport(
            planned.total_size(),
            size,
            transport_object.passengers,
            transport_object.size_limit,
        ) && can_enter_transport(sim, rules, id, route.transport)
        {
            planned.board_forced_chain(&[(id, size)]);
            board.push(id);
        }
    }
    board
}

/// The selected transports a ferry order can take.
fn selected_transports(state: &AppState) -> Vec<u64> {
    let Some(runtime) = state.match_state.sim_runtime.runtime() else {
        return Vec::new();
    };
    let sim = &runtime.simulation;
    let rules = &runtime.resources.rules;
    let Some(owner) = preferred_local_owner_name(state).and_then(|name| sim.interner().get(&name))
    else {
        return Vec::new();
    };
    selected_stable_ids_in_order(
        Some(sim),
        Some(rules),
        &state.match_state.input.selection_order,
        state.match_state.input.selection_order_pending,
    )
    .into_iter()
    .filter(|&id| ferry_transport(sim, rules, id, owner).is_some())
    .collect()
}

/// The Ferry key: arm the drop click when a transport is selected; a second
/// press drops it again.
pub(crate) fn hotkey_pressed(state: &mut AppState) {
    if state.match_state.input.ferry.disarm() {
        return;
    }
    state.match_state.input.ferry.armed = !selected_transports(state).is_empty();
}

/// The route a drop click on `clicked` gives this transport: from the cell it
/// stands on to the cell a Move click there would send it to
/// ([`ordinary_cell_move_goal`]). `None` when the transport cannot ferry, no
/// Move goes there, or the two ends are within [`MIN_ROUTE_CELLS`].
fn route_for(
    sim: &Simulation,
    rules: &RuleSet,
    registry: Option<&OverlayTypeRegistry>,
    owner: InternedId,
    transport: u64,
    clicked: Cell,
    now: u64,
) -> Option<FerryRoute> {
    let (entity, _) = ferry_transport(sim, rules, transport, owner)?;
    let pickup = (entity.position.rx(), entity.position.ry());
    let drop = ordinary_cell_move_goal(sim, rules, registry, owner, transport, clicked, false)?;
    if within(pickup, drop, MIN_ROUTE_CELLS) {
        return None;
    }
    Some(FerryRoute::new(
        transport,
        pickup,
        drop,
        entity.passenger_role.passenger_count(),
        now,
    ))
}

/// The armed left click: every selected transport ferries from where it
/// stands to the clicked cell. Returns whether the click was taken.
pub(crate) fn drop_click(state: &mut AppState) -> bool {
    if !state.match_state.input.ferry.disarm() {
        return false;
    }
    let transports = selected_transports(state);
    if transports.is_empty() {
        return false;
    }
    let clicked = crate::app::match_runtime::sim_tick::screen_point_to_world_cell(
        state,
        state.match_state.input.cursor_x,
        state.match_state.input.cursor_y,
    );
    let routes: Vec<FerryRoute> = {
        let Some(runtime) = state.match_state.sim_runtime.runtime() else {
            return true;
        };
        let sim = &runtime.simulation;
        let Some(owner) =
            preferred_local_owner_name(state).and_then(|name| sim.interner().get(&name))
        else {
            return true;
        };
        let now = sim.session().tick();
        transports
            .into_iter()
            .filter_map(|id| {
                route_for(
                    sim,
                    &runtime.resources.rules,
                    Some(&runtime.resources.overlay_registry),
                    owner,
                    id,
                    clicked,
                    now,
                )
            })
            .collect()
    };
    for route in routes {
        state.match_state.input.ferry.add(route);
    }
    true
}

/// The Stop key ends the selected transports' routes.
pub(crate) fn stop_selected(state: &mut AppState, selected: &[u64]) {
    state.match_state.input.ferry.end_for(selected);
}

/// Once per logic frame: every [`CHECK_INTERVAL`] frames look at the routes
/// and send their orders.
pub(crate) fn frame_pass(state: &mut AppState) {
    if state.match_state.input.ferry.routes.is_empty() {
        return;
    }
    let Some(owner_name) = preferred_local_owner_name(state) else {
        return;
    };
    let mut ferry = std::mem::take(&mut state.match_state.input.ferry);
    let orders = match state.match_state.sim_runtime.runtime() {
        Some(runtime) => {
            let sim = &runtime.simulation;
            let now = sim.session().tick();
            match sim.interner().get(&owner_name) {
                Some(owner) if now % CHECK_INTERVAL == 0 => ferry.check(
                    sim,
                    &runtime.resources.rules,
                    Some(&runtime.resources.overlay_registry),
                    owner,
                    now,
                ),
                _ => Vec::new(),
            }
        }
        None => Vec::new(),
    };
    state.match_state.input.ferry = ferry;
    for order in orders {
        // An order the queue cannot take now is sent again by a later look.
        let _ = try_schedule_command(state, &owner_name, order);
    }
}

/// The armed drop click as it would land on the cell under the cursor: the
/// selected transports' cells, that cell, and whether the click would give at
/// least one of them a route. `None` while no click is armed or no transport
/// is selected.
fn armed_preview(state: &AppState) -> Option<(Vec<Cell>, Cell, bool)> {
    if !state.match_state.input.ferry.armed() {
        return None;
    }
    let transports = selected_transports(state);
    let runtime = state.match_state.sim_runtime.runtime()?;
    let sim = &runtime.simulation;
    let owner = preferred_local_owner_name(state).and_then(|name| sim.interner().get(&name))?;
    let clicked = crate::app::match_runtime::sim_tick::screen_point_to_world_cell(
        state,
        state.match_state.input.cursor_x,
        state.match_state.input.cursor_y,
    );
    let pickups: Vec<Cell> = transports
        .iter()
        .filter_map(|&id| sim.entities().get(id))
        .map(|entity| (entity.position.rx(), entity.position.ry()))
        .collect();
    if pickups.is_empty() {
        return None;
    }
    let accepted = transports.iter().any(|&id| {
        route_for(
            sim,
            &runtime.resources.rules,
            Some(&runtime.resources.overlay_registry),
            owner,
            id,
            clicked,
            sim.session().tick(),
        )
        .is_some()
    });
    Some((pickups, clicked, accepted))
}

/// Whether the armed drop click would be taken where the cursor is; `None`
/// while none is armed. The cursor shows a refusal.
pub(crate) fn armed_click_accepted(state: &AppState) -> Option<bool> {
    armed_preview(state).map(|(_, _, accepted)| accepted)
}

/// One circle of the ferry's marks: a closed line in VERA's world-pixel
/// frame.
#[derive(Debug, Clone)]
pub(crate) struct FerryRing {
    pub points: Vec<(f32, f32)>,
    /// The drop zone's circle; otherwise a pick-up area's.
    pub drop: bool,
    /// Part of the preview of a click that would be refused.
    pub refused: bool,
}

/// The ferry's marks: for each selected transport's route a circle around its
/// pick-up area and one around its drop zone; and, while a drop click is
/// armed, the same circles for the selected transports and the cell under the
/// cursor.
pub(crate) fn marks(state: &AppState) -> Vec<FerryRing> {
    use crate::util::lepton::{
        GROUND_LEVEL_HEIGHT_LEPTONS, LEPTONS_PER_CELL_I32, absolute_leptons_to_screen,
    };
    // Only a selected transport shows its route.
    let entities = state.sim_view().map(|view| view.simulation().entities());
    let routes: Vec<&FerryRoute> = state
        .match_state
        .input
        .ferry
        .routes
        .iter()
        .filter(|route| {
            entities
                .and_then(|entities| entities.get(route.transport))
                .is_some_and(|entity| entity.selection.is_selected())
        })
        .collect();
    let preview = armed_preview(state);
    if routes.is_empty() && preview.is_none() {
        return Vec::new();
    }
    let cells = state
        .sim_view()
        .and_then(|view| crate::app::match_runtime::sim_tick::tactical_cells(view.simulation()));
    let cell = LEPTONS_PER_CELL_I32 as f32;
    let pickup_radius = (PICKUP_RADIUS_CELLS as f32 + 0.5) * cell;
    let drop_radius = (DROP_RADIUS_CELLS as f32 + 0.5) * cell;
    // A circle around a cell's centre, at its ground height, as 64 corners.
    let ring = |at: Cell, drop: bool, refused: bool| {
        let radius = if drop { drop_radius } else { pickup_radius };
        let level = cells
            .and_then(|cells| cells.cell_level(at.0, at.1))
            .unwrap_or(0);
        let x = (i32::from(at.0) * LEPTONS_PER_CELL_I32 + LEPTONS_PER_CELL_I32 / 2) as f32;
        let y = (i32::from(at.1) * LEPTONS_PER_CELL_I32 + LEPTONS_PER_CELL_I32 / 2) as f32;
        let z = i32::from(level) * GROUND_LEVEL_HEIGHT_LEPTONS;
        FerryRing {
            points: (0..64)
                .map(|corner| {
                    let angle = std::f32::consts::TAU * corner as f32 / 64.0;
                    absolute_leptons_to_screen(
                        (x + radius * angle.cos()) as i32,
                        (y + radius * angle.sin()) as i32,
                        z,
                    )
                })
                .collect(),
            drop,
            refused,
        }
    };
    let mut rings = Vec::new();
    for route in routes {
        rings.push(ring(route.pickup, false, false));
        rings.push(ring(route.drop, true, false));
    }
    if let Some((pickups, clicked, accepted)) = preview {
        for pickup in pickups {
            rings.push(ring(pickup, false, !accepted));
        }
        rings.push(ring(clicked, true, !accepted));
    }
    rings
}

#[cfg(test)]
#[path = "ferry_tests.rs"]
mod tests;
