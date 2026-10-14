//! Stock Slave Miner pack-up through the production Rules and Selling owners.

use super::*;
use crate::sim::combat::Health;
use crate::sim::game_entity::GameEntity;
use crate::sim::house_state::HouseState;
use serde_json::json;

#[test]
fn retail_slave_miner_selling_stage_queues_its_pack_voice_before_deploy_sound() {
    let Some(retail) = crate::rules::retail_ini_fixture::retail_battle_rules_for_map("XMP03T4.MAP")
    else {
        return;
    };
    let rules = retail.rules;
    let native = crate::rules::deploy_voice_tests::native();
    let controls = native["selling_cue_controls"].as_array().unwrap();
    assert_eq!(controls.len(), 2);
    for row in controls {
        let mut sim = Simulation::new();
        let owner = sim.interner.intern("YuriCountry");
        sim.session.current_house = Some(owner);
        sim.houses
            .insert(owner, HouseState::new(owner, 0, None, true, 0, 10));
        let type_ref = sim.interner.intern("YAREFN");
        // Original cue controls declare a Cell Archive and a centered Location.
        // Compare cue identity/order, rather than the legacy source projection.
        let coords = &row["calls"][1]["coords"];
        let rx = u16::try_from(coords[0].as_i64().unwrap() >> 8).unwrap();
        let ry = u16::try_from(coords[1].as_i64().unwrap() >> 8).unwrap();
        let mut entity = GameEntity::new_at_frame_zero_for_test(
            1,
            rx,
            ry,
            0,
            0,
            owner,
            Health::new(2000),
            type_ref,
            EntityCategory::Structure,
            0,
            5,
            false,
        );
        entity.set_archive_target(Some(crate::sim::combat::TargetKind::Cell(rx, ry)));
        sim.entities_mut().insert(entity);
        let mut terrain = crate::map::resolved_terrain::test_flat_ground_grid(16);
        if row["name"] == "resource_destination" {
            terrain.cell_mut(rx, ry).unwrap().land_type =
                crate::rules::terrain_rules::LandType::Tiberium.as_index();
        }
        sim.install_resolved_terrain_for_new_map(terrain);
        sell_stage_zero(&mut sim, Some(&rules), 1, FrameEffects::empty());
        let cues: Vec<_> = sim.sound_events.iter().filter_map(|event| match event {
            SimSoundEvent::UnitVoiceRequested { owner: 1, sound_id } =>
                Some(json!({"kind":"QueueVoice", "sound":sound_id})),
            SimSoundEvent::EntityDeployed { deploy_sound_id, .. } =>
                Some(json!({"kind":"positional_PlayAt_boundary", "sound":sim.interner.resolve(*deploy_sound_id)})),
            _ => None,
        }).collect();
        let expected: Vec<_> = row["calls"]
            .as_array()
            .unwrap()
            .iter()
            .map(|call| json!({"kind":call["kind"], "sound":call["sound"]}))
            .collect();
        assert_eq!(cues, expected, "{}: reached Selling cues", row["name"]);
        assert_eq!(
            sim.entities().get(1).unwrap().deploy_voice_suppressed(),
            row["suppression"].as_bool().unwrap()
        );
    }
}

#[test]
fn deploy_voice_suppression_roundtrips_independently_of_the_audio_latch() {
    let native = crate::rules::deploy_voice_tests::native();
    let mut entity = GameEntity::test_default(1, "YAREFN", "YuriCountry", 6, 7);
    assert!(!entity.deploy_voice_suppressed());
    entity.suppress_next_deploy_voice();
    let bytes = bincode::serialize(&entity).unwrap();
    let mut loaded: GameEntity = bincode::deserialize(&bytes).unwrap();
    assert_eq!(
        loaded.deploy_voice_suppressed(),
        native["loaded_voice_reset"]["suppression"]
            .as_bool()
            .unwrap()
    );
    loaded.clear_deploy_voice_suppression();
    assert!(!loaded.deploy_voice_suppressed());
    assert!(
        entity.deploy_voice_suppressed(),
        "restored authority is independent"
    );
}
