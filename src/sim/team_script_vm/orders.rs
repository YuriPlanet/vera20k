//! The member virtuals `TeamClass` calls, each through its class's port.
//!
//! Team code orders its members only through these: `Queue_Mission`
//! (`vt+0x1E8`), `Ready_To_Commence`/`Commence` (`vt+0x200`/`vt+0x1EC`),
//! `Assign_Target` (`vt+0x3C8`), `Set_Destination` (`vt+0x480`),
//! `Enter_Idle_Mode` (`vt+0x484`, [`crate::sim::world::enter_idle_mode`])
//! and `Set_ArchiveTarget` (`0x0070C610`).

use crate::rules::overlay_types::OverlayTypeRegistry;
use crate::rules::ruleset::RuleSet;
use crate::sim::mission::authority::LiveReadyInputProvider;
use crate::sim::mission::{MissionId, MissionType};
use crate::sim::world::FrameEffects;
use crate::sim::world::Simulation;

use super::TeamTarget;

impl Simulation {
    /// `vt+0x1E8` `Queue_Mission(mission, 0)`: `MissionClass::Queue_Mission
    /// @ 0x005B35E0`, behind the Aircraft filter `0x0041BA90`.
    pub(super) fn team_member_queue_mission(
        &mut self,
        id: u64,
        mission: MissionType,
        rules: &RuleSet,
    ) {
        self.team_member_queue_mission_id(id, MissionId::from_known(mission), rules);
    }

    /// [`Self::team_member_queue_mission`] with a raw mission (script
    /// action 11's argument).
    pub(super) fn team_member_queue_mission_id(
        &mut self,
        id: u64,
        mission: MissionId,
        rules: &RuleSet,
    ) {
        let now = self.session.binary_frame();
        let readiness = LiveReadyInputProvider { rules };
        let _ = self.mission_queue_exact(id, mission, 0, now, &readiness);
    }

    /// `vt+0x200` then `vt+0x1EC`: the queued mission commences at once when
    /// the member is ready for it.
    pub(super) fn team_member_commence(&mut self, id: u64, rules: &RuleSet) {
        if self.mission_ready_to_commence(id, rules) {
            let now = self.session.binary_frame();
            let _ = self.substrate.mission_commence_exact(id, now);
        }
    }

    /// `vt+0x3C8` `Assign_Target(NULL)`: `TechnoClass::Assign_Target @
    /// 0x006FCDB0`, Infantry's `0x0051B1F0`.
    pub(super) fn team_member_clear_target(&mut self, id: u64, rules: &RuleSet) {
        let _ =
            self.assign_target_represented(id, None, Some(rules), None, FrameEffects::default());
    }

    /// `vt+0x3C8` `Assign_Target(target)`.
    pub(super) fn team_member_assign_target(
        &mut self,
        id: u64,
        target: TeamTarget,
        rules: &RuleSet,
        registry: Option<&OverlayTypeRegistry>,
        frame_effects: FrameEffects<'_>,
    ) {
        let _ = self.assign_target_represented(
            id,
            Some(target.target_kind()),
            Some(rules),
            registry,
            frame_effects,
        );
    }

    /// `vt+0x480` `Set_Destination(target, 1)` through the class setters
    /// ([`Simulation::assign_destination_represented`]).
    pub(super) fn team_member_set_destination(
        &mut self,
        id: u64,
        target: Option<TeamTarget>,
        rules: &RuleSet,
        registry: Option<&OverlayTypeRegistry>,
        frame_effects: FrameEffects<'_>,
    ) {
        let requested = target.map(|target| match target {
            TeamTarget::Cell { x, y } => {
                crate::sim::components::NavTargetRef::cell(x as u16, y as u16)
            }
            TeamTarget::Object(id) => crate::sim::components::NavTargetRef::object(id),
        });
        let _ = self.assign_destination_represented(
            id,
            requested,
            Some(rules),
            registry,
            frame_effects,
        );
    }
}

impl crate::sim::world::ObjectSubstrate {
    /// `Set_ArchiveTarget(NULL)` (`0x0070C610`), a plain store.
    pub(super) fn team_member_clear_archive_target(&mut self, id: u64) {
        if let Some(entity) = self.entities.get_mut(id) {
            entity.set_archive_target(None);
        }
    }
}
