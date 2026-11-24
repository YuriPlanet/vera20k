//! One root-file choice for Rules, fixed Art and standalone AI, plus optional
//! audio, theater, campaign and skirmish catalog names.
//!
//! Adapted from my ra2-mono GameConfig/GameIniLoader file selection. VERA
//! keeps one root per family and its existing native layer processing; this
//! does not import mono's multi-file AI merging. Archive selection is separate
//! from the file names a game reads.
//!
//! SOUNDMD and EVAMD are unlayered presentation readers, not part of the
//! rules/art/AI triplet: their overrides are optional for every profile and a
//! missing file keeps the tolerant empty audio catalog instead of failing the
//! root selection. The theater suffix is optional for every profile too and is
//! never probed: the theater loader reports its own missing INI.
//!
//! Optional catalogs keep their stock names when omitted. They are never
//! probed here; each reader retains its own missing-file behavior.

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum IniProfile {
    #[default]
    Yr,
    Auto,
    /// Original Red Alert 2 (non-md) control files. Every root is a fixed
    /// retail name from `ra2.mix -> local.mix`; no per-file override is needed.
    Ra2,
    Custom,
}

impl std::str::FromStr for IniProfile {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "yr" => Ok(Self::Yr),
            "auto" => Ok(Self::Auto),
            "ra2" => Ok(Self::Ra2),
            "custom" => Ok(Self::Custom),
            _ => Err(format!(
                "unknown INI profile '{value}'; use yr, auto, ra2 or custom with explicit file names"
            )),
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct IniRootConfig {
    pub profile: IniProfile,
    pub rules: Option<String>,
    pub art: Option<String>,
    pub ai: Option<String>,
    /// Optional SOUNDMD root for any profile. Defaults to `soundmd.ini`.
    pub sound: Option<String>,
    /// Optional EVAMD root for any profile. Defaults to `evamd.ini`.
    pub eva: Option<String>,
    /// Optional theater control-file suffix for any profile, the `MD` of
    /// `SNOWMD.INI`. Defaults to `md`.
    pub theater_suffix: Option<String>,
    pub theme: Option<String>,
    pub battle: Option<String>,
    pub mission: Option<String>,
    pub mapsel: Option<String>,
    pub modes: Option<String>,
    pub map_catalog: Option<String>,
}

/// Frozen file names, resolved once before any consumer builds game data.
/// This is host resource configuration, not a second Rules/AI state owner.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct IniRoots {
    profile: IniProfile,
    rules: String,
    art: String,
    ai: String,
    sound: String,
    eva: String,
    theater_suffix: String,
    theme: String,
    battle: String,
    mission: String,
    mapsel: String,
    modes: String,
    map_catalog: String,
}

impl Default for IniRoots {
    fn default() -> Self {
        Self {
            profile: IniProfile::Yr,
            rules: "rulesmd.ini".into(),
            art: "artmd.ini".into(),
            ai: "aimd.ini".into(),
            sound: "soundmd.ini".into(),
            eva: "evamd.ini".into(),
            theater_suffix: "md".into(),
            theme: "thememd.ini".into(),
            battle: "BATTLEMD.INI".into(),
            mission: "MISSIONMD.INI".into(),
            mapsel: "MAPSELMD.INI".into(),
            modes: "MPModesMD.ini".into(),
            map_catalog: "missionsmd.pkt".into(),
        }
    }
}

impl IniRoots {
    /// The original Red Alert 2 roots, all read from `ra2.mix -> local.mix`.
    /// RA2 predates the `MD` suffix: its theater control files are bare
    /// `SNOW.INI`/`URBAN.INI`, hence the empty theater suffix. The map catalog
    /// is `missions.pkt`, the RA2 campaign/skirmish packet.
    pub fn ra2() -> Self {
        Self {
            profile: IniProfile::Ra2,
            rules: "rules.ini".into(),
            art: "art.ini".into(),
            ai: "ai.ini".into(),
            sound: "sound.ini".into(),
            eva: "eva.ini".into(),
            theater_suffix: String::new(),
            theme: "theme.ini".into(),
            battle: "BATTLE.INI".into(),
            mission: "MISSION.INI".into(),
            mapsel: "MAPSEL.INI".into(),
            modes: "MPModes.ini".into(),
            map_catalog: "missions.pkt".into(),
        }
    }

