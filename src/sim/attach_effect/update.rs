//! The per-frame AttachEffect update on the simulation: discard conditions,
//! the AI walk with expiry and animations, firing and damage records, owner
//! change and limbo handling.

use crate::rules::attach_effect::{AttachEffectType, DiscardOn, ExpireWeaponCondition};
use crate::rules::phobos_shields::AffectedHouse;
use crate::sim::world::Simulation;

use super::{
    AttachAnimState, AttachEffectInstance, AttachEffectWeaponShot, affects_house, cpp_i32,
    range_modifier_source, range_with_attach_effects, scale_i32,
};

/// The per-frame target facts `AttachEffectClass::ShouldBeDiscardedNow`
/// reads.
#[cfg(feature = "phobos")]
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct DiscardFacts {
    pub is_foot: bool,
    pub is_building: bool,
    pub is_moving: bool,
    pub is_really_moving: bool,
    pub harvesting: bool,
    pub mission_harvest_on_tiberium: bool,
    pub selling: bool,
    pub undeploying: bool,
    pub draining: bool,
    pub ammo: i32,
    /// The target's current and type-maximum health, read by
    /// [`crate::sim::combat::health_in_threshold`].
    pub health: i32,
    pub strength: i32,
    pub land_type: Option<crate::rules::terrain_rules::LandType>,
    pub mission: Option<crate::rules::mission_data::MissionType>,
    pub sequence: Option<crate::rules::animation_sequence::SequenceKind>,
    /// The held target's 3-D distance and the selected weapon's effective
    /// range in leptons (`-1` without a weapon).
    pub target: Option<(i32, i32)>,
    pub owner_human: bool,
}

#[cfg(feature = "phobos")]
impl AttachEffectInstance {
    /// `ShouldBeDiscardedNow`: the event flag or any continuous condition the
    /// type lists. RESIDUAL: `Drain` waits on a victim-side drain owner.
    pub(crate) fn should_be_discarded_now(
        &self,
        effect: &AttachEffectType,
        facts: &DiscardFacts,
        globals: &crate::rules::attach_effect::AttachEffectGlobals,
    ) -> bool {
        use crate::rules::attach_effect::DiscardOn;
        if self.should_be_discarded {
            return true;
        }
        let flags = effect.discard_on;
        if flags.is_none() {
            return false;
        }
        if facts.is_foot {
            let by_destination = effect
                .discard_move_based_on_destination
                .unwrap_or(globals.discard_move_based_on_destination);
            let moving = if by_destination {
                facts.is_moving
            } else {
                facts.is_really_moving
            };
            if moving {
                if flags.contains(DiscardOn::MOVE) {
                    return true;
                }
            } else if effect
                .discard_consider_harvesting_as_stationary
                .unwrap_or(globals.discard_consider_harvesting_as_stationary)
            {
                if flags.contains(DiscardOn::STATIONARY) {
                    return true;
                }
            } else if facts.harvesting {
                if flags.contains(DiscardOn::HARVESTING) {
                    return true;
                }
            } else if facts.mission_harvest_on_tiberium {
                // The intermediate state that is about to start harvesting is
                // neither harvesting nor stationary this frame.
                return false;
            } else if flags.contains(DiscardOn::STATIONARY) {
                return true;
            }
        }
        if facts.is_building && facts.selling {
            if facts.undeploying {
                if flags.contains(DiscardOn::UNDEPLOYING) {
                    return true;
                }
            } else if flags.contains(DiscardOn::SELLING) {
                return true;
            }
        }
        if facts.draining && flags.contains(DiscardOn::DRAIN) {
            return true;
        }
        if flags.contains(DiscardOn::AMMO) {
            let min = effect.discard_ammo_minimum;
            let max = effect.discard_ammo_maximum;
            if (min < 0 || facts.ammo >= min) && (max < 0 || facts.ammo <= max) {
                return true;
            }
        }
        if flags.contains(DiscardOn::HEALTH)
            && crate::sim::combat::health_in_threshold(
                facts.health,
                facts.strength,
                effect.discard_health_above,
                effect.discard_health_below,
            )
        {
            return true;
        }
        if flags.contains(DiscardOn::LAND_TYPE) && !effect.discard_land_types.is_empty() {
            if let Some(land) = facts.land_type {
                if effect
                    .discard_land_types
                    .iter()
                    .any(|token| token.eq_ignore_ascii_case(land.section_name()))
                {
                    return true;
                }
            }
        }
        if flags.contains(DiscardOn::MISSION) {
            // A human-controlled owner uses `DiscardOn.Missions`; an AI uses
            // `DiscardOn.AIMissions` when it is set.
            let missions = if facts.owner_human {
                &effect.discard_missions
            } else {
                effect
                    .discard_ai_missions
                    .as_ref()
                    .unwrap_or(&effect.discard_missions)
            };
            if let Some(mission) = facts.mission {
                if missions
                    .iter()
                    .any(|name| mission.ini_section().eq_ignore_ascii_case(name))
                {
                    return true;
                }
            }
        }
        if flags.contains(DiscardOn::SEQUENCE) && !effect.discard_sequences.is_empty() {
            if let Some(current) = facts.sequence {
                let matches = |kind| {
                    effect.discard_sequences.iter().any(|token| {
                        crate::rules::animation_sequence::SequenceKind::from_native_name(token)
                            == Some(kind)
                    })
                };
                let immediate = effect
                    .discard_sequences_immediate
                    .unwrap_or(globals.discard_sequences_immediate);
                if immediate {
                    if matches(current) {
                        return true;
                    }
                } else if let Some(last) = self.last_sequence_check
                    && last != current
                    && matches(last)
                {
                    return true;
                }
            }
        }
        if flags.contains(DiscardOn::IN_RANGE) || flags.contains(DiscardOn::OUT_OF_RANGE) {
            if let Some((distance, weapon_range)) = facts.target {
                let range = effect
                    .discard_range_override
                    .map_or(weapon_range, |cells| cpp_i32(cells * 256.0));
                if (flags.contains(DiscardOn::IN_RANGE) && distance <= range)
                    || (flags.contains(DiscardOn::OUT_OF_RANGE) && distance >= range)
                {
                    return true;
                }
            }
        }
        false
    }
}

