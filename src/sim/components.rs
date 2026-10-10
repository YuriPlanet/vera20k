//! Shared data structs used as fields of GameEntity and components.rs.
//!
//! These are plain data types with no behavior. Game logic lives in systems
//! (movement, combat, etc.). The render loop reads GameEntity fields to
//! determine what to draw and where.
//!
//! ## Design notes
//! - Position stores isometric cell coords only. Where an entity is *drawn* is
//!   `render::locomotor_visual`'s business, derived on read — sim/ writes no
//!   screen coordinates.
//!
//! ## Dependency rules
//! - Part of sim/.
//! - sim/ NEVER depends on render/, ui/, audio/, net/.

use crate::sim::intern::InternedId;
use crate::sim::movement::locomotor::MovementLayer;
use crate::util::fixed_math::{SIM_ZERO, SimFixed};

/// World position in isometric cell coordinates plus sub-cell lepton offset.
///
/// RA2 uses leptons as its spatial unit (256 leptons = 1 cell). We store the
/// cell coordinate (rx, ry) plus a sub-cell lepton offset (sub_x, sub_y) to
/// get sub-cell precision without overflowing SimFixed on large maps.
#[derive(Debug, Clone, Copy, serde::Serialize, serde::Deserialize)]
pub struct Position {
    /// Isometric cell X coordinate.
    pub rx: u16,
    /// Isometric cell Y coordinate.
    pub ry: u16,
    /// Elevation level (0 = ground). Each level is 15px visual offset.
    pub z: u8,
    /// Exact native ObjectClass coordinate Z, in signed leptons.
    ///
    /// Ground movement retains ramp height at the native height-write points;
    /// Drive/Ship residual XY interpolation can retain an earlier sampled Z.
    /// TubeMovement also retains its signed interpolation remainder after exit.
    /// Neither coordinate can be reconstructed from the coarse `z` level or
    /// indiscriminately replaced with the current surface on idle ticks.
    /// None is the legacy coarse/independent-altitude representation.
    #[serde(default)]
    pub exact_z_leptons: Option<i32>,
    /// Sub-cell lepton offset X (0..256). 128 = cell center.
    /// Provides sub-cell precision for smooth movement and accurate range checks.
    pub sub_x: SimFixed,
    /// Sub-cell lepton offset Y (0..256). 128 = cell center.
    pub sub_y: SimFixed,
}

/// Signed actual ObjectClass health (+0x6C in gamemd.exe).
///
/// Live ObjectType::strength owns the cap/ratio denominator. EstimatedHealth
/// is independent state; writes to this field do not implicitly update it.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Health {
    pub current: i32,
}

impl Health {
    /// ObjectClass::GetHealthRatio (gamemd.exe 0x005F5C60..0x005F5C7F).
    /// Signed loads and PC53/chop division; zero Strength keeps the native
    /// masked infinity/NaN result for the caller's actual comparison/conversion.
    pub fn ratio(self, strength: i32) -> crate::util::native_x87::MaskedX87Value {
        use crate::util::native_x87::MaskedX87Chop53 as X87;
        X87::div(X87::load_i32(self.current), X87::load_i32(strength))
    }

    /// [`Self::ratio`] compared `>=` `Rules+0x16F8` with an ordered FCOMP
    /// (the C0-clear test): 0/0 (NaN) is not full and x/0 for x > 0 (+inf)
    /// is. AudioVisual `0x0066B323..0x0066B32D` writes that threshold as 1.0
    /// unconditionally; there is no ConditionGreen key. Object radio
    /// (`0x005F5339`), Techno repair (`0x006F4DE5`), the healer fire errors and
    /// the hospital and depot tests ask it. The quotient of two dwords cannot
    /// round across 1.0, so the integer form is exact.
    pub(crate) fn is_full(self, strength: i32) -> bool {
        match strength.cmp(&0) {
            std::cmp::Ordering::Greater => self.current >= strength,
            std::cmp::Ordering::Less => self.current <= strength,
            std::cmp::Ordering::Equal => self.current > 0,
        }
    }

    /// Compare the native ratio without converting masked values to a host float.
    pub fn compare_ratio(
        self,
        strength: i32,
        threshold: f64,
    ) -> crate::util::native_x87::MaskedX87Ordering {
        use crate::util::native_x87::{MaskedX87Chop53 as X87, NativeF64Bits};
        X87::compare(
            self.ratio(strength),
            X87::load_f64(NativeF64Bits::from_bits(threshold.to_bits())),
        )
    }
}

// Placement descriptors are transient; MissionCom, MissionLeaf and the private
// GameEntity body own retained construction and sale state.
pub(crate) use crate::sim::building_construction::{BuildingDown, BuildingUp};

/// Move-order scheduling adapter.
///
/// The order adapter a move order attaches (ground orders through
/// `issue_move_command_with_destination`, Fly and Jumpjet orders through
/// their own setters). Walk, Drive and Ship keep presence and speed for their
/// Process; their locomotor owns the destination and paid head. Other adapters
/// retain their existing goal. No order
/// keeps route cells: a Foot's route is its Foot+5E0 queue
/// (`navigation.path_replay`).
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct MovementTarget {
    /// Maximum movement speed in leptons per second (from rules.ini Speed= value).
    /// 256 leptons = 1 cell. Fixed-point for deterministic multiplayer.
    pub speed: SimFixed,
    /// Goal for the remaining adapter families, including Jumpjet cruise.
    /// Walk/Drive/Ship leave this empty; their locomotor owns the coordinate.
    pub final_goal: Option<(u16, u16)>,
}

