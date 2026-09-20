//! Persistent MapClass crate-slot authority and native timer words.
//!
//! Active `gamemd.exe` owns 256 consecutive 16-byte slots. The coordinate
//! pair is the sole emptiness discriminator; accepted ghosts retain the same
//! coordinate/timer state as visible crates.

use crate::util::native_x87::{NativeF64Bits, X87Chop53};

pub(crate) const CRATE_SLOT_CAPACITY: usize = 256;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct CrateSlot {
    pub start_frame: i32,
    pub aux: u32,
    pub duration: i32,
    pub cell_x: i16,
    pub cell_y: i16,
}

impl Default for CrateSlot {
    fn default() -> Self {
        Self {
            start_frame: -1,
            aux: 0,
            duration: 0,
            cell_x: 0,
            cell_y: 0,
        }
    }
}

impl CrateSlot {
    pub fn is_empty(self) -> bool {
        self.cell_x == 0 && self.cell_y == 0
    }
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct CrateAuthority {
    #[serde(with = "crate_slot_array_serde")]
    slots: [CrateSlot; CRATE_SLOT_CAPACITY],
}

impl Default for CrateAuthority {
    fn default() -> Self {
        Self {
            slots: [CrateSlot::default(); CRATE_SLOT_CAPACITY],
        }
    }
}

impl CrateAuthority {
    pub(crate) fn first_empty_index(&self) -> Option<usize> {
        self.slots.iter().position(|slot| slot.is_empty())
    }

    pub(crate) fn slots(&self) -> &[CrateSlot; CRATE_SLOT_CAPACITY] {
        &self.slots
    }

    pub(crate) fn slot_mut(&mut self, index: usize) -> &mut CrateSlot {
        &mut self.slots[index]
    }

    /// Accepted coordinates in native ascending slot order. Negative packed
    /// values are malformed snapshot state and cannot name a Rust grid cell.
    pub(crate) fn occupied_cells(&self) -> impl Iterator<Item = (u16, u16)> + '_ {
        self.slots.iter().filter_map(|slot| {
            if slot.is_empty() {
                return None;
            }
            Some((
                u16::try_from(slot.cell_x).ok()?,
                u16::try_from(slot.cell_y).ok()?,
            ))
        })
    }
}

/// Compute the accepted-slot timer words in the verified x87 expression
/// direction from `CrateSlot__PlaceOverlayAndInitTimer @ 0x004A17C0`.
pub(crate) fn crate_timer_words(
    regen: NativeF64Bits,
    draw: u32,
    current_frame: i32,
) -> (i32, u32, i32) {
    debug_assert!(draw <= 0x7fff_fffe);
    let regen = X87Chop53::load_f64(regen).expect("validated CrateRegen");
    let lower = X87Chop53::mul(regen, X87Chop53::load_i32(450));
    let upper = X87Chop53::mul(regen, X87Chop53::load_i32(1800));
    let fraction = X87Chop53::div(
        X87Chop53::load_i32(draw as i32),
        X87Chop53::load_i32(0x7fff_fffe),
    )
    .expect("nonzero crate timer divisor");
    let value = X87Chop53::add(
        lower,
        X87Chop53::mul(fraction, X87Chop53::sub(upper, lower)),
    );
    let stored_upper = X87Chop53::store_f64(upper).expect("finite CrateRegen upper");
    // `Math__ftol @ 0x007C5F00` executes masked `FISTP qword`; an
    // out-of-range conversion stores integer-indefinite i64::MIN. The crate
    // writer at 0x004A18C5 keeps only EAX, so either-sign overflow becomes 0.
    let duration = X87Chop53::ftol_i32_low_masked(value);
    (current_frame, (stored_upper.bits() >> 32) as u32, duration)
}

mod crate_slot_array_serde {
    use super::{CRATE_SLOT_CAPACITY, CrateSlot};
    use serde::de::{self, SeqAccess, Visitor};
    use serde::ser::SerializeTuple;
    use serde::{Deserializer, Serializer};
    use std::fmt;

