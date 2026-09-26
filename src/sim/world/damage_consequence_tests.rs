//! Regression coverage of the existing Rust consequence schedules, not native parity.

use super::*;
use crate::rules::{art_data::ArtRegistry, ini_parser::IniFile};
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
    rules.art_registry = art;
    rules
}

fn consequence_world() -> (Simulation, u64, u64) {
    let mut sim = Simulation::with_seed(17);
    sim.input_delay_ticks = 0;
    let attacker = sim.allocate_stable_id();
    let victim = sim.allocate_stable_id();
    for (id, kind, owner, rx, hp) in [
        (attacker, "TESLA", "Americans", 5, 300),
        (victim, "TARGET", "Soviet", 8, 10),
    ] {
        let mut entity = GameEntity::test_default(id, kind, owner, rx, 5);
        entity.health.current = hp;
        sim.substrate.entities.insert(entity);
    }
    sim.interner = crate::sim::intern::test_interner();
    for id in [attacker, victim] {
        assert!(matches!(sim.reveal(id), RevealOutcome::Revealed { .. }));
    }
    (sim, attacker, victim)
}

fn debris_fingerprint(sim: &Simulation, id: u64) -> u64 {
    bincode::serialize(sim.substrate.voxel_anims.get(id).unwrap())
        .unwrap()
        .into_iter()
        .fold(0xcbf29ce484222325_u64, |hash, byte| {
            (hash ^ u64::from(byte)).wrapping_mul(0x100000001b3)
        })
}

fn delivered_ids(sim: &Simulation) -> Vec<u64> {
    let debris = sim.substrate.voxel_anims.ids();
    assert_eq!(debris.len(), 1);
    let mut ids = debris;
    for name in ["DEATHFX", "DESTROYFX"] {
        ids.push(
            sim.anims()
                .find(|(_, anim)| sim.interner.resolve(anim.type_id) == name)
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
            SimSoundEvent::EntityDied { die_sound_id, .. } => {
                Some(sim.interner.resolve(*die_sound_id).to_owned())
            }
            SimSoundEvent::AnimationStarted { sound_id, .. } => {
                Some(sim.interner.resolve(*sound_id).to_owned())
            }
            _ => None,
        })
        .collect()
}

fn advance_lethal_shot(sim: &mut Simulation, rules: &RuleSet, attacker: u64, victim: u64) {
    let owner = sim.interner.intern("Americans");
    let grid = PathGrid::test_all_passable(64, 64);
    sim.queue_command(CommandEnvelope::new(
        owner,
        sim.session.tick + 1,
        Command::Attack {
            attacker_id: attacker,
            target_id: victim,
        },
    ));
    for _ in 0..60 {
        let commands = sim.take_due_commands();
        sim.advance_tick(
            &commands,
            Some(rules),
            &BTreeMap::new(),
            Some(&grid),
            None,
            100,
        );
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
        .expect("late target lookup constructs spark")
        .0;
    assert_eq!(spark, ids[2] + 1);
    ids.push(spark);
    assert_eq!(
        sim.live_object_order_snapshot(),
        [vec![attacker], ids.clone()].concat()
    );
    assert_eq!(
        delivery_sounds(&sim),
        ["DEATHREPORT", "DESTROYREPORT", "DieA"]
    );
    assert!(sim.substrate.entities.get(victim).is_none());
    // Rust regression, not a gamemd-derived golden. Fingerprint includes the
    // complete debris body. Re-captured when the debris and Explosion= draws
    // moved to the Scenario stream (`0x007022C8`, `0x007386A7`); only the
    // death sound stays on the main stream.
    assert_eq!(sim.scenario_rng.state(), 3954386809370758752);
    assert_eq!(sim.main_rng.state(), 6706932826526710953);
    // Re-captured when the shot became a bullet: the body is unchanged but
    // for its stable id, one later (the bullet took the one before it).
    assert_eq!(debris_fingerprint(&sim, ids[0]), 17414601428498543321);
    let next_id = sim.allocate_stable_id();
    assert_eq!(next_id, spark + 1);
    sim.advance_tick(&[], Some(&rules), &BTreeMap::new(), Some(&grid), None, 100);
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
    let passenger = sim.allocate_stable_id();
    let mut entity = GameEntity::test_default(passenger, "TARGET", "Soviet", 8, 5);
    entity.passenger_role = PassengerRole::Inside {
        transport_id: carrier,
    };
    sim.substrate.entities.insert(entity);
    let mut cargo = PassengerCargo::new(1, 1);
    assert!(cargo.board(passenger, 1));
    sim.substrate
        .entities
        .get_mut(carrier)
        .unwrap()
        .passenger_role = PassengerRole::Transport { cargo };
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
    // `passenger + 1` is the shot's bullet.
    assert_eq!(
        delivered_ids(&sim),
        vec![passenger + 2, passenger + 3, passenger + 4]
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
    let projectile = sim.allocate_stable_id();
    let impact = ProjectileCoord::new(8 * 256 + 128, 5 * 256 + 128, 0);
    let mut spawn = super::lifecycle_tests::gsi_05_02_projectile(attacker, Some(0));
    spawn.origin = impact;
    spawn.target = ProjectileTarget::Entity(victim);
    spawn.initial_target_position = impact;
    spawn.payload = ProjectilePayload {
        base_damage: 40,
        warhead: sim.interner.intern("AP"),
        weapon: sim.interner.intern("CoilBolt"),
    };
    sim.admit_projectile(projectile, spawn);
    assert!(sim.object_ai_visit_one(projectile, Some(&rules), ObjectAiCtx::default()));
    let ids = delivered_ids(&sim);
    assert_eq!(ids, vec![projectile + 1, projectile + 2, projectile + 3]);
    assert_eq!(
        sim.live_object_order_snapshot(),
        [vec![attacker], ids.clone()].concat()
    );
    assert_eq!(sim.substrate.pending_delete, vec![victim, projectile]);
    assert!(
        !sim.substrate
            .entities
            .get(victim)
            .unwrap()
            .lifecycle
            .object_alive
    );
    assert_eq!(
        delivery_sounds(&sim),
        ["DEATHREPORT", "DESTROYREPORT", "DieA"]
    );
    assert!(sim.particle_systems().is_empty());
    // Rust regression from the real Bullet AI, re-captured when the debris and
    // Explosion= draws moved to the Scenario stream; only the death sound
    // stays on the main stream.
    assert_eq!(sim.scenario_rng.state(), 2066545679410230777);
    assert_eq!(sim.main_rng.state(), 6706932826526710953);
    assert_eq!(debris_fingerprint(&sim, ids[0]), 8120097345519581533);
    // Retired objects remain physically resolvable until the shared drain.
    // Visiting again must not deliver another death transaction.
    let _ = sim.object_ai_visit_one(projectile, Some(&rules), ObjectAiCtx::default());
    assert_eq!(
        delivery_sounds(&sim),
        ["DEATHREPORT", "DESTROYREPORT", "DieA"]
    );
    assert_eq!(delivered_ids(&sim), ids);
}
