//! Exact active-YR `Inviso=yes` impact-animation coordinate scatter.
//!
//! The binary consumes one low byte from `ScenarioClass::Random`, looks up two
//! `f32` samples, and evaluates the radius-32 CoordStruct math under the
//! process x87 control word (53-bit precision, truncate toward zero). Damage
//! keeps the original coordinate; this module only derives the animation
//! coordinate.

use crate::sim::rng::SimRng;
use crate::util::fixed_math::SimFixed;
use crate::util::native_x87::{NativeF32Bits, X87Chop53};

const LEPTONS_PER_CELL: i32 = crate::util::lepton::LEPTONS_PER_CELL_I32;
const MAP_CELL_LIMIT: u32 = 512;
const INVISO_ANIM_RADIUS: i32 = 0x20;

// Binary-derived samples reached by the 256 possible low bytes at
// 0x004CACB0. Source table: gamemd.exe 0x0084F084.
// SHA-256 over these little-endian u32 values:
// 6dcc1d9e3c620727e0e60f4219a481cee2c666429430cfc3d3bab3e8442f14de
const SINE_BITS: [u32; 256] = [
    0x3F800000, 0x3F7FEC43, 0x3F7FB10F, 0x3F7F4E6D, 0x3F7EC46D, 0x3F7E1323, 0x3F7D3AAB, 0x3F7C3B27,
    0x3F7B14BE, 0x3F79C79D, 0x3F7853F7, 0x3F76BA07, 0x3F74FA0A, 0x3F731447, 0x3F710908, 0x3F6ED89D,
    0x3F6C835E, 0x3F6A09A6, 0x3F676BD7, 0x3F64AA59, 0x3F61C597, 0x3F5EBE05, 0x3F5B941A, 0x3F584852,
    0x3F54DB31, 0x3F514D3D, 0x3F4D9F02, 0x3F49D112, 0x3F45E403, 0x3F41D870, 0x3F3DAEF9, 0x3F396841,
    0x3F3504F3, 0x3F3085BA, 0x3F2BEB49, 0x3F273655, 0x3F226799, 0x3F1D7FD1, 0x3F187FBF, 0x3F13682A,
    0x3F0E39D9, 0x3F08F59A, 0x3F039C3C, 0x3EFC5D26, 0x3EF15AE9, 0x3EE63374, 0x3EDAE880, 0x3ECF7BCA,
    0x3EC3EF15, 0x3EB84429, 0x3EAC7CD3, 0x3EA09AE4, 0x3E94A031, 0x3E888E93, 0x3E78CFCB, 0x3E605C13,
    0x3E47C5C1, 0x3E2F10A2, 0x3E164083, 0x3DFAB272, 0x3DC8BD35, 0x3D96A904, 0x3D48FB2F, 0x3CC90AAF,
    0x00000000, 0xBCC90AAF, 0xBD48FB2F, 0xBD96A904, 0xBDC8BD35, 0xBDFAB272, 0xBE164083, 0xBE2F10A2,
    0xBE47C5C1, 0xBE605C13, 0xBE78CFCB, 0xBE888E93, 0xBE94A031, 0xBEA09AE4, 0xBEAC7CD3, 0xBEB84429,
    0xBEC3EF15, 0xBECF7BCA, 0xBEDAE880, 0xBEE63374, 0xBEF15AE9, 0xBEFC5D26, 0xBF039C3C, 0xBF08F59A,
    0xBF0E39D9, 0xBF13682A, 0xBF187FBF, 0xBF1D7FD1, 0xBF226799, 0xBF273655, 0xBF2BEB49, 0xBF3085BA,
    0xBF3504F3, 0xBF396841, 0xBF3DAEF9, 0xBF41D870, 0xBF45E403, 0xBF49D112, 0xBF4D9F02, 0xBF514D3D,
    0xBF54DB31, 0xBF584852, 0xBF5B941A, 0xBF5EBE05, 0xBF61C597, 0xBF64AA59, 0xBF676BD7, 0xBF6A09A6,
    0xBF6C835E, 0xBF6ED89D, 0xBF710908, 0xBF731447, 0xBF74FA0A, 0xBF76BA07, 0xBF7853F7, 0xBF79C79D,
    0xBF7B14BE, 0xBF7C3B27, 0xBF7D3AAB, 0xBF7E1323, 0xBF7EC46D, 0xBF7F4E6D, 0xBF7FB10F, 0xBF7FEC43,
    0xBF800000, 0xBF7FEC43, 0xBF7FB10F, 0xBF7F4E6D, 0xBF7EC46D, 0xBF7E1323, 0xBF7D3AAB, 0xBF7C3B27,
    0xBF7B14BE, 0xBF79C79D, 0xBF7853F7, 0xBF76BA07, 0xBF74FA0A, 0xBF731447, 0xBF710908, 0xBF6ED89D,
    0xBF6C835E, 0xBF6A09A6, 0xBF676BD7, 0xBF64AA59, 0xBF61C597, 0xBF5EBE05, 0xBF5B941A, 0xBF584852,
    0xBF54DB31, 0xBF514D3D, 0xBF4D9F02, 0xBF49D112, 0xBF45E403, 0xBF41D870, 0xBF3DAEF9, 0xBF396841,
    0xBF3504F3, 0xBF3085BA, 0xBF2BEB49, 0xBF273655, 0xBF226799, 0xBF1D7FD1, 0xBF187FBF, 0xBF13682A,
    0xBF0E39D9, 0xBF08F59A, 0xBF039C3C, 0xBEFC5D26, 0xBEF15AE9, 0xBEE63374, 0xBEDAE880, 0xBECF7BCA,
    0xBEC3EF15, 0xBEB84429, 0xBEAC7CD3, 0xBEA09AE4, 0xBE94A031, 0xBE888E93, 0xBE78CFCB, 0xBE605C13,
    0xBE47C5C1, 0xBE2F10A2, 0xBE164083, 0xBDFAB272, 0xBDC8BD35, 0xBD96A904, 0xBD48FB2F, 0xBCC90AAF,
    0x250D3000, 0x3CC90AAF, 0x3D48FB2F, 0x3D96A904, 0x3DC8BD35, 0x3DFAB272, 0x3E164083, 0x3E2F10A2,
    0x3E47C5C1, 0x3E605C13, 0x3E78CFCB, 0x3E888E93, 0x3E94A031, 0x3EA09AE4, 0x3EAC7CD3, 0x3EB84429,
    0x3EC3EF15, 0x3ECF7BCA, 0x3EDAE880, 0x3EE63374, 0x3EF15AE9, 0x3EFC5D26, 0x3F039C3C, 0x3F08F59A,
    0x3F0E39D9, 0x3F13682A, 0x3F187FBF, 0x3F1D7FD1, 0x3F226799, 0x3F273655, 0x3F2BEB49, 0x3F3085BA,
    0x3F3504F3, 0x3F396841, 0x3F3DAEF9, 0x3F41D870, 0x3F45E403, 0x3F49D112, 0x3F4D9F02, 0x3F514D3D,
    0x3F54DB31, 0x3F584852, 0x3F5B941A, 0x3F5EBE05, 0x3F61C597, 0x3F64AA59, 0x3F676BD7, 0x3F6A09A6,
    0x3F6C835E, 0x3F6ED89D, 0x3F710908, 0x3F731447, 0x3F74FA0A, 0x3F76BA07, 0x3F7853F7, 0x3F79C79D,
    0x3F7B14BE, 0x3F7C3B27, 0x3F7D3AAB, 0x3F7E1323, 0x3F7EC46D, 0x3F7F4E6D, 0x3F7FB10F, 0x3F7FEC43,
];

