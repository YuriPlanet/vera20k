//! The Foot's MegaMission order (`FootClass +0x5C4..+0x5D1`): the attack-move.
//!
//! `TechnoClass::ClickedMission @ 0x006FFBE0` promotes a Move or Attack click
//! to mission 29 while the Ctrl+Shift chord is held and the clicked object can
//! attack-move ([`Simulation::techno_can_attack_move`]). The MEGAMISSION event
//! hands mission 29 to `Assign_Target_Command` (vt+0x4A4), which stores the
//! order here and answers Move or Attack for the event to queue in its place
//! ([`Simulation::foot_assign_target_command`]). From then on:
//! - the targeting slot of `TechnoClass::AI` asks `Arrival_Target_Handler`
//!   (vt+0x4CC) to scan instead of the passive block
//!   ([`Simulation::foot_arrival_target_handler`]);
//! - `Enter_Idle_Mode` asks `RefreshMegaMission` (vt+0x4D0) whether to resume
//!   the order ([`Simulation::foot_refresh_mega_mission`]);
//! - pointer expiry turns an expiring MegaTarget into a destination at its
//!   cell and drops an expiring MegaDestination
//!   ([`MegaMission::pointer_expired`]).
//!
//! Every Foot class (Infantry, Unit, Aircraft) holds the Foot bodies in these
//! slots; a Building holds TechnoClass's stubs (`0x0070F000..0x0070F110`),
//! which store nothing.

use crate::rules::ruleset::RuleSet;
use crate::sim::combat::TargetKind;
use crate::sim::mission::authority::{EntityReadyInputProvider, LiveReadyInputProvider};
use crate::sim::mission::{MissionId, MissionType};
use crate::sim::world::{FrameEffects, ObjectAiCtx, Simulation};

/// `FootClass +0x5C4` MegaMission, `+0x5C8` MegaDestination, `+0x5CC`
/// MegaTarget and `+0x5D1` HaveAttackMoveTarget.
#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize,
)]
pub(crate) struct MegaMission {
    /// MegaMission is 29, attack move. `Assign_Target_Command` stores no
    /// other mission and `Clear_All_TarCom` (`0x004DF1A0`) stores NONE, so
    /// HaveMegaMission (vt+0x4AC, `0x004DF1C0`) and MegaMissionIsAttackMove
    /// (vt+0x4C4, `0x004DF310`) answer alike.
    attack_move: bool,
    /// MegaDestination: the ordered Cell or object, or the Cell a MegaTarget
    /// expired on.
    destination: Option<TargetKind>,
    /// MegaTarget: the ordered target.
    target: Option<TargetKind>,
    /// HaveAttackMoveTarget: Arrival_Target_Handler's scan found the target
    /// the object is now attacking.
    have_attack_move_target: bool,
}

impl MegaMission {
    /// An attack-move that has found nothing yet, with this MegaDestination
    /// and no MegaTarget.
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn attack_move(destination: Option<TargetKind>) -> Self {
        Self::for_test(true, destination, None, false)
    }

    /// Any field values, as a native state supplies them.
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn for_test(
        attack_move: bool,
        destination: Option<TargetKind>,
        target: Option<TargetKind>,
        have_attack_move_target: bool,
    ) -> Self {
        Self {
            attack_move,
            destination,
            target,
            have_attack_move_target,
        }
    }

    /// HaveMegaMission (vt+0x4AC) and MegaMissionIsAttackMove (vt+0x4C4).
    pub(crate) fn is_attack_move(&self) -> bool {
        self.attack_move
    }

    /// Planning renderer's Foot getters (vt+0x4B0/0x4B8,
    /// 4DF1D0/4DF1F0): preserve the acquired-target latch and prefer the
    /// MegaDestination token, then MegaTarget. Coordinate resolution stays
    /// with the shared TargetClass coordinate owner.
    pub(crate) fn planning_draw_target(&self) -> (bool, Option<TargetKind>) {
        (
            self.have_attack_move_target,
            self.destination.or(self.target),
        )
    }

    /// MegaDestination.
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn destination(&self) -> Option<TargetKind> {
        self.destination
    }

    /// Whether the MegaTarget is this object.
    pub(crate) fn targets(&self, id: u64) -> bool {
        self.target == Some(TargetKind::Entity(id))
    }

    /// Whether the MegaTarget or the MegaDestination is this object, which
    /// [`Self::pointer_expired`] then changes.
    pub(crate) fn references(&self, id: u64) -> bool {
        self.referenced_ids().any(|referenced| referenced == id)
    }

