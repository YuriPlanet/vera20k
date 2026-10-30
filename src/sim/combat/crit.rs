//! Phobos warhead critical hits and the per-target warhead pass that carries
//! them.
//!
//! Native identity: `WarheadTypeExt::Detonate`'s per-target prefix and pass
//! (`src/Ext/WarheadType/Detonate.cpp:156..199`), `DetonateOnOneUnit`
//! (`201..275`), `GetCritChance`/`ApplyCrit` (`525..608`, `735..778`), called
//! from the `BulletClass_Detonate @ 0x46920B` and `MapClass_DamageArea
//! @ 0x489286` hooks. The pass runs before the native area damage, so a hit
//! it kills is already gone when the main warhead dispatches.
//!
//! RESIDUALS:
//! - The pass runs only in the ordinary-damage arm of the bullet detonation
//!   and in `MapClass::DamageArea`; Phobos runs it before the whole bullet arm
//!   chain (`0x46920B`). A warhead with a special arm (IvanBomb, MindControl,
//!   Temporal, Locomotor, ...) therefore skips its per-target effects and the
//!   prefix draws.
//! - Of `PossibleCellSpreadDetonate`'s members VERA reads only the
//!   AttachEffect lists. A warhead that sets only an unread member
//!   (`RemoveDisguise`, `Shield.*`, `Convert.*`, `ReturnWarhead`, `Taunt`, ...)
//!   skips the two buffer draws native makes per detonation, shifting the
//!   scenario stream; the members' own effects are not ported.
//! - `Detonate`'s earlier per-house arms (`CreateGap`, `Reveal`,
//!   `TransactMoney`, `SpawnsCrate.Types`' `RandomDouble`, `LaunchSW`) and
//!   `InterceptBullets` are not ported; a warhead with `SpawnsCrate.Types`
//!   misses one draw before this prefix.
//! - The per-target list reuses the area receiver collection instead of
//!   Phobos' own `getCellSpreadItemsExt` walk, which also reads the
//!   `AircraftTracker` for airborne items; membership and order can differ
//!   (no airborne phase, different cell-list filters). Because that collection
//!   happens after SpawnShrapnel, the pass also runs after the shrapnel
//!   draws, where native runs it first.
//! - `CanAffectTarget`'s `AffectsAbovePercent`/`AffectsBelowPercent`,
//!   `AffectsVeterancy` and `AffectsInvokerOnly` gates are unread and pass.
//! - `Crit.Warhead`'s full detonation creates a projectile with no
//!   `Projectile=` type, so the default bullet type's `Cluster` is not read.
//! - `Crit.ExtraDamage.ApplyFirepowerMult` reads the owner's live multiplier;
//!   native uses the bullet's firepower captured at launch.
//! - Crit animations set the owner house but no invoker (VERA stores no anim
//!   invoker on this branch).
//! - `Crit.SuppressWhenIntercepted` is read but VERA tracks no intercepted
//!   bullet, so the bullet's `false` value never suppresses.
//! - `Crit.AffectsTarget` treats every building as a plain building; native's
//!   `IsStrange` split has no VERA reader.
//! - `ApplyPerTargetEffectsOnDetonate=no`: native skips this prefix and runs
//!   `DetonateOnOneUnit` from `TechnoClass::ReceiveDamage` for every target
//!   that receives non-zero damage, with the attacker's house. VERA keeps the
//!   pass here on the same targets with per-target dice and no prefix draws,
//!   so the targets and the order against the receivers differ.

