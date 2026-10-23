//! The player's optional rules overrides, beside their VERA.INI preferences.

use std::io;
use std::path::Path;

use crate::util::case_insensitive_path::find;

pub(crate) const FILENAME: &str = "VERARULES.INI";

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

/// Create an inactive starter only when missing; always load the player's
/// existing spelling and bytes without replacing their edits.
pub(crate) fn load_or_create(directory: &Path) -> io::Result<Vec<u8>> {
    let path = find(directory, FILENAME).unwrap_or_else(|| directory.join(FILENAME));
    match std::fs::read(&path) {
        Ok(bytes) => Ok(bytes),
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            std::fs::create_dir_all(directory)?;
            super::profile_file::create_new(&path, STARTER)?;
            std::fs::read(path)
        }
        Err(error) => Err(error),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rules::ini_parser::IniFile;

    #[test]
    fn creates_an_inactive_starter_in_a_new_directory() {
        let root = std::env::temp_dir().join(format!("vera-rules-starter-{}", std::process::id()));
        let directory = root.join("profile");
        let bytes = load_or_create(&directory).unwrap();
        assert_eq!(bytes, STARTER);
        assert_eq!(std::fs::read(directory.join(FILENAME)).unwrap(), bytes);
        assert!(IniFile::from_bytes(&bytes).unwrap().is_empty());
        assert!(
            bytes
                .split(|&byte| byte == b'\n')
                .all(|line| { line.is_empty() || line.starts_with(b";") })
        );
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn preserves_existing_rules_with_case_insensitive_filename() {
        let directory =
            std::env::temp_dir().join(format!("vera-rules-existing-{}", std::process::id()));
        std::fs::create_dir_all(&directory).unwrap();
        let path = directory.join("VeRaRuLeS.InI");
        let custom = b"[E1]\r\nCost=75\r\n; personal note \xe9\r\n";
        std::fs::write(&path, custom).unwrap();
        assert_eq!(load_or_create(&directory).unwrap(), custom);
        assert_eq!(std::fs::read(&path).unwrap(), custom);
        assert_eq!(std::fs::read_dir(&directory).unwrap().count(), 1);
        std::fs::remove_dir_all(directory).unwrap();
    }
}
