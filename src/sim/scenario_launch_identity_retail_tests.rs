//! A participant handle cannot become another House's alliance identity.
//! Original CreateHouses687F10 allocates participants independently, stores
//! the human scenario name `<human player>` at68804A..688083 and its display
//! handle separately at688092..6880AD. IsAllied4F9A90 asks that House's mask,
//! not a normalized display name. The selected consumer is SPY's51EE3B arm.

use crate::headless_scenario::{SIM_TICK_MS, battle_session, load_session};
use crate::sim::command::{Command, CommandEnvelope};
use crate::sim::spy_infiltration::InfiltrationEffect;
use crate::sim::world::{FrameEffects, SimSoundEvent, TickLane};
use crate::skirmish_launch::{AiDifficulty, LaunchCountry, LaunchStartPosition, LaunchTeam};

#[test]
fn padded_ai_handle_does_not_turn_an_enemy_refinery_into_an_allied_target() {
    let Some((root, assets)) = crate::rules::retail_ini_fixture::retail_assets() else {
        return;
    };
    let directory = crate::map::source::test_support::TestDirectory::new("launch-name-spy");
    let path = directory.path().join("launch-name-spy.map");
    let mut map = String::from_utf8(
        assets
            .resolve_ref("XMP03T4.MAP")
            .expect("physical Hills map")
            .bytes
            .to_vec(),
    )
    .expect("retail Hills is a text INI");
    for (section, records) in [
        (
            "Infantry",
            "NameSpy=<human player>,SPY,256,30,85,0,Guard,0,None,0,0,0,1,1\n",
        ),
        (
            "Structures",
            "NameRefinery=Computer,NAREFN,256,31,85,0,None,0,0,1,0,0,0,None,None,None,0\n",
        ),
    ] {
        let marker = format!("[{section}]");
        if let Some(start) = map.find(&marker) {
            let line_end = start + map[start..].find('\n').unwrap() + 1;
            map.insert_str(line_end, records);
        } else {
            map.push_str(&format!("\n{marker}\n{records}"));
        }
    }
    std::fs::write(&path, map).unwrap();
    let name = path.to_str().unwrap();
    let mut launch = battle_session(
        name,
        &[
            (LaunchCountry::Russia, AiDifficulty::Easy),
            (LaunchCountry::Russia, AiDifficulty::Easy),
        ],
    );
    launch.local_player_mut().name = " Computer2 ".to_owned();
    launch.local_slot_mut().start_position = LaunchStartPosition::Position(0);
    launch.local_slot_mut().team = LaunchTeam::None;
    for (index, opponent) in launch.opponents.iter_mut().enumerate() {
        opponent.start_position = LaunchStartPosition::Position(index as u8 + 1);
        opponent.team = LaunchTeam::Team(1);
    }
    launch.options.starting_credits = 5000;
    launch.options.bases = true;
    launch.options.unit_count = 0;
    launch.options.tech_level = 10;
    launch.options.short_game = false;
    launch.options.crates = false;
    let mut scene = load_session(&root, name, 0x1234_5678, launch)
        .expect("ordinary three-participant retail Battle startup");
    let sim = scene.sim();
    let player = sim.session.current_house().unwrap();
    let computer = sim.interner.get("Computer1").unwrap();
    let teammate = sim.interner.get("Computer2").unwrap();
    assert_ne!(player, computer);
    assert_ne!(player, teammate);
    assert_eq!(sim.scenario_house_named("Computer"), Some(computer));
    assert_eq!(
        sim.houses[&player].native_scenario_name(),
        Some("<human player>")
    );
    assert!(crate::map::houses::are_houses_mutually_allied(
        &sim.house_alliances,
        "Computer1",
        "Computer2"
    ));
    let refinery = sim
        .entities()
        .values()
        .find(|entity| {
            sim.interner.resolve(entity.type_ref()) == "NAREFN"
                && entity.owner() == computer
                && (entity.position.rx(), entity.position.ry()) == (31, 85)
        })
        .expect("authored Computer refinery is admitted as the first AI House")
        .stable_id();
    let spy = sim
        .entities()
        .values()
        .find(|entity| sim.interner.resolve(entity.type_ref()) == "SPY" && entity.owner() == player)
        .expect("authored current-human SPY is actually admitted")
        .stable_id();
    assert!(
        sim.infiltrator_building_action(spy, refinery, &scene.runtime.resources.rules),
        "a participant display name cannot import an AI team's ally mask"
    );

    // The local action classification above is essential: a delivered
    // CaptureBuilding envelope deliberately does not repeat its alliance gate.
    // After admission, use the ordinary command/frame and physical entry owners.
    let command = CommandEnvelope::new(
        player,
        sim.session.tick() + 1,
        Command::CaptureBuilding {
            engineer_id: spy,
            target_building_id: refinery,
        },
    );
    scene.runtime.simulation.queue_command(command);
    let mut cash_entries = 0;
    let mut entered = false;
    for _ in 0..200 {
        let commands = scene.runtime.simulation.take_due_commands();
        let frame = scene
            .runtime
            .advance_frame(
                &commands,
                SIM_TICK_MS,
                TickLane::Ordinary,
                FrameEffects::default(),
            )
            .expect("ordinary bound-resource command frame");
        cash_entries += frame.sound_events.iter().filter(|event| matches!(
            event,
            SimSoundEvent::BuildingInfiltrated { owner, infiltrator, effect: InfiltrationEffect::Cash, .. }
                if *owner == computer && *infiltrator == player
        )).count();
        let sim = scene.sim();
        if sim.entities().get(spy).is_none() {
            entered = true;
            break;
        }
        assert_eq!(sim.houses[&computer].economy().credits(), 5000);
        assert_eq!(sim.houses[&player].economy().credits(), 5000);
    }
    assert!(
        entered,
        "the real SPY must enter and complete UnInit cleanup"
    );
    assert_eq!(cash_entries, 1);
    let sim = scene.sim();
    assert_eq!(sim.entities().get(refinery).unwrap().owner(), computer);
    assert_eq!(sim.houses[&player].economy().credits(), 7500);
    assert_eq!(sim.houses[&computer].economy().credits(), 2500);
    assert_eq!(sim.houses[&computer].economy().spent_credits(), 2500);
    assert_eq!(sim.houses[&teammate].economy().credits(), 5000);
}
