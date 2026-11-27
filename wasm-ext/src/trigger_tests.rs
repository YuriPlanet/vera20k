//! `TriggerDef` parsing, the custom sections and `on_events` delivery.

use crate::triggers::{ACTION_MODULE, EVENT_TAKE_DAMAGE, EventRecord, Repeat, TriggerDef};
use crate::{Export, Fault, FindPage, Invalid, Limits, Query, SimHost, SimModule, Status};

/// A trigger: house 0, repeating, all difficulties, one `TakeDamage` event
/// with one Subject clause, one `Module` action with key 7.
fn trigger_bytes() -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(&0i64.to_le_bytes()); // house
    out.extend_from_slice(&2i32.to_le_bytes()); // repeat
    out.extend_from_slice(&7i32.to_le_bytes()); // difficulty
    out.extend_from_slice(&0i32.to_le_bytes()); // flags
    out.extend_from_slice(&(-1i32).to_le_bytes()); // decide_key
    out.extend_from_slice(&1i32.to_le_bytes()); // event_count
    out.extend_from_slice(&1i32.to_le_bytes()); // action_count
    out.extend_from_slice(&EVENT_TAKE_DAMAGE.to_le_bytes());
    out.extend_from_slice(&[0; 8]); // op, interval
    out.extend_from_slice(&(-1i32).to_le_bytes()); // id
    out.extend_from_slice(&0i64.to_le_bytes()); // value
    out.extend_from_slice(&1i32.to_le_bytes()); // clause_count
    out.extend_from_slice(&0i32.to_le_bytes()); // reserved
    out.extend_from_slice(&[0, 2, 0, 0]); // Subject, Lt
    out.extend_from_slice(&5i32.to_le_bytes()); // HealthPercent
    out.extend_from_slice(&30i64.to_le_bytes());
    out.extend_from_slice(&ACTION_MODULE.to_le_bytes());
    out.extend_from_slice(&0i32.to_le_bytes()); // target Subject
    let mut params = [0u8; 72];
    params[..4].copy_from_slice(&7i32.to_le_bytes());
    out.extend_from_slice(&params);
    out
}

#[test]
fn a_trigger_definition_parses_and_its_breaches_are_invalid() {
    let bytes = trigger_bytes();
    let def = TriggerDef::parse(&bytes).expect("valid definition");
    assert_eq!(def.repeat, Repeat::Repeating);
    assert_eq!(def.events.len(), 1);
    assert_eq!(def.events[0].clauses.len(), 1);
    assert!(def.events[0].clauses[0].holds(29));
    assert!(!def.events[0].clauses[0].holds(30));
    assert_eq!(def.actions[0].params.id, 7);
    // A trailing byte, a short buffer and an unknown repeat mode.
    let mut trailing = bytes.clone();
    trailing.push(0);
    assert!(TriggerDef::parse(&trailing).is_err());
    assert!(TriggerDef::parse(&bytes[..bytes.len() - 1]).is_err());
    let mut repeat = bytes.clone();
    repeat[8] = 3;
    assert!(TriggerDef::parse(&repeat).is_err());
}

/// Records each `trigger_create` and refuses nothing else.
#[derive(Default)]
struct Triggers {
    created: Vec<TriggerDef>,
}

impl SimHost for Triggers {
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
        Ok(Status::Ok)
    }
    fn action(
        &mut self,
        _kind: i32,
        _target: i64,
        _params: &crate::ActionParams,
        _text: &str,
    ) -> Result<(i64, Status), Invalid> {
        Ok((0, Status::Unsupported))
    }
    fn trigger_create(&mut self, def: &TriggerDef) -> Result<(i64, Status), Invalid> {
        self.created.push(def.clone());
        Ok((99, Status::Ok))
    }
}

fn wat_bytes(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("\\{b:02x}")).collect()
}

