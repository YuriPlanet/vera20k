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
