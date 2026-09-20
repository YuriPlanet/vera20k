//! Wide-amplitude self-destruct detection [L30].
//!
//! When a rocking entity's body angle exceeds ±π on either axis, the
//! reference engine invokes self-damage equal to the unit's max HP using
//! the `[CombatDamage] C4Warhead=` warhead with force-kill enabled. This
//! is the kill-on-tipover path.
//!
//! In retail play this almost never fires — the impulse/decay constants are
//! tuned to keep angles inside ±π/4. A faithful port still needs it so that:
//!   - sustained EMP on a type without the ship-rock clamp,
//!   - modded warheads with extreme impulses,
//!   - external angle writes,
//! all produce identical observable behavior to the reference engine.

use crate::sim::game_entity::GameEntity;
use crate::util::fixed_math::SimFixed;

/// Trigger threshold for the wide-amplitude self-destruct (radians).
/// Either axis exceeding this (in absolute value) fires the hook.
pub const WIDE_AMPLITUDE_THRESHOLD: SimFixed = SimFixed::lit("3.141593");

/// Callback invoked when a rocking entity's body angle exceeds ±π.
///
/// Implementations should apply `damage = live ObjectType.strength` with the
/// ruleset's C4Warhead, `source_house = None`, `source_object = None`,
/// `force_kill = true`. Until combat-side damage lands, use
/// `NoopSelfDestruct`; swap in a real implementation in Phase F (Task 19).
pub trait SelfDestructHook {
    fn fire(&mut self, entity: &mut GameEntity);
}

/// No-op hook for tests and for the period before combat-side damage lands.
pub struct NoopSelfDestruct;

impl SelfDestructHook for NoopSelfDestruct {
    fn fire(&mut self, _entity: &mut GameEntity) {}
}

/// Inspect an entity's rocking state; if either body angle exceeds ±π,
/// invoke `hook`. The hook is responsible for the damage application; this
/// function does not modify rocking state itself.
pub fn check_and_fire(entity: &mut GameEntity, hook: &mut dyn SelfDestructHook) {
    let Some(rocking) = entity.rocking.as_ref() else {
        return;
    };
    if rocking.angle_sideways.abs() > WIDE_AMPLITUDE_THRESHOLD
        || rocking.angle_forwards.abs() > WIDE_AMPLITUDE_THRESHOLD
    {
        hook.fire(entity);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::map::entities::EntityCategory;
    use crate::sim::components::{Health, RockingState};
    use crate::sim::intern::test_intern;

    struct CountingHook {
        fired: usize,
    }
    impl SelfDestructHook for CountingHook {
        fn fire(&mut self, _entity: &mut GameEntity) {
            self.fired += 1;
        }
    }

    fn entity_with_rocking() -> GameEntity {
        let mut e = GameEntity::new_at_frame_zero_for_test(
            1,
            10,
            10,
            0,
            0,
            test_intern("Americans"),
            Health { current: 400 },
            test_intern("HTNK"),
            EntityCategory::Unit,
            0,
            5,
            true,
        );
        e.rocking = Some(RockingState::default());
        e
    }

    #[test]
    fn fires_when_sideways_exceeds_pi() {
        let mut e = entity_with_rocking();
        e.rocking.as_mut().unwrap().angle_sideways =
            WIDE_AMPLITUDE_THRESHOLD + SimFixed::lit("0.01");
        let mut hook = CountingHook { fired: 0 };
        check_and_fire(&mut e, &mut hook);
        assert_eq!(hook.fired, 1);
    }

    #[test]
    fn fires_when_forwards_exceeds_pi_negative() {
        let mut e = entity_with_rocking();
        e.rocking.as_mut().unwrap().angle_forwards =
            -WIDE_AMPLITUDE_THRESHOLD - SimFixed::lit("0.01");
        let mut hook = CountingHook { fired: 0 };
        check_and_fire(&mut e, &mut hook);
        assert_eq!(hook.fired, 1);
    }

    #[test]
    fn does_not_fire_within_envelope() {
        let mut e = entity_with_rocking();
        // Even at the saturation cap (±π/4), should not fire.
        e.rocking.as_mut().unwrap().angle_sideways = SimFixed::lit("0.78");
        e.rocking.as_mut().unwrap().angle_forwards = SimFixed::lit("-0.78");
        let mut hook = CountingHook { fired: 0 };
        check_and_fire(&mut e, &mut hook);
        assert_eq!(hook.fired, 0);
    }

    #[test]
    fn skipped_for_entities_without_rocking() {
        let mut e = entity_with_rocking();
        e.rocking = None;
        let mut hook = CountingHook { fired: 0 };
        check_and_fire(&mut e, &mut hook);
        assert_eq!(hook.fired, 0);
    }

    #[test]
    fn does_not_fire_at_exactly_pi() {
        // Strict `>` comparison: angle exactly equal to π should NOT fire.
        let mut e = entity_with_rocking();
        e.rocking.as_mut().unwrap().angle_sideways = WIDE_AMPLITUDE_THRESHOLD;
        let mut hook = CountingHook { fired: 0 };
        check_and_fire(&mut e, &mut hook);
        assert_eq!(hook.fired, 0);
    }
}
