//! Ordinary ground-victim and independent powered-building Ares EMP.
//! The Techno+504 counter and held Anim have one private owner; the existing powered-unit owner keeps +1C8.
//! Installed Bullet46920B ->100536E0/10062320 and AI6FAF0D ->10053DF0.
//! Evidence/transport bounds: tools/rules_oracle/ares_emp.{py,json,meta.json};
//! full chain ownership and residuals: docs/ares-emp.md.
use super::GameEntity;
use std::hash::{Hash, Hasher};

#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub(super) struct EmpState {
    remaining: i32,
    sparkle: Option<u64>,
    last_mission: i32,
}
impl Default for EmpState {
    fn default() -> Self {
        // Installed TechnoExt constructor10042D20/10042D27.
        Self {
            remaining: 0,
            sparkle: None,
            last_mission: -1,
        }
    }
}
impl GameEntity {
    pub(crate) fn emp_remaining(&self) -> i32 {
        self.emp.remaining
    }
    #[cfg(any(test, feature = "test-support"))]
    pub fn set_emp_remaining_for_test(&mut self, remaining: i32) {
        self.emp.remaining = remaining;
    }
    pub(crate) fn emp_sparkle(&self) -> Option<u64> {
        self.emp.sparkle
    }
    pub(crate) fn emp_anim_pointer_expired(&mut self, expired: u64) {
        if self.emp.sparkle == Some(expired) {
            self.emp.sparkle = None;
        }
    }
    pub(crate) fn hash_emp_state<H: Hasher>(&self, hasher: &mut H) {
        if self.emp != EmpState::default() {
            b"techno-emp-v1".hash(hasher);
            self.emp.hash(hasher);
        }
    }
}

