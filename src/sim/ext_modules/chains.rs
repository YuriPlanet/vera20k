//! Chain bounds for module triggers (RFC 0001 section 5, "Chains"; open
//! question 17).
//!
//! A module trigger's immediate actions raise events like any other code,
//! and those events fire module triggers again: a `TakeDamage` trigger that
//! deals damage raises `TakeDamage`. Such chained firings, the ones raised
//! while a module trigger's actions run, are bounded per frame:
//! - by depth: a firing [`MAX_CHAIN_DEPTH`] module-trigger firings deep
//!   waits;
//! - by count: after [`MAX_CHAIN_FIRINGS`] chained firings in one frame,
//!   further chained firings wait.
//!
//! A firing that waits keeps its record as raised and fires at the start of
//! the next frame's module phase, before `Timer` and queued records, as a
//! new chain: the trigger must still exist and be enabled, its filter
//! clauses are read again, and a trigger without a house takes the
//! subject's current owner. Its actions on a vanished subject answer
//! `Gone`. Waiting firings are saved and hashed with the trigger registry.
//! A module whose triggers have more than [`MAX_DEFERRED`] firings waiting
//! is stopped as unable to keep up (`Fault::Backlog`), so a chain that
//! fans out cannot grow without bound.
//!
//! Firings raised by engine events (depth 0), by module code's own
//! `action` calls and by deferred firings themselves count against neither
//! cap: those are bounded by the engine's own events. Chains made only of
//! stock events and stock actions on map triggers are the trigger runtime's
//! and run as in the original game.
//!
//! The numbers are this engine's proposal for open question 17.

use vera20k_wasm_ext::{EventRecord, Export, Fault};

use super::ModuleEvent;
use crate::rules::ruleset::RuleSet;
use crate::sim::trigger_runtime::DeferredFiring;
use crate::sim::world::{FrameEffects, Simulation};

/// Module-trigger firings nested inside one chain before the next waits.
pub const MAX_CHAIN_DEPTH: u32 = 8;
/// Chained module-trigger firings per frame before the rest wait.
pub const MAX_CHAIN_FIRINGS: u32 = 256;
/// Waiting firings one module's triggers may have before it is stopped.
pub const MAX_DEFERRED: usize = 4096;

/// The chain on the stack. Not saved: it is empty between frames.
#[derive(Debug, Default)]
pub(super) struct ChainState {
    /// Module-trigger action runs on the stack.
    pub(super) depth: u32,
    /// The frame `firings` counts.
    frame: u32,
    firings: u32,
}

impl ChainState {
    /// Whether a firing raised now runs at once; a chained one is counted.
    pub(super) fn admit(&mut self, frame: u32) -> bool {
        if self.depth == 0 {
            return true;
        }
        if self.frame != frame {
            self.frame = frame;
            self.firings = 0;
        }
        if self.depth >= MAX_CHAIN_DEPTH || self.firings >= MAX_CHAIN_FIRINGS {
            return false;
        }
        self.firings += 1;
        true
    }
}

impl Simulation {
    /// Keeps a firing past the caps for the next module phase. One past
    /// [`MAX_DEFERRED`] is kept so the phase sees the overflow; more are
    /// dropped, the module being stopped anyway.
    pub(super) fn defer_module_firing(&mut self, trigger: u64, holder: u64, record: EventRecord) {
        let triggers = self.trigger_runtime.module_triggers_mut();
        let Some(source) = triggers.get(trigger).map(|t| t.source) else {
            return;
        };
        if triggers.deferred_of(source) > MAX_DEFERRED {
            return;
        }
        log::debug!(target: "ext_module", "trigger {trigger} chain capped; fires next frame");
        triggers.defer(DeferredFiring {
            trigger,
            holder,
            record,
        });
    }

    /// The start of the module phase: stops a module with too many waiting
    /// firings, then fires the others in raise order.
    pub(super) fn fire_deferred_module_triggers(
        &mut self,
        rules: &RuleSet,
        frame_effects: &mut FrameEffects<'_>,
    ) {
        for index in 0..self.ext_modules.slots.len() {
            let source = self.ext_modules.slots[index].source;
            let waiting = self.trigger_runtime.module_triggers().deferred_of(source);
            if waiting > MAX_DEFERRED && self.ext_modules.slots[index].disabled.is_none() {
                self.disable_ext_module(index, Export::Events, Fault::Backlog(waiting));
            }
        }
        let difficulty = self.trigger_difficulty();
        for firing in self.trigger_runtime.module_triggers_mut().take_deferred() {
            let Some(trigger) = self.trigger_runtime.module_triggers().get(firing.trigger) else {
                continue;
            };
            if !trigger.enabled || !trigger.admits_difficulty(difficulty) {
                continue;
            }
            let (source, fixed_house) = (trigger.source, trigger.house);
            let clauses: Vec<_> = trigger
                .events
                .iter()
                .filter(|def| def.kind == firing.record.kind)
                .map(|def| def.clauses.clone())
                .collect();
            let record = firing.record;
            let event = ModuleEvent {
                other: u64::try_from(record.other).unwrap_or(0),
                ..ModuleEvent::new(record.kind, u64::try_from(record.subject).unwrap_or(0))
            };
            let house = fixed_house.or_else(|| self.module_trigger_house(rules, event.subject));
            let holds = clauses
                .iter()
                .any(|clauses| self.module_clauses_hold(rules, source, clauses, &event, house));
            if holds {
                self.run_module_trigger_actions(
                    rules,
                    frame_effects,
                    firing.trigger,
                    record,
                    house,
                    house,
                );
            }
        }
    }
}
