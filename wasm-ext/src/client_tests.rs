//! Client modules (`ui.wasm`): load checks, `client_init`'s restriction, the
//! frame budget, event delivery and the client-only imports, through modules
//! written in WAT and a fake [`ClientHost`].

use crate::client::{CLIENT_FRAME_EVENTS, client_action, client_event};
use crate::{
    ActionParams, ClientEvent, ClientHost, ClientModule, Fault, FindPage, Invalid, Limits,
    LoadError, NodeInit, Order, Query, SimModule, Source, Status, property_id,
};

#[derive(Default)]
struct Fake {
    logs: Vec<String>,
    gets: usize,
    /// (reference, property, value) of every `client_set`.
    sets: Vec<(i64, i32, i64)>,
    created: Vec<(i32, i64, Vec<NodeInit>)>,
    actions: Vec<(i32, i64, String, Vec<u8>)>,
    messages: Vec<Vec<u8>>,
    storage: Vec<(String, Vec<u8>)>,
}

impl ClientHost for Fake {
    fn frame(&mut self) -> i32 {
        42
    }
    fn log(&mut self, line: &str) {
        self.logs.push(line.to_owned());
    }
    fn has_kind(&mut self, _table: i32, _id: i32) -> bool {
        true
    }
    fn lookup(&mut self, _kind: i32, _name: &str) -> Result<(i32, Status), Invalid> {
        Ok((-1, Status::NotFound))
    }
    fn name_of(&mut self, _kind: i32, _id: i32) -> Result<(String, Status), Invalid> {
        Ok((String::new(), Status::NotFound))
    }
    fn get(&mut self, _reference: i64, _prop: i32) -> Result<(i64, Status), Invalid> {
        self.gets += 1;
        Ok((1, Status::Ok))
    }
    fn find(&mut self, _query: &Query, _cap: usize) -> Result<(FindPage, Status), Invalid> {
        Ok((FindPage::default(), Status::Ok))
    }
    fn var_get(&mut self, _scope: i64, _var: i32) -> Result<(i64, Status), Invalid> {
        Ok((0, Status::Ok))
    }
    fn set(&mut self, reference: i64, prop: i32, value: i64) -> Result<Status, Invalid> {
        self.sets.push((reference, prop, value));
        Ok(Status::Ok)
    }
    fn set_bytes(&mut self, _reference: i64, _prop: i32, _bytes: &[u8]) -> Result<Status, Invalid> {
        Ok(Status::Ok)
    }
    fn node_create(
        &mut self,
        kind: i32,
        parent: i64,
        init: &[NodeInit],
    ) -> Result<(i64, Status), Invalid> {
        self.created.push((kind, parent, init.to_vec()));
        Ok((5 << 48 | self.created.len() as i64, Status::Ok))
    }
    fn node_remove(&mut self, _node: i64) -> Result<Status, Invalid> {
        Ok(Status::Ok)
    }
    fn node_find(&mut self, _name: &str) -> Result<(i64, Status), Invalid> {
        Ok((0, Status::NotFound))
    }
    fn action(
        &mut self,
        kind: i32,
        target: i64,
        _params: &ActionParams,
        text: &str,
        extra: &[u8],
    ) -> Result<Status, Invalid> {
        self.actions
            .push((kind, target, text.to_owned(), extra.to_vec()));
        Ok(Status::Ok)
    }
    fn send_order(&mut self, _order: &Order) -> Result<Status, Invalid> {
        Ok(Status::Refused)
    }
    fn send_message(&mut self, data: &[u8]) -> Result<Status, Invalid> {
        self.messages.push(data.to_vec());
        Ok(Status::Ok)
    }
    fn storage_read(&mut self, key: &str) -> Result<(Option<Vec<u8>>, Status), Invalid> {
        Ok(match self.storage.iter().find(|(k, _)| k == key) {
            Some((_, value)) => (Some(value.clone()), Status::Ok),
            None => (None, Status::NotFound),
        })
    }
    fn status_template(&mut self, text: &str) -> Result<Status, Invalid> {
        self.logs.push(text.to_owned());
        Ok(Status::Ok)
    }

    fn storage_write(&mut self, key: &str, data: &[u8]) -> Result<Status, Invalid> {
        self.storage.retain(|(k, _)| k != key);
        if !data.is_empty() {
            self.storage.push((key.to_owned(), data.to_vec()));
        }
        Ok(Status::Ok)
    }
}

fn load(wat: &str) -> Result<ClientModule, LoadError> {
    ClientModule::load(&wat::parse_str(wat).expect("test WAT parses"), Limits::MAP)
}

