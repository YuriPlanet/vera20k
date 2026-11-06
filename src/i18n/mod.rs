//! VERA's interface text in the player's language.
//!
//! Every text VERA draws itself comes from a language file,
//! `vera20k.<tag>.ini`, where `<tag>` is a BCP 47 language tag such as `en`,
//! `zh-CN` or `pt-BR`. The files beside this module ship inside VERA (build.rs
//! lists them, so shipping a language is adding its file); a file of that
//! name in the settings folder adds a language, or replaces a shipped one
//! while it is being written. `vera20k --check-language <file>` lists what a
//! file leaves untranslated ([`check`]).
//!
//! The format, UTF-8:
//!
//! ```ini
//! [Language]
//! Name=Deutsch                  ; how the Language list shows it
//!
//! [Strings]
//! VERA:SaveSetup=Setup speichern
//! VERA:CatchingUp[one]=Holt auf - {count} Frame zurück
//! VERA:CatchingUp[other]=Holt auf - {count} Frames zurück
//! GUI:Ok=OK                     ; one of the game's own labels
//! ```
//!
//! A line starting with `;` is a comment. `\n` in a text is a line break, and
//! `{name}` marks a value VERA fills in, in whatever order the sentence needs.
//! A text with a count gives one line per CLDR plural form the language uses
//! (`zero`, `one`, `two`, `few`, `many`, `other`); `{count}` is the count.
//!
//! English holds every VERA label and answers any label the chosen language
//! lacks. A label that is not VERA's own (`GUI:Ok`, `NAME:MTNK`) translates
//! the game's string table: the chosen language's text replaces the game
//! files' ([`apply`]), for players whose game files are in another language.
//!
//! Until the player picks a language (`[VERA] Language=`), VERA follows the
//! system's preferred languages. The pseudo-language [`PSEUDO_LANGUAGE`]
//! rewrites and lengthens every text, VERA's and the game's, so a screen
//! shows any text that does not come from a language file or does not fit.

use std::collections::{BTreeSet, HashMap};
use std::fmt::{self, Display, Write as _};
use std::path::{Path, PathBuf};
use std::sync::{Arc, LazyLock, PoisonError, RwLock};

use icu_locale_core::LanguageIdentifier;
use icu_plurals::{PluralCategory, PluralOperands, PluralRules};

use crate::assets::csf_file::CsfFile;

mod shipped {
    include!(concat!(env!("OUT_DIR"), "/shipped_languages.rs"));
}

pub const DEFAULT_LANGUAGE: &str = "en";

/// The pseudo-language: every text with its letters swapped for accented
/// look-alikes, lengthened and bracketed. A screen in it shows untranslated
/// text as plain English and clipped text as a missing `]`.
pub const PSEUDO_LANGUAGE: &str = "qps-ploc";

/// The plural forms a language file can give, in CLDR order.
const FORMS: [(PluralCategory, &str); 6] = [
    (PluralCategory::Zero, "zero"),
    (PluralCategory::One, "one"),
    (PluralCategory::Two, "two"),
    (PluralCategory::Few, "few"),
    (PluralCategory::Many, "many"),
    (PluralCategory::Other, "other"),
];

fn form_name(category: PluralCategory) -> &'static str {
    FORMS
        .iter()
        .find(|(form, _)| *form == category)
        .map_or("other", |(_, name)| name)
}

/// One language: its file's texts.
struct Language {
    tag: String,
    name: String,
    /// Upper-case label, or `LABEL[form]` for a plural form, to text.
    strings: HashMap<String, String>,
}

impl Language {
    fn from_file(tag: &str, text: &str) -> Self {
        let contents = parse(text);
        Self {
            tag: tag.to_owned(),
            name: contents.name.unwrap_or_else(|| tag.to_owned()),
            strings: contents
                .entries
                .into_iter()
                .map(|entry| (entry.key(), entry.text))
                .collect(),
        }
    }

    /// The pseudo-language, made from English.
    fn pseudo(english: &Language) -> Self {
        Self {
            tag: PSEUDO_LANGUAGE.to_owned(),
            name: pseudo("Pseudo"),
            strings: english
                .strings
                .iter()
                .map(|(key, text)| (key.clone(), pseudo(text)))
                .collect(),
        }
    }

