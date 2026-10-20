//! Steam gamemd.exe3e81a61775d2745d1dabe397325ef663cd994ffc194da4e998e3bf5d2d308600.
//! Original Execute88/89 controls: 6DF3F7/6DF49E, 68BCC0, 642740,
//! 41BEA0, 50E470 and virtual F8=6301E0. Creation controls declare type
//! arrays and observe Ground578080 / constructor62DC50 as argument sinks;
//! the Rust lifecycle checks below are not a full native scene comparison.
use super::*;
use crate::map::resolved_terrain::{test_flat_cell, test_grid};
use crate::map::waypoints::Waypoint;
use crate::rules::{ini_parser::IniFile, ruleset::RuleSet};
use crate::sim::particles::system_ai::tick_particle_system;
use crate::sim::snapshot::GameSnapshot;
use glam::IVec3;
use std::collections::HashMap;

fn action(kind: i32, value: i32) -> RuntimeAction {
    let ini = IniFile::from_str(&format!("[Actions]\nCASE=1,{kind},0,{value},0,0,0,0,A\n"));
    let entry = crate::map::actions::parse_actions(&ini)
        .remove("CASE")
        .unwrap()
        .entries()[0]
        .clone();
    RuntimeDefinitions::default().resolve_action(entry, None, None)
}

fn run(sim: &mut Simulation, action: &RuntimeAction, rules: Option<&RuleSet>) -> bool {
    sim.apply_live_trigger_action(
        action,
        TriggerActionContext {
            live_index: None,
            incoming_house: None,
            input: TagEventInput::default(),
        },
        &mut Vec::new(),
        rules,
        None,
        FrameEffects::default(),
    )
}

fn fixture_rules() -> RuleSet {
    RuleSet::from_ini(&IniFile::from_str("[ParticleSystems]\n0=Alpha\n1=Beta\n[Alpha]\nBehavesLike=Smoke\n[Beta]\nBehavesLike=Smoke\n")).unwrap()
}

fn world(cell: (i16, i16), level: u8, flags: u32) -> Simulation {
    let mut sim = Simulation::with_seed(0);
    sim.session.set_map_width_for_test(32);
    sim.session.set_map_height_for_test(32);
    let mut terrain = test_grid(32, 32, test_flat_cell);
    let at = terrain.native_cell_identity(cell);
    terrain.write_native_cell_level(at, level);
    terrain.write_native_cell_flags(at, flags);
    sim.install_resolved_terrain_for_new_map(terrain);
    waypoint(&mut sim, cell);
    sim
}

fn waypoint(sim: &mut Simulation, cell: (i16, i16)) {
    sim.session.install_waypoints(&HashMap::from([(
        0,
        Waypoint {
            index: 0,
            rx: cell.0 as u16,
            ry: cell.1 as u16,
        },
    )]));
}

fn rng(sim: &Simulation) -> [String; 3] {
    [
        sim.main_rng.native_state_hex(),
        sim.scenario_rng.native_state_hex(),
        sim.mapgen_rng.native_state_hex(),
    ]
}

#[test]
fn original_creation_controls_preserve_signed_center_registry_index_and_ground_only_arguments() {
    // Ten original calls: these five centers for each of two registry types.
    // The supplied native Ground results are explicit fixture inputs.
    for (cell, height, expected) in [
        ((12i16, 17i16), 208, [3200, 4480, 208]),
        ((0, 0), 0, [128, 128, 0]),
        ((-1, 2), -17, [-128, 640, -17]),
        ((32767, -32768), 208, [8388480, -8388480, 208]),
        ((-32768, 32767), -17, [-8388480, 8388480, -17]),
    ] {
        let actual = crate::sim::cell_kernel::cell_center(
            crate::sim::cell_kernel::CellCoordinate {
                x: i32::from(cell.0),
                y: i32::from(cell.1),
            },
            height,
        );
        assert_eq!([actual.x, actual.y, actual.z], expected);
    }
    let rules = fixture_rules();
    for index in [0, 1] {
        // A real flat level2 cell plus both raw bridge flags. The action's
        // Ground call stays at208; camera/animation bridge adjustment is separate.
        let mut sim = world((12, 17), 2, 0x500);
        let before = rng(&sim);
        let frame = sim.session.binary_frame();
        assert!(run(&mut sim, &action(88, index), Some(&rules)));
        let systems: Vec<_> = sim
            .particle_systems()
            .iter()
            .map(|(_, system)| system)
            .collect();
        assert_eq!(systems.len(), 1);
        let system = systems[0];
        assert_eq!(system.type_id.0, index as u32);
        assert_eq!(system.coords, IVec3::new(3200, 4480, 208));
        assert_eq!(system.target_coords, IVec3::ZERO);
        assert_eq!(system.offset, IVec3::ZERO);
        assert_eq!(
            (system.target_ref, system.owner_entity, system.owner_house),
            (None, None, None)
        );
        assert!(system.alive() && !system.done_spawning);
        assert!(system.particles.is_empty());
        assert_eq!(rng(&sim), before);
        assert_eq!(sim.session.binary_frame(), frame);
    }
    let mut sim = world((12, 17), 0, 0);
    assert!(!run(&mut sim, &action(88, -1), Some(&rules)));
    assert!(!run(&mut sim, &action(88, 2), Some(&rules)));
    assert!(sim.particle_systems().is_empty());
}

