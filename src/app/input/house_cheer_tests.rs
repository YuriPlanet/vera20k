//! Original AllToCheer536C00 ->730F30 admission histories. The bounded
//! OutList comparison connects the production input clock to the existing
//! SynchronizedCommandQueue explicitly; ordinary app scheduling is tested
//! separately and retains its current unsaturated-ingress bound. Ring storage
//! indices and timeGetTime imports are native observations, not Rust layout
//! or wall-clock claims. No House receiver or device audio runs in this leaf.

use super::{MatchInputState, PreparedSavedLocalView};
use crate::app::input::commands::queue_house_cheer_in_sim;
use crate::app::input::hotkeys::{HotkeyBindings, HotkeyCommand, HotkeyResolution};
use crate::net::lockstep::{SynchronizedCommand, SynchronizedCommandQueue};
use crate::rules::{ini_parser::IniFile, ruleset::RuleSet};
use crate::sim::command::{Command, CommandRecord};
use crate::sim::house_state::HouseState;
use crate::sim::intern::InternedId;
use crate::sim::world::Simulation;
use crate::ui::sidebar::gadget_flash::SidebarGadgetState;
use crate::util::save_codec;
use serde_json::{Value, json};
use winit::keyboard::{Key, KeyLocation, ModifiersState};

fn corpus() -> &'static Value {
    crate::test_fixture::json("tools/input_oracle/house_cheer.json")
}

fn fixture(house_index: usize) -> (Simulation, InternedId) {
    let mut sim = Simulation::new();
    for index in 0..=house_index {
        let owner = sim.intern(&format!("CheerFixture{index}"));
        sim.houses_mut_for_test().register(HouseState::new(
            owner,
            0,
            None,
            index == house_index,
            0,
            10,
        ));
    }
    let owner = sim.houses().order()[house_index];
    sim.session_mut_for_test()
        .set_current_house_for_test(Some(owner));
    // A deliberately distinct ordinal proves that admission reads binary
    // frames, while the ordinary scheduler keeps its own due-tick owner.
    sim.session_mut_for_test().set_tick_for_test(10_000);
    (sim, owner)
}

fn rules_with_rate(rate: i64) -> RuleSet {
    RuleSet::from_ini(&IniFile::from_str(&format!(
        "[General]\nMaximumCheerRate={rate}\n"
    )))
    .unwrap()
}

fn event_header(record: &CommandRecord) -> Value {
    json!({
        "opcode": record.opcode(),
        "house_index": record.house_id(),
        "frame": record.frame_stamp(),
    })
}

