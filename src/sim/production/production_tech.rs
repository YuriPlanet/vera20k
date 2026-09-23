//! Tech tree, build options, factory matching, and spawn cell logic.
//!
//! Determines what a player can build based on owned structures, prerequisites,
//! faction ownership, and available factories. Also handles spawn cell selection
//! for newly produced units.

use crate::map::entities::EntityCategory;
use crate::rules::object_type::{BuildCategory, FactoryType, ObjectCategory};
use crate::rules::ruleset::RuleSet;
use crate::sim::entity_store::EntityStore;
use crate::sim::world::Simulation;

use super::production_queue::credits_for_owner;
use super::production_types::*;

/// Maximum tech level available in standard skirmish/multiplayer.
/// Units with TechLevel > this are not buildable.
const MATCH_TECH_LEVEL: i32 = 10;

pub(super) fn build_option_for_owner(
    sim: &Simulation,
    rules: &RuleSet,
    owner: &str,
    type_id: &str,
    mode: BuildMode,
) -> Option<BuildOption> {
    let obj = rules.object(type_id)?;
    let queue_category = production_category_for_object(obj);

    let mut reason: Option<BuildDisabledReason> = None;
    if obj.tech_level < 0 || obj.tech_level > MATCH_TECH_LEVEL {
        reason = Some(BuildDisabledReason::UnbuildableTechLevel);
    } else if mode == BuildMode::Strict
        && !obj.owner.is_empty()
        && !owner_matches_any_build_identity(sim, owner, &obj.owner)
    {
        reason = Some(BuildDisabledReason::WrongOwner);
    } else if mode == BuildMode::Strict
        && !obj.required_houses.is_empty()
        && !owner_matches_any_build_identity(sim, owner, &obj.required_houses)
    {
        reason = Some(BuildDisabledReason::WrongHouse);
    } else if mode == BuildMode::Strict
        && !obj.forbidden_houses.is_empty()
        && owner_matches_any_build_identity(sim, owner, &obj.forbidden_houses)
    {
        reason = Some(BuildDisabledReason::ForbiddenHouse);
    } else if mode == BuildMode::Strict
        && (obj.requires_stolen_allied_tech
            || obj.requires_stolen_soviet_tech
            || obj.requires_stolen_third_tech)
    {
        // Spy infiltration not yet implemented — always block stolen-tech units.
        reason = Some(BuildDisabledReason::RequiresStolenTech);
    } else if mode == BuildMode::Strict {
        // PrerequisiteOverride: if owner has ANY override building, skip normal prereqs.
        let override_satisfied = !obj.prerequisite_override.is_empty()
            && has_any_override_building(sim, owner, &obj.prerequisite_override);
        if !override_satisfied {
            if let Some(missing) = first_missing_prereq(sim, rules, owner, &obj.prerequisite) {
                reason = Some(BuildDisabledReason::MissingPrerequisite(missing));
            }
        }
    }
    if reason.is_none()
        && mode == BuildMode::Strict
        && !has_factory_for_owner(
            &sim.substrate.entities,
            rules,
            owner,
            queue_category,
            &sim.interner,
        )
    {
        reason = Some(BuildDisabledReason::NoFactory);
    }
    // BuildLimit check: count owned entities + queued + ready-for-placement.
    if reason.is_none() && mode == BuildMode::Strict {
        if let Some(limit) = effective_build_limit(obj.build_limit) {
            if count_owned_and_queued(sim, owner, &obj.id) >= limit {
                reason = Some(BuildDisabledReason::AtBuildLimit);
            }
        }
    }
    if reason.is_none() && (obj.cost <= 0 || credits_for_owner(sim, owner) < obj.cost) {
        reason = Some(BuildDisabledReason::InsufficientCredits);
    }
    let type_interned = sim.interner.get(type_id).unwrap_or_default();
    Some(BuildOption {
        type_id: type_interned,
        display_name: obj.name.clone().unwrap_or_else(|| obj.id.clone()),
        cost: obj.cost,
        object_category: obj.category,
        queue_category,
        enabled: reason.is_none(),
        reason,
    })
}

/// P6 revalidation classifier: re-check an active/queued build's eligibility AFTER enqueue,
/// so a build whose prerequisites / producing factory were lost is disposed of. Reproduces
/// gamemd's `FindFactoryBuilding(1,0,1)` gate (the embedded `HouseClass::CanBuild` scan across
/// the owner's candidate factory buildings): a tech-tree / owner / factory-presence failure ->
/// `PermanentlyBlocked` (abandon); a pure credit stall (`on_hold`, handled per-step) or a
/// build-limit "busy" is NOT a mid-build abandon -> `Buildable` (keep charging).
///
/// `TemporarilyBlocked` (gamemd's `(1,0,1)` passes but `(1,1,1)` fails = a factory building
/// exists but is UNPOWERED via an EMP/spy/trigger GoOffline event) is intentionally
/// UNREACHABLE here: the Rust power model is per-house low-power, which gamemd applies as a
/// RATE penalty (already modeled in `prepare_step_inputs`), NOT a suspend — gamemd SLOWS,
/// it does not halt, production on a grid deficit. Per-building powered-down (EMP) is not yet
/// modeled, so this arm is a forward seam for the EMP slice.
pub(in crate::sim) fn revalidate_eligibility(
    sim: &Simulation,
    rules: &RuleSet,
    owner: &str,
    type_id: &str,
) -> super::factory::BuildEligibility {
    use super::factory::BuildEligibility;
    match build_option_for_owner(sim, rules, owner, type_id, BuildMode::Strict) {
        None => BuildEligibility::PermanentlyBlocked,
        Some(opt) if opt.enabled => BuildEligibility::Buildable,
        Some(opt) => match opt.reason {
            // Credit stall + build-limit "busy" are not abandons in gamemd — keep building.
            Some(BuildDisabledReason::InsufficientCredits)
            | Some(BuildDisabledReason::AtBuildLimit)
            | None => BuildEligibility::Buildable,
            // Tech-tree / owner / factory loss -> CanBuild fails -> abandon.
            Some(_) => BuildEligibility::PermanentlyBlocked,
        },
    }
}

