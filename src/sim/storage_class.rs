//! The four-float `StorageClass` that House (`+0x2FC`, `+0x314`), Building
//! (`+0x33C`) and Unit (`+0x33C`) each embed.
//!
//! `StorageClass::GetTotalAmount @ 0x006C9650` and `Add_Amount @ 0x006C9690`
//! each have exactly one Rust port, here. The building refinery/silo art
//! (`building_art_storage.rs`) and the house weed store (`economy.rs`) are
//! callers, not second owners.
//!
//! Never depends on render/ui/audio/net (sim invariant #1).

use crate::util::native_x87::{MaskedX87Chop53 as X87, NativeF32Bits};

/// Four retained float slots in native Tiberium slot order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub(crate) struct StorageAmounts {
    amounts: [NativeF32Bits; 4],
}

impl Default for StorageAmounts {
    fn default() -> Self {
        Self::EMPTY
    }
}

impl StorageAmounts {
    /// `HouseClass::Constructor @ 0x004F57F6` and the ordinary Techno
    /// constructor `0x006C95E0` clear all four slots to +0.0.
    pub(crate) const EMPTY: Self = Self {
        amounts: [NativeF32Bits::POSITIVE_ZERO; 4],
    };

    /// A captured set of slots, e.g. a native-save read.
    pub(crate) const fn from_amounts(amounts: [NativeF32Bits; 4]) -> Self {
        Self { amounts }
    }

    pub(crate) const fn amounts(self) -> [NativeF32Bits; 4] {
        self.amounts
    }

    /// `StorageClass::GetTotalAmount @ 0x006C9650`: `fild` the signed
    /// accumulator, `fadd` one float slot, `_ftol @ 0x007C5F00`, store the
    /// low dword back into the accumulator — repeated for all four slots —
    /// and return `fild` of that final integer in ST0. Every addition
    /// truncates, not just the last one.
    pub(crate) fn total(self) -> i32 {
        self.amounts.into_iter().fold(0, |total, amount| {
            X87::ftol_i32_low_masked(X87::add(X87::load_i32(total), X87::load_f32(amount)))
        })
    }

    /// `StorageClass::Add_Amount @ 0x006C9690`: `fld` the supplied float,
    /// `fadd` the slot, `fst` the float sum back into the slot. The sum also
    /// stays in ST0; `HouseClass::GiveWeed @ 0x004F9700` pops it with
    /// `fstp st(0)` and never reads it.
    ///
    /// RESIDUAL: a slot index outside 0..3 cannot come from native
    /// `FindFirstUsedSlot @ 0x006C9820`, and VERA's four-slot store has no
    /// native out-of-bounds body to copy. Such a call is dropped rather than
    /// writing an invented fifth slot.
    pub(crate) fn add_amount(&mut self, slot: usize, amount: NativeF32Bits) {
        let Some(slot) = self.amounts.get_mut(slot) else {
            return;
        };
        *slot = X87::store_f32_masked_chop(X87::add(X87::load_f32(amount), X87::load_f32(*slot)));
    }

    /// A captured set of slots for a test.
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn set_amounts_for_test(&mut self, amounts: [NativeF32Bits; 4]) {
        self.amounts = amounts;
    }
}
