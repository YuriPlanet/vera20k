//! `asset csf-get <KEY>` / `asset csf-grep <TEXT>` — read the retail string table.
//!
//! Every piece of UI text the game draws — unit names, tooltips, EVA lines,
//! briefings, error dialogs — comes out of a `.csf`, so this answers "what does
//! the game actually call this".
//!
//! ## Why both forms of every string are reported
//!
//! The parser normalises whitespace *while decoding*, inside
//! `assets::csf_file::decode_csf_string`, and stores only the result. So
//! `CsfFile::get` and `CsfFile::text` both hand back the normalised text;
//! `text` differs from `get` by exactly one thing — it substitutes
//! `MISSING:'<key>'` for a label that is absent — and applies no further
//! transformation. Neither can reach the bytes as the file stores them.
//!
//! That matters because retail carries hundreds of strings whose stored form
//! differs from the drawn form (briefings and tooltips pad their line breaks with
//! spaces the game never shows). A verb that reported one form silently would
//! misdescribe them: a caller diffing our text against a hex dump of the file
//! would see a mismatch and have no way to tell a normalisation from a bug. So
//! `value` is always what the game displays, and the stored text is re-derived
//! here and reported whenever the two differ — or on `--raw` for every hit.
//!
//! Values legitimately contain newlines and tabs. Nothing here strips them; JSON
//! string escaping preserves them exactly, so `\n` in the output is a real line
//! break in the retail string, not a rendering artefact.
//!
//! ## Dependency rules
//! - Depends on `assets/` (archive resolution, the CSF parser), `util/`
//!   (byte-widening for label names) and the sibling `locate` / `report`
//!   modules. Nothing from `sim/`, `render/`, `ui/`, `audio/`, or `net/`.

use std::collections::HashMap;

use crate::asset_tools::report::{CsfEntry, CsfReport, ErrorReport};
use crate::assets::asset_manager::AssetManager;
use crate::assets::csf_file::CsfFile;
use crate::util::native_string::widen_bytes;

/// Default page size for `csf-grep`. A one-word grep over the retail table
/// routinely matches hundreds of labels, and dumping them all buries the answer.
pub const DEFAULT_LIMIT: usize = 50;

/// String tables searched when the caller names none, in INI authority order:
/// active Yuri's Revenge initialises `ra2md.csf` standalone and does not merge
/// `ra2.csf` beneath it, so the md table wins and the base is only a fallback.
const CSF_CANDIDATES: [&str; 2] = ["ra2md.csf", "ra2.csf"];

/// The one table active YR actually loads. Reading anything else is legitimate
/// for investigation but is not what the running game would show.
const YR_CSF: &str = "ra2md.csf";

/// Longest key fragment quoted back in a `csf-get` miss hint.
const MAX_HINT_FRAGMENT_CHARS: usize = 24;

// --- Record constants for the raw rescan ---
//
// These mirror the private constants in `assets::csf_file`. Duplicating them is
// deliberate and is the narrowest available option: the parser normalises inside
// its decoder and exposes no accessor for the stored text, so the pre-normalised
// form has to be walked out of the bytes a second time. If the two ever drift,
// the rescan finds no records and the report says so in a warning rather than
// quietly reporting every string as unchanged.

/// Bytes of the file header before the first label record.
const CSF_HEADER_BYTES: usize = 24;
/// Label record marker, " LBL" in Westwood byte order.
const LABEL_MAGIC: u32 = 0x4C42_4C20;
/// Plain string value marker, " RTS".
const STRING_MAGIC: u32 = 0x5354_5220;
/// String value carrying a trailing extra field, "WRTS".
const STRING_EXTRA_MAGIC: u32 = 0x5354_5257;
/// Smallest label record: magic + pair count + name length.
const LABEL_RECORD_MIN_BYTES: usize = 12;

/// Ceiling on labels held by the rescan. Retail `ra2md.csf` carries a few
/// thousand; the cap exists so a corrupt file that looks like an endless run of
/// 12-byte records cannot turn into an unbounded allocation.
const MAX_LABELS_SCANNED: usize = 200_000;