use super::combat_aoe::AreaDamageReceiver;
use super::effects::DeathEffects;
use super::receiver_call::EntityDamageEvent;
use super::world_receiver::{
    ReceiverRun, apply_area_damage_in_run_with_target, commit_entities, commit_projectiles,
};
use crate::map::entities::EntityCategory;
use crate::rules::object_type::ObjectType;
use crate::rules::phobos_crit::{AffectedTarget, PhobosAbilityFlags};
use crate::rules::ruleset::RuleSet;
use crate::rules::terrain_rules::LandType;
use crate::rules::warhead_type::WarheadType;
use crate::sim::combat::veterancy::{VeterancyRank, has_weapon_ability};
use crate::sim::intern::InternedId;
use crate::sim::projectile::{
    ProjectileCoord, ProjectileDetonation, ProjectileDetonationReason, ProjectilePayload,
    ProjectileTarget,
};
use crate::sim::world::Simulation;

/// The per-detonation crit state `WarheadTypeExt::Detonate` sets before its
/// per-target pass: the firer's chance once, and the dice buffer the whole
/// detonation shares unless `Crit.ApplyChancePerTarget` is set.
pub(crate) struct CritRoll {
    chance: f64,
    buffer: f64,
    per_target: bool,
}

/// `WarheadTypeExt::Detonate`'s per-target prefix and pass
/// (`Detonate.cpp:156..199`): `Crit_CurrentChance = GetCritChance(owner)`;
/// when the warhead has a per-target effect or a chance, the crit and
/// `ReturnWarhead` buffers and `Crit.ActiveChanceAnims`; then
/// `DetonateOnOneUnit` on every collected receiver of a cell-spread warhead,
/// or on `intended_target` (the bullet's target, or `DamageAreaTarget`) within
/// 64 leptons of `impact`. `source_house` is the hook's house; a direct
/// `DamageArea` without one takes the owner's (`0x489286`). Returns
/// `Crit_Active`, which the damage-animation selection reads.
#[cfg(feature = "phobos")]
#[allow(clippy::too_many_arguments)]
pub(crate) fn detonate(
    world: &mut Simulation,
    run: &mut ReceiverRun<'_>,
    rules: &RuleSet,
    overlay_registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
    warhead: &WarheadType,
    warhead_ref: InternedId,
    source_id: u64,
    source_house: Option<InternedId>,
    impact: ProjectileCoord,
    intended_target: Option<u64>,
    receivers: &[AreaDamageReceiver],
    effects: &mut DeathEffects,
) -> bool {
    let crit = &warhead.crit;
    let globals = rules.crit_globals();
    let house = source_house.or_else(|| {
        world
            .substrate
            .entities
            .get(source_id)
            .map(|source| source.owner())
    });
    let chance = get_crit_chance(world, rules, warhead, source_id);
    // `PossibleCellSpreadDetonate`, of which VERA reads the AttachEffect lists.
    let attach = !warhead.attach_effect.is_empty();
    if !(attach || chance > 0.0) {
        return false;
    }
    let on_detonate = warhead
        .apply_per_target_effects_on_detonate
        .unwrap_or(globals.apply_per_target_effects_on_detonate());
    let mut roll = CritRoll {
        chance,
        buffer: 0.0,
        per_target: true,
    };
    if on_detonate {
        roll.per_target = crit.apply_chance_per_target(globals);
        if !roll.per_target {
            roll.buffer = world.scenario_rng.random_double();
        }
        // `ReturnWarhead_RandomBuffer`: drawn for every such detonation, read
        // only by the unported `ApplyReturnWarhead`.
        if !crit.return_warhead_apply_chance_per_target(globals) {
            world.scenario_rng.random_double();
        }
        // One random pick per detonation, even when every target later fails a
        // gate: `CreateRandomAnim` draws before it constructs.
        if !crit.active_chance_anims().is_empty() && chance > 0.0 {
            let index = world.scenario_rng.next_range_i32_inclusive(
                0,
                i32::try_from(crit.active_chance_anims().len() - 1)
                    .expect("native anim count fits i32"),
            );
            let name = crit.active_chance_anims()[index as usize].clone();
            spawn_anim_at_world_coord(world, rules, &name, impact, house);
        }
    }
    let invoker = (source_id != 0).then_some(source_id);
    let mut active = false;
    for target_id in detonation_targets(world, warhead, impact, intended_target, receivers) {
        if !can_detonate_on(world, rules, warhead, house, target_id) {
            continue;
        }
        if attach {
            world.apply_warhead_attach_effects(
                target_id,
                warhead_ref,
                invoker,
                house,
                rules,
                run.effects(),
                overlay_registry,
            );
        }
        if chance > 0.0
            && apply_one(
                world,
                run,
                rules,
                overlay_registry,
                warhead,
                warhead_ref,
                source_id,
                house,
                &roll,
                target_id,
                effects,
            )
        {
            active = true;
        }
    }
    active
}

