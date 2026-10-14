//! Headless app-boundary transport checks. These exercise the production
//! dispatcher and its local/radar gates; they do not establish device parity.

use super::{FrameSoundBoundary, dispatch_sim_sound_events};
use crate::audio::events::SoundEventQueue;
use crate::rules::ini_parser::IniFile;
use crate::rules::ruleset::RuleSet;
use crate::sim::radar::{RadarEventRequest, RadarEventType};
use crate::sim::world::{SimSoundEvent, Simulation, SoundBoundary};

/// Original6D2140 controls execute the registered Tactical startup writer,
/// signed XY arithmetic, height rounding and view subtraction. The app's
/// existing projection owner supplies the retained750E20 request coordinate;
/// VERA's world-pixel Y and camera Y both include WORLD_ROW_BIAS_PX.
#[test]
fn coordinate_request_sources_match_executed_native_projection_controls() {
    let fixture: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/building_damage_sound_joined.json",
    ))
    .unwrap();
    let controls = fixture["receiver_controls"]["projection_controls"]
        .as_array()
        .unwrap();
    assert_eq!(controls.len(), 24);
    let rules = RuleSet::from_ini(&IniFile::from_str("")).unwrap();
    let sim = Simulation::new();
    let before_rng = sim.rng_state();
    let mut output = SoundEventQueue::new();
    for control in controls {
        let input = &control["input"];
        let axis = |index: usize| input["coordinate"][index].as_i64().unwrap() as i32;
        dispatch_sim_sound_events(
            [SimSoundEvent::VocAtCoord {
                sound_id: "NativeProjectionControl".into(),
                coordinate: crate::sim::components::DriveCoord {
                    x: axis(0),
                    y: axis(1),
                    z: axis(2),
                },
                flags: 0,
            }],
            &sim,
            &rules,
            None,
            &mut |_| panic!("coordinate requests have no radar admission"),
            &mut output,
        );
        let events = output.drain();
        assert_eq!(events.len(), 1);
        assert!(matches!(
            &events[0],
            crate::audio::events::GameSoundEvent::VocAtCoord { .. }
        ));
        let source = events[0].source().unwrap();
        let view = &input["listener"]["view_xy"];
        let expected_x = control["output"][0].as_i64().unwrap() + view[0].as_i64().unwrap();
        let (_, world_row_origin) = crate::util::lepton::absolute_leptons_to_screen(0, 0, 0);
        let expected_y = (control["output"][1].as_i64().unwrap() + view[1].as_i64().unwrap())
            as f32
            + world_row_origin;
        assert_eq!(
            source.screen_pos(),
            (expected_x as f32, expected_y),
            "native projection input {:?}",
            input["coordinate"]
        );
    }
    assert_eq!(sim.rng_state(), before_rng);
}

#[test]
fn coordinate_stop_transport_keeps_exact_words_flags_and_action_order() {
    let rules = RuleSet::from_ini(&IniFile::from_str("")).unwrap();
    let sim = Simulation::new();
    let coordinate = crate::sim::components::DriveCoord {
        x: -257,
        y: 1537,
        z: 104,
    };
    let mut output = SoundEventQueue::new();
    dispatch_sim_sound_events(
        [
            SimSoundEvent::VocAtCoord {
                sound_id: "First".into(),
                coordinate,
                flags: 3,
            },
            SimSoundEvent::VocStopAtCoord {
                coordinate,
                flags: 1,
            },
            SimSoundEvent::VocAtCoord {
                sound_id: "Later".into(),
                coordinate,
                flags: 0,
            },
        ],
        &sim,
        &rules,
        None,
        &mut |_| panic!("coordinate stop has no radar or owner gate"),
        &mut output,
    );
    let events = output.drain();
    assert!(matches!(events.as_slice(), [
        crate::audio::events::GameSoundEvent::VocAtCoord { sound_id: first, coordinate: a, flags: 3, .. },
        crate::audio::events::GameSoundEvent::VocStopAtCoord { coordinate: b, flags: 1 },
        crate::audio::events::GameSoundEvent::VocAtCoord { sound_id: later, coordinate: c, flags: 0, .. },
    ] if first == "First" && later == "Later" && *a == [-257, 1537, 104] && a == b && b == c));
}

#[test]
fn frame_boundary_consumes_each_prefix_once_and_preserves_later_facts() {
    let rules = RuleSet::from_ini(&IniFile::from_str("")).unwrap();
    let mut sim = Simulation::new();
    sim.session.tick = 17;
    let local = sim.interner.intern("Local");
    let remote = sim.interner.intern("Remote");
    let before_main = sim.rng_views().main.native_state_hex();
    let repaired = |owner, rx| SimSoundEvent::UnitRepaired {
        owner,
        radar: RadarEventRequest::new(RadarEventType::UnitRepaired, rx, 7),
    };
    let mut facts = vec![repaired(remote, 1), repaired(local, 2), repaired(local, 99)];
    let mut output = SoundEventQueue::new();
    let mut admitted = Vec::new();
    let delivered;
    {
        let mut admit_radar = |request: RadarEventRequest, tick, bound_rules: &RuleSet| {
            assert!(std::ptr::eq(bound_rules, &rules));
            admitted.push((tick, request.rx));
            request.rx != 99
        };
        // Absent output is the ordinary headless/audio-unavailable path.
        // Cue/radar conversion still runs through the production dispatcher.
        let boundary = FrameSoundBoundary::new(None, Some("LOCAL"), &mut output, &mut admit_radar);
        boundary.consume(&sim, &rules, &facts);
        boundary.consume(&sim, &rules, &facts);
        facts.push(SimSoundEvent::UnitVoiceVisit { owner: 41 });
        boundary.consume(&sim, &rules, &facts);
        facts.push(repaired(local, 3));
        boundary.consume(&sim, &rules, &facts);
        delivered = boundary.delivered();
    }
    assert_eq!(admitted, [(17, 2), (17, 99), (17, 3)]);
    assert_eq!(delivered, 5);
    assert_eq!(facts.len(), 5, "inline audio must preserve the frame facts");
    assert!(matches!(
        facts[3],
        SimSoundEvent::UnitVoiceVisit { owner: 41 }
    ));
    assert!(
        output.is_empty(),
        "inline delivery drains the existing queue"
    );

    // The ordinary post-frame dispatcher takes only the unconsumed suffix.
    facts.push(repaired(local, 4));
    dispatch_sim_sound_events(
        facts.into_iter().skip(delivered),
        &sim,
        &rules,
        Some("LOCAL"),
        &mut |request| {
            admitted.push((sim.session.tick, request.rx));
            true
        },
        &mut output,
    );
    assert_eq!(admitted, [(17, 2), (17, 99), (17, 3), (17, 4)]);
    assert_eq!(output.drain().len(), 1);

    // A later frame starts a new borrowed scope, with no retained frontier.
    sim.session.tick = 18;
    {
        let mut admit_radar = |request: RadarEventRequest, tick, _: &RuleSet| {
            admitted.push((tick, request.rx));
            true
        };
        let boundary = FrameSoundBoundary::new(None, Some("LOCAL"), &mut output, &mut admit_radar);
        boundary.consume(&sim, &rules, &[repaired(local, 2)]);
        assert_eq!(boundary.delivered(), 1);
    }
    assert_eq!(admitted.last(), Some(&(18, 2)));
    assert_eq!(sim.rng_views().main.native_state_hex(), before_main);
}
