//! `asset parse-check` — run every parser over the whole retail corpus.
//!
//! Owns parser dispatch for both the asset CLI and retail corpus regression
//! checks. Takes the caller's mounted [`AssetManager`] and returns a
//! [`ParseCheckReport`].
//!
//! ## `unsniffed` does not mean broken
//!
//! Every archive entry is identified first, then handed to the parser that
//! covers its format tag. Seven tags have no parser behind them — `mix`, `bik`,
//! `vqa`, `text`, `xcc`, `unknown` and `tiny` — because the crate does not read
//! nested containers, video, INI text as a binary format, the XCC filename
//! database, unrecognised blobs, or fragments too short to identify. Those
//! entries are counted in `unsniffed` / `unsniffed_bytes` and in **no** format
//! tally. They are *uncovered, not broken*: nothing failed, nothing was
//! rejected, no parser ever ran. Reading `unsniffed` as a failure count is the
//! obvious misreading of this report and it is wrong in both directions — it
//! invents failures that did not happen and hides the fact that those bytes
//! were never checked at all.
//!
//! ## `ok` is structural only
//!
//! An `ok` means `from_bytes` returned `Ok` — the bytes were readable in the
//! shape the parser expects. It is never a statement that the decoded values
//! match what gamemd does with the same file. This is repeated into `warnings`
//! because a machine-readable "100% ok" invites exactly the wrong conclusion.
//!
//! ## Corpus reach
//!
//! The walk goes through [`AssetManager::visit_archives`], which visits the
//! registered archives *and* the catalogued nested ones. Name lookup cannot
//! reach the latter, so a failure reported there will not resolve through
//! `asset find`; `asset ls <archive>` still opens it, and the failure row
//! carries the archive plus the entry ID needed to do that.
//!
//! ## Dependency rules
//! - Depends on `assets/` and sibling `asset_tools` modules only.

use std::collections::{BTreeMap, HashSet};

use crate::asset_tools::identify::identify;
use crate::asset_tools::names::NameDict;
use crate::asset_tools::report::{ErrorReport, ParseCheckFormat, ParseCheckReport, ParseFailure};
use crate::assets::asset_manager::AssetManager;
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

/// Failures sampled per format. The tally is authoritative; this bounds output.
pub const DEFAULT_FAILURE_CAP: usize = 8;

/// Format tags a parser covers, in [`run_parser`]'s dispatch order — the same
/// order used by the corpus checks. Anything `identify` can name that is absent here
/// is uncovered and lands in `unsniffed`.
pub const COVERED_FORMATS: [&str; 10] = [
    "shp", "vxl", "hva", "tmp", "pal", "csf", "vpl", "fnt", "pcx", "aud",
];

/// Longest parser error kept per sampled failure.
const MAX_ERROR_CHARS: usize = 200;

/// Emitted on every run. A caller that reads a clean report as parity evidence
/// has drawn the wrong conclusion, and the report has to say so itself.
const STRUCTURAL_ONLY_WARNING: &str = "\"ok\" means from_bytes returned Ok — structural validity only. It is never a claim that \
     the decoded values match gamemd semantics. A 100% ok run says the retail bytes are \
     readable, not that this engine reads them the way the original does.";

/// Emitted on every run. Without it, `unsniffed` reads as a failure bucket.
const UNCOVERED_WARNING: &str = "`unsniffed` counts entries no parser covers — nested MIX containers, INI/text, BIK, VQA, \
     the XCC filename database, unrecognised blobs, and fragments too short to identify. They \
     are uncovered, not broken: no parser ran and nothing was rejected.";

/// Options for one `asset parse-check` invocation.
#[derive(Debug, Clone)]
pub struct ParseCheckOptions {
    /// Restrict to one sniffed format tag.
    pub format: Option<String>,
    /// Max sampled failures reported per format.
    pub failure_cap: usize,
}

impl Default for ParseCheckOptions {
    fn default() -> Self {
        Self {
            format: None,
            failure_cap: DEFAULT_FAILURE_CAP,
        }
    }
}

