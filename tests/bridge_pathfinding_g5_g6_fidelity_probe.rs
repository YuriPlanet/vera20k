//! Fidelity probe for the two deferred bridge-pathfinding gaps:
//!
//! - **G5** — diff-1 SlopeIndex gate in CheckBridgeTraversal. gamemd reads the
//!   LOWER cell's SlopeIndex byte (CellClass+0x11C, == TMP_ReadSlopeType byte)
//!   on every |level_diff|==1 step. If the lower cell's SlopeIndex is 0
//!   (cliff fallback / no canonical ramp), the step is blocked (return code 7).
//!   Our Rust A* has no equivalent gate.
//!
//! - **G6** — two-pass Can_Enter_Cell at bridgeheads. gamemd uses a 3-step
//!   state machine: pre-vtable object-list decision, ground occupancy snapshot,
//!   then a post-vtable conditional bridge-layer overwrite gated by
//!   `targetHeight == cell.Level + 4 AND cell has flag 0x100`. Our Rust decides
//!   layer once at A* push-time.
//!
//! The point of this test is NOT to assert behavior. It is to scan retail YR
//! and RA2 maps to see whether the gaps actually fire — if neither does in
//! retail terrain, both can be deferred indefinitely. See
//! `ra2-rust-game-docs/BRIDGE_DEFERRED_MECHANICS_GHIDRA_REPORT.md` for the
//! full RE source.
//!
//! Run with:
//!   cargo test --test bridge_pathfinding_g5_g6_fidelity_probe -- --ignored --nocapture

use std::path::Path;
use vera20k::assets::asset_manager::{AssetManager, MediaArchiveMode};
use vera20k::map::map_file::{self, MapFile};
use vera20k::map::resolved_terrain::{ResolvedTerrainCell, ResolvedTerrainGrid};
use vera20k::map::theater::{TheaterData, load_theater};
use vera20k::rules::ini_parser::IniFile;
use vera20k::rules::overlay_types::OverlayTypeRegistry;
use vera20k::rules::terrain_rules::TerrainRules;

/// The retail install named by `RA2_DIR`; empty when it is unset, so the test skips.
fn ra2_dir() -> String {
    std::env::var("RA2_DIR").unwrap_or_default()
}

/// 8-directional neighbor offsets (matches A*'s ortho + diagonal step set).
const NEIGHBOR_OFFSETS: [(i32, i32); 8] = [
    (1, 0),
    (-1, 0),
    (0, 1),
    (0, -1),
    (1, 1),
    (1, -1),
    (-1, 1),
    (-1, -1),
];

#[derive(Default, Debug)]
struct G5Report {
    /// Diff-1 pairs where the LOWER cell has slope_type == 0 AND both cells
    /// are otherwise treated as walkable by Rust (`ground_walk_blocked==false`).
    /// gamemd would block this step; Rust currently permits it.
    rust_permits_gamemd_blocks: u32,
    /// Subset of `rust_permits_gamemd_blocks` where BOTH cells in the pair have
    /// a resolved `tileset_index` — i.e., we trust the slope_type byte. This is
    /// the high-confidence count.
    rust_permits_gamemd_blocks_trusted: u32,
    /// Same pair but the LOWER cell has nonzero slope_type. Both engines
    /// permit; informational baseline for "how many diff-1 pairs exist".
    rust_permits_gamemd_permits: u32,
    /// Diff-1 pairs where Rust ALREADY blocks one side via other gates
    /// (ground_walk_blocked, water, cliff). These cannot produce a divergence
    /// even if G5 were implemented — informational only.
    rust_already_blocks: u32,
    /// Up to a handful of example coordinates that fire the divergence
    /// AND are high-confidence (both cells have resolved metadata).
    examples: Vec<G5Example>,
}

#[derive(Default, Debug)]
struct MetadataHealth {
    /// Fraction of grid cells (0..1) whose TMP metadata was successfully resolved
    /// (tileset_index is Some). Low values mean the theater INI's tileset bounds
    /// don't cover this map's tile IDs — slope_type readings are unreliable.
    resolved_fraction: f32,
}

#[derive(Debug)]
struct G5Example {
    lower: (u16, u16, u8),
    upper: (u16, u16, u8),
    lower_slope: u8,
}

