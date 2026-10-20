//! Robot Tanks and their Robot Control Center: the spark block against
//! original execution (`tools/robot_offline_oracle.{py,json,meta.json}`,
//! `sparks`; its `hover_power_off` section is `movement::hover`'s), the
//! retail keys through the production readers, and retail GAROBO and ROBO
//! through the production frame.

use super::*;
use crate::map::entities::EntityCategory;
use crate::rules::ini_parser::IniFile;
use crate::rules::sound_ini::SoundRegistry;
use crate::sim::combat::{EntityDamageEvent, RAD_NO_ATTACKER, ReceiverCallFlags};
use crate::sim::command::{Command, CommandEnvelope};
use crate::sim::game_entity::GameEntity;
use crate::sim::house_state::HouseState;
use crate::sim::mission::{MissionId, MissionType};
use crate::sim::pathfinding::PathGrid;
use crate::sim::rng::SimRng;

fn int(value: &serde_json::Value) -> i32 {
    value.as_i64().expect("integer") as i32
}

/// `[CombatDamage] DefaultSparkSystem=` naming a Spark system.
fn spark_rules() -> RuleSet {
    RuleSet::from_ini(&IniFile::from_str(
        "[CombatDamage]\nDefaultSparkSystem=SparkSys\n[ParticleSystems]\n0=SparkSys\n\
         [SparkSys]\nBehavesLike=Spark\nHoldsWhat=Spark\nLifetime=5\n\
         [Particles]\n0=Spark\n[Spark]\nBehavesLike=Spark\n",
    ))
    .unwrap()
}

/// `UnitClass::AI`'s spark block (`0x007360C9..0x007361A4`) through
/// [`Simulation::deactivated_unit_sparks`]: the phase over the Location, the
/// Frame and the ID, the two Scenario draws (Y, then X) and the system's
/// constructor arguments.
#[test]
fn offline_sparks_match_original_execution() {
    let vectors: serde_json::Value =
        serde_json::from_str(crate::test_fixture::text("tools/robot_offline_oracle.json"))
            .expect("robot offline oracle vectors");
    let rules = spark_rules();
    let spark = rules.ps_type_id_by_name("SparkSys").unwrap();
    let mut rows = 0;
    for row in vectors["sparks"].as_array().unwrap() {
        let name = row["name"].as_str().unwrap();
        let location = [0, 1, 2].map(|axis| int(&row["location"][axis]));
        let mut entity = GameEntity::test_default(1, "ROBO", "Americans", 0, 0);
        entity.position.put_world_xy([location[0], location[1]]);
        entity
            .position
            .set_exact_z_leptons_for_test(Some(location[2]));
        entity.set_native_unique_id_for_test(int(&row["id"]));
        entity
            .deactivated
            .set_for_test(row["deactivated"].as_bool().unwrap());
        let mut sim = Simulation::new();
        sim.substrate.entities.insert(entity);
        sim.session
            .set_binary_frame_for_test(row["frame"].as_u64().unwrap() as u32);
        let seed = row["seed"].as_u64().unwrap();
        sim.scenario_rng = SimRng::new(seed);
        sim.deactivated_unit_sparks(1, &rules);

        let mut expected = row["expected"].clone();
        let draws = expected.as_object_mut().unwrap().remove("draws").unwrap();
        let systems: Vec<_> = sim
            .particle_systems()
            .iter()
            .map(|(_, system)| {
                assert_eq!(system.type_id(), spark, "{name}");
                let coord = |coord: glam::IVec3| serde_json::json!([coord.x, coord.y, coord.z]);
                serde_json::json!({
                    "coords": coord(system.coords()),
                    "target": system.target_ref().map_or(0, |_| 1),
                    "owner": system.owner_entity().map_or(0, |_| 1),
                    "target_coords": coord(system.target_coords()),
                    "house": system.owner_house().map_or(0, |_| 1),
                })
            })
            .collect();
        let rng = sim.scenario_rng.logical_view();
        let actual = serde_json::json!({
            "systems": systems,
            "rng_indices": [rng.index_a, rng.index_b],
        });
        assert_eq!(actual, expected, "{name}");
        let mut replay = SimRng::new(seed);
        for draw in draws.as_array().unwrap() {
            assert_eq!(
                replay.next_range_i32_inclusive(int(&draw[0]), int(&draw[1])),
                int(&draw[2]),
                "{name}"
            );
        }
        assert_eq!(replay.state(), sim.scenario_rng.state(), "{name}");
        rows += 1;
    }
    assert_eq!(rows, 17);
}

