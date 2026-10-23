//! UpdateRepairAndPower4507C4..45080D consumes the Scenario sale roll, then
//! refuses any attached Object+34 Tag (4507D7..4507DC). This legal Scenario
//! enables the IQ gate through ReadIQ674240, not supplied runtime House state.

use crate::headless_scenario::{HeadlessScenario, SIM_TICK_MS, battle_session, load_session};
use crate::rules::mission_data::MissionType;
use crate::sim::combat::TargetKind;
use crate::sim::command::{Command, CommandEnvelope};
use crate::sim::intern::InternedId;
use crate::sim::rng::trace_draws;
use crate::sim::world::{FrameEffects, SimFrameOutput, TickLane};
use crate::skirmish_launch::{AiDifficulty, LaunchCountry, LaunchStartPosition};

fn insert_records(map: &mut String, section: &str, records: &str) {
    let marker = format!("[{section}]");
    if let Some(start) = map.find(&marker) {
        let end = start + map[start..].find('\n').unwrap() + 1;
        map.insert_str(end, records);
    } else {
        map.push_str(&format!("\n{marker}\n{records}"));
    }
}

fn queue(scene: &mut HeadlessScenario, owner: InternedId, command: Command) {
    let tick = scene.sim().session.tick() + 1;
    scene
        .runtime
        .simulation
        .queue_command(CommandEnvelope::new(owner, tick, command));
}

fn frame(scene: &mut HeadlessScenario, building: u64) -> (SimFrameOutput, Option<u32>) {
    let commands = scene.runtime.simulation.take_due_commands();
    let (output, draws) = trace_draws(|| {
        scene
            .runtime
            .advance_frame(
                &commands,
                SIM_TICK_MS,
                TickLane::Ordinary,
                FrameEffects::default(),
            )
            .expect("ordinary bound-resource frame")
    });
    assert!(!output.network_waiting && !output.outcome_waiting);
    // Observation of this Building's actual call, not a seeded answer or a
    // supplied RNG cursor. RandomRanged65C7E0(0,50) rejects masked words>50.
    let samples: Vec<_> = draws
        .iter()
        .filter(|draw| {
            draw["logic_object"].as_u64() == Some(building)
                && draw["callers"]
                    .as_str()
                    .is_some_and(|callers| callers.contains("low_credit_sale"))
        })
        .map(|draw| draw["value"].as_u64().unwrap() as u32 & 63)
        .collect();
    let roll = samples.last().copied();
    if let Some(roll) = roll {
        assert!(roll <= 50);
        assert!(
            samples[..samples.len() - 1]
                .iter()
                .all(|sample| *sample > 50)
        );
    }
    (output, roll)
}

