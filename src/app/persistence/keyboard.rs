//! Keyboard A3's explicit disk transactions, original5FB900/5FBA05/533D20.
//! A reload reads current loose bytes before the currently registered archive;
//! it never uses AssetManager's immutable startup loose-file snapshot.
use crate::app::input::hotkeys::HotkeyBindings;
use std::path::{Path, PathBuf};

fn path(root: &Path) -> PathBuf {
    // Windows case-insensitivity also retained on development/test hosts.
    crate::util::case_insensitive_path::find(root, "KEYBOARDMD.INI")
        .unwrap_or_else(|| root.join("KeyboardMD.ini"))
}
pub(crate) fn reload(root: &Path, archive: Option<&[u8]>) -> HotkeyBindings {
    let bytes = std::fs::read(path(root)).ok();
    HotkeyBindings::reload_from_ini_bytes(bytes.as_deref().or(archive))
}
pub(crate) fn save(root: &Path, bindings: &HotkeyBindings) -> std::io::Result<()> {
    std::fs::write(path(root), bindings.to_ini_string())
}
pub(crate) fn reset(root: &Path, archive: Option<&[u8]>) -> HotkeyBindings {
    // Native delete failure has no dialog: reload whatever file remains.
    if let Err(error) = std::fs::remove_file(path(root)) {
        if error.kind() != std::io::ErrorKind::NotFound {
            log::warn!("Could not reset KeyboardMD.ini: {error}");
        }
    }
    reload(root, archive)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::input::hotkeys::HotkeyCommand;
    #[test]
    fn disk_reload_and_reset_follow_current_file_not_entry_snapshot() {
        let root = std::env::temp_dir().join(format!("vera-keyboard-{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        let archive = b"[Hotkey]\nStopObject=83\nDelete=110\n";
        std::fs::write(root.join("KeyboardMD.ini"), b"[Hotkey]\nStopObject=81\n").unwrap();
        let mut edited = reload(&root, Some(archive));
        edited.assign(HotkeyCommand::StopObject, 88).unwrap();
        assert_eq!(
            reload(&root, Some(archive)).first_key(HotkeyCommand::StopObject),
            Some(81)
        );
        save(&root, &edited).unwrap();
        assert_eq!(
            reload(&root, Some(archive)).first_key(HotkeyCommand::StopObject),
            Some(88)
        );
        let defaults = reset(&root, Some(archive));
        assert_eq!(defaults.first_key(HotkeyCommand::StopObject), Some(83));
        assert_eq!(defaults.first_key(HotkeyCommand::Delete), Some(110));
        assert!(!root.join("KeyboardMD.ini").exists());
        assert_eq!(
            reload(&root, Some(archive)).first_key(HotkeyCommand::StopObject),
            Some(83)
        );
        std::fs::remove_dir(&root).unwrap();
    }
}
