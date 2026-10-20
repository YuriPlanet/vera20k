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
//! of class deployment state — only the deployment owners
//! advance set it.
//!
//! ## Dependency rules
//! - Part of sim/ — depends on rules/ (RuleSet) and sim/components.
//! - sim/ NEVER depends on render/, ui/, audio/, net/.

use super::TargetKind;
use super::combat_weapon::{
    attacker_facts, is_ally_by_object, techno_target_facts, what_weapon_should_i_use,
};
use super::threat_range::{self, ScanMission};
use crate::map::entities::EntityCategory;
use crate::map::houses::HouseAllianceMap;
use crate::map::resolved_terrain::ResolvedTerrainGrid;
use crate::rules::object_type::ObjectType;
use crate::rules::ruleset::RuleSet;
use crate::sim::entity_store::EntityStore;
use crate::sim::game_entity::GameEntity;
use crate::sim::intern::{InternedId, StringInterner};
use crate::sim::vision::FogState;
use crate::util::fixed_math::SimFixed;
use crate::util::native_x87::{
    MaskedX87Chop53 as X87Chop53, MaskedX87Ordering as X87Ordering, MaskedX87Value,
};

/// The fire state of an occupied building attacker
/// ([`super::combat_weapon::is_occupied`]), read in Phase 1. Its weapon is
/// GetWeapon's, read live ([`super::combat_weapon::get_weapon`]).
#[derive(Clone)]
pub(crate) struct GarrisonSnapshot {
    /// `+0x69C`, the firing occupant's index: the shot's muzzle port.
    pub fire_index: u8,
    /// The occupant count (`+0x694`), GetROF's divisor.
    pub occupant_count: u8,
}

/// Snapshot of an attacker's state for target scanning.
/// Extracted to avoid borrow conflicts during entity iteration.
#[derive(Clone)]
pub(crate) struct AttackerSnapshot {
    pub stable_id: u64,
    pub owner: InternedId,
    pub category: EntityCategory,
    /// What the attacker is firing at — entity ID or cell coord.
    /// Cell targets skip friendly-fire checks (the player explicitly chose
    /// this cell).
    pub target: TargetKind,
    pub pos_rx: u16,
    pub pos_ry: u16,
    pub pos_z: u8,
    pub pos_exact_z_leptons: Option<i32>,
    pub sub_x: SimFixed,
    pub sub_y: SimFixed,
    pub type_id: InternedId,
    pub veterancy: u16,
    pub barrel_facing: Option<crate::sim::movement::FacingClass>,
    /// Body FacingClass (`+0x388`), including infantry fire-start snaps and
    /// vehicle turns. Facing gates and emission read its full 16-bit value.
    pub hull_facing: crate::sim::movement::FacingClass,
    /// Garrison state — present only for garrisoned buildings (IsOccupied).
    pub garrison: Option<GarrisonSnapshot>,
    /// The threat mask this scan's CALLER pushed — `Greatest_Threat`'s second
    /// argument, always a literal in retail. It selects the radius formula
    /// (Area Guard reaches roughly twice as far as plain Guard) and, for mask
    /// 0, the scan topology itself.
    pub scan_mission: ScanMission,
    /// Call-local arguments of a building's synchronous FireAt. Its caller
    /// already admitted GetFireError; no deferred queue retains the shot.
    pub building_shot: Option<super::BuildingShot>,
}

