//! VERA's interface text in the player's language.
//!
//! Every text VERA draws itself comes from a language file,
//! `vera20k.<tag>.ini`, where `<tag>` is a BCP 47 language tag such as `en`,
//! `zh-CN` or `pt-BR`. VERA ships the files beside this module (build.rs lists
//! them, so shipping a language is adding its file). They are compiled in,
//! except in the installers' builds: those compile in only English, and each
//! installer puts the rest in a folder of its own ([`packaged_language_dir`]),
//! so a language can be removed after the build by deleting its files. A file
//! of that name in the settings folder adds a language, or replaces a shipped
//! one while it is being written. `vera20k --check-language <file>` lists what
//! a file leaves untranslated ([`check`]).
//!
//! The format, UTF-8:
//!
//! ```ini
//! [Language]
//! ; How the Language list shows it.
//! Name=Deutsch
//!
//! [Strings]
//! VERA:SaveSetup=Setup speichern
//! VERA:CatchingUp[one]=Holt auf - {count} Frame zurück
//! VERA:CatchingUp[other]=Holt auf - {count} Frames zurück
//! ; One of the game's own labels.
//! GUI:Ok=OK
//! ```
//!
//! A line starting with `;` is a comment; a `;` after a text is part of it.
//! `\n` in a text is a line break, and `{name}` marks a value VERA fills in,
//! in whatever order the sentence needs.
//! A text with a count gives one line per CLDR plural form the language uses
//! (`zero`, `one`, `two`, `few`, `many`, `other`); `{count}` is the count.
//!
//! A language whose texts mostly start with a right-to-left letter (Arabic,
//! Hebrew) reads right to left ([`right_to_left`]): there a text holding
//! such a letter reads from the right even when it starts with a name or a
//! number. VERA keeps gamemd's layouts unmirrored, so text stays on its
//! box's side.
//!
//! English holds every VERA label and answers any label the chosen language
//! lacks. A label that is not VERA's own (`GUI:Ok`, `NAME:MTNK`) translates
//! the game's string table: the chosen language's text replaces the game
//! files' ([`apply`]), for players whose game files are in another language.
//!
//! A language can also bring its movie subtitles and the names drawn on its
//! cameos, in files beside its text: `vera20k.<tag>.subtitles.txt`, in the
//! game's `subtitlemd.txt` format ([`subtitle_file`]), and
//! `vera20k.<tag>.cameos` ([`cameo_labels`]). Each is optional and only
//! changes what the screen shows.
//!
//! Until the player picks a language (`[VERA] Language=`), VERA follows the
//! system's preferred languages. The pseudo-language [`PSEUDO_LANGUAGE`]
//! rewrites and lengthens every text, VERA's and the game's, so a screen
//! shows any text that does not come from a language file or does not fit;
//! `VERA20K_TEXT_AUDIT` logs both ([`crate::render::text_audit`]).
//!
//! Options' Developer page arms a hotkey ([`set_show_string_keys`]) that shows
//! every label as its own string-table key ([`toggle_string_keys`]), so a
//! screen can name the key it draws, whether the text comes from the game's
//! table or from a language file; the texts show until the hotkey asks for
//! keys. While that box is ticked, a language file added or rewritten in the
//! settings folder — or in a checkout's `src/i18n`, which then replaces the
//! shipped files by tag — is loaded again within a second
//! ([`poll_language_files`]), so its texts follow without a restart.

use std::collections::{BTreeSet, HashMap};
use std::fmt::{self, Display, Write as _};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, LazyLock, Mutex, PoisonError, RwLock};

use crate::util::time::{Duration, Instant};

use icu_locale_core::LanguageIdentifier;
use icu_plurals::{PluralCategory, PluralOperands, PluralRules};

use crate::assets::csf_file::CsfFile;

pub(crate) mod cameo_labels;

use cameo_labels::CameoLabel;

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

/// Right-to-left letters: Hebrew, Arabic, Syriac, Thaana, N'Ko, Samaritan,
/// Mandaic and their presentation forms.
pub fn is_right_to_left_letter(c: char) -> bool {
    matches!(c as u32, 0x0590..=0x08FF | 0xFB1D..=0xFDFF | 0xFE70..=0xFEFF)
}

