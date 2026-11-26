//! The event-type control points of RFC 0001 0.0.22 (appendix 4.3 and
//! 4.5): cheap decision points and delivered events on a unit's targeting,
//! firing, retaliation, movement, crushing, harvesting, deployment,
//! veterancy and cloak. Each entry checks `module_decides` or
//! `module_listens` first and builds nothing without a listener (RFC 15.1).
//! None keeps state across frames: every decision completes at its commit
//! point and every delivered event is raised there.
//!
//! Decision points (decision-only, tier `preview`; 1 allow, 0 refuse unless
//! noted; asked only at commit points, never in the parallel target scan
//! `scan_batch` or the path batch `path_batch`):
//! - `FireAllowed` (veto): the fire routines' GetFireError ask
//!   (`combat::world_fire`, Unit `0x00736E3A`, Infantry `0x005206F3`,
//!   Aircraft `0x0041832E`, a building's admitted shot) once it answered OK;
//!   a refusal answers Rearm, as `DisableWeapons` does. `other` the target
//!   object, `cell` the target cell, `value` the weapon slot, `id` the
//!   weapon.
//! - `Retaliate`: TechnoClass::ReceiveDamage's retaliation tail
//!   (`0x00702A43`), after ShouldRetaliate answered, for a hit with a
//!   source; `native` that answer, `other` the source, `id` the warhead. A
//!   1 still needs the source in reach to take Override_Mission; either
//!   answer then reaches the scatter tail with it.
//! - `DestinationSet`: the represented class setter `Assign_Destination`
//!   (vt+0x480; `mission::concrete_effects`, Unit `0x00741970`, Infantry
//!   `0x0051AA40`, Aircraft `0x0041AA80`, Building) for a non-null
//!   destination. The result is a reference: the native destination (an
//!   object or cell, also in `other` or `cell`), another object or cell, or
//!   0 for none (the NULL destination). A reference to no live object or to
//!   an off-map cell, or one the setter's admission refuses, keeps the
//!   native destination.
//! - `CrushAllowed` (veto): Crush_Or_Prescatter_Cell (`0x007416A0`) for each
//!   occupant the crusher would crush (`bump_crush::crushes`), on the
//!   victim, `other` the crusher. A refusal leaves that occupant alive in
//!   the cell, as a commit-time `crushes` refusal does. Can_Enter_Cell's
//!   crush codes 4 and 5 (`0x0073F0A0`, a path query the RFC keeps free of
//!   module calls) are not asked, so the crusher still drives in.
//! - `HarvestCell`: `Scan_For_Tiberium` (vt+0x338, `miner::ore_scan`) for a
//!   miner or slave manager: the result is the cell to harvest, a cell
//!   reference (`native` the scanned cell's, 0 for none) or 0 for none; an
//!   off-map cell keeps the native answer.
//! - `DeployAllowed` (veto): before UnitClass::Deploy's transaction
//!   (`0x007393C0`, after its placement test), at a simple deployer's
//!   deploy admission (`0x00739AC0`) and before every infantryman's
//!   Do_Action(Deploy) (`deploy::infantry_request_deploy`: the pending
//!   deploy `0x00521B40`, Mission_Unload `0x0051F6E0`, the computer's
//!   automatic deploy `0x00521631`, the Approach arm `0x00522472`). A
//!   refusal leaves the object as a refused deploy does: a DeploysInto unit
//!   queues Guard (without the EVA), a simple deployer's or an
//!   infantryman's Unload ends in Guard without deploying, the others stay
//!   as they stand. Undeploys are not asked.
//!
//! Delivered events (tier `v1`):
//! - `TargetChanged`: after the represented class setter `Assign_Target`
//!   (`0x006FCDB0`'s store at `0x006FCF3E`, `mission::concrete_effects`)
//!   changed TarCom, and after the bare writes with the world at hand
//!   (PointerExpired's clear, the arrival handler, a Temporal firer, a
//!   drainer, a parasite, a paradrop plane giving up). `other` the new
//!   target object, `cell` its cell; both 0 when cleared.
//! - `Deployed`: `value` 1 deployed, 0 undeployed; `other` the object it
//!   became, 0 when it stays itself.
//! - `Promoted`: `value` the new rank (1 veteran, 2 elite), at the rank
//!   sample (`0x006FA054`, Ares `1004F2E0`).
//! - `ExperienceGained`: on each recipient of a kill's experience, `other`
//!   the victim, `value` the gain in hundredths of a rank (`combat::
//!   record_kill`).
//! - `Cloaked` and `Decloaked`: when a cloak completes (state 1 → 2,
//!   `0x006FBA98`; a building's fade stage 15, `0x00454E7F`) and when an
//!   uncloak does (3 → 0; fade stage 0).
//!
//! Residuals:
//! - `MissionChanged` (100048) has no raise point yet: Commence, Assign,
//!   Override and Restore write the entity's MissionClass with neither the
//!   rules nor the frame effects at hand, from a score of callers.
//! - `TargetChanged` is not raised for the target a newly constructed object
//!   starts with (a survivor's planter, a paradrop plane's cell).
//! - `DeployAllowed` and `Deployed` do not cover Ares' convert-on-deploy
//!   arms beyond the simple deployer's.

