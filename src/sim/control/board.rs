//! The control board: the one place a frame's requests become commands.
//!
//! Per frame, in this order:
//!
//! 1. the brain **thinks** against a fresh projection and answers orders and
//!    intents;
//! 2. what a factory has finished is taken out of it, so a finished build
//!    never blocks its own category;
//! 3. attached objects are stepped — garbage-collected, then the rotation for
//!    this frame — and each routine's answer becomes a goal;
//! 4. every order and intent is admitted: domain switch, per-frame budget, the
//!    acting object's ownership and the native order gate;
//! 5. what survived is stamped with the house and the current tick and queued
//!    through `Simulation::queue_commands`, the append-only ingress a player's
//!    clicks use.
//!
//! Step 4 is why this layer exists: the same admission applies to a retail
//! brain, a script and a managed routine, and a refusal is *counted and
//! reported*, never silent. A unit that stops is either waiting on a native
//! transition (`NotReady`) or has left the house's control (rejected), and the
//! counters say which.
//!
//! Nothing above this file may issue a command on its own — that is the whole
//! contract. The board writes no simulation state except through the owners it
//! calls, and owns no gameplay rule.

use std::collections::BTreeSet;

use crate::rules::ruleset::RuleSet;
use crate::sim::behavior::managed::{FallbackReason, MicroKind, MicroParams};
use crate::sim::behavior::order::{ApplyOutcome, Funnel};
use crate::sim::behavior::view::ThreatView;
use crate::sim::behavior::{Goal, MicroStep};
use crate::sim::command::{Command, CommandEnvelope};
use crate::sim::control::brain::{Brain, Outbox};
use crate::sim::control::budget::Budget;
use crate::sim::control::micro::MicroTable;
use crate::sim::control::observation::Observation;
use crate::sim::intern::InternedId;
use crate::sim::production::ProductionCategory;
use crate::sim::world::Simulation;

/// The categories whose product leaves a factory as a unit, and which
/// `Place_Production` therefore takes without a cell: everything a factory
/// makes that is not a building.
const MOBILE_CATEGORIES: [ProductionCategory; 4] = [
    ProductionCategory::Infantry,
    ProductionCategory::Vehicle,
    ProductionCategory::Aircraft,
    ProductionCategory::Ship,
];

/// Which part of the work the player lets auto play do.
///
/// Three switches and one master key: the base, the troops, and the abilities
/// (a paradrop, a spy plane, a missile). Each key flips its own switch, so a
/// player can keep the base growing while commanding the troops, hand the
/// troops over and take building back, or keep the superweapons for themselves
/// while everything else runs. Turning one off stops new commands of that kind;
/// it never undoes what was already issued or queued.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AutoPlayDomains {
    /// Production, building and placement: the base's own work.
    pub production: bool,
    /// Unit orders and managed routines: what the army does.
    pub forces: bool,
    /// Abilities: the superweapon launches, a paradrop and a spy plane among
    /// them. Their own switch because a player who wants the army but not the
    /// missile is not asking for half an army.
    pub abilities: bool,
}

impl AutoPlayDomains {
    pub const ALL: Self = Self {
        production: true,
        forces: true,
        abilities: true,
    };

    pub const NONE: Self = Self {
        production: false,
        forces: false,
        abilities: false,
    };

    /// Whether any switch is on: a host with none is a host driving nothing.
    pub const fn any(&self) -> bool {
        self.production || self.forces || self.abilities
    }

    /// Whether a command of this kind may leave the board now.
    pub fn admits(&self, command: &Command) -> bool {
        if belongs_to_production(command) {
            self.production
        } else if belongs_to_abilities(command) {
            self.abilities
        } else {
            self.forces
        }
    }

    /// The switches as a player-facing phrase, for the message line.
    pub fn describe(&self) -> String {
        let mut on: Vec<&str> = Vec::new();
        if self.production {
            on.push("build");
        }
        if self.forces {
            on.push("forces");
        }
        if self.abilities {
            on.push("abilities");
        }
        if on.is_empty() {
            return "paused".to_string();
        }
        on.join(" + ")
    }
}

