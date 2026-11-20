//! Phobos `ExtraThreat.*` / `ExtraThreatCoefficient.*` threat-score bonuses,
//! ported from `TechnoClass_ThreatCoefficient_CanAttackMeThreatBonus`
//! (`0x70CF87`, `Ext/Techno/Hooks.TargetEvaluation.cpp:429-511`).
//!
//! The hook runs inside `TechnoClass::Calculate_Threat_Score @ 0x0070CD10`
//! after the B, C, enemy-house and A terms have been stored to the stack's
//! `totalThreat` double and before the D (health) and E (distance) terms, so
//! [`super::greatest_threat`]'s score body calls [`apply_extra_threat_bonus`]
//! at exactly that point. Each bonus is skipped when its resolved value is
//! `0.0`, and the four additions keep the native order:
//!
//! 1. `ExtraThreat.IsThreat` when `IsAThreatToMe` holds.
//! 2. `ExtraThreat.InRange` + `distanceCells * ExtraThreatCoefficient.InRangeDistance`
//!    when the scanner is close enough to attack.
//! 3. `deltaFacing * ExtraThreatCoefficient.Facing`, the angular difference
//!    between the target direction and the class's fire facing.
//! 4. `targetDistanceToLastTarget * ExtraThreatCoefficient.DistanceToLastTarget`
//!    unless the retained last-target coordinate is `(0, 0, 0)`.
//!
//! The per-object `LastTargetCrd` state itself is maintained by
//! `update_phobos_extra_threat_last_target` (`TechnoExt::UpdateLastTargetCrd`,
//! `Ext/Techno/Body.Update.cpp:1286-1309`, from `OnEarlyUpdate` at `:24`).
//!
//! ## Dependency rules
//! - Part of sim/ — depends on rules/, map/ and sim/ only.
//! - sim/ NEVER depends on render/, ui/, audio/, net/.

/// The live world the bonus's fire-error, InRange, facing and coordinate
/// queries read. `overlay_registry` is the caller's registry when it has one
/// ([`super::greatest_threat`]'s scan); the retaliation caller passes `None`,
/// which only leaves walls out of the line-of-fire walk.
///
/// The fields exist only with the feature; builds without it keep the type as
/// an empty placeholder so the shared score signature stays stable.
#[derive(Clone, Copy)]
pub(crate) struct ThreatBonusContext<'a> {
    pub world: &'a crate::sim::world::Simulation,
    pub overlay_registry: Option<&'a crate::rules::overlay_types::OverlayTypeRegistry>,
}

impl<'a> ThreatBonusContext<'a> {
    /// The context a live world supplies. Without the feature the bonus body
    /// is not compiled, so there is no context to carry.
    pub(crate) fn for_world(
        world: &'a crate::sim::world::Simulation,
        overlay_registry: Option<&'a crate::rules::overlay_types::OverlayTypeRegistry>,
    ) -> Option<Self> {
        {
            Some(Self {
                world,
                overlay_registry,
            })
        }
    }
}

mod implementation {
    use super::ThreatBonusContext;
    use crate::map::entities::EntityCategory;
    use crate::rules::object_type::ObjectType;
    use crate::rules::ruleset::RuleSet;
    use crate::sim::combat::combat_weapon::SelectedWeapon;
    use crate::sim::combat::fire_error::FireError;
    use crate::sim::combat::fire_error_world::FireSubject;
    use crate::sim::combat::{TargetKind, in_range};
    use crate::sim::game_entity::GameEntity;
    use crate::sim::movement::ground_pose::{object_get_coords, target_get_coords};
    use crate::sim::world::Simulation;
    use crate::util::native_x87::{MaskedX87Chop53 as ScoreX87, MaskedX87Value, NativeF64Bits};

    /// The stack double at the `0x70CF87` hook point, added to in double
    /// precision like Phobos' `/arch:SSE2` `totalThreat += bonus`.
    #[inline]
    fn spilled(score: MaskedX87Value) -> f64 {
        f64::from_bits(ScoreX87::store_f64_masked_chop(score).bits())
    }

    #[inline]
    fn reload(total: f64) -> MaskedX87Value {
        ScoreX87::load_f64(NativeF64Bits::from_bits(total.to_bits()))
    }

