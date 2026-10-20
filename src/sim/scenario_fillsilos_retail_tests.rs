//! Fresh FillInData684F02 → ordinary SPY entry. The small native money
//! corpus supplies startup outputs; physical retail map/rules/placement and
//! subsequent command frames supply the production path, without assigned state.

use crate::headless_scenario::{SIM_TICK_MS, battle_session, load_session};
use crate::sim::command::{Command, CommandEnvelope};
use crate::sim::spy_infiltration::InfiltrationEffect;
use crate::sim::world::{FrameEffects, SimSoundEvent, TickLane};
use crate::skirmish_launch::{AiDifficulty, LaunchCountry, LaunchStartPosition, LaunchTeam};

#[test]
fn fresh_fill_silos_credits_reach_a_real_spy_entry() {
    let Some((root, assets)) = crate::rules::retail_ini_fixture::retail_assets() else {
        return;
    };
    let corpus: serde_json::Value =
        serde_json::from_str(crate::test_fixture::text("tools/house_money_oracle.json")).unwrap();
    for enabled in [true, false] {
        let directory = crate::map::source::test_support::TestDirectory::new("fill-silos-spy");
        let path = directory.path().join("fill-silos-spy.map");
        let mut map =
            String::from_utf8(assets.resolve_ref("XMP03T4.MAP").unwrap().bytes.to_vec()).unwrap();
        for (section, records) in [
            (
                "Basic",
                format!("FillSilos={}\n", if enabled { "yes" } else { "no" }),
            ),
            ("NAHAND", "Storage=1\n".to_string()),
            (
                "Infantry",
                "StorageSpy=<human player>,SPY,256,30,85,0,Guard,0,None,0,0,0,1,1\n".to_string(),
            ),
            (
                "Structures",
                "StorageBarracks=Computer,NAHAND,256,31,85,0,None,0,0,1,0,0,0,None,None,None,0\n\
                 StoragePower=Computer,NAPOWR,256,31,89,0,None,0,0,1,0,0,0,None,None,None,0\n"
                    .to_string(),
            ),
        ] {
            let marker = format!("[{section}]");
            if let Some(start) = map.find(&marker) {
                let line_end = start + map[start..].find('\n').unwrap() + 1;
                map.insert_str(line_end, &records);
            } else {
                map.push_str(&format!("\n{marker}\n{records}"));
            }
        }
        std::fs::write(&path, map).unwrap();
        let name = path.to_str().unwrap();
        let mut launch = battle_session(name, &[(LaunchCountry::Russia, AiDifficulty::Easy)]);
        launch.local_player_mut().name = "Player".to_string();
        launch.local_slot_mut().country = LaunchCountry::America;
        launch.local_slot_mut().start_position = LaunchStartPosition::Position(0);
        launch.local_slot_mut().team = LaunchTeam::None;
        launch.opponents[0].start_position = LaunchStartPosition::Position(1);
        launch.opponents[0].team = LaunchTeam::None;
        launch.options.starting_credits = 5000;
        launch.options.bases = true;
        launch.options.unit_count = 0;
        launch.options.tech_level = 10;
        launch.options.short_game = false;
        launch.options.crates = false;
        let mut scene = load_session(&root, name, 0x1234_5678, launch).unwrap();
        let rules = &scene.runtime.resources.rules;
        assert_eq!(rules.object("NAHAND").unwrap().storage, 1);
        assert_eq!(rules.tiberium_types.types()[0].value, 25);
        let sim = scene.sim();
        let player = sim.session.current_house().unwrap();
        let computer = sim.scenario_house_named("Computer").unwrap();
        assert_eq!(sim.houses[&computer].tracking().storage_capacity(), 1);
        assert_eq!(sim.houses[&player].tracking().storage_capacity(), 0);
        assert_eq!(
            crate::sim::house_state::income_ppm_for_owner(
                &sim.houses,
                &sim.interner,
                rules,
                sim.interner.resolve(computer),
            ),
            1_000_000
        );
        let target = sim
            .entities()
            .values()
            .find(|entity| {
                entity.owner() == computer
                    && sim.interner.resolve(entity.type_ref()) == "NAHAND"
                    && (entity.position.rx(), entity.position.ry()) == (31, 85)
            })
            .expect("the actual authored Building must be admitted before Fill")
            .stable_id();
        let spy = sim
            .entities()
            .values()
            .find(|entity| {
                entity.owner() == player && sim.interner.resolve(entity.type_ref()) == "SPY"
            })
            .expect("the actual human SPY must be admitted")
            .stable_id();
        assert!(sim.infiltrator_building_action(spy, target, rules));
        let control = corpus["fill_silos"]
            .as_array()
            .unwrap()
            .iter()
            .find(|row| {
                row["name"]
                    == if enabled {
                        "ordinary 5000 budget"
                    } else {
                        "false flag"
                    }
            })
            .unwrap();
        assert_eq!(
            sim.houses[&computer].economy().credits(),
            control["expected"]["balance"].as_i64().unwrap() as i32,
            "startup uses its initial budget and the actually admitted capacity; enabled={enabled}"
        );
        assert_eq!(
            sim.houses[&computer].economy().score(),
            control["expected"]["score"].as_i64().unwrap() as i32
        );
        assert_eq!(sim.houses[&player].economy().credits(), 5000);
        let health = sim.entities().get(target).unwrap().health;
        let execute_tick = sim.session.tick() + 1;
        scene.runtime.simulation.queue_command(CommandEnvelope::new(
            player,
            execute_tick,
            Command::CaptureBuilding {
                engineer_id: spy,
                target_building_id: target,
            },
        ));
        let mut entries = 0;
        let mut entered = false;
        for _ in 0..200 {
            let before = scene.sim().houses[&computer].economy().clone();
            let commands = scene.runtime.simulation.take_due_commands();
            let frame = scene
                .runtime
                .advance_frame(
                    &commands,
                    SIM_TICK_MS,
                    TickLane::Ordinary,
                    FrameEffects::default(),
                )
                .unwrap();
            entries += frame.sound_events.iter().filter(|event| matches!(event,
                SimSoundEvent::BuildingInfiltrated { owner, infiltrator, effect: InfiltrationEffect::Cash, .. }
                    if *owner == computer && *infiltrator == player
            )).count();
            let sim = scene.sim();
            if sim.entities().get(spy).is_none() {
                let taken = sim.houses[&player].economy().credits() - 5000;
                assert_eq!(
                    taken,
                    before.credits() / 2,
                    "cash uses the actual entry-frame wallet"
                );
                assert_eq!(
                    sim.houses[&computer].economy().credits(),
                    before.credits() - taken
                );
                assert_eq!(
                    sim.houses[&computer].economy().spent_credits(),
                    before.spent_credits() + taken
                );
                entered = true;
                break;
            }
            assert_eq!(sim.houses[&player].economy().credits(), 5000);
        }
        assert!(
            entered,
            "ordinary travel/physical entry must consume the SPY"
        );
        assert_eq!(entries, 1);
        let building = scene.sim().entities().get(target).unwrap();
        assert_eq!((building.owner(), building.health), (computer, health));
    }
}
