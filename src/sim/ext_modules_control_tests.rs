//! The RFC 0001 0.0.22 control points and the step-one gaps on a retail
//! map: what `has_kind` lists, `FatalDamage` and `ExperienceGained` on a
//! kill without a damage commit, `TargetChanged` at the class setter,
//! `Promoted` at the rank sample, `Detonated` after a GI's shot, stock
//! `SpiedBy`/`ThievedBy` and the garrison exit events at their raises, the
//! `DamageIncoming`, `DestinationSet` and `FireAllowed` decisions, and the
//! Jumpjet speed factor.

use vera20k_wasm_ext::decide::{
    EVENT_CRUSH_ALLOWED, EVENT_DAMAGE_INCOMING, EVENT_DEPLOY_ALLOWED, EVENT_DESTINATION_SET,
    EVENT_FIRE_ALLOWED, EVENT_HARVEST_CELL, EVENT_RETALIATE,
};
use vera20k_wasm_ext::triggers::{
    EVENT_CLOAKED, EVENT_CRUSHED, EVENT_DECLOAKED, EVENT_DEPLOYED, EVENT_DETONATED,
    EVENT_EXPERIENCE_GAINED, EVENT_FATAL_DAMAGE, EVENT_MISSION_CHANGED, EVENT_PASSENGER_REMOVED,
    EVENT_PROMOTED, EVENT_TARGET_CHANGED, EVENT_TRANSPORT_EXITED,
};

use super::decide_tests::{REFUSE, ground_unit, install_decider};
use super::event_tests::{Ev, def, first_record, installed, module};
use super::house_ref;
use super::trigger_tests::{memory, received, scenario, sturdy_object};
use crate::headless_scenario::HeadlessScenario;
use crate::sim::combat::{
    EntityDamageEvent, KillCallback, KillSource, RAD_NO_ATTACKER, ReceiverCallFlags, TargetKind,
};
use crate::sim::world::{FrameEffects, UninitContext};

/// The first delivered record's `other`.
fn first_other(scene: &HeadlessScenario) -> i64 {
    let m = memory(scene);
    i64::from_le_bytes(m[512 + 32..512 + 40].try_into().expect("8 bytes"))
}

/// The first delivered record's `value2`.
fn first_value2(scene: &HeadlessScenario) -> i64 {
    let m = memory(scene);
    i64::from_le_bytes(m[512 + 80..512 + 88].try_into().expect("8 bytes"))
}

fn health(scene: &HeadlessScenario, id: u64) -> i32 {
    scene
        .sim()
        .entities()
        .get(id)
        .expect("object lives")
        .health
        .current()
}

/// A direct hit of `damage` with the `SA` warhead that the defenses read
/// (not IgnoreDefenses), so `DamageIncoming` is asked.
fn defended_hit(scene: &mut HeadlessScenario, victim: u64, damage: i32) {
    let runtime = &mut scene.runtime;
    let rules = &runtime.resources.rules;
    let warhead = runtime.simulation.interner.intern("SA");
    let event = EntityDamageEvent::direct_receiver(
        victim,
        damage,
        0,
        RAD_NO_ATTACKER,
        None,
        warhead,
        ReceiverCallFlags {
            ignore_defenses: false,
            arg6: false,
        },
    );
    runtime.simulation.commit_direct_damage_receiver(
        rules,
        Some(rules.overlay_registry()),
        event,
        &mut FrameEffects::default(),
    );
}

/// `has_kind` lists the delivered events this change raises and the
/// decision-only ones it asks, stock 2 and 3, and not `MissionChanged`,
/// which has no raise point yet.
#[test]
fn has_kind_lists_the_control_point_events() {
    let Some(scene) = scenario() else { return };
    let rules = &scene.runtime.resources.rules;
    for kind in [
        2,
        3,
        EVENT_TARGET_CHANGED,
        EVENT_DETONATED,
        EVENT_CRUSHED,
        EVENT_DEPLOYED,
        EVENT_PROMOTED,
        EVENT_EXPERIENCE_GAINED,
        EVENT_CLOAKED,
        EVENT_DECLOAKED,
        EVENT_DAMAGE_INCOMING,
        EVENT_FIRE_ALLOWED,
        EVENT_RETALIATE,
        EVENT_DESTINATION_SET,
        EVENT_CRUSH_ALLOWED,
        EVENT_HARVEST_CELL,
        EVENT_DEPLOY_ALLOWED,
    ] {
        assert!(super::has_kind(rules, 0, kind), "event {kind}");
    }
    assert!(!super::has_kind(rules, 0, EVENT_MISSION_CHANGED));
}

