//! The release manifest that `.github/release/publish-r2.sh` publishes to R2.
//!
//! `latest.json` names the newest stable release. Until its bytes verify, only
//! its version is read, to decide whether there is anything to fetch; the
//! release itself comes from those same bytes once
//! `releases/v<version>/manifest.json.sig` verifies them (Ed25519 over the
//! exact bytes, base64). See `.github/release/downloads.md`.
//!
//! A newer release this program cannot install is offered as a download by
//! hand: one whose manifest needs a newer updater, or one that another key
//! signs, the update key its manifest names, as after a key change.

use std::collections::BTreeMap;

use anyhow::{Context, Result, bail, ensure};
use base64::Engine;
use serde::Deserialize;

/// The largest installer the updater downloads. Releases are about 15 MB.
const MAX_INSTALLER_BYTES: u64 = 512 * 1024 * 1024;

/// A stable release version, `MAJOR.MINOR.PATCH` exactly as the publisher's
/// tag check accepts it (no leading zeros, at most nine digits each).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct Version {
    major: u32,
    minor: u32,
    patch: u32,
}

impl Version {
    pub(crate) fn parse(text: &str) -> Option<Self> {
        let mut parts = text.split('.');
        let mut part = || -> Option<u32> {
            let digits = parts.next()?;
            let canonical = digits == "0"
                || (!digits.is_empty()
                    && digits.len() <= 9
                    && !digits.starts_with('0')
                    && digits.bytes().all(|byte| byte.is_ascii_digit()));
            canonical.then(|| digits.parse().ok()).flatten()
        };
        let version = Self {
            major: part()?,
            minor: part()?,
            patch: part()?,
        };
        parts.next().is_none().then_some(version)
    }

    /// This program's version.
    pub(crate) fn current() -> Self {
        Self::parse(env!("CARGO_PKG_VERSION"))
            .expect("Cargo.toml holds a MAJOR.MINOR.PATCH version")
    }
}

impl std::fmt::Display for Version {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}.{}.{}", self.major, self.minor, self.patch)
    }
}

/// The manifest's name for this build's installer, when the Release workflow
/// publishes one for it: `windows` (x64 setup), `macos` (Apple Silicon disk
/// image), `macos-x64` (Intel Mac disk image) or `linux` (x64 AppImage).
/// Releases before the Intel Mac image have no `macos-x64` installer.
pub(crate) fn platform() -> Option<&'static str> {
    if cfg!(all(target_os = "windows", target_arch = "x86_64")) {
        Some("windows")
    } else if cfg!(all(target_os = "macos", target_arch = "aarch64")) {
        Some("macos")
    } else if cfg!(all(target_os = "macos", target_arch = "x86_64")) {
        Some("macos-x64")
    } else if cfg!(all(target_os = "linux", target_arch = "x86_64")) {
        Some("linux")
    } else {
        None
    }
}

/// The installer's file name ending for each platform.
fn installer_suffix(platform: &str) -> &'static str {
    match platform {
        "windows" => "-windows-x64-setup.exe",
        "macos" => "-macos-arm64.dmg",
        "macos-x64" => "-macos-x64.dmg",
        _ => "-linux-x64.AppImage",
    }
}

/// The updater version this program is (`.github/release/updater-version`):
/// a release whose manifest needs a higher one is a download by hand.
fn updater() -> u32 {
    include_str!("../../../.github/release/updater-version")
        .trim()
        .parse()
        .expect("updater-version holds a number")
}

/// What every manifest keeps, whichever updater installs its release.
#[derive(Deserialize)]
struct Header {
    version: String,
    tag: String,
    /// The updater version that installs it; manifests before them have none.
    #[serde(default)]
    updater: u32,
    /// The base64 public key that signs it.
    #[serde(default)]
    update_key: Option<String>,
    #[serde(default)]
    notes: Vec<String>,
}

fn header(manifest: &[u8]) -> Result<(Header, Version)> {
    let header: Header = serde_json::from_slice(manifest).context("not a release manifest")?;
    let version = Version::parse(&header.version).context("the manifest has no valid version")?;
    ensure!(
        header.tag == format!("v{version}"),
        "the manifest names tag {} for version {version}",
        header.tag
    );
    Ok((header, version))
}

/// The unverified version and tag `latest.json` names.
pub(crate) fn pointer(latest: &[u8]) -> Result<(Version, String)> {
    let (header, version) = header(latest).context("latest.json is not valid")?;
    Ok((version, header.tag))
}

