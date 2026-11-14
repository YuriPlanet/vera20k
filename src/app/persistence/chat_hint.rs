//! The one-shot Android "long press the message lane to chat" hint flag.
//!
//! gamemd tells a fresh install about its intro once and records that it did
//! (`[Intro] Play`, see `intro_play`). The touch chat entry needs the same kind
//! of notice: the first network match after a fresh install posts one line into
//! the message lane, and the key is consumed so later launches stay silent.
//!
//! The key lives in the writable profile (`VERA.INI`) next to the other VERA
//! switches; an absent key is a fresh install, and a present `no` means the
//! player has already been told (or has taken the line out by hand).

use std::path::Path;

use crate::rules::ini_parser::IniFile;
use crate::util::ini_writer::set_ini_values;

const SECTION: &str = "Android";
const KEY: &str = "ChatHint";

/// Read `[Android] ChatHint=` and, when the hint is still due, store `no`.
/// Returns whether this launch should post the hint.
///
/// An unreadable or malformed profile keeps the default and still records the
/// value, exactly as `intro_play` does for the intro.
pub(crate) fn consume(settings_dir: &Path) -> bool {
    let path = super::profile_file::path(settings_dir);
    let due = match crate::util::file_system::read(&path) {
        Ok(bytes) => match IniFile::from_bytes(&bytes) {
            Ok(ini) => ini.section_or_empty(SECTION).read_bool(KEY, true),
            Err(error) => {
                log::warn!(
                    "Could not parse {} for [Android] ChatHint: {error}",
                    path.display()
                );
                true
            }
        },
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => true,
        Err(error) => {
            log::warn!(
                "Could not read {} for [Android] ChatHint: {error}",
                path.display()
            );
            true
        }
    };
    if due
        && let Err(error) = super::profile_file::update(settings_dir, |bytes| {
            set_ini_values(bytes, SECTION, &[(KEY, "no")])
        })
    {
        log::warn!("Could not store [Android] ChatHint=no: {error}");
    }
    due
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rules::ini_parser::IniFile;

    static NEXT_PROFILE: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

    fn profile_with(contents: &str) -> std::path::PathBuf {
        let directory = std::env::temp_dir().join(format!(
            "vera-chat-hint-{}-{}",
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
    fn an_absent_key_is_a_fresh_install_and_is_spent_on_the_first_match() {
        let directory = profile_with("[Options]\r\nGameSpeed=2\r\n");
        assert!(consume(&directory));
        // Only the one key changes; the rest of the profile round-trips.
        let bytes = std::fs::read(super::super::profile_file::path(&directory)).unwrap();
        assert!(bytes.starts_with(b"[Options]\r\nGameSpeed=2\r\n"));
        assert_eq!(stored(&directory), Some(false));
        // The next match of the same profile stays silent.
        assert!(!consume(&directory));
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn an_explicit_no_is_honoured() {
        let directory = profile_with("[Android]\r\nChatHint=no\r\n");
        assert!(!consume(&directory));
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn a_missing_profile_records_that_the_hint_was_shown() {
        let empty =
            std::env::temp_dir().join(format!("vera-chat-hint-missing-{}", std::process::id()));
        assert!(consume(&empty));
        assert_eq!(stored(&empty), Some(false));
        std::fs::remove_dir_all(empty).unwrap();
    }
}
