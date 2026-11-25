//! Unit-level AI decisions (control-point list MIS-S2; RFC 0001 appendix
//! 4.5, tier preview): the mission an object idles into. Decision-only,
//! bound to the object or its type, and asks nothing without a decider
//! (`TriggerRuntime::module_decides`). The ore cell a harvester heads for
//! (MIS-S8) is `HarvestCell`, asked in every scan (`ext_modules::control`).
//!
//! - `IdleMission` → a `Mission` number, 255 to queue nothing: at the queue
//!   of `Enter_Idle_Mode` once the class receiver has picked (Infantry
//!   `0x0051CD75..0x0051CD9D`, `mission_handlers::infantry_enter_idle_mode`;
//!   Unit `0x00738CFA..0x00738D12`, `Simulation::unit_enter_idle_mode`), with
//!   the receiver's own setters already done, when it queues a mission.
//!   `native` is that mission; the answer replaces what is queued (255
//!   queues nothing). A receiver that queues nothing does not ask, nor do
//!   aircraft and buildings.
//!
//! Residuals: the Guard, Area Guard, Hunt and Attack mission bodies
//! (MIS-S3..S6) have no slot of their own yet. Their computer-only arms yield
//! to the house domain claims (`ext_modules::claims`); a module steering a
//! unit's mission otherwise orders it from `on_frame`.

use vera20k_wasm_ext::decide::EVENT_IDLE_MISSION;

use super::ModuleEvent;
use crate::rules::mission_data::MissionType;
use crate::rules::ruleset::RuleSet;
use crate::sim::world::Simulation;

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
}

