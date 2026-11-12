//! Ares EMP victims hit by real fire: retail Hills rules with an EMP gun
//! layered in through the map, the attacker ordered by a queued Attack
//! command and every frame stepped by `advance_tick`.

use crate::map::entities::EntityCategory;
use crate::rules::ini_parser::IniFile;
use crate::rules::ruleset::RuleSet;
use crate::sim::command::{Command, CommandEnvelope};
use crate::sim::mission::MissionType;
use crate::sim::pathfinding::PathGrid;
use crate::sim::vision::FogState;
use crate::sim::world::{SimSoundEvent, Simulation};
use std::sync::Arc;

/// The Grizzly's and the Destroyer's guns fire one EMP shell each; the
/// warhead's `Malicious=no` keeps the damage path's own warnings quiet. The
/// building tests (`emp_building_tests.rs`) also take the Cloning Vats (not
/// prone by type) and a Flak Cannon made a LaserFencePost.
const PATCH: &str = "[MTNK]\nPrimary=EMPGun\n[DEST]\nPrimary=EMPGun\n\
     [EMPGun]\nDamage=1\nROF=1000\nRange=6\nProjectile=Invisible\nSpeed=100\nWarhead=EMPWH\n\
     [EMPWH]\nCellSpread=2\nMalicious=no\nEMP.Duration=150\n\
     [E1]\nImmuneToEMP=no\n[V3]\nImmuneToEMP=yes\n\
     [NACLON]\nImmuneToEMP=no\n[NASAM]\nLaserFencePost=yes\n";

pub(super) fn rules() -> Option<RuleSet> {
    let rules = crate::test_fixture::once_per_process("ares emp victims", || {
        crate::rules::retail_ini_fixture::retail_battle_rules_with_map_patch(&IniFile::from_str(
            PATCH,
        ))
        .map(|retail| retail.rules)
    })?;
    Some(rules.with_fresh_serial())
}

/// The rival arena (human Americans, computer Russians), every cell seen by
/// the Americans, commands executing on the next frame.
pub(super) fn scene(rules: &RuleSet, water: bool) -> (Simulation, Arc<PathGrid>) {
    let mut sim = if water {
        crate::sim::arena_fixture::rival_water_arena(rules, 0xE3B, 32)
    } else {
        crate::sim::arena_fixture::rival_arena(rules, 0xE3B)
    };
    sim.set_input_delay_ticks(0);
    let americans = sim.interner.intern("Americans");
    sim.session.set_current_house_for_test(Some(americans));
    sim.fog = FogState::new(32, 32);
    for y in 0..32 {
        for x in 0..32 {
            sim.fog.mark_visible_for_owner(americans, x, y);
        }
    }
    sim.intern_rule_type_ids(rules);
    sim.resolve_type_handles(rules);
    let grid = sim.path_grid_snapshot().expect("arena path grid");
    (sim, grid)
}

pub(super) fn spawn(
    sim: &mut Simulation,
    rules: &RuleSet,
    kind: &str,
    owner: &str,
    rx: u16,
    ry: u16,
) -> u64 {
    sim.spawn_object(kind, owner, rx, ry, 0, rules)
        .unwrap_or_else(|| panic!("{kind} spawns"))
}

/// The human Americans send every order: DoList skips a computer House's
/// events, and the order's actor need not be the sender's.
pub(super) fn order(sim: &mut Simulation, command: Command) {
    let americans = sim.interner.intern("Americans");
    let tick = sim.session.tick() + 1;
    sim.queue_command(CommandEnvelope::new(americans, tick, command));
}

pub(super) fn step(sim: &mut Simulation, rules: &RuleSet, grid: &PathGrid) {
    let commands = sim.take_due_commands();
    sim.advance_tick(&commands, Some(rules), Some(grid), None, 33);
}

pub(super) fn emp(sim: &Simulation, id: u64) -> i32 {
    sim.entities()
        .get(id)
        .map_or(0, |entity| entity.emp_remaining())
}

pub(super) fn offline(sim: &Simulation, id: u64) -> bool {
    sim.entities()
        .get(id)
        .is_some_and(|entity| entity.deactivated.is_offline())
}

