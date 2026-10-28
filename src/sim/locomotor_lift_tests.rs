//! Rust regressions for the Magnetron's lift through the production frame on
//! retail rules (the local `ini/`; skipped without it), and for the state it
//! keeps. They check VERA's composition of the native bodies cited in
//! `locomotor_lift`, `jumpjet_flight` and `world::crash`; no native execution
//! backs them.

use super::LocomotorShotTarget;
use crate::rules::locomotor_type::LocomotorKind;
use crate::rules::ruleset::RuleSet;
use crate::sim::command::{Command, CommandEnvelope};
use crate::sim::game_entity::GameEntity;
use crate::sim::mission::{MissionId, MissionType};
use crate::sim::pathfinding::PathGrid;
use crate::sim::world::{FrameEffects, Simulation};

fn retail_rules() -> Option<RuleSet> {
    Some(crate::rules::retail_ini_fixture::retail_battle_rules()?.rules)
}

/// The flat arena: `Americans` (human) field the Magnetron, `Russians`
/// (computer) the tanks.
fn arena(rules: &RuleSet) -> (Simulation, PathGrid) {
    let mut sim = crate::sim::arena_fixture::rival_arena(rules, 0x1269);
    sim.set_input_delay_ticks(0);
    sim.intern_rule_type_ids(rules);
    sim.resolve_type_handles(rules);
    let grid = sim
        .path_grid_snapshot()
        .map(|grid| (*grid).clone())
        .unwrap();
    (sim, grid)
}

fn spawn(sim: &mut Simulation, rules: &RuleSet, kind: &str, owner: &str, rx: u16, ry: u16) -> u64 {
    sim.spawn_object(kind, owner, rx, ry, 0, rules)
        .unwrap_or_else(|| panic!("{kind} spawns"))
}

fn entity(sim: &Simulation, id: u64) -> &GameEntity {
    sim.substrate.entities.get(id).expect("live object")
}

fn order(sim: &mut Simulation, house: &str, command: Command) {
    let owner = sim.interner.get(house).unwrap();
    let tick = sim.session.tick();
    sim.queue_command(CommandEnvelope::new(owner, tick + 1, command));
}

/// Runs the production frame until `done` holds, for at most `frames`
/// frames; answers whether it held.
fn run_until(
    sim: &mut Simulation,
    rules: &RuleSet,
    grid: &PathGrid,
    frames: u32,
    mut done: impl FnMut(&Simulation) -> bool,
) -> bool {
    for _ in 0..frames {
        let commands = sim.take_due_commands();
        sim.advance_tick(&commands, Some(rules), Some(grid), None, 33);
        if done(sim) {
            return true;
        }
    }
    false
}

/// Runs until the held tank is well up.
fn climb(sim: &mut Simulation, rules: &RuleSet, grid: &PathGrid, tank: u64) {
    assert!(
        run_until(sim, rules, grid, 200, |sim| {
            crate::sim::movement::air_movement::current_fly_height(
                entity(sim, tank),
                sim.resolved_terrain.as_ref(),
            ) > 200
        }),
        "the tank climbs"
    );
}

/// Orders the Magnetron away: its own non-null destination lets go
/// (`0x004D9509`).
fn move_away(sim: &mut Simulation, magnetron: u64) {
    order(
        sim,
        "Americans",
        Command::Move {
            entity_id: magnetron,
            target_rx: 4,
            target_ry: 16,
        },
    );
}

/// Lethal damage through the production receiver, as a weapon deals it.
fn kill(sim: &mut Simulation, rules: &RuleSet, victim: u64, attacker: u64) {
    let health = entity(sim, victim).health.current();
    let warhead = sim.interner.intern(&rules.bridge_warheads.crush_name);
    sim.commit_noncombat_aoe_receivers(
        rules,
        None,
        &[crate::sim::combat::combat_aoe::AreaDamageReceiver::Entity(
            crate::sim::combat::EntityDamageEvent::direct_receiver(
                victim,
                health,
                0,
                attacker,
                None,
                warhead,
                crate::sim::combat::ReceiverCallFlags {
                    ignore_defenses: true,
                    arg6: false,
                },
            ),
        )],
        FrameEffects::default(),
    );
}

/// A Magnetron ordered to attack a tank lifts it: no damage, a Jumpjet over
/// its Drive and both links (Imbue `0x00710000`).
fn lift(rules: &RuleSet) -> (Simulation, PathGrid, u64, u64) {
    let (mut sim, grid) = arena(rules);
    let magnetron = spawn(&mut sim, rules, "TELE", "Americans", 10, 16);
    let tank = spawn(&mut sim, rules, "MTNK", "Russians", 16, 16);
    let health = entity(&sim, tank).health.current();
    order(
        &mut sim,
        "Americans",
        Command::Attack {
            attacker_id: magnetron,
            target_id: tank,
        },
    );
    assert!(
        run_until(&mut sim, rules, &grid, 120, |sim| {
            entity(sim, tank).lift.swap_active()
        }),
        "the beam lifts the tank"
    );
    let held = entity(&sim, tank);
    assert_eq!(held.lift.source(), Some(magnetron));
    assert_eq!(entity(&sim, magnetron).lift.target(), Some(tank));
    assert_eq!(held.health.current(), health, "the beam deals no damage");
    let locomotor = held.locomotor.as_ref().unwrap();
    assert_eq!(locomotor.active_kind(), LocomotorKind::Jumpjet);
    assert!(locomotor.piggyback().is_some(), "the Drive is suspended");
    (sim, grid, magnetron, tank)
}

fn pull_sensor_ship_across_cell(
    runtime: &mut crate::sim::runtime::SimRuntime,
    magnetron: u64,
    ship: u64,
) -> (u16, u16) {
    let held = entity(&runtime.simulation, ship);
    let origin = (held.position.rx(), held.position.ry());
    let attack = CommandEnvelope::new(
        entity(&runtime.simulation, magnetron).owner(),
        runtime.simulation.session.tick() + 1,
        Command::Attack {
            attacker_id: magnetron,
            target_id: ship,
        },
    );
    for frame in 0..400 {
        runtime
            .advance_frame_for_tooling(
                if frame == 0 {
                    std::slice::from_ref(&attack)
                } else {
                    &[]
                },
                crate::headless_scenario::SIM_TICK_MS,
            )
            .expect("ordinary Magnetron/Destroyer frame");
        let sim = &runtime.simulation;
        let held = entity(sim, ship);
        let at = (held.position.rx(), held.position.ry());
        if held.lift.source() == Some(magnetron)
            && at != origin
            && crate::sim::movement::air_movement::is_high_flying(
                held,
                sim.resolved_terrain.as_ref(),
                Some((&runtime.resources.rules, &sim.interner)),
            )
        {
            let active = held.locomotor.as_ref().unwrap();
            assert_eq!(active.active_kind(), LocomotorKind::Jumpjet);
            assert_eq!(
                active.piggyback().as_ref().unwrap().active_kind(),
                LocomotorKind::Ship
            );
            return at;
        }
    }
    panic!(
        "the real beam must lift and pull the ship: {:?}",
        (
            entity(&runtime.simulation, ship).position,
            entity(&runtime.simulation, ship).lift
        )
    );
}

