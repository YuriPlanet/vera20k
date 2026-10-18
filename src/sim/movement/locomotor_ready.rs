//! Raw locomotor inputs consumed by Mission readiness.
//!
//! The active locomotor families implement the native readiness virtual with
//! different state and comparison rules.  This module keeps those inputs
//! separate and evaluates native double comparisons from their raw bits so
//! simulation logic never passes through floating-point arithmetic.

use serde::{Deserialize, Serialize};

const F64_SIGN: u64 = 1 << 63;
const F64_EXPONENT: u64 = 0x7ff0_0000_0000_0000;
const F64_FRACTION: u64 = 0x000f_ffff_ffff_ffff;
const F64_MAGNITUDE: u64 = !F64_SIGN;

#[inline]
fn native_double_is_nan(bits: u64) -> bool {
    bits & F64_EXPONENT == F64_EXPONENT && bits & F64_FRACTION != 0
}

#[inline]
fn native_double_ordered_not_zero(bits: u64) -> bool {
    bits & F64_MAGNITUDE != 0 && !native_double_is_nan(bits)
}

#[inline]
fn native_double_ordered_gt_zero(bits: u64) -> bool {
    let magnitude = bits & F64_MAGNITUDE;
    let is_negative = bits & F64_SIGN != 0;
    !is_negative && magnitude != 0 && !native_double_is_nan(bits)
}

/// Exact raw inputs read by active locomotor readiness implementations.
///
/// Drive and Ship intentionally remain distinct variants even though their
/// final predicates match: their native producers and virtual slots do not.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub(crate) enum LocomotorReadyState {
    Drive {
        turning_active: bool,
        slot_moving: bool,
        head_to_nonnull: bool,
        owner_speed: i32,
    },
    Ship {
        turning_active: bool,
        slot_moving: bool,
        head_to_nonnull: bool,
        owner_speed: i32,
    },
    Hover {
        slot_moving: bool,
        speed_bits: u64,
    },
    Walk {
        moving_byte: u8,
        applied_speed_bits: u64,
        destination_nonnull: bool,
    },
    Teleport {
        state: u8,
    },
    Jumpjet {
        state: i32,
    },
}

