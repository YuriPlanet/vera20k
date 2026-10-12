//! A vehicle's approach to what it attacks, through the production frame:
//! its mission's Approach (`FootClass::Approach_Target @ 0x004D5690`, reached
//! through `UnitClass::Approach_Target @ 0x007414E0`) picks a cell to fire
//! from, and the per-cell range stop (`0x004D882F..0x004D896E`) halts it on
//! entering a cell with a Foot Target in range. Expected behaviour comes from
//! instruction reading.

use crate::rules::ruleset::RuleSet;
use crate::sim::combat::AttackTarget;
use crate::sim::command::{Command, CommandEnvelope};
use crate::sim::components::NavTargetRef;
use crate::sim::mission::{MissionId, MissionType};
use crate::sim::movement::ground_pose::position_world_coord;
use crate::sim::world::Simulation;

fn retail_rules() -> Option<RuleSet> {
    let (ini, art) = crate::rules::retail_ini_fixture::retail_rules_and_art()?;
    let mut rules = RuleSet::from_ini_with_fixed_art_for_test(&ini, &art).expect("retail rules");
    rules.install_art_data(crate::rules::art_data::ArtRegistry::from_ini(&art));
    Some(rules)
}

/// The flat arena with the computer `Russians` and the human `Americans`.
fn arena(rules: &RuleSet) -> Simulation {
    let mut sim = crate::sim::arena_fixture::rival_arena(rules, 0x4D5690);
    sim.intern_rule_type_ids(rules);
    sim.resolve_type_handles(rules);
    sim.input_delay_ticks = 0;
    sim.session.binary_frame = 1;
    sim
}

fn spawn(sim: &mut Simulation, rules: &RuleSet, kind: &str, owner: &str, cell: (u16, u16)) -> u64 {
    sim.spawn_object(kind, owner, cell.0, cell.1, 0, rules)
        .unwrap_or_else(|| panic!("{kind} spawns"))
}

fn start_mission(sim: &mut Simulation, id: u64, mission: MissionType) {
    let now = sim.session.binary_frame;
    sim.mission_assign_exact(id, MissionId::from_known(mission), now)
        .unwrap();
    let _ = sim.mission_commence_exact(id, now);
}

fn target(sim: &mut Simulation, id: u64, target: u64) {
    sim.substrate.entities.get_mut(id).unwrap().attack_target = Some(AttackTarget::new(target));
}

fn order(sim: &mut Simulation, owner: &str, command: Command) {
    let owner = sim.interner.intern(owner);
    let tick = sim.session.tick + 1;
    sim.queue_command(CommandEnvelope::new(owner, tick, command));
}

fn step(sim: &mut Simulation, rules: &RuleSet) {
    let commands = sim.take_due_commands();
    let grid = sim.path_grid_snapshot();
    sim.advance_tick(&commands, Some(rules), grid.as_deref(), None, 33);
}

/// What one frame left of the pursuer and its Target.
#[derive(Debug, Clone, Copy)]
struct Frame {
    /// The pursuer's coordinate, in leptons.
    at: (i32, i32),
    nav_com: Option<NavTargetRef>,
    /// Lepton distance from the pursuer to the Target's coordinate.
    distance: f64,
}

impl Frame {
    fn at_cell_centre(&self) -> bool {
        self.at.0.rem_euclid(256) == 128 && self.at.1.rem_euclid(256) == 128
    }

    /// Within a quarter cell of a cell centre on both axes, where a Hover
    /// takes its next cell.
    fn near_cell_centre(&self) -> bool {
        (self.at.0.rem_euclid(256) - 128).abs() <= 64
            && (self.at.1.rem_euclid(256) - 128).abs() <= 64
    }

    fn cell(&self) -> (u16, u16) {
        ((self.at.0 / 256) as u16, (self.at.1 / 256) as u16)
    }
}

fn frame(sim: &Simulation, pursuer: u64, target: u64) -> Frame {
    let own = position_world_coord(&sim.substrate.entities.get(pursuer).unwrap().position);
    let aim = position_world_coord(&sim.substrate.entities.get(target).unwrap().position);
    let (dx, dy, dz) = (
        f64::from(own.x - aim.x),
        f64::from(own.y - aim.y),
        f64::from(own.z - aim.z),
    );
    Frame {
        at: (own.x, own.y),
        nav_com: sim
            .substrate
            .entities
            .get(pursuer)
            .unwrap()
            .navigation
            .nav_com,
        distance: (dx * dx + dy * dy + dz * dz).sqrt(),
    }
}

