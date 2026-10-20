//! `asset render` for TMP terrain templates — the isometric tile files.
//!
//! The template is written to PNGs, plus the per-tile fields that decide how the
//! cell behaves in the sim.
//!
//! Two layouts, because they answer different questions:
//!
//! - **grid** (default): one cell per template slot at the tile pitch. Every
//!   tile is separated from its neighbours, so a missing cell, a wrong tile size
//!   or an odd `offset_x`/`offset_y` is obvious. Labels land in the empty corner
//!   of each diamond and cover nothing.
//! - **isometric** (`--isometric`): the half-cell stagger the game composes, so
//!   the template reads as the shape a player sees.
//!
//! Per-tile `height` and `ramp_type` are burned into the image, not just the
//! JSON: those two fields drive cliff/slope behaviour, and pairing them with the
//! picture is the whole reason to render a template rather than read `asset
//! info`.
//!
//! ## Dependency rules
//! - Part of `asset_tools/`: depends on `assets/` (TMP, palette, archive
//!   resolution), `rules/` (the art registry, for palette inference) and the
//!   sibling `canvas` / `identify` / `locate` / `names` / `palette` / `report`
//!   modules.
//! - Nothing from `sim/`, `render/`, `ui/`, `audio/`, or `net/`.

use std::path::{Path, PathBuf};

use crate::asset_tools::canvas::{self, Rgba};
use crate::asset_tools::identify;
use crate::asset_tools::names::NameDict;
use crate::asset_tools::palette;
use crate::asset_tools::report::{ErrorReport, RenderOutputs, RenderReport};
use crate::asset_tools::verb_render::RenderOptions;
use crate::assets::asset_manager::AssetManager;
use crate::assets::pal_file::Palette;
use crate::assets::tmp_file::{TmpFile, TmpTile};
use crate::rules::art_data::ArtRegistry;

/// `RenderReport::kind` for this path.
const KIND_TMP: &str = "tmp";

/// Values of `RenderReport::mode`.
const MODE_ISOMETRIC: &str = "isometric";
const MODE_GRID: &str = "grid";

/// Subdirectory under the output root shared by every render path. Mirrors
/// `verb_render`, which owns the same convention for SHP; both write under
/// `<out>/render/<name>/`.
const RENDER_SUBDIR: &str = "render";

/// Sidecar filename listing the per-tile fields.
const INDEX_TSV_NAME: &str = "index.tsv";

/// Header row of [`INDEX_TSV_NAME`]. The trailing `png` column mirrors
/// `verb_render`'s sidecar so one reader handles both.
const INDEX_TSV_HEADER: &str =
    "index\tpresent\theight\tterrain_type\tramp_type\tpixel_w\tpixel_h\toffset_x\toffset_y\tpng";

/// `png` column for a cell the template leaves empty — no file was written.
const ABSENT_PNG_MARKER: &str = "-";

/// Directory name used when the asset name sanitises to nothing.
const FALLBACK_DIR_NAME: &str = "asset";

/// Integer upscale bounds. 1 is "no scaling"; the ceiling keeps a large
/// template from turning into a gigapixel PNG.
const MIN_SCALE: u32 = 1;
const MAX_SCALE: u32 = 16;

/// Per-image pixel budget (~256 MB of RGBA). A template header claiming an
/// enormous tile size must fail with a report, not an allocation abort.
const MAX_OUTPUT_PIXELS: u64 = 64_000_000;

/// Tiles named in a warning before it summarises.
const MAX_LISTED_TILES_IN_WARNING: usize = 8;

/// Burned-in per-tile caption colour. Warm yellow reads over both the dark
/// checkerboard and typical terrain greens/browns.
const TILE_LABEL_COLOR: [u8; 4] = [255, 230, 0, 255];
/// Header text above the composite.
const HEADER_COLOR: [u8; 4] = [255, 255, 255, 255];
/// Cyan outline around the composite, so its bounds survive a transparent edge.
const ART_OUTLINE_COLOR: [u8; 4] = [0, 200, 255, 255];

/// Gap between a tile's origin and its burned-in caption, in final PNG pixels.
const LABEL_INSET: u32 = 2;

/// Replacement for characters the shared 5x7 font has no glyph for. Those
/// characters advance the pen without drawing, so an unfiltered `.` or `>` in a
/// filename renders as an unexplained gap; a hyphen at least reads as a break.
const UNDRAWABLE_REPLACEMENT: char = '-';

/// Substrings that mark a palette reason as the last-resort "any 768-byte entry
/// in the source archive" scan. Matched case-insensitively so a wording change
/// downgrades the warning rather than breaking the build.
const LAST_RESORT_REASON_MARKERS: [&str; 5] = [
    "768",
    "last-resort",
    "last resort",
    "archive scan",
    "scanned",
];

