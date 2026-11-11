use super::*;

const RED: [u8; 4] = [255, 0, 0, 255];
const BLUE: [u8; 4] = [0, 0, 255, 255];
const GREEN: [u8; 4] = [0, 255, 0, 255];
const GREY: [u8; 4] = [96, 96, 96, 255];
const WHITE: [u8; 4] = [252, 252, 252, 255];
const BLACK: [u8; 4] = [0, 0, 0, 255];

fn image(width: usize, height: usize, pixel: impl Fn(usize, usize) -> [u8; 4]) -> Vec<u8> {
    let mut rgba = Vec::with_capacity(width * height * 4);
    for y in 0..height {
        for x in 0..width {
            rgba.extend_from_slice(&pixel(x, y));
        }
    }
    rgba
}

fn pixel(rgba: &[u8], width: usize, x: usize, y: usize) -> [u8; 4] {
    rgba[(y * width + x) * 4..][..4].try_into().unwrap()
}

#[test]
fn each_piece_scales_alone_with_its_edges_continued() {
    // A red|blue piece and a green one, a clear column apart. Scaled with the
    // clear pixels beside it, xBR would blend black into the red|blue
    // piece's bottom corners.
    let rgba = image(5, 3, |x, y| match (x, y) {
        (0, 0..2) => RED,
        (1, 0..2) => BLUE,
        (3..5, _) => GREEN,
        _ => [0; 4],
    });
    let source = SmoothSource::new(rgba, [5, 3], vec![[0, 0, 2, 2], [3, 0, 2, 3]]);
    let scaled = source.upscale(3).unwrap();
    assert_eq!(scaled.len(), 15 * 9 * 4);
    for y in 0..9 {
        for x in 0..15 {
            let expected = match (x, y) {
                (0..3, 0..6) => RED,
                (3..6, 0..6) => BLUE,
                (9..15, _) => GREEN,
                _ => [0; 4],
            };
            assert_eq!(pixel(&scaled, 15, x, y), expected, "at ({x}, {y})");
        }
    }
}

#[test]
fn scales_past_xbr_add_blocks() {
    let rgba = image(3, 3, |x, y| if x == y { RED } else { BLUE });
    let source = SmoothSource::new(rgba, [3, 3], vec![[0, 0, 3, 3]]);
    let four = source.upscale(4).unwrap();
    let eight = source.upscale(8).unwrap();
    for y in 0..24 {
        for x in 0..24 {
            assert_eq!(
                pixel(&eight, 24, x, y),
                pixel(&four, 12, x / 2, y / 2),
                "at ({x}, {y})"
            );
        }
    }
    assert!(source.upscale(5).is_none());
}

#[test]
fn smooths_at_four_tenths_strength() {
    // Each pixel of the copy moves 40% of the way from its own block's colour
    // toward xBR's.
    let rgba = image(3, 3, |x, y| if x == y { RED } else { BLUE });
    let copy = SmoothSource::new(rgba.clone(), [3, 3], vec![[0, 0, 3, 3]])
        .upscale(2)
        .unwrap();
    let full = xbr::scale(&rgba, 3, 3, 2);
    let mut blended = 0;
    for y in 0..6 {
        for x in 0..6 {
            let [smooth, block] = [pixel(&full, 6, x, y), pixel(&rgba, 3, x / 2, y / 2)];
            let expected: [u8; 4] = std::array::from_fn(|c| {
                ((u16::from(block[c]) * 6 + u16::from(smooth[c]) * 4 + 5) / 10) as u8
            });
            blended += usize::from(smooth != block);
            assert_eq!(pixel(&copy, 6, x, y), expected, "at ({x}, {y})");
        }
    }
    assert!(blended > 0, "xBR rounds the diagonal");
}

