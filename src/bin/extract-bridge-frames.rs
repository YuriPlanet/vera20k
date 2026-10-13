//! One-off tool to extract frames from `bridge.tem` for visual bridge-axis
//! inspection. Dumps frames 0 and 9 (the two healthy-bridge body frame
//! families) as PNG files into the cwd.
//!
//! Background: the physical sprite orientation labels and Rust's runtime
//! `Axis` labels have been easy to conflate. This tool only shows what the
//! raw asset frames look like; renderer code must follow the cell damage-state
//! byte family used by `DamageState::to_state_byte(axis)`.
//!
//! Usage: `cargo run --bin extract-bridge-frames`

use vera20k::asset_tools::root::resolve_ra2_dir;
use vera20k::assets::asset_manager::{AssetManager, MediaArchiveMode};
use vera20k::assets::pal_file::Palette;
use vera20k::assets::shp_file::ShpFile;

fn main() {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("warn")).init();

    let (ra2_dir, _) = resolve_ra2_dir(None).unwrap_or_else(|error| panic!("{error}"));
    println!("Loading MIX archives from {}...", ra2_dir.display());
    let asset_manager = AssetManager::new(&ra2_dir, MediaArchiveMode::STOCK_DIGITAL)
        .expect("Failed to load MIX archives");

    // 1. Load bridge.tem
    let (bridge_bytes, source) = asset_manager
        .get_with_source("bridge.tem")
        .expect("bridge.tem not found in any MIX archive");
    println!("bridge.tem: {} bytes from {}", bridge_bytes.len(), source);

    let shp = ShpFile::from_bytes(&bridge_bytes).expect("Failed to parse bridge.tem as SHP");
    println!(
        "  SHP header: canvas {}x{}, {} frames",
        shp.width,
        shp.height,
        shp.frames.len()
    );

    // 2. Load ISOTEM.PAL (temperate theater palette for bridge rendering)
    let (pal_bytes, pal_source) = asset_manager
        .get_with_source("isotem.pal")
        .expect("isotem.pal not found");
    println!("isotem.pal: {} bytes from {}", pal_bytes.len(), pal_source);
    let palette = Palette::from_bytes(&pal_bytes).expect("Failed to parse isotem.pal");

    // 3. Dump frames 0 and 9, plus a few neighbors for context
    let frames_to_dump = [0usize, 1, 2, 3, 9, 10, 11, 12, 18, 27];
    for &idx in &frames_to_dump {
        if idx >= shp.frames.len() {
            println!("  skip frame {idx} (out of range)");
            continue;
        }
        let frame = &shp.frames[idx];
        println!(
            "  frame {idx}: frame_xy=({},{})  size={}x{}",
            frame.frame_x, frame.frame_y, frame.frame_width, frame.frame_height
        );

        if frame.frame_width == 0 || frame.frame_height == 0 {
            println!("    (empty frame, skipping PNG)");
            continue;
        }

        // Decode to RGBA at the FRAME's native size (small image).
        let rgba_frame: Vec<u8> = shp
            .frame_to_rgba(idx, &palette)
            .expect("Should convert to RGBA");

        // Save the small frame.
        let path_frame = format!("bridge_frame_{idx:02}_raw.png");
        image::save_buffer(
            &path_frame,
            &rgba_frame,
            frame.frame_width as u32,
            frame.frame_height as u32,
            image::ColorType::Rgba8,
        )
        .expect("Failed to save frame PNG");
        println!("    wrote {path_frame}");

        // Also composite onto the full 180x180 canvas so we can see where
        // the frame sits within the SHP's full bounds (the frame_y offset
        // is what produces the EW-vs-NS visual height difference).
        let canvas_w = shp.width as usize;
        let canvas_h = shp.height as usize;
        let mut canvas: Vec<u8> = vec![0u8; canvas_w * canvas_h * 4];

        let fx = frame.frame_x as usize;
        let fy = frame.frame_y as usize;
        let fw = frame.frame_width as usize;
        let fh = frame.frame_height as usize;
        for row in 0..fh {
            let cy = fy + row;
            if cy >= canvas_h {
                break;
            }
            for col in 0..fw {
                let cx = fx + col;
                if cx >= canvas_w {
                    break;
                }
                let src = (row * fw + col) * 4;
                let dst = (cy * canvas_w + cx) * 4;
                canvas[dst] = rgba_frame[src];
                canvas[dst + 1] = rgba_frame[src + 1];
                canvas[dst + 2] = rgba_frame[src + 2];
                canvas[dst + 3] = rgba_frame[src + 3];
            }
        }

        let path_canvas = format!("bridge_frame_{idx:02}_canvas.png");
        image::save_buffer(
            &path_canvas,
            &canvas,
            canvas_w as u32,
            canvas_h as u32,
            image::ColorType::Rgba8,
        )
        .expect("Failed to save canvas PNG");
        println!("    wrote {path_canvas}");
    }

    println!("\nDone. Inspect bridge_frame_00_*.png and bridge_frame_09_*.png.");
    println!("These files show raw physical sprite orientation only.");
    println!("Runtime rendering should follow state bytes: Axis::NS => 0..8, Axis::EW => 9..17.");
}