    pub fn serialize<S>(
        slots: &[CrateSlot; CRATE_SLOT_CAPACITY],
        serializer: S,
    ) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut tuple = serializer.serialize_tuple(CRATE_SLOT_CAPACITY)?;
        for slot in slots {
            tuple.serialize_element(slot)?;
        }
        tuple.end()
    }

    pub fn deserialize<'de, D>(
        deserializer: D,
    ) -> Result<[CrateSlot; CRATE_SLOT_CAPACITY], D::Error>
    where
        D: Deserializer<'de>,
    {
        struct SlotsVisitor;

        impl<'de> Visitor<'de> for SlotsVisitor {
            type Value = [CrateSlot; CRATE_SLOT_CAPACITY];

            fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                write!(formatter, "exactly {CRATE_SLOT_CAPACITY} crate slots")
            }

            fn visit_seq<A>(self, mut seq: A) -> Result<Self::Value, A::Error>
            where
                A: SeqAccess<'de>,
            {
                let mut slots = [CrateSlot::default(); CRATE_SLOT_CAPACITY];
                for (index, slot) in slots.iter_mut().enumerate() {
                    *slot = seq.next_element()?.ok_or_else(|| {
                        de::Error::invalid_length(index, &"exactly 256 crate slots")
                    })?;
                }
                Ok(slots)
            }
        }

        deserializer.deserialize_tuple(CRATE_SLOT_CAPACITY, SlotsVisitor)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crate_authority_fresh_slots_and_first_empty_are_exact() {
        let mut authority = CrateAuthority::default();
        assert!(authority.slots().iter().all(|slot| {
            *slot
                == CrateSlot {
                    start_frame: -1,
                    aux: 0,
                    duration: 0,
                    cell_x: 0,
                    cell_y: 0,
                }
        }));
        assert_eq!(authority.first_empty_index(), Some(0));

        authority.slot_mut(0).cell_x = 7;
        assert_eq!(authority.first_empty_index(), Some(1));
        authority.slot_mut(0).cell_x = 0;
        authority.slot_mut(0).cell_y = -2;
        assert_eq!(authority.first_empty_index(), Some(1));
        authority.slot_mut(0).cell_y = 0;
        authority.slot_mut(0).duration = 99;
        assert_eq!(
            authority.first_empty_index(),
            Some(0),
            "timer words do not make a zero-coordinate slot occupied"
        );

        for (index, slot) in authority.slots.iter_mut().enumerate() {
            slot.cell_x = i16::try_from(index + 1).expect("slot index fits i16");
        }
        assert_eq!(authority.first_empty_index(), None);
        assert_eq!(
            authority.occupied_cells().take(3).collect::<Vec<_>>(),
            vec![(1, 0), (2, 0), (3, 0)]
        );
    }

    #[test]
    fn crate_timer_native_endpoints_and_interior_vector() {
        let regen = NativeF64Bits::from_bits(3.0_f64.to_bits());
        assert_eq!(crate_timer_words(regen, 0, -1), (-1, 0x40b5_1800, 1350));
        assert_eq!(
            crate_timer_words(regen, 0x7fff_fffe, i32::MIN),
            (i32::MIN, 0x40b5_1800, 5400)
        );
        assert_eq!(
            crate_timer_words(regen, 0x3fff_ffff, 77),
            (77, 0x40b5_1800, 3375)
        );
    }

    #[test]
    fn crate_timer_out_of_range_fistp_stores_integer_indefinite_low_dword() {
        for regen in [1.0e20_f64, -1.0e20_f64] {
            let (_, _, duration) =
                crate_timer_words(NativeF64Bits::from_bits(regen.to_bits()), 0x3fff_ffff, 91);
            assert_eq!(
                duration, 0,
                "masked FISTP qword writes i64::MIN and the native slot keeps low EAX"
            );
        }
    }

    #[test]
    fn crate_authority_tuple_codec_rejects_truncated_storage() {
        let authority = CrateAuthority::default();
        let mut value = serde_json::to_value(&authority).expect("authority serializes");
        let slots = value
            .get_mut("slots")
            .and_then(serde_json::Value::as_array_mut)
            .expect("slots serialize as a tuple sequence");
        assert_eq!(slots.len(), CRATE_SLOT_CAPACITY);
        slots.pop();
        let error = serde_json::from_value::<CrateAuthority>(value)
            .expect_err("255 slots must not deserialize");
        assert!(error.to_string().contains("256 crate slots"));
    }

    #[test]
    fn crate_authority_every_raw_word_changes_v114_hash_only() {
        use crate::sim::world::Simulation;

        let baseline = Simulation::new();
        let baseline_current = baseline.state_hash();
        let baseline_v113 = baseline.state_hash_without_crate_authority_v114();
        for slot in [
            CrateSlot {
                start_frame: 8,
                ..CrateSlot::default()
            },
            CrateSlot {
                aux: 9,
                ..CrateSlot::default()
            },
            CrateSlot {
                duration: -10,
                ..CrateSlot::default()
            },
            CrateSlot {
                cell_x: -11,
                ..CrateSlot::default()
            },
            CrateSlot {
                cell_y: 12,
                ..CrateSlot::default()
            },
        ] {
            let mut changed = Simulation::new();
            *changed.crate_authority.slot_mut(73) = slot;
            assert_ne!(baseline_current, changed.state_hash());
            assert_eq!(
                baseline_v113,
                changed.state_hash_without_crate_authority_v114()
            );
        }
    }

    #[test]
    fn crate_authority_is_excluded_from_retail_multiplayer_checksum() {
        use crate::sim::world::Simulation;

        let mut baseline = Simulation::with_seed(0x1414);
        let mut changed = Simulation::with_seed(0x1414);
        *changed.crate_authority.slot_mut(4) = CrateSlot {
            start_frame: i32::MAX,
            aux: u32::MAX,
            duration: i32::MIN,
            cell_x: -7,
            cell_y: 31,
        };
        let empty_families = [&[][..], &[][..], &[][..], &[][..], &[][..]];
        assert_eq!(
            baseline
                .compute_retail_multiplayer_checksum(empty_families)
                .expect("baseline checksum"),
            changed
                .compute_retail_multiplayer_checksum(empty_families)
                .expect("changed checksum")
        );
    }
}
