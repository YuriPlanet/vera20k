//! Jumpjet flight kernel: the per-frame altitude and translation body shared by
//! every moving Jumpjet state, and the cruise state that steers it.
//!
//! gamemd-derived (disassembly read 2026-09-15; YR 1.001, SHA-256 `1cdd1180…4298c`):
//!
//! - `JumpjetLocomotionClass::Update_Coordinates_And_Altitude @ 0x0054D0F0`,
//!   called from `Process @ 0x0054AEC0` whenever the locomotor reports moving.
//!   It ramps the speed double toward the target speed, advances the hover bob,
//!   integrates height toward the bobbed target height over a terrain reference,
//!   zeroes the speed while too low outside the destination cell, then steps the
//!   owner along the locomotor's own facing and copies that facing to the body.
//! - `JumpjetLocomotionClass::State3_Translate @ 0x0054BFF0`: desired facing
//!   toward the destination through the retail atan table, four distance speed
//!   zones with a turn-error slowdown, target-height choice, and arrival below
//!   20 leptons.
//! - The crash: `Process`'s latch (`0x0054AF2E..0x0054B02C`) turns a crashing
//!   owner (`FootClass+0x425`) still above the ground into State 5
//!   `0x0054CA90`, which drops it by `JumpjetCrash=` a frame on top of
//!   Update's descent, spins its facing, and at the ground (or on crossing a
//!   bridge deck) releases its air slot and sends the owner's impact notice.
//! - The reference height helper `0x0054D820` and the cell top height
//!   `CellClass @ 0x00485080` (ground at the cell centre, plus a building's
//!   `Dimension2` height or 85 leptons for any other techno in the cell).
//!
//! Parity demonstrated for the flat-map subset by
//! `tools/spatial_oracle/jumpjet_flight.json` (native Unicorn execution; see the
//! `.meta.json` scope): speed ramps, zones, turn slowdowns, bob, climb and
//! descent, the low-altitude speed gate, arrival into state 4 or a claimed hold.
//! The crash fall and its impact are pinned by
//! `tools/spatial_oracle/jumpjet_crash.json` for every stock Crashable Unit
//! type. Bridges, building tops and cell objects are Rust-tested only.
//!
//! Numeric model: `WinMain` installs x87 control word `0x0E7F` (53-bit
//! precision, round toward zero; `_controlfp(0x300, 0x300)` at `0x006BBFC1`),
//! and every double here is evaluated in native operand order with
//! [`X87Chop53`]. Sine, cosine and arctangent read the retail tables; the
//! distance uses `Sqrt_Approx`; `Math::ftol @ 0x007C5F00` keeps the low dword.
//!
//! ## Dependency rules
//! - Part of sim/ — depends on map/retail_trig, rules/jumpjet_params, util and
//!   sim/movement only.

use crate::map::retail_trig::{AtanTable, TrigTable};
use crate::rules::jumpjet_params::JumpjetParams;
use crate::sim::movement::facing_class::FacingClass;
use crate::util::lepton::GROUND_LEVEL_HEIGHT_LEPTONS;
use crate::util::native_x87::{NativeF32Bits, NativeF64Bits, X87Chop53, X87Ordering, X87Value};

/// `[0x00ABC5DC]`, the bridge deck height.
pub(crate) const BRIDGE_DECK_LEPTONS: i32 = 416;
/// Arrival radius in leptons (`CMP EBX,0x14` at `0x0054C163`).
const ARRIVAL_RADIUS: i32 = 20;
/// Extra reference height for a non-building techno in a cell (`ADD EBP,0x55`
/// at `0x004850EF`).
const CELL_OBJECT_LIFT: i32 = 0x55;
/// `0x007E2810`: -2pi/65536.
const NEG_RADIANS_PER_FACING_UNIT: u64 = 0xBF19_222D_989F_5E57;
/// `0x007E2818`: -65536/2pi.
const NEG_FACING_UNITS_PER_RADIAN: u64 = 0xC0C4_5F07_AF68_ECEF;
/// `0x007E2820`: pi/2.
const HALF_PI: u64 = 0x3FF9_21FB_5444_2D18;
/// `0x007E3CC0`: 2pi.
const TWO_PI: u64 = 0x4019_21FB_5444_2D18;
/// `0x007ECE60`: 15.0.
const FIFTEEN: u64 = 0x402E_0000_0000_0000;
/// `0x007E48F0`: 1.5.
const ONE_AND_A_HALF: u64 = 0x3FF8_0000_0000_0000;
/// `0x007E7FC0`: 0.75.
const THREE_QUARTERS: u64 = 0x3FE8_0000_0000_0000;
/// `0x007E1718`: 1.0.
const ONE: u64 = 0x3FF0_0000_0000_0000;
/// `g_DirectionDelta` at `0x0089F6D8`, filled by `0x0049F3A0`: north first,
/// clockwise, one cell in leptons.
const DIRECTION_DELTA: [(i32, i32); 8] = [
    (0, -256),
    (256, -256),
    (256, 0),
    (256, 256),
    (0, 256),
    (-256, 256),
    (-256, 0),
    (-256, -256),
];

/// Native state byte `+0x50`.
pub(crate) const STATE_GROUND: i32 = 0;
pub(crate) const STATE_ASCEND: i32 = 1;
pub(crate) const STATE_HOLD: i32 = 2;
pub(crate) const STATE_TRANSLATE: i32 = 3;
pub(crate) const STATE_DESCEND: i32 = 4;
/// State 5, the crash fall (`0x0054CA90`).
pub(crate) const STATE_CRASH: i32 = 5;
/// State 6, set by the crash's impact; the jump table at `0x0054B19C` runs no
/// body for it.
pub(crate) const STATE_CRASHED: i32 = 6;
/// The target height the crash latch sets (`MOV [ESI+0x7C],-5` at
/// `0x0054B006`), which Update's descent chases below the ground.
const CRASH_TARGET_HEIGHT: i32 = -5;
/// `ADD CX,0x2D00` at `0x0054CB24`: State 5 re-aims the facing this far past
/// its animated current every frame, so it turns at the full turn rate.
const CRASH_SPIN: u16 = 0x2D00;

/// The type block `Link_To_Object @ 0x0054AD30` copies into the locomotor
/// (receiver `+0x1C..+0x3C`). Floats keep their binary32 bits.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct JumpjetFlightParams {
    /// `+0x1C` `JumpjetTurnRate=`: the facing rate and the turn-error threshold.
    pub turn_rate: i32,
    /// `+0x20` `JumpjetSpeed=` (an int).
    pub speed: i32,
    /// `+0x24` `JumpjetClimb=`.
    pub climb_bits: u32,
    /// `+0x28` `JumpjetCrash=`.
    pub crash_bits: u32,
    /// `+0x2C` `JumpjetHeight=`, floored at two cell levels (`0x0054AD9B`).
    pub height: i32,
    /// `+0x30` `JumpjetAccel=`.
    pub accel_bits: u32,
    /// `+0x34` `JumpjetWobbles=`.
    pub wobbles_bits: u32,
    /// `+0x38` `JumpjetDeviation=`.
    pub deviation: i32,
    /// `+0x3C` `JumpjetNoWobbles=`.
    pub no_wobbles: bool,
}

impl Default for JumpjetFlightParams {
    fn default() -> Self {
        Self::link(&JumpjetParams::default())
    }
}

impl JumpjetFlightParams {
    /// `Link_To_Object @ 0x0054AD30`'s straight-line copy.
    ///
    /// `JumpjetSpeed=` is an int natively; VERA's rules keep a fraction, which
    /// truncates here (the reader residual is recorded in `jumpjet_params.rs`).
    pub fn link(params: &JumpjetParams) -> Self {
        Self {
            turn_rate: params.turn_rate,
            speed: params.speed.to_num::<i32>(),
            climb_bits: params.climb.to_bits(),
            crash_bits: params.crash.to_bits(),
            height: params.height.max(2 * GROUND_LEVEL_HEIGHT_LEPTONS),
            accel_bits: params.accel.to_bits(),
            wobbles_bits: params.wobbles.to_bits(),
            deviation: params.deviation,
            no_wobbles: params.no_wobbles,
        }
    }

    /// The locomotor facing `Link_To_Object` builds (`FUN_004C91E0` then
    /// `Set`/`UpdateFacing` to `0x4000`). The rate constructor4C91E0 uses the
    /// same signed clamp/low-byte shift as SetROT4C9680; preserve its raw word
    /// even when the controller interprets it as non-positive (instant).
    pub fn linked_facing(&self) -> FacingClass {
        FacingClass::new(0x4000, self.turn_rate)
    }
}

/// Flight fields of the locomotor (receiver `+0x54..+0x8C`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct JumpjetFlight {
    /// `+0x54` the locomotor's own facing; the body copies it every Update.
    pub facing: FacingClass,
    /// `+0x70` current speed, binary64 bits.
    pub current_speed_bits: u64,
    /// `+0x78` target speed, binary64 bits.
    pub target_speed_bits: u64,
    /// `+0x80` target height above the reference.
    pub target_height: i32,
    /// `+0x88` hover bob phase in radians, binary64 bits.
    pub bob_phase_bits: u64,
}

impl JumpjetFlight {
    pub fn linked(params: &JumpjetFlightParams) -> Self {
        Self {
            facing: params.linked_facing(),
            current_speed_bits: 0,
            target_speed_bits: 0,
            target_height: 0,
            bob_phase_bits: 0,
        }
    }

    pub fn current_speed(&self) -> f64 {
        f64::from_bits(self.current_speed_bits)
    }

    #[cfg(test)]
    pub fn target_speed(&self) -> f64 {
        f64::from_bits(self.target_speed_bits)
    }
}

impl Default for JumpjetFlight {
    fn default() -> Self {
        Self::linked(&JumpjetFlightParams::default())
    }
}

/// RTTI of the owner as the kernel distinguishes it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FlightOwnerKind {
    Unit,
    Infantry,
    Other,
}

