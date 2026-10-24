//! `HoverLocomotionClass` (CLSID `{4A582742-9839-11d1-B709-00A024DDAFD1}`):
//! its object state and the double arithmetic of `SpeedUpdate` (0x00515ED0)
//! and the altitude controller (0x00513D20). The retail users are LCRF, ROBO,
//! SAPC and YHVR.
//!
//! The ILocomotion interface sits at object +4 (vtable 0x007EACFC) and the
//! linked Foot at +0xC. The class methods that move the Foot — Process,
//! Move_To, Stop_Moving and their callees — are the child module `process`
//! (`hover_process.rs`), which is the only writer of this state besides the
//! constructor and link here.
//!
//! The four doubles keep their native bits and are computed with the
//! process's 53-bit chop x87 arithmetic ([`X87Chop53`]), so the ramp and the
//! altitude spring reproduce the executable's values, not an approximation.

use super::facing_class::FacingClass;
use crate::rules::ruleset::RuleSet;
use crate::sim::cell_kernel::native_xyz_distance;
use crate::sim::components::DriveCoord;
use crate::util::direction_tables::facing16_from_delta;
use crate::util::native_x87::{
    NativeF64Bits, X87Chop53, X87Ordering, X87Value, distance_3d_leptons,
};

#[path = "hover_process.rs"]
mod process;

pub(crate) use process::{hover_is_powered, hover_move_to, hover_stop_moving};

/// `HoverLocomotionClass::Draw_Matrix @ 0x00513F40` (vt+0x24) as the slope
/// [`ground_locomotor_matrix`] builds it at, with the same angles. Either
/// rocking angle at the gate (`0x00513F53..0x00513F7F`) takes the rocking arm,
/// keyed -1 (`0x005140A2`): `(lift * F) * frame` (`0x00514262`,
/// `0x00514270`), F being `LocomotionClass::Draw_Matrix @ 0x0055A730`, which
/// is the owner's slope-0 tilt, since its extra identity product is exact.
/// Otherwise Is_Powered (`0x00513F88`) returns F itself
/// (`0x0051407F..0x00514092`), the owner's untilted slope 0. Only an
/// unpowered Hover takes the slope of the cell under its Location
/// (`0x00513F93..0x0051405C`: Map `0x00565730`, CellClass `+0x11C`), read
/// through the caller's query. The voxel draw, GetFLH (`0x006F3B9B`) and the
/// turret pivot (`0x006F3DB7`) all ask it.
///
/// [`ground_locomotor_matrix`]: crate::map::voxel_pose::ground_locomotor_matrix
pub fn hover_draw_slope(
    entity: &crate::sim::game_entity::GameEntity,
    cells: Option<&crate::map::resolved_terrain::NativeCellQuery<'_>>,
    angles: [f32; 2],
) -> u8 {
    if crate::map::voxel_pose::voxel_tilt_applies(angles) || hover_is_powered(entity, cells) {
        return 0;
    }
    let [x, y] = super::ground_pose::position_world_xy(&entity.position);
    cells.map_or(0, |cells| cells.ground_fields(cells.lookup_world(x, y)).1)
}

/// Double 0x007E27F8, the minute-to-frame factor of the Hover time keys.
const FRAMES_PER_MINUTE: NativeF64Bits = NativeF64Bits::from_bits(0x408c_2000_0000_0000);
/// Double 0x007E9258, the bob period factor of an even-ID Foot.
const EVEN_ID_BOB_SCALE: NativeF64Bits = NativeF64Bits::from_bits(0x3ff1_9999_9999_999a);
/// Double 0x007E3CC0, 2π.
const TWO_PI: NativeF64Bits = NativeF64Bits::from_bits(0x4019_21fb_5444_2d18);