#[test]
fn finds_cameo_names_by_their_font_and_shadow() {
    let palette = Palette::from_rgb(|index| match index {
        254 => [0, 0, 0],
        255 => [252, 252, 252],
        _ => [96, 96, 96],
    });
    // Strokes of the font's white (255) ending in its black shadow (254), on
    // a grey cameo canvas.
    let mut canvas = vec![9u8; 60 * 48];
    let mut stroke = |x: usize, y: usize, len: usize| {
        canvas[y * 60 + x..][..len].fill(255);
        canvas[y * 60 + x + len] = 254;
    };
    stroke(10, 42, 3); // a name stroke
    stroke(30, 20, 3); // the same in the picture, above the name rows
    stroke(40, 44, 6); // wider than the font's strokes
    canvas[43 * 60 + 50] = 255; // no shadow
    let sharp = cameo_name_pixels(&canvas, 60, 48, &palette);
    let found: Vec<usize> = (0..sharp.len()).filter(|&index| sharp[index]).collect();
    // The name stroke and its shadow, grown by a pixel.
    let expected: Vec<usize> = (41..=43)
        .flat_map(|y| (9..=14).map(move |x| y * 60 + x))
        .collect();
    assert_eq!(found, expected);
}

#[test]
fn draws_sharp_pixels_as_blocks() {
    // A white stroke and its shadow on grey, kept sharp with a pixel around.
    let rgba = image(9, 5, |x, y| match (x, y) {
        (3..=5, 2) => WHITE,
        (6, 2) => BLACK,
        _ => GREY,
    });
    let sharp: Vec<bool> = (0..45)
        .map(|index| (1..=3).contains(&(index / 9)) && (2..=7).contains(&(index % 9)))
        .collect();
    let source = || SmoothSource::new(rgba.clone(), [9, 5], vec![[0, 0, 9, 5]]);
    let smooth = source().upscale(2).unwrap();
    let kept = source()
        .with_sharp_pixels(sharp.clone())
        .upscale(2)
        .unwrap();
    // xBR alone would round the stroke.
    assert_ne!(kept, smooth);
    for y in 0..10 {
        for x in 0..18 {
            let expected = if sharp[y / 2 * 9 + x / 2] {
                pixel(&rgba, 9, x / 2, y / 2)
            } else {
                pixel(&smooth, 18, x, y)
            };
            assert_eq!(pixel(&kept, 18, x, y), expected, "at ({x}, {y})");
        }
    }
}

#[test]
fn retarget_finds_stacked_pieces_in_their_packed_copy() {
    let rgba = image(4, 10, |x, y| [x as u8, y as u8, 7, 255]);
    let pieces = [[0, 0, 4, 3], [0, 4, 2, 2], [0, 7, 3, 3]];
    let gathered = SidebarPieces::gather(&rgba, [4, 10], &pieces, 1, 64);
    let instance = |origin: [f32; 2], size: [f32; 2]| SpriteInstance {
        uv_origin: [origin[0] / 4.0, origin[1] / 10.0],
        uv_size: [size[0] / 4.0, size[1] / 10.0],
        ..Default::default()
    };

    // Texels 1..3 x 8..10, inside the third piece.
    let mut instances = [instance([1.0, 8.0], [2.0, 2.0])];
    assert!(gathered.retarget(&mut instances));
    let packed = &gathered.source;
    let [width, height] = packed.size.map(|value| value as f32);
    let [x, y] = [
        (instances[0].uv_origin[0] * width).round() as usize,
        (instances[0].uv_origin[1] * height).round() as usize,
    ];
    assert_eq!(
        [
            instances[0].uv_size[0] * width,
            instances[0].uv_size[1] * height
        ],
        [2.0, 2.0]
    );
    for dy in 0..2 {
        for dx in 0..2 {
            assert_eq!(
                pixel(&packed.rgba, packed.size[0] as usize, x + dx, y + dy),
                [1 + dx as u8, 8 + dy as u8, 7, 255]
            );
        }
    }

    // Row 3 lies between pieces: nothing moves.
    let mut instances = [
        instance([1.0, 8.0], [2.0, 2.0]),
        instance([0.0, 3.0], [1.0, 1.0]),
    ];
    let before = instances.map(|instance| (instance.uv_origin, instance.uv_size));
    assert!(!gathered.retarget(&mut instances));
    assert_eq!(
        instances.map(|instance| (instance.uv_origin, instance.uv_size)),
        before
    );
}
