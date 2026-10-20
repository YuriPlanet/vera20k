//! Westwood .aud audio file parser and IMA ADPCM decoder.
//!
//! .aud files store compressed audio used for music and sound effects in RA2/YR.
//! Two compression formats are supported:
//! - Format 99: IMA ADPCM (4-bit per sample, used by most music tracks)
//! - Format 1: Westwood Compressed (2/4-bit adaptive, older format)
//!
//! The file is divided into chunks, each with a small header and compressed payload.
//! Decoding produces 16-bit signed PCM samples.
//!
//! ## Reachability in gamemd
//!
//! The DEAF-chunk container this file walks has **no decoder in `gamemd.exe`**:
//! the only `.aud` filenames the binary references are TEXT1–3.AUD (score-screen
//! narration), no code in the audio path compares the `0x0000DEAF` chunk magic
//! (the image's one `CMP word ptr [...+0x4],0xDEAF` is at `0x005F1D2F`, inside
//! the serial/null-modem packet framer `FUN_005F1C60`), and the image's
//! single IMA decoder (`IMA_ADPCM__DecodeSample @ 0x0040ACD0`, reached only via
//! the block decoder `IMA_ADPCM__DecodeBlock @ 0x0040AA70`) is block-framed, not
//! chunk-framed. See `docs/research/AUD_TRAILING_SAMPLE_UNREACHABLE_GHIDRA_REPORT.md`.
//! The chunk walk below is therefore VERA-internal — a reader for files the
//! original never decodes — while the nibble math it borrows from
//! `ima_adpcm` is the certified native one.
//!
//! ## Dependency rules
//! - Part of assets/ — standalone parser, no game dependencies.
//! - IMA ADPCM sample math comes from `assets::ima_adpcm`, the crate's single
//!   implementation.

use super::ima_adpcm::{ImaAdpcmState, decode_nibbles};

/// Magic value at the start of each audio chunk.
const CHUNK_MAGIC: u32 = 0x0000DEAF;

/// Parsed .aud file header.
#[derive(Debug, Clone)]
pub struct AudHeader {
    /// Sample rate in Hz (e.g. 22050).
    pub sample_rate: u16,
    /// Size of the compressed data in bytes.
    pub data_size: u32,
    /// Size of the decompressed output in bytes.
    pub output_size: u32,
    /// Flags: bit 0 = stereo, bit 1 = 16-bit samples.
    pub flags: u8,
    /// Compression format: 1 = Westwood Compressed, 99 = IMA ADPCM.
    pub format: u8,
}

impl AudHeader {
    /// Whether the audio is stereo (2 channels).
    pub fn is_stereo(&self) -> bool {
        self.flags & 0x01 != 0
    }

    /// Whether samples are 16-bit (vs 8-bit).
    pub fn is_16bit(&self) -> bool {
        self.flags & 0x02 != 0
    }

    /// Number of audio channels.
    pub fn channels(&self) -> u16 {
        if self.is_stereo() { 2 } else { 1 }
    }
}

/// AUD header size in bytes.
const HEADER_SIZE: usize = 12;

/// Chunk header size in bytes (compressed_size u16 + output_size u16 + magic u32 = 8).
const CHUNK_HEADER_SIZE: usize = 8;

/// Parse an .aud file header from raw bytes.
/// Returns None if the data is too short.
pub fn parse_header(data: &[u8]) -> Option<AudHeader> {
    if data.len() < HEADER_SIZE {
        return None;
    }
    let sample_rate: u16 = u16::from_le_bytes([data[0], data[1]]);
    let data_size: u32 = u32::from_le_bytes([data[2], data[3], data[4], data[5]]);
    let output_size: u32 = u32::from_le_bytes([data[6], data[7], data[8], data[9]]);
    let flags: u8 = data[10];
    let format: u8 = data[11];
    Some(AudHeader {
        sample_rate,
        data_size,
        output_size,
        flags,
        format,
    })
}

