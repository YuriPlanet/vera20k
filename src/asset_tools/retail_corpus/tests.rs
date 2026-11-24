use super::*;

#[test]
fn zero_and_partial_format_coverage_are_errors() {
    assert!(require_formats(std::iter::empty()).is_err());
    let error = require_formats(["shp", "tmp"].into_iter()).unwrap_err();
    assert!(error.contains("aud"), "{error}");
}

#[test]
fn unreadable_indexed_payload_is_an_error_not_an_unclassified_file() {
    // A valid MIX index can advertise bytes outside its actual body.
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&0u32.to_le_bytes()); // unencrypted new format
    bytes.extend_from_slice(&1u16.to_le_bytes()); // one index entry
    bytes.extend_from_slice(&8u32.to_le_bytes()); // advertised body length
    bytes.extend_from_slice(&7i32.to_le_bytes());
    bytes.extend_from_slice(&0u32.to_le_bytes()); // entry offset
    bytes.extend_from_slice(&8u32.to_le_bytes()); // entry length; body absent
    let archive = crate::assets::mix_archive::MixArchive::from_bytes(bytes).unwrap();
    let error = walk_archive_sniffed("truncated.mix", &archive, &mut |_, _| {
        panic!("unreadable bytes must not reach a parser");
    })
    .unwrap_err();
    assert!(
        error.contains("truncated.mix") && error.contains("00000007"),
        "{error}"
    );
}
