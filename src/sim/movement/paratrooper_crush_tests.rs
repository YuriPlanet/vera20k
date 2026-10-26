//! UnitCrush741811 skips Object+8D while an ordinary paradrop is falling.

use crate::map::playfield::PlayfieldBounds;
use crate::rules::ruleset::RuleSet;
use crate::sim::command::{Command, CommandEnvelope};
use crate::sim::intern::InternedId;
use crate::sim::movement::locomotor::MovementLayer;
use crate::sim::movement::track_process::TrackFamily;
use crate::sim::overlay_grid::OverlayGrid;
use crate::sim::scenario_session::ScenarioGameMode;
use crate::sim::world::{SimSoundEvent, Simulation};

fn frame(sim: &mut Simulation, rules: &RuleSet) {
    let commands = sim.take_due_commands();
    assert!(
        sim.advance_tick(&commands, Some(rules), None, None, 33)
            .frame_committed
    );
}

fn scene() -> Option<(RuleSet, Simulation, InternedId, u64)> {
    let rules = crate::rules::retail_ini_fixture::retail_battle_rules()?.rules;
    let (rules, mut sim, owner) =
        crate::sim::superweapon::chronosphere_tests::world_with(rules, 64, &[]);
    sim.session
        .set_game_mode(ScenarioGameMode::from_legacy_nonzero(true));
    sim.session.game_options_mut_for_test().short_game = false;
    sim.session.set_current_house_for_test(Some(owner));
    let enemy = sim.interner.get("Russians").unwrap();
    sim.houses
        .get_mut(&enemy)
        .unwrap()
        .set_is_human_for_test(true);
    sim.set_playfield_bounds_for_test(Some(PlayfieldBounds::from_raw_local_size(
        32,
        32,
        [2, 4, 28, 22],
    )));
    sim.set_playfield_size_height_for_test(Some(32));
    sim.fog.set_width_for_test(64);
    sim.fog.set_height_for_test(64);
    sim.set_input_delay_ticks(0);
    sim.overlay_grid = Some(OverlayGrid::new(64, 64));
    for (kind, house, at) in [
        ("CAAIRP", "Americans", (30, 40)),
        ("GAPOWR", "Americans", (30, 36)),
        ("NAPOWR", "Russians", (44, 36)),
    ] {
        sim.spawn_object(kind, house, at.0, at.1, 0, &rules)
            .unwrap_or_else(|| panic!("initial {kind} Unlimbo at {at:?}"));
    }
    let crusher = sim
        .spawn_object("SMCV", "Russians", 38, 40, 0, &rules)
        .unwrap();
    Some((rules, sim, owner, crusher))
}

fn drop() -> Option<(RuleSet, Simulation, u64, u64, (u16, u16))> {
    let (rules, mut sim, owner, crusher) = scene()?;
    let sw = sim.interner.get("ParaDropSpecial").unwrap();
    for _ in 0..5000 {
        if sim.super_weapons[&owner][&sw].is_ready() {
            break;
        }
        frame(&mut sim, &rules);
    }
    assert!(sim.super_weapons[&owner][&sw].is_active());
    assert!(
        sim.super_weapons[&owner][&sw].is_ready(),
        "the owned airfield naturally charges"
    );
    sim.queue_command(CommandEnvelope::new(
        owner,
        sim.session.tick() + 1,
        Command::LaunchSuperWeapon {
            sw_type_id: sw,
            target_rx: 40,
            target_ry: 40,
        },
    ));
    frame(&mut sim, &rules);
    assert!(!sim.super_weapons[&owner][&sw].is_ready());
    let mut dropped = None;
    for _ in 0..1800 {
        frame(&mut sim, &rules);
        dropped = sim
            .substrate
            .entities
            .values()
            .find(|e| {
                sim.interner.resolve(e.type_ref()) == "E1"
                    && e.owner() == owner
                    && !e.lifecycle.in_limbo()
                    && e.is_falling_down()
            })
            .map(|e| (e.stable_id(), (e.position.rx(), e.position.ry())));
        if dropped.is_some() {
            break;
        }
    }
    let (victim, at) = dropped.expect("the launched plane actually drops an E1");
    let e = sim.substrate.entities.get(victim).unwrap();
    assert!(e.lifecycle.object_alive() && e.lifecycle.cell_marked());
    assert!(e.position.exact_z_leptons().unwrap() > 0);
    assert!(
        sim.substrate
            .occupancy
            .get(at.0, at.1)
            .unwrap()
            .iter_layer(MovementLayer::Ground)
            .any(|o| o.entity_id == victim)
    );
    eprintln!(
        "drop frame={} cell={at:?} height={:?}",
        sim.session.binary_frame(),
        e.position.exact_z_leptons()
    );
    Some((rules, sim, crusher, victim, at))
}

