//! `asset scan` — corpus-wide search by sniffed format and field predicates.
//!
//! It answers questions like "find every SHP that is 60x48" or "every VXL with
//! more than one limb" without a throwaway test.
//!
//! Unlike the name-based verbs, this walks [`AssetManager::visit_archives`]
//! directly rather than going through `locate`. That is deliberate: name lookup
//! reaches only the registered archives, and a sweep that silently skipped the
//! catalogued nested ones would be lying about the word "every". Hits in
//! unreachable archives are reported and counted in a warning, never dropped.
//!
//! **Run this from a release build.** It is the same 8000+ entry walk the retail
//! certification suite makes, and that suite is `#[ignore]`d and release-only for
//! exactly this reason — sniffing and then parsing the whole corpus under a debug
//! build is minutes, not seconds. `--format` is what makes it interactive: it
//! turns the per-entry cost from "sniff plus parse" into "sniff, then parse only
//! the matches".
//!
//! ## Dependency rules
//! - Depends on `assets/` parsers and sibling `asset_tools` modules only.
//! - Nothing from `sim/`, `render/`, `ui/`, `audio/`, or `net/`.

use std::collections::BTreeMap;

use crate::asset_tools::identify::{self, Identified};
use crate::asset_tools::names::NameDict;
use crate::asset_tools::report::{ErrorReport, ScanReport, ScanRow};
use crate::assets::asset_manager::AssetManager;
use crate::assets::aud_file;
use crate::assets::csf_file::CsfFile;
use crate::assets::fnt_file::FntFile;
use crate::assets::hva_file::HvaFile;
use crate::assets::pcx_file::PcxFile;
use crate::assets::shp_file::ShpFile;
use crate::assets::tmp_file::TmpFile;
use crate::assets::vpl_file::VplFile;
use crate::assets::vxl_file::VxlFile;

/// Default page size. Larger than `ls`'s because a scan's whole point is
/// finding every hit, and a 200-row page of `WxH` numbers still reads in one
/// response.
pub const DEFAULT_LIMIT: usize = 200;

/// Format tag for an index entry whose bytes could not be read back. Mirrors
/// the tag `verb_ls` uses for the same condition.
const UNREADABLE_FORMAT: &str = "unreadable";

/// Entries in a .pal file: 256 RGB triplets.
const PAL_ENTRY_COUNT: usize = 256;

/// How many archive names an aggregate warning spells out before eliding.
const MAX_NAMED_ARCHIVES: usize = 6;

/// Fields every row carries, whatever the format sniffed to.
const COMMON_FIELDS: &[&str] = &["archive", "format", "name", "size"];

/// Every tag `--format` may name. This is the union of what
/// [`identify::identify`] can return, and it is validated up front: `--format
/// shpp` returning an empty result would read as "there are none", which is the
/// worst possible answer from a search verb.
const KNOWN_FORMATS: &[&str] = &[
    "aud",
    "bik",
    "csf",
    "fnt",
    "hva",
    "mix",
    "pal",
    "pcx",
    "shp",
    "text",
    "tiny",
    "tmp",
    "unknown",
    UNREADABLE_FORMAT,
    "vpl",
    "vqa",
    "vxl",
    "xcc",
];

/// Comparison operators, longest first at each starting character so `>=` is
/// never read as `>` followed by a value of `=60`.
const OPERATORS: [&str; 5] = [">=", "<=", "=", ">", "<"];

/// The operators that survive into the encoded predicate value. `=` is the
/// implicit default and carries no prefix — see [`ScanOptions::predicates`].
const ENCODED_OPERATORS: [&str; 4] = [">=", "<=", ">", "<"];

/// Options for one `asset scan` invocation.
pub struct ScanOptions {
    /// Restrict to one sniffed format tag, e.g. `shp`. Strongly recommended:
    /// without it every entry in the corpus is parsed.
    pub format: Option<String>,
    /// Case-insensitive substring filter on the archive name.
    pub archive: Option<String>,
    /// AND-only field predicates, already split into `(key, value)` pairs by
    /// [`parse_predicates`].
    ///
    /// The comparison operator rides on the *value*, not the key, so the key
    /// stays a bare field name that can be validated against the format's field
    /// list: `frames>100` arrives as `("frames", ">100")` and `w=60` as
    /// `("w", "60")`. A literal value beginning with `>`, `<`, `>=` or `<=`
    /// therefore cannot be expressed — no queryable field has one.
    pub predicates: Vec<(String, String)>,
    pub limit: usize,
    pub offset: usize,
}

