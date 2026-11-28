//! The load-time rewrite decides operation counts, stopping points, call
//! depth, NaN bits, float globals and memory growth, so the browser's
//! WebAssembly engine and the desktop's Wasmtime agree (RFC 0001 open
//! question 1). The browser side is checked in a browser; here Wasmtime runs
//! the rewritten bytes.

use crate::{
    ActionParams, Export, Fault, FindPage, Invalid, Limits, Query, SimHost, SimModule, Status,
};

/// Records every call a module makes, in order.
#[derive(Default, Debug, PartialEq)]
struct Calls(Vec<String>);

impl SimHost for Calls {
    fn frame(&mut self) -> i32 {
        self.0.push("frame".into());
        7
    }
    fn random(&mut self, lo: i32, hi: i32) -> i32 {
        self.0.push(format!("random {lo} {hi}"));
        (lo + hi) / 2
    }
    fn log(&mut self, line: &str) {
        self.0.push(format!("log {line}"));
    }
    fn has_kind(&mut self, _table: i32, _id: i32) -> bool {
        false
    }
    fn lookup(&mut self, _kind: i32, _name: &str) -> Result<(i32, Status), Invalid> {
        Ok((0, Status::NotFound))
    }
    fn name_of(&mut self, _kind: i32, _id: i32) -> Result<(String, Status), Invalid> {
        Ok((String::new(), Status::NotFound))
    }
    fn get(&mut self, reference: i64, prop: i32) -> Result<(i64, Status), Invalid> {
        self.0.push(format!("get {reference} {prop}"));
        Ok((reference * 3 + i64::from(prop), Status::Ok))
    }
    fn set(&mut self, reference: i64, prop: i32, value: i64) -> Result<Status, Invalid> {
        self.0.push(format!("set {reference} {prop} {value}"));
        Ok(Status::Ok)
    }
    fn find(&mut self, _query: &Query, _cap: usize) -> Result<(FindPage, Status), Invalid> {
        Ok((FindPage::default(), Status::Ok))
    }
    fn var_get(&mut self, _scope: i64, _var: i32) -> Result<(i64, Status), Invalid> {
        Ok((0, Status::Ok))
    }
    fn var_set(&mut self, scope: i64, var: i32, value: i64) -> Result<Status, Invalid> {
        self.0.push(format!("var_set {scope} {var} {value}"));
        Ok(Status::Ok)
    }
    fn action(
        &mut self,
        _kind: i32,
        _target: i64,
        _params: &ActionParams,
        _text: &str,
    ) -> Result<(i64, Status), Invalid> {
        Ok((0, Status::Ok))
    }
}

/// What one run leaves behind.
#[derive(Debug, PartialEq)]
struct Outcome {
    results: Vec<Result<u64, Fault>>,
    memory: Vec<u8>,
    globals: Vec<u64>,
    calls: Calls,
}

fn run(wat: &str, budgets: &[u64]) -> Outcome {
    let bytes = wat::parse_str(wat).expect("test WAT parses");
    let mut module = SimModule::load(&bytes, Limits::MAP).expect("module loads");
    let mut calls = Calls::default();
    let results = budgets
        .iter()
        .map(|budget| module.call(Export::Frame, *budget, &mut calls))
        .collect();
    Outcome {
        results,
        memory: module.memory(),
        globals: module.globals().to_vec(),
        calls,
    }
}

