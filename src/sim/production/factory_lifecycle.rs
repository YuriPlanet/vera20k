//! Complete mutations of factory-held Techno identity and its accounting.
//!
//! The registry owns queue/charge kernels. This world-facing owner completes
//! their constructor, disposal and successor work before an operation returns.
//! Native StartProduction/AbandonProduction/StartNextQueued:
//! 0x004C9C70 / 0x004C9FF0 / 0x004CA5A0.
//! Delivery selection and placement keep their existing phase positions and
//! call settlement only after their successful world effects have committed.

use super::CancelOutcome;
use super::can_build::{check_build_limit, find_factory};
use super::factory::{
    AbandonedObject, EnqueueOutcome, FactoryHolder, time_to_build, time_to_build_inputs,
};
use super::production_tech::{production_category_for_object, strip_keeps};
use super::production_types::ProductionCategory;
use crate::rules::object_type::ObjectCategory;
use crate::rules::ruleset::RuleSet;
use crate::sim::ai_unit_choice::UnitChoiceKind;
use crate::sim::intern::InternedId;
use crate::sim::world::FrameEffects;
use crate::sim::world::{SimSoundEvent, Simulation};

/// An admitted HouseAbandon4FAA10 keeps this registry identity through
/// settlement/removal. This temporary value owns no Factory state. Native
/// Sidebar6ABAD0 can settle the product before the House cursor prefix;
/// direct FactoryAbandon/revalidation does not enter these local boundaries.
/// Source: tools/sidebar_oracle/cameo_order.md, executed linked/unlinked order.
struct HouseFactoryAbandonment {
    owner: InternedId,
    category: ProductionCategory,
    construction_order: u64,
}

impl HouseFactoryAbandonment {
    fn capture(sim: &Simulation, owner: InternedId, category: ProductionCategory) -> Option<Self> {
        let factory = sim.production.factories.view(owner, category)?;
        Some(Self {
            owner,
            category,
            construction_order: factory.construction_order(),
        })
    }

    fn begin(&self, sim: &mut Simulation) {
        if sim.session.current_house == Some(self.owner) {
            sim.lifecycle_outputs.push(
                crate::sim::world::LifecycleOutput::ProductionFactoryAbandoning {
                    owner: self.owner,
                    category: self.category,
                    construction_order: self.construction_order,
                },
            );
        }
    }

    fn finish(self, sim: &mut Simulation) {
        if sim.session.current_house == Some(self.owner) {
            sim.lifecycle_outputs.push(
                crate::sim::world::LifecycleOutput::ProductionFactoryUnlinked {
                    owner: self.owner,
                    category: self.category,
                    construction_order: self.construction_order,
                },
            );
        }
    }
}

/// The PRODUCE event for `type_id`: `HouseClass::Begin_Production @ 0x004FA350`.
///
/// The build needs an online factory that may build it, `FindFactory(0,1,1)`
/// (`0x004FA438`), whose `CanBuild(type, 1, 1)` asks only the build limit and
/// lets a type at it through while one of the house's factories holds it
/// (`0x004F8348`); a PRODUCE event asks no second time (`0x004C7130` passes
/// 0). Nothing here asks the house's prerequisites: the sidebar offers only
/// what the strip keeps, and the frame's [`revalidate_and_step_factories`]
/// abandons what it no longer keeps. A stopped active object of the same
/// type takes the same-type branch (`0x004FA5A8..0x004FA5C4`): a held build
/// resumes, and a finished one stays (the build start refuses stage 54,
/// `0x004C9ECD`). Otherwise `FactoryClass::StartProduction @ 0x004C9C70`
/// starts or queues the build. Nothing on this path checks money: a short
/// wallet stalls the steps instead. A queue already holding `[General]
/// MaximumQueuedObjects=` builds, or a type at its limit
/// (`HouseClass::CheckBuildLimit`, `0x004C9CEA`), refuses the append and
/// scolds the local player.
///
/// Residual (building path): a building never queues in gamemd.
/// Begin_Production refuses a building PRODUCE while the building factory runs
/// (`0x004FA52E..0x004FA550`), and StartProduction abandons a held or finished
/// building before starting a different one (`0x004C9C87..0x004C9C98`); VERA
/// appends it. Trigger: a second building PRODUCE before the first executes
/// (the sidebar refuses a busy structure strip, and VERA's AI asks only while
/// no building is queued). Effect: VERA builds both in turn. Rare.
pub(crate) fn enqueue_by_type(
    sim: &mut Simulation,
    rules: &RuleSet,
    owner: &str,
    type_id: &str,
    effects: FrameEffects<'_>,
) -> bool {
    let Some(obj) = rules.object(type_id) else {
        return false;
    };
    let owner_id = sim.interner.intern(owner);
    if find_factory(sim, rules, owner_id, obj, false, true, true).is_none() {
        return false;
    }
    let category = production_category_for_object(obj);
    let type_interned = sim.interner.intern(type_id);
    if let Some((active, held, finished)) =
        sim.production.factories.active_object(owner_id, category)
        && active == type_interned
        && (held || finished)
    {
        if !held {
            return false;
        }
        let time_to_build = time_to_build(&time_to_build_inputs(sim, rules, owner_id, obj));
        let frame = sim.session.binary_frame;
        sim.production
            .factories
            .resume(owner_id, category, time_to_build, frame);
        publish_cameo_factory_link(sim, owner_id, category, type_interned);
        return true;
    }
    let enqueue_order = sim.production.next_enqueue_order;
    let cost = sim.cost_of(owner_id, obj, rules);
    let at_build_limit = check_build_limit(sim, rules, owner_id, obj);
    let outcome = sim.production.factories.enqueue(
        owner_id,
        category,
        type_interned,
        enqueue_order,
        cost,
        rules.general.maximum_queued_objects,
        at_build_limit,
    );
    if outcome == EnqueueOutcome::AppendRefused {
        sim.sound_events
            .push(SimSoundEvent::ProductionRefused { owner: owner_id });
        return false;
    }
    sim.production.next_enqueue_order = enqueue_order.saturating_add(1);
    if outcome == EnqueueOutcome::Started {
        start_active_production(
            sim,
            rules,
            FactoryHolder::House(owner_id, category),
            type_interned,
            effects,
        )
        .expect("validated StartProduction type must construct one Techno");
    }
    true
}

