//! IMA ADPCM decoding — the single decoder every compressed retail sound flows through.
//!
//! `gamemd.exe` contains exactly **one** IMA-ADPCM nibble decoder,
//! `IMA_ADPCM__DecodeSample @ 0x0040ACD0`, and exactly **one** block decoder,
//! `IMA_ADPCM__DecodeBlock @ 0x0040AA70`. Verified by `get_xrefs_to` on both
//! coefficient tables (`0x00816558` step, `0x00816518` index adjust): each has a
//! single DATA reference, both from `0x0040ACD0`; a `search_byte_patterns` sweep
//! for the step table's head found no second copy in the image. The block decoder
//! is reached only as the `+0xB0` callback installed by the audio pipeline setup
//! `FUN_00409C40 @ 0x00409C40` when the format descriptor's compression id
//! (`+0x04`) is 1.
//!
//! Both retail sound sources feed that one pipeline through the *same* format
//! descriptor, so one Rust implementation is the gamemd-native structure rather
//! than a Rust convenience:
//! - bag entries: `AudioIndex__GetFormat @ 0x00401640` fills the descriptor from
//!   an `audio.idx` entry (compression 1 when flags bit 3 is set, `chunk_size`
//!   from entry `+0x20` into descriptor `+0x18`).
//! - WAV files: `WAV__ParseHeader @ 0x00408610` fills the same descriptor for
//!   `wFormatTag == 0x11`, taking descriptor `+0x18` from the fmt chunk's
//!   **`nBlockAlign`** (`psVar6[6]`, fmt bytes 12..14).
//!
//! `FUN_00409C40` then reads descriptor `+0x18` into the stream's input-buffer
//! size (`+0x8C`/`+0xAC`) and sizes the output buffer as `chunk*4 + 0x80`, so the
//! field is a **byte** stride — the number of compressed bytes per block — not a
//! sample count. (`docs/research/AUDIO_IDX_BAG_GHIDRA_REPORT.md` §4 called it
//! `wSamplesPerBlock`; that name is wrong, the offset it cites is `nBlockAlign`.)
//!
//! ## No VOC decoder, and none is needed
//!
//! `gamemd.exe` has no Creative VOC support: `search_strings "(?i)\.voc"` and
//! `search_strings "(?i)creative voice"` both return zero matches, no retail
//! `ini/` file references a `.voc`, and the retail install carries no `.voc`
//! file. The format is excluded, not unimplemented.
//!
//! ## Dependency rules
//! - Part of `assets/` — standalone decoder, no game dependencies.

/// IMA ADPCM step size table (89 entries).
///
/// Machine-read from `gamemd.exe` at `0x00816558` (89 x i32) — see
/// `ima_step_table_matches_image_bytes`.
pub(crate) const STEP_TABLE: [i32; 89] = [
    7, 8, 9, 10, 11, 12, 13, 14, 16, 17, 19, 21, 23, 25, 28, 31, 34, 37, 41, 45, 50, 55, 60, 66,
    73, 80, 88, 97, 107, 118, 130, 143, 157, 173, 190, 209, 230, 253, 279, 307, 337, 371, 408, 449,
    494, 544, 598, 658, 724, 796, 876, 963, 1060, 1166, 1282, 1411, 1552, 1707, 1878, 2066, 2272,
    2499, 2749, 3024, 3327, 3660, 4026, 4428, 4871, 5358, 5894, 6484, 7132, 7845, 8630, 9493,
    10442, 11487, 12635, 13899, 15289, 16818, 18500, 20350, 22385, 24623, 27086, 29794, 32767,
];

/// IMA ADPCM step index adjustment, indexed by the low 3 bits of the nibble.
///
/// The native table at `0x00816518` is 16 x i32 and mirrors entries 0..7 into
/// 8..15 (`[-1,-1,-1,-1,2,4,6,8]` twice), so indexing it by the full nibble is
/// identical to indexing these 8 entries by `nibble & 7` — see
/// `ima_index_table_matches_image_bytes`.
pub(crate) const INDEX_ADJUST: [i32; 8] = [-1, -1, -1, -1, 2, 4, 6, 8];

/// Highest legal step index; the native clamps to this and rejects any block
/// preamble declaring more (`0x0040AAE8: CMP EDX,0x58 / JG reject`).
pub(crate) const MAX_STEP_INDEX: i32 = 0x58;

/// IMA ADPCM decoder state: the running predictor and step index.
///
/// `IMA_ADPCM__DecodeSample @ 0x0040ACD0` keeps this as two `i32` at
/// `[EDX]` (predictor) and `[EDX+4]` (step index).
#[derive(Clone, Copy)]
pub(crate) struct ImaAdpcmState {
    index: i32,
    predicted: i32,
}

impl ImaAdpcmState {
    pub(crate) fn new() -> Self {
        Self {
            index: 0,
            predicted: 0,
        }
    }

    /// Initialize state from a block preamble (predictor + step_index).
    pub(crate) fn set_state(&mut self, predicted: i32, index: i32) {
        self.predicted = predicted;
        self.index = index.clamp(0, MAX_STEP_INDEX);
    }

