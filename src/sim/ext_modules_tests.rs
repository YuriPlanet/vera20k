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
