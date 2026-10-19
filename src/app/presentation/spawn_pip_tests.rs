//! Stock Carrier construction, launch, recall and reload feed the existing
//! spawn-count owner. Original drawing controls supply the expected frames;
//! they do not certify the whole native aircraft flight lifecycle.
use crate::rules::ruleset::RuleSet;
use crate::sim::command::Command;
use crate::sim::spawn_manager::SpawnSlotState;
use crate::sim::world::Simulation;
use std::collections::BTreeSet;

const OWNER: &str = "Americans";

fn frames(sim: &Simulation, rules: &RuleSet, carrier: u64, hover: Option<u64>) -> Vec<usize> {
    let entity = sim.entities().get(carrier).expect("Carrier survives");
    let before = (
        sim.state_hash(),
        sim.main_rng.native_state_hex(),
        sim.scenario_rng.native_state_hex(),
        sim.mapgen_rng.native_state_hex(),
        entity.spawn_manager.clone(),
    );
    let result = super::ui_overlays::scale_pip_draws(sim, entity, hover, Some(OWNER), rules)
        .into_iter()
        .map(|pip| pip.frame)
        .collect();
    assert_eq!(
        before,
        (
            sim.state_hash(),
            sim.main_rng.native_state_hex(),
            sim.scenario_rng.native_state_hex(),
            sim.mapgen_rng.native_state_hex(),
            entity.spawn_manager.clone(),
        ),
        "drawing reads retained nodes without advancing RNG, timers or targets"
    );
    result
}

fn native_frames(count: usize) -> Vec<usize> {
    let native = crate::test_fixture::json("tools/procedural_drawing_oracle/spawn_pips.json");
    native["raster_controls"]
        .as_array()
        .unwrap()
        .iter()
        .find_map(|row| {
            let actors = row["input"]["actors"].as_array()?;
            if actors.len() != 1
                || actors[0]["type_name"] != "CARRIER"
                || !row["input"]["scenario_patch"].is_null()
                || row["actor_results"][0]["spawn_count_docked"].as_u64() != Some(count as u64)
            {
                return None;
            }
            let frames: Vec<_> = row["draws"]
                .as_array()?
                .iter()
                .filter(|draw| draw["asset"] == "PIPS2.SHP")
                .map(|draw| draw["frame"].as_u64().unwrap() as usize)
                .collect();
            (frames.len() == 3).then_some(frames)
        })
        .expect("original admitted three-slot Carrier draw for this count")
}