/// The SUSPEND event: `HouseClass::Suspend_Production @ 0x004FA910` puts the
/// category's running build on hold.
pub fn suspend_production(sim: &mut Simulation, owner: &str, category: ProductionCategory) -> bool {
    let Some(owner_id) = sim.interner.get(owner) else {
        return false;
    };
    let frame = sim.session.binary_frame;
    sim.production.factories.suspend(owner_id, category, frame)
}

/// Start the build an active factory head holds. `HouseClass::Begin_Production
/// @ 0x004FA350` runs `FactoryClass::StartProduction @ 0x004C9C70`, which calls
/// `type->CreateInstance(owner)` and stores the result at `Factory+0x58`, then
/// the build start `0x004C9EA0` (Ghidra label `FactoryClass__SetRate`) at
/// `0x004FA628`, which arms the step rate and timer from that object's
/// `Time_To_Build`. Queued tail entries start only when
/// `FactoryClass::StartNextQueued @ 0x004CA5A0` promotes them, which runs the
/// same Begin_Production (`0x004CA60A`).
///
/// Begin_Production's network headstart (`0x004FA631..0x004FA68F`) never runs:
/// it needs the factory unsuspended just before the start (read at
/// `0x004FA622`), and every path there has just suspended it (StartProduction's
/// create path, `0x004C9D72`) or resumed a suspended one (`0x004FA5A8..0x004FA5C6`).
/// A queue append (`0x004C9D22..0x004C9D2E`) returns at `0x004FA612` first, and
/// a promotion's StartProduction never appends (`0x004C9CC9..0x004C9CCF`).
///
/// A computer's building factory runs the same StartProduction and build
/// start (`BuildingClass::Factory_AI`, `0x0045039A` and `0x004503C5`).
/// StartProduction also marks a computer house's building
/// (`+0x6CA = 1`, `0x004C9DC5`), the `[Structures]` AI Rebuildable byte
/// (`0x0044FB61`), which only the map writer and the building's CRC read;
/// VERA does not keep it.
pub(super) fn start_active_production(
    sim: &mut Simulation,
    rules: &RuleSet,
    holder: FactoryHolder,
    type_id: InternedId,
    effects: FrameEffects<'_>,
) -> Option<u64> {
    let owner_id = sim
        .production
        .factories
        .factory(holder)
        .map(|factory| factory.owner)?;
    let obj = sim.object_type(type_id, rules)?;
    let owner = sim.interner.resolve(owner_id).to_string();
    let type_name = sim.interner.resolve(type_id).to_string();
    // A factory-held object is still in limbo and has no cell authority. Zero
    // is only inert storage here; the result-bearing Unlimbo later installs the
    // selected delivery/placement coordinate on this same stable identity.
    let stable_id = sim.construct_object_limbo_at_height(&type_name, &owner, 0, 0, 0, 0, rules)?;
    let linked = sim
        .production
        .factories
        .link_active_entity(holder, stable_id);
    if linked != Some(stable_id) {
        let _ = sim.discard_constructed_limbo(stable_id, Some(rules), effects);
        return None;
    }
    let time_to_build = time_to_build(&time_to_build_inputs(sim, rules, owner_id, obj));
    let frame = sim.session.binary_frame;
    sim.production
        .factories
        .start_rate(holder, time_to_build, frame);
    if let FactoryHolder::House(owner, category) = holder {
        publish_cameo_factory_link(sim, owner, category, type_id);
    }
    Some(stable_id)
}