#[derive(Default, Debug)]
struct G6Report {
    /// Total bridgehead cells in this map (bridge_transition == true).
    /// This is the upper-bound surface area where the pre/post-vtable
    /// divergence could in principle occur.
    bridgehead_cells: u32,
    /// Bridge-deck cells (has_bridge_deck == true). For scale context.
    deck_cells: u32,
}

fn metadata_health(grid: &ResolvedTerrainGrid) -> MetadataHealth {
    let mut total = 0u32;
    let mut resolved = 0u32;
    for cell in grid.iter() {
        total += 1;
        if cell.tileset_index.is_some() {
            resolved += 1;
        }
    }
    let frac = if total == 0 {
        0.0
    } else {
        resolved as f32 / total as f32
    };
    MetadataHealth {
        resolved_fraction: frac,
    }
}

fn scan_g5(grid: &ResolvedTerrainGrid) -> G5Report {
    let mut report = G5Report::default();
    let w = grid.width();
    let h = grid.height();
    for y in 0..h {
        for x in 0..w {
            let Some(a) = grid.cell(x, y) else { continue };
            for (dx, dy) in NEIGHBOR_OFFSETS {
                let nx = x as i32 + dx;
                let ny = y as i32 + dy;
                if nx < 0 || ny < 0 || nx >= w as i32 || ny >= h as i32 {
                    continue;
                }
                // De-duplicate each pair: only count when (a) is the canonical
                // "first" coord (lower row, or same row + lower column).
                if (ny, nx) < (y as i32, x as i32) {
                    continue;
                }
                let Some(b) = grid.cell(nx as u16, ny as u16) else {
                    continue;
                };
                let level_diff = (a.level as i16) - (b.level as i16);
                if level_diff.abs() != 1 {
                    continue;
                }
                let (lower, upper): (&ResolvedTerrainCell, &ResolvedTerrainCell) =
                    if level_diff < 0 { (a, b) } else { (b, a) };
                // If either side is already blocked in Rust, G5 cannot produce
                // a new divergence — the path is rejected for other reasons.
                if a.ground_walk_blocked || b.ground_walk_blocked {
                    report.rust_already_blocks += 1;
                    continue;
                }
                if lower.slope_type == 0 {
                    report.rust_permits_gamemd_blocks += 1;
                    // High confidence: both cells have resolved tile metadata, so the
                    // slope_type byte we just read came from a real TMP file, not from
                    // the default-on-missing fallback.
                    let trusted = lower.tileset_index.is_some() && upper.tileset_index.is_some();
                    if trusted {
                        report.rust_permits_gamemd_blocks_trusted += 1;
                        if report.examples.len() < 8 {
                            report.examples.push(G5Example {
                                lower: (lower.rx, lower.ry, lower.level),
                                upper: (upper.rx, upper.ry, upper.level),
                                lower_slope: lower.slope_type,
                            });
                        }
                    }
                } else {
                    report.rust_permits_gamemd_permits += 1;
                }
            }
        }
    }
    report
}

fn scan_g6(grid: &ResolvedTerrainGrid) -> G6Report {
    let mut report = G6Report::default();
    for cell in grid.iter() {
        if cell.bridge_transition {
            report.bridgehead_cells += 1;
        }
        if cell.has_bridge_deck {
            report.deck_cells += 1;
        }
    }
    report
}

fn build_grid_for_map(
    am: &mut AssetManager,
    map: &MapFile,
    rules_ini: &IniFile,
) -> Option<ResolvedTerrainGrid> {
    let theater_name = &map.header.theater;
    let theater: Option<TheaterData> = load_theater(am, theater_name);
    if theater.is_none() {
        eprintln!("  WARN: theater '{}' not loadable", theater_name);
    }
    let terrain_rules = TerrainRules::from_ini(rules_ini);
    let overlay_registry = OverlayTypeRegistry::from_ini(rules_ini, None);
    Some(ResolvedTerrainGrid::build(
        map,
        theater.as_ref(),
        Some(am),
        Some(&terrain_rules),
        Some(&overlay_registry),
        true,
        2,
    ))
}