/// Retail `rulesmd.ini` and `artmd.ini`, with the type sounds bound against
/// retail `soundmd.ini`; `None` when the retail INIs are absent.
fn retail_rules() -> Option<RuleSet> {
    let rules_ini = crate::rules::retail_ini_fixture::retail_ini("rulesmd.ini")?;
    let sounds = SoundRegistry::from_ini(&crate::rules::retail_ini_fixture::retail_ini(
        "soundmd.ini",
    )?);
    let mut rules = crate::rules::retail_ini_fixture::retail_unlayered_rules_with_art()?;
    rules.bind_type_sound_references(&rules_ini, &sounds);
    Some(rules)
}

/// The five keys' readers: `PowersUnit=` (`0x007132DF`), `PoweredUnit=`
/// (`0x0071330F`), `VoiceSelectDeactivated=` (`0x00712BF1`), and
/// `ActivateSound=`/`DeactivateSound=` (`0x007138D1`, `0x0071390D`, whose
/// constructor default is -1 at `0x00710F99`; `[AudioVisual]`'s keys of the
/// same names are Rules fields this chain does not read).
#[test]
fn retail_rules_read_the_robot_keys() {
    let plain_ini = IniFile::from_str("[VehicleTypes]\n0=PLAIN\n[PLAIN]\nStrength=1\n");
    let mut plain = RuleSet::from_ini(&plain_ini).unwrap();
    plain.bind_type_sound_references(&plain_ini, &SoundRegistry::from_ini(&IniFile::from_str("")));
    let object = plain.object("PLAIN").unwrap();
    assert_eq!(object.powers_unit, None);
    assert!(!object.powered_unit);
    assert!(object.voice_select_deactivated.is_empty());
    assert_eq!(
        (&object.activate_sound, &object.deactivate_sound),
        (&None, &None)
    );

    let Some(rules) = retail_rules() else {
        return;
    };
    assert_eq!(
        rules.object("GAROBO").unwrap().powers_unit.as_deref(),
        Some("ROBO")
    );
    let robo = rules.object("ROBO").unwrap();
    assert!(robo.powered_unit);
    assert_eq!(
        robo.voice_select_deactivated,
        ["RobotTankSelectDeactivated"]
    );
    assert_eq!(robo.activate_sound.as_deref(), Some("RobotTankOnline"));
    assert_eq!(robo.deactivate_sound.as_deref(), Some("RobotTankOffline"));
    let named = |keep: fn(&crate::rules::object_type::ObjectType) -> bool| {
        rules
            .all_objects()
            .filter(|object| keep(object))
            .map(|object| object.id.as_str())
            .collect::<Vec<_>>()
    };
    assert_eq!(named(|object| object.powers_unit.is_some()), ["GAROBO"]);
    assert_eq!(named(|object| object.powered_unit), ["ROBO"]);
    assert_eq!(
        rules.combat_damage.default_spark_system.as_deref(),
        Some("SparkSys")
    );
}

/// The flat arena with the human `Americans`, the local player, and the
/// computer `Russians`.
fn arena(rules: &RuleSet) -> (Simulation, PathGrid) {
    let mut sim = crate::sim::arena_fixture::rival_arena(rules, 0x0050_E0E0);
    sim.intern_rule_type_ids(rules);
    sim.resolve_type_handles(rules);
    sim.session
        .set_game_mode(crate::sim::scenario_session::ScenarioGameMode::from_legacy_nonzero(true));
    sim.session
        .set_current_house_for_test(sim.interner.get("Americans"));
    let grid = sim
        .path_grid_snapshot()
        .map(|grid| (*grid).clone())
        .unwrap();
    (sim, grid)
}

fn spawn(sim: &mut Simulation, rules: &RuleSet, kind: &str, owner: &str, rx: u16, ry: u16) -> u64 {
    sim.spawn_object_at_height(kind, owner, rx, ry, 0, 0, rules)
        .unwrap_or_else(|| panic!("{kind} spawns"))
}

