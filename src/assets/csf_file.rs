//! CSF string table parser — localized text for RA2/YR UI, EVA, and menus.
//!
//! Active Yuri's Revenge initializes only `ra2md.csf` from the retail language
//! archive stack. Optional Ares builds compose its supplemental tables through
//! [`CsfFile::load_active`]. Each entry maps a label name (e.g., `"NAME:MTNK"`) to a
//! Unicode display string (e.g., `"Grizzly Battle Tank"`).
//!
//! ## Binary format
//! - 24-byte header: signature ` FSC`, version, label count, string count, reserved, language
//! - Repeating label entries: ` LBL` marker, pair count, ASCII label name, then one or more
//!   string values encoded as bitwise-NOT'd UTF-16-LE.
//!
//! ## Dependency rules
//! - Part of assets/ — no dependencies on game modules.

use std::borrow::Cow;
use std::collections::HashMap;

use crate::assets::error::AssetError;
use crate::util::read_helpers::read_u32_le;

#[cfg(feature = "ares")]
mod ares_labels;
#[cfg(feature = "ares")]
use ares_labels::{native_label_search, sort_labels as ares_sort_labels};

/// Magic bytes for the file header: " FSC".
const HEADER_MAGIC: u32 = 0x4353_4620;
/// Magic bytes for a label entry: " LBL" in Westwood byte order.
const LABEL_MAGIC: u32 = 0x4C42_4C20;
/// Magic bytes for a regular string value: " RTS" in Westwood byte order.
const STRING_MAGIC: u32 = 0x5354_5220;
/// Magic bytes for a string value with extra data: "WRTS".
const STRING_EXTRA_MAGIC: u32 = 0x5354_5257;
/// Minimum file size: 24-byte header.
const MIN_FILE_SIZE: usize = 24;

/// Parsed CSF string table.
///
/// Keys are stored uppercased for case-insensitive lookup.
/// Values are decoded Unicode strings.
#[derive(Debug, Clone)]
pub struct CsfFile {
    pub version: u32,
    pub language: u32,
    // Ares's override hook frees the old value before reading its replacement.
    // A malformed body can therefore leave a registered label with a null value.
    entries: CsfEntries,
}

/// Raw diagnostics retain the existing map semantics. The active Ares table
/// owns an ordered label array instead: appending before searching can produce
/// duplicate slots even when every supplemental file has unique labels.
#[derive(Debug, Clone)]
enum CsfEntries {
    Raw(HashMap<String, Option<CsfText>>),
    #[cfg(feature = "ares")]
    Ares(Vec<(String, Option<CsfText>)>),
}

/// Text and its original record travel together when native sorting moves slots.
/// Diagnostic raw text must come from the same slot as the displayed value.
#[derive(Debug, Clone)]
struct CsfText {
    value: String,
    raw: String,
    stored_key: String,
}

impl CsfFile {
    /// Parse a CSF file from raw bytes.
    pub fn from_bytes(data: &[u8]) -> Result<Self, AssetError> {
        if data.len() < MIN_FILE_SIZE {
            return Err(AssetError::ParseError {
                format: "CSF".to_string(),
                detail: format!(
                    "file too small: {} bytes (minimum {})",
                    data.len(),
                    MIN_FILE_SIZE
                ),
            });
        }

        let magic: u32 = read_u32_le(data, 0);
        if magic != HEADER_MAGIC {
            return Err(AssetError::ParseError {
                format: "CSF".to_string(),
                detail: format!(
                    "bad header magic: {:#010X} (expected {:#010X})",
                    magic, HEADER_MAGIC
                ),
            });
        }

        let version: u32 = read_u32_le(data, 4);
        let label_count: u32 = read_u32_le(data, 8);
        let string_count = read_u32_le(data, 12);
        if label_count == 0 || string_count == 0 {
            return Err(AssetError::ParseError {
                format: "CSF".to_string(),
                detail: "header label and string counts must both be nonzero".to_string(),
            });
        }
        // +0x10 is reserved. Version 2+
        // stores the language DWORD at +0x14; older tables force language 0.
        let language = if version >= 2 {
            read_u32_le(data, 20)
        } else {
            0
        };

        // The header counts are validated but are not the loop bound: retail
        // walks records until the next read/magic check fails. Bound the map
        // reservation by what the buffer could hold — one entry needs at least
        // its 12-byte label header plus a 10-byte one-character string value —
        // so a malformed count cannot steer the allocation before any record
        // is parsed.
        let possible_entries = (data.len() / 22) as u32;
        let mut entries = HashMap::with_capacity(label_count.min(possible_entries) as usize);
        let mut offset: usize = 24;

        // Retail walks records until the next read/magic check fails;
        // header counts are validated but are not the loop bound.
        while let Ok((key, value, next_offset)) = parse_label_entry(data, offset) {
            entries.insert(key, Some(value));
            offset = next_offset;
        }

        log::info!(
            "CSF: parsed {} entries (version={}, language={})",
            entries.len(),
            version,
            language,
        );

        Ok(Self {
            version,
            language,
            entries: CsfEntries::Raw(entries),
        })
    }