    /// Decode a single 4-bit IMA ADPCM nibble into a 16-bit sample.
    ///
    /// `IMA_ADPCM__DecodeSample @ 0x0040ACD0`, instruction for instruction:
    /// the predictor clamps (`0x0040AD0F`/`0x0040AD1F`) *before* the index
    /// update (`0x0040AD2C`), and the returned sample is the clamped predictor
    /// re-read as a 16-bit word (`0x0040AD5A: MOV AX, word ptr [EDX]`).
    pub(crate) fn decode_nibble(&mut self, nibble: u8) -> i16 {
        let step: i32 = STEP_TABLE[self.index as usize];
        let code: u8 = nibble & 0x07;

        // 0x0040ACDE..0x0040ACFD: step/8, plus step/4, step/2, step per set bit.
        let mut diff: i32 = step >> 3;
        if code & 0x04 != 0 {
            diff += step;
        }
        if code & 0x02 != 0 {
            diff += step >> 1;
        }
        if code & 0x01 != 0 {
            diff += step >> 2;
        }

        // 0x0040ACFF: sign bit 3 negates the delta.
        if nibble & 0x08 != 0 {
            self.predicted -= diff;
        } else {
            self.predicted += diff;
        }

        self.predicted = self.predicted.clamp(-32768, 32767);

        self.index += INDEX_ADJUST[code as usize];
        self.index = self.index.clamp(0, MAX_STEP_INDEX);

        self.predicted as i16
    }
}

/// Decode a run of packed nibbles, low nibble of each byte first.
///
/// The native's inner loops fetch `BL = [EBP]`, decode `BL & 0xF` then
/// `BL >> 4` (`0x0040AB83`/`0x0040AB99` mono, `0x0040AC36`/`0x0040AC50`
/// stereo), so the low nibble is always the earlier sample.
pub(crate) fn decode_nibbles(data: &[u8], state: &mut ImaAdpcmState, out: &mut Vec<i16>) {
    for &byte in data {
        out.push(state.decode_nibble(byte & 0x0F));
        out.push(state.decode_nibble((byte >> 4) & 0x0F));
    }
}

/// Bytes of preamble the native consumes per channel: `i16` predictor,
/// `u8` step index, `u8` reserved.
const PREAMBLE_BYTES_PER_CHANNEL: usize = 4;

/// Compressed bytes each channel contributes to one interleave group; each
/// group yields 8 sample frames.
const GROUP_BYTES_PER_CHANNEL: usize = 4;

/// Decode block-framed IMA ADPCM into interleaved 16-bit PCM.
///
/// This is `IMA_ADPCM__DecodeBlock @ 0x0040AA70` driven the way the native
/// streaming pump drives it, because the pump — not the file — decides how many
/// bytes each block decode sees.
///
/// Per block the native:
/// 1. requires at least `4 * channels` bytes, else fails (`0x0040AAAD`);
/// 2. reads one `{i16 predictor, u8 step_index, u8 reserved}` preamble per
///    channel and **rejects the block** if `step_index > 0x58` or
///    `reserved != 0` (`0x0040AAEE`, `0x0040AAF9`);
/// 3. emits each channel's predictor as the block's first sample frame
///    (`0x0040AB0C`);
/// 4. consumes the payload in groups of `4 * channels` bytes — 4 bytes per
///    channel, channel 0 first, the input pointer advancing monotonically —
///    each group producing 8 interleaved frames (`0x0040AB77` mono,
///    `0x0040AC2C` stereo).
///
/// A rejected block makes the decoder return false, which makes
/// `Audio__DecodeCompressedBlock` return 0 (`0x00409F4E`) and the buffer pump
/// abandon the sound, so the block's samples never reach the mixer. We stop and
/// return what earlier blocks produced, which is what the player hears.
///
/// ## The native feed: every block is exactly `block_align` bytes
///
/// The decoder's available-byte count is `+0x80 - +0x84` (`0x0040AA70`
/// prologue). Those two fields have exactly six writes in the whole image, four
/// of them in `Audio__DecodeCompressedBlock @ 0x00409DE0`:
/// - case 3 sets both to `+0xAC` = `block_align` (`0x00409F8A`, `0x00409F90`);
/// - case 0 fills the `malloc(block_align)` input buffer at `+0x7C`, writing at
///   offset `+0x80 - +0x84` and subtracting what it copied (`0x00409EA4`,
///   preceded by `0x00409E92 MOV ECX,[EBX+0x84]` / `0x00409E9A SUB ECX,EAX`);
/// - case 0 forces `+0x84 = 0` (`0x00409EBE MOV [EBX+0x84],ESI`, `ESI == 0`)
///   when the source is exhausted or the source pointer is NULL, paired with
///   the state transition at `0x00409EC4` that hands over to the block decoder.
///
/// The other two are `FUN_00409C40 @ 0x00409C40` zeroing both fields at
/// configure time (`0x00409C6C`, `0x00409C72`). They never reach the block
/// decoder: the same straight-line block forces the state back to 3
/// (`0x00409C78`), so case 3 reloads both from `+0xAC` first. `FUN_00409880 @
/// 0x00409880` writes neither field, and it calls `FUN_00409C40` only while
/// `+0xA8 == 0` (`0x00409A83 CMP [ESI+0xa8],EDI / JNZ`), so the buffer cannot be
/// reallocated or re-formatted while a partial block is pending. Case 0 only
/// ever leaves the fill state with `+0x84 == 0`, so **`avail` is always exactly
/// `block_align`**. Two consequences, both of which this function reproduces:
///
/// - At end of stream the pump calls the decode callback with a NULL source and
///   a NULL count (`0x00409B41`, taken while the stream's "decoder still holds
///   data" flag `+0xA8` is set), so a final short tail is still decoded as a
///   **full `block_align` block**. The bytes past the tail are whatever the
///   previous cycle left in the input buffer at those same offsets — i.e. the
///   previous block's bytes — because every cycle refills the buffer from
///   offset 0. We rebuild exactly that buffer. A source delivered in several
///   segments still flushes exactly once: a new segment sets the remaining
///   source count `+0x2C` (`0x00409ACD`) and `0x00409AE3 JG` then routes to the
///   with-source call at `0x00409B65`, so the flush branch is unreachable until
///   true EOF; and once case 1 has drained the flushed block it clears `+0xA8`
///   (`0x00409ED9`), after which the pump fills silence (`0x00409AED`) rather
///   than calling the decoder again.
/// - The "payload is not a whole number of groups" failure (mono
///   `0x0040AB4E: LEA EAX,[ECX+3] / SHR EAX,2` then `0x0040ABD4 SETZ`; stereo
///   `0x0040ACB3: TEST ECX,ECX / JG`) is therefore decided by `block_align`
///   alone, never by where the data ends. Both retail strides are whole
///   (`(512-4) % 4 == 0`, `(1024-8) % 8 == 0`), so it is unreachable on retail
///   data; we keep the rule gated on the stride so a synthetic stride behaves
///   natively. We do not reproduce the over-read values, which the rejected
///   block discards anyway.
pub fn decode_blocks(data: &[u8], channels: u16, block_align: u32) -> Vec<i16> {
    let mut out: Vec<i16> = Vec::with_capacity(data.len() * 2 + usize::from(channels.max(1)));
    let mut decoder = ImaBlockDecoder::new(channels, block_align);
    while decoder.decode_next(data, &mut out) {}
    out
}