    fn is_pseudo(&self) -> bool {
        self.tag == PSEUDO_LANGUAGE
    }

    /// The text for `key` with `count`: the count's form, then `other`, then
    /// the plain label.
    fn plural(&self, key: &str, count: PluralOperands) -> Option<&str> {
        // The pseudo-language counts as English does. The rules are made per
        // count: ICU4X's are not shareable between threads.
        let tag = if self.is_pseudo() {
            DEFAULT_LANGUAGE
        } else {
            &self.tag
        };
        let category = plural_rules(tag).category_for(count);
        [category, PluralCategory::Other]
            .into_iter()
            .find_map(|form| self.strings.get(&format!("{key}[{}]", form_name(form))))
            .or_else(|| self.strings.get(key))
            .map(String::as_str)
    }
}

fn plural_rules(tag: &str) -> PluralRules {
    let id = LanguageIdentifier::try_from_str(tag).unwrap_or(LanguageIdentifier::UNKNOWN);
    PluralRules::try_new_cardinal((&id).into())
        .or_else(|_| PluralRules::try_new_cardinal((&LanguageIdentifier::UNKNOWN).into()))
        .expect("compiled plural rules hold the root locale")
}

/// The languages and the one in use. English is always first.
struct Catalog {
    languages: Vec<Arc<Language>>,
    selected: Arc<Language>,
}

impl Catalog {
    fn new(languages: Vec<Arc<Language>>) -> Self {
        let selected = languages[0].clone();
        Self {
            languages,
            selected,
        }
    }

    fn english(&self) -> &Language {
        &self.languages[0]
    }

    fn find(&self, tag: &str) -> Option<&Arc<Language>> {
        let tag = tag.trim();
        self.languages
            .iter()
            .find(|language| language.tag.eq_ignore_ascii_case(tag))
    }

    fn text(&self, key: &str) -> Option<&str> {
        let key = key.to_ascii_uppercase();
        [&*self.selected, self.english()]
            .into_iter()
            .find_map(|language| language.strings.get(&key))
            .map(String::as_str)
    }

    fn plural(&self, key: &str, count: PluralOperands) -> Option<&str> {
        let key = key.to_ascii_uppercase();
        self.selected
            .plural(&key, count)
            .or_else(|| self.english().plural(&key, count))
    }
}

static CATALOG: LazyLock<RwLock<Catalog>> =
    LazyLock::new(|| RwLock::new(Catalog::new(languages(None))));

fn catalog() -> std::sync::RwLockReadGuard<'static, Catalog> {
    CATALOG.read().unwrap_or_else(PoisonError::into_inner)
}

/// The shipped languages, those in `settings_dir`, and the pseudo-language.
fn languages(settings_dir: Option<&Path>) -> Vec<Arc<Language>> {
    let mut languages: Vec<Arc<Language>> = shipped::FILES
        .iter()
        .map(|(tag, text)| Arc::new(Language::from_file(tag, text)))
        .collect();
    if let Some(dir) = settings_dir {
        for (tag, path) in settings_files(dir) {
            let text = match std::fs::read(&path).map(String::from_utf8) {
                Ok(Ok(text)) => text,
                Ok(Err(_)) => {
                    log::warn!("{} is not UTF-8 text; skipped", path.display());
                    continue;
                }
                Err(error) => {
                    log::warn!("Could not read {}: {error}", path.display());
                    continue;
                }
            };
            // The log tells a translator what the file still lacks.
            let name = path
                .file_name()
                .map(|name| name.to_string_lossy().into_owned());
            let report = check(name.as_deref().unwrap_or_default(), &text);
            if !report.complete() {
                log::info!("{report}");
            }
            let language = Arc::new(Language::from_file(&tag, &text));
            match languages
                .iter()
                .position(|known| known.tag.eq_ignore_ascii_case(&tag))
            {
                Some(index) => {
                    log::info!("vera20k.{tag}.ini in the settings folder replaces the shipped one");
                    languages[index] = language;
                }
                None => {
                    log::info!(
                        "Language {tag} ({}) from the settings folder",
                        language.name
                    );
                    languages.push(language);
                }
            }
        }
    }
    let pseudo = Language::pseudo(&languages[0]);
    languages.push(Arc::new(pseudo));
    languages
}