/// The characters GAME.FNT draws on its full-cell grid and Noto Sans CJK on
/// its ideographic em: CJK ideographs, kana, Hangul, Bopomofo, CJK symbols and
/// punctuation, and the fullwidth and halfwidth forms.
pub(crate) fn is_cjk_sized(cp: u32) -> bool {
    matches!(
        cp,
        0x1100..=0x11FF
            | 0x2E80..=0x2FDF
            | 0x2FF0..=0x31FF
            | 0x3200..=0x9FFF
            | 0xA960..=0xA97F
            | 0xAC00..=0xD7FF
            | 0xF900..=0xFAFF
            | 0xFE30..=0xFE4F
            | 0xFF00..=0xFFEF
            | 0x20000..=0x3FFFF
    )
}

/// Whether more of `texts` start with a right-to-left letter than with a
/// left-to-right one, the `{values}` VERA fills in aside.
fn reads_right_to_left<'a>(texts: impl IntoIterator<Item = &'a str>) -> bool {
    let first_letter = |text: &str| {
        let mut in_value = false;
        text.chars().find(|&c| {
            match c {
                '{' => in_value = true,
                '}' => in_value = false,
                _ => {}
            }
            !in_value && c.is_alphabetic()
        })
    };
    let balance: i64 = texts
        .into_iter()
        .filter_map(first_letter)
        .map(|c| if is_right_to_left_letter(c) { 1 } else { -1 })
        .sum();
    balance > 0
}

/// One language: its file's texts.
struct Language {
    tag: String,
    name: String,
    /// Upper-case label, or `LABEL[form]` for a plural form, to text.
    strings: HashMap<String, String>,
    /// Its texts read right to left ([`reads_right_to_left`]).
    right_to_left: bool,
    /// Its movie subtitles, a `subtitlemd.txt` of its own.
    subtitles: Option<Arc<[u8]>>,
    /// The names it draws on cameos, by lower-case file name.
    cameo_labels: HashMap<String, Arc<CameoLabel>>,
}

impl Language {
    fn from_file(tag: &str, text: &str) -> Self {
        let contents = parse(text);
        let strings: HashMap<String, String> = contents
            .entries
            .into_iter()
            .map(|entry| (entry.key(), entry.text))
            .collect();
        Self {
            tag: tag.to_owned(),
            name: contents.name.unwrap_or_else(|| tag.to_owned()),
            right_to_left: reads_right_to_left(strings.values().map(String::as_str)),
            strings,
            subtitles: None,
            cameo_labels: HashMap::new(),
        }
    }

