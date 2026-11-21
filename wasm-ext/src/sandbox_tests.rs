//! Load checks, metering and the host boundary, through modules written in WAT.

use crate::{
    ActionParams, Export, Fault, FindPage, Invalid, Limits, LoadError, Query, SimHost, SimModule,
    Status,
};

#[derive(Default)]
struct Recorder {
    frame: i32,
    logs: Vec<String>,
    vars: Vec<(i64, i32, i64)>,
    actions: Vec<(i32, i64, ActionParams)>,
    finds: usize,
}

/// The fake world's objects: sort keys 10, 20, ..., 50, reference = key.
const OBJECT_KEYS: [i64; 5] = [10, 20, 30, 40, 50];

impl SimHost for Recorder {
    fn frame(&mut self) -> i32 {
        self.frame
    }
    fn random(&mut self, lo: i32, _hi: i32) -> i32 {
        lo
    }
    fn log(&mut self, line: &str) {
        self.logs.push(line.to_owned());
    }
    fn has_kind(&mut self, _table: i32, _id: i32) -> bool {
        false
    }
    fn lookup(&mut self, _kind: i32, _name: &str) -> Result<(i32, Status), Invalid> {
        Ok((7, Status::Unsupported))
    }
    fn name_of(&mut self, _kind: i32, _id: i32) -> Result<(String, Status), Invalid> {
        Ok((String::new(), Status::Unsupported))
    }
    /// Reference 0 is gone; any other reads `reference * 100 + prop`.
    fn get(&mut self, reference: i64, prop: i32) -> Result<(i64, Status), Invalid> {
        Ok(match reference {
            0 => (99, Status::Gone),
            _ => (reference * 100 + i64::from(prop), Status::Ok),
        })
    }
    fn set(&mut self, _reference: i64, _prop: i32, _value: i64) -> Result<Status, Invalid> {
        Ok(Status::Refused)
    }
    fn find(&mut self, query: &Query, cap: usize) -> Result<(FindPage, Status), Invalid> {
        self.finds += 1;
        let found: Vec<(i64, i64)> = OBJECT_KEYS
            .iter()
            .filter(|key| **key >= query.after)
            .take(cap)
            .map(|key| (*key, *key))
            .collect();
        let scanned = found.len() as u64;
        Ok((FindPage { found, scanned }, Status::Ok))
    }
    fn var_get(&mut self, _scope: i64, var: i32) -> Result<(i64, Status), Invalid> {
        if var < 0 {
            return Err(Invalid("negative variable".into()));
        }
        Ok((7, Status::Ok))
    }
    fn var_set(&mut self, scope: i64, var: i32, value: i64) -> Result<Status, Invalid> {
        self.vars.push((scope, var, value));
        Ok(Status::Ok)
    }
    fn action(
        &mut self,
        kind: i32,
        target: i64,
        params: &ActionParams,
    ) -> Result<(i64, Status), Invalid> {
        self.actions.push((kind, target, *params));
        Ok((0, Status::Ok))
    }
}

fn load(wat: &str) -> Result<SimModule, LoadError> {
    SimModule::load(&wat::parse_str(wat).expect("test WAT parses"), Limits::MAP)
}

const HOST_USER: &str = r#"
(module
  (import "vera:sim@1.0" "frame" (func $frame (result i32)))
  (import "vera:sim@1.0" "log" (func $log (param i32 i32)))
  (import "vera:sim@1.0" "var_get" (func $var_get (param i64 i32 i32) (result i32)))
  (import "vera:sim@1.0" "var_set" (func $var_set (param i64 i32 i64) (result i32)))
  (import "vera:sim@1.0" "action" (func $action (param i32 i64 i32 i32 i32) (result i32)))
  (memory (export "memory") 1)
  (data (i32.const 0) "hello")
  (func (export "on_frame")
    ;; var_set(0, 3, frame + var_get(0, 3))
    (drop (call $var_get (i64.const 0) (i32.const 3) (i32.const 64)))
    (drop (call $var_set (i64.const 0) (i32.const 3)
      (i64.add (i64.extend_i32_s (call $frame)) (i64.load (i32.const 64)))))
    (call $log (i32.const 0) (i32.const 5))
    ;; ActionParams at 128: value = 500
    (i64.store (i32.const 144) (i64.const 500))
    (drop (call $action (i32.const 7) (i64.const 42) (i32.const 128) (i32.const 72) (i32.const 256)))))
"#;

