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
use crate::sim::mission::authority::LiveReadyInputProvider;
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
            .map(|entity| entity.seldom.mega_mission)
            .unwrap_or_default()
    }

    /// `Can_Attack_Move` (vt+0x4C0): Infantry `0x005228C0` and Unit
    /// `0x00746CC0` jump to `TechnoClass::Can_Attack_Move @ 0x0070F090`, which
    /// asks the type ([`crate::rules::object_type::ObjectType::can_attack_move`]).
    /// Phobos `[General] AttackMove.IgnoreWeaponCheck=` instead reaches
    /// [`crate::rules::object_type::ObjectType::can_attack_move_with`], which
    /// drops only the `Primary != null` half (`0x00711E90`).
    pub fn techno_can_attack_move(&self, id: u64, rules: &RuleSet) -> bool {
        self.substrate
            .entities
            .get(id)
            .and_then(|entity| self.object_type(entity.type_ref(), rules))
            .is_some_and(|object| {
                if rules.phobos() {
                    object
                        .can_attack_move_with(rules.phobos_globals.attack_move_ignore_weapon_check)
                } else {
                    object.can_attack_move()
                }
            })
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
        rules: &RuleSet,
    ) -> MissionType {
        let foot = self.substrate.entities.get(id).is_some_and(|entity| {
            entity.category() != crate::map::entities::EntityCategory::Structure
        });
        if !foot {
            return mission;
        }
        if mission != MissionType::AttackMove {
            self.substrate.foot_clear_all_tarcom(id);
            return mission;
        }
        if !self.techno_can_attack_move(id, rules) {
            return if target.is_some() {
                MissionType::Attack
            } else {
                MissionType::Move
            };
        }
        // Only the token stored is converted: the destination
        // (`0x004DF141`), else the target (`0x004DF15F`).
        let resolved = self.resolve_event_token(destination.or(target));
        let Some(entity) = self.substrate.entities.get_mut(id) else {
            return mission;
        };
        let mega = &mut entity.seldom.mega_mission;
        mega.have_attack_move_target = false;
        mega.attack_move = true;
        if destination.is_some() {
            mega.destination = resolved;
            return MissionType::Move;
        }
        if target.is_some() {
            mega.target = resolved;
            return MissionType::Attack;
        }
        self.substrate.foot_clear_all_tarcom(id);
        MissionType::AttackMove
    }

    /// `FootClass::ContinueMegaMission @ 0x004DF320` (vt+0x4C8):
    /// HaveAttackMoveTarget = 0, then `Queue_Mission(Move, 1)` and
    /// `Set_Destination(MegaDestination, 1)`, or `Queue_Mission(Attack, 1)`
    /// and `Assign_Target(MegaTarget)`; with neither, Clear_All_TarCom and
    /// false.
    pub(crate) fn foot_continue_mega_mission(
        &mut self,
        id: u64,
        rules: &RuleSet,
        effects: &mut FrameEffects<'_>,
    ) -> bool {
        let Some(entity) = self.substrate.entities.get_mut(id) else {
            return false;
        };
        entity.seldom.mega_mission.have_attack_move_target = false;
        let MegaMission {
            destination,
            target,
            ..
        } = entity.seldom.mega_mission;
        if let Some(destination) = destination {
            self.foot_queue_mission_now(id, MissionType::Move, rules);
            if let Err(cause) =
                self.assign_destination_represented(id, Some(destination.into()), rules, effects)
            {
                log::debug!("attack-move {id} resumes to {destination:?}: {cause}");
            }
            return true;
        }
        if let Some(target) = target {
            self.foot_queue_mission_now(id, MissionType::Attack, rules);
            if let Err(cause) = self.assign_target_represented(id, Some(target), rules, effects) {
                log::debug!("attack-move {id} resumes on {target:?}: {cause}");
            }
            return true;
        }
        self.substrate.foot_clear_all_tarcom(id);
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
        rules: &RuleSet,
        effects: &mut FrameEffects<'_>,
    ) -> bool {
        let Some(entity) = self.substrate.entities.get_mut(id) else {
            return false;
        };
        if !entity.seldom.mega_mission.attack_move {
            return false;
        }
        if entity.seldom.mega_mission.have_attack_move_target {
            entity.seldom.mega_mission.have_attack_move_target = false;
        } else if entity.mission.effective().known() != Some(MissionType::Guard) {
            self.substrate.foot_clear_all_tarcom(id);
            return false;
        }
        self.foot_continue_mega_mission(id, rules, effects)
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
        ctx: &mut ObjectAiCtx<'_>,
    ) {
        // `FootClass_UpdateAttackMove_SelectNewTarget @ 0x4DF3A0`
        // (`Ext/Techno/Hooks.TargetEvaluation.cpp:179-194`) runs before the
        // handler's own body.
        if rules.phobos() {
            self.phobos_attack_move_select_new_target(id, rules, ctx.overlay_registry);
        }
        // Phobos `FootClass_UpdateAttackMove_Follow @ 0x4DF3A6`
        // (`Ext/Techno/Hooks.cpp:1266-1358`) hooks the very next six bytes,
        // `mov eax,[esi+0x5C8]`. Its follow arm returns through `0x4DF425`,
        // the handler's epilogue, so the native body below must not run.
        if rules.phobos() && self.phobos_attack_move_follow(id, rules, ctx.reborrow()) {
            return;
        }

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
            // Phobos `FootClass_UpdateAttackMove_SetTarget` (`0x4DF3D3`):
            // every target write restarts a `RateDown.Reset=yes` gattling.
            self.apply_rate_down_target_reset(id, rules);
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
            let mut set_fallback = false;
            if let Some(target) = fallback
                && let Some(entity) = self.substrate.entities.get_mut(id)
            {
                // `0x004DF4A5`.
                entity.attack_target.set(target);
                set_fallback = true;
            }
            // `0x4DF46A`/`0x4DF4A5` reach the same reset after the write.
            if set_fallback {
                self.apply_rate_down_target_reset(id, rules);
            }
            return;
        }
        self.foot_queue_mission_now(id, MissionType::Attack, rules);
        if let Some(entity) = self.substrate.entities.get_mut(id) {
            entity.seldom.mega_mission.have_attack_move_target = true;
        }
    }

    /// Phobos `FootClass_UpdateAttackMove_Follow @ 0x4DF3A6`
    /// (`Ext/Techno/Hooks.cpp:1266-1358`), the instruction immediately after
    /// the `AttackMove.UpdateTarget` hook and still inside
    /// `Arrival_Target_Handler @ 0x4DF3A0`.
    ///
    /// Two independent halves live here, as in the hook:
    /// - `:1273-1295` repairs a mega mission that another order interrupted,
    ///   for **every** attack-mover, follow or not;
    /// - `:1297-1357` is the follow search, reached by `AttackMove.Follow=` or
    ///   by `.Follow.IfMindControlIsFull=` on a full capture manager. It picks
    ///   the nearest friendly attack-mover that is not itself a follower and
    ///   is not already surrounded, hands this object to it through
    ///   `SetDestination`/`SetArchiveTarget`/`QueueMission(Area_Guard, 1)`,
    ///   then clears the mega mission. With no candidate it re-issues the
    ///   object's own MegaTarget or MegaDestination and clears the mega
    ///   mission anyway.
    ///
    /// Answers `true` when the hook takes its early return (`0x4DF425`).
    fn phobos_attack_move_follow(
        &mut self,
        id: u64,
        rules: &RuleSet,
        mut ctx: ObjectAiCtx<'_>,
    ) -> bool {
        // `pThis->GetCurrentMission()` (`:1273`). A SimpleDeployer interrupted
        // out of Unload keeps its order (`:1279-1291`); every other object
        // with a mission other than Attack/Move resumes the mega mission.
        let Some(mission) = self
            .substrate
            .entities
            .get(id)
            .map(|entity| entity.mission.current().known())
        else {
            return false;
        };
        if mission != Some(MissionType::Attack) && mission != Some(MissionType::Move) {
            let deploys = self.substrate.entities.get(id).is_some_and(|entity| {
                mission == Some(MissionType::Unload)
                    && entity.category() == crate::map::entities::EntityCategory::Unit
                    && self
                        .object_type(entity.type_ref(), rules)
                        .is_some_and(|object| object.is_simple_deployer)
            });
            if !deploys {
                self.foot_continue_mega_mission(id, rules, &mut ctx.effects);
            }
        }

        // `pTypeExt->AttackMove_Follow || (…IfMindControlIsFull &&
        // pThis->CaptureManager && pThis->CaptureManager->CannotControlAnyMore())`
        // (`:1297`).
        let (radius, include_air) = {
            let Some(entity) = self.substrate.entities.get(id) else {
                return false;
            };
            let Some(object) = self.object_type(entity.type_ref(), rules) else {
                return false;
            };
            if !rules.phobos() {
                return false;
            }
            let settings = object.phobos_attack_move;
            let follow = settings.follow
                || (settings.follow_if_mind_control_is_full
                    && self.phobos_capture_cannot_control_any_more(id, rules));
            if !follow {
                return false;
            }
            // `pThis->GetGuardRange(2)` (`:1300`): TechnoClass::Threat_Range's
            // Patrol mode, then `/(double)LeptonsPerCell` for the CellSpread.
            let ranges = [
                crate::sim::combat::combat_weapon::weapon_range(
                    entity,
                    object,
                    0,
                    &self.substrate.entities,
                    rules,
                    &self.interner,
                ),
                crate::sim::combat::combat_weapon::weapon_range(
                    entity,
                    object,
                    1,
                    &self.substrate.entities,
                    rules,
                    &self.interner,
                ),
            ];
            (
                crate::sim::combat::threat_range::threat_range_leptons(object, 2, ranges, false),
                settings.follow_include_air,
            )
        };

        // `pMegaMissionTarget = MegaDestination ? MegaDestination :
        // (MegaTarget ? MegaTarget : pThis)` (`:1303`).
        let mega = self.mega_mission(id);
        let anchor = mega.destination.or(mega.target);
        let Some(origin) = self.phobos_attack_move_target_coords(Some(TargetKind::Entity(id)))
        else {
            return false;
        };
        let anchor_coords = self
            .phobos_attack_move_target_coords(anchor)
            .unwrap_or(origin);

        // `getCellSpreadItems(pThis->GetCoords(), GuardRange / 256.0,
        // IncludeAir)` (`:1298-1301`).
        let candidates = crate::sim::combat::cell_spread_technos_with_air(
            self,
            rules,
            crate::sim::projectile::ProjectileCoord::new(origin.x, origin.y, origin.z),
            f64::from(radius.max(0)) / 256.0,
            false,
            include_air,
        );

        let owner = self.substrate.entities.get(id).map(|entity| entity.owner());
        let mut closest: Option<(u64, i32)> = None;
        for candidate in candidates {
            if candidate == id {
                continue;
            }
            let Some(other) = self.substrate.entities.get(candidate) else {
                continue;
            };
            // `AbstractFlags::Foot` (`:1308`): Infantry, Unit or Aircraft; a
            // Building never follows and never leads.
            if !matches!(
                other.category(),
                crate::map::entities::EntityCategory::Infantry
                    | crate::map::entities::EntityCategory::Unit
                    | crate::map::entities::EntityCategory::Aircraft
            ) {
                continue;
            }
            if Some(other.owner()) != owner {
                continue;
            }
            if !other.seldom.mega_mission.is_attack_move() {
                continue;
            }
            // `AttackMoveFollowerTempCount >= 6` (`:1317`): the leader is
            // surrounded, look elsewhere.
            if other.phobos_attack_move_follower_temp_count() >= 6 {
                continue;
            }
            let Some(other_object) = self.object_type(other.type_ref(), rules) else {
                continue;
            };
            // `!pTargetTypeExt->AttackMove_Follow` (`:1322`): a follower is
            // not a leader, which would chain the whole squad.
            if other_object.phobos_attack_move.follow {
                continue;
            }
            let Some(coords) =
                self.phobos_attack_move_target_coords(Some(TargetKind::Entity(candidate)))
            else {
                continue;
            };
            // `pFoot->DistanceFrom(pMegaMissionTarget)` (`:1324`), a 3D
            // lepton distance through the native `Sqrt_Approx`.
            let distance = crate::util::native_x87::sqrt_approx_length([
                coords.x.wrapping_sub(anchor_coords.x),
                coords.z.wrapping_sub(anchor_coords.z),
                coords.y.wrapping_sub(anchor_coords.y),
            ]);
            if distance < closest.map_or(65536, |(_, best)| best) {
                closest = Some((candidate, distance));
            }
        }

        if let Some((target, _)) = closest {
            // `+= WhatAmI() == Infantry ? 1 : 3` (`:1338`).
            let bump = if self.substrate.entities.get(id).is_some_and(|entity| {
                entity.category() == crate::map::entities::EntityCategory::Infantry
            }) {
                1
            } else {
                3
            };
            if let Some(leader) = self.substrate.entities.get_mut(target) {
                let count = leader.phobos_attack_move_follower_temp_count();
                leader.set_phobos_attack_move_follower_temp_count(count + bump);
            }
            // `SetDestination(pClosestTarget, false)` / `SetArchiveTarget`
            // / `QueueMission(Area_Guard, true)` (`:1339-1341`).
            let _ = self.assign_destination_represented(
                id,
                Some(crate::sim::components::NavTargetRef::Object { id: target }),
                rules,
                &mut ctx.effects,
            );
            if let Some(entity) = self.substrate.entities.get_mut(id) {
                entity.set_archive_target(Some(TargetKind::Entity(target)));
            }
            self.foot_queue_mission_now(id, MissionType::AreaGuard, rules);
        } else {
            // `SetDestination(MegaTarget ? MegaTarget : MegaDestination,
            // false)` (`:1345-1349`).
            let destination = mega.target.or(mega.destination);
            let _ = self.assign_destination_represented(
                id,
                destination.map(Into::into),
                rules,
                &mut ctx.effects,
            );
        }
        // `ClearMegaMissionData()` (`:1351`): the order is spent in both
        // arms.
        self.substrate.foot_clear_all_tarcom(id);
        true
    }

    /// `pThis->CaptureManager && pThis->CaptureManager->CannotControlAnyMore()`
    /// (`Ext/Techno/Hooks.cpp:1297`): a capture manager with no free control
    /// slot. `IsFull @ 0x004722A0` supplies the capacity verdict, with Phobos'
    /// `GetControlledCount` size sum as the count.
    fn phobos_capture_cannot_control_any_more(&self, id: u64, rules: &RuleSet) -> bool {
        let Some(entity) = self.substrate.entities.get(id) else {
            return false;
        };
        let Some(manager) = entity.capture_manager.as_ref() else {
            return false;
        };
        let effective = self.capture_effective_count(id, rules).unwrap_or(0);
        manager.is_full(effective)
    }

    /// `GetCoords` of a [`TargetKind`], or of this object when the target is
    /// absent. The mega mission's anchor uses it for `DistanceFrom`.
    fn phobos_attack_move_target_coords(
        &self,
        target: Option<TargetKind>,
    ) -> Option<crate::sim::components::DriveCoord> {
        let terrain = self.resolved_terrain.as_ref();
        match target? {
            TargetKind::Entity(id) => {
                let entity = self.substrate.entities.get(id)?;
                Some(crate::sim::movement::ground_pose::object_get_coords(
                    entity, terrain,
                ))
            }
            TargetKind::Cell(rx, ry) => {
                let coord = crate::sim::projectile::cell_target_coord(terrain, rx, ry);
                Some(crate::sim::components::DriveCoord {
                    x: coord.x,
                    y: coord.y,
                    z: coord.z,
                })
            }
            TargetKind::Bullet(id) => {
                let coord = self.projectiles.get(id)?.position();
                Some(crate::sim::components::DriveCoord {
                    x: coord.x,
                    y: coord.y,
                    z: coord.z,
                })
            }
        }
    }

    /// Phobos `AttackMove.UpdateTarget` (`Ext/Techno/Hooks.TargetEvaluation.cpp:179-194`),
    /// `FootClass_UpdateAttackMove_SelectNewTarget @ 0x4DF3A0`.
    ///
    /// The key turns [`Self::phobos_can_reset_attack_move_target`] into a
    /// permission to drop the held TarCom and clear `HaveAttackMoveTarget`
    /// before the arrival handler decides; `UpdateGattlingRateDownReset()`
    /// follows (`:191-192`). The handler then sees no target and rescans.
    fn phobos_attack_move_select_new_target(
        &mut self,
        id: u64,
        rules: &RuleSet,
        registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
    ) {
        if !rules.phobos() {
            return;
        }
        let Some(entity) = self.substrate.entities.get(id) else {
            return;
        };
        let Some(obj) = self.object_type(entity.type_ref(), rules) else {
            return;
        };
        if !obj.phobos_attack_move.update_target(&rules.phobos_globals) {
            return;
        }
        // Steps 1-3 of `CheckAttackMoveCanResetTarget`: no target, the
        // MegaTarget itself, a target that is not a Techno or one that
        // `IsArmed()`, and a targeting timer still running.
        let Some(stale) = self.phobos_attack_move_stale_target(id, rules) else {
            return;
        };
        // Step 4: `pThis->GetWeapon(0)->WeaponType` must exist.
        let Some(primary) = crate::sim::world::weapon_at_index(self, rules, id, 0) else {
            return;
        };
        let primary_range = primary.range_leptons;
        // Step 5: `Greatest_Threat(ThreatType::Range, &Location, false)`.
        // `ThreatType::Range` is mask 1, the passive scan's literal.
        let Some(crate::sim::combat::TargetKind::Entity(found)) =
            crate::sim::world::direct_greatest_threat(
                self,
                rules,
                registry,
                id,
                crate::sim::combat::ScanMission::Guard,
            )
        else {
            return;
        };
        // Step 5b: the pick must be a different TechnoType than the held one.
        match (
            self.substrate.entities.get(stale),
            self.substrate.entities.get(found),
        ) {
            (Some(held), Some(pick)) if held.type_ref() != pick.type_ref() => {}
            _ => return,
        }
        // Step 6: a slot-1 weapon that exists and is `NeverUse=` vetoes unless
        // its range is at most the slot-0 range (a melee unit's virtual
        // scanner).
        match crate::sim::world::weapon_at_index(self, rules, id, 1) {
            Some(secondary) if secondary.never_use && secondary.range_leptons > primary_range => {
                return;
            }
            _ => {}
        }
        if let Some(entity) = self.substrate.entities.get_mut(id) {
            entity.attack_target.set(None);
            entity.seldom.mega_mission.have_attack_move_target = false;
        }
        self.apply_rate_down_target_reset(id, rules);
    }

    /// The `&self` half of Phobos `CheckAttackMoveCanResetTarget`
    /// (`Ext/Techno/Hooks.TargetEvaluation.cpp:144-176`): the id of the held
    /// target when it is worth replacing, or `None` when the hook refuses.
    ///
    /// - no target, or the target **is** the MegaTarget, refuses;
    /// - a target that is not a Techno, or one that `IsArmed()`, refuses;
    /// - a targeting timer still `InProgress()` refuses.
    ///
    /// `InProgress()` is the passive scan's live timer
    /// (`TechnoClass+0x180/+0x188`); `!due` is this tree's "still running".
    fn phobos_attack_move_stale_target(&self, id: u64, rules: &RuleSet) -> Option<u64> {
        use crate::sim::combat::TargetKind;
        let entity = self.substrate.entities.get(id)?;
        let target = entity.attack_target.target()?;
        if Some(target) == entity.seldom.mega_mission.target {
            return None;
        }
        let TargetKind::Entity(target_id) = target else {
            return None;
        };
        let target_entity = self.substrate.entities.get(target_id)?;
        let target_obj = self.object_type(target_entity.type_ref(), rules)?;
        if crate::sim::combat::combat_weapon::is_armed(
            target_entity,
            target_obj,
            &self.substrate.entities,
            rules,
            &self.interner,
        ) {
            return None;
        }
        if !entity.passive_scan.timer().due(self.session.binary_frame()) {
            return None;
        }
        Some(target_id)
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
        let mega = entity.seldom.mega_mission;
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
    fn foot_queue_mission_now(&mut self, id: u64, mission: MissionType, rules: &RuleSet) {
        let mission = MissionId::from_known(mission);
        let now = self.session.binary_frame();
        let _ = self.mission_queue_exact(id, mission, 1, now, &LiveReadyInputProvider { rules });
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
            return crate::sim::world::can_fire_at(self, rules, id, target, 1);
        }
        self.techno_can_fire_at_target(id, target, rules, registry)
    }
}

impl crate::sim::world::ObjectSubstrate {
    /// `FootClass::Clear_All_TarCom @ 0x004DF1A0` (vt+0x4A8): MegaMission
    /// NONE, no MegaDestination or MegaTarget, HaveAttackMoveTarget clear.
    pub(crate) fn foot_clear_all_tarcom(&mut self, id: u64) {
        if let Some(entity) = self.entities.get_mut(id) {
            entity.seldom.mega_mission = MegaMission::default();
        }
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

#[cfg(all(test, tests_sim_h_o))]
#[path = "mega_mission_tests.rs"]
mod tests;

/// Phobos `AttackMove.*` (`Ext/TechnoType/Body.cpp:1086-1087`).
#[cfg(all(test, tests_sim_h_o))]
#[path = "phobos_attack_move_tests.rs"]
mod phobos_attack_move_tests;

/// Phobos `AttackMove.Follow.*` (`Ext/TechnoType/Body.cpp:1149-1151`).
#[cfg(all(test, tests_sim_h_o))]
#[path = "phobos_attack_move_follow_tests.rs"]
mod phobos_attack_move_follow_tests;
