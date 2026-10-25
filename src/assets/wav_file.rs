//! Borrowed RIFF/WAVE metadata shared by playback and the retail music list,
//! and the payload decoder both playback paths read it through.
//!
//! Native `408610` supplies the descriptor and declared data length to `408560`;
//! Theme catalog scan `7207F0` stores the resulting whole seconds. Its IMA rate
//! is deliberately different from both the WAV average rate and playback length.

use std::ops::Range;

use crate::assets::ima_adpcm::ImaBlockDecoder;

#[derive(Debug, Clone, Copy)]
pub struct WavFile<'a> {
    pub format_tag: u16,
    pub channels: u16,
    pub sample_rate: u32,
    pub block_align: u16,
    pub bits_per_sample: u16,
    pub(crate) declared_data_len: u32,
    pub data: &'a [u8],
    /// File offset of `data`.
    data_start: usize,
}

impl<'a> WavFile<'a> {
    /// Preserve the playback parser's first data-after-fmt choice, bounded data
    /// slice and word-aligned chunk walk. Malformed short fmt chunks fail safely.
    pub fn parse(bytes: &'a [u8]) -> Option<Self> {
        if bytes.len() < 44 || &bytes[..4] != b"RIFF" || &bytes[8..12] != b"WAVE" {
            return None;
        }
        let mut offset = 12_usize;
        let mut format = None;
        while offset.checked_add(8)? <= bytes.len() {
            let size = u32::from_le_bytes(bytes[offset + 4..offset + 8].try_into().ok()?);
            let start = offset + 8;
            let end = start.checked_add(size as usize)?;
            match &bytes[offset..offset + 4] {
                b"fmt " if end <= bytes.len() => {
                    let fmt = bytes.get(start..end)?;
                    let word = |i: usize| -> Option<u16> {
                        Some(u16::from_le_bytes(fmt.get(i..i + 2)?.try_into().ok()?))
                    };
                    format = Some((
                        word(0)?,
                        word(2)?,
                        u32::from_le_bytes(fmt.get(4..8)?.try_into().ok()?),
                        word(12)?,
                        word(14)?,
                    ));
                }
                b"data" => {
                    if let Some((format_tag, channels, sample_rate, block_align, bits_per_sample)) =
                        format
                    {
                        return Some(Self {
                            format_tag,
                            channels,
                            sample_rate,
                            block_align,
                            bits_per_sample,
                            declared_data_len: size,
                            data: &bytes[start..end.min(bytes.len())],
                            data_start: start,
                        });
                    }
                }
                _ => {}
            }
            offset = end.checked_add((size & 1) as usize)?;
        }
        None
    }

    /// Original `408751..4087C3` recomputes the descriptor rate, overwriting
    /// nAvgBytesPerSec. IMA uses (2 * channels * sample_rate) / 4; PCM uses
    /// (bits / 8) * channels * sample_rate. `408560` divides data*1000 by that
    /// rate, then `7208D3..7208EC` divides by1000 before storing float seconds.
    /// Ordinary positive audio sizes/rates are covered by sound_theme_metadata.
    pub fn native_duration_seconds(&self) -> Option<u32> {
        let sample_bytes = match self.format_tag {
            1 => u32::from(self.bits_per_sample >> 3),
            0x11 => 2,
            _ => return None,
        };
        let mut rate = sample_bytes
            .checked_mul(u32::from(self.channels))?
            .checked_mul(self.sample_rate)?;
        if self.format_tag == 0x11 {
            rate /= 4;
        }
        if rate == 0 {
            return Some(0);
        }
        let milliseconds = u64::from(self.declared_data_len) * 1000 / u64::from(rate);
        u32::try_from(milliseconds / 1000).ok()
    }

    /// Where [`Self::data`] lies in the parsed file bytes.
    pub fn data_range(&self) -> Range<usize> {
        self.data_start..self.data_start + self.data.len()
    }
}

/// PCM bytes one [`WavPcmCursor::decode_next`] call converts: a VERA streaming
/// granularity, near one retail IMA block's 1017 frames.
const PCM_RUN_BYTES: usize = 4096;