/// A kill recorded without a damage commit (as a crush or a Temporal erase
/// records it) raises `FatalDamage` with the Strength left.
#[test]
fn a_kill_without_a_damage_commit_raises_fatal_damage() {
    let Some(scene) = scenario() else { return };
    let victim = sturdy_object(&scene);
    let fatal = def(0, 2, &[Ev::on(EVENT_FATAL_DAMAGE)], &[]);
    let mut scene = installed(scene, &module(&fatal, victim as i64, ""));
    let left = health(&scene, victim);
    let runtime = &mut scene.runtime;
    runtime.simulation.record_the_kill(
        victim,
        KillSource::Techno(None),
        KillCallback::Terminal,
        UninitContext::new(&runtime.resources.rules),
    );
    scene.tick();
    assert_eq!(received(&scene), 1);
    let (kind, subject, _, value, id) = first_record(&scene);
    assert_eq!(
        (kind, subject, value, id),
        (EVENT_FATAL_DAMAGE, victim as i64, i64::from(left), -1)
    );
}

/// The kill of an enemy GI gives the other GI experience: `ExperienceGained`
/// on it, `other` the victim, a positive gain.
#[test]
fn a_kill_raises_experience_gained_on_its_recipient() {
    let Some((scene, attacker, target)) = super::tests::gi_duel() else {
        return;
    };
    let gained = def(0, 2, &[Ev::on(EVENT_EXPERIENCE_GAINED)], &[]);
    let mut scene = installed(scene, &module(&gained, attacker as i64, ""));
    let runtime = &mut scene.runtime;
    runtime.simulation.record_the_kill(
        target,
        KillSource::Techno(Some(attacker)),
        KillCallback::Terminal,
        UninitContext::new(&runtime.resources.rules),
    );
    scene.tick();
    assert_eq!(received(&scene), 1);
    let (kind, subject, _, value, _) = first_record(&scene);
    assert_eq!((kind, subject), (EVENT_EXPERIENCE_GAINED, attacker as i64));
    assert_eq!(first_other(&scene), target as i64);
    assert!(value > 0, "the gain {value}");
}

/// The class setter's store raises `TargetChanged` with the new cell
/// target (`other` 0, `cell` the cell).
#[test]
fn the_target_setter_raises_target_changed() {
    let Some(scene) = scenario() else { return };
    let (unit, _) = ground_unit(&scene);
    let changed = def(0, 2, &[Ev::on(EVENT_TARGET_CHANGED)], &[]);
    let mut scene = installed(scene, &module(&changed, unit as i64, ""));
    let entity = scene.sim().entities().get(unit).expect("unit");
    let cell = (entity.position.rx().saturating_add(1), entity.position.ry());
    let assign = |scene: &mut HeadlessScenario| {
        let runtime = &mut scene.runtime;
        runtime
            .simulation
            .assign_target_represented(
                unit,
                Some(TargetKind::Cell(cell.0, cell.1)),
                &runtime.resources.rules,
                &mut FrameEffects::default(),
            )
            .expect("the unit takes a target");
    };
    assign(&mut scene);
    // The unit's own AI may change the target again in the frame; the first
    // record is the setter's.
    scene.tick();
    assert!(received(&scene) >= 1);
    let (kind, subject, _, _, _) = first_record(&scene);
    assert_eq!((kind, subject), (EVENT_TARGET_CHANGED, unit as i64));
    assert_eq!(first_other(&scene), 0);
    let m = memory(&scene);
    let record_cell = i64::from_le_bytes(m[512 + 64..512 + 72].try_into().expect("8 bytes"));
    assert_eq!(record_cell, super::cell_ref(cell.0, cell.1));
}