/// Transport the original reached local4FA6AA link to its app-owned strip.
/// Queued/refused StartProduction never constructs here; same-type resume
/// explicitly publishes the same link. Registry identity stays authoritative.
fn publish_cameo_factory_link(
    sim: &mut Simulation,
    owner: InternedId,
    category: ProductionCategory,
    type_id: InternedId,
) {
    if sim.session.current_house != Some(owner) {
        return;
    }
    if let Some(factory) = sim.production.factories.view(owner, category) {
        sim.lifecycle_outputs.push(
            crate::sim::world::LifecycleOutput::ProductionFactoryLinked {
                owner,
                type_id,
                category,
                construction_order: factory.construction_order(),
            },
        );
    }
}

/// Finish `FactoryClass::AbandonProduction @ 0x004C9FF0` for an object the registry
/// let go: refund it ([`refund_abandoned`]); a house no human controls then
/// forgets its choice of that kind (`0x004CA082..0x004CA0DD`: the building
/// choice of `sim::ai_base_building`, or a choice of `sim::ai_unit_choice`);
/// then the held limbo object is destroyed without rewinding RNG
/// (`0x004CA0E0`).
pub(super) fn settle_abandoned(
    sim: &mut Simulation,
    rules: &RuleSet,
    owner_id: InternedId,
    abandoned: AbandonedObject,
    effects: FrameEffects<'_>,
) {
    let category = sim
        .object_type(abandoned.type_id, rules)
        .map(|object| object.category);
    refund_abandoned(sim, rules, owner_id, abandoned.type_id, abandoned.balance);
    let game_mode_nonzero = sim.session.game_mode_nonzero;
    if let Some(category) = category
        && let Some(house) = sim.houses.get_mut(&owner_id)
        && !house.is_controlled_by_human(game_mode_nonzero)
    {
        match UnitChoiceKind::of(category) {
            Some(kind) => house.ai_unit_choices.clear(kind),
            None => house.ai_production.clear_building_choice(),
        }
    }
    if let Some(entity_id) = abandoned.entity_id {
        // Original4CA0E3..4CA109 brackets the scalar destructor, including
        // Building43BD67 pointer expiry. Its listener timers cannot draw under
        // A8E7AC; preserve the caller's bracket through the shared destructor.
        let discarded = sim.with_object_placement_scope(|sim| {
            sim.discard_constructed_limbo(entity_id, Some(rules), effects)
        });
        debug_assert!(
            discarded,
            "AbandonProduction destroys the held limbo object"
        );
    }
}

/// `HouseClass::AI_Manage_Build_Queue`'s factory loop (`0x004FE04F..
/// 0x004FE098`): from the last factory in FactoryClass::Array to the first,
/// each that house `owner` owns (`+0x6C`) is held (`Suspend(1) @
/// 0x004C9E60`, whose writes the delete drops), abandoned and deleted
/// ([`abandon_production`]; the destructor's own AbandonProduction finds no
/// object left).
pub(crate) fn abandon_house_factories(
    sim: &mut Simulation,
    rules: &RuleSet,
    owner: InternedId,
    effects: FrameEffects<'_>,
) {
    for holder in sim.production.factories.owner_holders_last_first(owner) {
        abandon_production(sim, rules, holder, owner, effects);
    }
}

/// `FactoryClass::AbandonProduction @ 0x004C9FF0` and the delete (`vt+0x20`)
/// of `holder`'s factory, whose house (`+0x6C`) is `owner`: the object it
/// holds is settled ([`settle_abandoned`]).
pub(super) fn abandon_production(
    sim: &mut Simulation,
    rules: &RuleSet,
    holder: FactoryHolder,
    owner: InternedId,
    effects: FrameEffects<'_>,
) {
    if let Some(abandoned) = sim.production.factories.abandon_factory(holder) {
        settle_abandoned(sim, rules, owner, abandoned, effects);
    }
}

/// The AbandonProduction refund: the object's Cost_Of for its owner now, less the
/// Balance it still owed (`0x004CA029..0x004CA043`), through `HouseClass::Add_Credits
/// @ 0x004F9950` (`0x004CA046`). A FactoryPlant gained or lost during the build
/// changes that Cost_Of, so the refund can differ from what was paid, negative
/// included.
fn refund_abandoned(
    sim: &mut Simulation,
    rules: &RuleSet,
    owner_id: InternedId,
    type_id: InternedId,
    balance: i32,
) {
    let Some(object) = sim.object_type(type_id, rules) else {
        return;
    };
    let refund = sim.cost_of(owner_id, object, rules).wrapping_sub(balance);
    crate::sim::credit_income::add_credits(sim, owner_id, refund);
}

