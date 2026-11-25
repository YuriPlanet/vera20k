//! `autoplay-replay` — read a recorded match back and report the whole process.
//!
//! Every skirmish the client plays leaves a replay behind
//! (`replays/replay_tick{tick}_{secs}.json`): a header, the resolved launch,
//! and then one row per tick carrying every command any house issued that
//! frame, plus a state hash every fifteenth frame. Playback re-runs that log
//! through the production loader and the ordinary playback cursor, so a report
//! built while it runs describes the match that was actually played rather
//! than a reconstruction from a summary.
//!
//! Two files come out, named after the replay:
//!
//!   * `<name>.report.json` — the whole process: the setup, every house's
//!     economy and army, exact-tick events, a sampled timeline, and the focus
//!     house's roster at each sample.
//!   * `<name>.report.md` — the same numbers as one page a reader can skim.
//!
//! The point is a *reference*. A policy tuned only against its own summary
//! lines is tuned against a mirror; a match a human won, read back in the same
//! units the policy is judged in, is something to compare against.
//!
//! ```text
//! autoplay-replay --ra2-dir <install> --replay replays/replay_tick1234_5678.json
//! ```
//!
//! What is measured, and where it comes from:
//!
//!   * the command stream — every order and every production click, with the
//!     tick it landed on and the house that issued it, read straight off the
//!     replay rows;
//!   * the houses — credits, `Spend_Money`'s running total, score, power,
//!     base centre and the defeat flag, from `HouseState` and `Economy`;
//!   * the army — counted per type off the entity store at each sample, valued
//!     at the rules' own cost through `Simulation::cost_of`;
//!   * the losses — `HouseTracking`'s per-pair totals (exact for the whole
//!     match) and its retained death rows (tick, mission, health at the kill).
//!
//! Two readings are deliberately *not* here: a per-house kill count (the
//! engine credits a kill to the victim's ledger by killer *type*, never by
//! killer house), and anything the replay does not contain. A legacy
//! diagnostic segment without a startup header cannot be rebuilt at all, and
//! the tool says so instead of guessing.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde_json::{Value, json};
use vera20k_core::assets::asset_manager::{AssetManager, MediaArchiveMode};
use vera20k_core::headless_scenario::{self, HeadlessScenario};
use vera20k_core::map::entities::EntityCategory;
use vera20k_core::rules::audio_sources::AudioDefinitions;
use vera20k_core::rules::ini_parser::IniFile;
use vera20k_core::rules::object_type::ObjectCategory;
use vera20k_core::rules::personal_rules::PersonalRules;
use vera20k_core::rules::process_owner::NativeRulesProcessOwner;
use vera20k_core::rules::retail_sources::RetailRulesSources;
use vera20k_core::rules::ruleset::RuleSet;
use vera20k_core::sim::combat::TargetKind;
use vera20k_core::sim::command::{
    Command, PaidSuperAction, PlaceRecord, ProductionEvent, ProductionRecord,
    SpecialPlaceRecord,
};
use vera20k_core::sim::control::observation::Observation;
use vera20k_core::sim::intern::InternedId;
use vera20k_core::sim::mission::MissionId;
use vera20k_core::sim::production::ProductionCategory;
use vera20k_core::sim::replay::{ReplayLog, ReplayPlayback, ReplayTick};
use vera20k_core::sim::world::{FrameEffects, Simulation};

/// Report schema, so a later reader can tell what it is holding.
const REPORT_SCHEMA: &str = "vera20k.replay-report.v1";
/// This tool's own version, inside the report.
const TOOL_VERSION: u32 = 1;
/// Timeline interval by default: five seconds of match at the replay's own
/// rate. Events carry exact ticks; this is the continuous shape between them.
const DEFAULT_SAMPLE_SECONDS: f64 = 5.0;
/// Rows one (house, kind) of order may contribute before the report keeps
/// counting without keeping rows. A click storm is not a report.
/// Rows one (house, kind) of order keeps by default before the report keeps
/// counting without keeping rows. A long match issues thousands of orders, and
/// the shape matters more than any single click.
const DEFAULT_MAX_ORDER_ROWS: usize = 2_000;
/// Rows of the focus house's build order the page shows before pointing at the
/// JSON for the rest.
const MAX_BUILD_ROWS_IN_PAGE: usize = 120;

const USAGE: &str = "\
autoplay-replay: replay a recorded match and report the whole process

  --ra2-dir <dir>       the retail install (or RA2_DIR)
  --replay <file.json>  the replay to read (replays/replay_tick*.json)
  --map <file>          the recorded map; default: beside the replay, then
                        VERA's own Maps folder, then the header's name in the
                        retail install
  --out <dir>           where the two reports go (default: beside the replay)
  --house <name>        the focus house (default: the human house)
  --sample-seconds <s>  timeline interval in seconds (default 5)
  --max-ticks <n>       stop after n recorded ticks, for a quick look
  --max-orders <n>      order rows kept per house and kind (default 2000)
  --dump-type-table <p> write (category, type-array index) → type name here,
                        for reading a replay this build cannot load itself
  --allow-rules-mismatch
                        read a replay whose rules/art hash this build does not
                        reproduce; every recorded state hash is still checked,
                        and the report says where they stop matching
  --verify-only         replay and check the hashes; write nothing
  --quiet               no summary on stdout

The report is written next to the replay unless --out says otherwise:
<name>.report.json and <name>.report.md.
";

struct Args {
    ra2_dir: PathBuf,
    replay: PathBuf,
    map: Option<String>,
    out: Option<PathBuf>,
    house: Option<String>,
    sample_seconds: f64,
    max_ticks: Option<usize>,
    max_order_rows: usize,
    dump_type_table: Option<PathBuf>,
    allow_rules_mismatch: bool,
    verify_only: bool,
    quiet: bool,
}

fn main() {
    match run() {
        Ok(()) => {}
        // An empty message is `--help`: the usage on stdout, success.
        Err(error) if error.is_empty() => print!("{USAGE}"),
        Err(error) => {
            eprintln!("autoplay-replay: {error}");
            std::process::exit(1);
        }
    }
}

fn run() -> Result<(), String> {
    let args = parse_args()?;
    let file_bytes = std::fs::read(&args.replay)
        .map_err(|error| format!("read {}: {error}", args.replay.display()))?;
    let file_sha256 = vera20k_core::util::sha256::sha256_hex(&file_bytes);
    let replay = Arc::new(
        ReplayLog::load(&args.replay)
            .map_err(|error| format!("parse {}: {error:#}", args.replay.display()))?,
    );

    let recorded_ticks = replay.ticks.len();
    let first_tick = replay.ticks.first().map_or(0, |tick| tick.tick);
    let last_tick = replay.ticks.last().map_or(0, |tick| tick.tick);
    let recorded_hashes = replay
        .ticks
        .iter()
        .filter(|tick| tick.state_hash.is_some())
        .count();

    // A segment without a startup cannot be rebuilt: there is no launch, no seed
    // binding and no map identity to load. Its command stream is still complete,
    // so say what it holds instead of refusing the file.
    if replay.startup.is_none() {
        let report = build_static_report(&args, &replay, &file_sha256, file_bytes.len());
        return write_report(&args, report, "static");
    }

    // The startup owns the launch, the seed and the map identity. Copy what
    // the loader needs out of it before the replay moves into the playback.
    let (map_sha256, seed, game_mode, native_id_prefix, mut session) = {
        let startup = replay.standalone_startup().map_err(|error| {
            format!(
                "{} has no replayable startup: {error:#}. A legacy diagnostic \
                 segment records commands and hashes but cannot be rebuilt; \
                 record a fresh match instead.",
                args.replay.display()
            )
        })?;
        (
            startup.map_sha256().to_owned(),
            startup.seed(),
            startup.game_mode(),
            startup.native_id_prefix().map_err(|error| format!("{error:#}"))?,
            startup.launch_session().clone(),
        )
    };

    let map_file = resolve_map(&args, &replay.header.map_name, &map_sha256)?;
    session.selected_map_file = Some(map_file.clone());

    // The rules the game actually played: retail sources plus the two Options
    // switches that reach the rules. Without them the rules hash differs from
    // the replay's, and no analysis of that replay would be honest.
    let mut rules_owner = load_rules_owner(&args.ra2_dir)?;
    let launch = vera20k_core::sim::scenario_bootstrap::MatchLaunchDescriptor::from_resolved(
        session.clone(),
    )
    .map_err(|error| format!("resolve the recorded launch: {error}"))?;

    let mut loaded: HeadlessScenario = headless_scenario::load_with_launch_and_ini(
        &args.ra2_dir,
        &map_file,
        seed,
        launch,
        &Default::default(),
        game_mode,
        Some(native_id_prefix),
        Some(&mut rules_owner),
    )
    .map_err(|error| format!("load {map_file} from {}: {error}", args.ra2_dir.display()))?;

    // The ordinary client enables these before recording frame zero; playback
    // refuses a world built any other way.
    loaded.runtime.simulation.batch_path_searches();
    loaded.runtime.simulation.batch_target_scans();
    loaded.runtime.simulation.sleep_idle_objects_in_match();

    let map_hash = loaded.map.ini.content_hash();
    // Playback would refuse a different rules/art hash with a message that
    // names neither side. Say both here: a mismatch means the replay was
    // recorded by a build whose INI processing differs, and no analysis of it
    // can be trusted until it is replayed by that build.
    let mut playback_source: Arc<ReplayLog> = replay.clone();
    let recorded_rules_hash = replay.header.rules_hash;
    let current_rules_hash = loaded.runtime.resources.rules.simulation_config_hash();
    let rules_hash_match = recorded_rules_hash == 0 || recorded_rules_hash == current_rules_hash;
    if !rules_hash_match {
        if !args.allow_rules_mismatch {
            return Err(format!(
                "{} was recorded with rules/art hash {recorded_rules_hash:#018x}; this build \
                 produces {current_rules_hash:#018x}. That hash covers the loaded INI layers \
                 (rules, art, the player's Options switches), so a reader that cannot reproduce it \
                 is reading a different ruleset. Replay it with the build that recorded it, or \
                 pass --allow-rules-mismatch to let the recorded state hashes decide: every \
                 frame that hashed is then compared, and the report says where they stop \
                 matching.",
                args.replay.display()
            ));
        }
        // The rules hash is a build-local coordinate, not a behaviour. Pin this
        // build's value so the playback cursor will run, and let the recorded
        // per-frame state hashes be the actual proof: if the rulesets differ in
        // any way the simulation can see, the first hashed frame after the
        // difference fails and the report says so.
        let mut patched = (*replay).clone();
        patched.header.rules_hash = current_rules_hash;
        playback_source = Arc::new(patched);
    }
    let mut playback = ReplayPlayback::prepare(
        &mut loaded.runtime,
        playback_source,
        &loaded.source,
        map_hash,
    )
    .map_err(|error| format!("bind the recorded startup: {error:#}"))?;

    let tick_hz = replay.header.tick_hz.max(1);
    let limit = args.max_ticks.unwrap_or(recorded_ticks).min(recorded_ticks);
    let sample_ticks = sample_ticks_for(args.sample_seconds, tick_hz);

    let mut walk = Walk::new(
            &loaded.runtime.simulation,
            &loaded.runtime.resources.rules,
            args.house.as_deref(),
            sample_ticks,
            args.max_order_rows,
        );
    if let Some(path) = &args.dump_type_table {
        walk.dump_type_table(path)?;
    }

    // Walk the recorded frames. A frame that fails (a state hash that does not
    // match, most often) stops the walk but keeps everything before it: a
    // partial report that says where it stopped beats no report.
    let mut walked = 0_usize;
    let mut stopped: Option<String> = None;
    while walked < limit {
        let entry = &replay.ticks[walked];
        walk.record_commands(&loaded.runtime.simulation, entry);
        match playback.advance(&mut loaded.runtime, FrameEffects::empty()) {
            Ok(Some(output)) => {
                walk.after_frame(
                    &loaded.runtime.simulation,
                    &loaded.runtime.resources.rules,
                    output.tick.tick,
                );
            }
            Ok(None) => break,
            Err(error) => {
                stopped = Some(format!("tick {}: {error:#}", entry.tick));
                break;
            }
        }
        walked += 1;
    }

    // What the playback itself proved. Every recorded hash is compared inside
    // `advance`, so a run that reached the end matched all of them.
    let verification = if walked >= recorded_ticks && stopped.is_none() {
        playback
            .verification(&loaded.runtime)
            .map(Some)
            .map_err(|error| format!("verify: {error:#}"))?
    } else {
        None
    };

    if args.verify_only {
        // A replay that stopped before the recording's last tick — by a hash
        // mismatch, an undecodable command or the end of the input — has
        // verified nothing. Printing the stop and exiting zero is how a failed
        // verification reads as a pass, so it is an error instead.
        let Some(verification) = verification.as_ref() else {
            let stopped_at = match &stopped {
                Some(stopped) => format!("stopped at {stopped}"),
                None => format!("stopped at tick {walked} of {recorded_ticks}"),
            };
            if !args.quiet {
                println!("{}: {stopped_at}", args.replay.display());
            }
            return Err(format!("{}: {stopped_at}", args.replay.display()));
        };
        if !args.quiet {
            println!(
                "{}: {} ticks, {} hashes checked, final hash {:#018x}",
                args.replay.display(),
                verification.ticks,
                verification.checked_hashes,
                verification.final_state_hash
            );
        }
        return Ok(());
    }

    let report = build_report(
        &args,
        &replay,
        &file_sha256,
        file_bytes.len(),
        &map_file,
        map_sha256,
        map_hash,
        seed,
        tick_hz,
        &session,
        verification.as_ref(),
        &loaded.runtime.simulation,
        walk,
        walked,
        first_tick,
        last_tick,
        recorded_hashes,
        rules_hash_match,
        current_rules_hash,
        stopped,
    );

    write_report(&args, report, "full")
}

