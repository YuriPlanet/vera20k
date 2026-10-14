//! Retained750E20 requests and the shared7509E0 port, compared with original
//! executable controls. Device creation is optional on headless platforms;
//! admission tests do not claim natural device playback or sample scheduling.

use super::*;
use crate::audio::arbiter::coordinate_tests::{control, native};

pub(super) fn cue(input: &str) -> &'static str {
    let index = &native()["audio_core_controls"]["input"][input];
    native()["selected_sounds"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["fixture_index"] == *index)
        .unwrap()["name"]
        .as_str()
        .unwrap()
}

pub(super) fn source() -> SoundSource {
    let [x, y, z] = coordinate();
    SoundSource::new(
        crate::util::lepton::absolute_leptons_to_screen(x, y, z),
        ((x >> 8) as u16, (y >> 8) as u16),
    )
}

pub(super) fn coordinate() -> [i32; 3] {
    let coordinate = &native()["audio_core_controls"]["input"]["coordinate"];
    std::array::from_fn(|index| coordinate[index].as_i64().unwrap() as i32)
}

pub(super) fn request_flags() -> u32 {
    native()["audio_core_controls"]["input"]["caller_argument"]
        .as_u64()
        .unwrap() as u32
}

pub(super) fn gain(label: &str, sound: &str, registry: &SoundRegistry) -> Option<SpatialGain> {
    let listener = &control(label)["state"]["listener"];
    let view = &listener["view_xy"];
    let viewport = &listener["viewport_886fa8_fac"];
    let (_, world_row_origin) = crate::util::lepton::absolute_leptons_to_screen(0, 0, 0);
    let listener = SpatialListener {
        tactical_width: viewport[0].as_i64().unwrap() as i32,
        tactical_height: viewport[1].as_i64().unwrap() as i32,
        origin_x: view[0].as_i64().unwrap() as f32,
        origin_y: view[1].as_i64().unwrap() as f32 + world_row_origin,
        zoom: 1.0,
    };
    let source = source();
    let (x, y) = listener.client_point(source.screen_x, source.screen_y);
    let (width, height) = listener.view_extent();
    calc_volume_and_pan(
        x,
        y,
        width,
        height,
        SpatialSource::from_entry(registry.get(sound).unwrap()),
        false,
    )
}

pub(super) fn with_player(
    run: impl FnOnce(
        &mut SfxPlayer,
        &SoundRegistry,
        &AssetManager,
        Option<&crate::assets::audio_bag::AudioIndex>,
    ),
) {
    let Some((_root, assets)) = crate::rules::retail_ini_fixture::retail_assets() else {
        return;
    };
    let Some(mut player) = SfxPlayer::new() else {
        return;
    };
    player.set_volume(0.0);
    let definitions = crate::rules::audio_sources::AudioDefinitions::select(&assets);
    let selected_index = assets.load_audio_index().unwrap();
    run(
        &mut player,
        definitions.sounds(),
        &assets,
        selected_index.as_ref().map(|selected| &selected.index),
    );
    player.stop_all();
}

