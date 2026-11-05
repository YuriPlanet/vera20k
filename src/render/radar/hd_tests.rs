use super::*;

#[test]
fn objects_cover_their_native_pixel_as_a_block_over_the_hd_terrain() {
    // Generated 140x91, centred 8 rows down the native aperture.
    let surface = NativeRadarSurfaceGeometry::from_raw_rect(0, 0, 173, 113).unwrap();
    let mut image = HdImage::new(surface, 2);
    let packed = RGB565.pack_rgb8([0, 255, 0]);
    let terrain = vec![packed; 280 * 182];
    image.paint(&terrain, 0, |_, _| CellVisibility::Revealed);
    let mut native = COLOR_SHROUD.repeat(140 * 108);
    image.keep_native_terrain(&native);
    native[(20 * 140 + 5) * 4..][..4].copy_from_slice(&[255, 0, 0, 255]);
    image.draw_objects(&native);

    let at =
        |x: usize, y: usize| <[u8; 4]>::try_from(&image.rgba()[(y * 280 + x) * 4..][..4]).unwrap();
    let [red, green, blue] = RGB565.unpack_rgb8(packed);
    let terrain = [red, green, blue, 255];
    for (x, y) in [(10, 40), (11, 41)] {
        assert_eq!(at(x, y), [255, 0, 0, 255], "({x}, {y})");
    }
    for (x, y) in [(12, 40), (10, 42), (9, 41)] {
        assert_eq!(at(x, y), terrain, "({x}, {y})");
    }
    // The letterbox above the surface keeps the shroud fill.
    assert_eq!(at(10, 15), COLOR_SHROUD);
    assert_eq!(at(10, 16), terrain);
}
