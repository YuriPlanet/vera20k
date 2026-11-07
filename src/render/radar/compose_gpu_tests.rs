//! The radar compositor's pictures, read back from the GPU.

use std::collections::BTreeMap;

use super::*;
use crate::render::native_surface_format::RGB565;
use crate::render::radar::terrain::NativeRadarCellColors;
use crate::render::terrain_draw_gpu_tests::Gpu;
use crate::sim::vision::{FLAG_GAP_COVERED, FLAG_GAP_FOG, FLAG_REVEALED};

const FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8UnormSrgb;
const SHROUD: [u8; 4] = [0, 0, 0, 255];

/// Generated 140x91, centred 8 rows down the aperture, with a colour per
/// cell half so neighbouring pixels differ.
fn terrain() -> NativeRadarTerrainSurface {
    let geometry = NativeRadarSurfaceGeometry::from_raw_rect(0, 0, 173, 113).unwrap();
    let cells = (0..128u16)
        .flat_map(|x| (0..64u16).map(move |y| (x, y)))
        .map(|(x, y)| NativeRadarCellColors {
            cell: (x, y),
            left: [(x * 2) as u8, (y * 4) as u8, 40],
            right: [(x * 2) as u8, (y * 4) as u8, 240],
        })
        .collect();
    NativeRadarTerrainSurface::new(geometry, cells, BTreeMap::new(), 1.0)
}

fn opaque(word: u16) -> [u8; 4] {
    let [red, green, blue] = RGB565.unpack_rgb8(word);
    [red, green, blue, 255]
}

/// Compose, then read `picture` back as RGBA rows.
fn read(gpu: &Gpu, compositor: &mut RadarCompositor, hd: bool) -> (Vec<u8>, u32) {
    let mut encoder = gpu.device.create_command_encoder(&Default::default());
    compositor.encode(&gpu.device, &mut encoder);
    let picture = if hd {
        compositor.hd().unwrap().picture()
    } else {
        compositor.picture()
    };
    let size = [picture.width, picture.height];
    let read = gpu.read(&mut encoder, picture.view.texture());
    (gpu.finish(encoder, &[read], size).remove(0), size[0])
}

fn pixel(rgba: &[u8], width: u32, (x, y): (u32, u32)) -> [u8; 4] {
    rgba[((y * width + x) * 4) as usize..][..4]
        .try_into()
        .unwrap()
}

