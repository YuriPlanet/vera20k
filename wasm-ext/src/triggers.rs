//! Runtime triggers (RFC 0001 sections 5, 6a and 7): the `TriggerDef`
//! `trigger_create` reads, the `EventRecord` `on_events` receives, and the
//! numbers both use (appendix "Fixed layouts", 4 and 5.3).
//!
//! Provisional numbering (open question 5): the new delivered events of
//! appendix 4.3 are numbered from 1000 in table order, as the atomic actions
//! of 5.3 are, and the polling events of 4.4 follow them (1026..1030).
//! Stock events keep 0..61 and Ares 62..88.

use crate::{ActionParams, Invalid, Op, le_i32, le_i64, property_name};

/// New delivered events (appendix 4.3), from 1000 in table order. Which of
/// them the engine raises is its `has_kind(0, kind)`; `trigger_create`
/// answers `Unsupported` for the others.
pub const EVENT_TAKE_DAMAGE: i32 = 1000;
pub const EVENT_HIT_BY_WARHEAD: i32 = 1001;
pub const EVENT_ATTACKED_BY: i32 = 1002;
pub const EVENT_FATAL_DAMAGE: i32 = 1003;
pub const EVENT_KILLED: i32 = 1004;
pub const EVENT_FIRE: i32 = 1005;
pub const EVENT_SPAWN_FIRE: i32 = 1006;
pub const EVENT_HIT_TARGET: i32 = 1007;
pub const EVENT_TARGET_CHANGED: i32 = 1008;
pub const EVENT_PROJECTILE_INCOMING: i32 = 1009;
pub const EVENT_DETONATED: i32 = 1010;
pub const EVENT_BIRTH: i32 = 1011;
pub const EVENT_TRANSPORT_ENTERED: i32 = 1012;
pub const EVENT_TRANSPORT_EXITED: i32 = 1013;
pub const EVENT_PASSENGER_ADDED: i32 = 1014;
pub const EVENT_PASSENGER_REMOVED: i32 = 1015;
pub const EVENT_COMMANDED: i32 = 1016;
pub const EVENT_CRUSHED: i32 = 1017;
pub const EVENT_HOUSE_TRANSFER: i32 = 1018;
pub const EVENT_HOUSE_TRANSFERRED: i32 = 1019;
pub const EVENT_MODIFIER_START: i32 = 1020;
pub const EVENT_MODIFIER_EXPIRE: i32 = 1021;
pub const EVENT_MODIFIER_STACKS: i32 = 1022;
pub const EVENT_TIMER: i32 = 1023;
pub const EVENT_SUPER_WEAPON_LAUNCHED: i32 = 1024;
pub const EVENT_PRODUCTION_CANCELLED: i32 = 1025;

/// New polling events (appendix 4.4), numbered on after 4.3 in table order
/// (provisional, open question 5).
pub const EVENT_PROPERTY_POLL: i32 = 1026;
pub const EVENT_HOUSE_POWER_POLL: i32 = 1027;
pub const EVENT_MODIFIER_COUNT_POLL: i32 = 1028;
pub const EVENT_DISTANCE_POLL: i32 = 1029;
pub const EVENT_VARIABLE_POLL: i32 = 1030;

/// INI registration (RFC 0001 section 6h):
/// the module event `RaiseEvent` raises (`EventDef.id` is the registered
/// event ID), and the records the module phase hands a module before its
/// `on_scenario_start`: one per registry entry and one per registered key
/// found in the merged rules. The last two never appear in a trigger.
pub const EVENT_MODULE: i32 = 1040;
pub const EVENT_INI_ENTRY: i32 = 1041;
pub const EVENT_INI_KEY: i32 = 1042;
/// `OrderRefused` (RFC 0001 6i): an order refused in the command step.
pub const EVENT_ORDER_REFUSED: i32 = 1043;

/// Delivered events RFC 0001 0.0.22 added to appendix 4.3, numbered on from
/// 1044 with the decision-only events of the same version
/// ([`crate::decide`]).
pub const EVENT_MISSION_CHANGED: i32 = 1048;
pub const EVENT_DEPLOYED: i32 = 1051;
pub const EVENT_PROMOTED: i32 = 1052;
pub const EVENT_EXPERIENCE_GAINED: i32 = 1053;
pub const EVENT_CLOAKED: i32 = 1054;
pub const EVENT_DECLOAKED: i32 = 1055;