// Binary-derived samples reached by the 256 possible low bytes at
// 0x004CAD00. SHA-256 over these little-endian u32 values:
// 7240c6b2b9b8ee34e514593b845e4194701a381325aab77f2910721761b17737
const COSINE_BITS: [u32; 256] = [
    0x250D3000, 0x3CC90AAF, 0x3D48FB2F, 0x3D96A904, 0x3DC8BD35, 0x3DFAB272, 0x3E164083, 0x3E2F10A2,
    0x3E47C5C1, 0x3E605C13, 0x3E78CFCB, 0x3E888E93, 0x3E94A031, 0x3EA09AE4, 0x3EAC7CD3, 0x3EB84429,
    0x3EC3EF15, 0x3ECF7BCA, 0x3EDAE880, 0x3EE63374, 0x3EF15AE9, 0x3EFC5D26, 0x3F039C3C, 0x3F08F59A,
    0x3F0E39D9, 0x3F13682A, 0x3F187FBF, 0x3F1D7FD1, 0x3F226799, 0x3F273655, 0x3F2BEB49, 0x3F3085BA,
    0x3F3504F3, 0x3F396841, 0x3F3DAEF9, 0x3F41D870, 0x3F45E403, 0x3F49D112, 0x3F4D9F02, 0x3F514D3D,
    0x3F54DB31, 0x3F584852, 0x3F5B941A, 0x3F5EBE05, 0x3F61C597, 0x3F64AA59, 0x3F676BD7, 0x3F6A09A6,
    0x3F6C835E, 0x3F6ED89D, 0x3F710908, 0x3F731447, 0x3F74FA0A, 0x3F76BA07, 0x3F7853F7, 0x3F79C79D,
    0x3F7B14BE, 0x3F7C3B27, 0x3F7D3AAB, 0x3F7E1323, 0x3F7EC46D, 0x3F7F4E6D, 0x3F7FB10F, 0x3F7FEC43,
    0x3F800000, 0x3F7FEC43, 0x3F7FB10F, 0x3F7F4E6D, 0x3F7EC46D, 0x3F7E1323, 0x3F7D3AAB, 0x3F7C3B27,
    0x3F7B14BE, 0x3F79C79D, 0x3F7853F7, 0x3F76BA07, 0x3F74FA0A, 0x3F731447, 0x3F710908, 0x3F6ED89D,
    0x3F6C835E, 0x3F6A09A6, 0x3F676BD7, 0x3F64AA59, 0x3F61C597, 0x3F5EBE05, 0x3F5B941A, 0x3F584852,
    0x3F54DB31, 0x3F514D3D, 0x3F4D9F02, 0x3F49D112, 0x3F45E403, 0x3F41D870, 0x3F3DAEF9, 0x3F396841,
    0x3F3504F3, 0x3F3085BA, 0x3F2BEB49, 0x3F273655, 0x3F226799, 0x3F1D7FD1, 0x3F187FBF, 0x3F13682A,
    0x3F0E39D9, 0x3F08F59A, 0x3F039C3C, 0x3EFC5D26, 0x3EF15AE9, 0x3EE63374, 0x3EDAE880, 0x3ECF7BCA,
    0x3EC3EF15, 0x3EB84429, 0x3EAC7CD3, 0x3EA09AE4, 0x3E94A031, 0x3E888E93, 0x3E78CFCB, 0x3E605C13,
    0x3E47C5C1, 0x3E2F10A2, 0x3E164083, 0x3DFAB272, 0x3DC8BD35, 0x3D96A904, 0x3D48FB2F, 0x3CC90AAF,
    0xA58D3000, 0xBCC90AAF, 0xBD48FB2F, 0xBD96A904, 0xBDC8BD35, 0xBDFAB272, 0xBE164083, 0xBE2F10A2,
    0xBE47C5C1, 0xBE605C13, 0xBE78CFCB, 0xBE888E93, 0xBE94A031, 0xBEA09AE4, 0xBEAC7CD3, 0xBEB84429,
    0xBEC3EF15, 0xBECF7BCA, 0xBEDAE880, 0xBEE63374, 0xBEF15AE9, 0xBEFC5D26, 0xBF039C3C, 0xBF08F59A,
    0xBF0E39D9, 0xBF13682A, 0xBF187FBF, 0xBF1D7FD1, 0xBF226799, 0xBF273655, 0xBF2BEB49, 0xBF3085BA,
    0xBF3504F3, 0xBF396841, 0xBF3DAEF9, 0xBF41D870, 0xBF45E403, 0xBF49D112, 0xBF4D9F02, 0xBF514D3D,
    0xBF54DB31, 0xBF584852, 0xBF5B941A, 0xBF5EBE05, 0xBF61C597, 0xBF64AA59, 0xBF676BD7, 0xBF6A09A6,
    0xBF6C835E, 0xBF6ED89D, 0xBF710908, 0xBF731447, 0xBF74FA0A, 0xBF76BA07, 0xBF7853F7, 0xBF79C79D,
    0xBF7B14BE, 0xBF7C3B27, 0xBF7D3AAB, 0xBF7E1323, 0xBF7EC46D, 0xBF7F4E6D, 0xBF7FB10F, 0xBF7FEC43,
    0xBF800000, 0xBF7FEC43, 0xBF7FB10F, 0xBF7F4E6D, 0xBF7EC46D, 0xBF7E1323, 0xBF7D3AAB, 0xBF7C3B27,
    0xBF7B14BE, 0xBF79C79D, 0xBF7853F7, 0xBF76BA07, 0xBF74FA0A, 0xBF731447, 0xBF710908, 0xBF6ED89D,
    0xBF6C835E, 0xBF6A09A6, 0xBF676BD7, 0xBF64AA59, 0xBF61C597, 0xBF5EBE05, 0xBF5B941A, 0xBF584852,
    0xBF54DB31, 0xBF514D3D, 0xBF4D9F02, 0xBF49D112, 0xBF45E403, 0xBF41D870, 0xBF3DAEF9, 0xBF396841,
    0xBF3504F3, 0xBF3085BA, 0xBF2BEB49, 0xBF273655, 0xBF226799, 0xBF1D7FD1, 0xBF187FBF, 0xBF13682A,
    0xBF0E39D9, 0xBF08F59A, 0xBF039C3C, 0xBEFC5D26, 0xBEF15AE9, 0xBEE63374, 0xBEDAE880, 0xBECF7BCA,
    0xBEC3EF15, 0xBEB84429, 0xBEAC7CD3, 0xBEA09AE4, 0xBE94A031, 0xBE888E93, 0xBE78CFCB, 0xBE605C13,
    0xBE47C5C1, 0xBE2F10A2, 0xBE164083, 0xBDFAB272, 0xBDC8BD35, 0xBD96A904, 0xBD48FB2F, 0xBCC90AAF,
];

