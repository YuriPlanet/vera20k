//! The module phase on a retail map: effects through the owners,
//! determinism, a runaway module, and host-call cost.

use super::{ACTION_ADD_MONEY, ModuleSource, house_ref};
use crate::headless_scenario::HeadlessScenario;
use crate::rules::ini_parser::IniFile;

const MAP: &str = "XMP03T4.MAP";
const SEED: u32 = 0x1269;

pub(super) fn scenario() -> Option<HeadlessScenario> {
    let (retail, _) = crate::rules::retail_ini_fixture::retail_assets()?;
    Some(crate::headless_scenario::load(&retail, MAP, SEED).expect("retail scenario loads"))
}

/// The Battle on [`MAP`] against one computer house.
pub(super) fn session_with_computer() -> crate::skirmish_launch::SkirmishLaunchSession {
    session_against_computer(MAP)
}

/// The Battle on `map` against one computer house.
fn session_against_computer(map: &str) -> crate::skirmish_launch::SkirmishLaunchSession {
    crate::headless_scenario::battle_session(
        map,
        &[(
            crate::skirmish_launch::LaunchCountry::Russia,
            crate::skirmish_launch::AiDifficulty::Easy,
        )],
    )
}

/// [`scenario`] against one computer house.
pub(super) fn scenario_with_computer() -> Option<HeadlessScenario> {
    let (retail, _) = crate::rules::retail_ini_fixture::retail_assets()?;
    Some(
        crate::headless_scenario::load_session(&retail, MAP, SEED, session_with_computer())
            .expect("retail scenario loads"),
    )
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

/// Actual AssemblyScript modules from the SDK's example build, played on both
/// sides of a player-world restore. The short regression compares all saved
/// module data, including memory, globals, RNG, triggers and pending orders.
#[test]
fn typescript_modules_restore_their_complete_state() {
    check_typescript_module_restore(30, false);
}

/// RFC 0001 open question 2 additionally requires whole-game equality across
/// 9,000 frames, no traps and bounded module memory. Keep that stronger
/// criterion independently runnable while compiler selection remains open.
#[test]
#[ignore = "slow; RFC 0001 whole-game compiler acceptance experiment"]
fn typescript_modules_resume_from_a_save_exactly() {
    check_typescript_module_restore(4500, true);
}

fn check_typescript_module_restore(half: u32, whole_game: bool) {
    let Some(paths) = std::env::var_os("VERA20K_TEST_TS_WASM") else {
        return;
    };
    let save = |scene: &HeadlessScenario| {
        crate::sim::snapshot::GameSnapshot::save_validated(
            scene.sim(),
            0,
            scene.runtime.resources.rules.simulation_config_hash(),
            "ext modules",
            0,
        )
    };
    let module_state = |scene: &HeadlessScenario| {
        crate::util::save_codec::serialize(scene.sim().ext_modules()).expect("module state saves")
    };
    for path in std::env::split_paths(&paths) {
        let Some(mut live) = scenario_with_computer() else {
            return;
        };
        let bytes = std::fs::read(&path).expect("module file reads");
        live.runtime
            .simulation
            .install_ext_module(&live.runtime.resources.rules, ModuleSource::Mod, &bytes)
            .expect("module installs");
        for _ in 0..half {
            live.tick();
        }
        let halfway = live.sim().ext_modules().memory_bytes()[0];
        let mut resumed = scenario_with_computer().expect("second load");
        let snapshot = crate::sim::snapshot::GameSnapshot::load(&save(&live)).expect("save loads");
        let (restored, _) = snapshot
            .restore_player_world(
                crate::sim::world::SnapshotProcessState::Live(live.sim()),
                &resumed.runtime.resources.rules,
                resumed.sim().resolved_terrain().expect("terrain").clone(),
                *resumed.sim().terrain_speed_config(),
                crate::sim::snapshot::PlayerSaveCells::FixtureBaseline,
            )
            .expect("player world restores");
        resumed.runtime.simulation = restored;
        assert!(
            module_state(&resumed) == module_state(&live),
            "{} at restore",
            path.display()
        );
        for frame in 1..=half {
            live.tick();
            resumed.tick();
            if frame % 500 == 0 || frame == half {
                assert!(
                    module_state(&resumed) == module_state(&live),
                    "{} after {frame} frames",
                    path.display()
                );
            }
        }
        let status = live.sim().ext_modules().status();
        assert_eq!(status[0].1, None, "{} stopped", path.display());
        assert_eq!(resumed.sim().ext_modules().status(), status);
        if whole_game {
            let end = live.sim().ext_modules().memory_bytes()[0];
            assert_eq!(
                end,
                halfway,
                "{}: memory grew after frame {half}",
                path.display()
            );
            assert_eq!(
                resumed.sim().state_hash(),
                live.sim().state_hash(),
                "{}: final game hash",
                path.display()
            );
            assert!(
                save(&resumed) == save(&live),
                "{}: the saves differ at frame {}",
                path.display(),
                2 * half
            );
        }
    }
}

/// Smoke-runs the playtest mods (`sdk/examples/veteran-fury`,
/// `siege-doctrine`, `meteor-storm`, `trigger-lab`) when
/// `VERA20K_TEST_PLAYTEST_WASM` lists their modules, `:`-separated: each
/// plays 2000 frames of a Battle against one computer house and must still
/// be running. Prints each module's log (`--nocapture`).
#[test]
fn playtest_modules_keep_running() {
    let Some(paths) = std::env::var_os("VERA20K_TEST_PLAYTEST_WASM") else {
        return;
    };
    for path in std::env::split_paths(&paths) {
        let Some(mut scene) = scenario_with_computer() else {
            return;
        };
        let bytes = std::fs::read(&path).expect("module file reads");
        scene
            .runtime
            .simulation
            .install_ext_module(&scene.runtime.resources.rules, ModuleSource::Mod, &bytes)
            .expect("module installs");
        for _ in 0..2000 {
            scene.tick();
        }
        let status = scene.sim().ext_modules().status();
        let log = scene.runtime.simulation.ext_modules_mut().take_log();
        eprintln!("== {}", path.display());
        for line in &log {
            eprintln!("   {line}");
        }
        assert_eq!(status[0].1, None, "{} stopped; log {log:?}", path.display());
        assert!(
            !log.iter().any(|l| l.contains("refused")),
            "{}: {log:?}",
            path.display()
        );
    }
}

/// The INI demo mods (`sdk/examples/ini-*`) when `VERA20K_TEST_INI_DEMOS`
/// lists them, `;`-separated, each `module.wasm|rulesmd.ini|MARK1,MARK2`
/// with an optional `|FRAMES` (3000 by default):
/// the module installs (its `vera_init` registers its INI names), the mod's
/// INI becomes the merged rules text and its `Wasm.Tags` triggers are built,
/// then that many frames of a Battle against one computer house run. The module
/// must still be running, nothing may be refused, and its log must hold a
/// line `DEMO-OK <MARK>` for every listed mark. Prints each log
/// (`--nocapture`).
#[test]
fn ini_demo_mods_reach_their_marks() {
    let Some(demos) = std::env::var("VERA20K_TEST_INI_DEMOS").ok() else {
        return;
    };
    for demo in demos.split(';').filter(|d| !d.trim().is_empty()) {
        let mut parts = demo.split('|');
        let (Some(wasm), Some(ini), marks) = (parts.next(), parts.next(), parts.next()) else {
            panic!("{demo}: expected module.wasm|rulesmd.ini|MARKS[|FRAMES]");
        };
        let frames: u32 = parts
            .next()
            .map_or(3000, |f| f.trim().parse().expect("FRAMES is a number"));
        let Some(mut scene) = scenario_with_computer() else {
            return;
        };
        let bytes = std::fs::read(wasm.trim()).expect("module file reads");
        let text = std::fs::read_to_string(ini.trim()).expect("rulesmd.ini reads");
        let mod_ini = crate::rules::ini_parser::IniFile::from_str(&text);
        let rules = &scene.runtime.resources.rules;
        let sim = &mut scene.runtime.simulation;
        sim.install_ext_module(rules, ModuleSource::Mod, &bytes)
            .expect("module installs");
        sim.ext_modules.rules_ini = Some(mod_ini.clone());
        let triggers = super::ini_triggers::IniTriggers::read(&mod_ini)
            .unwrap_or_else(|e| panic!("{ini}: {e}"));
        sim.install_ini_triggers(rules, &triggers)
            .unwrap_or_else(|e| panic!("{ini}: {e}"));
        // The engine keeps only the latest log lines; collect them as they come.
        let mut log = Vec::new();
        for _ in 0..frames {
            scene.tick();
            log.extend(scene.runtime.simulation.ext_modules_mut().take_log());
        }
        let status = scene.sim().ext_modules().status();
        eprintln!("== {wasm}");
        for line in &log {
            eprintln!("   {line}");
        }
        assert_eq!(status[0].1, None, "{wasm} stopped; log {log:?}");
        assert!(
            !log.iter().any(|l| l.contains("refused")),
            "{wasm}: {log:?}"
        );
        for mark in marks
            .unwrap_or("")
            .split(',')
            .map(str::trim)
            .filter(|m| !m.is_empty())
        {
            let line = format!("DEMO-OK {mark}");
            assert!(
                log.iter().any(|l| l.contains(&line)),
                "{wasm}: no `{line}` in {log:?}"
            );
        }
    }
}

/// The Control Bench (`sdk/examples/control-bench`) played as a mod when
/// `VERA20K_TEST_CONTROL_BENCH` names its built module
/// (`VERA20K_TEST_CONTROL_BENCH_FRAMES` frames, 6000 by default). The test
/// makes the mod's folder as a modder ships it (docs/modding.md): the module
/// as `rules.wasm`; `rulesmd.ini`, the retail file with the bench's
/// `rulesmd.ini` merged in, since a mod's `rulesmd.ini` takes the place of
/// the game's; `mod.ini`; and the bench's `ControlBench.map`. The match then loads through
/// the mod: the module's `vera_init` and `on_rules` run while the rules are
/// processed, its `Wasm.Tags` triggers come from the mod's `rulesmd.ini`.
/// A Battle on the bench's map against one computer house runs that many
/// frames. The module must still be running, reach `DEMO-OK bench.all` and
/// log no refusals or `BENCH-FAIL` lines. A failure names the marks
/// of the bench's latest `BENCH-MISSING` lines that no `DEMO-OK` line
/// reached later. Prints the log
/// (`--nocapture`).
#[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
#[test]
fn control_bench_reaches_every_mark() {
    const BENCH_MAP: &str = "ControlBench.map";
    let Some(wasm) = std::env::var_os("VERA20K_TEST_CONTROL_BENCH") else {
        return;
    };
    let Some((retail, assets)) = crate::rules::retail_ini_fixture::retail_assets() else {
        return;
    };
    let frames: u32 = std::env::var("VERA20K_TEST_CONTROL_BENCH_FRAMES").map_or(6000, |f| {
        f.trim()
            .parse()
            .expect("VERA20K_TEST_CONTROL_BENCH_FRAMES is a number")
    });
    let bench = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("sdk/examples/control-bench");
    let read = |name: &str| {
        std::fs::read_to_string(bench.join(name))
            .unwrap_or_else(|e| panic!("{}: {e}", bench.join(name).display()))
    };
    let retail_file = |name: &str| {
        assets
            .resolve_ref(name)
            .unwrap_or_else(|| panic!("the installation has no {name}"))
            .bytes
            .to_vec()
    };
    let folder = tempfile::tempdir().expect("create the bench mod folder");
    std::fs::copy(&wasm, folder.path().join("rules.wasm")).expect("the module copies");
    let files = [
        ("mod.ini", read("mod.ini").into_bytes()),
        (
            "rulesmd.ini",
            merge_ini_text(&retail_file("rulesmd.ini"), &read("rulesmd.ini")),
        ),
        (
            BENCH_MAP,
            merge_ini_text(&retail_file(MAP), &read("map.ini")),
        ),
    ];
    for (name, bytes) in files {
        std::fs::write(folder.path().join(name), bytes).expect("the mod's file is written");
    }
    let mut scene = crate::headless_scenario::load_with_mod_folder(
        &retail,
        BENCH_MAP,
        SEED,
        session_against_computer(BENCH_MAP),
        folder.path(),
    )
    .expect("the bench loads as a mod");
    // The rules stage's lines, then each frame's: the engine keeps only the
    // latest lines, so collect them as they come.
    let mut log = scene.runtime.simulation.ext_modules_mut().take_log();
    for _ in 0..frames {
        scene.tick();
        log.extend(scene.runtime.simulation.ext_modules_mut().take_log());
    }
    eprintln!("== Control Bench, {frames} frames");
    for line in &log {
        eprintln!("   {line}");
    }
    let status = scene.sim().ext_modules().status();
    assert!(
        matches!(status.as_slice(), [(ModuleSource::Mod, None)]),
        "the bench is not running: {status:?}"
    );
    let failures: Vec<_> = log
        .iter()
        .filter(|line| line.starts_with("BENCH-FAIL ") || line.contains("refused"))
        .collect();
    let complete = log.iter().any(|line| line == "DEMO-OK bench.all");
    if !complete || !failures.is_empty() {
        // The latest report's list (the lines after the last `BENCH frame`
        // line that has any), less the marks that came after it.
        let mut missing: Vec<&str> = Vec::new();
        let mut report_seen = false;
        for line in &log {
            if line.starts_with("BENCH frame") {
                report_seen = true;
            } else if let Some(name) = line.strip_prefix("BENCH-MISSING ") {
                if report_seen {
                    missing.clear();
                    report_seen = false;
                }
                missing.push(name);
            }
        }
        let reached: std::collections::HashSet<&str> = log
            .iter()
            .filter_map(|l| l.strip_prefix("DEMO-OK "))
            .collect();
        missing.retain(|name| !reached.contains(name));
        panic!(
            "after {frames} frames: bench.all={complete}, missing={missing:?}, failures={failures:?}"
        );
    }
}

/// `fragment` merged into the INI text `full` as a modder adds a mod's
/// sections to a full copy of a file: the entries of each fragment section
/// go right after the first header of that section in `full` (a section's
/// first copy of a key wins), a section `full` lacks is appended. Comments
/// and blank lines of the fragment are dropped. Works on bytes: retail INI
/// text is not UTF-8.
fn merge_ini_text(full: &[u8], fragment: &str) -> Vec<u8> {
    let header = |line: &[u8]| -> Option<Vec<u8>> {
        let line = line.trim_ascii_start();
        let rest = line.strip_prefix(b"[")?;
        let end = rest.iter().position(|&b| b == b']')?;
        Some(rest[..end].to_vec())
    };
    let mut sections: Vec<(Vec<u8>, Vec<&str>, bool)> = Vec::new();
    for line in fragment.lines().map(str::trim) {
        if line.is_empty() || line.starts_with(';') {
            continue;
        }
        if let Some(name) = header(line.as_bytes()) {
            sections.push((name, Vec::new(), false));
        } else if let Some(section) = sections.last_mut() {
            section.1.push(line);
        }
    }
    let mut out = Vec::with_capacity(full.len() + fragment.len());
    for line in full.split_inclusive(|&b| b == b'\n') {
        out.extend_from_slice(line);
        if !line.ends_with(b"\n") {
            out.push(b'\n');
        }
        let Some(name) = header(line) else { continue };
        if let Some(section) = sections.iter_mut().find(|s| !s.2 && s.0 == name) {
            for entry in &section.1 {
                out.extend_from_slice(entry.as_bytes());
                out.push(b'\n');
            }
            section.2 = true;
        }
    }
    for (name, entries, merged) in &sections {
        if *merged {
            continue;
        }
        out.push(b'[');
        out.extend_from_slice(name);
        out.extend_from_slice(b"]\n");
        for entry in entries {
            out.extend_from_slice(entry.as_bytes());
            out.push(b'\n');
        }
    }
    out
}

#[test]
fn a_mod_fragment_merges_into_the_full_file() {
    let full = b"[A]\r\nX=1\r\n[B]\r\nY=2";
    let merged = merge_ini_text(full, "; note\n[B]\nZ=3\n\n[C]\nW=4\n");
    assert_eq!(
        String::from_utf8(merged).unwrap(),
        "[A]\r\nX=1\r\n[B]\r\nZ=3\nY=2\n[C]\nW=4\n"
    );
}

/// The player's units in Muster Field's camp, by cell: (id, cell, type).
type CampUnits = Vec<(u64, (u16, u16), String)>;

/// Loads Muster Field from `VERA20K_TEST_MUSTER_WASM` against one computer
/// house; the scene, the player's house index and the camp's units.
fn muster_field() -> Option<(HeadlessScenario, usize, crate::sim::intern::InternedId)> {
    let path = std::env::var_os("VERA20K_TEST_MUSTER_WASM")?;
    let mut scene = scenario_with_computer()?;
    let bytes = std::fs::read(path).expect("module file reads");
    scene
        .runtime
        .simulation
        .install_ext_module(&scene.runtime.resources.rules, ModuleSource::Mod, &bytes)
        .expect("module installs");
    for _ in 0..5 {
        scene.tick();
    }
    let sim = scene.sim();
    let (player, house) = sim
        .houses
        .order()
        .iter()
        .copied()
        .enumerate()
        .find(|(_, id)| sim.houses[id].is_human())
        .expect("the battle has a player");
    // The opening lines; a test reads what its orders bring.
    scene.runtime.simulation.ext_modules_mut().take_log();
    Some((scene, player, house))
}

fn camp_units(scene: &HeadlessScenario, house: crate::sim::intern::InternedId) -> CampUnits {
    let sim = scene.sim();
    let mut units: CampUnits = sim
        .entities()
        .iter_sorted()
        .filter(|(_, e)| {
            e.owner() == house && e.lifecycle.object_alive() && !e.lifecycle.in_limbo()
        })
        .map(|(id, e)| {
            let at = (e.position.rx(), e.position.ry());
            (id, at, sim.resolve(e.type_ref()).to_string())
        })
        .collect();
    units.sort_by_key(|unit| unit.1);
    units
}

/// Sends one order as a player's click travels: a native record through the
/// command queue, so `CommandAllowed` reads the decoded command.
fn order(
    scene: &mut HeadlessScenario,
    house: crate::sim::intern::InternedId,
    command: crate::sim::command::Command,
) {
    use crate::sim::command::{Command, CommandEnvelope};
    let sim = &mut scene.runtime.simulation;
    let record = match command {
        Command::Stop { entity_id } => sim.encode_stop_record(house, entity_id),
        Command::Scatter { entity_id } => sim.encode_scatter_record(house, entity_id),
        _ => sim.encode_megamission_record(house, &command),
    }
    .expect("the order encodes");
    let tick = sim.session.tick();
    sim.queue_command(CommandEnvelope::synchronized(record, tick));
    for _ in 0..4 {
        scene.tick();
    }
}

/// Plays frames until the module logs a line holding `wanted`; the lines.
fn play_until(scene: &mut HeadlessScenario, wanted: &str, frames: usize) -> Vec<String> {
    let mut log = Vec::new();
    for _ in 0..frames {
        scene.tick();
        log.extend(scene.runtime.simulation.ext_modules_mut().take_log());
        if log.iter().any(|line| line.contains(wanted)) {
            break;
        }
    }
    assert_eq!(scene.sim().ext_modules().status()[0].1, None, "{log:?}");
    log
}

/// Plays `sdk/examples/muster-field`: a muster that waits for the player,
/// hire, redraw, improve, a merge and its supply, then the first wave, all
/// from player orders.
#[test]
fn muster_field_plays_a_round_from_player_orders() {
    use crate::sim::combat::TargetKind;
    use crate::sim::command::Command;

    let Some((mut scene, player, house)) = muster_field() else {
        return;
    };
    let units = camp_units(&scene, house);
    let officer = units
        .iter()
        .find(|unit| unit.2 == "SSRV")
        .expect("the quartermaster stands")
        .clone();
    let row: CampUnits = units.iter().filter(|u| u.2 != "SSRV").cloned().collect();
    assert_eq!(row.len(), 3, "three offers at camp rank 1");
    // At unlimited game speed these frames pass in seconds.
    for _ in 0..4700 {
        scene.tick();
    }
    assert_eq!(camp_units(&scene, house), units, "the muster waits");
    assert_eq!(credits(&scene, player), 1600);
    let camp = row[1].1;
    let line = (camp.0, camp.1 + 5);
    let to = |entity_id, (target_rx, target_ry)| Command::Move {
        entity_id,
        target_rx,
        target_ry,
    };
    let guard = |entity_id, (x, y)| Command::Guard {
        entity_id,
        target: Some(TargetKind::Cell(x, y)),
    };

    // A move along the row hires nobody; one out of it does.
    order(&mut scene, house, to(row[0].0, camp));
    assert_eq!(credits(&scene, player), 1600);
    order(&mut scene, house, to(row[0].0, line));
    let after_hire = credits(&scene, player);
    assert!(after_hire < 1600, "the move hired");
    order(
        &mut scene,
        house,
        Command::Stop {
            entity_id: officer.0,
        },
    );
    assert_eq!(credits(&scene, player), after_hire - 50, "a redraw");
    order(&mut scene, house, guard(officer.0, officer.1));
    assert_eq!(credits(&scene, player), after_hire - 650, "an improvement");
    // A hired unit may still stand in the row.
    let mut taken = vec![row[0].0];
    let offers = |scene: &HeadlessScenario, taken: &[u64]| -> CampUnits {
        let units = camp_units(scene, house);
        units
            .into_iter()
            .filter(|u| u.1.1 == camp.1 && u.2 != "SSRV" && !taken.contains(&u.0))
            .collect()
    };
    assert_eq!(offers(&scene, &taken).len(), 4, "camp rank 2 offers four");

    // Hire by type until three of a kind merge into a champion.
    scene
        .runtime
        .simulation
        .houses
        .get_mut(&house)
        .expect("the player's house")
        .economy_mut()
        .add_credits(20_000);
    let mut hired: Vec<String> = vec![row[0].2.clone()];
    let mut log = Vec::new();
    for _ in 0..40 {
        if log.iter().any(|l: &String| l.contains("精锐")) {
            break;
        }
        let count = |ty: &str| hired.iter().filter(|h| *h == ty).count();
        let row = offers(&scene, &taken);
        match row.iter().max_by_key(|unit| count(&unit.2)) {
            Some(best) if count(&best.2) > 0 => {
                hired.push(best.2.clone());
                taken.push(best.0);
                order(&mut scene, house, to(best.0, line));
            }
            _ => order(
                &mut scene,
                house,
                Command::Stop {
                    entity_id: officer.0,
                },
            ),
        }
        log.extend(scene.runtime.simulation.ext_modules_mut().take_log());
    }
    assert!(log.iter().any(|l| l.contains("三合一")), "{log:?}");
    let trucks: CampUnits = camp_units(&scene, house)
        .into_iter()
        .filter(|u| ["TRUCKB", "PICK", "BUS"].contains(&u.2.as_str()))
        .collect();
    assert_eq!(trucks.len(), 3, "a merge offers three supplies");
    order(&mut scene, house, to(trucks[0].0, (camp.0, camp.1 - 9)));
    let log = scene.runtime.simulation.ext_modules_mut().take_log();
    assert!(log.iter().any(|l| l.contains("获得补给")), "{log:?}");

    // A champion alone is surrounded: two more go with it.
    for _ in 0..2 {
        let offer = offers(&scene, &taken)[0].0;
        taken.push(offer);
        order(&mut scene, house, to(offer, line));
    }
    let company = |scene: &HeadlessScenario| {
        camp_units(scene, house)
            .into_iter()
            .filter(|u| u.1.1 >= camp.1 + 2)
            .count()
    };
    // Out of the row and into the line.
    for _ in 0..150 {
        scene.tick();
    }
    let fielded = company(&scene);
    assert_eq!(fielded, 3, "a champion and two more");
    let before = credits(&scene, player);
    order(
        &mut scene,
        house,
        Command::Scatter {
            entity_id: officer.0,
        },
    );
    let log = play_until(&mut scene, "第2/8波", 9000);
    // A champion, a supply and a full company beat the first wave.
    assert!(log.iter().any(|l| l.starts_with("胜，歼")), "{log:?}");
    assert!(credits(&scene, player) >= before + 535, "the wage is paid");
    assert_eq!(company(&scene), fielded, "the dead return");
    eprintln!("{}", log.join("\n"));
}

/// An empty company loses three banners, and the campaign ends in defeat.
#[test]
fn muster_field_ends_when_the_last_banner_falls() {
    use crate::sim::command::Command;

    let Some((mut scene, _, house)) = muster_field() else {
        return;
    };
    let mut log = Vec::new();
    for banners in [2, 1] {
        let officer = camp_units(&scene, house)
            .into_iter()
            .find(|unit| unit.2 == "SSRV")
            .expect("the quartermaster stands");
        order(
            &mut scene,
            house,
            Command::Scatter {
                entity_id: officer.0,
            },
        );
        let wanted = "第1/8波【探路散兵】";
        log = play_until(&mut scene, wanted, 9000);
        let last = log.iter().find(|l| l.contains(wanted)).expect("a muster");
        assert!(last.starts_with("败，歼"), "{log:?}");
        assert!(last.ends_with(&format!("军旗{banners}")), "{log:?}");
    }
    let officer = camp_units(&scene, house)
        .into_iter()
        .find(|unit| unit.2 == "SSRV")
        .expect("the quartermaster stands");
    order(
        &mut scene,
        house,
        Command::Scatter {
            entity_id: officer.0,
        },
    );
    log.extend(play_until(&mut scene, "军旗倒下", 9000));
    assert!(
        log.iter().any(|l| l.contains("最后一面军旗倒下")),
        "{log:?}"
    );
}

/// Muster Field's variable `name` of house `index`, as its `ui.wasm` reads
/// it (the module publishes the camp, `muster_field_data::proto`).
fn muster_var(scene: &HeadlessScenario, index: usize, name: &str) -> i64 {
    let sim = scene.sim();
    let var = sim
        .ext_modules
        .vars
        .lookup(ModuleSource::Mod, name)
        .unwrap_or_else(|| panic!("Muster Field declares {name}"));
    let holder = sim
        .module_var_holder(house_ref(index), var)
        .expect("a house variable");
    sim.ext_modules.vars.read(var, holder).0
}

/// Sends the interface's two-byte message as its button does: through the
/// command stream to the mod's `on_message`.
fn muster_button(
    scene: &mut HeadlessScenario,
    house: crate::sim::intern::InternedId,
    what: u8,
    index: u8,
) {
    use crate::sim::command::CommandEnvelope;
    let sim = &mut scene.runtime.simulation;
    let record = sim
        .encode_module_message_record(house, 0, &[what, index])
        .expect("the message encodes");
    let tick = sim.session.tick();
    sim.queue_command(CommandEnvelope::synchronized(record, tick));
    for _ in 0..4 {
        scene.tick();
    }
}

/// Plays Muster Field from its interface alone, when
/// `VERA20K_TEST_MUSTER_UI_WASM` names its `ui.wasm` (and
/// `VERA20K_TEST_MUSTER_WASM` its `rules.wasm`): the published camp, and
/// each button doing what its key does (hire, keep, redraw, improve, a
/// dismissal, the battle).
#[test]
fn muster_field_plays_from_its_interface() {
    let Some(ui_path) = std::env::var_os("VERA20K_TEST_MUSTER_UI_WASM") else {
        return;
    };
    let ui = std::fs::read(ui_path).expect("ui.wasm reads");
    let client = vera20k_wasm_ext::ClientModule::load(&ui, vera20k_wasm_ext::Limits::MOD)
        .unwrap_or_else(|e| panic!("ui.wasm loads: {e}"));
    for export in ["client_on_start", "client_on_render", "client_on_events"] {
        assert!(client.has_export(export), "ui.wasm exports {export}");
    }
    let Some((mut scene, player, house)) = muster_field() else {
        return;
    };
    // proto's message kinds and variable layout.
    const HIRE: u8 = 1;
    const KEEP: u8 = 2;
    const REDRAW: u8 = 3;
    const IMPROVE: u8 = 4;
    const START: u8 = 5;
    const DISMISS: u8 = 7;
    let var = |scene: &HeadlessScenario, name| muster_var(scene, player, name);
    let byte = |packed: i64, at: usize| (packed >> (8 * at)) as u8;
    let member = |scene: &HeadlessScenario, at: usize| byte(var(scene, "MfCompanyLow"), at);

    assert_eq!(var(&scene, "MfPhase"), 1, "the muster is published");
    assert_eq!(var(&scene, "MfWave"), 1);
    assert_eq!(var(&scene, "MfBanners"), 3);
    assert_eq!(var(&scene, "MfFlags") & 1, 1, "the officer stands");
    let offers = var(&scene, "MfOffers");
    for at in 0..3 {
        assert_eq!(byte(offers, at) & 0x40, 0x40, "offer {at} stands: {offers:x}");
    }
    assert_eq!(byte(offers, 3), 0, "three offers at camp rank 1");

    // Hire offer 0: it joins the company where the line begins.
    let kind = byte(offers, 0) & 0x1f;
    muster_button(&mut scene, house, HIRE, 0);
    assert!(credits(&scene, player) < 1600, "the button hired");
    assert_eq!(member(&scene, 0) & 0x1f, kind, "member 0 is the offer hired");

    // Keep offer 1, then let it go.
    muster_button(&mut scene, house, KEEP, 1);
    assert_eq!(byte(var(&scene, "MfOffers"), 1) & 0x20, 0x20, "kept");
    muster_button(&mut scene, house, KEEP, 1);
    assert_eq!(byte(var(&scene, "MfOffers"), 1) & 0x20, 0, "let go");

    let money = credits(&scene, player);
    muster_button(&mut scene, house, REDRAW, 0);
    assert_eq!(credits(&scene, player), money - 50, "a redraw");
    muster_button(&mut scene, house, IMPROVE, 0);
    assert_eq!(credits(&scene, player), money - 650, "an improvement");
    assert_eq!(var(&scene, "MfRank"), 1);
    assert_eq!(byte(var(&scene, "MfOffers"), 3) & 0x40, 0x40, "four offers");

    // A dismissal pays half back and empties the place.
    let money = credits(&scene, player);
    muster_button(&mut scene, house, DISMISS, 0);
    assert!(credits(&scene, player) > money, "the dismissal refunds");
    assert_eq!(member(&scene, 0), 0, "member 0 left");

    // An empty company may fight: the battle starts and is published.
    muster_button(&mut scene, house, START, 0);
    assert_eq!(var(&scene, "MfPhase"), 2, "the battle began");
    for _ in 0..60 {
        scene.tick();
    }
    let battle = var(&scene, "MfBattle");
    assert!((battle >> 32) & 0xffff > 0, "raiders arrived: {battle:x}");
    let log = scene.runtime.simulation.ext_modules_mut().take_log();
    assert_eq!(scene.sim().ext_modules().status()[0].1, None, "{log:?}");
}

/// Spawns a Grizzly for the first computer house beside its base.
/// Returns (house index, tank, its cell).
pub(super) fn computer_tank(scene: &mut HeadlessScenario) -> (usize, u64, (u16, u16)) {
    spawn_for_computer(scene, "MTNK")
}

/// Spawns `type_name` for the first computer house beside its base.
pub(super) fn spawn_for_computer(
    scene: &mut HeadlessScenario,
    type_name: &str,
) -> (usize, u64, (u16, u16)) {
    let rules = &scene.runtime.resources.rules;
    let sim = &mut scene.runtime.simulation;
    let (index, house) = sim
        .houses
        .order()
        .iter()
        .copied()
        .enumerate()
        .find(|(_, id)| {
            let state = &sim.houses[id];
            !state.is_human() && !state.is_defeated() && state.base_center().is_some()
        })
        .expect("the battle has a computer house");
    let (bx, by) = sim.houses[&house].base_center().expect("a base centre");
    let owner = sim.interner.resolve(house).to_string();
    for n in 0..400 {
        let (Ok(rx), Ok(ry)) = (
            u16::try_from(i32::from(bx) + n % 20 - 10),
            u16::try_from(i32::from(by) + n / 20 - 10),
        ) else {
            continue;
        };
        let walkable = sim
            .path_grid()
            .is_some_and(|grid| grid.is_walkable_for_infantry(rx, ry));
        let empty = sim.occupancy().get(rx, ry).is_none_or(|cell| {
            cell.is_empty_on(crate::sim::movement::locomotor::MovementLayer::Ground)
        });
        if !walkable || !empty {
            continue;
        }
        let z = sim.terrain_cell_level(rx, ry).unwrap_or(0);
        if let Some(id) = sim.spawn_object_at_height(type_name, owner.as_str(), rx, ry, 0, z, rules)
        {
            return (index, id, (rx, ry));
        }
    }
    panic!("no free cell beside the computer base");
}

fn cell_of(scene: &HeadlessScenario, id: u64) -> (u16, u16) {
    let entity = scene.sim().entities().get(id).expect("the unit lives");
    (entity.position.rx(), entity.position.ry())
}

/// An order moves a computer house's tank in the command step after it was
/// issued; the same order for the player's house is refused.
#[test]
fn an_order_moves_a_computer_tank_and_a_players_house_is_refused() {
    let Some(mut plain) = scenario_with_computer() else {
        return;
    };
    let mut ordered = scenario_with_computer().expect("second load");
    let (house, tank, (x, y)) = computer_tank(&mut plain);
    assert_eq!(computer_tank(&mut ordered), (house, tank, (x, y)));
    let human = {
        let sim = ordered.sim();
        let order = sim.houses.order();
        order
            .iter()
            .position(|id| sim.houses[id].is_human())
            .expect("the battle has a player")
    };
    let goal = (x + 6, y);
    let target = super::cell_ref(goal.0, goal.1);
    let (computer, player) = (house_ref(house), house_ref(human));
    // Order at 64: Move (0), 1 unit, the goal cell, no value, the tank.
    let module = wat::parse_str(format!(
        r#"(module
  (import "vera:sim@1.0" "order" (func $order (param i64 i32 i32) (result i32)))
  (memory (export "memory") 1)
  (func (export "on_scenario_start")
    (i32.store (i32.const 64) (i32.const 0))
    (i32.store (i32.const 68) (i32.const 1))
    (i64.store (i32.const 72) (i64.const {target}))
    (i64.store (i32.const 88) (i64.const {tank}))
    (i32.store (i32.const 0) (call $order (i64.const {computer}) (i32.const 64) (i32.const 32)))
    (i32.store (i32.const 4) (call $order (i64.const {player}) (i32.const 64) (i32.const 32)))))"#
    ))
    .expect("test module parses");
    ordered
        .runtime
        .simulation
        .install_ext_module(&ordered.runtime.resources.rules, ModuleSource::Map, &module)
        .expect("module installs");
    let distance = |(cx, cy): (u16, u16)| goal.0.abs_diff(cx) + goal.1.abs_diff(cy);
    for _ in 0..60 {
        plain.tick();
        ordered.tick();
    }
    let memory = ordered.sim().ext_modules.slots[0].module.memory();
    let status = |at: usize| i32::from_le_bytes(memory[at..at + 4].try_into().expect("4 bytes"));
    assert_eq!(status(0), vera20k_wasm_ext::Status::Ok as i32);
    assert_eq!(status(4), vera20k_wasm_ext::Status::Refused as i32);
    assert!(ordered.sim().ext_modules.slots[0].orders.is_empty());
    assert!(
        distance(cell_of(&ordered, tank)) < distance(cell_of(&plain, tank)),
        "ordered {:?}, plain {:?}, goal {goal:?}",
        cell_of(&ordered, tank),
        cell_of(&plain, tank)
    );
}

