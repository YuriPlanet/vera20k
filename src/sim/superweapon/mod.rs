//! Superweapon system — charging, readiness, suspension, launch dispatch.
//!
//! Each player has a set of `SuperWeaponInstance`s, one per superweapon type
//! granted by their buildings. The system ticks after power (for suspend/resume)
//! and before combat. `fire` dispatches a launch to its `SuperClass::Launch`
//! case. A computer house fires its charged ones from its Strategy tick
//! (`ai_fire`).
//!
//! ## Dependency rules
//! - Part of sim/ — depends on rules/, sim/power_system, sim/components.
//! - sim/ NEVER depends on render/, ui/, audio/, net/.

pub(crate) mod ai_fire;
pub mod cell_grid;
mod chronosphere;
pub(crate) use chronosphere::CHRONO_WARP_SELECTION_INDEX;
#[cfg(test)]
mod cell_receiver_tests;
#[cfg(test)]
pub(crate) mod chronosphere_tests;
mod fire;
pub mod force_shield;
pub mod genetic_converter;
pub mod invulnerability;
pub mod iron_curtain;
pub mod lightning_storm;
#[cfg(test)]
mod lightning_storm_tests;
pub(crate) mod nuke;
#[cfg(test)]
mod nuke_tests;
pub mod paradrop;
pub(crate) mod psychic_dominator;
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
        .and_then(|entity| rules.object(sim.interner.resolve(entity.type_ref())))
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
    /// Native frame when charging began. -1 = timer stopped.
    pub charge_start_tick: i32,
    /// Total charge duration in native frames (may be adjusted on suspend/resume).
    pub charge_duration: i32,
    /// Charge/drain state: -1=N/A, 0=empty, 1=charged, 2=draining.
    /// Only used when UseChargeDrain=yes (Force Shield). -1 for all others.
    pub charge_drain_state: i32,
    /// Native frame when the SW became ready. -1 = not ready yet.
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
    pub(crate) fn charge_remaining(&self, current_frame: i32) -> i32 {
        self.charge_timer().remaining(current_frame)
    }

    /// `SuperClass::GetRechargeTime @ 0x006CC260`: the Super's own charge
    /// time (`+0x24`) unless it is -1, else its type's `RechargeTime=`.
    ///
    /// RESIDUAL: VERA keeps no charge time of its own, so this is always the
    /// type's. Trigger: a map trigger that changes a super weapon's charge
    /// time (none in skirmish maps). Effect: that Super charges in its
    /// type's time, the AI readiness tests and the timer line's GameMode 0
    /// hold skip compare against it.
    pub(crate) fn recharge_time(&self, sw: &SuperWeaponType) -> i32 {
        sw.recharge_time_frames
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
            charge_start_tick: frame,
            charge_duration: 0,
            charge_drain_state: -1,
            ready_tick: -1,
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
    pub(crate) fn fade(&self) -> (i32, [i32; 3]) {
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

    /// Activate (grant) this SW and start charging.
    pub fn activate(&mut self, recharge_frames: i32, current_frame: u32) {
        self.is_active = true;
        self.is_ready = false;
        self.is_suspended = false;
        self.charge_start_tick = current_frame as i32;
        self.charge_duration = recharge_frames;
        self.charge_drain_state = -1;
        self.ready_tick = -1;
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
    /// one-time (VERA grants none) and can hold (`+0x71`, set by the
    /// constructor): holding, or anything for a `ManualControl=` type, stops
    /// a running recharge keeping the time left; releasing restarts a stopped
    /// one. A charged Super holds too, and ClickFire refuses a stopped timer.
    /// Returns whether the hold changed.
    pub(crate) fn suspend(&mut self, on: bool, manual_control: bool, current_frame: u32) -> bool {
        if !self.is_active || on == self.is_suspended {
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
    /// launch did, for a Super that is not one-time (VERA grants none):
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

    /// Compute charge progress as 0.0–1.0 for sidebar display.
    /// Only valid when is_active and not is_ready.
    ///
    /// gamemd.exe parity: standard `SuperClass::AnimStage` at `0x006CBEE0`
    /// (active caller `StripClass::Draw` at `0x006A99AC`) derives progress from
    /// the type's full recharge time and the live `CDTimer` remainder.
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
    #[cfg(test)]
    pub(crate) fn for_test(
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
    for (_, inst) in weapons {
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

/// Tick the per-house superweapon instances: initialize grants, advance charge
/// timers, and handle power suspend/resume.
pub fn tick_superweapon_instances(sim: &mut Simulation, rules: &RuleSet) {
    let current_frame = sim.session.binary_frame;

    // One-time initialization: scan all owners' buildings for SW grants.
    // Handles map-pre-placed buildings that bypass production placement hooks.
    // Native grants them in each House's first update, so in the House
    // array's order (`LogicClass__PerTickUpdate @ 0x0055AFB0`, House loop
    // `0x0055B68D..0x0055B6B1`), the order they join the timer list in.
    if !sim.super_weapons_initialized {
        sim.super_weapons_initialized = true;
        let mut owners: std::collections::BTreeSet<InternedId> = sim
            .substrate
            .entities
            .values()
            .filter(|e| e.category == crate::map::entities::EntityCategory::Structure && !e.dying)
            .map(|e| e.owner())
            .collect();
        let mut ordered: Vec<InternedId> = sim
            .session
            .house_order
            .iter()
            .copied()
            .filter(|owner| owners.remove(owner))
            .collect();
        ordered.extend(owners);
        for owner_id in ordered {
            refresh_super_weapons_for_owner(sim, rules, owner_id);
        }
    }

    // Phase 1: Charge/suspend lifecycle for all instances.
    // Collect owners to avoid borrow conflict on sim.super_weapons.
    let owners: Vec<InternedId> = sim.super_weapons.keys().copied().collect();
    let offline_only = offline_only_supers(sim, rules);
    let mut became_ready: Vec<(InternedId, InternedId)> = Vec::new();
    let mut hold_changed: Vec<(InternedId, InternedId)> = Vec::new();
    let mut faded: Vec<(InternedId, [i32; 3])> = Vec::new();
    for owner_id in owners {
        let is_low_power = sim
            .power_states
            .get(&owner_id)
            .map_or(false, |ps| ps.is_low_power);

        let Some(weapons) = sim.super_weapons.get_mut(&owner_id) else {
            continue;
        };
        for (_, inst) in weapons.iter_mut() {
            // SuperClass::AI's head runs for every Super, granted or not,
            // ahead of its grant test (`0x006CBCFE`).
            if let Some(coords) = inst.step_fade() {
                faded.push((inst.type_id, coords));
            }
            if !inst.is_active {
                continue;
            }
            let sw = rules.super_weapon(sim.interner.resolve(inst.type_id));
            // The hold arm of `HouseClass @ 0x0050AF10`: a Super is released,
            // charged or not, while its house has power and one of the
            // buildings that provide it is online (`+0x660`); otherwise a
            // powered type holds and any other keeps its state. One that no
            // building provides was revoked when its last provider went
            // (`refresh_super_weapons_for_owner`). Native runs the pass when
            // the power state flips (`0x00508DD5`) and at the House update
            // after a building event (`+0x1FC`), after the object loop. VERA
            // tests every frame, after the object pass that starts, ends and
            // erases warps, so it holds and releases in the same frame; a
            // release by a Phase 5 kill lands a frame late.
            let manual_control = sw.is_some_and(|sw| sw.manual_control);
            let online = !is_low_power && !offline_only.contains(&(owner_id, inst.type_id));
            let changed = if online {
                inst.suspend(false, manual_control, current_frame)
            } else if sw.is_none_or(|sw| sw.is_powered) {
                inst.suspend(true, manual_control, current_frame)
            } else {
                false
            };
            if changed {
                hold_changed.push((owner_id, inst.type_id));
            }
            if inst.is_ready {
                continue;
            }

            // Charge advancement
            if inst.charge_start_tick != -1 && !inst.is_suspended {
                if inst.charge_timer().expired(current_frame as i32) {
                    inst.is_ready = true;
                    inst.ready_tick = current_frame as i32;
                    became_ready.push((owner_id, inst.type_id));
                }
            }
        }
    }
    // `0x006CBCBA..0x006CBCCF`: the type's `SpecialSound=` at the stored
    // coordinate, for every player.
    for (sw_type, [x, y, z]) in faded {
        let Some(sound_id) = rules
            .super_weapon(sim.interner.resolve(sw_type))
            .and_then(|sw| sw.special_sound.clone())
        else {
            continue;
        };
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
    for (owner, sw_type) in hold_changed {
        sim.sound_events
            .push(SimSoundEvent::SuperWeaponStatusChanged { owner, sw_type });
    }
    // `SuperClass::AI_Ready @ 0x006CBCA0`: `+0x6F` set at `0x006CBDB6`, then
    // `0x006CBE63 PlayEVA(<Type=-indexed *Ready line>, -1)` when the announce
    // argument (`house == PlayerPtr`, `HouseClass::Update 0x004F8E42`) holds.
    // The app applies that local-owner half.
    for (owner, sw_type) in became_ready {
        sim.sound_events
            .push(SimSoundEvent::SuperWeaponReady { owner, sw_type });
    }
}

/// Tick already-active global superweapon effects in their native pre-object
/// scheduler slot: `LightningStorm::Process @ 0x0053A6C0` steps the nuke's
/// screen flash, then runs the Psychic Dominator's Process (`0x0053A742`)
/// and, after the unported chrono screen (`0x0053A747`), the storm's own work.
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
            building.category == crate::map::entities::EntityCategory::Structure
                && !building.dying
                && !building.lifecycle.in_limbo
                && building.owner() == owner
        })
}

/// Whether `owner` meets `sw`'s `AuxBuilding=` (`+0xC8`, which
/// `BuildingTypeClass::FindOrAllocate @ 0x004653C0` resolves): `sw` names
/// none, or the house has a building of that type on the map
/// (`HouseClass+0x5550`). The provider test (`0x00457630`) and the
/// detection EVA (`0x004468FA..0x00446935`) read it. Retail names none.
pub(crate) fn aux_building_present(
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
fn provided_supers<'r>(
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

/// The (owner, Super) pairs the hold pass's online test
/// (`0x0050AFF2..0x0050B05F`) finds providers for, none of them online
/// (`BuildingClass+0x660`, cleared through a Temporal warp): it holds a
/// powered one as it does under low power (`0x0050B0FE..0x0050B12C`).
/// Only the buildings of houses holding a Super are walked, and their
/// Supers only when one of them is offline.
///
/// RESIDUAL: a warp is VERA's only way offline
/// ([`GameEntity::building_online`](crate::sim::game_entity::GameEntity::building_online)).
/// `BuildingClass::GoOffline @ 0x00452360`, which also sets the owner's
/// `+0x1FC` (`0x004523B8`), is not ported, nor its callers: the power
/// toggle event (`0x004C6D9A`), trigger action 61 (`0x006DDFB9`) and a
/// map's powered-down building (`0x0044FD23`). Trigger: a player powers
/// down every building that provides a powered Super. Effect: native holds
/// the Super until one is powered up; VERA has no toggle. Frequency:
/// a player's choice. No retail map powers a superweapon building down:
/// its 338 map texts hold one powered-down `[Structures]` entry and one
/// action 61, neither on one.
fn offline_only_supers(
    sim: &Simulation,
    rules: &RuleSet,
) -> std::collections::BTreeSet<(InternedId, InternedId)> {
    use std::collections::BTreeSet;
    let mut pairs = BTreeSet::new();
    for (&owner, weapons) in &sim.super_weapons {
        if !weapons.values().any(|inst| inst.is_active)
            || owned_live_buildings(sim, owner).all(|building| building.building_online())
        {
            continue;
        }
        let providers = |online: bool| -> BTreeSet<&str> {
            owned_live_buildings(sim, owner)
                .filter(|building| building.building_online() == online)
                .flat_map(|building| provided_supers(sim, rules, building))
                .collect()
        };
        let online = providers(true);
        pairs.extend(
            providers(false)
                .difference(&online)
                .filter_map(|sw_id| Some((owner, sim.interner.get(sw_id)?))),
        );
    }
    pairs
}

/// Refresh superweapon grants for a specific owner by scanning their buildings.
///
/// Call when a building is completed, sold, destroyed, captured or erased.
/// Grants each weapon the owner's buildings provide ([`provided_supers`])
/// and the owner does not hold, including one revoked earlier, and revokes
/// each one no building provides. With the Super Weapons option off, no
/// building provides a `DisableableFromShell=` type.
///
/// gamemd: the grant pass `HouseClass @ 0x0050B1D0` (Ghidra label
/// `HouseClass__Grant_Provided_Supers`, called from `BuildingClass::Unlimbo`
/// and `HouseClass::Update`) calls `SuperClass::Grant @ 0x006CB560` for each
/// weapon whose present flag `+0x6D` is clear. The revoke pass
/// `HouseClass @ 0x0050AF10` (label `HouseClass__Update_Owned_Supers`) calls
/// `SuperClass @ 0x006CB7B0` (label `SuperClass__Deactivate`), which clears
/// `+0x6D` and the charged flag `+0x6F`. Grant returns early only while
/// `+0x6D` is set, and otherwise restarts the recharge timer at the full
/// recharge time. Each loss is reported as
/// [`SimSoundEvent::SuperWeaponStatusChanged`], as is each hold change of
/// [`tick_superweapon_instances`].
///
/// RESIDUALS: an ordinary placement (`BuildingClass::Unlimbo` `0x00440D1E`)
/// and a capture (`0x0044936E`, `0x00449379`) only set the house's `+0x1FC`;
/// the passes run at its next update (`0x004F92F6`, `0x004F92FD`), houses in
/// `HouseClass::Array` order. VERA runs them at the event. Trigger: two
/// `ShowTimer=` grants in one frame, by two houses or two buildings of one
/// house. Effect: their timer lines stack in event order, not in house and
/// `[SuperWeaponTypes]` order; rare, and only the lines' order. The revoke
/// pass also deactivates every Super of a defeated house (`+0x1F5`,
/// `0x0050AF70`, `0x0050B0F4`) whatever buildings it keeps; VERA revokes by
/// building only. Trigger: a defeated house that still owns a superweapon
/// building, not seen in retail play. The MCV's deploy
/// ([`Simulation::deploy_mcv`]) and a construction yard's undeploy
/// ([`Simulation::finish_undeploy`]) unlimbo and limbo a building without
/// a refresh, where native Unlimbo and Limbo set `+0x1FC` (`0x00440D1E`,
/// `0x00445D99`); dormant, as no retail construction yard provides a
/// Super.
pub fn refresh_super_weapons_for_owner(sim: &mut Simulation, rules: &RuleSet, owner: InternedId) {
    use std::collections::BTreeSet;

    let owner_str = sim.interner.resolve(owner).to_string();
    // The Super Weapons game option (`0x00A8B263`) withholds the
    // `DisableableFromShell=` types (`+0xE7`): the grant pass skips them
    // (`0x0050B2A4..0x0050B2B5`) and the hold pass revokes one already
    // granted (`0x0050B085..0x0050B09B`). The others work with it off.
    let super_weapons_allowed = sim.session.game_options.super_weapons;
    let allowed = |sw_id: &&str| {
        super_weapons_allowed
            || rules
                .super_weapon(sw_id)
                .is_some_and(|sw| !sw.disableable_from_shell)
    };

    // Collect all SW type IDs (as strings) granted by living buildings of this owner.
    let granted_strs: Vec<String> = owned_live_buildings(sim, owner)
        .flat_map(|building| provided_supers(sim, rules, building))
        .filter(allowed)
        .map(str::to_string)
        .collect();

    // Intern all granted SW IDs. `BTreeSet` (not `HashSet`) keeps the
    // revoke pass deterministic across machines.
    let granted: BTreeSet<InternedId> = granted_strs
        .iter()
        .map(|s| sim.interner.intern(s))
        .collect();
    // `HouseClass @ 0x0050B1D0` grants in the house's Supers order, the
    // `[SuperWeaponTypes]` order, which is also the order they join the
    // timer list in.
    let mut grant_order: Vec<(Option<usize>, InternedId)> = granted
        .iter()
        .map(|&id| (rules.super_weapon_index(sim.interner.resolve(id)), id))
        .collect();
    grant_order.sort();
    let passive = sim
        .houses
        .get(&owner)
        .is_some_and(|house| house.multiplay_passive);
    let mut timer_place = next_timer_place(sim);

    let weapons = sim.super_weapons.entry(owner).or_default();

    // Grant each weapon the owner does not hold: a new one, or one revoked
    // when its building was lost. Grant keeps the Super's countdown and cell
    // (`+0x50`, `+0x54`, `+0x62`); a type that is not `ManualControl=`
    // (`+0xF5`) releases its held anim (`0x006CB6B4..0x006CB6D2`).
    let mut released = Vec::new();
    for &(_, sw_iid) in &grant_order {
        if weapons.get(&sw_iid).is_some_and(|inst| inst.is_active) {
            continue;
        }
        let sw_str = sim.interner.resolve(sw_iid).to_string();
        let sw = rules.super_weapon(&sw_str);
        let instance = weapons
            .entry(sw_iid)
            .or_insert_with(|| SuperWeaponInstance::new(sw_iid, owner, 0));
        let recharge = sw.map_or(4500, |sw| instance.recharge_time(sw));
        instance.activate(recharge, sim.session.binary_frame);
        // `0x006CB5D2..0x006CB63B`: a `ShowTimer=` Super whose owner is not
        // `MultiplayPassive` joins the end of the timer list.
        instance.timer_place = (sw.is_some_and(|sw| sw.show_timer) && !passive).then(|| {
            timer_place += 1;
            timer_place - 1
        });
        if !sw.is_some_and(|sw| sw.manual_control) {
            released.push(sw_iid);
        }
        log::info!("SuperWeapon '{}' granted to '{}'", sw_str, owner_str);
    }

    // Deactivate revoked (building destroyed, no other provides it, or the
    // option withholds it). The hold pass revokes only a Super that can hold
    // (`+0x60`) and is not one-time (`+0x6E`), or any Super of a defeated
    // house (`0x0050AF5B..0x0050AF78`); VERA's all can hold and none is
    // one-time (the constructor sets `+0x60`, `0x006CAFC7`; only the crate
    // and trigger grants, unported, clear it or grant one-time).
    let revoke_ids: Vec<InternedId> = weapons
        .iter()
        .filter(|(sw_iid, inst)| inst.is_active && !granted.contains(sw_iid))
        .map(|(sw_iid, _)| *sw_iid)
        .collect();
    for &sw_iid in &revoke_ids {
        let sw_str = sim.interner.resolve(sw_iid).to_string();
        log::info!("SuperWeapon '{}' revoked from '{}'", sw_str, owner_str);
        if let Some(inst) = weapons.get_mut(&sw_iid) {
            inst.deactivate();
        }
    }
    for sw_type in revoke_ids {
        sim.sound_events
            .push(SimSoundEvent::SuperWeaponStatusChanged { owner, sw_type });
    }
    for sw_type in released {
        sim.release_super_anim(owner, sw_type);
    }
}

#[cfg(test)]
mod frame_tests {
    use super::*;

    /// A house for each owner, as every scenario owner has: the passes walk
    /// the house's buildings.
    fn add_houses(sim: &mut Simulation, names: &[&str]) {
        for name in names {
            let id = sim.interner.intern(name);
            sim.houses.entry(id).or_insert_with(|| {
                crate::sim::house_state::HouseState::new(id, 0, None, true, 10_000, 10)
            });
        }
    }

    #[test]
    fn charge_progress_uses_full_recharge_time_across_suspend_resume() {
        let id = InternedId::from_index(1);
        let mut instance = SuperWeaponInstance::new(id, id, 0);
        instance.activate(10, 100);

        assert_eq!(instance.charge_progress(104, 10), 0.4);

        assert!(instance.suspend(true, false, 104));
        assert_eq!(instance.charge_start_tick, -1);
        assert_eq!(instance.charge_duration, 6);
        assert_eq!(instance.charge_progress(1000, 10), 0.4);

        assert!(instance.suspend(false, false, 1000));
        assert_eq!(instance.charge_progress(1003, 10), 0.7);
        assert_eq!(instance.charge_progress(1006, 10), 1.0);
    }

    #[test]
    fn charge_progress_uses_full_recharge_time_across_frame_wrap() {
        let id = InternedId::from_index(1);
        let mut instance = SuperWeaponInstance::new(id, id, 0);
        instance.activate(4, u32::MAX - 1);

        assert_eq!(instance.charge_progress(0, 4), 0.5);
        assert!(instance.suspend(true, false, 0));
        assert_eq!(instance.charge_start_tick, -1);
        assert_eq!(instance.charge_duration, 2);
        assert_eq!(instance.charge_progress(1000, 4), 0.5);

        assert!(instance.suspend(false, false, 0));
        assert_eq!(instance.charge_progress(1, 4), 0.75);
        assert_eq!(instance.charge_progress(2, 4), 1.0);
    }

    /// `SuperClass::AI_Ready 0x006CBDCC MOV [ESI+0x6F],BL` then `0x006CBE63
    /// PlayEVA`: the ready line is a sim fact emitted once, at expiry.
    #[test]
    fn charge_expiry_emits_super_weapon_ready_once() {
        use crate::rules::ini_parser::IniFile;
        let ini = IniFile::from_str(
            "[SuperWeaponTypes]\n1=NukeSpecial\n[NukeSpecial]\nType=MultiMissile\n\
             RechargeTime=1\nIsPowered=no\n\
             [InfantryTypes]\n[VehicleTypes]\n[AircraftTypes]\n[BuildingTypes]\n",
        );
        let rules = RuleSet::from_ini(&ini).expect("superweapon ready rules should parse");
        let mut sim = Simulation::new();
        let owner = sim.interner.intern("Americans");
        let sw = sim.interner.intern("NukeSpecial");
        let mut instance = SuperWeaponInstance::new(sw, owner, 0);
        instance.activate(10, sim.session.binary_frame);
        sim.super_weapons
            .entry(owner)
            .or_default()
            .insert(sw, instance);
        sim.super_weapons_initialized = true;
        let ready = |sim: &Simulation| {
            sim.sound_events
                .iter()
                .filter(|event| {
                    matches!(
                        event,
                        SimSoundEvent::SuperWeaponReady { owner: o, sw_type }
                            if *o == owner && *sw_type == sw
                    )
                })
                .count()
        };

        sim.session.binary_frame += 5;
        tick_superweapon_instances(&mut sim, &rules);
        assert_eq!(ready(&sim), 0, "still charging");

        sim.session.binary_frame += 10;
        tick_superweapon_instances(&mut sim, &rules);
        assert_eq!(ready(&sim), 1, "the expiry tick announces");
        assert!(sim.super_weapons[&owner][&sw].is_ready);

        tick_superweapon_instances(&mut sim, &rules);
        assert_eq!(ready(&sim), 1, "a ready weapon is not re-announced");
    }

    /// `SuperClass::Grant @ 0x006CB560` returns early only while the present
    /// flag `+0x6D` is set, and the revoke at `0x006CB7B0` clears it: a house
    /// that sells its only superweapon building and builds another gets the
    /// weapon back, charging from the full recharge time.
    #[test]
    fn a_rebuilt_superweapon_building_grants_its_weapon_again() {
        use crate::rules::ini_parser::IniFile;
        let ini = IniFile::from_str(
            "[SuperWeaponTypes]\n1=NukeSpecial\n[NukeSpecial]\nType=MultiMissile\n\
             RechargeTime=1\nIsPowered=no\n\
             [InfantryTypes]\n[VehicleTypes]\n[AircraftTypes]\n\
             [BuildingTypes]\n1=NAMISL\n\
             [NAMISL]\nStrength=1000\nCost=100\nTechLevel=1\nOwner=Americans\n\
             SuperWeapon=NukeSpecial\n",
        );
        let mut rules = RuleSet::from_ini(&ini).expect("superweapon grant rules should parse");
        rules.set_buildup_control_for_test("NAMISL", [0, 25, 2]);
        let mut sim = Simulation::new();
        add_houses(&mut sim, &["Americans"]);
        let owner = sim.interner.intern("Americans");
        let nuke = sim.interner.intern("NukeSpecial");
        let weapon = |sim: &Simulation| {
            let inst = &sim.super_weapons[&owner][&nuke];
            (
                inst.is_active,
                inst.is_ready,
                inst.charge_start_tick,
                inst.charge_duration,
            )
        };

        let first = sim
            .spawn_object("NAMISL", "Americans", 10, 10, 0, &rules)
            .expect("first silo spawns");
        refresh_super_weapons_for_owner(&mut sim, &rules, owner);
        assert_eq!(weapon(&sim), (true, false, 0, 900));

        assert!(crate::sim::production::sell_building_now_for_test(
            &mut sim, &rules, first
        ));
        assert!(
            !sim.super_weapons[&owner][&nuke].is_active,
            "the sale revokes"
        );

        sim.session.binary_frame = 300;
        sim.spawn_object("NAMISL", "Americans", 20, 20, 0, &rules)
            .expect("second silo spawns");
        refresh_super_weapons_for_owner(&mut sim, &rules, owner);
        assert_eq!(weapon(&sim), (true, false, 300, 900));
    }

    /// The Super timer list `0x00A83D50`: Grant appends a `ShowTimer=` Super
    /// whose owner is not `MultiplayPassive` (`0x006CB5D2..0x006CB63B`), and
    /// the grant pass grants in `[SuperWeaponTypes]` order (`0x0050B1F7`);
    /// Deactivate removes it (`0x006CB7C0..0x006CB807`), the others keeping
    /// their order, and leaves its timer. A new grant appends it again.
    #[test]
    fn the_timer_list_follows_grants_and_losses() {
        use crate::rules::ini_parser::IniFile;
        let ini = IniFile::from_str(
            "[SuperWeaponTypes]\n0=NukeSpecial\n1=IronCurtainSpecial\n2=SpyPlaneSpecial\n\
             [NukeSpecial]\nType=MultiMissile\nRechargeTime=1\nShowTimer=yes\n\
             [IronCurtainSpecial]\nType=IronCurtain\nRechargeTime=2\nShowTimer=yes\n\
             [SpyPlaneSpecial]\nType=SpyPlane\nRechargeTime=3\n\
             [InfantryTypes]\n[VehicleTypes]\n[AircraftTypes]\n\
             [BuildingTypes]\n1=CURTAIN\n2=SILO\n3=RADAR\n\
             [CURTAIN]\nStrength=1000\nSuperWeapon=IronCurtainSpecial\n\
             [SILO]\nStrength=1000\nSuperWeapon=NukeSpecial\n\
             [RADAR]\nStrength=1000\nSuperWeapon=SpyPlaneSpecial\n",
        );
        let mut rules = RuleSet::from_ini(&ini).expect("superweapon timer rules should parse");
        for name in ["CURTAIN", "SILO", "RADAR"] {
            rules.set_buildup_control_for_test(name, [0, 25, 2]);
        }
        let mut sim = Simulation::new();
        add_houses(&mut sim, &["Americans", "Russians"]);
        let americans = sim.interner.intern("Americans");
        let russians = sim.interner.intern("Russians");
        let neutral = sim.interner.intern("Neutral");
        sim.houses
            .entry(neutral)
            .or_insert_with(|| {
                crate::sim::house_state::HouseState::new(neutral, 0, None, true, 10_000, 10)
            })
            .multiplay_passive = true;
        // Interned ahead of the Nuke, so an id-ordered pass would grant it
        // first.
        let curtain = sim.interner.intern("IronCurtainSpecial");
        let nuke = sim.interner.intern("NukeSpecial");
        let listed = |sim: &Simulation, rules: &RuleSet| {
            super_timer_views(sim, rules)
                .into_iter()
                .map(|view| (view.owner(), view.sw_type()))
                .collect::<Vec<_>>()
        };

        sim.spawn_object("CURTAIN", "Americans", 10, 10, 0, &rules)
            .expect("curtain spawns");
        let silo = sim
            .spawn_object("SILO", "Americans", 14, 10, 0, &rules)
            .expect("silo spawns");
        sim.spawn_object("RADAR", "Americans", 18, 10, 0, &rules)
            .expect("radar spawns");
        refresh_super_weapons_for_owner(&mut sim, &rules, americans);
        sim.spawn_object("SILO", "Russians", 10, 20, 0, &rules)
            .expect("russian silo spawns");
        refresh_super_weapons_for_owner(&mut sim, &rules, russians);
        sim.spawn_object("SILO", "Neutral", 10, 30, 0, &rules)
            .expect("neutral silo spawns");
        refresh_super_weapons_for_owner(&mut sim, &rules, neutral);
        assert!(sim.super_weapons[&neutral][&nuke].is_active);
        assert_eq!(
            listed(&sim, &rules),
            [(americans, nuke), (americans, curtain), (russians, nuke)]
        );

        sim.session.binary_frame = 100;
        assert!(crate::sim::production::sell_building_now_for_test(
            &mut sim, &rules, silo
        ));
        assert_eq!(
            listed(&sim, &rules),
            [(americans, curtain), (russians, nuke)]
        );
        let lost = &sim.super_weapons[&americans][&nuke];
        assert_eq!(
            (lost.is_active, lost.charge_start_tick, lost.charge_duration),
            (false, 0, 900),
            "Deactivate leaves the timer"
        );

        sim.spawn_object("SILO", "Americans", 14, 10, 0, &rules)
            .expect("silo spawns again");
        refresh_super_weapons_for_owner(&mut sim, &rules, americans);
        assert_eq!(
            listed(&sim, &rules),
            [(americans, curtain), (russians, nuke), (americans, nuke)]
        );
    }

    /// A map's buildings grant their Supers in each House's first update, in
    /// the House array's order (`LogicClass__PerTickUpdate @ 0x0055AFB0`'s
    /// House loop `0x0055B68D..0x0055B6B1`), which orders the timer list.
    #[test]
    fn the_first_grants_follow_the_house_array() {
        use crate::rules::ini_parser::IniFile;
        let ini = IniFile::from_str(
            "[SuperWeaponTypes]\n0=NukeSpecial\n\
             [NukeSpecial]\nType=MultiMissile\nRechargeTime=1\nShowTimer=yes\n\
             [InfantryTypes]\n[VehicleTypes]\n[AircraftTypes]\n\
             [BuildingTypes]\n1=SILO\n\
             [SILO]\nStrength=1000\nSuperWeapon=NukeSpecial\n",
        );
        let mut rules = RuleSet::from_ini(&ini).expect("superweapon timer rules should parse");
        rules.set_buildup_control_for_test("SILO", [0, 25, 2]);
        let mut sim = Simulation::new();
        // Interned in the other order, so an id-ordered pass lists the
        // Americans first.
        let americans = sim.interner.intern("Americans");
        let russians = sim.interner.intern("Russians");
        for house in [russians, americans] {
            sim.houses.insert(
                house,
                crate::sim::house_state::HouseState::new(house, 0, None, true, 10_000, 10),
            );
            sim.session.house_order.push(house);
        }
        sim.spawn_object("SILO", "Americans", 10, 10, 0, &rules)
            .expect("american silo spawns");
        sim.spawn_object("SILO", "Russians", 10, 20, 0, &rules)
            .expect("russian silo spawns");
        tick_superweapon_instances(&mut sim, &rules);
        let nuke = sim.interner.intern("NukeSpecial");
        let listed: Vec<_> = super_timer_views(&sim, &rules)
            .into_iter()
            .map(|view| (view.owner(), view.sw_type()))
            .collect();
        assert_eq!(listed, [(russians, nuke), (americans, nuke)]);
    }

    /// `BuildingClass::ChangeOwner` asks both houses' next update for their
    /// revoke and grant passes (`+0x1FC` at `0x0044936E` and `0x00449379`;
    /// `HouseClass::Update` `0x004F92F6`, `0x004F92FD`): a captured silo's
    /// Super leaves its old owner and charges for the new one.
    #[test]
    fn a_captured_superweapon_building_moves_its_weapon() {
        use crate::rules::ini_parser::IniFile;
        let ini = IniFile::from_str(
            "[SuperWeaponTypes]\n1=NukeSpecial\n[NukeSpecial]\nType=MultiMissile\n\
             RechargeTime=1\nIsPowered=no\n\
             [InfantryTypes]\n[VehicleTypes]\n[AircraftTypes]\n\
             [BuildingTypes]\n1=NAMISL\n\
             [NAMISL]\nStrength=1000\nCost=100\nTechLevel=1\nOwner=Americans\n\
             SuperWeapon=NukeSpecial\n",
        );
        let mut rules = RuleSet::from_ini(&ini).expect("superweapon grant rules should parse");
        rules.set_buildup_control_for_test("NAMISL", [0, 25, 2]);
        let mut sim = Simulation::new();
        add_houses(&mut sim, &["Americans", "Russians"]);
        let americans = sim.interner.intern("Americans");
        let russians = sim.interner.intern("Russians");
        let nuke = sim.interner.intern("NukeSpecial");
        let weapon = |sim: &Simulation, owner| {
            sim.super_weapons
                .get(&owner)
                .and_then(|weapons| weapons.get(&nuke))
                .map(|inst| (inst.is_active, inst.charge_start_tick))
        };

        let silo = sim
            .spawn_object("NAMISL", "Americans", 10, 10, 0, &rules)
            .expect("silo spawns");
        refresh_super_weapons_for_owner(&mut sim, &rules, americans);
        assert_eq!(weapon(&sim, americans), Some((true, 0)));

        sim.session.binary_frame = 300;
        sim.change_owner_with_rules(
            silo,
            russians,
            &rules,
            None,
            crate::sim::world::FrameEffects::default(),
        );
        assert_eq!(
            weapon(&sim, americans),
            Some((false, 0)),
            "the old owner loses it, its timer left as it was"
        );
        assert_eq!(
            weapon(&sim, russians),
            Some((true, 300)),
            "the new owner charges"
        );
    }

    /// `HouseClass @ 0x0050AF10` acts on a Super's hold changing
    /// (`SuperClass::Suspend @ 0x006CB4D0` returned true) or its loss
    /// (`0x006CB7B0` returned true): each is reported once, the grant is not.
    #[test]
    fn hold_changes_and_losses_report_the_super() {
        use crate::rules::ini_parser::IniFile;
        let ini = IniFile::from_str(
            "[SuperWeaponTypes]\n1=NukeSpecial\n[NukeSpecial]\nType=MultiMissile\n\
             RechargeTime=1\nIsPowered=yes\n\
             [InfantryTypes]\n[VehicleTypes]\n[AircraftTypes]\n\
             [BuildingTypes]\n1=NAMISL\n\
             [NAMISL]\nStrength=1000\nCost=100\nTechLevel=1\nOwner=Americans\n\
             SuperWeapon=NukeSpecial\n",
        );
        let mut rules = RuleSet::from_ini(&ini).expect("superweapon status rules should parse");
        rules.set_buildup_control_for_test("NAMISL", [0, 25, 2]);
        let mut sim = Simulation::new();
        add_houses(&mut sim, &["Americans"]);
        let owner = sim.interner.intern("Americans");
        let nuke = sim.interner.intern("NukeSpecial");
        let reports = |sim: &Simulation| {
            sim.sound_events
                .iter()
                .filter(|event| {
                    matches!(
                        event,
                        SimSoundEvent::SuperWeaponStatusChanged { owner: o, sw_type }
                            if *o == owner && *sw_type == nuke
                    )
                })
                .count()
        };

        let silo = sim
            .spawn_object("NAMISL", "Americans", 10, 10, 0, &rules)
            .expect("silo spawns");
        refresh_super_weapons_for_owner(&mut sim, &rules, owner);
        sim.super_weapons_initialized = true;
        tick_superweapon_instances(&mut sim, &rules);
        assert_eq!(reports(&sim), 0, "the grant");

        sim.power_states.entry(owner).or_default().is_low_power = true;
        tick_superweapon_instances(&mut sim, &rules);
        tick_superweapon_instances(&mut sim, &rules);
        assert_eq!(reports(&sim), 1, "the hold");

        sim.power_states.entry(owner).or_default().is_low_power = false;
        tick_superweapon_instances(&mut sim, &rules);
        assert_eq!(reports(&sim), 2, "the release");

        assert!(crate::sim::production::sell_building_now_for_test(
            &mut sim, &rules, silo
        ));
        assert_eq!(reports(&sim), 3, "the loss");
    }

    /// `BuildingClass @ 0x00457630`: a Super whose `AuxBuilding=` names a
    /// building type is provided only while its owner keeps one of that type
    /// on the map (`HouseClass+0x5550`). `BuildingTypeClass::FindOrAllocate
    /// @ 0x004653C0` resolves the name: any case, `none` for no type, and a
    /// new type no one builds for an unknown name.
    #[test]
    fn an_aux_building_gates_its_super() {
        use crate::rules::ini_parser::IniFile;
        let granted = |aux: &str, plant: bool| {
            let mut rules = RuleSet::from_ini(&IniFile::from_str(&format!(
                "[SuperWeaponTypes]\n0=NukeSpecial\n\
                 [NukeSpecial]\nType=MultiMissile\nRechargeTime=1\nAuxBuilding={aux}\n\
                 [InfantryTypes]\n[VehicleTypes]\n[AircraftTypes]\n\
                 [BuildingTypes]\n1=SILO\n2=PLANT\n\
                 [SILO]\nStrength=1000\nSuperWeapon=NukeSpecial\n[PLANT]\nStrength=1000\n"
            )))
            .expect("aux rules should parse");
            for name in ["SILO", "PLANT"] {
                rules.set_buildup_control_for_test(name, [0, 25, 2]);
            }
            let mut sim = Simulation::new();
            let owner = sim.interner.intern("Americans");
            sim.houses.insert(
                owner,
                crate::sim::house_state::HouseState::new(owner, 0, None, true, 10_000, 10),
            );
            sim.spawn_object("SILO", "Americans", 10, 10, 0, &rules)
                .expect("silo spawns");
            if plant {
                sim.spawn_object("PLANT", "Americans", 14, 10, 0, &rules)
                    .expect("plant spawns");
            }
            refresh_super_weapons_for_owner(&mut sim, &rules, owner);
            let nuke = sim.interner.intern("NukeSpecial");
            sim.super_weapons
                .get(&owner)
                .and_then(|weapons| weapons.get(&nuke))
                .is_some_and(|instance| instance.is_active)
        };
        assert!(!granted("plant", false), "no PLANT on the map");
        assert!(granted("plant", true), "a PLANT");
        assert!(granted("none", false), "no type");
        assert!(!granted("ABSENT", true), "a type no one builds");
    }

    /// The Super Weapons option (`0x00A8B263`) on the retail rules, through
    /// the production reader: off, a house holding a provider of every
    /// `[SuperWeaponTypes]` entry is granted only the types retail leaves
    /// without `DisableableFromShell=yes` (the grant pass's skip,
    /// `0x0050B2A4..0x0050B2B5`); on, every type.
    #[test]
    fn the_super_weapons_option_withholds_the_disableable_types() {
        use super::chronosphere_tests::{retail_rules_binding, world_with};
        let Some(rules) = retail_rules_binding(&[]) else {
            return;
        };
        let (rules, mut sim, americans) = world_with(rules, 64, &[]);
        let providers = [
            "NAMISL", "NAIRON", "GAWEAT", "GACSPH", "GADUMY", "CAAIRP", "AMRADR", "YAPPET",
            "NARADR", "YAGNTC", "GATECH", "NAPSIS",
        ];
        for (index, kind) in (0u16..).zip(providers) {
            let at = (4 + 10 * (index % 4), 4 + 10 * (index / 4));
            sim.spawn_object_at_height(kind, "Americans", at.0, at.1, 0, 0, &rules)
                .unwrap_or_else(|| panic!("{kind} stands"));
        }
        let granted = |sim: &Simulation| {
            rules
                .super_weapon_order
                .iter()
                .filter(|name| {
                    sim.interner
                        .get(name)
                        .and_then(|id| sim.super_weapons.get(&americans)?.get(&id))
                        .is_some_and(|instance| instance.is_active)
                })
                .map(String::as_str)
                .collect::<Vec<_>>()
        };
        let kept = [
            "ChronoWarpSpecial",
            "ParaDropSpecial",
            "AmericanParaDropSpecial",
            "SpyPlaneSpecial",
            "PsychicRevealSpecial",
        ];

        sim.session.game_options.super_weapons = false;
        refresh_super_weapons_for_owner(&mut sim, &rules, americans);
        assert_eq!(granted(&sim), kept);

        sim.session.game_options.super_weapons = true;
        refresh_super_weapons_for_owner(&mut sim, &rules, americans);
        assert_eq!(granted(&sim), rules.super_weapon_order);
    }
}
