//! One root-file choice for Rules, fixed Art and standalone AI.
//!
//! Adapted from my ra2-mono GameConfig/GameIniLoader file selection. VERA
//! keeps one root per family and its existing native layer processing; this
//! does not import mono's multi-file AI merging or change archive mounting.

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
}

/// Frozen file names, resolved once before any consumer builds game data.
/// This is host resource configuration, not a second Rules/AI state owner.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct IniRoots {
    profile: IniProfile,
    rules: String,
    art: String,
    ai: String,
}

impl Default for IniRoots {
    fn default() -> Self {
        Self::preset(IniProfile::Yr)
    }
}

impl IniRoots {
    fn preset(profile: IniProfile) -> Self {
        let (rules, art, ai) = match profile {
            IniProfile::Yr => ("rulesmd.ini", "artmd.ini", "aimd.ini"),
            IniProfile::Mo => ("rulesmo.ini", "artmo.ini", "aimo.ini"),
            _ => unreachable!("only concrete built-in profiles have presets"),
        };
        Self {
            profile,
            rules: rules.into(),
            art: art.into(),
            ai: ai.into(),
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
            return Ok(());
        }
        for (family, file) in ["rules", "art", "ai"].into_iter().zip(files) {
            let name = file.ok_or_else(|| format!("custom INI profile requires '{family}'"))?;
            // Root references are logical basenames, never host paths. Includes
            // and the existing VFS retain their own lookup policy.
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
                    "custom {family} must be a plain nonempty INI basename: '{name}'"
                ));
            }
        }
        Ok(())
    }

    /// Probe only the existing resource provider. Missing, unreadable and empty
    /// winners are unavailable; do not fall through to a lower-priority source.
    pub fn resolve(&self, mut available: impl FnMut(&str) -> bool) -> Result<IniRoots, String> {
        self.validate()?;
        // Preserve legacy YR admission: consumers still report missing roots
        // at their original load stage. Merely constructing an asset manager
        // must not eagerly require AI in asset-only tools or small fixtures.
        if self.profile == IniProfile::Yr {
            return Ok(IniRoots::default());
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
            return match (yr_count, mo_count) {
                (3, 0) => Ok(yr),
                (0, 3) => Ok(mo),
                _ => Err(format!(
                    "automatic INI selection is ambiguous or incomplete (YR {yr_count}/3, MO {mo_count}/3); explicitly select yr/mo or a complete custom profile; no files were mixed"
                )),
            };
        }
        let selected = if self.profile == IniProfile::Mo {
            IniRoots::preset(IniProfile::Mo)
        } else {
            IniRoots {
                profile: IniProfile::Custom,
                rules: self.rules.clone().unwrap(),
                art: self.art.clone().unwrap(),
                ai: self.ai.clone().unwrap(),
            }
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
        Ok(selected)
    }
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
}