#[test]
#[ignore]
fn probe_g5_g6_against_retail_maps() {
    let _ = env_logger::try_init();
    let dir_str = ra2_dir();
    let ra2_dir = Path::new(&dir_str);
    if !ra2_dir.exists() {
        eprintln!("SKIP: RA2 dir not found at {}", dir_str);
        return;
    }
    let mut am =
        AssetManager::new(ra2_dir, MediaArchiveMode::STOCK_DIGITAL).expect("AssetManager::new");
    // Pull in standard mix archives where map files and theater data live.
    for mix in &[
        "maps01.mix",
        "maps02.mix",
        "maps03.mix",
        "mapsmd01.mix",
        "mapsmd02.mix",
        "mapsmd03.mix",
    ] {
        let _ = am.load_nested(mix);
    }
    let _ = am.load_all_disk_mixes();

    let rules_bytes = am
        .get("rulesmd.ini")
        .or_else(|| am.get("rules.ini"))
        .expect("rules ini required");
    let rules_ini = IniFile::from_bytes(&rules_bytes).expect("rules parse");

    // Enumerate every loose .mmx / .yro / .map / .mpr file in the install dir.
    // These are the retail multiplayer + campaign maps shipped on disk.
    let mut candidates: Vec<String> = Vec::new();
    if let Ok(entries) = std::fs::read_dir(ra2_dir) {
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            let lower = name.to_ascii_lowercase();
            if lower.ends_with(".mmx")
                || lower.ends_with(".yro")
                || lower.ends_with(".map")
                || lower.ends_with(".mpr")
            {
                candidates.push(name);
            }
        }
    }
    candidates.sort();

    println!("Found {} candidate map files", candidates.len());

    let mut total_g5_fire = 0u32;
    let mut total_g5_fire_trusted = 0u32;
    let mut total_g5_diff1_pairs = 0u32;
    let mut total_g6_bridgeheads = 0u32;
    let mut maps_with_g5_fire: Vec<(String, u32, u32, f32, G5Example)> = Vec::new();
    let mut maps_processed = 0usize;
    let mut maps_skipped: Vec<String> = Vec::new();

    for name in &candidates {
        let path = ra2_dir.join(name);
        let map = match map_file::load_from_path(&path) {
            Ok(m) => m,
            Err(e) => {
                maps_skipped.push(format!("{}: {}", name, e));
                continue;
            }
        };
        process_map(
            name,
            map,
            &mut am,
            &rules_ini,
            &mut total_g5_fire,
            &mut total_g5_fire_trusted,
            &mut total_g5_diff1_pairs,
            &mut total_g6_bridgeheads,
            &mut maps_with_g5_fire,
            &mut maps_processed,
            &mut maps_skipped,
        );
    }

    println!();
    println!("=== SUMMARY ===");
    println!("Maps processed: {}", maps_processed);
    println!("Maps skipped:   {}", maps_skipped.len());
    for s in &maps_skipped {
        println!("  - {}", s);
    }
    println!();
    println!("G5 (diff-1 SlopeIndex==0 on LOWER cell, Rust permits, gamemd blocks):");
    println!(
        "  Total diff-1 walkable pairs across all maps: {}",
        total_g5_diff1_pairs
    );
    println!(
        "  Total raw firing pairs:                      {}",
        total_g5_fire
    );
    println!(
        "  Total TRUSTED firing pairs (both cells have resolved TMP metadata): {}",
        total_g5_fire_trusted
    );
    println!(
        "  Maps with at least one TRUSTED firing pair:  {}",
        maps_with_g5_fire.len()
    );
    println!("  (trusted=both cells resolved; raw includes cells where metadata is missing");
    println!("   and slope_type defaults to 0 — false positives.)");
    for (map, raw, trusted, meta_frac, ex) in &maps_with_g5_fire {
        println!(
            "    {:24} raw={:5} trusted={:5} resolved-meta={:5.1}%  example: lower=({:3},{:3},lvl{}) upper=({:3},{:3},lvl{}) slope={}",
            map,
            raw,
            trusted,
            meta_frac * 100.0,
            ex.lower.0,
            ex.lower.1,
            ex.lower.2,
            ex.upper.0,
            ex.upper.1,
            ex.upper.2,
            ex.lower_slope,
        );
    }
    println!();
    println!("G6 (bridgehead cells — upper-bound surface area for pre/post-vtable divergence):");
    println!(
        "  Total bridgehead cells across all maps: {}",
        total_g6_bridgeheads
    );
    println!(
        "  NOTE: G6 divergence requires runtime simultaneous ground+bridge layer occupancy on"
    );
    println!("        a bridgehead cell. Stock maps do not preplace such configurations; whether");
    println!(
        "        normal play produces them is a runtime question not answerable from terrain."
    );
    println!();
    println!("=== INTERPRETATION ===");
    if total_g5_fire_trusted == 0 {
        println!("G5: does NOT fire on any retail map (trusted count = 0). The diff-1");
        println!("    SlopeIndex gate has zero observable surface area — every diff-1");
        println!("    step on retail maps either has nonzero slope on the lower cell OR");
        println!("    is already blocked by other gates.");
        println!("    Recommendation: defer G5 indefinitely.");
    } else {
        println!(
            "G5: SCAN-ONLY ({} maps, {} TRUSTED pairs). Legality gate landed 2026-05-12.",
            maps_with_g5_fire.len(),
            total_g5_fire_trusted,
        );
        println!("    The raw counts above are useful for regression monitoring — A* now",);
        println!("    blocks these pairs. Run `probe_g5_astar_rejects_all_trusted_firing_pairs`",);
        println!("    for empirical confirmation.");
    }
    if total_g6_bridgeheads == 0 {
        println!("G6: no bridgehead cells found — divergence impossible. Defer.");
    } else {
        println!(
            "G6: {} bridgehead cells exist across {} maps. Divergence is bounded to that",
            total_g6_bridgeheads, maps_processed,
        );
        println!("    surface area AND requires simultaneous-layer occupancy at runtime. Not");
        println!("    answerable from terrain alone — needs a constructed scripted scenario.");
    }
}