/// Options shared by `csf-get` and `csf-grep`.
#[derive(Debug, Clone)]
pub struct CsfOptions {
    /// Which .csf to read. None searches `ra2md.csf` then `ra2.csf`.
    pub source: Option<String>,
    /// Page size for `csf-grep`. `csf-get` returns the single exact hit and
    /// ignores this.
    pub limit: usize,
    /// Matches skipped before the page. `csf-get` ignores this.
    pub offset: usize,
    /// Also report the pre-normalisation stored text for each hit. Without it,
    /// `raw` appears only where normalisation changed the string.
    pub raw: bool,
}

impl Default for CsfOptions {
    fn default() -> Self {
        Self {
            source: None,
            limit: DEFAULT_LIMIT,
            offset: 0,
            raw: false,
        }
    }
}

/// What the caller asked for. The two verbs differ only here, so everything from
/// table resolution to raw-text joining is shared.
#[derive(Debug, Clone, Copy)]
enum Query<'a> {
    /// One label, matched case-insensitively but in full.
    Exact(&'a str),
    /// Case-insensitive substring over keys and values. An empty needle matches
    /// every entry, which is the intended way to page the whole table.
    Substring(&'a str),
}

/// A resolved string table, before parsing.
struct LocatedTable<'a> {
    /// The filename that resolved, e.g. `ra2md.csf`.
    source: String,
    source_archive: String,
    bytes: &'a [u8],
    /// Warnings raised while resolving, carried into the report.
    warnings: Vec<String>,
}

/// One label as the file stores it, before the parser's normalisation.
struct RawLabel {
    /// The label name with its original casing. The parser upper-cases keys for
    /// lookup; retail writes them mixed-case, and the file's spelling is what a
    /// caller grepping an INI or a decompile will be looking at.
    stored_key: String,
    /// First string value, decoded but not normalised.
    value: String,
}

/// A matched page plus the totals the report needs. The two verbs converge here.
struct Matches<'a> {
    /// Echoed back as `CsfReport::query`, exactly as the caller spelled it.
    query: String,
    /// Total matches before `--offset` / `--limit`.
    matched: usize,
    /// `(upper-cased join key, displayed value)` for the rows that survived paging.
    page: Vec<(String, &'a str)>,
}

/// Result of re-walking the label records for their stored text.
struct RawScan {
    /// Keyed by the ASCII-upper-cased label, matching how `CsfFile` keys its map
    /// — the join between the two would silently miss every mixed-case label
    /// otherwise.
    by_key: HashMap<String, RawLabel>,
    /// True when [`MAX_LABELS_SCANNED`] stopped the walk early.
    truncated: bool,
}

/// `asset csf-get <KEY>` — one label, matched in full but case-insensitively.
///
/// A miss is an error rather than an empty result: it almost always means a
/// guessed spelling, and the hint points at `csf-grep`, which is the recovery.
pub fn get(
    asset_manager: &AssetManager,
    key: &str,
    opts: &CsfOptions,
) -> Result<CsfReport, ErrorReport> {
    let table = locate_table(asset_manager, opts)?;
    build_report(&table, Query::Exact(key), opts)
}

/// `asset csf-grep <TEXT>` — case-insensitive substring over keys and values.
pub fn grep(
    asset_manager: &AssetManager,
    needle: &str,
    opts: &CsfOptions,
) -> Result<CsfReport, ErrorReport> {
    let table = locate_table(asset_manager, opts)?;
    build_report(&table, Query::Substring(needle), opts)
}

/// Resolve the table to read, honouring `--source` and warning when the result
/// is not the table the running game initialises.
fn locate_table<'a>(
    asset_manager: &'a AssetManager,
    opts: &CsfOptions,
) -> Result<LocatedTable<'a>, ErrorReport> {
    if let Some(name) = &opts.source {
        let Some(resolved) = crate::asset_tools::locate::locate(asset_manager, name) else {
            return Err(ErrorReport {
                error: format!("string table not found: {name}"),
                hint: Some(format!(
                    "run `asset find {name}` — it also reports catalogued archives that name \
                     lookup cannot reach; drop --source to search {}",
                    CSF_CANDIDATES.join(" then ")
                )),
            });
        };
        return Ok(table_from(name, resolved));
    }

    for candidate in CSF_CANDIDATES {
        if let Some(resolved) = crate::asset_tools::locate::locate(asset_manager, candidate) {
            return Ok(table_from(candidate, resolved));
        }
    }

    Err(ErrorReport {
        error: format!(
            "no string table resolved; tried {}",
            CSF_CANDIDATES.join(", ")
        ),
        hint: Some(format!(
            "pass `--source <FILE.CSF>`; `asset find {YR_CSF}` reports whether any mounted \
             archive holds it, including the catalogued ones normal lookup cannot reach"
        )),
    })
}

