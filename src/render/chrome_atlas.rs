//! Shelf packer shared by the loading-screen, main-menu and skirmish shell
//! chrome atlases.
//!
//! Pieces keep their caller order on rows of a page at least 1024 px wide,
//! two pixels apart; the page height rounds up to a power of two. The map
//! atlases use a different policy (area-sorted, one-pixel padding, page
//! limits) and `atlas_growth::ShelfCursor` wraps without the trailing gap, so
//! neither shares this placement.

use crate::render::batch::{BatchRenderer, BatchTexture};
use crate::render::gpu::GpuContext;

const ATLAS_PADDING: u32 = 2;
const MIN_ATLAS_WIDTH: u32 = 1024;

/// One CPU-rendered RGBA chrome piece awaiting placement.
pub(crate) struct ChromePiece {
    pub(crate) label: String,
    pub(crate) width: u32,
    pub(crate) height: u32,
    pub(crate) rgba: Vec<u8>,
}

/// A packed piece's normalized rectangle and native pixel size.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ChromeAtlasEntry {
    pub uv_origin: [f32; 2],
    pub uv_size: [f32; 2],
    pub pixel_size: [f32; 2],
}

impl ChromeAtlasEntry {
    /// The image's native width and height in pixels.
    pub fn native_size(self) -> (i32, i32) {
        (
            self.pixel_size[0].round() as i32,
            self.pixel_size[1].round() as i32,
        )
    }
}

#[derive(Debug, PartialEq, Eq)]
struct ChromeAtlasLayout {
    width: u32,
    height: u32,
    origins: Vec<[u32; 2]>,
}

fn layout(sizes: &[(u32, u32)]) -> ChromeAtlasLayout {
    let width = sizes
        .iter()
        .map(|&(width, _)| width)
        .max()
        .unwrap_or(1)
        .max(MIN_ATLAS_WIDTH);
    let (mut x, mut y, mut row_height) = (0u32, 0u32, 0u32);
    let mut origins = Vec::with_capacity(sizes.len());
    for &(piece_width, piece_height) in sizes {
        // A row's first piece stays even when it is wider than the page
        // minus the trailing gap.
        if x > 0 && x + piece_width + ATLAS_PADDING > width {
            x = 0;
            y += row_height + ATLAS_PADDING;
            row_height = 0;
        }
        origins.push([x, y]);
        x += piece_width + ATLAS_PADDING;
        row_height = row_height.max(piece_height);
    }
    ChromeAtlasLayout {
        width,
        height: (y + row_height).next_power_of_two().max(1),
        origins,
    }
}

/// Pack `pieces` in order into one uploaded texture; `None` when empty.
/// Entries come back in the same order as `pieces`.
pub(crate) fn pack_chrome_atlas(
    gpu: &GpuContext,
    batch: &BatchRenderer,
    atlas_name: &str,
    pieces: &[ChromePiece],
) -> Option<(BatchTexture, Vec<ChromeAtlasEntry>)> {
    if pieces.is_empty() {
        return None;
    }
    let sizes: Vec<_> = pieces
        .iter()
        .map(|piece| (piece.width, piece.height))
        .collect();
    let ChromeAtlasLayout {
        width,
        height,
        origins,
    } = layout(&sizes);

    let mut rgba = vec![0u8; (width * height * 4) as usize];
    for (piece, &[px, py]) in pieces.iter().zip(&origins) {
        let len = (piece.width * 4) as usize;
        for row in 0..piece.height {
            let src = (row * piece.width * 4) as usize;
            let dst = (((py + row) * width + px) * 4) as usize;
            rgba[dst..dst + len].copy_from_slice(&piece.rgba[src..src + len]);
        }
    }

    log::info!(
        "{atlas_name} atlas: {width}x{height} px, {} pieces",
        pieces.len()
    );
    for piece in pieces {
        log::debug!("  {}: {}x{}", piece.label, piece.width, piece.height);
    }

    let texture = batch.create_texture(gpu, &rgba, width, height);
    let entries = pieces
        .iter()
        .zip(origins)
        .map(|(piece, [px, py])| ChromeAtlasEntry {
            uv_origin: [px as f32 / width as f32, py as f32 / height as f32],
            uv_size: [
                piece.width as f32 / width as f32,
                piece.height as f32 / height as f32,
            ],
            pixel_size: [piece.width as f32, piece.height as f32],
        })
        .collect();
    Some((texture, entries))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Expected rectangles come from running the three former per-screen
    /// loops on this input: an over-wide first piece, an exact-width fit and
    /// two wraps.
    #[test]
    fn chrome_layout_keeps_former_shelf_rectangles() {
        let sizes = [
            (1030, 20),
            (800, 600),
            (226, 30),
            (2, 1),
            (640, 480),
            (300, 16),
        ];
        assert_eq!(
            layout(&sizes),
            ChromeAtlasLayout {
                width: 1030,
                height: 2048,
                origins: vec![[0, 0], [0, 22], [802, 22], [0, 624], [4, 624], [646, 624]],
            }
        );
    }
}
