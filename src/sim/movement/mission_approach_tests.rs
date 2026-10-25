//! A vehicle's or infantryman's approach to what it attacks, through the
//! production frame: its mission's Approach (`FootClass::Approach_Target @
//! 0x004D5690`, reached through `UnitClass::Approach_Target @ 0x007414E0` or
//! `InfantryClass::Approach_Target @ 0x00522340`) picks a cell to fire from,
//! and the per-cell range stop (`0x004D882F..0x004D896E`) halts it on
//! entering a cell with a Foot Target in range. Expected behaviour comes from
//! instruction reading.

use crate::rules::ruleset::RuleSet;
use crate::sim::combat::TargetKind;
use crate::sim::command::{Command, CommandEnvelope};
use crate::sim::components::NavTargetRef;
use crate::sim::mission::leaf::MissionLeafState;
use crate::sim::mission::{MissionId, MissionType};
use crate::sim::movement::ground_pose::position_world_coord;
use crate::sim::movement::infantry_action::{DO_DEPLOY, DO_DEPLOYED, DO_DEPLOYED_IDLE};
use crate::sim::world::Simulation;

fn retail_rules() -> Option<RuleSet> {
    let art = crate::rules::retail_ini_fixture::retail_ini("artmd.ini")?;
    let mut rules = crate::rules::retail_ini_fixture::retail_unlayered_rules_with_art()?;
    // The app's ART sequence binding, without which an infantryman's
    // Do_Action refuses an action his sequence holds.
    rules.bind_animation_sequences(
        &crate::rules::infantry_sequence::parse_infantry_sequence_registry(&art),
    );
    Some(rules)
}

/// The flat arena with the computer `Russians` and the human `Americans`.
fn arena(rules: &RuleSet) -> Simulation {
    let mut sim = crate::sim::arena_fixture::rival_arena(rules, 0x4D5690);
    sim.intern_rule_type_ids(rules);
    sim.resolve_type_handles(rules);
    sim.set_input_delay_ticks(0);
    sim.session.set_binary_frame_for_test(1);
    sim
}

fn spawn(sim: &mut Simulation, rules: &RuleSet, kind: &str, owner: &str, cell: (u16, u16)) -> u64 {
    sim.spawn_object(kind, owner, cell.0, cell.1, 0, rules)
        .unwrap_or_else(|| panic!("{kind} spawns"))
}

fn start_mission(sim: &mut Simulation, id: u64, mission: MissionType) {
    let now = sim.session.binary_frame();
    sim.substrate
        .mission_assign_exact(id, MissionId::from_known(mission), now)
        .unwrap();
    let _ = sim.substrate.mission_commence_exact(id, now);
}

fn target(sim: &mut Simulation, id: u64, target: u64) {
    sim.substrate
        .entities
        .get_mut(id)
        .unwrap()
        .attack_target
        .set_for_test(Some(TargetKind::Entity(target)));
}