/// The stock event kinds (appendix 4.1).
pub const STOCK_EVENTS: std::ops::RangeInclusive<i32> = 0..=61;

/// Atomic actions (appendix 5.3) from 1000 in table order: `SetVar` is entry
/// 24 and `Module` entry 36 (`AddMoney`, entry 15, is the engine's
/// `ACTION_ADD_MONEY`).
pub const ACTION_SET_VAR: i32 = 1024;
pub const ACTION_MODULE: i32 = 1036;
/// `RaiseEvent` (RFC 0001 appendix 5.3).
pub const ACTION_RAISE_EVENT: i32 = 1042;

/// Name kind 17 (appendix 8.1): a name in this module's `vera.actions`,
/// answered by the sandbox with its dispatch key.
pub const NAME_MODULE_ACTION: i32 = 17;
/// `has_kind` table 0: event kinds.
pub const TABLE_EVENT: i32 = 0;

/// Most events, actions and clauses per event in one `TriggerDef` (open
/// question 7). Events latch in a 32-bit mask.
pub const MAX_EVENTS: usize = 16;
pub const MAX_ACTIONS: usize = 32;
pub const MAX_CLAUSES: usize = 16;

/// `ActionTarget` (appendix 5.1).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum ActionTarget {
    Subject,
    Other,
    TagHolder,
    Projectile,
    House,
    Cell,
}

/// `TagRepeat` (appendix 8.3, enum 15).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum Repeat {
    OneTimeOr,
    OneTimeAnd,
    Repeating,
}

/// Whose value a filter clause's variable is (`VarTarget`, appendix 8.3).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum VarTarget {
    Global,
    Subject,
    Other,
    House,
}

/// What a trigger's filter clause reads (appendix "Filter clause").
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum FilterSubject {
    /// A property of the event's subject.
    Subject(i32),
    /// A property of the event's other object.
    Other(i32),
    /// A property of the trigger's house for this firing.
    House(i32),
    Var {
        var: i32,
        target: VarTarget,
    },
}

/// One filter clause of an event definition.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct FilterClause {
    pub subject: FilterSubject,
    pub op: u8,
    pub value: i64,
}

impl FilterClause {
    pub const SIZE: usize = 16;

    fn parse(bytes: &[u8]) -> Result<Self, Invalid> {
        let invalid = |why: String| Err(Invalid(format!("Clause: {why}")));
        let [subject, op, scope, reserved] = [bytes[0], bytes[1], bytes[2], bytes[3]];
        let prop = le_i32(bytes, 4);
        if reserved != 0 {
            return invalid("reserved is not 0".into());
        }
        Op::parse(op)?;
        let property = |prop: i32| match property_name(prop) {
            Some(_) if scope == 0 => Ok(prop),
            _ => Err(Invalid(format!("Clause: property {prop}, scope {scope}"))),
        };
        let subject = match subject {
            0 => FilterSubject::Subject(property(prop)?),
            1 => FilterSubject::Other(property(prop)?),
            2 => FilterSubject::House(property(prop)?),
            3 => FilterSubject::Var {
                var: prop,
                target: match scope {
                    0 => VarTarget::Global,
                    1 => VarTarget::Subject,
                    2 => VarTarget::Other,
                    3 => VarTarget::House,
                    _ => return invalid(format!("variable scope {scope}")),
                },
            },
            _ => return invalid(format!("unknown subject {subject}")),
        };
        Ok(Self {
            subject,
            op,
            value: le_i64(bytes, 8),
        })
    }

    /// Whether `reading op value` holds.
    pub fn holds(&self, reading: i64) -> bool {
        // `parse` admitted only known operators.
        Op::parse(self.op).is_ok_and(|op| op.holds(reading, self.value))
    }
}

/// One event of a trigger (`EventDef` and its clauses).
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct EventDef {
    pub kind: i32,
    pub op: u8,
    pub interval: i32,
    pub id: i32,
    pub value: i64,
    pub clauses: Vec<FilterClause>,
}

impl EventDef {
    pub const SIZE: usize = 32;

    /// Whether a polling event's `reading op value` holds (appendix 4.4);
    /// `DistancePoll` compares with `id`, its `value` being the cell.
    pub fn compares(&self, reading: i64) -> bool {
        let constant = match self.kind {
            EVENT_DISTANCE_POLL => i64::from(self.id),
            _ => self.value,
        };
        // `TriggerDef::parse` admitted only known operators.
        Op::parse(self.op).is_ok_and(|op| op.holds(reading, constant))
    }
}

