//! Client messages (`on_message`) and notifications (`send_notify`,
//! `vera.notify`, `lookup(Notify)`): RFC 0001 section 10, appendix 1.1 #22,
//! 1.2, 8.1.

use crate::sections::{Sections, notify_collision, notify_id};
use crate::{
    ActionParams, Export, Fault, FindPage, Invalid, Limits, LoadError, MAX_NOTIFY_BYTES, Query,
    SimHost, SimModule, Status,
};

#[derive(Default)]
struct Notifier {
    sent: Vec<(i64, i32, Vec<u8>)>,
}

impl SimHost for Notifier {
    fn frame(&mut self) -> i32 {
        0
    }
    fn random(&mut self, lo: i32, _hi: i32) -> i32 {
        lo
    }
    fn log(&mut self, _line: &str) {}
    fn has_kind(&mut self, _table: i32, _id: i32) -> bool {
        false
    }
    fn lookup(&mut self, _kind: i32, _name: &str) -> Result<(i32, Status), Invalid> {
        Ok((-1, Status::Unsupported))
    }
    fn name_of(&mut self, _kind: i32, _id: i32) -> Result<(String, Status), Invalid> {
        Ok((String::new(), Status::Unsupported))
    }
    fn get(&mut self, _reference: i64, _prop: i32) -> Result<(i64, Status), Invalid> {
        Ok((0, Status::Unsupported))
    }
    fn set(&mut self, _reference: i64, _prop: i32, _value: i64) -> Result<Status, Invalid> {
        Ok(Status::Unsupported)
    }
    fn find(&mut self, _query: &Query, _cap: usize) -> Result<(FindPage, Status), Invalid> {
        Ok((FindPage::default(), Status::Unsupported))
    }
    fn var_get(&mut self, _scope: i64, _var: i32) -> Result<(i64, Status), Invalid> {
        Ok((0, Status::Unsupported))
    }
    fn var_set(&mut self, _scope: i64, _var: i32, _value: i64) -> Result<Status, Invalid> {
        Ok(Status::Unsupported)
    }
    fn action(
        &mut self,
        _kind: i32,
        _target: i64,
        _params: &ActionParams,
        _text: &str,
    ) -> Result<(i64, Status), Invalid> {
        Ok((0, Status::Unsupported))
    }
    fn send_notify(&mut self, to: i64, id: i32, data: &[u8]) -> Result<Status, Invalid> {
        self.sent.push((to, id, data.to_vec()));
        Ok(Status::Ok)
    }
}

fn escape(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("\\{b:02x}")).collect()
}

/// A `vera.notify` section with `names`.
fn notify_section(names: &[&str]) -> Vec<u8> {
    let mut bytes = vec![1];
    bytes.extend_from_slice(&(names.len() as u32).to_le_bytes());
    for name in names {
        bytes.extend_from_slice(&(name.len() as u16).to_le_bytes());
        bytes.extend_from_slice(name.as_bytes());
    }
    bytes
}

/// A `vera.delivery` section: 240 bytes at 4096.
fn delivery_section() -> Vec<u8> {
    let mut bytes = vec![1];
    bytes.extend_from_slice(&4096u32.to_le_bytes());
    bytes.extend_from_slice(&240u32.to_le_bytes());
    bytes
}

fn load(wat: &str) -> Result<SimModule, LoadError> {
    SimModule::load(&wat::parse_str(wat).expect("test WAT parses"), Limits::MAP)
}

fn mem_i32(module: &SimModule, at: usize) -> i32 {
    let mut bytes = [0u8; 4];
    assert!(module.read_memory(at, &mut bytes));
    i32::from_le_bytes(bytes)
}

fn mem_i64(module: &SimModule, at: usize) -> i64 {
    let mut bytes = [0u8; 8];
    assert!(module.read_memory(at, &mut bytes));
    i64::from_le_bytes(bytes)
}

#[test]
fn notification_ids_are_fnv1a_of_the_ascii_lowercased_name() {
    // FNV-1a 32 test vectors.
    assert_eq!(notify_id(""), 0x811C_9DC5u32 as i32);
    assert_eq!(notify_id("a"), 0xE40C_292Cu32 as i32);
    assert_eq!(notify_id("foobar"), 0xBF9C_F968u32 as i32);
    // Only A..Z fold.
    assert_eq!(notify_id("FooBar"), notify_id("foobar"));
    assert_ne!(notify_id("ä"), notify_id("Ä"));
    // A name hashing to -1, and two names sharing an ID.
    assert_eq!(notify_id("hqduatcd"), -1);
    assert_eq!(notify_id("glbvs"), notify_id("yacxa"));
}

