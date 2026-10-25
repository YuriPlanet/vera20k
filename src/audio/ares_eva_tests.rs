//! Executed Ares selection controls through the shared Vox binder. The queue
//! joins have instruction evidence, not an installed-hook/audio parity claim.

use super::*;
use crate::rules::ini_parser::IniFile;
use crate::rules::process_owner::{NativeRulesProcessOwner, NativeScenarioRulesPrefix};
use crate::rules::ruleset::RuleSet;
use crate::rules::sound_ini::SoundRegistry;
use std::sync::Arc;

fn registry() -> Arc<EvaRegistry> {
    let corpus = crate::test_fixture::json("tools/spatial_oracle/ares_eva.json");
    let registration = corpus["read_bind"]["controls"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["kind"] == "registration")
        .unwrap();
    Arc::new(EvaRegistry::from_ini(&crate::test_fixture::ini_sections(
        &registration["input"],
    )))
}

fn rules(tag: &str, eva: Arc<EvaRegistry>) -> RuleSet {
    let root = IniFile::from_str(&format!(
        "[Sides]\nGDI=ONE\nNod=TWO\nThirdSide=THREE\nSyntheticSide=FOUR\n\
         [SyntheticSide]\nEVA.Tag={tag}\n",
    ));
    let mut process = NativeRulesProcessOwner::from_cold_start_sources(
        root,
        None,
        IniFile::empty(),
        Arc::new(SoundRegistry::from_ini(&IniFile::empty())),
        eva,
    )
    .unwrap();
    process
        .load_scenario(
            NativeScenarioRulesPrefix::NonCampaign(None),
            &IniFile::empty(),
        )
        .unwrap()
        .into_parts()
        .0
}

fn probe(registry: &EvaRegistry) -> VoxNode {
    let entry = registry.entry("EVA_TEST").unwrap();
    VoxNode {
        event: entry.name.to_ascii_uppercase(),
        eva_type: entry.eva_type,
        priority: entry.priority,
        seq: 0,
    }
}

#[test]
fn ares_binding_and_filename_match_native_controls() {
    let registry = registry();
    let rules = rules("VoiceB", registry.clone());
    let corpus = crate::test_fixture::json("tools/spatial_oracle/ares_eva.json");
    let probe = probe(&registry);
    for row in corpus["read_bind"]["controls"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|row| row["kind"] == "set_side")
    {
        let mut queue = VoxQueue::new();
        // This is the actual native before-image of the process cache.
        queue.selected_voice = row["prior"].as_i64().unwrap() as i32;
        queue.set_side(row["side_index"].as_i64().unwrap() as i32, &rules);
        assert_eq!(
            queue.selected_voice(),
            row["bound"].as_i64().unwrap() as i32
        );
        assert_eq!(
            queue.sample_for(&probe, &registry).unwrap_or(""),
            row["filename"].as_str().unwrap()
        );
    }
}

#[test]
fn queued_identity_uses_the_later_bound_voice_and_reset_preserves_it() {
    let registry = registry();
    let first = rules("VoiceA", registry.clone());
    let second = rules("VoiceB", registry.clone());
    let entry = registry.entry("EVA_TEST").unwrap();
    let mut queue = VoxQueue::new();
    queue.set_side(3, &first);
    queue.set_paused(true);
    let effect = queue.queue_voice(
        VoxRequest {
            event: &entry.name,
            eva_type: entry.eva_type,
            priority: entry.priority,
        },
        None,
        None,
    );
    assert!(effect.inserted);
    assert!(queue.take_next(1, false).is_none());
    queue.set_side(3, &second);
    queue.set_paused(false);
    let node = queue
        .take_next(1, false)
        .expect("same event survives rebind");
    assert_eq!(node.event, "EVA_TEST");
    let expected =
        crate::test_fixture::json("tools/spatial_oracle/ares_eva.json")["read_bind"]["controls"]
            .as_array()
            .unwrap()
            .iter()
            .find(|row| row["kind"] == "filename" && row["voice_index"] == 4)
            .unwrap()["filename"]
            .as_str()
            .unwrap();
    assert_eq!(queue.sample_for(&node, &registry), Some(expected));
    queue.started(node);
    assert!(queue.reset_all());
    assert_eq!(queue.selected_voice(), 4);
    queue.set_side(-1, &second);
    assert_eq!(queue.sample_for(&probe(&registry), &registry), None);
}
