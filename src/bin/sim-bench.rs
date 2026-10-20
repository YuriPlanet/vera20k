//! Release-build simulation tick benchmark at scale.
//!
//! Loads a retail map headlessly (the same GPU-free funnel as `parity-digest`),
//! sets up one of two scenarios, then times each `SimRuntime` frame and prints
//! the distribution.
//!
//! `--scenario march` (the default) adds `--houses` hostile houses on a ring
//! around the map centre, spawns `--units` objects in formation around each
//! house's home cell, and orders every object to attack-move to the opposite
//! house's home. Its houses own no buildings and are passive, so objects march
//! and jam but almost never fight: it measures movement and path searches.
//! `--return-at T` also gives every object a second attack-move, back home,
//! from tick T, spread like the first orders, so movers already under way
//! take a new destination at scale.
//!
//! `--scenario match` plays the middle of a multiplayer match instead (see
//! `sim_bench_match`): the map loads as a Battle against `--computers`
//! computer houses (by default the lobby's most, 7), the other start waypoints
//! get human players' houses, and every house has a finished base, harvesters,
//! factories that keep building and a mixed army. Their own AI plays the
//! computer houses; a script plays the human ones, sending their armies at
//! the nearest bases in squads until those fall. Objects fight, die, are
//! rebuilt and harvest, so it measures a battle. `--houses` counts both kinds
//! and defaults to every start but the local house's; `--ticks` defaults to
//! 1800, and `after_orders_mean_ms` skips the first 150. Every
//! `PROGRESS_EVERY` ticks it prints what the houses have and have done.
//!
//! The run is deterministic for a given map, seed and arguments: the final
//! `Simulation::state_hash` (and the optional periodic hashes, taken outside
//! the timed region) identify the simulation result, so two builds can be
//! compared for identical behavior as well as for speed. The periodic hashes
//! are timed on their own, which measures what each hashing frame also takes.
//! The scenarios' own work (choosing each frame's commands) is not timed.
//!
//! Frames hash their own state as normal play does, every
//! `sim::replay::PLAY_HASH_INTERVAL`th frame; `--frame-hash-every 1` hashes
//! every frame.
//!
//! `--map` also takes a loose map file; `python -m tools.scale_map` writes the
//! flat 500x500-cell map used for 20,000-unit, 30-house march runs, and with
//! `--starts 30` the match map with ore and trees.
//!
//! The simulation batches its path searches and target scans as a real
//! match does; `--serial-paths` searches and `--serial-scans` scans in native
//! order instead.
//!
//! Beside each frame's wall time, the CSV's `cpu_ms` column and the
//! `main_thread_cpu_*` line give the main thread's CPU time (Unix only). On a
//! machine other work keeps busy it moves far less than wall time, so it
//! compares two builds there; it leaves out the worker pool's share.
//!
//! Build with `cargo build --release -p vera20k-core --bin sim-bench`; timings from a debug
//! build are not meaningful.

mod sim_bench_match;

use std::num::NonZeroU64;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use vera20k_core::headless_scenario::{self, SIM_TICK_MS};
use vera20k_core::sim::command::{Command, CommandEnvelope};
use vera20k_core::sim::house_state::HouseState;
use vera20k_core::sim::intern::InternedId;
use vera20k_core::sim::runtime::SimRuntime;

use sim_bench_match::MatchScenario;

/// The match scenario's progress line cadence, in ticks.
const PROGRESS_EVERY: u64 = 100;

#[derive(Clone, Copy, PartialEq)]
enum ScenarioKind {
    March,
    Match,
}

struct Args {
    ra2_dir: PathBuf,
    map: String,
    scenario: ScenarioKind,
    seed: u32,
    units: usize,
    houses: Option<usize>,
    computers: Option<usize>,
    ticks: Option<NonZeroU64>,
    types: Vec<String>,
    order_spread: u64,
    return_at: Option<u64>,
    hash_every: u64,
    frame_hash_every: NonZeroU64,
    csv: Option<PathBuf>,
    serial_paths: bool,
    serial_scans: bool,
}

