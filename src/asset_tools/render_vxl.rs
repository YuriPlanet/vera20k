//! `asset render` for voxel models — one PNG per facing, plus a contact sheet.
//!
//! A voxel has no single correct picture. The same model looks different at
//! every one of the 256 facing bytes, and the bugs worth catching here — a
//! model that mirrors, a turret that points the wrong way, a house colour that
//! never reaches the remap band — only show up when several facings sit side by
//! side. So the unit of output is a *facing*, not a frame, and the default is
//! the eight compass points rather than one pose.
//!
//! The rasteriser is [`crate::render::vxl_raster`], which is pure CPU and
//! runs headless. What it returns is a buffer
//! of post-VPL, pre-house-remap palette indices in which byte 0 means "no voxel
//! here". That byte must become fully transparent regardless of what the
//! palette holds at index 0 — every palette entry is opaque, and honouring it
//! would paint the whole background in palette colour 0.
//!
//! Turret and barrel are composited in, through the same depth-correct CPU
//! merge the unit atlas bakes with (`render::unit_atlas::composite_unit_vxl_cpu`)
//! — so a tank is drawn with its gun. The render is per image, not per type: a
//! vehicle type without `Turret=` draws its image bare. Which layers were merged
//! is reported in the warnings.
//!
//! ## Dependency rules
//! - Depends on `assets/` (VXL/HVA/VPL/palette), `rules/` (`[Colors]` schemes
//!   and the art registry), the CPU-only `render::vxl_raster`, and the sibling
//!   `canvas` / `identify` / `locate` / `names` / `palette` / `report` modules.
//! - Nothing from `sim/`, `ui/`, `audio/`, `net/`, and no GPU type.

use std::path::{Path, PathBuf};

use crate::asset_tools::canvas::{self, Rgba, SheetCell};
use crate::asset_tools::identify;
use crate::asset_tools::names::NameDict;
use crate::asset_tools::palette;
use crate::asset_tools::report::{ErrorReport, RenderOutputs, RenderReport};
use crate::asset_tools::verb_render::RenderOptions;
use crate::assets::asset_manager::AssetManager;
use crate::assets::hva_file::HvaFile;
use crate::assets::pal_file::Palette;
use crate::assets::vpl_file::VplFile;
use crate::assets::vxl_file::VxlFile;
use crate::render::unit_atlas::composite_unit_vxl_cpu;
use crate::render::vxl_raster::{VxlRenderParams, VxlSprite};
use crate::rules::art_data::ArtRegistry;
use crate::rules::color_scheme::parse_color_schemes;
use crate::rules::house_colors::{HouseColorIndex, HouseColorRamps};
use crate::rules::ini_parser::IniFile;

/// `RenderReport::kind` for this path.
const KIND_VXL: &str = "vxl";

/// `RenderReport::mode` for this path. There is no canvas/crop distinction: a
/// voxel sprite's extent is computed from the model, not declared in a header.
const MODE_VOXEL: &str = "voxel";

/// Format tag `identify` returns for a voxel model.
const FORMAT_VXL: &str = "vxl";

/// Subdirectory under the output root shared with the SHP render path, so a
/// caller finds sprite and voxel output in the same tree.
const RENDER_SUBDIR: &str = "render";

/// Sidecar filename listing the geometry of every rendered facing.
const INDEX_TSV_NAME: &str = "index.tsv";

/// Header row of [`INDEX_TSV_NAME`].
const INDEX_TSV_HEADER: &str = "facing\tdegrees\tw\th\tpng";

/// Directory name used when the asset name sanitises to nothing.
const FALLBACK_DIR_NAME: &str = "asset";

/// Voxel lighting table used by the production unit atlas. Loading a different
/// one changes every shaded colour, so the default matches what the game path
/// reads rather than whatever `.vpl` happens to resolve first.
const DEFAULT_VPL: &str = "VOXELS.VPL";

/// Rasteriser output byte meaning "no voxel rasterised at this pixel".
const VOXEL_EMPTY_INDEX: u8 = 0;

/// One full turn is 256 facing bytes: `0x00` = N, `0x40` = E, `0x80` = S,
/// `0xC0` = W. Every facing conversion in this module derives from that.
const FACINGS_PER_TURN: u32 = 256;

/// Degrees in one full turn, for the human-readable column in `index.tsv`.
const DEGREES_PER_TURN: u32 = 360;

/// The eight compass points in increasing facing order from north.
const COMPASS_NAMES: [&str; 8] = ["N", "NE", "E", "SE", "S", "SW", "W", "NW"];

/// Facing bytes between adjacent compass points, i.e. `256 / 8`.
const COMPASS_STEP: u8 = (FACINGS_PER_TURN / COMPASS_NAMES.len() as u32) as u8;

/// Suffixes the production compositor appends to an image id when it looks for
/// the separately-modelled gun parts, as gamemd's voxel loader formats them.
const LAYER_SUFFIXES: [&str; 2] = ["TUR", "BARL"];

/// Cyan: the sprite's own bounds, so an off-centre or clipped model is visible.
const SPRITE_OUTLINE_COLOR: [u8; 4] = [0, 200, 255, 255];
/// Yellow: where the model's centre projects. It should stay put as the facing
/// turns; a centre that walks across the sheet is a transform bug.
const MODEL_CENTRE_COLOR: [u8; 4] = [255, 230, 0, 255];
/// Crosshair arm length in unscaled sprite pixels. Short, because the marker
/// covers model pixels and a long arm would hide a small unit entirely.
const MODEL_CENTRE_ARM: u32 = 3;

/// Bound on a marker coordinate. Far beyond any canvas, but small enough that
/// the drawing helpers' own `x + arm` arithmetic cannot overflow `i64`.
const MARKER_COORD_LIMIT: i64 = 1 << 24;

/// Integer upscale bounds. 1 is "no scaling"; the ceiling keeps a large model
/// from turning into a gigapixel PNG.
const MIN_SCALE: u32 = 1;
const MAX_SCALE: u32 = 16;

/// Per-image pixel budget (~256 MB of RGBA), matching the SHP path.
const MAX_OUTPUT_PIXELS: u64 = 64_000_000;

/// Facings named in a summarising warning before it stops listing.
const MAX_LISTED_FACINGS_IN_WARNING: usize = 8;