/// `vera.actions` names `Panic` (key 7); `vera.delivery` is 240 bytes at
/// 1024. `on_scenario_start` looks the name up into 64 and creates the
/// trigger from 2048; `on_events` sums each record's `value` into 80 and
/// counts records into 88.
fn module(actions: &[u8], delivery: &[u8]) -> Vec<u8> {
    let def = trigger_bytes();
    wat::parse_str(format!(
        r#"(module
  (import "vera:sim@1.0" "lookup" (func $lookup (param i32 i32 i32 i32) (result i32)))
  (import "vera:sim@1.0" "trigger_create" (func $create (param i32 i32 i32) (result i32)))
  (@custom "vera.actions" "{actions}")
  (@custom "vera.delivery" "{delivery}")
  (memory (export "memory") 1)
  (data (i32.const 0) "panic")
  (data (i32.const 2048) "{def}")
  (func (export "on_scenario_start")
    (drop (call $lookup (i32.const 17) (i32.const 0) (i32.const 5) (i32.const 64)))
    (drop (call $create (i32.const 2048) (i32.const {len}) (i32.const 72))))
  (func (export "on_events") (param $ptr i32) (param $count i32)
    (loop $l
      (i64.store (i32.const 80) (i64.add (i64.load (i32.const 80))
        (i64.load offset=72 (local.get $ptr))))
      (i32.store (i32.const 88) (i32.add (i32.load (i32.const 88)) (i32.const 1)))
      (local.set $ptr (i32.add (local.get $ptr) (i32.const 120)))
      (br_if $l (local.tee $count (i32.sub (local.get $count) (i32.const 1)))))))"#,
        actions = wat_bytes(actions),
        delivery = wat_bytes(delivery),
        def = wat_bytes(&def),
        len = def.len(),
    ))
    .expect("test module parses")
}

fn actions(names: &[(&str, i32)]) -> Vec<u8> {
    let mut out = vec![1];
    out.extend_from_slice(&(names.len() as u32).to_le_bytes());
    for (name, key) in names {
        out.extend_from_slice(&(name.len() as u16).to_le_bytes());
        out.extend_from_slice(name.as_bytes());
        out.extend_from_slice(&key.to_le_bytes());
    }
    out
}

fn delivery(offset: u32, bytes: u32) -> Vec<u8> {
    let mut out = vec![1];
    out.extend_from_slice(&offset.to_le_bytes());
    out.extend_from_slice(&bytes.to_le_bytes());
    out
}

#[test]
fn a_module_names_its_actions_creates_a_trigger_and_receives_records() {
    let bytes = module(&actions(&[("Panic", 7)]), &delivery(1024, 240));
    let mut module = SimModule::load(&bytes, Limits::MAP).expect("module loads");
    assert_eq!(module.batch_capacity(), 2);
    let mut host = Triggers::default();
    module
        .call(Export::ScenarioStart, 100_000, &mut host)
        .expect("call succeeds");
    assert_eq!(host.created.len(), 1);
    let memory = &module.memory();
    assert_eq!(&memory[64..68], &7i32.to_le_bytes(), "lookup(ModuleAction)");
    assert_eq!(&memory[72..80], &99i64.to_le_bytes(), "trigger handle");
    let records = [
        EventRecord {
            value: 40,
            ..EventRecord::empty(EVENT_TAKE_DAMAGE)
        },
        EventRecord {
            value: 2,
            ..EventRecord::empty(EVENT_TAKE_DAMAGE)
        },
    ];
    module
        .deliver(&records, 100_000, &mut host)
        .expect("on_events runs");
    let memory = &module.memory();
    assert_eq!(&memory[80..88], &42i64.to_le_bytes());
    assert_eq!(&memory[88..92], &2i32.to_le_bytes());
    // More records than the area holds is the engine's error, not a trap.
    let three = [records[0]; 3];
    assert!(matches!(
        module.deliver(&three, 100_000, &mut host),
        Err(Fault::Invalid(_))
    ));
}

#[test]
fn malformed_custom_sections_reject_the_module() {
    let good = delivery(1024, 240);
    let cases = [
        (
            actions(&[("Panic", 7), ("PANIC", 8)]),
            good.clone(),
            "duplicate",
        ),
        (actions(&[("P\u{e9}", 7)]), good.clone(), "ASCII"),
        (
            [actions(&[("Panic", 7)]), vec![0]].concat(),
            good.clone(),
            "trailing",
        ),
        (vec![2, 0, 0, 0, 0], good.clone(), "format"),
        (actions(&[]), delivery(65_504, 240), "past"),
        (actions(&[]), delivery(1020, 240), "unaligned"),
    ];
    for (actions, delivery, why) in cases {
        let error = SimModule::load(&module(&actions, &delivery), Limits::MAP)
            .err()
            .unwrap_or_else(|| panic!("{why}: module loaded"));
        assert!(error.to_string().contains(why), "{why}: {error}");
    }
}