/// One action of a trigger (`ActionDef` and its `ActionParams`).
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct ActionDef {
    pub kind: i32,
    pub target: ActionTarget,
    pub params: ActionParams,
    /// `params.text` copied out of module memory by `trigger_create`
    /// (appendix "Fixed layouts"); [`TriggerDef::parse`] leaves it empty and
    /// the sandbox fills it.
    pub text: String,
}

impl ActionDef {
    pub const SIZE: usize = 8 + ActionParams::SIZE;
    /// Most bytes of text one trigger action keeps (open question 7).
    pub const MAX_TEXT: usize = 1024;
}

/// `TriggerFlags` bits of `TriggerDef.flags`.
pub const FLAG_STARTS_DISABLED: i32 = 1;
pub const FLAG_ALLOW_WIN: i32 = 2;
pub const FLAG_TRANSFER_ON_CAPTURE: i32 = 4;

/// A `TriggerDef` with its events and actions, checked for the shape rules
/// of the appendix. What each kind accepts is the host's to check.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TriggerDef {
    pub house: i64,
    pub repeat: Repeat,
    /// Bit 0 Easy, 1 Normal, 2 Hard; never 0.
    pub difficulty: u8,
    pub flags: i32,
    /// −1 for none.
    pub decide_key: i32,
    pub events: Vec<EventDef>,
    pub actions: Vec<ActionDef>,
}

impl TriggerDef {
    pub const HEADER: usize = 32;

    /// Reads a definition that fills `bytes` exactly. Any breach of the
    /// layout, an unknown enum value or trailing bytes is invalid.
    pub fn parse(bytes: &[u8]) -> Result<Self, Invalid> {
        let invalid = |why: String| Err(Invalid(format!("TriggerDef: {why}")));
        let Some(header) = bytes.get(..Self::HEADER) else {
            return invalid(format!("{} bytes, header is {}", bytes.len(), Self::HEADER));
        };
        let repeat = match le_i32(header, 8) {
            0 => Repeat::OneTimeOr,
            1 => Repeat::OneTimeAnd,
            2 => Repeat::Repeating,
            other => return invalid(format!("repeat {other}")),
        };
        let difficulty = le_i32(header, 12);
        let difficulty = match u8::try_from(difficulty) {
            Ok(mask @ 1..=7) => mask,
            _ => return invalid(format!("difficulty mask {difficulty}")),
        };
        let flags = le_i32(header, 16);
        if flags & !(FLAG_STARTS_DISABLED | FLAG_ALLOW_WIN | FLAG_TRANSFER_ON_CAPTURE) != 0 {
            return invalid(format!("flags {flags:#x}"));
        }
        let decide_key = le_i32(header, 20);
        let counts = (le_i32(header, 24), le_i32(header, 28));
        let (events, actions) = match counts {
            (e, a)
                if (1..=MAX_EVENTS as i32).contains(&e)
                    && (0..=MAX_ACTIONS as i32).contains(&a) =>
            {
                (e as usize, a as usize)
            }
            (e, a) => return invalid(format!("{e} events and {a} actions")),
        };
        if actions == 0 && decide_key == -1 {
            return invalid("neither an action nor a decision handler".into());
        }
        let mut cursor = Cursor {
            bytes,
            at: Self::HEADER,
        };
        let mut parsed_events = Vec::with_capacity(events);
        for _ in 0..events {
            let event = cursor.take(EventDef::SIZE)?;
            let (op, clause_count) = (le_i32(event, 4), le_i32(event, 24));
            if le_i32(event, 28) != 0 {
                return invalid("EventDef reserved is not 0".into());
            }
            let op = u8::try_from(op)
                .ok()
                .filter(|op| Op::parse(*op).is_ok())
                .ok_or_else(|| Invalid(format!("TriggerDef: event op {op}")))?;
            let clause_count = usize::try_from(clause_count)
                .ok()
                .filter(|n| *n <= MAX_CLAUSES)
                .ok_or_else(|| Invalid(format!("TriggerDef: {clause_count} clauses")))?;
            let mut clauses = Vec::with_capacity(clause_count);
            for _ in 0..clause_count {
                clauses.push(FilterClause::parse(cursor.take(FilterClause::SIZE)?)?);
            }
            parsed_events.push(EventDef {
                kind: le_i32(event, 0),
                op,
                interval: le_i32(event, 8),
                id: le_i32(event, 12),
                value: le_i64(event, 16),
                clauses,
            });
        }
        let mut parsed_actions = Vec::with_capacity(actions);
        for _ in 0..actions {
            let action = cursor.take(ActionDef::SIZE)?;
            let target = match le_i32(action, 4) {
                0 => ActionTarget::Subject,
                1 => ActionTarget::Other,
                2 => ActionTarget::TagHolder,
                3 => ActionTarget::Projectile,
                4 => ActionTarget::House,
                5 => ActionTarget::Cell,
                other => return invalid(format!("action target {other}")),
            };
            parsed_actions.push(ActionDef {
                kind: le_i32(action, 0),
                target,
                params: ActionParams::parse(&action[8..])?,
                text: String::new(),
            });
        }
        if cursor.at != bytes.len() {
            return invalid(format!("{} trailing bytes", bytes.len() - cursor.at));
        }
        Ok(Self {
            house: le_i64(header, 0),
            repeat,
            difficulty,
            flags,
            decide_key,
            events: parsed_events,
            actions: parsed_actions,
        })
    }
}