/// Everything the flight kernel reads from or writes to the owner and the map.
/// Coordinates are world leptons.
pub(crate) trait JumpjetFlightHost {
    fn binary_frame(&self) -> u32;
    fn trig(&self) -> &TrigTable;
    fn atan(&self) -> &AtanTable;
    fn owner_kind(&self) -> FlightOwnerKind;
    /// Owner `+0x9C` coordinate.
    fn location(&self) -> [i32; 3];
    /// `FootClass::SetLocation` (vtable `+0x1B4`).
    fn set_location(&mut self, coord: [i32; 3]);
    /// `ObjectClass::SetZ @ 0x005F6060`.
    fn set_z(&mut self, z: i32);
    /// `ObjectClass::GetHeight @ 0x005F5F40`.
    fn height_above_ground(&self) -> i32;
    /// Owner `+0x8C`, on a bridge deck.
    fn on_bridge(&self) -> bool;
    /// Update's grounded reset at `0x0054D407..D438`: vtable `+0xF4` with the
    /// location, then `+0x8C = 0`.
    fn grounded_reset(&mut self);
    /// `CellClass::GetGroundHeight @ 0x00578080` at a coordinate, deck excluded.
    fn floor_height(&self, xy: [i32; 2]) -> i32;
    /// The cell holding `xy` carries a high bridge (`+0x140 & 0x100`).
    fn cell_high_bridge(&self, xy: [i32; 2]) -> bool;
    /// `CellClass @ 0x00485080` for the cell holding `xy`.
    fn cell_top_height(&self, xy: [i32; 2]) -> i32;
    /// `CellClass+0xEC` LandType of the cell holding `xy`.
    fn cell_land_type(&self, xy: [i32; 2]) -> u8;
    /// `TechnoTypeClass+0xD6A` `BalloonHover=`.
    fn balloon_hover(&self) -> bool;
    /// Owner `+0x2B4` TarCom is set.
    fn has_target(&self) -> bool;
    /// Foot `+0x6AD`, the locomotor piggyback/deploy flag.
    fn piggyback_active(&self) -> bool;
    /// Arrival with the piggyback flag: owner `+0x2B0` then `0x0070FEE0(1)`.
    fn piggyback_arrival(&mut self);
    /// `UnitTypeClass+0xE13` `IsSimpleDeployer=` on a Unit owner.
    fn simple_deployer(&self) -> bool;
    /// `TechnoTypeClass+0x6AD` on a Unit owner.
    fn type_flag_6ad(&self) -> bool;
    /// `SetSpeedFraction` (vtable `+0x544`), with the binary64 argument.
    fn set_speed_fraction(&mut self, fraction_bits: u64);
    /// `0x00705D60` on the owner after an arrival transition.
    fn arrival_notify(&mut self);
    /// `FacingClass::UpdateFacing` on the body facing (`+0x388`).
    fn snap_body_facing(&mut self, facing: u16);
    /// Infantry with `InfantryTypeClass+0xECB` holding a target in state 2:
    /// the facing `0x005F3DB0` answers toward the target.
    fn hold_target_facing(&self) -> Option<u16>;

    // Seams the takeoff, hold and landing states add on top of the cruise.
    /// The cell holding `xy`, as `MapClass::Get_CellClass_At_Coord @ 0x00565730`
    /// resolves it.
    fn cell_of(&self, xy: [i32; 2]) -> (i16, i16);
    /// Owner body facing `+0x388`; State 0 snaps the locomotor facing to it.
    fn body_facing(&self) -> u16;
    /// `RandomRanged(0, 7)` on `ScenarioClass+0x218`.
    fn random_direction(&mut self) -> u32;
    /// `0x004135A0`: `cell+0xE0` holds an object that is not the owner.
    fn air_slot_taken_at(&mut self, cell: (i16, i16)) -> bool;
    /// `cell+0xE0` is the owner itself.
    fn holds_air_slot_at(&self, cell: (i16, i16)) -> bool;
    /// `cell+0xE0 == 0`, read directly rather than through `0x004135A0`.
    fn air_slot_empty_at(&self, cell: (i16, i16)) -> bool;
    /// Drop every slot this owner still holds. This stands in for the release
    /// of the owner's cached cell `+0x560` at `0x0054BF75..0x0054BFA8`, which
    /// VERA has no equivalent for: the cached cell names the cell the owner was
    /// last marked into, which is the one it claimed.
    fn release_owner_air_slots(&mut self);
    /// `0x00487D70(cell, owner)`.
    fn claim_air_slot_at(&mut self, cell: (i16, i16));
    /// `0x00487D70(cell, 0)`.
    fn release_air_slot_at(&mut self, cell: (i16, i16));
    /// `Set_Destination` (vtable `+0x480`) with the stepped neighbour cell.
    fn set_destination_cell(&mut self, cell: (i16, i16));
    /// `Can_Enter_Cell` (vtable `+0x1AC`) at the landing cell. Native answers 0
    /// for a clear cell; 2 is conditional and anything above 2 refuses.
    fn can_enter_cell(&self, cell: (i16, i16)) -> i32;
    /// `CellClass::IsSubCellFree @ 0x00481130`, for sub-cells 2 and above.
    /// Sub-cells 0 and 1 answer a definite `false` (`XOR AL,AL` at
    /// `0x0048117A`; only the upper bytes of EAX are stale), which the caller
    /// applies, so this is never consulted for them.
    fn sub_cell_free(&self, cell: (i16, i16), sub_cell: i32, bridge: bool) -> bool;
    /// The owner's mission (`+0xB4`) or its queued mission (vtable `+0x184`)
    /// is 7, which skips State 4's landing admission.
    fn mission_is_seven(&self) -> bool;
    /// `Stop_Moving` (interface vtable `+0x48`), which re-targets a nearby cell
    /// through `Move_To` and answers the state that leaves it in.
    fn stop_moving(&mut self) -> i32;
    /// `CellClass+0x140 & 0x100`: the cell carries a high bridge.
    fn cell_high_bridge_at(&self, cell: (i16, i16)) -> bool;
    /// Locomotor `+0x90`, the latch State 4 sets once it has admitted a
    /// landing, so later frames stop re-testing the cell.
    fn landing_latched(&self) -> bool;
    /// Set that latch and run the owner's `+0xF0` landing callback.
    fn begin_landing(&mut self, destination: [i32; 3]);
    /// Owner `+0x134`, which suppresses the `DeployToLand=` hold.
    fn deploy_latched(&self) -> bool;
    /// `RulesClass+0x48` as a facing, the heading a simple deployer turns to
    /// before it settles (`0x0054C767..C7A2`).
    fn deploy_facing(&self) -> Option<u16>;
    /// Touchdown's owner bookkeeping at `0x0054C8CB..0x0054CA7B`: the
    /// destination becomes NullCoord, the moving byte clears,
    /// `Set_Destination(0, 1)` runs, the air slot and bucket are released, the
    /// crate at the cell is picked up and the landing latches are cleared.
    fn touchdown(&mut self);

    // Seams the crash adds.
    /// Owner `+0x425`, the latch `FootClass::Crash @ 0x004DEBB0` raises.
    fn crashing(&self) -> bool;
    /// `MapClass::In_Bounds @ 0x00568300` for a cell.
    fn in_bounds(&self, cell: (i16, i16)) -> bool;
    /// State 5's relocation (`0x0054CBCB..0x0054CC0D`): the owner leaves the
    /// display (`0x004A9770`) and its cell (`Mark(REMOVE)`, vtable `+0x124`),
    /// moves (`SetLocation`, `+0x1B4`), and is marked (`Mark(PUT)`) and
    /// submitted (`0x004A9720`) again.
    fn crash_relocate(&mut self, coord: [i32; 3]);
    /// The crash impact's owner work after its air slot release
    /// (`0x0054D06C..0x0054D095`): `AircraftTracker::Remove @ 0x004135D0`, then
    /// the owner's `INoticeSink` slot 0 with `(0x117C, 0)`, whose handler
    /// finishes the wreck.
    fn crash_impact(&mut self);
}

fn zero() -> X87Value {
    X87Chop53::load_i32(0)
}

fn int(value: i32) -> X87Value {
    X87Chop53::load_i32(value)
}

fn double(bits: u64) -> X87Value {
    X87Chop53::load_f64(NativeF64Bits::from_bits(bits)).unwrap_or_else(|_| zero())
}

fn single(bits: u32) -> X87Value {
    X87Chop53::load_f32(NativeF32Bits::from_bits(bits)).unwrap_or_else(|_| zero())
}

fn store_double(value: X87Value) -> u64 {
    X87Chop53::store_f64(value).map_or(0, |bits| bits.bits())
}

fn ordering(lhs: X87Value, rhs: X87Value) -> X87Ordering {
    X87Chop53::compare(lhs, rhs)
}

fn native_cell(value: i32) -> i16 {
    (value.wrapping_add((value >> 31) & 0xFF) >> 8) as i16
}

/// The eight-way direction `0x0054D897..D8AC` derives from a 16-bit facing.
fn facing_direction(facing: u16) -> usize {
    ((((u32::from(facing) >> 12) + 1) >> 1) & 7) as usize
}

/// Reference height `0x0054D820`: the current cell's top height, and while
/// moving the max-or-average with the cell one step ahead along the facing.
///
/// Both bridge tests add the deck to the *current* cell's value (`ADD EDI` at
/// `0x0054D875` and `0x0054D906`), so a bridge ahead raises the current sample.
fn reference_height(
    flight: &JumpjetFlight,
    host: &impl JumpjetFlightHost,
    location: [i32; 3],
) -> i32 {
    let here_xy = [location[0], location[1]];
    let mut here = host.cell_top_height(here_xy);
    if host.cell_high_bridge(here_xy) {
        here = here.wrapping_add(BRIDGE_DECK_LEPTONS);
    }
    if ordering(double(flight.current_speed_bits), zero()) != X87Ordering::Greater {
        return here;
    }
    let (dx, dy) = DIRECTION_DELTA[facing_direction(flight.facing.current(host.binary_frame()))];
    let ahead_xy = [location[0].wrapping_add(dx), location[1].wrapping_add(dy)];
    let ahead = host.cell_top_height(ahead_xy);
    if host.cell_high_bridge(ahead_xy) {
        here = here.wrapping_add(BRIDGE_DECK_LEPTONS);
    }
    if ahead > here {
        ahead
    } else {
        ahead.wrapping_add(here) / 2
    }
}

