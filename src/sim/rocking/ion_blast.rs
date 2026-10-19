//! IonBlast53CFC0..53D2D1 writes angles, preserving angular rates. This is
//! distinct from ApplyRocker70B280, which writes rates. Both use RockingState.
//! Numeric controls: tools/ion_blast_forces.json, original53CBE0 execution.

use super::{RockingState, absolute, double, load, store};
use crate::map::retail_trig::TrigTable;
use crate::util::native_x87::{NativeF32Bits, X87Chop53 as X, X87Ordering, sqrt_approx_f32};

///53CEDA..53CF33: screen translation cancels; neither camera nor viewport
///admission enters the force. Sampling is at ftol(Sqrt_Approx(dx²+dy²))+8.
pub(crate) fn projected_radius(origin: [i32; 3], position: [i32; 3]) -> i32 {
    let project = |[x, y, z]: [i32; 3]| {
        let (x, y) = crate::util::lepton::project_absolute_lepton_xy(x, y);
        [
            x,
            y.wrapping_sub(crate::util::native_x87::adjust_for_z_standard(z)),
        ]
    };
    let a = project(origin);
    let b = project(position);
    let dx = X::load_i32(b[0].wrapping_sub(a[0]));
    let dy = X::load_i32(b[1].wrapping_sub(a[1]));
    let sum = X::add(X::mul(dx, dx), X::mul(dy, dy));
    let distance = sqrt_approx_f32(sum).expect("finite projected ion distance");
    X::ftol_i32_low_masked(load(distance)).wrapping_add(8)
}

/// The accepted53CFC0 branch. Its caller has already checked the signed
/// ripple byte and loaded main VXL. No Weight, NoDampen, RNG or timer enters.
pub(crate) fn apply(
    state: &mut Option<RockingState>,
    origin: [i32; 3],
    position: [i32; 3],
    facing: u16,
    frame: i32,
) -> bool {
    //53CFE4/53CFEA/53D00A; this direction is opposite ApplyRocker's input.
    let d = [
        origin[0].wrapping_sub(position[0]),
        position[1].wrapping_sub(origin[1]),
        origin[2].wrapping_sub(position[2]),
    ]
    .map(|v| store(X::load_i32(v)));
    let squared = X::add(
        X::add(
            X::mul(load(d[0]), load(d[0])),
            X::mul(load(d[1]), load(d[1])),
        ),
        X::mul(load(d[2]), load(d[2])),
    );
    let distance = sqrt_approx_f32(squared).expect("finite ion distance");
    if X::compare(absolute(load(distance)), double(0x3ef4_f8b5_88e3_68f1)) != X87Ordering::Greater {
        return false;
    }
    let angle = X::load_f64(crate::sim::projectile::native_math::radians(facing)).unwrap();
    let trig = TrigTable::embedded();
    let sin = store(trig.sin_from_table(angle));
    let cos = store(trig.cos_from_table(angle));
    let planar = sqrt_approx_f32(X::add(
        X::mul(load(d[0]), load(d[0])),
        X::mul(load(d[1]), load(d[1])),
    ))
    .unwrap();
    let [nx, ny] = if X::compare(load(planar), X::load_i32(0)) == X87Ordering::Equal {
        [d[0], d[1]]
    } else {
        [
            store(X::div(load(d[0]), load(planar)).unwrap()),
            store(X::div(load(d[1]), load(planar)).unwrap()),
        ]
    };
    let forward = store(X::add(
        X::mul(load(nx), load(cos)),
        X::mul(load(ny), load(sin)),
    ));
    let cross = X::sub(X::mul(load(nx), load(sin)), X::mul(load(ny), load(cos)));
    let mut side = load(sqrt_approx_f32(X::mul(cross, cross)).unwrap());
    let tolerance = double(0x3f2a_36e2_eb1c_432d);
    let reconstructed_x = X::sub(
        X::sub(X::mul(load(cos), load(forward)), X::mul(load(sin), side)),
        load(nx),
    );
    let reconstructed_y = X::sub(
        X::add(X::mul(load(sin), load(forward)), X::mul(load(cos), side)),
        load(ny),
    );
    if X::compare(absolute(reconstructed_x), tolerance) == X87Ordering::Greater
        || X::compare(absolute(reconstructed_y), tolerance) == X87Ordering::Greater
    {
        side = X::neg(side);
    }
    //53D211..53D2A9: derivative of (sin(theta)*3.5+3)*51/(r+51),
    //with the original binary32 spills and binary64 numerator/denominator.
    let denominator = X::store_f64(X::add(load(distance), double(0x4049_8000_0000_0000))).unwrap();
    let theta = X::store_f64(X::mul(
        X::add(
            X::sub(
                load(distance),
                X::mul(
                    X::load_i32(frame),
                    X::load_f64(crate::sim::ion_blast_ripple::RING_STEP).unwrap(),
                ),
            ),
            double(0x4043_0000_0000_0000),
        ),
        double(0x3fbc_28f5_c28f_5c29),
    ))
    .unwrap();
    let amplitude = X::store_f64(X::mul(
        X::add(
            X::mul(
                trig.sin_from_table(X::load_f64(theta).unwrap()),
                double(0x400c_0000_0000_0000),
            ),
            double(0x4008_0000_0000_0000),
        ),
        double(0x4049_8000_0000_0000),
    ))
    .unwrap();
    let denominator = X::load_f64(denominator).unwrap();
    let slope = X::div(
        X::sub(
            X::mul(
                X::mul(
                    X::mul(
                        X::mul(
                            trig.cos_from_table(X::load_f64(theta).unwrap()),
                            double(0x3fbc_28f5_c28f_5c29),
                        ),
                        double(0x4049_8000_0000_0000),
                    ),
                    double(0x400c_0000_0000_0000),
                ),
                denominator,
            ),
            X::load_f64(amplitude).unwrap(),
        ),
        X::mul(denominator, denominator),
    )
    .unwrap();
    let tau = double(0x4019_21fb_5444_2d18);
    let state = state.get_or_insert_with(RockingState::default);
    state.angle_sideways = store(X::mul(X::mul(side, slope), tau));
    state.angle_forwards = store(X::neg(X::mul(X::mul(load(forward), slope), tau)));
    true
}

#[cfg(test)]
#[path = "ion_blast_tests.rs"]
mod tests;
