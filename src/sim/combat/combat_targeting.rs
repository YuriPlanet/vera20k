//! Target acquisition and retaliation for the combat system.
//!
//! Handles two responsibilities:
//! 1. **Target acquisition** — finding the best hostile target for an idle or
//!    attack-moving unit within its guard/weapon range.
//! 2. **Retaliation** — [`should_retaliate`], `TechnoClass::ShouldRetaliate`'s
//!    gate list, which ReceiveDamage asks before turning a damaged object on
//!    its attacker.
//!
//! ## Target priority
//! `TechnoClass::Greatest_Threat @ 0x006F8DF0` scores every candidate the walk
//! reaches and keeps the maximum, ties going to whatever the walk saw first.
//! The walk, the score and the candidate gates live in
//! [`super::greatest_threat`]; this module owns the snapshot the scan runs on
//! and the retaliation gate.
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

use super::combat_weapon::{
    attacker_facts, is_ally_by_object, select_weapon_for_target, techno_target_facts,
};
use super::threat_range::ScanMission;
use crate::map::entities::EntityCategory;
use crate::map::houses::HouseAllianceMap;
use crate::map::resolved_terrain::ResolvedTerrainGrid;
use crate::rules::object_type::ObjectType;
use crate::rules::ruleset::RuleSet;
use crate::rules::weapon_type::WeaponType;
use crate::sim::entity_store::EntityStore;
use crate::sim::game_entity::GameEntity;
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
    fire_world: Option<&crate::sim::world::Simulation>,
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
        fire_world,
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
    attacker_obj: &ObjectType,
    fog: Option<&FogState>,
    scan_range_override: Option<SimFixed>,
    terrain: Option<&ResolvedTerrainGrid>,
    require_playfield_membership: bool,
    zone_grid: Option<&crate::sim::pathfinding::zone_map::ZoneGrid>,
    los: super::line_of_fire::LineOfFireInputs<'_>,
    fire_world: Option<&crate::sim::world::Simulation>,
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
        fire_world,
    )
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

/// SelectWeapon (vt+0x2E4) of `victim` against `source`, as ShouldRetaliate
/// (`0x007088DB`) and ReceiveDamage's reach gate (`0x00702A5D`) each call it.
/// Every slot of an occupied building answers its occupant's weapon
/// (`BuildingClass::GetWeapon @ 0x004526F0`), so its choice cannot change the
/// weapon read; slot 0 stands for it.
pub(crate) fn retaliation_weapon_index(
    world: &crate::sim::world::Simulation,
    rules: &RuleSet,
    victim: &GameEntity,
    victim_type: &ObjectType,
    source: &GameEntity,
    source_type: &ObjectType,
    garrison: Option<(&WeaponType, SimFixed)>,
) -> Option<i32> {
    if garrison.is_some() {
        return Some(0);
    }
    let allied = is_ally_by_object(
        Some(&world.house_alliances),
        &world.interner,
        victim.owner(),
        source.owner(),
    );
    let source_as_target =
        techno_target_facts(source, source_type, world.resolved_terrain.as_ref(), allied);
    select_weapon_for_target(
        rules,
        victim_type,
        &attacker_facts(victim, victim_type),
        &source_as_target,
    )
    .map(|selected| selected.index)
}