/// `Update_Coordinates_And_Altitude @ 0x0054D0F0`.
pub(crate) fn update_coordinates_and_altitude(
    phase: i32,
    destination: [i32; 3],
    params: &JumpjetFlightParams,
    flight: &mut JumpjetFlight,
    host: &mut impl JumpjetFlightHost,
) {
    let hold_or_translate = matches!(phase, STATE_HOLD | STATE_TRANSLATE);

    // Speed ramp 0x0054D138..D1AE. The acceleration test runs first and the
    // deceleration test reads the updated value, so a target below the cap can
    // be overshot and pulled back in the same frame.
    let cap = int(params.speed);
    let target = double(flight.target_speed_bits);
    let mut current = double(flight.current_speed_bits);
    if ordering(target, current) == X87Ordering::Greater {
        let next = X87Chop53::add(single(params.accel_bits), current);
        current = if ordering(next, cap) == X87Ordering::Less {
            next
        } else {
            cap
        };
    }
    if ordering(target, current) == X87Ordering::Less {
        let braking = X87Chop53::mul(single(params.accel_bits), double(ONE_AND_A_HALF));
        let next = X87Chop53::sub(current, braking);
        current = if ordering(next, zero()) == X87Ordering::Greater {
            next
        } else {
            zero()
        };
    }
    flight.current_speed_bits = store_double(current);
    // A zero speed divides by zero natively (masked, infinity or NaN); no stock
    // jumpjet has one, and VERA reports a zero fraction instead.
    host.set_speed_fraction(X87Chop53::div(current, cap).map_or(0, store_double));

    let location = host.location();
    let same_cell = native_cell(destination[0]) == native_cell(location[0])
        && native_cell(destination[1]) == native_cell(location[1]);

    // Hover bob 0x0054D1FB..D23A, then the bobbed target height.
    let bob = if hold_or_translate && !params.no_wobbles {
        let step = X87Chop53::div(double(FIFTEEN), single(params.wobbles_bits))
            .and_then(|period| X87Chop53::div(double(TWO_PI), period))
            .unwrap_or_else(|_| zero());
        X87Chop53::add(step, double(flight.bob_phase_bits))
    } else {
        zero()
    };
    flight.bob_phase_bits = store_double(bob);
    let bob_target = X87Chop53::ftol_i32_low_masked(X87Chop53::add(
        X87Chop53::mul(host.trig().sin_from_table(bob), int(params.deviation)),
        int(flight.target_height),
    ));

    // Ground under the owner, raised to the deck once near it (0x0054D29A..D314).
    let z = location[2];
    let xy = [location[0], location[1]];
    let mut ground = host.floor_height(xy);
    if host.cell_high_bridge(xy) {
        let deck_approach = X87Chop53::sub(
            int(ground.wrapping_add(4 * GROUND_LEVEL_HEIGHT_LEPTONS)),
            single(params.crash_bits),
        );
        if ordering(deck_approach, int(z)) != X87Ordering::Greater {
            ground = ground.wrapping_add(BRIDGE_DECK_LEPTONS);
        }
    }
    let reference =
        if matches!(phase, STATE_DESCEND | STATE_GROUND) || (same_cell && !host.balloon_hover()) {
            ground
        } else {
            reference_height(flight, host, location)
        };

    // Height integration 0x0054D354..D4BB.
    let climb = single(params.climb_bits);
    let mut altitude = z.wrapping_sub(reference);
    let mut new_z = None;
    if altitude < bob_target {
        let mut height = host.height_above_ground();
        if host.cell_high_bridge(xy)
            && !host.on_bridge()
            && z >= host.floor_height(xy).wrapping_add(BRIDGE_DECK_LEPTONS)
        {
            height = height.wrapping_sub(BRIDGE_DECK_LEPTONS);
        }
        if height == 0 {
            host.grounded_reset();
        }
        let reach = X87Chop53::add(int(altitude), climb);
        new_z = Some(if ordering(int(bob_target), reach) == X87Ordering::Less {
            z.wrapping_add(bob_target.wrapping_sub(altitude))
        } else {
            X87Chop53::ftol_i32_low_masked(X87Chop53::add(int(z), climb))
        });
    } else if altitude > bob_target {
        let floor = X87Chop53::sub(int(altitude), climb);
        let mut next = if ordering(int(bob_target), floor) != X87Ordering::Greater {
            X87Chop53::ftol_i32_low_masked(X87Chop53::sub(int(z), climb))
        } else {
            z.wrapping_add(bob_target.wrapping_sub(altitude))
        };
        if next <= ground {
            next = ground;
        }
        if altitude <= 0 {
            altitude = 0;
        }
        new_z = Some(next);
    }

    // Too low outside the destination cell: no horizontal speed (0x0054D4FD..D52D).
    if !same_cell && (altitude < bob_target / 2 || altitude < bob_target / 4) {
        flight.current_speed_bits = 0;
    }
    if let Some(next) = new_z {
        host.set_z(next);
    }

    // Horizontal step along the locomotor facing (0x0054D55A..D607).
    let location = host.location();
    let frame = host.binary_frame();
    let step = int(X87Chop53::ftol_i32_low_masked(double(
        flight.current_speed_bits,
    )));
    let facing = flight.facing.current(frame) as i16;
    let angle = X87Chop53::mul(
        int(i32::from(facing) - 0x3FFF),
        double(NEG_RADIANS_PER_FACING_UNIT),
    );
    let new_y = X87Chop53::ftol_i32_low_masked(X87Chop53::sub(
        int(location[1]),
        X87Chop53::mul(host.trig().sin_from_table(angle), step),
    ));
    let new_x = X87Chop53::ftol_i32_low_masked(X87Chop53::add(
        X87Chop53::mul(host.trig().cos_from_table(angle), step),
        int(location[0]),
    ));
    host.set_location([new_x, new_y, location[2]]);

    // Body facing (0x0054D60D..D692).
    if !host.piggyback_active() {
        let hold_facing = (host.owner_kind() == FlightOwnerKind::Infantry
            && phase == STATE_HOLD
            && host.has_target())
        .then(|| host.hold_target_facing())
        .flatten();
        match hold_facing {
            Some(toward_target) => {
                flight.facing.snap(toward_target, frame);
            }
            None => host.snap_body_facing(flight.facing.current(frame)),
        }
    }
}

/// `State3_Translate @ 0x0054BFF0`. Returns the new state.
pub(crate) fn state3_translate(
    destination: [i32; 3],
    params: &JumpjetFlightParams,
    flight: &mut JumpjetFlight,
    host: &mut impl JumpjetFlightHost,
) -> i32 {
    let mut state = STATE_TRANSLATE;
    let frame = host.binary_frame();
    let location = host.location();

    // Desired facing toward the destination (0x0054C081..C0CD).
    flight
        .facing
        .set(desired_facing(destination, location, host), frame);
    // `FUN_004C9530` is destination minus animated current; the error byte
    // rounds the unsigned 16-bit difference to eighths of a byte, so any turn
    // to the left reads as a large error.
    let difference = flight
        .facing
        .destination()
        .wrapping_sub(flight.facing.current(frame));
    let turn_error = ((((u32::from(difference) >> 7) + 1) >> 1) & 0xFF) as i32;

    let distance = crate::sim::cell_kernel::native_xy_distance(
        location[0].wrapping_sub(destination[0]),
        location[1].wrapping_sub(destination[1]),
    );
    let unit_owner = host.owner_kind() == FlightOwnerKind::Unit;
    let has_target = host.has_target();

    if distance < ARRIVAL_RADIUS {
        flight.current_speed_bits = 0;
        flight.target_speed_bits = 0;
        host.set_location([destination[0], destination[1], location[2]]);
        // The arrival snaps the owner onto the destination, so the cell it
        // claims is the destination's.
        let here = host.cell_of([destination[0], destination[1]]);
        if host.piggyback_active() {
            flight.target_height = 0;
            host.piggyback_arrival();
            state = STATE_DESCEND;
        } else if !has_target && !host.balloon_hover() {
            if unit_owner && host.simple_deployer() && host.type_flag_6ad() {
                if claim_or_scatter(here, host) {
                    flight.target_height = params.height;
                    state = STATE_HOLD;
                }
            } else {
                flight.target_height = 0;
                state = STATE_DESCEND;
            }
            host.arrival_notify();
        } else if claim_or_scatter(here, host) {
            state = STATE_HOLD;
        }
    } else {
        let speed = params.speed;
        // `FCOMP 1.0` then store 1.0 when below (`0x0054C432`, `0x0054C4CC`).
        let at_least_one = |value: i32| {
            if ordering(int(value), double(ONE)) == X87Ordering::Less {
                ONE
            } else {
                store_double(int(value))
            }
        };
        let target_bits = if distance < speed {
            if !has_target {
                flight.target_height = params.height / 2;
            }
            store_double(int(speed / 8))
        } else if distance < speed.wrapping_mul(2) {
            if !has_target {
                flight.target_height = params.height / 2;
            }
            if turn_error > params.turn_rate {
                at_least_one(speed / 10)
            } else {
                store_double(int(speed / 4))
            }
        } else if speed
            .wrapping_mul(50)
            .checked_div(params.turn_rate)
            .is_some_and(|slow_radius| distance < slow_radius)
        {
            // A zero turn rate faults natively (`IDIV` at `0x0054C45E`); no
            // stock type reaches it, and VERA skips the zone instead.
            if !has_target {
                flight.target_height = X87Chop53::ftol_i32_low_masked(X87Chop53::mul(
                    int(params.height),
                    double(THREE_QUARTERS),
                ));
            }
            if turn_error > params.turn_rate.wrapping_mul(5) {
                at_least_one(speed / 5)
            } else {
                store_double(int(speed / 2))
            }
        } else {
            flight.target_height = params.height;
            store_double(int(speed))
        };
        flight.target_speed_bits = target_bits;
    }

    // Tail 0x0054C4FD..C544: hovering types, water and beach destinations and
    // flagged units keep full height.
    if host.balloon_hover()
        || matches!(host.cell_land_type([destination[0], destination[1]]), 2 | 6)
        || (unit_owner && host.type_flag_6ad())
    {
        flight.target_height = params.height;
    }
    state
}

/// `Is_Moving_Now @ 0x0054D0D0`: true for any state other than ground and
/// hold. `Process` gates Update on this or the moving byte, so an idle landed
/// (state 0) or idle holding (state 2) Jumpjet is advanced by nothing at all.
pub(crate) fn is_moving_now(state: i32) -> bool {
    state != STATE_GROUND && state != STATE_HOLD
}

/// One `Process @ 0x0054AEC0` frame: the Update gate, the crash latch, then the
/// state at `+0x50` through the jump table at `0x0054B19C`. Returns the new
/// state.
///
/// The gate comes first, so a crashing owner idle in the hold (state 2 with the
/// moving byte clear) never latches and hangs where it is, natively too.
///
/// Not modelled: `0x0053A130`'s constant-false arm and the two visibility
/// probes in the tail.
pub(crate) fn process(
    moving: bool,
    state: i32,
    destination: [i32; 3],
    params: &JumpjetFlightParams,
    flight: &mut JumpjetFlight,
    host: &mut impl JumpjetFlightHost,
) -> i32 {
    if !moving && !is_moving_now(state) {
        return state;
    }
    update_coordinates_and_altitude(state, destination, params, flight, host);
    match crash_latch(state, flight, host) {
        STATE_GROUND => state0_ground(moving, params, flight, host),
        STATE_ASCEND => state1_ascend(destination, params, flight, host),
        STATE_HOLD => state2_hold(moving, destination, params, flight, host),
        STATE_TRANSLATE => state3_translate(destination, params, flight, host),
        STATE_DESCEND => state4_descend(destination, params, flight, host),
        STATE_CRASH => state5_crash(params, flight, host),
        other => other,
    }
}

