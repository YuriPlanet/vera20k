//! Command admission and issuing: the one door every order in the assist tree
//! goes through.
//!
//! Parts: none. This module is the admission half of the orders layer; the
//! decisions themselves stay in the parent.
//!
//! Dependency rules: same as assist/.

use crate::rules::locomotor_type::MovementZone;
use crate::rules::object_type::ObjectCategory;
use crate::rules::ruleset::RuleSet;
use crate::sim::control::Goal;
use crate::sim::control::brain::Outbox;
use crate::sim::control::observation::Observation;
use crate::sim::strategy::assist::AssistBrain;

/// Whether an earlier step already gave this unit an order this frame. The
/// steps run in priority order — a bridge before a capture, a capture before a
/// guard — and a unit is one unit: the first answer wins.
pub(in crate::sim::strategy::assist) fn ordered_this_frame(out: &Outbox, actor: u64) -> bool {
    out.ordered_this_frame(actor)
}

/// Whether `goal` names a target this unit can actually act on.
///
/// The brain aims units at what the house can see, never at what the unit can
/// do: the nearest hostile is handed to whoever is nearest, so a dog is pointed
/// at a tank whose armour its `Parasite=yes` warhead cannot touch, and an
/// engineer at a building that answers Repair rather than Capture. Both are
/// orders the object refuses on arrival — the unit walks out, stands at the
/// target and does nothing — which reads as a stuck unit and is not one.
///
/// The answer is the simulation's own, not a table here: `GetFireError` for
/// whether a weapon may fire ([`Observation::can_engage`]), the armour floor
/// the native target scan applies for whether it does anything at all
/// ([`Observation::can_hurt`]), and `What_Action_OnObject` for a specialist
/// ([`Observation::can_take_building`]). That is the same answer the player's
/// click gets, so a pass cannot invent a rule the engine does not have, and
/// every object a mod adds is covered without another list.
///
/// Goals that name a cell rather than an object — a move, an attack-move, a
/// guard post — have no target to be wrong about; the engine owns whether the
/// ground can be reached. What they can be wrong about is the map: a fog of
/// war scan, a squad's staging offset or a retreat post can land outside it,
/// and the receiver clamps or drops such a command silently, which leaves the
/// unit marching at nothing. [`issue`] drops them here instead.
/// Whether `unit` is already shooting `target`.
///
/// The engine's mission owns the approach once it holds a target: `Mission_Attack`
/// walks the unit into range itself, and `Queue_Mission` restarts that mission
/// from its opening state. A brain that re-issues the same `Attack` every frame
/// therefore never lets the unit leave the opening state — the order is only
/// new when the target is new, and that is the one thing this answers.
pub(in crate::sim::strategy::assist) fn already_attacking(
    obs: &Observation<'_>,
    unit: u64,
    target: u64,
) -> bool {
    obs.attack_target(unit) == Some(target)
}

/// Whether `unit` may be given `goal` at all.
///
/// The hostility test is here and not only in [`super::board::order_stands`]
/// because this is the one gate every order goes through, and the other is the
/// "does this order still stand" layer: a squad that captured the refinery it
/// was shelling reached this function with a legal target — `can_engage` and
/// `can_hurt` are both true against a friendly building, since neither asks
/// whose it is — and the order went out. The wishlist's "占领后仍然自己打自己"
/// is that gap.
pub(in crate::sim::strategy::assist) fn admits(
    rules: &RuleSet,
    obs: &Observation<'_>,
    unit: u64,
    goal: &Goal,
) -> bool {
    match *goal {
        Goal::Attack { target } => {
            obs.is_hostile(target)
                && obs.can_engage(rules, unit, target)
                && obs.can_hurt(rules, unit, target)
        }
        Goal::Capture { target } => obs.can_take_building(rules, unit, target),
        _ => true,
    }
}

