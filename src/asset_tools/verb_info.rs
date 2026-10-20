//! `asset info` — every structural fact about one asset, without rendering it.
//!
//! This verb exists to retire a recurring cost: writing a throwaway unit test
//! just to learn one SHP's canvas size, one TMP's tile count, or whether a
//! voxel's HVA sections line up with its limbs. Everything here is read out of
//! a *parsed* file — never re-derived from raw bytes — so the numbers reported
//! are the numbers the engine's own readers see.
//!
//! Parse failures never fail the verb. The header (name, archive, size, sniffed
//! format) is useful precisely when a file will not parse, so a broken body
//! degrades to [`InfoBody::Opaque`] with the parser's message in `warnings`.
//!
//! ## Dependency rules
//! - Depends on `assets/` parsers, `asset_tools::identify`, and
//!   `asset_tools::report`. No render, sim, or UI types.

use crate::assets::asset_manager::AssetManager;
use crate::assets::aud_file;
use crate::assets::csf_file::CsfFile;
use crate::assets::fnt_file::FntFile;
use crate::assets::hva_file::HvaFile;
use crate::assets::pal_file::Palette;
use crate::assets::pcx_file::PcxFile;
use crate::assets::shp_file::ShpFile;
use crate::assets::tmp_file::TmpFile;
use crate::assets::vpl_file::VplFile;
use crate::assets::vxl_file::VxlFile;

use crate::asset_tools::identify;
use crate::asset_tools::report::{
    AsciiGrid, ErrorReport, HvaInfo, InfoBody, InfoReport, ShpFrameInfo, TmpTileInfo, VxlLimbInfo,
};

/// Frames/tiles listed by default. Big enough for a full 8-facing unit sequence
/// with damage states, small enough that a 900-frame animation does not bury
/// the caller's context window.
const DEFAULT_LIMIT: usize = 64;

/// Largest frame the ASCII grid will draw. 4096 px is a 64x64 sprite — the
/// scale where a hex grid is still readable as a picture; above that it is
/// just a wall of digits, and an actual render is the better tool.
const ASCII_MAX_PIXELS: usize = 4096;

/// Bytes of a .pal file: 256 entries x 3 components.
const PAL_FILE_BYTES: usize = 768;

/// Highest legal component in an unscaled VGA 6-bit palette.
const VGA_6BIT_MAX: u8 = 63;

/// House-colour remap band, as a half-open `[start, end)` palette index range.
const HOUSE_REMAP_BAND: [usize; 2] = [16, 32];

/// Frame-header bit that selects row-framed RLE-Zero pixel data.
const SHP_FORMAT_RLE_ZERO_BIT: u8 = 0x02;

/// The shroud sprite is the one SHP whose pixels are not palette indices.
const SHROUD_SHP: &str = "SHROUD.SHP";

/// Emitted for every SHP. The shadow-half split is an art *convention*, not a
/// flag in the file, so the verb states what VERA's sprite path does rather
/// than guessing whether a given file follows it.
const SHADOW_HALF_NOTE: &str = "frame_count is the raw file count: VERA's production sprite path \
     discards the second half of unit/building SHP frames as shadows, so a unit's visible frame \
     count is typically half this. That split is an art convention, not a flag in the file — this \
     verb does not guess whether this asset follows it.";

/// Options for `asset info`.
pub struct InfoOptions {
    /// Frame/tile to detail with `--ascii`. Default 0.
    pub frame: usize,
    /// Emit the palette-index grid for the selected frame.
    pub ascii: bool,
    /// Max frames/tiles listed in the report. Default 64.
    pub limit: usize,
}

impl Default for InfoOptions {
    fn default() -> Self {
        Self {
            frame: 0,
            ascii: false,
            limit: DEFAULT_LIMIT,
        }
    }
}

