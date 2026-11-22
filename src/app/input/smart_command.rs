//! Ctrl+D smart command: one hotkey, nine mutually-exclusive convenience
//! actions chosen from the *whole* selection. The key only ever emits ordinary
//! `Command`s, so every client and replay sees identical orders; it never
//! mutates simulation state directly.
//!
//! The action is picked by what every selected unit has in common. Because
//! the engine projects `infiltrate = infiltrate | c4 | engineer | agent`, the
//! order of the checks matters: the most specific capability wins, so an
//! Engineer is never mistaken for a spy.
//!
//! The whole feature is single-player only (campaign + skirmish): its target
//! search and "分散" assignment are convenience heuristics whose balance has
//! not been considered for networked play.

use crate::app::AppState;
use crate::app::input::commands::{preferred_local_owner_name, schedule_command};
use crate::app::input::dispatch::selected_stable_ids_in_order;
use crate::map::entities::EntityCategory;
use crate::rules::object_type::ObjectType;
use crate::rules::ruleset::RuleSet;
use crate::sim::combat::TargetKind;
use crate::sim::combat::combat_weapon::is_ally_by_object;
use crate::sim::command::Command;
use crate::sim::game_entity::GameEntity;
use crate::sim::mission::MissionType;
use crate::sim::movement::ground_pose::object_center_xy;
use crate::sim::superweapon::invulnerability::is_invulnerable;
use crate::sim::world::Simulation;

/// Garrison / engineer / sabotage / charge targets within this many cells.
const NEAR_RANGE_CELLS: i64 = 10;
/// Tank-bunker targets are deliberately tighter so a tank never crosses the
/// map to fill a bunker.
const BUNKER_RANGE_CELLS: i64 = 5;
/// Infantry-absorb (bio reactor) targets: tighter still. Keeping this smaller
/// than [`NEAR_RANGE_CELLS`] is what distinguishes "feed a reactor nearby"
/// from "garrison a structure a little further out".
const REACTOR_RANGE_CELLS: i64 = 4;
/// One cell in world leptons (`ObjectClass` Location units).
const LEPTONS_PER_CELL: i64 = 256;
/// Upper bound on the orders one hotkey press may issue.
const MAX_ORDERS: usize = 512;

/// The action Ctrl+D resolves to, in descending order of capability
/// specificity. Ordering is load-bearing — see the module header.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SmartMode {
    C4,
    Transport,
    Engineer,
    Spy,
    Infiltrate,
    Ivan,
    Tesla,
    Garrison,
    Bunker,
    Reactor,
}

/// A selected actor that can perform one of the convenience actions.
#[derive(Debug, Clone, Copy)]
struct Actor {
    id: u64,
    pos: [i32; 2],
}

fn dist_sq(a: [i32; 2], b: [i32; 2]) -> i64 {
    let dx = i64::from(a[0]) - i64::from(b[0]);
    let dy = i64::from(a[1]) - i64::from(b[1]);
    dx * dx + dy * dy
}

fn in_range(a: [i32; 2], b: [i32; 2], cells: i64) -> bool {
    let limit = cells * LEPTONS_PER_CELL;
    dist_sq(a, b) <= limit * limit
}

/// Live, player-controllable surface actors, in selection order. Buildings
/// and air units never act; everything else that is alive and ground-based is
/// a candidate whose common capabilities decide the mode.
fn surface_selection(sim: &Simulation, rules: &RuleSet, selected: &[u64]) -> Vec<Actor> {
    let control = sim.control_projection();
    selected
        .iter()
        .filter_map(|&id| {
            let entity = sim.entities().get(id)?;
            if !entity.is_alive()
                || entity.death.is_dying()
                || entity.category() == EntityCategory::Structure
                || entity.movement_layer_or_ground()
                    == crate::sim::movement::locomotor::MovementLayer::Air
                || !sim.local_techno_controllable_with(&control, id, rules)
            {
                return None;
            }
            Some(Actor {
                id,
                pos: object_center_xy(entity),
            })
        })
        .collect()
}

fn object_of<'a>(
    sim: &Simulation,
    entity: &GameEntity,
    rules: &'a RuleSet,
) -> Option<&'a ObjectType> {
    sim.interner().object_type(entity.type_ref(), rules)
}

/// A Tesla charger is identified by its secondary weapon's warhead carrying
/// `ElectricAssault=yes`, exactly as the simulation's own auto-charge does —
/// never by a weapon name string.
fn is_tesla_charger(sim: &Simulation, rules: &RuleSet, id: u64) -> bool {
    let Some(entity) = sim.entities().get(id) else {
        return false;
    };
    let Some(object) = object_of(sim, entity, rules) else {
        return false;
    };
    object
        .weapon_at(1)
        .and_then(|name| rules.weapon(name))
        .and_then(|weapon| rules.weapon_warhead(weapon))
        .is_some_and(|warhead| warhead.electric_assault)
}