/// Run the parser that covers `format` over `data`.
///
/// `None` means no parser covers the tag — the caller counts that as uncovered,
/// never as a failure. `Some(Err)` is a real rejection by a real parser.
///
/// Adding a parser means adding it here and to [`COVERED_FORMATS`], which
/// the tests pin together.
pub fn run_parser(format: &str, data: &[u8]) -> Option<Result<(), String>> {
    let outcome = match format {
        "shp" => ShpFile::from_bytes(data)
            .map(|_| ())
            .map_err(|e| e.to_string()),
        "vxl" => VxlFile::from_bytes(data)
            .map(|_| ())
            .map_err(|e| e.to_string()),
        "hva" => HvaFile::from_bytes(data)
            .map(|_| ())
            .map_err(|e| e.to_string()),
        "tmp" => TmpFile::from_bytes(data)
            .map(|_| ())
            .map_err(|e| e.to_string()),
        "pal" => Palette::from_bytes(data)
            .map(|_| ())
            .map_err(|e| e.to_string()),
        "csf" => CsfFile::from_bytes(data)
            .map(|_| ())
            .map_err(|e| e.to_string()),
        "vpl" => VplFile::from_bytes(data)
            .map(|_| ())
            .map_err(|e| e.to_string()),
        "fnt" => FntFile::from_bytes(data)
            .map(|_| ())
            .map_err(|e| e.to_string()),
        "pcx" => PcxFile::from_bytes(data)
            .map(|_| ())
            .map_err(|e| e.to_string()),
        // decode_aud is the only reader without a Result; None is its rejection.
        "aud" => match decode_aud(data) {
            Some(_) => Ok(()),
            None => Err("decode_aud returned None".to_string()),
        },
        _ => return None,
    };
    Some(outcome)
}

pub fn run(
    asset_manager: &AssetManager,
    dict: &NameDict,
    opts: &ParseCheckOptions,
) -> Result<ParseCheckReport, ErrorReport> {
    let only_format: Option<&'static str> = match &opts.format {
        Some(requested) => {
            let tag = covered_tag(requested).ok_or_else(|| unknown_format_error(requested))?;
            Some(tag)
        }
        None => None,
    };

    let reachable: HashSet<String> = asset_manager
        .registered_archive_names()
        .into_iter()
        .collect();

    let mut tallies = Tallies::new(opts.failure_cap);
    let mut scanned_archives = 0usize;
    let mut scanned_entries = 0usize;
    let mut unsniffed = 0usize;
    let mut unsniffed_bytes = 0u64;
    // Entries whose index row exists but whose payload could not be read back.
    let mut unreadable = 0usize;
    let mut catalog_only_archives = 0usize;

    asset_manager.visit_archives(|archive_name, archive| {
        scanned_archives += 1;
        if !reachable.contains(archive_name) {
            catalog_only_archives += 1;
        }

        for entry in archive.entries() {
            scanned_entries += 1;
            let Some(data) = archive.get_by_id(entry.id) else {
                unreadable += 1;
                continue;
            };

            let identified = identify(data);
            let Some(tag) = covered_tag(identified.format) else {
                // Uncovered, not broken — see the module docs.
                unsniffed += 1;
                unsniffed_bytes += data.len() as u64;
                continue;
            };
            if only_format.is_some_and(|wanted| wanted != tag) {
                continue;
            }

            // `tag` came from COVERED_FORMATS, so dispatch always matches.
            let Some(outcome) = run_parser(tag, data) else {
                continue;
            };
            match outcome {
                Ok(()) => tallies.record_ok(tag, data.len()),
                Err(error) => {
                    // A hex entry ID alone cannot be looked up by a human; the
                    // dictionary name is what makes the failure actionable.
                    let (name, _identified) = dict.resolve(entry.id);
                    tallies.record_failure(
                        tag,
                        data.len(),
                        ParseFailure {
                            archive: archive_name.to_string(),
                            entry_id: format!("{:#010X}", entry.id as u32),
                            name,
                            size: data.len(),
                            error: truncate_error(&error),
                        },
                    );
                }
            }
        }
    });

    let (formats, cap_warnings) = tallies.into_rows();

    let mut warnings = vec![
        STRUCTURAL_ONLY_WARNING.to_string(),
        UNCOVERED_WARNING.to_string(),
    ];
    if let Some(wanted) = only_format {
        warnings.push(format!(
            "--format {wanted} restricts `formats` to that tag; scanned_archives, \
             scanned_entries, unsniffed and unsniffed_bytes stay corpus-wide."
        ));
    }
    if unreadable > 0 {
        warnings.push(format!(
            "{unreadable} entries are indexed but could not be read back out of their archive; \
             they are counted in scanned_entries and tallied nowhere."
        ));
    }
    if catalog_only_archives > 0 {
        warnings.push(format!(
            "{catalog_only_archives} of the {scanned_archives} scanned archives are catalogued \
             nested archives that name lookup cannot reach; a failure reported there will not \
             resolve through `asset find`, but `asset ls <archive>` still opens it."
        ));
    }
    warnings.extend(cap_warnings);

    Ok(ParseCheckReport {
        scanned_archives,
        scanned_entries,
        unsniffed,
        unsniffed_bytes,
        formats,
        warnings,
    })
}

