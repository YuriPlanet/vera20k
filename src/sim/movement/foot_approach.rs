//! Foot4D5690, reached through Unit7414E0 by a unit's Approach (Attack, Hunt,
//! AreaGuard, Rescue), and through Infantry522340 by an infantryman's
//! ([`Simulation::approach_target`]), whatever their locomotor. Native execution:
//! tools/spatial_oracle/fv_cell_attack. The shared range,
//! Cell admission, zone-cost, target and destination owners retain their work.
//!
//! The head of `UnitClass::Approach_Target @ 0x007414E0` lives here too: its
//! computer and OmniCrusher crush arms ([`ApproachCrush`]) and its
//! BalloonHover arm, ahead of the Foot body.

use super::foot_path::{cell_centre, chebyshev, coord_cell};
use crate::map::entities::EntityCategory;
use crate::map::resolved_terrain::NativeCellQuery;
use crate::rules::locomotor_type::LocomotorKind;
use crate::rules::object_type::Ability;
use crate::rules::overlay_types::OverlayTypeRegistry;
use crate::rules::ruleset::RuleSet;
use crate::sim::cell_rect::{self, CellRect, CellRectPassabilityContext};
use crate::sim::combat::veterancy::{has_weapon_ability, rank_from_u16};
use crate::sim::combat::{self, TargetKind, in_range};
use crate::sim::components::{DriveCoord, NavTargetRef};
use crate::sim::mission::MissionType;
use crate::sim::movement::bump_crush::{CrushAllyGate, CrushTarget, object_is_crushable_by};
use crate::sim::world::FrameEffects;
use crate::sim::world::Simulation;
use crate::util::direction_tables::{cell_delta_unchecked, facing16_between};
use crate::util::native_trig::facing_step_world_xy;
use crate::util::native_x87::distance_3d_leptons;

const OFFSETS: [i16; 25] = [
    0, 1, -1, 2, -2, 3, -3, 4, -4, 5, -5, 6, -6, 8, -8, 16, -16, 24, -24, 32, -32, 48, -48, 64, -64,
]; // original signed table8224DC

#[cfg(test)]
#[path = "foot_approach_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "approach_crush_tests.rs"]
mod approach_crush_tests;

#[cfg(test)]
#[path = "mission_approach_tests.rs"]
mod mission_approach_tests;

/// What the crush arms of `UnitClass::Approach_Target @ 0x007414E0` (vt+0x53C)
/// ask of a Unit and the object it targets. Either arm takes the Target as the
/// destination (`0x0074165F`: SetDestination(Target, 1), vt+0x480, unless
/// no-move) and returns it; the BalloonHover arm sits between them. The arms
/// ask for the distance and crushability, the costly facts, only where their
/// earlier tests pass, as native calls them.
///
/// Native execution: tools/unit_crush_oracle.json `approach` rows, replayed by
/// `approach_crush_tests::approach_crush_matches_native_execution`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ApproachCrush {
    /// `HouseClass::IsControlledByHuman @ 0x0050B730` of the owner.
    pub(crate) controlled_by_human: bool,
    /// The unit holds a NavCom (`+0x5A4`).
    pub(crate) nav_com: bool,
    /// UnitType `Crusher=` (`+0xD28`), or the rank's CRUSHER ability
    /// (`HasWeaponAbility(0x11) @ 0x0070D0D0`).
    pub(crate) crusher: bool,
    /// `[CombatDamage] Crush=` (`Rules+0x1728`).
    pub(crate) reach: i32,
    /// The Target is a Techno (`+0x14 & 1`).
    pub(crate) techno: bool,
    /// The Target is an Object (`+0x14 & 2`).
    pub(crate) object: bool,
    /// UnitType `OmniCrusher=` (`+0xD29`).
    pub(crate) omni: bool,
    /// The owner's IsHuman byte (`+0x1EC`), not IsControlledByHuman.
    pub(crate) is_human: bool,
    /// The Target came from the passive scan (`+0x50C`).
    pub(crate) passive: bool,
}

impl ApproachCrush {
    /// The computer arm (`0x007414E4..0x00741593`): an owner no human
    /// controls, no NavCom, a crusher, and a Techno Target strictly closer
    /// than `Crush=` (`distance`, `ObjectClass::Distance @ 0x005F6360` in
    /// leptons; `JGE` refuses at `0x00741560`) that it can crush
    /// (`crushable`, `ObjectClass::Is_Crushable_By @ 0x005F6CD0`). Its last
    /// test, `AutoCrush=` (`+0xD2D`) or no human control, always passes for
    /// that owner.
    pub(crate) fn computer_arm(
        self,
        distance: impl FnOnce() -> i32,
        crushable: impl FnOnce() -> bool,
    ) -> bool {
        !self.controlled_by_human
            && !self.nav_com
            && self.crusher
            && distance() < self.reach
            && self.techno
            && crushable()
    }

    /// The OmniCrusher arm (`0x00741603..0x0074165D`): any owner, reach and
    /// NavCom, and an Object Target it can crush (`crushable`), unless a
    /// human house (IsHuman) holds that Target from the passive scan.
    pub(crate) fn omni_arm(self, crushable: impl FnOnce() -> bool) -> bool {
        self.omni && !(self.is_human && self.passive) && self.object && crushable()
    }
}

impl Simulation {
    /// The facts [`ApproachCrush`] tests for Unit `id` and the object it
    /// targets, with that object; None where neither arm can take the call:
    /// another actor, a Cell Target (neither Techno nor Object) or a unit
    /// that crushes nothing. Every represented object is a Techno.
    fn approach_crush(&self, id: u64, rules: &RuleSet) -> Option<(ApproachCrush, u64)> {
        let actor = self.substrate.entities.get(id)?;
        let TargetKind::Entity(target) = actor.attack_target.as_ref()?.target else {
            return None;
        };
        if actor.category != EntityCategory::Unit || !self.substrate.entities.contains(target) {
            return None;
        }
        let object = self.object_type(actor.type_ref(), rules)?;
        let crusher = object.crusher
            || has_weapon_ability(rank_from_u16(actor.veterancy()), object, Ability::Crusher);
        // The pursuit pass asks this of every pursuer each frame.
        if !crusher && !object.omni_crusher {
            return None;
        }
        let facts = ApproachCrush {
            controlled_by_human: self.owner_is_human(actor.owner()),
            nav_com: actor.navigation.nav_com.is_some(),
            crusher,
            reach: rules.combat_damage.crush,
            techno: true,
            object: true,
            omni: object.omni_crusher,
            is_human: self
                .houses
                .get(&actor.owner())
                .is_some_and(|house| house.is_human),
            passive: actor.passively_acquired_target,
        };
        Some((facts, target))
    }