/// Native-like navigation target reference.
///
/// In gamemd this is an `AbstractClass*`. Phase 1 needs cell targets for normal
/// Drive move commands, while the entity variant gives action lines and later
/// destination paths a shared shape without claiming those paths are complete.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum NavTargetRef {
    Cell { rx: u16, ry: u16 },
    Entity { id: u64 },
    Object { id: u64 },
    Building { id: u64 },
}

impl NavTargetRef {
    pub fn cell(rx: u16, ry: u16) -> Self {
        Self::Cell { rx, ry }
    }

    pub fn object(id: u64) -> Self {
        Self::Object { id }
    }

    pub fn building(id: u64) -> Self {
        Self::Building { id }
    }
}

/// FootClass-style owner navigation fields.
///
/// `nav_com` is the owner destination. It must stay separate from
/// `MovementTarget`, which is only the active execution path.
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct NavigationState {
    /// Foot+55C is retained lifecycle history, not current occupancy.
    #[serde(default)]
    pub neighbor_state: crate::sim::cell_neighbors::FootNeighborState,
    /// Foot timers, retry count and blockage latch survive path retirement.
    #[serde(default)]
    pub path_runtime: FootPathRuntime,
    /// Foot-owned direction replay and reference cell (+5E0/+558), shared by
    /// every locomotor instance. Retirement must not discard the owner queue.
    /// Native chain tails reload the owner at Drive4B1DF7 / Ship6A143A.
    #[serde(default)]
    pub path_replay: FootPathQueue,
    #[serde(default)]
    pub nav_com_aux: Option<NavTargetRef>,
    #[serde(default)]
    pub nav_com: Option<NavTargetRef>,
    #[serde(default)]
    pub suspended_nav_com: Option<NavTargetRef>,
    #[serde(default)]
    pub nav_queue: Vec<NavTargetRef>,
    /// Drive path execution reached the destination cell, but the owner
    /// no-active-track arrival clear has not run yet.
    #[serde(default)]
    pub pending_arrival_clear: bool,
    /// FootClass+530, constructor positive-zero. FootUnlimbo4D72EA..4D72F4
    /// copies TechnoType+2F0 only after successful Techno placement, neighbor
    /// publication and the optional AirTracker tail. It outlives routes and
    /// locomotor instances; native getter4DC760 reads this retained double.
    #[serde(default = "default_threat_avoidance_coefficient")]
    threat_avoidance_coefficient: crate::util::native_x87::NativeF64Bits,
}

const fn default_threat_avoidance_coefficient() -> crate::util::native_x87::NativeF64Bits {
    crate::util::native_x87::NativeF64Bits::POSITIVE_ZERO
}

impl Default for NavigationState {
    fn default() -> Self {
        Self::at_frame(0)
    }
}

impl NavigationState {
    /// Foot constructor-owned navigation state, including its frame anchors.
    pub(crate) fn at_frame(frame: u32) -> Self {
        Self {
            neighbor_state: Default::default(),
            path_runtime: FootPathRuntime::at_frame(frame),
            path_replay: Default::default(),
            nav_com_aux: None,
            nav_com: None,
            suspended_nav_com: None,
            nav_queue: Vec::new(),
            pending_arrival_clear: false,
            threat_avoidance_coefficient: default_threat_avoidance_coefficient(),
        }
    }

    /// Retained Foot+530 input to4DC760. Team AvoidThreats overrides this
    /// value at the getter; it does not mutate the stored coefficient.
    pub(crate) const fn path_threat_coefficient(&self) -> crate::util::native_x87::NativeF64Bits {
        self.threat_avoidance_coefficient
    }

    /// Foot4DC760 reads its TeamType+F2 override without changing Foot+530.
    /// Original reader/getter controls: astar_threat_inputs.{py,json}.
    pub(crate) fn path_threat_coefficient_for_team(
        &self,
        avoids_threats: bool,
    ) -> crate::util::native_x87::NativeF64Bits {
        if avoids_threats {
            crate::util::native_x87::NativeF64Bits::ONE
        } else {
            self.path_threat_coefficient()
        }
    }

    /// Successful FootUnlimbo4D72F4's sole type-to-object copy. Failed early
    /// placement and failed Mark must retain constructor or prior Load state.
    pub(crate) fn retain_threat_avoidance_after_unlimbo(
        &mut self,
        coefficient: crate::util::native_x87::NativeF64Bits,
    ) {
        self.threat_avoidance_coefficient = coefficient;
    }
}

/// Persistent Foot path state. The FootClass constructor 0x004D31E0 anchors
/// both timers at the current frame (0x4D3320, 0x4D335B), stores +64C = 10
/// (0x4D332C) and +6B7 = 0 (0x4D3451). Set_Destination_Internal 0x004D94B0
/// rewrites the timers and latch at 0x4D96C2..0x4D9707 for every accepted
/// setter, including a null destination, so this owner outlives MovementTarget.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct FootPathRuntime {
    /// Foot+640/+648, including the native frame anchor.
    pub movement_timer: crate::sim::timer::CdTimer,
    /// Foot+668/+670.
    pub blocked_timer: crate::sim::timer::CdTimer,
    /// Foot+6B7.
    pub path_blocked: bool,
    /// Foot+64C, a dword decremented only while nonzero.
    pub retries_left: u32,
    /// Foot+68A: the pending path-failure sound byte. Ordinary construction
    /// clears it (0x004D33B4); raw Load (0x004103CD) and the no-init Foot
    /// constructor (0x004D3540) preserve the exact byte, including 255.
    /// No ordinary gameplay writer that arms it has been established.
    /// Evidence: `tools/spatial_oracle/foot_scold_latch.{py,json,md}`.
    #[serde(default)]
    scold_latch: u8,
}