/// `Process`'s crash latch (`0x0054AF2E..0x0054B02C`), between Update and the
/// dispatch: a crashing owner above the ground, not already falling, enters
/// State 5 with the target height at -5.
///
/// Not ported: a Magnetron-held owner (`+0x6AD`) first has its destination
/// moved to its cell's centre and latches only once it stands there
/// (`0x0054AFE0..0x0054B004`), and an Infantry owner's latch plays sequence
/// `0x22` (vtable `+0x558`, `0x0054B02C`). VERA never raises `+0x6AD` (the
/// `IsLocomotor=` warhead is unported) and crashes no Infantry.
fn crash_latch(state: i32, flight: &mut JumpjetFlight, host: &impl JumpjetFlightHost) -> i32 {
    if !host.crashing()
        || matches!(state, STATE_CRASH | STATE_CRASHED)
        || host.height_above_ground() <= 0
    {
        return state;
    }
    flight.target_height = CRASH_TARGET_HEIGHT;
    STATE_CRASH
}

/// The desired facing toward `destination` (`0x0054C081..C0CD`, and the same
/// sequence in State 1 and State 2): the retail arctangent, less a quarter
/// turn, scaled by `-65536/2pi` and truncated.
fn desired_facing(destination: [i32; 3], location: [i32; 3], host: &impl JumpjetFlightHost) -> u16 {
    let angle = host.atan().atan2(
        X87Chop53::sub(int(location[1]), int(destination[1])),
        X87Chop53::sub(int(destination[0]), int(location[0])),
    );
    X87Chop53::ftol_i32_low_masked(X87Chop53::mul(
        X87Chop53::sub(angle, double(HALF_PI)),
        double(NEG_FACING_UNITS_PER_RADIAN),
    )) as u16
}

/// The packed adjacent-cell offsets at `0x0089F688` that
/// `MapCoord_StepByDir_GetCell @ 0x00481810` adds to a cell
/// (`[EDX*4 + 0x89F688]` at `0x0048182D`).
///
/// This is a *different* table from [`DIRECTION_DELTA`]: the CRT initializer
/// `0x0049F2F0` fills these eight 4-byte entries, while `0x0049F3A0` fills the
/// 8-byte lepton deltas. Their values agree — north first, clockwise — which
/// `0x0049F2F0` builds from `DX = 0`, `CX = -1` and `AX = 1`.
const ADJACENT_CELL_DELTA: [(i16, i16); 8] = [
    (0, -1),
    (1, -1),
    (1, 0),
    (1, 1),
    (0, 1),
    (-1, 1),
    (-1, 0),
    (-1, -1),
];

fn step_cell(cell: (i16, i16), direction: u32) -> (i16, i16) {
    let (dx, dy) = ADJACENT_CELL_DELTA[(direction & 7) as usize];
    (cell.0.wrapping_add(dx), cell.1.wrapping_add(dy))
}

/// The air-slot bookkeeping States 1, 3 and 4 share: claim the cell's slot, or
/// step to a random neighbour and re-target when another object holds it.
/// Answers whether the slot was claimed.
fn claim_or_scatter(cell: (i16, i16), host: &mut impl JumpjetFlightHost) -> bool {
    if host.air_slot_taken_at(cell) {
        let direction = host.random_direction();
        host.set_destination_cell(step_cell(cell, direction));
        false
    } else {
        host.claim_air_slot_at(cell);
        true
    }
}

/// `CellClass::GetSubCell @ 0x004810A0`: the infantry sub-cell a coordinate
/// falls in. Only 0, 2, 3 and 4 are produced — never 1.
fn sub_cell_of(coord: [i32; 3]) -> i32 {
    let x = coord[0] & 0xFF;
    let y = coord[1] & 0xFF;
    if crate::sim::cell_kernel::native_xy_distance(x - 0x80, y - 0x80) < 0x3C {
        return 0;
    }
    let quadrant = i32::from(x > 0x80) | (i32::from(y > 0x80) << 1);
    if quadrant == 0 { 0 } else { quadrant + 1 }
}

/// `State0_GroundIdle @ 0x0054B980`. Returns the new state.
///
/// The air-bucket add at `0x0054BA20` is gated on the owner's cell matching the
/// global cell at `0x00ABC588`, which this port does not model; no corpus row
/// reaches it.
pub(crate) fn state0_ground(
    moving: bool,
    params: &JumpjetFlightParams,
    flight: &mut JumpjetFlight,
    host: &mut impl JumpjetFlightHost,
) -> i32 {
    if !moving {
        return STATE_GROUND;
    }
    let frame = host.binary_frame();
    host.set_speed_fraction(ONE);
    // Takeoff keeps the body's heading, so the link-time 0x4000 never leaks.
    flight.facing.snap(host.body_facing(), frame);
    flight.current_speed_bits = 0;
    flight.target_speed_bits = 0;
    flight.target_height = params.height;
    // `0x0053A130` is the constant-false leaf, so its early return is dead.
    STATE_ASCEND
}

/// `State1_Ascend @ 0x0054BA30`.
pub(crate) fn state1_ascend(
    destination: [i32; 3],
    params: &JumpjetFlightParams,
    flight: &mut JumpjetFlight,
    host: &mut impl JumpjetFlightHost,
) -> i32 {
    let frame = host.binary_frame();
    let location = host.location();
    let here = host.cell_of([location[0], location[1]]);
    // The owner's own height, taken off the deck while it flies over a bridge.
    let mut height = host.height_above_ground();
    if !host.on_bridge() && host.cell_high_bridge_at(here) && height >= BRIDGE_DECK_LEPTONS {
        height = height.wrapping_sub(BRIDGE_DECK_LEPTONS);
    }

    if height < flight.target_height {
        // Climbing: a quarter of the way up releases the horizontal speed, and
        // a hovering Unit waits until half way (0x0054BB8D..).
        let gate = if host.owner_kind() == FlightOwnerKind::Unit && host.balloon_hover() {
            flight.target_height / 2
        } else {
            flight.target_height / 4
        };
        if gate < height {
            flight.target_speed_bits = store_double(int(params.speed));
            flight
                .facing
                .set(desired_facing(destination, location, host), frame);
            let dest_cell = host.cell_of([destination[0], destination[1]]);
            if here == dest_cell && !host.balloon_hover() {
                // Already over the destination: hold this height and translate.
                flight.target_height = host.height_above_ground();
                return STATE_TRANSLATE;
            }
        }
        return STATE_ASCEND;
    }

    // At cruise height the owner claims the cell's air slot, or scatters to a
    // random neighbour when another object already holds it.
    if claim_or_scatter(here, host) {
        STATE_HOLD
    } else {
        STATE_TRANSLATE
    }
}

/// `State2_HoldStation @ 0x0054BD30`.
pub(crate) fn state2_hold(
    moving: bool,
    destination: [i32; 3],
    params: &JumpjetFlightParams,
    flight: &mut JumpjetFlight,
    host: &mut impl JumpjetFlightHost,
) -> i32 {
    if !moving {
        return STATE_HOLD;
    }
    let frame = host.binary_frame();
    let location = host.location();
    let here = host.cell_of([location[0], location[1]]);

    // A destination elsewhere re-opens the cruise (0x0054BEE7).
    if destination[0] != location[0] || destination[1] != location[1] {
        flight
            .facing
            .set(desired_facing(destination, location, host), frame);
        // 0x0054BF60..0x0054BFBC: with the current cell's slot empty, the
        // owner's cached cell (`+0x560`) is released when it still holds the
        // owner; then the current cell is released unconditionally, even if
        // another object holds it. VERA has no cached cell, so it drops every
        // cell this owner still holds — which is what the cached cell names in
        // practice. Without it the claim orphans, because State 1 sets the
        // target speed before promoting and the owner drifts out of the cell it
        // just claimed, leaking into a hashed, snapshotted grid.
        if host.air_slot_empty_at(here) {
            host.release_owner_air_slots();
        }
        host.release_air_slot_at(here);
        return STATE_TRANSLATE;
    }

    // Arrived and holding: a target keeps the owner in place.
    if host.has_target() {
        return STATE_HOLD;
    }
    let deployer = host.owner_kind() == FlightOwnerKind::Unit
        && host.simple_deployer()
        && !host.piggyback_active();
    if deployer {
        // A Siege Chopper with `DeployToLand=` hovers at full height instead.
        if host.type_flag_6ad() && !host.deploy_latched() {
            flight.target_height = params.height;
            host.arrival_notify();
            return STATE_HOLD;
        }
    } else if host.balloon_hover() {
        host.arrival_notify();
        return STATE_HOLD;
    }
    if host.holds_air_slot_at(here) {
        host.release_air_slot_at(here);
    }
    host.arrival_notify();
    STATE_DESCEND
}

/// `State4_Descend @ 0x0054C550`.
pub(crate) fn state4_descend(
    destination: [i32; 3],
    params: &JumpjetFlightParams,
    flight: &mut JumpjetFlight,
    host: &mut impl JumpjetFlightHost,
) -> i32 {
    let frame = host.binary_frame();
    let location = host.location();
    let here = host.cell_of([location[0], location[1]]);
    let dest_cell = host.cell_of([destination[0], destination[1]]);

    if !host.piggyback_active() {
        let mission_seven = host.mission_is_seven();
        // Water and beach refuse a landing: hold over the cell instead.
        if matches!(host.cell_land_type([destination[0], destination[1]]), 2 | 6) && !mission_seven
        {
            if !claim_or_scatter(here, host) {
                return STATE_TRANSLATE;
            }
            flight.target_height = params.height;
            return STATE_HOLD;
        }

        // Landing admission 0x0054C65F..0x0054C736.
        let answer = host.can_enter_cell(dest_cell);
        let sub_cell = sub_cell_of(destination);
        let bridge = host.cell_high_bridge_at(dest_cell);
        // `IsSubCellFree 0x00481130` answers a definite false for sub-cells 0
        // and 1 (`XOR AL,AL` at `0x0048117A`). Native's Unit-on-sub-cell-0
        // override at `0x0054C6BE` exists precisely because of that, and an
        // Infantry owner on the cell centre is refused the first admission.
        let sub_free = sub_cell > 1 && host.sub_cell_free(dest_cell, sub_cell, bridge);
        let admitted = sub_free
            || host.landing_latched()
            || (host.owner_kind() == FlightOwnerKind::Unit && sub_cell == 0);
        if !mission_seven {
            let refused =
                !admitted || (!bridge && (answer > 2 || (answer == 2 && !host.landing_latched())));
            if refused {
                return host.stop_moving();
            }
            if !host.landing_latched() {
                host.begin_landing(destination);
            }
        }
    }

    // 0x0054C737: a simple deployer turns to the Rules deploy facing first.
    if host.owner_kind() == FlightOwnerKind::Unit
        && host.simple_deployer()
        && !host.piggyback_active()
        && let Some(deploy) = host.deploy_facing()
        && flight.facing.current(frame) != deploy
    {
        flight.facing.set(deploy, frame);
    }
    flight.target_height = 0;

    let mut height = host.height_above_ground();
    if !host.on_bridge() && host.cell_high_bridge_at(here) && height >= BRIDGE_DECK_LEPTONS {
        height = height.wrapping_sub(BRIDGE_DECK_LEPTONS);
    }
    if height != 0 {
        return STATE_DESCEND;
    }

    // Touchdown 0x0054C80D..0x0054CA7B.
    host.set_speed_fraction(0);
    if !host.piggyback_active() {
        host.set_location(destination);
    }
    host.touchdown();
    STATE_GROUND
}

