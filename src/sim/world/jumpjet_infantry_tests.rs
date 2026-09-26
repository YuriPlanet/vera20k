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

/// Retail Dustbowl with a Rocketeer on open level ground at (x, y), from which
/// it can fly eight cells east, with sixteen cells of open ground east of it.
/// The map's own `Player` house, whose start forces (an MCV, Rhinos and GIs)
/// stand within the 20mm's range of the hold, allies with both sides: a parked
/// Rocketeer engages whatever enemy comes near, and it would otherwise shoot
/// them, so the only enemies are the ones each test places. Each side keeps a
/// power plant out of the fight, so neither house is defeated under the Battle
/// mode's ShortGame.
fn retail_dustbowl_rocketeer() -> (crate::headless_scenario::HeadlessScenario, u64, u16, u16) {
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
    for (house, ally) in [
        ("AMERICANS", "PLAYER"),
        ("PLAYER", "AMERICANS"),
        ("RUSSIANS", "PLAYER"),
        ("PLAYER", "RUSSIANS"),
    ] {
        sim.house_alliances
            .entry(house.to_string())
            .or_default()
            .insert(ally.to_string());
    }
    sim.resolve_type_handles(&resources.rules);
    (scenario, rocketeer, x, y)
}

/// One production frame of the retail runtime.
fn retail_frame(
    scenario: &mut crate::headless_scenario::HeadlessScenario,
    orders: Vec<crate::sim::command::CommandEnvelope>,
) -> super::SimFrameOutput {
    scenario
        .runtime
        .advance_frame(
            &orders,
            crate::headless_scenario::SIM_TICK_MS,
            super::TickLane::Ordinary,
        )
        .expect("retail frame")
}