impl Default for FootPathRuntime {
    fn default() -> Self {
        Self::at_frame(0)
    }
}

impl FootPathRuntime {
    pub const fn at_frame(frame: u32) -> Self {
        Self {
            movement_timer: crate::sim::timer::CdTimer::started(frame as i32, 0),
            blocked_timer: crate::sim::timer::CdTimer::started(frame as i32, 0),
            path_blocked: false,
            retries_left: 10,
            scold_latch: 0,
        }
    }

    /// Native Foot checksum 0x004DBCFE reads the byte without normalizing it.
    pub(crate) const fn scold_latch_raw(&self) -> u8 {
        self.scold_latch
    }

    /// Walk 0x0075B085..0x0075B0B0 tests nonzero, then clears unconditionally.
    /// Callers decide whether that branch requests a sound before clearing.
    pub(crate) fn clear_scold_latch(&mut self) -> bool {
        std::mem::replace(&mut self.scold_latch, 0) != 0
    }

    /// Supply native retained inputs without inventing a gameplay arming edge.
    #[cfg(test)]
    pub(crate) fn set_scold_latch_for_test(&mut self, raw: u8) {
        self.scold_latch = raw;
    }

    /// Original Foot timers retain the binary frame; repeated Process calls
    /// must not consume time (Drive4B3607, Ship6A2C56, Walk75B979).
    pub(crate) fn start_movement(&mut self, frame: u32, duration: i32) {
        self.movement_timer = crate::sim::timer::CdTimer::started(frame as i32, duration);
    }

    pub(crate) fn start_blocked(&mut self, frame: u32, duration: i32) {
        self.blocked_timer = crate::sim::timer::CdTimer::started(frame as i32, duration);
    }
}

/// Integer world coordinate triplet used by DriveLocomotion state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct DriveCoord {
    pub x: i32,
    pub y: i32,
    pub z: i32,
}

impl DriveCoord {
    const CELL_LEPTONS: i32 = crate::util::lepton::LEPTONS_PER_CELL_I32;
    const CELL_CENTER: i32 = crate::util::lepton::CELL_CENTER_LEPTON_I32;

    pub fn cell(rx: u16, ry: u16, z: i32) -> Self {
        Self {
            x: i32::from(rx) * Self::CELL_LEPTONS + Self::CELL_CENTER,
            y: i32::from(ry) * Self::CELL_LEPTONS + Self::CELL_CENTER,
            z,
        }
    }
}

/// Foot-owned path replay. Native shifts a 24-dword queue on consumption;
/// Rust retains the consumed prefix with an explicit cursor. Consumers must
/// interpret the remaining suffix, not vector emptiness, as the native queue.
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct FootPathQueue {
    /// Foot+5E0: octants0..7, explicit tube direction8, native -1 terminator.
    /// Find_Path4D3E98 writes it; Drive/Ship movement consumes the same owner
    /// memory, independently of which locomotor is currently installed.
    #[serde(default)]
    pub directions: Vec<u8>,
    #[serde(default)]
    pub cursor: u16,
    /// Foot+558. Fresh acceptance writes this reference (4B4618/6A3C47);
    /// chain consumption preserves it (4B1DF7/6A143A).
    #[serde(default)]
    pub reference_cell: Option<(i16, i16)>,
}

impl FootPathQueue {
    /// `start` followed by the cells `words` step through from it. A tube
    /// word (8) names no adjacent cell and ends the walk.
    fn walk_words(start: (i16, i16), words: &[u8]) -> Vec<(i16, i16)> {
        let mut cell = start;
        words
            .iter()
            .map_while(|&direction| {
                let (dx, dy) =
                    *crate::util::direction::DIRECTION_DELTAS.get(usize::from(direction))?;
                cell = (
                    cell.0.wrapping_add(dx as i16),
                    cell.1.wrapping_add(dy as i16),
                );
                Some(cell)
            })
            .collect()
    }

    /// The cells the remaining words step through from the reference cell
    /// (Foot+558), as the path markers read them. Diagnostics and tests.
    pub(crate) fn remaining_cells(&self) -> Vec<(i16, i16)> {
        self.reference_cell
            .map(|reference| Self::walk_words(reference, self.remaining_directions()))
            .unwrap_or_default()
    }

    /// The reference cell followed by [`Self::remaining_cells`].
    #[cfg(test)]
    pub(crate) fn route_cells(&self) -> Vec<(u16, u16)> {
        self.reference_cell
            .into_iter()
            .chain(self.remaining_cells())
            .map(|(x, y)| (x as u16, y as u16))
            .collect()
    }

    /// `start` followed by the cells every word (consumed or not) steps
    /// through from it: the route one Find_Path installed from `start`, while
    /// the queue still holds that install's words.
    #[cfg(test)]
    pub(crate) fn installed_cells(&self, start: (u16, u16)) -> Vec<(u16, u16)> {
        std::iter::once((start.0 as i16, start.1 as i16))
            .chain(Self::walk_words(
                (start.0 as i16, start.1 as i16),
                &self.directions,
            ))
            .map(|(x, y)| (x as u16, y as u16))
            .collect()
    }

    /// Native queue emptiness tests the unconsumed head, not retained history.
    pub(crate) fn remaining_directions(&self) -> &[u8] {
        let suffix = &self.directions[usize::from(self.cursor).min(self.directions.len())..];
        &suffix[..suffix
            .iter()
            .position(|&direction| direction == u8::MAX)
            .unwrap_or(suffix.len())]
    }

