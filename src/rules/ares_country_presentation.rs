//! Installed Ares 3.0 country presentation state.
//!
//! `HouseTypeExt::Initialize @ 0x1001C0F0`, called by the extension
//! constructor (`0x1001B911`), writes the stock flag, status, taunt and
//! observer defaults by comparing the stored Country ID against its literal
//! name table (`0x1001C107..0x1001C135`, shared with the loading rows in
//! [`crate::rules::country_loading`]); the unknown branch at `0x1001C413`
//! supplies `rani.pcx`, `GUI:Unknown` and `taunts\tauam%02i.wav`. The
//! primary-Rules block `0x1001C770` reads the file/status keys before the
//! one-shot `ParentCountry` copy `0x1001BDC0`, which overwrites them; the
//! common block `0x1001C5D0` then reads the weights and `ListIndex`. These
//! values are GUI/process state, not simulation CRC inputs.

#[cfg(feature = "ares")]
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct AresCountryPresentation {
    /// `+0x08` `File.Flag=` (PCX, lowercased, at most 31 bytes). `None` is
    /// the cleared file, which the flag hook answers with `rani.pcx`.
    pub flag: Option<String>,
    /// `+0xF8` `MenuText.Status=` CSF key.
    pub status: Option<String>,
    /// `+0x6C` `File.Taunt=` pattern, normally `Taunts\tauam~~.wav` or the
    /// stock `taunts\tauam%02i.wav`.
    pub taunt: Option<String>,
    /// `+0x168` `File.ObserverBackground=` (PCX or SHP name).
    pub observer_background: Option<String>,
    /// `+0x190` `File.ObserverFlag=` (PCX or SHP name).
    pub observer_flag: Option<String>,
    /// `+0x1B8` `File.ObserverFlagAltPalette=`.
    pub observer_flag_alt_palette: bool,
    /// `+0x12C` `ListIndex=`; a negative value hides the country.
    pub list_index: i32,
    /// `+0x120` `RandomSelectionWeight=`.
    pub random_weight: i32,
    /// `+0x124`/`+0x128` `AIRandomSelectionWeight=` (Nullable<int>); unset
    /// until a read succeeds, and never copied from a parent.
    pub ai_random_weight: Option<i32>,
}

#[cfg(feature = "ares")]
impl Default for AresCountryPresentation {
    fn default() -> Self {
        Self::for_country_id("")
    }
}

#[cfg(feature = "ares")]
impl AresCountryPresentation {
    /// Constructor `0x1001B767..0x1001B908` (`ListIndex` 100, weight 1, AI
    /// weight unset), then `HouseTypeExt::Initialize @ 0x1001C0F0` by stored
    /// ID, regardless of the Country's registry position.
    pub(crate) fn for_country_id(id: &str) -> Self {
        let row = crate::rules::country_loading::stock_country_by_id(id);
        let [flag, status, taunt] = row.map_or(
            [
                crate::rules::ruleset::STOCK_UNKNOWN_INSIGNIA,
                "GUI:Unknown",
                "taunts\\tauam%02i.wav",
            ],
            |row| row.presentation(),
        );
        let observer = row.map(|row| row.observer());
        Self {
            flag: Some(flag.to_owned()),
            status: Some(status.to_owned()),
            taunt: Some(taunt.to_owned()),
            observer_background: observer.map(|(background, _, _)| background.to_owned()),
            observer_flag: observer.map(|(_, flag, _)| flag.to_owned()),
            observer_flag_alt_palette: observer.is_some_and(|(_, _, alt)| alt),
            list_index: 100,
            random_weight: 1,
            ai_random_weight: None,
        }
    }

    /// The weight an AI "Random" row draws with (`0x1001CA11..0x1001CA1A`):
    /// `AIRandomSelectionWeight` once set, else `RandomSelectionWeight`.
    pub(crate) fn ai_weight(&self) -> i32 {
        self.ai_random_weight.unwrap_or(self.random_weight)
    }

    /// Primary-Rules block `0x1001C770`. Every key keeps its value when
    /// ReadString returns 0 (absent or empty) and `none`/`<none>` clears a
    /// file (`0x10003950`).
    /// - `File.Flag` (`0x1001C79D..0x1001C7E5`): the first 31 bytes,
    ///   lowercased (`0x10076470`).
    /// - Observer files (`0x1001B940`): the first 31 bytes. The native SHP
    ///   vs `.pcx` split has no consumer and is not modeled.
    /// - `File.Taunt` (0x20 fixed string, `0x1001C8A1`): the first 31 bytes.
    /// - `MenuText.Status` (CSF label `0x10076510`): the first 31 bytes;
    ///   `none` is an ordinary label.
    ///
    /// RESIDUAL: native also clears `File.Flag` and the observer files when
    /// the named PCX/SHP fails to load (`0x1001C7CA..0x1001C7E5`,
    /// `0x1001B9DA..0x1001B9F3`). VERA keeps the name; see the docs page's
    /// missing-file residual.
    pub(crate) fn read_primary(&mut self, section: &crate::rules::ini_parser::IniSection) {
        use crate::rules::ini_parser::is_native_none_type_name;
        use crate::rules::ini_value::truncate_native_bytes;
        if let Some(flag) = section.read_ares_pcx_filename("File.Flag") {
            self.flag = (!is_native_none_type_name(&flag)).then_some(flag);
        }
        for (key, target) in [
            ("File.ObserverFlag", &mut self.observer_flag),
            ("File.ObserverBackground", &mut self.observer_background),
            ("File.Taunt", &mut self.taunt),
        ] {
            if let Some(value) = section.read_name(key, 0x800) {
                *target = (!is_native_none_type_name(value))
                    .then(|| truncate_native_bytes(value, 0x1f).to_owned());
            }
        }
        if let Some(value) = section.read_name("MenuText.Status", 0x800) {
            self.status = Some(truncate_native_bytes(value, 0x1f).to_owned());
        }
        self.observer_flag_alt_palette = section.read_ares_bool(
            "File.ObserverFlagAltPalette",
            self.observer_flag_alt_palette,
        );
    }

    /// Common block `0x1001C69C..0x1001C6D9`, after the parent copy:
    /// `RandomSelectionWeight`, the optional `AIRandomSelectionWeight`
    /// (`0x10008CC0`), then `ListIndex`.
    pub(crate) fn read_common(&mut self, section: &crate::rules::ini_parser::IniSection) {
        self.random_weight = section.read_ares_int("RandomSelectionWeight", self.random_weight);
        self.ai_random_weight =
            section.read_ares_nullable_int("AIRandomSelectionWeight", self.ai_random_weight);
        self.list_index = section.read_ares_int("ListIndex", self.list_index);
    }

    /// `HouseTypeExt::InheritSettings @ 0x1001BDC0`: the parent's files,
    /// status, taunt, observer art, `RandomSelectionWeight` and `ListIndex`
    /// + 1. The AI weight (`+0x124`/`+0x128`) is not copied.
    pub(crate) fn inherit_from(&mut self, parent: &Self) {
        self.flag = parent.flag.clone();
        self.status = parent.status.clone();
        self.taunt = parent.taunt.clone();
        self.observer_background = parent.observer_background.clone();
        self.observer_flag = parent.observer_flag.clone();
        self.observer_flag_alt_palette = parent.observer_flag_alt_palette;
        self.list_index = parent.list_index.wrapping_add(1);
        self.random_weight = parent.random_weight;
    }
}
