//! The target scan every Techno shares, and its gates:
//! - `TechnoClass::Retaliate_And_Scan @ 0x00709820` (vt+0x39C), the one
//!   scanner: the passive block calls it with mask 1, `FootClass::
//!   Mission_AreaGuard` with mask 2 (`0x004D6F06`) and `FootClass::
//!   Mission_Hunt` with mask 0 (`0x004D5392`);
//! - `TechnoClass::PassiveAcquireGate @ 0x00709290` and
//!   `TechnoClass::CanAcquireTarget @ 0x007091D0`;
//! - the passive block of `TechnoClass::AI_Update` (`0x006FA65A..0x006FA6EE`).
//!
//! A scan keeps the target it holds unless its own earlier scan installed it
//! (`+0x50C`) and GetFireError now answers ILLEGAL, CANT without a
//! SpawnManager, or RANGE. With no target left it asks Greatest_Threat and
//! assigns the pick. Nothing else retargets: a kill clears the killers'
//! targets through pointer expiry, and the next passive scan picks again.
//!
//! Native execution: `tools/spatial_oracle/techno_target_scan.py` runs the
//! original `0x00709820` with supplied callees; [`tests`] replays every row
//! (callee order and arguments, the draw, the timer, the target, the flag and
//! the estimate debit). `tools/spatial_oracle/passive_acquire_gate.py` runs
//! the original gate with CanAcquireTarget, which [`tests`] compares through
//! the production gate.

use super::ObjectAiCtx;
use crate::map::entities::EntityCategory;
use crate::rules::overlay_types::OverlayTypeRegistry;
use crate::rules::ruleset::RuleSet;
use crate::rules::weapon_type::WeaponType;
use crate::sim::combat::fire_error::FireError;
use crate::sim::combat::fire_error_world::FireSubject;
use crate::sim::combat::{ScanMission, TargetKind, combat_weapon};
use crate::sim::mission::MissionType;
use crate::sim::mission::timer::MissionTimer;
use crate::sim::rng::SimRng;
use crate::sim::world::ObjectSubstrate;
use crate::sim::world::Simulation;

/// Committed mission id of Area Guard (`+0xAC == 0xB`, `0x0070983A`).
const AREA_GUARD_MISSION: i32 = 0xB;

/// What `Retaliate_And_Scan` asks of its object, in the order it asks.
/// Frames the passive target-scan timer is armed for at object construction:
/// the Techno constructor anchors it at the current frame (`0x006F2C33`) and
/// writes 45 as its duration (`0x006F2C39`), so a freshly built object waits
/// that long before its first passive scan.
const PASSIVE_SCAN_CONSTRUCTION_DELAY_FRAMES: u32 = 45;

/// The passive target scan's state on the object.
/// - The targeting timer (`TechnoClass+0x180` start, `+0x188` duration):
///   expiry opens the passive scan gate. The constructor arms it for 45
///   frames ([`Self::new`]); the scanner re-arms it to the `[General]`
///   targeting delay plus a 0..=2 jitter, and a live target's pointer expiry
///   re-arms it short (`RandomRanged(4, 8)`, `0x00707A0D`), both through
///   [`Self::arm_timer`].
/// - The last scan frame (`+0x4FC`): the constructor stamps the frame
///   (`0x006F3106`) and the scanner stamps it on entry
///   ([`Self::stamp_scan`]). Only the hash reads it.
/// - The acquisition mark (`+0x50C`): the passive block's scan changed the
///   target (`0x006FA6EE`, [`Self::mark_acquired`]; the teleport clone
///   `0x007094C8` is unported). Every Assign_Target clears it first
///   (`0x006FCDC4`) and the off-mission clear drops it with the target
///   (`0x006FA5F0`), both through [`Self::clear_acquired`]. Read by the
///   scan's drop step (`0x007098C3`), the off-mission clear and the Unit
///   approach's OmniCrusher arm (`0x0074162D`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct PassiveScan {
    timer: MissionTimer,
    last_scan_frame: u32,
    acquired: bool,
}

impl Default for PassiveScan {
    fn default() -> Self {
        Self::new(0)
    }
}

impl PassiveScan {
    /// The constructor's state at `frame`.
    pub(crate) fn new(frame: u32) -> Self {
        Self {
            timer: MissionTimer::armed(frame, PASSIVE_SCAN_CONSTRUCTION_DELAY_FRAMES),
            last_scan_frame: frame,
            acquired: false,
        }
    }

    /// The targeting timer.
    pub const fn timer(self) -> MissionTimer {
        self.timer
    }

    /// The frame of the last scan.
    pub(crate) const fn last_scan_frame(self) -> u32 {
        self.last_scan_frame
    }

    /// The current target came from the passive scan.
    pub const fn acquired(self) -> bool {
        self.acquired
    }

    /// Arm the targeting timer: the scanner's re-arm and the pointer
    /// expiry's short re-arm.
    pub(crate) fn arm_timer(&mut self, start: u32, duration: u32) {
        self.timer.arm(start, duration);
    }

    /// The scanner's entry stamp.
    fn stamp_scan(&mut self, frame: u32) {
        self.last_scan_frame = frame;
    }

    /// The passive block changed the target (`0x006FA6EE`).
    fn mark_acquired(&mut self) {
        self.acquired = true;
    }

    /// Assign_Target's first statement (`0x006FCDC4`) and the off-mission
    /// clear (`0x006FA5F0`).
    pub(crate) fn clear_acquired(&mut self) {
        self.acquired = false;
    }

    /// Install a fixture's timer.
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn set_passive_scan_timer_for_test(&mut self, timer: MissionTimer) {
        self.timer = timer;
    }

    /// A fixture's handle on the timer.
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn timer_mut_for_test(&mut self) -> &mut MissionTimer {
        &mut self.timer
    }

    /// Install a fixture's last scan frame.
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn set_last_target_scan_frame_for_test(&mut self, frame: u32) {
        self.last_scan_frame = frame;
    }

    /// Install a fixture's acquisition mark.
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn set_passively_acquired_target_for_test(&mut self, acquired: bool) {
        self.acquired = acquired;
    }
}

pub(super) trait ScanHost {
    /// A weapon GetWeapon (vt+0x3F8) returned.
    type Weapon: Copy;

