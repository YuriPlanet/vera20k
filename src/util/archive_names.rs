//! Data-selected names for the existing startup archive slots.
//!
//! A mod can rename these files without changing when they mount or which
//! native fallbacks run. Names never select another directory or executable.

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct ArchiveNames {
    pub expansion_prefix: String,
    pub map_prefix: String,
    pub multiplayer: String,
    pub theme: String,
    pub movie_prefix: String,
}

impl Default for ArchiveNames {
    fn default() -> Self {
        Self {
            expansion_prefix: "expandmd".into(),
            map_prefix: "mapsmd".into(),
            multiplayer: "multimd.mix".into(),
            theme: "thememd.mix".into(),
            movie_prefix: "movmd".into(),
        }
    }
}

impl ArchiveNames {
    /// Original Red Alert 2 (non-md) archive names. Yuri's Revenge shipped a
    /// parallel `md` archive family alongside the RA2 originals and mounted
    /// the md member first, so its data won. Original RA2 has only the base
    /// names. `-ra2` selects this set so the whole resource stack — maps,
    /// multiplayer, theme and movies — reads the base archives.
    pub fn ra2() -> Self {
        Self {
            expansion_prefix: "expand".into(),
            map_prefix: "maps".into(),
            multiplayer: "multi.mix".into(),
            theme: "theme.mix".into(),
            movie_prefix: "movies".into(),
        }
    }

    pub fn validate(&self) -> Result<(), String> {
        for (field, prefix) in [
            ("expansion_prefix", &self.expansion_prefix),
            ("map_prefix", &self.map_prefix),
            ("movie_prefix", &self.movie_prefix),
        ] {
            // Leave room for the numbered archive's two digits and .mix.
            if prefix.len() > 249 || !plain_stem(prefix) {
                return Err(format!(
                    "archive {field} must be a nonempty prefix of ASCII letters, digits, _ or -"
                ));
            }
        }
        for (field, name) in [("multiplayer", &self.multiplayer), ("theme", &self.theme)] {
            let stem = name
                .get(..name.len().saturating_sub(4))
                .filter(|_| name.to_ascii_lowercase().ends_with(".mix"));
            if name.len() > 255
                || !stem.is_some_and(|stem| plain_stem(stem) && !reserved_device(stem))
            {
                return Err(format!(
                    "archive {field} must be a plain .mix filename of ASCII letters, digits, _ or -"
                ));
            }
        }
        Ok(())
    }
}

fn plain_stem(name: &str) -> bool {
    !name.is_empty()
        && name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
}

fn reserved_device(stem: &str) -> bool {
    let upper = stem.to_ascii_uppercase();
    matches!(upper.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        || (upper.len() == 4
            && (upper.starts_with("COM") || upper.starts_with("LPT"))
            && upper.as_bytes()[3].is_ascii_digit())
}

#[cfg(all(test, tests_util))]
mod tests {
    use super::*;

    #[test]
    fn archive_names_refuse_paths_and_non_files_before_mounting() {
        assert!(ArchiveNames::default().validate().is_ok());
        let mut names = ArchiveNames::default();
        names.expansion_prefix = "../outside".into();
        assert!(names.validate().is_err());
        names.expansion_prefix = "expansion_custom".into();
        names.multiplayer = "C:\\outside.mix".into();
        assert!(names.validate().is_err());
        names.multiplayer = "CON.mix".into();
        assert!(names.validate().is_err());
        names.multiplayer = "custom.MIX".into();
        assert!(names.validate().is_ok());
    }

    #[test]
    fn ra2_archive_names_are_all_non_md_and_valid() {
        let names = ArchiveNames::ra2();
        assert!(names.validate().is_ok());
        assert_eq!(names.expansion_prefix, "expand");
        assert_eq!(names.map_prefix, "maps");
        assert_eq!(names.multiplayer, "multi.mix");
        assert_eq!(names.theme, "theme.mix");
        assert_eq!(names.movie_prefix, "movies");
        for value in [
            &names.expansion_prefix,
            &names.map_prefix,
            &names.multiplayer,
            &names.theme,
            &names.movie_prefix,
        ] {
            assert!(!value.contains("md"), "RA2 name contains md: {value}");
        }
    }
}
