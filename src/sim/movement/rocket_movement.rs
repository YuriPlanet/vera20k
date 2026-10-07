//! `RocketLocomotionClass`, the flight of the V3, Dreadnought and Boomer
//! missiles: constructor `0x00661EC0`, ILocomotion vtable `0x007F0B1C`.
//!
//! [`RocketRuntime`] is the class's private object, carried by the owner's
//! locomotor ([`LocomotorRuntimePayload::Rocket`]). [`move_to`] and
//! [`process`] are the two entries that change it; every owner, map and
//! subsystem effect goes through [`RocketHost`] at its native call site, so
//! the production host and the native oracle fixture run the same kernel.
//!
//! Selection: Move_To, Process and Detonate read the `RulesClass` block of
//! the owner's type ([`MissileSpawnRules::rocket_block`]): V3 for the V3
//! type, CMisl for the CMisl type, DMisl for any other.
//!
//! The states (`+0x40`, `Process @ 0x006622C0`, jump table `0x00663010`):
//! 0 idle until Move_To; 1 pause (`0x00662326`); 2 tilt (`0x00662720`);
//! 3 climb (`0x006628F0`); 4 cruise (`0x006629C7`); 5 dive (`0x00662CAC`);
//! 6 the Boomer's vertical raise (`0x006624C8`). Every arm then reaches the
//! shared tail (`0x00662D74`) unless a detonation returned first.
//!
//! Arithmetic runs under the process x87 control word `0x0E7F` (53-bit,
//! toward zero) as [`X87Chop53`]; the trig and arctangent are the retail
//! tables. Native comparisons: `tools/rocket_oracle/` (see its README).
//!
//! [`LocomotorRuntimePayload::Rocket`]: super::locomotion::piggyback::LocomotorRuntimePayload
//! [`MissileSpawnRules::rocket_block`]: crate::rules::missile_spawn::MissileSpawnRules::rocket_block

use crate::map::retail_trig::TrigTable;
use crate::rules::effect_asset_catalog::{ROCKET_TAKEOFF_ANIM, ROCKET_TRAIL_ANIM};
use crate::rules::missile_spawn::MissileSpawnParams;
use crate::sim::components::DriveCoord;
use crate::sim::timer::CdTimer;
use crate::util::direction_tables::{facing16_between, native_atan_from_table};
use crate::util::lepton::{BRIDGE_DECK_HEIGHT_LEPTONS, lepton_to_cell_packed};
use crate::util::native_trig::facing_step_world_xy;
use crate::util::native_x87::{
    NativeF32Bits, NativeF64Bits, X87Chop53, X87Ordering, X87Value, sqrt_approx_f32,
};

/// `+0x40` before Move_To: Process runs only the tail, which a zero speed
/// leaves idle.
const STATE_IDLE: i32 = 0;
const STATE_PAUSE: i32 = 1;
const STATE_TILT: i32 = 2;
const STATE_CLIMB: i32 = 3;
const STATE_CRUISE: i32 = 4;
const STATE_DIVE: i32 = 5;
const STATE_RAISE: i32 = 6;

/// `PUSH 0x600`: the AnimClass draw flags of every rocket puff, Process's and
/// the spawn manager's Boomer launch alike.
pub(crate) const PUFF_DRAW_FLAGS: u32 = 0x600;

/// `0x007E2820`, binary64 pi/2: the quarter-turn scale of the pitch keys.
const HALF_PI: NativeF64Bits = NativeF64Bits::from_bits(0x3FF9_21FB_5444_2D18);
/// `0x007F0C10`, binary64 -pi/2: the dive angle with nothing left to cover.
const NEG_HALF_PI: NativeF64Bits = NativeF64Bits::from_bits(0xBFF9_21FB_5444_2D18);

/// `[0x00B04E18]`, the cell the AircraftTracker's retained cell is compared
/// with before an Add: only the static initializer `0x00661E50` writes it,
/// with zero.
const UNTRACKED_CELL: (i16, i16) = (0, 0);

/// One rocket's `RocketLocomotionClass` fields.
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct RocketRuntime {
    /// `+0x18` Destination; the empty coordinate `[0x00B04E38]` (zero) until
    /// Move_To. Is_Moving (`0x00661F50`) is "not empty".
    destination: DriveCoord,
    /// `+0x24` MissionTimer: the pause, tilt or raise countdown.
    mission_timer: CdTimer,
    /// `+0x30` the length MissionTimer was last started with, the divisor of
    /// its "done" test and of the tilt's progress.
    mission_timer_rate: i32,
    /// `+0x34` TrailerTimer, gating the V3TAKOFF and V3TRAIL puffs.
    #[serde(deserialize_with = "deserialize_trailer_timer_after_load")]
    trailer_timer: CdTimer,
    /// `+0x40` MissionState.
    mission_state: i32,
    /// `+0x48` CurrentSpeed, binary64 bits, leptons per frame.
    current_speed: u64,
    /// `+0x50`: clear while the CMisl pause has resubmitted the owner to the
    /// display; the climb resubmits and sets it again. The constructor sets it.
    resubmit_latch: bool,
    /// `+0x51` SpawnerIsElite, refreshed by states 1, 2 and 6.
    spawner_is_elite: bool,
    /// `+0x54` CurrentPitch, binary32 bits, radians above the horizon.
    current_pitch: u32,
    /// `+0x58` CruiseStartDistance, the horizontal distance left when the
    /// climb ended: the LazyCurve blend's denominator.
    cruise_start_distance: i32,
}

