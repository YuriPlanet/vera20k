//! VERA's own localized text: the `vera20k.<language>.ini` files beside this
//! module, compiled into the binary.
//!
//! `[VERA] Language=` (VERA.INI, default `en`) selects one file for the whole
//! process at startup; changing it takes a restart. The files sit behind the
//! retail `ra2md.csf`: a label the CSF has keeps its CSF text, and the
//! selected file answers only labels the CSF lacks, such as VERA's own
//! `VERA:` labels. English fills labels that neither the CSF nor the
//! selected file has, which keeps them readable in a half-translated language.
//!
//! File format, UTF-8: a `[Strings]` section of `Label=Text` lines; `;`
//! starts a comment line, `\n` in a text is a line break, and `{name}` marks
//! a value the caller fills in through [`fill`].

use std::borrow::Cow;
use std::collections::HashMap;
use std::sync::OnceLock;

use crate::assets::csf_file::CsfFile;

/// Language tags with a compiled-in file. The first is the default and the
/// fallback for every label.
const LANGUAGES: &[(&str, &str)] = &[
    ("en", include_str!("vera20k.en.ini")),
    ("zh-CN", include_str!("vera20k.zh-CN.ini")),
];

pub const DEFAULT_LANGUAGE: &str = "en";

static SELECTED: OnceLock<usize> = OnceLock::new();

/// Select the process language once, before the first CSF load. An unknown
/// tag logs a warning and keeps English. Returns the tag that took effect.
pub fn select_language(tag: &str) -> &'static str {
    let index = language_index(tag).unwrap_or_else(|| {
        log::warn!("[VERA] Language={tag} has no string file; using {DEFAULT_LANGUAGE}");
        0
    });
    let index = *SELECTED.get_or_init(|| index);
    LANGUAGES[index].0
}

/// The process language's tag, such as `en` or `zh-CN`.
pub fn selected_language() -> &'static str {
    LANGUAGES[selected_index()].0
}

fn language_index(tag: &str) -> Option<usize> {
    LANGUAGES
        .iter()
        .position(|(name, _)| name.eq_ignore_ascii_case(tag.trim()))
}

fn table(index: usize) -> &'static HashMap<String, String> {
    static TABLES: OnceLock<Vec<HashMap<String, String>>> = OnceLock::new();
    &TABLES.get_or_init(|| LANGUAGES.iter().map(|(_, text)| parse(text)).collect())[index]
}

fn selected_index() -> usize {
    SELECTED.get().copied().unwrap_or(0)
}

fn parse(text: &str) -> HashMap<String, String> {
    let mut strings = HashMap::new();
    let mut in_strings = false;
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with(';') {
            continue;
        }
        if let Some(section) = line.strip_prefix('[').and_then(|s| s.strip_suffix(']')) {
            in_strings = section.trim().eq_ignore_ascii_case("Strings");
            continue;
        }
        if let (true, Some((key, value))) = (in_strings, line.split_once('=')) {
            strings.insert(
                key.trim().to_ascii_uppercase(),
                value.trim().replace("\\n", "\n"),
            );
        }
    }
    strings
}

/// Put the selected language behind a freshly loaded CSF. Called by the one
/// CSF loader, so every retail and VERA lookup sees the same text.
pub fn apply(csf: &mut CsfFile) {
    apply_language(csf, selected_index());
}

fn apply_language(csf: &mut CsfFile, index: usize) {
    // English first, so a translation replaces it label by label. Both stay
    // behind the loaded table, which answers first at lookup.
    for (key, value) in table(0) {
        csf.set_fallback(key, value);
    }
    if index != 0 {
        for (key, value) in table(index) {
            csf.set_fallback(key, value);
        }
    }
}

/// Text for a VERA label. Without a loaded CSF (startup failure screens) the
/// selected file and then English answer directly.
pub fn text<'a>(csf: Option<&'a CsfFile>, key: &str) -> Cow<'a, str> {
    if let Some(csf) = csf {
        return csf.text(key);
    }
    let key = key.to_ascii_uppercase();
    [selected_index(), 0]
        .into_iter()
        .find_map(|index| table(index).get(&key))
        .map(|value| Cow::Owned(value.clone()))
        .unwrap_or_else(|| Cow::Owned(format!("MISSING:'{key}'")))
}

/// Replace each `{name}` in a localized text with its value.
pub fn fill(text: &str, values: &[(&str, &dyn std::fmt::Display)]) -> String {
    values.iter().fold(text.to_owned(), |text, (name, value)| {
        text.replace(&format!("{{{name}}}"), &value.to_string())
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::assets::csf_file::test_csf;

    #[test]
    fn csf_text_wins_and_language_files_fill_only_gaps() {
        let zh = language_index("zh-cn").unwrap();
        let mut csf = test_csf(&[("GUI:Ok", "OK"), ("VERA:Pausing", "csf pausing")]);
        apply_language(&mut csf, zh);
        // A label the CSF has keeps its CSF text, even when the selected file has it.
        assert_eq!(csf.text("VERA:Pausing"), "csf pausing");
        assert_eq!(csf.text("GUI:Ok"), "OK");
        // The selected file answers a label the CSF lacks, ahead of English.
        assert_eq!(csf.text("VERA:Resuming"), table(zh)["VERA:RESUMING"]);

        let mut csf = test_csf(&[("VERA:Pausing", "csf pausing")]);
        apply_language(&mut csf, 0);
        assert_eq!(csf.text("VERA:Pausing"), "csf pausing");
        assert_eq!(csf.text("VERA:Resuming"), "Resuming...");
    }

    #[test]
    fn every_translated_vera_label_exists_in_english() {
        for index in 1..LANGUAGES.len() {
            for key in table(index).keys().filter(|key| key.starts_with("VERA:")) {
                assert!(table(0).contains_key(key), "{}: {key}", LANGUAGES[index].0);
            }
        }
    }
}
