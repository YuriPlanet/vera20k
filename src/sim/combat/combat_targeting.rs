//! Target acquisition and retaliation for the combat system.
//!
//! Handles two responsibilities:
//! 1. **Target acquisition** — finding the best hostile target for an idle or
//!    attack-moving unit within its guard/weapon range.
//! 2. **Retaliation** — idle units automatically attack the entity that hit them.
//!
//! ## Target priority
//! `TechnoClass::Greatest_Threat @ 0x006F8DF0` scores every candidate the walk
//! reaches and keeps the maximum, ties going to whatever the walk saw first.
//! The walk, the score and the candidate gates live in
//! [`super::greatest_threat`]; this module owns the snapshot the scan runs on
//! and the retaliation pass.
//!
//! ## Scan radius
//! How far the scan reaches is a property of the attacker and of the threat
//! mask its CALLER pushed, not of the candidate — see [`super::threat_range`].
//! A unit on Area Guard acquires roughly twice as far out as the same unit on
//! plain Guard, and a unit on Hunt is not walking cell rings at all.
//!
//! ## Auto-deploy on target acquisition
//! Targeting NEVER initiates a deploy transition. A walking GGI that acquires
//! an air target uses its Secondary weapon in place — it does not auto-deploy.
//! This matches the original's behavior: deploy is a player-driven command,
//! never triggered by AI target acquisition. Verified by grepping every writer
//! of `deploy_state` — only the player command handler and the deploy tick
//! advance set it.
//!
//! ## Dependency rules
//! - Part of sim/ — depends on rules/ (RuleSet) and sim/components.
//! - sim/ NEVER depends on render/, ui/, sidebar/, audio/, net/.

use std::collections::BTreeMap;

use super::combat_weapon::{
    VersesGate, attacker_facts, is_ally_by_object, select_weapon_for_target, techno_target_facts,
    verses_gate,
};
use super::threat_range::ScanMission;
use crate::map::entities::EntityCategory;
use crate::map::houses::{HouseAllianceMap, is_allied_with};
use crate::map::resolved_terrain::ResolvedTerrainGrid;
use crate::rules::ruleset::RuleSet;
use crate::sim::entity_store::EntityStore;
use crate::sim::game_entity::GameEntity;
use crate::sim::house_state::HouseState;
use crate::sim::intern::{InternedId, StringInterner};
use crate::sim::vision::FogState;
use crate::util::fixed_math::SimFixed;
use crate::util::native_x87::{
    MaskedX87Chop53 as X87Chop53, MaskedX87Ordering as X87Ordering, MaskedX87Value,
};

/// Snapshot of garrison state for a garrisoned building attacker.
/// Extracted during Phase 1 to avoid borrow conflicts in Phase 2.
#[derive(Clone)]
pub(crate) struct GarrisonSnapshot {
    /// Type ID of the occupant that will fire this tick.
    pub occupant_type_id: InternedId,
    /// Veterancy of the firing occupant (for elite weapon selection).
    pub occupant_veterancy: u16,
    /// Current round-robin fire index.
    pub fire_index: u8,
    /// Total occupant count (for ROF division).
    pub occupant_count: u8,
    /// Half foundation size: `min(width, height) / 2` (for range formula).
    pub half_foundation: u16,
}

