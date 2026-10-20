//! Boris' AirstrikeClass lifecycle, represented by an owner and ordered aircraft IDs.
//!
//! Native: constructor41D380, ShouldUpdateTarget41D7E0, SetTarget41D830,
//! Start41D860, Reset41DA20, Clear41DB40, PointerExpired41D540, Remove41DC80.
//! Source: original gamemd.exe SHA256
//! 1cdd1180e49024fbda8ad568caac2e86e856063ff67ab38f62b7d2c7bb84298c.
//! These control-flow bodies draw no RNG. Aircraft construction/placement and
//! tint stepping use their shared owners. The manager has no per-frame AI.

use crate::map::entities::EntityCategory;
use crate::rules::airstrike::AirstrikeSettings;
use crate::rules::ruleset::RuleSet;
use crate::sim::combat::TargetKind;
use crate::sim::combat::veterancy::VeterancyRank;
use crate::sim::game_entity::GameEntity;
use crate::sim::mission::{MissionId, MissionType};
use crate::sim::world::{FrameEffects, SimSoundEvent, Simulation};
use serde::{Deserialize, Serialize};
use std::hash::{Hash, Hasher};

/// Techno+294 has one identity: an owned manager or a borrowed aircraft/marker
/// link. Its manager ID is the owning Techno's stable ID, not a second entity.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct AirstrikeState {
    link: Option<u64>,
    manager: Option<AirstrikeManager>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct AirstrikeManager {
    settings: AirstrikeSettings,
    aircraft: Vec<u64>,
    remaining: i32,
    busy: bool,
    active: bool,
    target: Option<u64>,
    /// Native +40 is undefined before the first completed sortie; no admission
    /// reads it consequentially then because the initial target differs.
    returned_at: Option<i32>,
}

impl AirstrikeState {
    pub(crate) fn owns_manager(&self) -> bool {
        self.manager.is_some()
    }
    pub(crate) fn referenced_ids(&self) -> impl Iterator<Item = u64> + '_ {
        self.link.into_iter().chain(
            self.manager
                .iter()
                .flat_map(|m| m.target.into_iter().chain(m.aircraft.iter().copied())),
        )
    }
    pub(crate) fn hash_state(&self, h: &mut impl Hasher) {
        if self.link.is_none() && self.manager.is_none() {
            return;
        }
        b"airstrike-294".hash(h);
        self.link.hash(h);
        if let Some(m) = &self.manager {
            m.settings.normal_count.hash(h);
            m.settings.elite_count.hash(h);
            m.settings.normal_type.hash(h);
            m.settings.elite_type.hash(h);
            m.settings.normal_recharge.hash(h);
            m.settings.elite_recharge.hash(h);
            m.aircraft.hash(h);
            m.remaining.hash(h);
            m.busy.hash(h);
            m.active.hash(h);
            m.target.hash(h);
            m.returned_at.hash(h);
        }
    }
    #[cfg(any(test, feature = "test-support"))]
    pub fn set_test_link(&mut self, link: Option<u64>) {
        self.link = link;
    }
}

impl GameEntity {
    pub(crate) fn airstrike_link(&self) -> Option<u64> {
        self.airstrike.link
    }
    pub fn has_airstrike_marker(&self) -> bool {
        self.airstrike.link.is_some()
    }
}

impl Simulation {
    /// InitManagers6F41C0: only positive normal team count creates a manager.
    pub(crate) fn initialize_airstrike(&mut self, owner: u64, rules: &RuleSet) {
        let Some(settings) = self
            .substrate
            .entities
            .get(owner)
            .and_then(|e| self.object_type(e.type_ref(), rules))
            .map(|o| o.airstrike.clone())
            .filter(|s| s.normal_count > 0)
        else {
            return;
        };
        let e = self.substrate.entities.get_mut(owner).unwrap();
        e.airstrike.link = Some(owner);
        e.airstrike.manager = Some(AirstrikeManager {
            settings,
            aircraft: Vec::new(),
            remaining: 0,
            busy: false,
            active: false,
            target: None,
            returned_at: None,
        });
    }

    fn airstrike_manager(&self, owner: u64) -> Option<&AirstrikeManager> {
        self.substrate
            .entities
            .get(owner)?
            .airstrike
            .manager
            .as_ref()
    }

