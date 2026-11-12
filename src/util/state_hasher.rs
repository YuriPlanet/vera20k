//! The portable 64-bit hasher behind the simulation state hash and the rules,
//! layer-stack and map identities that replays and saves compare, plus the
//! bulk-write helpers that keep hashing a 20,000-unit world cheap.
//!
//! It lives in `util/` because `rules` hashes its own identities with it
//! (`IniFile::content_hash`, the processed registry, `Ruleset`'s
//! simulation-config hash) and `rules` may not depend on `sim`. The
//! rayon-backed batch digest over it stays in `sim::state_hasher`, which owns
//! the worker pool.

use std::hash::{Hash, Hasher};

/// A pinned, portable 64-bit hasher for the simulation state hash.
///
/// The hash names a simulation state for replay checks and A/B comparisons,
/// so it must not depend on the host: an integer written alone mixes as its
/// value whatever the word size, std passes integer slices as native-endian
/// bytes (little-endian on every supported target) at their own width, so a
/// `[usize]` goes through [`hash_indices`], and the algorithm is fixed here,
/// where std's `DefaultHasher` promises none. A write costs one
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
    hash_record_iter(items.len(), items, hasher, record);
}

/// [`hash_records`] over `len` items from any iterator, in the same batches,
/// so the bytes written match the slice form's exactly.
pub(crate) fn hash_record_iter<'a, T: 'a, const N: usize>(
    len: usize,
    items: impl IntoIterator<Item = &'a T>,
    hasher: &mut impl Hasher,
    record: impl Fn(&T) -> [u8; N],
) {
    const BATCH_BYTES: usize = 4096;
    const { assert!(N > 0 && N <= BATCH_BYTES) };
    len.hash(hasher);
    let per_batch = BATCH_BYTES / N;
    let mut buffer = [0u8; BATCH_BYTES];
    let mut filled = 0;
    for item in items {
        buffer[filled * N..(filled + 1) * N].copy_from_slice(&record(item));
        filled += 1;
        if filled == per_batch {
            hasher.write(&buffer[..filled * N]);
            filled = 0;
        }
    }
    if filled > 0 {
        hasher.write(&buffer[..filled * N]);
    }
}

/// Feed `indices` to `hasher` as a 64-bit target feeds a `[usize]`: the
/// count, then every index as eight little-endian bytes in one write. std
/// writes a `[usize]` at the native width, four bytes an index on wasm32.
pub(crate) fn hash_indices(indices: &[usize], hasher: &mut impl Hasher) {
    indices.len().hash(hasher);
    let bytes: Vec<u8> = indices
        .iter()
        .flat_map(|&index| (index as u64).to_le_bytes())
        .collect();
    hasher.write(&bytes);
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
    use super::{StateHasher, hash_indices, hash_records};
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

    /// A wasm32 build hashes an index list as the 64-bit builds do.
    #[test]
    fn an_index_list_hashes_as_64_bit_words() {
        let mut indices = StateHasher::new();
        hash_indices(&[3, 70_000], &mut indices);
        assert_eq!(indices.finish(), hash([3u64, 70_000].as_slice()));
    }

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
}