    /// `IsAThreatToMe(pThis, pTarget)` (`Hooks.TargetEvaluation.cpp:381-404`):
    /// the candidate is a Techno, counts unconditionally with
    /// `AlwaysConsideredThreat=yes`, or its selected weapon against the scanner
    /// (`SelectWeapon(pThis)`) exists and `GetFireError(scanner, slot,
    /// ignoreRange=true)` is not `ILLEGAL` — a Building also rejects `RANGE`.
    fn is_a_threat_to_me(
        scorer: &GameEntity,
        candidate: &GameEntity,
        candidate_type: &ObjectType,
        candidate_weapon: Option<&SelectedWeapon<'_>>,
        context: ThreatBonusContext<'_>,
        rules: &RuleSet,
    ) -> bool {
        if candidate_type.phobos_extra_threat.always_considered_threat {
            return true;
        }
        // `GetWeapon(slot)->WeaponType == null` answers "not a threat"; the
        // ladder's `None` is exactly that null weapon (an index that names no
        // WeaponType).
        let Some(weapon) = candidate_weapon else {
            return false;
        };
        let world = context.world;
        let subject = FireSubject {
            world,
            rules,
            fog: Some(world.fog.fire_view(&world.house_alliances)),
            firer: candidate,
            obj: candidate_type,
            target: Some(TargetKind::Entity(scorer.stable_id())),
            weapon_index: weapon.index,
        };
        // `GetFireError(pThis, weaponIndex, true)`: `ignoreRange=true` is
        // `check_range=false`, so a target merely out of range still counts.
        let error = subject.fire_error(false);
        if candidate.category() == EntityCategory::Structure {
            error != FireError::Illegal && error != FireError::Range
        } else {
            error != FireError::Illegal
        }
    }

    /// `GetFireFacing` (`Hooks.TargetEvaluation.cpp:404-427`): Building →
    /// PrimaryFacing; Unit → SecondaryFacing with `Turret=yes`, else
    /// PrimaryFacing; Aircraft → SecondaryFacing; Infantry → none.
    fn fire_facing(scorer: &GameEntity, scorer_type: &ObjectType, frame: u32) -> Option<u16> {
        let body = || scorer.body_facing_current(frame);
        let turret = || {
            scorer
                .barrel_facing
                .map_or_else(body, |facing| facing.current(frame))
        };
        match scorer.category() {
            EntityCategory::Structure => Some(body()),
            EntityCategory::Infantry => None,
            EntityCategory::Aircraft => Some(turret()),
            EntityCategory::Unit => Some(if scorer_type.has_turret {
                turret()
            } else {
                body()
            }),
        }
    }

    /// `IsCloseEnoughToAttack(pTarget)` through the shared InRange owner
    /// (`CanFireAtTarget 0x006F7780`). A world without resolved terrain
    /// answers the map-less policy like [`in_range::mapless_in_range`].
    fn close_enough_to_attack(
        scorer: &GameEntity,
        scorer_type: &ObjectType,
        candidate: &GameEntity,
        context: ThreatBonusContext<'_>,
        rules: &RuleSet,
    ) -> bool {
        let world = context.world;
        let target = TargetKind::Entity(candidate.stable_id());
        let Some(terrain) = world.resolved_terrain.as_ref() else {
            let entities = &world.substrate.entities;
            let Some(weapon) = crate::sim::combat::combat_weapon::current_weapon(
                scorer,
                scorer_type,
                entities,
                rules,
                &world.interner,
            ) else {
                return false;
            };
            return in_range::mapless_in_range(
                scorer,
                in_range::MaplessTarget::Techno(candidate),
                weapon,
                rules,
                &world.interner,
                Some(entities),
            );
        };
        let los = crate::sim::combat::line_of_fire::LineOfFireInputs {
            overlay_grid: world.overlay_grid.as_ref(),
            overlay_registry: context.overlay_registry,
            alliances: Some(&world.house_alliances),
            buildings: Some(crate::sim::combat::line_of_fire::CellBuildings::of(world)),
        };
        in_range::can_fire_at_target(
            world,
            rules,
            &world.interner,
            scorer.stable_id(),
            &target,
            terrain,
            Some(&world.house_alliances),
            &los,
        )
    }

