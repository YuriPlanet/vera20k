//! Retail Soviet Armor Easy through House demand, paid exits and its real script.
//! House4F8A00/4FEA60 ->FactoryAI4500F0 ->Recruit6EAA90 ->Script0/6ED090.
//! Instructions establish the expected loop; this is a Rust production witness.

use super::*;
use crate::headless_scenario::{battle_session, load_session};
use crate::rules::team_ai_ini::TeamAiIniRegistry;
use crate::sim::combat::TargetKind;
use crate::sim::command::{Command, CommandEnvelope};
use crate::sim::production::{
    FactoryHolder, placement_preview_for_owner_with_overlays, ready_building_entity_for_owner,
};
use crate::sim::world::{FrameEffects, Simulation, TickLane};
use crate::skirmish_launch::{AiDifficulty, LaunchCountry, LaunchStartPosition};
use std::collections::{BTreeMap, BTreeSet};

#[test]
#[ignore = "slow; retail ground-team production and attack uses physical map, rules and art"]
fn retail_soviet_tank_team_completes_its_stock_attack_script() {
    let (root, assets) = crate::rules::retail_ini_fixture::retail_assets().unwrap();
    let stock = assets.resolve_ref("XMP03T4.MAP").unwrap();
    let stock = std::str::from_utf8(&stock.bytes).unwrap();
    let map = crate::map::map_file::MapFile::from_bytes(stock.as_bytes()).unwrap();
    let ai = TeamAiIniRegistry::load_retail(&assets, &map.ini, true).unwrap();
    let trigger = ai
        .ai_triggers
        .iter()
        .find(|trigger| trigger.id == "0D2C448C-G")
        .unwrap();
    assert_eq!(trigger.primary_team_type.as_deref(), Some("06BB841C-G"));
    assert_eq!(trigger.condition, 4);
    let team = ai
        .team_types
        .iter()
        .find(|team| team.id == "06BB841C-G")
        .unwrap();
    let force = ai
        .task_forces
        .iter()
        .find(|force| Some(force.id.as_str()) == team.section().read_name("TaskForce", 0x20))
        .unwrap();
    assert_eq!(
        force
            .entries
            .iter()
            .map(|entry| (entry.member_type.as_str(), entry.count))
            .collect::<Vec<_>>(),
        [("HTNK", 2), ("HTK", 1)]
    );
    let script = ai
        .scripts
        .iter()
        .find(|script| Some(script.id.as_str()) == team.section().read_name("Script", 0x20))
        .unwrap();
    assert_eq!(
        script
            .actions
            .iter()
            .map(|action| (action.action_id, action.argument))
            .collect::<Vec<_>>(),
        [(54, 0), (63, 0), (53, 0), (0, 6), (49, 0), (0, 2), (0, 1)]
    );
    // Author only priority6F0C72, retaining the stock money/factory/zone gates,
    // TaskForce and script. This is not an unchanged-stock frequency witness.
    let mut tokens = trigger.tokens.clone();
    for weight in &mut tokens[7..10] {
        *weight = "5000".into();
    }
    let mut source = stock.to_owned();
    super::firesale_retail_tests::insert_section_records(
        &mut source,
        "AITriggerTypes",
        &format!("{}={}\n", trigger.id, tokens.join(",")),
    );
    let directory =
        crate::map::source::test_support::TestDirectory::new("soviet-ground-production");
    directory.write("ground.map", source.as_bytes());
    let path = directory.path().join("ground.map");
    let name = path.to_str().unwrap();
    let mut launch = battle_session(name, &[(LaunchCountry::Russia, AiDifficulty::Easy)]);
    launch.local_player_mut().name = "Enemy".into();
    launch.local_slot_mut().start_position = LaunchStartPosition::Position(2);
    launch.opponents[0].start_position = LaunchStartPosition::Position(1);
    launch.options.unit_count = 0;
    launch.options.starting_credits = 10_000;
    launch.options.short_game = false;
    launch.options.crates = false;
    launch.options.super_weapons = false;
    let mut scene = load_session(&root, name, 0x5EED_0001, launch).unwrap();
    let sim = &mut scene.runtime.simulation;
    let rules = &scene.runtime.resources.rules;
    let owner = Simulation::scenario_house_named(&sim.houses, "Computer").unwrap();
    let enemy = sim.session.current_house().unwrap();
    let mcv = sim
        .entities()
        .values()
        .find(|entity| entity.owner() == enemy && sim.interner.resolve(entity.type_ref()) == "AMCV")
        .unwrap()
        .stable_id();
    let home = sim.entities().get(mcv).unwrap().position;
    assert!(
        !sim.entities().values().any(|entity| entity.owner() == owner
            && matches!(sim.interner.resolve(entity.type_ref()), "HTNK" | "HTK"))
    );
    // Two initial unarmed enemy miners make target destruction/reselection
    // observable. They use ordinary constructors/Unlimbo; the AI gets no army,
    // factories, money beyond the lobby, timer writes or forced membership.
    let mut cells: Vec<_> = sim
        .resolved_terrain()
        .unwrap()
        .iter()
        .map(|cell| (cell.rx, cell.ry))
        .collect();
    let distance = |(x, y): (u16, u16)| {
        (i32::from(x) - i32::from(home.rx())).pow(2) + (i32::from(y) - i32::from(home.ry())).pow(2)
    };
    cells.sort_by_key(|&cell| (distance(cell), cell));
    let mut miners = Vec::new();
    for _ in 0..2 {
        let miner = cells
            .iter()
            .filter(|&&cell| (16..=64).contains(&distance(cell)))
            .find_map(|&(x, y)| sim.spawn_object("CMIN", "Enemy", x, y, 0, rules))
            .expect("initial miner fits beside player start");
        miners.push(miner);
    }
    let ty = sim.interner.get("06BB841C-G").unwrap();
    let power_type = sim.interner.get("GAPOWR").unwrap();
    let mut created = BTreeMap::new();
    let mut paid = BTreeSet::new();
    let mut members = BTreeSet::new();
    let mut cursors = BTreeSet::new();
    let mut fired_at = BTreeSet::new();
    let mut selected = None;
    let mut first_miner = None;
    let mut reselected = false;
    let mut ended = false;
    let mut saved_product = None;
    for step in 0..40_000 {
        let sim = &mut scene.runtime.simulation;
        let rules = &scene.runtime.resources.rules;
        let mut held = None;
        for (holder, factory) in sim.production.factories().holders_insertion_ordered() {
            let FactoryHolder::Building(producer) = holder else {
                continue;
            };
            let Some(id) = factory.object().and_then(|object| object.entity_id) else {
                continue;
            };
            let actor = sim.entities().get(id).unwrap();
            if actor.owner() == owner
                && matches!(sim.interner.resolve(actor.type_ref()), "HTNK" | "HTK")
            {
                created.insert(id, producer);
                let kind = rules
                    .object(sim.interner.resolve(actor.type_ref()))
                    .unwrap();
                if factory.balance() < sim.cost_of(owner, kind, rules) {
                    paid.insert(id);
                }
                if actor.lifecycle.in_limbo() {
                    held = Some(id);
                }
            }
        }
        let mut commands = sim.take_due_commands();
        if step == 0 {
            commands.push(CommandEnvelope::new(
                enemy,
                sim.session.tick(),
                Command::DeployMcv { entity_id: mcv },
            ));
        }
        // The opponent also has an ordinary opening: its actual MCV deploys,
        // then pays for and places a power plant. The first Building's
        // Unlimbo4FD150 scans the pre-append roster; the next placement
        // establishes the base cell that trigger41FEE0 needs for its zone gate.
        if step == 70 {
            commands.push(CommandEnvelope::new(
                enemy,
                sim.session.tick(),
                Command::QueueProduction {
                    type_id: power_type,
                },
            ));
        }
        if step == 650 {
            ready_building_entity_for_owner(sim, rules, enemy, power_type)
                .expect("opponent's paid power plant is ready");
            let (rx, ry) = cells
                .iter()
                .copied()
                .find(|&(rx, ry)| {
                    placement_preview_for_owner_with_overlays(
                        sim,
                        rules,
                        "Enemy",
                        "GAPOWR",
                        rx,
                        ry,
                        Some(&scene.runtime.resources.overlay_registry),
                    )
                    .is_some_and(|preview| preview.valid)
                })
                .expect("a normal adjacent power-plant site exists");
            commands.push(CommandEnvelope::new(
                enemy,
                sim.session.tick(),
                Command::PlaceReadyBuilding {
                    type_id: power_type,
                    rx,
                    ry,
                },
            ));
        }
        let output = scene
            .runtime
            .advance_frame(&commands, 22, TickLane::Ordinary, FrameEffects::default())
            .unwrap();
        let sim = &scene.runtime.simulation;
        if step == 700 {
            assert_ne!(sim.houses[&enemy].base_origin(), (0, 0));
        }
        if selected.is_none() {
            selected = sim
                .team_script_vm
                .first_team_of_type(ty)
                .map(|team| team.id());
        }
        if let Some(team) = selected.and_then(|id| sim.team_script_vm.team(id)) {
            assert!(team.refusal().is_none(), "{:?}", team.refusal());
            cursors.insert(team.cursor());
            for id in team.members() {
                assert!(
                    created.contains_key(&id),
                    "the actual paid factory product is recruited"
                );
                members.insert(id);
            }
            // Quarry6 first attacks factories; the final Quarry1 has mask0
            // (Quarry_To_Threat645BB0), so it also clears the two miners.
            if team.cursor() == 6
                && let Some(TeamTarget::Object(target)) = team.mission_target
                && miners.contains(&target)
            {
                if let Some(first) = first_miner {
                    reselected |= first != target
                        && sim
                            .entities()
                            .get(first)
                            .is_none_or(|actor| !actor.lifecycle.object_alive());
                } else {
                    first_miner = Some(target);
                }
            }
        } else if selected.is_some() {
            ended = true;
        }
        for event in &output.fire_events {
            if let TargetKind::Entity(target) = event.target
                && miners.contains(&target)
            {
                assert!(
                    members.contains(&event.attacker_id),
                    "this controlled history's miner fire comes from its selected Team"
                );
                fired_at.insert(target);
            }
        }
        if saved_product.is_none()
            && selected.is_some()
            && let Some(id) = held
            && sim
                .entities()
                .get(id)
                .is_some_and(|actor| actor.lifecycle.in_limbo())
        {
            let producer = created[&id];
            let factory = sim
                .production
                .factories()
                .building_factory(producer)
                .unwrap();
            let state = (factory.progress(), factory.balance());
            super::packed_cell_tests::reload(&mut scene);
            let factory = scene
                .sim()
                .production
                .factories()
                .building_factory(producer)
                .unwrap();
            assert_eq!(factory.object().unwrap().entity_id, Some(id));
            assert_eq!((factory.progress(), factory.balance()), state);
            saved_product = Some(id);
        }
        let sim = scene.sim();
        if step % 1_000 == 0 {
            let state = selected
                .and_then(|id| sim.team_script_vm.team(id))
                .map(|team| {
                    (
                        team.cursor(),
                        team.members().collect::<Vec<_>>(),
                        team.mission_target,
                    )
                });
            eprintln!(
                "step={step} created={} paid={} team={state:?} miners={:?}",
                created.len(),
                paid.len(),
                miners
                    .iter()
                    .map(|id| sim.entities().get(*id).map(|actor| actor.health.current()))
                    .collect::<Vec<_>>()
            );
        }
        if sim.ready_outcome_for_owner(enemy).is_some() {
            break;
        }
    }
    let sim = scene.sim();
    eprintln!(
        "ground end frame={} members={members:?} cursors={cursors:?} reselected={reselected} ended={ended} outcome={:?}",
        sim.session.binary_frame(),
        sim.ready_outcome_for_owner(enemy),
    );
    assert_eq!(
        members.len(),
        3,
        "stock TaskForce fills with its three produced vehicles"
    );
    assert!(members.iter().all(|id| paid.contains(id)));
    assert!(
        saved_product.is_some_and(|id| members.contains(&id)),
        "the particular restored held product is recruited"
    );
    assert_eq!(fired_at.len(), 2);
    assert!((0..=6).all(|cursor| cursors.contains(&cursor)));
    assert!(miners.iter().all(|id| {
        sim.entities()
            .get(*id)
            .is_none_or(|actor| !actor.lifecycle.object_alive())
    }));
    assert!(
        reselected,
        "target expiry searches again on the same attack-quarry cursor"
    );
    assert!(
        ended,
        "stock attack sequence reaches EOF after clearing the enemy"
    );
    assert_eq!(
        sim.ready_outcome_for_owner(enemy)
            .map(|outcome| outcome.kind),
        Some(crate::sim::house_state::HouseOutcomeKind::Defeat),
    );
    for id in members {
        assert!(sim.team_script_vm.team_for_member(id).is_none());
        let actor = sim.entities().get(id).unwrap();
        assert!(actor.lifecycle.object_alive());
        assert!(actor.attack_target.target().is_none());
    }
}