/// Steps until `until` holds, at most `frames` frames; whether it held.
pub(super) fn run(
    sim: &mut Simulation,
    rules: &RuleSet,
    grid: &PathGrid,
    frames: u32,
    mut until: impl FnMut(&Simulation) -> bool,
) -> bool {
    for _ in 0..frames {
        if until(sim) {
            return true;
        }
        step(sim, rules, grid);
    }
    until(sim)
}

/// `attacker` fires its EMP shell at `target`; true once `victim` is hit.
pub(super) fn fire(
    sim: &mut Simulation,
    rules: &RuleSet,
    grid: &PathGrid,
    attacker: u64,
    target: u64,
    victim: u64,
) -> bool {
    order(
        sim,
        Command::Attack {
            attacker_id: attacker,
            target_id: target,
        },
    );
    run(sim, rules, grid, 60, |sim| emp(sim, victim) > 0)
}

fn location(sim: &Simulation, id: u64) -> crate::sim::components::DriveCoord {
    crate::sim::movement::ground_pose::object_location(
        sim.entities().get(id).unwrap(),
        sim.resolved_terrain.as_ref(),
    )
}

pub(super) fn missions(sim: &Simulation, id: u64) -> (Option<MissionType>, Option<MissionType>) {
    let entity = sim.entities().get(id).unwrap();
    (
        entity.mission.current().known(),
        entity.mission.queued().known(),
    )
}

/// Installed10062320 admits a computer infantryman on Walk like any other
/// Foot: a walking Cyborg-prone E1 stops while the counter runs, then
/// DisableEMPEffect100627F0's computer branch queues Hunt for it.
#[test]
fn ares_emp_stops_a_walking_computer_infantryman_until_it_hunts() {
    let Some(rules) = rules() else {
        return;
    };
    let (mut sim, grid) = scene(&rules, false);
    let gun = spawn(&mut sim, &rules, "MTNK", "Americans", 12, 10);
    let e1 = spawn(&mut sim, &rules, "E1", "Russians", 15, 10);
    order(
        &mut sim,
        Command::Move {
            entity_id: e1,
            target_rx: 28,
            target_ry: 10,
        },
    );
    step(&mut sim, &rules, &grid);
    assert!(fire(&mut sim, &rules, &grid, gun, e1, e1), "E1 is hit");
    assert!(offline(&sim, e1));
    run(&mut sim, &rules, &grid, 20, |_| false);
    let stopped = location(&sim, e1);
    assert!(run(&mut sim, &rules, &grid, 200, |sim| emp(sim, e1) == 0));
    assert_eq!(location(&sim, e1), stopped, "no step while offline");
    assert!(!offline(&sim, e1));
    let (current, queued) = missions(&sim, e1);
    assert!(
        current == Some(MissionType::Hunt) || queued == Some(MissionType::Hunt),
        "{current:?} {queued:?}"
    );
}

/// DisableEMPEffect100627F0's Unit Harvester/ResourceGatherer branch: a
/// human War Miner, Chrono Miner (Teleport) and Slave Miner start their
/// saved mission again, Enter for a saved Guard, instead of nothing.
#[test]
fn ares_emp_miners_restart_their_saved_mission() {
    let Some(rules) = rules() else {
        return;
    };
    let (mut sim, grid) = scene(&rules, false);
    let gun = spawn(&mut sim, &rules, "MTNK", "Russians", 12, 10);
    let harv = spawn(&mut sim, &rules, "HARV", "Americans", 15, 10);
    let cmin = spawn(&mut sim, &rules, "CMIN", "Americans", 15, 11);
    let smin = spawn(&mut sim, &rules, "SMIN", "Americans", 16, 10);
    let miners = [harv, cmin, smin];
    step(&mut sim, &rules, &grid);
    let before = miners.map(|id| missions(&sim, id).0);
    assert!(
        fire(&mut sim, &rules, &grid, gun, harv, harv),
        "HARV is hit"
    );
    for id in miners {
        assert!(
            emp(&sim, id) > 0 && offline(&sim, id),
            "{id} shares the spread"
        );
    }
    assert!(run(&mut sim, &rules, &grid, 200, |sim| miners
        .iter()
        .all(|&id| emp(sim, id) == 0)));
    for (id, before) in miners.into_iter().zip(before) {
        assert!(!offline(&sim, id));
        let expected = match before {
            Some(MissionType::Guard) => Some(MissionType::Enter),
            other => other,
        };
        assert_eq!(missions(&sim, id).0, expected, "saved {before:?}");
    }
}