/// `DetonateOnOneUnit`'s target gate (`Detonate.cpp:203..207`): a live,
/// placed, non-sinking target not being warped out, then
/// `WarheadTypeExt::CanTargetHouse` and `CanAffectTarget`.
#[cfg(feature = "phobos")]
fn can_detonate_on(
    world: &Simulation,
    rules: &RuleSet,
    warhead: &WarheadType,
    house: Option<InternedId>,
    target_id: u64,
) -> bool {
    let Some(target) = world.substrate.entities.get(target_id) else {
        return false;
    };
    if target.lifecycle.in_limbo()
        || !target.lifecycle.object_alive()
        || target.health.current() <= 0
        || target.sinking.is_active()
        || target.is_warped_out()
    {
        return false;
    }
    if !crate::sim::shield::can_target_house(warhead, house, target.owner(), world) {
        return false;
    }
    can_affect_target(world, rules, warhead, target)
}

/// `WarheadTypeExt::CanAffectTarget` (`WarheadType/Body.cpp:37..57`) past
/// its unread threshold gates: with `EffectsRequireVerses`, the
/// `AffectsAir`/`AffectsGround` split on `IsInAir` (`0x005F6B90`) and a
/// non-zero `GetWarheadVersusArmor`. Under Ares that is the raw Verses;
/// without it `GetTotalDamage(100, WH, armor, 0) / 100`.
#[cfg(feature = "phobos")]
fn can_affect_target(
    world: &Simulation,
    rules: &RuleSet,
    warhead: &WarheadType,
    target: &crate::sim::game_entity::GameEntity,
) -> bool {
    if !warhead.effects_require_verses {
        return true;
    }
    let in_air = crate::sim::movement::air_movement::is_high_flying(
        target,
        world.resolved_terrain.as_ref(),
        Some((rules, &world.interner)),
    );
    if (in_air && !warhead.affects_air) || (!in_air && !warhead.affects_ground) {
        return false;
    }
    let Some(target_type) = world.interner.object_type(target.type_ref(), rules) else {
        return false;
    };
    let armor = crate::sim::shield::warhead_versus_armor(target, rules, warhead, target_type);
    let verses = warhead.interaction(armor).multiplier();
    #[cfg(feature = "ares")]
    {
        f64::from_bits(verses.bits()) != 0.0
    }
    #[cfg(not(feature = "ares"))]
    {
        super::damage::kernel::apply_warhead_damage(
            100,
            warhead.cell_spread_f64,
            warhead.percent_at_max_f64,
            verses,
            0,
            world.session.no_damage(),
            rules.combat_damage.max_damage,
        ) != 0
    }
}

