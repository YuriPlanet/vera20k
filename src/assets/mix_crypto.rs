//! Cryptographic decryption for RA2 MIX archive headers.
//!
//! RA2 MIX files encrypt their file index (the table of contents) using
//! Blowfish in ECB mode. The 56-byte Blowfish key is itself encrypted
//! with RSA (public key hardcoded in the game executable).
//!
//! ## Decryption flow
//! 1. Read the 80-byte RSA-encrypted key block from the MIX file
//! 2. Byte-reverse the entire 80 bytes (file is LE, RSA math needs BE)
//! 3. Split into two 40-byte halves, RSA-decrypt each as big-endian
//! 4. Combine: `(s0 << 312) + s1` to form a 56-byte big-endian value
//! 5. Byte-reverse the 56 bytes back to little-endian for Blowfish
//! 6. Use standard Blowfish ECB to decrypt the file index
//!
//! ## Why standard Blowfish (not "Blowfish-LE")?
//! Verified against two independent working implementations:
//! - ccmixar (Go) uses standard `golang.org/x/crypto/blowfish`
//! - ccmix (C++) uses Crypto++ `ECB_Mode<Blowfish>`
//!
//! Both use standard big-endian Blowfish and handle endianness at the
//! RSA level (byte-reversing before/after RSA math) rather than at
//! the Blowfish block level.
//!
//! ## Security note
//! This is a 320-bit RSA key from 2000 — trivially breakable by modern
//! standards. It's only meant to prevent casual file browsing, not
//! real security. We're implementing it to read the game's own files.
//!
//! ## Dependency rules
//! - Part of assets/ — no dependencies on game modules.
//! - Uses `assets::blowfish` for the cipher and num-bigint for RSA math.

use num_bigint::BigUint;

use crate::assets::blowfish::Blowfish;
use crate::assets::error::AssetError;

/// RSA public exponent for MIX decryption.
/// Standard small exponent used by most RSA implementations.
const RSA_EXPONENT: u32 = 65537;

/// RSA public modulus for MIX decryption (40 bytes, **big-endian**).
///
/// Extracted from the Base64-encoded key in Westwood's `keys.ini` file,
/// which was found inside a Tiberian Sun MIX archive. The DER header
/// (0x02 0x28) is stripped — these are the raw 40-byte integer bytes.
///
/// Base64 source: `AihRvNoIbTn85FZRYNZRcT+i6KpU+maCsEqr3Q5q+LDB5tH7Tz2qQ38V`
///
/// This same key is used for all C&C games from TS through RA2/YR.
/// It's documented in every open-source MIX reader.
const RSA_MODULUS_BE: [u8; 40] = [
    0x51, 0xBC, 0xDA, 0x08, 0x6D, 0x39, 0xFC, 0xE4, 0x56, 0x51, 0x60, 0xD6, 0x51, 0x71, 0x3F, 0xA2,
    0xE8, 0xAA, 0x54, 0xFA, 0x66, 0x82, 0xB0, 0x4A, 0xAB, 0xDD, 0x0E, 0x6A, 0xF8, 0xB0, 0xC1, 0xE6,
    0xD1, 0xFB, 0x4F, 0x3D, 0xAA, 0x43, 0x7F, 0x15,
];

/// Size of the RSA-encrypted key block in the MIX header.
pub const RSA_KEY_BLOCK_SIZE: usize = 80;

/// Size of one RSA-encrypted half (matches modulus size).
const RSA_HALF_SIZE: usize = 40;

/// Size of the Blowfish key derived from RSA decryption.
/// 56 bytes = 448 bits = maximum Blowfish key length.
const BLOWFISH_KEY_SIZE: usize = 56;

/// Blowfish block size in bytes. ECB mode processes one block at a time.
pub const BLOWFISH_BLOCK_SIZE: usize = 8;

/// Bit shift for combining two RSA-decrypted halves.
/// 312 bits = 39 bytes. The formula `(s0 << 312) + s1` concatenates
/// the two halves into a single 56-byte value.
const RSA_COMBINE_SHIFT_BITS: u64 = 312;