    /// The objects the MegaTarget and the MegaDestination name.
    pub(crate) fn referenced_ids(&self) -> impl Iterator<Item = u64> {
        [self.target, self.destination]
            .into_iter()
            .filter_map(|target| match target {
                Some(TargetKind::Entity(id)) => Some(id),
                _ => None,
            })
    }

    /// `FootClass::PointerExpired @ 0x004D9AC9..0x004D9B43`: an expiring
    /// MegaTarget becomes a MegaDestination at the cell of its coordinate
    /// (vt+0x48) and is dropped; then an expiring MegaDestination is dropped
    /// (`0x004D9B35..0x004D9B3D`).
    pub(crate) fn pointer_expired(&mut self, expired: u64, expired_cell: Option<(u16, u16)>) {
        if self.targets(expired) {
            self.destination = expired_cell.map(|(rx, ry)| TargetKind::Cell(rx, ry));
            self.target = None;
        }
        if self.destination == Some(TargetKind::Entity(expired)) {
            self.destination = None;
        }
    }
}

impl Simulation {
    /// The object's MegaMission (a Building has none).
    pub(crate) fn mega_mission(&self, id: u64) -> MegaMission {
        self.substrate
            .entities
            .get(id)
            .map(|entity| entity.mega_mission)
            .unwrap_or_default()
    }

    /// `Can_Attack_Move` (vt+0x4C0): Infantry `0x005228C0` and Unit
    /// `0x00746CC0` jump to `TechnoClass::Can_Attack_Move @ 0x0070F090`, which
    /// asks the type ([`crate::rules::object_type::ObjectType::can_attack_move`]).
    pub fn techno_can_attack_move(&self, id: u64, rules: &RuleSet) -> bool {
        self.substrate
            .entities
            .get(id)
            .and_then(|entity| self.object_type(entity.type_ref(), rules))
            .is_some_and(|object| object.can_attack_move())
    }

    /// `Assign_Target_Command` (vt+0x4A4), which the MEGAMISSION event asks
    /// for the mission to queue (`0x004C73AA`). `FootClass` `0x004DF0E0`:
    /// - any mission but 29 runs Clear_All_TarCom and answers itself;
    /// - mission 29 from an object that cannot attack-move answers Attack
    ///   with an event target, else Move, and stores nothing
    ///   (`0x004DF10B..0x004DF122`);
    /// - otherwise HaveAttackMoveTarget = 0 and MegaMission = 29, then the
    ///   event destination becomes MegaDestination and the answer is Move, or
    ///   else the event target becomes MegaTarget and the answer is Attack;
    ///   the other field keeps its value. With neither it runs
    ///   Clear_All_TarCom and answers 29 (`0x004DF12A..0x004DF186`).
    ///
    /// A token decides by its tag (`0x004DF0F7..0x004DF108`), and the field
    /// takes what it resolves to (TargetClass `0x006E6E20`,
    /// [`Simulation::resolve_event_token`]): an object deleted before the
    /// event runs still answers Move or Attack, with a NULL field.
    ///
    /// A Building's slot (`0x0070F000`) answers the event mission and stores
    /// nothing.
    pub(crate) fn foot_assign_target_command(
        &mut self,
        id: u64,
        mission: MissionType,
        target: Option<TargetKind>,
        destination: Option<TargetKind>,
        rules: Option<&RuleSet>,
    ) -> MissionType {
        let foot = self.substrate.entities.get(id).is_some_and(|entity| {
            entity.category() != crate::map::entities::EntityCategory::Structure
        });
        if !foot {
            return mission;
        }
        if mission != MissionType::AttackMove {
            self.foot_clear_all_tarcom(id);
            return mission;
        }
        if !rules.is_some_and(|rules| self.techno_can_attack_move(id, rules)) {
            return if target.is_some() {
                MissionType::Attack
            } else {
                MissionType::Move
            };
        }
        let resolved_target = self.resolve_event_token(target);
        let resolved_destination = self.resolve_event_token(destination);
        let Some(entity) = self.substrate.entities.get_mut(id) else {
            return mission;
        };
        let mega = &mut entity.mega_mission;
        mega.have_attack_move_target = false;
        mega.attack_move = true;
        if destination.is_some() {
            mega.destination = resolved_destination;
            return MissionType::Move;
        }
        if target.is_some() {
            mega.target = resolved_target;
            return MissionType::Attack;
        }
        self.foot_clear_all_tarcom(id);
        MissionType::AttackMove
    }