/// The HoverLocomotionClass object after its constructor (0x00513C20) and
/// `Link_To_Object` (0x00513CB0). Field comments give the object offset.
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct HoverRuntime {
    /// +0x18, written by Move_To and cleared by Stop_Moving.
    destination: Option<DriveCoord>,
    /// +0x24, the accepted next-cell head (ProcessMovement 0x00514F70).
    head: Option<DriveCoord>,
    /// +0x30, the steering facing the XY step follows. It is not the Foot's
    /// PrimaryFacing (+0x388), which SpeedUpdate points one cell ahead.
    facing: FacingClass,
    /// +0x48: 0, 0.5 or 1.0.
    speed_request: NativeF64Bits,
    /// +0x50: the ramped fraction of the Foot's current speed.
    speed_current: NativeF64Bits,
    /// +0x58: 1.0 or HoverBoost.
    speed_mult: NativeF64Bits,
    /// +0x60: the altitude spring's offset.
    bob_offset: NativeF64Bits,
    /// +0x68, +0x6C: Shove (0x00516FC0) turns the Foot's PrimaryFacing by a
    /// random count of steps that Process pays back.
    shove_turning: bool,
    shove_steps: i32,
    /// +0x70: set by Push (0x00516E10); SpeedUpdate then follows at full
    /// speed without the turn test.
    pushed: bool,
}

impl Default for HoverRuntime {
    /// Constructor 0x00513C20: null coordinates, a zero facing, all doubles
    /// zero except SpeedMult 1.0.
    fn default() -> Self {
        Self {
            destination: None,
            head: None,
            facing: FacingClass::new(0, 0),
            speed_request: NativeF64Bits::POSITIVE_ZERO,
            speed_current: NativeF64Bits::POSITIVE_ZERO,
            speed_mult: NativeF64Bits::ONE,
            bob_offset: NativeF64Bits::POSITIVE_ZERO,
            shove_turning: false,
            shove_steps: 0,
            pushed: false,
        }
    }
}

impl HoverRuntime {
    /// `Link_To_Object` 0x00513CD2..0x00513CEF rebuilds the steering facing
    /// at twice the type's `ROT=` (TechnoType+0x71C), heading 0.
    pub(crate) fn link(&mut self, rot: i32) {
        self.facing = FacingClass::new(0, rot.wrapping_mul(2));
    }

    pub(crate) fn head(&self) -> Option<DriveCoord> {
        self.head
    }

    /// Test fixtures only: ProcessMovement (`hover_process`) is the writer.
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn set_head(&mut self, head: Option<DriveCoord>) {
        self.head = head;
    }

    /// `Is_Moving` 0x00514C30: a destination or a head.
    pub(crate) fn is_moving(&self) -> bool {
        self.destination.is_some() || self.head.is_some()
    }

    /// `Is_Moving_Now` 0x00514C80's inputs: `Is_Moving` (0x00514C30) and a
    /// nonzero +0x48 request. The test is `!= 0`, so a negative request
    /// counts as moving. Hover's own `Move_To` asks the slot too
    /// (`0x00514E66`).
    pub(crate) fn ready_state(&self) -> super::locomotor_ready::LocomotorReadyState {
        super::locomotor_ready::LocomotorReadyState::Hover {
            slot_moving: self.is_moving(),
            speed_bits: self.speed_request.bits(),
        }
    }

    /// A Foot with `destination` and `request`, as Move_To leaves it.
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn moving_for_test(destination: Option<DriveCoord>, request: NativeF64Bits) -> Self {
        Self {
            destination,
            speed_request: request,
            ..Self::default()
        }
    }

    /// `Stop_Moving` 0x00516320 on the locomotor: with a destination other
    /// than the head, the destination is cleared and the caller clears the
    /// Foot's path head (+0x5E0 = -1). Returns whether it did.
    fn stop_moving(&mut self) -> bool {
        if self.destination == self.head {
            return false;
        }
        self.destination = None;
        true
    }

    /// The paired zero writes of +0x48 and +0x50 before a `Set_Speed(0)`.
    fn zero_speeds(&mut self) {
        self.speed_request = NativeF64Bits::POSITIVE_ZERO;
        self.speed_current = NativeF64Bits::POSITIVE_ZERO;
    }

