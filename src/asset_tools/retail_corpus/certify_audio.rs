//! `certify_*` AUD chunk-walk + audio.bag corpus tests.

use super::*;

use crate::assets::aud_file::{decode_aud, parse_header};

/// AUD chunk header: u16 compressed size + u16 output size + u32 magic.
const AUD_HEADER_SIZE: usize = 12;
const CHUNK_HEADER_SIZE: usize = 8;
const CHUNK_MAGIC: u32 = 0x0000_DEAF;

#[test]
#[ignore = "requires the recorded retail corpus; run the indexed retail-corpus profile"]
fn certify_aud_chunk_walk() {
    let am = required_corpus();
    let names = xcc_name_map();
    let mut failures: Vec<String> = Vec::new();
    let mut trailing_sample_files: Vec<String> = Vec::new();
    let mut total = 0usize;
    walk_sniffed(&am, |ce, data| {
        if ce.format != "aud" {
            return;
        }
        total += 1;
        let result = (|| -> Result<(), String> {
            let header = parse_header(data).ok_or("header too short")?;
            // Walk every chunk from the raw bytes: the walk must land exactly
            // on EOF and the summed chunk output sizes must equal the header's
            // declared output size — this certifies our chunk accounting
            // against the retail file's own layout.
            let mut pos = AUD_HEADER_SIZE;
            let mut summed_output: u64 = 0;
            let mut chunks: Vec<(usize, u64)> = Vec::new();
            while pos < data.len() {
                if pos + CHUNK_HEADER_SIZE > data.len() {
                    return Err(format!(
                        "truncated chunk header at {pos} (file {} bytes)",
                        data.len()
                    ));
                }
                let compressed = u16::from_le_bytes([data[pos], data[pos + 1]]) as usize;
                let output = u16::from_le_bytes([data[pos + 2], data[pos + 3]]) as u64;
                let magic = u32::from_le_bytes([
                    data[pos + 4],
                    data[pos + 5],
                    data[pos + 6],
                    data[pos + 7],
                ]);
                if magic != CHUNK_MAGIC {
                    return Err(format!("bad chunk magic {magic:#010X} at {pos}"));
                }
                summed_output += output;
                chunks.push((compressed, output));
                pos += CHUNK_HEADER_SIZE + compressed;
            }
            if pos != data.len() {
                return Err(format!(
                    "chunk walk overran EOF: ended at {pos}, file is {} bytes",
                    data.len()
                ));
            }
            if summed_output != header.output_size as u64 {
                return Err(format!(
                    "summed chunk outputs {summed_output} != header output_size {}",
                    header.output_size
                ));
            }
            // Machine-derived retail invariant (2026-07-19 corpus pass): for
            // format-99 IMA, every chunk declares output == 4*compressed —
            // EXCEPT that in 4 unique retail files the FINAL chunk declares
            // exactly 4*compressed + 2 (one extra trailing sample the nibble
            // stream cannot produce). Any other pattern is a new finding.
            let mut input_derived_output: u64 = 0;
            for (i, (compressed, output)) in chunks.iter().enumerate() {
                let expected = *compressed as u64 * 4;
                if *output == expected {
                    input_derived_output += expected;
                } else if i == chunks.len() - 1 && *output == expected + 2 {
                    input_derived_output += expected;
                    let file = names
                        .get(&ce.id)
                        .cloned()
                        .unwrap_or_else(|| format!("{:#010X}", ce.id as u32));
                    trailing_sample_files.push(format!("{} {file}", ce.archive));
                } else {
                    return Err(format!(
                        "chunk {i}/{}: output {output} vs compressed {compressed} \
                         (neither 4*c nor final-chunk 4*c+2)",
                        chunks.len()
                    ));
                }
            }
            // Our decoder is input-driven: it must consume every nibble and
            // emit exactly 2 samples per input byte.
            let (_, samples) = decode_aud(data).ok_or("decode_aud returned None")?;
            if samples.len() as u64 * 2 != input_derived_output {
                return Err(format!(
                    "decoded {} bytes != 4 * total compressed bytes {input_derived_output}",
                    samples.len() * 2
                ));
            }
            Ok(())
        })();
        if let Err(msg) = result {
            failures.push(format!(
                "{} {:#010X} ({} bytes): {msg}",
                ce.archive, ce.id as u32, ce.size
            ));
        }
    })
    .expect("read every indexed corpus entry");
    assert!(total > 0, "retail corpus contained no AUD files");
    assert!(
        failures.is_empty(),
        "aud: {} of {total} retail files failed chunk accounting:\n{}",
        failures.len(),
        failures.join("\n")
    );
    // Resolved 2026-07-19 (see docs/research/
    // AUD_TRAILING_SAMPLE_UNREACHABLE_GHIDRA_REPORT.md): the original engine
    // never decodes these four files (installer / RA2-era shell leftovers;
    // its music is WAV and its SFX come from audio.bag), so the declared
    // trailing sample is unobservable and our input-driven decoder needs no
    // change. Recorded here so a future consumer of these files knows.
    println!("RECORD: retail AUDs declaring a final trailing sample with no input nibbles:");
    for line in &trailing_sample_files {
        println!("RECORD:   {line}");
    }
}