/// Whether a command is an ability rather than the army's own work.
///
/// A superweapon launch is the house's whole hand out of a sidebar button —
/// the missile, the paradrop, the spy plane — and it is the one kind of order
/// a player may want to keep for themselves while the army runs.
fn belongs_to_abilities(command: &Command) -> bool {
    matches!(
        command,
        Command::LaunchSuperWeapon { .. }
            | Command::BeginPaidSuperWeapon { .. }
            | Command::CompletePaidSuperWeapon { .. }
            | Command::CancelPaidSuperWeapon { .. }
    )
}

/// Whether a command is the base's work rather than the troops'.
///
/// Production, placement and the commands that tidy a build up are the base's;
/// everything that moves, aims, fires, captures or launches is the forces'.
/// `DeployMcv` counts as the base's because it is how a base starts.
fn belongs_to_production(command: &Command) -> bool {
    matches!(
        command,
        Command::QueueProduction { .. }
            | Command::SuspendProduction { .. }
            | Command::CycleProducerFocus { .. }
            | Command::PlaceReadyBuilding { .. }
            | Command::PlaceProducedMobile { .. }
            | Command::CancelProductionByType { .. }
            | Command::SellBuilding { .. }
            | Command::ToggleRepair { .. }
            | Command::SetRally { .. }
            | Command::TogglePrimaryFactory { .. }
            | Command::DeployMcv { .. }
            | Command::UndeployBuilding { .. }
    )
}

/// Drives one brain and one house.
pub struct ControlBoard {
    brain: Box<dyn Brain>,
    owner: InternedId,
    domains: AutoPlayDomains,
    budget: Budget,
    micro: MicroTable,
    /// Objects the player has taken back. One of these is the player's to
    /// command: a control group is a promise that the unit will still be
    /// where the player put it, so this board orders it nothing at all and
    /// leaves any order it already had alone.
    excluded: BTreeSet<u64>,
    /// Commands the gate dropped, and orders the funnel called not-ready.
    /// Diagnostics: the queue is the authority, not these counts.
    rejected: usize,
    not_ready: usize,
}

impl ControlBoard {
    pub fn new(brain: Box<dyn Brain>, owner: InternedId, max_commands_per_frame: usize) -> Self {
        Self {
            brain,
            owner,
            domains: AutoPlayDomains::ALL,
            budget: Budget::new(max_commands_per_frame),
            excluded: BTreeSet::new(),
            micro: MicroTable::default(),
            rejected: 0,
            not_ready: 0,
        }
    }

    /// The house every issued command is stamped with. Not necessarily the
    /// local player's: a computer house can be driven the same way.
    pub fn owner(&self) -> InternedId {
        self.owner
    }

    /// Which halves of the work the board lets through now.
    pub fn domains(&self) -> AutoPlayDomains {
        self.domains
    }

    /// Flip the switches. A kind turned off from here on issues nothing more of
    /// its own; what is already queued in the simulation keeps running.
    pub fn set_domains(&mut self, domains: AutoPlayDomains) {
        self.domains = domains;
    }

    /// The objects the player has taken back, by stable id.
    pub fn excluded(&self) -> &BTreeSet<u64> {
        &self.excluded
    }

    /// The player takes these back: a control group is a promise that the unit
    /// stays where the player put it, so the board orders none of these and
    /// leaves any order it already had alone. The set is the player's whole
    /// current one, not an addition — a group released this frame is the
    /// board's to command again.
    pub fn take_back(&mut self, taken: BTreeSet<u64>) {
        self.excluded = taken;
    }