/// Normalise a caller-supplied or sniffer-supplied tag to a covered one.
///
/// Accepts `SHP`, `shp` and `.shp` — callers may spell format tags with
/// a leading dot, so that spelling reaches this verb by copy-paste.
fn covered_tag(tag: &str) -> Option<&'static str> {
    let normalised = tag.trim().trim_start_matches('.').to_ascii_lowercase();
    COVERED_FORMATS
        .iter()
        .find(|covered| **covered == normalised)
        .copied()
}

fn unknown_format_error(requested: &str) -> ErrorReport {
    ErrorReport {
        error: format!("no parser covers format \"{requested}\""),
        hint: Some(format!(
            "parsers exist for: {}. mix, bik, vqa, text, xcc, unknown and tiny are identified \
             but uncovered — they are counted as `unsniffed`, so filtering to one of them would \
             report nothing.",
            COVERED_FORMATS.join(", ")
        )),
    }
}

/// Keep one failure's message bounded. The tally is what carries the count;
/// a runaway nom error would otherwise dominate the JSON.
fn truncate_error(message: &str) -> String {
    if message.chars().count() <= MAX_ERROR_CHARS {
        return message.to_string();
    }
    let head: String = message.chars().take(MAX_ERROR_CHARS).collect();
    format!("{head}... (truncated)")
}

/// Running counts for one format tag.
#[derive(Default)]
struct FormatTally {
    ok: u32,
    failed: u32,
    total_bytes: u64,
    failures: Vec<ParseFailure>,
}

/// Per-format accumulator. Keyed by a `BTreeMap` so the emitted rows come out
/// sorted by format name and successive runs diff cleanly.
struct Tallies {
    failure_cap: usize,
    by_format: BTreeMap<&'static str, FormatTally>,
}

impl Tallies {
    fn new(failure_cap: usize) -> Self {
        Self {
            failure_cap,
            by_format: BTreeMap::new(),
        }
    }

    fn record_ok(&mut self, format: &'static str, size: usize) {
        let tally = self.by_format.entry(format).or_default();
        tally.ok += 1;
        tally.total_bytes += size as u64;
    }

    fn record_failure(&mut self, format: &'static str, size: usize, failure: ParseFailure) {
        let cap = self.failure_cap;
        let tally = self.by_format.entry(format).or_default();
        tally.failed += 1;
        tally.total_bytes += size as u64;
        if tally.failures.len() < cap {
            tally.failures.push(failure);
        }
    }

