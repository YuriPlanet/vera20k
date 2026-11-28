//! `on_decide`: the record a handler reads, its answer, and a write that
//! stops the module.

use crate::decide::{
    DECIDE_BUDGET, DecideRecord, EVENT_LAUNCH, EVENT_MISSION_STEP, EVENT_MOVE_FRAME,
    EVENT_PATH_FOUND, MAX_PATH_CELLS, MissionStepResult, MoveStep, PathCell, PathSpec,
    ProjectileSpec, ProjectileStep, ResultArea, result_area_bytes, result_len,
};
use crate::triggers::{EVENT_FIRE, EVENT_TAKE_DAMAGE, EventRecord};
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
        .decide(&record(1), &mut ResultArea::none(), DECIDE_BUDGET, &mut host)
        .expect("on_decide runs");
    assert_eq!(answer, Some(20));
    assert!(used > 0);
    assert_eq!(&module.memory()[256..416], &record(1).to_bytes(0, 0, 0));
    // `handled` left 0 keeps the previous result.
    let (answer, _) = module
        .decide(&record(2), &mut ResultArea::none(), DECIDE_BUDGET, &mut host)
        .expect("on_decide runs");
    assert_eq!(answer, None);
}

#[test]
fn a_write_in_on_decide_is_an_invalid_call() {
    let mut module = handler();
    let mut host = Host::default();
    let result = module.decide(&record(3), &mut ResultArea::none(), DECIDE_BUDGET, &mut host);
    assert!(matches!(result, Err(Fault::Invalid(_))), "{result:?}");
    assert_eq!(host.var_sets, 0, "the write never reached the host");
}

/// A handler with a 1024-byte delivery area at 1024: it copies the record it
/// got to 256, writes 7 into the first byte of its result area and answers 1.
const AREA_HANDLER: &str = r#"(module
  (@custom "vera.delivery" "\01\00\04\00\00\00\04\00\00")
  (memory (export "memory") 1)
  (func (export "on_decide") (param $ptr i32) (param $count i32)
    (memory.copy (i32.const 256) (local.get $ptr) (i32.const 160))
    (i32.store8 (i32.load offset=148 (local.get $ptr)) (i32.const 7))
    (i64.store offset=136 (local.get $ptr) (i64.const 1))
    (i32.store offset=144 (local.get $ptr) (i32.const 1))))"#;

#[test]
fn the_result_area_follows_the_record_with_the_previous_bytes() {
    let bytes = wat::parse_str(AREA_HANDLER).expect("test module parses");
    let mut module = SimModule::load(&bytes, Limits::MAP).expect("module loads");
    let mut area = ResultArea::of(EVENT_FIRE, vec![1, 2, 3]);
    let (answer, _) = module
        .decide(&record(0), &mut area, DECIDE_BUDGET, &mut Host::default())
        .expect("on_decide runs");
    assert_eq!(answer, Some(1));
    assert_eq!(area.cap, 768);
    assert_eq!(area.bytes.len(), 768);
    assert_eq!(&area.bytes[..4], &[7, 2, 3, 0]);
    let seen = &module.memory()[256..416];
    let field = |at: usize| i32::from_le_bytes(seen[at..at + 4].try_into().unwrap());
    assert_eq!((field(148), field(152), field(156)), (1024 + 160, 768, 3));
    assert_eq!(result_len(EVENT_FIRE, 1, area.cap), Some(48));
    assert_eq!(result_len(EVENT_FIRE, 17, area.cap), None);
}

#[test]
fn fixed_layouts_round_trip() {
    let spec = ProjectileSpec {
        projectile_type: 3,
        weapon: -1,
        target: 1 << 49,
        data: [9; 16],
        flags: ProjectileSpec::ABSOLUTE_ORIGIN,
        origin: [1, -2, 3],
    };
    assert_eq!(ProjectileSpec::from_bytes(&spec.to_bytes()), Some(spec));
    let path = PathSpec {
        kind: 3,
        speed: 5 << 16,
        param: [7, 8],
        points: vec![[1, 2, 3], [4, 5, 6]],
    };
    let bytes = path.to_bytes();
    assert_eq!(result_len(EVENT_LAUNCH, bytes.len() as i64, 416), Some(80));
    assert_eq!(PathSpec::from_bytes(&bytes, bytes.len()), Some(path));
    let over_limit = PathSpec {
        points: vec![[0; 3]; crate::decide::MAX_PATH_POINTS + 1],
        ..PathSpec::default()
    }
    .to_bytes();
    assert_eq!(PathSpec::from_bytes(&over_limit, over_limit.len()), None);
    let step = ProjectileStep {
        flags: ProjectileStep::SET_VELOCITY | ProjectileStep::DETONATE,
        velocity: [65_536, 0, -1],
        position: [4, 5, 6],
        data: [1; 16],
    };
    assert_eq!(ProjectileStep::from_bytes(&step.to_bytes()), Some(step));
    let mission = MissionStepResult {
        target: 7,
        destination: (2 << 48) | (3 << 16) | 4,
        mission: MissionStepResult::NO_MISSION,
        delay: 30,
    };
    assert_eq!(MissionStepResult::from_bytes(&mission.to_bytes()), Some(mission));
    assert_eq!(result_area_bytes(EVENT_MISSION_STEP), 24);
    assert_eq!(result_len(EVENT_MISSION_STEP, 0, 24), Some(24));
    assert_eq!(result_len(EVENT_MISSION_STEP, 0, 16), None);
    let move_step = MoveStep {
        flags: MoveStep::MOVE | MoveStep::FACING,
        facing: 64,
        displacement: [-32, 48],
    };
    assert_eq!(MoveStep::from_bytes(&move_step.to_bytes()), Some(move_step));
    assert_eq!(result_area_bytes(EVENT_MOVE_FRAME), 16);
    assert_eq!(result_len(EVENT_MOVE_FRAME, 0, 16), Some(16));
    let cells = [PathCell { x: 3, y: -4 }, PathCell { x: 4, y: -4 }];
    let bytes: Vec<u8> = cells.iter().flat_map(PathCell::to_bytes).collect();
    assert_eq!(PathCell::read_all(&bytes, 2), Some(cells.to_vec()));
    assert_eq!(PathCell::read_all(&bytes, 3), None);
    assert_eq!(result_area_bytes(EVENT_PATH_FOUND), 192);
    assert_eq!(result_len(EVENT_PATH_FOUND, 0, 192), Some(0));
    assert_eq!(result_len(EVENT_PATH_FOUND, 2, 192), Some(16));
    assert_eq!(result_len(EVENT_PATH_FOUND, MAX_PATH_CELLS as i64 + 1, 192), None);
    assert_eq!(result_len(EVENT_PATH_FOUND, -1, 192), None);
}