    /// Drive4B224F/Ship6A1899 overwrites the live queue head with -1 after
    /// terminal PerCell2, preserving the backing suffix and reference cell.
    pub(crate) fn clear_live_head(&mut self) {
        let cursor = usize::from(self.cursor).min(self.directions.len());
        if cursor == self.directions.len() {
            self.directions.push(u8::MAX);
        } else {
            self.directions[cursor] = u8::MAX;
        }
    }
}

/// Foot-owned applied speed, shared by every installed locomotor instance.
///
/// SetSpeedFraction4D3710 writes Foot+578/+57C; GetCurrentSpeed4DB1A0 reads
/// that fraction live (`movement::owner_current_speed`); nothing caches its
/// result. Drive4AF540/Ship69EC50 constructors and DriveEND4AF930 do
/// not own or reset it. Keep this outside both class payloads so a synchronous
/// callback can replace a locomotor without replacing the owner's speed.
/// Original executable witnesses: tools/spatial_oracle/foot_speed_owner.json.
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct FootSpeedState {
    /// Foot `+0x578`. The Foot constructor zeroes it (`0x004D327F`/`0x004D328B`,
    /// ported by `Default`); after that only `FootClass::SetSpeedFraction @
    /// 0x004D3710` writes it: [`Self::set_speed_fraction`] for a fixed-point
    /// request and [`Self::set_speed_fraction_native_bits`] for a native double.
    applied_fraction: SimFixed,
    /// Foot+580, initialized to exactly 1.0 at4D3292/4D329B. Retain the
    /// native bits: pickup refuses even the immediate neighbors of 1.0.
    /// Every speed query reads it (GetCurrentSpeed multiplies it in). Its only
    /// native writer is the Speed crate (`accept_speed_crate`), reached from
    /// `CellClass::PickupCrate` (`crates::pickup`) when a Foot commits to a
    /// crate cell; saved and hashed (v181).
    crate_multiplier: crate::util::native_x87::NativeF64Bits,
}

impl Default for FootSpeedState {
    fn default() -> Self {
        Self {
            applied_fraction: crate::util::fixed_math::SIM_ZERO,
            crate_multiplier: crate::util::native_x87::NativeF64Bits::ONE,
        }
    }
}

impl FootSpeedState {
    pub(crate) fn crate_multiplier(&self) -> crate::util::native_x87::NativeF64Bits {
        self.crate_multiplier
    }

    /// Foot `+0x578`, the applied speed fraction.
    pub(crate) fn applied_fraction(&self) -> SimFixed {
        self.applied_fraction
    }

    /// `FootClass::SetSpeedFraction @ 0x004D3710` for a fixed-point request:
    /// at least 1 stores 1, at most 0 stores 0, and anything between is
    /// stored as given ([`Self::stored_speed_fraction`]).
    pub(crate) fn set_speed_fraction(&mut self, fraction: SimFixed) {
        self.applied_fraction = fraction.clamp(
            crate::util::fixed_math::SIM_ZERO,
            crate::util::fixed_math::SIM_ONE,
        );
    }

    /// The double `FootClass::SetSpeedFraction @ 0x004D3710` stores at Foot
    /// `+0x578` for a requested one: at least 1.0, +infinity included, stores
    /// 1.0 (`0x004D3714..0x004D3735`); at most 0, negative zero included, or
    /// NaN stores +0 (`0x004D373C..0x004D375D`); anything between is stored
    /// as given. Original executable witnesses, signed zeros, NaNs,
    /// infinities and denormals included:
    /// `tools/spatial_oracle/foot_speed_owner.json`.
    pub(crate) fn stored_speed_fraction(
        requested: crate::util::native_x87::NativeF64Bits,
    ) -> crate::util::native_x87::NativeF64Bits {
        use crate::util::native_x87::NativeF64Bits;
        const INFINITY_BITS: u64 = 0x7ff0_0000_0000_0000;
        let bits = requested.bits();
        if bits >> 63 != 0 || bits == 0 || bits > INFINITY_BITS {
            // A negative value, a zero of either sign, or NaN.
            NativeF64Bits::POSITIVE_ZERO
        } else if bits >= NativeF64Bits::ONE.bits() {
            NativeF64Bits::ONE
        } else {
            requested
        }
    }

    /// `FootClass::SetSpeedFraction @ 0x004D3710` for a locomotor that computes
    /// its fraction as a native double (the Jumpjet's `Process`): the double
    /// [`Self::stored_speed_fraction`] keeps.
    ///
    /// The stored double becomes `SimFixed` by truncation, from its bits, so
    /// the fraction's readers compare against the truncated thresholds
    /// ([`Self::above_tenth`], [`Self::above_eight_tenths`]). A double within
    /// 2^-16 above a threshold would read as not above it. The Jumpjet's speed
    /// is always a whole number k (it steps by the constructor's 2.0 up and
    /// 3.0 down: stock spells `JumpJetAccel=`, which the case-sensitive reader
    /// never sees, and clamps to its integer cap C and 0), so k/C above 0.1 is
    /// at least 0.1 + 1/(10C), outside that window for any C up to 6553, and
    /// above 0.8 for any C up to 13107; k/C exactly 0.1 or 0.8 divides with
    /// truncation to just below it, as native reads it.
    pub(crate) fn set_speed_fraction_native_bits(&mut self, bits: u64) {
        use crate::util::native_x87::NativeF64Bits;
        let stored = Self::stored_speed_fraction(NativeF64Bits::from_bits(bits)).bits();
        self.applied_fraction = if stored == NativeF64Bits::ONE.bits() {
            crate::util::fixed_math::SIM_ONE
        } else {
            // 0 <= value < 1: floor(value * 2^16) from the significand.
            let exponent = ((stored >> 52) & 0x7ff) as i32;
            let mantissa = stored & ((1 << 52) - 1);
            let significand = if exponent == 0 {
                mantissa
            } else {
                mantissa | (1 << 52)
            };
            let shift = 1075 - i64::from(exponent.max(1)) - 16;
            SimFixed::from_bits(if shift >= 64 {
                0
            } else {
                (significand >> shift) as i32
            })
        };
    }