#[cfg(feature = "ares")]
mod receiver {
    use super::*;
    use crate::map::entities::EntityCategory;
    use crate::rules::{
        locomotor_type::LocomotorKind, ruleset::RuleSet, warhead_type::WarheadType,
    };
    use crate::sim::{
        anim_class::AnimWorldCoord,
        components::AnimClassSpawnDescriptor,
        mission::MissionType,
        world::{FrameEffects, Simulation},
    };
    use crate::util::native_x87::{MaskedX87Chop53 as X87, NativeF64Bits};

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    enum EmpDelivery {
        Rejected,
        Applied,
        /// Deliberate later mechanisms; never represented by a generic offline bool.
        Unsupported,
    }
    /// Installed10062476..100624B4; signed32 addition wraps before max0.
    fn capped_duration(old: i32, delta: i32, cap: i32) -> i32 {
        let sum = old.wrapping_add(delta);
        // Preserve10062476..A7's separate signed branches: moving max/min
        // across this wrapping add changes overflow and zero-cap removal.
        let new = if delta > 0 {
            if cap < 0 {
                old.max(delta)
            } else if cap == 0 {
                sum
            } else {
                old.max(sum.min(cap))
            }
        } else if cap < 0 {
            sum
        } else {
            sum.min(cap)
        };
        new.max(0)
    }
    fn duration_delta(duration: i32, modifier: NativeF64Bits) -> i32 {
        X87::fistp_i32_masked(X87::mul(
            X87::load_i32(duration),
            X87::load_f64(if duration > 0 {
                modifier
            } else {
                NativeF64Bits::ONE
            }),
        ))
    }
    fn advisable(entity: &GameEntity) -> bool {
        // Installed10062D60 calls vt184: original5B3040 returns current+AC,
        // or queued+B4 when current is NONE. LastMission separately saves rawAC.
        !matches!(
            entity.mission.effective().known(),
            Some(MissionType::Unload | MissionType::Selling | MissionType::Construction)
        )
    }
    /// Supported Building dependencies, not a replacement immunity predicate.
    /// Factories/providers and linked/managed effects need their own complete chains.
    fn ordinary_building(object: &crate::rules::object_type::ObjectType) -> bool {
        !object.has_factory()
            && !object.radar
            && !object.spy_sat
            && !object.gap_generator
            && object.super_weapon.is_none()
            && object.super_weapon2.is_none()
            && object.powers_unit.is_none()
            && !object.laser_fence
            && !object.laser_fence_post
            && !object.cloning
            && !object.construction_yard
            && object.undeploys_into.is_none()
            && !object.can_be_occupied
            && !object.overpowerable
    }
    /// Installed10035C26..10035D74 ring/row cursor. The first ring differs
    /// from gamemd's static AreaDamage table, and this cursor extends to256.
    fn visit_emp_cells(center: (i16, i16), radius: i32, mut visit: impl FnMut((i16, i16))) {
        visit(center);
        for ring in 1..=radius {
            let (mut x, mut y) = (-1i32, -ring);
            let (mut advance, mut mirror_advance) = (true, true);
            loop {
                visit((
                    center.0.wrapping_add(x as i16),
                    center.1.wrapping_add(y as i16),
                ));
                if x == 1 && y == ring {
                    break;
                }
                if advance {
                    x += 1;
                    advance = x == 0;
                } else if x < 0 {
                    x = -x;
                    advance = mirror_advance;
                } else {
                    y += 1;
                    let magnitude = y.abs();
                    x = -2 * (ring - magnitude) - 1;
                    let edge = x + magnitude;
                    advance = edge >= 0;
                    if edge < 0 {
                        x = if edge == -1 {
                            x + 1
                        } else {
                            -ring + magnitude / 2
                        };
                    }
                    mirror_advance = advance;
                }
            }
        }
    }
    /// Native100767F2 squares signed wrapped XYZ differences, FSQRTs and
    /// compares against CellSpread*256. Compare squared nonnegative lengths
    /// instead: ordinary map deltas have exact integer squares in binary64,
    /// avoid integer sqrt truncation, and are deterministic on every target.
    /// Native FSQRT rounding at arbitrary fractional boundaries is a residual;
    /// native corpus pins exact zero, half-cell and nonzero-Z controls.
    fn emp_ground_in_range(
        impact: crate::sim::projectile::ProjectileCoord,
        coord: [i32; 3],
        spread: f64,
    ) -> bool {
        let delta = [
            coord[0].wrapping_sub(impact.x),
            coord[1].wrapping_sub(impact.y),
            coord[2].wrapping_sub(impact.z),
        ];
        let cutoff = spread * 256.0;
        if cutoff < 0.0 || cutoff.is_nan() {
            return false;
        }
        let square = delta.map(|value| {
            let value = f64::from(value);
            value * value
        });
        square[0] + square[1] + square[2] <= cutoff * cutoff
    }
    impl Simulation {
        /// Installed Bullet46920B calls this pre-effect even when Damage=0.
        /// The bounded producer collects the ground CellClass lists. Airborne
        /// victims and their half-distance collector remain a later mechanism.
        pub(crate) fn apply_ares_emp_detonation(
            &mut self,
            impact: crate::sim::projectile::ProjectileCoord,
            source: Option<u64>,
            warhead: &WarheadType,
            rules: &RuleSet,
            effects: FrameEffects<'_>,
        ) {
            if !warhead.ares_rules().is_some_and(|emp| emp.duration() != 0) {
                return;
            }
            for target in self.ares_effect_ground_targets(impact, warhead.cell_spread_f64, rules) {
                let result = self.deliver_ares_emp(target, source, warhead, rules, effects);
                if result == EmpDelivery::Unsupported {
                    log::debug!("Ares EMP victim {target} requires a later dependency chain");
                }
            }
        }
        pub(crate) fn ares_effect_ground_targets(
            &self,
            impact: crate::sim::projectile::ProjectileCoord,
            spread: f64,
            rules: &RuleSet,
        ) -> Vec<u64> {
            use crate::map::{cell_index::NativeCellIdentity, resolved_terrain::NativeCellQuery};
            use crate::sim::movement::{ground_pose, locomotor::MovementLayer};
            let radius = (X87::fistp_i32_masked(X87::add(
                X87::load_f64(NativeF64Bits::from_bits(spread.to_bits())),
                X87::load_f64(NativeF64Bits::from_bits(0.99f64.to_bits())),
            )) as u32)
                .min(256) as i32;
            let cells = self
                .resolved_terrain
                .as_ref()
                .map(NativeCellQuery::canonical);
            let center = crate::sim::movement::coord_cell(crate::sim::components::DriveCoord {
                x: impact.x,
                y: impact.y,
                z: impact.z,
            });
            let mut candidates = Vec::new();
            visit_emp_cells(center, radius, |requested| {
                let (rx, ry, layer) = if let Some(cells) = cells.as_ref() {
                    //10035B2A..10035B48 indexes the fixed cell array and
                    // skips null, rather than stamping MapClass's Dummy.
                    let Some(index) = cells
                        .terrain()
                        .native_fixed_cell_index(requested.0, requested.1)
                    else {
                        return;
                    };
                    let cell = NativeCellIdentity::Real(index);
                    let (x, y) = cells.coord(cell);
                    (
                        x as u16,
                        y as u16,
                        if cells.flags(cell) & 0x100 != 0 {
                            MovementLayer::Bridge
                        } else {
                            MovementLayer::Ground
                        },
                    )
                } else {
                    (
                        requested.0 as u16,
                        requested.1 as u16,
                        MovementLayer::Ground,
                    )
                };
                if let Some(occupancy) = self.substrate.occupancy.get(rx, ry) {
                    for occupant in occupancy.iter_layer(layer) {
                        let Some(entity) = self.substrate.entities.get(occupant.entity_id) else {
                            continue;
                        };
                        if entity.category() == EntityCategory::Structure
                            && self
                                .interner
                                .object_type(entity.type_ref(), rules)
                                .is_some_and(|kind| kind.invisible_in_game)
                        {
                            continue;
                        }
                        let Some(coord) = ground_pose::target_get_coords(
                            crate::sim::combat::TargetKind::Entity(occupant.entity_id),
                            self,
                            cells.as_ref(),
                        ) else {
                            continue;
                        };
                        if emp_ground_in_range(impact, [coord.x, coord.y, coord.z], spread) {
                            candidates.push((entity.native_unique_id(), occupant.entity_id));
                        }
                    }
                }
            });
            // Native set comparator10035BA8/100765BA uses signed Abstract+10,
            // including equality: two references to the same UID deliver once.
            candidates.sort_by_key(|(uid, _)| *uid);
            candidates.dedup_by_key(|(uid, _)| *uid);
            candidates.into_iter().map(|(_, id)| id).collect()
        }
        /// One actual10062320 delivery. Supported victims are human
        /// Drive/Walk and ordinary AI Drive, with no Team/managers/harvester/positive death threshold.
        /// Their distinct detach/AI/aircraft/building paths remain explicit residuals.
        fn deliver_ares_emp(
            &mut self,
            target: u64,
            source: Option<u64>,
            warhead: &WarheadType,
            rules: &RuleSet,
            effects: FrameEffects<'_>,
        ) -> EmpDelivery {
            let Some(settings) = warhead
                .ares_rules()
                .filter(|settings| settings.duration() != 0)
            else {
                return EmpDelivery::Rejected;
            };
            let Some(victim) = self.substrate.entities.get(target) else {
                return EmpDelivery::Rejected;
            };
            let Some(object) = self.interner.object_type(victim.type_ref(), rules) else {
                return EmpDelivery::Rejected;
            };
            let Some(kind) = object.emp_rules() else {
                return EmpDelivery::Rejected;
            };
            let source_owner = source
                .and_then(|source| self.substrate.entities.get(source))
                .map(|source| source.owner());
            let allied = source_owner.is_some_and(|owner| {
                crate::sim::combat::combat_weapon::is_ally_by_object(
                    Some(self.house_alliances.as_map()),
                    &self.interner,
                    owner,
                    victim.owner(),
                )
            });
            if !settings.can_affect_house(
                source_owner.is_some(),
                source_owner == Some(victim.owner()),
                allied,
                warhead.affects_allies,
            ) || victim.is_warped_out()
                || crate::sim::superweapon::invulnerability::is_invulnerable(
                    victim.invulnerability.as_ref(),
                    self.session.binary_frame(),
                )
                || victim.lifecycle.in_limbo()
                || !victim.lifecycle.object_alive()
            {
                return EmpDelivery::Rejected;
            }
            let default_immune = match victim.category() {
                EntityCategory::Infantry => !object.cyborg,
                EntityCategory::Unit => object.organic,
                EntityCategory::Structure if ordinary_building(object) => {
                    // Installed10062DC0..10062E32: invisible is ineligible;
                    // ordinary powered Buildings need a positive native drain.
                    object.invisible_in_game
                        || !(object.powered
                            && crate::sim::power_system::native_building_power_drain(object.power)
                                > 0)
                }
                _ => return EmpDelivery::Unsupported,
            };
            if kind.immune().unwrap_or(default_immune)
                || object
                    .promotion_abilities()
                    .is_some_and(|abilities| abilities.emp_immune(victim.veterancy.rank()))
            {
                return EmpDelivery::Rejected;
            }
            if source_owner == Some(victim.owner()) && object.type_immune {
                // Reuse every represented rank-specific weapon slot. Native
                // scans20; semantics/configuration of18/19 are a separate
                // unresolved getter/reader boundary, not guessed new slots.
                let elite = victim.veterancy.rank() >= 200;
                if (0..crate::rules::object_type::WEAPON_SLOT_COUNT).any(|index| {
                    (if elite {
                        object.elite_weapon_at(index)
                    } else {
                        object.weapon_at(index)
                    })
                    .and_then(|name| rules.weapon(name))
                    .and_then(|weapon| weapon.warhead.as_deref())
                    .and_then(|name| rules.warhead(name))
                    .and_then(|wh| wh.ares_rules())
                    .is_some_and(|emp| emp.duration() != 0)
                }) {
                    return EmpDelivery::Rejected;
                }
            }
            let verses = f64::from_bits(warhead.interaction(object.armor_id()).multiplier().bits());
            if verses.abs() < 0.001 {
                return EmpDelivery::Rejected;
            }
            // Restrict the new producer by actual dependency support, not a
            // fabricated immunity. An unsupported victim is observable to tests.
            if (victim.category() != EntityCategory::Structure
                && (!matches!(
                    victim.locomotor.as_ref().map(|loco| loco.active_kind()),
                    Some(LocomotorKind::Drive | LocomotorKind::Walk)
                ) || (!self.owner_is_human(victim.owner())
                    && victim.locomotor.as_ref().map(|loco| loco.active_kind())
                        != Some(LocomotorKind::Drive))))
                || (victim.category() == EntityCategory::Structure
                    && (victim.prism_support_count.count() != 0
                        || victim
                            .pending_building_fire
                            .pending()
                            .is_some_and(|pending| {
                                matches!(
                                    pending.fire(),
                                    crate::sim::world::DelayedFire::SupportBeam { .. }
                                )
                            })))
                || victim.is_harvester()
                || victim.is_falling_down()
                || victim.spawn_manager.is_some()
                || victim.slave_manager.is_some()
                || victim.capture_manager.is_some()
                || victim.mind_control.controller().is_some()
                || victim.temporal.warp_target().is_some()
                || self.team_script_vm.team_for_member(target).is_some()
                || kind.threshold() > 0
            {
                return EmpDelivery::Unsupported;
            }
            if self.unit_in_contact_war_factory(target, rules) {
                return EmpDelivery::Rejected;
            }
            let delta = duration_delta(settings.duration(), kind.modifier());
            let old = victim.emp.remaining;
            let remaining = capped_duration(old, delta, settings.cap());
            //10062625 first activation may select Type/General fallback;
            //1006256D refresh only supplies the explicit WH pointer. Preserve
            // that provenance when a prior held Anim has expired.
            let first_activation = old <= 0 && remaining > 0;
            let sparkle_name = settings
                .sparkles()
                .or_else(|| {
                    if first_activation {
                        kind.sparkles().unwrap_or_else(|| rules.emp_sparkles())
                    } else {
                        None
                    }
                })
                .map(str::to_owned);
            self.substrate
                .entities
                .get_mut(target)
                .expect("live EMP target")
                .emp
                .remaining = remaining;
            if old > 0 && remaining == 0 {
                self.disable_ares_emp(target, rules, effects);
                // Ares3.0 10062526..10062563: healing EMP raises65 then64.
                // Natural expiry calls Disable directly and raises neither.
                // Both calls resolve the current Tag; actions may remove it.
                let context =
                    crate::sim::world::UninitContext::new(rules, effects.overlay_registry())
                        .with_effects(effects);
                self.dispatch_object_tag_event(target, 65, Some(target), source, context);
                if self
                    .substrate
                    .entities
                    .get(target)
                    .is_some_and(|entity| entity.lifecycle.object_alive())
                {
                    self.dispatch_object_tag_event(target, 64, Some(target), None, context);
                }
            } else if old <= 0 && remaining > 0 {
                let selected = self
                    .substrate
                    .entities
                    .get(target)
                    .is_some_and(|entity| entity.selection.is_selected());
                self.enable_ares_emp(target, rules, effects);
                if selected {
                    if let Some(entity) = self.substrate.entities.get_mut(target) {
                        entity.selection.select();
                    }
                }
                // 100624C5..10062509 checks Alive before63 and again before
                //62. The generic raise has no source; retain the current Tag
                //rather than a pre-callback attachment. Neither check skips
                //the common threshold/sparkle tail in the native body.
                let context =
                    crate::sim::world::UninitContext::new(rules, effects.overlay_registry())
                        .with_effects(effects);
                if self
                    .substrate
                    .entities
                    .get(target)
                    .is_some_and(|entity| entity.lifecycle.object_alive())
                {
                    self.dispatch_object_tag_event(target, 63, Some(target), source, context);
                    if self
                        .substrate
                        .entities
                        .get(target)
                        .is_some_and(|entity| entity.lifecycle.object_alive())
                    {
                        self.dispatch_object_tag_event(target, 62, Some(target), None, context);
                    }
                }
            } else if old == remaining {
                return EmpDelivery::Applied;
            }
            if first_activation || settings.sparkles().is_some() {
                self.ares_emp_sparkle(target, sparkle_name.as_deref(), rules);
            }
            EmpDelivery::Applied
        }
        fn enable_ares_emp(&mut self, target: u64, rules: &RuleSet, effects: FrameEffects<'_>) {
            let building = self
                .substrate
                .entities
                .get(target)
                .is_some_and(|entity| entity.category() == EntityCategory::Structure);
            if building {
                //10062B58..98: House dirty, then DisableStuff452480. Online660
                // is independent and must stay unchanged during EMP.
                let owner = self.substrate.entities.get(target).unwrap().owner();
                if let Some(house) = self.houses.get_mut(&owner) {
                    house.request_tech_recheck();
                }
                Self::invalidate_house_power(&self.houses, &mut self.power_states, owner, false);
                self.set_building_stuff_enabled(target, false, rules);
            }
            if let Some(entity) = self.substrate.entities.get_mut(target) {
                entity.emp.last_mission = entity.mission.current().raw();
            }
            if self
                .substrate
                .entities
                .get(target)
                .is_some_and(|entity| !entity.deactivated.is_offline() && advisable(entity))
            {
                //10062C26..57 saves only a Building's Focus across the shared
                //Deactivate -> ResetOrdersToGuard, which normally clears it.
                let focus = building.then(|| {
                    self.substrate
                        .entities
                        .get(target)
                        .unwrap()
                        .archive_target()
                });
                self.techno_deactivate(target, rules, effects);
                if let Some(focus) = focus {
                    self.substrate
                        .entities
                        .get_mut(target)
                        .unwrap()
                        .set_archive_target(focus);
                }
            }
        }
        fn disable_ares_emp(&mut self, target: u64, rules: &RuleSet, effects: FrameEffects<'_>) {
            let building = self
                .substrate
                .entities
                .get(target)
                .is_some_and(|entity| entity.category() == EntityCategory::Structure);
            let operational =
                !building || self.building_operational_state(target, rules) == Some(true);
            if building {
                //1006280F..67 samples4555D0 before dirties; low power must not
                //enable Stuff or Reactivate, but Anim retirement still follows.
                let owner = self.substrate.entities.get(target).unwrap().owner();
                if let Some(house) = self.houses.get_mut(&owner) {
                    house.request_tech_recheck();
                }
                Self::invalidate_house_power(&self.houses, &mut self.power_states, owner, false);
                if operational {
                    self.set_building_stuff_enabled(target, true, rules);
                }
            }
            if operational
                && self
                    .substrate
                    .entities
                    .get(target)
                    .is_some_and(|entity| entity.deactivated.is_offline())
            {
                let focus = building.then(|| {
                    self.substrate
                        .entities
                        .get(target)
                        .unwrap()
                        .archive_target()
                });
                self.techno_reactivate(target, rules, effects);
                if let Some(focus) = focus {
                    self.substrate
                        .entities
                        .get_mut(target)
                        .unwrap()
                        .set_archive_target(focus);
                }
            }
            self.retire_ares_emp_sparkle(target);
            self.recover_ares_emp_ai_mission(target, rules);
        }
        /// Installed common TechnoUpdate1004B528..575, before original tint:
        /// an offline Building recovers on its next reached head after power
        /// returns. This is not EnableStuff and does not restore saved Focus.
        pub(crate) fn update_ares_emp_operability(
            &mut self,
            target: u64,
            rules: &RuleSet,
            effects: FrameEffects<'_>,
        ) {
            let Some(entity) = self.substrate.entities.get(target) else {
                return;
            };
            if entity.category() != EntityCategory::Structure
                || !entity.deactivated.is_offline()
                || entity.emp_remaining() > 0
                || !self
                    .interner
                    .object_type(entity.type_ref(), rules)
                    .is_some_and(ordinary_building)
                || self.building_operational_state(target, rules) != Some(true)
            {
                return;
            }
            let owner = entity.owner();
            self.techno_reactivate(target, rules, effects);
            if let Some(house) = self.houses.get_mut(&owner) {
                house.request_tech_recheck();
            }
        }
        /// Installed disable100627F0's final1006293A block, after the
        /// Reactivate/manager/Anim owners. Human control is50B730, not merely
        /// LocalPlayer. Keep QueueMission5B35E0's same-current and timer guards.
        fn recover_ares_emp_ai_mission(&mut self, target: u64, rules: &RuleSet) {
            let Some(entity) = self.substrate.entities.get(target) else {
                return;
            };
            //100628B2 tests AbstractFlags bit4 before Foot recovery. Buildings
            //never queue Hunt, even in a computer House.
            if !entity.category().is_foot() {
                return;
            }
            // DisableEMP100628C2 precedes the human/AI recovery choice.
            if entity.driver_killed() {
                let _ = self.mission_queue_exact(
                    target,
                    crate::sim::mission::MissionId::from_known(MissionType::Harmless),
                    1,
                    self.session.binary_frame(),
                    &crate::sim::mission::authority::LiveReadyInputProvider { rules },
                );
                return;
            }
            if self.owner_is_human(entity.owner()) {
                return;
            }
            let mission = rules
                .ares_emp_ai_recover_mission()
                .unwrap_or(MissionType::Hunt);
            let _ = self.mission_queue_exact(
                target,
                crate::sim::mission::MissionId::from_known(mission),
                0,
                self.session.binary_frame(),
                &crate::sim::mission::authority::LiveReadyInputProvider { rules },
            );
        }
        /// Installed10053DF0 is reached once per actual Techno AI visit.
        pub(crate) fn tick_ares_emp(
            &mut self,
            target: u64,
            rules: &RuleSet,
            effects: FrameEffects<'_>,
        ) {
            let Some(entity) = self.substrate.entities.get_mut(target) else {
                return;
            };
            if entity.emp.remaining == 0 {
                return;
            }
            entity.emp.remaining = entity.emp.remaining.wrapping_sub(1);
            let remaining = entity.emp.remaining;
            let delayed = !entity.deactivated.is_offline() && advisable(entity);
            if remaining == 0 {
                self.disable_ares_emp(target, rules, effects);
            } else if delayed {
                // Installed10053E1A..3E: deferred deactivation saves rawAC then
                // calls only Deactivate. First-hit House/Stuff writes and the
                // Building Focus save/restore belong to10062B40, not this tail.
                entity.emp.last_mission = entity.mission.current().raw();
                self.techno_deactivate(target, rules, effects);
            }
        }
        fn ares_emp_sparkle(&mut self, target: u64, name: Option<&str>, rules: &RuleSet) {
            let Some(entity) = self.substrate.entities.get(target) else {
                return;
            };
            // UpdateSparkle10062FE0 reads the live counter after tag callbacks.
            // A nested effect can replace the state used by the outer delivery.
            if entity.emp.remaining <= 0 {
                self.retire_ares_emp_sparkle(target);
                return;
            }
            if entity.emp.sparkle.is_some() {
                return;
            }
            let Some(name) = name else {
                return;
            };
            let coord = crate::sim::movement::ground_pose::position_world_coord(&entity.position);
            let world = AnimWorldCoord {
                x: coord.x,
                y: coord.y,
                z: coord.z,
            };
            let (rx, ry, sub_x, sub_y, z) = world.to_cell_sub_z();
            let name = self.interner.intern(name);
            // Native10063012..10063029 constructor pushes loop1,flags600.
            let descriptor = AnimClassSpawnDescriptor {
                draw_flags: 0x600,
                ..AnimClassSpawnDescriptor::new(name, rx, ry, sub_x, sub_y, z)
            };
            if let Ok(anim) = self.spawn_anim_at_world(rules, descriptor, world) {
                self.set_anim_owner_object(anim, Some(target), rules);
                if let Some(entity) = self.substrate.entities.get_mut(target) {
                    entity.emp.sparkle = Some(anim);
                }
            }
        }
        fn retire_ares_emp_sparkle(&mut self, target: u64) {
            let anim = self
                .substrate
                .entities
                .get_mut(target)
                .and_then(|entity| entity.emp.sparkle.take());
            if let Some(anim) = anim.and_then(|id| self.substrate.anims.get_mut(id)) {
                anim.end_loops();
            }
        }
    }
    #[cfg(test)]
    mod tests {
        use super::*;
        use crate::rules::ini_parser::IniFile;
        use crate::sim::movement::locomotor::MovementLayer;
        use crate::sim::occupancy::CellListInsertion;
        use crate::sim::projectile::ProjectileCoord;
        fn corpus() -> serde_json::Value {
            serde_json::from_str(crate::test_fixture::text(
                "tools/rules_oracle/ares_emp.json",
            ))
            .unwrap()
        }
        fn int(value: &serde_json::Value) -> i32 {
            value.as_i64().unwrap() as i32
        }
        fn rules(extra: &str) -> RuleSet {
            let ini = IniFile::from_str(
                "[VehicleTypes]\n0=TANK\n[Warheads]\n0=EMPWH\n[Animations]\n0=SPARK\n[TANK]\nStrength=400\nArmor=heavy\nSpeed=6\nLocomotor={4A582741-9839-11d1-B709-00A024DDAFD1}\nMovementZone=Normal\n[EMPWH]\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\nEMP.Duration=2\nEMP.Cap=2\nEMP.Sparkles=SPARK\n",
            );
            let art = IniFile::from_str("[SPARK]\nImage=SPARK\nLoopCount=-1\nRate=1\nEnd=4\n");
            let mut layers = crate::rules::native_processing::RulesLayerStack::new(ini);
            if !extra.is_empty() {
                // Native INI duplicate keys within one physical file are
                // first-wins. Apply corpus priors through a real later Rules
                // layer, not duplicate keys after the two-tick scene defaults.
                layers.push(
                    crate::rules::native_processing::RulesLayerKind::Scenario,
                    IniFile::from_str(&format!("[EMPWH]\n{extra}")),
                );
            }
            let processed = layers.process_with_fixed_art(&art).unwrap();
            let mut rules = RuleSet::from_processed_rules(&processed).unwrap();
            let mut registry = crate::rules::art_data::ArtRegistry::from_ini(&art);
            // Synthetic frame-count asset boundary, not an art extraction.
            registry.bind_anim_frame_count_for_test("SPARK", 4);
            rules.install_art_data(registry);
            rules
        }
        fn install_emp_trigger_chain(sim: &mut Simulation, source_house: i32, repeat: i32) {
            let map = crate::map::map_file::MapFile::from_bytes(
                format!(
                    "[Map]\nTheater=TEMPERATE\nSize=0,0,2,1\nLocalSize=0,0,2,1\n\
                     [IsoMapPack5]\n1=DwALABwBAAIA/////wAAABEAAA==\n\
                     [Basic]\nNewINIFormat=4\n\
                     [Triggers]\nE62=Neutral,E63,E62,0,1,1,1,0\n\
                     E63=Neutral,E64,E63,0,1,1,1,0\nE64=Neutral,E65,E64,0,1,1,1,0\n\
                     E65=Neutral,<none>,E65,0,1,1,1,0\n\
                     [Events]\nE62=1,62,0,0\nE63=1,63,0,{source_house}\n\
                     E64=1,64,0,0\nE65=1,65,0,{source_house}\n\
                     [Actions]\nE62=1,56,0,0,0,0,0,0,A\nE63=1,56,0,1,0,0,0,0,A\n\
                     E64=1,56,0,2,0,0,0,0,A\nE65=1,56,0,3,0,0,0,0,A\n\
                     [Tags]\nEMP={repeat},EMP,E62\n"
                )
                .as_bytes(),
            )
            .unwrap();
            sim.initialize_trigger_definitions(&map, &Default::default(), None, None);
        }
        fn tagged_emp_scene(
            source_house_delta: i32,
            repeat: i32,
        ) -> (Simulation, RuleSet, u64, u64) {
            let rules = rules("");
            let mut sim = crate::sim::arena_fixture::rival_arena(&rules, 17);
            sim.intern_rule_type_ids(&rules);
            sim.resolve_type_handles(&rules);
            let target = sim
                .spawn_object_at_height("TANK", "Americans", 10, 10, 0, 0, &rules)
                .unwrap();
            let source = sim
                .spawn_object_at_height("TANK", "Russians", 12, 10, 0, 0, &rules)
                .unwrap();
            let source_house = sim
                .base_reservation_house_index(sim.entities().get(source).unwrap().owner())
                .unwrap();
            install_emp_trigger_chain(&mut sim, source_house + source_house_delta, repeat);
            sim.attach_object_trigger_tag(target, Some("EMP"));
            (sim, rules, target, source)
        }
        fn emp_event_kinds(sim: &Simulation) -> Vec<i32> {
            sim.tag_dispatch_trace_for_test()
                .iter()
                .map(|(_, kind)| *kind)
                .collect()
        }

