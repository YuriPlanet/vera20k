//! Cryptographic decryption for RA2 MIX archive headers.
//!
//! RA2 MIX files encrypt their file index (the table of contents) using
//! Blowfish in ECB mode. The 56-byte Blowfish key is itself encrypted
//! with RSA (public key hardcoded in the game executable).
//!
//! The MIX header supplies two little-endian RSA blocks. The game takes
//! 39 bytes from each decrypted block and uses the first 56 bytes of that
//! stream as the key for the Blowfish-encrypted index.
//!
//! ## Dependency rules
//! - Part of assets/ — no dependencies on game modules.
//! - Uses `assets::blowfish` for the cipher and num-bigint for RSA math.

// Outside data reaches this module: it refuses bad input, never panics.
#![cfg_attr(
    not(test),
    deny(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::unreachable
    )
)]

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

/// PKey::Decrypt @ 0x632870 takes one fewer byte per block than the ciphertext.
const RSA_PLAIN_BLOCK_SIZE: usize = RSA_HALF_SIZE - 1;

/// PKStraw::Get @ 0x633130 passes the first 56 decrypted bytes to Blowfish.
pub fn extract_blowfish_key(encrypted_block: &[u8]) -> Result<[u8; BLOWFISH_KEY_SIZE], AssetError> {
    let Some(blocks) = encrypted_block.get(..RSA_KEY_BLOCK_SIZE) else {
        return Err(AssetError::MixDecryptionError {
            reason: format!(
                "RSA key block too small: {} bytes (need {})",
                encrypted_block.len(),
                RSA_KEY_BLOCK_SIZE
            ),
        });
    };

    let modulus = BigUint::from_bytes_be(&RSA_MODULUS_BE);
    let exponent = BigUint::from(RSA_EXPONENT);
    let mut key = [0; BLOWFISH_KEY_SIZE];
    for (ciphertext, destination) in blocks
        .chunks_exact(RSA_HALF_SIZE)
        .zip(key.chunks_mut(RSA_PLAIN_BLOCK_SIZE))
    {
        let ciphertext = BigUint::from_bytes_le(ciphertext);
        // Modular exponentiation @ 0x5C8500 leaves its initial 1 when the
        // ciphertext is at least the modulus. PKey::Decrypt still copies it.
        let plaintext = if ciphertext < modulus {
            ciphertext.modpow(&exponent, &modulus)
        } else {
            BigUint::from(1u8)
        };
        let bytes = plaintext.to_bytes_le();
        let count = destination.len().min(bytes.len());
        destination[..count].copy_from_slice(&bytes[..count]);
    }
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

#[cfg(all(test, tests_assets))]
mod tests {
    use super::*;

    #[test]
    fn rsa_block_equal_to_modulus_decodes_to_one() {
        let mut key_block = [0; RSA_KEY_BLOCK_SIZE];
        for (byte, modulus_byte) in key_block.iter_mut().zip(RSA_MODULUS_BE.iter().rev()) {
            *byte = *modulus_byte;
        }
        // Original PKey::Decrypt (0x632870), input blocks (modulus, 0).
        let mut expected = [0; BLOWFISH_KEY_SIZE];
        expected[0] = 1;
        assert_eq!(extract_blowfish_key(&key_block).unwrap(), expected);
    }
}
