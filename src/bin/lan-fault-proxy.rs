//! Explicit core-only test tool; normal game binaries have no fault hook.
use std::io::{self, BufRead, Read, Write};
use std::sync::mpsc;
use std::time::Duration;
use vera20k_core::net::fault_proxy::{Config, Control, run};

fn main() {
    if execute().is_err() {
        eprintln!("Owned LAN fault proxy failed; no packet payload was printed.");
        std::process::exit(1);
    }
}
fn execute() -> io::Result<()> {
    let mut args = std::env::args().skip(1);
    let (mut host_proxy, mut guest_proxy, mut host, mut guest, mut timeout) =
        (None, None, None, None, None);
    while let Some(flag) = args.next() {
        let value = args
            .next()
            .ok_or_else(|| io::Error::other("missing proxy argument"))?;
        match flag.as_str() {
            "--host-proxy" if host_proxy.is_none() => {
                host_proxy = Some(
                    value
                        .parse()
                        .map_err(|_| io::Error::other("invalid host proxy"))?,
                )
            }
            "--guest-proxy" if guest_proxy.is_none() => {
                guest_proxy = Some(
                    value
                        .parse()
                        .map_err(|_| io::Error::other("invalid guest proxy"))?,
                )
            }
            "--host" if host.is_none() => {
                host = Some(
                    value
                        .parse()
                        .map_err(|_| io::Error::other("invalid host"))?,
                )
            }
            "--guest" if guest.is_none() => {
                guest = Some(
                    value
                        .parse()
                        .map_err(|_| io::Error::other("invalid guest"))?,
                )
            }
            "--timeout" if timeout.is_none() => {
                timeout = Some(
                    value
                        .parse::<u64>()
                        .map_err(|_| io::Error::other("invalid lifetime"))?,
                )
            }
            _ => return Err(io::Error::other("invalid proxy arguments")),
        }
    }
    let config = Config {
        host_proxy: host_proxy.ok_or_else(|| io::Error::other("missing host proxy"))?,
        guest_proxy: guest_proxy.ok_or_else(|| io::Error::other("missing guest proxy"))?,
        host: host.ok_or_else(|| io::Error::other("missing host"))?,
        guest: guest.ok_or_else(|| io::Error::other("missing guest"))?,
        timeout: Duration::from_secs(timeout.ok_or_else(|| io::Error::other("missing lifetime"))?),
    };
    let (send, controls) = mpsc::sync_channel(2);
    std::thread::spawn(move || {
        let input = io::stdin();
        let mut input = input.lock();
        let mut line = Vec::with_capacity(6);
        loop {
            line.clear();
            let read = Read::by_ref(&mut input)
                .take(6)
                .read_until(b'\n', &mut line);
            let control = match (read, line.as_slice()) {
                (Ok(4), b"ARM\n") => Control::Arm,
                (Ok(5), b"STOP\n") => Control::Stop,
                _ => break,
            };
            if send.send(control).is_err() {
                return;
            }
        }
        let _ = send.send(Control::Stop);
    });
    run(config, controls, |notice| {
        let mut output = io::stdout().lock();
        serde_json::to_writer(&mut output, &notice).map_err(io::Error::other)?;
        output.write_all(b"\n")?;
        output.flush()
    })
}