    /// The four `TechnoClass_ThreatCoefficient_CanAttackMeThreatBonus`
    /// additions, in native order, on the score the caller has accumulated up
    /// to the `0x70CF87` hook. `candidate_weapon` is the candidate's
    /// `SelectWeapon` result the score body's B term already resolved.
    pub(crate) fn apply_extra_threat_bonus(
        score: MaskedX87Value,
        scorer: &GameEntity,
        scorer_type: &ObjectType,
        candidate: &GameEntity,
        candidate_type: &ObjectType,
        candidate_weapon: Option<&SelectedWeapon<'_>>,
        context: ThreatBonusContext<'_>,
        rules: &RuleSet,
    ) -> MaskedX87Value {
        let settings = &scorer_type.phobos_extra_threat;
        if !settings.enabled(rules) {
            return score;
        }
        let world = context.world;
        let terrain = world.resolved_terrain.as_ref();
        let mut total = spilled(score);

        // 1. `ApplyIsThreatBonus` (`Hooks.TargetEvaluation.cpp:441-453`).
        let bonus = settings.is_threat(rules);
        if bonus != 0.0
            && is_a_threat_to_me(
                scorer,
                candidate,
                candidate_type,
                candidate_weapon,
                context,
                rules,
            )
        {
            total += bonus;
        }

        // 2. `ApplyInRangeBonus` (`:455-470`): `DistanceFrom` is
        // `AbstractClass::DistanceFrom @ 0x005F6440`, an int — the body sums
        // `dx² + dy²`, runs `Sqrt_Approx @ 0x004CAC40` and `ftol`, then
        // subtracts `(Width + Height) * 64` for a building — which
        // `object_distance_to` owns. Dividing that int by 256 in double is
        // exactly the native `DistanceFrom / 256.0`.
        let distance_cells = crate::sim::combat::object_distance_to(
            scorer.stable_id(),
            &TargetKind::Entity(candidate.stable_id()),
            world,
        )
        .map_or(0.0, |leptons| f64::from(leptons) / 256.0);
        let bonus = settings.in_range(rules) + distance_cells * settings.in_range_distance(rules);
        if bonus != 0.0 && close_enough_to_attack(scorer, scorer_type, candidate, context, rules) {
            total += bonus;
        }

        // 3. `ApplyFacingBonus` (`:472-488`): `deltaFacing` is 0 when the fire
        // facing points at the target and 32768 when it points away.
        if let Some(fire_facing) = fire_facing(scorer, scorer_type, world.session.binary_frame()) {
            let bonus = settings.facing(rules);
            if bonus != 0.0 {
                let target = crate::sim::movement::turret::facing_toward_target(
                    scorer,
                    &TargetKind::Entity(candidate.stable_id()),
                    world,
                )
                .unwrap_or(0);
                let delta_facing = 32768
                    - ((i32::from(target) - i32::from(fire_facing))
                        .abs()
                        .wrapping_sub(32768))
                    .abs();
                total += f64::from(delta_facing) * bonus;
            }
        }

        // 4. `ApplyLastTargetDistanceBonus` (`:490-503`): skip until the object
        // has held a target (`LastTargetCrd != CoordStruct::Empty`).
        let last_target = scorer.phobos_extra_threat_last_target_crd();
        let bonus = settings.distance_to_last_target(rules);
        if bonus != 0.0 && last_target != [0, 0, 0] {
            let coords = object_get_coords(candidate, terrain);
            // `CoordStruct::DistanceFrom` is YRpp's `Vector3D<int>`:
            // `sqrt((X*X + Y*Y) + Z*Z)` with each product and the sum in
            // double (`YRMathVector.h`). Phobos is compiled with
            // `/arch:SSE2` (`Phobos.props:45`), so its CRT `sqrt` is the IEEE
            // `sqrtsd` that Rust's `f64::sqrt` also emits.
            let dx = f64::from(coords.x.wrapping_sub(last_target[0]));
            let dy = f64::from(coords.y.wrapping_sub(last_target[1]));
            let dz = f64::from(coords.z.wrapping_sub(last_target[2]));
            let distance_cells = ((dx * dx + dy * dy) + dz * dz).sqrt() / 256.0;
            total += distance_cells * bonus;
        }

        reload(total)
    }

    impl Simulation {
        /// Phobos `TechnoExt::UpdateLastTargetCrd` (`Ext/Techno/Body.Update.cpp:
        /// 1286-1309`), reached from `TechnoExt::OnEarlyUpdate` (`:24`) after
        /// the EMP/rearm passes and before `CheckDeathConditions`.
        ///
        /// With a target the retained coordinate follows `Target->GetCoords()`
        /// (vt+0x48) and the clear timer stops. Without one the timer runs 45
        /// frames and then empties the coordinate. Types whose `ExtraThreat.*`
        /// values all resolve to `0.0` never run the body (`:1288-1289`).
        pub(crate) fn update_phobos_extra_threat_last_target(&mut self, id: u64, rules: &RuleSet) {
            let frame = self.session.binary_frame() as i32;
            let Some(entity) = self.substrate.entities.get(id) else {
                return;
            };
            let Some(object) = self.interner.object_type(entity.type_ref(), rules) else {
                return;
            };
            if !object.phobos_extra_threat.enabled(rules) {
                return;
            }
            let target = entity.attack_target.target();
            if let Some(target) = target {
                let Some(coords) = ({
                    let cells = self
                        .resolved_terrain
                        .as_ref()
                        .map(crate::map::resolved_terrain::NativeCellQuery::canonical);
                    target_get_coords(target, self, cells.as_ref())
                }) else {
                    return;
                };
                let Some(entity) = self.substrate.entities.get_mut(id) else {
                    return;
                };
                entity.set_phobos_extra_threat_last_target_crd([coords.x, coords.y, coords.z]);
                *entity.phobos_extra_threat_last_target_clear_timer_mut() = Default::default();
                return;
            }
            let Some(entity) = self.substrate.entities.get_mut(id) else {
                return;
            };
            // `if (!pTimer->IsTicking()) pTimer->Start(45);` then
            // `if (pTimer->Completed()) { LastTargetCrd = Empty; pTimer->Stop(); }`.
            let mut timer = entity.phobos_extra_threat_last_target_clear_timer();
            if !timer.in_progress() {
                timer.start(frame, 45);
            }
            if timer.expired(frame) {
                entity.set_phobos_extra_threat_last_target_crd([0; 3]);
                *entity.phobos_extra_threat_last_target_clear_timer_mut() = Default::default();
            } else {
                *entity.phobos_extra_threat_last_target_clear_timer_mut() = timer;
            }
        }
    }
}

pub(crate) use implementation::apply_extra_threat_bonus;
