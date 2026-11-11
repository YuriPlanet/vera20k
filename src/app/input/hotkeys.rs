//! Retail keyboard binding loading and logical-key dispatch.
//!
//! This app-layer module translates winit's logical keys into the encoded
//! Win32 key words imported from `KEYBOARDMD.INI` and saved in `VERA.INI`. It deliberately stops at
//! semantic command names; gameplay effects remain in `input::dispatch`.

use std::collections::BTreeMap;

use winit::keyboard::{Key, KeyCode, KeyLocation, ModifiersState, NamedKey};

use crate::rules::ini_parser::IniFile;

pub(crate) mod catalog;

const SHIFT_BIT: u16 = 0x100;
const CTRL_BIT: u16 = 0x200;
const ALT_BIT: u16 = 0x400;
const VK_ESCAPE: u16 = 0x1b;
const VK_SPACE: u16 = 0x20;
const VK_DELETE: u16 = 0x2e;

/// VERA's own commands that come with a key: Performance Stats Ctrl+Alt+F, Lock
/// Cursor to Window Ctrl+Alt+L, Full Screen Alt+Enter, String Keys Ctrl+Alt+K,
/// Edit Layout Ctrl+Alt+E, Formation Line F, Formation Lock O, Ferry J and
/// Rotate Primary I.
/// Retail `KEYBOARDMD.INI` binds none of these keys but F, which is its
/// Follow; [`HotkeyBindings::move_follow_off_formation_key`] moves that.
const VERA_DEFAULT_KEYS: [(HotkeyCommand, u16); 9] = [
    (HotkeyCommand::PerformanceStats, CTRL_BIT | ALT_BIT | 0x46),
    (HotkeyCommand::LockCursor, CTRL_BIT | ALT_BIT | 0x4c),
    (HotkeyCommand::FullScreen, ALT_BIT | 0x0d),
    (HotkeyCommand::StringKeys, CTRL_BIT | ALT_BIT | 0x4b),
    (HotkeyCommand::LayoutEdit, CTRL_BIT | ALT_BIT | 0x45),
    (HotkeyCommand::FormationLine, FORMATION_LINE_KEY),
    (HotkeyCommand::FormationLock, 0x4f),
    (HotkeyCommand::Ferry, 0x4a),
    (HotkeyCommand::RotatePrimary, 0x49),
];
/// F: Formation Line's default key and retail Follow's.
const FORMATION_LINE_KEY: u16 = 0x46;
/// V: where Follow goes when Formation Line takes F. Retail binds nothing to it.
const FOLLOW_MOVED_KEY: u16 = 0x56;
/// VeraX AutoLoad's Ctrl+D.
#[cfg(feature = "verax")]
const VERAX_DEFAULT_KEYS: [(HotkeyCommand, u16); 1] = [(HotkeyCommand::AutoLoad, CTRL_BIT | 0x44)];

fn vera_default_keys() -> impl Iterator<Item = (HotkeyCommand, u16)> {
    let keys = VERA_DEFAULT_KEYS.into_iter();
    #[cfg(feature = "verax")]
    let keys = keys.chain(VERAX_DEFAULT_KEYS);
    keys
}

