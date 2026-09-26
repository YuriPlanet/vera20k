//! Release-build simulation tick benchmark at scale.
//!
//! Loads a retail map headlessly (the same GPU-free funnel as `parity-digest`),
//! adds `--houses` hostile houses on a ring around the map centre, spawns
//! `--units` objects in formation around each house's home cell, and orders
//! every object to attack-move to the opposite house's home. It then times
//! each `SimRuntime` frame and prints the distribution.
//!
//! `--queue-at T` also gives every object a queued attack-move back home from
//! tick T, spread like the first orders. A Walk mover already under way
//! searches its path when such an order is given, so this exercises the
//! order-time path search (owner block sets and blocker plane) at scale.
//!
//! The run is deterministic for a given map, seed and arguments: the final
//! `Simulation::state_hash` (and the optional periodic hashes, taken outside
//! the timed region) identify the simulation result, so two builds can be
//! compared for identical behavior as well as for speed.
//!
//! Build with `cargo build --release --bin sim-bench`; timings from a debug
//! build are not meaningful.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use vera20k::headless_scenario::{self, SIM_TICK_MS};
use vera20k::sim::command::{Command, CommandEnvelope};
use vera20k::sim::house_state::HouseState;
use vera20k::sim::intern::InternedId;
use vera20k::sim::runtime::SimRuntime;

struct Args {
    ra2_dir: PathBuf,
    map: String,
    seed: u32,
    units: usize,
    houses: usize,
    ticks: u64,
    types: Vec<String>,
    order_spread: u64,
    queue_at: Option<u64>,
    hash_every: u64,
    csv: Option<PathBuf>,
}

fn parse_args() -> Result<Args, String> {
    let mut args = Args {
        ra2_dir: std::env::var_os("RA2_DIR")
            .map(PathBuf::from)
            .unwrap_or_default(),
        map: "Death.mmx".to_string(),
        seed: 0x5EED_0001,
        units: 20_000,
        houses: 8,
        ticks: 300,
        types: ["E1", "E2", "MTNK", "HTNK"].map(String::from).to_vec(),
        order_spread: 30,
        queue_at: None,
        hash_every: 0,
        csv: None,
    };
    let mut argv = std::env::args().skip(1);
    while let Some(flag) = argv.next() {
        let mut value = || argv.next().ok_or(format!("{flag} needs a value"));
        let number = |raw: String| -> Result<u64, String> {
            raw.strip_prefix("0x")
                .map(|hex| u64::from_str_radix(hex, 16))
                .unwrap_or_else(|| raw.parse())
                .map_err(|_| format!("{flag} needs a number"))
        };
        match flag.as_str() {
            "--ra2-dir" => args.ra2_dir = PathBuf::from(value()?),
            "--map" => args.map = value()?,
            "--seed" => {
                args.seed = u32::try_from(number(value()?)?).map_err(|_| "--seed is 32-bit")?
            }
            "--units" => args.units = number(value()?)? as usize,
            "--houses" => args.houses = number(value()?)? as usize,
            "--ticks" => args.ticks = number(value()?)?,
            "--types" => args.types = value()?.split(',').map(str::to_string).collect(),
            "--order-spread" => args.order_spread = number(value()?)?.max(1),
            "--queue-at" => args.queue_at = Some(number(value()?)?),
            "--hash-every" => args.hash_every = number(value()?)?,
            "--csv" => args.csv = Some(PathBuf::from(value()?)),
            other => return Err(format!("unrecognised argument {other}")),
        }
    }
    if args.ra2_dir.as_os_str().is_empty() {
        return Err("--ra2-dir (or RA2_DIR) must name the retail install".to_string());
    }
    if args.houses < 2 || args.types.is_empty() {
        return Err("need at least two houses and one unit type".to_string());
    }
    Ok(args)
}

/// Every map cell, taken from the bound terrain height table.
fn map_cells(runtime: &SimRuntime) -> Vec<(u16, u16)> {
    runtime.resources.height_map.keys().copied().collect()
}