/// Imbue710000 preserves the sensor circle; Jumpjet54AEC0 has no airborne
/// PerCell. FootLimbo4DB376 removes at current GetCoords, not that old center.
#[test]
fn retail_lifted_destroyer_keeps_its_sensor_circle_until_real_lifecycle_callbacks() {
    let Some((retail, _)) = crate::rules::retail_ini_fixture::retail_assets() else {
        return;
    };
    let mut scene = crate::headless_scenario::load(&retail, "XMP03T4.MAP", 0x1269)
        .expect("physical AnyTown Battle scenario");
    let enemy = scene.add_human_house("Magnetron", crate::skirmish_launch::LaunchCountry::Yuri);
    let player = scene.sim().session.current_house().unwrap();
    let runtime = &mut scene.runtime;
    let rules = &runtime.resources.rules;
    let radius = u16::from(rules.object("DEST").unwrap().sensors_sight);
    assert_eq!(radius, 8);
    let enemy_name = runtime.simulation.interner.resolve(enemy).to_owned();
    let player_name = runtime.simulation.interner.resolve(player).to_owned();
    // Keep the extra human house alive under the normal ShortGame option.
    spawn(&mut runtime.simulation, rules, "PCV", &enemy_name, 88, 49);
    // Physical bank and water, within MagneticBeam's stock 3..12 range.
    // Spawn/Unlimbo supplies the circle; no lift, head or sensor state is set.
    let magnetron = spawn(&mut runtime.simulation, rules, "TELE", &enemy_name, 87, 50);
    let ship = spawn(&mut runtime.simulation, rules, "DEST", &player_name, 89, 54);
    let origin = (89, 54);
    let at = pull_sensor_ship_across_cell(runtime, magnetron, ship);
    let sim = &runtime.simulation;
    let current_circle = sim.fog.sensor_circle_cells(at, radius);
    let old_only = sim
        .fog
        .sensor_circle_cells(origin, radius)
        .into_iter()
        .find(|cell| !current_circle.contains(cell))
        .unwrap();
    assert!(
        sim.fog.has_sensor_for_house(player, old_only.0, old_only.1),
        "airborne cell crossing does not move the native sensor circle"
    );
    assert!(sim.fog.has_sensor_for_house(player, origin.0, origin.1));
    let counters = sim.fog.sensors_by_house().clone();
    // Unit7463AE refuses before Foot's same-owner remove/add. A lifted
    // source makes that distinction visible: current and deposited differ.
    runtime.simulation.change_owner_with_rules(
        ship,
        player,
        &runtime.resources.rules,
        None,
        FrameEffects::default(),
    );
    assert_eq!(runtime.simulation.fog.sensors_by_house(), &counters);
    // Unit's actual airborne damage/UnInit owns the current-coordinate remove.
    // Old-only coverage survives it natively; overlapping coverage is removed.
    kill(
        &mut runtime.simulation,
        &runtime.resources.rules,
        ship,
        magnetron,
    );
    assert!(
        runtime
            .simulation
            .fog
            .has_sensor_for_house(player, old_only.0, old_only.1),
        "native Limbo leaves the old-only sensor fringe stamped"
    );
    assert!(
        !runtime
            .simulation
            .fog
            .has_sensor_for_house(player, origin.0, origin.1),
        "Limbo removes the current-coordinate overlap"
    );
    let saved =
        crate::sim::snapshot::GameSnapshot::save(&runtime.simulation, 0, 0, "sensor fringe", 0);
    let restored = crate::sim::snapshot::GameSnapshot::load(&saved)
        .unwrap()
        .sim;
    assert_eq!(
        restored.fog.sensors_by_house(),
        runtime.simulation.fog.sensors_by_house(),
        "residual coverage is saved independently of a surviving sensor source"
    );
}

/// Fatal Techno702112 -> CaptureFreeAll472140 -> FreeUnit471FF0 restores
/// a previously captured ship while airborne; Foot4DBED0 uses its current cell.
#[test]
fn retail_controller_death_restores_a_lifted_ships_current_sensor_circle() {
    let Some((retail, _)) = crate::rules::retail_ini_fixture::retail_assets() else {
        return;
    };
    let mut scene = crate::headless_scenario::load(&retail, "XMP03T4.MAP", 0x1269)
        .expect("physical AnyTown Battle scenario");
    let captor = scene.add_human_house("Controller", crate::skirmish_launch::LaunchCountry::Yuri);
    let enemy = scene.add_human_house("Magnetron", crate::skirmish_launch::LaunchCountry::Yuri);
    let player = scene.sim().session.current_house().unwrap();
    let runtime = &mut scene.runtime;
    let captor_name = runtime.simulation.interner.resolve(captor).to_owned();
    let enemy_name = runtime.simulation.interner.resolve(enemy).to_owned();
    let player_name = runtime.simulation.interner.resolve(player).to_owned();
    let controller = spawn(
        &mut runtime.simulation,
        &runtime.resources.rules,
        "YURI",
        &captor_name,
        87,
        50,
    );
    // Pull along the real water strip90,54 ->89,54;88,54 has a bridge.
    let ship = spawn(
        &mut runtime.simulation,
        &runtime.resources.rules,
        "DEST",
        &player_name,
        90,
        54,
    );
    // Both added houses need a real BaseUnit under normal ShortGame rules.
    spawn_retail_lift_participant(runtime, "PCV", captor, (87, 50), 12, 16);
    spawn_retail_lift_participant(runtime, "PCV", enemy, (87, 50), 18, 24);
    let capture = CommandEnvelope::new(
        captor,
        runtime.simulation.session.tick() + 1,
        Command::Attack {
            attacker_id: controller,
            target_id: ship,
        },
    );
    let mut captured = false;
    for frame in 0..300 {
        runtime
            .advance_frame_for_tooling(
                if frame == 0 {
                    std::slice::from_ref(&capture)
                } else {
                    &[]
                },
                crate::headless_scenario::SIM_TICK_MS,
            )
            .expect("ordinary Yuri capture frame");
        if entity(&runtime.simulation, ship).owner() == captor {
            captured = true;
            break;
        }
    }
    assert!(captured, "the ground ship is captured through MindControl");
    // Foot ChangeOwner4DBF81 re-adds for the captor at the current cell.
    let origin = {
        let captured = entity(&runtime.simulation, ship);
        (captured.position.rx(), captured.position.ry())
    };
    assert!(
        runtime
            .simulation
            .fog
            .has_sensor_for_house(captor, origin.0, origin.1)
    );
    let magnetron = spawn(
        &mut runtime.simulation,
        &runtime.resources.rules,
        "TELE",
        &enemy_name,
        81,
        54,
    );
    let at = pull_sensor_ship_across_cell(runtime, magnetron, ship);
    let held = entity(&runtime.simulation, ship);
    assert_eq!(held.owner(), captor);
    assert_eq!(
        runtime
            .simulation
            .resolved_terrain
            .as_ref()
            .unwrap()
            .cell(at.0, at.1)
            .unwrap()
            .land_type,
        crate::rules::terrain_rules::LandType::Water as u8,
        "the nested NULL setter needs an admissible water stop"
    );
    let radius = u16::from(
        runtime
            .resources
            .rules
            .object("DEST")
            .unwrap()
            .sensors_sight,
    );
    let current_circle = runtime.simulation.fog.sensor_circle_cells(at, radius);
    let old_only = runtime
        .simulation
        .fog
        .sensor_circle_cells(origin, radius)
        .into_iter()
        .find(|cell| !current_circle.contains(cell))
        .unwrap();
    // No direct owner/lift writes: lethal damage frees the real capture node.
    kill(
        &mut runtime.simulation,
        &runtime.resources.rules,
        controller,
        magnetron,
    );
    let sim = &runtime.simulation;
    let freed = entity(sim, ship);
    assert!(freed.lifecycle.object_alive());
    assert_eq!(freed.owner(), player);
    assert_eq!((freed.position.rx(), freed.position.ry()), at);
    assert_eq!(
        freed.navigation.neighbor_state().cell(),
        (origin.0 as i16, origin.1 as i16),
        "high-flight owner change preserves the distinct PerCell source"
    );
    assert!(sim.fog.has_sensor_for_house(captor, old_only.0, old_only.1));
    assert!(!sim.fog.has_sensor_for_house(captor, origin.0, origin.1));
    assert!(
        sim.fog.has_sensor_for_house(player, at.0, at.1),
        "restoration adds around the current cell after the owner flip"
    );
    assert!(!sim.fog.has_sensor_for_house(player, old_only.0, old_only.1));
    assert!(
        freed.lift.source().is_none(),
        "the nested native NULL setter releases the hold"
    );
    let old_circle = sim.fog.sensor_circle_cells(origin, radius);
    let mut landed = false;
    for _ in 0..400 {
        runtime
            .advance_frame_for_tooling(&[], crate::headless_scenario::SIM_TICK_MS)
            .expect("released Destroyer descent and real touchdown");
        let sim = &runtime.simulation;
        let ship = entity(sim, ship);
        if !ship.lift.swap_active()
            && ship.locomotor.as_ref().unwrap().active_kind() == LocomotorKind::Ship
        {
            // Touchdown's PerCell adds at the landing cell.
            let center = (ship.position.rx(), ship.position.ry());
            assert_eq!(
                ship.navigation.neighbor_state().cell(),
                (center.0 as i16, center.1 as i16)
            );
            let witness = sim
                .fog
                .sensor_circle_cells(center, radius)
                .into_iter()
                .find(|cell| current_circle.contains(cell) && !old_circle.contains(cell))
                .expect("landing and restoration overlap beyond the old radius");
            let index =
                usize::from(witness.1) * usize::from(sim.fog.width()) + usize::from(witness.0);
            assert_eq!(
                sim.fog.sensors_by_house()[&player][index],
                2,
                "real touchdown removes at +55C=A, retaining the earlier B-only add"
            );
            assert!(sim.fog.has_sensor_for_house(captor, old_only.0, old_only.1));
            landed = true;
            break;
        }
    }
    assert!(
        landed,
        "the real landing callback restores Ship after owner restoration"
    );
}

