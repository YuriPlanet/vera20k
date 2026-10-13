//! Diagnostic: dump SHROUD.SHP frame layouts to inspect canvas-space pixel placement.
//!
//! SHROUD.SHP holds 47 brightness frames used at revealed/unrevealed boundaries.
//! Pixel values are NOT palette indices — they are direct ABuffer writes:
//!   0x00 = full black, 0x7F = neutral, 0xFE = transparent (skip).
//!
//! Each frame has a sub-rectangle (frame_x, frame_y, frame_width, frame_height)
//! within an overall 60×30 canvas. This test writes one PNG per frame plus a
//! contact sheet that shows where in the canvas each frame's pixels actually live,
//! relative to the diamond center (row 14/15 in a 60×30 cell). That reveals whether
//! gamemd's blit anchor (cell_center - (30, 15)) lines the frame data up correctly.
//!
//! Run with: `cargo test --test shroud_shp_dump -- --ignored --nocapture`
//! Output goes to `<repo>/exported_shp_pngs/shroud_dump/`.

use std::fs;
use std::path::PathBuf;

use image::RgbaImage;
use vera20k::assets::asset_manager::{AssetManager, MediaArchiveMode};
use vera20k::assets::shp_file::ShpFile;
use vera20k::render::shroud_buffer::{SHROUD_EDGE_LUT, extract_shp_brightness};

const ROOT_DIR: &str = env!("CARGO_MANIFEST_DIR");
const OUTPUT_SUBDIR: &str = "exported_shp_pngs/shroud_dump";

const PIXEL_SCALE: u32 = 4;
const FRAME_GAP: u32 = 8;
const FRAMES_PER_ROW: u32 = 8;

fn ra2_dir() -> Option<PathBuf> {
    std::env::var_os("RA2_DIR").map(PathBuf::from)
}

/// Color a brightness byte for the diagnostic output.
fn color_for_pixel(value: u8) -> [u8; 4] {
    match value {
        0xFE => [40, 40, 60, 255],    // transparent — dim blue-gray
        0x00 => [0, 0, 0, 255],       // full shroud — black
        0x7F => [255, 255, 255, 255], // full clear — white
        v if v < 0x7F => {
            // partial shroud — darker for smaller values
            let g = (v as u32 * 255 / 0x7F) as u8;
            [g, g, g, 255]
        }
        _ => [255, 0, 0, 255], // unexpected (>0x7F and not 0xFE) — red
    }
}

/// Draw a 60×30 canvas into the destination image at the given offset, scaled.
fn draw_canvas(
    dst: &mut RgbaImage,
    dst_x: u32,
    dst_y: u32,
    canvas: &[u8],
    canvas_w: u32,
    canvas_h: u32,
) {
    for cy in 0..canvas_h {
        for cx in 0..canvas_w {
            let pixel = canvas[(cy * canvas_w + cx) as usize];
            let color = color_for_pixel(pixel);
            for dy in 0..PIXEL_SCALE {
                for dx in 0..PIXEL_SCALE {
                    let px = dst_x + cx * PIXEL_SCALE + dx;
                    let py = dst_y + cy * PIXEL_SCALE + dy;
                    if px < dst.width() && py < dst.height() {
                        dst.put_pixel(px, py, image::Rgba(color));
                    }
                }
            }
        }
    }

    // Cyan crosshair at canvas center (col 30, between rows 14 and 15) — that's
    // where the cell center lands when anchoring at top-of-bbox.
    let cx_pixel = dst_x + 30 * PIXEL_SCALE;
    let cy_pixel = dst_y + 15 * PIXEL_SCALE;
    let canvas_pixel_w = canvas_w * PIXEL_SCALE;
    let canvas_pixel_h = canvas_h * PIXEL_SCALE;
    for dx in 0..canvas_pixel_w {
        let py = cy_pixel.saturating_sub(0);
        let px = dst_x + dx;
        if px < dst.width() && py < dst.height() {
            let mut p = dst.get_pixel(px, py).0;
            p[0] = p[0].saturating_add(30);
            p[1] = 255;
            p[2] = 255;
            dst.put_pixel(px, py, image::Rgba(p));
        }
    }
    for dy in 0..canvas_pixel_h {
        let px = cx_pixel;
        let py = dst_y + dy;
        if px < dst.width() && py < dst.height() {
            let mut p = dst.get_pixel(px, py).0;
            p[0] = 255;
            p[1] = p[1].saturating_add(30);
            p[2] = 255;
            dst.put_pixel(px, py, image::Rgba(p));
        }
    }
}

