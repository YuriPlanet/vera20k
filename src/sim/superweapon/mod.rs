//! Superweapon system — charging, readiness, suspension, launch dispatch.
//!
//! Each House constructs one `SuperWeaponInstance` per stock type before map
//! objects; buildings grant those retained instances. Its power edge, Super AI visit and
//! dirty tech-tree tail own their distinct passes. `fire` dispatches a launch to its `SuperClass::Launch`
//! case. A computer house fires its charged ones from its Strategy tick
//! (`ai_fire`).
//!
//! ## Dependency rules
//! - Part of sim/ — depends on rules/, sim/power_system, sim/components.
//! - sim/ NEVER depends on render/, ui/, audio/, net/.

pub(crate) mod ai_fire;
pub mod cell_grid;
pub mod chrono_screen;
mod chronosphere;
pub use chronosphere::CHRONO_WARP_SELECTION_INDEX;
pub mod airstrike_tint;
#[cfg(any(test, feature = "test-support"))]
mod cell_receiver_tests;
#[cfg(test)]
mod charge_native_tests;
#[cfg(any(test, feature = "test-support"))]
mod chronosphere_lunar_retail_tests;
#[cfg(any(test, feature = "test-support"))]
pub mod chronosphere_tests;
mod fire;
pub mod force_shield;
pub mod genetic_converter;
pub mod invulnerability;
pub mod iron_curtain;
pub mod lightning_storm;
#[cfg(test)]
pub(crate) mod lightning_storm_tests;
pub(crate) mod nuke;
#[cfg(test)]
mod nuke_tests;
pub mod paradrop;
pub mod psychic_dominator;
#[cfg(test)]
mod psychic_dominator_tests;
pub mod psychic_reveal;
mod spy_plane;
#[cfg(test)]
mod spy_plane_tests;

use crate::rules::ruleset::RuleSet;
use crate::rules::superweapon_type::{SuperWeaponKind, SuperWeaponType};
use crate::sim::intern::InternedId;
use crate::sim::timer::CdTimer;
use crate::sim::world::FrameEffects;
use crate::sim::world::{SimSoundEvent, Simulation};
use crate::util::native_x87::{NativeF32Bits, X87Chop53, X87Ordering};

/// Leptons `SuperClass::Launch` raises an invoke animation above its cell.
const INVOKE_ANIM_Z_LIFT_LEPTONS: i32 = 5;

/// `AnimClass` draw flags of the superweapon cell animations.
const INVOKE_ANIM_DRAW_FLAGS: u32 = 0x600;

/// Construct a superweapon's invoke animation over a cell.
///
/// `SuperClass::Launch @ 0x006CC390` builds all three the same way: Iron
/// Curtain (`Rules+0x348`, `0x006CCF09`), Force Shield (`Rules+0x34C`,
/// `0x006CD130`) and the Genetic Mutator's `IonBlast=` (`Rules+0x298`,
/// `0x006CD8A5`) each call `AnimClass::AnimClass @ 0x00421EA0` with
/// `(type, &coord, delay 0, loopCount 1, drawFlags 0x600, zAdjust 0, reverse 0)`
/// at the cell's [`deck_coords`] plus 5 leptons: Iron Curtain
/// `0x006CCE76..0x006CCF09`, Force Shield `0x006CD07D..0x006CD130` (executed
/// in `tools/superweapon_oracle.py` `force_shield_launch`), Genetic Mutator
/// `0x006CD7F9..0x006CD8A5`.
///
/// A real `AnimClass` plays the art type's `Report=` from `AnimClass::Start`
/// (retail `[IRONBLST] Report=IronCurtainBlast`) and follows its `Rate=` and
/// `Translucent=`. The store does not build native `Middle @ 0x00424F00`
/// (particles, `Scorch=`, `Crater=`).
pub(super) fn spawn_cell_anim(
    sim: &mut Simulation,
    rules: &RuleSet,
    anim_name: &str,
    rx: u16,
    ry: u16,
) {
    let [x, y, z] = deck_coords(sim, (rx, ry));
    spawn_super_anim(
        sim,
        rules,
        anim_name,
        [x, y, z.wrapping_add(INVOKE_ANIM_Z_LIFT_LEPTONS)],
    );
}

/// A cell's GetCoords (vt+0x48, `0x00486840`) raised by the bridge height
/// global (`0x00B0C07C`, initializer `0x006CAD80`: four levels) when the
/// cell carries flag `0x100`. Launch inlines it in cases 1, 3
/// (`0x006CC3C4..0x006CC41F`), 4, 9 and 10 (`0x006CD07D..0x006CD0D3`).
/// CellClass::GetTargetCoords (vt+0x58 @ `0x00486890`,
/// `projectile::cell_target_coord`) makes the same sum with CellClass's own
/// copy of the height (`0x0089E7B4`, the same formula at `0x0047B2C0`), but
/// Launch never calls it; here the flag is read through the superweapons'
/// MapClass lookup (`cell_grid`), as Launch reads the cell it looked up.
fn deck_coords(sim: &Simulation, (x, y): (u16, u16)) -> [i32; 3] {
    let mut coords = fire::cell_coords(sim, (x, y));
    if cell_grid::cell_has_bridge_flag(sim, x as i16, y as i16) {
        coords[2] = coords[2].wrapping_add(crate::util::lepton::BRIDGE_DECK_HEIGHT_LEPTONS);
    }
    coords
}

/// `AnimClass::AnimClass @ 0x00421EA0` with the superweapons' row `(type,
/// &coord, delay 0, loopCount 1, drawFlags 0x600, zAdjust 0, reverse 0)` at
/// a world coordinate (leptons) its caller computed: the invoke anims above,
/// the Chronosphere's (`0x006CB431`, `0x006CC5C5..0x006CC674`), the Psychic
/// Dominator's (`0x0053AEE5`, `0x0053B139`) and the Lightning Storm's clouds,
/// bolts and debris (`0x0053A237`, `0x0053A387`, `0x0053A68B`). An empty name
/// or an art type that never bound constructs nothing.
pub(super) fn spawn_super_anim(
    sim: &mut Simulation,
    rules: &RuleSet,
    anim_name: &str,
    [x, y, z]: [i32; 3],
) -> Option<crate::sim::anim_class::AnimId> {
    let name = anim_name.trim();
    if name.is_empty() {
        return None;
    }
    let world = crate::sim::anim_class::AnimWorldCoord { x, y, z };
    let (rx, ry, sub_x, sub_y, level) = world.to_cell_sub_z();
    let type_id = sim.interner.intern(&name.to_ascii_uppercase());
    let descriptor = crate::sim::components::AnimClassSpawnDescriptor {
        delay: 0,
        loop_count: 1,
        draw_flags: INVOKE_ANIM_DRAW_FLAGS,
        z_adjust: 0,
        reverse: false,
        ..crate::sim::components::AnimClassSpawnDescriptor::new(
            type_id, rx, ry, sub_x, sub_y, level,
        )
    };
    match sim.spawn_anim_at_world(rules, descriptor, world) {
        Ok(anim) => Some(anim),
        Err(error) => {
            log::debug!("superweapon anim [{name}] did not construct: {error}");
            None
        }
    }
}

