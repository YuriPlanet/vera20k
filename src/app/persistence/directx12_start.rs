//! Options' DirectX 12 box (`[VERA] DirectX12=`) chooses the graphics API
//! when the game starts (`GpuContext::new`). A driver or shader that fails on
//! DirectX 12 before startup finishes would then fail every start, and the
//! player could never reach Options to untick the box. So a start on DirectX
//! 12 leaves a marker beside VERA.INI until startup finishes, and the next
//! start that finds it turns the box off and starts on the default APIs.

use std::path::{Path, PathBuf};

use super::options_profile::{DIRECTX12_KEY, VERA_SECTION};
use crate::util::file_system;
use crate::util::ini_writer::set_ini_values;

/// The marker a start on DirectX 12 leaves until startup finishes.
const MARKER: &str = "DirectX12.starting";

fn marker(settings_dir: &Path) -> PathBuf {
    settings_dir.join(MARKER)
}

/// Whether this start draws through DirectX 12: the box is `ticked` and no
/// earlier start on it failed to finish. A failed one turns the box off, in
/// `ticked` and in VERA.INI.
pub(crate) fn begin(settings_dir: &Path, ticked: &mut bool) -> bool {
    let marker = marker(settings_dir);
    if file_system::exists(&marker) {
        log::warn!(
            "The last start on DirectX 12 did not finish; turning DirectX 12 off and starting on the default graphics APIs"
        );
        *ticked = false;
        match super::profile_file::update(settings_dir, |bytes| {
            set_ini_values(bytes, VERA_SECTION, &[(DIRECTX12_KEY, "no")])
        }) {
            Ok(()) => remove(&marker),
            // The marker stays, so the next start skips DirectX 12 as well.
            Err(error) => log::warn!("Could not store [VERA] DirectX12=no: {error}"),
        }
        return false;
    }
    if *ticked && let Err(error) = file_system::write(&marker, b"") {
        // Without the marker a failing start would repeat; this one goes on.
        log::warn!("Could not write {}: {error}", marker.display());
    }
    *ticked
}

/// Startup has finished: a start on DirectX 12 works on this computer.
pub(crate) fn finish(settings_dir: &Path) {
    remove(&marker(settings_dir));
}

fn remove(marker: &Path) {
    match file_system::remove_file(marker) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => log::warn!("Could not remove {}: {error}", marker.display()),
    }
}

#[cfg(all(test, tests_app))]
mod tests {
    use super::*;
    use crate::rules::ini_parser::IniFile;

    #[test]
    fn a_start_on_directx12_that_never_finished_turns_the_box_off() {
        let directory =
            std::env::temp_dir().join(format!("vera-directx12-start-{}", std::process::id()));
        std::fs::create_dir_all(&directory).unwrap();
        std::fs::write(directory.join("VERA.INI"), "[VERA]\nDirectX12=yes\n").unwrap();
        let mut ticked = true;
        // A start that finished keeps the box on for the next one.
        assert!(begin(&directory, &mut ticked));
        finish(&directory);
        assert!(begin(&directory, &mut ticked));
        // That start never finished: the next one turns the box off.
        assert!(!begin(&directory, &mut ticked));
        assert!(!ticked);
        let profile = std::fs::read(super::super::profile_file::path(&directory)).unwrap();
        let ini = IniFile::from_bytes(&profile).unwrap();
        assert!(
            !ini.section(VERA_SECTION)
                .unwrap()
                .read_bool(DIRECTX12_KEY, true)
        );
        assert!(!marker(&directory).exists());
        std::fs::remove_dir_all(&directory).unwrap();
    }
}