    /// `Frame` (`0x00A8ED84`).
    fn frame(&self) -> i32;
    /// `+0x4FC`, the last-scan frame.
    fn set_last_scan_frame(&mut self, frame: i32);
    /// The committed mission (`+0xAC`) is Area Guard.
    fn committed_area_guard(&self) -> bool;
    /// Scenario `RandomRanged(min, max)`.
    fn random_ranged(&mut self, min: i32, max: i32) -> i32;
    /// `[General] GuardAreaTargetingDelay=` (`Rules+0xE04`) or
    /// `NormalTargetingDelay=` (`Rules+0xE08`).
    fn targeting_delay(&self, area_guard: bool) -> i32;
    /// The targeting timer (`+0x180` start, `+0x188` duration).
    fn arm_targeting_timer(&mut self, start: i32, duration: i32);
    /// `+0x2B4`.
    fn target(&self) -> Option<TargetKind>;
    /// `+0x50C`: the current target came from this object's passive scan.
    fn passively_acquired(&self) -> bool;
    /// SelectWeapon (vt+0x2E4).
    fn select_weapon(&mut self, target: Option<TargetKind>) -> i32;
    /// GetFireError (vt+0x3C0) with the range test.
    fn fire_error(&mut self, target: Option<TargetKind>, weapon: i32) -> FireError;
    /// A SpawnManager (`+0x2D0`).
    fn has_spawn_manager(&self) -> bool;
    /// `SpawnManagerClass::ClearAllTargets @ 0x006B7BB0`.
    fn clear_spawn_targets(&mut self);
    /// Assign_Target (vt+0x3C8).
    fn assign_target(&mut self, target: Option<TargetKind>);
    /// Greatest_Threat (vt+0x3C4) with this mask around the scan anchor.
    fn greatest_threat(&mut self, mask: ScanMission) -> Option<TargetKind>;
    /// The type's `DistributedFire=` (`+0x6B0`, through vt+0x84).
    fn distributed_fire(&mut self) -> bool;
    /// `TechnoClass::DistributeFire @ 0x00709550`.
    fn distribute_fire(&mut self);
    /// GetWeapon (vt+0x3F8)'s WeaponType at this index.
    fn weapon(&mut self, index: i32) -> Option<Self::Weapon>;
    /// The weapon's projectile has `Inaccurate=` (BulletType `+0x2A2`).
    fn inaccurate(&self, weapon: Self::Weapon) -> bool;
    /// The object is a Techno (AbstractFlags `+0x14` bit 0).
    fn is_techno(&self, target: TargetKind) -> bool;
    /// `TechnoClass::EstimateDamage @ 0x006FDB80`.
    fn estimated_damage(&mut self, target: TargetKind, weapon: Self::Weapon) -> i32;
    /// `SUB [target+0x70], amount` (`0x007099B5`).
    fn debit(&mut self, target: TargetKind, amount: i32);
}

/// `TechnoClass::Retaliate_And_Scan @ 0x00709820(coords, mask)`:
/// 1. stamp `+0x4FC`, then one unconditional Scenario `RandomRanged(0, 2)`
///    and the targeting timer `{Frame, delay + draw}`, the delay by the
///    committed mission (`0x0070982E..0x007098B6`);
/// 2. a target the passive scan installed is dropped on ILLEGAL (5), CANT (6)
///    or RANGE (8); CANT with a SpawnManager clears the spawns' targets and
///    keeps it (`0x007098B9..0x00709912`);
/// 3. with no target: Greatest_Threat, then `DistributedFire=` hands off to
///    `DistributeFire` and returns; otherwise the pick is assigned and, for a
///    Techno pick and an accurate weapon, its estimate is debited
///    (`0x00709918..0x007099B5`);
/// 4. returns whether a target is held (`0x007099B8..0x007099C4`). It never
///    writes `+0x50C`; its passive caller does.
pub(super) fn retaliate_and_scan<H: ScanHost>(host: &mut H, mask: ScanMission) -> bool {
    let frame = host.frame();
    host.set_last_scan_frame(frame);
    let area_guard = host.committed_area_guard();
    let draw = host.random_ranged(0, 2);
    let duration = host.targeting_delay(area_guard).wrapping_add(draw);
    host.arm_targeting_timer(frame, duration);

    if host.target().is_some() && host.passively_acquired() {
        let weapon = host.select_weapon(host.target());
        match host.fire_error(host.target(), weapon) {
            FireError::Cant if host.has_spawn_manager() => host.clear_spawn_targets(),
            FireError::Cant | FireError::Illegal | FireError::Range => host.assign_target(None),
            _ => {}
        }
    }

    if host.target().is_none() {
        let pick = host.greatest_threat(mask);
        if host.distributed_fire() {
            host.distribute_fire();
            return host.target().is_some();
        }
        if pick.is_some() {
            host.assign_target(pick);
        }
        let weapon_index = host.select_weapon(host.target());
        let techno_pick = pick.filter(|pick| host.is_techno(*pick));
        let weapon = host.weapon(weapon_index);
        if let (Some(pick), Some(weapon)) = (techno_pick, weapon)
            && !host.inaccurate(weapon)
        {
            let amount = host.estimated_damage(pick, weapon);
            host.debit(pick, amount);
        }
    }
    host.target().is_some()
}

/// Run the scan for `id` with the caller's mask literal.
pub(crate) fn scan(
    sim: &mut Simulation,
    id: u64,
    rules: &RuleSet,
    mask: ScanMission,
    ctx: ObjectAiCtx<'_>,
    scan_coord: Option<[i32; 3]>,
) -> bool {
    if !sim.substrate.entities.contains(id) {
        return false;
    }
    retaliate_and_scan(
        &mut WorldScan {
            sim,
            rules,
            id,
            ctx,
            scan_coord,
        },
        mask,
    )
}

/// The passive block of `TechnoClass::AI_Update` (`0x006FA65A..0x006FA6EE`),
/// after mission dispatch:
/// - only once the targeting timer has run out (a stopped timer counts as
///   run out only with a zero duration);
/// - an attack-mover (MegaMissionIsAttackMove, vt+0x4C4) runs its
///   Arrival_Target_Handler (vt+0x4CC) instead
///   ([`Simulation::foot_arrival_target_handler`], `0x006FA681..0x006FA695`);
/// - otherwise only on Move (2), Harvest (10) or Guard (5);
/// - then [`passive_target_acquire`]'s body, which the block inlines.
///
/// The block has no Health gate: an infantry corpse, still IsAlive, scans
/// too.
pub(super) fn passive_acquire_step(
    sim: &mut Simulation,
    id: u64,
    rules: &RuleSet,
    ctx: ObjectAiCtx<'_>,
) {
    let Some(entity) = sim.substrate.entities.get(id) else {
        return;
    };
    match targeting_slot(entity, sim.session.binary_frame()) {
        TargetingSlot::Skip => {}
        TargetingSlot::ArrivalHandler => sim.foot_arrival_target_handler(id, rules, ctx),
        TargetingSlot::PassiveAcquire(_) => {
            passive_target_acquire(sim, id, rules, ctx);
        }
    }
}

/// Where the passive block goes ([`targeting_slot`]).
pub(super) enum TargetingSlot {
    /// The targeting timer still runs, or the mission is none of the three.
    Skip,
    /// An attack-mover's Arrival_Target_Handler.
    ArrivalHandler,
    /// [`passive_target_acquire`] on this mission.
    PassiveAcquire(MissionType),
}

/// Where the passive block of `TechnoClass::AI_Update` goes for `entity` at
/// `now` ([`passive_acquire_step`]).
pub(super) fn targeting_slot(
    entity: &crate::sim::game_entity::GameEntity,
    now: u32,
) -> TargetingSlot {
    if !entity.passive_scan.timer().due(now) {
        return TargetingSlot::Skip;
    }
    if entity.mega_mission.is_attack_move() {
        return TargetingSlot::ArrivalHandler;
    }
    let mission = entity.passive_acquire_mission();
    if !matches!(
        mission,
        MissionType::Move | MissionType::Guard | MissionType::Harvest
    ) {
        return TargetingSlot::Skip;
    }
    TargetingSlot::PassiveAcquire(mission)
}

