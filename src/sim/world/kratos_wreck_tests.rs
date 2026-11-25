//! Kratos `Wreck` through the production movement, scan and facing paths: a
//! frozen type keeps its coordinate, never looks for a target and never spins
//! its turret.

use crate::rules::ini_parser::IniFile;
use crate::rules::locomotor_type::LocomotorKind;
use crate::rules::ruleset::RuleSet;
use crate::sim::game_entity::GameEntity;
use crate::sim::movement::DriveLocomotionRuntime;
use crate::sim::movement::FacingClass;
use crate::sim::movement::locomotor::LocomotorState;
use crate::sim::movement::turret::facing_update;
use crate::sim::world::Simulation;

/// The arena's rules: one `WRCK` whose body carries `wreck_ini`, and one
/// `MTNK` that never does, so a scene's control differs by that body alone.
fn rules(wreck_ini: &str) -> RuleSet {
    RuleSet::from_ini(&IniFile::from_str(&format!(
        "[Countries]\n0=Americans\n1=Russians\n\
         [VehicleTypes]\n0=WRCK\n1=MTNK\n\
         [WRCK]\nStrength=300\nArmor=none\nSpeed=20\nSpeedType=Track\nPrimary=GUN\n{wreck_ini}\
         [MTNK]\nStrength=300\nArmor=none\nSpeed=20\nSpeedType=Track\nPrimary=GUN\n\
         [WeaponTypes]\n0=GUN\n\
         [GUN]\nDamage=10\nRange=5\nROF=20\nWarhead=WH\n\
         [Warheads]\n0=WH\n\
         [WH]\nCellSpread=0\n"
    )))
    .expect("rules")
}

/// A ruler's arena with one `WRCK` at (10, 10), ready to walk: the spawn gives
/// it the type's own locomotor, and this installs the drive runtime the ground
/// Process advances (the fixture path the movement suite uses).
fn scene(wreck_ini: &str) -> (Simulation, RuleSet, u64) {
    let rules = rules(wreck_ini);
    let mut sim = crate::sim::arena_fixture::rival_arena(&rules, 0x5A_AA_1E);
    sim.intern_rule_type_ids(&rules);
    sim.resolve_type_handles(&rules);
    let id = sim
        .spawn_object("WRCK", "Americans", 10, 10, 0, &rules)
        .expect("wreck");
    let entity = sim.substrate.entities.get_mut(id).expect("spawned");
    entity.mover_facts.set_drive_accelerates_for_test(false);
    entity.locomotor = Some(LocomotorState::for_test_kind(LocomotorKind::Drive));
    assert!(
        entity
            .locomotor
            .as_mut()
            .expect("locomotor")
            .install_drive_state_for_test(Some(DriveLocomotionRuntime::default())),
        "the drive runtime installs on a fresh spawn"
    );
    // `spawn_object` already unlimbos the fixture, so it is revealed; nothing
    // here conceals it again.
    (sim, rules, id)
}

fn position(sim: &Simulation, id: u64) -> [i32; 3] {
    let entity = sim.entities().get(id).expect("entity");
    let world = crate::sim::movement::ground_pose::position_world_coord(&entity.position);
    [world.x, world.y, world.z]
}

/// Give the object a destination six cells east.
fn order_move(sim: &mut Simulation, id: u64) {
    let terrain = sim.resolved_terrain.as_ref().expect("arena terrain");
    let entity = sim.substrate.entities.get_mut(id).expect("entity");
    crate::sim::movement::navcom::set_destination_internal_cell(entity, (16, 10), Some(terrain), 0);
    crate::sim::movement::DestinationTiming::new(0, 60).accept(entity);
}

/// Run `count` movement visits with the arena's flat path grid installed.
fn tick(
    sim: &mut Simulation,
    rules: &RuleSet,
    grid: &crate::sim::pathfinding::PathGrid,
    id: u64,
    count: u32,
) -> u32 {
    let mut moved = 0;
    for _ in 0..count {
        let stats = sim
            .process_ground_locomotor_for_test(id, Some(rules), Some(grid), None)
            .expect("locomotor");
        moved += stats.moved_steps;
    }
    moved
}

/// `Wreck=yes` turns the switch on and freezes the type.
#[test]
fn the_key_turns_kratos_on_and_freezes_the_type() {
    let (_, rules, _) = scene("Wreck=yes\n");
    assert!(rules.kratos());
    assert!(rules.object("WRCK").unwrap().kratos_freeze().frozen());

    let (_, plain, _) = scene("");
    assert!(!plain.kratos());
    assert!(!plain.object("WRCK").unwrap().kratos_freeze().frozen());
}

/// A wreck ordered six cells east runs no movement work and keeps its
/// coordinate; the same scene without the key walks.
#[test]
fn a_wreck_does_not_move() {
    let (mut sim, rules, id) = scene("Wreck=yes\n");
    let grid = crate::sim::arena_fixture::flat_arena(&mut sim, &rules);
    let before = position(&sim, id);
    order_move(&mut sim, id);
    assert_eq!(
        tick(&mut sim, &rules, &grid, id, 60),
        0,
        "no movement work runs"
    );
    assert_eq!(position(&sim, id), before, "the wreck keeps its coordinate");

    let (mut control, control_rules, control_id) = scene("");
    let control_grid = crate::sim::arena_fixture::flat_arena(&mut control, &control_rules);
    let start = position(&control, control_id);
    order_move(&mut control, control_id);
    assert!(
        tick(&mut control, &control_rules, &control_grid, control_id, 60) > 0,
        "the same type without the key walks"
    );
    assert_ne!(
        position(&control, control_id),
        start,
        "the control leaves its cell"
    );
}

/// Kratos' `0x00736A95` arm: a frozen `TurretSpins` type skips the spin,
/// and the same type without the key spins.
#[test]
fn a_wreck_does_not_spin_its_turret() {
    for (wreck_ini, expected) in [("Wreck=yes\n", false), ("", true)] {
        let rules = rules(&format!("TurretSpins=yes\n{wreck_ini}"));
        let mut sim = Simulation::with_seed(7);
        sim.intern_rule_type_ids(&rules);
        let mut entity = GameEntity::test_default(1, "WRCK", "Americans", 10, 10);
        entity.set_owner_for_test(sim.intern("Americans"));
        entity.set_type_ref_for_test(sim.intern("WRCK"));
        entity.set_category_for_test(crate::map::entities::EntityCategory::Unit);
        entity.barrel_facing = Some(FacingClass::new(0, 100));

        assert_eq!(
            facing_update(&entity, &sim, &rules, &sim.interner, 0).turret_spins,
            expected,
            "wreck_ini = {wreck_ini:?}"
        );
    }
}