impl Default for ScanOptions {
    fn default() -> Self {
        Self {
            format: None,
            archive: None,
            predicates: Vec::new(),
            limit: DEFAULT_LIMIT,
            offset: 0,
        }
    }
}

/// One compiled predicate: a validated field name, an operator, and a value.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Check {
    field: String,
    op: &'static str,
    wanted: String,
}

impl Check {
    /// Echo form for the report, so a caller can see exactly what was applied.
    fn echo(&self) -> String {
        format!("{}{}{}", self.field, self.op, self.wanted)
    }
}

/// Parse a `--where` argument such as `w=60,h=48` into predicate pairs.
///
/// Returns a descriptive `Err` for a malformed clause rather than skipping it:
/// a silently ignored predicate would report a filtered scan as an exhaustive
/// one. An entirely empty argument is not malformed — it simply carries no
/// predicates — but an empty clause inside a list (`w=60,,h=48`) is, because it
/// is always a typo.
pub fn parse_predicates(text: &str) -> Result<Vec<(String, String)>, String> {
    if text.trim().is_empty() {
        return Ok(Vec::new());
    }

    let mut parsed: Vec<(String, String)> = Vec::new();
    for clause in text.split(',') {
        let clause = clause.trim();
        if clause.is_empty() {
            return Err(format!(
                "empty predicate clause in \"{text}\" — write clauses as key=value separated by \
                 single commas, e.g. \"w=60,h=48\""
            ));
        }

        let Some((key, op, value)) = split_operator(clause) else {
            return Err(format!(
                "predicate \"{clause}\" has no comparison operator — use one of {}, e.g. \"w=60\" \
                 or \"frames>100\"",
                OPERATORS.join(" ")
            ));
        };
        let key = key.trim();
        let value = value.trim();
        if key.is_empty() {
            return Err(format!(
                "predicate \"{clause}\" has an empty field name — write it as field{op}value"
            ));
        }
        if value.is_empty() {
            return Err(format!(
                "predicate \"{clause}\" has an empty value — write it as {key}{op}value"
            ));
        }
        if value.contains('*') {
            if op != "=" {
                return Err(format!(
                    "predicate \"{clause}\" uses a wildcard with \"{op}\" — wildcards work only \
                     with \"=\", as in \"name=*icon*\""
                ));
            }
            // Only a leading and/or trailing `*` is supported. An interior one
            // would otherwise be compared as a literal character and match
            // nothing, which is the silent-empty-result failure this verb exists
            // to avoid.
            let core = value.strip_prefix('*').unwrap_or(value);
            let core = core.strip_suffix('*').unwrap_or(core);
            if core.contains('*') {
                return Err(format!(
                    "predicate \"{clause}\" has a wildcard in the middle of its value — only a \
                     leading and/or trailing \"*\" is supported (\"*icon*\", \"gi*\", \"*.shp\")"
                ));
            }
        }

        parsed.push((key.to_ascii_lowercase(), encode_operator(op, value)));
    }

    Ok(parsed)
}

/// Split a clause at its first comparison operator.
fn split_operator(clause: &str) -> Option<(&str, &'static str, &str)> {
    for (index, _) in clause.char_indices() {
        let rest = &clause[index..];
        for op in OPERATORS {
            if let Some(value) = rest.strip_prefix(op) {
                return Some((&clause[..index], op, value));
            }
        }
    }
    None
}

/// Fold the operator into the predicate value. See [`ScanOptions::predicates`].
fn encode_operator(op: &str, value: &str) -> String {
    if op == "=" {
        value.to_string()
    } else {
        format!("{op}{value}")
    }
}

/// Recover the operator from an encoded value.
fn decode_operator(value: &str) -> (&'static str, &str) {
    for op in ENCODED_OPERATORS {
        if let Some(rest) = value.strip_prefix(op) {
            return (op, rest);
        }
    }
    ("=", value)
}