/// `vera20k.<tag>.ini` files in `dir`, by tag.
fn settings_files(dir: &Path) -> Vec<(String, PathBuf)> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut files: Vec<(String, PathBuf)> = entries
        .filter_map(Result::ok)
        .filter_map(|entry| {
            let name = entry.file_name().into_string().ok()?;
            let tag = file_tag(&name)?;
            if tag.eq_ignore_ascii_case(PSEUDO_LANGUAGE) {
                return None;
            }
            LanguageIdentifier::try_from_str(tag).ok()?;
            Some((tag.to_owned(), entry.path()))
        })
        .collect();
    files.sort();
    files
}

/// The tag in a language file's name, `vera20k.<tag>.ini`.
fn file_tag(name: &str) -> Option<&str> {
    let lower = name.to_ascii_lowercase();
    if !lower.starts_with("vera20k.") || !lower.ends_with(".ini") {
        return None;
    }
    let tag = &name["vera20k.".len()..name.len() - ".ini".len()];
    (!tag.is_empty()).then_some(tag)
}

/// Load the shipped languages and those in `settings_dir`, and use `chosen`
/// (`[VERA] Language=`), or the best match for the system's preferred
/// languages when the player has not chosen one or it is gone. Returns the
/// tag in use.
pub fn init(settings_dir: Option<&Path>, chosen: Option<&str>) -> String {
    let mut catalog = Catalog::new(languages(settings_dir));
    let chosen_language = chosen.and_then(|tag| {
        let found = catalog.find(tag).cloned();
        if found.is_none() {
            log::warn!("[VERA] Language={tag} has no language file; following the system");
        }
        found
    });
    catalog.selected = chosen_language.unwrap_or_else(|| {
        let tags: Vec<&str> = catalog
            .languages
            .iter()
            .filter(|language| !language.is_pseudo())
            .map(|language| language.tag.as_str())
            .collect();
        let preferred: Vec<String> = sys_locale::get_locales().collect();
        let index = best_match(&tags, preferred.iter().map(String::as_str)).unwrap_or(0);
        catalog.languages[index].clone()
    });
    let tag = catalog.selected.tag.clone();
    log::info!("Interface language: {tag} ({})", catalog.selected.name);
    *CATALOG.write().unwrap_or_else(PoisonError::into_inner) = catalog;
    tag
}

/// Use `tag`'s language from now on. False when there is no such language.
/// The caller re-applies it to the loaded string table ([`apply`]).
pub fn select(tag: &str) -> bool {
    let mut catalog = CATALOG.write().unwrap_or_else(PoisonError::into_inner);
    let Some(language) = catalog.find(tag).cloned() else {
        return false;
    };
    log::info!("Interface language: {} ({})", language.tag, language.name);
    catalog.selected = language;
    true
}

/// The language in use's tag, such as `en` or `zh-CN`.
pub fn selected_language() -> String {
    catalog().selected.tag.clone()
}

/// A language the player can choose.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LanguageChoice {
    pub tag: String,
    /// The language's name in itself, such as `简体中文`.
    pub name: String,
}

/// The languages the player can choose, English first and the rest by tag.
pub fn choices() -> Vec<LanguageChoice> {
    let catalog = catalog();
    let mut choices: Vec<LanguageChoice> = catalog
        .languages
        .iter()
        .filter(|language| !language.is_pseudo())
        .map(|language| LanguageChoice {
            tag: language.tag.clone(),
            name: language.name.clone(),
        })
        .collect();
    choices[1..].sort_by(|a, b| a.tag.to_ascii_lowercase().cmp(&b.tag.to_ascii_lowercase()));
    choices
}

/// The index in `available` of the best language for `preferred` (most
/// preferred first): the first preference any language shares a language
/// with, and among those the one that also shares its script, then region.
fn best_match<'a>(available: &[&str], preferred: impl Iterator<Item = &'a str>) -> Option<usize> {
    let available: Vec<Option<Subtags>> = available.iter().map(|tag| Subtags::of(tag)).collect();
    preferred.filter_map(Subtags::of).find_map(|wanted| {
        available
            .iter()
            .enumerate()
            .filter_map(|(index, have)| Some((index, have.as_ref()?.closeness(&wanted)?)))
            .max_by_key(|&(index, closeness)| (closeness, std::cmp::Reverse(index)))
            .map(|(index, _)| index)
    })
}

