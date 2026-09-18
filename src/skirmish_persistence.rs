//! Raw `[Skirmish]` snapshot codec for `RA2MD.INI`.
//!
//! The app owns when the snapshot is loaded and saved. This module only models
//! the native key order, defaults, integer/boolean parsing, slot triples, and a
//! preservation-safe batched byte update. It deliberately has no UI or
//! `AppState` dependency.

use crate::rules::error::RulesError;

pub const SKIRMISH_INI_SECTION: &str = "Skirmish";
pub const SKIRMISH_PERSISTED_SLOT_COUNT: usize = 7;

const RANDOM_COMBO_ITEM_DATA: i32 = -2;
const SLOT01_DEFAULT_TYPE: i32 = 6;
const OTHER_SLOT_DEFAULT_TYPE: i32 = 1;

/// The ten global fallback values supplied by the current Rules/session state.
///
/// Native `SessionClass__ReadSkirmishSettings` does not use one universal
/// hardcoded default set: absent global keys inherit fields already loaded from
/// Rules (with `ScenIndex` supplied as zero by the owner). Keeping these values
/// caller-supplied preserves that authority boundary.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SkirmishGlobalDefaults {
    pub game_mode: i32,
    pub scenario_index: i32,
    pub game_speed: i32,
    pub credits: i32,
    pub unit_count: i32,
    pub short_game: bool,
    pub super_weapons_allowed: bool,
    pub build_off_ally: bool,
    pub mcv_repacks: bool,
    pub crates_appear: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SkirmishPersistedSlot {
    /// Native persisted row-type code: None=1, Hard=4, Normal=5, Easy=6.
    pub row_type: i32,
    /// Raw country combo item data, including Random=-2.
    pub country: i32,
    /// Raw colour combo item data, including Random=-2.
    pub colour: i32,
}