/// One attach-effect instance's reads for this frame's update, kept at the
/// instance's own index.
#[cfg(feature = "phobos")]
#[derive(Clone, Copy, Default)]
struct InstanceFrame {
    /// Initialized before this frame's `AI()` tick.
    initialized_before: bool,
    /// `IsActiveIgnorePowered()` after the tick.
    active: bool,
    /// `IsActiveIgnorePowered() && ShouldBeDiscardedNow()` after the tick.
    discarded: bool,
    anim_state: AttachAnimState,
    can_show: bool,
    /// The one instance of a cumulative type that shows its animation.
    designated: bool,
}

/// Where an effect's animation lives: attached to `owner` at its world point,
/// drawn under `house`, and paused or running with the effect's state.
#[cfg(feature = "phobos")]
struct AttachEffectAnimTarget {
    owner: u64,
    house: crate::sim::intern::InternedId,
    world: crate::sim::anim_class::AnimWorldCoord,
    paused: bool,
}

#[cfg(feature = "phobos")]
impl Simulation {
    /// Phobos `TechnoExt::UpdateAttachEffects`
    /// (`src/Ext/Techno/Body.Update.cpp:895..975`, reached from the
    /// `TechnoClass_AI_0x6F9E50` early update): every effect runs one `AI()`
    /// phase, expired effects are removed, and each surviving effect
    /// reconciles its `AnimClass` with `CanShowAnim`.
    ///
    /// RESIDUALS: `DiscardOn` belongs to its own slice, so nothing is discarded
    /// here yet; native `IsImmobilized` is approximated by the chrono-warp and
    /// transport-interior gates VERA models; `CloakCheck` and the tunnel
    /// animation state are not ported; a cumulative type's animation is
    /// rebuilt when the layer count changes instead of going through
    /// `AnimExt::ChangeAnimType`, so `CumulativeAnimations.RestartOnChange=no`
    /// still restarts; `Animation.ResetOnReapply` does not restart a refreshed
    /// effect's animation.
    /// `ShouldBeDiscardedNow`'s target facts for one entity, read once per
    /// frame before the AI walk. `needs_range` skips the weapon selection
    /// when no effect on the object reads inrange/outofrange.
    fn phobos_discard_facts(
        &self,
        id: u64,
        rules: &crate::rules::ruleset::RuleSet,
        needs_range: bool,
    ) -> DiscardFacts {
        let Some(entity) = self.substrate.entities.get(id) else {
            return DiscardFacts::default();
        };
        use crate::map::entities::EntityCategory;
        let category = entity.category();
        let is_foot = matches!(
            category,
            EntityCategory::Unit | EntityCategory::Infantry | EntityCategory::Aircraft
        );
        let is_building = category == EntityCategory::Structure;
        let frame = self.session.binary_frame();
        let (is_moving, is_really_moving) = if is_foot {
            (
                crate::sim::movement::motion_query::is_moving(entity).unwrap_or(false),
                crate::sim::movement::motion_query::is_really_moving_now(
                    entity,
                    crate::sim::movement::SpeedRules::new(rules, &self.interner, &self.houses),
                    frame,
                ),
            )
        } else {
            (false, false)
        };
        // `InfantryClass::SequenceAnim` follows the current Doing; VERA maps
        // the stored action id through the shared sequence table.
        let sequence = (category == EntityCategory::Infantry)
            .then(|| {
                entity
                    .mission_leaf
                    .as_infantry()
                    .and_then(|leaf| crate::rules::infantry_sequence::action_kind(leaf.doing()))
            })
            .flatten();
        let target = if needs_range {
            entity.attack_target.target().and_then(|target| {
                let target_coords = match target {
                    crate::sim::combat::TargetKind::Entity(id) => {
                        let target = self.substrate.entities.get(id)?;
                        let coord = crate::sim::movement::ground_pose::position_world_coord(
                            &target.position,
                        );
                        [coord.x, coord.y, coord.z]
                    }
                    crate::sim::combat::TargetKind::Cell(rx, ry) => {
                        let floor_z = self
                            .resolved_terrain
                            .as_ref()
                            .and_then(|terrain| terrain.cell(rx, ry))
                            .map_or(0, |cell| {
                                i32::from(cell.level as i8)
                                    .wrapping_mul(crate::util::lepton::GROUND_LEVEL_HEIGHT_LEPTONS)
                            });
                        let centre = crate::sim::cell_kernel::cell_center(
                            crate::sim::cell_kernel::CellCoordinate {
                                x: i32::from(rx),
                                y: i32::from(ry),
                            },
                            floor_z,
                        );
                        [centre.x, centre.y, centre.z]
                    }
                };
                let source =
                    crate::sim::movement::ground_pose::position_world_coord(&entity.position);
                let distance = crate::util::native_x87::distance_3d_leptons(
                    [source.x, source.y, source.z],
                    target_coords,
                );
                let object = self.interner.object_type(entity.type_ref(), rules)?;
                let (_, selected) = crate::sim::combat::combat_weapon::select_weapon_against(
                    rules,
                    entity,
                    object,
                    &crate::sim::combat::combat_weapon::attacker_facts(entity, object),
                    Some(&target),
                    self,
                    &self.interner,
                    self.resolved_terrain.as_ref(),
                    Some(&self.house_alliances),
                )?;
                let range = selected.map_or(-1, |selected| {
                    let source = range_modifier_source(
                        entity,
                        Some(&self.substrate.entities),
                        rules,
                        &self.interner,
                    );
                    range_with_attach_effects(
                        source,
                        &selected.weapon.id,
                        selected.weapon.range_leptons,
                        rules,
                    )
                });
                Some((distance, range))
            })
        } else {
            None
        };
        let land_type = self.resolved_terrain.as_ref().and_then(|terrain| {
            terrain
                .cell(entity.position.rx(), entity.position.ry())
                .and_then(|cell| crate::rules::terrain_rules::LandType::from_index(cell.land_type))
        });
        let mission = entity.mission.current().known();
        let harvesting = is_foot
            && (entity
                .miner
                .as_ref()
                .is_some_and(|miner| miner.is_harvesting())
                || entity.animation.as_ref().is_some_and(|animation| {
                    animation.sequence() == crate::rules::animation_sequence::SequenceKind::Shovel
                }));
        let mission_harvest_on_tiberium = mission
            == Some(crate::rules::mission_data::MissionType::Harvest)
            && land_type == Some(crate::rules::terrain_rules::LandType::Tiberium);
        let selling =
            is_building && mission == Some(crate::rules::mission_data::MissionType::Selling);
        let strength = self
            .interner
            .object_type(entity.type_ref(), rules)
            .map_or(0, |object| object.strength);
        let health = entity.health.current();
        DiscardFacts {
            is_foot,
            is_building,
            is_moving,
            is_really_moving,
            harvesting,
            mission_harvest_on_tiberium,
            selling,
            undeploying: selling && entity.archive_target().is_some(),
            // `TechnoClass+0x1D0 DrainingMe`, the Floating Disc victim side
            // owned by [`crate::sim::credit_income::Drain`].
            draining: entity.drain.draining_me().is_some(),
            ammo: entity.ammo().map_or(0, |ammo| ammo.current()),
            health,
            strength,
            land_type,
            mission,
            sequence,
            target,
            // `HouseClass::IsControlledByHuman @ 0x0050B730`, the predicate
            // the mission list reads.
            owner_human: self
                .houses
                .get(&entity.owner())
                .is_some_and(|house| house.is_controlled_by_human(self.session.rule_policy())),
        }
    }

