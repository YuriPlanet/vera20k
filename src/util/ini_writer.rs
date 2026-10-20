//! In-place INI byte writer and remover.
//!
//! Updates/inserts selected `key=value` lines or removes selected keys under a
//! named `[section]` in the raw bytes of an INI file. Every untouched byte —
//! comments, key order, casing, blank lines, other sections, and existing line
//! endings — round-trips untouched. This mirrors the original game writing
//! individual settings keys in place rather than rewriting the whole file — a
//! naive full rewrite would discard sections the engine does not yet model
//! (e.g. `[Skirmish]`, `[MultiPlayer]`).
//!
//! Operates on raw bytes so content that is not valid UTF-8 elsewhere in the
//! file (e.g. a player name with high-byte characters in another section)
//! round-trips verbatim and is never matched.
//!
//! ## Dependency rules
//! - Part of util/ — no dependencies on game modules.

/// Update several keys in the first matching section in one traversal and one
/// output buffer.
///
/// Existing keys keep their position and line terminator. Missing keys are
/// appended after the last key in the section in `values` order; a missing
/// section is appended with every key in that same order. Matching remains
/// case-insensitive, while written key casing comes from `values`. Duplicate
/// update keys are collapsed case-insensitively: the last supplied spelling and
/// value win without changing the first occurrence's order. If the file itself
/// repeats a key, its first occurrence is updated: the one `IniFile` reads.
///
/// This is the preservation-safe path for native writers that update a whole
/// settings snapshot in memory before performing one final file save.
pub fn set_ini_values(content: &[u8], section: &str, values: &[(&str, &str)]) -> Vec<u8> {
    let bytes: Vec<_> = values
        .iter()
        .map(|(key, value)| (*key, value.as_bytes()))
        .collect();
    set_ini_byte_values(content, section, &bytes)
}

