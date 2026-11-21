//! The module phase on a retail map: effects through the owners,
//! determinism, a runaway module, and host-call cost.

use super::{ACTION_ADD_MONEY, ModuleSource, house_ref};
use crate::headless_scenario::HeadlessScenario;

const MAP: &str = "XMP03T4.MAP";
const SEED: u32 = 0x1269;

fn scenario() -> Option<HeadlessScenario> {
    let (retail, _) = crate::rules::retail_ini_fixture::retail_assets()?;
    Some(crate::headless_scenario::load(&retail, MAP, SEED).expect("retail scenario loads"))
}

fn neutral_index(scene: &HeadlessScenario) -> usize {
    let sim = scene.sim();
    let neutral = sim
        .interner
        .get("Neutral")
        .expect("the map has a Neutral house");
    sim.houses
        .order()
        .iter()
        .position(|&h| h == neutral)
        .expect("Neutral is in the house order")
}

fn credits(scene: &HeadlessScenario, index: usize) -> i32 {
    let sim = scene.sim();
    let id = sim.houses.order()[index];
    sim.houses.get(&id).expect("house").economy().credits()
}

/// Pays the house 100 credits every 15th frame, sets local variable 5 on
/// frame 30 and draws from its own stream each frame.
fn paying_module(house: i64) -> Vec<u8> {
    wat::parse_str(format!(
        r#"(module
  (import "vera:sim@1.0" "frame" (func $frame (result i32)))
  (import "vera:sim@1.0" "log" (func $log (param i32 i32)))
  (import "vera:sim@1.0" "random" (func $random (param i32 i32 i32) (result i32)))
  (import "vera:sim@1.0" "var_set" (func $var_set (param i64 i32 i64) (result i32)))
  (import "vera:sim@1.0" "action" (func $action (param i32 i64 i32 i32 i32) (result i32)))
  (memory (export "memory") 1)
  (data (i32.const 0) "start")
  (data (i32.const 8) "paid")
  (func (export "on_scenario_start") (call $log (i32.const 0) (i32.const 5)))
  (func (export "on_frame") (local $f i32)
    (local.set $f (call $frame))
    (drop (call $random (i32.const 0) (i32.const 1000) (i32.const 520)))
    (i32.store (i32.const 512) (i32.add (i32.load (i32.const 512)) (i32.load (i32.const 520))))
    (if (i32.eqz (i32.rem_u (local.get $f) (i32.const 15)))
      (then
        (i64.store (i32.const 144) (i64.const 100))
        (drop (call $action (i32.const {ACTION_ADD_MONEY}) (i64.const {house}) (i32.const 128) (i32.const 72) (i32.const 256)))
        (call $log (i32.const 8) (i32.const 4))))
    (if (i32.eq (local.get $f) (i32.const 30))
      (then (drop (call $var_set (i64.const 0) (i32.const 5) (i64.const 1)))))))"#
    ))
    .expect("test module parses")
}

#[test]
fn a_module_pays_a_house_and_sets_a_variable_identically_on_every_run() {
    let Some(mut plain) = scenario() else { return };
    let house = neutral_index(&plain);
    let module = paying_module(house_ref(house));
    let mut runs = [
        scenario().expect("second load"),
        scenario().expect("third load"),
    ];
    for run in &mut runs {
        run.runtime
            .simulation
            .install_ext_module(&run.runtime.resources.rules, ModuleSource::Map, &module)
            .expect("module installs");
    }
    let mut paid = 0;
    for _ in 0..90 {
        let paying_frame = plain.sim().session.binary_frame() % 15 == 0;
        plain.tick();
        for run in &mut runs {
            run.tick();
        }
        // Neutral spends nothing, so each payment shows as an exact difference.
        if paying_frame {
            paid += 100;
        }
        assert_eq!(credits(&runs[0], house) - credits(&plain, house), paid);
        assert_eq!(runs[0].sim().state_hash(), runs[1].sim().state_hash());
    }
    assert!(paid >= 500);
    assert!(runs[0].sim().trigger_runtime.local_variable_is_set(5));
    assert!(!plain.sim().trigger_runtime.local_variable_is_set(5));
    let log = runs[0].runtime.simulation.ext_modules_mut().take_log();
    assert_eq!(log.first().map(String::as_str), Some("start"));
    assert_eq!(
        log.iter().filter(|l| *l == "paid").count() as i32,
        paid / 100
    );
    // The module's own stream leaves the native streams alone: everything
    // but the money, the variable and the module's state matches.
    assert_eq!(
        plain.sim().scenario_rng.state(),
        runs[0].sim().scenario_rng.state()
    );
}

