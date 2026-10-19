//! `asset render` for the two still formats: PCX pictures and PAL palettes.
//!
//! Two renders that share a verb and almost nothing else.
//!
//! A **PCX** is already a finished picture and carries its own trailing VGA
//! table, so there is nothing to infer: [`run_pcx`] leaves
//! `RenderReport::palette` as `None` rather than running the inference chain,
//! because naming a `.pal` there would credit a file that had no part in the
//! colours — and a caller reading the report would have no way to tell the
//! difference. `--palette` is reported as ignored for the same reason.
//!
//! A **PAL** is not a picture at all, so [`run_pal`] draws a diagnostic
//! instrument rather than decoration: 256 labelled swatches in a 16x16 grid,
//! with the house-colour remap band `[16, 32)` outlined and named, because
//! "which indices remap" is the question a palette dump is opened to answer.
//! Index 0 is outlined too: the SHP blitters skip it, so its colour alone tells
//! a reader nothing. No other entry is keyed. With `--house` a second grid is
//! drawn with the ramp applied, plus a contact sheet holding both, so the
//! before and after can be compared in one image.
//!
//! Every burned-in label is built from the drawable glyph set only (see
//! [`label_text`]): the shared 5x7 font covers space, `-`, `:`, `/`, digits and
//! letters, and any other character advances the pen while drawing nothing —
//! so an unmapped character in a retail filename would silently vanish from the
//! header instead of looking wrong.
//!
//! ## Dependency rules
//! - Part of `asset_tools/`: depends on `assets/` (PCX, palette, archive
//!   resolution), `rules/` (`[Colors]` schemes for `--house`) and the sibling
//!   `canvas` / `identify` / `locate` / `palette` / `report` modules.
//! - Nothing from `sim/`, `render/`, `ui/`, `audio/`, or `net/`.

use std::path::{Path, PathBuf};

use crate::asset_tools::canvas::{self, Rgba, SheetCell};
use crate::asset_tools::identify;
use crate::asset_tools::report::{ErrorReport, PaletteChoice, RenderOutputs, RenderReport};
use crate::asset_tools::verb_render::RenderOptions;
use crate::assets::asset_manager::AssetManager;
use crate::assets::pal_file::{Color, Palette};
use crate::assets::pcx_file::PcxFile;
use crate::rules::color_scheme::parse_color_schemes;
use crate::rules::house_colors::{HouseColorIndex, HouseColorRamps};
use crate::rules::ini_parser::IniFile;

/// `RenderReport::kind` values owned by this module.
const KIND_PCX: &str = "pcx";
const KIND_PAL: &str = "pal";

/// `RenderReport::mode` values owned by this module.
const MODE_IMAGE: &str = "image";
const MODE_SWATCH: &str = "swatch";

/// Subdirectory under the output root, shared with the SHP render path so one
/// asset's output always lands in `<out>/render/<sanitised-name>/`.
const RENDER_SUBDIR: &str = "render";

/// Directory name used when the asset name sanitises to nothing.
const FALLBACK_DIR_NAME: &str = "asset";

/// Sidecar filename listing every palette entry.
const INDEX_TSV_NAME: &str = "index.tsv";

/// Header row of [`INDEX_TSV_NAME`]. `raw_*` are the stored 6-bit components,
/// `hex` is what the engine's `raw << 2` decode produces from them.
const INDEX_TSV_HEADER: &str = "index\trow\tcol\thex\traw_r\traw_g\traw_b\tremap_band\tmark";

/// Bytes of a .pal file: 256 entries x 3 components.
const PAL_FILE_BYTES: usize = 768;

/// Entries in a palette, and therefore swatches in the grid.
const PAL_COLORS: usize = 256;

/// Highest legal component in an unscaled VGA 6-bit palette.
const VGA_6BIT_MAX: u8 = 63;

/// House-colour remap band as a half-open `[start, end)` palette index range.
/// With 16 grid columns this is exactly the second row, which is why the band
/// outline can be one rectangle.
const HOUSE_REMAP_BAND: [usize; 2] = [16, 32];

/// Shades in one house-colour ramp — one per index in the remap band.
const HOUSE_RAMP_LEN: usize = 16;

/// Rules files searched for `[Colors]`, YR first per the INI authority order.
const RULES_INI_CANDIDATES: [&str; 2] = ["rulesmd.ini", "rules.ini"];

// --- Swatch grid layout -----------------------------------------------------

/// Columns and rows of the swatch grid. 16x16 puts the remap band on its own
/// row and keeps an index recoverable as `row * 16 + col` by eye.
const GRID_COLS: usize = 16;
const GRID_ROWS: usize = 16;

/// Side of one swatch. Wide enough for a three-digit index label plus inset,
/// which is what makes the grid readable without a magnifier.
const SWATCH_SIZE: u32 = 24;

/// Blank pixels between neighbouring swatches. Two, so a marker outline drawn
/// just outside a swatch never covers a neighbour's colour.
const SWATCH_GAP: u32 = 2;

/// Border around the whole grid. Must exceed the deepest band outline inset.
const GRID_MARGIN: u32 = 6;

/// Blank rows between the header block and the first swatch row.
const HEADER_GAP: u32 = 4;

/// Inset of the index label from its swatch's top-left corner.
const SWATCH_TEXT_INSET: i64 = 2;

/// The band outline is drawn at each of these distances outside the band row,
/// so it reads as a deliberate band rather than one more cell marker.
const BAND_OUTLINE_INSETS: [u32; 2] = [2, 3];

/// Flat backdrop; the gaps between swatches then read as grid lines.
const GRID_BACKGROUND: [u8; 4] = [24, 24, 24, 255];
const HEADER_COLOR: [u8; 4] = [255, 255, 255, 255];
/// Cyan: the house-colour remap band.
const REMAP_BAND_COLOR: [u8; 4] = [0, 200, 255, 255];
/// Yellow: index 0, transparent by convention.
const TRANSPARENT_MARKER_COLOR: [u8; 4] = [255, 230, 0, 255];
/// Index labels, picked per swatch by [`label_color_for`].
const SWATCH_LABEL_DARK: [u8; 4] = [0, 0, 0, 255];
const SWATCH_LABEL_LIGHT: [u8; 4] = [255, 255, 255, 255];

/// BT.601 luma weights, scaled by 1000 to stay in integer arithmetic.
const LUMA_R: u32 = 299;
const LUMA_G: u32 = 587;
const LUMA_B: u32 = 114;
const LUMA_DENOMINATOR: u32 = 1000;
/// Luma at or above this gets a dark label, below it a light one.
const LUMA_MIDPOINT: u32 = 128;

/// The grid is authored at a readable size already, so it is written 1:1 unless
/// the caller asks for more. Only `--scale` ever magnifies it.
const PAL_DEFAULT_SCALE: u32 = 1;

/// Integer upscale bounds, matching the SHP render path.
const MIN_SCALE: u32 = 1;
const MAX_SCALE: u32 = 16;

/// Per-image pixel budget (~256 MB of RGBA). A malformed header claiming a huge
/// canvas must fail with a report, not an allocation abort.
const MAX_OUTPUT_PIXELS: u64 = 64_000_000;

/// Characters outside the shared 5x7 glyph table become this. A 1:1 mapping is
/// deliberate — the label then still lines up character-for-character with the
/// real name, and nothing disappears.
const LABEL_SUBSTITUTE: char = '-';

// ---------------------------------------------------------------------------
// PCX
// ---------------------------------------------------------------------------

