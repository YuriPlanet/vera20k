//! Ogg Opus (`.opus`) reader for VERA's audio overrides.
//!
//! VERA extension, no native counterpart: gamemd plays only WAV, AUD and the
//! `audio.bag` IMA ADPCM/PCM entries. A package that ships `<name>.opus`
//! beside (or instead of) those files has it played in their place; see
//! docs/opus-audio.md. Retail data holds no `.opus` file, so nothing here runs
//! for a stock game.
//!
//! Decoder choice: `opus-pure` (BSD-3-Clause, pure Rust, no dependencies, no
//! build script). Of the pure-Rust decoders on crates.io it is the one that
//! ships the whole RFC 7845 container path this needs (Ogg demux with page
//! CRCs, `OpusHead`/`OpusTags`, output gain, pre-skip and end-trim), and it is
//! checked against C libopus 1.6.1: all 12 RFC 6716/8251 vectors at mono and
//! stereo output, SILK bit-exact, CELT and hybrid within float rounding, in CI
//! on Linux, macOS and Windows. Its SIMD kernels sit behind `target_arch`
//! with scalar fallbacks, so the browser (wasm32) build takes the scalar path.
//! `opus-decoder` (RFC 8251 too) has no Ogg layer, `symphonia-opus` pulls in
//! symphonia-core, and `opus1` needs a newer compiler than the repository pins.
//!
//! Opus always decodes here at 48 kHz, its native rate; the players take the
//! rate with the samples, as they do a WAV's. Mono and stereo streams (mapping
//! family 0) are read; a surround stream is refused.

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

use std::io::Read;
use std::ops::Deref;

use opus_pure::{MAX_PACKET_SAMPLES, OggOpusReader, OggPacket, OpusDecoder, Trim};

/// The rate every override decodes at.
pub const OPUS_SAMPLE_RATE: u32 = 48_000;

/// The file that overrides the sound or track `name`: a trailing `.wav` or
/// `.aud` (any case) gives way to `.opus`, any other name gains it.
/// `igisea` → `igisea.opus`, `taunts\tauam01.wav` → `taunts\tauam01.opus`.
pub fn override_name(name: &str) -> String {
    let stem = name
        .len()
        .checked_sub(4)
        .filter(|&dot| {
            name.get(dot..).is_some_and(|extension| {
                extension.eq_ignore_ascii_case(".wav") || extension.eq_ignore_ascii_case(".aud")
            })
        })
        .and_then(|dot| name.get(..dot))
        .unwrap_or(name);
    format!("{stem}.opus")
}

/// A whole decoded file: 16-bit PCM, interleaved when stereo, at
/// [`OPUS_SAMPLE_RATE`].
pub struct OpusAudio {
    pub samples_i16: Vec<i16>,
    /// 1 or 2.
    pub channels: u16,
}

/// Decode a whole Ogg Opus file; `None` when it is not one, is surround, or
/// holds no audio.
pub fn decode(data: &[u8]) -> Option<OpusAudio> {
    let mut stream = OpusStream::open(data)?;
    let mut samples_i16 = Vec::new();
    while stream.decode_next(&mut samples_i16) {}
    (!samples_i16.is_empty()).then_some(OpusAudio {
        samples_i16,
        channels: stream.channels(),
    })
}

/// Whole seconds of audio the file holds, from its last page's granule
/// position less the `OpusHead` pre-skip (RFC 7845 §4), what a WAV header's
/// length gives the Theme list. Only the head and the last page are read,
/// so a catalog scan does not page in whole songs. `None` when either is
/// missing.
pub fn duration_seconds(data: &[u8]) -> Option<u32> {
    // `OpusHead` opens the first page's only packet (RFC 7845 §3).
    let head_window = data.get(..data.len().min(MAX_PAGE_BYTES))?;
    let head = head_window.windows(8).position(|w| w == b"OpusHead")?;
    let pre_skip = u16::from_le_bytes(data.get(head + 10..head + 12)?.try_into().ok()?);
    // The last page starts within one page's length of the end; a page that
    // completes no packet carries granule -1 and is passed over.
    let tail = data.get(data.len().saturating_sub(MAX_PAGE_BYTES)..)?;
    let granule = (0..tail.len())
        .rev()
        .filter(|&at| tail[at..].starts_with(b"OggS\0"))
        .find_map(|at| {
            let granule = i64::from_le_bytes(tail.get(at + 6..at + 14)?.try_into().ok()?);
            (granule >= 0).then_some(granule)
        })?;
    let samples = granule.saturating_sub(i64::from(pre_skip)).max(0);
    u32::try_from(samples / i64::from(OPUS_SAMPLE_RATE)).ok()
}

/// The longest Ogg page: a 27-byte header, 255 lacing values and 255
/// segments of 255 bytes (RFC 3533 §6).
const MAX_PAGE_BYTES: usize = 27 + 255 + 255 * 255;

/// An Ogg Opus file decoding one packet at a time, for a song the music
/// device pulls as it plays. `B` is the file's bytes (a slice, or a share of
/// archive storage that another thread can keep).
pub struct OpusStream<B: Deref<Target = [u8]>> {
    reader: OggOpusReader<ByteReader<B>>,
    decoder: OpusDecoder,
    trim: Trim,
    channels: u16,
    packet: OggPacket,
    block: Vec<i16>,
}

