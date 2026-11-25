//! Unit-level AI decisions (control-point list MIS-S2..S6; RFC 0001
//! appendix 4.5, tier preview): the mission an object idles into, and the
//! outcome of its Guard, Area Guard, Hunt and Attack mission bodies.
//! Decision-only, bound to the object or its type, and asks nothing without
//! a decider (`TriggerRuntime::module_decides`). The ore cell a harvester
//! heads for (MIS-S8) is `HarvestCell`, asked in every scan
//! (`ext_modules::control`).
//!
//! - `IdleMission` → a `Mission` number, 255 to queue nothing: at the queue
//!   of `Enter_Idle_Mode` once the class receiver has picked (Infantry
//!   `0x0051CD75..0x0051CD9D`, `mission_handlers::infantry_enter_idle_mode`;
//!   Unit `0x00738CFA..0x00738D12`, `Simulation::unit_enter_idle_mode`), with
//!   the receiver's own setters already done, when it queues a mission.
//!   `native` is that mission; the answer replaces what is queued (255
//!   queues nothing). A receiver that queues nothing does not ask, nor do
//!   aircraft and buildings.
//! - `MissionStep` → a `MissionStepResult` in the result area (`result`
//!   unused): at the commit of an infantryman's or vehicle's mission handler
//!   (`mission_handlers::dispatch_foot_mission`, `MissionClass::AI @
//!   0x005B3060`'s queue and timer write after the handler returns) when the
//!   committed mission is Guard or Sticky (MIS-S3; Foot `0x004D5070`, Unit
//!   `0x00740810`, Infantry `0x0051F620`), Area Guard (MIS-S4; Foot
//!   `0x004D6AA0`, Unit `0x00744100`, Infantry `0x0051F640`), Hunt (MIS-S5;
//!   Foot `0x004D5350`, Unit `0x0073EFC0`, Infantry `0x0051F540`) or Attack
//!   (MIS-S6; Foot `0x004D4DC0`, Unit `0x007447A0`, Infantry `0x0051F3E0`).
//!   `value` is that mission, `other` and `cell` the target's object and
//!   cell. The area goes in holding the body's outcome: the target (TarCom)
//!   and destination (NavCom) it left, the mission its handler return queues
//!   (−1 for none) and its delay. Each field an answer changes is applied
//!   with the body's own tools: the target through the class setter
//!   `Assign_Target`, the destination through `Assign_Destination` (which
//!   asks `DestinationSet`), the mission through `Queue_Mission` and the
//!   delay as the mission timer. A target or destination naming no live
//!   object or an off-map cell, a mission that is not a `Mission` number and
//!   a delay below 1 keep the body's. The question comes after the body, so
//!   the body's scenario draws (its Rate epilogue, the scans' timers, the
//!   computer's disguise test) all happen as without a module, and only
//!   their outcome is replaced (control-point list 4.4).
//!
//! Residuals:
//! - `MissionStep` replaces the four outcomes, not the rest of what the body
//!   did this call: a deployed GI's shot at its own cell, an infantryman's
//!   automatic deploy or undeploy, a Slave Miner's kick, an MCV's Hunt
//!   deploy, a C4 or computer capture approach, the Attack body's own
//!   Approach toward its target (the destination it set is in the area) and
//!   a mission the body queued through going idle (`IdleMission` decides
//!   that one; `mission` holds only what the handler return queues, so −1
//!   cannot unqueue it).
//! - A target the answer installs is approached from the next Attack or
//!   Hunt call on, not this one, unless the answer also sets the
//!   destination. The passive acquisition after the handler (Guard only)
//!   may still replace a target the answer cleared.
//! - Aircraft and buildings ask nothing: their bodies have their own
//!   dispatchers (`aircraft::dispatch_mission`, `building_missions`).

use vera20k_wasm_ext::decide::{EVENT_IDLE_MISSION, EVENT_MISSION_STEP, MissionStepResult};

use super::control::{destination_ref, target_fields};
use super::{ModuleEvent, Ref, cell_ref, decode_ref};
use crate::rules::mission_data::MissionType;
use crate::rules::ruleset::RuleSet;
use crate::sim::combat::TargetKind;
use crate::sim::components::NavTargetRef;
use crate::sim::world::{FrameEffects, Simulation};

/// `Mission` "none" (appendix 8.3).
const NO_MISSION: i64 = 255;