/// Render a PCX to a PNG plus a machine-readable report.
///
/// Errors are values, not panics: a malformed retail asset yields an
/// [`ErrorReport`] whose hint names the verb or flag that resolves it.
pub fn run_pcx(
    asset_manager: &AssetManager,
    name: &str,
    opts: &RenderOptions,
) -> Result<RenderReport, ErrorReport> {
    let resolved = locate_asset(asset_manager, name)?;
    let source_archive = resolved.source_archive.clone();
    let mut warnings: Vec<String> = Vec::new();
    warnings.extend(resolved.catalog_warning());

    let mut still = build_pcx_still(resolved.bytes, name, opts)?;
    warnings.append(&mut still.warnings);

    let sanitised = sanitise_name(name);
    let dir = prepare_output_dir(&opts.out, &sanitised)?;
    let png_path = dir.join(still_png_name(&sanitised));
    save(&png_path, &still.image)?;

    Ok(pcx_report(
        name,
        &source_archive,
        &still,
        RenderOutputs {
            dir: dir.display().to_string(),
            sheet: None,
            frames: vec![png_path.display().to_string()],
            index: None,
        },
        warnings,
    ))
}

/// One still image plus everything the report needs to describe it.
#[derive(Debug)]
struct Still {
    /// Already upscaled; its dimensions are exactly `canvas * scale`.
    image: Rgba,
    /// Pre-scale dimensions, which is what `RenderReport::canvas` carries.
    canvas: [u32; 2],
    scale: u32,
    warnings: Vec<String>,
}

/// Decode a PCX and compose the image, without touching the filesystem.
///
/// Split from [`run_pcx`] so the pixel path is exercisable from synthetic bytes
/// with no mounted retail install.
fn build_pcx_still(bytes: &[u8], name: &str, opts: &RenderOptions) -> Result<Still, ErrorReport> {
    let pcx = PcxFile::from_bytes(bytes).map_err(|err| {
        let identified = identify::identify(bytes);
        ErrorReport {
            error: format!(
                "PCX parse failed for {name} (sniffed as {}): {err}",
                identified.format
            ),
            hint: Some(format!(
                "`asset info {name}` reports the header fields that were readable"
            )),
        }
    })?;

    let mut warnings: Vec<String> = Vec::new();
    if opts.palette.is_some() {
        warnings.push(ignored(
            "--palette",
            "a PCX carries its own trailing VGA table, so no external palette is consulted",
        ));
    }
    if opts.house.is_some() {
        warnings.push(ignored(
            "--house",
            "PCX shell art is not house-remapped; the report's house_color stays null",
        ));
    }
    if opts.crop {
        warnings.push(ignored(
            "--crop",
            "a PCX is one image with no sub-rect inside a larger canvas",
        ));
    }
    if opts.frame.is_some_and(|frame| frame != 0) {
        warnings.push(ignored("--frame", "a PCX holds a single image"));
    }

    let w = u32::from(pcx.width);
    let h = u32::from(pcx.height);
    if w == 0 || h == 0 {
        return Err(ErrorReport {
            error: format!("PCX {name} decodes to a {w}x{h} image"),
            hint: Some(format!(
                "`asset info {name}` reports the window bounds the header declares"
            )),
        });
    }

    // The 3-plane form stores RGB triples and has no palette indices at all, so
    // a transparent index cannot apply to it. Length is the only discriminator
    // the parser exposes.
    let paletted = pcx.pixels.len() == (w as usize) * (h as usize);
    if let Some(index) = opts.transparent_index {
        if !paletted {
            warnings.push(ignored(
                "--transparent-index",
                "this is the 3-plane direct RGB form, which stores no palette indices",
            ));
        } else if !pcx.pixels.contains(&index) {
            warnings.push(format!(
                "no pixel in {name} uses index {index}, so nothing was made transparent"
            ));
        }
    }

    let rgba = pcx.to_rgba(opts.transparent_index);
    let decoded = Rgba::from_raw(rgba, w, h).ok_or_else(|| ErrorReport {
        error: format!("PCX {name} decoded to a pixel buffer that does not match {w}x{h}"),
        hint: Some(format!(
            "`asset info {name}` reports the declared dimensions"
        )),
    })?;

    // Checkerboard only when transparency was actually requested: without it
    // every pixel is opaque, and a backdrop that never shows through would only
    // enlarge the PNG.
    let composed = if opts.transparent_index.is_some() && paletted {
        let mut board = Rgba::checkerboard(w, h);
        canvas::blit_over(&mut board, &decoded, 0, 0);
        board
    } else {
        decoded
    };

    let requested = clamp_scale(opts.scale.unwrap_or_else(|| canvas::choose_scale(w, h)));
    let scale = fit_scale(w, h, requested).ok_or_else(|| ErrorReport {
        error: format!("PCX {name} at {w}x{h} exceeds the {MAX_OUTPUT_PIXELS}-pixel render budget"),
        hint: Some(format!(
            "this usually means a corrupt PCX header; check `asset info {name}`"
        )),
    })?;
    if scale < requested {
        warnings.push(format!(
            "scale reduced from {requested} to {scale} to stay inside the render budget"
        ));
    }

    Ok(Still {
        image: canvas::upscale_nearest(&composed, scale),
        canvas: [w, h],
        scale,
        warnings,
    })
}

fn pcx_report(
    name: &str,
    source_archive: &str,
    still: &Still,
    outputs: RenderOutputs,
    warnings: Vec<String>,
) -> RenderReport {
    RenderReport {
        kind: KIND_PCX.to_string(),
        asset: name.to_string(),
        source_archive: source_archive.to_string(),
        // Deliberately None — see the module header.
        palette: None,
        house_color: None,
        canvas: still.canvas,
        frame_count: 1,
        frames_rendered: vec![0],
        scale: still.scale,
        mode: MODE_IMAGE.to_string(),
        warnings,
        outputs,
    }
}

// ---------------------------------------------------------------------------
// PAL
// ---------------------------------------------------------------------------

/// Render a palette as a 16x16 swatch grid plus a machine-readable report.
///
/// With `--house` a second grid carrying the remapped band is written, along
/// with a contact sheet holding both.
pub fn run_pal(
    asset_manager: &AssetManager,
    name: &str,
    opts: &RenderOptions,
) -> Result<RenderReport, ErrorReport> {
    let resolved = locate_asset(asset_manager, name)?;
    let source_archive = resolved.source_archive.clone();
    let mut warnings: Vec<String> = Vec::new();
    warnings.extend(resolved.catalog_warning());
    let bytes = resolved.bytes;

    // Length *is* the format definition for a .pal, so it is checked here
    // rather than trusting the sniffer, whose PCX arm can claim a 768-byte file
    // that happens to open with the right four bytes.
    if bytes.len() != PAL_FILE_BYTES {
        let identified = identify::identify(bytes);
        return Err(ErrorReport {
            error: format!(
                "{name} is {} bytes and sniffs as {} ({}); a .pal is exactly {PAL_FILE_BYTES} bytes",
                bytes.len(),
                identified.format,
                identified.detail
            ),
            hint: Some(format!(
                "`asset info {name}` reports the sniffed format; SHP art renders through `asset render`"
            )),
        });
    }

    // Rules are parsed only for --house: it is a real startup cost that a plain
    // palette dump must not pay.
    let house = match opts.house {
        Some(index) => Some((
            index,
            resolve_house_ramp(asset_manager, index, &mut warnings)?,
        )),
        None => None,
    };

    let mut render = build_pal_render(bytes, name, &source_archive, opts, house)?;
    warnings.append(&mut render.warnings);

    let sanitised = sanitise_name(name);
    let dir = prepare_output_dir(&opts.out, &sanitised)?;

    let base_path = dir.join(grid_png_name(&sanitised));
    save(&base_path, &render.base)?;
    let mut frames = vec![base_path.display().to_string()];

    let mut sheet_path: Option<String> = None;
    if let (Some(house_image), Some((index, _))) = (render.house.as_ref(), house) {
        let house_path = dir.join(house_grid_png_name(&sanitised, index));
        save(&house_path, house_image)?;
        frames.push(house_path.display().to_string());

        // One image holding both grids: the whole point of --house here is the
        // comparison, and two files force the reader to flip between them.
        let sheet = canvas::build_contact_sheet(
            &comparison_sheet_header(name, &source_archive, index),
            &[
                SheetCell {
                    image: render.base.clone(),
                    label: label_text("raw palette"),
                },
                SheetCell {
                    image: house_image.clone(),
                    label: label_text(&format!("house {index} remap")),
                },
            ],
        );
        let path = dir.join(sheet_png_name(&sanitised));
        save(&path, &sheet)?;
        sheet_path = Some(path.display().to_string());
    }

    let index_path = dir.join(INDEX_TSV_NAME);
    std::fs::write(&index_path, pal_index_tsv(&render.rows)).map_err(|err| ErrorReport {
        error: format!("could not write {}: {err}", index_path.display()),
        hint: Some("pass a writable `--out` root".to_string()),
    })?;

    Ok(pal_report(
        name,
        &source_archive,
        &render,
        opts.house,
        RenderOutputs {
            dir: dir.display().to_string(),
            sheet: sheet_path,
            frames,
            index: Some(index_path.display().to_string()),
        },
        warnings,
    ))
}

