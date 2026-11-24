//! Measure VERA's own computer player against the stock one.
//!
//! The tool plays a match on a retail map, with VERA's player on the first
//! computer house and the stock computer player on the others, and reports
//! what the match was worth. The trainer reads the same numbers through the
//! same core, so a policy the trainer found measures the same way here.
//!
//! ```text
//! ai-eval --ra2-dir <retail> --map Death.mmx --seeds 4 --policy policy.json
//! ```

mod ai_policy_file;

use std::path::{Path, PathBuf};

use vera20k_core::headless_scenario::ai_match::{self, AiMatchRequest};

/// What the tool was asked to do.
struct Args {
    ra2_dir: PathBuf,
    map: String,
    seed_base: u32,
    seeds: usize,
    ticks: u64,
    computers: usize,
    policy: Option<PathBuf>,
    rotation: usize,
}

fn usage() -> String {
    "usage: ai-eval [--ra2-dir <retail path>] [--map Death.mmx] [--seed-base N] \
     [--seeds N] [--ticks N] [--computers N] [--policy <file.json>] [--rotation N]"
        .to_string()
}

fn parse() -> Result<Args, String> {
    let mut args = Args {
        ra2_dir: std::env::var("RA2_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from("game-files/yr")),
        map: "Death.mmx".to_string(),
        seed_base: 1,
        seeds: 4,
        ticks: 9_000,
        computers: 2,
        policy: None,
        rotation: 0,
    };
    let mut argv = std::env::args().skip(1);
    while let Some(flag) = argv.next() {
        let mut value = || argv.next().ok_or(format!("{flag} needs a value"));
        match flag.as_str() {
            "--ra2-dir" => args.ra2_dir = PathBuf::from(value()?),
            "--map" => args.map = value()?,
            "--seed-base" => args.seed_base = value()?.parse().map_err(|_| "seed is a number")?,
            "--seeds" => args.seeds = value()?.parse().map_err(|_| "seeds is a number")?,
            "--ticks" => args.ticks = value()?.parse().map_err(|_| "ticks is a number")?,
            "--computers" => {
                args.computers = value()?.parse().map_err(|_| "computers is a number")?
            }
            "--policy" => args.policy = Some(PathBuf::from(value()?)),
            "--rotation" => args.rotation = value()?.parse().map_err(|_| "rotation is a number")?,
            other => return Err(format!("unrecognised argument {other}\n{}", usage())),
        }
    }
    if args.computers < 2 {
        return Err("a match needs two computer houses".to_string());
    }
    Ok(args)
}

fn main() {
    if let Err(error) = run() {
        eprintln!("ai-eval: {error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let args = parse()?;
    let policy = match &args.policy {
        Some(path) => ai_policy_file::read(path)?,
        None => vera20k_core::rules::vera_ai::VeraAiPolicy::default(),
    };
    let mut policy = policy;
    policy.enabled = true;
    policy.max_houses = 1;

    let mut total = 0.0;
    let mut wins = 0usize;
    let mut losses = 0usize;
    let mut undecided = 0usize;
    for index in 0..args.seeds {
        let request = AiMatchRequest {
            map: args.map.clone(),
            seed: args.seed_base.wrapping_add(index as u32),
            computers: args.computers,
            ticks: args.ticks,
            policy,
            rotation: args.rotation,
        };
        let outcome = ai_match::play_ai_match(&args.ra2_dir, &request)?;
        let vera = outcome
            .houses
            .first()
            .cloned()
            .unwrap_or_else(|| "?".to_string());
        let opponent = outcome
            .houses
            .get(1)
            .cloned()
            .unwrap_or_else(|| "?".to_string());
        let reward = outcome.reward(&vera, &opponent);
        total += reward;
        if reward >= 1.0 {
            wins += 1;
        } else if reward <= -1.0 {
            losses += 1;
        } else {
            undecided += 1;
        }
        println!(
            "map={} seed={} ticks={} decided={} reward={reward:+.3} vera={vera} opponent={opponent} \
             allied={} objects={:?} detail={:?}",
            args.map,
            request.seed,
            outcome.ticks,
            outcome.decided,
            outcome.houses_allied,
            outcome.house_objects,
            outcome.house_detail
        );
    }
    let count = args.seeds.max(1) as f64;
    println!(
        "summary map={} seeds={} wins={wins} losses={losses} undecided={undecided} mean_reward={:+.4}",
        args.map,
        args.seeds,
        total / count
    );
    Ok(())
}

/// Keep the unused import of `Path` from warning when the policy file is not
/// given.
#[allow(dead_code)]
fn _unused(_: &Path) {}