    /// The brain driving the house, for diagnostics.
    pub fn brain_name(&self) -> &'static str {
        self.brain.name()
    }

    pub fn rejected(&self) -> usize {
        self.rejected
    }

    pub fn not_ready(&self) -> usize {
        self.not_ready
    }

    pub fn managed(&self) -> &MicroTable {
        &self.micro
    }

    /// Put an object under a managed routine, with an optional post. `false`
    /// when it is already managed.
    pub fn attach(
        &mut self,
        entity: u64,
        kind: MicroKind,
        params: MicroParams,
        post: Option<(u16, u16)>,
    ) -> bool {
        self.micro.attach(entity, kind, params, post)
    }

    /// Return an object to the brain's direct orders.
    pub fn detach(&mut self, entity: u64) -> bool {
        self.micro.detach(entity).is_some()
    }

    /// One frame of auto play. Call it before the frame advances.
    ///
    /// Returns the commands it queued, in issue order. They are the client's
    /// only view of what this hand did: a player's press writes its cameo's
    /// `request1` and predicted clock as it clicks
    /// (`Select6AB0C9..6AB94D`), so the client mirrors the same presentation
    /// writes for these orders. The queue stays the authority for the frame.
    pub fn step(&mut self, sim: &mut Simulation, rules: &RuleSet) -> Vec<Command> {
        if sim.session().input_locked() {
            return Vec::new();
        }
        let owner = self.owner;
        if !sim.houses().contains_key(&owner) {
            return Vec::new();
        }
        self.budget.begin_frame();
        let owner_name = sim.interner().resolve(owner).to_owned();

        let mut pending: Vec<Command> = Vec::new();

        // 1. Decisions.
        let mut out = Outbox::default();
        {
            let obs = Observation::new(sim, owner).with_player_groups(&self.excluded);
            self.brain.think(rules, &obs, &mut out);
        }
        for intent in out.intents {
            let command = intent.command();
            self.admit(sim, &owner_name, &command, &mut pending);
        }
        // Objects the brain hands to a managed routine. Consumed before the
        // orders below, because a routine is its object's only issuer: the
        // order the brain also asked for this frame is dropped by the
        // `is_managed` guard rather than issued and then overwritten.
        for request in out.attachments {
            if self.excluded.contains(&request.actor) {
                log::debug!(
                    "auto play: refused, the player owns {} (attach {})",
                    request.actor,
                    request.kind.name()
                );
                self.rejected += 1;
                continue;
            }
            if self
                .micro
                .attach(request.actor, request.kind, request.params, request.post)
            {
                log::debug!(
                    "auto play: {} attached to {}",
                    request.actor,
                    request.kind.name()
                );
            }
        }
        for command in &out.commands {
            self.admit(sim, &owner_name, command, &mut pending);
        }
        // The orders that never left. A brain keeps its own record of what it
        // has told each object, and an order dropped here — by the per-frame
        // budget, by a routine that owns the object, by the gate — leaves no
        // trace in the simulation for that record to contradict, so the brain
        // is told and forgets it. Without this a budget of eight against an
        // army of a hundred is a hundred units each told once, refused, and
        // held to it for the whole of the intent's term.
        let mut refused: Vec<(u64, Goal)> = Vec::new();
        for order in &out.orders {
            if self.micro.is_managed(order.actor) {
                refused.push((order.actor, order.goal));
                continue;
            }
            let outcome = self.issue(
                sim,
                rules,
                &owner_name,
                order.actor,
                order.goal,
                &mut pending,
            );
            if !outcome.is_queued() {
                refused.push((order.actor, order.goal));
            }
        }
        if !refused.is_empty() {
            self.brain.orders_refused(&refused);
        }

        // 2. Delivery: take what the factories have finished out of them. A
        // factory keeps its product until the house takes it, so a build that
        // is never taken leaves the category busy for good.
        for category in MOBILE_CATEGORIES {
            let finished = Observation::new(sim, owner)
                .finished_product(category)
                .is_some();
            if finished {
                self.admit(
                    sim,
                    &owner_name,
                    &Command::PlaceProducedMobile { category },
                    &mut pending,
                );
            }
        }

        // 3. Managed objects, when the forces half is on.
        if self.domains.forces {
            self.step_managed(sim, rules, &owner_name, &mut pending);
        }

        // 4. One ingress, stamped for this frame.
        let execute_tick = sim.session().tick();
        // One line every ten seconds in the log the client already writes:
        // "the brain does nothing" and "the brain is refused" look the same
        // from the screen, and only these four counts tell them apart.
        if execute_tick % 600 == 0 {
            log::info!(
                "auto play: tick={execute_tick} owner={owner_name} production={} forces={} queued={} rejected={} not_ready={} excluded={}",
                self.domains.production,
                self.domains.forces,
                pending.len(),
                self.rejected,
                self.not_ready,
                self.excluded.len()
            );
        }
        sim.queue_commands(
            pending
                .iter()
                .cloned()
                .map(|command| CommandEnvelope::new(owner, execute_tick, command)),
        );
        pending
    }

    /// Step this frame's rotation of attached objects and issue what they ask.
    fn step_managed(
        &mut self,
        sim: &mut Simulation,
        rules: &RuleSet,
        owner_name: &str,
        pending: &mut Vec<Command>,
    ) {
        let owner = self.owner;
        for (entity, reason) in self.micro.retain_live(sim, owner) {
            log::info!(
                "retail-ai: {} handed back to the brain ({})",
                entity,
                reason.describe()
            );
        }
        if self.micro.is_empty() {
            return;
        }
        let ids = self
            .micro
            .rotation(self.budget.rotation_start(), self.budget.micro_steps());
        if ids.is_empty() {
            return;
        }

        let mut steps: Vec<(u64, MicroStep)> = Vec::new();
        {
            let threats = ThreatView::new(sim, owner);
            for &entity in &ids {
                if let Some(step) = self.micro.step_one(entity, sim, rules, &threats) {
                    steps.push((entity, step));
                }
            }
        }
        if let Some(&last) = ids.last() {
            self.budget.set_rotation_start(last.wrapping_add(1));
        }

        for (entity, step) in steps {
            match step {
                MicroStep::Wait { .. } => {}
                MicroStep::Done => {
                    self.micro.detach(entity);
                }
                MicroStep::Fallback(reason) => {
                    self.fall_back(entity, reason);
                }
                MicroStep::Goal(goal) => {
                    let outcome = self.issue(sim, rules, owner_name, entity, goal, pending);
                    if let Some(reason) = self.micro.record_outcome(entity, outcome) {
                        self.fall_back(entity, reason);
                    }
                }
            }
        }
    }

    fn fall_back(&mut self, entity: u64, reason: FallbackReason) {
        self.micro.detach(entity);
        log::info!(
            "retail-ai: micro routine on {} handed back to the brain ({})",
            entity,
            reason.describe()
        );
    }

    /// Turn one goal into a command and admit it.
    fn issue(
        &mut self,
        sim: &Simulation,
        rules: &RuleSet,
        owner_name: &str,
        actor: u64,
        goal: Goal,
        pending: &mut Vec<Command>,
    ) -> ApplyOutcome {
        // Two orders are not sent: the one the object already stands on, and a
        // march that would delete the gun order it is working.
        //
        // `Set_Destination` re-runs the path, so a destination re-sent every
        // frame is a unit that restarts its walk before it covers a cell —
        // drift, and a command stream the engine has no use for. And
        // `FootClass::Assign_Target_Command` (`0x004DF0E0`) runs
        // `Clear_All_TarCom` for every mission but AttackMove, so a march does
        // not pause an attack: it deletes the target and the unit walks off
        // unarmed. A one-step reposition still preempts — the dodge and kite
        // passes move a unit out from under a shell — so only a destination
        // more than a step away is refused.
        if let Some(standing) = order_stands(sim, rules, self.owner, actor, &goal) {
            log::trace!("auto play: {actor} already {standing}, {goal:?} dropped");
            return ApplyOutcome::Queued;
        }
        let funnel = Funnel::new(sim, rules, self.owner);
        match funnel.prepare(actor, goal) {
            Ok(command) => {
                if self.admit(sim, owner_name, &command, pending) {
                    ApplyOutcome::Queued
                } else {
                    ApplyOutcome::Rejected
                }
            }
            Err(refusal) => {
                let outcome = refusal.outcome();
                if outcome == ApplyOutcome::NotReady {
                    self.not_ready += 1;
                } else {
                    self.rejected += 1;
                }
                outcome
            }
        }
    }

    /// The gate: budget, domain switch, ownership and the native order gate.
    fn admit(
        &mut self,
        sim: &Simulation,
        owner_name: &str,
        command: &Command,
        pending: &mut Vec<Command>,
    ) -> bool {
        if !self.domains.admits(command) || !admits(sim, owner_name, command) {
            log::debug!("auto play: refused by the gate {command:?}");
            self.rejected += 1;
            return false;
        }
        // The player's own objects: a unit in a control group stays under the
        // player's hand, whatever half of the work is switched on.
        if actor_of(command).is_some_and(|actor| self.excluded.contains(&actor)) {
            log::debug!("auto play: refused, the player owns {command:?}");
            self.rejected += 1;
            return false;
        }
        if !self.budget.try_command() {
            log::debug!("auto play: refused, the frame budget is spent {command:?}");
            self.rejected += 1;
            return false;
        }
        pending.push(command.clone());
        true
    }
}