/// Runs the production frame until `done` holds, for at most `frames`
/// frames, keeping every sound event; answers whether it held.
fn run_until(
    sim: &mut Simulation,
    rules: &RuleSet,
    grid: &PathGrid,
    frames: u32,
    events: &mut Vec<SimSoundEvent>,
    mut done: impl FnMut(&Simulation) -> bool,
) -> bool {
    for _ in 0..frames {
        let commands = sim.take_due_commands();
        sim.advance_tick(&commands, Some(rules), Some(grid), None, 33);
        events.append(&mut sim.sound_events);
        if done(sim) {
            return true;
        }
    }
    false
}

fn offline(sim: &Simulation, id: u64) -> bool {
    sim.substrate
        .entities
        .get(id)
        .unwrap()
        .deactivated
        .is_offline()
}

fn sounds(events: &[SimSoundEvent], name: &str) -> usize {
    events
        .iter()
        .filter(|event| matches!(event, SimSoundEvent::VocAt { sound_id, .. } if sound_id == name))
        .count()
}

fn lines(events: &[SimSoundEvent], house: InternedId, line: &str) -> usize {
    events
        .iter()
        .filter(|event| {
            matches!(event, SimSoundEvent::HouseEva { owner, event } if *owner == house && *event == line)
        })
        .count()
}

/// Destroys `id` as expiring C4 does: no attacker and no survivors.
fn destroy(sim: &mut Simulation, rules: &RuleSet, id: u64) {
    let health = sim.substrate.entities.get(id).unwrap().health.current();
    let c4 = sim.interner.intern(&rules.bridge_warheads.c4_name);
    let hit = EntityDamageEvent::direct_receiver(
        id,
        health,
        0,
        RAD_NO_ATTACKER,
        None,
        c4,
        ReceiverCallFlags {
            ignore_defenses: true,
            arg6: true,
        },
    );
    sim.commit_direct_damage_receiver(rules, None, hit, FrameEffects::default());
    assert!(
        sim.substrate
            .entities
            .get(id)
            .is_none_or(|entity| entity.death.is_dying())
    );
}

/// The object's cell.
fn cell(sim: &Simulation, id: u64) -> (u16, u16) {
    let entity = sim.substrate.entities.get(id).unwrap();
    (entity.position.rx(), entity.position.ry())
}

/// The object's height above the ground (GetHeight).
fn height(sim: &Simulation, id: u64) -> i32 {
    crate::sim::movement::air_movement::current_fly_height(
        sim.substrate.entities.get(id).unwrap(),
        sim.resolved_terrain.as_ref(),
    )
}

/// Orders the player's `id` to the cell `target`.
fn order_move(sim: &mut Simulation, id: u64, target: (u16, u16)) {
    let americans = sim.interner.get("Americans").unwrap();
    sim.queue_command(CommandEnvelope::new(
        americans,
        sim.session.tick() + 1,
        Command::Move {
            entity_id: id,
            target_rx: target.0,
            target_ry: target.1,
        },
    ));
}