fn process_map(
    name: &str,
    map: MapFile,
    am: &mut AssetManager,
    rules_ini: &IniFile,
    total_g5_fire: &mut u32,
    total_g5_fire_trusted: &mut u32,
    total_g5_diff1_pairs: &mut u32,
    total_g6_bridgeheads: &mut u32,
    maps_with_g5_fire: &mut Vec<(String, u32, u32, f32, G5Example)>,
    maps_processed: &mut usize,
    maps_skipped: &mut Vec<String>,
) {
    if map.cells.is_empty() {
        maps_skipped.push(format!("{}: empty cell list", name));
        return;
    }
    let Some(grid) = build_grid_for_map(am, &map, rules_ini) else {
        maps_skipped.push(format!("{}: grid build failed", name));
        return;
    };
    if grid.width() == 0 || grid.height() == 0 {
        maps_skipped.push(format!("{}: zero-size grid", name));
        return;
    }
    let meta = metadata_health(&grid);
    let g5 = scan_g5(&grid);
    let g6 = scan_g6(&grid);
    *maps_processed += 1;
    *total_g5_diff1_pairs += g5.rust_permits_gamemd_blocks + g5.rust_permits_gamemd_permits;
    *total_g5_fire += g5.rust_permits_gamemd_blocks;
    *total_g5_fire_trusted += g5.rust_permits_gamemd_blocks_trusted;
    *total_g6_bridgeheads += g6.bridgehead_cells;
    println!(
        "{:24} theater={:10} {:3}x{:3} meta={:5.1}%  G5 raw={:5} trusted={:5} / {:5} diff-1  G6 heads={:3} decks={:4}",
        name,
        map.header.theater,
        grid.width(),
        grid.height(),
        meta.resolved_fraction * 100.0,
        g5.rust_permits_gamemd_blocks,
        g5.rust_permits_gamemd_blocks_trusted,
        g5.rust_permits_gamemd_blocks + g5.rust_permits_gamemd_permits,
        g6.bridgehead_cells,
        g6.deck_cells,
    );
    if g5.rust_permits_gamemd_blocks_trusted > 0
        && let Some(first) = g5.examples.into_iter().next()
    {
        maps_with_g5_fire.push((
            name.to_string(),
            g5.rust_permits_gamemd_blocks,
            g5.rust_permits_gamemd_blocks_trusted,
            meta.resolved_fraction,
            first,
        ));
    }
}