/// Pretty-print a frame's 60×30 canvas as ASCII (one char per pixel).
/// '#' = 0x00 (shroud), '.' = 0x7F (clear), ' ' = 0xFE (transparent),
/// '?' = unexpected, digit/letter = intermediate brightness.
fn ascii_canvas(canvas: &[u8], w: u32, h: u32) -> String {
    let mut out = String::with_capacity(((w + 1) * h) as usize);
    for cy in 0..h {
        for cx in 0..w {
            let pixel = canvas[(cy * w + cx) as usize];
            let c = match pixel {
                0xFE => ' ',
                0x00 => '#',
                0x7F => '.',
                v if v < 0x7F => {
                    let bucket = (v as u32 * 9 / 0x7F) as u8;
                    (b'0' + bucket) as char
                }
                _ => '?',
            };
            out.push(c);
        }
        out.push('\n');
    }
    out
}

fn lut_masks_for_frame(frame_idx: u8) -> Vec<u8> {
    (0u32..=255)
        .filter(|m| SHROUD_EDGE_LUT[*m as usize] == frame_idx)
        .map(|m| m as u8)
        .collect()
}

#[test]
#[ignore] // Requires RA2 install
fn dump_shroud_shp_layout() {
    let Some(ra2_dir) = ra2_dir() else {
        eprintln!("SKIP: RA2_DIR not set");
        return;
    };

    let asset_manager =
        AssetManager::new(&ra2_dir, MediaArchiveMode::STOCK_DIGITAL).expect("asset manager");
    let Some((shp_bytes, source)) = asset_manager.get_with_source("shroud.shp") else {
        panic!("shroud.shp not found in any MIX archive");
    };
    let shp = ShpFile::from_bytes(&shp_bytes).expect("parse shroud.shp");

    let output_dir = PathBuf::from(ROOT_DIR).join(OUTPUT_SUBDIR);
    if output_dir.exists() {
        fs::remove_dir_all(&output_dir).expect("clear previous output");
    }
    fs::create_dir_all(&output_dir).expect("create output dir");

    let (frame_canvases, canvas_w, canvas_h) = extract_shp_brightness(&shp);

    eprintln!(
        "SHROUD.SHP loaded from {}: width={} height={} frames={}",
        source,
        shp.width,
        shp.height,
        shp.frames.len()
    );

    // Per-frame text report.
    let mut report = String::new();
    report.push_str(&format!(
        "SHROUD.SHP from: {}\nfile_w={} file_h={} frames={}\ncanvas_w={} canvas_h={}\n\n",
        source,
        shp.width,
        shp.height,
        shp.frames.len(),
        canvas_w,
        canvas_h
    ));

    // Histogram counters.
    let mut frame_summary: Vec<String> = Vec::new();

    for (idx, frame) in shp.frames.iter().enumerate() {
        let canvas = &frame_canvases[idx];

        // Count pixel values inside the canvas.
        let mut n_black = 0u32;
        let mut n_white = 0u32;
        let mut n_transparent = 0u32;
        let mut n_partial = 0u32;
        let mut n_other = 0u32;
        let mut min_y = u32::MAX;
        let mut max_y = 0u32;
        for cy in 0..canvas_h {
            let mut row_has_data = false;
            for cx in 0..canvas_w {
                let p = canvas[(cy * canvas_w + cx) as usize];
                match p {
                    0xFE => n_transparent += 1,
                    0x00 => {
                        n_black += 1;
                        row_has_data = true;
                    }
                    0x7F => {
                        n_white += 1;
                        row_has_data = true;
                    }
                    v if v < 0x7F => {
                        n_partial += 1;
                        row_has_data = true;
                    }
                    _ => n_other += 1,
                }
            }
            if row_has_data {
                if cy < min_y {
                    min_y = cy;
                }
                if cy > max_y {
                    max_y = cy;
                }
            }
        }
        let masks = lut_masks_for_frame(idx as u8);
        let masks_short = if masks.is_empty() {
            "(unused by LUT)".to_string()
        } else if masks.len() <= 6 {
            masks
                .iter()
                .map(|m| format!("0x{:02X}", m))
                .collect::<Vec<_>>()
                .join(",")
        } else {
            let head = masks
                .iter()
                .take(4)
                .map(|m| format!("0x{:02X}", m))
                .collect::<Vec<_>>()
                .join(",");
            format!("{},... ({} total)", head, masks.len())
        };

        frame_summary.push(format!(
            "frame {:02} (0x{:02X}): hdr=(x={}, y={}, w={}, h={})  data_rows=[{}..={}]  px(black={}, white={}, partial={}, transp={}, other={})  LUT={}",
            idx,
            idx,
            frame.frame_x,
            frame.frame_y,
            frame.frame_width,
            frame.frame_height,
            min_y as i32,
            max_y as i32,
            n_black,
            n_white,
            n_partial,
            n_transparent,
            n_other,
            masks_short
        ));

        // Per-frame ASCII canvas dump.
        report.push_str(&format!(
            "===== FRAME {:02} (0x{:02X}) =====\n\
             header: x={}, y={}, w={}, h={}\n\
             canvas data rows: [{}..={}] (out of 0..{})\n\
             pixel counts: black={} white={} partial={} transparent={} other={}\n\
             LUT bitmasks → this frame: {}\n\
             ascii canvas (60×30, '#'=shroud, '.'=clear, ' '=transparent):\n",
            idx,
            idx,
            frame.frame_x,
            frame.frame_y,
            frame.frame_width,
            frame.frame_height,
            min_y as i32,
            max_y as i32,
            canvas_h - 1,
            n_black,
            n_white,
            n_partial,
            n_transparent,
            n_other,
            masks_short
        ));
        // Numbered ruler at the top.
        report.push_str("    0         1         2         3         4         5\n");
        report.push_str("    0123456789012345678901234567890123456789012345678901234567890\n");
        for (line_idx, line) in ascii_canvas(canvas, canvas_w, canvas_h).lines().enumerate() {
            report.push_str(&format!("{:>2}: {}\n", line_idx, line));
        }
        report.push('\n');
    }

    fs::write(output_dir.join("shroud_report.txt"), &report).expect("write report");

    // Summary log to stderr.
    eprintln!("\nFrame summary:");
    for line in &frame_summary {
        eprintln!("  {}", line);
    }

    // Contact sheet PNG.
    let frame_count = shp.frames.len() as u32;
    let cols = FRAMES_PER_ROW;
    let rows = frame_count.div_ceil(cols);
    let cell_w = canvas_w * PIXEL_SCALE;
    let cell_h = canvas_h * PIXEL_SCALE;
    let label_h = 12;
    let total_cell_h = cell_h + label_h;
    let sheet_w = cols * cell_w + (cols + 1) * FRAME_GAP;
    let sheet_h = rows * total_cell_h + (rows + 1) * FRAME_GAP;
    let mut sheet = RgbaImage::from_pixel(sheet_w, sheet_h, image::Rgba([12, 12, 18, 255]));

    for (idx, canvas) in frame_canvases.iter().enumerate() {
        let row = idx as u32 / cols;
        let col = idx as u32 % cols;
        let cx = FRAME_GAP + col * (cell_w + FRAME_GAP);
        let cy = FRAME_GAP + row * (total_cell_h + FRAME_GAP) + label_h;
        draw_canvas(&mut sheet, cx, cy, canvas, canvas_w, canvas_h);
    }

    sheet
        .save(output_dir.join("shroud_contact_sheet.png"))
        .expect("save contact sheet");

    // Per-frame PNGs.
    for (idx, canvas) in frame_canvases.iter().enumerate() {
        let mut img = RgbaImage::from_pixel(
            canvas_w * PIXEL_SCALE,
            canvas_h * PIXEL_SCALE,
            image::Rgba([12, 12, 18, 255]),
        );
        draw_canvas(&mut img, 0, 0, canvas, canvas_w, canvas_h);
        let path = output_dir.join(format!("frame_{:02}.png", idx));
        img.save(&path).expect("save frame png");
    }

    eprintln!(
        "\nWrote {} per-frame PNGs + contact sheet + report to {}",
        frame_canvases.len(),
        output_dir.display()
    );

    // Sanity assertions.
    assert_eq!(
        canvas_w, 60,
        "expected SHROUD.SHP canvas width 60, got {}",
        canvas_w
    );
    assert_eq!(
        canvas_h, 30,
        "expected SHROUD.SHP canvas height 30, got {}",
        canvas_h
    );
    // SHROUD.SHP holds 96 frame slots — only 0..=46 are populated (47 used);
    // 47..=95 are zero-size placeholders. The LUT only references 0..=46.
    assert!(
        shp.frames.len() >= 47,
        "expected at least 47 SHROUD.SHP frames, got {}",
        shp.frames.len()
    );
}
