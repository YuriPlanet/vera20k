//! Integration tests for MIX archive loading against real RA2 files.
//! Tests skip gracefully if RA2 assets aren't found at the expected path,
//! allowing CI and machines without RA2 to run `cargo test` without failures.

use std::path::{Path, PathBuf};

use crate::assets::mix_archive::MixArchive;
use crate::assets::pal_file::Palette;
use crate::assets::shp_file::ShpFile;
use crate::assets::tmp_file::TmpFile;
use crate::map::map_file;

/// All known nested .mix files that might exist inside ra2.mix.
/// Different RA2 installations have different subsets.
const ALL_NESTED_NAMES: &[&str] = &[
    "local.mix",
    "conquer.mix",
    "cache.mix",
    "snow.mix",
    "temperat.mix",
    "urban.mix",
    "isosnow.mix",
    "isotem.mix",
    "isourb.mix",
];

/// Helper: return the asset path if it exists, or skip the test.
/// Uses a macro so the `return` exits the calling test function.
macro_rules! skip_if_missing {
    ($path:expr) => {{
        let p: &Path = Path::new($path);
        if !p.exists() {
            eprintln!("SKIPPED: {} not found (RA2 assets not installed)", $path);
            return;
        }
        p
    }};
}

/// Helper: return the RA2 asset directory or skip the test.
/// Reads the path from RA2_DIR env var, or falls back to config.toml.
macro_rules! require_ra2_dir {
    () => {{
        let dir: PathBuf = match std::env::var("RA2_DIR") {
            Ok(val) => PathBuf::from(val),
            Err(_) => match crate::util::config::GameConfig::load() {
                Ok(cfg) => cfg.paths.ra2_dir,
                Err(_) => {
                    eprintln!("SKIPPED: RA2_DIR not set and config.toml not found");
                    return;
                }
            },
        };
        // Without config.toml, GameConfig::load falls back to the running
        // executable's folder, which for a test binary is `target/`: require
        // an actual install, not just an existing directory.
        if !dir.join("ra2.mix").exists() {
            eprintln!("SKIPPED: RA2 assets not found at {}", dir.display());
            return;
        }
        dir
    }};
}

/// Load ra2.mix and extract all available nested MIX archives.
/// Returns (ra2_mix, vec of nested archives).
fn load_ra2_with_nested(dir: &Path) -> Option<(MixArchive, Vec<MixArchive>)> {
    let path: PathBuf = dir.join("ra2.mix");
    if !path.exists() {
        return None;
    }

    let ra2_mix: MixArchive = MixArchive::load(&path).ok()?;
    let mut nested: Vec<MixArchive> = Vec::new();

    for &name in ALL_NESTED_NAMES {
        if let Some(data) = ra2_mix.get_by_name(name) {
            if let Ok(archive) = MixArchive::from_bytes(data.to_vec()) {
                eprintln!(
                    "  Loaded nested: {} ({} entries)",
                    name,
                    archive.entry_count()
                );
                nested.push(archive);
            }
        }
    }

    Some((ra2_mix, nested))
}

#[test]
fn test_load_theme_mix_old_format() {
    let dir: PathBuf = require_ra2_dir!();

    let path: PathBuf = dir.join("theme.mix");
    skip_if_missing!(path.to_str().unwrap());

    let archive: MixArchive =
        MixArchive::load(&path).expect("theme.mix should parse as old-format MIX");

    // theme.mix is old-format (first u16 is file_count, non-zero).
    assert!(
        archive.entry_count() > 0,
        "theme.mix should have at least 1 entry, got {}",
        archive.entry_count()
    );

    eprintln!(
        "theme.mix: {} entries (old format, unencrypted)",
        archive.entry_count()
    );
}

#[test]
fn test_theme_mix_finds_known_track_by_name() {
    let dir: PathBuf = require_ra2_dir!();

    let path: PathBuf = dir.join("theme.mix");
    skip_if_missing!(path.to_str().unwrap());

    let archive: MixArchive = MixArchive::load(&path).expect("Should load theme.mix");
    let candidates: &[&str] = &["grinder.wav", "200meter.wav", "motorize.wav", "hm2.wav"];

    let found = candidates
        .iter()
        .find_map(|name| archive.get_by_name(name).map(|data| (*name, data.len())));

    match found {
        Some((name, len)) => eprintln!("theme.mix track lookup ok: {} ({} bytes)", name, len),
        None => panic!(
            "theme.mix did not resolve any known track names via get_by_name: {:?}",
            candidates
        ),
    }
}