/// Build the report for one resolved asset.
///
/// `name` is the requested filename; it drives the `.hva` pairing and the
/// name-specific warnings, so it is kept even though the bytes carry no name.
pub fn run(
    asset_manager: &AssetManager,
    name: &str,
    opts: &InfoOptions,
) -> Result<InfoReport, ErrorReport> {
    let Some(resolved) = crate::asset_tools::locate::locate(asset_manager, name) else {
        return Err(ErrorReport {
            error: format!("asset not found: {name}"),
            hint: Some(format!(
                "run `asset find {name}` — it reports loose-file shadowing and hits in \
                 catalogued-but-unreachable archives, which normal name lookup cannot see"
            )),
        });
    };

    let identified = identify::identify(resolved.bytes);
    let mut warnings: Vec<String> = Vec::new();
    warnings.extend(resolved.catalog_warning());

    // Companion lookup for the VXL arm. Both this closure and `resolved.bytes`
    // hold shared borrows of the manager, so they coexist.
    let lookup = |companion_name: &str| asset_manager.get(companion_name);
    let companion: &dyn Fn(&str) -> Option<Vec<u8>> = &lookup;

    let body = build_body(
        identified.format,
        resolved.bytes,
        name,
        opts,
        companion,
        &mut warnings,
    );

    Ok(InfoReport {
        name: name.to_string(),
        source_archive: resolved.source_archive.to_string(),
        entry_id: format!("0x{:08X}", resolved.entry_id as u32),
        bytes: resolved.bytes.len(),
        format: identified.format.to_string(),
        detail: identified.detail,
        body,
        warnings,
    })
}

/// Dispatch to the reader for `format` and build its report body.
///
/// Split out from [`run`] so the per-format logic is exercisable without a
/// mounted retail install: `companion` is the only archive touch, and tests
/// substitute a closure for it.
fn build_body(
    format: &str,
    data: &[u8],
    name: &str,
    opts: &InfoOptions,
    companion: &dyn Fn(&str) -> Option<Vec<u8>>,
    warnings: &mut Vec<String>,
) -> InfoBody {
    match format.to_ascii_lowercase().as_str() {
        "shp" => shp_body(data, name, opts, warnings),
        "tmp" => tmp_body(data, opts, warnings),
        "vxl" => vxl_body(data, name, companion, warnings),
        "hva" => hva_body(data, warnings),
        "pal" => pal_body(data, warnings),
        "csf" => csf_body(data, warnings),
        "aud" => aud_body(data, warnings),
        "pcx" => pcx_body(data, warnings),
        "fnt" => fnt_body(data, warnings),
        "vpl" => vpl_body(data, warnings),
        // MIX, BIK, VQA, INI text, and anything unsniffed: the header fields
        // and the sniffer's detail line are the whole answer.
        _ => InfoBody::Opaque {},
    }
}

// ---------------------------------------------------------------------------
// Format arms
// ---------------------------------------------------------------------------

fn shp_body(data: &[u8], name: &str, opts: &InfoOptions, warnings: &mut Vec<String>) -> InfoBody {
    let shp = match ShpFile::from_bytes(data) {
        Ok(shp) => shp,
        Err(err) => {
            warnings.push(format!("SHP parse failed: {err}"));
            return InfoBody::Opaque {};
        }
    };

    if basename_eq(name, SHROUD_SHP) {
        warnings.push(
            "SHROUD.SHP pixel values are ABuffer brightness levels, not palette indices — \
             rendering it through a palette produces meaningless colours, and the ASCII grid \
             below shows brightness, not colour indices."
                .to_string(),
        );
    }
    warnings.push(SHADOW_HALF_NOTE.to_string());

    let frame_count = shp.frames.len();
    let frames_shown = frame_count.min(opts.limit);
    let mut frames: Vec<ShpFrameInfo> = Vec::with_capacity(frames_shown);
    for (index, frame) in shp.frames.iter().take(frames_shown).enumerate() {
        let pixel_count = frame.pixels.len();
        let index0_count = frame.pixels.iter().filter(|&&px| px == 0).count();
        frames.push(ShpFrameInfo {
            index,
            x: frame.frame_x,
            y: frame.frame_y,
            w: frame.frame_width,
            h: frame.frame_height,
            format: frame.format,
            compressed: frame.format & SHP_FORMAT_RLE_ZERO_BIT != 0,
            radar_color: frame.radar_color,
            pixel_count,
            index0_count,
            // Subtraction is safe: index0_count is a filter over the same vec.
            nonzero_count: pixel_count - index0_count,
        });
    }
    if frames_shown < frame_count {
        warnings.push(format!(
            "frame list truncated by --limit {}: {} of {} frames shown, {} dropped",
            opts.limit,
            frames_shown,
            frame_count,
            frame_count - frames_shown
        ));
    }

    let ascii = if opts.ascii {
        build_ascii_grid(&shp, opts.frame, warnings)
    } else {
        None
    };

    InfoBody::Shp {
        canvas: [shp.width, shp.height],
        frame_count,
        frames_shown,
        frames,
        ascii,
    }
}