/// Snapshot of an attacker's state for target scanning.
/// Extracted to avoid borrow conflicts during entity iteration.
#[derive(Clone)]
pub(crate) struct AttackerSnapshot {
    pub stable_id: u64,
    pub owner: InternedId,
    pub category: EntityCategory,
    /// What the attacker is firing at — entity ID or cell coord.
    /// Cell targets skip auto-retarget and friendly-fire checks (the player
    /// explicitly chose this cell).
    pub target: super::TargetKind,
    pub pos_rx: u16,
    pub pos_ry: u16,
    pub pos_z: u8,
    pub pos_exact_z_leptons: Option<i32>,
    pub sub_x: SimFixed,
    pub sub_y: SimFixed,
    pub type_id: InternedId,
    pub facing: u8,
    pub veterancy: u16,
    pub cooldown_ticks: u16,
    pub animation_sequence: Option<crate::sim::animation::SequenceKind>,
    pub animation_frame: Option<u16>,
    pub is_prone: bool,
    pub is_fully_deployed: bool,
    pub has_movement: bool,
    pub pending_infantry_fire: Option<super::PendingInfantryFire>,
    pub pending_building_fire: Option<crate::sim::game_entity::PendingBuildingFire>,
    pub barrel_facing: Option<crate::sim::movement::FacingClass>,
    /// Retained body FacingClass (`+0x388`), including infantry fire-start
    /// snaps and vehicle turns. Facing gates and emission read its full
    /// 16-bit value rather than the byte mirrored for presentation.
    pub hull_facing: Option<crate::sim::movement::FacingClass>,
    /// Turret rotation latch (`UnitClass+0x6AF`) as it stood BEFORE this tick's
    /// `Facing_Update`, which is the value `UnitClass::GetFireError @
    /// 0x00741233` reads: `UnitClass::AI` runs `Fire_At_Target @ 0x007365E1`
    /// before `Facing_Update @ 0x007365E8`, and VERA commits the new latch in
    /// `apply_unit_facing` after Phase 5.
    pub turret_rotation_latch: bool,
    pub burst_delay_ticks: u8,
    /// Weapon-selection override (Gunner-IFV slot OR open-topped passenger weapon).
    pub weapon_override: Option<super::combat_weapon::WeaponOverride>,
    /// Garrison state — present only for garrisoned buildings (IsOccupied).
    pub garrison: Option<GarrisonSnapshot>,
    /// The threat mask this scan's CALLER pushed — `Greatest_Threat`'s second
    /// argument, always a literal in retail. It selects the radius formula
    /// (Area Guard reaches roughly twice as far as plain Guard) and, for mask
    /// 0, the scan topology itself.
    pub scan_mission: ScanMission,
}

