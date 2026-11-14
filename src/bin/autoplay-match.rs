//! Runs one standard skirmish headless, with auto play driving the local
//! house and the retail computer AI driving the opponent.
//!
//! This is the gate for the auto play chain: it plays a whole match with no
//! window and no human input, so "auto play can hold its own in a skirmish"
//! is something that can be run, not something that has to be remembered
//! from the last time somebody played it.
//!
//! Everything it decides depends only on the simulation state, the tick and
//! the pinned seed, in integer math, so a run replays.
//!
//! ```text
//! cargo run --bin autoplay-match -- --ra2-dir <install> --map Death.mmx
//! ```

use std::collections::BTreeMap;
use std::path::PathBuf;

use vera20k_core::headless_scenario::{self, SIM_TICK_MS};
use vera20k_core::rules::object_type::ObjectCategory;
use vera20k_core::rules::ruleset::RuleSet;
use vera20k_core::sim::autoplay::AutoPlayHost;
use vera20k_core::sim::control::brain::{Brain, Outbox};
use vera20k_core::sim::control::observation::Observation;
use vera20k_core::sim::intern::InternedId;
use vera20k_core::sim::production::ProductionCategory;
use vera20k_core::sim::strategy::retail::RetailBrain;
use vera20k_core::sim::world::Simulation;
use vera20k_core::skirmish_launch::{AiDifficulty, LaunchCountry};

/// The map main's own match bench loads by default, so it is known to have the
/// ore, the starts and the trigger data a match needs.
const DEFAULT_MAP: &str = "Death.mmx";
/// Forty minutes of game time at the native 15 frames a second: a match
/// against the computer AI does not resolve in ten, and a gate that stops
/// before one side has lost cannot tell a draw from a loss.
const DEFAULT_TICKS: u64 = 36_000;
/// A progress line every two minutes.
const DEFAULT_PROGRESS: u64 = 1_800;
/// With `--dump-teams`, the native team census every forty seconds: teams are
/// created, filled and dissolved well inside two minutes, and a coarse sample
/// cannot tell a script that advances from one that never started.
const TEAM_PROGRESS: u64 = 600;
/// Team detail lines per house, so a large `Max=` registry cannot bury the
/// rest of the run.
const MAX_TEAM_LINES: usize = 12;
/// Commands the control layer admits per frame: the same safety valve the
/// client uses (`app::input::dispatch`). It is not a hand — see the note there.
const DEFAULT_PER_FRAME: usize = 512;

struct Args {
    ra2_dir: PathBuf,
    map: String,
    seed: u32,
    ticks: u64,
    progress: u64,
    per_frame: usize,
    opponent: LaunchCountry,
    /// How many computer houses play against the local one.
    opponents: usize,
    difficulty: AiDifficulty,
    /// Print what the brain asks for, from a separate brain reading the same
    /// state, for the first this many frames. Diagnostics only: the run itself
    /// is driven by the host's own brain, whose generator has moved on.
    dump: u64,
    /// Print the native team pipeline for the first this many frames: every
    /// live team's type, script, cursor and members, plus a census every
    /// `TEAM_PROGRESS` frames and at the end. Read-only.
    dump_teams: u64,
}