/// Rules files searched for `[Colors]`, YR first per the INI authority order.
const RULES_INI_CANDIDATES: [&str; 2] = ["rulesmd.ini", "rules.ini"];

/// Substrings that mark a palette reason as the last-resort archive scan.
/// Matched case-insensitively so a wording change downgrades the warning rather
/// than breaking the build.
const LAST_RESORT_REASON_MARKERS: [&str; 5] = [
    "768",
    "last-resort",
    "last resort",
    "archive scan",
    "scanned",
];

/// One rendered facing's row in the TSV sidecar.
#[derive(Debug, Clone, PartialEq, Eq)]
struct FacingRow {
    facing: u8,
    w: u32,
    h: u32,
    png: String,
}

/// Result of applying `--facings` / `--limit`.
#[derive(Debug, Clone, PartialEq, Eq)]
struct FacingSelection {
    facings: Vec<u8>,
    /// Distinct facings asked for, before the limit. This is the report's
    /// `frame_count`.
    requested: usize,
    /// Facings the limit excluded. Non-zero means the render is partial.
    dropped: usize,
}

/// One facing rendered at 1:1, held until the run-wide scale is known.
struct RenderedFacing {
    facing: u8,
    image: Rgba,
    opaque_pixels: usize,
}

/// Render a voxel model to one PNG per facing plus a machine-readable report.
///
/// Errors are values, not panics: a malformed retail asset yields an
/// [`ErrorReport`] with a hint naming the verb or flag that resolves it.
pub fn run(
    asset_manager: &AssetManager,
    dict: &NameDict,
    art_registry: &ArtRegistry,
    name: &str,
    opts: &RenderOptions,
) -> Result<RenderReport, ErrorReport> {
    let resolved =
        crate::asset_tools::locate::locate(asset_manager, name).ok_or_else(|| ErrorReport {
            error: format!("asset not found: {name}"),
            hint: Some(format!(
                "run `asset find {name}` to see whether any archive holds it"
            )),
        })?;
    let source_archive = resolved.source_archive.clone();
    let catalog_warning = resolved.catalog_warning();
    let bytes = resolved.bytes;

    let identified = identify::identify(bytes);
    if identified.format != FORMAT_VXL {
        return Err(ErrorReport {
            error: format!(
                "{name} is {} ({}), not a voxel model",
                identified.format, identified.detail
            ),
            hint: Some(format!(
                "`asset info {name}` reports every parsed format without rendering"
            )),
        });
    }

    let vxl = VxlFile::from_bytes(bytes).map_err(|err| ErrorReport {
        error: format!("VXL parse failed for {name}: {err}"),
        hint: Some(format!(
            "`asset info {name}` reports the header fields that were readable"
        )),
    })?;

    let mut warnings: Vec<String> = Vec::new();
    warnings.extend(catalog_warning);
    if opts.crop {
        warnings.push(
            "--crop has no effect on a voxel render: the sprite is already the model's own \
             bounding box, not a sub-rect of a declared canvas"
                .to_string(),
        );
    }

    // Companion assets are addressed by the model's image id, not by the string
    // the caller typed — `ecache01.mix/htnk.vxl` still pairs with `HTNK.HVA`.
    let image = image_id(name);

    let (hva, hva_note) = load_hva(asset_manager, &image);
    warnings.extend(hva_note);
    let (vpl, vpl_note) = load_vpl(asset_manager, opts.vpl.as_deref());
    warnings.extend(vpl_note);
    warnings.extend(sibling_layer_warnings(asset_manager, &image));

    let hva_frame_count = hva.as_ref().map_or(1, |file| file.frame_count);
    let (hva_frame, frame_note) = select_hva_frame(opts.frame, hva_frame_count);
    warnings.extend(frame_note);

    let selection = select_facings(&opts.facings, opts.limit);
    if selection.dropped > 0 {
        warnings.push(format!(
            "rendered {} of {} facings; {} dropped by --limit {}",
            selection.facings.len(),
            selection.requested,
            selection.dropped,
            opts.limit.max(1)
        ));
    }

    let inference = palette::infer(
        asset_manager,
        dict,
        art_registry,
        Some(name),
        &source_archive,
        opts.palette.as_deref(),
    );
    let Some(load) = inference.chosen else {
        let tried = inference
            .candidates
            .iter()
            .map(|candidate| candidate.name.as_str())
            .collect::<Vec<_>>()
            .join(", ");
        return Err(ErrorReport {
            error: format!("no palette resolved for {name}"),
            hint: Some(if tried.is_empty() {
                format!(
                    "pass `--palette <FILE.PAL>`; `asset palette-for {name}` shows the inference chain"
                )
            } else {
                format!("pass `--palette <FILE.PAL>`; candidates tried: {tried}")
            }),
        });
    };

    let choice = load.choice();
    if is_last_resort_palette(&load.reason) {
        warnings.push(format!(
            "palette `{}` came from the last-resort archive scan ({}) — it renders, which is not \
             evidence it is the right palette",
            load.name, load.reason
        ));
    }

    // Rules are only parsed for --house: reading rulesmd.ini is a real startup
    // cost that a plain render must not pay. Voxel colours sit almost entirely
    // in the remap band, so this flag matters more here than for a sprite.
    let render_palette: Palette = match opts.house {
        Some(index) => apply_house_color(asset_manager, &load.palette, index, &mut warnings)?,
        None => load.palette,
    };

    // Render every facing at 1:1 first: one run-wide scale is what makes the
    // contact sheet honest about relative size, and it cannot be chosen until
    // the largest sprite is known.
    let mut rendered: Vec<RenderedFacing> = Vec::with_capacity(selection.facings.len());
    let mut unconverted: Vec<u8> = Vec::new();
    for &facing in &selection.facings {
        let params = VxlRenderParams {
            frame: hva_frame,
            facing,
            ..Default::default()
        };
        // Composite body + turret + barrel through the same CPU path the atlas
        // bake uses, so a tank is drawn with its gun rather than without it.
        let sprite = composite_unit_vxl_cpu(
            asset_manager,
            &vxl,
            hva.as_ref(),
            &image,
            &params,
            vpl.as_ref(),
        );

        let converted = sprite_to_rgba(&sprite, &render_palette);
        if converted.is_none() {
            unconverted.push(facing);
        }
        let opaque_pixels = converted
            .as_ref()
            .map_or(0, |image| count_opaque(&image.data));
        rendered.push(RenderedFacing {
            facing,
            image: compose_facing_image(&sprite, converted.as_ref()),
            opaque_pixels,
        });
    }

    if !unconverted.is_empty() {
        warnings.push(format!(
            "{} facing(s) produced a pixel buffer that did not match their declared size and were \
             drawn empty ({})",
            unconverted.len(),
            summarise_facings(&unconverted)
        ));
    }
    if rendered.is_empty() {
        return Err(ErrorReport {
            error: format!("no facing of {name} could be rendered"),
            hint: Some(
                "pass `--facings <BYTE,...>`; 0 32 64 96 128 160 192 224 are the compass points"
                    .to_string(),
            ),
        });
    }
    if rendered.iter().all(|entry| entry.opaque_pixels == 0) {
        warnings.push(
            "every facing rendered fully transparent — the model holds no voxels the rasteriser \
             could place, so these PNGs show only the checkerboard"
                .to_string(),
        );
    }

    let canvas_w = rendered.iter().map(|f| f.image.w).max().unwrap_or(1);
    let canvas_h = rendered.iter().map(|f| f.image.h).max().unwrap_or(1);
    let requested_scale = effective_scale(opts.scale, canvas_w, canvas_h);
    let scale = fit_scale(canvas_w, canvas_h, requested_scale).ok_or_else(|| ErrorReport {
        error: format!(
            "sprite {canvas_w}x{canvas_h} exceeds the {MAX_OUTPUT_PIXELS}-pixel render budget"
        ),
        hint: Some("this usually means a corrupt VXL header; check `asset info`".to_string()),
    })?;
    if scale < requested_scale {
        warnings.push(format!(
            "scale reduced from {requested_scale} to {scale} to stay inside the render budget"
        ));
    }

    let sanitised = sanitise_name(name);
    let dir = render_dir(&opts.out, &sanitised);
    std::fs::create_dir_all(&dir).map_err(|err| ErrorReport {
        error: format!("could not create {}: {err}", dir.display()),
        hint: Some("pass a writable `--out` root".to_string()),
    })?;
    // A re-run must replace the previous render, not blend with it: an 8-facing
    // run after a 64-facing run would otherwise leave 56 stale PNGs that read as
    // current output.
    clear_stale_outputs(&dir, &sanitised);

    let mut rows: Vec<FacingRow> = Vec::with_capacity(rendered.len());
    let mut cells: Vec<SheetCell> = Vec::with_capacity(rendered.len());
    let mut frame_paths: Vec<String> = Vec::with_capacity(rendered.len());
    let mut facings_rendered: Vec<usize> = Vec::with_capacity(rendered.len());

    for entry in &rendered {
        let upscaled = canvas::upscale_nearest(&entry.image, scale);
        let png_name = facing_png_name(&sanitised, entry.facing);
        let png_path = dir.join(&png_name);
        canvas::save_png(&png_path, &upscaled).map_err(|err| ErrorReport {
            error: format!("could not write {}: {err}", png_path.display()),
            hint: Some("pass a writable `--out` root".to_string()),
        })?;

        rows.push(FacingRow {
            facing: entry.facing,
            w: entry.image.w,
            h: entry.image.h,
            png: png_name,
        });
        cells.push(SheetCell {
            label: cell_label(entry.facing, entry.image.w, entry.image.h),
            image: upscaled,
        });
        frame_paths.push(png_path.display().to_string());
        facings_rendered.push(usize::from(entry.facing));
    }

    let index_path = dir.join(INDEX_TSV_NAME);
    std::fs::write(&index_path, index_tsv(&rows)).map_err(|err| ErrorReport {
        error: format!("could not write {}: {err}", index_path.display()),
        hint: Some("pass a writable `--out` root".to_string()),
    })?;

    // One Read call showing eight facings beats eight Read calls, and a voxel
    // bug is usually only visible across facings anyway.
    let sheet_path = if cells.len() > 1 {
        let header = sheet_header(
            name,
            &source_archive,
            &vxl,
            hva.as_ref(),
            hva_frame,
            vpl.is_some(),
            rows.len(),
            selection.requested,
            scale,
            &choice.name,
            &choice.reason,
            opts.house,
        );
        let sheet = canvas::build_contact_sheet(&header, &cells);
        let path = dir.join(sheet_png_name(&sanitised));
        canvas::save_png(&path, &sheet).map_err(|err| ErrorReport {
            error: format!("could not write {}: {err}", path.display()),
            hint: Some("pass a writable `--out` root".to_string()),
        })?;
        Some(path.display().to_string())
    } else {
        None
    };

    Ok(RenderReport {
        kind: KIND_VXL.to_string(),
        asset: name.to_string(),
        source_archive,
        palette: Some(choice),
        house_color: opts.house,
        canvas: [canvas_w, canvas_h],
        frame_count: selection.requested,
        frames_rendered: facings_rendered,
        scale,
        mode: MODE_VOXEL.to_string(),
        warnings,
        outputs: RenderOutputs {
            dir: dir.display().to_string(),
            sheet: sheet_path,
            frames: frame_paths,
            index: Some(index_path.display().to_string()),
        },
    })
}

