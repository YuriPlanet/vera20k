//! `asset render` — turn a retail SHP into PNGs an agent can actually look at.
//!
//! Phase 1 renders SHP only. The verb exists to catch placement and palette bugs
//! that a naive frame dump hides, so every default here is diagnostic rather than
//! pretty: frames are composited into the *file canvas* at their `(frame_x,
//! frame_y)` origin, transparency sits on a checkerboard, the canvas bounds,
//! canvas origin, and frame sub-rect are outlined, and the palette that produced
//! the pixels is printed onto the contact sheet so an agent looking only at the
//! image still knows what it is looking at.
//!
//! Output dimensions are exactly `canvas * scale` — every marker is drawn inside
//! the canvas, never in a border, so `RenderReport::scale` is enough to convert
//! PNG pixels back to asset pixels.
//!
//! ## Dependency rules
//! - Part of `asset_tools/`: depends on `assets/` (SHP, palette, archive
//!   resolution), `rules/` (`[Colors]` schemes and the art registry) and the
//!   sibling `canvas` / `identify` / `names` / `palette` / `report` modules.
//! - Nothing from `sim/`, `render/`, `ui/`, `audio/`, or `net/`.

use std::path::{Path, PathBuf};

use crate::asset_tools::canvas::{self, Rgba, SheetCell};
use crate::asset_tools::identify;
use crate::asset_tools::names::NameDict;
use crate::asset_tools::palette;
use crate::asset_tools::report::{ErrorReport, RenderOutputs, RenderReport};
use crate::assets::asset_manager::AssetManager;
use crate::assets::pal_file::Palette;
use crate::assets::shp_file::ShpFile;
use crate::rules::art_data::ArtRegistry;
use crate::rules::color_scheme::parse_color_schemes;
use crate::rules::house_colors::{HouseColorIndex, HouseColorRamps};
use crate::rules::ini_parser::IniFile;

/// Frames rendered in one call unless the caller raises it. Retail infantry SHPs
/// run past 500 frames; dumping all of them costs minutes and buries the answer.
const DEFAULT_FRAME_LIMIT: usize = 64;

/// Output root when the caller does not name one. Relative, so it lands beside
/// the build products rather than in a user directory.
const DEFAULT_OUT_ROOT: &str = "target/asset";

/// Subdirectory under the output root that this verb owns.
const RENDER_SUBDIR: &str = "render";

/// Sidecar filename listing the geometry of every rendered frame.
const INDEX_TSV_NAME: &str = "index.tsv";

/// Header row of [`INDEX_TSV_NAME`].
const INDEX_TSV_HEADER: &str = "index\tx\ty\tw\th\tformat\tcompressed\tpng";

/// Format tag `identify` returns for a sprite file.
const FORMAT_SHP: &str = "shp";

/// Frame header byte-8 bit that selects row-framed RLE-Zero data.
const SHP_FORMAT_RLE_ZERO_BIT: u8 = 0x02;

/// Values of `RenderReport::mode`.
const MODE_CANVAS: &str = "canvas";
const MODE_CROP: &str = "crop";

/// Cyan: the file canvas bounds.
const CANVAS_OUTLINE_COLOR: [u8; 4] = [0, 200, 255, 255];
/// Magenta: the frame sub-rect inside that canvas.
const FRAME_RECT_COLOR: [u8; 4] = [255, 0, 220, 255];
/// Yellow: canvas origin (0,0).
const ORIGIN_CROSSHAIR_COLOR: [u8; 4] = [255, 230, 0, 255];
/// Crosshair arm length in unscaled canvas pixels. Short on purpose — the marker
/// covers art at the origin, and a long arm would hide a small sprite entirely.
const ORIGIN_CROSSHAIR_ARM: u32 = 3;

/// Integer upscale bounds. 1 is "no scaling"; the ceiling keeps a 2048x2048
/// canvas from turning into a gigapixel PNG.
const MIN_SCALE: u32 = 1;
const MAX_SCALE: u32 = 16;

