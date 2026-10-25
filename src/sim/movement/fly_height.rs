//! Fly's retained destination, integer height target and vertical-motion range.
//!
//! Native4CDD0D..4CDFBC/4CE145 reads Object coordinates; it has no elapsed-time
//! climb rate or four-phase altitude machine. The surrounding horizontal,
//! landing-effect and Display transactions are separate callers, still being
//! migrated. Native comparisons: tools/spatial_oracle/fly_height.{py,json}.

use super::air_movement::{FlySpeedFacts, MIN_CREEP_SPEED, fly_may_slow};
#[cfg(any(test, feature = "test-support"))]
use super::locomotor::AirMovePhase;
use crate::sim::components::DriveCoord;
#[cfg(any(test, feature = "test-support"))]
use crate::util::fixed_math::SimFixed;
use crate::util::native_x87::{NativeF64Bits, X87Chop53 as X, X87Ordering, X87Value};

/// Original7E3860, added/subtracted with PC53/chop at4CE441..4CE495.
const FLY_SPEED_RAMP_STEP: NativeF64Bits = NativeF64Bits::from_bits(0x3fb9_9999_9999_999a);

/// The target speed of a Fly crawling in under the 0.1 slowdown floor
/// (`0x004CE27C`).
const FLY_CRAWL_SPEED: NativeF64Bits = FLY_SPEED_RAMP_STEP;

/// Horizontal_Step's arrival distances (`0x004CF4F1`, `0x004CF527`,
/// `0x004CF543`) and the speeds they ask for.
pub(crate) const ARRIVAL_STOP: i32 = 0x80;
const ARRIVAL_HALF: i32 = 0x200;
pub(crate) const ARRIVAL_THREE_QUARTERS: i32 = 0x300;
const HALF_SPEED: NativeF64Bits = NativeF64Bits::HALF;
const THREE_QUARTER_SPEED: NativeF64Bits = NativeF64Bits::from_bits(0x3fe8_0000_0000_0000);

fn speed_value(bits: NativeF64Bits) -> X87Value {
    X::load_f64(bits).expect("constructor-owned Fly speed is finite")
}

fn stored_speed(value: X87Value) -> u64 {
    X::store_f64(value).expect("bounded Fly speed store").bits()
}

const fn speed_is_zero(bits: u64) -> bool {
    bits & 0x7fff_ffff_ffff_ffff == 0
}

/// Constructor4CC9EE..4CC9FA clears target+38 and takeoff/landing+50/+51.
/// Only this owner mutates those fields. Object coordinates own current Z.
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct FlyRuntime {
    target_height: i32,
    taking_off: bool,
    landing: bool,
    /// Full Fly+1C/+20/+24, initialized to CoordStruct::Empty by4CC9A0.
    /// This is independent of Foot's NavCom identity and its changing position.
    destination: [i32; 3],
    /// Fly+5C, independent of the takeoff/landing callback flags. MoveTo
    /// chooses it from retained destination height, armed Target, Aircraft
    /// readiness and Landable. Enter resets it; the non-Landable phase branch
    /// forces it true. Constructor4CCA09 initializes false. Null MoveTo and
    /// mode-driven speed/navigation decisions still require their native port.
    #[serde(default)]
    cruise_mode: bool,
    /// Native+34: retained movement request; speed and destination can differ.
    #[serde(default)]
    moving: bool,
    /// Native+52: one-shot landing animation/sound, reset by BeginLanding.
    #[serde(default)]
    landing_effect_latched: bool,
    /// Full Fly+18: copied by LinkToObject4CCA20 from an Aircraft's type+E0D.
    /// Landing reads this instance value; BeginLanding separately reads live type.
    #[serde(default)]
    airport_bound: bool,
    /// Fly+58, the fall counter: the constructor clears it (`0x004CCA06`) and
    /// only Process's fall block writes it (`0x004CD6C6`), never resetting it.
    #[serde(default)]
    fall_counter: i32,
    /// Fly+40, the target speed fraction the ramp chases. The constructor
    /// (`0x004CC9E5`) starts it at 0; Process's slowdown
    /// ([`Self::write_target_speed`]), Horizontal_Step's arrival
    /// ([`Self::slow_for_arrival`]), the takeoff callback and the landing write
    /// it. Retain its binary64 store, including bits below one Q16 quantum.
    #[serde(default)]
    target_speed_bits: u64,
    /// Fly+48, the binary64 current fraction, chasing target by stored0.1
    /// with a PC53/chop operation and qword store each frame4CE441..4CE495.
    #[serde(default)]
    current_speed_bits: u64,
}