/// [`decode_blocks`]' feed one block per call, so a long stream (a theme song)
/// decodes as its output pulls it, as the native pump calls
/// `IMA_ADPCM__DecodeBlock @ 0x0040AA70` once per refill rather than once per
/// file.
pub(crate) struct ImaBlockDecoder {
    channels: usize,
    block_align: usize,
    /// Payload offset of the next block.
    pos: usize,
    states: Vec<ImaAdpcmState>,
    /// A rejected or lone short block ended the stream.
    ended: bool,
}

impl ImaBlockDecoder {
    pub(crate) fn new(channels: u16, block_align: u32) -> Self {
        // VERA-internal, gamemd equivalent UNCHECKED: with `+0xA4 == 0` the native's
        // stereo loop at `0x0040ABED` subtracts 0 per pass and spins forever, so
        // there is no native behavior to match. No retail entry declares 0 channels
        // (`AudioIndex__GetFormat @ 0x00401640` derives it from a flag bit).
        let channels = channels.max(1) as usize;
        let block_align = block_align as usize;
        Self {
            channels,
            block_align,
            pos: 0,
            states: (0..channels).map(|_| ImaAdpcmState::new()).collect(),
            // `FUN_00409C40` copies the format descriptor's `+0x18` straight into
            // the stream's input-buffer size, so a zero stride leaves the block
            // decoder with zero available bytes and it fails on its first call:
            // the sound is silent. (No retail entry reaches this — both retail
            // `audio.idx` files are v2 and every IMA entry carries 512 (mono) or
            // 1024 (stereo).)
            ended: block_align < PREAMBLE_BYTES_PER_CHANNEL * channels,
        }
    }