/// `WarheadTypeExt::GetCritChance` (`Detonate.cpp:745..791`): zero for a
/// warhead that cannot crit, else its `Crit.Chance`, the `CRITCHANCE` rank
/// ability's `VeteranCritChance` and the AttachEffect fold.
#[cfg(feature = "phobos")]
fn get_crit_chance(
    world: &Simulation,
    rules: &RuleSet,
    warhead: &WarheadType,
    owner_id: u64,
) -> f64 {
    if !warhead.crit.can_crit() {
        return 0.0;
    }
    let mut chance = warhead.crit.chance();
    if owner_id == 0 {
        return chance;
    }
    let Some(owner) = world.substrate.entities.get(owner_id) else {
        return chance;
    };
    let Some(object) = world.interner.object_type(owner.type_ref(), rules) else {
        return chance;
    };
    let rank = owner.veterancy.rank_kind();
    if has_phobos_ability(rank, object, PhobosAbilityFlags::CRIT_CHANCE) {
        let veteran = object
            .veteran_crit_chance()
            .unwrap_or(rules.crit_globals().veteran_crit_chance());
        chance *= veteran.max(0.0);
    }
    // Phobos `GetCritChance`'s AttachEffect fold: every active effect whose
    // block is more than the identity multiplies the chance and adds its
    // extra chance, gated by the Allow/Disallow warhead lists.
    if !owner.phobos_stat_multipliers().has_crit_modifiers {
        return chance;
    }
    let effects = rules.attach_effect_types().types();
    let mut extra_chance = 0.0;
    for (_, effect) in owner.phobos_attach_effects().active_effects(effects) {
        if !effect.has_crit_modifiers() {
            continue;
        }
        if !effect.crit_allow_warheads.is_empty()
            && !effect
                .crit_allow_warheads
                .iter()
                .any(|name| name.eq_ignore_ascii_case(&warhead.id))
        {
            continue;
        }
        if effect
            .crit_disallow_warheads
            .iter()
            .any(|name| name.eq_ignore_ascii_case(&warhead.id))
        {
            continue;
        }
        chance *= effect.crit_multiplier.max(0.0);
        extra_chance += effect.crit_extra_chance;
    }
    chance + extra_chance
}

#[cfg(feature = "phobos")]
fn has_phobos_ability(rank: VeterancyRank, object: &ObjectType, slot: usize) -> bool {
    match rank {
        VeterancyRank::Rookie => false,
        VeterancyRank::Veteran => object.phobos_abilities().has(slot, false),
        VeterancyRank::Elite => object.phobos_abilities().has(slot, true),
    }
}

/// One `ApplyCrit`: dice, immunity/health/house/cell/techno gates in native
/// order, then the crit's animations and damage.
#[cfg(feature = "phobos")]
#[allow(clippy::too_many_arguments)]
fn apply_one(
    world: &mut Simulation,
    run: &mut ReceiverRun<'_>,
    rules: &RuleSet,
    overlay_registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
    warhead: &WarheadType,
    warhead_ref: InternedId,
    source_id: u64,
    source_house: Option<InternedId>,
    roll: &CritRoll,
    target_id: u64,
    effects: &mut DeathEffects,
) -> bool {
    // `InApplyCrit`: a crit whose own damage detonates this same warhead
    // cannot crit again while the outer apply is running.
    if run.crit_warheads.contains(&warhead_ref) {
        return false;
    }
    let crit = &warhead.crit;
    // `ApplyChancePerTarget || !ApplyPerTargetEffectsOnDetonate` picks a fresh
    // draw; the shared buffer otherwise.
    let dice = if roll.per_target {
        world.scenario_rng.random_double()
    } else {
        roll.buffer
    };
    if roll.chance < dice {
        return false;
    }
    let Some(target) = world.substrate.entities.get(target_id) else {
        return false;
    };
    let target_object = world.interner.object_type(target.type_ref(), rules);
    if target_object.is_some_and(|object| object.immune_to_crit()) {
        return false;
    }
    if target_object.is_some_and(|object| {
        has_phobos_ability(
            target.veterancy.rank_kind(),
            object,
            PhobosAbilityFlags::CRIT_IMMUNE,
        )
    }) {
        return false;
    }
    if target.shield.as_ref().is_some_and(|state| {
        state.is_active()
            && rules
                .shield_type(state.type_name())
                .is_some_and(|ty| ty.immune_to_crit())
    }) {
        return false;
    }
    let strength = target_object.map_or(0, |object| object.strength);
    if !super::health_in_threshold(
        target.health.current(),
        strength,
        crit.affects_above_percent(),
        crit.affects_below_percent(),
    ) {
        return false;
    }
    if let Some(source) = source_house {
        let target_owner = target.owner();
        if !super::can_target_house(
            crit.affects_house(),
            source,
            target_owner,
            &world.houses,
            world.house_alliances.as_map(),
            &world.interner,
        ) {
            return false;
        }
    }
    let land = world.resolved_terrain.as_ref().and_then(|terrain| {
        terrain
            .cell(target.position.rx(), target.position.ry())
            .and_then(|cell| LandType::from_index(cell.land_type))
    });
    // `explicitEmptyCells = false`, no bridge consideration.
    if !cell_eligible(land, crit.affects_target(), None, false) {
        return false;
    }
    if !techno_eligible(target.category(), crit.affects_target()) {
        return false;
    }

    run.crit_warheads.push(warhead_ref);
    let applied = crit_effects(
        world,
        run,
        rules,
        overlay_registry,
        warhead,
        warhead_ref,
        source_id,
        source_house,
        target_id,
        effects,
    );
    run.crit_warheads.pop();
    applied
}