/// Steps until the pursuer, once it holds a NavCom, lets it go. Answers every
/// frame from the first NavCom to the stop, which is last.
fn chase(sim: &mut Simulation, rules: &RuleSet, pursuer: u64, target: u64) -> Vec<Frame> {
    let mut frames = Vec::new();
    for _ in 0..900 {
        step(sim, rules);
        let now = frame(sim, pursuer, target);
        if now.nav_com.is_some() || !frames.is_empty() {
            frames.push(now);
        }
        if now.nav_com.is_none() && !frames.is_empty() {
            return frames;
        }
    }
    panic!("the pursuer never stopped");
}

fn range(rules: &RuleSet, weapon: &str) -> f64 {
    f64::from(rules.weapon(weapon).unwrap().range_leptons)
}

fn cell_centre_distance(cell: (u16, u16), sim: &Simulation, target: u64) -> f64 {
    let aim = position_world_coord(&sim.substrate.entities.get(target).unwrap().position);
    let (x, y) = (
        f64::from(i32::from(cell.0) * 256 + 128),
        f64::from(i32::from(cell.1) * 256 + 128),
    );
    ((x - f64::from(aim.x)).powi(2) + (y - f64::from(aim.y)).powi(2)).sqrt()
}

/// A human's Rhino ordered to attack a tank 20 cells away sets out for a cell
/// its Approach picks within 120mm range, not the tank's own cell. It keeps
/// that NavCom while it drives, also through the frames where the tank first
/// comes in range between cell centres, and lets it go on entering the first
/// cell from which the tank is in range.
#[test]
fn a_rhino_stops_on_entering_the_first_cell_with_its_target_in_range() {
    let Some(rules) = retail_rules() else {
        return;
    };
    let mut sim = arena(&rules);
    let rhino = spawn(&mut sim, &rules, "HTNK", "Americans", (4, 10));
    let tank = spawn(&mut sim, &rules, "MTNK", "Russians", (24, 10));
    order(
        &mut sim,
        "Americans",
        Command::Attack {
            attacker_id: rhino,
            target_id: tank,
        },
    );
    let frames = chase(&mut sim, &rules, rhino, tank);
    let reach = range(&rules, "120mm");

    let Some(NavTargetRef::Cell { rx, ry }) = frames[0].nav_com else {
        panic!("Approach sets a cell: {:?}", frames[0].nav_com);
    };
    assert_ne!((rx, ry), (24, 10));
    assert!(cell_centre_distance((rx, ry), &sim, tank) <= reach);

    let (stop, before) = frames.split_last().unwrap();
    assert!(stop.at_cell_centre(), "stopped mid-cell at {:?}", stop.at);
    assert!(stop.distance <= reach, "{stop:?}");
    assert!(
        before
            .iter()
            .any(|f| !f.at_cell_centre() && f.distance <= reach),
        "the tank came in range between cell centres"
    );
    let last_centre = before
        .iter()
        .rev()
        .find(|f| f.at_cell_centre() && f.at != stop.at);
    assert!(
        last_centre.is_none_or(|f| f.distance > reach),
        "{last_centre:?}"
    );
}

/// A Rhino attacking a power plant drives to the cell its Approach picks,
/// whose ring the foundation widens (Approach may pick again on the way, once
/// that cell lies beyond weapon range of the plant's centre), and stops there
/// on arrival: the per-cell range stop measures only a Foot Target.
#[test]
fn a_rhino_drives_to_its_approach_cell_to_shell_a_building() {
    let Some(rules) = retail_rules() else {
        return;
    };
    let mut sim = arena(&rules);
    let rhino = spawn(&mut sim, &rules, "HTNK", "Americans", (4, 10));
    let plant = spawn(&mut sim, &rules, "NAPOWR", "Russians", (24, 10));
    order(
        &mut sim,
        "Americans",
        Command::Attack {
            attacker_id: rhino,
            target_id: plant,
        },
    );
    let frames = chase(&mut sim, &rules, rhino, plant);
    let Some(NavTargetRef::Cell { rx, ry }) = frames[0].nav_com else {
        panic!("Approach sets a cell: {:?}", frames[0].nav_com);
    };
    assert!(!(24..=25).contains(&rx) || !(10..=12).contains(&ry));
    let (stop, before) = frames.split_last().unwrap();
    assert!(stop.at_cell_centre(), "stopped mid-cell at {:?}", stop.at);
    let (x, y) = stop.cell();
    assert_eq!(
        before.last().unwrap().nav_com,
        Some(NavTargetRef::cell(x, y)),
        "stopped short of its approach cell"
    );
}