#[test]
fn test_load_ra2_mix_encrypted() {
    let dir: PathBuf = require_ra2_dir!();

    let path: PathBuf = dir.join("ra2.mix");
    skip_if_missing!(path.to_str().unwrap());

    let archive: MixArchive =
        MixArchive::load(&path).expect("ra2.mix should parse (new format, encrypted)");

    // ra2.mix should have entries.
    assert!(
        archive.entry_count() > 5,
        "ra2.mix should have entries, got {}",
        archive.entry_count()
    );

    eprintln!(
        "ra2.mix: {} entries (new format, encrypted)",
        archive.entry_count()
    );
}

#[test]
fn test_ra2_mix_contains_known_files() {
    let dir: PathBuf = require_ra2_dir!();

    let path: PathBuf = dir.join("ra2.mix");
    skip_if_missing!(path.to_str().unwrap());

    let archive: MixArchive = MixArchive::load(&path).expect("Should load ra2.mix");

    // ra2.mix should contain nested .mix archives. Different installations
    // have different subsets. We check that at least one is found.
    let mut found_count: usize = 0;

    for &name in ALL_NESTED_NAMES {
        if archive.get_by_name(name).is_some() {
            eprintln!("  Found {} in ra2.mix", name);
            found_count += 1;
        }
    }

    assert!(
        found_count > 0,
        "ra2.mix should contain at least one nested .mix archive from {:?}",
        ALL_NESTED_NAMES
    );

    eprintln!("Found {} nested archives in ra2.mix", found_count);
}

#[test]
fn test_nested_mix_extraction() {
    let dir: PathBuf = require_ra2_dir!();

    let (ra2_mix, nested) = match load_ra2_with_nested(&dir) {
        Some(result) => result,
        None => {
            eprintln!("SKIPPED: Could not load ra2.mix");
            return;
        }
    };

    // At least one nested archive should have been extracted and parsed.
    assert!(
        !nested.is_empty(),
        "Should extract at least one nested archive from ra2.mix ({} entries)",
        ra2_mix.entry_count()
    );

    for archive in &nested {
        assert!(
            archive.entry_count() > 0,
            "Nested archive should have entries"
        );
    }

    eprintln!(
        "Successfully extracted and parsed {} nested archives",
        nested.len()
    );
}

#[test]
fn test_load_language_mix_encrypted() {
    let dir: PathBuf = require_ra2_dir!();

    let path: PathBuf = dir.join("language.mix");
    skip_if_missing!(path.to_str().unwrap());

    let archive: MixArchive =
        MixArchive::load(&path).expect("language.mix should parse (encrypted)");

    assert!(
        archive.entry_count() > 0,
        "language.mix should have entries"
    );

    eprintln!("language.mix: {} entries", archive.entry_count());
}

#[test]
fn test_extract_palette_from_mix_chain() {
    let dir: PathBuf = require_ra2_dir!();

    let (ra2_mix, nested) = match load_ra2_with_nested(&dir) {
        Some(result) => result,
        None => {
            eprintln!("SKIPPED: Could not load ra2.mix");
            return;
        }
    };

    // Search for a palette file across all nested archives, then ra2.mix.
    let pal_names: &[&str] = &["unittem.pal", "unit.pal", "temperat.pal", "isotem.pal"];

    for &pal_name in pal_names {
        for archive in &nested {
            if let Some(pal_data) = archive.get_by_name(pal_name) {
                eprintln!(
                    "Found {} ({} bytes) in nested archive",
                    pal_name,
                    pal_data.len()
                );
                let palette: Palette = Palette::from_bytes(pal_data)
                    .unwrap_or_else(|e| panic!("{} should parse: {:#}", pal_name, e));
                assert_eq!(palette.colors.len(), 256);
                eprintln!("  Palette parsed successfully (256 colors)");
                return;
            }
        }

        if let Some(pal_data) = ra2_mix.get_by_name(pal_name) {
            eprintln!(
                "Found {} in ra2.mix top-level ({} bytes)",
                pal_name,
                pal_data.len()
            );
            let palette: Palette = Palette::from_bytes(pal_data)
                .unwrap_or_else(|e| panic!("{} should parse: {:#}", pal_name, e));
            assert_eq!(palette.colors.len(), 256);
            return;
        }
    }

    eprintln!("SKIPPED: No palette files found in MIX chain");
}