#[test]
fn vera_notify_is_read_and_checked_at_load() {
    let module = |names: &[&str]| {
        wat::parse_str(format!(
            r#"(module (@custom "vera.notify" "{}") (memory (export "memory") 1))"#,
            escape(&notify_section(names))
        ))
        .expect("test WAT parses")
    };
    let sections = Sections::read(&module(&["Bought", "WaveStarted"])).expect("section reads");
    assert_eq!(
        sections.notify,
        vec![
            ("Bought".to_owned(), notify_id("bought")),
            ("WaveStarted".to_owned(), notify_id("wavestarted")),
        ]
    );
    assert_eq!(sections.notify_lookup("BOUGHT"), Some(notify_id("bought")));
    assert_eq!(sections.notify_lookup("Sold"), None);
    // A name hashing to -1, two names with one ID, a repeated or empty
    // name: the module does not load.
    for names in [
        &["hqduatcd"][..],
        &["glbvs", "yacxa"],
        &["Bought", "BOUGHT"],
        &[""],
    ] {
        assert!(
            matches!(
                SimModule::load(&module(names), Limits::MAP),
                Err(LoadError::Invalid(_))
            ),
            "{names:?}"
        );
    }
    // Across modules (mod and map): different names with one ID collide,
    // the same name does not.
    let a = vec![("glbvs".to_owned(), notify_id("glbvs"))];
    let b = vec![("yacxa".to_owned(), notify_id("yacxa"))];
    let same = vec![("GLBVS".to_owned(), notify_id("glbvs"))];
    assert_eq!(notify_collision(&b, &a), Some(("yacxa", "glbvs")));
    assert_eq!(notify_collision(&same, &a), None);
}

/// Declares `Bought`; `on_frame` looks it up into 0, then sends it to house
/// reference 5 with 3 bytes (status at 8), an undeclared ID (12), 300 bytes
/// (16), and names it back (`name_of` into 64, length at 20).
fn notifier() -> String {
    format!(
        r#"(module
  (import "vera:sim@1.0" "lookup" (func $lookup (param i32 i32 i32 i32) (result i32)))
  (import "vera:sim@1.0" "name_of" (func $name_of (param i32 i32 i32 i32 i32) (result i32)))
  (import "vera:sim@1.0" "send_notify" (func $send (param i64 i32 i32 i32) (result i32)))
  (@custom "vera.notify" "{notify}")
  (memory (export "memory") 1)
  (data (i32.const 1024) "BOUGHT")
  (data (i32.const 1100) "abc")
  (func (export "vera_init")
    (i32.store (i32.const 24)
      (call $send (i64.const 5) (i32.const 0) (i32.const 1100) (i32.const 3))))
  (func (export "on_frame")
    (drop (call $lookup (i32.const 16) (i32.const 1024) (i32.const 6) (i32.const 0)))
    (i32.store (i32.const 8)
      (call $send (i64.const 5) (i32.load (i32.const 0)) (i32.const 1100) (i32.const 3)))
    (i32.store (i32.const 12)
      (call $send (i64.const 5) (i32.const 12345) (i32.const 1100) (i32.const 3)))
    (i32.store (i32.const 16)
      (call $send (i64.const 0) (i32.load (i32.const 0)) (i32.const 2000) (i32.const 300)))
    (drop (call $name_of (i32.const 16) (i32.load (i32.const 0)) (i32.const 64) (i32.const 32)
      (i32.const 20)))))"#,
        notify = escape(&notify_section(&["Bought"])),
    )
}