fn move_to(sim: &mut Simulation, crusher: u64, at: (u16, u16)) {
    let enemy = sim.substrate.entities.get(crusher).unwrap().owner();
    sim.queue_command(CommandEnvelope::new(
        enemy,
        sim.session.tick() + 1,
        Command::Move {
            entity_id: crusher,
            target_rx: at.0,
            target_ry: at.1,
        },
    ));
}

#[test]
fn a_vehicle_leaves_a_naturally_paradropped_falling_infantryman_alive() {
    let Some((rules, mut sim, crusher, victim, at)) = drop() else {
        return;
    };
    move_to(&mut sim, crusher, at);
    let mut moving = false;
    let mut contact = false;
    for n in 0..180 {
        let before = sim.substrate.entities.get(victim).map(|e| {
            (
                e.is_falling_down(),
                e.position.exact_z_leptons(),
                e.fall_rate(),
                e.position.rx(),
                e.position.ry(),
            )
        });
        frame(&mut sim, &rules);
        let vehicle = sim.substrate.entities.get(crusher).unwrap();
        let loco = vehicle.locomotor.as_ref().unwrap();
        moving |= loco.track_head(TrackFamily::Drive).is_some()
            && loco.track_valid(TrackFamily::Drive) == Some(true);
        let e = sim.substrate.entities.get(victim).unwrap_or_else(|| {
            panic!(
                "paratrooper removed at step {n}, before={before:?}, vehicle={:?}",
                vehicle.position
            )
        });
        assert!(
            e.is_falling_down(),
            "vehicle must reach the victim before landing; step {n}, vehicle={:?}",
            vehicle.position
        );
        let [x, y] = super::ground_pose::position_world_xy(&vehicle.position);
        let [vx, vy] = super::ground_pose::position_world_xy(&e.position);
        if moving
            && (x, y) == (i32::from(at.0) * 256 + 128, i32::from(at.1) * 256 + 128)
            && loco.track_head(TrackFamily::Drive).is_none()
            && loco.track_destination(TrackFamily::Drive).is_none()
            && loco
                .track_progress(TrackFamily::Drive)
                .unwrap()
                .turn_index()
                == -1
            && loco.track_valid(TrackFamily::Drive) == Some(false)
            && super::bump_crush::within_crush_distance_sq((x, y), (vx, vy))
        {
            assert!(e.lifecycle.object_alive() && e.lifecycle.cell_marked());
            assert!(e.position.exact_z_leptons().unwrap() > 0);
            assert!(
                sim.substrate
                    .occupancy
                    .get(at.0, at.1)
                    .unwrap()
                    .iter_layer(MovementLayer::Ground)
                    .any(|o| o.entity_id == victim)
            );
            contact = true;
            eprintln!(
                "falling arrival frame={} step={n} height={:?}",
                sim.session.binary_frame(),
                e.position.exact_z_leptons()
            );
            break;
        }
    }
    assert!(
        contact,
        "ordinary movement completes its crush callback beside the falling victim"
    );
    assert!(
        !sim.sound_events
            .iter()
            .any(|e| matches!(e, SimSoundEvent::EntityCrushed { .. }))
    );
}