#[test]
fn a_module_reaches_the_game_only_through_the_host() {
    let mut module = load(HOST_USER).expect("module loads");
    let mut host = Recorder {
        frame: 30,
        ..Default::default()
    };
    let used = module
        .call(Export::Frame, 100_000, &mut host)
        .expect("call succeeds");
    assert!(used > 0);
    assert_eq!(host.vars, vec![(0, 3, 37)]);
    assert_eq!(host.logs, vec!["hello".to_owned()]);
    assert_eq!(host.actions.len(), 1);
    assert_eq!(host.actions[0].2.value, 500);
    assert!(!module.has_export(Export::Init));
    assert_eq!(module.call(Export::Init, 10, &mut host), Ok(0));
}

#[test]
fn a_budget_stops_an_endless_loop_at_the_same_operation_every_time() {
    let wat = r#"(module (memory (export "memory") 1)
      (func (export "on_frame") (loop $l (i32.store (i32.const 0) (i32.add (i32.load (i32.const 0)) (i32.const 1))) (br $l))))"#;
    let mut counts = Vec::new();
    for _ in 0..2 {
        let mut module = load(wat).expect("module loads");
        assert_eq!(
            module.call(Export::Frame, 10_000, &mut Recorder::default()),
            Err(Fault::Budget)
        );
        counts.push(module.memory()[..4].to_vec());
    }
    assert_eq!(counts[0], counts[1]);
    assert_ne!(counts[0], vec![0, 0, 0, 0]);
}

#[test]
fn an_invalid_host_argument_is_a_fault() {
    let wat = r#"(module
      (import "vera:sim@1.0" "random" (func $random (param i32 i32 i32) (result i32)))
      (memory (export "memory") 1)
      (func (export "on_frame") (drop (call $random (i32.const 5) (i32.const 1) (i32.const 0)))))"#;
    let mut module = load(wat).expect("module loads");
    assert!(matches!(
        module.call(Export::Frame, 10_000, &mut Recorder::default()),
        Err(Fault::Invalid(_))
    ));
}

#[test]
fn float_results_use_the_canonical_nan() {
    // 0/0 with an operand NaN carrying a payload: hardware may propagate the
    // payload; canonicalization stores 0x7FC00000 on every CPU.
    let wat = r#"(module (memory (export "memory") 1)
      (func (export "on_frame")
        (f32.store (i32.const 0) (f32.add (f32.const nan:0x200001) (f32.const 1)))))"#;
    let mut module = load(wat).expect("module loads");
    module
        .call(Export::Frame, 10_000, &mut Recorder::default())
        .expect("call succeeds");
    assert_eq!(module.memory()[..4], 0x7FC0_0000u32.to_le_bytes());
}

#[test]
fn load_rejects_start_sections_foreign_imports_and_simd() {
    let start = r#"(module (func $s) (start $s))"#;
    let wasi = r#"(module (import "wasi_snapshot_preview1" "fd_write" (func (param i32 i32 i32 i32) (result i32))))"#;
    let unknown = r#"(module (import "vera:sim@1.0" "teleport" (func)))"#;
    let simd = r#"(module (func (drop (v128.const i64x2 0 0))))"#;
    for wat in [start, wasi, unknown, simd] {
        assert!(matches!(load(wat), Err(LoadError::Invalid(_))), "{wat}");
    }
}

fn i64_at(memory: &[u8], at: usize) -> i64 {
    i64::from_le_bytes(memory[at..at + 8].try_into().expect("8 bytes"))
}

fn i32_at(memory: &[u8], at: usize) -> i32 {
    i32::from_le_bytes(memory[at..at + 4].try_into().expect("4 bytes"))
}

#[test]
fn get_many_reads_row_major_and_empties_a_gone_row() {
    // refs [1, 0, 2] at 0, props [lookup("health"), 1] at 64; out at 128.
    let wat = r#"(module
      (import "vera:sim@1.0" "lookup" (func $lookup (param i32 i32 i32 i32) (result i32)))
      (import "vera:sim@1.0" "get_many" (func $get_many (param i32 i32 i32 i32 i32 i32) (result i32)))
      (memory (export "memory") 1)
      (data (i32.const 0) "\01\00\00\00\00\00\00\00\00\00\00\00\00\00\00\00\02\00\00\00\00\00\00\00")
      (data (i32.const 32) "health")
      (func (export "on_frame")
        (i32.store (i32.const 300) (call $lookup (i32.const 19) (i32.const 32) (i32.const 6) (i32.const 64)))
        (i32.store (i32.const 68) (i32.const 1))
        (i32.store (i32.const 304)
          (call $get_many (i32.const 0) (i32.const 3) (i32.const 64) (i32.const 2) (i32.const 128) (i32.const 308)))))"#;
    let mut module = load(wat).expect("module loads");
    module
        .call(Export::Frame, 100_000, &mut Recorder::default())
        .expect("call succeeds");
    let memory = module.memory();
    assert_eq!(i32_at(memory, 300), Status::Ok as i32);
    let health = crate::property_id("Health").expect("Health is a property");
    assert_eq!(i32_at(memory, 64), health);
    let values: Vec<i64> = (0..6).map(|i| i64_at(memory, 128 + i * 8)).collect();
    let h = i64::from(health);
    assert_eq!(values, vec![100 + h, 101, 0, 0, 200 + h, 201]);
    assert_eq!(i32_at(memory, 304), Status::Gone as i32);
    assert_eq!(i32_at(memory, 308), 6);
}

