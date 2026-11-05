//! VERA's hand-editable string-table format: a `[Strings]` section of
//! `Label=Text` lines.
//!
//! One format and one reader serve the compiled-in language files
//! (`crate::i18n`) and the optional CSF overlays
//! ([`crate::assets::csf_file::CsfFile::load_active`]). `;` starts a comment
//! line, `\n` inside a text is a line break, and labels are matched
//! case-insensitively. Unlike binary CSF records, the text is the displayed
//! text: no load-time whitespace normalisation is applied.

/// `Label=Text` pairs in file order, first occurrence first. Labels are
/// ASCII-uppercased and `\n` is expanded; a later duplicate replaces the
/// earlier value in place. A data line inside `[Strings]` without `=` is an
/// error naming its line, so a hand edit never disappears silently.
pub fn parse(text: &str) -> Result<Vec<(String, String)>, String> {
    let text = text.trim_start_matches('\u{FEFF}');
    let mut strings: Vec<(String, String)> = Vec::new();
    let mut in_strings = false;
    for (number, line) in text.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() || line.starts_with(';') {
            continue;
        }
        if let Some(section) = line.strip_prefix('[').and_then(|s| s.strip_suffix(']')) {
            in_strings = section.trim().eq_ignore_ascii_case("Strings");
            continue;
        }
        if !in_strings {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            return Err(format!(
                "line {}: expected Label=Text, found {line:?}",
                number + 1
            ));
        };
        let key = key.trim();
        if key.is_empty() {
            return Err(format!("line {}: the label is empty", number + 1));
        }
        let key = key.to_ascii_uppercase();
        let value = value.trim().replace("\\n", "\n");
        if let Some(existing) = strings.iter_mut().find(|(existing, _)| *existing == key) {
            existing.1 = value;
        } else {
            strings.push((key, value));
        }
    }
    Ok(strings)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn labels_keep_comments_and_last_duplicate_wins() {
        let parsed = parse(
            "; author notes\n[Strings]\nNAME:MTNK=first\nNAME:MTNK=last\n\
             TXT_FORMAT=%s $%d\nLINE=one\\ntwo\n",
        )
        .expect("well-formed table");
        assert_eq!(
            parsed,
            [
                ("NAME:MTNK".to_string(), "last".to_string()),
                ("TXT_FORMAT".to_string(), "%s $%d".to_string()),
                ("LINE".to_string(), "one\ntwo".to_string()),
            ]
        );
    }

    #[test]
    fn a_data_line_without_a_separator_names_its_line() {
        let error = parse("[Strings]\nGOOD=a\nNO_SEPARATOR\n").unwrap_err();
        assert!(error.contains("line 3"), "{error}");
        assert!(error.contains("NO_SEPARATOR"), "{error}");
    }
}