/// Guards the removal checks below: the victim's `falling` state seen at the
/// end of the previous two frames. A removal needs both true, so the one
/// frame whose movement may legitimately run after the landing tick (the
/// arrival loop's own `+0x8D` read is what decides natively) cannot be
/// mistaken for an airborne kill.
fn still_airborne(recent: [Option<bool>; 2]) -> bool {
    recent == [Some(true), Some(true)]
}

/// The through route: a Move whose destination lies *beyond* the falling
/// paratrooper's cell crosses it without stopping. Native has no kill on that
/// path: the entry walk latches the falling man (`Is_Crushable_By 0x5F6CD0`
/// at `0x73FB4A`, latch `0x73FB67`) and the entry tail admits an
/// infantry-only cell without asking the crush question (cell+0x124 bit 5
/// clear, `0x73FCF6..0x73FD37`); the only executor, the arrival loop
/// `0x007416A0`, skips `Object+8D` (`0x741811..0x741819`) whether the cell
/// was entered straight, through a chain turn (`0x004B1CFD`) or terminally
/// (`0x004B220F`).
///
/// The fixture map is flat open ground (`world_with(.., 64, &[])` builds
/// every cell from `test_terrain_cell`), so reachability is not the risk; the
/// geometry is. The crossing is forced by spawning the vehicle on the
/// victim's own row, three cells west, and sending it one cell east past him:
/// the straight row is the only cheap path, and `at` is an intermediate cell,
/// not the stop. Should the pathfinder still route around the occupied cell,
/// step 300 re-orders the vehicle INTO `at` itself — the covered tests prove
/// that entry reachable — and the diagnostics record `fallback=true`, so the
/// two routes stay distinguishable.
#[test]
fn a_vehicle_driving_through_the_cell_leaves_the_falling_infantryman_alive() {
    let Some((rules, mut sim, _idle, victim, at)) = drop() else {
        return;
    };
    let spawn_cell = (at.0.saturating_sub(3), at.1);
    let crosser = sim
        .spawn_object("SMCV", "Russians", spawn_cell.0, spawn_cell.1, 0, &rules)
        .unwrap_or_else(|| panic!("the row crosser spawns at {spawn_cell:?}"));
    let destination = (at.0.saturating_add(1), at.1);
    move_to(&mut sim, crosser, destination);
    let spawn = (i32::from(spawn_cell.0), i32::from(spawn_cell.1));
    let mut crossed = false;
    let mut moved = false;
    let mut landed = false;
    let mut fallback = false;
    let mut recent = [None; 2];
    let mut last_cell = spawn;
    for step in 0..900 {
        frame(&mut sim, &rules);
        // If the pathfinder routed around the occupied cell instead of
        // through it, force the entry the covered tests prove reachable:
        // order the vehicle INTO `at` while the man is still airborne.
        if !crossed && !landed && step == 300 {
            fallback = true;
            move_to(&mut sim, crosser, at);
        }
        let vehicle = sim.substrate.entities.get(crosser).unwrap();
        let [vx, vy] = super::ground_pose::position_world_xy(&vehicle.position);
        last_cell = (vx / 256, vy / 256);
        moved |= last_cell != spawn;
        crossed |= last_cell == (i32::from(at.0), i32::from(at.1));
        let was_airborne = still_airborne(recent);
        let Some(e) = sim.substrate.entities.get(victim) else {
            assert!(
                !was_airborne,
                "the through-driving vehicle removed the paratrooper mid-air: step {step}, \
                 crossed={crossed}, vehicle=({vx},{vy}), cell={at:?}"
            );
            // A removal after touchdown is the covered landing crush below.
            return;
        };
        let now_falling = e.is_falling_down();
        if !now_falling {
            landed = true;
        }
        recent = [recent[1], Some(now_falling)];
        let loco = vehicle.locomotor.as_ref().unwrap();
        if landed
            && crossed
            && loco.track_head(TrackFamily::Drive).is_none()
            && loco.track_destination(TrackFamily::Drive).is_none()
        {
            break;
        }
    }
    assert!(landed, "the paratrooper never landed; recent={recent:?}");
    assert!(
        crossed,
        "the vehicle never crossed the victim's cell {at:?}: moved={moved}, \
         fallback={fallback}, stopped at {last_cell:?}, spawn={spawn:?}, \
         destination={destination:?}"
    );
    let e = sim
        .substrate
        .entities
        .get(victim)
        .unwrap_or_else(|| panic!("the crossed paratrooper is gone after landing"));
    assert!(
        e.lifecycle.object_alive(),
        "the paratrooper did not survive the through drive"
    );
}

