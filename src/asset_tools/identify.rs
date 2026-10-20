//! Format identification for archive entries.
//!
//! Layers on top of [`crate::assets::format_sniff::detect_format`] rather than
//! forking it: that sniffer is the corpus definition the retail certification
//! suite walks, so its verdicts must stay identical. What it deliberately
//! returns `None` for — nested MIX containers, INI text, BIK/VQA video — is
//! exactly what a *browser* still needs to name, so those arms are added here,
//! along with a human-readable structural detail line for every format.
//!
//! ## Dependency rules
//! - Depends on `assets/` only.

use crate::assets::format_sniff::detect_format;
use crate::assets::hva_file::HvaFile;
use crate::assets::mix_archive::MixArchive;
use crate::assets::shp_file::ShpFile;
use crate::assets::vpl_file::VplFile;

/// Short stable tag plus a one-line structural summary.
#[derive(Debug, Clone)]
pub struct Identified {
    /// Stable machine tag: `shp`, `pal`, `mix`, `bik`, `text`, `unknown`, ...
    pub format: &'static str,
    /// Structural detail for a human/agent, e.g. `SHP(TS) 16x2, 5 frames`.
    pub detail: String,
}

/// CSF string table magic — the ASCII bytes ` FSC`.
const CSF_MAGIC: &[u8; 4] = b" FSC";
/// Minimum bytes before a header read is worth attempting.
const MIN_HEADER: usize = 4;

/// Identify one asset from its bytes.
pub fn identify(data: &[u8]) -> Identified {
    if data.len() < MIN_HEADER {
        return Identified {
            format: "tiny",
            detail: format!("{} bytes — too short to identify", data.len()),
        };
    }

    if let Some(tag) = detect_format(data) {
        return Identified {
            format: tag,
            detail: detail_for(tag, data),
        };
    }

    // Everything below is a case format_sniff intentionally skips.
    if let Some(behind_gate) = reclassify_behind_mix_gate(data) {
        return behind_gate;
    }

    if &data[0..3] == b"BIK" {
        let revision = data.get(3).map(|b| *b as char).unwrap_or('?');
        let (w, h) = if data.len() >= 28 {
            (
                u32::from_le_bytes([data[20], data[21], data[22], data[23]]),
                u32::from_le_bytes([data[24], data[25], data[26], data[27]]),
            )
        } else {
            (0, 0)
        };
        return Identified {
            format: "bik",
            detail: format!("Bink 1 video (BIK{revision}), {w}x{h}"),
        };
    }

    if &data[0..4] == b"FORM" {
        return Identified {
            format: "vqa",
            detail: "VQA video".to_string(),
        };
    }

    if data[0] == 0 && data[1] == 0 && MixArchive::looks_like_mix(data) {
        let entries = u16::from_le_bytes([data[4], data[5]]);
        return Identified {
            format: "mix",
            detail: format!("nested MIX archive, ~{entries} entries"),
        };
    }

    if data.len() > 52 && &data[0..4] == b"XCC " {
        return Identified {
            format: "xcc",
            detail: "XCC filename database".to_string(),
        };
    }

    if looks_like_text(data) {
        let preview = ascii_preview(data, 48);
        let kind = if data[0] == b'[' || data[0] == b';' {
            "INI"
        } else {
            "text"
        };
        return Identified {
            format: "text",
            detail: format!("{kind} \"{preview}\""),
        };
    }

    Identified {
        format: "unknown",
        detail: format!(
            "{} bytes, header {:02X} {:02X} {:02X} {:02X}",
            data.len(),
            data[0],
            data[1],
            data[2],
            data[3]
        ),
    }
}

