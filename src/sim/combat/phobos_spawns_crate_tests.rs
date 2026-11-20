//! Phobos `SpawnsCrate*` through `WarheadTypeExt::Detonate`: one weighted
//! powerup crate at the impact (one pick draw, then the crate-timer draw).

use super::{crit, world_receiver};
use crate::rules::ini_parser::IniFile;
use crate::rules::powerups::{POWERUP_MONEY, POWERUP_VETERAN};
use crate::rules::ruleset::RuleSet;
use crate::sim::crates::tests::{crate_cells, sim_with_grid};
use crate::sim::projectile::ProjectileCoord;

/// The `[CrateRules]` images the fixture names; the shared crate registry has
/// only the dense OV*/STEEPCRATE index space, so this one carries the names.
fn crate_images_registry() -> crate::rules::overlay_types::OverlayTypeRegistry {
    crate::rules::overlay_types::OverlayTypeRegistry::from_ini(
        &IniFile::from_str(
            "[OverlayTypes]
0=SILVER
1=WOOD
2=WATER
             [SILVER]
Crate=yes
[WOOD]
Crate=yes
[WATER]
Crate=yes
",
        ),
        None,
    )
}

fn rules(keys: &str) -> RuleSet {
    RuleSet::from_ini(&IniFile::from_str(&format!(
        "[InfantryTypes]\n[VehicleTypes]\n[AircraftTypes]\n[BuildingTypes]\n\
         [CrateRules]\nCrateImg=SILVER\nWoodCrateImg=WOOD\nWaterCrateImg=WATER\n\
         [Warheads]\n0=WH\n[WH]\n{keys}"
    )))
    .expect("rules")
}

fn fire(
    sim: &mut crate::sim::world::Simulation,
    rules: &RuleSet,
    owner: crate::sim::intern::InternedId,
    impact: ProjectileCoord,
    with_registry: bool,
) {
    let registry;
    let registry_ref = if with_registry {
        registry = crate_images_registry();
        Some(&registry)
    } else {
        None
    };
    let warhead = rules.warhead("WH").expect("warhead");
    let warhead_ref = sim.interner.intern("WH");
    let mut run = world_receiver::ReceiverRun::default();
    let mut effects = crate::sim::combat::DeathEffects::default();
    crit::detonate(
        sim,
        &mut run,
        rules,
        registry_ref,
        warhead,
        warhead_ref,
        0,
        Some(owner),
        impact,
        None,
        0,
        false,
        &mut effects,
    );
    run.finish();
}

fn register_owner(sim: &mut crate::sim::world::Simulation) -> crate::sim::intern::InternedId {
    let id = sim.interner.intern("Americans");
    sim.houses
        .register(crate::sim::house_state::HouseState::new(
            id, 0, None, true, 0, 10,
        ));
    id
}

#[test]
fn a_spawns_crate_powerup_is_placed_with_one_draw() {
    let rules = rules("SpawnsCrate0.Type=Money\nSpawnsCrate0.Weight=1\n");
    let registry = crate_images_registry();
    let mut sim = sim_with_grid(11);
    let owner = register_owner(&mut sim);
    let mut expected = sim.scenario_rng.clone();
    expected.random_double();
    // The placement's own crate-timer draw follows the weighted pick.
    expected.random_double();
    let impact = ProjectileCoord::new(20 * 256 + 128, 20 * 256 + 128, 0);
    fire(&mut sim, &rules, owner, impact, true);
    let cells = crate_cells(&sim, &registry);
    assert_eq!(cells.len(), 1, "one crate was placed");
    let grid = sim.overlay_grid.as_ref().expect("overlay grid");
    assert_eq!(
        grid.cell(cells[0].0, cells[0].1).overlay_data,
        POWERUP_MONEY as u8,
        "the stored selection is the configured powerup"
    );
    assert_eq!(
        sim.scenario_rng.logical_state(),
        expected.logical_state(),
        "the pick drew once and the crate timer drew once"
    );
}

#[test]
fn weights_pick_the_second_powerup_and_a_missing_registry_still_draws() {
    let rules = rules(
        "SpawnsCrate0.Type=Money\nSpawnsCrate0.Weight=0\n\
         SpawnsCrate1.Type=Veteran\nSpawnsCrate1.Weight=100\n",
    );
    let registry = crate_images_registry();
    let mut sim = sim_with_grid(22);
    let owner = register_owner(&mut sim);
    let impact = ProjectileCoord::new(20 * 256 + 128, 20 * 256 + 128, 0);
    fire(&mut sim, &rules, owner, impact, true);
    let cells = crate_cells(&sim, &registry);
    assert_eq!(cells.len(), 1);
    let grid = sim.overlay_grid.as_ref().expect("overlay grid");
    assert_eq!(
        grid.cell(cells[0].0, cells[0].1).overlay_data,
        POWERUP_VETERAN as u8,
        "weight 0 never reaches the first slot"
    );

    let mut sim = sim_with_grid(22);
    let owner = register_owner(&mut sim);
    let mut expected = sim.scenario_rng.clone();
    expected.random_double();
    fire(&mut sim, &rules, owner, impact, false);
    assert!(
        crate_cells(&sim, &registry).is_empty(),
        "no overlay registry means no crate"
    );
    assert_eq!(
        sim.scenario_rng.logical_state(),
        expected.logical_state(),
        "the pick still drew before the placement was refused"
    );
}

/// `Detonate.cpp:49` encloses the block in `if (pHouse)`: no House, no draw.
#[test]
fn a_house_less_detonation_draws_nothing() {
    let rules = rules("SpawnsCrate0.Type=Money\n");
    let registry = crate_images_registry();
    let mut sim = sim_with_grid(33);
    let before = sim.scenario_rng.logical_state();
    let warhead = rules.warhead("WH").expect("warhead");
    let warhead_ref = sim.interner.intern("WH");
    let impact = ProjectileCoord::new(20 * 256 + 128, 20 * 256 + 128, 0);
    let mut run = world_receiver::ReceiverRun::default();
    let mut effects = crate::sim::combat::DeathEffects::default();
    crit::detonate(
        &mut sim,
        &mut run,
        &rules,
        Some(&registry),
        warhead,
        warhead_ref,
        0,
        None,
        impact,
        None,
        0,
        false,
        &mut effects,
    );
    run.finish();
    assert!(
        crate_cells(&sim, &registry).is_empty(),
        "a house-less detonation places nothing"
    );
    assert_eq!(
        sim.scenario_rng.logical_state(),
        before,
        "and consumes no draw"
    );
}
