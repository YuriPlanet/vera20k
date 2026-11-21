//! Stock Slave Miner pack-up through the production Rules and Selling owners.

use super::*;
use crate::sim::combat::Health;
use crate::sim::game_entity::GameEntity;
use crate::sim::house_state::HouseState;
use crate::util::save_codec;
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
        sim.session.set_current_house_for_test(Some(owner));
        sim.houses
            .register(HouseState::new(owner, 0, None, true, 0, 10));
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
        sim.substrate.entities.insert(entity);
        let mut terrain = crate::map::resolved_terrain::test_flat_ground_grid(16);
        if row["name"] == "resource_destination" {
            terrain.cell_mut_for_test(rx, ry).unwrap().land_type =
                crate::rules::terrain_rules::LandType::Tiberium.as_index();
        }
        sim.install_resolved_terrain_for_new_map(terrain);
        sell_stage_zero(&mut sim, &rules, 1, &mut FrameEffects::empty());
        let cues: Vec<_> = sim.sound_events.iter().filter_map(|event| match event {
            SimSoundEvent::UnitVoiceRequested { owner: 1, sound_id } =>
                Some(json!({"kind":"QueueVoice", "sound":sound_id.name_in(&rules)})),
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
    entity.seldom.suppress_deploy_voice.suppress();
    let bytes = save_codec::serialize(&entity).unwrap();
    let mut loaded: GameEntity = save_codec::deserialize(&bytes).unwrap();
    assert_eq!(
        loaded.deploy_voice_suppressed(),
        native["loaded_voice_reset"]["suppression"]
            .as_bool()
            .unwrap()
    );
    loaded.seldom.suppress_deploy_voice.clear();
    assert!(!loaded.deploy_voice_suppressed());
    assert!(
        entity.deploy_voice_suppressed(),
        "restored authority is independent"
    );
}

/// A deploy click runs VoiceDeploy (`0x00708E00`) on the clicking client
/// only, clearing its Techno+4F8; the state hash must not tell the clients
/// apart.
#[test]
fn a_clients_own_deploy_click_does_not_reach_the_state_hash() {
    let Some(retail) = crate::rules::retail_ini_fixture::retail_battle_rules() else {
        return;
    };
    let rules = retail.rules;
    let client = |clicked: bool| {
        let mut sim = crate::sim::arena_fixture::rival_arena(&rules, 0x708E00);
        let miner = sim
            .spawn_object("SMIN", "Americans", 10, 10, 0, &rules)
            .expect("SMIN spawns");
        let entity = sim.substrate.entities.get_mut(miner).unwrap();
        entity.seldom.suppress_deploy_voice.suppress();
        if clicked {
            let _ = sim.deploy_voice_request(&rules, miner, true);
            assert!(!sim.entities().get(miner).unwrap().deploy_voice_suppressed());
        }
        sim
    };
    assert_eq!(client(true).state_hash(), client(false).state_hash());
}