/// `TechnoClass::Passive_Target_Acquire @ 0x00709480`: false unless
/// [`passive_acquire_gate`] passes (it reads the current mission itself);
/// then `+0x4FC = Frame`, the scan with mask 1, and `+0x50C = 1` when the scan
/// holds a target that differs from the one before it (`0x007094C8`).
/// Answers whether the scan holds a target. TeleportLocomotionClass's
/// TimerCheck (`0x00719C31`) calls it; AI_Update's passive block inlines the
/// same body.
pub(crate) fn passive_target_acquire(
    sim: &mut Simulation,
    id: u64,
    rules: &RuleSet,
    ctx: ObjectAiCtx<'_>,
) -> bool {
    let Some(mission) = sim
        .substrate
        .entities
        .get(id)
        .map(|entity| entity.passive_acquire_mission())
    else {
        return false;
    };
    if !passive_acquire_gate(sim, id, rules, mission) {
        return false;
    }
    let now = sim.session.binary_frame();
    let Some(entity) = sim.substrate.entities.get_mut(id) else {
        return false;
    };
    entity.passive_scan.stamp_scan(now);
    let before = entity.attack_target.target();
    let held = scan(sim, id, rules, ScanMission::Guard, ctx, None);
    if let Some(entity) = sim.substrate.entities.get_mut(id)
        && held
        && entity.attack_target.target() != before
    {
        entity.passive_scan.mark_acquired();
    }
    held
}

/// `TechnoClass::PassiveAcquireGate @ 0x00709290`, in native order:
/// 1. a computer-owned Foot with no target, in a team whose TeamType is
///    `Aggressive=yes` and `Suicide=no`, passes on Move without
///    CanAcquireTarget (`0x0070929A..0x007092EC`);
/// 2. otherwise [`can_acquire_target`] must pass;
/// 3. on Move, an object parked on its NavCom's cell passes, and a
///    `BalloonHover=` object that is no Foot fails ([`parked_on_move`],
///    `0x00709301..0x007093D3`);
/// 4. `OpportunityFire=` (`+0x6AF`) passes (`0x007093D5`);
/// 5. any mission but Guard fails;
/// 6. on Guard, the weapon in slot `SprayAttack ? 0 : 1` (vt+0x3E4,
///    `0x0070DD70`) refuses when it has `AreaFire=` (`+0x150`) and
///    SelectWeapon picks that slot for the current target
///    (`0x007093F8..0x00709449`).
///
/// Native execution: `tools/spatial_oracle/passive_acquire_gate.py` runs the
/// original gate and CanAcquireTarget with supplied queries ([`tests`]).
pub(super) fn passive_acquire_gate(
    sim: &Simulation,
    id: u64,
    rules: &RuleSet,
    mission: MissionType,
) -> bool {
    let Some(entity) = sim.substrate.entities.get(id) else {
        return false;
    };
    let is_foot = matches!(
        entity.category(),
        EntityCategory::Unit | EntityCategory::Infantry | EntityCategory::Aircraft
    );
    if mission == MissionType::Move
        && entity.attack_target.target().is_none()
        && is_foot
        && !sim.owner_is_human(entity.owner())
        && sim
            .team_script_vm
            .member_team_type(id)
            .is_some_and(|team_type| !team_type.suicide && team_type.aggressive)
    {
        return true;
    }
    if !can_acquire_target(sim, id, rules) {
        return false;
    }
    let Some(obj) = sim.interner.object_type(entity.type_ref(), rules) else {
        return false;
    };
    if mission == MissionType::Move
        && let Some(passes) = parked_on_move(entity, obj, is_foot)
    {
        return passes;
    }
    if obj.opportunity_fire {
        return true;
    }
    if mission != MissionType::Guard {
        return false;
    }
    let slot = if obj.spray_attack { 0 } else { 1 };
    let target = entity.attack_target.target();
    let area_fire = fire_subject(sim, rules, id, target, slot, None)
        .and_then(|subject| subject.weapon_at(slot).map(|weapon| weapon.area_fire))
        .unwrap_or(false);
    !(area_fire && select_weapon(sim, rules, id, target) == slot)
}

/// The gate's Move arms (`0x00709301..0x007093D3`), for an object whose
/// NavCom (`+0x5A4`) is its own cell and that holds no waypoint-planning token
/// with nodes (`+0x514`, `0x00636DC0`): a `BalloonHover=` Foot (`+0xD6A`,
/// `0x00709317`) or a Unit whose type is `IsSimpleDeployer=` (`+0xE13`,
/// `0x00709394`) passes. That is a Rocketeer, Lunar infantryman, Kirov or
/// Siege Chopper parked where a Move order left it: native
/// `FootClass::Mission_Move @ 0x004D4200` keeps Move while the NavCom is set
/// (read from the body, not executed). A `BalloonHover=` object that is no
/// Foot fails the gate outright (`0x00709333`). The own cell is vt+0x1BC,
/// `ObjectClass::GetCell @ 0x005F6960` for every Foot class: the cell holding
/// the Location, which `position.rx`/`ry` name. Answers the gate's verdict, or
/// `None` to go on.
///
/// RESIDUAL: VERA has no waypoint-planning mode (the token's setter
/// `0x00705D10` is called only from the planning code), so every object
/// passes as with a token of no nodes. Trigger: a player planning any
/// waypoint path for these types (its count positive). Effect: native keeps
/// such a parked object from scanning. Frequency: waypoint mode only.
///
/// RESIDUAL (scatter frame): when a parked Jumpjet yields its cell's air slot,
/// native's `Set_Destination` writes the new NavCom inside that frame's
/// Process; VERA's adapter writes it in the next frame's locomotor step, after
/// that frame's passive block, whose gate still sees the old cell. Trigger:
/// two Jumpjets contending for one cell's air slot while a scan is due.
/// Effect: one early scan (its Scenario draw and timer). Frequency: rare.
fn parked_on_move(
    entity: &crate::sim::game_entity::GameEntity,
    obj: &crate::rules::object_type::ObjectType,
    is_foot: bool,
) -> Option<bool> {
    let on_own_cell = entity.navigation.nav_com()
        == Some(crate::sim::components::NavTargetRef::cell(
            entity.position.rx(),
            entity.position.ry(),
        ));
    if obj.balloon_hover {
        if !is_foot {
            return Some(false);
        }
        if on_own_cell {
            return Some(true);
        }
    }
    (entity.category() == EntityCategory::Unit && obj.is_simple_deployer && on_own_cell)
        .then_some(true)
}

/// `TechnoClass::CanAcquireTarget @ 0x007091D0`: false when
/// - the object's Temporal holds a target (vt+0x1DC, `0x0070C5D0`);
/// - it is a slave (`+0x2DC`);
/// - its type has `CanPassiveAquire=no` (`+0xD99`);
/// - it is a building whose type `CanBeOccupied=` (`+0x157B`) with no
///   occupants (vt+0x408, `0x004581F0`);
/// - its CaptureManager is full (`0x004722A0`);
/// - it is an `Engineer=` infantryman (vt+0x330, `0x005224D0`) of a human
///   house;
/// - it is not armed (vt+0x2AC).
///
/// Every term is a pure predicate, so their order is not observable.
pub(super) fn can_acquire_target(sim: &Simulation, id: u64, rules: &RuleSet) -> bool {
    let Some(entity) = sim.substrate.entities.get(id) else {
        return false;
    };
    if entity.temporal.is_warping_someone() {
        return false;
    }
    if entity.slave.owner().is_some() {
        return false;
    }
    let Some(obj) = sim.interner.object_type(entity.type_ref(), rules) else {
        return false;
    };
    if !obj.can_passive_acquire {
        return false;
    }
    if entity.category() == EntityCategory::Structure
        && obj.can_be_occupied
        && entity
            .passenger_role
            .cargo()
            .is_none_or(|cargo| cargo.count() == 0)
    {
        return false;
    }
    if entity
        .capture_manager
        .as_ref()
        .is_some_and(|manager| manager.is_full())
    {
        return false;
    }
    if entity.category() == EntityCategory::Infantry
        && obj.engineer
        && sim.owner_is_human(entity.owner())
    {
        return false;
    }
    combat_weapon::is_armed(entity, obj)
}

