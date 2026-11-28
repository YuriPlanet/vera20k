//! Module trigger events beyond the first four, on a retail map: a stock
//! House event from its poll, `HouseTransferred` (and its mind-control
//! value), `PropertyPoll`, a `VariablePoll` whose interval a variable change
//! restarts, `DistancePoll`, `FatalDamage`, `Timer` counted on a bound
//! object, weapon and projectile bindings, and the chain depth cap.

use vera20k_wasm_ext::triggers::{
    ACTION_MODULE, EVENT_DISTANCE_POLL, EVENT_FATAL_DAMAGE, EVENT_FIRE, EVENT_HOUSE_TRANSFERRED,
    EVENT_MODIFIER_COUNT_POLL, EVENT_PROPERTY_POLL, EVENT_TAKE_DAMAGE, EVENT_TIMER,
    EVENT_VARIABLE_POLL,
};

use super::trigger_tests::{hit, memory, received, scenario, sturdy_object};
use super::{ModuleSource, house_ref};
use crate::headless_scenario::HeadlessScenario;
use crate::sim::world::FrameEffects;

/// The module action's dispatch key.
const KEY: i32 = 7;
/// Atomic `Damage` (appendix 5.3 entry 6).
const ACTION_DAMAGE: i32 = 1006;

/// One `EventDef` without clauses.
pub(super) struct Ev {
    pub(super) kind: i32,
    pub(super) op: i32,
    pub(super) interval: i32,
    pub(super) id: i32,
    pub(super) value: i64,
}

impl Ev {
    pub(super) const fn on(kind: i32) -> Self {
        Self {
            kind,
            op: 0,
            interval: 0,
            id: -1,
            value: 0,
        }
    }
}

fn i32s(values: &[i32]) -> Vec<u8> {
    values.iter().flat_map(|v| v.to_le_bytes()).collect()
}

/// A `TriggerDef` for every difficulty: `house`, `repeat`, `events`, a
/// `Module` action (key [`KEY`]) and then `actions` as (kind, target, id,
/// value).
pub(super) fn def(
    house: i64,
    repeat: i32,
    events: &[Ev],
    actions: &[(i32, i32, i32, i64)],
) -> Vec<u8> {
    let mut out = house.to_le_bytes().to_vec();
    out.extend(i32s(&[
        repeat,
        7,
        0,
        -1,
        events.len() as i32,
        actions.len() as i32 + 1,
    ]));
    for event in events {
        out.extend(i32s(&[event.kind, event.op, event.interval, event.id]));
        out.extend(event.value.to_le_bytes());
        out.extend(i32s(&[0, 0]));
    }
    for &(kind, target, id, value) in [(ACTION_MODULE, 0, KEY, 0)].iter().chain(actions) {
        out.extend(i32s(&[kind, target, id, 0, 0, 0]));
        out.extend(value.to_le_bytes());
        out.extend([0; 72 - 24]);
    }
    out
}

/// Creates `def` in `on_scenario_start` (handle at 64, status at 76) and,
/// unless `target` is 0, binds it to `target` (status at 72); `on_events`
/// copies the first record it gets to 512 and counts records at 80, as
/// `trigger_tests` modules do.
pub(super) fn module(def: &[u8], target: i64, sections: &str) -> Vec<u8> {
    let bytes = |b: &[u8]| b.iter().map(|b| format!("\\{b:02x}")).collect::<String>();
    let bind = if target == 0 {
        String::new()
    } else {
        format!(
            "(i32.store (i32.const 72) (call $bind (i64.load (i32.const 64)) (i64.const {target})))"
        )
    };
    wat::parse_str(format!(
        r#"(module
  (import "vera:sim@1.0" "trigger_create" (func $create (param i32 i32 i32) (result i32)))
  (import "vera:sim@1.0" "trigger_bind" (func $bind (param i64 i64) (result i32)))
  (@custom "vera.delivery" "\01\00\04\00\00\80\03\00\00")
  {sections}
  (memory (export "memory") 1)
  (data (i32.const 2048) "{def}")
  (func (export "on_scenario_start")
    (i32.store (i32.const 76) (call $create (i32.const 2048) (i32.const {len}) (i32.const 64)))
    {bind})
  (func (export "on_events") (param $ptr i32) (param $count i32)
    (if (i32.eqz (i32.load (i32.const 80)))
      (then (memory.copy (i32.const 512) (local.get $ptr) (i32.const 120))))
    (i32.store (i32.const 80) (i32.add (i32.load (i32.const 80)) (local.get $count)))))"#,
        def = bytes(def),
        len = def.len(),
    ))
    .expect("test module parses")
}