#[test]
#[ignore]
fn probe_g5_astar_rejects_all_trusted_firing_pairs() {
    // Sibling of `probe_g5_g6_against_retail_maps` that closes the parity
    // accountability loop: the terrain scan reports firing pairs, but only A*
    // can answer whether the gate actually rejects them. For each trusted
    // example coord from `scan_g5`, this test builds a PathGrid and calls
    // A* across the pair, asserting either no path or a detour that does
    // not include the direct cliff step.
    //
    // Coverage is sampled, not exhaustive: scan_g5 keeps at most 8 examples
    // per map. That's broad enough to surface a regression in any retail
    // theater but won't catch every individual pair.
    let _ = env_logger::try_init();
    let dir_str = ra2_dir();
    let ra2_dir = Path::new(&dir_str);
    if !ra2_dir.exists() {
        eprintln!("SKIP: RA2 dir not found at {}", dir_str);
        return;
    }
    let mut am =
        AssetManager::new(ra2_dir, MediaArchiveMode::STOCK_DIGITAL).expect("AssetManager::new");
    for mix in &[
        "maps01.mix",
        "maps02.mix",
        "maps03.mix",
        "mapsmd01.mix",
        "mapsmd02.mix",
        "mapsmd03.mix",
    ] {
        let _ = am.load_nested(mix);
    }
    let _ = am.load_all_disk_mixes();

    let rules_bytes = am
        .get("rulesmd.ini")
        .or_else(|| am.get("rules.ini"))
        .expect("rules ini required");
    let rules_ini = IniFile::from_bytes(&rules_bytes).expect("rules parse");

    let mut candidates: Vec<String> = Vec::new();
    if let Ok(entries) = std::fs::read_dir(ra2_dir) {
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            let lower = name.to_ascii_lowercase();
            if lower.ends_with(".mmx")
                || lower.ends_with(".yro")
                || lower.ends_with(".map")
                || lower.ends_with(".mpr")
            {
                candidates.push(name);
            }
        }
    }
    candidates.sort();

    let mut total_pairs = 0u32;
    let mut rejected_pairs = 0u32;
    type LeakedPair = (String, (u16, u16), (u16, u16));
    let mut leaked_pairs: Vec<LeakedPair> = Vec::new();
    let mut maps_scanned = 0u32;

    for name in &candidates {
        let path = ra2_dir.join(name);
        let Ok(map) = map_file::load_from_path(&path) else {
            continue;
        };
        if map.cells.is_empty() {
            continue;
        }
        let Some(grid) = build_grid_for_map(&mut am, &map, &rules_ini) else {
            continue;
        };
        if grid.width() == 0 || grid.height() == 0 {
            continue;
        }

        let meta = metadata_health(&grid);
        if meta.resolved_fraction < 0.5 {
            // Skip maps where TMP metadata didn't resolve. Slope readings
            // would be unreliable (DESERT/NEWURBAN/LUNAR theaters today).
            continue;
        }

        let path_grid =
            vera20k::sim::pathfinding::PathGrid::from_resolved_terrain_with_bridges(&grid);

        let g5 = scan_g5(&grid);
        if g5.examples.is_empty() {
            continue;
        }
        maps_scanned += 1;
        for ex in &g5.examples {
            total_pairs += 1;
            let lower_xy = (ex.lower.0, ex.lower.1);
            let upper_xy = (ex.upper.0, ex.upper.1);
            let path = vera20k::sim::pathfinding::find_path(&path_grid, lower_xy, upper_xy);
            match path {
                None => rejected_pairs += 1,
                Some(p) => {
                    let direct_step = p.windows(2).any(|w| {
                        (w[0] == lower_xy && w[1] == upper_xy)
                            || (w[0] == upper_xy && w[1] == lower_xy)
                    });
                    if direct_step {
                        leaked_pairs.push((name.clone(), lower_xy, upper_xy));
                    } else {
                        rejected_pairs += 1;
                    }
                }
            }
        }
    }

    println!();
    println!("=== G5 A* rejection probe ===");
    println!(
        "Maps scanned (>=50% resolved TMP metadata AND >=1 firing example): {}",
        maps_scanned
    );
    println!(
        "Tested pairs (sampled from each map's example set, max 8/map):     {}",
        total_pairs
    );
    println!(
        "Pairs rejected or detoured:                                        {}",
        rejected_pairs
    );
    println!(
        "Pairs LEAKED (A* still took the direct cliff step):                {}",
        leaked_pairs.len()
    );
    for (m, lo, up) in &leaked_pairs {
        println!("  {} : {:?} -> {:?}", m, lo, up);
    }
    assert!(
        leaked_pairs.is_empty(),
        "A* must not step across diff-1 SlopeIndex==0 firing pairs"
    );
}