fn memory_i32(module: &ClientModule, at: usize) -> i32 {
    let mut bytes = [0u8; 4];
    assert!(module.read_memory(at, &mut bytes));
    i32::from_le_bytes(bytes)
}

fn load_error(wat: &str) -> String {
    match load(wat) {
        Err(LoadError::Invalid(why)) => why,
        Ok(_) => panic!("module loaded"),
    }
}

#[test]
fn a_client_module_imports_only_client_functions() {
    let sim_import = r#"(module (import "vera:sim@1.0" "frame" (func (result i32))))"#;
    assert!(load_error(sim_import).contains("only vera:client@1.0"));
    let unknown = r#"(module (import "vera:client@1.0" "client_bogus" (func)))"#;
    assert!(load_error(unknown).contains("unknown function client_bogus"));
    let wrong_type =
        r#"(module (import "vera:client@1.0" "client_frame" (func (param i32) (result i32))))"#;
    assert!(load(wrong_type).is_err());
    // And a simulation module cannot import the client's.
    let client_import = r#"(module (import "vera:client@1.0" "client_frame" (func (result i32))))"#;
    let bytes = wat::parse_str(client_import).expect("test WAT parses");
    assert!(SimModule::load(&bytes, Limits::MAP).is_err());
    let ok = r#"(module (import "vera:client@1.0" "client_frame" (func (result i32)))
      (func (export "client_on_start")))"#;
    let module = load(ok).expect("module loads");
    assert!(module.has_export("client_on_start"));
    assert!(!module.has_export("client_on_render"));
}

#[test]
fn exports_and_sections_follow_the_client_abi() {
    let int_render = r#"(module (func (export "client_on_render") (param i32 i32)))"#;
    assert!(load_error(int_render).contains("(f32, f32)"));
    let start = r#"(module (start $s) (func $s))"#;
    assert!(load_error(start).contains("client_init"));
    let vars = r#"(module (@custom "vera.vars" "\01\00\00\00\00"))"#;
    assert!(load_error(vars).contains("vera.vars"));
    let events = r#"(module (memory (export "memory") 1)
      (func (export "client_on_events") (param i32 i32)))"#;
    assert!(load_error(events).contains("vera.delivery"));
    // 100 bytes cannot hold a record with a full notification.
    let small = r#"(module (memory (export "memory") 1)
      (@custom "vera.delivery" "\01\00\04\00\00\64\00\00\00")
      (func (export "client_on_events") (param i32 i32)))"#;
    assert!(load_error(small).contains("smaller than"));
}

const INIT: &str = r#"
(module
  (import "vera:client@1.0" "client_lookup" (func $lookup (param i32 i32 i32 i32) (result i32)))
  (import "vera:client@1.0" "client_get" (func $get (param i64 i32 i32) (result i32)))
  (import "vera:client@1.0" "client_log" (func $log (param i32 i32)))
  (import "vera:client@1.0" "client_frame" (func $frame (result i32)))
  (memory (export "memory") 1)
  (data (i32.const 100) "Health")
  (data (i32.const 120) "init")
  (func $probe
    (i32.store (i32.const 0) (call $get (i64.const 1) (i32.const 3) (i32.const 16)))
    (i32.store (i32.const 4) (call $lookup (i32.const 19) (i32.const 100) (i32.const 6) (i32.const 8)))
    (i32.store (i32.const 24) (call $frame))
    (call $log (i32.const 120) (i32.const 4)))
  (func (export "client_init") (call $probe))
  (func (export "client_on_start") (call $probe)))
"#;

#[test]
fn client_init_reaches_only_names_and_the_log() {
    let mut module = load(INIT).expect("module loads");
    let mut host = Fake::default();
    module.init(&mut host).expect("init runs");
    assert_eq!(memory_i32(&module, 0), Status::Unsupported as i32);
    assert_eq!(memory_i32(&module, 4), Status::Ok as i32);
    assert_eq!(Some(memory_i32(&module, 8)), property_id("Health"));
    assert_eq!(memory_i32(&module, 24), 0);
    assert_eq!(host.gets, 0);
    assert_eq!(host.logs, vec!["init".to_owned()]);
    // After initialization every import is there.
    module.start(&mut host).expect("start runs");
    assert_eq!(memory_i32(&module, 0), Status::Ok as i32);
    assert_eq!(memory_i32(&module, 24), 42);
    assert_eq!(host.gets, 1);
}