/// Assemble a [`LocatedTable`] and its provenance warnings from one hit.
fn table_from<'a>(
    name: &str,
    resolved: crate::asset_tools::locate::Resolved<'a>,
) -> LocatedTable<'a> {
    let mut warnings: Vec<String> = Vec::new();
    warnings.extend(resolved.catalog_warning());
    if !name.eq_ignore_ascii_case(YR_CSF) {
        warnings.push(format!(
            "read from \"{name}\", not \"{YR_CSF}\" — active Yuri's Revenge initialises the md \
             table standalone and does not merge this one beneath it, so a string here is not \
             necessarily the string the game shows"
        ));
    }
    LocatedTable {
        source: name.to_string(),
        source_archive: resolved.source_archive.clone(),
        bytes: resolved.bytes,
        warnings,
    }
}

/// Parse, match, and join displayed text with stored text.
///
/// Split out from [`get`]/[`grep`] so every behaviour below is exercisable
/// against a synthetic buffer, with no mounted retail install.
fn build_report(
    table: &LocatedTable<'_>,
    query: Query<'_>,
    opts: &CsfOptions,
) -> Result<CsfReport, ErrorReport> {
    let source = &table.source;
    let csf = CsfFile::from_bytes(table.bytes).map_err(|err| ErrorReport {
        error: format!("CSF parse failed for {source}: {err}"),
        hint: Some(format!(
            "`asset info {source}` reports the header fields that were readable; pass \
             `--source <FILE.CSF>` to read a different table"
        )),
    })?;

    let scan = scan_raw_labels(table.bytes);
    let mut warnings: Vec<String> = table.warnings.clone();
    if scan.truncated {
        warnings.push(format!(
            "the raw rescan stopped at {MAX_LABELS_SCANNED} labels, so stored text is unavailable \
             beyond that point"
        ));
    }
    if scan.by_key.is_empty() && !csf.is_empty() {
        warnings.push(format!(
            "the raw rescan read no label records although the parser read {} — stored text and \
             the normalisation flag are unavailable for every entry",
            csf.len()
        ));
    }

    // Both arms produce (upper-cased key, displayed value) pairs; values are
    // borrowed from the parsed table and only the kept page is materialised.
    let Matches {
        query: query_text,
        matched,
        page,
    } = match query {
        Query::Exact(key) => {
            let Some(value) = csf.get(key) else {
                return Err(miss_error(key, source, csf.len()));
            };
            Matches {
                query: key.to_string(),
                matched: 1,
                // `CsfFile` keys its map ASCII-upper-cased, so that is the join
                // key regardless of how the caller spelled the request.
                page: vec![(key.to_ascii_uppercase(), value)],
            }
        }
        Query::Substring(needle) => {
            let lowered = needle.to_lowercase();
            let mut hits: Vec<(&str, &str)> = csf
                .entries()
                .filter(|&(key, value)| matches_needle(key, value, &lowered))
                .collect();
            // The parser's map iterates in hash order; sorting makes paging with
            // --offset mean something and keeps repeated runs comparable.
            hits.sort_by(|left, right| left.0.cmp(right.0));
            let matched = hits.len();
            Matches {
                query: needle.to_string(),
                matched,
                page: hits
                    .into_iter()
                    .skip(opts.offset)
                    .take(opts.limit)
                    .map(|(key, value)| (key.to_string(), value))
                    .collect(),
            }
        }
    };

    let mut entries: Vec<CsfEntry> = Vec::with_capacity(page.len());
    let mut missing_raw = 0usize;
    for (key, value) in page {
        let (entry, raw_missing) = make_entry(&key, value, &scan, opts.raw);
        if raw_missing {
            missing_raw += 1;
        }
        entries.push(entry);
    }

    let shown = entries.len();
    if missing_raw > 0 && !scan.by_key.is_empty() {
        warnings.push(format!(
            "{missing_raw} of {shown} reported entries had no matching record in the raw rescan; \
             `raw` and `changed_by_normalization` are unavailable for those, not known to be \
             unchanged"
        ));
    }
    if matched > shown {
        warnings.push(format!(
            "showing {shown} of {matched} matches; {} dropped by --offset {} / --limit {} — raise \
             --limit or advance --offset to see the rest",
            matched - shown,
            opts.offset,
            opts.limit
        ));
    }

    Ok(CsfReport {
        source: source.clone(),
        source_archive: table.source_archive.clone(),
        version: csf.version,
        language: csf.language,
        entry_count: csf.len(),
        query: query_text,
        matched,
        shown,
        entries,
        warnings,
    })
}

