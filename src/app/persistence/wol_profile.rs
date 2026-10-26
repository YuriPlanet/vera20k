//! `wolinfoMD.ini` (`0x00849F8C`): the WOL persona file gamemd writes next
//! to VERA.INI. Retail does not ship it; the first login flow that reaches
//! `0x10F` creates it (`0x007873AC`).
//!
//! Nothing is cached, as in native: every getter and setter reads and parses
//! the file again, and every setter saves it. The sections are
//! `[DefaultPersona]` (AutoLogin, LastValid), `[Ask]` (AutoLogin,
//! AddMatchBuddies) and one per persona, named by the lowercased nickname,
//! with `[_Default_]` as the fallback record (`0x007786E0`).
//!
//! The WOLAPI `Options` registry value gates some of these (bit 0: no
//! auto-login, bit 1: no named records). VERA has no registry: both bits read
//! clear, as on a clean install.

use std::io;
use std::path::{Path, PathBuf};

use crate::rules::ini_parser::{IniFile, IniSection};
use crate::util::ini_writer::{remove_ini_keys, set_ini_values};

pub(crate) const WOLINFO_INI: &str = "wolinfoMD.ini";

const DEFAULT_PERSONA: &str = "DefaultPersona";
const ASK: &str = "Ask";
const AUTO_LOGIN: &str = "AutoLogin";
const ADD_MATCH_BUDDIES: &str = "AddMatchBuddies";
const LAST_VALID: &str = "LastValid";
/// `[0xB73590]`, set from `"_Default_"` by `0x007786E0`.
const DEFAULT_RECORD: &str = "_Default_";
/// The Wstring ReadString wrapper `0x00528C00` copies through `strncpy`
/// with `0x3FF`.
const STRING_CAPACITY: usize = 0x3FF;

/// The month tokens `0x00778F30`/`0x00779230` translate, in their order:
/// the wide literals at `0x0084A0F8..0x00849FEC` and their CSF names.
const MONTHS: [(&str, &str); 12] = [
    ("Jan", "TXT_JANUARY"),
    ("Feb", "TXT_FEBRUARY"),
    ("Mar", "TXT_MARCH"),
    ("Apr", "TXT_APRIL"),
    ("May", "TXT_MAY"),
    ("Jun", "TXT_JUNE"),
    ("Jul", "TXT_JULY"),
    ("Aug", "TXT_AUGUST"),
    ("Sep", "TXT_SEPTEMBER"),
    ("Oct", "TXT_OCTOBER"),
    ("Nov", "TXT_NOVEMBER"),
    ("Dec", "TXT_DECEMBER"),
];

/// One persona record (`0x44` bytes, constructor `0x00779690`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct WolPersona {
    /// +8: a setter changed it since the load.
    dirty: bool,
    /// +0xC: the preferred server's display name.
    pub(crate) server: String,
    /// +0x10.
    pub(crate) locale: i32,
    /// +0x14.
    pub(crate) locale_verified: bool,
    /// +0x18: how the locale was set; never written to the file.
    locale_priority: i32,
    /// +0x1C / +0x20: MultiPlayer item data, -2 for Random.
    pub(crate) country: i32,
    pub(crate) color: i32,
    /// +0x24..+0x34, -1 when unknown.
    pub(crate) rank: i32,
    pub(crate) points: i32,
    pub(crate) wins: i32,
    pub(crate) losses: i32,
    pub(crate) disconnects: i32,
    /// +0x38.
    pub(crate) match_by_res: bool,
    /// +0x3C / +0x40: the last ladder update, wide text.
    pub(crate) update_time: String,
    pub(crate) update_date: String,
}

impl Default for WolPersona {
    /// The default record `0x00778710` with no file or no `[_Default_]`.
    fn default() -> Self {
        Self {
            dirty: false,
            server: String::new(),
            locale: 0,
            locale_verified: false,
            locale_priority: 0,
            country: -2,
            color: -2,
            rank: -1,
            points: -1,
            wins: -1,
            losses: -1,
            disconnects: -1,
            match_by_res: true,
            update_time: String::new(),
            update_date: String::new(),
        }
    }
}

impl WolPersona {
    /// `0x0077A4A0`.
    pub(crate) fn set_server(&mut self, server: &str) {
        if self.server != server {
            self.server = server.to_owned();
            self.dirty = true;
        }
    }

    /// `0x0077A760`: MatchByRes (+0x38).
    pub(crate) fn set_match_by_res(&mut self, value: bool) {
        if self.match_by_res != value {
            self.match_by_res = value;
            self.dirty = true;
        }
    }

