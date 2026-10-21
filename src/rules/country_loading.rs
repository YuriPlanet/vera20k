//! Country loading presentation data. The retained Country extension owns Ares
//! reads; the loading attempt takes an immutable view before scenario reset.

/// Shared literal rows used by stock loading and Ares1001C0F0's ID defaults.
/// These are executable literals, not Country ordinal/Side inference in Ares.
#[derive(Debug, Clone, Copy)]
pub struct CountryLoadingDefaults {
    id: &'static str,
    token: &'static str,
    palette: &'static str,
    name: &'static str,
    special: &'static str,
    brief: &'static str,
}

impl CountryLoadingDefaults {
    pub fn art(&self) -> (&'static str, &'static str) {
        (self.token, self.palette)
    }
    pub fn labels(&self) -> [&'static str; 3] {
        [self.name, self.special, self.brief]
    }
}

const fn row(
    id: &'static str,
    token: &'static str,
    palette: &'static str,
    name: &'static str,
    special: &'static str,
    brief: &'static str,
) -> CountryLoadingDefaults {
    CountryLoadingDefaults {
        id,
        token,
        palette,
        name,
        special,
        brief,
    }
}

// Stock553412/5536DA/553A05/553D06 and Ares1001C0F0 literal tables.
const STOCK: [CountryLoadingDefaults; 10] = [
    row(
        "Americans",
        "ustates",
        "MPLSU.PAL",
        "Name:Americans",
        "Name:Para",
        "LoadBrief:USA",
    ),
    row(
        "Alliance",
        "korea",
        "MPLSK.PAL",
        "Name:Alliance",
        "Name:BEAGLE",
        "LoadBrief:Korea",
    ),
    row(
        "French",
        "france",
        "MPLSF.PAL",
        "Name:French",
        "Name:GTGCAN",
        "LoadBrief:French",
    ),
    row(
        "Germans",
        "germany",
        "MPLSG.PAL",
        "Name:Germans",
        "Name:TNKD",
        "LoadBrief:Germans",
    ),
    row(
        "British",
        "ukingdom",
        "MPLSUK.PAL",
        "Name:British",
        "Name:SNIPE",
        "LoadBrief:British",
    ),
    row(
        "Africans",
        "libya",
        "MPLSL.PAL",
        "Name:Africans",
        "Name:DTRUCK",
        "LoadBrief:Lybia",
    ),
    row(
        "Arabs",
        "iraq",
        "MPLSI.PAL",
        "Name:Arabs",
        "Name:DESO",
        "LoadBrief:Iraq",
    ),
    row(
        "Confederation",
        "cuba",
        "MPLSC.PAL",
        "Name:Confederation",
        "Name:TERROR",
        "LoadBrief:Cuba",
    ),
    row(
        "Russians",
        "russia",
        "MPLSR.PAL",
        "Name:Russians",
        "Name:TTNK",
        "LoadBrief:Russia",
    ),
    row(
        "YuriCountry",
        "yuri",
        "MPYLS.PAL",
        "Name:YuriCountry",
        "Name:YURI",
        "LoadBrief:YuriCountry",
    ),
];

pub fn stock_country_loading(index: usize) -> Option<&'static CountryLoadingDefaults> {
    STOCK.get(index)
}

/// CountryExt+2C/+4C, three CSF label records, and signed ColorScheme+11C.
/// Cached native CSF pointers are deliberately absent: save10076570 writes
/// labels only; load100764D0 resolves them against the fixed process catalog.
#[cfg(feature = "ares")]
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct AresCountryLoading {
    file: String,
    palette: String,
    name: String,
    special: String,
    brief: String,
    color: i32,
}

#[cfg(feature = "ares")]
impl Default for AresCountryLoading {
    fn default() -> Self {
        Self::for_country_id("")
    }
}