        /// Ares3.0 deliver100624C5..10062509 and10062526..10062563 raise
        /// house-specific events before generic events. Natural expiry does not.
        #[test]
        fn ares_emp_tagged_delivery_orders_events_and_natural_expiry_does_not_emit_them() {
            let (mut sim, rules, target, source) = tagged_emp_scene(0, 2);
            let wh = rules.warhead("EMPWH").unwrap();
            sim.deliver_ares_emp(target, Some(source), wh, &rules, FrameEffects::empty());
            assert_eq!(emp_event_kinds(&sim), [63, 62]);
            assert!(sim.trigger_local_is_set_for_test(0));
            assert!(sim.trigger_local_is_set_for_test(1));
            assert!(!sim.trigger_local_is_set_for_test(2));
            assert!(!sim.trigger_local_is_set_for_test(3));
            sim.clear_tag_dispatch_trace_for_test();
            sim.deliver_ares_emp(target, Some(source), wh, &rules, FrameEffects::empty());
            assert!(
                emp_event_kinds(&sim).is_empty(),
                "capped repeat is not a transition"
            );
            sim.tick_ares_emp(target, &rules, FrameEffects::empty());
            sim.tick_ares_emp(target, &rules, FrameEffects::empty());
            assert!(
                emp_event_kinds(&sim).is_empty(),
                "timer expiry has no removal events"
            );
            sim.deliver_ares_emp(target, Some(source), wh, &rules, FrameEffects::empty());
            sim.clear_tag_dispatch_trace_for_test();
            let healing = self::rules("EMP.Duration=-3\nEMP.Cap=-1\n");
            sim.deliver_ares_emp(
                target,
                Some(source),
                healing.warhead("EMPWH").unwrap(),
                &healing,
                FrameEffects::empty(),
            );
            assert_eq!(emp_event_kinds(&sim), [65, 64]);
            assert!(sim.trigger_local_is_set_for_test(2));
            assert!(sim.trigger_local_is_set_for_test(3));
        }

        #[test]
        fn ares_emp_tag_event_house_filter_uses_raw_source_index_and_requires_source() {
            for (delta, source_present) in [(10, true), (0, false)] {
                let (mut sim, rules, target, source) = tagged_emp_scene(delta, 2);
                sim.deliver_ares_emp(
                    target,
                    source_present.then_some(source),
                    rules.warhead("EMPWH").unwrap(),
                    &rules,
                    FrameEffects::empty(),
                );
                assert_eq!(emp_event_kinds(&sim), [63, 62]);
                assert!(sim.trigger_local_is_set_for_test(0));
                assert!(!sim.trigger_local_is_set_for_test(1));
                let healing = self::rules("EMP.Duration=-3\nEMP.Cap=-1\n");
                sim.deliver_ares_emp(
                    target,
                    source_present.then_some(source),
                    healing.warhead("EMPWH").unwrap(),
                    &healing,
                    FrameEffects::empty(),
                );
                assert!(sim.trigger_local_is_set_for_test(2));
                assert!(!sim.trigger_local_is_set_for_test(3));
            }
        }

        #[test]
        fn ares_emp_tag_predicates_require_matching_kind_actor_and_live_counter() {
            let (mut sim, rules, target, source) = tagged_emp_scene(0, 2);
            let context = crate::sim::world::UninitContext::with_rules(&rules);
            for kind in [62, 63] {
                sim.dispatch_object_tag_event(target, kind, Some(target), Some(source), context);
            }
            assert!(!sim.trigger_local_is_set_for_test(0));
            assert!(!sim.trigger_local_is_set_for_test(1));
            // A source alone cannot replace the affected actor argument.
            for kind in [64, 65] {
                sim.dispatch_object_tag_event(target, kind, None, Some(source), context);
            }
            assert!(!sim.trigger_local_is_set_for_test(2));
            assert!(!sim.trigger_local_is_set_for_test(3));
            sim.deliver_ares_emp(
                target,
                Some(source),
                rules.warhead("EMPWH").unwrap(),
                &rules,
                FrameEffects::empty(),
            );
            for kind in [64, 65] {
                sim.dispatch_object_tag_event(target, kind, Some(target), Some(source), context);
            }
            assert!(!sim.trigger_local_is_set_for_test(2));
            assert!(!sim.trigger_local_is_set_for_test(3));
        }

        #[test]
        fn ares_emp_first_tag_event_detachment_suppresses_the_generic_raise() {
            for healing in [false, true] {
                let (mut sim, rules, target, source) = tagged_emp_scene(0, 0);
                if healing {
                    sim.attach_object_trigger_tag(target, None);
                    sim.deliver_ares_emp(
                        target,
                        Some(source),
                        rules.warhead("EMPWH").unwrap(),
                        &rules,
                        FrameEffects::empty(),
                    );
                    sim.attach_object_trigger_tag(target, Some("EMP"));
                }
                let rules = if healing {
                    self::rules("EMP.Duration=-3\nEMP.Cap=-1\n")
                } else {
                    rules
                };
                sim.deliver_ares_emp(
                    target,
                    Some(source),
                    rules.warhead("EMPWH").unwrap(),
                    &rules,
                    FrameEffects::empty(),
                );
                let (specific, specific_flag, generic_flag) =
                    if healing { (65, 3, 2) } else { (63, 1, 0) };
                assert_eq!(emp_event_kinds(&sim), [specific]);
                assert!(sim.trigger_local_is_set_for_test(specific_flag));
                assert!(!sim.trigger_local_is_set_for_test(generic_flag));
                assert!(!sim.object_has_trigger_tag(target));
            }
        }