#[cfg(feature = "phobos")]
#[allow(clippy::too_many_arguments)]
fn crit_effects(
    world: &mut Simulation,
    run: &mut ReceiverRun<'_>,
    rules: &RuleSet,
    overlay_registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
    warhead: &WarheadType,
    warhead_ref: InternedId,
    source_id: u64,
    source_house: Option<InternedId>,
    target_id: u64,
    effects: &mut DeathEffects,
) -> bool {
    let crit = &warhead.crit;
    let globals = rules.crit_globals();
    let Some(target) = world.substrate.entities.get(target_id) else {
        return false;
    };
    let coord = crate::sim::movement::ground_pose::object_get_coords(
        target,
        world.resolved_terrain.as_ref(),
    );
    let anim_coord = crate::sim::anim_class::AnimWorldCoord {
        x: coord.x,
        y: coord.y,
        z: coord.z,
    };
    let owner_house = source_house.or_else(|| {
        world
            .substrate
            .entities
            .get(source_id)
            .map(|source| source.owner())
    });

    // `Crit.AnimList` at the affected target; the detonation's damage
    // animation only takes the list when `Crit.AnimOnAffectedTargets` is off.
    if crit.anim_on_affected_targets(globals) && !crit.anim_list().is_empty() {
        if crit.anim_list_create_all() {
            for name in crit.anim_list() {
                spawn_anim_at_coord(world, rules, name, anim_coord, owner_house);
            }
        } else {
            // `EMEffect || Crit_AnimList_PickRandom` selects a random entry;
            // otherwise index 0.
            let index = if warhead.em_effect || crit.anim_list_pick_random() {
                world.scenario_rng.next_range_i32_inclusive(
                    0,
                    i32::try_from(crit.anim_list().len() - 1).expect("native anim count fits i32"),
                )
            } else {
                0
            };
            let name = crit.anim_list()[index as usize].clone();
            spawn_anim_at_coord(world, rules, &name, anim_coord, owner_house);
        }
    }

    let mut damage = crit.extra_damage();
    if crit.extra_damage_apply_firepower_mult(globals)
        && let Some(owner) = world.substrate.entities.get(source_id)
    {
        let firepower = current_firepower_multiplier(world, rules, owner);
        damage = crate::sim::attach_effect::scale_i32(damage, firepower);
    }

    match crit
        .warhead()
        .map(|name| (name, crit.warhead_full_detonation()))
    {
        Some((name, true)) => {
            let Some(crit_warhead) = rules.warhead(name) else {
                return true;
            };
            let warhead_ref = world.interner.intern(name);
            let impact = ProjectileCoord::new(coord.x, coord.y, coord.z);
            let detonation = ProjectileDetonation {
                projectile_id: target_id,
                source_id,
                target: ProjectileTarget::Entity(target_id),
                impact,
                payload: ProjectilePayload::new(damage, warhead_ref, None, crit_warhead.bright),
                reason: ProjectileDetonationReason::CritWarhead,
            };
            let commit = commit_projectiles(world, run, &[detonation], rules, overlay_registry);
            effects.append(commit.effects);
            effects
                .under_attack_events
                .extend(commit.under_attack_events);
        }
        Some((name, false)) => {
            let Some(crit_warhead) = rules.warhead(name) else {
                return true;
            };
            // `DamageAreaWithTarget(coords, damage, owner, Crit.Warhead, true,
            // house, target)`: `MapClass::DamageArea` with the crit warhead and
            // affectsTiberium set; the target is the nested pass's
            // `DamageAreaTarget` when the crit warhead does not spread.
            let crit_ref = world.interner.intern(name);
            let impact = ProjectileCoord::new(coord.x, coord.y, coord.z);
            let (nested, mut pings, _crit_active, _area_result) =
                apply_area_damage_in_run_with_target(
                    world,
                    run,
                    rules,
                    overlay_registry,
                    impact,
                    damage,
                    crit_warhead,
                    true,
                    (source_id, source_house, crit_ref),
                    Some(target_id),
                );
            effects.append(nested);
            effects.under_attack_events.append(&mut pings);
        }
        None => {
            // `pTarget->ReceiveDamage(&damage, 0, warhead, owner, false,
            // false, house)` with the ORIGINAL warhead's id.
            let event =
                EntityDamageEvent::area(target_id, damage, 0, source_id, source_house, warhead_ref);
            let (nested, mut pings) =
                commit_entities(world, run, &[event], None, rules, overlay_registry);
            effects.append(nested);
            effects.under_attack_events.append(&mut pings);
        }
    }
    true
}