/// `State5_Crash @ 0x0054CA90` for an owner no Magnetron holds. Returns the
/// new state.
///
/// The owner drops by `JumpjetCrash=` below where Update left it, truncated by
/// `Math::ftol` (`0x0054CAFE..0x0054CB0A`), and the facing is re-aimed
/// `0x2D00` past its animated current (`0x0054CB0E..0x0054CB39`). Only a cell
/// inside the map takes the drop (`0x0054CBC2`); outside it Update's descent
/// alone brings the owner down. The impact is the ground or a crossing of a
/// high bridge's deck from above (`0x0054CB3E..0x0054CB81`). It releases the
/// air slot the owner holds (`0x0054D045..0x0054D067`), zeroes the target
/// speed (`0x0054D07D`) and hands the owner its notice
/// ([`JumpjetFlightHost::crash_impact`]), leaving State 6.
///
/// Not ported: the Magnetron arms. An owner held by one (`+0x6AD`) instead
/// stops dead and falls ever faster (`0x0054CAD2..0x0054CAFC`), and one being
/// dropped by it (`+0x427`) lands on and crushes what is below
/// (`0x0054CC47..0x0054D012`); VERA raises neither byte.
pub(crate) fn state5_crash(
    params: &JumpjetFlightParams,
    flight: &mut JumpjetFlight,
    host: &mut impl JumpjetFlightHost,
) -> i32 {
    let frame = host.binary_frame();
    let [x, y, old_z] = host.location();
    let new_z =
        X87Chop53::ftol_i32_low_masked(X87Chop53::sub(int(old_z), single(params.crash_bits)));
    let spin = flight.facing.current(frame).wrapping_add(CRASH_SPIN);
    flight.facing.set(spin, frame);

    // `CMP EBP,ESI` / `CMP EAX,ESI`, both signed: from on or above the deck to
    // below it.
    let deck = host.floor_height([x, y]).wrapping_add(BRIDGE_DECK_LEPTONS);
    let crossed_deck = host.cell_high_bridge([x, y]) && old_z >= deck && new_z < deck;
    let here = host.cell_of([x, y]);
    if host.in_bounds(here) {
        host.crash_relocate([x, y, new_z]);
    }
    if host.height_above_ground() > 0 && !crossed_deck {
        return STATE_CRASH;
    }

    if host.holds_air_slot_at(here) {
        host.release_air_slot_at(here);
    }
    flight.target_speed_bits = 0;
    host.crash_impact();
    STATE_CRASHED
}

