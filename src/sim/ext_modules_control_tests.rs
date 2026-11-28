//! The RFC 0001 0.0.22 control points and the step-one gaps on a retail
//! map: what `has_kind` lists, `FatalDamage` and `ExperienceGained` on a
//! kill without a damage commit, `TargetChanged` at the class setter,
//! `Promoted` at the rank sample, `Detonated` after a GI's shot, stock
//! `SpiedBy`/`ThievedBy` and the garrison exit events at their raises, the
//! `DamageIncoming`, `DestinationSet` and `FireAllowed` decisions, the
//! Jumpjet speed factor, an `ImmunityFlags` override at the radiation gate,
//! a standing GI's `DeployAllowed` refusal, `Crushed` and a `CrushAllowed`
//! refusal, the `Retaliate` request and its refusal, `Cloaked`/`Decloaked`
//! through the cloak modifiers, and Fly's and Rocket's speed factor.

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

/// `AttachAnim` with a frame count keeps its animation looping on the GI
/// until the module phase of the frame the count runs out, putting back one
/// the engine destroyed; with a negative count until `DetachAnim` of this
/// origin takes it.
#[test]
fn attach_anim_holds_for_its_frames_or_until_detached() {
    use super::actions::{ATTACH_ANIM, ActionContext, DETACH_ANIM, run_action};
    let Some((mut scene, _, gi)) = super::tests::gi_duel() else {
        return;
    };
    let runtime = &mut scene.runtime;
    let rules = &runtime.resources.rules;
    let sim = &mut runtime.simulation;
    let owned = |sim: &crate::sim::world::Simulation| {
        sim.substrate
            .anims
            .iter()
            .filter(|(_, anim)| anim.owner_entity() == Some(gi))
            .count()
    };
    let act = |sim: &mut crate::sim::world::Simulation, kind: i32, id: i32, value2: i64| {
        let params = vera20k_wasm_ext::ActionParams {
            id,
            value2,
            ..Default::default()
        };
        let ctx = ActionContext {
            rules,
            overlay_registry: Some(rules.overlay_registry()),
            frame_effects: &mut FrameEffects::default(),
            text: "",
            trigger_house: None,
            source: super::ModuleSource::Map,
        };
        run_action(sim, kind, gi as i64, &params, ctx).expect("valid action")
    };
    // The first animation that constructs on the GI.
    let Some(anim) = (0..64).find(|&index| {
        let before = owned(sim);
        rules.anim_type_runtime_key_by_index(index).is_some()
            && act(sim, ATTACH_ANIM, index, 3).1 == vera20k_wasm_ext::Status::Ok
            && owned(sim) > before
    }) else {
        panic!("no animation constructs on the GI");
    };
    let frame = sim.session.binary_frame();
    let held = owned(sim);
    // One the engine destroyed while its object shows is put back.
    let first = sim
        .substrate
        .anims
        .iter()
        .find(|(_, anim)| anim.owner_entity() == Some(gi))
        .map(|(id, _)| *id)
        .expect("the kept animation");
    sim.destroy_anim_with_context(first, rules);
    assert_eq!(owned(sim), held - 1);
    sim.session.set_binary_frame_for_test(frame + 1);
    sim.maintain_ext_module_anims(rules);
    assert_eq!(owned(sim), held, "put back while the GI shows");
    sim.session.set_binary_frame_for_test(frame + 2);
    sim.maintain_ext_module_anims(rules);
    assert_eq!(owned(sim), held, "kept before its time is up");
    sim.session.set_binary_frame_for_test(frame + 3);
    sim.maintain_ext_module_anims(rules);
    assert_eq!(owned(sim), held - 1, "destroyed when its time is up");
    let mut cloak = crate::sim::cloak_disguise::CloakRuntime::new(0);
    cloak.establish_unlimbo_fully_cloaked();
    sim.substrate.entities.get_mut(gi).expect("GI").cloak = Some(Box::new(cloak));
    assert_eq!(act(sim, ATTACH_ANIM, anim, -1).1, vera20k_wasm_ext::Status::Ok);
    assert_eq!(owned(sim), held - 1, "attaching to a cloaked object must not reveal it");
    sim.substrate.entities.get_mut(gi).expect("GI").cloak = None;
    sim.session.set_binary_frame_for_test(frame + 10_000);
    sim.maintain_ext_module_anims(rules);
    assert_eq!(owned(sim), held, "kept until detached");
    assert_eq!(act(sim, DETACH_ANIM, -1, 0).1, vera20k_wasm_ext::Status::Ok);
    assert_eq!(owned(sim), held - 1, "detached");
}

