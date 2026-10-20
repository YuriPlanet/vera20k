//! `asset compare <NAME>` — every archive's copy of one name, side by side.
//!
//! Retail ships the same filename in several archives, and the copies are not
//! the same art. `POWERP.SHP` is 12x2 inside `ra2.mix -> sidec01.mix` and 16x2
//! inside `ra2.mix -> sidec02.mix` — the RA2 and the YR sidebar power strip.
//! Theater archives and `ecache` overrides do the same thing all over the
//! corpus, and it is the usual answer to "why does this look different in YR".
//!
//! Nothing else in the tool surfaces it. `asset find` lists where the copies
//! live but says nothing about how they differ, so answering the question by
//! hand means reading two `asset info` blobs side by side and eyeballing the
//! numbers — which is exactly how the `POWERP.SHP` split was found.
//!
//! This verb sweeps for every archive holding the name's entry id, builds the
//! same structural field map `asset scan` queries on, and reports one line per
//! field whose value is not identical everywhere. The structural diff is the
//! answer; the contact sheet is the illustration, and it is SHP-only.
//!
//! ## Dependency rules
//! - Part of `asset_tools/`: depends on `assets/` (parsers, archive access),
//!   `rules/` (the art registry palette inference reads) and the sibling
//!   `canvas` / `identify` / `names` / `palette` / `report` modules.
//! - Nothing from `sim/`, `render/`, `ui/`, `audio/`, or `net/`.

use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::path::{Path, PathBuf};

use crate::asset_tools::canvas::{self, Rgba, SheetCell};
use crate::asset_tools::identify;
use crate::asset_tools::names::NameDict;
use crate::asset_tools::palette;
use crate::asset_tools::report::{CompareReport, CompareVariant, ErrorReport, RenderOutputs};
use crate::assets::asset_manager::AssetManager;
use crate::assets::aud_file;
use crate::assets::csf_file::CsfFile;
use crate::assets::fnt_file::FntFile;
use crate::assets::hva_file::HvaFile;
use crate::assets::mix_hash::mix_hash;
use crate::assets::pcx_file::PcxFile;
use crate::assets::shp_file::ShpFile;
use crate::assets::tmp_file::TmpFile;
use crate::assets::vpl_file::VplFile;
use crate::assets::vxl_file::VxlFile;
use crate::rules::art_data::ArtRegistry;

/// Output root when the caller does not name one. Same default as `render` and
/// `extract`, so one `--out` moves every file-writing verb together.
const DEFAULT_OUT_ROOT: &str = "target/asset";

/// Subdirectory under the output root that this verb owns, a sibling of the
/// `render` tree rather than a child of it.
const COMPARE_SUBDIR: &str = "compare";

/// Directory name used when the asset name sanitises to nothing. Must equal
/// `verb_render`'s, or the same asset lands under two different names.
const FALLBACK_DIR_NAME: &str = "asset";

/// Filename stem of the side-by-side sheet.
const SHEET_STEM: &str = "compare";

/// Format tag `identify` returns for a sprite file — the only one drawn here.
const FORMAT_SHP: &str = "shp";

/// Entries in a .pal file: 256 RGB triplets. Mirrors `verb_scan`.
const PAL_ENTRY_COUNT: usize = 256;

/// Placeholder for a field a variant's format never populated. Only ever seen
/// when the variants sniff to different formats, or one of them failed to parse.
const ABSENT_VALUE: &str = "(absent)";

/// Loose files on disk report their source with this prefix, as in `verb_find`.
const LOOSE_PREFIX: &str = "loose:";

/// How many archive names an aggregate warning spells out before eliding.
const MAX_NAMED_ARCHIVES: usize = 6;

/// Difference lines burned into the sheet header. The full list always reaches
/// the caller through the report; this is just enough for an image-only reader.
const MAX_HEADER_DIFFERENCES: usize = 3;

/// Character budget for one burned-in difference line. A long one would widen
/// the whole sheet, because the layout sizes itself to its widest header line.
const MAX_HEADER_LINE_CHARS: usize = 96;

/// Cyan: the file canvas bounds. Same colour `verb_render` uses for it, and the
/// one marker that matters here — it is what makes a 12x2 canvas visibly
/// different from a 16x2 one once both are centred in a shared cell.
const CANVAS_OUTLINE_COLOR: [u8; 4] = [0, 200, 255, 255];

/// Integer upscale bounds, mirroring `verb_render`.
const MIN_SCALE: u32 = 1;
const MAX_SCALE: u32 = 16;

/// Per-image pixel budget (~256 MB of RGBA), mirroring `verb_render`. A corrupt
/// header claiming a huge canvas must degrade to a report, not an OOM abort.
const MAX_OUTPUT_PIXELS: u64 = 64_000_000;

/// Options for one `asset compare` invocation.
#[derive(Debug, Clone)]
pub struct CompareOptions {
    /// Frame (or tile) rendered from each variant. Default 0.
    pub frame: usize,
    pub scale: Option<u32>,
    pub out: PathBuf,
    /// Skip rendering; report the structural diff only.
    pub no_render: bool,
}

impl Default for CompareOptions {
    fn default() -> Self {
        Self {
            frame: 0,
            scale: None,
            out: PathBuf::from(DEFAULT_OUT_ROOT),
            no_render: false,
        }
    }
}