/// Extract the 56-byte Blowfish key from the 80-byte RSA-encrypted block.
///
/// Algorithm (verified against ccmixar and ccmix implementations):
/// 1. Reverse the 80 bytes (file LE → big-endian for RSA math)
/// 2. Split into two 40-byte halves
/// 3. RSA-decrypt each half: `plaintext = ciphertext^e mod n`
/// 4. Combine: `(s0 << 312) + s1`
/// 5. Encode as 56-byte big-endian, then reverse back to LE
pub fn extract_blowfish_key(encrypted_block: &[u8]) -> Result<[u8; BLOWFISH_KEY_SIZE], AssetError> {
    if encrypted_block.len() < RSA_KEY_BLOCK_SIZE {
        return Err(AssetError::MixDecryptionError {
            reason: format!(
                "RSA key block too small: {} bytes (need {})",
                encrypted_block.len(),
                RSA_KEY_BLOCK_SIZE
            ),
        });
    }

    // Step 1: Reverse the 80 bytes (convert from file's LE to BE for RSA).
    let mut reversed: Vec<u8> = encrypted_block[..RSA_KEY_BLOCK_SIZE].to_vec();
    reversed.reverse();

    // Step 2: Split into two 40-byte halves.
    let block0: &[u8] = &reversed[..RSA_HALF_SIZE];
    let block1: &[u8] = &reversed[RSA_HALF_SIZE..RSA_KEY_BLOCK_SIZE];

    // Step 3: RSA-decrypt each half as big-endian integers.
    let s0: BigUint = rsa_decrypt_be(block0);
    let s1: BigUint = rsa_decrypt_be(block1);

    // Step 4: Combine into a single value: (s0 << 312) + s1
    let combined: BigUint = (s0 << RSA_COMBINE_SHIFT_BITS) + s1;

    // Step 5: Encode as 56-byte big-endian, then reverse to LE.
    let be_bytes: Vec<u8> = combined.to_bytes_be();
    let mut key: [u8; BLOWFISH_KEY_SIZE] = [0u8; BLOWFISH_KEY_SIZE];

    // Right-align big-endian bytes within the 56-byte buffer
    // (if the combined value has fewer than 56 bytes, pad left with zeros).
    if be_bytes.len() <= BLOWFISH_KEY_SIZE {
        let start: usize = BLOWFISH_KEY_SIZE - be_bytes.len();
        key[start..].copy_from_slice(&be_bytes);
    } else {
        // Combined value is larger than 56 bytes — take the low 56 bytes.
        let skip: usize = be_bytes.len() - BLOWFISH_KEY_SIZE;
        key.copy_from_slice(&be_bytes[skip..]);
    }

    // Reverse to convert from big-endian to little-endian (file format).
    key.reverse();

    Ok(key)
}

/// Decrypt data in-place using standard Blowfish in ECB mode.
///
/// Standard big-endian Blowfish matches ccmixar (Go) and ccmix (C++),
/// which both use it without any endianness modifications at the block
/// level.
///
/// The data length must be a multiple of 8 bytes (Blowfish block size).
/// Only the file index is encrypted — body data is NOT encrypted.
pub fn blowfish_decrypt_ecb(key: &[u8], data: &mut [u8]) -> Result<(), AssetError> {
    if !data.len().is_multiple_of(BLOWFISH_BLOCK_SIZE) {
        return Err(AssetError::MixDecryptionError {
            reason: format!(
                "Blowfish data length {} is not a multiple of block size {}",
                data.len(),
                BLOWFISH_BLOCK_SIZE
            ),
        });
    }

    let cipher = Blowfish::new(key).ok_or_else(|| AssetError::MixDecryptionError {
        reason: format!("Blowfish key length {} is not 4 to 56 bytes", key.len()),
    })?;

    // Process each 8-byte block independently (ECB mode).
    for block in data.as_chunks_mut::<BLOWFISH_BLOCK_SIZE>().0 {
        cipher.decrypt_block(block);
    }

    Ok(())
}

/// Perform textbook RSA decryption on big-endian bytes.
///
/// Computes: plaintext = ciphertext^exponent mod modulus
///
/// Returns the decrypted result as a BigUint (caller decides byte format).
/// This is "textbook RSA" with no padding (PKCS#1, OAEP, etc.),
/// which is what Westwood used in the original game.
fn rsa_decrypt_be(ciphertext_be: &[u8]) -> BigUint {
    let ciphertext: BigUint = BigUint::from_bytes_be(ciphertext_be);
    let exponent: BigUint = BigUint::from(RSA_EXPONENT);
    let modulus: BigUint = BigUint::from_bytes_be(&RSA_MODULUS_BE);

    // Core RSA operation: m = c^e mod n
    ciphertext.modpow(&exponent, &modulus)
}
