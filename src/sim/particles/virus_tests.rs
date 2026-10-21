//! The stock Virus kill must reach its two different gas producers (#1282).

use crate::rules::ruleset::RuleSet;
use crate::sim::combat::{EntityDamageEvent, RAD_NO_ATTACKER, ReceiverCallFlags};
use crate::sim::native_identity::NativeUniqueIdCursor;
use crate::sim::world::{FrameEffects, Simulation};

fn retail_rules() -> Option<RuleSet> {
    let retail = crate::rules::retail_ini_fixture::retail_battle_rules()?;
    let mut rules = retail.rules;
    let bytes = crate::rules::retail_ini_fixture::retail_archive_bytes("VIRUSD.SHP")?;
    let frames = crate::assets::shp_file::ShpFile::frame_count_from_bytes(&bytes).unwrap();
    let mut art = crate::rules::retail_ini_fixture::art_registry(&retail.fixed_art);
    art.bind_anim_frame_count_for_test("VIRUSD", i32::from(frames));
    rules.install_art_data(art);
    Some(rules)
}

fn check_fatal_virus_shot(
    victim_type: &str,
    expected_particles: usize,
) -> Option<(RuleSet, Simulation)> {
    let rules = retail_rules()?;
    let mut sim = crate::sim::arena_fixture::rival_arena(&rules, 7);
    sim.native_unique_ids = Some(NativeUniqueIdCursor::test_at_current_value(1000));
    sim.construct_post_load_particle_system_id(&rules);
    let victim = sim
        .spawn_object(victim_type, "Americans", 12, 12, 64, &rules)
        .unwrap();
    let weapon = rules.weapon("Virusgun").unwrap();
    let warhead = sim.interner.intern(weapon.warhead.as_deref().unwrap());
    let hit = EntityDamageEvent::direct_receiver(
        victim,
        weapon.damage,
        0,
        RAD_NO_ATTACKER,
        None,
        warhead,
        ReceiverCallFlags {
            ignore_defenses: false,
            arg6: false,
        },
    );
    sim.commit_noncombat_aoe_hits(&rules, None, &[hit], FrameEffects::default());
    let gas = rules.p_type_id_by_name("VirusCloud1").unwrap();
    let count = |sim: &Simulation| {
        sim.particle_systems()
            .iter()
            .flat_map(|(_, system)| &system.particles)
            .filter(|p| p.type_id == gas)
            .count()
    };
    let grid = sim.path_grid_snapshot().unwrap();
    for _ in 0..400 {
        if count(&sim) != 0 {
            break;
        }
        sim.advance_tick(&[], Some(&rules), Some(&grid), None, 67);
    }
    assert_eq!(
        count(&sim),
        expected_particles,
        "fatal stock Virus shot against {victim_type}"
    );
    Some((rules, sim))
}

#[test]
fn retail_virus_human_death_emits_three_middle_particles() {
    let _ = check_fatal_virus_shot("E1", 3);
}

#[test]
fn retail_virus_animal_death_emits_one_inline_particle() {
    let _ = check_fatal_virus_shot("ADOG", 1);
}

#[test]
fn retail_gas_harms_both_houses_but_respects_virus_poison_immunity() {
    for (owner, kind, immune) in [
        ("Americans", "E1", false),
        ("Russians", "E1", false),
        ("Russians", "VIRUS", true),
    ] {
        let Some((rules, mut sim)) = check_fatal_virus_shot("E1", 3) else {
            return;
        };
        let bystander = sim.spawn_object(kind, owner, 12, 12, 64, &rules).unwrap();
        let before = sim.entities().get(bystander).unwrap().health.current();
        // Advance only production particle dispatch: the bystander remains
        // in the cloud and cannot acquire damage from ordinary combat AI.
        for _ in 0..30 {
            sim.session
                .set_binary_frame_for_test(sim.session.binary_frame() + 1);
            super::system_ai::tick_particle_systems(&mut sim, &rules);
        }
        let after = sim.entities().get(bystander).unwrap().health.current();
        if immune {
            assert_eq!(after, before, "VIRUS is ImmuneToPoison");
        } else {
            assert!(after < before, "{owner} bystander must take poison damage");
        }
    }
}