/// Imbue710326/710352 supplies both NavCom and Foot+6AD. UnitAI7365E8
/// still runs Facing_Update, whose736BB1 gate preserves an idle turret's
/// aim until the lift ends. The native idle_navcom_magnetron control in
/// turret_cadence.json pins that gate; this checks its production callers.
#[test]
fn retail_magnetron_lift_keeps_the_victims_idle_turret_aim() {
    let Some(rules) = retail_rules() else {
        return;
    };
    let (mut sim, grid) = arena(&rules);
    // Directional diplomacy stops the victim acquiring the Magnetron before
    // the beam lands, leaving a distinct idle aim for the lift to preserve.
    sim.house_alliances_mut_for_test()
        .entry("RUSSIANS".into())
        .or_default()
        .insert("AMERICANS".into());
    let magnetron = spawn(&mut sim, &rules, "TELE", "Americans", 10, 16);
    let tank = spawn(&mut sim, &rules, "MTNK", "Russians", 16, 16);
    let heading = entity(&sim, tank)
        .barrel_facing
        .as_ref()
        .unwrap()
        .destination();
    order(
        &mut sim,
        "Americans",
        Command::Attack {
            attacker_id: magnetron,
            target_id: tank,
        },
    );
    assert!(run_until(&mut sim, &rules, &grid, 120, |sim| {
        entity(sim, tank).lift.swap_active()
    }));
    let mut checked = 0;
    for _ in 0..120 {
        let held = entity(&sim, tank);
        if !held.lift.swap_active() || held.navigation.nav_com().is_none() {
            break;
        }
        assert!(held.attack_target.target().is_none());
        assert_eq!(held.barrel_facing.as_ref().unwrap().destination(), heading);
        checked += 1;
        let commands = sim.take_due_commands();
        sim.advance_tick(&commands, Some(&rules), Some(&grid), None, 33);
    }
    assert!(checked > 60, "the held tank passes the idle turret dwell");
}

/// Held all the way, the tank reaches the cell beside the Magnetron that the
/// Jumpjet's `Move_To` found free (`0x0054B275..0x0054B2D9`; the Magnetron's
/// own cell is taken), is let go there by State 3's arrival (`0x0054C1CB`)
/// and lands on that clear cell unharmed.
#[test]
fn retail_magnetron_pulls_a_tank_beside_itself_and_lets_go() {
    let Some(rules) = retail_rules() else {
        return;
    };
    let (mut sim, grid, magnetron, tank) = lift(&rules);
    let health = entity(&sim, tank).health.current();
    let americans = sim.interner.get("Americans").unwrap();

    assert!(
        run_until(&mut sim, &rules, &grid, 600, |sim| {
            entity(sim, tank).drop_credit.being_manipulated()
        }),
        "the tank arrives and is let go"
    );
    let dropped = entity(&sim, tank);
    assert!(dropped.crash.is_crashing());
    assert_eq!(dropped.drop_credit.manipulated_by(), Some(magnetron));
    assert_eq!(dropped.drop_credit.warped_by_house(), Some(americans));
    assert_eq!(dropped.lift.source(), None);
    let holder = entity(&sim, magnetron);
    assert_eq!(holder.lift.target(), None);
    assert!(
        holder.attack_target.target().is_none(),
        "the release clears its target"
    );
    let offset = (
        i32::from(dropped.position.rx()) - i32::from(holder.position.rx()),
        i32::from(dropped.position.ry()) - i32::from(holder.position.ry()),
    );
    assert_eq!(
        offset.0.abs().max(offset.1.abs()),
        1,
        "beside the Magnetron: {offset:?}"
    );

    assert!(
        run_until(&mut sim, &rules, &grid, 400, |sim| {
            !entity(sim, tank).lift.swap_active()
        }),
        "the tank lands and its lift ends"
    );
    assert_eq!(entity(&sim, tank).health.current(), health);
}