#[test]
fn stock_carrier_pips_follow_construction_sortie_recall_reload_and_selection() {
    let Some(retail) = crate::rules::retail_ini_fixture::retail_battle_rules_for_map("XMP03T4.MAP")
    else {
        return;
    };
    let rules = &retail.rules;
    let expected: [Vec<usize>; 4] = std::array::from_fn(native_frames);
    let mut sim = crate::sim::arena_fixture::rival_water_arena(rules, 1, 65);
    // Aircraft placement needs the actual Size diamond as well as LocalSize.
    sim.install_playfield_from_map_header(&crate::map::map_file::MapHeader {
        theater: "TEMPERATE".into(),
        fill: "Water".into(),
        level: 0,
        width: 32,
        height: 32,
        local_left: 2,
        local_top: 2,
        local_width: 28,
        local_height: 24,
    });
    sim.intern_rule_type_ids(rules);
    sim.resolve_type_handles(rules);
    let owner = sim.interner.get(OWNER).unwrap();
    sim.fog.reveal_all_for_owner(owner);
    let carrier = sim
        .spawn_object("CARRIER", OWNER, 20, 20, 64, rules)
        .unwrap();
    let target = sim
        .spawn_object("LCRF", "Russians", 34, 20, 64, rules)
        .unwrap();
    let wing: Vec<_> = sim
        .entities()
        .get(carrier)
        .unwrap()
        .spawn_manager
        .as_ref()
        .unwrap()
        .slots()
        .iter()
        .map(|slot| slot.spawn().expect("constructor-owned Hornet"))
        .collect();
    assert_eq!(wing.len(), 3);
    assert!(
        wing.iter()
            .all(|id| sim.entities().get(*id).unwrap().lifecycle.in_limbo())
    );
    assert!(sim.apply_command(
        OWNER,
        &Command::Select {
            entity_ids: vec![carrier],
            additive: false
        },
        Some(rules)
    ));
    assert_eq!(frames(&sim, rules, carrier, None), expected[3]);
    assert!(sim.apply_command(
        OWNER,
        &Command::Attack {
            attacker_id: carrier,
            target_id: target
        },
        Some(rules)
    ));
    let hornet_bomb = sim
        .interner
        .intern(rules.object("HORNET").unwrap().weapon_at(0).unwrap());
    let mut counts = BTreeSet::from([3]);
    let mut fired = false;
    let mut recalled = false;
    let mut reloading = false;
    let mut ready_again = false;
    for _ in 0..4000 {
        let result = sim
            .advance_app_frame(
                &[],
                Some(rules),
                None,
                33,
                crate::sim::world::TickLane::Ordinary,
                crate::sim::world::FrameEffects::default(),
            )
            .expect("ordinary stock Carrier frame");
        fired |= result
            .fire_events
            .iter()
            .any(|shot| shot.weapon_id == hornet_bomb && wing.contains(&shot.attacker_id));
        let manager = sim
            .entities()
            .get(carrier)
            .unwrap()
            .spawn_manager
            .as_ref()
            .unwrap();
        let count = manager.count_docked_spawns();
        counts.insert(count);
        assert_eq!(frames(&sim, rules, carrier, None), expected[count]);
        reloading |= manager
            .slots()
            .iter()
            .any(|slot| slot.state() == SpawnSlotState::Reloading);
        if recalled
            && reloading
            && manager
                .slots()
                .iter()
                .all(|slot| slot.state() == SpawnSlotState::ReadyDocked)
        {
            assert!(wing.iter().all(|id| {
                sim.entities().get(*id).is_some_and(|child| {
                    child.lifecycle.in_limbo()
                        && child.ammo().is_some_and(|ammo| ammo.current() == 1)
                })
            }));
            ready_again = true;
            break;
        }
        if fired && !recalled {
            assert!(sim.apply_command(OWNER, &Command::Stop { entity_id: carrier }, Some(rules)));
            recalled = true;
        }
    }
    assert_eq!(
        counts,
        BTreeSet::from([0, 1, 2, 3]),
        "ordinary launches and landings expose every count"
    );
    assert!(
        fired && recalled && reloading && ready_again,
        "stock Hornets must fire, recall, count while reloading, and become ready again"
    );
    assert!(sim.apply_command(
        OWNER,
        &Command::Select {
            entity_ids: vec![],
            additive: false
        },
        Some(rules)
    ));
    assert!(frames(&sim, rules, carrier, None).is_empty());
    assert_eq!(frames(&sim, rules, carrier, Some(carrier)), expected[3]);
}

