//! Regression coverage of the existing Rust consequence schedules, not native parity.

use super::*;
use crate::rules::{art_data::ArtRegistry, ini_parser::IniFile};
use crate::sim::command::Command;
use crate::sim::game_entity::GameEntity;
use crate::sim::projectile::{ProjectileCoord, ProjectilePayload, ProjectileTarget};

fn consequence_rules() -> RuleSet {
    let mut rules = RuleSet::from_ini(&IniFile::from_str(
        "[VehicleTypes]\n0=TESLA\n1=TARGET\n\
         [TESLA]\nStrength=300\nArmor=heavy\nSpeed=6\nPrimary=CoilBolt\n\
         [TARGET]\nStrength=10\nArmor=heavy\nSpeed=6\nDieSound=DieA\n\
         MinDebris=1\nMaxDebris=2\nDebrisTypes=TIRE\nDebrisMaximums=1\n\
         Explosion=DEATHFX\nDestroyAnim=DESTROYFX\n\
         [VoxelAnims]\n0=TIRE\n\
         [TIRE]\nElasticity=0.8\nMinAngularVelocity=12\nMaxAngularVelocity=24\n\
         MinZVel=28\nMaxZVel=32\nMaxXYVel=10\nDuration=150\n\
         [CoilBolt]\nDamage=40\nROF=50\nRange=6\nWarhead=AP\nIsElectricBolt=yes\n\
         [Warheads]\n0=AP\n\
         [AP]\nCellSpread=0\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n\
         [CombatDamage]\nDefaultSparkSystem=SparkSys\n\
         [ParticleSystems]\n0=SparkSys\n\
         [SparkSys]\nBehavesLike=Spark\nHoldsWhat=Spark\nParticleCap=6\n\
         SparkSpawnFrames=1\nSpawnSparkPercentage=1\nLifetime=200\n\
         [Particles]\n0=Spark\n\
         [Spark]\nBehavesLike=Spark\nMaxEC=500\nXVelocity=10\nYVelocity=10\n\
         MinZVelocity=40\nZVelocityRange=15\n",
    ))
    .unwrap();
    let mut art = ArtRegistry::from_ini(&IniFile::from_str(
        "[DEATHFX]\nReport=DeathReport\nEnd=80\n\
         [DESTROYFX]\nReport=DestroyReport\nEnd=80\n",
    ));
    for name in ["DEATHFX", "DESTROYFX"] {
        art.bind_anim_frame_count_for_test(name, 80);
    }
    rules.replace_art_registry_for_test(art);
    rules
}

fn consequence_world() -> (Simulation, u64, u64) {
    let mut sim = Simulation::with_seed(17);
    sim.input_delay_ticks = 0;
    let attacker = sim.substrate.allocate_stable_id();
    let victim = sim.substrate.allocate_stable_id();
    for (id, kind, owner, rx, hp) in [
        (attacker, "TESLA", "Americans", 5, 300),
        (victim, "TARGET", "Soviet", 8, 10),
    ] {
        let mut entity = GameEntity::test_default(id, kind, owner, rx, 5);
        entity.health.set_for_test(hp);
        sim.substrate.entities.insert(entity);
    }
    sim.interner = crate::sim::intern::test_interner();
    for id in [attacker, victim] {
        assert!(matches!(sim.reveal(id), RevealOutcome::Revealed { .. }));
    }
    (sim, attacker, victim)
}

fn delivered_ids(sim: &Simulation) -> Vec<u64> {
    let debris = sim.substrate.voxel_anims.ids();
    assert_eq!(debris.len(), 1);
    let mut ids = debris;
    for name in ["DEATHFX", "DESTROYFX"] {
        ids.push(
            sim.anims()
                .find(|(_, anim)| sim.interner.resolve(anim.type_id()) == name)
                .unwrap_or_else(|| panic!("missing {name}"))
                .0
                .to_owned(),
        );
    }
    assert!(ids.windows(2).all(|pair| pair[0] + 1 == pair[1]));
    let live = sim.live_object_order_snapshot();
    let selected: Vec<_> = live.into_iter().filter(|id| ids.contains(id)).collect();
    assert_eq!(selected, ids);
    sim.debug_assert_logic_membership_consistent();
    ids
}