    pub fn profile(&self) -> IniProfile {
        self.profile
    }
    pub fn rules(&self) -> &str {
        &self.rules
    }
    pub fn art(&self) -> &str {
        &self.art
    }
    pub fn ai(&self) -> &str {
        &self.ai
    }
    pub fn sound(&self) -> &str {
        &self.sound
    }
    pub fn eva(&self) -> &str {
        &self.eva
    }
    pub fn theater_suffix(&self) -> &str {
        &self.theater_suffix
    }

    /// The theater INI for a control-file name such as `snow`.
    /// `Read_Theater_TileSets_INI` (`0x00545150`) formats it with `"%sMD.INI"`
    /// (pushed at `0x005454FE`); this keeps that format with the selected
    /// suffix in place of `MD`.
    pub fn theater_ini(&self, control_name: &str) -> String {
        format!("{control_name}{}.ini", self.theater_suffix)
    }

    /// The Theme catalog: `THEMEMD.INI` (`0x00825D94`), pushed by Init_Game at
    /// `0x0052C8B9` for `ThemeClass::ReadINI` (`0x00720590`).
    pub fn theme(&self) -> &str {
        &self.theme
    }
    /// The campaign catalog: `BATTLEMD.INI` (`0x00826198`), read by
    /// Load_Campaigns (`0x0052CB90`) at `0x0052CC4E`/`0x0052CCCD`.
    pub fn battle(&self) -> &str {
        &self.battle
    }
    /// Mission metadata: `MISSIONMD.INI` (`0x00839724`), read by
    /// InitializeScenarioFromINI (`0x00686D46`) and, through `[0x00839710]`,
    /// by ShowMissionRestateBriefing at `0x0065F57F`.
    pub fn mission(&self) -> &str {
        &self.mission
    }
    /// The map-select stages: `MAPSELMD.INI` (`0x00830370`), read by
    /// MapSelect__LoadStagesFromINI (`0x005CEF62`).
    pub fn mapsel(&self) -> &str {
        &self.mapsel
    }

    /// The skirmish mode roster read by `0x005D7CE0`.
    pub fn modes(&self) -> &str {
        &self.modes
    }

    pub fn map_catalog(&self) -> &str {
        &self.map_catalog
    }

    pub fn names(&self) -> [&str; 3] {
        [self.rules(), self.art(), self.ai()]
    }

    /// Every root and catalog file: Rules, Art, AI, sound, EVA, theme,
    /// campaign, mission, map selection, mode and map catalog.
    pub fn files(&self) -> [&str; 11] {
        [
            self.rules(),
            self.art(),
            self.ai(),
            self.sound(),
            self.eva(),
            self.theme(),
            self.battle(),
            self.mission(),
            self.mapsel(),
            self.modes(),
            self.map_catalog(),
        ]
    }

    /// VERA: the roots whose names players' games compare: Rules, Art, AI,
    /// the mode roster and the map catalog, which hold what a match plays and
    /// which modes and maps a game offers. The others in [`Self::files`]
    /// (sound, EVA, theme, campaign, mission, map selection) name what a game
    /// plays aloud or shows, so two games may read them from other files.
    pub fn agreed_files(&self) -> [&str; 5] {
        [
            self.rules(),
            self.art(),
            self.ai(),
            self.modes(),
            self.map_catalog(),
        ]
    }

    /// Every file in [`Self::agreed_files`] is YR's, ignoring ASCII case.
    pub fn agreed_files_are_stock(&self) -> bool {
        self.agreed_files()
            .into_iter()
            .zip(Self::default().agreed_files())
            .all(|(used, stock)| used.eq_ignore_ascii_case(stock))
    }

    /// The [`Self::agreed_files`] and the theater suffix are YR's, ignoring
    /// ASCII case.
    pub fn is_stock(&self) -> bool {
        self.agreed_files_are_stock()
            && self
                .theater_suffix()
                .eq_ignore_ascii_case(Self::default().theater_suffix())
    }
}