/// Acquire the best currently valid target for one attacker entity.
/// Returns the target's stable entity ID.
///
/// `terrain` is threaded through for the 3D InRange check; when `None`
/// (headless tests, no map loaded), the range check falls back to the
/// existing 2D behavior.
///
/// `mask` is `Greatest_Threat`'s second argument. Every caller pushes a literal
/// — `0` from `FootClass::Mission_Hunt @ 0x004D5373`, `1` from the common Techno
/// AI body and from `FUN_0051F330`'s in-place re-acquire (`vt+0x3C4(1, ...)`),
/// `2` from `FootClass::Mission_AreaGuard` — and mask 0 selects a different scan
/// topology entirely, not a wider radius. So it is a parameter here rather than
/// something read off the entity. [`super::threat_range::scan_mission_for`]
/// remains, for the passive callsites whose literal genuinely depends on which
/// mission dispatched them.
///
/// What the literal is NOT is what `TechnoClass::Greatest_Threat` finally sees:
/// a `FootClass` dispatch goes through the `+0x3C4` overrides first, which OR
/// the attacker's projectile class bits in (`0x00743190`, `0x0051E39F`) and,
/// while `FootClass+0x688` is set, coerce it to `(mask & ~2) | 1`
/// (`0x004D9931`). Both are recorded as residuals on
/// [`super::greatest_threat::greatest_threat`]; neither is modelled here.
///
/// `zone_grid` is `MapClass`'s per-movement-zone connectivity, which mask 0 uses
/// to refuse candidates its own movement zone cannot reach.
#[allow(clippy::too_many_arguments)]
pub(crate) fn acquire_best_target_for_entity(
    entities: &EntityStore,
    occupancy: &crate::sim::occupancy::OccupancyGrid,
    rules: &RuleSet,
    interner: &StringInterner,
    attacker_id: u64,
    fog: Option<&FogState>,
    terrain: Option<&ResolvedTerrainGrid>,
    require_playfield_membership: bool,
    mask: ScanMission,
    zone_grid: Option<&crate::sim::pathfinding::zone_map::ZoneGrid>,
    los: super::line_of_fire::LineOfFireInputs<'_>,
) -> Option<u64> {
    let entity = entities.get(attacker_id)?;
    // Aircraft with 0 ammo should not acquire new targets — need to reload.
    if let Some(ref ammo) = entity.aircraft_ammo {
        if ammo.current == 0 {
            return None;
        }
    }
    let obj = rules.object(interner.resolve(entity.type_ref()))?;
    // Native `TechnoClass::Greatest_Threat @ 0x006F8DF0` has no weapon
    // early-out of its own; the armed requirement sits upstream in
    // `TechnoClass::CanAcquireTarget @ 0x007091D0`, whose last term is
    // `Is_Armed` (vtable `+0x2AC`). This is the same predicate, kept here
    // because VERA's acquisition entry is also reached from the order and
    // deployed-reacquire paths. It must NOT read `Primary=`: a `TurretCount>0`
    // type never parses that key (`TechnoTypeClass::ReadINI @ 0x007128B2`), so
    // `[SREF]` and `[YAGGUN]` were classified unarmed and could never acquire.
    if !super::combat_weapon::is_armed(entity, obj) {
        return None;
    }

    let snapshot = AttackerSnapshot {
        stable_id: entity.stable_id(),
        owner: entity.owner(),
        category: entity.category,
        target: super::TargetKind::Entity(0), // Dummy — no current target when acquiring fresh
        pos_rx: entity.position.rx,
        pos_ry: entity.position.ry,
        pos_z: entity.position.z,
        pos_exact_z_leptons: entity.position.exact_z_leptons,
        sub_x: entity.position.sub_x,
        sub_y: entity.position.sub_y,
        type_id: entity.type_ref(),
        facing: entity.facing,
        veterancy: entity.veterancy,
        cooldown_ticks: 0,
        animation_sequence: entity.animation.as_ref().map(|a| a.sequence),
        animation_frame: entity.animation.as_ref().map(|a| a.frame_index),
        is_prone: entity
            .infantry
            .as_ref()
            .is_some_and(|infantry| infantry.is_prone),
        is_fully_deployed: entity.is_fully_deployed(),
        has_movement: entity.movement_target.is_some(),
        pending_infantry_fire: None,
        pending_building_fire: None,
        barrel_facing: entity.barrel_facing,
        hull_facing: entity.body_facing,
        turret_rotation_latch: entity.turret_rotation_latch,
        burst_delay_ticks: 0,
        weapon_override: entity.weapon_override,
        garrison: None,
        scan_mission: mask,
    };
    acquire_best_target(
        entities,
        occupancy,
        rules,
        interner,
        &snapshot,
        obj,
        fog,
        None,
        terrain,
        require_playfield_membership,
        zone_grid,
        los,
    )
}