#[test]
fn all_to_cheer_matches_original_admission_outlist_and_reset_histories() {
    let controls = corpus();
    assert_eq!(controls["schema_version"], 1);
    let cases = controls["cases"].as_array().unwrap();
    let mut visits = 0;
    let mut presses = 0;
    let mut constructors = 0;
    let mut appended = 0;
    for case in cases {
        let initial = &case["input"];
        let (mut sim, owner) = fixture(initial["house_index"].as_u64().unwrap() as usize);
        let mut rules = rules_with_rate(initial["maximum_cheer_rate"].as_i64().unwrap());
        let mut input = MatchInputState::new(HotkeyBindings::default());
        input.last_cheer_frame = initial["last_frame"].as_i64().unwrap() as i32;
        // Historical corpus name: input_locked is the native Session+300
        // dword atA8B538, not Scenario+35A2. These explicit component priors
        // are separate from the real-loader local-loss regression.
        sim.houses_mut_for_test()
            .get_mut(&owner)
            .unwrap()
            .set_is_defeated_for_test(initial["input_locked"].as_u64().unwrap() != 0);
        assert!(!sim.session().input_locked());
        let mut queue = SynchronizedCommandQueue::new();
        for _ in 0..initial["queue_count"].as_u64().unwrap() {
            assert!(
                queue.issue(SynchronizedCommand::opaque(
                    CommandRecord::encode(
                        0x15,
                        initial["house_index"].as_i64().unwrap() as i32,
                        0,
                        &[]
                    )
                    .unwrap(),
                ))
            );
        }
        for (ordinal, phase) in case["phases"].as_array().unwrap().iter().enumerate() {
            let label = format!("{} phase{ordinal}", case["name"]);
            let boundary = &phase["input"];
            let expected = &phase["expected"];
            sim.session_mut_for_test()
                .set_binary_frame_for_test(boundary["current_frame"].as_i64().unwrap() as u32);
            if let Some(session300) = boundary["input_locked"].as_u64() {
                sim.houses_mut_for_test()
                    .get_mut(&owner)
                    .unwrap()
                    .set_is_defeated_for_test(session300 != 0);
            }
            if let Some(count) = boundary["queue_count"].as_u64() {
                // This history declares an external OutList drain between
                // presses. Use the real transfer owner rather than a test ring.
                assert_eq!(count, 0, "{label}: only the declared drain is supplied");
                queue.transfer_single_player();
            }
            let before = queue.local_len();
            let mut constructed = Vec::new();
            match boundary["kind"].as_str().unwrap() {
                "press" => {
                    presses += 1;
                    let issued = input.try_issue_cheer(
                        sim.session().binary_frame() as i32,
                        rules.general.maximum_cheer_rate(),
                        sim.local_player_defeated(),
                        || {
                            let record = sim.encode_cheer_record(owner).unwrap();
                            constructed.push(json!({
                                "opcode": record.opcode(),
                                "house_index": record.house_id(),
                            }));
                            queue
                                .issue(SynchronizedCommand::opaque(record))
                                .then_some(sim.session().tick())
                        },
                    );
                    assert_eq!(
                        issued,
                        (!expected["new_events"].as_array().unwrap().is_empty())
                            .then_some(sim.session().tick()),
                        "{label}: admitted append"
                    );
                }
                "reset" => input.reset_targeting_for_new_world(),
                "constructor_rate" => {
                    rules = RuleSet::from_ini(&IniFile::from_str("[General]\n")).unwrap();
                }
                kind => panic!("{label}: unknown original phase {kind}"),
            }
            let actual_events: Vec<_> = queue
                .local_records()
                .skip(before)
                .map(|command| event_header(command.record()))
                .collect();
            let expected_events: Vec<_> = expected["new_events"]
                .as_array()
                .unwrap()
                .iter()
                .map(|record| {
                    json!({
                        "opcode": record["opcode"],
                        "house_index": record["house_index"],
                        "frame": record["frame"],
                    })
                })
                .collect();
            assert_eq!(
                actual_events, expected_events,
                "{label}: native event headers"
            );
            assert_eq!(
                constructed,
                *expected["constructor_calls"].as_array().unwrap(),
                "{label}: construction even while full"
            );
            assert_eq!(
                i64::from(input.last_cheer_frame),
                expected["last_frame"].as_i64().unwrap(),
                "{label}: retained input admission frame"
            );
            assert_eq!(
                i64::from(rules.general.maximum_cheer_rate()),
                expected["maximum_cheer_rate"].as_i64().unwrap(),
                "{label}: constructor/read value"
            );
            assert_eq!(
                queue.local_len() as u64,
                expected["queue_count"].as_u64().unwrap(),
                "{label}: authoritative OutList count"
            );
            visits += 1;
            constructors += constructed.len();
            appended += actual_events.len();
        }
    }
    assert_eq!(
        (cases.len(), visits, presses, constructors, appended),
        (15, 34, 32, 15, 14)
    );
}