#[test]
fn a_runaway_module_is_disabled_and_the_match_goes_on() {
    let Some(mut scene) = scenario() else { return };
    let runaway = wat::parse_str(
        r#"(module (memory (export "memory") 1)
             (func (export "on_frame") (loop $l (br $l))))"#,
    )
    .expect("test module parses");
    scene
        .runtime
        .simulation
        .install_ext_module(&scene.runtime.resources.rules, ModuleSource::Mod, &runaway)
        .expect("module installs");
    let start = scene.sim().session.binary_frame();
    // The players are told once, through the frame's output.
    let mut notices = 0;
    for _ in 0..5 {
        let output = scene
            .runtime
            .advance_frame(
                &[],
                crate::headless_scenario::SIM_TICK_MS,
                crate::sim::world::TickLane::Ordinary,
                &mut crate::sim::world::FrameEffects::default(),
            )
            .expect("frame advances");
        notices += output
            .trigger_effects
            .iter()
            .filter(|e| {
                matches!(
                    e,
                    crate::sim::trigger_runtime::TriggerEffect::ExtModuleStopped { .. }
                )
            })
            .count();
    }
    assert_eq!(notices, 1);
    assert_eq!(scene.sim().session.binary_frame(), start + 5);
    let status = scene.sim().ext_modules().status();
    let reason = status[0].1.as_deref().expect("the module stopped");
    assert!(
        reason.contains("on_frame") && reason.contains("budget"),
        "{reason}"
    );
}

#[test]
fn a_module_that_breaks_the_load_rules_never_installs() {
    let Some(mut scene) = scenario() else { return };
    let wasi = wat::parse_str(
        r#"(module (import "wasi_snapshot_preview1" "proc_exit" (func (param i32))))"#,
    )
    .expect("test module parses");
    let error = scene
        .runtime
        .simulation
        .install_ext_module(&scene.runtime.resources.rules, ModuleSource::Map, &wasi)
        .unwrap_err();
    assert!(error.contains("wasi_snapshot_preview1"), "{error}");
    assert!(scene.sim().ext_modules().is_empty());
}

/// Not an assertion: prints the cost of one host call for the feasibility
/// report. `vera_init` makes 50,000 `var_get` calls under the one-time budget.
#[test]
fn host_call_cost() {
    let Some(mut scene) = scenario() else { return };
    let module = wat::parse_str(
        r#"(module
  (import "vera:sim@1.0" "var_get" (func $var_get (param i64 i32 i32) (result i32)))
  (memory (export "memory") 1)
  (func (export "vera_init") (local $i i32)
    (loop $l
      (drop (call $var_get (i64.const 0) (i32.const 3) (i32.const 64)))
      (local.set $i (i32.add (local.get $i) (i32.const 1)))
      (br_if $l (i32.lt_u (local.get $i) (i32.const 50000))))))"#,
    )
    .expect("test module parses");
    let started = std::time::Instant::now();
    scene
        .runtime
        .simulation
        .install_ext_module(&scene.runtime.resources.rules, ModuleSource::Mod, &module)
        .expect("module installs");
    let elapsed = started.elapsed();
    eprintln!(
        "load + compile + 50,000 host calls: {elapsed:?} ({:.0} ns per call upper bound)",
        elapsed.as_nanos() as f64 / 50_000.0
    );
}

/// Runs a `rules.wasm` built by a real toolchain when
/// `VERA20K_TEST_RULES_WASM` names one (`sdk/examples/minimal-rules`).
#[test]
fn a_rust_built_module_runs() {
    let Some(path) = std::env::var_os("VERA20K_TEST_RULES_WASM") else {
        return;
    };
    let Some(mut scene) = scenario() else { return };
    let bytes = std::fs::read(&path).expect("module file reads");
    scene
        .runtime
        .simulation
        .install_ext_module(&scene.runtime.resources.rules, ModuleSource::Mod, &bytes)
        .expect("module installs");
    for _ in 0..60 {
        scene.tick();
    }
    let status = scene.sim().ext_modules().status();
    assert_eq!(status[0].1, None, "the module is still running");
    assert!(scene.sim().trigger_runtime.local_variable_is_set(5));
    let log = scene.runtime.simulation.ext_modules_mut().take_log();
    assert!(
        log.iter().any(|l| l == "minimal-rules: paid 100"),
        "{log:?}"
    );
}