impl FlyRuntime {
    /// See the field.
    pub(crate) const fn target_speed(&self) -> NativeF64Bits {
        NativeF64Bits::from_bits(self.target_speed_bits)
    }

    /// See the field.
    pub(crate) const fn current_speed(&self) -> NativeF64Bits {
        NativeF64Bits::from_bits(self.current_speed_bits)
    }

    pub(crate) fn target_speed_is_zero(&self) -> bool {
        speed_is_zero(self.target_speed_bits)
    }

    pub(crate) fn current_speed_is_positive(&self) -> bool {
        X::compare(speed_value(self.current_speed()), X::load_i32(0)) == X87Ordering::Greater
    }

    pub(crate) fn current_speed_below_landing_threshold(&self) -> bool {
        X::compare(
            speed_value(self.current_speed()),
            speed_value(MIN_CREEP_SPEED),
        ) == X87Ordering::Less
    }

    /// Process `0x004CE441..0x004CE495`: the current speed (`+0x48`) chases
    /// the target speed (`+0x40`) by 0.1 a frame.
    pub(crate) fn ramp_speed(&mut self) {
        let target = speed_value(self.target_speed());
        let current = speed_value(self.current_speed());
        match X::compare(current, target) {
            X87Ordering::Less => {
                let next = X::add(current, speed_value(FLY_SPEED_RAMP_STEP));
                self.current_speed_bits = if X::compare(next, target) == X87Ordering::Less {
                    stored_speed(next)
                } else {
                    self.target_speed_bits
                };
            }
            X87Ordering::Greater => {
                let next = X::sub(current, speed_value(FLY_SPEED_RAMP_STEP));
                self.current_speed_bits = if X::compare(next, target) == X87Ordering::Greater {
                    stored_speed(next)
                } else {
                    self.target_speed_bits
                };
            }
            X87Ordering::Equal => {}
        }
    }

    /// Fly Process `0x004CE145..0x004CE2DB`: the target speed (`+0x40`) that the
    /// ramp chases, rewritten on every frame the Fly is moving and its owner
    /// lives, is not landing, has climbed to half its takeoff height and holds a
    /// destination:
    /// - `0x004D0180` refusing: full speed. A locked aircraft, a cruising FlyBy
    ///   and a cruising strafer or fighter with ammo never slow for their
    ///   destination.
    /// - HunterSeeker: full speed while it has a Target and is not taking off,
    ///   else a stop.
    /// - Otherwise `distance / SlowdownDistance`, capped at 1. Under 0.1 it is a
    ///   0.1 crawl beyond 85 leptons and, within, a stop that halves the current
    ///   speed. A current speed above the integer distance drops to it (only at
    ///   distance 0). A Fly already at a standstill short of its destination gets
    ///   a 0.05 current speed, which only the IsDropship attitude reads before the
    ///   ramp (`0x004CE46F`) takes it back to the zero target: it moves nothing.
    ///
    /// The ratio remains on the PC53/chop stack across its qword target store
    /// at4CE260. The converted integer distance remains below it and supplies
    /// the current-speed clamp at4CE29A. Neither retained speed is quantized.
    pub(crate) fn write_target_speed(&mut self, facts: &FlySpeedFacts) {
        let taking_off = self.taking_off;
        if facts.health <= 0
            || self.landing
            || (taking_off && facts.height < self.target_height / 2)
            || self.destination() == (DriveCoord { x: 0, y: 0, z: 0 })
        {
            return;
        }
        if !fly_may_slow(self.landing, self.cruise_mode, &facts.slow) {
            self.target_speed_bits = NativeF64Bits::ONE.bits();
            return;
        }
        if facts.hunter_seeker {
            self.target_speed_bits = if !taking_off && facts.target {
                NativeF64Bits::ONE.bits()
            } else {
                NativeF64Bits::POSITIVE_ZERO.bits()
            };
            return;
        }
        let distance = X::load_i32(facts.distance);
        let one = speed_value(NativeF64Bits::ONE);
        //4CE249..4CE25A caps infinity and unordered0/0 to1. Other legal
        // inputs divide bounded integers and retain a finite ratio.
        let ratio = if facts.slowdown_distance == 0 {
            one
        } else {
            X::div(distance, X::load_i32(facts.slowdown_distance)).unwrap()
        };
        let ratio = if X::compare(one, ratio) == X87Ordering::Less {
            one
        } else {
            ratio
        };
        self.target_speed_bits = stored_speed(ratio);
        if X::compare(ratio, speed_value(FLY_CRAWL_SPEED)) == X87Ordering::Less {
            if facts.distance > 0x55 {
                self.target_speed_bits = FLY_CRAWL_SPEED.bits();
            } else {
                self.current_speed_bits = stored_speed(X::mul(
                    speed_value(self.current_speed()),
                    speed_value(HALF_SPEED),
                ));
                self.target_speed_bits = NativeF64Bits::POSITIVE_ZERO.bits();
            }
        }
        if X::compare(distance, speed_value(self.current_speed())) == X87Ordering::Less {
            self.current_speed_bits = stored_speed(distance);
        }
        if self.target_speed_is_zero()
            && speed_is_zero(self.current_speed_bits)
            && facts.distance > 0
        {
            self.current_speed_bits = MIN_CREEP_SPEED.bits();
        }
    }

