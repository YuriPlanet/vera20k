//! Ares 3.0 ParticleTypeExt `DamageRange` and AnimTypeExt
//! `SpawnsParticle.Range*` on the shared Gas system.

use crate::rules::ini_parser::IniFile;
use crate::rules::ruleset::RuleSet;
use crate::sim::anim_class::AnimWorldCoord;
use crate::sim::components::AnimClassSpawnDescriptor;
use crate::sim::world::Simulation;
use glam::IVec3;

fn rules(gas: &str) -> RuleSet {
    let ini = crate::sim::arena_fixture::clear_ground_ini(&format!(
        "[Countries]\n0=Americans\n1=Russians\n[VehicleTypes]\n0=TARGET\n\
         [TARGET]\nStrength=1000\nArmor=light\n\
         [Animations]\n0=SPAWNER\n\
         [ParticleSystems]\n0=GasCloudSys\n[GasCloudSys]\nBehavesLike=Gas\n\
         [Particles]\n0=GASP\n\
         [GASP]\nBehavesLike=Gas\nMaxEC=100\nMaxDC=1\nDamage=5\nWarhead=WGAS\n{gas}\
         [Warheads]\n0=WGAS\n\
         [WGAS]\nCellSpread=2\nPercentAtMax=1\n\
         Verses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n"
    ));
    let art = IniFile::from_str(
        "[SPAWNER]\nRate=900\nEnd=8\nSpawnsParticle=GASP\nNumParticles=3\n\
         SpawnsParticle.RangeMinimum=2\nSpawnsParticle.RangeMaximum=2\n",
    );
    let processed = crate::rules::native_processing::RulesLayerStack::new(ini)
        .process_with_fixed_art(&art)
        .unwrap();
    let mut rules = RuleSet::from_processed_rules(&processed).unwrap();
    let mut registry = crate::rules::art_data::ArtRegistry::from_ini(&art);
    registry.bind_anim_frame_count_for_test("SPAWNER", 8);
    rules.install_art_data(registry);
    rules
}

fn world(rules: &RuleSet) -> Simulation {
    let mut sim = crate::sim::arena_fixture::rival_arena(rules, 0x62C23D);
    sim.intern_rule_type_ids(rules);
    sim.resolve_type_handles(rules);
    sim
}

fn gas_particles(sim: &Simulation) -> Vec<IVec3> {
    let system = sim.particle_systems().gas_cloud_system().unwrap();
    let system = sim.particle_systems().get(system).unwrap();
    system.particles.iter().map(|p| p.coords()).collect()
}

/// ParticleClass_Update_Gas_DamageRange (100297C0, hook 0x62C23D): with
/// `DamageRange=1.5` the gas damages an object on the next cell, where the
/// original loop reaches only the particle's own cell.
#[test]
fn ares_gas_damage_range_reaches_the_next_cell() {
    let _ares = crate::rules::ares_switch::read_ares_way(true);
    let rules = rules("DamageRange=1.5\n");
    let mut sim = world(&rules);
    let target = sim
        .spawn_object("TARGET", "Russians", 13, 12, 0, &rules)
        .unwrap();
    let kind = rules.p_type_id_by_name("GASP").unwrap();
    sim.spawn_gas_particle(kind, IVec3::new(12 * 256 + 128, 12 * 256 + 128, 0), &rules)
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
    assert!(sim.entities().get(target).unwrap().health.current() < 1000);
}

/// AnimClass_Expired_SpawnsParticle (1000AE40, hook 0x425002): with both
/// ranges at 2 cells, each SpawnsParticle particle lands 512 leptons from
/// the anim, where the original puts every one on the anim.
#[test]
fn ares_spawns_particle_range_spreads_the_particles() {
    let _ares = crate::rules::ares_switch::read_ares_way(true);
    let rules = rules("");
    let mut sim = world(&rules);
    let center = IVec3::new(20 * 256 + 128, 20 * 256 + 128, 0);
    let world = AnimWorldCoord {
        x: center.x,
        y: center.y,
        z: center.z,
    };
    let (rx, ry, sx, sy, z) = world.to_cell_sub_z();
    let kind = sim.interner.intern("SPAWNER");
    sim.spawn_anim_at_world(
        &rules,
        AnimClassSpawnDescriptor::new(kind, rx, ry, sx, sy, z),
        world,
    )
    .unwrap();
    for _ in 0..8 {
        if sim
            .particle_systems()
            .gas_cloud_system()
            .is_some_and(|_| !gas_particles(&sim).is_empty())
        {
            break;
        }
        sim.advance_tick(&[], Some(&rules), None, Some(rules.overlay_registry()), 22);
    }
    let particles = gas_particles(&sim);
    assert_eq!(particles.len(), 3);
    for at in particles {
        let d = (at - center).truncate().as_vec2().length();
        // The Gas AI may already have drifted them a little.
        assert!((448.0..=576.0).contains(&d), "{at} is {d} from {center}");
    }
}