/// Why one swatch is called out. Index 0 is never drawn (the SHP blitters skip
/// it), so its colour alone would mislead.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SwatchMark {
    None,
    /// Index 0: skipped by the SHP blitters.
    TransparentIndex0,
}

impl SwatchMark {
    fn marker_color(self) -> Option<[u8; 4]> {
        match self {
            Self::None => None,
            Self::TransparentIndex0 => Some(TRANSPARENT_MARKER_COLOR),
        }
    }

    /// Stable tag for the TSV sidecar.
    fn tag(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::TransparentIndex0 => "index0-transparent",
        }
    }
}

/// One palette entry as the TSV sidecar reports it.
#[derive(Debug, Clone, PartialEq, Eq)]
struct PalRow {
    index: usize,
    row: usize,
    col: usize,
    hex: String,
    raw: [u8; 3],
    remap_band: bool,
    mark: SwatchMark,
}

/// Both swatch grids plus the sidecar rows, without any filesystem contact.
#[derive(Debug)]
struct PalRender {
    base: Rgba,
    /// Present only when a house ramp was supplied.
    house: Option<Rgba>,
    canvas: [u32; 2],
    scale: u32,
    rows: Vec<PalRow>,
    warnings: Vec<String>,
}

/// Draw the swatch grid(s) for one palette.
///
/// `house` carries the `[Colors]` entry index and its ramp; the index is only
/// used to name the scheme in the second grid's header.
fn build_pal_render(
    bytes: &[u8],
    name: &str,
    source_archive: &str,
    opts: &RenderOptions,
    house: Option<(u8, [Color; HOUSE_RAMP_LEN])>,
) -> Result<PalRender, ErrorReport> {
    let palette = Palette::from_bytes(bytes).map_err(|err| ErrorReport {
        error: format!("PAL parse failed for {name}: {err}"),
        hint: Some(format!(
            "`asset info {name}` reports the entry's byte length"
        )),
    })?;

    let mut warnings: Vec<String> = Vec::new();
    if opts.palette.is_some() {
        warnings.push(ignored(
            "--palette",
            "the rendered asset is itself a palette",
        ));
    }
    if opts.crop {
        warnings.push(ignored("--crop", "a swatch grid has no frame sub-rect"));
    }
    if opts.frame.is_some() {
        warnings.push(ignored(
            "--frame",
            "every one of the 256 entries is drawn; the sidecar index.tsv lists them individually",
        ));
    }
    if opts.transparent_index.is_some() {
        warnings.push(ignored(
            "--transparent-index",
            "swatches are drawn opaque on purpose, with transparency shown by the marker outlines",
        ));
    }

    // Checked against the *source bytes*: the parser decodes components as
    // `raw << 2`, which both hides the 6-bit range and wraps for raw > 63, so
    // the parsed palette cannot answer this question.
    if bytes.iter().any(|&byte| byte > VGA_6BIT_MAX) {
        warnings.push(
            "palette has components above 63, so it is not an unscaled VGA 6-bit table; the \
             engine's `raw << 2` decode wraps those entries and the swatches show the wrapped \
             colours"
                .to_string(),
        );
    }

    let marks = swatch_marks();
    let rows = pal_rows(&palette, bytes, &marks);

    let base_header = pal_header(name, source_archive, None);
    let base_grid = draw_swatch_grid(&palette, &marks, &base_header);
    if base_grid.w == 0 || base_grid.h == 0 {
        return Err(ErrorReport {
            error: "the swatch grid exceeded the canvas allocation cap".to_string(),
            hint: Some("lower `--scale`".to_string()),
        });
    }
    let canvas_dims = [base_grid.w, base_grid.h];

    let requested = clamp_scale(opts.scale.unwrap_or(PAL_DEFAULT_SCALE));
    let scale =
        fit_scale(canvas_dims[0], canvas_dims[1], requested).ok_or_else(|| ErrorReport {
            error: format!(
                "the {}x{} swatch grid exceeds the {MAX_OUTPUT_PIXELS}-pixel render budget",
                canvas_dims[0], canvas_dims[1]
            ),
            hint: Some("lower `--scale`".to_string()),
        })?;
    if scale < requested {
        warnings.push(format!(
            "scale reduced from {requested} to {scale} to stay inside the render budget"
        ));
    }

    let house_grid = house.map(|(index, ramp)| {
        let remapped = palette.with_house_colors(&ramp);
        let header = pal_header(name, source_archive, Some(index));
        canvas::upscale_nearest(&draw_swatch_grid(&remapped, &marks, &header), scale)
    });

    Ok(PalRender {
        base: canvas::upscale_nearest(&base_grid, scale),
        house: house_grid,
        canvas: canvas_dims,
        scale,
        rows,
        warnings,
    })
}

fn pal_report(
    name: &str,
    source_archive: &str,
    render: &PalRender,
    house: Option<u8>,
    outputs: RenderOutputs,
    warnings: Vec<String>,
) -> RenderReport {
    RenderReport {
        kind: KIND_PAL.to_string(),
        asset: name.to_string(),
        source_archive: source_archive.to_string(),
        // The asset is the palette. Naming it here tells the reader which file
        // produced the swatch colours, which is the whole provenance question.
        palette: Some(PaletteChoice {
            name: name.to_string(),
            reason: "the rendered asset is itself the palette".to_string(),
            confidence: "declared".to_string(),
            // No engine path is being cited: the asset simply is the palette.
            production_site: None,
        }),
        house_color: house,
        canvas: render.canvas,
        frame_count: PAL_COLORS,
        // Indexes the swatch grids written, so it stays parallel with
        // `outputs.frames`: 0 is the palette as stored, 1 the remapped copy.
        // `frame_count` above is the entry count the grids draw.
        frames_rendered: (0..outputs.frames.len()).collect(),
        scale: render.scale,
        mode: MODE_SWATCH.to_string(),
        warnings,
        outputs,
    }
}

/// The mark of every entry: only index 0 is called out, whatever its colour.
/// It sits outside the remap band, so a remapped grid keeps the same marks.
fn swatch_marks() -> [SwatchMark; PAL_COLORS] {
    let mut marks = [SwatchMark::None; PAL_COLORS];
    marks[0] = SwatchMark::TransparentIndex0;
    marks
}