    /// Phobos `TechnoClass_FireAt_BeforeTruelyFire` (`0x6FDDC0`, 6 bytes
    /// replaced): every admitted shot counts towards `DiscardOn.Firing.Count`,
    /// including spawner, drain and suicide weapons that launch no bullet.
    pub(crate) fn record_phobos_attach_firing(
        &mut self,
        id: u64,
        rules: &crate::rules::ruleset::RuleSet,
    ) {
        let Some(entity) = self.substrate.entities.get(id) else {
            return;
        };
        if entity.phobos_attach_effects().is_empty() {
            return;
        }
        let effects = rules.attach_effect_types().types();
        let mut list = std::mem::take(
            self.substrate
                .entities
                .get_mut(id)
                .expect("the gated firer still exists")
                .phobos_attach_effects_mut(),
        );
        for instance in &mut list.instances {
            let Some(effect) = effects.get(instance.type_index) else {
                continue;
            };
            if !effect.discard_on.contains(DiscardOn::FIRING) {
                continue;
            }
            instance.firing_count += 1;
            if instance.firing_count >= effect.discard_firing_count {
                instance.should_be_discarded = true;
            }
        }
        *self
            .substrate
            .entities
            .get_mut(id)
            .expect("the gated firer still exists")
            .phobos_attach_effects_mut() = list;
    }

