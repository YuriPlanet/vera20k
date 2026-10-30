//! Release-build simulation tick benchmark at scale.
//!
//! Loads a retail map headlessly (the same GPU-free funnel as `parity-digest`),
//! sets up one of three scenarios, then times each `SimRuntime` frame and prints
//! the distribution.
//!
//! `--scenario march` (the default) adds `--houses` hostile houses on a ring
//! around the map centre, spawns `--units` objects in formation around each
//! house's home cell, and orders every object to attack-move to the opposite
//! house's home. Its houses are human players' houses built by the launch's
//! house constructor, with the match scenario's countries; they own no
//! buildings and are passive, so objects march and jam but almost never
//! fight: it measures movement and path searches.
//! `--return-at T` also gives every object a second attack-move, back home,
//! from tick T, spread like the first orders, so movers already under way
//! take a new destination at scale.
//!
//! `--scenario match` plays the middle of a multiplayer match instead (see
//! `sim_bench_match`): the map loads as a Battle against `--computers`
//! computer houses (by default the most the lobby admits on the map, at most
//! 7), the other start waypoints get human players' houses, and every house
//! has a finished base, harvesters, factories that keep building and a mixed
//! army. Their own AI plays the computer houses; a script plays the human
//! ones, sending their armies at the nearest bases in squads until those
//! fall. Objects fight, die, are rebuilt and harvest, so it measures a
//! battle. `--houses` counts both kinds and defaults to every start but the
//! local house's. An explicit `--houses` filling every start also plays the
//! local house, so `--houses 30` uses all 30 starts. `--ticks` defaults to 1800,
//! and `after_orders_mean_ms` skips the first 150. Every `PROGRESS_EVERY` ticks it prints what the
//! houses have and have done.
//!
//! `--scenario skirmish` plays a whole skirmish from its start: the map loads
//! as a Battle against `--computers` computer houses (by default, as in a
//! match, the most the lobby admits on the map) with the lobby's default
//! options, and their own AI plays each from its starting units: it deploys
//! its MCV, builds and harvests, produces, forms teams and attacks. The
//! launch's local house sits the match out, as in the match scenario, and no
//! command is issued. `--ticks` defaults to 9000, ten minutes of game time.
//!
//! The run is deterministic for a given map, seed and arguments: the final
//! `Simulation::state_hash` (and the optional periodic hashes, taken outside
//! the timed region) identify the simulation result, so two builds can be
//! compared for identical behavior as well as for speed. `--client-timing` uses
//! the client's step duration (1000 / fixed_math::SIM_TICK_HZ, currently 22 ms)
//! instead of the headless default 66 ms; the chosen duration is printed.
//! The periodic hashes
//! are timed on their own, which measures what each hashing frame also takes.
//! The scenarios' own work (choosing each frame's commands) is not timed.
//! A frame that fails ends the run with its error, after the CSV, summary and
//! periodic hashes of the frames before it; `final_hash` is then left out.
//!
//! Frames hash their own state as normal play does, every
//! `sim::replay::PLAY_HASH_INTERVAL`th frame; `--frame-hash-every 1` hashes
//! every frame.
//!
//! `--map` also takes a loose map file; `python -m tools.scale_map` writes the
//! flat 500x500-cell map used for 20,000-unit, 30-house march runs, and with
//! `--starts 30` the match map with ore and trees. Add `--size 500` for a
//! literal `Size=0,0,500,500` map using the extended cell array.
//!
//! The simulation batches its path searches as a real match does;
//! `--serial-paths` searches in native order instead. Target scans run at
//! each object's turn; `--serial-scans` remains an accepted compatibility flag.
//!
//! Beside each frame's wall time, the CSV's `cpu_ms` column and the
//! `main_thread_cpu_*` line give the main thread's CPU time (Unix only). On a
//! machine other work keeps busy it moves far less than wall time, so it
//! compares two builds there; it leaves out the worker pool's share.
//!
//! `--frame-profile` also prints the simulation's own frame profile
//! (`sim::frame_profile`) over the measured frames: its four buckets, the
//! vision slice's share of the mean frame, and every phase's mean. The vision
//! slice is the frame's own refresh; objects move their sight at their own
//! cell arrivals, inside the objects bucket. It needs a build with
//! `--features telemetry`.
//!
//! `--lan` plays the frames as a LAN game: they take the multiplayer route,
//! which stores a multiplayer sync checksum every frame. `--frame-hashes
//! <path>` writes every frame's state hash and that checksum (`-` off the
//! multiplayer route), one `tick hash checksum` line per frame, untimed: two
//! hosts' files are equal exactly when their simulations stayed identical, so
//! comparing them checks cross-platform determinism frame by frame.
//!
//! Build with `cargo build --release -p vera20k-core --bin sim-bench`; timings from a debug
//! build are not meaningful.