// ---------------------------------------------------------------------------
// Companion assets
// ---------------------------------------------------------------------------

/// Load `<IMAGE>.HVA`. A missing or unparsable HVA is a warning, never an
/// error: the rasteriser falls back to the limbs' own transforms, which is the
/// model's idle pose and is exactly what most retail vehicles ship anyway.
fn load_hva(asset_manager: &AssetManager, image_id: &str) -> (Option<HvaFile>, Option<String>) {
    let hva_name = format!("{image_id}.HVA");
    let Some(resolved) = crate::asset_tools::locate::locate(asset_manager, &hva_name) else {
        return (
            None,
            Some(format!(
                "no paired {hva_name} resolved — the idle pose was rendered from the limb \
                 transforms in the VXL itself"
            )),
        );
    };
    match HvaFile::from_bytes(resolved.bytes) {
        Ok(hva) => (Some(hva), None),
        Err(err) => (
            None,
            Some(format!(
                "{hva_name} failed to parse ({err}) — the idle pose was rendered from the limb \
                 transforms in the VXL itself"
            )),
        ),
    }
}

/// Load the voxel lighting table. Without it the rasteriser falls back to plain
/// N-dot-L shading, which is a visibly different — flatter, differently lit —
/// picture, so the fallback is always said out loud.
fn load_vpl(
    asset_manager: &AssetManager,
    override_name: Option<&str>,
) -> (Option<VplFile>, Option<String>) {
    let vpl_name = override_name
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .unwrap_or(DEFAULT_VPL);

    let Some(resolved) = crate::asset_tools::locate::locate(asset_manager, vpl_name) else {
        return (
            None,
            Some(format!(
                "voxel lighting table {vpl_name} did not resolve — these colours came from the \
                 fallback N-dot-L shading, which does not match the game's own voxel lighting; \
                 pass `--vpl <FILE.VPL>` to name one"
            )),
        );
    };
    match VplFile::from_bytes(resolved.bytes) {
        Ok(vpl) => (Some(vpl), None),
        Err(err) => (
            None,
            Some(format!(
                "{vpl_name} failed to parse ({err}) — these colours came from the fallback \
                 N-dot-L shading, which does not match the game's own voxel lighting"
            )),
        ),
    }
}

