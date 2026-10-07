//! Retail keyboard binding loading and logical-key dispatch.
//!
//! This app-layer module translates winit's logical keys into the encoded
//! Win32 key words stored in `KEYBOARDMD.INI`. It deliberately stops at
//! semantic command names; gameplay effects remain in `input::dispatch`.

use std::collections::BTreeMap;

use winit::keyboard::{Key, KeyLocation, ModifiersState, NamedKey};

use crate::assets::asset_manager::AssetManager;
use crate::rules::ini_parser::IniFile;

pub(crate) mod catalog;

const SHIFT_BIT: u16 = 0x100;
const CTRL_BIT: u16 = 0x200;
const ALT_BIT: u16 = 0x400;
const VK_ESCAPE: u16 = 0x1b;
const VK_SPACE: u16 = 0x20;
const VK_DELETE: u16 = 0x2e;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum HotkeyCommand {
    CenterView,
    Options,
    CenterOnRadarEvent,
    TeamSelect(usize),
    TeamAddSelect(usize),
    TeamCreate(usize),
    TeamCenter(usize),
    ToggleAlliance,
    PlaceBeacon,
    AllToCheer,
    DeployObject,
    InfantryTab,
    Follow,
    GuardObject,
    CenterBase,
    ToggleRepair,
    ToggleSell,
    PreviousObject,
    NextObject,
    CombatantSelect,
    StructureTab,
    UnitTab,
    StopObject,
    TypeSelect,
    PageUser,
    DefenseTab,
    ScatterObject,
    VeterancyNav,
    HealthNav,
    CursorCheat,
    PlanningMode,
    SidebarDown,
    SidebarUp,
    Delete,
    View(usize),
    Taunt(usize),
    ScreenCapture,
    SetView(usize),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum HotkeyFallback {
    DiplomacyDialog,
    ArrowLeft,
    ArrowUp,
    ArrowRight,
    ArrowDown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum HotkeyResolution {
    Command(HotkeyCommand),
    Fallback(HotkeyFallback),
    Unhandled,
}

impl HotkeyCommand {
    /// Native `CommandClass::AcceptsModifiers`, vtable slot +0x14, consulted by
    /// the FIRST (base virtual-key) lookup in `Process_Command` 0x0055DEE0.
    ///
    /// The shared default body at 0x00535BD0 is `xor al,al; ret 4` — always
    /// false — and 42 of the 47 command vtables point their +0x14 slot at it.
    /// A rejected first lookup falls through to the second lookup keyed on
    /// `key | modifiers`, which for a bare press is the same word, so "default"
    /// is observationally "fires only when no modifier is held".
    ///
    /// Five classes override the slot, and they are why this cannot be one
    /// blanket rule. `TypeSelect` (0x00536880), `CombatantSelect` (0x005367E0),
    /// `VeterancyNav` (0x005369E0) and `HealthNav` (0x00536940) share the body
    /// `mov eax,[esp+4]; shr eax,8; and eax,1; ret 4` — raw bit 8, the Shift
    /// bit, ignoring Ctrl and Alt. `PlanningMode` (0x00536710) is
    /// `mov eax,[esp+4]; test ah,7; ...` — true when any of Shift/Ctrl/Alt is
    /// held. Those five keep firing from their bare-key binding with the
    /// modifier down; the visible one in ordinary play is Shift+T, which stays
    /// a type-select instead of being swallowed.
    ///
    /// Identified by walking the +0x14 data refs to 0x00535BD0 (42 hits, stride
    /// 0x28 = one 9-slot vtable plus its RTTI word), then reading each gap
    /// vtable's +0x04 `GetName` thunk back to its INI-name string. The set is
    /// closed: the grid holds exactly 47 CommandClass vtables, so 42 defaults
    /// plus these 5 account for all of them. `HealthNav` is registered even
    /// though the stock INI leaves it unbound.
    ///
    /// Consequence for VERA's dev chord: Ctrl+Shift+P now resolves to the retail
    /// `CombatantSelect` instead of falling through to the pathgrid overlay
    /// toggle, which keeps its F9 binding.
    fn accepts_base_modifiers(self, modifiers: ModifiersState) -> bool {
        match self {
            Self::TypeSelect | Self::CombatantSelect | Self::VeterancyNav | Self::HealthNav => {
                modifiers.shift_key()
            }
            Self::PlanningMode => modifier_bits(modifiers) != 0,
            _ => modifier_bits(modifiers) == 0,
        }
    }

    /// Exact native vtable +0x14 predicate, also used by assignment at
    /// 48BB40/48BB60. Unlike the dispatch convenience above, the default is false.
    fn accepts_native_modifiers(self, encoded: u16) -> bool {
        match self {
            Self::TypeSelect | Self::CombatantSelect | Self::VeterancyNav | Self::HealthNav => {
                encoded & SHIFT_BIT != 0
            }
            Self::PlanningMode => encoded & (SHIFT_BIT | CTRL_BIT | ALT_BIT) != 0,
            _ => false,
        }
    }
}

impl HotkeyFallback {
    pub(crate) fn arrow_key_code(self) -> Option<winit::keyboard::KeyCode> {
        use winit::keyboard::KeyCode;
        match self {
            Self::ArrowLeft => Some(KeyCode::ArrowLeft),
            Self::ArrowUp => Some(KeyCode::ArrowUp),
            Self::ArrowRight => Some(KeyCode::ArrowRight),
            Self::ArrowDown => Some(KeyCode::ArrowDown),
            Self::DiplomacyDialog => None,
        }
    }
}

pub(crate) fn fallback_scroll_key(
    resolution: HotkeyResolution,
) -> Option<winit::keyboard::KeyCode> {
    match resolution {
        HotkeyResolution::Fallback(fallback) => fallback.arrow_key_code(),
        HotkeyResolution::Command(_) | HotkeyResolution::Unhandled => None,
    }
}

pub(crate) fn physical_scroll_key(
    physical: winit::keyboard::KeyCode,
) -> Option<winit::keyboard::KeyCode> {
    use winit::keyboard::KeyCode;
    match physical {
        KeyCode::ArrowLeft | KeyCode::Numpad4 => Some(KeyCode::ArrowLeft),
        KeyCode::ArrowUp | KeyCode::Numpad8 => Some(KeyCode::ArrowUp),
        KeyCode::ArrowRight | KeyCode::Numpad6 => Some(KeyCode::ArrowRight),
        KeyCode::ArrowDown | KeyCode::Numpad2 => Some(KeyCode::ArrowDown),
        _ => None,
    }
}

#[derive(Debug, Clone, Default)]
pub(crate) struct HotkeyBindings {
    by_key: BTreeMap<u16, HotkeyCommand>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BindingAssignmentError {
    CannotMap,
    CannotRemap,
}

impl HotkeyBindings {
    pub(crate) fn load(assets: Option<&AssetManager>) -> Self {
        Self::from_ini_bytes(assets.and_then(|assets| assets.get_ref("KEYBOARDMD.INI")))
    }

    fn from_ini_bytes(bytes: Option<&[u8]>) -> Self {
        let mut bindings = Self::reload_from_ini_bytes(bytes);
        // Startup 532150 calls reload 533D20, then installs these three keys.
        // Cancel/reset use reload alone and must not silently reinstall them.
        bindings.by_key.insert(VK_DELETE, HotkeyCommand::Delete);
        bindings.by_key.insert(VK_ESCAPE, HotkeyCommand::Options);
        bindings
            .by_key
            .insert(VK_SPACE, HotkeyCommand::CenterOnRadarEvent);
        bindings
    }

    pub(crate) fn reload_from_ini_bytes(bytes: Option<&[u8]>) -> Self {
        let mut bindings = Self::default();
        if let Some(section) = bytes
            .and_then(|bytes| IniFile::from_bytes(bytes).ok())
            .and_then(|ini| ini.section("Hotkey").cloned())
        {
            for name in section.keys() {
                let Some(command) = command_from_name(name) else {
                    continue;
                };
                // Reload `0x00533D20`: ReadInt over 0 for each entry name.
                let Some(encoded) = Some(section.read_int(name, 0))
                    .filter(|value| *value != 0)
                    .and_then(|value| u16::try_from(value).ok())
                else {
                    continue;
                };
                bindings.by_key.insert(encoded, command);
            }
        }

        bindings
    }

    pub(crate) fn command_at(&self, encoded: u16) -> Option<HotkeyCommand> {
        self.by_key.get(&encoded).copied()
    }

    /// The current-shortcut label and replacement both scan ascending keys and
    /// stop at the first match (5FB408..5FB44A and 5FBBAC). Additional bindings survive.
    pub(crate) fn first_key(&self, command: HotkeyCommand) -> Option<u16> {
        self.by_key
            .iter()
            .find_map(|(&key, &owner)| (owner == command).then_some(key))
    }

    /// Original 5FBB38..5FBDF1; executable comparisons in keyboard_bindings.json.
    /// Zero unassigns the first existing shortcut. Rejections leave all bindings intact.
    pub(crate) fn assign(
        &mut self,
        command: HotkeyCommand,
        encoded: u16,
    ) -> Result<(), BindingAssignmentError> {
        if encoded & (SHIFT_BIT | CTRL_BIT | ALT_BIT) != 0 {
            if command.accepts_native_modifiers(encoded) {
                return Err(BindingAssignmentError::CannotMap);
            }
            if self
                .command_at(encoded & 0xff)
                .is_some_and(|owner| owner.accepts_native_modifiers(encoded))
            {
                return Err(BindingAssignmentError::CannotRemap);
            }
        }
        if let Some(key) = self.first_key(command) {
            self.by_key.remove(&key);
        }
        if encoded != 0 {
            self.by_key.insert(encoded, command);
        }
        Ok(())
    }

    /// Back's fresh INI writer (5FB900..5FB9D8) visits ascending encoded keys.
    /// Repeated command names overwrite their earlier value, retaining the
    /// first insertion position. Thus the highest key survives disk round-trip.
    pub(crate) fn to_ini_string(&self) -> String {
        use std::fmt::Write;
        let mut entries: Vec<(&str, u16)> = Vec::new();
        for (&key, &command) in &self.by_key {
            let Some(metadata) = catalog::registered_commands()
                .iter()
                .find(|row| row.command == command)
            else {
                continue;
            };
            if let Some((_, value)) = entries
                .iter_mut()
                .find(|(name, _)| *name == metadata.ini_name)
            {
                *value = key;
            } else {
                entries.push((metadata.ini_name, key));
            }
        }
        let mut output = String::from("[Hotkey]\n");
        for (name, key) in entries {
            writeln!(output, "{name}={key}").expect("writing a String cannot fail");
        }
        output
    }

    pub(crate) fn resolve_event(
        &self,
        logical_key: &Key,
        location: KeyLocation,
        modifiers: ModifiersState,
    ) -> HotkeyResolution {
        // Native removes the 0x800 release bit from both lookup identities and
        // passes the raw edge only to CanExecute; ordinary receivers reject
        // releases. The app therefore resolves one edge-neutral identity and
        // models press/release only for the hardcoded held-arrow fallback.
        let Some(virtual_key) = logical_virtual_key(logical_key, location) else {
            return HotkeyResolution::Unhandled;
        };
        if let Some(command) = self.by_key.get(&virtual_key).copied()
            && command.accepts_base_modifiers(modifiers)
        {
            return HotkeyResolution::Command(command);
        }
        if let Some(command) = self
            .by_key
            .get(&(virtual_key | modifier_bits(modifiers)))
            .copied()
        {
            return HotkeyResolution::Command(command);
        }
        fallback_for_virtual_key(virtual_key)
            .map(HotkeyResolution::Fallback)
            .unwrap_or(HotkeyResolution::Unhandled)
    }

    #[cfg(test)]
    fn resolve(
        &self,
        logical_key: &Key,
        location: KeyLocation,
        modifiers: ModifiersState,
    ) -> Option<HotkeyCommand> {
        match self.resolve_event(logical_key, location, modifiers) {
            HotkeyResolution::Command(command) => Some(command),
            HotkeyResolution::Fallback(_) | HotkeyResolution::Unhandled => None,
        }
    }
}

fn fallback_for_virtual_key(virtual_key: u16) -> Option<HotkeyFallback> {
    Some(match virtual_key {
        0x09 => HotkeyFallback::DiplomacyDialog,
        0x25 => HotkeyFallback::ArrowLeft,
        0x26 => HotkeyFallback::ArrowUp,
        0x27 => HotkeyFallback::ArrowRight,
        0x28 => HotkeyFallback::ArrowDown,
        _ => return None,
    })
}

/// Shell controls observe the live message pump while gameplay retains its
/// paused admission snapshot. A3 capture must not latch a modifier into gameplay
/// when its key-up occurs later in the paused parent.
pub(crate) fn record_modifier_event(
    live: &mut ModifiersState,
    gameplay: &mut ModifiersState,
    incoming: ModifiersState,
    paused: bool,
) {
    *live = incoming;
    if !paused {
        *gameplay = incoming;
    }
}

pub(crate) fn modifier_bits(modifiers: ModifiersState) -> u16 {
    (if modifiers.shift_key() { SHIFT_BIT } else { 0 })
        | (if modifiers.control_key() { CTRL_BIT } else { 0 })
        | (if modifiers.alt_key() { ALT_BIT } else { 0 })
}

pub(crate) fn input_admitted_while_paused(paused: bool, key: &Key) -> bool {
    !paused || matches!(key, Key::Named(NamedKey::Escape))
}

/// Windows derives `key_without_modifiers` with NumLock forced off. Preserve
/// the raw logical keypad identity, while standard keys use the modifier-free
/// value needed for bindings such as Shift+1.
pub(crate) fn binding_logical_key<'a>(
    raw: &'a Key,
    without_modifiers: &'a Key,
    location: KeyLocation,
) -> &'a Key {
    if location == KeyLocation::Numpad {
        raw
    } else {
        without_modifiers
    }
}

pub(crate) fn logical_virtual_key(key: &Key, location: KeyLocation) -> Option<u16> {
    match key {
        Key::Character(text) if text.chars().count() == 1 => {
            let character = text.chars().next()?.to_ascii_uppercase();
            if location == KeyLocation::Numpad {
                if character.is_ascii_digit() {
                    return Some(0x60 + (character as u16 - '0' as u16));
                }
                match character {
                    '.' | ',' => return Some(0x6e),
                    '*' => return Some(0x6a),
                    '+' => return Some(0x6b),
                    '-' => return Some(0x6d),
                    '/' => return Some(0x6f),
                    _ => {}
                }
            }
            if character.is_ascii_alphanumeric() {
                Some(character as u16)
            } else {
                printable_virtual_key(character)
            }
        }
        Key::Named(named) => named_virtual_key(*named),
        _ => None,
    }
}

/// Native receives a Win32 virtual key before 55DEE0 dispatch. Winit supplies
/// the modifier-free logical character instead, so OEM punctuation and ordinary
/// national-layout letters must be mapped through the current Windows layout.
/// VkKeyScanW's high-byte modifier recipe is not an event modifier state; the
/// caller already owns the actual Shift/Ctrl/Alt bits.
#[cfg(windows)]
fn printable_virtual_key(character: char) -> Option<u16> {
    #[link(name = "user32")]
    unsafe extern "system" {
        fn VkKeyScanW(character: u16) -> i16;
    }
    let character = u16::try_from(character as u32).ok()?;
    // SAFETY: VkKeyScanW takes one UTF-16 code unit and no pointers.
    let encoded = unsafe { VkKeyScanW(character) };
    (encoded != -1).then_some(encoded as u16 & 0xff)
}

#[cfg(not(windows))]
fn printable_virtual_key(_character: char) -> Option<u16> {
    None
}

fn named_virtual_key(key: NamedKey) -> Option<u16> {
    Some(match key {
        NamedKey::Backspace => 0x08,
        NamedKey::Tab => 0x09,
        NamedKey::Clear => 0x0c,
        NamedKey::Enter => 0x0d,
        NamedKey::Shift => 0x10,
        NamedKey::Control => 0x11,
        NamedKey::Alt => 0x12,
        NamedKey::Escape => VK_ESCAPE,
        NamedKey::Space => VK_SPACE,
        NamedKey::PageUp => 0x21,
        NamedKey::PageDown => 0x22,
        NamedKey::End => 0x23,
        NamedKey::Home => 0x24,
        NamedKey::ArrowLeft => 0x25,
        NamedKey::ArrowUp => 0x26,
        NamedKey::ArrowRight => 0x27,
        NamedKey::ArrowDown => 0x28,
        NamedKey::Insert => 0x2d,
        NamedKey::Delete => VK_DELETE,
        NamedKey::F1 => 0x70,
        NamedKey::F2 => 0x71,
        NamedKey::F3 => 0x72,
        NamedKey::F4 => 0x73,
        NamedKey::F5 => 0x74,
        NamedKey::F6 => 0x75,
        NamedKey::F7 => 0x76,
        NamedKey::F8 => 0x77,
        NamedKey::F9 => 0x78,
        NamedKey::F10 => 0x79,
        NamedKey::F11 => 0x7a,
        NamedKey::F12 => 0x7b,
        _ => return None,
    })
}

fn command_from_name(name: &str) -> Option<HotkeyCommand> {
    // Reload 533DE4..533E29 walks registered vtable+4 names and compares exact
    // bytes. Numbered commands are registered names, not a permissive grammar.
    catalog::registered_commands()
        .iter()
        .find(|row| row.ini_name == name)
        .map(|row| row.command)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn native_keyboard_fixture() -> serde_json::Value {
        serde_json::from_str(crate::test_fixture::text(
            "tools/storage_oracle/keyboard_bindings.json",
        ))
        .expect("preserved original keyboard fixture")
    }

    #[test]
    fn registered_catalog_matches_original_objects_and_metadata_getters() {
        let fixture = native_keyboard_fixture();
        let native = fixture["catalog"].as_array().unwrap();
        let catalog = catalog::registered_commands();
        assert_eq!(catalog.len(), 87);
        assert_eq!(catalog.len(), native.len());
        for (index, (actual, expected)) in catalog.iter().zip(native).enumerate() {
            assert_eq!(expected["index"].as_u64(), Some(index as u64));
            assert_eq!(actual.ini_name, expected["ini_name"].as_str().unwrap());
            assert_eq!(actual.name_key, expected["name_key"].as_str().unwrap());
            assert_eq!(
                actual.category_key,
                expected["category_key"].as_str().unwrap()
            );
            assert_eq!(
                actual.description_key,
                expected["description_key"].as_str().unwrap()
            );
            assert_eq!(
                actual.parameter.map(u64::from),
                expected["parameter"].as_u64()
            );
            assert_eq!(command_from_name(actual.ini_name), Some(actual.command));
            assert!(
                !catalog[..index]
                    .iter()
                    .any(|row| row.command == actual.command)
            );
        }
    }

    #[test]
    fn every_original_registered_name_survives_reload_and_serialization() {
        use std::fmt::Write;
        let fixture = native_keyboard_fixture();
        let mut ini = String::from("[Hotkey]\n");
        for row in fixture["catalog"].as_array().unwrap() {
            let key = 0x200 + row["index"].as_u64().unwrap();
            writeln!(ini, "{}={key}", row["ini_name"].as_str().unwrap()).unwrap();
        }
        let loaded = HotkeyBindings::reload_from_ini_bytes(Some(ini.as_bytes()));
        assert_eq!(loaded.by_key.len(), 87);
        for (index, metadata) in catalog::registered_commands().iter().enumerate() {
            assert_eq!(
                loaded.command_at(0x200 + index as u16),
                Some(metadata.command)
            );
        }
        let saved = loaded.to_ini_string();
        assert_eq!(saved, ini);
        let reloaded = HotkeyBindings::reload_from_ini_bytes(Some(saved.as_bytes()));
        assert_eq!(reloaded.by_key, loaded.by_key);
    }

    #[test]
    fn reload_rejects_numeric_aliases_absent_from_original_registered_names() {
        use std::fmt::Write;
        // Native533DF4..533E1D compares bytes with each original registered name.
        // These are former Rust parser aliases, not additional registered commands.
        let aliases = [
            "TeamSelect_0",
            "TeamCreate_01",
            "TeamAddSelect_+1",
            "TeamCenter_00",
            "View01",
            "SetView+1",
            "Taunt_01",
        ];
        let fixture = native_keyboard_fixture();
        let original = fixture["catalog"].as_array().unwrap();
        let mut ini = String::from("[Hotkey]\n");
        for (index, alias) in aliases.iter().enumerate() {
            assert!(
                !original
                    .iter()
                    .any(|row| row["ini_name"].as_str() == Some(alias))
            );
            assert_eq!(command_from_name(alias), None);
            writeln!(ini, "{alias}={}", 0x200 + index).unwrap();
        }
        assert!(
            HotkeyBindings::reload_from_ini_bytes(Some(ini.as_bytes()))
                .by_key
                .is_empty()
        );
        // The stock team10 -> engine slot0 mapping remains valid through metadata.
        assert_eq!(
            command_from_name("TeamSelect_10"),
            Some(HotkeyCommand::TeamSelect(0))
        );
    }

    #[test]
    fn editable_bindings_match_original_assignment_execution() {
        let fixture = native_keyboard_fixture();
        let assignments = fixture["assignments"].as_array().unwrap();
        assert_eq!(assignments.len(), 73);
        for case in assignments {
            let mut bindings = HotkeyBindings::default();
            for row in case["before"].as_array().unwrap() {
                bindings.by_key.insert(
                    row[0].as_u64().unwrap() as u16,
                    command_from_name(row[1].as_str().unwrap()).unwrap(),
                );
            }
            let command = command_from_name(case["command"].as_str().unwrap()).unwrap();
            let expected_error = match case["error"].as_str() {
                Some("CannotMap") => Some(BindingAssignmentError::CannotMap),
                Some("CannotRemap") => Some(BindingAssignmentError::CannotRemap),
                None => None,
                other => panic!("unexpected fixture rejection {other:?}"),
            };
            assert_eq!(
                bindings
                    .assign(command, case["encoded"].as_u64().unwrap() as u16)
                    .err(),
                expected_error,
                "{}",
                case["name"]
            );
            let expected: BTreeMap<_, _> = case["after"]
                .as_array()
                .unwrap()
                .iter()
                .map(|row| {
                    (
                        row[0].as_u64().unwrap() as u16,
                        command_from_name(row[1].as_str().unwrap()).unwrap(),
                    )
                })
                .collect();
            assert_eq!(bindings.by_key, expected, "{}", case["name"]);
            assert_eq!(
                bindings.first_key(command),
                expected
                    .iter()
                    .find_map(|(&key, &owner)| (owner == command).then_some(key)),
                "{}",
                case["name"]
            );
        }
    }

    #[test]
    fn serialization_collapses_duplicate_commands_and_reload_does_not_force_keys() {
        let startup =
            HotkeyBindings::from_ini_bytes(Some(b"[Hotkey]\nDelete=110\nStopObject=83\n"));
        assert_eq!(startup.first_key(HotkeyCommand::Delete), Some(46));
        let serialized = startup.to_ini_string();
        assert_eq!(
            serialized,
            "[Hotkey]\nOptions=27\nCenterOnRadarEvent=32\nDelete=110\nStopObject=83\n"
        );
        let mut reloaded = HotkeyBindings::reload_from_ini_bytes(Some(serialized.as_bytes()));
        assert_eq!(reloaded.command_at(46), None);
        assert_eq!(reloaded.first_key(HotkeyCommand::Delete), Some(110));
        reloaded.assign(HotkeyCommand::Options, 0).unwrap();
        reloaded
            .assign(HotkeyCommand::CenterOnRadarEvent, 0)
            .unwrap();
        let serialized = reloaded.to_ini_string();
        let reloaded = HotkeyBindings::reload_from_ini_bytes(Some(serialized.as_bytes()));
        assert_eq!(reloaded.command_at(VK_ESCAPE), None);
        assert_eq!(reloaded.command_at(VK_SPACE), None);
        assert_eq!(reloaded.command_at(110), Some(HotkeyCommand::Delete));
        assert_eq!(reloaded.command_at(83), Some(HotkeyCommand::StopObject));
    }

    #[test]
    fn keypad_decimal_and_operators_keep_their_win32_identities() {
        for (text, expected) in [
            (".", 110),
            (",", 110),
            ("*", 106),
            ("+", 107),
            ("-", 109),
            ("/", 111),
        ] {
            let mut bindings = HotkeyBindings::default();
            bindings
                .assign(HotkeyCommand::StopObject, expected)
                .unwrap();
            assert_eq!(
                logical_virtual_key(&character(text), KeyLocation::Numpad),
                Some(expected)
            );
            assert_eq!(
                bindings.resolve(
                    &character(text),
                    KeyLocation::Numpad,
                    ModifiersState::empty()
                ),
                Some(HotkeyCommand::StopObject)
            );
        }
        // NumLock-off Delete is VK_DELETE even on the keypad; it is not VK_DECIMAL.
        for location in [KeyLocation::Standard, KeyLocation::Numpad] {
            assert_eq!(
                logical_virtual_key(&Key::Named(NamedKey::Delete), location),
                Some(46)
            );
        }
    }

    #[cfg(windows)]
    #[test]
    fn ordinary_oem_bindings_dispatch_using_preserved_host_keyboard_mapping() {
        #[link(name = "user32")]
        unsafe extern "system" {
            fn GetKeyboardLayoutNameW(name: *mut u16) -> i32;
        }
        let fixture: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
            "tools/storage_oracle/keyboard_key_names.json",
        ))
        .unwrap();
        let mut layout = [0u16; 9];
        // SAFETY: Windows writes at most KL_NAMELENGTH (9) UTF-16 units.
        assert_ne!(unsafe { GetKeyboardLayoutNameW(layout.as_mut_ptr()) }, 0);
        let current = String::from_utf16_lossy(&layout[..8]);
        if fixture["host"]["keyboard_layout"].as_str() != Some(current.as_str()) {
            // Fixture coverage is explicitly the captured host layout, not every layout.
            return;
        }
        let mut checked = 0;
        for row in fixture["printable"].as_array().unwrap() {
            let mapping = row["mapping"].as_i64().unwrap();
            if !(0..=255).contains(&mapping) {
                continue; // Only ordinary unmodified characters in this bounded probe.
            }
            let key = character(row["character"].as_str().unwrap());
            let mut bindings = HotkeyBindings::default();
            bindings
                .assign(HotkeyCommand::StopObject, mapping as u16 | CTRL_BIT)
                .unwrap();
            assert_eq!(
                bindings.resolve(&key, KeyLocation::Standard, ModifiersState::CONTROL),
                Some(HotkeyCommand::StopObject),
                "{}",
                row["character"]
            );
            assert_eq!(
                bindings.resolve(&key, KeyLocation::Standard, ModifiersState::empty()),
                None
            );
            checked += 1;
        }
        assert!(checked >= 5);
    }

    fn modifiers(shift: bool, ctrl: bool, alt: bool) -> ModifiersState {
        let mut value = ModifiersState::empty();
        value.set(ModifiersState::SHIFT, shift);
        value.set(ModifiersState::CONTROL, ctrl);
        value.set(ModifiersState::ALT, alt);
        value
    }

    fn character(value: &str) -> Key {
        Key::Character(value.into())
    }

    #[test]
    fn parser_skips_unknown_and_zero_entries() {
        let bindings = HotkeyBindings::from_ini_bytes(Some(
            b"[Hotkey]\nStopObject=83\nUnknownCommand=71\nDeployObject=0\n",
        ));
        assert_eq!(
            bindings.resolve(
                &character("s"),
                KeyLocation::Standard,
                ModifiersState::empty()
            ),
            Some(HotkeyCommand::StopObject)
        );
        assert_eq!(
            bindings.resolve(
                &character("g"),
                KeyLocation::Standard,
                ModifiersState::empty()
            ),
            None
        );
        assert_eq!(
            bindings.resolve(
                &character("d"),
                KeyLocation::Standard,
                ModifiersState::empty()
            ),
            None
        );
    }

    #[test]
    fn inactive_archive_residue_names_are_not_registered_commands() {
        let bindings = HotkeyBindings::from_ini_bytes(Some(
            b"[Hotkey]\nRightSidebarUp=65\nRightSidebarDown=66\nLeftSidebarDown=67\nLeftSidebarUp=68\nRaiseCell=69\nLowerCell=70\nDeleteObject=71\nSidebarPageUp=72\nSidebarPageDown=73\nSidebarDown=98\nSidebarUp=104\n",
        ));
        for letter in ["a", "b", "c", "d", "e", "f", "g", "h", "i"] {
            assert_eq!(
                bindings.resolve_event(
                    &character(letter),
                    KeyLocation::Standard,
                    ModifiersState::empty(),
                ),
                HotkeyResolution::Unhandled
            );
        }
        assert_eq!(
            bindings.resolve_event(
                &character("2"),
                KeyLocation::Numpad,
                ModifiersState::empty(),
            ),
            HotkeyResolution::Command(HotkeyCommand::SidebarDown)
        );
        assert_eq!(
            bindings.resolve_event(
                &character("8"),
                KeyLocation::Numpad,
                ModifiersState::empty(),
            ),
            HotkeyResolution::Command(HotkeyCommand::SidebarUp)
        );
    }

    #[test]
    fn missing_file_has_only_forced_bindings() {
        let bindings = HotkeyBindings::from_ini_bytes(None);
        assert_eq!(
            bindings.resolve(
                &Key::Named(NamedKey::Escape),
                KeyLocation::Standard,
                ModifiersState::empty()
            ),
            Some(HotkeyCommand::Options)
        );
        assert_eq!(
            bindings.resolve(
                &Key::Named(NamedKey::Space),
                KeyLocation::Standard,
                ModifiersState::empty()
            ),
            Some(HotkeyCommand::CenterOnRadarEvent)
        );
        assert_eq!(
            bindings.resolve(
                &character("s"),
                KeyLocation::Standard,
                ModifiersState::empty()
            ),
            None
        );
    }

    #[test]
    fn forced_bindings_override_archive_rows() {
        let bindings = HotkeyBindings::from_ini_bytes(Some(
            b"[Hotkey]\nStopObject=27\nDeployObject=32\nGuardObject=46\n",
        ));
        assert_eq!(
            bindings.resolve(
                &Key::Named(NamedKey::Escape),
                KeyLocation::Standard,
                ModifiersState::empty()
            ),
            Some(HotkeyCommand::Options)
        );
        assert_eq!(
            bindings.resolve(
                &Key::Named(NamedKey::Space),
                KeyLocation::Standard,
                ModifiersState::empty()
            ),
            Some(HotkeyCommand::CenterOnRadarEvent)
        );
        assert_eq!(
            bindings.resolve(
                &Key::Named(NamedKey::Delete),
                KeyLocation::Standard,
                ModifiersState::empty()
            ),
            Some(HotkeyCommand::Delete)
        );
    }

    #[test]
    fn logical_letters_and_numpad_digits_use_distinct_virtual_keys() {
        assert_eq!(
            logical_virtual_key(&character("z"), KeyLocation::Standard),
            Some(0x5a)
        );
        assert_eq!(
            logical_virtual_key(&character("2"), KeyLocation::Standard),
            Some(0x32)
        );
        assert_eq!(
            logical_virtual_key(&character("2"), KeyLocation::Numpad),
            Some(0x62)
        );
        assert_eq!(
            logical_virtual_key(&character("."), KeyLocation::Numpad),
            Some(0x6e)
        );
    }

    #[test]
    fn paused_capture_admits_only_escape() {
        assert!(input_admitted_while_paused(
            true,
            &Key::Named(NamedKey::Escape)
        ));
        assert!(!input_admitted_while_paused(true, &character("s")));
        assert!(input_admitted_while_paused(false, &character("s")));
    }

    #[test]
    fn exact_modifiers_resolve_group_commands_and_reject_two_modifier_chords() {
        let bindings = HotkeyBindings::from_ini_bytes(Some(
            b"[Hotkey]\nTeamSelect_1=49\nTeamAddSelect_1=305\nTeamCreate_1=561\nTeamCenter_1=1073\n",
        ));
        let key = character("1");
        assert_eq!(
            bindings.resolve(&key, KeyLocation::Standard, ModifiersState::empty()),
            Some(HotkeyCommand::TeamSelect(1))
        );
        assert_eq!(
            bindings.resolve(&key, KeyLocation::Standard, modifiers(true, false, false)),
            Some(HotkeyCommand::TeamAddSelect(1))
        );
        assert_eq!(
            bindings.resolve(&key, KeyLocation::Standard, modifiers(false, true, false)),
            Some(HotkeyCommand::TeamCreate(1))
        );
        assert_eq!(
            bindings.resolve(&key, KeyLocation::Standard, modifiers(false, false, true)),
            Some(HotkeyCommand::TeamCenter(1))
        );
        assert_eq!(
            bindings.resolve(&key, KeyLocation::Standard, modifiers(true, true, false)),
            None
        );
    }

    /// The five `AcceptsModifiers` overrides, against the stock bare-key
    /// bindings: TypeSelect=84 (T), CombatantSelect=80 (P), VeterancyNav=89 (Y),
    /// PlanningMode=90 (Z), plus StopObject=83 (S) as the unmodified default.
    #[test]
    fn modifier_accepting_commands_survive_their_modifier_and_defaults_do_not() {
        let bindings = HotkeyBindings::from_ini_bytes(Some(
            b"[Hotkey]\nTypeSelect=84\nCombatantSelect=80\nVeterancyNav=89\nPlanningMode=90\nStopObject=83\n",
        ));
        // Raw bit 8 only: Shift admits, Ctrl and Alt alone do not, and
        // Ctrl+Shift still admits because the native body masks bit 8 alone.
        for (letter, command) in [
            ("t", HotkeyCommand::TypeSelect),
            ("p", HotkeyCommand::CombatantSelect),
            ("y", HotkeyCommand::VeterancyNav),
        ] {
            let key = character(letter);
            assert_eq!(
                bindings.resolve(&key, KeyLocation::Standard, ModifiersState::empty()),
                Some(command),
            );
            assert_eq!(
                bindings.resolve(&key, KeyLocation::Standard, modifiers(true, false, false)),
                Some(command),
            );
            assert_eq!(
                bindings.resolve(&key, KeyLocation::Standard, modifiers(true, true, false)),
                Some(command),
            );
            assert_eq!(
                bindings.resolve(&key, KeyLocation::Standard, modifiers(false, true, false)),
                None,
            );
            assert_eq!(
                bindings.resolve(&key, KeyLocation::Standard, modifiers(false, false, true)),
                None,
            );
        }

        // `test ah,7`: any single modifier admits PlanningMode, and so does none.
        let planning = character("z");
        for held in [
            ModifiersState::empty(),
            modifiers(true, false, false),
            modifiers(false, true, false),
            modifiers(false, false, true),
            modifiers(true, true, false),
        ] {
            assert_eq!(
                bindings.resolve(&planning, KeyLocation::Standard, held),
                Some(HotkeyCommand::PlanningMode),
            );
        }

        // The shared default body rejects every modifier state but the bare one.
        let stop = character("s");
        assert_eq!(
            bindings.resolve(&stop, KeyLocation::Standard, ModifiersState::empty()),
            Some(HotkeyCommand::StopObject),
        );
        for held in [
            modifiers(true, false, false),
            modifiers(false, true, false),
            modifiers(false, false, true),
        ] {
            assert_eq!(bindings.resolve(&stop, KeyLocation::Standard, held), None);
        }
    }

    /// A modifier-accepting command must not steal a key whose modified word is
    /// separately bound: ScreenCapture=339 is Shift+S, and StopObject keeps the
    /// default gate, so Shift+S resolves through the second lookup.
    #[test]
    fn second_lookup_still_wins_for_default_gated_commands() {
        let bindings = HotkeyBindings::from_ini_bytes(Some(
            b"[Hotkey]\nStopObject=83\nScreenCapture=339\nTypeSelect=84\n",
        ));
        assert_eq!(
            bindings.resolve(
                &character("s"),
                KeyLocation::Standard,
                modifiers(true, false, false)
            ),
            Some(HotkeyCommand::ScreenCapture),
        );
        assert_eq!(
            bindings.resolve(
                &character("t"),
                KeyLocation::Standard,
                modifiers(true, false, false)
            ),
            Some(HotkeyCommand::TypeSelect),
        );
    }

    #[test]
    fn shifted_symbol_routes_through_modifier_free_logical_key() {
        let bindings = HotkeyBindings::from_ini_bytes(Some(
            b"[Hotkey]\nTeamAddSelect_1=305\nScreenCapture=339\n",
        ));
        let raw_shifted_digit = character("!");
        let modifier_free_digit = character("1");
        let selected_digit = binding_logical_key(
            &raw_shifted_digit,
            &modifier_free_digit,
            KeyLocation::Standard,
        );
        // OEM mapping may also resolve the shifted glyph on the current host,
        // but the event boundary must still select the modifier-free identity.
        assert_eq!(selected_digit, &modifier_free_digit);
        assert_eq!(
            bindings.resolve_event(
                selected_digit,
                KeyLocation::Standard,
                modifiers(true, false, false),
            ),
            HotkeyResolution::Command(HotkeyCommand::TeamAddSelect(1))
        );

        let raw_shifted_letter = character("S");
        let modifier_free_letter = character("s");
        let selected_letter = binding_logical_key(
            &raw_shifted_letter,
            &modifier_free_letter,
            KeyLocation::Standard,
        );
        assert_eq!(
            bindings.resolve_event(
                selected_letter,
                KeyLocation::Standard,
                modifiers(true, false, false),
            ),
            HotkeyResolution::Command(HotkeyCommand::ScreenCapture)
        );
        assert_eq!(
            logical_virtual_key(&raw_shifted_letter, KeyLocation::Standard),
            logical_virtual_key(&modifier_free_letter, KeyLocation::Standard)
        );
    }

    #[test]
    fn boundary_preserves_numpad_identity_but_strips_standard_shift() {
        let raw_numpad_2 = character("2");
        let modifier_free_down = Key::Named(NamedKey::ArrowDown);
        assert_eq!(
            binding_logical_key(&raw_numpad_2, &modifier_free_down, KeyLocation::Numpad),
            &raw_numpad_2
        );

        let raw_shifted = character("!");
        let modifier_free_1 = character("1");
        assert_eq!(
            binding_logical_key(&raw_shifted, &modifier_free_1, KeyLocation::Standard),
            &modifier_free_1
        );
    }

    #[test]
    fn numlock_on_digits_resolve_sidebar_bindings() {
        let bindings =
            HotkeyBindings::from_ini_bytes(Some(b"[Hotkey]\nSidebarDown=98\nSidebarUp=104\n"));
        for (raw, without_modifiers, expected) in [
            (
                character("2"),
                Key::Named(NamedKey::ArrowDown),
                HotkeyCommand::SidebarDown,
            ),
            (
                character("8"),
                Key::Named(NamedKey::ArrowUp),
                HotkeyCommand::SidebarUp,
            ),
        ] {
            let key = binding_logical_key(&raw, &without_modifiers, KeyLocation::Numpad);
            assert_eq!(
                bindings.resolve_event(key, KeyLocation::Numpad, ModifiersState::empty()),
                HotkeyResolution::Command(expected)
            );
        }
    }

    #[test]
    fn numlock_off_named_arrows_remain_arrow_fallbacks() {
        let bindings = HotkeyBindings::from_ini_bytes(None);
        for (key, expected) in [
            (Key::Named(NamedKey::ArrowDown), HotkeyFallback::ArrowDown),
            (Key::Named(NamedKey::ArrowUp), HotkeyFallback::ArrowUp),
        ] {
            let without_modifiers = character("5");
            let selected = binding_logical_key(&key, &without_modifiers, KeyLocation::Numpad);
            assert_eq!(
                bindings.resolve_event(selected, KeyLocation::Numpad, ModifiersState::empty()),
                HotkeyResolution::Fallback(expected)
            );
        }
    }

    #[test]
    fn held_scroll_keys_canonicalize_keypad_fallbacks_and_preserve_binding_precedence() {
        use winit::keyboard::KeyCode;

        assert_eq!(
            fallback_scroll_key(HotkeyResolution::Fallback(HotkeyFallback::ArrowDown)),
            Some(KeyCode::ArrowDown)
        );
        assert_eq!(
            fallback_scroll_key(HotkeyResolution::Command(HotkeyCommand::SidebarDown)),
            None
        );
        assert_eq!(
            physical_scroll_key(KeyCode::Numpad2),
            Some(KeyCode::ArrowDown)
        );
        assert_eq!(
            physical_scroll_key(KeyCode::ArrowLeft),
            Some(KeyCode::ArrowLeft)
        );
    }

    #[test]
    fn keypad_clear_resolves_center_view_and_standard_arrow_stays_fallback() {
        let bindings = HotkeyBindings::from_ini_bytes(Some(b"[Hotkey]\nCenterView=12\n"));
        assert_eq!(
            bindings.resolve_event(
                &Key::Named(NamedKey::Clear),
                KeyLocation::Numpad,
                ModifiersState::empty(),
            ),
            HotkeyResolution::Command(HotkeyCommand::CenterView)
        );
        assert_eq!(
            bindings.resolve_event(
                &Key::Named(NamedKey::ArrowLeft),
                KeyLocation::Standard,
                ModifiersState::empty(),
            ),
            HotkeyResolution::Fallback(HotkeyFallback::ArrowLeft)
        );
    }

    #[test]
    fn archive_shaped_stock_bindings_resolve_existing_semantic_owners() {
        let bindings = HotkeyBindings::from_ini_bytes(Some(
            b"[Hotkey]\nDeployObject=68\nGuardObject=71\nStructureTab=81\nUnitTab=82\nStopObject=83\nTypeSelect=84\nDefenseTab=87\nDelete=110\nScreenCapture=339\nView1=112\nSetView1=624\n",
        ));
        assert_eq!(
            bindings.resolve(
                &character("d"),
                KeyLocation::Standard,
                ModifiersState::empty()
            ),
            Some(HotkeyCommand::DeployObject)
        );
        assert_eq!(
            bindings.resolve(
                &character("s"),
                KeyLocation::Standard,
                modifiers(true, false, false)
            ),
            Some(HotkeyCommand::ScreenCapture)
        );
        assert_eq!(
            bindings.resolve(
                &Key::Named(NamedKey::F1),
                KeyLocation::Standard,
                ModifiersState::empty()
            ),
            Some(HotkeyCommand::View(0))
        );
        assert_eq!(
            bindings.resolve(
                &Key::Named(NamedKey::F1),
                KeyLocation::Standard,
                modifiers(false, true, false)
            ),
            Some(HotkeyCommand::SetView(0))
        );
        assert_eq!(
            bindings.resolve(
                &character("."),
                KeyLocation::Numpad,
                ModifiersState::empty()
            ),
            Some(HotkeyCommand::Delete)
        );
    }

    #[test]
    fn unclaimed_tab_and_arrows_resolve_to_hardcoded_fallbacks() {
        let bindings = HotkeyBindings::from_ini_bytes(None);
        assert_eq!(
            bindings.resolve_event(
                &Key::Named(NamedKey::Tab),
                KeyLocation::Standard,
                ModifiersState::empty(),
            ),
            HotkeyResolution::Fallback(HotkeyFallback::DiplomacyDialog)
        );
        assert_eq!(
            bindings.resolve_event(
                &Key::Named(NamedKey::ArrowLeft),
                KeyLocation::Standard,
                modifiers(true, false, false),
            ),
            HotkeyResolution::Fallback(HotkeyFallback::ArrowLeft)
        );
    }

    #[test]
    fn ini_binding_claims_tab_or_arrow_before_fallback() {
        let bindings =
            HotkeyBindings::from_ini_bytes(Some(b"[Hotkey]\nStopObject=9\nDeployObject=37\n"));
        assert_eq!(
            bindings.resolve_event(
                &Key::Named(NamedKey::Tab),
                KeyLocation::Standard,
                ModifiersState::empty(),
            ),
            HotkeyResolution::Command(HotkeyCommand::StopObject)
        );
        assert_eq!(
            bindings.resolve_event(
                &Key::Named(NamedKey::ArrowLeft),
                KeyLocation::Standard,
                ModifiersState::empty(),
            ),
            HotkeyResolution::Command(HotkeyCommand::DeployObject)
        );
    }
}

#[cfg(test)]
mod modifier_owner_tests {
    use super::*;
    #[test]
    fn child_capture_modifier_and_parent_release_do_not_leak_into_resumed_commands() {
        let mut live = ModifiersState::empty();
        let mut gameplay = ModifiersState::empty();
        record_modifier_event(
            &mut live,
            &mut gameplay,
            ModifiersState::SHIFT | ModifiersState::CONTROL,
            true,
        );
        assert_eq!(modifier_bits(live), 0x300);
        assert!(gameplay.is_empty());
        // Back returns to pausedBBB while keys remain held; key-up occurs there.
        record_modifier_event(&mut live, &mut gameplay, ModifiersState::empty(), true);
        assert!(live.is_empty());
        assert!(gameplay.is_empty());
        record_modifier_event(&mut live, &mut gameplay, ModifiersState::ALT, false);
        assert_eq!(live, gameplay);
        assert_eq!(modifier_bits(gameplay), 0x400);
    }
}