/// Determine one action from the whole selection. More specific infantry
/// capabilities are checked first so the projected `infiltrate` flag cannot
/// swallow engineers, C4 troops or spies.
fn detect_mode(sim: &Simulation, rules: &RuleSet, actors: &[Actor]) -> Option<SmartMode> {
    if actors.is_empty() {
        return None;
    }
    // Transport loading is the only mixed selection: a carrier plus anything
    // it can board. It is checked first because a carrier's own capability
    // flags would otherwise make the "all are X" infantry tests fail anyway.
    let has_carrier = actors.iter().any(|actor| {
        sim.entities()
            .get(actor.id)
            .and_then(|entity| object_of(sim, entity, rules))
            .is_some_and(|object| object.passengers > 0)
    });
    let has_boarder = actors.iter().any(|actor| {
        let Some(entity) = sim.entities().get(actor.id) else {
            return false;
        };
        let infantry = entity.category() == EntityCategory::Infantry;
        infantry || entity.category() == EntityCategory::Unit
    });
    if has_carrier && has_boarder && actors.len() > 1 {
        return Some(SmartMode::Transport);
    }

    // Give each unit exactly one role (most specific capability first), then
    // require every role to match. This is the "清一色" rule: a mixed group
    // never falls through to a broad category such as Reactor.
    let roles: Vec<Option<SmartMode>> = actors
        .iter()
        .map(|actor| role_of(sim, rules, actor))
        .collect();
    let Some(first) = roles[0] else {
        return None;
    };
    if !roles.iter().all(|role| *role == Some(first)) {
        return None;
    }
    // An `Occupier=` rifleman is also a plain infantryman, so `role_of` would
    // always resolve him to Garrison and the reactor branch would be dead.
    // When a bio reactor is right next to the squad, feeding it wins over a
    // garrisonable building further out: the reactor search is deliberately
    // tighter than the garrison search for exactly this reason.
    if matches!(first, SmartMode::Garrison | SmartMode::Reactor)
        && nearby_infantry_absorb(sim, rules, actors)
    {
        return Some(SmartMode::Reactor);
    }
    Some(first)
}

/// A bio reactor with its remaining `Passengers=` slots. Carrying the free
/// capacity on the target lets both detection and planning agree on which
/// reactors can still take a soldier, so a full reactor is never fed.
#[derive(Debug, Clone, Copy)]
struct ReactorTarget {
    target: Target,
    free: u32,
}

/// Living, non-grinding bio reactors within the (tight) reactor range of any
/// actor, paired with their remaining cargo slots. This mirrors the engine's
/// own `can_load` absorbs gate (`radio/receive.rs`): `cargo.count() <
/// Passengers`. The result is shared by [`nearby_infantry_absorb`] and the
/// Reactor plan so detection and ordering can never disagree about what can
/// be fed.
fn nearby_reactors(sim: &Simulation, rules: &RuleSet, actors: &[Actor]) -> Vec<ReactorTarget> {
    let mut out = Vec::new();
    for entity in sim.entities().values_of_category(EntityCategory::Structure) {
        if !entity.is_alive() || entity.death.is_dying() {
            continue;
        }
        let Some(object) = object_of(sim, entity, rules) else {
            continue;
        };
        if !object.infantry_absorb || object.grinding {
            continue;
        }
        let capacity = object.passengers.max(0) as u32;
        let aboard = entity
            .passenger_role()
            .cargo()
            .map_or(0, |cargo| cargo.count());
        let free = capacity.saturating_sub(aboard);
        if free == 0 {
            continue;
        }
        let pos = object_center_xy(entity);
        if actors
            .iter()
            .any(|actor| in_range(actor.pos, pos, REACTOR_RANGE_CELLS))
        {
            out.push(ReactorTarget {
                target: Target {
                    id: entity.stable_id(),
                    pos,
                },
                free,
            });
        }
    }
    out
}

/// Whether any actor stands next to a bio reactor that still has room. A
/// full reactor must not flip a garrison selection into Reactor mode — there
/// is nothing to feed and the soldiers should keep their garrison order.
fn nearby_infantry_absorb(sim: &Simulation, rules: &RuleSet, actors: &[Actor]) -> bool {
    !nearby_reactors(sim, rules, actors).is_empty()
}

