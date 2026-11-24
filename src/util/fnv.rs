//! FNV-1a 64-bit fold — the crate's single implementation.
//!
//! Three determinism-comparison hashes (the parity digest, the retail trig
//! table check, and the RNG debug state) fold with the same constants and the
//! same xor-then-multiply byte loop; they all delegate here so the fold has
//! one landing site.

pub const FNV1A64_OFFSET_BASIS: u64 = 0xCBF2_9CE4_8422_2325;
pub const FNV1A64_PRIME: u64 = 0x0000_0100_0000_01B3;

/// Fold `bytes` into `hash` (FNV-1a: xor byte, multiply by prime).
#[inline]
pub fn fnv1a64_fold_bytes(mut hash: u64, bytes: &[u8]) -> u64 {
    for &byte in bytes {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(FNV1A64_PRIME);
    }
    hash
}
