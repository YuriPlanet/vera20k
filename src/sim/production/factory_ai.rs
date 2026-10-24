//! A computer house's production at its own factory buildings:
//! `BuildingClass::Factory_AI @ 0x004500F0`, run in the building's Update
//! right after UpdateRepairAndPower when its type has a `Factory=`
//! (`0x004401BB..0x004401CD`). The building's factory
//! (`BuildingClass+0x524`) sits in the registry under
//! [`FactoryHolder::Building`] and steps with every other factory in
//! construction order; before each try at placing its object it waits for
//! the building's `ai_placement_timer` (`+0x550`).
//!
//! Each visit, in order:
//! - A finished object (`FactoryClass::IsComplete @ 0x004CA130`) whose wait
//!   is over leaves through `Exit_Object` (`vt+0x100`: a building through
//!   [`ai_base_building::exit_building`], any other object through
//!   [`exit_produced_object`]). Placed: `Record_Last_Built @ 0x004FB6B0` and
//!   `CompletedProduction @ 0x004CA1A0`, which lets the object go, then the
//!   factory is deleted. Try later: the wait restarts for `[General]
//!   PlacementDelay=`. Failed: a naval building turns the house's naval
//!   choices off, then `AbandonProduction @ 0x004C9FF0` refunds and destroys
//!   the object and the factory is deleted.
//! - Then, for a house whose Production latch is set (`+0x1EE`) and a
//!   building neither building up nor selling (`Get_Mission` 0x12/0x13; a
//!   yard whose build-up ends this frame already reads Guard, as the repair
//!   step does, `GameEntity::constructing_or_selling`): a
//!   factory whose wait is over with no rate or stopped is abandoned; with
//!   no factory, a house holding more than 10 credits makes one for its
//!   choice (`Suggest_New_Object @ 0x004FBD80`) and starts it
//!   (`FactoryClass::StartProduction @ 0x004C9C70`, then the build start
//!   `0x004C9EA0`; the call between, `0x004FF540`, is an empty body); a
//!   failed start deletes it unrefunded. A war factory makes no Unit whose
//!   `Naval=` differs from its own (`0x00450343..0x0045036A`). The NPSE
//!   extension switch `[General] DisableParallelAIQueues=` blocks the start
//!   when the house already holds another factory producing the same
//!   produced-type class and `Naval=` — see
//!   [`house_holds_parallel_production`].
//!
//! The same factory also goes, abandoned, with its building's
//! `Detach_All(1)` (a kill, a sale's completion or a Limbo, [`detach_all`])
//! and `ChangeOwner` (`0x004486DF..0x00448701`).
//!
//! Evidence: instruction reading of the bodies named here. The retry wait
//! (`ftol(PlacementDelay * 900.0)`, `RuleSet::placement_delay_frames`) is
//! executed by `tools/ai_base_building_oracle.py`.
//!
//! RESIDUALS:
//! - A placed object plays the building type's `CreateUnitSound=`
//!   (`+0xE74`, read at `0x004607C8..0x004607F8`) at the building
//!   (`0x00450198`); among retail types only the Cloning Vats set it, which
//!   have no `Factory=`.
//! - The try-later branch also copies an uninitialised stack word into the
//!   timer's middle field (`+0x554`, `0x004501F1`), which no reader uses.
//! - Mobile products leave through [`exit_produced_object`], the same
//!   `Exit_Object` arms a human's placement takes, including the Infantry
//!   arm's Cloning Vat copies. Its residual: the refinery arm's Unit exit.

use crate::map::entities::EntityCategory;
use crate::rules::object_type::ObjectCategory;
use crate::rules::object_type::ObjectType;
use crate::rules::overlay_types::OverlayTypeRegistry;
use crate::rules::ruleset::RuleSet;
use crate::sim::ai_base_building::{self, BuildingExit};
use crate::sim::ai_unit_choice::UnitChoiceKind;
use crate::sim::intern::InternedId;
use crate::sim::timer::CdTimer;
use crate::sim::world::FrameEffects;
use crate::sim::world::Simulation;

use super::factory::{FactoryHolder, PRODUCTION_STEPS};
use super::factory_lifecycle::{
    BuiltObject, abandon_production, record_last_built, start_active_production,
};
use super::production_queue::exit_produced_object;
use super::production_tech::production_category_for_object;