pub(super) fn build_options_for_owner_mode(
    sim: &Simulation,
    rules: &RuleSet,
    owner: &str,
    mode: BuildMode,
) -> Vec<BuildOption> {
    let mut out: Vec<BuildOption> = Vec::new();
    for id in &rules.building_ids {
        if let Some(opt) = build_option_for_owner(sim, rules, owner, id, mode) {
            out.push(opt);
        }
    }
    for id in &rules.infantry_ids {
        if let Some(opt) = build_option_for_owner(sim, rules, owner, id, mode) {
            out.push(opt);
        }
    }
    for id in &rules.vehicle_ids {
        if let Some(opt) = build_option_for_owner(sim, rules, owner, id, mode) {
            out.push(opt);
        }
    }
    for id in &rules.aircraft_ids {
        if let Some(opt) = build_option_for_owner(sim, rules, owner, id, mode) {
            out.push(opt);
        }
    }
    out.sort_by_key(|opt| opt.queue_category);
    out
}

pub(super) fn should_use_relaxed_build_mode(
    sim: &Simulation,
    rules: &RuleSet,
    owner: &str,
) -> bool {
    if !prototype_fallback_enabled() {
        return false;
    }
    let strict = build_options_for_owner_mode(sim, rules, owner, BuildMode::Strict);
    !strict.iter().any(|o| o.enabled)
}

/// Prototype build fallback — disabled by default.
/// If needed in the future, move to GameOptions (synced across multiplayer peers).
const PROTOTYPE_BUILD_FALLBACK: bool = false;

pub(super) fn prototype_fallback_enabled() -> bool {
    PROTOTYPE_BUILD_FALLBACK
}

pub(super) fn owner_matches_any_build_identity(
    sim: &Simulation,
    owner: &str,
    candidates: &[String],
) -> bool {
    candidates
        .iter()
        .any(|candidate| owner_matches_build_identity(sim, owner, candidate))
}

pub(super) fn owner_matches_build_identity(sim: &Simulation, owner: &str, candidate: &str) -> bool {
    if candidate.eq_ignore_ascii_case(owner) {
        return true;
    }
    sim.interner
        .get(owner)
        .and_then(|owner_id| sim.houses.get(&owner_id))
        .and_then(|house| house.country)
        .is_some_and(|country| candidate.eq_ignore_ascii_case(sim.interner.resolve(country)))
}

/// Check if the owner has ANY completed structure from the PrerequisiteOverride list.
fn has_any_override_building(sim: &Simulation, owner: &str, overrides: &[String]) -> bool {
    sim.substrate.entities.values().any(|e| {
        !e.dying
            && !e.lifecycle.in_limbo
            && sim.interner.resolve(e.owner()).eq_ignore_ascii_case(owner)
            && e.category == EntityCategory::Structure
            && e.building_up.is_none()
            && overrides
                .iter()
                .any(|ov| ov.eq_ignore_ascii_case(sim.interner.resolve(e.type_ref())))
    })
}

/// Interpret BuildLimit value. Returns None if no limit applies (0 = unlimited).
fn effective_build_limit(build_limit: i32) -> Option<u32> {
    if build_limit == 0 {
        return None;
    }
    Some(build_limit.unsigned_abs())
}

/// Count owned entities + queued items + ready-for-placement of this type for an owner.
fn count_owned_and_queued(sim: &Simulation, owner: &str, type_id: &str) -> u32 {
    let owner_id = sim.interner.get(owner);
    let type_interned = sim.interner.get(type_id);

    let owned = match (owner_id, type_interned) {
        (Some(oid), Some(tid)) => sim
            .substrate
            .entities
            .values()
            .filter(|e| !e.dying && e.owner() == oid && e.type_ref() == tid)
            .count() as u32,
        _ => 0,
    };

    // P5d: count from the registry queue-of-record — the active build (head) + the FIFO
    // tail, across the owner's factories (was the per-`BuildQueueItem` `queues_by_owner` scan).
    let queued = match (owner_id, type_interned) {
        (Some(oid), Some(tid)) => sim
            .production
            .factory_shadow
            .iter_insertion_ordered()
            .iter()
            .filter(|f| f.owner == oid)
            .map(|f| {
                // StartProduction materializes the active object into EntityStore,
                // so `owned` above already counts it. Only a restored/malformed
                // active head missing its swizzled identity needs the registry
                // fallback; queued tails remain unconstructed and count here.
                let active = f
                    .object
                    .as_ref()
                    .map_or(0, |o| u32::from(o.type_id == tid && o.entity_id.is_none()));
                let tail = f.queue.iter().filter(|e| e.type_id == tid).count() as u32;
                active + tail
            })
            .sum(),
        _ => 0,
    };

    let ready = owner_id
        .and_then(|oid| sim.production.ready_by_owner.get(&oid))
        .map(|ready| {
            ready
                .iter()
                .filter(|&&tid| type_interned.map_or(false, |expected| tid == expected))
                .count() as u32
        })
        .unwrap_or(0);

    let materialized_ready = match (owner_id, type_interned) {
        (Some(oid), Some(tid)) => {
            sim.production
                .factory_shadow
                .iter_insertion_ordered()
                .iter()
                .filter(|factory| {
                    factory.owner == oid
                        && factory.progress >= super::factory::PRODUCTION_STEPS
                        && factory.object.as_ref().is_some_and(|object| {
                            object.type_id == tid && object.entity_id.is_some()
                        })
                })
                .count() as u32
        }
        _ => 0,
    };

    owned + queued + ready.saturating_sub(materialized_ready)
}

