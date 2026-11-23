//! Running-game diagnostic for the ALL02S "Chapel captured == destroyed" report.
//!
//! This is deliberately a runtime trace, not a unit test: the question is what
//! the real campaign scenario resolves in RA2 Mode while the player captures
//! the Air Force Academy. All output goes through the engine's own file logger,
//! so it lands in `logs/ra2.log` beside the game.
//!
//! It is inert unless the game plays RA2 Mode, and it only writes
//! about BuildingTypes[186] (the index used by ALL02S' Chapel triggers), so an
//! ordinary skirmish session produces no lines. Nothing here changes gameplay:
//! every hook is a read-only observation of state that already exists.

use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};

use crate::map::entities::EntityCategory;
use crate::rules::ruleset::RuleSet;
use crate::sim::intern::InternedId;
use crate::sim::world::Simulation;

const TAG: &str = "RA2-ALL02";
const CHAPEL_BUILDING_INDEX: usize = 186;

static BOOT_WRITTEN: AtomicBool = AtomicBool::new(false);
static LAST_EVENT: Mutex<Option<(i32, String, i32, bool)>> = Mutex::new(None);

fn active(rules: &RuleSet) -> bool {
    rules.ra2()
}

fn chapel_type(sim: &Simulation, rules: &RuleSet) -> Option<(String, InternedId)> {
    let name = rules.building_ids.get(CHAPEL_BUILDING_INDEX)?.clone();
    let id = sim.interner.get(&name)?;
    Some((name, id))
}

fn house_count(sim: &Simulation, owner: InternedId, type_id: InternedId) -> i32 {
    sim.houses.get(&owner).map_or(0, |house| {
        house
            .tracking()
            .active_count(EntityCategory::Structure, type_id)
    })
}

fn house_label(sim: &Simulation, house: InternedId) -> String {
    let name = sim.interner.resolve(house);
    let index = sim
        .houses
        .index_of(house)
        .map_or_else(|| "?".to_string(), |index| index.to_string());
    let country = sim
        .houses
        .get(&house)
        .and_then(|state| state.country())
        .map_or("?".to_string(), |id| sim.interner.resolve(id).to_string());
    format!("{name}[house #{index}, country={country}]")
}

fn all_house_counts(sim: &Simulation, type_id: InternedId) -> String {
    sim.houses
        .order()
        .iter()
        .map(|&house| format!("{}={}", house_label(sim, house), house_count(sim, house, type_id)))
        .collect::<Vec<_>>()
        .join(" | ")
}

fn ensure_boot(sim: &Simulation, rules: &RuleSet) {
    if !active(rules) || BOOT_WRITTEN.swap(true, Ordering::SeqCst) {
        return;
    }
    let frame = sim.session.binary_frame();
    match chapel_type(sim, rules) {
        Some((name, type_id)) => log::info!(
            "[{TAG}] BOOT frame={frame} building_ids[{CHAPEL_BUILDING_INDEX}]={name} counts=[{}]",
            all_house_counts(sim, type_id)
        ),
        None => log::info!(
            "[{TAG}] BOOT frame={frame} building_ids[{CHAPEL_BUILDING_INDEX}]=<unresolved>"
        ),
    }
    let resolve = |token: &str| {
        let country = rules.trigger_house_type_index(token);
        let house = country.and_then(|country| sim.house_by_country_index(country, rules));
        format!(
            "{token} -> {}",
            house.map_or_else(|| "<no house>".to_string(), |house| house_label(sim, house))
        )
    };
    log::info!(
        "[{TAG}] BOOT trigger owners: {} ; {}",
        resolve("NeutralA"),
        resolve("French")
    );
}

/// Called once on the first trigger evaluation of a loaded scenario so the
/// log always contains the static resolution, even before the player reaches
/// the Chapel.
pub(crate) fn on_trigger_evaluation(sim: &Simulation, rules: &RuleSet) {
    ensure_boot(sim, rules);
}

/// Event 32 ("house owns at least one") / 57 ("house owns none"), the two
/// ALL02S Chapel predicates. Printing only on a changed observation keeps a
/// long play session from flooding the log.
pub(crate) fn on_building_count_event(
    sim: &Simulation,
    rules: &RuleSet,
    incoming_house: Option<InternedId>,
    kind: i32,
    value: i32,
    count: i32,
    result: bool,
) {
    if !active(rules) || value != CHAPEL_BUILDING_INDEX as i32 {
        return;
    }
    ensure_boot(sim, rules);
    let owner = incoming_house.map_or_else(
        || "<none>".to_string(),
        |house| house_label(sim, house),
    );
    let key = (kind, owner.clone(), count, result);
    let mut last = LAST_EVENT.lock().unwrap_or_else(|error| error.into_inner());
    if last.as_ref() == Some(&key) {
        return;
    }
    *last = Some(key);
    let Some((name, type_id)) = chapel_type(sim, rules) else {
        return;
    };
    log::info!(
        "[{TAG}] EVENT kind={kind} value={value} type={name} owner={owner} count={count} result={result} frame={} counts=[{}]",
        sim.session.binary_frame(),
        all_house_counts(sim, type_id)
    );
}

/// Owner transfer of a BuildingTypes[186] structure — the path an engineer
/// capture takes. The BEFORE/AFTER pair shows whether `NeutralA`'s count goes
/// to zero, which is what event 57 observes.
pub(crate) fn on_owner_change(
    sim: &Simulation,
    stable_id: u64,
    old_owner: InternedId,
    new_owner: InternedId,
    rules: &RuleSet,
    phase: &str,
) {
    if !active(rules) {
        return;
    }
    let Some((name, type_id)) = chapel_type(sim, rules) else {
        return;
    };
    let is_chapel = sim
        .substrate
        .entities
        .get(stable_id)
        .is_some_and(|entity| {
            entity.category() == EntityCategory::Structure && entity.type_ref() == type_id
        });
    if !is_chapel {
        return;
    }
    ensure_boot(sim, rules);
    log::info!(
        "[{TAG}] OWNER-{phase} frame={} entity={stable_id} type={name} old={}(count={}) new={}(count={}) counts=[{}]",
        sim.session.binary_frame(),
        house_label(sim, old_owner),
        house_count(sim, old_owner, type_id),
        house_label(sim, new_owner),
        house_count(sim, new_owner, type_id),
        all_house_counts(sim, type_id)
    );
}