/// A stock aircraft first spends its ammunition and returns to an actual
/// airfield through its ordinary missions. The Magnetron then attacks that
/// landed aircraft: Detonate46967E..46969C admits Aircraft as well as Unit,
/// and Imbue710326 calls Aircraft41AA80 -> Foot4D94B0 -> the installed
/// Jumpjet's Move_To. The save/resume and continued clear-cell landing are
/// Rust composition regressions; the native oracle covers bounded calls,
/// not the entire aircraft flight or the snapshot format.
#[test]
#[ignore = "physical retail map, rules and art (RA2_DIR)"]
fn retail_magnetron_lifts_a_docked_aircraft_through_its_attack_order() {
    use crate::rules::locomotor_type::{MovementZone, SpeedType};
    use crate::sim::cell_rect::{CellPassabilityArgs, canonical_cell_ref, check_cell_passability};
    use crate::sim::components::{DriveCoord, NavTargetRef};
    use crate::sim::movement::air_movement;

    let retail = std::path::PathBuf::from(std::env::var_os("RA2_DIR").expect("RA2_DIR"));
    for aircraft_type in ["ORCA", "BEAG"] {
        let crate::sim::arena_fixture::RetailHarrierDockScene {
            mut runtime,
            player,
            airfield,
            harriers,
            target: rhino,
        } = crate::sim::arena_fixture::retail_harrier_dock_scene(&retail)
            .expect("retail airfield scene");
        // Headless loading retains only the live map, not the app's separate
        // terrain_template. Keep its production-loaded terrain before command
        // frames; saved overlay/bridge authority is reapplied during restore.
        let restore_terrain = runtime
            .simulation
            .resolved_terrain
            .as_ref()
            .expect("loaded retail terrain")
            .clone();
        let aircraft = if aircraft_type == "ORCA" {
            harriers[0]
        } else {
            let initial = entity(&runtime.simulation, harriers[0]);
            let cell = (initial.position.rx(), initial.position.ry());
            spawn_retail_lift_participant(&mut runtime, aircraft_type, player, cell, 1, 4)
        };
        let strike = CommandEnvelope::new(
            player,
            runtime.simulation.session.tick() + 1,
            Command::Attack {
                attacker_id: aircraft,
                target_id: rhino,
            },
        );
        let mut fired = false;
        let mut entered = false;
        let mut guarding_frames = 0;
        for frame in 0..9000 {
            let commands = if frame == 0 {
                std::slice::from_ref(&strike)
            } else {
                &[]
            };
            runtime
                .advance_frame_for_tooling(commands, crate::headless_scenario::SIM_TICK_MS)
                .expect("aircraft strike/return frame");
            let sim = &runtime.simulation;
            let plane = entity(sim, aircraft);
            let ammo = plane.ammo().expect("stock aircraft ammunition");
            fired |= ammo.current() < ammo.max();
            entered |= fired && plane.mission.current().known() == Some(MissionType::Enter);
            let on_pad = plane.radio_contacts.contains(airfield)
                && entity(sim, airfield).radio_contacts.contains(aircraft)
                && air_movement::current_fly_height(plane, sim.resolved_terrain.as_ref()) == 0;
            if entered
                && on_pad
                && ammo.current() == ammo.max()
                && plane.mission.current().known() == Some(MissionType::Guard)
                && crate::sim::movement::motion_query::is_moving(plane) == Some(false)
            {
                guarding_frames += 1;
                if guarding_frames == 30 {
                    break;
                }
            } else {
                guarding_frames = 0;
            }
        }
        assert!(
            fired && entered,
            "{aircraft_type} fires and returns through Enter"
        );
        assert_eq!(
            guarding_frames, 30,
            "{aircraft_type} reloads and guards on its pad"
        );
        let plane = entity(&runtime.simulation, aircraft);
        assert_eq!(
            plane.locomotor.as_ref().unwrap().active_kind(),
            LocomotorKind::Fly
        );
        assert!(!plane.lift.swap_active());
        let health = plane.health.current();
        let pad_cell = (plane.position.rx(), plane.position.ry());
        let pad_coordinate =
            crate::sim::movement::ground_pose::position_world_coord(&plane.position);
        let enemy = entity(&runtime.simulation, rhino).owner();
        let magnetron = spawn_retail_lift_participant(&mut runtime, "TELE", enemy, pad_cell, 6, 8);
        let attack = CommandEnvelope::new(
            enemy,
            runtime.simulation.session.tick() + 1,
            Command::Attack {
                attacker_id: magnetron,
                target_id: aircraft,
            },
        );
        let mut lifted = false;
        for frame in 0..240 {
            let commands = if frame == 0 {
                std::slice::from_ref(&attack)
            } else {
                &[]
            };
            runtime
                .advance_frame_for_tooling(commands, crate::headless_scenario::SIM_TICK_MS)
                .expect("Magnetron attack frame");
            if entity(&runtime.simulation, aircraft).lift.swap_active() {
                lifted = true;
                break;
            }
        }
        let sim = &runtime.simulation;
        assert!(
            lifted,
            "the ordinary TELE attack lifts docked {aircraft_type}: attacker mission {:?}, target {:?}, nav {:?}; aircraft mission {:?}, health {}",
            entity(sim, magnetron).mission.current(),
            entity(sim, magnetron).attack_target.target(),
            entity(sim, magnetron).navigation.nav_com(),
            entity(sim, aircraft).mission.current(),
            entity(sim, aircraft).health.current(),
        );
        let plane = entity(sim, aircraft);
        assert_eq!(
            plane.health.current(),
            health,
            "{aircraft_type}: the beam does no damage"
        );
        assert_eq!(
            plane.lift.source(),
            Some(magnetron),
            "{aircraft_type}: victim link"
        );
        assert_eq!(
            entity(sim, magnetron).lift.target(),
            Some(aircraft),
            "{aircraft_type}: firer link"
        );
        let active = plane.locomotor.as_ref().unwrap();
        assert_eq!(
            active.active_kind(),
            LocomotorKind::Jumpjet,
            "{aircraft_type}: installed locomotor"
        );
        assert_eq!(
            active
                .piggyback()
                .as_ref()
                .expect("suspended aircraft locomotor")
                .active_kind(),
            LocomotorKind::Fly,
            "{aircraft_type}: the original Fly is suspended"
        );
        let jumpjet = active.jumpjet_runtime().expect("installed Jumpjet");
        assert!(
            jumpjet.moving(),
            "{aircraft_type}: Move_To reached the installed Jumpjet"
        );
        assert_ne!(jumpjet.destination(), DriveCoord { x: 0, y: 0, z: 0 });
        // Aircraft/Foot first receives the firer object; the installed
        // Jumpjet's Move_To54B43D..54B44B then publishes its chosen Cell.
        let destination = jumpjet.destination();
        assert_eq!(
            plane.navigation.nav_com(),
            Some(NavTargetRef::cell(
                (destination.x / 256) as u16,
                (destination.y / 256) as u16,
            )),
            "{aircraft_type}: NavCom follows the Jumpjet's chosen destination cell"
        );
        let terrain = sim.resolved_terrain.as_ref().unwrap();
        assert!(
            check_cell_passability(
                &canonical_cell_ref(
                    terrain,
                    terrain.native_cell_identity((
                        (destination.x / 256) as i16,
                        (destination.y / 256) as i16,
                    )),
                ),
                CellPassabilityArgs {
                    speed_type: SpeedType::Wheel,
                    ignore_infantry: true,
                    ignore_vehicles: true,
                    zone: None,
                    movement_zone: MovementZone::Normal,
                    level: None,
                    is_bridge: true,
                },
                &sim.cell_passability_map(),
            )
            .admits(),
            "{aircraft_type}: chosen landing cell meets Aircraft State5's Wheel gate"
        );
        assert!(
            !plane.radio_contacts.contains(airfield),
            "{aircraft_type}: aircraft leaves pad contact"
        );
        assert!(
            !entity(sim, airfield).radio_contacts.contains(aircraft),
            "{aircraft_type}: airfield releases its dock"
        );

        let mut moved = false;
        for _ in 0..240 {
            runtime
                .advance_frame_for_tooling(&[], crate::headless_scenario::SIM_TICK_MS)
                .expect("lifted aircraft movement frame");
            let plane = entity(&runtime.simulation, aircraft);
            let at = crate::sim::movement::ground_pose::position_world_coord(&plane.position);
            if plane.lift.source() == Some(magnetron)
                && at.z > pad_coordinate.z
                && (at.x, at.y) != (pad_coordinate.x, pad_coordinate.y)
            {
                moved = true;
                break;
            }
        }
        assert!(
            moved,
            "{aircraft_type}: ordinary frames lift and pull the aircraft off its pad"
        );

        // Preserve the entire installed Jumpjet and its boxed Fly, not just
        // their class tags. A Scenario load restarts its RNG from Seed0, so
        // compare the retained gameplay state rather than the world hash.
        let held_state = |sim: &Simulation| {
            let plane = entity(sim, aircraft);
            (
                plane.locomotor.clone(),
                plane.lift,
                plane.drop_credit,
                plane.navigation.nav_com(),
                plane.radio_contacts.clone(),
                crate::sim::movement::ground_pose::position_world_coord(&plane.position),
            )
        };
        let saved_state = held_state(&runtime.simulation);
        let saved_holder_lift = entity(&runtime.simulation, magnetron).lift;
        let saved_pad_contacts = entity(&runtime.simulation, airfield).radio_contacts.clone();
        let saved =
            crate::sim::snapshot::GameSnapshot::save(&runtime.simulation, 0, 0, "held aircraft", 0);
        let mut restored = crate::sim::snapshot::GameSnapshot::load(&saved)
            .expect("held aircraft snapshot")
            .sim;
        restored.retain_process_state_for_load(crate::sim::world::SnapshotProcessState::Live(
            &runtime.simulation,
        ));
        restored.bind_shared_cell_dummy(crate::map::resolved_terrain::SharedCellDummy::fresh());
        restored.reconstruct_cellclass_dummy_for_map_resize();
        restored
            .restore_after_snapshot_load()
            .expect("restore held aircraft owners");
        crate::sim::production::validate_restored_factory_state(
            &restored,
            &runtime.resources.rules,
        )
        .expect("restore production identities");
        restored.rebuild_caches_after_load(
            restore_terrain,
            runtime.simulation.terrain_speed_config().clone(),
            &runtime.resources.rules,
        );
        restored
            .restore_map_authority_after_snapshot_load(
                &runtime.resources.rules,
                &runtime.resources.overlay_registry,
                None,
            )
            .expect("restore retail map authority");
        restored.intern_rule_type_ids(&runtime.resources.rules);
        restored.resolve_type_handles(&runtime.resources.rules);
        restored.rebuild_lighting_sources_after_load(&runtime.resources.rules);
        // Commit the same process identities as PreparedLoad: retained tile
        // catalogs may share Main and the live fallback Cell with the map.
        let live_dummy = runtime.simulation.effective_shared_cell_dummy();
        live_dummy.adopt_prepared_load_state(&restored.effective_shared_cell_dummy());
        restored.bind_shared_cell_dummy(live_dummy);
        restored.install_fresh_process_main(runtime.simulation.main_rng());
        restored.publish_scenario_started();
        runtime.replace_simulation(restored);
        assert_eq!(
            held_state(&runtime.simulation),
            saved_state,
            "{aircraft_type}: save/load retains the complete held aircraft state"
        );
        assert_eq!(
            entity(&runtime.simulation, magnetron).lift,
            saved_holder_lift
        );
        assert_eq!(
            entity(&runtime.simulation, airfield).radio_contacts,
            saved_pad_contacts
        );
        eprintln!(
            "{aircraft_type}: resumed held aircraft at frame {}, position {:?}",
            runtime.simulation.session.tick(),
            saved_state.5
        );

        // Leave the attack running: the installed Jumpjet reaches the free
        // cell chosen beside the Magnetron and its own State3 arrival lets
        // go. No test setter supplies release, touchdown, or End_Piggyback.
        let mut released = false;
        let mut restored_fly = false;
        for _ in 0..600 {
            runtime
                .advance_frame_for_tooling(&[], crate::headless_scenario::SIM_TICK_MS)
                .expect("resumed aircraft pull/release frame");
            let sim = &runtime.simulation;
            let plane = sim.substrate.entities.get(aircraft).unwrap_or_else(|| {
                panic!("{aircraft_type}: clear-cell pull must not destroy the aircraft")
            });
            if !released && plane.lift.source().is_none() {
                released = true;
                assert_eq!(entity(sim, magnetron).lift.target(), None);
                assert!(plane.drop_credit.being_manipulated());
                assert_eq!(plane.drop_credit.manipulated_by(), Some(magnetron));
                assert_eq!(plane.drop_credit.warped_by_house(), Some(enemy));
                let active = plane.locomotor.as_ref().unwrap();
                assert_eq!(active.active_kind(), LocomotorKind::Jumpjet);
                assert!(active.jumpjet_runtime().unwrap().moving());
                assert_eq!(
                    active.piggyback().as_ref().unwrap().active_kind(),
                    LocomotorKind::Fly,
                    "{aircraft_type}: release alone does not restore Fly"
                );
                eprintln!(
                    "{aircraft_type}: released at frame {}, position {:?}",
                    sim.session.tick(),
                    crate::sim::movement::ground_pose::position_world_coord(&plane.position)
                );
            }
            if released && !plane.lift.swap_active() {
                restored_fly = true;
                break;
            }
        }
        assert!(
            released,
            "{aircraft_type}: natural arrival releases the lift"
        );
        assert!(
            restored_fly,
            "{aircraft_type}: resumed ordinary frames settle and restore Fly; state {:?}",
            held_state(&runtime.simulation)
        );
        let plane = entity(&runtime.simulation, aircraft);
        assert_eq!(
            plane.health.current(),
            health,
            "{aircraft_type}: clear landing is harmless"
        );
        let active = plane.locomotor.as_ref().unwrap();
        assert_eq!(active.active_kind(), LocomotorKind::Fly);
        assert!(active.piggyback().is_none());
        assert_eq!(
            air_movement::current_fly_height(plane, runtime.simulation.resolved_terrain.as_ref()),
            0,
            "{aircraft_type}: Fly is restored after touchdown"
        );
    }
}