/// Installs `module` as the map's and runs the frame that creates its
/// trigger; create and bind answered `Ok`.
pub(super) fn installed(mut scene: HeadlessScenario, module: &[u8]) -> HeadlessScenario {
    scene
        .runtime
        .simulation
        .install_ext_module(&scene.runtime.resources.rules, ModuleSource::Map, module)
        .expect("module installs");
    scene.tick();
    assert_eq!(
        &memory(&scene)[72..80],
        &[0; 8],
        "create and bind answer Ok"
    );
    scene
}

/// The first delivered record's `(kind, subject, house, value, id)`.
pub(super) fn first_record(scene: &HeadlessScenario) -> (i32, i64, i64, i64, i32) {
    let m = memory(scene);
    let i32_at = |at: usize| i32::from_le_bytes(m[at..at + 4].try_into().expect("4 bytes"));
    let i64_at = |at: usize| i64::from_le_bytes(m[at..at + 8].try_into().expect("8 bytes"));
    (
        i32_at(512 + 4),
        i64_at(512 + 24),
        i64_at(512 + 40),
        i64_at(512 + 72),
        i32_at(512 + 88),
    )
}

fn ticks(scene: &mut HeadlessScenario, count: usize) {
    for _ in 0..count {
        scene.tick();
    }
}

/// Stock `CreditsExceed` (12) on a global trigger with a fixed house: the
/// House poll reads the house's available money through the map trigger
/// predicate and fires once (one-time) within eight frames, the record
/// carrying the house. `has_kind` lists it and not `RandomDelay` (51).
#[test]
fn a_stock_credits_event_fires_from_the_house_poll() {
    let Some(scene) = scenario() else { return };
    let rules = &scene.runtime.resources.rules;
    assert!(super::has_kind(rules, 0, 12));
    assert!(!super::has_kind(rules, 0, 51));
    assert!(super::has_kind(rules, 0, EVENT_HOUSE_TRANSFERRED));
    let sim = scene.sim();
    let owner = sim.houses.order()[0];
    let money = crate::sim::credit_income::available_money(&sim.houses, owner);
    let credits = Ev {
        value: i64::from(money),
        ..Ev::on(12)
    };
    let mut scene = installed(
        scene,
        &module(&def(house_ref(0), 0, &[credits], &[]), 0, ""),
    );
    ticks(&mut scene, 9);
    assert_eq!(received(&scene), 1);
    assert_eq!(first_record(&scene), (12, 0, house_ref(0), -1, -1));
    ticks(&mut scene, 16);
    assert_eq!(received(&scene), 1, "a one-time trigger");
}