/// One tile's row in the TSV sidecar. Absent cells keep their row: a hole in a
/// template is a fact about its shape, and dropping the row would shift every
/// index after it.
#[derive(Debug, Clone, PartialEq, Eq)]
struct TileRow {
    index: usize,
    present: bool,
    height: u8,
    terrain_type: u8,
    ramp_type: u8,
    pixel_w: u32,
    pixel_h: u32,
    offset_x: i32,
    offset_y: i32,
    png: String,
}

impl TileRow {
    /// Row for a cell the template leaves empty.
    fn absent(index: usize) -> Self {
        Self {
            index,
            present: false,
            height: 0,
            terrain_type: 0,
            ramp_type: 0,
            pixel_w: 0,
            pixel_h: 0,
            offset_x: 0,
            offset_y: 0,
            png: ABSENT_PNG_MARKER.to_string(),
        }
    }
}

/// A drawn tile's caption and where it hangs, in unscaled composite pixels.
#[derive(Debug, Clone)]
struct Caption {
    origin: (i64, i64),
    text: String,
}

/// Result of applying `--frame` / `--limit` to a tile count.
#[derive(Debug, Clone, PartialEq, Eq)]
struct TileSelection {
    tiles: Vec<usize>,
    /// Tiles the limit excluded. Non-zero means the render is partial.
    dropped: usize,
}