/// GAROBO's operational edges through the production frame: its first
/// working frame counts it and Reactivates the house's ROBOs (no sound, as
/// none was offline); losing the only power plant leaves the house in low
/// power, which ends its operation, and LostPoweredCenter Deactivates them
/// (reset to Guard, deselected, the type's `DeactivateSound=`), not another
/// house's ROBO or another type. Offline, each throws one spark system every
/// 30 frames and refuses control; a new power plant brings them back with
/// `ActivateSound=`.
#[test]
fn losing_and_regaining_the_last_powered_center_switches_robot_tanks() {
    let Some(rules) = retail_rules() else {
        return;
    };
    let (mut sim, grid) = arena(&rules);
    let americans = sim.interner.get("Americans").unwrap();
    let russians = sim.interner.get("Russians").unwrap();
    let plant = spawn(&mut sim, &rules, "GAPOWR", "Americans", 3, 3);
    spawn(&mut sim, &rules, "GAROBO", "Americans", 8, 3);
    spawn(&mut sim, &rules, "GAPOWR", "Russians", 3, 26);
    spawn(&mut sim, &rules, "GAROBO", "Russians", 8, 26);
    let tanks = [
        spawn(&mut sim, &rules, "ROBO", "Americans", 12, 14),
        spawn(&mut sim, &rules, "ROBO", "Americans", 15, 14),
    ];
    let grizzly = spawn(&mut sim, &rules, "MTNK", "Americans", 18, 14);
    let rival = spawn(&mut sim, &rules, "ROBO", "Russians", 22, 24);
    let mut events = Vec::new();
    assert!(run_until(
        &mut sim,
        &rules,
        &grid,
        300,
        &mut events,
        |sim| { sim.has_powered_center(americans) && sim.has_powered_center(russians) }
    ));
    assert_eq!(lines(&events, americans, EVA_ROBOT_TANKS_BACK_ONLINE), 1);
    assert_eq!(lines(&events, russians, EVA_ROBOT_TANKS_BACK_ONLINE), 1);
    assert_eq!(sounds(&events, "RobotTankOnline"), 0);
    sim.substrate
        .entities
        .get_mut(tanks[0])
        .unwrap()
        .selection
        .set_selected_for_test(true);

    events.clear();
    destroy(&mut sim, &rules, plant);
    assert!(run_until(
        &mut sim,
        &rules,
        &grid,
        300,
        &mut events,
        |sim| { tanks.iter().all(|&id| offline(sim, id)) }
    ));
    assert!(!sim.has_powered_center(americans));
    assert_eq!(lines(&events, americans, EVA_ROBOT_TANKS_OFFLINE), 1);
    assert_eq!(sounds(&events, "RobotTankOffline"), 2);
    let guard = MissionId::from_known(MissionType::Guard);
    for id in tanks {
        let tank = sim.substrate.entities.get(id).unwrap();
        assert!(!tank.selection.is_selected());
        assert!([tank.mission.current(), tank.mission.queued()].contains(&guard));
        assert!(!sim.techno_player_controllable(id, &rules));
        assert!(!sim.techno_is_active(id, &rules));
    }
    assert!(!offline(&sim, grizzly) && !offline(&sim, rival));
    assert!(sim.techno_player_controllable(grizzly, &rules));
    assert!(sim.techno_is_active(grizzly, &rules));
    assert_eq!(
        sim.selection_voice_request(&rules, tanks[0], true),
        Some("RobotTankSelectDeactivated")
    );

    let spark = rules.ps_type_id_by_name("SparkSys").unwrap();
    let mut sparks = std::collections::BTreeSet::new();
    run_until(&mut sim, &rules, &grid, 30, &mut events, |sim| {
        sparks.extend(
            sim.particle_systems()
                .iter()
                .filter(|(_, system)| system.type_id() == spark)
                .map(|(&id, _)| id),
        );
        false
    });
    assert_eq!(sparks.len(), 2, "one system per offline tank in 30 frames");

    events.clear();
    spawn(&mut sim, &rules, "GAPOWR", "Americans", 3, 8);
    assert!(run_until(
        &mut sim,
        &rules,
        &grid,
        300,
        &mut events,
        |sim| { tanks.iter().all(|&id| !offline(sim, id)) }
    ));
    assert!(sim.has_powered_center(americans));
    assert_eq!(lines(&events, americans, EVA_ROBOT_TANKS_BACK_ONLINE), 1);
    assert_eq!(sounds(&events, "RobotTankOnline"), 2);
    assert!(sim.techno_player_controllable(tanks[0], &rules));
}

/// Removed_From_Game's arm (`0x0050263B..0x00502776`): a destroyed center
/// stops counting as it leaves the map, with no operational edge, and its
/// house's ROBOs go offline in the same call.
#[test]
fn destroying_the_center_switches_its_robot_tanks_off() {
    let Some(rules) = retail_rules() else {
        return;
    };
    let (mut sim, grid) = arena(&rules);
    let americans = sim.interner.get("Americans").unwrap();
    spawn(&mut sim, &rules, "GAPOWR", "Americans", 3, 3);
    let center = spawn(&mut sim, &rules, "GAROBO", "Americans", 8, 3);
    let tanks = [
        spawn(&mut sim, &rules, "ROBO", "Americans", 12, 14),
        spawn(&mut sim, &rules, "ROBO", "Americans", 15, 14),
    ];
    let mut events = Vec::new();
    assert!(run_until(
        &mut sim,
        &rules,
        &grid,
        300,
        &mut events,
        |sim| sim.has_powered_center(americans)
    ));

    events.clear();
    destroy(&mut sim, &rules, center);
    events.append(&mut sim.sound_events);
    assert!(tanks.iter().all(|&id| offline(&sim, id)));
    assert!(!sim.has_powered_center(americans));
    assert_eq!(lines(&events, americans, EVA_ROBOT_TANKS_OFFLINE), 1);
    assert_eq!(sounds(&events, "RobotTankOffline"), 2);
}

