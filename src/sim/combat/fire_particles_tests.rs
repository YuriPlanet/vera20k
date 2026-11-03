//! FireAt's Fire arm (`0x006FF15F..0x006FF1A7`) and the `TechnoClass+0x304`
//! link through production.

use crate::rules::ini_parser::IniFile;
use crate::rules::particle_system_type::ParticleSystemBehavesLike;
use crate::rules::retail_ini_fixture::RetailBattleRules;
use crate::rules::ruleset::RuleSet;
use crate::sim::command::Command;
use crate::sim::vision::FogState;
use crate::sim::world::Simulation;

/// Retail arms no type with `FireballLauncher`, so a map that does allocates
/// the weapon in its own pass and must carry the section. This layer copies
/// retail `[FireballLauncher]` and arms the Grizzly.
fn flame_rules() -> Option<RetailBattleRules> {
    crate::rules::retail_ini_fixture::retail_battle_rules_with_map_patch(&IniFile::from_str(
        "[MTNK]\nPrimary=FireballLauncher\n\
         [FireballLauncher]\nDamage=0\nAmbientDamage=2\nROF=50\nRange=4.25\n\
         Projectile=Invisible\nSpeed=1\nWarhead=Fire\nReport=FLAMTNK1\n\
         UseFireParticles=yes\nAttachedParticleSystem=FireStreamSys\nBurst=2\n",
    ))
}

/// A flame Grizzly ordered to attack a Russian Flak Cannon three cells east:
/// a target that neither moves nor shoots back.
fn flame_attack(rules: &RuleSet) -> (Simulation, u64, u64) {
    let mut sim = crate::sim::arena_fixture::rival_arena(rules, 0xF1A3_E000);
    sim.fog = FogState::new(32, 32);
    let owner = sim.interner.intern("Americans");
    sim.session.set_current_house_for_test(Some(owner));
    sim.intern_rule_type_ids(rules);
    sim.resolve_type_handles(rules);
    let source = sim
        .spawn_object("MTNK", "Americans", 16, 16, 64, rules)
        .unwrap();
    let target = sim
        .spawn_object("NAFLAK", "Russians", 19, 16, 0, rules)
        .unwrap();
    sim.fog.mark_visible_for_owner(owner, 19, 16);
    assert!(sim.apply_command(
        "Americans",
        &Command::Attack {
            attacker_id: source,
            target_id: target
        },
        Some(rules),
    ));
    (sim, source, target)
}

fn fire_link(sim: &Simulation, id: u64) -> Option<u64> {
    sim.substrate
        .entities
        .get(id)
        .unwrap()
        .fire_particle_system
        .system()
}

/// The shot links a Fire system as `+0x304` aimed at the target, refuses the
/// burst's second shot while it is held (`0x006FDE0E`), and its particles,
/// not the 0-damage bullet, hurt the target before the system expires and
/// clears the link.
#[test]
fn a_flame_shot_holds_its_fire_system_whose_particles_burn_the_target() {
    let Some(patched) = flame_rules() else {
        return;
    };
    let rules = &patched.rules;
    assert!(rules.weapon("FireballLauncher").unwrap().use_fire_particles);
    let (mut sim, source, target) = flame_attack(rules);
    let grid = sim.path_grid_snapshot().unwrap();
    let full = sim.substrate.entities.get(target).unwrap().health.current();
    let fired = (0..64).find(|_| {
        sim.advance_tick(&[], Some(rules), Some(&grid), None, 67);
        sim.fire_events
            .iter()
            .any(|shot| shot.attacker_id == source)
    });
    assert!(fired.is_some(), "the flame Grizzly did not fire");
    let system = fire_link(&sim, source).expect("FireAt links the Fire system");
    let held = sim.particle_systems().get(system).unwrap();
    assert_eq!(
        rules.particle_system_type(held.type_id()).behaves_like,
        ParticleSystemBehavesLike::Fire
    );
    let aim = super::world_fire::object_get_coords(&sim, target).unwrap();
    assert_eq!(held.target_coords().to_array(), [aim.x, aim.y, aim.z]);
    assert_eq!(
        sim.substrate.entities.get(target).unwrap().health.current(),
        full,
        "the bullet itself does no damage"
    );

    let mut burned = false;
    while fire_link(&sim, source) == Some(system) {
        sim.fire_events.clear_for_test();
        sim.advance_tick(&[], Some(rules), Some(&grid), None, 67);
        assert!(
            !sim.fire_events
                .iter()
                .any(|shot| shot.attacker_id == source),
            "a second shot while +0x304 is live"
        );
        burned |= sim
            .substrate
            .entities
            .get(target)
            .is_none_or(|flak| flak.health.current() < full);
        assert!(
            sim.session.binary_frame() < 400,
            "the Fire system never expired"
        );
    }
    assert!(burned, "the Fire particles never damaged the target");
    assert!(sim.particle_systems().get(system).is_none());
}

/// A move order clears the target first, and Assign_Target(NULL) releases
/// `+0x304` (`0x006FCF61`): the system stops spawning at its next AI but
/// keeps its particles until they expire.
#[test]
fn a_move_order_releases_the_fire_system_link() {
    let Some(patched) = flame_rules() else {
        return;
    };
    let rules = &patched.rules;
    let (mut sim, source, _) = flame_attack(rules);
    let grid = sim.path_grid_snapshot().unwrap();
    let system = (0..64)
        .find_map(|_| {
            sim.advance_tick(&[], Some(rules), Some(&grid), None, 67);
            fire_link(&sim, source)
        })
        .expect("FireAt links the Fire system");
    let spawned = (0..16).any(|_| {
        sim.advance_tick(&[], Some(rules), Some(&grid), None, 67);
        !sim.particle_systems()
            .get(system)
            .unwrap()
            .particles()
            .is_empty()
    });
    assert!(spawned, "the Fire system spawns particles");
    assert!(!sim.particle_systems().get(system).unwrap().done_spawning());
    assert!(sim.apply_command(
        "Americans",
        &Command::Move {
            entity_id: source,
            target_rx: 10,
            target_ry: 10
        },
        Some(rules),
    ));
    assert_eq!(fire_link(&sim, source), None);
    sim.advance_tick(&[], Some(rules), Some(&grid), None, 67);
    let released = sim.particle_systems().get(system).unwrap();
    assert!(released.done_spawning());
    assert!(!released.particles().is_empty());
}