/// Consume one Scenario-RNG draw and derive only the impact-animation coordinate.
pub(crate) fn scatter_inviso_effect_coord(
    rng: &mut SimRng,
    rx: u16,
    ry: u16,
    sub_x: SimFixed,
    sub_y: SimFixed,
) -> (u16, u16, SimFixed, SimFixed) {
    let base_x = i32::from(rx) * LEPTONS_PER_CELL + sub_x.to_num::<i32>();
    let base_y = i32::from(ry) * LEPTONS_PER_CELL + sub_y.to_num::<i32>();
    let (x, y) = random_direction_coord(rng, base_x, base_y, INVISO_ANIM_RADIUS);
    split_valid_coord(x, y)
}

#[cfg(test)]
fn scatter_effect_coord_for_byte(
    byte: u8,
    rx: u16,
    ry: u16,
    sub_x: SimFixed,
    sub_y: SimFixed,
) -> (u16, u16, SimFixed, SimFixed) {
    let base_x = i32::from(rx) * LEPTONS_PER_CELL + sub_x.to_num::<i32>();
    let base_y = i32::from(ry) * LEPTONS_PER_CELL + sub_y.to_num::<i32>();
    let (x, y) = random_direction_coord_for_byte(byte, base_x, base_y, INVISO_ANIM_RADIUS);
    split_valid_coord(x, y)
}