/// Recover leaf formats that the upstream nested-MIX gate swallows.
///
/// `detect_format` tests for a nested MIX *before* it tests for TMP, SHP, PAL,
/// HVA or VPL, and that container test is a heuristic keyed on a leading zero
/// word plus loose size arithmetic. Real assets satisfy it: `POWERP.SHP` reads
/// as a container, and so does any palette whose first entry is black. Once
/// that gate fires the leaf checks never run and the asset comes back as
/// `None`, which a browser would report as an archive.
///
/// Rather than fork the certified sniffer — the retail certification suite
/// walks it, so its verdicts must not move — this reruns the checks it skipped,
/// in its own order, using decisive tests: an exact length for PAL, the same
/// explicit 60x30 header test for TMP, and full parses for the rest. A
/// genuinely nested MIX fails all of them and falls through to the MIX arm.
fn reclassify_behind_mix_gate(data: &[u8]) -> Option<Identified> {
    // The gate only fires on a leading zero word; nothing else can be affected.
    if data[0] != 0 || data[1] != 0 {
        return None;
    }

    // PAL: exactly 768 bytes, the same unconditional rule the sniffer uses.
    if data.len() == PAL_FILE_SIZE {
        return Some(Identified {
            format: "pal",
            detail: detail_for("pal", data),
        });
    }

    // TMP: the sniffer's own explicit test — 60x30 tiles at offsets 8 and 12.
    if data.len() >= 16 {
        let tile_w = u32::from_le_bytes([data[8], data[9], data[10], data[11]]);
        let tile_h = u32::from_le_bytes([data[12], data[13], data[14], data[15]]);
        if tile_w == RA2_TILE_WIDTH && tile_h == RA2_TILE_HEIGHT {
            return Some(Identified {
                format: "tmp",
                detail: detail_for("tmp", data),
            });
        }
    }

    // SHP: a full parse is decisive where the container heuristic is not.
    if let Ok(shp) = ShpFile::from_bytes(data)
        && !shp.frames.is_empty()
        && shp.width > 0
        && shp.height > 0
    {
        return Some(Identified {
            format: "shp",
            detail: format!(
                "SHP(TS) {}x{}, {} frames",
                shp.width,
                shp.height,
                shp.frames.len()
            ),
        });
    }

    // HVA and VPL both carry exact size relationships, so their parsers are
    // decisive too. Order matches the sniffer's.
    if HvaFile::from_bytes(data).is_ok() {
        return Some(Identified {
            format: "hva",
            detail: detail_for("hva", data),
        });
    }
    if VplFile::from_bytes(data).is_ok() {
        return Some(Identified {
            format: "vpl",
            detail: detail_for("vpl", data),
        });
    }

    None
}

/// A .pal is exactly 256 RGB triplets.
const PAL_FILE_SIZE: usize = 768;
/// RA2 isometric tile dimensions, as the sniffer tests them.
const RA2_TILE_WIDTH: u32 = 60;
const RA2_TILE_HEIGHT: u32 = 30;

/// Build the structural detail line for a format the sniffer already named.
/// Header fields are read directly here so a malformed body cannot suppress the
/// summary — the parsers are for the `info` verb, this is for listings.
fn detail_for(tag: &'static str, data: &[u8]) -> String {
    match tag {
        "shp" => {
            let w = u16::from_le_bytes([data[2], data[3]]);
            let h = u16::from_le_bytes([data[4], data[5]]);
            let frames = u16::from_le_bytes([data[6], data[7]]);
            format!("SHP(TS) {w}x{h}, {frames} frames")
        }
        "tmp" => {
            let tw = u32::from_le_bytes([data[0], data[1], data[2], data[3]]);
            let th = u32::from_le_bytes([data[4], data[5], data[6], data[7]]);
            let tile_w = u32::from_le_bytes([data[8], data[9], data[10], data[11]]);
            let tile_h = u32::from_le_bytes([data[12], data[13], data[14], data[15]]);
            format!("TMP {tw}x{th} template, {tile_w}x{tile_h} tiles")
        }
        "vxl" => {
            let limbs = if data.len() >= 32 {
                u32::from_le_bytes([data[28], data[29], data[30], data[31]])
            } else {
                0
            };
            format!("VXL voxel model, {limbs} limbs")
        }
        "hva" => {
            let frames = u32::from_le_bytes([data[16], data[17], data[18], data[19]]);
            let sections = u32::from_le_bytes([data[20], data[21], data[22], data[23]]);
            let name = ascii_preview(&data[0..16.min(data.len())], 16);
            format!("HVA {frames} frames, {sections} sections, \"{name}\"")
        }
        "pal" => {
            let vga_6bit = data.iter().all(|b| *b <= 63);
            if vga_6bit {
                "PAL 256 colors (VGA 6-bit)".to_string()
            } else {
                "PAL 256 colors (full-range bytes)".to_string()
            }
        }
        "csf" => {
            let labels = if data.len() >= 16 && &data[0..4] == CSF_MAGIC {
                u32::from_le_bytes([data[12], data[13], data[14], data[15]])
            } else {
                0
            };
            format!("CSF string table, {labels} labels")
        }
        "aud" => {
            let sample_rate = u16::from_le_bytes([data[0], data[1]]);
            let compression = if data[11] == 99 {
                "IMA ADPCM"
            } else {
                "WS ADPCM"
            };
            format!("AUD {sample_rate} Hz, {compression}")
        }
        "pcx" => {
            // PCX stores inclusive window bounds at offsets 4..12.
            let xmin = u16::from_le_bytes([data[4], data[5]]) as u32;
            let ymin = u16::from_le_bytes([data[6], data[7]]) as u32;
            let xmax = u16::from_le_bytes([data[8], data[9]]) as u32;
            let ymax = u16::from_le_bytes([data[10], data[11]]) as u32;
            format!(
                "PCX {}x{}",
                xmax.saturating_sub(xmin) + 1,
                ymax.saturating_sub(ymin) + 1
            )
        }
        "vpl" => {
            let sections = u32::from_le_bytes([data[8], data[9], data[10], data[11]]);
            format!("VPL voxel lighting, {sections} sections")
        }
        "fnt" => "FNT bitmap font".to_string(),
        other => format!("{} ({} bytes)", other.to_ascii_uppercase(), data.len()),
    }
}

