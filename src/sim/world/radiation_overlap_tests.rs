//! Stock DESO overlap at the Cell487CE0 -> Cell487CB0 -> Foot4DA604 boundary.
//!
//! Original field/Foot/concrete-DOG receiver controls are retained in
//! tools/spatial_oracle/radiation_overlap.json. These tests join that numeric
//! boundary to stock Guard/self-shot admission, later pulses and persistence.

use super::{FrameEffects, Simulation, TickLane};
use crate::rules::ruleset::RuleSet;
use crate::sim::combat::TargetKind;
use crate::sim::combat::combat_weapon::WeaponSlot;
use crate::sim::mission::state::MissionTestFixture;
use crate::sim::mission::{MissionDispatchTimer, MissionId, MissionType};
use crate::util::save_codec;

const CENTERS: [(u16, u16); 3] = [(17, 24), (31, 24), (24, 17)];
const DOG_CELL: (u16, u16) = (24, 24);

fn advance(sim: &mut Simulation, rules: &RuleSet) -> super::SimFrameOutput {
    let commands = sim.take_due_commands();
    let output = sim
        .advance_app_frame(
            &commands,
            Some(rules),
            None,
            33,
            TickLane::Ordinary,
            FrameEffects::default(),
        )
        .expect("ordinary DESO overlap frame");
    assert!(output.tick.frame_committed);
    output
}

fn create_overlap(rules: &RuleSet, count: usize) -> (Simulation, u64) {
    let mut sim = crate::sim::arena_fixture::rival_arena_with_size(rules, 31, 64);
    let mut desos = Vec::new();
    for (index, &(rx, ry)) in CENTERS[..count].iter().enumerate() {
        let id = sim
            .spawn_object("DESO", "Americans", rx, ry, 0, rules)
            .unwrap();
        let actor = sim.substrate.entities.get_mut(id).unwrap();
        actor.mission_leaf.set_infantry_doing_verified(27).unwrap();
        let frame = 13 + index as u32;
        actor.mission.apply_test_fixture(MissionTestFixture {
            current: MissionId::from_known(MissionType::Guard),
            suspended: MissionId::NONE,
            queued: MissionId::NONE,
            movement_bypass_latch: 0,
            handler_state: 0,
            mission_start_frame: frame,
            ai_counter: 0,
            dispatch_timer: MissionDispatchTimer::at_frame(frame),
        });
        desos.push(id);
    }
    sim.session.set_binary_frame_for_test(13);
    assert!(sim.radiation.is_empty());
    // Separate ordinary frames retain the native compacting-Bullet skip rule:
    // no second adjacent Inviso Bullet can lose its first AI behind retirement.
    // All three fields predate16 and their first level-decay timer remains90.
    for frame in 13..=15 {
        assert_eq!(sim.session.binary_frame(), frame);
        let output = advance(&mut sim, rules);
        let index = (frame - 13) as usize;
        let shots: Vec<_> = output
            .fire_events
            .iter()
            .filter(|shot| {
                desos.contains(&shot.attacker_id)
                    && sim.interner.resolve(shot.weapon_id) == "RadEruptionWeapon"
            })
            .collect();
        if index < count {
            assert_eq!(shots.len(), 1, "one first Guard shot on frame{frame}");
            assert_eq!(shots[0].attacker_id, desos[index]);
            assert_eq!(shots[0].weapon_slot, WeaponSlot::Secondary);
            assert_eq!(
                shots[0].target,
                TargetKind::Cell(CENTERS[index].0, CENTERS[index].1)
            );
            assert!(
                sim.radiation.site_at(CENTERS[index]).is_some(),
                "the real Bullet must create its site before frame16"
            );
        } else {
            assert!(shots.is_empty());
        }
    }
    assert_eq!(sim.session.binary_frame(), 16);
    assert_eq!(sim.radiation.sites().count(), count);
    for center in &CENTERS[..count] {
        let site = sim.radiation.site_at(*center).unwrap();
        assert_eq!(site.level(), 500);
        assert_eq!(site.radius_leptons(), 2688);
    }
    // Admit the fresh target after all direct self-shot damage has completed.
    // Field geometry and stock DOG health are the native comparison inputs.
    let dog = sim
        .spawn_object("DOG", "Americans", DOG_CELL.0, DOG_CELL.1, 0, rules)
        .unwrap();
    assert_eq!(
        sim.substrate.entities.get(dog).unwrap().health.current(),
        100
    );
    (sim, dog)
}

