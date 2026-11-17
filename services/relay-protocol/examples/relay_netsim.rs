//! Runs network-simulation scenarios against the in-process relay and
//! prints one JSON report per scenario, one per line
//! (services/relay/adaptive-transport/e2e-plan.md §1, §2.4). Nothing
//! touches a socket or the wall clock. Clients are the production
//! `ClientSession` scheduler; the old protocol's baseline is this runner at
//! commit 720f12400 on the same files. `--clients` adds every client's row.
//!
//! ```sh
//! cargo build --release -p vera20k-relay-protocol --features test-support \
//!     --example relay_netsim
//! relay_netsim services/relay-protocol/netsim/*.json
//! ```
use vera20k_relay_protocol::netsim::Simulation;
use vera20k_relay_protocol::netsim::scenario::Scenario;

fn usage() -> ! {
    eprintln!("usage: relay_netsim [--clients] <scenario.json>...");
    std::process::exit(2);
}

fn main() {
    let mut clients = false;
    let mut paths = Vec::new();
    for argument in std::env::args().skip(1) {
        match argument.as_str() {
            "--clients" => clients = true,
            _ => paths.push(argument),
        }
    }
    if paths.is_empty() {
        usage();
    }
    for path in paths {
        let scenarios = std::fs::read_to_string(&path)
            .map_err(|error| error.to_string())
            .and_then(|text| Scenario::list_from_json(&text));
        let scenarios = match scenarios {
            Ok(scenarios) => scenarios,
            Err(error) => {
                eprintln!("{path}: {error}");
                std::process::exit(2);
            }
        };
        for scenario in scenarios {
            let mut report = Simulation::new(scenario).run();
            if !clients {
                report.clients.clear();
            }
            println!("{}", serde_json::to_string(&report).expect("report"));
        }
    }
}