/// The queryable fields a given sniffed format exposes.
///
/// This table and [`build_fields`] are two halves of one contract — a field
/// named here but never populated would be accepted by the validator and then
/// match nothing. A `debug_assert!` in `build_fields` pins them together.
fn format_fields(format: &str) -> &'static [&'static str] {
    match format {
        "shp" => &["w", "h", "frames"],
        "tmp" => &["tw", "th", "tiles"],
        "vxl" => &["limbs", "voxels"],
        "hva" => &["frames", "sections"],
        "csf" => &["entries"],
        "pcx" => &["w", "h"],
        "vpl" => &["sections"],
        // A .pal is a fixed 768-byte table, so its dimensions say nothing.
        // Distinct colour count does: it separates a real palette from a
        // padded or largely-black one, and finds near-duplicate theater tables.
        "pal" => &["unique"],
        // Cell height is the one number that distinguishes the retail fonts
        // from each other; the rest of the header derives from it.
        "fnt" => &["cellh"],
        // Sample rate is the field worth searching on — the compression byte is
        // already spelled out in every row's `detail`.
        "aud" => &["rate"],
        // MIX containers, text, video, and anything unsniffed have no parsed
        // structure. `size` and `name` still apply to them.
        _ => &[],
    }
}

/// Every field name a predicate may use, given the selected format.
///
/// With no `--format` this is the union across all formats, so a cross-format
/// query like `frames>100` is legal; rows whose format does not expose the
/// field simply never match, and `run` warns about that.
fn valid_fields(format: Option<&str>) -> Vec<&'static str> {
    let mut fields: Vec<&'static str> = COMMON_FIELDS.to_vec();
    match format {
        Some(tag) => fields.extend_from_slice(format_fields(tag)),
        None => {
            for tag in KNOWN_FORMATS {
                fields.extend_from_slice(format_fields(tag));
            }
        }
    }
    fields.sort_unstable();
    fields.dedup();
    fields
}

/// Which formats expose `field`, for the cross-format warning.
fn formats_exposing(field: &str) -> Vec<&'static str> {
    KNOWN_FORMATS
        .iter()
        .copied()
        .filter(|tag| format_fields(tag).contains(&field))
        .collect()
}

/// Validate `--format` against the tags the sniffer can actually produce.
fn compile_format(format: Option<&String>) -> Result<Option<String>, ErrorReport> {
    let Some(raw) = format else {
        return Ok(None);
    };
    let tag = raw.trim().to_ascii_lowercase();
    if !KNOWN_FORMATS.contains(&tag.as_str()) {
        return Err(ErrorReport {
            error: format!("unknown --format \"{raw}\""),
            hint: Some(format!(
                "known format tags: {}. These are sniffed from the bytes, not from the filename.",
                KNOWN_FORMATS.join(", ")
            )),
        });
    }
    Ok(Some(tag))
}

/// Validate every predicate field against the selected format.
///
/// An unknown field is an error carrying the valid list, never an empty result:
/// a typo that quietly matches nothing is indistinguishable from a real answer
/// of "none", and the two have opposite meanings.
fn compile_checks(
    format: Option<&str>,
    predicates: &[(String, String)],
) -> Result<Vec<Check>, ErrorReport> {
    let valid = valid_fields(format);
    let mut checks: Vec<Check> = Vec::with_capacity(predicates.len());

    for (key, value) in predicates {
        let field = key.trim().to_ascii_lowercase();
        if !valid.contains(&field.as_str()) {
            let scope = match format {
                Some(tag) => format!("format {tag}"),
                None => "a scan with no --format (the union across every format)".to_string(),
            };
            return Err(ErrorReport {
                error: format!("unknown --where field \"{key}\" for {scope}"),
                hint: Some(format!("valid fields for {scope}: {}", valid.join(", "))),
            });
        }
        let (op, wanted) = decode_operator(value);
        checks.push(Check {
            field,
            op,
            wanted: wanted.to_string(),
        });
    }

    Ok(checks)
}

/// The queryable fields for one entry, plus whether its parser rejected it.
struct RowFields {
    map: BTreeMap<String, String>,
    /// True when the format was known but the reader returned an error, so the
    /// row carries the common fields only.
    parse_failed: bool,
}