#[test]
fn nuclear_launch_callbacks_follow_the_effect_that_fired() {
    use vera20k_wasm_ext::triggers::EVENT_SUPER_WEAPON_LAUNCHED;
    for fallback in [false, true] {
        let _form = crate::rules::ares_switch::read_ares_way(fallback);
        let Some(mut scene) = scenario() else { return };
        if fallback {
            let patch = crate::rules::ini_parser::IniFile::from_str(crate::test_fixture::text(
                "tests/fixtures/ini/ares_nuclear_fallback.ini",
            ));
            let Some(retail) =
                crate::rules::retail_ini_fixture::retail_battle_rules_for_map_with_patch(
                    "XMP03T4.MAP",
                    &patch,
                )
            else {
                return;
            };
            let nuke = retail.rules.super_weapon("NukeSpecial").unwrap();
            assert_eq!(nuke.money_amount, -25);
            assert_eq!(
                crate::sim::superweapon::nuke::ares_payload_hit(&retail.rules, nuke)
                    .map(|(weapon, _, _)| (weapon.id.as_str(), weapon.projectile.as_deref())),
                Some(("NoNuclearProjectile", None))
            );
            scene.runtime.resources_mut().rules = retail.rules;
            let runtime = &mut scene.runtime;
            runtime
                .simulation
                .intern_rule_type_ids(&runtime.resources.rules);
            runtime
                .simulation
                .resolve_type_handles(&runtime.resources.rules);
        }
        let listener = def(0, 2, &[Ev::on(EVENT_SUPER_WEAPON_LAUNCHED)], &[]);
        let mut scene = installed(scene, &module(&listener, 0, ""));
        let start = &scene.map.waypoints[&0];
        let cell = (start.rx, start.ry);
        let nuclear_launches = i32::from(fallback);
        for (name, expected) in [
            ("NukeSpecial", nuclear_launches),
            ("PsychicRevealSpecial", nuclear_launches + 1),
        ] {
            let runtime = &mut scene.runtime;
            let sim = &mut runtime.simulation;
            let rules = &runtime.resources.rules;
            let owner = sim.session.current_house().unwrap_or(sim.houses.order()[0]);
            let fallback_nuke = fallback && name == "NukeSpecial";
            let sw = if fallback_nuke {
                let id = sim.interner.get(name).unwrap();
                assert!(crate::sim::superweapon::trigger_grant_super(
                    &sim.interner,
                    &sim.session,
                    &mut sim.substrate,
                    &sim.houses,
                    &mut sim.super_weapons,
                    rules,
                    owner,
                    id,
                    true
                ));
                crate::sim::credit_income::add_credits(&mut sim.houses, owner, 100);
                id
            } else {
                crate::sim::superweapon::chronosphere_tests::charge_super(sim, owner, name)
            };
            let credits = crate::sim::credit_income::available_money(&sim.houses, owner);
            if fallback_nuke {
                let instance = &sim.super_weapons()[&owner][&sw];
                assert!(
                    instance.is_ready()
                        && instance.is_active()
                        && instance.charge_start_tick() != -1
                );
                assert!(crate::sim::superweapon::super_weapon_available(
                    sim, rules, owner, name
                ));
                assert_eq!(
                    sim.super_weapon_target_check(rules, owner, sw, cell, false),
                    Ok(())
                );
            }
            assert!(sim.fire_super_weapon(
                rules,
                owner,
                sw,
                cell,
                None,
                &mut FrameEffects::default(),
                false
            ));
            if fallback_nuke {
                assert_eq!(
                    crate::sim::credit_income::available_money(&sim.houses, owner),
                    credits,
                    "refused handler charges no fee"
                );
                assert!(!sim.sound_events().iter().any(|e| matches!(
                    e,
                    crate::sim::world::SimSoundEvent::AresSuperWeaponActivatedEva { .. }
                        | crate::sim::world::SimSoundEvent::AresSuperWeaponFinished { .. }
                )));
                assert!(sim.projectiles.iter().any(|(_, bullet)| {
                    bullet
                        .payload()
                        .installed_weapon()
                        .is_some_and(|id| sim.interner.resolve(id) == "NukePayload")
                }));
            }
            scene.tick();
            assert_eq!(received(&scene), expected, "{name} fallback={fallback}");
        }
    }
}