/// Write both reports and say where they went.
fn write_report(args: &Args, report: Value, mode: &str) -> Result<(), String> {
    let out_dir = args
        .out
        .clone()
        .unwrap_or_else(|| args.replay.parent().unwrap_or(Path::new(".")).to_path_buf());
    std::fs::create_dir_all(&out_dir)
        .map_err(|error| format!("create {}: {error}", out_dir.display()))?;
    let stem = args
        .replay
        .file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or("replay")
        .to_owned();
    let suffix = if mode == "static" { ".static" } else { "" };
    let json_path = out_dir.join(format!("{stem}{suffix}.report.json"));
    let md_path = out_dir.join(format!("{stem}{suffix}.report.md"));

    let json_text = serde_json::to_string_pretty(&report)
        .map_err(|error| format!("encode the report: {error}"))?;
    std::fs::write(&json_path, format!("{json_text}\n"))
        .map_err(|error| format!("write {}: {error}", json_path.display()))?;
    let md_text = if mode == "static" {
        render_static_markdown(&report)
    } else {
        render_markdown(&report)
    };
    std::fs::write(&md_path, &md_text)
        .map_err(|error| format!("write {}: {error}", md_path.display()))?;

    if !args.quiet {
        println!(
            "autoplay-replay: {} — {} report",
            args.replay.display(),
            if mode == "static" { "static" } else { "full" }
        );
        println!("  json: {}", json_path.display());
        println!("  page: {}", md_path.display());
    }
    Ok(())
}

/// A segment with no startup: the command stream is still there, but nothing
/// can be rebuilt from it. Report its shape rather than refusing the file.
fn build_static_report(
    args: &Args,
    replay: &ReplayLog,
    file_sha256: &str,
    file_bytes: usize,
) -> Value {
    let mut kinds: BTreeMap<String, usize> = BTreeMap::new();
    let mut owners: BTreeMap<u32, usize> = BTreeMap::new();
    let mut first_commands: Vec<Value> = Vec::new();
    for entry in &replay.ticks {
        for command in &entry.commands {
            let kind = command_kind(&command.payload).to_owned();
            *kinds.entry(kind.clone()).or_insert(0) += 1;
            *owners.entry(command.owner.index()).or_insert(0) += 1;
            if first_commands.len() < 40 {
                first_commands.push(json!({
                    "tick": entry.tick,
                    "owner": command.owner,
                    "kind": kind,
                }));
            }
        }
    }
    let last_tick = replay.ticks.last().map_or(0, |tick| tick.tick);
    let tick_hz = replay.header.tick_hz.max(1);
    json!({
        "tool": { "name": "autoplay-replay", "version": TOOL_VERSION, "schema": REPORT_SCHEMA },
        "replay": {
            "path": args.replay.to_string_lossy(),
            "file_sha256": file_sha256,
            "file_bytes": file_bytes,
            "format_version": replay.header.version,
            "tick_hz": tick_hz,
            "seed": replay.header.seed,
            "map_name": replay.header.map_name,
            "rules_hash": format!("{:#018x}", replay.header.rules_hash),
            "recorded_ticks": replay.ticks.len(),
            "first_tick": replay.ticks.first().map_or(0, |tick| tick.tick),
            "last_tick": last_tick,
            "recorded_seconds": last_tick as f64 / f64::from(tick_hz),
            "recorded_state_hashes": replay.ticks.iter().filter(|tick| tick.state_hash.is_some()).count(),
            "startup": false,
        },
        "note": "no startup: this segment cannot be rebuilt, so no money, army or \
                 building numbers are reported. The counts below are the recorded \
                 command wrappers; a build records a local command as its native \
                 record, so decoding them needs the engine's interner.",
        "command_kinds": kinds,
        "commands_by_owner": owners,
        "first_commands": first_commands,
    })
}

/// The enum variant name of a recorded payload, for the static report.
fn command_kind(payload: &Command) -> &'static str {
    match payload {
        Command::SynchronizedRecord { .. } => "SynchronizedRecord",
        Command::Select { .. } => "Select",
        Command::MoveFlash { .. } => "MoveFlash",
        Command::AssignControlGroup { .. } => "AssignControlGroup",
        Command::PlanningRecord { .. } => "PlanningRecord",
        Command::QueueProduction { .. } => "QueueProduction",
        Command::PlaceReadyBuilding { .. } => "PlaceReadyBuilding",
        Command::SellBuilding { .. } => "SellBuilding",
        Command::CancelProductionByType { .. } => "CancelProductionByType",
        Command::DeployMcv { .. } => "DeployMcv",
        Command::UndeployBuilding { .. } => "UndeployBuilding",
        Command::CaptureBuilding { .. } => "CaptureBuilding",
        Command::Move { .. } => "Move",
        Command::MoveToObject { .. } => "MoveToObject",
        Command::Attack { .. } => "Attack",
        Command::AttackMove { .. } => "AttackMove",
        Command::Guard { .. } => "Guard",
        Command::Stop { .. } => "Stop",
        Command::SetRally { .. } => "SetRally",
        _ => "other",
    }
}

/// The function checklist as report rows: every checklist entry, covered or not.
fn function_checklist(walk: &Walk) -> Vec<Value> {
    FUNCTION_CHECKLIST
        .iter()
        .map(|(name, label)| {
            let coverage = walk.functions.get(*name);
            json!({
                "function": name,
                "label": label,
                "count": coverage.map_or(0, |coverage| coverage.count),
                "first_tick": coverage.and_then(|coverage| coverage.first_tick),
                "last_tick": coverage.and_then(|coverage| coverage.last_tick),
            })
        })
        .collect()
}

/// The static report as one page.
fn render_static_markdown(report: &Value) -> String {
    let replay = &report["replay"];
    let hz = replay["tick_hz"].as_u64().unwrap_or(1).max(1);
    let mut out = String::new();
    out.push_str(&format!(
        "# Replay (static): {} — seed {}\n\n",
        replay["map_name"].as_str().unwrap_or("?"),
        replay["seed"].as_u64().unwrap_or(0)
    ));
    out.push_str(&format!(
        "- {} ticks, {:.1} s at {} Hz\n",
        replay["recorded_ticks"].as_u64().unwrap_or(0),
        replay["recorded_seconds"].as_f64().unwrap_or(0.0),
        hz
    ));
    out.push_str(&format!(
        "- {} recorded state hashes\n- rules hash `{}`\n\n",
        replay["recorded_state_hashes"].as_u64().unwrap_or(0),
        replay["rules_hash"].as_str().unwrap_or("")
    ));
    out.push_str(&format!("> {}\n\n", report["note"].as_str().unwrap_or("")));
    out.push_str("## Recorded commands by kind\n\n| kind | count |\n|---|---|\n");
    if let Some(kinds) = report["command_kinds"].as_object() {
        let mut rows: Vec<_> = kinds.iter().collect();
        rows.sort_by_key(|(_, count)| std::cmp::Reverse(count.as_u64().unwrap_or(0)));
        for (kind, count) in rows {
            out.push_str(&format!("| {} | {} |\n", kind, count.as_u64().unwrap_or(0)));
        }
    }
    out.push_str("\n## By owner\n\n| owner | commands |\n|---|---|\n");
    if let Some(owners) = report["commands_by_owner"].as_object() {
        for (owner, count) in owners {
            out.push_str(&format!("| {} | {} |\n", owner, count.as_u64().unwrap_or(0)));
        }
    }
    out
}

