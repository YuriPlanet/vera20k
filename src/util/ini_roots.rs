//! One root-file choice for Rules, fixed Art and standalone AI, plus optional
//! SOUNDMD/EVAMD roots.
//!
//! Adapted from my ra2-mono GameConfig/GameIniLoader file selection. VERA
//! keeps one root per family and its existing native layer processing; this
//! does not import mono's multi-file AI merging or change archive mounting.
//!
//! SOUNDMD and EVAMD are unlayered presentation readers, not part of the
//! rules/art/AI triplet: their overrides are optional for every profile and a
//! missing file keeps the tolerant empty audio catalog instead of failing the
//! root selection.

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum IniProfile {
    #[default]
    Yr,
    Mo,
    Auto,
    Custom,
}

impl std::str::FromStr for IniProfile {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "yr" => Ok(Self::Yr),
            "mo" => Ok(Self::Mo),
            "auto" => Ok(Self::Auto),
            "custom" => Ok(Self::Custom),
            _ => Err(format!(
                "unknown INI profile '{value}'; use yr, mo, auto or custom"
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
    /// Optional SOUNDMD root for any profile. Absent keeps the profile's
    /// preset: `soundmd.ini` for YR/custom, `soundmo.ini` for MO.
    pub sound: Option<String>,
    /// Optional EVAMD root for any profile. Absent keeps the profile's preset:
    /// `evamd.ini` for YR/custom, `evamo.ini` for MO.
    pub eva: Option<String>,
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
}

impl Default for IniRoots {
    fn default() -> Self {
        Self::preset(IniProfile::Yr)
    }
}

impl IniRoots {
    fn preset(profile: IniProfile) -> Self {
        let (rules, art, ai, sound, eva) = match profile {
            IniProfile::Yr => (
                "rulesmd.ini",
                "artmd.ini",
                "aimd.ini",
                "soundmd.ini",
                "evamd.ini",
            ),
            IniProfile::Mo => (
                "rulesmo.ini",
                "artmo.ini",
                "aimo.ini",
                "soundmo.ini",
                "evamo.ini",
            ),
            _ => unreachable!("only concrete built-in profiles have presets"),
        };
        Self {
            profile,
            rules: rules.into(),
            art: art.into(),
            ai: ai.into(),
            sound: sound.into(),
            eva: eva.into(),
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

    pub fn names(&self) -> [&str; 3] {
        [self.rules(), self.art(), self.ai()]
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
                validate_ini_basename(family, name)?;
            }
        }
        // SOUNDMD/EVAMD are optional presentation roots for every profile:
        // validated here, but never required and never part of the
        // rules/art/AI completeness gate.
        for (family, name) in [
            ("sound", self.sound.as_deref()),
            ("eva", self.eva.as_deref()),
        ] {
            if let Some(name) = name {
                validate_ini_basename(family, name)?;
            }
        }
        Ok(())
    }

    /// Probe only the existing resource provider. Missing, unreadable and empty
    /// winners are unavailable; do not fall through to a lower-priority source.
    /// SOUNDMD/EVAMD overrides are not probed: a missing presentation file keeps
    /// the audio reader's tolerant empty catalog, as it does for the fixed names.
    pub fn resolve(&self, mut available: impl FnMut(&str) -> bool) -> Result<IniRoots, String> {
        self.validate()?;
        // Preserve legacy YR admission: consumers still report missing roots
        // at their original load stage. Merely constructing an asset manager
        // must not eagerly require AI in asset-only tools or small fixtures.
        if self.profile == IniProfile::Yr {
            let mut roots = IniRoots::default();
            self.apply_audio_roots(&mut roots);
            return Ok(roots);
        }
        if self.profile == IniProfile::Auto {
            let yr = IniRoots::preset(IniProfile::Yr);
            let mo = IniRoots::preset(IniProfile::Mo);
            let yr_count = yr
                .names()
                .into_iter()
                .filter(|name| available(name))
                .count();
            let mo_count = mo
                .names()
                .into_iter()
                .filter(|name| available(name))
                .count();
            let mut selected = match (yr_count, mo_count) {
                (3, 0) => yr,
                (0, 3) => mo,
                _ => {
                    return Err(format!(
                        "automatic INI selection is ambiguous or incomplete (YR {yr_count}/3, MO {mo_count}/3); explicitly select yr/mo or a complete custom profile; no files were mixed"
                    ));
                }
            };
            self.apply_audio_roots(&mut selected);
            return Ok(selected);
        }
        let mut selected = if self.profile == IniProfile::Mo {
            IniRoots::preset(IniProfile::Mo)
        } else {
            // Start from the YR preset so the optional audio roots keep one
            // default source, then replace the required triplet.
            let mut custom = IniRoots::preset(IniProfile::Yr);
            custom.profile = IniProfile::Custom;
            custom.rules = self.rules.clone().unwrap();
            custom.art = self.art.clone().unwrap();
            custom.ai = self.ai.clone().unwrap();
            custom
        };
        let missing: Vec<_> = selected
            .names()
            .into_iter()
            .filter(|name| !available(name))
            .collect();
        if !missing.is_empty() {
            return Err(format!(
                "selected {:?} INI profile has missing, unreadable or empty roots: {}; no YR fallback is allowed",
                selected.profile,
                missing.join(", ")
            ));
        }
        self.apply_audio_roots(&mut selected);
        Ok(selected)
    }

    fn apply_audio_roots(&self, roots: &mut IniRoots) {
        if let Some(sound) = &self.sound {
            roots.sound.clone_from(sound);
        }
        if let Some(eva) = &self.eva {
            roots.eva.clone_from(eva);
        }
    }
}

/// Root references are logical basenames, never host paths. Includes and the
/// existing VFS retain their own lookup policy.
fn validate_ini_basename(family: &str, name: &str) -> Result<(), String> {
    if name.is_empty()
        || name.trim() != name
        || !name.to_ascii_lowercase().ends_with(".ini")
        || name == ".ini"
        || name.bytes().any(|c| {
            c < 32
                || matches!(
                    c,
                    b'/' | b'\\' | b':' | b'*' | b'?' | b'"' | b'<' | b'>' | b'|'
                )
        })
    {
        return Err(format!(
            "{family} root must be a plain nonempty INI basename: '{name}'"
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ini_roots_default_preserves_yr_without_probing() {
        let roots = IniRootConfig::default()
            .resolve(|_| panic!("legacy YR must not probe"))
            .unwrap();
        assert_eq!(roots.names(), ["rulesmd.ini", "artmd.ini", "aimd.ini"]);
        assert_eq!((roots.sound(), roots.eva()), ("soundmd.ini", "evamd.ini"));
    }

    #[test]
    fn ini_roots_auto_accepts_only_one_complete_unmixed_set() {
        for (files, expected) in [
            (
                vec!["rulesmd.ini", "artmd.ini", "aimd.ini"],
                Some(IniProfile::Yr),
            ),
            (
                vec!["rulesmo.ini", "artmo.ini", "aimo.ini"],
                Some(IniProfile::Mo),
            ),
            (
                vec!["rulesmd.ini", "artmd.ini", "aimd.ini", "rulesmo.ini"],
                None,
            ),
            (vec!["rulesmo.ini", "artmo.ini"], None),
            (
                vec![
                    "rulesmd.ini",
                    "artmd.ini",
                    "aimd.ini",
                    "rulesmo.ini",
                    "artmo.ini",
                    "aimo.ini",
                ],
                None,
            ),
            (vec![], None),
        ] {
            let result = IniRootConfig {
                profile: IniProfile::Auto,
                ..Default::default()
            }
            .resolve(|name| files.contains(&name));
            assert_eq!(
                result.ok().map(|roots| roots.profile()),
                expected,
                "{files:?}"
            );
        }
    }

    #[test]
    fn ini_roots_explicit_mo_ignores_yr_and_rejects_missing_ai() {
        let request = IniRootConfig {
            profile: IniProfile::Mo,
            ..Default::default()
        };
        let roots = request.resolve(|_| true).unwrap();
        assert_eq!(roots.names(), ["rulesmo.ini", "artmo.ini", "aimo.ini"]);
        assert_eq!((roots.sound(), roots.eva()), ("soundmo.ini", "evamo.ini"));
        assert!(
            request
                .resolve(|name| name != "aimo.ini")
                .unwrap_err()
                .contains("aimo.ini")
        );
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
        request.profile = IniProfile::Mo;
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

        // MO's preset carries its own SOUNDMO/EVAMO pair; Auto follows the
        // family its rules triplet selected.
        let mo = IniRootConfig {
            profile: IniProfile::Mo,
            ..Default::default()
        }
        .resolve(|_| true)
        .unwrap();
        assert_eq!((mo.sound(), mo.eva()), ("soundmo.ini", "evamo.ini"));

        let auto_mo = IniRootConfig {
            profile: IniProfile::Auto,
            ..Default::default()
        }
        .resolve(|name| matches!(name, "rulesmo.ini" | "artmo.ini" | "aimo.ini"))
        .unwrap();
        assert_eq!(auto_mo.profile(), IniProfile::Mo);
        assert_eq!((auto_mo.sound(), auto_mo.eva()), ("soundmo.ini", "evamo.ini"));

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
}