impl<B: Deref<Target = [u8]>> OpusStream<B> {
    /// Read the headers; `None` when the bytes are not mono or stereo Ogg Opus.
    pub fn open(bytes: B) -> Option<Self> {
        let reader = OggOpusReader::new(ByteReader::new(bytes)).ok()?;
        let head = reader.head();
        let channels = u16::from(head.channel_count);
        if !(1..=2).contains(&channels) {
            return None;
        }
        let rate = OPUS_SAMPLE_RATE as i32;
        // `decoder` applies the header's output gain and refuses any mapping
        // family but 0.
        let decoder = head.decoder(rate).ok()?;
        let trim = Trim::new(head, rate, usize::from(channels)).ok()?;
        Some(Self {
            reader,
            decoder,
            trim,
            channels,
            packet: OggPacket::new(Vec::new(), -1, false),
            block: vec![0; MAX_PACKET_SAMPLES * usize::from(channels)],
        })
    }

    /// 1 or 2.
    pub fn channels(&self) -> u16 {
        self.channels
    }

    /// Append the next packets' audio to `out` until some is appended; false
    /// once the stream ends or a packet fails to read or decode.
    pub fn decode_next(&mut self, out: &mut Vec<i16>) -> bool {
        loop {
            if !matches!(self.reader.read_packet_into(&mut self.packet), Ok(true)) {
                return false;
            }
            let Ok(frames) =
                self.decoder
                    .decode_s16(&self.packet.data, MAX_PACKET_SAMPLES, &mut self.block)
            else {
                return false;
            };
            let Some(decoded) = self.block.get(..frames * usize::from(self.channels)) else {
                return false;
            };
            let kept = self.trim.keep(&self.packet, decoded);
            if !kept.is_empty() {
                out.extend_from_slice(kept);
                return true;
            }
        }
    }
}

/// `Read` over bytes held by value.
struct ByteReader<B> {
    bytes: B,
    position: usize,
}

impl<B> ByteReader<B> {
    fn new(bytes: B) -> Self {
        Self { bytes, position: 0 }
    }
}

impl<B: Deref<Target = [u8]>> Read for ByteReader<B> {
    fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
        let rest = self.bytes.get(self.position..).unwrap_or_default();
        let count = rest.len().min(buffer.len());
        buffer[..count].copy_from_slice(&rest[..count]);
        self.position += count;
        Ok(count)
    }
}

#[cfg(all(test, tests_assets))]
mod tests {
    use super::*;

    const FIXTURE: &str = "tests/fixtures/audio/sine440.opus";

    /// ffmpeg's libopus, 0.2 s of a 440 Hz sine (command in the fixture's README):
    /// 9,600 samples at 48 kHz after the 312-sample pre-skip comes off.
    #[test]
    fn the_fixture_decodes_to_its_granule_length_at_48_khz() {
        let audio = decode(crate::test_fixture::bytes(FIXTURE)).expect("Ogg Opus decodes");
        assert_eq!(audio.channels, 1);
        assert_eq!(audio.samples_i16.len(), 9_600);
        assert!(
            audio
                .samples_i16
                .iter()
                .any(|&sample| sample.unsigned_abs() > 1_000)
        );
    }

    /// The length comes from the last page's granule less the pre-skip: the
    /// fixture's 9,912 - 312 samples are 0.2 s, and a granule moved to three
    /// seconds and one sample past the pre-skip reads as 3.
    #[test]
    fn duration_reads_the_last_granule_less_the_pre_skip() {
        let mut bytes = crate::test_fixture::bytes(FIXTURE).to_vec();
        assert_eq!(duration_seconds(&bytes), Some(0));
        let last = bytes.windows(4).rposition(|w| w == b"OggS").unwrap();
        let granule = 3 * 48_000 + 312 + 1_i64;
        bytes[last + 6..last + 14].copy_from_slice(&granule.to_le_bytes());
        assert_eq!(duration_seconds(&bytes), Some(3));
        assert_eq!(duration_seconds(b"RIFF"), None);
    }

    #[test]
    fn only_ogg_opus_bytes_open() {
        assert!(decode(b"RIFF\0\0\0\0WAVEfmt ").is_none());
        let fixture = crate::test_fixture::bytes(FIXTURE);
        // A truncated file refuses or keeps what it read, without panicking.
        let truncated = decode(&fixture[..fixture.len() / 2]);
        assert!(truncated.is_none_or(|audio| audio.samples_i16.len() < 9_600));
    }

    #[test]
    fn override_names_replace_a_wav_or_aud_extension() {
        assert_eq!(override_name("igisea"), "igisea.opus");
        assert_eq!(override_name("Dummy.WAV"), "Dummy.opus");
        assert_eq!(override_name("taunts\\tauam01.wav"), "taunts\\tauam01.opus");
        assert_eq!(override_name("intro.aud"), "intro.opus");
        assert_eq!(override_name("a.b"), "a.b.opus");
    }
}
