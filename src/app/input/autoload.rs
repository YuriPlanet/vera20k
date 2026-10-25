//! VeraX AutoLoad: Ctrl+D loads the selected infantry and vehicles into the
//! selected transports that are nearby and have room.
//!
//! The hotkey emits only ordinary Enter missions — the same command a click on
//! a carrier produces (Foot `ClickedAction` `0x004D74E0` ->
//! `EventClass__BuildMegaMissionEnvelope` `0x004C6860`), so every client
//! replays exactly the same orders. Admission is the engine's own:
//! [`crate::sim::passenger::can_enter_transport`] and
//! [`crate::sim::passenger::PassengerCargo::size_fits_transport`]
//! (`UnitClass::Receive_Radio @ 0x00737430`, message 0x0F). Nothing here
//! mutates simulation state.
//!
//! Buildings (bunkers, grinders, magnetrons) board through their Garrison /
//! Occupy / Absorb chains, not EnterTransport, so only vehicle carriers are
//! collected.

use crate::app::AppState;
use crate::app::input::commands::{preferred_local_owner_name, schedule_command};
use crate::app::input::dispatch::selected_stable_ids_in_order;
use crate::map::entities::EntityCategory;
use crate::rules::object_type::ObjectType;
use crate::rules::ruleset::RuleSet;
use crate::sim::combat::TargetKind;
use crate::sim::command::Command;
use crate::sim::game_entity::GameEntity;
use crate::sim::mission::MissionType;
use crate::sim::movement::ground_pose::object_center_xy;
use crate::sim::movement::locomotor::MovementLayer;
use crate::sim::passenger::{PassengerCargo, can_enter_transport};
use crate::sim::world::Simulation;

/// Carriers farther than this many cells from every selected passenger are
/// not considered.
const SEARCH_RANGE_CELLS: i64 = 30;
/// Upper bound on the orders one hotkey press may issue.
const MAX_ORDERS: usize = 512;
/// One cell in world leptons (`ObjectClass` Location units).
const LEPTONS_PER_CELL: i64 = 256;

/// A selected object that may board: its type groups it for balance, and its
/// `Size=` is the capacity unit the carrier's cargo measures.
#[derive(Debug, Clone, Copy)]
pub(crate) struct PassengerFact {
    pub id: u64,
    pub type_key: u32,
    pub size: f64,
    pub infantry: bool,
    pub pos: [i32; 2],
}

/// A selected vehicle whose type holds passengers and whose cargo has room.
#[derive(Debug, Clone)]
pub(crate) struct TransportFact {
    pub id: u64,
    /// `Passengers=` (`i32`, as Receive_Radio reads it).
    pub capacity: i32,
    /// binary64 `SizeLimit=`.
    pub size_limit: f64,
    /// A planning copy, updated through the authoritative cargo owner.
    pub cargo: PassengerCargo,
    pub pos: [i32; 2],
}

fn dist_sq(a: [i32; 2], b: [i32; 2]) -> i64 {
    let dx = i64::from(a[0]) - i64::from(b[0]);
    let dy = i64::from(a[1]) - i64::from(b[1]);
    dx * dx + dy * dy
}

/// Whether this pair is in range, fits the supplied planned total and passes
/// the engine's live admission query. Self-boarding is never planned.
fn admits(
    passenger: &PassengerFact,
    transport: &TransportFact,
    total: i32,
    eligible: &impl Fn(u64, u64) -> bool,
) -> bool {
    passenger.id != transport.id
        && dist_sq(passenger.pos, transport.pos) <= (SEARCH_RANGE_CELLS * LEPTONS_PER_CELL).pow(2)
        && PassengerCargo::size_fits_transport(
            total,
            passenger.size,
            transport.capacity,
            transport.size_limit,
        )
        && eligible(passenger.id, transport.id)
}

/// The nearest carrier that could take `passenger` with its current cargo, or
/// `i64::MAX` when none is in principle eligible. Used only to order each
/// type's members so crowded selections fill the closest hulls first.
fn nearest_eligible_dist(
    passenger: &PassengerFact,
    transports: &[TransportFact],
    eligible: &impl Fn(u64, u64) -> bool,
) -> i64 {
    transports
        .iter()
        .filter(|transport| admits(passenger, transport, transport.cargo.total_size(), eligible))
        .map(|transport| dist_sq(passenger.pos, transport.pos))
        .min()
        .unwrap_or(i64::MAX)
}