/// `TechnoClass::Greatest_Threat @ 0x006F8DF0` — pick the best hostile target
/// for one attacker snapshot. Returns the winning candidate's stable entity id.
///
/// The walk, the per-cell single-candidate rule, the gate ladder and the
/// weighted score all live in [`super::greatest_threat`]; this is the adapter
/// the acquisition and retarget call sites already speak to.
///
/// `scan_range_override`: when `Some`, replaces the mission-derived radius with
/// a hard cutoff. Used by garrisoned buildings whose scan range is derived from
/// foundation size + OccupyWeaponRange.
///
/// This replaces VERA's own `(distance², threat_class, stable_id)` nearest-first
/// key, which had no native counterpart: gamemd scores each candidate and keeps
/// the maximum, walking outward one cell ring at a time and stopping early once
/// something has been found. The consequences a player sees are that value now
/// beats proximity — a Grizzly on Guard shoots the engineer walking past instead
/// of the wall segment beside it, because the wall is refused outright and the
/// engineer scores highest — and that a base defence with several attackers in
/// reach commits to the one in the innermost band rather than the nearest by
/// Euclidean distance.
#[allow(clippy::too_many_arguments)]
pub(crate) fn acquire_best_target(
    entities: &EntityStore,
    occupancy: &crate::sim::occupancy::OccupancyGrid,
    rules: &RuleSet,
    interner: &StringInterner,
    attacker: &AttackerSnapshot,
    attacker_obj: &crate::rules::object_type::ObjectType,
    fog: Option<&FogState>,
    scan_range_override: Option<SimFixed>,
    terrain: Option<&ResolvedTerrainGrid>,
    require_playfield_membership: bool,
    zone_grid: Option<&crate::sim::pathfinding::zone_map::ZoneGrid>,
    los: super::line_of_fire::LineOfFireInputs<'_>,
) -> Option<u64> {
    super::greatest_threat::greatest_threat(
        entities,
        occupancy,
        rules,
        interner,
        attacker,
        attacker_obj,
        fog,
        scan_range_override,
        terrain,
        require_playfield_membership,
        zone_grid,
        los,
    )
}

/// Check if an entity can retaliate against an attacker (weapon + Verses gate).
fn can_retaliate(
    entity: &GameEntity,
    attacker: &GameEntity,
    rules: &RuleSet,
    interner: &StringInterner,
    terrain: Option<&ResolvedTerrainGrid>,
    alliances: Option<&HouseAllianceMap>,
) -> bool {
    let obj = match rules.object(interner.resolve(entity.type_ref())) {
        Some(o) => o,
        None => return false,
    };
    let Some(attacker_obj) = rules.object(interner.resolve(attacker.type_ref())) else {
        return false;
    };
    let target_facts = techno_target_facts(
        attacker,
        attacker_obj,
        terrain,
        is_ally_by_object(alliances, interner, entity.owner(), attacker.owner()),
    );
    let selected =
        match select_weapon_for_target(rules, obj, &attacker_facts(entity, obj), &target_facts) {
            Some(s) => s,
            None => return false,
        };
    // 0% is already filtered by the GetFireError subset (returns None).
    // 1% (Suppressed) also blocks retaliation.
    verses_gate(selected.verses_pct) != VersesGate::Suppressed
}

/// `TechnoClass::Calculate_Threat_Score @ 0x0070CD10` as `ShouldRetaliate`
/// consumes it, on the native `&NullCoord` branch — verified, not assumed:
/// both `ShouldRetaliate` callsites push the sentinel literally
/// (`PUSH 0xb0ea90 @ 0x00708A81` and `@ 0x00708A92` before the calls at
/// `0x00708A89` / `0x00708A9A`), so this comparison is on the CELL scale. The
/// lepton-scale branch is reachable only from `Evaluate_Candidate`'s mask-0
/// flat walk; see [`super::greatest_threat::ThreatReference`].
///
/// The five coefficients are the ones the scorer's own house selects — see
/// [`super::greatest_threat::ThreatCoefficients`] and
/// [`super::greatest_threat::HOUSE_SELECTS_OWN_COEFFICIENTS`]. This used to read
/// the `[General] Dumb*Coefficient` set unconditionally, which is the branch
/// native takes only for a house constructed without a country type — no house
/// in a skirmish. The two sets differ by two sign flips and a 10x on the
/// distance weight, so a defender picking between its current target and
/// whatever just shot it could come to the opposite conclusion.
pub(crate) fn calculate_ai_threat_score(
    entities: &EntityStore,
    scorer_id: u64,
    candidate_id: u64,
    rules: &RuleSet,
    interner: &StringInterner,
    terrain: Option<&ResolvedTerrainGrid>,
    alliances: Option<&HouseAllianceMap>,
) -> Option<MaskedX87Value> {
    let scorer = entities.get(scorer_id)?;
    let scorer_type = rules.object(interner.resolve(scorer.type_ref()))?;
    let coefficients = super::greatest_threat::ThreatCoefficients::resolve(
        rules,
        scorer_type,
        super::greatest_threat::HOUSE_SELECTS_OWN_COEFFICIENTS,
    );
    super::greatest_threat::calculate_threat_score(
        entities,
        scorer_id,
        candidate_id,
        rules,
        interner,
        terrain,
        alliances,
        coefficients,
        super::greatest_threat::ThreatReference::NullCoord,
    )
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(i32)]
enum RetaliationPeekFireError {
    Clear = 0,
    Illegal = 5,
}

