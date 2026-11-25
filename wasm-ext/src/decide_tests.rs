//! `on_decide`: the record a handler reads, its answer, and a write that
//! stops the module.

use crate::decide::{DECIDE_BUDGET, DecideRecord};
use crate::triggers::{EVENT_TAKE_DAMAGE, EventRecord};
use crate::{ActionParams, Fault, FindPage, Invalid, Limits, Query, SimHost, SimModule, Status};

/// Records `var_set` calls; every read answers empty.
#[derive(Default)]
struct Host {
    var_sets: usize,
}

impl SimHost for Host {
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
        Ok((-1, Status::NotFound))
    }
    fn name_of(&mut self, _kind: i32, _id: i32) -> Result<(String, Status), Invalid> {
        Ok((String::new(), Status::NotFound))
    }
    fn get(&mut self, _reference: i64, _prop: i32) -> Result<(i64, Status), Invalid> {
        Ok((0, Status::Gone))
    }
    fn set(&mut self, _reference: i64, _prop: i32, _value: i64) -> Result<Status, Invalid> {
        Ok(Status::Refused)
    }
    fn find(&mut self, _query: &Query, _cap: usize) -> Result<(FindPage, Status), Invalid> {
        Ok((FindPage::default(), Status::Unsupported))
    }
    fn var_get(&mut self, _scope: i64, _var: i32) -> Result<(i64, Status), Invalid> {
        Ok((0, Status::Ok))
    }
    fn var_set(&mut self, _scope: i64, _var: i32, _value: i64) -> Result<Status, Invalid> {
        self.var_sets += 1;
        Ok(Status::Ok)
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
}

/// A handler with a 160-byte delivery area at 1024. With key 1 it halves
/// `native` into `result` and sets `handled`; with key 2 it leaves the
/// record alone; with key 3 it calls `var_set`. It copies the record it got
/// to 256.
const HANDLER: &str = r#"(module
  (import "vera:sim@1.0" "var_set" (func $var_set (param i64 i32 i64) (result i32)))
  (@custom "vera.delivery" "\01\00\04\00\00\a0\00\00\00")
  (memory (export "memory") 1)
  (func (export "on_decide") (param $ptr i32) (param $count i32)
    (memory.copy (i32.const 256) (local.get $ptr) (i32.const 160))
    (if (i32.eq (i32.load (local.get $ptr)) (i32.const 1))
      (then
        (i64.store offset=136 (local.get $ptr)
          (i64.div_s (i64.load offset=120 (local.get $ptr)) (i64.const 2)))
        (i32.store offset=144 (local.get $ptr) (i32.const 1))))
    (if (i32.eq (i32.load (local.get $ptr)) (i32.const 3))
      (then (drop (call $var_set (i64.const 0) (i32.const 3) (i64.const 1)))))))"#;

fn record(key: i32) -> DecideRecord {
    DecideRecord {
        event: EventRecord {
            key,
            value: 40,
            ..EventRecord::empty(EVENT_TAKE_DAMAGE)
        },
        native: 40,
        previous: 30,
    }
}

fn handler() -> SimModule {
    let bytes = wat::parse_str(HANDLER).expect("test module parses");
    SimModule::load(&bytes, Limits::MAP).expect("module loads")
}

#[test]
fn a_handler_reads_the_record_in_place_and_answers_through_handled() {
    let mut module = handler();
    let mut host = Host::default();
    let (answer, used) = module
        .decide(&record(1), DECIDE_BUDGET, &mut host)
        .expect("on_decide runs");
    assert_eq!(answer, Some(20));
    assert!(used > 0);
    assert_eq!(&module.memory()[256..416], &record(1).to_bytes());
    // `handled` left 0 keeps the previous result.
    let (answer, _) = module
        .decide(&record(2), DECIDE_BUDGET, &mut host)
        .expect("on_decide runs");
    assert_eq!(answer, None);
}

#[test]
fn a_write_in_on_decide_is_an_invalid_call() {
    let mut module = handler();
    let mut host = Host::default();
    let result = module.decide(&record(3), DECIDE_BUDGET, &mut host);
    assert!(matches!(result, Err(Fault::Invalid(_))), "{result:?}");
    assert_eq!(host.var_sets, 0, "the write never reached the host");
}