fn parse_args() -> Result<Args, String> {
    let mut args = Args {
        ra2_dir: std::env::var_os("RA2_DIR")
            .map(PathBuf::from)
            .unwrap_or_default(),
        map: "Death.mmx".to_string(),
        scenario: ScenarioKind::March,
        seed: 0x5EED_0001,
        units: 20_000,
        houses: None,
        computers: None,
        ticks: None,
        types: ["E1", "E2", "MTNK", "HTNK"].map(String::from).to_vec(),
        order_spread: 30,
        return_at: None,
        hash_every: 0,
        frame_hash_every: vera20k_core::sim::replay::PLAY_HASH_INTERVAL,
        csv: None,
        serial_paths: false,
        serial_scans: false,
    };
    let mut march_flag = None;
    let mut argv = std::env::args().skip(1);
    while let Some(flag) = argv.next() {
        if ["--types", "--order-spread", "--return-at"].contains(&flag.as_str()) {
            march_flag.get_or_insert_with(|| flag.clone());
        }
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
            "--scenario" => {
                args.scenario = match value()?.as_str() {
                    "march" => ScenarioKind::March,
                    "match" => ScenarioKind::Match,
                    other => return Err(format!("unknown scenario {other}: march or match")),
                }
            }
            "--seed" => {
                args.seed = u32::try_from(number(value()?)?).map_err(|_| "--seed is 32-bit")?
            }
            "--units" => args.units = number(value()?)? as usize,
            "--houses" => args.houses = Some(number(value()?)? as usize),
            "--computers" => args.computers = Some(number(value()?)? as usize),
            "--ticks" => {
                args.ticks = Some(
                    NonZeroU64::new(number(value()?)?).ok_or("--ticks needs a positive number")?,
                )
            }
            "--types" => args.types = value()?.split(',').map(str::to_string).collect(),
            "--order-spread" => args.order_spread = number(value()?)?.max(1),
            "--return-at" => args.return_at = Some(number(value()?)?),
            "--hash-every" => args.hash_every = number(value()?)?,
            "--frame-hash-every" => {
                args.frame_hash_every = NonZeroU64::new(number(value()?)?)
                    .ok_or("--frame-hash-every needs a positive number")?
            }
            "--csv" => args.csv = Some(PathBuf::from(value()?)),
            "--serial-paths" => args.serial_paths = true,
            "--serial-scans" => args.serial_scans = true,
            other => return Err(format!("unrecognised argument {other}")),
        }
    }
    if args.ra2_dir.as_os_str().is_empty() {
        return Err("--ra2-dir (or RA2_DIR) must name the retail install".to_string());
    }
    if args.houses.is_some_and(|houses| houses < 2) || args.types.is_empty() {
        return Err("need at least two houses and one unit type".to_string());
    }
    let matched = args.scenario == ScenarioKind::Match;
    if !matched && args.computers.is_some() {
        return Err("--computers is a match option".to_string());
    }
    if matched && let Some(flag) = march_flag {
        return Err(format!("{flag} is a march option"));
    }
    if args
        .computers
        .is_some_and(|computers| computers > sim_bench_match::COMPUTERS)
    {
        return Err(format!(
            "a Battle has at most {} computer houses",
            sim_bench_match::COMPUTERS
        ));
    }
    Ok(args)
}