/// `HouseTransferred` on a bound object after ChangeOwner, carrying the
/// old house; `value` 0, as this is no mind control.
#[test]
fn a_house_transfer_raises_house_transferred() {
    let Some(scene) = scenario() else { return };
    let victim = sturdy_object(&scene);
    let transferred = def(0, 2, &[Ev::on(EVENT_HOUSE_TRANSFERRED)], &[]);
    let mut scene = installed(scene, &module(&transferred, victim as i64, ""));
    let runtime = &mut scene.runtime;
    let sim = &mut runtime.simulation;
    let old = sim.entities().get(victim).expect("victim").owner();
    let old_index = sim.houses.index_of(old).expect("old house");
    let new = *sim
        .houses
        .order()
        .iter()
        .find(|&&house| house != old)
        .expect("a second house");
    let rules = &runtime.resources.rules;
    sim.change_owner_with_rules(
        victim,
        new,
        rules,
        Some(rules.overlay_registry()),
        &mut FrameEffects::default(),
    );
    scene.tick();
    assert_eq!(received(&scene), 1);
    let (kind, subject, house, value, _) = first_record(&scene);
    assert_eq!(
        (kind, subject, house, value),
        (
            EVENT_HOUSE_TRANSFERRED,
            victim as i64,
            house_ref(old_index),
            0
        )
    );
}

/// `PropertyPoll` `HealthPercent < 50`, checked every frame on a bound
/// object: it fires when a hit makes it true, with the reading, and not
/// again while it stays true.
#[test]
fn a_property_poll_fires_on_the_transition_only() {
    let Some(scene) = scenario() else { return };
    let victim = sturdy_object(&scene);
    let poll = Ev {
        op: 2, // Lt
        interval: 1,
        id: vera20k_wasm_ext::property_id("HealthPercent").expect("property"),
        value: 50,
        ..Ev::on(EVENT_PROPERTY_POLL)
    };
    let mut scene = installed(scene, &module(&def(0, 2, &[poll], &[]), victim as i64, ""));
    ticks(&mut scene, 3);
    assert_eq!(received(&scene), 0, "above half health");
    let sim = scene.sim();
    let entity = sim.entities().get(victim).expect("victim");
    let strength = sim
        .entity_object_type(entity, &scene.runtime.resources.rules)
        .expect("type")
        .strength;
    let damage = entity.health.current() - strength * 2 / 5;
    hit(&mut scene, victim, damage);
    ticks(&mut scene, 2);
    assert_eq!(received(&scene), 1);
    let (kind, subject, _, value, _) = first_record(&scene);
    assert_eq!((kind, subject), (EVENT_PROPERTY_POLL, victim as i64));
    assert!(value < 50, "the reading {value}");
    hit(&mut scene, victim, 1);
    ticks(&mut scene, 3);
    assert_eq!(received(&scene), 1, "still below: no new transition");
}

#[test]
fn modifier_count_poll_reads_only_its_origin_and_counts_zero_additions() {
    use super::modifiers::{Modifier, ModifierOp, Stat};

    let Some(mut scene) = scenario() else { return };
    let target = sturdy_object(&scene);
    let add = |scene: &mut HeadlessScenario, origin| {
        let sim = &mut scene.runtime.simulation;
        let handle = sim.substrate.allocate_stable_id();
        let modifier = Modifier {
            target,
            stat: Stat::Firepower,
            op: ModifierOp::Add,
            value: 0,
            expires: None,
            source: 0,
            group: 0,
            origin,
        };
        sim.ext_modules
            .modifiers
            .insert(&mut sim.substrate.entities, handle, modifier)
            .unwrap();
        handle
    };

    add(&mut scene, ModuleSource::Mod);
    let poll = Ev {
        interval: 1,
        id: 0,
        ..Ev::on(EVENT_MODIFIER_COUNT_POLL)
    };
    let mut scene = installed(scene, &module(&def(0, 2, &[poll], &[]), target as i64, ""));
    ticks(&mut scene, 2);
    assert_eq!(received(&scene), 1, "the other origin does not count");
    assert_eq!(first_record(&scene).3, 0);

    let handle = add(&mut scene, ModuleSource::Map);
    ticks(&mut scene, 2);
    assert_eq!(received(&scene), 1, "the zero addition counts");

    let sim = &mut scene.runtime.simulation;
    sim.ext_modules
        .modifiers
        .remove(&mut sim.substrate.entities, handle)
        .unwrap();
    ticks(&mut scene, 2);
    assert_eq!(received(&scene), 2, "removal returns the count to zero");
}