use vera20k_wasm_ext::decide::{
    EVENT_CRUSH_ALLOWED, EVENT_DEPLOY_ALLOWED, EVENT_DESTINATION_SET, EVENT_FIRE_ALLOWED,
    EVENT_HARVEST_CELL, EVENT_RETALIATE,
};
use vera20k_wasm_ext::triggers::{
    EVENT_CLOAKED, EVENT_DECLOAKED, EVENT_DEPLOYED, EVENT_PROMOTED, EVENT_TARGET_CHANGED,
};

use super::decide::allow;
use super::{ModuleEvent, Ref, cell_ref, decode_ref};
use crate::rules::ruleset::RuleSet;
use crate::sim::combat::TargetKind;
use crate::sim::components::NavTargetRef;
use crate::sim::world::{FrameEffects, Simulation};

/// A reference to `target`'s object or cell, for `other` and `cell`.
pub(super) fn target_fields(sim: &Simulation, target: Option<TargetKind>) -> (u64, Option<(u16, u16)>) {
    match target {
        None => (0, None),
        Some(TargetKind::Cell(x, y)) => (0, Some((x, y))),
        Some(TargetKind::Entity(id)) => (
            id,
            sim.entities()
                .get(id)
                .map(|entity| (entity.position.rx(), entity.position.ry())),
        ),
    }
}

/// A NavCom destination as a module reference.
pub(super) fn destination_ref(destination: NavTargetRef) -> i64 {
    match destination {
        NavTargetRef::Cell { rx, ry } => cell_ref(rx, ry),
        NavTargetRef::Object { id } => id as i64,
    }
}

/// An object or cell reference; anything else is not an answer.
fn object_or_cell(answer: i64) -> Option<i64> {
    match decode_ref(answer) {
        Ok(None | Some(Ref::Object(_) | Ref::Cell { .. })) => Some(answer),
        _ => None,
    }
}

/// A cell reference or 0; anything else is not an answer.
fn cell_or_none(answer: i64) -> Option<i64> {
    match decode_ref(answer) {
        Ok(None | Some(Ref::Cell { .. })) => Some(answer),
        _ => None,
    }
}

impl Simulation {
    /// `FireAllowed` for `shooter` about to fire weapon slot `slot` (weapon
    /// ID `weapon`, −1 unknown) at `target` (module docs): false when a
    /// decider refuses.
    pub(crate) fn module_allows_fire(
        &mut self,
        rules: &RuleSet,
        shooter: u64,
        target: TargetKind,
        slot: i32,
        weapon: Option<&str>,
    ) -> bool {
        if !self.trigger_runtime.module_decides(EVENT_FIRE_ALLOWED) {
            return true;
        }
        let (other, cell) = target_fields(self, Some(target));
        let event = ModuleEvent {
            other,
            cell,
            value: i64::from(slot),
            id: weapon
                .and_then(|name| rules.weapon_index(name))
                .map_or(-1, |index| index as i32),
            ..ModuleEvent::new(EVENT_FIRE_ALLOWED, shooter)
        };
        self.decide_module_event(rules, &event, None, None, 1, allow) != Some(0)
    }

