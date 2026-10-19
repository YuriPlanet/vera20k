//! Authoritative body rocking: Techno70B280 impulses and70B570 updates.
//!
//! The four retained native binary32 words are the sole simulation state.
//! Keeping tiny nonzero rates matters: native damping can amplify them on the
//! next visit. Quantizing them to I16F16 first loses that gameplay branch.
//! Arithmetic uses the existing deterministic x87 numeric owners, not host
//! floating-point arithmetic. Native controls: unit_rocker_impulse.json and
//! unit_rocking_update.json. Drive/Ship slope state remains locomotor-owned.

pub mod impulse;
pub(crate) mod ion_blast;
pub mod rocking_system;
pub mod self_destruct;

#[cfg(test)]
#[path = "rocking_tests.rs"]
mod rocking_tests;

pub use impulse::{RockerImpulse, apply_rocker_impulse};
pub(crate) use rocking_system::update;

use crate::util::fixed_math::SimFixed;
use crate::util::native_x87::{NativeF32Bits, NativeF64Bits, X87Chop53 as X, X87Value};

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct RockingState {
    angle_sideways: NativeF32Bits,
    angle_forwards: NativeF32Bits,
    vel_sideways: NativeF32Bits,
    vel_forwards: NativeF32Bits,
}

impl Default for RockingState {
    fn default() -> Self {
        Self {
            angle_sideways: NativeF32Bits::POSITIVE_ZERO,
            angle_forwards: NativeF32Bits::POSITIVE_ZERO,
            vel_sideways: NativeF32Bits::POSITIVE_ZERO,
            vel_forwards: NativeF32Bits::POSITIVE_ZERO,
        }
    }
}

impl RockingState {
    /// Compatibility boundary only; simulation and pose consumers retain the
    /// native words. A value outside I16F16 saturates at this read boundary.
    pub fn angle_sideways(&self) -> SimFixed {
        fixed(self.angle_sideways)
    }
    pub fn angle_forwards(&self) -> SimFixed {
        fixed(self.angle_forwards)
    }
    #[cfg(test)]
    pub fn vel_sideways(&self) -> SimFixed {
        fixed(self.vel_sideways)
    }
    #[cfg(test)]
    pub fn vel_forwards(&self) -> SimFixed {
        fixed(self.vel_forwards)
    }

    pub fn angles_native(&self) -> [f32; 2] {
        [self.angle_sideways, self.angle_forwards].map(|word| f32::from_bits(word.bits()))
    }

    /// AngleSideways, AngleForwards, RateSideways, RateForwards, in native
    /// field order328/32C/330/334. Hash/save never quantize these words.
    pub fn native_bits(&self) -> [u32; 4] {
        [
            self.angle_sideways,
            self.angle_forwards,
            self.vel_sideways,
            self.vel_forwards,
        ]
        .map(NativeF32Bits::bits)
    }

    pub(crate) fn spin_for_crash(
        rocking: &mut Option<Self>,
        sideways: SimFixed,
        forwards: SimFixed,
    ) {
        let state = rocking.get_or_insert_with(Self::default);
        state.vel_sideways = fixed_word(sideways);
        state.vel_forwards = fixed_word(forwards);
    }

    /// Parasite629A46 writes sideways angle; ExitUnit62A869 clears it.
    /// Neither writer changes either angular rate.
    pub fn set_angle_sideways(&mut self, angle: SimFixed) {
        self.angle_sideways = fixed_word(angle);
    }

    /// Direct native rate writers replace only Techno+330. CaptureManager
    /// 471C78 uses a binary32 literal; quantizing through SimFixed would lose
    /// its value before the next ordinary RockingUpdate70B570 visit.
    pub(crate) fn set_vel_sideways_native(&mut self, velocity: NativeF32Bits) {
        self.vel_sideways = velocity;
    }

    /// Drive4B19F7 replaces Techno+334 with its binary32 pre-crush rate.
    pub(crate) fn set_vel_forwards_native(&mut self, velocity: NativeF32Bits) {
        self.vel_forwards = velocity;
    }

    /// Unit73B05B..73B06E, after Cell480CB0(-1) regardless of its return:
    /// FADD binary32 constant7F4E38 (+0.02), then FSTP Techno+334.
    /// Original numeric controls: unit_per_cell_overlay.json.
    pub(crate) fn recoil_after_overlay_crush(&mut self) {
        self.vel_forwards = store(X::add(
            load(self.vel_forwards),
            load(NativeF32Bits::from_bits(0x3ca3_d70a)),
        ));
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn set_angle_forwards_for_test(&mut self, angle: SimFixed) {
        self.angle_forwards = fixed_word(angle);
    }
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn set_vel_sideways_for_test(&mut self, value: SimFixed) {
        self.vel_sideways = fixed_word(value);
    }
    #[cfg(any(test, feature = "test-support"))]
    pub fn is_neutral(&self) -> bool {
        self.angles_native()
            .iter()
            .all(|angle| angle.abs() <= 0.00002)
    }
}

fn fixed(word: NativeF32Bits) -> SimFixed {
    SimFixed::saturating_from_num(f32::from_bits(word.bits()))
}
fn fixed_word(value: SimFixed) -> NativeF32Bits {
    NativeF32Bits::from_bits(value.to_num::<f32>().to_bits())
}
pub(super) fn load(word: NativeF32Bits) -> X87Value {
    X::load_f32(word).expect("finite rocking state")
}
pub(super) fn double(bits: u64) -> X87Value {
    X::load_f64(NativeF64Bits::from_bits(bits)).unwrap()
}
pub(super) fn store(value: X87Value) -> NativeF32Bits {
    X::store_f32_masked_chop(value)
}
pub(super) fn absolute(value: X87Value) -> X87Value {
    if X::compare(value, X::load_i32(0)) == crate::util::native_x87::X87Ordering::Less {
        X::neg(value)
    } else {
        value
    }
}