/// The direct ReceiveDamage (vt+0x16C) Launch hands an object its type's
/// Strength (`+0xA0`) through: `warhead` at distance 0 with no source, from
/// `house`. InfantryClass::IronCurtain (`0x00522600..0x00522632`) and case
/// 9's walk (`0x006CD9F3..0x006CDA29`) ignore defenses and name the
/// launching house; FootClass::IronCurtain's Organic arm
/// (`0x004DEAF8..0x004DEB2B`) does neither. `None` for an object without a
/// type.
fn strength_receiver_event(
    sim: &mut Simulation,
    rules: &RuleSet,
    id: u64,
    warhead: &str,
    house: Option<InternedId>,
    ignore_defenses: bool,
) -> Option<crate::sim::combat::EntityDamageEvent> {
    let strength = sim
        .substrate
        .entities
        .get(id)
        .and_then(|entity| sim.interner.object_type(entity.type_ref(), rules))
        .map(|object| object.strength)?;
    Some(crate::sim::combat::EntityDamageEvent::direct_receiver(
        id,
        strength,
        0,
        crate::sim::combat::RAD_NO_ATTACKER,
        house,
        sim.interner.intern(warhead),
        crate::sim::combat::ReceiverCallFlags {
            ignore_defenses,
            arg6: false,
        },
    ))
}

/// Per-house, per-superweapon-type runtime state.
///
/// Tracks charging progress, readiness, and power suspension.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SuperWeaponInstance {
    /// Actual AbstractClass+10 identity assigned by the admitted Scenario
    /// before House registration. Synthetic fixtures leave it unavailable.
    #[serde(default)]
    native_unique_id: Option<i32>,
    /// Which SuperWeaponType this instance represents (interned ID of the INI section name).
    pub type_id: InternedId,
    /// Which house owns this instance.
    pub owner: InternedId,
    /// Whether the SW is granted (owning building exists and is alive).
    pub is_active: bool,
    /// Whether the SW is fully charged and ready to fire.
    pub is_ready: bool,
    /// Whether charging is paused due to low power.
    pub is_suspended: bool,
    /// Super+6E: Grant6CB57D stores its one-time argument. ClickFire6CBB18
    /// consumes it after Launch; Deactivate deliberately leaves it retained.
    #[serde(default)]
    one_time: bool,
    /// Super+60: constructor6CAFC7 sets it; Execute34 6DE9BB clears it.
    /// Both House50AF5B and Suspend6CB4EA gate provider loss/power on it.
    #[serde(default = "default_can_lose")]
    can_lose: bool,
    /// Native frame when charging began. -1 = timer stopped.
    pub charge_start_tick: i32,
    /// Total charge duration in native frames (may be adjusted on suspend/resume).
    pub charge_duration: i32,
    /// Super+24, initialized to -1 by 0x006CAFA7. Exactly -1 inherits the type's
    /// recharge time; every other signed value is retained verbatim.
    #[serde(default = "inherited_recharge_time")]
    custom_recharge_time: i32,
    /// Charge/drain state: -1=N/A, 0=empty, 1=charged, 2=draining.
    /// Only used when UseChargeDrain=yes (Force Shield). -1 for all others.
    pub charge_drain_state: i32,
    /// Native frame when the SW became ready. Construction uses -100.
    pub ready_tick: i32,
    /// `SuperClass+0x62` (ChronoMapCoords): the cell a Chronosphere click
    /// stores (`0x006CC3D3`) and a PostClick Fire_SW copies from its
    /// PreDependent Super (`0x004FAE8F`). Construction copies the static
    /// `0x00B0C000`, zeroed by its initializer `0x006CADB0`.
    #[serde(default)]
    chrono_cell: (u16, u16),
    /// `SuperClass+0x68`: the ChronoPlacement anim the Chronosphere loops
    /// over that cell (`SuperClass::CreateChronoAnim @ 0x006CB3A0`) until a
    /// release winds it down ([`Simulation::release_super_anim`]).
    #[serde(default)]
    placement_anim: Option<crate::sim::anim_class::AnimId>,
    /// `SuperClass+0x50`: SuperClass::AI calls left until the type's
    /// `SpecialSound=` plays (`step_fade`); -1, the constructor's
    /// (`0x006CAFC4`), is idle. Only the Force Shield's launch arms it
    /// (`0x006CD14F`); a grant leaves it.
    #[serde(default = "idle_fade")]
    fade_countdown: i32,
    /// `SuperClass+0x54`: where that sound plays, the launch's deck
    /// coordinate (leptons).
    #[serde(default)]
    fade_coords: [i32; 3],
    /// The place the Super took in the Super timer list `0x00A83D50`, which
    /// only TacticalClass::Draw reads ([`super_timer_views`]), at its last
    /// grant: Grant appends a `ShowTimer=` Super whose owner is not
    /// `MultiplayPassive` (`0x006CB5D2..0x006CB63B`), and Deactivate removes
    /// it (`0x006CB7C0..0x006CB807`), the others keeping their order. So the
    /// Super is listed while granted with a place, a later place later in
    /// the list; the place outlives Deactivate so that each grant takes a
    /// new one. None when that grant did not list it.
    #[serde(default)]
    timer_place: Option<u32>,
}

fn idle_fade() -> i32 {
    -1
}

fn inherited_recharge_time() -> i32 {
    -1
}

fn default_can_lose() -> bool {
    true
}

impl SuperWeaponInstance {
    #[inline]
    fn charge_timer(&self) -> CdTimer {
        CdTimer::from_raw(self.charge_start_tick, self.charge_duration)
    }