/// `attacker_id`'s `Greatest_Threat` through its `+0x3C4` override, the one
/// port every scan asks: `TechnoClass::Retaliate_And_Scan @ 0x00709820`
/// (vt+0x39C for every class, Aircraft included) and the direct `+0x3C4`
/// calls (team script actions, Mission_Rescue, the aircraft's own
/// Mission_Hunt and others). Neither `Greatest_Threat` nor the Foot override
/// (`0x004D9920`) tests ammo or `Is_Armed`. A caller that wants an armed
/// scanner asks `CanAcquireTarget @ 0x007091D0` first (the passive gate, Foot
/// Mission_AreaGuard at `0x004D6ED9`), whose `Is_Armed` has no ammo term;
/// Foot Mission_Hunt (`0x004D5392`) and Arrival_Target_Handler scan without
/// it, and Aircraft Mission_Hunt (`0x00414A80`) checks its own ammo. Reports
/// the target and whether the concrete dispatch reached Foot.
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
/// the scanner's `+0x3C4` override rewrites it first, which
/// [`super::greatest_threat::greatest_threat`] applies ([`super::threat_mask`]);
/// including Foot+688's coercion to `(mask & ~2) | 1` (`0x004D9931`).
/// The world dispatch commits Foot's empty-result latch write before any
/// subsequent class target assignment.
///
/// `zone_grid` is `MapClass`'s per-movement-zone connectivity, which mask 0 uses
/// to refuse candidates its own movement zone cannot reach.
pub(crate) fn greatest_threat_for_entity(
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
    scan_coord: Option<[i32; 3]>,
) -> super::greatest_threat::ThreatScanOutcome {
    let Some(entity) = entities.get(attacker_id) else {
        return super::greatest_threat::ThreatScanOutcome::default();
    };
    let Some(obj) = interner.object_type(entity.type_ref(), rules) else {
        return super::greatest_threat::ThreatScanOutcome::default();
    };
    // Dummy target: no current target when acquiring fresh.
    let snapshot = AttackerSnapshot {
        scan_mission: mask,
        ..build_attacker_snapshot(entity, TargetKind::Entity(0), None)
    };
    super::greatest_threat::greatest_threat(
        entities,
        occupancy,
        rules,
        interner,
        &snapshot,
        obj,
        fog,
        terrain,
        require_playfield_membership,
        zone_grid,
        los,
        fire_world,
        scan_coord,
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
    scorer_enemy_house: Option<crate::sim::intern::InternedId>,
) -> Option<MaskedX87Value> {
    let scorer = entities.get(scorer_id)?;
    let scorer_type = interner.object_type(scorer.type_ref(), rules)?;
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
        scorer_enemy_house,
    )
}