/// Whether the house may issue this command: the acting object, when the
/// command names exactly one, must be the house's and must admit orders.
///
/// Both predicates are the production ones, reused rather than restated:
/// `world_commands.rs` documents that the MegaMission receiver resolves its
/// actor independently of the event house, so without the ownership half an
/// agent could order an object it does not own.
fn admits(sim: &Simulation, owner: &str, command: &Command) -> bool {
    match actor_of(command) {
        None => true,
        Some(entity_id) => {
            sim.entity_owned_by_id(owner, entity_id) && sim.order_actor_admits(entity_id)
        }
    }
}

/// How far from where a unit stands a movement order may name and still count
/// as a reposition rather than a march, in cells. A dodge or a kite step moves
/// a unit out from under a shell; a march sends it somewhere else entirely.
const REPOSITION_CELLS: u16 = 2;

/// When `goal` is already what `actor` is doing — or a march that would delete
/// the fire order it is working — the reason, for the log; `None` otherwise.
fn order_stands(
    sim: &Simulation,
    rules: &RuleSet,
    owner: InternedId,
    actor: u64,
    goal: &Goal,
) -> Option<&'static str> {
    let obs = Observation::new(sim, owner);
    match *goal {
        Goal::Attack { target } => {
            // A target this house may no longer shoot is not an order standing:
            // the object changed hands under the gun, and letting the answer
            // stay `on that target` is how the army keeps firing at its own
            // captured building until the last hit point is gone. Asking
            // `is_hostile` per frame is the whole point — the order has to die
            // with the enmity it was issued under.
            if !obs.is_hostile(target) {
                return None;
            }
            (obs.attack_target(actor) == Some(target)).then_some("on that target")
        }
        Goal::Move { cell } | Goal::AttackMove { cell } => {
            if obs.destination(actor) == Some(cell) {
                return Some("walking there");
            }
            let entity = sim.entities().get(actor)?;
            let from = (entity.position.rx(), entity.position.ry());
            let steps = from.0.abs_diff(cell.0).max(from.1.abs_diff(cell.1));
            let live_gun = obs.attack_target(actor).is_some_and(|target| {
                obs.is_hostile(target) && obs.engage_error(rules, actor, target).is_none()
            });
            (live_gun && steps > REPOSITION_CELLS).then_some("firing on a target")
        }
        _ => None,
    }
}