    #[inline]
    fn store_charge_timer(&mut self, timer: CdTimer) {
        self.charge_start_tick = timer.start_frame();
        self.charge_duration = timer.duration();
    }

    /// The charge timer's remaining frames (`SuperClass+0x30`/`+0x38`).
    pub fn charge_remaining(&self, current_frame: i32) -> i32 {
        self.charge_timer().remaining(current_frame)
    }

    /// `SuperClass::GetRechargeTime @ 0x006CC260`: the Super's own charge
    /// time (`+0x24`) unless it is -1, else its type's `RechargeTime=`.
    pub(crate) fn recharge_time(&self, sw: &SuperWeaponType) -> i32 {
        if self.custom_recharge_time == -1 {
            sw.recharge_time_frames
        } else {
            self.custom_recharge_time
        }
    }

    /// SuperClass::SetRechargeTime @ 0x006CC280, called by TAction1326E4320:
    /// only Super+24 changes, including before grant. The existing charge
    /// timer, ready/held flags and anim are untouched; no RNG or detach.
    /// Identity/controls: tools/spatial_oracle/trigger_superweapon_state.meta.json.
    pub(crate) fn set_recharge_time(&mut self, frames: i32) {
        self.custom_recharge_time = frames;
    }

    /// SuperClass::ResetRechargeTime @ 0x006CC290, reached by Action133
    /// through6E4360: restore Type inheritance without changing the current
    /// timer, grant, ready/held state or animation. No RNG/timer/detach calls.
    /// Original Steam execution: trigger_superweapon_reset.meta.json.
    pub(crate) fn reset_recharge_time(&mut self) {
        self.custom_recharge_time = -1;
    }

    /// This newly retained state affects future grants/recharges even when
    /// it currently equals the type value. Preserve the inherited-time fold.
    pub(crate) fn hash_recharge_time(&self, hasher: &mut impl std::hash::Hasher) {
        if self.custom_recharge_time != -1 {
            std::hash::Hash::hash(b"super-recharge-time-v377", hasher);
            std::hash::Hash::hash(&self.custom_recharge_time, hasher);
        }
    }

    pub(crate) fn hash_grant_flags(&self, hasher: &mut impl std::hash::Hasher) {
        if self.one_time || !self.can_lose {
            std::hash::Hash::hash(b"super-grant-flags-v1", hasher);
            std::hash::Hash::hash(&self.one_time, hasher);
            std::hash::Hash::hash(&self.can_lose, hasher);
        }
    }

    /// SuperClass constructor6CAF90: its charge timer starts at currentFrame
    /// with duration0; flags6C..70 begin false. The House constructs all types
    /// before its own attack-timer draw. See campaign_start --houses receipts.
    pub fn new(type_id: InternedId, owner: InternedId, frame: i32) -> Self {
        Self {
            native_unique_id: None,
            type_id,
            owner,
            is_active: false,
            is_ready: false,
            is_suspended: false,
            one_time: false,
            can_lose: true,
            charge_start_tick: frame,
            charge_duration: 0,
            custom_recharge_time: inherited_recharge_time(),
            charge_drain_state: -1,
            ready_tick: -100,
            chrono_cell: (0, 0),
            placement_anim: None,
            fade_countdown: idle_fade(),
            fade_coords: [0; 3],
            timer_place: None,
        }
    }

    pub(crate) fn bind_native_identity(&mut self, id: i32) {
        assert!(
            self.native_unique_id.is_none(),
            "Super construction bound once"
        );
        self.native_unique_id = Some(id);
    }

    pub(crate) const fn native_unique_id(&self) -> Option<i32> {
        self.native_unique_id
    }

    /// Launch case 10's arm (`0x006CD135..0x006CD162`).
    fn arm_fade(&mut self, frames: i32, coords: [i32; 3]) {
        self.fade_countdown = frames;
        self.fade_coords = coords;
    }

    /// The head of `SuperClass::AI @ 0x006CBCA0` (`0x006CBCA8..0x006CBCCF`):
    /// a positive countdown drops by one; one at zero goes idle and returns
    /// where `SpecialSound=` plays (`VocClass::PlayAt @ 0x007509E0`).
    fn step_fade(&mut self) -> Option<[i32; 3]> {
        if self.fade_countdown > 0 {
            self.fade_countdown -= 1;
        }
        if self.fade_countdown != 0 {
            return None;
        }
        self.fade_countdown = idle_fade();
        Some(self.fade_coords)
    }

    /// `SuperClass+0x50` and `+0x54`, for the world hash and map observation.
    pub fn fade(&self) -> (i32, [i32; 3]) {
        (self.fade_countdown, self.fade_coords)
    }

    /// `SuperClass+0x62`, the Chronosphere's source cell.
    pub(crate) fn chrono_cell(&self) -> (u16, u16) {
        self.chrono_cell
    }

    /// `SuperClass+0x68`, the ChronoPlacement anim the Super holds.
    pub(crate) fn placement_anim(&self) -> Option<crate::sim::anim_class::AnimId> {
        self.placement_anim
    }

    /// Retained-state part of Grant6CB560, coordinated by `grant_super` below.
    /// This Boolean is Rust admission, not native AL's timer-restart receipt.
    /// One-time grants retain an old hold and immediately charge via6CC080;
    /// ordinary grants only clear/pause a hold when the +60 gate permits it.
    fn apply_grant_state(
        &mut self,
        sw: &SuperWeaponType,
        current_frame: u32,
        one_time: bool,
        on_hold: bool,
    ) -> bool {
        if self.is_active {
            return false;
        }
        self.is_active = true;
        self.one_time = one_time;
        let frame = current_frame as i32;
        if !one_time && self.is_suspended && self.can_lose {
            let mut timer = self.charge_timer();
            if sw.manual_control {
                timer.pause(frame);
            } else {
                timer.resume(frame);
            }
            self.store_charge_timer(timer);
            self.is_suspended = false;
        }
        if sw.manual_control {
            let mut timer = CdTimer::started(frame, self.recharge_time(sw));
            timer.pause(frame);
            self.store_charge_timer(timer);
        } else if !self.is_ready && (!self.is_suspended || sw.pre_click) {
            self.store_charge_timer(CdTimer::started(frame, self.recharge_time(sw)));
            if sw.use_charge_drain {
                self.charge_drain_state = 0;
            }
        }
        if on_hold && !one_time && !self.is_suspended && self.can_lose {
            let mut timer = self.charge_timer();
            timer.pause(frame);
            self.store_charge_timer(timer);
            self.is_suspended = true;
        }
        if one_time {
            self.set_charged(sw, current_frame);
        }
        true
    }