// ---------------------------------------------------------------------------
// Arguments
// ---------------------------------------------------------------------------

fn parse_args() -> Result<Args, String> {
    let mut ra2_dir: Option<PathBuf> = None;
    let mut replay: Option<PathBuf> = None;
    let mut map: Option<String> = None;
    let mut out: Option<PathBuf> = None;
    let mut house: Option<String> = None;
    let mut sample_seconds = DEFAULT_SAMPLE_SECONDS;
    let mut max_ticks: Option<usize> = None;
    let mut max_order_rows = DEFAULT_MAX_ORDER_ROWS;
    let mut dump_type_table: Option<PathBuf> = None;
    let mut allow_rules_mismatch = false;
    let mut verify_only = false;
    let mut quiet = false;

    let mut args = std::env::args_os().skip(1);
    while let Some(arg) = args.next() {
        let flag = arg.to_string_lossy().to_string();
        let mut value = || -> Result<String, String> {
            args.next()
                .map(|value| value.to_string_lossy().to_string())
                .ok_or_else(|| format!("{flag} needs a value"))
        };
        match flag.as_str() {
            "--ra2-dir" => ra2_dir = Some(PathBuf::from(value()?)),
            "--replay" => replay = Some(PathBuf::from(value()?)),
            "--map" => map = Some(value()?),
            "--out" => out = Some(PathBuf::from(value()?)),
            "--house" => house = Some(value()?),
            "--sample-seconds" => {
                sample_seconds = value()?
                    .parse()
                    .map_err(|_| "--sample-seconds is a number".to_string())?
            }
            "--max-orders" => {
                max_order_rows = value()?
                    .parse()
                    .map_err(|_| "--max-orders is a number".to_string())?
            }
            "--max-ticks" => {
                max_ticks = Some(
                    value()?
                        .parse()
                        .map_err(|_| "--max-ticks is a number".to_string())?,
                )
            }
            "--dump-type-table" => dump_type_table = Some(PathBuf::from(value()?)),
            "--allow-rules-mismatch" => allow_rules_mismatch = true,
            "--verify-only" => verify_only = true,
            "--quiet" => quiet = true,
            "--help" | "-h" => return Err(String::new()),
            other => return Err(format!("unknown flag {other}")),
        }
    }

    let ra2_dir = ra2_dir
        .or_else(|| std::env::var_os("RA2_DIR").map(PathBuf::from))
        .ok_or_else(|| "the retail install: --ra2-dir <dir> or RA2_DIR".to_owned())?;
    let replay = replay.ok_or_else(|| "the replay: --replay <file.json>".to_owned())?;
    if !replay.is_file() {
        return Err(format!("{} is not a file", replay.display()));
    }
    if !ra2_dir.is_dir() {
        return Err(format!("{} is not a directory", ra2_dir.display()));
    }
    if !sample_seconds.is_finite() || sample_seconds <= 0.0 {
        return Err("--sample-seconds is a positive number of seconds".to_owned());
    }

    Ok(Args {
        ra2_dir,
        replay,
        map,
        out,
        house,
        sample_seconds,
        max_ticks,
        max_order_rows,
        dump_type_table,
        allow_rules_mismatch,
        verify_only,
        quiet,
    })
}

/// The recorded map: an explicit `--map`, else the file beside the replay or
/// one folder up (a `-SPAWN` game keeps its map beside `spawn.ini`), else the
/// header's own name resolved through the retail install.
fn resolve_map(args: &Args, map_name: &str, map_sha256: &str) -> Result<String, String> {
    if let Some(map) = &args.map {
        return Ok(map.clone());
    }
    // A logical name with a directory has to keep resolving through the game
    // folder; only a bare file name can sit beside the replay.
    if !map_name.contains(['/', '\\']) {
        if let Ok(canonical) = args.replay.canonicalize() {
            for directory in canonical.ancestors().skip(1).take(2) {
                let candidate = directory.join(map_name);
                let Ok(bytes) = std::fs::read(&candidate) else {
                    continue;
                };
                if vera20k_core::util::sha256::sha256_hex(&bytes) == map_sha256 {
                    return Ok(candidate.to_string_lossy().to_string());
                }
            }
        }
        // VERA's own Maps folder, where a generated or received map lives and
        // where the retail install never looks: the same place the game found
        // it when it played the match. Only a byte-identical file is taken.
        if let Ok(settings) = vera20k_core::util::config::default_settings_dir() {
            let candidate = settings.join("Maps").join(map_name);
            if let Ok(bytes) = std::fs::read(&candidate) {
                if vera20k_core::util::sha256::sha256_hex(&bytes) == map_sha256 {
                    return Ok(candidate.to_string_lossy().to_string());
                }
            }
        }
    }
    Ok(map_name.to_owned())
}

/// The rules authority the game itself would build on this machine: the retail
/// sources with the two Options switches the app folds into it before the first
/// scenario (`app::initialize`).
///
/// This is what makes a replay from the *installed* game replayable: the
/// recorded `rules_hash` covers those layers, so a reader without them is
/// reading a different game and cannot check a single recorded hash.
fn load_rules_owner(ra2_dir: &Path) -> Result<NativeRulesProcessOwner, String> {
    let assets = AssetManager::new(ra2_dir, MediaArchiveMode::STOCK_DIGITAL)
        .map_err(|error| format!("open retail MIX archives in {}: {error}", ra2_dir.display()))?;
    // The audio definitions are a rules input, not decoration: the native Rules
    // pass binds type audio references, and a reader with empty registries
    // processes different rules than the game did.
    let audio = AudioDefinitions::select(&assets);
    let sources = RetailRulesSources::select(&assets)
        .map_err(|error| format!("select the retail rules: {error}"))?
        .with_personal_rules(personal_rules_from_settings());
    sources
        .into_owner(audio.sounds().clone(), audio.eva().clone())
        .map_err(|error| format!("build the rules authority: {error}"))
}

/// The two switches the Options page stores in `VERA.INI`, with the app's own
/// defaults when it is absent.
fn personal_rules_from_settings() -> PersonalRules {
    let Ok(dir) = vera20k_core::util::config::default_settings_dir() else {
        return PersonalRules::default();
    };
    // VERARULES.INI was retired for New Mod; the two switches a player can
    // still set live in VERA.INI, which is the only layer left to read.
    let mut rules = PersonalRules::default();
    if let Some(path) = find_ini(&dir, "VERA.INI")
        && let Ok(bytes) = std::fs::read(&path)
        && let Ok(ini) = IniFile::from_bytes(&bytes)
        && let Some(vera) = ini.section("VERA")
    {
        rules.set_terrain_deformation(vera.read_bool("TerrainDeformation", rules.terrain_deformation()));
        let limit = vera.read_int("SuperweaponLimit", i32::from(rules.superweapon_limit()));
        rules.set_superweapon_limit(limit);
    }
    rules
}

/// The file `name` in `dir` in any capitalisation: the profile's own lookup
/// ignores case, and so does this.
fn find_ini(dir: &Path, name: &str) -> Option<PathBuf> {
    let exact = dir.join(name);
    if exact.is_file() {
        return Some(exact);
    }
    std::fs::read_dir(dir).ok()?.flatten().find_map(|entry| {
        let path = entry.path();
        (path.is_file() && entry.file_name().to_string_lossy().eq_ignore_ascii_case(name))
            .then_some(path)
    })
}

// ---------------------------------------------------------------------------
// The walk: commands, samples, and the per-house ledger
// ---------------------------------------------------------------------------

/// The function checklist a complete match exercises.
///
/// This is the functional half of the autoplay's acceptance: a function with no
/// rows is a gap, not a pass. The list is the union of what the reference
/// matches use (see `autoplay/docs/reference-1v29-win.md`), so a function the
/// reference never uses is deliberately absent.
const FUNCTION_CHECKLIST: &[(&str, &str)] = &[
    ("mcv:deploy", "展开基地车"),
    ("mcv:redeploy", "基地车再部署"),
    ("build:power", "电厂"),
    ("build:refinery", "矿厂"),
    ("build:barracks", "兵营"),
    ("build:factory", "战车工厂"),
    ("build:airfield", "机场"),
    ("build:battlelab", "作战实验室"),
    ("build:depot", "维修厂"),
    ("build:radar", "雷达"),
    ("build:defense", "防御建筑"),
    ("produce:infantry", "步兵"),
    ("produce:vehicle", "载具"),
    ("produce:aircraft", "空军"),
    ("unit:engineer", "工程师"),
    ("unit:spy", "间谍"),
    ("move", "移动"),
    ("attack", "攻击"),
    ("attack-move", "进攻"),
    ("guard", "警戒"),
    ("stop", "停止"),
    ("rally", "集结点"),
    ("capture", "占领/渗透"),
    ("repair", "维修"),
    ("sell", "卖建筑"),
    ("cancel", "取消生产"),
    ("superweapon", "超武"),
    ("defend", "防守（本方损失）"),
];

/// Which function a queued or placed building type belongs to.
fn building_function(name: &str) -> Option<&'static str> {
    Some(match name {
        "GAPOWR" | "NAPOWR" | "YAPOWR" => "build:power",
        "GAREFN" | "NAREFN" => "build:refinery",
        "GAPILE" | "NAHAND" | "YABRCK" => "build:barracks",
        "GAWEAP" | "NAWEAP" | "YAWEAP" => "build:factory",
        "AMRADR" | "NARADR" => "build:airfield",
        "GATECH" | "NATECH" | "YATECH" => "build:battlelab",
        "GADEPT" => "build:depot",
        "GASPYSAT" | "NAPSIS" => "build:radar",
        "GAVULC" | "GASAM" | "NASAM" | "NAFLAK" | "GTGCAN" | "NABNKR" => "build:defense",
        _ => return None,
    })
}