/// Order `goal` for `unit`, if its target is one the unit can act on.
///
/// The one door every pass in this tree uses, so a step cannot hand out what
/// the engine will refuse. `false` when the target is not one the unit can act
/// on: nothing is ordered, nothing is recorded, and the caller is free to pick
/// another target or leave the unit alone.
pub(in crate::sim::strategy::assist) fn issue(
    brain: &AssistBrain,
    out: &mut Outbox,
    rules: &RuleSet,
    obs: &Observation<'_>,
    unit: u64,
    goal: Goal,
) -> bool {
    // One order per unit per frame, and it is the strongest ask that stands:
    // the outbox keeps the best rank each object was asked for, so a pass
    // cannot take an object from a stronger one merely by running later. The
    // rank is the intent the caller recorded for this unit — the same ladder
    // `Tracker::admits` uses across frames, so both axes answer alike.
    let rank = brain
        .tracked
        .rank_of(obs.tick(), unit)
        .unwrap_or(crate::sim::control::brain::ORDINARY_RANK);
    // The map is the other thing a goal can be wrong about, and neither the
    // funnel nor `Funnel::prepare` looks at it: `Command::Move` to a cell past
    // the edge is accepted, the nav target is never reached, and the unit
    // spends the rest of the order walking at the boundary.
    if goal_cell(&goal).is_some_and(|cell| !cell_in_bounds(obs, cell)) {
        log::debug!(
            "auto play: {} refused for {unit}: the cell is off the map",
            format!("{goal:?}")
        );
        return false;
    }
    // The map's second answer: on it, and reachable. A destination on the far
    // side of a wall, a cliff or an island is accepted by the receiver, the
    // nav target is never reached, and the mission stays on `Move` — the unit
    // stands against the obstacle while its intent holds the layer off for the
    // whole term. Refused here it is re-decided next frame instead.
    if let Some(cell) = goal_cell(&goal)
        && !reachable(rules, obs, unit, cell)
    {
        // The refusal is kept. A walk the map will not take is not "nothing
        // to do": something stands between the unit and the cell, and that
        // something is what a player clicks next. Without the record the unit
        // stands in front of it for the rest of the match — the report of a
        // unit parked at a gate it never shoots.
        out.blocked.push((unit, cell, obs.tick()));
        log::debug!("auto play: {goal:?} refused for {unit}: no way there from here");
        return false;
    }
    // The engine's mission owns the approach once it holds a target, and
    // `Queue_Mission` restarts that mission. Re-issuing the same attack every
    // frame therefore keeps the unit in `Mission_Attack`'s opening state and it
    // never closes the distance — the reported "it is ordered to attack and
    // never attacks". One gate, here, so every caller is covered by one rule.
    if let Goal::Attack { target } = goal
        && already_attacking(obs, unit, target)
    {
        log::debug!(
            "auto play: {} for {unit} is already its mission; not re-issued",
            format!("{goal:?}")
        );
        return false;
    }
    // An aircraft out of ammunition has its own way home: the engine's return
    // to the airfield, the rearm and the take-off again are one mission, and an
    // `Attack` issued over it replaces that mission with a flight at the target
    // — the plane arrives with empty racks, turns around, and does it again.
    // That is the "it keeps taking off and reloading" a play test reports, and
    // it costs the house an aircraft every time the target shoots back.
    //
    // Only the flying case: an `Ammo=` on a ground type is not a reload cycle
    // with a building behind it, and grounding those would be a new rule about
    // types the rules never wrote one for.
    if matches!(goal, Goal::Attack { .. } | Goal::AttackMove { .. })
        && obs.out_of_ammo(unit)
        && obs
            .entity_type(rules, unit)
            .is_some_and(|ty| ty.category == ObjectCategory::Aircraft)
    {
        log::debug!("auto play: {goal:?} for {unit} waits for the rearm");
        return false;
    }
    if !admits(rules, obs, unit, &goal) {
        log::debug!(
            "auto play: {} refused for {unit}: the target is not one it can act on",
            format!("{goal:?}")
        );
        return false;
    }
    let taken = out.want(unit, rank, goal);
    if brain.trace == Some(unit) {
        log::info!(
            "trace {unit}: {} {} (rank {rank}, taken {taken})",
            format!("{goal:?}"),
            if taken { "issued" } else { "replaced" }
        );
    }
    taken
}

/// Whether the walker can get to `cell` at all.
///
/// A cell can be on the map, open, and still sit on the far side of a wall, a
/// cliff or an island; a walk ordered to it never arrives. The connectivity
/// answer is the zone map's, in the unit's own movement zone, because a boat's
/// map is not a dog's. A flying unit is not asked: its route is not the ground
/// layer's, and the zone map's rows are.
pub(in crate::sim::strategy::assist) fn reachable(
    rules: &RuleSet,
    obs: &Observation<'_>,
    unit: u64,
    cell: (u16, u16),
) -> bool {
    if !obs.has_zone_grid() {
        return true;
    }
    let Some((from, type_id)) = obs.object_cell_and_type(unit) else {
        return true;
    };
    let Some(ty) = obs.object_type(rules, type_id) else {
        return true;
    };
    if ty.movement_zone == MovementZone::Fly {
        return true;
    }
    obs.can_reach(ty.movement_zone, from, cell)
}

/// The cell a goal names, for the bounds check: one per goal, because every
/// order this brain issues names at most one.
fn goal_cell(goal: &Goal) -> Option<(u16, u16)> {
    match *goal {
        Goal::AttackCell { cell }
        | Goal::AttackMove { cell }
        | Goal::Move { cell }
        | Goal::GuardAt { cell }
        | Goal::Harvest { cell } => Some(cell),
        _ => None,
    }
}

/// Whether `cell` is on a map of `extent`. `rx`/`ry` are zero-based, so the
/// extent is the first cell past the edge.
///
/// A zero extent is "no map", not an empty one: a headless sim and a test
/// fixture both run without fog, and refusing every cell goal there would take
/// the army off the board.
///
/// This is the one answer to "is this cell on the map", and it is asked by the
/// passes that *pick* a cell as well as by the door that refuses one: a step
/// that would leave the map is not a candidate at all, so a unit at the edge
/// is given the step that stays instead of being given none.
pub(in crate::sim::strategy::assist) fn on_map(cell: (u16, u16), extent: (u16, u16)) -> bool {
    (extent.0 == 0 || cell.0 < extent.0) && (extent.1 == 0 || cell.1 < extent.1)
}

/// Whether `cell` is on this map: the extent the projection reports.
fn cell_in_bounds(obs: &Observation<'_>, cell: (u16, u16)) -> bool {
    on_map(cell, obs.map_extent())
}