fn first_missing_prereq(
    sim: &Simulation,
    rules: &RuleSet,
    owner: &str,
    prereqs: &[String],
) -> Option<String> {
    for p in prereqs {
        if p.is_empty() {
            continue;
        }
        // Only structures satisfy prerequisites — units/infantry/aircraft don't count.
        let ok = sim.substrate.entities.values().any(|e| {
            !e.dying
                && !e.lifecycle.in_limbo
                && sim.interner.resolve(e.owner()).eq_ignore_ascii_case(owner)
                && e.category == EntityCategory::Structure
                && e.building_up.is_none()
                && structure_satisfies_prerequisite(rules, sim.interner.resolve(e.type_ref()), p)
        });
        if !ok {
            return Some(p.clone());
        }
    }
    None
}

pub(super) fn production_category_for_object(
    obj: &crate::rules::object_type::ObjectType,
) -> ProductionCategory {
    match obj.category {
        ObjectCategory::Infantry => ProductionCategory::Infantry,
        ObjectCategory::Vehicle if obj.naval => ProductionCategory::Ship,
        ObjectCategory::Vehicle => ProductionCategory::Vehicle,
        ObjectCategory::Aircraft => ProductionCategory::Aircraft,
        ObjectCategory::Building => match obj.build_cat {
            Some(BuildCategory::Combat) => ProductionCategory::Defense,
            _ => ProductionCategory::Building,
        },
    }
}

pub(super) fn supports_live_production(obj: &crate::rules::object_type::ObjectType) -> bool {
    matches!(
        production_category_for_object(obj),
        ProductionCategory::Building
            | ProductionCategory::Defense
            | ProductionCategory::Infantry
            | ProductionCategory::Vehicle
            | ProductionCategory::Aircraft
            | ProductionCategory::Ship
    )
}

pub(super) fn has_factory_for_owner(
    entities: &EntityStore,
    rules: &RuleSet,
    owner: &str,
    category: ProductionCategory,
    interner: &crate::sim::intern::StringInterner,
) -> bool {
    entities.values().any(|e| {
        !e.dying
            && !e.lifecycle.in_limbo
            && interner.resolve(e.owner()).eq_ignore_ascii_case(owner)
            && e.category == EntityCategory::Structure
            && e.building_up.is_none()
            && is_production_factory(rules, interner.resolve(e.type_ref()), category)
    })
}

/// Check if a structure is a production factory for the given category.
///
/// Uses the data-driven Factory= key from rules.ini via RuleSet.factory_map.
/// A building with `Factory=InfantryType` produces infantry, `Factory=UnitType`
/// produces vehicles, etc. Buildings without Factory= are never factories.
pub(super) fn is_production_factory(
    rules: &RuleSet,
    structure_id: &str,
    category: ProductionCategory,
) -> bool {
    let Some(factory_type) = rules.factory_type(structure_id) else {
        return false;
    };
    match category {
        ProductionCategory::Infantry => factory_type == FactoryType::InfantryType,
        ProductionCategory::Vehicle => {
            factory_type == FactoryType::UnitType
                && rules.object(structure_id).is_some_and(|object| !object.naval)
        }
        ProductionCategory::Ship => {
            factory_type == FactoryType::UnitType
                && rules.object(structure_id).is_some_and(|object| object.naval)
        }
        ProductionCategory::Aircraft => factory_type == FactoryType::AircraftType,
        ProductionCategory::Building | ProductionCategory::Defense => {
            factory_type == FactoryType::BuildingType
        }
    }
}

const RA2_QUEUE_FRAME_MS: u64 = 66;

/// Lower floor on production speed, matching the original `if speed == 0.0 { 0.01 }`
/// guard in the f64 chain — expressed in PPM (10_000 = 0.01×).
const MIN_PRODUCTION_SPEED_PPM: u64 = PRODUCTION_RATE_SCALE / 100;

pub(in crate::sim) fn build_time_base_frames(
    rules: &RuleSet,
    obj: &crate::rules::object_type::ObjectType,
) -> u32 {
    if obj.cost <= 0 {
        return 0;
    }
    // Integer-only build-time calculation (replaces f64 chain for determinism):
    //   base = trunc(cost * BuildSpeed * 0.9)
    //   frames = trunc(base * BuildTimeMultiplier)
    //
    // Using x1000 pre-scaled integers: cost * speed_x1000 * 9 / 10000
    // then frames = base * btm_x1000 / 1000
    //
    // Use i64 to prevent overflow (cost up to ~50000, speed_x1000 up to ~500).
    let cost = obj.cost.max(0) as i64;
    let speed_x1000 = rules.production.build_speed_x1000.max(1) as i64;
    let base_value = (cost * speed_x1000 * 9 / 10000) as i32;
    let btm_x1000 = obj.build_time_multiplier_x1000.max(1) as i64;
    let raw_frames = (base_value as i64 * btm_x1000 / 1000).max(0) as i32;
    raw_frames as u32
}