/// A tag's language, script and region, with the script Chinese implies.
struct Subtags {
    language: String,
    script: Option<String>,
    region: Option<String>,
}

impl Subtags {
    fn of(tag: &str) -> Option<Self> {
        // POSIX names such as zh_CN.UTF-8 reach here from some systems.
        let tag = tag.split(['.', '@']).next()?.replace('_', "-");
        let id = LanguageIdentifier::try_from_str(&tag).ok()?;
        let language = id.language.as_str().to_owned();
        let region = id.region.map(|region| region.as_str().to_owned());
        let script = id
            .script
            .map(|script| script.as_str().to_owned())
            .or_else(|| {
                (language == "zh").then(|| {
                    match region.as_deref() {
                        Some("TW" | "HK" | "MO") => "Hant",
                        _ => "Hans",
                    }
                    .to_owned()
                })
            });
        (language != "und").then_some(Self {
            language,
            script,
            region,
        })
    }

    /// How close `self`, an available language, is to `wanted`: `None`
    /// across languages; otherwise more for the same script, then region.
    fn closeness(&self, wanted: &Subtags) -> Option<u8> {
        (self.language == wanted.language).then(|| {
            let script =
                self.script.is_none() || wanted.script.is_none() || self.script == wanted.script;
            u8::from(script) * 2 + u8::from(self.region.is_some() && self.region == wanted.region)
        })
    }
}

/// Put the language in use over a loaded string table: its texts answer
/// ahead of the game's, and English answers labels neither has. Called by the
/// one CSF loader and after a language change.
pub fn apply(csf: &mut CsfFile) {
    apply_catalog(&catalog(), csf);
}

fn apply_catalog(catalog: &Catalog, csf: &mut CsfFile) {
    csf.clear_localization();
    let single = |(key, _): &(&String, &String)| !key.ends_with(']');
    for (key, text) in catalog.english().strings.iter().filter(single) {
        csf.set_fallback(key, text);
    }
    if catalog.selected.is_pseudo() {
        let game: Vec<(String, String)> = csf
            .entries()
            .map(|(key, text)| (key.to_owned(), pseudo(text)))
            .collect();
        for (key, text) in game {
            csf.set_override(&key, &text);
        }
    }
    for (key, text) in catalog.selected.strings.iter().filter(single) {
        csf.set_override(key, text);
    }
}

/// VERA's text for `key` in the language in use.
pub fn tr(key: &str) -> String {
    catalog()
        .text(key)
        .map_or_else(|| missing(key), str::to_owned)
}

/// [`tr`] with each `{name}` filled in.
pub fn tr_fill(key: &str, values: &[(&str, &dyn Display)]) -> String {
    fill(&tr(key), values)
}

/// The text for `key` in the plural form `count` takes in the language in
/// use, with `{count}` and each `{name}` filled in.
pub fn tr_count<N>(key: &str, count: N, values: &[(&str, &dyn Display)]) -> String
where
    N: Into<PluralOperands> + Display + Copy,
{
    let text = catalog()
        .plural(key, count.into())
        .map_or_else(|| missing(key), str::to_owned);
    fill(&fill(&text, &[("count", &count)]), values)
}

fn missing(key: &str) -> String {
    format!("MISSING:'{key}'")
}

/// Replace each `{name}` in a localized text with its value.
pub fn fill(text: &str, values: &[(&str, &dyn Display)]) -> String {
    values.iter().fold(text.to_owned(), |text, (name, value)| {
        text.replace(&format!("{{{name}}}"), &value.to_string())
    })
}

