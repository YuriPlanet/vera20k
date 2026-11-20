//! Phobos `HouseExt::ActiveHarvesterCount` and `TotalHarvesterCount`
//! (`Ext/House/Body.cpp:278-303`) with `TechnoExt::IsHarvesting`
//! (`Ext/Techno/Body.cpp:78-129`), counted through the entity store by
//! [`Simulation::harvester_counts`].
use crate::map::entities::EntityCategory;
use crate::rules::ini_parser::IniFile;
use crate::rules::mission_data::MissionType;
use crate::rules::ruleset::RuleSet;
use crate::sim::arena_fixture;
use crate::sim::combat::Health;
use crate::sim::game_entity::GameEntity;
use crate::sim::house_state::HouseState;
use crate::sim::intern::InternedId;
use crate::sim::mission::state::MissionId;
use crate::sim::world::Simulation;

const RULES: &str = "\
[General]
[Countries]
0=Americans
[Americans]
Side=Allied
[Sides]
Allied=Americans
[InfantryTypes]
[AircraftTypes]
[VehicleTypes]
0=HARV
1=SLVD
[BuildingTypes]
0=REFN
1=DERICK
[HARV]
Strength=1000
Speed=8
Harvester=yes
Dock=REFN
[SLVD]
Strength=1000
Speed=8
Harvester=yes
Harvester.Counted=no
Dock=REFN
[REFN]
Owner=Americans
Strength=1000
Refinery=yes
[DERICK]
Owner=Americans
Strength=1000
Power=-5
Harvester.Counted=yes
";

fn rules() -> RuleSet {
    let mut rules = RuleSet::from_ini(&IniFile::from_str(RULES)).expect("rules read");
    for building in ["REFN", "DERICK"] {
        rules.set_buildup_control_for_test(building, [0, 17, 3]);
    }
    rules
}

fn fixture(rules: &RuleSet) -> (Simulation, InternedId) {
    let mut sim = Simulation::new();
    arena_fixture::flat_ground(&mut sim, rules);
    let owner = sim.interner.intern("Player");
    sim.houses.register(HouseState::new(
        owner,
        0,
        Some(sim.interner.intern("Americans")),
        true,
        5000,
        10,
    ));
    (sim, owner)
}

/// The total adds `TechnoExt::HasBeenPlacedOnMap`, so an object that never left
/// limbo joins neither number. The working count then needs the Harvest mission
/// and one of this type's `Dock=` buildings present
/// (`TechnoExt::HasAvailableDock`, `Body.cpp:131-140`).
#[test]
fn a_placed_miner_needs_harvest_and_an_owned_dock() {
    let rules = rules();
    let (mut sim, owner) = fixture(&rules);
    let miner = sim
        .spawn_object("HARV", "Player", 12, 12, 0, &rules)
        .expect("miner");
    let miner_type = sim.interner.intern("HARV");
    sim.entities_mut_for_test()
        .insert(GameEntity::new_at_frame_zero_for_test(
            9_000,
            20,
            20,
            0,
            0,
            owner,
            Health::new(100),
            miner_type,
            EntityCategory::Unit,
            0,
            5,
            false,
        ));
    assert_eq!(
        sim.harvester_counts(&rules, owner),
        (0, 1),
        "the placed miner is in the total, the never-placed one is not"
    );

    sim.substrate
        .mission_assign_exact(miner, MissionId::from_known(MissionType::Harvest), 100);
    assert_eq!(
        sim.harvester_counts(&rules, owner),
        (0, 1),
        "on Harvest, but the house has no dock yet"
    );

    // One tracked REFN answers CountOwnedAndPresent for the dock test, which
    // is what `HasAvailableDock` reads.
    let refinery_type = sim.interner.intern("REFN");
    sim.houses_mut_for_test()
        .get_mut(&owner)
        .expect("house")
        .tracking_mut_for_test()
        .set_owned_for_test(EntityCategory::Structure, refinery_type, 1);
    assert_eq!(
        sim.harvester_counts(&rules, owner),
        (1, 1),
        "an owned Dock= building makes the same miner work"
    );
}

/// `Harvester.Counted=no` overrides the `Harvester=yes` fallback, so the type
/// joins neither number.
#[test]
fn a_counted_no_type_joins_neither_number() {
    let rules = rules();
    let (mut sim, owner) = fixture(&rules);
    let miner = sim
        .spawn_object("SLVD", "Player", 12, 12, 0, &rules)
        .expect("excluded miner");
    sim.substrate
        .mission_assign_exact(miner, MissionId::from_known(MissionType::Harvest), 100);
    assert_eq!(sim.harvester_counts(&rules, owner), (0, 0));
}

/// A counted building — the documented oil-derrick use — is working while its
/// `IsPowerOnline` latch is up, and stops counting as working once it goes
/// offline (`Body.cpp:88-89`).
#[test]
fn a_counted_building_works_only_while_powered_online() {
    let rules = rules();
    let (mut sim, owner) = fixture(&rules);
    let derrick = 7_000;
    let derrick_type = sim.interner.intern("DERICK");
    sim.entities_mut_for_test()
        .insert(GameEntity::new_at_frame_zero_for_test(
            derrick,
            14,
            10,
            0,
            0,
            owner,
            Health::new(100),
            derrick_type,
            EntityCategory::Structure,
            0,
            5,
            false,
        ));
    sim.phobos_tracking_before_unlimbo(derrick);
    sim.entities_mut_for_test()
        .get_mut(derrick)
        .expect("derrick")
        .lifecycle
        .set_in_limbo_for_test(false);
    assert_eq!(
        sim.harvester_counts(&rules, owner),
        (1, 1),
        "a placed, powered counted building works"
    );

    sim.entities_mut_for_test()
        .get_mut(derrick)
        .expect("derrick")
        .set_building_online_for_test(false);
    assert_eq!(
        sim.harvester_counts(&rules, owner),
        (0, 1),
        "the total keeps it, the working count does not"
    );
}