/// `RocketLocomotionClass::Load @ 0x00663410` restarts TrailerTimer at the
/// load frame with no time left after the saved bytes are read. A saved start
/// is never after the load frame, so keeping it with a zero length reads the
/// same zero remainder from then on.
fn deserialize_trailer_timer_after_load<'de, D>(deserializer: D) -> Result<CdTimer, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let saved = <CdTimer as serde::Deserialize>::deserialize(deserializer)?;
    Ok(CdTimer::from_raw(saved.start_frame(), 0))
}

impl RocketRuntime {
    /// The constructor (`0x00661EC0`): both timers start now with no length,
    /// the latch is set and every other field is zero.
    pub(crate) fn constructed(binary_frame: u32) -> Self {
        let now = binary_frame as i32;
        Self {
            destination: DriveCoord { x: 0, y: 0, z: 0 },
            mission_timer: CdTimer::started(now, 0),
            mission_timer_rate: 0,
            trailer_timer: CdTimer::started(now, 0),
            mission_state: STATE_IDLE,
            current_speed: NativeF64Bits::POSITIVE_ZERO.bits(),
            resubmit_latch: true,
            spawner_is_elite: false,
            current_pitch: NativeF32Bits::POSITIVE_ZERO.bits(),
            cruise_start_distance: 0,
        }
    }

    /// `Is_Moving @ 0x00661F50`.
    pub(crate) fn is_moving(&self) -> bool {
        self.destination != (DriveCoord { x: 0, y: 0, z: 0 })
    }

    /// `Is_Moving_Now @ 0x00661F90`: climbing, cruising or diving.
    pub(crate) fn is_moving_now(&self) -> bool {
        (STATE_CLIMB..=STATE_DIVE).contains(&self.mission_state)
    }

    /// The MissionTimer "done" test every arm makes: `(rate - remaining) /
    /// rate == 1.0`, skipped as done when the rate is zero. Two int32s divide
    /// to exactly one only when equal, so it is "no time left".
    fn mission_timer_done(&self, frame: i32) -> bool {
        self.mission_timer_rate == 0 || self.mission_timer.remaining(frame) == 0
    }

    fn start_mission_timer(&mut self, frame: i32, frames: i32) {
        self.mission_timer = CdTimer::started(frame, frames);
        self.mission_timer_rate = frames;
    }

    fn pitch_value(&self) -> X87Value {
        f32_value(NativeF32Bits::from_bits(self.current_pitch))
    }

    #[cfg(test)]
    pub(crate) fn set_mission_state_for_test(&mut self, state: i32) {
        self.mission_state = state;
    }

    #[cfg(test)]
    pub(crate) fn mission_state_for_test(&self) -> i32 {
        self.mission_state
    }

    #[cfg(test)]
    pub(crate) fn destination_for_test(&self) -> DriveCoord {
        self.destination
    }

    #[cfg(test)]
    pub(crate) fn current_speed_for_test(&self) -> f64 {
        f64::from_bits(self.current_speed)
    }
}