/// `BuildingClass::Factory_AI @ 0x004500F0` for building `building`.
pub(crate) fn factory_ai(
    sim: &mut Simulation,
    rules: &RuleSet,
    building: u64,
    overlay_registry: Option<&OverlayTypeRegistry>,
    effects: FrameEffects<'_>,
) {
    let Some(owner) = sim.substrate.entities.get(building).map(|b| b.owner()) else {
        return;
    };
    exit_finished_object(sim, rules, building, owner, overlay_registry, effects);

    // `0x00450248..0x0045028C`.
    let Some(entity) = sim.substrate.entities.get(building) else {
        return;
    };
    let production = sim
        .houses
        .get(&owner)
        .is_some_and(|house| house.ai_activation().production);
    if !production || entity.constructing_or_selling() {
        return;
    }
    // The second raw gate (450286) precedes the existing-factory abandon
    // branch (450292); finished-object exit above remains unconditional.
    let Some(factory_rtti) = sim
        .substrate
        .entities
        .get(building)
        .and_then(|entity| sim.entity_object_type(entity, rules))
        .map(|ty| ty.factory_rtti())
        .filter(|&factory| factory != 0)
    else {
        return;
    };
    if sim
        .production
        .factories()
        .building_factory(building)
        .is_some()
    {
        abandon_stopped_factory(sim, rules, building, owner, effects);
        return;
    }
    start_factory(sim, rules, building, owner, factory_rtti, effects);
}

/// `0x004500FA..0x00450242`: the finished object's exit once the wait is
/// over, whatever the house.
fn exit_finished_object(
    sim: &mut Simulation,
    rules: &RuleSet,
    building: u64,
    owner: InternedId,
    overlay_registry: Option<&OverlayTypeRegistry>,
    effects: FrameEffects<'_>,
) {
    let Some(object) = sim
        .production
        .factories()
        .building_factory(building)
        .filter(|factory| factory.progress() >= PRODUCTION_STEPS)
        .and_then(|factory| factory.object().cloned())
    else {
        return;
    };
    if !placement_wait_over(sim, building) {
        return;
    }
    let Some(product) = object.entity_id else {
        return;
    };
    let Some(product_type) = sim
        .substrate
        .entities
        .get(product)
        .and_then(|entity| sim.entity_object_type(entity, rules))
    else {
        return;
    };
    let is_building = product_type.category == ObjectCategory::Building;
    let exit = exit_produced_object(
        sim,
        rules,
        building,
        product,
        (0, 0),
        overlay_registry,
        effects,
    );
    if !is_building && exit == BuildingExit::Placed {
        sim.note_mission_spawned_entities();
    }
    match exit {
        BuildingExit::Placed => {
            //4501A4 reads after exit. A consumed wall is still retained at
            //its constructor Location until the frame's shared725C70 drain.
            let object = BuiltObject::capture(sim, product);
            record_last_built(
                &sim.interner,
                &mut sim.sound_events,
                &mut sim.houses,
                rules,
                owner,
                object,
            );
            // CompletedProduction lets the placed object go; the delete
            // finds none to abandon.
            sim.production
                .factories_mut()
                .remove_building_factory(building);
        }
        BuildingExit::TryLater => {
            let frame = sim.session.binary_frame() as i32;
            let delay = rules.general.placement_delay_frames();
            if let Some(entity) = sim.substrate.entities.get_mut(building) {
                entity.ai_placement_timer.restart(frame, delay);
            }
        }
        BuildingExit::Failed => {
            // `0x004501FF..0x00450220`: the object's owner stops choosing
            // naval buildings.
            let naval_building = sim
                .substrate
                .entities
                .get(product)
                .and_then(|entity| sim.entity_object_type(entity, rules))
                .is_some_and(|ty| ty.category == ObjectCategory::Building && ty.naval);
            if naval_building && let Some(house) = sim.houses.get_mut(&owner) {
                house.ai_production_mut().forbid_naval();
            }
            abandon_production(
                sim,
                rules,
                FactoryHolder::Building(building),
                owner,
                effects,
            );
        }
    }
}

/// `0x00450292..0x004502E7`: a factory whose wait is over with no rate or
/// stopped (`+0x38`, `+0x70`) is abandoned.
fn abandon_stopped_factory(
    sim: &mut Simulation,
    rules: &RuleSet,
    building: u64,
    owner: InternedId,
    effects: FrameEffects<'_>,
) {
    if !placement_wait_over(sim, building) {
        return;
    }
    let stopped = sim
        .production
        .factories()
        .building_factory(building)
        .is_some_and(|factory| !factory.view().is_running());
    if stopped {
        abandon_production(
            sim,
            rules,
            FactoryHolder::Building(building),
            owner,
            effects,
        );
    }
}