fn pal_rows(palette: &Palette, bytes: &[u8], marks: &[SwatchMark; PAL_COLORS]) -> Vec<PalRow> {
    (0..PAL_COLORS)
        .map(|index| {
            let color = palette.colors()[index];
            let base = index * 3;
            let raw = match bytes.get(base..base + 3) {
                Some(slice) => [slice[0], slice[1], slice[2]],
                None => [0, 0, 0],
            };
            PalRow {
                index,
                row: index / GRID_COLS,
                col: index % GRID_COLS,
                hex: format!("#{:02X}{:02X}{:02X}", color.r, color.g, color.b),
                raw,
                remap_band: (HOUSE_REMAP_BAND[0]..HOUSE_REMAP_BAND[1]).contains(&index),
                mark: marks[index],
            }
        })
        .collect()
}

fn pal_index_tsv_row(row: &PalRow) -> String {
    format!(
        "{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}",
        row.index,
        row.row,
        row.col,
        row.hex,
        row.raw[0],
        row.raw[1],
        row.raw[2],
        row.remap_band,
        row.mark.tag()
    )
}

fn pal_index_tsv(rows: &[PalRow]) -> String {
    let mut text = String::with_capacity(INDEX_TSV_HEADER.len() + rows.len() * 48);
    text.push_str(INDEX_TSV_HEADER);
    text.push('\n');
    for row in rows {
        text.push_str(&pal_index_tsv_row(row));
        text.push('\n');
    }
    text
}

/// Build the house-colour ramp from the ruleset's `[Colors]` list.
///
/// The ramps come from the INI every time — a hardcoded retail colour table
/// would be a second source of truth that a mod or a rules edit silently
/// invalidates. Mirrors the SHP render path, including its out-of-range warning.
fn resolve_house_ramp(
    asset_manager: &AssetManager,
    index: u8,
    warnings: &mut Vec<String>,
) -> Result<[Color; HOUSE_RAMP_LEN], ErrorReport> {
    let Some((ini_name, ini_bytes)) = RULES_INI_CANDIDATES.iter().find_map(|candidate| {
        asset_manager
            .get(candidate)
            .map(|bytes| (*candidate, bytes))
    }) else {
        return Err(ErrorReport {
            error: "--house needs the ruleset, but neither rulesmd.ini nor rules.ini resolved"
                .to_string(),
            hint: Some("drop --house to render the palette as stored".to_string()),
        });
    };

    let ini = IniFile::from_bytes(&ini_bytes).map_err(|err| ErrorReport {
        error: format!("could not parse {ini_name} for --house: {err}"),
        hint: Some("drop --house to render the palette as stored".to_string()),
    })?;
    let schemes = parse_color_schemes(&ini);
    if schemes.is_empty() {
        return Err(ErrorReport {
            error: format!(
                "{ini_name} has no usable `[Colors]` entries, so --house cannot resolve"
            ),
            hint: Some("drop --house to render the palette as stored".to_string()),
        });
    }
    if usize::from(index) >= schemes.len() {
        warnings.push(format!(
            "--house {index} is outside the {} `[Colors]` entries in {ini_name}; the default scheme ramp was used",
            schemes.len()
        ));
    }

    let ramps = HouseColorRamps::from_schemes(&schemes);
    Ok(*ramps.ramp(HouseColorIndex(index)))
}

// ---------------------------------------------------------------------------
// Swatch grid drawing
// ---------------------------------------------------------------------------

/// Draw one 16x16 swatch grid under `header`.
///
/// Every marker is drawn *outside* its swatch, in the gap, so no swatch colour
/// is ever covered by an annotation — the colours are the data here.
fn draw_swatch_grid(
    palette: &Palette,
    marks: &[SwatchMark; PAL_COLORS],
    header: &[String],
) -> Rgba {
    let (width, height) = grid_image_size(header.len());
    let mut image = Rgba::new_filled(width, height, GRID_BACKGROUND);
    if image.w == 0 || image.h == 0 {
        return image;
    }

    for (line, text) in header.iter().enumerate() {
        let y = i64::from(GRID_MARGIN) + line as i64 * i64::from(canvas::LABEL_HEIGHT);
        canvas::draw_text(&mut image, i64::from(GRID_MARGIN), y, text, HEADER_COLOR);
    }

    for (index, (&color, mark)) in palette.colors().iter().zip(marks.iter()).enumerate() {
        let (x, y) = swatch_origin(index, header.len());
        let swatch = Rgba::new_filled(SWATCH_SIZE, SWATCH_SIZE, [color.r, color.g, color.b, 255]);
        canvas::blit_over(&mut image, &swatch, x, y);
        canvas::draw_text(
            &mut image,
            x + SWATCH_TEXT_INSET,
            y + SWATCH_TEXT_INSET,
            &index.to_string(),
            label_color_for(color),
        );
        if let Some(marker) = mark.marker_color() {
            canvas::draw_rect_outline(
                &mut image,
                x - 1,
                y - 1,
                SWATCH_SIZE + 2,
                SWATCH_SIZE + 2,
                marker,
            );
        }
    }

    // The remap band is exactly the second row, so one rectangle covers it.
    let (band_x, band_y) = swatch_origin(HOUSE_REMAP_BAND[0], header.len());
    for inset in BAND_OUTLINE_INSETS {
        canvas::draw_rect_outline(
            &mut image,
            band_x - i64::from(inset),
            band_y - i64::from(inset),
            grid_pixel_width() + inset * 2,
            SWATCH_SIZE + inset * 2,
            REMAP_BAND_COLOR,
        );
    }

    image
}

/// Distance between the left edges of two neighbouring swatches.
fn swatch_pitch() -> u32 {
    SWATCH_SIZE + SWATCH_GAP
}

/// Width of the swatch block alone, excluding margins.
fn grid_pixel_width() -> u32 {
    GRID_COLS as u32 * swatch_pitch() - SWATCH_GAP
}

/// Height of the swatch block alone, excluding margins and the header.
fn grid_pixel_height() -> u32 {
    GRID_ROWS as u32 * swatch_pitch() - SWATCH_GAP
}

/// Vertical space the header block occupies, gap included.
fn header_block_height(lines: usize) -> u32 {
    if lines == 0 {
        0
    } else {
        lines as u32 * canvas::LABEL_HEIGHT + HEADER_GAP
    }
}

/// Top edge of the first swatch row.
fn grid_origin_y(header_lines: usize) -> u32 {
    GRID_MARGIN + header_block_height(header_lines)
}

/// Top-left corner of one swatch, in unscaled grid pixels.
fn swatch_origin(index: usize, header_lines: usize) -> (i64, i64) {
    let row = (index / GRID_COLS) as u32;
    let col = (index % GRID_COLS) as u32;
    (
        i64::from(GRID_MARGIN + col * swatch_pitch()),
        i64::from(grid_origin_y(header_lines) + row * swatch_pitch()),
    )
}

/// Full grid image size. Independent of the header *text* — lines are truncated
/// to the grid width instead of widening it — so the raw grid and the
/// house-remapped grid always come out identical in size, which is what makes
/// `RenderReport::canvas` true for both.
fn grid_image_size(header_lines: usize) -> (u32, u32) {
    (
        GRID_MARGIN * 2 + grid_pixel_width(),
        GRID_MARGIN * 2 + header_block_height(header_lines) + grid_pixel_height(),
    )
}

/// Dark label on a bright swatch, light on a dark one, by BT.601 luma.
fn label_color_for(color: Color) -> [u8; 4] {
    let luma =
        (LUMA_R * u32::from(color.r) + LUMA_G * u32::from(color.g) + LUMA_B * u32::from(color.b))
            / LUMA_DENOMINATOR;
    if luma >= LUMA_MIDPOINT {
        SWATCH_LABEL_DARK
    } else {
        SWATCH_LABEL_LIGHT
    }
}