/// Report which turret/barrel layers were merged into the body sprite.
///
/// These images go through the same CPU compositor the atlas bake uses, so a
/// tank is drawn with its gun. Naming the layers tells a caller the image is a
/// composite rather than a bare hull. A vehicle type without `Turret=` loads no
/// gun parts, so the game draws that type's image bare.
///
/// The one asymmetry worth reporting: the compositor resolves siblings through
/// production lookup, which cannot reach catalogued archives, while this verb's
/// own `locate` can. A sibling visible to one and not the other is silently
/// absent from the picture, so it is called out rather than left to be noticed.
fn sibling_layer_warnings(asset_manager: &AssetManager, image_id: &str) -> Vec<String> {
    let mut composited: Vec<String> = Vec::new();
    let mut unreachable: Vec<String> = Vec::new();

    for suffix in LAYER_SUFFIXES {
        let candidate = format!("{image_id}{suffix}.VXL");
        if crate::asset_tools::locate::locate(asset_manager, &candidate).is_none() {
            continue;
        }
        // The compositor uses `get_ref`; match that reachability exactly.
        if asset_manager.get_ref(&candidate).is_some() {
            composited.push(candidate);
        } else {
            unreachable.push(candidate);
        }
    }

    let mut notes = Vec::new();
    if !composited.is_empty() {
        notes.push(format!(
            "composite: {} were depth-merged into the body, as the unit atlas bakes them for a \
             type with a turret; a vehicle type without Turret= draws the body alone",
            composited.join(", ")
        ));
    }
    if !unreachable.is_empty() {
        notes.push(format!(
            "{} exists but only in a catalogued archive that production lookup cannot reach, so \
             it is MISSING from these images — the game would not load it by this name either",
            unreachable.join(", ")
        ));
    }
    notes
}

/// `HTNK.VXL` -> `HTNK`, path prefixes and case normalised.
///
/// MIX entry names are flat, so the final dot is unambiguously the extension
/// separator and a name with no dot is already an image id.
fn image_id(name: &str) -> String {
    let base = name.rsplit(['/', '\\']).next().unwrap_or(name).trim();
    match base.rfind('.') {
        Some(dot) => &base[..dot],
        None => base,
    }
    .to_ascii_uppercase()
}

// ---------------------------------------------------------------------------
// Facings
// ---------------------------------------------------------------------------

/// The eight compass points, derived from the facing-byte convention rather
/// than written out: one point every `256 / 8` bytes starting at north.
fn default_facings() -> Vec<u8> {
    (0..COMPASS_NAMES.len())
        .map(|point| (point as u8).saturating_mul(COMPASS_STEP))
        .collect()
}

/// Apply `--facings` / `--limit`.
///
/// Duplicates are dropped: two identical facings would write the same PNG twice
/// and emit two identical `index.tsv` rows, which reads as a rendering bug.
fn select_facings(requested: &[u8], limit: usize) -> FacingSelection {
    let source: Vec<u8> = if requested.is_empty() {
        default_facings()
    } else {
        requested.to_vec()
    };

    let mut unique: Vec<u8> = Vec::with_capacity(source.len());
    for facing in source {
        if !unique.contains(&facing) {
            unique.push(facing);
        }
    }

    let requested_count = unique.len();
    // A zero limit would render nothing and report success; treat it as one.
    let take = limit.max(1).min(requested_count);
    unique.truncate(take);
    FacingSelection {
        facings: unique,
        requested: requested_count,
        dropped: requested_count - take,
    }
}

/// Compass point for a facing byte, only when it lands exactly on one.
///
/// Exact-only on purpose: labelling facing 70 as "E" would be a small lie
/// burned into an image, and the degrees column already places it.
fn compass_name(facing: u8) -> Option<&'static str> {
    if !facing.is_multiple_of(COMPASS_STEP) {
        return None;
    }
    COMPASS_NAMES
        .get(usize::from(facing / COMPASS_STEP))
        .copied()
}

/// Facing byte to whole degrees, truncated. Exact for every multiple of 32,
/// which is every facing the default set uses.
fn facing_degrees(facing: u8) -> u32 {
    u32::from(facing) * DEGREES_PER_TURN / FACINGS_PER_TURN
}

/// HVA frame selected by `--frame`, clamped to what the animation holds.
///
/// For a voxel, `--frame` is an HVA transform frame rather than a sprite index.
/// Most retail vehicles ship a single-frame HVA, so this only bites on the few
/// that animate.
fn select_hva_frame(requested: Option<usize>, frame_count: u32) -> (u32, Option<String>) {
    let available = frame_count.max(1);
    let Some(index) = requested else {
        return (0, None);
    };
    match u32::try_from(index) {
        Ok(frame) if frame < available => (frame, None),
        _ => (
            0,
            Some(format!(
                "--frame {index} is outside the {available} HVA frame(s) this model has; frame 0 \
                 was rendered instead"
            )),
        ),
    }
}