    /// Silent SetCharged6CC080 as reached from Grant6CB791.
    /// Its fild(i32)/ftol identity subtracts exactly the recharge from itself,
    /// so the new countdown is zero even for signed extreme overrides.
    /// The announcement argument is zero for Execute33; no EVA is queued.
    fn set_charged(&mut self, sw: &SuperWeaponType, current_frame: u32) {
        if !self.is_active {
            return;
        }
        self.is_ready = true;
        self.store_charge_timer(CdTimer::started(current_frame as i32, 0));
        self.ready_tick = current_frame as i32;
        if sw.use_charge_drain {
            self.charge_drain_state = 1;
        }
    }

    /// SetPercentage6CC1E0, called by Execute1296E430D. The actual f32 0.01
    /// (3C23D70A) is5368709/2^29. With percentage0..100 and an i32 recharge,
    /// the product fits i64; PC53 chop discards at most8 low bits, all below
    /// that29-bit binary point, so signed division gives the same ftol integer.
    /// Native execution: trigger_super_charge_percent.json setter_controls.
    /// Preserve ready/held except that a zero remainder sets ready, and restart
    /// the raw timer even while held. TAction132 changes the same effective
    /// recharge time, without retroactively changing an existing timer.
    pub(crate) fn set_charge_percentage(
        &mut self,
        percentage: i32,
        sw: &SuperWeaponType,
        current_frame: u32,
    ) -> bool {
        if !self.is_active || !(0..=100).contains(&percentage) {
            return false;
        }
        let recharge = self.recharge_time(sw);
        let scaled = (i64::from(percentage) * i64::from(recharge) * 5_368_709) / (1_i64 << 29);
        let remaining = recharge.wrapping_sub(scaled as i32);
        if remaining == 0 {
            self.is_ready = true;
        }
        self.store_charge_timer(CdTimer::started(current_frame as i32, remaining));
        true
    }

    /// `SuperClass::Deactivate @ 0x006CB7B0` when no building provides the
    /// Super any more, or the Super Weapons option withholds it: the grant
    /// (`+0x6D`) and the charge (`+0x6F`) end, which takes the Super off the
    /// timer list; its timer and hold stay as they were. The entry stays; a
    /// later grant activates it again.
    pub fn deactivate(&mut self) {
        self.is_active = false;
        self.is_ready = false;
    }

    /// `SuperClass::Suspend @ 0x006CB4D0` for a granted Super that is not
    /// one-time and can hold (`+0x60`, set by the
    /// constructor): holding, or anything for a `ManualControl=` type, stops
    /// a running recharge keeping the time left; releasing restarts a stopped
    /// one. A charged Super holds too, and ClickFire refuses a stopped timer.
    /// Returns whether the hold changed.
    pub(crate) fn suspend(&mut self, on: bool, manual_control: bool, current_frame: u32) -> bool {
        if !self.is_active || self.one_time || !self.can_lose || on == self.is_suspended {
            return false;
        }
        let mut timer = self.charge_timer();
        if on || manual_control {
            timer.pause(current_frame as i32);
        } else {
            timer.resume(current_frame as i32);
        }
        self.store_charge_timer(timer);
        self.is_suspended = on;
        true
    }

    /// `SuperClass::ClickFire @ 0x006CB920`'s admission without charge drain
    /// (`0x006CB933..0x006CB95F`): a running recharge timer on a granted,
    /// charged Super. A timer held for low power does not run.
    fn click_fire_admits(&self) -> bool {
        self.charge_start_tick != -1 && self.is_active && self.is_ready
    }

    /// ClickFire after its Launch (`0x006CBA72..0x006CBB8A`), whatever the
    /// launch did: a one-time instance clears its one-time/granted/ready
    /// flags and leaves its timer/hold intact (6CBB18..6CBB60). Otherwise,
    /// readiness ends unless the type is `PostClick=`; a `ManualControl=`
    /// type's timer starts with the recharge time and is paused at once,
    /// keeping all of it; any other type but `PreClick=`/`PostClick=`
    /// restarts the recharge of a granted Super that is not ready and not
    /// held, for [`Self::recharge_time`]. `CameoChargeState`, -1 on both
    /// restarts, is the sidebar's.
    fn finish_click_fire(&mut self, sw: &SuperWeaponType, current_frame: u32) {
        let frame = current_frame as i32;
        if !sw.post_click {
            self.is_ready = false;
        }
        if self.one_time {
            self.one_time = false;
            if self.is_active {
                self.deactivate();
            }
            return;
        }
        if sw.manual_control {
            let mut timer = CdTimer::started(frame, self.recharge_time(sw));
            timer.pause(frame);
            self.store_charge_timer(timer);
            return;
        }
        if !sw.pre_click && !sw.post_click && self.is_active && !self.is_ready && !self.is_suspended
        {
            self.store_charge_timer(CdTimer::started(frame, self.recharge_time(sw)));
        }
    }

    /// `SuperClass::StopPreclickAnim @ 0x006CB830` after its anim release
    /// ([`Simulation::release_super_anim`]): a granted Super without a
    /// charge restarts its recharge, unless it holds a type that is not
    /// `PreClick=` (`0x006CB89F..0x006CB8FF`). `CameoChargeState` (`+0x78`,
    /// -1 here) is the sidebar's; a `UseChargeDrain=` type's drain state
    /// (`+0x7C`) returns to 0. Returns whether it restarted.
    pub(super) fn stop_preclick(&mut self, sw: &SuperWeaponType, current_frame: u32) -> bool {
        if !self.is_active || self.is_ready || (self.is_suspended && !sw.pre_click) {
            return false;
        }
        self.store_charge_timer(CdTimer::started(
            current_frame as i32,
            self.recharge_time(sw),
        ));
        if sw.use_charge_drain {
            self.charge_drain_state = 0;
        }
        true
    }