        #[test]
        fn ares_emp_first_tag_event_can_kill_the_victim_before_generic_raise() {
            for healing in [false, true] {
                let (mut sim, _, target, source) = tagged_emp_scene(0, 2);
                sim.attach_object_trigger_tag(target, None);
                if healing {
                    let initial = rules("");
                    sim.deliver_ares_emp(
                        target,
                        Some(source),
                        initial.warhead("EMPWH").unwrap(),
                        &initial,
                        FrameEffects::empty(),
                    );
                }
                let rules = rules(&format!(
                    "EMP.Duration={}\nEMP.Cap=-1\n[Warheads]\n1=C4\n[C4]\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n[CombatDamage]\nC4Warhead=C4\n",
                    if healing { -3 } else { 2 }
                ));
                assert_eq!(rules.bridge_warheads.c4_name, "C4");
                let source_house = sim
                    .base_reservation_house_index(sim.entities().get(source).unwrap().owner())
                    .unwrap();
                let (specific, generic) = if healing { (65, 64) } else { (63, 62) };
                let map = crate::map::map_file::MapFile::from_bytes(
                    format!(
                        "[Map]\nTheater=TEMPERATE\nSize=0,0,2,1\nLocalSize=0,0,2,1\n\
                     [IsoMapPack5]\n1=DwALABwBAAIA/////wAAABEAAA==\n\
                     [Basic]\nNewINIFormat=4\n\
                     [Triggers]\nKILL=Neutral,GENERIC,KILL,0,1,1,1,0\n\
                     GENERIC=Neutral,<none>,GENERIC,0,1,1,1,0\n\
                     [Events]\nKILL=1,{specific},0,{source_house}\nGENERIC=1,{generic},0,0\n\
                     [Actions]\nKILL=1,32,0,0,0,0,0,0,A\nGENERIC=1,56,0,0,0,0,0,0,A\n\
                     [Tags]\nEMP=2,EMP,KILL\n"
                    )
                    .as_bytes(),
                )
                .unwrap();
                sim.initialize_trigger_definitions(&map, &Default::default(), None, None);
                sim.attach_object_trigger_tag(target, Some("EMP"));
                sim.deliver_ares_emp(
                    target,
                    Some(source),
                    rules.warhead("EMPWH").unwrap(),
                    &rules,
                    FrameEffects::empty(),
                );
                assert!(
                    sim.entities()
                        .get(target)
                        .is_none_or(|entity| !entity.lifecycle.object_alive()),
                    "the configured C4 callback must kill its actor, healing={healing}"
                );
                assert!(!sim.trigger_local_is_set_for_test(0));
                assert!(!emp_event_kinds(&sim).contains(&generic));
            }
        }
        fn building_scene(extra: &str) -> (Simulation, RuleSet, u64, u64, u64) {
            let ini = crate::sim::arena_fixture::clear_ground_ini(
                "[VehicleTypes]\n0=EMITTER\n[BuildingTypes]\n0=TOWER\n[Warheads]\n0=EMPWH\n1=HURT\n[Animations]\n0=SPARK\n1=ELECTRO\n2=TOWER_B\n[General]\nEMPulseSparkles=SPARK\nPrismType=TOWER\n[EMITTER]\nStrength=400\nArmor=heavy\nSpeed=6\nPrimary=PULSE\nLocomotor={4A582741-9839-11d1-B709-00A024DDAFD1}\nMovementZone=Normal\n[TOWER]\nImage=TOWERART\nStrength=1000\nArmor=steel\nPowered=yes\nPower=-75\nPrimary=BEAM\nCanPassiveAquire=no\nHasStupidGuardMode=no\n[PULSE]\nDamage=1\nROF=1\nRange=9.5\nSpeed=100\nProjectile=INVIS\nWarhead=EMPWH\n[BEAM]\nDamage=20\nROF=1\nRange=10\nSpeed=100\nProjectile=INVIS\nWarhead=HURT\n[INVIS]\nAirburst=yes\nInviso=yes\nAG=yes\nAA=no\n[EMPWH]\nCellSpread=.7\nVerses=0%,0%,0%,0%,0%,0%,3%,3%,3%,0%,0%\nEMP.Duration=5\nEMP.Cap=10\nAffectsAllies=yes\n[HURT]\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n",
            );
            let art = IniFile::from_str(
                "[TOWERART]\nFoundation=1x1\nDelayedFireDelay=1\nActiveAnim=TOWER_B\nActiveAnimPowered=yes\n[TOWER_B]\nLoopCount=-1\nRate=1\nEnd=100\n[SPARK]\nLoopCount=-1\nRate=1\nEnd=4\n",
            );
            let mut layers = crate::rules::native_processing::RulesLayerStack::new(ini);
            if !extra.is_empty() {
                layers.push(
                    crate::rules::native_processing::RulesLayerKind::Scenario,
                    IniFile::from_str(&format!("[TOWER]\n{extra}")),
                );
            }
            let processed = layers.process_with_fixed_art(&art).unwrap();
            let mut rules = RuleSet::from_processed_rules(&processed).unwrap();
            let mut registry = crate::rules::art_data::ArtRegistry::from_ini(&art);
            for name in ["SPARK", "TOWER_B"] {
                registry
                    .bind_anim_frame_count_for_test(name, if name == "TOWER_B" { 100 } else { 4 });
            }
            rules.install_art_data(registry);
            let mut sim = crate::sim::arena_fixture::rival_arena(&rules, 17);
            sim.intern_rule_type_ids(&rules);
            let target = sim
                .spawn_object("TOWER", "Russians", 10, 10, 0, &rules)
                .unwrap();
            let source = sim
                .spawn_object("EMITTER", "Americans", 12, 10, 0, &rules)
                .unwrap();
            sim.resolve_type_handles(&rules);
            let owner = sim.entities().get(target).unwrap().owner();
            let power = sim
                .power_states
                .map_mut_for_test()
                .entry(owner)
                .or_default();
            power.set_total_output_for_test(100);
            power.set_total_drain_for_test(75);
            let slot = sim
                .set_building_anim_slot(target, 3, false, false, 0, &rules)
                .unwrap();
            sim.visit_building_operational(target, &rules, None, FrameEffects::empty());
            sim.substrate
                .entities
                .get_mut(target)
                .unwrap()
                .mission
                .set_current_for_test(
                    crate::sim::mission::MissionId::from_known(MissionType::Guard),
                    0,
                );
            (sim, rules, target, source, slot)
        }

        fn tower_fire_error(
            sim: &Simulation,
            rules: &RuleSet,
            tower: u64,
            target: u64,
        ) -> crate::sim::combat::fire_error::FireError {
            let firer = sim.entities().get(tower).unwrap();
            crate::sim::combat::fire_error_world::FireSubject {
                world: sim,
                rules,
                overlay_registry: None,
                fog: None,
                firer,
                obj: rules.object("TOWER").unwrap(),
                target: Some(crate::sim::combat::TargetKind::Entity(target)),
                weapon_index: 0,
            }
            .fire_error(false)
        }

        #[test]
        fn ares_emp_building_parent_bullet_actual_ai_fire_and_power_recovery() {
            use crate::sim::{
                combat::{TargetKind, fire_error::FireError, world_fire::FireVisit},
                world::ObjectAiCtx,
            };
            let (mut sim, rules, target, source, slot) = building_scene("ImmuneToEMP=no\n");
            let owner = sim.entities().get(target).unwrap().owner();
            assert_eq!(
                tower_fire_error(&sim, &rules, target, source),
                FireError::Ok
            );
            let focus = Some(TargetKind::Entity(source));
            sim.substrate
                .entities
                .get_mut(target)
                .unwrap()
                .set_archive_target(focus);
            // The production FireAt/Bullet/receiver chain, not only direct EMP delivery.
            sim.commit_fire_visit(
                FireVisit::Direct {
                    id: source,
                    target: TargetKind::Entity(target),
                    weapon_index: 0,
                },
                &rules,
                None,
                FrameEffects::empty(),
            );
            let bullets: Vec<_> = sim.projectiles.iter().map(|(&id, _)| id).collect();
            assert_eq!(bullets.len(), 1);
            let emp_bullet = bullets[0];
            for bullet in bullets {
                sim.object_ai_visit_one(bullet, Some(&rules), ObjectAiCtx::default());
            }
            assert_eq!(sim.entities().get(target).unwrap().emp_remaining(), 5);
            assert_eq!(sim.building_operational_state(target, &rules), Some(false));
            assert_eq!(
                tower_fire_error(&sim, &rules, target, source),
                FireError::Cant
            );
            assert!(sim.entities().get(target).unwrap().building_online());
            assert!(
                !sim.entities()
                    .get(target)
                    .unwrap()
                    .building_art
                    .stuff_enabled()
            );
            assert!(sim.anim(slot).unwrap().runtime().paused);
            let sparkle = sim.entities().get(target).unwrap().emp_sparkle().unwrap();
            // Low-power expiry still retires the sparkle, but cannot activate the tower.
            sim.power_states
                .map_mut_for_test()
                .get_mut(&owner)
                .unwrap()
                .set_total_output_for_test(0);
            for remaining in (0..5).rev() {
                assert!(sim.object_ai_visit_one(target, Some(&rules), ObjectAiCtx::default()));
                assert_eq!(
                    sim.entities().get(target).unwrap().emp_remaining(),
                    remaining
                );
            }
            assert!(sim.entities().get(target).unwrap().deactivated.is_offline());
            assert_eq!(sim.entities().get(target).unwrap().archive_target(), focus);
            assert_eq!(
                sim.entities().get(target).unwrap().mission.queued().raw(),
                -1,
                "AI Building is not Foot Hunt recovery"
            );
            assert!(sim.entities().get(target).unwrap().emp_sparkle().is_none());
            assert_eq!(sim.anim(sparkle).unwrap().runtime().loop_remaining, 0);
            sim.object_ai_visit_one(target, Some(&rules), ObjectAiCtx::default());
            assert!(sim.entities().get(target).unwrap().deactivated.is_offline());
            sim.power_states
                .map_mut_for_test()
                .get_mut(&owner)
                .unwrap()
                .set_total_output_for_test(100);
            // First operational edge resumes the existing powered slot. The reached
            //Techno head then activates; it must not invent a delayed EnableStuff.
            sim.object_ai_visit_one(target, Some(&rules), ObjectAiCtx::default());
            assert!(!sim.entities().get(target).unwrap().deactivated.is_offline());
            assert!(
                !sim.entities()
                    .get(target)
                    .unwrap()
                    .building_art
                    .stuff_enabled()
            );
            assert!(!sim.anim(slot).unwrap().runtime().paused);
            assert_eq!(
                sim.entities().get(target).unwrap().archive_target(),
                None,
                "delayed shared Guard clears Focus"
            );
            assert_eq!(
                tower_fire_error(&sim, &rules, target, source),
                FireError::Ok
            );
            // Re-enter the real tower attack handler after recovery and consume
            //its emitted Bullet, so availability is observed as actual damage.
            sim.assign_target_represented(
                target,
                Some(TargetKind::Entity(source)),
                &rules,
                None,
                FrameEffects::empty(),
            )
            .unwrap();
            sim.substrate
                .mission_assign_exact(
                    target,
                    crate::sim::mission::MissionId::from_known(MissionType::Attack),
                    sim.session.binary_frame(),
                )
                .unwrap();
            let health = sim.entities().get(source).unwrap().health.current();
            sim.object_ai_visit_one(target, Some(&rules), ObjectAiCtx::default());
            // UnInit removes the old EMP Bullet from Logic55BAE0 immediately,
            // but its physical storage survives until the725C70 late drain.
            // These direct object visits do not advance that drain. Advancing
            // every stored Bullet here would detonate the retired EMP twice.
            assert!(
                sim.projectiles
                    .get(emp_bullet)
                    .is_some_and(|bullet| !bullet.in_logic_vector())
            );
            let bullets: Vec<_> = sim
                .projectiles
                .iter()
                .filter(|(_, bullet)| bullet.in_logic_vector())
                .map(|(&id, _)| id)
                .collect();
            assert!(
                !bullets.is_empty(),
                "restored real tower Mission_Attack emits a Bullet"
            );
            for bullet in bullets {
                sim.object_ai_visit_one(bullet, Some(&rules), ObjectAiCtx::default());
            }
            assert!(sim.entities().get(source).unwrap().health.current() < health);
            assert_eq!(sim.entities().get(target).unwrap().emp_remaining(), 0);
            sim.notify_anim_pointer_expired(sparkle);
            assert!(sim.entities().get(target).unwrap().emp_sparkle().is_none());
        }

