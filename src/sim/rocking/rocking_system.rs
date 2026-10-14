//! Per-tick body-rocking spring-damper advance.
//!
//! All angles and angular velocities are in radians, stored as `SimFixed`
//! (I16F16, ~1.5e-5 precision). Constants here are extracted from the
//! reference engine; do not change without binary verification.

use crate::map::entities::EntityCategory;
use crate::rules::ruleset::RuleSet;
use crate::sim::components::RockingState;
use crate::sim::entity_store::EntityStore;
use crate::sim::game_entity::GameEntity;
use crate::sim::rocking::self_destruct::SelfDestructHook;
use crate::util::fixed_math::SimFixed;

/// Tilt-renderer deadband. Both angles below this snap to zero and the unit
/// renders via the static atlas path.
pub const TILT_DEADBAND: SimFixed = SimFixed::lit("0.00002");

/// Saturation cap for body roll/pitch (±π/4 ≈ 0.7854 rad).
pub const SATURATION_PI4: SimFixed = SimFixed::lit("0.7853982");

/// Tighter forwards-only saturation cap (±π/10 ≈ 0.3142 rad) used when a
/// Crusher vehicle is mid-crush of a building. The `TechnoClass+0x6B5`-style
/// gate that selects this cap is DEFERRED until building-crushing lands; the
/// constant is defined so re-enabling that path later is a one-line change.
pub const SATURATION_PI10: SimFixed = SimFixed::lit("0.3141593");

/// "Out of normal range" threshold (±π/2). Above this, dampening pushes back
/// inward at the base rate whatever the DirectRocker link.
pub const NORMAL_RANGE_PI2: SimFixed = SimFixed::lit("1.5707963");

/// Base damping rate (rad/tick). An unlinked rocker uses this directly; a
/// DirectRocker-linked one scales it by `fallback_coefficient` (default 0.1
/// → 0.0002 rad/tick).
pub const BASE_DECAY_RATE: SimFixed = SimFixed::lit("0.002");

/// Snap-back rate for the velocity-fighting-itself sub-branch (rad/tick).
/// Used only in the out-of-normal-range path.
pub const SNAP_BACK_RATE: SimFixed = SimFixed::lit("0.005");

/// Per-axis velocity cap applied at impulse-receive time (rad/tick).
pub const IMPULSE_VEL_CAP: SimFixed = SimFixed::lit("0.05");

/// Saturation cap on rocker impulse force from area-damage (clamped to 4.0
/// before the per-axis velocity gate). Bounds defended both at the source
/// and inside `apply_rocker_impulse` against any wiring error.
pub const FORCE_SATURATION: SimFixed = SimFixed::lit("4");

/// Minimum force for the Apply_area_damage 3×3 cell impulse loop to fire at
/// all (after `FORCE_SATURATION` clamp). Below this floor, no impulses are
/// applied to any target in the radius.
pub const APPLY_AREA_FORCE_FLOOR: SimFixed = SimFixed::lit("0.3");