impl SkirmishPersistedSlot {
    const fn native_default(slot_index: usize) -> Self {
        Self {
            row_type: if slot_index == 0 {
                SLOT01_DEFAULT_TYPE
            } else {
                OTHER_SLOT_DEFAULT_TYPE
            },
            country: RANDOM_COMBO_ITEM_DATA,
            colour: RANDOM_COMBO_ITEM_DATA,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkirmishPersistedSnapshot {
    pub game_mode: i32,
    pub scenario_index: i32,
    pub game_speed: i32,
    pub credits: i32,
    pub unit_count: i32,
    pub short_game: bool,
    pub super_weapons_allowed: bool,
    pub build_off_ally: bool,
    pub mcv_repacks: bool,
    pub crates_appear: bool,
    pub slots: [SkirmishPersistedSlot; SKIRMISH_PERSISTED_SLOT_COUNT],
}

impl SkirmishPersistedSnapshot {
    pub fn from_global_defaults(defaults: SkirmishGlobalDefaults) -> Self {
        Self {
            game_mode: defaults.game_mode,
            scenario_index: defaults.scenario_index,
            game_speed: defaults.game_speed,
            credits: defaults.credits,
            unit_count: defaults.unit_count,
            short_game: defaults.short_game,
            super_weapons_allowed: defaults.super_weapons_allowed,
            build_off_ally: defaults.build_off_ally,
            mcv_repacks: defaults.mcv_repacks,
            crates_appear: defaults.crates_appear,
            slots: std::array::from_fn(SkirmishPersistedSlot::native_default),
        }
    }

    /// Return all persisted key/value pairs in native writer order.
    pub fn ini_updates(&self) -> Vec<(String, String)> {
        let mut updates = Vec::with_capacity(10 + SKIRMISH_PERSISTED_SLOT_COUNT);
        updates.push(("GameMode".to_string(), self.game_mode.to_string()));
        updates.push(("ScenIndex".to_string(), self.scenario_index.to_string()));
        updates.push(("GameSpeed".to_string(), self.game_speed.to_string()));
        updates.push(("Credits".to_string(), self.credits.to_string()));
        updates.push(("UnitCount".to_string(), self.unit_count.to_string()));
        updates.push(("ShortGame".to_string(), yes_no(self.short_game).to_string()));
        updates.push((
            "SuperWeaponsAllowed".to_string(),
            yes_no(self.super_weapons_allowed).to_string(),
        ));
        updates.push((
            "BuildOffAlly".to_string(),
            yes_no(self.build_off_ally).to_string(),
        ));
        updates.push((
            "MCVRepacks".to_string(),
            yes_no(self.mcv_repacks).to_string(),
        ));
        updates.push((
            "CratesAppear".to_string(),
            yes_no(self.crates_appear).to_string(),
        ));
        for (index, slot) in self.slots.iter().enumerate() {
            updates.push((
                format!("Slot{:02}", index + 1),
                format!("{},{},{}", slot.row_type, slot.country, slot.colour),
            ));
        }
        updates
    }

    /// Apply this snapshot to raw `RA2MD.INI` bytes without rewriting any
    /// unrelated byte. All seventeen keys are composed into one output buffer.
    pub fn update_ini_bytes(&self, content: &[u8]) -> Vec<u8> {
        let updates = self.ini_updates();
        let borrowed: Vec<(&str, &str)> = updates
            .iter()
            .map(|(key, value)| (key.as_str(), value.as_str()))
            .collect();
        crate::util::ini_writer::set_ini_values(content, SKIRMISH_INI_SECTION, &borrowed)
    }
}

/// Read a raw `RA2MD.INI` buffer using native Skirmish key order and defaults.
pub fn read_skirmish_snapshot(
    content: &[u8],
    defaults: SkirmishGlobalDefaults,
) -> Result<SkirmishPersistedSnapshot, RulesError> {
    let mut snapshot = SkirmishPersistedSnapshot::from_global_defaults(defaults);
    let entries = raw_first_section_entries(content, SKIRMISH_INI_SECTION);
    if entries.is_empty() {
        return Ok(snapshot);
    }

    snapshot.game_mode = raw_native_int(&entries, "GameMode", snapshot.game_mode);
    snapshot.scenario_index = raw_native_int(&entries, "ScenIndex", snapshot.scenario_index);
    snapshot.game_speed = raw_native_int(&entries, "GameSpeed", snapshot.game_speed);
    snapshot.credits = raw_native_int(&entries, "Credits", snapshot.credits);
    snapshot.unit_count = raw_native_int(&entries, "UnitCount", snapshot.unit_count);
    snapshot.short_game = raw_native_bool(&entries, "ShortGame", snapshot.short_game);
    snapshot.super_weapons_allowed = raw_native_bool(
        &entries,
        "SuperWeaponsAllowed",
        snapshot.super_weapons_allowed,
    );
    snapshot.build_off_ally = raw_native_bool(&entries, "BuildOffAlly", snapshot.build_off_ally);
    snapshot.mcv_repacks = raw_native_bool(&entries, "MCVRepacks", snapshot.mcv_repacks);
    snapshot.crates_appear = raw_native_bool(&entries, "CratesAppear", snapshot.crates_appear);

    for (slot_index, slot) in snapshot.slots.iter_mut().enumerate() {
        let key = format!("Slot{:02}", slot_index + 1);
        if let Some(value) = raw_value(&entries, &key) {
            read_native_slot_triple(value, slot);
        }
    }
    Ok(snapshot)
}

fn yes_no(value: bool) -> &'static str {
    if value { "yes" } else { "no" }
}

fn raw_native_int(entries: &[(&str, &str)], key: &str, default: i32) -> i32 {
    raw_value(entries, key)
        .map(native_atoi_or_hex)
        .unwrap_or(default)
}

fn raw_native_bool(entries: &[(&str, &str)], key: &str, default: bool) -> bool {
    native_bool_value(raw_value(entries, key), default)
}

fn native_bool_value(value: Option<&str>, default: bool) -> bool {
    let Some(first) = value.and_then(|value| value.as_bytes().first()) else {
        return default;
    };
    match first.to_ascii_uppercase() {
        b'1' | b'T' | b'Y' => true,
        b'0' | b'F' | b'N' => false,
        _ => default,
    }
}

/// Parse only the first target section from raw ANSI-compatible bytes. Unknown
/// high-byte lines elsewhere are ignored, so an unrelated legacy player name
/// cannot prevent the ASCII `[Skirmish]` settings from loading.
fn raw_first_section_entries<'a>(content: &'a [u8], section: &str) -> Vec<(&'a str, &'a str)> {
    let mut entries = Vec::new();
    let mut in_target = false;
    let mut found_target = false;

    for raw_line in content.split(|byte| *byte == b'\n') {
        let line = trim_ascii_bytes(raw_line.strip_suffix(b"\r").unwrap_or(raw_line));
        if line.is_empty() || line.starts_with(b";") || line.starts_with(b"#") {
            continue;
        }

        if line.first() == Some(&b'[') {
            if let Some(close) = line.iter().position(|byte| *byte == b']') {
                let name = trim_ascii_bytes(&line[1..close]);
                if !found_target && ascii_eq_ignore_case(name, section.as_bytes()) {
                    found_target = true;
                    in_target = true;
                } else if in_target {
                    break;
                } else {
                    in_target = false;
                }
                continue;
            }
        }
        if !in_target {
            continue;
        }

        let Some(equals) = line.iter().position(|byte| *byte == b'=') else {
            continue;
        };
        let key_bytes = trim_ascii_bytes(&line[..equals]);
        if key_bytes.is_empty() {
            continue;
        }
        let mut value_bytes = trim_ascii_bytes(&line[equals + 1..]);
        if let Some(comment) = value_bytes.iter().position(|byte| *byte == b';') {
            value_bytes = trim_ascii_bytes(&value_bytes[..comment]);
        }
        let (Ok(key), Ok(value)) = (
            std::str::from_utf8(key_bytes),
            std::str::from_utf8(value_bytes),
        ) else {
            continue;
        };
        entries.push((key, value));
    }
    entries
}

fn raw_value<'a>(entries: &'a [(&str, &str)], key: &str) -> Option<&'a str> {
    entries
        .iter()
        .rev()
        .find(|(entry_key, _)| entry_key.eq_ignore_ascii_case(key))
        .map(|(_, value)| *value)
}

