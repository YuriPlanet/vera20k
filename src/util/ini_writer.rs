//! In-place INI value writer.
//!
//! Updates (or inserts) a single `key=value` under a named `[section]` in the
//! raw bytes of an INI file, preserving every other byte exactly: comments,
//! key order, casing, blank lines, and the existing line-ending style all
//! round-trip untouched. This mirrors the original game writing individual
//! settings keys in place rather than rewriting the whole file — a naive full
//! rewrite would discard sections the engine does not yet model (e.g.
//! `[Skirmish]`, `[MultiPlayer]`).
//!
//! Operates on raw bytes so content that is not valid UTF-8 elsewhere in the
//! file (e.g. a player name with high-byte characters in another section)
//! round-trips verbatim and is never matched.
//!
//! ## Dependency rules
//! - Part of util/ — no dependencies on game modules.

/// Update (or insert) `[section] key=value` in `content`, returning the new
/// file bytes.
///
/// Matching is case-insensitive on the section and key names (as INI requires);
/// the names are written using the casing passed in. When the key already
/// exists in the section, only its value is replaced and the line's terminator
/// is kept. When the section exists but the key does not, the key is appended
/// to the end of that section. When the section is absent, a new section is
/// appended at the end of the file. An empty input yields a fresh section.
/// Lines that are not valid UTF-8 are passed through verbatim, never matched.
#[cfg(test)]
pub fn set_ini_value(content: &[u8], section: &str, key: &str, value: &str) -> Vec<u8> {
    let target_section = section.trim();
    let target_key = key.trim();
    let new_line = format!("{key}={value}");
    let lines = split_lines(content);

    // Locate the FIRST matching section block, the key within it (if present),
    // and where a missing key would be appended (the end of that block's keys).
    // Win32's WritePrivateProfileString operates on the first matching section
    // span, so any later duplicate `[section]` blocks are ignored for both the
    // lookup and the insert position.
    let mut section_found = false;
    let mut key_idx: Option<usize> = None;
    let mut insert_after: Option<usize> = None;
    let mut in_first_block = false;
    for (idx, (text, _)) in lines.iter().enumerate() {
        let Ok(line) = std::str::from_utf8(text) else {
            continue; // non-UTF-8: never a header/key, leave untouched
        };
        if let Some(name) = section_header_name(line) {
            if !section_found && name.eq_ignore_ascii_case(target_section) {
                section_found = true;
                in_first_block = true;
                insert_after = Some(idx);
            } else {
                // Any later header (including a duplicate target) ends the block.
                in_first_block = false;
            }
            continue;
        }
        if in_first_block {
            if let Some(k) = key_name(line) {
                if key_idx.is_none() && k.eq_ignore_ascii_case(target_key) {
                    key_idx = Some(idx);
                }
                insert_after = Some(idx);
            }
        }
    }

    let mut out = Vec::with_capacity(content.len() + new_line.len() + CRLF.len() + 4);
    for (idx, (text, terminator)) in lines.iter().enumerate() {
        if Some(idx) == key_idx {
            // Replace the value in place, keeping this line's own terminator.
            out.extend_from_slice(new_line.as_bytes());
            out.extend_from_slice(terminator);
            continue;
        }
        out.extend_from_slice(text);
        out.extend_from_slice(terminator);
        if key_idx.is_none() && section_found && Some(idx) == insert_after {
            // Section exists but the key does not — append it here. If the
            // anchor line was the unterminated last line, terminate it first.
            if terminator.is_empty() {
                out.extend_from_slice(CRLF);
            }
            out.extend_from_slice(new_line.as_bytes());
            out.extend_from_slice(CRLF);
        }
    }

    if !section_found {
        if !out.is_empty() && !out.ends_with(b"\n") {
            out.extend_from_slice(CRLF);
        }
        out.extend_from_slice(format!("[{section}]").as_bytes());
        out.extend_from_slice(CRLF);
        out.extend_from_slice(new_line.as_bytes());
        out.extend_from_slice(CRLF);
    }

    out
}