/// Home cells on a ring around the map centre, in screen-aligned (rx - ry,
/// rx + ry) space so the ring follows the isometric diamond.
fn home_cells(cells: &[(u16, u16)], houses: usize) -> Vec<(u16, u16)> {
    let (mut u_min, mut u_max, mut v_min, mut v_max) = (i32::MAX, i32::MIN, i32::MAX, i32::MIN);
    for &(rx, ry) in cells {
        let (u, v) = (i32::from(rx) - i32::from(ry), i32::from(rx) + i32::from(ry));
        u_min = u_min.min(u);
        u_max = u_max.max(u);
        v_min = v_min.min(v);
        v_max = v_max.max(v);
    }
    let (u_mid, v_mid) = (
        f64::from(u_min + u_max) / 2.0,
        f64::from(v_min + v_max) / 2.0,
    );
    let (u_radius, v_radius) = (
        f64::from(u_max - u_min) * 0.36,
        f64::from(v_max - v_min) * 0.36,
    );
    (0..houses)
        .map(|index| {
            let angle = std::f64::consts::TAU * index as f64 / houses as f64;
            let u = u_mid + u_radius * angle.cos();
            let v = v_mid + v_radius * angle.sin();
            let rx = ((v + u) / 2.0).round().max(0.0) as u16;
            let ry = ((v - u) / 2.0).round().max(0.0) as u16;
            (rx, ry)
        })
        .collect()
}

struct Spawned {
    id: u64,
    house: usize,
    offset: (i32, i32),
}

fn spawn_armies(
    runtime: &mut SimRuntime,
    args: &Args,
    owners: &[String],
    homes: &[(u16, u16)],
) -> Vec<Spawned> {
    let cells = map_cells(runtime);
    let per_house = args.units / owners.len();
    let mut spawned = Vec::with_capacity(args.units);
    for (house, (owner, &home)) in owners.iter().zip(homes).enumerate() {
        let quota = per_house + usize::from(house < args.units % owners.len());
        // Nearest cells first; ties broken by cell so the order is total.
        let mut ring: Vec<(i32, (u16, u16))> = cells
            .iter()
            .map(|&(rx, ry)| {
                let (dx, dy) = (
                    i32::from(rx) - i32::from(home.0),
                    i32::from(ry) - i32::from(home.1),
                );
                (dx * dx + dy * dy, (rx, ry))
            })
            .collect();
        ring.sort_unstable();
        let mut placed = 0;
        for (_, (rx, ry)) in ring {
            if placed == quota {
                break;
            }
            let type_id = &args.types[placed % args.types.len()];
            let SimRuntime {
                simulation,
                resources,
            } = &mut *runtime;
            if let Some(id) = simulation.spawn_object(
                type_id,
                owner,
                rx,
                ry,
                0,
                &resources.rules,
                &resources.height_map,
            ) {
                spawned.push(Spawned {
                    id,
                    house,
                    offset: (
                        i32::from(rx) - i32::from(home.0),
                        i32::from(ry) - i32::from(home.1),
                    ),
                });
                placed += 1;
            }
        }
        if placed < quota {
            eprintln!("house {owner}: placed {placed} of {quota} (map full)");
        }
    }
    spawned
}

fn percentile(sorted: &[Duration], fraction: f64) -> f64 {
    let index = ((sorted.len() as f64 - 1.0) * fraction).round() as usize;
    sorted[index].as_secs_f64() * 1000.0
}