/// Header burned into a swatch grid. Always the same number of lines for the
/// raw and remapped grids, so both images stay the same height.
///
/// The legend is split across two lines on purpose: one line naming both
/// marker colours would clip against the grid width, and a clipped legend is
/// worse than no legend.
fn pal_header(name: &str, source_archive: &str, house: Option<u8>) -> Vec<String> {
    let band_line = match house {
        Some(index) => format!("house colour scheme {index} applied to indices 16 to 31"),
        None => "palette entries exactly as stored   nothing remapped".to_string(),
    };
    vec![
        header_line(&format!("{name} in {source_archive}")),
        header_line("palette swatch grid 16 by 16   index 0 top left   row major"),
        header_line(&band_line),
        header_line("cyan outline is the house colour remap band 16 to 31"),
        header_line("yellow outline index 0 transparent"),
    ]
}

/// Header for the raw-versus-remapped contact sheet. The sheet sizes itself to
/// its header, so these lines are not truncated to the grid width.
fn comparison_sheet_header(name: &str, source_archive: &str, house: u8) -> Vec<String> {
    vec![
        label_text(&format!("{name} in {source_archive}")),
        label_text(&format!(
            "palette as stored first   house colour scheme {house} applied second"
        )),
        label_text("cyan outline house remap band 16 to 31   yellow index 0"),
    ]
}

/// Map every character into the drawable glyph set, one for one.
///
/// The shared 5x7 table covers space, `-`, `:`, `/`, digits and letters; every
/// other character advances the pen and draws nothing, so a retail name
/// containing `.`, `>` or `_` would come out with holes in it. Substituting
/// keeps the label the same length as the source, so it still lines up with the
/// exact name the report carries.
fn label_text(text: &str) -> String {
    text.chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || matches!(character, ' ' | '-' | ':' | '/') {
                character
            } else {
                LABEL_SUBSTITUTE
            }
        })
        .collect()
}

/// A drawable header line, clipped to the grid width.
fn header_line(text: &str) -> String {
    label_text(text)
        .chars()
        .take(header_char_budget(grid_pixel_width()))
        .collect()
}

/// Longest label, in characters, that fits `width` pixels.
///
/// Derived from the public text metrics rather than the glyph constants, so a
/// font change cannot leave this stale.
fn header_char_budget(width: u32) -> usize {
    let single = canvas::text_width("A");
    let advance = canvas::text_width("AA").saturating_sub(single).max(1);
    let spacing = advance.saturating_sub(single);
    ((width + spacing) / advance) as usize
}

// ---------------------------------------------------------------------------
// Shared plumbing
//
// These mirror the SHP render path's private helpers; they are duplicated
// rather than shared because module wiring is not this file's to change.
// ---------------------------------------------------------------------------

fn locate_asset<'a>(
    asset_manager: &'a AssetManager,
    name: &str,
) -> Result<crate::asset_tools::locate::Resolved<'a>, ErrorReport> {
    crate::asset_tools::locate::locate(asset_manager, name).ok_or_else(|| ErrorReport {
        error: format!("asset not found: {name}"),
        hint: Some(format!(
            "run `asset find {name}` to see whether any archive holds it"
        )),
    })
}

/// Create the output directory and clear this verb's previous PNGs from it.
///
/// A re-run must replace the previous render, not blend with it: a run without
/// `--house` after one with it would otherwise leave a remapped grid sitting
/// there reading as current output.
fn prepare_output_dir(out: &Path, sanitised: &str) -> Result<PathBuf, ErrorReport> {
    let dir = render_dir(out, sanitised);
    std::fs::create_dir_all(&dir).map_err(|err| ErrorReport {
        error: format!("could not create {}: {err}", dir.display()),
        hint: Some("pass a writable `--out` root".to_string()),
    })?;
    clear_stale_outputs(&dir, sanitised);
    Ok(dir)
}

fn save(path: &Path, image: &Rgba) -> Result<(), ErrorReport> {
    canvas::save_png(path, image).map_err(|err| ErrorReport {
        error: format!("could not write {}: {err}", path.display()),
        hint: Some("pass a writable `--out` root".to_string()),
    })
}

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

fn still_png_name(sanitised: &str) -> String {
    format!("{sanitised}.png")
}

fn grid_png_name(sanitised: &str) -> String {
    format!("{sanitised}.pal.png")
}

fn house_grid_png_name(sanitised: &str, house: u8) -> String {
    format!("{sanitised}.house-{house:03}.png")
}

fn sheet_png_name(sanitised: &str) -> String {
    format!("{sanitised}.sheet.png")
}