/// A save carries each module's memory, globals and stream; the loaded match
/// runs its modules on exactly as the live one does.
#[test]
fn a_saved_match_resumes_its_modules_exactly() {
    use crate::sim::snapshot::GameSnapshot;
    let Some(mut live) = scenario() else { return };
    let house = neutral_index(&live);
    live.runtime
        .simulation
        .install_ext_module(
            &live.runtime.resources.rules,
            ModuleSource::Map,
            &paying_module(house_ref(house)),
        )
        .expect("module installs");
    for _ in 0..20 {
        live.tick();
    }
    let rules = &live.runtime.resources.rules;
    let bytes = GameSnapshot::save_validated(
        live.sim(),
        0,
        rules.simulation_config_hash(),
        "ext modules",
        0,
    );
    let mut loaded = GameSnapshot::load(&bytes).expect("save loads").sim;
    loaded
        .restore_after_snapshot_load()
        .expect("modules restore");
    loaded.rebuild_caches_after_load(
        live.sim().resolved_terrain().expect("terrain").clone(),
        Default::default(),
        rules,
    );
    loaded
        .restore_map_authority_after_snapshot_load(rules, rules.overlay_registry(), None)
        .expect("map authority restores");
    loaded.resolve_type_handles(rules);
    let mut resumed = scenario().expect("second load");
    resumed.runtime.simulation = loaded;
    let digests = |scene: &HeadlessScenario| {
        scene
            .sim()
            .ext_modules
            .checksum_digests()
            .collect::<Vec<_>>()
    };
    assert_eq!(digests(&resumed), digests(&live));
    for _ in 0..40 {
        live.tick();
        resumed.tick();
        assert_eq!(digests(&resumed), digests(&live));
    }
    assert_eq!(credits(&resumed, house), credits(&live, house));
    assert_eq!(
        resumed.sim().ext_modules().status(),
        live.sim().ext_modules().status()
    );
}

/// `find` pages through a house's objects; `get` reads the first one's
/// owner and health, which the module checks and records in local
/// variable 6.
#[test]
fn a_module_finds_a_houses_objects_and_reads_them() {
    let Some(mut scene) = scenario() else { return };
    let sim = scene.sim();
    // The house with the most objects, and its first object.
    let (house, owned) = sim
        .houses
        .order()
        .iter()
        .enumerate()
        .map(|(index, id)| {
            let owned: Vec<u64> = sim
                .entities()
                .iter_sorted()
                .filter(|(_, e)| e.owner() == *id)
                .map(|(stable_id, _)| stable_id)
                .collect();
            (index, owned)
        })
        .max_by_key(|(_, owned)| owned.len())
        .expect("the map has houses");
    assert!(owned.len() > 16, "more than one page");
    let first = owned[0];
    let prop = |name| vera20k_wasm_ext::property_id(name).expect("known property");
    let (owner, health) = (prop("Owner"), prop("Health"));
    let reference = house_ref(house);
    let module = wat::parse_str(format!(
        r#"(module
  (import "vera:sim@1.0" "find" (func $find (param i32 i32 i32 i32 i32) (result i32)))
  (import "vera:sim@1.0" "get" (func $get (param i64 i32 i32) (result i32)))
  (import "vera:sim@1.0" "var_set" (func $var_set (param i64 i32 i64) (result i32)))
  (memory (export "memory") 1)
  (func (export "on_scenario_start")
    ;; Query at 0: Objects, whole map, owner at 32, after at 48.
    (i64.store (i32.const 32) (i64.const {reference}))
    (loop $page
      (drop (call $find (i32.const 0) (i32.const 64) (i32.const 1024) (i32.const 16) (i32.const 512)))
      (if (i64.eqz (i64.load (i32.const 904)))
        (then (i64.store (i32.const 904) (i64.load (i32.const 1024)))))
      (i32.store (i32.const 900) (i32.add (i32.load (i32.const 900)) (i32.load (i32.const 512))))
      (i64.store (i32.const 48) (i64.load (i32.const 520)))
      (br_if $page (i64.ne (i64.load (i32.const 520)) (i64.const 0))))
    (drop (call $get (i64.load (i32.const 904)) (i32.const {owner}) (i32.const 920)))
    (drop (call $get (i64.load (i32.const 904)) (i32.const {health}) (i32.const 928)))
    (drop (call $var_set (i64.const 0) (i32.const 6)
      (i64.extend_i32_u (i32.and
        (i64.eq (i64.load (i32.const 920)) (i64.const {reference}))
        (i64.gt_s (i64.load (i32.const 928)) (i64.const 0))))))))"#
    ))
    .expect("test module parses");
    scene
        .runtime
        .simulation
        .install_ext_module(&scene.runtime.resources.rules, ModuleSource::Map, &module)
        .expect("module installs");
    scene.tick();
    assert_eq!(scene.sim().ext_modules().status()[0].1, None);
    let memory = scene.sim().ext_modules.slots[0].module.memory();
    let i64_at = |at: usize| i64::from_le_bytes(memory[at..at + 8].try_into().expect("8 bytes"));
    let count = i32::from_le_bytes(memory[900..904].try_into().expect("4 bytes"));
    assert_eq!(count as usize, owned.len());
    assert_eq!(i64_at(904), first as i64);
    assert_eq!(i64_at(920), reference);
    let entity = scene.sim().entities().get(first).expect("first object");
    assert_eq!(i64_at(928), i64::from(entity.health.current()));
    assert!(scene.sim().trigger_runtime.local_variable_is_set(6));
}

