//! Unit-level AI decisions (control-point list MIS-S2 and MIS-S8; RFC 0001
//! appendix 4.5, tier preview): the mission an object idles into and the ore
//! cell a harvester heads for. Both are decision-only, bound to the object
//! or its type, and ask nothing without a decider
//! (`TriggerRuntime::module_decides`).
//!
//! - `IdleMission` → a `Mission` number, 255 to queue nothing: at the queue
//!   of `Enter_Idle_Mode` once the class receiver has picked (Infantry
//!   `0x0051CD75..0x0051CD9D`, `mission_handlers::infantry_enter_idle_mode`;
//!   Unit `0x00738CFA..0x00738D12`, `Simulation::unit_enter_idle_mode`), with
//!   the receiver's own setters already done, when it queues a mission.
//!   `native` is that mission; the answer replaces what is queued (255
//!   queues nothing). A receiver that queues nothing does not ask, nor do
//!   aircraft and buildings.
//! - `HarvestCell` → a cell reference, 0 for none: in
//!   `FootClass::Search_For_Tiberium_And_Move @ 0x004DCFE0`
//!   (`miner::ore_scan::search_for_tiberium_and_move`), after the native scan,
//!   before the destination is set. `native` is the scan's cell (0 for a
//!   miss); `value` the scan range in cells. An answer naming a cell off
//!   the map keeps the engine's cell. The answer's own cell reports "on ore" as the scan's
//!   does. The slave miners' scans (`slave_manager`) do not ask.
//!
//! Residuals: the Guard, Area Guard, Hunt and Attack mission bodies
//! (MIS-S3..S6) have no slot of their own yet. Their computer-only arms yield
//! to the house domain claims (`ext_modules::claims`); a module steering a
//! unit's mission otherwise orders it from `on_frame`.

use vera20k_wasm_ext::decide::{EVENT_HARVEST_CELL, EVENT_IDLE_MISSION};

use super::{ModuleEvent, Ref, cell_ref, decode_ref};
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

    /// `HarvestCell` for harvester `id` (module docs): the cell its search
    /// settles on, `None` for none; `native` without a decider.
    pub(crate) fn module_harvest_cell(
        &mut self,
        rules: &RuleSet,
        id: u64,
        range: i32,
        native: Option<(u16, u16)>,
    ) -> Option<(u16, u16)> {
        if !self.trigger_runtime.module_decides(EVENT_HARVEST_CELL) {
            return native;
        }
        let event = ModuleEvent {
            cell: native,
            value: i64::from(range),
            ..ModuleEvent::new(EVENT_HARVEST_CELL, id)
        };
        let value = native.map_or(0, |(x, y)| cell_ref(x, y));
        let cell_of = |answer: i64| match decode_ref(answer) {
            Ok(Some(Ref::Cell { x, y })) => Some((x, y)),
            _ => None,
        };
        let accept = |answer: i64| (answer == 0 || cell_of(answer).is_some()).then_some(answer);
        match self.decide_module_event(rules, &event, None, None, value, accept) {
            None => native,
            Some(0) => None,
            // A cell off the map keeps the engine's.
            Some(answer) => match cell_of(answer).filter(|&(x, y)| {
                self.resolved_terrain
                    .as_ref()
                    .is_some_and(|terrain| terrain.cell(x, y).is_some())
            }) {
                Some(cell) => Some(cell),
                None => native,
            },
        }
    }
}