/// Per-image pixel budget (~256 MB of RGBA). A malformed header claiming a huge
/// canvas must fail with a report, not an allocation abort.
const MAX_OUTPUT_PIXELS: u64 = 64_000_000;

/// Frames named in the "extends past the canvas" warning before it summarises.
const MAX_LISTED_FRAMES_IN_WARNING: usize = 8;

/// Directory name used when the asset name sanitises to nothing.
const FALLBACK_DIR_NAME: &str = "asset";

/// Its pixel values are brightness levels consumed by the shroud blitter, not
/// palette indices, so any palette render of it is meaningless.
const SHROUD_ASSET: &str = "SHROUD.SHP";

/// Rules files searched for `[Colors]`, YR first per the INI authority order.
const RULES_INI_CANDIDATES: [&str; 2] = ["rulesmd.ini", "rules.ini"];

/// Substrings that mark a palette reason as the last-resort "any 768-byte entry
/// in the source archive" scan. Matched case-insensitively against the reason
/// text so a wording change downgrades the warning rather than breaking the
/// build.
const LAST_RESORT_REASON_MARKERS: [&str; 5] = [
    "768",
    "last-resort",
    "last resort",
    "archive scan",
    "scanned",
];

/// Options for one `asset render` invocation.
#[derive(Debug, Clone)]
pub struct RenderOptions {
    /// None = every frame. Some(n) = just frame n.
    pub frame: Option<usize>,
    /// Explicit palette filename override, e.g. "sidebar.pal".
    pub palette: Option<String>,
    /// House colour scheme index for the [16,32) remap band.
    pub house: Option<u8>,
    /// Draw the bare frame sub-rect instead of the full file canvas.
    pub crop: bool,
    /// Explicit integer upscale. None = canvas::choose_scale.
    pub scale: Option<u32>,
    /// Output root. Files land in <out>/render/<sanitised-name>/.
    pub out: PathBuf,
    /// Cap on frames rendered in one call, to bound a 600-frame SHP.
    pub limit: usize,

    // --- Phase 2: format-specific knobs, ignored by the SHP path ---
    /// TMP only: compose the template as an isometric diamond rather than a grid.
    pub isometric: bool,
    /// VXL only: facing bytes to render. Empty means the default single facing.
    pub facings: Vec<u8>,
    /// VXL only: explicit .vpl for voxel lighting. None uses `voxels.vpl`.
    pub vpl: Option<String>,
    /// PCX only: palette index to treat as transparent.
    pub transparent_index: Option<u8>,
}

impl Default for RenderOptions {
    fn default() -> Self {
        Self {
            frame: None,
            palette: None,
            house: None,
            crop: false,
            scale: None,
            out: PathBuf::from(DEFAULT_OUT_ROOT),
            limit: DEFAULT_FRAME_LIMIT,
            isometric: false,
            facings: Vec::new(),
            vpl: None,
            transparent_index: None,
        }
    }
}

/// Geometry of one SHP frame inside the file canvas.
#[derive(Debug, Clone, Copy)]
struct FrameGeometry {
    x: u16,
    y: u16,
    w: u16,
    h: u16,
}

/// One rendered frame's row in the TSV sidecar.
#[derive(Debug, Clone)]
struct FrameRow {
    index: usize,
    geom: FrameGeometry,
    format: u8,
    png: String,
}

/// Result of applying `--frame` / `--limit` to a frame count.
#[derive(Debug, Clone, PartialEq, Eq)]
struct FrameSelection {
    frames: Vec<usize>,
    /// Frames the limit excluded. Non-zero means the render is partial.
    dropped: usize,
}