impl LocomotorReadyState {
    /// Return the result of the active locomotor's native "moving now" slot.
    pub(crate) fn is_moving_now(self) -> bool {
        match self {
            Self::Drive {
                turning_active,
                slot_moving,
                head_to_nonnull,
                owner_speed,
            }
            | Self::Ship {
                turning_active,
                slot_moving,
                head_to_nonnull,
                owner_speed,
            } => turning_active || (slot_moving && head_to_nonnull && owner_speed > 0),
            Self::Hover {
                slot_moving,
                speed_bits,
            } => slot_moving && native_double_ordered_not_zero(speed_bits),
            Self::Walk {
                moving_byte,
                applied_speed_bits,
                destination_nonnull,
            } => {
                moving_byte != 0
                    && native_double_ordered_gt_zero(applied_speed_bits)
                    && destination_nonnull
            }
            Self::Teleport { state } => state == 1,
            // `0x0054D0D0`: any state but ground (0) and hold (2), the
            // `jumpjet_flight` `STATE_GROUND` and `STATE_HOLD`.
            Self::Jumpjet { state } => state != 0 && state != 2,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};

    use super::*;

    const POSITIVE_ZERO: u64 = 0x0000_0000_0000_0000;
    const NEGATIVE_ZERO: u64 = 0x8000_0000_0000_0000;
    const POSITIVE_ONE: u64 = 0x3ff0_0000_0000_0000;
    const NEGATIVE_ONE: u64 = 0xbff0_0000_0000_0000;
    const POSITIVE_SUBNORMAL: u64 = 0x0000_0000_0000_0001;
    const POSITIVE_INFINITY: u64 = 0x7ff0_0000_0000_0000;
    const NEGATIVE_INFINITY: u64 = 0xfff0_0000_0000_0000;
    const SIGNALING_NAN: u64 = 0x7ff0_0000_0000_0001;
    const QUIET_NAN: u64 = 0x7ff8_0000_0000_0001;
    const NEGATIVE_QUIET_NAN: u64 = 0xfff8_0000_0000_0001;

    fn drive(
        turning_active: bool,
        slot_moving: bool,
        head_to_nonnull: bool,
        owner_speed: i32,
    ) -> LocomotorReadyState {
        LocomotorReadyState::Drive {
            turning_active,
            slot_moving,
            head_to_nonnull,
            owner_speed,
        }
    }

    fn ship(
        turning_active: bool,
        slot_moving: bool,
        head_to_nonnull: bool,
        owner_speed: i32,
    ) -> LocomotorReadyState {
        LocomotorReadyState::Ship {
            turning_active,
            slot_moving,
            head_to_nonnull,
            owner_speed,
        }
    }

    #[test]
    fn locomotor_ready_hover_uses_native_ordered_nonzero_comparison() {
        for speed_bits in [
            POSITIVE_ONE,
            NEGATIVE_ONE,
            POSITIVE_INFINITY,
            NEGATIVE_INFINITY,
            POSITIVE_SUBNORMAL,
        ] {
            assert!(
                LocomotorReadyState::Hover {
                    slot_moving: true,
                    speed_bits,
                }
                .is_moving_now()
            );
        }

        for speed_bits in [
            POSITIVE_ZERO,
            NEGATIVE_ZERO,
            SIGNALING_NAN,
            QUIET_NAN,
            NEGATIVE_QUIET_NAN,
        ] {
            assert!(
                !LocomotorReadyState::Hover {
                    slot_moving: true,
                    speed_bits,
                }
                .is_moving_now()
            );
        }
        assert!(
            !LocomotorReadyState::Hover {
                slot_moving: false,
                speed_bits: POSITIVE_ONE,
            }
            .is_moving_now()
        );
    }

    #[test]
    fn locomotor_ready_walk_uses_native_ordered_positive_bits() {
        for applied_speed_bits in [POSITIVE_ONE, POSITIVE_SUBNORMAL, POSITIVE_INFINITY] {
            assert!(
                LocomotorReadyState::Walk {
                    moving_byte: 1,
                    applied_speed_bits,
                    destination_nonnull: true,
                }
                .is_moving_now()
            );
        }

        for applied_speed_bits in [POSITIVE_ZERO, NEGATIVE_ZERO, NEGATIVE_ONE, QUIET_NAN] {
            assert!(
                !LocomotorReadyState::Walk {
                    moving_byte: 1,
                    applied_speed_bits,
                    destination_nonnull: true,
                }
                .is_moving_now()
            );
        }
        assert!(
            !LocomotorReadyState::Walk {
                moving_byte: 0,
                applied_speed_bits: POSITIVE_ONE,
                destination_nonnull: true,
            }
            .is_moving_now()
        );
        assert!(
            !LocomotorReadyState::Walk {
                moving_byte: 1,
                applied_speed_bits: POSITIVE_ONE,
                destination_nonnull: false,
            }
            .is_moving_now()
        );
    }

    #[test]
    fn locomotor_ready_teleport_and_jumpjet_state_tables() {
        for (state, expected) in [(0, false), (1, true), (2, false), (255, false)] {
            assert_eq!(
                LocomotorReadyState::Teleport { state }.is_moving_now(),
                expected
            );
        }
        for (state, expected) in [(-1, true), (0, false), (1, true), (2, false), (3, true)] {
            assert_eq!(
                LocomotorReadyState::Jumpjet { state }.is_moving_now(),
                expected
            );
        }
    }

    #[test]
    fn locomotor_ready_state_serde_and_hash_preserve_raw_variants() {
        let fixtures = [
            drive(true, false, true, i32::MIN),
            ship(false, true, true, i32::MAX),
            LocomotorReadyState::Hover {
                slot_moving: true,
                speed_bits: QUIET_NAN,
            },
            LocomotorReadyState::Walk {
                moving_byte: 255,
                applied_speed_bits: NEGATIVE_ZERO,
                destination_nonnull: true,
            },
            LocomotorReadyState::Teleport { state: 255 },
            LocomotorReadyState::Jumpjet { state: -1 },
        ];

        for fixture in fixtures {
            let bytes =
                crate::util::save_codec::serialize(&fixture).expect("serialize raw readiness");
            let restored: LocomotorReadyState =
                crate::util::save_codec::deserialize(&bytes).expect("deserialize raw readiness");
            assert_eq!(restored, fixture);

            let mut before = DefaultHasher::new();
            fixture.hash(&mut before);
            let mut after = DefaultHasher::new();
            restored.hash(&mut after);
            assert_eq!(before.finish(), after.finish());
        }
    }
}
