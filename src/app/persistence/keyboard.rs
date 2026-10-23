//! Keyboard A3's explicit disk transactions, original5FB900/5FBA05/533D20.
//! VERA.INI owns [Hotkey]; the retail archive supplies reset defaults.
use super::profile_file;
use crate::app::input::hotkeys::HotkeyBindings;
use crate::rules::ini_parser::IniFile;
use crate::util::ini_writer::replace_ini_section;
use std::path::Path;

fn read(root: &Path) -> Option<Vec<u8>> {
    let bytes = std::fs::read(profile_file::path(root)).ok()?;
    // An options-only profile must still use the archive's keyboard defaults.
    IniFile::from_bytes(&bytes).ok()?.section("Hotkey")?;
    Some(bytes)
}
pub(crate) fn reload(root: &Path, archive: Option<&[u8]>) -> HotkeyBindings {
    let bytes = read(root);
    HotkeyBindings::reload_from_ini_bytes(bytes.as_deref().or(archive))
}
/// Process startup shares the profile file owner, but retains the initial
/// Escape/Delete/Space installation that a dialog reload intentionally omits.
pub(crate) fn startup(root: &Path, archive: Option<&[u8]>) -> HotkeyBindings {
    let bytes = read(root);
    HotkeyBindings::from_ini_bytes(bytes.as_deref().or(archive))
}
pub(crate) fn save(root: &Path, bindings: &HotkeyBindings) -> std::io::Result<()> {
    let ini = IniFile::from_str(&bindings.to_ini_string());
    let entries = ini.section_or_empty("Hotkey");
    let mut entries = entries.raw_entries().collect::<Vec<_>>();
    if entries.is_empty() {
        // The parser drops entryless sections. A zero binding keeps a cleared
        // keyboard from falling back to the archive on the next load.
        entries.push(("StopObject", "0"));
    }
    profile_file::update(root, |bytes| replace_ini_section(bytes, "Hotkey", &entries))
}
pub(crate) fn reset(root: &Path, archive: Option<&[u8]>) -> HotkeyBindings {
    let defaults = HotkeyBindings::reload_from_ini_bytes(archive);
    // Native delete/reload resets bindings. Replace only our section so the
    // options sharing VERA.INI survive; a failed save retains current bindings.
    if let Err(error) = save(root, &defaults) {
        log::warn!("Could not reset VERA.INI keyboard settings: {error}");
        return reload(root, archive);
    }
    defaults
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::input::hotkeys::HotkeyCommand;
    #[test]
    fn keyboard_save_and_reset_preserve_options_and_legacy_file() {
        let root = std::env::temp_dir().join(format!("vera-shared-profile-{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        let legacy = b"[Hotkey]\nStopObject=81\n";
        let options = b"[Audio]\nSoundVolume=0.25\n[VERA]\nZoom=150\n";
        std::fs::write(root.join("KeyboardMD.ini"), legacy).unwrap();
        std::fs::write(root.join("VERA.INI"), options).unwrap();
        let archive = b"[Hotkey]\nStopObject=83\n";
        let mut bindings = HotkeyBindings::reload_from_ini_bytes(Some(archive));
        bindings.assign(HotkeyCommand::StopObject, 88).unwrap();
        save(&root, &bindings).unwrap();
        let saved = std::fs::read(root.join("VERA.INI")).unwrap();
        assert!(saved.starts_with(options));
        assert_eq!(
            reload(&root, Some(archive)).first_key(HotkeyCommand::StopObject),
            Some(88)
        );
        assert!(String::from_utf8_lossy(&saved).contains("StopObject=88"));
        let reset = reset(&root, Some(archive));
        assert_eq!(reset.first_key(HotkeyCommand::StopObject), Some(83));
        assert!(
            std::fs::read(root.join("VERA.INI"))
                .unwrap()
                .starts_with(options)
        );
        assert_eq!(std::fs::read(root.join("KeyboardMD.ini")).unwrap(), legacy);
        save(&root, &HotkeyBindings::default()).unwrap();
        assert_eq!(
            reload(&root, Some(archive)).first_key(HotkeyCommand::StopObject),
            None
        );
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn cncnet_profile_keyboard_startup_keeps_the_initial_system_keys() {
        let root =
            std::env::temp_dir().join(format!("vera-cncnet-keyboard-{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        let bytes = b"[Hotkey]\nStopObject=88\n";
        std::fs::write(root.join("VERA.INI"), bytes).unwrap();
        let bindings = startup(&root, Some(b"[Hotkey]\nStopObject=83\n"));
        assert_eq!(bindings.first_key(HotkeyCommand::StopObject), Some(88));
        assert_eq!(bindings.command_at(27), Some(HotkeyCommand::Options));
        assert_eq!(
            bindings.command_at(32),
            Some(HotkeyCommand::CenterOnRadarEvent)
        );
        assert_eq!(std::fs::read(root.join("VERA.INI")).unwrap(), bytes);
        std::fs::remove_file(root.join("VERA.INI")).unwrap();
        std::fs::remove_dir(root).unwrap();
    }
    #[test]
    fn disk_reload_and_reset_follow_current_file_not_entry_snapshot() {
        let root = std::env::temp_dir().join(format!("vera-keyboard-{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        let archive = b"[Hotkey]\nStopObject=83\nDelete=110\n";
        std::fs::write(root.join("VERA.INI"), b"[Hotkey]\nStopObject=81\n").unwrap();
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
        assert!(root.join("VERA.INI").exists());
        assert_eq!(
            reload(&root, Some(archive)).first_key(HotkeyCommand::StopObject),
            Some(83)
        );
        std::fs::remove_dir_all(&root).unwrap();
    }
}
