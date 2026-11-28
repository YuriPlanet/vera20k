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
//! 119 offline), the other start waypoints get human players' houses, and every house
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
//! `--scenario battle` is the scale benchmark's battle (docs/scale-benchmark.md):
//! the map itself makes the armies. It loads as a skirmish does, and the
//! launch's local house plays too, so every start fights. `tools.scale_map
//! --battle` writes its map, whose triggers give every start's house an army
//! in squads, send the squads to patrol the fronts between neighbouring
//! starts and replace a squad when its marker dies (`tools/scale_battle.py`).
//! No command is issued.
//! `--ticks` defaults to [`BATTLE_TICKS`], of which the first
//! [`BATTLE_WARMUP`] are left out of the measured window. Every
//! `PROGRESS_EVERY` ticks it prints how many mobile objects live, how many
//! were lost and made since the last line and how many objects of any kind
//! (shots and anims too) the frames made; the run fails when a window of the
//! measured part loses nothing or the battle falls under `--battle-units`
//! mobile objects ([`BATTLE_UNITS`] by default), so a timing never comes from
//! a battle that stopped. A smaller map of the same kind takes a smaller
//! floor.
//!
//! `--watchers N` (skirmish only) plays the match VERA's room starts when
//! everyone in it watches while only AI play: the launch seats N watching
//! humans ahead of the computer houses and loads the map as a LAN game, so
//! the watchers' Houses are observers. The run reports each House's defeat
//! as it happens and stops once the match is decided.
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
//!
//! `--state-isolation-report <new.json>` compares process lifetimes through
//! the production headless loader. Run once in a fresh process, then again
//! with `--prior-map <map>` (repeatable), and compare the reports' `target`
//! values. Use the same map, seed, `--isolation-mode lan|wol`,
//! `--isolation-humans 1|2`, computers and tick count. This exercises Rules,
//! world construction and ordinary frames; app input and transport need the
//! live LAN capture described in docs/Networking.md.
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
//! The standard battle benchmark is 1v29: a skirmish against 29 computer
//! houses on the 30-start match map, timed at the client's step:
//! `python -m tools.scale_map target/scale/match30.map --starts 30`, then
//! `sim-bench --map target/scale/match30.map --scenario skirmish
//! --client-timing --frame-profile`. The map's `MaxPlayers=30` lets the
//! lobby seat them all.
//!
//! `--presentation-frame` brings the copy of the world without its objects
//! up to date after every frame, as the client's simulation host does for
//! drawing (docs/sim-thread.md), and counts it in the frame's time. The
//! `presentation_copy_mean_ms` line reports the copy's own mean over the
//! measured window, so two builds' copies compare in one run.
//!
//! Idle objects sleep as in a real match (`sim::world::idle_sleep`), and
//! steady objects' AI visits are counted (`sim::world::steady_ai`):
//! `--no-sleep` runs every turn and visit, which must give the same hashes,
//! and `--sleep-check` runs every turn and visit either would skip and stops
//! if one changed anything. The summary prints how many were counted.
//!
//! The simulation batches its path searches and target scans as a real
//! match does, and with the scans spreads the patrol scans over their period
//! and updates each team on its own turns; `--serial-paths` searches in
//! native order instead, and `--serial-scans` scans in native order, has
//! every patrolling team scan on the same frame and updates every team every
//! frame. Each object's turn draws from its own random stream as in a real
//! match (`sim::world::turn_streams`); `--shared-stream` draws every turn
//! from the Scenario stream in Logic order, as gamemd does.
//! The `path_batch` line sums the batch's passes over the
//! measured frames ([`PathBatchStats`]): registrations waiting and answered,
//! the ones the cap, a re-path budget or the urgency-2 limit left for a later
//! pass (`capped` counts the limit's), and the cells the searches expanded,
//! with the most one pass and one search expanded (`peak_steps`,
//! `peak_search`) and the code-2 re-paths' share at each urgency.
//!
//! `--threads N` gives the worker pool N threads instead of one per CPU
//! thread; no result depends on it. The `measured` line gives the mean, p50,
//! p95, p99 and largest frame of the window after the warm-up, and the
//! process's CPU time per frame there over every thread (Unix only), whose
//! ratio to the wall time is how many cores the frame kept busy.
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

