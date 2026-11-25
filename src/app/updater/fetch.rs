//! The updater's HTTP, on worker threads only: the check reads `latest.json`
//! and, only when it names a newer version, that version's signature; the
//! download streams the installer to disk while hashing it.

use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::Duration;

use anyhow::{Context, Result, ensure};
use ureq::tls::{RootCerts, TlsConfig};

use super::manifest::{self, Offer, Release, Version};

/// The download base: `latest.json` and `releases/` sit below it
/// (`.github/release/downloads.md`). A test build names its own server.
const BASE_URL: &str = match option_env!("VERA20K_UPDATE_URL") {
    Some(url) => url,
    None => "https://downloads.vera20k.org/",
};

/// The base64 Ed25519 public key a manifest's signature must verify with
/// (`.github/release/update-key.pub`). A test build names its own key.
const PUBLIC_KEY: &str = match option_env!("VERA20K_UPDATE_KEY") {
    Some(key) => key,
    None => include_str!("../../../.github/release/update-key.pub"),
};

const USER_AGENT: &str = concat!("VERA20k/", env!("CARGO_PKG_VERSION"), " updater");

/// `latest.json` is under 1 KiB and a signature 88 bytes.
const MAX_POINTER_BYTES: u64 = 64 * 1024;
const MAX_SIGNATURE_BYTES: u64 = 1024;

/// Shared between the download worker and the progress box.
#[derive(Default)]
pub(super) struct Progress {
    pub received: AtomicU64,
    pub cancel: AtomicBool,
}

/// The player cancelled the download.
#[derive(Debug, thiserror::Error)]
#[error("the download was cancelled")]
pub(super) struct Cancelled;

fn public_key() -> Result<[u8; 32]> {
    manifest::decode_key(PUBLIC_KEY).context("the update key is not a base64 Ed25519 public key")
}

/// The updater's connections: through the operating system's certificates,
/// which it keeps current, then through the Mozilla roots built into this
/// program (webpki-roots) when the system's refuse the server or are missing.
/// TLS only keeps the server reachable and the files private; the manifest's
/// signature and the installer's SHA-256 decide what is installed, so
/// whichever roots verify will do.
struct Agents {
    system: ureq::Agent,
    built_in: ureq::Agent,
}

impl Agents {
    fn new() -> Self {
        Self {
            system: agent(RootCerts::PlatformVerifier),
            built_in: agent(RootCerts::WebPki),
        }
    }
}

fn agent(roots: RootCerts) -> ureq::Agent {
    ureq::Agent::config_builder()
        .tls_config(TlsConfig::builder().root_certs(roots).build())
        .https_only(BASE_URL.starts_with("https://"))
        .max_redirects(0)
        .http_status_as_error(false)
        .timeout_connect(Some(Duration::from_secs(15)))
        .timeout_recv_response(Some(Duration::from_secs(30)))
        // A stalled transfer fails; a slow one may take as long as it needs.
        .timeout_per_read(Some(Duration::from_secs(30)))
        .build()
        .into()
}

fn get(agents: &Agents, url: &str) -> Result<ureq::http::Response<ureq::Body>> {
    let call = |agent: &ureq::Agent| agent.get(url).header("User-Agent", USER_AGENT).call();
    let response = match call(&agents.system) {
        // Only after a TLS failure: the network or the server would fail both.
        Err(error) if is_tls(&error) => {
            log::info!("Updater: trying the built-in roots for {url}: {error}");
            call(&agents.built_in)
        }
        response => response,
    }
    .with_context(|| format!("cannot reach {url}"))?;
    ensure!(
        response.status() == 200,
        "{url} answered {}",
        response.status()
    );
    Ok(response)
}

/// A failed TLS handshake, or a verifier that could not be built.
fn is_tls(error: &ureq::Error) -> bool {
    match error {
        ureq::Error::Tls(_) | ureq::Error::Rustls(_) => true,
        ureq::Error::Io(error) => error
            .get_ref()
            .is_some_and(|inner| inner.is::<rustls::Error>()),
        _ => false,
    }
}

fn get_small(agents: &Agents, url: &str, limit: u64) -> Result<Vec<u8>> {
    let mut bytes = Vec::new();
    get(agents, url)?
        .body_mut()
        .as_reader()
        .take(limit + 1)
        .read_to_end(&mut bytes)
        .with_context(|| format!("cannot read {url}"))?;
    ensure!(
        bytes.len() as u64 <= limit,
        "{url} is larger than a manifest"
    );
    Ok(bytes)
}