#[test]
#[ignore = "requires the recorded retail corpus; run the indexed retail-corpus profile"]
fn certify_bag_adpcm_block_invariants() {
    // Value-parity certification for bag IMA-ADPCM decode, block level.
    // The original engine's block decoder (see docs/research/
    // ADPCM_NIBBLE_VALUE_CERTIFICATION_GHIDRA_REPORT.md):
    // - REJECTS a block whose per-channel preamble has step_index > 0x58 or a
    //   nonzero reserved byte (0x0040AAEE / 0x0040AAF9) — so equivalence needs:
    //   no retail preamble is invalid;
    // - would REJECT a block whose payload is not a whole number of
    //   4*channels-byte groups (mono 0x0040AB4E, stereo 0x0040ACB3, both then
    //   SETZ) — but never sees one: Audio__DecodeCompressedBlock hands the
    //   decoder exactly block_align bytes on every call (only four writes touch
    //   +0x80/+0x84, and case 0 always leaves +0x84 at 0 — see
    //   ADPCM_NIBBLE_VALUE_CERTIFICATION_GHIDRA_REPORT.md §2.1), and both retail
    //   strides are whole. A short final tail is padded from the previous
    //   block's bytes still in the input buffer, which decode_blocks now
    //   reproduces.
    // The ragged-tail sweep below is therefore a corpus fact, not a divergence:
    // it records which entries carry a *real* tail that is not group-aligned,
    // which is what the pre-2026-09-03 reading mistook for a dropped block.
    const KNOWN_RAGGED: &[&str] = &["GREXSELB"];
    let am = required_corpus();
    let mut failures: Vec<String> = Vec::new();
    let mut ragged_known: Vec<String> = Vec::new();
    let mut ima_entries = 0usize;
    let mut stereo_entries = 0usize;
    let mut short_tail_entries = 0usize;
    let mut shorter_than_stride_entries = 0usize;
    for mix_name in ["AUDIOMD.MIX", "AUDIO.MIX"] {
        let index = audio_index(&am, mix_name).expect("required retail audio index");
        let names: Vec<String> = index
            .names_with_prefix("")
            .into_iter()
            .map(str::to_string)
            .collect();
        for name in &names {
            let (entry, data) = index
                .get(name)
                .unwrap_or_else(|| panic!("{mix_name}: entry '{name}' failed lookup"));
            if !entry.is_ima_adpcm() {
                continue;
            }
            ima_entries += 1;
            let channels = entry.channels() as usize;
            if channels == 2 {
                stereo_entries += 1;
            }
            // Frequency evidence for the two end-of-stream classes: a short
            // final block (reproduced from the previous block's bytes) and a
            // sound shorter than one whole stride (not reproducible — the
            // native flushes it against a buffer this sound never filled).
            if entry.chunk_size > 0 {
                let stride = entry.chunk_size as usize;
                if data.len() % stride != 0 {
                    short_tail_entries += 1;
                }
                if data.len() < stride {
                    shorter_than_stride_entries += 1;
                }
            }
            let preamble = channels * 4;
            let block_size = if entry.chunk_size > 0 {
                entry.chunk_size as usize
            } else {
                data.len()
            };
            let mut pos = 0usize;
            while pos + preamble <= data.len() {
                let block_end = (pos + block_size).min(data.len());
                for ch in 0..channels {
                    let step = data[pos + ch * 4 + 2];
                    let reserved = data[pos + ch * 4 + 3];
                    if step > 0x58 || reserved != 0 {
                        failures.push(format!(
                            "{mix_name} '{name}': block at {pos} ch {ch} preamble \
                             step {step} reserved {reserved} — native would reject"
                        ));
                    }
                }
                let payload = block_end - pos - preamble;
                let group = 4 * channels;
                if !payload.is_multiple_of(group) {
                    let line = format!(
                        "{mix_name} '{name}': {channels}ch block at {pos} real payload \
                         {payload} not a multiple of {group} — the native pads it \
                         to the stride from the input buffer"
                    );
                    if KNOWN_RAGGED.contains(&name.as_str()) {
                        ragged_known.push(line);
                    } else {
                        failures.push(line);
                    }
                }
                if block_size == 0 {
                    break;
                }
                pos += block_size;
            }
        }
    }
    println!("RECORD: IMA-ADPCM bag entries: {ima_entries} ({stereo_entries} stereo)");
    println!(
        "RECORD: entries with a short final block (native pads from the previous \
         block, we reproduce it): {short_tail_entries}"
    );
    println!(
        "RECORD: entries shorter than one whole stride (native pads from a buffer \
         this sound never filled — VERA-internal residual): {shorter_than_stride_entries}"
    );
    for line in &ragged_known {
        println!("RECORD: known non-group-aligned real tail: {line}");
    }
    assert!(
        !ragged_known.is_empty(),
        "GREXSELB's non-group-aligned real tail is the recorded corpus fact; if it \
         is gone the corpus changed and the exception list should shrink"
    );
    assert!(
        failures.is_empty(),
        "bag ADPCM block invariants: {} violations:\n{}",
        failures.len(),
        failures.join("\n")
    );
}

#[test]
#[ignore = "requires the recorded retail corpus; run the indexed retail-corpus profile"]
fn certify_audio_bag_total() {
    let am = required_corpus();

    let total_entries = audio_bag_total(&am).expect("decode all retail audio bag entries");

    let m = read_manifest().expect("read committed retail corpus manifest");
    assert_eq!(
        m.bag_aud, total_entries,
        "CORPUS-DRIFT: audio.bag entry count changed"
    );
}
