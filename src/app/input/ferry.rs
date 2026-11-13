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
use crate::sim::components::DriveCoord;
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
/// A ground transport within this many cells of a waypoint is sent on to
/// the next.
const VIA_REACH_CELLS: i64 = 2;
/// The same for a Jumpjet or aircraft, which slows and sinks about a cell
/// before its destination (`State3_Translate`'s slow radius, `0x0054BFF0`)
/// and covers about a cell between two looks.
const VIA_AIR_REACH_CELLS: i64 = 4;
/// A Move to a waypoint may be given a cell this far from it: the group
/// spread (`0x0064CDA0`) hands transports sent to one cell a cell each, up to
/// six probes out.
const VIA_SPREAD_CELLS: i64 = 8;
/// The most waypoints one route takes.
const MAX_VIA: usize = 8;
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
    /// The cell its Move is heading for, when it has one.
    pub heading: Option<Cell>,
    /// How near a waypoint it is sent on: [`VIA_REACH_CELLS`] or
    /// [`VIA_AIR_REACH_CELLS`].
    pub reach: i64,
}

/// The order a look sends for a route's transport.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FerryAction {
    None,
    /// Move to a leg's end: the pick-up spot, or the drop zone.
    MoveTo(Cell),
    /// Move to a waypoint on the way there.
    MoveVia(Cell),
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
    /// The waypoints between the pick-up spot and the drop spot, in the
    /// order they are passed on the way out; the way back takes them in
    /// reverse.
    via: Vec<Cell>,
    /// How many waypoints of the current Move leg are still ahead.
    via_left: usize,
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
            via: Vec::new(),
            via_left: 0,
        }
    }

    /// The same route through these waypoints.
    pub(crate) fn with_via(mut self, via: Vec<Cell>) -> Self {
        self.via = via;
        self
    }

    /// The waypoint the current Move leg heads for next, if any is left.
    fn via_point(&self) -> Option<Cell> {
        let passed = self.via.len().checked_sub(self.via_left)?;
        match self.leg {
            _ if self.via_left == 0 => None,
            Leg::ToDrop => self.via.get(passed).copied(),
            Leg::ToPickup => self.via.get(self.via_left - 1).copied(),
            Leg::Loading | Leg::Unloading => None,
        }
    }

    /// Start a Move leg through every waypoint: the order for its first
    /// stretch.
    fn set_out(&mut self, leg: Leg, end: Cell, now: u64) -> FerryAction {
        self.begin(leg, now);
        self.target = end;
        self.via_left = self.via.len();
        match self.via_point() {
            Some(point) => FerryAction::MoveVia(point),
            None => FerryAction::MoveTo(end),
        }
    }

    /// Leave out the waypoint ahead: the order for the stretch after it.
    fn skip_waypoint(&mut self, now: u64) -> FerryAction {
        self.via_left = self.via_left.saturating_sub(1);
        self.ordered_at = now;
        match self.via_point() {
            Some(next) => FerryAction::MoveVia(next),
            None => FerryAction::MoveTo(self.target),
        }
    }

    /// A Move leg with waypoints still ahead. The transport is sent on to
    /// the next point, without stopping, once it is within its reach of the
    /// waypoint or of the cell near the waypoint its Move was given, or has
    /// stopped as near the waypoint as that Move took it. One that stops
    /// farther than [`VIA_SPREAD_CELLS`] from the waypoint was sent
    /// elsewhere, and its ferry ends. `None` once no waypoint is left and
    /// the leg's end decides.
    fn pass_waypoints(&mut self, transport: &TransportFacts, now: u64) -> Option<FerryAction> {
        let point = self.via_point()?;
        let waited = now.saturating_sub(self.ordered_at);
        let stopped = transport.idle && waited >= ORDER_GRACE_TICKS;
        let own_cell = transport
            .heading
            .filter(|&cell| within(cell, point, VIA_SPREAD_CELLS));
        if within(transport.cell, point, transport.reach)
            || own_cell.is_some_and(|cell| within(transport.cell, cell, transport.reach))
            || (stopped && within(transport.cell, point, VIA_SPREAD_CELLS))
        {
            // A transport sent on while at rest has yet to be seen moving.
            self.seen_busy = !transport.idle;
            return Some(self.skip_waypoint(now));
        }
        Some(if !stopped {
            FerryAction::None
        } else if self.seen_busy {
            FerryAction::End
        } else if waited < RESEND_TICKS {
            FerryAction::None
        } else {
            self.ordered_at = now;
            FerryAction::MoveVia(point)
        })
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
                    return self.set_out(Leg::ToDrop, self.drop, now);
                }
                FerryAction::None
            }
            Leg::ToDrop | Leg::ToPickup if self.via_left > 0 => self
                .pass_waypoints(transport, now)
                .unwrap_or(FerryAction::None),
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
                    return self.set_out(Leg::ToPickup, self.pickup, now);
                }
                // Still loaded and at rest: the passengers left aboard found
                // no way out. Cross again, to a cell with room for them.
                if !within(transport.cell, self.drop, LEASH_CELLS) {
                    return FerryAction::End;
                }
                self.begin(Leg::ToDrop, now);
                self.target = self.drop;
                self.via_left = 0;
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
        // An order sent while passing a waypoint needs a moment to show.
        if !near && !self.via.is_empty() && now.saturating_sub(self.ordered_at) < ORDER_GRACE_TICKS
        {
            return Arrival::Travelling;
        }
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
    /// The Planning Mode key, held while the click is armed: clicks add
    /// waypoints.
    via_key: Option<winit::keyboard::KeyCode>,
    /// The waypoints clicked so far for the armed click.
    pending_via: Vec<Cell>,
    routes: Vec<FerryRoute>,
    boarders: Vec<Boarder>,
    /// [`selected_transports`] and [`armed_preview`] as last answered with
    /// the world home (`sim_host::home_answer`): the draw asks them.
    transports_seen: std::cell::RefCell<Vec<u64>>,
    preview_seen: std::cell::RefCell<Option<(Vec<Cell>, Cell, bool)>>,
}