fn pending_controller(label: &str, set_spatial_volume: bool) {
    let Some((_root, assets)) = crate::rules::retail_ini_fixture::retail_assets() else {
        return;
    };
    let definitions = crate::rules::audio_sources::AudioDefinitions::select(&assets);
    let registry = definitions.sounds();
    let sound = cue("sound_index");
    let spatial = gain("position_first", sound, registry).unwrap();
    let facts = EntryFacts::from(registry.get(sound).unwrap());
    let row = control(label);
    let now = row["state"]["pump_last_ms"].as_u64().unwrap();
    let mut arbiter = SoundArbiter::new(now);
    let event = arbiter
        .submit(
            &PlayRequest {
                key: registry_key(sound),
                facts,
                volume_linear: combine_linear(spatial.volume_linear(), facts.entry_volume_linear),
                pan: spatial.pan,
            },
            now,
        )
        .unwrap();
    if set_spatial_volume {
        // Original7509E0 reuses this event and supplies spatial volume to
        // 4061D0, which owns multiplication by SoundEntry+1C. No channel or
        // device is needed to compare the recorded pending controller.
        arbiter.set_volume(event, spatial.volume_linear(), now);
    }
    let expected = row["state"]["events"]
        .as_array()
        .unwrap()
        .iter()
        .find(|event| event["pointer"] == row["returned_event"])
        .unwrap();
    let actual = arbiter.native_event_snapshot(event).unwrap();
    let fields: &[(&str, &str, usize)] = if set_spatial_volume {
        &[
            ("volume_current_fixed", "volume_controller_words", 1),
            ("volume_target_fixed", "volume_controller_words", 2),
        ]
    } else {
        &[
            ("pan_current_fixed", "pan_controller_words", 1),
            ("pan_target_fixed", "pan_controller_words", 2),
        ]
    };
    for &(field, words, index) in fields {
        assert_eq!(actual[field], expected[words][index], "{label}: {field}");
    }
}

#[test]
fn pending_pan_preserves_native_current_until_sound_service_without_a_device() {
    pending_controller("position_first", false);
}

#[test]
fn pending_redrive_retains_native_entry_volume_without_a_device() {
    pending_controller("position_same_entry_reuses", true);
}

#[test]
fn same_entry_reuses_event_and_invalid_name_clears_the_positional_handle() {
    with_player(|player, registry, _assets, _index| {
        let owner = HandleOwner::Positional(7);
        let sound = cue("sound_index");
        let gain = gain("position_first", sound, registry).unwrap();
        assert!(player.play_animation_sound_spatial(owner, sound, gain, registry));
        let event = player.arbiter.validate_loop_handle(owner).unwrap();
        let token = player.arbiter.event_token(event).unwrap();
        assert!(player.play_animation_sound_spatial(owner, sound, gain, registry));
        assert_eq!(player.arbiter.validate_loop_handle(owner), Some(event));
        assert_eq!(player.arbiter.event_token(event), Some(token));
        assert_eq!(
            control("position_first")["handle"],
            control("position_same_entry_reuses")["handle"]
        );
        let other = cue("other_sound_index");
        assert!(player.play_animation_sound_spatial(owner, other, gain, registry));
        assert!(!player.arbiter.is_live_token(token));
        assert_ne!(
            control("position_first")["returned_event"],
            control("position_different_entry_stops_old")["returned_event"]
        );
        // A name lookup failure is native -1. Raw positive invalid ordinals
        // are outside this bound-name API; audio-bag names are not Voc IDs.
        assert!(registry.get("bmetdama").is_none());
        assert!(!player.play_animation_sound_spatial(owner, "bmetdama", gain, registry));
        assert_eq!(control("position_negative_id_unbinds")["handle"][0], 0);
        assert!(player.arbiter.validate_loop_handle(owner).is_none());
        assert!(player.arbiter.handle_sound_key(owner).is_none());
    });
}

