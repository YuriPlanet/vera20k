//! Original Foot sound-handle comparisons. The native fixture executes
//! 405D40, 405FD0 and 406060 on tagged records with no attached channel.
//! Device-backed tests in sfx exercise the corresponding output lifetime.

use super::*;
use crate::rules::move_sound_tests::native;
use serde_json::{Value, json};

fn stock_request(corpus: &Value) -> PlayRequest {
    native_request(corpus, "SquidMove")
}

fn native_request(corpus: &Value, name: &str) -> PlayRequest {
    let sound = corpus["retail"]["sound_fields"]
        .as_array()
        .unwrap()
        .iter()
        .find(|sound| sound["name"] == name)
        .unwrap();
    PlayRequest {
        key: sound["name"].as_str().unwrap().to_ascii_uppercase(),
        facts: EntryFacts {
            priority: sound["priority"].as_i64().unwrap() as i32,
            limit: sound["limit"].as_i64().unwrap() as i32,
            control: sound["control"].as_u64().unwrap() as u32,
            loop_count: sound["loop_count"].as_i64().unwrap() as i32,
            delay_ms: (
                sound["delay_min"].as_i64().unwrap() as i32,
                sound["delay_max"].as_i64().unwrap() as i32,
            ),
            entry_volume_linear: (sound["volume_fixed16_raw"].as_i64().unwrap() >> 16) as i32,
        },
        volume_linear: VOLUME_SCALE / 2,
        pan: VOLUME_SCALE / 2,
    }
}

fn event_state(event: &EventRec) -> Value {
    let state = match event.state {
        EventState::NeedsChannel => 0,
        EventState::Ready => 1,
        EventState::PreDelay => 2,
        EventState::Playing => 3,
        EventState::Finished => 4,
    };
    json!({"state": state, "flags": event.flags, "serial": event.serial})
}

fn original_event_state(state: &Value) -> Value {
    json!({"state": state["state"], "flags": state["flags"], "serial": state["serial"]})
}

fn supplied_state(value: &Value) -> EventState {
    match value.as_u64().unwrap() {
        0 => EventState::NeedsChannel,
        1 => EventState::Ready,
        2 => EventState::PreDelay,
        3 => EventState::Playing,
        4 => EventState::Finished,
        other => panic!("unrepresented supplied event state {other}"),
    }
}

#[test]
fn move_sound_handle_cleanup_matches_original_tagged_records() {
    let corpus = native();
    let mut compared = 0;
    let mut excluded = 0;
    for row in corpus["handles"].as_array().unwrap() {
        let name = row["name"].as_str().unwrap();
        let input = &row["input"];
        if matches!(input["invalid"].as_str(), Some("tag" | "backend")) {
            // VERA has no raw audio-index pointer tag. An unavailable backend
            // has no SfxPlayer or arbiter at all; these native raw-pointer
            // controls are retained without inventing a representable state.
            excluded += 1;
            continue;
        }
        let mut arbiter = SoundArbiter::new(0);
        let mut request = stock_request(&corpus);
        if let Some(control) = input["control"].as_u64() {
            request.facts.control = control as u32;
        }
        if let Some(count) = input["loop_count"].as_i64() {
            request.facts.loop_count = count as i32;
        }
        let id = arbiter.submit(&request, 0).unwrap();
        // Same supplied record boundary as the native fixture, without a
        // channel/sample. These fields are inputs, not Rust-derived goldens.
        let before = &row["before"]["event_fields"][0];
        let event = arbiter.event_mut(id).unwrap();
        event.flags = before["flags"].as_u64().unwrap() as u32;
        event.serial = before["serial"].as_u64().unwrap() as u32;
        event.state = supplied_state(&before["state"]);
        arbiter.set_loop_handle(super::HandleOwner::Positional(7), Some(id), &request.key);
        let handle = arbiter
            .handles
            .get_mut(&super::HandleOwner::Positional(7))
            .unwrap();
        handle.serial = row["before"]["handle_fields"]["serial"].as_u64().unwrap() as u32;
        match input["invalid"].as_str() {
            Some("entry") => handle.entry = u32::MAX,
            Some("null") => handle.event = None,
            Some("serial") | None => {}
            other => panic!("{name}: unmapped invalid-handle input {other:?}"),
        }
        assert_eq!(
            event_state(arbiter.event(id).unwrap()),
            original_event_state(before),
            "{name}: supplied record"
        );
        match row["operation"].as_str().unwrap() {
            "decay_stop" | "foot_limbo_tail" => {
                arbiter.detach_owner(super::HandleOwner::Positional(7))
            }
            "release" | "foot_destructor_tail" => {
                arbiter.release_owner(super::HandleOwner::Positional(7))
            }
            "hard_stop" => {
                // SfxPlayer::stop_animation_sound's existing handle adapter;
                // the native expected state includes the actual Stop body.
                if let Some(event) = arbiter.validate_loop_handle(super::HandleOwner::Positional(7))
                {
                    arbiter.stop(event);
                }
                arbiter.clear_loop_handle(super::HandleOwner::Positional(7));
            }
            other => panic!("{name}: unexpected native operation {other}"),
        }
        assert_eq!(
            event_state(arbiter.event(id).unwrap()),
            original_event_state(&row["after"]["event_fields"][0]),
            "{name}: event lifetime"
        );
        assert_eq!(
            arbiter
                .handle_sound_key(super::HandleOwner::Positional(7))
                .is_some(),
            row["after"]["handle_fields"]["sound_present"]
                .as_bool()
                .unwrap(),
            "{name}: discarded sound identity"
        );
        // Native may retain an inert event pointer/serial after clearing the
        // sound pointer. VERA removes the handle instead; neither can name a
        // playable event or restart a loop. This is not raw-layout parity.
        assert_eq!(
            arbiter.validate_loop_handle(super::HandleOwner::Positional(7)),
            None,
            "{name}"
        );
        compared += 1;
    }
    assert_eq!((compared, excluded), (40, 10));
}