    /// `ObjectClass::Is_Crushable_By @ 0x005F6CD0` of `target` by Unit `id`,
    /// alliance question first ([`CrushAllyGate`], with its one-way-alliance
    /// residual), as the bump crush asks it.
    fn approach_crushable(&self, id: u64, target: u64, rules: &RuleSet) -> bool {
        let (Some(actor), Some(victim)) = (
            self.substrate.entities.get(id),
            self.substrate.entities.get(target),
        ) else {
            return false;
        };
        let ally = CrushAllyGate::new(
            self.interner.resolve(actor.owner()),
            &self.house_alliances,
            &self.interner,
        );
        !ally.spares(victim)
            && self
                .object_type(actor.type_ref(), rules)
                .is_some_and(|object| {
                    object_is_crushable_by(
                        object.omni_crusher,
                        CrushTarget::from_entity(victim, self.session.binary_frame),
                    )
                })
    }

    /// The head of `UnitClass::Approach_Target @ 0x007414E0` for Unit `id`:
    /// the computer crush arm, the BalloonHover arm
    /// ([`Self::balloon_hover_approach`]), then the OmniCrusher arm
    /// ([`ApproachCrush`]). Answers the destination of the first arm that takes
    /// the call, or None where native goes on to FootClass::Approach_Target
    /// (`0x00741685`).
    ///
    /// Residuals:
    /// - With the no-move flag set, native returns the Target without the
    ///   destination. Every mission passes 0, and no VERA caller passes 1.
    /// - Mission_Patrol calls Approach too (`0x004D473A`), and VERA has no
    ///   Foot Patrol body; the pursuit pass (`tick_attack_pursuit`) stands in.
    ///   Trigger: a unit on Patrol holding a target that an arm would take: a
    ///   Kirov, a Floating Disc, a computer's crusher beside it or a Battle
    ///   Fortress. Effect: the pass sends it to the target's cell, and its
    ///   in-range halt admits no Patrol (`range_stop_admits`), so it goes on
    ///   to that cell and stays on Patrol, roughly where the arms send it; a
    ///   Disc on its laser does not stop at range. Frequency: nothing in VERA
    ///   puts a Unit on Patrol today; native producers were not surveyed.
    pub(crate) fn unit_approach_head(&self, id: u64, rules: &RuleSet) -> Option<NavTargetRef> {
        let crush = self.approach_crush(id, rules);
        if let Some((facts, target)) = crush
            && facts.computer_arm(
                || {
                    self.object_distance_to(id, TargetKind::Entity(target))
                        .unwrap_or(i32::MAX)
                },
                || self.approach_crushable(id, target, rules),
            )
        {
            return Some(NavTargetRef::object(target));
        }
        if let Some(destination) = self.balloon_hover_approach(id, rules) {
            return Some(destination);
        }
        crush
            .filter(|&(facts, target)| {
                facts.omni_arm(|| self.approach_crushable(id, target, rules))
            })
            .map(|(_, target)| NavTargetRef::object(target))
    }

    /// Unit 0x7414E0's BalloonHover arm (0x741599..0x741600), as the Foot
    /// missions call Approach (vt+0x53C) with no-move 0
    /// ([`Self::approach_unit_head`]). A `BalloonHover=` type (+0xD6A) whose GetWeapon(SelectWeapon(Target))
    /// (vt+0x2E4, vt+0x3F8) has a `Vertical=` (+0x2C0) `Projectile=` (+0xA0)
    /// takes its Target as its destination (vt+0x480 with 1) and returns.
    /// Any other weapon, or none, goes on to the next arm (0x741603). This
    /// answers that destination, or None for the next arm.
    ///
    /// Native comparison: tools/spatial_oracle/balloon_hover.json `approach`
    /// rows.
    pub(crate) fn balloon_hover_approach(&self, id: u64, rules: &RuleSet) -> Option<NavTargetRef> {
        let actor = self.substrate.entities.get(id)?;
        let target = actor.attack_target.as_ref()?.target;
        if actor.category != EntityCategory::Unit
            || !self.object_type(actor.type_ref(), rules)?.balloon_hover
        {
            return None;
        }
        let selected = combat::pursuit_selection(
            actor,
            &target,
            &self.substrate.entities,
            rules,
            &self.interner,
            self.resolved_terrain.as_ref(),
            Some(&self.house_alliances),
        );
        balloon_hover_destination(selected.map(|selected| selected.weapon), target, rules)
    }

    /// Approach (vt+0x53C) with no-move 0, as Mission_Attack (`0x004D4E6A`),
    /// Mission_Hunt (`0x004D54E3`, `0x004D54D2`), Mission_AreaGuard
    /// (`0x004D6F32`) and Mission_Rescue (`0x004DDFEB`) call it: a Unit's head
    /// ([`Self::approach_unit_head`]) or an infantryman's deployed arm
    /// ([`Self::approach_infantry_deployed`]), then the Foot body
    /// ([`Self::approach_foot_target`]; Unit `0x00741689`, Infantry
    /// `0x0052236E`) for a mover [`Self::owns_foot_approach`] represents.
    /// The pursuit pass stands in for the rest of Approach for any other
    /// mover ([`Self::mission_owns_approach`]).
    pub(crate) fn approach_target(
        &mut self,
        id: u64,
        rules: &RuleSet,
        registry: Option<&OverlayTypeRegistry>,
        frame_effects: FrameEffects<'_>,
    ) {
        if self.approach_unit_head(id, rules, registry, frame_effects)
            || self.approach_infantry_deployed(id, rules, registry, frame_effects)
            || !self.owns_foot_approach(id, rules)
        {
            return;
        }
        self.approach_foot_target(id, rules, registry, frame_effects)
            .expect("ordinary Foot approach requires valid live map/navigation state");
    }