fn parse_args() -> Result<Args, String> {
    let mut ra2_dir = std::env::var("RA2_DIR")
        .ok()
        .filter(|dir| !dir.is_empty())
        .map(PathBuf::from);
    let mut map = DEFAULT_MAP.to_string();
    let mut seed = 0x5EED_u32;
    let mut ticks = DEFAULT_TICKS;
    let mut progress = DEFAULT_PROGRESS;
    let mut per_frame = DEFAULT_PER_FRAME;
    let mut opponent = LaunchCountry::Russia;
    let mut opponents = 1_usize;
    let mut difficulty = AiDifficulty::Normal;
    let mut dump = 0_u64;
    let mut dump_teams = 0_u64;

    let mut args = std::env::args_os().skip(1);
    while let Some(arg) = args.next() {
        let flag = arg.to_string_lossy().into_owned();
        let mut value = || -> Result<String, String> {
            args.next()
                .map(|value| value.to_string_lossy().into_owned())
                .ok_or_else(|| format!("{flag} needs a value"))
        };
        match flag.as_str() {
            "--ra2-dir" => ra2_dir = Some(PathBuf::from(value()?)),
            "--map" => map = value()?,
            "--seed" => seed = value()?.parse().map_err(|_| "seed is a u32".to_string())?,
            "--ticks" => ticks = value()?.parse().map_err(|_| "ticks is a u64".to_string())?,
            "--progress" => {
                progress = value()?
                    .parse()
                    .map_err(|_| "progress is a u64".to_string())?
            }
            "--per-frame" => {
                per_frame = value()?
                    .parse()
                    .map_err(|_| "per-frame is a usize".to_string())?
            }
            "--opponent" => opponent = parse_country(&value()?)?,
            "--opponents" => {
                opponents = value()?
                    .parse()
                    .map_err(|_| "opponents is a count".to_string())?
            }
            "--difficulty" => difficulty = parse_difficulty(&value()?)?,
            "--dump" => dump = value()?.parse().map_err(|_| "dump is a u64".to_string())?,
            "--dump-teams" => {
                dump_teams = value()?
                    .parse()
                    .map_err(|_| "dump-teams is a u64".to_string())?
            }
            "--help" | "-h" => return Err(String::new()),
            other => return Err(format!("unknown flag {other}")),
        }
    }
    let ra2_dir = ra2_dir.ok_or("set --ra2-dir or RA2_DIR to the retail install")?;
    Ok(Args {
        ra2_dir,
        map,
        seed,
        ticks,
        progress,
        per_frame,
        opponent,
        opponents,
        difficulty,
        dump,
        dump_teams,
    })
}

fn parse_country(name: &str) -> Result<LaunchCountry, String> {
    match name.to_ascii_lowercase().as_str() {
        "america" => Ok(LaunchCountry::America),
        "russia" => Ok(LaunchCountry::Russia),
        "yuri" => Ok(LaunchCountry::Yuri),
        "korea" => Ok(LaunchCountry::Korea),
        "iraq" => Ok(LaunchCountry::Iraq),
        "france" => Ok(LaunchCountry::France),
        "germany" => Ok(LaunchCountry::Germany),
        "cuba" => Ok(LaunchCountry::Cuba),
        "libya" => Ok(LaunchCountry::Libya),
        "greatbritain" | "britain" => Ok(LaunchCountry::GreatBritain),
        other => Err(format!("unknown country {other}")),
    }
}

fn parse_difficulty(name: &str) -> Result<AiDifficulty, String> {
    match name.to_ascii_lowercase().as_str() {
        "easy" => Ok(AiDifficulty::Easy),
        "normal" => Ok(AiDifficulty::Normal),
        "hard" => Ok(AiDifficulty::Hard),
        other => Err(format!("unknown difficulty {other}")),
    }
}

const USAGE: &str = "\
autoplay-match: a headless skirmish with auto play on the local house

  --ra2-dir <dir>    the retail install (or RA2_DIR)
  --map <file>       the map to load (default Death.mmx)
  --seed <u32>       the pinned seed (default 0x5eed)
  --ticks <u64>      how many frames to play (default 36000, forty minutes)
  --progress <u64>   a progress line every this many frames (default 1800)
  --per-frame <n>    commands the control layer admits per frame (default 8)
  --opponent <name>  the computer house's country (default russia)
  --difficulty <n>   easy | normal | hard (default normal)
  --dump <n>         print the brain's orders for the first n frames
  --dump-teams <n>   print the native team pipeline (teams, scripts, cursors,
                     members) for the first n frames, a census every 600
";

/// One line of telemetry, so a run can be read without a debugger.
struct Snapshot {
    tick: u64,
    credits: i32,
    enemy_credits: i32,
    power: i32,
    own: usize,
    enemy: usize,
    ready: usize,
    rejected: usize,
    not_ready: usize,
}

