//! File-backed scripts: editable `.rhai` sources under
//! `<ra2_dir>/vera20k/scripts/`.
//!
//! A script file is plain text; its name (or a `scripts.json` entry) is the
//! binding, and it is re-read from disk whenever its timestamp changes, so a
//! repeated behaviour can be tuned without rebuilding the engine.
//!
//! A script file defines up to two entry points and one optional initializer:
//!
//! ```text
//! fn default_state()                  // optional: the state map a new unit gets
//! fn on_update(ctx, state)            // every frame while the unit is alive
//! fn on_detonate(ctx, state)          // one of this unit's bullets went off
//! ```
//!
//! Each returns the updated `state` map (its last expression). `ctx` is a
//! read-only map of facts; effects are requested through the global functions
//! `laser`, `laser_rgb`, `detonate`, `damage`, `flash`, `superweapon`, `log`
//! and the geometry helpers `ring_point`, `cos_int`, `sin_int`, `cell_center`.
//!
//! Presentation and damage both go through the same `ScriptContext` command
//! channel as native scripts, so a file script cannot reach the world directly
//! and its state is still saved, hashed and replayed.

use std::cell::RefCell;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::SystemTime;

use rhai::{Array, Dynamic, Engine, Map, AST};

use crate::sim::combat::laser::{LaserBirth, LaserColor};
use crate::sim::intern::InternedId;
use crate::sim::projectile::ProjectileCoord;
use crate::util::native_x87::NativeF32Bits;

use super::{
    EventMask, Script, ScriptContext, ScriptDirectory, ScriptEvent, ScriptFileKind, ScriptId,
    ScriptState, SimCommand,
};

/// The first id handed to a file script. Native scripts own the low range, so
/// a published id stays stable when a file is added or removed.
const FIRST_FILE_SCRIPT_ID: u32 = 100;

/// 256 leptons per cell; `Position::rx/ry` are cell coordinates.
const LEPTONS_PER_CELL: i64 = 256;

/// Effects a script asked for during one call. Single-threaded by contract:
/// `dispatch_script_frame` runs on the simulation thread and calls one script
/// at a time, so a thread-local sink cannot interleave between scripts.
#[derive(Default)]
struct Sink {
    frame: i32,
    owner: InternedId,
    commands: Vec<SimCommand>,
    lasers: Vec<LaserBirth>,
}

thread_local! {
    static SINK: RefCell<Sink> = RefCell::new(Sink::default());
    /// One interpreter per thread: `Engine` is not `Sync`, and every thread
    /// that dispatches scripts (the game, a headless test) builds its own.
    static ENGINE: RefCell<Option<Engine>> = const { RefCell::new(None) };
    /// Compiled sources, keyed by path. A changed timestamp on disk replaces
    /// the entry, which is the whole hot-reload mechanism.
    static COMPILED: RefCell<HashMap<PathBuf, Compiled>> = RefCell::new(HashMap::new());
}

struct Compiled {
    modified: Option<SystemTime>,
    ast: AST,
}

/// A script that lives in a file rather than in this binary.
pub struct FileScript {
    binding: String,
    id: ScriptId,
    path: PathBuf,
    mask: EventMask,
    /// FNV-1a of the last error, so a broken script logs once instead of every
    /// frame. Advisory only; it never reaches the simulation state.
    last_error: AtomicU64,
}

impl FileScript {
    /// Check a file without registering it: the startup pass uses this so a
    /// typo is reported in the log before the first frame instead of silently
    /// doing nothing in a match.
    pub fn validate(path: &Path) -> Result<EventMask, String> {
        let source = std::fs::read_to_string(path)
            .map_err(|error| format!("{}: {error}", path.display()))?;
        ENGINE.with(|cell| {
            let mut engine = cell.borrow_mut();
            let engine = engine.get_or_insert_with(build_engine);
            let ast = engine
                .compile(&source)
                .map_err(|error| format!("{}: {error}", path.display()))?;
            let mask = mask_for(&ast);
            if mask.0 == 0 {
                return Err(format!(
                    "{}: no on_update / on_detonate function",
                    path.display()
                ));
            }
            Ok(mask)
        })
    }