/// `TechnoExt::GetCurrentFirepowerMultiplier`: `Techno+0x158`-adjacent
/// `FirepowerMultiplier`, the house bias, the FIREPOWER rank stage and an
/// occupiable building's `OccupyDamageMultiplier=`.
#[cfg(feature = "phobos")]
fn current_firepower_multiplier(
    world: &Simulation,
    rules: &RuleSet,
    entity: &crate::sim::game_entity::GameEntity,
) -> f64 {
    use crate::rules::object_type::Ability;
    use crate::util::native_x87::NativeF64Bits;
    let f64_bits = |bits: NativeF64Bits| f64::from_bits(bits.bits());
    let mut multiplier = f64_bits(entity.crate_multipliers.firepower());
    multiplier *= world
        .houses
        .get(&entity.owner())
        .map_or(1.0, |house| f64_bits(house.firepower_bias()));
    if let Some(object) = world.interner.object_type(entity.type_ref(), rules) {
        if has_weapon_ability(entity.veterancy.rank_kind(), object, Ability::Firepower) {
            multiplier *= rules.general.veteran_combat;
        }
        if entity.category() == EntityCategory::Structure
            && object.can_be_occupied
            && object.can_occupy_fire
            && object.max_number_occupants > 0
        {
            multiplier *= f64::from(rules.garrison_rules.occupy_damage_multiplier);
        }
    }
    multiplier
}

/// `EnumFunctions::IsCellEligible`: the AllCells pair follows the target
/// cell's water flag; any other flag passes as long as it is not empty. A
/// fixture without terrain reads as land. `techno_content` is whether
/// `CellClass::GetContent` is a Techno when the caller passes
/// `explicitEmptyCells = true`, `None` when it does not; `bridge_land` is
/// `considerBridgesLand && ContainsBridge()`, which reads a bridged water cell
/// as land.
#[cfg(feature = "phobos")]
pub(crate) fn cell_eligible(
    land: Option<LandType>,
    allowed: AffectedTarget,
    techno_content: Option<bool>,
    bridge_land: bool,
) -> bool {
    if allowed == AffectedTarget::ALL {
        return true;
    }
    if techno_content == Some(false) && !allowed.contains(AffectedTarget::NO_CONTENT) {
        return false;
    }
    if allowed.contains(AffectedTarget::ALL_CELLS) {
        if land == Some(LandType::Water) && !bridge_land {
            allowed.contains(AffectedTarget::WATER)
        } else {
            allowed.contains(AffectedTarget::LAND)
        }
    } else {
        allowed != AffectedTarget::NONE
    }
}