/// The same update transaction for native byte strings, without UTF-8 encoding.
/// Only section/key syntax is text; an existing value may contain any bytes.
pub fn set_ini_byte_values(content: &[u8], section: &str, values: &[(&str, &[u8])]) -> Vec<u8> {
    if values.is_empty() {
        return content.to_vec();
    }

    let target_section = section.trim();
    let mut updates: Vec<(&str, &[u8])> = Vec::with_capacity(values.len());
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
        let header = std::str::from_utf8(text).ok().and_then(section_header_name);
        if let Some(name) = header {
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
        // Parse only the ASCII key prefix: native values need not be UTF-8.
        let existing_key = key_name_bytes(text);
        if let Some(existing_key) = existing_key {
            if let Some(update_index) = updates
                .iter()
                .position(|update| update.0.eq_ignore_ascii_case(existing_key))
            {
                key_lines[update_index].get_or_insert(line_index);
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
            out.extend_from_slice(value);
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
                out.extend_from_slice(value);
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
            out.extend_from_slice(value);
            out.extend_from_slice(CRLF);
        }
    }

    out
}

/// Remove matching key lines from every occurrence of a named section.
///
/// Section and key matching is case-insensitive, like [`set_ini_values`]. The
/// complete matching line is removed, including any inline comment; separate
/// comment lines and every other byte retain their original order and line
/// ending. Invalid UTF-8 lines are left untouched.
pub fn remove_ini_keys(content: &[u8], section: &str, keys: &[&str]) -> Vec<u8> {
    if keys.is_empty() {
        return content.to_vec();
    }

    let target_section = section.trim();
    let mut in_target_section = false;
    let mut out = Vec::with_capacity(content.len());

    for (text, terminator) in split_lines(content) {
        let section_header = std::str::from_utf8(text).ok().and_then(section_header_name);
        if let Some(name) = section_header {
            in_target_section = name.eq_ignore_ascii_case(target_section);
            out.extend_from_slice(text);
            out.extend_from_slice(terminator);
            continue;
        }

        let remove = in_target_section
            && key_name_bytes(text)
                .is_some_and(|name| keys.iter().any(|key| name.eq_ignore_ascii_case(key.trim())));
        if !remove {
            out.extend_from_slice(text);
            out.extend_from_slice(terminator);
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

/// Identify an ASCII key from the prefix ending at `=` without decoding its
/// value or trailing comment, which may contain arbitrary bytes.
fn key_name_bytes(line: &[u8]) -> Option<&str> {
    let equals = line.iter().position(|byte| *byte == b'=')? + 1;
    std::str::from_utf8(&line[..equals]).ok().and_then(key_name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_byte_values_replace_existing_non_utf8_values() {
        let input = b"[Network]\r\nDestNet=1\xe9.2.3.4\r\nSocket=7\r\n[Other]\r\nHandle=\xff\r\n";
        let out = set_ini_byte_values(input, "Network", &[("DestNet", b"2\xff.3.4.5")]);
        assert_eq!(
            out,
            b"[Network]\r\nDestNet=2\xff.3.4.5\r\nSocket=7\r\n[Other]\r\nHandle=\xff\r\n"
        );
    }

    fn s(bytes: Vec<u8>) -> String {
        String::from_utf8(bytes).unwrap()
    }

    /// Real RA2MD.INI shape: replacing ScoreVolume keeps every sibling key and
    /// every other section byte-for-byte, drops the old value, and keeps CRLF.
    #[test]
    fn replaces_value_preserving_other_keys_and_sections() {
        let input = b"[Options]\r\nGameSpeed=3\r\n[Audio]\r\nSoundVolume=0.700000\r\n\
ScoreVolume=0.600000\r\nInGameMusic=yes\r\n[Network]\r\nNetID=ffff,ffff,ffff,\r\n";
        let out = s(set_ini_values(
            input,
            "Audio",
            &[("ScoreVolume", "0.250000")],
        ));
        assert!(out.contains("ScoreVolume=0.250000\r\n"));
        assert!(out.contains("SoundVolume=0.700000\r\n"));
        assert!(out.contains("InGameMusic=yes\r\n"));
        assert!(out.contains("[Network]\r\nNetID=ffff,ffff,ffff,\r\n"));
        assert!(!out.contains("0.600000"), "old value must be gone");
    }

    /// A file using LF endings keeps LF for the rewritten line.
    #[test]
    fn preserves_lf_line_endings() {
        let input = b"[Audio]\nScoreVolume=0.6\n";
        let out = s(set_ini_values(input, "Audio", &[("ScoreVolume", "0.3")]));
        assert_eq!(out, "[Audio]\nScoreVolume=0.3\n");
    }

    /// Section and key names match case-insensitively.
    #[test]
    fn matches_section_and_key_case_insensitively() {
        let input = b"[audio]\r\nscorevolume=0.6\r\n";
        let out = s(set_ini_values(input, "Audio", &[("ScoreVolume", "0.3")]));
        assert_eq!(out, "[audio]\r\nScoreVolume=0.3\r\n");
    }

    /// An unterminated final header line gets terminated before the inserted key.
    #[test]
    fn inserts_after_unterminated_header() {
        let out = s(set_ini_values(
            b"[Audio]",
            "Audio",
            &[("ScoreVolume", "0.4")],
        ));
        assert_eq!(out, "[Audio]\r\nScoreVolume=0.4\r\n");
    }

    /// With a duplicated section header, a missing key is appended into the
    /// FIRST matching block (Win32 first-section semantics), not the last.
    #[test]
    fn appends_missing_key_into_first_duplicate_section() {
        let input = b"[Audio]\r\nSoundVolume=0.7\r\n[Audio]\r\nMusicVolume=0.5\r\n";
        let out = s(set_ini_values(
            input,
            "Audio",
            &[("ScoreVolume", "0.250000")],
        ));
        assert_eq!(
            out,
            "[Audio]\r\nSoundVolume=0.7\r\nScoreVolume=0.250000\r\n\
[Audio]\r\nMusicVolume=0.5\r\n"
        );
    }

    /// A key appended into an LF-only file is written with CRLF (the original
    /// writer always emits CRLF for new lines); existing lines keep their LF.
    #[test]
    fn appended_key_uses_crlf_even_in_lf_file() {
        let input = b"[Audio]\nSoundVolume=0.7\n";
        let out = s(set_ini_values(input, "Audio", &[("ScoreVolume", "0.5")]));
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
    fn batch_updates_first_duplicate_key_in_first_section() {
        let input = b"[Skirmish]\r\nGameMode=1\r\nGameMode=8\r\n\
[Skirmish]\r\nGameMode=9\r\n";
        let out = s(set_ini_values(
            input,
            "Skirmish",
            &[("GameMode", "2"), ("Credits", "10000")],
        ));
        assert_eq!(
            out,
            "[Skirmish]\r\nGameMode=2\r\nGameMode=8\r\nCredits=10000\r\n\
[Skirmish]\r\nGameMode=9\r\n"
        );
    }

    #[test]
    fn batch_preserves_non_utf8_lines_verbatim() {
        let input = b"[Skirmish]\r\n\xff\xfe\r\nGameMode=1\r\n";
        let out = set_ini_values(input, "Skirmish", &[("GameMode", "4")]);
        assert_eq!(out, b"[Skirmish]\r\n\xff\xfe\r\nGameMode=4\r\n");
    }

    #[test]
    fn remove_keys_preserves_other_bytes_across_duplicate_sections() {
        let input = b"[VERA]\r\n; keep this comment\r\nRA2Look = yes ; remove inline comment too\r\nFullScreen=yes\r\n\xff\xfe\r\n[Other]\nRA2Look=keep in this section\n[vera]\nra2look=no\n# keep this comment\nShiftQueue=yes";
        let out = remove_ini_keys(input, "vera", &["RA2Look"]);

        assert_eq!(
            out,
            b"[VERA]\r\n; keep this comment\r\nFullScreen=yes\r\n\xff\xfe\r\n[Other]\nRA2Look=keep in this section\n[vera]\n# keep this comment\nShiftQueue=yes"
        );
    }

    #[test]
    fn remove_key_uses_ascii_identity_with_non_utf8_tails() {
        let input = b"[VERA]\r\nRA2Look=yes ; \xff inline comment\r\nra2look=no\xff\r\nYROriginalStyle=no\r\n[Other]\r\nRA2Look=keep\r\n[VERA]suffix\r\nRA2Look=keep under malformed header\r\nYROriginalStyle=no";
        let out = remove_ini_keys(input, "VERA", &["RA2Look"]);

        assert_eq!(
            out,
            b"[VERA]\r\nYROriginalStyle=no\r\n[Other]\r\nRA2Look=keep\r\n[VERA]suffix\r\nRA2Look=keep under malformed header\r\nYROriginalStyle=no"
        );
    }
}