fn prop(name: &str) -> i32 {
    vera20k_wasm_ext::property_id(name).expect("known property")
}

fn install(scene: &mut HeadlessScenario, source: ModuleSource, module: &[u8]) {
    scene
        .runtime
        .simulation
        .install_ext_module(&scene.runtime.resources.rules, source, module)
        .expect("module installs");
}

/// Module `slot`'s memory at `at`, little endian.
fn memory_at<const N: usize>(scene: &HeadlessScenario, slot: usize, at: usize) -> [u8; N] {
    let memory = scene.sim().ext_modules.slots[slot].module.memory();
    memory[at..at + N].try_into().expect("in memory")
}

fn mem_i32(scene: &HeadlessScenario, slot: usize, at: usize) -> i32 {
    i32::from_le_bytes(memory_at(scene, slot, at))
}

fn mem_i64(scene: &HeadlessScenario, slot: usize, at: usize) -> i64 {
    i64::from_le_bytes(memory_at(scene, slot, at))
}

/// The modules' share of the state hash: modules, modifiers and variables.
fn ext_hash(scene: &HeadlessScenario) -> u64 {
    use std::hash::Hasher;
    let mut hasher = std::hash::DefaultHasher::new();
    scene.sim().ext_modules.hash_state(&mut hasher);
    hasher.finish()
}

/// A save of `live` loaded into a fresh scenario, as the existing save test
/// does it.
fn saved_and_loaded(live: &HeadlessScenario) -> HeadlessScenario {
    use crate::sim::snapshot::GameSnapshot;
    let rules = &live.runtime.resources.rules;
    let bytes = GameSnapshot::save_validated(
        live.sim(),
        0,
        rules.simulation_config_hash(),
        "ext modules",
        0,
    );
    let mut loaded = GameSnapshot::load(&bytes).expect("save loads").sim;
    loaded
        .restore_after_snapshot_load()
        .expect("modules restore");
    loaded.rebuild_caches_after_load(
        live.sim().resolved_terrain().expect("terrain").clone(),
        Default::default(),
        rules,
    );
    loaded
        .restore_map_authority_after_snapshot_load(rules, rules.overlay_registry(), None)
        .expect("map authority restores");
    loaded.resolve_type_handles(rules);
    let mut resumed = scenario().expect("second load");
    resumed.runtime.simulation = loaded;
    resumed
}

/// The first live, placed unit or infantry, and a house that does not own it.
fn placed_unit(scene: &HeadlessScenario) -> (u64, usize) {
    let sim = scene.sim();
    let (id, entity) = sim
        .entities()
        .iter_sorted()
        .find(|(_, e)| {
            matches!(
                e.category(),
                crate::map::entities::EntityCategory::Unit
                    | crate::map::entities::EntityCategory::Infantry
            ) && e.lifecycle.object_alive()
                && e.lifecycle.cell_marked()
                && !e.lifecycle.in_limbo()
        })
        .expect("the map has a placed unit");
    let other = sim
        .houses
        .order()
        .iter()
        .position(|h| *h != entity.owner())
        .expect("a second house");
    (id, other)
}