/// The trigger-side action numbers are the appendix 5.3 table's.
#[test]
fn trigger_action_numbers_match_the_action_kind_table() {
    use crate::triggers::{ACTION_MODULE, ACTION_SET_VAR};
    assert_eq!(crate::action_kind_id("SetVar"), Some(ACTION_SET_VAR));
    assert_eq!(crate::action_kind_id("Module"), Some(ACTION_MODULE));
}

/// The trigger-side event numbers are the appendix 4.3 table's.
#[test]
fn trigger_event_numbers_match_the_event_kind_table() {
    use crate::triggers::{
        EVENT_FIRE, EVENT_KILLED, EVENT_PROPERTY_POLL, EVENT_TAKE_DAMAGE, EVENT_TIMER,
        EVENT_VARIABLE_POLL,
    };
    assert_eq!(crate::event_kind_id("takedamage"), Some(EVENT_TAKE_DAMAGE));
    assert_eq!(crate::event_kind_id("Killed"), Some(EVENT_KILLED));
    assert_eq!(crate::event_kind_id("Fire"), Some(EVENT_FIRE));
    assert_eq!(crate::event_kind_id("Timer"), Some(EVENT_TIMER));
    assert_eq!(
        crate::event_kind_id("PropertyPoll"),
        Some(EVENT_PROPERTY_POLL)
    );
    assert_eq!(
        crate::event_kind_id("VariablePoll"),
        Some(EVENT_VARIABLE_POLL)
    );
    use crate::decide::{
        EVENT_COMMAND_ALLOWED, EVENT_DEFEAT, EVENT_PRODUCTION_ALLOWED, EVENT_PRODUCTION_QUEUED,
    };
    assert_eq!(
        crate::event_kind_id("ProductionAllowed"),
        Some(EVENT_PRODUCTION_ALLOWED)
    );
    assert_eq!(
        crate::event_kind_id("ProductionQueued"),
        Some(EVENT_PRODUCTION_QUEUED)
    );
    assert_eq!(crate::event_kind_id("Defeat"), Some(EVENT_DEFEAT));
    assert_eq!(
        crate::event_kind_id("CommandAllowed"),
        Some(EVENT_COMMAND_ALLOWED)
    );
    assert_eq!(crate::event_kind_id("TechTypeDoesNotExist"), Some(61));
}

/// RFC 0001 0.0.22's events take 100044.. (1044.. here) in the event kind
/// table, decision-only and delivered alike, and `DamageIncoming` keeps 1039.
#[test]
fn control_point_event_numbers_match_the_event_kind_table() {
    use crate::decide::{
        DECISION_ONLY_EVENTS, EVENT_CRUSH_ALLOWED, EVENT_DAMAGE_INCOMING, EVENT_DEPLOY_ALLOWED,
        EVENT_DESTINATION_SET, EVENT_FIRE_ALLOWED, EVENT_HARVEST_CELL, EVENT_RETALIATE,
    };
    use crate::triggers::{
        EVENT_CLOAKED, EVENT_DECLOAKED, EVENT_DEPLOYED, EVENT_EXPERIENCE_GAINED,
        EVENT_MISSION_CHANGED, EVENT_PROMOTED,
    };
    for (name, kind) in [
        ("DamageIncoming", EVENT_DAMAGE_INCOMING),
        ("FireAllowed", EVENT_FIRE_ALLOWED),
        ("Retaliate", EVENT_RETALIATE),
        ("DestinationSet", EVENT_DESTINATION_SET),
        ("CrushAllowed", EVENT_CRUSH_ALLOWED),
        ("MissionChanged", EVENT_MISSION_CHANGED),
        ("HarvestCell", EVENT_HARVEST_CELL),
        ("DeployAllowed", EVENT_DEPLOY_ALLOWED),
        ("Deployed", EVENT_DEPLOYED),
        ("Promoted", EVENT_PROMOTED),
        ("ExperienceGained", EVENT_EXPERIENCE_GAINED),
        ("Cloaked", EVENT_CLOAKED),
        ("Decloaked", EVENT_DECLOAKED),
    ] {
        assert_eq!(crate::event_kind_id(name), Some(kind), "{name}");
    }
    for delivered in [
        EVENT_MISSION_CHANGED,
        EVENT_DEPLOYED,
        EVENT_PROMOTED,
        EVENT_EXPERIENCE_GAINED,
        EVENT_CLOAKED,
        EVENT_DECLOAKED,
    ] {
        assert!(!DECISION_ONLY_EVENTS.contains(&delivered));
    }
}
