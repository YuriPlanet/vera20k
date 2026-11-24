//! Rust-versus-prior-Rust change detectors. These do not establish native parity.
use super::*;

#[test]
#[ignore = "requires the recorded retail corpus; run the indexed retail-corpus profile"]
fn ratchet_decode_digests() {
    let am = required_corpus();
    let rollups = decoder_ratchets::decode_rollups(&am).expect("decode all corpus files");
    let m = read_manifest().expect("read committed retail corpus manifest");
    for (fmt, digest) in &rollups {
        let stored = m.decode_rollups.get(fmt);
        assert_eq!(
            stored,
            Some(digest),
            "RATCHET: {fmt} decode output changed (stored {stored:?}, computed {digest:#018x}). \
             Investigate the decoder change; asset corpus-baseline --all-mixes --out <NEW_FILE> exports a \
             reviewable candidate. Ratchet digests are NOT parity evidence."
        );
    }
    assert_eq!(
        m.decode_rollups.len(),
        rollups.len(),
        "RATCHET: stored format set differs from computed set"
    );
}

#[test]
#[ignore = "requires the recorded retail corpus; run the indexed retail-corpus profile"]
fn ratchet_named_file_digests() {
    let am = required_corpus();
    let rows = decoder_ratchets::named_file_digests(&am).expect("decode all named ratchet files");
    let m = read_manifest().expect("read committed retail corpus manifest");
    assert_eq!(
        m.files, rows,
        "RATCHET: a named file's decode digest changed. Investigate the decoder change; \
         asset corpus-baseline --all-mixes --out <NEW_FILE> exports a reviewable candidate. \
         Ratchet digests are NOT parity evidence."
    );
}
