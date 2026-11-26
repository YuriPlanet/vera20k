use super::*;

#[test]
fn native_radar_surface_width_branch_projects_and_sizes_event_radius() {
    // Binary-derived `0x6548B3..0x654900`: the stored f32 140/300
    // makes the subsequently stored scaled height 83.99999, then ftol chops.
    let geometry = NativeRadarSurfaceGeometry::from_raw_rect(0, 0, 300, 180).unwrap();
    assert_eq!(geometry.raw_size(), (300, 180));
    assert_eq!(geometry.generated_size(), (140, 83));
    assert_eq!(geometry.aperture_offset(), (0, 12));
    assert_eq!(geometry.cell_to_surface_pixel((150, 0)), (69, 69));
    assert_eq!(
        geometry.world_to_surface_pixel(150 * 256, 0, true),
        (69, 69)
    );
    let pixel = geometry.cell_to_surface_pixel((120, 30));
    assert_eq!(pixel, (41, 69));
    let (width, height) = geometry.generated_size();
    assert_eq!(native_event_initial_radius(pixel, (width, height)), 99);
}

/// `CellToRadarPixel @ 0x006550C0` adds ImageRect's origin before ftol, so
/// a cell left of or above the image rounds down: raw x -3 at 140/300 is
/// -1.4, pixel -2 (not -1 and then 0), and raw y -1 is pixel -1, not 0.
#[test]
fn off_image_cells_round_down_as_native_adds_the_image_origin_first() {
    let geometry = NativeRadarSurfaceGeometry::from_raw_rect(0, 0, 300, 180).unwrap();
    assert_eq!(geometry.cell_to_surface_pixel((0, 3)), (-2, 1));
    assert_eq!(geometry.cell_to_surface_pixel((0xFFFF, 0)), (0, -1));
}

/// A one-raw-pixel-wide map scales to an empty image; `GetCrdOnRadar`'s
/// clamp (`0x006557F0`) then gives x -1 instead of stopping the game.
#[test]
fn clamping_onto_an_empty_image_gives_minus_one() {
    let geometry = NativeRadarSurfaceGeometry::from_raw_rect(0, 0, 1, 300).unwrap();
    assert_eq!(geometry.generated_size(), (0, 108));
    assert_eq!(geometry.world_to_surface_pixel(0, 0, true).0, -1);
}

#[test]
fn native_radar_surface_height_branch_projects_and_sizes_event_radius() {
    // Binary-derived `0x654902..0x654930`: 108/300, generated width truncates.
    let geometry = NativeRadarSurfaceGeometry::from_raw_rect(0, 0, 180, 300).unwrap();
    assert_eq!(geometry.raw_size(), (180, 300));
    assert_eq!(geometry.generated_size(), (64, 108));
    assert_eq!(geometry.aperture_offset(), (38, 0));
    assert_eq!(geometry.cell_to_surface_pixel((90, 0)), (32, 32));
    assert_eq!(geometry.world_to_surface_pixel(90 * 256, 0, true), (32, 32));
    let pixel = geometry.cell_to_surface_pixel((75, 15));
    assert_eq!(pixel, (21, 32));
    let (width, height) = geometry.generated_size();
    assert_eq!(native_event_initial_radius(pixel, (width, height)), 76);
}

/// The integer projection answers as the x87 model does: either side of the
/// first pixel edges, for negative raw coordinates and the ends of the i32
/// range, at a native, an HD, a whole and a small zoom.
#[test]
fn the_integer_projection_answers_as_the_x87_model() {
    let native = NativeRadarSurfaceGeometry::from_raw_rect(0, 0, 300, 180).unwrap();
    for zoom in [native.zoom(), native.subdivided(3).zoom(), 1.0, 0.1] {
        let edges = (-3..=3).map(|pixel| (f64::from(pixel) * 256.0 / f64::from(zoom)) as i32);
        let raws =
            edges
                .flat_map(|edge| [edge - 1, edge, edge + 1])
                .chain([i32::MIN, i32::MAX, -1, 0, 1]);
        for raw in raws {
            assert_eq!(
                surface_pixel(raw, zoom),
                x87_surface_pixel(raw, zoom),
                "raw {raw}, zoom {zoom}"
            );
        }
    }
}
