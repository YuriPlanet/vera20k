//! The crush loop of `UnitClass::Crush_Or_Prescatter_Cell @ 0x007416A0`,
//! run through `Simulation::per_cell_process` on retail rules.
use super::*;
use crate::rules::retail_ini_fixture::retail_unlayered_rules_with_art;
use crate::sim::occupancy::OccupancyGrid;

/// A Battle Fortress driven onto a Grizzly crushes it, and the victim's
/// vtable `+0x170` (`0x007418E5` -> `0x00746D60`) runs Death_Explosion
/// (`0x00738680`): one Scenario pick from `Explosion=`, its anim at the
/// tank's Location (stock vehicles author no `DestroyAnim=`), before the
/// kill is recorded and the tank is UnInit. Having crushed a Unit, the
/// level fortress then tilts forwards.
#[test]
fn a_battle_fortress_crushing_a_tank_plays_its_death_explosion() {
    let Some(mut rules) = retail_unlayered_rules_with_art() else {
        return;
    };
    // Asset binding gives each anim type its SHP frame count; the count is
    // not under test.
    for name in rules.object("MTNK").unwrap().explosion_anims.clone() {
        rules.bind_anim_frame_count_for_test(&name, 20);
    }
    let mut sim = crate::sim::arena_fixture::rival_arena(&rules, 0x7418E5);
    let fortress = sim
        .spawn_object("BFRT", "Russians", 10, 10, 0, &rules)
        .expect("BFRT spawns");
    let tank = sim
        .spawn_object("MTNK", "Americans", 12, 10, 0, &rules)
        .expect("MTNK spawns");
    // The tank stands a little off the fortress's centre, inside the crush
    // distance, so its Location is not the crusher's.
    let mut position = sim.substrate.entities.get(fortress).unwrap().position;
    let centre = position_world_coord(&position);
    position.put_world_xy([centre.x + 40, centre.y - 24]);
    sim.substrate.entities.get_mut(tank).unwrap().position = position;
    sim.substrate.occupancy = OccupancyGrid::rebuild(&sim.substrate.entities);
    let location = position_world_coord(&position);
    let object = rules.object("MTNK").unwrap();
    let mut replay = sim.scenario_rng.clone();
    assert!(object.destroy_anims.is_empty());
    let explosion = &object.explosion_anims;
    let expected = explosion[(replay.next_u32() % explosion.len() as u32) as usize].clone();
    assert!(sim.substrate.anims.iter().next().is_none());
    sim.per_cell_process(
        fortress,
        PerCellReason::Arrival,
        Some(&rules),
        None,
        FrameEffects::default(),
    )
    .unwrap();
    assert!(
        sim.substrate
            .entities
            .get(tank)
            .is_none_or(|tank| !tank.lifecycle.object_alive()),
        "crushed"
    );
    let played: Vec<_> = sim
        .substrate
        .anims
        .iter()
        .map(|(_, anim)| {
            let coord = anim.world_coord();
            (
                sim.interner.resolve(anim.type_id()).to_string(),
                (coord.x, coord.y, coord.z),
            )
        })
        .collect();
    assert_eq!(played, [(expected, (location.x, location.y, location.z))]);
    assert_eq!(sim.scenario_rng.state(), replay.state());
    // The crush tail (`0x00741939..0x00741954`): the level fortress tilts.
    let rocking = sim
        .substrate
        .entities
        .get(fortress)
        .unwrap()
        .rocking
        .unwrap();
    assert_eq!(rocking.native_bits()[3], 0xbd4c_cccd);
}