#[cfg(feature = "ares")]
impl AresCountryLoading {
    /// Constructor1001B767 and initializer1001C0F0 compare stored ID literals,
    /// regardless of the current registry position or Side.
    pub(crate) fn for_country_id(id: &str) -> Self {
        let defaults = STOCK.iter().find(|entry| entry.id.eq_ignore_ascii_case(id));
        Self {
            file: format!("ls%s{}.shp", defaults.map_or("obs", |entry| entry.token)),
            palette: defaults
                .map_or("MPLSOBS.PAL", |entry| entry.palette)
                .to_ascii_lowercase(),
            name: defaults.map_or("GUI:Unknown", |entry| entry.name).into(),
            special: defaults.map_or("GUI:Unknown", |entry| entry.special).into(),
            brief: defaults.map_or("GUI:Unknown", |entry| entry.brief).into(),
            color: -1,
        }
    }

    pub fn files(&self) -> (&str, &str) {
        (&self.file, &self.palette)
    }
    pub fn labels(&self) -> [&str; 3] {
        [&self.name, &self.special, &self.brief]
    }
    pub fn color_index(&self) -> i32 {
        self.color
    }

    /// Original5534DF formats this Country template with exactly one width
    /// argument (640 only on that exact branch, otherwise800). Authored fixed
    /// filenames stay fixed. Reject other printf directives rather than
    /// interpreting arbitrary stack arguments from malformed configuration.
    pub fn background_file(&self, narrow: bool) -> Option<String> {
        let mut result = String::with_capacity(self.file.len() + 3);
        let mut chars = self.file.chars();
        let mut used_width = false;
        while let Some(ch) = chars.next() {
            if ch != '%' {
                result.push(ch);
                continue;
            }
            match chars.next()? {
                '%' => result.push('%'),
                's' if !used_width => {
                    result.push_str(if narrow { "640" } else { "800" });
                    used_width = true;
                }
                _ => return None,
            }
        }
        Some(result)
    }

    /// CountryExt1001C770: only the actual primary Rules pass calls this.
    /// ReadString2048 is followed by a byte-preserving 31-character copy.
    /// INI storage represents native bytes as characters, not UTF-8 bytes.
    pub(crate) fn read_primary(&mut self, section: &crate::rules::ini_parser::IniSection) {
        for (key, field, file) in [
            ("File.LoadScreen", &mut self.file, true),
            ("File.LoadScreenPAL", &mut self.palette, true),
            ("LoadScreenText.Name", &mut self.name, false),
            ("LoadScreenText.SpecialName", &mut self.special, false),
            ("LoadScreenText.Brief", &mut self.brief, false),
        ] {
            if let Some(value) = section.read_name(key, 2048) {
                *field = if file && crate::rules::ini_parser::is_native_none_type_name(value) {
                    String::new()
                } else {
                    value.chars().take(31).collect()
                };
            }
        }
    }

    /// Common1001C686/1001CC70: Find-only, kind1, unknown retains, none=-1.
    /// The caller supplies the current authoritative ColorScheme registry.
    pub(crate) fn read_color(
        &mut self,
        section: &crate::rules::ini_parser::IniSection,
        find: impl Fn(&str) -> Option<i32>,
    ) {
        if let Some(value) = section.read_name("LoadScreenText.Color", 2048) {
            if let Some(index) = find(value) {
                self.color = index;
            } else if crate::rules::ini_parser::is_native_none_type_name(value) {
                self.color = -1;
            }
        }
    }

    /// First-phase1001C549 uses Side only for this color, not any GUI file/key.
    pub(crate) fn initialize_color(&mut self, side: i32, find: impl Fn(&str) -> Option<i32>) {
        let index = match side {
            0 => find("AlliedLoad"),
            1 => find("SovietLoad"),
            2 => find("YuriLoad").or_else(|| find("Purple")),
            _ => return,
        };
        //1001C56C/1001C594 stores Find's -1 too. Only an unrelated Side
        // skips the write; a missing named default must not retain old color.
        self.color = index.unwrap_or(-1);
    }
}
