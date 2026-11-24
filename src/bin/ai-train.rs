//! Train VERA's own computer player: an evolution strategy over the numbers
//! in the `[VeraAI]` policy.
//!
//! The trainer does not change the game. It changes a small vector of
//! numbers — how often the player decides, how big its army must be before
//! it leaves, how much of the army goes out, how it mixes its production —
//! and measures each candidate by playing the same matches through the same
//! core the game plays (`headless_scenario::ai_match`). The policy the
//! trainer finds is written as a policy file, which `ai-eval` reads back and
//! which a game may carry in its own `[VeraAI]` section.
//!
//! ```text
//! ai-train --maps NewHghts.mmx,Round.mmx --seeds 2 --generations 20 --out best.json
//! ```
//!
//! The strategy is a (μ/μ, λ)-evolution strategy with rank weights: each
//! generation samples a population around the current mean, keeps the better
//! half and moves the mean onto it. Every knob is scaled into `[0, 1]` before
//! the search, so one step size fits them all, and the step size grows when
//! a generation brings no improvement and shrinks when it does.

mod ai_policy_file;

use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};

use vera20k_core::headless_scenario::ai_match::{self, AiMatchRequest};
use vera20k_core::rules::vera_ai::VeraAiPolicy;

/// The knobs the trainer moves, and the range each may take.
///
/// The ranges are the policy's own clamps: a number outside its range is
/// clamped by the reader, so the search never leaves a value the game would
/// refuse.
#[derive(Debug, Clone, Copy)]
struct Knobs {
    decision_period: f64,
    attack_threshold: f64,
    home_guard: f64,
    attack_waves: f64,
    infantry_weight: f64,
    vehicle_weight: f64,
    aircraft_weight: f64,
    infantry_period: f64,
    vehicle_period: f64,
    aircraft_period: f64,
    opening_delay: f64,
}

/// The knob's lower and upper bound.
type Range = (f64, f64);

