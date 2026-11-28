//! Immutable Theme catalog shared by playback and trigger operand reads.
//!
//! Native ThemeClass::ReadINI720590/Fill720480/Scan7207F0 establish the YR
//! prefix. Preserve the user-requested RA2 playlist extension after that prefix.
//! The process audio owner selects this once after neutral archive registration;
//! physical playback and scenario-specific side admission remain with that owner.

use std::collections::BTreeSet;
use std::sync::Arc;

use crate::assets::asset_manager::AssetManager;
use crate::assets::opus_file;
use crate::rules::ini_parser::{IniFile, IniSection};

const RA2_THEME_INI_NAME: &str = "theme.ini";

/// One `[Themes]` entry (native 0x290-byte record, ctor inside `0x00720590`,
/// reader `0x00720480`, scan `0x007207F0`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ThemeEntry {
    /// `[Themes]` value = section key (+0x000). `From_Name` matches this only.
    pub key: String,
    /// `Sound=` with leading `$`/`#` stripped (+0x100); `.WAV` appended at play.
    pub sound: String,
    /// `Name=` CSF key for the native localized +0x200 display name. B8's
    /// projection resolves it using the process CSF, with a 63-UTF16-unit cap.
    pub name_key: String,
    /// Whole seconds from native WAV metadata scan `7207F0`; independent of
    /// physical playback length (especially for IMA ADPCM music).
    pub duration_seconds: u32,
    /// `Scenario=` (+0x280, default 0).
    pub scenario: i32,
    /// `Normal=` (+0x288, default yes).
    pub normal: bool,
    /// `Repeat=` (+0x289, default no). Keyed by entry, never by `Sound=` stem.
    pub repeat: bool,
    /// `Side=` name (+0x28C holds the resolved index; default -1 = any side).
    pub side_name: Option<String>,
    /// `CCFileClass::IsAvailable(Sound + ".WAV")` (+0x28A), or VERA's
    /// `Sound + ".opus"` override (see [`ThemeCatalog::opus_sounds`]).
    pub available: bool,
}

/// Process-selected membership/order. A selected empty catalog is final too.
#[derive(Debug)]
pub struct ThemeCatalog {
    entries: Vec<ThemeEntry>,
    /// VERA: the lowercased `Sound=` stems the scan found a `<stem>.opus`
    /// override for (docs/opus-audio.md). Empty for retail data, whose
    /// playback then never looks for one.
    opus_sounds: Arc<BTreeSet<String>>,
}

impl ThemeCatalog {
    /// ReadINI720590 + Scan7207F0, then the authorized RA2 gameplay append.
    /// Select independently so RA2 cannot replace YR lifecycle sections.
    pub fn select(assets: &AssetManager) -> Self {
        let mut opus_sounds = BTreeSet::new();
        let mut entries =
            crate::rules::retail_sources::select_ini(assets, assets.ini_roots().theme())
                .map_or_else(|_| Vec::new(), |selected| catalog_from_ini(&selected.ini));
        scan_catalog_availability(&mut entries, assets, &mut opus_sounds);
        if let Ok(selected) = crate::rules::retail_sources::select_ini(assets, RA2_THEME_INI_NAME) {
            let mut ra2_entries = catalog_from_ini(&selected.ini);
            scan_catalog_availability(&mut ra2_entries, assets, &mut opus_sounds);
            for entry in ra2_entries {
                if entry.normal
                    && entry.available
                    && !entries
                        .iter()
                        .any(|yr| yr.key.eq_ignore_ascii_case(&entry.key))
                {
                    entries.push(entry);
                }
            }
        }
        Self {
            entries,
            opus_sounds: Arc::new(opus_sounds),
        }
    }

    /// The `Sound=` stems (lowercased) whose song plays from `<stem>.opus`,
    /// shared with the players that prepare songs from this catalog.
    pub fn opus_sounds(&self) -> &Arc<BTreeSet<String>> {
        &self.opus_sounds
    }

    pub fn entries(&self) -> &[ThemeEntry] {
        &self.entries
    }

    pub fn entry(&self, index: i32) -> Option<&ThemeEntry> {
        usize::try_from(index)
            .ok()
            .and_then(|index| self.entries.get(index))
    }

    /// ThemeClass::From_Name721210, original instruction listing: raw section
    /// key compared by stricmp7C8D20, first match; empty/unknown returns -1.
    /// Availability, Normal and playback gates do not participate.
    #[expect(clippy::wrong_self_convention, reason = "native ThemeClass::From_Name")]
    pub fn from_name(&self, name: &str) -> i32 {
        if name.is_empty() {
            return -1;
        }
        self.entries
            .iter()
            .position(|entry| entry.key.eq_ignore_ascii_case(name))
            .map_or(-1, |index| index as i32)
    }