mod sim_bench_match;

use std::io::Write;
use std::num::NonZeroU64;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use vera20k_core::assets::asset_manager::{AssetManager, MediaArchiveMode};
use vera20k_core::headless_scenario::{self, HeadlessScenario, SIM_TICK_MS, battle_session};
use vera20k_core::map::source::load_map_ini_by_name_or_path_with_assets;
use vera20k_core::map::waypoints::skirmish_player_capacity;
use vera20k_core::sim::command::{Command, CommandEnvelope};
use vera20k_core::sim::intern::InternedId;
use vera20k_core::sim::runtime::SimRuntime;
use vera20k_core::sim::scenario_session::ScenarioGameMode;

use sim_bench_match::MatchScenario;

/// The match scenario's progress line cadence, in ticks.
const PROGRESS_EVERY: u64 = 100;
/// The skirmish scenario's default run, ten minutes of game time.
const SKIRMISH_TICKS: u64 = 9000;
/// `--lan`'s MaxAhead: a LAN host's protocol-2 floor
/// (`net::lockstep::initial_multiplayer_max_ahead`).
const LAN_MAX_AHEAD: i32 = 10;

#[derive(Clone, Copy, PartialEq)]
enum ScenarioKind {
    March,
    Match,
    Skirmish,
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
    frame_profile: bool,
    lan: bool,
    frame_hashes: Option<PathBuf>,
    client_timing: bool,
}

impl Args {
    fn tick_ms(&self) -> u32 {
        if self.client_timing {
            1000 / vera20k_core::util::fixed_math::SIM_TICK_HZ
        } else {
            SIM_TICK_MS
        }
    }
}

fn parse_args() -> Result<Args, String> {
    parse_args_from(std::env::args().skip(1))
}

fn parse_args_from(mut argv: impl Iterator<Item = String>) -> Result<Args, String> {
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
        frame_profile: false,
        lan: false,
        frame_hashes: None,
        client_timing: false,
    };
    let mut march_flag = None;
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
                    "skirmish" => ScenarioKind::Skirmish,
                    other => {
                        return Err(format!(
                            "unknown scenario {other}: march, match or skirmish"
                        ));
                    }
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
            "--frame-profile" => args.frame_profile = true,
            "--lan" => args.lan = true,
            "--frame-hashes" => args.frame_hashes = Some(PathBuf::from(value()?)),
            "--client-timing" => args.client_timing = true,
            "--serial-scans" => {} // All target scans already run at their object turn.
            other => return Err(format!("unrecognised argument {other}")),
        }
    }
    if args.frame_profile && !cfg!(feature = "telemetry") {
        return Err("--frame-profile needs a build with --features telemetry".to_string());
    }
    if args.ra2_dir.as_os_str().is_empty() {
        return Err("--ra2-dir (or RA2_DIR) must name the retail install".to_string());
    }
    if args.houses.is_some_and(|houses| houses < 2) || args.types.is_empty() {
        return Err("need at least two houses and one unit type".to_string());
    }
    let marched = args.scenario == ScenarioKind::March;
    if marched && args.computers.is_some() {
        return Err("--computers is a match and skirmish option".to_string());
    }
    if !marched && let Some(flag) = march_flag {
        return Err(format!("{flag} is a march option"));
    }
    Ok(args)
}