/// The ABANDON / ABANDON_ALL events for `type_id` (a right click, with Shift
/// for all): `HouseClass::Abandon_Production @ 0x004FAA10` through the
/// registry's `cancel_one`. An abandoned active object is refunded and
/// destroyed ([`settle_abandoned`]), a finished building stops waiting for
/// placement, and the next queued build starts (`0x004FAC96`).
pub(crate) fn cancel_by_type_for_owner(
    sim: &mut Simulation,
    rules: &RuleSet,
    owner: &str,
    type_id: &str,
    all: bool,
    effects: FrameEffects<'_>,
) -> bool {
    let Some(obj) = rules.object(type_id) else {
        return false;
    };
    let category = production_category_for_object(obj);
    let owner_id = sim.interner.intern(owner);
    let type_interned = sim.interner.intern(type_id);
    // cancel_one may release an empty Factory. Capture its authoritative
    // identity before that mutation; a queued-only outcome never uses it.
    let abandonment = HouseFactoryAbandonment::capture(sim, owner_id, category);
    let outcome = sim.production.factories.cancel_one(
        owner_id,
        category,
        type_interned,
        all,
        obj.build_cat
            .filter(|_| obj.category == ObjectCategory::Building),
    );
    match outcome {
        CancelOutcome::NoMatch => return false,
        CancelOutcome::QueuedRemoved => {}
        CancelOutcome::ReleasedEmpty => {
            let abandonment = abandonment.expect("admitted House abandon has a Factory");
            abandonment.begin(sim);
            abandonment.finish(sim);
        }
        CancelOutcome::AbandonedActive(object) => {
            let abandonment = abandonment.expect("admitted House abandon has a Factory");
            abandonment.begin(sim);
            settle_abandoned(sim, rules, owner_id, object, effects);
            abandonment.finish(sim);
            advance_after_delivery(sim, rules, owner_id, category, effects);
        }
    }
    true
}

/// C7 StartNextQueued once the active object is gone: after a successful delivery, an
/// abandon or a completed-but-undeliverable refund. Clear the active object and promote
/// the next queued entry into the active slot, cost-seeded from `rules` and started at
/// this frame.
///
/// Human mobile release occurs only in the queued PLACE tail
/// (Event4C710B -> House4FB0E0 -> Abandon4FB663 -> StartNext4FAC96),
/// after Strip has left the completed object's identity held for one frame.
fn advance_after_delivery(
    sim: &mut Simulation,
    rules: &RuleSet,
    owner_id: InternedId,
    category: ProductionCategory,
    effects: FrameEffects<'_>,
) {
    let next_cost = sim
        .production
        .factories
        .peek_next_queued(owner_id, category)
        .and_then(|t| sim.object_type(t, rules))
        .map_or(0, |object| sim.cost_of(owner_id, object, rules));
    let promoted = sim
        .production
        .factories
        .clear_active_and_advance(owner_id, category, next_cost);
    if let Some(type_id) = promoted {
        start_active_production(
            sim,
            rules,
            FactoryHolder::House(owner_id, category),
            type_id,
            effects,
        )
        .expect("validated promoted production type must construct one Techno");
    }
}
pub(super) fn active_entity_id(
    sim: &Simulation,
    owner_id: InternedId,
    category: ProductionCategory,
) -> Option<u64> {
    sim.production
        .factories
        .view(owner_id, category)
        .and_then(|view| view.object.and_then(|object| object.entity_id))
        .filter(|&stable_id| sim.substrate.entities.contains(stable_id))
}
fn discard_active_factory_entity(
    sim: &mut Simulation,
    rules: &RuleSet,
    owner_id: InternedId,
    category: ProductionCategory,
    effects: FrameEffects<'_>,
) {
    if let Some(stable_id) = active_entity_id(sim, owner_id, category) {
        let discarded = sim.with_object_placement_scope(|sim| {
            sim.discard_constructed_limbo(stable_id, Some(rules), effects)
        });
        debug_assert!(
            discarded,
            "factory-held object must remain in limbo until delivery"
        );
    }
}
/// Publish the completion edge once, without releasing the held object.
pub(super) fn publish_completion(
    sim: &mut Simulation,
    rules: &RuleSet,
    owner: InternedId,
    category: ProductionCategory,
) {
    let Some(type_id) = sim
        .production
        .factories
        .view(owner, category)
        .and_then(|view| view.object.map(|object| object.type_id))
    else {
        return;
    };
    if !sim
        .production
        .factories
        .account_completed_object_once(owner, category)
    {
        return;
    }
    if rules
        .object(sim.interner.resolve(type_id))
        .is_some_and(|object| object.category == ObjectCategory::Building)
    {
        sim.sound_events
            .push(SimSoundEvent::BuildingComplete { owner });
    }
}