    /// Horizontal_Step's arrival arm (`0x004CF4F1`, `0x004CF527`,
    /// `0x004CF543`): a stop within `ARRIVAL_STOP`, half speed within
    /// `ARRIVAL_HALF`, three quarters within `ARRIVAL_THREE_QUARTERS`; a
    /// farther Fly keeps its target speed.
    pub(crate) fn slow_for_arrival(&mut self, distance: i32) {
        if distance < ARRIVAL_STOP {
            self.target_speed_bits = NativeF64Bits::POSITIVE_ZERO.bits();
        } else if distance < ARRIVAL_HALF {
            self.target_speed_bits = HALF_SPEED.bits();
        } else if distance < ARRIVAL_THREE_QUARTERS {
            self.target_speed_bits = THREE_QUARTER_SPEED.bits();
        }
    }

    /// The takeoff callback (`0x004CE680`) asks full speed.
    pub(crate) fn request_full_speed(&mut self) {
        self.target_speed_bits = NativeF64Bits::ONE.bits();
    }

    /// The landing callback (`0x004CE840`) asks a stop; the current speed
    /// ramps down to it.
    pub(crate) fn request_stop(&mut self) {
        self.target_speed_bits = NativeF64Bits::POSITIVE_ZERO.bits();
    }

    /// A completed landing stands the Fly still: target and current speed 0.
    pub(crate) fn halt(&mut self) {
        self.target_speed_bits = NativeF64Bits::POSITIVE_ZERO.bits();
        self.current_speed_bits = NativeF64Bits::POSITIVE_ZERO.bits();
    }

    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn set_target_speed_for_test(&mut self, target_speed: SimFixed) {
        self.target_speed_bits = target_speed.to_num::<f64>().to_bits();
    }

    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn set_current_speed_for_test(&mut self, current_speed: SimFixed) {
        self.current_speed_bits = current_speed.to_num::<f64>().to_bits();
    }

    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn set_target_speed_bits_for_test(&mut self, bits: NativeF64Bits) {
        self.target_speed_bits = bits.bits();
    }

    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn set_current_speed_bits_for_test(&mut self, bits: NativeF64Bits) {
        self.current_speed_bits = bits.bits();
    }
}

/// Techno-owned approach pitch. Native stores f32; the engine uses SimFixed.
/// The0.02 step rounds to1311/65536. Boundary histories are compared with the
/// executable; differences below one fixed quantum are intentionally retained.
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub(crate) struct FlightAttitude {
    pitch: crate::util::fixed_math::SimFixed,
}

impl FlightAttitude {
    pub(crate) fn blocks_landing(&self) -> bool {
        self.pitch > crate::util::fixed_math::SIM_ZERO
    }

    /// `+0x2E8 == 0.0`, as Aircraft Unload tests it (`0x0041520D`).
    pub(crate) fn is_level(&self) -> bool {
        self.pitch == crate::util::fixed_math::SIM_ZERO
    }