/// Consume one raw Scenario RNG draw and apply the active-YR random-direction
/// CoordStruct helper at an arbitrary magnitude.
///
/// The returned coordinate falls back as a whole to `(base_x, base_y)` when
/// either native signed cell conversion lands outside the 512-cell domain.
pub(crate) fn random_direction_coord(
    rng: &mut SimRng,
    base_x: i32,
    base_y: i32,
    magnitude_leptons: i32,
) -> (i32, i32) {
    let byte = (rng.next_u32() & 0xff) as u8;
    random_direction_coord_for_byte(byte, base_x, base_y, magnitude_leptons)
}

pub(crate) fn random_direction_coord_for_byte(
    byte: u8,
    base_x: i32,
    base_y: i32,
    magnitude_leptons: i32,
) -> (i32, i32) {
    let radius = X87Chop53::load_i32(magnitude_leptons);
    let cosine = X87Chop53::load_f32(NativeF32Bits::from_bits(COSINE_BITS[usize::from(byte)]))
        .expect("binary cosine sample is finite and normal");
    let sine = X87Chop53::load_f32(NativeF32Bits::from_bits(SINE_BITS[usize::from(byte)]))
        .expect("binary sine sample is finite or zero");

    let x = X87Chop53::ftol_i64(X87Chop53::add(
        X87Chop53::load_i32(base_x),
        X87Chop53::mul(cosine, radius),
    ))
    .expect("Inviso X remains in the signed 32-bit map domain") as i32;
    let y = X87Chop53::ftol_i64(X87Chop53::sub(
        X87Chop53::load_i32(base_y),
        X87Chop53::mul(sine, radius),
    ))
    .expect("Inviso Y remains in the signed 32-bit map domain") as i32;

    let x_cell = coord_to_cell_truncating(x);
    let y_cell = coord_to_cell_truncating(y);
    if (x_cell as u32) >= MAP_CELL_LIMIT || (y_cell as u32) >= MAP_CELL_LIMIT {
        return (base_x, base_y);
    }

    (x, y)
}