/// `VariablePoll` of a declared global variable every 20 frames: a change
/// of the variable ten frames in restarts the interval, so the check that
/// would have fired 20 frames after creation does not, and the one 20
/// frames after the change does.
#[test]
fn a_variable_change_restarts_its_variable_poll() {
    const WAVE: i32 = 1000;
    // vera.vars: format 1, one variable "Wave", global scope, private, 0.
    const VARS: &str =
        r#"(@custom "vera.vars" "\01\01\00\00\00\04\00Wave\00\00\00\00\00\00\00\00\00\00")"#;
    let Some(scene) = scenario() else { return };
    let poll = Ev {
        interval: 20,
        id: WAVE,
        value: 1,
        ..Ev::on(EVENT_VARIABLE_POLL)
    };
    let mut scene = installed(scene, &module(&def(0, 2, &[poll], &[]), 0, VARS));
    ticks(&mut scene, 10);
    assert_eq!(
        scene.runtime.simulation.write_module_var(0, WAVE, 1),
        Ok(vera20k_wasm_ext::Status::Ok)
    );
    ticks(&mut scene, 10);
    assert_eq!(received(&scene), 0, "the first interval restarted");
    ticks(&mut scene, 11);
    assert_eq!(received(&scene), 1);
    assert_eq!(first_record(&scene).3, 1, "the reading");
}

/// A `Fire` trigger bound to the GI's weapon, not to any object or type,
/// fires for a shooter using it.
#[test]
fn a_weapon_binding_raises_fire_for_its_shooter() {
    let Some((mut scene, attacker, target)) = super::tests::gi_duel() else {
        return;
    };
    let rules = &scene.runtime.resources.rules;
    let weapon = rules.object("E1").expect("E1").weapon_list[0]
        .clone()
        .expect("E1 has a primary weapon");
    let id = rules.weapon_index(&weapon).expect("weapon ID");
    let weapon_ref = super::type_ref(vera20k_wasm_ext::NAME_WEAPON, id);
    let fire = def(0, 2, &[Ev::on(EVENT_FIRE)], &[]);
    let mut scene = {
        scene
            .runtime
            .simulation
            .install_ext_module(
                &scene.runtime.resources.rules,
                ModuleSource::Map,
                &module(&fire, weapon_ref, ""),
            )
            .expect("module installs");
        scene.tick();
        assert_eq!(
            &memory(&scene)[72..80],
            &[0; 8],
            "create and bind answer Ok"
        );
        scene
    };
    super::tests::first_hit(&mut scene, attacker, target, |_| {});
    scene.tick();
    assert!(received(&scene) >= 1);
    // The target may answer first; either GI shoots the bound weapon.
    let (kind, subject, _, _, weapon_id) = first_record(&scene);
    assert_eq!((kind, weapon_id), (EVENT_FIRE, id as i32));
    assert!([attacker as i64, target as i64].contains(&subject));
}

