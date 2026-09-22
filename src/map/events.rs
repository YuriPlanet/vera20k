//! Map event parsing.
//!
//! `[Events]` stores a counted list of event-condition chunks per trigger id.
//! We preserve the raw field list and also expose normalized conditions so
//! runtime code can evaluate them without hardcoding a flat row assumption.

use std::collections::HashMap;

use crate::rules::ini_parser::IniFile;
use crate::rules::ini_value::scan_decimal_i32;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MapEvent {
    pub id: String,
    pub fields: Vec<String>,
    pub conditions: Vec<EventCondition>,
}

pub type EventMap = HashMap<String, MapEvent>;

/// Parse `[Events]` into an id -> event record map.
pub fn parse_events(ini: &IniFile) -> EventMap {
    let Some(section) = ini.section("Events") else {
        return HashMap::new();
    };

    let mut events: EventMap = HashMap::new();
    for key in section.keys() {
        // TriggerType Read7274A6 uses the same512-byte buffer as its header.
        let raw_value = section.read_string(key, "", 512);
        let id = key.trim();
        if id.is_empty() {
            continue;
        }
        let id = id.to_ascii_uppercase();
        let fields: Vec<String> = raw_value
            .split(',')
            .map(|part| part.trim().to_string())
            .collect();
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
    let number = |text: &str| scan_decimal_i32(&mut text.as_bytes()).unwrap_or(0);
    let mut tokens = raw.split(',').filter(|part| !part.is_empty());
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
                if let Some(name) = tokens.next().filter(|name| !name.is_empty()) {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn records_follow_native_width_values_and_prepend_order() {
        let ini = IniFile::from_str("[Events]\nEV_A=3,47,0,3,60,2,2,E1,27,0,7\nEV_B=2,7\n");
        let events = parse_events(&ini);
        assert_eq!(
            events["EV_A"].conditions,
            vec![
                EventCondition {
                    kind: 27,
                    value: 7,
                    ..Default::default()
                },
                EventCondition {
                    kind: 60,
                    value: 2,
                    type_name: Some("E1".into()),
                    ..Default::default()
                },
                EventCondition {
                    kind: 47,
                    value: 3,
                    ..Default::default()
                },
            ]
        );
        assert!(events["EV_B"].conditions.is_empty());
        assert_eq!(events["EV_B"].fields, ["2", "7"]);
    }

    #[test]
    fn test_missing_events_is_empty() {
        let ini = IniFile::from_str("[Map]\nTheater=TEMPERATE\n");
        assert!(parse_events(&ini).is_empty());
    }
}