    /// Fly4CE2E5..4CE3BA. Caller owns IsDropship, health, speed and takeoff gates.
    pub(crate) fn approach(
        &mut self,
        distance: i32,
        slowdown: i32,
        angle: crate::util::fixed_math::SimFixed,
    ) {
        if distance >= slowdown || slowdown <= 0 {
            return;
        }
        // 1-(distance-0.6*slowdown)/(0.4*slowdown), then min(angle, ratio*angle).
        // i128 intermediates keep authored distances outside I16F16 in range.
        let numerator = i128::from(slowdown) - i128::from(distance);
        let bits = i128::from(angle.to_bits()) * numerator * 5 / (i128::from(slowdown) * 2);
        self.pitch = crate::util::fixed_math::SimFixed::from_bits(
            bits.min(i128::from(angle.to_bits()))
                .clamp(i128::from(i32::MIN), i128::from(i32::MAX)) as i32,
        );
    }

    /// Fly4CE8D4..4CE90A, only IsDropship at sampled height exactly0.
    pub(crate) fn settle(&mut self) {
        if self.blocks_landing() {
            self.pitch = (self.pitch - crate::util::fixed_math::SimFixed::lit("0.02"))
                .max(crate::util::fixed_math::SIM_ZERO);
        }
    }
}

pub(crate) enum TakeoffFacing {
    Unchanged,
    PrimaryToDestination,
    SecondaryToPrimaryDestination,
}

impl FlyRuntime {
    pub(crate) fn link(&mut self, airport_bound: bool) {
        self.airport_bound = airport_bound;
    }

    pub(crate) fn airport_bound(&self) -> bool {
        self.airport_bound
    }

    pub(crate) fn has_phase_callback(&self) -> bool {
        self.taking_off || self.landing
    }

    pub(crate) fn landing(&self) -> bool {
        self.landing
    }

    pub(crate) fn taking_off(&self) -> bool {
        self.taking_off
    }

    pub(crate) fn moving(&self) -> bool {
        self.moving
    }

    /// `Is_Moving_Now` `0x004CCAC0`: the current speed (Fly+48) is not zero.
    /// The +34 request is not asked.
    pub(crate) fn is_moving_now(&self) -> bool {
        !speed_is_zero(self.current_speed_bits)
    }

    pub(crate) fn landing_effect_latched(&self) -> bool {
        self.landing_effect_latched
    }

    /// Process `0x004CD6B1..0x004CD6E4`: an unpowered or dead Fly above the
    /// ground adds 1 a frame to its fall counter when dead, 3 when alive, and
    /// drops by the new counter. Returns the counter.
    pub(crate) fn advance_fall(&mut self, dead: bool) -> i32 {
        self.fall_counter = self.fall_counter.wrapping_add(if dead { 1 } else { 3 });
        self.fall_counter
    }

    pub(crate) fn fall_counter(&self) -> i32 {
        self.fall_counter
    }

    ///4CEB53..4CEB90, once per admitted BeginLanding, strictly below300.
    pub(crate) fn admit_landing_effect(&mut self, height: i32) -> bool {
        if self.landing_effect_latched || height >= 300 {
            return false;
        }
        self.landing_effect_latched = true;
        true
    }

    ///4CED0B..4CED13; destination and moving are handled later, conditionally.
    pub(crate) fn finish_landing(&mut self) {
        self.landing = false;
        self.taking_off = false;
    }

    ///4CEF5F precedes null MoveTo. Null MoveTo itself never writes+34.
    pub(crate) fn finish_destination(&mut self) {
        self.moving = false;
    }

    /// Destination = the empty coordinate after a C4 self-destruct: the
    /// landing retry's (`0x004CEBCD`) and Stop_Moving's (`0x004CD273`).
    pub(crate) fn clear_destination(&mut self) {
        self.destination = [0; 3];
    }

    /// Admitted null MoveTo4CCD6D..4CCE00. Caller performs BeginLanding if
    /// requested; its refusal leaves the copied current coordinate intact.
    pub(crate) fn null_destination(&mut self, current: DriveCoord, height: i32, base: i32) -> bool {
        let begin = (height > base || self.taking_off) && !self.landing;
        self.destination = if begin {
            [current.x, current.y, current.z]
        } else {
            [0; 3]
        };
        self.cruise_mode = false;
        begin
    }