/// Update several keys in the first matching section in one traversal and one
/// output buffer.
///
/// Existing keys keep their position and line terminator. Missing keys are
/// appended after the last key in the section in `values` order; a missing
/// section is appended with every key in that same order. Matching remains
/// case-insensitive, while written key casing comes from `values`. Duplicate
/// update keys are collapsed case-insensitively: the last supplied spelling and
/// value win without changing the first occurrence's order. If the file itself
/// repeats a key, its last occurrence is updated, matching the parser's
/// later-value-wins lookup policy.
///
/// This is the preservation-safe path for native writers that update a whole
/// settings snapshot in memory before performing one final file save.
pub fn set_ini_values(content: &[u8], section: &str, values: &[(&str, &str)]) -> Vec<u8> {
    if values.is_empty() {
        return content.to_vec();
    }

    let target_section = section.trim();
    let mut updates: Vec<(&str, &str)> = Vec::with_capacity(values.len());
    for &(key, value) in values {
        let key = key.trim();
        if let Some(existing) = updates
            .iter_mut()
            .find(|(existing_key, _)| existing_key.eq_ignore_ascii_case(key))
        {
            *existing = (key, value);
        } else {
            updates.push((key, value));
        }
    }

    let lines = split_lines(content);
    let mut section_found = false;
    let mut in_first_block = false;
    let mut insert_after: Option<usize> = None;
    let mut key_lines = vec![None; updates.len()];

    for (line_index, (text, _)) in lines.iter().enumerate() {
        let Ok(line) = std::str::from_utf8(text) else {
            continue;
        };
        if let Some(name) = section_header_name(line) {
            if !section_found && name.eq_ignore_ascii_case(target_section) {
                section_found = true;
                in_first_block = true;
                insert_after = Some(line_index);
            } else {
                in_first_block = false;
            }
            continue;
        }
        if !in_first_block {
            continue;
        }
        if let Some(existing_key) = key_name(line) {
            if let Some(update_index) = updates
                .iter()
                .position(|update| update.0.eq_ignore_ascii_case(existing_key))
            {
                key_lines[update_index] = Some(line_index);
            }
            insert_after = Some(line_index);
        }
    }

    let added_len: usize = updates
        .iter()
        .enumerate()
        .filter(|(index, _)| key_lines[*index].is_none())
        .map(|(_, (key, value))| key.len() + 1 + value.len() + CRLF.len())
        .sum();
    let replacement_growth: usize = updates
        .iter()
        .enumerate()
        .filter_map(|(update_index, (key, value))| {
            let line_index = key_lines[update_index]?;
            Some((key.len() + 1 + value.len()).saturating_sub(lines[line_index].0.len()))
        })
        .sum();
    let mut out =
        Vec::with_capacity(content.len() + added_len + replacement_growth + section.len() + 4);

    for (line_index, (text, terminator)) in lines.iter().enumerate() {
        if let Some(update_index) = key_lines.iter().position(|line| *line == Some(line_index)) {
            let (key, value) = updates[update_index];
            out.extend_from_slice(key.as_bytes());
            out.push(b'=');
            out.extend_from_slice(value.as_bytes());
            out.extend_from_slice(terminator);
        } else {
            out.extend_from_slice(text);
            out.extend_from_slice(terminator);
        }

        if section_found && Some(line_index) == insert_after {
            let mut wrote_any = false;
            for (update_index, (key, value)) in updates.iter().enumerate() {
                if key_lines[update_index].is_some() {
                    continue;
                }
                if !wrote_any && terminator.is_empty() {
                    out.extend_from_slice(CRLF);
                }
                out.extend_from_slice(key.as_bytes());
                out.push(b'=');
                out.extend_from_slice(value.as_bytes());
                out.extend_from_slice(CRLF);
                wrote_any = true;
            }
        }
    }

    if !section_found {
        if !out.is_empty() && !out.ends_with(b"\n") {
            out.extend_from_slice(CRLF);
        }
        out.push(b'[');
        out.extend_from_slice(section.as_bytes());
        out.extend_from_slice(b"]\r\n");
        for (key, value) in updates {
            out.extend_from_slice(key.as_bytes());
            out.push(b'=');
            out.extend_from_slice(value.as_bytes());
            out.extend_from_slice(CRLF);
        }
    }

    out
}

/// Terminator for lines this writer *adds* (an inserted key or a created
/// section): always CRLF, the convention the original game's settings writer
/// (Win32 `WritePrivateProfileString`) emits regardless of the file's existing
/// style. Lines that already exist keep their own terminator on a value
/// replace.
const CRLF: &[u8] = b"\r\n";

/// Split `content` into `(text, terminator)` pairs, one per line, where `text`
/// excludes the line ending and `terminator` is the exact ending bytes
/// (`"\r\n"`, `"\n"`, or empty for a final line with no trailing newline).
fn split_lines(content: &[u8]) -> Vec<(&[u8], &[u8])> {
    let mut lines = Vec::new();
    let mut start = 0usize;
    let mut i = 0usize;
    while i < content.len() {
        if content[i] == b'\n' {
            let has_cr = i > start && content[i - 1] == b'\r';
            let text_end = if has_cr { i - 1 } else { i };
            lines.push((&content[start..text_end], &content[text_end..=i]));
            start = i + 1;
        }
        i += 1;
    }
    if start < content.len() {
        lines.push((&content[start..], &content[content.len()..]));
    }
    lines
}