    /// The process string table used by both the game and asset diagnostics.
    /// Stock YR loads the md table alone. Installed Ares6BD886->10060DF0
    /// appends its fixed candidates; native controls: tools/rules_oracle/ares_csf.
    pub fn load_active<'a>(
        mut lookup: impl FnMut(&str) -> Option<&'a [u8]>,
    ) -> Result<(Self, Vec<String>), AssetError> {
        let bytes = lookup("ra2md.csf").ok_or_else(|| AssetError::AssetNotFound {
            name: "ra2md.csf".to_string(),
        })?;
        let mut table = Self::from_bytes(bytes)?;
        #[cfg(feature = "ares")]
        {
            let CsfEntries::Raw(entries) = table.entries else {
                unreachable!("from_bytes returns a raw table")
            };
            let mut labels: Vec<_> = entries.into_iter().collect();
            // Base unique labels are sorted before the supplemental loader runs.
            // Their map iteration order cannot affect this unique-key order.
            ares_sort_labels(&mut labels);
            table.entries = CsfEntries::Ares(labels);
        }
        let mut admitted = Vec::new();
        #[cfg(feature = "ares")]
        for (name, ignore_language) in table.ares_candidates() {
            if let Some(bytes) = lookup(&name)
                && table.append_ares(bytes, ignore_language)
            {
                admitted.push(name);
            }
        }
        // Keep the feature-OFF load route unchanged.
        #[cfg(not(feature = "ares"))]
        let _ = (&mut table, &mut admitted);
        Ok((table, admitted))
    }

    #[cfg(feature = "ares")]
    fn ares_candidates(&self) -> impl Iterator<Item = (String, bool)> + use<> {
        // Original GetLanguage734640 table845678, descriptor+8. Ares
        //10060E13 uses "us" for an unknown language, not a host locale.
        let short = match self.language {
            1 => "uk",
            2 => "ge",
            3 => "fr",
            4 => "sp",
            5 => "it",
            6 => "ja",
            7 => "jb",
            8 => "ko",
            9 => "ch",
            _ => "us",
        };
        [
            ("ares.csf".to_string(), true),
            (format!("ares_{short}.csf"), false),
        ]
        .into_iter()
        .chain((0..100).map(|index| (format!("stringtable{index:02}.csf"), false)))
    }

    ///100608C0's admission is distinct from LoadFile's base-header checks:
    /// supplemental zero counts are valid, and the base language never changes.
    #[cfg(feature = "ares")]
    fn append_ares(&mut self, data: &[u8], ignore_language: bool) -> bool {
        if data.len() < MIN_FILE_SIZE
            || read_u32_le(data, 0) != HEADER_MAGIC
            || (read_u32_le(data, 4) as i32) < 2
            || (!ignore_language && ![self.language, u32::MAX].contains(&read_u32_le(data, 20)))
        {
            return false;
        }
        //734A5F->10060C80 searches the CURRENT array using CRT bsearch,
        //including unsorted new slots.10060D4A immediately grows that array;
        //only100609A0 sorts it after ReadFile returns (including a bad suffix).
        let CsfEntries::Ares(labels) = &mut self.entries else {
            unreachable!("supplements belong to the active ordered table")
        };
        let mut offset = MIN_FILE_SIZE;
        'records: while let Ok((key, pairs, start)) = parse_label_header(data, offset) {
            let slot =
                native_label_search(labels, &key.to_ascii_uppercase()).unwrap_or_else(|| {
                    labels.push((key.to_ascii_uppercase(), None));
                    labels.len() - 1
                });
            // The hook frees an existing value before decoding its replacement.
            labels[slot].1 = None;
            let mut position = start;
            for index in 0..pairs {
                let Ok((value, next)) = parse_value(data, position, &key) else {
                    break 'records;
                };
                if index == 0 {
                    labels[slot].1 = Some(value);
                }
                position = next;
            }
            offset = position;
        }
        ares_sort_labels(labels);
        true
    }

    /// Look up a string by label name (case-insensitive).
    pub fn get(&self, key: &str) -> Option<&str> {
        self.lookup_text(key).map(|text| text.value.as_str())
    }

    /// Stored key spelling and unnormalized text for the very same selected slot.
    /// This lets diagnostics describe a native duplicate winner without merging
    /// the input files again under different duplicate rules.
    pub fn raw_entry(&self, key: &str) -> Option<(&str, &str)> {
        self.lookup_text(key)
            .map(|text| (text.stored_key.as_str(), text.raw.as_str()))
    }

    fn lookup_text(&self, key: &str) -> Option<&CsfText> {
        let key = key.to_ascii_uppercase();
        match &self.entries {
            CsfEntries::Raw(entries) => entries.get(&key)?.as_ref(),
            #[cfg(feature = "ares")]
            CsfEntries::Ares(labels) => labels[native_label_search(labels, &key)?].1.as_ref(),
        }
    }

    /// Native initialized-table lookup. Missing labels are visible, not an
    /// invitation for each caller to substitute its own English fallback.
    pub fn text<'a>(&'a self, key: &str) -> Cow<'a, str> {
        self.get(key)
            .map(Cow::Borrowed)
            .unwrap_or_else(|| Cow::Owned(format!("MISSING:'{key}'")))
    }

    /// Number of entries in the string table.
    pub fn len(&self) -> usize {
        match &self.entries {
            CsfEntries::Raw(entries) => entries.len(),
            #[cfg(feature = "ares")]
            CsfEntries::Ares(labels) => labels.len(),
        }
    }

    /// Whether the string table is empty.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Iterate over all (key, value) pairs in the string table.
    pub fn entries(&self) -> Box<dyn Iterator<Item = (&str, &str)> + '_> {
        match &self.entries {
            CsfEntries::Raw(entries) => Box::new(
                entries
                    .iter()
                    .filter_map(|(k, v)| v.as_ref().map(|v| (k.as_str(), v.value.as_str()))),
            ),
            #[cfg(feature = "ares")]
            CsfEntries::Ares(labels) => Box::new(
                labels
                    .iter()
                    .filter_map(|(k, v)| v.as_ref().map(|v| (k.as_str(), v.value.as_str()))),
            ),
        }
    }
}

