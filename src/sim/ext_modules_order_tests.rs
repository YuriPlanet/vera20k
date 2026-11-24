//! Order results (RFC 0001 6i) on a retail battle against one computer
//! house: a module's orders raise `Commanded` on the units that took them
//! and `OrderRefused` with the receiver's reason for the ones refused, and
//! the commands the table gained (`Harvest`, `Enter`, `TakeProduct`,
//! `SuspendProduction`) reach the receiver.

use vera20k_wasm_ext::triggers::{EVENT_COMMANDED, EVENT_ORDER_REFUSED};
use vera20k_wasm_ext::{NAME_TECHNO_TYPE, refuse_reason};

use super::event_tests::{Ev, def};
use super::tests::{scenario_with_computer, spawn_for_computer};
use super::{ModuleSource, cell_ref, house_ref, type_ref};
use crate::headless_scenario::HeadlessScenario;

/// Records `on_events` keeps, from address [`LOG`].
const KEPT: usize = 32;
const LOG: usize = 16384;

/// One order's bytes (appendix `Order`): kind, unit count, target, value,
/// then the units.
fn order(kind: i32, target: i64, value: i64, units: &[u64]) -> Vec<u8> {
    let mut out = kind.to_le_bytes().to_vec();
    out.extend((units.len() as i32).to_le_bytes());
    out.extend(target.to_le_bytes());
    out.extend(value.to_le_bytes());
    for &unit in units {
        out.extend(unit.to_le_bytes());
    }
    out
}

/// A map module that, in `on_scenario_start`, creates a repeating
/// `Commanded` trigger bound to `bound` (statuses from 8) and a repeating
/// global `OrderRefused` trigger (create statuses at 0 and 4), then issues
/// `orders` for `house` (statuses from 256). `on_events` keeps the first
/// [`KEPT`] records from [`LOG`] and counts them at 80.
fn module(house: i64, bound: &[u64], orders: &[Vec<u8>]) -> Vec<u8> {
    let bytes = |b: &[u8]| b.iter().map(|b| format!("\\{b:02x}")).collect::<String>();
    let commanded = def(0, 2, &[Ev::on(EVENT_COMMANDED)], &[]);
    let refused = def(0, 2, &[Ev::on(EVENT_ORDER_REFUSED)], &[]);
    let binds: String = bound
        .iter()
        .enumerate()
        .map(|(i, unit)| {
            format!(
                "(i32.store (i32.const {}) (call $bind (i64.load (i32.const 64)) (i64.const {unit})))\n",
                8 + 4 * i
            )
        })
        .collect();
    let mut data = String::new();
    let mut calls = String::new();
    let mut at = 4096;
    for (i, order) in orders.iter().enumerate() {
        data.push_str(&format!("(data (i32.const {at}) \"{}\")\n", bytes(order)));
        calls.push_str(&format!(
            "(i32.store (i32.const {}) (call $order (i64.const {house}) (i32.const {at}) (i32.const {})))\n",
            256 + 4 * i,
            order.len()
        ));
        at += order.len().next_multiple_of(8);
    }
    wat::parse_str(format!(
        r#"(module
  (import "vera:sim@1.0" "trigger_create" (func $create (param i32 i32 i32) (result i32)))
  (import "vera:sim@1.0" "trigger_bind" (func $bind (param i64 i64) (result i32)))
  (import "vera:sim@1.0" "order" (func $order (param i64 i32 i32) (result i32)))
  (@custom "vera.delivery" "\01\00\20\00\00\00\10\00\00")
  (memory (export "memory") 1)
  (data (i32.const 2048) "{commanded}")
  (data (i32.const 3072) "{refused}")
  {data}
  (func (export "on_scenario_start")
    (i32.store (i32.const 0) (call $create (i32.const 2048) (i32.const {commanded_len}) (i32.const 64)))
    (i32.store (i32.const 4) (call $create (i32.const 3072) (i32.const {refused_len}) (i32.const 72)))
    {binds}
    {calls})
  (func (export "on_events") (param $ptr i32) (param $count i32) (local $i i32) (local $n i32)
    (block $done
      (loop $next
        (br_if $done (i32.ge_u (local.get $i) (local.get $count)))
        (local.set $n (i32.load (i32.const 80)))
        (if (i32.lt_u (local.get $n) (i32.const {KEPT}))
          (then
            (memory.copy
              (i32.add (i32.const {LOG}) (i32.mul (local.get $n) (i32.const 120)))
              (i32.add (local.get $ptr) (i32.mul (local.get $i) (i32.const 120)))
              (i32.const 120))
            (i32.store (i32.const 80) (i32.add (local.get $n) (i32.const 1)))))
        (local.set $i (i32.add (local.get $i) (i32.const 1)))
        (br $next)))))"#,
        commanded = bytes(&commanded),
        refused = bytes(&refused),
        commanded_len = commanded.len(),
        refused_len = refused.len(),
    ))
    .expect("test module parses")
}

/// A kept record's fields.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Record {
    kind: i32,
    subject: i64,
    other: i64,
    house: i64,
    cell: i64,
    value: i64,
    value2: i64,
    id: i32,
}

fn memory(scene: &HeadlessScenario) -> Vec<u8> {
    scene.sim().ext_modules.slots[0].module.memory().to_vec()
}