/// What this program can do about the newest release on `platform`
/// ([`manifest::offer`]), when it is newer than `current`. A skipped version
/// is still found: Check for Updates offers it, and so does the check the
/// lobby's word that this version is behind starts (`lobby_outdated`).
pub(super) fn check(current: Version, platform: &str) -> Result<Option<Offer>> {
    let agents = Agents::new();
    let latest = get_small(
        &agents,
        &format!("{BASE_URL}latest.json"),
        MAX_POINTER_BYTES,
    )?;
    let (version, _) = manifest::pointer(&latest)?;
    if version <= current {
        return Ok(None);
    }
    let signature = get_small(
        &agents,
        &format!(
            "{BASE_URL}{}/manifest.json.sig",
            manifest::release_folder(version)
        ),
        MAX_SIGNATURE_BYTES,
    )?;
    manifest::offer(&latest, &signature, &public_key()?, platform).map(Some)
}

/// Download `release` into `folder` and return the installer's path once its
/// size and SHA-256 match the signed manifest. Nothing is left behind on
/// failure or [`Cancelled`].
pub(super) fn download(release: &Release, folder: &Path, progress: &Progress) -> Result<PathBuf> {
    let url = format!("{BASE_URL}{}", release.key);
    let mut response = get(&Agents::new(), &url)?;
    save(response.body_mut().as_reader(), release, folder, progress)
        .with_context(|| format!("cannot download {url}"))
}

fn save(
    source: impl Read,
    release: &Release,
    folder: &Path,
    progress: &Progress,
) -> Result<PathBuf> {
    std::fs::create_dir_all(folder)
        .with_context(|| format!("cannot create {}", folder.display()))?;
    let target = folder.join(&release.name);
    let partial = folder.join(format!("{}.part", release.name));
    let result = save_to(source, release, &partial, progress)
        .and_then(|()| std::fs::rename(&partial, &target).context("cannot keep the download"));
    if result.is_err() {
        let _ = std::fs::remove_file(&partial);
    }
    result.map(|()| target)
}

fn save_to(
    mut source: impl Read,
    release: &Release,
    path: &Path,
    progress: &Progress,
) -> Result<()> {
    let mut file =
        std::fs::File::create(path).with_context(|| format!("cannot write {}", path.display()))?;
    let mut digest = crate::util::sha256::Sha256::new();
    let mut buffer = vec![0; 64 * 1024];
    let mut received = 0u64;
    loop {
        if progress.cancel.load(Ordering::Relaxed) {
            return Err(Cancelled.into());
        }
        let count = source.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        received += count as u64;
        ensure!(
            received <= release.size,
            "{} is larger than its manifest says",
            release.name
        );
        digest.update(&buffer[..count]);
        file.write_all(&buffer[..count])
            .with_context(|| format!("cannot write {}", path.display()))?;
        progress.received.store(received, Ordering::Relaxed);
    }
    ensure!(received == release.size, "{} ended early", release.name);
    ensure!(
        digest.finalize() == release.sha256,
        "{} does not match its signed SHA-256",
        release.name
    );
    file.sync_all()
        .with_context(|| format!("cannot write {}", path.display()))
}

#[cfg(all(test, tests_app))]
mod tests {
    use std::net::TcpListener;
    use std::sync::Arc;

    use rustls::pki_types::pem::PemObject;
    use rustls::pki_types::{CertificateDer, PrivateKeyDer};

    use super::*;

    /// A test-only certificate for 127.0.0.1 and its key: `openssl req -x509`
    /// with a P-256 key for 100 years, `CA:FALSE`, `serverAuth` and
    /// `subjectAltName = IP:127.0.0.1`.
    const CERTIFICATE: &str = "-----BEGIN CERTIFICATE-----
MIIBcjCCARigAwIBAgIUKwKTU03ua+rm3s5HPh5a9EifBD4wCgYIKoZIzj0EAwIw
FDESMBAGA1UEAwwJMTI3LjAuMC4xMCAXDTI2MTAzMDE3MTUyMVoYDzIxMjYxMDA2
MTcxNTIxWjAUMRIwEAYDVQQDDAkxMjcuMC4wLjEwWTATBgcqhkjOPQIBBggqhkjO
PQMBBwNCAAQrd6oeADA0C2Ku3g4Le1QMHu/nluSwUfvWNHIPedtHqL1okYOx6aPt
sftpmcRpBJgMK4txTACVw669msIaMMm4o0YwRDAMBgNVHRMBAf8EAjAAMA4GA1Ud
DwEB/wQEAwIHgDATBgNVHSUEDDAKBggrBgEFBQcDATAPBgNVHREECDAGhwR/AAAB
MAoGCCqGSM49BAMCA0gAMEUCIDULpAq2XGzq8Y4XsO0nakVtn6I1qgdiWZbvMKsR
zWJOAiEAjHNTfXDBXquAdVpSXRAxvq3HLcBLeeULM+1wiNul1+k=
-----END CERTIFICATE-----
";
    const KEY: &str = "-----BEGIN PRIVATE KEY-----
MIGHAgEAMBMGByqGSM49AgEGCCqGSM49AwEHBG0wawIBAQQgxPfPZXeKohEZ6BvH
vyENqvu/KQlaa7oIXDCLZGL6fbuhRANCAAQrd6oeADA0C2Ku3g4Le1QMHu/nluSw
UfvWNHIPedtHqL1okYOx6aPtsftpmcRpBJgMK4txTACVw669msIaMMm4
-----END PRIVATE KEY-----
";