fn fire_subject<'a>(
    sim: &'a Simulation,
    rules: &'a RuleSet,
    id: u64,
    target: Option<TargetKind>,
    weapon_index: i32,
    overlay_registry: Option<&'a OverlayTypeRegistry>,
) -> Option<FireSubject<'a>> {
    let firer = sim.substrate.entities.get(id)?;
    let obj = sim.interner.object_type(firer.type_ref(), rules)?;
    Some(FireSubject {
        world: sim,
        rules,
        overlay_registry,
        fog: Some(sim.fog.fire_view(sim.house_alliances.as_map())),
        firer,
        obj,
        target,
        weapon_index,
    })
}

/// GetWeapon (vt+0x3F8)'s WeaponType at `index`; an occupied building's is
/// its firing occupant's for every index (`0x004526F0`).
pub(crate) fn weapon_at_index<'r>(
    sim: &Simulation,
    rules: &'r RuleSet,
    id: u64,
    index: i32,
) -> Option<&'r WeaponType> {
    let subject = fire_subject(sim, rules, id, None, index, None)?;
    // `weapon_at` borrows the subject; re-resolve through the rules so the
    // weapon outlives it.
    rules.weapon(&subject.weapon_at(index)?.id)
}

/// SelectWeapon (vt+0x2E4) for `target`.
pub(super) fn select_weapon(
    sim: &Simulation,
    rules: &RuleSet,
    id: u64,
    target: Option<TargetKind>,
) -> i32 {
    let Some(firer) = sim.substrate.entities.get(id) else {
        return 0;
    };
    let Some(obj) = sim.interner.object_type(firer.type_ref(), rules) else {
        return 0;
    };
    let cells = sim
        .resolved_terrain
        .as_ref()
        .map(crate::map::resolved_terrain::NativeCellQuery::canonical);
    //7098D2/709971 dispatch the shared SelectWeapon ladder on an arbitrary
    //target Object. Terrain facts belong to that owner, not a second AI adapter.
    combat_weapon::select_weapon_index_against_in_query(
        rules,
        firer,
        obj,
        &combat_weapon::attacker_facts(firer, obj),
        target.as_ref(),
        sim,
        &sim.interner,
        cells.as_ref(),
        Some(sim.house_alliances.as_map()),
    )
    .unwrap_or(0)
}

/// `TechnoClass::CanFireAt @ 0x006F77B0` (vt+0x3A8): InRange for `target`
/// with `weapon_index`, measured from the object's own coordinate.
pub(crate) fn can_fire_at(
    sim: &Simulation,
    rules: &RuleSet,
    id: u64,
    target: TargetKind,
    weapon_index: i32,
    overlay_registry: Option<&OverlayTypeRegistry>,
) -> bool {
    fire_subject(sim, rules, id, Some(target), weapon_index, overlay_registry)
        .is_some_and(|subject| subject.in_range())
}

/// GetFireError (vt+0x3C0) for `target` with `weapon_index`; `check_range`
/// false is vt+0x3BC. Range-bearing callers carry the match's overlay types
/// into the shared InRange line query (6F7642 -> 4CC310 -> 4CC360).
pub(super) fn fire_error_at(
    sim: &Simulation,
    rules: &RuleSet,
    id: u64,
    target: Option<TargetKind>,
    weapon_index: i32,
    check_range: bool,
    overlay_registry: Option<&OverlayTypeRegistry>,
) -> FireError {
    fire_subject(sim, rules, id, target, weapon_index, overlay_registry)
        .map_or(FireError::Illegal, |subject| {
            subject.fire_error(check_range)
        })
}

impl Simulation {
    /// GetFireError (vt+0x3C0, range asked when `check_range`) of the slot
    /// SelectWeapon (vt+0x2E4) picks for `target`.
    /// `TechnoClass::What_Action_OnObject @ 0x006FFEC0` asks it with range
    /// (`0x0070022D`, `0x00700542`): ILLEGAL ends an unforced Attack action
    /// (`0x00700548`) unless the Infiltrate arm (`0x007004A0..0x00700531`)
    /// overrides it. `Coordinate_Attack` asks it of the team leader with
    /// range (`0x006EB5B0..0x006EB5CD`).
    pub fn selected_weapon_fire_error(
        &self,
        rules: &RuleSet,
        id: u64,
        target: TargetKind,
        check_range: bool,
        overlay_registry: Option<&OverlayTypeRegistry>,
    ) -> FireError {
        let weapon = select_weapon(self, rules, id, Some(target));
        fire_error_at(
            self,
            rules,
            id,
            Some(target),
            weapon,
            check_range,
            overlay_registry,
        )
    }
}

/// The production scan host.
struct WorldScan<'s, 'r> {
    sim: &'s mut Simulation,
    rules: &'r RuleSet,
    id: u64,
    ctx: ObjectAiCtx<'r>,
    scan_coord: Option<[i32; 3]>,
}

impl WorldScan<'_, '_> {
    fn entity(&self) -> &crate::sim::game_entity::GameEntity {
        self.sim
            .substrate
            .entities
            .get(self.id)
            .expect("the scanning object stays present")
    }
}

