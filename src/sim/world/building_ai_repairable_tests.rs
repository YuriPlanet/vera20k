//! Phobos `RepairBaseNodes` over the Unlimbo AI-repairable grant
//! (`Ext/Building/Hooks.cpp:316-331`, hook `0x440B4F`
//! `BuildingClass_Unlimbo_SetShouldRebuild`): a campaign building takes the
//! vanilla owner checks only through the house's difficulty slot or the
//! `[Basic]` global, never as a map-created structure.

use super::*;
use crate::map::entities::EntityCategory;
use crate::rules::ini_parser::IniFile;

fn rules(basic: &str) -> RuleSet {
    // `[Basic] RepairBaseNodes=` and the house slots are Ares keys.
    let _ares = crate::rules::ares_switch::read_ares_way(true);
    RuleSet::from_ini(&IniFile::from_str(&format!(
        "[Basic]\n{basic}\
         [BuildingTypes]\n0=POWER\n[POWER]\nStrength=100\nFoundation=1x1\n"
    )))
    .expect("repair base nodes fixture rules parse")
}

fn sim_with_ai_house() -> Simulation {
    let mut sim = Simulation::new();
    let owner = sim.interner.intern("AiHouse");
    sim.houses
        .register(crate::sim::house_state::HouseState::new(
            owner, 0, None, false, 0, 10,
        ));
    sim
}

fn ai_house(sim: &Simulation) -> InternedId {
    sim.interner.get("AiHouse").unwrap()
}

fn insert_structure(sim: &mut Simulation, owner: InternedId) -> u64 {
    let type_ref = sim.interner.intern("POWER");
    sim.insert_synthetic_techno_for_diagnostics(
        6,
        5,
        0,
        0,
        owner,
        crate::sim::combat::Health::new(100),
        type_ref,
        EntityCategory::Structure,
        0,
        8,
        false,
    )
}

fn granted(sim: &Simulation, id: u64) -> bool {
    sim.substrate
        .entities
        .get(id)
        .unwrap()
        .ai_sell_repair
        .ai_repairable()
}

/// The campaign grant follows the `[Basic]` global when the house's slots
/// are unset; the default no keeps vanilla's never-set behavior.
#[test]
fn campaign_ai_repairable_follows_the_basic_global() {
    let enabled = rules("RepairBaseNodes=yes\n");
    let mut sim = sim_with_ai_house();
    let owner = ai_house(&sim);
    let id = insert_structure(&mut sim, owner);
    Simulation::mark_ai_repairable_at_unlimbo(
        &sim.session,
        &enabled,
        &mut sim.substrate,
        &sim.houses,
        id,
    );
    assert!(
        granted(&sim, id),
        "[Basic] RepairBaseNodes=yes grants the campaign AI building"
    );

    let vanilla = rules("");
    let mut sim = sim_with_ai_house();
    let owner = ai_house(&sim);
    let id = insert_structure(&mut sim, owner);
    Simulation::mark_ai_repairable_at_unlimbo(
        &sim.session,
        &vanilla,
        &mut sim.substrate,
        &sim.houses,
        id,
    );
    assert!(
        !granted(&sim, id),
        "the global defaults no; vanilla never grants in a campaign"
    );
}

/// A preplaced structure keeps the map line's own AI Repairable: the grant
/// skips `IsCreatedFromMapFile` buildings even with the flag on.
#[test]
fn campaign_grant_skips_a_map_created_structure() {
    let enabled = rules("RepairBaseNodes=yes\n");
    let mut sim = sim_with_ai_house();
    let owner = ai_house(&sim);
    let id = insert_structure(&mut sim, owner);
    sim.substrate
        .entities
        .get_mut(id)
        .unwrap()
        .set_created_from_map_file(true);
    Simulation::mark_ai_repairable_at_unlimbo(
        &sim.session,
        &enabled,
        &mut sim.substrate,
        &sim.houses,
        id,
    );
    assert!(
        !granted(&sim, id),
        "a map-created structure is never granted"
    );
}

/// The house's own slot for the game difficulty row overrides the global
/// (`RepairBaseNodes[Difficulty].Get(global)`); unset slots follow it.
#[test]
fn the_house_slot_overrides_the_global_per_difficulty() {
    let enabled = rules("RepairBaseNodes=yes\n");
    let vanilla = rules("");
    let mut sim = sim_with_ai_house();
    let owner = ai_house(&sim);
    // The session's campaign row defaults to Normal (index 1).
    sim.houses
        .get_mut(&owner)
        .unwrap()
        .set_repair_base_nodes_for_test([None, Some(false), None]);
    let id = insert_structure(&mut sim, owner);
    Simulation::mark_ai_repairable_at_unlimbo(
        &sim.session,
        &enabled,
        &mut sim.substrate,
        &sim.houses,
        id,
    );
    assert!(
        !granted(&sim, id),
        "the house's Normal-slot no overrides the global yes"
    );

    sim.houses
        .get_mut(&owner)
        .unwrap()
        .set_repair_base_nodes_for_test([None, Some(true), None]);
    let id = insert_structure(&mut sim, owner);
    Simulation::mark_ai_repairable_at_unlimbo(
        &sim.session,
        &vanilla,
        &mut sim.substrate,
        &sim.houses,
        id,
    );
    assert!(
        granted(&sim, id),
        "the house's Normal-slot yes grants without a global"
    );
}
