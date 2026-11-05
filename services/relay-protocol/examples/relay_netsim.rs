//! Runs network-simulation scenarios against the in-process relay and
//! prints one JSON report per scenario and client kind, one per line
//! (services/relay/adaptive-transport/e2e-plan.md §1, §2.4). Nothing
//! touches a socket or the wall clock.
//!
//! - `--clients` adds every client's own row.
//! - `--client model|adaptive|both` overrides each scenario's `client`:
//!   `model` is the phase 1 model of the current production policy (the
//!   baseline), `adaptive` the production `ClientSession` scheduler, and
//!   `both` runs each scenario once with each, for the comparison of §2.4.
//!
//! ```sh
//! cargo build --release -p vera20k-relay-protocol --features test-support \
//!     --example relay_netsim
//! relay_netsim --client both services/relay-protocol/netsim/*.json
//! ```
use vera20k_relay_protocol::netsim::Simulation;
use vera20k_relay_protocol::netsim::scenario::{ClientKind, Scenario};

fn usage() -> ! {
    eprintln!("usage: relay_netsim [--clients] [--client model|adaptive|both] <scenario.json>...");
    std::process::exit(2);
}

fn main() {
    let mut clients = false;
    let mut kinds: Option<Vec<ClientKind>> = None;
    let mut paths = Vec::new();
    let mut arguments = std::env::args().skip(1);
    while let Some(argument) = arguments.next() {
        match argument.as_str() {
            "--clients" => clients = true,
            "--client" => {
                kinds = Some(match arguments.next().as_deref() {
                    Some("model") => vec![ClientKind::Model],
                    Some("adaptive") => vec![ClientKind::Adaptive],
                    Some("both") => vec![ClientKind::Model, ClientKind::Adaptive],
                    _ => usage(),
                })
            }
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
            for kind in kinds.clone().unwrap_or_else(|| vec![scenario.client]) {
                let mut scenario = scenario.clone();
                scenario.client = kind;
                let mut report = Simulation::new(scenario).run();
                if !clients {
                    report.clients.clear();
                }
                println!("{}", serde_json::to_string(&report).expect("report"));
            }
        }
    }
}