mod sim_bench_isolation;
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
use vera20k_core::sim::movement::PathBatchStats;
use vera20k_core::sim::runtime::SimRuntime;
use vera20k_core::sim::scenario_bootstrap::MatchLaunchDescriptor;
use vera20k_core::sim::scenario_session::ScenarioGameMode;
use vera20k_core::skirmish_launch::{NETWORK_PLAYER_SLOT_COUNT, SkirmishLaunchSession};

use sim_bench_match::MatchScenario;

/// The match scenario's progress line cadence, in ticks.
const PROGRESS_EVERY: u64 = 100;
/// The skirmish scenario's default run, ten minutes of game time.
const SKIRMISH_TICKS: u64 = 9000;
/// The battle scenario's default run: the warm-up, then a minute of game time
/// at the client's step.
const BATTLE_TICKS: u64 = BATTLE_WARMUP + 2700;
/// The battle's first contacts, before every front fights.
const BATTLE_WARMUP: u64 = 300;
/// The fewest mobile objects the benchmark's battle may hold in its measured
/// window.
const BATTLE_UNITS: usize = 20_000;
/// `--lan`'s MaxAhead: a LAN host's protocol-2 floor
/// (`net::lockstep::initial_multiplayer_max_ahead`).
const LAN_MAX_AHEAD: i32 = 10;

#[derive(Clone, Copy, PartialEq)]
enum ScenarioKind {
    March,
    Match,
    Skirmish,
    Battle,
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
    shared_stream: bool,
    no_sleep: bool,
    sleep_check: bool,
    frame_profile: bool,
    lan: bool,
    watchers: usize,
    frame_hashes: Option<PathBuf>,
    client_timing: bool,
    presentation_frame: bool,
    threads: Option<usize>,
    battle_units: usize,
    skirmish_replay: Option<PathBuf>,
    isolation_report: Option<PathBuf>,
    prior_maps: Vec<String>,
    isolation_mode: Option<ScenarioGameMode>,
    isolation_humans: usize,
}

