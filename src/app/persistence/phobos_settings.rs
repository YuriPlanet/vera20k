//! Phobos client settings, read from the imported player profile.
//!
//! Phobos reads `RA2MD.INI [Phobos]` directly each launch. VERA imports that
//! file once into `VERA.INI` and keeps it the writable profile, so the same
//! section arrives here from `profile_file`. Only settings that gate a
//! compiled extension live here; retail options keep `options_profile`.

use std::path::Path;

use crate::rules::ini_parser::IniFile;

/// The `[Phobos]` client switches this build reads.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct PhobosClientSettings {
    /// `[Phobos] ShowPlacementPreview=`, source default `yes` (b36 key; the
    /// old `ShowBuildingPlacementPreview` is not read).
    pub(crate) show_placement_preview: bool,
    /// `[Phobos] DigitalDisplay.Enable=`, default `no`.
    pub(crate) digital_display_enabled: bool,
    /// `[Phobos] SuperWeaponSidebar.RequiredSignificance=`, default `0`.
    pub(crate) superweapon_sidebar_required_significance: i32,
    /// `[Phobos] ShowFlashOnSelecting=`, default `no`: the client switch for
    /// `[AudioVisual] SelectionFlashDuration`.
    pub(crate) show_flash_on_selecting: bool,
}

impl Default for PhobosClientSettings {
    /// The source defaults, also used when there is no readable profile and by
    /// sealed captures.
    fn default() -> Self {
        Self {
            show_placement_preview: true,
            digital_display_enabled: false,
            superweapon_sidebar_required_significance: 0,
            show_flash_on_selecting: false,
        }
    }
}

/// Read the switches from the profile in `settings_dir`.
pub(crate) fn read(settings_dir: &Path) -> PhobosClientSettings {
    let defaults = PhobosClientSettings::default();
    let path = super::profile_file::path(settings_dir);
    let Ok(bytes) = std::fs::read(&path) else {
        return defaults;
    };
    let Ok(ini) = IniFile::from_bytes(&bytes) else {
        log::warn!("Could not parse {} for Phobos settings", path.display());
        return defaults;
    };
    let Some(section) = ini.section("Phobos") else {
        return defaults;
    };
    PhobosClientSettings {
        show_placement_preview: section
            .read_bool("ShowPlacementPreview", defaults.show_placement_preview),
        digital_display_enabled: section
            .read_bool("DigitalDisplay.Enable", defaults.digital_display_enabled),
        superweapon_sidebar_required_significance: section.read_int(
            "SuperWeaponSidebar.RequiredSignificance",
            defaults.superweapon_sidebar_required_significance,
        ),
        show_flash_on_selecting: section
            .read_bool("ShowFlashOnSelecting", defaults.show_flash_on_selecting),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    static NEXT_PROFILE: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

    fn profile_with(section: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "vera-phobos-settings-{}-{}",
            std::process::id(),
            NEXT_PROFILE.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("VERA.INI"), section).unwrap();
        dir
    }

    #[test]
    fn missing_profile_or_section_keeps_the_source_defaults() {
        let missing = std::env::temp_dir().join("vera-phobos-settings-missing");
        assert_eq!(read(&missing), PhobosClientSettings::default());
        let dir = profile_with("[Options]\nGameSpeed=1\n");
        assert_eq!(read(&dir), PhobosClientSettings::default());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn imported_switches_are_read() {
        let dir = profile_with(
            "[Phobos]\nShowPlacementPreview=no\nDigitalDisplay.Enable=yes\n\
             SuperWeaponSidebar.RequiredSignificance=2\nShowFlashOnSelecting=yes\n",
        );
        assert_eq!(
            read(&dir),
            PhobosClientSettings {
                show_placement_preview: false,
                digital_display_enabled: true,
                superweapon_sidebar_required_significance: 2,
                show_flash_on_selecting: true,
            }
        );
        std::fs::remove_dir_all(dir).unwrap();
    }
}