    /// `FootClass::Clear_All_TarCom @ 0x004DF1A0` (vt+0x4A8): MegaMission
    /// NONE, no MegaDestination or MegaTarget, HaveAttackMoveTarget clear.
    pub(crate) fn foot_clear_all_tarcom(&mut self, id: u64) {
        if let Some(entity) = self.substrate.entities.get_mut(id) {
            entity.mega_mission = MegaMission::default();
        }
    }

    /// `FootClass::ContinueMegaMission @ 0x004DF320` (vt+0x4C8):
    /// HaveAttackMoveTarget = 0, then `Queue_Mission(Move, 1)` and
    /// `Set_Destination(MegaDestination, 1)`, or `Queue_Mission(Attack, 1)`
    /// and `Assign_Target(MegaTarget)`; with neither, Clear_All_TarCom and
    /// false.
    pub(crate) fn foot_continue_mega_mission(
        &mut self,
        id: u64,
        rules: Option<&RuleSet>,
        registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
        effects: FrameEffects<'_>,
    ) -> bool {
        let Some(entity) = self.substrate.entities.get_mut(id) else {
            return false;
        };
        entity.mega_mission.have_attack_move_target = false;
        let MegaMission {
            destination,
            target,
            ..
        } = entity.mega_mission;
        if let Some(destination) = destination {
            self.foot_queue_mission_now(id, MissionType::Move, rules);
            if let Err(cause) = self.assign_destination_represented(
                id,
                Some(destination.into()),
                rules,
                registry,
                effects,
            ) {
                log::debug!("attack-move {id} resumes to {destination:?}: {cause}");
            }
            return true;
        }
        if let Some(target) = target {
            self.foot_queue_mission_now(id, MissionType::Attack, rules);
            if let Err(cause) =
                self.assign_target_represented(id, Some(target), rules, registry, effects)
            {
                log::debug!("attack-move {id} resumes on {target:?}: {cause}");
            }
            return true;
        }
        self.foot_clear_all_tarcom(id);
        false
    }

    /// `FootClass::RefreshMegaMission @ 0x004DF4B0` (vt+0x4D0), the first
    /// question of `TechnoClass::Enter_Idle_Mode @ 0x00709A40`. On an
    /// attack-move: a target found on the way is let go and the order resumes
    /// ([`Self::foot_continue_mega_mission`]); without one the order ends
    /// (Clear_All_TarCom, false) unless Get_Mission (vt+0x184) is Guard, when
    /// it resumes too. Answers false with no attack-move.
    pub(crate) fn foot_refresh_mega_mission(
        &mut self,
        id: u64,
        rules: Option<&RuleSet>,
        registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
        effects: FrameEffects<'_>,
    ) -> bool {
        let Some(entity) = self.substrate.entities.get_mut(id) else {
            return false;
        };
        if !entity.mega_mission.attack_move {
            return false;
        }
        if entity.mega_mission.have_attack_move_target {
            entity.mega_mission.have_attack_move_target = false;
        } else if entity.mission.effective().known() != Some(MissionType::Guard) {
            self.foot_clear_all_tarcom(id);
            return false;
        }
        self.foot_continue_mega_mission(id, rules, registry, effects)
    }

    /// `FootClass::Arrival_Target_Handler @ 0x004DF3A0` (vt+0x4CC), which the
    /// targeting slot of `TechnoClass::AI` runs for an attack-mover in place
    /// of the passive block (`0x006FA681..0x006FA695`).
    /// - With a MegaDestination: a target the scan found that is still in
    ///   reach ([`Self::foot_in_auxiliary_search_range`]) is kept; otherwise
    ///   the TarCom is dropped without the class setter.
    /// - Without one: a MegaTarget in reach, or such a found target, is kept;
    ///   otherwise the TarCom is dropped, and if the scan below finds nothing
    ///   the MegaTarget is written back as the TarCom.
    ///
    /// Then `Retaliate_And_Scan(&Location, 1)` (vt+0x39C); a target found
    /// queues Attack with `commence_now = 1` and sets HaveAttackMoveTarget.
    pub(crate) fn foot_arrival_target_handler(
        &mut self,
        id: u64,
        rules: &RuleSet,
        ctx: ObjectAiCtx<'_>,
    ) {
        let Some(ArrivalStep::Scan {
            drop_tarcom,
            fallback,
        }) = self.foot_arrival_step(id, rules, ctx.overlay_registry)
        else {
            return;
        };
        if drop_tarcom {
            self.substrate
                .entities
                .get_mut(id)
                .unwrap()
                .attack_target
                .set(None);
        }
        let found = crate::sim::world::retaliate_and_scan(
            self,
            id,
            rules,
            crate::sim::combat::ScanMission::Guard,
            ctx,
            None,
        );
        if !found {
            if let Some(target) = fallback
                && let Some(entity) = self.substrate.entities.get_mut(id)
            {
                // `0x004DF4A5`.
                entity.attack_target.set(target);
            }
            return;
        }
        self.foot_queue_mission_now(id, MissionType::Attack, Some(rules));
        if let Some(entity) = self.substrate.entities.get_mut(id) {
            entity.mega_mission.have_attack_move_target = true;
        }
    }