impl<'r> ScanHost for WorldScan<'_, 'r> {
    type Weapon = &'r WeaponType;

    fn frame(&self) -> i32 {
        self.sim.session.binary_frame() as i32
    }

    fn set_last_scan_frame(&mut self, frame: i32) {
        if let Some(entity) = self.sim.substrate.entities.get_mut(self.id) {
            entity.passive_scan.stamp_scan(frame as u32);
        }
    }

    /// `Retaliate_And_Scan @ 0x00709820`'s test `+0xAC == 0xB` (`0x0070983A`).
    fn committed_area_guard(&self) -> bool {
        self.entity().mission.current().raw() == AREA_GUARD_MISSION
    }

    fn random_ranged(&mut self, min: i32, max: i32) -> i32 {
        self.sim.scenario_rng.next_range_i32_inclusive(min, max)
    }

    fn targeting_delay(&self, area_guard: bool) -> i32 {
        let general = &self.rules.general;
        if area_guard {
            general.guard_area_targeting_delay
        } else {
            general.normal_targeting_delay
        }
    }

    fn arm_targeting_timer(&mut self, start: i32, duration: i32) {
        if let Some(entity) = self.sim.substrate.entities.get_mut(self.id) {
            entity.passive_scan.arm_timer(start as u32, duration as u32);
        }
    }

    /// TarCom `+0x2B4`, as `Retaliate_And_Scan @ 0x00709820` reads it.
    fn target(&self) -> Option<TargetKind> {
        self.entity().attack_target.target()
    }

    fn passively_acquired(&self) -> bool {
        self.entity().passive_scan.acquired()
    }

    fn select_weapon(&mut self, target: Option<TargetKind>) -> i32 {
        select_weapon(self.sim, self.rules, self.id, target)
    }

    fn fire_error(&mut self, target: Option<TargetKind>, weapon: i32) -> FireError {
        // Original7098DA PUSH1 before7098E6 GetFireError(vt+3C0): the
        // held passive target must pass the live wall-aware range query.
        fire_error_at(
            self.sim,
            self.rules,
            self.id,
            target,
            weapon,
            true,
            self.ctx.overlay_registry,
        )
    }

    /// The SpawnManager `+0x2D0` that `Retaliate_And_Scan @ 0x00709820` tests.
    fn has_spawn_manager(&self) -> bool {
        self.entity().spawn_manager.is_some()
    }

    fn clear_spawn_targets(&mut self) {
        crate::sim::spawn_manager::clear_all_spawn_targets(
            self.sim,
            self.id,
            self.rules,
            None,
            self.ctx.effects,
        );
    }

    fn assign_target(&mut self, target: Option<TargetKind>) {
        let _ = self.sim.assign_target_represented(
            self.id,
            target,
            self.rules,
            self.ctx.overlay_registry,
            self.ctx.effects,
        );
    }

    /// Through the object's `+0x3C4` override, or this pass's batched answer
    /// ([`super::scan_batch`]).
    fn greatest_threat(&mut self, mask: ScanMission) -> Option<TargetKind> {
        let (rules, registry) = (self.rules, self.ctx.overlay_registry);
        let outcome =
            match self
                .sim
                .take_batched_scan(rules, registry, self.id, mask, self.scan_coord)
            {
                Some(outcome) => outcome,
                None => self
                    .sim
                    .threat_scan(rules, registry, self.id, mask, self.scan_coord),
            };
        Simulation::commit_threat_scan(
            &mut self.sim.scenario_rng,
            &mut self.sim.substrate,
            self.id,
            outcome,
        )
        .map(TargetKind::Entity)
    }

    fn distributed_fire(&mut self) -> bool {
        self.sim
            .interner
            .object_type(self.entity().type_ref(), self.rules)
            .is_some_and(|obj| obj.distributed_fire)
    }

    fn distribute_fire(&mut self) {
        let assignment = self
            .sim
            .substrate
            .entities
            .get_mut(self.id)
            .and_then(|entity| entity.select_distributed_fire_target());
        //709550's empty-list branch makes no setter call. Nonempty but
        // nonpositive scores still call the concrete AssignTarget(NULL).
        if let Some(target) = assignment {
            self.assign_target(target);
        }
    }

    fn weapon(&mut self, index: i32) -> Option<&'r WeaponType> {
        weapon_at_index(self.sim, self.rules, self.id, index)
    }

    fn inaccurate(&self, weapon: &'r WeaponType) -> bool {
        weapon
            .projectile
            .as_deref()
            .and_then(|id| self.rules.projectile(id))
            .is_some_and(|projectile| projectile.inaccurate)
    }

    fn is_techno(&self, target: TargetKind) -> bool {
        //70997B tests AbstractFlags bit0 before estimated-health debit.
        //The legacy Entity arm also carries Terrain Object IDs.
        matches!(target, TargetKind::Entity(id) if self.sim.substrate.entities.contains(id))
    }

    fn estimated_damage(&mut self, target: TargetKind, weapon: &'r WeaponType) -> i32 {
        let TargetKind::Entity(target_id) = target else {
            return 0;
        };
        crate::sim::combat::estimated_damage_on(self.sim, self.rules, self.id, target_id, weapon)
    }

    fn debit(&mut self, target: TargetKind, amount: i32) {
        if let TargetKind::Entity(target_id) = target
            && let Some(target) = self.sim.substrate.entities.get_mut(target_id)
        {
            target.estimated_health.debit(amount);
        }
    }
}

impl Simulation {
    /// `id`'s concrete Greatest_Threat against the live world. The immutable
    /// selector reports whether Foot was reached; this adapter commits its empty
    /// result latch write before the caller performs any target assignment.
    /// Per-class Greatest_Threat (vtable `+0x3C4`); the latch write is
    /// `FootClass::Greatest_Threat @ 0x004D9920`'s `+0x688` clear (`0x004D9951..0x004D9955`).
    pub(crate) fn greatest_threat_represented(
        &mut self,
        rules: &RuleSet,
        overlay_registry: Option<&OverlayTypeRegistry>,
        id: u64,
        mask: ScanMission,
        scan_coord: Option<[i32; 3]>,
    ) -> Option<u64> {
        let result = self.threat_scan(rules, overlay_registry, id, mask, scan_coord);
        Self::commit_threat_scan(&mut self.scenario_rng, &mut self.substrate, id, result)
    }

    /// `id`'s Greatest_Threat
    /// ([`crate::sim::combat::combat_targeting::greatest_threat_for_entity`])
    /// against the world as it stands.
    pub(super) fn threat_scan(
        &self,
        rules: &RuleSet,
        overlay_registry: Option<&OverlayTypeRegistry>,
        id: u64,
        mask: ScanMission,
        scan_coord: Option<[i32; 3]>,
    ) -> crate::sim::combat::greatest_threat::ThreatScanOutcome {
        crate::sim::combat::combat_targeting::greatest_threat_for_entity(
            &self.substrate.entities,
            &self.substrate.occupancy,
            rules,
            &self.interner,
            id,
            Some(&self.fog),
            self.resolved_terrain.as_ref(),
            self.playfield_bounds.is_some(),
            mask,
            self.zone_grid.as_ref(),
            crate::sim::combat::line_of_fire::LineOfFireInputs {
                overlay_grid: self.overlay_grid.as_ref(),
                overlay_registry,
                alliances: Some(self.house_alliances.as_map()),
            },
            Some(self),
            scan_coord,
        )
    }

    /// A scan's writes to its scanner, in the order the adapter commits them,
    /// and its pick.
    fn commit_threat_scan(
        scenario_rng: &mut SimRng,
        substrate: &mut ObjectSubstrate,
        id: u64,
        mut result: crate::sim::combat::greatest_threat::ThreatScanOutcome,
    ) -> Option<u64> {
        // The computer's disguise detection draws during the scan
        // (Evaluate_Candidate 0x006F851C).
        if let Some(rng) = result.take_scenario_rng() {
            *scenario_rng = rng;
        }
        if let Some(collected) = result.take_collected()
            && let Some(entity) = substrate.entities.get_mut(id)
        {
            entity.replace_distributed_fire_candidates(collected);
        }
        if result.foot_wrapper_entered()
            && let Some(entity) = substrate.entities.get_mut(id)
        {
            entity
                .foot_retarget_after_stop
                .finish_scan(result.target().is_some());
        }
        result.target()
    }
}