    /// Phase4CD3C3 only sets this flag. It does not call BeginTakeoff.
    pub(crate) fn reject_landing_cell(&mut self) {
        self.taking_off = true;
    }

    /// Takeoff4CE746..4CE821. Height is already bridge-normalized by the caller.
    /// Both flags clear even below either threshold; this is a callback latch,
    /// not a persistent ascending phase. No coordinate write occurs here.
    pub(crate) fn complete_takeoff(&mut self, height: i32, landing_base: i32) -> TakeoffFacing {
        let height = height.wrapping_sub(landing_base);
        let target = self.target_height.wrapping_sub(landing_base);
        self.taking_off = false;
        self.landing = false;
        if height > target.wrapping_sub(target / 3) {
            TakeoffFacing::SecondaryToPrimaryDestination
        } else if height > target / 2 {
            TakeoffFacing::PrimaryToDestination
        } else {
            TakeoffFacing::Unchanged
        }
    }

    pub(crate) fn destination(&self) -> DriveCoord {
        let [x, y, z] = self.destination;
        DriveCoord { x, y, z }
    }

    pub(crate) fn cruise_mode(&self) -> bool {
        self.cruise_mode
    }

    /// Process `0x004CCB47..0x004CCB81`, first in every visit: a Fly neither
    /// landing nor taking off, with a target speed of at least 1 and no
    /// height target, takes its type's FlightLevel (type vt+0xBC) again.
    pub(crate) fn restore_flight_level(&mut self, flight_level: i32) {
        if !self.landing
            && !self.taking_off
            && X::compare(
                speed_value(self.target_speed()),
                speed_value(NativeF64Bits::ONE),
            ) != X87Ordering::Less
            && self.target_height == 0
        {
            self.target_height = flight_level;
        }
    }

    /// Horizontal_Step's height target (`0x004CF3F6`, `0x004CF4B6`,
    /// `0x004CF4CF`), rewritten on each step.
    pub(crate) fn set_flight_level(&mut self, flight_level: i32) {
        self.target_height = flight_level;
    }

    /// Process4CD664..4CD67F, before its power/health branches. The effective
    /// Mission getter5B3040 falls back to queued only when current is -1.
    pub(crate) fn prepare_process(&mut self, mission: crate::sim::mission::MissionId) {
        if mission
            == crate::sim::mission::MissionId::from_known(crate::sim::mission::MissionType::Enter)
        {
            self.cruise_mode = false;
        }
    }

    /// MoveTo4CCED9..4CCFA7, after the destination and takeoff transaction.
    /// Ground is queried from the retained coordinate even when the armed or
    /// non-Landable arm will force true. Signed addition wraps like native.
    pub(crate) fn select_destination_mode(
        &mut self,
        ground: i32,
        armed: bool,
        aircraft_ready: bool,
        non_landable_aircraft: bool,
    ) {
        self.cruise_mode = self.destination[2] > ground.wrapping_add(120)
            || armed
            || aircraft_ready
            || non_landable_aircraft;
    }

    /// Phase4CD2CA..4CD2F5 precedes the ordinary callback/Display transaction.
    /// It clears both requests even when neither was set and rereads the live
    /// type FlightLevel. It does not move the owner or reset either speed.
    pub(crate) fn force_non_landable_flight(&mut self, flight_level: i32) {
        self.cruise_mode = true;
        self.landing = false;
        self.taking_off = false;
        self.target_height = flight_level;
    }

    /// MoveTo4CCC80..4CCCE0: signed truncation, then 16-bit CellStruct equality.
    /// This refusal precedes owner disable/power gates and all ground queries.
    pub(crate) fn ignores_destination(&self, request: DriveCoord) -> bool {
        self.landing
            && (self.destination[0] / 256) as i16 == (request.x / 256) as i16
            && (self.destination[1] / 256) as i16 == (request.y / 256) as i16
    }

    /// Admitted non-null MoveTo4CCE1C..4CCE6E. A live Target and signed Ammo!=0
    /// replace Z with ground+FlightLevel; ground is read only on that arm.
    /// Moving+34 is written before destination; the caller selects mode+5C
    /// after the complete BeginTakeoff transaction.
    pub(crate) fn retain_destination(
        &mut self,
        request: DriveCoord,
        armed_flight_level: Option<i32>,
        ground: impl FnOnce() -> i32,
    ) {
        self.moving = true;
        self.destination = [request.x, request.y, request.z];
        if let Some(flight_level) = armed_flight_level {
            self.destination[2] = ground().wrapping_add(flight_level);
        }
    }