#[test]
fn stock_dog_survives_first_pulse_from_three_real_deso_fields() {
    let Some(fixture) = crate::rules::retail_ini_fixture::retail_battle_rules() else {
        return;
    };
    let rules = fixture.rules;
    let dog_type = rules.object("DOG").unwrap();
    assert_eq!(dog_type.strength, 100);
    assert!(!dog_type.immune_to_radiation);
    assert_eq!(rules.radiation.application_delay, 16);
    assert_eq!(rules.weapon("RadEruptionWeapon").unwrap().rad_level, 500);
    assert_eq!(
        rules.warhead("RadEruptionWarhead").unwrap().cell_spread_f64,
        10.0
    );
    let native: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/radiation_overlap.json",
    ))
    .unwrap();
    assert_eq!(native["schema_version"], 1);
    for (count, name) in [(1, "one_site"), (2, "two_sites"), (3, "three_sites")] {
        let field = native["rows"]
            .as_array()
            .unwrap()
            .iter()
            .find(|row| row["name"] == name)
            .unwrap();
        let receiver = native["concrete_receivers"]
            .as_array()
            .unwrap()
            .iter()
            .find(|row| row["name"] == name)
            .unwrap();
        assert_eq!(receiver["strength"], dog_type.strength);
        assert_eq!(receiver["base_damage"], field["base_damage"]);
        assert_eq!(receiver["alive"], true);
        assert_eq!(receiver["limbo"], false);
        assert_eq!(receiver["rng_unchanged"], true);
        for address in ["00517FA0", "004D7330", "00701900", "005F5390", "00489180"] {
            assert!(
                receiver["calls"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|call| call == address)
            );
        }
        let expected_health = receiver["health"].as_i64().unwrap() as i32;
        let (mut sim, dog) = create_overlap(&rules, count);
        let expected_bits = u64::from_str_radix(field["level_bits"].as_str().unwrap(), 16).unwrap();
        assert_eq!(
            sim.radiation.cell_level(DOG_CELL).to_bits(),
            expected_bits,
            "{name}: original Cell487CE0 accumulation stores"
        );
        assert_eq!(
            sim.radiation
                .damaging_level(DOG_CELL, rules.radiation.level_max),
            field["cell_damaging_level"].as_i64().unwrap() as i32
        );
        advance(&mut sim, &rules);
        let health = sim
            .substrate
            .entities
            .get(dog)
            .map(|actor| actor.health.current());
        assert_eq!(
            health,
            Some(expected_health),
            "{count} native-overlapping stock fields"
        );
        if count == 3 {
            while sim.session.binary_frame() < 32 {
                advance(&mut sim, &rules);
                assert_eq!(
                    sim.substrate.entities.get(dog).unwrap().health.current(),
                    expected_health,
                    "the survivor is not hit between application boundaries"
                );
            }
            advance(&mut sim, &rules);
            assert!(
                sim.substrate
                    .entities
                    .get(dog)
                    .is_none_or(|actor| actor.health.current() <= 0),
                "the next pulse kills the one-HP survivor through the real receiver"
            );
        }
    }
}

/// Numeric state persistence only: the exact field produced by real shots is
/// round-tripped through its existing serde and hashed by the world owner.
/// This deliberately does not exercise or claim saved-game loading parity.
#[test]
fn overlapping_stock_fields_preserve_serialized_bits_hash_and_decay() {
    let Some(fixture) = crate::rules::retail_ini_fixture::retail_battle_rules() else {
        return;
    };
    let rules = fixture.rules;
    let (scene, _) = create_overlap(&rules, 3);
    let bytes = save_codec::serialize(&scene.radiation).unwrap();
    let mut reference = Simulation::with_seed(0);
    reference.radiation = scene.radiation.clone();
    let mut restored = Simulation::with_seed(0);
    restored.radiation = save_codec::deserialize(&bytes).unwrap();
    assert_eq!(restored.state_hash(), reference.state_hash());
    let before_decay = reference.radiation.cell_level(DOG_CELL).to_bits();
    let initial_hash = reference.state_hash();
    // The actual sites were activated on13/14/15. Their first90-frame decay
    // deadlines are therefore103/104/105. Use the unchanged production owner.
    for frame in 16..110 {
        for world in [&mut reference, &mut restored] {
            world.radiation.tick_decay(
                frame,
                &rules.radiation,
                scene.resolved_terrain.as_ref(),
                &mut crate::sim::light_sources::LightingSources::default(),
            );
        }
        assert_eq!(
            save_codec::serialize(&restored.radiation).unwrap(),
            save_codec::serialize(&reference.radiation).unwrap(),
            "retained cell bits/site timers after frame{frame}"
        );
        assert_eq!(
            restored.state_hash(),
            reference.state_hash(),
            "numeric-state hash continuation frame{frame}"
        );
    }
    assert_ne!(
        reference.radiation.cell_level(DOG_CELL).to_bits(),
        before_decay,
        "the comparison must execute real field decay"
    );
    assert_ne!(
        reference.state_hash(),
        initial_hash,
        "the existing world hash must observe changed radiation state"
    );
}