/// Choose a real admitted map cell in the requested distance band. Placement
/// supplies only the initial scene; subsequent orders, docking and lift state
/// are produced by the normal runtime.
fn spawn_retail_lift_participant(
    runtime: &mut crate::sim::runtime::SimRuntime,
    kind: &str,
    owner: crate::sim::intern::InternedId,
    around: (u16, u16),
    minimum: i32,
    maximum: i32,
) -> u64 {
    let distance = |(x, y): (u16, u16)| {
        (i32::from(x) - i32::from(around.0)).pow(2) + (i32::from(y) - i32::from(around.1)).pow(2)
    };
    let mut cells: Vec<_> = runtime
        .simulation
        .resolved_terrain
        .iter()
        .flat_map(|terrain| terrain.iter().map(|cell| (cell.rx, cell.ry)))
        .filter(|&cell| (minimum * minimum..=maximum * maximum).contains(&distance(cell)))
        .collect();
    cells.sort_by_key(|&cell| (distance(cell), cell));
    let owner = runtime.simulation.interner.resolve(owner).to_owned();
    for (rx, ry) in cells {
        if let Some(id) =
            runtime
                .simulation
                .spawn_object(kind, &owner, rx, ry, 0, &runtime.resources.rules)
        {
            return id;
        }
    }
    panic!("no admitted retail cell for {kind} near {around:?}");
}

