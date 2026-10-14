//! The hasher behind `Simulation::state_hash`, and the helpers that keep
//! hashing a 20,000-unit world cheap.

use std::hash::{Hash, Hasher};

/// A pinned, portable 64-bit hasher for the simulation state hash.
///
/// The hash names a simulation state for replay checks and A/B comparisons,
/// so it must not depend on the host: an integer written alone mixes as its
/// value whatever the word size, std passes integer slices as native-endian
/// bytes (little-endian on every supported target), and the algorithm is
/// fixed here, where std's `DefaultHasher` promises none. A write costs one
/// folded multiply, and long byte runs (the per-House fog grids) mix in four
/// independent lanes. Not for adversarial input.
pub(crate) struct StateHasher {
    state: u64,
}

/// Odd multipliers: 2^64 / the golden ratio, then three lane constants.
const MULTIPLIERS: [u64; 4] = [
    0x9e37_79b9_7f4a_7c15,
    0xc2b2_ae3d_27d4_eb4f,
    0x1656_67b1_9e37_79f9,
    0x85eb_ca77_c2b2_ae63,
];
/// The fractional digits of pi.
const SEED: u64 = 0x243f_6a88_85a3_08d3;

#[inline]
fn folded_multiply(value: u64, multiplier: u64) -> u64 {
    let product = u128::from(value) * u128::from(multiplier);
    (product as u64) ^ ((product >> 64) as u64)
}

impl StateHasher {
    pub(crate) fn new() -> Self {
        Self { state: SEED }
    }

    #[inline]
    fn mix(&mut self, value: u64) {
        self.state = folded_multiply(self.state ^ value, MULTIPLIERS[0]);
    }
}

/// Feed `items` to `hasher` as fixed-width records in bulk writes, after the
/// item count; `record` gives one item's little-endian bytes. A write per
/// field would cost a mix per field per item, and the grids this serves hold
/// a value per map cell.
pub(crate) fn hash_records<T, const N: usize>(
    items: &[T],
    hasher: &mut impl Hasher,
    record: impl Fn(&T) -> [u8; N],
) {
    const BATCH_BYTES: usize = 4096;
    const { assert!(N > 0 && N <= BATCH_BYTES) };
    items.len().hash(hasher);
    let mut buffer = [0u8; BATCH_BYTES];
    for batch in items.chunks(BATCH_BYTES / N) {
        let bytes = &mut buffer[..batch.len() * N];
        for (slot, item) in bytes.as_chunks_mut::<N>().0.iter_mut().zip(batch) {
            *slot = record(item);
        }
        hasher.write(bytes);
    }
}

/// One [`StateHasher`] digest per item, each fed by `fold`. With `parallel`
/// the items spread over scoped threads; a digest depends on its item alone,
/// so the result never depends on the thread count.
pub(crate) fn digests<T: Sync>(
    items: &[T],
    parallel: bool,
    fold: impl Fn(&T, &mut StateHasher) + Sync,
) -> Vec<u64> {
    let digest = |item: &T| {
        let mut hasher = StateHasher::new();
        fold(item, &mut hasher);
        hasher.finish()
    };
    if parallel {
        crate::sim::parallel::map(items, digest)
    } else {
        items.iter().map(digest).collect()
    }
}

impl Hasher for StateHasher {
    fn finish(&self) -> u64 {
        // MurmurHash3's fmix64, so every state bit reaches every hash bit.
        let mut hash = self.state;
        hash ^= hash >> 33;
        hash = hash.wrapping_mul(0xff51_afd7_ed55_8ccd);
        hash ^= hash >> 33;
        hash = hash.wrapping_mul(0xc4ce_b9fe_1a85_ec53);
        hash ^ (hash >> 33)
    }

    fn write(&mut self, bytes: &[u8]) {
        let (blocks, rest) = bytes.as_chunks::<32>();
        if !blocks.is_empty() {
            let mut lanes = MULTIPLIERS.map(|multiplier| self.state ^ multiplier);
            for block in blocks {
                let (words, _) = block.as_chunks::<8>();
                for ((lane, multiplier), word) in lanes.iter_mut().zip(MULTIPLIERS).zip(words) {
                    *lane = folded_multiply(*lane ^ u64::from_le_bytes(*word), multiplier);
                }
            }
            for lane in lanes {
                self.mix(lane);
            }
        }
        let (words, rest) = rest.as_chunks::<8>();
        for word in words {
            self.mix(u64::from_le_bytes(*word));
        }
        let mut tail = [0u8; 8];
        tail[..rest.len()].copy_from_slice(rest);
        self.mix(u64::from_le_bytes(tail));
        self.mix(bytes.len() as u64);
    }