    /// With the files beside its text: from the first of `dirs` that has
    /// each, else those compiled into VERA.
    fn with_companions(mut self, dirs: &[&Path]) -> Self {
        self.subtitles = companion(&self.tag, "subtitles.txt", dirs, shipped::SUBTITLES);
        if let Some(file) = companion(&self.tag, "cameos", dirs, shipped::CAMEO_LABELS) {
            match cameo_labels::parse(&file) {
                Some(labels) => self.cameo_labels = labels,
                None => log::warn!("vera20k.{}.cameos is damaged; skipped", self.tag),
            }
        }
        self
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
            right_to_left: false,
            subtitles: None,
            cameo_labels: HashMap::new(),
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

/// `vera20k.<tag>.<suffix>` from the first of `dirs` that has it, else the
/// copy compiled into VERA.
fn companion(
    tag: &str,
    suffix: &str,
    dirs: &[&Path],
    shipped: &[(&str, &[u8])],
) -> Option<Arc<[u8]>> {
    let file = format!("vera20k.{tag}.{suffix}");
    dirs.iter()
        .find_map(|dir| std::fs::read(dir.join(&file)).ok())
        .map(Arc::from)
        .or_else(|| {
            shipped
                .iter()
                .find(|(known, _)| known.eq_ignore_ascii_case(tag))
                .map(|(_, file)| Arc::from(*file))
        })
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

/// Whether every label displays its own string-table key instead of its text
/// (Options' Developer page), VERA's own `VERA:` labels included.
static SHOW_STRING_KEYS: AtomicBool = AtomicBool::new(false);
/// Set by the every-screen hotkey: while the box is ticked, show the texts
/// again without unticking it, so the two views compare in place.
static STRING_KEYS_VALUES: AtomicBool = AtomicBool::new(false);

/// Whether labels currently show their keys: the Developer box is ticked and
/// the hotkey has not flipped this view back to the texts.
pub fn show_string_keys() -> bool {
    string_keys_enabled() && !STRING_KEYS_VALUES.load(Ordering::Relaxed)
}

/// Whether the Developer page's String Keys box is ticked. The hotkey only
/// works while it is.
pub fn string_keys_enabled() -> bool {
    SHOW_STRING_KEYS.load(Ordering::Relaxed)
}

/// Turn the Developer page's String Keys box on or off. Turning it on only
/// arms the hotkey: the texts stay until [`toggle_string_keys`] asks for the
/// keys, so a cold start never opens in keys. The caller re-applies the loaded
/// string table ([`apply`]) and lets the next frame read it.
pub fn set_show_string_keys(enabled: bool) {
    SHOW_STRING_KEYS.store(enabled, Ordering::Relaxed);
    STRING_KEYS_VALUES.store(true, Ordering::Relaxed);
}

/// The every-screen hotkey: flip between keys and texts while the box is
/// ticked, and do nothing while it is not. True when labels now show keys.
pub fn toggle_string_keys() -> bool {
    if !string_keys_enabled() {
        return false;
    }
    // After the flip: values show iff they did not before.
    STRING_KEYS_VALUES.fetch_xor(true, Ordering::Relaxed)
}

/// How often [`poll_language_files`] looks at the settings folder.
const LANGUAGE_FILE_POLL: Duration = Duration::from_secs(1);

/// The settings folder's language files as the last poll saw them: name,
/// size and modified time, sorted by name.
struct LanguageFiles {
    stamped: Vec<(String, u64, u128)>,
    last_poll: Option<Instant>,
}

static LANGUAGE_FILES: LazyLock<Mutex<LanguageFiles>> = LazyLock::new(|| {
    Mutex::new(LanguageFiles {
        stamped: Vec::new(),
        last_poll: None,
    })
});

/// While the Developer page's String Keys box is ticked, notice a language
/// file added, removed or rewritten in `settings_dir` — or in the checkout's
/// `src/i18n` when the game runs from one — and load the languages again,
/// keeping the selected tag where it still exists. Checked at most once a
/// second. The first poll of a session reloads too, so a checkout's files
/// answer as soon as the box is ticked. True when the catalog was rebuilt, so
/// the caller re-applies it to the loaded string table.
pub fn poll_language_files(settings_dir: Option<&Path>) -> bool {
    let mut watch = LANGUAGE_FILES
        .lock()
        .unwrap_or_else(PoisonError::into_inner);
    let now = Instant::now();
    if watch
        .last_poll
        .is_some_and(|last| now.saturating_duration_since(last) < LANGUAGE_FILE_POLL)
    {
        return false;
    }
    let first = watch.last_poll.is_none();
    watch.last_poll = Some(now);
    let source_dir = source_language_dir();
    let stamped = language_file_stamp(settings_dir, source_dir.as_deref());
    if !first && stamped == watch.stamped {
        return false;
    }
    watch.stamped = stamped;
    drop(watch);
    reload_languages(settings_dir, source_dir.as_deref());
    true
}

/// The checkout's language files, `src/i18n` beside the working directory,
/// when the game runs from one. Their tags replace the shipped files while
/// the Developer page's String Keys box is ticked, so a translation edit
/// answers without a rebuild.
fn source_language_dir() -> Option<std::path::PathBuf> {
    let dir = std::env::current_dir().ok()?.join("src").join("i18n");
    dir.is_dir().then_some(dir)
}

/// Each directory's `vera20k.<tag>.ini` files, as path, size and modified
/// time. A file added, removed or rewritten changes the stamp.
fn language_file_stamp(
    settings_dir: Option<&Path>,
    source_dir: Option<&Path>,
) -> Vec<(String, u64, u128)> {
    let mut files: Vec<(String, u64, u128)> = settings_dir
        .into_iter()
        .chain(source_dir)
        .flat_map(|dir| {
            std::fs::read_dir(dir)
                .into_iter()
                .flatten()
                .filter_map(|entry| {
                    let entry = entry.ok()?;
                    file_tag(&entry.file_name().into_string().ok()?)?;
                    let metadata = entry.metadata().ok()?;
                    let modified = metadata
                        .modified()
                        .ok()?
                        .duration_since(std::time::UNIX_EPOCH)
                        .ok()?;
                    Some((
                        entry.path().to_string_lossy().into_owned(),
                        metadata.len(),
                        modified.as_nanos(),
                    ))
                })
        })
        .collect();
    files.sort();
    files
}

/// Load the shipped, settings-folder and checkout languages again, keeping
/// the selected tag when its file still exists. A checkout file replaces the
/// language of the same tag, as a settings-folder file does.
fn reload_languages(settings_dir: Option<&Path>, source_dir: Option<&Path>) {
    let selected = catalog().selected.tag.clone();
    let mut next = Catalog::new(languages_with_source(settings_dir, source_dir));
    let kept = next.find(&selected).cloned();
    next.selected = match kept {
        Some(language) => language,
        None => next.languages[0].clone(),
    };
    let tag = next.selected.tag.clone();
    *CATALOG.write().unwrap_or_else(PoisonError::into_inner) = next;
    log::info!("Interface language reloaded: {tag}");
}

/// The shipped and settings-folder languages, then the checkout's files by
/// tag: a file there replaces the loaded language it names.
fn languages_with_source(
    settings_dir: Option<&Path>,
    source_dir: Option<&Path>,
) -> Vec<Arc<Language>> {
    let mut languages = languages(settings_dir);
    let Some(source_dir) = source_dir else {
        return languages;
    };
    for (tag, path) in settings_files(source_dir) {
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        let language = Arc::new(Language::from_file(&tag, &text).with_companions(&[source_dir]));
        if place_language(&mut languages, language) {
            log::info!("vera20k.{tag}.ini in the checkout replaces the loaded one");
        }
    }
    languages
}

fn catalog() -> std::sync::RwLockReadGuard<'static, Catalog> {
    CATALOG.read().unwrap_or_else(PoisonError::into_inner)
}

/// The languages compiled in, those an installer put with the game, those in
/// `settings_dir`, and the pseudo-language.
fn languages(settings_dir: Option<&Path>) -> Vec<Arc<Language>> {
    languages_from(installed_language_dir().as_deref(), settings_dir)
}

/// The folder of languages an installer put with the running game.
fn installed_language_dir() -> Option<PathBuf> {
    packaged_language_dir(std::env::current_exe().ok()?.parent()?)
}

/// Where each installer puts the languages that are not compiled in, from the
/// folder holding the game's program: beside it on Windows, in the app bundle's
/// Resources on macOS, in the AppImage's `usr/share/vera20k` on Linux. The
/// last two are sealed, so there a language is removed before packaging.
fn packaged_language_dir(program_dir: &Path) -> Option<PathBuf> {
    [
        "languages",
        "../Resources/languages",
        "../share/vera20k/languages",
    ]
    .into_iter()
    .map(|place| program_dir.join(place))
    .find(|dir| dir.is_dir())
}

/// [`languages`], from `installed_dir` and `settings_dir`. A file in a later
/// place replaces the language of its tag, and keeps that language's other
/// files unless its own folder has them.
fn languages_from(installed_dir: Option<&Path>, settings_dir: Option<&Path>) -> Vec<Arc<Language>> {
    let companion_dirs: Vec<&Path> = settings_dir.into_iter().chain(installed_dir).collect();
    let load = |tag: &str, text: &str| {
        Arc::new(Language::from_file(tag, text).with_companions(&companion_dirs))
    };
    let mut languages: Vec<Arc<Language>> = shipped::FILES
        .iter()
        .map(|(tag, text)| load(tag, text))
        .collect();
    if let Some(dir) = installed_dir {
        for (tag, path) in settings_files(dir) {
            if let Some(text) = read_language_file(&path) {
                place_language(&mut languages, load(&tag, &text));
            }
        }
    }
    if let Some(dir) = settings_dir {
        for (tag, path) in settings_files(dir) {
            let Some(text) = read_language_file(&path) else {
                continue;
            };
            // The log tells a translator what the file still lacks.
            let name = path
                .file_name()
                .map(|name| name.to_string_lossy().into_owned());
            let report = check(name.as_deref().unwrap_or_default(), &text);
            if !report.complete() {
                log::info!("{report}");
            }
            let language = load(&tag, &text);
            if place_language(&mut languages, language.clone()) {
                log::info!("vera20k.{tag}.ini in the settings folder replaces the shipped one");
            } else {
                log::info!(
                    "Language {tag} ({}) from the settings folder",
                    language.name
                );
            }
        }
    }
    let pseudo = Language::pseudo(&languages[0]);
    languages.push(Arc::new(pseudo));
    languages
}

/// A language file's text, or `None` and a warning in the log.
fn read_language_file(path: &Path) -> Option<String> {
    match std::fs::read(path).map(String::from_utf8) {
        Ok(Ok(text)) => Some(text),
        Ok(Err(_)) => {
            log::warn!("{} is not UTF-8 text; skipped", path.display());
            None
        }
        Err(error) => {
            log::warn!("Could not read {}: {error}", path.display());
            None
        }
    }
}

/// Add `language`, in the place of the one of its tag when there is one, and
/// say whether there was.
fn place_language(languages: &mut Vec<Arc<Language>>, language: Arc<Language>) -> bool {
    match languages
        .iter()
        .position(|known| known.tag.eq_ignore_ascii_case(&language.tag))
    {
        Some(index) => {
            languages[index] = language;
            true
        }
        None => {
            languages.push(language);
            false
        }
    }
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
    let language = {
        let mut catalog = CATALOG.write().unwrap_or_else(PoisonError::into_inner);
        let Some(language) = catalog.find(tag).cloned() else {
            return false;
        };
        catalog.selected = language.clone();
        language
    };
    // Logged after the lock is released: a panic while logging reports
    // itself in the language in use, which reads the catalog.
    log::info!("Interface language: {} ({})", language.tag, language.name);
    true
}

/// The language in use's tag, such as `en` or `zh-CN`.
pub fn selected_language() -> String {
    catalog().selected.tag.clone()
}

/// The language in use's own movie subtitles, when it has them, in the
/// game's `subtitlemd.txt` format.
pub fn subtitle_file() -> Option<Arc<[u8]>> {
    catalog().selected.subtitles.clone()
}

/// The name the language in use draws on cameo file `file` (`GTNKICON.SHP`),
/// when it has one.
pub fn cameo_label(file: &str) -> Option<Arc<CameoLabel>> {
    catalog()
        .selected
        .cameo_labels
        .get(&file.to_ascii_lowercase())
        .cloned()
}

/// Whether the language `tag` reads right to left: most of its file's texts
/// start with a right-to-left letter. A text holding one then reads from
/// the right whatever it starts with, as a sentence that opens with a name
/// does; text without one, such as the game's English, keeps its own order.
pub fn right_to_left(tag: &str) -> bool {
    catalog()
        .find(tag)
        .is_some_and(|language| language.right_to_left)
}

/// A language the player can choose.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LanguageChoice {
    pub tag: String,
    /// The language's name in itself, such as `简体中文`.
    pub name: String,
}

/// Each shipped language's text for every label `wanted` picks, then the
/// pseudo-language's, as (tag, label, text): for tests that check a text
/// fits where it is drawn in every language.
#[cfg(test)]
pub(crate) fn shipped_texts(wanted: impl Fn(&str) -> bool) -> Vec<(String, String, String)> {
    let languages: Vec<Language> = shipped::FILES
        .iter()
        .map(|(tag, text)| Language::from_file(tag, text))
        .collect();
    let pseudo = Language::pseudo(&languages[0]);
    languages
        .iter()
        .chain([&pseudo])
        .flat_map(|language| {
            language
                .strings
                .iter()
                .filter(|(label, _)| wanted(label))
                .map(|(label, text)| (language.tag.clone(), label.clone(), text.clone()))
        })
        .collect()
}

/// The languages the player can choose, English first and the rest by tag;
/// the pseudo-language only while it is in use, so its name shows.
pub fn choices() -> Vec<LanguageChoice> {
    let catalog = catalog();
    let mut choices: Vec<LanguageChoice> = catalog
        .languages
        .iter()
        .filter(|language| !language.is_pseudo() || catalog.selected.is_pseudo())
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
    apply_catalog(&catalog(), csf, show_string_keys());
}

fn apply_catalog(catalog: &Catalog, csf: &mut CsfFile, show_keys: bool) {
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
    if show_keys {
        // Every label reads as its own key. The catalog's labels are in the
        // table as VERA's overrides and fallbacks, so they need their own key
        // overrides too; plural forms (`LABEL[one]`) are not labels a screen
        // asks for.
        let catalog_keys = catalog
            .english()
            .strings
            .keys()
            .chain(catalog.selected.strings.keys())
            .filter(|key| !key.ends_with(']'));
        let keys: Vec<String> = csf
            .entries()
            .map(|(key, _)| key)
            .chain(catalog_keys.map(String::as_str))
            .map(str::to_ascii_uppercase)
            .collect();
        for key in keys {
            csf.set_override(&key, &key);
        }
    }
}

/// The text of a label a screen names: VERA's own (`VERA:`) from its
/// language files, which answer whether or not the game's string table has
/// loaded, and the game's from that table (`csf`), or `fallback` while none
/// has.
pub fn label(csf: Option<&CsfFile>, key: &str, fallback: &str) -> String {
    if show_string_keys() {
        return key.to_ascii_uppercase();
    }
    if key.starts_with("VERA:") {
        return tr(key);
    }
    csf.map_or_else(|| fallback.to_owned(), |csf| csf.text(key).into_owned())
}

/// VERA's text for `key` in the language in use, or the key itself while the
/// Developer page shows string keys.
pub fn tr(key: &str) -> String {
    if show_string_keys() {
        return key.to_ascii_uppercase();
    }
    catalog()
        .text(key)
        .map_or_else(|| missing(key), str::to_owned)
}

/// [`tr`] with each `{name}` filled in.
pub fn tr_fill(key: &str, values: &[(&str, &dyn Display)]) -> String {
    fill(&tr(key), values)
}

/// The text for `key` in the plural form `count` takes in the language in
/// use, with `{count}` and each `{name}` filled in; the key itself while the
/// Developer page shows string keys.
pub fn tr_count<N>(key: &str, count: N, values: &[(&str, &dyn Display)]) -> String
where
    N: Into<PluralOperands> + Display + Copy,
{
    if show_string_keys() {
        return key.to_ascii_uppercase();
    }
    let text = catalog()
        .plural(key, count.into())
        .map_or_else(|| missing(key), str::to_owned);
    let mut all: Vec<(&str, &dyn Display)> = vec![("count", &count)];
    all.extend_from_slice(values);
    fill(&text, &all)
}

/// [`tr`]'s text before any value is filled in, or `None` when no language
/// file has `key`: for keys that come from outside VERA, such as the lobby's
/// lines, whose sender's own text stands in for an unknown key.
pub fn find(key: &str) -> Option<String> {
    catalog().text(key).map(str::to_owned)
}

/// [`tr_count`]'s form for `count` before any value is filled in, or `None`
/// as for [`find`].
pub fn find_count(key: &str, count: PluralOperands) -> Option<String> {
    catalog().plural(key, count).map(str::to_owned)
}

fn missing(key: &str) -> String {
    format!("MISSING:'{key}'")
}

/// Replace each `{name}` in a localized text with its value, in one pass, so
/// a value is never read for names itself: a player called `{count}` stays
/// `{count}`. A `{name}` with no value stays as it is.
pub fn fill(text: &str, values: &[(&str, &dyn Display)]) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(open) = rest.find('{') {
        out.push_str(&rest[..open]);
        let after = &rest[open + 1..];
        let value = after.find('}').and_then(|close| {
            let name = &after[..close];
            let value = values.iter().find(|(key, _)| *key == name)?.1;
            Some((close, value))
        });
        match value {
            Some((close, value)) => {
                let _ = write!(out, "{value}");
                rest = &after[close + 1..];
            }
            None => {
                out.push('{');
                rest = after;
            }
        }
    }
    out.push_str(rest);
    out
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
    right_to_left: bool,
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
            "{}: {} ({tag}), plural forms {}{}",
            self.file,
            self.name.as_deref().unwrap_or("no [Language] Name="),
            self.forms.join(", "),
            if self.right_to_left {
                ", reads right to left"
            } else {
                ""
            }
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
        right_to_left: reads_right_to_left(
            contents.entries.iter().map(|entry| entry.text.as_str()),
        ),
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