#[test]
fn full_shared_pool_keeps_both_caller_bindings_and_retries_after_dead_reap() {
    with_player(|player, registry, _assets, _index| {
        let sound = cue("sound_index");
        let capacity = control("native300event_pool_full")["returns"]
            .as_array()
            .unwrap()
            .len();
        for _ in 0..capacity {
            assert!(player.play_registered_sound_spatial(
                sound,
                SpatialGain::CENTRED_FULL,
                registry
            ));
        }
        let initial: Vec<_> = player.pending.keys().copied().collect();
        let position = HandleOwner::Positional(7);
        let gain = gain("full_pool_position_bind", sound, registry).unwrap();
        assert!(!player.play_animation_sound_spatial(position, sound, gain, registry));
        let voice = HandleOwner::UnitVoice(7);
        player.queue_unit_voice(7, sound);
        player.visit_unit_voice(7, registry);
        for (owner, label) in [
            (position, "full_pool_position_bind"),
            (voice, "full_pool_nonpositional_bind"),
        ] {
            assert_eq!(control(label)["returned_event"], 0);
            assert!(player.arbiter.validate_loop_handle(owner).is_none());
            assert_eq!(
                player.arbiter.handle_sound_key(owner),
                Some(registry_key(sound).as_str())
            );
        }
        // The native controls invoke original4052F0 directly on these two
        // queued events; no native DEAD or allocation answer is supplied.
        player.arbiter.stop(initial[0]);
        player.release_output(initial[0]);
        assert!(player.play_animation_sound_spatial(position, sound, gain, registry));
        assert_ne!(control("dead_only_position_retry")["returned_event"], 0);
        player.arbiter.stop(initial[1]);
        player.release_output(initial[1]);
        player.queue_unit_voice(7, sound);
        player.visit_unit_voice(7, registry);
        assert!(player.arbiter.validate_loop_handle(voice).is_some());
        assert_ne!(
            control("dead_only_nonpositional_retry")["returned_event"],
            0
        );
        assert_eq!(player.arbiter.live_event_count(), capacity);
    });
}

#[test]
fn inaudible_requests_use_the_independent_native200_pool_and_reap_in_order() {
    with_player(|player, registry, _assets, _index| {
        let sound = cue("sound_index");
        let source = source();
        assert!(!player.play_sound_at_coord(
            "bmetdama",
            coordinate(),
            request_flags(),
            source,
            None,
            registry
        ));
        assert_eq!(
            control("negative_request_no_node")["state"]["coordinate_request_count"],
            0
        );
        let count = control("native200request_pool_full")["state"]["coordinate_request_count"]
            .as_u64()
            .unwrap() as usize;
        for _ in 0..count {
            assert!(player.play_sound_at_coord(
                sound,
                coordinate(),
                request_flags(),
                source,
                None,
                registry
            ));
        }
        assert!(!player.play_sound_at_coord(
            sound,
            coordinate(),
            request_flags(),
            source,
            None,
            registry
        ));
        assert_eq!(player.coordinate_requests.len(), count);
        assert!(player.pending.is_empty());
        let rows = player.coordinate_sound_observation();
        assert_eq!(rows.len(), count);
        assert!(
            rows.windows(2)
                .all(|pair| pair[0]["request_id"].as_u64() < pair[1]["request_id"].as_u64())
        );
        // The source actor's positional handle is a separate owner even
        // when its numeric ID equals the first coordinate request ID.
        player.stop_animation_sound(1_u64);
        assert_eq!(player.coordinate_sound_observation(), rows);
        player.service_coordinate_sounds(registry);
        assert_eq!(
            player.coordinate_requests.len() as u64,
            control("inaudible200requests_reaped")["state"]["coordinate_request_count"]
                .as_u64()
                .unwrap()
        );
        assert!(player.coordinate_sound_observation().is_empty());
    });
}