    fn report(&self, message: &str) {
        #[cfg(test)]
        eprintln!("script {}: {message}", self.binding);
        let hash = fnv1a(message.as_bytes());
        if self.last_error.swap(hash, Ordering::Relaxed) != hash {
            log::warn!("script {}: {message}", self.binding);
        }
    }

    /// Compile (or reuse) this file's AST for the calling thread. Re-stats the
    /// file so an edit takes effect on the next event, without a restart.
    fn compiled(&self) -> Result<AST, String> {
        let modified = std::fs::metadata(&self.path)
            .and_then(|meta| meta.modified())
            .ok();
        COMPILED.with(|cache| {
            let mut cache = cache.borrow_mut();
            if let Some(entry) = cache.get(&self.path) {
                if entry.modified == modified {
                    return Ok(entry.ast.clone());
                }
            }
            let source = std::fs::read_to_string(&self.path)
                .map_err(|error| format!("{}: {error}", self.path.display()))?;
            let ast = ENGINE.with(|cell| {
                let mut engine = cell.borrow_mut();
                let engine = engine.get_or_insert_with(build_engine);
                engine
                    .compile(&source)
                    .map_err(|error| format!("{}: {error}", self.path.display()))
            })?;
            cache.insert(
                self.path.clone(),
                Compiled {
                    modified,
                    ast: ast.clone(),
                },
            );
            Ok(ast)
        })
    }
}

impl Script for FileScript {
    fn id(&self) -> ScriptId {
        self.id
    }

    fn binding_name(&self) -> &str {
        &self.binding
    }

    fn interests(&self) -> EventMask {
        self.mask
    }

    fn on_event(&self, event: &ScriptEvent, state: &mut ScriptState, ctx: &mut ScriptContext<'_>) {
        let entry = match event {
            ScriptEvent::Update { .. } => "on_update",
            ScriptEvent::BulletDetonated { .. } => "on_detonate",
        };

        let ast = match self.compiled() {
            Ok(ast) => ast,
            Err(message) => {
                self.report(&message);
                return;
            }
        };
        if !ast_has(&ast, entry) {
            return;
        }

        let facts = facts_map(event, ctx);
        let current = match decode_state(state) {
            Some(map) => map,
            None => default_state(&ast).unwrap_or_default(),
        };

        SINK.with(|sink| {
            let mut sink = sink.borrow_mut();
            sink.frame = ctx.frame;
            sink.owner = ctx.owner;
            sink.commands.clear();
            sink.lasers.clear();
        });

        let outcome = ENGINE.with(|cell| {
            let mut engine = cell.borrow_mut();
            let engine = engine.get_or_insert_with(build_engine);
            let mut scope = rhai::Scope::new();
            engine.call_fn::<Dynamic>(&mut scope, &ast, entry, (facts, current))
        });

        let effects = SINK.with(|sink| {
            let mut sink = sink.borrow_mut();
            (
                std::mem::take(&mut sink.commands),
                std::mem::take(&mut sink.lasers),
            )
        });

        match outcome {
            Ok(value) => {
                // A script that forgets to return its state keeps the previous
                // one instead of resetting to defaults.
                if let Some(map) = value.try_cast::<Map>() {
                    state.0 = encode_state(map);
                }
                ctx.commands.extend(effects.0);
                ctx.lasers.extend(effects.1);
            }
            Err(error) => self.report(&format!("{error}")),
        }
    }
}