/// A WAV payload decoded a piece per call into interleaved 16-bit samples in
/// the file's own channel layout, so a long file can stream: one IMA ADPCM
/// block per call, or one run of 8- or 16-bit PCM (8-bit widened as
/// `(byte - 128) << 8`).
///
/// `WAV__ParseHeader @ 0x00408610` maps `wFormatTag == 0x11` onto the audio
/// format descriptor the bag index fills, with the fmt chunk's `nBlockAlign`
/// (`psVar6[6]`) as the block stride; `FUN_00409C40 @ 0x00409C40` then installs
/// the one block decoder, `IMA_ADPCM__DecodeBlock @ 0x0040AA70`, so WAV and bag
/// IMA share `ima_adpcm` here as they share it natively.
pub struct WavPcmCursor {
    format: PayloadFormat,
    /// PCM payload offset of the next run.
    pos: usize,
}

enum PayloadFormat {
    Ima(ImaBlockDecoder),
    Pcm16,
    Pcm8,
}

impl WavPcmCursor {
    /// `None` for a payload playback does not decode: a format tag other than
    /// PCM or IMA ADPCM, or PCM other than 8 or 16 bits.
    pub fn new(wav: &WavFile<'_>) -> Option<Self> {
        match wav.format_tag {
            0x11 => Some(Self {
                format: PayloadFormat::Ima(ImaBlockDecoder::new(
                    wav.channels,
                    u32::from(wav.block_align),
                )),
                pos: 0,
            }),
            1 => Self::pcm(wav.bits_per_sample),
            _ => None,
        }
    }

    /// A cursor over raw PCM of `bits_per_sample`; `None` unless 8 or 16.
    pub fn pcm(bits_per_sample: u16) -> Option<Self> {
        let format = match bits_per_sample {
            16 => PayloadFormat::Pcm16,
            8 => PayloadFormat::Pcm8,
            _ => return None,
        };
        Some(Self { format, pos: 0 })
    }

    /// Append the next piece of `data` — the whole payload, passed again on
    /// every call — to `out`; `false`, appending nothing, once no whole sample
    /// remains.
    pub fn decode_next(&mut self, data: &[u8], out: &mut Vec<i16>) -> bool {
        let sample_bytes = match &mut self.format {
            PayloadFormat::Ima(decoder) => return decoder.decode_next(data, out),
            PayloadFormat::Pcm16 => 2,
            PayloadFormat::Pcm8 => 1,
        };
        let start = self.pos.min(data.len());
        let run = &data[start..(start + PCM_RUN_BYTES).min(data.len())];
        let run = &run[..run.len() / sample_bytes * sample_bytes];
        if run.is_empty() {
            return false;
        }
        self.pos = start + run.len();
        if sample_bytes == 2 {
            out.extend(
                run.as_chunks::<2>()
                    .0
                    .iter()
                    .map(|&c| i16::from_le_bytes(c)),
            );
        } else {
            out.extend(run.iter().map(|&b| (i16::from(b) - 128) << 8));
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn metadata_duration_matches_original_instructions_and_retail_headers() {
        let fixture: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
            "tools/storage_oracle/sound_theme_metadata.json",
        ))
        .unwrap();
        let cases = fixture["cases"].as_array().unwrap();
        assert_eq!(cases.len(), 24);
        assert_eq!(
            cases.iter().filter(|case| case["kind"] == "retail").count(),
            8
        );
        for case in cases {
            let header = case["header_hex"].as_str().unwrap();
            let mut bytes: Vec<u8> = (0..header.len())
                .step_by(2)
                .map(|i| u8::from_str_radix(&header[i..i + 2], 16).unwrap())
                .collect();
            // Supply the declared ordinary payload; no decode or file I/O is
            // involved. Original fmt/fact bytes remain exactly as recorded.
            bytes.resize(
                bytes.len() + case["data_bytes"].as_u64().unwrap() as usize,
                0,
            );
            let wav = WavFile::parse(&bytes).unwrap();
            assert_eq!(wav.format_tag, case["format_tag"].as_u64().unwrap() as u16);
            assert_eq!(wav.channels, case["channels"].as_u64().unwrap() as u16);
            assert_eq!(
                wav.sample_rate,
                case["sample_rate"].as_u64().unwrap() as u32
            );
            assert_eq!(
                wav.block_align,
                case["block_align"].as_u64().unwrap() as u16
            );
            assert_eq!(
                wav.bits_per_sample,
                case["bits_per_sample"].as_u64().unwrap() as u16
            );
            assert_eq!(
                wav.declared_data_len,
                case["data_bytes"].as_u64().unwrap() as u32
            );
            assert_eq!(
                wav.native_duration_seconds(),
                Some(case["seconds"].as_u64().unwrap() as u32),
                "{}",
                case["name"]
            );
        }
    }
}