/// Render one frame's palette indices as text, or explain why it was skipped.
fn build_ascii_grid(
    shp: &ShpFile,
    frame_index: usize,
    warnings: &mut Vec<String>,
) -> Option<AsciiGrid> {
    let Some(frame) = shp.frames.get(frame_index) else {
        warnings.push(format!(
            "no ASCII grid: frame {} is out of range (file has {} frames)",
            frame_index,
            shp.frames.len()
        ));
        return None;
    };

    let w = frame.frame_width as usize;
    let h = frame.frame_height as usize;
    let area = w.saturating_mul(h);
    if area == 0 {
        warnings.push(format!(
            "no ASCII grid: frame {frame_index} is empty ({w}x{h}); it carries header metadata only"
        ));
        return None;
    }
    if area > ASCII_MAX_PIXELS {
        warnings.push(format!(
            "no ASCII grid: frame {frame_index} is {w}x{h} = {area} px, over the {ASCII_MAX_PIXELS} \
             px readability cap — use `asset render` for a frame this size"
        ));
        return None;
    }
    // A malformed file can decode short of its declared extent; report it once
    // rather than trusting the header's area.
    if frame.pixels.len() < area {
        warnings.push(format!(
            "frame {} decoded {} of {} declared pixels; missing cells are shown as `??`",
            frame_index,
            frame.pixels.len(),
            area
        ));
    }

    let mut rows: Vec<String> = Vec::with_capacity(h);
    for y in 0..h {
        let mut row = String::with_capacity(w * 3);
        for x in 0..w {
            if x > 0 {
                row.push(' ');
            }
            match frame.pixels.get(y * w + x).copied() {
                // Index 0 is transparent; dots let the sprite's silhouette read
                // as a picture instead of a field of `00`s.
                Some(0) => row.push_str(".."),
                Some(px) => row.push_str(&format!("{px:02X}")),
                None => row.push_str("??"),
            }
        }
        rows.push(row);
    }

    Some(AsciiGrid {
        frame: frame_index,
        w: frame.frame_width,
        h: frame.frame_height,
        rows,
    })
}

fn tmp_body(data: &[u8], opts: &InfoOptions, warnings: &mut Vec<String>) -> InfoBody {
    let tmp = match TmpFile::from_bytes(data) {
        Ok(tmp) => tmp,
        Err(err) => {
            warnings.push(format!("TMP parse failed: {err}"));
            return InfoBody::Opaque {};
        }
    };

    let tile_count = tmp.tiles.len();
    let present_count = tmp.tiles.iter().filter(|slot| slot.is_some()).count();
    let tiles_shown = tile_count.min(opts.limit);

    let mut tiles: Vec<TmpTileInfo> = Vec::with_capacity(tiles_shown);
    for (index, slot) in tmp.tiles.iter().take(tiles_shown).enumerate() {
        tiles.push(match slot {
            Some(tile) => TmpTileInfo {
                index,
                present: true,
                height: tile.height,
                terrain_type: tile.terrain_type,
                ramp_type: tile.ramp_type,
                radar_left: tile.radar_left,
                radar_right: tile.radar_right,
                pixel_w: tile.pixel_width,
                pixel_h: tile.pixel_height,
                offset_x: tile.offset_x,
                offset_y: tile.offset_y,
                has_damaged_data: tile.has_damaged_data,
            },
            // An absent cell is a real fact about the template's shape, so it
            // keeps its row rather than being filtered out and shifting indices.
            None => TmpTileInfo {
                index,
                present: false,
                height: 0,
                terrain_type: 0,
                ramp_type: 0,
                radar_left: [0; 3],
                radar_right: [0; 3],
                pixel_w: 0,
                pixel_h: 0,
                offset_x: 0,
                offset_y: 0,
                has_damaged_data: false,
            },
        });
    }
    if tiles_shown < tile_count {
        warnings.push(format!(
            "tile list truncated by --limit {}: {} of {} tiles shown, {} dropped",
            opts.limit,
            tiles_shown,
            tile_count,
            tile_count - tiles_shown
        ));
    }

    InfoBody::Tmp {
        template: [tmp.template_width, tmp.template_height],
        tile: [tmp.tile_width, tmp.tile_height],
        tile_count,
        present_count,
        tiles_shown,
        tiles,
    }
}