/// Two hostile GIs two cells apart on open ground far from the map's own
/// objects: (scenario, attacker, target).
fn gi_duel() -> Option<(HeadlessScenario, u64, u64)> {
    use crate::sim::house_state::HouseState;
    let mut scene = scenario()?;
    let crate::sim::runtime::SimRuntime {
        simulation: sim,
        resources,
    } = &mut scene.runtime;
    let rules = &resources.rules;
    for (name, side) in [("Americans", 0), ("Russians", 1)] {
        let house = sim.interner.intern(name);
        sim.houses.get_or_register_for_test(house, || {
            HouseState::new(house, side, None, true, 10_000, 10)
        });
    }
    let occupied: Vec<(u16, u16)> = sim
        .entities()
        .iter_sorted()
        .map(|(_, e)| (e.position.rx(), e.position.ry()))
        .collect();
    let (x, y) = (20..120_u16)
        .flat_map(|y| (20..120_u16).map(move |x| (x, y)))
        .find(|&(x, y)| {
            let (Some(grid), Some(terrain)) = (sim.path_grid(), sim.resolved_terrain()) else {
                return false;
            };
            let Some(level) = terrain.cell(x, y).map(|c| c.level) else {
                return false;
            };
            (x - 1..=x + 3).all(|cx| {
                (y - 1..=y + 1).all(|cy| {
                    terrain.cell(cx, cy).is_some_and(|c| c.level == level)
                        && grid.cell(cx, cy).is_some_and(|c| c.ground_walkable)
                })
            }) && occupied
                .iter()
                .all(|&(ox, oy)| ox.abs_diff(x).max(oy.abs_diff(y)) >= 14)
        })?;
    // A power plant each, far away, so neither house is defeated.
    for (plant, owner) in [("GAPOWR", "Americans"), ("NAPOWR", "Russians")] {
        (20..120_u16)
            .flat_map(|py| (20..120_u16).map(move |px| (px, py)))
            .filter(|&(px, py)| px.abs_diff(x).max(py.abs_diff(y)) >= 16)
            .find_map(|(px, py)| sim.spawn_object(plant, owner, px, py, 0, rules))?;
    }
    let attacker = sim.spawn_object("E1", "Americans", x, y, 0, rules)?;
    let target = sim.spawn_object("E1", "Russians", x + 2, y, 0, rules)?;
    sim.resolve_type_handles(rules);
    Some((scene, attacker, target))
}

/// Orders `attacker` to attack `target`, as its player would, then ticks
/// until `target` first loses health; returns the loss.
fn first_hit(
    scene: &mut HeadlessScenario,
    attacker: u64,
    target: u64,
    mut each: impl FnMut(&HeadlessScenario),
) -> i32 {
    use crate::sim::command::{Command, CommandEnvelope};
    let full = scene
        .sim()
        .entities()
        .get(target)
        .expect("target")
        .health
        .current();
    let order = {
        let sim = scene.sim();
        CommandEnvelope::new(
            sim.interner.get("Americans").expect("Americans house"),
            sim.session.tick() + 1,
            Command::clicked_mission(
                attacker,
                crate::sim::mission::MissionType::Attack,
                crate::sim::combat::TargetKind::Entity(target),
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
                &mut crate::sim::world::FrameEffects::default(),
            )
            .expect("frame advances");
        each(scene);
        let now = scene
            .sim()
            .entities()
            .get(target)
            .expect("target lives")
            .health
            .current();
        if now < full {
            return full - now;
        }
    }
    panic!("the GIs never fought");
}

/// A doubled `FirepowerMultiplier` doubles the GI's real shot, and the
/// modifier stops applying in the module phase of its 150th frame.
#[test]
fn a_firepower_modifier_doubles_real_damage_and_expires_on_time() {
    let Some((mut plain, attacker, target)) = gi_duel() else {
        return;
    };
    let baseline = first_hit(&mut plain, attacker, target, |_| {});
    let (mut scene, attacker, target) = gi_duel().expect("second duel");
    let firepower = prop("FirepowerMultiplier");
    let module = wat::parse_str(format!(
        r#"(module
  (import "vera:sim@1.0" "modify" (func $modify (param i64 i32 i32 i64 i32 i64 i32 i32) (result i32)))
  (import "vera:sim@1.0" "get" (func $get (param i64 i32 i32) (result i32)))
  (import "vera:sim@1.0" "frame" (func $frame (result i32)))
  (memory (export "memory") 1)
  (func (export "on_scenario_start")
    (i32.store (i32.const 80) (call $modify (i64.const {attacker}) (i32.const {firepower})
      (i32.const 1) (i64.const 131072) (i32.const 150) (i64.const 0) (i32.const 0) (i32.const 64)))
    (i32.store (i32.const 88) (call $frame)))
  (func (export "on_frame")
    (i32.store (i32.const 92) (call $frame))
    (drop (call $get (i64.const {attacker}) (i32.const {firepower}) (i32.const 96)))))"#
    ))
    .expect("test module parses");
    install(&mut scene, ModuleSource::Map, &module);
    let mut checked = 0;
    let mut check = |scene: &HeadlessScenario| {
        let start = mem_i32(scene, 0, 88);
        let frame = mem_i32(scene, 0, 92);
        let expected = if frame < start + 150 { 131_072 } else { 65_536 };
        assert_eq!(mem_i64(scene, 0, 96), expected, "frame {frame} of {start}");
        assert_eq!(
            scene.sim().ext_modules().modifiers().is_empty(),
            frame >= start + 150
        );
        checked += 1;
    };
    let doubled = first_hit(&mut scene, attacker, target, &mut check);
    assert_eq!(mem_i32(&scene, 0, 80), 0, "modify succeeded");
    assert!(baseline > 0);
    assert_eq!(doubled, 2 * baseline);
    while mem_i32(&scene, 0, 92) < mem_i32(&scene, 0, 88) + 152 {
        scene.tick();
        check(&scene);
    }
    assert!(checked > 150);
    assert_eq!(scene.sim().ext_modules().status()[0].1, None);
}