    /// Bullet469705..469789 admits any Techno target through the firer's link.
    /// Refusal still consumes the special-impact arm; no damage is substituted.
    pub(crate) fn airstrike_detonation(
        &mut self,
        firer: u64,
        target: Option<u64>,
        rules: &RuleSet,
        effects: FrameEffects<'_>,
    ) {
        let Some(target) = target.filter(|id| self.substrate.entities.contains(*id)) else {
            return;
        };
        let Some(owner) = self
            .substrate
            .entities
            .get(firer)
            .and_then(GameEntity::airstrike_link)
        else {
            return;
        };
        let Some(m) = self.airstrike_manager(owner) else {
            return;
        };
        let elite = self
            .substrate
            .entities
            .get(owner)
            .unwrap()
            .veterancy
            .rank_kind()
            == VeterancyRank::Elite;
        let recharge = if elite {
            m.settings.elite_recharge
        } else {
            m.settings.normal_recharge
        };
        let ready = !m.busy
            && m.returned_at
                .is_some_and(|at| at.wrapping_add(recharge) <= self.session.binary_frame() as i32);
        if !ready && m.target == Some(target) {
            return;
        }
        if m.busy {
            self.airstrike_reset_target(owner, target, rules, effects);
        } else {
            self.airstrike_start(owner, target, rules, effects);
        }
    }

    fn airstrike_start(
        &mut self,
        owner: u64,
        target: u64,
        rules: &RuleSet,
        effects: FrameEffects<'_>,
    ) {
        let elite = self
            .substrate
            .entities
            .get(owner)
            .unwrap()
            .veterancy
            .rank_kind()
            == VeterancyRank::Elite;
        let m = self.substrate.airstrike_manager_mut(owner).unwrap();
        m.busy = true;
        m.active = true;
        let (kind, count) = if elite {
            (m.settings.elite_type.clone(), m.settings.elite_count)
        } else {
            (m.settings.normal_type.clone(), m.settings.normal_count)
        };
        let Some(kind) = kind else {
            return;
        };
        let spawned = self.create_airstrike_aircraft(owner, target, &kind, count, rules, effects);
        let first = spawned.aircraft.first().copied();
        let m = self.substrate.airstrike_manager_mut(owner).unwrap();
        m.remaining = spawned.count;
        m.aircraft = spawned.aircraft;
        if spawned.count == 0 {
            return;
        }
        m.target = Some(target);
        if let Some(first) = first {
            self.airstrike_sound(
                first,
                rules.general.airstrike_attack_voice.as_deref(),
                None,
                Some(rules),
                effects,
            );
        }
        self.airstrike_mark_target(owner, target);
    }

    fn airstrike_mark_target(&mut self, owner: u64, target: u64) {
        let frame = self.session.binary_frame() as i32;
        if let Some(e) = self
            .substrate
            .entities
            .get_mut(target)
            .filter(|e| e.category() == EntityCategory::Structure)
        {
            e.airstrike.link = Some(owner);
            e.airstrike_tint.start(frame, 100000);
        }
    }

    fn airstrike_reset_target(
        &mut self,
        owner: u64,
        target: u64,
        rules: &RuleSet,
        effects: FrameEffects<'_>,
    ) {
        let m = self.airstrike_manager(owner).unwrap();
        let aircraft = m.aircraft.clone();
        let old_target = m.target;
        let remaining = m.remaining.max(0) as usize;
        let ammo = |id| {
            self.substrate
                .entities
                .get(id)
                .and_then(|e| e.ammo())
                .map_or(0, |a| a.current())
        };
        if aircraft
            .first()
            .is_some_and(|&first| aircraft.iter().any(|&id| ammo(id) != ammo(first)))
        {
            self.airstrike_clear_target(owner, Some(rules), effects);
            return;
        }
        if let Some(old) = old_target
            .and_then(|id| self.substrate.entities.get_mut(id))
            .filter(|e| e.category() == EntityCategory::Structure)
        {
            old.airstrike.link = None;
            old.airstrike_tint.stop(self.session.binary_frame() as i32);
        }
        self.substrate.airstrike_manager_mut(owner).unwrap().target = None;
        self.airstrike_mark_target(owner, target);
        self.substrate.airstrike_manager_mut(owner).unwrap().target = Some(target);
        for aircraft in aircraft.into_iter().take(remaining) {
            if self
                .substrate
                .entities
                .get(aircraft)
                .and_then(|e| e.ammo())
                .is_some_and(|a| a.current() > 0)
            {
                let _ = self.assign_target_represented(
                    aircraft,
                    Some(TargetKind::Entity(target)),
                    Some(rules),
                    None,
                    effects,
                );
                let _ = self.mission_queue_exact(
                    aircraft,
                    MissionId::from_known(MissionType::Attack),
                    0,
                    self.session.binary_frame(),
                    &crate::sim::mission::authority::EntityReadyInputProvider,
                );
                self.substrate.airstrike_manager_mut(owner).unwrap().active = true;
            }
        }
    }