/// Let go over another object, the tank is centred on that cell and falls
/// onto it: a Unit below crashes it (`0x0054CC47`), and its falling damage
/// (`0x00746209`) destroys it and hurts the Unit below. Its death counts
/// twice for the Magnetron's house: once where a bullet kills it, once where
/// the falling damage books the drop's credit (`+0x428`, `0x007462F4`);
/// Record_The_Kill (`0x00702D40`) has no first-credit guard.
#[test]
fn retail_tank_let_go_over_a_unit_crashes_onto_it() {
    let Some(rules) = retail_rules() else {
        return;
    };
    let (mut sim, grid, magnetron, tank) = lift(&rules);
    let below = spawn(&mut sim, &rules, "AMCV", "Americans", 13, 16);
    let below_health = entity(&sim, below).health.current();
    let americans = sim.interner.get("Americans").unwrap();
    let kills = sim.houses[&americans].stats().units_killed();

    assert!(
        run_until(&mut sim, &rules, &grid, 600, |sim| {
            let held = entity(sim, tank);
            (held.position.rx(), held.position.ry()) == (13, 16)
        }),
        "the tank is pulled over the Unit"
    );
    assert!(rules.combat_damage.current_strength_damage);
    assert_eq!(rules.combat_damage.falling_damage_multiplier, 1.0);
    let falling_damage = entity(&sim, tank).health.current();
    let death_weapon = rules
        .weapon(rules.combat_damage.death_weapon.as_deref().unwrap())
        .unwrap();
    assert!(
        !death_weapon.bright,
        "the drop overrides the stock weapon's dark default"
    );
    sim.clear_lifecycle_test_events_for_test();
    move_away(&mut sim, magnetron);
    let mut impact_lights = None;
    for _ in 0..400 {
        let commands = sim.take_due_commands();
        let frame = sim
            .advance_app_frame(
                &commands,
                &rules,
                None,
                33,
                crate::sim::world::TickLane::Ordinary,
                FrameEffects::default(),
            )
            .expect("admitted drop frame");
        if !sim.substrate.entities.contains(tank) {
            impact_lights = Some(frame.combat_lights);
            break;
        }
    }
    let impact_lights = impact_lights.expect("the tank crashes onto the Unit");
    assert!(
        entity(&sim, below).health.current() < below_health,
        "the fall hurts the Unit below"
    );
    assert_eq!(
        sim.houses[&americans].stats().units_killed(),
        kills + 2,
        "the bullet and the drop's credit each book the kill"
    );
    // Unit746275/74631F pass Bright1 independently of the installed weapon
    // (which remains null). Detonate469BD6..469C41 emits forced lights at
    // the existing Inviso-scattered impact coordinates, in Crush/C4 order.
    let falling_lights: Vec<_> = impact_lights
        .iter()
        .filter_map(|light| match light {
            crate::sim::combat::CombatLightRequest::Impact {
                target_id,
                damage,
                warhead_ref,
                coord,
                force_create,
                flags,
            } if [
                rules.bridge_warheads.crush_name.as_str(),
                rules.bridge_warheads.c4_name.as_str(),
            ]
            .contains(&sim.interner.resolve(*warhead_ref)) =>
            {
                Some((
                    *target_id,
                    *damage,
                    sim.interner.resolve(*warhead_ref),
                    *coord,
                    *force_create,
                    *flags,
                ))
            }
            _ => None,
        })
        .collect();
    assert_eq!(
        falling_lights.len(),
        2,
        "both direct falling bullets force a combat flash"
    );
    // State5_Crash54CAFE..54CBC2 retains the last downward step before
    // testing GetHeight<=0; this scene crosses the floor at Z=-1. The
    // first Unit7462B5 impact subtracts GetHeight (so Z=0), whereas the
    // second74631A uses retained GetCoords. Inviso49F420 scatters only XY.
    for (light, (warhead, impact_z)) in falling_lights.iter().zip([
        (rules.bridge_warheads.crush_name.as_str(), 0),
        (rules.bridge_warheads.c4_name.as_str(), -1),
    ]) {
        assert_eq!(light.0, None);
        assert_eq!(light.1, falling_damage);
        assert_eq!(light.2, warhead);
        assert!((light.3.x - (13 * 256 + 128)).abs() <= 32);
        assert!((light.3.y - (16 * 256 + 128)).abs() <= 32);
        assert_eq!(light.3.z, impact_z);
        assert!(light.4);
        assert_eq!(light.5, 0, "stock Crush/Super disable no light channels");
    }
    assert_drop_bullet_lifetimes(&sim, &rules, tank, magnetron, falling_damage);
    assert!(sim.radiation.is_empty());
}

fn assert_drop_bullet_lifetimes(
    sim: &Simulation,
    rules: &RuleSet,
    tank: u64,
    source: u64,
    damage: i32,
) {
    use crate::sim::projectile::{ProjectileCoord, ProjectileTarget};
    use crate::sim::world::LifecycleTestEvent;
    let events = sim.lifecycle_test_events_for_test();
    let births: Vec<_> = events
        .iter()
        .enumerate()
        .filter_map(|(index, event)| {
            if let LifecycleTestEvent::UnfiredProjectileConstructed {
                stable_id,
                native_id,
                source_id,
                target,
                payload,
                position,
                in_logic,
                in_display,
            } = event
            {
                if *target == ProjectileTarget::Entity(tank) {
                    return Some((
                        index,
                        *stable_id,
                        *native_id,
                        *source_id,
                        *payload,
                        *position,
                        *in_logic,
                        *in_display,
                    ));
                }
            }
            None
        })
        .collect();
    assert_eq!(
        births.len(),
        2,
        "each falling damage call constructs its own Bullet"
    );
    for (birth, warhead) in births.iter().zip([
        &rules.bridge_warheads.crush_name,
        &rules.bridge_warheads.c4_name,
    ]) {
        assert_eq!(birth.3, source);
        assert_eq!(birth.4.base_damage, damage);
        assert_eq!(sim.interner.resolve(birth.4.warhead), warhead);
        assert_eq!(
            sim.interner.resolve(birth.4.projectile_type().unwrap()),
            "Invisible"
        );
        assert!(birth.4.bright());
        assert_eq!(birth.4.installed_weapon(), None);
        assert_eq!(birth.5, ProjectileCoord::new(0, 0, 0));
        assert!(
            !birth.6 && !birth.7,
            "constructor does not Fire or enter Logic/Display"
        );
        assert_ne!(birth.1, tank);
        assert!(sim.projectiles.get(birth.1).is_none());
        assert!(!sim.substrate.logic.as_slice().contains(&birth.1));
        let destroyed = events
            .iter()
            .position(|event| {
                matches!(event,
            LifecycleTestEvent::FinalizedCommon { stable_id } if *stable_id == birth.1)
            })
            .unwrap();
        assert!(birth.0 < destroyed);
        if birth.1 == births[0].1 {
            assert!(
                destroyed < births[1].0,
                "first scalar destruction precedes second constructor"
            );
        }
    }
    assert!(
        births[0].2 < births[1].2,
        "native IDs are spent in actual construction order"
    );
    assert!(events.iter().any(|event| matches!(event,
        LifecycleTestEvent::ProjectilePointerExpiredVisited { expired_id, projectile_id, source_id, target, .. }
        if *expired_id == tank && *projectile_id == births[0].1
            && *source_id == if source == tank { crate::sim::combat::RAD_NO_ATTACKER } else { source }
            && target.object_id() != Some(tank)
    )), "the still-represented first Bullet receives its target's fatal expiry");
}

