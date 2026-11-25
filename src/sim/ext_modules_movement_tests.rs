//! Module movement on a retail map (`ext_modules::motion`, `paths`): a
//! `MoveFrame` step moves a Grizzly within its cell and into the next one,
//! a held step keeps it from its move order, a `Push` spreads its distance
//! over its frames (saved and hashed while it runs), `SetFacing` turns it,
//! a `PathFound` refusal keeps it from leaving its cell, and a
//! `ROTMultiplier` scales its body rate, and an `ApproachPoint` answer
//! sends it to another cell than its approach search chose; plus what
//! `has_kind` lists and the push arithmetic.

use vera20k_wasm_ext::Status;
use vera20k_wasm_ext::decide::{
    EVENT_APPROACH_POINT, EVENT_MOVE_FRAME, EVENT_PATH_FOUND, MoveStep,
};

use super::ModuleSource;
use super::motion::tests_support;
use crate::headless_scenario::HeadlessScenario;
use crate::sim::components::DriveCoord;

/// Handler keys: a `MoveStep` of 16 leptons east; one of a cell east; a
/// held step (move, no displacement); no route (result 0, also what the
/// other keys answer as `result`).
const STEP: i32 = 1;
const STEP_CELL: i32 = 2;
const HOLD: i32 = 3;
const NO_PATH: i32 = 4;

/// A `TriggerDef`: house 0, repeating, every difficulty, decision handler
/// `key`, one event of `kind`, no actions.
fn decide_def(kind: i32, key: i32) -> Vec<u8> {
    let mut out = 0i64.to_le_bytes().to_vec();
    for value in [2, 7, 0, key, 1, 0, kind, 0, 0, -1] {
        out.extend(value.to_le_bytes());
    }
    out.extend(0i64.to_le_bytes());
    out.extend([0; 8]);
    out
}

/// Creates the trigger in `on_scenario_start` and binds it to `target`
/// (statuses at 72 and 76). `on_decide` copies its record to 512, counts
/// requests at 696 and answers by key, with `result` `answer`. Its delivery
/// area is 1024 bytes at 1024.
fn module(kind: i32, key: i32, target: i64, answer: i64) -> Vec<u8> {
    let def = decide_def(kind, key);
    let bytes = def.iter().map(|b| format!("\\{b:02x}")).collect::<String>();
    wat::parse_str(format!(
        r#"(module
  (import "vera:sim@1.0" "trigger_create" (func $create (param i32 i32 i32) (result i32)))
  (import "vera:sim@1.0" "trigger_bind" (func $bind (param i64 i64) (result i32)))
  (@custom "vera.delivery" "\01\00\04\00\00\00\04\00\00")
  (memory (export "memory") 1)
  (data (i32.const 3072) "{bytes}")
  (func (export "on_scenario_start")
    (i32.store (i32.const 76) (call $create (i32.const 3072) (i32.const {len}) (i32.const 64)))
    (i32.store (i32.const 72) (call $bind (i64.load (i32.const 64)) (i64.const {target}))))
  (func $step (param $area i32) (param $dx i32)
    (i32.store (local.get $area) (i32.const {MOVE}))
    (i32.store offset=8 (local.get $area) (local.get $dx))
    (i32.store offset=12 (local.get $area) (i32.const 0)))
  (func (export "on_decide") (param $ptr i32) (param $count i32)
    (local $key i32) (local $area i32)
    (memory.copy (i32.const 512) (local.get $ptr) (i32.const 160))
    (i32.store (i32.const 696) (i32.add (i32.load (i32.const 696)) (i32.const 1)))
    (local.set $key (i32.load (local.get $ptr)))
    (local.set $area (i32.load offset=148 (local.get $ptr)))
    (if (i32.eq (local.get $key) (i32.const {STEP}))
      (then (call $step (local.get $area) (i32.const 16))))
    (if (i32.eq (local.get $key) (i32.const {STEP_CELL}))
      (then (call $step (local.get $area) (i32.const 256))))
    (if (i32.eq (local.get $key) (i32.const {HOLD}))
      (then (call $step (local.get $area) (i32.const 0))))
    (i64.store offset=136 (local.get $ptr) (i64.const {answer}))
    (i32.store offset=144 (local.get $ptr) (i32.const 1))))"#,
        len = def.len(),
        MOVE = MoveStep::MOVE,
    ))
    .expect("test module parses")
}