fn vxl_body(
    data: &[u8],
    name: &str,
    companion: &dyn Fn(&str) -> Option<Vec<u8>>,
    warnings: &mut Vec<String>,
) -> InfoBody {
    let vxl = match VxlFile::from_bytes(data) {
        Ok(vxl) => vxl,
        Err(err) => {
            warnings.push(format!("VXL parse failed: {err}"));
            return InfoBody::Opaque {};
        }
    };

    let limbs: Vec<VxlLimbInfo> = vxl
        .limbs
        .iter()
        .enumerate()
        .map(|(index, limb)| VxlLimbInfo {
            index,
            name: limb.name.clone(),
            size: [limb.size_x, limb.size_y, limb.size_z],
            scale: limb.scale,
            bounds: limb.bounds,
            transform: limb.transform,
            normals_mode: limb.normals_mode,
            voxel_count: limb.voxels.len(),
        })
        .collect();

    let hva_name = with_extension(name, "hva");
    let hva = match companion(&hva_name) {
        None => {
            warnings.push(format!(
                "no paired {hva_name} resolved — this voxel has no animation transforms, so \
                 turrets and barrels will sit at their default section pose"
            ));
            None
        }
        Some(bytes) => match HvaFile::from_bytes(&bytes) {
            Err(err) => {
                warnings.push(format!("paired {hva_name} failed to parse: {err}"));
                None
            }
            Ok(hva) => {
                let vxl_names: Vec<String> =
                    vxl.limbs.iter().map(|limb| limb.name.clone()).collect();
                let (unmatched_hva_sections, unmatched_vxl_limbs) =
                    unmatched_names(&hva.section_names, &vxl_names);
                if !unmatched_hva_sections.is_empty() || !unmatched_vxl_limbs.is_empty() {
                    warnings.push(format!(
                        "VXL/HVA section names disagree — HVA-only: {:?}, VXL-only: {:?}. \
                         Animation binds sections by name, so those parts will not move.",
                        unmatched_hva_sections, unmatched_vxl_limbs
                    ));
                }
                if hva.section_count as usize != vxl.limbs.len() {
                    warnings.push(format!(
                        "{hva_name} has {} sections but the VXL has {} limbs",
                        hva.section_count,
                        vxl.limbs.len()
                    ));
                }
                Some(HvaInfo {
                    name: hva_name.clone(),
                    frame_count: hva.frame_count,
                    section_count: hva.section_count,
                    section_names: hva.section_names.clone(),
                    unmatched_hva_sections,
                    unmatched_vxl_limbs,
                })
            }
        },
    };

    InfoBody::Vxl {
        limb_count: vxl.limb_count,
        body_size: vxl.body_size,
        limbs,
        hva,
    }
}

fn hva_body(data: &[u8], warnings: &mut Vec<String>) -> InfoBody {
    match HvaFile::from_bytes(data) {
        Ok(hva) => InfoBody::Hva {
            frame_count: hva.frame_count,
            section_count: hva.section_count,
            section_names: hva.section_names,
        },
        Err(err) => {
            warnings.push(format!("HVA parse failed: {err}"));
            InfoBody::Opaque {}
        }
    }
}

