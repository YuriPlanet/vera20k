//! The armor-index domain shared by the native ObjectType reader and combat.
//! Ares extends the same field; it does not add a second per-object armor.

use crate::util::native_x87::NativeF64Bits;

pub(crate) const RETAIL_ARMOR_NAMES: [&str; 11] = [
    "none",
    "flak",
    "plate",
    "light",
    "medium",
    "heavy",
    "wood",
    "steel",
    "concrete",
    "special_1",
    "special_2",
];

/// An index in the scenario's ordered armor registry. Construction and saved
/// index validation belong to the rules owner; consumers cannot invent IDs.
#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize,
)]
pub struct ArmorId(u32);

impl ArmorId {
    pub const NONE: Self = Self(0);
    pub const WOOD: Self = Self(6);

    pub fn index(self) -> usize {
        self.0 as usize
    }

    pub(crate) fn from_index(index: usize) -> Self {
        Self(u32::try_from(index).expect("armor registry exceeds the native index domain"))
    }
}

/// Native armor-name lookup772A50: ASCII case-insensitive, unknown names map
/// to None0. ReadString owns trimming/capacity before this lookup.
pub fn armor_index(name: &str) -> ArmorId {
    ArmorId::from_index(
        RETAIL_ARMOR_NAMES
            .iter()
            .position(|candidate| candidate.eq_ignore_ascii_case(name))
            .unwrap_or(0),
    )
}

/// A call-local immutable projection of one warhead/armor interaction.
/// The retained Warhead record remains the source of truth.
#[derive(Debug, Clone, Copy)]
pub struct ArmorInteraction {
    multiplier: NativeF64Bits,
    fire: bool,
    retaliate: bool,
    passive_acquire: bool,
}

impl ArmorInteraction {
    pub fn multiplier(self) -> NativeF64Bits {
        self.multiplier
    }

    pub fn can_fire(self) -> bool {
        self.fire
    }

    pub fn can_retaliate(self) -> bool {
        self.retaliate
    }

    pub fn can_passively_acquire(self) -> bool {
        self.passive_acquire
    }

    #[cfg(not(feature = "ares"))]
    pub(crate) fn retail(multiplier: NativeF64Bits) -> Self {
        let value = f64::from_bits(multiplier.bits());
        Self {
            multiplier,
            // GetFireError6FCB53's unordered comparison rejects NaN too.
            fire: value != 0.0 && !value.is_nan(),
            retaliate: value > f64::from(0.01_f32),
            // Preserve Evaluate_Candidate6F7D1F's original comparison,
            // including its unordered continuation before later gates.
            passive_acquire: !(value <= f64::from(0.02_f32)),
        }
    }

    #[cfg(feature = "ares")]
    pub(crate) fn ares(
        multiplier: NativeF64Bits,
        force_fire: bool,
        retaliate: bool,
        passive_acquire: bool,
    ) -> Self {
        Self {
            multiplier,
            // Installed Ares3.0 CanFire100542A0 admits unordered values;
            // force_fire=false only rejects an ordered zero multiplier.
            fire: force_fire || f64::from_bits(multiplier.bits()) != 0.0,
            retaliate,
            passive_acquire,
        }
    }
}