/// `EnumFunctions::IsTechnoEligible` with `considerAircraftSeparately =
/// false`: the content flags split by class, aircraft under the Unit bit and
/// buildings under the Building bit.
#[cfg(feature = "phobos")]
pub(crate) fn techno_eligible(category: EntityCategory, allowed: AffectedTarget) -> bool {
    if allowed == AffectedTarget::ALL {
        return true;
    }
    if allowed.contains(AffectedTarget::ALL_CONTENTS) {
        return match category {
            EntityCategory::Infantry => allowed.contains(AffectedTarget::INFANTRY),
            EntityCategory::Unit | EntityCategory::Aircraft => {
                allowed.contains(AffectedTarget::UNIT)
            }
            EntityCategory::Structure => allowed.contains(AffectedTarget::BUILDING),
        };
    }
    allowed != AffectedTarget::NONE
}

/// `AnimExt::CreateRandomAnim` (`Ext/Anim/Body.cpp:407..432`): one random
/// entry constructed at the coordinates, owned by the firing house.
#[cfg(feature = "phobos")]
fn spawn_anim_at_world_coord(
    world: &mut Simulation,
    rules: &RuleSet,
    name: &str,
    coord: ProjectileCoord,
    owner_house: Option<InternedId>,
) {
    let world_coord = crate::sim::anim_class::AnimWorldCoord {
        x: coord.x,
        y: coord.y,
        z: coord.z,
    };
    spawn_anim_at_coord(world, rules, name, world_coord, owner_house);
}

#[cfg(feature = "phobos")]
fn spawn_anim_at_coord(
    world: &mut Simulation,
    rules: &RuleSet,
    name: &str,
    coord: crate::sim::anim_class::AnimWorldCoord,
    owner_house: Option<InternedId>,
) {
    let (rx, ry, sub_x, sub_y, z) = coord.to_cell_sub_z();
    let type_name = world.interner.intern(name);
    let descriptor =
        crate::sim::components::AnimClassSpawnDescriptor::new(type_name, rx, ry, sub_x, sub_y, z);
    match world.spawn_anim_at_world(rules, descriptor, coord) {
        Ok(anim) => {
            if let Some(house) = owner_house {
                world.substrate.set_anim_owner_house(anim, house);
            }
        }
        Err(error) => log::debug!("crit animation [{name}] did not construct: {error}"),
    }
}

/// The targets `DetonateOnOneUnit` visits for this detonation: every collected
/// entity receiver in order for a cell-spread warhead, else the intended target
/// within 64 leptons of the impact (Ares' quarter-cell tolerance).
#[cfg(feature = "phobos")]
fn detonation_targets(
    world: &Simulation,
    warhead: &WarheadType,
    impact: ProjectileCoord,
    intended_target: Option<u64>,
    receivers: &[AreaDamageReceiver],
) -> Vec<u64> {
    if warhead.cell_spread_f64 > 0.0 {
        return receivers
            .iter()
            .filter_map(|receiver| match receiver {
                AreaDamageReceiver::Entity(event) => Some(event.target_id),
                AreaDamageReceiver::Terrain(_) => None,
            })
            .collect();
    }
    intended_target
        .filter(|&id| {
            world.substrate.entities.get(id).is_some_and(|target| {
                let coords = crate::sim::movement::ground_pose::object_get_coords(
                    target,
                    world.resolved_terrain.as_ref(),
                );
                let dx = i64::from(impact.x) - i64::from(coords.x);
                let dy = i64::from(impact.y) - i64::from(coords.y);
                let dz = i64::from(impact.z) - i64::from(coords.z);
                dx * dx + dy * dy + dz * dz <= 4096
            })
        })
        .into_iter()
        .collect()
}