    /// What [`Self::foot_arrival_target_handler`] does with `id` as it
    /// stands, before any of its writes; `None` without the object.
    pub(crate) fn foot_arrival_step(
        &self,
        id: u64,
        rules: &RuleSet,
        registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
    ) -> Option<ArrivalStep> {
        let entity = self.substrate.entities.get(id)?;
        let mega = entity.mega_mission;
        let tarcom = entity.attack_target.target();
        let in_reach = |target| self.foot_in_auxiliary_search_range(id, target, rules, registry);
        Some(if mega.destination.is_some() {
            if mega.have_attack_move_target && in_reach(tarcom) {
                ArrivalStep::Hold
            } else {
                // `0x004DF3D3`.
                ArrivalStep::Scan {
                    drop_tarcom: mega.have_attack_move_target,
                    fallback: None,
                }
            }
        } else if mega.target.is_some() && in_reach(mega.target)
            || mega.have_attack_move_target && in_reach(tarcom)
        {
            ArrivalStep::Hold
        } else {
            // `0x004DF46A`.
            ArrivalStep::Scan {
                drop_tarcom: true,
                fallback: Some(mega.target),
            }
        })
    }

    /// `Queue_Mission(mission, 1)` (vt+0x1E8): queued, and committed at once
    /// when the object is ready ([`Self::mission_queue_exact`]).
    fn foot_queue_mission_now(&mut self, id: u64, mission: MissionType, rules: Option<&RuleSet>) {
        let mission = MissionId::from_known(mission);
        let now = self.session.binary_frame();
        let _ = match rules {
            Some(rules) => {
                self.mission_queue_exact(id, mission, 1, now, &LiveReadyInputProvider { rules })
            }
            None => self.mission_queue_exact(id, mission, 1, now, &EntityReadyInputProvider),
        };
    }

    /// `TechnoClass::InAuxiliarySearchRange @ 0x006F78D0` (vt+0x3B4), whose
    /// one caller is Arrival_Target_Handler: when the weapon in slot 1
    /// (vt+0x3E8, `0x0070DD90`) is `NeverUse=` (`+0x136`), CanFireAt with that
    /// slot (vt+0x3A8); otherwise CanFireAtTarget (vt+0x3AC, the selected
    /// weapon's InRange). Both answer true for no target
    /// (`0x006F77BB..0x006F77C6`), so a found target that has since expired
    /// still holds the handler off its scan.
    fn foot_in_auxiliary_search_range(
        &self,
        id: u64,
        target: Option<TargetKind>,
        rules: &RuleSet,
        registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
    ) -> bool {
        let Some(target) = target else {
            return true;
        };
        if crate::sim::world::weapon_at_index(self, rules, id, 1)
            .is_some_and(|weapon| weapon.never_use)
        {
            return crate::sim::world::can_fire_at(self, rules, id, target, 1, registry);
        }
        self.techno_can_fire_at_target(id, target, rules, registry)
    }
}

/// What Arrival_Target_Handler (`0x004DF3A0`) does with an attack-mover
/// ([`Simulation::foot_arrival_step`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ArrivalStep {
    /// A target in reach holds the handler off its scan.
    Hold,
    /// The handler scans, first dropping the TarCom without the class setter
    /// when `drop_tarcom`, and writes `fallback` back as the TarCom when the
    /// scan finds nothing.
    Scan {
        drop_tarcom: bool,
        fallback: Option<Option<TargetKind>>,
    },
}

#[cfg(test)]
#[path = "mega_mission_tests.rs"]
mod tests;