    /// Phobos' ReceiveDamage wrapper: a hit whose source house passes
    /// `DiscardOn.ReceivedDamage.AffectsHouse` counts towards the type's
    /// `DiscardOn.ReceivedDamage.Count`.
    pub(crate) fn record_phobos_attach_damage(
        &mut self,
        id: u64,
        source_house: Option<crate::sim::intern::InternedId>,
        rules: &crate::rules::ruleset::RuleSet,
    ) {
        let Some(entity) = self.substrate.entities.get(id) else {
            return;
        };
        if entity.phobos_attach_effects().is_empty() {
            return;
        }
        let owner = entity.owner();
        let effects = rules.attach_effect_types().types();
        let mut list = std::mem::take(
            self.substrate
                .entities
                .get_mut(id)
                .expect("the gated target still exists")
                .phobos_attach_effects_mut(),
        );
        for instance in &mut list.instances {
            let Some(effect) = effects.get(instance.type_index) else {
                continue;
            };
            if !effect.discard_on.contains(DiscardOn::RECEIVED_DAMAGE) {
                continue;
            }
            // `CanTargetHouse(flags, pThis->Owner, pSourceHouse)`: the target's
            // owner is the "owner" argument and the source house the target.
            let gated = match source_house {
                Some(source) => affects_house(
                    effect.discard_received_damage_affects_house,
                    Some(owner),
                    source,
                    &self.houses,
                    &self.house_alliances,
                    &self.interner,
                ),
                None => effect
                    .discard_received_damage_affects_house
                    .contains(AffectedHouse::ENEMIES),
            };
            if !gated {
                continue;
            }
            instance.received_damage_count += 1;
            if instance.received_damage_count >= effect.discard_received_damage_count {
                instance.should_be_discarded = true;
            }
        }
        *self
            .substrate
            .entities
            .get_mut(id)
            .expect("the gated target still exists")
            .phobos_attach_effects_mut() = list;
    }