// Legacy frames-rate family — retired from the production path at the authority flip
// (completion is now driven by the registry per-step charge). Kept test-only as the
// pinned spec of the per-owner/category rate differentiation.
#[cfg(test)]
pub(in crate::sim) fn effective_progress_rate_ppm_for_type(
    sim: &Simulation,
    rules: &RuleSet,
    owner: &str,
    type_id: &str,
) -> u64 {
    let Some(obj) = rules.object(type_id) else {
        return PRODUCTION_RATE_SCALE;
    };
    effective_progress_rate_ppm_for_category(
        sim,
        rules,
        owner,
        production_category_for_object(obj),
    )
}

#[cfg(test)]
pub(super) fn effective_progress_rate_ppm_for_category(
    sim: &Simulation,
    rules: &RuleSet,
    owner: &str,
    category: ProductionCategory,
) -> u64 {
    // power_speed and queue_time are both scaled by PRODUCTION_RATE_SCALE (1M).
    // effective_rate = power_speed / queue_time, also at 1M scale.
    let power_speed: u64 = owner_effective_production_speed_ppm(sim, rules, owner);
    let queue_time: u64 = matching_factory_time_multiplier_ppm(
        &sim.substrate.entities,
        rules,
        owner,
        category,
        &sim.interner,
    );
    // (power_speed / queue_time) * PRODUCTION_RATE_SCALE
    // = power_speed * PRODUCTION_RATE_SCALE / queue_time
    let rate: u64 = (u128::from(power_speed) * u128::from(PRODUCTION_RATE_SCALE)
        / u128::from(queue_time.max(1))) as u64;
    rate.max(1)
}

pub(super) fn estimated_real_time_ms(base_frames: u32, rate_ppm: u64) -> u32 {
    if base_frames == 0 {
        return 0;
    }
    let denom = u128::from(rate_ppm.max(1));
    let numer = u128::from(base_frames)
        * u128::from(RA2_QUEUE_FRAME_MS)
        * u128::from(PRODUCTION_RATE_SCALE);
    let rounded_up = numer.div_ceil(denom);
    rounded_up.min(u128::from(u32::MAX)) as u32
}

pub(in crate::sim) fn effective_time_to_build_frames_for_type(
    sim: &Simulation,
    rules: &RuleSet,
    owner: &str,
    type_id: &str,
    base_frames: u32,
) -> u32 {
    let Some(obj) = rules.object(type_id) else {
        return base_frames;
    };
    effective_time_to_build_frames_for_object(sim, rules, owner, obj, base_frames)
}

fn effective_time_to_build_frames_for_object(
    sim: &Simulation,
    rules: &RuleSet,
    owner: &str,
    obj: &crate::rules::object_type::ObjectType,
    base_frames: u32,
) -> u32 {
    // time = base_frames / speed = base_frames * PRODUCTION_RATE_SCALE / speed_ppm.
    let speed_ppm =
        owner_effective_production_speed_ppm(sim, rules, owner).max(MIN_PRODUCTION_SPEED_PPM);
    let mut time_to_build: u64 = ((u128::from(base_frames) * u128::from(PRODUCTION_RATE_SCALE))
        / u128::from(speed_ppm)) as u64;
    time_to_build = apply_multiple_factory_scaling_ppm(
        time_to_build,
        rules.production.multiple_factory_ppm,
        matching_factory_count_for_owner(
            &sim.substrate.entities,
            rules,
            owner,
            production_category_for_object(obj),
            &sim.interner,
        ),
    );
    if obj.category == ObjectCategory::Building && obj.wall {
        // Wall coefficient is f32 in rules and only consumed by this UI-display
        // helper. The conversion `(f32 as f64 * 1e6) as u64` is bit-exact on
        // every IEEE-754 platform; sim-hashed state never reads this branch.
        let coef_ppm = (rules.production.wall_build_speed_coefficient.max(0.0) as f64
            * PRODUCTION_RATE_SCALE as f64) as u64;
        time_to_build = (u128::from(time_to_build) * u128::from(coef_ppm)
            / u128::from(PRODUCTION_RATE_SCALE)) as u64;
    }
    time_to_build.min(u32::MAX as u64) as u32
}

/// Integer port of the original `MultipleFactory` exponential damp. Each extra
/// matching factory multiplies the time-to-build by `multiple_factory_ppm /
/// PRODUCTION_RATE_SCALE`, so for `n` factories the time is scaled by
/// `mf^(n-1)`. Computed via repeated PPM-domain multiply+divide so the result
/// is bit-exact across machines.
fn apply_multiple_factory_scaling_ppm(
    time_to_build: u64,
    multiple_factory_ppm: u64,
    queue_factory_count: u32,
) -> u64 {
    if multiple_factory_ppm == 0 || queue_factory_count <= 1 {
        return time_to_build;
    }
    let mut scaled = time_to_build;
    for _ in 1..queue_factory_count {
        scaled = (u128::from(scaled) * u128::from(multiple_factory_ppm)
            / u128::from(PRODUCTION_RATE_SCALE)) as u64;
    }
    scaled
}