    /// The head of Approach ([`Self::approach_target`]) for a Unit
    /// ([`Self::unit_approach_head`]): the destination the head answers is set
    /// (vt+0x480 with 1). Answers whether the head took the call.
    pub(crate) fn approach_unit_head(
        &mut self,
        id: u64,
        rules: &RuleSet,
        registry: Option<&OverlayTypeRegistry>,
        frame_effects: FrameEffects<'_>,
    ) -> bool {
        let Some(destination) = self.unit_approach_head(id, rules) else {
            return false;
        };
        let _ = self.assign_destination_represented(
            id,
            Some(destination),
            Some(rules),
            registry,
            frame_effects,
        );
        true
    }

    /// The deployed arm of `InfantryClass::Approach_Target @ 0x00522340`
    /// (`0x00522378..0x005224C6`): an infantryman with a Target and in a deploy
    /// action (Doing 27..30) never reaches the Foot body. A computer house's
    /// `Deployer=` (`+0xEC8`), `DeployFire=` (`+0x6AC`) man whose
    /// `DeployFireWeapon=` (`+0x6A8`) slot holds a weapon that is not
    /// `AreaFire=` (GetWeapon, vt+0x3F8), and who cannot fire on the Target
    /// with it from where he stands (CanFireAt, vt+0x3A8), undeploys
    /// (Do_Action(0x1F, 0, 0), vt+0x558) so his next Approach can walk. No RNG.
    /// Answers whether the arm took the call.
    ///
    /// Not ported: the arm at `0x00522434` (deploy, or stop with a discarded
    /// RandomRanged(0, 2)) needs a Doing outside 27..30 at `0x005223EF`, which
    /// the entry test (`0x00522352..0x0052236A`) excludes and the two queries
    /// between (`0x006F77B0`, `0x0070E140`) do not write.
    fn approach_infantry_deployed(
        &mut self,
        id: u64,
        rules: &RuleSet,
        registry: Option<&OverlayTypeRegistry>,
        frame_effects: FrameEffects<'_>,
    ) -> bool {
        let Some(actor) = self.substrate.entities.get(id) else {
            return false;
        };
        let Some(target) = actor.attack_target.as_ref().map(|a| a.target) else {
            return false;
        };
        if !actor.infantry_deploy_doing() {
            return false;
        }
        if self.owner_is_human(actor.owner()) {
            return true;
        }
        let Some(object) = self.object_type(actor.type_ref(), rules) else {
            return true;
        };
        if !object.deployer || !object.deploy_fire {
            return true;
        }
        let slot = object.deploy_fire_weapon;
        let in_reach = crate::sim::world::can_fire_at(self, rules, id, target, slot, registry);
        if crate::sim::world::weapon_at_index(self, rules, id, slot)
            .is_none_or(|weapon| weapon.area_fire)
            || in_reach
        {
            return true;
        }
        if let Err(cause) = self.infantry_do_action(
            id,
            super::infantry_action::DO_UNDEPLOY,
            false,
            rules,
            frame_effects,
        ) {
            log::debug!("infantry {id} Approach undeploy: {cause}");
        }
        true
    }

    /// Whether the current mission's own Approach ([`Self::approach_target`])
    /// takes `id`'s approach, so the pursuit pass (`tick_attack_pursuit`)
    /// leaves it alone: on a mission that calls Approach (Attack, Hunt,
    /// AreaGuard and Rescue), the Foot body ([`Self::owns_foot_approach`]) or
    /// the head's BalloonHover arm. A unit the head's crush arms carry
    /// ([`Self::unit_approach_head`]) holds a live Techno Target, so the Foot
    /// body's boundary covers it too. A hunting infantryman calls Approach
    /// from Mission_Hunt's no-flag arm (`0x004D54D2`); its Engineer and C4
    /// arms set the destination themselves (`0x004D53BA..0x004D5467`), so the
    /// mission decides his movement either way (`evaluate_foot_hunt`). Its
    /// VehicleThief arm (`0x004D546C..0x004D54B9`) is not ported, so the pass
    /// stands in for a `VehicleThief=` man. Keep it in step with those calls.
    pub(crate) fn mission_owns_approach(&self, id: u64, rules: &RuleSet) -> bool {
        let Some(actor) = self.substrate.entities.get(id) else {
            return false;
        };
        let calls_approach = match actor.mission.current().known() {
            Some(MissionType::Attack | MissionType::AreaGuard | MissionType::Rescue) => true,
            Some(MissionType::Hunt) => {
                actor.category != EntityCategory::Infantry
                    || !self
                        .object_type(actor.type_ref(), rules)
                        .is_some_and(|t| t.vehicle_thief)
            }
            _ => false,
        };
        calls_approach
            && (self.owns_foot_approach(id, rules)
                || self.balloon_hover_approach(id, rules).is_some())
    }

    /// Ownership boundary for the absorbed common Unit/Infantry approach. The
    /// BalloonHover arm ([`Self::balloon_hover_approach`]), non-Cell queued
    /// receivers and DestroyableCliffs redirect have separate native
    /// branches; their old adapter remains explicit.
    /// A mapless fixture cannot execute native cell/zone queries.
    pub(crate) fn owns_foot_approach(&self, id: u64, rules: &RuleSet) -> bool {
        let Some(actor) = self.substrate.entities.get(id) else {
            return false;
        };
        let Some(terrain) = self.resolved_terrain.as_ref() else {
            return false;
        };
        let Some(target) = actor.attack_target.as_ref().map(|a| a.target) else {
            return false;
        };
        // Unit7414E0 ends in Foot4D5690(false) at 741689, and Infantry522340
        // calls it at 52236E for any infantryman not in a deploy action
        // (522352..52236A; the deployed arm is `approach_infantry_deployed`),
        // whatever the locomotor: the body tells a Jumpjet apart itself.
        let represented = matches!(
            actor.category,
            EntityCategory::Unit | EntityCategory::Infantry
        ) && actor.locomotor.is_some()
            && !actor.infantry_deploy_doing()
            && match target {
                TargetKind::Cell(rx, ry) => !terrain.is_destroyable_cliff(rx, ry),
                TargetKind::Entity(target) => self.substrate.entities.contains(target),
            };
        represented
            && self.zone_grid.is_some()
            && self.object_type(actor.type_ref(), rules).is_some()
            && self.balloon_hover_approach(id, rules).is_none()
            && actor
                .navigation
                .nav_queue
                .iter()
                .all(|target| matches!(target, NavTargetRef::Cell { .. }))
    }

