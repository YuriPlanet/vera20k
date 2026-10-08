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
//! - sim/ NEVER depends on render/, ui/, sidebar/, audio/, net/.

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
#[cfg(test)]
mod paradrop_tests;
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

/// Per-house, per-superweapon-type runtime state.
///
/// Tracks charging progress, readiness, and power suspension.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SuperWeaponInstance {
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

    /// Create a new inactive instance.
    pub fn new(type_id: InternedId, owner: InternedId) -> Self {
        Self {
            type_id,
            owner,
            is_active: false,
            is_ready: false,
            is_suspended: false,
            charge_start_tick: -1,
            charge_duration: 0,
            charge_drain_state: -1,
            ready_tick: -1,
            chrono_cell: (0, 0),
            placement_anim: None,
            fade_countdown: idle_fade(),
            fade_coords: [0; 3],
        }
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

    /// Deactivate (revoke) this SW when the granting building is lost. The
    /// entry stays; a later grant activates it again.
    pub fn deactivate(&mut self) {
        self.is_active = false;
        self.is_ready = false;
        self.is_suspended = false;
        self.charge_start_tick = -1;
        self.ready_tick = -1;
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
    /// held. The recharge is `RechargeTime=`: the Super's custom charge
    /// time stays -1 (no VERA trigger sets it). `CameoChargeState`, -1 on
    /// both restarts, is the sidebar's.
    fn finish_click_fire(&mut self, sw: &SuperWeaponType, current_frame: u32) {
        let frame = current_frame as i32;
        if !sw.post_click {
            self.is_ready = false;
        }
        if sw.manual_control {
            let mut timer = CdTimer::started(frame, sw.recharge_time_frames);
            timer.pause(frame);
            self.store_charge_timer(timer);
            return;
        }
        if !sw.pre_click && !sw.post_click && self.is_active && !self.is_ready && !self.is_suspended
        {
            self.store_charge_timer(CdTimer::started(frame, sw.recharge_time_frames));
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
            sw.recharge_time_frames,
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
/// was never granted is not.
///
/// RESIDUAL: the recharge time is the type's `RechargeTime=`; the per-Super
/// override (`SuperClass+0x24`, read by `GetRechargeTime @ 0x006CC260`) is
/// not kept. Trigger: a map trigger that changes a super weapon's charge
/// time.
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
        .map_or(0, |sw| sw.recharge_time_frames);
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
            progress: inst.charge_progress(sim.session.binary_frame, sw_type.recharge_time_frames),
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
    if !sim.super_weapons_initialized {
        sim.super_weapons_initialized = true;
        let owners: Vec<InternedId> = sim
            .substrate
            .entities
            .values()
            .filter(|e| e.category == crate::map::entities::EntityCategory::Structure && !e.dying)
            .map(|e| e.owner())
            .collect::<std::collections::BTreeSet<_>>()
            .into_iter()
            .collect();
        for owner_id in owners {
            refresh_super_weapons_for_owner(sim, rules, owner_id);
        }
    }

    // Phase 1: Charge/suspend lifecycle for all instances.
    // Collect owners to avoid borrow conflict on sim.super_weapons.
    let owners: Vec<InternedId> = sim.super_weapons.keys().copied().collect();
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
            // The power arm of `HouseClass @ 0x0050AF10`: a powered type
            // holds while the house is short of power, and every type is
            // released otherwise, charged or not. RESIDUAL: native also
            // holds one whose providing building is offline (`+0x660`).
            let manual_control = sw.is_some_and(|sw| sw.manual_control);
            let changed = if !is_low_power {
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
pub fn tick_active_superweapon_effects(
    sim: &mut Simulation,
    rules: &RuleSet,
    overlay_registry: Option<&crate::map::overlay_types::OverlayTypeRegistry>,
) -> bool {
    if sim
        .session
        .lighting
        .step_nuke_flash(sim.session.binary_frame as i32)
    {
        // `0x0053A705`; the screen redraw (`0x0053A711`) is presentation.
        sim.update_lighting();
    }
    let dominator = psychic_dominator::process(sim, rules, overlay_registry);
    let storm = lightning_storm::process(sim, rules, overlay_registry);
    dominator || storm
}

/// Refresh superweapon grants for a specific owner by scanning their buildings.
///
/// Call when a building is completed, sold, or destroyed. Grants each weapon
/// the owner's buildings provide and the owner does not hold, including one
/// revoked earlier, and revokes each one no building provides.
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
pub fn refresh_super_weapons_for_owner(sim: &mut Simulation, rules: &RuleSet, owner: InternedId) {
    use std::collections::BTreeSet;

    let owner_str = sim.interner.resolve(owner).to_string();

    // Collect all SW type IDs (as strings) granted by living buildings of this owner.
    let mut granted_strs: Vec<String> = Vec::new();
    for (_, entity) in sim.substrate.entities.iter_sorted() {
        if entity.owner() != owner {
            continue;
        }
        if entity.category != crate::map::entities::EntityCategory::Structure {
            continue;
        }
        if entity.dying || entity.lifecycle.in_limbo {
            continue;
        }
        let type_str = sim.interner.resolve(entity.type_ref());
        if let Some(obj) = rules.object(type_str) {
            if let Some(ref sw_id) = obj.super_weapon {
                if rules.super_weapon(sw_id).is_some() {
                    granted_strs.push(sw_id.clone());
                }
            }
            if let Some(ref sw2_id) = obj.super_weapon2 {
                if rules.super_weapon(sw2_id).is_some() {
                    granted_strs.push(sw2_id.clone());
                }
            }
        }
    }

    // Intern all granted SW IDs. `BTreeSet` (not `HashSet`) keeps the
    // activation-loop iteration order deterministic across machines, and
    // makes the `log::info!` lines for SW grants reproducible.
    let granted: BTreeSet<InternedId> = granted_strs
        .iter()
        .map(|s| sim.interner.intern(s))
        .collect();

    let weapons = sim.super_weapons.entry(owner).or_default();

    // Grant each weapon the owner does not hold: a new one, or one revoked
    // when its building was lost. Grant keeps the Super's countdown and cell
    // (`+0x50`, `+0x54`, `+0x62`); a type that is not `ManualControl=`
    // (`+0xF5`) releases its held anim (`0x006CB6B4..0x006CB6D2`).
    let mut released = Vec::new();
    for &sw_iid in &granted {
        if weapons.get(&sw_iid).is_some_and(|inst| inst.is_active) {
            continue;
        }
        let sw_str = sim.interner.resolve(sw_iid).to_string();
        let sw = rules.super_weapon(&sw_str);
        let recharge = sw.map_or(4500, |sw| sw.recharge_time_frames);
        weapons
            .entry(sw_iid)
            .or_insert_with(|| SuperWeaponInstance::new(sw_iid, owner))
            .activate(recharge, sim.session.binary_frame);
        if !sw.is_some_and(|sw| sw.manual_control) {
            released.push(sw_iid);
        }
        log::info!("SuperWeapon '{}' granted to '{}'", sw_str, owner_str);
    }

    // Deactivate revoked (building destroyed, no other provides it).
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

    #[test]
    fn charge_progress_uses_full_recharge_time_across_suspend_resume() {
        let id = InternedId::from_index(1);
        let mut instance = SuperWeaponInstance::new(id, id);
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
        let mut instance = SuperWeaponInstance::new(id, id);
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
        let mut instance = SuperWeaponInstance::new(sw, owner);
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
}
