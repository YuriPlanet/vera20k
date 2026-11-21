//! Load checks, metering and the host boundary, through modules written in WAT.

use crate::{ActionParams, Export, Fault, Invalid, Limits, LoadError, SimHost, SimModule, Status};

#[derive(Default)]
struct Recorder {
    frame: i32,
    logs: Vec<String>,
    vars: Vec<(i64, i32, i64)>,
    actions: Vec<(i32, i64, ActionParams)>,
}

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