/// Advance one rocking axis (sideways OR forwards) by one tick, after
/// `TechnoClass::RockingUpdate @ 0x0070B570` (sideways `0x0070B700..0x0070B95D`,
/// forwards `0x0070B963..0x0070BC1D`):
///   1. Zero-velocity short-circuit — if velocity == 0, force angle to 0.
///   2. Integrate angle += velocity.
///   3. Saturation clamp — only when not DirectRocker-linked AND in normal
///      range AND the integration step crossed the cap this tick. A linked
///      rocker drifts past the cap; out-of-range angles fall through to step 4.
///   4. Dampening — a linked rocker decays at `fallback * BASE_DECAY_RATE`,
///      an unlinked one at `BASE_DECAY_RATE`. Out-of-range velocity is
///      pushed in the same direction as the velocity sign (which is what
///      produces the wide-amplitude run-away that L30 self-destruct catches).
///   5. Deadband snap — `|angle| <= TILT_DEADBAND` clears both fields.
///
/// `direct_rocker_linked` is the DirectRocker link (`Techno+0x2A8`), which
/// skips the clamp (sideways `0x0070B76C`, `0x0070B7A4`; forwards `0x0070BA30`,
/// `0x0070BA66`) and scales the in-range damper by FallBackCoefficient
/// (`Rules+0x18B8`, `0x0070B7FA`). The body never asks the locomotor whether
/// the object moves.
///
/// DRIFT, unported while the ordinary rocker has no producer (`sim::rocking`):
/// - native picks the damper's direction by whether the angle before the step
///   is above the deadband (`0x0070B7DD`), not by the velocity's sign;
/// - in range and unlinked, native changes the sideways velocity by 0.005
///   when it is zero or already has the damper's sign (`0x0070B868`,
///   `0x0070B90D`) and the forwards velocity by 0.005 always (`0x0070BB3D`,
///   `0x0070BBCD`); VERA changes a non-zero velocity by 0.002 and leaves the
///   zero velocity a clamp writes unchanged;
/// - the range flag tests the angle before the step (`0x0070B700..0x0070B72A`);
/// - the deadband compares the angles before and after the step: it clears
///   an angle that came down through +2e-5 or up through -2e-5, past zero
///   too, and not one that stays inside (`0x0070B927..0x0070B95D`).
///
/// `cap` selects ±π/4 (default) or ±π/10 (forwards during vehicle-vs-building
/// crush, currently DEFERRED).
pub(crate) fn advance_axis(
    angle: &mut SimFixed,
    velocity: &mut SimFixed,
    cap: SimFixed,
    direct_rocker_linked: bool,
    fallback: SimFixed,
) {
    // L10: strict velocity == 0 → angle force-zero, skip integration.
    if *velocity == SimFixed::ZERO {
        *angle = SimFixed::ZERO;
        return;
    }

    // L2: integrate.
    let prev = *angle;
    let new_angle = prev + *velocity;
    *angle = new_angle;

    let in_range = angle.abs() <= NORMAL_RANGE_PI2;

    // L7: saturation fires only when unlinked, in normal range, and crossing
    // the cap this tick. Both clamp the angle and zero the velocity.
    if !direct_rocker_linked && in_range {
        if new_angle > cap && prev < cap {
            *angle = cap;
            *velocity = SimFixed::ZERO;
        } else if new_angle < -cap && prev > -cap {
            *angle = -cap;
            *velocity = SimFixed::ZERO;
        }
    }

    // L3 / L4 / L5: dampening.
    //   - in_range, linked:   velocity decays by fallback * BASE_DECAY_RATE
    //   - in_range, unlinked: velocity decays by BASE_DECAY_RATE
    //   - out_of_range:       velocity is pushed in its own direction by
    //                         BASE_DECAY_RATE (subtract -BASE_DECAY_RATE = +)
    //                         — the runaway path the L30 self-destruct catches.
    let decay = if direct_rocker_linked {
        fallback * BASE_DECAY_RATE
    } else {
        BASE_DECAY_RATE
    };
    if *velocity > SimFixed::ZERO {
        *velocity -= if in_range { decay } else { -BASE_DECAY_RATE };
    } else if *velocity < SimFixed::ZERO {
        *velocity += if in_range { decay } else { -BASE_DECAY_RATE };
    }

    // L9: deadband snap clears both angle and velocity in the same tick.
    if angle.abs() <= TILT_DEADBAND {
        *angle = SimFixed::ZERO;
        *velocity = SimFixed::ZERO;
    }
}

/// Advance the ship-rocking path: integrate without damping, asymmetric
/// one-sided clamp. Used for EMP wobble and naval continuous rocking.
///
/// `type_supports_ship_rocking` is the per-type allow flag — when false,
/// integration still runs but no clamping is applied (matches the
/// non-clamping branch in the reference engine for types that "shouldn't"
/// ship-rock but had the flag set externally).
pub(crate) fn advance_ship_rocking(rocking: &mut RockingState, type_supports_ship_rocking: bool) {
    rocking.angle_forwards += rocking.vel_forwards;
    rocking.angle_sideways += rocking.vel_sideways;
    if !type_supports_ship_rocking {
        return;
    }
    // Lower clamps to -π/4 on both axes.
    if rocking.angle_forwards < -SATURATION_PI4 {
        rocking.angle_forwards = -SATURATION_PI4;
    }
    if rocking.angle_sideways < -SATURATION_PI4 {
        rocking.angle_sideways = -SATURATION_PI4;
    }
    // Asymmetric upper clamp — sideways only, forwards is allowed to drift
    // positive without clamping (matches the reference engine).
    if rocking.angle_sideways >= SATURATION_PI4 {
        rocking.angle_sideways = SATURATION_PI4;
    }
}

