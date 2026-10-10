//! Selected bridge landing child Report reader and destruction composition.

use super::*;
use crate::audio::arbiter::{
    ArbiterAction, EntryFacts, HandleOwner, PlayRequest, SoundArbiter, TestPlayback,
};
use crate::rules::{
    art_data::ArtRegistry, retail_ini_fixture::retail_ini, sound_ini::SoundRegistry,
};
use crate::util::fixed_math::SimFixed;

#[test]
fn retail_landing_child_reports_release_instead_of_cutting_samples() {
    let (Some(ini), Some(art), Some(sound)) = (
        retail_ini("rulesmd.ini"),
        retail_ini("artmd.ini"),
        retail_ini("soundmd.ini"),
    ) else {
        return;
    };
    let mut rules = RuleSet::from_ini_with_fixed_art_for_test(&ini, &art).unwrap();
    rules.install_art_data(ArtRegistry::from_ini(&art));
    let registry = SoundRegistry::from_ini(&sound);
    let native: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
        "tools/rules_oracle/bridge_child_sound.json",
    ))
    .unwrap();
    let art_native: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
        "tools/rules_oracle/bridge_anim_inputs.json",
    ))
    .unwrap();
    for report in native["anim_reports"].as_array().unwrap() {
        let name = report["name"].as_str().unwrap();
        let config = rules.art().anim_runtime_config(name).unwrap();
        // Native resolves Report with case-insensitive Voc FindByName;
        // ArtRegistry stores the same identity in uppercase.
        assert_eq!(
            config.report,
            report["report_name"].as_str().map(str::to_ascii_uppercase)
        );
        assert!(config.stop_sound.is_none());
        let Some(sound_name) = report["report_name"].as_str() else {
            continue; // SMOKEY2 has no Report in the original reader either.
        };
        let row = native["rows"]
            .as_array()
            .unwrap()
            .iter()
            .find(|row| row["name"] == sound_name)
            .unwrap();
        let entry = registry.get(sound_name).unwrap();
        assert_eq!(
            serde_json::json!({
                "control": entry.control,
                "type_flags": entry.type_flags,
                "volume_linear": entry.volume_linear,
                "priority": entry.priority,
                "limit": entry.limit,
                "loop_count": entry.loop_count,
                "range": entry.range,
                "delay_min": entry.delay_ms.0,
                "delay_max": entry.delay_ms.1,
                "fshift_min": entry.fshift.0,
                "fshift_max": entry.fshift.1,
                "vshift": entry.vshift,
                "samples": entry.sounds,
                "sample_count": entry.sounds.len(),
            }),
            serde_json::Value::Object(
                [
                    "control",
                    "type_flags",
                    "volume_linear",
                    "priority",
                    "limit",
                    "loop_count",
                    "range",
                    "delay_min",
                    "delay_max",
                    "fshift_min",
                    "fshift_max",
                    "vshift",
                    "samples",
                    "sample_count"
                ]
                .into_iter()
                .map(|key| (key.to_string(), row[key].clone()))
                .collect()
            ),
            "physical SOUNDMD original reader: {sound_name}"
        );
        let frames = art_native["rows"]
            .as_array()
            .unwrap()
            .iter()
            .find(|row| row["name"] == name)
            .unwrap()["raw_shp_frame_count"]
            .as_i64()
            .unwrap();
        rules.bind_anim_frame_count_for_test(name, frames as i32);
        let mut sim = Simulation::new();
        let type_id = sim.interner.intern(name);
        let id = sim
            .spawn_combat_explosion_anim(
                &rules,
                type_id,
                4,
                4,
                SimFixed::from_num(128),
                SimFixed::from_num(128),
                0,
                416,
            )
            .unwrap();
        // Exercise natural expiry; this test does not certify the native
        // child's entire timer/Middle/Scenario RNG trajectory.
        for frame in 0..100 {
            sim.session.binary_frame = frame;
            sim.visit_anim(id, &rules, None);
            if sim.substrate.pending_delete.contains(&id) {
                break;
            }
        }
        assert!(sim.substrate.pending_delete.contains(&id), "{name}");
        assert!(!sim.anim(id).unwrap().in_logic_vector, "{name}");
        let events = std::mem::take(&mut sim.sound_events);
        let mut output = SoundEventQueue::default();
        dispatch_sim_sound_events(events, &sim, &rules, None, &mut |_| true, &mut output);
        let events = output.drain();
        assert!(
            matches!(events.as_slice(), [
            GameSoundEvent::AnimationStarted { anim_id, sound_id, .. },
            GameSoundEvent::AnimationReleased { anim_id: released },
        ] if *anim_id == id && *released == id && sound_id.eq_ignore_ascii_case(sound_name)),
            "{name}: {events:?}"
        );
        // Original406060 leaves every byte of the selected playing event
        // unchanged. 405D40 instead moves it to stopped state and clears its
        // serial: the two operations are observably different.
        assert_eq!(row["release"]["event_unchanged"], true);
        assert_eq!(row["hard_stop"]["event_unchanged"], false);

        // The actual SfxPlayer release operation delegates to this same
        // device-free owner. Retain an admitted, playing one-shot through
        // release and the next pump; only sample completion ends it.
        let mut arbiter = SoundArbiter::new(0);
        let playing = arbiter
            .submit(
                &PlayRequest {
                    key: sound_name.to_ascii_uppercase(),
                    facts: EntryFacts::from(entry),
                    volume_linear: entry.volume_linear,
                    pan: 0x2000,
                },
                0,
            )
            .unwrap();
        arbiter.set_loop_handle(
            HandleOwner::Positional(id),
            Some(playing),
            &sound_name.to_ascii_uppercase(),
        );
        assert!(arbiter.update_tick(100, &mut TestPlayback::default()).iter().any(|action| matches!(
            action, ArbiterAction::Start { event, sustaining: false, .. } if *event == playing
        )));
        let GameSoundEvent::AnimationReleased { anim_id } = events[1] else {
            unreachable!("ordered app event checked above")
        };
        arbiter.release_owner(HandleOwner::Positional(anim_id));
        assert!(arbiter.loop_handle_owners().is_empty());
        assert!(
            arbiter
                .update_tick(200, &mut TestPlayback::default())
                .iter()
                .all(|action| !matches!(
                    action, ArbiterAction::Stop { event } if *event == playing
                ))
        );
        assert_eq!(arbiter.live_event_count(), 1);
        assert_eq!(arbiter.busy_channel_count(), 1);
        arbiter.notify_playout_ended(playing);
        arbiter.update_tick(240, &mut TestPlayback::default());
        assert_eq!(arbiter.live_event_count(), 0);
        assert_eq!(arbiter.busy_channel_count(), 0);
    }
}
