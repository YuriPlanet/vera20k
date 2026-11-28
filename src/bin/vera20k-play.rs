//! Drive one headless skirmish over standard input and output. See `src/play_cli/README.md`.

use std::io::{self, BufRead, BufReader};
use std::path::PathBuf;
use std::process::ExitCode;

use vera20k_core::play_cli::protocol::{self, Outcome};
use vera20k_core::play_cli::{Session, SessionOptions};
use vera20k_core::skirmish_launch::{AiDifficulty, LaunchCountry};
use vera20k_core::util::config::GameConfig;

const COUNTRIES: [LaunchCountry; 7] = [
    LaunchCountry::Russia,
    LaunchCountry::Cuba,
    LaunchCountry::Libya,
    LaunchCountry::France,
    LaunchCountry::Germany,
    LaunchCountry::Korea,
    LaunchCountry::Iraq,
];
const USAGE: &str = "\
Usage: vera20k-play [--ra2-dir DIR] [--map NAME_OR_PATH] [--script FILE]
                    [--opponents 0..7] [--difficulty easy|normal|hard]
                    [--seed NUMBER] [--max-ticks NUMBER]

Play a headless skirmish over JSON Lines. See src/play_cli/README.md.
Defaults: Dustbowl.mmx, one normal opponent, seed 0, standard input.
Retail files: --ra2-dir, RA2_DIR, or [paths] ra2_dir in config.toml.
";

struct Args {
    retail_dir: Option<PathBuf>,
    map: String,
    opponents: usize,
    difficulty: AiDifficulty,
    seed: u32,
    script: Option<PathBuf>,
    max_ticks: Option<u64>,
}

fn main() -> ExitCode {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if matches!(args.as_slice(), [arg] if arg == "--help" || arg == "-h") {
        print!("{USAGE}");
        return ExitCode::SUCCESS;
    }
    match parse_args(&args).and_then(run) {
        Ok(true) => ExitCode::SUCCESS,
        Ok(false) => ExitCode::FAILURE,
        Err(error) => {
            eprintln!("vera20k-play: {error}");
            ExitCode::FAILURE
        }
    }
}

fn parse_args(args: &[String]) -> Result<Args, String> {
    let mut options = Args {
        retail_dir: None,
        map: "Dustbowl.mmx".into(),
        opponents: 1,
        difficulty: AiDifficulty::Normal,
        seed: 0,
        script: None,
        max_ticks: None,
    };
    let mut args = args.iter();
    while let Some(flag) = args.next() {
        let value = args
            .next()
            .ok_or_else(|| format!("{flag} needs a value. Use --help for options."))?;
        match flag.as_str() {
            "--ra2-dir" => options.retail_dir = Some(value.into()),
            "--map" => options.map = value.clone(),
            "--script" => options.script = Some(value.into()),
            "--opponents" => {
                options.opponents = value
                    .parse()
                    .ok()
                    .filter(|count| *count <= COUNTRIES.len())
                    .ok_or("--opponents must be between 0 and 7")?;
            }
            "--difficulty" => {
                options.difficulty = match value.as_str() {
                    "easy" => AiDifficulty::Easy,
                    "normal" => AiDifficulty::Normal,
                    "hard" => AiDifficulty::Hard,
                    _ => return Err("--difficulty must be easy, normal or hard".into()),
                }
            }
            "--seed" => {
                options.seed = match value
                    .strip_prefix("0x")
                    .or_else(|| value.strip_prefix("0X"))
                {
                    Some(hex) => u32::from_str_radix(hex, 16),
                    None => value.parse(),
                }
                .map_err(|_| "--seed must be a 32-bit unsigned number")?;
            }
            "--max-ticks" => {
                options.max_ticks = Some(
                    value
                        .parse()
                        .ok()
                        .filter(|limit| *limit > 0)
                        .ok_or("--max-ticks must be a positive integer")?,
                );
            }
            _ => return Err(format!("unknown option {flag}. Use --help for options.")),
        }
    }
    Ok(options)
}

fn run(args: Args) -> Result<bool, String> {
    let retail_dir = match args.retail_dir.or_else(|| {
        std::env::var_os("RA2_DIR")
            .filter(|value| !value.is_empty())
            .map(PathBuf::from)
    }) {
        Some(path) => path,
        None => {
            GameConfig::load()
                .map_err(|error| error.to_string())?
                .paths
                .ra2_dir
        }
    };
    let stdin = io::stdin();
    let input: Box<dyn BufRead> = match args.script {
        Some(path) => {
            let file = std::fs::File::open(&path)
                .map_err(|error| format!("{}: {error}", path.display()))?;
            Box::new(BufReader::new(file))
        }
        None => Box::new(stdin.lock()),
    };
    let (mut session, ready) = Session::start(SessionOptions {
        retail_dir,
        map: args.map,
        seed: args.seed,
        opponents: COUNTRIES[..args.opponents]
            .iter()
            .map(|&country| (country, args.difficulty))
            .collect(),
        max_ticks: args.max_ticks,
    })?;
    let stdout = io::stdout();
    let mut output = stdout.lock();
    protocol::write_event(&mut output, &ready)?;
    drop(ready);
    match protocol::run(&mut session, input, &mut output)? {
        Outcome::Finished => Ok(true),
        Outcome::Failed(message) => {
            eprintln!("vera20k-play: {message}");
            Ok(false)
        }
    }
}
