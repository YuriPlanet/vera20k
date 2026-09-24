//! Case-insensitive file lookup in a retail install directory.
//!
//! Retail installs come from Windows, where file names ignore case; on macOS and
//! Linux hosts a name such as `RA2.ICO` or `KeyboardMD.ini` must match however the
//! install spells it.

use std::path::{Path, PathBuf};

/// The entry in `directory` whose name equals `name`, ignoring ASCII case.
pub fn find(directory: &Path, name: &str) -> Option<PathBuf> {
    std::fs::read_dir(directory)
        .ok()?
        .filter_map(Result::ok)
        .find(|entry| {
            entry
                .file_name()
                .to_string_lossy()
                .eq_ignore_ascii_case(name)
        })
        .map(|entry| entry.path())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_a_file_whatever_its_case() {
        let dir = std::env::temp_dir().join(format!("vera20k-case-path-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("Ra2.Ico"), b"icon").unwrap();

        assert_eq!(find(&dir, "RA2.ICO"), Some(dir.join("Ra2.Ico")));
        assert_eq!(find(&dir, "RA2MD.ICO"), None);
        assert_eq!(find(&dir.join("missing"), "RA2.ICO"), None);

        std::fs::remove_dir_all(&dir).unwrap();
    }
}