/// The mod module's `on_rules` doubles the GI's `Strength` before the rules
/// are processed: a GI built in the match has the new health, `rules_get`
/// saw the old text, and `rules_set` during the match is refused.
#[test]
fn on_rules_changes_a_type_the_match_uses() {
    let Some((retail, _)) = crate::rules::retail_ini_fixture::retail_assets() else {
        return;
    };
    let module = wat::parse_str(
        r#"(module
  (import "vera:sim@1.0" "rules_get" (func $get (param i32 i32 i32 i32 i32 i32 i32) (result i32)))
  (import "vera:sim@1.0" "rules_set" (func $set (param i32 i32 i32 i32 i32 i32) (result i32)))
  (memory (export "memory") 1)
  (data (i32.const 0) "E1")
  (data (i32.const 8) "Strength")
  (data (i32.const 24) "250")
  (func (export "on_rules")
    (i32.store (i32.const 100) (call $get (i32.const 0) (i32.const 2) (i32.const 8) (i32.const 8)
      (i32.const 128) (i32.const 16) (i32.const 120)))
    (i32.store (i32.const 104) (call $set (i32.const 0) (i32.const 2) (i32.const 8) (i32.const 8)
      (i32.const 24) (i32.const 3))))
  (func (export "on_frame")
    (i32.store (i32.const 108) (call $set (i32.const 0) (i32.const 2) (i32.const 8) (i32.const 8)
      (i32.const 24) (i32.const 3)))))"#,
    )
    .expect("test module parses");
    let mut scene = crate::headless_scenario::load_with_mod_module(
        &retail,
        MAP,
        SEED,
        session_with_computer(),
        &module,
    )
    .expect("scenario loads with the module");
    let rules = &scene.runtime.resources.rules;
    assert_eq!(rules.object("E1").expect("E1").strength, 250);
    let (_, gi, _) = spawn_for_computer(&mut scene, "E1");
    scene.tick();
    let memory = scene.sim().ext_modules.slots[0].module.memory();
    let i32_at = |at: usize| i32::from_le_bytes(memory[at..at + 4].try_into().expect("4 bytes"));
    assert_eq!((i32_at(100), i32_at(104), i32_at(108)), (0, 0, 3));
    let old = &memory[128..128 + i32_at(120) as usize];
    assert_eq!(std::str::from_utf8(old).map(str::trim), Ok("125"));
    let gi = scene.sim().entities().get(gi).expect("the GI lives");
    assert_eq!(gi.health.current(), 250);
}

