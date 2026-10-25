//! The build description the tactical view prints, ported from the native
//! Phobos version metadata.
//!
//! Native identity: `src/Phobos.version.h` derives the product name, the
//! version string and the build type once, and `src/Phobos.cpp` composes
//! `Phobos::VersionDescription` from them so no literal is spelled twice:
//!
//! ```text
//! VERSION_LONG_STR    "0.5.0.0"            (MAJOR.MINOR.REVISION.PATCH)
//! FILE_VERSION_STR    "0.5.0.0-alpha1"     (pre-release keeps the suffix)
//! PRODUCT_VERSION     "v0.5.0.0-alpha1"    (VERSION_PREFIX + the above)
//!
//! NIGHTLY         "Phobos v0.5.0.0 @ <commit>. DO NOT SHIP IN MODS!"
//! TESTING_BUILD   "Phobos v0.5.0.0-alpha1. Please test the build before shipping."
//! stable release  (compiled out: the draw hook prints nothing)
//! ```
//!
//! VERA20K carries the same product name, version and warning: a build
//! without `RELEASE` is a testing build, and a pre-release is a `RELEASE`
//! build that still defines `PRERELEASE_SUFFIX`. This tree is neither a
//! stable release nor a CI nightly, so it reports the pre-release testing
//! text. The nightly text additionally appends the commit and ref, which
//! VERA20K does not carry at run time.

/// `PRODUCT_NAME`.
const PRODUCT_NAME: &str = "Phobos";

/// `VERSION_MAJOR` — project maturity and completeness.
const VERSION_MAJOR: u32 = 0;
/// `VERSION_MINOR` — major changes and significant additions.
const VERSION_MINOR: u32 = 5;
/// `VERSION_REVISION` — minor changes, vanilla fixes and unhardcodings.
const VERSION_REVISION: u32 = 0;
/// `VERSION_PATCH` — Phobos-related bugfixes only.
const VERSION_PATCH: u32 = 0;

/// `PRERELEASE_SUFFIX`: its presence is what makes a release build a
/// pre-release. `None` for a stable release, as the native header comments
/// the macro out when one is prepared.
const PRERELEASE_SUFFIX: Option<&str> = Some("alpha1");

/// `VERSION_PREFIX`, applied in front of the numeric version only.
const VERSION_PREFIX: &str = "v";

/// `NIGHTLY`: the CI bleeding-edge build whose warning forbids shipping at
/// all. A local or pre-release build instead asks for testing before
/// shipping, and a stable release prints nothing (`SHOWS_DESCRIPTION`).
const NIGHTLY: bool = false;

/// Whether any description prints. Native compiles the draw hook's block out
/// of a stable release, which leaves the screen as retail left it.
pub(crate) const SHOWS_DESCRIPTION: bool = PRERELEASE_SUFFIX.is_some() || NIGHTLY;

/// `VERSION_LONG_STR`: the full four-part version.
fn version_long() -> String {
    format!("{VERSION_MAJOR}.{VERSION_MINOR}.{VERSION_REVISION}.{VERSION_PATCH}")
}

/// `PRODUCT_VERSION`: `v` plus the numeric version, plus the pre-release
/// suffix when the build is one. Nightly and local builds append their Git
/// commit and ref, which this binary does not know at run time.
pub(crate) fn product_version() -> String {
    let numeric = version_long();
    match (PRERELEASE_SUFFIX, NIGHTLY) {
        (Some(suffix), false) => format!("{VERSION_PREFIX}{numeric}-{suffix}"),
        _ => format!("{VERSION_PREFIX}{numeric}"),
    }
}

/// `Phobos::VersionDescription`: the product name, the version and the build
/// type's warning sentence.
pub(crate) fn version_description() -> String {
    let text = format!("{PRODUCT_NAME} {}", product_version());
    if NIGHTLY {
        format!("{text}. DO NOT SHIP IN MODS!")
    } else if SHOWS_DESCRIPTION {
        format!("{text}. Please test the build before shipping.")
    } else {
        text
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_is_the_four_part_native_number() {
        assert_eq!(version_long(), "0.5.0.0");
    }

    #[test]
    fn a_pre_release_carries_its_suffix_and_a_stable_version_does_not() {
        assert_eq!(product_version(), "v0.5.0.0-alpha1");
    }

    #[test]
    fn the_description_matches_the_native_testing_build_text() {
        assert_eq!(
            version_description(),
            "Phobos v0.5.0.0-alpha1. Please test the build before shipping.",
            "src/Phobos.cpp TESTING_BUILD arm"
        );
    }

    #[test]
    fn the_testing_build_shows_the_description() {
        assert!(SHOWS_DESCRIPTION);
    }
}