fn has_vera_default(command: HotkeyCommand) -> bool {
    vera_default_keys().any(|(owner, _)| owner == command)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum HotkeyCommand {
    CameraUp,
    CameraDown,
    CameraLeft,
    CameraRight,
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
    /// VERA's built-in keys in the `vera20k` keyboard category, each with a
    /// default in [`VERA_DEFAULT_KEYS`]. These four work on every screen
    /// ([`HotkeyBindings::window_command`]).
    PerformanceStats,
    LockCursor,
    FullScreen,
    /// Swap the interface between each label's text and its string-table key
    /// while the Developer page's String Keys box is ticked.
    StringKeys,
    /// In a match: turn the layout edit mode on or off, in which the mouse
    /// drags the interface's parts (`input::layout_edit`). It comes with a
    /// default key like the four above but works in a match only.
    LayoutEdit,
    /// Held while a left drag draws a formation line, or with Shift a
    /// block (`input::formation_line`). In the `vera20k` keyboard category
    /// with the F default.
    FormationLine,
    /// Lock the selected units' places for later moves, or unlock them
    /// (`input::formation_lock`). In the `vera20k` keyboard category with
    /// the O default.
    FormationLock,
    /// In a match: arm the ferry order's drop click for the selected
    /// transports (`input::ferry`). It comes with a default key like the
    /// five above.
    Ferry,
    /// In a match: the selected factories of one kind take turns being the
    /// primary (`input::primary_rotation`). It comes with a default key.
    RotatePrimary,
    /// VeraX: load the selection into nearby transports. Registered in the
    /// `vera20k` keyboard category with the Ctrl+D default.
    AutoLoad,
    /// Phobos: `FireTacticalSW1..10`, positional on the first left-sidebar column.
    #[cfg(feature = "phobos")]
    FireTacticalSw(usize),
    /// Phobos: collapse/expand the left superweapon sidebar.
    #[cfg(feature = "phobos")]
    ToggleSwSidebar,
}

/// VERA camera bindings supplement the retail held-arrow fallback at
/// ProcessCommand55DF80. Track the physical source so releasing one alias
/// cannot cancel another held key, and rebinding cannot strand a direction.
#[derive(Debug, Clone, Default)]
pub(crate) struct CameraScrollKeys(std::collections::HashMap<KeyCode, KeyCode>);

impl CameraScrollKeys {
    pub(crate) fn press(&mut self, physical: KeyCode, direction: KeyCode) {
        self.0.insert(physical, direction);
    }

    pub(crate) fn release(&mut self, physical: KeyCode) -> bool {
        self.0.remove(&physical).is_some()
    }

    pub(crate) fn held(
        &self,
        keys: &std::collections::HashSet<KeyCode>,
        direction: KeyCode,
    ) -> bool {
        (keys.contains(&direction) && !self.0.contains_key(&direction))
            || self
                .0
                .iter()
                .any(|(physical, arrow)| *arrow == direction && keys.contains(physical))
    }
}

pub(crate) fn camera_command_key(resolution: HotkeyResolution) -> Option<KeyCode> {
    match resolution {
        HotkeyResolution::Command(HotkeyCommand::CameraUp) => Some(KeyCode::ArrowUp),
        HotkeyResolution::Command(HotkeyCommand::CameraDown) => Some(KeyCode::ArrowDown),
        HotkeyResolution::Command(HotkeyCommand::CameraLeft) => Some(KeyCode::ArrowLeft),
        HotkeyResolution::Command(HotkeyCommand::CameraRight) => Some(KeyCode::ArrowRight),
        _ => None,
    }
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
            // VERA: Shift with the held key makes the drag a block.
            Self::FormationLine => modifier_bits(modifiers) & !SHIFT_BIT == 0,
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
            // VERA: Formation Line takes Shift alone, as above.
            Self::FormationLine => encoded & (SHIFT_BIT | CTRL_BIT | ALT_BIT) == SHIFT_BIT,
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
    /// VERA commands explicitly unbound (`Name=0`): reloads and startups keep
    /// them unbound instead of restoring their default key.
    vera_cleared: Vec<HotkeyCommand>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BindingAssignmentError {
    CannotMap,
    CannotRemap,
}

impl HotkeyBindings {
    pub(crate) fn from_ini_bytes(bytes: Option<&[u8]>) -> Self {
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
                let value = section.read_int(name, 0);
                if value == 0 && has_vera_default(command) {
                    if !bindings.vera_cleared.contains(&command) {
                        bindings.vera_cleared.push(command);
                    }
                    continue;
                }
                let Some(encoded) = Some(value)
                    .filter(|value| *value != 0)
                    .and_then(|value| u16::try_from(value).ok())
                else {
                    continue;
                };
                bindings.by_key.insert(encoded, command);
            }
        }
        bindings.move_follow_off_formation_key();
        // A VERA command keeps its default key whenever the profile neither
        // remaps it, explicitly clears it, nor gives the key to another
        // command, and only where Assign could put it. Installing it here,
        // rather than only at startup, also makes Cancel and Reset All show
        // and restore the default.
        for (command, key) in vera_default_keys() {
            if !bindings.vera_cleared.contains(&command)
                && bindings.first_key(command).is_none()
                && !bindings.by_key.contains_key(&key)
                && bindings.assignment_error(command, key).is_none()
            {
                bindings.by_key.insert(key, command);
            }
        }

        bindings
    }

    /// Formation Line's default key is F, which retail `KEYBOARDMD.INI` and
    /// every profile saved from it give to Follow. Where the profile says
    /// nothing about Formation Line and Follow still sits on F, Follow moves
    /// to V (kept on any other key it has, dropped if V is taken) so the
    /// default below can take F. A player who puts Follow back on F unbinds
    /// Formation Line ([`Self::assign`]), which is saved as `FormationLine=0`
    /// and ends this.
    fn move_follow_off_formation_key(&mut self) {
        if self.vera_cleared.contains(&HotkeyCommand::FormationLine)
            || self.first_key(HotkeyCommand::FormationLine).is_some()
            || self.command_at(FORMATION_LINE_KEY) != Some(HotkeyCommand::Follow)
        {
            return;
        }
        self.by_key.remove(&FORMATION_LINE_KEY);
        if self.first_key(HotkeyCommand::Follow).is_none()
            && !self.by_key.contains_key(&FOLLOW_MOVED_KEY)
        {
            self.by_key.insert(FOLLOW_MOVED_KEY, HotkeyCommand::Follow);
        }
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

    /// Assign's two modifier rejections (5FBB38..5FBDF1): the command itself
    /// takes the modifiers, or the bare key's owner already takes them.
    fn assignment_error(
        &self,
        command: HotkeyCommand,
        encoded: u16,
    ) -> Option<BindingAssignmentError> {
        if encoded & (SHIFT_BIT | CTRL_BIT | ALT_BIT) == 0 {
            return None;
        }
        if command.accepts_native_modifiers(encoded) {
            return Some(BindingAssignmentError::CannotMap);
        }
        self.command_at(encoded & 0xff)
            .is_some_and(|owner| owner.accepts_native_modifiers(encoded))
            .then_some(BindingAssignmentError::CannotRemap)
    }

    /// Original 5FBB38..5FBDF1; executable comparisons in keyboard_bindings.json.
    /// Zero unassigns the first existing shortcut. Rejections leave all bindings intact.
    pub(crate) fn assign(
        &mut self,
        command: HotkeyCommand,
        encoded: u16,
    ) -> Result<(), BindingAssignmentError> {
        if let Some(error) = self.assignment_error(command, encoded) {
            return Err(error);
        }
        if has_vera_default(command) {
            self.vera_cleared.retain(|&owner| owner != command);
            if encoded == 0 {
                self.vera_cleared.push(command);
            }
        }
        if let Some(key) = self.first_key(command) {
            self.by_key.remove(&key);
        }
        if encoded != 0
            && let Some(displaced) = self.by_key.insert(encoded, command)
            && displaced != command
            && has_vera_default(displaced)
            && self.first_key(displaced).is_none()
            && !self.vera_cleared.contains(&displaced)
        {
            // Taking a VERA command's last key unbinds it for good, as an
            // explicit unbind does; otherwise its default would come back
            // the moment the key is free again.
            self.vera_cleared.push(displaced);
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
        // An explicit zero keeps a deliberate unbind through reloads and the
        // next startup; an absent row would restore the default.
        for command in &self.vera_cleared {
            if let Some(metadata) = catalog::registered_commands()
                .iter()
                .find(|row| row.command == *command)
            {
                entries.push((metadata.ini_name, 0));
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
        // ProcessCommand55E079 opens Options only after both binding lookups
        // fail, including when A3 reload533D20 has removed the startup binding.
        if virtual_key == VK_ESCAPE {
            return HotkeyResolution::Command(HotkeyCommand::Options);
        }
        fallback_for_virtual_key(virtual_key)
            .map(HotkeyResolution::Fallback)
            .unwrap_or(HotkeyResolution::Unhandled)
    }

    /// VERA's every-screen commands (Performance Stats, Lock Cursor to Window,
    /// Full Screen, String Keys), which the window owner runs before any
    /// screen's own keys. A binding on a key that types text, held without Ctrl
    /// or Alt, is left to the battle's hotkey dispatch, which serves the chat
    /// edit first, so text boxes keep their letters.
    pub(crate) fn window_command(
        &self,
        logical_key: &Key,
        location: KeyLocation,
        modifiers: ModifiersState,
    ) -> Option<HotkeyCommand> {
        let HotkeyResolution::Command(
            command @ (HotkeyCommand::PerformanceStats
            | HotkeyCommand::LockCursor
            | HotkeyCommand::FullScreen
            | HotkeyCommand::StringKeys),
        ) = self.resolve_event(logical_key, location, modifiers)
        else {
            return None;
        };
        let chord = modifier_bits(modifiers) & (CTRL_BIT | ALT_BIT) != 0;
        let typing = logical_virtual_key(logical_key, location).is_some_and(types_text);
        (chord || !typing).then_some(command)
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

/// Keys that type a character into a text box: Space, the digits and
/// letters, the keypad's digits and operators and the OEM punctuation keys.
fn types_text(virtual_key: u16) -> bool {
    matches!(
        virtual_key,
        0x20 | 0x30..=0x39 | 0x41..=0x5a | 0x60..=0x6f | 0xba..=0xc0 | 0xdb..=0xdf | 0xe2
    )
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

/// The physical modifier keys currently held, one flag per side.
///
/// Modifier state is derived from the key press/release edges and never from
/// `WindowEvent::ModifiersChanged`. winit re-derives that event from the
/// modifier flags of every mouse event, and on macOS the mouse events
/// delivered while the built-in keyboard holds Control carry no Control flag:
/// the trace of 2026-10-08 shows `ControlLeft Pressed`, `CONTROL`, then an
/// empty modifier set before the first click, which turned every Ctrl+click
/// into a plain Move. An external keyboard's Ctrl did not drop. Tracking the
/// edges makes the game's view of Ctrl/Shift/Alt independent of what the
/// platform attaches to mouse events; a release that never arrives (Alt+Tab)
/// is already covered by the focus-edge reset.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct HeldModifierKeys {
    bits: u8,
}

impl HeldModifierKeys {
    const LSHIFT: u8 = 1 << 0;
    const RSHIFT: u8 = 1 << 1;
    const LCONTROL: u8 = 1 << 2;
    const RCONTROL: u8 = 1 << 3;
    const LALT: u8 = 1 << 4;
    const RALT: u8 = 1 << 5;

    fn flag(code: KeyCode) -> Option<u8> {
        Some(match code {
            KeyCode::ShiftLeft => Self::LSHIFT,
            KeyCode::ShiftRight => Self::RSHIFT,
            KeyCode::ControlLeft => Self::LCONTROL,
            KeyCode::ControlRight => Self::RCONTROL,
            KeyCode::AltLeft => Self::LALT,
            KeyCode::AltRight => Self::RALT,
            _ => return None,
        })
    }

    /// Apply one key edge. Returns false when `code` is not a modifier key.
    fn record(&mut self, code: KeyCode, pressed: bool) -> bool {
        let Some(flag) = Self::flag(code) else {
            return false;
        };
        if pressed {
            self.bits |= flag;
        } else {
            self.bits &= !flag;
        }
        true
    }

    pub(crate) fn clear(&mut self) {
        self.bits = 0;
    }

    /// The modifier set the held keys produce: a side-agnostic bit per modifier.
    pub(crate) fn state(self) -> ModifiersState {
        let mut state = ModifiersState::empty();
        state.set(
            ModifiersState::SHIFT,
            self.bits & (Self::LSHIFT | Self::RSHIFT) != 0,
        );
        state.set(
            ModifiersState::CONTROL,
            self.bits & (Self::LCONTROL | Self::RCONTROL) != 0,
        );
        state.set(
            ModifiersState::ALT,
            self.bits & (Self::LALT | Self::RALT) != 0,
        );
        state
    }
}

/// Record one keyboard edge into the held modifier keys and refresh the two
/// modifier views. Returns false, changing nothing, when the key is not a
/// modifier.
///
/// Shell controls observe the live message pump while gameplay retains its
/// paused admission snapshot. A3 capture must not latch a modifier into gameplay
/// when its key-up occurs later in the paused parent.
pub(crate) fn record_modifier_key(
    held: &mut HeldModifierKeys,
    live: &mut ModifiersState,
    gameplay: &mut ModifiersState,
    code: KeyCode,
    pressed: bool,
    paused: bool,
) -> bool {
    if !held.record(code, pressed) {
        return false;
    }
    *live = held.state();
    if !paused {
        *gameplay = *live;
    }
    true
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
const PORTABLE_OEM_KEYS: [(char, u16); 11] = [
    (';', 0xba),
    ('=', 0xbb),
    (',', 0xbc),
    ('-', 0xbd),
    ('.', 0xbe),
    ('/', 0xbf),
    ('`', 0xc0),
    ('[', 0xdb),
    ('\\', 0xdc),
    (']', 0xdd),
    ('\'', 0xde),
];

/// Non-Windows keyboards have no Win32 layout lookup. Use the conventional
/// US OEM identities for portable punctuation bindings; Windows retains its
/// layout-specific VkKeyScanW above. Imported non-US Windows OEM bindings may
/// need rebinding on these hosts. See Win32's layout-dependent OEM definitions:
/// https://learn.microsoft.com/en-us/windows/win32/inputdev/virtual-key-codes
#[cfg(not(windows))]
fn printable_virtual_key(character: char) -> Option<u16> {
    PORTABLE_OEM_KEYS
        .iter()
        .find_map(|&(text, vk)| (text == character).then_some(vk))
}

#[cfg(not(windows))]
pub(crate) fn portable_oem_key_name(vk: u16) -> Option<char> {
    PORTABLE_OEM_KEYS
        .iter()
        .find_map(|&(text, key)| (key == vk).then_some(text))
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
        NamedKey::Pause => 0x13,
        NamedKey::CapsLock => 0x14,
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
        NamedKey::PrintScreen => 0x2c,
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
        NamedKey::F13 => 0x7c,
        NamedKey::F14 => 0x7d,
        NamedKey::F15 => 0x7e,
        NamedKey::F16 => 0x7f,
        NamedKey::F17 => 0x80,
        NamedKey::F18 => 0x81,
        NamedKey::F19 => 0x82,
        NamedKey::F20 => 0x83,
        NamedKey::F21 => 0x84,
        NamedKey::F22 => 0x85,
        NamedKey::F23 => 0x86,
        NamedKey::F24 => 0x87,
        NamedKey::NumLock => 0x90,
        _ => return None,
    })
}

/// A registered command by its keyboard-file name, without regard to
/// capitals: what a layout file's `Command=` names. The keyboard file itself
/// compares exact bytes ([`command_from_name`]).
pub(crate) fn command_named(name: &str) -> Option<HotkeyCommand> {
    catalog::registered_commands()
        .iter()
        .find(|row| row.ini_name.eq_ignore_ascii_case(name))
        .map(|row| row.command)
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

    #[test]
    fn camera_bindings_use_normal_assignment_conflicts_and_persistence() {
        let mut bindings = HotkeyBindings::from_ini_bytes(Some(
            b"[Hotkey]\nStopObject=83\nDeployObject=68\nGuardObject=65\nScatterObject=87\n",
        ));
        assert_eq!(bindings.first_key(HotkeyCommand::CameraUp), None);
        assert_eq!(bindings.command_at(83), Some(HotkeyCommand::StopObject));
        bindings.assign(HotkeyCommand::StopObject, 79).unwrap();
        let moves = [
            (HotkeyCommand::CameraUp, 87, "w", KeyCode::ArrowUp),
            (HotkeyCommand::CameraLeft, 65, "a", KeyCode::ArrowLeft),
            (HotkeyCommand::CameraDown, 83, "s", KeyCode::ArrowDown),
            (HotkeyCommand::CameraRight, 68, "d", KeyCode::ArrowRight),
        ];
        for (command, key, _, _) in moves {
            bindings.assign(command, key).unwrap();
            assert!(catalog::registered_commands().iter().any(|row| {
                row.command == command && row.category_key == catalog::VERA_CATEGORY_KEY
            }));
        }
        let saved = bindings.to_ini_string();
        let mut restored = HotkeyBindings::reload_from_ini_bytes(Some(saved.as_bytes()));
        for (command, key, logical, direction) in moves {
            assert_eq!(restored.first_key(command), Some(key));
            let resolution = restored.resolve_event(
                &character(logical),
                KeyLocation::Standard,
                ModifiersState::empty(),
            );
            assert_eq!(resolution, HotkeyResolution::Command(command));
            assert_eq!(camera_command_key(resolution), Some(direction));
        }
        assert_eq!(restored.command_at(79), Some(HotkeyCommand::StopObject));
        assert_eq!(restored.first_key(HotkeyCommand::DeployObject), None);
        assert_eq!(restored.first_key(HotkeyCommand::GuardObject), None);
        assert_eq!(restored.first_key(HotkeyCommand::ScatterObject), None);
        restored.assign(HotkeyCommand::CameraDown, 0).unwrap();
        restored.assign(HotkeyCommand::StopObject, 83).unwrap();
        let saved = restored.to_ini_string();
        let restored = HotkeyBindings::reload_from_ini_bytes(Some(saved.as_bytes()));
        assert_eq!(restored.first_key(HotkeyCommand::CameraDown), None);
        assert_eq!(restored.command_at(83), Some(HotkeyCommand::StopObject));
        assert_eq!(
            restored.resolve_event(
                &Key::Named(NamedKey::ArrowUp),
                KeyLocation::Standard,
                ModifiersState::empty()
            ),
            HotkeyResolution::Fallback(HotkeyFallback::ArrowUp)
        );
    }

    #[test]
    fn camera_holds_release_by_physical_source_and_respect_arrow_rebindings() {
        use std::collections::HashSet;
        let mut scroll = CameraScrollKeys::default();
        let mut keys = HashSet::from([KeyCode::KeyW, KeyCode::ArrowUp]);
        scroll.press(KeyCode::KeyW, KeyCode::ArrowUp);
        assert!(scroll.held(&keys, KeyCode::ArrowUp));
        scroll.release(KeyCode::KeyW);
        keys.remove(&KeyCode::KeyW);
        assert!(scroll.held(&keys, KeyCode::ArrowUp));
        scroll.press(KeyCode::KeyW, KeyCode::ArrowUp);
        keys.insert(KeyCode::KeyW);
        keys.remove(&KeyCode::ArrowUp);
        assert!(scroll.held(&keys, KeyCode::ArrowUp));
        // A direction is retained from the admitted press, not re-resolved
        // from bindings or modifiers that can change before the release.
        assert!(scroll.release(KeyCode::KeyW));
        keys.remove(&KeyCode::KeyW);
        assert!(!scroll.held(&keys, KeyCode::ArrowUp));
        keys.insert(KeyCode::ArrowDown);
        scroll.press(KeyCode::ArrowDown, KeyCode::ArrowLeft);
        assert!(scroll.held(&keys, KeyCode::ArrowLeft));
        assert!(!scroll.held(&keys, KeyCode::ArrowDown));
        keys.clear(); // A focus loss clears all active sources.
        assert!(!scroll.held(&keys, KeyCode::ArrowLeft));
    }

    fn native_keyboard_fixture() -> &'static serde_json::Value {
        crate::test_fixture::json("tools/storage_oracle/keyboard_bindings.json")
    }

    #[test]
    fn registered_catalog_matches_original_objects_and_metadata_getters() {
        let fixture = native_keyboard_fixture();
        let native = fixture["catalog"].as_array().unwrap();
        let catalog = catalog::retail_commands();
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
        // The 87 retail rows, plus VERA's five default keys above 0x256 and
        // the two Formation keys, Ferry's J and Rotate Primary's I below 0x200.
        assert_eq!(loaded.by_key.len(), 96);
        for (index, metadata) in catalog::retail_commands().iter().enumerate() {
            assert_eq!(
                loaded.command_at(0x200 + index as u16),
                Some(metadata.command)
            );
        }
        let saved = loaded.to_ini_string();
        ini.push_str("FullScreen=1037\nLayoutEdit=1605\nPerformanceStats=1606\nStringKeys=1611\nLockCursor=1612\n");
        let ini = ini.replacen(
            "[Hotkey]\n",
            "[Hotkey]\nFormationLine=70\nRotatePrimary=73\nFerry=74\nFormationLock=79\n",
            1,
        );
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
        let rejected = HotkeyBindings::reload_from_ini_bytes(Some(ini.as_bytes()));
        // Only VERA's own default keys survive; no alias row is parsed.
        for index in 0..aliases.len() {
            assert_eq!(rejected.command_at(0x200 + index as u16), None);
        }
        assert!(
            rejected
                .by_key
                .values()
                .all(|&command| has_vera_default(command))
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
        #[cfg(feature = "verax")]
        let expected = "[Hotkey]\nOptions=27\nCenterOnRadarEvent=32\nDelete=110\nFormationLine=70\nRotatePrimary=73\nFerry=74\nFormationLock=79\nStopObject=83\nAutoLoad=580\nFullScreen=1037\nLayoutEdit=1605\nPerformanceStats=1606\nStringKeys=1611\nLockCursor=1612\n";
        #[cfg(not(feature = "verax"))]
        let expected = "[Hotkey]\nOptions=27\nCenterOnRadarEvent=32\nDelete=110\nFormationLine=70\nRotatePrimary=73\nFerry=74\nFormationLock=79\nStopObject=83\nFullScreen=1037\nLayoutEdit=1605\nPerformanceStats=1606\nStringKeys=1611\nLockCursor=1612\n";
        assert_eq!(serialized, expected);
        let mut reloaded = HotkeyBindings::reload_from_ini_bytes(Some(serialized.as_bytes()));
        assert_eq!(reloaded.command_at(46), None);
        assert_eq!(reloaded.first_key(HotkeyCommand::Delete), Some(110));
        #[cfg(feature = "verax")]
        assert_eq!(reloaded.first_key(HotkeyCommand::AutoLoad), Some(580));
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

    #[cfg(feature = "verax")]
    #[test]
    fn verax_autoload_registers_its_category_and_ctrl_d_default() {
        let catalog = catalog::registered_commands();
        let row = catalog
            .iter()
            .find(|row| row.ini_name == "AutoLoad")
            .expect("AutoLoad is registered");
        assert_eq!(row.command, HotkeyCommand::AutoLoad);
        assert_eq!(row.category_key, catalog::VERA_CATEGORY_KEY);
        assert_eq!(row.name_key, "VERA:AutoLoad");
        assert_eq!(command_from_name("AutoLoad"), Some(HotkeyCommand::AutoLoad));

        let bindings = HotkeyBindings::from_ini_bytes(None);
        assert_eq!(
            bindings.first_key(HotkeyCommand::AutoLoad),
            Some(CTRL_BIT | 0x44)
        );
        assert_eq!(
            bindings.resolve(
                &character("d"),
                KeyLocation::Standard,
                ModifiersState::CONTROL
            ),
            Some(HotkeyCommand::AutoLoad)
        );
        // Bare D keeps whatever the retail table gives it.
        assert_eq!(
            bindings.resolve(
                &character("d"),
                KeyLocation::Standard,
                ModifiersState::empty()
            ),
            None
        );

        // A saved remap survives, and a saved Ctrl+D owner wins over the default.
        let remapped = HotkeyBindings::from_ini_bytes(Some(b"[Hotkey]\nAutoLoad=88\n"));
        assert_eq!(remapped.first_key(HotkeyCommand::AutoLoad), Some(88));
        let occupied = HotkeyBindings::from_ini_bytes(Some(b"[Hotkey]\nStopObject=580\n"));
        assert_eq!(
            occupied.command_at(CTRL_BIT | 0x44),
            Some(HotkeyCommand::StopObject)
        );
        assert_eq!(occupied.first_key(HotkeyCommand::AutoLoad), None);

        // A deliberate unbind writes AutoLoad=0 and keeps the default away
        // through the next reload; Cancel/Reset restores it when no explicit
        // zero is on disk.
        let mut cleared = HotkeyBindings::from_ini_bytes(None);
        cleared.assign(HotkeyCommand::AutoLoad, 0).unwrap();
        assert_eq!(cleared.first_key(HotkeyCommand::AutoLoad), None);
        let saved = cleared.to_ini_string();
        assert!(saved.ends_with("AutoLoad=0\n"), "{saved}");
        let reloaded = HotkeyBindings::reload_from_ini_bytes(Some(saved.as_bytes()));
        assert_eq!(reloaded.first_key(HotkeyCommand::AutoLoad), None);
        assert_eq!(
            HotkeyBindings::reload_from_ini_bytes(None).first_key(HotkeyCommand::AutoLoad),
            Some(CTRL_BIT | 0x44)
        );
    }

    /// Formation Line takes F from a profile that still has retail's Follow
    /// there, and Follow moves to V. Putting Follow back on F unbinds
    /// Formation Line, and that survives the profile round trip.
    #[test]
    fn formation_line_takes_f_and_follow_moves_until_the_player_moves_it_back() {
        let mut bindings = HotkeyBindings::from_ini_bytes(Some(b"[Hotkey]\nFollow=70\n"));
        assert_eq!(bindings.command_at(70), Some(HotkeyCommand::FormationLine));
        assert_eq!(bindings.first_key(HotkeyCommand::Follow), Some(86));
        // Shift with the key is still the key, for the block; Ctrl is not.
        for (held, command) in [
            (
                modifiers(true, false, false),
                Some(HotkeyCommand::FormationLine),
            ),
            (modifiers(false, true, false), None),
        ] {
            assert_eq!(
                bindings.resolve(&character("f"), KeyLocation::Standard, held),
                command
            );
        }

        bindings.assign(HotkeyCommand::Follow, 70).unwrap();
        let saved = bindings.to_ini_string();
        assert!(saved.contains("\nFormationLine=0\n"), "{saved}");
        let reloaded = HotkeyBindings::reload_from_ini_bytes(Some(saved.as_bytes()));
        assert_eq!(reloaded.command_at(70), Some(HotkeyCommand::Follow));
        assert_eq!(reloaded.first_key(HotkeyCommand::FormationLine), None);

        // V already used: Follow keeps no key and Formation Line still gets F.
        let taken = HotkeyBindings::from_ini_bytes(Some(b"[Hotkey]\nFollow=70\nStopObject=86\n"));
        assert_eq!(taken.command_at(70), Some(HotkeyCommand::FormationLine));
        assert_eq!(taken.command_at(86), Some(HotkeyCommand::StopObject));
    }

    /// VERA's built-in keys keep their defaults beside the retail
    /// KEYBOARDMD.INI rows, and an unbind survives the profile round trip.
    #[test]
    fn vera_shortcuts_keep_their_default_keys_beside_retail_bindings() {
        let Some((_, assets)) = crate::rules::retail_ini_fixture::retail_assets() else {
            return;
        };
        let archive = assets.get_archive_ref("KEYBOARDMD.INI");
        assert!(archive.is_some(), "retail KEYBOARDMD.INI");
        let mut bindings = HotkeyBindings::from_ini_bytes(archive);
        for (command, key) in [
            (HotkeyCommand::PerformanceStats, 0x646),
            (HotkeyCommand::LockCursor, 0x64c),
            (HotkeyCommand::FullScreen, 0x40d),
            (HotkeyCommand::StringKeys, 0x64b),
            (HotkeyCommand::LayoutEdit, 0x645),
            (HotkeyCommand::FormationLock, 0x4f),
            // Retail leaves J unbound.
            (HotkeyCommand::Ferry, 0x4a),
            // Retail leaves I unbound too.
            (HotkeyCommand::RotatePrimary, 0x49),
        ] {
            assert_eq!(bindings.first_key(command), Some(key), "{command:?}");
        }
        assert_eq!(
            bindings.window_command(
                &character("f"),
                KeyLocation::Standard,
                modifiers(false, true, true)
            ),
            Some(HotkeyCommand::PerformanceStats)
        );
        #[cfg(not(windows))]
        assert_eq!(crate::app::input::keyboard::key_name(0x40d), "Alt+Enter");

        bindings.assign(HotkeyCommand::PerformanceStats, 0).unwrap();
        let saved = bindings.to_ini_string();
        assert!(saved.contains("\nPerformanceStats=0\n"), "{saved}");
        let reloaded = HotkeyBindings::reload_from_ini_bytes(Some(saved.as_bytes()));
        assert_eq!(reloaded.first_key(HotkeyCommand::PerformanceStats), None);
        assert_eq!(reloaded.first_key(HotkeyCommand::LockCursor), Some(0x64c));
    }

    /// The every-screen keys run from the window owner only as a Ctrl or Alt
    /// chord or on a key that types nothing; a typing key stays a battle key.
    #[test]
    fn vera_window_keys_leave_typing_keys_to_the_battle() {
        let mut bindings = HotkeyBindings::from_ini_bytes(None);
        bindings.assign(HotkeyCommand::FullScreen, 0x7a).unwrap(); // F11
        bindings
            .assign(HotkeyCommand::PerformanceStats, 0x47)
            .unwrap(); // G
        bindings
            .assign(HotkeyCommand::LockCursor, ALT_BIT | 0x47)
            .unwrap();
        let none = ModifiersState::empty();
        let standard = KeyLocation::Standard;
        assert_eq!(
            bindings.window_command(&Key::Named(NamedKey::F11), standard, none),
            Some(HotkeyCommand::FullScreen)
        );
        assert_eq!(
            bindings.window_command(&character("g"), standard, none),
            None
        );
        assert_eq!(
            bindings.resolve(&character("g"), standard, none),
            Some(HotkeyCommand::PerformanceStats)
        );
        assert_eq!(
            bindings.window_command(&character("g"), standard, modifiers(false, false, true)),
            Some(HotkeyCommand::LockCursor)
        );
        assert_eq!(
            bindings.window_command(&character("k"), standard, modifiers(false, true, true)),
            Some(HotkeyCommand::StringKeys)
        );
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

    #[cfg(not(windows))]
    #[test]
    fn portable_oem_shortcuts_roundtrip_saved_bindings() {
        // Win32's US OEM identities. Windows keeps its real layout lookup;
        // non-Windows hosts must at least be able to bind ordinary punctuation.
        for (text, vk) in [
            (";", 0xba),
            ("=", 0xbb),
            (",", 0xbc),
            ("-", 0xbd),
            (".", 0xbe),
            ("/", 0xbf),
            ("`", 0xc0),
            ("[", 0xdb),
            ("\\", 0xdc),
            ("]", 0xdd),
            ("'", 0xde),
        ] {
            let key = character(text);
            assert_eq!(logical_virtual_key(&key, KeyLocation::Standard), Some(vk));
            let mut bindings = HotkeyBindings::default();
            bindings
                .assign(HotkeyCommand::StopObject, vk | CTRL_BIT)
                .unwrap();
            let saved = bindings.to_ini_string();
            let bindings = HotkeyBindings::reload_from_ini_bytes(Some(saved.as_bytes()));
            assert_eq!(
                bindings.resolve_event(&key, KeyLocation::Standard, ModifiersState::CONTROL),
                HotkeyResolution::Command(HotkeyCommand::StopObject),
                "{text}"
            );
        }
    }

    #[test]
    fn reloaded_escape_binding_precedes_the_native_options_fallback() {
        let custom = HotkeyBindings::reload_from_ini_bytes(Some(b"[Hotkey]\nStopObject=27\n"));
        assert_eq!(
            custom.resolve_event(
                &Key::Named(NamedKey::Escape),
                KeyLocation::Standard,
                ModifiersState::empty()
            ),
            HotkeyResolution::Command(HotkeyCommand::StopObject)
        );
        for bindings in [custom, HotkeyBindings::default()] {
            assert_eq!(
                bindings.resolve_event(
                    &Key::Named(NamedKey::Escape),
                    KeyLocation::Standard,
                    ModifiersState::SHIFT
                ),
                HotkeyResolution::Command(HotkeyCommand::Options)
            );
        }
    }
}

#[cfg(test)]
mod modifier_owner_tests {
    use super::*;

    #[derive(Default)]
    struct Views {
        held: HeldModifierKeys,
        live: ModifiersState,
        gameplay: ModifiersState,
    }

    impl Views {
        fn edge(&mut self, code: KeyCode, pressed: bool, paused: bool) -> bool {
            record_modifier_key(
                &mut self.held,
                &mut self.live,
                &mut self.gameplay,
                code,
                pressed,
                paused,
            )
        }
    }

    #[test]
    fn child_capture_modifier_and_parent_release_do_not_leak_into_resumed_commands() {
        let mut views = Views::default();
        assert!(views.edge(KeyCode::ShiftLeft, true, true));
        assert!(views.edge(KeyCode::ControlLeft, true, true));
        assert_eq!(modifier_bits(views.live), 0x300);
        assert!(views.gameplay.is_empty());
        // Back returns to pausedBBB while keys remain held; key-up occurs there.
        assert!(views.edge(KeyCode::ShiftLeft, false, true));
        assert!(views.edge(KeyCode::ControlLeft, false, true));
        assert!(views.live.is_empty());
        assert!(views.gameplay.is_empty());
        assert!(views.edge(KeyCode::AltLeft, true, false));
        assert_eq!(views.live, views.gameplay);
        assert_eq!(modifier_bits(views.gameplay), 0x400);
    }

    /// A non-modifier key leaves both views untouched, and the two sides of a
    /// modifier are held independently: Control stays down until both are up.
    #[test]
    fn modifier_sides_release_independently() {
        let mut views = Views::default();
        assert!(!views.edge(KeyCode::KeyA, true, false));
        assert!(views.gameplay.is_empty());
        assert!(views.edge(KeyCode::ControlLeft, true, false));
        assert!(views.edge(KeyCode::ControlRight, true, false));
        assert!(views.edge(KeyCode::ControlLeft, false, false));
        assert_eq!(views.gameplay, ModifiersState::CONTROL);
        assert!(views.edge(KeyCode::ControlRight, false, false));
        assert!(views.gameplay.is_empty());
    }
}