/// For each record: `client_set(ref, 0, kind * 1000 + id)` and
/// `client_log(data, data_len)`; per call `client_set(0, 1, count)`.
const EVENTS: &str = r#"
(module
  (import "vera:client@1.0" "client_set" (func $set (param i64 i32 i64) (result i32)))
  (import "vera:client@1.0" "client_log" (func $log (param i32 i32)))
  (memory (export "memory") 1)
  (@custom "vera.delivery" "\01\00\04\00\00\00\04\00\00")
  (func (export "client_on_events") (param $ptr i32) (param $count i32)
    (local $end i32)
    (drop (call $set (i64.const 0) (i32.const 1) (i64.extend_i32_s (local.get $count))))
    (local.set $end (i32.add (local.get $ptr) (i32.mul (local.get $count) (i32.const 32))))
    (block $done
      (loop $next
        (br_if $done (i32.ge_u (local.get $ptr) (local.get $end)))
        (drop (call $set (i64.load offset=8 (local.get $ptr)) (i32.const 0)
          (i64.extend_i32_s (i32.add
            (i32.mul (i32.load (local.get $ptr)) (i32.const 1000))
            (i32.load offset=16 (local.get $ptr))))))
        (call $log (i32.load offset=24 (local.get $ptr)) (i32.load offset=28 (local.get $ptr)))
        (local.set $ptr (i32.add (local.get $ptr) (i32.const 32)))
        (br $next)))))
"#;

fn event(kind: i32, reference: i64, id: i32, data: &[u8]) -> ClientEvent {
    ClientEvent {
        kind,
        source: Source::Mod,
        reference,
        id,
        value: -1,
        data: data.to_vec(),
    }
}

#[test]
fn events_arrive_with_their_data_in_the_delivery_area() {
    let mut module = load(EVENTS).expect("module loads");
    let mut host = Fake::default();
    let sim_record = [b'x'; 120];
    let mut notify = event(client_event::NOTIFY, 0, 7, b"hello");
    notify.source = Source::Map;
    let events = [
        notify,
        event(client_event::SIM_EVENT, 0, -1, &sim_record),
        event(client_event::CLICK, 99, -1, &[]),
    ];
    module.begin_frame();
    module.events(&mut host, &events).expect("delivery runs");
    assert_eq!(
        host.sets,
        vec![(0, 1, 3), (0, 0, 6007), (0, 0, 6999), (99, 0, -1),]
    );
    assert_eq!(
        host.logs,
        vec!["hello".to_owned(), "x".repeat(120), String::new()]
    );
    // The Notify record names its source.
    let mut source = [0u8; 4];
    assert!(module.read_memory(1024 + 4, &mut source));
    assert_eq!(i32::from_le_bytes(source), Source::Map as i32);
}

#[test]
fn a_batch_holds_what_the_area_holds() {
    // 288 bytes: one record with a full notification per call.
    let wat = EVENTS.replace(
        r"\01\00\04\00\00\00\04\00\00",
        r"\01\00\04\00\00\20\01\00\00",
    );
    let mut module = load(&wat).expect("module loads");
    let mut host = Fake::default();
    let big = [b'n'; 256];
    let events = [
        event(client_event::NOTIFY, 0, 1, &big),
        event(client_event::NOTIFY, 0, 2, &big),
    ];
    module.begin_frame();
    module.events(&mut host, &events).expect("delivery runs");
    let calls: Vec<i64> = host
        .sets
        .iter()
        .filter(|(_, prop, _)| *prop == 1)
        .map(|(_, _, count)| *count)
        .collect();
    assert_eq!(calls, vec![1, 1]);
    // An event that fits no batch is the engine's error, reported.
    let huge = [event(client_event::NOTIFY, 0, 3, &[0u8; 300])];
    assert!(matches!(
        module.events(&mut host, &huge),
        Err(Fault::Invalid(_))
    ));
}

/// Reports each call's record count as `client_set(0, 1, count)`.
const COUNTER: &str = r#"
(module
  (import "vera:client@1.0" "client_set" (func $set (param i64 i32 i64) (result i32)))
  (memory (export "memory") 1)
  (@custom "vera.delivery" "\01\00\04\00\00\00\04\00\00")
  (func (export "client_on_events") (param $ptr i32) (param $count i32)
    (drop (call $set (i64.const 0) (i32.const 1) (i64.extend_i32_s (local.get $count))))))
"#;