/// `--computers`, by default the most the skirmish lobby admits on the map:
/// it refuses more players than the map's capacity (`MapCapacityExceeded`,
/// `TXT_SCENARIO_TOO_SMALL`), and a Battle has at most seven computer houses.
/// More would seat houses the map has no start for.
fn seated_computers(args: &Args) -> Result<usize, String> {
    let assets =
        AssetManager::new(&args.ra2_dir, MediaArchiveMode::STOCK_DIGITAL).map_err(|error| {
            format!(
                "open retail MIX archives in {}: {error}",
                args.ra2_dir.display()
            )
        })?;
    let map = load_map_ini_by_name_or_path_with_assets(&args.ra2_dir, &args.map, &assets)
        .map_err(|error| format!("read map {}: {error}", args.map))?;
    let most = usize::try_from(skirmish_player_capacity(&map.ini) - 1)
        .unwrap_or(0)
        .min(sim_bench_match::COMPUTERS);
    match args.computers {
        Some(computers) if computers > most => Err(format!(
            "{} admits at most {most} computer houses beside the local one",
            args.map
        )),
        computers => Ok(computers.unwrap_or(most)),
    }
}

/// Every map cell of the live terrain, in (rx, ry) order.
fn map_cells(runtime: &SimRuntime) -> Vec<(u16, u16)> {
    let mut cells: Vec<(u16, u16)> = runtime
        .simulation
        .resolved_terrain()
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
            // libm's trigonometry, so every platform rounds to the same homes.
            let angle = std::f64::consts::TAU * index as f64 / houses as f64;
            let u = u_mid + u_radius * libm::cos(angle);
            let v = v_mid + v_radius * libm::sin(angle);
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
    scenario: &mut HeadlessScenario,
    args: &Args,
    houses: usize,
) -> (usize, Vec<Vec<CommandEnvelope>>) {
    let owners: Vec<String> = (0..houses).map(|i| format!("Bench{i:02}")).collect();
    let owner_ids: Vec<InternedId> = owners
        .iter()
        .enumerate()
        .map(|(index, name)| {
            let countries = &sim_bench_match::HUMAN_COUNTRIES;
            let id = scenario.add_human_house(name, countries[index % countries.len()]);
            // No house owns a building, so exempt every house from the defeat
            // scan; objects still fight because only same-house pairs are allied.
            scenario.runtime.simulation.make_house_multiplay_passive(id);
            id
        })
        .collect();
    let grid = scenario.map.header.cell_grid();
    let runtime = &mut scenario.runtime;
    let homes = home_cells(&map_cells(runtime), houses);
    let spawned = spawn_armies(runtime, args, &owners, &homes);
    runtime
        .simulation
        .resolve_type_handles(&runtime.resources.rules);

    // Each army marches in formation to the opposite house's home, and with
    // --return-at later marches back to its own.
    let first_tick = runtime.simulation.session().tick() + 1;
    let mut orders: Vec<Vec<CommandEnvelope>> = Vec::new();
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
            let (x, y) = sim_bench_match::offset_cell(target_home, unit.offset, grid);
            orders[slot as usize].push(CommandEnvelope::new(
                owner_ids[unit.house],
                first_tick + slot,
                Command::AttackMove {
                    entity_id: unit.id,
                    target: None,
                    destination: Some(vera20k_core::sim::combat::TargetKind::Cell(x, y)),
                },
            ));
        }
    }
    (spawned.len(), orders)
}

enum Plan {
    March(Vec<Vec<CommandEnvelope>>),
    Match(MatchScenario),
    Skirmish,
}

/// Zero for no frames, as when the first frame failed.
fn percentile(sorted: &[Duration], fraction: f64) -> f64 {
    let index = ((sorted.len() as f64 - 1.0) * fraction).round() as usize;
    sorted
        .get(index)
        .map_or(0.0, |time| time.as_secs_f64() * 1000.0)
}