    /// The catalog as it stands when Scan `0x007207F0` never ran: its gate
    /// (`0x007207F0..0x0072081B`) needs ScoresPresent, an output device and
    /// no `-X…Q` (`0x00A8ED64`), and without it every entry keeps the
    /// constructor's unavailable flag and zero length. Membership, which
    /// trigger operands read through `from_name`, does not change.
    pub fn without_scan(mut self) -> Self {
        for entry in &mut self.entries {
            entry.available = false;
            entry.duration_seconds = 0;
        }
        self
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn from_entries_for_test(entries: Vec<ThemeEntry>) -> Self {
        Self {
            entries,
            opus_sounds: Arc::default(),
        }
    }
}

fn find_section<'a>(ini: &'a IniFile, name: &str) -> Option<&'a IniSection> {
    ini.section(name).or_else(|| {
        ini.section_names()
            .into_iter()
            .find(|candidate| candidate.eq_ignore_ascii_case(name))
            .and_then(|candidate| ini.section(candidate))
    })
}

/// Native catalog scan `0x007207F0`: WAV availability and declared metadata
/// supply the playlist row; the shared player owns subsequent decoding.
///
/// VERA: a `<Sound>.opus` file ([`opus_file::override_name`]) makes the entry
/// available ahead of the WAV, with its length from the Ogg granule, and its
/// stem joins `opus_sounds`. Retail data has none: one name-hash miss per
/// entry, then the native scan unchanged.
fn scan_catalog_availability(
    entries: &mut [ThemeEntry],
    assets: &AssetManager,
    opus_sounds: &mut BTreeSet<String>,
) {
    for entry in entries {
        if let Some(opus) = (!entry.sound.is_empty())
            .then(|| assets.get_ref(&opus_file::override_name(&entry.sound)))
            .flatten()
        {
            entry.available = true;
            entry.duration_seconds = opus_file::duration_seconds(opus).unwrap_or(0);
            opus_sounds.insert(entry.sound.to_ascii_lowercase());
            continue;
        }
        let wav = (!entry.sound.is_empty())
            .then(|| assets.get_ref(&format!("{}.wav", entry.sound)))
            .flatten();
        entry.available = wav.is_some();
        entry.duration_seconds = wav
            .and_then(crate::assets::wav_file::WavFile::parse)
            .and_then(|wav| wav.native_duration_seconds())
            .unwrap_or(0);
    }
}

/// Installer diagnostics use the playlist's own catalog and availability scan,
/// without initializing an audio device or decoding whole songs.
pub fn missing_install_music(assets: &AssetManager) -> Vec<String> {
    let mut missing = Vec::new();
    let theme_ini = assets.ini_roots().theme();
    for name in [theme_ini, RA2_THEME_INI_NAME] {
        let selected = match crate::rules::retail_sources::select_ini(assets, name) {
            Ok(selected) => selected,
            Err(error) => {
                if name == theme_ini {
                    missing.push(format!("{name}: {error}"));
                }
                continue;
            }
        };
        let mut entries = catalog_from_ini(&selected.ini);
        // A `.opus` override counts as the track (docs/opus-audio.md).
        scan_catalog_availability(&mut entries, assets, &mut BTreeSet::new());
        for entry in entries {
            if entry.sound.is_empty() {
                continue;
            }
            let wav = format!("{}.wav", entry.sound);
            if !entry.available {
                let issue = format!("{wav}: music track unavailable");
                if !missing.contains(&issue) {
                    missing.push(issue);
                }
            }
        }
    }
    missing
}

/// Catalog load `0x00720590`: every non-empty `[Themes]` value in source
/// order becomes an entry keyed by that value (duplicates re-read the existing
/// entry); `0x00720480` fills the section fields with the ctor defaults
/// (Normal=1, Repeat=0, Scenario=0, Side=-1, Sound="").
pub fn catalog_from_ini(ini: &IniFile) -> Vec<ThemeEntry> {
    let mut entries: Vec<ThemeEntry> = Vec::new();
    let Some(themes) = ini.section("Themes") else {
        return entries;
    };
    // Each entry through ReadString(0x20) (`0x007205DE`).
    for entry_name in themes.keys() {
        let Some(key) = themes.read_name(entry_name, 0x20) else {
            continue;
        };
        let existing = entries
            .iter()
            .position(|entry| entry.key.eq_ignore_ascii_case(key));
        let index = match existing {
            Some(index) => index,
            None => {
                entries.push(ThemeEntry {
                    key: key.to_string(),
                    sound: String::new(),
                    name_key: String::new(),
                    duration_seconds: 0,
                    scenario: 0,
                    normal: true,
                    repeat: false,
                    side_name: None,
                    available: false,
                });
                entries.len() - 1
            }
        };
        let entry = &mut entries[index];
        let Some(section) = find_section(ini, &entry.key) else {
            continue;
        };
        // `0x00720480`: ReadString 0x100 over an empty default, then
        // ReadInt/ReadBool over each field and the side-name read
        // (`0x004756F0`, 0x80 bytes).
        entry.sound = section
            .read_string("Sound", "", 0x100)
            .trim_start_matches(['$', '#'])
            .to_string();
        entry.scenario = section.read_int("Scenario", entry.scenario);
        entry.normal = section.read_bool("Normal", entry.normal);
        entry.repeat = section.read_bool("Repeat", entry.repeat);
        if let Some(side) = section.read_name("Side", 0x80) {
            entry.side_name = Some(side.to_string());
        }
        if entry.normal {
            // The string-table label read `0x00529160` copies up to 0x3FF bytes.
            entry.name_key = section.read_string("Name", "", 0x400);
        }
    }
    entries
}
