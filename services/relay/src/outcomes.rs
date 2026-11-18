//! Durable handoff of a marked federation's signed verdict to its lobby.
//! Only a committed database write releases the UDP owner's terminal result.
mod store;

use std::error::Error;
use std::io;
use std::path::Path;
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use self::store::{Delivery, Outbox};
use vera20k_relay_protocol::{FederationOutcome, RelayServer};

type Room = [u8; 16];
const IDLE: Duration = Duration::from_secs(1);

pub struct Reporter {
    write: mpsc::SyncSender<Vec<FederationOutcome>>,
    saved: mpsc::Receiver<Vec<Room>>,
    busy: bool,
    next_scan: Instant,
}

impl Reporter {
    pub fn configured(state_dir: &Path) -> Result<Option<Self>, Box<dyn Error>> {
        let url = match std::env::var("VERA_RELAY_OUTCOME_URL") {
            Ok(url) => url,
            Err(std::env::VarError::NotPresent) => return Ok(None),
            Err(_) => return Err("invalid federation outcome URL".into()),
        };
        validate_url(&url)?;
        let path = state_dir.join("federation-outcomes.sqlite3");
        let mut writer = Outbox::open(&path)?;
        let mut sender = Outbox::open(&path)?;
        let (write, requests) = mpsc::sync_channel::<Vec<FederationOutcome>>(1);
        let (saved, receipts) = mpsc::sync_channel(1);
        let (wake, new_receipts) = mpsc::sync_channel(1);
        thread::Builder::new()
            .name("relay-outcome-store".into())
            .spawn(move || {
                let mut warnings = Warnings::default();
                while let Ok(batch) = requests.recv() {
                    let count = batch.len();
                    let ids = writer.save(&batch, unix_time()).unwrap_or_default();
                    if ids.len() < count {
                        warnings.warn("federation outcome storage unavailable or full");
                    }
                    if !ids.is_empty() {
                        let _ = wake.try_send(());
                    }
                    if saved.send(ids).is_err() {
                        break;
                    }
                }
            })?;
        thread::Builder::new()
            .name("relay-outcome-https".into())
            .spawn(move || {
                let agent: ureq::Agent = ureq::Agent::config_builder()
                    .https_only(true)
                    .max_redirects(0)
                    .http_status_as_error(false)
                    .timeout_global(Some(Duration::from_secs(8)))
                    .build()
                    .into();
                let mut warnings = Warnings::default();
                loop {
                    let next = sender.claim(unix_time());
                    if let Ok(Some(receipt)) = &next {
                        let delivery = post(&agent, &url, receipt);
                        match delivery {
                            Delivery::Accepted => (),
                            Delivery::Retry => {
                                warnings.warn("federation outcome delivery will retry")
                            }
                            Delivery::Refused => {
                                warnings.warn("lobby refused a federation outcome")
                            }
                        }
                        if sender.settle(receipt, delivery).is_ok() {
                            continue;
                        }
                    }
                    if !matches!(next, Ok(None)) {
                        warnings.warn("federation outcome outbox could not advance");
                    }
                    if new_receipts.recv_timeout(IDLE) == Err(mpsc::RecvTimeoutError::Disconnected)
                    {
                        break;
                    }
                }
            })?;
        Ok(Some(Self {
            write,
            saved: receipts,
            busy: false,
            next_scan: Instant::now(),
        }))
    }

    /// The UDP thread never waits on disk, HTTPS, or a full worker queue.
    pub fn poll(&mut self, server: &mut RelayServer) {
        if self.busy {
            if let Ok(ids) = self.saved.try_recv() {
                for room in ids {
                    server.federation_outcome_saved(room);
                }
                self.busy = false;
            }
        }
        let now = Instant::now();
        if self.busy || now < self.next_scan {
            return;
        }
        self.next_scan = now + IDLE;
        let batch = server.pending_federation_outcomes();
        if !batch.is_empty() && self.write.try_send(batch).is_ok() {
            self.busy = true;
        }
    }
}

fn unix_time() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn validate_url(url: &str) -> io::Result<()> {
    let invalid = || io::Error::other("outcome URL must be an HTTPS origin with /v1/relay/outcome");
    let uri: ureq::http::Uri = url.parse().map_err(|_| invalid())?;
    if uri.scheme_str() != Some("https")
        || uri
            .authority()
            .is_none_or(|a| a.host().is_empty() || a.as_str().contains('@'))
        || uri.path_and_query().map(|p| p.as_str()) != Some("/v1/relay/outcome")
        || url.contains('#')
    {
        return Err(invalid());
    }
    Ok(())
}

fn post(agent: &ureq::Agent, url: &str, receipt: &FederationOutcome) -> Delivery {
    let response = agent
        .post(url)
        .header("Content-Type", "application/octet-stream")
        .send(receipt.as_bytes().as_slice());
    match response {
        Ok(response) => Delivery::from_status(response.status().as_u16()),
        Err(_) => Delivery::Retry,
    }
}

#[derive(Default)]
struct Warnings(Option<Instant>);