fn pal_body(data: &[u8], warnings: &mut Vec<String>) -> InfoBody {
    let palette = match Palette::from_bytes(data) {
        Ok(palette) => palette,
        Err(err) => {
            warnings.push(format!("PAL parse failed: {err}"));
            return InfoBody::Opaque {};
        }
    };

    // Checked against the *source bytes*: the parser decodes components as
    // `raw << 2`, which both hides the 6-bit range and wraps for raw > 63, so
    // the parsed palette cannot answer this question.
    let vga_6bit = match data.get(..PAL_FILE_BYTES) {
        Some(raw) => raw.iter().all(|&byte| byte <= VGA_6BIT_MAX),
        None => false,
    };
    if !vga_6bit {
        warnings.push(
            "palette has components above 63, so it is not an unscaled VGA 6-bit table; the \
             engine's `raw << 2` decode wraps those entries"
                .to_string(),
        );
    }

    let colors: Vec<String> = palette
        .colors()
        .iter()
        .map(|color| format!("#{:02X}{:02X}{:02X}", color.r, color.g, color.b))
        .collect();
    // Counted over the emitted strings so the number always matches the list a
    // caller can see, rather than a raw-byte count that could disagree.
    let mut distinct: Vec<&str> = colors.iter().map(String::as_str).collect();
    distinct.sort_unstable();
    distinct.dedup();
    let unique_colors = distinct.len();

    InfoBody::Pal {
        colors,
        remap_band: HOUSE_REMAP_BAND,
        vga_6bit,
        unique_colors,
    }
}

fn csf_body(data: &[u8], warnings: &mut Vec<String>) -> InfoBody {
    match CsfFile::from_bytes(data) {
        Ok(csf) => InfoBody::Csf {
            version: csf.version,
            language: csf.language,
            entry_count: csf.len(),
        },
        Err(err) => {
            warnings.push(format!("CSF parse failed: {err}"));
            InfoBody::Opaque {}
        }
    }
}

fn aud_body(data: &[u8], warnings: &mut Vec<String>) -> InfoBody {
    match aud_file::parse_header(data) {
        Some(header) => InfoBody::Aud {
            sample_rate: header.sample_rate,
            channels: header.channels(),
            is_16bit: header.is_16bit(),
            format: header.format,
            data_size: header.data_size,
            output_size: header.output_size,
        },
        None => {
            warnings.push("AUD header parse failed: file is shorter than 12 bytes".to_string());
            InfoBody::Opaque {}
        }
    }
}

fn pcx_body(data: &[u8], warnings: &mut Vec<String>) -> InfoBody {
    match PcxFile::from_bytes(data) {
        Ok(pcx) => InfoBody::Pcx {
            w: pcx.width,
            h: pcx.height,
            // The parser rejects a paletted PCX with no trailing VGA table and
            // leaves the array zeroed for the 3-plane direct-RGB form, so a
            // non-zero entry is exactly "this file carries its own palette".
            embedded_palette: pcx.palette.iter().any(|rgb| *rgb != [0, 0, 0]),
        },
        Err(err) => {
            warnings.push(format!("PCX parse failed: {err}"));
            InfoBody::Opaque {}
        }
    }
}

fn fnt_body(data: &[u8], warnings: &mut Vec<String>) -> InfoBody {
    match FntFile::from_bytes(data) {
        Ok(fnt) => InfoBody::Fnt {
            cell_height: fnt.cell_height,
            bitmap_rows: fnt.bitmap_rows,
            bytes_per_row: fnt.bytes_per_row,
            glyph_stride: fnt.glyph_stride,
        },
        Err(err) => {
            warnings.push(format!("FNT parse failed: {err}"));
            InfoBody::Opaque {}
        }
    }
}

