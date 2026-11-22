//! No-dependency SHA-1, the digest of gamemd's `SHAEngine`
//! (`SHAPipe::Result` `0x0069E9E0`, init words `67452301..C3D2E1F0`), which
//! `CCINIClass::GetCRC` (`0x00476D80`) runs over an INI's `SaveToPipe` bytes.
//!
//! The digest is computed over the caller's bytes without copying them: a
//! whole MIX archive or a map blob can be hundreds of megabytes, and a copy
//! plus a byte-at-a-time padding pass held three times the input in memory
//! while the digest was computed.

/// SHA-1 of `bytes`.
pub fn sha1_bytes(bytes: &[u8]) -> [u8; 20] {
    let mut engine = Sha1::new();
    engine.update(bytes);
    engine.finish()
}

/// The running SHA-1 state: the five words, the partial block and the length
/// the padding encodes.
struct Sha1 {
    state: [u32; 5],
    /// The bytes of the block that is still being filled.
    block: [u8; 64],
    /// How many bytes of `block` are filled.
    filled: usize,
    /// The total number of bytes fed in, which the padding encodes.
    length: u64,
}

impl Sha1 {
    fn new() -> Self {
        Self {
            state: [
                0x6745_2301,
                0xefcd_ab89,
                0x98ba_dcfe,
                0x1032_5476,
                0xc3d2_e1f0,
            ],
            block: [0; 64],
            filled: 0,
            length: 0,
        }
    }

    fn update(&mut self, mut bytes: &[u8]) {
        self.length = self.length.wrapping_add(bytes.len() as u64);
        // A full block in the buffer and more input: compress the buffer first.
        if self.filled > 0 {
            let take = (64 - self.filled).min(bytes.len());
            self.block[self.filled..self.filled + take].copy_from_slice(&bytes[..take]);
            self.filled += take;
            bytes = &bytes[take..];
            if self.filled == 64 {
                let block = self.block;
                self.compress(&block);
                self.filled = 0;
            }
        }
        // Whole blocks straight from the caller's bytes.
        let mut blocks = bytes.chunks_exact(64);
        for block in &mut blocks {
            let mut chunk = [0_u8; 64];
            chunk.copy_from_slice(block);
            self.compress(&chunk);
        }
        // The rest starts the next block.
        let rest = blocks.remainder();
        self.block[..rest.len()].copy_from_slice(rest);
        self.filled = rest.len();
    }

    fn finish(mut self) -> [u8; 20] {
        let bit_length = self.length.wrapping_mul(8);
        // The padding: a 0x80 byte, zeroes to 56 bytes into the block, then the
        // length. It never needs more than one extra block.
        self.block[self.filled] = 0x80;
        let rest = self.filled + 1;
        if rest > 56 {
            self.block[rest..].fill(0);
            let block = self.block;
            self.compress(&block);
            self.block = [0; 64];
            self.filled = 0;
        } else {
            self.filled = rest;
        }
        self.block[self.filled..56].fill(0);
        self.block[56..64].copy_from_slice(&bit_length.to_be_bytes());
        let block = self.block;
        self.compress(&block);

        let mut digest = [0_u8; 20];
        for (chunk, word) in digest.chunks_exact_mut(4).zip(self.state) {
            chunk.copy_from_slice(&word.to_be_bytes());
        }
        digest
    }

    /// The SHA-1 compression function over one 64-byte block.
    fn compress(&mut self, block: &[u8; 64]) {
        let mut words = [0_u32; 80];
        for (word, chunk) in words.iter_mut().zip(block.chunks_exact(4)) {
            *word = u32::from_be_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]);
        }
        for index in 16..80 {
            words[index] =
                (words[index - 3] ^ words[index - 8] ^ words[index - 14] ^ words[index - 16])
                    .rotate_left(1);
        }
        let [mut a, mut b, mut c, mut d, mut e] = self.state;
        for (index, word) in words.iter().enumerate() {
            let (f, k) = match index {
                0..20 => ((b & c) | (!b & d), 0x5a82_7999),
                20..40 => (b ^ c ^ d, 0x6ed9_eba1),
                40..60 => ((b & c) | (b & d) | (c & d), 0x8f1b_bcdc),
                _ => (b ^ c ^ d, 0xca62_c1d6),
            };
            let temp = a
                .rotate_left(5)
                .wrapping_add(f)
                .wrapping_add(e)
                .wrapping_add(k)
                .wrapping_add(*word);
            e = d;
            d = c;
            c = b.rotate_left(30);
            b = a;
            a = temp;
        }
        for (slot, value) in self.state.iter_mut().zip([a, b, c, d, e]) {
            *slot = slot.wrapping_add(value);
        }
    }
}

#[cfg(all(test, tests_util))]
mod tests {
    use super::sha1_bytes;

    fn hex(digest: [u8; 20]) -> String {
        digest.iter().map(|byte| format!("{byte:02x}")).collect()
    }

    #[test]
    fn sha1_matches_known_vectors() {
        assert_eq!(
            hex(sha1_bytes(b"")),
            "da39a3ee5e6b4b0d3255bfef95601890afd80709"
        );
        assert_eq!(
            hex(sha1_bytes(b"abc")),
            "a9993e364706816aba3e25717850c26c9cd0d89d"
        );
        assert_eq!(
            hex(sha1_bytes(
                b"abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq"
            )),
            "84983e441c3bd26ebaae4aa1f95129e5e54670f1"
        );
        assert_eq!(
            hex(sha1_bytes(b"The quick brown fox jumps over the lazy dog")),
            "2fd4e1c67a2d28fced849ee1bb76e7391b93eb12"
        );
    }

    #[test]
    fn sha1_matches_the_long_known_vector() {
        // A million 'a' bytes: the published vector for the multi-block case.
        let digest = sha1_bytes(&vec![b'a'; 1_000_000]);
        assert_eq!(hex(digest), "34aa973cd4c4daa4f61eeb2bdbad27316534016f");
    }

    #[test]
    fn sha1_holds_across_every_block_boundary() {
        // The padding changes shape at 55, 56 and 64 bytes, so a length on
        // each side of a boundary is checked against a reference digest.
        for (length, expected) in [
            (1_usize, "11f6ad8ec52a2984abaafd7c3b516503785c2072"),
            (54, "31045e7bb077ff8d188a776b196b980388735dbb"),
            (55, "cef734ba81a024479e09eb5a75b6ddae62e6abf1"),
            (56, "901305367c259952f4e7af8323f480d59f81335b"),
            (57, "025ecbd5d70f8fb3c5457cd96bab13fda305dc59"),
            (63, "0ddc4e0cccd9a12850deb5abb0853a4425559fec"),
            (64, "bb2fa3ee7afb9f54c6dfb5d021f14b1ffe40c163"),
            (65, "78c741ddc482e4cdf8c474a0876347a0905b6233"),
            (119, "4300320394f7ee239bcdce7d3b8bcee173a0cd5c"),
            (120, "ceb2821639c4b6dcb10bce0e522ca2e608ce056d"),
            (128, "150fa3fbdc899bd0b8f95a9fb6027f564d953762"),
        ] {
            let digest = sha1_bytes(&vec![b'x'; length]);
            assert_eq!(hex(digest), expected, "length {length}");
        }
    }
}