fn snapshot(
    sim: &Simulation,
    local: InternedId,
    enemies: &[InternedId],
    tick: u64,
    host: &AutoPlayHost,
) -> Snapshot {
    let obs = Observation::new(sim, local);
    // The whole other side, so a 1v7 reads as seven houses and not one.
    let enemy_credits = enemies
        .iter()
        .map(|enemy| Observation::new(sim, *enemy).credits())
        .sum();
    let enemy = enemies
        .iter()
        .map(|enemy| Observation::new(sim, *enemy).own_entities().count())
        .sum();
    Snapshot {
        tick,
        credits: obs.credits(),
        enemy_credits,
        power: obs.power_balance(),
        own: obs.own_entities().count(),
        enemy,
        ready: obs.ready_buildings().count(),
        rejected: host.rejected(),
        not_ready: host.not_ready(),
    }
}

/// What the native team pipeline is doing on one side of one frame.
///
/// The question a headless match has to answer before anyone touches tactics
/// is *who is fighting*: units a live team holds, driven by a retail script
/// (`TeamClass::AI`), or units standing outside every team, driven by this
/// run's own orders. These are counts of that split, plus why a team that
/// exists is not moving.
#[derive(Default)]
struct TeamCensus {
    /// Live teams owned by the side.
    teams: usize,
    /// Teams whose recruit loop filled them once (`+0x7F`), so their script
    /// is running.
    formed: usize,
    /// Units in a team (`TeamClass+0x54` member lists).
    members: usize,
    /// Formed teams whose script cursor moved since the previous sample.
    advancing: usize,
    /// Formed teams whose script cursor stood still since the previous
    /// sample: the script is blocked, not merely slow.
    stalled: usize,
    /// Teams whose script stopped on an unported action.
    refused: usize,
    /// Teams that reached the end of their script.
    succeeded: usize,
    /// Orderable non-building units of the side that belong to no team.
    loose: usize,
    /// Live teams per current script action id.
    actions: BTreeMap<i32, usize>,
}

impl TeamCensus {
    fn line(&self) -> String {
        let actions: Vec<String> = self
            .actions
            .iter()
            .map(|(action, count)| format!("{action}:{count}"))
            .collect();
        format!(
            "{} teams/{} formed ({} advancing, {} stalled, {} refused, {} done), \
             {} members, {} loose, actions [{}]",
            self.teams,
            self.formed,
            self.advancing,
            self.stalled,
            self.refused,
            self.succeeded,
            self.members,
            self.loose,
            actions.join(" ")
        )
    }
}

/// Counts the native team pipeline for `houses`, and returns the cursors it
/// saw so the next sample can tell a running script from a stuck one.
///
/// Read-only: it walks `TeamScriptVm`'s existing views and the entity store.
/// `previous` is the caller's own scratch, not simulation state.
fn census_teams(
    sim: &Simulation,
    rules: &RuleSet,
    houses: &[InternedId],
    previous: &mut BTreeMap<u64, i32>,
) -> TeamCensus {
    let vm = sim.team_script_vm();
    let mut census = TeamCensus::default();
    let mut cursors = BTreeMap::new();
    for team in vm.teams_in_order() {
        if !houses.contains(&team.owner()) {
            continue;
        }
        census.teams += 1;
        census.members += team.members().count();
        census.refused += usize::from(team.refusal().is_some());
        census.succeeded += usize::from(team.succeeded());
        *census.actions.entry(team.current_action(vm)).or_default() += 1;
        if team.formed() {
            census.formed += 1;
            match previous.get(&team.id()) {
                Some(&before) if team.cursor() > before => census.advancing += 1,
                Some(_) => census.stalled += 1,
                // The first sample has nothing to compare against.
                None => {}
            }
        }
        cursors.insert(team.id(), team.cursor());
    }
    *previous = cursors;

    for house in houses {
        for entity in Observation::new(sim, *house).own_entities() {
            if vm.team_for_member(entity.entity_id).is_some() {
                continue;
            }
            let combat = sim
                .object_type(entity.type_id, rules)
                .is_some_and(|ty| ty.category != ObjectCategory::Building && entity.orderable);
            census.loose += usize::from(combat);
        }
    }
    census
}