#[test]
fn queued_move_sound_admission_and_preemption_match_original_start_iteration() {
    let corpus = native();
    let rows = corpus["queued_playout"].as_array().unwrap();
    assert_eq!(rows.len(), 5);
    for row in rows {
        let name = row["name"].as_str().unwrap();
        let input = &row["input"];
        assert_eq!(input["preloaded_samples"], 1, "{name}");
        let original = row["before"]["foot"]["event_fields"].as_array().unwrap();
        let mut arbiter = SoundArbiter::new(0);
        let request = stock_request(&corpus);
        let current = arbiter.submit(&request, 0).unwrap();
        let mut ids = vec![current];
        for _ in input["victims"].as_array().unwrap() {
            let mut victim = native_request(&corpus, "GenLargeWaterDie");
            victim.facts.priority = input["native_victim_priority"].as_i64().unwrap() as i32;
            ids.push(arbiter.submit(&victim, 0).unwrap());
        }
        assert_eq!(ids.len(), original.len(), "{name}: supplied global list");
        for (&id, original) in ids.iter().zip(original) {
            // The native fixture begins after channel/sample admission. Give
            // Rust a channel through its existing owner at the same boundary.
            let priority = arbiter.effective_priority(arbiter.event(id).unwrap());
            let channel =
                (original["state"] != 4).then(|| arbiter.allocate_channel(id, priority).unwrap());
            let event = arbiter.event_mut(id).unwrap();
            event.channel = channel;
            event.flags = original["flags"].as_u64().unwrap() as u32;
            event.serial = original["serial"].as_u64().unwrap() as u32;
            event.state = supplied_state(&original["state"]);
            assert_eq!(event_state(event), original_event_state(original), "{name}");
        }
        if ids.len() > 1 {
            // Original supplied ranking boundary: all collateral records
            // share one low-priority Voc entry, including an already-dead one.
            let slot = input["native_victim_priority"].as_u64().unwrap() as usize * VOLUME_BUCKETS
                + input["victim_rank_bucket"].as_u64().unwrap() as usize;
            let entry = arbiter.event(ids[1]).unwrap().entry;
            arbiter.buckets[slot].push(entry);
            arbiter.entries[entry as usize].bucket_slot = Some(slot);
        }
        arbiter.set_loop_handle(
            super::HandleOwner::Positional(7),
            Some(current),
            &request.key,
        );
        match input["operation"].as_str() {
            Some("0x405fd0") => arbiter.detach_owner(super::HandleOwner::Positional(7)),
            Some("0x406060") => arbiter.release_owner(super::HandleOwner::Positional(7)),
            None => {}
            other => panic!("{name}: unmapped handle operation {other:?}"),
        }
        assert_eq!(
            event_state(arbiter.event(current).unwrap()),
            original_event_state(&row["after_handle"]["foot"]["event_fields"][0]),
            "{name}: original handle writes before the start iteration"
        );
        let mut actions = Vec::new();
        arbiter.start_pass(100, &mut super::TestPlayback::default(), &mut actions);
        let native_events = row["events"].as_array().unwrap();
        assert_eq!(
            actions.iter().any(|action| matches!(action,
                ArbiterAction::Start { event, .. } if *event == current)),
            native_events
                .iter()
                .any(|event| event["kind"] == "start_playback" && event["event_index"] == 0),
            "{name}: current-event playback admission"
        );
        let stopped: Vec<_> = actions
            .iter()
            .filter_map(|action| match action {
                ArbiterAction::Stop { event } => {
                    Some(ids.iter().position(|id| id == event).unwrap())
                }
                _ => None,
            })
            .collect();
        let returned: Vec<_> = native_events
            .iter()
            .filter(|event| event["kind"] == "return_to_pool")
            .map(|event| event["event_index"].as_u64().unwrap() as usize)
            .collect();
        assert_eq!(
            stopped, returned,
            "{name}: ordered collateral and current eviction"
        );
        let live: Vec<_> = arbiter
            .order
            .iter()
            .map(|id| ids.iter().position(|other| other == id).unwrap())
            .collect();
        assert_eq!(json!(live), row["after"]["live_event_indices"], "{name}");
        assert_eq!(
            json!(arbiter.live_event_count()),
            row["after"]["live_count"],
            "{name}"
        );
        // Native executes one current-event iteration and supplies the device
        // StartPlayback boundary. Rust may also admit the later ready victim;
        // neither that later visit nor device start fields are compared here.
        // Full UpdateTick and actual device playback have separate tests.
        if ids.len() == 1 {
            // Production integration of the same decision: the event starts
            // at submit's NeedsChannel state and reaches the gate through all
            // UpdateTick phases. No test-only preemption invocation.
            let mut full = SoundArbiter::new(0);
            let event = full.submit(&request, 0).unwrap();
            full.set_loop_handle(super::HandleOwner::Positional(7), Some(event), &request.key);
            match input["operation"].as_str() {
                Some("0x405fd0") => full.detach_owner(super::HandleOwner::Positional(7)),
                Some("0x406060") => full.release_owner(super::HandleOwner::Positional(7)),
                None => {}
                _ => unreachable!(),
            }
            let actions = full.update_tick(100, &mut super::TestPlayback::default());
            assert_eq!(
                actions
                    .iter()
                    .any(|action| matches!(action, ArbiterAction::Start { .. })),
                native_events
                    .iter()
                    .any(|event| event["kind"] == "start_playback"),
                "{name}: full UpdateTick admission"
            );
            assert_eq!(
                json!(full.live_event_count()),
                row["after"]["live_count"],
                "{name}"
            );
            assert_eq!(
                json!(full.busy_channel_count()),
                row["after"]["live_count"],
                "{name}: channel cleanup"
            );
        }
    }
}