/// Case-insensitive substring test over a key and its value.
///
/// Full Unicode lowering rather than ASCII: localised retail tables carry
/// accented text in values, and ASCII-only folding would miss a caller grepping
/// for it in the casing they saw on screen.
fn matches_needle(key: &str, value: &str, lowered_needle: &str) -> bool {
    if lowered_needle.is_empty() {
        return true;
    }
    key.to_lowercase().contains(lowered_needle) || value.to_lowercase().contains(lowered_needle)
}

/// Build one report row, joining the parser's displayed value to the stored text.
///
/// Returns `true` alongside the row when the rescan had no record for this
/// label: "could not tell" and "nothing changed" are different answers, so the
/// flag stays false and the caller counts the gap for a warning instead.
fn make_entry(key_upper: &str, value: &str, scan: &RawScan, want_raw: bool) -> (CsfEntry, bool) {
    let label = scan.by_key.get(key_upper);
    let stored_key = match label {
        Some(found) => found.stored_key.clone(),
        None => key_upper.to_string(),
    };
    let changed = label.is_some_and(|found| found.value != value);
    let raw = match label {
        Some(found) if changed || want_raw => Some(found.value.clone()),
        _ => None,
    };
    (
        CsfEntry {
            key: stored_key,
            value: value.to_string(),
            raw,
            changed_by_normalization: changed,
        },
        label.is_none(),
    )
}

/// Error for a `csf-get` that matched nothing.
fn miss_error(key: &str, source: &str, entry_count: usize) -> ErrorReport {
    let fragment = grep_fragment(key);
    ErrorReport {
        error: format!("no label \"{key}\" in {source} ({entry_count} entries)"),
        hint: Some(if fragment.is_empty() {
            "`asset csf-get` needs the full label; run `asset csf-grep <TEXT>` to search keys and \
             values case-insensitively"
                .to_string()
        } else {
            format!(
                "`asset csf-get` needs the full label; run `asset csf-grep {fragment}` to search \
                 keys and values case-insensitively"
            )
        }),
    }
}

/// The most searchable piece of a missed key. Retail labels are `Family:Item`,
/// and a miss is usually a wrong family with the right item, so the tail after
/// the last colon is what recovers the lookup.
fn grep_fragment(key: &str) -> String {
    let tail = key.rsplit(':').next().unwrap_or(key).trim();
    let fragment = if tail.is_empty() { key.trim() } else { tail };
    fragment.chars().take(MAX_HINT_FRAGMENT_CHARS).collect()
}

/// Re-walk the label records for the text each label stores, un-normalised.
///
/// Bounded and total: every read is checked against the buffer, every offset is
/// checked for overflow, and the label count is capped. A malformed retail file
/// therefore yields a short scan (and a warning) rather than a panic or a
/// multi-gigabyte allocation.
fn scan_raw_labels(data: &[u8]) -> RawScan {
    let mut by_key: HashMap<String, RawLabel> = HashMap::new();
    let mut truncated = false;

    if data.len() < CSF_HEADER_BYTES {
        return RawScan { by_key, truncated };
    }

    let mut offset = CSF_HEADER_BYTES;
    loop {
        if by_key.len() >= MAX_LABELS_SCANNED {
            truncated = true;
            break;
        }
        let Some((label, next_offset)) = scan_one_label(data, offset) else {
            break;
        };
        // Later duplicates win, matching the parser's map insert: a file with the
        // same label twice must join against the same value the parser kept.
        by_key.insert(label.stored_key.to_ascii_uppercase(), label);
        // A record always consumes at least LABEL_RECORD_MIN_BYTES; the guard
        // states that invariant rather than trusting it.
        if next_offset <= offset {
            break;
        }
        offset = next_offset;
    }

    RawScan { by_key, truncated }
}