/// The module tint (appendix 7.2): `TintColor` takes only an override of
/// `0xRRGGBB` and `TintIntensity` only additions; the newest colour wins,
/// intensities sum, and both read back as properties.
#[test]
fn tint_modifiers_compose_into_the_module_tint() {
    use super::modifiers::{Modifier, ModifierOp, Stat};
    assert!(!Stat::TintColor.admits(ModifierOp::Add, 0xFF));
    assert!(!Stat::TintColor.admits(ModifierOp::Override, 0x100_0000));
    assert!(Stat::TintColor.admits(ModifierOp::Override, 0xFF_8000));
    assert!(!Stat::TintIntensity.admits(ModifierOp::Override, 1 << 16));
    assert!(Stat::TintIntensity.admits(ModifierOp::Add, -(1 << 15)));
    let Some((mut scene, _, target)) = super::tests::gi_duel() else {
        return;
    };
    let sim = &mut scene.runtime.simulation;
    assert_eq!(sim.entities().get(target).expect("GI").module_tint(), None);
    for (stat, op, value) in [
        (Stat::TintColor, ModifierOp::Override, 0x00_00FF),
        (Stat::TintColor, ModifierOp::Override, 0xFF_8000),
        (Stat::TintIntensity, ModifierOp::Add, 1 << 15),
        (Stat::TintIntensity, ModifierOp::Add, 1 << 14),
    ] {
        let handle = sim.substrate.allocate_stable_id();
        let modifier = Modifier {
            target,
            stat,
            op,
            value,
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
    let entity = sim.entities().get(target).expect("GI");
    assert_eq!(entity.module_tint(), Some(([0xFF, 0x80, 0x00], 0.75)));
    assert_eq!(entity.effective_stat(Stat::TintColor), 0xFF_8000);
    assert_eq!(entity.effective_stat(Stat::TintIntensity), 3 << 14);
}

/// `ImageOverride` draws a GI as another infantry type and reads back; a
/// vehicle type is another category and refused; 0 draws it as itself.
#[test]
fn image_override_takes_a_type_of_the_objects_category() {
    use vera20k_wasm_ext::{NAME_TECHNO_TYPE, SimHost, Status, property_id};
    let Some((mut scene, _, gi)) = super::tests::gi_duel() else {
        return;
    };
    let runtime = &mut scene.runtime;
    let rules = &runtime.resources.rules;
    let mut rng = crate::sim::rng::SimRng::new(0);
    let mut effects = FrameEffects::empty();
    let mut host = super::Host {
        sim: &mut runtime.simulation,
        rules,
        overlay_registry: None,
        frame_effects: &mut effects,
        rng: &mut rng,
        source: super::ModuleSource::Map,
        orders: &mut Vec::new(),
        rules_ini: None,
    };
    let prop = property_id("ImageOverride").expect("a property");
    let kind = |name: &str| {
        super::type_ref(NAME_TECHNO_TYPE, rules.type_handle(name).expect("retail type").0)
    };
    assert_eq!(host.set(gi as i64, prop, kind("E2")), Ok(Status::Ok));
    assert_eq!(host.get(gi as i64, prop), Ok((kind("E2"), Status::Ok)));
    assert_eq!(host.set(gi as i64, prop, kind("MTNK")), Ok(Status::Refused));
    assert_eq!(host.sim.ext_module_image(gi), Some("E2"));
    assert_eq!(host.set(gi as i64, prop, 0), Ok(Status::Ok));
    assert_eq!(host.get(gi as i64, prop), Ok((0, Status::Ok)));
}

/// A copy of the world for drawing keeps the variable declarations (the
/// status templates name variables by them) but no value.
#[test]
fn a_world_copy_keeps_the_variable_declarations() {
    use vera20k_wasm_ext::{VarDecl, VarScope, Visibility};
    let mut modules = super::ExtModules::default();
    let decl = VarDecl {
        name: "Level".into(),
        scope: VarScope::Object,
        visibility: Visibility::Public,
        default: 0,
    };
    modules
        .vars
        .declare(std::iter::once((super::ModuleSource::Mod, std::slice::from_ref(&decl))));
    let var = modules.vars.find(super::ModuleSource::Mod, "Level").expect("declared");
    modules.vars.set(var, super::vars::Holder::Object(7), 3);
    let copy = modules.clone();
    assert_eq!(copy.vars.find(super::ModuleSource::Mod, "Level"), Some(var));
    assert_eq!(copy.vars.object_values(7).count(), 0);
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

/// Sends `command` as the Americans' and advances up to `frames` frames,
/// stopping after the first one `done` holds for.
fn order_until(
    scene: &mut HeadlessScenario,
    command: crate::sim::command::Command,
    frames: usize,
    done: impl Fn(&HeadlessScenario) -> bool,
) {
    use crate::sim::command::CommandEnvelope;
    let order = {
        let sim = scene.sim();
        CommandEnvelope::new(
            sim.interner.get("Americans").expect("Americans house"),
            sim.session.tick() + 1,
            command,
        )
    };
    let mut orders = vec![order];
    for _ in 0..frames {
        scene
            .runtime
            .advance_frame(
                &std::mem::take(&mut orders),
                crate::headless_scenario::SIM_TICK_MS,
                crate::sim::world::TickLane::Ordinary,
                &mut FrameEffects::default(),
            )
            .expect("frame advances");
        if done(scene) {
            return;
        }
    }
}

/// Puts a map-origin modifier on `target`.
fn put_modifier(
    scene: &mut HeadlessScenario,
    target: u64,
    stat: super::modifiers::Stat,
    op: super::modifiers::ModifierOp,
    value: i64,
) {
    let sim = &mut scene.runtime.simulation;
    let handle = sim.substrate.allocate_stable_id();
    let modifier = super::modifiers::Modifier {
        target,
        stat,
        op,
        value,
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

/// The duel's American GI alone (the Russian one removed), standing:
/// (scene, GI).
fn lone_gi() -> Option<(HeadlessScenario, u64)> {
    let (mut scene, gi, other) = super::tests::gi_duel()?;
    let runtime = &mut scene.runtime;
    runtime
        .simulation
        .uninit_with_context(other, UninitContext::new(&runtime.resources.rules));
    Some((scene, gi))
}

fn deploying(scene: &HeadlessScenario, id: u64) -> bool {
    scene
        .sim()
        .entities()
        .get(id)
        .is_some_and(|entity| entity.infantry_deploy_doing())
}

/// A standing GI ordered to deploy (its Unload) deploys; with a
/// `DeployAllowed` decider refusing, the deploy is asked (`subject` the
/// GI) and he stays standing, his Unload ended in Guard.
#[test]
fn a_deploy_allowed_refusal_keeps_a_standing_gi_standing() {
    use crate::sim::command::Command;
    use crate::sim::mission::MissionType;
    let Some((mut scene, gi)) = lone_gi() else {
        return;
    };
    let order = |gi: u64| Command::ToggleInfantryDeploy { entity_id: gi };
    order_until(&mut scene, order(gi), 60, |scene| deploying(scene, gi));
    assert!(deploying(&scene, gi), "the GI deploys");

    let (mut scene, gi) = lone_gi().expect("second scene");
    install_decider(&mut scene, EVENT_DEPLOY_ALLOWED, REFUSE, gi as i64);
    order_until(&mut scene, order(gi), 60, |_| false);
    let m = memory(&scene);
    assert_eq!(
        i32::from_le_bytes(m[512 + 4..512 + 8].try_into().expect("4 bytes")),
        EVENT_DEPLOY_ALLOWED,
        "the deploy was asked"
    );
    assert_eq!(
        i64::from_le_bytes(m[512 + 24..512 + 32].try_into().expect("8 bytes")),
        gi as i64,
        "`subject` the GI"
    );
    assert!(!deploying(&scene, gi), "refused: he stays standing");
    let mission = scene
        .sim()
        .entities()
        .get(gi)
        .expect("the GI lives")
        .mission
        .effective()
        .known();
    assert_ne!(mission, Some(MissionType::Unload), "the Unload ended");
}

/// A GI duel scene with an American Grizzly east of the Russian GI and the
/// Move order that drives it over the GI: (scene, tank, victim, order).
fn crush_scene() -> Option<(HeadlessScenario, u64, u64, crate::sim::command::Command)> {
    let (mut scene, _, victim) = super::tests::gi_duel()?;
    let (x, y) = {
        let entity = scene.sim().entities().get(victim)?;
        (entity.position.rx(), entity.position.ry())
    };
    let runtime = &mut scene.runtime;
    let rules = &runtime.resources.rules;
    let tank = runtime
        .simulation
        .spawn_object("MTNK", "Americans", x + 1, y, 192, rules)?;
    runtime.simulation.resolve_type_handles(rules);
    let order = crate::sim::command::Command::Move {
        entity_id: tank,
        target_rx: x - 1,
        target_ry: y,
    };
    Some((scene, tank, victim, order))
}

fn alive(scene: &HeadlessScenario, id: u64) -> bool {
    scene
        .sim()
        .entities()
        .get(id)
        .is_some_and(|entity| entity.lifecycle.object_alive())
}

/// A Grizzly driven over an enemy GI crushes it: `Crushed` on the GI,
/// `other` the tank. A `CrushAllowed` refusal leaves the GI alive.
#[test]
fn a_crush_raises_crushed_and_crush_allowed_can_spare_the_victim() {
    let Some((scene, tank, victim, order)) = crush_scene() else {
        return;
    };
    let crushed = def(0, 2, &[Ev::on(EVENT_CRUSHED)], &[]);
    let mut scene = installed(scene, &module(&crushed, victim as i64, ""));
    order_until(&mut scene, order, 150, |scene| received(scene) > 0);
    assert_eq!(received(&scene), 1, "the GI was crushed");
    let (kind, subject, _, _, _) = first_record(&scene);
    assert_eq!((kind, subject), (EVENT_CRUSHED, victim as i64));
    assert_eq!(first_other(&scene), tank as i64);
    assert!(!alive(&scene, victim));

    let (mut scene, tank, victim, order) = crush_scene().expect("second scene");
    install_decider(&mut scene, EVENT_CRUSH_ALLOWED, REFUSE, victim as i64);
    let goal = match order {
        crate::sim::command::Command::Move {
            target_rx,
            target_ry,
            ..
        } => (target_rx, target_ry),
        _ => unreachable!(),
    };
    order_until(&mut scene, order, 150, |scene| {
        scene.sim().entities().get(tank).is_some_and(|entity| {
            (entity.position.rx(), entity.position.ry()) == goal
                && entity.navigation.nav_com().is_none()
        })
    });
    assert_eq!(
        i32::from_le_bytes(
            memory(&scene)[512 + 4..512 + 8]
                .try_into()
                .expect("4 bytes")
        ),
        EVENT_CRUSH_ALLOWED,
        "the crush was asked"
    );
    assert_eq!(
        i64::from_le_bytes(
            memory(&scene)[512 + 32..512 + 40]
                .try_into()
                .expect("8 bytes")
        ),
        tank as i64,
        "`other` the crusher"
    );
    assert!(alive(&scene, victim), "spared");
}

/// A hit with a source asks `Retaliate` on the victim (`other` the
/// source, `id` the warhead, `native` the engine's answer); a refusal
/// leaves the victim's target as it was.
#[test]
fn a_retaliate_refusal_keeps_the_victims_target() {
    let hit_from = |scene: &mut HeadlessScenario, victim: u64, source: u64| {
        let runtime = &mut scene.runtime;
        let rules = &runtime.resources.rules;
        let warhead = runtime.simulation.interner.intern("SA");
        let event = EntityDamageEvent::direct_receiver(
            victim,
            10,
            0,
            source,
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
    };
    let target_of = |scene: &HeadlessScenario, id: u64| {
        scene
            .sim()
            .entities()
            .get(id)
            .expect("lives")
            .attack_target
            .target()
    };
    let Some((mut scene, attacker, victim)) = super::tests::gi_duel() else {
        return;
    };
    install_decider(&mut scene, EVENT_RETALIATE, REFUSE, victim as i64);
    let before = target_of(&scene, victim);
    hit_from(&mut scene, victim, attacker);
    let m = memory(&scene);
    let i64_at = |at: usize| i64::from_le_bytes(m[512 + at..512 + at + 8].try_into().unwrap());
    let i32_at = |at: usize| i32::from_le_bytes(m[512 + at..512 + at + 4].try_into().unwrap());
    assert_eq!(i32_at(4), EVENT_RETALIATE);
    assert_eq!((i64_at(24), i64_at(32)), (victim as i64, attacker as i64));
    let sa = scene
        .runtime
        .resources
        .rules
        .warhead_index("SA")
        .map_or(-1, |index| index as i32);
    assert_eq!(i32_at(88), sa, "`id` the warhead");
    let native = i64_at(120);
    assert_eq!(target_of(&scene, victim), before, "refused: no new target");

    let (mut scene, attacker, victim) = super::tests::gi_duel().expect("second scene");
    scene.tick();
    let before = target_of(&scene, victim);
    hit_from(&mut scene, victim, attacker);
    let attacked = Some(TargetKind::Entity(attacker));
    if native == 1 && before != attacked {
        assert_eq!(target_of(&scene, victim), attacked, "natively retaliates");
    }
}

/// A Wasm module `Cloakable` override cloaks a unit (`Cloaked`), and a
/// `ForceDecloak` override then uncloaks it (`Decloaked`).
#[test]
fn cloakable_and_force_decloak_raise_the_cloak_edges() {
    use super::modifiers::{ModifierOp, Stat};
    for decloak in [false, true] {
        let Some(scene) = scenario() else { return };
        let (unit, _) = ground_unit(&scene);
        let kind = if decloak {
            EVENT_DECLOAKED
        } else {
            EVENT_CLOAKED
        };
        let edge = def(0, 2, &[Ev::on(kind)], &[]);
        let mut scene = installed(scene, &module(&edge, unit as i64, ""));
        put_modifier(&mut scene, unit, Stat::Cloakable, ModifierOp::Override, 1);
        for _ in 0..300 {
            scene.tick();
            if !decloak && received(&scene) > 0 {
                break;
            }
        }
        if decloak {
            // The cloak has had its time; force it off.
            put_modifier(
                &mut scene,
                unit,
                Stat::ForceDecloak,
                ModifierOp::Override,
                1,
            );
            for _ in 0..300 {
                scene.tick();
                if received(&scene) > 0 {
                    break;
                }
            }
        }
        assert!(received(&scene) > 0, "event {kind}");
        let (got, subject, _, _, _) = first_record(&scene);
        assert_eq!((got, subject), (kind, unit as i64));
    }
}

/// `Cloaked` reads CloakState 2: 0 at first, 1 once a `Cloakable`
/// override has cloaked the unit, 0 again after a `ForceDecloak` override.
#[test]
fn cloaked_reads_the_cloak_state() {
    use super::modifiers::{ModifierOp, Stat};
    let Some(scene) = scenario() else { return };
    let (unit, _) = ground_unit(&scene);
    let edge = def(0, 2, &[Ev::on(EVENT_CLOAKED)], &[]);
    let mut scene = installed(scene, &module(&edge, unit as i64, ""));
    let cloaked = |scene: &mut HeadlessScenario| {
        let runtime = &mut scene.runtime;
        runtime
            .simulation
            .with_module_host(&runtime.resources.rules, super::ModuleSource::Map, |host| {
                host.reader().read(unit as i64, super::tests::prop("Cloaked"))
            })
            .expect("a valid read")
    };
    let ok = |value| (value, vera20k_wasm_ext::Status::Ok);
    assert_eq!(cloaked(&mut scene), ok(0));
    put_modifier(&mut scene, unit, Stat::Cloakable, ModifierOp::Override, 1);
    let on = (0..300).any(|_| {
        scene.tick();
        cloaked(&mut scene) == ok(1)
    });
    assert!(on, "the unit cloaks");
    put_modifier(
        &mut scene,
        unit,
        Stat::ForceDecloak,
        ModifierOp::Override,
        1,
    );
    let off = (0..300).any(|_| {
        scene.tick();
        cloaked(&mut scene) == ok(0)
    });
    assert!(off, "the unit uncloaks");
}

/// Fly's paid speed and Rocket's type speed scale by the module
/// `SpeedMultiplier` alone, truncated toward zero; without one they stay.
#[test]
fn fly_and_rocket_speeds_scale_by_the_module_speed_multiplier() {
    use super::modifiers::{ModifierOp, Stat};
    let Some(mut scene) = scenario() else { return };
    let (unit, _) = ground_unit(&scene);
    let scaled = |scene: &HeadlessScenario, speed: i32| {
        scene
            .sim()
            .entities()
            .get(unit)
            .expect("unit")
            .module_scaled_speed(speed)
    };
    assert_eq!(scaled(&scene, 41), 41);
    put_modifier(&mut scene, unit, Stat::Speed, ModifierOp::Multiply, 1 << 15);
    assert_eq!(scaled(&scene, 41), 20, "half, truncated");
    assert_eq!(scaled(&scene, -41), -20, "toward zero");
}
