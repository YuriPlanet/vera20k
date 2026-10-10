//! Presentation Main draws must not allocate gameplay identities.
//!
//! The two-name MoveSound input is an executed native reader/tail control in
//! foot_move_sound.json. This test establishes Rust state isolation across a
//! presentation service boundary, not a native whole-frame audio schedule.

use super::{SimSoundEvent, Simulation};
use crate::map::entities::EntityCategory;
use crate::rules::ini_parser::IniFile;
use crate::rules::locomotor_type::LocomotorKind;
use crate::rules::ruleset::RuleSet;
use crate::rules::sound_ini::SoundRegistry;
use crate::sim::combat::TargetKind;
use crate::sim::combat::world_receiver::FireVisit;
use crate::sim::game_entity::GameEntity;
use crate::sim::house_state::HouseState;
use crate::sim::movement::locomotor::LocomotorState;
use crate::sim::snapshot::GameSnapshot;
use serde_json::Value;

fn rules() -> RuleSet {
    let corpus = crate::rules::move_sound_tests::native();
    let row = corpus["timelines"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["name"] == "ordered_two_sounds")
        .unwrap();
    let ini = IniFile::from_str(&format!(
        "[VehicleTypes]\n0=SQD\n1=GUN\n2=TARGET\n\
         [SQD]\nStrength=200\nNaval=yes\nLocomotor={}\nMoveSound={}\n\
         [GUN]\nStrength=100\nPrimary=LateGun\nSpeed=6\nLocomotor={}\n\
         [TARGET]\nStrength=100\nArmor=steel\n\
         [LateGun]\nDamage=10\nROF=30\nRange=6\nSpeed=20\nProjectile=LateBullet\nWarhead=LateWarhead\n\
         [LateBullet]\nROT=1\nAG=yes\nAA=no\n\
         [LateWarhead]\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n",
        LocomotorKind::Ship.clsid(),
        row["input"]["move_sound"].as_str().unwrap(),
        LocomotorKind::Drive.clsid(),
    ));
    let sounds = SoundRegistry::from_ini(&IniFile::from_str(
        "[SoundList]\n0=SquidMove\n1=GenLargeWaterDie\n",
    ));
    let mut rules = RuleSet::from_ini(&ini).unwrap();
    rules.bind_type_sound_references(&ini, &sounds);
    assert_eq!(
        serde_json::json!(rules.object("SQD").unwrap().move_sound),
        row["binding"]["names"],
        "the fixture uses the executed two-entry reader result"
    );
    rules
}

fn scene(rules: &RuleSet) -> (Simulation, [u64; 2], u64, u64) {
    let mut sim = Simulation::with_seed(1);
    sim.session.map_width = 64;
    sim.session.map_height = 64;
    sim.intern_rule_type_ids(rules);
    sim.resolve_type_handles(rules);
    for (index, name) in ["Local", "Enemy"].into_iter().enumerate() {
        let owner = sim.interner.intern(name);
        sim.houses.insert(
            owner,
            HouseState::new(owner, index as u8, None, index == 0, 0, 10),
        );
        sim.session.house_order.push(owner);
    }
    let owner = sim.interner.get("Local").unwrap();
    let squids = std::array::from_fn(|index| {
        let id = sim.allocate_stable_id();
        let mut actor = GameEntity::test_default(id, "SQD", "Local", 10, 10 + index as u16);
        actor.owner = owner;
        actor.type_ref = sim.interner.get("SQD").unwrap();
        actor.category = EntityCategory::Unit;
        actor.lifecycle.in_limbo = false;
        actor.lifecycle.object_alive = true;
        actor.locomotor = Some(LocomotorState::from_object_type(
            rules.object("SQD").unwrap(),
            0,
        ));
        // Supplied Process result, as in the native no-movement tail controls.
        actor.body_frame_counter = 1;
        sim.substrate.entities.insert(actor);
        id
    });
    let gun = sim
        .spawn_object("GUN", "Local", 20, 20, 128, rules)
        .unwrap();
    let target = sim
        .spawn_object("TARGET", "Enemy", 20, 24, 0, rules)
        .unwrap();
    sim.assign_target_represented(gun, Some(TargetKind::Entity(target)), Some(rules))
        .unwrap();
    assert!(sim.interner.get("LateGun").is_none());
    assert!(sim.interner.get("LateWarhead").is_none());
    (sim, squids, gun, target)
}

fn started_names(sim: &Simulation) -> Vec<String> {
    sim.sound_events
        .iter()
        .filter_map(|event| match event {
            SimSoundEvent::AnimationStarted { sound_id, .. } => Some(sound_id.clone()),
            _ => None,
        })
        .collect()
}

