//! Independent frozen pre-shortcut projection. Do not compare two wrappers
//! that both reach the new zero-height branch.
use crate::util::lepton::{absolute_leptons_to_screen, project_absolute_lepton_xy};
use crate::util::native_x87::adjust_for_z_standard;

fn before(x: i32, y: i32, z: i32) -> [u32; 2] {
    let (sx, sy) = project_absolute_lepton_xy(x, y);
    // Original WORLD_ROW_BIAS_PX and original native-x87 operation ordering.
    [sx as f32, (sy + 15 - adjust_for_z_standard(z)) as f32].map(f32::to_bits)
}

#[test]
fn render_interpolation_zero_height_projection_matches_original_native_path() {
    assert_eq!(adjust_for_z_standard(0), 0);
    let xy = [
        i32::MIN,
        -8_388_608,
        -65_537,
        -257,
        -1,
        0,
        1,
        5,
        255,
        256,
        4097,
        8_388_607,
        i32::MAX,
    ];
    let zs = [i32::MIN, -1500, -1, 0, 1, 727, 728, 1500, i32::MAX];
    let check = |x, y, z| {
        let (sx, sy) = absolute_leptons_to_screen(x, y, z);
        assert_eq!([sx, sy].map(f32::to_bits), before(x, y, z), "{x},{y},{z}");
    };
    for x in xy {
        for y in xy {
            for z in zs {
                check(x, y, z);
            }
        }
    }
    // Broad signed values and float conversion boundaries, deterministic and
    // unrelated to the simulation's RNG. All-zero and nonzero Z are included.
    let mut seed = 0x2026_1020_u32;
    for index in 0..65_536 {
        seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        let x = seed as i32;
        seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        let y = seed as i32;
        check(
            x,
            y,
            if index % 4 == 0 {
                zs[index % zs.len()]
            } else {
                0
            },
        );
    }
}