/// A `TakeDamage` trigger whose immediate `Damage` hurts its own subject
/// raises `TakeDamage` again: the chain fires [`MAX_CHAIN_DEPTH`] deep,
/// the next firing waits for the module phase, and there it starts a new
/// chain of the same depth. No recursion without end, the module runs on.
#[test]
fn a_self_damaging_trigger_chain_stops_at_its_depth_cap() {
    use super::chains::MAX_CHAIN_DEPTH;
    let Some(scene) = scenario() else { return };
    let victim = sturdy_object(&scene);
    let warhead = scene
        .runtime
        .resources
        .rules
        .warhead_index("Super")
        .expect("Super") as i32;
    let chain = def(
        0,
        2,
        &[Ev::on(EVENT_TAKE_DAMAGE)],
        &[(ACTION_DAMAGE, 0, warhead, 1)],
    );
    let mut scene = installed(scene, &module(&chain, victim as i64, ""));
    let health = |scene: &HeadlessScenario| {
        scene
            .sim()
            .entities()
            .get(victim)
            .expect("victim lives")
            .health
            .current()
    };
    let before = health(&scene);
    hit(&mut scene, victim, 10);
    let depth = MAX_CHAIN_DEPTH as i32;
    assert_eq!(health(&scene), before - 10 - depth);
    let triggers = scene.sim().trigger_runtime.module_triggers();
    assert_eq!(triggers.deferred_of(ModuleSource::Map), 1);
    assert_eq!(triggers.queued(ModuleSource::Map), depth as usize);
    scene.tick();
    assert_eq!(health(&scene), before - 10 - 2 * depth);
    assert_eq!(received(&scene), 2 * depth);
    assert_eq!(scene.sim().ext_modules().status()[0].1, None);
}

/// A mind control's ChangeOwner (CaptureManager's) raises `HouseTransferred`
/// with `value` 1.
#[test]
fn a_mind_control_transfer_reports_its_cause() {
    let Some(scene) = scenario() else { return };
    let victim = sturdy_object(&scene);
    let transferred = def(0, 2, &[Ev::on(EVENT_HOUSE_TRANSFERRED)], &[]);
    let mut scene = installed(scene, &module(&transferred, victim as i64, ""));
    let runtime = &mut scene.runtime;
    let sim = &mut runtime.simulation;
    let old = sim.entities().get(victim).expect("victim").owner();
    let new = *sim
        .houses
        .order()
        .iter()
        .find(|&&house| house != old)
        .expect("a second house");
    let rules = &runtime.resources.rules;
    sim.change_owner_by_mind_control(
        victim,
        new,
        rules,
        Some(rules.overlay_registry()),
        &mut FrameEffects::default(),
    );
    scene.tick();
    assert_eq!(received(&scene), 1);
    let (kind, subject, _, value, _) = first_record(&scene);
    assert_eq!(
        (kind, subject, value),
        (EVENT_HOUSE_TRANSFERRED, victim as i64, 1)
    );
}

/// `DistancePoll` on a bound object to its own cell, `< 1024` leptons: it
/// fires at its first check, the reading under the threshold; one to a cell
/// 40 cells away does not. `has_kind` lists the poll, and not Ares' events
/// without Ares.
#[test]
fn a_distance_poll_compares_with_its_id() {
    let Some(scene) = scenario() else { return };
    let rules = &scene.runtime.resources.rules;
    assert!(super::has_kind(rules, 0, EVENT_DISTANCE_POLL));
    assert!(!rules.ares_form() && !super::has_kind(rules, 0, 62));
    let victim = sturdy_object(&scene);
    let at = crate::sim::movement::ground_pose::position_world_coord(
        &scene.sim().entities().get(victim).expect("victim").position,
    );
    let (x, y) = ((at.x / 256) as u16, (at.y / 256) as u16);
    let poll = |cell: i64| Ev {
        op: 2, // Lt
        interval: 1,
        id: 1024,
        value: cell,
        ..Ev::on(EVENT_DISTANCE_POLL)
    };
    let far = super::cell_ref(x.saturating_add(40), y);
    let mut scene = installed(
        scene,
        &module(&def(0, 2, &[poll(far)], &[]), victim as i64, ""),
    );
    ticks(&mut scene, 3);
    assert_eq!(received(&scene), 0, "40 cells away");
    let Some(scene) = scenario() else { return };
    let near = super::cell_ref(x, y);
    let mut scene = installed(
        scene,
        &module(&def(0, 2, &[poll(near)], &[]), victim as i64, ""),
    );
    ticks(&mut scene, 3);
    assert_eq!(received(&scene), 1);
    let (kind, subject, _, value, _) = first_record(&scene);
    assert_eq!((kind, subject), (EVENT_DISTANCE_POLL, victim as i64));
    assert!((0..1024).contains(&value), "the reading {value}");
}