/// AbstractTypeClass's visible type name (`+0x60`): the constructor leaves
/// it empty (`0x004108EE`), and ReadINI resolves `UIName=` through the
/// process string table (`0x00410AFB..0x00410B69`). `Name=` is a separate
/// internal/debug name. Without loaded CSF assets, retain the authored key.
pub fn type_ui_name(ui_name: Option<&str>, csf: Option<&CsfFile>) -> String {
    ui_name.map_or_else(String::new, |key| {
        csf.map_or_else(|| key.to_string(), |csf| csf.text(key).into_owned())
    })
}

/// One argument for [`format_csf`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum CsfArg<'a> {
    Str(&'a str),
    Int(i64),
}

/// Substitute arguments into a CSF value that is a `printf` format string.
///
/// Several retail labels are formats rather than finished text —
/// `TXT_MONEY_FORMAT_2` is `"%s $%d"`, `TXT_POWER_DRAIN` is
/// `"Power = %d\nDrain = %d"`. The engine feeds them to `swprintf`, which
/// consumes its variadic arguments strictly in the order the conversion
/// specifiers appear, so callers must never rebuild the surrounding literal
/// text (the currency sign, the `Power =` prefix) in Rust: it lives in the
/// localized table and changes per language.
///
/// Supported the way the retail formats use them: `%%` for a literal percent,
/// `%s` (with the `h`/`l`/`w` size prefixes the engine's wide call sites
/// carry) for a string argument, and `%d`/`%i`/`%u` — optionally
/// zero/width/`hh`..`ll`-qualified, as in `TXT_TIME_FORMAT_HOURS`'s `%02d` —
/// for an integer argument. A specifier with no argument left, or one this
/// helper does not model, is copied through verbatim so a mismatched
/// localization degrades to visible text instead of silently dropping content.
pub fn format_csf(fmt: &str, args: &[CsfArg]) -> String {
    let mut out = String::with_capacity(fmt.len() + 16);
    let mut next_arg = 0usize;
    let chars: Vec<char> = fmt.chars().collect();
    let mut i = 0usize;
    while i < chars.len() {
        if chars[i] != '%' {
            out.push(chars[i]);
            i += 1;
            continue;
        }
        // Scan the specifier: flags/width/precision/size prefix, then the
        // conversion character.
        let start = i;
        let mut j = i + 1;
        while j < chars.len() && !chars[j].is_ascii_alphabetic() && chars[j] != '%' {
            j += 1;
        }
        while j < chars.len() && matches!(chars[j], 'h' | 'l' | 'w' | 'L') {
            j += 1;
        }
        if j >= chars.len() {
            // Trailing '%' with no conversion — copy the tail verbatim.
            out.extend(&chars[start..]);
            break;
        }
        let conv = chars[j];
        i = j + 1;
        match conv {
            '%' => out.push('%'),
            's' | 'S' => match args.get(next_arg) {
                Some(CsfArg::Str(s)) => {
                    out.push_str(s);
                    next_arg += 1;
                }
                _ => out.extend(&chars[start..=j]),
            },
            'd' | 'i' | 'u' => match args.get(next_arg) {
                Some(CsfArg::Int(v)) => {
                    push_padded_int(&mut out, &chars[start + 1..j], *v);
                    next_arg += 1;
                }
                _ => out.extend(&chars[start..=j]),
            },
            _ => out.extend(&chars[start..=j]),
        }
    }
    out
}