/// Mostly-printable in the first block, with no embedded NULs.
fn looks_like_text(data: &[u8]) -> bool {
    const SAMPLE: usize = 64;
    const PRINTABLE_THRESHOLD: usize = 48;
    let sample = &data[..SAMPLE.min(data.len())];
    if sample.contains(&0) {
        return false;
    }
    sample
        .iter()
        .filter(|byte| {
            let b = **byte;
            (0x20..=0x7E).contains(&b) || b == b'\r' || b == b'\n' || b == b'\t'
        })
        .count()
        >= PRINTABLE_THRESHOLD.min(sample.len())
}

fn ascii_preview(data: &[u8], max: usize) -> String {
    data.iter()
        .take(max)
        .take_while(|b| **b != 0)
        .map(|b| {
            if (0x20..=0x7E).contains(b) {
                *b as char
            } else {
                '.'
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shp_detail_carries_canvas_and_frame_count() {
        // Header only: zero, width=16, height=2, frame_count=5, then 5*24 header bytes.
        let mut data = vec![0u8; 8 + 5 * 24];
        data[2..4].copy_from_slice(&16u16.to_le_bytes());
        data[4..6].copy_from_slice(&2u16.to_le_bytes());
        data[6..8].copy_from_slice(&5u16.to_le_bytes());
        let id = identify(&data);
        assert_eq!(id.format, "shp");
        assert!(id.detail.contains("16x2"), "got {}", id.detail);
        assert!(id.detail.contains("5 frames"), "got {}", id.detail);
    }

    #[test]
    fn palette_reports_whether_bytes_are_vga_range() {
        let vga = vec![10u8; 768];
        assert!(identify(&vga).detail.contains("VGA 6-bit"));
        let mut full = vec![10u8; 768];
        full[0] = 200;
        assert!(identify(&full).detail.contains("full-range"));
    }

    #[test]
    fn ini_text_is_named_rather_than_dropped() {
        let ini = b"[General]\nName=Value\nOther=Thing\n; a comment line here\n".to_vec();
        let id = identify(&ini);
        assert_eq!(id.format, "text");
        assert!(id.detail.starts_with("INI"), "got {}", id.detail);
    }

    #[test]
    fn bink_video_is_named_rather_than_dropped() {
        let mut data = vec![0u8; 64];
        data[0..4].copy_from_slice(b"BIKi");
        data[20..24].copy_from_slice(&640u32.to_le_bytes());
        data[24..28].copy_from_slice(&480u32.to_le_bytes());
        let id = identify(&data);
        assert_eq!(id.format, "bik");
        assert!(id.detail.contains("640x480"), "got {}", id.detail);
    }
}