/// An unlimited modifier, its handle and its effect on the object come back
/// from a save.
#[test]
fn a_modifier_survives_save_and_load() {
    let Some(mut live) = scenario() else { return };
    let (unit, _) = placed_unit(&live);
    let speed = prop("SpeedMultiplier");
    let module = wat::parse_str(format!(
        r#"(module
  (import "vera:sim@1.0" "modify" (func $modify (param i64 i32 i32 i64 i32 i64 i32 i32) (result i32)))
  (import "vera:sim@1.0" "get" (func $get (param i64 i32 i32) (result i32)))
  (memory (export "memory") 1)
  (func (export "on_scenario_start")
    (drop (call $modify (i64.const {unit}) (i32.const {speed})
      (i32.const 1) (i64.const 32768) (i32.const -1) (i64.const 0) (i32.const 7) (i32.const 64))))
  (func (export "on_frame")
    (drop (call $get (i64.const {unit}) (i32.const {speed}) (i32.const 96)))
    (drop (call $get (i64.load (i32.const 64)) (i32.const {remaining}) (i32.const 104)))))"#,
        remaining = prop("Remaining"),
    ))
    .expect("test module parses");
    install(&mut live, ModuleSource::Map, &module);
    for _ in 0..3 {
        live.tick();
    }
    let mut resumed = saved_and_loaded(&live);
    let speed_of = |scene: &HeadlessScenario| {
        scene
            .sim()
            .entities()
            .get(unit)
            .expect("unit")
            .effect_speed_multiplier()
    };
    assert_eq!(speed_of(&resumed), 0.5);
    assert_eq!(
        resumed.sim().ext_modules().modifiers(),
        live.sim().ext_modules().modifiers()
    );
    assert_eq!(ext_hash(&resumed), ext_hash(&live));
    for _ in 0..5 {
        live.tick();
        resumed.tick();
    }
    assert_eq!(ext_hash(&resumed), ext_hash(&live));
    assert_eq!(mem_i64(&resumed, 0, 96), 32_768);
    assert_eq!(mem_i64(&resumed, 0, 104), -1, "unlimited");
}