/// `FatalDamage` on a bound object whose hit leaves no Strength, with the
/// damage written.
#[test]
fn a_lethal_hit_raises_fatal_damage() {
    let Some(scene) = scenario() else { return };
    let victim = sturdy_object(&scene);
    let fatal = def(0, 2, &[Ev::on(EVENT_FATAL_DAMAGE)], &[]);
    let mut scene = installed(scene, &module(&fatal, victim as i64, ""));
    hit(&mut scene, victim, 10);
    scene.tick();
    assert_eq!(received(&scene), 0, "a hit it survives");
    let health = scene
        .sim()
        .entities()
        .get(victim)
        .expect("victim")
        .health
        .current();
    hit(&mut scene, victim, health.saturating_mul(4));
    scene.tick();
    assert_eq!(received(&scene), 1);
    let (kind, subject, _, value, _) = first_record(&scene);
    assert_eq!(
        (kind, subject, value),
        (EVENT_FATAL_DAMAGE, victim as i64, i64::from(health))
    );
}

/// `Timer` beside an object event counts on the bound object's instance:
/// after a period ended, the next hit completes the trigger, whose record
/// is the hit's.
#[test]
fn a_timer_with_an_object_event_latches_on_the_bound_object() {
    let Some(scene) = scenario() else { return };
    let victim = sturdy_object(&scene);
    let timer = Ev {
        interval: 4,
        ..Ev::on(EVENT_TIMER)
    };
    let both = def(0, 2, &[Ev::on(EVENT_TAKE_DAMAGE), timer], &[]);
    let mut scene = installed(scene, &module(&both, victim as i64, ""));
    ticks(&mut scene, 5);
    assert_eq!(received(&scene), 0, "the timer alone");
    hit(&mut scene, victim, 10);
    scene.tick();
    assert_eq!(received(&scene), 1);
    let (kind, subject, _, _, _) = first_record(&scene);
    assert_eq!((kind, subject), (EVENT_TAKE_DAMAGE, victim as i64));
}

/// A `Fire` trigger bound to the GI weapon's projectile type fires for a
/// shooter using it; `lookup(Projectile)` answers that ID.
#[test]
fn a_projectile_binding_raises_fire_for_its_shooter() {
    use vera20k_wasm_ext::{NAME_PROJECTILE, TABLE_NAME_KIND};
    let Some((mut scene, attacker, target)) = super::tests::gi_duel() else {
        return;
    };
    let rules = &scene.runtime.resources.rules;
    assert!(super::has_kind(rules, TABLE_NAME_KIND, NAME_PROJECTILE));
    let weapon = rules.object("E1").expect("E1").weapon_list[0]
        .clone()
        .expect("E1 has a primary weapon");
    let projectile = rules
        .weapon(&weapon)
        .and_then(|weapon| weapon.projectile.clone())
        .expect("the weapon has a projectile");
    let id = rules.projectile_index(&projectile).expect("projectile ID");
    assert_eq!(
        super::rules_name_id(rules, NAME_PROJECTILE, &projectile),
        Some(Some(id as i32))
    );
    let projectile_ref = super::type_ref(NAME_PROJECTILE, id);
    let fire = def(0, 2, &[Ev::on(EVENT_FIRE)], &[]);
    scene
        .runtime
        .simulation
        .install_ext_module(
            &scene.runtime.resources.rules,
            ModuleSource::Map,
            &module(&fire, projectile_ref, ""),
        )
        .expect("module installs");
    scene.tick();
    assert_eq!(
        &memory(&scene)[72..80],
        &[0; 8],
        "create and bind answer Ok"
    );
    super::tests::first_hit(&mut scene, attacker, target, |_| {});
    scene.tick();
    assert!(received(&scene) >= 1);
    let (kind, subject, _, _, _) = first_record(&scene);
    assert_eq!(kind, EVENT_FIRE);
    assert!([attacker as i64, target as i64].contains(&subject));
}