/// Delivery effects already committed; retain the revealed identity and start
/// its successor. A refused vehicle delivery must never call this operation.
pub(in crate::sim) fn release_delivered_mobile(
    sim: &mut Simulation,
    rules: &RuleSet,
    owner: InternedId,
    category: ProductionCategory,
    effects: FrameEffects<'_>,
) {
    let type_id = sim
        .production
        .factories
        .view(owner, category)
        .and_then(|factory| factory.object.map(|object| object.type_id));
    if let Some(abandonment) = HouseFactoryAbandonment::capture(sim, owner, category) {
        abandonment.begin(sim);
        abandonment.finish(sim);
    }
    advance_after_delivery(sim, rules, owner, category, effects);
    // HousePlace4FB66D joins4FB4B4 after Abandon/StartNext, then records the
    // successfully delivered type at4FB4B7. Never score the promoted type.
    if let Some(type_id) = type_id {
        record_last_built(sim, rules, owner, type_id);
    }
}

/// `HouseClass::Record_Last_Built @ 0x004FB6B0` for an object of type
/// `type_id` that left its factory: `HouseClass::Place_Production` calls it
/// after a successful exit (`0x004FB4B7`: a player's placed building, a
/// delivered unit) and a computer's building factory after its placement
/// (`BuildingClass::Factory_AI`, `0x004501A4`). The object's kind counter
/// (`+0x55A0`, `+0x55B4`, `+0x55C8` or `+0x55DC`) grows unless its type is
/// `DontScore=` (`+0xC9F`);
/// [`MatchStatistics::built`](crate::sim::house_state::MatchStatistics) is
/// the sum of the four. Its other writes are residuals of
/// `sim::ai_base_building`.
pub(super) fn record_last_built(
    sim: &mut Simulation,
    rules: &RuleSet,
    owner: InternedId,
    type_id: InternedId,
) {
    let scores = sim
        .object_type(type_id, rules)
        .is_some_and(|ty| !ty.dont_score);
    if scores && let Some(house) = sim.houses.get_mut(&owner) {
        house.stats.record_built();
    }
}

/// Terminal mobile failure abandons the finished object with the AbandonProduction
/// refund (its whole Cost_Of, as it owes no Balance), destroys the held graph and
/// starts the successor. This is distinct from a retryable refusal.
pub(super) fn refund_failed_delivery(
    sim: &mut Simulation,
    rules: &RuleSet,
    owner: InternedId,
    category: ProductionCategory,
    effects: FrameEffects<'_>,
) {
    let abandonment = HouseFactoryAbandonment::capture(sim, owner, category);
    if let Some(abandonment) = &abandonment {
        abandonment.begin(sim);
    }
    let type_id = sim
        .production
        .factories
        .view(owner, category)
        .and_then(|factory| factory.object.map(|object| object.type_id));
    if let Some(type_id) = type_id {
        refund_abandoned(sim, rules, owner, type_id, 0);
    }
    discard_active_factory_entity(sim, rules, owner, category, effects);
    if let Some(abandonment) = abandonment {
        abandonment.finish(sim);
    }
    advance_after_delivery(sim, rules, owner, category, effects);
}

/// Matching identity captured by a successful placement admission. The caller
/// cannot combine an entity ID with another owner's type or factory category.
pub(super) struct ReadyFactoryObject {
    owner: InternedId,
    category: ProductionCategory,
    type_id: InternedId,
    entity_id: u64,
}

impl ReadyFactoryObject {
    pub(super) fn entity_id(&self) -> u64 {
        self.entity_id
    }

    /// Building Unlimbo/build-up/superweapon effects precede factory release.
    pub(super) fn release_after_placement(
        self,
        sim: &mut Simulation,
        rules: &RuleSet,
        effects: FrameEffects<'_>,
    ) {
        if let Some(abandonment) = HouseFactoryAbandonment::capture(sim, self.owner, self.category)
        {
            abandonment.begin(sim);
            abandonment.finish(sim);
        }
        advance_after_delivery(sim, rules, self.owner, self.category, effects);
    }

    /// Primary and autofill overlays are already stamped when the constructor
    /// identity is consumed. Successor construction follows.
    pub(super) fn consume_after_wall_stamp(
        self,
        sim: &mut Simulation,
        rules: &RuleSet,
        effects: FrameEffects<'_>,
    ) {
        let _ = sim.discard_constructed_limbo(self.entity_id, Some(rules), effects);
        let (owner, type_id) = (self.owner, self.type_id);
        self.release_after_placement(sim, rules, effects);
        record_last_built(sim, rules, owner, type_id);
    }
}