/// Reads consecutive fixed-size parts of a buffer.
struct Cursor<'a> {
    bytes: &'a [u8],
    at: usize,
}

impl<'a> Cursor<'a> {
    fn take(&mut self, len: usize) -> Result<&'a [u8], Invalid> {
        let at = self.at;
        let slice = at
            .checked_add(len)
            .and_then(|end| self.bytes.get(at..end))
            .ok_or_else(|| Invalid(format!("TriggerDef: truncated at byte {at}")))?;
        self.at += len;
        Ok(slice)
    }
}

/// `EventRecord` (appendix "Fixed layouts"), 120 bytes little endian, read
/// in place by `on_events`. Queued records travel in saves and the state
/// hash.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct EventRecord {
    pub key: i32,
    pub kind: i32,
    pub trigger: i64,
    pub holder: i64,
    pub subject: i64,
    pub other: i64,
    pub house: i64,
    pub projectile: i64,
    pub modifier: i64,
    pub cell: i64,
    pub value: i64,
    pub value2: i64,
    pub id: i32,
    pub frame: i32,
    pub x: i32,
    pub y: i32,
    pub z: i32,
    /// A string ID: a module action's parameter text, an `IniEntry`'s entry
    /// name, an `IniKey`'s value; 0 for none. Saves from before the INI
    /// registration (RFC 0001 0.0.21) read 0.
    #[serde(default)]
    pub args: i32,
    /// A module action's `ActionTarget` (appendix 5.1 order); 0 otherwise.
    #[serde(default)]
    pub target: i32,
}

impl EventRecord {
    pub const SIZE: usize = 120;

    /// A record of `kind` with every reference 0 and every unlisted integer
    /// −1, as the appendix asks.
    pub const fn empty(kind: i32) -> Self {
        Self {
            key: 0,
            kind,
            trigger: 0,
            holder: 0,
            subject: 0,
            other: 0,
            house: 0,
            projectile: 0,
            modifier: 0,
            cell: 0,
            value: -1,
            value2: -1,
            id: -1,
            frame: 0,
            x: -1,
            y: -1,
            z: -1,
            args: 0,
            target: 0,
        }
    }

    pub fn to_bytes(&self) -> [u8; Self::SIZE] {
        let mut out = [0u8; Self::SIZE];
        let mut put = |at: usize, bytes: &[u8]| out[at..at + bytes.len()].copy_from_slice(bytes);
        put(0, &self.key.to_le_bytes());
        put(4, &self.kind.to_le_bytes());
        put(8, &self.trigger.to_le_bytes());
        put(16, &self.holder.to_le_bytes());
        put(24, &self.subject.to_le_bytes());
        put(32, &self.other.to_le_bytes());
        put(40, &self.house.to_le_bytes());
        put(48, &self.projectile.to_le_bytes());
        put(56, &self.modifier.to_le_bytes());
        put(64, &self.cell.to_le_bytes());
        put(72, &self.value.to_le_bytes());
        put(80, &self.value2.to_le_bytes());
        put(88, &self.id.to_le_bytes());
        put(92, &self.frame.to_le_bytes());
        put(96, &self.x.to_le_bytes());
        put(100, &self.y.to_le_bytes());
        put(104, &self.z.to_le_bytes());
        put(108, &self.args.to_le_bytes());
        put(112, &self.target.to_le_bytes());
        // 116: reserved, always 0.
        out
    }
}