/// A mod's `[VeraWasm] MapWasm=no` refuses a map's module, and a map's
/// `VeraMinVersion` above this build refuses the map.
#[test]
fn a_mod_can_refuse_map_modules_and_a_map_can_need_a_newer_game() {
    let module = wat::parse_str("(module)").expect("test module parses");
    let mut map = format!(
        "[VeraWasm]\nApi=1.0\nRulesSize={}\n[VeraWasmRulesPack]\n",
        module.len()
    );
    for (key, value) in crate::map::ext_module_pack::pack_lines(&module) {
        map.push_str(&format!("{key}={value}\n"));
    }
    let map = IniFile::from_str(&map);
    let stock = IniFile::from_str("[General]\n");
    let refusing = IniFile::from_str("[VeraWasm]\nMapWasm=no\n");
    let allowed =
        super::PreparedExtModules::load(&stock, None, &map, 0, 0).expect("stock allows it");
    assert_eq!(allowed.modules.len(), 1);
    let refused = super::PreparedExtModules::load(&refusing, None, &map, 0, 0)
        .err()
        .expect("the mod refuses map modules");
    assert!(refused.contains("MapWasm=no"), "{refused}");
    let newer = IniFile::from_str("[Basic]\nVeraMinVersion=999.0\n");
    let refused = super::PreparedExtModules::load(&stock, None, &newer, 0, 0)
        .err()
        .expect("the map needs a newer game");
    assert!(refused.contains("999.0"), "{refused}");
}