fn trim_ascii_bytes(mut bytes: &[u8]) -> &[u8] {
    while bytes.first().is_some_and(|byte| *byte <= b' ') {
        bytes = &bytes[1..];
    }
    while bytes.last().is_some_and(|byte| *byte <= b' ') {
        bytes = &bytes[..bytes.len() - 1];
    }
    bytes
}

fn ascii_eq_ignore_case(left: &[u8], right: &[u8]) -> bool {
    left.len() == right.len()
        && left
            .iter()
            .zip(right)
            .all(|(left, right)| left.eq_ignore_ascii_case(right))
}

/// `CCINIClass::ReadInt`: `$` prefix and `h` suffix select hexadecimal;
/// otherwise the CRT `atoi` decimal-prefix behavior is used.
fn native_atoi_or_hex(value: &str) -> i32 {
    let value = trim_native_whitespace(value);
    if let Some(hex) = value.strip_prefix('$') {
        return native_hex_prefix(hex).unwrap_or(0);
    }
    if value
        .as_bytes()
        .last()
        .is_some_and(|byte| byte.eq_ignore_ascii_case(&b'h'))
    {
        return native_hex_prefix(&value[..value.len().saturating_sub(1)]).unwrap_or(0);
    }
    native_atoi(value)
}

fn native_hex_prefix(value: &str) -> Option<i32> {
    let bytes = value.as_bytes();
    let mut cursor = 0;
    let negative = match bytes.first() {
        Some(b'-') => {
            cursor = 1;
            true
        }
        Some(b'+') => {
            cursor = 1;
            false
        }
        _ => false,
    };
    if bytes.get(cursor) == Some(&b'0')
        && bytes
            .get(cursor + 1)
            .is_some_and(|byte| byte.eq_ignore_ascii_case(&b'x'))
    {
        cursor += 2;
    }

    let mut value = 0u32;
    let mut found = false;
    while let Some(&byte) = bytes.get(cursor) {
        let digit = match byte {
            b'0'..=b'9' => u32::from(byte - b'0'),
            b'a'..=b'f' => u32::from(byte - b'a' + 10),
            b'A'..=b'F' => u32::from(byte - b'A' + 10),
            _ => break,
        };
        value = value.wrapping_mul(16).wrapping_add(digit);
        found = true;
        cursor += 1;
    }
    found.then(|| {
        let signed = value as i32;
        if negative {
            signed.wrapping_neg()
        } else {
            signed
        }
    })
}