#[test]
fn world_replacement_discards_live_and_inaudible_handles_before_pool_reuse() {
    let mut arbiter = SoundArbiter::new(0);
    let mut request = stock_request(&native());
    let outgoing = arbiter.submit(&request, 0).unwrap();
    arbiter.set_loop_handle(
        super::HandleOwner::Positional(7),
        Some(outgoing),
        &request.key,
    );
    request.key = "AUTHOREDLOOP".to_owned();
    request.facts.control = control::LOOP;
    let looped = arbiter.submit(&request, 0).unwrap();
    arbiter.set_loop_handle(
        super::HandleOwner::Positional(8),
        Some(looped),
        &request.key,
    );
    arbiter.keep_loop_sound(super::HandleOwner::Positional(9), &request.key, true);
    arbiter.update_tick(100, &mut super::TestPlayback::default());
    assert_eq!(arbiter.busy_channel_count(), 2);
    assert_eq!(
        arbiter.kept_loop_key(super::HandleOwner::Positional(9)),
        Some("AUTHOREDLOOP")
    );
    let serial = arbiter.serial;
    let catalog = arbiter.names.clone();
    let epochs = (arbiter.limit_epoch, arbiter.rank_epoch);

    // Rust integration regression for Load's original global Clear404E70.
    // Native control-flow evidence establishes all-event teardown; this test
    // catches stale kept handles restarting in the new world, not a full
    // native save/load or device comparison.
    arbiter.clear_for_world_replacement();
    assert_eq!(arbiter.live_event_count(), 0);
    assert_eq!(arbiter.busy_channel_count(), 0);
    assert!(arbiter.loop_handle_owners().is_empty());
    assert_eq!(arbiter.serial, serial);
    assert_eq!(arbiter.names, catalog);
    assert_eq!((arbiter.limit_epoch, arbiter.rank_epoch,), epochs);

    request.facts.control = 0;
    let loaded = arbiter.submit(&request, 101).unwrap();
    assert_eq!(loaded, outgoing, "exercise actual pool-slot reuse");
    for owner in [7, 8, 9] {
        assert_eq!(
            arbiter.validate_loop_handle(super::HandleOwner::Positional(owner)),
            None
        );
        assert_eq!(
            arbiter.kept_loop_key(super::HandleOwner::Positional(owner)),
            None
        );
    }
    let actions = arbiter.update_tick(140, &mut super::TestPlayback::default());
    assert_eq!(
        actions
            .iter()
            .filter(|action| matches!(action, ArbiterAction::Start { .. }))
            .count(),
        1
    );
    assert!(actions.iter().any(|action| matches!(action,
        ArbiterAction::Start { event, .. } if *event == loaded)));
    assert!(
        !actions
            .iter()
            .any(|action| matches!(action, ArbiterAction::Stop { .. }))
    );
}