/// `CellClass @ 0x00485080`: ground at the cell centre, plus the first
/// building's `BuildingTypeClass::Dimension2` height (vtable `+0x7C`,
/// `0x00464AF0`) or, with no building, 85 leptons when
/// `Find_Nearest_Object @ 0x0047C3D0` finds any techno in the ground list.
pub(crate) fn cell_top_height(
    centre_ground: i32,
    first_building_height: Option<i32>,
    any_techno: bool,
) -> i32 {
    match first_building_height {
        Some(height) => centre_ground.wrapping_add(height),
        None if any_techno => centre_ground.wrapping_add(CELL_OBJECT_LIFT),
        None => centre_ground,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::map::retail_trig::{required_atan_table, required_math_tables};
    use crate::util::fixed_math::SimFixed;
    use serde_json::{Value, json};

    /// The oracle's map fixture: cell (10,10) is level 0 and flat, cell (9,10)
    /// is level 2 with slope 1 (the shared `Original` harness), and every other
    /// lookup is the zero-height dummy cell. No cell has a bridge, building or
    /// object, and the owner flags are clear.
    struct FixtureHost<'a> {
        frame: u32,
        trig: &'a TrigTable,
        atan: &'a AtanTable,
        kind: FlightOwnerKind,
        location: [i32; 3],
        balloon_hover: bool,
        has_target: bool,
        fractions: Vec<u64>,
        events: Vec<&'static str>,
        /// Cell top heights that replace the terrain sample (buildings, objects).
        tops: Vec<((i16, i16), i32)>,
        /// Cells carrying a high bridge.
        bridges: Vec<(i16, i16)>,
        piggyback: bool,
        simple_deployer: bool,
        deploy_to_land: bool,
        /// LandType of fixture cell (10,10).
        destination_land_type: u8,
        /// Owner body facing `+0x388`, written by Update's `UpdateFacing`.
        body_facing: u16,
        /// Cells whose `+0xE0` air slot another object already holds.
        taken_slots: Vec<(i16, i16)>,
        /// The cell whose air slot this owner holds.
        held_slot: Option<(i16, i16)>,
        /// The direction `RandomRanged(0, 7)` answers for a scatter.
        scatter_direction: u32,
        /// The `Can_Enter_Cell` answer for the landing cell.
        can_enter: i32,
        /// Locomotor `+0x90`, the landing-admitted latch.
        landing_latched: bool,
    }

    impl FixtureHost<'_> {
        fn cell_terrain(xy: [i32; 2]) -> (u8, u8) {
            if (native_cell(xy[0]), native_cell(xy[1])) == (9, 10) {
                (2, 1)
            } else {
                (0, 0)
            }
        }
    }

    impl JumpjetFlightHost for FixtureHost<'_> {
        fn binary_frame(&self) -> u32 {
            self.frame
        }
        fn trig(&self) -> &TrigTable {
            self.trig
        }
        fn atan(&self) -> &AtanTable {
            self.atan
        }
        fn owner_kind(&self) -> FlightOwnerKind {
            self.kind
        }
        fn location(&self) -> [i32; 3] {
            self.location
        }
        fn set_location(&mut self, coord: [i32; 3]) {
            self.location = coord;
        }
        fn set_z(&mut self, z: i32) {
            self.location[2] = z;
        }
        fn height_above_ground(&self) -> i32 {
            self.location[2] - self.floor_height([self.location[0], self.location[1]])
        }
        fn on_bridge(&self) -> bool {
            false
        }
        fn grounded_reset(&mut self) {
            self.events.push("grounded_reset");
        }
        fn floor_height(&self, xy: [i32; 2]) -> i32 {
            let (level, slope) = Self::cell_terrain(xy);
            crate::util::lepton::ground_height_leptons(level, slope, xy[0], xy[1])
                .expect("fixture slope is supported")
        }
        fn cell_high_bridge(&self, xy: [i32; 2]) -> bool {
            self.bridges
                .contains(&(native_cell(xy[0]), native_cell(xy[1])))
        }
        fn cell_top_height(&self, xy: [i32; 2]) -> i32 {
            let cell = (native_cell(xy[0]), native_cell(xy[1]));
            if let Some(&(_, top)) = self.tops.iter().find(|(key, _)| *key == cell) {
                return top;
            }
            let (level, slope) = Self::cell_terrain(xy);
            let centre = crate::util::lepton::ground_height_leptons(level, slope, 128, 128)
                .expect("fixture slope is supported");
            cell_top_height(centre, None, false)
        }
        fn cell_land_type(&self, xy: [i32; 2]) -> u8 {
            if (native_cell(xy[0]), native_cell(xy[1])) == (10, 10) {
                self.destination_land_type
            } else {
                0
            }
        }
        fn balloon_hover(&self) -> bool {
            self.balloon_hover
        }
        fn has_target(&self) -> bool {
            self.has_target
        }
        fn piggyback_active(&self) -> bool {
            self.piggyback
        }
        fn piggyback_arrival(&mut self) {}
        fn simple_deployer(&self) -> bool {
            self.simple_deployer
        }
        fn type_flag_6ad(&self) -> bool {
            self.deploy_to_land
        }
        fn set_speed_fraction(&mut self, fraction_bits: u64) {
            self.fractions.push(fraction_bits);
        }
        fn arrival_notify(&mut self) {
            self.events.push("mission_notify");
        }
        fn snap_body_facing(&mut self, facing: u16) {
            self.body_facing = facing;
        }
        fn hold_target_facing(&self) -> Option<u16> {
            None
        }
        fn cell_of(&self, xy: [i32; 2]) -> (i16, i16) {
            (native_cell(xy[0]), native_cell(xy[1]))
        }
        fn body_facing(&self) -> u16 {
            self.body_facing
        }
        fn random_direction(&mut self) -> u32 {
            self.scatter_direction
        }
        fn air_slot_taken_at(&mut self, cell: (i16, i16)) -> bool {
            self.events.push("slot_query");
            self.taken_slots.contains(&cell)
        }
        fn holds_air_slot_at(&self, cell: (i16, i16)) -> bool {
            self.held_slot == Some(cell)
        }
        fn air_slot_empty_at(&self, cell: (i16, i16)) -> bool {
            self.held_slot != Some(cell) && !self.taken_slots.contains(&cell)
        }
        fn release_owner_air_slots(&mut self) {
            if self.held_slot.take().is_some() {
                self.events.push("slot_release");
            }
        }
        fn claim_air_slot_at(&mut self, cell: (i16, i16)) {
            self.events.push("slot_claim");
            self.held_slot = Some(cell);
        }
        fn release_air_slot_at(&mut self, cell: (i16, i16)) {
            self.events.push("slot_release");
            if self.held_slot == Some(cell) {
                self.held_slot = None;
            }
        }
        fn set_destination_cell(&mut self, _cell: (i16, i16)) {
            self.events.push("set_destination");
        }
        fn can_enter_cell(&self, _cell: (i16, i16)) -> i32 {
            self.can_enter
        }
        fn sub_cell_free(&self, _cell: (i16, i16), _sub_cell: i32, _bridge: bool) -> bool {
            true
        }
        fn mission_is_seven(&self) -> bool {
            false
        }
        fn stop_moving(&mut self) -> i32 {
            self.events.push("stop_moving");
            STATE_ASCEND
        }
        fn cell_high_bridge_at(&self, cell: (i16, i16)) -> bool {
            self.bridges.contains(&cell)
        }
        fn landing_latched(&self) -> bool {
            self.landing_latched
        }
        fn begin_landing(&mut self, _destination: [i32; 3]) {
            self.events.push("begin_landing");
            self.landing_latched = true;
        }
        fn deploy_latched(&self) -> bool {
            false
        }
        fn deploy_facing(&self) -> Option<u16> {
            None
        }
        fn touchdown(&mut self) {
            self.events.push("touchdown");
            self.held_slot = None;
        }
        fn crashing(&self) -> bool {
            false
        }
        fn in_bounds(&self, _cell: (i16, i16)) -> bool {
            true
        }
        fn crash_relocate(&mut self, coord: [i32; 3]) {
            self.events.push("crash_relocate");
            self.location = coord;
        }
        fn crash_impact(&mut self) {
            self.events.push("crash_impact");
        }
    }

    fn int(value: &Value) -> i64 {
        value.as_i64().expect("integer field")
    }

    /// Parity with `tools/spatial_oracle/jumpjet_flight.json`: native Update
    /// `0x0054D0F0` then State3 `0x0054BFF0` per frame, every field bit-exact.
    #[test]
    fn flight_matches_the_native_update_and_translate_corpus() {
        let (trig, _) = required_math_tables();
        let atan = required_atan_table();
        if !trig.matches_retail() || !atan.matches_retail() {
            // With RA2_DIR set, a mismatched table is a failure, not a skip.
            assert!(
                std::env::var_os("RA2_DIR").is_none(),
                "RA2_DIR is set but the retail sine or atan table does not match"
            );
            eprintln!("skipped: set RA2_DIR to the retail install to run this");
            return;
        }
        let rows: Value = serde_json::from_str(include_str!(
            "../../../tools/spatial_oracle/jumpjet_flight.json"
        ))
        .expect("corpus parses");
        let rows = rows.as_array().expect("row list");
        assert_eq!(rows.len(), 14);
        for row in rows {
            let name = row["name"].as_str().expect("row name");
            let input = &row["input"];
            let float = |key: &str| input[key].as_f64().expect("float field") as f32;
            let type_params = JumpjetParams {
                turn_rate: int(&input["turn_rate"]) as i32,
                speed: SimFixed::from_num(int(&input["speed"])),
                climb: float("climb"),
                crash: float("crash"),
                height: int(&input["height"]) as i32,
                accel: float("accel"),
                wobbles: float("wobbles"),
                deviation: int(&input["deviation"]) as i32,
                no_wobbles: input["no_wobbles"].as_bool().expect("flag"),
            };
            let params = JumpjetFlightParams::link(&type_params);
            let first_frame = int(&input["first_frame"]) as u32;
            let mut flight = JumpjetFlight::linked(&params);
            flight
                .facing
                .snap(int(&input["facing"]) as u16, first_frame);
            flight.target_height = int(&input["target_height"]) as i32;
            let start = &input["start"];
            let destination = [
                int(&input["destination"][0]) as i32,
                int(&input["destination"][1]) as i32,
                0,
            ];
            let mut host = FixtureHost {
                frame: first_frame,
                trig,
                atan,
                kind: if int(&input["rtti"]) == 15 {
                    FlightOwnerKind::Infantry
                } else {
                    FlightOwnerKind::Unit
                },
                location: [
                    int(&start[0]) as i32,
                    int(&start[1]) as i32,
                    int(&start[2]) as i32,
                ],
                balloon_hover: input["balloon_hover"].as_bool().expect("flag"),
                has_target: input["tarcom"].as_bool().expect("flag"),
                fractions: Vec::new(),
                events: Vec::new(),
                tops: Vec::new(),
                bridges: Vec::new(),
                piggyback: input["piggyback"].as_bool().expect("flag"),
                simple_deployer: input["simple_deployer"].as_bool().expect("flag"),
                deploy_to_land: input["deploy_to_land"].as_bool().expect("flag"),
                destination_land_type: int(&input["destination_land_type"]) as u8,
                body_facing: 0,
                taken_slots: Vec::new(),
                held_slot: None,
                scatter_direction: 0,
                can_enter: 0,
                landing_latched: false,
            };
            let expected = row["output"]["frames"].as_array().expect("frames");
            let mut phase = STATE_TRANSLATE;
            let mut produced = 0;
            for _ in 0..int(&input["max_frames"]) {
                host.fractions.clear();
                host.events.clear();
                host.frame += 1;
                update_coordinates_and_altitude(
                    phase,
                    destination,
                    &params,
                    &mut flight,
                    &mut host,
                );
                if phase == STATE_TRANSLATE {
                    phase = state3_translate(destination, &params, &mut flight, &mut host);
                }
                let frame = json!({
                    "coord": host.location,
                    "current_speed": flight.current_speed_bits,
                    "target_speed": flight.target_speed_bits,
                    "target_height": flight.target_height,
                    "bob_phase": flight.bob_phase_bits,
                    "phase": phase,
                    "facing_current": flight.facing.current(host.frame),
                    "facing_destination": flight.facing.destination(),
                    "body_facing": host.body_facing,
                    "speed_fractions": host.fractions,
                    "events": host.events,
                });
                assert_eq!(
                    Some(&frame),
                    expected.get(produced),
                    "{name}: frame {produced} differs"
                );
                produced += 1;
                if phase != STATE_TRANSLATE {
                    break;
                }
            }
            assert_eq!(produced, expected.len(), "{name}: frame count");
        }
    }

    /// The states corpus fixture: the declared cell block x=6..20 by y=9..11,
    /// flat except where a row names a level and slope on the flight line y=10.
    /// Everything outside the block is the zero-height dummy cell.
    struct StatesHost<'a> {
        frame: u32,
        trig: &'a TrigTable,
        atan: &'a AtanTable,
        kind: FlightOwnerKind,
        location: [i32; 3],
        balloon_hover: bool,
        has_target: bool,
        piggyback: bool,
        simple_deployer: bool,
        deploy_to_land: bool,
        body_facing: u16,
        /// `x -> (level, slope)` on the flight line.
        terrain: Vec<(i16, (u8, u8))>,
        /// `x -> LandType` on the flight line.
        land_types: Vec<(i16, u8)>,
        /// `x -> Can_Enter_Cell answer` on the flight line.
        can_enter: Vec<(i16, i32)>,
        /// The AltObject slots, owner id per cell.
        slots: Vec<((i16, i16), u64)>,
        owner: u64,
        /// The `RandomRanged(0, 7)` draw, supplied from the recorded neighbour.
        scatter_direction: u32,
        scatter_to: Option<(i16, i16)>,
        /// The state `Stop_Moving` answers, with the destination it re-targets.
        stop_state: i32,
        stop_requested: bool,
        landing_latched: bool,
        touched_down: bool,
        /// The air-slot calls this frame, in the corpus's own vocabulary.
        slot_events: Vec<Value>,
        /// Owner `+0x425`.
        crashing: bool,
        /// `MapClass+0xF4/+0xF8`, the `In_Bounds` diamond's width and height.
        map_size: (i32, i32),
        /// State 5's owner calls this frame, in the corpus's vocabulary: the
        /// relocation's display removal, the AircraftTracker removal and the
        /// `(0x117C, 0)` notice.
        impact_events: Vec<Value>,
    }

    impl StatesHost<'_> {
        fn cell_terrain(&self, cell: (i16, i16)) -> (u8, u8) {
            if cell.1 != 10 {
                return (0, 0);
            }
            self.terrain
                .iter()
                .find(|(x, _)| *x == cell.0)
                .map_or((0, 0), |(_, levels)| *levels)
        }
    }

    impl JumpjetFlightHost for StatesHost<'_> {
        fn binary_frame(&self) -> u32 {
            self.frame
        }
        fn trig(&self) -> &TrigTable {
            self.trig
        }
        fn atan(&self) -> &AtanTable {
            self.atan
        }
        fn owner_kind(&self) -> FlightOwnerKind {
            self.kind
        }
        fn location(&self) -> [i32; 3] {
            self.location
        }
        fn set_location(&mut self, coord: [i32; 3]) {
            self.location = coord;
        }
        fn set_z(&mut self, z: i32) {
            self.location[2] = z;
        }
        fn height_above_ground(&self) -> i32 {
            self.location[2] - self.floor_height([self.location[0], self.location[1]])
        }
        fn on_bridge(&self) -> bool {
            false
        }
        fn grounded_reset(&mut self) {}
        fn floor_height(&self, xy: [i32; 2]) -> i32 {
            let (level, slope) = self.cell_terrain((native_cell(xy[0]), native_cell(xy[1])));
            crate::util::lepton::ground_height_leptons(level, slope, xy[0], xy[1])
                .expect("fixture slope is supported")
        }
        fn cell_high_bridge(&self, _xy: [i32; 2]) -> bool {
            false
        }
        fn cell_top_height(&self, xy: [i32; 2]) -> i32 {
            let (level, slope) = self.cell_terrain((native_cell(xy[0]), native_cell(xy[1])));
            let centre = crate::util::lepton::ground_height_leptons(level, slope, 128, 128)
                .expect("fixture slope is supported");
            cell_top_height(centre, None, false)
        }
        fn cell_land_type(&self, xy: [i32; 2]) -> u8 {
            let cell = (native_cell(xy[0]), native_cell(xy[1]));
            if cell.1 != 10 {
                return 0;
            }
            self.land_types
                .iter()
                .find(|(x, _)| *x == cell.0)
                .map_or(0, |(_, land)| *land)
        }
        fn balloon_hover(&self) -> bool {
            self.balloon_hover
        }
        fn has_target(&self) -> bool {
            self.has_target
        }
        fn piggyback_active(&self) -> bool {
            self.piggyback
        }
        fn piggyback_arrival(&mut self) {}
        fn simple_deployer(&self) -> bool {
            self.simple_deployer
        }
        fn type_flag_6ad(&self) -> bool {
            self.deploy_to_land
        }
        fn set_speed_fraction(&mut self, _fraction_bits: u64) {}
        fn arrival_notify(&mut self) {}
        fn snap_body_facing(&mut self, facing: u16) {
            self.body_facing = facing;
        }
        fn hold_target_facing(&self) -> Option<u16> {
            None
        }
        fn cell_of(&self, xy: [i32; 2]) -> (i16, i16) {
            (native_cell(xy[0]), native_cell(xy[1]))
        }
        fn body_facing(&self) -> u16 {
            self.body_facing
        }
        fn random_direction(&mut self) -> u32 {
            self.scatter_direction
        }
        fn air_slot_taken_at(&mut self, cell: (i16, i16)) -> bool {
            let taken = self
                .slots
                .iter()
                .any(|(at, held)| *at == cell && *held != self.owner);
            self.slot_events
                .push(json!(["query", cell.0, cell.1, taken]));
            taken
        }
        fn holds_air_slot_at(&self, cell: (i16, i16)) -> bool {
            self.slots
                .iter()
                .any(|(at, held)| *at == cell && *held == self.owner)
        }
        fn air_slot_empty_at(&self, cell: (i16, i16)) -> bool {
            !self.slots.iter().any(|(at, _)| *at == cell)
        }
        fn release_owner_air_slots(&mut self) {
            // Native releases the owner's *cached* cell (`+0x560`) here. The
            // corpus declares that `+0x560` keeps its supplied value, because
            // its only writers are the no-op air-bucket helpers — so the cached
            // cell never holds the owner and the original skips this release
            // entirely. Reproducing the fixture means doing nothing.
            //
            // Production has no cached cell and instead drops every slot the
            // owner holds, which is what stops a claim orphaning. The corpus is
            // structurally blind to that, so it is covered by a Rust regression
            // test (`re_opening_a_cruise_drops_a_drifted_slot`) rather than by
            // this parity comparison.
        }
        fn claim_air_slot_at(&mut self, cell: (i16, i16)) {
            self.slot_events.push(json!(["claim", cell.0, cell.1]));
            if !self.slots.iter().any(|(at, _)| *at == cell) {
                self.slots.push((cell, self.owner));
            }
        }
        fn release_air_slot_at(&mut self, cell: (i16, i16)) {
            self.slot_events.push(json!(["release", cell.0, cell.1]));
            self.slots.retain(|(at, _)| *at != cell);
        }
        fn set_destination_cell(&mut self, cell: (i16, i16)) {
            self.scatter_to = Some(cell);
        }
        fn can_enter_cell(&self, cell: (i16, i16)) -> i32 {
            if cell.1 != 10 {
                return 0;
            }
            self.can_enter
                .iter()
                .find(|(x, _)| *x == cell.0)
                .map_or(0, |(_, answer)| *answer)
        }
        fn sub_cell_free(&self, _cell: (i16, i16), _sub_cell: i32, _bridge: bool) -> bool {
            true
        }
        fn mission_is_seven(&self) -> bool {
            false
        }
        fn stop_moving(&mut self) -> i32 {
            self.stop_requested = true;
            self.stop_state
        }
        fn cell_high_bridge_at(&self, _cell: (i16, i16)) -> bool {
            false
        }
        fn landing_latched(&self) -> bool {
            self.landing_latched
        }
        fn begin_landing(&mut self, _destination: [i32; 3]) {
            self.landing_latched = true;
        }
        fn deploy_latched(&self) -> bool {
            false
        }
        fn deploy_facing(&self) -> Option<u16> {
            None
        }
        fn touchdown(&mut self) {
            self.touched_down = true;
            // 0x0054C9B2..0x0054C9CE: the touchdown release is conditional —
            // `CMP [cell+0xE0], owner` skips it unless the owner still holds
            // that slot, which it does not after State 2 re-opened the cruise.
            let here = self.cell_of([self.location[0], self.location[1]]);
            if self.holds_air_slot_at(here) {
                self.release_air_slot_at(here);
            }
            self.landing_latched = false;
        }
        fn crashing(&self) -> bool {
            self.crashing
        }
        fn in_bounds(&self, cell: (i16, i16)) -> bool {
            crate::map::playfield::size_diamond_contains(self.map_size.0, self.map_size.1, cell)
        }
        fn crash_relocate(&mut self, coord: [i32; 3]) {
            self.impact_events.push(json!("layer_remove"));
            self.location = coord;
        }
        fn crash_impact(&mut self) {
            self.impact_events.push(json!("bucket_remove"));
            self.impact_events.push(json!([0x117C, 0]));
        }
    }

    /// Parity with `tools/spatial_oracle/jumpjet_states.json`: the native
    /// `Process 0x0054AEC0` gate and state dispatch, frame by frame.
    ///
    /// Two values are supplied inputs rather than predictions, exactly as the
    /// oracle supplies them: the `RandomRanged(0, 7)` scatter draw (recovered
    /// from the neighbour the corpus recorded) and the cell `Stop_Moving`
    /// re-targets through FNPC.
    #[test]
    fn states_match_the_native_process_corpus() {
        let (trig, _) = required_math_tables();
        let atan = required_atan_table();
        if !trig.matches_retail() || !atan.matches_retail() {
            assert!(
                std::env::var_os("RA2_DIR").is_none(),
                "RA2_DIR is set but the retail sine or atan table does not match"
            );
            eprintln!("skipped: set RA2_DIR to the retail install to run this");
            return;
        }
        let rows: Value = serde_json::from_str(include_str!(
            "../../../tools/spatial_oracle/jumpjet_states.json"
        ))
        .expect("corpus parses");
        let rows = rows.as_array().expect("row list");
        assert_eq!(rows.len(), 10);

        for row in rows {
            let name = row["name"].as_str().expect("row name");
            let input = &row["input"];
            let float = |key: &str| input[key].as_f64().expect("float field") as f32;
            let params = JumpjetFlightParams::link(&JumpjetParams {
                turn_rate: int(&input["turn_rate"]) as i32,
                speed: SimFixed::from_num(int(&input["speed"])),
                climb: float("climb"),
                crash: float("crash"),
                height: int(&input["height"]) as i32,
                accel: float("accel"),
                wobbles: float("wobbles"),
                deviation: int(&input["deviation"]) as i32,
                no_wobbles: input["no_wobbles"].as_bool().expect("flag"),
            });
            let first_frame = int(&input["first_frame"]) as u32;
            let mut flight = JumpjetFlight::linked(&params);
            flight
                .facing
                .snap(int(&input["facing"]) as u16, first_frame);
            flight.target_height = int(&input["target_height"]) as i32;

            let list = |key: &str| -> Vec<Vec<i64>> {
                input[key]
                    .as_array()
                    .expect("list field")
                    .iter()
                    .map(|entry| entry.as_array().expect("pair").iter().map(int).collect())
                    .collect()
            };
            let frames = row["output"]["frames"].as_array().expect("frames");
            // The neighbour a scatter chose tells us which draw the original
            // made; solve for it once so the step table itself is under test.
            let scatter_direction = frames
                .iter()
                .find_map(|frame| {
                    let target = frame["events"].as_array()?.iter().find_map(|event| {
                        let event = event.as_array()?;
                        (event.first()?.as_str()? == "set_destination")
                            .then(|| event.get(1)?.as_array())
                            .flatten()
                    })?;
                    let from = frame["cell"].as_array()?;
                    let from = (int(&from[0]) as i16, int(&from[1]) as i16);
                    let target = (int(&target[0]) as i16, int(&target[1]) as i16);
                    (0..8u32).find(|d| step_cell(from, *d) == target)
                })
                .unwrap_or(0);

            let mut host = StatesHost {
                frame: first_frame,
                trig,
                atan,
                kind: if int(&input["rtti"]) == 15 {
                    FlightOwnerKind::Infantry
                } else {
                    FlightOwnerKind::Unit
                },
                location: [
                    int(&input["start"][0]) as i32,
                    int(&input["start"][1]) as i32,
                    int(&input["start"][2]) as i32,
                ],
                balloon_hover: input["balloon_hover"].as_bool().expect("flag"),
                has_target: input["tarcom"].as_bool().expect("flag"),
                piggyback: input["piggyback"].as_bool().expect("flag"),
                simple_deployer: input["simple_deployer"].as_bool().expect("flag"),
                deploy_to_land: input["deploy_to_land"].as_bool().expect("flag"),
                body_facing: 0,
                terrain: list("terrain")
                    .into_iter()
                    .map(|row| (row[0] as i16, (row[1] as u8, row[2] as u8)))
                    .collect(),
                land_types: list("land_types")
                    .into_iter()
                    .map(|row| (row[0] as i16, row[1] as u8))
                    .collect(),
                can_enter: input["can_enter_cells"]
                    .as_object()
                    .expect("map")
                    .iter()
                    .map(|(cell, answer)| {
                        (
                            cell.parse::<i16>().expect("cell key"),
                            answer.as_i64().expect("answer") as i32,
                        )
                    })
                    .collect(),
                slots: input["occupied_slots"]
                    .as_array()
                    .expect("list")
                    .iter()
                    .map(|x| ((int(x) as i16, 10), 2))
                    .collect(),
                owner: 1,
                scatter_direction,
                scatter_to: None,
                stop_state: STATE_ASCEND,
                stop_requested: false,
                landing_latched: false,
                touched_down: false,
                slot_events: Vec::new(),
                crashing: false,
                map_size: (0, 0),
                impact_events: Vec::new(),
            };

            // `Move_To` ran before the first Process frame; take what it left
            // behind, which is what the corpus records on that labelled frame.
            let seeded = frames[0]["label"] == "move_to";
            let (mut state, mut moving, mut destination) = if seeded {
                let seed = &frames[0];
                (
                    int(&seed["phase"]) as i32,
                    seed["moving"].as_bool().expect("flag"),
                    [
                        int(&seed["destination"][0]) as i32,
                        int(&seed["destination"][1]) as i32,
                        int(&seed["destination"][2]) as i32,
                    ],
                )
            } else {
                (
                    int(&input["phase"]) as i32,
                    input["moving"].as_bool().expect("flag"),
                    [0, 0, 0],
                )
            };
            let fnpc: Vec<Option<(i32, i32)>> = input["fnpc_cells"]
                .as_array()
                .map(|answers| {
                    answers
                        .iter()
                        .map(|answer| {
                            answer
                                .as_array()
                                .map(|cell| (int(&cell[0]) as i32, int(&cell[1]) as i32))
                        })
                        .collect()
                })
                .unwrap_or_default();
            let mut fnpc_index = 1usize;

            for (index, expected) in frames.iter().enumerate().skip(usize::from(seeded)) {
                host.frame += 1;
                host.scatter_to = None;
                host.slot_events.clear();
                state = process(moving, state, destination, &params, &mut flight, &mut host);
                if host.touched_down {
                    host.touched_down = false;
                    moving = false;
                    destination = [0, 0, 0];
                }
                if host.stop_requested {
                    host.stop_requested = false;
                    // `Stop_Moving` re-runs `Move_To` on the cell its search
                    // answered, which the corpus declares per row.
                    if let Some(Some(cell)) = fnpc.get(fnpc_index.min(fnpc.len() - 1)) {
                        destination = [cell.0 * 256 + 128, cell.1 * 256 + 128, 0];
                        moving = true;
                        // Move_To restores `JumpjetHeight=` when it lifts a
                        // descent back into the climb, undoing State 4's zero.
                        flight.target_height = params.height;
                    }
                    fnpc_index += 1;
                }
                let produced = json!({
                    "coord": host.location,
                    "current_speed": flight.current_speed_bits,
                    "target_speed": flight.target_speed_bits,
                    "target_height": flight.target_height,
                    "bob_phase": flight.bob_phase_bits,
                    "phase": state,
                    "moving": moving,
                    "facing_current": flight.facing.current(host.frame),
                    "facing_destination": flight.facing.destination(),
                    "body_facing": host.body_facing,
                    // The air slot is the one genuinely new mechanism here, so
                    // its native call sequence is compared, not just kinematics.
                    "slot_events": host.slot_events,
                });
                let native = json!({
                    "coord": expected["coord"],
                    "current_speed": expected["current_speed"],
                    "target_speed": expected["target_speed"],
                    "target_height": expected["target_height"],
                    "bob_phase": expected["bob_phase"],
                    "phase": expected["phase"],
                    "moving": expected["moving"],
                    "facing_current": expected["facing_current"],
                    "facing_destination": expected["facing_destination"],
                    "body_facing": expected["body_facing"],
                    "slot_events": expected["slot_events"],
                });
                assert_eq!(produced, native, "{name}: frame {index} differs");
            }
        }
    }

    /// Parity with `tools/spatial_oracle/jumpjet_crash.json`: from the state
    /// the kill leaves (Health 0 and two `Stop_Moving` calls around the
    /// `+0x425` latch), the native `Process` latch and State 5 `0x0054CA90`
    /// frame by frame to the impact notice, for every stock Crashable Unit
    /// type, an idle hover the latch never reaches and a fall outside
    /// `In_Bounds`.
    #[test]
    fn a_crashing_jumpjet_falls_like_the_native_state5() {
        let (trig, _) = required_math_tables();
        let atan = required_atan_table();
        if !trig.matches_retail() || !atan.matches_retail() {
            assert!(
                std::env::var_os("RA2_DIR").is_none(),
                "RA2_DIR is set but the retail sine or atan table does not match"
            );
            eprintln!("skipped: set RA2_DIR to the retail install to run this");
            return;
        }
        let corpus: Value = serde_json::from_str(include_str!(
            "../../../tools/spatial_oracle/jumpjet_crash.json"
        ))
        .expect("corpus parses");
        let rows = corpus["fall"].as_array().expect("fall rows");
        assert_eq!(rows.len(), 7);

        for row in rows {
            let name = row["name"].as_str().expect("row name");
            let input = &row["input"];
            let float = |key: &str| input[key].as_f64().expect("float field") as f32;
            let params = JumpjetFlightParams::link(&JumpjetParams {
                turn_rate: int(&input["turn_rate"]) as i32,
                speed: SimFixed::from_num(int(&input["speed"])),
                climb: float("climb"),
                crash: float("crash"),
                height: int(&input["height"]) as i32,
                accel: float("accel"),
                wobbles: float("wobbles"),
                deviation: int(&input["deviation"]) as i32,
                no_wobbles: input["no_wobbles"].as_bool().expect("flag"),
            });
            let killed = &row["output"]["killed"];
            let bits = |value: &Value| value.as_u64().expect("bit field");
            let triple = |value: &Value| [0, 1, 2].map(|i| int(&value[i]) as i32);
            // Every row's kill leaves the facing at rest, so a snap reproduces
            // its animated value from any frame.
            assert_eq!(
                killed["facing_current"], killed["facing_destination"],
                "{name}"
            );
            let first_frame = 1000;
            let mut flight = JumpjetFlight::linked(&params);
            flight
                .facing
                .snap(int(&killed["facing_current"]) as u16, first_frame);
            flight.current_speed_bits = bits(&killed["current_speed"]);
            flight.target_speed_bits = bits(&killed["target_speed"]);
            flight.target_height = int(&killed["target_height"]) as i32;
            flight.bob_phase_bits = bits(&killed["bob_phase"]);
            let cell = (
                int(&killed["cell"][0]) as i16,
                int(&killed["cell"][1]) as i16,
            );
            let map_size = if input["kill_map_size"].is_null() {
                &input["map_size"]
            } else {
                &input["kill_map_size"]
            };
            let mut host = StatesHost {
                frame: first_frame,
                trig,
                atan,
                kind: FlightOwnerKind::Unit,
                location: triple(&killed["coord"]),
                balloon_hover: input["balloon_hover"].as_bool().expect("flag"),
                has_target: input["tarcom"].as_bool().expect("flag"),
                piggyback: false,
                simple_deployer: false,
                deploy_to_land: false,
                body_facing: int(&killed["body_facing"]) as u16,
                terrain: Vec::new(),
                land_types: Vec::new(),
                can_enter: Vec::new(),
                slots: if bits(&killed["slot_holder"]) == 0 {
                    Vec::new()
                } else {
                    vec![(cell, 1)]
                },
                owner: 1,
                scatter_direction: 0,
                scatter_to: None,
                stop_state: STATE_ASCEND,
                stop_requested: false,
                landing_latched: false,
                touched_down: false,
                slot_events: Vec::new(),
                crashing: true,
                map_size: (int(&map_size[0]) as i32, int(&map_size[1]) as i32),
                impact_events: Vec::new(),
            };
            let mut state = int(&killed["phase"]) as i32;
            let moving = killed["moving"].as_bool().expect("flag");
            let destination = triple(&killed["destination"]);

            let frames = row["output"]["frames"].as_array().expect("frames");
            for (index, expected) in frames.iter().enumerate() {
                host.frame += 1;
                host.slot_events.clear();
                host.impact_events.clear();
                state = process(moving, state, destination, &params, &mut flight, &mut host);
                let produced = json!({
                    "coord": host.location,
                    "current_speed": flight.current_speed_bits,
                    "target_speed": flight.target_speed_bits,
                    "target_height": flight.target_height,
                    "bob_phase": flight.bob_phase_bits,
                    "phase": state,
                    "facing_current": flight.facing.current(host.frame),
                    "facing_destination": flight.facing.destination(),
                    "body_facing": host.body_facing,
                    "slot_events": host.slot_events,
                    "impact": host.impact_events,
                });
                let native_impact: Vec<Value> = expected["events"]
                    .as_array()
                    .expect("events")
                    .iter()
                    .filter(|event| {
                        matches!(event.as_str(), Some("layer_remove" | "bucket_remove"))
                    })
                    .chain(expected["notices"].as_array().expect("notices"))
                    .cloned()
                    .collect();
                let native = json!({
                    "coord": expected["coord"],
                    "current_speed": expected["current_speed"],
                    "target_speed": expected["target_speed"],
                    "target_height": expected["target_height"],
                    "bob_phase": expected["bob_phase"],
                    "phase": expected["phase"],
                    "facing_current": expected["facing_current"],
                    "facing_destination": expected["facing_destination"],
                    "body_facing": expected["body_facing"],
                    "slot_events": expected["slot_events"],
                    "impact": native_impact,
                });
                assert_eq!(produced, native, "{name}: frame {index} differs");
            }
            let impacts = frames
                .iter()
                .filter(|frame| !frame["notices"].as_array().expect("notices").is_empty())
                .count();
            let hangs = name == "SHAD_hover_without_moving_byte_hangs";
            assert_eq!(impacts, usize::from(!hangs), "{name}: impact count");
        }
    }

    fn edge_host(
        trig: &'static TrigTable,
        atan: &'static AtanTable,
        tops: Vec<((i16, i16), i32)>,
        bridges: Vec<(i16, i16)>,
    ) -> FixtureHost<'static> {
        FixtureHost {
            frame: 10,
            trig,
            atan,
            kind: FlightOwnerKind::Unit,
            // Cell (3,3) centre, far from the terrain fixture's sloped cell.
            location: [3 * 256 + 128, 3 * 256 + 128, 500],
            balloon_hover: false,
            has_target: false,
            fractions: Vec::new(),
            events: Vec::new(),
            tops,
            bridges,
            piggyback: false,
            simple_deployer: false,
            deploy_to_land: false,
            destination_land_type: 0,
            body_facing: 0,
            taken_slots: Vec::new(),
            held_slot: None,
            scatter_direction: 0,
            can_enter: 0,
            landing_latched: false,
        }
    }

    fn eastbound_flight(speed: f64) -> JumpjetFlight {
        let mut flight = JumpjetFlight::linked(&JumpjetFlightParams::default());
        flight.facing.snap(0x4000, 0);
        flight.current_speed_bits = speed.to_bits();
        flight
    }

    /// `0x0054D820`: both bridge tests add the deck to the *current* cell's
    /// sample, so a bridge one cell ahead raises the current value, and the
    /// flat cell ahead then averages with it.
    #[test]
    fn a_bridge_ahead_raises_the_current_reference_sample() {
        let (trig, _) = required_math_tables();
        let host = edge_host(trig, required_atan_table(), Vec::new(), vec![(4, 3)]);
        let location = host.location;
        assert_eq!(
            reference_height(&eastbound_flight(4.0), &host, location),
            (0 + BRIDGE_DECK_LEPTONS) / 2
        );
        // Stationary: no look-ahead, and the current cell has no bridge.
        assert_eq!(reference_height(&eastbound_flight(0.0), &host, location), 0);
    }

    /// While moving, a higher cell one step ahead along the facing wins.
    #[test]
    fn a_higher_cell_ahead_sets_the_reference() {
        let (trig, _) = required_math_tables();
        let host = edge_host(trig, required_atan_table(), vec![((4, 3), 300)], Vec::new());
        let location = host.location;
        assert_eq!(
            reference_height(&eastbound_flight(4.0), &host, location),
            300
        );
        // A lower cell ahead averages instead: (100 + 300) / 2 from the current cell.
        let host = edge_host(
            trig,
            required_atan_table(),
            vec![((3, 3), 300), ((4, 3), 100)],
            Vec::new(),
        );
        assert_eq!(
            reference_height(&eastbound_flight(4.0), &host, location),
            200
        );
    }

    /// `CellClass @ 0x00485080`: a building's `Dimension2` height is used ahead
    /// of the 85-lepton lift for any other techno.
    #[test]
    fn cell_top_height_prefers_a_building_over_the_object_lift() {
        assert_eq!(cell_top_height(10, Some(208), true), 218);
        assert_eq!(cell_top_height(10, None, true), 95);
        assert_eq!(cell_top_height(10, None, false), 10);
    }

    /// State3's turn error rounds the unsigned 16-bit difference, so a quarter
    /// turn left reads 192 and a quarter turn right reads 64.
    #[test]
    fn a_left_turn_reads_as_a_larger_error_than_the_same_right_turn() {
        let error = |difference: u16| ((((u32::from(difference) >> 7) + 1) >> 1) & 0xFF) as i32;
        assert_eq!(error(0x4000), 64);
        assert_eq!(error(0xC000), 192);
        assert_eq!(error(0xFFFF), 0);
    }
}