    /// `0x0077A780`: the MultiPlayer country item data (+0x1C).
    pub(crate) fn set_country(&mut self, value: i32) {
        if self.country != value {
            self.country = value;
            self.dirty = true;
        }
    }

    /// `0x0077A7A0`: the MultiPlayer colour item data (+0x20).
    pub(crate) fn set_color(&mut self, value: i32) {
        if self.color != value {
            self.color = value;
            self.dirty = true;
        }
    }

    /// `0x0077A700(locale, priority)`: the same locale only verifies (from
    /// a service answer, priority 2); another needs a nonzero value and an
    /// unset locale or at least the stored priority, and priority 3 (the
    /// player's own pick) clears the verified flag.
    pub(crate) fn set_locale(&mut self, locale: i32, priority: i32) {
        if locale == self.locale {
            if priority == 2 && !self.locale_verified {
                self.locale_verified = true;
                self.dirty = true;
            }
            return;
        }
        let allowed = self.locale == 0 || priority >= self.locale_priority;
        if locale == 0 || !allowed {
            return;
        }
        if priority == 3 {
            self.locale_verified = false;
        }
        self.locale = locale;
        self.locale_priority = priority;
        self.dirty = true;
    }

    /// The default record's fields from `[_Default_]` (`0x00778710`).
    fn read_default(section: &IniSection, csf: &dyn Fn(&str) -> String) -> Self {
        Self {
            server: section.read_string("Server", "", STRING_CAPACITY),
            locale: section.read_int("Locale", 0),
            country: section.read_int("Country", -2),
            color: section.read_int("Color", -2),
            update_time: localize_months(
                &section.read_string("UpdateTime", "", STRING_CAPACITY),
                csf,
            ),
            update_date: section.read_string("UpdateDate", "", STRING_CAPACITY),
            ..Self::default()
        }
    }

    /// A named record (`0x00778A20` from `0x00778B9E`), in its read order.
    fn read_named(section: &IniSection, csf: &dyn Fn(&str) -> String) -> Self {
        let server = section.read_string("Server", "", STRING_CAPACITY);
        let locale = section.read_int("Locale", 0);
        let locale_verified = section.read_bool("LocaleVerified", false);
        Self {
            dirty: false,
            server,
            locale,
            locale_verified,
            // NEG/SBB/AND -2/ADD 3 at 0x00778E7F..0x00778EA0.
            locale_priority: if locale_verified { 1 } else { 3 },
            country: section.read_int("Country", -2),
            color: section.read_int("Color", -2),
            rank: section.read_int("Rank", -1),
            points: section.read_int("Points", -1),
            wins: section.read_int("Wins", -1),
            losses: section.read_int("Losses", -1),
            disconnects: section.read_int("Disconnects", -1),
            match_by_res: section.read_bool("MatchByRes", true),
            update_time: localize_months(
                &section.read_string("UpdateTime", "", STRING_CAPACITY),
                csf,
            ),
            update_date: section.read_string("UpdateDate", "", STRING_CAPACITY),
        }
    }
}

/// `0x00778F30`: the first month token found (January first) is replaced
/// everywhere by its CSF text (`WideWstring::replace` `0x007B73F0`).
fn localize_months(text: &str, csf: &dyn Fn(&str) -> String) -> String {
    MONTHS
        .iter()
        .find(|(token, _)| text.contains(token))
        .map(|(token, key)| text.replace(token, &csf(key)))
        .unwrap_or_else(|| text.to_owned())
}

/// `0x00779230`: every month's CSF text is replaced by its token, January
/// to December.
fn tokenize_months(text: &str, csf: &dyn Fn(&str) -> String) -> String {
    MONTHS.iter().fold(text.to_owned(), |text, (token, key)| {
        let name = csf(key);
        if !name.is_empty() && text.contains(&name) {
            text.replace(&name, token)
        } else {
            text
        }
    })
}

/// INIClass PutString `0x00528660`: the section is created even for an
/// empty value, and an empty value removes the key.
fn put_string(content: &[u8], section: &str, key: &str, value: &str) -> Vec<u8> {
    if value.is_empty() {
        let created = set_ini_values(content, section, &[(key, "")]);
        remove_ini_keys(&created, section, &[key])
    } else {
        set_ini_values(content, section, &[(key, value)])
    }
}