    /// Foot `+0x578` above 0.1 (`0x007E3860`): the Infantry fire error's
    /// moving gate (`0x0051C9B8`) and the sequencer's default arm
    /// (`0x00520D45`). 0.1 lies between `SimFixed` raw 6553 and 6554; the
    /// truncated threshold keeps a fraction that truncated to 6553 below it.
    /// Original boundary witnesses: `tools/spatial_oracle/infantry_fire_speed.json`.
    pub(crate) fn above_tenth(&self) -> bool {
        self.applied_fraction > SimFixed::ONE / SimFixed::from_num(10)
    }

    /// Foot `+0x578` above 0.8 (`0x007EB5C8`): the Jumpjet infantryman's Fly
    /// over Hover (`0x0052123C`). floor(0.8 * 2^16) = 52428.
    pub(crate) fn above_eight_tenths(&self) -> bool {
        self.applied_fraction > SimFixed::from_bits(52_428)
    }

    /// Cell48303A..483072: an already-modified Foot never stacks this effect.
    /// Class/radius eligibility belongs to the pickup effect caller.
    pub(crate) fn accept_speed_crate(
        &mut self,
        multiplier: crate::util::native_x87::NativeF64Bits,
    ) -> bool {
        use crate::util::native_x87::{MaskedX87Chop53 as X, NativeF64Bits};
        if self.crate_multiplier != NativeF64Bits::ONE {
            return false;
        }
        self.crate_multiplier = X::store_f64_masked_chop(X::mul(
            X::load_f64(self.crate_multiplier),
            X::load_f64(multiplier),
        ));
        true
    }
}

/// One active Drive/Ship locomotor's retained track selector, signed cursor,
/// short-track choice and residual (+58/+5C/+60/+4C). Curve geometry and a
/// temporary Process_Track call must not own serialized copies of this state.
/// Native evidence: tools/spatial_oracle/locomotor_track_cursor.json.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct TrackProgress {
    pub turn_index: i32,
    /// Next-to-consume cursor. Drive constructor4AF5A6/4AF5A9 and Ship
    /// constructor69ECB6/69ECB9 initialize selector/cursor to -1;
    /// fresh/forced acceptance and completion retirement instead store zero.
    pub cursor: i32,
    pub reversed: bool,
    pub residual: i32,
}

impl Default for TrackProgress {
    fn default() -> Self {
        Self {
            turn_index: -1,
            cursor: -1,
            reversed: false,
            residual: 0,
        }
    }
}

/// Drive-owned occupation mark installed ahead of the live object-list cell.
///
/// The cell list remains tied to the unit's committed coordinates. This record
/// persists the independent head-to mark so the transient per-cell occupation
/// index can be rebuilt after loading a snapshot.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct DriveOccupationFootprint {
    pub rx: u16,
    pub ry: u16,
    pub layer: MovementLayer,
}

/// Default acceleration/deceleration values — zero means no ramping,
/// movement system falls back to using `speed` directly.
impl Default for MovementTarget {
    fn default() -> Self {
        Self {
            speed: SIM_ZERO,
            final_goal: None,
        }
    }
}

/// Persistent high-level order state that survives transient combat/movement components.
///
/// This keeps attack-move intent alive while systems temporarily
/// add/remove `MovementTarget` and `AttackTarget`.
///
/// Slice 6: the "is this unit busy?" signalling role moved to the `mission`
/// substrate (`mission::verb::get_current_mission`/`is_busy`). What remains here
/// is the AttackMove goal. Area Guard uses its native mission and
/// ArchiveTarget instead. Retiring this enum entirely waits
/// on a goal field landing on the mission/nav substrate (a later slice).
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum OrderIntent {
    /// Move toward a destination but auto-acquire enemies along the way.
    AttackMove { goal_rx: u16, goal_ry: u16 },
}

/// Which part of a multi-part voxel model an entity/atlas entry represents.
///
/// Non-turret units use `Composite` (body+turret+barrel baked together).
/// Turret units store Body/Turret/Barrel separately so the turret can
/// be drawn at a different facing than the body at render time.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, serde::Serialize, serde::Deserialize,
)]
pub enum VxlLayer {
    /// All parts composited into one sprite (for units without independent turret).
    Composite,
    /// Body only (hull/chassis).
    Body,
    /// Turret only ({IMAGE}TUR.VXL).
    Turret,
    /// Barrel only ({IMAGE}BARL.VXL).
    Barrel,
    /// Ground shadow of the main voxel: each section's occupied (x, y) columns
    /// flattened onto z = 0 and shifted by the shadow light vector, always at
    /// motion frame 0. See `render::vxl_raster::render_vxl_shadow`.
    Shadow,
}