/// Prints one house's live teams: TeamType, script, cursor, current action,
/// member count by unit type. Read-only.
fn print_teams(sim: &Simulation, house: InternedId, tick: u64, previous: &BTreeMap<u64, i32>) {
    let vm = sim.team_script_vm();
    let name = sim.interner().resolve(house);
    let mut types: BTreeMap<u64, InternedId> = BTreeMap::new();
    for entity in Observation::new(sim, house).own_entities() {
        types.insert(entity.entity_id, entity.type_id);
    }
    let mut shown = 0_usize;
    let mut total = 0_usize;
    for team in vm.teams_in_order() {
        if team.owner() != house {
            continue;
        }
        total += 1;
        if shown == MAX_TEAM_LINES {
            continue;
        }
        shown += 1;
        let mut counts: BTreeMap<String, usize> = BTreeMap::new();
        for member in team.members() {
            let label = types
                .get(&member)
                .map(|type_id| sim.interner().resolve(*type_id))
                .unwrap_or("?");
            *counts.entry(label.to_owned()).or_default() += 1;
        }
        let members: Vec<String> = counts
            .iter()
            .map(|(label, count)| format!("{label}x{count}"))
            .collect();
        let state = if team.succeeded() {
            "done"
        } else if team.refusal().is_some() {
            "refused"
        } else if !team.formed() {
            "forming"
        } else if previous
            .get(&team.id())
            .is_none_or(|before| team.cursor() > *before)
        {
            "running"
        } else {
            "stalled"
        };
        println!(
            "  team@{tick} {name}: {} script {} cursor {} action {} {state} members [{}]",
            team.team_type_id()
                .map(|id| sim.interner().resolve(id))
                .unwrap_or("?"),
            sim.interner().resolve(team.script_id()),
            team.cursor(),
            team.current_action(vm),
            members.join(" "),
        );
    }
    if total > shown {
        println!("  team@{tick} {name}: ... and {} more", total - shown);
    }
}

/// The AI trigger table's own activity: how many triggers exist, how many have
/// a finished team on their record, and the first few of those.
///
/// The counts are the trigger's own accounting (`RegisterSuccess` /
/// `RegisterFailure`, `+0x104`/`+0x108`), which native updates when a team it
/// started ends — so a trigger that has been picked but whose team is still
/// alive reads as zero here. This is the other half of "the computer's AI is
/// not attacking": a table nothing ever picks is a different defect from
/// teams that form and then sit still.
fn print_ai_triggers(sim: &Simulation, tick: u64) {
    let vm = sim.team_script_vm();
    let mut total = 0_usize;
    let mut fired: Vec<String> = Vec::new();
    let mut ever_started = 0_usize;
    for (definition, record) in vm.ai_triggers_in_order() {
        total += 1;
        if record.attempts() == 0 {
            continue;
        }
        ever_started += 1;
        if fired.len() < MAX_TEAM_LINES {
            fired.push(format!(
                "{} ({} attempts, {} ok{})",
                definition.display_name,
                record.attempts(),
                record.successes(),
                if definition.enabled { "" } else { ", disabled" }
            ));
        }
    }
    println!(
        "  triggers@{tick}: {total} registered, {ever_started} with a finished team{}",
        if fired.is_empty() {
            String::new()
        } else {
            format!(": {}", fired.join("; "))
        }
    );
}

/// Each house's activation latch and chosen enemy, as the native AI leaves
/// them: the two fields the computer's own `HouseClass::Update` would set.
fn print_house_ai(sim: &Simulation, houses: &[InternedId], tick: u64) {
    for house in houses {
        let Some(state) = sim.houses().get(house) else {
            continue;
        };
        let activation = state.ai_activation();
        println!(
            "  house@{tick} {}: human {} enemy {} | triggers_active {} production {} \
             autocreate {} base_building {}",
            sim.interner().resolve(*house),
            state.is_human(),
            state
                .enemy_house()
                .map(|id| sim.interner().resolve(id))
                .unwrap_or("-"),
            activation.ai_triggers_active,
            activation.production,
            activation.autocreate_allowed,
            activation.auto_base_building,
        );
    }
}