#[test]
fn find_pages_by_sort_key_and_a_zero_capacity_query_scans_nothing() {
    // Query at 0 (scope Objects, whole map); each call pages with cap 2 from
    // `after` (offset 48) and stores the next key back into it.
    let wat = r#"(module
      (import "vera:sim@1.0" "find" (func $find (param i32 i32 i32 i32 i32) (result i32)))
      (memory (export "memory") 1)
      (func (export "on_frame")
        (i32.store (i32.const 600)
          (call $find (i32.const 0) (i32.const 64) (i32.const 256) (i32.const 2) (i32.const 512)))
        (i64.store (i32.const 48) (i64.load (i32.const 520))))
      (func (export "vera_init")
        (drop (call $find (i32.const 0) (i32.const 64) (i32.const 256) (i32.const 0) (i32.const 512)))))"#;
    let mut module = load(wat).expect("module loads");
    let mut host = Recorder::default();
    module
        .call(Export::Init, 100_000, &mut host)
        .expect("call succeeds");
    assert_eq!(host.finds, 0);
    assert_eq!(i64_at(module.memory(), 520), 0);
    let mut pages = Vec::new();
    for _ in 0..3 {
        module
            .call(Export::Frame, 100_000, &mut host)
            .expect("call succeeds");
        let memory = module.memory();
        assert_eq!(i32_at(memory, 600), Status::Ok as i32);
        let count = i32_at(memory, 512) as usize;
        let refs: Vec<i64> = (0..count).map(|i| i64_at(memory, 256 + i * 8)).collect();
        pages.push((refs, i64_at(memory, 520)));
    }
    assert_eq!(
        pages,
        vec![(vec![10, 20], 21), (vec![30, 40], 41), (vec![50], 0)]
    );
    // An `Other` clause is not valid in a query: the module stops.
    let bad = r#"(module
      (import "vera:sim@1.0" "find" (func $find (param i32 i32 i32 i32 i32) (result i32)))
      (memory (export "memory") 1)
      (func (export "on_frame")
        (i32.store (i32.const 56) (i32.const 1))
        (i32.store8 (i32.const 64) (i32.const 1))
        (drop (call $find (i32.const 0) (i32.const 80) (i32.const 256) (i32.const 2) (i32.const 512)))))"#;
    let mut module = load(bad).expect("module loads");
    assert!(matches!(
        module.call(Export::Frame, 100_000, &mut Recorder::default()),
        Err(Fault::Invalid(_))
    ));
}

#[test]
fn property_names_are_unique_ignoring_case() {
    for (i, name) in crate::PROPERTIES.iter().enumerate() {
        assert_eq!(crate::property_id(name), Some(i as i32), "{name}");
    }
}

#[test]
fn a_restored_module_continues_exactly_where_the_save_left_it() {
    // An unexported mutable global (like a compiler's stack pointer) and a
    // memory write two pages in, both changing every call.
    let wat = r#"(module (memory (export "memory") 1)
      (global $n (mut i64) (i64.const 0))
      (func (export "on_frame")
        (global.set $n (i64.add (global.get $n) (i64.const 3)))
        (if (i32.eqz (i32.wrap_i64 (i64.rem_u (global.get $n) (i64.const 2))))
          (then (drop (memory.grow (i32.const 1)))))
        (i64.store (i32.const 70000) (global.get $n))))"#;
    let bytes = wat::parse_str(wat).expect("test WAT parses");
    let mut live = SimModule::load(&bytes, Limits::MAP).expect("module loads");
    for _ in 0..3 {
        live.call(Export::Frame, 10_000, &mut Recorder::default())
            .ok();
    }
    let state = live.state();
    assert_eq!(state.globals, vec![9]);
    let mut restored =
        SimModule::restore(live.bytes(), Limits::MAP, &state).expect("state restores");
    assert_eq!(restored.memory(), live.memory());
    for module in [&mut live, &mut restored] {
        module
            .call(Export::Frame, 10_000, &mut Recorder::default())
            .ok();
    }
    assert_eq!(restored.globals(), live.globals());
    assert_eq!(restored.memory(), live.memory());
    assert_eq!(live.globals(), &[12]);
}
