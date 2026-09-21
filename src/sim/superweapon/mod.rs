//! Superweapon system — charging, readiness, suspension, launch dispatch.
//!
//! Each player has a set of `SuperWeaponInstance`s, one per superweapon type
//! granted by their buildings. The system ticks after power (for suspend/resume)
//! and before combat. Lightning Storm is the first implemented launch handler.
//!
//! ## Dependency rules
//! - Part of sim/ — depends on rules/, sim/power_system, sim/components.
//! - sim/ NEVER depends on render/, ui/, sidebar/, audio/, net/.

#[cfg(test)]
mod cell_receiver_tests;
pub mod cell_grid;
pub mod force_shield;
pub mod genetic_converter;
pub mod invulnerability;
pub mod iron_curtain;
pub mod lightning_storm;
pub mod paradrop;
#[cfg(test)]
mod paradrop_tests;
pub mod psychic_reveal;

use crate::rules::ruleset::RuleSet;
use crate::rules::superweapon_type::SuperWeaponKind;
use crate::sim::intern::InternedId;
use crate::sim::timer::CdTimer;
use crate::sim::world::{SimSoundEvent, Simulation};

/// Leptons `SuperClass::Launch` raises an invoke animation above its cell.
const INVOKE_ANIM_Z_LIFT_LEPTONS: i32 = 5;

/// `AnimClass` draw flags of the superweapon cell animations.
const INVOKE_ANIM_DRAW_FLAGS: u32 = 0x600;