    fn write_u8(&mut self, value: u8) {
        self.mix(u64::from(value));
    }

    fn write_u16(&mut self, value: u16) {
        self.mix(u64::from(value));
    }

    fn write_u32(&mut self, value: u32) {
        self.mix(u64::from(value));
    }

    fn write_u64(&mut self, value: u64) {
        self.mix(value);
    }

    fn write_u128(&mut self, value: u128) {
        self.mix(value as u64);
        self.mix((value >> 64) as u64);
    }

    fn write_usize(&mut self, value: usize) {
        self.mix(value as u64);
    }

    fn write_i8(&mut self, value: i8) {
        self.write_u8(value as u8);
    }

    fn write_i16(&mut self, value: i16) {
        self.write_u16(value as u16);
    }

    fn write_i32(&mut self, value: i32) {
        self.write_u32(value as u32);
    }

    fn write_i64(&mut self, value: i64) {
        self.write_u64(value as u64);
    }

    fn write_i128(&mut self, value: i128) {
        self.write_u128(value as u128);
    }

    fn write_isize(&mut self, value: isize) {
        self.write_u64(value as i64 as u64);
    }
}

#[cfg(test)]
mod tests {
    use super::{StateHasher, digests, hash_records};
    use std::hash::{Hash, Hasher};

    fn hash(value: impl Hash) -> u64 {
        let mut hasher = StateHasher::new();
        value.hash(&mut hasher);
        hasher.finish()
    }

    /// The hash must not change with the platform or the Rust release: CI
    /// hosts of every supported OS and CPU compare against these values.
    const PINNED_MIXED: u64 = 0x8d9b_cd19_8c88_70c7;
    const PINNED_EMPTY: u64 = 0x668d_3463_22a1_5e23;

    #[test]
    fn the_hash_of_fixed_values_is_pinned() {
        assert_eq!(
            (
                hash((1u8, -2i32, 3usize, "four", [5u8; 40])),
                hash(Vec::<u64>::new())
            ),
            (PINNED_MIXED, PINNED_EMPTY)
        );
    }

    #[test]
    fn byte_runs_split_differently_hash_differently() {
        let bytes: Vec<u8> = (0..100).collect();
        let whole = hash(&bytes[..]);
        assert_ne!(whole, hash((&bytes[..40], &bytes[40..])));
        assert_ne!(whole, hash(&bytes[..99]));
    }

    /// 100 bytes cover all four lanes of three blocks, a whole word after
    /// them and a padded tail.
    #[test]
    fn every_byte_of_a_run_moves_the_hash() {
        let bytes: Vec<u8> = (0..100).collect();
        let whole = hash(&bytes[..]);
        for index in 0..bytes.len() {
            let mut changed = bytes.clone();
            changed[index] ^= 1;
            assert_ne!(hash(&changed[..]), whole, "byte {index}");
        }
    }

    #[test]
    fn every_record_across_batches_moves_the_hash() {
        // 3000 four-byte records span three 4096-byte batches.
        let records: Vec<u32> = (0..3000).collect();
        let fold = |records: &[u32]| {
            let mut hasher = StateHasher::new();
            hash_records(records, &mut hasher, |value| value.to_le_bytes());
            hasher.finish()
        };
        let whole = fold(&records);
        for index in [0, 1023, 1024, 2047, 2048, 2999] {
            let mut changed = records.clone();
            changed[index] ^= 1;
            assert_ne!(fold(&changed), whole, "record {index}");
        }
        assert_ne!(fold(&records[..2999]), whole);
    }

    #[test]
    fn parallel_digests_match_serial_ones() {
        let items: Vec<(u64, Vec<u8>)> = (0..1000)
            .map(|n| (n, vec![n as u8; n as usize % 70]))
            .collect();
        let fold = |item: &(u64, Vec<u8>), hasher: &mut StateHasher| item.hash(hasher);
        let serial = digests(&items, false, fold);
        assert_eq!(digests(&items, true, fold), serial);
        assert_eq!(serial[7], hash(&items[7]));
    }
}