// ---------------------------------------------------------------------------
// Pixels
// ---------------------------------------------------------------------------

/// Palette indices to RGBA, with the rasteriser's empty byte forced clear.
///
/// Index 0 is "no voxel", not "palette colour 0". Every palette entry is
/// opaque, index 0 included, so trusting the palette would fill the sprite's
/// whole bounding box with a solid colour.
///
/// The caller bounds the slice length; this walks it once and allocates 4x.
fn indices_to_rgba(indices: &[u8], palette: &Palette) -> Vec<u8> {
    let mut data: Vec<u8> = Vec::with_capacity(indices.len() * 4);
    for &index in indices {
        if index == VOXEL_EMPTY_INDEX {
            data.extend_from_slice(&[0, 0, 0, 0]);
            continue;
        }
        let color = palette.color(index);
        data.extend_from_slice(&[color.r, color.g, color.b, 255]);
    }
    data
}

/// Convert one rasterised sprite. `None` when the buffer does not match the
/// declared size, which a corrupt model can produce and which must not panic.
fn sprite_to_rgba(sprite: &VxlSprite, palette: &Palette) -> Option<Rgba> {
    let expected = (sprite.width as usize).checked_mul(sprite.height as usize)?;
    if expected == 0 || sprite.palette_indices.len() != expected {
        return None;
    }
    // Guard the 4x expansion before allocating: on a 32-bit host the product
    // can overflow even though the source buffer exists.
    expected.checked_mul(4)?;
    Rgba::from_raw(
        indices_to_rgba(&sprite.palette_indices, palette),
        sprite.width,
        sprite.height,
    )
}

fn count_opaque(rgba: &[u8]) -> usize {
    rgba.as_chunks::<4>()
        .0
        .iter()
        .filter(|pixel| pixel[3] != 0)
        .count()
}

/// Composite one facing onto the checkerboard and draw its markers.
///
/// Markers live inside the sprite bounds so the PNG stays exactly
/// `sprite * scale`. The rasteriser leaves a two-pixel margin around the model,
/// so the bounds outline costs background rather than voxels.
fn compose_facing_image(sprite: &VxlSprite, converted: Option<&Rgba>) -> Rgba {
    let w = sprite.width.max(1);
    let h = sprite.height.max(1);

    // Checkerboard, so an empty pixel is distinguishable from a black voxel.
    let mut image = Rgba::checkerboard(w, h);
    if let Some(src) = converted {
        canvas::blit_over(&mut image, src, 0, 0);
    }
    canvas::draw_rect_outline(&mut image, 0, 0, w, h, SPRITE_OUTLINE_COLOR);

    // `offset_*` is the model-centre -> sprite-top-left vector, so negating it
    // puts the centre back in sprite pixel coordinates.
    canvas::draw_crosshair(
        &mut image,
        marker_coord(sprite.offset_x),
        marker_coord(sprite.offset_y),
        MODEL_CENTRE_ARM,
        MODEL_CENTRE_COLOR,
    );
    image
}

/// One rasteriser offset, negated and rounded into a drawable coordinate.
///
/// A degenerate model can hand back NaN or an infinity. A float `as` cast
/// saturates and maps NaN to zero rather than wrapping, so it cannot produce
/// nonsense on its own — but a saturated `i64::MAX` would then overflow the
/// `x + arm` inside the crosshair helper, which panics in a debug build. The
/// clamp exists for that, not for the cast.
fn marker_coord(offset: f32) -> i64 {
    ((-offset).round() as i64).clamp(-MARKER_COORD_LIMIT, MARKER_COORD_LIMIT)
}

// ---------------------------------------------------------------------------
// House colours
// ---------------------------------------------------------------------------

/// Build the house-colour remap band from the ruleset's `[Colors]` list.
///
/// Mirrors the SHP render path deliberately: the ramps come from the INI every
/// time, because a hardcoded retail colour table would be a second source of
/// truth that a rules edit silently invalidates.
fn apply_house_color(
    asset_manager: &AssetManager,
    base: &Palette,
    index: u8,
    warnings: &mut Vec<String>,
) -> Result<Palette, ErrorReport> {
    let Some((ini_name, ini_bytes)) = RULES_INI_CANDIDATES.iter().find_map(|candidate| {
        asset_manager
            .get(candidate)
            .map(|bytes| (*candidate, bytes))
    }) else {
        return Err(ErrorReport {
            error: "--house needs the ruleset, but neither rulesmd.ini nor rules.ini resolved"
                .to_string(),
            hint: Some("drop --house to render with the unremapped palette".to_string()),
        });
    };

    let ini = IniFile::from_bytes(&ini_bytes).map_err(|err| ErrorReport {
        error: format!("could not parse {ini_name} for --house: {err}"),
        hint: Some("drop --house to render with the unremapped palette".to_string()),
    })?;
    let schemes = parse_color_schemes(&ini);
    if schemes.is_empty() {
        return Err(ErrorReport {
            error: format!(
                "{ini_name} has no usable `[Colors]` entries, so --house cannot resolve"
            ),
            hint: Some("drop --house to render with the unremapped palette".to_string()),
        });
    }
    if usize::from(index) >= schemes.len() {
        warnings.push(format!(
            "--house {index} is outside the {} `[Colors]` entries in {ini_name}; the default \
             scheme ramp was used",
            schemes.len()
        ));
    }

    let ramps = HouseColorRamps::from_schemes(&schemes);
    Ok(base.with_house_colors(ramps.ramp(HouseColorIndex(index))))
}

// ---------------------------------------------------------------------------
// Labels and sidecars
// ---------------------------------------------------------------------------

/// `f64 90deg E 48x42` — the facing byte, its angle, its compass point when it
/// has one, and the unscaled sprite size.
///
/// Restricted to letters, digits and spaces on purpose: the built-in 5x7 glyph
/// table covers only space, `-`, `:`, `/`, digits and both letter cases, and a
/// missing glyph advances the pen without drawing anything.
fn cell_label(facing: u8, w: u32, h: u32) -> String {
    let degrees = facing_degrees(facing);
    match compass_name(facing) {
        Some(point) => format!("f{facing} {degrees}deg {point} {w}x{h}"),
        None => format!("f{facing} {degrees}deg {w}x{h}"),
    }
}

