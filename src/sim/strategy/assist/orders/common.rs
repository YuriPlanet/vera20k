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
            // A remembered position may be searched by AttackMove, but an
            // entity-id Attack is a click on a currently visible object.
            !super::protected_neutral(rules,obs,target)
                && ((obs.is_hostile(target)
                && obs.visible_enemy_entities().any(|enemy| enemy.entity_id == target))
                || obs.can_control_neutral(rules, unit, target))
                && obs.can_engage(rules, unit, target)
                && super::super::doctrine::target_ok(rules,obs,unit,target)
                && (obs.can_control_neutral(rules, unit, target)
                    || obs.can_hurt(rules, unit, target)
                    || (obs.entity_type(rules, target)
                        .is_some_and(|ty| ty.category == ObjectCategory::Infantry)
                        && obs.entity_type(rules, unit)
                            .is_some_and(|ty| ty.agent
                                && super::has_disguise_weapon(rules, ty))))
        }
        // Expendable and bomb-planting units cannot be given autonomous
        // AttackMove/Guard. The native guard target scan may pick a rifleman
        // in the middle of our own army and detonate there. Their explicit
        // target orders are handled by orders::suicide_errand/bomb_errand.
        Goal::AttackMove { .. } | Goal::Guard | Goal::GuardAt { .. }
            if super::super::doctrine::actor_duty(rules,obs,unit).is_some_and(|duty| matches!(
                duty, super::super::doctrine::Duty::Disposable |
                    super::super::doctrine::Duty::BombPlanter)) => false,
        Goal::AttackCell { .. }
            if super::super::doctrine::actor_duty(rules,obs,unit).is_some_and(|duty| matches!(
                duty, super::super::doctrine::Duty::Disposable |
                    super::super::doctrine::Duty::BombPlanter)) => false,
        Goal::Capture { target } => {
            // Ownership or current visibility is required for every fresh
            // Capture click, including neutral assets and empty garrisons.
            obs.observed_object_cell(target).is_some()
                && (!obs.is_hostile(target)
                    || obs.visible_enemy_entities().any(|enemy| enemy.entity_id==target))
                && obs.can_take_building(rules, unit, target)
        }
        Goal::Grind { target } => obs.controlled_by_own_controller(unit)
            && obs.can_enter_grinder(rules, unit, target),
        Goal::MoveToObject { target } | Goal::Enter { target }
            if obs.is_hostile(target) => {
                obs.visible_enemy_entities().any(|enemy| enemy.entity_id == target)
            }
        _ => true,
    }
}

/// Issue a request with its *proposed* priority, without changing the tracker.
///
/// The world and the frame arbiter may both reject a proposed order. The
/// tracker must only claim the unit after this function says the outbox took
/// it; otherwise a scout can wait out a 450-frame Move for a walk that never
/// left the brain. This is also why rank is supplied by the proposed kind:
/// reading the old record would give the new request the wrong priority.
fn issue_at_rank(
    trace: Option<u64>,
    rank: u8,
    out: &mut Outbox,
    rules: &RuleSet,
    obs: &Observation<'_>,
    unit: u64,
    goal: Goal,
) -> bool {
    // The native door/track owns this product until its factory radio contact
    // is released. Do not record a tactical task for an unready product.
    if obs.own_factory_handoff(rules, unit).is_some() { return false; }
    // A mobile construction yard may appear from a crate, not only a war
    // factory. Its value is the future base: never borrow it for an assault,
    // even while the squads are waiting for their next regroup. Only a
    // normal Move/Deploy from the expansion owner may steer it.
    if matches!(goal, Goal::Attack { .. } | Goal::AttackMove { .. } | Goal::Guard)
        && obs.entity_type(rules, unit)
            .is_some_and(|ty| super::super::expand::makes_a_yard(rules, ty))
    {
        log::debug!("auto play: rejecting combat order {goal:?} on mobile yard {unit}");
        return false;
    }
    // Engineer capture admission is shared by bridge, asset, retry and
    // ordinary capture passes. A new target does not inherit the old escort's
    // safety verdict. The final arbiter also checks native ongoing missions.
    if let Goal::Capture { target } = goal
        && obs.entity_type(rules, unit).is_some_and(|ty| ty.engineer)
        && !super::super::safety::specialist_approach_safe(rules,obs,unit,target)
    { return false; }
    if let Goal::Capture {target}=goal
        && obs.entity_type(rules,unit).is_some_and(|ty|ty.occupier && !ty.engineer && !ty.agent)
        && !super::super::safety::garrison_approach_safe(rules,obs,unit,target)
    {return false;}
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
    if trace == Some(unit) {
        log::info!(
            "trace {unit}: {} {} (rank {rank}, taken {taken})",
            format!("{goal:?}"),
            if taken { "issued" } else { "replaced" }
        );
    }
    taken
}

