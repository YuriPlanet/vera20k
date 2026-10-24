//! A computer house's Harrier team running its retail AIMD script.

use crate::sim::combat::TargetKind;
use crate::sim::mission::MissionType;
use crate::sim::movement::air_movement;
use crate::sim::world::Simulation;

fn mission(sim: &Simulation, id: u64) -> Option<MissionType> {
    sim.substrate.entities.get(id)?.mission.current().known()
}

/// What a frame shows of one Harrier.
#[derive(Debug, Default)]
struct Seen {
    took_miner: bool,
    fired: bool,
    spent: bool,
    cleared_target: bool,
    entered: bool,
    landed_linked: bool,
    reloaded: bool,
    guarding: bool,
}

/// Retail H2 (`0CACFEFC-G`, four ORCA) through House selection/choice4FF210,
/// FactoryAI4500F0, Team recruitment6E927A and its AIMD attack/return script.
/// Airfield, power and miner are initial scene actors on physical retail
/// geometry; only the trigger's weight is authored for predictable selection.
/// Planes and teams must come from their normal frame callers.
#[test]
#[ignore = "slow; retail aircraft production and strike uses physical map, rules and art"]
fn retail_harrier_team_strikes_the_enemy_miner_and_goes_home() {
    use crate::headless_scenario::{battle_session, load_session};
    use crate::rules::ini_parser::IniFile;
    use crate::rules::team_ai_ini::TeamAiIniRegistry;
    use crate::sim::command::{Command, CommandEnvelope};
    use crate::sim::production::FactoryHolder;
    use crate::sim::snapshot::GameSnapshot;
    use crate::sim::world::SnapshotProcessState;
    use crate::sim::world::{FrameEffects, TickLane};
    use crate::skirmish_launch::{AiDifficulty, LaunchCountry};
    use std::collections::{BTreeMap, BTreeSet};
    use std::fmt::Write as _;

    let Some((retail, assets)) = crate::rules::retail_ini_fixture::retail_assets() else {
        return;
    };
    let mut map =
        crate::map::source::load_map_ini_by_name_or_path_with_assets(&retail, "Death.mmx", &assets)
            .unwrap()
            .ini;
    let ai = TeamAiIniRegistry::load_retail(&assets, &map, true).unwrap();
    let trigger = ai
        .ai_triggers
        .iter()
        .find(|trigger| trigger.id == "0D0848BC-G")
        .unwrap();
    let mut tokens = trigger.tokens.clone();
    // Selector6F0C72 gives weight5000 priority, after normal eligibility.
    // Author that priority only; retain the stock trigger gates and H2 data.
    for weight in &mut tokens[7..10] {
        *weight = "5000".to_owned();
    }
    map.merge(&IniFile::from_str(&format!(
        "[AITriggerTypes]\n{}={}\n",
        trigger.id,
        tokens.join(",")
    )));
    let mut source = String::new();
    for name in map.section_names() {
        writeln!(source, "[{name}]").unwrap();
        for (key, value) in map.section(name).unwrap().raw_entries() {
            writeln!(source, "{key}={value}").unwrap();
        }
    }
    let directory = crate::map::source::test_support::TestDirectory::new("harrier-production");
    directory.write("harrier.map", source.as_bytes());
    let map_name = directory
        .path()
        .join("harrier.map")
        .to_str()
        .unwrap()
        .to_owned();
    let mut launch = battle_session(&map_name, &[(LaunchCountry::America, AiDifficulty::Hard)]);
    launch.local_player_mut().name = "Enemy".to_owned();
    launch.local_slot_mut().country = LaunchCountry::Russia;
    launch.options.unit_count = 0;
    launch.options.starting_credits = 10_000;
    launch.options.short_game = false;
    launch.options.crates = false;
    let mut scenario = load_session(&retail, &map_name, 0x5EED_0001, launch).unwrap();
    let map_hash = scenario.map.ini.content_hash();
    let runtime = &mut scenario.runtime;
    let sim = &mut runtime.simulation;
    let rules = &runtime.resources.rules;
    let restore_terrain = sim.resolved_terrain().unwrap().clone();
    let owners = [
        sim.interner.get("Computer1").unwrap(),
        sim.interner.get("Enemy").unwrap(),
    ];
    let mut cells: Vec<_> = sim
        .resolved_terrain()
        .unwrap()
        .iter()
        .map(|cell| (cell.rx, cell.ry))
        .collect();
    let (sum_x, sum_y) = cells.iter().fold((0_i64, 0_i64), |(x, y), &(rx, ry)| {
        (x + i64::from(rx), y + i64::from(ry))
    });
    let centre = (sum_x / cells.len() as i64, sum_y / cells.len() as i64);
    let reach = |(rx, ry): (u16, u16)| {
        (i64::from(rx) - centre.0).pow(2) + (i64::from(ry) - centre.1).pow(2)
    };
    cells.sort_by_key(|&cell| (reach(cell), cell));
    let mut spawn = |name: &str, owner: usize, cells_out: i64| {
        let house = sim.interner.resolve(owners[owner]).to_owned();
        let object = rules.object(name).unwrap();
        for &(rx, ry) in cells
            .iter()
            .filter(|&&cell| reach(cell) >= cells_out * cells_out)
        {
            if object.category == crate::rules::object_type::ObjectCategory::Building
                && !crate::sim::build_site::can_place_building_at(
                    sim,
                    rules,
                    None,
                    object,
                    (rx as i16, ry as i16),
                    Some(owners[owner]),
                    None,
                    None,
                )
            {
                continue;
            }
            if let Some(id) = sim.spawn_object(name, &house, rx, ry, 0, rules) {
                return id;
            }
        }
        panic!("no room for {name}")
    };
    spawn("GAAIRC", 0, 0);
    spawn("GAPOWR", 0, 4);
    let miner = spawn("HARV", 1, 18);
    spawn("NAPOWR", 1, 30);
    let human_mcv = sim
        .entities()
        .values()
        .find(|entity| {
            entity.owner() == owners[1] && sim.interner.resolve(entity.type_ref()) == "SMCV"
        })
        .unwrap()
        .stable_id();
    let team_type = sim.interner.get("0CACFEFC-G").expect("Allied Harrier - H2");
    let mut team = None;
    let mut produced = BTreeMap::new();
    let mut paid = BTreeSet::new();
    let mut built_on_pad = BTreeSet::new();
    let mut seen = BTreeMap::<u64, Seen>::new();
    let mut snapshot_product = None;
    let mut team_ended = false;
    for step in 0..20_000 {
        let sim = &runtime.simulation;
        let mut held = None;
        for (holder, factory) in sim.production.factories().holders_insertion_ordered() {
            let FactoryHolder::Building(producer) = holder else {
                continue;
            };
            let Some(id) = factory.object().and_then(|object| object.entity_id) else {
                continue;
            };
            let entity = sim.entities().get(id).unwrap();
            if entity.owner() == owners[0] && sim.interner.resolve(entity.type_ref()) == "ORCA" {
                produced.insert(id, producer);
                held = Some((id, producer));
                if factory.balance()
                    < sim.cost_of(
                        owners[0],
                        runtime.resources.rules.object("ORCA").unwrap(),
                        &runtime.resources.rules,
                    )
                {
                    paid.insert(id);
                }
            }
        }
        // The first post-load building legitimately clears the base centre:
        // Building440D07 -> House4FD150 scans the PRE-append roster. Deploy
        // the player's actual MCV; Unit7398CE -> House50E000 writes its base.
        let commands: Vec<_> = (step == 0)
            .then(|| {
                CommandEnvelope::new(
                    owners[1],
                    sim.session.tick(),
                    Command::DeployMcv {
                        entity_id: human_mcv,
                    },
                )
            })
            .into_iter()
            .collect();
        let output = runtime
            .advance_frame(&commands, 22, TickLane::Ordinary, FrameEffects::default())
            .unwrap();
        assert!(
            !output.outcome_waiting,
            "the aircraft history must keep advancing"
        );
        let sim = &runtime.simulation;
        for (&id, &producer) in &produced {
            if !built_on_pad.contains(&id)
                && let Some(entity) = sim.entities().get(id)
                && entity.lifecycle.object_alive()
                && !entity.lifecycle.in_limbo()
            {
                let dock = entity.ammo().unwrap().dock().expect("product's dock");
                assert_eq!(dock, producer, "first exit retains its actual factory");
                assert!(entity.radio_contacts.contains(producer));
                assert!(
                    sim.entities()
                        .get(producer)
                        .unwrap()
                        .radio_contacts
                        .contains(id)
                );
                assert_eq!(
                    air_movement::current_fly_height(entity, sim.resolved_terrain()),
                    0
                );
                let pad = crate::sim::movement::building_dock_coordinate(
                    sim.entities(),
                    producer,
                    Some(id),
                    sim.resolved_terrain(),
                    &runtime.resources.rules,
                    &sim.interner,
                )
                .unwrap();
                assert_eq!(
                    crate::sim::movement::ground_pose::object_get_coords(
                        entity,
                        sim.resolved_terrain()
                    ),
                    pad,
                    "first live pose is its assigned pad (Exit443FE5..44404D)"
                );
                built_on_pad.insert(id);
            }
        }
        if team.is_none() {
            team = sim
                .team_script_vm
                .first_team_of_type(team_type)
                .map(|state| state.id());
        }
        if let Some(state) = team.and_then(|team| sim.team_script_vm.team(team)) {
            for id in state.members() {
                assert!(
                    produced.contains_key(&id),
                    "recruitment retains the factory's constructed plane"
                );
                seen.entry(id).or_default();
            }
        } else if team.is_some() {
            team_ended = true;
        }
        for (&harrier, seen) in &mut seen {
            let entity = sim.entities().get(harrier).expect("Harrier survives");
            let ammo = entity.ammo().unwrap();
            seen.took_miner |= entity.attack_target.target() == Some(TargetKind::Entity(miner));
            seen.fired |= output.fire_events.iter().any(|event| {
                event.attacker_id == harrier && event.target == TargetKind::Entity(miner)
            });
            seen.spent |= ammo.current() < ammo.max();
            seen.cleared_target |= seen.spent && entity.attack_target.target().is_none();
            seen.entered |= seen.fired && mission(sim, harrier) == Some(MissionType::Enter);
            let linked = ammo
                .dock()
                .and_then(|dock| sim.entities().get(dock))
                .is_some_and(|dock| {
                    entity.radio_contacts.contains(dock.stable_id())
                        && dock.radio_contacts.contains(harrier)
                        && dock.owner() == owners[0]
                        && sim
                            .object_type(dock.type_ref(), &runtime.resources.rules)
                            .is_some_and(|kind| kind.unit_reload)
                });
            let height = air_movement::current_fly_height(entity, sim.resolved_terrain());
            seen.landed_linked |= seen.spent && seen.entered && linked && height == 0;
            if seen.spent && !seen.reloaded && ammo.current() == ammo.max() {
                assert!(
                    seen.landed_linked,
                    "first refill follows return and landing"
                );
                assert!(
                    entity.attack_target.target().is_none(),
                    "refill requires a cleared target"
                );
                seen.reloaded = true;
            }
            seen.guarding |= seen.reloaded
                && team_ended
                && mission(sim, harrier) == Some(MissionType::Guard)
                && ammo.current() == ammo.max()
                && linked
                && height == 0;
        }
        // Preserve an actual held product and House-created demand across load.
        if snapshot_product.is_none()
            && team.is_some()
            && held.is_some_and(|(id, _)| {
                sim.entities().get(id).is_some_and(|entity| {
                    entity.lifecycle.object_alive() && entity.lifecycle.in_limbo()
                })
            })
        {
            let sim = &runtime.simulation;
            let rules = &runtime.resources.rules;
            let (held_id, producer) = held.unwrap();
            let factory = sim
                .production
                .factories()
                .building_factory(producer)
                .unwrap();
            assert_eq!(factory.object().unwrap().entity_id, Some(held_id));
            let saved_factory = (factory.progress(), factory.balance());
            let rules_hash = rules.simulation_config_hash();
            let bytes = GameSnapshot::save_validated(sim, map_hash, rules_hash, "harrier", 0);
            let mut loaded =
                GameSnapshot::load_validated(&bytes, map_hash, rules_hash, sim.session.map_name())
                    .unwrap()
                    .sim;
            loaded.retain_process_state_for_load(SnapshotProcessState::Live(sim));
            loaded.bind_shared_cell_dummy(crate::map::resolved_terrain::SharedCellDummy::fresh());
            loaded.reconstruct_cellclass_dummy_for_map_resize();
            loaded.restore_after_snapshot_load().unwrap();
            crate::sim::production::validate_restored_factory_state(&loaded, rules).unwrap();
            let factory = loaded
                .production
                .factories()
                .building_factory(producer)
                .unwrap();
            assert_eq!(factory.object().unwrap().entity_id, Some(held_id));
            assert_eq!((factory.progress(), factory.balance()), saved_factory);
            assert!(loaded.entities().get(held_id).unwrap().lifecycle.in_limbo());
            loaded.rebuild_caches_after_load(
                restore_terrain.clone(),
                sim.terrain_speed_config().clone(),
                rules,
            );
            loaded
                .restore_map_authority_after_snapshot_load(
                    rules,
                    &runtime.resources.overlay_registry,
                    None,
                )
                .unwrap();
            loaded.intern_rule_type_ids(rules);
            loaded.resolve_type_handles(rules);
            loaded.rebuild_lighting_sources_after_load(rules);
            let live_dummy = sim.effective_shared_cell_dummy();
            live_dummy.adopt_prepared_load_state(&loaded.effective_shared_cell_dummy());
            loaded.bind_shared_cell_dummy(live_dummy);
            loaded.install_fresh_process_main(sim.main_rng());
            loaded.publish_scenario_started();
            runtime.replace_simulation(loaded);
            snapshot_product = Some(held_id);
        }
        if seen.len() == 4 && seen.values().all(|seen| seen.guarding) {
            break;
        }
    }
    let sim = &runtime.simulation;
    eprintln!(
        "frame={} team={team:?} ended={team_ended} produced={} seen={seen:?}",
        sim.session.binary_frame(),
        produced.len()
    );
    let miner_health = sim.entities().get(miner).map(|e| e.health.current());
    assert!(
        miner_health.is_some_and(|health| health < 1000),
        "the strikes hurt the miner: {miner_health:?}"
    );
    assert!(
        snapshot_product.is_some_and(|id| seen.contains_key(&id)),
        "the specific saved factory-held plane is an H2 recruit"
    );
    assert!(team_ended, "the script ran past its end");
    assert_eq!(
        seen.len(),
        4,
        "the retail H2 TaskForce recruited four produced planes"
    );
    for (n, (&id, seen)) in seen.iter().enumerate() {
        assert!(
            paid.contains(&id),
            "plane {id} was paid for in its real factory"
        );
        assert!(
            built_on_pad.contains(&id),
            "plane {id} left its factory on a linked pad"
        );
        assert!(seen.took_miner, "Harrier {n} takes the miner: {seen:?}");
        assert!(seen.fired, "Harrier {n} strikes: {seen:?}");
        assert!(seen.spent, "Harrier {n} debits its pending ammo: {seen:?}");
        assert!(
            seen.cleared_target,
            "Harrier {n} clears its target before reloading: {seen:?}"
        );
        assert!(seen.entered, "Harrier {n} Enters its dock: {seen:?}");
        assert!(seen.landed_linked, "Harrier {n} lands linked: {seen:?}");
        assert!(seen.reloaded, "Harrier {n} reloads: {seen:?}");
        assert!(seen.guarding, "Harrier {n} guards on its pad: {seen:?}");
    }
}