/// Per-entity voxel HVA animation state.
///
/// Attached to voxel entities that cycle through HVA animation frames at runtime.
/// Used for harvesting miners (arm/turret animation), and potentially other voxel
/// units with multi-frame HVA files. The render loop reads `frame` to select
/// the correct pre-rendered atlas sprite.
#[derive(Debug, Clone, Copy, serde::Serialize, serde::Deserialize)]
pub struct VoxelAnimation {
    /// Current HVA frame index (0-based).
    pub frame: u32,
    /// Total number of HVA animation frames.
    pub frame_count: u32,
    /// Reached native frames accumulated since last image advance.
    pub elapsed_frames: u16,
    /// Reached native frames per image. 0 = no auto-advance.
    pub frame_delay: u16,
    /// Whether the animation is currently playing (cycling frames).
    pub playing: bool,
}

impl VoxelAnimation {
    /// Create a new VoxelAnimation in stopped state.
    pub fn new(frame_count: u32, frame_delay: u16) -> Self {
        Self {
            frame: 0,
            frame_count,
            elapsed_frames: 0,
            frame_delay,
            playing: false,
        }
    }
}

/// Harvest overlay animation state for the oregath.shp ore-gathering sprite.
///
/// Attached to harvester entities (HARV, CMIN). Shows the visual "sucking up ore"
/// animation as an SHP overlay on top of the VXL body when actively harvesting.
/// Uses the effect palette (anim.pal), independent of house colors.
#[derive(Debug, Clone, Copy, serde::Serialize, serde::Deserialize)]
pub struct HarvestOverlay {
    /// Current animation frame (0..14, 15 frames per facing direction).
    pub frame: u16,
    /// Whether the overlay is currently visible and animating.
    pub visible: bool,
}

/// Constructor row for a generic AnimClass-like runtime spawn.
///
/// This preserves the fields passed to `AnimClass::Constructor` separately from
/// presentation conveniences such as cached frame count and wall-clock frame
/// delay, so parity-sensitive code can inspect the original constructor surface.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
pub struct AnimClassSpawnDescriptor {
    /// AnimType/SHP type interned ID.
    pub type_name: InternedId,
    /// World cell containing the constructor coordinate.
    pub rx: u16,
    pub ry: u16,
    /// Sub-cell constructor coordinate in leptons.
    pub sub_x: SimFixed,
    pub sub_y: SimFixed,
    /// Height level for the constructor coordinate.
    pub z: u8,
    /// Constructor `delay` argument, in native logic frames.
    pub delay: u16,
    /// Signed constructor `loop` argument.
    pub loop_count: i32,
    /// Constructor draw flags argument.
    pub draw_flags: u32,
    /// Constructor `ZAdjust` argument.
    pub z_adjust: i32,
    /// Constructor reverse argument.
    pub reverse: bool,
    /// AnimClass `+0x196`: draw through the owning cell's palette/light path.
    #[serde(default)]
    pub use_cell_drawer: bool,
    /// AnimClass `+0x197`: marks the instance as terrain-attached.
    #[serde(default)]
    pub terrain_attached: bool,
    /// Instance draw-state bytes supplied by the native producer.
    pub draw_runtime: crate::sim::anim_class::AnimDrawRuntime,
}

impl AnimClassSpawnDescriptor {
    pub fn new(
        type_name: InternedId,
        rx: u16,
        ry: u16,
        sub_x: SimFixed,
        sub_y: SimFixed,
        z: u8,
    ) -> Self {
        Self {
            type_name,
            rx,
            ry,
            sub_x,
            sub_y,
            z,
            delay: 0,
            loop_count: 1,
            draw_flags: 0,
            z_adjust: 0,
            reverse: false,
            use_cell_drawer: false,
            terrain_attached: false,
            draw_runtime: crate::sim::anim_class::AnimDrawRuntime::default(),
        }
    }
}

/// Emitted by the refinery dock state machine for EVERY due dump gate of
/// `UnitClass::Mission_Unload @ 0x0073D630` state 3 (`HarvesterDumpRate × 900
/// <= unit+0xF8`, `0x0073E355..0x0073E374`), including the final gate that finds
/// no cargo. The authoritative master-frame tail consumes it to spawn the
/// refinery smoke burst (`BuildingClass` vtable+0x468 → `0x00459900`, fired
/// first on each gate at `0x0073E37E`), start SpecialAnim slot 10 only while
/// none is live (`building+0x584 == NULL`, `0x0073E384`), and cut the running
/// SpecialAnim on the empty gate (`ClearAnimSlot(0xA)`, `0x0073E530`) before
/// the returned state hash. The queue itself is transient and serde-skipped.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct BaleDepositEvent {
    /// Refinery stable_id where the bale was deposited.
    pub building_id: u64,
    /// Sim tick when this event was emitted (for ordering / debugging).
    pub tick: u64,
    /// One StorageClass slot drained on this gate (credits were paid;
    /// `0x0073E4A2..0x0073E4DA`).
    pub drained: bool,
    /// The gate found no cargo (`FindFirstNonEmptySlot == -1`,
    /// `0x0073E4DC..0x0073E534`): state 3 → 4 and the SpecialAnim is cut.
    pub empty: bool,
}

/// Emitted by the tank-bunker lifecycle when the walls rise (install) or fall
/// (teardown). The authoritative frame tail creates the bunker's SpecialAnim
/// overlays — document order within `kind == Special` decides the pair: 0/1 =
/// walls-up, 2/3 = walls-down. `damaged` selects the `…Damaged` art variant when
/// the building was at/below ConditionRed health at emit time. The queue is
/// transient; its resulting overlay component participates in the frame hash.
#[derive(Debug, Clone, Copy, serde::Serialize, serde::Deserialize)]
pub struct BunkerWallAnimEvent {
    /// Bunker building stable_id whose walls are animating.
    pub building_id: u64,
    /// `true` = walls rising (install), `false` = walls falling (teardown).
    pub up: bool,
    /// Building was at/below ConditionRed when the event fired — use the
    /// `…Damaged` SpecialAnim variant.
    pub damaged: bool,
}