    /// Common Foot4D5690 body (Unit741689 and Infantry52236C pass false).
    /// Its only RNG is PlaceInfantryInCell's centre-row draw on a CloseRange
    /// infantryman's spot requests (the pre-search arm and each ring
    /// candidate); MissionAttack draws its cadence after this returns.
    pub(crate) fn approach_foot_target(
        &mut self,
        id: u64,
        rules: &RuleSet,
        registry: Option<&OverlayTypeRegistry>,
        frame_effects: FrameEffects<'_>,
    ) -> Result<Option<(u16, u16)>, String> {
        let Some(actor) = self.substrate.entities.get(id) else {
            return Ok(None);
        };
        let Some(target) = actor.attack_target.as_ref().map(|a| a.target) else {
            return Ok(None);
        };
        // 4D56A1..56B1: a Magnetron holding its target (+2AC == TarCom)
        // does not approach it.
        if matches!(target, TargetKind::Entity(target) if actor.locomotor_target == Some(target)) {
            return Ok(None);
        }
        let Some(selected) = combat::pursuit_selection(
            actor,
            &target,
            &self.substrate.entities,
            rules,
            &self.interner,
            self.resolved_terrain.as_ref(),
            Some(&self.house_alliances),
        ) else {
            return Ok(None);
        };
        let object = self
            .object_type(actor.type_ref(), rules)
            .ok_or("Approach requires mover type")?;
        let terrain = self
            .resolved_terrain
            .as_ref()
            .ok_or("Approach requires map cells")?;
        let identity = target.cell_identity(terrain);
        let target_reference = match target {
            TargetKind::Cell(rx, ry) => NavTargetRef::cell(rx, ry),
            TargetKind::Entity(id) => NavTargetRef::object(id),
        };
        let cells = NativeCellQuery::canonical(terrain);
        let mut range = combat::combat_weapon::weapon_range(
            actor,
            object,
            selected.index,
            &self.substrate.entities,
            rules,
            &self.interner,
        );
        if range >= 512 {
            range -= 128;
        }
        let los = combat::line_of_fire::LineOfFireInputs {
            overlay_grid: self.overlay_grid.as_ref(),
            overlay_registry: registry,
            alliances: Some(&self.fog.alliances),
        };
        let can_fire = if let Some(identity) = identity {
            in_range::cell_target_in_range(
                actor,
                identity,
                selected.weapon,
                rules,
                &self.interner,
                &self.substrate.entities,
                terrain,
                &los,
            )
            .ok_or("Approach CanFireAt coordinate failed")?
        } else {
            combat::can_fire_at_target_in_query(
                &self.substrate.entities,
                rules,
                &self.interner,
                id,
                &target,
                &cells,
                Some(&self.house_alliances),
                &los,
                Some(&target),
            )
        };
        let mission = actor.mission.current().known();
        let human = self
            .houses
            .get(&actor.owner())
            .is_none_or(|h| h.is_controlled_by_human(self.session.game_mode_nonzero));
        let may_approach = object.can_approach_target
            && actor.drain_target.is_none()
            && actor.bunker_link.installed_in().is_none()
            && !actor.passenger_role.in_open_transport();
        if !can_fire
            && (mission == Some(MissionType::Sticky)
                || !may_approach
                    && !(mission == Some(MissionType::Attack)
                        || human && mission == Some(MissionType::AreaGuard)
                        || !human && mission == Some(MissionType::Hunt)))
        {
            self.assign_target_represented(id, None, Some(rules), registry, frame_effects)
                .map_err(|e| format!("Approach target clear: {e}"))?;
            self.assign_null_destination(id, Some(rules), registry, frame_effects);
            return Ok(None);
        }

        // Read target+48 before NavCom+48. Neither retained Cell receiver is a
        // fresh coord-to-Cell query: a Dummy may already have been restamped.
        let cells = NativeCellQuery::canonical(terrain);
        let mut clear_nav = false;
        if let Some(nav) = actor.navigation.nav_com
            && object.can_recalc_approach_target
            && !can_fire
        {
            let own = self.approach_nav_center(target_reference, &cells)?;
            let nav = self.approach_nav_center(nav, &cells)?;
            clear_nav = distance_3d_leptons([own.x, own.y, own.z], [nav.x, nav.y, nav.z])
                > range.wrapping_mul(rules.general.approach_target_reset_multiplier);
        }
        if clear_nav {
            //4D59FB/5A01 are precisely FootStopMoving's two direct writes.
            // The independently retained locomotor head/destination stay live.
            super::navcom::foot_stop_moving(self.substrate.entities.get_mut(id).unwrap());
        }
        let actor = self.substrate.entities.get(id).unwrap();
        let terrain = self.resolved_terrain.as_ref().unwrap();
        let cells = NativeCellQuery::canonical(terrain);
        if actor.navigation.nav_com.is_some()
            && !super::air_movement::is_high_flying_in_query(
                actor,
                &cells,
                Some((rules, &self.interner)),
            )
            || can_fire && actor.in_playfield
        {
            return Ok(None);
        }
        if let Some(NavTargetRef::Cell { rx, ry }) = actor.navigation.nav_queue.first().copied() {
            if actor.category == EntityCategory::Unit {
                self.set_unit_destination(
                    id,
                    NavTargetRef::cell(rx, ry),
                    rules,
                    false,
                    frame_effects,
                );
            } else {
                //4D5A64 passes flag0. Infantry51AA40 forwards the flag,
                // but Foot4D94B0 never reads it; original ordinary CLEG
                // Cell-setter flag0/1 controls have equal state and RNG.
                self.assign_destination_represented(
                    id,
                    Some(NavTargetRef::cell(rx, ry)),
                    Some(rules),
                    registry,
                    frame_effects,
                )
                .map_err(|e| format!("Approach queued destination: {e}"))?;
            }
            let queue = &mut self
                .substrate
                .entities
                .get_mut(id)
                .unwrap()
                .navigation
                .nav_queue;
            if !queue.is_empty() {
                queue.remove(0);
            }
            return Ok(Some((rx, ry)));
        }

        let foundation = match target {
            TargetKind::Entity(id) => self
                .substrate
                .entities
                .get(id)
                .filter(|t| t.category == EntityCategory::Structure)
                .map(|t| {
                    let (width, height) =
                        crate::rules::foundation::foundation_dimensions(&t.foundation);
                    (i32::from(width), i32::from(height))
                }),
            TargetKind::Cell(..) => None,
        };
        let mut radius = approach_search_radius(range, actor.category, foundation);
        let own = self.approach_nav_center(target_reference, &cells)?;
        let target_cell = ((own.x / 256) as i16, (own.y / 256) as i16);
        let source = super::ground_pose::position_world_coord(&actor.position);
        let facing = facing16_between([own.x, own.y], [source.x, source.y]);
        let facing_byte = ((u32::from(facing) + 128) >> 8) as u8;
        //4D5CBC..5CFE: Cell uses its flag; a Foot uses OnBridge. A
        // building has no Foot flag, even when its coordinate lies on a bridge.
        let target_bridge = match target {
            TargetKind::Cell(..) => cells.flags(identity.unwrap()) & 0x100 != 0,
            TargetKind::Entity(id) => self.substrate.entities.get(id).is_some_and(|target| {
                target.category != EntityCategory::Structure && target.on_bridge
            }),
        };
        let category = actor.category;
        //4D5909..590D: a mover whose locomotor is Jumpjet flies there, so
        // it takes the first admitted candidate without either path-cost
        // test (4D6764 -> 4D689A). 4D5873 sets the same flag for an
        // aircraft, which native brings here from Foot's Mission_Rescue
        // (AircraftClass vt+0x258 = 4DDF90, Approach at 4DDFEB).
        // RESIDUAL: VERA's aircraft never run Rescue
        // (`aircraft::dispatch_mission`) and `owns_foot_approach` refuses
        // them, so that arm is not ported. Trigger: an aircraft on Rescue; VERA's one
        // Rescue producer, the base-defense response, queues it only on
        // infantry and units. Effect: none while no producer exists.
        let flies = actor
            .locomotor
            .as_ref()
            .is_some_and(|loco| loco.active_kind() == LocomotorKind::Jumpjet);
        let close_range = object.close_range;
        //4D5CFE..5D2F: a CloseRange= infantryman's radius under 332.8
        // (0x7E9248) becomes 332.
        if category == EntityCategory::Infantry && close_range {
            radius = radius.max(332);
        }
        let forced = close_range
            && self.approach_close_range_spot(id, target, own, rules, registry, frame_effects)?;
        let terrain = self.resolved_terrain.as_ref().unwrap();
        let cells = NativeCellQuery::canonical(terrain);
        //4D5F6F target+50 may resample ground and flags before any candidate.
        let _ = if let Some(identity) = identity {
            in_range::native_cell_range_coords(identity, &cells)
        } else {
            in_range::resolve_target_coords_3d(
                &target,
                &self.substrate.entities,
                rules,
                &self.interner,
                &cells,
            )
        }
        .ok_or("Approach target surface failed")?;
        let mut retained_candidate = target_cell;
        //4D5FE0: after the CloseRange= arm's spot the destination is the
        // Target's own cell, without the search (4D6892 -> 4D689A).
        let mut chosen = forced.then_some(target_cell);
        //4D6093..60F9: a CloseRange= infantryman's candidate is the spot
        // PlaceInfantryInCell picks in the radial cell for the Target's
        // coordinate; anyone else's is the cell centre (4D60FB..613A).
        let spots = category == EntityCategory::Infantry && close_range;
        'rings: while chosen.is_none() && radius > 204 {
            for offset in OFFSETS {
                let direction = (i16::from(facing_byte).wrapping_add(offset) as u16) << 8;
                let [x, y] = facing_step_world_xy([own.x, own.y], direction, radius);
                let radial = ((x / 256) as i16, (y / 256) as i16);
                let mut point = if spots {
                    let cell = NativeCellQuery::canonical(self.resolved_terrain.as_ref().unwrap())
                        .lookup_world(x, y);
                    // The empty coordinate is the origin.
                    self.place_infantry_spot_in_cell(rules, cell, own)?
                        .unwrap_or(DriveCoord { x: 0, y: 0, z: 0 })
                } else {
                    cell_centre(radial)
                };
                let cells = NativeCellQuery::canonical(self.resolved_terrain.as_ref().unwrap());
                point.z = in_range::native_ground_target_z(point.x, point.y, &cells)
                    .ok_or("Approach candidate surface failed")? as i32;
                if !self.approach_accepts(
                    id,
                    point,
                    &target,
                    selected.weapon,
                    close_range.then_some(own),
                    rules,
                    registry,
                )? {
                    continue;
                }
                retained_candidate = radial;
                if !cell_rect::cell_is_in_playfield_height_aware_in_query(
                    (i32::from(radial.0), i32::from(radial.1)),
                    self.playfield_bounds,
                    Some(cells.terrain()),
                    Some(&cells),
                ) {
                    continue;
                }
                let mut candidate = radial;
                let mut admitted = self.approach_admits(
                    id,
                    candidate,
                    close_range,
                    rules,
                    registry,
                    frame_effects,
                )?;
                //4D642D..6437: an infantryman goes on to the next candidate
                // without the neighbour walk.
                if !admitted && category != EntityCategory::Infantry {
                    for direction in 0..8 {
                        let (dx, dy) = cell_delta_unchecked(direction);
                        candidate = (
                            candidate.0.wrapping_add(dx as i16),
                            candidate.1.wrapping_add(dy as i16),
                        );
                        //4D643D..671E: cumulative neighbors, Z0, no new bounds
                        // or ground lookup. Destination setting resolves Z later.
                        if self.approach_accepts(
                            id,
                            cell_centre(candidate),
                            &target,
                            selected.weapon,
                            close_range.then_some(own),
                            rules,
                            registry,
                        )? && self.approach_admits(
                            id,
                            candidate,
                            close_range,
                            rules,
                            registry,
                            frame_effects,
                        )? {
                            admitted = true;
                            retained_candidate = candidate;
                            break;
                        }
                    }
                }
                if !admitted {
                    continue;
                }
                if flies {
                    chosen = Some(candidate);
                    break 'rings;
                }
                let actor = self
                    .substrate
                    .entities
                    .get(id)
                    .ok_or("Approach actor retired")?;
                let cells = NativeCellQuery::canonical(self.resolved_terrain.as_ref().unwrap());
                let candidate_bridge = cells.flags(cells.lookup(candidate)) & 0x100 != 0;
                if self.estimate_zone_cost(
                    id,
                    candidate,
                    target_cell,
                    candidate_bridge,
                    target_bridge,
                    rules,
                )? <= chebyshev(candidate, target_cell) + 8
                {
                    chosen = Some(candidate);
                    break 'rings;
                }
                let source_nav = self.foot_navigation_coordinate(id)?;
                let start = coord_cell(source_nav);
                let source_bridge = super::ground_pose::navigation_should_be_on_bridge(
                    &cells,
                    source_nav,
                    source,
                    actor.on_bridge,
                    actor.low_bridge_tube_state.is_some(),
                )?;
                if self.estimate_zone_cost(
                    id,
                    start,
                    candidate,
                    source_bridge,
                    candidate_bridge,
                    rules,
                )? <= chebyshev(start, candidate) + 8
                {
                    chosen = Some(candidate);
                    break 'rings;
                }
            }
            radius -= 256;
        }
        if let Some(cell) = chosen {
            self.assign_destination_represented(
                id,
                Some(NavTargetRef::cell(cell.0 as u16, cell.1 as u16)),
                Some(rules),
                registry,
                frame_effects,
            )
            .map_err(|e| format!("Approach destination: {e}"))?;
            //4D68B8 and4D68C7 perform separate lookups for setter and return.
            self.resolved_terrain
                .as_ref()
                .unwrap()
                .native_cell_identity(cell);
            return Ok(Some((cell.0 as u16, cell.1 as u16)));
        }
        //4D68E7..6919: a HunterSeeker, or a VehicleThief infantryman, takes
        // the Target itself as its destination (4D6A71, vt+0x480(TarCom, 1))
        // and returns it; the answer here is the cell under it. Neither key is
        // set in retail.
        if object.hunter_seeker || category == EntityCategory::Infantry && object.vehicle_thief {
            self.assign_destination_represented(
                id,
                Some(target_reference),
                Some(rules),
                registry,
                frame_effects,
            )
            .map_err(|e| format!("Approach Target destination: {e}"))?;
            return Ok(Some((target_cell.0 as u16, target_cell.1 as u16)));
        }
        let cells = NativeCellQuery::canonical(self.resolved_terrain.as_ref().unwrap());
        let fallback = self.approach_fallback(id, retained_candidate, rules, &cells)?;
        self.assign_target_represented(id, None, Some(rules), registry, frame_effects)
            .map_err(|e| format!("Approach exhausted target clear: {e}"))?;
        if let Some(cell) = fallback {
            self.assign_destination_represented(
                id,
                Some(NavTargetRef::cell(cell.0, cell.1)),
                Some(rules),
                registry,
                frame_effects,
            )
            .map_err(|e| format!("Approach fallback destination: {e}"))?;
        } else {
            self.assign_null_destination(id, Some(rules), registry, frame_effects);
        }
        Ok(None)
    }

    fn approach_nav_center(
        &self,
        target: NavTargetRef,
        cells: &NativeCellQuery<'_>,
    ) -> Result<DriveCoord, String> {
        let id = match target {
            NavTargetRef::Cell { rx, ry } => {
                let cell = TargetKind::Cell(rx, ry)
                    .cell_identity(cells.terrain())
                    .unwrap();
                let (x, y, z) = crate::sim::cell_kernel::native_cell_own_coords(cell, cells)
                    .ok_or("NavCom Cell coordinate failed")?;
                return Ok(DriveCoord {
                    x: x as i32,
                    y: y as i32,
                    z: z as i32,
                });
            }
            NavTargetRef::Entity { id }
            | NavTargetRef::Object { id }
            | NavTargetRef::Building { id } => id,
        };
        let entity = self
            .substrate
            .entities
            .get(id)
            .ok_or("retired Approach NavCom")?;
        Ok(super::ground_pose::object_get_coords(
            entity,
            Some(cells.terrain()),
        ))
    }

    /// The CloseRange= arm ahead of the search (`0x004D5D37..0x004D5F62`).
    /// When the mover's cell centre lies at most 384 (`0x007E9240`) from the
    /// Target's cell centre (the same cell or a neighbour), it asks for the
    /// spot in its own cell that the Target's coordinate `target_center`
    /// prefers ([`Self::place_infantry_spot_in_cell`]). A spot strictly nearer
    /// the Target's cell centre becomes its Locomotor's immediate destination
    /// ([`Self::foot_force_immediate_destination`]; both callers pass no-move
    /// false), and the body then sends it to the Target's cell. Answers
    /// whether it did.
    fn approach_close_range_spot(
        &mut self,
        id: u64,
        target: TargetKind,
        target_center: DriveCoord,
        rules: &RuleSet,
        registry: Option<&OverlayTypeRegistry>,
        frame_effects: FrameEffects<'_>,
    ) -> Result<bool, String> {
        use super::ground_pose::{position_world_coord, query_object_cell};
        use crate::sim::cell_kernel::{native_cell_own_xy, native_xy_distance};
        let terrain = self
            .resolved_terrain
            .as_ref()
            .ok_or("Approach requires map cells")?;
        let cells = NativeCellQuery::canonical(terrain);
        let location = position_world_coord(
            &self
                .substrate
                .entities
                .get(id)
                .ok_or("Approach actor retired")?
                .position,
        );
        //4D5D4F..5DEB: the centre (vt+0x48) of this->GetCell() (vt+0x1BC),
        // then of a Techno Target's GetCell(), or of a Cell Target itself.
        let own_centre = native_cell_own_xy(query_object_cell(&cells, location), &cells);
        let target_centre = match target {
            TargetKind::Entity(target) => {
                let target = self
                    .substrate
                    .entities
                    .get(target)
                    .ok_or("Approach Target retired")?;
                native_cell_own_xy(
                    query_object_cell(&cells, position_world_coord(&target.position)),
                    &cells,
                )
            }
            TargetKind::Cell(..) => native_cell_own_xy(
                target
                    .cell_identity(terrain)
                    .ok_or("Approach Cell Target")?,
                &cells,
            ),
        };
        //4D5DF2..5E80: each centre's Cell again (Map 565730), then the XY
        // distance between their centres through Sqrt_Approx and ftol.
        let [ax, ay] = native_cell_own_xy(cells.lookup_world(own_centre[0], own_centre[1]), &cells);
        let [bx, by] = native_cell_own_xy(
            cells.lookup_world(target_centre[0], target_centre[1]),
            &cells,
        );
        let reach = native_xy_distance(ax - bx, ay - by);
        //4D5E84..5E93: FCOMP 384.0, TEST AH,0x41 admits equality.
        if reach > 384 {
            return Ok(false);
        }
        //4D5E99..5EC6: PlaceInfantryInCell(out, Target->Center, 0, 0, 1) on
        // this->GetCell().
        let cell = query_object_cell(&cells, location);
        let spot = self.place_infantry_spot_in_cell(rules, cell, target_center)?;
        //4D5ECB..5F25: the spot's distance to the Target's cell centre, the
        // empty coordinate's from the origin.
        let [x, y] = spot.map_or([0, 0], |spot| [spot.x, spot.y]);
        if native_xy_distance(x - target_centre[0], y - target_centre[1]) >= reach {
            return Ok(false);
        }
        //4D5F2E..5F5F: Locomotor (+0x674) vt+0x78.
        self.foot_force_immediate_destination(id, spot, Some(rules), registry, frame_effects)?;
        Ok(true)
    }

    /// Whether an Approach candidate point will do: In_Range of it
    /// (`TechnoClass::In_Range @ 0x006F7220`, at `0x004D622C` and
    /// `0x004D6550`) or, for a CloseRange= mover (`close_range_target`, the
    /// Target's coordinate), an XY distance to the Target under 307.2
    /// (`0x007E9230`; `0x004D61A4..0x004D625C`, `0x004D64C0..0x004D6580`).
    fn approach_accepts(
        &self,
        id: u64,
        point: DriveCoord,
        target: &TargetKind,
        weapon: &crate::rules::weapon_type::WeaponType,
        close_range_target: Option<DriveCoord>,
        rules: &RuleSet,
        registry: Option<&OverlayTypeRegistry>,
    ) -> Result<bool, String> {
        let actor = self
            .substrate
            .entities
            .get(id)
            .ok_or("Approach actor retired")?;
        let cells = NativeCellQuery::canonical(
            self.resolved_terrain
                .as_ref()
                .ok_or("Approach requires map cells")?,
        );
        let in_range = in_range::compute_in_range_in_query(
            actor,
            (i64::from(point.x), i64::from(point.y), i64::from(point.z)),
            target,
            weapon,
            rules,
            &self.interner,
            &self.substrate.entities,
            &cells,
            &combat::line_of_fire::LineOfFireInputs {
                overlay_grid: self.overlay_grid.as_ref(),
                overlay_registry: registry,
                alliances: Some(&self.fog.alliances),
            },
        );
        // The ftol'd distance is an integer: under 307.2 is at most 307.
        Ok(in_range
            || close_range_target.is_some_and(|target| {
                crate::sim::cell_kernel::native_xy_distance(point.x - target.x, point.y - target.y)
                    <= 307
            }))
    }

    /// [`Self::approach_cell_is_clear`] for an Approach candidate. A
    /// CloseRange= mover is picked up around it: Mark(UP) and Mark(DOWN)
    /// (vt+0x124(0) and vt+0x124(1), `0x004D62C0..0x004D637D` and
    /// `0x004D6586..0x004D664D`).
    fn approach_admits(
        &mut self,
        id: u64,
        candidate: (i16, i16),
        close_range: bool,
        rules: &RuleSet,
        registry: Option<&OverlayTypeRegistry>,
        frame_effects: FrameEffects<'_>,
    ) -> Result<bool, String> {
        if close_range {
            self.foot_mark_remove(id, Some(rules), registry, frame_effects);
        }
        let cells = NativeCellQuery::canonical(
            self.resolved_terrain
                .as_ref()
                .ok_or("Approach requires map cells")?,
        );
        let clear = self.approach_cell_is_clear(id, candidate, rules, &cells);
        if close_range {
            self.foot_mark_put(id, Some(rules), registry, frame_effects);
        }
        clear
    }

    fn approach_cell_is_clear(
        &self,
        id: u64,
        candidate: (i16, i16),
        rules: &RuleSet,
        cells: &NativeCellQuery<'_>,
    ) -> Result<bool, String> {
        let actor = self
            .substrate
            .entities
            .get(id)
            .ok_or("Approach actor retired")?;
        let object = self
            .object_type(actor.type_ref(), rules)
            .ok_or("Approach mover type absent")?;
        let zones = self.zone_grid.as_ref().ok_or("Approach requires zones")?;
        let source = coord_cell(self.foot_navigation_coordinate(id)?);
        let zone = zones
            .get_zone_id_native_in_query(
                cells.terrain(),
                (source.0 as u16, source.1 as u16),
                object.movement_zone,
                actor.on_bridge,
                Some(cells),
            )
            .ok_or("Approach source zone absent")?;
        Ok(cell_rect::check_cell_passability(
            &CellRectPassabilityContext {
                native_cells: Some(cells),
                rect: CellRect {
                    x: i32::from(candidate.0),
                    y: i32::from(candidate.1),
                    width: 1,
                    height: 1,
                },
                speed_type: object.speed_type,
                required_zone_id: (zone != u32::MAX).then_some(zone),
                movement_zone: object.movement_zone,
                required_height_or_level: None,
                bridge_aware_zone: true,
                reject_any_overlay: false,
                path_grid: self.path_grid.as_deref(),
                resolved_terrain: Some(cells.terrain()),
                overlay_grid: self.overlay_grid.as_ref(),
                occupancy: Some(&self.substrate.occupancy),
                zone_grid: Some(zones),
            },
            Some(&self.substrate.raw_cell_occupation),
            i32::from(candidate.0),
            i32::from(candidate.1),
        ))
    }

    fn approach_fallback(
        &self,
        id: u64,
        retained: (i16, i16),
        rules: &RuleSet,
        cells: &NativeCellQuery<'_>,
    ) -> Result<Option<(u16, u16)>, String> {
        use crate::sim::find_nearby_cell::{
            NearbyAnchorGate, NearbyFootprint, NearbyQuery, PassabilityArgs,
            find_nearby_passable_cell, map_owned_radius_cap,
        };
        let actor = self
            .substrate
            .entities
            .get(id)
            .ok_or("Approach actor retired")?;
        let object = self
            .object_type(actor.type_ref(), rules)
            .ok_or("Approach mover type absent")?;
        let source = self.foot_navigation_coordinate(id)?;
        //4D6994 reads the retained candidate before sourceShouldBeOnBridge.
        let bridge_aware = cells.flags(cells.lookup(retained)) & 0x100 != 0;
        let should_bridge = super::ground_pose::navigation_should_be_on_bridge(
            cells,
            source,
            super::ground_pose::position_world_coord(&actor.position),
            actor.on_bridge,
            actor.low_bridge_tube_state.is_some(),
        )?;
        let source = coord_cell(source);
        let zone = self
            .zone_grid
            .as_ref()
            .ok_or("Approach requires zones")?
            .get_zone_id_native_in_query(
                cells.terrain(),
                (source.0 as u16, source.1 as u16),
                object.movement_zone,
                should_bridge,
                Some(cells),
            )
            .ok_or("Approach source zone absent")?;
        let (bounds, height) = self
            .playfield_bounds
            .zip(self.playfield_size_height)
            .ok_or("Approach requires map dimensions")?;
        Ok(find_nearby_passable_cell(
            (i32::from(retained.0), i32::from(retained.1)),
            &NearbyQuery {
                native_cells: Some(cells),
                raw_occupation: Some(&self.substrate.raw_cell_occupation),
                passability: PassabilityArgs {
                    speed_type: object.speed_type,
                    required_zone_id: (zone != u32::MAX).then_some(zone),
                    movement_zone: object.movement_zone,
                    bridge_aware_zone: bridge_aware,
                },
                footprint: NearbyFootprint::SINGLE,
                anchor_gate: NearbyAnchorGate::NativeHeightAware,
                allow_bridge_cells: false,
                check_height: true,
                check_occupancy: false,
                radius_cap: map_owned_radius_cap(bounds.base, height),
                target_cell: None,
                path_grid: self.path_grid.as_deref(),
                resolved_terrain: Some(cells.terrain()),
                overlay_grid: self.overlay_grid.as_ref(),
                occupancy: Some(&self.substrate.occupancy),
                entities: Some(&self.substrate.entities),
                zone_grid: self.zone_grid.as_ref(),
                playfield_bounds: Some(bounds),
            },
            self.session.binary_frame,
        )
        .filter(|cell| *cell != (0, 0)))
    }
}