    /// Append the next block of `data` — the whole payload, passed again on
    /// every call — to `out` as interleaved frames; `false`, appending
    /// nothing, once the stream has ended.
    pub(crate) fn decode_next(&mut self, data: &[u8], out: &mut Vec<i16>) -> bool {
        let (channels, block_align, pos) = (self.channels, self.block_align, self.pos);
        if self.ended || pos >= data.len() {
            return false;
        }
        let preamble = PREAMBLE_BYTES_PER_CHANNEL * channels;
        let group = GROUP_BYTES_PER_CHANNEL * channels;

        let real = &data[pos..(pos + block_align).min(data.len())];
        let stale_padded;
        let block: &[u8] = if real.len() == block_align {
            real
        } else if pos >= block_align {
            // End-of-stream flush: the input buffer still holds the previous
            // block's bytes past the short tail (see `decode_blocks`).
            let mut buffer = Vec::with_capacity(block_align);
            buffer.extend_from_slice(real);
            buffer.extend_from_slice(&data[pos - block_align + real.len()..pos]);
            stale_padded = buffer;
            &stale_padded
        } else {
            // VERA-internal, gamemd equivalent UNCHECKED: a sound shorter than
            // one stride is flushed against a buffer that was never filled by
            // this sound — `malloc`ed on first use, otherwise the previous
            // sound's bytes, since `FUN_00409C40` only reallocates when the
            // stride grows. Neither is reproducible from the file, so we decode
            // the whole groups this sound actually carries.
            if real.len() < preamble {
                self.ended = true;
                return false;
            }
            &real[..preamble + (real.len() - preamble) / group * group]
        };

        // A rejected block is discarded whole by the caller, so everything this
        // call appends has to go with it.
        let block_out_start = out.len();

        // Preambles: predictor, step index, reserved — per channel, in order.
        let mut rejected = !(block.len() - preamble).is_multiple_of(group);
        for (ch, state) in self.states.iter_mut().enumerate() {
            if rejected {
                break;
            }
            let base = ch * PREAMBLE_BYTES_PER_CHANNEL;
            let predictor = i16::from_le_bytes([block[base], block[base + 1]]) as i32;
            let step_index = block[base + 2] as i32;
            let reserved = block[base + 3];
            if step_index > MAX_STEP_INDEX || reserved != 0 {
                rejected = true;
                break;
            }
            state.set_state(predictor, step_index);
            out.push(predictor as i16);
        }
        if rejected {
            out.truncate(block_out_start);
            self.ended = true;
            return false;
        }

        // The loops work on a local copy of each state and a slice of `out`:
        // through `&mut self` and `&mut Vec` the optimizer must assume every
        // sample store may alias them (measured about 5% slower).
        let payload = &block[preamble..];
        if channels == 1 {
            let mut state = self.states[0];
            decode_nibbles(payload, &mut state, out);
            self.states[0] = state;
        } else {
            let mut group_start = out.len();
            for chunk in payload.chunks_exact(group) {
                out.resize(group_start + 8 * channels, 0);
                let frames = &mut out[group_start..];
                for (ch, slot) in self.states.iter_mut().enumerate() {
                    let mut state = *slot;
                    let bytes = &chunk[ch * GROUP_BYTES_PER_CHANNEL..][..GROUP_BYTES_PER_CHANNEL];
                    let mut frame = 0usize;
                    for &byte in bytes {
                        frames[frame * channels + ch] = state.decode_nibble(byte & 0x0F);
                        frame += 1;
                        frames[frame * channels + ch] = state.decode_nibble((byte >> 4) & 0x0F);
                        frame += 1;
                    }
                    *slot = state;
                }
                group_start += 8 * channels;
            }
        }

        self.pos += block_align;
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The native step table, machine-read out of `gamemd.exe` at `0x00816558`
    /// with `read_memory` (356 bytes = 89 x i32 little-endian). This is the
    /// binary's own data, not a restatement of ours.
    const IMAGE_STEP_TABLE_BYTES: &[u8] = &[
        0x07, 0x00, 0x00, 0x00, 0x08, 0x00, 0x00, 0x00, 0x09, 0x00, 0x00, 0x00, 0x0a, 0x00, 0x00,
        0x00, 0x0b, 0x00, 0x00, 0x00, 0x0c, 0x00, 0x00, 0x00, 0x0d, 0x00, 0x00, 0x00, 0x0e, 0x00,
        0x00, 0x00, 0x10, 0x00, 0x00, 0x00, 0x11, 0x00, 0x00, 0x00, 0x13, 0x00, 0x00, 0x00, 0x15,
        0x00, 0x00, 0x00, 0x17, 0x00, 0x00, 0x00, 0x19, 0x00, 0x00, 0x00, 0x1c, 0x00, 0x00, 0x00,
        0x1f, 0x00, 0x00, 0x00, 0x22, 0x00, 0x00, 0x00, 0x25, 0x00, 0x00, 0x00, 0x29, 0x00, 0x00,
        0x00, 0x2d, 0x00, 0x00, 0x00, 0x32, 0x00, 0x00, 0x00, 0x37, 0x00, 0x00, 0x00, 0x3c, 0x00,
        0x00, 0x00, 0x42, 0x00, 0x00, 0x00, 0x49, 0x00, 0x00, 0x00, 0x50, 0x00, 0x00, 0x00, 0x58,
        0x00, 0x00, 0x00, 0x61, 0x00, 0x00, 0x00, 0x6b, 0x00, 0x00, 0x00, 0x76, 0x00, 0x00, 0x00,
        0x82, 0x00, 0x00, 0x00, 0x8f, 0x00, 0x00, 0x00, 0x9d, 0x00, 0x00, 0x00, 0xad, 0x00, 0x00,
        0x00, 0xbe, 0x00, 0x00, 0x00, 0xd1, 0x00, 0x00, 0x00, 0xe6, 0x00, 0x00, 0x00, 0xfd, 0x00,
        0x00, 0x00, 0x17, 0x01, 0x00, 0x00, 0x33, 0x01, 0x00, 0x00, 0x51, 0x01, 0x00, 0x00, 0x73,
        0x01, 0x00, 0x00, 0x98, 0x01, 0x00, 0x00, 0xc1, 0x01, 0x00, 0x00, 0xee, 0x01, 0x00, 0x00,
        0x20, 0x02, 0x00, 0x00, 0x56, 0x02, 0x00, 0x00, 0x92, 0x02, 0x00, 0x00, 0xd4, 0x02, 0x00,
        0x00, 0x1c, 0x03, 0x00, 0x00, 0x6c, 0x03, 0x00, 0x00, 0xc3, 0x03, 0x00, 0x00, 0x24, 0x04,
        0x00, 0x00, 0x8e, 0x04, 0x00, 0x00, 0x02, 0x05, 0x00, 0x00, 0x83, 0x05, 0x00, 0x00, 0x10,
        0x06, 0x00, 0x00, 0xab, 0x06, 0x00, 0x00, 0x56, 0x07, 0x00, 0x00, 0x12, 0x08, 0x00, 0x00,
        0xe0, 0x08, 0x00, 0x00, 0xc3, 0x09, 0x00, 0x00, 0xbd, 0x0a, 0x00, 0x00, 0xd0, 0x0b, 0x00,
        0x00, 0xff, 0x0c, 0x00, 0x00, 0x4c, 0x0e, 0x00, 0x00, 0xba, 0x0f, 0x00, 0x00, 0x4c, 0x11,
        0x00, 0x00, 0x07, 0x13, 0x00, 0x00, 0xee, 0x14, 0x00, 0x00, 0x06, 0x17, 0x00, 0x00, 0x54,
        0x19, 0x00, 0x00, 0xdc, 0x1b, 0x00, 0x00, 0xa5, 0x1e, 0x00, 0x00, 0xb6, 0x21, 0x00, 0x00,
        0x15, 0x25, 0x00, 0x00, 0xca, 0x28, 0x00, 0x00, 0xdf, 0x2c, 0x00, 0x00, 0x5b, 0x31, 0x00,
        0x00, 0x4b, 0x36, 0x00, 0x00, 0xb9, 0x3b, 0x00, 0x00, 0xb2, 0x41, 0x00, 0x00, 0x44, 0x48,
        0x00, 0x00, 0x7e, 0x4f, 0x00, 0x00, 0x71, 0x57, 0x00, 0x00, 0x2f, 0x60, 0x00, 0x00, 0xce,
        0x69, 0x00, 0x00, 0x62, 0x74, 0x00, 0x00, 0xff, 0x7f, 0x00, 0x00,
    ];

    /// The native index-adjust table, machine-read at `0x00816518`
    /// (64 bytes = 16 x i32 little-endian).
    const IMAGE_INDEX_TABLE_BYTES: &[u8] = &[
        0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff,
        0xff, 0x02, 0x00, 0x00, 0x00, 0x04, 0x00, 0x00, 0x00, 0x06, 0x00, 0x00, 0x00, 0x08, 0x00,
        0x00, 0x00, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff,
        0xff, 0xff, 0xff, 0x02, 0x00, 0x00, 0x00, 0x04, 0x00, 0x00, 0x00, 0x06, 0x00, 0x00, 0x00,
        0x08, 0x00, 0x00, 0x00,
    ];

    fn as_i32_le(bytes: &[u8]) -> Vec<i32> {
        bytes
            .as_chunks::<4>()
            .0
            .iter()
            .map(|c| i32::from_le_bytes([c[0], c[1], c[2], c[3]]))
            .collect()
    }

    #[test]
    fn ima_step_table_matches_image_bytes() {
        let image = as_i32_le(IMAGE_STEP_TABLE_BYTES);
        assert_eq!(image.len(), STEP_TABLE.len(), "step table length");
        assert_eq!(image.as_slice(), STEP_TABLE.as_slice());
    }

    #[test]
    fn ima_index_table_matches_image_bytes() {
        let image = as_i32_le(IMAGE_INDEX_TABLE_BYTES);
        assert_eq!(image.len(), 16, "native table is 16 x i32");
        // Entries 8..15 mirror 0..7, which is why indexing our 8-entry table by
        // `nibble & 7` equals the native's full-nibble indexing.
        assert_eq!(&image[0..8], &image[8..16], "native halves must mirror");
        for (i, &want) in image[0..8].iter().enumerate() {
            assert_eq!(INDEX_ADJUST[i], want, "index adjust entry {i}");
        }
    }

    /// Golden vectors captured by emulating the original engine's IMA nibble
    /// decoder (`emulate_function` on `IMA_ADPCM__DecodeSample @ 0x0040ACD0`;
    /// capture log in docs/research/
    /// ADPCM_NIBBLE_VALUE_CERTIFICATION_GHIDRA_REPORT.md). Each row:
    /// (predictor, step_index) state, input nibble, expected 16-bit sample.
    /// Covers zero state, mid states, and both saturation clamps.
    #[test]
    fn adpcm_nibble_matches_original_engine_emulation_vectors() {
        const VECTORS: &[(i32, i32, u8, i16)] = &[
            (0, 0, 0x0, 0),
            (0, 0, 0x3, 4),
            (0, 0, 0x5, 8),
            (0, 0, 0x7, 11),
            (0, 0, 0x8, 0),
            (0, 0, 0xB, -4),
            (0, 0, 0xF, -11),
            (0, 4, 0x7, 19),
            (0, 4, 0xF, -19),
            (24, 40, 0x7, 655),
            (24, 40, 0xA, -186),
            (36, 64, 0x5, 4609),
            (36, 64, 0xD, -4537),
            (60, 84, 0x7, 32767),  // positive clamp
            (60, 84, 0xF, -32768), // negative clamp
        ];
        for &(pred, idx, nibble, expected) in VECTORS {
            let mut state = ImaAdpcmState::new();
            state.set_state(pred, idx);
            assert_eq!(
                state.decode_nibble(nibble),
                expected,
                "state ({pred},{idx}) nibble {nibble:#x}"
            );
        }
    }

    // ---------------------------------------------------------------------
    // Retail-byte block fixtures.
    //
    // The compressed bytes below are copied verbatim out of the retail
    // `audio.bag` inside `langmd.mix -> audiomd.mix`; the expected samples come
    // from a decoder transcribed from the `0x0040AA70` / `0x0040ACD0`
    // disassembly using the tables read out of the image above, cross-checked
    // against all 15 emulation vectors before the goldens were emitted. They are
    // certification references, not Rust-vs-Rust ratchets, and they run in the
    // default `cargo test` with no retail install present.
    // ---------------------------------------------------------------------

    /// `ABIRJ01A`, AUDIOMD bag offset 11,770,646, size 14,388, mono IMA,
    /// `chunk_size` 512: this is its final block — 28 x 512 bytes precede it,
    /// leaving a 52-byte tail (4-byte preamble + 48-byte payload).
    const ABIRJ01A_TAIL_BLOCK: &[u8] = &[
        0xf8, 0xff, 0x01, 0x00, 0x17, 0xbc, 0x33, 0xa1, 0x1b, 0x00, 0x90, 0x3a, 0xb5, 0x2c, 0x92,
        0x10, 0xb2, 0x9c, 0x32, 0xb3, 0x2d, 0x21, 0xcb, 0x79, 0xa1, 0x0a, 0x31, 0xc0, 0x1a, 0x23,
        0xbb, 0x4b, 0xb2, 0x1b, 0x13, 0x11, 0xd9, 0x39, 0x92, 0xa9, 0x30, 0x90, 0x0a, 0x11, 0x11,
        0x1a, 0x91, 0x09, 0x11, 0x09, 0x00, 0x00,
    ];

    const ABIRJ01A_TAIL_SAMPLES: &[i16] = &[
        -8, 7, 13, -5, -20, -6, 8, 12, 5, -5, -2, -1, 0, 1, 0, -3, 1, 9, 1, -10, -3, 3, 0, 1, 4, 7,
        3, -4, -7, -2, 2, 6, 2, -6, 0, 3, 8, 1, -6, -9, 6, 12, 2, -6, -5, -1, 7, 8, -2, -8, -5, 2,
        7, 3, -1, -5, 2, 7, 0, -4, -3, 1, 2, 3, 4, 3, -5, -8, 0, 5, 2, 1, -2, -2, 2, 2, 1, -2, -2,
        -1, 0, 1, 2, -1, 0, 1, 0, -1, -1, 0, 1, 0, 0, 0, 0, 0, 0,
    ];

    /// `GTIMESHI`, AUDIOMD bag offset 33,758,996, stereo IMA, `chunk_size` 1024:
    /// the first 44 bytes of its block 10 (8-byte preamble pair + 36 payload
    /// bytes). The first 40 bytes are a whole number of 8-byte L/R groups; the
    /// spare 4 let the same bytes serve the sub-stride case, where VERA decodes
    /// the whole groups only (`lone_short_block_decodes_whole_groups_only`).
    const GTIMESHI_STEREO_BYTES: &[u8] = &[
        0x3d, 0xda, 0x45, 0x00, 0x40, 0x18, 0x4a, 0x00, 0x07, 0x90, 0x09, 0x4a, 0x8d, 0x10, 0xa1,
        0xc6, 0x1c, 0x90, 0xa0, 0x5a, 0xa2, 0x90, 0x00, 0x98, 0x8f, 0xa8, 0x42, 0x90, 0xb8, 0xd3,
        0x97, 0x82, 0x19, 0x1a, 0x01, 0x0d, 0xa8, 0x48, 0x29, 0x8d, 0x81, 0x2b, 0xb4, 0x99,
    ];

    const GTIMESHI_STEREO_SAMPLES: &[i16] = &[
        -9667, 6208, 378, -5657, 1813, -7236, 3118, -5801, -441, -1886, -3676, 1673, -2696, -3720,
        -7153, 9027, 141, -6609, -8684, 3902, -5125, -5653, -4047, -3916, -6988, -8653, -6097,
        -7218, -10149, -5913, -13832, -7099, -6466, -10334, -21174, -11314, -23276, -17554, -25187,
        -11881, -32768, -19984, -24872, -3804, -11950, -10741, -10213, -230, -14950, -2141, -19256,
        -3878, -15341, -11774, -21273, -13209, -18038, -1462, -15097, -6199, -14206, 979, -23121,
        -13378, -21935, -15289,
    ];

    #[test]
    fn retail_mono_tail_block_matches_native_block_decode() {
        // Fed alone these are the 52 bytes the native's flush block starts
        // with, so these 97 samples are the leading 97 the native emits for it
        // (the rest come from the stale buffer tail, covered separately).
        let out = decode_blocks(ABIRJ01A_TAIL_BLOCK, 1, 512);
        // 1 preamble sample + 48 payload bytes x 2 nibbles.
        assert_eq!(out.len(), 97);
        // 0x0040AB0C emits the preamble predictor as the block's first sample.
        assert_eq!(
            out[0],
            i16::from_le_bytes([ABIRJ01A_TAIL_BLOCK[0], ABIRJ01A_TAIL_BLOCK[1]])
        );
        assert_eq!(out.as_slice(), ABIRJ01A_TAIL_SAMPLES);
    }

    #[test]
    fn retail_stereo_block_matches_native_interleave() {
        let aligned = &GTIMESHI_STEREO_BYTES[..40];
        let out = decode_blocks(aligned, 2, 1024);
        // 1 preamble frame + 4 groups x 8 frames = 33 frames = 66 samples.
        assert_eq!(out.len(), 66);
        assert_eq!(out[0], i16::from_le_bytes([aligned[0], aligned[1]]));
        assert_eq!(out[1], i16::from_le_bytes([aligned[4], aligned[5]]));
        // Left and right differ in every frame here, so a swapped or collapsed
        // interleave cannot pass.
        assert!(out.as_chunks::<2>().0.iter().all(|f| f[0] != f[1]));
        assert_eq!(out.as_slice(), GTIMESHI_STEREO_SAMPLES);
        // The negative saturation clamp is exercised by these retail bytes.
        assert!(out.contains(&-32768));
    }

    #[test]
    fn ragged_stride_drops_the_block_like_the_native() {
        // The over-read failure is decided by the STRIDE, not by where the data
        // ends: the pump always presents exactly `block_align` bytes, so
        // `0x0040AB4E`'s rounded-up group count only overshoots when
        // `(block_align - 4*channels) % (4*channels) != 0`. Stride 13 mono
        // leaves a 9-byte payload — two whole groups plus one byte — so the
        // native reads a third group past the block and `0x0040ABD4 SETZ`
        // reports failure, dropping it.
        let ragged = vec![0u8; 26];
        assert!(decode_blocks(&ragged, 1, 13).is_empty());
        // Neither retail stride is ragged, so this is unreachable on retail
        // data: (512-4) % 4 == 0 and (1024-8) % 8 == 0.
        assert_eq!((512 - 4) % 4, 0);
        assert_eq!((1024 - 8) % 8, 0);
    }

    #[test]
    fn end_of_stream_flush_pads_from_the_previous_block_like_the_native() {
        // `FUN_00409880`'s NULL-source call at `0x00409B41` makes
        // `Audio__DecodeCompressedBlock` force `+0x84 = 0` (`0x00409EBE`), so
        // the block decoder still sees a full stride. Every fill cycle writes
        // the input buffer from offset 0, so the bytes past a short tail are
        // the previous block's bytes at those same offsets.
        let block0: Vec<u8> = vec![
            0x64, 0x00, 0x05, 0x00, 0x3c, 0x91, 0x27, 0x08, 0xa5, 0x1f, 0x60, 0xd3,
        ];
        let tail: Vec<u8> = vec![0x10, 0x00, 0x02, 0x00, 0x8f, 0x41];

        let mut data = block0.clone();
        data.extend_from_slice(&tail);
        let out = decode_blocks(&data, 1, 12);

        // Two whole blocks' worth of output, not one and a fragment.
        assert_eq!(out.len(), 34, "the short tail still decodes a full stride");

        // The flush block is byte-for-byte `tail ++ block0[6..12]`.
        let mut expected_buffer = tail.clone();
        expected_buffer.extend_from_slice(&block0[6..]);
        assert_eq!(expected_buffer.len(), 12);
        assert_eq!(
            &out[17..],
            decode_blocks(&expected_buffer, 1, 12).as_slice()
        );
    }

    #[test]
    fn retail_sound_shapes_decode_the_native_block_count() {
        // Sample counts measured against gamemd's pipeline: every sound decodes
        // ceil(size / stride) blocks of 1 + (stride - 4*ch)/(4*ch) * 8 frames,
        // because the end-of-stream flush presents a full stride.
        //
        // ABIRJ01A: 14,388 bytes, mono, stride 512 -> 29 blocks x 1017 samples.
        let mut mono = vec![0x11u8; 14_388];
        for start in (0..mono.len()).step_by(512) {
            mono[start..start + 4].fill(0);
        }
        assert_eq!(decode_blocks(&mono, 1, 512).len(), 29 * 1017);

        // GREXSELB: 41,212 bytes, stereo, stride 1024 -> 41 blocks x 1017
        // frames = 83,394 samples. Its 252-byte tail is the corpus's only block
        // whose *real* payload is ragged; padded to the stride it is not.
        let mut stereo = vec![0x11u8; 41_212];
        for start in (0..stereo.len()).step_by(1024) {
            stereo[start..start + 8].fill(0);
        }
        assert_eq!(decode_blocks(&stereo, 2, 1024).len(), 41 * 1017 * 2);
    }

    #[test]
    fn lone_short_block_decodes_whole_groups_only() {
        // VERA-internal: a sound shorter than one stride is flushed against a
        // buffer this sound never filled, so there is nothing to reproduce. We
        // decode the whole groups it carries. 44 bytes stereo = 8 preamble + 36
        // payload -> 4 whole 8-byte groups, the spare 4 bytes dropped.
        let out = decode_blocks(GTIMESHI_STEREO_BYTES, 2, 1024);
        assert_eq!(out.len(), 66);
        assert_eq!(out.as_slice(), GTIMESHI_STEREO_SAMPLES);
    }

    #[test]
    fn invalid_preamble_stops_the_stream_like_the_native() {
        // Two 12-byte mono blocks; the second declares step_index 0x59, one past
        // the native's `CMP EDX,0x58 / JG` limit at 0x0040AAEE.
        let mut data = vec![0x00, 0x00, 0x00, 0x00];
        data.extend_from_slice(&[0x11; 8]);
        data.extend_from_slice(&[0x00, 0x00, 0x59, 0x00]);
        data.extend_from_slice(&[0x11; 8]);
        let out = decode_blocks(&data, 1, 12);
        assert_eq!(out.len(), 17, "first block only: 1 + 8 bytes x 2 nibbles");

        // A nonzero reserved byte rejects just the same (0x0040AAF9).
        let mut data = vec![0x00, 0x00, 0x00, 0x00];
        data.extend_from_slice(&[0x11; 8]);
        data.extend_from_slice(&[0x00, 0x00, 0x00, 0x01]);
        data.extend_from_slice(&[0x11; 8]);
        assert_eq!(decode_blocks(&data, 1, 12).len(), 17);
    }

    #[test]
    fn zero_block_align_decodes_nothing() {
        // `FUN_00409C40` copies the stride into the input-buffer size, so a zero
        // stride leaves `0x0040AAAD` with no bytes and the sound stays silent.
        assert!(decode_blocks(ABIRJ01A_TAIL_BLOCK, 1, 0).is_empty());
        assert!(decode_blocks(ABIRJ01A_TAIL_BLOCK, 1, 3).is_empty());
    }

    #[test]
    fn multi_block_state_restarts_from_each_preamble() {
        // Two identical 12-byte blocks must decode to two identical halves: the
        // native reloads predictor and step index from every preamble
        // (0x0040AAC9..0x0040AAEB) rather than carrying state across blocks.
        let mut block = vec![0x64, 0x00, 0x05, 0x00];
        block.extend_from_slice(&[0x3c, 0x91, 0x27, 0x08, 0xa5, 0x1f, 0x60, 0xd3]);
        let mut data = block.clone();
        data.extend_from_slice(&block);
        let out = decode_blocks(&data, 1, 12);
        assert_eq!(out.len(), 34);
        assert_eq!(&out[..17], &out[17..]);
        assert_eq!(out[0], 100);
    }

    #[test]
    fn step_index_walks_and_clamps_at_both_ends() {
        // The index moves by INDEX_ADJUST[nibble & 7] and clamps to 0..=0x58
        // (0x0040AD41 `JNS` and 0x0040AD4E `CMP EAX,0x58`).
        let mut state = ImaAdpcmState::new();
        state.decode_nibble(0x0); // adjust -1 from 0 -> clamps to 0
        assert_eq!(state.index, 0);
        state.decode_nibble(0x7); // +8
        assert_eq!(state.index, 8);
        state.decode_nibble(0xF); // sign bit ignored by the adjust lookup: +8
        assert_eq!(state.index, 16);
        state.set_state(0, MAX_STEP_INDEX);
        state.decode_nibble(0x7);
        assert_eq!(state.index, MAX_STEP_INDEX, "upper clamp");
        assert_eq!(STEP_TABLE[MAX_STEP_INDEX as usize], 32767);
    }

    #[test]
    fn nibble_run_decodes_low_nibble_first() {
        let mut state = ImaAdpcmState::new();
        let mut out: Vec<i16> = Vec::new();
        // 0x70: low nibble 0 (delta 0), high nibble 7 (delta +11 at step 7).
        decode_nibbles(&[0x70], &mut state, &mut out);
        assert_eq!(out, vec![0, 11]);
    }
}
