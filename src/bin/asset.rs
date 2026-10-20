//! `asset` — headless browser for retail RA2/YR assets, for automated callers.
//!
//! Every verb prints JSON to stdout, including failures, so a caller parsing
//! stdout always gets JSON. All logic lives in `vera20k::asset_tools`; this file
//! only parses arguments, dispatches, and prints.
//!
//! Exit codes: 0 success, 1 the verb failed, 2 the command line was wrong.

use vera20k::asset_tools::args::{self, Cli, Verb};
use vera20k::asset_tools::names::NameDict;
use vera20k::asset_tools::report::{ErrorReport, to_json};
use vera20k::asset_tools::{
    palette, render_dispatch, retail_corpus, root, verb_art, verb_compare, verb_csf, verb_extract,
    verb_find, verb_info, verb_ini, verb_ini_expand, verb_ls, verb_palette, verb_parse_check,
    verb_scan, verb_sound,
};

const EXIT_FAILED: i32 = 1;
const EXIT_USAGE: i32 = 2;

fn main() {
    // Logs go to stderr; stdout is reserved for the JSON payload.
    let _ = vera20k::util::logging::init_stderr_logger("warn");

    let cli = match args::parse(std::env::args().skip(1)) {
        Ok(cli) => cli,
        Err(message) => {
            eprintln!("{}", args::usage());
            print_error(&message, Some("run `asset --help` for the verb list"));
            std::process::exit(EXIT_USAGE);
        }
    };

    if matches!(cli.verb, Verb::Help) {
        println!("{}", args::usage());
        return;
    }

    if let Err(error) = run(cli) {
        print_error(&error.error, error.hint.as_deref());
        std::process::exit(EXIT_FAILED);
    }
}