/// What Process reads from and writes to its owner, the map and the shared
/// subsystems. Coordinates are world leptons.
pub(crate) trait RocketHost {
    /// `[0x00A8ED84]`, the frame the timers count from.
    fn binary_frame(&self) -> u32;
    fn trig(&self) -> &TrigTable;
    /// Owner `+0x2D4` SpawnOwner is set and `VeterancyStruct::IsElite @
    /// 0x00750010` holds for its `+0x150`.
    fn spawn_owner_is_elite(&self) -> bool;
    /// Owner `+0x9C` Location, which its GetCoords (vtable `+0x48` ->
    /// `0x005F65A0`) also answers.
    fn location(&self) -> [i32; 3];
    /// `FootClass::SetLocation` (vtable `+0x1B4` -> `0x004DB810`).
    fn set_location(&mut self, coord: [i32; 3]);
    /// `ObjectClass::GetHeight @ 0x005F5F40` (vtable `+0x1C8`).
    fn height(&self) -> i32;
    /// Owner type `+0x678`, the `Speed=` leptons per frame.
    fn type_speed(&self) -> i32;
    /// `MapClass::In_Bounds @ 0x00568300`.
    fn in_bounds(&self, cell: (i16, i16)) -> bool;
    /// Owner Mark (vtable `+0x124`): `false` lifts it (MARK_UP), `true` puts
    /// it down (MARK_DOWN).
    fn mark(&mut self, put: bool);
    /// `DisplayClass::Submit @ 0x004A9720` of the owner.
    fn submit_display(&mut self);
    /// Owner `+0x560`, the cell the AircraftTracker retained.
    fn tracker_cell(&self) -> (i16, i16);
    /// `AircraftTrackerClass::Add @ 0x004134A0`.
    fn tracker_add(&mut self);
    /// `AircraftTrackerClass::Update @ 0x004138C0` to `cell`.
    fn tracker_update(&mut self, cell: (i16, i16));
    /// `AircraftTrackerClass::Remove @ 0x004135D0`.
    fn tracker_remove(&mut self);
    /// Owner `+0x90` IsAlive.
    fn is_alive(&self) -> bool;
    /// Owner `+0x6C` Health.
    fn health(&self) -> i32;
    /// `FacingClass::Current @ 0x004C93D0` of the owner's `+0x388`.
    fn facing(&self) -> u16;
    /// `FacingClass::Set @ 0x004C9220` of the owner's `+0x388`.
    fn set_facing(&mut self, facing: u16);
    /// `new AnimClass(type, &coord, delay, 1, 0x600, z_adjust, 0)` for one of
    /// the two hard-coded names.
    fn anim(&mut self, name: &'static str, coord: [i32; 3], delay: i32, z_adjust: i32);
    /// `VocClass::PlayAt @ 0x007509E0` of the owner type's `AuxSound1=`
    /// (`+0x52C`) at `coord`, with no handle.
    fn aux_sound(&mut self, coord: [i32; 3]);
    /// The owner's cell (GetCell, vtable `+0x1BC` -> `0x005F6960`): its own
    /// GetCoords Z (`0x00486840`) when it carries a structural bridge
    /// (`+0x140 & 0x100`), else `None`.
    fn structural_bridge_floor(&self) -> Option<i32>;
    /// Detonate's explosion: `SelectAnim @ 0x0048A4F0(damage, warhead, land,
    /// &coord)` with the land of the cell holding `coord`, then
    /// `new AnimClass(type, &coord, 0, 1, 0x2600, -15, 0)`.
    fn explosion(&mut self, damage: i32, warhead: &str, coord: [i32; 3]);
    /// `0x0048A620(damage, warhead, coord, 0, 0)`, the impact's flash.
    fn combat_light(&mut self, damage: i32, warhead: &str, coord: [i32; 3]);
    /// `Apply_area_damage @ 0x00489280(&coord, damage, owner, warhead, 1,
    /// null house)`.
    fn area_damage(&mut self, coord: [i32; 3], damage: i32, warhead: &str);
    /// Owner UnInit (vtable `+0xF8`).
    fn uninit(&mut self);
}

/// `Move_To @ 0x006632E0`. Only an empty Destination accepts: a rocket in
/// flight keeps its first target. The pause starts when the block has one,
/// otherwise the tilt, and the pitch starts at `PitchInitial` quarter turns.
pub(crate) fn move_to(
    rocket: &mut RocketRuntime,
    block: &MissileSpawnParams,
    to: DriveCoord,
    binary_frame: u32,
) {
    if rocket.is_moving() {
        return;
    }
    let frames = if block.pause_frames != 0 {
        rocket.mission_state = STATE_PAUSE;
        block.pause_frames
    } else {
        rocket.mission_state = STATE_TILT;
        block.tilt_frames
    };
    rocket.start_mission_timer(binary_frame as i32, frames);
    rocket.current_pitch = quarter_turns(block.pitch_initial);
    rocket.destination = to;
}

/// `Process @ 0x006622C0` for one frame. `block` is the owner type's rocket
/// block and `cmisl` whether that block's type is the CMisl type, which the
/// Boomer arms compare (`0x0066235A`, `0x00662493`), not the selection.
/// The native return, Is_Moving, has no reader (`FootClass::AI` tests the
/// owner's IsAlive instead).
pub(crate) fn process(
    rocket: &mut RocketRuntime,
    block: &MissileSpawnParams,
    cmisl: bool,
    host: &mut impl RocketHost,
) {
    let frame = host.binary_frame() as i32;
    match rocket.mission_state {
        STATE_PAUSE => pause(rocket, block, cmisl, frame, host),
        STATE_TILT => tilt(rocket, block, frame, host),
        STATE_CLIMB => climb(rocket, block, host),
        STATE_CRUISE => {
            if !cruise(rocket, block, host) {
                return;
            }
        }
        STATE_DIVE => {
            if predicted_impact(rocket, block, host) {
                return;
            }
            dive(rocket, block, host);
        }
        STATE_RAISE => raise(rocket, block, frame, host),
        _ => {}
    }
    tail(rocket, block, frame, host);
}

/// State 1 (`0x00662326..0x006624C3`).
fn pause(
    rocket: &mut RocketRuntime,
    block: &MissileSpawnParams,
    cmisl: bool,
    frame: i32,
    host: &mut impl RocketHost,
) {
    rocket.current_speed = NativeF64Bits::POSITIVE_ZERO.bits();
    rocket.spawner_is_elite = host.spawn_owner_is_elite();
    if cmisl {
        if rocket.trailer_timer.remaining(frame) == 0 {
            host.anim(ROCKET_TAKEOFF_ANIM, host.location(), 2, -10);
            rocket.trailer_timer = CdTimer::started(frame, 0x18);
        }
        if rocket.resubmit_latch {
            host.mark(false);
            rocket.resubmit_latch = false;
            host.submit_display();
            host.mark(true);
        }
    } else {
        rocket.resubmit_latch = true;
    }
    if rocket.mission_timer_done(frame) {
        rocket.mission_state = if cmisl { STATE_RAISE } else { STATE_TILT };
        rocket.start_mission_timer(frame, block.tilt_frames);
    }
}

/// State 2 (`0x00662720..0x006628EB`): the pitch moves linearly from
/// PitchInitial to PitchFinal over TiltFrames; at the end the rocket climbs,
/// joins the AircraftTracker, puffs V3TAKOFF and plays AuxSound1.
fn tilt(
    rocket: &mut RocketRuntime,
    block: &MissileSpawnParams,
    frame: i32,
    host: &mut impl RocketHost,
) {
    rocket.current_speed = NativeF64Bits::POSITIVE_ZERO.bits();
    rocket.spawner_is_elite = host.spawn_owner_is_elite();
    if rocket.mission_timer_done(frame) {
        rocket.current_pitch = quarter_turns(block.pitch_final);
        rocket.mission_state = STATE_CLIMB;
        if host.tracker_cell() == UNTRACKED_CELL {
            host.tracker_add();
        }
        host.anim(ROCKET_TAKEOFF_ANIM, host.location(), 2, -10);
        host.aux_sound(host.location());
        return;
    }
    let half_pi = f64_value(HALF_PI);
    let initial = X87Chop53::mul(f32_value(block.pitch_initial), half_pi);
    let last = X87Chop53::mul(f32_value(block.pitch_final), half_pi);
    let rate = rocket.mission_timer_rate;
    let progress = X87Chop53::div(
        X87Chop53::load_i32(rate.wrapping_sub(rocket.mission_timer.remaining(frame))),
        X87Chop53::load_i32(rate),
    )
    .expect("a running tilt has a nonzero rate");
    rocket.current_pitch = store_f32(X87Chop53::add(
        X87Chop53::mul(X87Chop53::sub(last, initial), progress),
        initial,
    ));
}

/// State 3 (`0x006628F0..0x006629C2`): accelerate straight along the pitch
/// until `Altitude=` above the ground, then remember the horizontal distance
/// still to go.
fn climb(rocket: &mut RocketRuntime, block: &MissileSpawnParams, host: &mut impl RocketHost) {
    if !rocket.resubmit_latch {
        host.mark(false);
        rocket.resubmit_latch = true;
        host.submit_display();
        host.mark(true);
    }
    accelerate(rocket, block, &*host);
    if host.height() < block.altitude {
        return;
    }
    rocket.mission_state = STATE_CRUISE;
    let here = host.location();
    let dx = X87Chop53::load_i32(here[0].wrapping_sub(rocket.destination.x));
    let dy = X87Chop53::load_i32(here[1].wrapping_sub(rocket.destination.y));
    rocket.cruise_start_distance = sqrt_approx_ftol(X87Chop53::add(
        X87Chop53::mul(dy, dy),
        X87Chop53::mul(dx, dx),
    ));
}

/// State 4 (`0x006629C7..0x00662CA7`). Answers false when the rocket
/// detonated, which returns before the tail.
fn cruise(
    rocket: &mut RocketRuntime,
    block: &MissileSpawnParams,
    host: &mut impl RocketHost,
) -> bool {
    if host.height() <= 0 {
        detonate(rocket, block, host);
        return false;
    }
    accelerate(rocket, block, &*host);
    let destination = rocket.destination;
    if !block.lazy_curve || rocket.cruise_start_distance == 0 {
        // 0x00662B51: level off by TurnRate a frame, not below zero; the dive
        // starts once the distance is no more than the height over the
        // destination. GetCoords is the Location, so the Z term is zero.
        let zero = X87Chop53::load_i32(0);
        let pitch = rocket.pitch_value();
        if X87Chop53::compare(pitch, zero) == X87Ordering::Greater {
            let lowered = X87Chop53::sub(pitch, f32_value(block.turn_rate));
            rocket.current_pitch = store_f32(lowered);
            if X87Chop53::compare(lowered, zero) == X87Ordering::Less {
                rocket.current_pitch = NativeF32Bits::POSITIVE_ZERO.bits();
            }
        }
        let here = host.location();
        let above = here[2].wrapping_sub(destination.z);
        let coords = host.location();
        let dx = X87Chop53::load_i32(coords[0].wrapping_sub(destination.x));
        let dy = X87Chop53::load_i32(coords[1].wrapping_sub(destination.y));
        let dz = X87Chop53::load_i32(coords[2].wrapping_sub(here[2]));
        let squared = X87Chop53::add(
            X87Chop53::add(X87Chop53::mul(dz, dz), X87Chop53::mul(dy, dy)),
            X87Chop53::mul(dx, dx),
        );
        if sqrt_approx_ftol(squared) <= above {
            rocket.mission_state = STATE_DIVE;
        }
    } else {
        // 0x00662A32: blend from the target angle toward PitchFinal as the
        // remaining share of CruiseStartDistance shrinks.
        if predicted_impact(rocket, block, host) {
            return false;
        }
        let coords = host.location();
        let dx = X87Chop53::load_i32(coords[0].wrapping_sub(destination.x));
        let dy = X87Chop53::load_i32(coords[1].wrapping_sub(destination.y));
        let left = sqrt_approx_ftol(X87Chop53::add(
            X87Chop53::mul(dy, dy),
            X87Chop53::mul(dx, dx),
        ));
        let share = X87Chop53::div(
            X87Chop53::load_i32(left),
            X87Chop53::load_i32(rocket.cruise_start_distance),
        )
        .expect("the lazy curve runs with a nonzero start distance");
        let angle = target_angle(destination, host.location());
        let rest = X87Chop53::sub(f64_value(NativeF64Bits::ONE), share);
        let toward_final = X87Chop53::mul(
            X87Chop53::mul(f32_value(block.pitch_final), share),
            f64_value(HALF_PI),
        );
        rocket.current_pitch = store_f32(X87Chop53::add(toward_final, X87Chop53::mul(angle, rest)));
    }
    // 0x00662C3A: steer the body at the destination.
    let coords = host.location();
    let facing = facing16_between([coords[0], coords[1]], [destination.x, destination.y]);
    host.set_facing(facing);
    true
}

/// State 5 (`0x00662CBF..0x0066300A`) after the impact prediction: turn the
/// pitch toward the target angle by at most TurnRate.
fn dive(rocket: &mut RocketRuntime, block: &MissileSpawnParams, host: &mut impl RocketHost) {
    let angle = target_angle(rocket.destination, host.location());
    let zero = X87Chop53::load_i32(0);
    let pitch = rocket.pitch_value();
    let turn = f32_value(block.turn_rate);
    let difference = X87Chop53::sub(angle, pitch);
    let magnitude = if X87Chop53::compare(difference, zero) == X87Ordering::Less {
        X87Chop53::neg(difference)
    } else {
        difference
    };
    rocket.current_pitch = store_f32(
        if X87Chop53::compare(magnitude, turn) != X87Ordering::Greater {
            X87Chop53::add(pitch, difference)
        } else if X87Chop53::compare(difference, zero) == X87Ordering::Less {
            X87Chop53::sub(pitch, turn)
        } else {
            X87Chop53::add(turn, pitch)
        },
    );
}

/// State 6 (`0x006624C8..0x0066271B`): the Boomer's missile rises RaiseRate
/// leptons a frame for TiltFrames, puffing V3TAKOFF, then pitches to
/// PitchFinal and climbs.
fn raise(
    rocket: &mut RocketRuntime,
    block: &MissileSpawnParams,
    frame: i32,
    host: &mut impl RocketHost,
) {
    rocket.spawner_is_elite = host.spawn_owner_is_elite();
    if rocket.trailer_timer.remaining(frame) == 0 {
        host.anim(ROCKET_TAKEOFF_ANIM, host.location(), 2, -10);
        rocket.trailer_timer = CdTimer::started(frame, 0x18);
    }
    let here = host.location();
    if !rocket.mission_timer_done(frame) {
        let raised = [here[0], here[1], here[2].wrapping_add(block.raise_rate)];
        if host.in_bounds(cell_of(raised)) {
            host.set_location(raised);
        }
        return;
    }
    rocket.current_pitch = quarter_turns(block.pitch_final);
    if host.tracker_cell() == UNTRACKED_CELL {
        host.aux_sound(host.location());
        host.tracker_add();
    }
    rocket.trailer_timer = CdTimer::started(frame, 0);
    rocket.mission_state = STATE_CLIMB;
}

/// The shared tail (`0x00662D74..0x00662FE1`): the V3TRAIL puffs while
/// moving, then a frame of flight along the facing and pitch, the tracker's
/// cell, and the explosion of a rocket left without Health.
fn tail(
    rocket: &mut RocketRuntime,
    block: &MissileSpawnParams,
    frame: i32,
    host: &mut impl RocketHost,
) {
    if rocket.is_moving_now() && rocket.trailer_timer.remaining(frame) == 0 {
        host.anim(ROCKET_TRAIL_ANIM, host.location(), 0, 0);
        rocket.trailer_timer = CdTimer::started(frame, 3);
    }
    let speed = f64_value(NativeF64Bits::from_bits(rocket.current_speed));
    if X87Chop53::compare(speed, X87Chop53::load_i32(0)) != X87Ordering::Greater {
        return;
    }
    let speed = X87Chop53::load_i32(X87Chop53::ftol_i32_low_masked(speed));
    let next = along_nose(rocket, &*host, speed);
    if host.in_bounds(cell_of(next)) {
        host.set_location(next);
    }
    let cell = cell_of(host.location());
    if cell != host.tracker_cell() && host.is_alive() {
        host.tracker_update(cell);
    }
    if host.health() <= 0 {
        detonate(rocket, block, host);
    }
}

/// CurrentSpeed gains Acceleration, then is clamped to the type's Speed
/// (`0x0066291F..0x00662954`, again at `0x006629E6..0x00662A1B`).
fn accelerate(rocket: &mut RocketRuntime, block: &MissileSpawnParams, host: &impl RocketHost) {
    let speed = X87Chop53::add(
        f32_value(block.acceleration),
        f64_value(NativeF64Bits::from_bits(rocket.current_speed)),
    );
    rocket.current_speed = store_f64(speed);
    let limit = X87Chop53::load_i32(host.type_speed());
    if X87Chop53::compare(limit, speed) == X87Ordering::Less {
        rocket.current_speed = store_f64(limit);
    }
}

/// The impact predictor (`0x006620F0`): the nose, BodyLength ahead along the
/// pitch, has reached the destination's height, or the deck height over a
/// structural bridge whose cell floor is the destination's, or the rocket is
/// on the ground. It detonates and answers true.
fn predicted_impact(
    rocket: &mut RocketRuntime,
    block: &MissileSpawnParams,
    host: &mut impl RocketHost,
) -> bool {
    let here = host.location();
    let nose = X87Chop53::ftol_i32_low_masked(X87Chop53::add(
        X87Chop53::mul(
            host.trig().sin_from_table(rocket.pitch_value()),
            X87Chop53::load_i32(block.body_length),
        ),
        X87Chop53::load_i32(here[2]),
    ));
    let floor = rocket.destination.z;
    let impact = nose <= floor
        || host.structural_bridge_floor().is_some_and(|cell_floor| {
            cell_floor == floor && nose <= floor.wrapping_add(BRIDGE_DECK_HEIGHT_LEPTONS)
        })
        || host.height() <= 0;
    if impact {
        detonate(rocket, block, host);
    }
    impact
}

/// `Detonate @ 0x00663030`: leave the AircraftTracker, explode BodyLength
/// ahead along the nose with the elite or normal damage and warhead, and
/// UnInit the owner.
fn detonate(rocket: &RocketRuntime, block: &MissileSpawnParams, host: &mut impl RocketHost) {
    host.tracker_remove();
    let impact = along_nose(rocket, &*host, X87Chop53::load_i32(block.body_length));
    let elite = rocket.spawner_is_elite;
    let damage = block.damage_for(elite);
    let warhead = block.warhead_for(elite);
    host.explosion(damage, warhead, impact);
    host.combat_light(damage, warhead, impact);
    host.area_damage(impact, damage, warhead);
    host.uninit();
}

/// The point `length` ahead of the owner along its facing and pitch: the
/// horizontal share `ftol(cos(pitch) * length)` steps along the facing and
/// Z gains `sin(pitch) * length` (the tail `0x00662E2A..0x00662F27`, the
/// impact `0x006630F7..0x006631C7`).
fn along_nose(rocket: &RocketRuntime, host: &impl RocketHost, length: X87Value) -> [i32; 3] {
    let here = host.location();
    let pitch = rocket.pitch_value();
    let horizontal =
        X87Chop53::ftol_i32_low_masked(X87Chop53::mul(host.trig().cos_from_table(pitch), length));
    let [x, y] = facing_step_world_xy([here[0], here[1]], host.facing(), horizontal);
    let z = X87Chop53::ftol_i32_low_masked(X87Chop53::add(
        X87Chop53::mul(host.trig().sin_from_table(pitch), length),
        X87Chop53::load_i32(here[2]),
    ));
    [x, y, z]
}

/// The angle down to the destination, inlined in the cruise (`0x00662AA2..
/// 0x00662B2E`) and the dive (`0x00662CBF..0x00662D2E`): the horizontal
/// distance squares and sums in int32, and nothing left to cover is straight
/// down.
fn target_angle(destination: DriveCoord, here: [i32; 3]) -> X87Value {
    let dx = destination.x.wrapping_sub(here[0]);
    let dy = destination.y.wrapping_sub(here[1]);
    let dz = destination.z.wrapping_sub(here[2]);
    let squared = dx.wrapping_mul(dx).wrapping_add(dy.wrapping_mul(dy));
    let distance = f32_value(
        sqrt_approx_f32(X87Chop53::load_i32(squared)).expect("a finite int32 has a finite root"),
    );
    if X87Chop53::compare(distance, X87Chop53::load_i32(0)) != X87Ordering::Greater {
        return f64_value(NEG_HALF_PI);
    }
    native_atan_from_table(
        X87Chop53::div(X87Chop53::load_i32(dz), distance).expect("the distance is positive"),
    )
}

/// `Sqrt_Approx @ 0x004CAC40` then `Math::ftol @ 0x007C5F00`.
fn sqrt_approx_ftol(squared: X87Value) -> i32 {
    let root = sqrt_approx_f32(squared).expect("a finite sum of squares has a finite root");
    X87Chop53::ftol_i32_low_masked(f32_value(root))
}

/// A pitch key in quarter turns as radians, stored as binary32
/// (`FLD float; FMUL [0x007E2820]; FSTP float`).
fn quarter_turns(value: NativeF32Bits) -> u32 {
    store_f32(X87Chop53::mul(f32_value(value), f64_value(HALF_PI)))
}

/// The cell a coordinate falls in, as `0x0041BEA0` and the In_Bounds
/// callers truncate it.
fn cell_of(coord: [i32; 3]) -> (i16, i16) {
    (
        lepton_to_cell_packed(coord[0]),
        lepton_to_cell_packed(coord[1]),
    )
}

fn f32_value(bits: NativeF32Bits) -> X87Value {
    X87Chop53::load_f32(bits).expect("rocket binary32 values are finite")
}

fn f64_value(bits: NativeF64Bits) -> X87Value {
    X87Chop53::load_f64(bits).expect("rocket binary64 values are finite")
}

fn store_f32(value: X87Value) -> u32 {
    X87Chop53::store_f32_masked_chop(value).bits()
}

/// CurrentSpeed never exceeds binary64: each frame clamps it to an int32.
fn store_f64(value: X87Value) -> u64 {
    X87Chop53::store_f64(value)
        .expect("rocket speed stays finite")
        .bits()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::map::retail_trig::required_math_tables;
    use crate::rules::missile_spawn::MissileSpawnRules;
    use crate::sim::movement::facing_class::FacingClass;
    use serde_json::{Value, json};

    /// The oracle's seams (`tools/rocket_oracle/flight.py`), recording each
    /// call as the native harness records it.
    struct FixtureHost<'a> {
        input: &'a Value,
        frame: u32,
        trig: &'a TrigTable,
        location: [i32; 3],
        facing: FacingClass,
        health: i32,
        tracker_cell: (i16, i16),
        events: Vec<Value>,
        uninit: bool,
    }