/// Apply the zero-pad / minimum-width part of an integer specifier
/// (`%02d` in `TXT_TIME_FORMAT_HOURS`). Anything else in the flag run is
/// ignored, matching how the retail formats actually use it.
fn push_padded_int(out: &mut String, flags: &[char], value: i64) {
    let zero_pad = flags.first() == Some(&'0');
    let width: usize = flags
        .iter()
        .skip(usize::from(zero_pad))
        .take_while(|c| c.is_ascii_digit())
        .fold(0usize, |acc, c| acc * 10 + (*c as usize - '0' as usize));
    let digits = value.to_string();
    if digits.len() < width {
        let pad = if zero_pad { '0' } else { ' ' };
        // Zero padding goes after the sign, exactly like printf.
        if zero_pad && value < 0 {
            out.push('-');
            for _ in 0..(width - digits.len()) {
                out.push(pad);
            }
            out.push_str(&digits[1..]);
            return;
        }
        for _ in 0..(width - digits.len()) {
            out.push(pad);
        }
    }
    out.push_str(&digits);
}

/// Parse one label entry starting at `offset`. Returns (key, value, next_offset).
fn parse_label_entry(data: &[u8], offset: usize) -> Result<(String, CsfText, usize), ()> {
    let (label_name, pair_count, mut pos) = parse_label_header(data, offset)?;
    let mut value = CsfText {
        value: String::new(),
        raw: String::new(),
        stored_key: label_name.clone(),
    };
    for _ in 0..pair_count {
        let (parsed, next) = parse_value(data, pos, &label_name)?;
        if value.value.is_empty() {
            value = parsed;
        }
        pos = next;
    }
    Ok((label_name.to_ascii_uppercase(), value, pos))
}

/// One header/value decoder serves base tables and in-place Ares overrides.
fn parse_label_header(data: &[u8], offset: usize) -> Result<(String, u32, usize), ()> {
    // Need at least 12 bytes: 4 (LBL magic) + 4 (pair count) + 4 (name length).
    if offset + 12 > data.len() {
        return Err(());
    }

    let magic: u32 = read_u32_le(data, offset);
    if magic != LABEL_MAGIC {
        return Err(());
    }

    let pair_count: u32 = read_u32_le(data, offset + 4);
    let name_len: u32 = read_u32_le(data, offset + 8);
    let name_start: usize = offset + 12;
    let name_end: usize = name_start + name_len as usize;

    if name_end > data.len() {
        return Err(());
    }

    let label_name = crate::util::native_string::widen_bytes(&data[name_start..name_end]);

    Ok((label_name, pair_count, name_end))
}

fn parse_value(data: &[u8], mut pos: usize, stored_key: &str) -> Result<(CsfText, usize), ()> {
    if pos + 8 > data.len() {
        return Err(());
    }

    let str_magic: u32 = read_u32_le(data, pos);
    let has_extra = str_magic == STRING_EXTRA_MAGIC;

    if str_magic != STRING_MAGIC && str_magic != STRING_EXTRA_MAGIC {
        return Err(());
    }

    let char_count: u32 = read_u32_le(data, pos + 4);
    let byte_count: usize = char_count as usize * 2; // UTF-16-LE: 2 bytes per char.
    pos += 8;

    if pos + byte_count > data.len() {
        return Err(());
    }

    let value = decode_csf_string(&data[pos..pos + byte_count], stored_key);
    pos += byte_count;

    // Skip the extra value if present (used for audio cue names, not display text).
    if has_extra {
        if pos + 4 > data.len() {
            return Err(());
        }
        let extra_len: u32 = read_u32_le(data, pos);
        pos += 4 + extra_len as usize;
        if pos > data.len() {
            return Err(());
        }
    }
    Ok((value, pos))
}

/// Decode a CSF-encoded string: bitwise-NOT each byte, interpret as UTF-16-LE,
/// then apply the engine's load-time whitespace normalization.
fn decode_csf_string(encoded: &[u8], stored_key: &str) -> CsfText {
    let decoded_bytes: Vec<u8> = encoded.iter().map(|b| !b).collect();
    let u16_values: Vec<u16> = decoded_bytes
        .as_chunks::<2>()
        .0
        .iter()
        .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
        .collect();
    CsfText {
        value: String::from_utf16_lossy(&normalize_whitespace(&u16_values)),
        raw: String::from_utf16_lossy(&u16_values),
        stored_key: stored_key.to_owned(),
    }
}