    /// Phobos' House owner-change hook: every effect with
    /// `DiscardOn=ownerchange` is flagged when the human/computer direction
    /// and the two per-type switches admit it, except a revert driven by a
    /// transport exit. RESIDUAL: VERA has no `Passengers.SyncOwner`
    /// revert-on-exit caller, so `IgnoreRevertOnExit` is always false here.
    pub(crate) fn phobos_attach_effects_on_owner_change(
        &mut self,
        id: u64,
        old_owner: crate::sim::intern::InternedId,
        new_owner: crate::sim::intern::InternedId,
        rules: &crate::rules::ruleset::RuleSet,
    ) {
        let Some(entity) = self.substrate.entities.get(id) else {
            return;
        };
        if entity.phobos_attach_effects().is_empty() {
            return;
        }
        let old_human = self
            .houses
            .get(&old_owner)
            .is_some_and(|house| house.is_controlled_by_human(self.session.rule_policy()));
        let new_human = self
            .houses
            .get(&new_owner)
            .is_some_and(|house| house.is_controlled_by_human(self.session.rule_policy()));
        let human_and_computer = old_human != new_human;
        let has_transporter = entity.passenger_role().inside_transport_id().is_some();
        let effects = rules.attach_effect_types().types();
        let mut list = std::mem::take(
            self.substrate
                .entities
                .get_mut(id)
                .expect("the gated entity still exists")
                .phobos_attach_effects_mut(),
        );
        for instance in &mut list.instances {
            let Some(effect) = effects.get(instance.type_index) else {
                continue;
            };
            if !effect.discard_on.contains(DiscardOn::OWNER_CHANGE) {
                continue;
            }
            let human_to_computer = effect.discard_owner_change_human_to_computer;
            let computer_to_human = effect.discard_owner_change_computer_to_human;
            let ignore_revert = false;
            let mut discard = false;
            if effect.discard_owner_change_ignore_revert_on_exit && ignore_revert {
                discard = false;
            } else if human_to_computer && computer_to_human {
                discard = true;
            } else if human_to_computer || computer_to_human {
                if human_and_computer
                    && ((old_human && human_to_computer) || (!old_human && computer_to_human))
                {
                    discard = true;
                }
            }
            if discard && has_transporter && !ignore_revert {
                discard = false;
            }
            if discard {
                instance.should_be_discarded = true;
            }
        }
        *self
            .substrate
            .entities
            .get_mut(id)
            .expect("the gated entity still exists")
            .phobos_attach_effects_mut() = list;
    }

    /// Phobos `TechnoClass_Limbo_Wrapper` (`Ext/Techno/Hooks.cpp:400..472`,
    /// call/vtable replacements): every effect with `DiscardOn=Entry` is reset
    /// or removed when the object enters limbo, except on an undeploying
    /// building (`UndeploysInto=` while its mission is Selling at status 2).
    /// A removed effect queues its `Discard` expire weapon; the stat
    /// multipliers are recalculated, then the weapons detonate at the
    /// object's coordinates before the original Limbo body. A reset effect
    /// keeps its `ShouldBeDiscarded` flag, as native does.
    pub(crate) fn limbo_phobos_attach_effects(
        &mut self,
        id: u64,
        rules: &crate::rules::ruleset::RuleSet,
        overlay_registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
        frame_effects: &mut crate::sim::world::FrameEffects<'_>,
    ) {
        let Some(entity) = self.substrate.entities.get(id) else {
            return;
        };
        if entity.phobos_attach_effects().is_empty() {
            return;
        }
        if entity.category() == crate::map::entities::EntityCategory::Structure {
            let undeploying = self
                .interner
                .object_type(entity.type_ref(), rules)
                .is_some_and(|object| object.undeploys_into.is_some())
                && entity.mission.current().known()
                    == Some(crate::rules::mission_data::MissionType::Selling)
                && entity.mission.handler_state() == 2;
            if undeploying {
                return;
            }
        }
        let owner_house = entity.owner();
        let effects = rules.attach_effect_types().types();
        let mut list = std::mem::take(
            self.substrate
                .entities
                .get_mut(id)
                .expect("the gated entity still exists")
                .phobos_attach_effects_mut(),
        );
        let mut expire_shots = Vec::new();
        let mut index = 0;
        while index < list.instances.len() {
            let type_index = list.instances[index].type_index;
            let Some(effect) = effects
                .get(type_index)
                .filter(|effect| effect.discard_on.contains(DiscardOn::ENTRY))
            else {
                index += 1;
                continue;
            };
            if let Some(anim) = list.instances[index].anim.take() {
                self.destroy_anim_with_context(anim, rules);
            }
            if list.instances[index].reset_if_recreatable() {
                index += 1;
                continue;
            }
            let active_count = list.active_count(type_index);
            if let Some(shot) = list.instances[index].expire_weapon_shot(
                effect,
                ExpireWeaponCondition::DISCARD,
                id,
                owner_house,
                active_count,
                false,
            ) {
                expire_shots.push(shot);
            }
            list.instances.remove(index);
        }
        let entity = self
            .substrate
            .entities
            .get_mut(id)
            .expect("the gated entity still exists");
        *entity.phobos_attach_effects_mut() = list;
        entity.recalculate_phobos_stat_multipliers(effects);
        if !expire_shots.is_empty() {
            let detonations: Vec<_> = expire_shots
                .iter()
                .filter_map(|shot| self.attach_effect_weapon_detonation(shot, id, rules))
                .collect();
            self.commit_attach_effect_detonations(
                &detonations,
                rules,
                overlay_registry,
                frame_effects,
            );
        }
    }

