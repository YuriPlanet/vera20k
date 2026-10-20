//! Entity-local access to the one native building body, StageClass and ready
//! byte. World scheduling/effects use these owner APIs without writable views.

use super::GameEntity;
#[cfg(any(test, feature = "test-support"))]
use crate::sim::building_construction::BuildingUp;
use crate::sim::building_construction::{BuildingBodyMode, BuildingDown, PackUpFrame};
use crate::sim::game_options::GameOptions;
#[cfg(any(test, feature = "test-support"))]
use crate::sim::mission::MissionId;
use crate::sim::mission::MissionType;

impl GameEntity {
    pub(crate) fn bind_building_construction_control(&mut self, control: [i32; 3]) {
        self.building_body
            .as_mut()
            .expect("building body requires Structure")
            .bind_control(control);
    }
    #[cfg(any(test, feature = "test-support"))]
    pub fn install_building_up(&mut self, entry: BuildingUp, now: i32) {
        entry.install(self, now);
    }
    /// Construction is the effective native mission, including a queued entry.
    /// `MissionClass::Get_Mission @ 0x005B3040`: `+0xAC`, else queued `+0xB4`.
    pub fn building_up(&self) -> bool {
        self.category == crate::map::entities::EntityCategory::Structure
            && self.mission.effective().known() == Some(MissionType::Construction)
    }
    /// Selling is `MissionClass::Get_Mission @ 0x005B3040`'s answer: `+0xAC`,
    /// else queued `+0xB4`.
    pub fn building_down(&self) -> bool {
        self.category == crate::map::entities::EntityCategory::Structure
            && self.mission.effective().known() == Some(MissionType::Selling)
    }
    pub(crate) fn install_building_down(&mut self, route: BuildingDown) {
        self.building_sale = Some(route);
    }
    pub(crate) fn sale_is_undeploy_order(&self) -> bool {
        self.building_down() && self.building_sale.is_some_and(BuildingDown::undeploy_order)
    }
    /// `BuildingClass::Mission_Selling @ 0x00449C30`'s stage visit: its `+0x6DD`
    /// read (`0x00449C99`) and stage 0's clear (`0x0044AB61`).
    pub(crate) fn advance_building_down(&mut self, status: &mut u32) -> PackUpFrame {
        let ready = self.building_ready_latch() != 0;
        let has_upgrades = self.building_upgrades().count() != 0;
        let visit = self
            .building_sale
            .unwrap_or_else(|| BuildingDown::commenced(0, false))
            .visit(status, ready, has_upgrades);
        if visit == PackUpFrame::StageZero && !has_upgrades {
            self.mission_leaf.set_building_ready_latch(0);
        }
        visit
    }
    /// `Mission_Selling`'s stage-1 tail (`0x0044A8A2..0x0044A8B5`): status 2,
    /// `Begin_Mode(0)` (`0x00447780`), then `+0x6DD = 0`.
    pub(crate) fn begin_building_pack_up_stage_two(&mut self, status: &mut u32, now: i32) {
        *status = 2;
        self.begin_building_body(BuildingBodyMode::Construction, now);
        self.mission_leaf.set_building_ready_latch(0);
    }
    pub(crate) fn begin_building_body(&mut self, mode: BuildingBodyMode, now: i32) {
        self.building_body
            .as_mut()
            .expect("building body requires Structure")
            .begin(mode, &mut self.stage, now);
    }
    pub(crate) fn initialize_building_idle_body(&mut self, now: i32) {
        self.building_body
            .as_mut()
            .expect("building body requires Structure")
            .initialize_idle(&mut self.stage, now);
    }
    /// `BuildingClass::UpdateAnimation @ 0x004509D0`'s stage step and ready
    /// admission, which sets `+0x6DD` (`0x004511DF`, `0x00451218`).
    pub(crate) fn advance_building_body(
        &mut self,
        now: i32,
        archive_less_sale: bool,
        has_turret: bool,
        options: &GameOptions,
    ) {
        let ready_allowed = !has_turret || self.constructing_or_selling();
        if self.building_body.as_ref().is_some_and(|body| {
            body.update(
                &mut self.stage,
                now,
                archive_less_sale,
                ready_allowed,
                options,
            )
        }) {
            self.mission_leaf.set_building_ready_latch(1);
        }
    }
    pub(crate) fn apply_queued_building_body(&mut self, now: i32, options: &GameOptions) {
        if let Some(body) = self.building_body.as_mut() {
            body.apply_queued(&mut self.stage, now, options);
        }
    }
    pub fn building_body_state(&self) -> Option<i32> {
        self.building_body.as_ref().map(|body| body.state())
    }
    pub fn queued_building_body_state(&self) -> Option<i32> {
        self.building_body.as_ref().map(|body| body.queued())
    }
    pub fn building_ready_latch(&self) -> u8 {
        self.mission_leaf
            .as_building()
            .map_or(0, |leaf| leaf.ready_latch())
    }
    pub fn building_construction_control(&self) -> [i32; 3] {
        self.building_body
            .as_ref()
            .expect("building control requires Structure")
            .construction_control()
    }
    pub(crate) fn hash_building_body<H: std::hash::Hasher>(&self, hasher: &mut H) {
        use std::hash::Hash;
        self.building_body.hash(hasher);
        self.building_sale.hash(hasher);
    }
    pub fn construction_stage_value(&self) -> i32 {
        self.stage.value()
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn finish_building_construction_for_test(&mut self) {
        crate::sim::mission::authority::queue_entity_mission_deferred(
            self,
            MissionId::from_known(MissionType::Guard),
        );
        crate::sim::mission::authority::commence_entity_mission(self, 0);
        self.initialize_building_idle_body(0);
        self.mission_leaf.set_building_ready_latch(0);
    }
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn finish_pack_up_for_test(&mut self) {
        self.mission_leaf.set_building_ready_latch(1);
        self.mission.set_handler_state(2);
    }
}
