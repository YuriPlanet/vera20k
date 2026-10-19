//! Map event parsing.
//!
//! `[Events]` stores a counted list of event-condition chunks per trigger id.
//! We preserve the raw field list and also expose normalized conditions so
//! runtime code can evaluate them without hardcoding a flat row assumption.
//! Records keep their exact key: TriggerType::ReadINI (`0x0072749B`) reads
//! the entry by the Type's own ID.

use std::collections::HashMap;

use crate::rules::ini_parser::IniFile;
use crate::rules::ini_value::{crt_atoi, strtok};

#[derive(Debug, Clone, Default, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct EventCondition {
    pub kind: i32,
    /// Materialized TEvent+34, not the parameter-type discriminator.
    pub value: i32,
    /// Native +38: parameter type2 copies at most24 bytes, without trimming.
    pub type_name: Option<String>,
    /// Unresolved parameter type1 reference; native Read resolves ID or Name
    /// against the already loaded TeamType registry (6F0FC0).
    pub team_name: Option<String>,
}

/// One `[Events]` record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MapEvent {
    id: String,
    fields: Vec<String>,
    conditions: Vec<EventCondition>,
}

impl MapEvent {
    /// The events in native list order: the loop at `0x007274E2` prepends
    /// each one it reads, so the last authored event comes first.
    pub fn conditions(&self) -> &[EventCondition] {
        &self.conditions
    }
}

pub type EventMap = HashMap<String, MapEvent>;

/// Parse `[Events]` into exact-key event records.
pub fn parse_events(ini: &IniFile) -> EventMap {
    let Some(section) = ini.section("Events") else {
        return HashMap::new();
    };

    let mut events: EventMap = HashMap::new();
    for key in section.keys() {
        // TriggerType Read7274A6 uses the same512-byte buffer as its header.
        let raw_value = section.read_string(key, "", 512);
        let id = key.to_string();
        let fields: Vec<String> = strtok(&raw_value, &[',']).map(str::to_string).collect();
        let conditions = parse_event_conditions(&raw_value);
        events.insert(
            id.clone(),
            MapEvent {
                id,
                fields,
                conditions,
            },
        );
    }

    if !events.is_empty() {
        log::info!("Parsed {} events from [Events]", events.len());
    }
    events
}

/// Original TriggerType loop7274DC..727516 prepends every newly read event.
/// TEvent Read71F4E0 consumes kind/parameter-type/value and, only for type2,
/// one additional type-name token. Empty comma fields are skipped by strtok.
fn parse_event_conditions(raw: &str) -> Vec<EventCondition> {
    let number = crt_atoi;
    let mut tokens = strtok(raw, &[',']);
    let count = tokens.next().map(number).unwrap_or(0);
    let mut conditions = Vec::new();
    for _ in 0..count {
        // Malformed truncated records are kept only in the diagnostic fields.
        let (Some(kind), Some(parameter_type), Some(parameter)) =
            (tokens.next(), tokens.next(), tokens.next())
        else {
            break;
        };
        let mut condition = EventCondition {
            kind: number(kind),
            ..Default::default()
        };
        match number(parameter_type) {
            0 => condition.value = number(parameter),
            1 => condition.team_name = Some(parameter.to_string()),
            2 => {
                condition.value = number(parameter);
                if let Some(name) = tokens.next() {
                    let bytes = name.as_bytes();
                    condition.type_name =
                        Some(String::from_utf8_lossy(&bytes[..bytes.len().min(24)]).into_owned());
                }
            }
            _ => {}
        }
        conditions.push(condition);
    }
    conditions.reverse();
    conditions
}