fn install(scene: &mut HeadlessScenario, kind: i32, key: i32, target: u64) {
    install_answering(scene, kind, key, target, 0);
}

/// [`install`] with a module whose answers' `result` is `answer`.
fn install_answering(scene: &mut HeadlessScenario, kind: i32, key: i32, target: u64, answer: i64) {
    scene
        .runtime
        .simulation
        .install_ext_module(
            &scene.runtime.resources.rules,
            ModuleSource::Map,
            &module(kind, key, target as i64, answer),
        )
        .expect("module installs");
    scene.tick();
    assert_eq!(&memory(scene)[72..80], &[0; 8], "create and bind answer Ok");
}

fn memory(scene: &HeadlessScenario) -> Vec<u8> {
    scene.sim().ext_modules.slots[0].module.memory().to_vec()
}

fn i32_at(bytes: &[u8], at: usize) -> i32 {
    i32::from_le_bytes(bytes[at..at + 4].try_into().expect("4 bytes"))
}

fn i64_at(bytes: &[u8], at: usize) -> i64 {
    i64::from_le_bytes(bytes[at..at + 8].try_into().expect("8 bytes"))
}

fn requests(scene: &HeadlessScenario) -> i32 {
    i32_at(&memory(scene), 696)
}

/// An American Grizzly on open flat ground, the four cells east of it and
/// the rows beside them walkable, nothing else within 14 cells; a power
/// plant each for two houses far away: (scenario, tank, its cell).
fn tank_scene() -> Option<(HeadlessScenario, u64, (u16, u16))> {
    use crate::sim::house_state::HouseState;
    let mut scene = super::tests::scenario()?;
    let crate::sim::runtime::SimRuntime {
        simulation: sim,
        resources,
    } = &mut scene.runtime;
    let rules = &resources.rules;
    for (name, side) in [("Americans", 0), ("Russians", 1)] {
        let house = sim.interner.intern(name);
        // Named, so that a save of the scene restores.
        sim.houses.get_or_register_for_test(house, || {
            let mut state = HouseState::new(house, side, None, true, 10_000, 10);
            state.initialize_scenario_name(name);
            state
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
            (x - 1..=x + 4).all(|cx| {
                (y - 1..=y + 1).all(|cy| {
                    terrain.cell(cx, cy).is_some_and(|c| c.level == level)
                        && grid.cell(cx, cy).is_some_and(|c| c.ground_walkable)
                })
            }) && occupied
                .iter()
                .all(|&(ox, oy)| ox.abs_diff(x).max(oy.abs_diff(y)) >= 14)
        })?;
    for (plant, owner) in [("GAPOWR", "Americans"), ("NAPOWR", "Russians")] {
        (20..120_u16)
            .flat_map(|py| (20..120_u16).map(move |px| (px, py)))
            .filter(|&(px, py)| px.abs_diff(x).max(py.abs_diff(y)) >= 16)
            .find_map(|(px, py)| sim.spawn_object(plant, owner, px, py, 0, rules))?;
    }
    let tank = sim.spawn_object("MTNK", "Americans", x, y, 0, rules)?;
    sim.resolve_type_handles(rules);
    Some((scene, tank, (x, y)))
}

fn at(scene: &HeadlessScenario, id: u64) -> DriveCoord {
    scene
        .sim()
        .foot_current_coordinate(id)
        .expect("the tank is placed")
}

fn cell_of(coord: DriveCoord) -> (i32, i32) {
    (coord.x.div_euclid(256), coord.y.div_euclid(256))
}

/// Runs `frames` frames, the first with a Move order of `tank` to `goal`.
fn order_move(scene: &mut HeadlessScenario, tank: u64, goal: (u16, u16), frames: usize) {
    use crate::sim::command::{Command, CommandEnvelope};
    let order = CommandEnvelope::new(
        scene.sim().interner.get("Americans").expect("Americans house"),
        scene.sim().session.tick() + 1,
        Command::Move {
            entity_id: tank,
            target_rx: goal.0,
            target_ry: goal.1,
        },
    );
    let mut orders = vec![order];
    for _ in 0..frames {
        scene
            .runtime
            .advance_frame(
                &std::mem::take(&mut orders),
                crate::headless_scenario::SIM_TICK_MS,
                crate::sim::world::TickLane::Ordinary,
                &mut crate::sim::world::FrameEffects::default(),
            )
            .expect("frame advances");
    }
}

/// `has_kind` lists the movement decisions and both actions.
#[test]
fn has_kind_lists_the_movement_kinds() {
    let Some(scene) = super::tests::scenario() else {
        return;
    };
    let rules = &scene.runtime.resources.rules;
    assert!(super::has_kind(rules, 0, EVENT_MOVE_FRAME));
    assert!(super::has_kind(rules, 0, EVENT_PATH_FOUND));
    assert!(super::has_kind(rules, 0, EVENT_APPROACH_POINT));
    assert!(super::has_kind(rules, 1, super::actions::PUSH));
    assert!(super::has_kind(rules, 1, super::actions::SET_FACING));
    assert_eq!(
        vera20k_wasm_ext::action_kind_id("SetFacing"),
        Some(super::actions::SET_FACING)
    );
    assert_eq!(
        vera20k_wasm_ext::action_kind_id("Push"),
        Some(super::actions::PUSH)
    );
}

/// A `MoveFrame` step of 16 leptons east moves the resting tank by that
/// much in one frame; the request carried the tank, `value` 0 (not moving)
/// and a 16-byte area holding the zero step.
#[test]
fn a_move_frame_step_moves_the_tank() {
    let Some((mut scene, tank, _)) = tank_scene() else {
        return;
    };
    install(&mut scene, EVENT_MOVE_FRAME, STEP, tank);
    let before = at(&scene, tank);
    let asked = requests(&scene);
    scene.tick();
    let after = at(&scene, tank);
    assert_eq!((after.x, after.y), (before.x + 16, before.y));
    assert_eq!(requests(&scene), asked + 1);
    let record = &memory(&scene)[512..672];
    assert_eq!(i32_at(record, 4), EVENT_MOVE_FRAME);
    assert_eq!(i64_at(record, 24), tank as i64, "subject");
    assert_eq!(i64_at(record, 72), 0, "not moving");
    assert_eq!((i32_at(record, 152), i32_at(record, 156)), (16, 16));
    assert_eq!(scene.sim().ext_modules().status()[0].1, None);
}

/// A step of a cell east enters the free cell east of the tank: its mark
/// moves with it.
#[test]
fn a_move_frame_step_enters_the_next_cell() {
    let Some((mut scene, tank, _)) = tank_scene() else {
        return;
    };
    install(&mut scene, EVENT_MOVE_FRAME, STEP_CELL, tank);
    let before = at(&scene, tank);
    scene.tick();
    let after = at(&scene, tank);
    assert_eq!((after.x, after.y), (before.x + 256, before.y));
    let (cx, cy) = cell_of(after);
    let entity = scene.sim().entities().get(tank).expect("tank");
    assert_eq!(
        (i32::from(entity.position.rx()), i32::from(entity.position.ry())),
        (cx, cy)
    );
    assert!(entity.lifecycle.cell_marked());
}

/// A held step (move, no displacement) keeps the tank where it stands
/// through a move order; the requests that came later say it is moving.
#[test]
fn a_held_step_keeps_the_tank_from_its_order() {
    let Some((mut scene, tank, (x, y))) = tank_scene() else {
        return;
    };
    install(&mut scene, EVENT_MOVE_FRAME, HOLD, tank);
    let before = at(&scene, tank);
    order_move(&mut scene, tank, (x + 3, y), 30);
    let after = at(&scene, tank);
    assert_eq!((after.x, after.y), (before.x, before.y));
    assert!(requests(&scene) >= 30);
    assert_eq!(scene.sim().ext_modules().status()[0].1, None);
}

/// A `PathFound` decider answering no route keeps a tank ordered three
/// cells east in its cell; the request named a goal cell.
#[test]
fn a_path_found_refusal_keeps_the_tank_in_its_cell() {
    let Some((mut scene, tank, (x, y))) = tank_scene() else {
        return;
    };
    install(&mut scene, EVENT_PATH_FOUND, NO_PATH, tank);
    let before = cell_of(at(&scene, tank));
    order_move(&mut scene, tank, (x + 3, y), 40);
    assert_eq!(cell_of(at(&scene, tank)), before);
    assert!(requests(&scene) >= 1, "asked at the search");
    let record = &memory(&scene)[512..672];
    assert_eq!(i32_at(record, 4), EVENT_PATH_FOUND);
    assert_eq!(i64_at(record, 24), tank as i64);
    assert_ne!(i64_at(record, 64), 0, "a goal cell");
}

/// A `Push` of 32 leptons toward a cell three east over two frames moves
/// the tank by its whole displacement in its next two movement frames, then
/// ends. While it
/// runs it is in the state hash and survives a save.
#[test]
fn a_push_moves_the_tank_over_its_frames() {
    let Some((mut scene, tank, (x, y))) = tank_scene() else {
        return;
    };
    let before = at(&scene, tank);
    let hash = super::tests::ext_hash(&scene);
    let goal = (x as i16 + 3, y as i16);
    assert_eq!(
        scene
            .runtime
            .simulation
            .start_module_push(tank, goal, 32, 2),
        Status::Ok
    );
    assert_ne!(super::tests::ext_hash(&scene), hash, "a running push is hashed");
    let loaded = super::tests::saved_and_loaded(&scene);
    assert_eq!(
        loaded.sim().ext_modules.motions,
        scene.sim().ext_modules.motions
    );
    let dx = i64::from(goal.0) * 256 + 128 - i64::from(before.x);
    let dy = i64::from(goal.1) * 256 + 128 - i64::from(before.y);
    let length = ((dx * dx + dy * dy) as f64).sqrt() as i64;
    let total = [dx * 32 / length, dy * 32 / length];
    // The push takes the tank's next two movement frames; the first tick
    // after it starts may come before the tank's own turn.
    for _ in 0..3 {
        scene.tick();
    }
    let after = at(&scene, tank);
    assert_eq!(
        [i64::from(after.x - before.x), i64::from(after.y - before.y)],
        total
    );
    assert!(scene.sim().ext_modules.motions.is_empty());
    assert_eq!(super::tests::ext_hash(&scene), hash);
}

/// `SetFacing` turns the tank's body toward the facing through its Drive
/// locomotor (Set, at its ROT).
#[test]
fn set_facing_turns_the_tank() {
    let Some((mut scene, tank, _)) = tank_scene() else {
        return;
    };
    assert_eq!(
        scene.runtime.simulation.module_set_facing(tank, 64),
        Status::Ok
    );
    let entity = scene.sim().entities().get(tank).expect("tank");
    assert_eq!(entity.body_facing.destination() >> 8, 64);
}

#[test]
fn push_arithmetic() {
    tests_support::check_isqrt();
    let steps = tests_support::push_steps([100, -7], 3);
    assert_eq!(steps, [[33, -2], [33, -2], [34, -3]]);
    let steps = tests_support::push_steps([5, 0], 8);
    assert_eq!(steps.len(), 8);
    assert_eq!(steps.iter().map(|s| s[0]).sum::<i32>(), 5);
}

/// Puts a `ROTMultiplier` multiply of `factor` (raw `fixed`) on `target`:
/// its handle.
fn rot_modifier(scene: &mut HeadlessScenario, target: u64, factor: i64) -> u64 {
    use super::modifiers::{Modifier, ModifierOp, Stat};
    let sim = &mut scene.runtime.simulation;
    let handle = sim.substrate.allocate_stable_id();
    let modifier = Modifier {
        target,
        stat: Stat::Rot,
        op: ModifierOp::Multiply,
        value: factor,
        expires: None,
        source: 0,
        group: 0,
        origin: ModuleSource::Map,
    };
    sim.ext_modules
        .modifiers
        .insert(&mut sim.substrate.entities, handle, modifier)
        .expect("the modifier fits");
    handle
}

fn body_rate(scene: &HeadlessScenario, tank: u64) -> u16 {
    scene
        .sim()
        .entities()
        .get(tank)
        .expect("tank")
        .body_facing
        .rot_per_frame()
}

/// A `ROTMultiplier` of 0.5 halves the Grizzly's body rate word, a second
/// one of 4.0 makes it double the native word, a save keeps the scaled word,
/// and removing both brings back the native word bit for bit.
#[test]
fn a_rot_multiplier_scales_the_body_rate() {
    let Some((mut scene, tank, _)) = tank_scene() else {
        return;
    };
    let native = body_rate(&scene, tank);
    assert!(native as i16 > 1, "the Grizzly turns at a rate");
    let half = rot_modifier(&mut scene, tank, 1 << 15);
    assert_eq!(body_rate(&scene, tank), native / 2);
    let quadruple = rot_modifier(&mut scene, tank, 4 << 16);
    assert_eq!(body_rate(&scene, tank), native * 2);
    let loaded = super::tests::saved_and_loaded(&scene);
    assert_eq!(body_rate(&loaded, tank), native * 2);
    assert_eq!(
        loaded.sim().ext_modules.modifiers,
        scene.sim().ext_modules.modifiers
    );
    let sim = &mut scene.runtime.simulation;
    for handle in [half, quadruple] {
        sim.ext_modules
            .modifiers
            .remove(&mut sim.substrate.entities, handle)
            .expect("the modifier is held");
    }
    assert_eq!(body_rate(&scene, tank), native);
    assert!(scene.sim().ext_modules.modifiers.is_empty());
}

/// The tank ordered to attack a Russian infantryman eight cells east, out
/// of its range, approaches it; an `ApproachPoint` decider bound to the
/// tank answers the cell two east of it, which becomes the tank's
/// destination in place of the search's cell. The request carried the
/// target and its cell, and the chosen cell as `cell` and `value`.
#[test]
fn an_approach_point_answer_replaces_the_approach_cell() {
    use crate::sim::command::{Command, CommandEnvelope};
    use crate::sim::components::NavTargetRef;
    let Some((mut scene, tank, (x, y))) = tank_scene() else {
        return;
    };
    let Some(enemy) = ({
        let runtime = &mut scene.runtime;
        let enemy = runtime.simulation.spawn_object(
            "E1",
            "Russians",
            x + 8,
            y,
            0,
            &runtime.resources.rules,
        );
        runtime
            .simulation
            .resolve_type_handles(&runtime.resources.rules);
        enemy
    }) else {
        return;
    };
    let answer = super::cell_ref(x + 2, y);
    install_answering(&mut scene, EVENT_APPROACH_POINT, NO_PATH, tank, answer);
    let order = CommandEnvelope::new(
        scene.sim().interner.get("Americans").expect("Americans house"),
        scene.sim().session.tick() + 1,
        Command::Attack {
            attacker_id: tank,
            target_id: enemy,
        },
    );
    let mut orders = vec![order];
    for _ in 0..60 {
        scene
            .runtime
            .advance_frame(
                &std::mem::take(&mut orders),
                crate::headless_scenario::SIM_TICK_MS,
                crate::sim::world::TickLane::Ordinary,
                &mut crate::sim::world::FrameEffects::default(),
            )
            .expect("frame advances");
        if requests(&scene) > 0 {
            break;
        }
    }
    assert!(requests(&scene) > 0, "the tank's approach asked");
    let record = &memory(&scene)[512..672];
    assert_eq!(i32_at(record, 4), EVENT_APPROACH_POINT);
    assert_eq!(i64_at(record, 24), tank as i64, "subject");
    assert_eq!(i64_at(record, 32), enemy as i64, "the target");
    assert_eq!(i64_at(record, 80), super::cell_ref(x + 8, y), "its cell");
    assert_ne!(i64_at(record, 64), 0, "the chosen cell");
    assert_eq!(i64_at(record, 64), i64_at(record, 72));
    let entity = scene.sim().entities().get(tank).expect("tank");
    assert_eq!(
        entity.navigation.nav_com(),
        Some(NavTargetRef::cell(x + 2, y))
    );
    assert_eq!(scene.sim().ext_modules().status()[0].1, None);
}