impl Knobs {
    /// The knobs the trainer moves, with their ranges.
    const KNOBS: [(&'static str, Range); 11] = [
        ("DecisionPeriod", (5.0, 300.0)),
        ("AttackThreshold", (0.0, 60.0)),
        ("HomeGuard", (0.0, 30.0)),
        ("AttackWaves", (1.0, 8.0)),
        ("InfantryWeight", (0.0, 3.0)),
        ("VehicleWeight", (0.0, 3.0)),
        ("AircraftWeight", (0.0, 3.0)),
        ("InfantryPeriod", (15.0, 600.0)),
        ("VehiclePeriod", (15.0, 600.0)),
        ("AircraftPeriod", (15.0, 600.0)),
        ("OpeningDelay", (0.0, 1800.0)),
    ];

    /// The policy the defaults hold.
    fn default() -> Self {
        let policy = VeraAiPolicy::default();
        Self {
            decision_period: policy.decision_period as f64,
            attack_threshold: policy.attack_threshold as f64,
            home_guard: policy.home_guard as f64,
            attack_waves: policy.attack_waves as f64,
            infantry_weight: policy.infantry_weight as f64,
            vehicle_weight: policy.vehicle_weight as f64,
            aircraft_weight: policy.aircraft_weight as f64,
            infantry_period: policy.infantry_period as f64,
            vehicle_period: policy.vehicle_period as f64,
            aircraft_period: policy.aircraft_period as f64,
            opening_delay: policy.opening_delay as f64,
        }
    }

    /// The knobs a policy holds.
    fn of(policy: &VeraAiPolicy) -> Self {
        Self {
            decision_period: policy.decision_period as f64,
            attack_threshold: policy.attack_threshold as f64,
            home_guard: policy.home_guard as f64,
            attack_waves: policy.attack_waves as f64,
            infantry_weight: policy.infantry_weight as f64,
            vehicle_weight: policy.vehicle_weight as f64,
            aircraft_weight: policy.aircraft_weight as f64,
            infantry_period: policy.infantry_period as f64,
            vehicle_period: policy.vehicle_period as f64,
            aircraft_period: policy.aircraft_period as f64,
            opening_delay: policy.opening_delay as f64,
        }
    }

    /// The knobs as the scaled vector the search works on.
    fn scaled(&self) -> Vec<f64> {
        Self::KNOBS
            .iter()
            .zip([
                self.decision_period,
                self.attack_threshold,
                self.home_guard,
                self.attack_waves,
                self.infantry_weight,
                self.vehicle_weight,
                self.aircraft_weight,
                self.infantry_period,
                self.vehicle_period,
                self.aircraft_period,
                self.opening_delay,
            ])
            .map(|((_, (low, high)), value)| (value - low) / (high - low))
            .collect()
    }

    /// The knobs a scaled vector holds, clamped into their ranges.
    fn from_scaled(vector: &[f64]) -> Self {
        let value = |index: usize| {
            let (low, high) = Self::KNOBS[index].1;
            let scaled = vector.get(index).copied().unwrap_or(0.5).clamp(0.0, 1.0);
            low + scaled * (high - low)
        };
        Self {
            decision_period: value(0),
            attack_threshold: value(1),
            home_guard: value(2),
            attack_waves: value(3),
            infantry_weight: value(4),
            vehicle_weight: value(5),
            aircraft_weight: value(6),
            infantry_period: value(7),
            vehicle_period: value(8),
            aircraft_period: value(9),
            opening_delay: value(10),
        }
    }

    /// The policy the knobs hold: the player's own numbers, with the
    /// player's own rounding.
    fn policy(&self) -> VeraAiPolicy {
        let base = VeraAiPolicy::default();
        VeraAiPolicy {
            enabled: true,
            max_houses: 1,
            decision_period: self.decision_period.round() as i32,
            attack_threshold: self.attack_threshold.round() as i32,
            home_guard: self.home_guard.round() as i32,
            attack_waves: self.attack_waves.round() as i32,
            infantry_weight: self.infantry_weight as f32,
            vehicle_weight: self.vehicle_weight as f32,
            aircraft_weight: self.aircraft_weight as f32,
            infantry_period: self.infantry_period.round() as i32,
            vehicle_period: self.vehicle_period.round() as i32,
            aircraft_period: self.aircraft_period.round() as i32,
            target_production: base.target_production,
            opening_delay: self.opening_delay.round() as i32,
        }
    }

    /// The knobs as the trainer prints them.
    fn report(&self) -> String {
        Self::KNOBS
            .iter()
            .zip([
                self.decision_period,
                self.attack_threshold,
                self.home_guard,
                self.attack_waves,
                self.infantry_weight,
                self.vehicle_weight,
                self.aircraft_weight,
                self.infantry_period,
                self.vehicle_period,
                self.aircraft_period,
                self.opening_delay,
            ])
            .map(|((name, _), value)| format!("{name}={value:.2}"))
            .collect::<Vec<_>>()
            .join(" ")
    }
}

/// A match the trainer plays: a map and a seed.
#[derive(Debug, Clone)]
struct Case {
    map: String,
    seed: u32,
}

/// What the tool was asked to do.
struct Args {
    ra2_dir: PathBuf,
    maps: Vec<String>,
    seeds: usize,
    seed_base: u32,
    ticks: u64,
    generations: usize,
    population: usize,
    sigma: f64,
    threads: usize,
    start: Option<PathBuf>,
    out: Option<PathBuf>,
    baseline: bool,
}

fn usage() -> String {
    "usage: ai-train [--ra2-dir <retail path>] [--maps A.mmx,B.mmx] [--seeds N] \
     [--seed-base N] [--ticks N] [--generations N] [--population N] [--sigma F] \
     [--threads N] [--start <file.json>] [--out <file.json>] [--baseline]"
        .to_string()
}

fn parse() -> Result<Args, String> {
    let mut args = Args {
        ra2_dir: std::env::var("RA2_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from("game-files/yr")),
        maps: vec!["NewHghts.mmx".to_string()],
        seeds: 2,
        seed_base: 1,
        ticks: 8_000,
        generations: 10,
        population: 8,
        sigma: 0.15,
        threads: 0,
        start: None,
        out: None,
        baseline: false,
    };
    let mut argv = std::env::args().skip(1);
    while let Some(flag) = argv.next() {
        let mut value = || argv.next().ok_or(format!("{flag} needs a value"));
        match flag.as_str() {
            "--ra2-dir" => args.ra2_dir = PathBuf::from(value()?),
            "--maps" => {
                args.maps = value()?
                    .split(',')
                    .map(str::trim)
                    .filter(|map| !map.is_empty())
                    .map(str::to_string)
                    .collect();
                if args.maps.is_empty() {
                    return Err("a run needs at least one map".to_string());
                }
            }
            "--seeds" => args.seeds = value()?.parse().map_err(|_| "seeds is a number")?,
            "--seed-base" => {
                args.seed_base = value()?.parse().map_err(|_| "seed-base is a number")?
            }
            "--ticks" => args.ticks = value()?.parse().map_err(|_| "ticks is a number")?,
            "--generations" => {
                args.generations = value()?.parse().map_err(|_| "generations is a number")?
            }
            "--population" => {
                args.population = value()?.parse().map_err(|_| "population is a number")?
            }
            "--sigma" => args.sigma = value()?.parse().map_err(|_| "sigma is a number")?,
            "--threads" => args.threads = value()?.parse().map_err(|_| "threads is a number")?,
            "--start" => args.start = Some(PathBuf::from(value()?)),
            "--out" => args.out = Some(PathBuf::from(value()?)),
            "--baseline" => args.baseline = true,
            other => return Err(format!("unrecognised argument {other}\n{}", usage())),
        }
    }
    if args.seeds == 0 || args.generations == 0 {
        return Err("training needs at least one seed and one generation".to_string());
    }
    if args.population < 2 {
        return Err("a population needs at least two candidates".to_string());
    }
    Ok(args)
}

fn main() {
    if let Err(error) = run() {
        eprintln!("ai-train: {error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let args = parse()?;
    let cases: Vec<Case> = args
        .maps
        .iter()
        .flat_map(|map| {
            (0..args.seeds).map(move |index| Case {
                map: map.clone(),
                seed: args.seed_base.wrapping_add(index as u32),
            })
        })
        .collect();

    let mut mean = match &args.start {
        Some(path) => Knobs::of(&ai_policy_file::read(path)?).scaled(),
        None => Knobs::default().scaled(),
    };
    let mut sigma = args.sigma;
    let mut rng = Rng::new(0x5EED_1234_ABCD_0001);
    let mut best_score = f64::NEG_INFINITY;
    let mut best = mean.clone();

    if args.baseline {
        // The stock computer player, measured on the same matches: the number
        // the trained policy has to beat.
        let stock = score_policy(
            &args,
            &cases,
            &VeraAiPolicy {
                enabled: false,
                ..VeraAiPolicy::default()
            },
            args.threads,
        )?;
        println!("baseline(stock) mean_reward={stock:+.4}");
    }

    for generation in 0..args.generations {
        let population: Vec<Vec<f64>> = (0..args.population)
            .map(|_| {
                let mut candidate = mean.clone();
                for value in &mut candidate {
                    *value += sigma * rng.gaussian();
                }
                candidate
            })
            .collect();
        let mut scored: Vec<(f64, Vec<f64>)> = Vec::with_capacity(population.len());
        for candidate in &population {
            let policy = Knobs::from_scaled(candidate).policy();
            let score = score_policy(&args, &cases, &policy, args.threads)?;
            scored.push((score, candidate.clone()));
        }
        scored.sort_by(|left, right| {
            right
                .0
                .partial_cmp(&left.0)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        let best_score_now = scored[0].0;
        if best_score_now > best_score {
            best_score = best_score_now;
            best = scored[0].1.clone();
            sigma = (sigma * 0.9).clamp(0.01, 1.0);
        } else {
            sigma = (sigma * 1.2).clamp(0.01, 1.0);
        }
        // The better half, weighted by rank, moves the mean.
        let keep = (scored.len() / 2).max(1);
        let mut weights: Vec<f64> = (1..=keep).map(|rank| (keep + 1 - rank) as f64).collect();
        let total: f64 = weights.iter().sum();
        for weight in &mut weights {
            *weight /= total;
        }
        let mut next = vec![0.0; mean.len()];
        for (index, (_, candidate)) in scored.iter().take(keep).enumerate() {
            for (value, candidate_value) in next.iter_mut().zip(candidate) {
                *value += weights[index] * candidate_value;
            }
        }
        mean = next;
        let best_knobs = Knobs::from_scaled(&best);
        println!(
            "generation {generation} best={best_score_now:+.4} sigma={sigma:.3} {}",
            best_knobs.report()
        );
    }

    let policy = Knobs::from_scaled(&best).policy();
    println!("best policy: {}", Knobs::of(&policy).report());
    if let Some(path) = &args.out {
        ai_policy_file::write(path, &policy)?;
        println!("wrote {}", path.display());
    }
    Ok(())
}

/// Play the cases with one policy and answer the mean reward.
fn score_policy(
    args: &Args,
    cases: &[Case],
    policy: &VeraAiPolicy,
    threads: usize,
) -> Result<f64, String> {
    let jobs: Vec<(usize, &Case)> = cases.iter().enumerate().collect();
    let scores = Mutex::new(vec![Ok(0.0); cases.len()]);
    let next = AtomicUsize::new(0);
    let workers = if threads > 0 {
        threads
    } else {
        std::thread::available_parallelism()
            .map(|count| count.get())
            .unwrap_or(1)
    }
    .min(jobs.len().max(1));

    std::thread::scope(|scope| {
        for _ in 0..workers {
            scope.spawn(|| {
                loop {
                    let index = next.fetch_add(1, Ordering::Relaxed);
                    let Some((case_index, case)) = jobs.get(index) else {
                        return;
                    };
                    let request = AiMatchRequest {
                        map: case.map.clone(),
                        seed: case.seed,
                        computers: 2,
                        ticks: args.ticks,
                        policy: *policy,
                        rotation: 0,
                    };
                    let reward = ai_match::play_ai_match(&args.ra2_dir, &request)
                        .map(|outcome| {
                            let vera = outcome.houses.first().map_or("", String::as_str);
                            let opponent = outcome.houses.get(1).map_or("", String::as_str);
                            outcome.reward(vera, opponent)
                        })
                        .map_err(|error| format!("{} seed {}: {error}", case.map, case.seed));
                    scores.lock().unwrap()[*case_index] = reward;
                }
            });
        }
    });

    let sum = scores
        .into_inner()
        .unwrap()
        .into_iter()
        .sum::<Result<f64, _>>()?;
    Ok(sum / cases.len() as f64)
}

/// A small deterministic generator: the trainer's own stream, so a run with
/// the same arguments searches the same candidates.
struct Rng {
    state: u64,
}

impl Rng {
    fn new(seed: u64) -> Self {
        Self { state: seed | 1 }
    }

    /// The next value in `[0, 1)`.
    fn next_unit(&mut self) -> f64 {
        // xorshift64*.
        self.state ^= self.state >> 12;
        self.state ^= self.state << 25;
        self.state ^= self.state >> 27;
        let value = self.state.wrapping_mul(0x2545_F491_4F6C_DD1D);
        (value >> 11) as f64 / (1u64 << 53) as f64
    }

    /// A standard normal value.
    fn gaussian(&mut self) -> f64 {
        // Box-Muller: two uniforms, one normal.
        let first = self.next_unit().max(f64::MIN_POSITIVE);
        let second = self.next_unit();
        (-2.0 * first.ln()).sqrt() * (std::f64::consts::TAU * second).cos()
    }
}

/// Keep the unused import of `Path` from warning.
#[allow(dead_code)]
fn _unused(_: &Path) {}
