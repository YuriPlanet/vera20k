//! Test access to the retail INI corpus in the gitignored `ini/` directory.
//!
//! The retail INIs are EA data, so they are never committed. `cargo run --bin
//! extract-ini` fills `ini/` from a local Red Alert 2 / Yuri's Revenge install.
//! Without them a retail-data test prints a `SKIPPED` line and passes, so a
//! fresh clone's `cargo test -p vera20k --lib` is green. Set
//! `VERA20K_REQUIRE_RETAIL_INI=1` to make a missing file fail instead; do that
//! in any checkout whose results you rely on for parity.

use std::path::{Path, PathBuf};

use crate::rules::ini_parser::IniFile;

/// Environment variable that turns a missing retail INI into a test failure.
pub(crate) const REQUIRE_RETAIL_INI_ENV: &str = "VERA20K_REQUIRE_RETAIL_INI";

fn retail_ini_path(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("ini").join(name)
}

fn require_retail_ini() -> bool {
    std::env::var_os(REQUIRE_RETAIL_INI_ENV).is_some_and(|value| !value.is_empty() && value != "0")
}

/// The bytes of `ini/<name>`, or `None` after printing a `SKIPPED` line when
/// the file is absent and [`REQUIRE_RETAIL_INI_ENV`] is not set.
pub(crate) fn retail_ini_bytes(name: &str) -> Option<Vec<u8>> {
    let path = retail_ini_path(name);
    match std::fs::read(&path) {
        Ok(bytes) => Some(bytes),
        Err(error) if require_retail_ini() => panic!(
            "cannot read {}: {error} ({REQUIRE_RETAIL_INI_ENV} is set)",
            path.display()
        ),
        Err(error) => {
            eprintln!(
                "SKIPPED: {} is unavailable ({error}); run `cargo run --bin extract-ini` \
                 to extract the retail INIs",
                path.display()
            );
            None
        }
    }
}

/// `ini/<name>` as UTF-8 text, or `None` when absent (see [`retail_ini_bytes`]).
pub(crate) fn retail_ini_text(name: &str) -> Option<String> {
    let bytes = retail_ini_bytes(name)?;
    Some(String::from_utf8(bytes).unwrap_or_else(|error| panic!("retail {name} is UTF-8: {error}")))
}

/// `ini/<name>` parsed, or `None` when absent (see [`retail_ini_bytes`]).
pub(crate) fn retail_ini(name: &str) -> Option<IniFile> {
    let bytes = retail_ini_bytes(name)?;
    Some(
        IniFile::from_bytes(&bytes).unwrap_or_else(|error| panic!("retail {name} parses: {error}")),
    )
}

/// Retail `rulesmd.ini` and `artmd.ini` parsed, or `None` when either is absent.
pub(crate) fn retail_rules_and_art() -> Option<(IniFile, IniFile)> {
    Some((retail_ini("rulesmd.ini")?, retail_ini("artmd.ini")?))
}