/// Per-attacker walk-up intent for the C4 plant mission.
///
/// Mirrors gamemd's SEAL/Tanya/PTROOP behavior: while this is `Some`, the
/// unit pathfinds toward the target building. On arrival at the target's
/// cell, `tick_c4_plants` claims the plant by setting
/// `PendingC4Detonation` on the building. This state is cleared when the
/// player retasks the unit (Move/Stop) or when the target is lost.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct C4PlantState {
    pub target_building_id: u64,
}

/// Body rocking state for voxel-bodied units.
///
/// Tracks spring-damped roll/pitch angles driven by weapon impacts and EMP
/// wobble. Drive/Ship slope interpolation is locomotor-owned state and is
/// intentionally independent from this optional component.
///
/// Optional component on `GameEntity` — present on vehicles, ships, and
/// voxel-bodied buildings; `None` for infantry and SHP-bodied buildings, and
/// for aircraft until `FootClass::Crash` sets their spin rates.
#[derive(Debug, Clone, Copy, Default, serde::Serialize, serde::Deserialize)]
pub struct RockingState {
    /// Roll angle, rad. Positive sign matches AngleRotatedSideways convention.
    pub angle_sideways: SimFixed,
    /// Pitch angle, rad. Positive sign matches AngleRotatedForwards convention.
    pub angle_forwards: SimFixed,
    /// Roll angular velocity, rad/tick.
    pub vel_sideways: SimFixed,
    /// Pitch angular velocity, rad/tick.
    pub vel_forwards: SimFixed,
    /// If true, integrate without damping (EMP wobble, naval continuous rocking).
    pub is_ship_rocking: bool,
}

impl RockingState {
    /// Tilt-renderer deadband — both angles below this snap to zero and the unit
    /// renders via the static atlas path.
    pub const DEADBAND: SimFixed = SimFixed::lit("0.00002");

    /// Returns true when the body-rocking transform is neutral.
    #[cfg(test)]
    pub fn is_neutral(&self) -> bool {
        !self.is_ship_rocking
            && self.angle_sideways.abs() <= Self::DEADBAND
            && self.angle_forwards.abs() <= Self::DEADBAND
    }
}