/// Read one label record. `None` ends the walk — the same "stop at the first
/// failed read or magic check" rule the parser uses, so both see the same set.
fn scan_one_label(data: &[u8], offset: usize) -> Option<(RawLabel, usize)> {
    if read_u32_at(data, offset)? != LABEL_MAGIC {
        return None;
    }
    let pair_count = read_u32_at(data, offset.checked_add(4)?)?;
    let name_len = read_u32_at(data, offset.checked_add(8)?)? as usize;
    let name_start = offset.checked_add(LABEL_RECORD_MIN_BYTES)?;
    let name_end = name_start.checked_add(name_len)?;
    let stored_key = widen_bytes(data.get(name_start..name_end)?);

    let mut pos = name_end;
    let mut value = String::new();
    for _ in 0..pair_count {
        let str_magic = read_u32_at(data, pos)?;
        let has_extra = str_magic == STRING_EXTRA_MAGIC;
        if str_magic != STRING_MAGIC && !has_extra {
            return None;
        }
        let char_count = read_u32_at(data, pos.checked_add(4)?)? as usize;
        let byte_count = char_count.checked_mul(2)?; // UTF-16-LE.
        pos = pos.checked_add(8)?;
        let encoded = data.get(pos..pos.checked_add(byte_count)?)?;
        // Only the first value is display text; later pairs are unused in retail.
        if value.is_empty() {
            value = decode_stored(encoded);
        }
        pos = pos.checked_add(byte_count)?;

        // The extra field holds an audio cue name, never display text.
        if has_extra {
            let extra_len = read_u32_at(data, pos)? as usize;
            pos = pos.checked_add(4)?.checked_add(extra_len)?;
            if pos > data.len() {
                return None;
            }
        }
    }

    Some((RawLabel { stored_key, value }, pos))
}

/// Bitwise-NOT then UTF-16-LE, with no whitespace normalisation: the string as
/// the file stores it. A trailing odd byte is dropped, matching the parser.
fn decode_stored(encoded: &[u8]) -> String {
    let units: Vec<u16> = encoded
        .as_chunks::<2>()
        .0
        .iter()
        .map(|pair| u16::from_le_bytes([!pair[0], !pair[1]]))
        .collect();
    String::from_utf16_lossy(&units)
}

