//! Production checks of a Jumpjet infantryman's airborne actions
//! (`sim::movement::infantry_action`) on a retail map.

use crate::sim::animation::SequenceKind;
use crate::sim::movement::infantry_action::{DO_FIRE_FLY, DO_FLY, DO_HOVER};

/// One frame of a Rocketeer: its locomotor state, speed fraction (Foot
/// `+0x578`), Doing and displayed sequence.
#[derive(Debug, Clone, Copy)]
struct Pose {
    phase: i32,
    fraction: i32,
    doing: i32,
    sequence: Option<SequenceKind>,
    altitude: i32,
}

fn pose(sim: &super::Simulation, id: u64) -> Pose {
    let entity = sim.substrate.entities.get(id).expect("rocketeer");
    let locomotor = entity.locomotor.as_ref().expect("locomotor");
    Pose {
        phase: locomotor.jumpjet_runtime().expect("Jumpjet").phase,
        fraction: entity.foot_speed.applied_fraction.to_bits(),
        doing: entity.mission_leaf.as_infantry().expect("Infantry").doing(),
        sequence: entity
            .animation
            .as_ref()
            .map(|animation| animation.sequence),
        altitude: locomotor.altitude.to_num(),
    }
}

/// A Rocketeer on Dustbowl flies to a cell eight cells away and holds there,
/// then is ordered to attack a conscript three cells off. At cruise speed it
/// takes Fly, in the hold Hover and at each shot FireFly, whose sequence it
/// shows. Every shot leaves it at a speed fraction of a tenth or less (the
/// Infantry fire error's I4). Before this chain, VERA showed its standing
/// Ready frame in the air, and it fired at full speed.
#[test]
#[ignore = "requires a retail RA2/YR install (RA2_DIR or config.toml)"]
fn retail_dustbowl_rocketeer_flies_hovers_and_fires_in_its_airborne_poses() {
    use crate::sim::command::{Command, CommandEnvelope};
    use crate::sim::house_state::HouseState;

    let dir = std::env::var("RA2_DIR")
        .ok()
        .filter(|path| !path.trim().is_empty())
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| {
            crate::util::config::GameConfig::load()
                .expect("set RA2_DIR or provide config.toml for this ignored test")
                .paths
                .ra2_dir
        });
    let mut scenario =
        crate::headless_scenario::load(&dir, "Dustbowl.mmx", 0x00C0_FFEE).expect("Dustbowl loads");
    let crate::sim::runtime::SimRuntime {
        simulation: sim,
        resources,
    } = &mut scenario.runtime;
    for (name, side, human) in [("Americans", 0, true), ("Russians", 1, false)] {
        let house = sim.interner.intern(name);
        sim.houses
            .entry(house)
            .or_insert_with(|| HouseState::new(house, side, None, human, 10_000, 10));
        if !sim.session.house_order.contains(&house) {
            sim.session.house_order.push(house);
        }
    }
    // Open level ground: the Rocketeer lifts off at x and holds at x + 8.
    // Each side keeps a power plant out of the fight, so neither house is
    // defeated under the Battle mode's ShortGame.
    let (rocketeer, x, y) = (40..100_u16)
        .flat_map(|y| (40..100_u16).map(move |x| (x, y)))
        .find_map(|(x, y)| {
            let grid = sim.path_grid()?;
            let terrain = sim.resolved_terrain.as_ref()?;
            let level = terrain.cell(x, y)?.level;
            let open = (x.checked_sub(4)?..=x + 16).all(|cx| {
                (y - 1..=y + 1).all(|cy| {
                    terrain.cell(cx, cy).is_some_and(|cell| cell.level == level)
                        && grid.cell(cx, cy).is_some_and(|cell| cell.ground_walkable)
                })
            });
            if !open {
                return None;
            }
            for (plant, owner, px) in [
                ("GAPOWR", "Americans", x - 4),
                ("NAPOWR", "Russians", x + 15),
            ] {
                sim.spawn_object(
                    plant,
                    owner,
                    px,
                    y - 1,
                    0,
                    &resources.rules,
                    &resources.height_map,
                )?;
            }
            let rocketeer = sim.spawn_object(
                "JUMPJET",
                "Americans",
                x,
                y,
                64,
                &resources.rules,
                &resources.height_map,
            )?;
            Some((rocketeer, x, y))
        })
        .expect("open level ground for the flight");
    sim.resolve_type_handles(&resources.rules);
    let americans = sim.interner.intern("Americans");
    let order = CommandEnvelope::new(
        americans,
        sim.session.tick + 1,
        Command::Move {
            entity_id: rocketeer,
            target_rx: x + 8,
            target_ry: y,
            queue: false,
            group_id: None,
        },
    );
    let mut orders = vec![order];
    let mut flight = Vec::new();
    for _ in 0..240 {
        scenario
            .runtime
            .advance_frame(
                &std::mem::take(&mut orders),
                crate::headless_scenario::SIM_TICK_MS,
                super::TickLane::Ordinary,
            )
            .expect("flight frame");
        flight.push(pose(&scenario.runtime.simulation, rocketeer));
    }
    let airborne = |pose: &Pose| pose.altitude > 0;
    // Cruising at full speed it flies; the frame's sequence is its Doing's.
    let cruise: Vec<_> = flight
        .iter()
        .filter(|pose| pose.phase == 3 && pose.fraction > 52_428)
        .collect();
    assert!(
        !cruise.is_empty(),
        "the flight reached cruise speed: {flight:?}"
    );
    for pose in &cruise {
        assert_eq!(pose.doing, DO_FLY, "{pose:?}");
        assert_eq!(pose.sequence, Some(SequenceKind::Fly), "{pose:?}");
    }
    // Holding over the ordered cell it hovers.
    let held = flight.last().expect("flight frames");
    assert_eq!(held.phase, 2, "{flight:?}");
    assert_eq!(held.doing, DO_HOVER, "{held:?}");
    assert_eq!(held.sequence, Some(SequenceKind::Hover), "{held:?}");
    // No airborne frame shows the standing or walking pose.
    for pose in flight.iter().filter(|pose| airborne(pose)) {
        assert!(
            matches!(pose.sequence, Some(SequenceKind::Fly | SequenceKind::Hover)),
            "airborne pose {pose:?}"
        );
    }

    // Ordered to attack a conscript standing three cells from its hold, it
    // shoots from the hover.
    let crate::sim::runtime::SimRuntime {
        simulation: sim,
        resources,
    } = &mut scenario.runtime;
    let conscript = sim
        .spawn_object(
            "E2",
            "Russians",
            x + 11,
            y,
            192,
            &resources.rules,
            &resources.height_map,
        )
        .expect("conscript");
    sim.resolve_type_handles(&resources.rules);
    let attack = CommandEnvelope::new(
        americans,
        sim.session.tick + 1,
        Command::Attack {
            attacker_id: rocketeer,
            target_id: conscript,
        },
    );
    let mut orders = vec![attack];
    let rearm_start = |sim: &super::Simulation| {
        sim.substrate
            .entities
            .get(rocketeer)
            .map(|entity| entity.rearm_timer)
    };
    let mut last_rearm = rearm_start(&scenario.runtime.simulation);
    let mut shots = Vec::new();
    let mut fight = Vec::new();
    for _ in 0..300 {
        scenario
            .runtime
            .advance_frame(
                &std::mem::take(&mut orders),
                crate::headless_scenario::SIM_TICK_MS,
                super::TickLane::Ordinary,
            )
            .expect("fight frame");
        let sim = &scenario.runtime.simulation;
        let now = pose(sim, rocketeer);
        fight.push(now);
        if now.doing == DO_FIRE_FLY {
            assert_eq!(now.sequence, Some(SequenceKind::FireFly), "{now:?}");
        }
        // A discharge restarts the Rocketeer's rearm timer.
        let rearm = rearm_start(sim);
        if rearm != last_rearm {
            shots.push(now);
            last_rearm = rearm;
        }
        if !sim.substrate.entities.contains(conscript) {
            break;
        }
    }
    assert!(
        shots.len() >= 2,
        "the Rocketeer shot the conscript: {fight:?}"
    );
    for shot in &shots {
        // Fired in FireFly, at its FireUp frame, and (I4 `0x0051C9B8`) at
        // no more than a tenth of full speed.
        assert_eq!(shot.doing, DO_FIRE_FLY, "{shot:?}");
        assert_eq!(shot.sequence, Some(SequenceKind::FireFly), "{shot:?}");
        assert!(shot.fraction <= 6553, "{shot:?}");
    }
    let after = pose(&scenario.runtime.simulation, rocketeer);
    assert!(
        matches!(after.doing, DO_HOVER | DO_FIRE_FLY),
        "back to its hover: {after:?}"
    );
}
