//! Real local loss -> AllToCheer refusal during the ordinary SavourDelay.
//! Original1cdd730F30 reads Session+300/A8B538 before rate/Event/stamp;
//! Scenario+35A2 is the distinct cinematic input-lock authority.

use super::MatchInputState;
use crate::app::input::commands::{queue_house_cheer_in_sim, schedule_command_in_sim};
use crate::app::input::hotkeys::{HotkeyBindings, HotkeyCommand, HotkeyResolution};
use crate::headless_scenario::{HeadlessScenario, SIM_TICK_MS, battle_session, load_session};
use crate::sim::command::Command;
use crate::sim::world::{FrameEffects, SimSoundEvent, TickLane};
use crate::skirmish_launch::{AiDifficulty, LaunchCountry, LaunchStartPosition};
use winit::keyboard::{Key, KeyLocation, ModifiersState};

#[test]
#[ignore = "slow; paired retail local-loss and Cheer histories"]
fn actual_local_loss_refuses_eligible_cheer_without_consuming_the_input_clock() {
    let Some((root, _)) = crate::rules::retail_ini_fixture::retail_assets() else {
        return;
    };
    let mut scenes: [HeadlessScenario; 2] = std::array::from_fn(|_| {
        let mut launch = battle_session(
            "XMP03T4.MAP",
            &[(LaunchCountry::America, AiDifficulty::Easy)],
        );
        launch.local_slot_mut().country = LaunchCountry::Russia;
        launch.local_slot_mut().start_position = LaunchStartPosition::Position(1);
        launch.opponents[0].start_position = LaunchStartPosition::Position(2);
        launch.options.starting_credits = 5000;
        launch.options.unit_count = 0;
        launch.options.bases = true;
        launch.options.tech_level = 10;
        launch.options.short_game = true;
        launch.options.crates = false;
        launch.options.shroud = false;
        load_session(&root, "XMP03T4.MAP", 0x1234_5678, launch).unwrap()
    });
    let owners = scenes.each_ref().map(|scene| {
        let sim = scene.sim();
        let owner = sim.session().current_house().unwrap();
        (owner, sim.interner().resolve(owner).to_owned())
    });
    let mut inputs = std::array::from_fn::<_, 2, _>(|_| {
        MatchInputState::new(HotkeyBindings::reload_from_ini_bytes(Some(
            b"[Hotkey]\nAllToCheer=67\n",
        )))
    });
    for (scene, (owner, name)) in scenes.iter_mut().zip(&owners) {
        let sim = &mut scene.runtime.simulation;
        let mcv = sim
            .entities()
            .values()
            .find(|entity| {
                entity.owner() == *owner && sim.interner().resolve(entity.type_ref()) == "SMCV"
            })
            .unwrap()
            .stable_id();
        assert_eq!(
            scene.runtime.resources.rules.general.maximum_cheer_rate(),
            300
        );
        assert!(!sim.local_player_defeated() && !sim.session().input_locked());
        assert!(
            schedule_command_in_sim(sim, name, Command::DeployMcv { entity_id: mcv }).is_some()
        );
    }
    let mut loss_frame = None;
    for step in 0..400 {
        if step == 300 {
            for (index, scene) in scenes.iter_mut().enumerate() {
                let sim = &mut scene.runtime.simulation;
                let yards: Vec<_> = sim
                    .entities()
                    .values()
                    .filter(|entity| {
                        entity.owner() == owners[index].0
                            && sim.interner().resolve(entity.type_ref()) == "NACNST"
                    })
                    .collect();
                assert_eq!(
                    yards.len(),
                    1,
                    "normal starting MCV must deploy a real yard"
                );
                assert!(yards[0].building_art.actually_placed());
                let yard = yards[0].stable_id();
                if index == 0 {
                    assert!(
                        schedule_command_in_sim(
                            sim,
                            &owners[index].1,
                            Command::SellBuilding { entity_id: yard },
                        )
                        .is_some()
                    );
                }
            }
        }
        for scene in &mut scenes {
            let commands = scene.runtime.simulation.take_due_commands();
            scene
                .runtime
                .advance_frame(
                    &commands,
                    SIM_TICK_MS,
                    TickLane::Ordinary,
                    FrameEffects::default(),
                )
                .unwrap();
        }
        if scenes[0].sim().local_player_defeated() {
            loss_frame = Some(scenes[0].sim().session().binary_frame());
            break;
        }
    }
    let loss_frame = loss_frame.expect("ordinary last-yard sale must reach MPlayer_Defeated");
    assert!(!scenes[1].sim().local_player_defeated());
    let local = &scenes[0].sim().houses()[&owners[0].0];
    assert!(local.has_lost() && !local.outcome().unwrap().exit_ready);
    for (scene, input) in scenes.iter().zip(&inputs) {
        assert_eq!(scene.sim().session().binary_frame(), loss_frame);
        assert!(
            !scene.sim().session().input_locked(),
            "offline loss does not cinematic-lock"
        );
        assert!(scene.sim().pending_command_snapshot().is_empty());
        assert!(
            input
                .last_cheer_frame
                .wrapping_add(scene.runtime.resources.rules.general.maximum_cheer_rate())
                < loss_frame as i32,
            "admit the natural rate boundary, not a cooldown refusal"
        );
        assert_eq!(
            input.hotkey_bindings.resolve_event(
                &Key::Character("c".into()),
                KeyLocation::Standard,
                ModifiersState::empty(),
            ),
            HotkeyResolution::Command(HotkeyCommand::AllToCheer)
        );
    }
    // The unchanged live control proves the same input frame is eligible.
    let control = &mut scenes[1].runtime;
    assert!(
        queue_house_cheer_in_sim(
            &mut control.simulation,
            &control.resources.rules,
            &mut inputs[1],
            &owners[1].1,
        )
        .is_some()
    );
    let prior = inputs[0].last_cheer_frame;
    let losing = &mut scenes[0].runtime;
    let issued = queue_house_cheer_in_sim(
        &mut losing.simulation,
        &losing.resources.rules,
        &mut inputs[0],
        &owners[0].1,
    );
    assert_eq!(
        issued, None,
        "original730F30 Session300 guard at actual loss frame{loss_frame}"
    );
    assert_eq!(
        inputs[0].last_cheer_frame, prior,
        "refusal does not consume the Cheer opportunity"
    );
    assert!(losing.simulation.pending_command_snapshot().is_empty());
    for (index, scene) in scenes.iter_mut().enumerate() {
        let commands = scene.runtime.simulation.take_due_commands();
        let mut output = scene
            .runtime
            .advance_frame(
                &commands,
                SIM_TICK_MS,
                TickLane::Ordinary,
                FrameEffects::default(),
            )
            .unwrap();
        let admitted = output.take_admitted_commands();
        assert_eq!(
            admitted
                .iter()
                .filter(|command| {
                    command.owner == owners[index].0 && command.payload == Command::Cheer
                })
                .count(),
            usize::from(index == 1)
        );
        assert_eq!(
            output
                .sound_events
                .iter()
                .filter(|event| {
                    matches!(event, SimSoundEvent::VocCentered { sound_id } if sound_id == "Cheer")
                })
                .count(),
            usize::from(index == 1)
        );
        assert!(scene.sim().pending_command_snapshot().is_empty());
    }
}