/// The section name inside `[...]`, trimmed, or `None` if `line` is not a clean
/// section header.
fn section_header_name(line: &str) -> Option<&str> {
    let t = line.trim();
    let inner = t.strip_prefix('[')?.strip_suffix(']')?;
    Some(inner.trim())
}

/// The key name before `=`, trimmed, or `None` if `line` is blank, a comment,
/// a section header, or has no `=`.
fn key_name(line: &str) -> Option<&str> {
    let t = line.trim();
    if t.is_empty() || t.starts_with(';') || t.starts_with('#') || t.starts_with('[') {
        return None;
    }
    let eq = t.find('=')?;
    let k = t[..eq].trim();
    if k.is_empty() { None } else { Some(k) }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(bytes: Vec<u8>) -> String {
        String::from_utf8(bytes).unwrap()
    }

    /// Real RA2MD.INI shape: replacing ScoreVolume keeps every sibling key and
    /// every other section byte-for-byte, drops the old value, and keeps CRLF.
    #[test]
    fn replaces_value_preserving_other_keys_and_sections() {
        let input = b"[Options]\r\nGameSpeed=3\r\n[Audio]\r\nSoundVolume=0.700000\r\n\
ScoreVolume=0.600000\r\nInGameMusic=yes\r\n[Network]\r\nNetID=ffff,ffff,ffff,\r\n";
        let out = s(set_ini_value(input, "Audio", "ScoreVolume", "0.250000"));
        assert!(out.contains("ScoreVolume=0.250000\r\n"));
        assert!(out.contains("SoundVolume=0.700000\r\n"));
        assert!(out.contains("InGameMusic=yes\r\n"));
        assert!(out.contains("[Network]\r\nNetID=ffff,ffff,ffff,\r\n"));
        assert!(!out.contains("0.600000"), "old value must be gone");
    }

    /// The same key name in two sections only updates the targeted section.
    #[test]
    fn updates_only_the_targeted_section() {
        let input = b"[A]\r\nVol=1\r\n[B]\r\nVol=2\r\n";
        let out = s(set_ini_value(input, "B", "Vol", "9"));
        assert_eq!(out, "[A]\r\nVol=1\r\n[B]\r\nVol=9\r\n");
    }

    /// Section present, key absent: the key is appended within that section.
    #[test]
    fn appends_missing_key_within_section() {
        let input = b"[Audio]\r\nSoundVolume=0.7\r\n";
        let out = s(set_ini_value(input, "Audio", "ScoreVolume", "0.5"));
        assert_eq!(out, "[Audio]\r\nSoundVolume=0.7\r\nScoreVolume=0.5\r\n");
    }

    /// Section absent: a new section is appended at the end of the file.
    #[test]
    fn appends_missing_section_at_eof() {
        let input = b"[Options]\r\nGameSpeed=3\r\n";
        let out = s(set_ini_value(input, "Audio", "ScoreVolume", "0.5"));
        assert_eq!(
            out,
            "[Options]\r\nGameSpeed=3\r\n[Audio]\r\nScoreVolume=0.5\r\n"
        );
    }

    /// Empty input yields a fresh section (CRLF default).
    #[test]
    fn empty_input_creates_section() {
        let out = s(set_ini_value(b"", "Audio", "ScoreVolume", "0.4"));
        assert_eq!(out, "[Audio]\r\nScoreVolume=0.4\r\n");
    }

    /// A file using LF endings keeps LF for the rewritten line.
    #[test]
    fn preserves_lf_line_endings() {
        let input = b"[Audio]\nScoreVolume=0.6\n";
        let out = s(set_ini_value(input, "Audio", "ScoreVolume", "0.3"));
        assert_eq!(out, "[Audio]\nScoreVolume=0.3\n");
    }

    /// Section and key names match case-insensitively.
    #[test]
    fn matches_section_and_key_case_insensitively() {
        let input = b"[audio]\r\nscorevolume=0.6\r\n";
        let out = s(set_ini_value(input, "Audio", "ScoreVolume", "0.3"));
        assert_eq!(out, "[audio]\r\nScoreVolume=0.3\r\n");
    }

    /// An unterminated final header line gets terminated before the inserted key.
    #[test]
    fn inserts_after_unterminated_header() {
        let out = s(set_ini_value(b"[Audio]", "Audio", "ScoreVolume", "0.4"));
        assert_eq!(out, "[Audio]\r\nScoreVolume=0.4\r\n");
    }

    /// Comment lines inside the section are preserved on a value replace.
    #[test]
    fn preserves_comment_lines() {
        let input = b"[Audio]\r\n; music level\r\nScoreVolume=0.6\r\n";
        let out = s(set_ini_value(input, "Audio", "ScoreVolume", "0.1"));
        assert_eq!(out, "[Audio]\r\n; music level\r\nScoreVolume=0.1\r\n");
    }

    /// With a duplicated section header, a missing key is appended into the
    /// FIRST matching block (Win32 first-section semantics), not the last.
    #[test]
    fn appends_missing_key_into_first_duplicate_section() {
        let input = b"[Audio]\r\nSoundVolume=0.7\r\n[Audio]\r\nMusicVolume=0.5\r\n";
        let out = s(set_ini_value(input, "Audio", "ScoreVolume", "0.250000"));
        assert_eq!(
            out,
            "[Audio]\r\nSoundVolume=0.7\r\nScoreVolume=0.250000\r\n\
[Audio]\r\nMusicVolume=0.5\r\n"
        );
    }

    /// With the key duplicated across two same-named sections, only the first
    /// occurrence is replaced (first-match-wins, matching the boot reader's
    /// first-section preference).
    #[test]
    fn replaces_only_first_duplicate_section_key() {
        let input = b"[Audio]\r\nScoreVolume=0.6\r\n[Audio]\r\nScoreVolume=0.9\r\n";
        let out = s(set_ini_value(input, "Audio", "ScoreVolume", "0.1"));
        assert_eq!(
            out,
            "[Audio]\r\nScoreVolume=0.1\r\n[Audio]\r\nScoreVolume=0.9\r\n"
        );
    }

    /// A key appended into an LF-only file is written with CRLF (the original
    /// writer always emits CRLF for new lines); existing lines keep their LF.
    #[test]
    fn appended_key_uses_crlf_even_in_lf_file() {
        let input = b"[Audio]\nSoundVolume=0.7\n";
        let out = s(set_ini_value(input, "Audio", "ScoreVolume", "0.5"));
        assert_eq!(out, "[Audio]\nSoundVolume=0.7\nScoreVolume=0.5\r\n");
    }

    #[test]
    fn batch_replaces_and_appends_in_one_preservation_pass() {
        let input = b"[Skirmish]\n; retained\nGameMode=1\nCredits=5000\n\n[Other]\nCredits=7\n";
        let out = s(set_ini_values(
            input,
            "Skirmish",
            &[
                ("GameMode", "3"),
                ("Credits", "10000"),
                ("Slot01", "6,-2,-2"),
            ],
        ));
        assert_eq!(
            out,
            "[Skirmish]\n; retained\nGameMode=3\nCredits=10000\nSlot01=6,-2,-2\r\n\n\
[Other]\nCredits=7\n"
        );
    }

    #[test]
    fn batch_creates_missing_section_in_input_order() {
        let out = s(set_ini_values(
            b"[Options]\r\nFoo=bar\r\n",
            "Skirmish",
            &[("GameMode", "1"), ("ScenIndex", "12"), ("ShortGame", "yes")],
        ));
        assert_eq!(
            out,
            "[Options]\r\nFoo=bar\r\n[Skirmish]\r\nGameMode=1\r\nScenIndex=12\r\n\
ShortGame=yes\r\n"
        );
    }

    #[test]
    fn batch_updates_last_duplicate_key_in_first_section() {
        let input = b"[skirmish]\r\ngamemode=1\r\nGameMode=8\r\n\
[Skirmish]\r\nGameMode=9\r\n";
        let out = s(set_ini_values(
            input,
            "Skirmish",
            &[("GameMode", "2"), ("Credits", "10000")],
        ));
        assert_eq!(
            out,
            "[skirmish]\r\ngamemode=1\r\nGameMode=2\r\nCredits=10000\r\n\
[Skirmish]\r\nGameMode=9\r\n"
        );
    }

    #[test]
    fn batch_preserves_non_utf8_lines_verbatim() {
        let input = b"[Skirmish]\r\n\xff\xfe\r\nGameMode=1\r\n";
        let out = set_ini_values(input, "Skirmish", &[("GameMode", "4")]);
        assert_eq!(out, b"[Skirmish]\r\n\xff\xfe\r\nGameMode=4\r\n");
    }
}