/// Issue an order whose caller already recorded its intent. Kept for the
/// older steps while they migrate to [`issue_tracked`].
pub(in crate::sim::strategy::assist) fn issue(
    brain: &AssistBrain,
    out: &mut Outbox,
    rules: &RuleSet,
    obs: &Observation<'_>,
    unit: u64,
    goal: Goal,
) -> bool {
    if !brain.tracked.tasks.permits(unit, None, goal) { return false; }
    let rank = brain
        .tracked
        .rank_of(obs.tick(), unit)
        .unwrap_or(crate::sim::control::brain::ORDINARY_RANK);
    issue_at_rank(brain.trace, rank, out, rules, obs, unit, goal)
}

/// A real, still-active order to enter a garrisonable building. This is
/// shared by the ordinary order gate and the exceptional low-level retreat
/// passes: otherwise a tactical `Stop` can still kill the Capture mission
/// even when every normal attack/micro request is blocked.
pub(in crate::sim::strategy::assist) fn committed_garrison_entry(
    tracker: &super::super::tracked::Tracker,
    rules: &RuleSet, obs: &Observation<'_>, unit: u64,
) -> bool {
    let Some(Goal::Capture { target }) = tracker.primary_goal(unit) else {
        return false;
    };
    tracker.confirmed_capture(obs.tick(), unit, target)
        && obs.inside_transport(unit).is_none()
        && obs.entity_type(rules, target)
            .is_some_and(|ty| ty.category == ObjectCategory::Building && ty.can_be_occupied)
}

