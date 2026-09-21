//! BuildingLightClass searchlight visual resolution and batch lowering.

#[cfg(test)]
use crate::render::batch::SpriteInstance;

pub const TYPE16_MASK_WIDTH: usize = 256;
pub const TYPE16_MASK_HEIGHT: usize = 128;
pub const TYPE16_MASK_COUNT: usize = 10;
pub const TYPE16_ATLAS_WIDTH: usize = TYPE16_MASK_WIDTH * TYPE16_MASK_COUNT;
pub const DEFAULT_SPOTLIGHT_RADIUS: i32 = 175;
const TYPE16_FIRST_INTENSITY_INDEX: u8 = 80;
const TYPE16_LAST_INTENSITY_INDEX: u8 = 89;
const TYPE16_FIRST_MASK_INDEX: u8 = 64;
const TYPE16_MASK_PEAKS: [u8; TYPE16_MASK_COUNT] = [128, 122, 116, 110, 104, 98, 92, 86, 80, 74];
const TYPE16_RADIUS_SCALE_BITS: u64 = 0xbf66_db6d_b6db_6db7;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpotlightDrawPath {
    ShapeBlitter,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BuildingLightVisual {
    pub glow_type: u8,
    pub glow_intensity_index: u8,
    pub glow_draw_path: SpotlightDrawPath,
    pub beam_line_alpha: u8,
}

/// Exact selection metadata for the procedurally generated type-16 mask bank.
/// The mask pixels themselves remain a renderer-initialization concern: native
/// `SpotlightClass::Initialize @ 0x005FF420` generates them rather than loading
/// an art asset.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Type16MaskDescriptor {
    pub mask_index: u8,
    pub peak: u8,
}

pub fn type16_mask_descriptor(intensity_index: u8) -> Option<Type16MaskDescriptor> {
    let offset = intensity_index.checked_sub(TYPE16_FIRST_INTENSITY_INDEX)?;
    if intensity_index > TYPE16_LAST_INTENSITY_INDEX {
        return None;
    }
    Some(Type16MaskDescriptor {
        mask_index: TYPE16_FIRST_MASK_INDEX + offset,
        peak: TYPE16_MASK_PEAKS[usize::from(offset)],
    })
}

/// Native circle radius selected by `SpotlightClass::Initialize @ 0x005ff684`.
/// `FPU__FistpToInt32 @ 0x007c5f00` truncates the scaled value toward zero.
pub fn type16_circle_radius(spotlight_radius: i32) -> i32 {
    let scaled = f64::from(spotlight_radius * 60) * f64::from_bits(TYPE16_RADIUS_SCALE_BITS);
    -(scaled.trunc() as i32)
}

fn draw_mask_span(surface: &mut [u8], x0: i32, x1: i32, y: i32, value: u8) {
    if !(0..256).contains(&y) {
        return;
    }
    let left = x0.min(x1).max(0);
    let right = x0.max(x1).min(255);
    if left <= right {
        surface[y as usize * 256 + left as usize..=y as usize * 256 + right as usize].fill(value);
    }
}

/// Generate one native type-16 mask: midpoint circle on a cleared 256x256
/// byte surface, followed by the native even-row copy into 256x128 storage.
pub fn generate_type16_mask(spotlight_radius: i32, intensity_index: u8) -> Option<Vec<u8>> {
    let descriptor = type16_mask_descriptor(intensity_index)?;
    let radius = type16_circle_radius(spotlight_radius);
    let mut scratch = vec![0_u8; 256 * 256];

    // Named location: `Circle @ 0x007bb920`. Each midpoint step issues four
    // inclusive horizontal line calls between the symmetric endpoints, filling
    // the circle on the cleared scratch surface.
    if radius >= 0 {
        let mut x = 0_i32;
        let mut y = radius;
        let mut error = 3 - 2 * radius;
        while x <= y {
            draw_mask_span(&mut scratch, 128 + y, 128 - y, 128 + x, descriptor.peak);
            draw_mask_span(&mut scratch, 128 + x, 128 - x, 128 + y, descriptor.peak);
            draw_mask_span(&mut scratch, 128 + y, 128 - y, 128 - x, descriptor.peak);
            draw_mask_span(&mut scratch, 128 + x, 128 - x, 128 - y, descriptor.peak);
            if error < 0 {
                error += 4 * x + 6;
            } else {
                error += 4 * (x - y) + 10;
                y -= 1;
            }
            x += 1;
        }
    }

    let mut mask = vec![0_u8; TYPE16_MASK_WIDTH * TYPE16_MASK_HEIGHT];
    for destination_y in 0..TYPE16_MASK_HEIGHT {
        let source_start = destination_y * 2 * TYPE16_MASK_WIDTH;
        let destination_start = destination_y * TYPE16_MASK_WIDTH;
        mask[destination_start..destination_start + TYPE16_MASK_WIDTH]
            .copy_from_slice(&scratch[source_start..source_start + TYPE16_MASK_WIDTH]);
    }
    Some(mask)
}