/// Build one row's field map.
///
/// The parse is what costs — this is where a scan without `--format` spends its
/// time, because every entry of every parseable format is read in full.
fn build_fields(format: &str, data: &[u8], archive: &str, name: &str, size: u32) -> RowFields {
    let mut map: BTreeMap<String, String> = BTreeMap::new();
    map.insert("archive".to_string(), archive.to_string());
    map.insert("format".to_string(), format.to_string());
    map.insert("name".to_string(), name.to_string());
    map.insert("size".to_string(), size.to_string());

    // `None` means the reader rejected the bytes; an empty Vec means the format
    // has no structure worth querying. The two are different facts.
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
                // The parsed limb list, not the header's `limb_count`: a file
                // where the two disagree is broken, and the list is what any
                // consumer actually iterates.
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
        // No parser call: a .pal is only ever the fixed 768-byte table, so there
        // is nothing to fail. Counted over the source bytes rather than a parsed
        // palette because the reader decodes components as `raw << 2`, which
        // wraps above 63 and would merge distinct entries.
        "pal" => Some(vec![("unique", unique_palette_colors(data).to_string())]),
        "fnt" => FntFile::from_bytes(data)
            .ok()
            .map(|fnt| vec![("cellh", fnt.cell_height.to_string())]),
        "aud" => aud_file::parse_header(data)
            .map(|header| vec![("rate", header.sample_rate.to_string())]),
        _ => Some(Vec::new()),
    };

    let mut parse_failed = false;
    match extra {
        Some(pairs) => {
            debug_assert!(
                pairs
                    .iter()
                    .map(|(key, _)| *key)
                    .eq(format_fields(format).iter().copied()),
                "build_fields and format_fields disagree for \"{format}\""
            );
            for (key, value) in pairs {
                map.insert(key.to_string(), value);
            }
        }
        None => parse_failed = true,
    }

    RowFields { map, parse_failed }
}

/// Distinct RGB triplets in a palette's source bytes.
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

/// AND every predicate against one row.
fn row_matches(fields: &BTreeMap<String, String>, checks: &[Check]) -> bool {
    checks.iter().all(|check| match fields.get(&check.field) {
        Some(actual) => compare(actual, check.op, &check.wanted),
        // A field this row's format does not expose cannot be satisfied. Only
        // reachable with no `--format`, where the valid set is the union.
        None => false,
    })
}

/// Compare one field value against a predicate value.
///
/// Numeric when *both* sides parse as integers, string otherwise — so `size>9`
/// orders by magnitude while `name>a` orders lexicographically. String
/// comparison is case-insensitive, and `=` additionally honours a leading
/// and/or trailing `*` as a wildcard.
fn compare(actual: &str, op: &str, wanted: &str) -> bool {
    if let (Ok(left), Ok(right)) = (actual.parse::<i64>(), wanted.parse::<i64>()) {
        return match op {
            "=" => left == right,
            ">=" => left >= right,
            "<=" => left <= right,
            ">" => left > right,
            "<" => left < right,
            _ => false,
        };
    }

    let left = actual.to_ascii_lowercase();
    let right = wanted.to_ascii_lowercase();
    match op {
        "=" => glob_matches(&left, &right),
        ">=" => left >= right,
        "<=" => left <= right,
        ">" => left > right,
        "<" => left < right,
        _ => false,
    }
}

/// `*icon*` contains, `*icon` ends with, `icon*` starts with, `icon` equals.
fn glob_matches(actual: &str, pattern: &str) -> bool {
    let leading = pattern.starts_with('*');
    let trailing = pattern.ends_with('*');
    // A bare `*` trims to an empty body, and every value contains "".
    let body = pattern.trim_matches('*');
    match (leading, trailing) {
        (true, true) => actual.contains(body),
        (true, false) => actual.ends_with(body),
        (false, true) => actual.starts_with(body),
        (false, false) => actual == pattern,
    }
}

/// Warn when the page hides matches. Silent truncation reads as "that is all
/// there is", which for a search verb is a wrong answer, not a terse one.
fn truncation_warning(matched: usize, shown: usize, limit: usize, offset: usize) -> Option<String> {
    if matched <= shown {
        return None;
    }
    if offset >= matched {
        return Some(format!(
            "--offset {offset} is past the end of the {matched} matching rows, so none are shown"
        ));
    }
    if shown == 0 {
        return Some(format!(
            "--limit {limit} shows none of the {matched} matching rows"
        ));
    }
    Some(format!(
        "{} of {matched} matching rows are not shown (--limit {limit}, --offset {offset}); \
         continue with --offset {}",
        matched - shown,
        offset + shown
    ))
}

