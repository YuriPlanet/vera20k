//! The one-shot process-start `[Intro] Play=` flag.
//!
//! gamemd's WinMain reads it (default `yes`, `0x006BDF64..0x006BDF89`), sets
//! `[0xA8E960] & 4` for the YR intro that `Init_Game` plays through `0x0052CB50`
//! (call site `0x0052CA65`), and stores `Play=no` back
//! (`0x006BDFED..0x006BE018`). A fresh install therefore plays the intro once,
//! and every later launch skips it.
//!
//! Native keeps the key in `RA2MD.INI`. VERA imports that file once and keeps
//! `VERA.INI` the writable profile (`profile_file`), so the key lives there,
//! like the Phobos client switches. An absent key is a fresh install: it plays
//! the intro and records `no`.

use std::path::Path;

use crate::rules::ini_parser::IniFile;
use crate::util::ini_writer::set_ini_values;

const SECTION: &str = "Intro";
const KEY: &str = "Play";

/// Read `[Intro] Play=` and, when it asks for the intro, store `no` so no later
/// launch plays it again. Returns whether this launch plays the intro.
///
/// An unreadable or malformed profile keeps the native default and still
/// records the value, exactly as WinMain's read-then-write pair does.
pub(crate) fn consume(settings_dir: &Path) -> bool {
    let path = super::profile_file::path(settings_dir);
    let play = match crate::util::file_system::read(&path) {
        Ok(bytes) => match IniFile::from_bytes(&bytes) {
            Ok(ini) => ini.section_or_empty(SECTION).read_bool(KEY, true),
            Err(error) => {
                log::warn!(
                    "Could not parse {} for [Intro] Play: {error}",
                    path.display()
                );
                true
            }
        },
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => true,
        Err(error) => {
            log::warn!(
                "Could not read {} for [Intro] Play: {error}",
                path.display()
            );
            true
        }
    };
    if play
        && let Err(error) = super::profile_file::update(settings_dir, |bytes| {
            set_ini_values(bytes, SECTION, &[(KEY, "no")])
        })
    {
        log::warn!("Could not store [Intro] Play=no: {error}");
    }
    play
}

#[cfg(all(test, tests_app))]
mod tests {
    use super::*;
    use crate::rules::ini_parser::IniFile;

    static NEXT_PROFILE: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

    fn profile_with(contents: &str) -> std::path::PathBuf {
        let directory = std::env::temp_dir().join(format!(
            "vera-intro-play-{}-{}",
            std::process::id(),
            NEXT_PROFILE.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&directory).unwrap();
        std::fs::write(directory.join("VERA.INI"), contents).unwrap();
        directory
    }

    fn stored(directory: &std::path::Path) -> Option<bool> {
        let bytes = std::fs::read(super::super::profile_file::path(directory)).unwrap();
        let ini = IniFile::from_bytes(&bytes).unwrap();
        ini.section(SECTION)
            .and_then(|section| section.read_bool_value(KEY))
    }

    #[test]
    fn absent_key_is_a_fresh_install_and_records_that_the_intro_played() {
        let directory = profile_with("[Options]\r\nGameSpeed=2\r\n");
        assert!(consume(&directory));
        // Only the one key changes; the rest of the profile round-trips.
        let bytes = std::fs::read(super::super::profile_file::path(&directory)).unwrap();
        assert!(bytes.starts_with(b"[Options]\r\nGameSpeed=2\r\n"));
        assert_eq!(stored(&directory), Some(false));
        // The second launch of the same profile stays silent.
        assert!(!consume(&directory));
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn an_explicit_value_is_honoured_and_a_missing_profile_plays_once() {
        let directory = profile_with("[Intro]\r\nPlay=no\r\n");
        assert!(!consume(&directory));
        std::fs::remove_dir_all(directory).unwrap();

        // No profile yet: the import has not run, so this is the first launch.
        let empty =
            std::env::temp_dir().join(format!("vera-intro-play-missing-{}", std::process::id()));
        assert!(consume(&empty));
        assert_eq!(stored(&empty), Some(false));
        std::fs::remove_dir_all(empty).unwrap();
    }
}