impl Warnings {
    fn warn(&mut self, message: &'static str) {
        if self
            .0
            .is_none_or(|last| last.elapsed() >= Duration::from_secs(60))
        {
            eprintln!("VERA20k relay: {message}");
            self.0 = Some(Instant::now());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{BufRead, BufReader, Read, Write};
    use std::net::TcpListener;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU32, Ordering};

    struct Directory(PathBuf);

    impl Directory {
        fn new() -> Self {
            static NEXT: AtomicU32 = AtomicU32::new(0);
            let path = std::env::temp_dir().join(format!(
                "vera-outcomes-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            std::fs::create_dir(&path).unwrap();
            Self(path)
        }
    }

    impl Drop for Directory {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    // No signing key or valid MAC is needed by the local spool or HTTP client.
    fn sample(id: u32, issued: u64) -> FederationOutcome {
        let mut bytes = [0; 94];
        bytes[..4].copy_from_slice(b"VFO1");
        bytes[4..8].copy_from_slice(&id.to_le_bytes());
        bytes[54..62].copy_from_slice(&issued.to_le_bytes());
        FederationOutcome::from_bytes(bytes).unwrap()
    }

    #[test]
    fn committed_outcomes_and_retry_deadlines_survive_reopen() {
        let dir = Directory::new();
        let path = dir.0.join("outcomes.sqlite3");
        let receipt = sample(3, 100);
        let mut spool = Outbox::open(&path).unwrap();
        assert_eq!(
            spool.save(std::slice::from_ref(&receipt), 100).unwrap(),
            [receipt.room()]
        );
        assert_eq!(
            spool.claim(100).unwrap().unwrap().as_bytes(),
            receipt.as_bytes()
        );
        drop(spool); // process stopped during the first POST

        let mut spool = Outbox::open(&path).unwrap();
        assert!(spool.claim(109).unwrap().is_none());
        assert_eq!(
            spool.claim(110).unwrap().unwrap().as_bytes(),
            receipt.as_bytes()
        );
        spool.settle(&receipt, Delivery::Retry).unwrap();
        assert!(spool.claim(129).unwrap().is_none());
        assert!(spool.claim(130).unwrap().is_some());
        spool.settle(&receipt, Delivery::Accepted).unwrap();
        assert!(spool.claim(receipt.expires_at()).unwrap().is_none());
    }

    #[test]
    fn a_full_spool_keeps_old_receipts_and_only_acknowledges_identical_bytes() {
        let dir = Directory::new();
        let mut spool = Outbox::open(&dir.0.join("outcomes.sqlite3")).unwrap();
        let originals: Vec<_> = (0..4096).map(|id| sample(id, 100)).collect();
        let old = &originals[0];
        let extra = sample(4096, 101);
        assert_eq!(spool.save(&originals, 100).unwrap().len(), originals.len());
        let batch = [sample(0, 101), extra.clone(), old.clone()];
        assert_eq!(spool.save(&batch, 101).unwrap(), [old.room()]);
        assert!(
            spool
                .save(std::slice::from_ref(&extra), old.expires_at())
                .unwrap()
                .is_empty()
        );
        assert_eq!(
            spool.save(&[extra.clone()], old.expires_at() + 1).unwrap(),
            [extra.room()]
        );
        spool.settle(&extra, Delivery::Refused).unwrap();
        assert!(spool.claim(extra.expires_at()).unwrap().is_none());
        assert!(spool.claim(extra.expires_at() + 1).unwrap().is_none());
    }

    #[test]
    fn delivery_posts_the_exact_receipt_and_only_accepts_a_204() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}/v1/relay/outcome", listener.local_addr().unwrap());
        assert!(validate_url(&url).is_err());
        assert!(validate_url("https://lobby.test/v1/relay/outcome").is_ok());
        assert!(validate_url("https://user@lobby.test/v1/relay/outcome").is_err());
        let receipt = sample(7, 100);
        let expected = receipt.clone();
        let receiver = thread::spawn(move || {
            for status in [429, 204] {
                let (mut socket, _) = listener.accept().unwrap();
                let mut reader = BufReader::new(&socket);
                let mut line = String::new();
                reader.read_line(&mut line).unwrap();
                assert_eq!(line, "POST /v1/relay/outcome HTTP/1.1\r\n");
                let mut binary = false;
                loop {
                    line.clear();
                    reader.read_line(&mut line).unwrap();
                    if line == "\r\n" {
                        break;
                    }
                    binary |= line
                        .trim()
                        .eq_ignore_ascii_case("Content-Type: application/octet-stream");
                }
                assert!(binary);
                let mut body = [0; 94];
                reader.read_exact(&mut body).unwrap();
                assert_eq!(&body, expected.as_bytes());
                write!(
                    socket,
                    "HTTP/1.1 {status} Result\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
                )
                .unwrap();
            }
        });
        // Only the localhost fixture uses HTTP; production validates HTTPS
        // before it starts its worker and configures the agent as HTTPS-only.
        let agent = ureq::Agent::config_builder()
            .proxy(None)
            .http_status_as_error(false)
            .build()
            .into();
        assert!(matches!(post(&agent, &url, &receipt), Delivery::Retry));
        assert!(matches!(post(&agent, &url, &receipt), Delivery::Accepted));
        receiver.join().unwrap();
        assert!(matches!(Delivery::from_status(503), Delivery::Retry));
        assert!(matches!(Delivery::from_status(408), Delivery::Retry));
        assert!(matches!(Delivery::from_status(200), Delivery::Refused));
        assert!(matches!(Delivery::from_status(403), Delivery::Refused));
    }
}