/// ChangeOwner (`0x0070158A..0x007015E6`) runs Removed_From_Game on the old
/// house: a captured center stops counting there, and its old house's ROBOs
/// go offline in the same call. Added_To_Game never counts, and Building
/// `+0x6C8` has no ownership-change writer, so the captor counts it only at
/// its next operational edge: here, when the captor's power fails and
/// returns.
#[test]
fn a_captured_center_counts_for_its_captor_only_at_its_next_edge() {
    let Some(rules) = retail_rules() else {
        return;
    };
    let (mut sim, grid) = arena(&rules);
    let americans = sim.interner.get("Americans").unwrap();
    let russians = sim.interner.get("Russians").unwrap();
    spawn(&mut sim, &rules, "GAPOWR", "Americans", 3, 3);
    let center = spawn(&mut sim, &rules, "GAROBO", "Americans", 8, 3);
    let tanks = [
        spawn(&mut sim, &rules, "ROBO", "Americans", 12, 14),
        spawn(&mut sim, &rules, "ROBO", "Americans", 15, 14),
    ];
    let plant = spawn(&mut sim, &rules, "GAPOWR", "Russians", 3, 26);
    let rival = spawn(&mut sim, &rules, "ROBO", "Russians", 22, 24);
    let mut events = Vec::new();
    assert!(run_until(
        &mut sim,
        &rules,
        &grid,
        300,
        &mut events,
        |sim| sim.has_powered_center(americans)
    ));

    events.clear();
    sim.change_owner_with_rules(center, russians, &rules, None, FrameEffects::default());
    events.append(&mut sim.sound_events);
    assert!(tanks.iter().all(|&id| offline(&sim, id)));
    assert!(!sim.has_powered_center(americans));
    assert_eq!(lines(&events, americans, EVA_ROBOT_TANKS_OFFLINE), 1);
    assert_eq!(sounds(&events, "RobotTankOffline"), 2);
    // Working for the Russians, it does not count for them.
    run_until(&mut sim, &rules, &grid, 60, &mut events, |_| false);
    assert!(!sim.has_powered_center(russians));

    let working = |sim: &Simulation| {
        sim.substrate
            .entities
            .get(center)
            .unwrap()
            .building_last_operational
            .is_operational()
    };
    destroy(&mut sim, &rules, plant);
    assert!(run_until(
        &mut sim,
        &rules,
        &grid,
        300,
        &mut events,
        |sim| !working(sim)
    ));
    assert!(!sim.has_powered_center(russians));
    // Without a counted center, the rival goes offline if it moves.
    let rival_was_offline = offline(&sim, rival);
    events.clear();
    spawn(&mut sim, &rules, "GAPOWR", "Russians", 14, 26);
    assert!(run_until(
        &mut sim,
        &rules,
        &grid,
        300,
        &mut events,
        |sim| sim.has_powered_center(russians)
    ));
    assert!(working(&sim));
    assert_eq!(lines(&events, russians, EVA_ROBOT_TANKS_BACK_ONLINE), 1);
    assert_eq!(
        sounds(&events, "RobotTankOnline"),
        usize::from(rival_was_offline)
    );
    assert!(!offline(&sim, rival));
    assert!(tanks.iter().all(|&id| offline(&sim, id)));
}

