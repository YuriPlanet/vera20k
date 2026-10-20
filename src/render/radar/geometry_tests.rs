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