/// Render a TMP template to PNGs plus a machine-readable report.
///
/// Errors are values, not panics: a malformed retail template yields an
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

    // The format sniffer only calls a file `tmp` when its tile size is exactly
    // 60x30, so a valid template with any other tile size reads as `unknown`.
    // The parser is the better gate: attempt it first, and let its own error
    // carry the sniffer's verdict when it fails.
    let tmp = TmpFile::from_bytes(bytes).map_err(|err| {
        let identified = identify::identify(bytes);
        ErrorReport {
            error: format!(
                "TMP parse failed for {name}: {err} (sniffed as {}: {})",
                identified.format, identified.detail
            ),
            hint: Some(format!(
                "`asset info {name}` reports the header fields that were readable"
            )),
        }
    })?;

    let mut warnings: Vec<String> = Vec::new();
    warnings.extend(catalog_warning);
    if opts.house.is_some() {
        // Terrain art never indexes the [16,32) remap band, so applying a
        // scheme would only recolour whatever happens to land there.
        warnings.push(
            "--house was ignored: a terrain template carries no house-colour remap band"
                .to_string(),
        );
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
            "palette `{}` came from the last-resort archive scan ({}) — it renders, which is not evidence it is the right palette",
            load.name, load.reason
        ));
    }
    let render_palette = load.palette;

    let cols = tmp.template_width.max(1) as usize;
    let tile_count = tmp.tiles.len();
    let present_count = tmp.tiles.iter().filter(|slot| slot.is_some()).count();

    let selection =
        select_tiles(tile_count, opts.frame, opts.limit).map_err(|error| ErrorReport {
            error,
            hint: Some(format!(
                "`asset info {name}` lists every tile index and its fields"
            )),
        })?;
    if selection.dropped > 0 {
        warnings.push(format!(
            "selected {} of {} tile cells; {} dropped by --limit {}",
            selection.tiles.len(),
            tile_count,
            selection.dropped,
            opts.limit.max(1)
        ));
    }

    let mode = if opts.isometric {
        MODE_ISOMETRIC
    } else {
        MODE_GRID
    };
    let (composite_w, composite_h) = composite_size(
        tmp.template_width,
        tmp.template_height,
        tmp.tile_width,
        tmp.tile_height,
        opts.isometric,
    )
    .ok_or_else(|| ErrorReport {
        error: format!(
            "a {}x{} template of {}x{} tiles exceeds the {MAX_OUTPUT_PIXELS}-pixel render budget",
            tmp.template_width, tmp.template_height, tmp.tile_width, tmp.tile_height
        ),
        hint: Some(format!(
            "this usually means a corrupt TMP header; `asset info {name}` reports it without rendering"
        )),
    })?;

    // One scale for the whole run: scaling tiles individually would make the
    // composite lie about relative tile sizes and leave `scale` ambiguous.
    let requested_scale = effective_scale(opts.scale, composite_w, composite_h);
    let scale = fit_scale(composite_w, composite_h, requested_scale).ok_or_else(|| ErrorReport {
        error: format!(
            "composite {composite_w}x{composite_h} exceeds the {MAX_OUTPUT_PIXELS}-pixel render budget"
        ),
        hint: Some("pass a smaller `--scale`, or `--frame <N>` for one tile".to_string()),
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
    // A re-run must replace the previous render, not blend with it: a one-tile
    // run after a full-template run would otherwise leave stale PNGs that read
    // as current output.
    clear_stale_outputs(&dir, &sanitised);

    let mut composite = Rgba::checkerboard(composite_w, composite_h);
    if composite.w == 0 || composite.h == 0 {
        return Err(ErrorReport {
            error: format!(
                "could not allocate the {composite_w}x{composite_h} composite for {name}"
            ),
            hint: Some("pass a smaller `--scale`, or `--frame <N>` for one tile".to_string()),
        });
    }

    let mut rows: Vec<TileRow> = Vec::with_capacity(selection.tiles.len());
    let mut captions: Vec<Caption> = Vec::with_capacity(selection.tiles.len());
    let mut tile_paths: Vec<String> = Vec::with_capacity(selection.tiles.len());
    let mut tiles_rendered: Vec<usize> = Vec::with_capacity(selection.tiles.len());
    let mut out_of_bounds: Vec<usize> = Vec::new();
    let mut unreadable: Vec<usize> = Vec::new();

    // Holes are normal retail data, not a failure, so they are counted once up
    // front and the draw loop simply skips them.
    let empty_selected = empty_cell_indices(&tmp.tiles, &selection.tiles);

    for &tile_index in &selection.tiles {
        let Some(tile) = tmp.tiles.get(tile_index).and_then(Option::as_ref) else {
            rows.push(TileRow::absent(tile_index));
            continue;
        };

        let Some(tile_image) = decode_tile(&tmp, tile_index, tile, &render_palette) else {
            unreadable.push(tile_index);
            rows.push(tile_row(tile_index, tile, ABSENT_PNG_MARKER.to_string()));
            continue;
        };

        let origin = tile_origin(
            tile_index,
            cols,
            tmp.tile_width,
            tmp.tile_height,
            opts.isometric,
        );
        // `offset_x`/`offset_y` translate the pixel buffer into diamond-local
        // space, so the buffer's top-left sits at origin + offset.
        let place = (
            origin.0 + i64::from(tile.offset_x),
            origin.1 + i64::from(tile.offset_y),
        );
        if place.0 < 0
            || place.1 < 0
            || place.0 + i64::from(tile_image.w) > i64::from(composite_w)
            || place.1 + i64::from(tile_image.h) > i64::from(composite_h)
        {
            out_of_bounds.push(tile_index);
        }
        canvas::blit_over(&mut composite, &tile_image, place.0, place.1);
        captions.push(Caption {
            origin,
            text: tile_label(tile_index, tile),
        });

        // Per-tile PNGs are exactly `pixel_w x pixel_h * scale` with no markers,
        // so the sidecar's dimensions convert straight to PNG pixels. The
        // backdrop always allocates: `decode_tile` already cleared the same
        // buffer cap for these exact dimensions.
        let mut framed = Rgba::checkerboard(tile_image.w, tile_image.h);
        canvas::blit_over(&mut framed, &tile_image, 0, 0);
        let upscaled = canvas::upscale_nearest(&framed, scale);
        let png_name = tile_png_name(&sanitised, tile_index);
        let png_path = dir.join(&png_name);
        canvas::save_png(&png_path, &upscaled).map_err(|err| ErrorReport {
            error: format!("could not write {}: {err}", png_path.display()),
            hint: Some("pass a writable `--out` root".to_string()),
        })?;

        rows.push(tile_row(tile_index, tile, png_name));
        tile_paths.push(png_path.display().to_string());
        tiles_rendered.push(tile_index);
    }

    if !empty_selected.is_empty() {
        warnings.push(format!(
            "{} of {} selected cells are empty in the template and were skipped ({}); the template has {present_count} of {tile_count} cells filled",
            empty_selected.len(),
            selection.tiles.len(),
            summarise_indices(&empty_selected)
        ));
    }
    if !out_of_bounds.is_empty() {
        warnings.push(format!(
            "{} tile(s) extend past the {composite_w}x{composite_h} composite ({}) — they are clipped there",
            out_of_bounds.len(),
            summarise_indices(&out_of_bounds)
        ));
    }
    if !unreadable.is_empty() {
        warnings.push(format!(
            "{} tile(s) could not be converted and were left out of the composite ({})",
            unreadable.len(),
            summarise_indices(&unreadable)
        ));
    }
    if opts.isometric && !captions.is_empty() {
        warnings.push(
            "isometric captions hang at each tile origin and therefore overlay the neighbouring \
             tile's art; the default grid layout draws them in empty diamond corners"
                .to_string(),
        );
    }

    if tiles_rendered.is_empty() {
        return Err(ErrorReport {
            error: format!("no tiles of {name} could be rendered"),
            hint: Some(if warnings.is_empty() {
                format!("`asset info {name}` reports the tile table")
            } else {
                warnings.join("; ")
            }),
        });
    }

    let index_path = dir.join(INDEX_TSV_NAME);
    std::fs::write(&index_path, index_tsv(&rows)).map_err(|err| ErrorReport {
        error: format!("could not write {}: {err}", index_path.display()),
        hint: Some("pass a writable `--out` root".to_string()),
    })?;

    // Captions go on *after* magnification so the 5x7 font stays 5x7, and after
    // every tile is composited so a later neighbour cannot bury an earlier one.
    let mut sheet_art = canvas::upscale_nearest(&composite, scale);
    draw_captions(&mut sheet_art, &captions, tmp.tile_width, scale);
    let header = sheet_header(
        name,
        &source_archive,
        &tmp,
        tiles_rendered.len(),
        tile_count,
        present_count,
        composite_w,
        composite_h,
        scale,
        mode,
        &choice.name,
        &choice.reason,
    );
    let sheet = assemble_sheet(&header, &sheet_art);
    let sheet_path = dir.join(sheet_png_name(&sanitised));
    canvas::save_png(&sheet_path, &sheet).map_err(|err| ErrorReport {
        error: format!("could not write {}: {err}", sheet_path.display()),
        hint: Some("pass a writable `--out` root".to_string()),
    })?;

    Ok(RenderReport {
        kind: KIND_TMP.to_string(),
        asset: name.to_string(),
        source_archive,
        palette: Some(choice),
        // Recorded as unset because it was not applied — see the warning above.
        house_color: None,
        canvas: [composite_w, composite_h],
        frame_count: tile_count,
        frames_rendered: tiles_rendered,
        scale,
        mode: mode.to_string(),
        warnings,
        outputs: RenderOutputs {
            dir: dir.display().to_string(),
            sheet: Some(sheet_path.display().to_string()),
            frames: tile_paths,
            index: Some(index_path.display().to_string()),
        },
    })
}

/// Decode one tile into an RGBA buffer, or `None` when the pixel data and the
/// declared dimensions disagree.
fn decode_tile(
    tmp: &TmpFile,
    tile_index: usize,
    tile: &TmpTile,
    palette: &Palette,
) -> Option<Rgba> {
    if tile.pixel_width == 0 || tile.pixel_height == 0 {
        return None;
    }
    let rgba = tmp.tile_to_rgba(tile_index, palette).ok()?;
    Rgba::from_raw(rgba, tile.pixel_width, tile.pixel_height)
}

fn tile_row(index: usize, tile: &TmpTile, png: String) -> TileRow {
    TileRow {
        index,
        present: true,
        height: tile.height,
        terrain_type: tile.terrain_type,
        ramp_type: tile.ramp_type,
        pixel_w: tile.pixel_width,
        pixel_h: tile.pixel_height,
        offset_x: tile.offset_x,
        offset_y: tile.offset_y,
        png,
    }
}

/// Top-left of tile `index`'s diamond, in unscaled composite pixels.
///
/// The isometric arm is the half-cell stagger: odd rows shift right by half a
/// tile and every row advances by half a tile height, which is what makes the
/// diamonds interlock.
fn tile_origin(index: usize, cols: usize, tile_w: u32, tile_h: u32, isometric: bool) -> (i64, i64) {
    let cols = cols.max(1);
    let col = (index % cols) as i64;
    let row = (index / cols) as i64;
    let tile_w = i64::from(tile_w);
    let tile_h = i64::from(tile_h);
    if isometric {
        (col * tile_w + (row & 1) * (tile_w / 2), row * tile_h / 2)
    } else {
        (col * tile_w, row * tile_h)
    }
}

/// Composite dimensions for a whole template, or `None` when the header's
/// numbers put it past the render budget.
///
/// The isometric arm carries a half tile of slack in each axis: odd rows hang
/// half a tile off the right edge, and the final row's diamond extends a full
/// tile height below its own origin.
fn composite_size(
    template_w: u32,
    template_h: u32,
    tile_w: u32,
    tile_h: u32,
    isometric: bool,
) -> Option<(u32, u32)> {
    // u64 throughout: `tile_w`/`tile_h` are raw file dwords, and the parser only
    // enforces a lower bound on them.
    let cols = u64::from(template_w.max(1));
    let rows = u64::from(template_h.max(1));
    let tile_w = u64::from(tile_w);
    let tile_h = u64::from(tile_h);
    let (w, h) = if isometric {
        (cols * tile_w + tile_w / 2, rows * tile_h / 2 + tile_h)
    } else {
        (cols * tile_w, rows * tile_h)
    };
    if w.checked_mul(h)? > MAX_OUTPUT_PIXELS {
        return None;
    }
    Some((u32::try_from(w).ok()?.max(1), u32::try_from(h).ok()?.max(1)))
}

/// Burn each drawn tile's caption into the magnified composite.
///
/// The caption hangs at the tile's origin because in the grid layout that is the
/// empty corner outside the diamond — the one place a label covers no art.
fn draw_captions(art: &mut Rgba, captions: &[Caption], tile_w: u32, scale: u32) {
    let scale = scale.max(1);
    let cell_w = tile_w.saturating_mul(scale);
    let step = i64::from(scale);
    for caption in captions {
        let text = fit_label(&caption.text, cell_w);
        if text.is_empty() {
            continue;
        }
        canvas::draw_text(
            art,
            caption.origin.0 * step + i64::from(LABEL_INSET),
            caption.origin.1 * step + i64::from(LABEL_INSET),
            &text,
            TILE_LABEL_COLOR,
        );
    }
}

/// `t<index> h<height> r<ramp>` — index plus the two fields that decide how the
/// cell behaves underfoot. Built from letters, digits and spaces only, because
/// the shared 5x7 table has no glyph for punctuation beyond `-`, `:` and `/`.
fn tile_label(index: usize, tile: &TmpTile) -> String {
    format!("t{index} h{} r{}", tile.height, tile.ramp_type)
}

/// Header lines above the composite. An agent that only looks at the image must
/// still know which palette and which layout produced it.
fn sheet_header(
    name: &str,
    source_archive: &str,
    tmp: &TmpFile,
    drawn: usize,
    tile_count: usize,
    present_count: usize,
    composite_w: u32,
    composite_h: u32,
    scale: u32,
    mode: &str,
    palette_name: &str,
    palette_reason: &str,
) -> Vec<String> {
    vec![
        glyphable(&format!("{name}   {source_archive}")),
        glyphable(&format!(
            "template {}x{}   tiles {}x{}   filled {present_count}/{tile_count}",
            tmp.template_width, tmp.template_height, tmp.tile_width, tmp.tile_height
        )),
        glyphable(&format!(
            "drawn {drawn}   composite {composite_w}x{composite_h}   scale {scale}x   mode {mode}"
        )),
        glyphable(&format!("palette {palette_name}   {palette_reason}")),
        glyphable("captions are t index  h height  r ramp type"),
    ]
}

/// Frame the magnified composite with its header block.
fn assemble_sheet(header: &[String], art: &Rgba) -> Rgba {
    let header_text_w = header
        .iter()
        .map(|line| canvas::text_width(line))
        .max()
        .unwrap_or(0);
    let header_h = if header.is_empty() {
        0
    } else {
        (header.len() as u32)
            .saturating_mul(canvas::LABEL_HEIGHT)
            .saturating_add(canvas::SHEET_GAP)
    };

    let sheet_w = art
        .w
        .max(header_text_w)
        .max(1)
        .saturating_add(canvas::SHEET_PADDING * 2);
    let sheet_h = header_h
        .saturating_add(art.h)
        .max(1)
        .saturating_add(canvas::SHEET_PADDING * 2);

    let mut sheet = Rgba::checkerboard(sheet_w, sheet_h);
    if sheet.w == 0 || sheet.h == 0 {
        // The allocation cap rejected the framed layout; the bare composite is
        // still worth writing, so hand that back rather than nothing.
        return art.clone();
    }

    for (line_index, line) in header.iter().enumerate() {
        let y =
            i64::from(canvas::SHEET_PADDING) + line_index as i64 * i64::from(canvas::LABEL_HEIGHT);
        canvas::draw_text(
            &mut sheet,
            i64::from(canvas::SHEET_PADDING),
            y,
            line,
            HEADER_COLOR,
        );
    }

    let art_x = i64::from(canvas::SHEET_PADDING);
    let art_y = i64::from(canvas::SHEET_PADDING) + i64::from(header_h);
    canvas::blit_over(&mut sheet, art, art_x, art_y);
    // One pixel outside the art, so the outline costs padding rather than tiles.
    canvas::draw_rect_outline(
        &mut sheet,
        art_x - 1,
        art_y - 1,
        art.w.saturating_add(2),
        art.h.saturating_add(2),
        ART_OUTLINE_COLOR,
    );
    sheet
}

/// True for characters the shared 5x7 table can actually draw.
fn is_drawable(character: char) -> bool {
    matches!(character, ' ' | '-' | ':' | '/')
        || character.is_ascii_digit()
        || character.is_ascii_alphabetic()
}

/// Replace every character the font cannot draw, so a filename's `.` or an
/// archive chain's `->` reads as a break instead of an unexplained gap.
fn glyphable(text: &str) -> String {
    text.chars()
        .map(|character| {
            if is_drawable(character) {
                character
            } else {
                UNDRAWABLE_REPLACEMENT
            }
        })
        .collect()
}

/// Longest prefix of `text` that fits `width` pixels of the shared 5x7 font.
///
/// Metrics are read back out of `canvas::text_width` rather than hardcoded, so a
/// font change cannot silently start overflowing a cell.
fn fit_label(text: &str, width: u32) -> String {
    let single = canvas::text_width("0");
    let advance = canvas::text_width("00").saturating_sub(single);
    if advance == 0 {
        return String::new();
    }
    // n glyphs occupy n * advance - spacing, so n <= (width + spacing) / advance.
    let spacing = advance.saturating_sub(single);
    let budget = (width.saturating_add(spacing) / advance) as usize;
    text.chars().take(budget).collect()
}

/// Apply `--frame` / `--limit` to a tile count.
fn select_tiles(
    tile_count: usize,
    requested: Option<usize>,
    limit: usize,
) -> Result<TileSelection, String> {
    if tile_count == 0 {
        return Err("TMP declares zero tile cells".to_string());
    }
    if let Some(index) = requested {
        if index >= tile_count {
            return Err(format!(
                "tile {index} is out of range; the template has {tile_count} cells (0..{})",
                tile_count - 1
            ));
        }
        return Ok(TileSelection {
            tiles: vec![index],
            dropped: 0,
        });
    }
    // A zero limit would render nothing and report success; treat it as one.
    let take = limit.max(1).min(tile_count);
    Ok(TileSelection {
        tiles: (0..take).collect(),
        dropped: tile_count - take,
    })
}

/// Selected cells the template leaves empty, in selection order.
///
/// An index past the end of the table counts as empty rather than panicking:
/// the selection is derived from `tiles.len()`, so that can only happen if a
/// caller hands in indices from a different template.
fn empty_cell_indices(tiles: &[Option<TmpTile>], selected: &[usize]) -> Vec<usize> {
    selected
        .iter()
        .copied()
        .filter(|index| tiles.get(*index).is_none_or(Option::is_none))
        .collect()
}

fn clamp_scale(scale: u32) -> u32 {
    scale.clamp(MIN_SCALE, MAX_SCALE)
}

fn effective_scale(requested: Option<u32>, w: u32, h: u32) -> u32 {
    clamp_scale(requested.unwrap_or_else(|| canvas::choose_scale(w, h)))
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

/// Matched case-insensitively against `PaletteLoad::reason`.
fn is_last_resort_palette(reason: &str) -> bool {
    let lowered = reason.to_ascii_lowercase();
    LAST_RESORT_REASON_MARKERS
        .iter()
        .any(|marker| lowered.contains(marker))
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

/// `<sanitised>.<index:03>.png`, matching the SHP path's naming so one listing
/// convention covers every renderable format.
fn tile_png_name(sanitised: &str, index: usize) -> String {
    format!("{sanitised}.{index:03}.png")
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

fn index_tsv_row(row: &TileRow) -> String {
    format!(
        "{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}",
        row.index,
        row.present,
        row.height,
        row.terrain_type,
        row.ramp_type,
        row.pixel_w,
        row.pixel_h,
        row.offset_x,
        row.offset_y,
        row.png
    )
}

fn index_tsv(rows: &[TileRow]) -> String {
    let mut text = String::with_capacity(INDEX_TSV_HEADER.len() + rows.len() * 48);
    text.push_str(INDEX_TSV_HEADER);
    text.push('\n');
    for row in rows {
        text.push_str(&index_tsv_row(row));
        text.push('\n');
    }
    text
}

fn summarise_indices(indices: &[usize]) -> String {
    let listed: Vec<String> = indices
        .iter()
        .take(MAX_LISTED_TILES_IN_WARNING)
        .map(usize::to_string)
        .collect();
    if indices.len() > MAX_LISTED_TILES_IN_WARNING {
        format!(
            "{}, +{} more",
            listed.join(", "),
            indices.len() - MAX_LISTED_TILES_IN_WARNING
        )
    } else {
        listed.join(", ")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Retail RA2/YR isometric tile geometry.
    const TILE_W: u32 = 60;
    const TILE_H: u32 = 30;

    fn tile(height: u8, ramp: u8) -> TmpTile {
        TmpTile {
            height,
            terrain_type: 0,
            ramp_type: ramp,
            radar_left: [0; 3],
            radar_right: [0; 3],
            pixels: vec![0; (TILE_W * TILE_H) as usize],
            depth: vec![0; (TILE_W * TILE_H) as usize],
            pixel_width: TILE_W,
            pixel_height: TILE_H,
            relative_extra_y: 0,
            offset_x: 0,
            offset_y: 0,
            has_damaged_data: false,
        }
    }

    /// `present` is row-major, one flag per template cell.
    fn template(cols: u32, rows: u32, present: &[bool]) -> TmpFile {
        assert_eq!(present.len(), (cols * rows) as usize);
        TmpFile {
            template_width: cols,
            template_height: rows,
            tile_width: TILE_W,
            tile_height: TILE_H,
            tiles: present
                .iter()
                .map(|filled| filled.then(|| tile(0, 0)))
                .collect(),
        }
    }

    #[test]
    fn isometric_origins_stagger_odd_rows_by_half_a_tile() {
        let origin = |index| tile_origin(index, 3, TILE_W, TILE_H, true);
        // Row 0 sits flush; every row drops half a tile height.
        assert_eq!(origin(0), (0, 0));
        assert_eq!(origin(1), (60, 0));
        assert_eq!(origin(2), (120, 0));
        // Row 1 is the staggered one: +30 in x, +15 in y.
        assert_eq!(origin(3), (30, 15));
        assert_eq!(origin(4), (90, 15));
        assert_eq!(origin(5), (150, 15));
        // Row 2 returns to the unstaggered column positions.
        assert_eq!(origin(6), (0, 30));
        assert_eq!(origin(7), (60, 30));
        assert_eq!(origin(8), (120, 30));
    }

    #[test]
    fn a_single_column_template_stacks_without_stagger_drift() {
        // cols = 1, so every index is its own row and the stagger alternates.
        let origin = |index| tile_origin(index, 1, TILE_W, TILE_H, true);
        assert_eq!(origin(0), (0, 0));
        assert_eq!(origin(1), (30, 15));
        assert_eq!(origin(2), (0, 30));
        assert_eq!(origin(3), (30, 45));
        // A zero column count must not divide by zero.
        assert_eq!(tile_origin(2, 0, TILE_W, TILE_H, false), (0, 60));
    }

    #[test]
    fn every_tile_of_a_full_template_lands_inside_its_composite() {
        for isometric in [false, true] {
            let (w, h) = composite_size(3, 3, TILE_W, TILE_H, isometric).expect("fits");
            for index in 0..9 {
                let (x, y) = tile_origin(index, 3, TILE_W, TILE_H, isometric);
                assert!(x >= 0 && y >= 0, "tile {index} origin {x}:{y}");
                assert!(
                    x + i64::from(TILE_W) <= i64::from(w),
                    "tile {index} overflows width in isometric={isometric}"
                );
                assert!(
                    y + i64::from(TILE_H) <= i64::from(h),
                    "tile {index} overflows height in isometric={isometric}"
                );
            }
        }
    }

    #[test]
    fn composite_size_refuses_a_header_past_the_render_budget() {
        // A corrupt tile size must fail with a report, not an allocation abort.
        assert_eq!(composite_size(1, 1, u32::MAX, u32::MAX, false), None);
        assert_eq!(composite_size(255, 255, 60, 30, false), None);
        // Zero template dimensions are treated as one cell rather than dividing.
        assert_eq!(composite_size(0, 0, TILE_W, TILE_H, false), Some((60, 30)));
    }

    #[test]
    fn empty_cells_are_counted_not_treated_as_a_failure() {
        let tmp = template(
            3,
            3,
            &[true, false, true, false, true, false, true, true, false],
        );
        let all: Vec<usize> = (0..9).collect();
        assert_eq!(empty_cell_indices(&tmp.tiles, &all), vec![1, 3, 5, 8]);
        // Counting follows the selection, not the whole template.
        assert!(empty_cell_indices(&tmp.tiles, &[0, 2, 4]).is_empty());
        assert_eq!(empty_cell_indices(&tmp.tiles, &[1, 3, 5]), vec![1, 3, 5]);
        // An index past the end counts as absent rather than panicking.
        assert_eq!(empty_cell_indices(&tmp.tiles, &[99]), vec![99]);
    }

    #[test]
    fn select_tiles_honours_an_explicit_index() {
        assert_eq!(
            select_tiles(9, Some(4), 64).unwrap(),
            TileSelection {
                tiles: vec![4],
                dropped: 0
            }
        );
        // An explicit tile ignores the limit entirely.
        assert_eq!(
            select_tiles(9, Some(8), 1).unwrap(),
            TileSelection {
                tiles: vec![8],
                dropped: 0
            }
        );
        let err = select_tiles(9, Some(9), 64).unwrap_err();
        assert!(err.contains("out of range"), "{err}");
        assert!(err.contains("0..8"), "{err}");
    }

    #[test]
    fn select_tiles_applies_the_limit_and_reports_the_drop() {
        assert_eq!(
            select_tiles(4, None, 64).unwrap(),
            TileSelection {
                tiles: vec![0, 1, 2, 3],
                dropped: 0
            }
        );
        let capped = select_tiles(144, None, 5).unwrap();
        assert_eq!(capped.tiles, vec![0, 1, 2, 3, 4]);
        assert_eq!(capped.dropped, 139);
        // A zero limit must not silently render nothing.
        let zero = select_tiles(6, None, 0).unwrap();
        assert_eq!(zero.tiles, vec![0]);
        assert_eq!(zero.dropped, 5);
        assert!(select_tiles(0, None, 64).is_err());
    }

    #[test]
    fn tile_captions_use_only_glyphs_the_font_can_draw() {
        let label = tile_label(12, &tile(5, 3));
        assert_eq!(label, "t12 h5 r3");
        assert!(
            label.chars().all(is_drawable),
            "caption {label:?} would render with gaps"
        );
        for line in sheet_header(
            "clat01.tem",
            "ra2.mix -> isotemp.mix",
            &template(1, 1, &[true]),
            1,
            1,
            1,
            90,
            45,
            11,
            MODE_ISOMETRIC,
            "isotem.pal",
            "archive-map:isotemp",
        ) {
            assert!(line.chars().all(is_drawable), "header line {line:?}");
        }
    }

    #[test]
    fn glyphable_replaces_characters_the_font_cannot_draw() {
        assert_eq!(glyphable("clat01.tem"), "clat01-tem");
        assert_eq!(
            glyphable("ra2.mix -> isotemp.mix"),
            "ra2-mix -- isotemp-mix"
        );
        // Everything the table does cover survives untouched.
        assert_eq!(glyphable("t12 h5 r3"), "t12 h5 r3");
        assert_eq!(glyphable("a-b:c/d 9Z"), "a-b:c/d 9Z");
    }

    #[test]
    fn fit_label_never_overflows_the_cell_width() {
        for width in 0..200u32 {
            let fitted = fit_label("t144 h14 r7", width);
            assert!(
                canvas::text_width(&fitted) <= width || fitted.is_empty(),
                "{fitted:?} overflows width {width}"
            );
        }
        // A wide cell keeps the whole caption.
        assert_eq!(fit_label("t12 h5 r3", 1024), "t12 h5 r3");
        // A one-pixel cell keeps nothing.
        assert_eq!(fit_label("t12 h5 r3", 1), "");
    }

    #[test]
    fn index_tsv_rows_carry_every_tile_field() {
        let rows = vec![
            TileRow {
                index: 0,
                present: true,
                height: 5,
                terrain_type: 1,
                ramp_type: 3,
                pixel_w: 60,
                pixel_h: 34,
                offset_x: 0,
                offset_y: -4,
                png: "clat01.tem.000.png".to_string(),
            },
            TileRow::absent(1),
        ];
        assert_eq!(
            index_tsv_row(&rows[0]),
            "0\ttrue\t5\t1\t3\t60\t34\t0\t-4\tclat01.tem.000.png"
        );
        assert_eq!(index_tsv_row(&rows[1]), "1\tfalse\t0\t0\t0\t0\t0\t0\t0\t-");

        let text = index_tsv(&rows);
        let mut lines = text.lines();
        assert_eq!(lines.next(), Some(INDEX_TSV_HEADER));
        assert_eq!(lines.next(), Some(index_tsv_row(&rows[0]).as_str()));
        assert_eq!(lines.next(), Some(index_tsv_row(&rows[1]).as_str()));
        assert_eq!(lines.next(), None);
        assert!(text.ends_with('\n'));
        // Every enumerated column is present, in order, before the png column.
        assert_eq!(INDEX_TSV_HEADER.split('\t').count(), 10);
    }

    #[test]
    fn scale_is_clamped_and_fitted_to_the_pixel_budget() {
        assert_eq!(clamp_scale(0), MIN_SCALE);
        assert_eq!(clamp_scale(999), MAX_SCALE);
        assert_eq!(effective_scale(Some(4), 180, 90), 4);
        // A 3x3 grid composite is small enough to magnify hard.
        assert!(effective_scale(None, 180, 90) > 1);
        assert_eq!(fit_scale(180, 90, 5), Some(5));
        // A large composite is reduced rather than refused.
        let fitted = fit_scale(4000, 4000, 16).unwrap();
        assert!((MIN_SCALE..16).contains(&fitted), "{fitted}");
        assert_eq!(fit_scale(60_000, 60_000, 1), None);
    }

    #[test]
    fn assemble_sheet_leaves_room_for_the_header_and_the_art() {
        let art = Rgba::new_filled(180, 90, [10, 10, 10, 255]);
        let header = vec!["clat01-tem".to_string(), "template 3x3".to_string()];
        let sheet = assemble_sheet(&header, &art);
        assert!(sheet.w >= art.w + canvas::SHEET_PADDING * 2);
        assert!(sheet.h >= art.h + canvas::SHEET_PADDING * 2 + 2 * canvas::LABEL_HEIGHT);
        assert_eq!(sheet.data.len(), sheet.pixel_count() * 4);
    }

    #[test]
    fn draw_captions_draws_on_canvas_and_clips_everything_else() {
        let mut art = Rgba::checkerboard(64, 64);
        let before = art.data.len();
        draw_captions(
            &mut art,
            &[
                Caption {
                    origin: (0, 0),
                    text: "t0 h0 r0".to_string(),
                },
                // Far off both edges: these must clip, not panic or wrap.
                Caption {
                    origin: (-10_000, -10_000),
                    text: "t1 h0 r0".to_string(),
                },
                Caption {
                    origin: (10_000, 10_000),
                    text: "t2 h0 r0".to_string(),
                },
            ],
            TILE_W,
            1,
        );
        assert_eq!(art.data.len(), before);

        let lit = art
            .data
            .as_chunks::<4>()
            .0
            .iter()
            .filter(|px| px[0] == TILE_LABEL_COLOR[0] && px[1] == TILE_LABEL_COLOR[1])
            .count();
        assert!(lit > 0, "the on-canvas caption should light some pixels");
    }
}