#[test]
fn a_frame_delivers_at_most_its_event_limit() {
    let mut module = load(COUNTER).expect("module loads");
    let mut host = Fake::default();
    let clicks = vec![event(client_event::CLICK, 1, -1, &[]); CLIENT_FRAME_EVENTS + 6];
    let delivered = |host: &Fake| host.sets.iter().map(|(_, _, n)| *n).sum::<i64>();
    module.begin_frame();
    module.events(&mut host, &clicks).expect("delivery runs");
    assert_eq!(delivered(&host), CLIENT_FRAME_EVENTS as i64);
    module
        .events(&mut host, &clicks[..5])
        .expect("nothing left to deliver");
    assert_eq!(delivered(&host), CLIENT_FRAME_EVENTS as i64);
    module.begin_frame();
    module
        .events(&mut host, &clicks[..5])
        .expect("delivery runs");
    assert_eq!(delivered(&host), CLIENT_FRAME_EVENTS as i64 + 5);
}

/// `client_on_render`: reports `alpha * 1000` as `client_set(0, 0, ..)`, then
/// spins `dt` times (five operations a turn).
const RENDER: &str = r#"
(module
  (import "vera:client@1.0" "client_set" (func $set (param i64 i32 i64) (result i32)))
  (func (export "client_on_render") (param $dt f32) (param $alpha f32)
    (local $n i32)
    (drop (call $set (i64.const 0) (i32.const 0)
      (i64.trunc_f32_s (f32.mul (local.get $alpha) (f32.const 1000)))))
    (local.set $n (i32.trunc_f32_s (local.get $dt)))
    (if (i32.gt_s (local.get $n) (i32.const 0))
      (then (loop $spin
        (br_if $spin (local.tee $n (i32.sub (local.get $n) (i32.const 1)))))))))
"#;

#[test]
fn render_takes_floats_and_shares_the_frame_budget() {
    let mut module = load(RENDER).expect("module loads");
    let mut host = Fake::default();
    module.begin_frame();
    // About 60,000 of the frame's 100,000 operations.
    module
        .render(&mut host, 12_000.0, 0.25)
        .expect("render runs");
    assert_eq!(host.sets, vec![(0, 0, 250)]);
    assert_eq!(module.render(&mut host, 12_000.0, 0.5), Err(Fault::Budget));
    module.begin_frame();
    module
        .render(&mut host, 12_000.0, 0.75)
        .expect("a new frame has a new budget");
    module.begin_frame();
    assert_eq!(module.render(&mut host, 1.0e9, 0.0), Err(Fault::Budget));
}

const IMPORTS: &str = r#"
(module
  (import "vera:client@1.0" "client_random" (func $random (param i32 i32 i32) (result i32)))
  (import "vera:client@1.0" "client_node_create" (func $create (param i32 i64 i32 i32 i32) (result i32)))
  (import "vera:client@1.0" "client_action" (func $action (param i32 i64 i32 i32) (result i32)))
  (import "vera:client@1.0" "client_send_message" (func $message (param i32 i32) (result i32)))
  (import "vera:client@1.0" "client_storage_read" (func $read (param i32 i32 i32 i32 i32) (result i32)))
  (import "vera:client@1.0" "client_storage_write" (func $write (param i32 i32 i32 i32) (result i32)))
  (memory (export "memory") 1)
  ;; PropValues at 64: Visible = 1, X = 10.
  (data (i32.const 64) "\9c\00\00\00\00\00\00\00\01\00\00\00\00\00\00\00")
  (data (i32.const 80) "\06\00\00\00\00\00\00\00\0a\00\00\00\00\00\00\00")
  (data (i32.const 400) "Buy")
  (data (i32.const 408) "Ctrl+K")
  (data (i32.const 420) "kz")
  (func (export "client_on_start")
    (local $i i32)
    ;; 100 draws in [5, 9], then one in [3, 3].
    (local.set $i (i32.const 100))
    (loop $draw
      (drop (call $random (i32.const 5) (i32.const 9) (i32.const 0)))
      (if (i32.or (i32.lt_s (i32.load (i32.const 0)) (i32.const 5))
                  (i32.gt_s (i32.load (i32.const 0)) (i32.const 9)))
        (then unreachable))
      (br_if $draw (local.tee $i (i32.sub (local.get $i) (i32.const 1)))))
    (drop (call $random (i32.const 3) (i32.const 3) (i32.const 0)))
    ;; A Text node at the top level with two properties; handle at 8.
    (i32.store (i32.const 4) (call $create (i32.const 0) (i64.const 0) (i32.const 64) (i32.const 32) (i32.const 8)))
    ;; RegisterHotkey with text "Buy" and extra "Ctrl+K" (ActionParams at 200).
    (i32.store (i32.const 256) (i32.const 400))
    (i32.store (i32.const 260) (i32.const 3))
    (i32.store (i32.const 264) (i32.const 408))
    (i32.store (i32.const 268) (i32.const 6))
    (drop (call $action (i32.const 8) (i64.const 0) (i32.const 200) (i32.const 72)))
    (drop (call $message (i32.const 400) (i32.const 3)))
    ;; Storage: write "k" = "Buy", read it with capacity 2 and 16, read "z".
    (drop (call $write (i32.const 420) (i32.const 1) (i32.const 400) (i32.const 3)))
    (i32.store (i32.const 20) (call $read (i32.const 420) (i32.const 1) (i32.const 500) (i32.const 2) (i32.const 24)))
    (i32.store (i32.const 28) (call $read (i32.const 420) (i32.const 1) (i32.const 508) (i32.const 16) (i32.const 32)))
    (i32.store (i32.const 36) (call $read (i32.const 421) (i32.const 1) (i32.const 516) (i32.const 16) (i32.const 40))))
  (func (export "client_on_render") (param f32 f32)
    (drop (call $random (i32.const 2) (i32.const 1) (i32.const 0)))))
