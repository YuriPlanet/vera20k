//! Phobos `Deploy.*` / `IsSimpleDeployer.*` through the production unload and
//! deploy-admission owners (`Ext/Unit/Hooks.Unload.cpp`, `Ext/Unit/Body.cpp:258`,
//! `TechnoClass::CanDeploySlashUnload @ 0x00700D50`).

use crate::rules::ruleset::RuleSet;
use crate::rules::tiberium_type::TiberiumTypeId;
use crate::sim::miner::{CargoBale, ResourceType};
use crate::sim::world::Simulation;

const LISTS: &str = "[InfantryTypes]\n[AircraftTypes]\n[BuildingTypes]\n[VehicleTypes]\n";

fn scene(text: &str) -> (Simulation, RuleSet) {
    let ini = crate::sim::arena_fixture::clear_ground_ini(text);
    let rules = RuleSet::from_ini(&ini).expect("deploy-gate rules");
    let mut sim = Simulation::with_seed(0);
    crate::sim::arena_fixture::flat_ground(&mut sim, &rules);
    (sim, rules)
}

fn spawn(sim: &mut Simulation, rules: &RuleSet, name: &str) -> u64 {
    sim.spawn_object(name, "Russians", 10, 10, 0, rules)
        .expect("deploy-gate unit spawns")
}

/// `UnitClass_Mi_Unload_Subterranean` (hook 0x73D63B,
/// `Ext/Unit/Hooks.Unload.cpp:53-58`): SkipPassengerUnload admits a loaded
/// transport's deploy-fire path; NoPassenger skips only while empty AND
/// MissionStatus == 0, and unloads normally once a passenger boards.
#[test]
fn passenger_skip_gates_the_unload_deploy_path() {
    let (mut sim, rules) = scene(&format!(
        "{LISTS}0=TRN\n[TRN]\nStrength=300\nSpeed=6\nPassengers=2\nDeployFire=yes\n\
         Deploy.SkipPassengerUnload=yes\n"
    ));
    let id = spawn(&mut sim, &rules, "TRN");
    sim.substrate
        .entities
        .get_mut(id)
        .unwrap()
        .passenger_role
        .cargo_mut()
        .unwrap()
        .board_forced_chain(&[(999, 1.0)]);
    let entity = sim.substrate.entities.get(id).unwrap();
    assert_eq!(entity.passenger_role.passenger_count(), 1);
    assert!(super::phobos_unload_skips_passengers(&sim, entity, &rules));
    assert!(
        super::unit_fires_on_deploy(&sim, entity, &rules),
        "a loaded transport that skips unload takes the deploy-fire arm"
    );

    let (mut sim, rules) = scene(&format!(
        "{LISTS}0=TRN\n[TRN]\nStrength=300\nSpeed=6\nPassengers=2\nDeployFire=yes\n\
         Deploy.NoPassenger=yes\n"
    ));
    let id = spawn(&mut sim, &rules, "TRN");
    sim.substrate
        .entities
        .get_mut(id)
        .unwrap()
        .mission
        .set_handler_state(1);
    assert!(
        !super::phobos_unload_skips_passengers(
            &sim,
            sim.substrate.entities.get(id).unwrap(),
            &rules,
        ),
        "NoPassenger waits one status step (MissionStatus != 0)"
    );
    sim.substrate
        .entities
        .get_mut(id)
        .unwrap()
        .mission
        .set_handler_state(0);
    assert!(super::phobos_unload_skips_passengers(
        &sim,
        sim.substrate.entities.get(id).unwrap(),
        &rules,
    ));
    sim.substrate
        .entities
        .get_mut(id)
        .unwrap()
        .passenger_role
        .cargo_mut()
        .unwrap()
        .board_forced_chain(&[(999, 1.0)]);
    assert!(
        !super::phobos_unload_skips_passengers(
            &sim,
            sim.substrate.entities.get(id).unwrap(),
            &rules,
        ),
        "a loaded transport unloads its passengers"
    );
}