/// An object's `Greatest_Threat` through its `+0x3C4` override, called
/// directly with the caller's literal mask around the object's own Location:
/// a team leader's for script actions 0,16 and 57 (`0x006ED15E`,
/// `0x006ECE06`, `0x006F0253`),
/// and an aircraft's own for Mission_Hunt (`0x00414B24`, `0x00414B64`).
pub(crate) fn direct_greatest_threat(
    sim: &mut Simulation,
    rules: &RuleSet,
    overlay_registry: Option<&OverlayTypeRegistry>,
    scanner: u64,
    mask: ScanMission,
) -> Option<u64> {
    // These callers explicitly copy Object+9C..A4, not virtual GetCoords.
    // Preserve the shared direct-call owner for all of them. Original Steam
    // gamemd:6ED132,6ECDEF,6F0222 and414B3D.
    let coord = crate::sim::movement::ground_pose::object_location(
        sim.substrate.entities.get(scanner)?,
        sim.resolved_terrain.as_ref(),
    );
    sim.greatest_threat_represented(
        rules,
        overlay_registry,
        scanner,
        mask,
        Some([coord.x, coord.y, coord.z]),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;

    /// Original7098DA pushes1 before GetFireError(vt+3C0): a held passive
    /// target must pass the same wall-aware InRange as ordinary fire. The
    /// existing171-row scanner corpus consumes that argument/order with
    /// supplied callees; this test joins it to the real stock range query.
    #[test]
    fn stock_passive_scan_drops_a_held_target_behind_a_new_wall() {
        let Some(fixture) = crate::rules::retail_ini_fixture::retail_battle_rules() else {
            return;
        };
        let rules = &fixture.rules;
        let registry =
            OverlayTypeRegistry::from_ini(&fixture.processed_rules, Some(&fixture.fixed_art));
        let weapon = rules.weapon("M60").unwrap();
        assert_eq!(rules.object("E1").unwrap().primary(), Some("M60"));
        assert_eq!(weapon.projectile.as_deref(), Some("InvisibleLow"));
        assert!(rules.projectile("InvisibleLow").unwrap().subject_to_walls);
        assert!(
            !rules
                .warhead(weapon.warhead.as_deref().unwrap())
                .unwrap()
                .wall
        );
        let wall = registry.id_for_name("GAWALL").unwrap();
        assert!(registry.flags(wall).unwrap().wall);

        for blocked in [false, true] {
            let mut sim = crate::sim::arena_fixture::rival_arena(rules, 626);
            let scanner = sim
                .spawn_object("E1", "Americans", 12, 12, 0, rules)
                .unwrap();
            let victim = sim
                .spawn_object("E1", "Russians", 15, 12, 0, rules)
                .unwrap();
            let owner = sim.substrate.entities.get(scanner).unwrap().owner();
            sim.fog.mark_visible_for_owner(owner, 15, 12);
            sim.session.set_binary_frame_for_test(100);
            sim.substrate
                .entities
                .get_mut(scanner)
                .unwrap()
                .passive_scan
                .timer_mut_for_test()
                .clear();
            let ctx = ObjectAiCtx {
                overlay_registry: Some(&registry),
                ..ObjectAiCtx::default()
            };
            passive_acquire_step(&mut sim, scanner, rules, ctx);
            let actor = sim.substrate.entities.get(scanner).unwrap();
            assert_eq!(
                actor.attack_target.target(),
                Some(TargetKind::Entity(victim))
            );
            assert!(
                actor.passive_scan.acquired(),
                "the real passive scan installed this target"
            );

            let mut overlays = crate::sim::overlay_grid::OverlayGrid::new(32, 32);
            if blocked {
                overlays.place_overlay(13, 12, wall, 0);
            }
            sim.overlay_grid = Some(overlays);
            let actor = sim.substrate.entities.get(scanner).unwrap();
            let subject = FireSubject {
                world: &sim,
                rules,
                overlay_registry: Some(&registry),
                fog: Some(sim.fog.fire_view(sim.house_alliances.as_map())),
                firer: actor,
                obj: rules.object("E1").unwrap(),
                target: Some(TargetKind::Entity(victim)),
                weapon_index: select_weapon(&sim, rules, scanner, Some(TargetKind::Entity(victim))),
            };
            assert_eq!(
                subject.in_range(),
                !blocked,
                "stock wall/no-wall geometry control"
            );
            sim.substrate
                .entities
                .get_mut(scanner)
                .unwrap()
                .passive_scan
                .timer_mut_for_test()
                .clear();
            let mut expected_scenario = sim.scenario_rng.clone();
            let jitter = expected_scenario.next_range_u32_inclusive(0, 2);
            let main = sim.main_rng.state();
            let mapgen = sim.mapgen_rng.state();
            passive_acquire_step(&mut sim, scanner, rules, ctx);
            let actor = sim.substrate.entities.get(scanner).unwrap();
            assert_eq!(actor.passive_scan.last_scan_frame(), 100);
            assert_eq!(actor.passive_scan.timer().start_frame(), 100);
            assert_eq!(
                actor.passive_scan.timer().duration(),
                rules.general.normal_targeting_delay as u32 + jitter
            );
            assert_eq!(sim.scenario_rng.state(), expected_scenario.state());
            assert_eq!(sim.main_rng.state(), main);
            assert_eq!(sim.mapgen_rng.state(), mapgen);
            assert_eq!(
                actor.attack_target.target(),
                (!blocked).then_some(TargetKind::Entity(victim)),
                "a passive target behind an unbreakable wall must be dropped, blocked={blocked}",
            );
            assert_eq!(actor.passive_scan.acquired(), !blocked);
            let after_scan = sim.scenario_rng.state();
            passive_acquire_step(&mut sim, scanner, rules, ctx);
            assert_eq!(
                sim.scenario_rng.state(),
                after_scan,
                "the rearmed timer gates another scan"
            );
        }
    }

    /// Neither `Retaliate_And_Scan` (`0x00709820`, vt+0x39C for every class)
    /// nor `Greatest_Threat` tests ammo or `Is_Armed`. A docked aircraft with
    /// no ammo left still passes Foot Mission_AreaGuard's CanAcquireTarget
    /// (`0x004D6ED9`; `Is_Armed` has no ammo term), and Foot Mission_Hunt
    /// scans an unarmed unit (`0x004D5392`). Both run
    /// `FootClass::Greatest_Threat` (`0x004D9920`), whose empty scan clears the
    /// `+0x688` latch (`0x004D9951..0x004D9955`). Neither picks the enemy in
    /// reach: their flags word holds no class bit (`0x006F821A`).
    #[test]
    fn an_empty_scan_clears_the_foot_latch_without_ammo_or_a_weapon() {
        use crate::rules::ini_parser::IniFile;
        use crate::sim::house_state::HouseState;
        let rules = RuleSet::from_ini(&IniFile::from_str(
            "[VehicleTypes]\n0=TANK\n[AircraftTypes]\n0=PLANE\n\
             [InfantryTypes]\n[BuildingTypes]\n\
             [PLANE]\nStrength=150\nSpeed=14\nAmmo=1\nLandable=yes\nFighter=yes\n\
             Primary=TestGun\nLocomotor={4A582746-9839-11D1-B709-00A024DDAFD1}\n\
             [TestGun]\nDamage=10\nRange=6\nROF=60\nProjectile=TestShot\nWarhead=TestWH\n\
             [TestShot]\nROT=0\n[TestWH]\nVerses=100%\n\
             [TANK]\nStrength=1000\nLocomotor={4A582741-9839-11D1-B709-00A024DDAFD1}\n",
        ))
        .expect("rules");
        let mut sim = Simulation::new();
        // The enemy is human and not passive, so its objects stay in the
        // scans (`0x006F826E`).
        for (index, (name, human)) in [("Americans", false), ("Russians", true)]
            .into_iter()
            .enumerate()
        {
            let id = sim.interner.intern(name);
            let house = HouseState::new(id, index as u8, None, human, 0, 10);
            sim.houses.register(house);
        }
        let plane = sim
            .spawn_object_at_height("PLANE", "Americans", 10, 10, 0, 0, &rules)
            .expect("aircraft");
        let tank = sim
            .spawn_object("TANK", "Americans", 20, 10, 0, &rules)
            .expect("unarmed tank");
        sim.spawn_object("TANK", "Russians", 15, 10, 0, &rules)
            .expect("enemy in reach");
        let entity = sim.substrate.entities.get_mut(plane).unwrap();
        entity.ammo_mut().unwrap().set_current_for_test(0);
        for (scanner, mask) in [(plane, ScanMission::AreaGuard), (tank, ScanMission::Hunt)] {
            sim.substrate
                .entities
                .get_mut(scanner)
                .unwrap()
                .foot_retarget_after_stop
                .mark_stopped_cannot_fire();
            let held = scan(
                &mut sim,
                scanner,
                &rules,
                mask,
                ObjectAiCtx::default(),
                None,
            );
            let entity = sim.substrate.entities.get(scanner).unwrap();
            assert!(!held, "{scanner}: no class bit, no pick");
            assert_eq!(entity.attack_target.target(), None, "{scanner}");
            assert!(
                !entity.foot_retarget_after_stop(),
                "{scanner}: the Foot scan ran and cleared +0x688"
            );
        }
    }

    #[test]
    fn scan_estimates_only_real_techno_targets() {
        use crate::rules::ini_parser::IniFile;
        use crate::sim::game_entity::GameEntity;
        use crate::sim::terrain_object::TerrainObjectState;

        let rules =
            RuleSet::from_ini(&IniFile::from_str("[VehicleTypes]\n0=TEST\n[TEST]\n")).unwrap();
        let mut sim = Simulation::new();
        sim.substrate
            .entities
            .insert(GameEntity::test_default(1, "TEST", "Americans", 5, 5));
        let tree_type = sim.interner.intern("TREE01");
        sim.terrain_objects
            .objects_mut_for_test()
            .insert(2, TerrainObjectState::for_test(2, tree_type, 7, 5));
        let host = WorldScan {
            sim: &mut sim,
            rules: &rules,
            id: 1,
            ctx: ObjectAiCtx::default(),
            scan_coord: None,
        };
        assert!(host.is_techno(TargetKind::Entity(1)));
        assert!(!host.is_techno(TargetKind::Entity(2)));
        assert!(!host.is_techno(TargetKind::Entity(999)));
        assert!(!host.is_techno(TargetKind::Cell(7, 5)));
    }

    const TARGET: i64 = 0x2000_4000;
    const OTHER: i64 = 0x2000_4800;

    fn kind(pointer: i64) -> Option<TargetKind> {
        match pointer {
            0 => None,
            TARGET => Some(TargetKind::Entity(1)),
            OTHER => Some(TargetKind::Entity(2)),
            other => panic!("unexpected pointer {other:#x}"),
        }
    }

    fn pointer(target: Option<TargetKind>) -> i64 {
        match target {
            None => 0,
            Some(TargetKind::Entity(1)) => TARGET,
            Some(TargetKind::Entity(2)) => OTHER,
            other => panic!("unexpected target {other:?}"),
        }
    }

    /// The oracle's supplied callees: each returns the row's value, applies the
    /// row's mutation for it, and is logged with its arguments.
    struct Recorded<'a> {
        row: &'a Value,
        target: Option<TargetKind>,
        passive: bool,
        last_scan: i32,
        timer: (i32, i32),
        health: [i64; 2],
        events: Vec<(String, Vec<i64>)>,
    }

    impl Recorded<'_> {
        fn call(&mut self, name: &str, args: Vec<i64>) {
            self.events.push((name.to_string(), args));
            if let Some(changes) = self.row["mutations"][name].as_object() {
                for (field, value) in changes {
                    let value = value.as_i64().unwrap();
                    match field.as_str() {
                        "target" => self.target = kind(value),
                        "estimated_health" => self.health[0] = value,
                        other => panic!("unexpected mutation {other}"),
                    }
                }
            }
        }

        fn input(&self, key: &str) -> i64 {
            self.row[key].as_i64().unwrap()
        }
    }

    impl ScanHost for Recorded<'_> {
        type Weapon = ();

        fn frame(&self) -> i32 {
            173
        }
        fn set_last_scan_frame(&mut self, frame: i32) {
            self.last_scan = frame;
        }
        fn committed_area_guard(&self) -> bool {
            self.input("mission") == 11
        }
        fn random_ranged(&mut self, min: i32, max: i32) -> i32 {
            self.call("random", vec![min.into(), max.into()]);
            self.input("jitter") as i32
        }
        fn targeting_delay(&self, area_guard: bool) -> i32 {
            if area_guard { 41 } else { 23 }
        }
        fn arm_targeting_timer(&mut self, start: i32, duration: i32) {
            self.timer = (start, duration);
        }
        fn target(&self) -> Option<TargetKind> {
            self.target
        }
        fn passively_acquired(&self) -> bool {
            self.passive
        }
        fn select_weapon(&mut self, target: Option<TargetKind>) -> i32 {
            self.call("select_weapon", vec![pointer(target)]);
            1
        }
        fn fire_error(&mut self, target: Option<TargetKind>, weapon: i32) -> FireError {
            self.call("fire_error", vec![pointer(target), weapon.into(), 1]);
            match self.input("error") {
                0 => FireError::Ok,
                1 => FireError::Ammo,
                2 => FireError::Facing,
                3 => FireError::Rearm,
                4 => FireError::Rotating,
                5 => FireError::Illegal,
                6 => FireError::Cant,
                7 => FireError::Moving,
                8 => FireError::Range,
                _ => FireError::Cloaked,
            }
        }
        fn has_spawn_manager(&self) -> bool {
            self.row["spawn"] == true
        }
        fn clear_spawn_targets(&mut self) {
            self.call("spawn_abandon", vec![]);
        }
        fn assign_target(&mut self, target: Option<TargetKind>) {
            // The fixture setter writes the target and clears the flag first.
            self.target = target;
            self.passive = false;
            self.call("assign_target", vec![pointer(target)]);
        }
        fn greatest_threat(&mut self, mask: ScanMission) -> Option<TargetKind> {
            self.call("greatest_threat", vec![i64::from(mask.literal_mask())]);
            kind(self.input("pick"))
        }
        fn distributed_fire(&mut self) -> bool {
            self.call("get_type", vec![]);
            self.input("distributed") != 0
        }
        fn distribute_fire(&mut self) {
            self.call("distributed_fire", vec![]);
        }
        fn weapon(&mut self, index: i32) -> Option<()> {
            self.call("get_weapon", vec![index.into()]);
            (self.row["weapon"] == true).then_some(())
        }
        fn inaccurate(&self, _weapon: ()) -> bool {
            self.input("projectile_debit_excluded") != 0
        }
        fn is_techno(&self, target: TargetKind) -> bool {
            target != TargetKind::Entity(1) || self.input("pick_flags") & 1 != 0
        }
        fn estimated_damage(&mut self, target: TargetKind, _weapon: ()) -> i32 {
            self.call("estimated_damage", vec![pointer(Some(target))]);
            self.input("damage") as i32
        }
        fn debit(&mut self, target: TargetKind, amount: i32) {
            let slot = usize::from(target == TargetKind::Entity(2));
            self.health[slot] = i64::from((self.health[slot] as i32).wrapping_sub(amount));
        }
    }

    /// Every `techno_target_scan.json` row: the callee sequence with its
    /// target, weapon and mask arguments, the returned flag, and the retained
    /// target, passive flag, last-scan frame, timer and estimates.
    #[test]
    fn retaliate_and_scan_matches_the_original() {
        let rows: Vec<Value> = serde_json::from_str(crate::test_fixture::text(
            "tools/spatial_oracle/techno_target_scan.json",
        ))
        .unwrap();
        assert_eq!(rows.len(), 171);
        for row in &rows {
            let input = &row["input"];
            let mut host = Recorded {
                row: input,
                target: kind(input["target"].as_i64().unwrap()),
                passive: input["passive"] != 0,
                last_scan: 99,
                timer: (33, 55),
                health: [input["health"].as_i64().unwrap(), 456],
                events: Vec::new(),
            };
            let mask = match input["mask"].as_i64().unwrap() & 3 {
                0 => ScanMission::Hunt,
                1 => ScanMission::Guard,
                2 => ScanMission::AreaGuard,
                // Mask 3 reaches Greatest_Threat as 3; VERA has no such
                // caller. Its only difference is the argument.
                _ => ScanMission::AreaGuard,
            };
            let held = retaliate_and_scan(&mut host, mask);
            let name = input["name"].as_str().unwrap();

            let native: Vec<(String, Vec<i64>)> = row["events"]
                .as_array()
                .unwrap()
                .iter()
                .map(|event| {
                    let name = event["name"].as_str().unwrap().to_string();
                    let args: Vec<i64> = event["args"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .map(|a| a.as_i64().unwrap())
                        .collect();
                    let args = match name.as_str() {
                        "greatest_threat" => vec![args[0]],
                        "estimated_damage" => vec![args[0]],
                        "get_weapon" => vec![args[0]],
                        _ => args,
                    };
                    (name, args)
                })
                .collect();
            let mut ours = host.events.clone();
            if input["mask"].as_i64().unwrap() & 3 == 3 {
                for event in &mut ours {
                    if event.0 == "greatest_threat" {
                        event.1 = vec![3];
                    }
                }
            }
            assert_eq!(ours, native, "{name}");

            let after = &row["after"];
            let field = |key: &str| after[key].as_i64().unwrap();
            assert_eq!(pointer(host.target), field("target"), "{name}");
            assert_eq!(i64::from(host.passive), field("passive"), "{name}");
            assert_eq!(i64::from(host.last_scan), field("last_scan"), "{name}");
            assert_eq!(i64::from(host.timer.0), field("timer_start"), "{name}");
            assert_eq!(i64::from(host.timer.1), field("timer_duration"), "{name}");
            assert_eq!(
                host.health[0] as u32,
                field("estimated_health") as u32,
                "{name}"
            );
            assert_eq!(host.health[1], field("other_health"), "{name}");
            assert_eq!(
                i64::from(held),
                row["returned_al"].as_i64().unwrap(),
                "{name}"
            );
        }
    }

    /// Parity with `tools/spatial_oracle/passive_acquire_gate.json`: the
    /// original gate and CanAcquireTarget, through the production gate with a
    /// type carrying the row's flags. A token of no nodes compares as VERA's
    /// absent token. Not compared: a token with nodes and a Foot class without
    /// the Foot flag, which VERA cannot hold; and the Temporal, slave,
    /// CaptureManager, team, garrison and AreaFire-selection rows, whose
    /// inputs this fixture does not build (those arms predate the Move arms).
    #[test]
    fn passive_acquire_gate_matches_the_native_gate() {
        use crate::rules::ini_parser::IniFile;
        use crate::sim::components::NavTargetRef;
        use crate::sim::game_entity::GameEntity;
        use crate::sim::house_state::HouseState;

        let corpus: Value = serde_json::from_str(crate::test_fixture::text(
            "tools/spatial_oracle/passive_acquire_gate.json",
        ))
        .expect("corpus parses");
        let native_only = |input: &Value| {
            let name = input["name"].as_str().unwrap();
            input["token"].as_i64().is_some_and(|nodes| nodes > 0)
                || input["foot"] == false
                || input["team"].is_object()
                || [
                    "warping",
                    "slave",
                    "capture_full",
                    "capture_not_full",
                    "empty_garrison",
                    "garrisoned",
                    "guard_no_weapon",
                ]
                .contains(&name)
                || name.starts_with("guard_area1")
        };
        let flag = |input: &Value, key: &str, default: bool| match &input[key] {
            Value::Bool(value) => *value,
            Value::Number(value) => value.as_i64() != Some(0),
            _ => default,
        };
        let class = |input: &Value| input["class"].as_str().unwrap_or("infantry").to_string();
        let type_name = |input: &Value| {
            let bits: String = [
                ("balloon_hover", true),
                ("simple_deployer", false),
                ("opportunity_fire", false),
                ("can_passive_acquire", true),
                ("engineer", false),
                ("armed", true),
            ]
            .iter()
            .map(|(key, default)| if flag(input, key, *default) { '1' } else { '0' })
            .collect();
            format!("{}{bits}", class(input).to_uppercase())
        };
        let rows: Vec<&Value> = corpus
            .as_array()
            .unwrap()
            .iter()
            .filter(|row| !native_only(&row["input"]))
            .collect();
        let mut lists: std::collections::BTreeMap<&str, Vec<String>> = Default::default();
        let mut sections = String::new();
        for row in &rows {
            let input = &row["input"];
            let name = type_name(input);
            let list = match class(input).as_str() {
                "unit" => "VehicleTypes",
                "aircraft" => "AircraftTypes",
                "building" => "BuildingTypes",
                _ => "InfantryTypes",
            };
            if lists.values().any(|names| names.contains(&name)) {
                continue;
            }
            let yes = |value: bool| if value { "yes" } else { "no" };
            sections += &format!(
                "[{name}]\nStrength=100\nBalloonHover={}\nIsSimpleDeployer={}\n\
                 OpportunityFire={}\nCanPassiveAquire={}\nEngineer={}\n{}",
                yes(flag(input, "balloon_hover", true)),
                yes(flag(input, "simple_deployer", false)),
                yes(flag(input, "opportunity_fire", false)),
                yes(flag(input, "can_passive_acquire", true)),
                yes(flag(input, "engineer", false)),
                if flag(input, "armed", true) {
                    "Primary=GUN\n"
                } else {
                    ""
                },
            );
            lists.entry(list).or_default().push(name);
        }
        let mut ini = String::from(
            "[Warheads]\n0=WH\n[GUN]\nDamage=10\nROF=10\nRange=5\nWarhead=WH\n\
             [WH]\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n",
        );
        for (list, names) in &lists {
            ini += &format!("[{list}]\n");
            for (index, name) in names.iter().enumerate() {
                ini += &format!("{index}={name}\n");
            }
        }
        ini += &sections;
        let rules = RuleSet::from_ini(&IniFile::from_str(&ini)).expect("gate rules");

        let mut compared = Vec::new();
        for row in &rows {
            let input = &row["input"];
            let name = input["name"].as_str().unwrap();
            let mut sim = Simulation::new();
            let owner = sim.interner.intern("Americans");
            let human = input["human"].as_bool().unwrap_or(true);
            sim.houses
                .register(HouseState::new(owner, 0, None, human, 0, 10));
            let mut entity = GameEntity::test_default(1, "X", "Americans", 10, 10);
            entity.set_owner_for_test(owner);
            entity.set_type_ref_for_test(sim.interner.intern(&type_name(input)));
            entity.set_category_for_test(match class(input).as_str() {
                "unit" => EntityCategory::Unit,
                "aircraft" => EntityCategory::Aircraft,
                "building" => EntityCategory::Structure,
                _ => EntityCategory::Infantry,
            });
            entity
                .navigation
                .set_nav_com_for_test(match &input["nav_com"] {
                    Value::Null if input.get("nav_com").is_some() => None,
                    Value::String(other) if other == "other" => Some(NavTargetRef::cell(11, 10)),
                    _ => Some(NavTargetRef::cell(10, 10)),
                });
            sim.substrate.entities.insert(entity);
            let mission = match input["mission"].as_str().unwrap_or("move") {
                "attack" => MissionType::Attack,
                "sleep" => MissionType::Sleep,
                "hunt" => MissionType::Hunt,
                "area_guard" => MissionType::AreaGuard,
                "guard" => MissionType::Guard,
                "none" => MissionType::None,
                _ => MissionType::Move,
            };
            assert_eq!(
                passive_acquire_gate(&sim, 1, &rules, mission),
                row["output"]["passes"].as_bool().unwrap(),
                "{name}"
            );
            compared.push(name);
        }
        assert_eq!(compared.len(), 32, "{compared:?}");
    }
}