    /// Answers every request with `{}` over TLS with [`CERTIFICATE`], one
    /// connection at a time, and returns the port.
    fn serve() -> u16 {
        let provider = Arc::new(rustls::crypto::ring::default_provider());
        let config = rustls::ServerConfig::builder_with_provider(provider)
            .with_safe_default_protocol_versions()
            .unwrap()
            .with_no_client_auth()
            .with_single_cert(
                vec![CertificateDer::from_pem_slice(CERTIFICATE.as_bytes()).unwrap()],
                PrivateKeyDer::from_pem_slice(KEY.as_bytes()).unwrap(),
            )
            .unwrap();
        let config = Arc::new(config);
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        std::thread::spawn(move || {
            for mut socket in listener.incoming().flatten() {
                let mut tls = rustls::ServerConnection::new(config.clone()).unwrap();
                let mut stream = rustls::Stream::new(&mut tls, &mut socket);
                let mut request = Vec::new();
                let mut byte = [0];
                // A client that refuses the certificate ends the reading here.
                while !request.ends_with(b"\r\n\r\n")
                    && stream.read(&mut byte).is_ok_and(|n| n == 1)
                {
                    request.push(byte[0]);
                }
                let _ = stream.write_all(
                    b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\n{}",
                );
                let _ = stream.flush();
            }
        });
        port
    }

    #[test]
    fn a_server_is_reached_when_either_set_of_roots_trusts_it() {
        let url = format!("https://127.0.0.1:{}/latest.json", serve());
        let trusting = || {
            let certificate = ureq::tls::Certificate::from_pem(CERTIFICATE.as_bytes()).unwrap();
            agent(RootCerts::new_with_certs(&[certificate]))
        };
        // Mozilla's roots don't include the test certificate.
        let refusing = || agent(RootCerts::WebPki);
        let reach = |system, built_in| get_small(&Agents { system, built_in }, &url, 16).ok();
        assert_eq!(reach(trusting(), refusing()).as_deref(), Some(&b"{}"[..]));
        assert_eq!(reach(refusing(), trusting()).as_deref(), Some(&b"{}"[..]));
        assert_eq!(reach(refusing(), refusing()), None);
    }

    #[test]
    fn only_an_installer_matching_its_signed_sha256_is_kept() {
        let folder =
            std::env::temp_dir().join(format!("vera20k-update-save-{}", std::process::id()));
        let installer = b"installer bytes";
        let mut digest = crate::util::sha256::Sha256::new();
        digest.update(installer);
        let mut release = Release {
            version: Version::parse("1.2.3").unwrap(),
            key: "releases/v1.2.3/vera20k-1.2.3-macos-arm64.dmg".into(),
            name: "vera20k-1.2.3-macos-arm64.dmg".into(),
            sha256: digest.finalize(),
            size: installer.len() as u64,
            notes: Vec::new(),
        };
        let changed = b"installer bytez";
        let refused = save(&changed[..], &release, &folder, &Progress::default());
        assert!(refused.unwrap_err().to_string().contains("SHA-256"));
        assert_eq!(std::fs::read_dir(&folder).unwrap().count(), 0);
        let kept = save(&installer[..], &release, &folder, &Progress::default()).unwrap();
        assert_eq!(std::fs::read(&kept).unwrap(), installer);
        release.size += 1;
        assert!(save(&installer[..], &release, &folder, &Progress::default()).is_err());
        std::fs::remove_dir_all(&folder).unwrap();
    }
}