#[test]
fn test_extract_shp_from_mix_chain() {
    let dir: PathBuf = require_ra2_dir!();

    let (_ra2_mix, nested) = match load_ra2_with_nested(&dir) {
        Some(result) => result,
        None => {
            eprintln!("SKIPPED: Could not load ra2.mix");
            return;
        }
    };

    // Search for a simple SHP sprite in nested archives.
    let shp_names: &[&str] = &["mouse.shp", "clock.shp", "pips.shp", "power.shp"];

    for &shp_name in shp_names {
        for archive in &nested {
            if let Some(shp_data) = archive.get_by_name(shp_name) {
                eprintln!("Found {} ({} bytes)", shp_name, shp_data.len());
                let shp: ShpFile = ShpFile::from_bytes(shp_data)
                    .unwrap_or_else(|e| panic!("{} should parse: {:#}", shp_name, e));
                assert!(!shp.frames.is_empty(), "{} should have frames", shp_name);
                eprintln!(
                    "  SHP: {}x{}, {} frames",
                    shp.width,
                    shp.height,
                    shp.frames.len()
                );
                return;
            }
        }
    }

    eprintln!("SKIPPED: No SHP files found in MIX chain");
}

#[test]
fn test_dump_pip_shp_frame_info() {
    let dir: PathBuf = require_ra2_dir!();
    let (_ra2_mix, nested) = match load_ra2_with_nested(&dir) {
        Some(result) => result,
        None => {
            eprintln!("SKIPPED: Could not load ra2.mix");
            return;
        }
    };

    for &name in &["pips.shp", "pipbrd.shp", "pips2.shp"] {
        let mut found_count = 0;
        for archive in &nested {
            if let Some(data) = archive.get_by_name(name) {
                let shp = ShpFile::from_bytes(data).expect(name);
                found_count += 1;
                eprintln!(
                    "\n{} (copy {}): canvas={}x{}, {} frames",
                    name,
                    found_count,
                    shp.width,
                    shp.height,
                    shp.frames.len()
                );
                for (i, f) in shp.frames.iter().enumerate() {
                    if i < 5 || (i >= 13 && i <= 20) {
                        eprintln!(
                            "  frame {:2}: pos=({:3},{:3}) size={}x{}",
                            i, f.frame_x, f.frame_y, f.frame_width, f.frame_height
                        );
                    }
                }
            }
        }
        if found_count == 0 {
            eprintln!("\n{}: NOT FOUND", name);
        }
    }
}

#[test]
fn test_extract_tmp_from_mix_chain() {
    let dir: PathBuf = require_ra2_dir!();

    let (_ra2_mix, nested) = match load_ra2_with_nested(&dir) {
        Some(result) => result,
        None => {
            eprintln!("SKIPPED: Could not load ra2.mix");
            return;
        }
    };

    // TMP terrain tile filenames vary by theater extension:
    // .tem (temperate), .sno (snow), .urb (urban).
    let tmp_names: &[&str] = &[
        "clat01.tem",
        "shore01.tem",
        "cliff01.tem",
        "s1.tem",
        "clat01.sno",
        "shore01.sno",
        "cliff01.sno",
        "s1.sno",
    ];

    for &tmp_name in tmp_names {
        for archive in &nested {
            if let Some(tmp_data) = archive.get_by_name(tmp_name) {
                eprintln!("Found {} ({} bytes)", tmp_name, tmp_data.len());
                match TmpFile::from_bytes(tmp_data) {
                    Ok(tmp) => {
                        let non_empty: usize = tmp.tiles.iter().filter(|t| t.is_some()).count();
                        eprintln!(
                            "  TMP: {}x{} grid, tile {}x{}, {} non-empty cells",
                            tmp.template_width,
                            tmp.template_height,
                            tmp.tile_width,
                            tmp.tile_height,
                            non_empty
                        );
                        assert!(non_empty > 0, "TMP should have at least one tile");
                        assert!(tmp.tile_width > 0 && tmp.tile_height > 0);
                        return;
                    }
                    Err(e) => {
                        eprintln!("  Parse error (trying next): {:#}", e);
                    }
                }
            }
        }
    }

    eprintln!("SKIPPED: No TMP files found in MIX chain");
}