/// `0x004502F4..0x004503C5`: with more than 10 credits, a new factory for
/// the house's choice, started at once.
fn start_factory(
    sim: &mut Simulation,
    rules: &RuleSet,
    building: u64,
    owner: InternedId,
    factory_rtti: i32,
    effects: FrameEffects<'_>,
) {
    if crate::sim::credit_income::available_money(&sim.houses, owner) <= 10 {
        return;
    }
    // The producing building's `Naval=` decides which vehicle choice
    // `Suggest_New_Object` reads (NPSE `0xC2F130`), so resolve it first.
    let factory_naval = sim
        .substrate
        .entities
        .get(building)
        .and_then(|entity| sim.entity_object_type(entity, rules))
        .is_some_and(|ty| ty.naval);
    let Some(object) =
        ai_base_building::suggest_new_object(sim, rules, owner, factory_rtti, factory_naval)
    else {
        return;
    };
    // The NPSE per-type `AIBuildLimit=` runs where `Factory_AI` fetched the
    // type: at limit the category's choice slot clears and the start skips.
    if crate::rules::NPATCH_ENABLED
        && ai_build_limit_blocks(sim, rules, owner, object, object.aibuild_limit)
    {
        clear_aibuild_limit_choice(sim, owner, object.category);
        return;
    }
    // The NPSE `[General] DisableParallelAIQueues=` gate runs where
    // `Factory_AI` fetched the type, after the build-limit arm and before the
    // naval mismatch check (`0x45033C` hook, cave `0xC335A0` -> allow path
    // `0xC2F07C`). The cave reads `Rules+0x2D` and, when set, blocks the start
    // while the house already counts a producing object of the fetched type's
    // class; the counters it checks live at `House+0x160D4` (aircraft),
    // `+0x160D8` (unit, `UnitType+0xCCE` clear), `+0x160DC` (unit,
    // `UnitType+0xCCE` set), `+0x160E0` (infantry) and `+0x160E4` (building).
    // `0xCCE` is the `Naval=` byte (`0x00714A77`), so ground and naval units
    // count apart — the NPSE #333 fix for a global switch that used to stop
    // naval yards.
    #[cfg(feature = "npatch")]
    if rules.general.disable_parallel_ai_queues
        && house_holds_parallel_production(sim, rules, owner, building, object)
    {
        return;
    }
    if object.category == ObjectCategory::Vehicle && object.naval != factory_naval {
        return;
    }
    let type_id = sim.interner.intern(&object.id);
    let category = production_category_for_object(object);
    let cost = sim.cost_of(owner, object, rules);
    // The FactoryClass constructor joins the factory vector
    // (`0x004C9974..0x004C9989`) before StartProduction constructs the object.
    let insertion_seq = sim.production.next_enqueue_order();
    sim.production.spend_enqueue_order();
    sim.production.factories_mut().create_building_factory(
        building,
        owner,
        category,
        type_id,
        insertion_seq,
        cost,
    );
    if start_active_production(
        sim,
        rules,
        FactoryHolder::Building(building),
        type_id,
        effects,
        false,
    )
    .is_none()
    {
        // `0x004503A7 -> 0x004502DC`: deleted, nothing held to abandon.
        sim.production
            .factories_mut()
            .remove_building_factory(building);
    }
}

/// `CDTimerClass` at `+0x550` has run out (`0x00450115..0x00450136`).
fn placement_wait_over(sim: &Simulation, building: u64) -> bool {
    let frame = sim.session.binary_frame() as i32;
    sim.substrate
        .entities
        .get(building)
        .is_some_and(|entity| entity.ai_placement_timer.expired(frame))
}

/// The NPSE per-type `AIBuildLimit=` side of the `Factory_AI` fetch hook
/// (cave `0xC335A0..0xC33726`): whether `owner` is at `object`'s limit. The
/// cave branches on the fetched type's class — `0x10` infantry, `0x28` unit,
/// `3` aircraft, `7` building — and reads the limit from `Type+0x40`
/// (`0xC335C4`; the constructor stores `0x7FFFFFFF` there, `0xC32950`, and
/// the reader at `0xC27249..0xC2725A` defaults to the current value). A positive limit
/// compares the house's tracked count of the type
/// (`Add_Tracking @ 0x004FF700` counts the limbo factory object from
/// production start, so clone production fills up to the limit and never
/// exceeds it) with `jl` for "under the limit"; a non-positive one compares
/// its produced and tracked counts by magnitude (`neg` then `jle`, the
/// `House+0x55B4..0x55DC` produced and `+0x5514..0x553C` tracked tables
/// through `0x49FAE0`), so an explicit `0` blocks the type outright.
fn ai_build_limit_blocks(
    sim: &Simulation,
    rules: &RuleSet,
    owner: InternedId,
    object: &ObjectType,
    limit: i32,
) -> bool {
    let owned = sim
        .interner
        .get(&object.id)
        .zip(sim.houses.get(&owner))
        .map_or(0, |(type_id, house)| {
            house
                .tracking()
                .owned_count(EntityCategory::from(object.category), type_id)
        });
    if limit > 0 {
        return owned >= limit;
    }
    let produced = rules
        .type_array_index(object.category, &object.id)
        .zip(sim.houses.get(&owner))
        .map_or(0, |(index, house)| {
            house.produced_count(object.category, index)
        });
    produced.wrapping_neg() <= limit || owned.wrapping_neg() <= limit
}