    /// `ftol(speed * SpeedCurrent)` (0x00514372..0x00514383): the leptons the
    /// Foot moves this frame, from its current speed (vt+0x538).
    fn step_leptons(&self, foot_speed: i32) -> i32 {
        X87Chop53::ftol_i32_low_masked(X87Chop53::mul(
            X87Chop53::load_i32(foot_speed),
            load(self.speed_current),
        ))
    }

    /// SpeedUpdate 0x00515F2F..0x0051610B for a Foot at `foot` with a head:
    /// the steering facing turns toward the head (Set 0x004C9220; a pushed
    /// Foot snaps, UpdateFacing 0x004C9300). A pushed Foot, or an `airborne`
    /// one (Is_Powered 0x00516C70) within 0x2000 of the head, asks for half
    /// speed within 0x100 leptons of its destination (of its head when the
    /// destination is null: Distance3D 0x0041C380, else Sqrt_Approx) and
    /// full speed beyond, where a pushed Foot also runs at full at once.
    /// Otherwise it asks for none.
    fn choose_request(&mut self, foot: DriveCoord, head: DriveCoord, airborne: bool, frame: u32) {
        let desired = facing16_from_delta(head.x.wrapping_sub(foot.x), head.y.wrapping_sub(foot.y));
        if self.pushed {
            self.facing.snap(desired, frame);
        } else {
            self.facing.set(desired, frame);
        }
        let turn = (self.facing.current(frame).wrapping_sub(desired) as i16).unsigned_abs();
        if !(self.pushed || (airborne && turn <= 0x2000)) {
            self.speed_request = NativeF64Bits::POSITIVE_ZERO;
            return;
        }
        let near = match self.destination {
            None => distance_3d_leptons([foot.x, foot.y, foot.z], [head.x, head.y, head.z]) < 0x100,
            Some(destination) => {
                native_xyz_distance(
                    foot.x.wrapping_sub(destination.x),
                    foot.y.wrapping_sub(destination.y),
                    foot.z.wrapping_sub(destination.z),
                ) < 0x100
            }
        };
        if near {
            self.speed_request = NativeF64Bits::HALF;
        } else {
            self.speed_request = NativeF64Bits::ONE;
            if self.pushed {
                self.speed_current = NativeF64Bits::ONE;
            }
        }
    }

    /// SpeedUpdate 0x0051610B..0x005161E4: with a positive request, SpeedMult
    /// becomes HoverBoost when `boost` (two equal queued path words) and 1.0
    /// otherwise; the target `min(mult * request, 1)` is approached by
    /// `1 / (HoverAcceleration * 900)` up to the target, or left by
    /// `1 / (HoverBrake * 900)` down to zero — the brake is not clamped at
    /// the target.
    fn ramp_speed(&mut self, boost: bool, rules: &RuleSet) {
        let zero = X87Chop53::load_i32(0);
        let one = load(NativeF64Bits::ONE);
        if X87Chop53::compare(load(self.speed_request), zero) == X87Ordering::Greater {
            self.speed_mult = if boost {
                rules.general.hover_boost
            } else {
                NativeF64Bits::ONE
            };
        }
        let mut target = X87Chop53::mul(load(self.speed_mult), load(self.speed_request));
        if X87Chop53::compare(one, target) == X87Ordering::Less {
            target = one;
        }
        if X87Chop53::compare(target, load(self.speed_current)) == X87Ordering::Greater {
            let raised = X87Chop53::add(
                reciprocal_minutes(rules.general.hover_acceleration),
                load(self.speed_current),
            );
            self.speed_current =
                store(if X87Chop53::compare(raised, target) == X87Ordering::Less {
                    raised
                } else {
                    target
                });
        }
        if X87Chop53::compare(target, load(self.speed_current)) == X87Ordering::Less {
            let lowered = X87Chop53::sub(
                load(self.speed_current),
                reciprocal_minutes(rules.general.hover_brake),
            );
            self.speed_current = store(
                if X87Chop53::compare(lowered, zero) == X87Ordering::Greater {
                    lowered
                } else {
                    zero
                },
            );
        }
    }

