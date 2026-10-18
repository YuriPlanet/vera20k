//! Extract selected Yuri's Revenge INIs from the configured retail assets into
//! `ini/` for research and retail-data tests.
//! Run from the repository root with: `cargo run --bin extract-ini [RA2_DIR]`
//!
//! The install folder comes from the optional argument, then `$RA2_DIR`,
//! then `config.toml` (`resolve_ra2_dir`).

use std::fs;
use std::path::{Path, PathBuf};

fn main() {
    let _ = vera20k::util::logging::init_stderr_logger("warn");

    let explicit = std::env::args_os().nth(1).map(PathBuf::from);
    let (ra2_dir, _) = vera20k::asset_tools::root::resolve_ra2_dir(explicit.as_deref())
        .unwrap_or_else(|error| panic!("{error}"));
    let ra2_dir = ra2_dir.as_path();
    let out_dir = Path::new("ini");

    println!("Loading MIX archives from {}...", ra2_dir.display());
    let asset_manager = vera20k::assets::asset_manager::AssetManager::new(
        ra2_dir,
        vera20k::assets::asset_manager::MediaArchiveMode::STOCK_DIGITAL,
    )
    .expect("Failed to load MIX archives");

    fs::create_dir_all(out_dir).expect("Failed to create ini/ directory");

    // Standalone YR inputs. The active gamemd.exe never reads the RA2 base INIs.
    // Scenario/map INIs are outside this fixed archive inventory.
    let mut files: Vec<String> = [
        // Core gameplay
        "rulesmd.ini",
        "artmd.ini",
        "aimd.ini",
        "langrule.ini",
        // Audio/EVA
        "soundmd.ini",
        "evamd.ini",
        "thememd.ini",
        // Theater tilesets
        "temperatmd.ini",
        "snowmd.ini",
        "urbanmd.ini",
        "urbannmd.ini",
        "lunarmd.ini",
        "desertmd.ini",
        // Campaign/multiplayer
        "battlemd.ini",
        "missionmd.ini",
        // Multiplayer and shell setup
        "mpmodesmd.ini",
        "uimd.ini",
        "mapselmd.ini",
        "keyboardmd.ini",
        "coopcampmd.ini",
        // Random map generator tuning
        "rmgmd.ini",
    ]
    .into_iter()
    .map(str::to_owned)
    .collect();

    // Use the same selected MPModesMD roster and row reader as the skirmish UI.
    // The stock roster supplies nine rule-layer filenames; a modded roster may
    // supply different ones. Only plain INI basenames can be written below ini/.
    let modes = vera20k::skirmish_modes::skirmish_modes_from_assets(&asset_manager)
        .expect("Could not enumerate game-mode INIs from MPModesMD.ini");
    for mode in modes {
        let name = mode.override_file.trim();
        if name.is_empty() {
            continue;
        }
        if name.chars().any(|ch| matches!(ch, '/' | '\\' | ':'))
            || !name.to_ascii_lowercase().ends_with(".ini")
        {
            eprintln!("Skipping unsafe game-mode INI filename: {name}");
            continue;
        }
        if !files.iter().any(|file| file.eq_ignore_ascii_case(name)) {
            files.push(name.to_owned());
        }
    }

    let mut found = 0;
    let mut not_found = 0;

    for name in &files {
        match asset_manager.get_with_source(name) {
            Some((data, source)) => {
                let out_path = out_dir.join(name);
                fs::write(&out_path, &data).expect("Failed to write file");
                println!("  {:20} {:>8} bytes  from {}", name, data.len(), source);
                found += 1;
            }
            None => {
                not_found += 1;
            }
        }
    }

    println!(
        "\nExtracted {found} files to {}/  ({not_found} not found in archives)",
        out_dir.display()
    );
}