/// An authored stock-unit scenario through the normal skirmish loader. This
/// reaches the parent Spawner FireAt before any Hornet can disclose the ship.
#[test]
fn stock_carrier_spawner_fire_reveals_to_its_human_victim() {
    use crate::sim::combat::TargetKind;
    use crate::sim::command::CommandEnvelope;
    use crate::sim::world::{FrameEffects, TickLane};
    use crate::skirmish_launch::{AiDifficulty, LaunchCountry};

    let Some((retail, _)) = crate::rules::retail_ini_fixture::retail_assets() else {
        return;
    };
    let map = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tools/map_observation.carrier-fire-reveal.scenario");
    let map = map.to_str().unwrap();
    let mut launch = crate::headless_scenario::battle_session(
        map,
        &[(LaunchCountry::Russia, AiDifficulty::Easy)],
    );
    launch.player_name = "VERA-OBSERVER".into();
    launch.local.country = LaunchCountry::America;
    launch.options.bases = false;
    launch.options.unit_count = 0;
    launch.options.short_game = false;
    launch.options.shroud = true;
    launch.options.fog_of_war = false;
    let mut scene =
        crate::headless_scenario::load_session(&retail, map, 0x1234_5678, launch).unwrap();
    let current = scene.sim().session.current_house.unwrap();
    let carrier = scene
        .sim()
        .entities()
        .iter_sorted()
        .find_map(|(id, entity)| {
            (scene.sim().interner.resolve(entity.type_ref()) == "CARRIER").then_some(id)
        })
        .expect("authored Carrier admitted through normal Unlimbo");
    let target = scene
        .sim()
        .entities()
        .iter_sorted()
        .find_map(|(id, entity)| {
            (entity.owner() == current && scene.sim().interner.resolve(entity.type_ref()) == "LCRF")
                .then_some(id)
        })
        .expect("normal local human LCRF");
    assert!(scene.sim().owner_is_human(current));
    let firer = scene.sim().entities().get(carrier).unwrap();
    let owner = firer.owner();
    assert!(!scene.sim().owner_is_human(owner));
    assert_ne!(owner, current);
    let source = (firer.position.rx(), firer.position.ry());
    assert!(
        scene
            .sim()
            .resolved_terrain
            .as_ref()
            .unwrap()
            .cell(source.0, source.1)
            .unwrap()
            .land_type()
            == crate::rules::terrain_rules::LandType::Water.as_index(),
        "authored source is real loaded Water"
    );
    let weapon = scene
        .runtime
        .resources
        .rules
        .weapon("HornetLauncher")
        .unwrap();
    assert!(
        weapon.spawner && weapon.reveal_on_fire,
        "actual layered stock weapon"
    );
    let manager = firer
        .spawn_manager
        .as_ref()
        .expect("constructor-owned pool");
    assert_eq!(manager.slots().len(), 3);
    assert!(manager.slots().iter().all(|slot| {
        slot.state() == SpawnSlotState::ReadyDocked
            && scene
                .sim()
                .entities()
                .get(slot.spawn().unwrap())
                .unwrap()
                .lifecycle
                .in_limbo()
    }));
    assert!(
        !scene
            .sim()
            .fog
            .is_cell_revealed(current, source.0, source.1)
    );

    for frame in 0..30 {
        let commands = if frame == 0 {
            vec![CommandEnvelope::new(
                owner,
                scene.sim().session.tick,
                Command::Attack {
                    attacker_id: carrier,
                    target_id: target,
                },
            )]
        } else {
            Vec::new()
        };
        scene
            .runtime
            .advance_frame(
                &commands,
                crate::app::types::SIM_TICK_MS,
                TickLane::Ordinary,
                FrameEffects::default(),
            )
            .unwrap();
        let manager = scene
            .sim()
            .entities()
            .get(carrier)
            .unwrap()
            .spawn_manager
            .as_ref()
            .unwrap();
        if manager.queued_target() == Some(TargetKind::Entity(target))
            || manager.current_target() == Some(TargetKind::Entity(target))
        {
            assert!(
                manager.slots().iter().all(|slot| {
                    slot.state() == SpawnSlotState::ReadyDocked
                        && scene
                            .sim()
                            .entities()
                            .get(slot.spawn().unwrap())
                            .unwrap()
                            .lifecycle
                            .in_limbo()
                }),
                "parent admission must precede child-fire disclosure"
            );
            eprintln!(
                "stock Carrier parent Spawner FireAt admitted at frame{frame}, source{source:?}"
            );
            assert!(
                scene
                    .sim()
                    .fog
                    .is_cell_revealed(current, source.0, source.1),
                "original6FDF30 reveals a stock Spawner firer to its human victim"
            );
            return;
        }
    }
    panic!("normal Carrier Attack did not reach the parent Spawner fire handoff");
}