/// What the frame loop measured, up to the first frame that failed.
#[derive(Default)]
struct Measured {
    times: Vec<Duration>,
    cpu_times: Vec<Option<Duration>>,
    hashes: Vec<(u64, u64)>,
    hash_times: Vec<Duration>,
    /// With `--frame-hashes`: every frame's tick, state hash and multiplayer
    /// sync checksum.
    frame_records: Vec<(u64, u64, Option<u32>)>,
}

/// The benchmark's frames: each frame's commands and progress are untimed,
/// its advance is timed.
trait Frames {
    fn commands(&mut self, tick: u64) -> Vec<CommandEnvelope>;
    /// The frame's multiplayer sync checksum, on the multiplayer route.
    fn advance(&mut self, commands: &[CommandEnvelope]) -> Result<Option<u32>, String>;
    fn state_hash(&self) -> u64;
    /// After each completed frame, with every completed frame's time.
    fn completed(&mut self, tick: u64, times: &[Duration]);
}

struct Bench<'a> {
    runtime: &'a mut SimRuntime,
    plan: &'a mut Plan,
    tick_ms: u32,
}

impl Frames for Bench<'_> {
    fn commands(&mut self, tick: u64) -> Vec<CommandEnvelope> {
        match &mut *self.plan {
            Plan::March(orders) => orders.get(tick as usize).cloned().unwrap_or_default(),
            Plan::Match(script) => script.orders(self.runtime, tick),
            Plan::Skirmish => Vec::new(),
        }
    }

    fn advance(&mut self, commands: &[CommandEnvelope]) -> Result<Option<u32>, String> {
        self.runtime
            .advance_frame_for_tooling(commands, self.tick_ms)
            .map_err(|error| error.to_string())
    }

    fn state_hash(&self) -> u64 {
        self.runtime.simulation.state_hash()
    }

    fn completed(&mut self, tick: u64, times: &[Duration]) {
        if matches!(self.plan, Plan::March(_)) || (tick + 1) % PROGRESS_EVERY != 0 {
            return;
        }
        let recent = &times[times.len().saturating_sub(PROGRESS_EVERY as usize)..];
        let mean: Duration = recent.iter().sum::<Duration>() / recent.len() as u32;
        let progress = match &*self.plan {
            Plan::Match(script) => script.progress(self.runtime),
            _ => format!("live_objects={}", self.runtime.simulation.entities().len()),
        };
        println!(
            "progress tick={} mean_ms={:.1} {progress}",
            tick + 1,
            mean.as_secs_f64() * 1000.0,
        );
    }
}

/// Run `ticks` frames, stopping at the first that fails. The frames before it
/// keep their times and hashes, so the report still covers them. With
/// `every_frame`, each frame's record is kept as well.
fn run_frames(
    frames: &mut impl Frames,
    ticks: u64,
    hash_every: u64,
    every_frame: bool,
) -> (Measured, Result<(), String>) {
    let mut measured = Measured {
        times: Vec::with_capacity(ticks as usize),
        cpu_times: Vec::with_capacity(ticks as usize),
        ..Measured::default()
    };
    for tick in 0..ticks {
        let commands = frames.commands(tick);
        let cpu_started = thread_cpu_time();
        let started = Instant::now();
        let checksum = match frames.advance(&commands) {
            Ok(checksum) => checksum,
            Err(error) => return (measured, Err(format!("tick {tick}: {error}"))),
        };
        measured.times.push(started.elapsed());
        measured.cpu_times.push(
            cpu_started
                .zip(thread_cpu_time())
                .map(|(started, ended)| ended.saturating_sub(started)),
        );
        if hash_every > 0 && (tick + 1) % hash_every == 0 {
            let started = Instant::now();
            let hash = frames.state_hash();
            measured.hash_times.push(started.elapsed());
            measured.hashes.push((tick + 1, hash));
        }
        if every_frame {
            let hash = frames.state_hash();
            measured.frame_records.push((tick + 1, hash, checksum));
        }
        frames.completed(tick, &measured.times);
    }
    (measured, Ok(()))
}

