//! One original TechnoClass::RockingUpdate70B570..70BCA9 visit.
//! Explicit binary32 stores preserve branch-sensitive small rates. The AI
//! host owns eligibility/order and applies a returned C4 request afterward.

use super::{RockingState, absolute, double, load, store};
use crate::map::entities::EntityCategory;
use crate::rules::object_type::ObjectType;
use crate::sim::game_entity::GameEntity;
use crate::util::fixed_math::SimFixed;
use crate::util::native_x87::{NativeF32Bits, X87Chop53 as X, X87Ordering, X87Value};

const PI4: NativeF32Bits = NativeF32Bits::from_bits(0x3f49_0fdb);
const PI2: NativeF32Bits = NativeF32Bits::from_bits(0x3fc9_0fdb);
const PI10: NativeF32Bits = NativeF32Bits::from_bits(0x3ea0_d97c);
const BASE: NativeF32Bits = NativeF32Bits::from_bits(0x3b03_126f); //7F4E70

/// Pure caller inputs, not competing per-entity state. Production has no
/// DirectRocker-link or building-crush writer yet; native controls supply
/// those branches explicitly through this same numerical owner.
#[derive(Debug, Clone, Copy)]
pub(crate) struct RockingUpdateInputs {
    pub direct_rocker_linked: bool,
    pub is_foot: bool,
    pub crushing_building: bool,
    pub sinking: bool,
    pub crashing: bool,
    pub balloon_hover: bool,
    pub body_facing: u16,
    pub fallback: NativeF32Bits,
}

fn gt(left: X87Value, right: X87Value) -> bool {
    X::compare(left, right) == X87Ordering::Greater
}
fn lt(left: X87Value, right: X87Value) -> bool {
    X::compare(left, right) == X87Ordering::Less
}

///70B700/70B963. The two axes share control flow but deliberately differ
/// at their stored-sum comparison and unlinked damping branches.
fn advance_axis(
    angle: &mut NativeF32Bits,
    rate: &mut NativeF32Bits,
    sideways: bool,
    cap: NativeF32Bits,
    linked: bool,
    fallback: NativeF32Bits,
) {
    let previous = load(*angle);
    let epsilon = double(0x3ef4_f8b5_88e3_68f1);
    let was_positive = gt(previous, epsilon);
    let was_negative = lt(previous, X::neg(epsilon));
    let out_of_range = gt(previous, load(PI2)) || lt(previous, X::neg(load(PI2)));
    if X::compare(load(*rate), X::load_i32(0)) == X87Ordering::Equal {
        *angle = NativeF32Bits::POSITIVE_ZERO;
    } else {
        let sum = X::add(load(*rate), previous);
        *angle = store(sum);
        // Sideways compares its retained x87 sum; forwards reloads the FST
        // local at70BA23. Adjacent-float controls distinguish these paths.
        let compared_sum = if sideways { sum } else { load(*angle) };
        if !linked
            && !out_of_range
            && ((gt(compared_sum, load(cap)) && lt(previous, load(cap)))
                || (lt(compared_sum, X::neg(load(cap))) && gt(previous, X::neg(load(cap)))))
        {
            *angle = if gt(compared_sum, load(cap)) {
                cap
            } else {
                NativeF32Bits::from_bits(cap.bits() ^ 0x8000_0000)
            };
            *rate = NativeF32Bits::POSITIVE_ZERO;
        }
        let velocity = load(*rate);
        let next = if was_positive {
            if linked && !out_of_range {
                X::sub(velocity, X::mul(load(fallback), load(BASE)))
            } else if out_of_range {
                X::add(velocity, load(BASE))
            } else if sideways && gt(velocity, X::load_i32(0)) {
                X::sub(velocity, load(BASE))
            } else {
                X::sub(velocity, double(0x3f74_7ae1_5800_0000)) //7F4E68
            }
        } else if linked && !out_of_range {
            X::add(velocity, X::mul(load(fallback), load(BASE)))
        } else if out_of_range {
            X::sub(velocity, load(BASE))
        } else if sideways && lt(velocity, X::load_i32(0)) {
            X::add(velocity, load(BASE))
        } else {
            X::add(velocity, double(0x3f74_7ae1_5800_0000))
        };
        *rate = store(next);
    }
    if (was_positive && lt(load(*angle), epsilon))
        || (was_negative && gt(load(*angle), X::neg(epsilon)))
    {
        *angle = NativeF32Bits::POSITIVE_ZERO;
        *rate = NativeF32Bits::POSITIVE_ZERO;
    }
}