    /// `SuperClass::Reset @ 0x006CE0B0` past its anim release
    /// ([`reset_super`]): the charge ends (`+0x6F`) and the recharge timer
    /// restarts with [`Self::recharge_time`]; a stopped timer is stopped again
    /// at once, keeping all of it (`0x006CE12A..0x006CE18F`).
    /// `CameoChargeState` (`+0x78`, -1) is the sidebar's. The grant, the hold
    /// and the drain state stay.
    fn reset(&mut self, sw: &SuperWeaponType, current_frame: u32) {
        let frame = current_frame as i32;
        let stopped = self.charge_start_tick == -1;
        self.is_ready = false;
        let mut timer = CdTimer::started(frame, self.recharge_time(sw));
        if stopped {
            timer.pause(frame);
        }
        self.store_charge_timer(timer);
    }

    /// Compute charge progress as 0.0–1.0 for sidebar display.
    /// Only valid when is_active and not is_ready.
    ///
    /// gamemd.exe parity: standard `SuperClass::AnimStage` at `0x006CBEE0`
    /// (active caller `StripClass::Draw` at `0x006A99AC`) derives progress from
    /// the instance's effective recharge time and the live `CDTimer` remainder.
    pub fn charge_progress(&self, current_frame: u32, full_recharge_frames: i32) -> f32 {
        if self.is_ready {
            return 1.0;
        }
        if full_recharge_frames <= 0 {
            return 0.0;
        }
        let remaining = self.charge_timer().remaining(current_frame as i32);
        let elapsed = full_recharge_frames.wrapping_sub(remaining) as f32;
        (elapsed / full_recharge_frames as f32).clamp(0.0, 1.0)
    }
}

/// `SuperClass::Reset @ 0x006CE0B0` on `owner`'s Super of `sw_type`: the
/// anim release ([`Simulation::release_super_anim`],
/// `0x006CE0B6..0x006CE121`), then [`SuperWeaponInstance::reset`]. Its
/// callers are `BuildingClass::Infiltrate` (`0x00457388`,
/// `sim::spy_infiltration`) and Action134 (`0x006E43A0`). Both use this owner
/// without grant, ready or hold admission. A missing constructed instance
/// cannot be reset. Executed dispatch controls and signed timer boundaries:
/// `tools/spatial_oracle/trigger_superweapon_reset.meta.json`.
pub(crate) fn reset_super(sim: &mut Simulation, rules: &RuleSet, owner: InternedId, sw_type: &str) {
    let (Some(type_id), Some(sw)) = (sim.interner.get(sw_type), rules.super_weapon(sw_type)) else {
        return;
    };
    sim.release_super_anim(owner, type_id);
    let frame = sim.session.binary_frame;
    if let Some(instance) = sim
        .super_weapons
        .get_mut(&owner)
        .and_then(|weapons| weapons.get_mut(&type_id))
    {
        instance.reset(sw, frame);
    }
}

/// Whether `owner`'s Super of `sw_type` is charged (`SuperClass+0x6F`), the
/// flag each Launch case and the computer's launchers read first.
fn is_charged(sim: &Simulation, owner: InternedId, sw_type: InternedId) -> bool {
    sim.super_weapons
        .get(&owner)
        .and_then(|weapons| weapons.get(&sw_type))
        .is_some_and(|instance| instance.is_ready)
}

/// The `[SuperWeaponTypes]` entries whose `Type=` value (`+0xB4`) is
/// `type_value`, in the order of a house's Supers (`HouseClass+0x258`),
/// which hold one Super per entry from the house's constructor
/// (`0x004F620E..0x004F6290`).
pub(crate) fn super_types_with_type(
    rules: &RuleSet,
    type_value: i32,
) -> impl Iterator<Item = &str> {
    rules
        .super_weapon_order
        .iter()
        .filter(move |name| {
            rules
                .super_weapon(name)
                .is_some_and(|sw| sw.kind.native_index() == type_value)
        })
        .map(String::as_str)
}

/// `owner`'s Super of `type_name` is granted (`+0x6D`) and, in PC53/chop, its
/// remaining charge over its recharge time is not above `1.0f - [General]
/// AIMinorSuperReadyPercent=` (`FCOMPP`, `TEST AH,0x1`, so an unordered
/// compare fails it): the AI trigger conditions 5 and 6 (`0x0041F0D0`,
/// `0x0041F180`) and the wait test of team script actions 55 and 57
/// (`0x006EFDC9..0x006EFE4F`, `0x006F032B..0x006F039C`). A Super the house
/// was never granted is not. The recharge time is
/// [`SuperWeaponInstance::recharge_time`].
pub(crate) fn super_nearly_ready(
    sim: &Simulation,
    rules: &RuleSet,
    owner: InternedId,
    type_name: &str,
) -> bool {
    let Some(instance) = sim.interner.get(type_name).and_then(|type_id| {
        sim.super_weapons
            .get(&owner)
            .and_then(|weapons| weapons.get(&type_id))
    }) else {
        return false;
    };
    if !instance.is_active {
        return false;
    }
    let remaining = instance.charge_remaining(sim.session.binary_frame as i32);
    let recharge = rules
        .super_weapon(type_name)
        .map_or(0, |sw| instance.recharge_time(sw));
    charge_nearly_full(
        remaining,
        recharge,
        rules.general.ai_minor_super_ready_percent,
    )
}

/// `1.0f` (`[0x007E2AC8]`).
const ONE_F32: NativeF32Bits = NativeF32Bits::from_bits(0x3F80_0000);

/// `0x0041F148..0x0041F167`: in PC53/chop, `remaining / recharge` is not
/// above `1.0f - percent` (`FCOMPP`, `TEST AH,0x1`); a zero recharge gives
/// +inf or NaN, which fail, or -inf, which passes.
pub(crate) fn charge_nearly_full(remaining: i32, recharge: i32, percent: NativeF32Bits) -> bool {
    type X = X87Chop53;
    let Ok(ratio) = X::div(X::load_i32(remaining), X::load_i32(recharge)) else {
        return remaining < 0;
    };
    let percent = X::load_f32(percent).expect("AIMinorSuperReadyPercent is finite");
    let threshold = X::sub(X::load_f32(ONE_F32).expect("1.0f is finite"), percent);
    X::compare(threshold, ratio) != X87Ordering::Less
}