/// An object whose experience reaches elite is promoted at its next AI
/// sample: `Promoted` with `value` 2.
#[test]
fn a_rank_sample_raises_promoted() {
    let Some(scene) = scenario() else { return };
    let (unit, _) = ground_unit(&scene);
    let promoted = def(0, 2, &[Ev::on(EVENT_PROMOTED)], &[]);
    let mut scene = installed(scene, &module(&promoted, unit as i64, ""));
    let elite = crate::util::native_x87::NativeF32Bits::from_bits(2.0f32.to_bits());
    scene
        .runtime
        .simulation
        .substrate
        .entities
        .get_mut(unit)
        .expect("unit")
        .veterancy
        .inherit_raw(elite);
    for _ in 0..3 {
        scene.tick();
    }
    assert_eq!(received(&scene), 1);
    let (kind, subject, _, value, _) = first_record(&scene);
    assert_eq!((kind, subject, value), (EVENT_PROMOTED, unit as i64, 2));
}

/// A GI's shot detonates its projectile: `Detonated` on the shooter, `id`
/// the warhead.
#[test]
fn a_shot_raises_detonated_on_its_shooter() {
    let Some((scene, attacker, target)) = super::tests::gi_duel() else {
        return;
    };
    let detonated = def(0, 2, &[Ev::on(EVENT_DETONATED)], &[]);
    let mut scene = installed(scene, &module(&detonated, attacker as i64, ""));
    super::tests::first_hit(&mut scene, attacker, target, |_| {});
    scene.tick();
    assert!(received(&scene) >= 1);
    let (kind, subject, _, _, warhead) = first_record(&scene);
    assert_eq!((kind, subject), (EVENT_DETONATED, attacker as i64));
    assert!(warhead >= 0, "the warhead {warhead}");
}

/// Stock `SpiedBy` and `ThievedBy` reach a module trigger bound to the
/// building or vehicle; `ThievedBy` holds only for the thief's house.
#[test]
fn spied_and_thieved_reach_module_triggers() {
    let Some((scene, attacker, target)) = super::tests::gi_duel() else {
        return;
    };
    let spied = def(0, 2, &[Ev::on(2)], &[]);
    let mut scene = installed(scene, &module(&spied, target as i64, ""));
    {
        let runtime = &mut scene.runtime;
        runtime.simulation.raise_module_infiltration_event(
            &runtime.resources.rules,
            &mut FrameEffects::default(),
            2,
            target,
            attacker,
        );
    }
    scene.tick();
    assert_eq!(received(&scene), 1);
    assert_eq!(first_record(&scene).0, 2);
    assert_eq!(first_other(&scene), attacker as i64);

    let thieved = |house: usize| {
        def(
            0,
            2,
            &[Ev {
                value: house as i64,
                ..Ev::on(3)
            }],
            &[],
        )
    };
    for thiefs_own in [false, true] {
        let Some((scene, attacker, target)) = super::tests::gi_duel() else {
            return;
        };
        let sim = scene.sim();
        let thief_house = sim
            .houses
            .index_of(sim.entities().get(attacker).expect("thief").owner())
            .expect("thief house");
        let house = if thiefs_own {
            thief_house
        } else {
            (thief_house + 1) % sim.houses.order().len()
        };
        let fires = i32::from(thiefs_own);
        let mut scene = installed(scene, &module(&thieved(house), target as i64, ""));
        {
            let runtime = &mut scene.runtime;
            runtime.simulation.raise_module_infiltration_event(
                &runtime.resources.rules,
                &mut FrameEffects::default(),
                3,
                target,
                attacker,
            );
        }
        scene.tick();
        assert_eq!(received(&scene), fires, "value house {house}");
        if fires == 1 {
            assert_eq!(first_record(&scene).2, house_ref(thief_house));
        }
    }
}