impl Simulation {
    /// `IdleMission` for `id` (module docs): the mission to queue, `None`
    /// for nothing; `native` without a decider.
    pub(crate) fn module_idle_mission(
        &mut self,
        rules: &RuleSet,
        id: u64,
        native: Option<MissionType>,
    ) -> Option<MissionType> {
        if !self.trigger_runtime.module_decides(EVENT_IDLE_MISSION) {
            return native;
        }
        let event = ModuleEvent::new(EVENT_IDLE_MISSION, id);
        let value = native.map_or(NO_MISSION, |mission| i64::from(mission.id()));
        let accept = |answer: i64| match answer {
            NO_MISSION => Some(answer),
            answer => u8::try_from(answer)
                .ok()
                .and_then(MissionType::from_id)
                .map(|_| answer),
        };
        match self.decide_module_event(rules, &event, None, None, value, accept) {
            None => native,
            Some(NO_MISSION) => None,
            Some(answer) => u8::try_from(answer).ok().and_then(MissionType::from_id),
        }
    }

    /// `MissionStep` for `id`, whose `mission` body just returned `delay`
    /// and `queue` (module docs): applies the answer's target and
    /// destination and returns the mission to queue and the delay. Without
    /// a decider or an answer, `queue` and `delay`.
    pub(crate) fn module_mission_step(
        &mut self,
        rules: &RuleSet,
        id: u64,
        mission: MissionType,
        queue: Option<MissionType>,
        delay: i32,
        effects: &mut FrameEffects<'_>,
    ) -> (Option<MissionType>, i32) {
        if !self.trigger_runtime.module_decides(EVENT_MISSION_STEP) {
            return (queue, delay);
        }
        let Some(entity) = self.entities().get(id) else {
            return (queue, delay);
        };
        let (target, destination) = (entity.attack_target.target(), entity.navigation.nav_com());
        let native = MissionStepResult {
            target: match target {
                None => 0,
                Some(TargetKind::Entity(object)) => object as i64,
                Some(TargetKind::Cell(x, y)) => cell_ref(x, y),
            },
            destination: destination.map_or(0, destination_ref),
            mission: queue.map_or(MissionStepResult::NO_MISSION, |queued| {
                i32::from(queued.id())
            }),
            delay,
        };
        let (other, cell) = target_fields(self, target);
        let event = ModuleEvent {
            other,
            cell,
            value: i64::from(mission.id()),
            ..ModuleEvent::new(EVENT_MISSION_STEP, id)
        };
        let mut area = native.to_bytes().to_vec();
        let answer = self
            .decide_module_area(rules, &event, 0, &mut area)
            .and_then(|_| MissionStepResult::from_bytes(&area));
        let Some(answer) = answer else {
            return (queue, delay);
        };
        if answer.target != native.target
            && let Some(requested) = self.module_step_reference(answer.target)
        {
            let requested = requested.map(|at| match at {
                NavTargetRef::Object { id: object } => TargetKind::Entity(object),
                NavTargetRef::Cell { rx, ry } => TargetKind::Cell(rx, ry),
            });
            let _ = self.assign_target_represented(id, requested, rules, effects);
        }
        if answer.destination != native.destination
            && let Some(requested) = self.module_step_reference(answer.destination)
        {
            let _ = self.assign_destination_represented(id, requested, rules, effects);
        }
        let queue = if answer.mission == native.mission {
            queue
        } else if answer.mission == MissionStepResult::NO_MISSION {
            None
        } else {
            match u8::try_from(answer.mission)
                .ok()
                .and_then(MissionType::from_id)
            {
                Some(MissionType::None) => None,
                Some(answer) => Some(answer),
                None => queue,
            }
        };
        let delay = if answer.delay >= 1 {
            answer.delay
        } else {
            delay
        };
        (queue, delay)
    }

    /// A `MissionStepResult` target or destination: `Some(None)` for 0,
    /// `Some(Some(_))` for a live object or an on-map cell, `None` for
    /// anything else (the body's stands).
    fn module_step_reference(&self, reference: i64) -> Option<Option<NavTargetRef>> {
        match decode_ref(reference).ok()? {
            None => Some(None),
            Some(Ref::Object(object)) if self.entities().get(object).is_some() => {
                Some(Some(NavTargetRef::object(object)))
            }
            Some(Ref::Cell { x, y }) if self.map_cell_in_bounds((x as i16, y as i16)) => {
                Some(Some(NavTargetRef::cell(x, y)))
            }
            _ => None,
        }
    }
}