/// The Unit's cell arrival (`0x00739F21..0x00739F59`): a hovering ROBO
/// whose house has no powered center goes offline at its first step, which
/// Hover processes before it leaves its cell (`TryAdvancePathStep`'s
/// Per_Cell_Process at `0x00515A1C` precedes the claim). Stop_Moving
/// (`0x00516320`) keeps the head, and its height keeps it powered
/// (`0x00516C70`) as it settles, so it drifts toward that head until it
/// touches down; from a standing start that is inside its own cell, where
/// it stays.
#[test]
fn a_robot_tank_without_a_powered_center_goes_offline_when_it_moves() {
    let Some(rules) = retail_rules() else {
        return;
    };
    let (mut sim, grid) = arena(&rules);
    let americans = sim.interner.get("Americans").unwrap();
    // A building keeps the house in the game (ShortGame=yes).
    spawn(&mut sim, &rules, "GAPOWR", "Americans", 3, 3);
    let tank = spawn(&mut sim, &rules, "ROBO", "Americans", 10, 16);
    let mut events = Vec::new();
    assert!(run_until(
        &mut sim,
        &rules,
        &grid,
        300,
        &mut events,
        |sim| height(sim, tank) >= rules.general.hover_height / 2
    ));
    let start_x = sim.substrate.entities.get(tank).unwrap().position.sub_x();
    order_move(&mut sim, tank, (16, 16));
    assert!(run_until(
        &mut sim,
        &rules,
        &grid,
        300,
        &mut events,
        |sim| offline(sim, tank)
    ));
    assert_eq!(cell(&sim, tank), (10, 16));
    assert!(height(&sim, tank) > 0);
    assert_eq!(sounds(&events, "RobotTankOffline"), 1);
    assert_eq!(lines(&events, americans, EVA_ROBOT_TANKS_OFFLINE), 0);
    run_until(&mut sim, &rules, &grid, 120, &mut events, |_| false);
    let rest = sim.substrate.entities.get(tank).unwrap().position;
    assert_eq!(height(&sim, tank), 0);
    assert_eq!(cell(&sim, tank), (10, 16));
    assert!(rest.sub_x() > start_x, "it drifted toward its head, east");
    run_until(&mut sim, &rules, &grid, 120, &mut events, |_| false);
    let now = sim.substrate.entities.get(tank).unwrap().position;
    assert_eq!(
        (now.rx(), now.ry(), now.sub_x(), now.sub_y()),
        (rest.rx(), rest.ry(), rest.sub_x(), rest.sub_y())
    );
    assert!(offline(&sim, tank));
    assert_eq!(sounds(&events, "RobotTankOffline"), 1);
}

/// Retail ROBO through the production frame: an established hover survives
/// water while GAROBO works. Losing its center or the center's last power
/// plant spins it down, admits DropAsBomb below the native 104-lepton gate,
/// then lands the fall through the C4 receiver and removes its occupation.
/// The same power edges on land leave a living, grounded offline tank.
#[test]
fn losing_robot_power_drops_water_hover_and_keeps_land_hover_alive() {
    let Some(mut rules) = retail_rules() else {
        return;
    };
    for lose_center in [false, true] {
        let wake = rules.general.wake.name.clone();
        rules.bind_anim_frame_count_for_test(&wake, 100);
        for splash in rules.combat_damage.splash_list.clone() {
            rules.bind_anim_frame_count_for_test(&splash, 100);
        }
        let (mut sim, grid) = arena(&rules);
        let americans = sim.interner.get("Americans").unwrap();
        let plant = spawn(&mut sim, &rules, "GAPOWR", "Americans", 3, 3);
        let center = spawn(&mut sim, &rules, "GAROBO", "Americans", 8, 3);
        // A second structure keeps the house active after its plant dies.
        spawn(&mut sim, &rules, "GAPILE", "Americans", 3, 8);
        let water_cell = (12, 14);
        let land_cell = (18, 14);
        let water_tank = spawn(
            &mut sim,
            &rules,
            "ROBO",
            "Americans",
            water_cell.0,
            water_cell.1,
        );
        let land_tank = spawn(
            &mut sim,
            &rules,
            "ROBO",
            "Americans",
            land_cell.0,
            land_cell.1,
        );
        let mut events = Vec::new();
        let established = run_until(&mut sim, &rules, &grid, 300, &mut events, |sim| {
            sim.has_powered_center(americans) && height(sim, water_tank) > 0
        });
        assert!(
            established,
            "center={} water_height={} land_height={} water_offline={} land_offline={}",
            sim.has_powered_center(americans),
            height(&sim, water_tank),
            height(&sim, land_tank),
            offline(&sim, water_tank),
            offline(&sim, land_tank)
        );
        // Change only the terrain under an already established hover. This
        // fixture does not invent altitude, spring offset or locomotor state.
        let terrain_cell = sim
            .resolved_terrain
            .as_mut()
            .unwrap()
            .cell_mut_for_test(water_cell.0, water_cell.1)
            .unwrap();
        terrain_cell.land_type = 2;
        terrain_cell.base_land_type = 2;
        run_until(&mut sim, &rules, &grid, 60, &mut events, |sim| {
            let tank = sim
                .substrate
                .entities
                .get(water_tank)
                .expect("powered water hover remains alive");
            assert!(!tank.deactivated.is_offline() && !tank.was_dropped_as_bomb());
            assert!(tank.health.current() > 0);
            false
        });
        assert!(
            sim.substrate
                .occupancy
                .contains_entity(water_cell.0, water_cell.1, water_tank)
        );
        events.clear();
        destroy(&mut sim, &rules, if lose_center { center } else { plant });
        let mut saw_bomb = false;
        let splash = rules.combat_damage.splash_list.last();
        let mut saw_splash = false;
        assert!(
            run_until(&mut sim, &rules, &grid, 300, &mut events, |sim| {
                if let Some(tank) = sim.substrate.entities.get(water_tank) {
                    saw_bomb |= tank.was_dropped_as_bomb();
                }
                if let Some(splash) = splash {
                    saw_splash |= sim.substrate.anims.iter().any(|(_, anim)| {
                        sim.interner.resolve(anim.type_id()) == splash.as_str()
                            && anim.world_coord().to_cell_sub_z().0 == water_cell.0
                            && anim.world_coord().to_cell_sub_z().1 == water_cell.1
                    });
                }
                sim.substrate.entities.get(water_tank).is_none()
            }),
            "water hover must die after losing {}",
            if lose_center { "center" } else { "power" }
        );
        assert!(saw_bomb, "the production water gate admitted DropAsBomb");
        if splash.is_some() {
            assert!(
                saw_splash,
                "the native Unit water-death arm emitted the last SplashList animation"
            );
        }
        assert!(!sim.has_powered_center(americans));
        assert!(
            !sim.substrate
                .occupancy
                .contains_entity(water_cell.0, water_cell.1, water_tank)
        );
        assert_eq!(
            sim.substrate
                .raw_cell_occupation
                .ground_bits(water_cell.0, water_cell.1)
                & crate::sim::occupancy::VEHICLE_OCCUPATION_BIT,
            0
        );
        run_until(&mut sim, &rules, &grid, 120, &mut events, |_| false);
        let land_tank = sim
            .substrate
            .entities
            .get(land_tank)
            .expect("land hover stays represented");
        assert!(land_tank.deactivated.is_offline() && land_tank.health.current() > 0);
        assert!(!land_tank.was_dropped_as_bomb() && !land_tank.is_falling_down());
        assert_eq!(
            crate::sim::movement::air_movement::current_fly_height(
                land_tank,
                sim.resolved_terrain.as_ref()
            ),
            0
        );
    }
}