/// The installers in a manifest this program's updater installs.
#[derive(Deserialize)]
struct Installers {
    files: BTreeMap<String, ManifestFile>,
}

#[derive(Deserialize)]
struct ManifestFile {
    key: String,
    name: String,
    sha256: String,
    size: u64,
}

/// One platform's installer in a verified release.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Release {
    pub version: Version,
    /// The object key below the download base, `releases/<tag>/<name>`.
    pub key: String,
    /// The installer's file name: only ASCII letters, digits, `.`, `_`, `-`.
    pub name: String,
    pub sha256: [u8; 32],
    pub size: u64,
    /// What changed, in plain words: the release page's change list.
    pub notes: Vec<String>,
}

/// What this program can do about a newer release.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Offer {
    /// Install this platform's installer.
    Update(Release),
    /// It needs a newer updater: point the player to vera20k.org.
    Manual {
        version: Version,
        notes: Vec<String>,
    },
    /// Another key signs it, the one its manifest names, as after a key
    /// change. Anyone who can write to the download server could sign one
    /// that way, and vera20k.org's download links follow its `latest.json`:
    /// only the version is shown, and the player is pointed to the GitHub
    /// releases instead.
    NewKey { version: Version },
}

impl Offer {
    pub(crate) fn version(&self) -> Version {
        match self {
            Self::Update(release) => release.version,
            Self::Manual { version, .. } | Self::NewKey { version } => *version,
        }
    }
}

/// What `manifest` offers this program on `platform`, given `signature`
/// (base64), an Ed25519 signature of exactly these bytes. Signed by
/// `public_key`, it is the release's installer, or [`Offer::Manual`] when it
/// needs a newer updater; signed by the key it names, [`Offer::NewKey`].
pub(crate) fn offer(
    manifest: &[u8],
    signature: &[u8],
    public_key: &[u8; 32],
    platform: &str,
) -> Result<Offer> {
    let signature = base64::engine::general_purpose::STANDARD
        .decode(signature.trim_ascii())
        .context("manifest signature is not base64")?;
    let signed_by = |key: &[u8]| {
        ring::signature::UnparsedPublicKey::new(&ring::signature::ED25519, key)
            .verify(manifest, &signature)
            .is_ok()
    };
    // Read before it verifies, as `pointer` reads the same bytes.
    let (header, version) = header(manifest)?;
    if !signed_by(public_key) {
        let named = header.update_key.as_deref().and_then(decode_key);
        ensure!(
            named.is_some_and(|key| signed_by(&key)),
            "manifest signature does not verify"
        );
        return Ok(Offer::NewKey { version });
    }
    if header.updater > updater() {
        return Ok(Offer::Manual {
            version,
            notes: header.notes,
        });
    }
    release(manifest, version, header.notes, platform).map(Offer::Update)
}

/// The installer for `platform` in a verified manifest.
fn release(
    manifest: &[u8],
    version: Version,
    notes: Vec<String>,
    platform: &str,
) -> Result<Release> {
    let installers: Installers =
        serde_json::from_slice(manifest).context("signed manifest is not valid")?;
    let Some(file) = installers.files.get(platform) else {
        bail!("release {version} has no {platform} installer");
    };
    let safe_name = !file.name.starts_with('.')
        && file
            .name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'));
    ensure!(
        safe_name && file.name == format!("vera20k-{version}{}", installer_suffix(platform)),
        "release {version} names an unexpected installer {:?}",
        file.name
    );
    ensure!(
        file.key == format!("{}/{}", release_folder(version), file.name),
        "release {version} stores its installer at an unexpected key"
    );
    ensure!(
        (1..=MAX_INSTALLER_BYTES).contains(&file.size),
        "release {version} installer size {} is out of range",
        file.size
    );
    let sha256 = decode_sha256(&file.sha256).context("installer SHA-256 is not 64 hex digits")?;
    Ok(Release {
        version,
        key: file.key.clone(),
        name: file.name.clone(),
        sha256,
        size: file.size,
        notes,
    })
}

/// A base64 Ed25519 public key, as `update-key.pub` holds it.
pub(crate) fn decode_key(text: &str) -> Option<[u8; 32]> {
    base64::engine::general_purpose::STANDARD
        .decode(text.trim())
        .ok()?
        .try_into()
        .ok()
}

/// Where the publisher stores one version's files below the download base.
pub(crate) fn release_folder(version: Version) -> String {
    format!("releases/v{version}")
}

