//! Emit per-tick parity digests from a headless simulation run.
//!
//! Two modes:
//!
//! * `--map <file> --ra2-dir <path>` loads a real retail scenario with a pinned seed.
//!   This is the mode a cross-engine comparison uses: the same map and the same seed on
//!   both engines is the precondition for a divergence meaning anything.
//! * no `--map` runs a small synthetic two-house setup. It validates that real
//!   `Simulation` state serialises and survives the consumer's strict parsing — it says
//!   nothing about parity, because the scenario has no counterpart in gamemd.
//!
//! **What a real-map run contains (F09).** The scenario is constructed through the same
//! GPU-free funnel the app uses (see `headless_scenario`): map-roster houses, map-placed
//! units and structures, bridge state, wall ownership, smudges, and the seeded RNG
//! streams are all real. What it still lacks versus an app launch is the skirmish
//! *session* — player houses, start-position placement — so digests represent a
//! spectatorless load of the map as authored, not a played skirmish opening.

use std::path::PathBuf;

use vera20k::headless_scenario::{self, SIM_TICK_MS};
use vera20k::map::entities::EntityCategory;
use vera20k::sim::components::Health;
use vera20k::sim::house_state::HouseState;
use vera20k::sim::parity_digest::ParityDigestSink;
use vera20k::sim::world::Simulation;

const DEFAULT_TICKS: u64 = 600;
/// Arbitrary but fixed, so two synthetic runs are comparable to each other.
const DEFAULT_SEED: u32 = 0x5EED_0001;

struct Args {
    out: PathBuf,
    ticks: u64,
    seed: u32,
    map: Option<String>,
    ra2_dir: Option<PathBuf>,
}

fn parse_args() -> Result<Args, String> {
    let mut out: Option<PathBuf> = None;
    let mut ticks = DEFAULT_TICKS;
    let mut seed = DEFAULT_SEED;
    let mut map: Option<String> = None;
    let mut ra2_dir: Option<PathBuf> = None;
    let mut argv = std::env::args().skip(1);
    while let Some(flag) = argv.next() {
        let mut next = |what: &str| argv.next().ok_or(format!("{flag} needs {what}"));
        match flag.as_str() {
            "--out" => out = Some(PathBuf::from(next("a path")?)),
            "--ticks" => {
                ticks = next("a count")?
                    .parse()
                    .map_err(|_| "--ticks must be a positive integer".to_string())?;
            }
            "--seed" => {
                let raw = next("a 32-bit value")?;
                let parsed = raw
                    .strip_prefix("0x")
                    .map(|hex| u32::from_str_radix(hex, 16))
                    .unwrap_or_else(|| raw.parse());
                seed =
                    parsed.map_err(|_| "--seed must be a u32 (decimal or 0x-hex)".to_string())?;
            }
            "--map" => map = Some(next("a map file name")?),
            "--ra2-dir" => ra2_dir = Some(PathBuf::from(next("a path")?)),
            other => return Err(format!("unrecognised argument {other}")),
        }
    }
    if map.is_some() && ra2_dir.is_none() {
        return Err("--map also needs --ra2-dir naming the retail install".to_string());
    }
    Ok(Args {
        out: out.ok_or("--out <path> is required".to_string())?,
        ticks,
        seed,
        map,
        ra2_dir,
    })
}

/// Two houses with credits and a handful of entities.
///
/// Enough for every digest field to carry a non-trivial value; the runtime is
/// bound to empty resources, so no rules-driven behaviour runs.
fn build_synthetic_simulation(seed: u32) -> Simulation {
    let mut sim = Simulation::with_seed(u64::from(seed));

    for (index, (owner, side)) in [("Americans", 0u8), ("Russians", 1u8)].iter().enumerate() {
        let owner_id = sim.interner.intern(owner);
        let mut house = HouseState::new(owner_id, *side, None, index == 0, 10_000, 10);
        // The structures below are raw-inserted without lifecycle registration, so
        // the defeat scan would read both houses as owning nothing and resolve the
        // match on tick 1, freezing the committed-tick counter. Passive houses are
        // exempt from defeat evaluation, keeping every tick committable.
        house.multiplay_passive = true;
        sim.houses.insert(owner_id, house);

        let type_ref = sim.interner.intern("GACNST");
        let base_x = 10 + (index as u16) * 20;
        for slot in 0..3u16 {
            sim.insert_synthetic_techno_for_diagnostics(
                base_x + slot * 2,
                12,
                0,
                0,
                owner_id,
                Health { current: 1000 },
                type_ref,
                EntityCategory::Structure,
                0,
                6,
                false,
            );
        }
    }
    sim
}

fn main() -> Result<(), String> {
    let args = match parse_args() {
        Ok(args) => args,
        Err(error) => {
            eprintln!("{error}");
            eprintln!(
                "usage: parity-digest --out <path.jsonl> [--ticks N] [--seed S] \
                 [--map <file> --ra2-dir <retail path>]"
            );
            std::process::exit(2);
        }
    };

    let mut sink = ParityDigestSink::create(&args.out)
        .map_err(|error| format!("could not open {}: {error}", args.out.display()))?;

    match (&args.map, &args.ra2_dir) {
        (Some(map), Some(ra2_dir)) => {
            let mut scenario = headless_scenario::load(ra2_dir, map, args.seed)?;
            println!(
                "loaded {map} ({}x{}, theater {}) seed 0x{:08X}",
                scenario.sim().session.map_width,
                scenario.sim().session.map_height,
                scenario.map.header.theater,
                args.seed
            );
            for _ in 0..args.ticks {
                scenario.tick();
                let digest = scenario.sim().parity_digest();
                sink.write(&digest)
                    .map_err(|error| format!("digest write failed: {error}"))?;
            }
        }
        _ => {
            // F09: the synthetic run advances through the same bound-resource
            // runtime transaction as everything else. Empty resources stand in
            // for the "no rules loaded" contract the synthetic scenario always
            // had; synthetic digests are self-comparable, not parity evidence.
            let mut runtime = vera20k::sim::runtime::SimRuntime {
                simulation: build_synthetic_simulation(args.seed),
                resources: vera20k::sim::runtime::SimResources::empty(),
            };
            for _ in 0..args.ticks {
                runtime
                    .advance_frame_for_tooling(&[], SIM_TICK_MS)
                    .map_err(|error| error.to_string())?;
                let digest = runtime.simulation.parity_digest();
                sink.write(&digest)
                    .map_err(|error| format!("digest write failed: {error}"))?;
            }
        }
    }

    println!("wrote {} digests to {}", sink.written(), args.out.display());
    Ok(())
}