/// Generate the ten type-16 masks as a single horizontal R8 atlas.
pub fn generate_type16_mask_bank(spotlight_radius: i32) -> Vec<u8> {
    let mut atlas = vec![0_u8; TYPE16_ATLAS_WIDTH * TYPE16_MASK_HEIGHT];
    for mask_index in 0..TYPE16_MASK_COUNT {
        let intensity = TYPE16_FIRST_INTENSITY_INDEX + mask_index as u8;
        let mask = generate_type16_mask(spotlight_radius, intensity)
            .expect("type-16 bank intensity is in the native descriptor range");
        for y in 0..TYPE16_MASK_HEIGHT {
            let source_start = y * TYPE16_MASK_WIDTH;
            let destination_start = y * TYPE16_ATLAS_WIDTH + mask_index * TYPE16_MASK_WIDTH;
            atlas[destination_start..destination_start + TYPE16_MASK_WIDTH]
                .copy_from_slice(&mask[source_start..source_start + TYPE16_MASK_WIDTH]);
        }
    }
    atlas
}

/// Texel-centred atlas coordinates avoid nearest-sampler bleed between masks.
pub fn type16_mask_uv(intensity_index: u8) -> Option<([f32; 2], [f32; 2])> {
    type16_mask_descriptor(intensity_index)?;
    let bank_index = usize::from(intensity_index - TYPE16_FIRST_INTENSITY_INDEX);
    let origin_x = (bank_index * TYPE16_MASK_WIDTH) as f32 + 0.5;
    Some((
        [
            origin_x / TYPE16_ATLAS_WIDTH as f32,
            0.5 / TYPE16_MASK_HEIGHT as f32,
        ],
        [
            (TYPE16_MASK_WIDTH - 1) as f32 / TYPE16_ATLAS_WIDTH as f32,
            (TYPE16_MASK_HEIGHT - 1) as f32 / TYPE16_MASK_HEIGHT as f32,
        ],
    ))
}

/// Lower an already-projected authoritative child-light center to a mask quad.
/// Child coordinate/angle production remains simulation-owned.
#[cfg(test)]
pub fn build_type16_mask_instance(
    projected_center: [f32; 2],
    intensity_index: u8,
    depth: f32,
) -> Option<SpriteInstance> {
    let (uv_origin, uv_size) = type16_mask_uv(intensity_index)?;
    Some(SpriteInstance {
        position: [
            projected_center[0] - TYPE16_MASK_WIDTH as f32 / 2.0,
            projected_center[1] - TYPE16_MASK_HEIGHT as f32 / 2.0,
        ],
        size: [TYPE16_MASK_WIDTH as f32, TYPE16_MASK_HEIGHT as f32],
        uv_origin,
        uv_size,
        depth,
        ..Default::default()
    })
}

/// Clipped source/destination window for the native 256x128 type-16 mask.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SpotlightRasterClip {
    pub source_x: usize,
    pub source_y: usize,
    pub destination_x: usize,
    pub destination_y: usize,
    pub width: usize,
    pub height: usize,
}

/// Clip the mask centered at `screen_center` against an origin-zero target.
/// Named location: native rectangle helper `0x007BC040` advances source and
/// destination origins together after intersecting the destination clip.
#[cfg(test)]
pub fn clip_type16_mask(
    screen_center: (i32, i32),
    target_size: (usize, usize),
) -> Option<SpotlightRasterClip> {
    let destination_left = screen_center.0 - TYPE16_MASK_WIDTH as i32 / 2;
    let destination_top = screen_center.1 - TYPE16_MASK_HEIGHT as i32 / 2;
    let destination_right = destination_left + TYPE16_MASK_WIDTH as i32;
    let destination_bottom = destination_top + TYPE16_MASK_HEIGHT as i32;
    let clipped_left = destination_left.max(0);
    let clipped_top = destination_top.max(0);
    let clipped_right = destination_right.min(target_size.0 as i32);
    let clipped_bottom = destination_bottom.min(target_size.1 as i32);
    if clipped_left >= clipped_right || clipped_top >= clipped_bottom {
        return None;
    }
    Some(SpotlightRasterClip {
        source_x: (clipped_left - destination_left) as usize,
        source_y: (clipped_top - destination_top) as usize,
        destination_x: clipped_left as usize,
        destination_y: clipped_top as usize,
        width: (clipped_right - clipped_left) as usize,
        height: (clipped_bottom - clipped_top) as usize,
    })
}

