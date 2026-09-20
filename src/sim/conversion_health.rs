//! Conversion health ownership: native5F5C60, Unit73992B..953 and Building
//! 449E64..74/44A010..39. Original-byte comparisons: spatial_oracle/conversion_health.
//! Actual health and Object+70 reset together; source reservations are not copied.

use crate::rules::object_type::ObjectType;
use crate::sim::game_entity::GameEntity;
use crate::util::native_x87::X87Chop53;

#[derive(Debug, Clone, Copy)]
pub(crate) enum ConversionKind {
    Unit,
    Building,
}

/// Original signed-dword result, including _ftol's low-EAX consumption.
/// The native health ratio reads source ObjectType.Strength, not Health.max.
pub(crate) fn native_health(
    current: i32,
    source_strength: i32,
    destination_strength: i32,
    kind: ConversionKind,
) -> i32 {
    // 5F5C60 has no zero-divisor branch. Masked x87 produces infinity/NaN;
    // _ftol returns integer indefinite (low EAX0), then the caller clamps1.
    // Both zero/nonzero numerator paths execute in the original-byte corpus.
    if source_strength == 0 {
        return 1;
    }
    let ratio = X87Chop53::div(
        X87Chop53::load_i32(current),
        X87Chop53::load_i32(source_strength),
    )
    .expect("nonzero signed Strength produces a finite ratio");
    let ratio = match kind {
        ConversionKind::Unit => ratio,
        ConversionKind::Building => X87Chop53::load_f64(
            X87Chop53::store_f64(ratio).expect("signed-dword ratio fits binary64"),
        )
        .expect("stored finite ratio"),
    };
    let product = X87Chop53::mul(ratio, X87Chop53::load_i32(destination_strength));
    let integer = X87Chop53::ftol_i64(product)
        .expect("product of signed-dword ratio and Strength fits signed64");
    (integer as i32).max(1)
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct ConversionHealth(i32);

impl ConversionHealth {
    pub(crate) fn capture(
        source: &GameEntity,
        source_type: &ObjectType,
        destination_type: &ObjectType,
        kind: ConversionKind,
    ) -> Self {
        Self(native_health(
            source.health.current,
            source_type.strength,
            destination_type.strength,
            kind,
        ))
    }

    pub(crate) fn apply(self, destination: &mut GameEntity) {
        destination.health.current = self.0;
        destination.estimated_health.reset(self.0);
    }
}

#[cfg(test)]
#[path = "conversion_health_tests.rs"]
mod tests;