fn decode_sha256(text: &str) -> Option<[u8; 32]> {
    let digits = text.as_bytes();
    if digits.len() != 64 {
        return None;
    }
    let mut digest = [0; 32];
    for (byte, pair) in digest.iter_mut().zip(digits.chunks_exact(2)) {
        let nibble = |digit: u8| match digit {
            b'0'..=b'9' => Some(digit - b'0'),
            b'a'..=b'f' => Some(digit - b'a' + 10),
            _ => None,
        };
        *byte = nibble(pair[0])? << 4 | nibble(pair[1])?;
    }
    Some(digest)
}

#[cfg(test)]
mod tests {
    use super::*;

    // Published by the real `.github/release/publish-r2.sh`, signed with a
    // throwaway key: `python -m tools.update_test fixture src/app/updater/fixtures`.
    fn manifest() -> &'static [u8] {
        crate::test_fixture::bytes("src/app/updater/fixtures/manifest.json")
    }

    fn signature() -> &'static [u8] {
        crate::test_fixture::bytes("src/app/updater/fixtures/manifest.json.sig")
    }

    fn public_key() -> [u8; 32] {
        decode_key(crate::test_fixture::text(
            "src/app/updater/fixtures/update-key.pub",
        ))
        .unwrap()
    }

    #[test]
    fn the_publishers_signed_manifest_names_each_platforms_installer() {
        let version = Version::parse("1.2.3").unwrap();
        assert_eq!(pointer(manifest()).unwrap(), (version, "v1.2.3".into()));
        for (platform, size) in [
            ("windows", 45),
            ("macos", 39),
            ("macos-x64", 37),
            ("linux", 42),
        ] {
            let Offer::Update(release) =
                offer(manifest(), signature(), &public_key(), platform).unwrap()
            else {
                panic!("the fixture's updater version is this program's");
            };
            assert_eq!(release.version, version);
            assert_eq!(
                release.name,
                format!("vera20k-1.2.3{}", installer_suffix(platform))
            );
            assert_eq!(release.key, format!("releases/v1.2.3/{}", release.name));
            assert_eq!(release.size, size);
            assert_eq!(release.notes.len(), 6);
            assert_eq!(release.notes[3], "Emphasis and links lose their Markdown.");
        }
    }

    #[test]
    fn a_changed_manifest_is_refused_and_a_new_keys_release_only_named() {
        // The Linux installer's SHA-256 begins 7b115c.
        let at = manifest()
            .windows(6)
            .position(|digits| digits == b"7b115c")
            .unwrap();
        let mut changed = manifest().to_vec();
        changed[at] = b'8';
        assert!(offer(&changed, signature(), &public_key(), "linux").is_err());
        // To a program that trusts another key, the fixture is a release
        // after a key change: signed by the key its manifest names.
        let mut other = public_key();
        other[31] ^= 1;
        let named = Offer::NewKey {
            version: Version::parse("1.2.3").unwrap(),
        };
        assert_eq!(
            offer(manifest(), signature(), &other, "linux").unwrap(),
            named
        );
        assert!(offer(&changed, signature(), &other, "linux").is_err());
    }

    #[test]
    fn a_release_for_a_newer_updater_is_a_download_by_hand_with_its_notes() {
        use ring::signature::KeyPair;
        // Its installers may take another form; its header stays.
        let mut later: serde_json::Value = serde_json::from_slice(manifest()).unwrap();
        later["updater"] = (updater() + 1).into();
        later["files"] = "another form".into();
        let later = serde_json::to_vec(&later).unwrap();
        let pair = ring::signature::Ed25519KeyPair::from_seed_unchecked(&[7; 32]).unwrap();
        let signature = base64::engine::general_purpose::STANDARD.encode(pair.sign(&later));
        let key = pair.public_key().as_ref().try_into().unwrap();
        let Offer::Manual { version, notes } =
            offer(&later, signature.as_bytes(), &key, "linux").unwrap()
        else {
            panic!("a newer updater's release is not installed");
        };
        assert_eq!(version, Version::parse("1.2.3").unwrap());
        assert_eq!(notes.len(), 6);
    }

    #[test]
    fn versions_compare_by_number_and_parse_only_canonically() {
        assert!(Version::parse("0.10.0") > Version::parse("0.9.9"));
        assert!(Version::parse("1.0.0") > Version::parse("0.99.99"));
        for text in [
            "1.2",
            "1.2.3.4",
            "01.2.3",
            "1.2.3-rc1",
            "v1.2.3",
            "1..3",
            "",
        ] {
            assert_eq!(Version::parse(text), None, "{text:?}");
        }
    }
}