/// `vera.vars` bytes for (name, scope, visibility, default) entries.
fn vars_section(entries: &[(&str, u8, u8, i64)]) -> String {
    let mut bytes = vec![1];
    bytes.extend_from_slice(&(entries.len() as u32).to_le_bytes());
    for (name, scope, visibility, default) in entries {
        bytes.extend_from_slice(&(name.len() as u16).to_le_bytes());
        bytes.extend_from_slice(name.as_bytes());
        bytes.extend_from_slice(&[*scope, *visibility]);
        bytes.extend_from_slice(&default.to_le_bytes());
    }
    let escaped: String = bytes.iter().map(|b| format!("\\{b:02x}")).collect();
    format!(r#"(@custom "vera.vars" "{escaped}")"#)
}

/// Declared variables: IDs from 1000, defaults, global, house and object
/// scope, a `find` by object variable, saves, removal with the object, and a
/// wrong scope stopping the module.
#[test]
fn declared_variables_keep_their_scopes_defaults_and_saves() {
    let Some(mut scene) = scenario() else { return };
    let (unit, house) = placed_unit(&scene);
    let house = house_ref(house);
    let section = vars_section(&[("Wave", 0, 3, 7), ("Score", 1, 1, 3), ("Marked", 2, 2, 0)]);
    let module = wat::parse_str(format!(
        r#"(module
  {section}
  (import "vera:sim@1.0" "lookup" (func $lookup (param i32 i32 i32 i32) (result i32)))
  (import "vera:sim@1.0" "var_get" (func $var_get (param i64 i32 i32) (result i32)))
  (import "vera:sim@1.0" "var_set" (func $var_set (param i64 i32 i64) (result i32)))
  (import "vera:sim@1.0" "find" (func $find (param i32 i32 i32 i32 i32) (result i32)))
  (memory (export "memory") 1)
  (data (i32.const 400) "Wavemap.ScoreMARKED")
  (func (export "on_scenario_start")
    (drop (call $lookup (i32.const 15) (i32.const 400) (i32.const 4) (i32.const 0)))
    (drop (call $lookup (i32.const 15) (i32.const 404) (i32.const 9) (i32.const 4)))
    (drop (call $lookup (i32.const 15) (i32.const 413) (i32.const 6) (i32.const 8)))
    (drop (call $var_get (i64.const 0) (i32.const 1000) (i32.const 16)))
    (drop (call $var_get (i64.const {house}) (i32.const 1001) (i32.const 24)))
    (drop (call $var_get (i64.const {unit}) (i32.const 1002) (i32.const 32)))
    (drop (call $var_set (i64.const 0) (i32.const 1000) (i64.const 9)))
    (drop (call $var_set (i64.const {house}) (i32.const 1001) (i64.const 5)))
    (drop (call $var_set (i64.const {unit}) (i32.const 1002) (i64.const 1)))
    ;; Query at 128: Objects, whole map, one clause Var(Subject, 1002) = 1.
    (i32.store (i32.const 184) (i32.const 1))
    (i32.store8 (i32.const 192) (i32.const 3))
    (i32.store8 (i32.const 194) (i32.const 1))
    (i32.store (i32.const 196) (i32.const 1002))
    (i64.store (i32.const 200) (i64.const 1))
    (i32.store (i32.const 40)
      (call $find (i32.const 128) (i32.const 80) (i32.const 256) (i32.const 4) (i32.const 224)))))"#
    ))
    .expect("test module parses");
    install(&mut scene, ModuleSource::Map, &module);
    scene.tick();
    assert_eq!(scene.sim().ext_modules().status()[0].1, None);
    let ids: Vec<i32> = (0..3).map(|i| mem_i32(&scene, 0, i * 4)).collect();
    assert_eq!(ids, vec![1000, 1001, 1002]);
    let defaults: Vec<i64> = [16, 24, 32].map(|at| mem_i64(&scene, 0, at)).to_vec();
    assert_eq!(defaults, vec![7, 3, 0]);
    assert_eq!(mem_i32(&scene, 0, 40), 0, "find succeeded");
    assert_eq!(mem_i32(&scene, 0, 224), 1, "one marked object");
    assert_eq!(mem_i64(&scene, 0, 256), unit as i64);
    let read = |scene: &HeadlessScenario, scope: i64, var: i32| {
        let sim = scene.sim();
        sim.ext_modules().declared_var(sim, scope, var)
    };
    let values = |scene: &HeadlessScenario| {
        [(0, 1000), (house, 1001), (unit as i64, 1002)].map(|(s, v)| read(scene, s, v))
    };
    assert_eq!(values(&scene), [Some(9), Some(5), Some(1)]);

    let resumed = saved_and_loaded(&scene);
    assert_eq!(values(&resumed), [Some(9), Some(5), Some(1)]);
    assert_eq!(ext_hash(&resumed), ext_hash(&scene));

    // The object leaves the game: its variable goes with it.
    scene.runtime.simulation.uninit(unit);
    for _ in 0..3 {
        scene.tick();
    }
    assert!(scene.sim().entities().get(unit).is_none());
    assert!(scene.sim().ext_modules.vars.values.object.is_empty());

    // An object variable read with global scope stops the module that does it.
    let wrong = wat::parse_str(
        r#"(module
  (import "vera:sim@1.0" "var_get" (func $var_get (param i64 i32 i32) (result i32)))
  (memory (export "memory") 1)
  (func (export "on_frame") (drop (call $var_get (i64.const 0) (i32.const 1002) (i32.const 0)))))"#,
    )
    .expect("test module parses");
    install(&mut scene, ModuleSource::Mod, &wrong);
    scene.tick();
    let status = scene.sim().ext_modules().status();
    let reason = status[0].1.as_deref().expect("the mod module stopped");
    assert!(reason.contains("scope"), "{reason}");
    assert_eq!(status[1].1, None);
}