/// Discover every `.rhai` file in a script directory and give each one the
/// binding name a TechnoType's `Script=` can name.
pub fn discover(dir: &ScriptDirectory) -> (Vec<FileScript>, Vec<String>) {
    let mut out = Vec::new();
    let mut diagnostics = Vec::new();
    let mut next_id = FIRST_FILE_SCRIPT_ID;

    for file in &dir.files {
        if file.kind != ScriptFileKind::Source {
            continue;
        }
        let path = dir.root.join(&file.file_name);
        let binding = dir
            .manifest
            .as_ref()
            .and_then(|manifest| {
                manifest
                    .scripts
                    .iter()
                    .find(|entry| entry.module.eq_ignore_ascii_case(&file.file_name))
                    .map(|entry| entry.name.clone())
            })
            .or_else(|| {
                Path::new(&file.file_name)
                    .file_stem()
                    .map(|stem| stem.to_string_lossy().into_owned())
            });
        let Some(binding) = binding else {
            diagnostics.push(format!("{}: no binding name", file.file_name));
            continue;
        };

        match FileScript::validate(&path) {
            Ok(mask) => {
                out.push(FileScript {
                    binding,
                    id: ScriptId(next_id),
                    path,
                    mask,
                    last_error: AtomicU64::new(0),
                });
                next_id += 1;
            }
            Err(message) => diagnostics.push(message),
        }
    }

    (out, diagnostics)
}

fn ast_has(ast: &AST, name: &str) -> bool {
    ast.iter_functions().any(|function| function.name == name)
}

fn mask_for(ast: &AST) -> EventMask {
    let mut bits = 0;
    if ast_has(ast, "on_update") {
        bits |= EventMask::UPDATE;
    }
    if ast_has(ast, "on_detonate") {
        bits |= EventMask::BULLET_DETONATED;
    }
    EventMask::of(bits)
}

fn default_state(ast: &AST) -> Option<Map> {
    if !ast_has(ast, "default_state") {
        return None;
    }
    ENGINE.with(|cell| {
        let mut engine = cell.borrow_mut();
        let engine = engine.get_or_insert_with(build_engine);
        let mut scope = rhai::Scope::new();
        engine
            .call_fn::<Map>(&mut scope, ast, "default_state", ())
            .ok()
    })
}

fn decode_state(state: &ScriptState) -> Option<Map> {
    if state.0.is_empty() {
        return None;
    }
    let value: serde_json::Value = serde_json::from_slice(&state.0).ok()?;
    rhai::serde::to_dynamic(value)
        .ok()?
        .try_cast::<Map>()
}

fn encode_state(map: Map) -> Vec<u8> {
    let Ok(value) = rhai::serde::from_dynamic::<serde_json::Value>(&Dynamic::from(map)) else {
        return Vec::new();
    };
    serde_json::to_vec(&value).unwrap_or_default()
}

/// Everything a script may read about this event. Deliberately flat and
/// JSON-shaped: the same map can cross a WASM boundary later.
fn facts_map(event: &ScriptEvent, ctx: &ScriptContext<'_>) -> Map {
    let mut facts = Map::new();
    facts.insert(
        "event".into(),
        Dynamic::from(match event {
            ScriptEvent::Update { .. } => "update",
            ScriptEvent::BulletDetonated { .. } => "detonate",
        }),
    );
    facts.insert("frame".into(), Dynamic::from(i64::from(ctx.frame)));
    facts.insert("has_target".into(), Dynamic::from(ctx.has_target));
    facts.insert("air".into(), Dynamic::from(ctx.is_air_target));
    facts.insert(
        "height".into(),
        Dynamic::from(i64::from(ctx.target_height)),
    );
    facts.insert("anchor".into(), Dynamic::from(coord_array(ctx.anchor)));
    facts.insert(
        "target".into(),
        Dynamic::from(ctx.target_entity.map(|id| id as i64).unwrap_or(0)),
    );
    facts.insert(
        "projectile".into(),
        Dynamic::from(ctx.projectile.unwrap_or("").to_owned()),
    );
    let impact = match event {
        ScriptEvent::BulletDetonated { impact, .. } => *impact,
        _ => (0, 0, 0),
    };
    facts.insert("impact".into(), Dynamic::from(coord_array(impact)));
    facts
}

