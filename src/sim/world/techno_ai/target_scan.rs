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
//! the estimate debit).

use super::ObjectAiCtx;
use crate::map::entities::EntityCategory;
use crate::rules::ruleset::RuleSet;
use crate::rules::weapon_type::WeaponType;
use crate::sim::combat::fire_error::FireError;
use crate::sim::combat::fire_error_world::{FireSubject, garrison_weapon};
use crate::sim::combat::{ScanMission, TargetKind, combat_weapon};
use crate::sim::mission::MissionType;
use crate::sim::world::Simulation;

/// Committed mission id of Area Guard (`+0xAC == 0xB`, `0x0070983A`).
const AREA_GUARD_MISSION: i32 = 0xB;

/// What `Retaliate_And_Scan` asks of its object, in the order it asks.
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
pub(super) fn scan(
    sim: &mut Simulation,
    id: u64,
    rules: &RuleSet,
    mask: ScanMission,
    ctx: ObjectAiCtx<'_>,
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
        },
        mask,
    )
}

/// The passive block of `TechnoClass::AI_Update` (`0x006FA65A..0x006FA6EE`),
/// after mission dispatch:
/// - only once the targeting timer has run out (a stopped timer counts as
///   run out only with a zero duration);
/// - only on Move (2), Harvest (10) or Guard (5);
/// - only through [`passive_acquire_gate`];
/// - then `+0x4FC = Frame`, the scan with mask 1, and `+0x50C = 1` when the
///   scan holds a target that differs from the one before it (`0x006FA6EE`).
///
/// RESIDUALS:
/// - The mission test reads [`crate::sim::game_entity::GameEntity::
///   passive_acquire_mission`], VERA's bridge for objects whose committed
///   mission (`+0xAC`) no handler maintains yet. An infantryman commits Guard
///   when it enters the map, so the two agree on it.
/// - The attack-move divert (`vt+0x4C4`: a saved mission `+0x5C4 == 0x1D`,
///   then `vt+0x4CC` instead of this block) belongs to attack-move, which VERA
///   drives from its own order path; such an object reaches this block.
/// - The block has no health gate: a dying infantryman reaches it from its
///   corpse visit (`techno_ai::dying_infantry_techno_ai`).
pub(super) fn passive_acquire_step(
    sim: &mut Simulation,
    id: u64,
    rules: Option<&RuleSet>,
    ctx: ObjectAiCtx<'_>,
) {
    let Some(rules) = rules else {
        return;
    };
    let now = sim.session.binary_frame;
    let Some(entity) = sim.substrate.entities.get(id) else {
        return;
    };
    if !entity.passive_scan_timer.due(now) {
        return;
    }
    let mission = entity.passive_acquire_mission();
    if !matches!(
        mission,
        MissionType::Move | MissionType::Guard | MissionType::Harvest
    ) {
        return;
    }
    if !passive_acquire_gate(sim, id, rules, mission) {
        return;
    }
    let Some(entity) = sim.substrate.entities.get_mut(id) else {
        return;
    };
    entity.last_target_scan_frame = now;
    let before = entity.attack_target.as_ref().map(|attack| attack.target);
    let held = scan(sim, id, rules, ScanMission::Guard, ctx);
    if let Some(entity) = sim.substrate.entities.get_mut(id)
        && held
        && entity.attack_target.as_ref().map(|attack| attack.target) != before
    {
        entity.passively_acquired_target = true;
    }
}