impl Args {
    fn tick_ms(&self) -> u32 {
        // The battle is the client's match, at the client's step.
        if self.client_timing || self.scenario == ScenarioKind::Battle {
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
        serial_scans: false,
        shared_stream: false,
        no_sleep: false,
        sleep_check: false,
        frame_profile: false,
        lan: false,
        watchers: 0,
        frame_hashes: None,
        client_timing: false,
        presentation_frame: false,
        threads: None,
        battle_units: BATTLE_UNITS,
        skirmish_replay: None,
        isolation_report: None,
        prior_maps: Vec::new(),
        isolation_mode: None,
        isolation_humans: 1,
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
                    "battle" => ScenarioKind::Battle,
                    other => {
                        return Err(format!(
                            "unknown scenario {other}: march, match, skirmish or battle"
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
            "--watchers" => args.watchers = number(value()?)? as usize,
            "--record-skirmish-replay" => args.skirmish_replay = Some(PathBuf::from(value()?)),
            "--frame-hashes" => args.frame_hashes = Some(PathBuf::from(value()?)),
            "--client-timing" => args.client_timing = true,
            "--serial-scans" => args.serial_scans = true,
            "--shared-stream" => args.shared_stream = true,
            "--no-sleep" => args.no_sleep = true,
            "--sleep-check" => args.sleep_check = true,
            "--presentation-frame" => args.presentation_frame = true,
            "--battle-units" => args.battle_units = number(value()?)? as usize,
            "--threads" => {
                args.threads = Some(
                    usize::try_from(number(value()?)?)
                        .ok()
                        .filter(|&threads| threads > 0)
                        .ok_or("--threads needs a positive number")?,
                )
            }
            "--state-isolation-report" => args.isolation_report = Some(PathBuf::from(value()?)),
            "--prior-map" => args.prior_maps.push(value()?),
            "--isolation-mode" => {
                args.isolation_mode = Some(match value()?.as_str() {
                    "lan" => ScenarioGameMode::Lan,
                    "wol" => ScenarioGameMode::Wol,
                    _ => return Err("--isolation-mode needs lan or wol".into()),
                });
            }
            "--isolation-humans" => args.isolation_humans = number(value()?)? as usize,
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
    if args.watchers > 0 {
        if args.scenario != ScenarioKind::Skirmish {
            return Err("--watchers is a skirmish option".to_string());
        }
        // Observers exist only in a network game.
        args.lan = true;
    }
    let marched = args.scenario == ScenarioKind::March;
    if marched && args.computers.is_some() && args.isolation_report.is_none() {
        return Err("--computers is a match, skirmish and battle option".to_string());
    }
    if args.scenario == ScenarioKind::Battle && args.houses.is_some() {
        return Err("the battle seats every start: --computers sets how many".to_string());
    }
    if !marched && let Some(flag) = march_flag {
        return Err(format!("{flag} is a march option"));
    }
    if args.isolation_report.is_none()
        && (!args.prior_maps.is_empty()
            || args.isolation_mode.is_some()
            || args.isolation_humans != 1)
    {
        return Err("state-isolation options need --state-isolation-report".into());
    }
    if args.skirmish_replay.is_some()
        && (args.scenario != ScenarioKind::Skirmish
            || args.isolation_report.is_some()
            || args.lan
            || args.ticks.is_none())
    {
        return Err(
            "--record-skirmish-replay needs --scenario skirmish and --ticks, offline".into(),
        );
    }
    Ok(args)
}

/// `--computers`, by default the most the offline Skirmish shell admits on the
/// map: it refuses more players than the map's capacity (`MapCapacityExceeded`,
/// `TXT_SCENARIO_TOO_SMALL`). More would seat houses the map has no start
/// for. A map's `[Basic] MaxPlayers` (9 to 120) raises it past the eight
/// retail starts, up to 119 computer houses beside the local one.
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
    let cap = if args.lan {
        NETWORK_PLAYER_SLOT_COUNT.saturating_sub(args.watchers.max(1))
    } else {
        sim_bench_match::COMPUTERS
    };
    let most = usize::try_from(skirmish_player_capacity(&map.ini) - 1)
        .unwrap_or(0)
        .min(cap);
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
            if let Some(id) = simulation.spawn_object_with_map_overlays(
                type_id,
                owner,
                rx,
                ry,
                0,
                &resources.rules,
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
    Battle(Battle),
}

/// The battle scenario's watch over its own workload: the mobile objects
/// alive at the last progress line, and what the windows since the warm-up
/// held.
#[derive(Default)]
struct Battle {
    /// The mobile objects' ids at the last progress line, ascending.
    mobile: Vec<u64>,
    /// `Simulation::next_stable_object_id` at the last progress line.
    next_id: u64,
    /// The fewest mobile objects a measured window ended with.
    fewest: Option<usize>,
    /// The measured windows' losses and objects made.
    lost: usize,
    made: u64,
    /// A measured window that lost no mobile object.
    quiet_window: Option<u64>,
    /// `--battle-units`: the fewest mobile objects a measured window may end
    /// with.
    floor: usize,
}

/// The ids of the mobile objects on the map, ascending.
fn mobile_ids(runtime: &SimRuntime) -> Vec<u64> {
    use vera20k_core::map::entities::EntityCategory;
    runtime
        .simulation
        .entities()
        .values()
        .filter(|entity| {
            entity.category() != EntityCategory::Structure
                && entity.is_alive()
                && !entity.in_limbo()
        })
        .map(|entity| entity.stable_id())
        .collect()
}

impl Battle {
    fn start(runtime: &SimRuntime, floor: usize) -> Self {
        Self {
            mobile: mobile_ids(runtime),
            next_id: runtime.simulation.next_stable_object_id(),
            floor,
            ..Self::default()
        }
    }

    /// One progress line's worth: what changed since the last one. `tick`
    /// counts the completed frames.
    fn progress(&mut self, runtime: &SimRuntime, tick: u64) -> String {
        let mobile = mobile_ids(runtime);
        // Both lists ascend, as the store does.
        let lost = self
            .mobile
            .iter()
            .filter(|id| mobile.binary_search(id).is_err())
            .count();
        let new = mobile
            .iter()
            .filter(|id| self.mobile.binary_search(id).is_err())
            .count();
        let next_id = runtime.simulation.next_stable_object_id();
        let made = next_id - self.next_id;
        if tick > BATTLE_WARMUP {
            self.fewest = Some(
                self.fewest
                    .map_or(mobile.len(), |few| few.min(mobile.len())),
            );
            self.lost += lost;
            self.made += made;
            if lost == 0 {
                self.quiet_window.get_or_insert(tick);
            }
        }
        let line = format!(
            "mobile={} lost={lost} new={new} objects_made={made} shots_live={} live_objects={}",
            mobile.len(),
            runtime.simulation.projectiles().len(),
            runtime.simulation.entities().len(),
        );
        self.mobile = mobile;
        self.next_id = next_id;
        line
    }

    /// Whether the measured windows were the battle the benchmark names.
    fn verdict(&self) -> Result<(), String> {
        if let Some(tick) = self.quiet_window {
            return Err(format!(
                "the battle stopped: no mobile object was lost in the {PROGRESS_EVERY} ticks before tick {tick}"
            ));
        }
        match self.fewest {
            Some(fewest) if fewest < self.floor => Err(format!(
                "the battle shrank to {fewest} mobile objects, under {}",
                self.floor
            )),
            _ => Ok(()),
        }
    }
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
    /// The process's CPU time inside the frames after the warm-up, over
    /// every thread.
    window_cpu: Option<Duration>,
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
    /// Whether the run ends before its last tick.
    fn finished(&self) -> bool {
        false
    }
}

struct Bench<'a> {
    runtime: &'a mut SimRuntime,
    plan: &'a mut Plan,
    tick_ms: u32,
    /// `--presentation-frame`: whether each frame keeps a copy of the world.
    keep_frame: bool,
    frame: Option<SimRuntime>,
    /// Time spent inside `--presentation-frame`'s copy, one per frame.
    copy_times: Vec<Duration>,
    paths: PathTotals,
    /// `--watchers`: the match's defeats and decision, reported as they happen.
    watch: Option<Watch>,
}

/// The Houses already reported defeated, and whether the match is decided.
#[derive(Default)]
struct Watch {
    defeated: Vec<InternedId>,
    decided: bool,
}

impl Watch {
    fn report(&mut self, runtime: &SimRuntime, tick: u64) {
        let sim = &runtime.simulation;
        for &owner in sim.houses().order() {
            let Some(house) = sim.houses().get(&owner) else {
                continue;
            };
            if house.is_defeated() && !self.defeated.contains(&owner) {
                self.defeated.push(owner);
                let kind = if house.is_human() {
                    "watcher"
                } else {
                    "computer"
                };
                println!("tick={tick} defeated {} ({kind})", sim.resolve(owner));
            }
        }
        if !self.decided && sim.network_match_winners().is_some() {
            self.decided = true;
            let left: Vec<_> = sim
                .houses()
                .order()
                .iter()
                .filter(|owner| {
                    sim.houses()
                        .get(owner)
                        .is_some_and(|house| !house.is_defeated() && !house.multiplay_passive())
                })
                .map(|owner| sim.resolve(*owner))
                .collect();
            println!("tick={tick} match decided, left: {}", left.join(", "));
        }
    }
}

/// `--watchers`' launch: `watchers` observing humans in the first rows, then
/// the computer houses, as VERA's room seats a match where only AI play.
fn watched_session(
    map: &str,
    computers: usize,
    watchers: usize,
) -> Result<SkirmishLaunchSession, String> {
    if watchers + computers > NETWORK_PLAYER_SLOT_COUNT || computers < 2 {
        return Err(format!(
            "--watchers needs two computer houses or more and at most {NETWORK_PLAYER_SLOT_COUNT} rows in all"
        ));
    }
    let mut session = battle_session(map, &sim_bench_match::COMPUTER_ROSTER[..computers]);
    let template = session.humans[0].clone();
    session.humans = (0..watchers)
        .map(|row| {
            let mut human = template.clone();
            human.name = format!("Watcher{row}");
            human.raw_slot_index = Some(row as u8);
            human.set_observer(true);
            human.slot.color_index = 8;
            human
        })
        .collect();
    for computer in &mut session.opponents {
        computer.raw_slot_index += watchers as u8 - 1;
    }
    Ok(session)
}

/// The path batch's passes summed over the completed frames; no passes while
/// the simulation searches serially.
#[derive(Clone, Copy, Default)]
struct PathTotals {
    passes: u64,
    waiting: u64,
    answered: u64,
    left_out: u64,
    /// Passes that left any registration for a later one.
    left_out_passes: u64,
    peak_left_out: u32,
    /// Urgency-2 re-paths stopped at their limit.
    capped: u64,
    steps: u64,
    /// The most cells one pass's searches expanded, and one search.
    peak_steps: u64,
    peak_search: u64,
    urgency2_steps: u64,
    urgency1_steps: u64,
    peak_pass_ms: f64,
}

impl PathTotals {
    fn add(&mut self, pass: PathBatchStats) {
        self.passes += 1;
        self.waiting += u64::from(pass.waiting);
        self.answered += u64::from(pass.answered);
        self.left_out += u64::from(pass.left_out);
        self.left_out_passes += u64::from(pass.left_out > 0);
        self.peak_left_out = self.peak_left_out.max(pass.left_out);
        self.capped += u64::from(pass.capped);
        self.steps += pass.steps;
        self.peak_steps = self.peak_steps.max(pass.steps);
        self.peak_search = self.peak_search.max(pass.largest_search);
        self.urgency2_steps += pass.urgency2_steps;
        self.urgency1_steps += pass.urgency1_steps;
        self.peak_pass_ms = self.peak_pass_ms.max(pass.last_ms);
    }
}

impl Frames for Bench<'_> {
    fn commands(&mut self, tick: u64) -> Vec<CommandEnvelope> {
        match &mut *self.plan {
            Plan::March(orders) => orders.get(tick as usize).cloned().unwrap_or_default(),
            Plan::Match(script) => script.orders(self.runtime, tick),
            Plan::Skirmish | Plan::Battle(_) => Vec::new(),
        }
    }

    fn advance(&mut self, commands: &[CommandEnvelope]) -> Result<Option<u32>, String> {
        let checksum = self
            .runtime
            .advance_frame_for_tooling(commands, self.tick_ms)
            .map_err(|error| error.to_string())?;
        if self.keep_frame {
            // As the client's simulation host does when it lends a tick: the
            // world without its objects, with the local player's fog, replaces
            // the previous tick's copy, inside the frame's time. The objects'
            // draw records are the client's and are not counted here.
            let started = Instant::now();
            let viewer = self.runtime.simulation.session().current_house();
            let reuse = self.frame.take().map(|frame| frame.simulation);
            self.frame = Some(self.runtime.presentation_copy(viewer, reuse));
            self.copy_times.push(started.elapsed());
        }
        Ok(checksum)
    }

    fn state_hash(&self) -> u64 {
        self.runtime.simulation.state_hash()
    }

    fn finished(&self) -> bool {
        self.watch.as_ref().is_some_and(|watch| watch.decided)
    }

    fn completed(&mut self, tick: u64, times: &[Duration]) {
        if let Some(pass) = self.runtime.simulation.path_batch_stats() {
            self.paths.add(pass);
        }
        if let Some(watch) = &mut self.watch {
            watch.report(self.runtime, tick + 1);
        }
        if matches!(self.plan, Plan::March(_)) || (tick + 1) % PROGRESS_EVERY != 0 {
            return;
        }
        let recent = &times[times.len().saturating_sub(PROGRESS_EVERY as usize)..];
        let mean: Duration = recent.iter().sum::<Duration>() / recent.len() as u32;
        let progress = match &mut *self.plan {
            Plan::Match(script) => script.progress(self.runtime),
            Plan::Battle(battle) => battle.progress(self.runtime, tick + 1),
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
    warmup: u64,
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
        let process_started = (tick >= warmup).then(process_cpu_time).flatten();
        let cpu_started = thread_cpu_time();
        let started = Instant::now();
        let checksum = match frames.advance(&commands) {
            Ok(checksum) => checksum,
            Err(error) => return (measured, Err(format!("tick {tick}: {error}"))),
        };
        measured.times.push(started.elapsed());
        if let Some((started, ended)) = process_started.zip(process_cpu_time()) {
            *measured.window_cpu.get_or_insert_default() += ended.saturating_sub(started);
        }
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
        if frames.finished() {
            break;
        }
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
        let steady_mean = steady_total.as_secs_f64() * 1000.0 / steady.len() as f64;
        writeln!(
            out,
            "after_orders_mean_ms={steady_mean:.2} (ticks {}..={})",
            warmup + 1,
            times.len()
        )?;
        let mut sorted = steady.clone();
        sorted.sort_unstable();
        write!(
            out,
            "measured ticks={} mean_ms={steady_mean:.2} p50_ms={:.2} p95_ms={:.2} p99_ms={:.2} max_ms={:.2}",
            steady.len(),
            percentile(&sorted, 0.5),
            percentile(&sorted, 0.95),
            percentile(&sorted, 0.99),
            percentile(&sorted, 1.0),
        )?;
        if let Some(cpu) = measured.window_cpu {
            let cpu_ms = cpu.as_secs_f64() * 1000.0 / steady.len() as f64;
            write!(
                out,
                " process_cpu_ms={cpu_ms:.2} cores={:.2}",
                cpu_ms / steady_mean.max(f64::MIN_POSITIVE)
            )?;
        }
        writeln!(out)?;
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
                 [--scenario march|match|skirmish|battle] [--seed S] [--units 20000] [--houses N] \
                 [--computers N (match, skirmish, battle; default the map's most, at most 119)] \
                 [--ticks 300 (match: 1800, skirmish: 9000, battle: 3000)] \
                 [--types E1,E2,MTNK,HTNK] [--order-spread 30] [--return-at T] (march) \
                 [--hash-every N] [--frame-hash-every 15] [--csv <path>] [--serial-paths] [--serial-scans] \
                 [--shared-stream] [--frame-profile] [--lan] [--watchers N (skirmish)] [--frame-hashes <path>] \
                 [--client-timing] [--threads N] [--battle-units 20000 (battle)] \
                 [--record-skirmish-replay <new.json> (skirmish, --ticks)] \
                 [--state-isolation-report <new.json> --prior-map <map> \
                 --isolation-mode lan|wol --isolation-humans 1|2]"
            );
            std::process::exit(2);
        }
    };
    match args.threads {
        Some(threads) => use_worker_threads(threads)?,
        #[cfg(not(target_arch = "wasm32"))]
        None => vera20k_core::sim::parallel::start_worker_pool(),
        #[cfg(target_arch = "wasm32")]
        None => {}
    }
    if args.isolation_report.is_some() {
        return sim_bench_isolation::run(&args);
    }
    let computers = match args.scenario {
        ScenarioKind::March => 0,
        ScenarioKind::Match | ScenarioKind::Skirmish | ScenarioKind::Battle => {
            seated_computers(&args)?
        }
    };
    if let Some(output) = &args.skirmish_replay {
        // The verifier reconstructs the ordinary frame-zero world. Record before
        // the benchmark changes the local House or any simulation settings.
        let launch = battle_session(&args.map, &sim_bench_match::COMPUTER_ROSTER[..computers]);
        let mut scene =
            headless_scenario::load_session(&args.ra2_dir, &args.map, args.seed, launch.clone())?;
        let frames = args
            .ticks
            .and_then(|n| u32::try_from(n.get()).ok())
            .ok_or("replay frame count is out of range")?;
        let replay = vera20k_core::sim::replay::ReplayRunner::record_authored_match(
            &mut scene,
            vera20k_core::sim::replay::ReplayLaunchInput::Slots(&launch),
            frames,
        )
        .map_err(|error| error.to_string())?;
        let mut bytes = Vec::new();
        replay
            .write_json(&mut bytes)
            .map_err(|error| error.to_string())?;
        return vera20k_core::util::file_system::write_new(output, &bytes)
            .map_err(|error| format!("write replay {}: {error}", output.display()));
    }
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
        ScenarioKind::Skirmish if args.watchers > 0 => headless_scenario::load_with_launch_and_ini(
            &args.ra2_dir,
            &args.map,
            args.seed,
            MatchLaunchDescriptor::from_resolved(watched_session(
                &args.map,
                computers,
                args.watchers,
            )?)
            .map_err(|error| format!("resolve Battle launch: {error}"))?,
            &Default::default(),
            ScenarioGameMode::Lan,
            None,
            None,
        )?,
        ScenarioKind::Skirmish | ScenarioKind::Battle => headless_scenario::load_session(
            &args.ra2_dir,
            &args.map,
            args.seed,
            battle_session(&args.map, &sim_bench_match::COMPUTER_ROSTER[..computers]),
        )?,
    };
    if !args.serial_paths {
        scenario.runtime.simulation.batch_path_searches();
    }
    if !args.serial_scans {
        scenario.runtime.simulation.batch_target_scans();
    }
    // As a real match's handoff does (`app::loading::transitions`).
    scenario.runtime.simulation.present_effect_anims();
    scenario
        .runtime
        .simulation
        .separate_turn_streams(!args.shared_stream);
    scenario
        .runtime
        .simulation
        .sleep_idle_objects(!args.no_sleep);
    scenario
        .runtime
        .simulation
        .check_idle_sleep(args.sleep_check);
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
            // A watcher's House is an observer, which its first defeat check defeats.
            if args.watchers == 0
                && let Some(local) = simulation.session().current_house()
            {
                simulation.make_house_multiplay_passive(local);
            }
            let ticks = args.ticks.map_or(SKIRMISH_TICKS, NonZeroU64::get);
            (simulation.houses().order().len(), 0, Plan::Skirmish, ticks)
        }
        ScenarioKind::Battle => {
            // The local house plays: its start's army fights as the others do.
            let houses = scenario.runtime.simulation.houses().order().len();
            let ticks = args.ticks.map_or(BATTLE_TICKS, NonZeroU64::get);
            let battle = Battle::start(&scenario.runtime, args.battle_units);
            (houses, 0, Plan::Battle(battle), ticks)
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
        Plan::Battle(_) => BATTLE_WARMUP,
    };
    let computers = match &plan {
        Plan::March(_) => 0,
        Plan::Match(script) => script.computers(),
        Plan::Skirmish | Plan::Battle(_) => computers,
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
    if let Some(threads) = worker_threads() {
        println!("worker_threads={threads}");
    }
    if let Plan::Match(script) = &plan {
        println!("progress tick=0 {}", script.progress(runtime));
    }

    #[cfg(feature = "telemetry")]
    runtime.simulation.set_frame_profiling(args.frame_profile);
    // A failed frame still reports the frames before it, then fails the run.
    let (measured, frames, paths, copy) = {
        let mut bench = Bench {
            runtime: &mut *runtime,
            plan: &mut plan,
            tick_ms,
            keep_frame: args.presentation_frame,
            frame: None,
            copy_times: Vec::new(),
            paths: PathTotals::default(),
            watch: (args.watchers > 0).then(Watch::default),
        };
        let (measured, frames) = run_frames(
            &mut bench,
            ticks,
            warmup,
            args.hash_every,
            args.frame_hashes.is_some(),
        );
        (measured, frames, bench.paths, bench.copy_times)
    };
    let live_at_end = runtime.simulation.entities().len();
    let csv_written = args.csv.as_ref().map_or(Ok(()), |path| {
        std::fs::write(path, csv(&measured)).map_err(|error| format!("{}: {error}", path.display()))
    });
    let frame_hashes_written = args.frame_hashes.as_ref().map_or(Ok(()), |path| {
        std::fs::write(path, frame_hashes(&measured))
            .map_err(|error| format!("{}: {error}", path.display()))
    });
    report(&mut std::io::stdout().lock(), &measured, warmup).map_err(|error| error.to_string())?;
    let steady_copy: Vec<Duration> = copy.iter().skip(warmup as usize).copied().collect();
    if !steady_copy.is_empty() {
        let total: Duration = steady_copy.iter().sum();
        println!(
            "presentation_copy_mean_ms={:.3} frames={}",
            total.as_secs_f64() * 1000.0 / steady_copy.len() as f64,
            steady_copy.len()
        );
    }
    if paths.passes > 0 {
        println!(
            "path_batch passes={} waiting={} answered={} left_out={} left_out_passes={} \
             peak_left_out={} capped={} steps={} peak_steps={} peak_search={} \
             urgency2_steps={} urgency1_steps={} peak_pass_ms={:.2}",
            paths.passes,
            paths.waiting,
            paths.answered,
            paths.left_out,
            paths.left_out_passes,
            paths.peak_left_out,
            paths.capped,
            paths.steps,
            paths.peak_steps,
            paths.peak_search,
            paths.urgency2_steps,
            paths.urgency1_steps,
            paths.peak_pass_ms,
        );
    }
    #[cfg(feature = "telemetry")]
    if args.frame_profile {
        print_frame_profile(runtime);
    }
    println!("live_objects start={live_at_start} end={live_at_end}");
    // Whether the game's data turned Ares on (`RuleSet::ares`).
    let ares = runtime.resources.rules.ares();
    println!("ares={}", if ares { "on" } else { "off" });
    frames?;
    csv_written?;
    frame_hashes_written?;
    let final_hash = runtime.simulation.state_hash();
    println!("final_hash={final_hash:016x}");
    let (asleep, counted) = runtime.simulation.idle_sleep_counts();
    let steady = runtime.simulation.steady_ai_visits();
    let locomotor_rests = runtime.simulation.locomotor_rest_visits();
    println!(
        "idle_sleep asleep_now={asleep} visits_counted={counted} steady_ai_visits={steady} locomotor_rests={locomotor_rests}"
    );
    if let Plan::Battle(battle) = &plan {
        println!(
            "battle fewest_mobile={} lost={} objects_made={}",
            battle.fewest.unwrap_or(0),
            battle.lost,
            battle.made
        );
        battle.verdict()?;
    }
    Ok(())
}

/// Give the game's worker pool `threads` threads. It must run before anything
/// uses the pool.
#[cfg(not(target_arch = "wasm32"))]
fn use_worker_threads(threads: usize) -> Result<(), String> {
    vera20k_core::sim::parallel::worker_pool()
        .num_threads(threads)
        .build_global()
        .map_err(|error| format!("--threads: {error}"))
}

/// A build without the pool runs every item on the calling thread.
#[cfg(target_arch = "wasm32")]
fn use_worker_threads(_threads: usize) -> Result<(), String> {
    Err("--threads: this build has no worker pool".to_string())
}

/// The worker pool's threads, where the build has a pool.
fn worker_threads() -> Option<usize> {
    #[cfg(not(target_arch = "wasm32"))]
    {
        Some(rayon::current_num_threads())
    }
    #[cfg(target_arch = "wasm32")]
    {
        None
    }
}

/// The process's CPU time over every thread, where the platform reports it.
fn process_cpu_time() -> Option<Duration> {
    #[cfg(unix)]
    {
        let mut time = std::mem::MaybeUninit::<libc::timespec>::uninit();
        // SAFETY: clock_gettime only writes the timespec it is given, and
        // that timespec is read only when the call reports success.
        if unsafe { libc::clock_gettime(libc::CLOCK_PROCESS_CPUTIME_ID, time.as_mut_ptr()) } == 0 {
            // SAFETY: clock_gettime succeeded, so it initialized `time`.
            let time = unsafe { time.assume_init() };
            return Some(Duration::new(time.tv_sec as u64, time.tv_nsec as u32));
        }
    }
    None
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
            // SAFETY: clock_gettime succeeded, so it initialized `time`.
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
        let (measured, result) = run_frames(&mut frames, 5, 1, 1, true);
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