impl IniRootConfig {
    pub fn validate(&self) -> Result<(), String> {
        let files = [
            self.rules.as_deref(),
            self.art.as_deref(),
            self.ai.as_deref(),
        ];
        if self.profile != IniProfile::Custom {
            if files.iter().any(Option::is_some) {
                return Err(
                    "rules/art/ai overrides require profile=custom and all three names".into(),
                );
            }
        } else {
            for (family, file) in ["rules", "art", "ai"].into_iter().zip(files) {
                let name = file.ok_or_else(|| format!("custom INI profile requires '{family}'"))?;
                validate_basename(family, name, ".ini")?;
            }
        }
        // Optional catalogs retain their readers' own missing-file policies
        // and never enter the required Rules/Art/AI completeness gate.
        for (family, name) in [
            ("sound", self.sound.as_deref()),
            ("eva", self.eva.as_deref()),
            ("theme", self.theme.as_deref()),
            ("battle", self.battle.as_deref()),
            ("mission", self.mission.as_deref()),
            ("mapsel", self.mapsel.as_deref()),
            ("modes", self.modes.as_deref()),
        ] {
            if let Some(name) = name {
                validate_basename(family, name, ".ini")?;
            }
        }
        if let Some(name) = self.map_catalog.as_deref() {
            validate_basename("map_catalog", name, ".pkt")?;
        }
        if let Some(suffix) = self.theater_suffix.as_deref()
            && (suffix.is_empty() || !suffix.bytes().all(|c| c.is_ascii_alphanumeric()))
        {
            return Err(format!(
                "theater_suffix must be letters and digits only: '{suffix}'"
            ));
        }
        Ok(())
    }

    /// Probe only the existing resource provider. Missing, unreadable and empty
    /// winners are unavailable; do not fall through to a lower-priority source.
    /// Optional catalogs are not probed: their readers keep the same missing-file
    /// behavior as for the default names. Auto validates only the stock triplet;
    /// other games name their triplet explicitly with Custom.
    pub fn resolve(&self, mut available: impl FnMut(&str) -> bool) -> Result<IniRoots, String> {
        self.validate()?;
        // Preserve legacy YR admission: consumers still report missing roots
        // at their original load stage. Merely constructing an asset manager
        // must not eagerly require AI in asset-only tools or small fixtures.
        // Ra2 starts from the fixed non-md set; optional roots below may still
        // override any name without disturbing the empty theater suffix.
        let mut selected = match self.profile {
            IniProfile::Ra2 => IniRoots::ra2(),
            _ => IniRoots::default(),
        };
        if self.profile == IniProfile::Custom {
            selected.profile = IniProfile::Custom;
            for (target, name) in [
                (&mut selected.rules, &self.rules),
                (&mut selected.art, &self.art),
                (&mut selected.ai, &self.ai),
            ] {
                if let Some(name) = name {
                    target.clone_from(name);
                }
            }
        }
        if self.profile != IniProfile::Yr {
            let missing: Vec<_> = selected
                .names()
                .into_iter()
                .filter(|name| !available(name))
                .collect();
            if !missing.is_empty() {
                return Err(format!(
                    "selected {:?} INI profile has missing, unreadable or empty roots: {}; use custom with a complete triplet for other file names; no files were mixed",
                    self.profile,
                    missing.join(", ")
                ));
            }
        }
        self.apply_optional_roots(&mut selected);
        Ok(selected)
    }

    fn apply_optional_roots(&self, roots: &mut IniRoots) {
        for (target, name) in [
            (&mut roots.sound, &self.sound),
            (&mut roots.eva, &self.eva),
            (&mut roots.theater_suffix, &self.theater_suffix),
            (&mut roots.theme, &self.theme),
            (&mut roots.battle, &self.battle),
            (&mut roots.mission, &self.mission),
            (&mut roots.mapsel, &self.mapsel),
            (&mut roots.modes, &self.modes),
            (&mut roots.map_catalog, &self.map_catalog),
        ] {
            if let Some(name) = name {
                target.clone_from(name);
            }
        }
    }
}

/// Root references are logical basenames, never host paths. Includes and the
/// existing VFS retain their own lookup policy.
fn validate_basename(family: &str, name: &str, extension: &str) -> Result<(), String> {
    if name.is_empty()
        || name.trim() != name
        || !name.to_ascii_lowercase().ends_with(extension)
        || name.len() == extension.len()
        || name.bytes().any(|c| {
            c < 32
                || matches!(
                    c,
                    b'/' | b'\\' | b':' | b'*' | b'?' | b'"' | b'<' | b'>' | b'|'
                )
        })
    {
        return Err(format!(
            "{family} root must be a plain nonempty {extension} basename: '{name}'"
        ));
    }
    Ok(())
}

#[cfg(all(test, tests_util))]
mod tests {
    use super::*;