/// `owner`'s completed `type_id` waiting in its `category` factory: the
/// factory `FactoryClass::IsComplete @ 0x004CA130` answers (its object at the
/// last stage), holding an object of that type.
///
/// RESIDUAL: `HouseClass::Place_Production @ 0x004FB0E0` uses the event's type
/// only to choose the factory and places whatever object that factory holds
/// (`0x004FB18C`); VERA refuses a PLACE whose type is not the held object's.
/// Trigger: a building PLACE naming another type of the same factory, which
/// the sidebar never sends. Effect: native places the held building, VERA
/// refuses the event.
pub(super) fn ready_object(
    sim: &Simulation,
    owner: InternedId,
    category: ProductionCategory,
    type_id: InternedId,
) -> Option<ReadyFactoryObject> {
    let object = sim
        .production
        .factories
        .view(owner, category)?
        .complete_object()?;
    if object.type_id != type_id {
        return None;
    }
    let entity_id = object
        .entity_id
        .filter(|&id| sim.substrate.entities.contains(id))?;
    Some(ReadyFactoryObject {
        owner,
        category,
        type_id,
        entity_id,
    })
}

/// Read-only identity of a completed building held by the existing House
/// factory owner. Input uses the same ready-object lookup as placement; no
/// app-side ready flag or copy of the factory is retained.
pub(crate) fn ready_building_entity_for_owner(
    sim: &Simulation,
    rules: &RuleSet,
    owner: InternedId,
    type_id: InternedId,
) -> Option<u64> {
    let object_type = sim.object_type(type_id, rules)?;
    if object_type.category != ObjectCategory::Building {
        return None;
    }
    let category = production_category_for_object(object_type);
    ready_object(sim, owner, category, type_id).map(|object| object.entity_id())
}

/// Fixture: `owner`'s `type_id` completed in its House factory and held there
/// with its completion published, as a finished building waits for placement.
/// Returns the held object's identity.
#[cfg(test)]
pub(crate) fn complete_held_building_for_test(
    sim: &mut Simulation,
    rules: &RuleSet,
    owner: InternedId,
    type_id: InternedId,
) -> u64 {
    let object = sim
        .object_type(type_id, rules)
        .expect("completed-building fixture type");
    let (category, cost) = (production_category_for_object(object), object.cost.max(0));
    let enqueue_order = sim.production.next_enqueue_order;
    assert!(
        sim.production
            .factories
            .test_enqueue_kernel(owner, category, type_id, enqueue_order, cost),
        "the fixture arms one fresh factory head"
    );
    sim.production.next_enqueue_order = enqueue_order.saturating_add(1);
    let held = start_active_production(
        sim,
        rules,
        FactoryHolder::House(owner, category),
        type_id,
        FrameEffects::default(),
    )
    .expect("the fixture constructs its object at StartProduction");
    assert!(sim.production.factories.test_arm_ready(owner, category));
    super::production_queue::publish_production_changes(sim, rules);
    assert!(
        sim.production
            .factories
            .view(owner, category)
            .and_then(|factory| factory.complete_object())
            .is_some_and(|object| object.completion_accounted),
        "the Strip publishes the completed building"
    );
    held
}

/// Fixture-only bridge for tests that deliberately seed a registry kernel.
#[cfg(test)]
pub(in crate::sim) fn construct_active_factory_fixture(
    sim: &mut Simulation,
    rules: &RuleSet,
    owner: InternedId,
    category: ProductionCategory,
    type_id: InternedId,
) -> Option<u64> {
    start_active_production(
        sim,
        rules,
        FactoryHolder::House(owner, category),
        type_id,
        FrameEffects::default(),
    )
}