/// An occupant leaving a garrison raises `TransportExited` on itself and
/// `PassengerRemoved` on the building, `value2` 1.
#[test]
fn a_garrison_exit_raises_the_exit_events() {
    for exited in [true, false] {
        let Some((fresh, attacker, target)) = super::tests::gi_duel() else {
            return;
        };
        let (kind, bound, other) = if exited {
            (EVENT_TRANSPORT_EXITED, attacker, target)
        } else {
            (EVENT_PASSENGER_REMOVED, target, attacker)
        };
        let exit = def(0, 2, &[Ev::on(kind)], &[]);
        let mut scene = installed(fresh, &module(&exit, bound as i64, ""));
        {
            let runtime = &mut scene.runtime;
            // `target` stands in for the building, `attacker` for the
            // occupant, three occupants left inside.
            runtime.simulation.raise_module_garrison_left(
                &runtime.resources.rules,
                &mut FrameEffects::default(),
                target,
                attacker,
                3,
            );
        }
        scene.tick();
        assert_eq!(received(&scene), 1);
        let (got, subject, _, value, _) = first_record(&scene);
        assert_eq!((got, subject, value), (kind, bound as i64, 3));
        assert_eq!(first_other(&scene), other as i64);
        assert_eq!(first_value2(&scene), 1);
    }
}

/// A `DamageIncoming` decider answering 0 stops a defended hit, which
/// otherwise hurts; an IgnoreDefenses hit is not asked.
#[test]
fn a_damage_incoming_refusal_stops_a_defended_hit() {
    let Some(mut plain) = scenario() else { return };
    let victim = sturdy_object(&plain);
    let before = health(&plain, victim);
    defended_hit(&mut plain, victim, 40);
    assert!(health(&plain, victim) < before, "the hit hurts");

    let Some(mut scene) = scenario() else { return };
    install_decider(&mut scene, EVENT_DAMAGE_INCOMING, REFUSE, victim as i64);
    let before = health(&scene, victim);
    defended_hit(&mut scene, victim, 40);
    assert_eq!(health(&scene, victim), before, "refused");
    // The request carried the incoming damage as `value`.
    let m = scene.sim().ext_modules.slots[0].module.memory().to_vec();
    let value = i64::from_le_bytes(m[512 + 72..512 + 80].try_into().expect("8 bytes"));
    assert_eq!(value, 40);
}

/// A `DestinationSet` decider answering 0 leaves a move order without a
/// destination.
#[test]
fn a_destination_set_refusal_drops_the_destination() {
    use crate::sim::components::NavTargetRef;
    let Some(mut scene) = scenario() else { return };
    let (unit, _) = ground_unit(&scene);
    install_decider(&mut scene, EVENT_DESTINATION_SET, REFUSE, unit as i64);
    let entity = scene.sim().entities().get(unit).expect("unit");
    let (x, y) = (entity.position.rx(), entity.position.ry());
    let runtime = &mut scene.runtime;
    runtime
        .simulation
        .assign_destination_represented(
            unit,
            Some(NavTargetRef::cell(x.saturating_add(3), y)),
            &runtime.resources.rules,
            &mut FrameEffects::default(),
        )
        .expect("the unit takes a destination");
    assert_eq!(
        scene
            .sim()
            .entities()
            .get(unit)
            .expect("unit")
            .navigation
            .nav_com(),
        None
    );
}

/// A `FireAllowed` decider refusing every shot of the attacker keeps its
/// target unhurt through an attack order.
#[test]
fn a_fire_allowed_refusal_keeps_the_target_unhurt() {
    use crate::sim::command::{Command, CommandEnvelope};
    let Some((mut scene, attacker, target)) = super::tests::gi_duel() else {
        return;
    };
    install_decider(&mut scene, EVENT_FIRE_ALLOWED, REFUSE, attacker as i64);
    let full = health(&scene, target);
    let order = {
        let sim = scene.sim();
        CommandEnvelope::new(
            sim.interner.get("Americans").expect("Americans house"),
            sim.session.tick() + 1,
            Command::clicked_mission(
                attacker,
                crate::sim::mission::MissionType::Attack,
                TargetKind::Entity(target),
            ),
        )
    };
    let mut orders = vec![order];
    for _ in 0..300 {
        scene
            .runtime
            .advance_frame(
                &std::mem::take(&mut orders),
                crate::headless_scenario::SIM_TICK_MS,
                crate::sim::world::TickLane::Ordinary,
                &mut FrameEffects::default(),
            )
            .expect("frame advances");
    }
    assert_eq!(health(&scene, target), full);
    assert_eq!(scene.sim().ext_modules().status()[0].1, None);
}