/// Plan the balanced assignment: passengers grouped by type in first-selection
/// order, each group sorted nearest-first, then groups take turns; within a
/// turn the carrier with the fewest assignments for this group, then the
/// fewest overall, then the shortest distance wins.
///
/// A selected vehicle that has served as a carrier may no longer board; a
/// carrier stays available to later passengers while its planned cargo fits.
/// Returns `(passenger_id, transport_id)` in issue order.
pub(crate) fn assign(
    passengers: &[PassengerFact],
    transports: &[TransportFact],
    eligible: impl Fn(u64, u64) -> bool,
) -> Vec<(u64, u64)> {
    if passengers.is_empty() || transports.is_empty() {
        return Vec::new();
    }
    // Type groups in first-occurrence order; members keep selection order
    // until the nearest-first sort below.
    let mut group_of: Vec<(u32, usize)> = Vec::new();
    let mut groups: Vec<Vec<usize>> = Vec::new();
    for (index, passenger) in passengers.iter().enumerate() {
        let slot = group_of.iter().find(|(key, _)| *key == passenger.type_key);
        match slot {
            Some((_, group)) => groups[*group].push(index),
            None => {
                group_of.push((passenger.type_key, groups.len()));
                groups.push(vec![index]);
            }
        }
    }

    let nearest: Vec<i64> = passengers
        .iter()
        .map(|passenger| nearest_eligible_dist(passenger, transports, &eligible))
        .collect();
    for members in &mut groups {
        members.sort_by_key(|&index| (nearest[index], index));
    }

    let group_count = groups.len();
    let mut cursors = vec![0_usize; group_count];
    let mut alive = vec![true; group_count];
    let mut cargo: Vec<PassengerCargo> = transports
        .iter()
        .map(|transport| transport.cargo.clone())
        .collect();
    let mut assigned_total = vec![0_usize; transports.len()];
    let mut assigned_by_group = vec![vec![0_usize; transports.len()]; group_count];
    let mut used_as_transport: std::collections::HashSet<u64> = std::collections::HashSet::new();
    let mut used_as_passenger: std::collections::HashSet<u64> = std::collections::HashSet::new();
    let mut orders: Vec<(u64, u64)> = Vec::new();

    while alive.iter().any(|&group_alive| group_alive) && orders.len() < MAX_ORDERS {
        for group in 0..group_count {
            if !alive[group] || orders.len() >= MAX_ORDERS {
                continue;
            }
            // A vehicle carrier that already served cannot board.
            let mut member = cursors[group];
            while member < groups[group].len() {
                let passenger = &passengers[groups[group][member]];
                if !passenger.infantry && used_as_transport.contains(&passenger.id) {
                    member += 1;
                    continue;
                }
                break;
            }
            if member >= groups[group].len() {
                alive[group] = false;
                continue;
            }
            let passenger_index = groups[group][member];
            let passenger = &passengers[passenger_index];

            let mut best: Option<usize> = None;
            let mut best_distance = 0_i64;
            for (slot, transport) in transports.iter().enumerate() {
                if used_as_passenger.contains(&transport.id)
                    || !admits(passenger, transport, cargo[slot].total_size(), &eligible)
                {
                    continue;
                }
                let distance = dist_sq(passenger.pos, transport.pos);
                let candidate = (
                    assigned_by_group[group][slot],
                    assigned_total[slot],
                    distance,
                );
                let replace = match best {
                    None => true,
                    Some(current) => {
                        candidate
                            < (
                                assigned_by_group[group][current],
                                assigned_total[current],
                                best_distance,
                            )
                    }
                };
                if replace {
                    best = Some(slot);
                    best_distance = distance;
                }
            }
            let Some(slot) = best else {
                // Admission and range vary by passenger, even within a type.
                cursors[group] = member + 1;
                alive[group] = cursors[group] < groups[group].len();
                continue;
            };
            cargo[slot].board_forced_chain(&[(passenger.id, passenger.size)]);
            assigned_total[slot] += 1;
            assigned_by_group[group][slot] += 1;
            used_as_transport.insert(transports[slot].id);
            used_as_passenger.insert(passenger.id);
            orders.push((passenger.id, transports[slot].id));
            cursors[group] = member + 1;
            if cursors[group] >= groups[group].len() {
                alive[group] = false;
            }
        }
    }
    orders
}