/// Float work whose NaNs a CPU may give any bits, integer work, a loop and a
/// table call; each call adds one row of results to memory.
const COMPUTE: &str = r#"
(module
  (memory (export "memory") 1)
  (global $row (mut i32) (i32.const 0))
  (global $acc (mut f64) (f64.const 1.5))
  (table 2 funcref)
  (elem (i32.const 0) $sq $neg)
  (type $un (func (param f64) (result f64)))
  (func $sq (param f64) (result f64) (f64.mul (local.get 0) (local.get 0)))
  (func $neg (param f64) (result f64) (f64.neg (local.get 0)))
  (func (export "on_frame")
    (local $i i32) (local $x f32) (local $base i32)
    (local.set $base (i32.mul (global.get $row) (i32.const 64)))
    ;; NaNs from 0/0, sqrt(-1), inf-inf, and min with a NaN input.
    (f32.store (local.get $base) (f32.div (f32.const 0) (f32.const 0)))
    (f64.store offset=8 (local.get $base) (f64.sqrt (f64.const -1)))
    (f64.store offset=16 (local.get $base)
      (f64.sub (f64.const inf) (f64.const inf)))
    (f32.store offset=24 (local.get $base)
      (f32.min (f32.const nan:0x200001) (f32.const 1)))
    (f32.store offset=28 (local.get $base)
      (f32.demote_f64 (f64.const nan:0x4000000000001)))
    ;; A loop of mixed arithmetic.
    (local.set $x (f32.const 0.1))
    (loop $l
      (local.set $x (f32.add (f32.mul (local.get $x) (f32.const 1.0001)) (f32.const 0.3)))
      (global.set $acc (call_indirect (type $un)
        (f64.add (global.get $acc) (f64.promote_f32 (local.get $x)))
        (i32.and (local.get $i) (i32.const 1))))
      (br_if $l (i32.lt_u (local.tee $i (i32.add (local.get $i) (i32.const 1)))
                          (i32.const 40))))
    (f32.store offset=32 (local.get $base) (local.get $x))
    (f64.store offset=40 (local.get $base) (global.get $acc))
    (i64.store offset=48 (local.get $base)
      (i64.rotl (i64.const 0x123456789) (i64.extend_i32_u (local.get $i))))
    (global.set $row (i32.add (global.get $row) (i32.const 1)))))
"#;

#[test]
fn float_results_are_canonical_nans() {
    let outcome = run(COMPUTE, &[100_000, 100_000, 100_000]);
    assert!(outcome.results.iter().all(Result::is_ok), "{:?}", outcome.results);
    // Every NaN stored is the canonical one.
    for row in 0..3 {
        let at = |o: usize| &outcome.memory[row * 64 + o..];
        let f32_bits = |o| u32::from_le_bytes(at(o)[..4].try_into().unwrap());
        let f64_bits = |o| u64::from_le_bytes(at(o)[..8].try_into().unwrap());
        assert_eq!(f32_bits(0), 0x7FC0_0000);
        assert_eq!(f64_bits(8), 0x7FF8_0000_0000_0000);
        assert_eq!(f64_bits(16), 0x7FF8_0000_0000_0000);
        assert_eq!(f32_bits(24), 0x7FC0_0000);
        assert_eq!(f32_bits(28), 0x7FC0_0000);
    }
}

#[test]
fn a_budget_stops_at_the_charged_block() {
    // Counts loop iterations in a global until the budget runs out; each
    // budget below stops it at a different iteration.
    let wat = r#"(module (memory (export "memory") 1)
      (global $n (mut i64) (i64.const 0))
      (func (export "on_frame")
        (loop $l
          (global.set $n (i64.add (global.get $n) (i64.const 1)))
          (br $l))))"#;
    for budget in [1, 7, 8, 1_000, 12_345] {
        let outcome = run(wat, &[budget]);
        assert_eq!(outcome.results, vec![Err(Fault::Budget)]);
    }
    // Entry charges the `loop` instruction; each iteration is one block of
    // five instructions, charged before it runs.
    let outcome = run(wat, &[1 + 5 * 1_200]);
    assert_eq!(outcome.results, vec![Err(Fault::Budget)]);
    assert_eq!(outcome.globals, vec![1_200]);
}

#[test]
fn recursion_stops_at_the_weighted_depth_limit() {
    let wat = r#"(module (memory (export "memory") 1)
      (global $depth (mut i32) (i32.const 0))
      (func $down (param i64) (local i64 i64)
        (global.set $depth (i32.add (global.get $depth) (i32.const 1)))
        (call $down (i64.add (local.get 0) (i64.const 1))))
      (func (export "on_frame") (call $down (i64.const 0))))"#;
    let outcome = run(wat, &[u64::MAX / 2]);
    assert_eq!(
        outcome.results,
        vec![Err(Fault::Trap("call stack exhausted".into()))]
    );
    // `$down` weighs 4 + 1 parameter + 2 locals; `on_frame` weighs 4.
    assert_eq!(
        outcome.globals,
        vec![((crate::instrument::MAX_STACK_SLOTS - 4) / 7) as u64]
    );
}