/// Check and issue a tracked order as one transaction: a failed ask cannot
/// replace a standing intent, and cannot claim a unit for an order the engine
/// will never see. The frame may still reject the accepted ask later; in that
/// case L2 calls `Brain::orders_refused` to release it with a short cooldown.
pub(in crate::sim::strategy::assist) fn issue_tracked(
    tracker: &mut super::super::tracked::Tracker,
    trace: Option<u64>,
    out: &mut Outbox,
    rules: &RuleSet,
    obs: &Observation<'_>,
    unit: u64,
    kind: super::super::tracked::TrackedKind,
    target: Option<u64>,
    cell: Option<(u16, u16)>,
    goal: Goal,
) -> bool {
    let tick = obs.tick();
    // A confirmed Capture into a civilian house owns the pedestrian until
    // the entrance is reached. A later tactical Attack/Move/Dodge must not
    // delete that native Capture while he is exposed outside the door. This
    // is deliberately bounded by the tracked 300-tick term; stuck infantry
    // become eligible for re-planning instead of being frozen forever.
    if committed_garrison_entry(tracker, rules, obs, unit)
        && !matches!(goal, Goal::Capture { target }
            if tracker.primary_goal(unit) == Some(Goal::Capture { target }))
    {
        return false;
    }
    if !tracker.tasks.permits(unit, Some(kind), goal) { return false; }
    if !tracker.admits(tick, unit, kind, target, cell) {
        return false;
    }
    if !issue_at_rank(
        trace,
        super::super::tracked::rank(kind),
        out,
        rules,
        obs,
        unit,
        goal,
    ) {
        return false;
    }
    tracker.record_goal(tick, unit, kind, target, cell, goal);
    true
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

#[cfg(all(test, tests_sim_p_s))]
mod tests {
    use super::*;
    use super::super::super::tracked::{TrackedKind, Tracker};

    fn arena() -> Option<(crate::sim::world::Simulation, RuleSet)> {
        let rules = crate::rules::retail_ini_fixture::retail_unlayered_rules()?;
        let sim = crate::sim::arena_fixture::rival_arena_known(&rules, 0xA17);
        Some((sim, rules))
    }

    /// A bad scout destination is not an order, and must not hold the dog
    /// idle for `Move`'s 450-frame term. A legal post can be chosen instead.
    #[test]
    fn a_rejected_scout_walk_does_not_claim_its_unit() {
        let Some((sim, rules)) = arena() else { return };
        let owner = sim.houses().keys().next().copied().expect("a house");
        let obs = Observation::new(&sim, owner);
        assert!(obs.map_extent().0 > 0, "the fixture has real map bounds");
        let mut tracker = Tracker::new();
        let mut out = Outbox::default();
        let outside = (u16::MAX, u16::MAX);
        assert!(!issue_tracked(
            &mut tracker, None, &mut out, &rules, &obs, 7,
            TrackedKind::Move, None, Some(outside), Goal::Move { cell: outside },
        ));
        assert!(!tracker.is_claimed(obs.tick(), 7));
        assert!(out.orders.is_empty());
        assert!(issue_tracked(
            &mut tracker, None, &mut out, &rules, &obs, 7,
            TrackedKind::Guard, None, None, Goal::Guard,
        ));
        assert!(tracker.stands(obs.tick(), 7, &[TrackedKind::Guard]));
    }

    /// Only a successful replacement may change the tracker. Even if a
    /// proposed dodge ranks above a standing guard, a rejected cell goal
    /// cannot erase the guard or give the unit an imaginary dodge.
    #[test]
    fn a_rejected_high_priority_request_preserves_the_old_intent() {
        let Some((sim, rules)) = arena() else { return };
        let owner = sim.houses().keys().next().copied().expect("a house");
        let obs = Observation::new(&sim, owner);
        let mut tracker = Tracker::new();
        let mut out = Outbox::default();
        tracker.record(obs.tick(), 7, TrackedKind::Guard, None, None);
        let outside = (u16::MAX, u16::MAX);
        assert!(!issue_tracked(
            &mut tracker, None, &mut out, &rules, &obs, 7,
            TrackedKind::Dodge, None, Some(outside), Goal::Move { cell: outside },
        ));
        assert!(tracker.stands(obs.tick(), 7, &[TrackedKind::Guard]));
        assert!(!tracker.stands(obs.tick(), 7, &[TrackedKind::Dodge]));
        assert!(out.orders.is_empty());
    }

    /// Rejection can happen after validity checks too: if the frame already
    /// holds a stronger ask, a weaker ask must not overwrite the tracker.
    #[test]
    fn a_superseded_ask_does_not_leave_a_ghost_intent() {
        let Some((sim, rules)) = arena() else { return };
        let owner = sim.houses().keys().next().copied().expect("a house");
        let obs = Observation::new(&sim, owner);
        let mut tracker = Tracker::new();
        let mut out = Outbox::default();
        let emergency = (4, 4);
        assert!(out.want(
            7,
            super::super::super::tracked::rank(TrackedKind::Dodge),
            Goal::Move { cell: emergency },
        ));
        assert!(!issue_tracked(
            &mut tracker, None, &mut out, &rules, &obs, 7,
            TrackedKind::Guard, None, None, Goal::Guard,
        ));
        assert!(!tracker.is_claimed(obs.tick(), 7));
        out.resolve();
        assert_eq!(out.orders.len(), 1);
        assert_eq!(out.orders[0].goal, Goal::Move { cell: emergency });
    }
}