/// The computer route: a computer-owned crusher holding the falling
/// paratrooper as its Target runs `UnitClass::Approach_Target 0x007414E0`
/// every AI tick. Its computer arm measures the **3D**
/// `ObjectClass::Distance 0x005F6360` (`0x74154F..0x741560` against
/// `[CombatDamage] Crush=`), so a man high in his parachute is out of reach,
/// and either arm only sets the destination — the kill itself stays with the
/// arrival loop's `+0x8D` skip (`0x741811..0x741819`), so the chase must not
/// remove him while he falls.
#[test]
fn a_computer_crusher_chasing_the_falling_target_leaves_him_alive_while_he_falls() {
    let Some((rules, mut sim, crusher, victim, _at)) = drop() else {
        return;
    };
    let owner = sim.substrate.entities.get(crusher).unwrap().owner();
    sim.houses
        .get_mut(&owner)
        .unwrap()
        .set_is_human_for_test(false);
    sim.substrate
        .entities
        .get_mut(crusher)
        .unwrap()
        .attack_target
        .set_for_test(Some(crate::sim::combat::TargetKind::Entity(victim)));
    let now = sim.session.binary_frame();
    sim.substrate
        .mission_assign_exact(
            crusher,
            crate::sim::mission::MissionId::from_known(crate::sim::mission::MissionType::Attack),
            now,
        )
        .unwrap();
    let _ = sim.substrate.mission_commence_exact(crusher, now);
    let mut recent = [None; 2];
    let mut moved = false;
    for step in 0..2000 {
        // Keep the Target through any exhausted-arm clear, so the crush arms
        // are live on every tick the man spends low enough to be in reach.
        sim.substrate
            .entities
            .get_mut(crusher)
            .unwrap()
            .attack_target
            .set_for_test(Some(crate::sim::combat::TargetKind::Entity(victim)));
        frame(&mut sim, &rules);
        let vehicle = sim.substrate.entities.get(crusher).unwrap();
        moved |= vehicle
            .locomotor
            .as_ref()
            .unwrap()
            .track_head(TrackFamily::Drive)
            .is_some();
        let was_airborne = still_airborne(recent);
        let Some(e) = sim.substrate.entities.get(victim) else {
            assert!(
                !was_airborne,
                "the computer chase removed the paratrooper mid-air: step {step}, moved={moved}"
            );
            return;
        };
        let now_falling = e.is_falling_down();
        if was_airborne && !now_falling {
            return;
        }
        recent = [recent[1], Some(now_falling)];
    }
    panic!("the falling paratrooper never landed; moved={moved}");
}

#[test]
fn the_same_vehicle_crushes_a_naturally_landed_paratrooper() {
    let Some((rules, mut sim, crusher, victim, at)) = drop() else {
        return;
    };
    for _ in 0..1000 {
        if !sim
            .substrate
            .entities
            .get(victim)
            .unwrap()
            .is_falling_down()
        {
            break;
        }
        frame(&mut sim, &rules);
    }
    let e = sim.substrate.entities.get(victim).unwrap();
    assert!(e.lifecycle.object_alive() && !e.is_falling_down());
    assert_eq!(e.position.exact_z_leptons(), Some(0));
    assert_eq!((e.position.rx(), e.position.ry()), at);
    move_to(&mut sim, crusher, at);
    for _ in 0..240 {
        frame(&mut sim, &rules);
        if !sim.substrate.entities.contains(victim) {
            assert!(
                sim.sound_events
                    .iter()
                    .any(|e| matches!(e, SimSoundEvent::EntityCrushed { .. }))
            );
            return;
        }
    }
    panic!("the unarmed MCV must still crush the landed soldier");
}