/// `text` in the pseudo-language: letters swapped for accented look-alikes,
/// lengthened by a run of tildes and bracketed, with `{name}` values,
/// `%`-formats and line breaks kept as they are. A blank text stays blank,
/// and a single letter, such as a team's, is not lengthened.
pub fn pseudo(text: &str) -> String {
    if text.trim().is_empty() {
        return text.to_owned();
    }
    const LOWER: [char; 26] = [
        'å', 'ƀ', 'ç', 'ð', 'é', 'ƒ', 'ĝ', 'ĥ', 'î', 'ĵ', 'ķ', 'ļ', 'ɱ', 'ñ', 'ö', 'þ', 'ǫ', 'ŕ',
        'š', 'ţ', 'û', 'ṽ', 'ŵ', 'ẋ', 'ý', 'ž',
    ];
    const UPPER: [char; 26] = [
        'Å', 'Ɓ', 'Ç', 'Ð', 'É', 'Ƒ', 'Ĝ', 'Ĥ', 'Î', 'Ĵ', 'Ķ', 'Ļ', 'Ṁ', 'Ñ', 'Ö', 'Þ', 'Ǫ', 'Ŕ',
        'Š', 'Ţ', 'Û', 'Ṽ', 'Ŵ', 'Ẋ', 'Ý', 'Ž',
    ];
    let mut out = String::with_capacity(text.len() * 2 + 2);
    out.push('[');
    let mut letters = 0usize;
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '{' => {
                out.push(c);
                for c in chars.by_ref() {
                    out.push(c);
                    if c == '}' {
                        break;
                    }
                }
            }
            '%' => {
                // printf conversions, as the game's texts use them: %s, %d,
                // %-12s, %.2f, %ls, %%.
                out.push(c);
                while let Some(&c) = chars.peek() {
                    out.push(c);
                    chars.next();
                    if !(c.is_ascii_digit() || "-+ #.lhz$".contains(c)) {
                        break;
                    }
                }
            }
            'a'..='z' => {
                out.push(LOWER[(c as u8 - b'a') as usize]);
                letters += 1;
            }
            'A'..='Z' => {
                out.push(UPPER[(c as u8 - b'A') as usize]);
                letters += 1;
            }
            _ => out.push(c),
        }
    }
    // About a third longer, as German or Russian often runs; half again for
    // short texts, which grow most in translation.
    let extra = if letters <= 1 {
        0
    } else if letters <= 10 {
        letters.div_ceil(2)
    } else {
        letters.div_ceil(3)
    };
    if extra > 0 {
        out.push(' ');
        for index in 0..extra {
            out.push(if index % 6 == 5 { ' ' } else { '~' });
        }
    }
    out.push(']');
    out
}

/// A language file's lines.
struct FileContents {
    name: Option<String>,
    entries: Vec<Entry>,
    problems: Vec<String>,
}

struct Entry {
    line: usize,
    label: String,
    form: Option<PluralCategory>,
    text: String,
}

impl Entry {
    fn key(&self) -> String {
        let mut key = self.label.to_ascii_uppercase();
        if let Some(form) = self.form {
            let _ = write!(key, "[{}]", form_name(form));
        }
        key
    }
}

fn parse(text: &str) -> FileContents {
    #[derive(PartialEq)]
    enum Section {
        None,
        Language,
        Strings,
        Unknown,
    }
    let mut contents = FileContents {
        name: None,
        entries: Vec::new(),
        problems: Vec::new(),
    };
    let mut section = Section::None;
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    for (index, line) in text.lines().enumerate() {
        let number = index + 1;
        let line = line.trim();
        if line.is_empty() || line.starts_with(';') {
            continue;
        }
        if let Some(name) = line
            .strip_prefix('[')
            .and_then(|line| line.strip_suffix(']'))
        {
            section = match name.trim() {
                name if name.eq_ignore_ascii_case("Language") => Section::Language,
                name if name.eq_ignore_ascii_case("Strings") => Section::Strings,
                name => {
                    contents
                        .problems
                        .push(format!("line {number}: unknown section [{name}]"));
                    Section::Unknown
                }
            };
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            contents
                .problems
                .push(format!("line {number}: not Label=Text: {line}"));
            continue;
        };
        let (key, text) = (key.trim(), value.trim().replace("\\n", "\n"));
        match section {
            Section::Language if key.eq_ignore_ascii_case("Name") => contents.name = Some(text),
            Section::Language => contents.problems.push(format!(
                "line {number}: [Language] has only Name=, not {key}="
            )),
            Section::Strings => {
                let (label, form) = match key.strip_suffix(']').and_then(|key| key.split_once('['))
                {
                    Some((label, form)) => {
                        let Some(&(category, _)) = FORMS
                            .iter()
                            .find(|(_, name)| name.eq_ignore_ascii_case(form.trim()))
                        else {
                            contents.problems.push(format!(
                                "line {number}: [{form}] is not a plural form; use zero, one, two, few, many or other"
                            ));
                            continue;
                        };
                        (label.trim(), Some(category))
                    }
                    None => (key, None),
                };
                if label.is_empty() {
                    contents
                        .problems
                        .push(format!("line {number}: a text without a label"));
                    continue;
                }
                contents.entries.push(Entry {
                    line: number,
                    label: label.to_owned(),
                    form,
                    text,
                });
            }
            Section::None => contents
                .problems
                .push(format!("line {number}: a text before [Strings]")),
            Section::Unknown => {}
        }
    }
    contents
}