/// Native direct 5:6:5 zero-blend operation at `0x007DEEFA`.
/// Every channel saturates independently after `c + floor(c*m/256)`.
pub fn blend_type16_rgb565(destination: u16, mask: u8) -> u16 {
    if mask == 0 {
        return destination;
    }
    let red = u32::from((destination >> 8) & 0xf8);
    let green = u32::from((destination >> 3) & 0xfc);
    let blue = u32::from((destination << 3) & 0xff);
    let brighten = |channel: u32| (channel + channel * u32::from(mask) / 256).min(0xff);
    let red = brighten(red) >> 3;
    let green = brighten(green) >> 2;
    let blue = brighten(blue) >> 3;
    ((red << 11) | (green << 5) | blue) as u16
}

/// Apply already-generated native mask bytes to an RGB565 surface.
/// This is the exact CPU raster substrate; using an alpha-blended sprite in its
/// place would change the destination-dependent additive multiplication.
#[cfg(test)]
pub fn raster_type16_rgb565(
    destination: &mut [u16],
    destination_pitch: usize,
    mask: &[u8],
    clip: SpotlightRasterClip,
) {
    debug_assert!(mask.len() >= TYPE16_MASK_WIDTH * TYPE16_MASK_HEIGHT);
    for row in 0..clip.height {
        let destination_row = (clip.destination_y + row) * destination_pitch + clip.destination_x;
        let source_row = (clip.source_y + row) * TYPE16_MASK_WIDTH + clip.source_x;
        for column in 0..clip.width {
            let destination_index = destination_row + column;
            destination[destination_index] =
                blend_type16_rgb565(destination[destination_index], mask[source_row + column]);
        }
    }
}

/// RA2/YR `BuildingLightClass::DrawIt` (YR 0x435be0) visual-only contract.
#[cfg(test)]
pub fn resolve_building_light_visual(mode: u8, distance_bucket: i32) -> BuildingLightVisual {
    let bucket = distance_bucket.clamp(0, 10) as u8;
    BuildingLightVisual {
        glow_type: 16,
        glow_intensity_index: if mode == 3 { (bucket + 80).min(89) } else { 80 },
        glow_draw_path: SpotlightDrawPath::ShapeBlitter,
        beam_line_alpha: 75 - 6 * bucket,
    }
}