/// A Robot Tank, on Hover, approaches the same way. Its Process runs
/// Per_Cell_Process(2) as it takes its next cell (`0x00515A1C`) or arrives
/// (`0x005146CA`), near a cell centre, so it stops there.
#[test]
fn a_robot_tank_stops_on_entering_the_first_cell_with_its_target_in_range() {
    let Some(rules) = retail_rules() else {
        return;
    };
    let mut sim = arena(&rules);
    // `PoweredUnit=` wants a powered Robot Control Center.
    spawn(&mut sim, &rules, "GAPOWR", "Americans", (2, 20));
    spawn(&mut sim, &rules, "GAROBO", "Americans", (6, 20));
    let robot = spawn(&mut sim, &rules, "ROBO", "Americans", (4, 10));
    let tank = spawn(&mut sim, &rules, "MTNK", "Russians", (24, 13));
    order(
        &mut sim,
        "Americans",
        Command::Attack {
            attacker_id: robot,
            target_id: tank,
        },
    );
    let frames = chase(&mut sim, &rules, robot, tank);
    let reach = range(&rules, "Robogun");
    let Some(NavTargetRef::Cell { rx, ry }) = frames[0].nav_com else {
        panic!("Approach sets a cell: {:?}", frames[0].nav_com);
    };
    assert_ne!((rx, ry), (24, 13));
    assert!(cell_centre_distance((rx, ry), &sim, tank) <= reach);
    let stop = frames.last().unwrap();
    assert!(stop.near_cell_centre(), "stopped mid-cell at {:?}", stop.at);
    assert!(stop.distance <= reach, "{stop:?}");
}

/// A computer's Rhino on Hunt approaches what its scan picks up the same way:
/// to a cell in range, stopping on entering the first cell with the tank in
/// range.
#[test]
fn a_hunting_rhino_approaches_its_quarry_and_stops_in_range() {
    let Some(rules) = retail_rules() else {
        return;
    };
    let mut sim = arena(&rules);
    let rhino = spawn(&mut sim, &rules, "HTNK", "Russians", (4, 10));
    let tank = spawn(&mut sim, &rules, "MTNK", "Americans", (24, 10));
    start_mission(&mut sim, rhino, MissionType::Hunt);
    let frames = chase(&mut sim, &rules, rhino, tank);
    let reach = range(&rules, "120mm");
    let Some(NavTargetRef::Cell { rx, ry }) = frames[0].nav_com else {
        panic!("Approach sets a cell: {:?}", frames[0].nav_com);
    };
    assert_ne!((rx, ry), (24, 10));
    assert!(cell_centre_distance((rx, ry), &sim, tank) <= reach);
    let (stop, before) = frames.split_last().unwrap();
    assert!(stop.at_cell_centre(), "stopped mid-cell at {:?}", stop.at);
    assert!(stop.distance <= reach, "{stop:?}");
    assert!(
        before
            .iter()
            .any(|f| !f.at_cell_centre() && f.distance <= reach),
        "the tank came in range between cell centres"
    );
    assert_eq!(
        sim.substrate.entities.get(rhino).unwrap().mission.current(),
        MissionId::from_known(MissionType::Hunt)
    );
}