impl FerryState {
    pub(crate) fn armed(&self) -> bool {
        self.armed
    }

    /// Drop the armed click; whether there was one.
    pub(crate) fn disarm(&mut self) -> bool {
        self.via_key = None;
        self.pending_via.clear();
        std::mem::take(&mut self.armed)
    }

    /// The Planning Mode key went down while the click is armed; whether it
    /// was up before.
    fn press_via_key(&mut self, physical: winit::keyboard::KeyCode) -> bool {
        self.via_key.replace(physical).is_none()
    }

    /// A key came up; the waypoint key no longer marks clicks. Whether it was
    /// the waypoint key.
    fn release_via_key(&mut self, physical: winit::keyboard::KeyCode) -> bool {
        let held = self.via_key == Some(physical);
        if held {
            self.via_key = None;
        }
        held
    }

    /// Whether the waypoint key is held: the armed click adds a waypoint.
    pub(crate) fn waypoint_click(&self) -> bool {
        self.armed && self.via_key.is_some()
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
                heading: match entity.navigation.nav_com() {
                    Some(crate::sim::components::NavTargetRef::Cell { rx, ry }) => Some((rx, ry)),
                    _ => None,
                },
                reach: if entity.locomotor.as_ref().is_some_and(|locomotor| {
                    matches!(
                        locomotor.active_kind(),
                        crate::rules::locomotor_type::LocomotorKind::Jumpjet
                            | crate::rules::locomotor_type::LocomotorKind::Fly
                    )
                }) {
                    VIA_AIR_REACH_CELLS
                } else {
                    VIA_REACH_CELLS
                },
            };
            let mut action = route.step(&facts, now);
            // A waypoint no Move click would send this transport to is left
            // out, and the route carries on with the point after it.
            while let FerryAction::MoveVia(cell) = action {
                action = match ordinary_cell_move_goal(
                    sim,
                    rules,
                    registry,
                    owner,
                    route.transport,
                    cell,
                    false,
                ) {
                    Some(goal) => {
                        orders.push(Command::Move {
                            entity_id: route.transport,
                            target_rx: goal.0,
                            target_ry: goal.1,
                        });
                        FerryAction::None
                    }
                    None => route.skip_waypoint(now),
                };
            }
            match action {
                FerryAction::None | FerryAction::MoveVia(_) => {}
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
    crate::app::match_runtime::sim_host::home_answer(
        &state.match_state.sim_runtime,
        &state.match_state.input.ferry.transports_seen,
        || find_selected_transports(state),
    )
}

fn find_selected_transports(state: &AppState) -> Vec<u64> {
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

/// The Planning Mode key's edge while the drop click is armed: held, clicks
/// add waypoints. Going down and coming up it gives Planning Mode's own
/// sounds (`StartPlanningModeSound`, `EndPlanningModeSound`), as the key does
/// there.
pub(crate) fn via_key_edge(state: &mut AppState, physical: winit::keyboard::KeyCode, down: bool) {
    let ferry = &mut state.match_state.input.ferry;
    let changed = if down {
        ferry.press_via_key(physical)
    } else {
        ferry.release_via_key(physical)
    };
    if !changed {
        return;
    }
    let sound = state.rules().and_then(|rules| {
        if down {
            rules.general.start_planning_mode_sound.clone()
        } else {
            rules.general.end_planning_mode_sound.clone()
        }
    });
    if let Some(sound_id) = sound {
        state
            .match_state
            .match_audio
            .sound_events
            .push(crate::audio::events::GameSoundEvent::UiSound { sound_id });
    }
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
    // With the Planning Mode key held, the click is a waypoint and the drop
    // click stays armed.
    if state.match_state.input.ferry.waypoint_click() {
        let clicked = crate::app::match_runtime::sim_tick::screen_point_to_world_cell(
            state,
            state.match_state.input.cursor_x,
            state.match_state.input.cursor_y,
        );
        // A cell none of the selected transports can move to is not taken.
        let accepted = armed_preview(state).is_some_and(|(_, _, accepted)| accepted);
        let pending = &mut state.match_state.input.ferry.pending_via;
        if accepted && pending.len() < MAX_VIA {
            pending.push(clicked);
        }
        return true;
    }
    let via = std::mem::take(&mut state.match_state.input.ferry.pending_via);
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
                .map(|route| route.with_via(via.clone()))
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

/// The armed click as it would land on the cell under the cursor: the
/// selected transports' cells, that cell, and whether the click would be
/// taken: a route for at least one of them, or with the waypoint key held a
/// cell at least one of them can move to. `None` while no click is armed or no transport
/// is selected.
fn armed_preview(state: &AppState) -> Option<(Vec<Cell>, Cell, bool)> {
    if !state.match_state.input.ferry.armed() {
        return None;
    }
    crate::app::match_runtime::sim_host::home_answer(
        &state.match_state.sim_runtime,
        &state.match_state.input.ferry.preview_seen,
        || find_armed_preview(state),
    )
}

fn find_armed_preview(state: &AppState) -> Option<(Vec<Cell>, Cell, bool)> {
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
    // A waypoint click needs only a cell one of them can move to; the drop
    // click needs a whole route.
    let waypoint_click = state.match_state.input.ferry.waypoint_click();
    let rules = &runtime.resources.rules;
    let registry = Some(&runtime.resources.overlay_registry);
    let accepted = transports.iter().any(|&id| {
        if waypoint_click {
            ordinary_cell_move_goal(sim, rules, registry, owner, id, clicked, false).is_some()
        } else {
            route_for(
                sim,
                rules,
                registry,
                owner,
                id,
                clicked,
                sim.session().tick(),
            )
            .is_some()
        }
    });
    Some((pickups, clicked, accepted))
}

/// Whether the armed drop click would be taken where the cursor is; `None`
/// while none is armed. The cursor shows a refusal.
pub(crate) fn armed_click_accepted(state: &AppState) -> Option<bool> {
    armed_preview(state).map(|(_, _, accepted)| accepted)
}

/// One circle of the ferry's marks: the corners of a closed line, in leptons
/// at the height of the ground under its centre.
#[derive(Debug, Clone)]
pub(crate) struct FerryRing {
    pub points: Vec<DriveCoord>,
    /// The drop zone's circle; otherwise a pick-up area's.
    pub drop: bool,
    /// Part of the preview of a click that would be refused.
    pub refused: bool,
}

/// The routes that show their marks: only a selected transport's.
/// Drawing reads the objects from the presentation frame.
fn shown_routes(state: &AppState) -> Vec<&FerryRoute> {
    let frame = state.match_state.sim_runtime.frame();
    state
        .match_state
        .input
        .ferry
        .routes
        .iter()
        .filter(|route| {
            frame
                .and_then(|frame| frame.object(route.transport))
                .is_some_and(|entity| entity.selection.is_selected())
        })
        .collect()
}

/// A ferry's waypoints as one path of cells, drawn in Planning Mode's look.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct FerryPath {
    /// The pick-up spot, the waypoints in order, and the drop spot once it is
    /// clicked.
    pub cells: Vec<Cell>,
    /// The drop click is still armed: Planning Mode's colour for nodes it has
    /// not committed yet.
    pub pending: bool,
}

/// The waypoint paths to draw: one for each selected transport's route that
/// has waypoints, and while the drop click is armed one from each selected
/// transport through the waypoints clicked so far.
pub(crate) fn waypoint_paths(state: &AppState) -> Vec<FerryPath> {
    let mut paths: Vec<FerryPath> = shown_routes(state)
        .into_iter()
        .filter(|route| !route.via.is_empty())
        .map(|route| FerryPath {
            cells: std::iter::once(route.pickup)
                .chain(route.via.iter().copied())
                .chain(std::iter::once(route.drop))
                .collect(),
            pending: false,
        })
        .collect();
    let ferry = &state.match_state.input.ferry;
    if ferry.armed
        && !ferry.pending_via.is_empty()
        && let Some(frame) = state.match_state.sim_runtime.frame()
    {
        for id in selected_transports(state) {
            if let Some(entity) = frame.object(id) {
                let pickup = (entity.position.rx(), entity.position.ry());
                paths.push(FerryPath {
                    cells: std::iter::once(pickup)
                        .chain(ferry.pending_via.iter().copied())
                        .collect(),
                    pending: true,
                });
            }
        }
    }
    paths
}

/// The ferry's circles: for each selected transport's route one around its
/// pick-up area and one around its drop zone; and, while a drop click is
/// armed, the same circles for the selected transports and the cell under the
/// cursor. The waypoints are [`waypoint_paths`].
pub(crate) fn marks(state: &AppState) -> Vec<FerryRing> {
    use crate::util::lepton::{GROUND_LEVEL_HEIGHT_LEPTONS, LEPTONS_PER_CELL_I32};
    let routes = shown_routes(state);
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
                    DriveCoord {
                        x: (x + radius * angle.cos()) as i32,
                        y: (y + radius * angle.sin()) as i32,
                        z,
                    }
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
        // With the waypoint key held the click adds a waypoint, not the drop:
        // no drop zone follows the cursor, and a cell no transport can move
        // to is the cursor's to refuse.
        let waypoint_click = state.match_state.input.ferry.waypoint_click();
        for pickup in pickups {
            rings.push(ring(pickup, false, !accepted && !waypoint_click));
        }
        if !waypoint_click {
            rings.push(ring(clicked, true, !accepted));
        }
    }
    rings
}

#[cfg(test)]
#[path = "ferry_tests.rs"]
mod tests;