/// The whole team report for one sample: activation latches, trigger activity,
/// then per house the census line and the live teams.
fn report_teams(
    sim: &Simulation,
    rules: &RuleSet,
    houses: &[InternedId],
    tick: u64,
    previous: &mut BTreeMap<InternedId, BTreeMap<u64, i32>>,
) {
    print_house_ai(sim, houses, tick);
    print_ai_triggers(sim, tick);
    for house in houses {
        let before = previous.get(house).cloned().unwrap_or_default();
        let census = census_teams(sim, rules, &[*house], previous.entry(*house).or_default());
        println!(
            "  teams@{tick} {}: {}",
            sim.interner().resolve(*house),
            census.line()
        );
        print_teams(sim, *house, tick, &before);
    }
}

/// A logger that prints `retail-ai` and friends to stderr. The brain logs why
/// it is waiting, which is the whole point of a headless run: nobody is
/// watching the base to notice it stopped.
struct StderrLog;

impl log::Log for StderrLog {
    fn enabled(&self, _metadata: &log::Metadata<'_>) -> bool {
        true
    }

    fn log(&self, record: &log::Record<'_>) {
        eprintln!("[{}] {}", record.level(), record.args());
    }

    fn flush(&self) {}
}

fn main() -> Result<(), String> {
    static LOGGER: StderrLog = StderrLog;
    log::set_logger(&LOGGER).map_err(|error| error.to_string())?;
    log::set_max_level(if std::env::var("VERA20K_TRACE").is_ok() {
        log::LevelFilter::Debug
    } else {
        log::LevelFilter::Info
    });
    log::info!("logger up");

    let args = match parse_args() {
        Ok(args) => args,
        Err(error) => {
            if error.is_empty() {
                print!("{USAGE}");
                return Ok(());
            }
            eprintln!("autoplay-match: {error}\n\n{USAGE}");
            std::process::exit(2);
        }
    };

    let seats: Vec<(LaunchCountry, AiDifficulty)> =
        std::iter::repeat_n((args.opponent, args.difficulty), args.opponents.max(1)).collect();
    let session = headless_scenario::battle_session(&args.map, &seats);
    let mut scenario =
        headless_scenario::load_session(&args.ra2_dir, &args.map, args.seed, session)?;
    let sim = &scenario.runtime.simulation;

    let local = sim
        .session()
        .current_house()
        .ok_or("the launch has no local house")?;
    // Every computer house on the map is an opponent: the run is won when they
    // are all defeated, which on a 1v7 is seven bases and not one.
    //
    // The passive houses are not: the Neutral and Special owners hold the map's
    // civilians and the scripted props, nobody can defeat them, and counting
    // them turned every run into a timeout the moment the list stopped being
    // "the first computer house".
    let enemies: Vec<InternedId> = sim
        .houses()
        .order()
        .iter()
        .copied()
        .filter(|id| {
            *id != local
                && sim
                    .houses()
                    .get(id)
                    .is_some_and(|house| !house.is_human() && !house.multiplay_passive())
        })
        .collect();
    if enemies.is_empty() {
        return Err("the launch has no computer opponent".to_string());
    }

    println!(
        "autoplay-match: {} on {}, seed {:#x}, {} ticks, {} per frame",
        sim.interner().resolve(local),
        args.map,
        args.seed,
        args.ticks,
        args.per_frame
    );
    println!(
        "{:>6} {:>9} {:>9} {:>7} {:>6} {:>6} {:>6} {:>9} {:>9}",
        "tick", "credits", "ecred", "power", "own", "enemy", "ready", "rejected", "notready"
    );

    if args.dump > 0 {
        for entity in Observation::new(sim, local).own_entities() {
            println!(
                "  start: {} {} at {:?} health {} mission {:?} orderable {}",
                entity.entity_id,
                sim.interner().resolve(entity.type_id),
                entity.cell,
                entity.health,
                entity.mission,
                entity.orderable
            );
        }
    }

    let mut host = AutoPlayHost::new(
        Box::new(RetailBrain::new(u64::from(args.seed))),
        local,
        args.per_frame,
    );
    let mut last = snapshot(sim, local, &enemies, 0, &host);
    println!(
        "{:>6} {:>9} {:>9} {:>7} {:>6} {:>6} {:>6} {:>9} {:>9}",
        last.tick,
        last.credits,
        last.enemy_credits,
        last.power,
        last.own,
        last.enemy,
        last.ready,
        last.rejected,
        last.not_ready
    );

    // Both sides, so the report says who is fighting, not just how much.
    let mut houses = vec![local];
    houses.extend(enemies.iter().copied());
    // Per house, each team's script cursor at the previous sample. Diagnostic
    // scratch only: it never feeds back into the simulation.
    let mut team_cursors: BTreeMap<InternedId, BTreeMap<u64, i32>> = BTreeMap::new();
    if args.dump_teams > 0 {
        report_teams(
            sim,
            &scenario.runtime.resources.rules,
            &houses,
            0,
            &mut team_cursors,
        );
    }

    let mut outcome = "timeout";
    // The frame the loop actually reached: the snapshot only refreshes on a
    // progress line, so `last.tick` stays 0 when a short run ends first.
    let mut reached = 0_u64;
    for tick in 1..=args.ticks {
        reached = tick;
        if tick <= args.dump {
            let sim = &scenario.runtime.simulation;
            let obs = Observation::new(sim, local);
            let mut brain = RetailBrain::new(u64::from(args.seed));
            let mut out = Outbox::default();
            brain.think(&scenario.runtime.resources.rules, &obs, &mut out);
            let funnel = vera20k_core::sim::behavior::Funnel::new(
                sim,
                &scenario.runtime.resources.rules,
                local,
            );
            for order in &out.orders {
                println!(
                    "  brain@{tick}: {:?} -> {:?}",
                    order,
                    funnel.prepare(order.actor, order.goal)
                );
            }
            println!("  brain@{tick}: intents={:?}", out.intents);
        }
        {
            let runtime = &mut scenario.runtime;
            host.step(&mut runtime.simulation, &runtime.resources.rules);
        }
        scenario
            .runtime
            .advance_frame_for_tooling(&[], SIM_TICK_MS)
            .map_err(|error| format!("tick {tick}: {error}"))?;

        let sim = &scenario.runtime.simulation;
        let won = enemies.iter().all(|enemy| is_defeated(sim, *enemy));
        let lost = is_defeated(sim, local);
        if lost {
            outcome = "lost";
        } else if won {
            outcome = "won";
        }
        if args.dump_teams > 0 && tick % TEAM_PROGRESS == 0 {
            let rules = &scenario.runtime.resources.rules;
            if tick <= args.dump_teams {
                report_teams(sim, rules, &houses, tick, &mut team_cursors);
            } else {
                for house in &houses {
                    let census = census_teams(
                        sim,
                        rules,
                        &[*house],
                        team_cursors.entry(*house).or_default(),
                    );
                    println!(
                        "  teams@{tick} {}: {}",
                        sim.interner().resolve(*house),
                        census.line()
                    );
                }
            }
        }
        if tick % args.progress == 0 || outcome != "timeout" {
            if args.dump > 0 {
                let obs = Observation::new(sim, local);
                let mut counts: std::collections::BTreeMap<String, usize> =
                    std::collections::BTreeMap::new();
                for entity in obs.own_entities() {
                    *counts
                        .entry(format!(
                            "{}#{:?}",
                            sim.interner().resolve(entity.type_id),
                            entity.mission
                        ))
                        .or_default() += 1;
                }
                let line: Vec<String> = counts
                    .iter()
                    .map(|(name, count)| format!("{name} {count}"))
                    .collect();
                println!("  own@{tick}: {}", line.join(", "));
                // What the other side is fielding, by type: an army census
                // that only counted its own units cannot answer "what beat
                // us", and the types are the sim's own names for them.
                let mut theirs: std::collections::BTreeMap<String, usize> =
                    std::collections::BTreeMap::new();
                for entity in enemies.iter().flat_map(|enemy| {
                    Observation::new(sim, *enemy)
                        .own_entities()
                        .collect::<Vec<_>>()
                }) {
                    *theirs
                        .entry(sim.interner().resolve(entity.type_id).to_owned())
                        .or_default() += 1;
                }
                let line: Vec<String> = theirs
                    .iter()
                    .map(|(name, count)| format!("{name} {count}"))
                    .collect();
                println!("  enemy@{tick}: {}", line.join(", "));
                let yard = obs
                    .building_in_production()
                    .map(|(type_id, suspended)| {
                        format!("{} held={suspended}", sim.interner().resolve(type_id))
                    })
                    .unwrap_or_else(|| "idle".to_owned());
                let rules = &scenario.runtime.resources.rules;
                let factories = |owner: InternedId| {
                    let obs = Observation::new(sim, owner);
                    [ProductionCategory::Vehicle, ProductionCategory::Infantry]
                        .into_iter()
                        .map(|category| {
                            let text = obs
                                .factory_product(category)
                                .map(|(type_id, suspended)| {
                                    let progress =
                                        obs.production_progress(rules, type_id).unwrap_or(0);
                                    format!(
                                        "{}@{progress}{}",
                                        sim.interner().resolve(type_id),
                                        if suspended { "!" } else { "" }
                                    )
                                })
                                .unwrap_or_else(|| "-".to_owned());
                            format!("{category:?}={text}")
                        })
                        .collect::<Vec<_>>()
                        .join(" ")
                };
                let their_lines: Vec<String> = enemies
                    .iter()
                    .map(|enemy| {
                        format!("{}: {}", sim.interner().resolve(*enemy), factories(*enemy))
                    })
                    .collect();
                println!(
                    "  spend@{tick}: yard {yard} | own {} | enemy {}",
                    factories(local),
                    their_lines.join(" "),
                );
            }
            last = snapshot(sim, local, &enemies, tick, &host);
            println!(
                "{:>6} {:>9} {:>9} {:>7} {:>6} {:>6} {:>6} {:>9} {:>9}",
                last.tick,
                last.credits,
                last.enemy_credits,
                last.power,
                last.own,
                last.enemy,
                last.ready,
                last.rejected,
                last.not_ready
            );
        }
        if outcome != "timeout" {
            break;
        }
    }

    println!("autoplay-match: {outcome} at tick {reached}");
    // A run that ended on a census frame has already printed this one.
    if args.dump_teams > 0 && reached % TEAM_PROGRESS != 0 {
        let sim = &scenario.runtime.simulation;
        report_teams(
            sim,
            &scenario.runtime.resources.rules,
            &houses,
            reached,
            &mut team_cursors,
        );
    }
    if args.dump > 0 {
        let sim = &scenario.runtime.simulation;
        let obs = Observation::new(sim, local);
        for entity in obs.own_entities() {
            println!(
                "  final: {} at {:?} health {} mission {:?} inprod {}",
                sim.interner().resolve(entity.type_id),
                entity.cell,
                entity.health,
                entity.mission,
                obs.in_production(&scenario.runtime.resources.rules, entity.type_id)
            );
        }
        for entity in enemies.iter().flat_map(|enemy| {
            Observation::new(sim, *enemy)
                .own_entities()
                .collect::<Vec<_>>()
        }) {
            println!(
                "  enemy: {} at {:?} health {}",
                sim.interner().resolve(entity.type_id),
                entity.cell,
                entity.health
            );
        }
    }
    Ok(())
}

fn is_defeated(sim: &Simulation, house: InternedId) -> bool {
    sim.houses()
        .get(&house)
        .is_some_and(|house| house.is_defeated())
}