/// Revalidate before the charge sweep at its existing frame phase: first
/// `HouseClass::Update_Factory_Queue` (`FactoryRegistry::plan_revalidation`),
/// disposing all abandoned objects before starting any promoted ones, then
/// the human houses' strips ([`recalculate_strips`]). A freshly abandoned
/// factory is not charged this frame.
pub(in crate::sim) fn revalidate_and_step_factories(
    sim: &mut Simulation,
    rules: &RuleSet,
    effects: FrameEffects<'_>,
) {
    // UpdateFactoryQueue509140 -> FindFactory5F7900 -> CanBuild4F7870
    // reads Factory::Get_Object4CA160 for the held-object build-limit
    // exception (tools/can_build_oracle.json). Plan while that authoritative
    // registry remains visible: its limbo head already counts as owned.
    let reval_plan = sim.production.factories.plan_revalidation(sim, rules);
    let mut registry = std::mem::take(&mut sim.production.factories);
    let lifecycle = registry.apply_revalidation(&reval_plan);
    for (owner, abandoned) in lifecycle.abandoned {
        // Update_Factory_Queue50921C calls FactoryAbandon4C9FF0 directly;
        // it does not pass through the HouseAbandon4FAB79 placement prefix.
        settle_abandoned(sim, rules, owner, abandoned, effects);
    }
    sim.production.factories = registry;
    for (owner, category, type_id) in lifecycle.promoted {
        start_active_production(
            sim,
            rules,
            FactoryHolder::House(owner, category),
            type_id,
            effects,
        )
        .expect("validated revalidation promotion must construct one Techno");
    }
    recalculate_strips(sim, rules, effects);
    let mut registry = std::mem::take(&mut sim.production.factories);
    registry.step_all(&mut sim.houses, sim.session.binary_frame);
    sim.production.factories = registry;
}

/// `StripClass::Recalculate @ 0x006AA600` over the builds of every human
/// house, as a player's strip runs it when the house's tech tree is
/// rechecked (`HouseClass::Update @ 0x004F926C`). A type the strip no longer
/// keeps ([`strip_keeps`]) sends ABANDON (event `0x10`) when it is its
/// category's active build, whose cameo holds the factory (`0x006AA7B6`),
/// and ABANDON_ALL (event `0x2E`) when the house has a factory for the
/// type's kind (`HouseClass::GetPrimaryFactory @ 0x00500510` with build
/// category 0, so a defense asks the Buildings factory; `0x006AA809..`),
/// both through [`cancel_by_type_for_owner`]. Every event is decided before
/// any runs.
///
/// Residual: the strip recalculates only when the house's tech tree is
/// rechecked (House `+0x1FC`, set by a building's placement, opening,
/// limbo, sale and capture, trigger actions and more), and its events run
/// on a later frame; VERA decides every frame and abandons at once.
/// Trigger: a build whose prerequisite or last factory is lost. Effect: the
/// abandon and refund land a frame or more earlier. Frequency: occasional.
fn recalculate_strips(sim: &mut Simulation, rules: &RuleSet, effects: FrameEffects<'_>) {
    let mut abandons = Vec::new();
    for (owner, category, types) in sim.production.factories.house_build_types() {
        if !sim.houses.get(&owner).is_some_and(|house| house.is_human) {
            continue;
        }
        let active = sim
            .production
            .factories
            .active_object(owner, category)
            .map(|(type_id, ..)| type_id);
        let kind = match category {
            ProductionCategory::Defense => ProductionCategory::Building,
            other => other,
        };
        let kind_factory = sim.production.factories.view(owner, kind).is_some();
        for type_id in types {
            let kept = sim
                .object_type(type_id, rules)
                .is_some_and(|obj| strip_keeps(sim, rules, owner, obj));
            if !kept {
                abandons.push((owner, type_id, active == Some(type_id), kind_factory));
            }
        }
    }
    for (owner, type_id, own_factory, kind_factory) in abandons {
        let owner_name = sim.interner.resolve(owner).to_string();
        let type_name = sim.interner.resolve(type_id).to_string();
        if own_factory {
            cancel_by_type_for_owner(sim, rules, &owner_name, &type_name, false, effects);
        }
        if kind_factory {
            cancel_by_type_for_owner(sim, rules, &owner_name, &type_name, true, effects);
        }
    }
}

/// House508D88 invokes the existing Factory rate owner after power changes.
pub(in crate::sim) fn refresh_factory_rates_for_house(
    sim: &mut Simulation,
    rules: &RuleSet,
    owner: InternedId,
) {
    let mut registry = std::mem::take(&mut sim.production.factories);
    registry.refresh_rates_for_house(sim, rules, owner);
    sim.production.factories = registry;
}

/// A saved relationship that the live factory operations cannot publish.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("factory state for owner {owner}: {reason}")]
pub(crate) struct FactoryRestoreError {
    pub(crate) owner: InternedId,
    pub(crate) reason: &'static str,
}