/// One field whose value is not identical across every variant.
///
/// Kept as structured data rather than a formatted string because it is
/// rendered twice: once for the report, with full archive chains and ordinary
/// punctuation, and once for the contact sheet, where the 5x7 glyph table draws
/// only a subset of ASCII and archive chains are too wide to fit.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Difference {
    field: String,
    /// `(archive, value)` for every variant, in sweep order. A variant whose
    /// format never populated the field carries [`ABSENT_VALUE`].
    values: Vec<(String, String)>,
}

/// A variant plus the bytes behind it, which the report DTO does not carry.
struct Variant<'a> {
    bytes: &'a [u8],
    report: CompareVariant,
}

/// Compare every archive's copy of one name.
///
/// Zero copies is an error. Exactly one is not: "there is only one" is a real
/// answer to the question that was asked, so it reports with `differ: false`
/// and says so in a warning.
pub fn run(
    asset_manager: &AssetManager,
    dict: &NameDict,
    art_registry: &ArtRegistry,
    name: &str,
    opts: &CompareOptions,
) -> Result<CompareReport, ErrorReport> {
    let entry_id = mix_hash(name);
    let holders = sweep_holders(asset_manager, entry_id);

    if holders.is_empty() {
        return Err(ErrorReport {
            error: format!("no archive holds {name}"),
            hint: Some(format!(
                "`asset find {name}` reports both hash spellings and every archive that was searched"
            )),
        });
    }

    // Reachability is decided exactly as `verb_find` decides it: an archive
    // outside the name-lookup index holds real bytes that the engine's own
    // lookup would never resolve, and the two must not be presented alike.
    let reachable: HashSet<String> = asset_manager
        .registered_archive_names()
        .into_iter()
        .collect();

    let mut warnings: Vec<String> = Vec::new();
    let mut variants: Vec<Variant<'_>> = Vec::new();
    let mut parse_failures: Vec<String> = Vec::new();

    for archive_name in &holders {
        let Some(bytes) = asset_manager
            .archive(archive_name)
            .and_then(|archive| archive.get_by_id(entry_id))
        else {
            // The sweep saw the entry a moment ago, so this only fires if the
            // archive name is ambiguous. Report it rather than dropping a copy.
            warnings.push(format!(
                "\"{archive_name}\" holds the entry but its bytes could not be re-read, so it is \
                 not among the variants"
            ));
            continue;
        };

        let identified = identify::identify(bytes);
        let size = bytes.len() as u32;
        let (fields, parse_failed) = variant_fields(identified.format, bytes, size);
        if parse_failed {
            parse_failures.push(archive_name.clone());
        }

        variants.push(Variant {
            bytes,
            report: CompareVariant {
                archive: archive_name.clone(),
                entry_id: format!("{:#010X}", entry_id as u32),
                size,
                format: identified.format.to_string(),
                detail: identified.detail,
                reachable: reachable.contains(archive_name),
                fields,
                png: None,
            },
        });
    }

    if variants.is_empty() {
        return Err(ErrorReport {
            error: format!("no copy of {name} could be read back"),
            hint: (!warnings.is_empty()).then(|| warnings.join("; ")),
        });
    }

    let differences = compute_differences(&variants);
    let differ = !differences.is_empty();

    if variants.len() == 1 {
        warnings.push(single_copy_warning(name, &variants[0].report.archive));
    }
    warnings.extend(loose_override_warning(asset_manager, name));
    warnings.extend(unreachable_warning(&variants));
    if !parse_failures.is_empty() {
        warnings.push(format!(
            "{} variant(s) failed to parse and carry the common fields only ({})",
            parse_failures.len(),
            name_list(&parse_failures)
        ));
    }

    let sanitised = sanitise_name(name);
    let dir = compare_dir(&opts.out, &sanitised);

    let mut outputs = RenderOutputs {
        // Reported even when nothing is written: it is where output would land,
        // and an empty `frames` list already says that nothing did.
        dir: dir.display().to_string(),
        sheet: None,
        frames: Vec::new(),
        // No sidecar: unlike `asset render`, every number the images illustrate
        // is already in this report's `variants`.
        index: None,
    };

    if !opts.no_render {
        warnings.extend(unrenderable_format_warning(&variants));
        render_variants(
            asset_manager,
            dict,
            art_registry,
            name,
            opts,
            &sanitised,
            &dir,
            &mut variants,
            &differences,
            differ,
            &mut outputs,
            &mut warnings,
        )?;
    }

    Ok(CompareReport {
        name: name.to_string(),
        variant_count: variants.len(),
        differ,
        differences: differences.iter().map(report_line).collect(),
        variants: variants.into_iter().map(|variant| variant.report).collect(),
        outputs,
        warnings,
    })
}

/// Every archive holding `entry_id`, in the manager's own search order.
///
/// The same sweep `verb_find` makes, minus the legacy-Westwood probe: that one
/// exists there to explain a *missing* asset, whereas comparing copies of one
/// name is a question about one entry id. Names are de-duplicated because
/// `AssetManager::archive` is keyed by name — two archives sharing a name
/// resolve to the same bytes, so a repeat would be a phantom variant.
fn sweep_holders(asset_manager: &AssetManager, entry_id: i32) -> Vec<String> {
    let mut holders: Vec<String> = Vec::new();
    asset_manager.visit_archives(|archive_name, archive| {
        if archive.get_by_id(entry_id).is_none() {
            return;
        }
        if holders.iter().any(|held| held == archive_name) {
            return;
        }
        holders.push(archive_name.to_string());
    });
    holders
}