#[test]
fn a_module_with_a_malformed_vars_section_never_installs() {
    let Some(mut scene) = scenario() else { return };
    let mut section = vars_section(&[("Wave", 0, 0, 0)]);
    // One byte past the declared content.
    section.insert_str(section.len() - 2, "\\00");
    let module = wat::parse_str(format!(
        r#"(module {section} (memory (export "memory") 1))"#
    ))
    .expect("test module parses");
    let error = scene
        .runtime
        .simulation
        .install_ext_module(&scene.runtime.resources.rules, ModuleSource::Map, &module)
        .unwrap_err();
    assert!(error.contains("trailing"), "{error}");
    assert!(scene.sim().ext_modules().is_empty());
}

/// `set` writes Health (clamped to Strength; 0 refused), Owner and Rank
/// through their owners.
#[test]
fn a_module_writes_health_owner_and_rank_through_their_owners() {
    let Some(mut scene) = scenario() else { return };
    let (unit, other) = placed_unit(&scene);
    let strength = {
        let sim = scene.sim();
        let entity = sim.entities().get(unit).expect("unit");
        sim.entity_object_type(entity, &scene.runtime.resources.rules)
            .expect("type")
            .strength
    };
    let module = wat::parse_str(format!(
        r#"(module
  (import "vera:sim@1.0" "set" (func $set (param i64 i32 i64) (result i32)))
  (import "vera:sim@1.0" "get" (func $get (param i64 i32 i32) (result i32)))
  (memory (export "memory") 1)
  (func (export "on_scenario_start")
    (i32.store (i32.const 0) (call $set (i64.const {unit}) (i32.const {health}) (i64.const 99999)))
    (drop (call $get (i64.const {unit}) (i32.const {health}) (i32.const 64)))
    (i32.store (i32.const 4) (call $set (i64.const {unit}) (i32.const {health}) (i64.const 10)))
    (i32.store (i32.const 8) (call $set (i64.const {unit}) (i32.const {health}) (i64.const 0)))
    (i32.store (i32.const 12) (call $set (i64.const {unit}) (i32.const {owner}) (i64.const {house})))
    (i32.store (i32.const 16) (call $set (i64.const {unit}) (i32.const {rank}) (i64.const 2)))))"#,
        health = prop("Health"),
        owner = prop("Owner"),
        rank = prop("Rank"),
        house = house_ref(other),
    ))
    .expect("test module parses");
    install(&mut scene, ModuleSource::Map, &module);
    scene.tick();
    assert_eq!(scene.sim().ext_modules().status()[0].1, None);
    let statuses: Vec<i32> = (0..5).map(|i| mem_i32(&scene, 0, i * 4)).collect();
    assert_eq!(statuses, vec![0, 0, 3, 0, 0]);
    assert_eq!(mem_i64(&scene, 0, 64), i64::from(strength));
    let sim = scene.sim();
    let entity = sim.entities().get(unit).expect("unit lives");
    assert_eq!(entity.health.current(), 10);
    assert_eq!(entity.owner(), sim.houses.order()[other]);
    assert_eq!(
        entity.veterancy.rank_kind(),
        crate::sim::combat::veterancy::VeterancyRank::Elite
    );
}

/// Runs the SDK's `modifier-vars` example when `VERA20K_TEST_MODIFIER_WASM`
/// names its build (`sdk/examples/modifier-vars`).
#[test]
fn a_rust_built_modifier_module_runs() {
    let Some(path) = std::env::var_os("VERA20K_TEST_MODIFIER_WASM") else {
        return;
    };
    let Some(mut scene) = scenario() else { return };
    let first = scene
        .sim()
        .entities()
        .iter_sorted()
        .next()
        .map(|(id, _)| id)
        .expect("the map has objects");
    let bytes = std::fs::read(&path).expect("module file reads");
    install(&mut scene, ModuleSource::Mod, &bytes);
    for _ in 0..45 {
        scene.tick();
    }
    assert_eq!(scene.sim().ext_modules().status()[0].1, None);
    let log = scene.runtime.simulation.ext_modules_mut().take_log();
    for line in ["doubled", "variables set", "expired"] {
        let line = format!("modifier-vars: {line}");
        assert!(log.contains(&line), "{line} in {log:?}");
    }
    let sim = scene.sim();
    assert_eq!(sim.ext_modules().declared_var(sim, 0, 1000), Some(8));
    assert_eq!(
        sim.ext_modules().declared_var(sim, first as i64, 1001),
        Some(1)
    );
}
