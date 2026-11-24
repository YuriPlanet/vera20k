//! TechnoClass::ApplyRocker70B280..70B56D, one shared numeric owner.
//! No RNG or timers here: callers choose the impact coordinate and force.
//! Field assignments replace earlier rates; rejected gates preserve sparse
//! absence and existing state. Native controls: unit_rocker_impulse.json.

use super::{RockingState, absolute, double, load, store};
use crate::util::fixed_math::SimFixed;
use crate::util::native_x87::{
    MaskedX87Chop53 as M, MaskedX87Ordering, NativeF32Bits, NativeF64Bits, X87Chop53 as X,
    X87Ordering, sqrt_approx_f32,
};
use crate::util::retail_trig::TrigTable;

#[derive(Debug, Clone, Copy)]
pub struct RockerImpulse {
    pub position: [i32; 3],
    pub origin: [i32; 3],
    pub body_facing: u16,
    pub force: NativeF32Bits,
    /// Existing rules precision boundary. Stock1/3.5 and the retained0/-1
    /// controls are exact; arbitrary authored doubles retain I16F16 tuning.
    pub weight: SimFixed,
    pub no_dampen: bool,
    pub has_loaded_voxel: bool,
}

pub fn apply_rocker_impulse(state: &mut Option<RockingState>, request: RockerImpulse) -> bool {
    //70B290..70B2B7 is the same loaded-main-VXL predicate as6F9E10.
    if !request.has_loaded_voxel {
        return false;
    }
    let d = [
        request.position[0].wrapping_sub(request.origin[0]),
        request.origin[1].wrapping_sub(request.position[1]),
        request.position[2].wrapping_sub(request.origin[2]),
    ]
    .map(|v| store(X::load_i32(v)));
    let squared = X::add(
        X::add(
            X::mul(load(d[0]), load(d[0])),
            X::mul(load(d[1]), load(d[1])),
        ),
        X::mul(load(d[2]), load(d[2])),
    );
    let distance = sqrt_approx_f32(squared).expect("finite squared rocker distance");
    //70B32A..70B341 uses the shared signed word-to-radians owner,
    // including its binary64 store, not a quantized compass step.
    let angle = X::load_f64(crate::util::direction_tables::radians_from_facing16(
        request.body_facing,
    ))
    .unwrap();
    let table = TrigTable::embedded();
    let sin = store(table.sin_from_table(angle));
    let cos = store(table.cos_from_table(angle));
    //70B390..70B3BE: native f32 attenuation constants, double numerator
    // spill, actual double Weight division, then a binary32 scaled force.
    let numerator = X::mul(
        X::sub(
            load(NativeF32Bits::from_bits(0x3d23_d70a)),
            X::mul(load(distance), load(NativeF32Bits::from_bits(0x37d1_b717))),
        ),
        load(request.force),
    );
    let weight = NativeF64Bits::from_bits(request.weight.to_num::<f64>().to_bits());
    let mut scaled = M::store_f32_masked_chop(M::div(
        M::load_f64(X::store_f64(numerator).unwrap()),
        M::load_f64(weight),
    ));
    if X::compare(absolute(load(distance)), double(0x3ef4_f8b5_88e3_68f1)) == X87Ordering::Less
        || matches!(
            M::compare(
                M::load_f32(scaled),
                M::load_f32(NativeF32Bits::from_bits(0x3c23_d70a))
            ),
            MaskedX87Ordering::Less | MaskedX87Ordering::Unordered
        )
    {
        return false;
    }
    if M::compare(
        M::load_f32(scaled),
        M::load_f32(NativeF32Bits::from_bits(0x3d4c_cccd)),
    ) == MaskedX87Ordering::Greater
    {
        scaled = NativeF32Bits::from_bits(0x3d4c_cccd);
    }

    let planar = sqrt_approx_f32(X::add(
        X::mul(load(d[0]), load(d[0])),
        X::mul(load(d[1]), load(d[1])),
    ))
    .expect("finite planar rocker distance");
    let [nx, ny] = if X::compare(load(planar), X::load_i32(0)) == X87Ordering::Equal {
        [d[0], d[1]]
    } else {
        [
            store(X::div(load(d[0]), load(planar)).unwrap()),
            store(X::div(load(d[1]), load(planar)).unwrap()),
        ]
    };
    let forwards = store(X::add(
        X::mul(load(ny), load(sin)),
        X::mul(load(nx), load(cos)),
    ));
    let cross = X::sub(X::mul(load(nx), load(sin)), X::mul(load(ny), load(cos)));
    // The normalized vector's Z is explicitly zero. The cross-vector's
    // other two components therefore contribute exact zero to its length.
    let mut sideways = load(sqrt_approx_f32(X::mul(cross, cross)).unwrap());
    let reconstructed_x = X::sub(
        X::sub(
            X::mul(load(cos), load(forwards)),
            X::mul(load(sin), sideways),
        ),
        load(nx),
    );
    let reconstructed_y = X::sub(
        X::add(
            X::mul(load(sin), load(forwards)),
            X::mul(load(cos), sideways),
        ),
        load(ny),
    );
    let tolerance = double(0x3f2a_36e2_eb1c_432d); //7EC0A8
    if X::compare(absolute(reconstructed_x), tolerance) == X87Ordering::Greater
        || X::compare(absolute(reconstructed_y), tolerance) == X87Ordering::Greater
    {
        sideways = X::neg(sideways);
    }
    let mut forwards = X::mul(load(forwards), load(scaled));
    if !request.no_dampen {
        forwards = X::mul(forwards, double(0x3fe0_0000_0000_0000));
    }
    let state = state.get_or_insert_with(RockingState::default);
    state.vel_forwards = store(forwards);
    state.vel_sideways = store(X::neg(X::mul(sideways, load(scaled))));
    true
}