#[test]
fn retained_request_depths_and_newborn_depth_match_executed_nested_pause() {
    with_player(|player, registry, _assets, _index| {
        let sound = cue("sound_index");
        let source = source();
        let gain = gain("coordinate_before_nested_pause", sound, registry).unwrap();
        assert!(player.play_sound_at_coord(
            sound,
            coordinate(),
            request_flags(),
            source,
            Some(gain),
            registry
        ));
        // Native whole GamePause can nest. Compare the existing two owning
        // passes directly; the app's public bool setter separately coalesces
        // repeated per-frame reports of the same pause edge.
        for label in ["after_game_pause_first", "after_game_pause_second"] {
            player.arbiter.suspend_all(1000);
            player.suspend_coordinate_sounds(1000);
            assert_eq!(
                i64::from(player.coordinate_requests[0].suspend_depth),
                control(label)["state"]["coordinate_requests"][0]["suspend_depth"]
                    .as_i64()
                    .unwrap()
            );
        }
        assert!(player.play_sound_at_coord(
            sound,
            coordinate(),
            request_flags(),
            source,
            Some(gain),
            registry
        ));
        assert_eq!(
            i64::from(player.coordinate_requests[1].suspend_depth),
            control("new_request_during_nested_pause_depth_zero")["state"]["coordinate_requests"]
                [1]["suspend_depth"]
                .as_i64()
                .unwrap()
        );
        assert!(
            player
                .arbiter
                .validate_loop_handle(HandleOwner::Coordinate(2))
                .is_some()
        );
        for label in [
            "after_game_resume_first",
            "after_game_resume_second",
            "after_game_resume_extra",
        ] {
            player.resume_coordinate_sounds(1068);
            player.arbiter.resume_all(1068);
            let requests = &control(label)["state"]["coordinate_requests"];
            for (index, request) in player.coordinate_requests.iter().enumerate() {
                assert_eq!(
                    i64::from(request.suspend_depth),
                    requests[index]["suspend_depth"].as_i64().unwrap()
                );
                assert_eq!(request.source, source);
            }
        }
        player.stop_all();
        assert!(player.coordinate_requests.is_empty());
        assert!(player.pending.is_empty());
        assert!(player.live.is_empty());
    });
}

/// Rust regression for750F40's instruction-level selector, using a registered
/// retail cue and the already executed750E20 source.405FD0's existing arbiter
/// controls establish detach; this does not claim a natural device endpoint.
#[test]
fn coordinate_stop_matches_full_xyz_and_intersecting_flags_without_hard_stop() {
    with_player(|player, registry, _assets, _index| {
        let sound = cue("sound_index");
        let coordinate = coordinate();
        let gain = gain("coordinate_before_nested_pause", sound, registry).unwrap();
        // Each unmatched selector preserves the request's handle. Projected
        // source is deliberately identical: spatial cache never decides stop.
        for (requested, flags) in [
            ([coordinate[0] + 1, coordinate[1], coordinate[2]], 1),
            ([coordinate[0], coordinate[1] + 1, coordinate[2]], 1),
            ([coordinate[0], coordinate[1], coordinate[2] + 1], 1),
            (coordinate, 2),
            (coordinate, 0),
        ] {
            player.stop_all();
            assert!(player.play_sound_at_coord(
                sound,
                coordinate,
                1,
                source(),
                Some(gain),
                registry,
            ));
            let owner = HandleOwner::Coordinate(player.coordinate_requests[0].id);
            let before = player.arbiter.validate_loop_handle(owner).unwrap();
            player.stop_sound_at_coord(requested, flags);
            assert_eq!(player.arbiter.validate_loop_handle(owner), Some(before));
            assert_eq!(player.coordinate_requests.len(), 1);
        }

        player.stop_all();
        let mut events = Vec::new();
        for flags in [1, 3] {
            assert!(player.play_sound_at_coord(
                sound,
                coordinate,
                flags,
                source(),
                Some(gain),
                registry,
            ));
            let owner = HandleOwner::Coordinate(player.coordinate_requests.last().unwrap().id);
            events.push((owner, player.arbiter.validate_loop_handle(owner).unwrap()));
        }
        let live = player.arbiter.live_event_count();
        player.stop_sound_at_coord(coordinate, 1);
        assert_eq!(player.coordinate_requests.len(), 2, "750F40 retains nodes");
        assert_eq!(
            player.arbiter.live_event_count(),
            live,
            "405FD0 is not a hard stop"
        );
        for (owner, event) in events {
            assert!(player.arbiter.validate_loop_handle(owner).is_none());
            let snapshot = player.arbiter.native_event_snapshot(event).unwrap();
            assert_eq!(snapshot["flags"].as_u64().unwrap() & 0x60, 0x60);
        }
        player.service_coordinate_sounds(registry);
        assert!(
            player.coordinate_requests.is_empty(),
            "750EC0 owns retirement"
        );
    });
}