pub(crate) fn advance_state(state: &mut RockingState, inputs: RockingUpdateInputs) -> bool {
    //70B5AE: retained ship sinking precedes crash and ordinary branches.
    if inputs.sinking {
        if lt(absolute(load(state.angle_forwards)), load(PI4)) {
            let sector = crate::util::direction_tables::dir_from_facing16(inputs.body_facing);
            let previous = load(state.angle_forwards);
            let delta = double(0x3f84_7ae1_47ae_147b); //7E3808, double0.01
            state.angle_forwards = store(if sector == 0 || sector > 5 {
                X::sub(previous, delta)
            } else {
                X::add(previous, delta)
            });
        }
        return false;
    }
    if inputs.crashing {
        advance_crash_spin(state, inputs.balloon_hover);
        return false;
    }
    advance_axis(
        &mut state.angle_sideways,
        &mut state.vel_sideways,
        true,
        PI4,
        inputs.direct_rocker_linked,
        inputs.fallback,
    );
    let cap = if inputs.is_foot && inputs.crushing_building {
        PI10
    } else {
        PI4
    };
    advance_axis(
        &mut state.angle_forwards,
        &mut state.vel_forwards,
        false,
        cap,
        inputs.direct_rocker_linked,
        inputs.fallback,
    );
    super::self_destruct::needs_damage(state)
}

///70B63D crash branch. It integrates retained rates without damping and
/// returns before ordinary wide-angle damage. BalloonHover clamps each
/// angle from below and only sideways from above.
pub(crate) fn advance_crash_spin(state: &mut RockingState, balloon_hover: bool) {
    state.angle_forwards = store(X::add(load(state.vel_forwards), load(state.angle_forwards)));
    state.angle_sideways = store(X::add(load(state.vel_sideways), load(state.angle_sideways)));
    if balloon_hover {
        let negative = X::neg(load(PI4));
        if lt(load(state.angle_forwards), negative) {
            state.angle_forwards = NativeF32Bits::from_bits(PI4.bits() ^ 0x8000_0000);
        }
        if lt(load(state.angle_sideways), negative) {
            state.angle_sideways = NativeF32Bits::from_bits(PI4.bits() ^ 0x8000_0000);
        } else if !lt(load(state.angle_sideways), load(PI4)) {
            state.angle_sideways = PI4;
        }
    }
}

/// The host runs this once at Techno6FA236, before the Foot parasite tail.
/// A true result requests the existing actual C4 receiver after this borrow.
pub(crate) fn update(
    entity: &mut GameEntity,
    object_type: &ObjectType,
    fallback: SimFixed,
    frame: u32,
) -> bool {
    // Native inline fields start at zero even before the first impulse.
    //70B5AE's sinking branch writes forwards from that zero state; sparse
    // Rust storage must materialize it on this first native producer.
    if entity.sinking.is_active() {
        entity.rocking.get_or_insert_with(RockingState::default);
    }
    let inputs = RockingUpdateInputs {
        direct_rocker_linked: false,
        is_foot: matches!(
            entity.category(),
            EntityCategory::Unit | EntityCategory::Infantry | EntityCategory::Aircraft
        ),
        crushing_building: false,
        sinking: entity.sinking.is_active(),
        crashing: entity.crash.is_crashing(),
        balloon_hover: object_type.balloon_hover,
        body_facing: entity.body_facing_current(frame),
        fallback: NativeF32Bits::from_bits(fallback.to_num::<f32>().to_bits()),
    };
    entity
        .rocking
        .as_mut()
        .is_some_and(|state| advance_state(state, inputs))
}