/// `TechnoClass::ShouldRetaliate @ 0x007087C0`, whose only caller is
/// `TechnoClass::ReceiveDamage @ 0x00702A43`: whether the damaged `victim`
/// turns on `source`. Every gate is a pure read (no RNG), taken in native
/// order.
pub(crate) fn should_retaliate(
    world: &crate::sim::world::Simulation,
    rules: &RuleSet,
    victim_id: u64,
    source_id: u64,
) -> bool {
    use crate::rules::object_type::Ability;
    let entities = &world.substrate.entities;
    let interner = &world.interner;
    let (Some(victim), Some(source)) = (entities.get(victim_id), entities.get(source_id)) else {
        return false;
    };
    // The receiver asks only in its surviving arms; a victim the represented
    // lifecycle already retired cannot be one.
    if !victim.is_alive()
        || victim.dying
        || !victim.lifecycle.object_alive
        || victim.lifecycle.in_limbo
    {
        return false;
    }
    let (Some(victim_type), Some(source_type)) = (
        rules.object(interner.resolve(victim.type_ref())),
        rules.object(interner.resolve(source.type_ref())),
    ) else {
        return false;
    };
    let house = world.houses.get(&victim.owner());
    let human =
        house.is_some_and(|house| house.is_controlled_by_human(world.session.game_mode_nonzero));
    // `0x007087DD` CanRetaliate; `0x007087EB` a slave (SlaveOwner `+0x2DC`);
    // `0x007087F9` a slaver (SlaveManager `+0x2D8`).
    // RESIDUAL: `slave_harvester`, VERA's SlaveOwner, is never cleared.
    // Liberation (`0x006B0AE0`, SlaveOwner = 0 at `0x006B0B73`) is not ported,
    // so a slave freed by its master's death, armed with `SHOVEL`, never
    // retaliates, where native's does.
    if !victim_type.can_retaliate
        || victim.slave_harvester.is_some()
        || victim_type
            .enslaves
            .as_deref()
            .is_some_and(|slave_type| rules.object_case_insensitive(slave_type).is_some())
    {
        return false;
    }
    // `0x00708807..0x0070881F`: draining (`+0x1CC`) for a house that is not
    // human (`House+0x1EC`).
    if victim.drain_target.is_some() && !house.is_some_and(|house| house.is_human) {
        return false;
    }
    // `0x0070882F` a full CaptureManager (being controlled is no gate);
    // `0x0070883C` a SpawnManager; `0x0070884A` a human's object that already
    // has a Target; `0x00708867` the mission's `Retaliate=`.
    if victim
        .capture_manager
        .as_ref()
        .is_some_and(|manager| manager.is_full())
        || victim.spawn_manager.is_some()
        || (human && victim.attack_target.is_some())
        || victim
            .mission
            .current()
            .known()
            .and_then(|mission| rules.mission_control.entry(mission))
            .is_some_and(|entry| !entry.retaliate)
    {
        return false;
    }
    // `0x00708880`: the owner's one-way alliance with the source's house;
    // `0x00708899`: the source is disguised to the owner (vt+0xC8).
    let allied = is_ally_by_object(
        Some(&world.house_alliances),
        interner,
        victim.owner(),
        source.owner(),
    );
    if allied
        || crate::sim::cloak_disguise::object_disguised_to(
            source,
            victim.owner(),
            Some(&world.fog),
            Some(&world.house_alliances),
            interner,
        )
    {
        return false;
    }
    // Every weapon read below goes through GetWeapon (vt+0x3F8), which for an
    // occupied building answers its firing occupant's weapon for every slot
    // (`BuildingClass::GetWeapon @ 0x004526F0`).
    let target = super::TargetKind::Entity(source_id);
    let garrison =
        super::fire_error_world::garrison_weapon(world, rules, victim, victim_type, target);
    let mut subject = super::fire_error_world::FireSubject {
        world,
        rules,
        overlay_registry: None,
        fog: Some(&world.fog),
        firer: victim,
        obj: victim_type,
        target: Some(target),
        weapon_index: 0,
        garrison,
    };
    // `0x007088A7` GetWeaponDamageValue(-1) > 0 (a healer never retaliates);
    // `0x007088BC` Is_Armed (`BuildingClass::Is_Armed @ 0x00458DB0` answers
    // true for an occupied building).
    if subject.weapon_damage_value() <= 0 || !super::combat_weapon::is_armed(victim, victim_type) {
        return false;
    }
    // `0x007088CA..0x007088FF`: SelectWeapon(source), then GetFireError
    // without the range test (vt+0x3BC); Illegal or Cant refuses.
    let Some(weapon_index) = retaliation_weapon_index(
        world,
        rules,
        victim,
        victim_type,
        source,
        source_type,
        garrison,
    ) else {
        return false;
    };
    subject.weapon_index = weapon_index;
    if matches!(
        subject.fire_error(false),
        super::fire_error::FireError::Illegal | super::fire_error::FireError::Cant
    ) {
        return false;
    }
    if human {
        // `0x00708905..0x007089A5`: a human's C4 infantryman, or a rank that
        // holds C4, leaves a building source alone.
        if source.category == EntityCategory::Structure
            && ((victim.category == EntityCategory::Infantry && victim_type.c4)
                || super::veterancy::has_weapon_ability(
                    super::veterancy::rank_from_u16(victim.veterancy),
                    victim_type,
                    Ability::C4,
                ))
        {
            return false;
        }
        // `0x007089AF..0x007089E2`: a human's unit that deploys into an
        // `Artillary=` building (none in retail).
        if victim.category == EntityCategory::Unit
            && victim_type
                .deploys_into
                .as_deref()
                .and_then(|building| rules.object(building))
                .is_some_and(|building| building.artillary)
        {
            return false;
        }
        // `0x007089E8..0x00708A26`: unless `PlayerReturnFire=`, a human's
        // non-building object retaliates only on Guard, Area Guard or Patrol.
        const GUARD: i32 = 5;
        const AREA_GUARD: i32 = 0x0B;
        const PATROL: i32 = 0x19;
        if !rules.general.player_return_fire
            && victim.category != EntityCategory::Structure
            && !matches!(victim.mission.current().raw(), GUARD | AREA_GUARD | PATROL)
        {
            return false;
        }
    }
    // `0x00708A2C..0x00708A54`: a member of a `Suicide=` team.
    if victim.category != EntityCategory::Structure
        && world
            .team_script_vm
            .member_team_type(victim_id)
            .is_some_and(|team_type| team_type.suicide)
    {
        return false;
    }
    // `0x00708A5A..0x00708AA8`: a computer house keeps a current object
    // target (the `+0x14` Object flag, `0x00708A73`) whose raw float10 threat
    // score is strictly greater.
    if !human
        && let Some(super::TargetKind::Entity(current_id)) =
            victim.attack_target.as_ref().map(|target| target.target)
        && let (Some(current_score), Some(source_score)) = (
            calculate_ai_threat_score(
                entities,
                victim_id,
                current_id,
                rules,
                interner,
                world.resolved_terrain.as_ref(),
                Some(&world.house_alliances),
            ),
            calculate_ai_threat_score(
                entities,
                victim_id,
                source_id,
                rules,
                interner,
                world.resolved_terrain.as_ref(),
                Some(&world.house_alliances),
            ),
        )
        && retaliation_score_refuses(current_score, source_score)
    {
        return false;
    }
    // `0x00708AAA..0x00708AC3`: a Foot never turns on the parasite eating it.
    if victim.parasite_eating_me == Some(source_id) {
        return false;
    }
    // `0x00708AC5..0x00708B09`: the selected weapon's Verses against the
    // source's armour must exceed the single 0.01 (`0x007F4E34`), so a 1%
    // warhead does retaliate and 0% (or NaN) does not. A missing weapon or
    // warhead skips the test and retaliates (`0x00708AD4`, `0x00708ADE`).
    subject
        .weapon_at(subject.weapon_index)
        .and_then(|weapon| super::combat_weapon::warhead_of(rules, weapon))
        .is_none_or(|warhead| {
            warhead.verses_f64[super::armor_index(&source_type.armor)] > f64::from(0.01_f32)
        })
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
                None,
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