fn split_valid_coord(x: i32, y: i32) -> (u16, u16, SimFixed, SimFixed) {
    let x_cell = coord_to_cell_truncating(x);
    let y_cell = coord_to_cell_truncating(y);
    (
        x_cell as u16,
        y_cell as u16,
        SimFixed::from_num(x - x_cell * LEPTONS_PER_CELL),
        SimFixed::from_num(y - y_cell * LEPTONS_PER_CELL),
    )
}

/// The native helper implements signed division by 256 with `CDQ/AND/ADD/SAR`,
/// so negative coordinates truncate toward zero rather than floor.
pub(crate) fn coord_to_cell_truncating(coord: i32) -> i32 {
    (coord + if coord.is_negative() { 0xff } else { 0 }) >> 8
}

#[cfg(test)]
mod tests {
    use super::*;

    // Generated independently from the live table samples for a base coordinate
    // of 65,536 leptons. At that map-scale base, every f32*32 product and
    // integer addition is exactly representable before native ftol.
    const EXPECTED_OFFSETS: [(i8, i8); 256] = [
        (0, -32),
        (0, -32),
        (1, -32),
        (2, -32),
        (3, -32),
        (3, -32),
        (4, -32),
        (5, -32),
        (6, -32),
        (7, -32),
        (7, -32),
        (8, -31),
        (9, -31),
        (10, -31),
        (10, -31),
        (11, -30),
        (12, -30),
        (12, -30),
        (13, -29),
        (14, -29),
        (15, -29),
        (15, -28),
        (16, -28),
        (17, -28),
        (17, -27),
        (18, -27),
        (19, -26),
        (19, -26),
        (20, -25),
        (20, -25),
        (21, -24),
        (22, -24),
        (22, -23),
        (23, -23),
        (23, -22),
        (24, -21),
        (24, -21),
        (25, -20),
        (25, -20),
        (26, -19),
        (26, -18),
        (27, -18),
        (27, -17),
        (27, -16),
        (28, -16),
        (28, -15),
        (28, -14),
        (29, -13),
        (29, -13),
        (29, -12),
        (30, -11),
        (30, -11),
        (30, -10),
        (30, -9),
        (31, -8),
        (31, -8),
        (31, -7),
        (31, -6),
        (31, -5),
        (31, -4),
        (31, -4),
        (31, -3),
        (31, -2),
        (31, -1),
        (32, 0),
        (31, 0),
        (31, 1),
        (31, 2),
        (31, 3),
        (31, 3),
        (31, 4),
        (31, 5),
        (31, 6),
        (31, 7),
        (31, 7),
        (30, 8),
        (30, 9),
        (30, 10),
        (30, 10),
        (29, 11),
        (29, 12),
        (29, 12),
        (28, 13),
        (28, 14),
        (28, 15),
        (27, 15),
        (27, 16),
        (27, 17),
        (26, 17),
        (26, 18),
        (25, 19),
        (25, 19),
        (24, 20),
        (24, 20),
        (23, 21),
        (23, 22),
        (22, 22),
        (22, 23),
        (21, 23),
        (20, 24),
        (20, 24),
        (19, 25),
        (19, 25),
        (18, 26),
        (17, 26),
        (17, 27),
        (16, 27),
        (15, 27),
        (15, 28),
        (14, 28),
        (13, 28),
        (12, 29),
        (12, 29),
        (11, 29),
        (10, 30),
        (10, 30),
        (9, 30),
        (8, 30),
        (7, 31),
        (7, 31),
        (6, 31),
        (5, 31),
        (4, 31),
        (3, 31),
        (3, 31),
        (2, 31),
        (1, 31),
        (0, 31),
        (-1, 32),
        (-1, 31),
        (-2, 31),
        (-3, 31),
        (-4, 31),
        (-4, 31),
        (-5, 31),
        (-6, 31),
        (-7, 31),
        (-8, 31),
        (-8, 31),
        (-9, 30),
        (-10, 30),
        (-11, 30),
        (-11, 30),
        (-12, 29),
        (-13, 29),
        (-13, 29),
        (-14, 28),
        (-15, 28),
        (-16, 28),
        (-16, 27),
        (-17, 27),
        (-18, 27),
        (-18, 26),
        (-19, 26),
        (-20, 25),
        (-20, 25),
        (-21, 24),
        (-21, 24),
        (-22, 23),
        (-23, 23),
        (-23, 22),
        (-24, 22),
        (-24, 21),
        (-25, 20),
        (-25, 20),
        (-26, 19),
        (-26, 19),
        (-27, 18),
        (-27, 17),
        (-28, 17),
        (-28, 16),
        (-28, 15),
        (-29, 15),
        (-29, 14),
        (-29, 13),
        (-30, 12),
        (-30, 12),
        (-30, 11),
        (-31, 10),
        (-31, 10),
        (-31, 9),
        (-31, 8),
        (-32, 7),
        (-32, 7),
        (-32, 6),
        (-32, 5),
        (-32, 4),
        (-32, 3),
        (-32, 3),
        (-32, 2),
        (-32, 1),
        (-32, 0),
        (-32, -1),
        (-32, -1),
        (-32, -2),
        (-32, -3),
        (-32, -4),
        (-32, -4),
        (-32, -5),
        (-32, -6),
        (-32, -7),
        (-32, -8),
        (-32, -8),
        (-31, -9),
        (-31, -10),
        (-31, -11),
        (-31, -11),
        (-30, -12),
        (-30, -13),
        (-30, -13),
        (-29, -14),
        (-29, -15),
        (-29, -16),
        (-28, -16),
        (-28, -17),
        (-28, -18),
        (-27, -18),
        (-27, -19),
        (-26, -20),
        (-26, -20),
        (-25, -21),
        (-25, -21),
        (-24, -22),
        (-24, -23),
        (-23, -23),
        (-23, -24),
        (-22, -24),
        (-21, -25),
        (-21, -25),
        (-20, -26),
        (-20, -26),
        (-19, -27),
        (-18, -27),
        (-18, -28),
        (-17, -28),
        (-16, -28),
        (-16, -29),
        (-15, -29),
        (-14, -29),
        (-13, -30),
        (-13, -30),
        (-12, -30),
        (-11, -31),
        (-11, -31),
        (-10, -31),
        (-9, -31),
        (-8, -32),
        (-8, -32),
        (-7, -32),
        (-6, -32),
        (-5, -32),
        (-4, -32),
        (-4, -32),
        (-3, -32),
        (-2, -32),
        (-1, -32),
    ];

