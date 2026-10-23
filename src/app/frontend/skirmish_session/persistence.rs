//! Raw `[Skirmish]` snapshot codec for `VERA.INI`.
//!
//! The app owns when the snapshot is loaded and saved. This module only models
//! the native key order, defaults, slot triples, and a preservation-safe batched
//! byte update. It deliberately has no UI or `AppState` dependency.

use crate::rules::error::RulesError;
use crate::rules::ini_parser::{IniFile, IniSection};
use crate::rules::ini_value::crt_atoi;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SetupProfile {
    Skirmish,
    Lan,
    /// Session+0x284 (`0xA8B4BC`), read at `00698473`, written at `006991EF`.
    Wol,
}
impl SetupProfile {
    const fn section(self) -> &'static str {
        match self {
            Self::Skirmish => "Skirmish",
            Self::Lan => "LAN",
            Self::Wol => "WonlinePref",
        }
    }
}

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
pub struct MultiplayerSetupSnapshot {
    profile: SetupProfile,
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

impl MultiplayerSetupSnapshot {
    pub fn from_global_defaults(defaults: SkirmishGlobalDefaults, profile: SetupProfile) -> Self {
        Self {
            profile,
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
            slots: std::array::from_fn(|index| {
                let mut slot = SkirmishPersistedSlot::native_default(index);
                // 00698456/0069846F: LAN and WOL read with defaults 2/2.
                if profile != SetupProfile::Skirmish {
                    slot.row_type = 2;
                }
                slot
            }),
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

    /// Apply this snapshot to raw `VERA.INI` bytes without rewriting any
    /// unrelated byte. All seventeen keys are composed into one output buffer.
    pub fn update_ini_bytes(&self, content: &[u8]) -> Vec<u8> {
        let updates = self.ini_updates();
        let borrowed: Vec<(&str, &str)> = updates
            .iter()
            .map(|(key, value)| (key.as_str(), value.as_str()))
            .collect();
        crate::util::ini_writer::set_ini_values(content, self.profile.section(), &borrowed)
    }
}

/// Read a raw `VERA.INI` buffer using native Skirmish key order and defaults.
pub fn read_setup_snapshot(
    content: &[u8],
    defaults: SkirmishGlobalDefaults,
    profile: SetupProfile,
) -> Result<MultiplayerSetupSnapshot, RulesError> {
    let mut snapshot = MultiplayerSetupSnapshot::from_global_defaults(defaults, profile);
    let ini = IniFile::from_bytes(content)?;
    let section = ini.section_or_empty(profile.section());

    snapshot.game_mode = section.read_int("GameMode", snapshot.game_mode);
    snapshot.scenario_index = section.read_int("ScenIndex", snapshot.scenario_index);
    snapshot.game_speed = section.read_int("GameSpeed", snapshot.game_speed);
    snapshot.credits = section.read_int("Credits", snapshot.credits);
    snapshot.unit_count = section.read_int("UnitCount", snapshot.unit_count);
    snapshot.short_game = section.read_bool("ShortGame", snapshot.short_game);
    snapshot.super_weapons_allowed =
        section.read_bool("SuperWeaponsAllowed", snapshot.super_weapons_allowed);
    snapshot.build_off_ally = section.read_bool("BuildOffAlly", snapshot.build_off_ally);
    snapshot.mcv_repacks = section.read_bool("MCVRepacks", snapshot.mcv_repacks);
    snapshot.crates_appear = section.read_bool("CratesAppear", snapshot.crates_appear);

    for (slot_index, slot) in snapshot.slots.iter_mut().enumerate() {
        read_native_slot_triple(section, &format!("Slot{:02}", slot_index + 1), slot);
    }
    Ok(snapshot)
}

pub(super) fn yes_no(value: bool) -> &'static str {
    if value { "yes" } else { "no" }
}

/// `FUN_00477440`: `ReadString` into 0x200 bytes, then `strtok(",")` + `atoi`
/// up to three times, leaving absent outputs unchanged.
fn read_native_slot_triple(section: &IniSection, key: &str, output: &mut SkirmishPersistedSlot) {
    let Some(tokens) = section.read_list(key, 0x200) else {
        return;
    };
    let fields = [
        &mut output.row_type,
        &mut output.country,
        &mut output.colour,
    ];
    for (field, token) in fields.into_iter().zip(tokens) {
        *field = crt_atoi(token);
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
    fn wol_profile_reads_wonlinepref_with_the_network_open_defaults() {
        let input = b"[LAN]\nSlot01=4,-2,-2\n[WonlinePref]\nSlot02=3,-2,-2\n";
        let wol = read_setup_snapshot(input, defaults(), SetupProfile::Wol).unwrap();
        assert_eq!(wol.slots[0].row_type, 2);
        assert_eq!(wol.slots[1].row_type, 3);
        let updated = wol.update_ini_bytes(input);
        assert!(
            String::from_utf8_lossy(&updated).contains("[LAN]\nSlot01=4,-2,-2"),
            "the WOL writer leaves the LAN profile alone"
        );
    }

    #[test]
    fn lan_profile_has_distinct_open_defaults_and_preserves_skirmish_bytes() {
        let input = b"[Skirmish]\nSlot01=6,2,3\nCredits=7500\n[LAN]\nSlot02=3,8,7\n";
        let snapshot = read_setup_snapshot(input, defaults(), SetupProfile::Lan).unwrap();
        assert_eq!(
            snapshot.slots[0],
            SkirmishPersistedSlot {
                row_type: 2,
                country: -2,
                colour: -2
            }
        );
        assert_eq!(
            snapshot.slots[1],
            SkirmishPersistedSlot {
                row_type: 3,
                country: 8,
                colour: 7
            }
        );
        let updated = snapshot.update_ini_bytes(input);
        let offline = read_setup_snapshot(&updated, defaults(), SetupProfile::Skirmish).unwrap();
        assert_eq!(offline.credits, 7500);
        assert_eq!(
            offline.slots[0],
            SkirmishPersistedSlot {
                row_type: 6,
                country: 2,
                colour: 3
            }
        );
        assert_eq!(
            read_setup_snapshot(&updated, defaults(), SetupProfile::Lan).unwrap(),
            snapshot
        );
    }

    #[test]
    fn absent_section_uses_caller_globals_and_native_slot_defaults() {
        let snapshot =
            read_setup_snapshot(b"[Options]\nFoo=bar\n", defaults(), SetupProfile::Skirmish)
                .unwrap();
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
        let snapshot = read_setup_snapshot(input, defaults(), SetupProfile::Skirmish).unwrap();
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
        let snapshot = read_setup_snapshot(input, defaults(), SetupProfile::Skirmish).unwrap();
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
        let mut snapshot =
            MultiplayerSetupSnapshot::from_global_defaults(defaults(), SetupProfile::Skirmish);
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
        let mut snapshot =
            MultiplayerSetupSnapshot::from_global_defaults(defaults(), SetupProfile::Skirmish);
        snapshot.game_mode = 9;
        let input = b"; preface\n[Skirmish]\nGameMode=1\n; keep\n[Other]\n\xff=value\n";
        let output = snapshot.update_ini_bytes(input);
        assert!(output.starts_with(b"; preface\n[Skirmish]\nGameMode=9\n"));
        assert!(output.ends_with(b"[Other]\n\xff=value\n"));
        assert_eq!(output.iter().filter(|byte| **byte == 0xff).count(), 1);
    }

    #[test]
    fn batched_update_and_reader_agree_on_duplicate_key_value() {
        let mut snapshot =
            MultiplayerSetupSnapshot::from_global_defaults(defaults(), SetupProfile::Skirmish);
        snapshot.game_mode = 3;
        let updated = snapshot
            .update_ini_bytes(b"[Skirmish]\r\nGameMode=1\r\nGameMode=8\r\n[Other]\r\nValue=1\r\n");

        let reloaded = read_setup_snapshot(&updated, defaults(), SetupProfile::Skirmish)
            .expect("re-read snapshot");

        assert_eq!(reloaded.game_mode, 3);
        assert!(
            updated
                .windows(b"GameMode=8".len())
                .any(|window| window == b"GameMode=8")
        );
    }

    #[test]
    fn reader_widens_non_utf8_bytes_outside_target_section() {
        let input = b"[MultiPlayer]\r\nHandle=Jos\xe9\r\n[Skirmish]\r\nGameMode=3\r\n\
Slot01=4,8,1\r\n";
        let snapshot = read_setup_snapshot(input, defaults(), SetupProfile::Skirmish).unwrap();
        assert_eq!(snapshot.game_mode, 3);
        assert_eq!(snapshot.slots[0].row_type, 4);
        assert_eq!(snapshot.slots[0].country, 8);
        assert_eq!(snapshot.slots[0].colour, 1);
    }
}