/// The kill lands in the bullet's tail slot, whose commit admits the death
/// anims (their `Report=`) before it queues the die sound. RESIDUAL: the
/// order of same-frame sound starts is not taken from gamemd; inaudible.
fn delivery_sounds(sim: &Simulation) -> Vec<String> {
    sim.sound_events
        .iter()
        .filter_map(|event| match event {
            SimSoundEvent::EntityDied { die_sound_id, .. } => Some(die_sound_id.clone()),
            SimSoundEvent::AnimationStarted { sound_id, .. } => Some(sound_id.clone()),
            _ => None,
        })
        .collect()
}

fn advance_lethal_shot(sim: &mut Simulation, rules: &RuleSet, attacker: u64, victim: u64) {
    let owner = sim.interner.intern("Americans");
    let grid = PathGrid::test_all_passable(64, 64);
    sim.queue_command(CommandEnvelope::new(
        owner,
        sim.session.tick() + 1,
        Command::Attack {
            attacker_id: attacker,
            target_id: victim,
        },
    ));
    for _ in 0..60 {
        let commands = sim.take_due_commands();
        sim.advance_tick(&commands, Some(rules), Some(&grid), None, 100);
        if !sim.substrate.voxel_anims.is_empty() {
            return;
        }
    }
    panic!("the command must produce a lethal shot");
}

#[test]
fn ordinary_lethal_fire_commits_debris_animations_and_sparks_once() {
    let rules = consequence_rules();
    let (mut sim, attacker, victim) = consequence_world();
    let grid = PathGrid::test_all_passable(64, 64);
    advance_lethal_shot(&mut sim, &rules, attacker, victim);
    let mut ids = delivered_ids(&sim);
    let spark = *sim
        .particle_systems()
        .iter()
        .next()
        .expect("FireAt constructs its spark before the Bullet visit")
        .0;
    assert_eq!(spark + 1, ids[0]);
    ids.push(spark);
    assert_eq!(
        sim.live_object_order_snapshot(),
        [vec![attacker, spark], ids[..3].to_vec()].concat()
    );
    assert_eq!(
        delivery_sounds(&sim),
        ["DEATHREPORT", "DESTROYREPORT", "DieA"]
    );
    assert!(sim.substrate.entities.get(victim).is_none());
    // Existing Rust stream regressions remain unchanged; native numeric body
    // comparisons live with the constructor owner rather than a serialized
    // Rust fingerprint that changes whenever its wire layout gains a field.
    assert_eq!(sim.scenario_rng.state(), 3954386809370758752);
    assert_eq!(sim.main_rng.state(), 8272706873942388551);
    let debris_native = sim.voxel_anims().get(ids[0]).unwrap().native_unique_id();
    let death_native = sim
        .anims()
        .find(|(id, _)| **id == ids[1])
        .unwrap()
        .1
        .native_unique_id();
    let destroy_native = sim
        .anims()
        .find(|(id, _)| **id == ids[2])
        .unwrap()
        .1
        .native_unique_id();
    assert_eq!(death_native, debris_native.wrapping_add(1));
    assert_eq!(destroy_native, death_native.wrapping_add(1));
    let next_id = sim.substrate.allocate_stable_id();
    assert_eq!(next_id, ids[2] + 1);
    sim.advance_tick(&[], Some(&rules), Some(&grid), None, 100);
    assert_eq!(delivered_ids(&sim), ids[..3]);
    assert_eq!(
        delivery_sounds(&sim),
        ["DEATHREPORT", "DESTROYREPORT", "DieA"]
    );
    assert_eq!(sim.particle_systems().len(), 1);
}