fn i32_at(memory: &[u8], at: usize) -> i32 {
    i32::from_le_bytes(memory[at..at + 4].try_into().expect("4 bytes"))
}

fn records(scene: &HeadlessScenario) -> Vec<Record> {
    let m = memory(scene);
    let i64_at = |at: usize| i64::from_le_bytes(m[at..at + 8].try_into().expect("8 bytes"));
    let count = (i32_at(&m, 80) as usize).min(KEPT);
    (0..count)
        .map(|n| {
            let r = LOG + n * 120;
            Record {
                kind: i32_at(&m, r + 4),
                subject: i64_at(r + 24),
                other: i64_at(r + 32),
                house: i64_at(r + 40),
                cell: i64_at(r + 64),
                value: i64_at(r + 72),
                value2: i64_at(r + 80),
                id: i32_at(&m, r + 88),
            }
        })
        .collect()
}

/// The computer house's orders: a Move, then for the same tank an Attack on
/// a tank that went into limbo; an Apocalypse Tank it has no prerequisites
/// for; a Harvest; an Enter; a `TakeProduct` with nothing built and a
/// `SuspendProduction` with nothing building.
#[test]
fn module_orders_raise_commanded_and_order_refused() {
    let Some(mut scene) = scenario_with_computer() else {
        return;
    };
    let (house, tank, (x, y)) = spawn_for_computer(&mut scene, "MTNK");
    let (_, victim, _) = spawn_for_computer(&mut scene, "MTNK");
    let (_, miner, (mx, my)) = spawn_for_computer(&mut scene, "HARV");
    let (_, conscript, _) = spawn_for_computer(&mut scene, "E2");
    let (_, transport, _) = spawn_for_computer(&mut scene, "SAPC");
    let rules = &scene.runtime.resources.rules;
    let apocalypse = rules.type_handle("APOC").expect("APOC").0;
    let vehicle = rules.type_handle("HTNK").expect("HTNK").0;
    let goal = (x + 6, y);
    let field = (mx + 3, my);
    let orders = [
        order(0, cell_ref(goal.0, goal.1), 0, &[tank]),
        order(3, victim as i64, 0, &[tank]),
        order(9, 0, type_ref(NAME_TECHNO_TYPE, apocalypse), &[]),
        order(26, cell_ref(field.0, field.1), 0, &[miner]),
        order(25, transport as i64, 0, &[conscript]),
        order(27, 0, 3, &[]),
        order(10, 0, type_ref(NAME_TECHNO_TYPE, vehicle), &[]),
    ];
    let computer = house_ref(house);
    let module = module(computer, &[tank, miner, conscript], &orders);
    scene
        .runtime
        .simulation
        .install_ext_module(&scene.runtime.resources.rules, ModuleSource::Map, &module)
        .expect("module installs");
    // The frame that issues the orders; they execute in a later command
    // step, while the attacked tank is in limbo.
    scene.tick();
    let m = memory(&scene);
    let statuses: Vec<i32> = (0..5).map(|i| i32_at(&m, 4 * i)).collect();
    assert_eq!(statuses, [0; 5], "create and bind answer Ok");
    let issued: Vec<i32> = (0..orders.len()).map(|i| i32_at(&m, 256 + 4 * i)).collect();
    assert_eq!(issued, [0; 7], "every order queues");
    let limbo = |scene: &mut HeadlessScenario, on: bool| {
        scene
            .runtime
            .simulation
            .entities_mut_for_test()
            .get_mut_for_test(victim)
            .expect("the attacked tank")
            .lifecycle
            .set_in_limbo_for_test(on);
    };
    limbo(&mut scene, true);
    scene.tick();
    scene.tick();
    limbo(&mut scene, false);
    for _ in 0..4 {
        scene.tick();
    }
    assert_eq!(scene.sim().ext_modules().status()[0].1, None);
    let got = records(&scene);
    let find = |kind: i32, subject: u64, value: i64| {
        got.iter()
            .find(|r| r.kind == kind && r.subject == subject as i64 && r.value == value)
            .copied()
            .unwrap_or_else(|| panic!("no record {kind} on {subject} for {value}: {got:?}"))
    };
    let moved = find(EVENT_COMMANDED, tank, 0);
    assert!(moved.cell != 0 && moved.value2 > 0, "{moved:?}");
    let attack = find(EVENT_ORDER_REFUSED, tank, 3);
    assert_eq!(
        (attack.other, attack.house, attack.value2, attack.id),
        (victim as i64, computer, refuse_reason::ILLEGAL_TARGET, -1)
    );
    let produce = find(EVENT_ORDER_REFUSED, 0, 9);
    assert_eq!(
        (produce.value2, produce.id),
        (refuse_reason::PREREQUISITE, apocalypse as i32)
    );
    let harvest = find(EVENT_COMMANDED, miner, 26);
    assert_ne!(harvest.cell, 0, "{harvest:?}");
    let enter = find(EVENT_COMMANDED, conscript, 25);
    assert_eq!(enter.other, transport as i64);
    assert_ne!(moved.value2, harvest.value2, "each order its own serial");
    let take = find(EVENT_ORDER_REFUSED, 0, 27);
    assert_eq!(take.value2, refuse_reason::NOT_READY);
    let suspend = find(EVENT_ORDER_REFUSED, 0, 10);
    assert_eq!((suspend.value2, suspend.id), (refuse_reason::OTHER, -1));
}