    fn flat(rx: u16, sub: SimFixed) -> i32 {
        i32::from(rx) * LEPTONS_PER_CELL + sub.to_num::<i32>()
    }

    /// `tools/projectile_oracle/launch_scatter.json`: the original helper
    /// (`0x0049F420`, no cell snap) at the cluster distances 0x100, 0x155 and
    /// 0x200 for every angle byte, and beside the map's edges.
    #[test]
    fn cluster_distances_match_the_native_helper() {
        let corpus: serde_json::Value = serde_json::from_str(include_str!(
            "../../../tools/projectile_oracle/launch_scatter.json"
        ))
        .unwrap();
        let rows = corpus["direction"].as_array().unwrap();
        assert_eq!(rows.len(), 792);
        for (index, row) in rows.iter().enumerate() {
            let input = &row["input"];
            let base = |axis: usize| input["base"][axis].as_i64().unwrap() as i32;
            let (x, y) = random_direction_coord_for_byte(
                (input["raw"].as_u64().unwrap() & 0xff) as u8,
                base(0),
                base(1),
                input["distance"].as_i64().unwrap() as i32,
            );
            let native = |axis: usize| row["result"][axis].as_i64().unwrap() as i32;
            assert_eq!(
                (x, y, base(2)),
                (native(0), native(1), native(2)),
                "row {index}"
            );
        }
    }