/// enableEMPEffect10062B40 does not touch the victim's controller: a tank
/// Yuri took is deactivated, stays his and hunts for his computer House.
#[test]
fn ares_emp_keeps_a_mind_controlled_tank_with_its_controller() {
    let Some(rules) = rules() else {
        return;
    };
    let (mut sim, grid) = scene(&rules, false);
    let yuri = spawn(&mut sim, &rules, "YURI", "Russians", 8, 10);
    let tank = spawn(&mut sim, &rules, "HTNK", "Americans", 11, 10);
    let gun = spawn(&mut sim, &rules, "MTNK", "Americans", 14, 10);
    order(
        &mut sim,
        Command::Attack {
            attacker_id: yuri,
            target_id: tank,
        },
    );
    assert!(run(&mut sim, &rules, &grid, 120, |sim| sim
        .entities()
        .get(tank)
        .is_some_and(
            |tank| tank.mind_control.controller() == Some(yuri)
        )));
    assert!(
        fire(&mut sim, &rules, &grid, gun, tank, tank),
        "tank is hit"
    );
    assert!(offline(&sim, tank));
    assert!(run(&mut sim, &rules, &grid, 200, |sim| emp(sim, tank) == 0));
    let captive = sim.entities().get(tank).unwrap();
    assert_eq!(captive.mind_control.controller(), Some(yuri));
    assert_eq!(sim.interner.resolve(captive.owner()), "Russians");
    assert!(!captive.deactivated.is_offline());
    let (current, queued) = missions(&sim, tank);
    assert!(
        current == Some(MissionType::Hunt) || queued == Some(MissionType::Hunt),
        "{current:?} {queued:?}"
    );
}

/// A paratrooper caught in the spread keeps falling (ObjectAI5F3F11 tests
/// no deactivation), its canopy spares it from the EMP.Threshold in-air
/// arm of10062320, and it lands offline until the counter runs out.
#[test]
fn ares_emp_falling_paratrooper_lands_offline() {
    let Some(mut rules) = rules() else {
        return;
    };
    // The canopy's physical frame bound, which the app's SHP binder supplies
    // (retail PARACH.SHP holds 140 frames); the rules fixture binds no art.
    rules.bind_anim_frame_count_for_test("PARACH", 140);
    let (mut sim, grid) = scene(&rules, false);
    let gun = spawn(&mut sim, &rules, "MTNK", "Americans", 12, 10);
    let tank = spawn(&mut sim, &rules, "HTNK", "Russians", 15, 10);
    let trooper = spawn(&mut sim, &rules, "E1", "Russians", 15, 11);
    let drop = location(&sim, trooper).z + 300;
    assert!(
        crate::sim::movement::parachute_descent::begin_parachute_descent(
            &mut sim.substrate.entities,
            trooper,
            drop,
        )
    );
    assert!(sim.attach_parachute_anim(&rules, trooper).is_some());
    assert!(
        fire(&mut sim, &rules, &grid, gun, tank, tank),
        "tank is hit"
    );
    let falling = sim.entities().get(trooper).unwrap();
    assert!(falling.is_falling_down());
    assert!(
        falling.emp_remaining() > 0,
        "the spread reaches the trooper"
    );
    assert!(falling.deactivated.is_offline());
    assert!(run(&mut sim, &rules, &grid, 200, |sim| !sim
        .entities()
        .get(trooper)
        .unwrap()
        .is_falling_down()));
    let landed = sim.entities().get(trooper).unwrap();
    assert!(landed.lifecycle.object_alive() && landed.health.current() > 0);
    assert!(landed.emp_remaining() > 0 && landed.deactivated.is_offline());
    assert!(run(&mut sim, &rules, &grid, 200, |sim| emp(sim, trooper) == 0));
    assert!(!offline(&sim, trooper));
}