    #[test]
    fn ini_roots_default_preserves_yr_without_probing() {
        let roots = IniRootConfig::default()
            .resolve(|_| panic!("legacy YR must not probe"))
            .unwrap();
        assert_eq!(roots.names(), ["rulesmd.ini", "artmd.ini", "aimd.ini"]);
        assert_eq!((roots.sound(), roots.eva()), ("soundmd.ini", "evamd.ini"));
        assert_eq!(
            [
                roots.theme(),
                roots.battle(),
                roots.mission(),
                roots.mapsel()
            ],
            [
                "thememd.ini",
                "BATTLEMD.INI",
                "MISSIONMD.INI",
                "MAPSELMD.INI"
            ]
        );
        assert_eq!(roots.modes(), "MPModesMD.ini");
        assert_eq!(roots.map_catalog(), "missionsmd.pkt");
    }

    #[test]
    fn ini_roots_ra2_uses_all_non_md_names_and_empty_theater_suffix() {
        let roots = IniRootConfig {
            profile: IniProfile::Ra2,
            ..Default::default()
        }
        // The three RA2 roots live in ra2.mix -> local.mix and must be probed.
        .resolve(|name| matches!(name, "rules.ini" | "art.ini" | "ai.ini"))
        .unwrap();
        assert_eq!(roots.profile(), IniProfile::Ra2);
        assert_eq!(roots.names(), ["rules.ini", "art.ini", "ai.ini"]);
        assert_eq!((roots.sound(), roots.eva()), ("sound.ini", "eva.ini"));
        assert_eq!(
            [
                roots.theme(),
                roots.battle(),
                roots.mission(),
                roots.mapsel()
            ],
            ["theme.ini", "BATTLE.INI", "MISSION.INI", "MAPSEL.INI"]
        );
        assert_eq!(roots.modes(), "MPModes.ini");
        assert_eq!(roots.map_catalog(), "missions.pkt");
        // RA2 predates the MD suffix: SNOW.INI, not SNOWMD.INI.
        assert_eq!(roots.theater_suffix(), "");
        assert_eq!(roots.theater_ini("snow"), "snow.ini");
        assert_eq!(roots.theater_ini("urban"), "urban.ini");
    }

    #[test]
    fn ini_roots_ra2_rejects_an_incomplete_triplet() {
        let request = IniRootConfig {
            profile: IniProfile::Ra2,
            ..Default::default()
        };
        assert!(
            request
                .resolve(|name| name != "ai.ini")
                .unwrap_err()
                .contains("ai.ini")
        );
    }

    #[test]
    fn ini_roots_ra2_optional_override_keeps_the_triplet() {
        let roots = IniRootConfig {
            profile: IniProfile::Ra2,
            eva: Some("eva-custom.ini".into()),
            ..Default::default()
        }
        .resolve(|_| true)
        .unwrap();
        assert_eq!(roots.names(), ["rules.ini", "art.ini", "ai.ini"]);
        assert_eq!(roots.eva(), "eva-custom.ini");
        // Overriding an optional root must not restore the MD suffix.
        assert_eq!(roots.theater_ini("snow"), "snow.ini");
    }

    #[test]
    fn ini_profile_parses_ra2() {
        assert_eq!("ra2".parse::<IniProfile>().unwrap(), IniProfile::Ra2);
        assert!("ra2x".parse::<IniProfile>().is_err());
    }

    #[test]
    fn ini_roots_auto_requires_only_the_complete_stock_triplet() {
        let request = IniRootConfig {
            profile: IniProfile::Auto,
            ..Default::default()
        };
        let mut probed = Vec::new();
        let roots = request
            .resolve(|name| {
                probed.push(name.to_owned());
                true
            })
            .unwrap();
        assert_eq!(roots.profile(), IniProfile::Yr);
        assert_eq!(probed, ["rulesmd.ini", "artmd.ini", "aimd.ini"]);
        assert!(
            request
                .resolve(|name| name != "aimd.ini")
                .unwrap_err()
                .contains("aimd.ini")
        );
    }

    #[test]
    fn ini_roots_optional_catalogs_use_explicit_names_without_probing() {
        let request = IniRootConfig {
            theme: Some("music-custom.ini".into()),
            battle: Some("campaigns-custom.ini".into()),
            mission: Some("missions-custom.ini".into()),
            mapsel: Some("stages-custom.ini".into()),
            modes: Some("modes-custom.ini".into()),
            map_catalog: Some("maps-custom.pkt".into()),
            ..Default::default()
        };
        let roots = request.resolve(|_| panic!("YR must not probe")).unwrap();
        assert_eq!(
            [
                roots.theme(),
                roots.battle(),
                roots.mission(),
                roots.mapsel()
            ],
            [
                "music-custom.ini",
                "campaigns-custom.ini",
                "missions-custom.ini",
                "stages-custom.ini"
            ]
        );
        assert_eq!(roots.modes(), "modes-custom.ini");
        assert_eq!(roots.map_catalog(), "maps-custom.pkt");
    }