fn main() -> Result<(), String> {
    let args = match parse_args() {
        Ok(args) => args,
        Err(error) => {
            eprintln!("{error}");
            eprintln!(
                "usage: sim-bench [--ra2-dir <retail path>] [--map Death.mmx] [--seed S] \
                 [--units 20000] [--houses 8] [--ticks 300] [--types E1,E2,MTNK,HTNK] \
                 [--order-spread 30] [--queue-at T] [--hash-every N] [--csv <path>]"
            );
            std::process::exit(2);
        }
    };
    let load_started = Instant::now();
    let mut scenario = headless_scenario::load(&args.ra2_dir, &args.map, args.seed)?;
    let runtime = &mut scenario.runtime;

    let owners: Vec<String> = (0..args.houses).map(|i| format!("Bench{i:02}")).collect();
    let owner_ids: Vec<InternedId> = owners
        .iter()
        .enumerate()
        .map(|(index, name)| {
            let sim = &mut runtime.simulation;
            let id = sim.interner.intern(name);
            let mut house = HouseState::new(id, (index % 2) as u8, None, false, 0, 10);
            // No house owns a building, so exempt every house from the defeat
            // scan; objects still fight because only same-house pairs are allied.
            house.multiplay_passive = true;
            sim.houses.insert(id, house);
            sim.session.house_order.push(id);
            id
        })
        .collect();
    let homes = home_cells(&map_cells(runtime), args.houses);
    let spawned = spawn_armies(runtime, &args, &owners, &homes);
    runtime
        .simulation
        .resolve_type_handles(&runtime.resources.rules);
    let live_at_start = runtime.simulation.entities().len();
    println!(
        "loaded {} seed 0x{:08X} in {:.1}s: {} houses, {} spawned, {} live objects",
        args.map,
        args.seed,
        load_started.elapsed().as_secs_f64(),
        args.houses,
        spawned.len(),
        live_at_start
    );

    // Each army marches in formation to the opposite house's home, and with
    // --queue-at later queues a march back to its own.
    let first_tick = runtime.simulation.session.tick + 1;
    let mut orders: Vec<Vec<CommandEnvelope>> = Vec::new();
    let clamp = |base: u16, delta: i32| (i32::from(base) + delta).clamp(0, 511) as u16;
    let waves = [(0, false, args.houses / 2)]
        .into_iter()
        .chain(args.queue_at.map(|at| (at, true, 0)));
    for (start, queue, house_shift) in waves {
        for (index, unit) in spawned.iter().enumerate() {
            let target_home = homes[(unit.house + house_shift) % args.houses];
            let slot = start + index as u64 % args.order_spread;
            if orders.len() <= slot as usize {
                orders.resize(slot as usize + 1, Vec::new());
            }
            orders[slot as usize].push(CommandEnvelope::new(
                owner_ids[unit.house],
                first_tick + slot,
                Command::AttackMove {
                    entity_id: unit.id,
                    target_rx: clamp(target_home.0, unit.offset.0),
                    target_ry: clamp(target_home.1, unit.offset.1),
                    queue,
                },
            ));
        }
    }

    let mut times = Vec::with_capacity(args.ticks as usize);
    let mut hashes = Vec::new();
    for tick in 0..args.ticks {
        let batch = orders.get(tick as usize).map_or(&[][..], Vec::as_slice);
        let started = Instant::now();
        runtime
            .advance_frame_for_tooling(batch, SIM_TICK_MS)
            .map_err(|error| format!("tick {tick}: {error}"))?;
        times.push(started.elapsed());
        if args.hash_every > 0 && (tick + 1) % args.hash_every == 0 {
            hashes.push((tick + 1, runtime.simulation.state_hash()));
        }
    }
    let final_hash = runtime.simulation.state_hash();
    let live_at_end = runtime.simulation.entities().len();

    if let Some(path) = &args.csv {
        let mut text = String::from("tick,ms\n");
        for (tick, time) in times.iter().enumerate() {
            text.push_str(&format!(
                "{},{:.3}\n",
                tick + 1,
                time.as_secs_f64() * 1000.0
            ));
        }
        std::fs::write(path, text).map_err(|error| format!("{}: {error}", path.display()))?;
    }
    let total: Duration = times.iter().sum();
    let steady: Vec<Duration> = times
        .iter()
        .skip(args.order_spread as usize)
        .copied()
        .collect();
    let mut sorted = times.clone();
    sorted.sort_unstable();
    println!(
        "ticks={} total_s={:.2} mean_ms={:.2} p50_ms={:.2} p95_ms={:.2} max_ms={:.2}",
        times.len(),
        total.as_secs_f64(),
        total.as_secs_f64() * 1000.0 / times.len().max(1) as f64,
        percentile(&sorted, 0.5),
        percentile(&sorted, 0.95),
        percentile(&sorted, 1.0),
    );
    if !steady.is_empty() {
        let steady_total: Duration = steady.iter().sum();
        println!(
            "after_orders_mean_ms={:.2} (ticks {}..={})",
            steady_total.as_secs_f64() * 1000.0 / steady.len() as f64,
            args.order_spread + 1,
            times.len()
        );
    }
    for (tick, hash) in &hashes {
        println!("hash tick={tick} {hash:016x}");
    }
    println!("live_objects start={live_at_start} end={live_at_end}");
    println!("final_hash={final_hash:016x}");
    Ok(())
}