/// Hover and Ship movers take the same delivery: Deactivate's Power_Off
/// (Hover's override spins it down) stops the moving amphibious APC and
/// the Sea Scorpion within the cell it is crossing, and both power up and
/// hunt once the counter ends.
#[test]
fn ares_emp_stops_hover_and_ship_movers() {
    let Some(rules) = rules() else {
        return;
    };
    for water in [false, true] {
        let (mut sim, grid) = scene(&rules, water);
        let (gun, victim) = if water {
            (
                spawn(&mut sim, &rules, "DEST", "Americans", 12, 10),
                spawn(&mut sim, &rules, "HYD", "Russians", 15, 10),
            )
        } else {
            (
                spawn(&mut sim, &rules, "MTNK", "Americans", 12, 10),
                spawn(&mut sim, &rules, "SAPC", "Russians", 15, 10),
            )
        };
        order(
            &mut sim,
            Command::Move {
                entity_id: victim,
                target_rx: 28,
                target_ry: 10,
            },
        );
        step(&mut sim, &rules, &grid);
        assert!(
            fire(&mut sim, &rules, &grid, gun, victim, victim),
            "victim is hit, water={water}"
        );
        assert!(offline(&sim, victim));
        assert!(
            !sim.entities()
                .get(victim)
                .unwrap()
                .locomotor
                .as_ref()
                .unwrap()
                .is_powered()
        );
        let hit = location(&sim, victim);
        assert!(run(&mut sim, &rules, &grid, 200, |sim| emp(sim, victim) == 0));
        let stopped = location(&sim, victim);
        assert!(
            (stopped.x - hit.x).abs() <= 256 && (stopped.y - hit.y).abs() <= 256,
            "{hit:?} -> {stopped:?} water={water}"
        );
        let entity = sim.entities().get(victim).unwrap();
        assert!(!entity.deactivated.is_offline());
        assert!(entity.locomotor.as_ref().unwrap().is_powered());
        let (current, queued) = missions(&sim, victim);
        assert!(
            current == Some(MissionType::Hunt) || queued == Some(MissionType::Hunt),
            "{current:?} {queued:?} water={water}"
        );
    }
}

/// A V3 rocket tilting on its launcher takes the Aircraft branch of
/// enableEMPEffect10062B40 like any Fly mover: above the ground it crashes
/// through TechnoExt::Destroy10044300. The launcher is made immune so its
/// own updateSpawnManager10063070 cannot take the rocket first.
#[test]
fn ares_emp_downs_a_v3_rocket_on_its_launcher() {
    let Some(rules) = rules() else {
        return;
    };
    let (mut sim, grid) = scene(&rules, false);
    let gun = spawn(&mut sim, &rules, "MTNK", "Americans", 12, 10);
    let v3 = spawn(&mut sim, &rules, "V3", "Russians", 15, 10);
    let mark = spawn(&mut sim, &rules, "MTNK", "Americans", 15, 26);
    let rocket = sim
        .entities()
        .get(v3)
        .and_then(|v3| v3.spawn_manager.as_ref())
        .and_then(|manager| manager.slots().first().and_then(|slot| slot.spawn()))
        .expect("the V3 holds its rocket");
    order(
        &mut sim,
        Command::Attack {
            attacker_id: v3,
            target_id: mark,
        },
    );
    assert!(
        run(&mut sim, &rules, &grid, 120, |sim| sim
            .entities()
            .get(rocket)
            .is_some_and(|rocket| !rocket.lifecycle.in_limbo())),
        "the rocket leaves the launcher"
    );
    let rocket_entity = sim.entities().get(rocket).unwrap();
    assert_eq!(rocket_entity.category(), EntityCategory::Aircraft);
    assert!(
        crate::sim::movement::air_movement::current_fly_height(
            rocket_entity,
            sim.resolved_terrain.as_ref()
        ) > 0
    );
    order(
        &mut sim,
        Command::Attack {
            attacker_id: gun,
            target_id: v3,
        },
    );
    assert!(
        run(&mut sim, &rules, &grid, 60, |sim| sim
            .entities()
            .get(rocket)
            .is_none_or(|rocket| rocket.health.current() == 0)),
        "the rocket crashes"
    );
    assert_eq!(emp(&sim, v3), 0, "the launcher is immune");
}