    /// Rows sorted by format name, plus one warning per format whose failures
    /// were sampled rather than listed in full.
    fn into_rows(self) -> (Vec<ParseCheckFormat>, Vec<String>) {
        let cap = self.failure_cap;
        let mut rows = Vec::with_capacity(self.by_format.len());
        let mut warnings = Vec::new();

        for (format, tally) in self.by_format {
            let dropped = tally.failed as usize - tally.failures.len();
            if dropped > 0 {
                warnings.push(format!(
                    "{format}: {dropped} of {} failures were not sampled (cap {cap}); the \
                     `failed` count is authoritative — raise the cap with `--limit`.",
                    tally.failed
                ));
            }
            rows.push(ParseCheckFormat {
                format: format.to_string(),
                ok: tally.ok,
                failed: tally.failed,
                total_bytes: tally.total_bytes,
                failures: tally.failures,
            });
        }

        (rows, warnings)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Tags `identify` can return that no parser covers. Kept literal rather
    /// than derived, so a parser added without updating the docs shows up here.
    const UNCOVERED_TAGS: [&str; 7] = ["mix", "bik", "vqa", "text", "xcc", "unknown", "tiny"];

    /// A 2x2 single-frame raw SHP: 8-byte file header, one 24-byte frame header
    /// pointing at four pixels appended at offset 32.
    fn tiny_shp() -> Vec<u8> {
        let mut data = vec![0u8; 8 + 24];
        data[2..4].copy_from_slice(&2u16.to_le_bytes()); // canvas width
        data[4..6].copy_from_slice(&2u16.to_le_bytes()); // canvas height
        data[6..8].copy_from_slice(&1u16.to_le_bytes()); // frame count
        data[12..14].copy_from_slice(&2u16.to_le_bytes()); // frame width
        data[14..16].copy_from_slice(&2u16.to_le_bytes()); // frame height
        data[16] = 0; // format: raw, not RLE-Zero
        data[28..32].copy_from_slice(&32u32.to_le_bytes()); // pixel data offset
        data.extend_from_slice(&[1, 2, 3, 4]);
        data
    }

    /// The same SHP with its pixel pointer aimed past the end of the file.
    fn corrupt_shp() -> Vec<u8> {
        let mut data = tiny_shp();
        data[28..32].copy_from_slice(&9999u32.to_le_bytes());
        data
    }

    fn failure(name: &str) -> ParseFailure {
        ParseFailure {
            archive: "ra2.mix -> local.mix".to_string(),
            entry_id: "0x12345678".to_string(),
            name: name.to_string(),
            size: 36,
            error: "synthetic".to_string(),
        }
    }

    #[test]
    fn shp_dispatch_separates_a_valid_file_from_a_corrupt_one() {
        assert_eq!(run_parser("shp", &tiny_shp()), Some(Ok(())));

        let outcome = run_parser("shp", &corrupt_shp()).expect("shp has a parser");
        let error = outcome.expect_err("a pointer past EOF must be rejected");
        assert!(error.contains("past end of file"), "{error}");
    }

    #[test]
    fn palette_and_vpl_dispatch_to_their_own_parsers() {
        // A .pal is exactly 768 bytes; anything else is not one.
        assert_eq!(run_parser("pal", &[0u8; 768]), Some(Ok(())));
        assert!(
            run_parser("pal", &[0u8; 767])
                .expect("pal has a parser")
                .is_err()
        );

        // A VPL needs a 16-byte header plus a 768-byte palette before its pages.
        let error = run_parser("vpl", &[0u8; 100])
            .expect("vpl has a parser")
            .expect_err("a 100-byte VPL is too small for its header");
        assert!(error.contains("too small"), "{error}");
    }

    #[test]
    fn every_covered_tag_dispatches_and_every_uncovered_one_does_not() {
        for tag in COVERED_FORMATS {
            assert!(
                run_parser(tag, &[0u8; 4]).is_some(),
                "{tag} is listed as covered but has no dispatch arm"
            );
        }
        for tag in UNCOVERED_TAGS {
            assert!(
                run_parser(tag, &[0u8; 4]).is_none(),
                "{tag} has a parser but is documented as uncovered"
            );
            assert!(
                covered_tag(tag).is_none(),
                "{tag} must not normalise onto a covered format"
            );
        }
    }

    /// The distinction the whole report hangs on: an uncovered entry is
    /// counted as unsniffed and never reaches a tally, while a covered entry
    /// the parser rejects is a failure. A corpus made only of uncovered bytes
    /// therefore reports zero formats and zero failures, not a clean run.
    #[test]
    fn uncovered_entries_are_unsniffed_while_rejected_ones_are_failures() {
        let mut tallies = Tallies::new(DEFAULT_FAILURE_CAP);
        let mut unsniffed = 0usize;
        let mut unsniffed_bytes = 0u64;

        // What the walk in `run` does, over a synthetic corpus of one entry per
        // uncovered tag plus one genuinely broken SHP.
        let corpus: Vec<(&str, Vec<u8>)> = UNCOVERED_TAGS
            .iter()
            .map(|tag| (*tag, vec![0u8; 16]))
            .chain(std::iter::once(("shp", corrupt_shp())))
            .collect();

        for (tag, data) in &corpus {
            let Some(covered) = covered_tag(tag) else {
                unsniffed += 1;
                unsniffed_bytes += data.len() as u64;
                continue;
            };
            match run_parser(covered, data).expect("a covered tag always dispatches") {
                Ok(()) => tallies.record_ok(covered, data.len()),
                Err(_) => tallies.record_failure(covered, data.len(), failure("bad.shp")),
            }
        }

        assert_eq!(unsniffed, UNCOVERED_TAGS.len());
        assert_eq!(unsniffed_bytes, 16 * UNCOVERED_TAGS.len() as u64);

        let (rows, _) = tallies.into_rows();
        assert_eq!(rows.len(), 1, "only the covered tag produces a row");
        assert_eq!(rows[0].format, "shp");
        assert_eq!(rows[0].failed, 1);
        assert_eq!(rows[0].ok, 0);
        // The uncovered bytes are nowhere in the tally — not as ok, not as failed.
        assert_eq!(rows[0].total_bytes, corrupt_shp().len() as u64);
    }

    #[test]
    fn failures_are_capped_and_the_drop_is_warned_with_its_count() {
        let mut tallies = Tallies::new(2);
        tallies.record_ok("shp", 36);
        for i in 0..5 {
            tallies.record_failure("shp", 36, failure(&format!("bad{i}.shp")));
        }

        let (rows, warnings) = tallies.into_rows();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].ok, 1);
        // The tally counts every failure even though only two were kept.
        assert_eq!(rows[0].failed, 5);
        assert_eq!(rows[0].failures.len(), 2);
        assert_eq!(rows[0].failures[0].name, "bad0.shp");
        assert_eq!(rows[0].total_bytes, 36 * 6);