/// The charge each of `owner`'s Supers with `Type=` value `kind_index` has
/// left, in the house's Supers order (`SuperWeaponTypeClass` array order).
///
/// BuildingClass's SuperAnim code compares a Super's `Type=` (`+0xB4`) with
/// its building type's `SuperWeapon=` array index (`0x0045101C`,
/// `0x00446430`); retail lists every type at the index of its `Type=`, so
/// the two name the same weapon. A Super the house was never granted keeps
/// the constructor's timer, started with no duration
/// (`SuperClass::SuperClass @ 0x006CAF90`): nothing remains.
pub(crate) fn supers_of_kind_remaining(
    sim: &Simulation,
    rules: &RuleSet,
    owner: InternedId,
    kind_index: i32,
) -> Vec<i32> {
    let frame = sim.session.binary_frame as i32;
    let weapons = sim.super_weapons.get(&owner);
    super_types_with_type(rules, kind_index)
        .map(|name| {
            sim.interner
                .get(name)
                .and_then(|id| weapons?.get(&id))
                .map_or(0, |inst| inst.charge_remaining(frame))
        })
        .collect()
}

/// The place a Super joining the timer list takes: after every place taken.
fn next_timer_place(sim: &Simulation) -> u32 {
    sim.super_weapons
        .values()
        .flat_map(|weapons| weapons.values())
        .filter_map(|instance| instance.timer_place)
        .max()
        .map_or(0, |place| place + 1)
}

/// A Super of the timer list `0x00A83D50` as TacticalClass::Draw reads it
/// (`0x006D49C9..0x006D4A6B`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SuperTimerView {
    owner: InternedId,
    sw_type: InternedId,
    frames_left: i32,
    on_hold: bool,
    recharge_frames: i32,
    place: u32,
}

impl SuperTimerView {
    #[cfg(any(test, feature = "test-support"))]
    pub fn for_test(
        owner: InternedId,
        sw_type: InternedId,
        frames_left: i32,
        on_hold: bool,
        recharge_frames: i32,
        place: u32,
    ) -> Self {
        Self {
            owner,
            sw_type,
            frames_left,
            on_hold,
            recharge_frames,
            place,
        }
    }

    pub fn owner(self) -> InternedId {
        self.owner
    }

    pub fn sw_type(self) -> InternedId {
        self.sw_type
    }

    /// The recharge timer's frames left (`+0x30`/`+0x38`).
    pub fn frames_left(self) -> i32 {
        self.frames_left
    }

    /// Held for low power (`+0x70`).
    pub fn on_hold(self) -> bool {
        self.on_hold
    }

    /// [`SuperWeaponInstance::recharge_time`].
    pub fn recharge_frames(self) -> i32 {
        self.recharge_frames
    }

    /// The Super's place in the list; each grant takes a new one, larger
    /// than any taken before.
    pub fn place(self) -> u32 {
        self.place
    }
}

/// The timer list, in its order.
pub fn super_timer_views(sim: &Simulation, rules: &RuleSet) -> Vec<SuperTimerView> {
    let frame = sim.session.binary_frame as i32;
    let mut views: Vec<SuperTimerView> = sim
        .super_weapons
        .iter()
        .flat_map(|(&owner, weapons)| {
            weapons.values().filter_map(move |instance| {
                if !instance.is_active {
                    return None;
                }
                Some(SuperTimerView {
                    owner,
                    sw_type: instance.type_id,
                    frames_left: instance.charge_remaining(frame),
                    on_hold: instance.is_suspended,
                    recharge_frames: rules
                        .super_weapon(sim.interner.resolve(instance.type_id))
                        .map_or(0, |sw| instance.recharge_time(sw)),
                    place: instance.timer_place?,
                })
            })
        })
        .collect();
    views.sort_by_key(|view| view.place);
    views
}

/// View struct for sidebar display — no sim internals exposed.
#[derive(Debug, Clone)]
pub struct SuperWeaponView {
    pub type_id: InternedId,
    pub display_name: String,
    pub progress: f32,
    pub is_ready: bool,
    pub is_online: bool,
    pub sidebar_image: Option<String>,
    pub kind: SuperWeaponKind,
}

/// Query active superweapons for a specific owner (for sidebar rendering).
pub fn superweapon_views_for_owner(
    sim: &Simulation,
    rules: &RuleSet,
    owner: &InternedId,
) -> Vec<SuperWeaponView> {
    let Some(weapons) = sim.super_weapons.get(owner) else {
        return Vec::new();
    };
    let mut views = Vec::new();
    for inst in weapons.values() {
        if !inst.is_active {
            continue;
        }
        let type_id_str = sim.interner.resolve(inst.type_id);
        let Some(sw_type) = rules.super_weapon(type_id_str) else {
            continue;
        };
        views.push(SuperWeaponView {
            type_id: inst.type_id,
            display_name: type_id_str.to_string(),
            progress: inst.charge_progress(sim.session.binary_frame, inst.recharge_time(sw_type)),
            is_ready: inst.is_ready,
            is_online: !inst.is_suspended,
            sidebar_image: sw_type.sidebar_image.clone(),
            kind: sw_type.kind,
        });
    }
    views
}

/// House4F8E28..4F8E84: each existing stock Super's AI6CBCA0, in type
/// registry order, after this House's power/teams/EVA and before its defeat.
/// Hold/revoke50AF10 and grant50B1D0 have their own reached caller phases.
pub(crate) fn tick_house_superweapon_instances(
    sim: &mut Simulation,
    rules: &RuleSet,
    owner: InternedId,
) {
    let current_frame = sim.session.binary_frame;
    for type_id in house_super_order(sim, rules, owner) {
        let (fade, ready) = {
            let instance = sim
                .super_weapons
                .get_mut(&owner)
                .and_then(|weapons| weapons.get_mut(&type_id))
                .expect("selected retained Super");
            // The fade head precedes the granted/held/readiness gates.
            let fade = instance.step_fade();
            let ready = instance.is_active
                && !instance.is_ready
                && !instance.is_suspended
                && instance.charge_start_tick != -1
                && instance.charge_timer().expired(current_frame as i32);
            if ready {
                instance.is_ready = true;
                instance.ready_tick = current_frame as i32;
            }
            (fade, ready)
        };
        if let Some([x, y, z]) = fade
            && let Some(sound_id) = rules
                .super_weapon(sim.interner.resolve(type_id))
                .and_then(|sw| sw.special_sound.clone())
        {
            let (rx, ry, sub_x, sub_y, _) =
                crate::sim::anim_class::AnimWorldCoord { x, y, z }.to_cell_sub_z();
            sim.sound_events.push(SimSoundEvent::VocAt {
                sound_id,
                audible_to: None,
                rx,
                ry,
                sub_x,
                sub_y,
                world_z_leptons: z,
            });
        }
        if ready {
            sim.sound_events.push(SimSoundEvent::SuperWeaponReady {
                owner,
                sw_type: type_id,
            });
        }
    }
}