    fn int(value: &Value) -> i64 {
        value.as_i64().expect("integer field")
    }

    fn coord(value: &Value) -> [i32; 3] {
        [0, 1, 2].map(|axis| int(&value[axis]) as i32)
    }

    impl RocketHost for FixtureHost<'_> {
        fn binary_frame(&self) -> u32 {
            self.frame
        }
        fn trig(&self) -> &TrigTable {
            self.trig
        }
        fn spawn_owner_is_elite(&self) -> bool {
            // VeterancyStruct::IsElite: Veterancy >= [0x007E37B4] (2.0f).
            self.input["spawner_veterancy"]
                .as_f64()
                .is_some_and(|veterancy| veterancy as f32 >= 2.0)
        }
        fn location(&self) -> [i32; 3] {
            self.location
        }
        fn set_location(&mut self, coord: [i32; 3]) {
            self.location = coord;
            self.events.push(json!(["set_location", coord]));
        }
        fn height(&self) -> i32 {
            let hill = &self.input["hill"];
            let floor = if !hill.is_null() && i64::from(self.location[0]) >= int(&hill["from_x"]) {
                int(&hill["floor"])
            } else {
                int(&self.input["floor"])
            };
            self.location[2] - floor as i32
        }
        fn type_speed(&self) -> i32 {
            int(&self.input["type_speed"]) as i32
        }
        fn in_bounds(&self, cell: (i16, i16)) -> bool {
            let size = &self.input["map_size"];
            crate::map::playfield::size_diamond_contains(
                int(&size[0]) as i32,
                int(&size[1]) as i32,
                cell,
            )
        }
        fn mark(&mut self, put: bool) {
            self.events.push(json!(["mark", u8::from(put)]));
        }
        fn submit_display(&mut self) {
            self.events.push(json!(["submit"]));
        }
        fn tracker_cell(&self) -> (i16, i16) {
            self.tracker_cell
        }
        fn tracker_add(&mut self) {
            self.tracker_cell = cell_of(self.location);
            let (x, y) = self.tracker_cell;
            self.events.push(json!(["tracker_add", [x, y]]));
        }
        fn tracker_update(&mut self, cell: (i16, i16)) {
            self.tracker_cell = cell;
            self.events
                .push(json!(["tracker_update", [cell.0, cell.1]]));
        }
        fn tracker_remove(&mut self) {
            self.events.push(json!(["tracker_remove"]));
        }
        fn is_alive(&self) -> bool {
            self.input["alive"].as_bool().expect("alive flag")
        }
        fn health(&self) -> i32 {
            self.health
        }
        fn facing(&self) -> u16 {
            self.facing.current(self.frame)
        }
        fn set_facing(&mut self, facing: u16) {
            self.facing.set(facing, self.frame);
        }
        fn anim(&mut self, name: &'static str, coord: [i32; 3], delay: i32, z_adjust: i32) {
            self.events
                .push(json!(["anim", name, coord, delay, 1, 0x600, z_adjust, 0]));
        }
        fn aux_sound(&mut self, coord: [i32; 3]) {
            self.events
                .push(json!(["aux_sound", self.input["aux_sound"], coord]));
        }
        fn structural_bridge_floor(&self) -> Option<i32> {
            let (x, y) = cell_of(self.location);
            self.input["bridge_cells"]
                .as_array()
                .expect("bridge cells")
                .contains(&json!([x, y]))
                .then(|| int(&self.input["bridge_floor"]) as i32)
        }
        fn explosion(&mut self, damage: i32, warhead: &str, coord: [i32; 3]) {
            let (x, y) = cell_of(coord);
            let land = &self.input["impact_land"];
            self.events.push(json!(["impact_cell", [x, y]]));
            self.events
                .push(json!(["select_anim", damage, warhead, land, coord]));
            self.events.push(json!([
                "anim",
                "explosion",
                coord,
                0,
                1,
                crate::sim::anim_class::COMBAT_EXPLOSION_DRAW_FLAGS,
                crate::sim::anim_class::COMBAT_EXPLOSION_Z_ADJUST,
                0
            ]));
        }
        fn combat_light(&mut self, damage: i32, warhead: &str, coord: [i32; 3]) {
            self.events
                .push(json!(["combat_light", damage, warhead, coord, 0, 0]));
        }
        fn area_damage(&mut self, coord: [i32; 3], damage: i32, warhead: &str) {
            self.events
                .push(json!(["area_damage", coord, damage, warhead, 1, 0]));
        }
        fn uninit(&mut self) {
            self.uninit = true;
            self.events.push(json!(["uninit"]));
        }
    }

    /// The oracle's rows through the production block selection: a block's
    /// type is the owner's, another type of its own, or unset.
    fn rules(input: &Value) -> MissileSpawnRules {
        let mut rules = MissileSpawnRules::default();
        for (key, block, other, warheads) in [
            ("V3", &mut rules.v3, "V3OTHER", ["V3WH", "V3EWH"]),
            (
                "DMisl",
                &mut rules.dmisl,
                "DMISLOTHER",
                ["DMISLWH", "DMISLEWH"],
            ),
            (
                "CMisl",
                &mut rules.cmisl,
                "CMISLOTHER",
                ["CMISLWH", "CMISLEWH"],
            ),
        ] {
            let row = &input["blocks"][key];
            let bits = |field: &str| NativeF32Bits::from_bits(int(&row[field]) as u32);
            block.type_name = match row["type"].as_str().expect("block type") {
                "owner" => "OWNER".to_string(),
                "other" => other.to_string(),
                _ => String::new(),
            };
            block.pause_frames = int(&row["pause_frames"]) as i32;
            block.tilt_frames = int(&row["tilt_frames"]) as i32;
            block.pitch_initial = bits("pitch_initial");
            block.pitch_final = bits("pitch_final");
            block.turn_rate = bits("turn_rate");
            block.raise_rate = int(&row["raise_rate"]) as i32;
            block.acceleration = bits("acceleration");
            block.altitude = int(&row["altitude"]) as i32;
            block.damage = int(&row["damage"]) as i32;
            block.elite_damage = int(&row["elite_damage"]) as i32;
            block.body_length = int(&row["body_length"]) as i32;
            block.lazy_curve = row["lazy_curve"].as_bool().expect("lazy flag");
            block.warhead = warheads[0].to_string();
            block.elite_warhead = warheads[1].to_string();
        }
        rules
    }

    fn observed(rocket: &RocketRuntime, host: &FixtureHost<'_>) -> Value {
        let d = rocket.destination;
        json!({
            "location": host.location,
            "facing": host.facing.current(host.frame),
            "destination": [d.x, d.y, d.z],
            "mission_timer": [
                rocket.mission_timer.start_frame(),
                rocket.mission_timer.duration(),
                rocket.mission_timer_rate
            ],
            "trailer_timer": [
                rocket.trailer_timer.start_frame(),
                rocket.trailer_timer.duration()
            ],
            "mission_state": rocket.mission_state,
            "current_speed": rocket.current_speed,
            "resubmit_latch": u8::from(rocket.resubmit_latch),
            "spawner_is_elite": u8::from(rocket.spawner_is_elite),
            "current_pitch": rocket.current_pitch,
            "cruise_start_distance": rocket.cruise_start_distance,
            "tracker_cell": [host.tracker_cell.0, host.tracker_cell.1],
        })
    }

    /// Parity with `tools/rocket_oracle/flight.json`: the original
    /// constructor, Move_To and a Process per frame through Detonate, every
    /// field and seam call bit-exact.
    #[test]
    fn flights_match_the_native_move_to_process_and_detonate_corpus() {
        let (trig, _) = required_math_tables();
        if !trig.matches_retail() {
            assert!(
                std::env::var_os("RA2_DIR").is_none(),
                "RA2_DIR is set but the retail sine table does not match"
            );
            eprintln!("skipped: set RA2_DIR to the retail install to run this");
            return;
        }
        let rows: Value =
            serde_json::from_str(crate::test_fixture::text("tools/rocket_oracle/flight.json"))
                .expect("corpus parses");
        let rows = rows.as_array().expect("row list");
        assert_eq!(rows.len(), 16);
        for row in rows {
            let name = row["name"].as_str().expect("row name");
            let input = &row["input"];
            let rules = rules(input);
            let (block, cmisl) = rules.rocket_block("OWNER");
            let first_frame = int(&input["first_frame"]) as u32;
            let mut facing = FacingClass::new(0, int(&input["rot"]) as i32);
            facing.snap(int(&input["facing"]) as u16, first_frame);
            let mut host = FixtureHost {
                input,
                frame: first_frame,
                trig,
                location: coord(&input["start"]),
                facing,
                health: int(&input["health"]) as i32,
                tracker_cell: (0, 0),
                events: Vec::new(),
                uninit: false,
            };
            let mut rocket = RocketRuntime::constructed(first_frame);
            let output = &row["output"];
            let mut start = observed(&rocket, &host);
            start["destination"] = json!([0, 0, 0]);
            assert_eq!(start, output["start"], "{name}: constructed");
            let to = |value: &Value| {
                let [x, y, z] = coord(value);
                DriveCoord { x, y, z }
            };
            move_to(&mut rocket, block, to(&input["destination"]), first_frame);
            let frames = output["frames"].as_array().expect("frames");
            for (index, expected) in frames.iter().enumerate() {
                host.frame += 1;
                host.events.clear();
                for later in input["later_moves"].as_array().expect("moves") {
                    if int(&later["frame"]) == i64::from(host.frame) {
                        move_to(&mut rocket, block, to(&later["destination"]), host.frame);
                    }
                }
                if input["kill_frames"]
                    .as_array()
                    .expect("kill frames")
                    .contains(&json!(host.frame))
                {
                    host.health = 0;
                }
                process(&mut rocket, block, cmisl, &mut host);
                let mut actual = observed(&rocket, &host);
                actual["events"] = json!(host.events);
                assert_eq!(&actual, expected, "{name}: frame {index}");
            }
            let last = frames.last().expect("a frame");
            assert_eq!(
                host.uninit,
                last["events"]
                    .as_array()
                    .expect("events")
                    .contains(&json!(["uninit"])),
                "{name}: the flight ends where the original's does"
            );
        }
    }
}