#[test]
fn send_notify_sends_declared_names_within_the_size_limit() {
    let mut module = load(&notifier()).expect("module loads");
    let mut host = Notifier::default();
    // Unavailable in the initialization function.
    module
        .call(Export::Init, 100_000, &mut host)
        .expect("init runs");
    assert_eq!(mem_i32(&module, 24), Status::Unsupported as i32);
    module
        .call(Export::Frame, 100_000, &mut host)
        .expect("frame runs");
    let id = notify_id("bought");
    assert_eq!(mem_i32(&module, 0), id);
    assert_eq!(mem_i32(&module, 8), Status::Ok as i32);
    assert_eq!(mem_i32(&module, 12), Status::NotFound as i32);
    assert_eq!(mem_i32(&module, 16), Status::Refused as i32);
    assert!(300 > MAX_NOTIFY_BYTES);
    assert_eq!(host.sent, vec![(5, id, b"abc".to_vec())]);
    assert_eq!(mem_i32(&module, 20), 6);
    let mut name = [0u8; 6];
    assert!(module.read_memory(64, &mut name));
    assert_eq!(&name, b"Bought");
}

#[test]
fn send_notify_is_a_write_that_on_decide_may_not_make() {
    let module = format!(
        r#"(module
  (import "vera:sim@1.0" "send_notify" (func $send (param i64 i32 i32 i32) (result i32)))
  (@custom "vera.notify" "{notify}")
  (@custom "vera.delivery" "{delivery}")
  (memory (export "memory") 1)
  (func (export "on_decide") (param i32 i32)
    (drop (call $send (i64.const 0) (i32.const {id}) (i32.const 0) (i32.const 0)))))"#,
        notify = escape(&notify_section(&["Bought"])),
        delivery = escape(&delivery_section()),
        id = notify_id("bought"),
    );
    let mut module = load(&module).expect("module loads");
    let record = crate::DecideRecord {
        event: crate::EventRecord::empty(crate::triggers::EVENT_TAKE_DAMAGE),
        native: 0,
        previous: 0,
    };
    let mut result = crate::decide::ResultArea::none();
    let outcome = module.decide(&record, &mut result, 20_000, &mut Notifier::default());
    assert!(matches!(outcome, Err(Fault::Invalid(_))), "{outcome:?}");
}

/// `on_message` keeps the sender at 0, the length at 8 and the first and
/// last bytes at 12 and 13.
fn receiver() -> String {
    format!(
        r#"(module
  (@custom "vera.delivery" "{delivery}")
  (memory (export "memory") 1)
  (func (export "on_message") (param $sender i64) (param $ptr i32) (param $len i32)
    (i64.store (i32.const 0) (local.get $sender))
    (i32.store (i32.const 8) (local.get $len))
    (i32.store8 (i32.const 12) (i32.load8_u (local.get $ptr)))
    (i32.store8 (i32.const 13)
      (i32.load8_u (i32.add (local.get $ptr) (i32.sub (local.get $len) (i32.const 1)))))))"#,
        delivery = escape(&delivery_section()),
    )
}

#[test]
fn a_message_reaches_on_message_through_the_delivery_area() {
    let mut module = load(&receiver()).expect("module loads");
    assert!(module.has_export(Export::Message));
    let sender = (1i64 << 47) | 3;
    let mut data = vec![7u8; 96];
    data[95] = 9;
    let used = module
        .deliver_message(sender, &data, 100_000, &mut Notifier::default())
        .expect("message delivered");
    assert!(used > 0);
    assert_eq!(mem_i64(&module, 0), sender);
    assert_eq!(mem_i32(&module, 8), 96);
    let mut ends = [0u8; 2];
    assert!(module.read_memory(12, &mut ends));
    assert_eq!(ends, [7, 9]);
    // Longer than a message may be: the engine's error, no call.
    assert!(matches!(
        module.deliver_message(sender, &[0; 97], 100_000, &mut Notifier::default()),
        Err(Fault::Invalid(_))
    ));
    // Without the export nothing runs.
    let mut silent = load(r#"(module (memory (export "memory") 1))"#).expect("module loads");
    assert_eq!(
        silent.deliver_message(sender, b"x", 100_000, &mut Notifier::default()),
        Ok(0)
    );
}

#[test]
fn on_message_needs_its_signature_and_a_delivery_area() {
    let wrong = format!(
        r#"(module
  (@custom "vera.delivery" "{delivery}")
  (memory (export "memory") 1)
  (func (export "on_message") (param i32 i32 i32)))"#,
        delivery = escape(&delivery_section()),
    );
    assert!(matches!(load(&wrong), Err(LoadError::Invalid(_))));
    let no_area = r#"(module
  (memory (export "memory") 1)
  (func (export "on_message") (param i64 i32 i32)))"#;
    assert!(matches!(load(no_area), Err(LoadError::Invalid(_))));
}
