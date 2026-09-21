//! Pure native-compatible tactical point consumer.
//!
//! This module consumes externally produced u16 A/Z words and runtime display
//! values. It never substitutes the R8 shroud texture or floating scene depth.

use glam::{IVec2, IVec3};
use thiserror::Error;

use crate::util::native_x87::{NativeF64Bits, NativeX87Error, X87Chop53, X87Value};

pub use super::native_surface_format::DirectDrawPixelFormat;

const PARTICLE_Z_BIAS: i32 = 0x32;
const A_PASSTHROUGH_THRESHOLD: u16 = 127;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SparkDrawGates {
    pub performance_passed: bool,
    pub extra_animations_enabled: bool,
    pub fog_passed: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SparkPointCommand {
    pub world: IVec3,
    pub start_rgb: [u8; 3],
    pub color_index: i32,
    pub color_accumulator: NativeF64Bits,
    pub damage: i32,
    pub gates: SparkDrawGates,
    pub draw_ordinal: u64,
}

#[derive(Debug, Clone, Copy)]
pub struct WordPlane<'a> {
    pub words: &'a [u16],
    pub width: usize,
    pub height: usize,
    pub pitch_words: usize,
    pub row_origin: i32,
}

impl WordPlane<'_> {
    pub fn sample(self, x: i32, screen_y: i32) -> Option<u16> {
        let row = screen_y.checked_sub(self.row_origin)?;
        let x = usize::try_from(x).ok()?;
        let row = usize::try_from(row).ok()?;
        if x >= self.width || row >= self.height || self.pitch_words < self.width {
            return None;
        }
        let index = row.checked_mul(self.pitch_words)?.checked_add(x)?;
        self.words.get(index).copied()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TacticalRect {
    pub left: i32,
    pub top: i32,
    pub width: i32,
    pub height: i32,
}

impl TacticalRect {
    pub fn contains(self, point: IVec2) -> bool {
        if self.width <= 0 || self.height <= 0 {
            return false;
        }
        let right = self.left.wrapping_add(self.width);
        let bottom = self.top.wrapping_add(self.height);
        point.x >= self.left && point.x < right && point.y >= self.top && point.y < bottom
    }
}

#[derive(Debug, Clone, Copy)]
pub struct TacticalCompatFrame<'a> {
    pub a_plane: WordPlane<'a>,
    pub z_plane: WordPlane<'a>,
    pub clip: TacticalRect,
    pub tactical_offset_x: i32,
    pub tactical_offset_y: i32,
    pub radar_viewport_offset_y: i32,
    pub adjust_for_z_multiplier: NativeF64Bits,
    pub z_origin_term: i16,
    pub z_bottom_term: i16,
    pub pixel_format: DirectDrawPixelFormat,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PointRejectReason {
    Performance,
    ExtraAnimations,
    Fog,
    OutsideClip,
    AZero,
    ZTest,
    InvalidColorIndex,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PackedPointWrite {
    pub screen: IVec2,
    pub packed_value: u16,
    pub byte_width: u8,
    pub draw_ordinal: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PointResolution {
    Rejected(PointRejectReason),
    Write(PackedPointWrite),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum TacticalCompatError {
    #[error("native-compatible A word is unavailable at ({x},{y})")]
    MissingAWord { x: i32, y: i32 },
    #[error("native-compatible Z word is unavailable at ({x},{y})")]
    MissingZWord { x: i32, y: i32 },
    #[error(transparent)]
    NativeX87(#[from] NativeX87Error),
}

#[cfg(test)]
pub fn resolve_spark_point(
    command: SparkPointCommand,
    color_list: &[[u8; 3]],
    frame: TacticalCompatFrame<'_>,
) -> Result<PointResolution, TacticalCompatError> {
    if !command.gates.performance_passed && command.damage == 0 {
        return Ok(PointResolution::Rejected(PointRejectReason::Performance));
    }
    if !command.gates.extra_animations_enabled {
        return Ok(PointResolution::Rejected(
            PointRejectReason::ExtraAnimations,
        ));
    }
    if !command.gates.fog_passed {
        return Ok(PointResolution::Rejected(PointRejectReason::Fog));
    }

    let screen = project_spark_point(command.world, frame)?;
    if !frame.clip.contains(screen) {
        return Ok(PointResolution::Rejected(PointRejectReason::OutsideClip));
    }

    let a_word =
        frame
            .a_plane
            .sample(screen.x, screen.y)
            .ok_or(TacticalCompatError::MissingAWord {
                x: screen.x,
                y: screen.y,
            })?;
    if a_word == 0 {
        return Ok(PointResolution::Rejected(PointRejectReason::AZero));
    }

    let z_word =
        frame
            .z_plane
            .sample(screen.x, screen.y)
            .ok_or(TacticalCompatError::MissingZWord {
                x: screen.x,
                y: screen.y,
            })?;
    let adjust_for_z = adjust_for_z(command.world.z, frame.adjust_for_z_multiplier)?;
    let candidate = z_candidate(
        frame.z_origin_term,
        frame.z_bottom_term,
        screen.y,
        adjust_for_z,
    );
    if !z_passes(candidate, z_word) {
        return Ok(PointResolution::Rejected(PointRejectReason::ZTest));
    }

    let Some((current, next)) = select_color_pair(command, color_list) else {
        return Ok(PointResolution::Rejected(
            PointRejectReason::InvalidColorIndex,
        ));
    };
    let interpolated = interpolate_rgb(current, next, command.color_accumulator)?;
    let modulated = modulate_rgb(interpolated, a_word);
    let packed_value = pack_rgb(modulated, frame.pixel_format);
    let byte_width = if frame.pixel_format.destination_bytes_per_pixel == 2 {
        2
    } else {
        1
    };

    Ok(PointResolution::Write(PackedPointWrite {
        screen,
        packed_value,
        byte_width,
        draw_ordinal: command.draw_ordinal,
    }))
}

pub fn project_spark_point(
    world: IVec3,
    frame: TacticalCompatFrame<'_>,
) -> Result<IVec2, TacticalCompatError> {
    let (planar_x, planar_y) = project_native_planar(world.x, world.y);
    let z_adjustment = adjust_for_z(world.z, frame.adjust_for_z_multiplier)?;
    Ok(IVec2::new(
        planar_x.wrapping_sub(frame.tactical_offset_x),
        planar_y
            .wrapping_sub(z_adjustment)
            .wrapping_sub(frame.tactical_offset_y)
            .wrapping_add(frame.radar_viewport_offset_y),
    ))
}

/// Native wrap-preserving planar projection: each 60/30 half-term is formed
/// in wrapping i32 before the truncating /256. The single implementation —
/// `combat_light` delegates here, and the wrap contract is pinned by this
/// module's `projection_preserves_native_wrap_before_each_half_term` test.
/// (The i64 no-wrap form for ordinary in-range coordinates is
/// `util::lepton::project_absolute_lepton_xy`.)
pub(crate) fn project_native_planar(world_x: i32, world_y: i32) -> (i32, i32) {
    let planar_x =
        projection_half_term(world_x, 60).wrapping_add(projection_half_term(world_y, -60)) / 256;
    let planar_y =
        projection_half_term(world_x, 30).wrapping_add(projection_half_term(world_y, 30)) / 256;
    (planar_x, planar_y)
}

fn projection_half_term(value: i32, factor: i32) -> i32 {
    value.wrapping_mul(factor) / 2
}

pub fn adjust_for_z(world_z: i32, multiplier: NativeF64Bits) -> Result<i32, NativeX87Error> {
    crate::util::native_x87::adjust_for_z_with_multiplier(world_z, multiplier)
}

pub fn z_candidate(origin_term: i16, bottom_term: i16, screen_y: i32, adjust_for_z: i32) -> i32 {
    let base = i32::from(origin_term)
        .wrapping_add(i32::from(bottom_term))
        .wrapping_sub(screen_y) as u16;
    (base as i32)
        .wrapping_sub(adjust_for_z)
        .wrapping_sub(PARTICLE_Z_BIAS)
}

pub const fn z_passes(candidate: i32, stored: u16) -> bool {
    candidate < stored as i32
}

#[cfg(test)]
fn select_color_pair(
    command: SparkPointCommand,
    color_list: &[[u8; 3]],
) -> Option<([u8; 3], [u8; 3])> {
    let index = usize::try_from(command.color_index).ok()?;
    if index == 0 {
        Some((command.start_rgb, *color_list.get(1)?))
    } else {
        Some((
            *color_list.get(index)?,
            *color_list.get(index.checked_add(1)?)?,
        ))
    }
}

pub fn interpolate_rgb(
    current: [u8; 3],
    next: [u8; 3],
    accumulator: NativeF64Bits,
) -> Result<[i32; 3], NativeX87Error> {
    let one_minus_a = X87Chop53::sub(
        X87Chop53::load_f64(NativeF64Bits::ONE)?,
        X87Chop53::load_f64(accumulator)?,
    );
    Ok([
        interpolate_channel(current[0], next[0], accumulator, one_minus_a)?,
        interpolate_channel(current[1], next[1], accumulator, one_minus_a)?,
        interpolate_channel(current[2], next[2], accumulator, one_minus_a)?,
    ])
}

fn interpolate_channel(
    current: u8,
    next: u8,
    accumulator: NativeF64Bits,
    one_minus_a: X87Value,
) -> Result<i32, NativeX87Error> {
    let next_term = X87Chop53::mul(
        X87Chop53::load_i32(next as i32),
        X87Chop53::load_f64(accumulator)?,
    );
    let current_term = X87Chop53::mul(X87Chop53::load_i32(current as i32), one_minus_a);
    Ok(X87Chop53::ftol_i64(X87Chop53::add(next_term, current_term))? as i32)
}

pub fn modulate_rgb(rgb: [i32; 3], a_word: u16) -> [i32; 3] {
    if a_word >= A_PASSTHROUGH_THRESHOLD {
        return rgb;
    }
    let a = a_word as i32;
    [
        rgb[0].wrapping_mul(a) >> 7,
        rgb[1].wrapping_mul(a) >> 7,
        rgb[2].wrapping_mul(a) >> 7,
    ]
}

pub fn pack_rgb(rgb: [i32; 3], format: DirectDrawPixelFormat) -> u16 {
    pack_channel(rgb[0], format.red_loss, format.red_shift)
        | pack_channel(rgb[1], format.green_loss, format.green_shift)
        | pack_channel(rgb[2], format.blue_loss, format.blue_shift)
}

fn pack_channel(channel: i32, loss: u32, shift: u32) -> u16 {
    let reduced = channel >> (loss & 31);
    (reduced.wrapping_shl(shift & 31) as u32 & 0xffff) as u16
}

#[cfg(test)]
mod tests {
    use super::*;

    const ZERO_MULTIPLIER: NativeF64Bits = NativeF64Bits::POSITIVE_ZERO;
    const SYNTHETIC_565: DirectDrawPixelFormat = DirectDrawPixelFormat {
        red_loss: 3,
        red_shift: 11,
        green_loss: 2,
        green_shift: 5,
        blue_loss: 3,
        blue_shift: 0,
        destination_bytes_per_pixel: 2,
    };

    fn plane(words: &[u16], width: usize, height: usize) -> WordPlane<'_> {
        WordPlane {
            words,
            width,
            height,
            pitch_words: width,
            row_origin: 0,
        }
    }

    fn frame<'a>(a_words: &'a [u16], z_words: &'a [u16]) -> TacticalCompatFrame<'a> {
        TacticalCompatFrame {
            a_plane: plane(a_words, 64, 64),
            z_plane: plane(z_words, 64, 64),
            clip: TacticalRect {
                left: 0,
                top: 0,
                width: 64,
                height: 64,
            },
            tactical_offset_x: 0,
            tactical_offset_y: 0,
            radar_viewport_offset_y: 0,
            adjust_for_z_multiplier: ZERO_MULTIPLIER,
            z_origin_term: 0,
            z_bottom_term: 100,
            pixel_format: SYNTHETIC_565,
        }
    }

    fn command(world: IVec3) -> SparkPointCommand {
        SparkPointCommand {
            world,
            start_rgb: [80, 255, 255],
            color_index: 0,
            color_accumulator: NativeF64Bits::POSITIVE_ZERO,
            damage: 0,
            gates: SparkDrawGates {
                performance_passed: true,
                extra_animations_enabled: true,
                fog_passed: true,
            },
            draw_ordinal: 42,
        }
    }

    #[test]
    fn verified_rgb565_and_rgb555_layouts_pack_expected_masks() {
        use crate::render::native_surface_format::{RGB555, RGB565};

        assert_eq!(pack_rgb([255, 255, 255], RGB565), 0xffff);
        assert_eq!(pack_rgb([255, 0, 0], RGB565), 0xf800);
        assert_eq!(pack_rgb([0, 255, 0], RGB565), 0x07e0);
        assert_eq!(pack_rgb([0, 0, 255], RGB565), 0x001f);
        assert_eq!(pack_rgb([255, 255, 0], RGB565), 0xffe0);

        assert_eq!(pack_rgb([255, 255, 255], RGB555), 0x7fff);
        assert_eq!(pack_rgb([255, 0, 0], RGB555), 0x7c00);
        assert_eq!(pack_rgb([0, 255, 0], RGB555), 0x03e0);
        assert_eq!(pack_rgb([0, 0, 255], RGB555), 0x001f);
        assert_eq!(pack_rgb([255, 255, 0], RGB555), 0x7fe0);
    }

    #[test]
    fn projection_matches_verified_signed_fixtures() {
        let a = vec![127; 64 * 64];
        let z = vec![u16::MAX; 64 * 64];
        let frame = frame(&a, &z);
        assert_eq!(
            project_spark_point(IVec3::new(256, 0, 0), frame).unwrap(),
            IVec2::new(30, 15),
        );
        assert_eq!(
            project_spark_point(IVec3::new(-1, 0, 0), frame).unwrap(),
            IVec2::new(0, 0),
        );
    }

    #[test]
    fn adjust_for_z_retains_injected_multiplier_contract() {
        assert_eq!(adjust_for_z(1_500, ZERO_MULTIPLIER).unwrap(), 1);
        assert_eq!(adjust_for_z(-400, ZERO_MULTIPLIER).unwrap(), 0);
        assert_eq!(
            adjust_for_z(
                256,
                crate::util::native_x87::STANDARD_ADJUST_FOR_Z_MULTIPLIER,
            )
            .unwrap(),
            37,
        );
    }

    #[test]
    fn projection_preserves_native_wrap_before_each_half_term() {
        let a = vec![127; 64 * 64];
        let z = vec![u16::MAX; 64 * 64];
        let frame = frame(&a, &z);
        assert_eq!(
            project_spark_point(IVec3::new(50_000_000, 0, 0), frame).unwrap(),
            IVec2::new(-2_529_233, 2_929_687),
        );
        assert_eq!(
            project_spark_point(IVec3::new(i32::MAX, 1, 0), frame).unwrap(),
            IVec2::ZERO,
        );
    }

    #[test]
    fn clip_is_inclusive_left_top_and_exclusive_right_bottom() {
        let clip = TacticalRect {
            left: 10,
            top: 20,
            width: 4,
            height: 3,
        };
        assert!(clip.contains(IVec2::new(10, 20)));
        assert!(clip.contains(IVec2::new(13, 22)));
        assert!(!clip.contains(IVec2::new(14, 22)));
        assert!(!clip.contains(IVec2::new(13, 23)));
        assert!(!clip.contains(IVec2::new(9, 20)));
        assert!(!clip.contains(IVec2::new(10, 19)));
    }

    #[test]
    fn complete_a_word_threshold_domain_matches_native_discontinuity() {
        let rgb = [80, 255, 255];
        assert_eq!(modulate_rgb(rgb, 1), [0, 1, 1]);
        assert_eq!(modulate_rgb(rgb, 126), [78, 251, 251]);
        assert_eq!(modulate_rgb(rgb, 127), rgb);
        assert_eq!(modulate_rgb(rgb, 128), rgb);
        assert_eq!(modulate_rgb(rgb, 65_535), rgb);
        assert_eq!(modulate_rgb([-1, -128, 255], 1), [-1, -1, 1]);
    }

    #[test]
    fn z_uses_wrapped_u16_base_and_strict_signed_comparison() {
        let candidate = z_candidate(i16::MAX, i16::MAX, -10, 0);
        let expected_base = (i32::from(i16::MAX)
            .wrapping_add(i32::from(i16::MAX))
            .wrapping_sub(-10) as u16) as i32;
        assert_eq!(candidate, expected_base - 50);
        assert!(!z_passes(100, 99));
        assert!(!z_passes(100, 100));
        assert!(z_passes(100, 101));
        assert!(z_passes(-1, 0));
        assert!(!z_passes(65_536, u16::MAX));
    }

    #[test]
    fn interpolation_uses_start_to_list_one_then_list_pairs() {
        let colors = [[0, 128, 255], [255, 255, 255], [200, 200, 150]];
        let zero = command(IVec3::ZERO);
        assert_eq!(
            select_color_pair(zero, &colors),
            Some(([80, 255, 255], [255, 255, 255])),
        );
        let one = SparkPointCommand {
            color_index: 1,
            ..zero
        };
        assert_eq!(
            select_color_pair(one, &colors),
            Some(([255, 255, 255], [200, 200, 150])),
        );
        assert_eq!(
            interpolate_rgb(
                [80, 255, 255],
                [255, 255, 255],
                NativeF64Bits::POSITIVE_ZERO,
            )
            .unwrap(),
            [80, 255, 255],
        );
        assert_eq!(
            interpolate_rgb([80, 255, 255], [255, 255, 255], NativeF64Bits::ONE,).unwrap(),
            [255, 255, 255],
        );
    }

    #[test]
    fn resolver_reads_a_then_z_and_preserves_z_and_ordinal() {
        let mut a = vec![127; 64 * 64];
        let mut z = vec![u16::MAX; 64 * 64];
        let point = IVec2::new(30, 15);
        let index = point.y as usize * 64 + point.x as usize;
        a[index] = 127;
        z[index] = u16::MAX;
        let z_before = z.clone();
        let colors = [[0, 128, 255], [255, 255, 255]];
        let result =
            resolve_spark_point(command(IVec3::new(256, 0, 0)), &colors, frame(&a, &z)).unwrap();
        assert_eq!(
            result,
            PointResolution::Write(PackedPointWrite {
                screen: point,
                packed_value: 0x57ff,
                byte_width: 2,
                draw_ordinal: 42,
            }),
        );
        assert_eq!(z, z_before);
    }

    #[test]
    fn zero_a_rejects_before_color_and_no_runtime_default_is_needed() {
        let a = vec![0; 64 * 64];
        let z = vec![u16::MAX; 64 * 64];
        let invalid_colors: [[u8; 3]; 0] = [];
        assert_eq!(
            resolve_spark_point(command(IVec3::ZERO), &invalid_colors, frame(&a, &z)).unwrap(),
            PointResolution::Rejected(PointRejectReason::AZero),
        );
    }

    #[test]
    fn gate_order_and_damage_override_are_explicit() {
        let a = vec![127; 64 * 64];
        let z = vec![u16::MAX; 64 * 64];
        let colors = [[0, 0, 0], [255, 255, 255]];
        let base = command(IVec3::ZERO);

        let performance = SparkPointCommand {
            gates: SparkDrawGates {
                performance_passed: false,
                ..base.gates
            },
            ..base
        };
        assert_eq!(
            resolve_spark_point(performance, &colors, frame(&a, &z)).unwrap(),
            PointResolution::Rejected(PointRejectReason::Performance),
        );

        let detail = SparkPointCommand {
            damage: 1,
            gates: SparkDrawGates {
                performance_passed: false,
                extra_animations_enabled: false,
                fog_passed: false,
            },
            ..base
        };
        assert_eq!(
            resolve_spark_point(detail, &colors, frame(&a, &z)).unwrap(),
            PointResolution::Rejected(PointRejectReason::ExtraAnimations),
        );
    }

    #[test]
    fn non_two_byte_destination_contract_keeps_only_one_output_byte() {
        let mut format = SYNTHETIC_565;
        format.destination_bytes_per_pixel = 4;
        let a = vec![127; 64 * 64];
        let z = vec![u16::MAX; 64 * 64];
        let colors = [[0, 0, 0], [255, 255, 255]];
        let mut frame = frame(&a, &z);
        frame.pixel_format = format;
        let PointResolution::Write(write) =
            resolve_spark_point(command(IVec3::ZERO), &colors, frame).unwrap()
        else {
            panic!("passing point must resolve to a write");
        };
        assert_eq!(write.byte_width, 1);
        assert_eq!(write.packed_value as u8, 0xff);
    }
}