fn coord_array(coord: (i32, i32, i32)) -> Array {
    vec![
        Dynamic::from(i64::from(coord.0)),
        Dynamic::from(i64::from(coord.1)),
        Dynamic::from(i64::from(coord.2)),
    ]
}

fn read_coord(value: &Array) -> Option<(i32, i32, i32)> {
    if value.len() != 3 {
        return None;
    }
    let mut out = [0i32; 3];
    for (slot, item) in out.iter_mut().zip(value.iter()) {
        *slot = item.as_int().ok()? as i32;
    }
    Some((out[0], out[1], out[2]))
}

fn fnv1a(bytes: &[u8]) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100_0000_01b3);
    }
    hash
}

/// The interpreter and its host API. Registered once per thread; every script
/// on that thread shares it.
fn build_engine() -> Engine {
    let mut engine = Engine::new();
    // A runaway loop in a script must not freeze the simulation tick. These
    // limits are per call, and a show needs a few hundred operations at most.
    engine.set_max_operations(200_000);
    engine.set_max_call_levels(16);
    engine.set_max_expr_depths(64, 32);
    engine.on_print(|text| log::info!("script: {text}"));

    engine.register_fn("log", |message: &str| {
        log::info!("script: {message}");
    });

    // `laser(from, to, width, duration)`: house-colour beam, the only arm that
    // widens with `width`.
    engine.register_fn(
        "laser",
        |from: Array, to: Array, width: i64, duration: i64| {
            let (Some(from), Some(to)) = (read_coord(&from), read_coord(&to)) else {
                log::warn!("script laser(): from/to must be [x, y, z]");
                return;
            };
            push_laser(from, to, width, duration, None);
        },
    );
    // `laser_rgb(from, to, width, duration, [r, g, b])`: explicit colour, which
    // takes the general draw arm and therefore ignores `width`.
    engine.register_fn(
        "laser_rgb",
        |from: Array, to: Array, width: i64, duration: i64, rgb: Array| {
            let (Some(from), Some(to)) = (read_coord(&from), read_coord(&to)) else {
                log::warn!("script laser_rgb(): from/to must be [x, y, z]");
                return;
            };
            if rgb.len() != 3 {
                log::warn!("script laser_rgb(): colour must be [r, g, b]");
                return;
            }
            let mut color = [0u8; 3];
            for (slot, item) in color.iter_mut().zip(rgb.iter()) {
                *slot = item.as_int().unwrap_or(0).clamp(0, 255) as u8;
            }
            push_laser(from, to, width, duration, Some(color));
        },
    );

    engine.register_fn("detonate", |warhead: &str, damage: i64, coord: Array| {
        let Some(coord) = read_coord(&coord) else {
            log::warn!("script detonate(): coordinate must be [x, y, z]");
            return;
        };
        push_command(SimCommand::Detonate {
            warhead: warhead.to_owned(),
            damage: damage.clamp(0, i32::MAX as i64) as i32,
            coord,
        });
    });
    engine.register_fn("damage", |warhead: &str, damage: i64, coord: Array| {
        let Some(coord) = read_coord(&coord) else {
            log::warn!("script damage(): coordinate must be [x, y, z]");
            return;
        };
        push_command(SimCommand::ApplyWarhead {
            warhead: warhead.to_owned(),
            damage: damage.clamp(0, i32::MAX as i64) as i32,
            coord,
        });
    });
    engine.register_fn("flash", |super_weapon: &str, frames: i64| {
        push_command(SimCommand::NukeFlash {
            super_weapon: super_weapon.to_owned(),
            frames: frames.clamp(0, i32::MAX as i64) as i32,
        });
    });
    engine.register_fn(
        "superweapon",
        |super_weapon: &str, cell_x: i64, cell_y: i64| {
            push_command(SimCommand::FireSuperWeapon {
                super_weapon: super_weapon.to_owned(),
                cell: (
                    cell_x.clamp(0, u16::MAX as i64) as u16,
                    cell_y.clamp(0, u16::MAX as i64) as u16,
                ),
            });
        },
    );

    // Integer geometry. `libm` keeps the results identical on every target,
    // which the rest of the simulation already relies on.
    engine.register_fn("cos_int", |radius: i64, degrees: i64| -> i64 {
        (radius as f64 * libm::cos((degrees as f64).to_radians())) as i64
    });
    engine.register_fn("sin_int", |radius: i64, degrees: i64| -> i64 {
        (radius as f64 * libm::sin((degrees as f64).to_radians())) as i64
    });
    // `ring_point(center, radius, degrees)`: the world point on a circle around
    // `center` at the same Z, truncated the way `(int)(radius * cos)` does.
    engine.register_fn("ring_point", |center: Array, radius: i64, degrees: i64| -> Array {
        let Some((x, y, z)) = read_coord(&center) else {
            return Vec::new();
        };
        let radians = (degrees as f64).to_radians();
        let dx = (radius as f64 * libm::cos(radians)) as i64;
        let dy = (radius as f64 * libm::sin(radians)) as i64;
        vec![
            Dynamic::from(i64::from(x) + dx),
            Dynamic::from(i64::from(y) + dy),
            Dynamic::from(i64::from(z)),
        ]
    });
    // `cell_center(x)`: `Cell2Coord(Coord2Cell(x))` for one axis, which is how
    // the original snapped the beams and the main wave onto one cell.
    engine.register_fn("cell_center", |leptons: i64| -> i64 {
        leptons.div_euclid(LEPTONS_PER_CELL) * LEPTONS_PER_CELL + LEPTONS_PER_CELL / 2
    });

    engine
}