        #[test]
        fn ares_emp_building_native_admission_refresh_and_slot_lifecycle() {
            use crate::sim::{
                combat::TargetKind,
                mission::{MissionId, state::MissionTestFixture, timer::MissionDispatchTimer},
                power_system::PowerState,
            };
            let native = corpus();
            for row in native["building_admission"].as_array().unwrap() {
                let f = &row["fixture"];
                let mut extra = String::new();
                if let Some(immune) = f["immune"].as_bool() {
                    extra.push_str(if immune {
                        "ImmuneToEMP=yes\n"
                    } else {
                        "ImmuneToEMP=no\n"
                    });
                }
                if f["powered"].as_bool() == Some(false) {
                    extra.push_str("Powered=no\nPower=0\n");
                }
                if f["invisible"].as_bool() == Some(true) {
                    extra.push_str("InvisibleInGame=yes\n");
                }
                let (mut sim, rules, target, source, _) = building_scene(&extra);
                let result = sim.deliver_ares_emp(
                    target,
                    Some(source),
                    rules.warhead("EMPWH").unwrap(),
                    &rules,
                    FrameEffects::empty(),
                );
                assert_eq!(
                    result == EmpDelivery::Rejected,
                    row["result"]["immune"].as_bool().unwrap(),
                    "{}",
                    row["name"]
                );
                assert_ne!(result, EmpDelivery::Unsupported, "{}", row["name"]);
            }
            for row in native["parent_impact_snap"].as_array().unwrap() {
                let f = &row["fixture"];
                let offset: Vec<_> = f["target_offset"]
                    .as_array()
                    .map(|values| values.iter().map(int).collect())
                    .unwrap_or_else(|| vec![0; 3]);
                let (mut sim, rules, tower, emitter, _) = building_scene(&format!(
                    "TargetCoordOffset={},{},{}\n",
                    offset[0], offset[1], offset[2]
                ));
                let target = match f["target_kind"].as_str().unwrap() {
                    "building" => Some(tower),
                    "unit" => Some(emitter),
                    "none" => None,
                    value => panic!("unhandled native snap target {value}"),
                };
                if let Some(target) = target {
                    let entity = sim.substrate.entities.get_mut(target).unwrap();
                    entity
                        .position
                        .put_world_xy([int(&f["target_coords"][0]), int(&f["target_coords"][1])]);
                    entity
                        .position
                        .set_exact_z_leptons_for_test(Some(int(&f["target_coords"][2])));
                }
                // Production construction supplies the unique Bullet identity;
                //case location/target/impact are declared native caller inputs.
                sim.commit_fire_visit(
                    crate::sim::combat::world_fire::FireVisit::Direct {
                        id: emitter,
                        target: crate::sim::combat::TargetKind::Entity(tower),
                        weapon_index: 0,
                    },
                    &rules,
                    None,
                    FrameEffects::empty(),
                );
                let bullet_id = *sim.projectiles.iter().next().unwrap().0;
                let target = target.map_or(
                    crate::sim::projectile::ProjectileTarget::None,
                    crate::sim::projectile::ProjectileTarget::Entity,
                );
                let coordinate = |value: &serde_json::Value| {
                    ProjectileCoord::new(int(&value[0]), int(&value[1]), int(&value[2]))
                };
                let bullet = sim.projectiles.get_mut(bullet_id).unwrap();
                bullet.set_position_for_test(coordinate(&f["location"]));
                bullet.set_target_for_test(target);
                let detonation = crate::sim::projectile::ProjectileDetonation {
                    projectile_id: bullet_id,
                    source_id: emitter,
                    target,
                    impact: coordinate(&f["impact"]),
                    payload: bullet.payload(),
                    reason: crate::sim::projectile::ProjectileDetonationReason::ReachedTarget,
                };
                let distance = match target {
                    crate::sim::projectile::ProjectileTarget::Entity(id) => {
                        crate::sim::combat::object_distance_to(
                            bullet_id,
                            &crate::sim::combat::TargetKind::Entity(id),
                            &sim,
                        )
                    }
                    _ => None,
                };
                assert_eq!(
                    distance,
                    row["result"]["distance"].as_i64().map(|value| value as i32),
                    "{}",
                    row["name"]
                );
                assert_eq!(
                    crate::sim::combat::world_receiver::ares_warhead_effect_coordinate(
                        &sim,
                        &detonation
                    ),
                    coordinate(&row["result"]["impact"]),
                    "{}",
                    row["name"]
                );
                assert_eq!(
                    detonation.impact,
                    coordinate(&f["impact"]),
                    "original receiver's impact stays separate"
                );
            }
            for row in native["building_collectors"].as_array().unwrap() {
                let (mut sim, rules, target, _, _) = building_scene("");
                let f = &row["fixture"];
                // The first tower was production-spawned from fixed ART's1x1
                //foundation. Extra overlap/neighbor references are explicit
                //structural inputs, not legal placement of overlapping towers.
                let template = sim.entities().get(target).unwrap().clone();
                for (index, input) in f["objects"].as_array().unwrap().iter().enumerate() {
                    let id = if index == 0 {
                        target
                    } else {
                        sim.substrate.allocate_stable_id()
                    };
                    let mut entity = template.clone();
                    entity.set_stable_id_for_test(id);
                    entity.set_native_unique_id_for_test(int(&input["uid"]));
                    entity
                        .position
                        .put_world_xy([int(&input["coords"][0]), int(&input["coords"][1])]);
                    entity
                        .position
                        .set_exact_z_leptons_for_test(Some(int(&input["coords"][2])));
                    sim.substrate.entities.insert(entity);
                    let rx = int(&input["cell"][0]) as u16;
                    let ry = int(&input["cell"][1]) as u16;
                    if index != 0 {
                        sim.substrate.occupancy.add(
                            rx,
                            ry,
                            id,
                            MovementLayer::Ground,
                            None,
                            CellListInsertion::PrependNonBuilding,
                        );
                    }
                    if f["duplicate_neighbor"].as_bool().unwrap() {
                        sim.substrate.occupancy.add(
                            11,
                            10,
                            id,
                            MovementLayer::Ground,
                            None,
                            CellListInsertion::PrependNonBuilding,
                        );
                    }
                }
                let impact = ProjectileCoord {
                    x: int(&f["impact"][0]),
                    y: int(&f["impact"][1]),
                    z: int(&f["impact"][2]),
                };
                let actual: Vec<_> = sim
                    .ares_effect_ground_targets(impact, f["spread"].as_f64().unwrap(), &rules)
                    .iter()
                    .map(|id| sim.entities().get(*id).unwrap().native_unique_id())
                    .collect();
                let expected: Vec<_> = row["result"]["ordered_uids"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(int)
                    .collect();
                assert_eq!(actual, expected, "{}", row["name"]);
            }
            for row in native["building_chains"]
                .as_array()
                .unwrap()
                .iter()
                .chain(native["building_deferred_deactivation"].as_array().unwrap())
            {
                let (mut sim, rules, target, source, slot) = building_scene("");
                let owner = sim.entities().get(target).unwrap().owner();
                sim.houses
                    .get_mut(&owner)
                    .unwrap()
                    .set_is_human_for_test(row["fixture"]["human"].as_bool().unwrap_or(false));
                let before = &row["result"][0];
                let focus = Some(TargetKind::Entity(source));
                {
                    let victim = sim.substrate.entities.get_mut(target).unwrap();
                    victim.emp.last_mission = int(&before["saved_mission"]);
                    victim.set_archive_target(focus);
                    victim.mission.apply_test_fixture(MissionTestFixture {
                        current: MissionId::from_raw(int(&before["current_mission"])),
                        queued: MissionId::from_raw(int(&before["queued_mission"])),
                        suspended: MissionId::NONE,
                        movement_bypass_latch: int(&before["bypass"]) as u8,
                        handler_state: 0,
                        mission_start_frame: 0,
                        ai_counter: int(&before["mission_timer"][1]) as u32,
                        dispatch_timer: MissionDispatchTimer::from_raw(
                            int(&before["mission_timer"][0]),
                            int(&before["mission_timer"][2]),
                        ),
                    });
                }
                // Match the native producer's supplied settled House prior; later
                //ack_dirty is diagnostic input, not a simulated House scan.
                let mut output = 1000;
                let settle = |sim: &mut Simulation, output| {
                    let mut state = PowerState::settled_for_test();
                    state.set_total_output_for_test(output);
                    state.set_total_drain_for_test(75);
                    sim.power_states.map_mut_for_test().insert(owner, state);
                    sim.houses.get_mut(&owner).unwrap().take_tech_recheck();
                };
                settle(&mut sim, output);
                let timer = sim.anim(slot).unwrap().runtime().frame_timer;
                let rng = sim.rng_state();
                for (index, step) in row["steps"].as_array().unwrap().iter().enumerate() {
                    match step["action"].as_str().unwrap() {
                        "prepared" => {}
                        "head" => {
                            sim.update_ares_emp_operability(target, &rules, FrameEffects::empty())
                        }
                        "hit" => assert_eq!(
                            sim.deliver_ares_emp(
                                target,
                                Some(source),
                                rules.warhead("EMPWH").unwrap(),
                                &rules,
                                FrameEffects::empty()
                            ),
                            EmpDelivery::Applied
                        ),
                        "edge" => sim.visit_building_operational(
                            target,
                            &rules,
                            None,
                            FrameEffects::empty(),
                        ),
                        "tick" => sim.tick_ares_emp(target, &rules, FrameEffects::empty()),
                        "power" => {
                            output = int(&step["output"]);
                            sim.power_states
                                .map_mut_for_test()
                                .get_mut(&owner)
                                .unwrap()
                                .set_total_output_for_test(output);
                        }
                        "mission" => sim
                            .substrate
                            .entities
                            .get_mut(target)
                            .unwrap()
                            .mission
                            .set_current_for_test(MissionId::from_raw(int(&step["value"])), 0),
                        "ack_dirty" => settle(&mut sim, output),
                        "queue_guard_and_ack" => {
                            // Native producer input: queued+B4 changes without
                            // Commence or QueueMission; House acknowledges dirty.
                            let mission =
                                &mut sim.substrate.entities.get_mut(target).unwrap().mission;
                            mission.apply_test_fixture(MissionTestFixture {
                                current: mission.current(),
                                queued: MissionId::from_known(MissionType::Guard),
                                suspended: MissionId::NONE,
                                movement_bypass_latch: mission.movement_bypass_latch(),
                                handler_state: 0,
                                mission_start_frame: 0,
                                ai_counter: mission.ai_counter(),
                                dispatch_timer: mission.dispatch_timer(),
                            });
                            settle(&mut sim, output);
                        }
                        action => panic!("unhandled native Building action {action}"),
                    }
                    let expected = &row["result"][index];
                    let victim = sim.entities().get(target).unwrap();
                    let label = format!("{} step{index}", row["name"]);
                    assert_eq!(victim.emp_remaining(), int(&expected["counter"]), "{label}");
                    assert_eq!(
                        victim.deactivated.is_offline(),
                        expected["deactivated"].as_bool().unwrap(),
                        "{label}"
                    );
                    assert_eq!(
                        victim.emp.last_mission,
                        int(&expected["saved_mission"]),
                        "{label}"
                    );
                    assert_eq!(
                        victim.archive_target(),
                        (int(&expected["focus"]) != 0).then_some(focus.unwrap()),
                        "{label}"
                    );
                    assert_eq!(
                        victim.building_online(),
                        expected["online"].as_bool().unwrap(),
                        "{label}"
                    );
                    assert_eq!(
                        victim.building_art.stuff_enabled(),
                        expected["stuff"].as_bool().unwrap(),
                        "{label}"
                    );
                    assert_eq!(
                        victim.mission.current().raw(),
                        int(&expected["current_mission"]),
                        "{label}"
                    );
                    assert_eq!(
                        victim.mission.queued().raw(),
                        int(&expected["queued_mission"]),
                        "{label}"
                    );
                    if let Some(effective) = expected["effective_mission"].as_i64() {
                        assert_eq!(
                            victim.mission.effective().raw(),
                            effective as i32,
                            "{label}"
                        );
                    }
                    assert_eq!(
                        i32::from(victim.mission.movement_bypass_latch()),
                        int(&expected["bypass"]),
                        "{label}"
                    );
                    assert_eq!(
                        [
                            victim.mission.dispatch_timer().start_frame(),
                            victim.mission.ai_counter() as i32,
                            victim.mission.dispatch_timer().delay()
                        ],
                        [
                            int(&expected["mission_timer"][0]),
                            int(&expected["mission_timer"][1]),
                            int(&expected["mission_timer"][2])
                        ],
                        "{label}"
                    );
                    let power = sim.power_states.get(&owner).unwrap();
                    assert_eq!(
                        [
                            sim.houses.get(&owner).unwrap().tech_recheck_pending(),
                            power.power_recheck_for_test(),
                            power.radar_recheck_for_test()
                        ],
                        [
                            expected["house_dirty"][0].as_i64().unwrap() != 0,
                            expected["house_dirty"][1].as_i64().unwrap() != 0,
                            expected["house_dirty"][2].as_i64().unwrap() != 0
                        ],
                        "{label}"
                    );
                    assert_eq!(
                        sim.building_operational_state(target, &rules),
                        expected["operational"].as_bool(),
                        "{label}"
                    );
                    assert_eq!(
                        sim.anim(slot).unwrap().runtime().paused,
                        expected["slot_paused"].as_bool().unwrap(),
                        "{label}"
                    );
                    assert_eq!(
                        sim.anim(slot).unwrap().runtime().frame_timer,
                        timer,
                        "{label}"
                    );
                    assert_eq!(victim.building_art.anim_slots()[3], Some(slot), "{label}");
                }
                assert_eq!(
                    sim.rng_state(),
                    rng,
                    "bounded state/art services draw no RNG: {}",
                    row["name"]
                );
                assert!(
                    row["queue_calls"].as_array().unwrap().is_empty(),
                    "native Structure skips Foot AI recovery"
                );
                assert_eq!(row["code_unchanged"], true);
                assert_eq!(row["rng_streams_unchanged"], true);
            }
            for row in native["building_slot_timer"].as_array().unwrap() {
                let (mut sim, rules, target, _, slot) = building_scene("");
                let fixture = &row["fixture"];
                let before = &row["result"]["before"];
                {
                    let runtime = sim
                        .substrate
                        .anims
                        .get_mut(slot)
                        .unwrap()
                        .runtime_mut_for_test();
                    runtime.current_frame = int(&before["frame"]);
                    runtime.first_ai_guard = false;
                    runtime.rate_reload = int(&fixture["rate_reload"]) as u16;
                    runtime.frame_step = int(&fixture["frame_step"]);
                    runtime.frame_timer = crate::sim::timer::CdTimer::started(
                        int(&fixture["timer_start"]),
                        int(&fixture["timer_duration"]),
                    );
                }
                sim.session
                    .set_binary_frame_for_test(int(&fixture["global_frame"]) as u32);
                sim.set_building_stuff_enabled(target, false, &rules);
                for (name, enabled) in [("paused", false), ("resumed", true)] {
                    if enabled {
                        sim.set_building_stuff_enabled(target, true, &rules);
                    }
                    sim.object_ai_visit_one(
                        slot,
                        Some(&rules),
                        crate::sim::world::ObjectAiCtx::default(),
                    );
                    let runtime = sim.anim(slot).unwrap().runtime();
                    let expected = &row["result"][name];
                    assert_eq!(runtime.current_frame, int(&expected["frame"]));
                    assert_eq!(runtime.paused, expected["paused"].as_bool().unwrap());
                    assert_eq!(
                        [
                            runtime.frame_timer.start_frame(),
                            runtime.frame_timer.duration()
                        ],
                        [
                            int(&expected["timer_start"]),
                            int(&expected["timer_duration"])
                        ]
                    );
                }
            }
        }

        #[test]
        fn ares_emp_ai_recovery_queue_matches_native_post_reactivate_controls() {
            use crate::sim::mission::{
                MissionId, state::MissionTestFixture, timer::MissionDispatchTimer,
            };
            for row in corpus()["ai_expiry"].as_array().unwrap() {
                let fixture = &row["fixture"];
                let extra = fixture["raw"].as_str().map_or(String::new(), |raw| {
                    format!("[CombatDamage]\nEMPAIRecoverMission={raw}\n")
                });
                let rules = rules(&extra);
                let mut sim = crate::sim::arena_fixture::rival_arena(&rules, 17);
                sim.intern_rule_type_ids(&rules);
                let target = sim
                    .spawn_object_at_height("TANK", "Russians", 10, 10, 0, 0, &rules)
                    .unwrap();
                let owner = sim.substrate.entities.get(target).unwrap().owner();
                let house = sim.houses.get_mut(&owner).unwrap();
                house.set_is_human_for_test(int(&fixture["house_1ec"]) != 0);
                house.set_player_control_for_test(int(&fixture["house_1ed"]) != 0);
                sim.session.set_game_mode(
                    crate::sim::scenario_session::ScenarioGameMode::from_game_mode(int(
                        &fixture["mode"],
                    )),
                );
                let before = &row["before"];
                let expected = &row["after"];
                // The native corpus substitutes Reactivate. Start this comparison
                // at its declared post-Reactivate boundary: only the final Ares
                // mission decision and actual QueueMission5B35E0 are compared.
                let victim = sim.substrate.entities.get_mut(target).unwrap();
                victim.emp.remaining = int(&expected["counter"]);
                victim
                    .deactivated
                    .set_for_test(expected["deactivated"].as_bool().unwrap());
                victim.mission.apply_test_fixture(MissionTestFixture {
                    current: MissionId::from_raw(int(&before["current_mission"])),
                    queued: MissionId::from_raw(int(&before["queued_mission"])),
                    suspended: MissionId::NONE,
                    movement_bypass_latch: int(&before["bypass"]) as u8,
                    handler_state: 0,
                    mission_start_frame: 0,
                    ai_counter: int(&before["mission_timer"][1]) as u32,
                    dispatch_timer: MissionDispatchTimer::from_raw(
                        int(&before["mission_timer"][0]),
                        int(&before["mission_timer"][2]),
                    ),
                });
                let rng = sim.rng_state();
                sim.recover_ares_emp_ai_mission(target, &rules);
                let victim = sim.substrate.entities.get(target).unwrap();
                let mission = &victim.mission;
                assert_eq!(
                    mission.current().raw(),
                    int(&expected["current_mission"]),
                    "{}",
                    row["name"]
                );
                assert_eq!(
                    mission.queued().raw(),
                    int(&expected["queued_mission"]),
                    "{}",
                    row["name"]
                );
                assert_eq!(
                    i32::from(mission.movement_bypass_latch()),
                    int(&expected["bypass"]),
                    "{}",
                    row["name"]
                );
                assert_eq!(
                    [
                        mission.dispatch_timer().start_frame(),
                        mission.ai_counter() as i32,
                        mission.dispatch_timer().delay()
                    ],
                    [
                        int(&expected["mission_timer"][0]),
                        int(&expected["mission_timer"][1]),
                        int(&expected["mission_timer"][2])
                    ],
                    "{}",
                    row["name"]
                );
                assert_eq!(sim.rng_state(), rng, "{}", row["name"]);
            }
        }
        #[test]
        fn ares_emp_ai_drive_hit_and_actual_visits_queue_recovery_after_sparkle() {
            use crate::sim::{mission::MissionId, world::ObjectAiCtx};
            let rules = rules("[CombatDamage]\nEMPAIRecoverMission=Area Guard\n");
            let mut sim = crate::sim::arena_fixture::rival_arena(&rules, 17);
            sim.intern_rule_type_ids(&rules);
            sim.resolve_type_handles(&rules);
            let target = sim
                .spawn_object_at_height("TANK", "Russians", 10, 10, 0, 0, &rules)
                .unwrap();
            let source = sim
                .spawn_object_at_height("TANK", "Americans", 12, 10, 0, 0, &rules)
                .unwrap();
            sim.substrate
                .entities
                .get_mut(target)
                .unwrap()
                .mission
                .set_current_for_test(MissionId::from_raw(5), 0);
            assert_eq!(
                sim.deliver_ares_emp(
                    target,
                    Some(source),
                    rules.warhead("EMPWH").unwrap(),
                    &rules,
                    FrameEffects::empty()
                ),
                EmpDelivery::Applied
            );
            let sparkle = sim
                .substrate
                .entities
                .get(target)
                .unwrap()
                .emp_sparkle()
                .unwrap();
            assert!(
                sim.substrate
                    .entities
                    .get(target)
                    .unwrap()
                    .deactivated
                    .is_offline()
            );
            assert!(sim.object_ai_visit_one(target, Some(&rules), ObjectAiCtx::default()));
            assert_eq!(
                sim.substrate.entities.get(target).unwrap().emp_remaining(),
                1
            );
            assert!(sim.object_ai_visit_one(target, Some(&rules), ObjectAiCtx::default()));
            let victim = sim.substrate.entities.get(target).unwrap();
            assert_eq!(victim.emp_remaining(), 0);
            assert!(!victim.deactivated.is_offline());
            assert_eq!(
                victim.mission.current().raw(),
                5,
                "queue(false) must not Commence"
            );
            assert_eq!(victim.mission.queued().raw(), 11);
            assert!(victim.emp_sparkle().is_none());
            assert_eq!(
                sim.substrate
                    .anims
                    .get(sparkle)
                    .unwrap()
                    .runtime()
                    .loop_remaining,
                0
            );
        }
        #[test]
        fn ares_emp_ai_mcv_cannot_deploy_until_expiry_then_recovers_through_hunt() {
            use crate::sim::{
                base_plan::BasePlanNode, house_state::HouseAiActivationLatches, world::ObjectAiCtx,
            };
            let (mut sim, rules, target, source) = mcv_emp_scene();
            let owner = sim.substrate.entities.get(target).unwrap().owner();
            let house = sim.houses.get_mut(&owner).unwrap();
            house.set_is_human_for_test(false);
            house.set_ai_activation_for_test(HouseAiActivationLatches {
                auto_base_building: true,
                ..house.ai_activation()
            });
            // The existing MCV fixture prior: reanchor an already filled
            // yard plan, without introducing base-plan generation here.
            house.base_plan_mut().set_nodes_for_test(vec![BasePlanNode {
                type_or_control: 0,
                packed_cell: 0,
                filled: true,
                retry_count: 0,
            }]);
            assert!(sim.can_deploy_slash_unload(target, &rules));
            assert_eq!(
                sim.deliver_ares_emp(
                    target,
                    Some(source),
                    rules.warhead("EMPWH").unwrap(),
                    &rules,
                    FrameEffects::empty()
                ),
                EmpDelivery::Applied
            );
            let sparkle = sim
                .substrate
                .entities
                .get(target)
                .unwrap()
                .emp_sparkle()
                .unwrap();
            for frame in 1..=3 {
                assert!(!sim.can_deploy_slash_unload(target, &rules));
                sim.session.set_binary_frame_for_test(frame);
                assert!(sim.object_ai_visit_one(target, Some(&rules), ObjectAiCtx::default()));
                assert!(
                    sim.substrate
                        .entities
                        .get(target)
                        .is_some_and(|entity| entity.lifecycle.object_alive()),
                    "MCV converted during EMP visit{frame}"
                );
                assert!(
                    !sim.substrate
                        .entities
                        .values()
                        .any(|entity| sim.interner.resolve(entity.type_ref()) == "YARD")
                );
            }
            let mcv = sim.substrate.entities.get(target).unwrap();
            assert_eq!(mcv.emp_remaining(), 0);
            assert!(!mcv.deactivated.is_offline());
            assert!(mcv.emp_sparkle().is_none());
            assert_eq!(
                mcv.mission.queued().raw(),
                15,
                "native default AI recovery is queued Hunt"
            );
            assert_eq!(
                sim.substrate
                    .anims
                    .get(sparkle)
                    .unwrap()
                    .runtime()
                    .loop_remaining,
                0
            );
            assert!(sim.can_deploy_slash_unload(target, &rules));
            for frame in 4..=20 {
                sim.session.set_binary_frame_for_test(frame);
                sim.object_ai_visit_one(target, Some(&rules), ObjectAiCtx::default());
                if sim
                    .substrate
                    .entities
                    .get(target)
                    .is_none_or(|entity| !entity.lifecycle.object_alive())
                {
                    break;
                }
            }
            assert!(
                sim.substrate
                    .entities
                    .values()
                    .any(|entity| entity.owner() == owner
                        && sim.interner.resolve(entity.type_ref()) == "YARD"
                        && entity.lifecycle.object_alive()),
                "ordinary recovered AI Hunt must reach the shared MCV conversion owner"
            );
        }
        #[test]
        fn ares_emp_native_cap_and_conversion_goldens() {
            let corpus = corpus();
            let defaults = EmpState::default();
            assert_eq!(
                defaults.last_mission,
                int(&corpus["constructor_defaults"][1]["result"]["last_mission"])
            );
            assert_eq!(
                defaults.sparkle.is_some(),
                corpus["constructor_defaults"][1]["result"]["sparkle_present"]
                    .as_bool()
                    .unwrap()
            );
            for row in corpus["caps"].as_array().unwrap() {
                let f = &row["fixture"];
                assert_eq!(
                    capped_duration(int(&f["old"]), int(&f["delta"]), int(&f["cap"])),
                    int(&row["result"]),
                    "{row}"
                );
            }
            for row in corpus["deliver"].as_array().unwrap() {
                if row["converted_deltas"].as_array().unwrap().is_empty() {
                    continue;
                }
                let f = &row["fixture"];
                let duration = f["duration"].as_i64().unwrap_or(120) as i32;
                let modifier = f["modifier"].as_f64().unwrap_or(1.0);
                assert_eq!(
                    duration_delta(duration, NativeF64Bits::from_bits(modifier.to_bits())),
                    int(&row["converted_deltas"][0]),
                    "{}",
                    row["name"]
                );
            }
        }
        fn mcv_emp_scene() -> (Simulation, RuleSet, u64, u64) {
            use crate::rules::{art_data::ArtRegistry, native_processing::RulesLayerStack};
            let ini = IniFile::from_str(
                "[VehicleTypes]\n0=MCV\n1=SOURCE\n[BuildingTypes]\n0=YARD\n[Warheads]\n0=EMPWH\n[Animations]\n0=SPARK\n[AI]\nBuildConst=YARD\n[MCV]\nStrength=1000\nArmor=heavy\nSpeed=5\nROT=5\nLocomotor={4A582741-9839-11d1-B709-00A024DDAFD1}\nDeploysInto=YARD\n[SOURCE]\nStrength=400\nLocomotor={4A582741-9839-11d1-B709-00A024DDAFD1}\n[YARD]\nStrength=1000\nConstructionYard=yes\nDeployFacing=4\n[EMPWH]\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\nEMP.Duration=3\nEMP.Cap=3\nEMP.Sparkles=SPARK\n[Clear]\nBuildable=yes\n[Hunt]\nRate=0.016\n[Guard]\nRate=0.016\n[Unload]\nRate=0.016\n",
            );
            let art =
                IniFile::from_str("[YARD]\nFoundation=4x3\n[SPARK]\nLoopCount=-1\nRate=1\nEnd=4\n");
            let mut rules = RuleSet::from_processed_rules(
                &RulesLayerStack::new(ini)
                    .process_with_fixed_art(&art)
                    .unwrap(),
            )
            .unwrap();
            let mut art = ArtRegistry::from_ini(&art);
            art.bind_anim_frame_count_for_test("SPARK", 4);
            rules.install_art_data(art);
            let mut sim = crate::sim::arena_fixture::rival_arena(&rules, 17);
            sim.session
                .set_game_mode(crate::sim::scenario_session::ScenarioGameMode::Skirmish);
            sim.intern_rule_type_ids(&rules);
            sim.resolve_type_handles(&rules);
            let target = sim
                .spawn_object_at_height("MCV", "Americans", 20, 22, 128, 0, &rules)
                .unwrap();
            let source = sim
                .spawn_object_at_height("SOURCE", "Russians", 5, 5, 0, 0, &rules)
                .unwrap();
            (sim, rules, target, source)
        }
        #[test]
        fn ares_emp_mcv_shared_admission_and_pending_unload_respect_emp() {
            use crate::sim::{
                mission::{MissionId, authority::LiveReadyInputProvider},
                world::ObjectAiCtx,
            };
            let native = crate::test_fixture::json("tools/mcv_deploy_oracle.json");
            for row in native["deploy_admission"].as_array().unwrap() {
                // Unit DeathFrame's independent virtual override is not a
                // represented runtime state of this ordinary live MCV fixture.
                if int(&row["death_frame"]) != -1 {
                    continue;
                }
                let (mut sim, rules, target, _) = mcv_emp_scene();
                sim.substrate
                    .entities
                    .get_mut(target)
                    .unwrap()
                    .emp
                    .remaining = int(&row["emp_counter"]);
                assert_eq!(
                    sim.can_deploy_slash_unload(target, &rules),
                    row["result"]["allowed"].as_bool().unwrap(),
                    "{row}"
                );
                if row["entry"] == "unit_deploy" && !row["result"]["allowed"].as_bool().unwrap() {
                    sim.substrate
                        .entities
                        .get_mut(target)
                        .unwrap()
                        .mcv_deploy_pending
                        .set_for_test(true);
                    assert!(!sim.deploy_mcv(target, &rules, None, FrameEffects::empty()));
                    assert_eq!(
                        u8::from(
                            sim.substrate
                                .entities
                                .get(target)
                                .unwrap()
                                .mcv_deploy_pending
                                .is_pending()
                        ),
                        int(&row["result"]["deploy_retry"]) as u8
                    );
                }
            }
            let (mut sim, rules, target, source) = mcv_emp_scene();
            // A deploy already commenced before the hit. Unload defers EMP
            // Deactivate, so the shared CanDeploy owner must protect its AI.
            sim.mission_queue_exact(
                target,
                MissionId::from_known(MissionType::Unload),
                1,
                0,
                &LiveReadyInputProvider { rules: &rules },
            )
            .unwrap();
            assert_eq!(
                sim.substrate
                    .entities
                    .get(target)
                    .unwrap()
                    .mission
                    .current()
                    .known(),
                Some(MissionType::Unload)
            );
            assert_eq!(
                sim.deliver_ares_emp(
                    target,
                    Some(source),
                    rules.warhead("EMPWH").unwrap(),
                    &rules,
                    FrameEffects::empty()
                ),
                EmpDelivery::Applied
            );
            assert!(!sim.can_deploy_slash_unload(target, &rules));
            sim.session.set_binary_frame_for_test(1);
            assert!(sim.object_ai_visit_one(target, Some(&rules), ObjectAiCtx::default()));
            let mcv = sim
                .substrate
                .entities
                .get(target)
                .expect("pending Unload must not convert during EMP");
            assert!(mcv.lifecycle.object_alive());
            assert_eq!(mcv.emp_remaining(), 2);
            assert!(
                !sim.substrate
                    .entities
                    .values()
                    .any(|entity| sim.interner.resolve(entity.type_ref()) == "YARD")
            );
        }
        #[test]
        fn ares_emp_shared_activation_matches_native_hook_gates() {
            for row in corpus()["activation"].as_array().unwrap() {
                let fixture = &row["fixture"];
                let rules = rules(&format!(
                    "[TANK]\nPoweredUnit={}\n[BuildingTypes]\n0=CENTER\n[CENTER]\nStrength=400\nPowersUnit=TANK\n[Clear]\nBuildable=yes\n",
                    if fixture["powered_unit"].as_bool().unwrap() {
                        "yes"
                    } else {
                        "no"
                    }
                ));
                let mut sim = crate::sim::arena_fixture::rival_arena(&rules, 17);
                sim.intern_rule_type_ids(&rules);
                sim.resolve_type_handles(&rules);
                let target = sim
                    .spawn_object_at_height("TANK", "Americans", 10, 10, 0, 0, &rules)
                    .unwrap();
                if let Some(counter) = fixture["center_counter"].as_i64() {
                    let center = sim
                        .spawn_object_at_height("CENTER", "Americans", 16, 16, 0, 0, &rules)
                        .unwrap();
                    let center = sim.substrate.entities.get_mut(center).unwrap();
                    center.building_powered_center.set_for_test(true);
                    center.emp.remaining = counter as i32;
                }
                let victim = sim.substrate.entities.get_mut(target).unwrap();
                victim.emp.remaining = int(&fixture["counter"]);
                victim
                    .deactivated
                    .set_for_test(fixture["deactivated"].as_bool().unwrap());
                victim
                    .mission
                    .set_current_for_test(crate::sim::mission::MissionId::from_raw(2), 7);
                let rng = sim.rng_state();
                sim.techno_reactivate(target, &rules, FrameEffects::empty());
                let victim = sim.substrate.entities.get(target).unwrap();
                let remains_offline = row["after"]["deactivated"].as_bool().unwrap();
                assert_eq!(
                    victim.deactivated.is_offline(),
                    remains_offline,
                    "{}",
                    row["name"]
                );
                if remains_offline {
                    assert_eq!(
                        victim.mission.current().raw(),
                        2,
                        "veto before ResetOrders: {}",
                        row["name"]
                    );
                    assert_eq!(victim.mission.handler_state(), 7, "{}", row["name"]);
                }
                assert_eq!(sim.rng_state(), rng, "{}", row["name"]);
            }
        }
        #[test]
        fn ares_emp_power_center_return_cannot_reactivate_an_emp_victim() {
            let rules = rules(
                "[TANK]\nPoweredUnit=yes\n[BuildingTypes]\n0=CENTER\n[CENTER]\nStrength=400\nPowersUnit=TANK\n[Clear]\nBuildable=yes\n",
            );
            let mut sim = crate::sim::arena_fixture::rival_arena(&rules, 17);
            sim.intern_rule_type_ids(&rules);
            sim.resolve_type_handles(&rules);
            let target = sim
                .spawn_object_at_height("TANK", "Americans", 10, 10, 0, 0, &rules)
                .unwrap();
            let source = sim
                .spawn_object_at_height("TANK", "Russians", 12, 10, 0, 0, &rules)
                .unwrap();
            let center = sim
                .spawn_object_at_height("CENTER", "Americans", 16, 16, 0, 0, &rules)
                .unwrap();
            sim.substrate
                .entities
                .get_mut(target)
                .unwrap()
                .selection
                .select();
            assert_eq!(
                sim.deliver_ares_emp(
                    target,
                    Some(source),
                    rules.warhead("EMPWH").unwrap(),
                    &rules,
                    FrameEffects::empty()
                ),
                EmpDelivery::Applied
            );
            let owner = sim.substrate.entities.get(target).unwrap().owner();
            assert!(!sim.has_powered_center(owner));
            sim.powered_center_edge(center, true, &rules, FrameEffects::empty());
            assert!(sim.has_powered_center(owner));
            let victim = sim.substrate.entities.get(target).unwrap();
            assert!(
                victim.deactivated.is_offline(),
                "House50E010 must pass the shared EMP veto"
            );
            assert!(victim.selection.is_selected());
            assert_eq!(victim.emp_remaining(), 2);
            sim.tick_ares_emp(target, &rules, FrameEffects::empty());
            assert!(
                sim.substrate
                    .entities
                    .get(target)
                    .unwrap()
                    .deactivated
                    .is_offline()
            );
            sim.tick_ares_emp(target, &rules, FrameEffects::empty());
            assert!(
                !sim.substrate
                    .entities
                    .get(target)
                    .unwrap()
                    .deactivated
                    .is_offline()
            );
        }
        #[test]
        fn ares_emp_expired_sparkle_refresh_preserves_native_fallback_provenance() {
            for row in corpus()["sparkle_refresh"].as_array().unwrap() {
                let fixture = &row["fixture"];
                let mut extra = format!(
                    "EMP.Duration={}\nEMP.Cap={}\nEMP.Sparkles={}\n",
                    int(&fixture["duration"]),
                    int(&fixture["cap"]),
                    if fixture["sparkle"].as_bool().unwrap_or(false) {
                        "SPARK"
                    } else {
                        "none"
                    }
                );
                if fixture["type_sparkle"].as_bool().unwrap_or(false) {
                    extra.push_str("[TANK]\nEMP.Sparkles=SPARK\n");
                }
                if fixture["default_sparkle"].as_bool().unwrap_or(false) {
                    extra.push_str("[General]\nEMPulseSparkles=SPARK\n");
                }
                let rules = rules(&extra);
                let mut sim = crate::sim::arena_fixture::rival_arena(&rules, 17);
                sim.intern_rule_type_ids(&rules);
                sim.resolve_type_handles(&rules);
                let target = sim
                    .spawn_object_at_height("TANK", "Americans", 10, 10, 0, 0, &rules)
                    .unwrap();
                let source = sim
                    .spawn_object_at_height("TANK", "Russians", 12, 10, 0, 0, &rules)
                    .unwrap();
                let warhead = rules.warhead("EMPWH").unwrap();
                assert_eq!(
                    sim.deliver_ares_emp(
                        target,
                        Some(source),
                        warhead,
                        &rules,
                        FrameEffects::empty()
                    ),
                    EmpDelivery::Applied
                );
                let held = sim
                    .substrate
                    .entities
                    .get(target)
                    .unwrap()
                    .emp_sparkle()
                    .unwrap();
                assert_eq!(
                    sim.substrate.entities.get(target).unwrap().emp_remaining(),
                    int(&row["first"]["counter"])
                );
                sim.notify_anim_pointer_expired(held);
                assert_eq!(
                    sim.substrate
                        .entities
                        .get(target)
                        .unwrap()
                        .emp_sparkle()
                        .is_some(),
                    row["expired"]["sparkle_present"].as_bool().unwrap()
                );
                let rng = sim.rng_state();
                let anim_count = sim.substrate.anims.len();
                assert_eq!(
                    sim.deliver_ares_emp(
                        target,
                        Some(source),
                        warhead,
                        &rules,
                        FrameEffects::empty()
                    ),
                    EmpDelivery::Applied
                );
                let victim = sim.substrate.entities.get(target).unwrap();
                assert_eq!(
                    victim.emp_remaining(),
                    int(&row["after"]["counter"]),
                    "{}",
                    row["name"]
                );
                let constructed = row["after"]["sparkle_present"].as_bool().unwrap();
                assert_eq!(
                    victim.emp_sparkle().is_some(),
                    constructed,
                    "{}",
                    row["name"]
                );
                assert_eq!(
                    sim.substrate.anims.len(),
                    anim_count + usize::from(constructed),
                    "{}",
                    row["name"]
                );
                assert_eq!(
                    sim.rng_state(),
                    rng,
                    "synthetic fixed-rate Anim: {}",
                    row["name"]
                );
            }
        }
        #[test]
        fn ares_emp_native_collector_uses_existing_cells_coordinates_and_uid() {
            let rules = rules("");
            for row in corpus()["collectors"].as_array().unwrap() {
                let f = &row["fixture"];
                let impact = ProjectileCoord {
                    x: int(&f["impact"][0]),
                    y: int(&f["impact"][1]),
                    z: int(&f["impact"][2]),
                };
                let mut sim = crate::sim::arena_fixture::rival_arena(&rules, 17);
                sim.intern_rule_type_ids(&rules);
                let bridge = f["bridge"].as_bool().unwrap();
                if bridge {
                    use crate::map::{
                        cell_index::NativeCellIdentity, resolved_terrain::NativeCellQuery,
                    };
                    let terrain = sim.resolved_terrain.as_mut().unwrap();
                    let NativeCellIdentity::Real(index) =
                        NativeCellQuery::canonical(terrain).lookup((10, 10))
                    else {
                        panic!("real fixture cell");
                    };
                    let mut flags = terrain.capture_real_cell_bridge_flags_0x1180();
                    flags.set_allocated_cell(index, 0x100);
                    assert!(terrain.restore_real_cell_bridge_flags_0x1180(&flags));
                }
                for (index, object) in f["objects"].as_array().unwrap().iter().enumerate() {
                    let id = index as u64 + 100; // Stable order differs from native UID order.
                    let mut entity = GameEntity::test_default(id, "TANK", "Americans", 10, 10);
                    entity.set_type_ref_for_test(sim.interner.intern("TANK"));
                    entity.set_native_unique_id_for_test(int(&object["uid"]));
                    entity
                        .position
                        .put_world_xy([int(&object["coords"][0]), int(&object["coords"][1])]);
                    entity
                        .position
                        .set_exact_z_leptons_for_test(Some(int(&object["coords"][2])));
                    sim.substrate.entities.insert(entity);
                    let layer = if bridge {
                        MovementLayer::Bridge
                    } else {
                        MovementLayer::Ground
                    };
                    sim.substrate.occupancy.add(
                        10,
                        10,
                        id,
                        layer,
                        None,
                        CellListInsertion::PrependNonBuilding,
                    );
                    if f["duplicate_neighbor"].as_bool().unwrap() {
                        sim.substrate.occupancy.add(
                            11,
                            10,
                            id,
                            layer,
                            None,
                            CellListInsertion::PrependNonBuilding,
                        );
                    }
                }
                let ids =
                    sim.ares_effect_ground_targets(impact, f["spread"].as_f64().unwrap(), &rules);
                let uids: Vec<_> = ids
                    .iter()
                    .map(|id| sim.substrate.entities.get(*id).unwrap().native_unique_id())
                    .collect();
                let expected: Vec<_> = row["result"]["ordered_uids"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(int)
                    .collect();
                assert_eq!(uids, expected, "{}", row["name"]);
                let radius = (f["spread"].as_f64().unwrap() + 0.99) as i32;
                let mut visited = Vec::new();
                visit_emp_cells((10, 10), radius, |cell| {
                    visited.push([i32::from(cell.0), i32::from(cell.1)])
                });
                let expected: Vec<_> = row["result"]["visited_cells"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|cell| [int(&cell[0]), int(&cell[1])])
                    .collect();
                assert_eq!(visited, expected, "{}", row["name"]);
            }
        }
        #[test]
        fn ares_emp_native_ground_delivery_admission_and_transitions() {
            for row in corpus()["deliver"].as_array().unwrap() {
                let f = &row["fixture"];
                // These native controls establish later actor/getter branches,
                // not the ordinary Drive victim's production configuration.
                if f.get("actor_kind").is_some()
                    || f["warping"].as_bool().unwrap_or(false)
                    || f["normal_emp_slot"].as_i64().unwrap_or(0) >= 18
                    || f["elite_emp_slot"].as_i64().unwrap_or(0) >= 18
                {
                    continue;
                }
                let yesno = |key: &str, default: bool| {
                    if f[key].as_bool().unwrap_or(default) {
                        "yes"
                    } else {
                        "no"
                    }
                };
                let owner = f["affects_owner"].as_bool().map_or(String::new(), |value| {
                    format!("AffectsOwner={}\n", if value { "yes" } else { "no" })
                });
                let immune = f["immune"].as_bool().map_or(String::new(), |value| {
                    format!("ImmuneToEMP={}\n", if value { "yes" } else { "no" })
                });
                let weapon = if f["normal_emp_slot"].as_i64() == Some(1) {
                    "Secondary=EMPWeapon\n[EMPWeapon]\nDamage=0\nWarhead=EMPWH\n"
                } else {
                    ""
                };
                let verses = vec![f["verses"].as_f64().unwrap_or(1.0).to_string(); 11].join(",");
                let rules = rules(&format!(
                    "Verses={verses}\nEMP.Duration={}\nEMP.Cap={}\nEMP.Sparkles=none\nAffectsAllies={}\nAffectsEnemies={}\n{owner}[TANK]\nEMP.Modifier={}\nOrganic={}\nTypeImmune={}\n{immune}VeteranAbilities={}\n{weapon}",
                    f["duration"].as_i64().unwrap_or(120),
                    f["cap"].as_i64().unwrap_or(120),
                    yesno("affects_allies", false),
                    yesno("affects_enemies", true),
                    f["modifier"].as_f64().unwrap_or(1.0),
                    yesno("organic", false),
                    yesno("type_immune", false),
                    if f["veteran_immune"].as_bool().unwrap_or(false) {
                        "EMPIMMUNE"
                    } else {
                        "RADIMMUNE"
                    },
                ));
                let mut sim = crate::sim::arena_fixture::rival_arena(&rules, 17);
                sim.intern_rule_type_ids(&rules);
                let target = sim
                    .spawn_object_at_height("TANK", "Americans", 10, 10, 0, 0, &rules)
                    .unwrap();
                let source_owner = if f["same_owner"].as_bool().unwrap_or(false) {
                    "Americans"
                } else {
                    "Russians"
                };
                let source = sim
                    .spawn_object_at_height("TANK", source_owner, 12, 10, 0, 0, &rules)
                    .unwrap();
                if f["allied"].as_bool().unwrap_or(false) {
                    sim.house_alliances_mut_for_test()
                        .entry(source_owner.to_ascii_uppercase())
                        .or_default()
                        .insert("AMERICANS".into());
                }
                {
                    let victim = sim.substrate.entities.get_mut(target).unwrap();
                    victim.emp.remaining = f["old"].as_i64().unwrap_or(0) as i32;
                    victim.emp.last_mission = 0;
                    victim
                        .deactivated
                        .set_for_test(f["deactivated"].as_bool().unwrap_or(false));
                    victim.selection.select();
                    victim
                        .mission
                        .set_current_for_test(crate::sim::mission::MissionId::from_raw(5), 0);
                    victim
                        .veterancy
                        .set_rank_for_test((f["rank"].as_f64().unwrap_or(0.0) * 100.0) as u16);
                    if f["iron_curtain"].as_bool().unwrap_or(false) {
                        crate::sim::superweapon::invulnerability::apply_invulnerability(
                            victim,
                            0,
                            100,
                            crate::sim::superweapon::invulnerability::InvulnKind::IronCurtain,
                        );
                    }
                }
                let source = (!f["no_firer"].as_bool().unwrap_or(false)).then_some(source);
                for expected in row["result"].as_array().unwrap() {
                    let verdict = sim.deliver_ares_emp(
                        target,
                        source,
                        rules.warhead("EMPWH").unwrap(),
                        &rules,
                        FrameEffects::empty(),
                    );
                    assert_ne!(verdict, EmpDelivery::Unsupported, "{}", row["name"]);
                    let actual = sim.substrate.entities.get(target).unwrap();
                    assert_eq!(
                        actual.emp_remaining(),
                        int(&expected["counter"]),
                        "{}",
                        row["name"]
                    );
                    assert_eq!(
                        actual.emp.last_mission,
                        int(&expected["saved_mission"]),
                        "{}",
                        row["name"]
                    );
                    assert_eq!(
                        actual.deactivated.is_offline(),
                        expected["deactivated"].as_bool().unwrap(),
                        "{}",
                        row["name"]
                    );
                    assert_eq!(
                        actual.selection.is_selected(),
                        expected["selected"].as_bool().unwrap(),
                        "{}",
                        row["name"]
                    );
                }
            }
        }
        #[test]
        fn ares_emp_native_ai_visits_defer_unload_and_restore_without_reselect() {
            let rules = rules("EMP.Sparkles=none\n");
            for row in corpus()["ticks"].as_array().unwrap() {
                let mut sim = crate::sim::arena_fixture::rival_arena(&rules, 17);
                sim.intern_rule_type_ids(&rules);
                let target = sim
                    .spawn_object_at_height("TANK", "Americans", 10, 10, 0, 0, &rules)
                    .unwrap();
                {
                    let victim = sim.substrate.entities.get_mut(target).unwrap();
                    victim.emp.remaining = int(&row["fixture"]["old"]);
                    victim.emp.last_mission = 0; // Explicit prior in native fixture, not constructor.
                    victim
                        .deactivated
                        .set_for_test(row["fixture"]["deactivated"].as_bool().unwrap_or(false));
                    victim.selection.select();
                }
                let rng = sim.scenario_rng.logical_state();
                for (index, mission) in row["missions"].as_array().unwrap().iter().enumerate() {
                    sim.substrate
                        .entities
                        .get_mut(target)
                        .unwrap()
                        .mission
                        .set_current_for_test(
                            crate::sim::mission::MissionId::from_raw(int(mission)),
                            0,
                        );
                    sim.tick_ares_emp(target, &rules, FrameEffects::empty());
                    let actual = sim.substrate.entities.get(target).unwrap();
                    let expected = &row["result"][index];
                    assert_eq!(
                        actual.emp_remaining(),
                        int(&expected["counter"]),
                        "{} visit{index}",
                        row["name"]
                    );
                    assert_eq!(
                        actual.emp.last_mission,
                        int(&expected["saved_mission"]),
                        "{} visit{index}",
                        row["name"]
                    );
                    assert_eq!(
                        actual.deactivated.is_offline(),
                        expected["deactivated"].as_bool().unwrap(),
                        "{} visit{index}",
                        row["name"]
                    );
                    assert_eq!(
                        actual.selection.is_selected(),
                        expected["selected"].as_bool().unwrap(),
                        "{} visit{index}",
                        row["name"]
                    );
                }
                assert_eq!(sim.scenario_rng.logical_state(), rng, "{}", row["name"]);
            }
        }
        #[test]
        fn ares_emp_ground_hit_save_expiry_and_anim_pointer_lifecycle() {
            let rules = rules("");
            let mut sim = crate::sim::arena_fixture::rival_arena(&rules, 17);
            sim.intern_rule_type_ids(&rules);
            sim.resolve_type_handles(&rules);
            sim.session
                .set_current_house_for_test(sim.interner.get("Americans"));
            let target = sim
                .spawn_object_at_height("TANK", "Americans", 10, 10, 0, 0, &rules)
                .unwrap();
            let source = sim
                .spawn_object_at_height("TANK", "Russians", 12, 10, 0, 0, &rules)
                .unwrap();
            {
                let victim = sim.substrate.entities.get_mut(target).unwrap();
                victim
                    .mission
                    .set_current_for_test(crate::sim::mission::MissionId::from_raw(5), 0);
                victim.selection.select();
            }
            let coord = crate::sim::movement::ground_pose::position_world_coord(
                &sim.substrate.entities.get(target).unwrap().position,
            );
            let impact = ProjectileCoord {
                x: coord.x,
                y: coord.y,
                z: coord.z,
            };
            let rng = sim.scenario_rng.logical_state();
            sim.apply_ares_emp_detonation(
                impact,
                Some(source),
                rules.warhead("EMPWH").unwrap(),
                &rules,
                FrameEffects::empty(),
            );
            let expected = &corpus()["connected"][0]["result"];
            let assert_state = |sim: &Simulation, phase: usize| {
                let victim = sim.substrate.entities.get(target).unwrap();
                assert_eq!(victim.emp_remaining(), int(&expected[phase]["counter"]));
                assert_eq!(
                    victim.deactivated.is_offline(),
                    expected[phase]["deactivated"].as_bool().unwrap()
                );
                assert_eq!(
                    victim.selection.is_selected(),
                    expected[phase]["selected"].as_bool().unwrap()
                );
                assert_eq!(
                    victim.emp.last_mission,
                    int(&expected[phase]["saved_mission"])
                );
                assert_eq!(
                    victim.emp_sparkle().is_some(),
                    expected[phase]["sparkle_present"].as_bool().unwrap()
                );
            };
            assert_state(&sim, 0);
            assert!(!sim.techno_player_controllable(target, &rules));
            let held = sim
                .substrate
                .entities
                .get(target)
                .unwrap()
                .emp_sparkle()
                .unwrap();
            assert_eq!(
                sim.substrate
                    .anims
                    .get(held)
                    .unwrap()
                    .runtime()
                    .loop_remaining,
                255
            );
            // Actual hit and Drive/Walk transition draw no RNG; the Anim's
            // constructor uses its own established allocation/initialization owner.
            assert_eq!(sim.scenario_rng.logical_state(), rng);
            let bytes = crate::sim::snapshot::GameSnapshot::save(&sim, 0, 0, "EMP", 0);
            for row in corpus()["pointer_expiry"].as_array().unwrap() {
                let mut probe = crate::sim::snapshot::GameSnapshot::load(&bytes)
                    .unwrap()
                    .sim;
                let expired = if row["fixture"]["matching"].as_bool().unwrap() {
                    held
                } else {
                    held + 1
                };
                probe.notify_anim_pointer_expired(expired);
                let victim = probe.substrate.entities.get(target).unwrap();
                assert_eq!(victim.emp_remaining(), int(&row["result"]["counter"]));
                assert_eq!(
                    victim.emp_sparkle().is_some(),
                    row["result"]["sparkle_present"].as_bool().unwrap()
                );
                assert_eq!(
                    probe
                        .substrate
                        .anims
                        .get(held)
                        .unwrap()
                        .runtime()
                        .loop_remaining,
                    int(&row["result"]["animation_iterations"][0]) as u8
                );
            }
            sim = crate::sim::snapshot::GameSnapshot::load(&bytes)
                .unwrap()
                .sim;
            sim.resolve_type_handles(&rules);
            assert_state(&sim, 0);
            sim.tick_ares_emp(target, &rules, FrameEffects::empty());
            assert_state(&sim, 1);
            sim.tick_ares_emp(target, &rules, FrameEffects::empty());
            assert_state(&sim, 2);
            assert!(sim.techno_player_controllable(target, &rules));
            assert_eq!(
                sim.substrate
                    .anims
                    .get(held)
                    .unwrap()
                    .runtime()
                    .loop_remaining,
                0
            );
            let victim = sim.substrate.entities.get_mut(target).unwrap();
            victim.emp.sparkle = Some(held);
            victim.emp_anim_pointer_expired(held + 1);
            assert_eq!(victim.emp_sparkle(), Some(held));
            sim.notify_anim_pointer_expired(held);
            assert_eq!(
                sim.substrate.entities.get(target).unwrap().emp_sparkle(),
                None
            );
        }
    }
}