/// Construct a superweapon animation at a cell's centre coordinate.
///
/// `SuperClass::Launch @ 0x006CC390` builds all three the same way: Iron
/// Curtain (`Rules+0x348`, `0x006CCF09`), Force Shield (`Rules+0x34C`,
/// `0x006CD130`) and the Genetic Mutator's `IonBlast=` (`Rules+0x298`,
/// `0x006CD8A5`) each call `AnimClass::AnimClass @ 0x00421EA0` with
/// `(type, &coord, delay 0, loopCount 1, drawFlags 0x600, zAdjust 0, reverse 0)`.
/// `LightningStorm::GroundStrike @ 0x0053A300` passes the same row for its bolt
/// (`0x0053A387`).
///
/// The two differ in Z. Read at `0x006CCE76..0x006CCF09` for Iron Curtain: the
/// invoke `coord` is the target cell's own coordinate (`vtable+0x48`), raised
/// by the bridge height global (`0x00B0C07C`) when the cell carries flag
/// `0x100`, then `+5` leptons. The bolt takes
/// `CellClass::Get_Center_Coords @ 0x00480A30`, which is the ground with no
/// bridge term. `on_bridge_deck` selects between them: `true` is an invoke
/// row (deck-aware, `+5` leptons), `false` the bolt.
///
/// A real `AnimClass` plays the art type's `Report=` from `AnimClass::Start`
/// (retail `[IRONBLST] Report=IronCurtainBlast`) and follows its `Rate=` and
/// `Translucent=`. The store does not build native `Middle @ 0x00424F00`
/// (particles, `Scorch=`, `Crater=`).
///
/// RESIDUAL: the cell's own Z is taken as `level * 104`; on a ramp cell
/// native's cell coordinate carries the sloped height at the centre.
pub(super) fn spawn_cell_anim(
    sim: &mut Simulation,
    rules: &RuleSet,
    anim_name: &str,
    rx: u16,
    ry: u16,
    on_bridge_deck: bool,
) {
    if anim_name.trim().is_empty() {
        return;
    }
    let level = sim.resolved_terrain.as_ref().map_or(0, |terrain| {
        let flagged = on_bridge_deck
            && terrain.native_cell_flags(terrain.native_cell_identity((rx as i16, ry as i16)))
                & 0x100
                != 0;
        terrain.cell(rx, ry).map_or(0, |cell| {
            if flagged {
                cell.bridge_deck_level_if_any().unwrap_or(cell.level)
            } else {
                cell.level
            }
        })
    });
    let type_name = sim.interner.intern(&anim_name.trim().to_ascii_uppercase());
    let descriptor = crate::sim::components::AnimClassSpawnDescriptor {
        delay: 0,
        loop_count: 1,
        draw_flags: INVOKE_ANIM_DRAW_FLAGS,
        z_adjust: 0,
        reverse: false,
        ..crate::sim::components::AnimClassSpawnDescriptor::new(
            type_name,
            rx,
            ry,
            crate::util::lepton::CELL_CENTER_LEPTON,
            crate::util::lepton::CELL_CENTER_LEPTON,
            level,
        )
    };
    let mut world = crate::sim::anim_class::AnimWorldCoord::from_cell_sub_z(
        rx,
        ry,
        crate::util::lepton::CELL_CENTER_LEPTON,
        crate::util::lepton::CELL_CENTER_LEPTON,
        level,
    );
    if on_bridge_deck {
        // The invoke rows add 5 leptons (`0x006CCE76..0x006CCF09`); the bolt's
        // `Get_Center_Coords` does not.
        world.z = world.z.wrapping_add(INVOKE_ANIM_Z_LIFT_LEPTONS);
    }
    if let Err(error) = sim.spawn_anim_at_world(rules, descriptor, world) {
        // An art type that never bound draws nothing natively either.
        log::debug!("superweapon invoke anim [{anim_name}] did not construct: {error}");
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
        }
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

    /// Deactivate (revoke) this SW when the granting building is lost.
    pub fn deactivate(&mut self) {
        self.is_active = false;
        self.is_ready = false;
        self.is_suspended = false;
        self.charge_start_tick = -1;
        self.ready_tick = -1;
    }

    /// Suspend charging (low power). Saves remaining time.
    pub fn suspend(&mut self, current_frame: u32) {
        if self.charge_start_tick == -1 || self.is_suspended {
            return;
        }
        let mut timer = self.charge_timer();
        timer.pause(current_frame as i32);
        self.store_charge_timer(timer);
        self.is_suspended = true;
    }

    /// Resume charging (power restored). Restarts timer with saved remaining.
    pub fn resume(&mut self, current_frame: u32) {
        if !self.is_suspended {
            return;
        }
        let mut timer = self.charge_timer();
        timer.resume(current_frame as i32);
        self.store_charge_timer(timer);
        self.is_suspended = false;
    }

    /// Reset after firing — restart charge from full duration.
    pub fn reset_after_fire(&mut self, recharge_frames: i32, current_frame: u32) {
        self.is_ready = false;
        self.ready_tick = -1;
        self.charge_start_tick = current_frame as i32;
        self.charge_duration = recharge_frames;
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
            progress: inst.charge_progress(
                sim.session.binary_frame,
                sw_type.recharge_time_frames,
            ),
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
    for owner_id in owners {
        let is_low_power = sim
            .power_states
            .get(&owner_id)
            .map_or(false, |ps| ps.is_low_power);

        let Some(weapons) = sim.super_weapons.get_mut(&owner_id) else {
            continue;
        };
        for (_, inst) in weapons.iter_mut() {
            if !inst.is_active || inst.is_ready {
                continue;
            }
            let type_id_str = sim.interner.resolve(inst.type_id);
            let sw_powered = rules
                .super_weapon(type_id_str)
                .map_or(true, |sw| sw.is_powered);

            // Power suspend/resume
            if sw_powered {
                if is_low_power && !inst.is_suspended {
                    inst.suspend(current_frame);
                } else if !is_low_power && inst.is_suspended {
                    inst.resume(current_frame);
                }
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
/// scheduler slot.
pub fn tick_active_superweapon_effects(
    sim: &mut Simulation,
    rules: &RuleSet,
    overlay_registry: Option<&crate::map::overlay_types::OverlayTypeRegistry>,
) {
    lightning_storm::process(sim, rules, overlay_registry);
}

/// Refresh superweapon grants for a specific owner by scanning their buildings.
///
/// Call when a building is completed, sold, or destroyed. Activates new grants
/// and deactivates revoked ones.
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

    // Activate new grants.
    for &sw_iid in &granted {
        if !weapons.contains_key(&sw_iid) {
            let sw_str = sim.interner.resolve(sw_iid).to_string();
            let recharge = rules
                .super_weapon(&sw_str)
                .map_or(4500, |sw| sw.recharge_time_frames);
            let mut inst = SuperWeaponInstance::new(sw_iid, owner);
            inst.activate(recharge, sim.session.binary_frame);
            log::info!("SuperWeapon '{}' granted to '{}'", sw_str, owner_str);
            weapons.insert(sw_iid, inst);
        }
    }

    // Deactivate revoked (building destroyed, no other provides it).
    let revoke_ids: Vec<InternedId> = weapons
        .iter()
        .filter(|(sw_iid, inst)| inst.is_active && !granted.contains(sw_iid))
        .map(|(sw_iid, _)| *sw_iid)
        .collect();
    for sw_iid in revoke_ids {
        let sw_str = sim.interner.resolve(sw_iid).to_string();
        log::info!("SuperWeapon '{}' revoked from '{}'", sw_str, owner_str);
        if let Some(inst) = weapons.get_mut(&sw_iid) {
            inst.deactivate();
        }
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

        instance.suspend(104);
        assert_eq!(instance.charge_start_tick, -1);
        assert_eq!(instance.charge_duration, 6);
        assert_eq!(instance.charge_progress(1000, 10), 0.4);

        instance.resume(1000);
        assert_eq!(instance.charge_progress(1003, 10), 0.7);
        assert_eq!(instance.charge_progress(1006, 10), 1.0);
    }

    #[test]
    fn charge_progress_uses_full_recharge_time_across_frame_wrap() {
        let id = InternedId::from_index(1);
        let mut instance = SuperWeaponInstance::new(id, id);
        instance.activate(4, u32::MAX - 1);

        assert_eq!(instance.charge_progress(0, 4), 0.5);
        instance.suspend(0);
        assert_eq!(instance.charge_start_tick, -1);
        assert_eq!(instance.charge_duration, 2);
        assert_eq!(instance.charge_progress(1000, 4), 0.5);

        instance.resume(0);
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
        sim.super_weapons.entry(owner).or_default().insert(sw, instance);
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
}