/// Represented structural part of the read-only `GetFireError` peek issued by
/// `ShouldRetaliate`. The target is the still-represented damage source; a
/// source in ObjectClass limbo returns native `FIRE_ILLEGAL` without consuming
/// ammo, RNG, or weapon state.
fn retaliation_peek_fire_error(target: &GameEntity) -> RetaliationPeekFireError {
    if target.lifecycle.in_limbo {
        RetaliationPeekFireError::Illegal
    } else {
        RetaliationPeekFireError::Clear
    }
}

/// Evaluate the live, receiver-synchronous part of
/// `TechnoClass::ShouldRetaliate @ 0x007087C0` using represented authority.
///
/// The native call consumes the still-represented source object, including a
/// dying DeathWeapon producer, so source health is deliberately not an
/// admission gate here. Represented gates are read afresh for every ordered
/// receiver.
pub(crate) fn should_retaliate_from_damage(
    entities: &EntityStore,
    victim_id: u64,
    attacker_id: u64,
    rules: &RuleSet,
    interner: &StringInterner,
    houses: &BTreeMap<InternedId, HouseState>,
    alliances: &HouseAllianceMap,
    terrain: Option<&ResolvedTerrainGrid>,
) -> bool {
    let (Some(victim), Some(attacker)) = (entities.get(victim_id), entities.get(attacker_id))
    else {
        return false;
    };
    if !victim.is_alive()
        || victim.dying
        || !victim.lifecycle.object_alive
        || victim.lifecycle.in_limbo
    {
        return false;
    }

    let Some(victim_type) = rules.object(interner.resolve(victim.type_ref())) else {
        return false;
    };
    if !victim_type.can_retaliate
        || victim.bunker_link.installed_in().is_some()
        || victim.mind_controlled
        || victim
            .capture_manager
            .as_ref()
            .is_some_and(|manager| manager.blocks_retaliation())
        || victim.spawn_manager.is_some()
        || victim_type
            .enslaves
            .as_deref()
            .is_some_and(|slave_type| rules.object_case_insensitive(slave_type).is_some())
    {
        return false;
    }
    if victim
        .mission
        .current()
        .known()
        .and_then(|mission| rules.mission_control.entry(mission))
        .is_some_and(|entry| !entry.retaliate)
    {
        return false;
    }

    let victim_owner = interner.resolve(victim.owner());
    let attacker_owner = interner.resolve(attacker.owner());
    if is_allied_with(alliances, victim_owner, attacker_owner)
        || is_allied_with(alliances, attacker_owner, victim_owner)
    {
        return false;
    }
    // The active human-control branch refuses to replace an existing TarCom.
    // A computer-owned receiver compares raw float10 threat scores at its own
    // coordinate and keeps its current target only when that score is strictly
    // greater. Equal or lower permits the normal retaliation path.
    let is_human = houses
        .get(&victim.owner())
        .is_some_and(|house| house.is_human);
    if is_human && victim.attack_target.is_some() {
        return false;
    }
    let Some(attacker_type) = rules.object(interner.resolve(attacker.type_ref())) else {
        return false;
    };
    let attacker_as_target = techno_target_facts(
        attacker,
        attacker_type,
        terrain,
        is_ally_by_object(Some(alliances), interner, victim.owner(), attacker.owner()),
    );
    let Some(selected) = select_weapon_for_target(
        rules,
        victim_type,
        &attacker_facts(victim, victim_type),
        &attacker_as_target,
    ) else {
        return false;
    };
    if retaliation_peek_fire_error(attacker) == RetaliationPeekFireError::Illegal {
        return false;
    }

    if !is_human
        && let Some(super::TargetKind::Entity(current_id)) =
            victim.attack_target.as_ref().map(|target| target.target)
        && let (Some(current_score), Some(attacker_score)) = (
            calculate_ai_threat_score(
                entities,
                victim_id,
                current_id,
                rules,
                interner,
                terrain,
                Some(alliances),
            ),
            calculate_ai_threat_score(
                entities,
                victim_id,
                attacker_id,
                rules,
                interner,
                terrain,
                Some(alliances),
            ),
        )
        // ShouldRetaliate708A8E spills the current target's score to double;
        // 708A9F..A8 compares the new score and rejects on C0 (Less or Unordered).
        && retaliation_score_refuses(current_score, attacker_score)
    {
        return false;
    }
    // 0x00708AAA..0x00708AC3: a Foot never retaliates against the parasite
    // eating it (reachable for owners that stay visible while attached).
    if victim.parasite_eating_me == Some(attacker_id) {
        return false;
    }

    selected.weapon.range > SimFixed::ZERO && verses_gate(selected.verses_pct) == VersesGate::Normal
}