fn native_atoi(value: &str) -> i32 {
    let bytes = value.as_bytes();
    let mut cursor = 0;
    while bytes.get(cursor).is_some_and(|byte| *byte <= b' ') {
        cursor += 1;
    }
    let negative = match bytes.get(cursor) {
        Some(b'-') => {
            cursor += 1;
            true
        }
        Some(b'+') => {
            cursor += 1;
            false
        }
        _ => false,
    };
    let mut parsed = 0u32;
    while let Some(byte @ b'0'..=b'9') = bytes.get(cursor) {
        parsed = parsed
            .wrapping_mul(10)
            .wrapping_add(u32::from(*byte - b'0'));
        cursor += 1;
    }
    let signed = parsed as i32;
    if negative {
        signed.wrapping_neg()
    } else {
        signed
    }
}

fn trim_native_whitespace(value: &str) -> &str {
    value.trim_matches(|character: char| (character as u32) <= 0x20)
}

/// `FUN_00477440`: copy through a 0x200-byte `ReadString` buffer, then
/// `strtok(",")` + `atoi` up to three times, leaving absent outputs unchanged.
fn read_native_slot_triple(value: &str, output: &mut SkirmishPersistedSlot) {
    let bytes = value.as_bytes();
    let capped_len = bytes.len().min(0x1ff);
    let Ok(capped) = std::str::from_utf8(&bytes[..capped_len]) else {
        return;
    };
    let capped = trim_native_whitespace(capped);
    if capped.is_empty() {
        return;
    }
    let mut tokens = capped.split(',').filter(|token| !token.is_empty());
    if let Some(token) = tokens.next() {
        output.row_type = native_atoi(token);
    }
    if let Some(token) = tokens.next() {
        output.country = native_atoi(token);
    }
    if let Some(token) = tokens.next() {
        output.colour = native_atoi(token);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn defaults() -> SkirmishGlobalDefaults {
        SkirmishGlobalDefaults {
            game_mode: 1,
            scenario_index: 0,
            game_speed: 1,
            credits: 10_000,
            unit_count: 10,
            short_game: true,
            super_weapons_allowed: true,
            build_off_ally: true,
            mcv_repacks: true,
            crates_appear: true,
        }
    }

    #[test]
    fn absent_section_uses_caller_globals_and_native_slot_defaults() {
        let snapshot = read_skirmish_snapshot(b"[Options]\nFoo=bar\n", defaults()).unwrap();
        assert_eq!(snapshot.game_speed, 1);
        assert_eq!(snapshot.slots[0], SkirmishPersistedSlot::native_default(0));
        for slot in &snapshot.slots[1..] {
            assert_eq!(*slot, SkirmishPersistedSlot::native_default(1));
        }
    }

    #[test]
    fn reads_all_globals_and_slot_triples_with_native_scalar_rules() {
        let input = br#"[Skirmish]
GameMode=$03
ScenIndex=A2h
GameSpeed=4junk
Credits=7500
UnitCount=5
ShortGame=Yep
SuperWeaponsAllowed=Nope
BuildOffAlly=True
MCVRepacks=0anything
CratesAppear=?
Slot01=4,8,1
Slot02=5,-2,-2
"#;
        let snapshot = read_skirmish_snapshot(input, defaults()).unwrap();
        assert_eq!(snapshot.game_mode, 3);
        assert_eq!(snapshot.scenario_index, 0xa2);
        assert_eq!(snapshot.game_speed, 4);
        assert_eq!(snapshot.credits, 7500);
        assert_eq!(snapshot.unit_count, 5);
        assert!(snapshot.short_game);
        assert!(!snapshot.super_weapons_allowed);
        assert!(snapshot.build_off_ally);
        assert!(!snapshot.mcv_repacks);
        assert!(snapshot.crates_appear, "unknown bool keeps caller default");
        assert_eq!(
            snapshot.slots[0],
            SkirmishPersistedSlot {
                row_type: 4,
                country: 8,
                colour: 1,
            }
        );
        assert_eq!(
            snapshot.slots[1],
            SkirmishPersistedSlot {
                row_type: 5,
                country: -2,
                colour: -2,
            }
        );
    }

    #[test]
    fn slot_tokenizer_leaves_missing_outputs_at_native_defaults() {
        let input = b"[Skirmish]\nSlot01=4,,8\nSlot02=\nSlot03=junk,7\n";
        let snapshot = read_skirmish_snapshot(input, defaults()).unwrap();
        assert_eq!(snapshot.slots[0].row_type, 4);
        assert_eq!(
            snapshot.slots[0].country, 8,
            "strtok collapses empty fields"
        );
        assert_eq!(snapshot.slots[0].colour, -2);
        assert_eq!(snapshot.slots[1], SkirmishPersistedSlot::native_default(1));
        assert_eq!(snapshot.slots[2].row_type, 0, "atoi(non-number) is zero");
        assert_eq!(snapshot.slots[2].country, 7);
        assert_eq!(snapshot.slots[2].colour, -2);
    }

    #[test]
    fn writes_native_key_order_boolean_spelling_and_slot_format() {
        let mut snapshot = SkirmishPersistedSnapshot::from_global_defaults(defaults());
        snapshot.game_mode = 3;
        snapshot.scenario_index = 162;
        snapshot.short_game = false;
        snapshot.slots[0] = SkirmishPersistedSlot {
            row_type: 6,
            country: 8,
            colour: 1,
        };
        let output = String::from_utf8(snapshot.update_ini_bytes(b"")).unwrap();
        let expected = "[Skirmish]\r\nGameMode=3\r\nScenIndex=162\r\nGameSpeed=1\r\n\
Credits=10000\r\nUnitCount=10\r\nShortGame=no\r\nSuperWeaponsAllowed=yes\r\n\
BuildOffAlly=yes\r\nMCVRepacks=yes\r\nCratesAppear=yes\r\nSlot01=6,8,1\r\n\
Slot02=1,-2,-2\r\nSlot03=1,-2,-2\r\nSlot04=1,-2,-2\r\n\
Slot05=1,-2,-2\r\nSlot06=1,-2,-2\r\nSlot07=1,-2,-2\r\n";
        assert_eq!(output, expected);
    }

    #[test]
    fn batched_snapshot_update_preserves_unrelated_bytes() {
        let mut snapshot = SkirmishPersistedSnapshot::from_global_defaults(defaults());
        snapshot.game_mode = 9;
        let input = b"; preface\n[Skirmish]\nGameMode=1\n; keep\n[Other]\n\xff=value\n";
        let output = snapshot.update_ini_bytes(input);
        assert!(output.starts_with(b"; preface\n[Skirmish]\nGameMode=9\n"));
        assert!(output.ends_with(b"[Other]\n\xff=value\n"));
        assert_eq!(output.iter().filter(|byte| **byte == 0xff).count(), 1);
    }

    #[test]
    fn batched_update_and_reader_agree_on_duplicate_key_value() {
        let mut snapshot = SkirmishPersistedSnapshot::from_global_defaults(defaults());
        snapshot.game_mode = 3;
        let updated = snapshot
            .update_ini_bytes(b"[Skirmish]\r\nGameMode=1\r\nGameMode=8\r\n[Other]\r\nValue=1\r\n");

        let reloaded = read_skirmish_snapshot(&updated, defaults()).expect("re-read snapshot");

        assert_eq!(reloaded.game_mode, 3);
        assert!(
            updated
                .windows(b"GameMode=1".len())
                .any(|window| window == b"GameMode=1")
        );
    }

    #[test]
    fn raw_reader_ignores_non_utf8_bytes_outside_target_section() {
        let input = b"[MultiPlayer]\r\nHandle=Jos\xe9\r\n[Skirmish]\r\nGameMode=3\r\n\
Slot01=4,8,1\r\n";
        let snapshot = read_skirmish_snapshot(input, defaults()).unwrap();
        assert_eq!(snapshot.game_mode, 3);
        assert_eq!(snapshot.slots[0].row_type, 4);
        assert_eq!(snapshot.slots[0].country, 8);
        assert_eq!(snapshot.slots[0].colour, 1);
    }
}