/// Decode an entire .aud file into 16-bit signed PCM samples.
///
/// Returns `(header, samples)` on success.
/// Returns None if the file is malformed or uses an unsupported format.
pub fn decode_aud(data: &[u8]) -> Option<(AudHeader, Vec<i16>)> {
    let header: AudHeader = parse_header(data)?;
    if header.format != 99 && header.format != 1 {
        log::warn!("Unsupported .aud format: {}", header.format);
        return None;
    }

    let estimated_samples: usize = header.output_size as usize / 2;
    let mut samples: Vec<i16> = Vec::with_capacity(estimated_samples);
    let mut offset: usize = HEADER_SIZE;
    let mut state = ImaAdpcmState::new();

    while offset + CHUNK_HEADER_SIZE <= data.len() {
        let compressed_size: u16 = u16::from_le_bytes([data[offset], data[offset + 1]]);
        let chunk_output_size: u16 = u16::from_le_bytes([data[offset + 2], data[offset + 3]]);
        let magic: u32 = u32::from_le_bytes([
            data[offset + 4],
            data[offset + 5],
            data[offset + 6],
            data[offset + 7],
        ]);

        if magic != CHUNK_MAGIC {
            log::warn!(
                "Invalid chunk magic at offset {}: 0x{:08X} (expected 0x{:08X})",
                offset,
                magic,
                CHUNK_MAGIC
            );
            break;
        }

        offset += CHUNK_HEADER_SIZE;
        let chunk_end: usize = (offset + compressed_size as usize).min(data.len());
        let chunk_data: &[u8] = &data[offset..chunk_end];

        let before: usize = samples.len();
        if header.format == 99 {
            decode_nibbles(chunk_data, &mut state, &mut samples);
        } else {
            decode_ws_compressed_chunk(chunk_data, &mut samples);
        }

        // The chunk header also declares how many OUTPUT bytes the chunk should
        // produce. Decoding stays input-driven — every nibble present is
        // decoded and nothing is padded to the declaration — because gamemd has
        // no AUD chunk decoder to match (see the module header), and because the
        // only retail files where the two disagree declare exactly one 16-bit
        // sample more than their nibbles can produce: intro.aud, wipe.aud,
        // efficien.aud and mouseon.aud, none of which the original ever loads.
        // A disagreement is still worth seeing when it happens.
        let produced: usize = (samples.len() - before) * 2;
        if produced != chunk_output_size as usize {
            log::debug!(
                "AUD chunk at {offset}: declared {chunk_output_size} output bytes, \
                 {produced} produced from {compressed_size} compressed bytes"
            );
        }

        offset = chunk_end;
    }

    Some((header, samples))
}