#[test]
fn retail_drop_after_magnetron_expiry_uses_and_clears_the_falling_source() {
    let Some(rules) = retail_rules() else {
        return;
    };
    let (mut sim, grid, magnetron, tank) = lift(&rules);
    let below = spawn(&mut sim, &rules, "AMCV", "Americans", 13, 16);
    let before = entity(&sim, below).health.current();
    assert!(run_until(&mut sim, &rules, &grid, 600, |sim| {
        let held = entity(sim, tank);
        (held.position.rx(), held.position.ry()) == (13, 16)
    }));
    let damage = entity(&sim, tank).health.current();
    sim.clear_lifecycle_test_events_for_test();
    kill(&mut sim, &rules, magnetron, tank);
    assert_eq!(entity(&sim, tank).drop_credit.manipulated_by(), None);
    assert!(run_until(&mut sim, &rules, &grid, 400, |sim| !sim
        .substrate
        .entities
        .contains(tank)));
    assert!(entity(&sim, below).health.current() < before);
    assert_drop_bullet_lifetimes(&sim, &rules, tank, tank, damage);
    assert!(sim.radiation.is_empty());
}

/// Let go mid-flight, by a move order (the firer's own destination,
/// `0x004D9509`) or by the Magnetron's death (the receiver's UnInit, whose
/// Foot body lets go, `0x004DE5F0`), the tank is centred on its cell, falls,
/// lands on the clear ground and, once down, gets its Drive back unharmed
/// (Jumpjet END `0x0054DA50`).
#[test]
fn retail_tank_let_go_over_clear_ground_lands_and_drives_again() {
    let Some(rules) = retail_rules() else {
        return;
    };
    for death in [false, true] {
        let (mut sim, grid, magnetron, tank) = lift(&rules);
        let health = entity(&sim, tank).health.current();
        climb(&mut sim, &rules, &grid, tank);
        if death {
            kill(&mut sim, &rules, magnetron, tank);
            assert!(!entity(&sim, magnetron).lifecycle.object_alive());
        } else {
            move_away(&mut sim, magnetron);
            assert!(
                run_until(&mut sim, &rules, &grid, 5, |sim| {
                    entity(sim, magnetron).lift.target().is_none()
                }),
                "the move order lets the tank go"
            );
        }
        let dropped = entity(&sim, tank);
        assert!(dropped.drop_credit.being_manipulated(), "death {death}");
        assert_eq!(dropped.lift.source(), None, "death {death}");
        assert_eq!(
            dropped.drop_credit.manipulated_by(),
            (!death).then_some(magnetron),
            "an expiring Magnetron leaves no credit (death {death})"
        );

        assert!(
            run_until(&mut sim, &rules, &grid, 400, |sim| {
                !entity(sim, tank).lift.swap_active()
            }),
            "the tank lands and its lift ends (death {death})"
        );
        let landed = entity(&sim, tank);
        assert_eq!(landed.health.current(), health, "death {death}");
        assert!(!landed.crash.is_crashing() && !landed.drop_credit.being_manipulated());
        assert!(!landed.lift.may_end());
        assert_eq!(landed.drop_credit.manipulated_by(), None);
        assert_eq!(landed.drop_credit.warped_by_house(), None);
        let locomotor = landed.locomotor.as_ref().unwrap();
        assert_eq!(locomotor.active_kind(), LocomotorKind::Drive);
        assert!(locomotor.piggyback().is_none());
        let location = crate::sim::movement::ground_pose::position_world_coord(&landed.position);
        assert_eq!(
            (location.x & 0xFF, location.y & 0xFF),
            (0x80, 0x80),
            "centred on its cell before the fall (death {death})"
        );
    }
}

/// A tank killed while held or after it is let go is UnInit at once: Unit's
/// receiver keeps only a `Crashable=` type its Crash latched, or a sinking
/// hull (`0x00738457..0x007384A5`), so the drop's crash latch keeps nothing.
/// A held tank's death lets the Magnetron's link go (pointer expiry,
/// `0x00707B70`).
#[test]
fn retail_tank_killed_in_the_air_is_uninit_at_once() {
    let Some(rules) = retail_rules() else {
        return;
    };
    for dropped in [false, true] {
        let (mut sim, grid, magnetron, tank) = lift(&rules);
        climb(&mut sim, &rules, &grid, tank);
        if dropped {
            move_away(&mut sim, magnetron);
            assert!(
                run_until(&mut sim, &rules, &grid, 5, |sim| {
                    entity(sim, tank).drop_credit.being_manipulated()
                }),
                "the move order lets the tank go"
            );
        }
        kill(&mut sim, &rules, tank, magnetron);
        assert!(
            !entity(&sim, tank).lifecycle.object_alive(),
            "dropped {dropped}"
        );
        assert_eq!(
            entity(&sim, magnetron).lift.target(),
            None,
            "dropped {dropped}"
        );
    }
}

/// The arm's admission (`0x004694CB..0x00469700`): the shot must exceed the
/// target's `Size=` (`FCOMP`, equal refuses); only a Unit is lifted; a shot
/// at another tank first lets the held one go (`0x004695FD`), on the ground
/// a release that lets its lift end. A computer house's lifted tank queues
/// Hunt (`0x007103D5`).
#[test]
fn retail_locomotor_arm_admission() {
    let Some(rules) = retail_rules() else {
        return;
    };
    assert_eq!(rules.object("MTNK").unwrap().size, 3.0);
    for (strength, lifted) in [(3, false), (4, true)] {
        let (mut sim, _) = arena(&rules);
        let magnetron = spawn(&mut sim, &rules, "TELE", "Americans", 10, 16);
        let tank = spawn(&mut sim, &rules, "MTNK", "Russians", 16, 16);
        sim.locomotor_detonation(
            magnetron,
            LocomotorShotTarget::Object(tank),
            strength,
            LocomotorKind::Jumpjet,
            &rules,
            None,
            FrameEffects::default(),
        );
        assert_eq!(entity(&sim, tank).lift.swap_active(), lifted);
        assert_eq!(
            entity(&sim, magnetron).lift.target(),
            lifted.then_some(tank)
        );
        if lifted {
            assert_eq!(
                entity(&sim, tank).mission.queued(),
                MissionId::from_known(MissionType::Hunt)
            );
        }
    }

    let (mut sim, _) = arena(&rules);
    let magnetron = spawn(&mut sim, &rules, "TELE", "Americans", 10, 16);
    let gi = spawn(&mut sim, &rules, "E1", "Russians", 16, 16);
    let first = spawn(&mut sim, &rules, "MTNK", "Russians", 16, 18);
    let second = spawn(&mut sim, &rules, "MTNK", "Russians", 16, 20);
    let shoot = |sim: &mut Simulation, target| {
        sim.locomotor_detonation(
            magnetron,
            target,
            5000,
            LocomotorKind::Jumpjet,
            &rules,
            None,
            FrameEffects::default(),
        );
    };
    shoot(&mut sim, LocomotorShotTarget::Object(gi));
    shoot(&mut sim, LocomotorShotTarget::Cell);
    assert!(!entity(&sim, gi).lift.swap_active());
    assert_eq!(entity(&sim, magnetron).lift.target(), None);

    shoot(&mut sim, LocomotorShotTarget::Object(first));
    assert_eq!(entity(&sim, magnetron).lift.target(), Some(first));
    shoot(&mut sim, LocomotorShotTarget::Object(first));
    assert_eq!(entity(&sim, first).lift.source(), Some(magnetron));

    shoot(&mut sim, LocomotorShotTarget::Object(second));
    assert_eq!(entity(&sim, magnetron).lift.target(), Some(second));
    assert_eq!(entity(&sim, second).lift.source(), Some(magnetron));
    let released = entity(&sim, first);
    assert_eq!(released.lift.source(), None);
    assert!(
        released.lift.may_end(),
        "a grounded release lets the lift end"
    );
    assert!(!released.crash.is_crashing() && !released.drop_credit.being_manipulated());
}