    /// `Retaliate` for `victim` hit by `source` with warhead ID `warhead`
    /// (module docs): the native ShouldRetaliate answer `native`, or a
    /// decider's.
    pub(crate) fn module_retaliates(
        &mut self,
        rules: &RuleSet,
        victim: u64,
        source: u64,
        warhead: crate::sim::intern::InternedId,
        native: bool,
    ) -> bool {
        if !self.trigger_runtime.module_decides(EVENT_RETALIATE) {
            return native;
        }
        let event = ModuleEvent {
            other: source,
            id: rules
                .warhead_index(self.interner.resolve(warhead))
                .map_or(-1, |index| index as i32),
            ..ModuleEvent::new(EVENT_RETALIATE, victim)
        };
        self.decide_module_event(rules, &event, None, None, i64::from(native), allow)
            .map_or(native, |answer| answer == 1)
    }

    /// `DestinationSet` for `subject` handed the non-null `native`
    /// destination (module docs): `None` keeps it, `Some(None)` asks for no
    /// destination, `Some(Some(_))` replaces it with a live object or an
    /// on-map cell. The setter's own admission of a replacement is the
    /// caller's.
    pub(crate) fn module_destination(
        &mut self,
        rules: &RuleSet,
        subject: u64,
        native: NavTargetRef,
    ) -> Option<Option<NavTargetRef>> {
        if !self.trigger_runtime.module_decides(EVENT_DESTINATION_SET) {
            return None;
        }
        let native_ref = destination_ref(native);
        let (other, cell) = match native {
            NavTargetRef::Cell { rx, ry } => (0, Some((rx, ry))),
            NavTargetRef::Object { id } => target_fields(self, Some(TargetKind::Entity(id))),
        };
        let event = ModuleEvent {
            other,
            cell,
            value: native_ref,
            ..ModuleEvent::new(EVENT_DESTINATION_SET, subject)
        };
        let answer =
            self.decide_module_event(rules, &event, None, None, native_ref, object_or_cell)?;
        if answer == native_ref {
            return None;
        }
        match decode_ref(answer).ok()? {
            None => Some(None),
            Some(Ref::Object(id)) if self.entities().get(id).is_some() => {
                Some(Some(NavTargetRef::object(id)))
            }
            Some(Ref::Cell { x, y }) if self.map_cell_in_bounds((x as i16, y as i16)) => {
                Some(Some(NavTargetRef::cell(x, y)))
            }
            _ => None,
        }
    }

    /// `CrushAllowed` for `crusher` about to crush `victim` (module docs):
    /// false when a decider refuses.
    pub(crate) fn module_allows_crush(
        &mut self,
        rules: &RuleSet,
        crusher: u64,
        victim: u64,
    ) -> bool {
        if !self.trigger_runtime.module_decides(EVENT_CRUSH_ALLOWED) {
            return true;
        }
        let event = ModuleEvent {
            other: crusher,
            ..ModuleEvent::new(EVENT_CRUSH_ALLOWED, victim)
        };
        self.decide_module_event(rules, &event, None, None, 1, allow) != Some(0)
    }

    /// `HarvestCell` for `harvester`, whose scan answered `native` (module
    /// docs): the cell to harvest, or `None`.
    pub(crate) fn module_harvest_cell(
        &mut self,
        rules: &RuleSet,
        harvester: u64,
        native: Option<(u16, u16)>,
    ) -> Option<(u16, u16)> {
        if !self.trigger_runtime.module_decides(EVENT_HARVEST_CELL) {
            return native;
        }
        let native_ref = native.map_or(0, |(x, y)| cell_ref(x, y));
        let event = ModuleEvent {
            cell: native,
            value: native_ref,
            ..ModuleEvent::new(EVENT_HARVEST_CELL, harvester)
        };
        let Some(answer) =
            self.decide_module_event(rules, &event, None, None, native_ref, cell_or_none)
        else {
            return native;
        };
        match decode_ref(answer) {
            Ok(None) => None,
            Ok(Some(Ref::Cell { x, y })) if self.map_cell_in_bounds((x as i16, y as i16)) => {
                Some((x, y))
            }
            _ => native,
        }
    }