fn order(sim: &mut Simulation, owner: &str, command: Command) {
    let owner = sim.interner.intern(owner);
    let tick = sim.session.tick() + 1;
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

    /// At one of an infantryman's five spots in a cell, where Walk completes
    /// a step.
    fn at_spot(&self) -> bool {
        let at = (self.at.0.rem_euclid(256), self.at.1.rem_euclid(256));
        crate::util::lepton::SUBCELL_OFFSETS.contains(&at)
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
            .nav_com(),
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
    // Placement counts the Center's drain at once (0x00502B48..0x00502B52)
    // but the plant's output only at the House's first power assessment, so
    // the Center first counts as powered in the second frame. Its
    // RobotTanksBackOnline (0x0050E010) Reactivates the tank, which resets
    // its orders (0x0070FBE0): order the tank after that.
    let americans = sim.interner.get("Americans").unwrap();
    for _ in 0..10 {
        if sim.has_powered_center(americans) {
            break;
        }
        step(&mut sim, &rules);
    }
    assert!(sim.has_powered_center(americans), "the Center powers up");
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
    // Synthetic setup uses the registered human sender without changing
    // the Russian actor's AI flags or the order's frame-tail timing.
    order(
        &mut sim,
        "Americans",
        Command::Move {
            entity_id: tank,
            target_rx: 4,
            target_ry: 13,
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
    assert!(entity.attack_target.target().is_some());
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
    assert!(sim.owns_foot_approach(rhino, &rules));
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
/// the Target's own.
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
    assert!(sim.owns_foot_approach(legionnaire, &rules));
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
        .nav_com();
    let Some(NavTargetRef::Cell { rx, ry }) = nav_com else {
        panic!("Approach sets a cell: {nav_com:?}");
    };
    assert_ne!((rx, ry), (24, 14));
}

/// A human's GI ordered to attack a tank walks to a cell his Approach picks
/// (Infantry `0x00522340` into the Foot body), not the tank's own cell. He
/// keeps that NavCom through the frames where the tank first comes in range
/// between spots, and lets it go on completing his step into the first cell
/// from which the tank is in range, at his spot there.
#[test]
fn a_gi_stops_on_entering_the_first_cell_with_its_target_in_range() {
    let Some(rules) = retail_rules() else {
        return;
    };
    let mut sim = arena(&rules);
    let gi = spawn(&mut sim, &rules, "E1", "Americans", (4, 10));
    let tank = spawn(&mut sim, &rules, "MTNK", "Russians", (24, 10));
    order(
        &mut sim,
        "Americans",
        Command::Attack {
            attacker_id: gi,
            target_id: tank,
        },
    );
    let frames = chase(&mut sim, &rules, gi, tank);
    let reach = range(&rules, "M60");
    let Some(NavTargetRef::Cell { rx, ry }) = frames[0].nav_com else {
        panic!("Approach sets a cell: {:?}", frames[0].nav_com);
    };
    assert_ne!((rx, ry), (24, 10));
    assert!(cell_centre_distance((rx, ry), &sim, tank) <= reach);
    let (stop, before) = frames.split_last().unwrap();
    assert!(stop.at_spot(), "stopped between spots at {:?}", stop.at);
    assert!(stop.distance <= reach, "{stop:?}");
    assert!(
        before.iter().any(|f| !f.at_spot() && f.distance <= reach),
        "the tank came in range between spots"
    );
    let last_spot = before.iter().rev().find(|f| f.at_spot() && f.at != stop.at);
    assert!(
        last_spot.is_none_or(|f| f.distance > reach),
        "{last_spot:?}"
    );
}

/// A human's Rocketeer ordered to attack a tank flies to a cell his Approach
/// picks within his 20mm's range (Infantry `0x00522340` into the Foot body,
/// where a Jumpjet takes the first admitted candidate, `0x004D6764`), never
/// the tank's own cell, and shoots the tank.
#[test]
fn a_rocketeer_flies_to_a_cell_in_range_of_the_tank_he_attacks() {
    let Some(rules) = retail_rules() else {
        return;
    };
    let mut sim = arena(&rules);
    let rocketeer = spawn(&mut sim, &rules, "JUMPJET", "Americans", (4, 10));
    let tank = spawn(&mut sim, &rules, "MTNK", "Russians", (24, 10));
    let strength = sim.substrate.entities.get(tank).unwrap().health.current();
    order(
        &mut sim,
        "Americans",
        Command::Attack {
            attacker_id: rocketeer,
            target_id: tank,
        },
    );
    let reach = range(&rules, "20mm");
    let mut first = None;
    let mut shot = None;
    for _ in 0..900 {
        step(&mut sim, &rules);
        let now = frame(&sim, rocketeer, tank);
        if let Some(nav_com) = now.nav_com {
            assert_ne!(nav_com, NavTargetRef::cell(24, 10), "{now:?}");
            first.get_or_insert(nav_com);
        }
        if sim.substrate.entities.get(tank).unwrap().health.current() < strength {
            shot = Some(now);
            break;
        }
    }
    assert!(sim.owns_foot_approach(rocketeer, &rules));
    let Some(NavTargetRef::Cell { rx, ry }) = first else {
        panic!("Approach sets a cell: {first:?}");
    };
    assert!(cell_centre_distance((rx, ry), &sim, tank) <= reach);
    let shot = shot.expect("the Rocketeer shoots the tank");
    assert!(shot.distance <= reach, "{shot:?}");
}

/// A Night Hawk's Approach (`0x007414E0`, no BalloonHover) runs the Foot body
/// too, which picks a cell in its cannon's range, not the target's own.
#[test]
fn a_night_hawks_approach_picks_a_cell_in_range() {
    let Some(rules) = retail_rules() else {
        return;
    };
    let mut sim = arena(&rules);
    let hawk = spawn(&mut sim, &rules, "SHAD", "Americans", (6, 14));
    let tank = spawn(&mut sim, &rules, "MTNK", "Russians", (24, 14));
    target(&mut sim, hawk, tank);
    start_mission(&mut sim, hawk, MissionType::Attack);
    assert!(sim.owns_foot_approach(hawk, &rules));
    sim.approach_target(
        hawk,
        &rules,
        None,
        crate::sim::world::FrameEffects::default(),
    );
    let nav_com = sim
        .substrate
        .entities
        .get(hawk)
        .unwrap()
        .navigation
        .nav_com();
    let Some(NavTargetRef::Cell { rx, ry }) = nav_com else {
        panic!("Approach sets a cell: {nav_com:?}");
    };
    assert_ne!((rx, ry), (24, 14));
    assert!(cell_centre_distance((rx, ry), &sim, tank) <= range(&rules, "BlackHawkCannon"));
}

/// A GI attacking a power plant walks to the cell his Approach picks and
/// stops there on arrival: the per-cell range stop measures only a Foot
/// Target.
#[test]
fn a_gi_walks_to_his_approach_cell_to_shoot_a_building() {
    let Some(rules) = retail_rules() else {
        return;
    };
    let mut sim = arena(&rules);
    let gi = spawn(&mut sim, &rules, "E1", "Americans", (4, 10));
    let plant = spawn(&mut sim, &rules, "NAPOWR", "Russians", (24, 10));
    order(
        &mut sim,
        "Americans",
        Command::Attack {
            attacker_id: gi,
            target_id: plant,
        },
    );
    let frames = chase(&mut sim, &rules, gi, plant);
    let Some(NavTargetRef::Cell { rx, ry }) = frames[0].nav_com else {
        panic!("Approach sets a cell: {:?}", frames[0].nav_com);
    };
    assert!(!(24..=25).contains(&rx) || !(10..=12).contains(&ry));
    let (stop, before) = frames.split_last().unwrap();
    let (x, y) = stop.cell();
    assert_eq!(
        before.last().unwrap().nav_com,
        Some(NavTargetRef::cell(x, y)),
        "stopped short of his approach cell"
    );
}

/// A computer's Conscript walking elsewhere when put on Hunt drops that
/// destination at the first dispatch his scan finds a target
/// (`Set_Destination(NULL, 1)`, `0x004D54C6`) and approaches his quarry
/// (`0x004D54D2`) to a cell in range, where he comes to rest.
#[test]
fn a_hunting_conscript_drops_his_walk_to_approach_his_quarry() {
    let Some(rules) = retail_rules() else {
        return;
    };
    let mut sim = arena(&rules);
    let conscript = spawn(&mut sim, &rules, "E2", "Russians", (4, 10));
    let tank = spawn(&mut sim, &rules, "MTNK", "Americans", (24, 10));
    // Synthetic setup uses the registered human sender without changing
    // the Russian actor's AI flags or the order's frame-tail timing.
    order(
        &mut sim,
        "Americans",
        Command::Move {
            entity_id: conscript,
            target_rx: 4,
            target_ry: 20,
        },
    );
    let walk = Some(NavTargetRef::cell(4, 20));
    for _ in 0..3 {
        step(&mut sim, &rules);
    }
    assert_eq!(frame(&sim, conscript, tank).nav_com, walk);
    start_mission(&mut sim, conscript, MissionType::Hunt);
    let mut approach = None;
    for _ in 0..30 {
        step(&mut sim, &rules);
        let nav_com = frame(&sim, conscript, tank).nav_com;
        if nav_com != walk {
            approach = nav_com;
            break;
        }
    }
    let reach = range(&rules, "M1Carbine");
    let Some(NavTargetRef::Cell { rx, ry }) = approach else {
        panic!("Hunt kept his walk or set no cell: {approach:?}");
    };
    assert_ne!((rx, ry), (24, 10));
    assert!(cell_centre_distance((rx, ry), &sim, tank) <= reach);

    let mut rest = frame(&sim, conscript, tank);
    for _ in 0..900 {
        step(&mut sim, &rules);
        let now = frame(&sim, conscript, tank);
        if now.nav_com.is_none() && now.at == rest.at {
            break;
        }
        rest = now;
    }
    assert!(rest.nav_com.is_none(), "still walking: {rest:?}");
    assert!(rest.distance <= reach, "{rest:?}");
    assert_eq!(
        sim.substrate
            .entities
            .get(conscript)
            .unwrap()
            .mission
            .current(),
        MissionId::from_known(MissionType::Hunt)
    );
}

/// A GI on Area Guard with a Target out of range approaches it the same way
/// and stops on completing his step into the first cell with the tank in
/// range.
#[test]
fn an_area_guarding_gi_approaches_his_target_and_stops_in_range() {
    let Some(rules) = retail_rules() else {
        return;
    };
    let mut sim = arena(&rules);
    let gi = spawn(&mut sim, &rules, "E1", "Americans", (10, 10));
    let tank = spawn(&mut sim, &rules, "MTNK", "Russians", (17, 10));
    target(&mut sim, gi, tank);
    start_mission(&mut sim, gi, MissionType::AreaGuard);
    assert!(sim.owns_foot_approach(gi, &rules));
    let frames = chase(&mut sim, &rules, gi, tank);
    let reach = range(&rules, "M60");
    let Some(NavTargetRef::Cell { rx, ry }) = frames[0].nav_com else {
        panic!("Approach sets a cell: {:?}", frames[0].nav_com);
    };
    assert_ne!((rx, ry), (17, 10));
    let stop = frames.last().unwrap();
    assert!(stop.at_spot(), "stopped between spots at {:?}", stop.at);
    assert!(stop.distance <= reach, "{stop:?}");
    assert_eq!(
        sim.substrate.entities.get(gi).unwrap().mission.current(),
        MissionId::from_known(MissionType::AreaGuard)
    );
}

/// When a vehicle stands on the cell his Approach would pick, a GI goes on
/// round the ring to the next candidate (`0x004D642D..0x004D6437`). He does
/// not try the taken cell's neighbours, which a vehicle walks starting north
/// (`cell_delta` 0). Here the ring's offsets up to 8 stay in the taken cell,
/// and offset 16, 22.5 degrees round, reaches the cell south of it.
#[test]
fn a_gi_passes_over_a_taken_approach_cell_without_trying_its_neighbours() {
    let Some(rules) = retail_rules() else {
        return;
    };
    let approach_cell = |taken: Option<(u16, u16)>| {
        let mut sim = arena(&rules);
        let gi = spawn(&mut sim, &rules, "E1", "Americans", (28, 10));
        let tank = spawn(&mut sim, &rules, "MTNK", "Russians", (18, 10));
        if let Some(cell) = taken {
            spawn(&mut sim, &rules, "MTNK", "Americans", cell);
        }
        target(&mut sim, gi, tank);
        start_mission(&mut sim, gi, MissionType::Attack);
        sim.approach_target(gi, &rules, None, crate::sim::world::FrameEffects::default());
        match sim.substrate.entities.get(gi).unwrap().navigation.nav_com() {
            Some(NavTargetRef::Cell { rx, ry }) => (rx, ry),
            other => panic!("Approach sets a cell: {other:?}"),
        }
    };
    let open = approach_cell(None);
    let next = approach_cell(Some(open));
    assert_ne!(next, (open.0, open.1 - 1), "took the neighbour walk");
    assert_eq!(next, (open.0, open.1 + 1));
}

/// A GI ordered to fire on a cell walks to a cell his Approach picks in range
/// of it, not to the cell itself, and stops on arriving there: the per-cell
/// range stop measures only a Foot Target.
#[test]
fn a_gi_ordered_to_fire_on_a_cell_walks_to_his_approach_cell() {
    let Some(rules) = retail_rules() else {
        return;
    };
    let mut sim = arena(&rules);
    let gi = spawn(&mut sim, &rules, "E1", "Americans", (4, 10));
    order(
        &mut sim,
        "Americans",
        Command::ForceAttackCell {
            attacker_id: gi,
            target_rx: 24,
            target_ry: 10,
        },
    );
    let (mut first, mut last) = (None, None);
    for _ in 0..900 {
        step(&mut sim, &rules);
        let nav_com = sim.substrate.entities.get(gi).unwrap().navigation.nav_com();
        if nav_com.is_some() {
            first = first.or(nav_com);
            last = nav_com;
        } else if first.is_some() {
            break;
        }
    }
    let Some(NavTargetRef::Cell { rx, ry }) = first else {
        panic!("Approach sets a cell: {first:?}");
    };
    assert_ne!((rx, ry), (24, 10));
    let at = position_world_coord(&sim.substrate.entities.get(gi).unwrap().position);
    let cell = ((at.x / 256) as u16, (at.y / 256) as u16);
    assert_eq!(
        last,
        Some(NavTargetRef::cell(cell.0, cell.1)),
        "stopped short of his approach cell"
    );
}

/// A computer's deployed GI on Attack whose Target is beyond his deployed
/// weapon's reach undeploys (Infantry Approach `0x0052238E..0x0052242D`:
/// Do_Action(0x1F, 0, 0)) and then approaches it like any other GI. Within
/// reach he stays deployed. A human's deployed man is left as he is
/// (`0x00522388`).
#[test]
fn a_computers_deployed_gi_undeploys_to_approach_a_target_out_of_reach() {
    let Some(rules) = retail_rules() else {
        return;
    };
    let deployed_gi = |owner: &str, rival: &str, tank_cell: (u16, u16)| {
        let mut sim = arena(&rules);
        let gi = spawn(&mut sim, &rules, "E1", owner, (10, 10));
        let tank = spawn(&mut sim, &rules, "MTNK", rival, tank_cell);
        sim.substrate.entities.get_mut(gi).unwrap().mission_leaf =
            MissionLeafState::infantry_raw_for_test(0, DO_DEPLOYED);
        target(&mut sim, gi, tank);
        start_mission(&mut sim, gi, MissionType::Attack);
        (sim, gi)
    };
    let deployed = |sim: &Simulation, gi: u64| {
        let entity = sim.substrate.entities.get(gi).unwrap();
        (DO_DEPLOY..=DO_DEPLOYED_IDLE).contains(&entity.mission_leaf.as_infantry().unwrap().doing())
    };

    let (mut sim, gi) = deployed_gi("Americans", "Russians", (24, 10));
    sim.approach_target(gi, &rules, None, crate::sim::world::FrameEffects::default());
    assert!(deployed(&sim, gi), "a human's man stood up");

    for (tank_cell, approaches) in [((24, 10), true), ((13, 10), false)] {
        let (mut sim, gi) = deployed_gi("Russians", "Americans", tank_cell);
        let (mut stood_up, mut nav_com) = (false, None);
        for _ in 0..120 {
            step(&mut sim, &rules);
            stood_up |= !deployed(&sim, gi);
            nav_com = nav_com.or(sim.substrate.entities.get(gi).unwrap().navigation.nav_com());
        }
        assert_eq!(stood_up, approaches, "against {tank_cell:?}");
        match nav_com {
            Some(NavTargetRef::Cell { rx, ry }) if approaches => {
                assert_ne!((rx, ry), tank_cell);
            }
            None if !approaches => {}
            other => panic!("against {tank_cell:?}: NavCom {other:?}"),
        }
    }
}

/// The spot an infantryman stands on in his cell, in leptons from its origin.
fn spot(sim: &Simulation, id: u64) -> (i32, i32) {
    let at = position_world_coord(&sim.substrate.entities.get(id).unwrap().position);
    (at.x.rem_euclid(256), at.y.rem_euclid(256))
}

/// A Brute beside a tank he cannot yet reach (`CloseRange=`, Smash's 1.1
/// cells) takes the one free spot in his own cell, which lies nearer the
/// tank's cell centre than his cell centre does (`0x004D5D37..0x004D5F62`):
/// his Walk head is that spot (Force_Immediate_Destination, `0x0075AE30`),
/// his NavCom the tank's own cell without a search (`0x004D689A`), and the
/// arm's one draw is PlaceInfantryInCell's centre row (`0x00481180`), the
/// tank standing at its cell centre.
#[test]
fn a_brute_beside_a_tank_takes_the_spot_in_his_cell_nearer_it() {
    let Some(rules) = retail_rules() else {
        return;
    };
    let mut sim = arena(&rules);
    let gi = spawn(&mut sim, &rules, "E1", "Americans", (10, 10));
    let brute = spawn(&mut sim, &rules, "BRUTE", "Americans", (10, 10));
    let taken = [spot(&sim, gi), spot(&sim, brute)];
    let free = [(192, 64), (64, 192), (192, 192)]
        .into_iter()
        .find(|spot| !taken.contains(spot))
        .unwrap();
    // The neighbour that spot is nearer and the Brute is out of reach of,
    // wherever the spawn put him.
    let tank_cell = match free {
        (192, 64) => (10, 9),
        (64, 192) => (9, 10),
        _ => (11, 11),
    };
    let tank = spawn(&mut sim, &rules, "MTNK", "Russians", tank_cell);
    target(&mut sim, brute, tank);
    start_mission(&mut sim, brute, MissionType::Attack);
    assert!(frame(&sim, brute, tank).distance > range(&rules, "Smash"));
    assert!(sim.owns_foot_approach(brute, &rules));
    let mut one_draw = sim.scenario_rng.clone();
    one_draw.next_range_u32(4);
    sim.approach_target(
        brute,
        &rules,
        None,
        crate::sim::world::FrameEffects::default(),
    );
    assert_eq!(sim.scenario_rng.logical_state(), one_draw.logical_state());
    let entity = sim.substrate.entities.get(brute).unwrap();
    let head = entity.locomotor.as_ref().unwrap().step_head();
    assert_eq!(
        head.map(|head| (head.x, head.y)),
        Some((10 * 256 + free.0, 10 * 256 + free.1))
    );
    assert_eq!(
        entity.navigation.nav_com(),
        Some(NavTargetRef::cell(tank_cell.0, tank_cell.1))
    );
    // Through the frame he steps onto that spot and smashes the tank from it.
    let strength = sim.substrate.entities.get(tank).unwrap().health.current();
    for _ in 0..120 {
        step(&mut sim, &rules);
        if sim.substrate.entities.get(tank).unwrap().health.current() < strength {
            assert_eq!(frame(&sim, brute, tank).cell(), (10, 10));
            assert_eq!(spot(&sim, brute), free);
            return;
        }
    }
    panic!("the Brute never smashed the tank");
}

/// Each candidate of a Brute's search is the spot PlaceInfantryInCell picks
/// in the candidate's cell for the tank's coordinate
/// (`0x004D6093..0x004D60F9`): one Scenario draw each, the tank standing at
/// its cell centre, whether the candidate is taken or not. Here none is. In
/// the cell beside the tank towards the Brute every spot lies beyond Smash's
/// reach and 307.2 leptons of the tank (the spot nearest it, (64, 64), is no
/// infantryman's), and a GI stands in each other cell his ring reaches. All
/// 25 candidates draw.
#[test]
fn a_brutes_search_draws_once_for_each_candidate() {
    let Some(rules) = retail_rules() else {
        return;
    };
    let mut sim = arena(&rules);
    let tank = spawn(&mut sim, &rules, "MTNK", "Russians", (16, 16));
    for cell in [(17, 16), (16, 17), (17, 15), (15, 17)] {
        spawn(&mut sim, &rules, "E1", "Americans", cell);
    }
    let brute = spawn(&mut sim, &rules, "BRUTE", "Americans", (21, 21));
    target(&mut sim, brute, tank);
    start_mission(&mut sim, brute, MissionType::Attack);
    let mut draws = sim.scenario_rng.clone();
    for _ in 0..25 {
        draws.next_range_u32(4);
    }
    sim.approach_target(
        brute,
        &rules,
        None,
        crate::sim::world::FrameEffects::default(),
    );
    let nav_com = sim
        .substrate
        .entities
        .get(brute)
        .unwrap()
        .navigation
        .nav_com();
    assert_eq!(
        sim.scenario_rng.logical_state(),
        draws.logical_state(),
        "{nav_com:?}"
    );
}

/// A Brute's candidate spot beyond Smash's reach is still taken when it lies
/// under 307.2 leptons from the tank (`0x004D622C..0x004D625C`). The tank has
/// driven off its cell centre into the south-west quarter, so each spot
/// request prefers spot 3, (64, 192), without a draw: in the cell west of the
/// tank that spot lies 302 leptons from it.
#[test]
fn a_brute_takes_a_spot_beyond_his_reach_under_307_leptons_from_the_tank() {
    let Some(rules) = retail_rules() else {
        return;
    };
    let mut sim = arena(&rules);
    let tank = spawn(&mut sim, &rules, "MTNK", "Russians", (17, 16));
    let position = &mut sim.substrate.entities.get_mut(tank).unwrap().position;
    position.set_sub_x_for_test(crate::util::fixed_math::SimFixed::from_num(110));
    position.set_sub_y_for_test(crate::util::fixed_math::SimFixed::from_num(192));
    let brute = spawn(&mut sim, &rules, "BRUTE", "Americans", (11, 16));
    target(&mut sim, brute, tank);
    start_mission(&mut sim, brute, MissionType::Attack);
    let aim = position_world_coord(&sim.substrate.entities.get(tank).unwrap().position);
    let reach = f64::from(16 * 256 + 64 - aim.x).hypot(f64::from(16 * 256 + 192 - aim.y));
    assert!(reach > range(&rules, "Smash") && reach < 307.2, "{reach}");
    let before = sim.scenario_rng.logical_state();
    sim.approach_target(
        brute,
        &rules,
        None,
        crate::sim::world::FrameEffects::default(),
    );
    assert_eq!(sim.scenario_rng.logical_state(), before);
    assert_eq!(
        sim.substrate
            .entities
            .get(brute)
            .unwrap()
            .navigation
            .nav_com(),
        Some(NavTargetRef::cell(16, 16))
    );
}

/// A Brute far from the tank he attacks searches its neighbours (`CloseRange=`
/// infantry search no closer than 332 leptons, `0x004D5CFE..0x004D5D2F`):
/// his NavCom is a cell beside the tank, never its own. Each candidate's
/// admission picks him up and puts him down again (Mark(UP) and Mark(DOWN),
/// `0x004D62C0..0x004D637D`), which puts him back at the head of his cell's
/// list, ahead of a GI who came after him.
#[test]
fn a_brutes_approach_picks_a_cell_beside_the_tank() {
    let Some(rules) = retail_rules() else {
        return;
    };
    let mut sim = arena(&rules);
    let brute = spawn(&mut sim, &rules, "BRUTE", "Americans", (6, 10));
    let gi = spawn(&mut sim, &rules, "E1", "Americans", (6, 10));
    let tank = spawn(&mut sim, &rules, "MTNK", "Russians", (16, 10));
    target(&mut sim, brute, tank);
    start_mission(&mut sim, brute, MissionType::Attack);
    assert!(sim.owns_foot_approach(brute, &rules));
    let first = |sim: &Simulation| {
        sim.substrate
            .occupancy
            .get(6, 10)
            .and_then(|cell| {
                cell.iter_layer(crate::sim::movement::locomotor::MovementLayer::Ground)
                    .next()
            })
            .map(|occupant| occupant.entity_id)
    };
    assert_eq!(first(&sim), Some(gi));
    sim.approach_target(
        brute,
        &rules,
        None,
        crate::sim::world::FrameEffects::default(),
    );
    assert_eq!(first(&sim), Some(brute));
    let nav_com = sim
        .substrate
        .entities
        .get(brute)
        .unwrap()
        .navigation
        .nav_com();
    let Some(NavTargetRef::Cell { rx, ry }) = nav_com else {
        panic!("Approach sets a cell: {nav_com:?}");
    };
    assert_ne!((rx, ry), (16, 10));
    assert!(rx.abs_diff(16) <= 1 && ry.abs_diff(10) <= 1, "{nav_com:?}");
}

/// A human's Brute ordered to attack a tank sets out for a cell beside it,
/// not the tank's own cell, never makes the tank itself his destination, and
/// smashes it once in reach.
#[test]
fn a_brute_ordered_to_attack_a_tank_walks_beside_it_and_smashes_it() {
    let Some(rules) = retail_rules() else {
        return;
    };
    let mut sim = arena(&rules);
    let brute = spawn(&mut sim, &rules, "BRUTE", "Americans", (6, 10));
    let tank = spawn(&mut sim, &rules, "MTNK", "Russians", (16, 10));
    let strength = sim.substrate.entities.get(tank).unwrap().health.current();
    order(
        &mut sim,
        "Americans",
        Command::Attack {
            attacker_id: brute,
            target_id: tank,
        },
    );
    let mut first = None;
    let mut smashed = None;
    for _ in 0..900 {
        step(&mut sim, &rules);
        let now = frame(&sim, brute, tank);
        if let Some(nav_com) = now.nav_com {
            assert_ne!(nav_com, NavTargetRef::object(tank), "{now:?}");
            first.get_or_insert(nav_com);
        }
        if sim.substrate.entities.get(tank).unwrap().health.current() < strength {
            smashed = Some(now);
            break;
        }
    }
    let Some(NavTargetRef::Cell { rx, ry }) = first else {
        panic!("Approach sets a cell: {first:?}");
    };
    assert_ne!((rx, ry), (16, 10));
    assert!(rx.abs_diff(16) <= 1 && ry.abs_diff(10) <= 1, "{first:?}");
    let smashed = smashed.expect("the Brute smashes the tank");
    assert!(smashed.distance <= range(&rules, "Smash"), "{smashed:?}");
}

/// Production Hunt must carry a stock Kirov through approach and bomb release.
/// A stationary power plant keeps target motion out of this regression.
#[test]
fn a_hunting_kirov_closes_on_a_building_and_drops_bombs() {
    let Some(rules) = retail_rules() else {
        return;
    };
    let mut sim = arena(&rules);
    spawn(&mut sim, &rules, "NAPOWR", "Russians", (2, 24));
    let victim = spawn(&mut sim, &rules, "GAPOWR", "Americans", (18, 10));
    let kirov = spawn(&mut sim, &rules, "ZEP", "Russians", (4, 10));
    let goal = crate::sim::movement::ground_pose::object_get_coords(
        sim.substrate.entities.get(victim).unwrap(),
        sim.resolved_terrain.as_ref(),
    );
    let health = sim.substrate.entities.get(victim).unwrap().health.current();
    start_mission(&mut sim, kirov, MissionType::Hunt);
    let mut closest = i32::MAX;
    let mut fired = false;
    let mut seen = Vec::new();
    let mut last = None;
    for visit in 0..900 {
        step(&mut sim, &rules);
        let actor = sim.substrate.entities.get(kirov).expect("Kirov lives");
        assert_eq!(actor.mission.current().known(), Some(MissionType::Hunt));
        let at = position_world_coord(&actor.position);
        let distance =
            crate::util::native_x87::distance_3d_leptons([at.x, at.y, 0], [goal.x, goal.y, 0]);
        closest = closest.min(distance);
        fired |= actor.rearm_timer.duration() > 0;
        let state = (actor.attack_target.target(), actor.navigation.nav_com());
        if Some(state) != last {
            seen.push((visit, state, distance, at));
            last = Some(state);
        }
        if !sim.substrate.entities.contains(victim)
            || sim.substrate.entities.get(victim).unwrap().health.current() < health
        {
            assert!(fired, "bomb damage must follow a committed shot");
            return;
        }
    }
    panic!(
        "Hunt never bombed the stationary plant: closest={closest}, fired={fired}, transitions={seen:?}"
    );
}