    #[test]
    fn ini_roots_custom_requires_a_whole_explicit_basename_triplet() {
        let mut request = IniRootConfig {
            profile: IniProfile::Custom,
            rules: Some("my-rules.ini".into()),
            art: Some("my-art.ini".into()),
            ai: Some("my-ai.ini".into()),
            ..Default::default()
        };
        assert_eq!(request.resolve(|_| true).unwrap().rules(), "my-rules.ini");
        assert!(request.resolve(|name| name != "my-ai.ini").is_err());
        for bad in [
            "",
            "a.map",
            ".ini",
            " ../r.ini",
            "../r.ini",
            "a/r.ini",
            "a\\r.ini",
            "C:r.ini",
            "*.ini",
            "a\n.ini",
        ] {
            request.rules = Some(bad.into());
            assert!(request.validate().is_err(), "{bad}");
        }
        request.rules = None;
        assert!(request.validate().is_err());
        request.profile = IniProfile::Yr;
        assert!(request.validate().is_err());
    }

    #[test]
    fn ini_roots_audio_overrides_are_optional_and_do_not_probe() {
        // Omitted audio roots keep the shared names, including custom profiles.
        let custom = IniRootConfig {
            profile: IniProfile::Custom,
            rules: Some("r.ini".into()),
            art: Some("a.ini".into()),
            ai: Some("i.ini".into()),
            ..Default::default()
        };
        let roots = custom.resolve(|_| true).unwrap();
        assert_eq!((roots.sound(), roots.eva()), ("soundmd.ini", "evamd.ini"));

        // An override applies to the built-in profiles too. YR still never
        // probes, so these names stay presentation-only.
        let yr = IniRootConfig {
            sound: Some("my-sound.ini".into()),
            eva: Some("my-eva.ini".into()),
            ..Default::default()
        }
        .resolve(|_| panic!("legacy YR must not probe"))
        .unwrap();
        assert_eq!((yr.sound(), yr.eva()), ("my-sound.ini", "my-eva.ini"));

        let auto = IniRootConfig {
            profile: IniProfile::Auto,
            eva: Some("my-eva.ini".into()),
            ..Default::default()
        }
        .resolve(|name| matches!(name, "rulesmd.ini" | "artmd.ini" | "aimd.ini"))
        .unwrap();
        assert_eq!((auto.sound(), auto.eva()), ("soundmd.ini", "my-eva.ini"));
    }

    #[test]
    fn ini_roots_theater_suffix_uses_stock_or_explicit_names() {
        let yr = IniRootConfig::default()
            .resolve(|_| panic!("legacy YR must not probe"))
            .unwrap();
        assert_eq!(yr.theater_ini("snow"), "snowmd.ini");
        let custom = IniRootConfig {
            theater_suffix: Some("x2".into()),
            ..Default::default()
        };
        let roots = custom
            .resolve(|_| panic!("legacy YR must not probe"))
            .unwrap();
        assert_eq!(roots.theater_ini("temperat"), "temperatx2.ini");
        for bad in ["", "md.ini", "../md", "m d"] {
            let request = IniRootConfig {
                theater_suffix: Some(bad.into()),
                ..Default::default()
            };
            assert!(request.validate().is_err(), "{bad}");
        }
    }

    #[test]
    fn ini_roots_audio_roots_reject_host_paths() {
        for bad in [
            "",
            ".ini",
            "a.map",
            "../a.ini",
            "dir/a.ini",
            "a\\b.ini",
            "C:a.ini",
            "*.ini",
        ] {
            assert!(
                IniRootConfig {
                    sound: Some(bad.into()),
                    ..Default::default()
                }
                .validate()
                .is_err(),
                "sound {bad}"
            );
            assert!(
                IniRootConfig {
                    eva: Some(bad.into()),
                    ..Default::default()
                }
                .validate()
                .is_err(),
                "eva {bad}"
            );
        }
    }

    #[test]
    fn ini_roots_catalogs_reject_paths_and_wrong_extensions() {
        assert!(
            IniRootConfig {
                modes: Some("../modes.ini".into()),
                ..Default::default()
            }
            .validate()
            .is_err()
        );
        assert!(
            IniRootConfig {
                map_catalog: Some("maps.ini".into()),
                ..Default::default()
            }
            .validate()
            .is_err()
        );
    }
}