/// A Radar and SpySat provider is an independent powered building too:
/// enableEMPEffect10062B40 and DisableEMPEffect100627F0 dirty
/// RecheckRadar, and the House radar and SpySat scans (Ares 10024550,
/// 10024620) skip it while the counter runs, so the uplink's radar and map
/// go dark and come back with it.
#[test]
fn ares_emp_darkens_a_spy_satellite_uplink_until_expiry() {
    let Some(rules) = rules() else {
        return;
    };
    let (mut sim, grid) = scene(&rules, false);
    let americans = sim.interner.intern("Americans");
    let gun = spawn(&mut sim, &rules, "MTNK", "Russians", 12, 10);
    let uplink = spawn(&mut sim, &rules, "GASPYSAT", "Americans", 15, 9);
    let plant = spawn(&mut sim, &rules, "GAPOWR", "Americans", 22, 22);
    for id in [uplink, plant] {
        crate::sim::arena_fixture::discover(&mut sim, id);
        crate::sim::arena_fixture::own(&mut sim, id);
    }
    let lit = |sim: &Simulation| {
        (
            crate::sim::power_system::has_active_radar(sim.power_states.as_map(), americans),
            sim.houses[&americans].spy_sat_active(),
        )
    };
    assert!(
        run(&mut sim, &rules, &grid, 60, |sim| lit(sim) == (true, true)),
        "the powered uplink lights radar and map first: {:?}",
        lit(&sim)
    );
    assert!(
        fire(&mut sim, &rules, &grid, gun, uplink, uplink),
        "uplink is hit"
    );
    assert!(
        run(&mut sim, &rules, &grid, 30, |sim| lit(sim)
            == (false, false)),
        "{:?}",
        lit(&sim)
    );
    assert!(run(&mut sim, &rules, &grid, 200, |sim| emp(sim, uplink) == 0));
    assert!(
        run(&mut sim, &rules, &grid, 30, |sim| lit(sim) == (true, true)),
        "{:?}",
        lit(&sim)
    );
}

/// announceAttack10062150 for the player's own victims: the War Miner's
/// ore-miner warning and the Ore Purifier's base warning, with no ally
/// line. `Malicious=no` keeps the damage path's warnings out.
#[test]
fn ares_emp_announces_the_players_miner_and_base() {
    let Some(rules) = rules() else {
        return;
    };
    let (mut sim, grid) = scene(&rules, false);
    let americans = sim.interner.intern("Americans");
    let gun = spawn(&mut sim, &rules, "MTNK", "Russians", 12, 10);
    let harv = spawn(&mut sim, &rules, "HARV", "Americans", 15, 10);
    let other = spawn(&mut sim, &rules, "MTNK", "Russians", 12, 20);
    let purifier = spawn(&mut sim, &rules, "GAOREP", "Americans", 15, 19);
    assert_eq!(
        sim.entities().get(purifier).unwrap().category(),
        EntityCategory::Structure
    );
    order(
        &mut sim,
        Command::Attack {
            attacker_id: gun,
            target_id: harv,
        },
    );
    order(
        &mut sim,
        Command::Attack {
            attacker_id: other,
            target_id: purifier,
        },
    );
    assert!(run(&mut sim, &rules, &grid, 60, |sim| emp(sim, harv) > 0
        && emp(sim, purifier) > 0));
    // The fixture frame keeps every frame's sounds; nothing collects them.
    let mut warnings: Vec<_> = sim
        .sound_events
        .iter()
        .filter_map(|event| match event {
            SimSoundEvent::UnderAttack { owner, miner, .. } => Some((*owner, *miner)),
            SimSoundEvent::AllyUnderAttack { owner, .. } => Some((*owner, false)),
            _ => None,
        })
        .collect();
    warnings.sort();
    assert_eq!(warnings, [(americans, false), (americans, true)]);
}