/// A Rocketeer on Dustbowl flies to a cell eight cells away and holds there,
/// then is ordered to attack a conscript eight cells beyond, which it closes
/// on. At cruise speed it takes Fly, in the hold Hover and at each shot
/// FireFly, whose sequence it shows; no airborne frame shows the standing or
/// walking pose. It reaches range still at speed and holds its fire until it
/// is at a tenth of full speed or less (the Infantry fire error's I4). Before
/// this chain, VERA showed its standing Ready frame in the air, and it fired
/// at full speed.
#[test]
#[ignore = "requires a retail RA2/YR install (RA2_DIR or config.toml)"]
fn retail_dustbowl_rocketeer_flies_hovers_and_fires_in_its_airborne_poses() {
    use crate::sim::command::{Command, CommandEnvelope};

    let (mut scenario, rocketeer, x, y) = retail_dustbowl_rocketeer();
    let sim = &mut scenario.runtime.simulation;
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
        retail_frame(&mut scenario, std::mem::take(&mut orders));
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

    // Ordered to attack a conscript eight cells beyond its hold, three past
    // its 20mm's range, it closes in, slows and shoots from the hover.
    let crate::sim::runtime::SimRuntime {
        simulation: sim,
        resources,
    } = &mut scenario.runtime;
    let conscript = sim
        .spawn_object(
            "E2",
            "Russians",
            x + 16,
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
    let rearm = |sim: &super::Simulation| {
        sim.substrate
            .entities
            .get(rocketeer)
            .map(|entity| entity.rearm_timer)
    };
    let mut last_rearm = rearm(&scenario.runtime.simulation);
    let mut shots = Vec::new();
    let mut fight = Vec::new();
    let mut held_by_speed = 0;
    for _ in 0..400 {
        retail_frame(&mut scenario, std::mem::take(&mut orders));
        let sim = &scenario.runtime.simulation;
        let now = pose(sim, rocketeer);
        fight.push(now);
        if now.doing == DO_FIRE_FLY {
            assert_eq!(now.sequence, Some(SequenceKind::FireFly), "{now:?}");
        }
        if now.altitude > 0 {
            assert!(
                !matches!(now.sequence, Some(SequenceKind::Stand | SequenceKind::Walk)),
                "airborne pose {now:?}"
            );
        }
        let Some(target) = sim.substrate.entities.get(conscript) else {
            break;
        };
        // Within the 20mm's five cells while still faster than a tenth: the
        // Infantry fire error's I4 holds the shot.
        let shooter = sim.substrate.entities.get(rocketeer).expect("rocketeer");
        let in_range = shooter.position.rx.abs_diff(target.position.rx) <= 4
            && shooter.position.ry.abs_diff(target.position.ry) <= 1;
        if in_range && now.fraction > 6553 && shooter.attack_target.is_some() {
            held_by_speed += 1;
        }
        // A discharge restarts the Rocketeer's rearm timer.
        let rearmed = rearm(sim);
        if rearmed != last_rearm {
            shots.push((now, target.health.current));
            last_rearm = rearmed;
        }
    }
    assert!(
        shots.len() >= 2,
        "the Rocketeer shot the conscript: {fight:?}"
    );
    assert!(
        held_by_speed > 0,
        "it reached range still at speed: {fight:?}"
    );
    for (shot, target_health) in &shots {
        // Fired (I4 `0x0051C9B8`) at no more than a tenth of full speed, in
        // FireFly; the killing shot's target expires the same frame, and its
        // Assign_Target(NULL) (`0x007079A1`) turns the FireFly to Hover.
        assert!(shot.fraction <= 6553, "{shot:?}");
        let (doing, sequence) = if *target_health > 0 {
            (DO_FIRE_FLY, SequenceKind::FireFly)
        } else {
            (DO_HOVER, SequenceKind::Hover)
        };
        assert_eq!(shot.doing, doing, "{shot:?}");
        assert_eq!(shot.sequence, Some(sequence), "{shot:?}");
    }
    assert!(
        shots.iter().any(|(_, health)| *health <= 0),
        "the Rocketeer killed the conscript: {shots:?}"
    );
    let after = pose(&scenario.runtime.simulation, rocketeer);
    assert_eq!(after.doing, DO_HOVER, "back to its hover: {after:?}");
}

/// A Rocketeer parked over Dustbowl after a Move order engages the enemies
/// that come within its range. It stays on Move, as native's
/// `FootClass::Mission_Move @ 0x004D4200` keeps a NavCom that is its own cell,
/// and native's passive acquire gate passes a `BalloonHover=` Foot parked on
/// its NavCom's cell (`TechnoClass::PassiveAcquireGate @ 0x00709290`,
/// `0x00709301..0x00709360`), so its passive scan picks a target and it
/// shoots. Before this chain, VERA's gate refused every Move but a team's,
/// and a parked Rocketeer never fired.
#[test]
#[ignore = "requires a retail RA2/YR install (RA2_DIR or config.toml)"]
fn retail_dustbowl_parked_rocketeer_engages_nearby_enemies() {
    use crate::sim::command::{Command, CommandEnvelope};

    let (mut scenario, rocketeer, x, y) = retail_dustbowl_rocketeer();
    let sim = &mut scenario.runtime.simulation;
    let americans = sim.interner.intern("Americans");
    let park = CommandEnvelope::new(
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
    let mut orders = vec![park];
    for _ in 0..240 {
        retail_frame(&mut scenario, std::mem::take(&mut orders));
    }
    let crate::sim::runtime::SimRuntime {
        simulation: sim,
        resources,
    } = &mut scenario.runtime;
    let parked = sim.substrate.entities.get(rocketeer).expect("rocketeer");
    let move_mission = parked.mission.current();
    assert_eq!(
        move_mission.known(),
        Some(crate::sim::mission::MissionType::Move),
        "parked on Move"
    );
    assert_eq!(
        parked.navigation.nav_com,
        Some(crate::sim::components::NavTargetRef::cell(
            parked.position.rx,
            parked.position.ry
        )),
        "parked on its NavCom's cell"
    );
    assert!(parked.attack_target.is_none());
    // A conscript three cells beyond the hold, the only enemy within its
    // 20mm's range.
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
    let mut shots = Vec::new();
    let mut last_rearm = sim
        .substrate
        .entities
        .get(rocketeer)
        .map(|entity| entity.rearm_timer);
    for _ in 0..300 {
        retail_frame(&mut scenario, Vec::new());
        let sim = &scenario.runtime.simulation;
        let entity = sim.substrate.entities.get(rocketeer).expect("rocketeer");
        assert_eq!(entity.mission.current(), move_mission, "it stays on Move");
        let rearm = Some(entity.rearm_timer);
        if rearm != last_rearm {
            last_rearm = rearm;
            shots.push((
                entity.attack_target.as_ref().map(|attack| attack.target),
                pose(sim, rocketeer),
            ));
        }
        if sim.substrate.entities.get(conscript).is_none() {
            break;
        }
    }
    assert!(shots.len() >= 2, "the parked Rocketeer fires: {shots:?}");
    for (target, pose) in &shots {
        assert!(
            target.is_none() || *target == Some(crate::sim::combat::TargetKind::Entity(conscript)),
            "it shoots the conscript: {shots:?}"
        );
        assert!(
            matches!(pose.doing, DO_FIRE_FLY | DO_HOVER),
            "from its hover: {shots:?}"
        );
    }
    let sim = &scenario.runtime.simulation;
    assert!(
        sim.substrate
            .entities
            .get(conscript)
            .is_none_or(|entity| entity.health.current < 125),
        "the conscript is hit"
    );
}

/// Through the production frame: a Rocketeer ordered to a cell four cells
/// east parks there on Move with its NavCom on its own cell (native
/// `FootClass::Mission_Move @ 0x004D4200` keeps Move while the NavCom is set),
/// and when an enemy stands three cells beyond, its passive scan acquires it
/// through the gate's Move arm (`0x00709301..0x00709360`) and it fires, still
/// parked and on Move. Before, the gate refused every Move but a team's.
#[test]
fn a_parked_rocketeer_scans_and_fires_on_move() {
    use crate::sim::command::{Command, CommandEnvelope};
    use crate::sim::mission::MissionType;
    use std::collections::BTreeMap;
    let row = serde_json::json!({"doing": DO_HOVER, "fraction": 0.0, "armed": true,
        "owner": {"phase": 2, "moving": false}});
    let (mut sim, rules, shooter) = rocketeer_crash_fixture(&row);
    let grid = crate::sim::pathfinding::PathGrid::test_all_passable(70, 70);
    let americans = sim.interner.intern("Americans");
    let order = CommandEnvelope::new(
        americans,
        sim.session.tick + 1,
        Command::Move {
            entity_id: 1,
            target_rx: 56,
            target_ry: 52,
            queue: false,
            group_id: None,
        },
    );
    let mut orders = vec![order];
    let mut parked = false;
    for _ in 0..300 {
        sim.advance_tick(
            &std::mem::take(&mut orders),
            Some(&rules),
            &BTreeMap::new(),
            Some(&grid),
            None,
            67,
        );
        let entity = sim.substrate.entities.get(1).unwrap();
        let runtime = entity
            .locomotor
            .as_ref()
            .unwrap()
            .jumpjet_runtime()
            .unwrap();
        if entity.position.rx == 56
            && runtime.phase == 2
            && entity.navigation.nav_com == Some(crate::sim::components::NavTargetRef::cell(56, 52))
        {
            parked = true;
            break;
        }
    }
    assert!(parked, "the Rocketeer parks on its NavCom's cell");
    let entity = sim.substrate.entities.get(1).unwrap();
    assert_eq!(entity.mission.current().known(), Some(MissionType::Move));
    assert!(entity.attack_target.is_none());
    // The shooter steps three cells east of the hold.
    sim.remove_entity_occupancy(shooter);
    {
        let enemy = sim.substrate.entities.get_mut(shooter).unwrap();
        enemy.position.rx = 59;
        enemy.position.ry = 52;
    }
    sim.add_entity_occupancy(shooter);
    let rearm = sim.substrate.entities.get(1).unwrap().rearm_timer;
    let mut acquired = false;
    let mut fired = false;
    for _ in 0..120 {
        sim.advance_tick(&[], Some(&rules), &BTreeMap::new(), Some(&grid), None, 67);
        let Some(entity) = sim.substrate.entities.get(1) else {
            break;
        };
        assert_eq!(entity.mission.current().known(), Some(MissionType::Move));
        acquired |= entity.attack_target.as_ref().map(|attack| attack.target)
            == Some(crate::sim::combat::TargetKind::Entity(shooter));
        fired |= entity.rearm_timer != rearm;
        if fired {
            break;
        }
    }
    assert!(acquired, "the parked Rocketeer acquires the enemy");
    assert!(fired, "and fires at it");
}

/// A grounded Rocketeer idling on Guard fidgets as native does: its idle turn
/// requests `Do_Action(Idle1 or Idle2)` (`0x0051CEE0`, `0x0051CF42`), and when
/// the fidget has played, the sequencer's default arm turns it to the fidget's
/// facing hint (`0x00520CEB..0x00520D16`; `Idle1=..,S`, `Idle2=..,E`) and
/// returns it to Ready, where it idles again. With the pose alone its fidget
/// never ended, and it never idled again.
#[test]
fn a_grounded_rocketeer_fidgets_and_turns_to_the_fidgets_facing() {
    use crate::sim::movement::infantry_action::{DO_IDLE1, DO_IDLE2, DO_READY};
    use std::collections::BTreeMap;
    let row = serde_json::json!({"doing": DO_READY, "fraction": 0.0, "height": 0,
        "owner": {"phase": 0, "moving": false}});
    let (mut sim, rules, _) = rocketeer_crash_fixture(&row);
    let grid = crate::sim::pathfinding::PathGrid::test_all_passable(70, 70);
    let mut fidgets = Vec::new();
    let mut playing: Option<i32> = None;
    for _ in 0..3000 {
        sim.advance_tick(&[], Some(&rules), &BTreeMap::new(), Some(&grid), None, 67);
        let entity = sim.substrate.entities.get(1).unwrap();
        let doing = entity.mission_leaf.as_infantry().unwrap().doing();
        match (playing, doing) {
            (None, DO_IDLE1 | DO_IDLE2) => {
                let kind = crate::rules::infantry_sequence::action_kind(doing);
                assert_eq!(entity.animation.as_ref().map(|a| a.sequence), kind);
                playing = Some(doing);
            }
            (Some(action), DO_READY) => {
                fidgets.push((action, entity.facing));
                playing = None;
            }
            _ => {}
        }
    }
    assert!(fidgets.len() >= 2, "it fidgets again: {fidgets:?}");
    for (action, facing) in &fidgets {
        let hint = if *action == DO_IDLE1 { 128 } else { 64 };
        assert_eq!(*facing, hint, "{fidgets:?}");
    }
}

/// One frame of a shot-down Rocketeer: the Pose, Health, crash latch, and the
/// anims and Rocketeer sounds the frame produced.
#[derive(Debug)]
struct Fall {
    pose: Option<Pose>,
    health: i32,
    crashing: bool,
    rearm: Option<crate::sim::timer::CdTimer>,
    anims: Vec<String>,
    crash_sounds: usize,
}

/// A Rocketeer holding over Dustbowl is shot down by a Flak Track six cells
/// east. The killing hit builds `InfantryExplode=` (S_BANG34) where it hovers
/// and leaves it alive at Health 0; its `CrashingSound=` (RocketeerDie) plays
/// once. From the next frame it plays AirDeathStart at Health 1 and falls
/// from its 500-lepton hover to the ground, where it plays AirDeathFinish and
/// is removed without a body: the crash corpus's frames (a landing 12 frames
/// after the kill, removal 18 after that). The Flak Track never targets it
/// again (Assign_Target refuses a death action), and the faller never fires
/// (the Infantry fire error's I1). Before this chain, VERA removed it at the
/// hit.
#[test]
#[ignore = "requires a retail RA2/YR install (RA2_DIR or config.toml)"]
fn retail_dustbowl_shot_down_rocketeer_falls_and_leaves_no_body() {
    use crate::sim::command::{Command, CommandEnvelope};
    use crate::sim::movement::infantry_action::{DO_AIR_DEATH_FINISH, DO_AIR_DEATH_START};

    let (mut scenario, rocketeer, x, y) = retail_dustbowl_rocketeer();
    let sim = &mut scenario.runtime.simulation;
    let americans = sim.interner.intern("Americans");
    let hold = CommandEnvelope::new(
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
    let mut orders = vec![hold];
    for _ in 0..240 {
        retail_frame(&mut scenario, std::mem::take(&mut orders));
    }
    let held = pose(&scenario.runtime.simulation, rocketeer);
    assert_eq!((held.phase, held.doing), (2, DO_HOVER), "{held:?}");

    let crate::sim::runtime::SimRuntime {
        simulation: sim,
        resources,
    } = &mut scenario.runtime;
    let flak = sim
        .spawn_object(
            "HTK",
            "Russians",
            x + 14,
            y + 1,
            192,
            &resources.rules,
            &resources.height_map,
        )
        .expect("Flak Track");
    sim.resolve_type_handles(&resources.rules);
    let russians = sim.interner.intern("Russians");
    let attack = CommandEnvelope::new(
        russians,
        sim.session.tick + 1,
        Command::Attack {
            attacker_id: flak,
            target_id: rocketeer,
        },
    );
    let crash_sound = sim.interner.intern("RocketeerDie");
    let mut orders = vec![attack];
    let mut seen: std::collections::BTreeSet<_> =
        sim.substrate.anims.iter().map(|(id, _)| *id).collect();
    let mut frames: Vec<Fall> = Vec::new();
    let mut flak_targets = Vec::new();
    for _ in 0..900 {
        let output = retail_frame(&mut scenario, std::mem::take(&mut orders));
        let sim = &scenario.runtime.simulation;
        let anims = sim
            .substrate
            .anims
            .iter()
            .filter(|(id, _)| seen.insert(**id))
            .map(|(_, anim)| sim.interner.resolve(anim.type_id).to_string())
            .collect();
        let crash_sounds = output
            .sound_events
            .iter()
            .filter(|event| {
                matches!(event, super::SimSoundEvent::AnimationStarted { anim_id, sound_id, .. }
                    if *anim_id == rocketeer && *sound_id == crash_sound)
            })
            .count();
        let entity = sim
            .substrate
            .entities
            .get(rocketeer)
            .filter(|entity| entity.lifecycle.object_alive);
        frames.push(Fall {
            pose: entity.map(|_| pose(sim, rocketeer)),
            health: entity.map_or(0, |entity| entity.health.current),
            crashing: entity.is_some_and(|entity| entity.crashing),
            rearm: entity.map(|entity| entity.rearm_timer),
            anims,
            crash_sounds,
        });
        flak_targets.push(
            sim.substrate
                .entities
                .get(flak)
                .and_then(|entity| entity.attack_target.as_ref())
                .map(|attack| attack.target.clone()),
        );
        if entity.is_none() {
            break;
        }
    }
    let kill = frames
        .iter()
        .position(|frame| frame.crashing)
        .unwrap_or_else(|| panic!("the Flak Track shot the Rocketeer down: {frames:#?}"));
    let at_kill = &frames[kill];
    let kill_pose = at_kill.pose.expect("it stays at the hit");
    assert_eq!(at_kill.health, 0, "{at_kill:?}");
    assert_eq!(kill_pose.altitude, 500, "killed in its hover: {at_kill:?}");
    assert_eq!(
        at_kill
            .anims
            .iter()
            .filter(|name| *name == "S_BANG34")
            .count(),
        1,
        "InfantryExplode at the hit: {at_kill:?}"
    );
    assert!(frames[..kill].iter().all(|frame| frame.pose.is_some()));
    // The crash corpus (`jumpjet_infantry_crash.json`): frames 0..=10 fall in
    // AirDeathStart, frame 11 lands in AirDeathFinish, frame 29 UnInits.
    let fall = &frames[kill + 1..];
    assert_eq!(fall.len(), 30, "removed on the corpus frame: {fall:#?}");
    let mut last_height = kill_pose.altitude;
    for (index, frame) in fall.iter().enumerate() {
        if index == 29 {
            assert!(frame.pose.is_none(), "removed: {frame:?}");
            break;
        }
        let now = frame.pose.expect("falling");
        let expected = if index < 11 {
            (DO_AIR_DEATH_START, SequenceKind::AirDeathStart)
        } else {
            (DO_AIR_DEATH_FINISH, SequenceKind::AirDeathFinish)
        };
        assert_eq!(
            (now.doing, now.sequence),
            (expected.0, Some(expected.1)),
            "frame {index}: {frame:?}"
        );
        assert_eq!(frame.health, 1, "frame {index}: {frame:?}");
        if index < 11 {
            assert!(
                now.altitude < last_height && now.altitude > 0,
                "frame {index}: {frame:?}"
            );
        } else {
            assert_eq!(now.altitude, 0, "frame {index}: {frame:?}");
        }
        last_height = now.altitude;
        // The faller never fires.
        assert_eq!(frame.rearm, fall[0].rearm, "frame {index}: {frame:?}");
    }
    // No body (`DeadBodies=` is Die1..5's) and no second explosion.
    let general = &scenario.runtime.resources.rules.general;
    for frame in fall {
        for name in &frame.anims {
            assert!(
                name != "S_BANG34"
                    && !general
                        .dead_bodies
                        .iter()
                        .any(|body| body.eq_ignore_ascii_case(name)),
                "{frame:?}"
            );
        }
    }
    // The crash sound plays once, following the Rocketeer.
    assert_eq!(
        frames.iter().map(|frame| frame.crash_sounds).sum::<usize>(),
        1,
        "{frames:#?}"
    );
    // After the hit nothing targets the faller.
    for target in &flak_targets[kill + 1..] {
        assert_ne!(
            *target,
            Some(crate::sim::combat::TargetKind::Entity(rocketeer)),
            "{flak_targets:?}"
        );
    }
}

/// A Rocketeer (the retail `[JUMPJET]` Jumpjet block and
/// `[RocketeerSequence]`) and an area warhead to shoot it down with.
fn rocketeer_crash_rules() -> crate::rules::ruleset::RuleSet {
    rocketeer_rules_armed(false)
}

/// The crash corpus's rules; `armed` gives the Rocketeer the shooter's gun.
fn rocketeer_rules_armed(armed: bool) -> crate::rules::ruleset::RuleSet {
    use crate::rules::ini_parser::IniFile;
    let mut rules = crate::rules::ruleset::RuleSet::from_ini(&IniFile::from_str(&format!(
        "[General]\nConditionRed=0.25\n\
         [InfantryTypes]\n0=JUMPJET\n\
         [VehicleTypes]\n0=SHOOTER\n\
         [JUMPJET]\nStrength=125\nArmor=flak\nImage=ROCK\nJumpJet=yes\nCrashable=yes\n\
         BalloonHover=yes\nLocomotor={{92612C46-F71F-11d1-AC9F-006008055BB5}}\n\
         SpeedType=Hover\nMovementZone=Fly\nJumpjetSpeed=30\nJumpjetClimb=20\n\
         JumpjetCrash=25\nJumpjetHeight=500\nJumpjetWobbles=.01\nJumpjetDeviation=1\n\
         JumpjetNoWobbles=yes\nCrashingSound=RocketeerDie\n{}\
         [SHOOTER]\nStrength=100\nPrimary=CrashGun\n\
         [CrashGun]\nDamage=150\nRange=6\nWarhead=CrashWH\n\
         [CrashWH]\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n",
        if armed { "Primary=CrashGun\n" } else { "" }
    )))
    .unwrap();
    let art = IniFile::from_str(
        "[ROCK]\nSequence=RocketeerSequence\nFireUp=2\n\
         [RocketeerSequence]\nReady=0,1,1\nGuard=0,1,1\nProne=86,1,6\nWalk=8,6,6\n\
         FireUp=164,6,6\nDown=260,2,2\nCrawl=86,6,6\nUp=276,2,2\nFireProne=212,6,6\n\
         Idle1=56,15,0,S\nIdle2=71,15,0,E\nDie1=134,15,0\nDie2=149,15,0\nDie3=0,0,0\n\
         Die4=0,0,0\nDie5=0,0,0\nFly=292,6,6\nHover=292,6,6\nFireFly=370,6,6\n\
         Tumble=340,15,0\nAirDeathStart=340,8,0\nAirDeathFalling=348,1,0\n\
         AirDeathFinish=349,6,0\nParadrop=418,1,0\nCheer=419,8,0,E\nPanic=8,6,6\n",
    );
    rules.merge_art_data(&crate::rules::art_data::ArtRegistry::from_ini(&art));
    let registry = crate::rules::infantry_sequence::parse_infantry_sequence_registry(&art);
    rules.replace_animation_sequences_for_test(
        crate::rules::animation_sequence::build_animation_sequence_catalog(&rules, Some(&registry)),
    );
    rules
}

/// Rocketeer 1 over cell (52, 52) of a flat 70 x 70 map, in the corpus row's
/// locomotor state, height, Doing and speed fraction, holding that cell's air
/// slot; a Soviet shooter at (40, 40). The Scenario RNG is seeded as the
/// oracle seeds it (31).
fn rocketeer_crash_fixture(
    input: &serde_json::Value,
) -> (super::Simulation, crate::rules::ruleset::RuleSet, u64) {
    use super::lifecycle_tests::install_common_raw_terrain;
    use super::{PlacementEvidence, RevealOutcome, RevealPosition, RevealRequest};
    use crate::map::entities::EntityCategory;
    use crate::sim::house_state::HouseState;
    use crate::util::fixed_math::SimFixed;
    let rules = rocketeer_rules_armed(input["armed"].as_bool().unwrap_or(false));
    let mut sim = super::Simulation::with_seed(0);
    sim.scenario_rng = crate::sim::rng::SimRng::new(31);
    install_common_raw_terrain(&mut sim, 70, 70, 0, None);
    sim.playfield_bounds = Some(crate::sim::cell_rect::PlayfieldBounds {
        base: 64,
        off_fc: 0,
        off_100: 0,
        off_104: 64,
        off_108: 64,
    });
    sim.playfield_size_height = Some(64);
    sim.session.binary_frame = 1000;
    for (name, human) in [("Americans", true), ("Soviets", false)] {
        let house = sim.interner.intern(name);
        sim.houses
            .insert(house, HouseState::new(house, 0, None, human, 0, 10));
    }
    let reveal = |sim: &mut super::Simulation, id: u64, rx: u16, ry: u16| {
        assert!(matches!(
            sim.try_reveal_entity(
                id,
                RevealRequest {
                    position: RevealPosition {
                        rx,
                        ry,
                        z: 0,
                        sub_x: SimFixed::from_num(128),
                        sub_y: SimFixed::from_num(128),
                    },
                    placement: PlacementEvidence::MarkSucceeded,
                    logic_eligible: true,
                }
            ),
            RevealOutcome::Revealed { .. }
        ));
    };
    assert_eq!(sim.allocate_stable_id(), 1);
    let americans = sim.interner.intern("Americans");
    let jumpjet = sim.interner.intern("JUMPJET");
    let mut rocketeer = crate::sim::game_entity::GameEntity::new_at_frame_zero_for_test(
        1,
        52,
        52,
        0,
        0,
        americans,
        crate::sim::components::Health { current: 125 },
        jumpjet,
        EntityCategory::Infantry,
        0,
        5,
        false,
    );
    rocketeer.lifecycle.in_limbo = true;
    sim.substrate.entities.insert(rocketeer);
    {
        let entity = sim.substrate.entities.get_mut(1).unwrap();
        entity.animation = Some(crate::sim::animation::Animation::new(SequenceKind::Stand));
        entity.infantry = Some(crate::sim::game_entity::InfantryRuntime::new());
        entity.mission_leaf = crate::sim::mission::leaf::MissionLeafState::for_entity_category(
            EntityCategory::Infantry,
        );
        entity.locomotor = Some(
            crate::sim::movement::locomotor::LocomotorState::from_object_type(
                rules.object("JUMPJET").unwrap(),
                0,
            ),
        );
    }
    reveal(&mut sim, 1, 52, 52);
    sim.remove_entity_occupancy(1);
    let height = input["height"].as_i64().unwrap_or(500) as i32;
    {
        let entity = sim.substrate.entities.get_mut(1).unwrap();
        entity.health.current = 125;
        entity.position.exact_z_leptons = Some(height);
        entity.body_facing = Some(crate::sim::movement::FacingClass::new(0x4000, 127));
        entity
            .foot_speed
            .set_speed_fraction_native_bits(input["fraction"].as_f64().unwrap_or(0.0).to_bits());
        let loco = entity.locomotor.as_mut().unwrap();
        loco.altitude = SimFixed::from_num(height);
        let runtime = loco.jumpjet_runtime_mut().unwrap();
        runtime.phase = input["owner"]["phase"].as_i64().unwrap() as i32;
        runtime.moving = input["owner"]["moving"].as_bool().unwrap();
        runtime.destination = crate::sim::components::DriveCoord { x: 0, y: 0, z: 0 };
        runtime.flight.facing.snap(0x4000, 1000);
        // The oracle's locomotor holds `JumpjetHeight=` as its target height
        // whatever the row's height (`jumpjet_infantry_actions` ROCKETEER).
        runtime.flight.target_height = 500;
    }
    sim.add_entity_occupancy(1);
    if height > 0 {
        assert!(sim.substrate.air_slots.claim(52, 52, 1));
    }
    // The row's action, started by Do_Action (forced) as the oracle starts it.
    let doing = input["doing"].as_i64().unwrap() as i32;
    if doing != -1 {
        assert!(sim.infantry_do_action(1, doing, true, &rules).unwrap());
    }
    let shooter_type = sim.interner.intern("SHOOTER");
    let soviets = sim.interner.intern("Soviets");
    let shooter = sim.allocate_stable_id();
    let mut entity = crate::sim::game_entity::GameEntity::new_at_frame_zero_for_test(
        shooter,
        40,
        40,
        0,
        0,
        soviets,
        crate::sim::components::Health { current: 100 },
        shooter_type,
        EntityCategory::Unit,
        0,
        5,
        true,
    );
    entity.lifecycle.in_limbo = true;
    sim.substrate.entities.insert(entity);
    reveal(&mut sim, shooter, 40, 40);
    sim.resolve_type_handles(&rules);
    (sim, rules, shooter)
}

/// Kill Rocketeer 1 through the production receiver: one lethal direct hit.
fn shoot_the_rocketeer(
    sim: &mut super::Simulation,
    rules: &crate::rules::ruleset::RuleSet,
    shooter: u64,
) {
    let soviets = sim.interner.intern("Soviets");
    let warhead = sim.interner.intern("CrashWH");
    let health = sim.substrate.entities.get(1).unwrap().health.current;
    sim.commit_direct_damage_receiver(
        rules,
        None,
        crate::sim::combat::EntityDamageEvent::direct_receiver(
            1,
            i32::from(health) + 100,
            0,
            shooter,
            Some(soviets),
            warhead,
            crate::sim::combat::ReceiverCallFlags {
                ignore_defenses: false,
                arg6: false,
            },
        ),
    );
}

fn scenario_draws(before: i32, after: i32) -> i32 {
    (after - before).rem_euclid(97)
}

/// Parity with `tools/spatial_oracle/jumpjet_infantry_crash.json`: a
/// Rocketeer shot down through the production receiver stops its locomotor
/// as often as the native kill (one Scenario draw each: 10 from Hover or
/// Cheer, 11 from Fly, FireFly or no action), keeps its action and crash
/// latch as native does, and then, through `advance_tick`, falls,
/// plays AirDeathStart, lands in AirDeathFinish at Health 1 and is removed
/// on the native frame. A second kill in the fall draws 10 more and one at
/// its landing (the Health-0 re-entry into Stop_Driver); a fall from 1200
/// leptons outlasts AirDeathStart, plays AirDeathFalling and takes the default
/// arm to Hover before it lands; a grounded kill crashes nothing and draws
/// nothing. Every frame, Assign_Target refuses the faller exactly while its
/// native action is a death action (`0x006FCF2D`): a Cheer faller stays a
/// target.
///
/// Not compared: the Scenario draws of the fall's other frames. VERA's also
/// carry the faller's Guard mission, which native runs too (InfantryClass::AI
/// calls FootClass::AI at any Health, `0x0051BC9D`) but the oracle does not.
#[test]
fn a_shot_down_rocketeer_falls_like_the_native_crash() {
    use std::collections::BTreeMap;
    let corpus: serde_json::Value = serde_json::from_str(include_str!(
        "../../../tools/spatial_oracle/jumpjet_infantry_crash.json"
    ))
    .expect("corpus parses");
    let grid = crate::sim::pathfinding::PathGrid::test_all_passable(70, 70);
    for row in corpus.as_array().expect("rows") {
        let name = row["name"].as_str().unwrap();
        let input = &row["input"];
        let output = &row["output"];
        let (mut sim, rules, shooter) = rocketeer_crash_fixture(input);
        let kill = |sim: &mut super::Simulation, native: &serde_json::Value, label: &str| {
            let before = sim.scenario_rng.logical_view().index_a;
            shoot_the_rocketeer(sim, &rules, shooter);
            let after = sim.scenario_rng.logical_view().index_a;
            let steps = native.as_array().unwrap();
            let native_before = if label == "kill" {
                output["before"]["scenario_rng"][0].as_i64().unwrap() as i32
            } else {
                steps[0]["after"]["scenario_rng"][0].as_i64().unwrap() as i32
                    - steps[0]["events"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .filter(|event| event[0] == "stop_moving")
                        .count() as i32
            };
            let native_after = steps.last().unwrap()["after"]["scenario_rng"][0]
                .as_i64()
                .unwrap() as i32;
            assert_eq!(
                scenario_draws(before, after),
                scenario_draws(native_before, native_after),
                "{name}: {label} Scenario draws"
            );
            steps.last().unwrap()["accepted"].as_bool().unwrap()
        };
        let accepted = kill(&mut sim, &output["kills"][0], "kill");
        let entity = sim.substrate.entities.get(1);
        if !accepted {
            // A refused crash: InfantryExplode, then UnInit.
            assert!(
                entity.is_none_or(|entity| !entity.lifecycle.object_alive),
                "{name}: refused crash removes it"
            );
            continue;
        }
        let entity = entity.expect("a crashing Rocketeer stays");
        let crash = output["kills"][0].as_array().unwrap().last().unwrap()["after"].clone();
        assert!(entity.crashing && !entity.dying, "{name}");
        assert_eq!(entity.health.current, 0, "{name}");
        assert_eq!(
            i64::from(entity.mission_leaf.as_infantry().unwrap().doing()),
            crash["doing"].as_i64().unwrap(),
            "{name}: Doing after the kill"
        );
        let frames = output["frames"].as_array().unwrap();
        let second = input["second_kill_frame"]
            .as_u64()
            .map(|frame| frame as usize);
        let index_after = |kill: &serde_json::Value| {
            kill.as_array().unwrap().last().unwrap()["after"]["scenario_rng"][0]
                .as_i64()
                .unwrap() as i32
        };
        let mut native_before = index_after(&output["kills"][0]);
        for (index, expected) in frames.iter().enumerate() {
            if second == Some(index) {
                kill(&mut sim, &output["kills"][1], "second kill");
                native_before = index_after(&output["kills"][1]);
            }
            let before = sim.scenario_rng.logical_view().index_a;
            sim.advance_tick(&[], Some(&rules), &BTreeMap::new(), Some(&grid), None, 67);
            let native_after = expected["scenario_rng"][0].as_i64().unwrap() as i32;
            let draws = (
                scenario_draws(before, sim.scenario_rng.logical_view().index_a),
                scenario_draws(native_before, native_after),
            );
            native_before = native_after;
            let events = expected["events"].as_array().unwrap();
            let removed = events.iter().any(|event| event == "uninit");
            let entity = sim
                .substrate
                .entities
                .get(1)
                .filter(|e| e.lifecycle.object_alive);
            if removed {
                assert!(entity.is_none(), "{name}: removed on frame {index}");
                break;
            }
            let entity = entity.unwrap_or_else(|| panic!("{name}: alive on frame {index}"));
            let at = format!("{name}: frame {index}");
            assert_eq!(
                i64::from(entity.mission_leaf.as_infantry().unwrap().doing()),
                expected["doing"].as_i64().unwrap(),
                "{at}: Doing"
            );
            assert_eq!(
                i64::from(entity.animation.as_ref().unwrap().frame_index),
                expected["stage"].as_i64().unwrap(),
                "{at}: stage"
            );
            assert_eq!(
                i64::from(entity.health.current),
                expected["health"].as_i64().unwrap(),
                "{at}: Health"
            );
            assert_eq!(
                i64::from(entity.position.exact_z_leptons.unwrap()),
                expected["coord"][2].as_i64().unwrap(),
                "{at}: height"
            );
            // The landing: at Health 0 its forced AirDeathFinish re-enters
            // Stop_Driver (`0x0051DA96`), one Stop_Moving and one draw.
            if events.iter().any(|event| event[0] == "set_height") {
                assert_eq!(draws.0, draws.1, "{at}: landing Scenario draws");
            }
            let native_doing = expected["doing"].as_i64().unwrap() as i32;
            assert_eq!(
                crate::sim::mission::concrete_effects::assign_target_commits(
                    &sim.substrate.entities,
                    Some(crate::sim::combat::TargetKind::Entity(1)),
                ),
                expected["health"].as_i64() != Some(0)
                    && !crate::sim::movement::infantry_action::in_death_sequence(native_doing),
                "{at}: a target"
            );
        }
    }
}