/// The original engine normalizes whitespace in every CSF string at load
/// time; retail files rely on it (213 retail strings — briefings, tooltips —
/// carry padding spaces around line breaks that the game never displays).
/// Rules, applied over UTF-16 code units:
/// - a space is dropped when it follows another space, starts the string, or
///   starts a line (i.e. follows a newline/tab);
/// - a space immediately before a newline or tab is dropped;
/// - one trailing space is trimmed.
fn normalize_whitespace(units: &[u16]) -> Vec<u16> {
    const SPACE: u16 = 0x20;
    const NEWLINE: u16 = 0x0A;
    const TAB: u16 = 0x09;
    let mut out: Vec<u16> = Vec::with_capacity(units.len());
    let mut prev: u16 = 0;
    let mut at_line_start = true;
    for &c in units {
        if c == SPACE {
            if prev != SPACE && !at_line_start {
                out.push(c);
                at_line_start = false;
                prev = c;
            }
            // Skipped spaces leave prev/at_line_start untouched.
        } else if c == NEWLINE || c == TAB {
            if prev == SPACE {
                out.pop();
            }
            out.push(c);
            at_line_start = true;
            prev = c;
        } else {
            out.push(c);
            at_line_start = false;
            prev = c;
        }
    }
    if prev == SPACE {
        out.pop();
    }
    out
}

/// Encode a string into CSF format (UTF-16-LE then bitwise-NOT).
#[cfg(any(test, feature = "test-support"))]
fn encode_csf_string(s: &str) -> Vec<u8> {
    let utf16: Vec<u16> = s.encode_utf16().collect();
    utf16
        .iter()
        .flat_map(|c| c.to_le_bytes())
        .map(|b| !b)
        .collect()
}

/// Build a minimal CSF file with one label entry.
#[cfg(any(test, feature = "test-support"))]
fn build_test_csf(label: &str, value: &str) -> Vec<u8> {
    let encoded_value: Vec<u8> = encode_csf_string(value);
    let char_count: u32 = value.encode_utf16().count() as u32;

    let mut data: Vec<u8> = Vec::new();

    // Header (24 bytes).
    data.extend_from_slice(&HEADER_MAGIC.to_le_bytes()); // " FSC"
    data.extend_from_slice(&3u32.to_le_bytes()); // version
    data.extend_from_slice(&1u32.to_le_bytes()); // label count
    data.extend_from_slice(&1u32.to_le_bytes()); // string pair count
    data.extend_from_slice(&0xAABB_CCDDu32.to_le_bytes()); // reserved
    data.extend_from_slice(&0u32.to_le_bytes()); // language (US English)

    // Label entry.
    data.extend_from_slice(&LABEL_MAGIC.to_le_bytes()); // " LBL"
    data.extend_from_slice(&1u32.to_le_bytes()); // 1 string pair
    data.extend_from_slice(&(label.len() as u32).to_le_bytes());
    data.extend_from_slice(label.as_bytes());

    // String value.
    data.extend_from_slice(&STRING_MAGIC.to_le_bytes()); // " RTS"
    data.extend_from_slice(&char_count.to_le_bytes());
    data.extend_from_slice(&encoded_value);

    data
}