fn push_laser(
    from: (i32, i32, i32),
    to: (i32, i32, i32),
    width: i64,
    duration: i64,
    rgb: Option<[u8; 3]>,
) {
    SINK.with(|sink| {
        let mut sink = sink.borrow_mut();
        let frame = sink.frame;
        let owner = sink.owner;
        sink.lasers.push(LaserBirth {
            frame,
            from: ProjectileCoord::new(from.0, from.1, from.2),
            to: ProjectileCoord::new(to.0, to.1, to.2),
            z_adjust: 0,
            duration: duration.clamp(0, i32::MAX as i64) as i32,
            width: width.clamp(0, i32::MAX as i64) as i32,
            supported: false,
            color: match rgb {
                Some(inner) => LaserColor::Explicit {
                    inner,
                    outer: inner,
                    spread: [0, 0, 0],
                },
                None => LaserColor::House(owner),
            },
            fade_start: NativeF32Bits::ONE,
            fade_end: NativeF32Bits::POSITIVE_ZERO,
        });
    });
}

fn push_command(command: SimCommand) {
    SINK.with(|sink| sink.borrow_mut().commands.push(command));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rules::ruleset::RuleSet;

    fn temp_dir(name: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!(
            "vera20k_file_script_{}_{}",
            std::process::id(),
            name
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).expect("temp script dir");
        root
    }

    fn script_at(root: &Path, file: &str, source: &str) -> FileScript {
        std::fs::write(root.join(file), source).expect("write script");
        let dir = ScriptDirectory::discover_at(root);
        let (scripts, diagnostics) = discover(&dir);
        assert!(diagnostics.is_empty(), "{diagnostics:?}");
        scripts.into_iter().next().expect("one script")
    }

    #[test]
    fn a_file_script_runs_and_returns_its_state() {
        let root = temp_dir("state");
        let script = script_at(
            &root,
            "Counter.rhai",
            r#"
fn default_state() {
    #{ ticks: 0 }
}

fn on_update(ctx, state) {
    state.ticks += 1;
    state
}
"#,
        );
        assert_eq!(script.binding_name(), "Counter");
        assert!(script.interests().contains(EventMask::UPDATE));

        let rules = RuleSet::empty_for_test();
        let mut state = ScriptState::default();
        for frame in 0..3 {
            let mut ctx = ScriptContext::new(&rules, frame);
            script.on_event(
                &ScriptEvent::Update { entity: 1, frame },
                &mut state,
                &mut ctx,
            );
        }
        let decoded = decode_state(&state).expect("state map");
        assert_eq!(decoded.get("ticks").unwrap().as_int().unwrap(), 3);

        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn host_calls_become_script_commands_and_lasers() {
        let root = temp_dir("effects");
        let script = script_at(
            &root,
            "Show.rhai",
            r#"
fn on_update(ctx, state) {
    laser(ctx.anchor, [0, 0, 0], 40, 30);
    laser_rgb(ctx.anchor, [1, 2, 3], 4, 2, [0, 35, 180]);
    detonate("TestImpactWH", 20, ctx.anchor);
    flash("TestLightSW", 100);
    state
}
"#,
        );

        let rules = RuleSet::empty_for_test();
        let mut state = ScriptState::default();
        let mut ctx = ScriptContext::new(&rules, 7);
        ctx.anchor = (512, 768, 0);
        script.on_event(&ScriptEvent::Update { entity: 1, frame: 7 }, &mut state, &mut ctx);

        assert_eq!(ctx.lasers.len(), 2);
        assert_eq!(ctx.lasers[0].width, 40);
        assert_eq!(ctx.lasers[0].duration, 30);
        assert!(matches!(ctx.lasers[0].color, LaserColor::House(_)));
        assert!(matches!(ctx.lasers[1].color, LaserColor::Explicit { .. }));
        assert_eq!(ctx.commands.len(), 2);
        assert!(matches!(ctx.commands[0], SimCommand::Detonate { .. }));
        assert!(matches!(ctx.commands[1], SimCommand::NukeFlash { .. }));

        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn an_edited_file_is_picked_up_without_a_restart() {
        let root = temp_dir("reload");
        let script = script_at(
            &root,
            "Tunable.rhai",
            r#"
fn on_update(ctx, state) {
    laser(ctx.anchor, [0, 0, 0], 4, 2);
    state
}
"#,
        );

        let rules = RuleSet::empty_for_test();
        let mut state = ScriptState::default();
        let mut ctx = ScriptContext::new(&rules, 0);
        script.on_event(&ScriptEvent::Update { entity: 1, frame: 0 }, &mut state, &mut ctx);
        assert_eq!(ctx.lasers[0].width, 4);

        // Touch the file with a later timestamp, as an editor's save does.
        std::thread::sleep(std::time::Duration::from_millis(20));
        std::fs::write(
            root.join("Tunable.rhai"),
            r#"
fn on_update(ctx, state) {
    laser(ctx.anchor, [0, 0, 0], 150, 30);
    state
}
"#,
        )
        .unwrap();

        let mut ctx = ScriptContext::new(&rules, 1);
        script.on_event(&ScriptEvent::Update { entity: 1, frame: 1 }, &mut state, &mut ctx);
        assert_eq!(ctx.lasers[0].width, 150);
        assert_eq!(ctx.lasers[0].duration, 30);

        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn a_script_without_entry_points_is_reported() {
        let root = temp_dir("noentry");
        std::fs::write(root.join("Broken.rhai"), "fn helper() { 1 }").unwrap();
        let dir = ScriptDirectory::discover_at(&root);
        let (scripts, diagnostics) = discover(&dir);
        assert!(scripts.is_empty());
        assert!(diagnostics[0].contains("no on_update"), "{diagnostics:?}");

        std::fs::remove_dir_all(&root).ok();
    }

    /// Rhai functions do not see the file's top-level scope, so tuning lives in
    /// a `cfg()` function every act calls. This pins that shape down: a script
    /// written with `const` at file scope would fail at runtime, not at compile
    /// time, and the game would silently play nothing.
    #[test]
    fn a_config_function_is_reachable_from_every_entry_point() {
        let root = temp_dir("config");
        let script = script_at(
            &root,
            "Config.rhai",
            r#"
fn cfg() {
    #{ width: 40, offsets: [-20, 0, 20], warhead: "TestWH" }
}

fn default_state() {
    let cfg = cfg();
    #{ width: cfg.width, frames: 0 }
}

fn on_update(ctx, state) {
    let cfg = cfg();
    for offset in cfg.offsets {
        laser(ctx.anchor, [0, 0, 0], cfg.width + offset, 2);
    }
    detonate(cfg.warhead, 10, ctx.anchor);
    state.frames += 1;
    state
}
"#,
        );

        let rules = RuleSet::empty_for_test();
        let mut state = ScriptState::default();
        let mut ctx = ScriptContext::new(&rules, 0);
        ctx.anchor = (256, 512, 0);
        script.on_event(
            &ScriptEvent::Update { entity: 1, frame: 0 },
            &mut state,
            &mut ctx,
        );
        assert_eq!(ctx.lasers.len(), 3, "a script error would emit nothing");
        assert_eq!(ctx.lasers[0].width, 20);
        assert_eq!(ctx.lasers[1].width, 40);
        assert_eq!(ctx.commands.len(), 1);
        assert_eq!(
            decode_state(&state).unwrap().get("frames").unwrap().as_int(),
            Ok(1)
        );

        std::fs::remove_dir_all(&root).ok();
    }

    /// The shipped example must compile and behave under the interpreter: the
    /// engine cannot type-check a script the way it checks Rust, so this test
    /// is what keeps `docs/scripts/example_ring.rhai` honest.
    #[test]
    fn the_shipped_example_script_runs() {
        let root = temp_dir("example");
        std::fs::write(
            root.join("example_ring.rhai"),
            include_str!("../../../docs/scripts/example_ring.rhai"),
        )
        .unwrap();
        let dir = ScriptDirectory::discover_at(&root);
        let (scripts, diagnostics) = discover(&dir);
        assert!(diagnostics.is_empty(), "{diagnostics:?}");
        let script = scripts.into_iter().next().expect("one script");
        assert_eq!(script.binding_name(), "example_ring");

        let rules = RuleSet::empty_for_test();
        let mut state = ScriptState::default();
        let target = (1200i32, 2400i32, 0i32);

        // No target: a script must never aim at its own caster.
        let mut ctx = ScriptContext::new(&rules, 0);
        script.on_event(
            &ScriptEvent::Update { entity: 1, frame: 0 },
            &mut state,
            &mut ctx,
        );
        assert!(ctx.lasers.is_empty() && ctx.commands.is_empty());

        // A held target draws the ring; the centre detonates every 30 frames.
        let mut saw_lasers = false;
        let mut saw_detonation = false;
        for frame in 1..31 {
            let mut ctx = ScriptContext::new(&rules, frame);
            ctx.has_target = true;
            ctx.anchor = target;
            script.on_event(
                &ScriptEvent::Update { entity: 1, frame },
                &mut state,
                &mut ctx,
            );
            saw_lasers |= ctx.lasers.len() == 8;
            for command in &ctx.commands {
                if let SimCommand::Detonate { coord, .. } = command {
                    assert!(
                        (coord.0 - target.0).abs() <= 256 && (coord.1 - target.1).abs() <= 256,
                        "detonation at {coord:?} is not on the target"
                    );
                    saw_detonation = true;
                }
            }
        }
        assert!(saw_lasers, "the ring must emit eight beams per frame");
        assert!(saw_detonation, "the centre must detonate");

        // Its own shot going off requests the map lighting.
        let mut ctx = ScriptContext::new(&rules, 31);
        script.on_event(
            &ScriptEvent::BulletDetonated {
                owner: 1,
                projectile: crate::sim::intern::InternedId::default(),
                impact: target,
            },
            &mut state,
            &mut ctx,
        );
        assert!(matches!(ctx.commands[0], SimCommand::NukeFlash { .. }));

        std::fs::remove_dir_all(&root).ok();
    }
}