#[test]
fn all_five_original_stop_controls_only_mark_alive_same_cell_systems() {
    let rules = fixture_rules();
    let priors = [
        (true, [3200, 4480, 0], false),
        (true, [3073, 4607, 1], false),
        (true, [3328, 4352, 0], false),
        (false, [3200, 4480, 0], false),
        (true, [3200, 4480, 0], true),
        (true, [-256, 512, 7], false),
        (true, [-255, -255, 7], false),
    ];
    for (cell, count, expected) in [
        ((12, 17), 7, [true, true, false, false, true, false, false]),
        ((0, 0), 7, [false, false, false, false, true, false, true]),
        ((-1, 2), 7, [false, false, false, false, true, true, false]),
        ((0, 5), 7, [false, false, false, false, true, false, false]),
        (
            (12, 17),
            0,
            [false, false, false, false, true, false, false],
        ),
    ] {
        let mut sim = Simulation::with_seed(0);
        waypoint(&mut sim, cell);
        let mut ids = Vec::new();
        for (index, &(alive, position, done)) in priors.iter().take(count).enumerate() {
            let id = sim
                .spawn_particle_system(
                    crate::rules::particle_system_type::ParticleSystemTypeId((index % 2) as u32),
                    IVec3::from_array(position),
                    None,
                    None,
                    IVec3::ZERO,
                    None,
                    &rules,
                )
                .unwrap();
            if !alive {
                sim.particle_systems_mut().enter_limbo(id);
            }
            if done {
                sim.particle_systems_mut()
                    .get_mut(id)
                    .unwrap()
                    .mark_done_spawning();
            }
            ids.push(id);
        }
        let before = rng(&sim);
        // The scalar is deliberately invalid as a type ID;89 ignores it and
        // needs neither Rules nor an actor/House/terrain.
        assert!(run(&mut sim, &action(89, i32::MAX), None));
        assert_eq!(sim.particle_systems().len(), count);
        for (index, id) in ids.into_iter().enumerate() {
            let system = sim.particle_systems().get(id).unwrap();
            assert_eq!(system.done_spawning, expected[index], "{cell:?}/{index}");
            assert_eq!(system.alive(), priors[index].0);
            assert_eq!(system.coords, IVec3::from_array(priors[index].1));
        }
        assert_eq!(rng(&sim), before);
        assert_eq!(sim.session.binary_frame(), 0);
    }
}

#[test]
fn retail_smoke_stop_keeps_existing_children_and_saved_flag_until_their_shared_retirement() {
    let Some(rules) = crate::rules::retail_ini_fixture::retail_unlayered_rules() else {
        return;
    };
    let type_id = rules.ps_type_id_by_name("BigGreySmokeSys").unwrap();
    assert_eq!(
        rules.particle_system_type(type_id).behaves_like,
        crate::rules::particle_system_type::ParticleSystemBehavesLike::Smoke
    );
    let mut sim = world((12, 17), 0, 0);
    assert!(run(&mut sim, &action(88, type_id.0 as i32), Some(&rules)));
    let id = *sim.particle_systems().iter().next().unwrap().0;
    assert!(tick_particle_system(
        &mut sim,
        &rules,
        id,
        crate::sim::world::UninitContext::with_rules(&rules)
    ));
    let system = sim.particle_systems().get(id).unwrap();
    assert!(!system.done_spawning);
    assert!(!system.particles.is_empty());
    let children = serde_json::to_value(&system.particles).unwrap();
    let before = rng(&sim);
    assert!(run(&mut sim, &action(89, -1), None));
    let system = sim.particle_systems().get(id).unwrap();
    assert!(system.alive() && system.done_spawning);
    assert_eq!(serde_json::to_value(&system.particles).unwrap(), children);
    assert_eq!(rng(&sim), before);
    let serialized = serde_json::to_value(system).unwrap();
    let bytes = GameSnapshot::save(&sim, 0, 0, "Stopped smoke", 0);
    let mut restored = GameSnapshot::load(&bytes).unwrap().sim;
    restored.restore_after_snapshot_load().unwrap();
    assert_eq!(
        serde_json::to_value(restored.particle_systems().get(id).unwrap()).unwrap(),
        serialized
    );
    // The existing loader reseeds Scenario. Supply the same continuation
    // RNG boundary as the established Smoke snapshot test; do not claim a
    // whole-load RNG parity result or change that independent load policy.
    restored.scenario_rng = crate::sim::rng::SimRng::from_native_state_hex_for_test(
        &sim.scenario_rng.native_state_hex(),
    );
    let mut retired = false;
    for frame in 1..=1024 {
        for world in [&mut sim, &mut restored] {
            world.session.set_binary_frame_for_test(frame);
            tick_particle_system(
                world,
                &rules,
                id,
                crate::sim::world::UninitContext::with_rules(&rules),
            );
            world.process_pending_delete();
        }
        assert_eq!(
            serde_json::to_value(sim.particle_systems().get(id)).unwrap(),
            serde_json::to_value(restored.particle_systems().get(id)).unwrap()
        );
        assert_eq!(rng(&sim), rng(&restored));
        if sim.particle_systems().is_empty() {
            retired = true;
            break;
        }
    }
    assert!(
        retired,
        "retail Smoke children must reach shared done-and-empty retirement"
    );
}