    /// [`Self::update_phobos_attach_effects_with`] without a caller frame
    /// boundary: fixtures that observe the list directly.
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn update_phobos_attach_effects(
        &mut self,
        id: u64,
        rules: &crate::rules::ruleset::RuleSet,
    ) {
        self.update_phobos_attach_effects_with(
            id,
            rules,
            &mut crate::sim::world::FrameEffects::empty(),
            None,
        );
    }

    pub(crate) fn update_phobos_attach_effects_with(
        &mut self,
        id: u64,
        rules: &crate::rules::ruleset::RuleSet,
        frame_effects: &mut crate::sim::world::FrameEffects<'_>,
        overlay_registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
    ) {
        let Some(entity) = self.substrate.entities.get(id) else {
            return;
        };
        // `InLimbo || IsImmobilized`: an object that cannot act keeps its
        // effects untouched. Chrono warp and transport interior stand in for
        // the native immobilised states VERA models.
        if entity.lifecycle.in_limbo()
            || entity.chrono_warp_latch()
            || entity.passenger_role().inside_transport_id().is_some()
            || entity.phobos_attach_effects().is_empty()
        {
            return;
        }
        // `OnlineCheck`: Deactivated, EMP, and a building's power-online latch.
        let owner_active = !entity.deactivated.is_offline()
            && entity.emp_remaining() <= 0
            && (entity.category() != crate::map::entities::EntityCategory::Structure
                || entity.building_online());
        let owner_house = entity.owner();
        let position = crate::sim::movement::ground_pose::position_world_coord(&entity.position);
        let world = crate::sim::anim_class::AnimWorldCoord {
            x: position.x,
            y: position.y,
            z: position.z,
        };
        let mut list = std::mem::take(
            self.substrate
                .entities
                .get_mut(id)
                .expect("the gated entity still exists")
                .phobos_attach_effects_mut(),
        );
        let effects = rules.attach_effect_types().types();
        let globals = *rules.attach_effect_globals();
        let needs_range = list.instances().iter().any(|instance| {
            effects.get(instance.type_index).is_some_and(|effect| {
                effect.discard_on.contains(DiscardOn::IN_RANGE)
                    || effect.discard_on.contains(DiscardOn::OUT_OF_RANGE)
            })
        });
        let facts = self.phobos_discard_facts(id, rules, needs_range);
        list.online_check_all(effects, owner_active);
        // `PointerGotInvalid` flags an `InvokerDie` effect whose invoker is
        // gone. VERA checks the store each frame instead of the native
        // pointer-expiry broadcast, so a target that ran earlier in the
        // invoker's death frame sees it one frame later.
        for instance in &mut list.instances {
            let Some(effect) = effects.get(instance.type_index) else {
                continue;
            };
            if effect.discard_on.contains(DiscardOn::INVOKER_DIE)
                && let Some(invoker) = instance.source.and_then(|source| source.invoker)
                && self.substrate.entities.get(invoker).is_none()
            {
                instance.should_be_discarded = true;
            }
        }
        // One record per instance, at the instance's index; the removal below
        // drops both together so they stay aligned. Built per object that
        // holds effects, as long as its effect list; only the `phobos` build
        // runs this.
        let mut records: Vec<InstanceFrame> = list
            .instances
            .iter()
            .map(|instance| InstanceFrame {
                initialized_before: instance.has_initialized(),
                ..InstanceFrame::default()
            })
            .collect();
        // `AI()` reads `ShouldBeDiscardedNow()` at its delay and expiry
        // branches after initialization, so the tick evaluates it lazily
        // there instead of taking a value cached before the frame.
        list.tick_all(effects, |_, instance, effect| {
            instance.should_be_discarded_now(effect, &facts, &globals)
        });
        // `IsActiveIgnorePowered() && ShouldBeDiscardedNow()`
        // (`Body.Update.cpp:940`) runs after `AI()`; the per-frame native
        // LastDiscardCheckFrame cache makes this value equal to the tick's.
        for (record, instance) in records.iter_mut().zip(&list.instances) {
            record.active = instance.is_active_ignoring_power(instance.is_self_owned());
            record.discarded = record.active
                && effects.get(instance.type_index).is_some_and(|effect| {
                    instance.should_be_discarded_now(effect, &facts, &globals)
                });
        }
        // `LastSequenceCheck` advances only when the frame reached the
        // sequence block: an earlier condition returned first.
        for (record, instance) in records.iter().zip(list.instances.iter_mut()) {
            if !record.active || record.discarded {
                continue;
            }
            let Some(effect) = effects.get(instance.type_index) else {
                continue;
            };
            if !effect.discard_on.contains(DiscardOn::SEQUENCE) {
                continue;
            }
            match facts.sequence {
                Some(current) => {
                    if !effect.discard_sequences.is_empty()
                        && !effect
                            .discard_sequences_immediate
                            .unwrap_or(globals.discard_sequences_immediate)
                    {
                        instance.last_sequence_check = Some(current);
                    }
                }
                None => instance.last_sequence_check = None,
            }
        }
        // `AI()`'s initialization block: `ROFMultiplier.ApplyOnCurrentTimer`
        // scales the timer already running when the effect first activates.
        let frame = self.session.binary_frame() as i32;
        for (record, instance) in records.iter().zip(&list.instances) {
            if record.initialized_before || !instance.has_initialized() {
                continue;
            }
            let Some(effect) = effects.get(instance.type_index) else {
                continue;
            };
            if effect.rof_multiplier <= 0.0 || !effect.rof_multiplier_apply_on_current_timer {
                continue;
            }
            let Some(entity) = self.substrate.entities.get_mut(id) else {
                continue;
            };
            let remaining = entity.seldom.rearm_timer.remaining(frame);
            if remaining > 0 {
                entity
                    .seldom
                    .rearm_timer
                    .start(frame, scale_i32(remaining, effect.rof_multiplier));
            }
        }
        // `HasExpired || (IsActiveIgnorePowered && ShouldBeDiscardedNow)`:
        // a discardable effect behind a recreation delay is reset instead of
        // erased; the destructor unlinks the animation either way.
        // Reverse order keeps the records aligned with the shifting list.
        let mut expire_shots: Vec<AttachEffectWeaponShot> = Vec::new();
        for index in (0..list.instances.len()).rev() {
            let expired = list.instances[index].has_expired();
            let discard = records[index].discarded;
            if !(expired || discard) {
                continue;
            }
            // `AddExpireWeaponParams` runs before the reset/erase, and its
            // cumulative check reads the count at this moment.
            let type_index = list.instances[index].type_index;
            if let Some(effect) = effects.get(type_index) {
                let condition = if expired {
                    ExpireWeaponCondition::EXPIRE
                } else {
                    ExpireWeaponCondition::DISCARD
                };
                let active_count = list.active_count(type_index);
                if let Some(shot) = list.instances[index].expire_weapon_shot(
                    effect,
                    condition,
                    id,
                    owner_house,
                    active_count,
                    false,
                ) {
                    expire_shots.push(shot);
                }
            }
            if discard {
                list.instances[index].clear_discarded();
                if list.instances[index].reset_if_recreatable() {
                    if let Some(anim) = list.instances[index].anim.take() {
                        self.destroy_anim_with_context(anim, rules);
                    }
                    continue;
                }
            }
            if let Some(anim) = list.instances[index].anim.take() {
                self.destroy_anim_with_context(anim, rules);
            }
            list.instances.remove(index);
            records.remove(index);
        }
        // The removal walks backwards; the shots keep native's attach order.
        expire_shots.reverse();
        // `UpdateConditionalAnimDrawingLogic` reacts to other effects on the
        // same object, so every effect re-evaluates it each frame.
        for (record, instance) in records.iter_mut().zip(&list.instances) {
            let Some(effect) = effects.get(instance.type_index) else {
                continue;
            };
            // Temporal state is not tracked yet, so only the power gate applies.
            record.anim_state = instance.animation_state(effect, false);
            let hidden =
                list.has_active_type(&effect.animation_hide_if_attached_with, effects, false);
            record.can_show = instance.animation_phase() && record.anim_state.show && !hidden;
        }
        // One cumulative type shows one animation no matter how many layers it
        // has: `UpdateCumulativeAttachEffects` keeps the instance with the
        // longest remaining duration (any negative duration wins, last seen).
        for index in 0..records.len() {
            let type_index = list.instances[index].type_index;
            if !records[index].can_show
                || !effects.get(type_index).is_some_and(|effect| {
                    effect.cumulative && !effect.cumulative_animations.is_empty()
                })
            {
                continue;
            }
            let mut best = None;
            let mut duration = 0;
            for (other_index, other) in list.instances.iter().enumerate() {
                if other.type_index != type_index || !records[other_index].can_show {
                    continue;
                }
                let current = other.remaining();
                if current < 0 || current > duration {
                    best = Some(other_index);
                    duration = current;
                }
            }
            records[index].designated = best == Some(index);
        }
        for instance_index in 0..list.instances.len() {
            let type_index = list.instances[instance_index].type_index;
            let Some(effect) = effects.get(type_index) else {
                continue;
            };
            let record = records[instance_index];
            let desired = if record.can_show {
                if effect.cumulative && !effect.cumulative_animations.is_empty() {
                    let count = list
                        .instances
                        .iter()
                        .filter(|other| other.type_index == type_index)
                        .filter(|other| other.is_active(other.is_self_owned()))
                        .count() as i32;
                    if record.designated {
                        effect.cumulative_animation(count)
                    } else {
                        None
                    }
                } else {
                    effect.animation.as_deref()
                }
            } else {
                None
            };
            let house = if effect.animation_use_invoker_as_owner {
                list.instances[instance_index]
                    .source
                    .and_then(|source| source.invoker)
                    .and_then(|invoker| self.substrate.entities.get(invoker).map(|e| e.owner()))
                    .unwrap_or(owner_house)
            } else {
                owner_house
            };
            self.maintain_phobos_attach_effect_anim(
                &mut list.instances[instance_index],
                desired,
                AttachEffectAnimTarget {
                    owner: id,
                    house,
                    world,
                    paused: record.anim_state.paused,
                },
                rules,
            );
        }
        let entity = self
            .substrate
            .entities
            .get_mut(id)
            .expect("the gated entity still exists");
        *entity.phobos_attach_effects_mut() = list;
        entity.recalculate_phobos_stat_multipliers(effects);
        // `UpdateAttachEffects` fires one weapon per expired/discarded effect
        // at the object's own coordinates, after stats and animations.
        if !expire_shots.is_empty() {
            let detonations: Vec<_> = expire_shots
                .iter()
                .filter_map(|shot| self.attach_effect_weapon_detonation(shot, id, rules))
                .collect();
            self.commit_attach_effect_detonations(
                &detonations,
                rules,
                overlay_registry,
                frame_effects,
            );
        }
    }