    /// The altitude controller 0x00513D20 over the Foot's `height` above its
    /// surface (GetHeight 0x005F5F40). Returns the height it sets (SetHeight
    /// 0x005F5FA0) and updates the spring offset.
    /// - `climbing`: the Foot has a path head whose cell's ground is higher
    ///   than its own (0x00513D45..0x00513DDC); the lift then measures from
    ///   HoverHeight below.
    /// - `id`: the Foot's AbstractClass ID (Fetch_ID), which phases and
    ///   scales the bob period (0x00513DF6..0x00513E20).
    /// - `powered`: the base `Is_Powered` byte (0x0055A930), not Hover's
    ///   airborne override.
    fn altitude_step(
        &mut self,
        height: i32,
        climbing: bool,
        id: i32,
        frame: i32,
        powered: bool,
        rules: &RuleSet,
    ) -> i32 {
        let general = &rules.general;
        let hover_height = general.hover_height;
        let lift_height = if climbing {
            height.wrapping_sub(hover_height)
        } else {
            height
        };
        let resting = X87Chop53::ftol_i32_low_masked(X87Chop53::add(
            X87Chop53::load_i32(height),
            load(self.bob_offset),
        ));
        let scale = load(if id & 1 != 0 {
            NativeF64Bits::ONE
        } else {
            EVEN_ID_BOB_SCALE
        });
        let period = X87Chop53::mul(
            X87Chop53::mul(scale, load(general.hover_bob)),
            load(FRAMES_PER_MINUTE),
        );
        let counter = frame.wrapping_add(id.wrapping_mul(2));
        let phase = X87Chop53::div(
            X87Chop53::mul(
                X87Chop53::load_i32(counter.wrapping_rem(X87Chop53::ftol_i32_low_masked(period))),
                load(TWO_PI),
            ),
            period,
        )
        .expect("HoverBob gives a nonzero bob period");
        let sine = crate::util::retail_trig::TrigTable::embedded().sin_from_table(phase);
        let mut visible = X87Chop53::ftol_i32_low_masked(X87Chop53::add(
            X87Chop53::add(sine, sine),
            X87Chop53::load_i32(resting),
        ));
        if visible < 0 {
            self.bob_offset = NativeF64Bits::POSITIVE_ZERO;
            visible = 0;
        }
        let gravity = X87Chop53::load_i32(general.gravity);
        if lift_height < hover_height {
            if powered {
                let cruise = X87Chop53::load_i32(hover_height);
                let deficit = X87Chop53::sub(
                    X87Chop53::add(cruise, cruise),
                    X87Chop53::load_i32(lift_height),
                );
                let lift = X87Chop53::mul(
                    X87Chop53::div(deficit, cruise).expect("HoverHeight is nonzero"),
                    gravity,
                );
                self.bob_offset = store(X87Chop53::add(lift, load(self.bob_offset)));
            }
            if lift_height < hover_height / 4 {
                self.bob_offset = store(X87Chop53::add(
                    X87Chop53::load_i32(general.gravity / 3),
                    load(self.bob_offset),
                ));
            }
        }
        let settled = X87Chop53::sub(load(self.bob_offset), gravity);
        self.bob_offset = store(X87Chop53::mul(settled, load(general.hover_dampen)));
        visible
    }
}

/// `1 / (minutes * 900)`, FLD/FMUL/FDIVR at 0x0051617F and 0x005161B7.
fn reciprocal_minutes(minutes: NativeF64Bits) -> X87Value {
    X87Chop53::div(
        load(NativeF64Bits::ONE),
        X87Chop53::mul(load(minutes), load(FRAMES_PER_MINUTE)),
    )
    .expect("the Hover time keys are nonzero")
}

fn load(bits: NativeF64Bits) -> X87Value {
    X87Chop53::load_f64(bits).expect("Hover doubles stay finite")
}

fn store(value: X87Value) -> NativeF64Bits {
    X87Chop53::store_f64(value).expect("Hover doubles stay finite")
}

#[cfg(test)]
#[path = "hover_tests.rs"]
mod tests;