    /// Clear41DB40 leaves busy/count/list intact until aircraft removal.
    pub(crate) fn airstrike_clear_target(
        &mut self,
        owner: u64,
        rules: Option<&RuleSet>,
        effects: FrameEffects<'_>,
    ) {
        let Some(m) = self.airstrike_manager(owner) else {
            return;
        };
        let aircraft = m.aircraft.clone();
        let target = m.target;
        let empty = aircraft.iter().any(|&id| {
            self.substrate
                .entities
                .get(id)
                .and_then(|e| e.ammo())
                .is_some_and(|a| a.current() == 0)
        });
        if empty {
            for &id in &aircraft {
                if let Some(ammo) = self
                    .substrate
                    .entities
                    .get_mut(id)
                    .and_then(|e| e.ammo_mut())
                {
                    ammo.empty();
                }
            }
        }
        for &id in aircraft.iter().take(101) {
            let _ = self.mission_queue_exact(
                id,
                MissionId::from_known(MissionType::Retreat),
                0,
                self.session.binary_frame(),
                &crate::sim::mission::authority::EntityReadyInputProvider,
            );
            let _ = self.assign_target_represented(id, None, rules, None, effects);
            let _ = self.assign_destination_represented(id, None, rules, None, effects);
        }
        self.substrate.airstrike_manager_mut(owner).unwrap().active = false;
        if let Some(target) = target.filter(|&id| {
            self.substrate.entities.get(id).is_some_and(|e| {
                e.category() == EntityCategory::Structure && e.airstrike.link == Some(owner)
            })
        }) {
            // Native registry order equals manager construction order. Each
            // owner creates its manager in its constructor, before Unlimbo.
            let replacement = self
                .substrate
                .entities
                .iter_sorted()
                .filter_map(|(id, e)| {
                    (id != owner
                        && e.airstrike
                            .manager
                            .as_ref()
                            .is_some_and(|m| m.target == Some(target)))
                    .then_some(id)
                })
                .next_back();
            self.substrate
                .entities
                .get_mut(target)
                .unwrap()
                .airstrike
                .link = replacement;
        }
        self.substrate.airstrike_manager_mut(owner).unwrap().target = None;
    }

    fn airstrike_sound(
        &mut self,
        source: u64,
        sound: Option<&str>,
        audible_to: Option<[crate::sim::intern::InternedId; 2]>,
        rules: Option<&RuleSet>,
        effects: FrameEffects<'_>,
    ) {
        if let (Some(sound), Some(entity)) = (sound, self.substrate.entities.get(source)) {
            self.sound_events.emit(SimSoundEvent::voc_at_for(
                sound.to_owned(),
                audible_to,
                &entity.position,
            ));
            effects.flush_sound(self, rules);
        }
    }

    /// Remove41DC80 clears only this aircraft link; last removal starts recharge.
    fn airstrike_remove_aircraft(&mut self, owner: u64, aircraft: u64) {
        let frame = self.session.binary_frame() as i32;
        let Some(m) = self.substrate.airstrike_manager_mut(owner) else {
            return;
        };
        if let Some(index) = m.aircraft.iter().take(102).position(|&id| id == aircraft) {
            m.aircraft.remove(index);
            m.remaining = m.remaining.wrapping_sub(1);
            if m.remaining <= 0 {
                m.busy = false;
                m.active = false;
                m.returned_at = Some(frame);
            }
            if let Some(e) = self.substrate.entities.get_mut(aircraft) {
                e.airstrike.link = None;
            }
        }
    }