    /// `CreateAnim` / `KillAnim`: keep exactly `desired` alive on the effect.
    /// Phobos creates the animation at the owner's location, attaches it with
    /// `SetOwnerObject`, and holds its loops; a changed cumulative type is
    /// rebuilt here because `AnimExt::ChangeAnimType` has no VERA owner yet.
    fn maintain_phobos_attach_effect_anim(
        &mut self,
        instance: &mut AttachEffectInstance,
        desired: Option<&str>,
        target: AttachEffectAnimTarget,
        rules: &crate::rules::ruleset::RuleSet,
    ) {
        let AttachEffectAnimTarget {
            owner,
            house,
            world,
            paused,
        } = target;
        if let Some(anim) = instance.anim {
            let keeps = desired.is_some_and(|name| {
                self.substrate.anims.get(anim).is_some_and(|live| {
                    self.interner
                        .resolve(live.type_id())
                        .eq_ignore_ascii_case(name)
                })
            });
            if !keeps {
                self.destroy_anim_with_context(anim, rules);
                instance.anim = None;
            }
        }
        if instance.anim.is_none()
            && let Some(name) = desired
        {
            let (rx, ry, sub_x, sub_y, z) = world.to_cell_sub_z();
            let type_name = self.interner.intern(name);
            let descriptor = crate::sim::components::AnimClassSpawnDescriptor {
                draw_flags: 0x600,
                owner: Some(owner),
                ..crate::sim::components::AnimClassSpawnDescriptor::new(
                    type_name, rx, ry, sub_x, sub_y, z,
                )
            };
            match self.spawn_anim_at_world(rules, descriptor, world) {
                Ok(anim) => {
                    self.substrate
                        .anims
                        .get_mut(anim)
                        .expect("the new effect animation")
                        .hold_loops();
                    self.substrate.set_anim_owner_house(anim, house);
                    instance.anim = Some(anim);
                }
                Err(error) => log::debug!(
                    "attach effect animation [{name}] on {owner} did not construct: {error}"
                ),
            }
        }
        if let Some(anim) = instance.anim
            && let Some(live) = self.substrate.anims.get_mut(anim)
        {
            if paused {
                live.pause();
            } else {
                live.resume();
            }
        }
    }
}