/// Effective production speed as PPM (PRODUCTION_RATE_SCALE = 1.0×). Integer
/// port of gamemd's LowPowerPenaltyModifier formula:
///
/// ```text
///   speed = 1 - (1 - power_pct) * low_power_penalty_modifier
///   speed = max(speed, min_low_power_production_speed)
///   if power_pct < 1.0: speed = min(speed, max_low_power_production_speed)
///   if speed == 0: speed = 0.01
/// ```
///
/// All inputs are PPM-scaled at INI parse time (`*_ppm` fields on
/// `ProductionRules`), so the entire chain is deterministic.
fn owner_effective_production_speed_ppm(sim: &Simulation, rules: &RuleSet, owner: &str) -> u64 {
    let power_pct_ppm = owner_power_percentage_ppm(sim, owner);
    let deficit_ppm = PRODUCTION_RATE_SCALE.saturating_sub(power_pct_ppm);
    let penalty_ppm = ((u128::from(deficit_ppm)
        * u128::from(rules.production.low_power_penalty_modifier_ppm))
        / u128::from(PRODUCTION_RATE_SCALE)) as u64;
    let mut speed_ppm = PRODUCTION_RATE_SCALE.saturating_sub(penalty_ppm);
    speed_ppm = speed_ppm.max(rules.production.min_low_power_production_speed_ppm);
    if power_pct_ppm < PRODUCTION_RATE_SCALE {
        speed_ppm = speed_ppm.min(rules.production.max_low_power_production_speed_ppm);
    }
    if speed_ppm == 0 {
        MIN_PRODUCTION_SPEED_PPM
    } else {
        speed_ppm
    }
}

/// Owner power ratio as PPM, clamped to `[0, PRODUCTION_RATE_SCALE]`. Returns
/// `PRODUCTION_RATE_SCALE` (1.0×) when no power is drained (matches the
/// original `if drained <= 0 { 1.0 }` short-circuit).
pub(in crate::sim::production) fn owner_power_percentage_ppm(sim: &Simulation, owner: &str) -> u64 {
    let (produced, drained) = sim
        .interner
        .get(owner)
        .and_then(|id| sim.power_states.get(&id))
        .map(|state| (state.total_output, state.total_drain))
        .unwrap_or((0, 0));
    if drained <= 0 {
        return PRODUCTION_RATE_SCALE;
    }
    let produced_u = u128::from(produced.max(0) as u64);
    let drained_u = u128::from(drained as u64);
    let ratio_ppm = (produced_u * u128::from(PRODUCTION_RATE_SCALE)) / drained_u;
    ratio_ppm.min(u128::from(PRODUCTION_RATE_SCALE)) as u64
}

/// Factory time multiplier scaled by PRODUCTION_RATE_SCALE (1M = 1.0×).
/// MultipleFactory^(n-1) computed via repeated integer multiply.
/// Test-only: sole caller is the retired `effective_progress_rate_ppm_for_category`.
#[cfg(test)]
fn matching_factory_time_multiplier_ppm(
    entities: &EntityStore,
    rules: &RuleSet,
    owner: &str,
    category: ProductionCategory,
    interner: &crate::sim::intern::StringInterner,
) -> u64 {
    let factory_count: u32 =
        matching_factory_count_for_owner(entities, rules, owner, category, interner);
    if factory_count <= 1 {
        return PRODUCTION_RATE_SCALE; // 1.0×
    }
    // Use pre-computed PPM value from rules (converted at INI parse time).
    let mf_ppm: u64 = rules.production.multiple_factory_ppm;
    // Compute mf_ppm^(n-1) / PRODUCTION_RATE_SCALE^(n-2) via repeated multiply+divide.
    let mut result: u64 = mf_ppm;
    for _ in 1..(factory_count - 1) {
        result = result * mf_ppm / PRODUCTION_RATE_SCALE;
    }
    result.max(1)
}

pub(in crate::sim::production) fn matching_factory_count_for_owner(
    entities: &EntityStore,
    rules: &RuleSet,
    owner: &str,
    category: ProductionCategory,
    interner: &crate::sim::intern::StringInterner,
) -> u32 {
    entities
        .values()
        .filter(|e| {
            !e.dying
                && !e.lifecycle.in_limbo
                && interner.resolve(e.owner()).eq_ignore_ascii_case(owner)
                && e.category == EntityCategory::Structure
                && e.building_up.is_none()
                && is_production_factory(rules, interner.resolve(e.type_ref()), category)
        })
        .count() as u32
}

pub fn producer_candidates_for_owner_category(
    entities: &EntityStore,
    rules: &RuleSet,
    owner: &str,
    category: ProductionCategory,
    require_matching_factory: bool,
    interner: &crate::sim::intern::StringInterner,
) -> Vec<(u64, u16, u16, String)> {
    let mut preferred_factories: Vec<(u64, u16, u16, String)> = Vec::new();
    for e in entities.values() {
        // A Dying factory corpse must not be selected as a unit's producer/exit.
        if e.dying {
            continue;
        }
        if e.lifecycle.in_limbo {
            continue;
        }
        if !interner.resolve(e.owner()).eq_ignore_ascii_case(owner) {
            continue;
        }
        if e.category != EntityCategory::Structure {
            continue;
        }
        if e.building_up.is_some() {
            continue;
        }
        let type_ref_str = interner.resolve(e.type_ref());
        let is_match = is_production_factory(rules, type_ref_str, category);
        if require_matching_factory && !is_match {
            continue;
        }
        if !require_matching_factory || is_match {
            preferred_factories.push((
                e.stable_id(),
                e.position.rx,
                e.position.ry,
                type_ref_str.to_string(),
            ));
        }
    }
    preferred_factories.sort_by(|a, b| a.0.cmp(&b.0));
    preferred_factories
}

pub fn is_matching_factory(
    rules: &RuleSet,
    structure_id: &str,
    produced_category: ObjectCategory,
) -> bool {
    match produced_category {
        ObjectCategory::Infantry => {
            is_production_factory(rules, structure_id, ProductionCategory::Infantry)
        }
        ObjectCategory::Vehicle => {
            is_production_factory(rules, structure_id, ProductionCategory::Vehicle)
                || is_production_factory(rules, structure_id, ProductionCategory::Ship)
        }
        ObjectCategory::Aircraft => {
            is_production_factory(rules, structure_id, ProductionCategory::Aircraft)
        }
        ObjectCategory::Building => {
            is_production_factory(rules, structure_id, ProductionCategory::Building)
        }
    }
}