/// The latch (Techno `+0x1C8`), a building's share (`+0x662`) and the
/// house's count (`+0x2D8`) survive a save and each changes the world hash.
#[test]
fn robot_tank_state_is_saved_and_hashed() {
    use crate::sim::snapshot::GameSnapshot;

    let mut sim = Simulation::new();
    sim.interner = crate::sim::intern::test_interner();
    // Load resets the Scenario Random to seed zero.
    sim.scenario_rng = SimRng::new(0);
    let house = sim.interner.intern("Americans");
    sim.houses
        .register(HouseState::new(house, 0, None, true, 0, 10));
    let tank = sim.allocate_stable_id();
    sim.substrate
        .entities
        .insert(GameEntity::test_default(tank, "ROBO", "Americans", 5, 5));
    let center = sim.allocate_stable_id();
    sim.substrate
        .entities
        .insert(GameEntity::test_default_of_category(
            center,
            "GAROBO",
            "Americans",
            8,
            8,
            EntityCategory::Structure,
        ));
    let mut seen = vec![sim.state_hash()];
    sim.substrate
        .entities
        .get_mut(tank)
        .unwrap()
        .deactivated
        .set_for_test(true);
    seen.push(sim.state_hash());
    sim.substrate
        .entities
        .get_mut(center)
        .unwrap()
        .building_powered_center
        .set_for_test(true);
    seen.push(sim.state_hash());
    assert_eq!(sim.houses.get_mut(&house).unwrap().gain_powered_center(), 1);
    seen.push(sim.state_hash());
    let distinct: std::collections::BTreeSet<_> = seen.iter().collect();
    assert_eq!(distinct.len(), seen.len(), "each change moves the hash");

    let bytes = GameSnapshot::save(&sim, 0, 0, "robot tanks", 0);
    let mut restored = GameSnapshot::load(&bytes).unwrap().sim;
    restored.restore_after_snapshot_load().unwrap();
    assert!(
        restored
            .substrate
            .entities
            .get(tank)
            .unwrap()
            .deactivated
            .is_offline()
    );
    assert!(
        restored
            .substrate
            .entities
            .get(center)
            .unwrap()
            .building_powered_center
            .is_counted()
    );
    assert!(restored.has_powered_center(house));
    assert_eq!(restored.state_hash(), sim.state_hash());
}