/// Header lines printed above the contact sheet. An agent that only looks at
/// the image must still know which palette and which lighting table produced
/// these colours.
fn sheet_header(
    name: &str,
    source_archive: &str,
    vxl: &VxlFile,
    hva: Option<&HvaFile>,
    hva_frame: u32,
    has_vpl: bool,
    rendered: usize,
    requested: usize,
    scale: u32,
    palette_name: &str,
    palette_reason: &str,
    house: Option<u8>,
) -> Vec<String> {
    let lighting = if has_vpl {
        "vpl lighting"
    } else {
        "fallback N-dot-L shading"
    };
    let animation = match hva {
        Some(file) => format!("hva {} frames at frame {hva_frame}", file.frame_count),
        None => "no hva".to_string(),
    };
    let mut header = vec![
        format!("{name}   {source_archive}"),
        format!(
            "limbs {}   facings {rendered}/{requested}   scale {scale}x   mode {MODE_VOXEL}",
            vxl.limb_count
        ),
        format!("{animation}   {lighting}"),
        format!("palette {palette_name}   {palette_reason}"),
    ];
    if let Some(index) = house {
        header.push(format!(
            "house colour scheme {index} applied to indices 16 to 32"
        ));
    }
    header.push("composite body turret barrel   cyan bounds   yellow model centre".to_string());
    header
}

fn index_tsv_row(row: &FacingRow) -> String {
    format!(
        "{}\t{}\t{}\t{}\t{}",
        row.facing,
        facing_degrees(row.facing),
        row.w,
        row.h,
        row.png
    )
}

fn index_tsv(rows: &[FacingRow]) -> String {
    let mut text = String::with_capacity(INDEX_TSV_HEADER.len() + rows.len() * 32);
    text.push_str(INDEX_TSV_HEADER);
    text.push('\n');
    for row in rows {
        text.push_str(&index_tsv_row(row));
        text.push('\n');
    }
    text
}

// ---------------------------------------------------------------------------
// Output paths
// ---------------------------------------------------------------------------

/// Replace every character outside `[A-Za-z0-9._-]` so the name is safe as a
/// directory and filename component on any platform.
fn sanitise_name(name: &str) -> String {
    let sanitised: String = name
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || matches!(character, '.' | '_' | '-') {
                character
            } else {
                '_'
            }
        })
        .collect();
    if sanitised.is_empty() {
        FALLBACK_DIR_NAME.to_string()
    } else {
        sanitised
    }
}

/// `<out>/render/<sanitised>/`, absolutised so the reported paths do not depend
/// on the reader's working directory.
fn render_dir(out: &Path, sanitised: &str) -> PathBuf {
    let root = std::path::absolute(out).unwrap_or_else(|_| out.to_path_buf());
    root.join(RENDER_SUBDIR).join(sanitised)
}

/// `<sanitised>.<facing:03>.png`. Three digits keeps all 256 facings sorted in
/// a file listing, and matches the SHP path's shape so one cleanup rule covers
/// both.
fn facing_png_name(sanitised: &str, facing: u8) -> String {
    format!("{sanitised}.{facing:03}.png")
}

fn sheet_png_name(sanitised: &str) -> String {
    format!("{sanitised}.sheet.png")
}

/// True for a file this verb itself wrote for `sanitised`, so a regeneration
/// clears only its own previous output.
fn is_generated_output(file_name: &str, sanitised: &str) -> bool {
    let Some(rest) = file_name.strip_prefix(sanitised) else {
        return false;
    };
    let Some(rest) = rest.strip_prefix('.') else {
        return false;
    };
    let Some(stem) = rest.strip_suffix(".png") else {
        return false;
    };
    stem == "sheet" || (!stem.is_empty() && stem.chars().all(|c| c.is_ascii_digit()))
}

/// Remove the previous run's PNGs. Failures are ignored: a stale file is a
/// nuisance, a hard error here would block an otherwise good render.
fn clear_stale_outputs(dir: &Path, sanitised: &str) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let file_name = entry.file_name();
        let Some(file_name) = file_name.to_str() else {
            continue;
        };
        if is_generated_output(file_name, sanitised) {
            let _ = std::fs::remove_file(entry.path());
        }
    }
}

// ---------------------------------------------------------------------------
// Scale
// ---------------------------------------------------------------------------

fn clamp_scale(scale: u32) -> u32 {
    scale.clamp(MIN_SCALE, MAX_SCALE)
}

fn effective_scale(requested: Option<u32>, w: u32, h: u32) -> u32 {
    clamp_scale(requested.unwrap_or_else(|| canvas::choose_scale(w, h)))
}

/// Largest scale <= `requested` that keeps one image inside the pixel budget.
/// `None` when even 1x is too large, i.e. the model itself is unusable.
fn fit_scale(w: u32, h: u32, requested: u32) -> Option<u32> {
    let area = u64::from(w.max(1)) * u64::from(h.max(1));
    if area > MAX_OUTPUT_PIXELS {
        return None;
    }
    let mut scale = clamp_scale(requested);
    while scale > MIN_SCALE && area * u64::from(scale) * u64::from(scale) > MAX_OUTPUT_PIXELS {
        scale -= 1;
    }
    Some(scale)
}

/// Matched case-insensitively against `PaletteLoad::reason`.
fn is_last_resort_palette(reason: &str) -> bool {
    let lowered = reason.to_ascii_lowercase();
    LAST_RESORT_REASON_MARKERS
        .iter()
        .any(|marker| lowered.contains(marker))
}

