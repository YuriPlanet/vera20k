//! VERARULES.INI, retired. The game read the player's own rule keys from it
//! beside VERA.INI until New Mod gave players the whole rules files to change
//! (docs/modding.md). It is no longer read: a start takes away the commented
//! starter the game itself wrote, and tells a player whose file still holds
//! changes where they go now. Nothing else of the player's is touched.

use std::path::Path;

use crate::util::case_insensitive_path::find;

const FILENAME: &str = "VERARULES.INI";

/// The starter released versions wrote when the player had none: comments
/// only, so a copy that still holds exactly this was never changed. Any other
/// comments-only starter stays, unmentioned.
const STARTER: &[u8] = b"; Personal gameplay changes. Original game files are never changed.
; Restart VERA and start a new game after editing this file.
; Remove the leading ; from a section and its settings to enable them.

; Map and campaign rules take priority by default.
; Set OverrideMapRules=yes to override mission/map rules with your changes.
;[VERA]
;OverrideMapRules=no

; GI example values, not game defaults.
;[E1]
;Cost=100
;Strength=200
";

/// The player's VERARULES.INI in `directory`, at a start: the untouched
/// starter goes, and a file with settings that are not commented out gives
/// the notice that says it is no longer read. A file of comments stays,
/// unmentioned.
pub(crate) fn retire(directory: &Path) -> Option<String> {
    let path = find(directory, FILENAME)?;
    let bytes = match crate::util::file_system::read(&path) {
        Ok(bytes) => bytes,
        Err(error) => {
            log::warn!("{} could not be read: {error}", path.display());
            return None;
        }
    };
    if bytes == STARTER {
        if let Err(error) = crate::util::file_system::remove_file(&path) {
            log::warn!("{} could not be removed: {error}", path.display());
        }
        return None;
    }
    // `[VERA]` held only the reader's own switch, OverrideMapRules, which
    // changed no rule by itself, so a file of only that has nothing to keep.
    let settings = crate::rules::ini_parser::IniFile::from_bytes(&bytes).map_or(true, |ini| {
        ini.section_names()
            .iter()
            .any(|name| !name.eq_ignore_ascii_case("VERA"))
    });
    if !settings {
        return None;
    }
    log::warn!(
        "{} holds settings, which are no longer read",
        path.display()
    );
    Some(crate::i18n::tr_fill(
        "VERA:RulesFileRetired",
        &[("file", &path.display())],
    ))
}

#[cfg(all(test, tests_app))]
mod tests {
    use super::*;

    /// The starter the game wrote goes; a file with settings stays where it
    /// is, as it is, and gives the notice; a file of comments and the
    /// reader's own `[VERA]` switch stays, with none.
    #[test]
    fn a_start_removes_the_starter_and_names_a_file_with_settings() {
        let directory =
            std::env::temp_dir().join(format!("vera-rules-retired-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&directory);
        std::fs::create_dir_all(&directory).unwrap();
        assert_eq!(retire(&directory), None);

        std::fs::write(directory.join(FILENAME), STARTER).unwrap();
        assert_eq!(retire(&directory), None);
        assert_eq!(std::fs::read_dir(&directory).unwrap().count(), 0);

        let path = directory.join("VeRaRuLeS.InI");
        let custom = b"[E1]\r\nCost=75\r\n; personal note \xe9\r\n";
        std::fs::write(&path, custom).unwrap();
        assert!(retire(&directory).is_some());
        assert_eq!(std::fs::read(&path).unwrap(), custom);

        let comments = b";[E1]\r\n;Cost=75\r\n[VERA]\r\nOverrideMapRules=yes\r\n";
        std::fs::write(&path, comments).unwrap();
        assert_eq!(retire(&directory), None);
        assert_eq!(std::fs::read(&path).unwrap(), comments);
        std::fs::remove_dir_all(directory).unwrap();
    }
}
