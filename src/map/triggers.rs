//! Map trigger parsing.
//!
//! `[Triggers]` defines the raw trigger records that later connect tags,
//! events, and actions into actual scenario behavior. We preserve the raw
//! fields and also expose the key structured values needed by runtime logic.
//! Format per ModEnc for RA2/YR maps:
//! `ID=HOUSE,<TRIGGER>,NAME,A,B,C,D,E`

use std::collections::HashMap;

use crate::rules::ini_parser::IniFile;
use crate::rules::ini_value::scan_decimal_i32;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TriggerDifficulty {
    pub easy: bool,
    pub medium: bool,
    pub hard: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MapTrigger {
    pub id: String,
    pub fields: Vec<String>,
    pub owner: Option<String>,
    pub linked_trigger_id: Option<String>,
    pub name: Option<String>,
    pub enabled: bool,
    pub difficulty: TriggerDifficulty,
    /// Legacy runtime approximation. Native repetition is TagType+9C, not
    /// TriggerType+A0 (the last token); remove this with the Tag lifecycle port.
    pub repeating: bool,
}

pub type TriggerMap = HashMap<String, MapTrigger>;

/// Parse `[Triggers]` into an id -> raw trigger record map.
pub fn parse_triggers(ini: &IniFile) -> TriggerMap {
    let Some(section) = ini.section("Triggers") else {
        return HashMap::new();
    };

    let mut triggers: TriggerMap = HashMap::new();
    for key in section.keys() {
        let id = key.trim();
        if id.is_empty() {
            continue;
        }
        let id = id.to_ascii_uppercase();
        // TriggerType::ReadINI727274 uses a 512-byte ReadString buffer.
        let value = section.read_string(key, "", 512);
        if !value.is_empty() {
            triggers.insert(id.clone(), parse_record(id, &value));
        }
    }

    if !triggers.is_empty() {
        log::info!("Parsed {} triggers from [Triggers]", triggers.len());
    }
    triggers
}

fn parse_record(id: String, value: &str) -> MapTrigger {
    let fields: Vec<String> = value
        .split(',')
        .map(|part| part.trim().to_string())
        .collect();
    // Original strtok skips empty tokens, but a whitespace-only token still
    // consumes a slot. Keep diagnostic fields separate from semantic tokens.
    let tokens: Vec<&str> = value.split(',').filter(|part| !part.is_empty()).collect();
    let mut flags = tokens.iter().skip(3).map(|token| {
        // Original atoi7C9BFD: decimal prefix, CRT whitespace, wrapping i32.
        scan_decimal_i32(&mut token.as_bytes()).unwrap_or(0)
    });
    // ReadINI7273B9..727475: token4 is DISABLED; difficulty tokens5..7
    // accept any nonzero atoi value. A missing token writes false, including
    // enabled+9F. Native executable comparisons: trigger_type_flags.json.
    let enabled = flags.next().is_some_and(|value| value == 0);
    let difficulty = TriggerDifficulty {
        easy: flags.next().is_some_and(|value| value != 0),
        medium: flags.next().is_some_and(|value| value != 0),
        hard: flags.next().is_some_and(|value| value != 0),
    };
    MapTrigger {
        id,
        owner: parse_owner(&tokens),
        linked_trigger_id: parse_linked_trigger(&tokens),
        name: parse_name(&tokens),
        enabled,
        difficulty,
        repeating: parse_repeat_mode(&fields),
        fields,
    }
}

fn parse_owner(fields: &[&str]) -> Option<String> {
    let owner = fields.first()?.trim();
    (!owner.is_empty()).then(|| owner.to_string())
}

fn parse_linked_trigger(fields: &[&str]) -> Option<String> {
    let value = fields.get(1)?.trim();
    if value.is_empty() || value.eq_ignore_ascii_case("<none>") {
        None
    } else {
        Some(value.to_ascii_uppercase())
    }
}

fn parse_name(fields: &[&str]) -> Option<String> {
    let name = fields.get(2)?.trim();
    (!name.is_empty()).then(|| name.to_string())
}

fn parse_repeat_mode(fields: &[String]) -> bool {
    fields
        .get(7)
        .map(|value| value.trim() == "2")
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_triggers() {
        let ini = IniFile::from_str(
            "[Triggers]\nTR_A=Neutral,TR_NEXT,Alpha Trigger,1,1,0,1,0\nTR_B=House,<none>,Beta Trigger\n",
        );
        let triggers = parse_triggers(&ini);
        assert_eq!(triggers.len(), 2);
        assert_eq!(
            triggers.get("TR_A"),
            Some(&MapTrigger {
                id: "TR_A".to_string(),
                owner: Some("Neutral".to_string()),
                linked_trigger_id: Some("TR_NEXT".to_string()),
                name: Some("Alpha Trigger".to_string()),
                enabled: false,
                difficulty: TriggerDifficulty {
                    easy: true,
                    medium: false,
                    hard: true,
                },
                repeating: false,
                fields: vec![
                    "Neutral".to_string(),
                    "TR_NEXT".to_string(),
                    "Alpha Trigger".to_string(),
                    "1".to_string(),
                    "1".to_string(),
                    "0".to_string(),
                    "1".to_string(),
                    "0".to_string()
                ],
            })
        );
        assert_eq!(
            triggers
                .get("TR_B")
                .map(|trigger| trigger.fields.as_slice()),
            Some(
                &[
                    "House".to_string(),
                    "<none>".to_string(),
                    "Beta Trigger".to_string(),
                ][..]
            )
        );
        assert_eq!(
            triggers
                .get("TR_B")
                .map(|trigger| trigger.linked_trigger_id.as_deref()),
            Some(None)
        );
    }

    #[test]
    fn flag_reader_matches_original_trigger_type_instructions() {
        #[derive(serde::Deserialize)]
        struct NativeRow {
            prior: [u8; 5],
            read: String,
            applied: bool,
            output: [u8; 5],
        }
        let rows: Vec<NativeRow> = serde_json::from_str(include_str!(
            "../../tools/spatial_oracle/trigger_type_flags.json"
        ))
        .unwrap();
        let mut checked = 0;
        for row in rows {
            // The loader creates definitions once. Reload retention, absent
            // records and the unported A0 field are native evidence only.
            if row.prior != [1, 1, 1, 1, 0] || !row.applied {
                continue;
            }
            let trigger = parse_record("TEST".to_string(), &row.read);
            assert_eq!(
                [
                    trigger.difficulty.easy as u8,
                    trigger.difficulty.medium as u8,
                    trigger.difficulty.hard as u8,
                    trigger.enabled as u8,
                ],
                row.output[..4],
                "{:?}",
                row.read
            );
            checked += 1;
        }
        assert_eq!(checked, 29);
    }

    #[test]
    fn test_missing_triggers_is_empty() {
        let ini = IniFile::from_str("[Map]\nTheater=TEMPERATE\n");
        assert!(parse_triggers(&ini).is_empty());
    }
}
