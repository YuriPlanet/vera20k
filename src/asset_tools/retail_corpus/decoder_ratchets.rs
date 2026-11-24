//! Digests of this Rust decoder's outputs, not native parity goldens.

use super::*;

use crate::assets::aud_file::decode_aud;
use crate::assets::csf_file::CsfFile;
use crate::assets::fnt_file::FntFile;
use crate::assets::hva_file::HvaFile;
use crate::assets::pal_file::Palette;
use crate::assets::pcx_file::PcxFile;
use crate::assets::shp_file::ShpFile;
use crate::assets::tmp_file::TmpFile;
use crate::assets::vpl_file::VplFile;
use crate::assets::vxl_file::VxlFile;

/// Digest one file's DECODED output (not its raw bytes) with FNV-1a.
///
/// Every fold is length- or field-ordered deterministically so the digest is
/// stable across runs and platforms. CSF pairs are sorted by key first (the
/// parser stores them in a HashMap, whose iteration order is not stable).
fn digest_decode(format: &str, data: &[u8], mut h: u64) -> Result<u64, String> {
    match format {
        "shp" => {
            let shp = ShpFile::from_bytes(data).map_err(|e| e.to_string())?;
            for fr in &shp.frames {
                for v in [fr.frame_x, fr.frame_y, fr.frame_width, fr.frame_height] {
                    h = fnv1a64_fold_bytes(h, &v.to_le_bytes());
                }
                h = fnv1a64_fold_bytes(h, &fr.pixels);
            }
        }
        "tmp" => {
            let tmp = TmpFile::from_bytes(data).map_err(|e| e.to_string())?;
            for tile in tmp.tiles.iter().flatten() {
                for v in [tile.pixel_width, tile.pixel_height] {
                    h = fnv1a64_fold_bytes(h, &v.to_le_bytes());
                }
                for v in [tile.offset_x, tile.offset_y] {
                    h = fnv1a64_fold_bytes(h, &v.to_le_bytes());
                }
                h = fnv1a64_fold_bytes(h, &tile.pixels);
                h = fnv1a64_fold_bytes(h, &tile.depth);
            }
        }
        "vxl" => {
            let vxl = VxlFile::from_bytes(data).map_err(|e| e.to_string())?;
            for limb in &vxl.limbs {
                for v in &limb.voxels {
                    h = fnv1a64_fold_bytes(h, &[v.x, v.y, v.z, v.color_index, v.normal_index]);
                }
            }
        }
        "hva" => {
            let hva = HvaFile::from_bytes(data).map_err(|e| e.to_string())?;
            for matrix in &hva.transforms {
                for f in matrix {
                    h = fnv1a64_fold_bytes(h, &f.to_le_bytes());
                }
            }
        }
        "pal" => {
            let pal = Palette::from_bytes(data).map_err(|e| e.to_string())?;
            h = fnv1a64_fold_bytes(h, &pal.to_rgba_bytes());
        }
        "csf" => {
            let csf = CsfFile::from_bytes(data).map_err(|e| e.to_string())?;
            let mut pairs: Vec<(&str, &str)> = csf.entries().collect();
            pairs.sort_unstable();
            for (k, v) in pairs {
                h = fnv1a64_fold_bytes(h, &(k.len() as u32).to_le_bytes());
                h = fnv1a64_fold_bytes(h, k.as_bytes());
                h = fnv1a64_fold_bytes(h, &(v.len() as u32).to_le_bytes());
                h = fnv1a64_fold_bytes(h, v.as_bytes());
            }
        }
        "aud" => {
            let (_, samples) = decode_aud(data).ok_or("decode_aud returned None")?;
            for s in samples {
                h = fnv1a64_fold_bytes(h, &s.to_le_bytes());
            }
        }
        "fnt" => {
            let fnt = FntFile::from_bytes(data).map_err(|e| e.to_string())?;
            for cp in 0u16..=u16::MAX {
                let Some(g) = fnt.glyph(cp) else { continue };
                h = fnv1a64_fold_bytes(h, &cp.to_le_bytes());
                h = fnv1a64_fold_bytes(h, &g.width.to_le_bytes());
                h = fnv1a64_fold_bytes(h, &g.rgba);
            }
        }
        "pcx" => {
            let pcx = PcxFile::from_bytes(data).map_err(|e| e.to_string())?;
            for v in [pcx.width, pcx.height] {
                h = fnv1a64_fold_bytes(h, &v.to_le_bytes());
            }
            h = fnv1a64_fold_bytes(h, &pcx.pixels);
            for rgb in &pcx.palette {
                h = fnv1a64_fold_bytes(h, rgb);
            }
        }
        "vpl" => {
            let vpl = VplFile::from_bytes(data).map_err(|e| e.to_string())?;
            for page in vpl.pages_slice() {
                h = fnv1a64_fold_bytes(h, page);
            }
        }
        other => return Err(format!("no digest rule for format '{other}'")),
    }
    Ok(h)
}

const NAMED_FILES: &[(&str, &str)] = &[
    ("shp", "gtnkicon.shp"),
    ("pal", "cameo.pal"),
    ("pal", "unittem.pal"),
    ("pal", "isotem.pal"),
    ("csf", "ra2md.csf"),
    ("vxl", "bus.vxl"),
    ("hva", "bus.hva"),
    ("tmp", "clear01.tem"),
    ("tmp", "clear01.sno"),
    ("aud", "intro.aud"),
];

pub(super) fn decode_rollups(am: &AssetManager) -> Result<BTreeMap<String, u64>, String> {
    let mut rollups = BTreeMap::new();
    let mut errors = Vec::new();
    walk_sniffed(am, |ce, data| {
        let h = rollups
            .entry(ce.format.to_string())
            .or_insert(FNV1A64_OFFSET_BASIS);
        *h = fnv1a64_fold_bytes(*h, &(ce.id as u32).to_le_bytes());
        match digest_decode(ce.format, data, *h) {
            Ok(next) => *h = next,
            Err(msg) => errors.push(format!("{} {:#010X}: {msg}", ce.archive, ce.id as u32)),
        }
    })?;
    if !errors.is_empty() {
        return Err(format!(
            "decode errors during digest:\n{}",
            errors.join("\n")
        ));
    }
    require_formats(rollups.keys().map(String::as_str))?;
    Ok(rollups)
}

pub(super) fn named_file_digests(am: &AssetManager) -> Result<Vec<(String, String, u64)>, String> {
    let mut rows = Vec::new();
    let mut failures = Vec::new();
    for (fmt, name) in NAMED_FILES {
        // This is a tooling corpus, including inactive theater archives. Use
        // the browser's explicit catalogue fallback, not runtime reachability.
        let Some(resolved) = crate::asset_tools::locate::locate(am, name) else {
            failures.push(format!(
                "{name}: absent from production lookup and the archive catalogue"
            ));
            continue;
        };
        match digest_decode(fmt, resolved.bytes, FNV1A64_OFFSET_BASIS) {
            Ok(h) => rows.push((fmt.to_string(), name.to_string(), h)),
            Err(msg) => failures.push(format!("{name}: {msg}")),
        }
    }
    if !failures.is_empty() {
        return Err(format!(
            "named ratchet files: {} failures:\n{}",
            failures.len(),
            failures.join("\n")
        ));
    }
    Ok(rows)
}