/// Retaliation system: idle units that were recently hit auto-attack their attacker.
///
/// Called after `tick_combat_with_fog()` in the game loop. Iterates entities
/// that have a `last_attacker_id` but no `attack_target` and no `order_intent`.
/// Skips retaliation if the weapon has 0% or 1% Verses against the attacker's armor.
pub fn tick_retaliation(
    entities: &mut EntityStore,
    rules: &RuleSet,
    interner: &StringInterner,
    live_order: &[u64],
    terrain: Option<&ResolvedTerrainGrid>,
    alliances: Option<&HouseAllianceMap>,
) {
    // Collect retaliation candidates: (retaliator_id, attacker_id).
    let mut retaliators: Vec<(u64, u64)> = Vec::new();
    // Native retaliation is resolved during the same live-object (reveal/insertion
    // order) AI walk as the rest of combat, so scan victims in live order rather
    // than stable-id order.
    for &id in live_order {
        let entity = match entities.get(id) {
            Some(e) => e,
            None => continue,
        };
        // Must have last_attacker, no current attack target, no order intent.
        let attacker_sid = match entity.last_attacker_id {
            Some(sid) => sid,
            None => continue,
        };
        // The `order_intent.is_some()` suppression is the conceptual mission-busy
        // gate (`mission::verb::get_current_mission`), kept LITERAL on purpose: an
        // `is_busy`-only gate would let a Guarding unit (order_intent = Guard,
        // mission idle) begin retaliating — a proven DRIFT. Retiring the
        // `order_intent` predicate in favour of a mission goal field is a later
        // slice; the runtime check stays byte-identical here.
        if entity.attack_target.is_some() || entity.order_intent.is_some() {
            continue;
        }
        // Verify attacker is still alive. A sold/captured attacker keeps health
        // but is `dying` (a corpse awaiting the end-of-tick drain) — exclude it
        // so the victim doesn't retaliate against a dead object.
        let attacker_alive = entities
            .get(attacker_sid)
            .is_some_and(|a| a.health.current > 0 && !a.dying);
        if !attacker_alive {
            continue;
        }
        retaliators.push((id, attacker_sid));
    }

    // Process retaliation — issue attack commands.
    for (entity_id, attacker_sid) in retaliators {
        let retaliate = {
            let entity = match entities.get(entity_id) {
                Some(e) => e,
                None => continue,
            };
            let attacker = match entities.get(attacker_sid) {
                Some(a) => a,
                None => {
                    // Attacker gone — clear last_attacker.
                    if let Some(e) = entities.get_mut(entity_id) {
                        e.last_attacker_id = None;
                    }
                    continue;
                }
            };
            can_retaliate(entity, attacker, rules, interner, terrain, alliances)
        };

        if retaliate {
            // Read attacker rx/ry (only needed for body-only retaliators).
            let attacker_pos = match entities.get(attacker_sid) {
                Some(a) => (a.position.rx, a.position.ry),
                None => continue,
            };
            if let Some(entity) = entities.get_mut(entity_id) {
                if entity.barrel_facing.is_none()
                    && !matches!(
                        entity.category,
                        EntityCategory::Unit | EntityCategory::Infantry
                    )
                {
                    // Body-only retaliator — instantly face the attacker.
                    // Turreted retaliators get their turret driven by
                    // `Facing_Update`, and a TURRETLESS VEHICLE turns its hull
                    // through `UnitClass::Fire_At_Target @ 0x00736DF0` case 2
                    // once the fire gate refuses it for facing — assignment
                    // itself writes no facing in gamemd. Infantry snap when
                    // their fire action starts in resolve_attacker_fire.
                    let dx: i32 = attacker_pos.0 as i32 - entity.position.rx as i32;
                    let dy: i32 = attacker_pos.1 as i32 - entity.position.ry as i32;
                    entity.facing = crate::sim::movement::facing_from_delta(dx, dy);
                }
                entity.movement_target = None;
                entity.attack_target = Some(crate::sim::combat::AttackTarget::new(attacker_sid));
                // Retaliation is a damage-driven target, not a scanner pick.
                entity.passively_acquired_target = false;
            }
        }
        // Clear last_attacker regardless (prevent repeated attempts).
        if let Some(entity) = entities.get_mut(entity_id) {
            entity.last_attacker_id = None;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rules::ini_parser::IniFile;
    use crate::sim::intern::test_interner;

    /// Stock key shape for the two `TurretCount>0` types that carry NO live
    /// `Primary=`, copied from retail `ini/rulesmd.ini`:
    ///
    /// ```text
    /// [SREF]    ; Primary=Comet          <- commented out by Westwood
    ///           ; ElitePrimary=SuperComet
    ///           TurretCount=4  WeaponCount=1  Weapon1=Comet
    /// [YAGGUN]  (no Primary=, no Secondary= anywhere in the section)
    ///           IsGattling=yes  TurretCount=1  WeaponCount=6  Weapon1=AGGattling ...
    /// ```
    ///
    /// `TechnoTypeClass::ReadINI @ 0x007128B2` branches on `TurretCount > 0`
    /// and jumps past the `Primary=` block, so those keys are never read for
    /// either type and `obj.primary`/`obj.secondary` stay `None`.
    fn gunner_rules() -> RuleSet {
        RuleSet::from_ini(&IniFile::from_str(
            "[VehicleTypes]\n0=SREF\n1=HTNK\n\
             [BuildingTypes]\n0=YAGGUN\n\
             [WeaponTypes]\n0=Comet\n1=AGGattling\n\
             [SREF]\nStrength=300\nArmor=heavy\nCost=1200\n\
             TurretCount=4\nWeaponCount=1\nWeapon1=Comet\nEliteWeapon1=Comet\n\
             [YAGGUN]\nStrength=810\nArmor=steel\nCost=1000\n\
             IsGattling=yes\nTurretCount=1\nWeaponCount=6\nWeapon1=AGGattling\n\
             [HTNK]\nStrength=400\nArmor=heavy\nCost=900\nPrimary=Comet\n\
             [Comet]\nDamage=100\nROF=110\nRange=6\nWarhead=WH\n\
             [AGGattling]\nDamage=15\nROF=10\nRange=6\nWarhead=WH\n\
             [WH]\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n",
        ))
        .expect("gunner fixture")
    }

    /// GSI-08.02 regression: the Prism Tank and the Gattling Cannon must read
    /// as armed. `TechnoClass::Is_Armed @ 0x00701120` resolves ONE slot through
    /// `GetCurrentWeapon @ 0x0070E1A0`, which for a `TurretCount>0` type is
    /// `GetWeapon(CurrentWeaponNumber)`.
    ///
    /// This also pins the storage fact the whole crate now depends on:
    /// `Weapon1=` writes the same `TechnoTypeClass+0x898` field that `Primary=`
    /// does (`TechnoTypeClass::ReadINI`, cursor `0x007128D6 LEA EDI,[EBP+0xA94]`
    /// storing at `0x0071294A MOV [EDI-0x1FC],EAX`, and `0xA94-0x1FC = 0x898`),
    /// so `obj.primary` reads as the native field for these two types even
    /// though neither section authors a live `Primary=` key.
    #[test]
    fn gsi_08_02_stock_sref_and_yaggun_are_armed_through_weapon_one() {
        let rules = gunner_rules();
        let sref_obj = rules.object("SREF").expect("SREF");
        let yaggun_obj = rules.object("YAGGUN").expect("YAGGUN");

        // Same storage: `Weapon1=` lands in the `Primary` field, `Weapon2=` in
        // `Secondary`. SREF stops at `WeaponCount=1`, so its slot 1 is empty.
        assert_eq!(sref_obj.primary.as_deref(), Some("Comet"));
        assert_eq!(sref_obj.secondary, None);
        assert_eq!(yaggun_obj.primary.as_deref(), Some("AGGattling"));

        let mut sref = GameEntity::test_default(1, "SREF", "Americans", 5, 5);
        sref.category = EntityCategory::Unit;
        let mut yaggun = GameEntity::test_default(2, "YAGGUN", "YuriCountry", 9, 9);
        yaggun.category = EntityCategory::Structure;

        assert!(super::super::combat_weapon::is_armed(&sref, sref_obj));
        assert!(super::super::combat_weapon::is_armed(&yaggun, yaggun_obj));
    }

    /// The gate this file owns: a Prism Tank must get past
    /// `acquire_best_target_for_entity`'s armed check and pick the enemy tank
    /// next to it. Against the old predicate — which read the raw
    /// `Primary=`/`Secondary=` INI keys instead of the weapon-array fields they
    /// name — this returns `None`, and a Prism Tank on Guard never opened fire
    /// on anything that walked past.
    #[test]
    fn gsi_08_02_sref_acquires_a_target_through_the_armed_gate() {
        let rules = gunner_rules();
        let mut entities = EntityStore::new();

        let mut sref = GameEntity::test_default(1, "SREF", "Americans", 5, 5);
        sref.category = EntityCategory::Unit;
        sref.lifecycle.in_limbo = false;
        sref.lifecycle.cell_marked = true;
        entities.insert(sref);

        let mut enemy = GameEntity::test_default(2, "HTNK", "Russians", 6, 5);
        enemy.category = EntityCategory::Unit;
        enemy.lifecycle.in_limbo = false;
        enemy.lifecycle.cell_marked = true;
        entities.insert(enemy);

        // Snapshot the thread-local test interner only after `test_default`
        // has interned both owners and both type names.
        let interner = test_interner();

        assert_eq!(
            acquire_best_target_for_entity(
                &entities,
                &crate::sim::occupancy::OccupancyGrid::rebuild(&entities),
                &rules,
                &interner,
                1,
                None,
                None,
                false,
                ScanMission::Guard,
                None,
                crate::sim::combat::line_of_fire::LineOfFireInputs::default(),
            ),
            Some(2)
        );
    }
}

/// Original708A8E stores the old score;708A9F..A8 refuses on C0.
pub(super) fn retaliation_score_refuses(current: MaskedX87Value, attacker: MaskedX87Value) -> bool {
    let current = X87Chop53::load_f64(X87Chop53::store_f64_masked_chop(current));
    matches!(
        X87Chop53::compare(attacker, current),
        X87Ordering::Less | X87Ordering::Unordered
    )
}