/// `TechnoClass::PassiveAcquireGate @ 0x00709290`, in native order:
/// 1. a computer-owned Foot with no target, in a team whose TeamType is
///    `Aggressive=yes` and `Suicide=no`, passes on Move without
///    CanAcquireTarget (`0x0070929A..0x007092EC`);
/// 2. otherwise [`can_acquire_target`] must pass;
/// 3. `OpportunityFire=` (`+0x6AF`) passes (`0x007093D5`);
/// 4. any mission but Guard fails;
/// 5. on Guard, the weapon in slot `SprayAttack ? 0 : 1` (vt+0x3E4,
///    `0x0070DD70`) refuses when it has `AreaFire=` (`+0x150`) and
///    SelectWeapon picks that slot for the current target
///    (`0x007093F8..0x00709449`).
///
/// RESIDUAL: the two Move arms between 2 and 3 (`0x00709301..0x007093D3`),
/// for a `BalloonHover=` Foot or a Unit whose type has `+0xE13`, pass when
/// the NavCom is the object's own cell and the `+0x514` object's `+0x14` is not
/// positive. `+0x514` is unidentified. Trigger: those types on Move (retail:
/// two `BalloonHover=` aircraft). Effect: they do not scan while parked on
/// their own cell.
fn passive_acquire_gate(sim: &Simulation, id: u64, rules: &RuleSet, mission: MissionType) -> bool {
    let Some(entity) = sim.substrate.entities.get(id) else {
        return false;
    };
    let is_foot = matches!(
        entity.category,
        EntityCategory::Unit | EntityCategory::Infantry | EntityCategory::Aircraft
    );
    if mission == MissionType::Move
        && entity.attack_target.is_none()
        && is_foot
        && !owner_is_human(sim, entity.owner())
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
    let Some(obj) = rules.object(sim.interner.resolve(entity.type_ref())) else {
        return false;
    };
    if obj.opportunity_fire {
        return true;
    }
    if mission != MissionType::Guard {
        return false;
    }
    let slot = if obj.spray_attack { 0 } else { 1 };
    let target = entity.attack_target.as_ref().map(|attack| attack.target);
    let area_fire = fire_subject(sim, rules, id, target, slot)
        .and_then(|subject| subject.weapon_at(slot).map(|weapon| weapon.area_fire))
        .unwrap_or(false);
    !(area_fire && select_weapon(sim, rules, id, target) == slot)
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
    let Some(obj) = rules.object(sim.interner.resolve(entity.type_ref())) else {
        return false;
    };
    if !obj.can_passive_acquire {
        return false;
    }
    if entity.category == EntityCategory::Structure
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
    if entity.category == EntityCategory::Infantry
        && obj.engineer
        && owner_is_human(sim, entity.owner())
    {
        return false;
    }
    combat_weapon::is_armed(entity, obj)
}

/// `HouseClass::IsControlledByHuman @ 0x0050B730`.
fn owner_is_human(sim: &Simulation, owner: crate::sim::intern::InternedId) -> bool {
    sim.houses
        .get(&owner)
        .is_some_and(|house| house.is_controlled_by_human(sim.session.game_mode_nonzero))
}

fn fire_subject<'a>(
    sim: &'a Simulation,
    rules: &'a RuleSet,
    id: u64,
    target: Option<TargetKind>,
    weapon_index: i32,
) -> Option<FireSubject<'a>> {
    let firer = sim.substrate.entities.get(id)?;
    let obj = rules.object(sim.interner.resolve(firer.type_ref()))?;
    Some(FireSubject {
        world: sim,
        rules,
        overlay_registry: None,
        fog: Some(&sim.fog),
        firer,
        obj,
        target,
        weapon_index,
        garrison: target.and_then(|target| garrison_weapon(sim, rules, firer, obj, target)),
    })
}

/// GetWeapon (vt+0x3F8)'s WeaponType at `index`, a garrison's occupant weapon
/// included.
pub(super) fn weapon_at_index<'r>(
    sim: &Simulation,
    rules: &'r RuleSet,
    id: u64,
    index: i32,
) -> Option<&'r WeaponType> {
    let target = sim
        .substrate
        .entities
        .get(id)?
        .attack_target
        .as_ref()
        .map(|attack| attack.target);
    let subject = fire_subject(sim, rules, id, target, index)?;
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
    let Some(obj) = rules.object(sim.interner.resolve(firer.type_ref())) else {
        return 0;
    };
    let terrain = sim.resolved_terrain.as_ref();
    let target_facts = match target {
        Some(TargetKind::Entity(target_id)) => {
            sim.substrate.entities.get(target_id).and_then(|target| {
                rules
                    .object(sim.interner.resolve(target.type_ref()))
                    .map(|target_obj| {
                        combat_weapon::techno_target_facts(
                            target,
                            target_obj,
                            terrain,
                            combat_weapon::is_ally_by_object(
                                Some(&sim.fog.alliances),
                                &sim.interner,
                                firer.owner(),
                                target.owner(),
                            ),
                        )
                    })
            })
        }
        Some(TargetKind::Cell(rx, ry)) => Some(combat_weapon::cell_target_facts(rx, ry, terrain)),
        None => None,
    };
    combat_weapon::what_weapon_should_i_use(
        rules,
        obj,
        &combat_weapon::attacker_facts(firer, obj),
        target_facts.as_ref(),
    )
}

