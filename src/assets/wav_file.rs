//! Borrowed RIFF/WAVE metadata shared by playback and the retail music list.
//!
//! Native `408610` supplies the descriptor and declared data length to `408560`;
//! Theme catalog scan `7207F0` stores the resulting whole seconds. Its IMA rate
//! is deliberately different from both the WAV average rate and playback length.

#[derive(Debug, Clone, Copy)]
pub(crate) struct WavFile<'a> {
    pub(crate) format_tag: u16,
    pub(crate) channels: u16,
    pub(crate) sample_rate: u32,
    pub(crate) block_align: u16,
    pub(crate) bits_per_sample: u16,
    pub(crate) declared_data_len: u32,
    pub(crate) data: &'a [u8],
}

impl<'a> WavFile<'a> {
    /// Preserve the playback parser's first data-after-fmt choice, bounded data
    /// slice and word-aligned chunk walk. Malformed short fmt chunks fail safely.
    pub(crate) fn parse(bytes: &'a [u8]) -> Option<Self> {
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
    pub(crate) fn native_duration_seconds(&self) -> Option<u32> {
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