#[test]
fn ordinary_cheer_uses_binary_frame_admission_then_the_shared_due_scheduler() {
    let history = &corpus()["cases"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["name"] == "stock_initial_boundary")
        .unwrap();
    let (mut sim, owner) = fixture(history["input"]["house_index"].as_u64().unwrap() as usize);
    let owner_name = sim.interner().resolve(owner).to_owned();
    let rules = rules_with_rate(history["input"]["maximum_cheer_rate"].as_i64().unwrap());
    let mut input = MatchInputState::new(HotkeyBindings::reload_from_ini_bytes(Some(
        b"[Hotkey]\nAllToCheer=67\n",
    )));
    assert_eq!(
        input.hotkey_bindings.resolve_event(
            &Key::Character("c".into()),
            KeyLocation::Standard,
            ModifiersState::empty(),
        ),
        HotkeyResolution::Command(HotkeyCommand::AllToCheer),
        "the declared stock binding resolves through the production reader"
    );
    let before_rng = sim.rng_state();
    for phase in history["phases"].as_array().unwrap() {
        sim.session_mut_for_test()
            .set_binary_frame_for_test(phase["input"]["current_frame"].as_i64().unwrap() as u32);
        let expected = &phase["expected"];
        let issued = queue_house_cheer_in_sim(&mut sim, &rules, &mut input, &owner_name);
        assert_eq!(
            issued,
            (!expected["new_events"].as_array().unwrap().is_empty())
                .then_some(sim.session().tick()),
        );
        let pending = sim.pending_command_snapshot();
        assert_eq!(
            pending.len() as u64,
            expected["queue_count"].as_u64().unwrap()
        );
        for envelope in pending {
            assert_eq!(envelope.owner, owner);
            assert_eq!(envelope.execute_tick, sim.session().tick());
            assert_eq!(envelope.payload, Command::Cheer);
        }
        assert_eq!(
            i64::from(input.last_cheer_frame),
            expected["last_frame"].as_i64().unwrap()
        );
    }
    assert_eq!(
        sim.rng_state(),
        before_rng,
        "local event issue draws no simulation RNG"
    );
    assert_eq!(sim.take_due_commands().len(), 2);
    assert!(sim.pending_command_snapshot().is_empty());
}

#[test]
fn saved_local_view_commit_resets_cheer_admission_instead_of_restoring_it() {
    let (mut sim, owner) = fixture(7);
    let owner_name = sim.interner().resolve(owner).to_owned();
    let rules = rules_with_rate(300);
    let mut source = MatchInputState::new(Default::default());
    source.last_cheer_frame = 77;
    let saved = source
        .capture_saved_local_view(&sim, &rules, &SidebarGadgetState::new())
        .unwrap();
    let saved = save_codec::deserialize(&save_codec::serialize(&saved).unwrap()).unwrap();
    let mut outgoing = MatchInputState::new(Default::default());
    outgoing.last_cheer_frame = 123_456;
    let prepared = PreparedSavedLocalView::new(saved, &sim, &rules).unwrap();
    assert_eq!(
        outgoing.last_cheer_frame, 123_456,
        "preparation leaves outgoing input intact"
    );
    prepared.commit_into(&mut outgoing, &mut SidebarGadgetState::new());

    let reset_history = corpus()["cases"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["name"] == "scene_reset_stock_rate")
        .unwrap();
    assert_eq!(
        i64::from(outgoing.last_cheer_frame),
        reset_history["phases"][0]["expected"]["last_frame"]
            .as_i64()
            .unwrap(),
        "PostLoad734210 uses the original reset value"
    );
    for phase in reset_history["phases"].as_array().unwrap().iter().skip(1) {
        sim.session_mut_for_test()
            .set_binary_frame_for_test(phase["input"]["current_frame"].as_i64().unwrap() as u32);
        let issued = queue_house_cheer_in_sim(&mut sim, &rules, &mut outgoing, &owner_name);
        assert_eq!(
            issued.is_some(),
            !phase["expected"]["new_events"]
                .as_array()
                .unwrap()
                .is_empty(),
        );
        assert_eq!(
            i64::from(outgoing.last_cheer_frame),
            phase["expected"]["last_frame"].as_i64().unwrap(),
        );
    }
}