/// True for a file this module itself wrote for `sanitised`, so a regeneration
/// clears only its own previous output.
fn is_generated_output(file_name: &str, sanitised: &str) -> bool {
    let Some(rest) = file_name.strip_prefix(sanitised) else {
        return false;
    };
    if matches!(rest, ".png" | ".pal.png" | ".sheet.png") {
        return true;
    }
    rest.strip_prefix(".house-")
        .and_then(|rest| rest.strip_suffix(".png"))
        .is_some_and(|digits| !digits.is_empty() && digits.chars().all(|c| c.is_ascii_digit()))
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

/// Wording for a flag that belongs to another format's render path. Saying so
/// keeps a caller from concluding the flag worked and the asset is at fault.
fn ignored(flag: &str, reason: &str) -> String {
    format!("{flag} was ignored: {reason}")
}

fn clamp_scale(scale: u32) -> u32 {
    scale.clamp(MIN_SCALE, MAX_SCALE)
}

/// Largest scale <= `requested` that keeps one image inside the pixel budget.
/// `None` when even 1x is too large, i.e. the header itself is unusable.
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

#[cfg(test)]
mod tests {
    use super::*;

    /// Every character the shared 5x7 table can actually draw.
    const DRAWABLE: &str = " -:/0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz";

    /// A 768-byte palette whose entry `i` stores `(i, i/2, i/4)` wrapped into
    /// the 6-bit range, so neighbouring swatches differ.
    fn make_pal() -> Vec<u8> {
        let mut data = vec![0u8; PAL_FILE_BYTES];
        for index in 0..PAL_COLORS {
            let base = index * 3;
            data[base] = (index % 64) as u8;
            data[base + 1] = ((index / 2) % 64) as u8;
            data[base + 2] = ((index / 4) % 64) as u8;
        }
        data
    }

    /// A minimal 8-bit RLE PCX with a trailing VGA table: `w` x `h`, every
    /// pixel `fill`, with `palette[fill]` set to `color`.
    fn make_pcx(w: u16, h: u16, fill: u8, color: [u8; 3]) -> Vec<u8> {
        let mut data = vec![0u8; 128];
        data[0] = 0x0A;
        data[2] = 1;
        data[3] = 8;
        // The image window is inclusive, so the maxima are one less than the size.
        data[8..10].copy_from_slice(&(w - 1).to_le_bytes());
        data[10..12].copy_from_slice(&(h - 1).to_le_bytes());
        data[65] = 1;
        data[66..68].copy_from_slice(&w.to_le_bytes());

        // One run per scanline; runs cap at 63 in the marker byte.
        for _ in 0..h {
            let mut remaining = w as usize;
            while remaining > 0 {
                let run = remaining.min(0x3F);
                data.push(0xC0 | run as u8);
                data.push(fill);
                remaining -= run;
            }
        }

        data.push(0x0C);
        let mut pal = vec![0u8; 768];
        pal[fill as usize * 3..fill as usize * 3 + 3].copy_from_slice(&color);
        data.extend_from_slice(&pal);
        data
    }

    fn flat_ramp(color: Color) -> [Color; HOUSE_RAMP_LEN] {
        [color; HOUSE_RAMP_LEN]
    }

    fn no_outputs() -> RenderOutputs {
        RenderOutputs {
            dir: "unused".to_string(),
            sheet: None,
            frames: vec!["unused.png".to_string()],
            index: None,
        }
    }

    // --- grid geometry -----------------------------------------------------

    #[test]
    fn swatch_grid_geometry_matches_the_declared_layout() {
        assert_eq!(swatch_pitch(), SWATCH_SIZE + SWATCH_GAP);
        // 16 columns of pitch, minus the trailing gap.
        assert_eq!(
            grid_pixel_width(),
            16 * (SWATCH_SIZE + SWATCH_GAP) - SWATCH_GAP
        );
        assert_eq!(grid_pixel_height(), grid_pixel_width());

        let (w, h) = grid_image_size(4);
        assert_eq!(w, GRID_MARGIN * 2 + grid_pixel_width());
        assert_eq!(
            h,
            GRID_MARGIN * 2 + 4 * canvas::LABEL_HEIGHT + HEADER_GAP + grid_pixel_height()
        );

        // No header means no header block at all, not an empty one.
        assert_eq!(header_block_height(0), 0);
        assert_eq!(grid_origin_y(0), GRID_MARGIN);
    }

    #[test]
    fn grid_image_size_ignores_header_text_so_both_grids_match() {
        // Both grids carry the same line count but different wording; the
        // report's single `canvas` field has to describe both.
        let raw = pal_header("sidebar.pal", "ra2.mix", None);
        let remapped = pal_header("sidebar.pal", "ra2.mix", Some(7));
        assert_eq!(raw.len(), remapped.len());
        assert_eq!(grid_image_size(raw.len()), grid_image_size(remapped.len()));
    }

    #[test]
    fn every_swatch_sits_inside_the_grid_image() {
        let lines = 4;
        let (w, h) = grid_image_size(lines);
        for index in 0..PAL_COLORS {
            let (x, y) = swatch_origin(index, lines);
            // One pixel of slack each side for the marker outline.
            assert!(x > 0, "index {index} marker runs off the left edge");
            assert!(y > 0, "index {index} marker runs off the top edge");
            assert!(
                x + i64::from(SWATCH_SIZE) < i64::from(w),
                "index {index} runs off the right edge"
            );
            assert!(
                y + i64::from(SWATCH_SIZE) < i64::from(h),
                "index {index} runs off the bottom edge"
            );
        }
    }

    #[test]
    fn remap_band_occupies_exactly_the_second_row() {
        let lines = 4;
        let pitch = i64::from(swatch_pitch());
        let (first_x, first_y) = swatch_origin(0, lines);
        let (band_start_x, band_start_y) = swatch_origin(HOUSE_REMAP_BAND[0], lines);
        let (band_end_x, band_end_y) = swatch_origin(HOUSE_REMAP_BAND[1] - 1, lines);

        // Index 16 starts the second row at column 0.
        assert_eq!(band_start_x, first_x);
        assert_eq!(band_start_y, first_y + pitch);
        // Index 31 closes it at column 15, on the same row.
        assert_eq!(band_end_y, band_start_y);
        assert_eq!(band_end_x, first_x + 15 * pitch);
        // So one rectangle as wide as the swatch block covers the whole band.
        assert_eq!(
            band_end_x + i64::from(SWATCH_SIZE) - band_start_x,
            i64::from(grid_pixel_width())
        );
        // And the outline insets stay inside the margin.
        assert!(BAND_OUTLINE_INSETS.iter().all(|inset| *inset < GRID_MARGIN));
    }

    #[test]
    fn swatch_origin_walks_row_major_from_index_zero() {
        let pitch = i64::from(swatch_pitch());
        let (x0, y0) = swatch_origin(0, 0);
        assert_eq!((x0, y0), (i64::from(GRID_MARGIN), i64::from(GRID_MARGIN)));
        assert_eq!(swatch_origin(1, 0), (x0 + pitch, y0));
        assert_eq!(swatch_origin(15, 0), (x0 + 15 * pitch, y0));
        assert_eq!(swatch_origin(16, 0), (x0, y0 + pitch));
        assert_eq!(swatch_origin(255, 0), (x0 + 15 * pitch, y0 + 15 * pitch));
    }

    #[test]
    fn a_three_digit_index_label_fits_inside_its_swatch() {
        let widest = canvas::text_width("255");
        assert!(
            widest + SWATCH_TEXT_INSET as u32 * 2 <= SWATCH_SIZE,
            "label {widest}px does not fit a {SWATCH_SIZE}px swatch"
        );
    }

    // --- labels ------------------------------------------------------------

    #[test]
    fn label_text_maps_every_character_into_the_drawable_set() {
        let messy = "SIDEBAR.PAL in ra2.mix -> sidec02.mix (theme_2)";
        let drawn = label_text(messy);
        assert_eq!(
            drawn.chars().count(),
            messy.chars().count(),
            "substitution must stay one for one"
        );
        for character in drawn.chars() {
            assert!(
                DRAWABLE.contains(character),
                "{character:?} is not in the glyph table"
            );
        }
        // Drawable characters survive untouched.
        assert_eq!(label_text("house 12 remap 16-31"), "house 12 remap 16-31");
        // The characters that silently vanished in phase 1 are the ones mapped.
        assert_eq!(label_text("a.b_c,d(e)f=g#h@i"), "a-b-c-d-e-f-g-h-i");
    }

    #[test]
    fn every_burned_in_header_line_is_drawable_and_fits_the_grid() {
        let mut lines = pal_header("SIDEBAR.PAL", "ra2.mix -> sidec02.mix", None);
        lines.extend(pal_header("SIDEBAR.PAL", "ra2.mix -> sidec02.mix", Some(3)));
        lines.extend(comparison_sheet_header(
            "SIDEBAR.PAL",
            "ra2.mix -> sidec02.mix",
            3,
        ));
        assert!(!lines.is_empty());
        for line in &lines {
            for character in line.chars() {
                assert!(
                    DRAWABLE.contains(character),
                    "{character:?} in {line:?} is not in the glyph table"
                );
            }
        }
        // Only the grid headers are width-clipped; the sheet sizes to its own.
        for line in pal_header("SIDEBAR.PAL", "ra2.mix -> sidec02.mix", Some(3)) {
            assert!(
                canvas::text_width(&line) <= grid_pixel_width(),
                "{line:?} overflows the grid width"
            );
        }
    }

    #[test]
    fn header_line_clips_an_overlong_name_instead_of_widening_the_grid() {
        let long = "X".repeat(400);
        let line = header_line(&long);
        assert_eq!(line.chars().count(), header_char_budget(grid_pixel_width()));
        assert!(canvas::text_width(&line) <= grid_pixel_width());
    }

    #[test]
    fn header_char_budget_never_overflows_its_width() {
        for width in 0..200u32 {
            let budget = header_char_budget(width);
            let text = "A".repeat(budget);
            assert!(
                canvas::text_width(&text) <= width,
                "budget {budget} overflows width {width}"
            );
            let one_more = "A".repeat(budget + 1);
            assert!(
                canvas::text_width(&one_more) > width,
                "budget {budget} is short of width {width}"
            );
        }
    }

    #[test]
    fn swatch_label_flips_with_swatch_brightness() {
        assert_eq!(
            label_color_for(Color::rgb(255, 255, 255)),
            SWATCH_LABEL_DARK
        );
        assert_eq!(label_color_for(Color::rgb(0, 0, 0)), SWATCH_LABEL_LIGHT);
        // Green dominates BT.601 luma, blue barely registers.
        assert_eq!(label_color_for(Color::rgb(0, 255, 0)), SWATCH_LABEL_DARK);
        assert_eq!(label_color_for(Color::rgb(0, 0, 255)), SWATCH_LABEL_LIGHT);
    }

    // --- palette marks and sidecar -----------------------------------------

    #[test]
    fn marks_flag_index_zero_only() {
        let marks = swatch_marks();
        assert_eq!(marks[0], SwatchMark::TransparentIndex0);
        assert!(marks[0].marker_color().is_some());
        assert!(
            marks[1..]
                .iter()
                .all(|mark| *mark == SwatchMark::None && mark.marker_color().is_none())
        );
    }

    #[test]
    fn pal_index_tsv_carries_the_raw_bytes_the_decode_and_the_band() {
        let mut data = make_pal();
        data[3..6].copy_from_slice(&[63, 0, 63]);
        let palette = Palette::from_bytes(&data).expect("palette");
        let rows = pal_rows(&palette, &data, &swatch_marks());

        assert_eq!(rows.len(), PAL_COLORS);
        assert_eq!(rows[0].mark, SwatchMark::TransparentIndex0);
        // A raw magenta entry is an ordinary colour, not a key.
        assert_eq!(rows[1].mark, SwatchMark::None);
        assert_eq!(rows[1].raw, [63, 0, 63]);
        // The decode is `raw << 2`, so 63 becomes 252.
        assert_eq!(rows[1].hex, "#FC00FC");
        assert_eq!((rows[16].row, rows[16].col), (1, 0));
        assert_eq!((rows[31].row, rows[31].col), (1, 15));
        assert!(rows[16].remap_band && rows[31].remap_band);
        assert!(!rows[15].remap_band && !rows[32].remap_band);

        let text = pal_index_tsv(&rows);
        let mut lines = text.lines();
        assert_eq!(lines.next(), Some(INDEX_TSV_HEADER));
        assert_eq!(lines.next(), Some(pal_index_tsv_row(&rows[0]).as_str()));
        assert_eq!(text.lines().count(), PAL_COLORS + 1);
        assert!(text.ends_with('\n'));
        // Index 1 sits outside the band, so the band column reads false.
        assert!(pal_index_tsv_row(&rows[1]).ends_with("\tfalse\tnone"));
        assert!(pal_index_tsv_row(&rows[16]).ends_with("\ttrue\tnone"));
    }

    // --- pal render --------------------------------------------------------

    #[test]
    fn pal_render_draws_one_grid_and_a_second_only_for_a_house_ramp() {
        let data = make_pal();
        let opts = RenderOptions::default();

        let plain = build_pal_render(&data, "sidebar.pal", "ra2.mix", &opts, None).expect("render");
        assert!(plain.house.is_none());
        assert_eq!(plain.scale, PAL_DEFAULT_SCALE);
        assert_eq!(plain.canvas, {
            let (w, h) = grid_image_size(pal_header("sidebar.pal", "ra2.mix", None).len());
            [w, h]
        });
        assert_eq!(
            (plain.base.w, plain.base.h),
            (plain.canvas[0], plain.canvas[1])
        );
        assert_eq!(plain.rows.len(), PAL_COLORS);

        let remapped = build_pal_render(
            &data,
            "sidebar.pal",
            "ra2.mix",
            &opts,
            Some((3, flat_ramp(Color::rgb(255, 0, 0)))),
        )
        .expect("render");
        let house = remapped.house.expect("house grid");
        // Both grids must be the same size for the comparison sheet to line up.
        assert_eq!((house.w, house.h), (remapped.base.w, remapped.base.h));
        assert_ne!(
            house.data, remapped.base.data,
            "the ramp must change pixels"
        );
    }

    #[test]
    fn pal_render_scales_the_grid_by_the_requested_factor() {
        let data = make_pal();
        let opts = RenderOptions {
            scale: Some(2),
            ..RenderOptions::default()
        };
        let render = build_pal_render(&data, "x.pal", "ra2.mix", &opts, None).expect("render");
        assert_eq!(render.scale, 2);
        assert_eq!(render.base.w, render.canvas[0] * 2);
        assert_eq!(render.base.h, render.canvas[1] * 2);
    }

    #[test]
    fn pal_render_warns_about_flags_that_belong_to_other_formats() {
        let data = make_pal();
        let opts = RenderOptions {
            palette: Some("unittem.pal".to_string()),
            crop: true,
            frame: Some(4),
            transparent_index: Some(0),
            ..RenderOptions::default()
        };
        let render = build_pal_render(&data, "x.pal", "ra2.mix", &opts, None).expect("render");
        let joined = render.warnings.join("\n");
        for flag in ["--palette", "--crop", "--frame", "--transparent-index"] {
            assert!(joined.contains(flag), "missing {flag} in {joined}");
        }
    }

    #[test]
    fn pal_render_warns_when_components_leave_the_vga_6bit_range() {
        let mut data = make_pal();
        data[9] = 200;
        let render = build_pal_render(&data, "x.pal", "ra2.mix", &RenderOptions::default(), None)
            .expect("render");
        assert!(
            render.warnings.iter().any(|w| w.contains("above 63")),
            "{:?}",
            render.warnings
        );
    }

    #[test]
    fn pal_render_reports_no_key_for_raw_magenta_entries() {
        let mut data = make_pal();
        data[3..6].copy_from_slice(&[63, 0, 63]);
        data[6..9].copy_from_slice(&[63, 0, 63]);
        let render = build_pal_render(&data, "x.pal", "ra2.mix", &RenderOptions::default(), None)
            .expect("render");
        assert!(render.warnings.is_empty(), "{:?}", render.warnings);
    }

    #[test]
    fn pal_render_rejects_a_palette_that_is_not_768_bytes() {
        let err = build_pal_render(
            &[0u8; 700],
            "short.pal",
            "ra2.mix",
            &RenderOptions::default(),
            None,
        )
        .unwrap_err();
        assert!(err.error.contains("PAL parse failed"), "{}", err.error);
        assert!(err.hint.is_some());
    }

    #[test]
    fn pal_report_describes_the_entries_and_stays_parallel_with_its_outputs() {
        let data = make_pal();
        let render = build_pal_render(
            &data,
            "sidebar.pal",
            "ra2.mix -> sidec02.mix",
            &RenderOptions::default(),
            Some((3, flat_ramp(Color::rgb(0, 0, 255)))),
        )
        .expect("render");

        let outputs = RenderOutputs {
            dir: "d".to_string(),
            sheet: Some("d/sidebar.pal.sheet.png".to_string()),
            frames: vec![
                "d/sidebar.pal.pal.png".to_string(),
                "d/sidebar.pal.house-003.png".to_string(),
            ],
            index: Some("d/index.tsv".to_string()),
        };
        let report = pal_report(
            "sidebar.pal",
            "ra2.mix -> sidec02.mix",
            &render,
            Some(3),
            outputs,
            Vec::new(),
        );

        assert_eq!(report.kind, KIND_PAL);
        assert_eq!(report.mode, MODE_SWATCH);
        assert_eq!(report.frame_count, PAL_COLORS);
        assert_eq!(report.house_color, Some(3));
        assert_eq!(report.canvas, render.canvas);
        assert_eq!(report.scale, render.scale);
        assert_eq!(
            report.frames_rendered.len(),
            report.outputs.frames.len(),
            "frames_rendered indexes outputs.frames"
        );
        assert_eq!(report.frames_rendered, vec![0, 1]);
        let choice = report.palette.expect("the asset is its own palette");
        assert_eq!(choice.name, "sidebar.pal");
        assert_eq!(choice.confidence, "declared");
    }

    // --- pcx render --------------------------------------------------------

    #[test]
    fn pcx_report_is_built_from_a_synthetic_tiny_pcx() {
        let data = make_pcx(2, 2, 1, [10, 20, 30]);
        let still = build_pcx_still(&data, "shell.pcx", &RenderOptions::default()).expect("still");

        assert_eq!(still.canvas, [2, 2]);
        // A 2px image magnifies hard, so the clamp is what caps it.
        assert_eq!(still.scale, MAX_SCALE);
        assert_eq!(
            (still.image.w, still.image.h),
            (2 * MAX_SCALE, 2 * MAX_SCALE)
        );
        assert!(still.warnings.is_empty(), "{:?}", still.warnings);

        let report = pcx_report("shell.pcx", "ra2.mix", &still, no_outputs(), Vec::new());
        assert_eq!(report.kind, KIND_PCX);
        assert_eq!(report.mode, MODE_IMAGE);
        assert_eq!(report.asset, "shell.pcx");
        assert_eq!(report.source_archive, "ra2.mix");
        assert_eq!(report.canvas, [2, 2]);
        assert_eq!(report.frame_count, 1);
        assert_eq!(report.frames_rendered, vec![0]);
        assert_eq!(report.scale, MAX_SCALE);
        assert!(
            report.palette.is_none(),
            "a PCX carries its own table; inference must not be credited"
        );
        assert!(report.house_color.is_none());
    }

    #[test]
    fn pcx_pixels_come_out_of_the_embedded_palette() {
        let data = make_pcx(2, 2, 1, [10, 20, 30]);
        let opts = RenderOptions {
            scale: Some(1),
            ..RenderOptions::default()
        };
        let still = build_pcx_still(&data, "shell.pcx", &opts).expect("still");
        assert_eq!(still.image.data[0..4], [10, 20, 30, 255]);
    }

    #[test]
    fn pcx_transparency_puts_a_checkerboard_behind_the_image() {
        let data = make_pcx(2, 2, 1, [10, 20, 30]);
        let opts = RenderOptions {
            scale: Some(1),
            transparent_index: Some(1),
            ..RenderOptions::default()
        };
        let still = build_pcx_still(&data, "shell.pcx", &opts).expect("still");
        // Every pixel was keyed out, so only the backdrop remains — and it is
        // opaque, which is how a transparent region stays visible in a PNG.
        assert_eq!(still.image.data[3], 255);
        assert_ne!(still.image.data[0..3], [10, 20, 30]);
    }

    #[test]
    fn pcx_warns_when_the_requested_transparent_index_is_unused() {
        let data = make_pcx(4, 2, 1, [10, 20, 30]);
        let opts = RenderOptions {
            transparent_index: Some(7),
            ..RenderOptions::default()
        };
        let still = build_pcx_still(&data, "shell.pcx", &opts).expect("still");
        assert!(
            still.warnings.iter().any(|w| w.contains("index 7")),
            "{:?}",
            still.warnings
        );
    }

    #[test]
    fn pcx_warns_about_flags_that_belong_to_other_formats() {
        let data = make_pcx(4, 4, 2, [1, 2, 3]);
        let opts = RenderOptions {
            palette: Some("cameo.pal".to_string()),
            house: Some(2),
            crop: true,
            frame: Some(3),
            ..RenderOptions::default()
        };
        let still = build_pcx_still(&data, "shell.pcx", &opts).expect("still");
        let joined = still.warnings.join("\n");
        for flag in ["--palette", "--house", "--crop", "--frame"] {
            assert!(joined.contains(flag), "missing {flag} in {joined}");
        }
        // --frame 0 is the only frame there is, so it is not worth a warning.
        let opts = RenderOptions {
            frame: Some(0),
            ..RenderOptions::default()
        };
        let still = build_pcx_still(&data, "shell.pcx", &opts).expect("still");
        assert!(still.warnings.is_empty(), "{:?}", still.warnings);
    }

    #[test]
    fn pcx_parse_failure_is_a_value_with_a_hint() {
        // A SHP header, pointed at the PCX path.
        let mut data = vec![0u8; 8 + 24];
        data[2..4].copy_from_slice(&16u16.to_le_bytes());
        data[4..6].copy_from_slice(&2u16.to_le_bytes());
        data[6..8].copy_from_slice(&1u16.to_le_bytes());

        let err = build_pcx_still(&data, "gi.shp", &RenderOptions::default()).unwrap_err();
        assert!(err.error.contains("PCX parse failed"), "{}", err.error);
        assert!(err.error.contains("sniffed as"), "{}", err.error);
        assert!(
            err.hint.is_some_and(|hint| hint.contains("asset info")),
            "the hint must name the verb that helps"
        );
    }

    // --- output paths ------------------------------------------------------

    #[test]
    fn render_dir_is_absolute_and_ends_with_the_render_subdir() {
        let dir = render_dir(Path::new("target/asset"), "sidebar.pal");
        assert!(dir.is_absolute(), "{}", dir.display());
        assert!(
            dir.ends_with(Path::new(RENDER_SUBDIR).join("sidebar.pal")),
            "{}",
            dir.display()
        );
    }

    #[test]
    fn sanitise_name_keeps_safe_characters_and_replaces_the_rest() {
        assert_eq!(sanitise_name("sidebar.pal"), "sidebar.pal");
        assert_eq!(sanitise_name("ra2\\dir/na me:x*?"), "ra2_dir_na_me_x__");
        assert_eq!(sanitise_name(""), FALLBACK_DIR_NAME);
    }

    #[test]
    fn output_filenames_are_stable_and_zero_padded() {
        assert_eq!(still_png_name("shell.pcx"), "shell.pcx.png");
        assert_eq!(grid_png_name("sidebar.pal"), "sidebar.pal.pal.png");
        assert_eq!(
            house_grid_png_name("sidebar.pal", 3),
            "sidebar.pal.house-003.png"
        );
        assert_eq!(
            house_grid_png_name("sidebar.pal", 255),
            "sidebar.pal.house-255.png"
        );
        assert_eq!(sheet_png_name("sidebar.pal"), "sidebar.pal.sheet.png");
    }

    #[test]
    fn generated_output_predicate_matches_only_our_own_files() {
        assert!(is_generated_output("shell.pcx.png", "shell.pcx"));
        assert!(is_generated_output("sidebar.pal.pal.png", "sidebar.pal"));
        assert!(is_generated_output(
            "sidebar.pal.house-003.png",
            "sidebar.pal"
        ));
        assert!(is_generated_output("sidebar.pal.sheet.png", "sidebar.pal"));
        // Not ours: other assets, other extensions, other shapes.
        assert!(!is_generated_output("sidebar.pal.pal.png", "cameo.pal"));
        assert!(!is_generated_output("index.tsv", "sidebar.pal"));
        assert!(!is_generated_output(
            "sidebar.pal.house-.png",
            "sidebar.pal"
        ));
        assert!(!is_generated_output(
            "sidebar.pal.house-abc.png",
            "sidebar.pal"
        ));
        assert!(!is_generated_output("sidebar.pal.notes.png", "sidebar.pal"));
        assert!(!is_generated_output("sidebar.pal.000.png", "sidebar.pal"));
    }

    #[test]
    fn scale_is_clamped_and_fitted_to_the_pixel_budget() {
        assert_eq!(clamp_scale(0), MIN_SCALE);
        assert_eq!(clamp_scale(999), MAX_SCALE);
        assert_eq!(fit_scale(60, 30, 8), Some(8));
        let fitted = fit_scale(4000, 4000, 16).unwrap();
        assert!((MIN_SCALE..16).contains(&fitted), "{fitted}");
        assert!(
            u64::from(4000_u32) * u64::from(4000_u32) * u64::from(fitted) * u64::from(fitted)
                <= MAX_OUTPUT_PIXELS
        );
        // A corrupt header claiming a giant image fails instead of allocating.
        assert_eq!(fit_scale(60_000, 60_000, 1), None);
    }
}