pub fn structure_satisfies_prerequisite(rules: &RuleSet, structure_id: &str, prereq: &str) -> bool {
    // Direct match: the structure ID is exactly the prerequisite.
    if structure_id.eq_ignore_ascii_case(prereq) {
        return true;
    }
    // Alias match: look up the prerequisite in [General] PrerequisiteXxx groups.
    // e.g. prereq="POWER" → check if structure_id is in PrerequisitePower list.
    if let Some(group) = rules.prerequisite_group(prereq) {
        let sid_upper: String = structure_id.to_ascii_uppercase();
        return group.iter().any(|id| *id == sid_upper);
    }
    false
}

pub fn foundation_dimensions(foundation: &str) -> (u16, u16) {
    crate::rules::foundation::foundation_dimensions(foundation)
}

/// Returns the base foundation cells for normal building occupancy.
///
/// gamemd keeps these cells separate from `AddOccupy`/`RemoveOccupy`, which only
/// adjust hidden occupancy counters behind `CanHideThings`.
pub fn building_base_foundation_cells(
    origin_rx: u16,
    origin_ry: u16,
    foundation: &str,
) -> Vec<(u16, u16)> {
    use std::collections::BTreeSet;
    let (w, h) = foundation_dimensions(foundation);
    let mut cells: BTreeSet<(u16, u16)> = BTreeSet::new();

    for dx in 0..w {
        for dy in 0..h {
            let rx = origin_rx as i32 + dx as i32;
            let ry = origin_ry as i32 + dy as i32;
            if rx >= 0 && rx <= u16::MAX as i32 && ry >= 0 && ry <= u16::MAX as i32 {
                cells.insert((rx as u16, ry as u16));
            }
        }
    }

    cells.into_iter().collect()
}

/// Compatibility alias for older callers. Add/Remove modifiers are deliberately
/// ignored: the hidden mechanism is a counted lifecycle state, not a footprint.
pub fn building_footprint_cells(
    origin_rx: u16,
    origin_ry: u16,
    foundation: &str,
    _add_occupy: &[(i16, i16)],
    _remove_occupy: &[(i16, i16)],
) -> Vec<(u16, u16)> {
    building_base_foundation_cells(origin_rx, origin_ry, foundation)
}

/// Cells that block static grid movement, given the base foundation and whether
/// the building has a bib (`Bib=yes` in rules.ini).
///
/// For non-bib buildings, this is just the base foundation. For `Bib=yes`
/// buildings, the east-edge column of the foundation is excluded: those cells
/// are unit-passable in the original engine via the per-occupant-chain bib
/// relaxation in `Can_Enter_Cell` (probes the east neighbor; if it isn't part
/// of the same building, the building stops blocking the cell).
///
/// "East edge" = any cell in `base_foundation` whose east neighbor `(x+1, y)`
/// is outside the base foundation.
///
/// `NumberImpassableRows` is deliberately not applied here. That field is a
/// live `UnitClass::Can_Enter_Cell` object-list skip, not static terrain data.
pub fn building_movement_blocking_cells(
    base_foundation: &[(u16, u16)],
    has_bib: bool,
) -> Vec<(u16, u16)> {
    building_movement_blocking_cells_for_state(base_foundation, 0, has_bib, -1, false, true, false)
}

/// Movement-blocking cells for an actual building object-list occupant.
///
/// `NumberImpassableRows` is only active in gamemd's live helper after a
/// radio/contact or the UnitRepair/Bunker branch reaches that helper. Ordinary
/// buildings outside those branches block their full base foundation, subject
/// to the bib edge relaxation.
pub fn building_movement_blocking_cells_for_state(
    base_foundation: &[(u16, u16)],
    foundation_origin_rx: u16,
    has_bib: bool,
    number_impassable_rows: i32,
    is_bunker: bool,
    bunker_occupied: bool,
    number_rows_active: bool,
) -> Vec<(u16, u16)> {
    use std::collections::BTreeSet;
    let set: BTreeSet<(u16, u16)> = base_foundation.iter().copied().collect();
    let apply_number_rows = number_rows_active && (!is_bunker || !bunker_occupied);
    let impassable_row_limit = if apply_number_rows && number_impassable_rows >= 0 {
        Some(foundation_origin_rx.saturating_add(number_impassable_rows as u16))
    } else {
        None
    };
    base_foundation
        .iter()
        .copied()
        .filter(|&(x, _)| match impassable_row_limit {
            Some(limit_x) => x < limit_x,
            None => true,
        })
        .filter(|&(x, y)| {
            if !has_bib {
                return true;
            }
            let east = x.checked_add(1).map(|nx| (nx, y));
            east.is_some_and(|neighbor| set.contains(&neighbor))
        })
        .collect()
}

#[cfg(test)]
mod production_speed_ppm_tests {
    use super::*;
    use crate::rules::ini_parser::IniFile;
    use crate::rules::ruleset::RuleSet;
    use crate::sim::world::Simulation;

    /// f64 reference for the OLD chain. Used by tests to confirm the integer
    /// PPM port matches the original formula to within 1 PPM (the rounding
    /// budget of `f64.trunc() as u64` at the 1M scale).
    fn reference_speed_f64(
        produced: i32,
        drained: i32,
        lppm: f32,
        min_lp: f32,
        max_lp: f32,
    ) -> f64 {
        let power_pct: f64 = if drained <= 0 {
            1.0
        } else {
            ((produced.max(0) as f64) / (drained as f64)).clamp(0.0, 1.0)
        };
        let mut speed = 1.0 - (1.0 - power_pct) * lppm as f64;
        speed = speed.max(min_lp as f64);
        if power_pct < 1.0 {
            speed = speed.min(max_lp as f64);
        }
        if speed == 0.0 { 0.01 } else { speed }
    }