/// The object a command acts on, when it names exactly one.
fn actor_of(command: &Command) -> Option<u64> {
    Some(match command {
        Command::Move { entity_id, .. }
        | Command::MoveToObject { entity_id, .. }
        | Command::Stop { entity_id }
        | Command::AttackMove { entity_id, .. }
        | Command::Guard { entity_id, .. }
        | Command::DeployMcv { entity_id }
        | Command::UndeployBuilding { entity_id }
        | Command::SellBuilding { entity_id }
        | Command::ToggleRepair { entity_id }
        | Command::UnloadPassengers {
            transport_id: entity_id,
        }
        | Command::ToggleInfantryDeploy { entity_id }
        | Command::Scatter { entity_id }
        | Command::TogglePrimaryFactory { entity_id }
        | Command::MegaMission { entity_id, .. }
        | Command::SetBuildingOnLine { entity_id, .. }
        | Command::ArchiveObject { entity_id, .. } => *entity_id,
        Command::Attack { attacker_id, .. }
        | Command::ForceAttackCell { attacker_id, .. }
        | Command::PlantC4 { attacker_id, .. } => *attacker_id,
        Command::EjectBunker { bunker_id } => *bunker_id,
        Command::CaptureBuilding { engineer_id, .. } => *engineer_id,
        // House-scoped commands: production, sale of a wall cell, superweapon
        // targeting, speed, exit, diplomacy and the rest. Admission belongs to
        // their receivers.
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn arena() -> Option<(Simulation, RuleSet)> {
        let rules = crate::rules::retail_ini_fixture::retail_unlayered_rules()?;
        let mut sim = crate::sim::arena_fixture::rival_arena(&rules, 0xA17);
        sim.intern_rule_type_ids(&rules);
        sim.resolve_type_handles(&rules);
        Some((sim, rules))
    }

    /// The three switches own three kinds of order, and the abilities switch
    /// owns the superweapon launches alone: a player who takes the missile
    /// back keeps the army, and a player who takes the army back keeps the
    /// missile.
    #[test]
    fn the_switches_own_their_own_kinds_of_order() {
        let launch = Command::LaunchSuperWeapon {
            sw_type_id: crate::sim::intern::InternedId::from_index(0),
            target_rx: 10,
            target_ry: 10,
        };
        let march = Command::Move {
            entity_id: 1,
            target_rx: 10,
            target_ry: 10,
        };
        let queue = Command::QueueProduction {
            type_id: crate::sim::intern::InternedId::from_index(0),
        };

        let abilities_only = AutoPlayDomains {
            production: false,
            forces: false,
            abilities: true,
        };
        assert!(
            abilities_only.admits(&launch),
            "the missile is the abilities'"
        );
        assert!(!abilities_only.admits(&march), "and not the army's");
        assert!(!abilities_only.admits(&queue), "and not the base's");

        let forces_only = AutoPlayDomains {
            production: false,
            forces: true,
            abilities: false,
        };
        assert!(forces_only.admits(&march));
        assert!(
            !forces_only.admits(&launch),
            "the missile stays with the player"
        );

        let build_only = AutoPlayDomains {
            production: true,
            forces: false,
            abilities: false,
        };
        assert!(build_only.admits(&queue));
        assert!(!build_only.admits(&march));
        assert!(!build_only.admits(&launch));

        assert!(AutoPlayDomains::ALL.admits(&launch));
        assert!(
            !AutoPlayDomains::NONE.any(),
            "nothing on is a host driving nothing"
        );
        assert_eq!(
            AutoPlayDomains::ALL.describe(),
            "build + forces + abilities"
        );
        assert_eq!(abilities_only.describe(), "abilities");
        assert_eq!(AutoPlayDomains::NONE.describe(), "paused");
    }
    /// A march does not replace a gun that is firing: `Assign_Target_Command`
    /// clears the target on any mission but AttackMove, so the Move does not
    /// pause the attack — it deletes it. A one-step reposition still preempts,
    /// which is what the dodge and kite passes move a unit with.
    #[test]
    fn a_march_does_not_replace_a_live_fire_order() {
        let Some((mut sim, rules)) = arena() else {
            return;
        };
        let owner = sim.interner.intern("Americans");
        let gi = sim
            .spawn_object("E1", "Americans", 20, 20, 255, &rules)
            .expect("a GI");
        let target = sim
            .spawn_object("E2", "Russians", 21, 20, 255, &rules)
            .expect("a conscript");
        sim.queue_commands([CommandEnvelope::new(
            owner,
            sim.session().tick(),
            Command::Attack {
                attacker_id: gi,
                target_id: target,
            },
        )]);
        sim.advance_tick(&[], Some(&rules), None, None, 33);
        let obs = Observation::new(&sim, owner);
        assert_eq!(obs.attack_target(gi), Some(target), "the target is held");
        assert_eq!(
            order_stands(&sim, &rules, owner, gi, &Goal::Move { cell: (40, 40) }),
            Some("firing on a target")
        );
        assert_eq!(
            order_stands(&sim, &rules, owner, gi, &Goal::Move { cell: (21, 21) }),
            None,
            "a step out from under a shell is still a reposition"
        );
        assert_eq!(
            order_stands(&sim, &rules, owner, gi, &Goal::Attack { target }),
            Some("on that target"),
            "an attack it already holds is not re-sent"
        );
    }

    /// A destination the unit already holds is not re-sent: the path is not
    /// re-run, which is what drift looked like from the outside.
    #[test]
    fn a_destination_already_held_is_not_re_sent() {
        let Some((mut sim, rules)) = arena() else {
            return;
        };
        let owner = sim.interner.intern("Americans");
        let gi = sim
            .spawn_object("E1", "Americans", 20, 20, 255, &rules)
            .expect("a GI");
        sim.queue_commands([CommandEnvelope::new(
            owner,
            sim.session().tick(),
            Command::Move {
                entity_id: gi,
                target_rx: 26,
                target_ry: 20,
            },
        )]);
        sim.advance_tick(&[], Some(&rules), None, None, 2);
        let obs = Observation::new(&sim, owner);
        assert_eq!(obs.destination(gi), Some((26, 20)), "it is walking there");
        assert_eq!(
            order_stands(&sim, &rules, owner, gi, &Goal::Move { cell: (26, 20) }),
            Some("walking there")
        );
        assert_eq!(
            order_stands(&sim, &rules, owner, gi, &Goal::Move { cell: (26, 30) }),
            None,
            "a different destination is a new order"
        );
    }

    /// An attack order dies with the enmity it was issued under.
    ///
    /// A capture, an engineer or a mind control turns the thing being shot at
    /// into this house's own building. The gunner never re-reads it, so the
    /// old answer — "already on that target" — kept the order standing and the
    /// army kept firing at its own prize until the last hit point.
    ///
    /// Measured on Arena.mmx seed 0x5EED at 24000 ticks with the death ledger
    /// on: 26 of 41 local losses were under `Attack`, and the roster showed
    /// `CABUNK01:2` — two neutral bunkers the house had taken and was then
    /// shooting at.
    #[test]
    fn an_attack_on_a_building_that_changed_hands_does_not_stand() {
        let Some((mut sim, rules)) = arena() else {
            return;
        };
        let owner = sim.interner.intern("Americans");
        let russian = sim.interner.intern("Russians");
        let tank = sim
            .spawn_object("MTNK", "Americans", 20, 20, 0, &rules)
            .expect("a grizzly");
        let prize = sim
            .spawn_object("NAREFN", "Russians", 22, 20, 0, &rules)
            .expect("an enemy refinery");
        sim.queue_commands([CommandEnvelope::new(
            owner,
            sim.session().tick(),
            Command::Attack {
                attacker_id: tank,
                target_id: prize,
            },
        )]);
        sim.advance_tick(&[], Some(&rules), None, None, 2);
        let obs = Observation::new(&sim, owner);
        assert_eq!(obs.attack_target(tank), Some(prize), "it is shooting at it");
        assert!(
            order_stands(&sim, &rules, owner, tank, &Goal::Attack { target: prize }).is_some(),
            "while the refinery is the enemy's, the order stands"
        );

        // The building changes hands: same object, same gun, different side.
        sim.change_owner_with_rules(
            prize,
            owner,
            &rules,
            None,
            crate::sim::world::FrameEffects::empty(),
        );
        let obs = Observation::new(&sim, owner);
        assert!(!obs.is_hostile(prize), "it is the house's own now");
        assert_eq!(
            order_stands(&sim, &rules, owner, tank, &Goal::Attack { target: prize }),
            None,
            "so the order is not lived out and has to be replaced"
        );
        let _ = russian;
    }
}