    /// `DeployAllowed` for `deployer` (module docs): false when a decider
    /// refuses.
    pub(crate) fn module_allows_deploy(&mut self, rules: &RuleSet, deployer: u64) -> bool {
        if !self.trigger_runtime.module_decides(EVENT_DEPLOY_ALLOWED) {
            return true;
        }
        let event = ModuleEvent::new(EVENT_DEPLOY_ALLOWED, deployer);
        self.decide_module_event(rules, &event, None, None, 1, allow) != Some(0)
    }

    /// `TargetChanged` on `subject` when its TarCom no longer holds `before`
    /// (module docs).
    pub(crate) fn raise_module_target_changed(
        &mut self,
        rules: &RuleSet,
        frame_effects: &mut FrameEffects<'_>,
        subject: u64,
        before: Option<TargetKind>,
    ) {
        if !self.trigger_runtime.module_listens(EVENT_TARGET_CHANGED) {
            return;
        }
        let Some(now) = self
            .entities()
            .get(subject)
            .map(|entity| entity.attack_target.target())
        else {
            return;
        };
        if now == before {
            return;
        }
        let (other, cell) = target_fields(self, now);
        let event = ModuleEvent {
            other,
            cell,
            ..ModuleEvent::new(EVENT_TARGET_CHANGED, subject)
        };
        self.raise_module_event(rules, frame_effects, event);
    }

    /// The TarCom of `subject` now, for [`Self::raise_module_target_changed`]
    /// after a write; `None` both without the object and without a target.
    pub(crate) fn module_target_before(&self, subject: u64) -> Option<TargetKind> {
        if !self.trigger_runtime.module_listens(EVENT_TARGET_CHANGED) {
            return None;
        }
        self.entities()
            .get(subject)
            .and_then(|entity| entity.attack_target.target())
    }

    /// `Deployed` on `subject` (module docs): `deployed` false for an
    /// undeploy; `became` the object it turned into.
    pub(crate) fn raise_module_deployed(
        &mut self,
        rules: &RuleSet,
        frame_effects: &mut FrameEffects<'_>,
        subject: u64,
        became: Option<u64>,
        deployed: bool,
    ) {
        if !self.trigger_runtime.module_listens(EVENT_DEPLOYED) {
            return;
        }
        let event = ModuleEvent {
            other: became.unwrap_or(0),
            value: i64::from(deployed),
            ..ModuleEvent::new(EVENT_DEPLOYED, subject)
        };
        self.raise_module_event(rules, frame_effects, event);
    }

    /// `Promoted` on `subject`, `rank` 1 veteran or 2 elite.
    pub(crate) fn raise_module_promoted(
        &mut self,
        rules: &RuleSet,
        frame_effects: &mut FrameEffects<'_>,
        subject: u64,
        rank: i64,
    ) {
        if !self.trigger_runtime.module_listens(EVENT_PROMOTED) {
            return;
        }
        let event = ModuleEvent {
            value: rank,
            ..ModuleEvent::new(EVENT_PROMOTED, subject)
        };
        self.raise_module_event(rules, frame_effects, event);
    }

    /// `Cloaked` (`cloaked`) or `Decloaked` on `subject`.
    pub(crate) fn raise_module_cloak_edge(
        &mut self,
        rules: &RuleSet,
        frame_effects: &mut FrameEffects<'_>,
        subject: u64,
        cloaked: bool,
    ) {
        let kind = if cloaked {
            EVENT_CLOAKED
        } else {
            EVENT_DECLOAKED
        };
        if !self.trigger_runtime.module_listens(kind) {
            return;
        }
        self.raise_module_event(rules, frame_effects, ModuleEvent::new(kind, subject));
    }
}