fn vpl_body(data: &[u8], warnings: &mut Vec<String>) -> InfoBody {
    match VplFile::from_bytes(data) {
        Ok(vpl) => InfoBody::Vpl {
            first_remap: vpl.first_remap,
            last_remap: vpl.last_remap,
            num_sections: vpl.num_sections,
        },
        Err(err) => {
            warnings.push(format!("VPL parse failed: {err}"));
            InfoBody::Opaque {}
        }
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Compare two section-name lists case- and padding-insensitively, returning
/// `(hva_only, vxl_only)` in their original spelling.
///
/// Retail mixes cases freely (`BODY` vs `body`) and section names arrive
/// null-padded, so a byte-exact compare would report mismatches that the
/// engine's own lookup does not see.
fn unmatched_names(hva_sections: &[String], vxl_limbs: &[String]) -> (Vec<String>, Vec<String>) {
    let hva_keys: Vec<String> = hva_sections.iter().map(|s| name_key(s)).collect();
    let vxl_keys: Vec<String> = vxl_limbs.iter().map(|s| name_key(s)).collect();

    let mut hva_only: Vec<String> = Vec::new();
    for (name, key) in hva_sections.iter().zip(hva_keys.iter()) {
        if !vxl_keys.contains(key) {
            hva_only.push(name.clone());
        }
    }
    let mut vxl_only: Vec<String> = Vec::new();
    for (name, key) in vxl_limbs.iter().zip(vxl_keys.iter()) {
        if !hva_keys.contains(key) {
            vxl_only.push(name.clone());
        }
    }

    (hva_only, vxl_only)
}

/// Normalised comparison key for a limb/section name.
fn name_key(name: &str) -> String {
    name.trim_end_matches('\0').trim().to_ascii_uppercase()
}

/// Swap an asset's extension, e.g. `HTNK.VXL` -> `HTNK.hva`.
///
/// MIX entry names are flat, so the final dot is unambiguously the extension
/// separator; a name with no dot simply gains one.
fn with_extension(name: &str, ext: &str) -> String {
    match name.rfind('.') {
        Some(dot) => format!("{}.{}", &name[..dot], ext),
        None => format!("{name}.{ext}"),
    }
}

/// Case-insensitive match of an asset's filename against `target`.
fn basename_eq(name: &str, target: &str) -> bool {
    let base = name.rsplit(['/', '\\']).next().unwrap_or(name);
    base.eq_ignore_ascii_case(target)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Hand-assemble an SHP with uncompressed frames, mirroring the byte layout
    /// in `assets::shp_file`'s `make_test_shp_raw`.
    fn make_shp(
        canvas_w: u16,
        canvas_h: u16,
        frame_w: u16,
        frame_h: u16,
        frames: &[Vec<u8>],
    ) -> Vec<u8> {
        let mut out: Vec<u8> = Vec::new();

        // File header (8 bytes).
        out.extend_from_slice(&0u16.to_le_bytes()); // zero marker
        out.extend_from_slice(&canvas_w.to_le_bytes());
        out.extend_from_slice(&canvas_h.to_le_bytes());
        out.extend_from_slice(&(frames.len() as u16).to_le_bytes());

        // Frame headers (24 bytes each), then the pixel payloads back to back.
        let mut data_offset: u32 = 8 + (frames.len() as u32) * 24;
        for pixels in frames {
            out.extend_from_slice(&0u16.to_le_bytes()); // +0 frame_x
            out.extend_from_slice(&0u16.to_le_bytes()); // +2 frame_y
            out.extend_from_slice(&frame_w.to_le_bytes()); // +4
            out.extend_from_slice(&frame_h.to_le_bytes()); // +6
            out.push(0x00); // +8 format: raw
            out.extend_from_slice(&[0u8; 3]); // +9 padding
            out.extend_from_slice(&[0x40, 0x80, 0xC0]); // +12 radar colour
            out.extend_from_slice(&[0u8; 5]); // +15 unused + reserved
            out.extend_from_slice(&data_offset.to_le_bytes()); // +20
            data_offset += pixels.len() as u32;
        }
        for pixels in frames {
            out.extend_from_slice(pixels);
        }
        out
    }

    /// A 768-byte palette whose entry `i` is `(i, i/2, i/4)` wrapped into the
    /// VGA 6-bit range, so every component stays <= 63.
    fn make_pal() -> Vec<u8> {
        let mut data = vec![0u8; PAL_FILE_BYTES];
        for i in 0..256usize {
            data[i * 3] = (i as u8) % 64;
            data[i * 3 + 1] = ((i as u8) / 2) % 64;
            data[i * 3 + 2] = ((i as u8) / 4) % 64;
        }
        data
    }

    fn no_companion(_: &str) -> Option<Vec<u8>> {
        None
    }

    #[test]
    fn shp_body_reports_canvas_frame_geometry_and_index_zero_counts() {
        let data = make_shp(2, 2, 2, 2, &[vec![1, 2, 3, 0]]);
        let mut warnings = Vec::new();
        let body = build_body(
            "shp",
            &data,
            "TEST.SHP",
            &InfoOptions::default(),
            &no_companion,
            &mut warnings,
        );

        let InfoBody::Shp {
            canvas,
            frame_count,
            frames_shown,
            frames,
            ascii,
        } = body
        else {
            panic!("expected an SHP body");
        };
        assert_eq!(canvas, [2, 2]);
        assert_eq!(frame_count, 1);
        assert_eq!(frames_shown, 1);
        assert!(ascii.is_none(), "ascii is opt-in");
        assert_eq!(frames.len(), 1);
        assert_eq!((frames[0].w, frames[0].h), (2, 2));
        assert_eq!(frames[0].pixel_count, 4);
        assert_eq!(frames[0].index0_count, 1);
        assert_eq!(frames[0].nonzero_count, 3);
        assert!(!frames[0].compressed);
        assert_eq!(frames[0].radar_color, [0x40, 0x80, 0xC0]);
    }

    #[test]
    fn shroud_shp_warns_that_pixels_are_brightness_not_palette_indices() {
        let data = make_shp(2, 2, 2, 2, &[vec![1, 2, 3, 0]]);
        let mut warnings = Vec::new();
        build_body(
            "shp",
            &data,
            "shroud.shp",
            &InfoOptions::default(),
            &no_companion,
            &mut warnings,
        );
        assert!(
            warnings.iter().any(|w| w.contains("ABuffer brightness")),
            "expected the shroud warning for a case-insensitive name match, got {warnings:?}"
        );
    }

    #[test]
    fn frame_list_truncation_is_never_silent() {
        let frames: Vec<Vec<u8>> = (0..5).map(|i| vec![i as u8; 4]).collect();
        let data = make_shp(2, 2, 2, 2, &frames);
        let opts = InfoOptions {
            limit: 2,
            ..InfoOptions::default()
        };
        let mut warnings = Vec::new();
        let body = build_body(
            "shp",
            &data,
            "TEST.SHP",
            &opts,
            &no_companion,
            &mut warnings,
        );

        let InfoBody::Shp {
            frame_count,
            frames_shown,
            frames,
            ..
        } = body
        else {
            panic!("expected an SHP body");
        };
        assert_eq!(frame_count, 5, "the true total must survive truncation");
        assert_eq!(frames_shown, 2);
        assert_eq!(frames.len(), 2);
        let truncation = warnings
            .iter()
            .find(|w| w.contains("truncated"))
            .expect("truncation must be reported");
        assert!(truncation.contains("3 dropped"), "got {truncation}");
    }

    #[test]
    fn ascii_grid_dimensions_match_the_frame_and_index_zero_reads_as_dots() {
        let data = make_shp(2, 2, 2, 2, &[vec![1, 2, 3, 0]]);
        let opts = InfoOptions {
            ascii: true,
            ..InfoOptions::default()
        };
        let mut warnings = Vec::new();
        let body = build_body(
            "shp",
            &data,
            "TEST.SHP",
            &opts,
            &no_companion,
            &mut warnings,
        );

        let InfoBody::Shp { ascii, .. } = body else {
            panic!("expected an SHP body");
        };
        let grid = ascii.expect("ascii grid requested");
        assert_eq!(grid.frame, 0);
        assert_eq!((grid.w, grid.h), (2, 2));
        assert_eq!(grid.rows, vec!["01 02".to_string(), "03 ..".to_string()]);
    }

    #[test]
    fn ascii_grid_is_skipped_with_a_reason_when_the_frame_is_too_large() {
        // 128x64 = 8192 px, over the readability cap.
        let data = make_shp(128, 64, 128, 64, &[vec![7u8; 8192]]);
        let opts = InfoOptions {
            ascii: true,
            ..InfoOptions::default()
        };
        let mut warnings = Vec::new();
        let body = build_body("shp", &data, "BIG.SHP", &opts, &no_companion, &mut warnings);

        let InfoBody::Shp { ascii, .. } = body else {
            panic!("expected an SHP body");
        };
        assert!(ascii.is_none());
        assert!(
            warnings.iter().any(|w| w.contains("readability cap")),
            "the skip must be explained, got {warnings:?}"
        );
    }

    #[test]
    fn ascii_grid_out_of_range_frame_warns_instead_of_panicking() {
        let data = make_shp(2, 2, 2, 2, &[vec![1, 2, 3, 0]]);
        let opts = InfoOptions {
            frame: 9,
            ascii: true,
            ..InfoOptions::default()
        };
        let mut warnings = Vec::new();
        let body = build_body(
            "shp",
            &data,
            "TEST.SHP",
            &opts,
            &no_companion,
            &mut warnings,
        );

        let InfoBody::Shp { ascii, .. } = body else {
            panic!("expected an SHP body");
        };
        assert!(ascii.is_none());
        assert!(warnings.iter().any(|w| w.contains("out of range")));
    }

    #[test]
    fn an_unparsable_body_degrades_to_opaque_and_keeps_the_error() {
        let mut warnings = Vec::new();
        let body = build_body(
            "shp",
            &[0u8; 6],
            "BROKEN.SHP",
            &InfoOptions::default(),
            &no_companion,
            &mut warnings,
        );
        assert!(matches!(body, InfoBody::Opaque {}));
        assert!(
            warnings.iter().any(|w| w.contains("SHP parse failed")),
            "got {warnings:?}"
        );
    }

    #[test]
    fn pal_body_emits_256_hex_colors_and_flags_a_6bit_table() {
        let data = make_pal();
        let mut warnings = Vec::new();
        let body = build_body(
            "pal",
            &data,
            "UNITTEM.PAL",
            &InfoOptions::default(),
            &no_companion,
            &mut warnings,
        );

        let InfoBody::Pal {
            colors,
            remap_band,
            vga_6bit,
            unique_colors,
        } = body
        else {
            panic!("expected a PAL body");
        };
        assert_eq!(colors.len(), 256);
        assert_eq!(remap_band, [16, 32]);
        assert!(vga_6bit, "every component is <= 63");
        // Entry 1 is raw (1, 0, 0); the engine decodes components as raw << 2.
        assert_eq!(colors[1], "#040000");
        assert!(colors.iter().all(|c| c.starts_with('#') && c.len() == 7));
        assert!(unique_colors > 1 && unique_colors <= 256);
        assert!(warnings.is_empty(), "a clean 6-bit palette warns nothing");
    }

    #[test]
    fn pal_body_detects_a_non_6bit_table_from_the_source_bytes() {
        let mut data = make_pal();
        data[9] = 200; // above the VGA 6-bit range
        let mut warnings = Vec::new();
        let body = build_body(
            "pal",
            &data,
            "ODD.PAL",
            &InfoOptions::default(),
            &no_companion,
            &mut warnings,
        );

        let InfoBody::Pal { vga_6bit, .. } = body else {
            panic!("expected a PAL body");
        };
        assert!(!vga_6bit);
        assert!(warnings.iter().any(|w| w.contains("above 63")));
    }

    #[test]
    fn section_name_matching_ignores_case_and_null_padding() {
        let hva = vec!["BODY".to_string(), "turret\0".to_string()];
        let vxl = vec!["body".to_string(), "TURRET".to_string()];
        let (hva_only, vxl_only) = unmatched_names(&hva, &vxl);
        assert!(hva_only.is_empty(), "got {hva_only:?}");
        assert!(vxl_only.is_empty(), "got {vxl_only:?}");
    }

    #[test]
    fn section_name_mismatch_is_reported_from_both_sides() {
        let hva = vec!["BODY".to_string(), "BARREL".to_string()];
        let vxl = vec!["body".to_string(), "turret".to_string()];
        let (hva_only, vxl_only) = unmatched_names(&hva, &vxl);
        assert_eq!(hva_only, vec!["BARREL".to_string()]);
        assert_eq!(vxl_only, vec!["turret".to_string()]);
    }

    #[test]
    fn extension_swap_targets_the_final_dot() {
        assert_eq!(with_extension("HTNK.VXL", "hva"), "HTNK.hva");
        assert_eq!(with_extension("HTNKTUR", "hva"), "HTNKTUR.hva");
        assert_eq!(with_extension("A.B.VXL", "hva"), "A.B.hva");
    }
}
