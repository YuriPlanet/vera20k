//! Retail internal-version text resolution.
//!
//! `VERSION.TXT` is a raw current-working-directory file. Retail reads one
//! 16-byte block, reserves the last byte as a null terminator, and removes only
//! trailing carriage returns. The internal-version formatter keeps that text
//! in a separate cached field and returns its numeric build string.

use std::fs::{File, OpenOptions};
use std::io::{self, Read};
use std::sync::OnceLock;

pub const INTERNAL_VERSION_FALLBACK: &str = "1.001TUC";

/// This build's VERA20k version, which a mod's `mod.ini` and a map's
/// `[Basic] VeraMinVersion=` compare against.
pub const GAME_VERSION: &str = env!("CARGO_PKG_VERSION");

/// `1`, `1.2` or `1.2.3` as three numbers, absent ones 0; anything else,
/// such as a later scheme this build does not know, is `None`.
fn version_parts(text: &str) -> Option<[u32; 3]> {
    let mut parts = [0; 3];
    let mut given = text.trim().split('.');
    for part in &mut parts {
        match given.next() {
            Some(digits) if digits.len() <= 9 && !digits.is_empty() => {
                *part = digits
                    .bytes()
                    .all(|byte| byte.is_ascii_digit())
                    .then(|| digits.parse().ok())
                    .flatten()?;
            }
            Some(_) => return None,
            None => break,
        }
    }
    given.next().is_none().then_some(parts)
}

/// Whether this build, `current`, can play a mod or map that needs `needs`.
/// A build whose own version does not read as three numbers refuses nothing.
pub fn version_serves(current: &str, needs: &str) -> bool {
    match (version_parts(current), version_parts(needs)) {
        (Some(current), Some(needs)) => current >= needs,
        (None, _) => true,
        (Some(_), None) => false,
    }
}

const VERSION_TXT_READ_LEN: usize = 16;
const VERSION_TXT_TERMINATOR_INDEX: usize = VERSION_TXT_READ_LEN - 1;

static VERSION_CACHE: OnceLock<VersionCache> = OnceLock::new();

struct VersionCache {
    // The active formatter loads and caches this side field even though it does
    // not substitute the text into the returned internal-version string.
    _version_txt: String,
    internal_version: String,
}

/// Resolve the process-wide retail internal version.
///
/// The relative filename intentionally uses the current working directory; it
/// is not resolved through the configured asset root or MIX archives.
pub fn retail_internal_version() -> &'static str {
    VERSION_CACHE
        .get_or_init(|| version_cache(open_version_txt().and_then(read_version_txt)))
        .internal_version
        .as_str()
}

fn open_version_txt() -> io::Result<File> {
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        options.share_mode(1).attributes(0x80);
    }
    options.open("VERSION.TXT")
}

fn version_cache(result: io::Result<String>) -> VersionCache {
    VersionCache {
        _version_txt: result.unwrap_or_default(),
        internal_version: INTERNAL_VERSION_FALLBACK.to_owned(),
    }
}

fn read_version_txt(mut reader: impl Read) -> io::Result<String> {
    let mut bytes = [0_u8; VERSION_TXT_READ_LEN];
    let _ = reader.read(&mut bytes)?;

    // Retail overwrites byte 15 after the read, so at most 15 bytes are
    // visible even when ReadFile returned all 16.
    bytes[VERSION_TXT_TERMINATOR_INDEX] = 0;
    let mut end = bytes
        .iter()
        .position(|&byte| byte == 0)
        .unwrap_or(bytes.len());
    while end != 0 && bytes[end - 1] == b'\r' {
        end -= 1;
    }

    // Stock VERSION.TXT is ASCII. Mapping one byte to one scalar keeps the raw
    // C-string byte boundaries deterministic for non-stock files as well.
    Ok(bytes[..end].iter().copied().map(char::from).collect())
}

#[cfg(all(test, tests_util))]
mod tests {
    use std::io::Cursor;

    use super::{INTERNAL_VERSION_FALLBACK, read_version_txt, version_cache};

    #[test]
    fn version_text_removes_every_trailing_cr_but_not_lf() {
        assert_eq!(
            read_version_txt(Cursor::new(b"1.001TUC\r\r")).unwrap(),
            "1.001TUC"
        );
        assert_eq!(
            read_version_txt(Cursor::new(b"1.001TUC\r\n")).unwrap(),
            "1.001TUC\r\n"
        );
    }

    #[test]
    fn byte_fifteen_is_the_forced_terminator() {
        assert_eq!(
            read_version_txt(Cursor::new(b"1234567890abcdefignored")).unwrap(),
            "1234567890abcde"
        );
    }

    #[test]
    fn embedded_nul_ends_the_version_text() {
        assert_eq!(
            read_version_txt(Cursor::new(b"1.001\0discarded")).unwrap(),
            "1.001"
        );
    }

    #[test]
    fn raw_version_text_never_replaces_the_numeric_internal_string() {
        assert_eq!(
            version_cache(Ok("9.999CUSTOM".to_owned())).internal_version,
            INTERNAL_VERSION_FALLBACK
        );
        assert_eq!(
            version_cache(Err(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "missing"
            )))
            .internal_version,
            INTERNAL_VERSION_FALLBACK
        );
    }
}
