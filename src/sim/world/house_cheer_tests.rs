//! House50C8C0/Event2D production receivers with explicit initialized
//! Infantry/type/mission priors. Ordinary paid-map C ingress is a separate
//! app/GPU witness; this does not emulate native map startup or audio devices.

use super::*;
use crate::rules::ini_parser::IniFile;
use crate::rules::sound_ini::SoundRegistry;
use crate::sim::command::{CHEER_OPCODE, Command, CommandRecord};
use crate::sim::house_state::HouseState;
use crate::sim::mission::state::MissionTestFixture;
use crate::sim::mission::{MissionDispatchTimer, MissionId};

#[test]
fn native_cheer_record_resolves_registered_house_and_runs_the_shared_receiver() {
    let art = IniFile::from_str(
        "[ENGINEER]\nSequence=CheerSequence\n[CheerSequence]\nReady=100,6,6\nCheer=100,8,0,E\n",
    );
    let source = "[AudioVisual]\nCheerSound=Cheer\n";
    let ini = IniFile::from_str(source);
    let (mut sim, mut rules, _) =
        entry_test_fixture::fixture_with_rules_and_fixed_art(source, &art);
    rules.bind_animation_sequences(
        &crate::rules::infantry_sequence::parse_infantry_sequence_registry(&art),
    );
    rules.bind_type_sound_references(
        &ini,
        &SoundRegistry::from_ini(&IniFile::from_str("[SoundList]\n0=Cheer\n[Cheer]\n")),
    );
    let foreign = sim.intern("Foreign");
    let owner = sim.intern("Americans");
    for house in [foreign, owner] {
        sim.houses
            .register(HouseState::new(house, 0, None, true, 0, 10));
    }
    let mut ids = Vec::new();
    for house in [owner, foreign, owner] {
        let name = sim.resolve(house).to_owned();
        let id = sim
            .construct_object_limbo_at_height(
                "ENGINEER",
                &name,
                15,
                15,
                0,
                0,
                &rules,
                FrameEffects::default(),
            )
            .unwrap();
        // Mission/Doing are explicit initialized component inputs. Native
        // House50C8C0 supplies no limbo/selection gate before CanCheer.
        let actor = sim.substrate.entities.get_mut(id).unwrap();
        actor.mission.apply_test_fixture(MissionTestFixture {
            current: MissionId::from_raw(5),
            queued: MissionId::NONE,
            suspended: MissionId::NONE,
            movement_bypass_latch: 0,
            handler_state: 0,
            mission_start_frame: 0,
            ai_counter: 0,
            dispatch_timer: MissionDispatchTimer::at_frame(0),
        });
        actor.mission_leaf.set_infantry_doing_verified(0).unwrap();
        ids.push(id);
    }
    sim.sound_events.clear();
    sim.session.set_binary_frame_for_test(0xffff_fffe);
    let record = sim.encode_cheer_record(owner).unwrap();
    assert_eq!(
        (record.opcode(), record.house_id(), record.frame_stamp()),
        (CHEER_OPCODE, 1, -2)
    );
    sim.session.set_input_locked(true);
    let event = sim.decode_native_command_record(&record, 99).unwrap();
    assert_eq!(event.owner, owner);
    assert_eq!(event.payload, Command::Cheer);
    assert!(sim.apply_command("Americans", &event.payload, Some(&rules)));
    let doing = |id| {
        sim.substrate
            .entities
            .get(id)
            .unwrap()
            .mission_leaf
            .as_infantry()
            .unwrap()
            .doing()
    };
    assert_eq!([doing(ids[0]), doing(ids[1]), doing(ids[2])], [32, 0, 32]);
    assert_eq!(sim.sound_events.iter().filter(|event| matches!(event, SimSoundEvent::VocCentered { sound_id } if sound_id == "Cheer")).count(), 1);
    let invalid = CommandRecord::encode(CHEER_OPCODE, 7, 0, &[]).unwrap();
    assert!(sim.decode_native_command_record(&invalid, 99).is_none());
}