fn summarise_facings(facings: &[u8]) -> String {
    let listed: Vec<String> = facings
        .iter()
        .take(MAX_LISTED_FACINGS_IN_WARNING)
        .map(u8::to_string)
        .collect();
    if facings.len() > MAX_LISTED_FACINGS_IN_WARNING {
        format!(
            "{}, +{} more",
            listed.join(", "),
            facings.len() - MAX_LISTED_FACINGS_IN_WARNING
        )
    } else {
        listed.join(", ")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Palette entries are opaque, index 0 included, so a test that sees a
    /// transparent index-0 pixel proves the voxel rule fired.
    fn opaque_zero_palette() -> Palette {
        let mut bytes = vec![0u8; 768];
        // Index 0: mid grey. Index 1: pure red. Index 2: pure green.
        bytes[0..3].copy_from_slice(&[32, 32, 32]);
        bytes[3..6].copy_from_slice(&[63, 0, 0]);
        bytes[6..9].copy_from_slice(&[0, 63, 0]);
        Palette::from_bytes(&bytes).expect("768 bytes parse")
    }

    fn sprite(indices: Vec<u8>, w: u32, h: u32) -> VxlSprite {
        VxlSprite {
            depth: vec![f32::NEG_INFINITY; indices.len()],
            palette_indices: indices,
            width: w,
            height: h,
            offset_x: 0.0,
            offset_y: 0.0,
        }
    }

    #[test]
    fn compass_points_follow_the_facing_byte_convention() {
        // The four cardinals are the anchors the coordinate reference uses.
        assert_eq!(compass_name(0x00), Some("N"));
        assert_eq!(compass_name(0x40), Some("E"));
        assert_eq!(compass_name(0x80), Some("S"));
        assert_eq!(compass_name(0xC0), Some("W"));
        // The intercardinals sit halfway between them.
        assert_eq!(compass_name(0x20), Some("NE"));
        assert_eq!(compass_name(0x60), Some("SE"));
        assert_eq!(compass_name(0xA0), Some("SW"));
        assert_eq!(compass_name(0xE0), Some("NW"));
        // Anything off the eight points is not labelled at all.
        assert_eq!(compass_name(0x01), None);
        assert_eq!(compass_name(0x3F), None);
        assert_eq!(compass_name(0xFF), None);
    }

    #[test]
    fn facing_degrees_are_exact_on_the_compass_points() {
        assert_eq!(facing_degrees(0x00), 0);
        assert_eq!(facing_degrees(0x20), 45);
        assert_eq!(facing_degrees(0x40), 90);
        assert_eq!(facing_degrees(0x60), 135);
        assert_eq!(facing_degrees(0x80), 180);
        assert_eq!(facing_degrees(0xC0), 270);
        assert_eq!(facing_degrees(0xE0), 315);
        // Off-point facings truncate rather than wrapping past a full turn.
        assert_eq!(facing_degrees(0xFF), 358);
    }

    #[test]
    fn selection_defaults_to_the_compass_and_honours_an_explicit_list() {
        let default = select_facings(&[], 64);
        assert_eq!(default.facings, default_facings());
        assert_eq!(default.requested, 8);
        assert_eq!(default.dropped, 0);

        let explicit = select_facings(&[64, 192], 64);
        assert_eq!(explicit.facings, vec![64, 192]);
        assert_eq!(explicit.requested, 2);
    }

    #[test]
    fn selection_drops_duplicates_and_applies_the_limit() {
        // A duplicate would overwrite its own PNG and duplicate a TSV row.
        let deduped = select_facings(&[0, 64, 0, 64, 128], 64);
        assert_eq!(deduped.facings, vec![0, 64, 128]);
        assert_eq!(deduped.requested, 3);
        assert_eq!(deduped.dropped, 0);

        let capped = select_facings(&[], 3);
        assert_eq!(capped.facings, vec![0, 32, 64]);
        assert_eq!(capped.requested, 8);
        assert_eq!(capped.dropped, 5);

        // A zero limit must not silently render nothing.
        let zero = select_facings(&[], 0);
        assert_eq!(zero.facings, vec![0]);
        assert_eq!(zero.dropped, 7);
    }

    #[test]
    fn index_zero_is_transparent_regardless_of_the_palette() {
        let palette = opaque_zero_palette();
        // The palette itself reports index 0 as opaque mid grey.
        assert_eq!(palette.color(0).a, 255);

        let rgba = indices_to_rgba(&[0, 1, 2, 0], &palette);
        assert_eq!(rgba.len(), 16);
        assert_eq!(&rgba[0..4], &[0, 0, 0, 0], "empty voxel must be clear");
        assert_eq!(rgba[7], 255, "index 1 is opaque");
        assert!(
            rgba[4] > 0 && rgba[5] == 0,
            "index 1 is red: {:?}",
            &rgba[4..8]
        );
        assert!(
            rgba[9] > 0 && rgba[8] == 0,
            "index 2 is green: {:?}",
            &rgba[8..12]
        );
        assert_eq!(
            &rgba[12..16],
            &[0, 0, 0, 0],
            "trailing empty voxel is clear"
        );
    }

    #[test]
    fn sprite_conversion_rejects_a_buffer_that_does_not_match_its_size() {
        let palette = opaque_zero_palette();
        assert!(sprite_to_rgba(&sprite(vec![1; 6], 3, 2), &palette).is_some());
        // Short buffer: a corrupt model must degrade, not panic.
        assert!(sprite_to_rgba(&sprite(vec![1; 5], 3, 2), &palette).is_none());
        assert!(sprite_to_rgba(&sprite(vec![1; 7], 3, 2), &palette).is_none());
        // Degenerate dimensions.
        assert!(sprite_to_rgba(&sprite(Vec::new(), 0, 0), &palette).is_none());
    }

    #[test]
    fn composing_a_facing_keeps_the_sprite_dimensions() {
        let palette = opaque_zero_palette();
        let model = sprite(vec![0, 1, 2, 0, 1, 0], 3, 2);
        let converted = sprite_to_rgba(&model, &palette);
        let image = compose_facing_image(&model, converted.as_ref());
        assert_eq!((image.w, image.h), (3, 2));
        assert_eq!(image.data.len(), image.pixel_count() * 4);

        // A model that failed to convert still yields a canvas of the right size.
        let empty = compose_facing_image(&model, None);
        assert_eq!((empty.w, empty.h), (3, 2));
    }

    #[test]
    fn marker_coordinates_are_negated_rounded_and_bounded() {
        // The marker is the model centre, i.e. the negated sprite offset.
        assert_eq!(marker_coord(-12.0), 12);
        assert_eq!(marker_coord(12.4), -12);
        assert_eq!(marker_coord(0.0), 0);
        // Non-finite offsets must not reach the drawing helpers' arithmetic.
        assert_eq!(marker_coord(f32::NAN), 0);
        assert_eq!(marker_coord(f32::NEG_INFINITY), MARKER_COORD_LIMIT);
        assert_eq!(marker_coord(f32::INFINITY), -MARKER_COORD_LIMIT);
    }

    #[test]
    fn composing_tolerates_a_zero_sized_and_a_non_finite_sprite() {
        let mut degenerate = sprite(Vec::new(), 0, 0);
        degenerate.offset_x = f32::NAN;
        degenerate.offset_y = f32::NEG_INFINITY;
        let image = compose_facing_image(&degenerate, None);
        assert_eq!((image.w, image.h), (1, 1));
        assert_eq!(image.data.len(), 4);
    }

    #[test]
    fn index_tsv_rows_carry_the_facing_its_angle_and_the_filename() {
        let rows = vec![
            FacingRow {
                facing: 0,
                w: 48,
                h: 42,
                png: "htnk.vxl.000.png".to_string(),
            },
            FacingRow {
                facing: 192,
                w: 50,
                h: 40,
                png: "htnk.vxl.192.png".to_string(),
            },
        ];
        assert_eq!(index_tsv_row(&rows[0]), "0\t0\t48\t42\thtnk.vxl.000.png");
        assert_eq!(
            index_tsv_row(&rows[1]),
            "192\t270\t50\t40\thtnk.vxl.192.png"
        );

        let text = index_tsv(&rows);
        let mut lines = text.lines();
        assert_eq!(lines.next(), Some(INDEX_TSV_HEADER));
        assert_eq!(lines.next(), Some(index_tsv_row(&rows[0]).as_str()));
        assert_eq!(lines.next(), Some(index_tsv_row(&rows[1]).as_str()));
        assert_eq!(lines.next(), None);
        assert!(text.ends_with('\n'));
        // Five columns, header included — a consumer splits on tabs.
        assert_eq!(INDEX_TSV_HEADER.split('\t').count(), 5);
        assert_eq!(index_tsv_row(&rows[0]).split('\t').count(), 5);
    }

    #[test]
    fn cell_labels_use_only_glyphs_the_bitmap_font_can_draw() {
        assert_eq!(cell_label(64, 48, 42), "f64 90deg E 48x42");
        assert_eq!(cell_label(70, 48, 42), "f70 98deg 48x42");
        // The 5x7 table covers space, -, :, / and both letter cases only;
        // anything else advances the pen and draws nothing.
        for facing in [0u8, 32, 70, 255] {
            let label = cell_label(facing, 100, 90);
            assert!(
                label
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == ' ' || c == '-' || c == ':'),
                "unprintable glyph in {label:?}"
            );
        }
    }

    #[test]
    fn hva_frame_is_clamped_to_what_the_animation_holds() {
        assert_eq!(select_hva_frame(None, 1), (0, None));
        assert_eq!(select_hva_frame(Some(0), 1), (0, None));
        assert_eq!(select_hva_frame(Some(3), 8), (3, None));

        let (frame, note) = select_hva_frame(Some(8), 8);
        assert_eq!(frame, 0);
        assert!(note.expect("out-of-range warns").contains("--frame 8"));

        // A model with no HVA still has the single idle pose.
        let (frame, note) = select_hva_frame(Some(1), 0);
        assert_eq!(frame, 0);
        assert!(note.is_some());
    }

    #[test]
    fn image_id_strips_the_extension_and_any_path() {
        assert_eq!(image_id("HTNK.VXL"), "HTNK");
        assert_eq!(image_id("htnk.vxl"), "HTNK");
        assert_eq!(image_id("htnk"), "HTNK");
        assert_eq!(image_id("ecache01.mix/htnk.vxl"), "HTNK");
        assert_eq!(image_id("a.b.vxl"), "A.B");
    }

    #[test]
    fn generated_output_predicate_matches_only_our_own_files() {
        assert!(is_generated_output("htnk.vxl.000.png", "htnk.vxl"));
        assert!(is_generated_output("htnk.vxl.224.png", "htnk.vxl"));
        assert!(is_generated_output("htnk.vxl.sheet.png", "htnk.vxl"));
        assert!(!is_generated_output("htnk.vxl.000.png", "mtnk.vxl"));
        assert!(!is_generated_output("index.tsv", "htnk.vxl"));
        assert!(!is_generated_output("htnk.vxl.notes.png", "htnk.vxl"));
        assert!(!is_generated_output("htnk.vxl.000.txt", "htnk.vxl"));
    }

    #[test]
    fn scale_is_clamped_and_fitted_to_the_pixel_budget() {
        assert_eq!(clamp_scale(0), MIN_SCALE);
        assert_eq!(clamp_scale(999), MAX_SCALE);
        assert_eq!(effective_scale(Some(4), 64, 64), 4);
        assert_eq!(fit_scale(60, 50, 8), Some(8));

        let fitted = fit_scale(4000, 4000, 16).expect("reduced, not refused");
        assert!((MIN_SCALE..16).contains(&fitted), "{fitted}");
        // A corrupt header claiming a giant sprite fails instead of allocating.
        assert_eq!(fit_scale(60_000, 60_000, 1), None);
        assert_eq!(fit_scale(0, 0, 2), Some(2));
    }

    #[test]
    fn sheet_header_names_the_palette_the_lighting_and_the_body_only_caveat() {
        let vxl = VxlFile {
            limb_count: 3,
            body_size: 0,
            palette: Vec::new(),
            limbs: Vec::new(),
        };
        let header = sheet_header(
            "HTNK.VXL",
            "ra2md.mix",
            &vxl,
            None,
            0,
            false,
            8,
            8,
            4,
            "unittem.pal",
            "fallback-chain",
            Some(2),
        );
        let joined = header.join("\n");
        assert!(joined.contains("HTNK.VXL"));
        assert!(joined.contains("ra2md.mix"));
        assert!(joined.contains("limbs 3"));
        assert!(joined.contains("facings 8/8"));
        assert!(joined.contains("scale 4x"));
        assert!(joined.contains("unittem.pal"));
        assert!(joined.contains("no hva"));
        assert!(joined.contains("N-dot-L"), "must flag the missing vpl");
        assert!(joined.contains("house colour scheme 2"));
        assert!(joined.contains("composite body turret barrel"));
    }
}