    /// Historical hash projection before the destination was retained (188).
    pub(crate) fn height_hash_fields(&self) -> (i32, bool, bool) {
        (self.target_height, self.taking_off, self.landing)
    }

    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn target_height(&self) -> i32 {
        self.target_height
    }

    /// MoveTo4CCEAE..4CCED4. The caller owns admission and the Aircraft
    /// landing-base query (zero for ordinary, non-Carryall aircraft).
    pub(crate) fn should_begin_takeoff(
        &self,
        health: i32,
        height: impl FnOnce() -> i32,
        landing_base: i32,
    ) -> bool {
        // GetHeight may stamp the shared map Dummy. Native calls it only on
        // the final arm, after health and both phase gates.
        health > 0 && !self.taking_off && (self.landing || height() <= landing_base)
    }

    /// Admitted BeginTakeoff stores4CF9A5/4CF9A8/4CF9F8. Owner refusal,
    /// facing, spatial membership and sound belong to the calling transaction.
    pub(crate) fn begin_takeoff(&mut self, flight_level: i32) {
        self.landing = false;
        self.taking_off = true;
        self.target_height = flight_level;
    }

    /// Admitted BeginLanding stores4CFADF..4CFAEB. The full native landing
    /// callback/effect transaction remains required; these are not phase names.
    pub(crate) fn begin_landing(&mut self) {
        self.taking_off = false;
        self.landing = true;
        self.landing_effect_latched = false;
        self.target_height = 0;
    }

    /// A test projection of the flight phase. This is deliberately not the
    /// native+50/+51 flags or an independently saved FSM.
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn mission_phase(&self, height: i32) -> AirMovePhase {
        if height == 0 && self.target_height == 0 {
            AirMovePhase::Landed
        } else if height < self.target_height {
            AirMovePhase::Ascending
        } else if height > self.target_height {
            AirMovePhase::Descending
        } else {
            AirMovePhase::Cruising
        }
    }

    pub(crate) fn step_height(&self, input: HeightInput) -> HeightOutput {
        let HeightInput {
            world_z,
            ground_z,
            mut on_bridge,
            structural_bridge,
            health,
            has_passenger,
            is_dropship,
            flight_level,
        } = input;
        // Object GetHeight5F5F40 subtracts ground and its current OnBridge
        // adjustment. Fly then independently normalizes an unattached deck.
        let mut height = super::ground_pose::height_at_z(world_z, ground_z, on_bridge);
        let mut bridge_bonus = 0;
        if !on_bridge
            && height >= crate::util::lepton::BRIDGE_DECK_HEIGHT_LEPTONS
            && structural_bridge
        {
            height = height.wrapping_sub(crate::util::lepton::BRIDGE_DECK_HEIGHT_LEPTONS);
            bridge_bonus = crate::util::lepton::BRIDGE_DECK_HEIGHT_LEPTONS;
        }
        let mut output_z = world_z;
        if height < self.target_height && health > 0 {
            let rate = if is_dropship {
                16
            } else if has_passenger {
                10
            } else {
                20
            };
            let step = self.target_height.wrapping_sub(height).min(rate);
            // SetHeight happens BEFORE OnBridge is cleared (4CDE9D/4CDEA6).
            output_z = super::ground_pose::z_at_height(
                ground_z,
                height.wrapping_add(step).wrapping_add(bridge_bonus),
                on_bridge,
            );
            on_bridge = false;
        }
        // Native compares the original normalized height again. Health==0
        // forces descent, while a negative health is not interchangeable.
        let descended = height > self.target_height || health == 0;
        if descended {
            let delta = height.wrapping_sub(self.target_height);
            let step = if is_dropship {
                if self.landing {
                    delta.min((delta / 20 + 10).min(48))
                } else {
                    height.min(if self.target_height == flight_level {
                        16
                    } else {
                        6
                    })
                }
            } else {
                delta.min(delta / 20).clamp(20, 50)
            };
            height = height.wrapping_sub(step);
            if health == 0 && height <= 1 {
                height = 1;
            } else if height <= 0 {
                height = 0;
                if bridge_bonus > 0 {
                    on_bridge = true;
                    bridge_bonus = 0;
                }
            }
            output_z = super::ground_pose::z_at_height(
                ground_z,
                height.wrapping_add(bridge_bonus),
                on_bridge,
            );
        }
        HeightOutput {
            world_z: output_z,
            on_bridge,
            height: super::ground_pose::height_at_z(output_z, ground_z, on_bridge),
            descended,
        }
    }
}

