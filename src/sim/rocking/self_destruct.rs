//! RockingUpdate70BC23..70BC6D requests the real C4 receiver only on
//! ordinary updates beyond either strict binary32 pi boundary. The caller
//! applies it after releasing its entity borrow; no callback or no-op owner.

use super::{RockingState, load};
use crate::util::native_x87::{NativeF32Bits, X87Chop53 as X, X87Ordering};

pub(crate) fn needs_damage(state: &RockingState) -> bool {
    [state.angle_sideways, state.angle_forwards]
        .into_iter()
        .any(|angle| {
            X::compare(load(angle), load(NativeF32Bits::from_bits(0x4049_0fdb)))
                == X87Ordering::Greater
                || X::compare(load(angle), load(NativeF32Bits::from_bits(0xc049_0fdb)))
                    == X87Ordering::Less
        })
}