    fn rules_with_power(lppm: &str, min_lp: &str, max_lp: &str) -> RuleSet {
        let ini = IniFile::from_str(&format!(
            "[General]\nBuildSpeed=0.05\nLowPowerPenaltyModifier={lppm}\n\
             MinLowPowerProductionSpeed={min_lp}\nMaxLowPowerProductionSpeed={max_lp}\n\
             [InfantryTypes]\n[AircraftTypes]\n[BuildingTypes]\n[VehicleTypes]\n"
        ));
        RuleSet::from_ini(&ini).expect("should parse")
    }

    /// Direct unit test of the math: build a power state for the owner, run
    /// `owner_effective_production_speed_ppm`, compare to the f64 reference.
    fn assert_ppm_matches_f64(
        produced: i32,
        drained: i32,
        rules: &RuleSet,
        lppm_f: f32,
        min_lp_f: f32,
        max_lp_f: f32,
    ) {
        let mut sim = Simulation::new();
        let owner_id = sim.interner.intern("Americans");
        sim.power_states.insert(
            owner_id,
            crate::sim::power_system::PowerState {
                total_output: produced,
                total_drain: drained,
                ..Default::default()
            },
        );
        let got_ppm = owner_effective_production_speed_ppm(&sim, rules, "Americans");
        let want_f64 = reference_speed_f64(produced, drained, lppm_f, min_lp_f, max_lp_f);
        let want_ppm = (want_f64 * PRODUCTION_RATE_SCALE as f64).trunc() as u64;
        // Integer PPM port can disagree with f64 trunc by at most 1 PPM due
        // to rounding-vs-truncation order in the intermediate steps.
        let diff = got_ppm.abs_diff(want_ppm);
        assert!(
            diff <= 1,
            "produced={}, drained={}: got {} ppm, want {} ppm (f64 ref {})",
            produced,
            drained,
            got_ppm,
            want_ppm,
            want_f64
        );
    }

    #[test]
    fn full_power_returns_one() {
        let rules = rules_with_power("1.0", "0.5", "0.9");
        assert_ppm_matches_f64(100, 100, &rules, 1.0, 0.5, 0.9);
    }

    #[test]
    fn no_drain_returns_one() {
        let rules = rules_with_power("1.0", "0.5", "0.9");
        assert_ppm_matches_f64(50, 0, &rules, 1.0, 0.5, 0.9);
    }

    #[test]
    fn brownout_clamps_to_max_lp() {
        // 80% power, lppm=1.0, max_lp=0.9 → speed = 1 - 0.2 = 0.8, then min(0.8, 0.9) = 0.8.
        let rules = rules_with_power("1.0", "0.5", "0.9");
        assert_ppm_matches_f64(80, 100, &rules, 1.0, 0.5, 0.9);
    }

    #[test]
    fn deep_brownout_clamps_to_min_lp() {
        // 10% power, lppm=1.0 → speed = 1 - 0.9 = 0.1, then max(0.1, 0.5) = 0.5.
        let rules = rules_with_power("1.0", "0.5", "0.9");
        assert_ppm_matches_f64(10, 100, &rules, 1.0, 0.5, 0.9);
    }

    #[test]
    fn zero_power_clamps_to_min_lp() {
        let rules = rules_with_power("1.0", "0.5", "0.9");
        assert_ppm_matches_f64(0, 100, &rules, 1.0, 0.5, 0.9);
    }

    #[test]
    fn aggressive_penalty_modifier() {
        // lppm=1.5 means each percentage-point of power loss costs 1.5pp of speed.
        let rules = rules_with_power("1.5", "0.25", "1.0");
        assert_ppm_matches_f64(60, 100, &rules, 1.5, 0.25, 1.0);
        assert_ppm_matches_f64(33, 100, &rules, 1.5, 0.25, 1.0);
        assert_ppm_matches_f64(0, 100, &rules, 1.5, 0.25, 1.0);
    }

    #[test]
    fn over_powered_clamps_ratio_to_one() {
        // produced > drained: power_pct must still cap at 1.0.
        let rules = rules_with_power("1.0", "0.5", "0.9");
        assert_ppm_matches_f64(200, 100, &rules, 1.0, 0.5, 0.9);
    }
}

#[cfg(test)]
mod build_time_integer_tests {
    use super::*;
    use crate::rules::ini_parser::IniFile;
    use crate::rules::ruleset::RuleSet;

    fn rules_with_units(speed: &str) -> RuleSet {
        let ini = IniFile::from_str(&format!(
            "[General]\nBuildSpeed={speed}\n\
             [InfantryTypes]\n[AircraftTypes]\n[BuildingTypes]\n\
             [VehicleTypes]\n0=GRIZZLY\n1=PRISM\n2=CHEAP\n3=STANDARD\n\
             [GRIZZLY]\nCost=700\nStrength=100\nArmor=heavy\n\
             [PRISM]\nCost=1500\nStrength=100\nArmor=light\nBuildTimeMultiplier=1.15\n\
             [CHEAP]\nCost=0\nStrength=100\nArmor=none\n\
             [STANDARD]\nCost=1000\nStrength=100\nArmor=none\n"
        ));
        RuleSet::from_ini(&ini).expect("should parse")
    }

