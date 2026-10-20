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
//!   `Naval=` differs from its own (`0x00450343..0x0045036A`).
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
//!   `Exit_Object` arms a human's placement takes. Its residuals: the
//!   refinery arm and the Infantry arm's Cloning Vat copies.

use crate::rules::object_type::ObjectCategory;
use crate::rules::overlay_types::OverlayTypeRegistry;
use crate::rules::ruleset::RuleSet;
use crate::sim::ai_base_building::{self, BuildingExit};
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
        .and_then(|entity| sim.object_type(entity.type_ref(), rules))
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
        .filter(|factory| factory.progress >= PRODUCTION_STEPS)
        .and_then(|factory| factory.object.clone())
    else {
        return;
    };
    if !placement_wait_over(sim, building) {
        return;
    }
    let Some(product) = object.entity_id else {
        return;
    };
    let Some(product_type) = sim.object_type(object.type_id, rules) else {
        return;
    };
    let is_building = product_type.category == ObjectCategory::Building;
    let exit = exit_produced_object(sim, rules, building, product, overlay_registry, effects);
    if !is_building && exit == BuildingExit::Placed {
        sim.mission_spawned_entities = true;
    }
    match exit {
        BuildingExit::Placed => {
            //4501A4 reads after exit. A consumed wall is still retained at
            //its constructor Location until the frame's shared725C70 drain.
            let object = BuiltObject::capture(sim, product);
            record_last_built(sim, rules, owner, object);
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
                .object_type(object.type_id, rules)
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
    if crate::sim::credit_income::available_money(sim, owner) <= 10 {
        return;
    }
    let Some(object) = ai_base_building::suggest_new_object(sim, rules, owner, factory_rtti) else {
        return;
    };
    let factory_naval = sim
        .substrate
        .entities
        .get(building)
        .and_then(|entity| sim.object_type(entity.type_ref(), rules))
        .is_some_and(|ty| ty.naval);
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
/// object refunded and destroyed, and deleted. Without rules (fixtures) the
/// object is destroyed unrefunded.
pub(crate) fn detach_all(
    sim: &mut Simulation,
    rules: Option<&RuleSet>,
    building: u64,
    effects: FrameEffects<'_>,
) {
    let Some(factory) = sim.production.factories().building_factory(building) else {
        return;
    };
    let owner = factory.owner;
    match rules {
        Some(rules) => abandon_production(
            sim,
            rules,
            FactoryHolder::Building(building),
            owner,
            effects,
        ),
        None => {
            let holder = FactoryHolder::Building(building);
            let object = sim
                .production
                .factories()
                .pending_abandonment(holder)
                .and_then(|abandoned| abandoned.entity_id);
            sim.production
                .factories_mut()
                .reset_for_abandon(holder, sim.session.binary_frame());
            sim.with_object_placement_scope(|sim| {
                if let Some(object) = object {
                    let _ = sim.discard_constructed_limbo(object, rules, effects);
                }
                sim.production.factories_mut().finish_abandon(holder);
            });
            sim.production.factories_mut().remove_factory(holder);
        }
    }
}

#[cfg(test)]
#[path = "factory_ai_tests.rs"]
mod tests;