/// The lift queues Guard to commence (`0x00710386`) after leaving the team,
/// whose `Enter_Idle_Mode` queued a Move toward the firer (`0x00738970`). So
/// the Queue writes Guard and asks the Unit's `Is_Ready_To_Commence`
/// (`0x00744270`): a human's tank clear of any factory commences it at once
/// (#2318: a readiness without rules answered not ready and left Guard queued
/// for the next frame). A computer house's tank queues Hunt over either
/// answer.
#[test]
fn retail_lifted_team_member_commences_guard_at_once() {
    let Some(rules) = retail_rules() else {
        return;
    };
    let (mut sim, _) = arena(&rules);
    let magnetron = spawn(&mut sim, &rules, "TELE", "Russians", 10, 16);
    let tank = spawn(&mut sim, &rules, "MTNK", "Americans", 16, 16);
    let owner = sim.interner.intern("Americans");
    let script = sim.interner.intern("LIFTED TEAM");
    sim.team_script_vm.create_team(owner, script, vec![tank], 0);
    sim.locomotor_detonation(
        magnetron,
        LocomotorShotTarget::Object(tank),
        4,
        LocomotorKind::Jumpjet,
        &rules,
        None,
        FrameEffects::default(),
    );
    let lifted = entity(&sim, tank);
    assert!(lifted.lift.swap_active());
    assert_eq!(sim.team_script_vm.team_for_member(tank), None);
    assert_eq!(
        lifted.mission.current(),
        MissionId::from_known(MissionType::Guard)
    );
    assert_eq!(lifted.mission.queued(), MissionId::NONE);
}

/// The actual detonation arm reads the fractional MTNK Size through the
/// retail Battle layers and the current Type getter before applying FCOMP.
#[test]
fn retail_magnetron_admission_reads_fractional_type_size() {
    let patch = crate::rules::ini_parser::IniFile::from_str("[MTNK]\nSize=1.5\n");
    let Some(retail) = crate::rules::retail_ini_fixture::retail_battle_rules_for_map_with_patch(
        "XMP03T4.MAP",
        &patch,
    ) else {
        return;
    };
    let rules = &retail.rules;
    assert_eq!(rules.object("MTNK").unwrap().size, 1.5);

    let (mut sim, _) = arena(rules);
    let magnetron = spawn(&mut sim, rules, "TELE", "Americans", 10, 16);
    let tank = spawn(&mut sim, rules, "MTNK", "Russians", 16, 16);
    for (strength, lifted) in [(1, false), (2, true)] {
        sim.locomotor_detonation(
            magnetron,
            LocomotorShotTarget::Object(tank),
            strength,
            LocomotorKind::Jumpjet,
            rules,
            None,
            FrameEffects::default(),
        );
        assert_eq!(
            entity(&sim, tank).lift.swap_active(),
            lifted,
            "strength {strength}"
        );
    }
}

/// The links and the drop's credit are saved, and each one moves the world
/// hash.
#[test]
fn lift_links_and_drop_credit_are_saved_and_hashed() {
    let rules = RuleSet::from_ini(&crate::rules::ini_parser::IniFile::from_str(
        "[VehicleTypes]\n0=TANK\n[TANK]\nStrength=300\nSpeed=7\n",
    ))
    .unwrap();
    // Without a map: the arena's map state does not come back through
    // `GameSnapshot::load` alone.
    let mut sim = Simulation::with_seed(0x1269);
    for (name, human) in [("Russians", false), ("Americans", true)] {
        let id = sim.interner.intern(name);
        sim.houses
            .register(crate::sim::house_state::HouseState::new(
                id, 0, None, human, 0, 10,
            ));
    }
    let magnetron = sim
        .spawn_object_at_height("TANK", "Americans", 10, 16, 0, 0, &rules)
        .unwrap();
    let tank = sim
        .spawn_object_at_height("TANK", "Russians", 16, 16, 0, 0, &rules)
        .unwrap();
    let house = sim.interner.get("Americans").unwrap();
    // A load restarts the Scenario stream from Seed0; put the source there too.
    sim.scenario_rng = crate::sim::rng::SimRng::new(0);
    type Write = fn(&mut GameEntity, u64, crate::sim::intern::InternedId);
    let writes: [(u64, Write); 6] = [
        (magnetron, |entity, other, _| {
            entity.lift.set_target_for_test(Some(other))
        }),
        (tank, |entity, other, _| {
            entity.lift.set_source_for_test(Some(other))
        }),
        (tank, |entity, _, _| entity.lift.set_may_end_for_test(true)),
        (tank, |entity, _, _| {
            entity.drop_credit.set_being_manipulated_for_test(true)
        }),
        (tank, |entity, other, _| {
            entity.drop_credit.set_manipulated_by_for_test(Some(other))
        }),
        (tank, |entity, _, house| {
            entity.drop_credit.set_warped_by_house_for_test(Some(house))
        }),
    ];
    for (id, write) in writes {
        let before = sim.state_hash();
        let other = if id == tank { magnetron } else { tank };
        write(sim.substrate.entities.get_mut(id).unwrap(), other, house);
        assert_ne!(sim.state_hash(), before);
    }
    let bytes = crate::sim::snapshot::GameSnapshot::save(&sim, 0, 0, "magnetron_lift", 0);
    let mut loaded = crate::sim::snapshot::GameSnapshot::load(&bytes)
        .unwrap()
        .sim;
    loaded.restore_after_snapshot_load().unwrap();
    assert_eq!(loaded.state_hash(), sim.state_hash());
    let held = entity(&loaded, tank);
    assert_eq!(held.lift.source(), Some(magnetron));
    assert!(held.lift.may_end() && held.drop_credit.being_manipulated());
    assert_eq!(held.drop_credit.manipulated_by(), Some(magnetron));
    assert_eq!(held.drop_credit.warped_by_house(), Some(house));
    assert_eq!(entity(&loaded, magnetron).lift.target(), Some(tank));
}