/// The `{name}` values a text fills in.
fn placeholders(text: &str) -> BTreeSet<&str> {
    text.split('{')
        .skip(1)
        .filter_map(|part| part.split_once('}').map(|(name, _)| name))
        .filter(|name| {
            !name.is_empty() && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
        })
        .collect()
}

/// What a language file leaves untranslated or gets wrong, against the
/// English VERA ships ([`check`]).
pub struct CheckReport {
    file: String,
    tag: Option<String>,
    name: Option<String>,
    forms: Vec<&'static str>,
    labels: usize,
    untranslated: Vec<(String, String)>,
    missing_forms: Vec<String>,
    placeholders: Vec<String>,
    unknown: Vec<String>,
    game_labels: usize,
    problems: Vec<String>,
}

impl CheckReport {
    /// Nothing untranslated and nothing wrong.
    pub fn complete(&self) -> bool {
        self.tag.is_some()
            && self.untranslated.is_empty()
            && self.missing_forms.is_empty()
            && self.placeholders.is_empty()
            && self.unknown.is_empty()
            && self.problems.is_empty()
    }
}

impl Display for CheckReport {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let Some(tag) = &self.tag else {
            return writeln!(
                f,
                "{}: name a language file vera20k.<tag>.ini, with a language tag such as de, pt-BR or zh-CN",
                self.file
            );
        };
        writeln!(
            f,
            "{}: {} ({tag}), plural forms {}",
            self.file,
            self.name.as_deref().unwrap_or("no [Language] Name="),
            self.forms.join(", ")
        )?;
        let translated = self.labels - self.untranslated.len();
        writeln!(f, "{translated} of {} VERA texts translated", self.labels)?;
        if self.game_labels > 0 {
            writeln!(f, "{} of the game's own texts translated", self.game_labels)?;
        }
        let mut list = |heading: &str, lines: &mut dyn Iterator<Item = String>| -> fmt::Result {
            let lines: Vec<String> = lines.collect();
            if !lines.is_empty() {
                writeln!(f, "\n{heading} ({}):", lines.len())?;
                for line in lines {
                    writeln!(f, "  {line}")?;
                }
            }
            Ok(())
        };
        list(
            "Untranslated, with the English",
            &mut self
                .untranslated
                .iter()
                .map(|(label, english)| format!("{label}={}", english.replace('\n', "\\n"))),
        )?;
        list(
            "Plural forms missing",
            &mut self.missing_forms.iter().cloned(),
        )?;
        list(
            "Values that differ from the English",
            &mut self.placeholders.iter().cloned(),
        )?;
        list(
            "VERA labels English does not have (misspelt, or no longer used)",
            &mut self.unknown.iter().cloned(),
        )?;
        list("Lines not understood", &mut self.problems.iter().cloned())?;
        if self.complete() {
            writeln!(f, "\nComplete.")?;
        }
        Ok(())
    }
}

