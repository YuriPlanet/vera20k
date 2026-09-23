//! Extract retail INI and data files from the .mix archives into `ini/`
//! for research grepping and the retail-data tests.
//! Run from the repository root with: `cargo run --bin extract-ini [RA2_DIR]`
//!
//! The install folder comes from the optional argument, then `$RA2_DIR`,
//! then `config.toml` (`resolve_ra2_dir`).

use std::fs;
use std::path::{Path, PathBuf};

fn main() {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("warn")).init();

    let explicit = std::env::args_os().nth(1).map(PathBuf::from);
    let (ra2_dir, _) = vera20k::asset_tools::root::resolve_ra2_dir(explicit.as_deref())
        .unwrap_or_else(|error| panic!("{error}"));
    let ra2_dir = ra2_dir.as_path();
    let out_dir = Path::new("ini");

    println!("Loading MIX archives from {}...", ra2_dir.display());
    let asset_manager = vera20k::assets::asset_manager::AssetManager::new(ra2_dir)
        .expect("Failed to load MIX archives");

    fs::create_dir_all(out_dir).expect("Failed to create ini/ directory");

    // All INI files that gamemd.exe loads from .mix archives.
    // Base RA2 files and YR (*md) patches listed together.
    let files = [
        // Core gameplay
        "rules.ini",
        "rulesmd.ini",
        "art.ini",
        "artmd.ini",
        "ai.ini",
        "aimd.ini",
        // Audio/EVA
        "sound.ini",
        "soundmd.ini",
        "eva.ini",
        "evamd.ini",
        "theme.ini",
        "thememd.ini",
        // Theater tilesets
        "temperat.ini",
        "temperatmd.ini",
        "snow.ini",
        "snowmd.ini",
        "urban.ini",
        "urbanmd.ini",
        "urbann.ini",
        "urbannmd.ini",
        "lunar.ini",
        "lunarmd.ini",
        "desert.ini",
        "desertmd.ini",
        // Campaign/multiplayer
        "battle.ini",
        "battlemd.ini",
        "missionmd.ini",
        // Multiplayer dialog
        "mpmodesmd.ini",
        // Random map generator tuning
        "rmg.ini",
        "rmgmd.ini",
    ];

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