/// Every map cell of the live terrain, in (rx, ry) order.
fn map_cells(runtime: &SimRuntime) -> Vec<(u16, u16)> {
    let mut cells: Vec<(u16, u16)> = runtime
        .simulation
        .resolved_terrain
        .iter()
        .flat_map(|terrain| terrain.iter().map(|cell| (cell.rx, cell.ry)))
        .collect();
    cells.sort_unstable();
    cells
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
            if let Some(id) = simulation.spawn_object(type_id, owner, rx, ry, 0, &resources.rules) {
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

/// The march scenario's houses and armies, and every frame's commands.
fn march(
    runtime: &mut SimRuntime,
    args: &Args,
    houses: usize,
) -> (usize, Vec<Vec<CommandEnvelope>>) {
    let owners: Vec<String> = (0..houses).map(|i| format!("Bench{i:02}")).collect();
    let owner_ids: Vec<InternedId> = owners
        .iter()
        .enumerate()
        .map(|(index, name)| {
            let sim = &mut runtime.simulation;
            let id = sim.interner.intern(name);
            let mut house = HouseState::new(id, (index % 2) as u8, None, false, 0, 10);
            // No house owns a building, so exempt every house from the defeat
            // scan; objects still fight because only same-house pairs are allied.
            house.make_multiplay_passive();
            sim.houses.register(house);
            id
        })
        .collect();
    let homes = home_cells(&map_cells(runtime), houses);
    let spawned = spawn_armies(runtime, args, &owners, &homes);
    runtime
        .simulation
        .resolve_type_handles(&runtime.resources.rules);

    // Each army marches in formation to the opposite house's home, and with
    // --return-at later marches back to its own.
    let first_tick = runtime.simulation.session.tick() + 1;
    let mut orders: Vec<Vec<CommandEnvelope>> = Vec::new();
    let clamp = |base: u16, delta: i32| (i32::from(base) + delta).clamp(0, 511) as u16;
    let waves = [(0, houses / 2)]
        .into_iter()
        .chain(args.return_at.map(|at| (at, 0)));
    for (start, house_shift) in waves {
        for (index, unit) in spawned.iter().enumerate() {
            let target_home = homes[(unit.house + house_shift) % houses];
            let slot = start + index as u64 % args.order_spread;
            if orders.len() <= slot as usize {
                orders.resize(slot as usize + 1, Vec::new());
            }
            orders[slot as usize].push(CommandEnvelope::new(
                owner_ids[unit.house],
                first_tick + slot,
                Command::AttackMove {
                    entity_id: unit.id,
                    target: None,
                    destination: Some(vera20k_core::sim::combat::TargetKind::Cell(
                        clamp(target_home.0, unit.offset.0),
                        clamp(target_home.1, unit.offset.1),
                    )),
                },
            ));
        }
    }
    (spawned.len(), orders)
}

enum Plan {
    March(Vec<Vec<CommandEnvelope>>),
    Match(MatchScenario),
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
                "usage: sim-bench [--ra2-dir <retail path>] [--map Death.mmx] \
                 [--scenario march|match] [--seed S] [--units 20000] [--houses N] \
                 [--computers 7 (match)] [--ticks 300 (match: 1800)] \
                 [--types E1,E2,MTNK,HTNK] [--order-spread 30] [--return-at T] (march) \
                 [--hash-every N] [--frame-hash-every 15] [--csv <path>] [--serial-paths] [--serial-scans]"
            );
            std::process::exit(2);
        }
    };
    let load_started = Instant::now();
    let mut scenario = match args.scenario {
        ScenarioKind::March => headless_scenario::load(&args.ra2_dir, &args.map, args.seed)?,
        ScenarioKind::Match => {
            let computers = args
                .computers
                .unwrap_or(sim_bench_match::COMPUTERS)
                .min(args.houses.unwrap_or(usize::MAX));
            headless_scenario::load_session(
                &args.ra2_dir,
                &args.map,
                args.seed,
                sim_bench_match::session(&args.map, computers),
            )?
        }
    };
    if !args.serial_paths {
        scenario.runtime.simulation.batch_path_searches();
    }
    if !args.serial_scans {
        scenario.runtime.simulation.batch_target_scans();
    }
    scenario
        .runtime
        .simulation
        .hash_state_every(args.frame_hash_every);

    let (houses, spawned, mut plan, ticks) = match args.scenario {
        ScenarioKind::March => {
            let houses = args.houses.unwrap_or(8);
            let (spawned, orders) = march(&mut scenario.runtime, &args, houses);
            let ticks = args.ticks.map_or(300, NonZeroU64::get);
            (houses, spawned, Plan::March(orders), ticks)
        }
        ScenarioKind::Match => {
            let script = MatchScenario::setup(&mut scenario, args.houses, args.units)?;
            let ticks = args.ticks.map_or(sim_bench_match::TICKS, NonZeroU64::get);
            (script.houses(), script.spawned, Plan::Match(script), ticks)
        }
    };
    // The steady window leaves out the march's spread first orders and the
    // match's launch and first contacts.
    let warmup = match plan {
        Plan::March(_) => args.order_spread,
        Plan::Match(_) => sim_bench_match::WARMUP,
    };
    let computers = match &plan {
        Plan::March(_) => 0,
        Plan::Match(script) => script.computers(),
    };
    let runtime = &mut scenario.runtime;
    let live_at_start = runtime.simulation.entities().len();
    println!(
        "loaded {} seed 0x{:08X} in {:.1}s: {} houses ({} computer), {} spawned, {} live objects",
        args.map,
        args.seed,
        load_started.elapsed().as_secs_f64(),
        houses,
        computers,
        spawned,
        live_at_start
    );
    if let Plan::Match(script) = &plan {
        println!("progress tick=0 {}", script.progress(runtime));
    }

    let mut times = Vec::with_capacity(ticks as usize);
    let mut cpu_times = Vec::with_capacity(ticks as usize);
    let mut hashes = Vec::new();
    let mut hash_times = Vec::new();
    for tick in 0..ticks {
        let batch = match &mut plan {
            Plan::March(orders) => orders.get(tick as usize).cloned().unwrap_or_default(),
            Plan::Match(script) => script.orders(runtime, tick),
        };
        let cpu_started = thread_cpu_time();
        let started = Instant::now();
        runtime
            .advance_frame_for_tooling(&batch, SIM_TICK_MS)
            .map_err(|error| format!("tick {tick}: {error}"))?;
        times.push(started.elapsed());
        cpu_times.push(
            cpu_started
                .zip(thread_cpu_time())
                .map(|(started, ended)| ended.saturating_sub(started)),
        );
        if args.hash_every > 0 && (tick + 1) % args.hash_every == 0 {
            let started = Instant::now();
            let hash = runtime.simulation.state_hash();
            hash_times.push(started.elapsed());
            hashes.push((tick + 1, hash));
        }
        if let Plan::Match(script) = &plan
            && (tick + 1) % PROGRESS_EVERY == 0
        {
            let recent = &times[times.len().saturating_sub(PROGRESS_EVERY as usize)..];
            let mean: Duration = recent.iter().sum::<Duration>() / recent.len() as u32;
            println!(
                "progress tick={} mean_ms={:.1} {}",
                tick + 1,
                mean.as_secs_f64() * 1000.0,
                script.progress(runtime)
            );
        }
    }
    let final_hash = runtime.simulation.state_hash();
    let live_at_end = runtime.simulation.entities().len();

    if let Some(path) = &args.csv {
        let mut text = String::from("tick,ms,cpu_ms\n");
        for (tick, (time, cpu)) in times.iter().zip(&cpu_times).enumerate() {
            text.push_str(&format!(
                "{},{:.3},{}\n",
                tick + 1,
                time.as_secs_f64() * 1000.0,
                cpu.map_or(String::new(), |cpu| format!(
                    "{:.3}",
                    cpu.as_secs_f64() * 1000.0
                ))
            ));
        }
        std::fs::write(path, text).map_err(|error| format!("{}: {error}", path.display()))?;
    }
    let total: Duration = times.iter().sum();
    let steady: Vec<Duration> = times.iter().skip(warmup as usize).copied().collect();
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
    let mut cpu_sorted: Vec<Duration> = cpu_times.iter().flatten().copied().collect();
    if cpu_sorted.len() == times.len() && !cpu_sorted.is_empty() {
        let cpu_total: Duration = cpu_sorted.iter().sum();
        cpu_sorted.sort_unstable();
        println!(
            "main_thread_cpu_mean_ms={:.2} p50_ms={:.2} p95_ms={:.2} max_ms={:.2}",
            cpu_total.as_secs_f64() * 1000.0 / cpu_sorted.len() as f64,
            percentile(&cpu_sorted, 0.5),
            percentile(&cpu_sorted, 0.95),
            percentile(&cpu_sorted, 1.0),
        );
    }
    if !steady.is_empty() {
        let steady_total: Duration = steady.iter().sum();
        println!(
            "after_orders_mean_ms={:.2} (ticks {}..={})",
            steady_total.as_secs_f64() * 1000.0 / steady.len() as f64,
            warmup + 1,
            times.len()
        );
    }
    for (tick, hash) in &hashes {
        println!("hash tick={tick} {hash:016x}");
    }
    if !hash_times.is_empty() {
        let total: Duration = hash_times.iter().sum();
        hash_times.sort_unstable();
        println!(
            "periodic_hash_mean_ms={:.3} p50_ms={:.3}",
            total.as_secs_f64() * 1000.0 / hash_times.len() as f64,
            percentile(&hash_times, 0.5)
        );
    }
    println!("live_objects start={live_at_start} end={live_at_end}");
    println!("final_hash={final_hash:016x}");
    Ok(())
}

/// The calling thread's CPU time, where the platform reports it.
fn thread_cpu_time() -> Option<Duration> {
    #[cfg(unix)]
    {
        let mut time = std::mem::MaybeUninit::<libc::timespec>::uninit();
        // SAFETY: clock_gettime only writes the timespec it is given, and
        // that timespec is read only when the call reports success.
        if unsafe { libc::clock_gettime(libc::CLOCK_THREAD_CPUTIME_ID, time.as_mut_ptr()) } == 0 {
            let time = unsafe { time.assume_init() };
            return Some(Duration::new(time.tv_sec as u64, time.tv_nsec as u32));
        }
    }
    None
}