/// Check the language file `file_name` holding `text` against the English
/// VERA ships: what it leaves untranslated, which plural forms its language
/// needs and it lacks, `{values}` that differ from the English, labels VERA
/// does not have and lines it cannot read.
pub fn check(file_name: &str, text: &str) -> CheckReport {
    let english = parse(shipped::FILES[0].1);
    let tag = file_tag(file_name)
        .filter(|tag| LanguageIdentifier::try_from_str(tag).is_ok())
        .map(str::to_owned);
    let contents = parse(text);
    let rules = plural_rules(tag.as_deref().unwrap_or(DEFAULT_LANGUAGE));
    let forms: Vec<PluralCategory> = rules.categories().collect();
    let mut report = CheckReport {
        file: file_name.to_owned(),
        tag,
        name: contents.name.clone(),
        forms: forms.iter().map(|&form| form_name(form)).collect(),
        labels: 0,
        untranslated: Vec::new(),
        missing_forms: Vec::new(),
        placeholders: Vec::new(),
        unknown: Vec::new(),
        game_labels: 0,
        problems: contents.problems,
    };
    // Each label once, in the English file's order, with its texts.
    let mut english_labels: Vec<(String, String, Vec<&Entry>)> = Vec::new();
    for entry in &english.entries {
        let upper = entry.label.to_ascii_uppercase();
        match english_labels
            .iter_mut()
            .find(|(label, _, _)| label.to_ascii_uppercase() == upper)
        {
            Some((_, _, entries)) => entries.push(entry),
            None => english_labels.push((entry.label.clone(), upper, vec![entry])),
        }
    }
    let mut translated: HashMap<String, Vec<&Entry>> = HashMap::new();
    for entry in &contents.entries {
        translated
            .entry(entry.label.to_ascii_uppercase())
            .or_default()
            .push(entry);
    }
    report.labels = english_labels.len();
    for (label, upper, english_entries) in &english_labels {
        let Some(entries) = translated.get(upper) else {
            let english_text = english_entries
                .iter()
                .find(|entry| entry.form.is_none_or(|form| form == PluralCategory::Other))
                .map_or(String::new(), |entry| entry.text.clone());
            report.untranslated.push((label.clone(), english_text));
            continue;
        };
        let plural = english_entries.iter().any(|entry| entry.form.is_some());
        if plural {
            let plain = entries.iter().any(|entry| entry.form.is_none());
            for &form in &forms {
                let given = entries.iter().any(|entry| entry.form == Some(form));
                if !given && !(plain && forms == [PluralCategory::Other]) {
                    report
                        .missing_forms
                        .push(format!("{label}[{}]", form_name(form)));
                }
            }
        }
        let wanted: BTreeSet<&str> = english_entries
            .iter()
            .flat_map(|entry| placeholders(&entry.text))
            .filter(|name| !plural || *name != "count")
            .collect();
        for entry in entries {
            let have = placeholders(&entry.text);
            let lacking: Vec<&str> = wanted.difference(&have).copied().collect();
            let extra: Vec<&str> = have
                .iter()
                .copied()
                .filter(|name| !wanted.contains(name) && !(plural && *name == "count"))
                .collect();
            if !lacking.is_empty() || !extra.is_empty() {
                let mut line = format!("line {}: {}", entry.line, entry.key());
                for name in lacking {
                    let _ = write!(line, ", needs {{{name}}}");
                }
                for name in extra {
                    let _ = write!(line, ", English has no {{{name}}}");
                }
                report.placeholders.push(line);
            }
        }
    }
    for (upper, entries) in &translated {
        let known = english_labels.iter().any(|(_, label, _)| label == upper);
        if known {
            continue;
        }
        if upper.starts_with("VERA:") {
            report
                .unknown
                .push(format!("line {}: {}", entries[0].line, entries[0].label));
        } else {
            report.game_labels += 1;
        }
    }
    report.unknown.sort();
    report
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::assets::csf_file::test_csf;

    fn language(tag: &str, text: &str) -> Arc<Language> {
        Arc::new(Language::from_file(tag, text))
    }

    fn catalog_with(selected: &str, file: &str) -> Catalog {
        let english = language("en", shipped::FILES[0].1);
        let mut catalog = Catalog::new(vec![english, language(selected, file)]);
        catalog.selected = catalog.languages[1].clone();
        catalog
    }

    #[test]
    fn the_chosen_language_translates_the_game_and_english_fills_gaps() {
        let file = "[Strings]\nGUI:Ok=确定\nVERA:Pausing=正在暂停...\n";
        let catalog = catalog_with("zh-CN", file);
        let mut csf = test_csf(&[("GUI:Ok", "OK"), ("GUI:Cancel", "Cancel")]);
        apply_catalog(&catalog, &mut csf);
        // The chosen language replaces the game's own text...
        assert_eq!(csf.text("GUI:Ok"), "确定");
        assert_eq!(csf.text("GUI:Cancel"), "Cancel");
        assert_eq!(csf.text("VERA:Pausing"), "正在暂停...");
        // ...and English answers what it lacks.
        assert_eq!(csf.text("VERA:Resuming"), "Resuming...");
        assert_eq!(catalog.text("VERA:Resuming"), Some("Resuming..."));
    }

    #[test]
    fn plural_forms_follow_the_language_rules() {
        let english = "[Strings]\nVERA:Behind[one]={count} frame behind\nVERA:Behind[other]={count} frames behind\n";
        let catalog = Catalog::new(vec![language("en", english)]);
        let text = |count: u32| catalog.plural("VERA:BEHIND", count.into()).unwrap();
        assert_eq!(text(1), "{count} frame behind");
        assert_eq!(text(2), "{count} frames behind");

        let russian = "[Strings]\nVERA:Behind[one]=один\nVERA:Behind[few]=несколько\nVERA:Behind[many]=много\n";
        let mut catalog = Catalog::new(vec![language("en", english), language("ru", russian)]);
        catalog.selected = catalog.languages[1].clone();
        let text = |count: u32| catalog.plural("VERA:BEHIND", count.into()).unwrap();
        assert_eq!(
            [text(1), text(3), text(5), text(21)],
            ["один", "несколько", "много", "один"]
        );
    }

    #[test]
    fn the_system_language_picks_the_closest_file() {
        let available = ["en", "zh-CN", "pt-BR", "pt-PT"];
        let pick = |preferred: &[&str]| best_match(&available, preferred.iter().copied());
        assert_eq!(pick(&["zh-Hans-CN"]), Some(1));
        assert_eq!(pick(&["zh_CN.UTF-8"]), Some(1));
        assert_eq!(pick(&["pt-PT", "en-US"]), Some(3));
        assert_eq!(pick(&["pt-BR"]), Some(2));
        // A language VERA lacks yields to the next preference.
        assert_eq!(pick(&["fi-FI", "en-GB"]), Some(0));
        assert_eq!(pick(&["fi-FI"]), None);
    }

    #[test]
    fn the_pseudo_language_keeps_values_and_formats() {
        assert_eq!(pseudo("Rank {rank}, %d%%"), "[Ŕåñķ {rank}, %d%% ~~]");
        assert_eq!(pseudo("OK"), "[ÖĶ ~]");
        assert_eq!(pseudo("A"), "[Å]");
        assert_eq!(pseudo(""), "");
        assert_eq!(pseudo(" "), " ");
    }

    #[test]
    fn the_check_lists_untranslated_texts_missing_forms_and_values() {
        let file = "[Language]\nName=Русский\n[Strings]\nVERA:CatchingUp[one]=Догоняем\nVERA:PlayersOnline=Игроки\nVERA:Bogus=?\nNoEquals\n";
        assert!(!check("russian.ini", file).complete());
        let report = check("vera20k.ru.ini", file);
        assert!(!report.complete());
        assert!(
            report
                .untranslated
                .iter()
                .any(|(label, _)| label == "VERA:Pausing")
        );
        assert!(
            report
                .missing_forms
                .contains(&"VERA:CatchingUp[few]".to_owned())
        );
        assert!(
            report
                .placeholders
                .iter()
                .any(|line| line.contains("VERA:PLAYERSONLINE, needs {count}"))
        );
        assert_eq!(report.unknown.len(), 1);
        assert_eq!(report.problems.len(), 1);
    }

    #[test]
    fn shipped_languages_are_complete() {
        for (tag, text) in shipped::FILES {
            let report = check(&format!("vera20k.{tag}.ini"), text);
            assert!(report.complete(), "{report}");
        }
    }
}
