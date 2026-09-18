//! Asset preview renderers for the mix-browser tool.
//!
//! Each function converts raw asset bytes into an egui::ColorImage for display.
//! Supports PAL grid, TMP terrain tiles, and (future) VXL voxel models.

use eframe::egui;
use vera20k::assets::pal_file::Palette;
use vera20k::assets::tmp_file::TmpFile;

/// Cell size in pixels for each palette entry in the 16x16 grid.
const PAL_CELL_SIZE: usize = 16;
/// Total palette grid image dimensions (16 entries * 16px each = 256px).
const PAL_GRID_SIZE: usize = PAL_CELL_SIZE * 16;

/// Render a 256-color palette as a 16x16 grid image (256x256 pixels).
///
/// - Index 0 is shown as a checkerboard (transparent marker).
/// - Indices 16-31 get a 1px border to mark the house color remap region.
/// - All other indices show their solid color.
pub fn render_palette_grid(palette: &Palette) -> egui::ColorImage {
    let mut rgba = vec![0u8; PAL_GRID_SIZE * PAL_GRID_SIZE * 4];

    for index in 0u16..256 {
        let row = (index / 16) as usize;
        let col = (index % 16) as usize;
        let color = palette.colors[index as usize];
        let is_house_color = (16..=31).contains(&index);

        for py in 0..PAL_CELL_SIZE {
            for px in 0..PAL_CELL_SIZE {
                let x = col * PAL_CELL_SIZE + px;
                let y = row * PAL_CELL_SIZE + py;
                let offset = (y * PAL_GRID_SIZE + x) * 4;

                if index == 0 {
                    // Checkerboard for transparent index.
                    let checker = ((px / 4) + (py / 4)) % 2 == 0;
                    let v = if checker { 40u8 } else { 60u8 };
                    rgba[offset] = v;
                    rgba[offset + 1] = v;
                    rgba[offset + 2] = v;
                    rgba[offset + 3] = 255;
                } else if is_house_color
                    && (px == 0 || px == PAL_CELL_SIZE - 1 || py == 0 || py == PAL_CELL_SIZE - 1)
                {
                    // White border for house color region (indices 16-31).
                    rgba[offset] = 255;
                    rgba[offset + 1] = 255;
                    rgba[offset + 2] = 255;
                    rgba[offset + 3] = 255;
                } else {
                    rgba[offset] = color.r;
                    rgba[offset + 1] = color.g;
                    rgba[offset + 2] = color.b;
                    rgba[offset + 3] = 255;
                }
            }
        }
    }

    egui::ColorImage::from_rgba_unmultiplied([PAL_GRID_SIZE, PAL_GRID_SIZE], &rgba)
}

/// Render a TMP terrain template as a composite image.
///
/// Arranges tiles in their grid positions (template_width × template_height).
/// Empty cells are shown as dark checkerboard. Returns (image, tile_count).
pub fn render_tmp_preview(tmp: &TmpFile, palette: &Palette) -> Option<(egui::ColorImage, usize)> {
    let tw = tmp.tile_width as usize;
    let th = tmp.tile_height as usize;
    let cols = tmp.template_width as usize;
    let rows = tmp.template_height as usize;
    if cols == 0 || rows == 0 || tw == 0 || th == 0 {
        return None;
    }

    let img_w = cols * tw;
    let img_h = rows * th;
    // Start with dark checkerboard background.
    let mut rgba = vec![0u8; img_w * img_h * 4];
    for y in 0..img_h {
        for x in 0..img_w {
            let offset = (y * img_w + x) * 4;
            let checker = ((x / 8) + (y / 8)) % 2 == 0;
            let v = if checker { 30u8 } else { 50u8 };
            rgba[offset] = v;
            rgba[offset + 1] = v;
            rgba[offset + 2] = v;
            rgba[offset + 3] = 255;
        }
    }

    let mut tile_count = 0usize;
    for row in 0..rows {
        for col in 0..cols {
            let tile_idx = row * cols + col;
            let Ok(tile_rgba) = tmp.tile_to_rgba(tile_idx, palette) else {
                continue;
            };
            let tile = &tmp.tiles[tile_idx];
            let Some(tile_data) = tile else { continue };
            tile_count += 1;

            let pw = tile_data.pixel_width as usize;
            let ph = tile_data.pixel_height as usize;
            let base_x = col * tw;
            let base_y = row * th;

            for py in 0..ph {
                for px in 0..pw {
                    let src = (py * pw + px) * 4;
                    if src + 3 >= tile_rgba.len() {
                        continue;
                    }
                    // Skip transparent pixels (palette index 0 → alpha 0).
                    if tile_rgba[src + 3] == 0 {
                        continue;
                    }
                    let dx = base_x as i32 + tile_data.offset_x + px as i32;
                    let dy = base_y as i32 + tile_data.offset_y + py as i32;
                    if dx < 0 || dy < 0 || dx >= img_w as i32 || dy >= img_h as i32 {
                        continue;
                    }
                    let dst = (dy as usize * img_w + dx as usize) * 4;
                    rgba[dst] = tile_rgba[src];
                    rgba[dst + 1] = tile_rgba[src + 1];
                    rgba[dst + 2] = tile_rgba[src + 2];
                    rgba[dst + 3] = 255;
                }
            }
        }
    }

    let image = egui::ColorImage::from_rgba_unmultiplied([img_w, img_h], &rgba);
    Some((image, tile_count))
}