/// `UnitClass_Mi_Unload_SkipHarvester` (hook 0x73DEEB,
/// `Ext/Unit/Hooks.Unload.cpp:61-75`): Deploy.NoTiberium dumps a full hold and
/// skips only an empty one; the unload latch always dumps.
#[test]
fn no_tiberium_skips_only_an_empty_hold() {
    let (mut sim, rules) = scene(&format!(
        "{LISTS}0=HARV\n[HARV]\nStrength=300\nSpeed=6\nHarvester=yes\nStorage=20\n\
         Deploy.NoTiberium=yes\n"
    ));
    let id = spawn(&mut sim, &rules, "HARV");
    let bale = CargoBale {
        tiberium_type: TiberiumTypeId(0),
        resource_type: ResourceType::Ore,
        value: 25,
    };
    let miner = sim
        .substrate
        .entities
        .get_mut(id)
        .unwrap()
        .miner
        .as_mut()
        .unwrap();
    miner.set_cargo_for_test(vec![bale]);
    assert!(
        !sim.phobos_harvester_skips_tiberium_dump(id, &rules),
        "a full hold dumps its tiberium"
    );
    sim.substrate
        .entities
        .get_mut(id)
        .unwrap()
        .miner
        .as_mut()
        .unwrap()
        .set_cargo_for_test(vec![]);
    assert!(
        sim.phobos_harvester_skips_tiberium_dump(id, &rules),
        "an empty hold skips the dump for the deploy path"
    );
    let miner = sim
        .substrate
        .entities
        .get_mut(id)
        .unwrap()
        .miner
        .as_mut()
        .unwrap();
    miner.set_cargo_for_test(vec![bale]);
    miner.set_unload_active_for_test(true);
    assert!(
        !sim.phobos_harvester_skips_tiberium_dump(id, &rules),
        "an unloading harvester is never skipped"
    );
}

/// `UnitExt::SimpleDeployerAllowedToDeploy` (hook 0x7010C1,
/// `Ext/Unit/Body.cpp:258-316`) inside `CanDeploySlashUnload`: ConsiderPathfinding
/// refuses a simple deployer on a disallowed own-cell land type and admits a
/// permitted one; without the key the gate stays closed.
#[test]
fn consider_pathfinding_gates_the_simple_deployer_cell() {
    let base = "[DEP]\nStrength=300\nSpeed=6\nIsSimpleDeployer=yes\nSpeedType=Track\nMovementZone=Normal\n";
    let (mut sim, rules) = scene(&format!(
        "{LISTS}0=DEP\n{base}IsSimpleDeployer.ConsiderPathfinding=yes\n\
         IsSimpleDeployer.DisallowedLandTypes=Clear\n"
    ));
    let id = spawn(&mut sim, &rules, "DEP");
    assert!(
        !sim.can_deploy_slash_unload(id, &rules),
        "the own Clear cell is disallowed"
    );

    let (mut sim, rules) = scene(&format!(
        "{LISTS}0=DEP\n{base}IsSimpleDeployer.ConsiderPathfinding=yes\n\
         IsSimpleDeployer.DisallowedLandTypes=Water\n"
    ));
    let id = spawn(&mut sim, &rules, "DEP");
    assert!(
        sim.can_deploy_slash_unload(id, &rules),
        "a Clear cell is not Water-disallowed and passes pathfinding"
    );

    let (mut sim, rules) = scene(&format!(
        "{LISTS}0=DEP\n{base}IsSimpleDeployer.DisallowedLandTypes=Clear\n"
    ));
    let id = spawn(&mut sim, &rules, "DEP");
    assert!(
        sim.can_deploy_slash_unload(id, &rules),
        "without ConsiderPathfinding the gate stays closed"
    );
}

/// A Vehicle without IsSimpleDeployer (and an Infantry, whose arm never reaches
/// the gate) ignores ConsiderPathfinding/DisallowedLandTypes even on a disallowed
/// cell (`Ext/Unit/Body.cpp:262-263` returns the default).
#[test]
fn non_simple_deployers_ignore_the_pathfinding_gate() {
    let keys =
        "IsSimpleDeployer.ConsiderPathfinding=yes\nIsSimpleDeployer.DisallowedLandTypes=Clear\n";
    let (mut sim, rules) = scene(&format!(
        "[AircraftTypes]\n[BuildingTypes]\n[VehicleTypes]\n0=DFIRE\n[InfantryTypes]\n0=INF\n\
         [DFIRE]\nStrength=300\nSpeed=6\nDeployFire=yes\nSpeedType=Track\nMovementZone=Normal\n{keys}\
         [INF]\nStrength=100\nSpeed=4\nDeployer=yes\n{keys}"
    ));
    let id = spawn(&mut sim, &rules, "DFIRE");
    assert!(
        sim.can_deploy_slash_unload(id, &rules),
        "a DeployFire vehicle is not a simple deployer, so the gate does not apply"
    );
    // DFIRE already occupies (10, 10); a second object there is refused by
    // placement, and this test only exercises the admission gate.
    let infantry = sim
        .spawn_object("INF", "Russians", 12, 10, 0, &rules)
        .expect("the Infantry gate probe spawns beside the vehicle");
    assert!(
        sim.can_deploy_slash_unload(infantry, &rules),
        "the Infantry admission arm never reaches the Unit gate"
    );
}