"#;

#[test]
fn the_client_imports_reach_the_host() {
    let mut module = load(IMPORTS).expect("module loads");
    let mut host = Fake::default();
    module.start(&mut host).expect("start runs");
    assert_eq!(memory_i32(&module, 0), 3);
    // The node.
    assert_eq!(memory_i32(&module, 4), Status::Ok as i32);
    let mut handle = [0u8; 8];
    assert!(module.read_memory(8, &mut handle));
    assert_eq!(i64::from_le_bytes(handle), 5 << 48 | 1);
    let visible = property_id("Visible").expect("Visible is a property");
    assert_eq!(
        host.created,
        vec![(
            0,
            0,
            vec![
                NodeInit {
                    prop: visible,
                    value: 1
                },
                NodeInit { prop: 6, value: 10 },
            ]
        )]
    );
    // The action with its text and extra bytes, the message.
    assert_eq!(
        host.actions,
        vec![(
            client_action::REGISTER_HOTKEY,
            0,
            "Buy".to_owned(),
            b"Ctrl+K".to_vec()
        )]
    );
    assert_eq!(host.messages, vec![b"Buy".to_vec()]);
    // Storage: too small a buffer gets the length only; a missing key -1.
    assert_eq!(memory_i32(&module, 20), Status::Ok as i32);
    assert_eq!(memory_i32(&module, 24), 3);
    assert_eq!(memory_i32(&module, 500), 0);
    assert_eq!(memory_i32(&module, 28), Status::Ok as i32);
    let mut value = [0u8; 3];
    assert!(module.read_memory(508, &mut value));
    assert_eq!(&value, b"Buy");
    assert_eq!(memory_i32(&module, 36), Status::NotFound as i32);
    assert_eq!(memory_i32(&module, 40), -1);
    // `lo > hi` is an invalid call.
    module.begin_frame();
    assert!(matches!(
        module.render(&mut host, 0.0, 0.0),
        Err(Fault::Invalid(_))
    ));
}

const BATTLEFIELD_NODES: &str = r#"
(module
  (import "vera:client@1.0" "client_node_create" (func $create (param i32 i64 i32 i32 i32) (result i32)))
  (import "vera:client@1.0" "client_status" (func $status (param i32 i32) (result i32)))
  (memory (export "memory") 1)
  (data (i32.const 100) "Name=Hp")
  (func (export "client_on_start")
    ;; A Decoration (8) and a MapMarker (9) at the top level.
    (i32.store (i32.const 0) (call $create (i32.const 8) (i64.const 0) (i32.const 0) (i32.const 0) (i32.const 16)))
    (i32.store (i32.const 4) (call $create (i32.const 9) (i64.const 0) (i32.const 0) (i32.const 0) (i32.const 24)))
    (i32.store (i32.const 8) (call $status (i32.const 100) (i32.const 7))))
  (func (export "client_on_render") (param f32 f32)))
"#;

/// The 0.2.0 node kinds `Decoration` and `MapMarker` reach the host, and
/// `client_status` hands it the template text.
#[test]
fn decorations_markers_and_status_templates_reach_the_host() {
    let mut module = load(BATTLEFIELD_NODES).expect("module loads");
    let mut host = Fake::default();
    module.start(&mut host).expect("start runs");
    assert_eq!(memory_i32(&module, 0), Status::Ok as i32);
    assert_eq!(memory_i32(&module, 4), Status::Ok as i32);
    assert_eq!(memory_i32(&module, 8), Status::Ok as i32);
    let kinds: Vec<i32> = host.created.iter().map(|(kind, _, _)| *kind).collect();
    assert_eq!(kinds, [8, 9]);
    assert_eq!(host.logs, ["Name=Hp"]);
}