#[test]
fn host_calls_spend_the_same_budget() {
    let wat = r#"(module
      (import "vera:sim@1.0" "frame" (func $frame (result i32)))
      (import "vera:sim@1.0" "random" (func $random (param i32 i32 i32) (result i32)))
      (import "vera:sim@1.0" "log" (func $log (param i32 i32)))
      (import "vera:sim@1.0" "get" (func $get (param i64 i32 i32) (result i32)))
      (import "vera:sim@1.0" "set" (func $set (param i64 i32 i64) (result i32)))
      (import "vera:sim@1.0" "var_set" (func $var_set (param i64 i32 i64) (result i32)))
      (memory (export "memory") 1)
      (data (i32.const 0) "tick")
      (func (export "on_frame") (local $i i32)
        (call $log (i32.const 0) (i32.const 4))
        (loop $l
          (drop (call $get (i64.extend_i32_u (local.get $i)) (i32.const 3) (i32.const 64)))
          (drop (call $set (i64.extend_i32_u (local.get $i)) (i32.const 3)
            (i64.add (i64.load (i32.const 64)) (i64.extend_i32_s (call $frame)))))
          (drop (call $random (i32.const 0) (local.get $i) (i32.const 72)))
          (br_if $l (i32.lt_u (local.tee $i (i32.add (local.get $i) (i32.const 1)))
                              (i32.const 5))))
        (drop (call $var_set (i64.const 0) (i32.const 1) (i64.load (i32.const 72))))))"#;
    // Enough, then too little for the last calls.
    let enough = run(wat, &[100_000]);
    let used = *enough.results[0].as_ref().expect("enough budget");
    let short = run(wat, &[used - 1]);
    assert_eq!(short.results, vec![Err(Fault::Budget)]);
    assert!(short.calls.0.len() < enough.calls.0.len());
}

#[test]
fn bulk_memory_is_charged_by_length() {
    let wat = r#"(module (memory (export "memory") 2)
      (func (export "on_frame")
        (memory.fill (i32.const 0) (i32.const 7) (i32.const 65536))
        (memory.copy (i32.const 65536) (i32.const 0) (i32.const 4096))))"#;
    let enough = run(wat, &[1_000_000]);
    // 65,536 / 8 + 4,096 / 8 plus the instructions.
    let used = *enough.results[0].as_ref().expect("enough budget");
    assert!(used > 8_192 + 512, "{used}");
    let short = run(wat, &[8_000]);
    assert_eq!(short.results, vec![Err(Fault::Budget)]);
    assert!(short.memory.iter().all(|b| *b == 0));
}

#[test]
fn a_float_global_keeps_every_bit_as_an_integer() {
    // A NaN with a payload, stored and read back through a mutable f64
    // global, keeps its bits; the saved global holds the same bits.
    let wat = r#"(module (memory (export "memory") 1)
      (global $g (mut f64) (f64.const 1.5))
      (func (export "on_frame")
        (global.set $g (f64.reinterpret_i64 (i64.const 0x7FF4000000000123)))
        (i64.store (i32.const 0) (i64.reinterpret_f64 (global.get $g)))))"#;
    let outcome = run(wat, &[1_000]);
    assert_eq!(outcome.results.len(), 1);
    assert!(outcome.results[0].is_ok());
    assert_eq!(&outcome.memory[..8], &0x7FF4_0000_0000_0123u64.to_le_bytes());
    assert_eq!(outcome.globals, vec![0x7FF4_0000_0000_0123]);
}

#[test]
fn memory_stops_growing_at_the_module_limit() {
    // Map modules get 16 MiB, 256 pages: growing from 1 by 255 succeeds,
    // one more page fails with -1, whatever the module declared.
    let wat = r#"(module (memory (export "memory") 1 65536)
      (func (export "on_frame")
        (i32.store (i32.const 0) (memory.grow (i32.const 255)))
        (i32.store (i32.const 4) (memory.grow (i32.const 1)))))"#;
    let outcome = run(wat, &[1_000]);
    assert!(outcome.results[0].is_ok());
    assert_eq!(&outcome.memory[..8], &[1, 0, 0, 0, 0xFF, 0xFF, 0xFF, 0xFF]);
    assert_eq!(outcome.memory.len(), 256 * crate::PAGE_BYTES);
}
