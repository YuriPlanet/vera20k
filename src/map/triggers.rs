//! Map trigger records.
//!
//! `[Triggers]` holds each TriggerType header. TriggerType::ReadINI
//! (`0x00727240`) reads it by the Type's own ID through the exact-case INI
//! lookup, so records are keyed by their exact key and the runtime reader in
//! `sim::trigger_runtime` takes each one by that ID.
//! Format: `ID=HOUSE,<TRIGGER>,NAME,DISABLED,EASY,MEDIUM,HARD,TRANSFER`.

use std::collections::HashMap;

use crate::rules::ini_parser::IniFile;
use crate::rules::ini_value::{crt_atoi, strtok, truncate_native_bytes};

#[derive(Debug, Clone, PartialEq, Eq)]
struct TriggerDifficulty {
    easy: bool,
    medium: bool,
    hard: bool,
}

/// One `[Triggers]` header as TriggerType::ReadINI takes it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MapTrigger {
    id: String,
    fields: Vec<String>,
    owner: Option<String>,
    linked_trigger_id: Option<String>,
    name: Option<String>,
    enabled: bool,
    difficulty: TriggerDifficulty,
}

impl MapTrigger {
    /// Token 1, which `HouseTypeClass::FindIndex` (`0x007272B0`) resolves.
    pub fn owner(&self) -> Option<&str> {
        self.owner.as_deref()
    }

    /// Token 2 as written, absent for `<none>` or `none` (`0x007272E4`,
    /// `0x0072730C`); the caller's factory matches it ignoring case.
    pub fn linked_trigger_id(&self) -> Option<&str> {
        self.linked_trigger_id.as_deref()
    }

    /// TriggerType+9F: token 4 is DISABLED.
    pub fn enabled(&self) -> bool {
        self.enabled
    }

    /// TriggerType+9C/9D/9E in that order: easy, medium, hard.
    pub fn difficulty(&self) -> [bool; 3] {
        [
            self.difficulty.easy,
            self.difficulty.medium,
            self.difficulty.hard,
        ]
    }

    /// Token 8 (`0x00727475..0x0072749B`): a nonzero `atoi` sets
    /// TriggerType+A0, which a zero token never clears.
    pub fn transfer_on_capture(&self) -> bool {
        self.fields.get(7).is_some_and(|value| crt_atoi(value) != 0)
    }
}

pub type TriggerMap = HashMap<String, MapTrigger>;

/// Parse `[Triggers]` into exact-key header records.
pub fn parse_triggers(ini: &IniFile) -> TriggerMap {
    let Some(section) = ini.section("Triggers") else {
        return HashMap::new();
    };

    let mut triggers: TriggerMap = HashMap::new();
    for key in section.keys() {
        // TriggerType::ReadINI727274 uses a 512-byte ReadString buffer.
        let value = section.read_string(key, "", 512);
        if !value.is_empty() {
            triggers.insert(key.to_string(), parse_record(key.to_string(), &value));
        }
    }

    if !triggers.is_empty() {
        log::info!("Parsed {} triggers from [Triggers]", triggers.len());
    }
    triggers
}

fn parse_record(id: String, value: &str) -> MapTrigger {
    // Original strtok skips empty tokens, but a whitespace-only token still
    // consumes a slot.
    let tokens: Vec<&str> = strtok(value, &[',']).collect();
    // Original atoi7C9BFD: decimal prefix, CRT whitespace, wrapping i32.
    let mut flags = tokens.iter().skip(3).map(|token| crt_atoi(token));
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
        fields: tokens.iter().map(|token| token.to_string()).collect(),
    }
}

// TriggerTypeClass::Read_INI uses each `strtok` token as it stands: the owner
// through `HouseTypeClass::FindIndex` (`0x007272B0`), the link unless it is
// `<none>` or `none` (`0x007272E4`, `0x0072730C`), the name cut to 48 bytes
// (`0x00727395`).
fn parse_owner(fields: &[&str]) -> Option<String> {
    fields.first().map(|owner| owner.to_string())
}

fn parse_linked_trigger(fields: &[&str]) -> Option<String> {
    let value = fields.get(1)?;
    if value.eq_ignore_ascii_case("<none>") || value.eq_ignore_ascii_case("none") {
        None
    } else {
        Some(value.to_string())
    }
}

fn parse_name(fields: &[&str]) -> Option<String> {
    fields
        .get(2)
        .map(|name| truncate_native_bytes(name, 0x30).to_string())
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
    fn reference_tokens_keep_their_spaces_and_none_spellings_unlink() {
        let name = "N".repeat(60);
        let trigger = parse_record(
            "T".to_string(),
            &format!(" Neutral,none,{name} ,0,1,1,1,02"),
        );
        assert_eq!(trigger.owner.as_deref(), Some(" Neutral"));
        assert_eq!(trigger.linked_trigger_id, None);
        assert_eq!(trigger.name.as_deref(), Some(&name[..0x30]));
        assert_eq!(trigger.fields[7], "02");
        let linked = parse_record("T".to_string(), "Neutral, next,x");
        assert_eq!(linked.linked_trigger_id(), Some(" next"));
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
        let rows: Vec<NativeRow> = serde_json::from_str(crate::test_fixture::text(
            "tools/spatial_oracle/trigger_type_flags.json",
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