/// Validate factory relationships before a loaded candidate becomes a match.
///
/// VERA-internal snapshot admission; gamemd equivalent UNCHECKED. These checks
/// follow the synchronous constructor/publication/settlement operations above,
/// not a new queue, refund or gameplay policy. Generic snapshot reference and
/// LogicVector validation precede this operation. Missing Houses and unrelated
/// limbo objects remain supported; manager pointers are not globally reciprocal.
pub(crate) fn validate_restored_factory_state(
    sim: &Simulation,
    rules: &RuleSet,
) -> Result<(), FactoryRestoreError> {
    use crate::map::entities::EntityCategory;
    use std::collections::BTreeSet;

    let fail = |owner, reason| FactoryRestoreError { owner, reason };
    let mut roots = BTreeSet::new();
    for (&holder, factory) in sim.production.factories.keyed_factories() {
        let (owner, category) = (factory.owner, factory.category);
        match holder {
            FactoryHolder::House(key_owner, key_category) => {
                if owner != key_owner || category != key_category {
                    return Err(fail(owner, "registry key disagrees with factory identity"));
                }
            }
            // `BuildingClass::Factory_AI` makes one for its building's owner,
            // holding one object and no queue, and counts no completion.
            FactoryHolder::Building(building) => {
                let held_by_owner = sim.substrate.entities.get(building).is_some_and(|entity| {
                    entity.category == EntityCategory::Structure && entity.owner() == owner
                });
                if !held_by_owner {
                    return Err(fail(
                        owner,
                        "factory's building is absent or another house's",
                    ));
                }
                if factory.object.is_none() || !factory.queue.is_empty() {
                    return Err(fail(owner, "building factory holds no single object"));
                }
                if factory
                    .object
                    .as_ref()
                    .is_some_and(|object| object.completion_accounted)
                {
                    return Err(fail(owner, "building factory accounted a completion"));
                }
            }
        }
        if sim.interner.try_resolve(owner).is_none() {
            return Err(fail(owner, "factory owner is absent from the interner"));
        }
        for queued in &factory.queue {
            let object = sim
                .interner
                .try_resolve(queued.type_id)
                .and_then(|name| rules.object(name))
                .ok_or_else(|| fail(owner, "queued type is absent from bound rules"))?;
            if production_category_for_object(object) != category {
                return Err(fail(owner, "queued type disagrees with factory category"));
            }
        }
        let Some(held) = &factory.object else {
            if !factory.queue.is_empty() {
                return Err(fail(owner, "queued tail has no active object"));
            }
            continue;
        };
        let object = sim
            .interner
            .try_resolve(held.type_id)
            .and_then(|name| rules.object(name))
            .ok_or_else(|| fail(owner, "active type is absent from bound rules"))?;
        if production_category_for_object(object) != category {
            return Err(fail(owner, "active type disagrees with factory category"));
        }
        let entity_id = held
            .entity_id
            .ok_or_else(|| fail(owner, "active object has no constructed identity"))?;
        if !roots.insert(entity_id) {
            return Err(fail(
                owner,
                "constructed identity belongs to multiple factories",
            ));
        }
        let entity = sim.substrate.entities.get(entity_id).ok_or_else(|| {
            fail(
                owner,
                "constructed identity is absent from the entity store",
            )
        })?;
        if entity.owner() != owner || entity.type_ref() != held.type_id {
            return Err(fail(
                owner,
                "constructed identity disagrees with factory owner or type",
            ));
        }
        let expected_category = EntityCategory::from(object.category);
        if entity.category != expected_category {
            return Err(fail(
                owner,
                "constructed identity has the wrong concrete category",
            ));
        }
        if entity.spawn_owner_id.is_some() || entity.slave.owner().is_some() {
            return Err(fail(owner, "factory root is itself a manager child"));
        }
        // Factory admission validates retained identity and membership, without
        // inferring health or death flags. Terminal-state admission separately
        // validates the object's lifecycle handoff.
        if !entity.lifecycle.in_limbo || entity.lifecycle.cell_marked || entity.in_logic_vector {
            return Err(fail(
                owner,
                "factory object is already admitted to the world",
            ));
        }
        if held.completion_accounted && factory.progress < super::PRODUCTION_STEPS {
            return Err(fail(
                owner,
                "completion accounting precedes completed progress",
            ));
        }
    }

    // Restrict only factory roots: native manager pointers elsewhere can alias
    // without implying reciprocal ownership, and ordinary limbo is not a root.
    for (_, factory) in sim.production.factories.keyed_factories() {
        let owner = factory.owner;
        let Some(parent) = factory.object.as_ref().and_then(|object| object.entity_id) else {
            continue;
        };
        let entity = sim
            .substrate
            .entities
            .get(parent)
            .expect("validated factory root");
        let spawn_alias = entity.spawn_manager.as_ref().is_some_and(|manager| {
            manager
                .slots
                .iter()
                .filter_map(|slot| slot.spawn)
                .any(|id| roots.contains(&id))
        });
        let slave_alias = entity
            .slave_manager
            .as_ref()
            .is_some_and(|manager| manager.slaves().any(|id| roots.contains(&id)));
        if spawn_alias || slave_alias {
            return Err(fail(owner, "factory root aliases a held constructor child"));
        }
    }
    Ok(())
}