/// Lower the two independently supplied, Z-adjusted cone edges to batch pixels.
/// Target acquisition remains simulation-owned and deliberately absent here.
#[cfg(test)]
pub fn build_searchlight_beam_instances(
    edges: [([f32; 2], [f32; 2]); 2],
    visual: BuildingLightVisual,
    tint: [f32; 3],
    depth: f32,
) -> Vec<SpriteInstance> {
    let mut instances = Vec::new();
    let alpha = f32::from(visual.beam_line_alpha) / 255.0;
    for (start, end) in edges {
        let dx = end[0] - start[0];
        let dy = end[1] - start[1];
        let steps = dx.abs().max(dy.abs()).ceil() as usize;
        for index in 0..steps.max(1) {
            let fraction = index as f32 / steps.max(1) as f32;
            instances.push(SpriteInstance {
                position: [
                    (start[0] + dx * fraction).round(),
                    (start[1] + dy * fraction).round(),
                ],
                size: [1.0, 1.0],
                uv_size: [1.0, 1.0],
                tint,
                alpha,
                depth,
                ..Default::default()
            });
        }
    }
    instances
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn executable_visual_vectors() {
        assert_eq!(
            resolve_building_light_visual(1, 0),
            BuildingLightVisual {
                glow_type: 16,
                glow_intensity_index: 80,
                glow_draw_path: SpotlightDrawPath::ShapeBlitter,
                beam_line_alpha: 75
            }
        );
        assert_eq!(
            resolve_building_light_visual(3, 7),
            BuildingLightVisual {
                glow_type: 16,
                glow_intensity_index: 87,
                glow_draw_path: SpotlightDrawPath::ShapeBlitter,
                beam_line_alpha: 33
            }
        );
        assert_eq!(
            resolve_building_light_visual(3, 15),
            BuildingLightVisual {
                glow_type: 16,
                glow_intensity_index: 89,
                glow_draw_path: SpotlightDrawPath::ShapeBlitter,
                beam_line_alpha: 15
            }
        );
    }

    #[test]
    fn type16_indices_select_procedural_masks_and_peaks() {
        assert_eq!(
            type16_mask_descriptor(80),
            Some(Type16MaskDescriptor {
                mask_index: 64,
                peak: 128,
            })
        );
        assert_eq!(
            type16_mask_descriptor(89),
            Some(Type16MaskDescriptor {
                mask_index: 73,
                peak: 74,
            })
        );
        assert_eq!(type16_mask_descriptor(79), None);
        assert_eq!(type16_mask_descriptor(90), None);
    }

    #[test]
    fn stock_type16_masks_use_native_radius_and_even_row_copy() {
        assert_eq!(type16_circle_radius(DEFAULT_SPOTLIGHT_RADIUS), 29);
        let mask = generate_type16_mask(DEFAULT_SPOTLIGHT_RADIUS, 80).unwrap();
        assert_eq!(mask.len(), TYPE16_MASK_WIDTH * TYPE16_MASK_HEIGHT);
        assert_eq!(mask.iter().copied().max(), Some(128));
        assert_eq!(mask[64 * TYPE16_MASK_WIDTH + 99], 128);
        assert_eq!(mask[64 * TYPE16_MASK_WIDTH + 157], 128);
        assert_eq!(mask[49 * TYPE16_MASK_WIDTH + 128], 0);
        assert_eq!(mask[50 * TYPE16_MASK_WIDTH + 128], 128);
        assert_eq!(mask[78 * TYPE16_MASK_WIDTH + 128], 128);
        assert_eq!(mask[79 * TYPE16_MASK_WIDTH + 128], 0);
    }

    #[test]
    fn type16_bank_preserves_each_descriptor_peak() {
        let bank = generate_type16_mask_bank(DEFAULT_SPOTLIGHT_RADIUS);
        assert_eq!(bank.len(), TYPE16_ATLAS_WIDTH * TYPE16_MASK_HEIGHT);
        for (index, peak) in TYPE16_MASK_PEAKS.into_iter().enumerate() {
            let start = index * TYPE16_MASK_WIDTH;
            let observed_peak = (0..TYPE16_MASK_HEIGHT)
                .flat_map(|y| {
                    bank[y * TYPE16_ATLAS_WIDTH + start
                        ..y * TYPE16_ATLAS_WIDTH + start + TYPE16_MASK_WIDTH]
                        .iter()
                        .copied()
                })
                .max();
            assert_eq!(observed_peak, Some(peak));
        }
    }

    #[test]
    fn type16_instance_uses_texel_centres_and_native_extent() {
        let instance = build_type16_mask_instance([500.0, 300.0], 89, 0.25).unwrap();
        assert_eq!(instance.position, [372.0, 236.0]);
        assert_eq!(instance.size, [256.0, 128.0]);
        assert_eq!(instance.depth, 0.25);
        let first_sample_x = instance.uv_origin[0] * TYPE16_ATLAS_WIDTH as f32;
        let last_sample_x =
            (instance.uv_origin[0] + instance.uv_size[0]) * TYPE16_ATLAS_WIDTH as f32;
        assert!((first_sample_x - (9.0 * 256.0 + 0.5)).abs() < 0.001);
        assert!((last_sample_x - (10.0 * 256.0 - 0.5)).abs() < 0.001);
    }

    #[test]
    fn type16_clip_advances_mask_origin_with_destination() {
        assert_eq!(
            clip_type16_mask((64, 32), (320, 200)),
            Some(SpotlightRasterClip {
                source_x: 64,
                source_y: 32,
                destination_x: 0,
                destination_y: 0,
                width: 192,
                height: 96,
            })
        );
        assert_eq!(clip_type16_mask((-129, 64), (320, 200)), None);
    }

    #[test]
    fn type16_rgb565_blend_is_destination_dependent_and_saturating() {
        assert_eq!(blend_type16_rgb565(0x4208, 0), 0x4208);
        assert_eq!(blend_type16_rgb565(0x4208, 128), 0x630c);
        assert_eq!(blend_type16_rgb565(0xffff, 128), 0xffff);
    }

    #[test]
    fn type16_raster_uses_fixed_mask_stride() {
        let mut destination = vec![0x4208; 4 * 3];
        let mut mask = vec![0; TYPE16_MASK_WIDTH * TYPE16_MASK_HEIGHT];
        mask[TYPE16_MASK_WIDTH + 2] = 128;
        raster_type16_rgb565(
            &mut destination,
            4,
            &mask,
            SpotlightRasterClip {
                source_x: 2,
                source_y: 1,
                destination_x: 1,
                destination_y: 1,
                width: 1,
                height: 1,
            },
        );
        assert_eq!(destination[5], 0x630c);
        assert!(
            destination
                .iter()
                .enumerate()
                .all(|(index, value)| index == 5 || *value == 0x4208)
        );
    }

    #[test]
    fn beam_lowering_preserves_resolved_alpha() {
        let visual = resolve_building_light_visual(3, 7);
        let instances = build_searchlight_beam_instances(
            [([0.0, 0.0], [3.0, 0.0]), ([0.0, 1.0], [3.0, 1.0])],
            visual,
            [1.0; 3],
            0.5,
        );
        assert_eq!(instances.len(), 6);
        assert!(
            instances
                .iter()
                .all(|instance| instance.alpha == 33.0 / 255.0)
        );
    }
}