#[test]
fn authored_tag_keeps_a_hostile_damaged_low_credit_building_from_auto_sale() {
    let Some((root, assets)) = crate::rules::retail_ini_fixture::retail_assets() else {
        return;
    };
    let hills = String::from_utf8(
        assets
            .resolve_ref("XMP03T4.MAP")
            .expect("physical Hills retail map")
            .bytes
            .to_vec(),
    )
    .expect("Hills text INI");
    let directory = crate::map::source::test_support::TestDirectory::new("ai-sale-tag");
    // The untagged companion must really sell before the veto is tested.
    for tagged in [false, true] {
        let path = directory
            .path()
            .join(if tagged { "tagged.map" } else { "untagged.map" });
        let mut map = hills.clone();
        insert_records(
            &mut map,
            "Units",
            "SaleAttacker=<human player>,LTNK,256,30,85,64,Guard,None,0,-1,0,-1,1,1\n",
        );
        let tag = if tagged { "AI_KEEP_TAG" } else { "None" };
        insert_records(
            &mut map,
            "Structures",
            &format!(
                "SaleBarracks=Neutral,GAPILE,256,31,85,0,{tag},0,0,1,0,0,0,None,None,None,0\n"
            ),
        );
        for (section, records) in [
            ("IQ", "RepairSell=0\nSellBack=0\n"),
            (
                "Triggers",
                "AI_KEEP=Neutral,<none>,keep tagged structure,1,1,1,1,0\n",
            ),
            ("Tags", "AI_KEEP_TAG=2,keep tagged structure,AI_KEEP\n"),
            ("Events", "AI_KEEP=1,0,0,0\n"),
            ("Actions", "AI_KEEP=1,0,0,0,0,0,0,0,A\n"),
        ] {
            insert_records(&mut map, section, records);
        }
        std::fs::write(&path, map).unwrap();
        let name = path.to_str().unwrap();
        let mut launch = battle_session(name, &[(LaunchCountry::Russia, AiDifficulty::Easy)]);
        launch.local_slot_mut().start_position = LaunchStartPosition::Position(0);
        launch.opponents[0].start_position = LaunchStartPosition::Position(1);
        launch.options.starting_credits = 5000;
        launch.options.bases = true;
        launch.options.unit_count = 0;
        launch.options.tech_level = 1;
        launch.options.short_game = false;
        launch.options.crates = false;
        let mut scene = load_session(&root, name, 0x1234_5678, launch)
            .expect("ordinary Hills Scenario with legal IQ and Tag records");
        let sim = scene.sim();
        let player = sim.session.current_house().unwrap();
        let neutral = sim.interner.get("Neutral").unwrap();
        let building = sim
            .entities()
            .values()
            .find(|entity| {
                sim.interner.resolve(entity.type_ref()) == "GAPILE"
                    && entity.owner() == neutral
                    && (entity.position.rx(), entity.position.ry()) == (31, 85)
            })
            .expect("real Neutral GAPILE placement");
        let attacker = sim
            .entities()
            .values()
            .find(|entity| {
                sim.interner.resolve(entity.type_ref()) == "LTNK"
                    && entity.owner() == player
                    && (entity.position.rx(), entity.position.ry()) == (30, 85)
            })
            .expect("real current-human LTNK placement");
        let (building, attacker) = (building.stable_id(), attacker.stable_id());
        let target = sim.entities().get(building).unwrap();
        let rules = &scene.runtime.resources.rules;
        let object = rules.object("GAPILE").unwrap();
        assert_eq!(object.strength, 500);
        assert!(object.click_repairable && object.repairable);
        assert_eq!(target.health.current(), object.strength);
        assert!(!target.ai_sell_repair.was_attacked_by_enemy());
        assert!(!target.constructing_or_selling());
        assert!(target.lifecycle.cell_marked() && !target.in_limbo());
        assert!(sim.logic_order().contains(&building) && sim.logic_order().contains(&attacker));
        assert_eq!(sim.object_has_trigger_tag(building), tagged);
        assert_eq!(rules.general.iq_repair_sell, 0);
        assert_eq!(rules.general.iq_sell_back, 0);
        assert_eq!(rules.general.credit_reserve, 100);
        assert_eq!(rules.general.condition_red, 0.25);
        let house = &sim.houses[&neutral];
        assert_eq!(
            (house.current_iq(), house.authored_iq(), house.tech_level()),
            (0, 0, 1)
        );
        assert_eq!(house.economy().credits(), 0);
        assert!(!crate::map::houses::is_allied_with(
            sim.house_alliances(),
            sim.interner.resolve(neutral),
            sim.interner.resolve(player),
        ));
        assert_eq!(
            rules
                .art()
                .resolve_metadata_entry(&object.id, &object.image)
                .unwrap()
                .buildup
                .as_deref(),
            Some("GAPILEMK")
        );
        assert!(
            rules.has_buildup("GAPILE"),
            "load_session must bind the physical Buildup SHP"
        );
        assert!(rules.buildup_control("GAPILE")[1] > 0);
        // These cells also support the existing physical Hills Neutral-refinery
        // entry fixture. Assert this pair's actual resolved terrain, not a
        // synthetic passable grid or an assumed rectangular playfield.
        let terrain = sim.resolved_terrain().unwrap();
        for cell in [
            (30, 85),
            (31, 85),
            (32, 85),
            (33, 85),
            (31, 86),
            (32, 86),
            (33, 86),
        ] {
            let ground = terrain
                .cell(cell.0, cell.1)
                .expect("authored footprint cell");
            assert!(!ground.outside_playfield && !ground.has_bridge_deck && !ground.has_ramp);
            assert!(
                !ground.base_build_blocked,
                "physical buildable land at {cell:?}"
            );
            assert!(
                ground
                    .base_speed_costs
                    .cost_for_speed_type(rules.object("LTNK").unwrap().speed_type)
                    .is_some_and(|cost| cost > 0),
                "physical LTNK land admission at {cell:?}"
            );
            assert!(
                !ground.base_ground_walk_blocked
                    && !ground.terrain_object_blocks
                    && !ground.overlay_blocks
            );
        }
        queue(
            &mut scene,
            player,
            Command::Attack {
                attacker_id: attacker,
                target_id: building,
            },
        );
        let mut shots = 0;
        let mut stopped = false;
        let mut qualified = false;
        let mut selling = false;
        for _ in 0..3000 {
            let before = scene.sim().entities().get(building).unwrap();
            let red_before = before.health.current() > 0
                && before.health.current() < 125
                && before.ai_sell_repair.was_attacked_by_enemy();
            let (output, roll) = frame(&mut scene, building);
            shots += output
                .fire_events
                .iter()
                .filter(|event| {
                    event.attacker_id == attacker && event.target == TargetKind::Entity(building)
                })
                .count();
            let target = scene
                .sim()
                .entities()
                .get(building)
                .expect("shots must leave the barracks alive");
            assert_eq!(target.owner(), neutral);
            assert!(target.health.current() > 0);
            assert_eq!(scene.sim().object_has_trigger_tag(building), tagged);
            let red = target.health.current() < 125;
            let hit = target.ai_sell_repair.was_attacked_by_enemy();
            selling = target.mission.effective().known() == Some(MissionType::Selling);
            if red {
                assert!(
                    shots > 0 && hit,
                    "red health must come from actual hostile shots"
                );
                if !stopped {
                    queue(
                        &mut scene,
                        player,
                        Command::Stop {
                            entity_id: attacker,
                        },
                    );
                    stopped = true;
                }
            }
            qualified |= red_before && roll == Some(0);
            if selling {
                assert!(red && hit && shots > 0);
                assert_eq!(
                    roll,
                    Some(0),
                    "actual sale-admitting Scenario roll at TechLevel1"
                );
                assert!(
                    !tagged,
                    "4507D7 attached Tag must veto automatic sale after the real hostile hit and sale roll"
                );
                break;
            }
            assert_eq!(scene.sim().houses[&neutral].economy().credits(), 0);
            if tagged && qualified {
                break;
            }
        }
        assert!(
            stopped && shots > 0,
            "LTNK must really reach and damage the barracks to red HP"
        );
        if tagged {
            assert!(
                qualified && !selling,
                "must observe a natural red-health sale-admitting roll"
            );
            for _ in 0..64 {
                frame(&mut scene, building);
                let sim = scene.sim();
                let target = sim
                    .entities()
                    .get(building)
                    .expect("tagged barracks remains on the map");
                assert!(target.is_alive() && target.lifecycle.cell_marked() && !target.in_limbo());
                assert!(sim.object_has_trigger_tag(building));
                assert_ne!(
                    target.mission.effective().known(),
                    Some(MissionType::Selling)
                );
                assert_eq!(sim.houses[&neutral].economy().credits(), 0);
            }
        } else {
            assert!(
                selling,
                "untagged companion must actually start its automatic sale"
            );
            for _ in 0..600 {
                frame(&mut scene, building);
                if scene.sim().entities().get(building).is_none() {
                    break;
                }
            }
            assert!(
                scene.sim().entities().get(building).is_none(),
                "ordinary Selling must remove the untagged barracks"
            );
            assert!(
                scene.sim().houses[&neutral].economy().credits() > 0,
                "real sale refund reaches Neutral"
            );
        }
    }
}
