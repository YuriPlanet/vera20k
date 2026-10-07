//! `certify_*` byte-identity round-trips (PAL, HVA, VPL, MIX name resolution).
//!
//! HVA/VPL fields are compared directly with retail bytes. PAL components
//! are compared with checked native expansion outputs for those input bytes;
//! renderer alpha policy is outside that native component comparison.

use super::*;

use crate::assets::hva_file::HvaFile;
use crate::assets::pal_file::Palette;
use crate::assets::vpl_file::VplFile;

/// Original PAL expansion 0x0072AE3E -> 0x0072AE99, reproduced with
/// `python -m tools.sidebar_oracle.palette --check`. The synthetic full-byte
/// case exercises all component inputs; no second conversion formula lives here.
fn native_palette_components() -> [u8; 256] {
    #[derive(serde::Deserialize)]
    struct Packet {
        cases: Vec<Case>,
    }
    #[derive(serde::Deserialize)]
    struct Case {
        archive: String,
        name: String,
        raw: Vec<u8>,
        rgb: Vec<u8>,
    }
    let packet: Packet = serde_json::from_str(crate::test_fixture::text(
        "tools/sidebar_oracle/palette.json",
    ))
    .expect("checked native palette fixture");
    let mut cases = packet
        .cases
        .into_iter()
        .filter(|case| case.archive == "synthetic" && case.name == "full-byte");
    let case = cases.next().expect("native full-byte palette case");
    assert!(
        cases.next().is_none(),
        "full-byte palette case must be unique"
    );
    assert_eq!(case.raw.len(), 768, "native input palette size");
    assert_eq!(
        case.rgb.len(),
        case.raw.len(),
        "native expansion output size"
    );
    let mut components = [None; 256];
    for (raw, expanded) in case.raw.into_iter().zip(case.rgb) {
        if let Some(previous) = components[usize::from(raw)].replace(expanded) {
            assert_eq!(
                previous, expanded,
                "native component {raw} has inconsistent outputs"
            );
        }
    }
    assert!(
        components.iter().all(Option::is_some),
        "native component coverage must include all 256 bytes"
    );
    components.map(|value| value.expect("complete component coverage"))
}

#[test]
#[ignore = "requires the recorded retail corpus; run the indexed retail-corpus profile"]
fn certify_pal_roundtrip_bytes() {
    let native_components = native_palette_components();
    certify_format("pal", |_, data| {
        // The retail corpus is six-bit even though the checked native fixture
        // covers all byte inputs. Preserve that separate corpus invariant.
        if let Some(pos) = data.iter().position(|&b| b > 63) {
            return Err(format!(
                "raw byte {} at offset {pos} exceeds the 6-bit VGA domain",
                data[pos]
            ));
        }
        let vga = Palette::from_bytes(data).map_err(|e| e.to_string())?;
        let ui = Palette::from_bytes_gamemd_ui(data).map_err(|e| e.to_string())?;
        for i in 0..256 {
            let raw = [data[i * 3], data[i * 3 + 1], data[i * 3 + 2]];
            let v = vga.colors[i];
            let u = ui.colors[i];
            // Alpha excluded: transparency policy is a documented parser
            // choice (index 0 / magenta chroma key), not file data.
            let expected = raw.map(|component| native_components[usize::from(component)]);
            if [v.r, v.g, v.b] != expected {
                return Err(format!(
                    "index {i}: from_bytes {:?} != native expansion of {raw:?} = {expected:?}",
                    [v.r, v.g, v.b]
                ));
            }
            if [u.r, u.g, u.b] != expected {
                return Err(format!(
                    "index {i}: from_bytes_gamemd_ui {:?} != native expansion of {raw:?} = {expected:?}",
                    [u.r, u.g, u.b]
                ));
            }
        }
        Ok(())
    });
}