/// Decode a Westwood Compressed (format 1) chunk.
/// This is a simpler 2/4-bit adaptive scheme used by older .aud files.
/// NO-DIFF (GSI-02.15) — format 1 is dead for stock assets. An `asset_scan`
/// sweep over 57 archives and 13,348 entries finds all 23 retail `.aud` files
/// are IMA ADPCM, so no retail bytes exist to test the format-1 path against and
/// no stock playback can reach it. It stays untested because it is unreachable,
/// not because the coverage was skipped.
fn decode_ws_compressed_chunk(data: &[u8], out: &mut Vec<i16>) {
    // Westwood compressed format: each byte is either a 2-bit or 4-bit encoded delta.
    // For simplicity and because RA2 music primarily uses format 99 (IMA ADPCM),
    // this is a basic implementation.
    let mut sample: i16 = 0;
    let mut i: usize = 0;
    while i < data.len() {
        let byte: u8 = data[i];
        i += 1;

        let count_code: u8 = byte >> 6;
        match count_code {
            // 2-bit delta: 6 values packed in current + next 2 bytes.
            0b00 => {
                // Skip count (byte & 0x3F) samples of silence.
                let skip: usize = (byte & 0x3F) as usize;
                for _ in 0..skip {
                    out.push(sample);
                }
            }
            0b01 => {
                // Low 6 bits = count, next N bytes are raw 8-bit unsigned deltas.
                let count: usize = (byte & 0x3F) as usize;
                for _ in 0..count {
                    if i >= data.len() {
                        break;
                    }
                    let raw: u8 = data[i];
                    i += 1;
                    // Treat as signed offset from 128 (unsigned bias).
                    sample = ((raw as i16) - 128) * 256;
                    out.push(sample);
                }
            }
            0b10 => {
                // 4-bit deltas: low 6 bits = count, each byte has 2 nibbles.
                let count: usize = (byte & 0x3F) as usize;
                for _ in 0..count {
                    if i >= data.len() {
                        break;
                    }
                    let raw: u8 = data[i];
                    i += 1;
                    let lo: i16 = ((raw & 0x0F) as i16) - 8;
                    let hi: i16 = (((raw >> 4) & 0x0F) as i16) - 8;
                    sample = sample.saturating_add(lo * 16);
                    out.push(sample);
                    sample = sample.saturating_add(hi * 16);
                    out.push(sample);
                }
            }
            _ => {
                // 0b11: raw 8-bit sample (single).
                sample = (((byte & 0x3F) as i16) - 32) * 512;
                out.push(sample);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_header_valid() {
        // sample_rate=22050 (0x5622), data_size=1000, output_size=4000, flags=2, format=99
        let mut data: Vec<u8> = Vec::new();
        data.extend_from_slice(&22050u16.to_le_bytes()); // sample_rate
        data.extend_from_slice(&1000u32.to_le_bytes()); // data_size
        data.extend_from_slice(&4000u32.to_le_bytes()); // output_size
        data.push(0x02); // flags (16-bit)
        data.push(99); // format (IMA ADPCM)

        let hdr: AudHeader = parse_header(&data).expect("should parse");
        assert_eq!(hdr.sample_rate, 22050);
        assert_eq!(hdr.data_size, 1000);
        assert_eq!(hdr.output_size, 4000);
        assert!(!hdr.is_stereo());
        assert!(hdr.is_16bit());
        assert_eq!(hdr.channels(), 1);
        assert_eq!(hdr.format, 99);
    }

    #[test]
    fn test_decode_aud_invalid_format() {
        let mut data: Vec<u8> = Vec::new();
        data.extend_from_slice(&22050u16.to_le_bytes());
        data.extend_from_slice(&0u32.to_le_bytes());
        data.extend_from_slice(&0u32.to_le_bytes());
        data.push(0x02);
        data.push(50); // unsupported format
        assert!(decode_aud(&data).is_none());
    }

    /// The four retail AUDs whose final chunk declares `4*compressed + 2` —
    /// intro.aud, wipe.aud, efficien.aud, mouseon.aud — declare one 16-bit
    /// sample more than their nibbles can produce. Decoding stays input-driven,
    /// so we end one sample short of the declaration rather than padding.
    /// gamemd never loads any of them (it has no AUD chunk decoder at all), so
    /// there is no native behavior to match here; this pins the choice.
    #[test]
    fn decode_aud_is_input_driven_when_a_chunk_overdeclares_its_output() {
        let mut data: Vec<u8> = Vec::new();
        data.extend_from_slice(&22050u16.to_le_bytes());
        data.extend_from_slice(&4u32.to_le_bytes()); // data_size
        data.extend_from_slice(&18u32.to_le_bytes()); // output_size (declared)
        data.push(0x02);
        data.push(99);
        // One chunk: 4 compressed bytes but 4*4 + 2 = 18 declared output bytes.
        data.extend_from_slice(&4u16.to_le_bytes());
        data.extend_from_slice(&18u16.to_le_bytes());
        data.extend_from_slice(&CHUNK_MAGIC.to_le_bytes());
        data.extend_from_slice(&[0x00, 0x11, 0x22, 0x33]);

        let (hdr, samples) = decode_aud(&data).expect("should decode");
        assert_eq!(hdr.output_size, 18);
        // 4 bytes x 2 nibbles = 8 samples = 16 bytes: one sample short of 18.
        assert_eq!(samples.len(), 8);
    }

    #[test]
    fn decode_aud_carries_adpcm_state_across_chunks() {
        // The decoder state is created once per file, so splitting the same
        // nibbles across two chunks must give the same samples as one chunk.
        let payload: [u8; 8] = [0x37, 0x59, 0x2b, 0xc4, 0x16, 0x8a, 0x71, 0x03];
        let mut header: Vec<u8> = Vec::new();
        header.extend_from_slice(&22050u16.to_le_bytes());
        header.extend_from_slice(&(payload.len() as u32).to_le_bytes());
        header.extend_from_slice(&((payload.len() as u32) * 4).to_le_bytes());
        header.push(0x02);
        header.push(99);

        let chunk = |bytes: &[u8]| {
            let mut c: Vec<u8> = Vec::new();
            c.extend_from_slice(&(bytes.len() as u16).to_le_bytes());
            c.extend_from_slice(&((bytes.len() as u16) * 4).to_le_bytes());
            c.extend_from_slice(&CHUNK_MAGIC.to_le_bytes());
            c.extend_from_slice(bytes);
            c
        };

        let mut one = header.clone();
        one.extend_from_slice(&chunk(&payload));
        let mut two = header.clone();
        two.extend_from_slice(&chunk(&payload[..4]));
        two.extend_from_slice(&chunk(&payload[4..]));

        let (_, a) = decode_aud(&one).expect("single chunk");
        let (_, b) = decode_aud(&two).expect("split chunks");
        assert_eq!(a.len(), 16);
        assert_eq!(a, b);
    }
}