/// Foot4D5B97..4D5BEE: building dimensions expand the search radius before
/// Infantry RTTI15 subtracts281.6; the other represented class subtracts179.2.
/// Integral inputs permit integer-only truncation toward zero, preserving
/// CRT ftol's consumed low i32 and the wrapping foundation addition.
/// Executed controls: fv_cell_attack/cleg_approach_vectors.json (original CRT,
/// both ambient precision controls); no host floating-point math is needed.
fn approach_search_radius(
    range: i32,
    class: EntityCategory,
    foundation: Option<(i32, i32)>,
) -> i32 {
    let range = foundation.map_or(range, |(width, height)| {
        range.wrapping_add(width.wrapping_add(height).wrapping_mul(64))
    });
    let whole = if class == EntityCategory::Infantry {
        281
    } else {
        179
    };
    // Original ftol truncates toward zero into i64, then consumes its low
    // i32. For a negative intermediate the fractional part truncates up;
    // the low word can wrap at i32::MIN (native corpus boundary controls).
    let subtraction = whole + i32::from(range > whole);
    let radius = range.wrapping_sub(subtraction).max(0);
    if radius > 0 { radius.max(256) } else { 0 }
}

/// The BalloonHover arm once SelectWeapon has answered
/// (`0x007415BA..0x007415F8`): a selected weapon whose `Projectile=` (+0xA0)
/// is `Vertical=` (+0x2C0) makes the Target the destination; no weapon, no
/// projectile or a level one goes on to the next arm.
fn balloon_hover_destination(
    weapon: Option<&crate::rules::weapon_type::WeaponType>,
    target: TargetKind,
    rules: &RuleSet,
) -> Option<NavTargetRef> {
    weapon?
        .projectile
        .as_deref()
        .and_then(|name| rules.projectile(name))
        .is_some_and(|projectile| projectile.vertical)
        .then_some(match target {
            TargetKind::Entity(id) => NavTargetRef::object(id),
            TargetKind::Cell(rx, ry) => NavTargetRef::cell(rx, ry),
        })
}
