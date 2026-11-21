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

/// `ActionParams` with `id`, `arg`, `value` and `house` set, as WAT stores
/// at `at` (appendix "Fixed layouts").
fn params_wat(at: u32, id: i32, arg: i32, value: i64, house: i64) -> String {
    format!(
        "(i32.store (i32.const {at}) (i32.const {id})) (i32.store (i32.const {}) (i32.const {arg})) \
         (i64.store (i32.const {}) (i64.const {value})) (i64.store (i32.const {}) (i64.const {house}))",
        at + 4,
        at + 16,
        at + 40
    )
}

/// One module call kills an object, damages another, creates an infantryman
/// on a free cell and runs stock action `SetGlobal` through the map trigger
/// executor; every effect is the owners' own.
#[test]
fn atomic_and_stock_actions_change_the_game_through_their_owners() {
    use crate::map::entities::EntityCategory;
    let Some(mut scene) = scenario() else { return };
    let (victim, hurt, health, owner, free, warhead, e1) = {
        let sim = scene.sim();
        let rules = &scene.runtime.resources.rules;
        let mut ground = sim.entities().iter_sorted().filter(|(_, e)| {
            matches!(
                e.category(),
                EntityCategory::Unit | EntityCategory::Infantry
            ) && e.health.current() > 20
                && !e.lifecycle.in_limbo()
        });
        let (victim, _) = ground.next().expect("a ground object");
        let (hurt, hurt_entity) = ground.next().expect("a second ground object");
        let health = hurt_entity.health.current();
        let owner = sim
            .houses
            .index_of(hurt_entity.owner())
            .expect("owner index");
        let (hx, hy) = (hurt_entity.position.rx(), hurt_entity.position.ry());
        // Cells near the damaged object with no object or tree; the module
        // tries them in turn, since the land may refuse an infantryman.
        let free: Vec<(u16, u16)> = (hx.saturating_sub(3)..=hx + 3)
            .flat_map(|x| (hy.saturating_sub(3)..=hy + 3).map(move |y| (x, y)))
            .filter(|&(x, y)| {
                !sim.terrain_objects.cell_index().contains_key(&(x, y))
                    && sim
                        .entities()
                        .iter_sorted()
                        .all(|(_, e)| (e.position.rx(), e.position.ry()) != (x, y))
            })
            .take(32)
            .collect();
        assert!(!free.is_empty(), "a free cell");
        let warhead = rules.bridge_warheads.c4_name.clone();
        let e1 = rules.type_handle("E1").expect("E1").0;
        (victim, hurt, health, owner, free, warhead, e1)
    };
    let house = house_ref(owner);
    let module = wat::parse_str(format!(
        r#"(module
  (import "vera:sim@1.0" "lookup" (func $lookup (param i32 i32 i32 i32) (result i32)))
  (import "vera:sim@1.0" "action" (func $action (param i32 i64 i32 i32 i32) (result i32)))
  (memory (export "memory") 1)
  (data (i32.const 0) "{warhead}")
  (data (i32.const 64) "SetGlobal")
  (func (export "on_scenario_start") (local $i i32)
    {store_cells}
    (drop (call $lookup (i32.const 2) (i32.const 0) (i32.const {wlen}) (i32.const 1000)))
    (drop (call $lookup (i32.const 21) (i32.const 64) (i32.const 9) (i32.const 1004)))
    {damage}
    (i32.store (i32.const 256) (i32.load (i32.const 1000)))
    {create}
    {global}
    (i32.store (i32.const 1100) (call $action (i32.const 1005) (i64.const {victim}) (i32.const 128) (i32.const 72) (i32.const 2000)))
    (i32.store (i32.const 1104) (call $action (i32.const 1006) (i64.const {hurt}) (i32.const 256) (i32.const 72) (i32.const 2008)))
    ;; CreateObject on each candidate cell until one admits it.
    (block $made (loop $next
      (i32.store (i32.const 1108) (call $action (i32.const 1008)
        (i64.load (i32.add (i32.const 4096) (i32.mul (local.get $i) (i32.const 8))))
        (i32.const 384) (i32.const 72) (i32.const 2016)))
      (br_if $made (i32.eqz (i32.load (i32.const 1108))))
      (local.set $i (i32.add (local.get $i) (i32.const 1)))
      (br_if $next (i32.lt_u (local.get $i) (i32.const {count})))))
    (i32.store (i32.const 1112) (call $action (i32.load (i32.const 1004)) (i64.const 0) (i32.const 512) (i32.const 72) (i32.const 2024)))))"#,
        wlen = warhead.len(),
        count = free.len(),
        store_cells = free
            .iter()
            .enumerate()
            .map(|(i, &(x, y))| format!(
                "(i64.store (i32.const {}) (i64.const {}))",
                4096 + 8 * i,
                super::cell_ref(x, y)
            ))
            .collect::<Vec<_>>()
            .join(" "),
        damage = params_wat(256, 0, 0, 10, 0),
        create = params_wat(384, e1 as i32, 255, 0, house),
        global = params_wat(512, 0, 0, 7, 0),
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
    let i32_at = |at: usize| i32::from_le_bytes(memory[at..at + 4].try_into().expect("4 bytes"));
    let created = i64::from_le_bytes(memory[2016..2024].try_into().expect("8 bytes"));
    assert_eq!(i32_at(1004), 28, "lookup(ActionKind, SetGlobal)");
    assert_eq!(
        [i32_at(1100), i32_at(1104), i32_at(1108), i32_at(1112)],
        [0; 4],
        "every action is Ok"
    );
    let sim = scene.sim();
    assert!(
        sim.entities()
            .get(victim)
            .is_none_or(|e| e.health.current() == 0),
        "Kill takes all health"
    );
    let damaged = sim.entities().get(hurt).expect("damaged object stays");
    assert!(damaged.health.current() < health && damaged.health.current() > 0);
    let infantry = sim
        .entities()
        .get(created as u64)
        .expect("created object is live");
    assert_eq!(sim.resolve(infantry.type_ref()), "E1");
    assert_eq!(sim.houses.index_of(infantry.owner()), Some(owner));
    assert!(sim.trigger_runtime.global_variable_is_set(7));
}

/// A direct call with house 0 is an invalid argument: the module stops.
#[test]
fn an_action_with_an_invalid_parameter_disables_the_module() {
    let Some(mut scene) = scenario() else { return };
    let module = wat::parse_str(format!(
        r#"(module
  (import "vera:sim@1.0" "action" (func $action (param i32 i64 i32 i32 i32) (result i32)))
  (memory (export "memory") 1)
  (func (export "on_scenario_start")
    (drop (call $action (i32.const {ACTION_ADD_MONEY}) (i64.const 0) (i32.const 128) (i32.const 72) (i32.const 256)))))"#
    ))
    .expect("test module parses");
    scene
        .runtime
        .simulation
        .install_ext_module(&scene.runtime.resources.rules, ModuleSource::Map, &module)
        .expect("module installs");
    scene.tick();
    let status = scene.sim().ext_modules().status();
    let reason = status[0].1.as_deref().expect("the module stopped");
    assert!(reason.contains("house 0"), "{reason}");
}

/// Runs `sdk/examples/actions-demo` when `VERA20K_TEST_ACTIONS_WASM` names
/// its `rules.wasm`: SDK actions reach the owners.
#[test]
fn a_rust_built_actions_module_runs() {
    let Some(path) = std::env::var_os("VERA20K_TEST_ACTIONS_WASM") else {
        return;
    };
    let Some(mut scene) = scenario() else { return };
    let bytes = std::fs::read(&path).expect("module file reads");
    scene
        .runtime
        .simulation
        .install_ext_module(&scene.runtime.resources.rules, ModuleSource::Mod, &bytes)
        .expect("module installs");
    scene.tick();
    assert_eq!(scene.sim().ext_modules().status()[0].1, None);
    assert!(scene.sim().trigger_runtime.global_variable_is_set(3));
    let log = scene.runtime.simulation.ext_modules_mut().take_log();
    assert!(log.iter().any(|l| l == "actions-demo: damage"), "{log:?}");
    assert!(!log.iter().any(|l| l == "actions-demo: failed"), "{log:?}");
}