/// The single most specific convenience role of one unit. `infiltrate` is
/// projected onto c4/engineer/agent, so it is checked last among infantry and
/// only when none of the specific flags applies.
fn role_of(sim: &Simulation, rules: &RuleSet, actor: &Actor) -> Option<SmartMode> {
    let entity = sim.entities().get(actor.id)?;
    let object = object_of(sim, entity, rules)?;
    if object.engineer {
        return Some(SmartMode::Engineer);
    }
    if object.c4 {
        return Some(SmartMode::C4);
    }
    if object.agent {
        return Some(SmartMode::Spy);
    }
    if object.ivan {
        return Some(SmartMode::Ivan);
    }
    if is_tesla_charger(sim, rules, actor.id) {
        return Some(SmartMode::Tesla);
    }
    if object.infiltrate {
        return Some(SmartMode::Infiltrate);
    }
    if object.occupier {
        return Some(SmartMode::Garrison);
    }
    match entity.category() {
        EntityCategory::Unit if object.bunkerable => Some(SmartMode::Bunker),
        EntityCategory::Infantry => Some(SmartMode::Reactor),
        _ => None,
    }
}

/// A target building within range of an actor.
#[derive(Debug, Clone, Copy)]
struct Target {
    id: u64,
    pos: [i32; 2],
}

/// Collect live structures of `category` within `cells` of any actor and
/// accepted by `admit`. Each target appears once.
fn nearby_targets(
    sim: &Simulation,
    rules: &RuleSet,
    actors: &[Actor],
    cells: i64,
    admit: impl Fn(&GameEntity, &ObjectType) -> bool,
) -> Vec<Target> {
    let mut out = Vec::new();
    for entity in sim.entities().values_of_category(EntityCategory::Structure) {
        if !entity.is_alive() || entity.death.is_dying() {
            continue;
        }
        let Some(object) = object_of(sim, entity, rules) else {
            continue;
        };
        if !admit(entity, object) {
            continue;
        }
        let pos = object_center_xy(entity);
        if actors.iter().any(|actor| in_range(actor.pos, pos, cells)) {
            out.push(Target {
                id: entity.stable_id(),
                pos,
            });
        }
    }
    out
}

/// Evenly spread `actors` over `targets`: each actor takes the nearest target
/// that still has a free slot, preferring targets with the fewest assignments.
/// `slots` is a per-target cap; callers that bomb or charge pass a small value,
/// callers that fill buildings pass [`spread_all`]. Returns `(actor, target)`.
fn spread(actors: &[Actor], targets: &[Target], slots: u32) -> Vec<(u64, u64)> {
    if actors.is_empty() || targets.is_empty() {
        return Vec::new();
    }
    let mut assigned = vec![0_u32; targets.len()];
    let mut orders = Vec::new();
    for actor in actors {
        let mut best: Option<usize> = None;
        for (index, target) in targets.iter().enumerate() {
            if assigned[index] >= slots {
                continue;
            }
            let candidate = (assigned[index], dist_sq(actor.pos, target.pos));
            let replace = best
                .map(|cur| candidate < (assigned[cur], dist_sq(actor.pos, targets[cur].pos)))
                .unwrap_or(true);
            if replace {
                best = Some(index);
            }
        }
        if let Some(index) = best {
            assigned[index] += 1;
            orders.push((actor.id, targets[index].id));
        }
        if orders.len() >= MAX_ORDERS {
            break;
        }
    }
    orders
}

/// Give every selected actor an order, balancing the load over the available
/// buildings. Garrison houses hold a whole squad, reactors absorb a crowd and
/// one infiltration team can enter several structures at once, so the
/// per-target cap must be the whole selection rather than one. The simulation
/// still rejects the overflow when a building is actually full.
fn spread_all(actors: &[Actor], targets: &[Target]) -> Vec<(u64, u64)> {
    spread(actors, targets, actors.len() as u32)
}

/// Like [`spread`], but each target carries its own free-slot count. Used by
/// the reactor plan so a reactor with one vacancy gets one soldier, never a
/// whole squad, and a full reactor gets none. The `cap` is a soft ceiling —
/// ordering more than the engine admits is harmless (it refuses the excess),
/// but ordering less would silently leave free slots unused.
fn spread_by_capacity(actors: &[Actor], targets: &[ReactorTarget]) -> Vec<(u64, u64)> {
    let plain: Vec<Target> = targets.iter().map(|entry| entry.target).collect();
    let caps: Vec<u32> = targets.iter().map(|entry| entry.free).collect();
    if actors.is_empty() || targets.is_empty() {
        return Vec::new();
    }
    let mut assigned = vec![0_u32; targets.len()];
    let mut orders = Vec::new();
    for actor in actors {
        let mut best: Option<usize> = None;
        for (index, target) in plain.iter().enumerate() {
            if assigned[index] >= caps[index] {
                continue;
            }
            let candidate = (assigned[index], dist_sq(actor.pos, target.pos));
            let replace = best
                .map(|cur| candidate < (assigned[cur], dist_sq(actor.pos, plain[cur].pos)))
                .unwrap_or(true);
            if replace {
                best = Some(index);
            }
        }
        if let Some(index) = best {
            assigned[index] += 1;
            orders.push((actor.id, plain[index].id));
        }
        if orders.len() >= MAX_ORDERS {
            break;
        }
    }
    orders
}