fn yes_no(value: bool) -> &'static str {
    // WriteBool 0x005295A0: 0x00825BF8 / 0x00825BF4.
    if value { "yes" } else { "no" }
}

/// `WOLInfo__SaveProfileToINI` `0x0077A080`'s writes, in order.
fn write_persona(
    content: &[u8],
    section: &str,
    persona: &WolPersona,
    csf: &dyn Fn(&str) -> String,
) -> Vec<u8> {
    let ints = [
        ("Country", persona.country),
        ("Color", persona.color),
        ("Rank", persona.rank),
        ("Points", persona.points),
        ("Wins", persona.wins),
        ("Losses", persona.losses),
        ("Disconnects", persona.disconnects),
    ];
    let mut out = put_string(content, section, "Server", &persona.server);
    out = put_string(&out, section, "Locale", &persona.locale.to_string());
    out = put_string(
        &out,
        section,
        "LocaleVerified",
        yes_no(persona.locale_verified),
    );
    for (key, value) in ints {
        out = put_string(&out, section, key, &value.to_string());
    }
    out = put_string(&out, section, "MatchByRes", yes_no(persona.match_by_res));
    // The native quirk: the tokenizer runs on the time, and the date keeps
    // its localized month (0x0077A3B8..0x0077A41A).
    out = put_string(
        &out,
        section,
        "UpdateTime",
        &tokenize_months(&persona.update_time, csf),
    );
    put_string(&out, section, "UpdateDate", &persona.update_date)
}

/// The file in one settings directory.
#[derive(Debug, Clone)]
pub(crate) struct WolInfoFile {
    path: PathBuf,
}

impl WolInfoFile {
    pub(crate) fn in_directory(directory: &Path) -> Self {
        Self {
            path: directory.join(WOLINFO_INI),
        }
    }

    fn read(&self) -> Option<IniFile> {
        let bytes = crate::util::file_system::read(&self.path).ok()?;
        Some(IniFile::from_bytes(&bytes).unwrap_or_else(|_| IniFile::from_str("")))
    }