#[test]
fn gas_damage_can_append_animal_death_particle_to_the_ticking_system() {
    let Some(rules) = retail_rules() else { return };
    let mut sim = crate::sim::arena_fixture::rival_arena(&rules, 7);
    sim.native_unique_ids = Some(NativeUniqueIdCursor::test_at_current_value(1000));
    sim.construct_post_load_particle_system_id(&rules);
    let victim = sim
        .spawn_object("ADOG", "Americans", 12, 12, 64, &rules)
        .unwrap();
    // Explicit wounded-victim prior; the lethal pulse still traverses the
    // ordinary poison receiver and the NotHuman temporary-animation branch.
    sim.substrate
        .entities
        .get_mut(victim)
        .unwrap()
        .health
        .set_for_test(1);
    let c = crate::sim::movement::ground_pose::position_world_coord(
        &sim.entities().get(victim).unwrap().position,
    );
    let kind = rules.p_type_id_by_name("VirusCloud1").unwrap();
    sim.spawn_gas_particle(kind, glam::IVec3::new(c.x, c.y, c.z), &rules)
        .unwrap();
    let system = sim.particle_systems().gas_cloud_system().unwrap();
    sim.substrate
        .particle_systems
        .get_mut(system)
        .unwrap()
        .particles[0]
        .damage_counter = 1;
    sim.session.set_binary_frame_for_test(1);
    super::system_ai::tick_particle_systems(&mut sim, &rules);
    let particles = &sim.particle_systems().get(system).unwrap().particles;
    assert_eq!(
        particles.len(),
        2,
        "synchronous death must append to the same live owner"
    );
    assert_eq!(
        particles[1].damage_counter,
        rules.particle_type(kind).max_dc as i16,
        "new birth joins Move but not this visit's AI pass"
    );
    assert!(
        sim.entities()
            .get(victim)
            .is_none_or(|e| e.health.current() <= 1)
    );
}

#[test]
fn retail_gas_snapshot_retains_global_owner_motion_and_child_cleanup() {
    use crate::sim::snapshot::GameSnapshot;
    let Some(rules) = retail_rules() else { return };
    let mut sim = crate::sim::arena_fixture::rival_arena(&rules, 7);
    sim.native_unique_ids = Some(NativeUniqueIdCursor::test_at_current_value(1000));
    sim.construct_post_load_particle_system_id(&rules);
    let kind = rules.p_type_id_by_name("VirusCloud1").unwrap();
    sim.spawn_gas_particle(kind, glam::IVec3::new(3200, 3200, 0), &rules)
        .unwrap();
    let system = sim.particle_systems().gas_cloud_system().unwrap();
    // Explicit near-expiry boundary exercises both authored particle types
    // without a thousand-frame save test. Native expiry controls pin ordering.
    sim.substrate
        .particle_systems
        .get_mut(system)
        .unwrap()
        .particles[0]
        .lifetime_remaining = 2;
    sim.session.set_binary_frame_for_test(36);
    super::system_ai::tick_particle_systems(&mut sim, &rules);
    let hash_with_owner = sim.state_hash();
    sim.substrate.particle_systems.gas_cloud_system = None;
    assert_ne!(
        sim.state_hash(),
        hash_with_owner,
        "global reference reaches the state hash"
    );
    sim.substrate.particle_systems.gas_cloud_system = Some(system);
    sim.session
        .set_map_name_for_test("virus-cloud-continuation".into());
    let bytes = GameSnapshot::save_validated(&sim, 17, 23, "Virus continuation", 0);
    let mut restored = GameSnapshot::load_validated(&bytes, 17, 23, "virus-cloud-continuation")
        .unwrap()
        .sim;
    restored.restore_after_snapshot_load().unwrap();
    // Resolved terrain is an asset-derived cache, reinstalled by the normal
    // app loader after snapshot restoration. Supply the same flat arena here.
    restored.rebuild_caches_after_load(
        sim.resolved_terrain.clone().unwrap(),
        Default::default(),
        &rules,
    );
    assert_eq!(restored.particle_systems().gas_cloud_system(), Some(system));
    assert!(!restored.construct_post_load_particle_system_id(&rules));
    // The existing native-style load restarts Scenario RNG; align this test's
    // continuation cursor so only the new saved particle state is compared.
    restored.scenario_rng = crate::sim::rng::SimRng::from_native_state_hex_for_test(
        &sim.scenario_rng.native_state_hex(),
    );
    assert_eq!(
        serde_json::to_value(restored.particle_systems()).unwrap(),
        serde_json::to_value(sim.particle_systems()).unwrap()
    );
    for frame in 37..=180 {
        for world in [&mut sim, &mut restored] {
            world.session.set_binary_frame_for_test(frame);
            super::system_ai::tick_particle_systems(world, &rules);
            world.process_pending_delete_with(
                &rules,
                None,
                FrameEffects::default(),
                1,
                world.session.started(),
            );
        }
        assert_eq!(
            serde_json::to_value(restored.particle_systems()).unwrap(),
            serde_json::to_value(sim.particle_systems()).unwrap()
        );
    }
    assert!(
        sim.particle_systems()
            .get(system)
            .unwrap()
            .particles
            .is_empty(),
        "VirusCloud1 must become VirusCloudD1 and eventually leave Display/store"
    );
    assert_eq!(
        sim.scenario_rng.native_state_hex(),
        restored.scenario_rng.native_state_hex()
    );
}