#[test]
fn presentation_main_service_cannot_change_later_projectile_identity() {
    let rules = rules();
    let (mut quiet, quiet_squids, quiet_gun, quiet_target) = scene(&rules);
    let (mut serviced, serviced_squids, serviced_gun, serviced_target) = scene(&rules);
    assert_eq!(quiet.state_hash(), serviced.state_hash());

    let (_, quiet_draws) = crate::sim::rng::trace_draws(|| {
        quiet.tick_move_sound_after_process(quiet_squids[0], 0, Some(&rules));
        quiet.tick_move_sound_after_process(quiet_squids[1], 0, Some(&rules));
    });
    let (_, serviced_draws) = crate::sim::rng::trace_draws(|| {
        serviced.tick_move_sound_after_process(serviced_squids[0], 0, Some(&rules));
        // One ordinary inclusive presentation draw between otherwise identical
        // paid Foot visits. No Scenario or MapGen capability is lent.
        let _ = serviced.presentation_main_draws().ranged(0, 1);
        serviced.tick_move_sound_after_process(serviced_squids[1], 0, Some(&rules));
    });
    let rng: Value = serde_json::from_str(crate::test_fixture::text(
        "tools/rmg_oracle/vectors/rng.json",
    ))
    .unwrap();
    let native = rng["cases"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["seed"] == 1)
        .unwrap();
    let expected = native["draws"]
        .as_array()
        .unwrap()
        .iter()
        .take(3)
        .map(|word| u32::from_str_radix(word.as_str().unwrap(), 16).unwrap())
        .collect::<Vec<_>>();
    for (actual, count) in [(&quiet_draws, 2), (&serviced_draws, 3)] {
        assert_eq!(
            actual
                .iter()
                .map(|draw| draw["value"].as_u64().unwrap() as u32)
                .collect::<Vec<_>>(),
            expected[..count],
            "the existing native Main corpus pins the interleaved raw prefix"
        );
    }
    let quiet_names = started_names(&quiet);
    let serviced_names = started_names(&serviced);
    assert_eq!(quiet_names.len(), 2);
    assert_eq!(serviced_names.len(), 2);
    assert_eq!(quiet_names[0], serviced_names[0]);
    assert_ne!(quiet_names[1], serviced_names[1]);
    assert_eq!(serviced_names[0], serviced_names[1]);
    assert_ne!(
        quiet.main_rng.logical_state(),
        serviced.main_rng.logical_state()
    );
    assert_eq!(
        quiet.scenario_rng.logical_state(),
        serviced.scenario_rng.logical_state()
    );
    assert_eq!(
        quiet.mapgen_rng.logical_state(),
        serviced.mapgen_rng.logical_state()
    );
    assert_eq!(
        quiet.state_hash(),
        serviced.state_hash(),
        "sound latch state is independent of the selected cue"
    );

    for (sim, gun, target) in [
        (&mut quiet, quiet_gun, quiet_target),
        (&mut serviced, serviced_gun, serviced_target),
    ] {
        sim.sound_events.clear();
        sim.commit_fire_visit(
            FireVisit::Direct {
                id: gun,
                target: TargetKind::Entity(target),
                weapon_index: 0,
            },
            &rules,
            None,
        );
        assert_eq!(
            sim.projectiles.iter().count(),
            1,
            "the production FireAt path must launch a surviving Bullet"
        );
        let (_, bullet) = sim.projectiles.iter().next().unwrap();
        assert_eq!(sim.interner.resolve(bullet.payload.weapon), "LateGun");
        assert_eq!(sim.interner.resolve(bullet.payload.warhead), "LateWarhead");
    }
    assert_eq!(
        quiet.scenario_rng.logical_state(),
        serviced.scenario_rng.logical_state()
    );
    assert_eq!(
        quiet.mapgen_rng.logical_state(),
        serviced.mapgen_rng.logical_state()
    );
    let payload_ids = |sim: &Simulation| {
        let (_, bullet) = sim.projectiles.iter().next().unwrap();
        (
            bullet.payload.weapon.index(),
            bullet.payload.warhead.index(),
        )
    };
    assert_eq!(
        quiet.state_hash(),
        serviced.state_hash(),
        "presentation service must not shift later gameplay Bullet IDs: quiet {:?}, serviced {:?}",
        payload_ids(&quiet),
        payload_ids(&serviced),
    );
    assert_eq!(payload_ids(&quiet), payload_ids(&serviced));
    assert_eq!(
        GameSnapshot::save_validated(
            &quiet,
            0,
            rules.simulation_config_hash(),
            "Main sound identity",
            0
        ),
        GameSnapshot::save_validated(
            &serviced,
            0,
            rules.simulation_config_hash(),
            "Main sound identity",
            0
        ),
        "Main and transient sound choices must not leak through the saved interner"
    );
}