#[test]
fn test_load_mmx_map_file() {
    let dir: PathBuf = require_ra2_dir!();

    // Try known .mmx map files from the RA2 directory.
    let mmx_names: &[&str] = &[
        "Arena.mmx",
        "Hills.mmx",
        "TwoRivers.mmx",
        "LittleBigLake.mmx",
        "Alley.mmx",
    ];

    for &name in mmx_names {
        let path: PathBuf = dir.join(name);
        if !path.exists() {
            continue;
        }
        match map_file::load_mmx(&path) {
            Ok(map) => {
                eprintln!(
                    "{}: theater={}, {}x{}, {} cells",
                    name,
                    map.header.theater,
                    map.header.width,
                    map.header.height,
                    map.cells.len()
                );
                assert!(map.header.width > 0, "Map width should be > 0");
                assert!(map.header.height > 0, "Map height should be > 0");
                assert!(!map.cells.is_empty(), "Map should have terrain cells");
                assert!(!map.header.theater.is_empty(), "Map should have a theater");
                return;
            }
            Err(e) => {
                eprintln!("  {} failed: {:#}", name, e);
            }
        }
    }

    eprintln!("SKIPPED: No .mmx map files found in {}", dir.display());
}

/// Test that the AssetManager can find palettes after brute-force loading.
#[test]
fn test_asset_manager_finds_palettes() {
    let dir: PathBuf = require_ra2_dir!();
    let asset_manager =
        crate::assets::asset_manager::AssetManager::new(&dir).expect("Should load AssetManager");

    let pal_names: &[&str] = &[
        "unittem.pal",
        "unit.pal",
        "temperat.pal",
        "isotem.pal",
        "unitsno.pal",
        "uniturb.pal",
        "snow.pal",
        "urban.pal",
        "anim.pal",
        "lib.pal",
        "mousepal.pal",
        "unitdes.pal",
        "unitlun.pal",
    ];
    let mut found: usize = 0;
    for &name in pal_names {
        if let Some(data) = asset_manager.get(name) {
            eprintln!("  PAL found: {} ({} bytes)", name, data.len());
            found += 1;
        }
    }
    eprintln!("Palettes found: {}/{}", found, pal_names.len());

    // Also check for common SHP sprites
    let shp_names: &[&str] = &[
        "mouse.shp",
        "pips.shp",
        "power.shp",
        "clock.shp",
        "sidebar.shp",
        "credits.shp",
    ];
    let mut shp_found: usize = 0;
    for &name in shp_names {
        if let Some(data) = asset_manager.get(name) {
            eprintln!("  SHP found: {} ({} bytes)", name, data.len());
            shp_found += 1;
        }
    }
    eprintln!("SHPs found: {}/{}", shp_found, shp_names.len());

    // Check for terrain tiles (TMP files used by the map renderer)
    let tmp_names: &[&str] = &[
        "clat01.tem",
        "clat02.tem",
        "shore01.tem",
        "s1.tem",
        "clat01.sno",
        "clat02.sno",
        "shore01.sno",
        "s1.sno",
        "clat01.urb",
        "shore01.urb",
    ];
    let mut tmp_found: usize = 0;
    for &name in tmp_names {
        if let Some(data) = asset_manager.get(name) {
            eprintln!("  TMP found: {} ({} bytes)", name, data.len());
            tmp_found += 1;
        }
    }
    eprintln!("TMPs found: {}/{}", tmp_found, tmp_names.len());

    // Check for rules.ini (critical game data file)
    if let Some(data) = asset_manager.get("rules.ini") {
        eprintln!("  rules.ini found! ({} bytes)", data.len());
    } else {
        eprintln!("  rules.ini NOT found");
    }
    if let Some(data) = asset_manager.get("art.ini") {
        eprintln!("  art.ini found! ({} bytes)", data.len());
    } else {
        eprintln!("  art.ini NOT found");
    }
}

#[test]
fn test_find_theater_ini_in_mix_chain() {
    let dir: PathBuf = require_ra2_dir!();
    let asset_manager =
        crate::assets::asset_manager::AssetManager::new(&dir).expect("Should load AssetManager");

    // Theater INI files live inside local.mix (nested in ra2.mix),
    // or localmd.mix (nested in ra2md.mix) for Yuri's Revenge installs.
    let ini_names: &[&str] = &[
        "temperat.ini",
        "snow.ini",
        "urban.ini",
        "temperatmd.ini",
        "snowmd.ini",
        "urbanmd.ini",
    ];
    let mut found: usize = 0;
    for &name in ini_names {
        if let Some(data) = asset_manager.get(name) {
            let text: &str = std::str::from_utf8(&data).unwrap_or("<non-utf8>");
            let has_tileset: bool = text.contains("[TileSet");
            eprintln!(
                "  Found {} ({} bytes, has TileSet sections: {})",
                name,
                data.len(),
                has_tileset
            );
            found += 1;
        } else {
            eprintln!("  {} NOT found in MIX chain", name);
        }
    }
    eprintln!("Theater INIs found: {}/{}", found, ini_names.len());
}