/// Bounds-checked little-endian read. `util::read_helpers::read_u32_le` indexes
/// directly and would panic on a truncated record — which retail-sourced bytes
/// must never be able to do inside a tool that reports errors as values.
fn read_u32_at(data: &[u8], offset: usize) -> Option<u32> {
    let end = offset.checked_add(4)?;
    let bytes: [u8; 4] = data.get(offset..end)?.try_into().ok()?;
    Some(u32::from_le_bytes(bytes))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// File header marker, " FSC" in Westwood byte order.
    const HEADER_MAGIC: u32 = 0x4353_4620;

    /// Encode display text the way a .csf stores it: UTF-16-LE, bitwise-NOT'd.
    fn encode_value(text: &str) -> Vec<u8> {
        text.encode_utf16()
            .flat_map(u16::to_le_bytes)
            .map(|byte| !byte)
            .collect()
    }

    /// One ` LBL` record with a single ` RTS` value.
    fn label_record(label: &str, value: &str) -> Vec<u8> {
        let mut out: Vec<u8> = Vec::new();
        out.extend_from_slice(&LABEL_MAGIC.to_le_bytes());
        out.extend_from_slice(&1u32.to_le_bytes()); // one string pair
        out.extend_from_slice(&(label.len() as u32).to_le_bytes());
        out.extend_from_slice(label.as_bytes());
        out.extend_from_slice(&STRING_MAGIC.to_le_bytes());
        out.extend_from_slice(&(value.encode_utf16().count() as u32).to_le_bytes());
        out.extend_from_slice(&encode_value(value));
        out
    }

    /// A complete synthetic table. Header counts are set honestly, though the
    /// parser walks records rather than trusting them.
    fn build_csf(labels: &[(&str, &str)]) -> Vec<u8> {
        let mut out: Vec<u8> = Vec::new();
        out.extend_from_slice(&HEADER_MAGIC.to_le_bytes());
        out.extend_from_slice(&3u32.to_le_bytes()); // version
        out.extend_from_slice(&(labels.len().max(1) as u32).to_le_bytes()); // label count
        out.extend_from_slice(&(labels.len().max(1) as u32).to_le_bytes()); // string count
        out.extend_from_slice(&0u32.to_le_bytes()); // reserved
        out.extend_from_slice(&0u32.to_le_bytes()); // language
        for (label, value) in labels {
            out.extend_from_slice(&label_record(label, value));
        }
        out
    }

    fn table(bytes: &[u8]) -> LocatedTable<'_> {
        LocatedTable {
            source: "ra2md.csf".to_string(),
            source_archive: "ra2md.mix -> language.mix".to_string(),
            bytes,
            warnings: Vec::new(),
        }
    }

    fn report(bytes: &[u8], query: Query<'_>, opts: &CsfOptions) -> CsfReport {
        build_report(&table(bytes), query, opts).expect("synthetic table should build a report")
    }

    #[test]
    fn exact_get_returns_the_displayed_value_and_its_provenance() {
        let data = build_csf(&[("NAME:MTNK", "Grizzly Battle Tank")]);
        let out = report(&data, Query::Exact("NAME:MTNK"), &CsfOptions::default());

        assert_eq!(out.source, "ra2md.csf");
        assert_eq!(out.version, 3);
        assert_eq!(out.entry_count, 1);
        assert_eq!((out.matched, out.shown), (1, 1));
        assert_eq!(out.query, "NAME:MTNK");
        assert_eq!(out.entries[0].value, "Grizzly Battle Tank");
        assert!(!out.entries[0].changed_by_normalization);
        assert!(out.entries[0].raw.is_none(), "unchanged strings stay lean");
        assert!(out.warnings.is_empty(), "got {:?}", out.warnings);
    }

    #[test]
    fn get_is_case_insensitive_but_reports_the_files_own_spelling() {
        let data = build_csf(&[("Name:MTNK", "Grizzly Battle Tank")]);
        let out = report(&data, Query::Exact("name:mtnk"), &CsfOptions::default());

        // The parser keys its map upper-cased; the file writes mixed case, and
        // the file's spelling is what a caller cross-referencing a decompile or
        // an INI will be looking for.
        assert_eq!(out.entries[0].key, "Name:MTNK");
        assert_eq!(out.entries[0].value, "Grizzly Battle Tank");
    }

    #[test]
    fn get_miss_is_an_error_that_hints_at_grep_with_a_key_fragment() {
        let data = build_csf(&[("NAME:MTNK", "Grizzly Battle Tank")]);
        let err = build_report(
            &table(&data),
            Query::Exact("Name:MTNK:Long"),
            &CsfOptions::default(),
        )
        .expect_err("a missing label is an error, not an empty page");

        assert!(err.error.contains("Name:MTNK:Long"), "got {}", err.error);
        let hint = err.hint.expect("a miss must name the recovery");
        assert!(hint.contains("csf-grep Long"), "got {hint}");
    }

    #[test]
    fn get_miss_on_a_key_without_a_family_still_produces_a_usable_hint() {
        let data = build_csf(&[("NAME:MTNK", "Grizzly Battle Tank")]);
        let err = build_report(&table(&data), Query::Exact("GUI"), &CsfOptions::default())
            .expect_err("missing label");
        let hint = err.hint.expect("hint");
        assert!(hint.contains("csf-grep GUI"), "got {hint}");
    }

    #[test]
    fn grep_matches_over_both_key_and_value() {
        let data = build_csf(&[
            ("NAME:MTNK", "Grizzly Battle Tank"),
            ("NAME:HTNK", "Apocalypse Tank"),
            ("GUI:Sell", "Sell Structure"),
        ]);

        // Value-only hit: no key contains "apocalypse".
        let out = report(
            &data,
            Query::Substring("apocalypse"),
            &CsfOptions::default(),
        );
        assert_eq!(out.matched, 1);
        assert_eq!(out.entries[0].key, "NAME:HTNK");

        // Key-only hit: no value contains "gui".
        let out = report(&data, Query::Substring("GUI"), &CsfOptions::default());
        assert_eq!(out.matched, 1);
        assert_eq!(out.entries[0].key, "GUI:Sell");

        // Matches both sides, case-insensitively, and the page is key-sorted.
        let out = report(&data, Query::Substring("tank"), &CsfOptions::default());
        assert_eq!(out.matched, 2);
        let keys: Vec<&str> = out.entries.iter().map(|e| e.key.as_str()).collect();
        assert_eq!(keys, vec!["NAME:HTNK", "NAME:MTNK"]);
    }

    #[test]
    fn grep_truncation_names_the_dropped_count() {
        let data = build_csf(&[
            ("NAME:A", "Tank A"),
            ("NAME:B", "Tank B"),
            ("NAME:C", "Tank C"),
            ("NAME:D", "Tank D"),
            ("NAME:E", "Tank E"),
        ]);
        let opts = CsfOptions {
            limit: 2,
            ..CsfOptions::default()
        };
        let out = report(&data, Query::Substring("tank"), &opts);

        assert_eq!(out.matched, 5);
        assert_eq!(out.shown, 2);
        let warning = out
            .warnings
            .iter()
            .find(|w| w.contains("dropped"))
            .expect("truncation must never be silent");
        assert!(warning.contains("showing 2 of 5"), "got {warning}");
        assert!(warning.contains("3 dropped"), "got {warning}");
    }

    #[test]
    fn grep_offset_pages_through_a_stable_sorted_order() {
        let data = build_csf(&[("NAME:A", "x"), ("NAME:B", "x"), ("NAME:C", "x")]);
        let opts = CsfOptions {
            limit: 1,
            offset: 1,
            ..CsfOptions::default()
        };
        let out = report(&data, Query::Substring("name:"), &opts);

        assert_eq!(out.matched, 3);
        assert_eq!(out.shown, 1);
        assert_eq!(out.entries[0].key, "NAME:B");
    }

    #[test]
    fn an_empty_needle_pages_the_whole_table() {
        let data = build_csf(&[("A", "one"), ("B", "two")]);
        let out = report(&data, Query::Substring(""), &CsfOptions::default());
        assert_eq!(out.matched, 2);
        assert_eq!(out.shown, 2);
    }

    #[test]
    fn normalisation_is_reported_with_both_forms_of_the_string() {
        // The retail briefing/tooltip pattern: spaces padding a line break, which
        // the engine drops at load time and never draws.
        let stored = "Click to show \n advanced commands";
        let data = build_csf(&[("Tip:Advanced", stored)]);
        let out = report(&data, Query::Exact("Tip:Advanced"), &CsfOptions::default());

        let entry = &out.entries[0];
        assert!(entry.changed_by_normalization);
        assert_eq!(entry.value, "Click to show\nadvanced commands");
        assert_eq!(
            entry.raw.as_deref(),
            Some(stored),
            "the stored form must survive verbatim, newline included"
        );
    }

    #[test]
    fn raw_flag_reports_stored_text_even_where_nothing_changed() {
        let data = build_csf(&[("NAME:MTNK", "Grizzly Battle Tank")]);
        let opts = CsfOptions {
            raw: true,
            ..CsfOptions::default()
        };
        let out = report(&data, Query::Exact("NAME:MTNK"), &opts);

        assert!(!out.entries[0].changed_by_normalization);
        assert_eq!(out.entries[0].raw.as_deref(), Some("Grizzly Battle Tank"));
    }

    #[test]
    fn the_rescan_reads_wrts_records_and_skips_their_extra_field() {
        let extra: &[u8] = b"some_audio_cue";
        let value = "Tank  ready";
        let mut data: Vec<u8> = build_csf(&[]);
        data.extend_from_slice(&LABEL_MAGIC.to_le_bytes());
        data.extend_from_slice(&1u32.to_le_bytes());
        data.extend_from_slice(&("EVA:TankReady".len() as u32).to_le_bytes());
        data.extend_from_slice(b"EVA:TankReady");
        data.extend_from_slice(&STRING_EXTRA_MAGIC.to_le_bytes());
        data.extend_from_slice(&(value.encode_utf16().count() as u32).to_le_bytes());
        data.extend_from_slice(&encode_value(value));
        data.extend_from_slice(&(extra.len() as u32).to_le_bytes());
        data.extend_from_slice(extra);

        let scan = scan_raw_labels(&data);
        let label = scan
            .by_key
            .get("EVA:TANKREADY")
            .expect("WRTS records carry display text like RTS ones");
        assert_eq!(label.stored_key, "EVA:TankReady");
        assert_eq!(label.value, value, "the rescan must not normalise");

        // And the joined report still shows the collapsed double space.
        let out = report(&data, Query::Exact("EVA:TankReady"), &CsfOptions::default());
        assert_eq!(out.entries[0].value, "Tank ready");
        assert!(out.entries[0].changed_by_normalization);
    }

    #[test]
    fn a_later_duplicate_label_wins_the_join_like_the_parser_map() {
        let data = build_csf(&[("DUP", "first"), ("DUP", "second")]);
        let out = report(&data, Query::Exact("DUP"), &CsfOptions::default());
        assert_eq!(out.entries[0].value, "second");
        assert!(
            !out.entries[0].changed_by_normalization,
            "the rescan must have joined the same occurrence the parser kept"
        );
    }

    #[test]
    fn a_bad_header_reports_an_error_with_a_hint() {
        let mut data = build_csf(&[("A", "one")]);
        data[0] = 0x00;
        let err = build_report(&table(&data), Query::Exact("A"), &CsfOptions::default())
            .expect_err("a corrupt header is an error report, not a panic");
        assert!(err.error.contains("CSF parse failed"), "got {}", err.error);
        assert!(err.hint.is_some());
    }

    #[test]
    fn a_truncated_trailing_record_never_panics_the_rescan() {
        let mut data = build_csf(&[("A", "one")]);
        // A label record that claims a name far longer than the file holds.
        data.extend_from_slice(&LABEL_MAGIC.to_le_bytes());
        data.extend_from_slice(&1u32.to_le_bytes());
        data.extend_from_slice(&u32::MAX.to_le_bytes());
        data.extend_from_slice(b"AB");

        let scan = scan_raw_labels(&data);
        assert_eq!(scan.by_key.len(), 1);
        assert!(!scan.truncated);

        let out = report(&data, Query::Exact("A"), &CsfOptions::default());
        assert_eq!(out.entries[0].value, "one");
    }

    #[test]
    fn a_record_claiming_a_huge_string_is_bounded_not_allocated() {
        let mut data = build_csf(&[("A", "one")]);
        data.extend_from_slice(&LABEL_MAGIC.to_le_bytes());
        data.extend_from_slice(&1u32.to_le_bytes());
        data.extend_from_slice(&1u32.to_le_bytes());
        data.push(b'B');
        data.extend_from_slice(&STRING_MAGIC.to_le_bytes());
        data.extend_from_slice(&u32::MAX.to_le_bytes()); // 8 GB of characters

        let scan = scan_raw_labels(&data);
        assert_eq!(
            scan.by_key.len(),
            1,
            "the oversized record is skipped, not allocated"
        );
    }

    #[test]
    fn empty_and_short_buffers_scan_to_nothing() {
        assert!(scan_raw_labels(&[]).by_key.is_empty());
        assert!(scan_raw_labels(&[0x20, 0x46, 0x53, 0x43]).by_key.is_empty());
    }

    #[test]
    fn locate_warnings_survive_into_the_report() {
        let data = build_csf(&[("A", "one")]);
        let mut located = table(&data);
        located.source = "ra2.csf".to_string();
        located
            .warnings
            .push("read from \"ra2.csf\", not \"ra2md.csf\"".to_string());

        let out = build_report(&located, Query::Exact("A"), &CsfOptions::default())
            .expect("report builds");
        assert!(
            out.warnings.iter().any(|w| w.contains("ra2md.csf")),
            "got {:?}",
            out.warnings
        );
        assert_eq!(out.source, "ra2.csf");
    }

    #[test]
    fn missing_raw_records_are_reported_as_unknown_not_as_unchanged() {
        // A table whose label records the parser reads but the rescan cannot:
        // simulated by joining against an empty scan.
        let scan = RawScan {
            by_key: HashMap::from([(
                "OTHER".to_string(),
                RawLabel {
                    stored_key: "Other".to_string(),
                    value: "x".to_string(),
                },
            )]),
            truncated: false,
        };
        let (entry, missing) = make_entry("NAME:MTNK", "Grizzly", &scan, true);
        assert!(missing);
        assert!(!entry.changed_by_normalization);
        assert!(
            entry.raw.is_none(),
            "an absent record must not be reported as an empty stored string"
        );
        assert_eq!(entry.key, "NAME:MTNK");
    }

    #[test]
    fn grep_fragment_prefers_the_item_half_of_a_family_key() {
        assert_eq!(grep_fragment("NAME:MTNK"), "MTNK");
        assert_eq!(grep_fragment("GUI:Sell"), "Sell");
        assert_eq!(grep_fragment("Plain"), "Plain");
        assert_eq!(grep_fragment("Trailing:"), "Trailing:");
        assert_eq!(grep_fragment(""), "");
        assert_eq!(
            grep_fragment(&"A".repeat(MAX_HINT_FRAGMENT_CHARS + 10)).len(),
            MAX_HINT_FRAGMENT_CHARS
        );
    }
}
