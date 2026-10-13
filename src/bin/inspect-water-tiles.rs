//! One-off tool to inspect Water01-Water14.tem pixel data for "starlight"-like
//! bright pixels that might explain the perceived sparkle-on-water effect at night.
//!
//! Outputs:
//! - PNG of each tile in fullbright
//! - PNG of each tile at low ambient (simulates night)
//! - Stats: bright-pixel count, positions, luminance histogram
//!
//! Usage: `cargo run --bin inspect-water-tiles`

use vera20k::asset_tools::root::resolve_ra2_dir;
use vera20k::assets::asset_manager::{AssetManager, MediaArchiveMode};
use vera20k::assets::pal_file::Palette;
use vera20k::assets::tmp_file::TmpFile;

const BRIGHT_THRESHOLD: u32 = 600; // R+G+B > 600 (out of 765) → "very bright"
const NIGHT_AMBIENT: f32 = 0.30; // 30% brightness, typical night map

fn main() {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("warn")).init();

    let (ra2_dir, _) = resolve_ra2_dir(None).unwrap_or_else(|error| panic!("{error}"));
    println!("Loading MIX archives from {}...", ra2_dir.display());
    let asset_manager = AssetManager::new(&ra2_dir, MediaArchiveMode::STOCK_DIGITAL)
        .expect("Failed to load MIX archives");

    // Load the temperate iso palette
    let (pal_bytes, pal_source) = asset_manager
        .get_with_source("isotem.pal")
        .expect("isotem.pal not found");
    println!("isotem.pal: {} bytes from {}", pal_bytes.len(), pal_source);
    let palette = Palette::from_bytes(&pal_bytes).expect("Failed to parse isotem.pal");

    // Find very bright palette indices — candidates for "starlight" pixels
    println!("\n=== Palette luminance survey ===");
    println!("Indices with R+G+B > {BRIGHT_THRESHOLD}:");
    let mut bright_indices: Vec<u8> = Vec::new();
    for (i, c) in palette.colors.iter().enumerate() {
        let lum = c.r as u32 + c.g as u32 + c.b as u32;
        if lum > BRIGHT_THRESHOLD {
            println!(
                "  [{:3}] RGB=({:3},{:3},{:3}) lum={}",
                i, c.r, c.g, c.b, lum
            );
            bright_indices.push(i as u8);
        }
    }
    println!("Total bright indices: {}\n", bright_indices.len());

    // Iterate Water01.tem through Water14.tem + a/b/c/d variants
    for n in 1..=14u32 {
        for suffix in ["", "a", "b", "c", "d"] {
            let name = format!("water{n:02}{suffix}.tem");
            let bytes = match asset_manager.get_with_source(&name) {
                Some((b, src)) => {
                    println!("=== {} ({} bytes from {}) ===", name, b.len(), src);
                    b
                }
                None => {
                    println!("=== {} NOT FOUND ===", name);
                    continue;
                }
            };

            let tmp = match TmpFile::from_bytes(&bytes) {
                Ok(t) => t,
                Err(e) => {
                    println!("  parse error: {:?}", e);
                    continue;
                }
            };

            println!(
                "  template {}x{} cells, tile {}x{} px, total cells={}",
                tmp.template_width,
                tmp.template_height,
                tmp.tile_width,
                tmp.tile_height,
                tmp.tiles.len()
            );
            // Dump per-cell flags for animation/extra/damaged detection
            for (i, t_opt) in tmp.tiles.iter().enumerate() {
                if let Some(t) = t_opt {
                    println!(
                        "    cell[{i}]: pixel_size={}x{} offset=({},{}) has_damaged={} terrain_type={} ramp={}",
                        t.pixel_width,
                        t.pixel_height,
                        t.offset_x,
                        t.offset_y,
                        t.has_damaged_data,
                        t.terrain_type,
                        t.ramp_type
                    );
                }
            }

            // Per-tile stats + composite all sub-tiles into one image
            let cells_w = tmp.template_width as usize;
            let cells_h = tmp.template_height as usize;
            let tile_w = tmp.tile_width as usize;
            let tile_h = tmp.tile_height as usize;

            // Composite canvas: half-cell stagger isometric layout
            let canvas_w = cells_w * tile_w + tile_w / 2;
            let canvas_h = cells_h * tile_h / 2 + tile_h;
            let mut canvas_day: Vec<u8> = vec![0u8; canvas_w * canvas_h * 4];
            let mut canvas_night: Vec<u8> = vec![0u8; canvas_w * canvas_h * 4];

            let mut total_pixels: u32 = 0;
            let mut bright_pixel_count: u32 = 0;
            let mut bright_positions: Vec<(usize, usize, u8)> = Vec::new();

            for (cell_idx, tile_opt) in tmp.tiles.iter().enumerate() {
                let Some(tile) = tile_opt else {
                    continue;
                };
                let col = cell_idx % cells_w;
                let row = cell_idx / cells_w;

                // Iso layout: even rows at x=col*tile_w, odd rows offset by tile_w/2
                let base_x = col * tile_w + (row & 1) * (tile_w / 2);
                let base_y = row * tile_h / 2;

                let pw = tile.pixel_width as usize;
                let ph = tile.pixel_height as usize;

                for py in 0..ph {
                    for px in 0..pw {
                        let idx = tile.pixels[py * pw + px];
                        if idx == 0 {
                            continue;
                        } // transparent
                        total_pixels += 1;
                        let c = palette.colors[idx as usize];
                        let lum = c.r as u32 + c.g as u32 + c.b as u32;
                        let is_bright = lum > BRIGHT_THRESHOLD;
                        if is_bright {
                            bright_pixel_count += 1;
                            bright_positions.push((px, py, idx));
                        }

                        // Place into canvases
                        let cx = base_x + px + tile.offset_x as usize;
                        let cy = base_y + py + tile.offset_y as usize;
                        if cx < canvas_w && cy < canvas_h {
                            let off = (cy * canvas_w + cx) * 4;
                            canvas_day[off] = c.r;
                            canvas_day[off + 1] = c.g;
                            canvas_day[off + 2] = c.b;
                            canvas_day[off + 3] = 255;

                            let dim_r = ((c.r as f32) * NIGHT_AMBIENT) as u8;
                            let dim_g = ((c.g as f32) * NIGHT_AMBIENT) as u8;
                            let dim_b = ((c.b as f32) * NIGHT_AMBIENT) as u8;
                            canvas_night[off] = dim_r;
                            canvas_night[off + 1] = dim_g;
                            canvas_night[off + 2] = dim_b;
                            canvas_night[off + 3] = 255;
                        }
                    }
                }
            }

            println!(
                "  total opaque pixels: {}, bright (lum > {}): {} ({:.2}%)",
                total_pixels,
                BRIGHT_THRESHOLD,
                bright_pixel_count,
                100.0 * bright_pixel_count as f32 / total_pixels.max(1) as f32
            );
            if !bright_positions.is_empty() && bright_positions.len() <= 30 {
                println!("  bright pixel positions (px, py, palidx):");
                for (px, py, idx) in &bright_positions {
                    let c = palette.colors[*idx as usize];
                    println!(
                        "    ({:3},{:2}) idx={:3} RGB=({},{},{})",
                        px, py, idx, c.r, c.g, c.b
                    );
                }
            } else if bright_positions.len() > 30 {
                println!("  (>30 bright pixels — too many to list)");
            }

            // Save day + night PNG
            let day_path = format!("water_{n:02}_day.png");
            image::save_buffer(
                &day_path,
                &canvas_day,
                canvas_w as u32,
                canvas_h as u32,
                image::ColorType::Rgba8,
            )
            .expect("save day PNG");
            println!("  wrote {day_path}");

            let night_path = format!("water_{n:02}_night.png");
            image::save_buffer(
                &night_path,
                &canvas_night,
                canvas_w as u32,
                canvas_h as u32,
                image::ColorType::Rgba8,
            )
            .expect("save night PNG");
            println!("  wrote {night_path}\n");
        } // for suffix
    }

    println!("=== Done ===");
}