/// The at-limit arm: the native hook writes `-1` into the category's
/// production choice slot before the start skips — `+0x564C` building
/// (`0xC3371C`), `+0x5654` infantry (`0xC3360C`), `+0x5658` aircraft, and for
/// a unit `+0x5650` or the naval queue's `+0x16130` by `UnitType+0xCCE`
/// (`0xC33570`). VERA keeps one unit slot, so a naval type clears that one;
/// the `+0x16130` split is a residual. A capped building is cleared too, and
/// its plan node stays unsatisfied, so `choose_building` returns the same
/// type: that is the manual's warning (`AI克隆生产`) that a building at its
/// limit leaves the computer unable to build anything, not a recovery.
fn clear_aibuild_limit_choice(sim: &mut Simulation, owner: InternedId, category: ObjectCategory) {
    let Some(house) = sim.houses.get_mut(&owner) else {
        return;
    };
    match UnitChoiceKind::of(category) {
        Some(kind) => house.ai_unit_choices_mut().clear(kind),
        None => house.ai_production_mut().clear_building_choice(),
    }
}

/// `BuildingClass+0x550..+0x558`, the wait before the building's computer
/// factory tries its finished object again. The constructor starts it with no
/// duration (`0x0043B7A7`, `0x0043B7AD`); a placement that must wait restarts
/// it for `[General] PlacementDelay=` (`0x004501CB..0x004501F7`). Only this
/// module writes or reads it.
#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize,
)]
#[serde(transparent)]
pub(crate) struct AiPlacementTimer {
    timer: CdTimer,
}

/// The NPSE parallel gate's "is the class counter above zero" side
/// (`0xC2F07C..0xC2F117`): `owner` holds another building's factory whose
/// current object's type has the same class and `Naval=` as `object`, counted
/// from production start and released when the object leaves through
/// `Exit_Object` (`0x443CCA..0x4452F0`) or `AbandonProduction` (`0x4CA004`) —
/// so a finished object still waiting to leave still counts. The live scan
/// replaces the native counters (`House+0x160D4..0x160E4`); only `Factory_AI`
/// starts ever incremented them, so House-holder production stays outside, and
/// a `PointerExpired` clearing releases here where the native counter would
/// leak. The cave splits the unit class on `UnitType+0xCCE` (the `Naval=` byte,
/// `0x00714A77`) into two counters, which is NPSE #333's fix for a global
/// switch that otherwise stopped the AI's naval yards behind its war
/// factories; the class split below follows it, so a ground factory never
/// blocks a naval one and the reverse holds too.
#[cfg(feature = "npatch")]
fn house_holds_parallel_production(
    sim: &Simulation,
    rules: &RuleSet,
    owner: InternedId,
    building: u64,
    object: &ObjectType,
) -> bool {
    sim.production
        .factories()
        .keyed_factories()
        .any(|(holder, factory)| {
            matches!(holder, FactoryHolder::Building(other) if *other != building)
                && factory.owner() == owner
                && factory.object().is_some_and(|held| {
                    sim.object_type(held.type_id, rules).is_some_and(|ty| {
                        ty.category == object.category && ty.naval == object.naval
                    })
                })
        })
}

impl AiPlacementTimer {
    /// The timer, for the state hash.
    pub(crate) fn timer(self) -> CdTimer {
        self.timer
    }

    fn restart(&mut self, frame: i32, delay: i32) {
        self.timer = CdTimer::started(frame, delay);
    }

    fn expired(self, frame: i32) -> bool {
        self.timer.expired(frame)
    }
}

/// `BuildingClass::Detach_All(1) @ 0x0044EBF0`'s factory arm
/// (`0x0044EC01..0x0044EC21`) and `BuildingClass::ChangeOwner`'s
/// (`0x004486DF..0x00448701`): the building's factory is abandoned, its
/// object refunded and destroyed, and deleted.
pub(crate) fn detach_all(
    sim: &mut Simulation,
    rules: &RuleSet,
    building: u64,
    effects: FrameEffects<'_>,
) {
    let Some(factory) = sim.production.factories().building_factory(building) else {
        return;
    };
    let owner = factory.owner();
    abandon_production(
        sim,
        rules,
        FactoryHolder::Building(building),
        owner,
        effects,
    );
}

#[cfg(test)]
#[path = "factory_ai_tests.rs"]
mod tests;