fn ally_of(sim: &Simulation, actor: u64, target: u64) -> bool {
    let (Some(a), Some(b)) = (sim.entities().get(actor), sim.entities().get(target)) else {
        return false;
    };
    is_ally_by_object(
        Some(sim.house_alliances()),
        sim.interner(),
        a.owner(),
        b.owner(),
    )
}

/// Build the orders for a resolved mode. Targets and admission reuse the
/// engine's own read-only queries so a command is only sent where the native
/// click would also be admitted.
fn plan_mode(sim: &Simulation, rules: &RuleSet, actors: &[Actor], mode: SmartMode) -> Vec<Command> {
    match mode {
        SmartMode::Transport => crate::app::input::autoload::autoload_orders(
            sim,
            rules,
            &actors.iter().map(|a| a.id).collect::<Vec<_>>(),
        ),
        SmartMode::C4 => {
            let targets = nearby_targets(sim, rules, actors, NEAR_RANGE_CELLS, |entity, object| {
                object.can_c4
                    && !object.invisible_in_game
                    && !object.is_1x1_with_undeploy()
                    && !is_invulnerable(
                        entity.invulnerability.as_ref(),
                        sim.session().binary_frame(),
                    )
            });
            spread(actors, &targets, 1)
                .into_iter()
                // Sabotage only hostile/neutral buildings; never blow up an
                // ally (PlantC4's gate does not check alliance itself).
                .filter(|&(actor, target)| !ally_of(sim, actor, target))
                .map(|(actor, target)| Command::PlantC4 {
                    attacker_id: actor,
                    target_building_id: target,
                })
                .collect()
        }
        SmartMode::Engineer => {
            let targets =
                nearby_targets(sim, rules, actors, NEAR_RANGE_CELLS, |_entity, object| {
                    object.capturable || object.can_be_occupied
                });
            spread_all(actors, &targets)
                .into_iter()
                .filter(|&(actor, target)| {
                    matches!(
                        sim.engineer_building_action(actor, target, rules),
                        Some(crate::sim::world::EngineerBuildingAction::Capture)
                            | Some(crate::sim::world::EngineerBuildingAction::Damage)
                            | Some(crate::sim::world::EngineerBuildingAction::Repair(true))
                    )
                })
                .map(|(actor, target)| Command::CaptureBuilding {
                    engineer_id: actor,
                    target_building_id: target,
                })
                .collect()
        }
        SmartMode::Spy | SmartMode::Infiltrate => {
            let targets =
                nearby_targets(sim, rules, actors, NEAR_RANGE_CELLS, |_entity, _object| {
                    true
                });
            spread_all(actors, &targets)
                .into_iter()
                .filter(|&(actor, target)| {
                    !ally_of(sim, actor, target)
                        && sim.infiltrator_building_action(actor, target, rules)
                        && !matches!(
                            sim.infiltrate_action(actor, target, rules),
                            crate::sim::world::InfiltrateAction::None
                        )
                })
                .map(|(actor, target)| Command::CaptureBuilding {
                    engineer_id: actor,
                    target_building_id: target,
                })
                .collect()
        }
        SmartMode::Ivan => {
            // Two independent reasons the old version issued zero orders:
            // `values()` includes the selected Ivans themselves at distance
            // zero, so `spread` always handed each Ivan his own id, and the
            // trailing `actor != target` filter then dropped every order.
            // Exclude the actors from the candidate set *before* spreading so
            // the nearest eligible object can actually be picked.
            let mut targets: Vec<Target> = Vec::new();
            for entity in sim.entities().values() {
                if !entity.is_alive() || entity.death.is_dying() {
                    continue;
                }
                let id = entity.stable_id();
                if actors.iter().any(|actor| actor.id == id) {
                    continue;
                }
                // Match the native `IvanBomb` cursor: a target already carrying
                // a bomb, or one that is not `Bombable=`, is refused outright.
                if entity.bomb.is_some() {
                    continue;
                }
                let Some(object) = object_of(sim, entity, rules) else {
                    continue;
                };
                if !object.bombable {
                    continue;
                }
                let pos = object_center_xy(entity);
                if actors
                    .iter()
                    .any(|actor| in_range(actor.pos, pos, NEAR_RANGE_CELLS))
                {
                    targets.push(Target { id, pos });
                }
            }
            spread(actors, &targets, 1)
                .into_iter()
                // Never let an Ivan bomb an ally, matching the C4 guard.
                .filter(|&(actor, target)| !ally_of(sim, actor, target))
                .map(|(actor, target)| Command::Attack {
                    attacker_id: actor,
                    target_id: target,
                })
                .collect()
        }
        SmartMode::Tesla => {
            let targets =
                nearby_targets(sim, rules, actors, NEAR_RANGE_CELLS, |_entity, object| {
                    object.overpowerable
                });
            spread(actors, &targets, 3)
                .into_iter()
                .filter(|&(actor, target)| ally_of(sim, actor, target))
                .map(|(actor, target)| Command::Attack {
                    attacker_id: actor,
                    target_id: target,
                })
                .collect()
        }
        SmartMode::Garrison => {
            let targets =
                nearby_targets(sim, rules, actors, NEAR_RANGE_CELLS, |_entity, object| {
                    object.can_be_occupied && !object.grinding
                });
            spread_all(actors, &targets)
                .into_iter()
                .filter(|&(actor, target)| {
                    let (Some(p), Some(b)) =
                        (sim.entities().get(actor), sim.entities().get(target))
                    else {
                        return false;
                    };
                    let (Some(po), Some(bo)) = (object_of(sim, p, rules), object_of(sim, b, rules))
                    else {
                        return false;
                    };
                    crate::sim::passenger::can_dock_occupier_garrison(
                        p,
                        b,
                        po,
                        bo,
                        b.passenger_role().occupants(),
                        rules,
                        sim,
                    )
                })
                .map(|(actor, target)| Command::CaptureBuilding {
                    engineer_id: actor,
                    target_building_id: target,
                })
                .collect()
        }
        SmartMode::Bunker => {
            let targets =
                nearby_targets(sim, rules, actors, BUNKER_RANGE_CELLS, |entity, object| {
                    object.bunker
                        && entity.building.bunker_runtime.is_some()
                        && entity.bunker.occupant().is_none()
                        && entity.seldom.radio_contacts.slot(0).is_none()
                });
            spread(actors, &targets, 1)
                .into_iter()
                .filter(|&(actor, _target)| {
                    crate::sim::docking::bunker_link::can_auto_deploy_here(sim, actor, rules)
                })
                .map(|(actor, target)| Command::MegaMission {
                    entity_id: actor,
                    mission: MissionType::Enter,
                    target: None,
                    destination: Some(TargetKind::Entity(target)),
                })
                .collect()
        }
        SmartMode::Reactor => {
            // Only reactors with a free `Passengers=` slot are targets, and
            // each gets at most its own vacancy. A far bigger squad than the
            // total number of slots simply leaves the excess soldiers where
            // they stand instead of sending a fresh wave on every press.
            let reactors = nearby_reactors(sim, rules, actors);
            spread_by_capacity(actors, &reactors)
                .into_iter()
                .map(|(actor, target)| Command::MegaMission {
                    entity_id: actor,
                    mission: MissionType::Enter,
                    target: None,
                    destination: Some(TargetKind::Entity(target)),
                })
                .collect()
        }
    }
}

/// The Ctrl+D press. Single-player only; resolves one mode from the whole
/// selection and schedules its orders with the shared unified feedback line.
pub(crate) fn hotkey_pressed(state: &mut AppState) {
    if !state.match_state.session_mode().is_single_player() {
        return;
    }
    let Some(owner) = preferred_local_owner_name(state) else {
        return;
    };
    let orders = {
        let Some(runtime) = state.match_state.sim_runtime.runtime() else {
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
        let actors = surface_selection(sim, rules, &selected);
        match detect_mode(sim, rules, &actors) {
            Some(mode) => plan_mode(sim, rules, &actors, mode),
            None => Vec::new(),
        }
    };
    if orders.is_empty() {
        return;
    }
    let count = orders.len();
    for order in orders {
        schedule_command(state, &owner, order);
    }
    crate::app::input::messages::post_eva_auto_loaded(state, count);
    log::info!("Smart command: issued {count} orders");
}

#[cfg(all(test, tests_app_input_d_z))]
#[path = "smart_command_tests.rs"]
mod tests;