/// Authored localization fixture using the CSF owner's binary encoder.
#[cfg(any(test, feature = "test-support"))]
pub fn test_csf(entries: &[(&str, &str)]) -> CsfFile {
    assert!(!entries.is_empty());
    let mut data = build_test_csf(entries[0].0, entries[0].1);
    let count = u32::try_from(entries.len()).unwrap().to_le_bytes();
    data[8..12].copy_from_slice(&count);
    data[12..16].copy_from_slice(&count);
    for &(key, value) in &entries[1..] {
        data.extend_from_slice(&build_test_csf(key, value)[24..]);
    }
    CsfFile::from_bytes(&data).unwrap()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_huge_header_label_count_cannot_steer_the_map_reservation() {
        // 24-byte header claiming u32::MAX labels with no records behind it.
        // The reservation must come from the file size, not the count: the
        // count once asked for roughly 833 GB before the first read.
        let mut data = Vec::with_capacity(25);
        data.extend_from_slice(&HEADER_MAGIC.to_le_bytes());
        data.extend_from_slice(&3u32.to_le_bytes());
        data.extend_from_slice(&u32::MAX.to_le_bytes());
        data.extend_from_slice(&u32::MAX.to_le_bytes());
        data.extend_from_slice(&[0; 8]);
        data.push(0);
        let parsed = CsfFile::from_bytes(&data).expect("parse walks until reads fail");
        match parsed.entries {
            CsfEntries::Raw(entries) => assert!(entries.is_empty()),
            #[cfg(feature = "ares")]
            CsfEntries::Ares(entries) => assert!(entries.is_empty()),
        }
    }

    #[cfg(feature = "ares")]
    fn supplemental_fixture(
        entries: &[(&str, &str)],
        language: u32,
        version: u32,
        counts: u32,
    ) -> Vec<u8> {
        let mut data = build_test_csf(entries[0].0, entries[0].1);
        for &(key, value) in &entries[1..] {
            data.extend_from_slice(&build_test_csf(key, value)[24..]);
        }
        data[4..8].copy_from_slice(&version.to_le_bytes());
        data[8..12].copy_from_slice(&counts.to_le_bytes());
        data[12..16].copy_from_slice(&counts.to_le_bytes());
        data[20..24].copy_from_slice(&language.to_le_bytes());
        data
    }

    #[test]
    fn active_csf_profile_uses_shared_base_and_optional_supplements() {
        let base = build_test_csf("A", "base");
        let extra = build_test_csf("A", "extra");
        let mut requested = Vec::new();
        let (table, admitted) = CsfFile::load_active(|name| {
            requested.push(name.to_string());
            match name {
                "ra2md.csf" => Some(base.as_slice()),
                "stringtable00.csf" => Some(extra.as_slice()),
                _ => None,
            }
        })
        .unwrap();
        assert_eq!(
            table.get("a"),
            Some(if cfg!(feature = "ares") {
                "extra"
            } else {
                "base"
            })
        );
        assert_eq!(admitted.len(), usize::from(cfg!(feature = "ares")));
        assert_eq!(
            requested.len(),
            if cfg!(feature = "ares") { 103 } else { 1 }
        );
        assert!(
            CsfFile::load_active(|_| None).is_err(),
            "base table is required in both profiles"
        );
    }

    #[cfg(feature = "ares")]
    #[test]
    fn supplemental_csf_three_native_histories_preserve_order_gates_and_partial_writes() {
        let native: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
            "tools/rules_oracle/ares_csf.json",
        ))
        .unwrap();
        for row in native["controls"].as_array().unwrap() {
            let fixture = |value, language, version, count| {
                supplemental_fixture(&[("A", value)], language, version, count)
            };
            let mut files: HashMap<String, Vec<u8>> = HashMap::new();
            match row["name"].as_str().unwrap() {
                "ordered_overrides" => {
                    files.insert("ares.csf".into(), fixture("generic", 9, 3, 1));
                    files.insert(
                        "ares_us.csf".into(),
                        supplemental_fixture(&[("a", "locale"), ("NEW", "new")], 0, 3, 2),
                    );
                    files.insert("stringtable00.csf".into(), fixture("zero", u32::MAX, 3, 1));
                    files.insert("stringtable01.csf".into(), fixture("one", 0, 3, 1));
                }
                "language_and_header_admission" => {
                    files.insert("ares.csf".into(), fixture("generic", 9, 3, 1));
                    files.insert("ares_us.csf".into(), fixture("wrong", 9, 3, 1));
                    files.insert("stringtable00.csf".into(), fixture("zero_counts", 0, 3, 0));
                    files.insert("stringtable01.csf".into(), fixture("bad_version", 0, 1, 1));
                    files.insert("stringtable02.csf".into(), b"bad".to_vec());
                    let mut bad_magic = fixture("bad_magic", 0, 3, 1);
                    bad_magic[..4].copy_from_slice(b"BAD!");
                    files.insert("stringtable03.csf".into(), bad_magic);
                    files.insert(
                        "stringtable04.csf".into(),
                        fixture("signed_version", 0, 0x80000000, 1),
                    );
                }
                "partial_body_keeps_prior_writes" => {
                    let mut data = supplemental_fixture(&[("NEW", "committed")], 0, 3, 1);
                    let broken = fixture("clear_old", 0, 3, 1);
                    data.extend_from_slice(&broken[24..24 + 12 + 1]);
                    data.extend_from_slice(&0_u32.to_le_bytes());
                    files.insert("stringtable00.csf".into(), data);
                }
                name => panic!("unknown native history {name}"),
            }
            files.insert(
                "ra2md.csf".into(),
                supplemental_fixture(&[("A", "base"), ("KEEP", "keep")], 0, 3, 2),
            );
            let mut requested = Vec::new();
            let (table, admitted) = CsfFile::load_active(|name| {
                if name != "ra2md.csf" {
                    requested.push(name.to_string());
                }
                files.get(name).map(Vec::as_slice)
            })
            .unwrap();
            let expected = &row["result"];
            assert_eq!(serde_json::json!(requested), expected["attempts"]);
            assert_eq!(serde_json::json!(admitted), expected["accepted"]);
            assert_eq!(serde_json::json!(table.language), expected["base_language"]);
            assert_eq!(table.len(), expected["entries"].as_object().unwrap().len());
            for (key, value) in expected["entries"].as_object().unwrap() {
                assert_eq!(table.get(key), value.as_str(), "{} {key}", row["name"]);
            }
        }
    }

    #[cfg(feature = "ares")]
    #[test]
    fn supplemental_unique_records_match_native_ordered_slots_and_lookup() {
        let native: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
            "tools/rules_oracle/ares_csf_ordered.json",
        ))
        .unwrap();
        for row in native["controls"].as_array().unwrap() {
            let fixture = |entries: &serde_json::Value| {
                let pairs: Vec<_> = entries
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|entry| (entry[0].as_str().unwrap(), entry[1].as_str().unwrap()))
                    .collect();
                supplemental_fixture(&pairs, 0, 3, pairs.len() as u32)
            };
            let base = fixture(&row["base"]);
            let extra = fixture(&row["supplemental"]);
            let (table, _) = CsfFile::load_active(|name| match name {
                "ra2md.csf" => Some(base.as_slice()),
                "stringtable00.csf" => Some(extra.as_slice()),
                _ => None,
            })
            .unwrap();
            let expected = &row["result"];
            assert_eq!(
                table.len(),
                expected["label_count"].as_u64().unwrap() as usize,
                "{}",
                row["name"]
            );
            let labels: Vec<_> = table.entries().collect();
            assert_eq!(
                serde_json::json!(labels),
                expected["labels"],
                "{}",
                row["name"]
            );
            // These are actual StringTable::LoadString outputs, not a map
            // projection of the native labels (which would silently lose ties).
            for (key, value) in expected["lookups"].as_object().unwrap() {
                assert_eq!(table.get(key), value.as_str(), "{} {key}", row["name"]);
            }
        }
    }

    #[test]
    fn format_csf_consumes_specifiers_in_order_and_keeps_literals() {
        // Retail TXT_MONEY_FORMAT_2 — the currency sign is table data, not
        // something a caller may rebuild.
        assert_eq!(
            format_csf(
                "%s $%d",
                &[CsfArg::Str("Grizzly Battle Tank"), CsfArg::Int(700)]
            ),
            "Grizzly Battle Tank $700"
        );
        // Retail TXT_MONEY_FORMAT_1.
        assert_eq!(format_csf("$%d", &[CsfArg::Int(700)]), "$700");
        // Retail TXT_POWER_DRAIN — two integers, embedded newline preserved.
        assert_eq!(
            format_csf(
                "Power = %d\nDrain = %d",
                &[CsfArg::Int(150), CsfArg::Int(220)]
            ),
            "Power = 150\nDrain = 220"
        );
        // A localization that reorders must still pair args by specifier
        // order, exactly like swprintf.
        assert_eq!(
            format_csf("%d kr %s", &[CsfArg::Int(700), CsfArg::Str("Grizzly")]),
            "700 kr Grizzly"
        );
    }

    #[test]
    fn format_csf_handles_percent_width_and_size_prefixes() {
        // Retail TXT_TIME_FORMAT_HOURS uses %02d.
        assert_eq!(
            format_csf(
                "Time: %02d:%02d:%02d",
                &[CsfArg::Int(1), CsfArg::Int(5), CsfArg::Int(30)]
            ),
            "Time: 01:05:30"
        );
        assert_eq!(format_csf("100%%", &[]), "100%");
        // The engine's wide call sites carry %hs / %ls.
        assert_eq!(format_csf("%hs", &[CsfArg::Str("ok")]), "ok");
        assert_eq!(format_csf("%ls", &[CsfArg::Str("ok")]), "ok");
    }

    #[test]
    fn format_csf_leaves_unsatisfied_specifiers_visible() {
        // A localization with more specifiers than the call site supplies must
        // degrade to visible text, never silently swallow content.
        assert_eq!(
            format_csf("%s $%d", &[CsfArg::Str("Grizzly")]),
            "Grizzly $%d"
        );
        assert_eq!(format_csf("%d", &[CsfArg::Str("x")]), "%d");
        assert_eq!(format_csf("abc%", &[]), "abc%");
    }

    #[test]
    fn parse_minimal_csf() {
        let data: Vec<u8> = build_test_csf("NAME:MTNK", "Grizzly Battle Tank");
        let csf: CsfFile = CsfFile::from_bytes(&data).expect("should parse");
        assert_eq!(csf.version, 3);
        assert_eq!(csf.language, 0);
        assert_eq!(csf.len(), 1);
        assert_eq!(csf.get("NAME:MTNK"), Some("Grizzly Battle Tank"));
    }

    #[test]
    fn missing_key_uses_native_visible_marker() {
        let data: Vec<u8> = build_test_csf("NAME:MTNK", "Grizzly Battle Tank");
        let csf: CsfFile = CsfFile::from_bytes(&data).expect("should parse");
        assert_eq!(csf.get("NAME:NONEXISTENT"), None);
        assert_eq!(csf.text("NAME:NONEXISTENT"), "MISSING:'NAME:NONEXISTENT'");
    }

    #[test]
    fn language_is_version_gated_dword_at_header_14() {
        let mut data = build_test_csf("A", "B");
        data[16..20].copy_from_slice(&0x1122_3344u32.to_le_bytes());
        data[20..24].copy_from_slice(&0x5566_7788u32.to_le_bytes());
        let csf = CsfFile::from_bytes(&data).expect("version 3");
        assert_eq!(csf.language, 0x5566_7788);

        data[4..8].copy_from_slice(&1u32.to_le_bytes());
        let csf = CsfFile::from_bytes(&data).expect("version 1");
        assert_eq!(csf.language, 0);
    }

    #[test]
    fn header_requires_nonzero_label_and_string_counts() {
        let mut data = build_test_csf("A", "B");
        data[8..12].copy_from_slice(&0u32.to_le_bytes());
        assert!(CsfFile::from_bytes(&data).is_err());

        let mut data = build_test_csf("A", "B");
        data[12..16].copy_from_slice(&0u32.to_le_bytes());
        assert!(CsfFile::from_bytes(&data).is_err());
    }

    #[test]
    fn record_walk_is_not_bounded_by_header_label_count() {
        let mut data = build_test_csf("A", "First");
        let second = build_test_csf("B", "Second");
        data.extend_from_slice(&second[MIN_FILE_SIZE..]);

        let csf = CsfFile::from_bytes(&data).expect("record walk");
        assert_eq!(csf.get("A"), Some("First"));
        assert_eq!(csf.get("B"), Some("Second"));
    }

    #[test]
    fn alternate_record_magics_are_not_accepted() {
        let mut data = build_test_csf("A", "B");
        data[MIN_FILE_SIZE..MIN_FILE_SIZE + 4].copy_from_slice(&0x204C_424Cu32.to_le_bytes());

        let csf = CsfFile::from_bytes(&data).expect("valid header");
        assert!(csf.is_empty());
    }

    #[test]
    fn reject_bad_magic() {
        let mut data: Vec<u8> = build_test_csf("NAME:MTNK", "Tank");
        data[0] = 0x00; // corrupt magic
        assert!(CsfFile::from_bytes(&data).is_err());
    }

    #[test]
    fn load_time_whitespace_normalization_matches_engine() {
        // Space-padded line breaks (the retail briefing/tooltip pattern):
        // spaces before AND after a newline are dropped.
        let data: Vec<u8> = build_test_csf("Tip:X", "Click to show \n advanced commands");
        let csf: CsfFile = CsfFile::from_bytes(&data).expect("should parse");
        assert_eq!(csf.get("Tip:X"), Some("Click to show\nadvanced commands"));

        // Consecutive spaces collapse; leading skipped; one trailing trimmed.
        let data: Vec<u8> = build_test_csf("A", "  double  space inside ");
        let csf: CsfFile = CsfFile::from_bytes(&data).expect("should parse");
        assert_eq!(csf.get("A"), Some("double space inside"));

        // Space before a tab is dropped and the tab restarts a line.
        let data: Vec<u8> = build_test_csf("B", "col \t next");
        let csf: CsfFile = CsfFile::from_bytes(&data).expect("should parse");
        assert_eq!(csf.get("B"), Some("col\tnext"));
    }

    #[test]
    fn wrts_entry_with_extra_value() {
        let encoded_value: Vec<u8> = encode_csf_string("Tank");
        let char_count: u32 = "Tank".encode_utf16().count() as u32;
        let extra: &[u8] = b"some_audio_cue";

        let mut data: Vec<u8> = Vec::new();

        // Header.
        data.extend_from_slice(&HEADER_MAGIC.to_le_bytes());
        data.extend_from_slice(&3u32.to_le_bytes());
        data.extend_from_slice(&1u32.to_le_bytes());
        data.extend_from_slice(&1u32.to_le_bytes());
        data.extend_from_slice(&0u32.to_le_bytes());
        data.extend_from_slice(&0u32.to_le_bytes());

        // Label.
        data.extend_from_slice(&LABEL_MAGIC.to_le_bytes());
        data.extend_from_slice(&1u32.to_le_bytes());
        let label: &str = "EVA:TankReady";
        data.extend_from_slice(&(label.len() as u32).to_le_bytes());
        data.extend_from_slice(label.as_bytes());

        // WRTS string with extra value.
        data.extend_from_slice(&STRING_EXTRA_MAGIC.to_le_bytes());
        data.extend_from_slice(&char_count.to_le_bytes());
        data.extend_from_slice(&encoded_value);
        data.extend_from_slice(&(extra.len() as u32).to_le_bytes());
        data.extend_from_slice(extra);

        let csf: CsfFile = CsfFile::from_bytes(&data).expect("should parse WRTS");
        assert_eq!(csf.get("EVA:TANKREADY"), Some("Tank"));
    }
}