/// Per-function coverage: how often it fired and when.
#[derive(Default, Clone)]
struct FunctionCoverage {
    count: usize,
    first_tick: Option<u64>,
    last_tick: Option<u64>,
}

/// (rules category, type-array index) → type name, so a native production or
/// placement record can be named without the engine's private resolver.
///
/// The engine keeps those records raw on purpose — a category-only view loses
/// Naval and the `-1` wildcard — so the names come from the rules' own arrays:
/// `RuleSet::type_array_index` is the inverse of the array the record indexes.
struct ProductionNames {
    by_index: BTreeMap<(ObjectCategory, i32), String>,
    by_name: BTreeMap<String, ObjectCategory>,
}

impl ProductionNames {
    fn of(rules: &RuleSet) -> Self {
        let mut by_index = BTreeMap::new();
        let mut by_name = BTreeMap::new();
        for object in rules.all_objects() {
            if let Some(index) = rules.type_array_index(object.category, &object.id) {
                by_index.insert((object.category, index), object.id.clone());
                by_name.insert(object.id.clone(), object.category);
            }
        }
        Self { by_index, by_name }
    }

    /// The category a type belongs to, for classifying a queued type.
    fn category_of(&self, name: &str) -> Option<ObjectCategory> {
        self.by_name.get(name).copied()
    }

    /// The type a record's RTTI and type-array index name.
    ///
    /// Production records carry the production RTTI (7/16/40/3); a *placement*
    /// record carries the object RTTI the House receiver reads (6/15/1/2), and
    /// an automatic mobile placement passes `-1` for the index, which names the
    /// factory's current head rather than a type.
    fn type_name(&self, rtti: i32, type_index: i32) -> Option<String> {
        let category = match rtti {
            7 | 6 => ObjectCategory::Building,
            16 | 15 => ObjectCategory::Infantry,
            40 | 1 => ObjectCategory::Vehicle,
            3 | 2 => ObjectCategory::Aircraft,
            _ => return None,
        };
        self.by_index.get(&(category, type_index)).cloned()
    }
}

/// One house's identity, fixed before the first frame.
struct HouseMeta {
    owner: InternedId,
    name: String,
    side_index: i32,
    difficulty: String,
    human: bool,
    passive: bool,
}

/// Per-type history read off the focus house's sampled rosters: the first tick
/// a type was seen, and how many copies appeared and vanished between samples.
///
/// This is the build order the engine will not hand over directly: a build
/// records its production and placement clicks as native records, which stay
/// raw on purpose (a category-only view loses Naval and the wildcard), so the
/// sampled roster is where "what existed, and when" is actually readable.
#[derive(Default)]
struct RosterHistory {
    first_seen: BTreeMap<String, u64>,
    added: BTreeMap<String, i64>,
    removed: BTreeMap<String, i64>,
}

fn roster_history(rows: &[Row]) -> RosterHistory {
    let mut history = RosterHistory::default();
    let mut previous: BTreeMap<String, i64> = BTreeMap::new();
    for row in rows {
        let mut current: BTreeMap<String, i64> = BTreeMap::new();
        for (name, count) in &row.roster {
            current.insert(name.clone(), i64::from(*count));
            history.first_seen.entry(name.clone()).or_insert(row.tick);
        }
        for (name, count) in &current {
            let before = previous.get(name).copied().unwrap_or(0);
            if count > &before {
                *history.added.entry(name.clone()).or_insert(0) += count - before;
            }
        }
        for (name, count) in &previous {
            let after = current.get(name).copied().unwrap_or(0);
            if count > &after {
                *history.removed.entry(name.clone()).or_insert(0) += count - after;
            }
        }
        previous = current;
    }
    history
}

/// One timeline row for one house.
struct Row {
    tick: u64,
    credits: i32,
    spent: i32,
    power: i32,
    units: u32,
    infantry: u32,
    aircraft: u32,
    structures: u32,
    army_value: i32,
    structure_value: i32,
    base: Option<(u16, u16)>,
    /// Focus house only: live count per type.
    roster: BTreeMap<String, u32>,
    /// Focus house only: what each factory line is doing.
    production: Vec<ProductionRow>,
}

struct ProductionRow {
    category: &'static str,
    product: Option<String>,
    queued: usize,
}

/// One house's accumulated facts.
struct HouseFacts {
    meta: HouseMeta,
    rows: Vec<Row>,
    costs: BTreeMap<InternedId, i32>,
    first_defeat_tick: Option<u64>,
    peak_army_value: i32,
    peak_army_tick: u64,
    peak_credits: i32,
    start_credits: i32,
    start_roster: BTreeMap<String, u32>,
}

/// One exact-tick event. `detail` is rendered into both reports.
struct Event {
    tick: u64,
    kind: &'static str,
    house: String,
    detail: Value,
}