/// A Wasm module `SpeedMultiplier` scales `JumpjetSpeed=` for the flight;
/// 1.0 leaves it.
#[test]
fn the_jumpjet_speed_factor_scales_its_speed() {
    use crate::sim::movement::jumpjet_movement::jumpjet_flight::JumpjetFlightParams;
    let params = JumpjetFlightParams::default();
    assert_eq!(params.with_speed_factor(1.0), params);
    assert_eq!(
        params.with_speed_factor(0.5).speed(),
        (f64::from(params.speed()) * 0.5) as i32
    );
}

/// A Wasm module `ImmunityFlags` override of bit 1 makes a GI immune to a
/// radiation warhead's direct hit, which hurts it without one, and shows in
/// the property; the mask takes only an override of its seven bits.
#[test]
fn an_immunity_override_nullifies_a_radiation_hit() {
    use super::modifiers::{Immunity, Modifier, ModifierOp, Stat};
    assert!(!Stat::Immunity.admits(ModifierOp::Add, 2));
    assert!(!Stat::Immunity.admits(ModifierOp::Multiply, 1 << 16));
    assert!(!Stat::Immunity.admits(ModifierOp::Override, 1 << 7));
    assert!(Stat::Immunity.admits(ModifierOp::Override, 0x7F));
    // The GI's health before and after the hit, and its `ImmunityFlags`.
    let hit = |immune: bool| -> Option<(i32, i32, i64)> {
        let (mut scene, _, target) = super::tests::gi_duel()?;
        let runtime = &mut scene.runtime;
        let rules = &runtime.resources.rules;
        let sim = &mut runtime.simulation;
        let object = sim.entity_object_type(sim.entities().get(target)?, rules)?;
        let armor = object.armor_id();
        let warhead = rules
            .warheads_iter()
            .filter(|warhead| {
                warhead.radiation
                    && f64::from_bits(warhead.interaction(armor).multiplier().bits()) > 0.0
            })
            .map(|warhead| warhead.id.clone())
            .min()?;
        if immune {
            let handle = sim.substrate.allocate_stable_id();
            let modifier = Modifier {
                target,
                stat: Stat::Immunity,
                op: ModifierOp::Override,
                value: 1 << Immunity::Radiation as u32,
                expires: None,
                source: 0,
                group: 0,
                origin: super::ModuleSource::Map,
            };
            sim.ext_modules
                .modifiers
                .insert(&mut sim.substrate.entities, handle, modifier)
                .expect("the modifier fits");
        }
        let entity = sim.entities().get(target)?;
        let flags = entity.immunity_flags(object, rules);
        let before = entity.health.current();
        let warhead = sim.interner.intern(&warhead);
        let event = EntityDamageEvent::direct_receiver(
            target,
            30,
            0,
            RAD_NO_ATTACKER,
            None,
            warhead,
            ReceiverCallFlags {
                ignore_defenses: false,
                arg6: false,
            },
        );
        sim.commit_direct_damage_receiver(
            rules,
            Some(rules.overlay_registry()),
            event,
            &mut FrameEffects::default(),
        );
        Some((before, sim.entities().get(target)?.health.current(), flags))
    };
    let Some((before, after, flags)) = hit(false) else {
        return;
    };
    assert!(after < before, "the radiation hit hurts a GI");
    assert_eq!(flags & 0b10, 0, "a GI is not immune to radiation");
    let (before, after, flags) = hit(true).expect("second scene");
    assert_eq!(after, before, "immune by the override");
    assert_eq!(flags, 0b10, "the override replaces the type's flags");
}