/// `HouseClass::HouseClass` builds the House's whole Supers vector, one
/// Super per `[SuperWeaponTypes]` entry in that order
/// (`0x004F620E..0x004F6290`), for every House in every game mode; the grant
/// and hold passes only walk that list ([`house_super_order`]). Only the
/// admitted campaign prefix has a measured native-ID cursor, so only it binds
/// each Super's ID (`bind_native_ids`).
pub(crate) fn construct_house_supers(
    sim: &mut Simulation,
    rules: &RuleSet,
    owner: InternedId,
    bind_native_ids: bool,
) {
    let frame = sim.session.binary_frame as i32;
    let mut supers = std::collections::BTreeMap::new();
    for name in &rules.super_weapon_order {
        let type_id = sim.interner.intern(name);
        let mut instance = SuperWeaponInstance::new(type_id, owner, frame);
        if bind_native_ids {
            let super_id = sim
                .next_native_load_id()
                .expect("admitted campaign native-ID cursor");
            instance.bind_native_identity(super_id);
        }
        assert!(
            supers.insert(type_id, instance).is_none(),
            "distinct Super type registry"
        );
    }
    assert!(
        sim.super_weapons.insert(owner, supers).is_none(),
        "a House constructs its Supers once"
    );
}

/// Supers are constructed by the scenario House owner before map objects.
/// Storage-key order is not the original SuperWeaponTypes registry order.
fn house_super_order(sim: &Simulation, rules: &RuleSet, owner: InternedId) -> Vec<InternedId> {
    let Some(weapons) = sim.super_weapons.get(&owner) else {
        return Vec::new();
    };
    rules
        .super_weapon_order
        .iter()
        .filter_map(|name| {
            let id = sim.interner.get(name)?;
            weapons.contains_key(&id).then_some(id)
        })
        .collect()
}

/// Tick already-active global superweapon effects in their native pre-object
/// scheduler slot: `LightningStorm::Process @ 0x0053A6C0` steps the nuke's
/// screen flash, then runs the Psychic Dominator's Process (`0x0053A742`)
/// then ChronoScreenEffect (`0x0053A747`), and the storm's own work.
/// Returns whether a bridge changed.
pub(crate) fn tick_active_superweapon_effects(
    sim: &mut Simulation,
    rules: &RuleSet,
    overlay_registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
    frame_effects: FrameEffects<'_>,
) -> bool {
    if sim
        .session
        .lighting
        .step_nuke_flash(sim.session.binary_frame as i32)
    {
        // `0x0053A705`; the screen redraw (`0x0053A711`) is presentation.
        sim.update_lighting();
    }
    let dominator = psychic_dominator::process(sim, rules, overlay_registry, frame_effects);
    sim.process_chrono_screen_effect();
    let storm = lightning_storm::process(sim, rules, overlay_registry, frame_effects);
    dominator || storm
}

/// The owner's buildings the grant and hold passes scan: on the map, out
/// of limbo and not dying (`0x0050B22E..0x0050B240`,
/// `0x0050AFB0..0x0050AFC6`). Native walks `BuildingClass::Array` for the
/// owner's; the house's own list (House+0x68), which Unlimbo and
/// ChangeOwner append to and the building's expiry clears, holds the same
/// buildings. The passes' answers do not depend on the order.
fn owned_live_buildings(
    sim: &Simulation,
    owner: InternedId,
) -> impl Iterator<Item = &crate::sim::game_entity::GameEntity> {
    sim.houses
        .get(&owner)
        .into_iter()
        .flat_map(|house| house.base_projection.buildings())
        .filter_map(|&id| sim.substrate.entities.get(id))
        .filter(move |building| {
            building.category() == crate::map::entities::EntityCategory::Structure
                && !building.death.is_dying()
                && !building.lifecycle.in_limbo()
                && building.owner() == owner
        })
}

/// Whether `owner` meets `sw`'s `AuxBuilding=` (`+0xC8`, which
/// `BuildingTypeClass::FindOrAllocate @ 0x004653C0` resolves): `sw` names
/// none, or the house has a building of that type on the map
/// (`HouseClass+0x5550`). The provider test (`0x00457630`) and the
/// detection EVA (`0x004468FA..0x00446935`) read it. Retail names none.
pub fn aux_building_present(
    sim: &Simulation,
    rules: &RuleSet,
    owner: InternedId,
    sw: &SuperWeaponType,
) -> bool {
    use crate::map::entities::EntityCategory;
    use crate::rules::object_type::ObjectCategory;
    sw.aux_building.as_deref().is_none_or(|aux| {
        rules
            .object_in_category(ObjectCategory::Building, aux)
            .and_then(|aux| sim.interner.get(&aux.id))
            .zip(sim.houses.get(&owner))
            .is_some_and(|(aux, house)| {
                house.tracking.active_count(EntityCategory::Structure, aux) > 0
            })
    })
}

/// The Supers `building` provides: its type's `SuperWeapon=` and
/// `SuperWeapon2=` (`BuildingClass @ 0x00457630`, `0x00457690`), each only
/// while its owner meets the Super's `AuxBuilding=`
/// ([`aux_building_present`]). The grant pass, the hold pass and the
/// building destructor (`0x0043BE47..0x0043BEEA`) share it.
///
/// RESIDUAL: the passes also count the `SuperWeapon=`/`SuperWeapon2=` of a
/// building's upgrades (`+0x5EC`; `0x0050B242..0x0050B267`,
/// `0x0050AFF2..0x0050B02A`), without the `AuxBuilding=` test. Retail has
/// no upgrade building (`PowersUpBuilding=`), so it never decides.
pub(crate) fn provided_supers<'r>(
    sim: &Simulation,
    rules: &'r RuleSet,
    building: &crate::sim::game_entity::GameEntity,
) -> impl Iterator<Item = &'r str> + use<'r> {
    let object = sim.object_type(building.type_ref(), rules);
    let provides = |sw_id: &str| {
        rules
            .super_weapon(sw_id)
            .is_some_and(|sw| aux_building_present(sim, rules, building.owner(), sw))
    };
    [
        object.and_then(|object| object.super_weapon.as_deref()),
        object.and_then(|object| object.super_weapon2.as_deref()),
    ]
    .map(|sw_id| sw_id.filter(|sw_id| provides(sw_id)))
    .into_iter()
    .flatten()
}