/// When the tank drives at the Rhino after Approach picked its cell, the
/// Rhino lets its NavCom go on entering the first cell from which the tank is
/// in range, short of that cell. It then comes to rest at most one cell on:
/// a stop at a track chain point (`0x004B1CF5`) still drives the track it had
/// chained into.
#[test]
fn a_rhino_stops_short_of_its_approach_cell_when_its_target_closes_in() {
    let Some(rules) = retail_rules() else {
        return;
    };
    let mut sim = arena(&rules);
    let rhino = spawn(&mut sim, &rules, "HTNK", "Americans", (4, 10));
    let tank = spawn(&mut sim, &rules, "MTNK", "Russians", (24, 10));
    order(
        &mut sim,
        "Americans",
        Command::Attack {
            attacker_id: rhino,
            target_id: tank,
        },
    );
    order(
        &mut sim,
        "Russians",
        Command::Move {
            entity_id: tank,
            target_rx: 4,
            target_ry: 13,
            queue: false,
        },
    );
    let frames = chase(&mut sim, &rules, rhino, tank);
    let reach = range(&rules, "120mm");
    let (stop, before) = frames.split_last().unwrap();
    assert!(stop.at_cell_centre(), "stopped mid-cell at {:?}", stop.at);
    assert!(stop.distance <= reach, "{stop:?}");
    let (x, y) = stop.cell();
    assert_ne!(
        before.last().unwrap().nav_com,
        Some(NavTargetRef::cell(x, y)),
        "arrived at its approach cell instead"
    );
    assert!(
        before
            .iter()
            .any(|f| !f.at_cell_centre() && f.distance <= reach),
        "the tank came in range between cell centres"
    );

    let mut rest = stop.at;
    for _ in 0..30 {
        step(&mut sim, &rules);
        rest = frame(&sim, rhino, tank).at;
    }
    let cell = ((rest.0 / 256) as u16, (rest.1 / 256) as u16);
    assert!(
        cell.0.abs_diff(x) <= 1 && cell.1.abs_diff(y) <= 1,
        "came to rest at {cell:?}, stopped at {:?}",
        (x, y)
    );
    let entity = sim.substrate.entities.get(rhino).unwrap();
    assert!(entity.attack_target.is_some());
}

/// A Rhino on Area Guard with a Target out of range approaches it the same
/// way, from the post the mission takes (`0x004D6F32`), and stops on entering
/// the first cell with the tank in range, inside its leash.
#[test]
fn an_area_guarding_rhino_approaches_its_target_and_stops_in_range() {
    let Some(rules) = retail_rules() else {
        return;
    };
    let mut sim = arena(&rules);
    let rhino = spawn(&mut sim, &rules, "HTNK", "Americans", (10, 10));
    let tank = spawn(&mut sim, &rules, "MTNK", "Russians", (18, 10));
    target(&mut sim, rhino, tank);
    start_mission(&mut sim, rhino, MissionType::AreaGuard);
    assert!(sim.mission_owns_approach(rhino, &rules));
    let frames = chase(&mut sim, &rules, rhino, tank);
    let reach = range(&rules, "120mm");
    let Some(NavTargetRef::Cell { rx, ry }) = frames[0].nav_com else {
        panic!("Approach sets a cell: {:?}", frames[0].nav_com);
    };
    assert_ne!((rx, ry), (18, 10));
    assert!(cell_centre_distance((rx, ry), &sim, tank) <= reach);
    let stop = frames.last().unwrap();
    assert!(stop.at_cell_centre(), "stopped mid-cell at {:?}", stop.at);
    assert!(stop.distance <= reach, "{stop:?}");
    assert_eq!(
        sim.substrate.entities.get(rhino).unwrap().mission.current(),
        MissionId::from_known(MissionType::AreaGuard)
    );
}

/// A Chrono Legionnaire on Area Guard takes the same Foot body through
/// Infantry `0x00522340` as on Attack: Approach picks a cell in range, not
/// the Target's own, and the pursuit pass leaves it alone.
#[test]
fn an_area_guarding_chrono_legionnaire_approaches_through_the_foot_body() {
    let Some(rules) = retail_rules() else {
        return;
    };
    let mut sim = arena(&rules);
    let legionnaire = spawn(&mut sim, &rules, "CLEG", "Americans", (6, 14));
    let tank = spawn(&mut sim, &rules, "MTNK", "Russians", (24, 14));
    target(&mut sim, legionnaire, tank);
    start_mission(&mut sim, legionnaire, MissionType::AreaGuard);
    assert!(sim.mission_owns_approach(legionnaire, &rules));
    sim.approach_target(
        legionnaire,
        &rules,
        None,
        crate::sim::world::FrameEffects::default(),
    );
    let nav_com = sim
        .substrate
        .entities
        .get(legionnaire)
        .unwrap()
        .navigation
        .nav_com;
    let Some(NavTargetRef::Cell { rx, ry }) = nav_com else {
        panic!("Approach sets a cell: {nav_com:?}");
    };
    assert_ne!((rx, ry), (24, 14));
}