/// The `--frame-hashes` file: one `tick hash checksum` line per completed
/// frame, `-` for a frame off the multiplayer route.
fn frame_hashes(measured: &Measured) -> String {
    let mut text = String::new();
    for (tick, hash, checksum) in &measured.frame_records {
        let checksum = checksum.map_or("-".to_string(), |checksum| format!("{checksum:08x}"));
        text.push_str(&format!("{tick} {hash:016x} {checksum}\n"));
    }
    text
}

/// The `--csv` file: one row per completed frame.
fn csv(measured: &Measured) -> String {
    let mut text = String::from("tick,ms,cpu_ms\n");
    for (tick, (time, cpu)) in measured.times.iter().zip(&measured.cpu_times).enumerate() {
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
    text
}

/// The timing summary and periodic hashes of the completed frames.
fn report(out: &mut impl Write, measured: &Measured, warmup: u64) -> std::io::Result<()> {
    let times = &measured.times;
    let total: Duration = times.iter().sum();
    let steady: Vec<Duration> = times.iter().skip(warmup as usize).copied().collect();
    let mut sorted = times.clone();
    sorted.sort_unstable();
    writeln!(
        out,
        "ticks={} total_s={:.2} mean_ms={:.2} p50_ms={:.2} p95_ms={:.2} max_ms={:.2}",
        times.len(),
        total.as_secs_f64(),
        total.as_secs_f64() * 1000.0 / times.len().max(1) as f64,
        percentile(&sorted, 0.5),
        percentile(&sorted, 0.95),
        percentile(&sorted, 1.0),
    )?;
    let mut cpu_sorted: Vec<Duration> = measured.cpu_times.iter().flatten().copied().collect();
    if cpu_sorted.len() == times.len() && !cpu_sorted.is_empty() {
        let cpu_total: Duration = cpu_sorted.iter().sum();
        cpu_sorted.sort_unstable();
        writeln!(
            out,
            "main_thread_cpu_mean_ms={:.2} p50_ms={:.2} p95_ms={:.2} max_ms={:.2}",
            cpu_total.as_secs_f64() * 1000.0 / cpu_sorted.len() as f64,
            percentile(&cpu_sorted, 0.5),
            percentile(&cpu_sorted, 0.95),
            percentile(&cpu_sorted, 1.0),
        )?;
    }
    if !steady.is_empty() {
        let steady_total: Duration = steady.iter().sum();
        writeln!(
            out,
            "after_orders_mean_ms={:.2} (ticks {}..={})",
            steady_total.as_secs_f64() * 1000.0 / steady.len() as f64,
            warmup + 1,
            times.len()
        )?;
    }
    for (tick, hash) in &measured.hashes {
        writeln!(out, "hash tick={tick} {hash:016x}")?;
    }
    if !measured.hash_times.is_empty() {
        let total: Duration = measured.hash_times.iter().sum();
        let mut hash_times = measured.hash_times.clone();
        hash_times.sort_unstable();
        writeln!(
            out,
            "periodic_hash_mean_ms={:.3} p50_ms={:.3}",
            total.as_secs_f64() * 1000.0 / hash_times.len() as f64,
            percentile(&hash_times, 0.5)
        )?;
    }
    Ok(())
}

fn main() -> Result<(), String> {
    let args = match parse_args() {
        Ok(args) => args,
        Err(error) => {
            eprintln!("{error}");
            eprintln!(
                "usage: sim-bench [--ra2-dir <retail path>] [--map Death.mmx] \
                 [--scenario march|match|skirmish] [--seed S] [--units 20000] [--houses N] \
                 [--computers N (match, skirmish; default the map's most, at most 7)] \
                 [--ticks 300 (match: 1800, skirmish: 9000)] \
                 [--types E1,E2,MTNK,HTNK] [--order-spread 30] [--return-at T] (march) \
                 [--hash-every N] [--frame-hash-every 15] [--csv <path>] [--serial-paths] [--serial-scans] \
                 [--frame-profile] [--lan] [--frame-hashes <path>] [--client-timing]"
            );
            std::process::exit(2);
        }
    };
    let computers = match args.scenario {
        ScenarioKind::March => 0,
        ScenarioKind::Match | ScenarioKind::Skirmish => seated_computers(&args)?,
    };
    let load_started = Instant::now();
    let mut scenario = match args.scenario {
        ScenarioKind::March => headless_scenario::load(&args.ra2_dir, &args.map, args.seed)?,
        ScenarioKind::Match => {
            let computers = computers.min(args.houses.unwrap_or(usize::MAX));
            headless_scenario::load_session(
                &args.ra2_dir,
                &args.map,
                args.seed,
                sim_bench_match::session(&args.map, computers),
            )?
        }
        ScenarioKind::Skirmish => headless_scenario::load_session(
            &args.ra2_dir,
            &args.map,
            args.seed,
            battle_session(&args.map, &sim_bench_match::COMPUTER_ROSTER[..computers]),
        )?,
    };
    if !args.serial_paths {
        scenario.runtime.simulation.batch_path_searches();
    }
    scenario
        .runtime
        .simulation
        .hash_state_every(args.frame_hash_every);

    let (houses, spawned, mut plan, ticks) = match args.scenario {
        ScenarioKind::March => {
            let houses = args.houses.unwrap_or(8);
            let (spawned, orders) = march(&mut scenario, &args, houses);
            let ticks = args.ticks.map_or(300, NonZeroU64::get);
            (houses, spawned, Plan::March(orders), ticks)
        }
        ScenarioKind::Match => {
            let script = MatchScenario::setup(&mut scenario, args.houses, args.units)?;
            let ticks = args.ticks.map_or(sim_bench_match::TICKS, NonZeroU64::get);
            (script.houses(), script.spawned, Plan::Match(script), ticks)
        }
        ScenarioKind::Skirmish => {
            let simulation = &mut scenario.runtime.simulation;
            if let Some(local) = simulation.session().current_house() {
                simulation.make_house_multiplay_passive(local);
            }
            let ticks = args.ticks.map_or(SKIRMISH_TICKS, NonZeroU64::get);
            (simulation.houses().order().len(), 0, Plan::Skirmish, ticks)
        }
    };
    if args.lan {
        // A LAN GameMode selects `Queue_AI_Multiplayer @ 0x006475F0`, whose
        // frames store the sync checksum; the admitted session has timing.
        let simulation = &mut scenario.runtime.simulation;
        simulation.install_session_mode(ScenarioGameMode::Lan);
        simulation.install_network_timing(LAN_MAX_AHEAD);
    }
    // The steady window leaves out the march's spread first orders and the
    // match's launch and first contacts.
    let warmup = match plan {
        Plan::March(_) => args.order_spread,
        Plan::Match(_) => sim_bench_match::WARMUP,
        Plan::Skirmish => 0,
    };
    let computers = match &plan {
        Plan::March(_) => 0,
        Plan::Match(script) => script.computers(),
        Plan::Skirmish => computers,
    };
    let tick_ms = args.tick_ms();
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
    println!("simulation_tick_ms={tick_ms}");
    if let Plan::Match(script) = &plan {
        println!("progress tick=0 {}", script.progress(runtime));
    }

    #[cfg(feature = "telemetry")]
    runtime.simulation.set_frame_profiling(args.frame_profile);
    // A failed frame still reports the frames before it, then fails the run.
    let (measured, frames) = run_frames(
        &mut Bench {
            runtime: &mut *runtime,
            plan: &mut plan,
            tick_ms,
        },
        ticks,
        args.hash_every,
        args.frame_hashes.is_some(),
    );
    let live_at_end = runtime.simulation.entities().len();
    let csv_written = args.csv.as_ref().map_or(Ok(()), |path| {
        std::fs::write(path, csv(&measured)).map_err(|error| format!("{}: {error}", path.display()))
    });
    let frame_hashes_written = args.frame_hashes.as_ref().map_or(Ok(()), |path| {
        std::fs::write(path, frame_hashes(&measured))
            .map_err(|error| format!("{}: {error}", path.display()))
    });
    report(&mut std::io::stdout().lock(), &measured, warmup).map_err(|error| error.to_string())?;
    #[cfg(feature = "telemetry")]
    if args.frame_profile {
        print_frame_profile(runtime);
    }
    println!("live_objects start={live_at_start} end={live_at_end}");
    frames?;
    csv_written?;
    frame_hashes_written?;
    let final_hash = runtime.simulation.state_hash();
    println!("final_hash={final_hash:016x}");
    Ok(())
}

/// The simulation's frame profile over the measured frames: the buckets, the
/// vision slice's share of the mean frame, then every phase's mean.
#[cfg(feature = "telemetry")]
fn print_frame_profile(runtime: &SimRuntime) {
    use vera20k_core::sim::frame_profile::PHASE_KEYS;
    let Some(profile) = runtime.simulation.frame_profile().report() else {
        return;
    };
    println!(
        "frame_profile frames={} mean_ms={:.2} peak_ms={:.2} logic_ms={:.2} objects_ms={:.2} \
         vision_ms={:.3} vision_share={:.1}% tail_ms={:.2} unmarked_ms={:.3}",
        profile.frames,
        profile.mean_frame_ms,
        profile.peak_frame_ms,
        profile.logic_ms,
        profile.objects_ms,
        profile.vision_ms,
        100.0 * profile.vision_ms / profile.mean_frame_ms.max(f64::MIN_POSITIVE),
        profile.tail_ms,
        profile.unmarked_ms,
    );
    let phases: Vec<String> = PHASE_KEYS
        .iter()
        .zip(profile.phases_ms)
        .map(|(key, ms)| format!("{key}={ms:.3}"))
        .collect();
    println!("frame_profile_phases_ms {}", phases.join(" "));
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

#[cfg(test)]
mod partial_report_tests {
    use super::*;

    /// Frames that fail at `fail_at`; the state hash counts completed frames.
    struct Scripted {
        fail_at: u64,
        done: u64,
    }

    impl Frames for Scripted {
        fn commands(&mut self, _tick: u64) -> Vec<CommandEnvelope> {
            Vec::new()
        }

        fn advance(&mut self, _commands: &[CommandEnvelope]) -> Result<Option<u32>, String> {
            if self.done == self.fail_at {
                return Err("boom".to_string());
            }
            self.done += 1;
            Ok((self.done % 2 == 0).then_some(self.done as u32))
        }

        fn state_hash(&self) -> u64 {
            self.done
        }

        fn completed(&mut self, _tick: u64, _times: &[Duration]) {}
    }

    fn run(fail_at: u64) -> (Measured, Result<(), String>, String) {
        let mut frames = Scripted { fail_at, done: 0 };
        let (measured, result) = run_frames(&mut frames, 5, 1, true);
        let mut out = Vec::new();
        report(&mut out, &measured, 1).unwrap();
        (measured, result, String::from_utf8(out).unwrap())
    }

    #[test]
    fn failed_frame_keeps_the_frames_before_it() {
        let (measured, result, out) = run(2);
        assert_eq!(result, Err("tick 2: boom".to_string()));
        assert_eq!(measured.hashes, [(1, 1), (2, 2)]);
        assert_eq!(csv(&measured).lines().count(), 3);
        assert_eq!(
            frame_hashes(&measured),
            "1 0000000000000001 -\n2 0000000000000002 00000002\n"
        );
        assert!(out.starts_with("ticks=2 "), "{out}");
        assert!(out.contains("hash tick=2 0000000000000002\n"), "{out}");
    }

    #[test]
    fn first_frame_failure_reports_no_frames() {
        let (measured, result, out) = run(0);
        assert_eq!(result, Err("tick 0: boom".to_string()));
        assert_eq!(csv(&measured), "tick,ms,cpu_ms\n");
        assert!(
            out.starts_with("ticks=0 total_s=0.00 mean_ms=0.00 p50_ms=0.00 "),
            "{out}"
        );
    }
}