    /// A language's subtitles and cameo names load from the files beside its
    /// text in the settings folder, and a damaged cameo file only loses those.
    #[test]
    fn a_languages_companion_files_load_from_beside_its_text() {
        let dir = std::env::temp_dir().join(format!("vera-i18n-beside-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("vera20k.xx.ini"), "[Language]\nName=Test\n").unwrap();
        std::fs::write(dir.join("vera20k.yy.ini"), "[Language]\nName=Other\n").unwrap();
        std::fs::write(dir.join("vera20k.xx.subtitles.txt"), b"cues").unwrap();
        let canvas = vec![1u8; cameo_labels::CAMEO_WIDTH * cameo_labels::CAMEO_HEIGHT];
        let labels = cameo_labels::test_support::label_file("icon.shp", 40, &canvas, &canvas);
        std::fs::write(dir.join("vera20k.xx.cameos"), labels).unwrap();
        std::fs::write(dir.join("vera20k.yy.cameos"), b"damaged").unwrap();

        let loaded = languages(Some(&dir));
        let find = |tag: &str| loaded.iter().find(|language| language.tag == tag).unwrap();
        assert_eq!(find("xx").subtitles.as_deref(), Some(&b"cues"[..]));
        assert!(find("xx").cameo_labels.contains_key("icon.shp"));
        assert_eq!(find("yy").subtitles, None);
        assert!(find("yy").cameo_labels.is_empty());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// An installer's folder brings a language and its other files, a copy of
    /// its text in the settings folder replaces only the text, and deleting
    /// the files takes the language out of the game.
    #[test]
    fn an_installed_language_can_be_replaced_and_removed() {
        let root = std::env::temp_dir().join(format!("vera-i18n-installed-{}", std::process::id()));
        let installed = root.join("languages");
        let settings = root.join("settings");
        std::fs::create_dir_all(&installed).unwrap();
        std::fs::create_dir_all(&settings).unwrap();
        let installed_text = installed.join("vera20k.xx.ini");
        let settings_text = settings.join("vera20k.xx.ini");
        std::fs::write(&installed_text, "[Language]\nName=Installed\n").unwrap();
        std::fs::write(installed.join("vera20k.xx.subtitles.txt"), b"cues").unwrap();
        let load = || {
            languages_from(Some(&installed), Some(&settings))
                .into_iter()
                .find(|language| language.tag == "xx")
        };

        let language = load().expect("the installed language");
        assert_eq!(language.name, "Installed");
        assert_eq!(language.subtitles.as_deref(), Some(&b"cues"[..]));

        std::fs::write(&settings_text, "[Language]\nName=Mine\n").unwrap();
        let language = load().expect("the player's copy");
        assert_eq!(language.name, "Mine");
        assert_eq!(language.subtitles.as_deref(), Some(&b"cues"[..]));

        std::fs::remove_file(&settings_text).unwrap();
        std::fs::remove_file(&installed_text).unwrap();
        assert!(load().is_none());
        std::fs::remove_dir_all(&root).unwrap();
    }

    /// The game finds the language folder of each installer's layout, and
    /// none where there is no folder.
    #[test]
    fn each_installers_language_folder_is_found() {
        let root = std::env::temp_dir().join(format!("vera-i18n-layouts-{}", std::process::id()));
        for (program, folder) in [
            ("windows", "windows/languages"),
            (
                "VERA20K.app/Contents/MacOS",
                "VERA20K.app/Contents/Resources/languages",
            ),
            ("AppDir/usr/bin", "AppDir/usr/share/vera20k/languages"),
        ] {
            let program = root.join(program);
            let folder = root.join(folder);
            std::fs::create_dir_all(&program).unwrap();
            assert_eq!(packaged_language_dir(&program), None);
            std::fs::create_dir_all(&folder).unwrap();
            let found = packaged_language_dir(&program).expect("the folder");
            assert_eq!(
                found.canonicalize().unwrap(),
                folder.canonicalize().unwrap()
            );
        }
        std::fs::remove_dir_all(&root).unwrap();
    }

    /// A value is never read for names: a player called `{count}` keeps the
    /// name, and a name with no value stays as it is.
    #[test]
    fn fill_puts_each_value_in_once() {
        let values: [(&str, &dyn Display); 2] = [("name", &"{count}"), ("count", &3)];
        assert_eq!(
            fill("{name} has {count} {rest}", &values),
            "{count} has 3 {rest}"
        );
    }

    #[test]
    fn the_chosen_language_translates_the_game_and_english_fills_gaps() {
        let file = "[Strings]\nGUI:Ok=确定\nVERA:Pausing=正在暂停...\n";
        let catalog = catalog_with("zh-CN", file);
        let mut csf = test_csf(&[("GUI:Ok", "OK"), ("GUI:Cancel", "Cancel")]);
        apply_catalog(&catalog, &mut csf, false);
        // The chosen language replaces the game's own text...
        assert_eq!(csf.text("GUI:Ok"), "确定");
        assert_eq!(csf.text("GUI:Cancel"), "Cancel");
        assert_eq!(csf.text("VERA:Pausing"), "正在暂停...");
        // ...and English answers what it lacks.
        assert_eq!(csf.text("VERA:Resuming"), "Resuming...");
        assert_eq!(catalog.text("VERA:Resuming"), Some("Resuming..."));
    }

    /// The watch sees a language file added or rewritten, and ignores files
    /// the loader would not read.
    #[test]
    fn language_file_changes_change_the_watch_stamp() {
        let dir = std::env::temp_dir().join(format!("vera-i18n-watch-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("vera20k.xx.ini");
        std::fs::write(&file, "[Language]\nName=Test\n").unwrap();
        let before = language_file_stamp(Some(&dir), None);
        assert_eq!(before.len(), 1);
        assert!(before[0].0.ends_with("vera20k.xx.ini"));
        // A file the loader never reads does not wake the watch.
        std::fs::write(dir.join("other.ini"), "x").unwrap();
        assert_eq!(language_file_stamp(Some(&dir), None), before);
        // A rewrite does.
        std::fs::write(
            &file,
            "[Language]\nName=Test\n[Strings]\nVERA:SaveSetup=S\n",
        )
        .unwrap();
        assert_ne!(language_file_stamp(Some(&dir), None), before);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// A checkout's language file replaces the loaded language of its tag,
    /// so an edit answers on the next poll instead of the next build.
    #[test]
    fn checkout_language_files_replace_the_loaded_tag() {
        let root = std::env::temp_dir().join(format!("vera-i18n-source-{}", std::process::id()));
        let settings = root.join("settings");
        let source = root.join("source");
        std::fs::create_dir_all(&settings).unwrap();
        std::fs::create_dir_all(&source).unwrap();
        std::fs::write(
            settings.join("vera20k.xx.ini"),
            "[Language]\nName=Settings\n[Strings]\nVERA:SaveSetup=Settings\n",
        )
        .unwrap();
        std::fs::write(
            source.join("vera20k.xx.ini"),
            "[Language]\nName=Source\n[Strings]\nVERA:SaveSetup=Source\n",
        )
        .unwrap();
        let languages = languages_with_source(Some(&settings), Some(&source));
        let xx = languages
            .iter()
            .find(|language| language.tag == "xx")
            .unwrap();
        assert_eq!(
            xx.strings.get("VERA:SAVESETUP").map(String::as_str),
            Some("Source")
        );
        std::fs::remove_dir_all(&root).unwrap();
    }

    /// The Developer page's box shows every label's key, the game's and
    /// VERA's own alike.
    #[test]
    fn string_keys_replace_every_label_including_vera_labels() {
        let file = "[Strings]\nGUI:Ok=确定\nVERA:Pausing=正在暂停...\n";
        let catalog = catalog_with("zh-CN", file);
        let mut csf = test_csf(&[("GUI:Ok", "OK"), ("TXT_MONEY_FORMAT", "%s $%d")]);
        apply_catalog(&catalog, &mut csf, true);
        assert_eq!(csf.text("GUI:Ok"), "GUI:OK");
        assert_eq!(csf.text("TXT_MONEY_FORMAT"), "TXT_MONEY_FORMAT");
        assert_eq!(csf.text("VERA:Pausing"), "VERA:PAUSING");
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

    /// A language reads right to left when most of its texts start with a
    /// right-to-left letter; a value VERA fills in does not count.
    #[test]
    fn a_language_reads_right_to_left_when_its_texts_do() {
        let arabic = "[Language]\nName=العربية\n[Strings]\nGUI:Ok=موافق\n\
                      VERA:Joined={name} انضم\nVERA:Brand=VERA20k\n";
        assert!(Language::from_file("ar", arabic).right_to_left);
        assert!(
            check("vera20k.ar.ini", arabic)
                .to_string()
                .contains("reads right to left")
        );
        for (tag, text) in shipped::FILES {
            let right_to_left = Language::from_file(tag, text).right_to_left;
            assert_eq!(right_to_left, *tag == "ar", "{tag}");
        }
    }

    #[test]
    fn shipped_languages_are_complete() {
        for (tag, text) in shipped::FILES {
            let report = check(&format!("vera20k.{tag}.ini"), text);
            assert!(report.complete(), "{report}");
        }
    }
}