/// GetFireError (vt+0x3C0) for `target` with `weapon_index`; `check_range`
/// false is vt+0x3BC.
pub(super) fn fire_error_at(
    sim: &Simulation,
    rules: &RuleSet,
    id: u64,
    target: Option<TargetKind>,
    weapon_index: i32,
    check_range: bool,
) -> FireError {
    fire_subject(sim, rules, id, target, weapon_index).map_or(FireError::Illegal, |subject| {
        subject.fire_error(check_range)
    })
}

/// The production scan host.
struct WorldScan<'s, 'r> {
    sim: &'s mut Simulation,
    rules: &'r RuleSet,
    id: u64,
    ctx: ObjectAiCtx<'r>,
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
        self.sim.session.binary_frame as i32
    }

    fn set_last_scan_frame(&mut self, frame: i32) {
        if let Some(entity) = self.sim.substrate.entities.get_mut(self.id) {
            entity.last_target_scan_frame = frame as u32;
        }
    }

    fn committed_area_guard(&self) -> bool {
        self.entity().mission.current().raw() == AREA_GUARD_MISSION
    }

    fn random_ranged(&mut self, min: i32, max: i32) -> i32 {
        self.sim
            .scenario_rng
            .next_range_u32_inclusive(min as u32, max as u32) as i32
    }

    fn targeting_delay(&self, area_guard: bool) -> i32 {
        let general = &self.rules.general;
        if area_guard {
            general.guard_area_targeting_delay as i32
        } else {
            general.normal_targeting_delay as i32
        }
    }

    fn arm_targeting_timer(&mut self, start: i32, duration: i32) {
        if let Some(entity) = self.sim.substrate.entities.get_mut(self.id) {
            entity.passive_scan_timer.arm(start as u32, duration as u32);
        }
    }

    fn target(&self) -> Option<TargetKind> {
        self.entity()
            .attack_target
            .as_ref()
            .map(|attack| attack.target)
    }

    fn passively_acquired(&self) -> bool {
        self.entity().passively_acquired_target
    }

    fn select_weapon(&mut self, target: Option<TargetKind>) -> i32 {
        select_weapon(self.sim, self.rules, self.id, target)
    }

    fn fire_error(&mut self, target: Option<TargetKind>, weapon: i32) -> FireError {
        fire_error_at(self.sim, self.rules, self.id, target, weapon, true)
    }

    fn has_spawn_manager(&self) -> bool {
        self.entity().spawn_manager.is_some()
    }

    fn clear_spawn_targets(&mut self) {
        crate::sim::spawn_manager::clear_all_spawn_targets(self.sim, self.id);
    }

    fn assign_target(&mut self, target: Option<TargetKind>) {
        let _ = self.sim.assign_target_represented(self.id, target);
    }

    /// RESIDUAL: Area Guard natively scans around its guard post (the
    /// ArchiveTarget, `0x004D6EE6`); VERA scans around the object, which is
    /// the same cell while it stands on its post.
    fn greatest_threat(&mut self, mask: ScanMission) -> Option<TargetKind> {
        let sim = &*self.sim;
        crate::sim::combat::acquire_best_target_for_entity(
            &sim.substrate.entities,
            &sim.substrate.occupancy,
            self.rules,
            &sim.interner,
            self.id,
            Some(&sim.fog),
            sim.resolved_terrain.as_ref(),
            sim.playfield_bounds.is_some(),
            mask,
            sim.zone_grid.as_ref(),
            crate::sim::combat::line_of_fire::LineOfFireInputs {
                overlay_grid: sim.overlay_grid.as_ref(),
                overlay_registry: self.ctx.overlay_registry,
                alliances: Some(&sim.fog.alliances),
            },
            Some(sim),
        )
        .map(TargetKind::Entity)
    }

    fn distributed_fire(&mut self) -> bool {
        self.rules
            .object(self.sim.interner.resolve(self.entity().type_ref()))
            .is_some_and(|obj| obj.distributed_fire)
    }

    /// RESIDUAL: `DistributeFire @ 0x00709550` (the Aegis Cruiser's spread
    /// fire) is not ported; such an object takes no target from a scan.
    fn distribute_fire(&mut self) {}

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
        matches!(target, TargetKind::Entity(_))
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

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;

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
            let mask = match mask {
                ScanMission::Hunt => 0,
                ScanMission::Guard => 1,
                ScanMission::AreaGuard => 2,
            };
            self.call("greatest_threat", vec![mask]);
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
        let rows: Vec<Value> = serde_json::from_str(include_str!(
            "../../../../tools/spatial_oracle/techno_target_scan.json"
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
}