/// Everything the walk collected.
struct Walk {
    houses: Vec<HouseFacts>,
    by_owner: BTreeMap<InternedId, usize>,
    focus: Option<usize>,
    sample_ticks: u64,
    next_sample: u64,
    events: Vec<Event>,
    /// Order rows kept per (house, kind), so a click storm cannot fill a file.
    order_rows: BTreeMap<(usize, &'static str), usize>,
    /// Rows kept per (house, kind) before only the count grows.
    max_order_rows: usize,
    /// Names production and placement records by rules type-array index.
    production_names: ProductionNames,
    /// Per-function coverage over the whole walk.
    functions: BTreeMap<&'static str, FunctionCoverage>,
    /// The focus house's name, for classifying its losses as defending.
    focus_name: Option<String>,
    /// Placements that named the factory head rather than a type.
    untyped_places: usize,
    order_totals: BTreeMap<(usize, &'static str), usize>,
}

impl Walk {
    fn new(
        sim: &Simulation,
        rules: &RuleSet,
        focus_name: Option<&str>,
        sample_ticks: u64,
        max_order_rows: usize,
    ) -> Self {
        let mut houses = Vec::new();
        let mut by_owner = BTreeMap::new();
        for (owner, house) in sim.houses().iter() {
            let index = houses.len();
            by_owner.insert(*owner, index);
            let credits = house.economy().credits();
            houses.push(HouseFacts {
                meta: HouseMeta {
                    owner: *owner,
                    name: sim.interner().resolve(house.name()).to_owned(),
                    side_index: house.side_index(),
                    difficulty: format!("{:?}", house.difficulty()).to_lowercase(),
                    human: house.is_human(),
                    passive: house.multiplay_passive(),
                },
                rows: Vec::new(),
                costs: BTreeMap::new(),
                first_defeat_tick: None,
                peak_army_value: 0,
                peak_army_tick: 0,
                peak_credits: credits,
                start_credits: credits,
                start_roster: BTreeMap::new(),
            });
        }
        // The focus is the named house, else the first human one, else the
        // first house that is not a passive map owner.
        let focus = match focus_name {
            Some(name) => houses
                .iter()
                .position(|house| house.meta.name.eq_ignore_ascii_case(name)),
            None => houses
                .iter()
                .position(|house| house.meta.human)
                .or_else(|| houses.iter().position(|house| !house.meta.passive)),
        };
        Self {
            houses,
            by_owner,
            focus,
            sample_ticks,
            max_order_rows,
            production_names: ProductionNames::of(rules),
            functions: BTreeMap::new(),
            focus_name: focus_name.map(str::to_owned),
            untyped_places: 0,
            next_sample: 0,
            events: Vec::new(),
            order_rows: BTreeMap::new(),
            order_totals: BTreeMap::new(),
        }
    }

    fn house_name(&self, owner: InternedId) -> Option<&str> {
        self.by_owner
            .get(&owner)
            .map(|index| self.houses[*index].meta.name.as_str())
    }

    /// Record the frame's commands. These are the orders and production clicks
    /// themselves, so they carry the exact tick without any reconstruction.
    fn record_commands(&mut self, sim: &Simulation, entry: &ReplayTick) {
        for command in &entry.commands {
            let Some(house) = self.house_name(command.owner).map(str::to_owned) else {
                continue;
            };
            let Some(&owner_index) = self.by_owner.get(&command.owner) else {
                continue;
            };
            let tick = entry.tick;

            // Production and placement are recorded as native records and stay
            // raw on purpose (a category-only view loses Naval and the
            // wildcard). Name them from the rules' own type arrays, or the
            // report would have orders and no build order.
            if let Command::SynchronizedRecord { record } = &command.payload {
                if let Some(event) = ProductionRecord::decode(record) {
                    let name = self
                        .production_names
                        .type_name(event.rtti, event.type_index)
                        .unwrap_or_else(|| format!("#{}", event.type_index));
                    let (kind, detail) = match event.event {
                        ProductionEvent::Produce => ("queue", json!({ "type": name })),
                        ProductionEvent::Suspend => ("suspend", json!({ "type": name })),
                        ProductionEvent::Abandon => ("cancel", json!({ "type": name, "all": false })),
                        ProductionEvent::AbandonAll => ("cancel", json!({ "type": name, "all": true })),
                    };
                    self.push_event(Event { tick, kind, house, detail });
                    continue;
                }
                if let Some(event) = PlaceRecord::decode(record) {
                    // `-1` names the factory's current head, not a type: that is
                    // the production completion path, and its cell is the
                    // object's own. Count it, do not dress it up as a placement.
                    if event.type_index < 0 {
                        self.untyped_places += 1;
                        continue;
                    }
                    let name = self
                        .production_names
                        .type_name(event.rtti, event.type_index)
                        .unwrap_or_else(|| format!("#{}", event.type_index));
                    self.push_event(Event {
                        tick,
                        kind: "place",
                        house,
                        detail: json!({
                            "type": name,
                            "cell": [event.x, event.y],
                            "rtti": event.rtti,
                            "type_index": event.type_index,
                        }),
                    });
                    continue;
                }
                if let Some(event) = SpecialPlaceRecord::decode(record) {
                    self.push_event(Event {
                        tick,
                        kind: "super",
                        house,
                        detail: json!({ "type_index": event.type_index, "cell": [event.cell.0, event.cell.1] }),
                    });
                    continue;
                }
                if let Some(event) = vera20k_core::sim::command::PaidSuperRecord::decode(record) {
                    let action = match event.action {
                        PaidSuperAction::Begin => "begin",
                        PaidSuperAction::Complete => "complete",
                        PaidSuperAction::Cancel => "cancel",
                    };
                    self.push_event(Event {
                        tick,
                        kind: "paid-super",
                        house,
                        detail: json!({ "action": action, "cell": [event.cell.0, event.cell.1] }),
                    });
                    continue;
                }
            }

            // A build stores a local command as its native record; the typed
            // enum is a *view* of those bytes. Decode it here, or every replay
            // from this build would read as an empty command stream.
            let decoded = match &command.payload {
                Command::SynchronizedRecord { record } => {
                    sim.decode_native_command_record(record, entry.tick)
                }
                _ => None,
            };
            let payload = decoded.as_ref().map_or(&command.payload, |view| &view.payload);
            let match_result = self.match_payload(payload, house, owner_index, tick, sim);
            if match_result {
                continue;
            }
        }
    }

    /// The semantic match on a decoded payload. Returns whether the payload was
    /// handled (the raw records were already handled by the caller).
    fn match_payload(
        &mut self,
        payload: &Command,
        house: String,
        owner_index: usize,
        tick: u64,
        sim: &Simulation,
    ) -> bool {
        match payload {
                Command::QueueProduction { type_id } => {
                    self.push_event(Event {
                        tick,
                        kind: "queue",
                        house,
                        detail: json!({ "type": type_name(sim, *type_id) }),
                    });
                }
                Command::CancelProductionByType { type_id, all } => {
                    self.push_event(Event {
                        tick,
                        kind: "cancel",
                        house,
                        detail: json!({ "type": type_name(sim, *type_id), "all": all }),
                    });
                }
                Command::PlaceReadyBuilding { type_id, rx, ry } => {
                    self.push_event(Event {
                        tick,
                        kind: "place",
                        house,
                        detail: json!({ "type": type_name(sim, *type_id), "cell": [rx, ry] }),
                    });
                }
                Command::SellBuilding { entity_id } => {
                    let (entity, cell) = entity_facts(sim, *entity_id);
                    self.push_event(Event {
                        tick,
                        kind: "sell",
                        house,
                        detail: json!({ "type": entity, "cell": cell_json(cell) }),
                    });
                }
                Command::DeployMcv { entity_id } => {
                    let (entity, cell) = entity_facts(sim, *entity_id);
                    self.push_event(Event {
                        tick,
                        kind: "deploy",
                        house,
                        detail: json!({ "type": entity, "cell": cell_json(cell) }),
                    });
                }
                Command::UndeployBuilding { entity_id } => {
                    let (entity, cell) = entity_facts(sim, *entity_id);
                    self.push_event(Event {
                        tick,
                        kind: "undeploy",
                        house,
                        detail: json!({ "type": entity, "cell": cell_json(cell) }),
                    });
                }
                Command::CaptureBuilding {
                    engineer_id,
                    target_building_id,
                } => {
                    let (engineer, _) = entity_facts(sim, *engineer_id);
                    let (target, cell) = entity_facts(sim, *target_building_id);
                    self.push_event(Event {
                        tick,
                        kind: "capture",
                        house,
                        detail: json!({
                            "engineer": engineer,
                            "target": target,
                            "cell": cell_json(cell),
                        }),
                    });
                }
                Command::Attack {
                    attacker_id,
                    target_id,
                } => {
                    let (attacker, _) = entity_facts(sim, *attacker_id);
                    let (target, cell) = entity_facts(sim, *target_id);
                    self.push_order(
                        owner_index,
                        "attack",
                        Event {
                            tick,
                            kind: "attack",
                            house,
                            detail: json!({
                                "attacker": attacker,
                                "target": target,
                                "cell": cell_json(cell),
                            }),
                        },
                    );
                }
                Command::Move {
                    entity_id,
                    target_rx,
                    target_ry,
                } => {
                    self.push_order(
                        owner_index,
                        "move",
                        Event {
                            tick,
                            kind: "move",
                            house,
                            detail: json!({ "entity": entity_id, "to": [target_rx, target_ry] }),
                        },
                    );
                }
                Command::MoveToObject {
                    entity_id,
                    target_id,
                } => {
                    let (_, cell) = entity_facts(sim, *target_id);
                    self.push_order(
                        owner_index,
                        "move",
                        Event {
                            tick,
                            kind: "move",
                            house,
                            detail: json!({
                                "entity": entity_id,
                                "onto": target_id,
                                "cell": cell_json(cell),
                            }),
                        },
                    );
                }
                Command::AttackMove {
                    entity_id,
                    target,
                    destination,
                } => {
                    self.push_order(
                        owner_index,
                        "attack-move",
                        Event {
                            tick,
                            kind: "attack-move",
                            house,
                            detail: json!({
                                "entity": entity_id,
                                "target": target.map(target_label),
                                "destination": destination.map(target_label),
                            }),
                        },
                    );
                }
                Command::Guard { entity_id, target } => {
                    self.push_order(
                        owner_index,
                        "guard",
                        Event {
                            tick,
                            kind: "guard",
                            house,
                            detail: json!({
                                "entity": entity_id,
                                "target": target.map(target_label),
                            }),
                        },
                    );
                }
                Command::Stop { entity_id } => {
                    self.push_order(
                        owner_index,
                        "stop",
                        Event {
                            tick,
                            kind: "stop",
                            house,
                            detail: json!({ "entity": entity_id }),
                        },
                    );
                }
                Command::SetRally {
                    rx,
                    ry,
                    producer_ids,
                } => {
                    self.push_order(
                        owner_index,
                        "rally",
                        Event {
                            tick,
                            kind: "rally",
                            house,
                            detail: json!({ "cell": [rx, ry], "producers": producer_ids.len() }),
                        },
                    );
                }
                // Selection and the opaque planning records carry no decision
                // this report can name; counting them would bury the rest.
                Command::Select { .. } | Command::PlanningRecord { .. } => return false,
                _ => return false,
        }
        true
    }

    /// Write the (category, type-array index) → type name table.
    ///
    /// A native production or placement record names a type by its rules
    /// category and its index in that category's array. The engine can resolve
    /// that only from inside a live scenario, so a replay this build cannot
    /// load (a launch beyond the slot cap, for one) can still be read if the
    /// table is written out from a build that can.
    fn dump_type_table(&self, path: &Path) -> Result<(), String> {
        let mut table: BTreeMap<String, BTreeMap<i32, String>> = BTreeMap::new();
        for ((category, index), name) in &self.production_names.by_index {
            let key = match category {
                ObjectCategory::Building => "building",
                ObjectCategory::Infantry => "infantry",
                ObjectCategory::Vehicle => "vehicle",
                ObjectCategory::Aircraft => "aircraft",
            };
            table.entry(key.to_owned()).or_default().insert(*index, name.clone());
        }
        let text = serde_json::to_string_pretty(&table)
            .map_err(|error| format!("encode the type table: {error}"))?;
        std::fs::write(path, format!("{text}\n"))
            .map_err(|error| format!("write {}: {error}", path.display()))?;
        Ok(())
    }

    /// One frame ran. Check every house's defeat flag and sample on schedule.
    fn after_frame(&mut self, sim: &Simulation, rules: &RuleSet, tick: u64) {
        for facts in &mut self.houses {
            if facts.first_defeat_tick.is_none()
                && sim
                    .houses()
                    .get(&facts.meta.owner)
                    .is_some_and(|house| house.is_defeated())
            {
                facts.first_defeat_tick = Some(tick);
            }
        }
        if tick < self.next_sample {
            return;
        }
        self.sample(sim, rules, tick);
    }

    /// Take one timeline row per house. The focus house also gets its roster
    /// and its factory lines; the others get counts and money.
    fn sample(&mut self, sim: &Simulation, rules: &RuleSet, tick: u64) {
        // One pass over the entity store, bucketed per house.
        let mut counts: Vec<(u32, u32, u32, u32)> = vec![(0, 0, 0, 0); self.houses.len()];
        let mut values: Vec<(i32, i32)> = vec![(0, 0); self.houses.len()];
        let mut rosters: Vec<BTreeMap<String, u32>> = vec![BTreeMap::new(); self.houses.len()];
        let focus = self.focus;
        for entity in sim.entities().values() {
            let Some(&index) = self.by_owner.get(&entity.owner()) else {
                continue;
            };
            if entity.lifecycle.in_limbo() || entity.health.current() <= 0 {
                continue;
            }
            let category = entity.category();
            let row = &mut counts[index];
            match category {
                EntityCategory::Unit => row.0 += 1,
                EntityCategory::Infantry => row.1 += 1,
                EntityCategory::Aircraft => row.2 += 1,
                EntityCategory::Structure => row.3 += 1,
            }
            let cost = self.cost_of(sim, rules, index, entity.type_ref());
            let value = &mut values[index];
            if category == EntityCategory::Structure {
                value.1 += cost;
            } else {
                value.0 += cost;
            }
            if focus == Some(index) {
                *rosters[index]
                    .entry(sim.interner().resolve(entity.type_ref()).to_owned())
                    .or_insert(0) += 1;
            }
        }

        for (index, facts) in self.houses.iter_mut().enumerate() {
            let Some(house) = sim.houses().get(&facts.meta.owner) else {
                continue;
            };
            let economy = house.economy();
            let credits = economy.credits();
            let spent = economy.spent_credits();
            let (units, infantry, aircraft, structures) = counts[index];
            let (army_value, structure_value) = values[index];
            let base = house.base_center();
            facts.peak_credits = facts.peak_credits.max(credits);
            if army_value > facts.peak_army_value {
                facts.peak_army_value = army_value;
                facts.peak_army_tick = tick;
            }
            let (roster, production) = if focus == Some(index) {
                let obs = Observation::new(sim, facts.meta.owner);
                let mut production = Vec::new();
                for (category, label) in PRODUCTION_LINES {
                    if let Some(line) = obs.factory_line(*category) {
                        if line.product.is_some() || line.queued > 0 {
                            production.push(ProductionRow {
                                category: label,
                                product: line.product.map(|product| type_name(sim, product)),
                                queued: line.queued,
                            });
                        }
                    }
                }
                (std::mem::take(&mut rosters[index]), production)
            } else {
                (BTreeMap::new(), Vec::new())
            };
            facts.rows.push(Row {
                tick,
                credits,
                spent,
                power: Observation::new(sim, facts.meta.owner).power_balance(),
                units,
                infantry,
                aircraft,
                structures,
                army_value,
                structure_value,
                base,
                roster,
                production,
            });
        }

        self.next_sample = tick.saturating_add(self.sample_ticks);
    }

    fn cost_of(
        &mut self,
        sim: &Simulation,
        rules: &RuleSet,
        index: usize,
        type_ref: InternedId,
    ) -> i32 {
        if let Some(cost) = self.houses[index].costs.get(&type_ref) {
            return *cost;
        }
        let owner = self.houses[index].meta.owner;
        let cost = sim
            .object_type(type_ref, rules)
            .map_or(0, |object| sim.cost_of(owner, object, rules));
        self.houses[index].costs.insert(type_ref, cost);
        cost
    }

    fn push_event(&mut self, event: Event) {
        self.record(&event);
        self.events.push(event);
    }

    /// Count one event against the function checklist, whatever path it took.
    fn record(&mut self, event: &Event) {
        if let Some(function) = self.classify(event) {
            let entry = self.functions.entry(function).or_default();
            entry.count += 1;
            entry.first_tick.get_or_insert(event.tick);
            entry.last_tick = Some(event.tick);
        }
    }

    /// Which checklist function an event belongs to, if any.
    fn classify(&self, event: &Event) -> Option<&'static str> {
        let name = event.detail.get("type").and_then(Value::as_str);
        match event.kind {
            "deploy" => Some("mcv:deploy"),
            "undeploy" => Some("mcv:redeploy"),
            "place" => name.and_then(building_function),
            "queue" => {
                let name = name?;
                match name {
                    "SPY" => Some("unit:spy"),
                    "ENGINEER" => Some("unit:engineer"),
                    _ => match self.production_names.category_of(name) {
                        Some(ObjectCategory::Infantry) => Some("produce:infantry"),
                        Some(ObjectCategory::Vehicle) => Some("produce:vehicle"),
                        Some(ObjectCategory::Aircraft) => Some("produce:aircraft"),
                        _ => building_function(name),
                    },
                }
            }
            "sell" => Some("sell"),
            "cancel" => Some("cancel"),
            "super" | "paid-super" => Some("superweapon"),
            "attack" => Some("attack"),
            "attack-move" => Some("attack-move"),
            "move" => Some("move"),
            "guard" => Some("guard"),
            "stop" => Some("stop"),
            "rally" => Some("rally"),
            "capture" => Some("capture"),
            "repair" => Some("repair"),
            "death" if self.focus_name.as_deref() == Some(event.house.as_str()) => {
                Some("defend")
            }
            _ => None,
        }
    }

    /// Keep at most `max_order_rows` rows per (house, kind) and count the
    /// rest, so an order-heavy match reports its shape without a wall of rows.
    fn push_order(&mut self, owner: usize, kind: &'static str, event: Event) {
        let key = (owner, kind);
        *self.order_totals.entry(key).or_insert(0) += 1;
        // Orders are counted even when the rows are dropped, or a busy match
        // would look like it never moved.
        self.record(&event);
        let kept = self.order_rows.entry(key).or_insert(0);
        if *kept < self.max_order_rows {
            *kept += 1;
            self.events.push(event);
        }
    }
}

const PRODUCTION_LINES: &[(ProductionCategory, &str)] = &[
    (ProductionCategory::Building, "building"),
    (ProductionCategory::Defense, "defense"),
    (ProductionCategory::Infantry, "infantry"),
    (ProductionCategory::Vehicle, "vehicle"),
    (ProductionCategory::Aircraft, "aircraft"),
    (ProductionCategory::Ship, "ship"),
];

fn type_name(sim: &Simulation, type_id: InternedId) -> String {
    sim.interner().resolve(type_id).to_owned()
}

/// The type and cell of an entity, for a command that names one by id.
fn entity_facts(sim: &Simulation, entity_id: u64) -> (Option<String>, Option<(u16, u16)>) {
    let Some(entity) = sim.entities().get(entity_id) else {
        return (None, None);
    };
    (
        Some(type_name(sim, entity.type_ref())),
        Some((entity.position.rx(), entity.position.ry())),
    )
}

fn cell_json(cell: Option<(u16, u16)>) -> Value {
    cell.map_or(Value::Null, |(rx, ry)| json!([rx, ry]))
}

fn target_label(target: TargetKind) -> Value {
    match target {
        TargetKind::Entity(id) => json!({ "entity": id }),
        TargetKind::Cell(rx, ry) => json!({ "cell": [rx, ry] }),
    }
}

fn clock(tick: u64, tick_hz: u32) -> String {
    let total = tick as f64 / f64::from(tick_hz.max(1));
    let minutes = (total / 60.0) as u64;
    let seconds = total - minutes as f64 * 60.0;
    format!("{minutes}:{seconds:05.2}")
}

/// Ticks between samples, from the requested seconds and the replay's rate.
fn sample_ticks_for(seconds: f64, tick_hz: u32) -> u64 {
    let ticks = (seconds * f64::from(tick_hz.max(1))).round() as u64;
    ticks.max(1)
}

// ---------------------------------------------------------------------------
// The report
// ---------------------------------------------------------------------------

/// Everything the report says, assembled once so the JSON and the page cannot
/// disagree.
#[allow(clippy::too_many_arguments)]
fn build_report(
    args: &Args,
    replay: &ReplayLog,
    file_sha256: &str,
    file_bytes: usize,
    map_file: &str,
    map_sha256: String,
    map_hash: u64,
    seed: u32,
    tick_hz: u32,
    session: &vera20k_core::skirmish_launch::SkirmishLaunchSession,
    verification: Option<&vera20k_core::sim::replay::ReplayVerification>,
    sim: &Simulation,
    mut walk: Walk,
    walked: usize,
    first_tick: u64,
    last_tick: u64,
    recorded_hashes: usize,
    rules_hash_match: bool,
    current_rules_hash: u64,
    stopped: Option<String>,
) -> Value {
    // Events: the walk's own, plus the death rows the houses kept. Sorted by
    // tick so one reader sees one sequence.
    let mut events = std::mem::take(&mut walk.events);
    for facts in &walk.houses {
        let Some(house) = sim.houses().get(&facts.meta.owner) else {
            continue;
        };
        for death in house.tracking().deaths() {
            events.push(Event {
                tick: death.tick,
                kind: "death",
                house: facts.meta.name.clone(),
                detail: json!({
                    "victim": type_name(sim, death.victim_type),
                    "killer": death.killer_type.map(|killer| type_name(sim, killer)),
                    "mission": mission_label(death.mission),
                    "health_permille": death.health_permille,
                }),
            });
        }
    }

    // Deaths are added at report time, so the walk never saw them: count the
    // focus house's losses as defending here, or the table says it never
    // defended even when it lost 300 objects doing so.
    let focus_name = walk.focus.map(|index| walk.houses[index].meta.name.clone());
    let losses: Vec<u64> = events
        .iter()
        .filter(|event| event.kind == "death" && Some(&event.house) == focus_name.as_ref())
        .map(|event| event.tick)
        .collect();
    if !losses.is_empty() {
        let entry = walk.functions.entry("defend").or_default();
        entry.count = losses.len();
        entry.first_tick = losses.iter().copied().min();
        entry.last_tick = losses.iter().copied().max();
    }
    events.sort_by_key(|event| event.tick);

    let focus_index = walk.focus;
    let houses: Vec<Value> = walk
        .houses
        .iter()
        .enumerate()
        .map(|(index, facts)| {
            let identity = json!({
                "name": facts.meta.name,
                "side_index": facts.meta.side_index,
                "difficulty": facts.meta.difficulty,
                "human": facts.meta.human,
                "passive": facts.meta.passive,
                "start_credits": facts.start_credits,
                "start_roster": object_of(&facts.start_roster),
            });
            let Some(house) = sim.houses().get(&facts.meta.owner) else {
                return json!({ "identity": identity });
            };
            let tracking = house.tracking();
            let obs = Observation::new(sim, facts.meta.owner);
            let final_row = facts.rows.last();
            let mut pairs: Vec<(i32, String, Option<String>)> = tracking
                .deaths_by_pair()
                .iter()
                .map(|(&(victim, killer), &count)| {
                    (
                        count,
                        type_name(sim, victim),
                        killer.map(|killer| type_name(sim, killer)),
                    )
                })
                .collect();
            pairs.sort_by(|left, right| right.0.cmp(&left.0));
            let pairs: Vec<Value> = pairs
                .into_iter()
                .map(|(count, victim, killer)| {
                    json!({ "count": count, "victim": victim, "killer": killer })
                })
                .collect();
            let mut by_mission: BTreeMap<String, i32> = BTreeMap::new();
            let mut one_shot = 0;
            let mut worn_down = 0;
            for death in tracking.deaths() {
                *by_mission.entry(mission_label(death.mission)).or_insert(0) += 1;
                if death.health_permille >= 1000 {
                    one_shot += 1;
                } else {
                    worn_down += 1;
                }
            }
            let final_counts = final_row.map_or(Value::Null, |row| {
                json!({
                    "units": row.units,
                    "infantry": row.infantry,
                    "aircraft": row.aircraft,
                    "structures": row.structures,
                })
            });
            let history = if focus_index == Some(index) {
                roster_history(&facts.rows)
            } else {
                RosterHistory::default()
            };
            json!({
                "identity": identity,
                "roster_history": {
                    "first_seen": object_of(&history.first_seen),
                    "added": object_of(&history.added),
                    "removed": object_of(&history.removed),
                },
                "economy": {
                    "final_credits": final_row.map_or(0, |row| row.credits),
                    "peak_credits": facts.peak_credits,
                    "earned": obs.earned(),
                    "spent": final_row.map_or(0, |row| row.spent),
                    "score": house.economy().score(),
                },
                "military": {
                    "peak_army_value": facts.peak_army_value,
                    "peak_army_tick": facts.peak_army_tick,
                    "final_army_value": final_row.map_or(0, |row| row.army_value),
                    "final_counts": final_counts,
                    "structure_value": final_row.map_or(0, |row| row.structure_value),
                },
                "losses": {
                    "by_pair": pairs,
                    "detail_rows": tracking.deaths().len(),
                    "by_mission": object_of(&by_mission),
                    "at_full_strength": one_shot,
                    "worn_down": worn_down,
                },
                "result": {
                    "defeated": house.is_defeated(),
                    "defeat_tick": facts.first_defeat_tick,
                },
            })
        })
        .collect();

    let timeline: Vec<Value> = walk
        .houses
        .iter()
        .map(|facts| {
            let rows: Vec<Value> = facts
                .rows
                .iter()
                .map(|row| {
                    json!({
                        "tick": row.tick,
                        "credits": row.credits,
                        "spent": row.spent,
                        "army_value": row.army_value,
                        "structure_value": row.structure_value,
                        "structures": row.structures,
                        "units": row.units,
                        "infantry": row.infantry,
                        "aircraft": row.aircraft,
                        "power": row.power,
                        "base": cell_json(row.base),
                    })
                })
                .collect();
            json!({ "house": facts.meta.name, "rows": rows })
        })
        .collect();

    let focus = walk.focus.map(|index| {
        let facts = &walk.houses[index];
        let rows: Vec<Value> = facts
            .rows
            .iter()
            .map(|row| {
                json!({
                    "tick": row.tick,
                    "credits": row.credits,
                    "spent": row.spent,
                    "army_value": row.army_value,
                    "structure_value": row.structure_value,
                    "counts": {
                        "units": row.units,
                        "infantry": row.infantry,
                        "aircraft": row.aircraft,
                        "structures": row.structures,
                    },
                    "base": cell_json(row.base),
                    "roster": object_of(&row.roster),
                    "production": row.production.iter().map(|line| json!({
                        "category": line.category,
                        "product": line.product,
                        "queued": line.queued,
                    })).collect::<Vec<_>>(),
                })
            })
            .collect();
        json!({
            "house": facts.meta.name,
            "sample_ticks": walk.sample_ticks,
            "rows": rows,
        })
    });

    let event_json: Vec<Value> = events
        .iter()
        .map(|event| {
            json!({
                "tick": event.tick,
                "time": clock(event.tick, tick_hz),
                "kind": event.kind,
                "house": event.house,
                "detail": event.detail,
            })
        })
        .collect();

    let order_totals: BTreeMap<String, usize> = walk
        .order_totals
        .iter()
        .map(|((index, kind), total)| (format!("{}:{kind}", walk.houses[*index].meta.name), *total))
        .collect();

    let focus_name = focus
        .as_ref()
        .and_then(|focus| focus.get("house").and_then(Value::as_str))
        .map(str::to_owned);

    let defeated: Vec<Value> = walk
        .houses
        .iter()
        .filter_map(|facts| {
            facts
                .first_defeat_tick
                .map(|tick| json!({ "house": facts.meta.name, "tick": tick }))
        })
        .collect();

    json!({
        "tool": { "name": "autoplay-replay", "version": TOOL_VERSION, "schema": REPORT_SCHEMA },
        "replay": {
            "path": args.replay.to_string_lossy(),
            "file_sha256": file_sha256,
            "file_bytes": file_bytes,
            "format_version": replay.header.version,
            "tick_hz": tick_hz,
            "seed": replay.header.seed,
            "map_name": replay.header.map_name,
            "rules_hash": format!("{:#018x}", replay.header.rules_hash),
            "recorded_ticks": replay.ticks.len(),
            "walked_ticks": walked,
            "first_tick": first_tick,
            "last_tick": last_tick,
            "recorded_seconds": last_tick as f64 / f64::from(tick_hz.max(1)),
            "recorded_state_hashes": recorded_hashes,
            "rules_hash_match": rules_hash_match,
            "rules_hash_this_build": format!("{current_rules_hash:#018x}"),
            "map": {
                "resolved": map_file,
                "sha256": map_sha256,
                "content_hash": format!("{:#018x}", map_hash),
            },
        },
        "playback": {
            "ok": verification.is_some(),
            "verified": verification.map(|verification| json!({
                "ticks": verification.ticks,
                "hashes_checked": verification.checked_hashes,
                "final_tick": verification.final_tick,
                "final_state_hash": format!("{:#018x}", verification.final_state_hash),
                "recorded_final_hash": replay.ticks.last().and_then(|tick| tick.state_hash).map(|hash| format!("{hash:#018x}")),
            })),
            "walked_ticks": walked,
            "complete": walked >= replay.ticks.len(),
            "stopped": stopped,
        },
        "setup": {
            "game_mode": format!("{:?}", session.mode),
            "seed": seed,
            "local_player_index": session.local_player_index,
            "session": serde_json::to_value(session).unwrap_or(Value::Null),
        },
        "summary": {
            "tick_hz": tick_hz,
            "duration_seconds": last_tick as f64 / f64::from(tick_hz.max(1)),
            "sample_ticks": walk.sample_ticks,
            "focus_house": focus_name,
            "houses": walk.houses.iter().map(|facts| facts.meta.name.clone()).collect::<Vec<_>>(),
            "humans": session.humans.len(),
            "defeated": defeated,
            "order_totals": order_totals,
        },
        "houses": houses,
        "functions": {
            "checklist": function_checklist(&walk),
            "missing": FUNCTION_CHECKLIST
                .iter()
                .filter(|(name, _)| !walk.functions.contains_key(*name))
                .map(|(name, _)| String::from(*name))
                .collect::<Vec<_>>(),
        },
        "timeline": { "sample_ticks": walk.sample_ticks, "houses": timeline },
        "focus": focus,
        "events": event_json,
    })
}

/// A `BTreeMap` as a JSON object, keeping the map's own order.
///
/// `Into<Value>` rather than `Serialize` so the tool needs no serde
/// dependency of its own: every value it records is a count or a sum.
fn object_of<K: ToString, V: Copy + Into<Value>>(map: &BTreeMap<K, V>) -> Value {
    let mut object = serde_json::Map::new();
    for (key, value) in map {
        object.insert(key.to_string(), (*value).into());
    }
    Value::Object(object)
}

fn mission_label(mission: MissionId) -> String {
    mission.known().map_or_else(
        || format!("MissionId({})", mission.raw()),
        |known| format!("{known:?}"),
    )
}

// ---------------------------------------------------------------------------
// The page a reader skims
// ---------------------------------------------------------------------------

fn render_markdown(report: &Value) -> String {
    let mut out = String::new();
    let replay = &report["replay"];
    let summary = &report["summary"];
    let hz = summary["tick_hz"].as_u64().unwrap_or(1).max(1);
    let duration = summary["duration_seconds"].as_f64().unwrap_or(0.0);
    let focus = summary["focus_house"].as_str().unwrap_or("(none)");

    out.push_str(&format!(
        "# Replay report: {} (seed {})\n\n",
        replay["map_name"].as_str().unwrap_or("?"),
        replay["seed"].as_u64().unwrap_or(0)
    ));
    // The functional half of the report: which checklist functions fired.
    let functions = &report["functions"]["checklist"];
    if let Some(rows) = functions.as_array() {
        let covered = rows.iter().filter(|row| row["count"].as_u64().unwrap_or(0) > 0).count();
        out.push_str(&format!(
            "## Function coverage: {}/{} covered\n\n",
            covered,
            rows.len()
        ));
        out.push_str("| function | what | count | first |\n|---|---|---|---|\n");
        for row in rows {
            let count = row["count"].as_u64().unwrap_or(0);
            let first = row["first_tick"].as_u64();
            out.push_str(&format!(
                "| `{}` | {} | {} | {} |\n",
                row["function"].as_str().unwrap_or("?"),
                row["label"].as_str().unwrap_or(""),
                if count == 0 { "**0**".to_owned() } else { count.to_string() },
                first.map_or_else(|| "—".to_owned(), |tick| tick.to_string())
            ));
        }
        out.push('\n');
    }
    out.push_str(&format!(
        "- File: `{}` — {} ticks, {duration:.1} s at {hz} Hz{}\n",
        replay["path"].as_str().unwrap_or("?"),
        replay["recorded_ticks"].as_u64().unwrap_or(0),
        if replay["walked_ticks"]
            .as_u64()
            .unwrap_or(0)
            < replay["recorded_ticks"].as_u64().unwrap_or(0)
        {
            format!(
                " (walked {} — partial)",
                replay["walked_ticks"].as_u64().unwrap_or(0)
            )
        } else {
            String::new()
        }
    ));
    out.push_str(&format!(
        "- Map: `{}` (sha256 `{}`)\n",
        replay["map"]["resolved"].as_str().unwrap_or("?"),
        short(replay["map"]["sha256"].as_str().unwrap_or(""), 16)
    ));
    if let Some(verified) = report["playback"]["verified"].as_object() {
        out.push_str(&format!(
            "- Playback: {} ticks, {} recorded hashes checked, all matched (final `{}`)\n",
            verified.get("ticks").and_then(Value::as_u64).unwrap_or(0),
            verified
                .get("hashes_checked")
                .and_then(Value::as_u64)
                .unwrap_or(0),
            verified
                .get("final_state_hash")
                .and_then(Value::as_str)
                .unwrap_or("?")
        ));
    } else {
        out.push_str("- Playback: did not reach the end, so the recorded hashes are unverified\n");
    }
    let defeated: Vec<String> = summary["defeated"]
        .as_array()
        .map(|rows| {
            rows.iter()
                .map(|row| {
                    format!(
                        "{} @ {}",
                        row["house"].as_str().unwrap_or("?"),
                        row["tick"].as_u64().unwrap_or(0)
                    )
                })
                .collect()
        })
        .unwrap_or_default();
    if !defeated.is_empty() {
        out.push_str(&format!("- Defeated: {}\n", defeated.join(", ")));
    }
    out.push_str(&format!(
        "- Houses: {} ({} human)\n\n",
        summary["houses"].as_array().map_or(0, Vec::len),
        summary["humans"].as_u64().unwrap_or(0)
    ));

    if let Some(house) = report["houses"].as_array().and_then(|houses| {
        houses
            .iter()
            .find(|house| house["identity"]["name"].as_str() == Some(focus))
    }) {
        let identity = &house["identity"];
        let economy = &house["economy"];
        let military = &house["military"];
        out.push_str(&format!(
            "## {} ({})\n\n",
            identity["name"].as_str().unwrap_or(focus),
            if identity["human"].as_bool().unwrap_or(false) {
                "human"
            } else {
                identity["difficulty"].as_str().unwrap_or("computer")
            }
        ));
        out.push_str(&format!(
            "- Credits: start {} → final {}; earned {}; spent {}; score {}\n",
            identity["start_credits"].as_i64().unwrap_or(0),
            economy["final_credits"].as_i64().unwrap_or(0),
            economy["earned"].as_i64().unwrap_or(0),
            economy["spent"].as_i64().unwrap_or(0),
            economy["score"].as_i64().unwrap_or(0)
        ));
        out.push_str(&format!(
            "- Army: peak {} at tick {} ({:.1} s), final {}; buildings worth {}\n",
            military["peak_army_value"].as_i64().unwrap_or(0),
            military["peak_army_tick"].as_u64().unwrap_or(0),
            military["peak_army_tick"].as_u64().unwrap_or(0) as f64 / hz as f64,
            military["final_army_value"].as_i64().unwrap_or(0),
            military["structure_value"].as_i64().unwrap_or(0)
        ));
        let final_counts = &military["final_counts"];
        out.push_str(&format!(
            "- Final: {} units, {} infantry, {} aircraft, {} structures\n",
            final_counts["units"].as_u64().unwrap_or(0),
            final_counts["infantry"].as_u64().unwrap_or(0),
            final_counts["aircraft"].as_u64().unwrap_or(0),
            final_counts["structures"].as_u64().unwrap_or(0)
        ));
        let losses = &house["losses"];
        let pairs: Vec<String> = losses["by_pair"]
            .as_array()
            .map(|rows| {
                rows.iter()
                    .take(8)
                    .map(|row| {
                        format!(
                            "{} {}←{}",
                            row["count"].as_i64().unwrap_or(0),
                            row["victim"].as_str().unwrap_or("?"),
                            row["killer"].as_str().unwrap_or("<none>")
                        )
                    })
                    .collect()
            })
            .unwrap_or_default();
        if !pairs.is_empty() {
            let total: i64 = losses["by_pair"]
                .as_array()
                .map(|rows| {
                    rows.iter()
                        .map(|row| row["count"].as_i64().unwrap_or(0))
                        .sum()
                })
                .unwrap_or(0);
            out.push_str(&format!(
                "- Losses: {} objects — {} (detail covers {} rows; at full strength {}, worn down {})\n",
                total,
                pairs.join(", "),
                losses["detail_rows"].as_u64().unwrap_or(0),
                losses["at_full_strength"].as_i64().unwrap_or(0),
                losses["worn_down"].as_i64().unwrap_or(0)
            ));
        }
        // What the house actually fielded, read off its sampled roster: the
        // engine keeps production clicks as raw native records, so the roster
        // is where "what existed, and when" is readable.
        let history = &house["roster_history"];
        let mut built: Vec<(i64, &str, u64)> = history["added"]
            .as_object()
            .map(|added| {
                added
                    .iter()
                    .filter_map(|(name, count)| {
                        let tick = history["first_seen"][name].as_u64().unwrap_or(0);
                        Some((count.as_i64().unwrap_or(0), name.as_str(), tick))
                    })
                    .collect()
            })
            .unwrap_or_default();
        if !built.is_empty() {
            built.sort_by(|left, right| right.0.cmp(&left.0));
            out.push_str(&format!(
                "### Built (from the sampled roster, every {} s)\n\n| copies | type | first seen |\n|---|---|---|\n",
                report["timeline"]["sample_ticks"].as_u64().unwrap_or(0)
            ));
            for (count, name, tick) in built.iter().take(40) {
                out.push_str(&format!(
                    "| {} | {} | {} ({:.1}s) |\n",
                    count,
                    name,
                    tick,
                    *tick as f64 / hz as f64
                ));
            }
            out.push('\n');
        }
        out.push('\n');

        // The build order: what the focus house queued and placed, in order.
        let build: Vec<String> = report["events"]
            .as_array()
            .map(|events| {
                events
                    .iter()
                    .filter(|event| {
                        event["house"].as_str() == Some(focus)
                            && matches!(event["kind"].as_str(), Some("queue" | "place" | "sell"))
                    })
                    .map(|event| {
                        let detail = &event["detail"];
                        let what = match event["kind"].as_str() {
                            Some("place") => format!(
                                "place {} at ({})",
                                detail["type"].as_str().unwrap_or("?"),
                                detail["cell"]
                                    .as_array()
                                    .map(|cell| cell
                                        .iter()
                                        .map(Value::to_string)
                                        .collect::<Vec<_>>()
                                        .join(","))
                                    .unwrap_or_default()
                            ),
                            Some("sell") => {
                                format!("sell {}", detail["type"].as_str().unwrap_or("?"))
                            }
                            _ => format!("queue {}", detail["type"].as_str().unwrap_or("?")),
                        };
                        format!(
                            "| {} | {} | {} |",
                            event["tick"].as_u64().unwrap_or(0),
                            event["time"].as_str().unwrap_or(""),
                            what
                        )
                    })
                    .collect()
            })
            .unwrap_or_default();
        if !build.is_empty() {
            out.push_str(
                "### Build order\n\n\
                The command stream, in ticks: what was clicked, not what was finished. \
                A queue that no factory could serve still appears here; the roster column \
                of the JSON report says what was actually alive.\n\n\
                | tick | time | what |\n|---|---|---|\n",
            );
            for line in build.iter().take(MAX_BUILD_ROWS_IN_PAGE) {
                out.push_str(line);
                out.push('\n');
            }
            if build.len() > MAX_BUILD_ROWS_IN_PAGE {
                out.push_str(&format!(
                    "\n… {} more rows in the JSON report\n",
                    build.len() - MAX_BUILD_ROWS_IN_PAGE
                ));
            }
            out.push('\n');
        }
    }

    // Every house, one line each: enough to see who is actually winning.
    out.push_str(
        "## Every house\n\n| house | side | credits | spent | army | structures | lost | defeated |\n\
        |---|---|---|---|---|---|---|---|\n",
    );
    for house in report["houses"].as_array().into_iter().flatten() {
        let identity = &house["identity"];
        let lost: i64 = house["losses"]["by_pair"]
            .as_array()
            .map(|rows| {
                rows.iter()
                    .map(|row| row["count"].as_i64().unwrap_or(0))
                    .sum()
            })
            .unwrap_or(0);
        let defeated = if house["result"]["defeated"].as_bool().unwrap_or(false) {
            house["result"]["defeat_tick"]
                .as_u64()
                .map_or_else(|| "yes".to_owned(), |tick| tick.to_string())
        } else {
            "no".to_owned()
        };
        out.push_str(&format!(
            "| {} | {} | {} | {} | {} | {} | {} | {} |\n",
            identity["name"].as_str().unwrap_or("?"),
            identity["side_index"].as_i64().unwrap_or(0),
            house["economy"]["final_credits"].as_i64().unwrap_or(0),
            house["economy"]["spent"].as_i64().unwrap_or(0),
            house["military"]["final_army_value"].as_i64().unwrap_or(0),
            house["military"]["final_counts"]["structures"]
                .as_u64()
                .unwrap_or(0),
            lost,
            defeated
        ));
    }
    out.push('\n');

    // The timeline: the shape over time, for the focus house against the field.
    out.push_str("## Timeline\n\n");
    out.push_str(&format!(
        "| time | tick | {focus} credits | {focus} army | {focus} structures | field army | field structures |\n"
    ));
    out.push_str("|---|---|---|---|---|---|---|\n");
    let timeline = &report["timeline"]["houses"];
    // The field is every *contending* house but the focus one: a passive house
    // owns the map's civilians and tech buildings, which are not an army.
    let passive: Vec<&str> = report["houses"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|house| house["identity"]["passive"].as_bool().unwrap_or(false))
        .filter_map(|house| house["identity"]["name"].as_str())
        .collect();
    let focus_rows = timeline
        .as_array()
        .into_iter()
        .flatten()
        .find(|house| house["house"].as_str() == Some(focus))
        .and_then(|house| house["rows"].as_array().cloned())
        .unwrap_or_default();
    for row in &focus_rows {
        let tick = row["tick"].as_u64().unwrap_or(0);
        let mut field_army = 0_i64;
        let mut field_structures = 0_i64;
        for house in timeline.as_array().into_iter().flatten() {
            let name = house["house"].as_str().unwrap_or("");
            if name == focus || passive.contains(&name) {
                continue;
            }
            if let Some(rows) = house["rows"].as_array() {
                if let Some(other) = rows.iter().find(|other| other["tick"].as_u64() == Some(tick))
                {
                    field_army += other["army_value"].as_i64().unwrap_or(0);
                    field_structures += other["structures"].as_i64().unwrap_or(0);
                }
            }
        }
        out.push_str(&format!(
            "| {:.1} | {} | {} | {} | {} | {} | {} |\n",
            tick as f64 / hz as f64,
            tick,
            row["credits"].as_i64().unwrap_or(0),
            row["army_value"].as_i64().unwrap_or(0),
            row["structures"].as_u64().unwrap_or(0),
            field_army,
            field_structures
        ));
    }
    out.push('\n');

    let events = report["events"].as_array().map_or(0, Vec::len);
    out.push_str(&format!(
        "{events} exact-tick events are in the JSON report.\n"
    ));
    out
}


fn short(value: &str, keep: usize) -> String {
    if value.len() <= keep {
        return value.to_owned();
    }
    format!("{}…", &value[..keep])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_five_second_window_is_the_replays_own_rate() {
        assert_eq!(sample_ticks_for(5.0, 45), 225);
        assert_eq!(sample_ticks_for(5.0, 15), 75);
    }

    #[test]
    fn a_window_shorter_than_one_tick_still_samples() {
        assert_eq!(sample_ticks_for(0.001, 45), 1);
    }

    #[test]
    fn a_clock_reads_minutes_and_seconds() {
        assert_eq!(clock(0, 45), "0:00.00");
        assert_eq!(clock(45, 45), "0:01.00");
        assert_eq!(clock(45 * 61, 45), "1:01.00");
    }

    #[test]
    fn a_map_of_counts_serializes_as_json_numbers() {
        let mut counts = BTreeMap::new();
        counts.insert("HTNK".to_owned(), 12_u32);
        assert_eq!(object_of(&counts), json!({ "HTNK": 12 }));
    }
}