#[test]
fn ordinary_fatal_transport_finishes_cargo_lifecycle_before_consequence_admission() {
    use crate::sim::passenger::{PassengerCargo, PassengerRole};
    let rules = consequence_rules();
    let (mut sim, attacker, carrier) = consequence_world();
    let passenger = sim.substrate.allocate_stable_id();
    let mut entity = GameEntity::test_default(passenger, "TARGET", "Soviet", 8, 5);
    entity.passenger_role = PassengerRole::inside_for_test(carrier, false);
    sim.substrate.entities.insert(entity);
    let mut cargo = PassengerCargo::new(1, 1.0);
    cargo.board_forced_chain(&[(passenger, 1.0)]);
    sim.substrate
        .entities
        .get_mut(carrier)
        .unwrap()
        .passenger_role = PassengerRole::transport(cargo);
    sim.lifecycle_test_events.clear();
    advance_lethal_shot(&mut sim, &rules, attacker, carrier);
    assert!(sim.substrate.entities.get(passenger).is_none());
    assert!(sim.substrate.entities.get(carrier).is_none());
    assert!(sim.substrate.pending_delete.is_empty());
    let cleared: Vec<_> = sim
        .lifecycle_test_events
        .iter()
        .filter_map(|event| match event {
            LifecycleTestEvent::UninitAliveCleared { stable_id } => Some(*stable_id),
            _ => None,
        })
        .collect();
    assert_eq!(cleared, vec![passenger, carrier]);
    // `passenger + 1` is the shot's Bullet, then FireAt admits its spark
    // before that Bullet's later live AI constructs the death consequences.
    assert_eq!(
        delivered_ids(&sim),
        vec![passenger + 3, passenger + 4, passenger + 5]
    );
    assert_eq!(
        delivery_sounds(&sim),
        ["DEATHREPORT", "DESTROYREPORT", "DieA"]
    );
}

#[test]
fn immediate_bullet_commits_before_return_with_its_original_sound_order() {
    let rules = consequence_rules();
    let (mut sim, attacker, victim) = consequence_world();
    let projectile = sim.substrate.allocate_stable_id();
    let impact = ProjectileCoord::new(8 * 256 + 128, 5 * 256 + 128, 0);
    let mut spawn = super::lifecycle_tests::gsi_05_02_projectile(attacker, Some(0));
    spawn.origin = impact;
    spawn.target = ProjectileTarget::Entity(victim);
    spawn.initial_target_position = impact;
    spawn.payload = ProjectilePayload::new(40, sim.interner.intern("AP"), None, false);
    sim.admit_projectile(projectile, spawn);
    // Supply the already-arrived Bullet AI boundary: this consequence-order
    // fixture detonates at the receiver, after native launch placement.
    sim.projectiles
        .get_mut(projectile)
        .unwrap()
        .set_position_for_test(impact);
    assert!(sim.object_ai_visit_one(projectile, Some(&rules), ObjectAiCtx::default()));
    let ids = delivered_ids(&sim);
    assert_eq!(ids, vec![projectile + 1, projectile + 2, projectile + 3]);
    assert_eq!(
        sim.live_object_order_snapshot(),
        [vec![attacker], ids.clone()].concat()
    );
    assert_eq!(
        sim.substrate.pending_delete.object_ids(),
        vec![victim, projectile]
    );
    assert!(
        !sim.substrate
            .entities
            .get(victim)
            .unwrap()
            .lifecycle
            .object_alive()
    );
    assert_eq!(
        delivery_sounds(&sim),
        ["DEATHREPORT", "DESTROYREPORT", "DieA"]
    );
    assert!(sim.particle_systems().is_empty());
    // Existing Rust stream regressions from the real Bullet AI.
    assert_eq!(sim.scenario_rng.state(), 2066545679410230777);
    assert_eq!(sim.main_rng.state(), 6706932826526710953);
    // Retired objects remain physically resolvable until the shared drain.
    // Visiting again must not deliver another death transaction.
    let _ = sim.object_ai_visit_one(projectile, Some(&rules), ObjectAiCtx::default());
    assert_eq!(
        delivery_sounds(&sim),
        ["DEATHREPORT", "DESTROYREPORT", "DieA"]
    );
    assert_eq!(delivered_ids(&sim), ids);
}