/// The selected objects as plan facts. A passenger is a living, unloaded,
/// non-air infantryman or vehicle; a carrier is a selected vehicle of a type
/// with `Passengers=` and `SizeLimit=` that has room now.
fn collect_facts(
    sim: &Simulation,
    rules: &RuleSet,
    selected: &[u64],
) -> (Vec<PassengerFact>, Vec<TransportFact>) {
    let mut passengers = Vec::new();
    let mut transports = Vec::new();
    for &id in selected {
        let Some(entity) = sim.entities().get(id) else {
            continue;
        };
        if !entity.is_alive() || entity.death.is_dying() {
            continue;
        }
        if entity.passenger_role.is_inside_transport() {
            continue;
        }
        let Some(object) = sim.interner().object_type(entity.type_ref(), rules) else {
            continue;
        };
        let category = entity.category();
        let infantry = category == EntityCategory::Infantry;
        if !infantry && category != EntityCategory::Unit {
            continue;
        }
        if entity.movement_layer_or_ground() == MovementLayer::Air {
            continue;
        }
        let pos = object_center_xy(entity);
        // ClickedAction's Enter arm (0x004D76C6) requires vt+A0. A loaded
        // carrier also keeps its transport role for this AutoLoad plan.
        if sim.techno_player_controllable(id, rules)
            && entity
                .passenger_role
                .cargo()
                .is_none_or(PassengerCargo::is_empty)
        {
            passengers.push(PassengerFact {
                id,
                type_key: entity.type_ref().index(),
                size: object.size,
                infantry,
                pos,
            });
        }
        if category == EntityCategory::Unit {
            if let Some(transport) = transport_fact(entity, object, pos) {
                transports.push(transport);
            }
        }
    }
    (passengers, transports)
}

fn transport_fact(
    entity: &GameEntity,
    object: &ObjectType,
    pos: [i32; 2],
) -> Option<TransportFact> {
    if object.passengers <= 0 {
        return None;
    }
    let cargo = entity
        .passenger_role
        .cargo()
        .cloned()
        .unwrap_or_else(|| PassengerCargo::new(object.passengers as u32, object.size_limit));
    Some(TransportFact {
        id: entity.stable_id(),
        capacity: object.passengers,
        size_limit: object.size_limit,
        cargo,
        pos,
    })
}

/// Standard Enter orders for one AutoLoad press, in assignment order.
///
/// Every emitted pair is first put through the engine's own admission query,
/// so an order is only sent when a click there would answer ROGER.
pub(crate) fn autoload_orders(sim: &Simulation, rules: &RuleSet, selected: &[u64]) -> Vec<Command> {
    let (passengers, transports) = collect_facts(sim, rules, selected);
    assign(&passengers, &transports, |passenger_id, transport_id| {
        can_enter_transport(sim, rules, passenger_id, transport_id)
    })
    .into_iter()
    .map(|(passenger_id, transport_id)| Command::MegaMission {
        entity_id: passenger_id,
        mission: MissionType::Enter,
        target: None,
        destination: Some(TargetKind::Entity(transport_id)),
    })
    .collect()
}

/// The Ctrl+D press: plan from the live selection and queue the orders.
pub(crate) fn hotkey_pressed(state: &mut AppState) {
    let Some(owner) = preferred_local_owner_name(state) else {
        return;
    };
    let orders = {
        let Some(runtime) = state.match_state.sim_runtime.as_ref() else {
            return;
        };
        let sim = &runtime.simulation;
        let rules = &runtime.resources.rules;
        let selected = selected_stable_ids_in_order(
            Some(sim),
            Some(rules),
            &state.match_state.input.selection_order,
            state.match_state.input.selection_order_pending,
        );
        autoload_orders(sim, rules, &selected)
    };
    if orders.is_empty() {
        return;
    }
    let count = orders.len();
    for order in orders {
        schedule_command(state, &owner, order);
    }
    log::info!("VeraX AutoLoad: issued {count} enter orders");
}

#[cfg(all(test, feature = "verax"))]
#[path = "autoload_tests.rs"]
mod tests;