    /// Techno707B39 forwards only its owned manager, for either expiry control.
    pub(crate) fn airstrike_pointer_expired(
        &mut self,
        owner: u64,
        expired: u64,
        rules: Option<&RuleSet>,
        effects: FrameEffects<'_>,
    ) {
        let Some(m) = self.airstrike_manager(owner) else {
            return;
        };
        if self
            .substrate
            .entities
            .get(owner)
            .and_then(GameEntity::airstrike_link)
            != Some(owner)
        {
            return;
        }
        if owner == expired {
            let aircraft = m.aircraft.clone();
            if m.active
                && let Some(first) = aircraft.first().copied()
                && self
                    .substrate
                    .entities
                    .get(owner)
                    .and_then(|e| self.houses.get(&e.owner()))
                    .is_some_and(|h| h.is_human())
            {
                // IsHumanPlayer50B6F0 in skirmish means this machine's local
                // house. The common sound boundary resolves that audience;
                // another human player's Boris must not play our abort voice.
                let house = self.substrate.entities.get(owner).unwrap().owner();
                self.airstrike_sound(
                    first,
                    rules.and_then(|r| r.general.airstrike_abort_sound.as_deref()),
                    Some([house, house]),
                    rules,
                    effects,
                );
            }
            self.airstrike_clear_target(owner, rules, effects);
            for id in aircraft {
                self.airstrike_remove_aircraft(owner, id);
            }
        } else if self
            .substrate
            .entities
            .get(expired)
            .and_then(GameEntity::airstrike_link)
            == Some(owner)
            && self
                .substrate
                .entities
                .get(expired)
                .is_some_and(|e| e.category() != EntityCategory::Structure)
        {
            self.airstrike_remove_aircraft(owner, expired);
        } else if self.substrate.entities.get(expired).is_some_and(|e| {
            e.category() == EntityCategory::Structure
                && e.airstrike.link == Some(owner)
                && (!e.is_object_alive() || e.health.current() < 1)
        }) {
            self.airstrike_clear_target(owner, rules, effects);
        }
    }

    pub(crate) fn airstrike_detach_preserves_target(&self, listener: u64, target: u64) -> bool {
        self.substrate
            .entities
            .get(listener)
            .and_then(GameEntity::airstrike_link)
            .and_then(|id| self.airstrike_manager(id))
            .is_some_and(|m| m.target == Some(target))
            && self
                .substrate
                .entities
                .get(target)
                .is_some_and(|e| e.is_object_alive() && e.health.current() > 0)
    }

    /// Tactical6D47FB..6D48D4 reads the owning manager and current Building
    /// target, irrespective of active/busy or remaining aircraft. Source is
    /// GetFLH(0,zero); target is GetCoords(+48), without TargetCoordOffset.
    /// Presentation owns the planning/Health gate and clock-driven wobble.
    /// No RNG draw, timer write or detach; isolate the shared fallback Cell.
    pub fn airstrike_designation_coordinates(
        &self,
        owner: u64,
        rules: &RuleSet,
    ) -> Option<[crate::sim::components::DriveCoord; 2]> {
        use crate::sim::combat::fire_coord::{FireSource, fire_coordinate};
        use crate::sim::movement::ground_pose::object_get_coords;
        let source = self.substrate.entities.get(owner)?;
        if source.airstrike.link != Some(owner) {
            return None;
        }
        let target = self
            .substrate
            .entities
            .get(source.airstrike.manager.as_ref()?.target?)?;
        if target.category() != EntityCategory::Structure {
            return None;
        }
        let obj = self.object_type(source.type_ref(), rules)?;
        self.effective_shared_cell_dummy().detached(|| {
            let from = fire_coordinate(
                self,
                rules,
                &FireSource::of_entity(source),
                obj,
                0,
                source.weapon_burst.index() as u8,
                crate::rules::flh::Flh::default(),
            )
            .coord;
            Some([
                crate::sim::components::DriveCoord {
                    x: from.x,
                    y: from.y,
                    z: from.z,
                },
                object_get_coords(target, self.resolved_terrain.as_ref()),
            ])
        })
    }

    pub fn airstrike_tint_active(&self, target: u64) -> bool {
        self.substrate
            .entities
            .get(target)
            .filter(|e| e.category() == EntityCategory::Structure)
            .and_then(GameEntity::airstrike_link)
            .and_then(|id| self.airstrike_manager(id))
            .is_some_and(|m| m.target == Some(target))
    }
}

impl crate::sim::world::ObjectSubstrate {
    pub(crate) fn link_airstrike_aircraft(&mut self, owner: u64, aircraft: u64) {
        self.airstrike_manager_mut(owner)
            .expect("strike owner")
            .aircraft
            .insert(0, aircraft);
        self.entities
            .get_mut(aircraft)
            .expect("constructed aircraft")
            .airstrike
            .link = Some(owner);
    }
    fn airstrike_manager_mut(&mut self, owner: u64) -> Option<&mut AirstrikeManager> {
        self.entities.get_mut(owner)?.airstrike.manager.as_mut()
    }
}

#[cfg(test)]
#[path = "airstrike_tests.rs"]
mod tests;