fn run(cli: Cli) -> Result<(), ErrorReport> {
    let (ra2_dir, source) =
        root::resolve_ra2_dir(cli.ra2_dir.as_deref()).map_err(|error| ErrorReport {
            error,
            hint: Some(
                "pass --ra2-dir <PATH>, set RA2_DIR, or put a config.toml with [paths] ra2_dir \
                 next to the working directory"
                    .to_string(),
            ),
        })?;
    log::info!("retail root {} (from {source:?})", ra2_dir.display());

    let source_only = matches!(cli.verb, Verb::IniExpand { .. }) && cli.ini_source_only;
    let manager = if source_only {
        root::open_ini_probe_manager(&ra2_dir, cli.all_mixes)
    } else if matches!(
        cli.verb,
        Verb::IniGet { .. } | Verb::IniType { .. } | Verb::IniRoots
    ) {
        root::open_ini_manager(&ra2_dir, cli.all_mixes, cli.ini_roots.as_ref())
    } else {
        root::open_manager(&ra2_dir, cli.all_mixes)
    };
    let mut manager = manager.map_err(|error| ErrorReport {
        error,
        hint: Some(format!(
            "check that {} is the install root containing ra2md.mix",
            ra2_dir.display()
        )),
    })?;

    match cli.verb {
        Verb::Help => unreachable!("handled before dispatch"),
        Verb::IniRoots => {
            let sources = manager.ini_roots().names().into_iter().map(|name| {
                vera20k::rules::retail_sources::select_ini(&manager, name).map(|selected| selected.source)
            }).collect::<Result<Vec<_>, _>>().map_err(|error| ErrorReport {
                error, hint: Some("Check the complete selected root triplet; missing roots do not fall back to YR.".into()),
            })?;
            println!(
                "{}",
                to_json(&serde_json::json!({
                    "ini_roots": manager.ini_roots(), "sources": sources,
                    "scope": if cli.all_mixes { "tooling_all_mixes" } else { "game_archive_stack" },
                    "limits": "Root selection only; this is not MO gameplay acceptance or arbitrary game MIX mounting."
                }))
            );
        }
        Verb::IniExpand { name } => {
            let report = verb_ini_expand::run(
                &manager,
                &name,
                cli.ini_strict,
                cli.ini_max_depth,
                cli.all_mixes,
                source_only,
            )?;
            println!("{}", to_json(&report));
        }
        Verb::IniGet { section, key } => {
            let report = verb_ini::run(
                &mut manager,
                &ra2_dir,
                verb_ini::IniQuery::Accessor {
                    section: &section,
                    key: &key,
                },
                &cli.ini,
            )?;
            println!("{}", to_json(&report));
        }
        Verb::IniType { type_id } => {
            let report = verb_ini::run(
                &mut manager,
                &ra2_dir,
                verb_ini::IniQuery::Type { type_id: &type_id },
                &cli.ini,
            )?;
            println!("{}", to_json(&report));
        }

        // `find` hashes the requested name forward, so it needs no INI expansion —
        // only the offline dictionary, for naming palette entries and reporting
        // which name database backed the answer.
        Verb::Find { name } => {
            let dict = NameDict::build_offline();
            let report = verb_find::run(&manager, &dict, &name, &cli.find)?;
            println!("{}", to_json(&report));
        }

        // `ls` is the one verb that must reverse hashes for every row, so it pays
        // for the full dictionary including the INI-derived names.
        Verb::Ls { archive } => {
            let dict = NameDict::build(&manager);
            let report = verb_ls::run(&manager, &dict, &archive, &cli.ls)?;
            println!("{}", to_json(&report));
        }

        Verb::Archives => {
            let rows = verb_ls::archives(&manager);
            println!("{}", to_json(&rows));
        }

        // `info` never reverses a hash and never picks a palette.
        Verb::Info { name } => {
            let report = verb_info::run(&manager, &name, &cli.info)?;
            println!("{}", to_json(&report));
        }

        // `render` sniffs the bytes and routes to the matching renderer.
        Verb::Render { name } => {
            let dict = NameDict::build_offline();
            let art_registry = palette::load_art_registry(&manager);
            let report = render_dispatch::run(&manager, &dict, &art_registry, &name, &cli.render)?;
            println!("{}", to_json(&report));
        }

        Verb::PaletteFor { name } => {
            let dict = NameDict::build_offline();
            let art_registry = palette::load_art_registry(&manager);
            let report = verb_palette::run(&manager, &dict, &art_registry, &name, &cli.palette)?;
            println!("{}", to_json(&report));
        }

        Verb::Extract { name } => {
            let report = verb_extract::run(&manager, &name, &cli.extract)?;
            println!("{}", to_json(&report));
        }

        Verb::CsfGet { key } => {
            let report = verb_csf::get(&manager, &key, &cli.csf)?;
            println!("{}", to_json(&report));
        }

        Verb::CsfGrep { text } => {
            let report = verb_csf::grep(&manager, &text, &cli.csf)?;
            println!("{}", to_json(&report));
        }

        Verb::Sound { name } => {
            let report = verb_sound::sound(&manager, &name, &cli.sound)?;
            println!("{}", to_json(&report));
        }

        Verb::BagLs => {
            let report = verb_sound::bag_ls(&manager, &cli.sound)?;
            println!("{}", to_json(&report));
        }

        // `compare` renders each variant with its own inferred palette, so it
        // needs the art registry the way `render` does.
        Verb::Compare { name } => {
            let dict = NameDict::build_offline();
            let art_registry = palette::load_art_registry(&manager);
            let report = verb_compare::run(&manager, &dict, &art_registry, &name, &cli.compare)?;
            println!("{}", to_json(&report));
        }

        // The two corpus-wide verbs both reverse hashes for every hit they
        // report, so they pay for the full dictionary like `ls` does.
        Verb::Scan => {
            let dict = NameDict::build(&manager);
            let report = verb_scan::run(&manager, &dict, &cli.scan)?;
            println!("{}", to_json(&report));
        }

        Verb::CorpusBaseline => {
            let destination = cli.corpus_baseline_out.as_deref().expect("validated --out");
            let report = retail_corpus::export_candidate(&manager, destination)
                .map_err(|error| ErrorReport { error, hint: Some("Use a new candidate file and review the result; never rebaseline to hide a test failure.".to_string()) })?;
            println!("{}", to_json(&report));
        }

        Verb::ParseCheck => {
            let dict = NameDict::build(&manager);
            let report = verb_parse_check::run(&manager, &dict, &cli.parse_check)?;
            println!("{}", to_json(&report));
        }

        // `art-for` is the one verb that needs the art registry for its answer
        // rather than for a palette guess.
        Verb::ArtFor { type_id } => {
            let art_registry = palette::load_art_registry(&manager);
            let report =
                verb_art::run(&manager, &art_registry, &cli.art_image, &type_id, &cli.art)?;
            println!("{}", to_json(&report));
        }
    }

    Ok(())
}

fn print_error(error: &str, hint: Option<&str>) {
    let report = ErrorReport {
        error: error.to_string(),
        hint: hint.map(str::to_string),
    };
    println!("{}", to_json(&report));
}