/// Shared provider facts for one reached House pass. Source authority is the
/// retained House building list and the existing457630/457690 provider readers.
fn provider_supers(
    sim: &Simulation,
    rules: &RuleSet,
    owner: InternedId,
) -> (
    std::collections::BTreeSet<InternedId>,
    std::collections::BTreeSet<InternedId>,
) {
    let mut provided = std::collections::BTreeSet::new();
    let mut online = std::collections::BTreeSet::new();
    for building in owned_live_buildings(sim, owner) {
        for name in provided_supers(sim, rules, building) {
            if let Some(id) = sim.interner.get(name) {
                provided.insert(id);
                if building.building_online() {
                    online.insert(id);
                }
            }
        }
    }
    (provided, online)
}

/// House50AF10: hold/release or revoke existing Supers. Reached only at the
/// native power edge, dirty House tail and represented Building destructor.
/// Native code reads the shared building providers and retained power totals;
/// it does not grant or construct. Changed hold/loss re-dirties House+1FC.
pub(crate) fn update_owned_supers(sim: &mut Simulation, rules: &RuleSet, owner: InternedId) {
    let Some(house) = sim.houses.get(&owner) else {
        return;
    };
    let defeated = house.is_defeated;
    let low_power = sim
        .power_states
        .get(&owner)
        .is_some_and(|state| state.is_low_power);
    let (provided, online) = provider_supers(sim, rules, owner);
    for type_id in house_super_order(sim, rules, owner) {
        let Some(sw) = rules.super_weapon(sim.interner.resolve(type_id)) else {
            continue;
        };
        let allowed = sim.session.game_options.super_weapons || !sw.disableable_from_shell;
        let changed = {
            let instance = sim
                .super_weapons
                .get_mut(&owner)
                .and_then(|weapons| weapons.get_mut(&type_id))
                .expect("selected retained Super");
            if !instance.is_active {
                false
            } else if !defeated && (!instance.can_lose || instance.one_time) {
                //50AF5B..50AF78 bypasses shell/provider/power checks, but a
                //defeated House still reaches the ordinary Deactivate owner.
                false
            } else if defeated || !allowed || !provided.contains(&type_id) {
                instance.deactivate();
                true
            } else if !low_power && online.contains(&type_id) {
                instance.suspend(false, sw.manual_control, sim.session.binary_frame)
            } else if sw.is_powered {
                instance.suspend(true, sw.manual_control, sim.session.binary_frame)
            } else {
                false
            }
        };
        if changed {
            sim.request_house_tech_tree_recheck(owner);
            sim.sound_events
                .push(SimSoundEvent::SuperWeaponStatusChanged {
                    owner,
                    sw_type: type_id,
                });
        }
    }
}

/// House50B1D0: grant provided stock instances in the existing type registry
/// order. Native dirty-tail callers run hold/revoke before this pass. Ordinary
/// placement merely requests that tail; upgrade attachment is a separate,
/// currently unported synchronous caller. Never construct a missing Super here.
pub(crate) fn grant_provided_supers(sim: &mut Simulation, rules: &RuleSet, owner: InternedId) {
    let Some(house) = sim.houses.get(&owner) else {
        return;
    };
    if house.is_defeated {
        return;
    }
    //50B2C7..50B305 uses the power ratio even for an unpowered Super type.
    let low_power = sim
        .power_states
        .get(&owner)
        .is_some_and(|state| state.is_low_power);
    let (provided, _) = provider_supers(sim, rules, owner);
    for type_id in house_super_order(sim, rules, owner) {
        if !provided.contains(&type_id) {
            continue;
        }
        let Some(sw) = rules.super_weapon(sim.interner.resolve(type_id)) else {
            continue;
        };
        if !sim.session.game_options.super_weapons && sw.disableable_from_shell {
            continue;
        }
        grant_super(sim, rules, owner, type_id, false, low_power);
    }
}

/// Full Grant6CB560 owner: retained instance flags/timers, timer-list admission
/// and placement-Anim release. House and trigger producers share this path.
/// Original AL reports timer restart; both native callers ignore it. The
/// private state helper instead reports admission to coordinate these effects.
fn grant_super(
    sim: &mut Simulation,
    rules: &RuleSet,
    owner: InternedId,
    type_id: InternedId,
    one_time: bool,
    on_hold: bool,
) -> bool {
    let Some(house) = sim.houses.get(&owner) else {
        return false;
    };
    let passive = house.multiplay_passive;
    let Some(sw) = rules.super_weapon(sim.interner.resolve(type_id)) else {
        return false;
    };
    let place = next_timer_place(sim);
    let Some(instance) = sim
        .super_weapons
        .get_mut(&owner)
        .and_then(|weapons| weapons.get_mut(&type_id))
    else {
        return false;
    };
    if !instance.apply_grant_state(sw, sim.session.binary_frame, one_time, on_hold) {
        return false;
    }
    instance.timer_place = (sw.show_timer && !passive).then_some(place);
    if !sw.manual_control {
        sim.release_super_anim(owner, type_id);
    }
    true
}

/// Execute33/34 at6DE925/6DE98E: use the incoming House's retained Super.
/// The repeat action clears +60 even if Grant rejected an already-active
/// instance; it does not convert that instance's existing one-time flag.
/// Native local Sidebar Add/Recalc is projected by the existing active views,
/// not by StatusChanged (whose distinct hold/revoke consumer clears selection).
pub(crate) fn trigger_grant_super(
    sim: &mut Simulation,
    rules: &RuleSet,
    owner: InternedId,
    type_id: InternedId,
    one_time: bool,
) -> bool {
    if !sim
        .super_weapons
        .get(&owner)
        .is_some_and(|weapons| weapons.contains_key(&type_id))
    {
        return false;
    }
    grant_super(sim, rules, owner, type_id, one_time, false);
    if !one_time {
        sim.super_weapons
            .get_mut(&owner)
            .unwrap()
            .get_mut(&type_id)
            .unwrap()
            .can_lose = false;
    }
    true
}