    /// CDFileClass open-or-create, as the setters and `LoadRecord` do.
    fn read_or_create(&self) -> io::Result<Vec<u8>> {
        match crate::util::file_system::read(&self.path) {
            Ok(bytes) => Ok(bytes),
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                crate::util::file_system::write(&self.path, b"")?;
                Ok(Vec::new())
            }
            Err(error) => Err(error),
        }
    }

    fn update(&self, edit: impl FnOnce(&[u8]) -> Vec<u8>) {
        let result = self
            .read_or_create()
            .and_then(|bytes| crate::util::file_system::write(&self.path, edit(&bytes)));
        if let Err(error) = result {
            log::warn!("Could not save {}: {error}", self.path.display());
        }
    }

    fn read_string(&self, section: &str, key: &str) -> String {
        self.read()
            .map(|ini| {
                ini.section_or_empty(section)
                    .read_string(key, "", STRING_CAPACITY)
            })
            .unwrap_or_default()
    }

    /// `WOLInfo__GetAutoLogin` `0x00779B50`: "" without the file.
    pub(crate) fn auto_login(&self) -> String {
        self.read_string(DEFAULT_PERSONA, AUTO_LOGIN)
    }

    /// `WOLInfo__SetAutoLogin` `0x00779C90`.
    pub(crate) fn set_auto_login(&self, name: &str) {
        self.update(|bytes| put_string(bytes, DEFAULT_PERSONA, AUTO_LOGIN, name));
    }

    /// `WOLInfo__GetLastValid` `0x00779DD0`.
    pub(crate) fn last_valid(&self) -> String {
        self.read_string(DEFAULT_PERSONA, LAST_VALID)
    }

    /// `WOLInfo__SetLastValid` `0x00779F10`.
    pub(crate) fn set_last_valid(&self, name: &str) {
        self.update(|bytes| put_string(bytes, DEFAULT_PERSONA, LAST_VALID, name));
    }

    /// `WOLSettings__GetAutoLogin` `0x00779730`: false without the file,
    /// else ReadBool with default true.
    pub(crate) fn ask_auto_login(&self) -> bool {
        self.read()
            .is_some_and(|ini| ini.section_or_empty(ASK).read_bool(AUTO_LOGIN, true))
    }

    /// `WOLSettings__SetAutoLogin` `0x00779830`.
    pub(crate) fn set_ask_auto_login(&self, ask: bool) {
        self.update(|bytes| put_string(bytes, ASK, AUTO_LOGIN, yes_no(ask)));
    }

    /// `WOLSettings__GetAddMatchBuddies` `0x00779940`: false without the
    /// file, else ReadBool with default true.
    pub(crate) fn ask_add_match_buddies(&self) -> bool {
        self.read()
            .is_some_and(|ini| ini.section_or_empty(ASK).read_bool(ADD_MATCH_BUDDIES, true))
    }

    /// `WOLSettings__SetAddMatchBuddies` `0x00779A40`.
    pub(crate) fn set_ask_add_match_buddies(&self, ask: bool) {
        self.update(|bytes| put_string(bytes, ASK, ADD_MATCH_BUDDIES, yes_no(ask)));
    }

    /// `WOLPlayerInfo__LoadRecord` `0x00778A20`: the file is created when
    /// missing; a missing section is the default record `0x00778710`.
    pub(crate) fn load_persona(&self, name: &str, csf: &dyn Fn(&str) -> String) -> WolPersona {
        let ini = match self.read_or_create() {
            Ok(bytes) => IniFile::from_bytes(&bytes).unwrap_or_else(|_| IniFile::from_str("")),
            Err(error) => {
                log::warn!("Could not open {}: {error}", self.path.display());
                return WolPersona::default();
            }
        };
        match ini.section(&name.to_ascii_lowercase()) {
            Some(section) => WolPersona::read_named(section, csf),
            None => ini
                .section(DEFAULT_RECORD)
                .map(|section| WolPersona::read_default(section, csf))
                .unwrap_or_default(),
        }
    }

    /// `WOLInfo__SaveProfileToINI` `0x0077A080`: only a changed record is
    /// written; an empty name writes `[_Default_]`.
    pub(crate) fn save_persona(
        &self,
        persona: &mut WolPersona,
        name: &str,
        csf: &dyn Fn(&str) -> String,
    ) {
        if !persona.dirty {
            return;
        }
        let section = if name.is_empty() {
            DEFAULT_RECORD.to_owned()
        } else {
            name.to_ascii_lowercase()
        };
        self.update(|bytes| write_persona(bytes, &section, persona, csf));
        persona.dirty = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::map::source::test_support::TestDirectory;

    fn csf(key: &str) -> String {
        match key {
            "TXT_OCTOBER" => "October".into(),
            "TXT_MAY" => "May".into(),
            other => other.into(),
        }
    }

    fn file() -> (TestDirectory, WolInfoFile) {
        let directory = TestDirectory::new("wolinfo");
        let file = WolInfoFile::in_directory(directory.path());
        (directory, file)
    }

    #[test]
    fn a_named_record_reads_in_native_order_with_native_defaults() {
        let ini = IniFile::from_str(
            "[echo]\nServer=USA East\nLocale=4\nLocaleVerified=yes\nCountry=3\n\
             Rank=42\nWins=7\nMatchByRes=no\nUpdateTime=Oct 12:00\nUpdateDate=May 1 2002\n",
        );
        let persona = WolPersona::read_named(ini.section("echo").unwrap(), &csf);
        assert_eq!(persona.server, "USA East");
        assert_eq!((persona.locale, persona.locale_verified), (4, true));
        assert_eq!(persona.locale_priority, 1);
        assert_eq!((persona.country, persona.color), (3, -2));
        assert_eq!((persona.rank, persona.points, persona.wins), (42, -1, 7));
        assert_eq!((persona.losses, persona.disconnects), (-1, -1));
        assert!(!persona.match_by_res);
        // Only the time is translated; the date keeps its token.
        assert_eq!(persona.update_time, "October 12:00");
        assert_eq!(persona.update_date, "May 1 2002");
        let fresh = WolPersona::read_named(
            IniFile::from_str("[x]\nRank=-1\n").section("x").unwrap(),
            &csf,
        );
        assert_eq!(fresh.locale_priority, 3);
        assert!(fresh.match_by_res);
    }

    #[test]
    fn the_default_record_reads_only_its_six_keys() {
        let ini = IniFile::from_str(
            "[_Default_]\nServer=Europe\nLocale=2\nLocaleVerified=yes\nRank=9\nCountry=1\n",
        );
        let persona = WolPersona::read_default(ini.section(DEFAULT_RECORD).unwrap(), &csf);
        assert_eq!((persona.server.as_str(), persona.locale), ("Europe", 2));
        assert_eq!((persona.country, persona.color), (1, -2));
        assert!(!persona.locale_verified);
        assert_eq!((persona.rank, persona.locale_priority), (-1, 0));
    }

    #[test]
    fn the_locale_setter_follows_0x77a700() {
        let mut persona = WolPersona::default();
        persona.set_locale(0, 3);
        assert!(!persona.dirty);
        persona.set_locale(5, 3);
        assert_eq!((persona.locale, persona.locale_priority), (5, 3));
        assert!(persona.dirty && !persona.locale_verified);
        // The same locale from the service verifies it.
        persona.set_locale(5, 2);
        assert!(persona.locale_verified);
        assert_eq!(persona.locale_priority, 3);
        // A lower priority cannot replace a set locale.
        persona.set_locale(6, 2);
        assert_eq!(persona.locale, 5);
        persona.set_locale(6, 3);
        assert_eq!(persona.locale, 6);
        assert!(!persona.locale_verified);
        let mut verified = WolPersona {
            locale: 6,
            locale_verified: true,
            locale_priority: 1,
            ..WolPersona::default()
        };
        verified.set_locale(7, 2);
        assert_eq!((verified.locale, verified.locale_priority), (7, 2));
        assert!(verified.locale_verified);
    }

    #[test]
    fn the_my_information_setters_dirty_only_on_a_change() {
        let mut persona = WolPersona::default();
        persona.set_country(-2);
        persona.set_color(-2);
        persona.set_match_by_res(true);
        assert!(!persona.dirty);
        persona.set_country(3);
        assert!(persona.dirty && persona.country == 3);
        persona.dirty = false;
        persona.set_color(5);
        assert!(persona.dirty && persona.color == 5);
        persona.dirty = false;
        persona.set_match_by_res(false);
        assert!(persona.dirty && !persona.match_by_res);
    }

    #[test]
    fn getters_answer_their_defaults_without_the_file_and_create_nothing() {
        let (directory, file) = file();
        assert_eq!(file.auto_login(), "");
        assert_eq!(file.last_valid(), "");
        assert!(!file.ask_auto_login());
        assert!(!file.ask_add_match_buddies());
        assert!(!directory.path().join(WOLINFO_INI).exists());
        // Once the file exists, both [Ask] keys default to yes.
        file.set_auto_login("");
        assert!(file.ask_auto_login());
        assert!(file.ask_add_match_buddies());
        assert_eq!(
            std::fs::read(directory.path().join(WOLINFO_INI)).unwrap(),
            b"[DefaultPersona]\r\n"
        );
    }

    #[test]
    fn an_empty_value_removes_the_key_and_keeps_the_section() {
        let (directory, file) = file();
        file.set_auto_login("Echo");
        file.set_last_valid("Echo");
        assert_eq!(file.auto_login(), "Echo");
        file.set_auto_login("");
        assert_eq!(file.auto_login(), "");
        assert_eq!(file.last_valid(), "Echo");
        file.set_ask_auto_login(false);
        assert!(!file.ask_auto_login());
        file.set_ask_add_match_buddies(false);
        assert!(!file.ask_add_match_buddies());
        assert_eq!(
            std::fs::read_to_string(directory.path().join(WOLINFO_INI)).unwrap(),
            "[DefaultPersona]\r\nLastValid=Echo\r\n[Ask]\r\nAutoLogin=no\r\nAddMatchBuddies=no\r\n"
        );
    }

    #[test]
    fn a_persona_loads_lowercased_and_saves_only_when_changed() {
        let (directory, file) = file();
        let mut persona = file.load_persona("Echo", &csf);
        assert_eq!(persona, WolPersona::default());
        assert!(directory.path().join(WOLINFO_INI).exists());
        file.save_persona(&mut persona, "Echo", &csf);
        assert_eq!(
            std::fs::read(directory.path().join(WOLINFO_INI)).unwrap(),
            b""
        );
        persona.set_server("USA West");
        persona.update_time = "October 12:00".into();
        file.save_persona(&mut persona, "Echo", &csf);
        assert!(!persona.dirty);
        assert_eq!(
            std::fs::read_to_string(directory.path().join(WOLINFO_INI)).unwrap(),
            "[echo]\r\nServer=USA West\r\nLocale=0\r\nLocaleVerified=no\r\nCountry=-2\r\n\
             Color=-2\r\nRank=-1\r\nPoints=-1\r\nWins=-1\r\nLosses=-1\r\nDisconnects=-1\r\n\
             MatchByRes=yes\r\nUpdateTime=Oct 12:00\r\n"
        );
        assert_eq!(file.load_persona("ECHO", &csf).server, "USA West");
    }
}