/// SelectWeapon (vt+0x2E4) of `victim` against `source`, as ShouldRetaliate
/// (`0x007088DB`) and ReceiveDamage's reach gate (`0x00702A5D`) each call it.
/// An occupied building answers slot 0 (ladder arm B), and its GetWeapon
/// answers the occupant's weapon for every slot (`0x004526F0`). The index is
/// the ladder's; GetFireError judges it.
pub(crate) fn retaliation_weapon_index(
    world: &crate::sim::world::Simulation,
    rules: &RuleSet,
    victim: &GameEntity,
    victim_type: &ObjectType,
    source: &GameEntity,
    source_type: &ObjectType,
) -> i32 {
    let allied = is_ally_by_object(
        Some(world.house_alliances.as_map()),
        &world.interner,
        victim.owner(),
        source.owner(),
    );
    let source_as_target = techno_target_facts(
        source,
        source_type,
        world.resolved_terrain.as_ref(),
        allied,
        rules,
        &world.interner,
    );
    what_weapon_should_i_use(
        rules,
        victim_type,
        &attacker_facts(victim, victim_type),
        Some(&source_as_target),
    )
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
        || victim.death.is_dying()
        || !victim.lifecycle.object_alive()
        || victim.lifecycle.in_limbo()
    {
        return false;
    }
    let (Some(victim_type), Some(source_type)) = (
        interner.object_type(victim.type_ref(), rules),
        interner.object_type(source.type_ref(), rules),
    ) else {
        return false;
    };
    let house = world.houses.get(&victim.owner());
    let human =
        house.is_some_and(|house| house.is_controlled_by_human(world.session.game_mode_nonzero()));
    // `0x007087DD` CanRetaliate; `0x007087EB` a slave (SlaveOwner `+0x2DC`);
    // `0x007087F9` a slaver (SlaveManager `+0x2D8`). A slave freed by its
    // master's death (`0x006B0B73`) retaliates again.
    if !victim_type.can_retaliate
        || victim.slave.owner().is_some()
        || victim.slave_manager.is_some()
    {
        return false;
    }
    // `0x00708807..0x0070881F`: draining (`+0x1CC`) for a house that is not
    // human (`House+0x1EC`).
    if victim.drain.target().is_some() && !house.is_some_and(|house| house.is_human()) {
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
        || (human && victim.attack_target.target().is_some())
        || crate::sim::mission::control::current_control(rules, &victim.mission)
            .is_some_and(|entry| !entry.retaliate)
    {
        return false;
    }
    // `0x00708880`: the owner's one-way alliance with the source's house;
    // `0x00708899`: the source is disguised to the owner (vt+0xC8).
    let allied = is_ally_by_object(
        Some(world.house_alliances.as_map()),
        interner,
        victim.owner(),
        source.owner(),
    );
    if allied
        || source.is_disguised_to(
            victim.owner(),
            Some(&world.fog),
            Some(world.house_alliances.as_map()),
            interner,
        )
    {
        return false;
    }
    // Every weapon read below goes through GetWeapon (vt+0x3F8), which for an
    // occupied building answers its firing occupant's weapon for every slot
    // (`BuildingClass::GetWeapon @ 0x004526F0`).
    let mut subject = super::fire_error_world::FireSubject {
        world,
        rules,
        overlay_registry: None,
        fog: Some(world.fog.fire_view(world.house_alliances.as_map())),
        firer: victim,
        obj: victim_type,
        target: Some(TargetKind::Entity(source_id)),
        weapon_index: 0,
    };
    // `0x007088A7` GetWeaponDamageValue(-1) > 0 (a healer never retaliates);
    // `0x007088BC` Is_Armed (`BuildingClass::Is_Armed @ 0x00458DB0` answers
    // true for an occupied building).
    if subject.weapon_damage_value() <= 0 || !super::combat_weapon::is_armed(victim, victim_type) {
        return false;
    }
    // `0x007088CA..0x007088FF`: SelectWeapon(source), then GetFireError
    // without the range test (vt+0x3BC); Illegal or Cant refuses.
    subject.weapon_index =
        retaliation_weapon_index(world, rules, victim, victim_type, source, source_type);
    if matches!(
        subject.fire_error(false),
        super::fire_error::FireError::Illegal | super::fire_error::FireError::Cant
    ) {
        return false;
    }
    if human {
        // `0x00708905..0x007089A5`: a human's C4 infantryman, or a rank that
        // holds C4, leaves a building source alone.
        if source.category() == EntityCategory::Structure
            && ((victim.category() == EntityCategory::Infantry && victim_type.c4)
                || super::veterancy::has_weapon_ability(
                    victim.veterancy.rank_kind(),
                    victim_type,
                    Ability::C4,
                ))
        {
            return false;
        }
        // `0x007089AF..0x007089E2`: a human's unit that deploys into an
        // `Artillary=` building (none in retail).
        if victim.category() == EntityCategory::Unit
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
            && victim.category() != EntityCategory::Structure
            && !matches!(victim.mission.current().raw(), GUARD | AREA_GUARD | PATROL)
        {
            return false;
        }
    }
    // `0x00708A2C..0x00708A54`: a member of a `Suicide=` team.
    if victim.category() != EntityCategory::Structure
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
    let victim_enemy = world
        .houses
        .get(&victim.owner())
        .and_then(|house| house.enemy_house());
    if !human
        && let Some(TargetKind::Entity(current_id)) = victim.attack_target.target()
        && let (Some(current_score), Some(source_score)) = (
            calculate_ai_threat_score(
                entities,
                victim_id,
                current_id,
                rules,
                interner,
                world.resolved_terrain.as_ref(),
                Some(world.house_alliances.as_map()),
                victim_enemy,
            ),
            calculate_ai_threat_score(
                entities,
                victim_id,
                source_id,
                rules,
                interner,
                world.resolved_terrain.as_ref(),
                Some(world.house_alliances.as_map()),
                victim_enemy,
            ),
        )
        && retaliation_score_refuses(current_score, source_score)
    {
        return false;
    }
    // `0x00708AAA..0x00708AC3`: a Foot never turns on the parasite eating it.
    if victim.parasite.eater() == Some(source_id) {
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

/// Original708A8E stores the old score;708A9F..A8 refuses on C0.
pub(super) fn retaliation_score_refuses(current: MaskedX87Value, attacker: MaskedX87Value) -> bool {
    let current = X87Chop53::load_f64(X87Chop53::store_f64_masked_chop(current));
    matches!(
        X87Chop53::compare(attacker, current),
        X87Ordering::Less | X87Ordering::Unordered
    )
}

/// Build the per-attacker fire snapshot from current entity state. PURE READ —
/// the caller has already decremented cooldown/burst-delay for this tick and
/// read any garrison's fire state. Single source of the snapshot field-reads so
/// non-Unit class hosts and the Unit live Fire→Facing host use the same
/// field reads.
pub(crate) fn build_attacker_snapshot(
    entity: &GameEntity,
    target: TargetKind,
    garrison: Option<GarrisonSnapshot>,
) -> AttackerSnapshot {
    AttackerSnapshot {
        stable_id: entity.stable_id(),
        owner: entity.owner(),
        category: entity.category(),
        target,
        pos_rx: entity.position.rx(),
        pos_ry: entity.position.ry(),
        pos_z: entity.position.z(),
        pos_exact_z_leptons: entity.position.exact_z_leptons(),
        sub_x: entity.position.sub_x(),
        sub_y: entity.position.sub_y(),
        type_id: entity.type_ref(),
        veterancy: entity.veterancy.rank(),
        barrel_facing: entity.barrel_facing,
        hull_facing: entity.body_facing,
        garrison,
        scan_mission: threat_range::scan_mission_for(entity),
        building_shot: None,
    }
}