#[test]
#[ignore = "requires a wgpu adapter"]
fn cell_flags_and_stores_follow_render_cell_pixel_and_update_order() {
    let gpu = Gpu::new();
    let batch = BatchRenderer::new_with_device(&gpu.device, &gpu.queue, FORMAT);
    let terrain = terrain();
    let geometry = terrain.geometry();
    let mut compositor = RadarCompositor::new(&gpu.device, &gpu.queue, &batch);
    compositor.set_surface(&gpu.device, &gpu.queue, Some(&terrain));

    // RenderCellPixel @ 0x00655C50: a hostile gap's cover beats a friendly
    // gap's fog, which beats the viewer's own reveal.
    let surface_pixels = [(20, 60), (30, 70), (40, 80), (50, 60)];
    let flags = [
        FLAG_REVEALED,
        FLAG_REVEALED | FLAG_GAP_FOG,
        FLAG_REVEALED | FLAG_GAP_FOG | FLAG_GAP_COVERED,
        0,
    ];
    let mut plane = vec![0; 256 * 256];
    let cells = surface_pixels.map(|pixel| geometry.surface_pixel_to_visibility_cell(pixel));
    for (&(rx, ry), &flag) in cells.iter().zip(&flags) {
        plane[usize::from(ry) * 256 + usize::from(rx)] = flag;
    }
    let distinct: std::collections::BTreeSet<_> = cells.iter().collect();
    assert_eq!(distinct.len(), cells.len(), "one cell per checked pixel");
    compositor.set_visibility(
        &gpu.device,
        &gpu.queue,
        RadarVisibility::Plane {
            flags: &plane,
            size: (256, 256),
        },
    );
    // Two stores at one pixel: the later one wins.
    let stores = [
        RadarStore::at((5, 3), [255, 0, 0, 255]).unwrap(),
        RadarStore::at((5, 3), [0, 0, 255, 255]).unwrap(),
    ];
    compositor.set_stores(&gpu.device, &gpu.queue, &stores);

    let (rgba, width) = read(&gpu, &mut compositor, false);
    let word = |(x, y): (i32, i32)| terrain.generated_rgb565()[(y * 140 + x) as usize];
    let aperture = |pixel: (i32, i32)| {
        let (x, y) = geometry.surface_to_aperture_pixel(pixel);
        (x as u32, y as u32)
    };
    let revealed = opaque(word(surface_pixels[0]));
    assert_eq!(pixel(&rgba, width, aperture(surface_pixels[0])), revealed);
    // Half-bright halves each R5G6B5 channel.
    let [red, green, blue] = RGB565.unpack_rgb8(word(surface_pixels[1]));
    let halved = opaque(RGB565.pack_rgb8([red >> 1, green >> 1, blue >> 1]));
    assert_eq!(pixel(&rgba, width, aperture(surface_pixels[1])), halved);
    assert_eq!(pixel(&rgba, width, aperture(surface_pixels[2])), SHROUD);
    assert_eq!(pixel(&rgba, width, aperture(surface_pixels[3])), SHROUD);
    assert_eq!(pixel(&rgba, width, (5, 3)), [0, 0, 255, 255]);

    // A later tick reveals the last cell: only its row is uploaded again.
    let (rx, ry) = cells[3];
    plane[usize::from(ry) * 256 + usize::from(rx)] = FLAG_REVEALED;
    compositor.set_visibility(
        &gpu.device,
        &gpu.queue,
        RadarVisibility::Plane {
            flags: &plane,
            size: (256, 256),
        },
    );
    compositor.request();
    let (rgba, width) = read(&gpu, &mut compositor, false);
    let newly_revealed = opaque(word(surface_pixels[3]));
    assert_eq!(
        pixel(&rgba, width, aperture(surface_pixels[3])),
        newly_revealed
    );
    assert_eq!(pixel(&rgba, width, aperture(surface_pixels[1])), halved);
}

#[test]
#[ignore = "requires a wgpu adapter"]
fn hd_radar_draws_stores_as_blocks_over_its_own_terrain() {
    let gpu = Gpu::new();
    let batch = BatchRenderer::new_with_device(&gpu.device, &gpu.queue, FORMAT);
    let mut terrain = terrain();
    terrain.set_hd_scale(Some(2));
    let mut compositor = RadarCompositor::new(&gpu.device, &gpu.queue, &batch);
    compositor.set_surface(&gpu.device, &gpu.queue, Some(&terrain));
    compositor.set_hd(&gpu.device, &gpu.queue, &batch, Some((2, &terrain)));
    compositor.set_visibility(&gpu.device, &gpu.queue, RadarVisibility::Revealed);
    let red = [255, 0, 0, 255];
    let stores = [RadarStore::at((5, 20), red).unwrap()];
    compositor.set_stores(&gpu.device, &gpu.queue, &stores);

    let hd = compositor.hd().unwrap();
    assert_eq!(hd.origin(), [0, 16]);
    let (words, _) = terrain.hd_rgb565().unwrap();
    let (rgba, width) = read(&gpu, &mut compositor, true);
    let at = |pixel| self::pixel(&rgba, width, pixel);
    let hd_terrain = |(x, y): (u32, u32)| opaque(words[((y - 16) * 280 + x) as usize]);
    for pixel in [(10, 40), (11, 41)] {
        assert_eq!(at(pixel), red, "{pixel:?}");
    }
    // Neighbouring blocks show the HD terrain under their own pixels.
    for pixel in [(12, 40), (10, 42), (9, 41), (10, 16)] {
        assert_eq!(at(pixel), hd_terrain(pixel), "{pixel:?}");
    }
    // The letterbox above the HD surface keeps the shroud fill.
    assert_eq!(at((10, 15)), SHROUD);
}