pub(crate) struct HeightInput {
    pub world_z: i32,
    pub ground_z: i32,
    pub on_bridge: bool,
    pub structural_bridge: bool,
    pub health: i32,
    pub has_passenger: bool,
    pub is_dropship: bool,
    pub flight_level: i32,
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) struct HeightOutput {
    pub world_z: i32,
    pub on_bridge: bool,
    pub height: i32,
    /// The descent block ran (`0x004CDED8..0x004CDFB6`): Process's landing
    /// drift follows its SetHeight.
    descended: bool,
}

impl HeightOutput {
    /// The descent block ran: Process's landing drift follows.
    pub(crate) fn descended(&self) -> bool {
        self.descended
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_approach_attitude_and_completion_histories() {
        use crate::util::fixed_math::SimFixed;
        let rows: Vec<serde_json::Value> = serde_json::from_str(crate::test_fixture::text(
            "tools/spatial_oracle/fly_attitude.json",
        ))
        .unwrap();
        assert_eq!(rows.len(), 65);
        for row in rows {
            let input = &row["input"];
            let mut state = FlightAttitude {
                pitch: SimFixed::from_num(input["initial"].as_f64().unwrap_or(0.0)),
            };
            if input["dropship"].as_bool().unwrap_or(true)
                && input["health"].as_i64().unwrap_or(100) > 0
                && input["speed"].as_f64().unwrap_or(0.5) > 0.0
                && !input["taking_off"].as_bool().unwrap_or(false)
            {
                state.approach(
                    input["distance"].as_i64().unwrap() as i32,
                    input["slowdown"].as_i64().unwrap() as i32,
                    SimFixed::from_num(row["radians"].as_f64().unwrap()),
                );
            }
            assert!(
                (state.pitch.to_num::<f64>() - row["history"][0].as_f64().unwrap()).abs()
                    < 1.0 / 65536.0,
                "{row}"
            );
            let mut ticks = 0;
            while state.blocks_landing() && ticks < 100 {
                state.settle();
                ticks += 1;
            }
            assert_eq!(ticks, row["settle_ticks"].as_u64().unwrap(), "{row}");
        }
    }

    #[test]
    fn all_saved_original_vertical_steps_match() {
        let rows: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
            "tools/spatial_oracle/fly_height.json",
        ))
        .unwrap();
        let rows = rows.as_array().unwrap();
        assert_eq!(rows.len(), 144);
        for row in rows {
            let c = &row["input"];
            let integer =
                |name: &str, fallback: i32| c[name].as_i64().map_or(fallback, |v| v as i32);
            let flag = |name: &str| c[name].as_bool().unwrap_or(false);
            let target = integer("target", 0);
            let mut state = FlyRuntime::default();
            if flag("landing") {
                state.begin_landing();
            }
            state.set_flight_level(target);
            let ground_z = crate::util::lepton::ground_height_leptons(
                integer("level", 0) as u8,
                integer("slope", 0) as u8,
                2688,
                2688,
            )
            .unwrap();
            let authored_level = integer("flight_level", -1);
            let actual = state.step_height(HeightInput {
                world_z: integer("z", 0),
                ground_z,
                on_bridge: flag("on_bridge"),
                structural_bridge: flag("bridge"),
                health: integer("health", 100),
                has_passenger: flag("loaded") && c["kind"].as_str() != Some("unit"),
                is_dropship: flag("dropship"),
                flight_level: if authored_level == -1 {
                    1500
                } else {
                    authored_level
                },
            });
            assert_eq!(
                (actual.world_z, actual.on_bridge, actual.height),
                (
                    row["z"].as_i64().unwrap() as i32,
                    row["on_bridge"].as_bool().unwrap(),
                    row["height"].as_i64().unwrap() as i32,
                ),
                "{}",
                c["name"]
            );
        }
    }
}