    #[test]
    fn grizzly_tank_build_time() {
        // cost=700, BuildSpeed=0.05, BTM=1.0
        // Float: trunc(700*0.05*0.9) = 31. Integer: 700*50*9/10000 = 31.
        let rules = rules_with_units("0.05");
        let obj = rules.object("GRIZZLY").unwrap();
        assert_eq!(build_time_base_frames(&rules, obj), 31);
    }

    #[test]
    fn prism_tower_build_time() {
        // cost=1500, BuildSpeed=0.05, BTM=1.15 (x1000=1150)
        // Float: trunc(67.5)=67, trunc(67*1.15)=77. Integer: 67*1150/1000=77.
        let rules = rules_with_units("0.05");
        let obj = rules.object("PRISM").unwrap();
        assert_eq!(build_time_base_frames(&rules, obj), 77);
    }

    #[test]
    fn zero_cost_returns_zero() {
        let rules = rules_with_units("0.05");
        let obj = rules.object("CHEAP").unwrap();
        assert_eq!(build_time_base_frames(&rules, obj), 0);
    }

    #[test]
    fn existing_test_parity_buildspeed_1() {
        // Matches production_queue_tests: cost=1000, BuildSpeed=1.0, BTM=1.0
        // Float: trunc(1000*1.0*0.9)=900. Integer: 1000*1000*9/10000=900.
        let rules = rules_with_units("1.0");
        let obj = rules.object("STANDARD").unwrap();
        assert_eq!(build_time_base_frames(&rules, obj), 900);
    }
}

#[cfg(test)]
mod footprint_tests {
    use super::*;

    #[test]
    fn rectangle_only_4x3() {
        let cells = building_base_foundation_cells(10, 20, "4x3");
        assert_eq!(cells.len(), 12);
        assert!(cells.contains(&(10, 20)));
        assert!(cells.contains(&(13, 22)));
    }

    #[test]
    fn garefn_base_foundation_ignores_hidden_occupy_modifiers() {
        // GAREFN: Foundation=4x3, AddOccupy1=-1,0, AddOccupy2=-1,-1, RemoveOccupy1=3,1
        let cells = building_base_foundation_cells(10, 20, "4x3");
        assert_eq!(cells.len(), 12);
        assert!(cells.contains(&(13, 21)));
        assert!(!cells.contains(&(9, 19)));
        assert!(!cells.contains(&(9, 20)));
    }

    #[test]
    fn narefn_base_foundation_keeps_dock_pad_despite_remove_occupy() {
        let cells = building_base_foundation_cells(10, 20, "4x3");
        assert_eq!(cells.len(), 12);
        assert!(cells.contains(&(13, 21)));
        assert!(!cells.contains(&(8, 20)));
        assert!(!cells.contains(&(8, 19)));
        assert!(!cells.contains(&(8, 18)));
    }

    #[test]
    fn gsi_04_05_hidden_modifiers_never_change_footprint_alias() {
        let cells = building_footprint_cells(10, 20, "2x2", &[(-1, 0)], &[(1, 1)]);
        assert_eq!(cells.len(), 4);
        assert!(
            cells.contains(&(11, 21)),
            "RemoveOccupy is not a footprint cutout"
        );
        assert!(
            !cells.contains(&(9, 20)),
            "AddOccupy is not a footprint expansion"
        );
    }

    #[test]
    fn negative_offset_clamping() {
        let cells = building_footprint_cells(0, 0, "1x1", &[(-1, 0), (-1, -1)], &[]);
        assert_eq!(cells.len(), 1);
    }

    #[test]
    fn deduplication() {
        let cells = building_footprint_cells(10, 20, "2x2", &[(0, 0)], &[]);
        assert_eq!(cells.len(), 4);
    }

    #[test]
    fn movement_blocking_no_bib_keeps_full_footprint() {
        let footprint = building_base_foundation_cells(10, 20, "4x3");
        let blocking = building_movement_blocking_cells(&footprint, false);
        assert_eq!(blocking.len(), footprint.len());
    }

    #[test]
    fn movement_blocking_with_bib_drops_east_edge_rectangle() {
        // Plain 4x3 with bib → east column (x = 13) drops.
        let footprint = building_base_foundation_cells(10, 20, "4x3");
        let blocking = building_movement_blocking_cells(&footprint, true);
        // 12 base cells - 3 east-edge column (13, 20), (13, 21), (13, 22) = 9.
        assert_eq!(blocking.len(), 9);
        assert!(!blocking.contains(&(13, 20)));
        assert!(!blocking.contains(&(13, 21)));
        assert!(!blocking.contains(&(13, 22)));
        assert!(blocking.contains(&(12, 20)));
        assert!(blocking.contains(&(10, 22)));
    }

    #[test]
    fn movement_blocking_with_bib_garefn_uses_base_foundation_topology() {
        let footprint = building_base_foundation_cells(10, 20, "4x3");
        let blocking = building_movement_blocking_cells(&footprint, true);
        assert_eq!(blocking.len(), 9);
        assert!(!blocking.contains(&(9, 19)));
        assert!(!blocking.contains(&(9, 20)));
        assert!(blocking.contains(&(12, 21)));
        assert!(!blocking.contains(&(13, 20)));
        assert!(!blocking.contains(&(13, 21)));
        assert!(blocking.contains(&(10, 20)));
    }

    #[test]
    fn static_movement_blocking_keeps_number_rows_out_of_the_grid() {
        let footprint = building_base_foundation_cells(10, 20, "4x3");
        let blocking = building_movement_blocking_cells(&footprint, false);
        assert_eq!(blocking.len(), 12);
        assert!(blocking.contains(&(12, 21)));
        assert!(blocking.contains(&(13, 21)));
    }
}