/// `get` reads a cell's land type and height, and `find` over cells counts
/// a 2 by 2 rectangle.
#[test]
fn a_module_reads_cells() {
    let Some(mut scene) = scenario() else { return };
    let (x, y) = cell_of(
        &scene,
        scene
            .sim()
            .entities()
            .iter_sorted()
            .next()
            .expect("an object")
            .0,
    );
    let terrain = scene.sim().resolved_terrain().expect("terrain");
    let cell = terrain.cell(x, y).expect("the object's cell");
    let expected = (i64::from(cell.land_type), i64::from(cell.level));
    let rect = (0..2)
        .flat_map(|dy| (0..2).map(move |dx| (x + dx, y + dy)))
        .filter(|&(cx, cy)| terrain.cell(cx, cy).is_some())
        .count() as i32;
    let reference = super::cell_ref(x, y);
    let prop = |name| vera20k_wasm_ext::property_id(name).expect("known property");
    let (land, height) = (prop("LandType"), prop("Height"));
    // Query at 256: Cells (2), Rect (2), the cell, width 2, height 2.
    let module = wat::parse_str(format!(
        r#"(module
  (import "vera:sim@1.0" "get" (func $get (param i64 i32 i32) (result i32)))
  (import "vera:sim@1.0" "find" (func $find (param i32 i32 i32 i32 i32) (result i32)))
  (memory (export "memory") 1)
  (func (export "on_scenario_start")
    (drop (call $get (i64.const {reference}) (i32.const {land}) (i32.const 0)))
    (drop (call $get (i64.const {reference}) (i32.const {height}) (i32.const 8)))
    (i32.store (i32.const 256) (i32.const 2))
    (i32.store (i32.const 260) (i32.const 2))
    (i64.store (i32.const 264) (i64.const {reference}))
    (i32.store (i32.const 272) (i32.const 2))
    (i32.store (i32.const 276) (i32.const 2))
    (i32.store (i32.const 16) (call $find (i32.const 256) (i32.const 64) (i32.const 512) (i32.const 8) (i32.const 32)))))"#
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
    let i32_at = |at: usize| i32::from_le_bytes(memory[at..at + 4].try_into().expect("4 bytes"));
    assert_eq!((i64_at(0), i64_at(8)), expected);
    assert_eq!((i32_at(16), i32_at(32)), (0, rect));
    assert_eq!(i64_at(512), reference);
}

/// Runs the `sdk/examples/orders-rules` module when
/// `VERA20K_TEST_ORDERS_WASM` names its build.
#[test]
fn a_rust_built_orders_module_runs() {
    let Some(path) = std::env::var_os("VERA20K_TEST_ORDERS_WASM") else {
        return;
    };
    let Some((retail, _)) = crate::rules::retail_ini_fixture::retail_assets() else {
        return;
    };
    let bytes = std::fs::read(&path).expect("module file reads");
    let mut scene = crate::headless_scenario::load_with_mod_module(
        &retail,
        MAP,
        SEED,
        session_with_computer(),
        &bytes,
    )
    .expect("scenario loads with the module");
    let strength = scene
        .runtime
        .resources
        .rules
        .object("E1")
        .expect("E1")
        .strength;
    assert_eq!(strength, 250);
    for _ in 0..30 {
        scene.tick();
    }
    assert_eq!(scene.sim().ext_modules().status()[0].1, None);
    let log = scene.runtime.simulation.ext_modules_mut().take_log();
    for line in ["orders-rules: doubled E1 Strength", "orders-rules: ordered"] {
        assert!(log.iter().any(|l| l == line), "{line}: {log:?}");
    }
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

pub(super) fn prop(name: &str) -> i32 {
    vera20k_wasm_ext::property_id(name).expect("known property")
}

pub(super) fn install(scene: &mut HeadlessScenario, source: ModuleSource, module: &[u8]) {
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

pub(super) fn mem_i32(scene: &HeadlessScenario, slot: usize, at: usize) -> i32 {
    i32::from_le_bytes(memory_at(scene, slot, at))
}

pub(super) fn mem_i64(scene: &HeadlessScenario, slot: usize, at: usize) -> i64 {
    i64::from_le_bytes(memory_at(scene, slot, at))
}

/// The modules' share of the state hash: modules, modifiers and variables.
pub(super) fn ext_hash(scene: &HeadlessScenario) -> u64 {
    use std::hash::Hasher;
    let mut hasher = std::hash::DefaultHasher::new();
    scene.sim().ext_modules.hash_state(&mut hasher);
    hasher.finish()
}

/// A save of `live` loaded into a fresh scenario, as the existing save test
/// does it.
pub(super) fn saved_and_loaded(live: &HeadlessScenario) -> HeadlessScenario {
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
pub(super) fn gi_duel() -> Option<(HeadlessScenario, u64, u64)> {
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
pub(super) fn first_hit(
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

/// A module with `on_rules` runs `vera_init` once, at its rules layer, and
/// resolves its declared variable there with the ID the match uses.
#[test]
fn a_rules_stage_module_initializes_once_with_its_variables() {
    let Some((retail, _)) = crate::rules::retail_ini_fixture::retail_assets() else {
        return;
    };
    let section = vars_section(&[("Wave", 0, 0, 7)]);
    let module = wat::parse_str(format!(
        r#"(module
  {section}
  (import "vera:sim@1.0" "lookup" (func $lookup (param i32 i32 i32 i32) (result i32)))
  (import "vera:sim@1.0" "var_get" (func $var_get (param i64 i32 i32) (result i32)))
  (memory (export "memory") 1)
  (data (i32.const 400) "Wave")
  (func (export "vera_init")
    (i32.store (i32.const 16) (i32.add (i32.load (i32.const 16)) (i32.const 1)))
    (drop (call $lookup (i32.const 15) (i32.const 400) (i32.const 4) (i32.const 0))))
  (func (export "on_rules"))
  (func (export "on_scenario_start")
    (drop (call $var_get (i64.const 0) (i32.load (i32.const 0)) (i32.const 8)))))"#
    ))
    .expect("test module parses");
    let mut scene = crate::headless_scenario::load_with_mod_module(
        &retail,
        MAP,
        SEED,
        session_with_computer(),
        &module,
    )
    .expect("scenario loads with the module");
    scene.tick();
    assert_eq!(scene.sim().ext_modules().status()[0].1, None);
    assert_eq!(mem_i32(&scene, 0, 0), 1000);
    assert_eq!(mem_i64(&scene, 0, 8), 7);
    assert_eq!(mem_i32(&scene, 0, 16), 1, "vera_init ran once");
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

/// `ActionParams` with `id`, `arg`, `value` and `house` set, as WAT stores
/// at `at` (appendix "Fixed layouts").
pub(super) fn params_wat(at: u32, id: i32, arg: i32, value: i64, house: i64) -> String {
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
    for line in ["damage", "heal", "guard"] {
        let line = format!("actions-demo: {line}");
        assert!(log.contains(&line), "{line} in {log:?}");
    }
    assert!(!log.iter().any(|l| l == "actions-demo: failed"), "{log:?}");
}

/// `get` calls of `(reference, property)` pairs: status `i` at `512 + 4i`,
/// value `i` at `8i`.
fn reads_module(reads: &[(i64, &str)]) -> Vec<u8> {
    let calls: String = reads
        .iter()
        .enumerate()
        .map(|(i, (reference, name))| {
            format!(
                "(i32.store (i32.const {}) (call $get (i64.const {reference}) (i32.const {}) (i32.const {})))\n",
                512 + i * 4,
                prop(name),
                i * 8
            )
        })
        .collect();
    wat::parse_str(format!(
        r#"(module
  (import "vera:sim@1.0" "get" (func $get (param i64 i32 i32) (result i32)))
  (memory (export "memory") 1)
  (func (export "on_scenario_start")
    {calls}))"#
    ))
    .expect("test module parses")
}

/// Appendix 7.1, 7.7 and 7.8 properties read what their owners hold; a
/// viewpoint property answers `Unsupported` to a plain `get`.
#[test]
fn a_module_reads_object_type_and_global_properties_through_their_owners() {
    let Some(mut scene) = scenario() else { return };
    let (unit, _) = placed_unit(&scene);
    let type_id = {
        let rules = &scene.runtime.resources.rules;
        let sim = scene.sim();
        let object = sim
            .entity_object_type(sim.entities().get(unit).expect("unit"), rules)
            .expect("the unit's type");
        rules.type_handle(&object.id).expect("a TechnoType").0
    };
    let ty = super::type_ref(vera20k_wasm_ext::NAME_TECHNO_TYPE, type_id);
    let global = 4i64 << 48;
    let object = unit as i64;
    let reads = [
        (object, "Layer"),
        (object, "Facing"),
        (object, "Armor"),
        (object, "Transport"),
        (object, "Relation"),
        (ty, "Speed"),
        (ty, "Category"),
        (global, "Difficulty"),
    ];
    install(&mut scene, ModuleSource::Map, &reads_module(&reads));
    scene.tick();
    assert_eq!(scene.sim().ext_modules().status()[0].1, None);
    let rules = &scene.runtime.resources.rules;
    let sim = scene.sim();
    let entity = sim.entities().get(unit).expect("unit");
    let object_type = sim.entity_object_type(entity, rules).expect("type");
    let category = match object_type.category {
        crate::rules::object_type::ObjectCategory::Infantry => 0,
        _ => 1,
    };
    let expected = [
        i64::from(
            sim.entity_display_layer(unit, rules)
                .expect("layer")
                .index(),
        ),
        i64::from(entity.body_facing_byte(sim.session.binary_frame())),
        object_type.armor_id().index() as i64,
        0,
        0,
        i64::from(object_type.speed),
        category,
        i64::from(sim.trigger_difficulty()),
    ];
    let values: Vec<i64> = (0..reads.len())
        .map(|i| mem_i64(&scene, 0, i * 8))
        .collect();
    let statuses: Vec<i32> = (0..reads.len())
        .map(|i| mem_i32(&scene, 0, 512 + i * 4))
        .collect();
    assert_eq!(values, expected);
    // Relation (the fifth read) needs a viewpoint: Unsupported (4).
    assert_eq!(statuses, [0, 0, 0, 0, 4, 0, 0, 0]);
}

/// Query bytes: `scope`, area 0 or a 1x1 rectangle at `cell`, `owner`,
/// `viewpoint`, and property clauses `(property, Eq, value)`.
pub(super) fn query_wat(
    at: u32,
    scope: i32,
    cell: Option<i64>,
    owner: i64,
    viewpoint: i64,
    clauses: &[(&str, i64)],
) -> String {
    let mut wat = format!(
        "(i32.store (i32.const {at}) (i32.const {scope}))\n\
         (i64.store (i32.const {}) (i64.const {owner}))\n\
         (i64.store (i32.const {}) (i64.const {viewpoint}))\n\
         (i32.store (i32.const {}) (i32.const {}))\n",
        at + 32,
        at + 40,
        at + 56,
        clauses.len()
    );
    if let Some(cell) = cell {
        wat += &format!(
            "(i32.store (i32.const {}) (i32.const 2))\n\
             (i64.store (i32.const {}) (i64.const {cell}))\n\
             (i32.store (i32.const {}) (i32.const 1))\n\
             (i32.store (i32.const {}) (i32.const 1))\n",
            at + 4,
            at + 8,
            at + 16,
            at + 20
        );
    }
    for (i, (name, value)) in clauses.iter().enumerate() {
        let clause = at + 64 + 16 * i as u32;
        wat += &format!(
            "(i32.store (i32.const {}) (i32.const {}))\n\
             (i64.store (i32.const {}) (i64.const {value}))\n",
            clause + 4,
            prop(name),
            clause + 8
        );
    }
    wat
}

/// Viewpoint properties in `find`: `Relation` for the query's viewpoint,
/// and a cell's `Revealed` and `Passable.Foot` as the fog and the land rows
/// hold them.
#[test]
fn a_query_reads_viewpoint_and_passability_properties_for_its_viewpoint() {
    let Some(mut scene) = scenario() else { return };
    let (unit, other) = placed_unit(&scene);
    let (owner, (x, y)) = {
        let sim = scene.sim();
        let entity = sim.entities().get(unit).expect("unit");
        let owner = sim.houses.index_of(entity.owner()).expect("owner index");
        (owner, (entity.position.rx(), entity.position.ry()))
    };
    let (house, foreign) = (house_ref(owner), house_ref(other));
    let cell = super::cell_ref(x, y);
    // Five queries at 1024 + 256q, results at 64 + 16q, references at 4096.
    let queries = [
        query_wat(1024, 0, None, house, house, &[("Relation", 0)]),
        query_wat(1280, 0, None, house, 0, &[]),
        query_wat(1536, 0, None, house, foreign, &[("Relation", 0)]),
        query_wat(1792, 2, Some(cell), 0, house, &[("Revealed", 1)]),
        query_wat(2048, 2, Some(cell), 0, house, &[("Passable.Foot", 1)]),
    ];
    let clauses = [1, 0, 1, 1, 1];
    let finds: String = (0..queries.len())
        .map(|q| {
            format!(
                "(i32.store (i32.const {}) (call $find (i32.const {}) (i32.const {}) (i32.const 4096) (i32.const 64) (i32.const {})))\n",
                512 + q * 4,
                1024 + q * 256,
                64 + 16 * clauses[q],
                64 + q * 16
            )
        })
        .collect();
    let module = wat::parse_str(format!(
        r#"(module
  (import "vera:sim@1.0" "find" (func $find (param i32 i32 i32 i32 i32) (result i32)))
  (memory (export "memory") 1)
  (func (export "on_scenario_start")
    {}
    {finds}))"#,
        queries.concat()
    ))
    .expect("test module parses");
    install(&mut scene, ModuleSource::Map, &module);
    scene.tick();
    assert_eq!(scene.sim().ext_modules().status()[0].1, None);
    let statuses: Vec<i32> = (0..5).map(|q| mem_i32(&scene, 0, 512 + q * 4)).collect();
    assert_eq!(statuses, [0; 5]);
    let count = |q: usize| mem_i32(&scene, 0, 64 + q * 16);
    // Every object of the house is its own (Self), none the other house's.
    assert!(count(1) > 0);
    assert_eq!(count(0), count(1));
    assert_eq!(count(2), 0);
    let sim = scene.sim();
    let viewer = sim.houses.order()[owner];
    let revealed = sim.fog.is_cell_revealed(viewer, x, y);
    let terrain = sim.resolved_terrain().expect("terrain");
    let land = terrain.cell(x, y).expect("the unit's cell").land_type;
    let passable = terrain
        .land_speed_costs(i32::from(land))
        .cost_for_speed_type(crate::rules::locomotor_type::SpeedType::Foot)
        .is_some_and(|cost| !cost.is_zero());
    assert_eq!(count(3), i32::from(revealed));
    assert_eq!(count(4), i32::from(passable));
}

/// RFC 15.1, item 7: each frame digests a fifteenth of the pages, so a
/// write shows within 15 frames, on the frame its page comes round; new
/// pages wait for theirs with digest 0.
#[test]
fn memory_digests_visit_every_page_within_fifteen_frames() {
    use super::PageDigests;
    let page = vera20k_wasm_ext::PAGE_BYTES;
    let refresh = |digests: &mut PageDigests, memory: &[u8], frame| {
        digests.refresh(memory.len(), frame, |at, out: &mut [u8]| {
            out.copy_from_slice(&memory[at..at + out.len()]);
            true
        });
    };
    let mut memory = vec![0u8; 20 * page];
    let mut digests = PageDigests::default();
    refresh(&mut digests, &memory, 0);
    assert_eq!(digests.0.len(), 20, "the page count shows at once");
    assert_eq!(digests.0[1], 0, "a new page waits for its turn");
    assert_ne!(digests.0[15], 0, "page 15 is frame 0's");
    digests = PageDigests::of(&memory);
    let before = digests.clone();
    memory[7 * page + 3] = 1;
    for frame in 1..7 {
        refresh(&mut digests, &memory, frame);
        assert_eq!(digests, before, "frame {frame}");
    }
    refresh(&mut digests, &memory, 7);
    assert_eq!(digests, PageDigests::of(&memory));
    // Page 19 comes round on frames 4, 19, 34, ...
    memory[19 * page] = 2;
    for frame in 8..19 {
        refresh(&mut digests, &memory, frame);
        assert_ne!(digests, PageDigests::of(&memory), "frame {frame}");
    }
    refresh(&mut digests, &memory, 19);
    assert_eq!(digests, PageDigests::of(&memory));
    memory.extend(std::iter::repeat_n(5u8, page));
    refresh(&mut digests, &memory, 19);
    assert_eq!(digests.0[20], 0, "a grown page waits");
    refresh(&mut digests, &memory, 20);
    assert_eq!(digests, PageDigests::of(&memory), "its turn");
}

/// A save before any page's turn resumes with the running game's digests.
#[test]
fn a_save_before_the_first_module_phase_keeps_its_digests() {
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
    let mut resumed = saved_and_loaded(&live);
    let digests = |scene: &HeadlessScenario| {
        scene
            .sim()
            .ext_modules
            .checksum_digests()
            .collect::<Vec<_>>()
    };
    assert_eq!(digests(&resumed), digests(&live));
    live.tick();
    resumed.tick();
    assert_eq!(digests(&resumed), digests(&live));
}

/// The published XXH32 vectors: short input, then stripes, words and bytes.
#[test]
fn page_digests_are_xxh32() {
    use super::page_digest;
    assert_eq!(page_digest(b""), 0x02CC_5D05);
    assert_eq!(page_digest(b"abc"), 0x32D1_53FF);
    let long = b"Nobody inspects the spammish repetition";
    assert_eq!(page_digest(long), 0xE229_3B2F);
}

/// `Wasm.Tags` on a weapon section binds the weapon; enum parameters take
/// names; a wrong name refuses the start; a map's `Wasm.Tags` is noticed.
#[test]
fn ini_triggers_bind_weapons_and_take_enum_names() {
    use super::ini_triggers::IniTriggers;
    let Some(mut scene) = scenario() else { return };
    let ini = |action: &str| {
        IniTriggers::read(&IniFile::from_str(&format!(
            "[M60]\nWasm.Tags=ShotTag\n[ShotTag]\nTrigger=Shot\n[Shot]\nEvent=Fire\n\
             Action1={action}\nAction2=EndFor(House,Lose)\nRepeat=Repeating\n"
        )))
        .expect("INI reads")
    };
    let rules = &scene.runtime.resources.rules;
    let sim = &mut scene.runtime.simulation;
    let wrong = sim
        .install_ini_triggers(rules, &ini("SetMission(,Panic)"))
        .expect_err("an unknown mission refuses the start");
    assert!(wrong.contains("\"Panic\" is not a Mission"), "{wrong}");
    sim.install_ini_triggers(rules, &ini("SetMission(,Hunt)"))
        .expect("INI triggers build");
    let triggers = sim.trigger_runtime.module_triggers();
    assert!(triggers.has_weapon_bindings());
    let args: Vec<i32> = triggers
        .handles()
        .into_iter()
        .filter_map(|handle| triggers.get(handle))
        .filter(|trigger| trigger.ini)
        .flat_map(|trigger| trigger.actions.iter().map(|action| action.params.arg))
        .collect();
    assert_eq!(args, [15, 1], "Hunt, Lose");

    let notice = super::PreparedExtModules::load(
        &IniFile::empty(),
        None,
        &IniFile::from_str("[E1]\nWasm.Tags=MapTag\n"),
        0,
        0,
    )
    .expect("the map loads")
    .notices;
    assert_eq!(notice.len(), 1);
    assert!(notice[0].contains("[E1]"), "{notice:?}");
}

/// RFC 15.4 (0.0.21): budget defaults, the ceiling, the room's
/// `WasmBudget=` and the floor that refuses the match.
#[test]
fn wasm_budgets_default_clamp_and_refuse() {
    use super::rules::WasmPolicy;
    let policy = WasmPolicy::read(&IniFile::empty(), 0).expect("defaults read");
    assert_eq!(
        (policy.budget, policy.map_budget, policy.seat_budget),
        (
            super::FRAME_BUDGET,
            super::MAP_FRAME_BUDGET,
            super::SEAT_BUDGET
        )
    );
    let high = IniFile::from_str("[VeraWasm]\nBudget=9000000\nMapBudget=300000\n");
    let policy = WasmPolicy::read(&high, 0).expect("reads");
    assert_eq!((policy.budget, policy.map_budget), (500_000, 300_000));
    let room = WasmPolicy::read(&high, 250_000).expect("reads");
    assert_eq!(
        room.budget, 250_000,
        "the room's WasmBudget replaces Budget"
    );
    let low = IniFile::from_str("[VeraWasm]\nMapBudget=5000\n");
    let error = WasmPolicy::read(&low, 0).expect_err("below the floor");
    assert!(error.contains("MapBudget=5000"), "{error}");
    assert!(WasmPolicy::read(&IniFile::empty(), 100).is_err());
}

#[test]
fn map_ui_is_read_from_the_mods_rules_alone() {
    use super::MapUi;
    let read = |text: &str| MapUi::read(&IniFile::from_str(text));
    assert_eq!(read(""), MapUi::Overlay);
    assert_eq!(read("[VeraWasm]\nMapUI=full\n"), MapUi::Full);
    assert_eq!(read("[VeraWasm]\nMapUI=No\n"), MapUi::No);
    assert_eq!(read("[VeraWasm]\nMapUI=sideways\n"), MapUi::Overlay);
}