    #[test]
    fn all_256_binary_samples_match_the_radius_32_oracle() {
        let base = 65_536;
        for (byte, &(expected_x, expected_y)) in EXPECTED_OFFSETS.iter().enumerate() {
            let (rx, ry, sub_x, sub_y) =
                scatter_effect_coord_for_byte(byte as u8, 256, 256, SimFixed::ZERO, SimFixed::ZERO);
            assert_eq!(
                flat(rx, sub_x) - base,
                i32::from(expected_x),
                "X byte {byte}"
            );
            assert_eq!(
                flat(ry, sub_y) - base,
                i32::from(expected_y),
                "Y byte {byte}"
            );
        }
    }

    #[test]
    fn cardinals_include_native_tiny_sample_chop_bias() {
        assert_eq!(EXPECTED_OFFSETS[0], (0, -32));
        assert_eq!(EXPECTED_OFFSETS[64], (32, 0));
        assert_eq!(EXPECTED_OFFSETS[128], (-1, 32));
        assert_eq!(EXPECTED_OFFSETS[192], (-32, -1));
    }

    #[test]
    fn boundary_check_uses_native_truncation_and_whole_coord_fallback() {
        let accepted =
            scatter_effect_coord_for_byte(192, 0, 7, SimFixed::ZERO, SimFixed::from_num(19));
        assert_eq!(accepted.0, 0);
        assert_eq!(accepted.2.to_num::<i32>(), -32);

        let base = (511, 7, SimFixed::from_num(250), SimFixed::from_num(19));
        assert_eq!(
            scatter_effect_coord_for_byte(64, base.0, base.1, base.2, base.3),
            base
        );
    }

    #[test]
    fn public_helper_consumes_exactly_one_raw_scenario_rng_draw() {
        let mut rng = SimRng::new(1);
        let mut reference = rng.clone();
        let byte = (reference.next_u32() & 0xff) as u8;
        let got = scatter_inviso_effect_coord(
            &mut rng,
            20,
            30,
            SimFixed::from_num(128),
            SimFixed::from_num(128),
        );
        let expected = scatter_effect_coord_for_byte(
            byte,
            20,
            30,
            SimFixed::from_num(128),
            SimFixed::from_num(128),
        );
        assert_eq!(got, expected);
        assert_eq!(rng.logical_state(), reference.logical_state());
    }

    #[test]
    fn gsi_04_11_radius_128_cardinals_fallback_and_draw_count_match_native() {
        let base = 65_536;
        let offsets = [
            (0_u8, (0, -128)),
            (64, (128, 0)),
            (128, (-1, 128)),
            (192, (-128, -1)),
        ];
        for (byte, expected) in offsets {
            let got = random_direction_coord_for_byte(byte, base, base, 0x80);
            assert_eq!((got.0 - base, got.1 - base), expected, "byte {byte}");
        }

        let edge = (511 * LEPTONS_PER_CELL + 250, 7 * LEPTONS_PER_CELL + 19);
        assert_eq!(
            random_direction_coord_for_byte(64, edge.0, edge.1, 0x80),
            edge,
            "one out-of-domain axis falls the whole coordinate back"
        );

        let mut actual_rng = SimRng::new(42);
        let mut reference_rng = actual_rng.clone();
        let _ = random_direction_coord(&mut actual_rng, base, base, 0x80);
        let _ = reference_rng.next_u32();
        assert_eq!(actual_rng.logical_state(), reference_rng.logical_state());
    }
}
