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
    sim.input_delay_ticks = 0;
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
                Some(&rules),
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