/// The queryable fields for one variant, plus whether its parser rejected it.
///
/// Field names are `verb_scan::build_fields`' names, deliberately: the two verbs
/// must not disagree about what a field is called, or `asset scan --where w=12`
/// and a compare line reading `w: ...` would be talking about different things.
/// `archive` and `name` are the two common fields left out — they are constant
/// per variant and would diff on every single row.
///
/// `checksum` is the one field with no `verb_scan` counterpart. Two copies can
/// share a format, dimensions and byte length and still be different art — the
/// theater copies of one sprite do exactly that — and structural equality alone
/// would report those as identical, which is a wrong answer, not a terse one.
fn variant_fields(format: &str, data: &[u8], size: u32) -> (BTreeMap<String, String>, bool) {
    let mut map: BTreeMap<String, String> = BTreeMap::new();
    map.insert("format".to_string(), format.to_string());
    map.insert("size".to_string(), size.to_string());
    map.insert(
        "checksum".to_string(),
        format!("{:016x}", content_checksum(data)),
    );

    // `None` means the reader rejected the bytes; an empty Vec means the format
    // has no structure worth comparing. The two are different facts.
    let extra: Option<Vec<(&'static str, String)>> = match format {
        "shp" => ShpFile::from_bytes(data).ok().map(|shp| {
            vec![
                ("w", shp.width.to_string()),
                ("h", shp.height.to_string()),
                ("frames", shp.frames.len().to_string()),
            ]
        }),
        "tmp" => TmpFile::from_bytes(data).ok().map(|tmp| {
            let present = tmp.tiles.iter().filter(|slot| slot.is_some()).count();
            vec![
                ("tw", tmp.template_width.to_string()),
                ("th", tmp.template_height.to_string()),
                ("tiles", present.to_string()),
            ]
        }),
        "vxl" => VxlFile::from_bytes(data).ok().map(|vxl| {
            let voxels: usize = vxl.limbs.iter().map(|limb| limb.voxels.len()).sum();
            vec![
                ("limbs", vxl.limbs.len().to_string()),
                ("voxels", voxels.to_string()),
            ]
        }),
        "hva" => HvaFile::from_bytes(data).ok().map(|hva| {
            vec![
                ("frames", hva.frame_count.to_string()),
                ("sections", hva.section_count.to_string()),
            ]
        }),
        "csf" => CsfFile::from_bytes(data)
            .ok()
            .map(|csf| vec![("entries", csf.len().to_string())]),
        "pcx" => PcxFile::from_bytes(data)
            .ok()
            .map(|pcx| vec![("w", pcx.width.to_string()), ("h", pcx.height.to_string())]),
        "vpl" => VplFile::from_bytes(data)
            .ok()
            .map(|vpl| vec![("sections", vpl.num_sections.to_string())]),
        // Counted over the source bytes, as in `verb_scan`: the reader decodes
        // components as `raw << 2`, which wraps above 63 and merges entries.
        "pal" => Some(vec![("unique", unique_palette_colors(data).to_string())]),
        "fnt" => FntFile::from_bytes(data)
            .ok()
            .map(|fnt| vec![("cellh", fnt.cell_height.to_string())]),
        "aud" => aud_file::parse_header(data)
            .map(|header| vec![("rate", header.sample_rate.to_string())]),
        _ => Some(Vec::new()),
    };

    match extra {
        Some(pairs) => {
            for (key, value) in pairs {
                map.insert(key.to_string(), value);
            }
            (map, false)
        }
        None => (map, true),
    }
}

/// Distinct RGB triplets in a palette's source bytes. Mirrors `verb_scan`.
fn unique_palette_colors(data: &[u8]) -> usize {
    let mut colors: Vec<[u8; 3]> = data
        .as_chunks::<3>()
        .0
        .iter()
        .take(PAL_ENTRY_COUNT)
        .map(|rgb| [rgb[0], rgb[1], rgb[2]])
        .collect();
    colors.sort_unstable();
    colors.dedup();
    colors.len()
}

/// FNV-1a over the whole entry. See [`variant_fields`] for why it exists.
fn content_checksum(data: &[u8]) -> u64 {
    use crate::util::fnv::{FNV1A64_OFFSET_BASIS, fnv1a64_fold_bytes};
    fnv1a64_fold_bytes(FNV1A64_OFFSET_BASIS, data)
}

