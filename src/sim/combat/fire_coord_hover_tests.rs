//! GetFLH (`0x006F3B9B`) and the turret pivot (`0x006F3DB7`) take a Hover's
//! basis from its Draw_Matrix (`0x00513F40`): the shared ground matrix at
//! [`hover_draw_slope`](crate::sim::movement::hover::hover_draw_slope)'s
//! slope. Each arm is checked against the same Foot read through Drive, whose
//! GetFLH basis the executed drone rows check (`drone_rocking_tests`).
use super::{FireSource, fire_coordinate, turret_pivot_coordinate};
use crate::rules::flh::Flh;
use crate::rules::locomotor_type::LocomotorKind;
use crate::rules::ruleset::RuleSet;
use crate::sim::movement::locomotor::LocomotorState;
use crate::sim::movement::slope_transition::SlopeTransitionState;
use crate::sim::projectile::ProjectileCoord;
use crate::sim::world::Simulation;

/// `slope01.tem`'s ramp, rising toward +X.
const RAMP: u8 = 1;

/// The muzzle of weapon 0 and the turret pivot.
fn muzzle(sim: &Simulation, rules: &RuleSet, id: u64) -> [ProjectileCoord; 2] {
    let entity = sim.substrate.entities.get(id).unwrap();
    let object = sim.interner.object_type(entity.type_ref(), rules).unwrap();
    let fire = fire_coordinate(
        sim,
        rules,
        &FireSource::of_entity(entity),
        object,
        0,
        0,
        Flh::default(),
    )
    .coord;
    [
        fire,
        turret_pivot_coordinate(sim, rules, entity, 0).unwrap(),
    ]
}

/// [`muzzle`] with the Foot's locomotor swapped for a Drive settled on
/// `slope`, keeping its facing and rocking.
fn as_drive(sim: &mut Simulation, rules: &RuleSet, id: u64, slope: u8) -> [ProjectileCoord; 2] {
    let entity = sim.substrate.entities.get_mut(id).unwrap();
    let mut drive = LocomotorState::for_test_kind(LocomotorKind::Drive);
    *drive.active_slope_transition_mut().unwrap() =
        SlopeTransitionState::from_fields_for_test(slope, slope, 0, 0);
    let hover = entity.locomotor.replace(drive);
    let read = muzzle(sim, rules, id);
    sim.substrate.entities.get_mut(id).unwrap().locomotor = hover;
    read
}

/// A powered ROBO on a ramp fires from the level facing matrix
/// (`0x0051407F..0x00514092`), a rocking one from the slope-0 tilt
/// (`0x00514262`, `0x00514270`), and only an unpowered one at rest from its
/// cell's slope (`0x00513F93..0x0051405C`).
#[test]
fn a_hover_fires_through_its_draw_matrix() {
    let Some(battle) = crate::rules::retail_ini_fixture::retail_battle_rules() else {
        return;
    };
    let Some((_, assets)) = crate::rules::retail_ini_fixture::retail_assets() else {
        return;
    };
    let mut rules = battle.rules;
    rules.bind_effect_assets(&assets, "TEM", "TEMPERATE");
    assert!(rules.object_voxel_half_sizes("ROBO").is_some());
    let mut sim = crate::sim::arena_fixture::rival_arena_with_size(&rules, 1, 32);
    sim.intern_rule_type_ids(&rules);
    sim.resolve_type_handles(&rules);
    sim.resolved_terrain
        .as_mut()
        .unwrap()
        .cell_mut_for_test(10, 10)
        .unwrap()
        .slope_type = RAMP;
    let robo = sim
        .spawn_object("ROBO", "Americans", 10, 10, 64, &rules)
        .expect("stock ROBO on the ramp");

    let level = muzzle(&sim, &rules, robo);
    assert_eq!(level, as_drive(&mut sim, &rules, robo, 0));
    assert_ne!(level, as_drive(&mut sim, &rules, robo, RAMP));

    let mut rocking = crate::sim::rocking::RockingState::default();
    rocking.set_angles_native_for_test([0.25, -0.1]);
    sim.substrate.entities.get_mut(robo).unwrap().rocking = Some(rocking);
    let tilted = muzzle(&sim, &rules, robo);
    assert_ne!(tilted, level);
    assert_eq!(tilted, as_drive(&mut sim, &rules, robo, 0));

    let entity = sim.substrate.entities.get_mut(robo).unwrap();
    entity.rocking = None;
    entity.locomotor.as_mut().unwrap().power_off();
    {
        let cells = sim
            .resolved_terrain
            .as_ref()
            .map(crate::map::resolved_terrain::NativeCellQuery::isolated);
        let entity = sim.substrate.entities.get(robo).unwrap();
        let height =
            crate::sim::movement::air_movement::queried_flight_height(entity, cells.as_ref());
        assert!(height <= 0, "it rests on the ramp, {height}");
    }
    assert_eq!(
        muzzle(&sim, &rules, robo),
        as_drive(&mut sim, &rules, robo, RAMP)
    );
}
