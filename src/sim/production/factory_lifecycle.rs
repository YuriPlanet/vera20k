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
use super::production_tech::{
    factory_category_for_building, production_category_for_object, strip_keeps,
};
use super::production_types::ProductionCategory;
use crate::rules::object_type::ObjectCategory;
use crate::rules::ruleset::RuleSet;
use crate::sim::ai_unit_choice::UnitChoiceKind;
use crate::sim::houses::Houses;
use crate::sim::intern::InternedId;
use crate::sim::intern::StringInterner;
use crate::sim::movement::ground_pose::Position;
use crate::sim::production::ProductionState;
use crate::sim::scenario_session::ScenarioSession;
use crate::sim::world::FrameChannel;
use crate::sim::world::FrameEffects;
use crate::sim::world::LifecycleOutput;
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
        let factory = sim.production.factories().view(owner, category)?;
        Some(Self {
            owner,
            category,
            construction_order: factory.construction_order(),
        })
    }

    fn begin(
        &self,
        session: &ScenarioSession,
        lifecycle_outputs: &mut FrameChannel<LifecycleOutput>,
    ) {
        if session.current_house() == Some(self.owner) {
            lifecycle_outputs.emit(
                crate::sim::world::LifecycleOutput::ProductionFactoryAbandoning {
                    owner: self.owner,
                    category: self.category,
                    construction_order: self.construction_order,
                },
            );
        }
    }

    fn finish(
        self,
        session: &ScenarioSession,
        lifecycle_outputs: &mut FrameChannel<LifecycleOutput>,
    ) {
        if session.current_house() == Some(self.owner) {
            lifecycle_outputs.emit(
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
/// what the strip keeps; the late House recheck sends cancellation events
/// for options it drops ([`recalculate_strips`]). A stopped active object of the same
/// type takes the same-type branch (`0x004FA5A8..0x004FA5C4`): a held build
/// resumes, and a finished one stays (the build start refuses stage 54,
/// `0x004C9ECD`). Otherwise `FactoryClass::StartProduction @ 0x004C9C70`
/// starts or queues the build. Resume checks the next charge only after
/// unpausing/rearming; House ignores its AL, so a short wallet still admits
/// the request and stalls at the next due step. A queue already holding `[General]
/// MaximumQueuedObjects=` builds, or a type at its limit
/// (`HouseClass::CheckBuildLimit`, `0x004C9CEA`), refuses the append and
/// scolds the local player.
///
/// StartNext4CA60A passes force1. Failed FindFactory(0,1,1) then permits
/// (1,0,1) at4FA45B, still rejecting Selling/InLimbo before construction.
/// Only that fallback requests a held start from the shared Resume owner;
/// the ordinary online path and Building factories pass false.
///
/// Residual (building path): a building never queues in gamemd.
/// Begin_Production refuses a building PRODUCE while the building factory runs
/// (`0x004FA52E..0x004FA550`), and StartProduction abandons a held or finished
/// building before starting a different one (`0x004C9C87..0x004C9C98`); VERA
/// appends it. Trigger: a second building PRODUCE before the first executes
/// (the sidebar refuses a busy structure strip, and VERA's AI asks only while
/// no building is queued). Effect: VERA builds both in turn. Rare.
pub fn enqueue_by_type(
    sim: &mut Simulation,
    rules: &RuleSet,
    owner: &str,
    type_id: &str,
    effects: FrameEffects<'_>,
    force: bool,
) -> bool {
    let Some(obj) = rules.object(type_id) else {
        return false;
    };
    let owner_id = sim.interner.intern(owner);
    let category = production_category_for_object(obj);
    // HouseBegin4FA41C clears the local prediction gate before FindFactory
    // can refuse. This fact leaves the cameo request itself with its strip
    // owner. Native: tools/sidebar_oracle/pending_production.md.
    if sim.session.current_house() == Some(owner_id) {
        sim.lifecycle_outputs.emit(
            crate::sim::world::LifecycleOutput::ProductionBeginAttempted {
                owner: owner_id,
                category,
                naval: obj.naval,
            },
        );
    }
    let held_start = if find_factory(sim, rules, owner_id, obj, false, true, true).is_some() {
        false
    } else if force && find_factory(sim, rules, owner_id, obj, true, false, true).is_some() {
        // House4FA45B's forced fallback alone supplies Resume(1).
        true
    } else {
        return false;
    };
    let type_interned = sim.interner.intern(type_id);
    if let Some((active, suspended)) = sim.production.factories().active_object(owner_id, category)
        && active == type_interned
        && suspended
    {
        //4FA5A8/+70 bypasses construction even at stage54. Resume4C9EA0
        // then refuses a complete product, but House ignores its AL and
        // still links/returns0. Source: factory_hold_resume.md.
        let time_to_build = time_to_build(&time_to_build_inputs(sim, rules, owner_id, obj));
        let frame = sim.session.binary_frame();
        let money = crate::sim::credit_income::available_money(&sim.houses, owner_id);
        sim.production.factories_mut().resume(
            FactoryHolder::House(owner_id, category),
            time_to_build,
            frame,
            held_start,
            money,
        );
        publish_cameo_factory_link(
            &sim.production,
            &sim.session,
            &mut sim.lifecycle_outputs,
            owner_id,
            category,
            type_interned,
        );
        return true;
    }
    let enqueue_order = sim.production.next_enqueue_order();
    let cost = sim.cost_of(owner_id, obj, rules);
    let at_build_limit = check_build_limit(sim, rules, owner_id, obj);
    let outcome = sim.production.factories_mut().enqueue(
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
            .emit(SimSoundEvent::ProductionRefused { owner: owner_id });
        return false;
    }
    sim.production.spend_enqueue_order();
    if outcome == EnqueueOutcome::Started {
        start_active_production(
            sim,
            rules,
            FactoryHolder::House(owner_id, category),
            type_interned,
            effects,
            held_start,
        )
        .expect("validated StartProduction type must construct one Techno");
    }
    true
}

/// The SUSPEND event: `HouseClass::Suspend_Production @ 0x004FA910` puts the
/// category's running build on hold.
pub(crate) fn suspend_production(
    interner: &StringInterner,
    production: &mut ProductionState,
    session: &ScenarioSession,
    owner: &str,
    category: ProductionCategory,
) -> bool {
    let Some(owner_id) = interner.get(owner) else {
        return false;
    };
    let frame = session.binary_frame();
    production
        .factories_mut()
        .suspend(owner_id, category, frame, true)
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
    held_start: bool,
) -> Option<u64> {
    let owner_id = sim
        .production
        .factories()
        .factory(holder)
        .map(|factory| factory.owner())?;
    let obj = sim.object_type(type_id, rules)?;
    let owner = sim.interner.resolve(owner_id).to_string();
    let type_name = sim.interner.resolve(type_id).to_string();
    // A factory-held object is still in limbo and has no cell authority. Zero
    // is only inert storage here; the result-bearing Unlimbo later installs the
    // selected delivery/placement coordinate on this same stable identity.
    let stable_id =
        sim.construct_object_limbo_at_height(&type_name, &owner, 0, 0, 0, 0, rules, effects)?;
    let linked = sim
        .production
        .factories_mut()
        .link_active_entity(holder, stable_id);
    if linked != Some(stable_id) {
        let _ = sim.discard_constructed_limbo(stable_id, rules, effects);
        return None;
    }
    let time_to_build = time_to_build(&time_to_build_inputs(sim, rules, owner_id, obj));
    let frame = sim.session.binary_frame();
    let money = crate::sim::credit_income::available_money(&sim.houses, owner_id);
    sim.production
        .factories_mut()
        .resume(holder, time_to_build, frame, held_start, money);
    if let FactoryHolder::House(owner, category) = holder {
        publish_cameo_factory_link(
            &sim.production,
            &sim.session,
            &mut sim.lifecycle_outputs,
            owner,
            category,
            type_id,
        );
    }
    Some(stable_id)
}

/// Transport the original reached local4FA6AA link to its app-owned strip.
/// Queued/refused StartProduction never constructs here; same-type resume
/// explicitly publishes the same link. Registry identity stays authoritative.
fn publish_cameo_factory_link(
    production: &ProductionState,
    session: &ScenarioSession,
    lifecycle_outputs: &mut FrameChannel<LifecycleOutput>,
    owner: InternedId,
    category: ProductionCategory,
    type_id: InternedId,
) {
    if session.current_house() != Some(owner) {
        return;
    }
    if let Some(factory) = production.factories().view(owner, category) {
        lifecycle_outputs.emit(
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
    holder: FactoryHolder,
    abandoned: AbandonedObject,
    effects: FrameEffects<'_>,
) {
    let category = sim
        .object_type(abandoned.type_id, rules)
        .map(|object| object.category);
    refund_abandoned(sim, rules, owner_id, abandoned.type_id, abandoned.balance);
    sim.production
        .factories_mut()
        .reset_for_abandon(holder, sim.session.binary_frame());
    let game_mode_nonzero = sim.session.game_mode_nonzero();
    if let Some(category) = category
        && let Some(house) = sim.houses.get_mut(&owner_id)
        && !house.is_controlled_by_human(game_mode_nonzero)
    {
        match UnitChoiceKind::of(category) {
            Some(kind) => house.ai_unit_choices_mut().clear(kind),
            None => house.ai_production_mut().clear_building_choice(),
        }
    }
    //4CA0E3..4CA109 brackets the scalar destructor AND final held=NULL.
    // The authoritative Factory remains registered/held through the nested
    // CanBuild/expiry callbacks; no constructor or RNG rewind occurs here.
    sim.with_object_placement_scope(|sim| {
        if let Some(entity_id) = abandoned.entity_id {
            let discarded = sim.discard_constructed_limbo(entity_id, rules, effects);
            debug_assert!(
                discarded,
                "AbandonProduction destroys the held limbo object"
            );
        }
        sim.production.factories_mut().finish_abandon(holder);
    });
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
    for holder in sim.production.factories().owner_holders_last_first(owner) {
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
    if let Some(abandoned) = sim.production.factories().pending_abandonment(holder) {
        settle_abandoned(sim, rules, owner, holder, abandoned, effects);
    }
    sim.production.factories_mut().remove_factory(holder);
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
    crate::sim::credit_income::add_credits(&mut sim.houses, owner_id, refund);
}

/// Native HouseAbandon selects either an indexed type or the wildcard -1
/// active slot used by mobile PLACE. This request is ephemeral; the registry
/// remains the sole owner of the held object, queue and Factory lifetime.
#[derive(Debug, Clone, Copy)]
pub enum ProductionCancellation<'a> {
    Type { type_id: &'a str, naval: bool },
    Category(ProductionCategory),
}

/// HouseAbandon4FAA10 for ABANDON/ALL, BuildingDetach and successful or
/// failed PLACE. The indexed branch prefers a matching queued copy for
/// BuildCat0; wildcard -1 admits NULL-held cleanup after Completed. One
/// owner publishes the local prefix, settles and selects queue/delete policy.
pub fn cancel_production_for_owner(
    sim: &mut Simulation,
    rules: &RuleSet,
    owner: &str,
    request: ProductionCancellation<'_>,
    all: bool,
    effects: FrameEffects<'_>,
) -> bool {
    let (category, type_id, build_cat) = match request {
        ProductionCancellation::Type { type_id, naval } => {
            let Some(object) = rules.object(type_id) else {
                return false;
            };
            // Native Naval independently selects the Unit slot. Detach uses
            // literal0 even for a naval producer (44EEA6);509140 later uses
            // its actual Naval. Preserve that distinction.
            let category = if object.category == ObjectCategory::Vehicle {
                if naval {
                    ProductionCategory::Ship
                } else {
                    ProductionCategory::Vehicle
                }
            } else {
                production_category_for_object(object)
            };
            (
                category,
                Some(sim.interner.intern(type_id)),
                object
                    .build_cat
                    .filter(|_| object.category == ObjectCategory::Building),
            )
        }
        ProductionCancellation::Category(category) => (category, None, None),
    };
    let owner_id = sim.interner.intern(owner);
    let abandonment = HouseFactoryAbandonment::capture(sim, owner_id, category);
    let outcome = sim
        .production
        .factories_mut()
        .cancel_one(owner_id, category, type_id, all, build_cat);
    match outcome {
        CancelOutcome::NoMatch => false,
        CancelOutcome::QueuedRemoved => true,
        CancelOutcome::AdmittedEmpty | CancelOutcome::AbandonedActive(_) => {
            let abandonment = abandonment.expect("admitted House abandon has a Factory");
            abandonment.begin(&sim.session, &mut sim.lifecycle_outputs);
            if let CancelOutcome::AbandonedActive(object) = outcome {
                settle_abandoned(
                    sim,
                    rules,
                    owner_id,
                    FactoryHolder::House(owner_id, category),
                    object,
                    effects,
                );
            }
            abandonment.finish(&sim.session, &mut sim.lifecycle_outputs);
            finish_house_abandon(sim, rules, owner_id, category, effects);
            true
        }
    }
}

/// HouseAbandon4FABAB samples the queue before StartNext. A nonempty queue
/// enters4FAC96 and retains Factory even if forced Begin rejects its last
/// entry;509140's later empty check owns that deletion. An empty queue
/// clears the House slot/deletes Factory at4FAC21/4FAC2D instead.
fn finish_house_abandon(
    sim: &mut Simulation,
    rules: &RuleSet,
    owner_id: InternedId,
    category: ProductionCategory,
    effects: FrameEffects<'_>,
) {
    let queued = sim
        .production
        .factories()
        .view(owner_id, category)
        .is_some_and(|factory| !factory.queue.is_empty());
    if queued {
        start_next_queued(sim, rules, owner_id, category, effects);
    } else {
        sim.production
            .factories_mut()
            .remove_factory(FactoryHolder::House(owner_id, category));
    }
}

/// FactoryStartNext4CA5A0 pops exactly one type, then calls the single
/// HouseBeginProduction port with force1 (4CA60A). FindFactory admission
/// precedes StartProduction and the constructor/RNG/identity allocation.
pub(super) fn start_next_queued(
    sim: &mut Simulation,
    rules: &RuleSet,
    owner: InternedId,
    category: ProductionCategory,
    effects: FrameEffects<'_>,
) {
    if let Some(type_id) = sim
        .production
        .factories_mut()
        .start_next_queued(owner, category)
    {
        let owner = sim.interner.resolve(owner).to_owned();
        let type_id = sim.interner.resolve(type_id).to_owned();
        let _ = enqueue_by_type(sim, rules, &owner, &type_id, effects, true);
    }
}

#[cfg(any(test, feature = "test-support"))]
pub(super) fn active_entity_id(
    sim: &Simulation,
    owner_id: InternedId,
    category: ProductionCategory,
) -> Option<u64> {
    sim.production
        .factories()
        .view(owner_id, category)
        .and_then(|view| view.object.and_then(|object| object.entity_id))
        .filter(|&stable_id| sim.substrate.entities.contains(stable_id))
}
/// Publish the completion edge once, without releasing the held object.
pub(super) fn publish_completion(
    interner: &StringInterner,
    production: &mut ProductionState,
    sound_events: &mut FrameChannel<SimSoundEvent>,
    rules: &RuleSet,
    owner: InternedId,
    category: ProductionCategory,
) {
    let Some(type_id) = production
        .factories()
        .view(owner, category)
        .and_then(|view| view.object.map(|object| object.type_id))
    else {
        return;
    };
    if !production
        .factories_mut()
        .account_completed_object_once(owner, category)
    {
        return;
    }
    if interner
        .object_type(type_id, rules)
        .is_some_and(|object| object.category == ObjectCategory::Building)
    {
        sound_events.emit(SimSoundEvent::BuildingComplete { owner });
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
    let product = sim
        .production
        .factories()
        .view(owner, category)
        .and_then(|factory| factory.object)
        .map(|object| {
            object
                .entity_id
                .expect("a factory product owns its constructed identity")
        });
    sim.production
        .factories_mut()
        .complete_production(owner, category, sim.session.binary_frame());
    let owner_name = sim.interner.resolve(owner).to_owned();
    let _ = cancel_production_for_owner(
        sim,
        rules,
        &owner_name,
        ProductionCancellation::Category(category),
        false,
        effects,
    );
    // HousePlace4FB66D joins4FB4B4 after Abandon/StartNext, then records the
    // successfully delivered type at4FB4B7. Never score the promoted type.
    if let Some(product) = product {
        let object = BuiltObject::capture(sim, product);
        record_last_built(
            &sim.interner,
            &mut sim.sound_events,
            &mut sim.houses,
            rules,
            owner,
            object,
        );
    }
}

/// Temporary facts from one actual product. Live delivery captures at its
/// RecordLastBuilt boundary after promotion/exit. Walls retain ctor Location
/// through native UnInit5F65F0 until deferred delete725C70. Capture their
/// retained pending identity before that shared drain without moving accounting.
/// Type and Location always come from the same authoritative entity.
#[derive(Debug, Clone, Copy)]
pub(super) struct BuiltObject {
    type_id: InternedId,
    location: Position,
}

impl BuiltObject {
    pub(super) fn capture(sim: &Simulation, product: u64) -> Self {
        let entity = sim.substrate.entities.get(product).unwrap_or_else(|| {
            panic!("successfully produced object {product} must remain available for recording")
        });
        Self {
            type_id: entity.type_ref(),
            location: entity.position,
        }
    }
}

/// `HouseClass::Record_Last_Built @ 0x004FB6B0` for the object that left
/// its factory: `HouseClass::Place_Production` calls it
/// after a successful exit (`0x004FB4B7`: a player's placed building, a
/// delivered unit) and a computer's building factory after its placement
/// (`BuildingClass::Factory_AI`, `0x004501A4`). The object's kind counter
/// (`+0x55A0`, `+0x55B4`, `+0x55C8` or `+0x55DC`) grows unless its type is
/// `DontScore=` (`+0xC9F`);
/// [`MatchStatistics::built`](crate::sim::house_state::MatchStatistics) is
/// the sum of the four. Before DontScore, each class stores its own Type
/// ArrayIndex (+26C/+270/+274/+278): TEvent19..22 reads the most recent one,
/// even for an unscored product. Executed: tag_lifecycle --build-type-events.
/// Original4FB7CA..4FB7FF overrides the class cue with type CreateSound,
/// copies raw Location and requests Voc7509E0 with null handle. No local,
/// human, alive or limbo gate applies; DontScore only gates the counter.
/// Original4FB804 requests the existing House+1FC tech recheck afterward.
/// House+246 and its90-frame timer (4F8DD3..4F8E21) remain a separate
/// documented raw-save residual.
pub(super) fn record_last_built(
    interner: &StringInterner,
    sound_events: &mut FrameChannel<SimSoundEvent>,
    houses: &mut Houses,
    rules: &RuleSet,
    owner: InternedId,
    object: BuiltObject,
) {
    let type_id = object.type_id;
    let Some(ty) = interner.object_type(type_id, rules) else {
        return;
    };
    let (category, scores) = (ty.category, !ty.dont_score);
    let sound = ty
        .create_sound()
        .or_else(|| match category {
            ObjectCategory::Vehicle => rules.general.create_unit_sound(),
            ObjectCategory::Infantry => rules.general.create_infantry_sound(),
            ObjectCategory::Aircraft => rules.general.create_aircraft_sound(),
            ObjectCategory::Building => None,
        })
        .map(str::to_owned);
    let type_index = rules
        .type_array_index(category, interner.resolve(type_id))
        .expect("a produced object belongs to its class Type registry");
    if let Some(house) = houses.get_mut(&owner) {
        house.record_built_type(category, type_index);
        if scores {
            house.record_scored_production(category, type_index);
        }
    }
    if let Some(sound) = sound {
        sound_events.emit(SimSoundEvent::voc_at(sound, &object.location));
    }
    if let Some(house) = houses.get_mut(&owner) {
        house.request_tech_recheck();
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
    let owner = sim.interner.resolve(owner).to_owned();
    let _ = cancel_production_for_owner(
        sim,
        rules,
        &owner,
        ProductionCancellation::Category(category),
        false,
        effects,
    );
}

/// Matching identity captured by a successful placement admission. The caller
/// cannot combine an entity ID with another owner's type or factory category.
pub(super) struct ReadyFactoryObject {
    owner: InternedId,
    category: ProductionCategory,
    entity_id: u64,
    type_id: InternedId,
}

impl ReadyFactoryObject {
    pub(super) fn entity_id(&self) -> u64 {
        self.entity_id
    }

    /// Factory::CompletedProduction4CA1A0 precedes the placed object's radio C.
    pub(super) fn complete_placement(
        &self,
        production: &mut ProductionState,
        session: &ScenarioSession,
    ) {
        production.factories_mut().complete_production(
            self.owner,
            self.category,
            session.binary_frame(),
        );
    }

    /// HousePlace4FB2C7 abandons only AFTER radio C4FB2AD. A consumed upgrade
    /// or wall still follows this tail; its pending object stays resolvable.
    pub(super) fn abandon_after_placement(
        self,
        sim: &mut Simulation,
        rules: &RuleSet,
        effects: FrameEffects<'_>,
    ) {
        // HousePlace4FB2C7 retains its original nonnegative index. Let
        // HouseAbandon's shared BuildCat0/NULL-held admission decide retention,
        // including a consumed wall whose entity has already expired.
        let owner = sim.interner.resolve(self.owner).to_owned();
        let type_id = sim.interner.resolve(self.type_id).to_owned();
        let naval = sim
            .object_type(self.type_id, rules)
            .is_some_and(|object| object.naval);
        let _ = cancel_production_for_owner(
            sim,
            rules,
            &owner,
            ProductionCancellation::Type {
                type_id: &type_id,
                naval,
            },
            false,
            effects,
        );
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
        .factories()
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
        entity_id,
        type_id,
    })
}

/// Read-only identity of a completed building held by the existing House
/// factory owner. Input uses the same ready-object lookup as placement; no
/// app-side ready flag or copy of the factory is retained.
pub fn ready_building_entity_for_owner(
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
#[cfg(any(test, feature = "test-support"))]
pub fn complete_held_building_for_test(
    sim: &mut Simulation,
    rules: &RuleSet,
    owner: InternedId,
    type_id: InternedId,
) -> u64 {
    let object = sim
        .object_type(type_id, rules)
        .expect("completed-building fixture type");
    let (category, cost) = (production_category_for_object(object), object.cost.max(0));
    let enqueue_order = sim.production.next_enqueue_order();
    assert!(
        sim.production.factories_mut().test_enqueue_kernel(
            owner,
            category,
            type_id,
            enqueue_order,
            cost
        ),
        "the fixture arms one fresh factory head"
    );
    sim.production.spend_enqueue_order();
    let held = start_active_production(
        sim,
        rules,
        FactoryHolder::House(owner, category),
        type_id,
        FrameEffects::default(),
        false,
    )
    .expect("the fixture constructs its object at StartProduction");
    assert!(
        sim.production
            .factories_mut()
            .test_arm_ready(owner, category)
    );
    super::production_queue::publish_production_changes(sim, rules);
    assert!(
        sim.production
            .factories()
            .view(owner, category)
            .and_then(|factory| factory.complete_object())
            .is_some_and(|object| object.completion_accounted),
        "the Strip publishes the completed building"
    );
    held
}

/// Fixture-only bridge for tests that deliberately seed a registry kernel.
#[cfg(any(test, feature = "test-support"))]
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
        false,
    )
}

/// HouseUpdateFactoryQueue509140, synchronously called by the represented
/// Building Unlimbo/Limbo/ReadINI/ChangeOwner sites. Reverse tail pruning
/// precedes the held-object query, synchronous Abandon/StartNext, and finally
/// empty Factory deletion (50928C..5092A0). Queries and nested destruction
/// always see the authoritative registry. No per-frame eligibility scan.
///
/// A retained head with only offline factories enters Suspend(0); a later
/// callback with an online factory resumes only a system hold (+70 && !+71).
/// Temporal start/stop has no native509140 callback. The independent
/// GoOnline/GoOffline owner calls here at452337/452403.
pub(in crate::sim) fn update_factory_queue(
    sim: &mut Simulation,
    rules: &RuleSet,
    owner: InternedId,
    category: ProductionCategory,
    effects: FrameEffects<'_>,
) {
    let queued: Vec<_> = sim
        .production
        .factories()
        .view(owner, category)
        .map(|factory| factory.queue.iter().map(|entry| entry.type_id).collect())
        .unwrap_or_default();
    for (index, type_id) in queued.into_iter().enumerate().rev() {
        let lost = sim.object_type(type_id, rules).is_none_or(|object| {
            find_factory(sim, rules, owner, object, true, false, true).is_none()
        });
        if lost {
            sim.production
                .factories_mut()
                .remove_queued(owner, category, index);
        }
    }
    let active = sim
        .production
        .factories()
        .view(owner, category)
        .and_then(|factory| factory.object.map(|object| object.type_id));
    if active.is_some_and(|type_id| {
        sim.object_type(type_id, rules).is_none_or(|object| {
            find_factory(sim, rules, owner, object, true, false, true).is_none()
        })
    }) {
        if let Some(abandoned) = sim
            .production
            .factories()
            .pending_abandonment(FactoryHolder::House(owner, category))
        {
            settle_abandoned(
                sim,
                rules,
                owner,
                FactoryHolder::House(owner, category),
                abandoned,
                effects,
            );
        }
        start_next_queued(sim, rules, owner, category, effects);
    } else if let Some(type_id) = active
        && let Some(object) = sim.object_type(type_id, rules)
    {
        //50922A..509287 only visits the ORIGINAL retained head. The loss
        //branch above jumps past this even if StartNext constructs a successor.
        if find_factory(sim, rules, owner, object, true, true, true).is_none() {
            let frame = sim.session.binary_frame();
            sim.production
                .factories_mut()
                .suspend(owner, category, frame, false);
            //509252..50926E requests repaint of the active strip, without
            //selection/EVA/membership changes. The client already redraws its
            //existing FactoryView each frame; no second dirty-state owner.
        } else if sim
            .production
            .factories()
            .automatic_resume_pending(owner, category)
        {
            let time_to_build = time_to_build(&time_to_build_inputs(sim, rules, owner, object));
            let frame = sim.session.binary_frame();
            let money = crate::sim::credit_income::available_money(&sim.houses, owner);
            sim.production.factories_mut().resume(
                FactoryHolder::House(owner, category),
                time_to_build,
                frame,
                false,
                money,
            );
        }
    }
    if sim
        .production
        .factories()
        .view(owner, category)
        .is_some_and(|factory| factory.object.is_none() && factory.queue.is_empty())
    {
        // Scalar Factory expiry clears the House pointer, without SidebarAbandon.
        // Existing cameo identity/live checks reject any stale raw link.
        sim.production
            .factories_mut()
            .remove_factory(FactoryHolder::House(owner, category));
    }
}

/// BuildingDetach44EBF0's production prelude, shared by Conceal and the
/// exact-zero damage callback. The separate Building+524 Factory is abandoned
/// first. House slots have no retained human producer binding: test the held
/// type with this building temporarily InLimbo, then ordinary HouseAbandon.
/// Source: original44EC01..44EF24; consumed factory_loss sale-queue controls.
pub(in crate::sim) fn detach_building_production(
    sim: &mut Simulation,
    rules: &RuleSet,
    building: u64,
    effects: FrameEffects<'_>,
) {
    super::factory_ai::detach_all(sim, rules, building, effects);
    let Some((owner, category)) = sim.substrate.entities.get(building).and_then(|entity| {
        let object = sim.object_type(entity.type_ref(), rules)?;
        Some((entity.owner(), factory_category_for_building(object)?))
    }) else {
        return;
    };
    // Native yards scan five literal slots53BC..53CC. BC/CC are represented
    // here. No assigning writer for the middle three was found in the bounded
    // literal-field inspection; alias writes and loaded native saves stay open.
    let categories = if category == ProductionCategory::Building {
        &[ProductionCategory::Building, ProductionCategory::Defense][..]
    } else {
        std::slice::from_ref(&category)
    };
    for &category in categories {
        let Some(type_id) = sim
            .production
            .factories()
            .view(owner, category)
            .and_then(|factory| factory.object.map(|object| object.type_id))
        else {
            continue;
        };
        sim.with_factory_building_in_limbo(building, |sim| {
            let lost = sim.object_type(type_id, rules).is_some_and(|object| {
                find_factory(sim, rules, owner, object, true, false, false).is_none()
            });
            if lost {
                let owner = sim.interner.resolve(owner).to_owned();
                let type_id = sim.interner.resolve(type_id).to_owned();
                let _ = cancel_production_for_owner(
                    sim,
                    rules,
                    &owner,
                    crate::sim::production::ProductionCancellation::Type {
                        type_id: &type_id,
                        naval: false,
                    },
                    false,
                    effects,
                );
            }
        });
    }
}

/// The global FactoryAI charge sweep55B66A runs after all live-object
/// callbacks and before the late House strip recheck/command tail.
pub(in crate::sim) fn step_factories(sim: &mut Simulation) {
    let mut registry = std::mem::take(sim.production.factories_mut());
    registry.step_all(&mut sim.houses, sim.session.binary_frame());
    #[cfg(feature = "debug-tools")]
    if let Some(owner) = sim.trainer_owner()
        && sim.debug_flags_for(owner).instant_build
    {
        registry.debug_complete(&mut sim.houses, sim.session.binary_frame(), owner);
    }
    *sim.production.factories_mut() = registry;
}

/// `StripClass::Recalculate @ 0x006AA600` over the builds of every human
/// house, as a player's strip runs it when the house's tech tree is
/// rechecked (`HouseClass::Update @ 0x004F926C`). A type the strip no longer
/// keeps ([`strip_keeps`]) sends ABANDON (event `0x10`) when it is its
/// category's active build, whose cameo holds the factory (`0x006AA7B6`),
/// and ABANDON_ALL (event `0x2E`) when the house has a factory for the
/// type's kind (`HouseClass::GetPrimaryFactory @ 0x00500510` with build
/// category 0, so a defense asks the Buildings factory; `0x006AA809..`),
/// both through the existing `CancelProductionByType` event dispatcher and
/// [`cancel_production_for_owner`]. Every event is decided before any runs.
///
/// Local MainTick55DC9E -> ProcessCommandQueues647260 (55DE40) executes
/// these events in the same binary-frame tail, before increment55DE7E.
/// Recheck admission is the private House+1FC owner, not a per-frame scan.
/// No RNG/timer/detach writes occur here; event execution reuses the owning
/// factory's cancellation/promotion and held-object lifecycle.
///
/// Residual: native client-local sidebar entry order and saturated/network
/// OutList transport are separate mechanisms. VERA represents every human
/// strip in deterministic House order; a single rejected stock MGTK cameo
/// has no cross-entry ordering ambiguity. TAction106 now requests this same
/// House recheck. Other unported trigger writers and unpowered-factory cameo
/// admission remain separate existing gaps.
pub(in crate::sim) fn recalculate_strips(
    sim: &Simulation,
    rules: &RuleSet,
    owner: InternedId,
) -> Vec<crate::sim::command::CommandEnvelope> {
    use crate::sim::command::{Command, CommandEnvelope};
    if !sim.houses.get(&owner).is_some_and(|house| house.is_human()) {
        return Vec::new();
    }
    let mut abandons = Vec::new();
    for (factory_owner, category, types) in sim.production.factories().house_build_types() {
        if factory_owner != owner {
            continue;
        }
        let active = sim
            .production
            .factories()
            .active_object(owner, category)
            .map(|(type_id, ..)| type_id);
        let kind = match category {
            ProductionCategory::Defense => ProductionCategory::Building,
            other => other,
        };
        let kind_factory = sim.production.factories().view(owner, kind).is_some();
        for type_id in types {
            let kept = sim
                .object_type(type_id, rules)
                .is_some_and(|obj| strip_keeps(sim, rules, owner, obj));
            if !kept {
                abandons.push((type_id, active == Some(type_id), kind_factory));
            }
        }
    }
    let mut events = Vec::new();
    let execute_tick = sim.session.tick().saturating_add(1);
    for (type_id, own_factory, kind_factory) in abandons {
        if own_factory {
            events.push(CommandEnvelope::new(
                owner,
                execute_tick,
                Command::CancelProductionByType {
                    type_id,
                    all: false,
                },
            ));
        }
        if kind_factory {
            events.push(CommandEnvelope::new(
                owner,
                execute_tick,
                Command::CancelProductionByType { type_id, all: true },
            ));
        }
    }
    events
}

/// House508D88 invokes the existing Factory rate owner after power changes.
pub(in crate::sim) fn refresh_factory_rates_for_house(
    sim: &mut Simulation,
    rules: &RuleSet,
    owner: InternedId,
) {
    let mut registry = std::mem::take(sim.production.factories_mut());
    registry.refresh_rates_for_house(sim, rules, owner);
    *sim.production.factories_mut() = registry;
}

/// A saved relationship that the live factory operations cannot publish.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("factory state for owner {owner}: {reason}")]
pub struct FactoryRestoreError {
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
/// VERA-only: checks each held object's InLimbo, Marked and LogicVector facts and its
/// spawn/slave links against the loaded store, their source of truth.
pub fn validate_restored_factory_state(
    sim: &Simulation,
    rules: &RuleSet,
) -> Result<(), FactoryRestoreError> {
    use crate::map::entities::EntityCategory;
    use std::collections::BTreeSet;

    let fail = |owner, reason| FactoryRestoreError { owner, reason };
    let mut roots = BTreeSet::new();
    for (&holder, factory) in sim.production.factories().keyed_factories() {
        let (owner, category) = (factory.owner(), factory.category());
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
                    entity.category() == EntityCategory::Structure && entity.owner() == owner
                });
                if !held_by_owner {
                    return Err(fail(
                        owner,
                        "factory's building is absent or another house's",
                    ));
                }
                if factory.object().is_none() || !factory.queue().is_empty() {
                    return Err(fail(owner, "building factory holds no single object"));
                }
                if factory
                    .object()
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
        for queued in factory.queue() {
            let object = sim
                .interner
                .try_resolve(queued.type_id)
                .and_then(|name| rules.object(name))
                .ok_or_else(|| fail(owner, "queued type is absent from bound rules"))?;
            if production_category_for_object(object) != category {
                return Err(fail(owner, "queued type disagrees with factory category"));
            }
        }
        let Some(held) = &factory.object() else {
            if !factory.queue().is_empty() {
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
        if entity.category() != expected_category {
            return Err(fail(
                owner,
                "constructed identity has the wrong concrete category",
            ));
        }
        if entity.spawn_owner_id.owner().is_some() || entity.slave.owner().is_some() {
            return Err(fail(owner, "factory root is itself a manager child"));
        }
        // Factory admission validates retained identity and membership, without
        // inferring health or death flags. Terminal-state admission separately
        // validates the object's lifecycle handoff.
        if !entity.lifecycle.in_limbo()
            || entity.lifecycle.cell_marked()
            || entity.in_logic_vector.is_member()
        {
            return Err(fail(
                owner,
                "factory object is already admitted to the world",
            ));
        }
        if held.completion_accounted && factory.progress() < super::PRODUCTION_STEPS {
            return Err(fail(
                owner,
                "completion accounting precedes completed progress",
            ));
        }
    }

    // Restrict only factory roots: native manager pointers elsewhere can alias
    // without implying reciprocal ownership, and ordinary limbo is not a root.
    for (_, factory) in sim.production.factories().keyed_factories() {
        let owner = factory.owner();
        let Some(parent) = factory.object().and_then(|object| object.entity_id) else {
            continue;
        };
        let entity = sim
            .substrate
            .entities
            .get(parent)
            .expect("validated factory root");
        let spawn_alias = entity.spawn_manager.as_ref().is_some_and(|manager| {
            manager
                .slots()
                .iter()
                .filter_map(|slot| slot.spawn())
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