/// Advance every entity's `RockingState` by one sim tick.
///
/// Order per entity:
///   1. If `is_ship_rocking`: integrate without damping (`advance_ship_rocking`).
///   2. Else: spring-damper on each axis (`advance_axis`). The forwards-axis
///      ±π/10 override during vehicle-vs-building crush [L8] is DEFERRED until
///      building-crushing lands; both axes use ±π/4 uniformly.
///   3. Wide-amplitude self-destruct check [L30]: if either body angle now
///      exceeds ±π, fire `hook`. Mirrors the end-of-RockingUpdate damage call
///      in the reference engine.
pub fn tick(
    entities: &mut EntityStore,
    rules: &RuleSet,
    interner: &crate::sim::intern::StringInterner,
    self_destruct_hook: &mut dyn SelfDestructHook,
) {
    let keys = entities.keys_sorted();
    for &id in &keys {
        let Some(entity) = entities.get_mut_if(id, |entity| entity.rocking.is_some()) else {
            continue;
        };

        // Read whole-entity properties before borrowing rocking mutably.
        // RESIDUAL: VERA keeps no DirectRocker link (`Techno+0x2A8`). Only the
        // DirectRocker detonation arm sets it (`0x004699AD..0x004699B9`), whose
        // effect `sim::projectile` does not perform, and no warhead in retail
        // `rulesmd.ini` sets `DirectRocker=`.
        let direct_rocker_linked = false;
        let supports_ship_rock = entity_type_supports_ship_rocking(entity);
        let fallback = rules.general.fallback_coefficient;

        // 1–2: mutate the rocking state. The &mut borrow is scoped to this
        // block so step 3 can take a fresh &mut GameEntity for the hook.
        {
            let crashing = entity.crash.is_crashing();
            let balloon_hover = balloon_hover_type(entity, rules, interner);
            let rocking = entity.rocking.as_mut().unwrap();
            if rocking.is_ship_rocking {
                advance_ship_rocking(rocking, supports_ship_rock);
            } else if crashing {
                // A crashing object spins by the rates Crash drew and returns:
                // no damping and no wide-amplitude self-destruct.
                advance_crash_spin(rocking, balloon_hover);
                continue;
            } else {
                // L8 forwards override (±π/10 during building-crush) is DEFERRED
                // — the gate it depends on isn't wired yet. Both axes use ±π/4.
                let forward_cap = SATURATION_PI4;
                advance_axis(
                    &mut rocking.angle_sideways,
                    &mut rocking.vel_sideways,
                    SATURATION_PI4,
                    direct_rocker_linked,
                    fallback,
                );
                advance_axis(
                    &mut rocking.angle_forwards,
                    &mut rocking.vel_forwards,
                    forward_cap,
                    direct_rocker_linked,
                    fallback,
                );
            }
        }

        // 3: wide-amplitude self-destruct check [L30].
        crate::sim::rocking::self_destruct::check_and_fire(entity, self_destruct_hook);
    }
}

/// `TechnoClass::RockingUpdate @ 0x0070B570`'s crashing branch
/// (`0x0070B63D..0x0070B6FF`, IsCrashing `+0x425`): both angles add the rates
/// `FootClass::Crash` drew (`+0x32C += +0x334`, then `+0x328 += +0x330`), and
/// only a `BalloonHover=` type (`+0xD6A`) clamps them: forwards and sideways
/// from below at -π/4, sideways from above at +π/4. The branch returns before
/// the damper and the wide-amplitude C4 kill.
///
/// Native keeps f32 angles; VERA SimFixed. The angles only orient the crashing
/// voxel (`FlyLocomotionClass` Draw_Matrix `0x004CF610`), so the sub-quantum
/// difference is not visible. Evidence: `tools/spatial_oracle/aircraft_crash`
/// `rocking` rows.
pub(crate) fn advance_crash_spin(rocking: &mut RockingState, balloon_hover: bool) {
    rocking.angle_forwards += rocking.vel_forwards;
    rocking.angle_sideways += rocking.vel_sideways;
    if balloon_hover {
        rocking.angle_forwards = rocking.angle_forwards.max(-SATURATION_PI4);
        rocking.angle_sideways = rocking
            .angle_sideways
            .max(-SATURATION_PI4)
            .min(SATURATION_PI4);
    }
}

fn balloon_hover_type(
    entity: &GameEntity,
    rules: &RuleSet,
    interner: &crate::sim::intern::StringInterner,
) -> bool {
    entity.crash.is_crashing()
        && rules
            .object(interner.resolve(entity.type_ref()))
            .is_some_and(|object| object.balloon_hover)
}

/// Working assumption per design doc §"Open Questions" #1: vehicles ship-rock,
/// infantry/aircraft/structures don't. Revisit when RE confirms.
fn entity_type_supports_ship_rocking(entity: &GameEntity) -> bool {
    matches!(entity.category(), EntityCategory::Unit)
}