/// Join up to [`MAX_NAMED_ARCHIVES`] names, eliding the rest.
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

/// Search every mounted archive.
pub fn run(
    asset_manager: &AssetManager,
    dict: &NameDict,
    opts: &ScanOptions,
) -> Result<ScanReport, ErrorReport> {
    let format_filter = compile_format(opts.format.as_ref())?;
    let checks = compile_checks(format_filter.as_deref(), &opts.predicates)?;
    let archive_filter = opts
        .archive
        .as_ref()
        .map(|substring| substring.trim().to_ascii_lowercase());

    // Registered archives are the ones normal name lookup can reach; the rest
    // are catalogued nested archives whose bytes are real but unreachable by
    // name. Scanning both is the point, reporting the difference is the honesty.
    let reachable = asset_manager.registered_archive_names();

    let mut scanned_archives = 0usize;
    let mut scanned_entries = 0usize;
    let mut matched = 0usize;
    let mut parse_failures = 0usize;
    let mut unreachable_matches = 0usize;
    let mut unreachable_archives: Vec<String> = Vec::new();
    let mut rows: Vec<ScanRow> = Vec::new();

    // Archive order then entry index, both of which are stable across runs, so
    // `--offset` pages the same corpus the same way every time.
    asset_manager.visit_archives(|archive_name, archive| {
        if let Some(needle) = &archive_filter
            && !archive_name.to_ascii_lowercase().contains(needle)
        {
            return;
        }
        scanned_archives += 1;
        let is_reachable = reachable.iter().any(|name| name == archive_name);

        for entry in archive.entries() {
            scanned_entries += 1;

            let bytes = archive.get_by_id(entry.id);
            let identified = match bytes {
                Some(data) => identify::identify(data),
                None => Identified {
                    format: UNREADABLE_FORMAT,
                    detail: "entry could not be read from the archive".to_string(),
                },
            };
            if let Some(wanted) = &format_filter
                && identified.format != wanted.as_str()
            {
                continue;
            }

            let (name, name_identified) = dict.resolve(entry.id);
            let fields = build_fields(
                identified.format,
                bytes.unwrap_or(&[]),
                archive_name,
                &name,
                entry.size,
            );
            if fields.parse_failed {
                parse_failures += 1;
            }
            if !row_matches(&fields.map, &checks) {
                continue;
            }

            matched += 1;
            if !is_reachable {
                unreachable_matches += 1;
                if !unreachable_archives.iter().any(|held| held == archive_name) {
                    unreachable_archives.push(archive_name.to_string());
                }
            }

            // `matched` is 1-based here, so this is "row index >= offset".
            // Only rows inside the page are materialised: the corpus is 8000+
            // entries and a full sweep would otherwise hold every hit at once.
            if matched > opts.offset && rows.len() < opts.limit {
                rows.push(ScanRow {
                    name,
                    identified: name_identified,
                    archive: archive_name.to_string(),
                    entry_id: format!("{:#010X}", entry.id as u32),
                    size: entry.size,
                    format: identified.format.to_string(),
                    detail: identified.detail,
                    fields: fields.map,
                });
            }
        }
    });

    // Zero archives walked means zero rows, which reads as "there are none" —
    // the same false negative the field validator exists to prevent. Say why.
    if scanned_archives == 0 {
        let mounted = asset_manager.loaded_archive_names().len();
        return Err(match &opts.archive {
            Some(substring) => ErrorReport {
                error: format!(
                    "--archive \"{substring}\" matched none of the {mounted} mounted archives, so \
                     nothing was scanned"
                ),
                hint: Some(
                    "run `asset archives` for the full list of mounted archives".to_string(),
                ),
            },
            None => ErrorReport {
                error: "no archives are mounted, so there was nothing to scan".to_string(),
                hint: Some(
                    "point the tool at the retail install with --ra2-dir <PATH>".to_string(),
                ),
            },
        });
    }

    let shown = rows.len();
    let mut warnings: Vec<String> = Vec::new();
    warnings.extend(truncation_warning(matched, shown, opts.limit, opts.offset));

    if format_filter.is_none() {
        warnings.push(format!(
            "no --format filter: all {scanned_entries} entries in {scanned_archives} archives were \
             sniffed and then parsed. Passing --format (shp, vxl, tmp, ...) parses only the \
             matches and is much cheaper."
        ));
        for check in &checks {
            if COMMON_FIELDS.contains(&check.field.as_str()) {
                continue;
            }
            warnings.push(format!(
                "\"{}\" is exposed only by {} rows; every other format can never match it. Pass \
                 --format to say so explicitly.",
                check.field,
                formats_exposing(&check.field).join("/")
            ));
        }
    }

    if parse_failures > 0 {
        warnings.push(format!(
            "{parse_failures} entries sniffed as a known format but were rejected by their reader, \
             so they expose no format fields and no format predicate can match them; run \
             `asset parse-check` for the failure list"
        ));
    }

    if unreachable_matches > 0 {
        warnings.push(format!(
            "{unreachable_matches} of the {matched} matching rows live in catalogued archives that \
             name lookup cannot reach ({}) — `asset find <name>` reports these as catalog_only, \
             and the engine's own lookup would not resolve them by name",
            name_list(&unreachable_archives)
        ));
    }

    Ok(ScanReport {
        scanned_archives,
        scanned_entries,
        matched,
        shown,
        offset: opts.offset,
        limit: opts.limit,
        predicates: checks.iter().map(Check::echo).collect(),
        name_db: dict.db(),
        rows,
        warnings,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A parseable SHP with the given canvas and frame count. Every frame
    /// header is zeroed, which the reader accepts as an empty frame — enough to
    /// exercise the field extraction without a retail asset.
    fn synthetic_shp(width: u16, height: u16, frames: u16) -> Vec<u8> {
        let mut data = vec![0u8; 8 + frames as usize * 24];
        data[2..4].copy_from_slice(&width.to_le_bytes());
        data[4..6].copy_from_slice(&height.to_le_bytes());
        data[6..8].copy_from_slice(&frames.to_le_bytes());
        data
    }

    fn pairs(parsed: &[(String, String)]) -> Vec<(&str, &str)> {
        parsed
            .iter()
            .map(|(key, value)| (key.as_str(), value.as_str()))
            .collect()
    }

    #[test]
    fn every_operator_parses_and_keeps_the_key_bare() {
        let parsed = parse_predicates("w=60,h>=48,frames<100,limbs>1,voxels<=10")
            .expect("all five operators are legal");
        assert_eq!(
            pairs(&parsed),
            vec![
                ("w", "60"),
                ("h", ">=48"),
                ("frames", "<100"),
                ("limbs", ">1"),
                ("voxels", "<=10"),
            ]
        );
    }

    #[test]
    fn two_character_operators_are_not_read_as_one() {
        // The trap: `>=` split as `>` would leave a value of "=48", which parses
        // as neither a number nor a name and would match nothing.
        let parsed = parse_predicates("h>=48").expect("legal");
        assert_eq!(decode_operator(&parsed[0].1), (">=", "48"));
        let parsed = parse_predicates("h<=48").expect("legal");
        assert_eq!(decode_operator(&parsed[0].1), ("<=", "48"));
        let parsed = parse_predicates("h>48").expect("legal");
        assert_eq!(decode_operator(&parsed[0].1), (">", "48"));
    }

    #[test]
    fn keys_are_lowercased_and_whitespace_is_trimmed() {
        let parsed = parse_predicates(" W = 60 , Frames > 8 ").expect("legal");
        assert_eq!(pairs(&parsed), vec![("w", "60"), ("frames", ">8")]);
    }

    #[test]
    fn malformed_clauses_are_errors_not_silent_drops() {
        let missing_op = parse_predicates("w60").expect_err("no operator");
        assert!(
            missing_op.contains("no comparison operator"),
            "{missing_op}"
        );

        let empty_field = parse_predicates("=60").expect_err("no field name");
        assert!(empty_field.contains("empty field name"), "{empty_field}");

        let empty_value = parse_predicates("w=").expect_err("no value");
        assert!(empty_value.contains("empty value"), "{empty_value}");

        let empty_clause = parse_predicates("w=60,,h=48").expect_err("empty clause");
        assert!(
            empty_clause.contains("empty predicate clause"),
            "{empty_clause}"
        );
    }

    #[test]
    fn wildcards_are_rejected_where_they_would_match_nothing() {
        let interior = parse_predicates("name=gi*icon.shp").expect_err("interior wildcard");
        assert!(interior.contains("middle"), "{interior}");

        let ordered = parse_predicates("name>*icon*").expect_err("wildcard with >");
        assert!(ordered.contains("only with \"=\""), "{ordered}");

        // The supported shapes still parse.
        for legal in ["name=*icon*", "name=gi*", "name=*.shp", "name=*"] {
            if let Err(err) = parse_predicates(legal) {
                panic!("{legal} should parse: {err}");
            }
        }
    }

    #[test]
    fn glob_shapes_match_the_documented_positions() {
        assert!(glob_matches("gaicon.shp", "*icon*"));
        assert!(!glob_matches("gapowr.shp", "*icon*"));
        assert!(glob_matches("gapowr.shp", "*.shp"));
        assert!(!glob_matches("gapowr.vxl", "*.shp"));
        assert!(glob_matches("gapowr.shp", "ga*"));
        assert!(!glob_matches("napowr.shp", "ga*"));
        assert!(glob_matches("anything", "*"));
        // No wildcard is plain equality, not a substring match.
        assert!(glob_matches("gapowr.shp", "gapowr.shp"));
        assert!(!glob_matches("gapowr.shp", "gapowr"));
    }

    #[test]
    fn numeric_fields_compare_by_magnitude_not_lexically() {
        // The whole point: "9" > "10" as text, and that would be wrong.
        assert!(!compare("9", ">", "10"));
        assert!(compare("9", "<", "10"));
        assert!(compare("100", ">=", "100"));
        assert!(compare("100", "<=", "100"));
        assert!(compare("60", "=", "60"));
        assert!(!compare("60", "=", "61"));
    }

    #[test]
    fn non_numeric_fields_compare_as_case_insensitive_text() {
        assert!(compare("GAPOWR.SHP", "=", "gapowr.shp"));
        assert!(compare("gapowr.shp", "=", "*POWR*"));
        assert!(compare("beta", ">", "alpha"));
        assert!(!compare("alpha", ">", "beta"));
        // A numeric field against a wildcard falls to the text path rather than
        // silently comparing as zero.
        assert!(compare("1024", "=", "10*"));
    }

    #[test]
    fn field_validation_rejects_a_typo_with_the_valid_list() {
        let predicates = vec![("ww".to_string(), "60".to_string())];
        let err = compile_checks(Some("shp"), &predicates).expect_err("ww is not an shp field");
        assert!(err.error.contains("unknown --where field"), "{}", err.error);
        let hint = err.hint.expect("the error lists what is valid");
        for field in ["w", "h", "frames", "size", "name"] {
            assert!(hint.contains(field), "hint should name {field}: {hint}");
        }
    }

    #[test]
    fn a_field_from_another_format_is_rejected_when_format_is_pinned() {
        let predicates = vec![("limbs".to_string(), ">1".to_string())];
        assert!(compile_checks(Some("shp"), &predicates).is_err());
        assert!(compile_checks(Some("vxl"), &predicates).is_ok());
        // With no --format the union is legal; `run` warns that only vxl rows
        // can ever match it.
        assert!(compile_checks(None, &predicates).is_ok());
        assert_eq!(formats_exposing("limbs"), vec!["vxl"]);
    }

    #[test]
    fn an_unknown_format_tag_is_an_error_not_an_empty_scan() {
        let err = compile_format(Some(&"shpp".to_string())).expect_err("shpp is not a tag");
        assert!(err.error.contains("unknown --format"), "{}", err.error);
        assert!(err.hint.expect("lists the tags").contains("shp"));
        // Case and padding are normalised rather than rejected.
        assert_eq!(
            compile_format(Some(&" SHP ".to_string())).expect("normalised"),
            Some("shp".to_string())
        );
    }

    #[test]
    fn shp_rows_expose_the_canvas_and_frame_count() {
        let data = synthetic_shp(60, 48, 3);
        let fields = build_fields("shp", &data, "ra2.mix", "gapowr.shp", data.len() as u32);
        assert!(!fields.parse_failed);
        assert_eq!(fields.map.get("w").map(String::as_str), Some("60"));
        assert_eq!(fields.map.get("h").map(String::as_str), Some("48"));
        assert_eq!(fields.map.get("frames").map(String::as_str), Some("3"));
        assert_eq!(
            fields.map.get("archive").map(String::as_str),
            Some("ra2.mix")
        );
        assert_eq!(
            fields.map.get("size").map(String::as_str),
            Some(data.len().to_string().as_str())
        );

        let checks = compile_checks(
            Some("shp"),
            &parse_predicates("w=60,h=48,frames>=3").expect("legal"),
        )
        .expect("valid");
        assert!(row_matches(&fields.map, &checks));
    }

    #[test]
    fn a_rejected_file_keeps_its_common_fields_and_matches_no_format_predicate() {
        // Declares 4 frame headers but carries none, so the reader errors.
        let mut data = synthetic_shp(60, 48, 4);
        data.truncate(20);
        let fields = build_fields("shp", &data, "ra2.mix", "broken.shp", data.len() as u32);
        assert!(fields.parse_failed);
        assert!(!fields.map.contains_key("w"));
        assert_eq!(
            fields.map.get("name").map(String::as_str),
            Some("broken.shp")
        );

        let checks =
            compile_checks(Some("shp"), &parse_predicates("w=60").expect("legal")).expect("valid");
        assert!(!row_matches(&fields.map, &checks));
    }

    #[test]
    fn predicates_are_anded_and_a_missing_field_never_matches() {
        let data = synthetic_shp(60, 48, 3);
        let fields = build_fields("shp", &data, "ra2.mix", "gapowr.shp", data.len() as u32);

        let both = compile_checks(
            Some("shp"),
            &parse_predicates("w=60,frames>10").expect("legal"),
        )
        .expect("valid");
        assert!(!row_matches(&fields.map, &both), "AND, not OR");

        // `limbs` is legal without --format but no shp row exposes it.
        let cross = compile_checks(None, &parse_predicates("limbs>1").expect("legal"))
            .expect("union is valid");
        assert!(!row_matches(&fields.map, &cross));

        // No predicates at all matches everything.
        assert!(row_matches(&fields.map, &[]));
    }

    #[test]
    fn a_format_without_structure_still_carries_the_common_fields() {
        let fields = build_fields("mix", b"whatever", "ra2.mix", "local.mix", 8);
        assert!(!fields.parse_failed);
        assert_eq!(fields.map.len(), COMMON_FIELDS.len());
        let checks = compile_checks(Some("mix"), &parse_predicates("size<9").expect("legal"))
            .expect("size is always valid");
        assert!(row_matches(&fields.map, &checks));
    }

    #[test]
    fn palette_rows_report_distinct_colours() {
        let mut data = vec![0u8; PAL_ENTRY_COUNT * 3];
        // Two distinct triplets: one black, one not.
        data[3..6].copy_from_slice(&[1, 2, 3]);
        let fields = build_fields("pal", &data, "ra2.mix", "anim.pal", data.len() as u32);
        assert!(!fields.parse_failed);
        assert_eq!(fields.map.get("unique").map(String::as_str), Some("2"));
    }

    #[test]
    fn truncation_is_always_announced() {
        let warning = truncation_warning(1500, 200, 200, 0).expect("1300 rows were dropped");
        assert!(warning.contains("1300"), "{warning}");
        assert!(warning.contains("--offset 200"), "{warning}");

        // Nothing hidden, nothing said.
        assert!(truncation_warning(12, 12, 200, 0).is_none());
        assert!(truncation_warning(0, 0, 200, 0).is_none());

        let past_end = truncation_warning(12, 0, 200, 50).expect("offset past the end");
        assert!(past_end.contains("past the end"), "{past_end}");

        let zero_limit = truncation_warning(12, 0, 0, 0).expect("limit 0 hides everything");
        assert!(zero_limit.contains("--limit 0"), "{zero_limit}");
    }

    #[test]
    fn archive_name_lists_elide_rather_than_run_on() {
        let few: Vec<String> = vec!["a.mix".to_string(), "b.mix".to_string()];
        assert_eq!(name_list(&few), "a.mix, b.mix");

        let many: Vec<String> = (0..10).map(|index| format!("{index}.mix")).collect();
        let listed = name_list(&many);
        assert!(listed.contains("and 4 more"), "{listed}");
    }
}