/// Every field whose value is not identical across all variants.
///
/// The union of field names is walked, not the first variant's: when the copies
/// sniff to different formats their field sets differ too, and a field only one
/// of them exposes is itself the difference worth reporting.
fn compute_differences(variants: &[Variant<'_>]) -> Vec<Difference> {
    let mut keys: BTreeSet<&str> = BTreeSet::new();
    for variant in variants {
        keys.extend(variant.report.fields.keys().map(String::as_str));
    }

    let mut differences: Vec<Difference> = Vec::new();
    for key in keys {
        let values: Vec<(String, String)> = variants
            .iter()
            .map(|variant| {
                (
                    variant.report.archive.clone(),
                    variant
                        .report
                        .fields
                        .get(key)
                        .cloned()
                        .unwrap_or_else(|| ABSENT_VALUE.to_string()),
                )
            })
            .collect();

        let first = values.first().map(|(_, value)| value.as_str());
        if values
            .iter()
            .all(|(_, value)| Some(value.as_str()) == first)
        {
            continue;
        }
        differences.push(Difference {
            field: key.to_string(),
            values,
        });
    }
    differences
}

/// `w: ra2.mix -> sidec01.mix=12, ra2.mix -> sidec02.mix=16`.
fn report_line(difference: &Difference) -> String {
    let parts: Vec<String> = difference
        .values
        .iter()
        .map(|(archive, value)| format!("{archive}={value}"))
        .collect();
    format!("{}: {}", difference.field, parts.join(", "))
}

/// The same difference, narrowed to archive leaves and the drawable glyph set.
fn sheet_line(difference: &Difference) -> String {
    let parts: Vec<String> = difference
        .values
        .iter()
        .map(|(archive, value)| format!("{} {value}", archive_leaf(archive)))
        .collect();
    let line = format!("{}: {}", difference.field, parts.join("   "));
    let truncated: String = line.chars().take(MAX_HEADER_LINE_CHARS).collect();
    label_text(&truncated)
}

/// The last link of an archive chain: `ra2.mix -> sidec01.mix` is `sidec01.mix`.
fn archive_leaf(archive: &str) -> &str {
    let leaf = archive.rsplit("->").next().unwrap_or(archive).trim();
    leaf.strip_prefix("nested:").unwrap_or(leaf).trim()
}

/// True for characters the shared 5x7 table actually draws.
///
/// Everything else advances the pen and renders blank, so a label spelled with
/// `#`, `,`, `.` or `(` silently comes out full of gaps. Kept in sync with
/// `render::bit_font::fallback_5x7_glyphs`, whose table is space, `-`, `:`, `/`,
/// the digits and both letter cases.
fn is_drawable_glyph(character: char) -> bool {
    matches!(character, ' ' | '-' | ':' | '/')
        || character.is_ascii_digit()
        || character.is_ascii_alphabetic()
}

/// Rewrite arbitrary text into that drawable subset.
///
/// Undrawable characters become `-` rather than being dropped: a filename like
/// `POWERP.SHP` reads as `POWERP-SHP`, which is recognisable, whereas deleting
/// the separator would run two components together.
fn label_text(text: &str) -> String {
    text.chars()
        .map(|character| {
            if is_drawable_glyph(character) {
                character
            } else {
                '-'
            }
        })
        .collect()
}

/// `sidec01-mix 12x2` — the archive leaf and the variant's canvas dimensions.
fn cell_label(archive: &str, w: u32, h: u32) -> String {
    format!("{} {w}x{h}", label_text(archive_leaf(archive)))
}

/// Header lines above the sheet. An agent that only looks at the image must
/// still learn which asset this is, how many copies exist, what the scale is,
/// and — the whole point of the verb — whether they differ.
fn sheet_header(
    name: &str,
    variant_count: usize,
    drawn: usize,
    frame: usize,
    scale: u32,
    differ: bool,
    differences: &[Difference],
) -> Vec<String> {
    let verdict = if differ {
        "VARIANTS DIFFER"
    } else {
        "ALL VARIANTS IDENTICAL"
    };
    let mut header = vec![
        format!("compare {}", label_text(name)),
        format!("{variant_count} variants   {drawn} drawn   frame {frame}   scale {scale}x"),
        verdict.to_string(),
    ];
    for difference in differences.iter().take(MAX_HEADER_DIFFERENCES) {
        header.push(sheet_line(difference));
    }
    if differences.len() > MAX_HEADER_DIFFERENCES {
        header.push(format!(
            "and {} more differing fields in the report",
            differences.len() - MAX_HEADER_DIFFERENCES
        ));
    }
    header.push("cyan outline is the canvas bounds   checkerboard is transparent".to_string());
    header
}

/// Draw one frame of every SHP variant into a single labelled sheet.
///
/// Non-SHP variants keep `png: None` — their structural diff is already in the
/// report, and inventing a render for a format this verb cannot draw would be
/// worse than saying so.
fn render_variants(
    asset_manager: &AssetManager,
    dict: &NameDict,
    art_registry: &ArtRegistry,
    name: &str,
    opts: &CompareOptions,
    sanitised: &str,
    dir: &Path,
    variants: &mut [Variant<'_>],
    differences: &[Difference],
    differ: bool,
    outputs: &mut RenderOutputs,
    warnings: &mut Vec<String>,
) -> Result<(), ErrorReport> {
    // Re-parsed here rather than threaded out of `variant_fields`: there are at
    // most a few dozen variants, and keeping the field builder format-agnostic
    // is worth the second pass.
    let sprites: Vec<(usize, ShpFile)> = variants
        .iter()
        .enumerate()
        .filter(|(_, variant)| variant.report.format == FORMAT_SHP)
        .filter_map(|(index, variant)| {
            ShpFile::from_bytes(variant.bytes)
                .ok()
                .map(|shp| (index, shp))
        })
        .collect();
    if sprites.is_empty() {
        return Ok(());
    }

    // One scale for every cell, chosen from the largest canvas: per-variant
    // scaling would make the sheet lie about exactly the relative sizes it
    // exists to show.
    let widest = sprites
        .iter()
        .map(|(_, shp)| u32::from(shp.width).max(1))
        .max()
        .unwrap_or(1);
    let tallest = sprites
        .iter()
        .map(|(_, shp)| u32::from(shp.height).max(1))
        .max()
        .unwrap_or(1);
    let requested = clamp_scale(
        opts.scale
            .unwrap_or_else(|| canvas::choose_scale(widest, tallest)),
    );
    let Some(scale) = fit_scale(widest, tallest, requested) else {
        warnings.push(format!(
            "the largest variant declares a {widest}x{tallest} canvas, past the \
             {MAX_OUTPUT_PIXELS}-pixel render budget; the structural diff is reported without images"
        ));
        return Ok(());
    };
    if scale < requested {
        warnings.push(format!(
            "scale reduced from {requested} to {scale} to stay inside the render budget"
        ));
    }

    std::fs::create_dir_all(dir).map_err(|err| ErrorReport {
        error: format!("could not create {}: {err}", dir.display()),
        hint: Some("pass a writable `--out` root".to_string()),
    })?;
    // A re-run must replace the previous comparison, not blend with it: a
    // two-variant run after a five-variant one would otherwise leave three
    // stale PNGs that read as current output.
    clear_stale_outputs(dir, sanitised);

    let mut cells: Vec<SheetCell> = Vec::with_capacity(sprites.len());

    for (index, shp) in &sprites {
        let archive = variants[*index].report.archive.clone();

        // Each copy may legitimately want a different palette — a sidec01 copy
        // and a sidec02 copy do — so inference runs per variant, against that
        // variant's own source archive.
        let inference = palette::infer(
            asset_manager,
            dict,
            art_registry,
            Some(name),
            &archive,
            None,
        );
        let Some(load) = inference.chosen else {
            warnings.push(format!(
                "no palette resolved for the copy in \"{archive}\", so it was not drawn"
            ));
            continue;
        };

        if opts.frame >= shp.frames.len() {
            warnings.push(format!(
                "the copy in \"{archive}\" has {} frame(s), so --frame {} is out of range and it \
                 was not drawn",
                shp.frames.len(),
                opts.frame
            ));
            continue;
        }

        let converted = shp.frame_to_rgba(opts.frame, &load.palette);
        let frame = &shp.frames[opts.frame];
        let frame_image = match converted {
            Ok(rgba) if frame.frame_width > 0 && frame.frame_height > 0 => {
                let decoded = Rgba::from_raw(
                    rgba,
                    u32::from(frame.frame_width),
                    u32::from(frame.frame_height),
                );
                if decoded.is_none() {
                    warnings.push(format!(
                        "frame {} of the copy in \"{archive}\" decoded short and was drawn empty",
                        opts.frame
                    ));
                }
                decoded
            }
            // A zero-sized frame is legal in retail art (blank animation slots);
            // the cell is still drawn so the canvas size stays comparable.
            Ok(_) => None,
            Err(_) => {
                warnings.push(format!(
                    "frame {} of the copy in \"{archive}\" could not be converted and was drawn empty",
                    opts.frame
                ));
                None
            }
        };

        let canvas_w = u32::from(shp.width);
        let canvas_h = u32::from(shp.height);
        let composed = compose_variant_image(
            canvas_w,
            canvas_h,
            frame.frame_x,
            frame.frame_y,
            frame_image.as_ref(),
        );
        let upscaled = canvas::upscale_nearest(&composed, scale);

        let png_name = variant_png_name(sanitised, *index, &archive);
        let png_path = dir.join(&png_name);
        canvas::save_png(&png_path, &upscaled).map_err(|err| ErrorReport {
            error: format!("could not write {}: {err}", png_path.display()),
            hint: Some("pass a writable `--out` root".to_string()),
        })?;

        cells.push(SheetCell {
            image: upscaled,
            label: cell_label(&archive, canvas_w, canvas_h),
        });
        outputs.frames.push(png_path.display().to_string());
        variants[*index].report.png = Some(png_path.display().to_string());
    }

    if cells.is_empty() {
        return Ok(());
    }

    // Built even for a single cell, unlike `asset render`: here the sheet is
    // where the verdict is written, so it is the deliverable rather than a
    // convenience over many frames.
    let header = sheet_header(
        name,
        variants.len(),
        cells.len(),
        opts.frame,
        scale,
        differ,
        differences,
    );
    let sheet = canvas::build_contact_sheet(&header, &cells);
    let sheet_path = dir.join(sheet_png_name(sanitised));
    canvas::save_png(&sheet_path, &sheet).map_err(|err| ErrorReport {
        error: format!("could not write {}: {err}", sheet_path.display()),
        hint: Some("pass a writable `--out` root".to_string()),
    })?;
    outputs.sheet = Some(sheet_path.display().to_string());

    Ok(())
}

/// Composite one frame into its file canvas, outlined.
///
/// Full canvas, never the bare sub-rect: two copies of a name can share frame
/// dimensions and differ only in where the frame sits, and a cropped cell would
/// hide exactly that.
fn compose_variant_image(
    canvas_w: u32,
    canvas_h: u32,
    frame_x: u16,
    frame_y: u16,
    frame_image: Option<&Rgba>,
) -> Rgba {
    let draw_w = canvas_w.max(1);
    let draw_h = canvas_h.max(1);
    // Checkerboard, so a transparent pixel is distinguishable from a black one.
    let mut image = Rgba::checkerboard(draw_w, draw_h);
    if let Some(source) = frame_image {
        canvas::blit_over(&mut image, source, i64::from(frame_x), i64::from(frame_y));
    }
    canvas::draw_rect_outline(&mut image, 0, 0, draw_w, draw_h, CANVAS_OUTLINE_COLOR);
    image
}

/// Warning for the one-copy case. Not an error: "there is only one" answers the
/// question that was asked, and an error would throw the answer away.
fn single_copy_warning(name: &str, archive: &str) -> String {
    format!(
        "only one copy of {name} exists, in \"{archive}\" — there is nothing to compare it against"
    )
}

/// Note a loose file on disk that shadows every archive copy.
///
/// Loose files are not variants here — this verb compares archives — but one
/// that overrides them all is what the game would actually load, so reporting
/// the archive diff without mentioning it would answer the wrong question.
fn loose_override_warning(asset_manager: &AssetManager, name: &str) -> Option<String> {
    let resolved = asset_manager.resolve_ref(name)?;
    let source = resolved.source_archive.to_string();
    source.starts_with(LOOSE_PREFIX).then(|| {
        format!(
            "a loose file on disk (\"{source}\") shadows every archive copy — it is what the engine \
             loads, and it is not one of the variants below"
        )
    })
}

/// Note variants only the catalogue sweep can see.
fn unreachable_warning(variants: &[Variant<'_>]) -> Option<String> {
    let unreachable: Vec<String> = variants
        .iter()
        .filter(|variant| !variant.report.reachable)
        .map(|variant| variant.report.archive.clone())
        .collect();
    (!unreachable.is_empty()).then(|| {
        format!(
            "{} variant(s) live in catalogued archives outside the name-lookup index — those bytes \
             exist but the engine's own lookup would not reach them ({})",
            unreachable.len(),
            name_list(&unreachable)
        )
    })
}

/// Note the formats this verb can diff but not draw.
fn unrenderable_format_warning(variants: &[Variant<'_>]) -> Option<String> {
    let mut formats: Vec<String> = variants
        .iter()
        .filter(|variant| variant.report.format != FORMAT_SHP)
        .map(|variant| variant.report.format.clone())
        .collect();
    formats.sort_unstable();
    formats.dedup();
    (!formats.is_empty()).then(|| {
        format!(
            "`asset compare` draws SHP only, so the {} variant(s) carry the structural diff without \
             an image",
            formats.join(", ")
        )
    })
}

/// Join up to [`MAX_NAMED_ARCHIVES`] names, eliding the rest. Mirrors the
/// aggregate-warning helper in `verb_scan`, which is module-private there.
fn name_list(names: &[String]) -> String {
    if names.len() <= MAX_NAMED_ARCHIVES {
        return names.join(", ");
    }
    format!(
        "{}, and {} more",
        names[..MAX_NAMED_ARCHIVES].join(", "),
        names.len() - MAX_NAMED_ARCHIVES
    )
}

/// Replace every character outside `[A-Za-z0-9._-]` so the name is safe as a
/// directory and filename component on any platform.
///
/// Mirrors `verb_render::sanitise_name` character for character — copied rather
/// than called because that one is module-private, and pinned to the same
/// fixtures by the test below. Flattening the path separators is also what keeps
/// a name like `../../x` from escaping the output root.
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

/// `<out>/compare/<sanitised>/`, a sibling of `<out>/render/<sanitised>/` and
/// absolutised the same way, so the reported paths do not depend on the
/// reader's working directory.
fn compare_dir(out: &Path, sanitised: &str) -> PathBuf {
    let root = std::path::absolute(out).unwrap_or_else(|_| out.to_path_buf());
    root.join(COMPARE_SUBDIR).join(sanitised)
}

/// `<sanitised>.v<NN>.<archive-leaf>.png`.
///
/// The index keeps the files sorted in sweep order and disambiguates two chains
/// that end in the same leaf; the leaf is what makes the filename readable
/// without going back to the report.
fn variant_png_name(sanitised: &str, index: usize, archive: &str) -> String {
    format!(
        "{sanitised}.v{index:02}.{}.png",
        sanitise_name(archive_leaf(archive))
    )
}

fn sheet_png_name(sanitised: &str) -> String {
    format!("{sanitised}.{SHEET_STEM}.png")
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
    if stem == SHEET_STEM {
        return true;
    }
    let Some(rest) = stem.strip_prefix('v') else {
        return false;
    };
    // Counted in bytes, and every counted byte is an ASCII digit, so the split
    // below always lands on a character boundary.
    let digits = rest.bytes().take_while(u8::is_ascii_digit).count();
    // `v00` alone is not one of ours — every variant PNG carries a leaf too.
    digits > 0 && rest.len() > digits + 1 && rest[digits..].starts_with('.')
}

/// Remove the previous run's PNGs. Failures are ignored: a stale file is a
/// nuisance, a hard error here would block an otherwise good comparison.
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

    /// Build a variant carrying only what the diff reads. `bytes` is empty
    /// because nothing below this line renders.
    fn variant(archive: &str, fields: &[(&str, &str)]) -> Variant<'static> {
        Variant {
            bytes: &[],
            report: CompareVariant {
                archive: archive.to_string(),
                entry_id: "0x00000000".to_string(),
                size: 0,
                format: FORMAT_SHP.to_string(),
                detail: String::new(),
                reachable: true,
                fields: fields
                    .iter()
                    .map(|(key, value)| (key.to_string(), value.to_string()))
                    .collect(),
                png: None,
            },
        }
    }

    const SIDEC01: &str = "ra2.mix -> sidec01.mix";
    const SIDEC02: &str = "ra2.mix -> sidec02.mix";

    #[test]
    fn identical_variants_produce_no_differences() {
        let variants = [
            variant(SIDEC01, &[("w", "12"), ("h", "2"), ("size", "96")]),
            variant(SIDEC02, &[("w", "12"), ("h", "2"), ("size", "96")]),
        ];
        assert!(compute_differences(&variants).is_empty());
    }

    #[test]
    fn one_varying_field_yields_one_line_naming_every_value() {
        // The POWERP.SHP split that motivated the verb: same name, same frame
        // count, different sidebar width.
        let variants = [
            variant(SIDEC01, &[("w", "12"), ("h", "2"), ("frames", "2")]),
            variant(SIDEC02, &[("w", "16"), ("h", "2"), ("frames", "2")]),
        ];
        let differences = compute_differences(&variants);
        assert_eq!(differences.len(), 1);
        assert_eq!(differences[0].field, "w");

        let line = report_line(&differences[0]);
        assert!(line.starts_with("w: "), "{line}");
        assert!(line.contains(&format!("{SIDEC01}=12")), "{line}");
        assert!(line.contains(&format!("{SIDEC02}=16")), "{line}");
    }

    #[test]
    fn differences_are_ordered_by_field_name() {
        let variants = [
            variant(SIDEC01, &[("w", "12"), ("h", "2"), ("frames", "2")]),
            variant(SIDEC02, &[("w", "16"), ("h", "4"), ("frames", "3")]),
        ];
        let fields: Vec<String> = compute_differences(&variants)
            .into_iter()
            .map(|difference| difference.field)
            .collect();
        assert_eq!(fields, ["frames", "h", "w"]);
    }

    #[test]
    fn a_field_only_one_variant_exposes_is_itself_a_difference() {
        // What a name that is SHP in one archive and PCX in another looks like.
        let variants = [
            variant(SIDEC01, &[("format", "shp"), ("frames", "2")]),
            variant(SIDEC02, &[("format", "pcx")]),
        ];
        let differences = compute_differences(&variants);
        let frames = differences
            .iter()
            .find(|difference| difference.field == "frames")
            .expect("the frames field differs");
        assert!(report_line(frames).contains(ABSENT_VALUE));
    }

    #[test]
    fn differing_bytes_at_an_identical_structure_still_differ() {
        // Two theater copies with the same geometry and the same length.
        let variants = [
            variant(
                SIDEC01,
                &[
                    ("w", "60"),
                    ("h", "48"),
                    ("size", "2048"),
                    ("checksum", "aa"),
                ],
            ),
            variant(
                SIDEC02,
                &[
                    ("w", "60"),
                    ("h", "48"),
                    ("size", "2048"),
                    ("checksum", "bb"),
                ],
            ),
        ];
        let differences = compute_differences(&variants);
        assert_eq!(differences.len(), 1);
        assert_eq!(differences[0].field, "checksum");
    }

    #[test]
    fn a_lone_variant_never_differs_and_says_so() {
        let variants = [variant(SIDEC01, &[("w", "12"), ("h", "2")])];
        assert!(compute_differences(&variants).is_empty());

        let warning = single_copy_warning("POWERP.SHP", SIDEC01);
        assert!(warning.contains("only one copy"), "{warning}");
        assert!(warning.contains("POWERP.SHP"), "{warning}");
        assert!(warning.contains(SIDEC01), "{warning}");
    }

    #[test]
    fn archive_leaf_takes_the_last_link_of_the_chain() {
        assert_eq!(archive_leaf(SIDEC01), "sidec01.mix");
        assert_eq!(archive_leaf("ra2md.mix -> cachemd.mix"), "cachemd.mix");
        assert_eq!(archive_leaf("ra2.mix"), "ra2.mix");
        assert_eq!(archive_leaf("nested: expandmd01.mix"), "expandmd01.mix");
    }

    #[test]
    fn every_drawn_label_stays_inside_the_glyph_table() {
        // Undrawable characters advance the pen without inking, so a label that
        // slipped one through would silently render with holes in it.
        let drawn = [
            cell_label(SIDEC01, 12, 2),
            label_text("POWERP.SHP"),
            sheet_line(&Difference {
                field: "w".to_string(),
                values: vec![
                    (SIDEC01.to_string(), "12".to_string()),
                    (SIDEC02.to_string(), "16".to_string()),
                ],
            }),
        ];
        for label in &drawn {
            assert!(
                label.chars().all(is_drawable_glyph),
                "undrawable glyph in {label:?}"
            );
        }

        assert_eq!(cell_label(SIDEC01, 12, 2), "sidec01-mix 12x2");
        assert_eq!(label_text("POWERP.SHP"), "POWERP-SHP");
    }

    #[test]
    fn the_sheet_header_carries_name_count_scale_and_verdict() {
        let differences = vec![Difference {
            field: "w".to_string(),
            values: vec![
                (SIDEC01.to_string(), "12".to_string()),
                (SIDEC02.to_string(), "16".to_string()),
            ],
        }];
        let header = sheet_header("POWERP.SHP", 2, 2, 0, 8, true, &differences);
        let text = header.join("\n");
        assert!(text.contains("POWERP-SHP"), "{text}");
        assert!(text.contains("2 variants"), "{text}");
        assert!(text.contains("scale 8x"), "{text}");
        assert!(text.contains("VARIANTS DIFFER"), "{text}");
        for line in &header {
            assert!(
                line.chars().all(is_drawable_glyph),
                "undrawable glyph in {line:?}"
            );
        }

        let same = sheet_header("POWERP.SHP", 2, 2, 0, 8, false, &[]);
        assert!(same.join("\n").contains("ALL VARIANTS IDENTICAL"));
    }

    #[test]
    fn the_sheet_header_elides_a_long_difference_list() {
        let differences: Vec<Difference> = ["a", "b", "c", "d", "e"]
            .iter()
            .map(|field| Difference {
                field: (*field).to_string(),
                values: vec![
                    (SIDEC01.to_string(), "1".to_string()),
                    (SIDEC02.to_string(), "2".to_string()),
                ],
            })
            .collect();
        let header = sheet_header("X.SHP", 2, 2, 0, 4, true, &differences);
        assert!(header.iter().any(|line| line.contains("and 2 more")));
    }

    #[test]
    fn compare_dir_is_absolute_and_sits_beside_the_render_tree() {
        let dir = compare_dir(Path::new(DEFAULT_OUT_ROOT), "POWERP.SHP");
        assert!(dir.is_absolute(), "{}", dir.display());
        assert!(
            dir.ends_with(Path::new(COMPARE_SUBDIR).join("POWERP.SHP")),
            "{}",
            dir.display()
        );
        // A sibling of `<out>/render/...`, never nested inside it: the parent of
        // the per-asset directory is the compare root itself.
        assert_eq!(
            dir.parent().and_then(Path::file_name),
            Some(std::ffi::OsStr::new(COMPARE_SUBDIR)),
            "{}",
            dir.display()
        );
    }

    #[test]
    fn a_traversing_name_flattens_to_one_component() {
        let dir = compare_dir(Path::new(DEFAULT_OUT_ROOT), &sanitise_name("../../etc/x"));
        assert!(dir.ends_with(".._.._etc_x"), "{}", dir.display());
        assert_eq!(
            dir.parent().and_then(Path::file_name),
            Some(std::ffi::OsStr::new(COMPARE_SUBDIR)),
            "{}",
            dir.display()
        );
    }

    #[test]
    fn generated_output_predicate_matches_only_our_own_files() {
        assert!(is_generated_output("gi.shp.v00.ra2.mix.png", "gi.shp"));
        assert!(is_generated_output("gi.shp.v12.conquer.mix.png", "gi.shp"));
        assert!(is_generated_output("gi.shp.compare.png", "gi.shp"));
        // Not ours: other assets, other verbs' layouts, other extensions.
        assert!(!is_generated_output("gi.shp.v00.ra2.mix.png", "e1.shp"));
        assert!(!is_generated_output("gi.shp.000.png", "gi.shp"));
        assert!(!is_generated_output("gi.shp.sheet.png", "gi.shp"));
        assert!(!is_generated_output("index.tsv", "gi.shp"));
        assert!(!is_generated_output("gi.shp.v00.png", "gi.shp"));
        assert!(!is_generated_output("gi.shp.vxx.ra2.mix.png", "gi.shp"));
    }

    #[test]
    fn common_fields_are_populated_for_every_format() {
        // An unparseable payload still carries size, format and checksum, so a
        // failed parse never leaves a variant with nothing to compare. The
        // leading 0xFFFF is what the SHP(TS) reader rejects — eight zero bytes
        // are a legal zero-frame header, not a failure.
        let bad_shp: [u8; 8] = [0xFF, 0xFF, 0, 0, 0, 0, 0, 0];
        let (fields, parse_failed) = variant_fields("shp", &bad_shp, 8);
        assert!(parse_failed);
        assert_eq!(fields.get("size").map(String::as_str), Some("8"));
        assert_eq!(fields.get("format").map(String::as_str), Some("shp"));
        assert!(fields.contains_key("checksum"));
        assert!(!fields.contains_key("w"));

        // A format with no parsed structure is not a parse failure.
        let (fields, parse_failed) = variant_fields("bik", &[0u8; 4], 4);
        assert!(!parse_failed);
        assert_eq!(fields.get("size").map(String::as_str), Some("4"));
    }

    #[test]
    fn scale_selection_stays_inside_the_budget() {
        assert_eq!(fit_scale(12, 2, 16), Some(16));
        assert_eq!(clamp_scale(0), MIN_SCALE);
        assert_eq!(clamp_scale(999), MAX_SCALE);
        // Past the per-image budget even at 1x: no images, diff still reported.
        assert_eq!(fit_scale(20_000, 20_000, 1), None);
    }

    #[test]
    fn the_unrenderable_format_warning_names_each_format_once() {
        let mut variants = [
            variant(SIDEC01, &[]),
            variant(SIDEC02, &[]),
            variant("ra2.mix", &[]),
        ];
        variants[1].report.format = "pcx".to_string();
        variants[2].report.format = "pcx".to_string();

        let warning = unrenderable_format_warning(&variants).expect("a non-SHP variant is present");
        assert_eq!(warning.matches("pcx").count(), 1, "{warning}");

        variants[1].report.format = FORMAT_SHP.to_string();
        variants[2].report.format = FORMAT_SHP.to_string();
        assert!(unrenderable_format_warning(&variants).is_none());
    }

    #[test]
    fn the_unreachable_warning_fires_only_for_catalogued_variants() {
        let mut variants = [variant(SIDEC01, &[]), variant(SIDEC02, &[])];
        assert!(unreachable_warning(&variants).is_none());

        variants[1].report.reachable = false;
        let warning = unreachable_warning(&variants).expect("one variant is unreachable");
        assert!(warning.contains(SIDEC02), "{warning}");
        assert!(!warning.contains(SIDEC01), "{warning}");
    }

    #[test]
    fn name_list_elides_past_the_cap() {
        let few: Vec<String> = (0..3).map(|n| format!("a{n}.mix")).collect();
        assert_eq!(name_list(&few), "a0.mix, a1.mix, a2.mix");

        let many: Vec<String> = (0..9).map(|n| format!("a{n}.mix")).collect();
        let listed = name_list(&many);
        assert!(listed.ends_with("and 3 more"), "{listed}");
    }
}