        assert_eq!(warnings.len(), 1, "{warnings:?}");
        assert!(warnings[0].contains("shp"), "{}", warnings[0]);
        assert!(warnings[0].contains("3 of 5"), "{}", warnings[0]);
    }

    #[test]
    fn an_unknown_format_lists_the_tags_that_have_parsers() {
        let err = unknown_format_error("wav");
        assert!(err.error.contains("wav"), "{}", err.error);
        let hint = err.hint.expect("the error names what is available");
        for tag in COVERED_FORMATS {
            assert!(hint.contains(tag), "hint omits {tag}: {hint}");
        }
        // The uncovered tags are named too, so "why did mix not work" is answered.
        assert!(hint.contains("unsniffed"), "{hint}");
    }

    #[test]
    fn a_format_tag_is_accepted_with_a_leading_dot_or_in_upper_case() {
        assert_eq!(covered_tag(".shp"), Some("shp"));
        assert_eq!(covered_tag("SHP"), Some("shp"));
        assert_eq!(covered_tag("  pal  "), Some("pal"));
        assert_eq!(covered_tag("wav"), None);
    }

    #[test]
    fn a_long_parser_error_is_truncated_but_still_identifiable() {
        let short = "File too small for header";
        assert_eq!(truncate_error(short), short);

        let long = "x".repeat(MAX_ERROR_CHARS + 50);
        let truncated = truncate_error(&long);
        assert!(truncated.starts_with(&"x".repeat(MAX_ERROR_CHARS)));
        assert!(truncated.ends_with("(truncated)"), "{truncated}");
    }
}