#[test]
#[ignore = "requires the recorded retail corpus; run the indexed retail-corpus profile"]
fn certify_hva_roundtrip_bytes() {
    certify_format("hva", |_, data| {
        let hva = HvaFile::from_bytes(data).map_err(|e| e.to_string())?;
        let sections = hva.section_count as usize;
        // Section names: raw null-padded 16-byte fields at 24 + i*16.
        for (i, name) in hva.section_names.iter().enumerate() {
            let off = 24 + i * 16;
            let raw = &data[off..off + 16];
            let end = raw.iter().position(|&b| b == 0).unwrap_or(16);
            let expected = String::from_utf8_lossy(&raw[..end]);
            if name != &expected {
                return Err(format!("section {i}: name '{name}' != raw '{expected}'"));
            }
        }
        // Transforms: bit-identical to the raw LE f32s (not epsilon).
        let matrices_start = 24 + sections * 16;
        for (m, matrix) in hva.transforms.iter().enumerate() {
            let base = matrices_start + m * 48;
            for (k, v) in matrix.iter().enumerate() {
                let off = base + k * 4;
                let raw = [data[off], data[off + 1], data[off + 2], data[off + 3]];
                if v.to_le_bytes() != raw {
                    return Err(format!(
                        "matrix {m} float {k}: {:?} != raw {raw:?} at offset {off}",
                        v.to_le_bytes()
                    ));
                }
            }
        }
        Ok(())
    });
}

#[test]
#[ignore = "requires the recorded retail corpus; run the indexed retail-corpus profile"]
fn certify_vpl_roundtrip_bytes() {
    certify_format("vpl", |_, data| {
        let vpl = VplFile::from_bytes(data).map_err(|e| e.to_string())?;
        let raw_first = u32::from_le_bytes([data[0], data[1], data[2], data[3]]);
        let raw_last = u32::from_le_bytes([data[4], data[5], data[6], data[7]]);
        let raw_sections = u32::from_le_bytes([data[8], data[9], data[10], data[11]]);
        if vpl.first_remap != raw_first
            || vpl.last_remap != raw_last
            || vpl.num_sections != raw_sections
        {
            return Err(format!(
                "header ({}, {}, {}) != raw ({raw_first}, {raw_last}, {raw_sections})",
                vpl.first_remap, vpl.last_remap, vpl.num_sections
            ));
        }
        let pages = vpl.pages_slice();
        if pages.len() != raw_sections as usize {
            return Err(format!(
                "pages_slice().len() {} != raw num_sections {raw_sections}",
                pages.len()
            ));
        }
        for (p, page) in pages.iter().enumerate() {
            let off = 16 + 768 + p * 256;
            if page != &data[off..off + 256] {
                return Err(format!("page {p} differs from raw bytes at offset {off}"));
            }
        }
        Ok(())
    });
}

/// Filenames the engine demonstrably resolves at runtime (src/bin/extract-ini.rs
/// INI chain, src/app_init.rs CSF chain, src/app_init_helpers.rs palettes).
/// Resolving them here through the retail Westwood-built MIX indexes certifies
/// our filename CRC (mix_hash) against indexes we did not build.
const KNOWN_NAMES: &[&str] = &[
    "rules.ini",
    "rulesmd.ini",
    "art.ini",
    "artmd.ini",
    "ai.ini",
    "aimd.ini",
    "sound.ini",
    "soundmd.ini",
    "eva.ini",
    "evamd.ini",
    "theme.ini",
    "thememd.ini",
    "ra2.csf",
    "ra2md.csf",
    "cameo.pal",
    "unittem.pal",
    "isotem.pal",
];

#[test]
#[ignore = "requires the recorded retail corpus; run the indexed retail-corpus profile"]
fn certify_mix_known_name_resolution() {
    let am = required_corpus();
    let mut failures: Vec<String> = Vec::new();
    for name in KNOWN_NAMES {
        match am.get_with_source_ref(name) {
            Some((data, source)) => {
                println!("RECORD: {name} -> {source} ({} bytes)", data.len());
            }
            None => failures.push(format!(
                "{name}: not resolved by any retail-built MIX index (CRC drift?)"
            )),
        }
    }
    assert!(
        failures.is_empty(),
        "mix name resolution: {} of {} known names failed:\n{}",
        failures.len(),
        KNOWN_NAMES.len(),
        failures.join("\n")
    );
}