/// Shared per-building C4 / PostMortem detonation timer.
///
/// Native BuildingClass uses one latch and timer triple for both an infantry
/// C4 plant and a qualifying `CausesDelayKill` fatal hit. Once elapsed, the
/// building's own Update fires a forced C4Warhead receiver packet with damage
/// equal to its current HP.
///
/// IronCurtain/ForceShield entry cancels this state. Normal targets keep an
/// expired latch if the forced receiver unexpectedly leaves them alive;
/// BridgeRepairHut owns the separate consume-and-clear branch.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct PendingC4Detonation {
    /// Native Building timer (`+0x528` start, `+0x530` duration), signed and
    /// unclamped; its remaining time drives the shorten test, the Building
    /// Update expiry and the checksum.
    pub timer: crate::sim::timer::CdTimer,
    /// Retained source-object identity (`+0x540`). Fresh PostMortem arms leave
    /// this null; shortening an infantry C4 timer preserves its source.
    pub source_entity_id: Option<u64>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sim::movement::DriveLocomotionRuntime;

    #[test]
    fn test_position_creation() {
        let pos: Position = Position {
            rx: 30,
            ry: 40,
            z: 0,
            exact_z_leptons: None,
            sub_x: crate::util::lepton::CELL_CENTER_LEPTON,
            sub_y: crate::util::lepton::CELL_CENTER_LEPTON,
        };
        assert_eq!(pos.rx, 30);
        assert_eq!(pos.ry, 40);
    }

    #[test]
    fn test_types_are_send_sync() {
        // GameEntity fields must be Send + Sync for future multithreaded sim ticks.
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<Position>();
        assert_send_sync::<Health>();
        assert_send_sync::<MovementTarget>();
        assert_send_sync::<OrderIntent>();
        assert_send_sync::<BuildingUp>();
        assert_send_sync::<VoxelAnimation>();
        assert_send_sync::<HarvestOverlay>();
        assert_send_sync::<crate::sim::movement::locomotor::LocomotorState>();
        assert_send_sync::<NavigationState>();
        assert_send_sync::<DriveLocomotionRuntime>();
    }

    #[test]
    fn drive_coord_cell_uses_center_leptons() {
        let coord = DriveCoord::cell(45, 40, 0);
        assert_eq!(coord.x, 45 * 256 + 128);
        assert_eq!(coord.y, 40 * 256 + 128);
        assert_eq!(coord.z, 0);
    }

    #[test]
    fn nav_target_ref_has_cell_object_and_building_shapes() {
        assert_eq!(
            NavTargetRef::cell(45, 40),
            NavTargetRef::Cell { rx: 45, ry: 40 }
        );
        assert_eq!(NavTargetRef::object(7), NavTargetRef::Object { id: 7 });
        assert_eq!(NavTargetRef::building(9), NavTargetRef::Building { id: 9 });
    }

    #[test]
    fn drive_locomotion_default_is_inert() {
        let drive = DriveLocomotionRuntime::default();
        assert_eq!(drive.destination(), None);
        assert_eq!(drive.head_to(), None);
        let navigation = NavigationState::default();
        assert!(navigation.path_replay.directions.is_empty());
        assert_eq!(navigation.path_replay.cursor, 0);
        assert_eq!(drive.track().turn_index, -1);
        assert_eq!(drive.track().cursor, -1);
        assert!(!drive.track_valid());
        assert!(!drive.track().reversed);
        assert_eq!(drive.target_speed_fraction(), SIM_ZERO);
        let owner_speed = FootSpeedState::default();
        assert_eq!(owner_speed.applied_fraction, SIM_ZERO);
        assert_eq!(drive.track().residual, 0);
    }

    /// `FootClass::SetSpeedFraction @ 0x004D3710` executed on the original
    /// bytes: the double it stores for each requested one, signed zeros,
    /// NaNs, infinities and denormals included, and that double's truncation
    /// to `SimFixed`.
    #[test]
    fn speed_fraction_setter_matches_the_original() {
        use crate::util::native_x87::NativeF64Bits;
        let cases: Vec<serde_json::Value> = serde_json::from_str(crate::test_fixture::text(
            "tools/spatial_oracle/foot_speed_owner.json",
        ))
        .unwrap();
        let bits = |row: &serde_json::Value, hex: &str, float: &str| {
            row[hex].as_str().map_or_else(
                || row[float].as_f64().unwrap().to_bits(),
                |hex| u64::from_str_radix(hex, 16).unwrap(),
            )
        };
        let mut setter_rows = 0;
        for case in &cases {
            let requested = bits(&case["input"], "requested_bits", "requested");
            let stored = bits(&case["output"], "applied_bits", "applied");
            let requested_double = NativeF64Bits::from_bits(requested);
            assert_eq!(
                FootSpeedState::stored_speed_fraction(requested_double).bits(),
                stored,
                "{case}"
            );
            let mut speed = FootSpeedState::default();
            speed.set_speed_fraction_native_bits(requested);
            // The stored double lies in [0, 1], so scaling by 2^16 is exact.
            let truncated = SimFixed::from_bits((f64::from_bits(stored) * 65536.0) as i32);
            assert_eq!(speed.applied_fraction(), truncated, "{case}");
            setter_rows += usize::from(case["input"]["family"] == "setter");
        }
        assert_eq!((cases.len(), setter_rows), (25, 13));
    }

    #[test]
    fn drive_locomotion_serde_defaults_missing_fields() {
        let drive: DriveLocomotionRuntime = serde_json::from_str("{}").expect("deserialize drive");
        assert_eq!(drive, DriveLocomotionRuntime::default());
    }

    #[test]
    fn drive_locomotion_hash_changes_when_runtime_state_changes() {
        use std::hash::{Hash, Hasher};

        fn hash_drive(drive: &DriveLocomotionRuntime) -> u64 {
            let mut hasher = std::collections::hash_map::DefaultHasher::new();
            drive.hash(&mut hasher);
            hasher.finish()
        }

        let drive_a = DriveLocomotionRuntime::default();
        let drive_b = DriveLocomotionRuntime::default()
            .with_destination_for_test(Some(DriveCoord::cell(45, 40, 0)))
            .with_track_for_test(TrackProgress {
                residual: 6,
                ..Default::default()
            });

        assert_ne!(hash_drive(&drive_a), hash_drive(&drive_b));
    }

    #[test]
    fn c4_state_types_are_send_sync_copy() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<C4PlantState>();
        assert_send_sync::<PendingC4Detonation>();
        // Compile-time Copy assertion via fn-bound:
        fn _assert_copy<T: Copy>() {}
        _assert_copy::<C4PlantState>();
        _assert_copy::<PendingC4Detonation>();
    }

    #[test]
    fn rocking_default_is_neutral() {
        let r = RockingState::default();
        assert!(r.is_neutral());
    }

    #[test]
    fn rocking_active_angle_is_not_neutral() {
        let mut r = RockingState::default();
        r.angle_sideways = SimFixed::lit("0.01");
        assert!(!r.is_neutral());
    }

    #[test]
    fn rocking_within_deadband_is_neutral() {
        let mut r = RockingState::default();
        // SimFixed precision is ~1.5e-5, so 1e-5 rounds to 0; pick a value
        // strictly between the smallest representable nonzero and DEADBAND.
        // DEADBAND is 2e-5; SIM_EPSILON is ~1.5e-5 — exactly one delta below.
        r.angle_sideways = SimFixed::DELTA;
        assert!(r.is_neutral());
    }

    #[test]
    fn rocking_ship_rocking_is_not_neutral() {
        let mut r = RockingState::default();
        r.is_ship_rocking = true;
        assert!(!r.is_neutral());
    }

    /// The integer `is_full` answers what the masked chop53 division and
    /// FCOMP against 1.0 answer, including x/0, 0/0 and the i32 extremes.
    #[test]
    fn is_full_matches_the_masked_ratio_compare() {
        use crate::util::native_x87::MaskedX87Ordering::{Equal, Greater};
        let mut values = vec![i32::MIN, i32::MIN + 1, i32::MAX - 1, i32::MAX];
        for base in [0i32, 1, 2, 3, 100, 600, 1 << 24, 1 << 30] {
            for delta in [-1i32, 0, 1] {
                let v = base + delta;
                values.extend([v, -v]);
            }
        }
        for &strength in &values {
            for &health in &values {
                let health = Health { current: health };
                let native = matches!(health.compare_ratio(strength, 1.0), Equal | Greater);
                assert_eq!(health.is_full(strength), native, "{health:?} / {strength}");
            }
        }
    }
}