/// Render an SHP to PNGs plus a machine-readable report.
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
    if identified.format != FORMAT_SHP {
        return Err(ErrorReport {
            error: format!(
                "phase 1 `asset render` renders SHP only; {name} is {} ({})",
                identified.format, identified.detail
            ),
            hint: Some(format!(
                "use `asset info {name}` — it reports every parsed format without rendering"
            )),
        });
    }

    let shp = ShpFile::from_bytes(bytes).map_err(|err| ErrorReport {
        error: format!("SHP parse failed for {name}: {err}"),
        hint: Some(format!(
            "`asset info {name}` reports the header fields that were readable"
        )),
    })?;

    let mut warnings: Vec<String> = Vec::new();
    warnings.extend(catalog_warning);
    if is_shroud_asset(name) {
        warnings.push(format!(
            "{name} stores shroud brightness levels, not palette indices — the rendered colours are not meaningful art"
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
            "palette `{}` came from the last-resort archive scan ({}) — it renders, which is not evidence it is the right palette",
            load.name, load.reason
        ));
    }

    // Rules are only loaded for --house: parsing rulesmd.ini is a real startup
    // cost that a plain render must not pay.
    let render_palette: Palette = match opts.house {
        Some(index) => apply_house_color(asset_manager, &load.palette, index, &mut warnings)?,
        None => load.palette,
    };

    let selection =
        select_frames(shp.frames.len(), opts.frame, opts.limit).map_err(|error| ErrorReport {
            error,
            hint: Some(format!(
                "`asset info {name}` lists every frame index and its geometry"
            )),
        })?;
    if selection.dropped > 0 {
        warnings.push(format!(
            "rendered {} of {} frames; {} dropped by --limit {}",
            selection.frames.len(),
            shp.frames.len(),
            selection.dropped,
            opts.limit.max(1)
        ));
    }

    let canvas_w = u32::from(shp.width);
    let canvas_h = u32::from(shp.height);
    if canvas_w == 0 || canvas_h == 0 {
        warnings.push(format!(
            "SHP header declares a {canvas_w}x{canvas_h} canvas; frames were drawn on a 1x1 minimum instead"
        ));
    }
    let draw_w = canvas_w.max(1);
    let draw_h = canvas_h.max(1);

    // One scale for the whole run: per-frame scaling would make the contact
    // sheet lie about relative sprite sizes and would leave `scale` ambiguous.
    let requested_scale = effective_scale(opts.scale, draw_w, draw_h);
    let scale = fit_scale(draw_w, draw_h, requested_scale).ok_or_else(|| ErrorReport {
        error: format!(
            "canvas {draw_w}x{draw_h} exceeds the {MAX_OUTPUT_PIXELS}-pixel render budget"
        ),
        hint: Some("this usually means a corrupt SHP header; check `asset info`".to_string()),
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
    // A re-run must replace the previous render, not blend with it: a 3-frame
    // run after a 200-frame run would otherwise leave 197 stale PNGs that read
    // as current output.
    clear_stale_outputs(&dir, &sanitised);

    let mut rows: Vec<FrameRow> = Vec::with_capacity(selection.frames.len());
    let mut cells: Vec<SheetCell> = Vec::with_capacity(selection.frames.len());
    let mut frame_paths: Vec<String> = Vec::with_capacity(selection.frames.len());
    let mut frames_rendered: Vec<usize> = Vec::with_capacity(selection.frames.len());
    let mut out_of_bounds: Vec<usize> = Vec::new();
    let mut unreadable: Vec<usize> = Vec::new();

    for &frame_index in &selection.frames {
        let Some(frame) = shp.frames.get(frame_index) else {
            unreadable.push(frame_index);
            continue;
        };
        let geom = FrameGeometry {
            x: frame.frame_x,
            y: frame.frame_y,
            w: frame.frame_width,
            h: frame.frame_height,
        };

        if !opts.crop
            && (u32::from(geom.x) + u32::from(geom.w) > canvas_w
                || u32::from(geom.y) + u32::from(geom.h) > canvas_h)
        {
            out_of_bounds.push(frame_index);
        }

        let converted = shp.frame_to_rgba(frame_index, &render_palette);
        let frame_image = match converted {
            Ok(rgba) if geom.w > 0 && geom.h > 0 => {
                // A short pixel buffer means the frame decoded partially; report
                // it rather than silently drawing an empty cell.
                let decoded = Rgba::from_raw(rgba, u32::from(geom.w), u32::from(geom.h));
                if decoded.is_none() {
                    unreadable.push(frame_index);
                }
                decoded
            }
            // A zero-sized frame is legal in retail art (blank animation slots);
            // it still gets a canvas so the sheet keeps frame indices aligned.
            Ok(_) => None,
            Err(_) => {
                unreadable.push(frame_index);
                None
            }
        };

        let composed = compose_frame_image(draw_w, draw_h, geom, frame_image.as_ref(), opts.crop);
        let upscaled = canvas::upscale_nearest(&composed, scale);

        let png_name = frame_png_name(&sanitised, frame_index);
        let png_path = dir.join(&png_name);
        canvas::save_png(&png_path, &upscaled).map_err(|err| ErrorReport {
            error: format!("could not write {}: {err}", png_path.display()),
            hint: Some("pass a writable `--out` root".to_string()),
        })?;

        rows.push(FrameRow {
            index: frame_index,
            geom,
            format: frame.format,
            png: png_name,
        });
        cells.push(SheetCell {
            image: upscaled,
            label: cell_label(frame_index, geom),
        });
        frame_paths.push(png_path.display().to_string());
        frames_rendered.push(frame_index);
    }

    if !out_of_bounds.is_empty() {
        warnings.push(format!(
            "{} frame(s) extend past the {canvas_w}x{canvas_h} canvas ({}) — the sub-rect outline is clipped there",
            out_of_bounds.len(),
            summarise_indices(&out_of_bounds)
        ));
    }
    if !unreadable.is_empty() {
        warnings.push(format!(
            "{} frame(s) could not be converted and were drawn empty ({})",
            unreadable.len(),
            summarise_indices(&unreadable)
        ));
    }

    if frames_rendered.is_empty() {
        return Err(ErrorReport {
            error: format!("no frames of {name} could be rendered"),
            hint: Some(if warnings.is_empty() {
                format!("`asset info {name}` reports the frame table")
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

    let mode = if opts.crop { MODE_CROP } else { MODE_CANVAS };

    // One Read call showing forty frames beats forty Read calls.
    let sheet_path = if cells.len() > 1 {
        let header = sheet_header(
            name,
            &source_archive,
            canvas_w,
            canvas_h,
            frames_rendered.len(),
            shp.frames.len(),
            scale,
            mode,
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
        kind: FORMAT_SHP.to_string(),
        asset: name.to_string(),
        source_archive,
        palette: Some(choice),
        house_color: opts.house,
        canvas: [canvas_w, canvas_h],
        frame_count: shp.frames.len(),
        frames_rendered,
        scale,
        mode: mode.to_string(),
        warnings,
        outputs: RenderOutputs {
            dir: dir.display().to_string(),
            sheet: sheet_path,
            frames: frame_paths,
            index: Some(index_path.display().to_string()),
        },
    })
}

/// Build the house-colour remap band from the ruleset's `[Colors]` list.
///
/// The ramps come from the INI every time — a hardcoded retail colour table
/// would be a second source of truth that a mod or a rules edit silently
/// invalidates.
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
            "--house {index} is outside the {} `[Colors]` entries in {ini_name}; the default scheme ramp was used",
            schemes.len()
        ));
    }

    let ramps = HouseColorRamps::from_schemes(&schemes);
    Ok(base.with_house_colors(ramps.ramp(HouseColorIndex(index))))
}

/// Composite one frame and draw the geometry markers.
///
/// Markers live inside the canvas so the PNG stays exactly `canvas * scale`.
/// The sub-rect outline is drawn one pixel *outside* the frame, which costs
/// background pixels rather than art; the origin crosshair does cover art at
/// (0,0), which is the point of it.
fn compose_frame_image(
    canvas_w: u32,
    canvas_h: u32,
    geom: FrameGeometry,
    frame_image: Option<&Rgba>,
    crop: bool,
) -> Rgba {
    let (base_w, base_h, art_x, art_y) = if crop {
        (
            u32::from(geom.w).max(1),
            u32::from(geom.h).max(1),
            0_i64,
            0_i64,
        )
    } else {
        (
            canvas_w.max(1),
            canvas_h.max(1),
            i64::from(geom.x),
            i64::from(geom.y),
        )
    };

    // Checkerboard, so a transparent pixel is distinguishable from a black one.
    let mut image = canvas::Rgba::checkerboard(base_w, base_h);
    if let Some(src) = frame_image {
        canvas::blit_over(&mut image, src, art_x, art_y);
    }

    if !crop {
        canvas::draw_rect_outline(
            &mut image,
            art_x - 1,
            art_y - 1,
            u32::from(geom.w).saturating_add(2),
            u32::from(geom.h).saturating_add(2),
            FRAME_RECT_COLOR,
        );
    }
    canvas::draw_rect_outline(&mut image, 0, 0, base_w, base_h, CANVAS_OUTLINE_COLOR);
    canvas::draw_crosshair(
        &mut image,
        0,
        0,
        ORIGIN_CROSSHAIR_ARM,
        ORIGIN_CROSSHAIR_COLOR,
    );
    image
}

/// Header lines printed above the contact sheet. An agent that only looks at the
/// image must still know which palette produced these colours.
fn sheet_header(
    name: &str,
    source_archive: &str,
    canvas_w: u32,
    canvas_h: u32,
    rendered: usize,
    total: usize,
    scale: u32,
    mode: &str,
    palette_name: &str,
    palette_reason: &str,
    house: Option<u8>,
) -> Vec<String> {
    let mut header = vec![
        format!("{name}   [{source_archive}]"),
        format!(
            "canvas {canvas_w}x{canvas_h}   frames {rendered}/{total}   scale {scale}x   mode {mode}"
        ),
        format!("palette {palette_name} ({palette_reason})"),
    ];
    if let Some(index) = house {
        header.push(format!(
            "house colour scheme {index} applied to indices 16..32"
        ));
    }
    header.push("cyan=canvas  magenta=frame rect  yellow=origin".to_string());
    header
}

/// `f<idx> <w>x<h> at <x>:<y>` — unscaled SHP geometry, not PNG pixels.
///
/// Restricted to letters, digits, space and `:` on purpose: the built-in 5x7
/// glyph table has no `#`, `@` or `,`, and missing glyphs advance without
/// drawing, so the punctuation spelling silently rendered as gaps.
fn cell_label(index: usize, geom: FrameGeometry) -> String {
    format!("f{index} {}x{} at {}:{}", geom.w, geom.h, geom.x, geom.y)
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

/// `<sanitised>.<index:03>.png`. Three digits keeps frames sorted in a file
/// listing; a >999-frame SHP simply widens.
fn frame_png_name(sanitised: &str, index: usize) -> String {
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

fn index_tsv_row(row: &FrameRow) -> String {
    format!(
        "{}\t{}\t{}\t{}\t{}\t0x{:02X}\t{}\t{}",
        row.index,
        row.geom.x,
        row.geom.y,
        row.geom.w,
        row.geom.h,
        row.format,
        row.format & SHP_FORMAT_RLE_ZERO_BIT != 0,
        row.png
    )
}

fn index_tsv(rows: &[FrameRow]) -> String {
    let mut text = String::with_capacity(INDEX_TSV_HEADER.len() + rows.len() * 48);
    text.push_str(INDEX_TSV_HEADER);
    text.push('\n');
    for row in rows {
        text.push_str(&index_tsv_row(row));
        text.push('\n');
    }
    text
}

/// Apply `--frame` / `--limit` to a frame count.
fn select_frames(
    frame_count: usize,
    requested: Option<usize>,
    limit: usize,
) -> Result<FrameSelection, String> {
    if frame_count == 0 {
        return Err("SHP declares zero frames".to_string());
    }
    if let Some(index) = requested {
        if index >= frame_count {
            return Err(format!(
                "frame {index} is out of range; the SHP has {frame_count} frames (0..{})",
                frame_count - 1
            ));
        }
        return Ok(FrameSelection {
            frames: vec![index],
            dropped: 0,
        });
    }
    // A zero limit would render nothing and report success; treat it as one.
    let take = limit.max(1).min(frame_count);
    Ok(FrameSelection {
        frames: (0..take).collect(),
        dropped: frame_count - take,
    })
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

fn is_shroud_asset(name: &str) -> bool {
    let base = name
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or(name)
        .trim()
        .to_ascii_uppercase();
    base == SHROUD_ASSET
}

fn summarise_indices(indices: &[usize]) -> String {
    let listed: Vec<String> = indices
        .iter()
        .take(MAX_LISTED_FRAMES_IN_WARNING)
        .map(usize::to_string)
        .collect();
    if indices.len() > MAX_LISTED_FRAMES_IN_WARNING {
        format!(
            "{}, +{} more",
            listed.join(", "),
            indices.len() - MAX_LISTED_FRAMES_IN_WARNING
        )
    } else {
        listed.join(", ")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn geom(x: u16, y: u16, w: u16, h: u16) -> FrameGeometry {
        FrameGeometry { x, y, w, h }
    }

    #[test]
    fn sanitise_name_keeps_safe_characters_and_replaces_the_rest() {
        assert_eq!(sanitise_name("sidebar.shp"), "sidebar.shp");
        assert_eq!(sanitise_name("gi-idle_01.shp"), "gi-idle_01.shp");
        assert_eq!(sanitise_name("ra2\\dir/na me:x*?"), "ra2_dir_na_me_x__");
        assert_eq!(sanitise_name(""), FALLBACK_DIR_NAME);
        assert_eq!(sanitise_name("///"), "___");
    }

    #[test]
    fn render_dir_is_absolute_and_ends_with_render_subdir() {
        let dir = render_dir(Path::new(DEFAULT_OUT_ROOT), "sidebar.shp");
        assert!(dir.is_absolute(), "{}", dir.display());
        assert!(
            dir.ends_with(Path::new(RENDER_SUBDIR).join("sidebar.shp")),
            "{}",
            dir.display()
        );
    }

    #[test]
    fn generated_output_predicate_matches_only_our_own_files() {
        assert!(is_generated_output("gi.shp.000.png", "gi.shp"));
        assert!(is_generated_output("gi.shp.1234.png", "gi.shp"));
        assert!(is_generated_output("gi.shp.sheet.png", "gi.shp"));
        // Not ours: other assets, other extensions, other shapes.
        assert!(!is_generated_output("gi.shp.000.png", "e1.shp"));
        assert!(!is_generated_output("index.tsv", "gi.shp"));
        assert!(!is_generated_output("gi.shp.notes.png", "gi.shp"));
        assert!(!is_generated_output("gi.shp.000.txt", "gi.shp"));
        assert!(!is_generated_output("gi.shp.png", "gi.shp"));
    }

    #[test]
    fn index_tsv_rows_carry_geometry_format_and_filename() {
        let rows = vec![
            FrameRow {
                index: 0,
                geom: geom(3, 4, 10, 12),
                format: 0x03,
                png: "gi.shp.000.png".to_string(),
            },
            FrameRow {
                index: 5,
                geom: geom(0, 0, 60, 30),
                format: 0x01,
                png: "gi.shp.005.png".to_string(),
            },
        ];
        assert_eq!(
            index_tsv_row(&rows[0]),
            "0\t3\t4\t10\t12\t0x03\ttrue\tgi.shp.000.png"
        );
        assert_eq!(
            index_tsv_row(&rows[1]),
            "5\t0\t0\t60\t30\t0x01\tfalse\tgi.shp.005.png"
        );

        let text = index_tsv(&rows);
        let mut lines = text.lines();
        assert_eq!(lines.next(), Some(INDEX_TSV_HEADER));
        assert_eq!(lines.next(), Some(index_tsv_row(&rows[0]).as_str()));
        assert_eq!(lines.next(), Some(index_tsv_row(&rows[1]).as_str()));
        assert_eq!(lines.next(), None);
        assert!(text.ends_with('\n'));
    }

    #[test]
    fn select_frames_honours_an_explicit_index() {
        assert_eq!(
            select_frames(10, Some(3), 64).unwrap(),
            FrameSelection {
                frames: vec![3],
                dropped: 0
            }
        );
        // An explicit frame ignores the limit entirely.
        assert_eq!(
            select_frames(10, Some(9), 1).unwrap(),
            FrameSelection {
                frames: vec![9],
                dropped: 0
            }
        );
        let err = select_frames(10, Some(10), 64).unwrap_err();
        assert!(err.contains("out of range"), "{err}");
        assert!(err.contains("0..9"), "{err}");
    }

    #[test]
    fn select_frames_applies_the_limit_and_reports_the_drop() {
        assert_eq!(
            select_frames(3, None, 64).unwrap(),
            FrameSelection {
                frames: vec![0, 1, 2],
                dropped: 0
            }
        );
        let capped = select_frames(600, None, 4).unwrap();
        assert_eq!(capped.frames, vec![0, 1, 2, 3]);
        assert_eq!(capped.dropped, 596);
        // A zero limit must not silently render nothing.
        let zero = select_frames(5, None, 0).unwrap();
        assert_eq!(zero.frames, vec![0]);
        assert_eq!(zero.dropped, 4);
        assert!(select_frames(0, None, 64).is_err());
    }

    #[test]
    fn scale_is_clamped_and_fitted_to_the_pixel_budget() {
        assert_eq!(clamp_scale(0), MIN_SCALE);
        assert_eq!(clamp_scale(3), 3);
        assert_eq!(clamp_scale(999), MAX_SCALE);
        assert_eq!(effective_scale(Some(0), 64, 64), MIN_SCALE);
        assert_eq!(effective_scale(Some(4), 64, 64), 4);
        assert_eq!(effective_scale(Some(64), 64, 64), MAX_SCALE);

        // Small canvas keeps the requested scale.
        assert_eq!(fit_scale(60, 30, 8), Some(8));
        // Large canvas is reduced rather than refused.
        let fitted = fit_scale(4000, 4000, 16).unwrap();
        assert!((MIN_SCALE..16).contains(&fitted), "{fitted}");
        assert!(
            u64::from(4000_u32) * u64::from(4000_u32) * u64::from(fitted) * u64::from(fitted)
                <= MAX_OUTPUT_PIXELS
        );
        // A corrupt header claiming a giant canvas fails instead of allocating.
        assert_eq!(fit_scale(60_000, 60_000, 1), None);
        // Zero dimensions are treated as 1 and never divide.
        assert_eq!(fit_scale(0, 0, 2), Some(2));
    }

    #[test]
    fn last_resort_palette_reasons_are_detected_case_insensitively() {
        assert!(is_last_resort_palette("any 768-byte entry in ra2.mix"));
        assert!(is_last_resort_palette("LAST-RESORT archive scan"));
        assert!(is_last_resort_palette("Scanned the source archive"));
        assert!(!is_last_resort_palette("declared by artmd.ini Palette="));
        assert!(!is_last_resort_palette("unittem.pal by theater convention"));
    }

    #[test]
    fn shroud_is_recognised_regardless_of_case_or_path() {
        assert!(is_shroud_asset("shroud.shp"));
        assert!(is_shroud_asset("SHROUD.SHP"));
        assert!(is_shroud_asset("ecache01.mix/shroud.shp"));
        assert!(!is_shroud_asset("shroud.pal"));
        assert!(!is_shroud_asset("gishroud.shp"));
    }

    #[test]
    fn sheet_header_names_the_palette_and_the_marker_colours() {
        let header = sheet_header(
            "gi.shp",
            "ra2.mix -> conquer.mix",
            60,
            30,
            8,
            600,
            4,
            MODE_CANVAS,
            "unittem.pal",
            "theater default",
            Some(2),
        );
        let joined = header.join("\n");
        assert!(joined.contains("gi.shp"));
        assert!(joined.contains("ra2.mix -> conquer.mix"));
        assert!(joined.contains("60x30"));
        assert!(joined.contains("8/600"));
        assert!(joined.contains("scale 4x"));
        assert!(joined.contains("unittem.pal"));
        assert!(joined.contains("theater default"));
        assert!(joined.contains("house colour scheme 2"));
    }
}
