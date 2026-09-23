//! Shared type definitions and constants used across app_* modules.
//!
//! These types were extracted from the render/input glue because multiple sibling
//! modules (input::cursor, input::dispatch, input::entity_pick,
//! presentation::ui_overlays, etc.)
//! depend on them. Centralizing them here avoids coupling unrelated modules
//! to the rendering orchestration file.
//!
//! ## Dependency rules
//! - Part of the app layer — no sim/render dependencies.

use std::time::{Duration, Instant};

use winit::keyboard::KeyCode;


/// Background clear color — black, matching the shroud/fog of war in RA2.
/// Areas outside the isometric terrain diamond are not visible in the original game.
pub(crate) const CLEAR_COLOR: wgpu::Color = wgpu::Color {
    r: 0.0,
    g: 0.0,
    b: 0.0,
    a: 1.0,
};

/// Fixed deterministic simulation rate — re-exported from util::fixed_math.
pub(crate) const SIM_TICK_HZ: u32 = crate::util::fixed_math::SIM_TICK_HZ;
/// Integer tick duration used by deterministic step execution.
pub(crate) const SIM_TICK_MS: u32 = 1000 / SIM_TICK_HZ;
/// Verified retail/YR skirmish fallback from rulesmd.ini
/// `[MultiplayerDialogSettings] GameSpeed=1`.
pub(crate) const DEFAULT_YR_SKIRMISH_GAME_SPEED: u32 = 1;
const GAME_SPEED_BUCKET_MS: u32 = 16;

/// Convert the stored speed byte to a UI/debug rate readout. Gameplay frame
/// admission uses the native 16 ms bucket comparison directly.
pub(crate) fn tps_for_game_speed(stored_speed: u32) -> u32 {
    if stored_speed == 0 {
        return 60;
    }
    let bucket_ms = stored_speed.saturating_mul(GAME_SPEED_BUCKET_MS).max(1);
    ((1000 + bucket_ms / 2) / bucket_ms).max(1)
}

pub(crate) fn default_yr_skirmish_tps() -> u32 {
    tps_for_game_speed(DEFAULT_YR_SKIRMISH_GAME_SPEED)
}
/// Next right-click order mode selected via hotkey.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum OrderMode {
    Move,
    AttackMove,
    Guard,
}

/// Local edge state for the retail TypeSelect command.
///
/// The command is unusual among the keyboard actions: the press arms a held
/// mode, mouse selection consults that mode while the key remains down, and
/// the release performs the tap action only when it arrives within 500 ms.
/// This is presentation/input state, never lockstep simulation state.
#[derive(Debug, Default)]
pub(crate) struct TypeSelectInputState {
    pressed_at: Option<Instant>,
    physical_key: Option<KeyCode>,
    selection_mode: SelectionNavigationMode,
    pub(crate) across_map: bool,
    pub(crate) last_outcome: Option<TypeSelectOutcome>,
}

/// Shared native B0FE54 mode; ordinary selection clears it (731D00).
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
enum SelectionNavigationMode {
    #[default]
    Ordinary,
    Type,
    Health,
}

impl TypeSelectInputState {
    pub(crate) fn held(&self) -> bool {
        self.pressed_at.is_some()
    }

    pub(crate) fn owns_key(&self, physical_key: KeyCode) -> bool {
        self.physical_key == Some(physical_key)
    }

    /// Arm on the first press edge. Auto-repeat and duplicate press events do
    /// not move the timestamp.
    pub(crate) fn press(&mut self, physical_key: KeyCode, now: Instant, repeat: bool) {
        if repeat || self.pressed_at.is_some() {
            return;
        }
        self.pressed_at = Some(now);
        self.physical_key = Some(physical_key);
    }

    /// Clear the matching held edge and report whether the release is a tap.
    pub(crate) fn release(&mut self, physical_key: KeyCode, now: Instant) -> bool {
        if self.physical_key != Some(physical_key) {
            return false;
        }
        self.physical_key = None;
        let Some(pressed_at) = self.pressed_at.take() else {
            return false;
        };
        now.saturating_duration_since(pressed_at) <= Duration::from_millis(500)
    }

    pub(crate) fn clear_held(&mut self) {
        self.pressed_at = None;
        self.physical_key = None;
    }

    /// TypeSelect's scope byte persists independently of SelectionMode. A held
    /// batch consumes the old scope first; its successful Select then resets
    /// SelectionMode, so a following short-release tap must restart on-screen.
    pub(crate) fn note_successful_selection_mutation(&mut self, clear_scope: bool) {
        self.selection_mode = SelectionNavigationMode::Ordinary;
        if clear_scope {
            self.across_map = false;
            self.last_outcome = None;
        }
    }

    pub(crate) fn prepare_tap_scope(&mut self) {
        if self.selection_mode != SelectionNavigationMode::Type {
            self.across_map = false;
        }
    }

    pub(crate) fn finish_tap(&mut self, outcome: TypeSelectOutcome, across_map: bool) {
        self.selection_mode = SelectionNavigationMode::Type;
        self.across_map = across_map;
        self.last_outcome = Some(outcome);
    }

    pub(crate) fn reset_scope(&mut self) {
        self.note_successful_selection_mutation(true);
    }

    pub(crate) fn health_navigation_continues(&self) -> bool {
        self.selection_mode == SelectionNavigationMode::Health
    }

    /// 7335BC..7335D7 restores mode3 after the Select calls reset the mode.
    pub(crate) fn finish_health_navigation(&mut self, has_candidates: bool) {
        self.reset_scope();
        if has_candidates {
            self.selection_mode = SelectionNavigationMode::Health;
        }
    }
}

/// Localized HUD outcomes emitted by the native TypeSelect action.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TypeSelectOutcome {
    Empty,
    Map,
    Screen,
}

impl TypeSelectOutcome {
    pub(crate) const fn csf_key(self) -> &'static str {
        match self {
            Self::Empty => "MSG:NothingSelected",
            Self::Map => "MSG:SelAcrossMap",
            Self::Screen => "MSG:SelAcrossScreen",
        }
    }
}

// Cursor identity and software-cursor DTOs are render-owned (F06);
// re-exported so app consumers keep their paths.
pub(crate) use crate::render::cursor_atlas::{
    CursorId, SoftwareCursor, SoftwareCursorFrame, SoftwareCursorSequence,
};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn health_and_type_navigation_share_selection_mode_reset() {
        let mut input = TypeSelectInputState::default();
        input.finish_health_navigation(true);
        assert!(input.health_navigation_continues());
        input.finish_tap(TypeSelectOutcome::Map, true);
        assert!(!input.health_navigation_continues());
        input.finish_health_navigation(true);
        assert!(!input.across_map);
        input.note_successful_selection_mutation(false);
        assert!(!input.health_navigation_continues());
        input.finish_health_navigation(false);
        assert!(!input.health_navigation_continues());
    }

    #[test]
    fn default_skirmish_speed_uses_verified_yr_stored_speed_one() {
        assert_eq!(DEFAULT_YR_SKIRMISH_GAME_SPEED, 1);
        assert_eq!(default_yr_skirmish_tps(), 63);
    }

    #[test]
    fn game_speed_one_is_not_the_old_speed_two_or_options_three_calibration() {
        let default = default_yr_skirmish_tps();
        assert_ne!(default, tps_for_game_speed(2));
        assert_ne!(default, tps_for_game_speed(3));
    }

    #[test]
    fn item83_type_select_release_window_is_inclusive_and_repeat_does_not_rearm() {
        let start = Instant::now();
        let mut state = TypeSelectInputState::default();
        state.press(KeyCode::KeyT, start, false);
        state.press(KeyCode::KeyT, start + Duration::from_millis(400), true);
        assert!(state.release(KeyCode::KeyT, start + Duration::from_millis(500)));

        state.press(KeyCode::KeyT, start, false);
        assert!(!state.release(KeyCode::KeyT, start + Duration::from_millis(501)));
        assert!(!state.held());
    }

    #[test]
    fn item83_type_select_outcomes_use_csf_keys_not_source_line_numbers() {
        assert_eq!(TypeSelectOutcome::Empty.csf_key(), "MSG:NothingSelected");
        assert_eq!(TypeSelectOutcome::Map.csf_key(), "MSG:SelAcrossMap");
        assert_eq!(TypeSelectOutcome::Screen.csf_key(), "MSG:SelAcrossScreen");
    }
}

/// Eight compass directions used for edge-scroll cursor selection.
/// Maps directly to the MoveN..MoveNW frames in mouse.sha (reference §4).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ScrollDir {
    N,
    NE,
    E,
    SE,
    S,
    SW,
    W,
    NW,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CursorFeedbackKind {
    Move,
    AttackMove,
    Guard,
    FriendlyUnit,
    FriendlyStructure,
    EnemyUnit,
    EnemyStructure,
    EnemyOutOfRange,
    /// Harvest — a miner over ore. The action switch sends this to the same
    /// cursor row as an out-of-range attack.
    Harvest,
    Invalid,
    PlaceValid,
    PlaceInvalid,
    /// Edge-scroll arrow shown on the one-pixel outer window edge.
    Scroll(ScrollDir),
    /// Directional barred edge-scroll arrow when the tactical clamp removes
    /// every requested movement component.
    ScrollBlocked(ScrollDir),
    /// The right-drag map pan's own cursor, shown for as long as the drag owns
    /// the camera. Distinct from edge scroll: the pan holds the mouse capture,
    /// which is exactly what stops edge scroll running.
    Pan,
    /// The pan cursor's directional variant, shown when the map cannot scroll
    /// any further the way the table's blocked-direction mask points.
    PanBlocked(ScrollDir),
    /// Move cursor minimap variant (frames 42–51) — shown when hovering over the minimap.
    MinimapMove,
    /// Deploy/undeploy cursor — shown when a Deployer unit hovers over itself.
    Deploy,
    /// Enter cursor — garrison, capture, board transport, sabotage.
    Enter,
    /// Engineer repair cursor — engineer hovering a damaged friendly building.
    EngineerRepair,
    /// C4 plant cursor — SEAL/Tanya/PTROOP hovering a CanC4 enemy structure
    /// (action 0x10 in gamemd, distinct mouse.shp frames from Enter).
    Demolish,
    /// Crazy Ivan bomb cursor (action 0x35 `IvanBomb`, cursor row 38).
    IvanBomb,
    /// Engineer defuse cursor (action 0x39 `DisarmBomb`, cursor row 59).
    DisarmBomb,
    /// Repair cursor mode active (sidebar wrench). `true` = an own building is
    /// under the cursor (wrench), `false` = no eligible target (no-repair).
    RepairMode(bool),
    /// Sell cursor mode active (sidebar dollar). `true` = an own building is
    /// under the cursor (sell), `false` = no eligible target (no-sell).
    SellMode(bool),
    /// Superweapon targeting reticle — shown while a charged SW is armed
    /// and the cursor is over the tactical map. Payload is the per-SW
    /// CursorId resolved from the `Action=` INI string.
    SuperWeaponTarget(CursorId),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum HoverTargetKind {
    FriendlyUnit,
    FriendlyStructure,
    EnemyUnit,
    EnemyStructure,
    HiddenEnemy,
}

/// Mutually-exclusive cursor-on-tactical-map targeting modes.
///
/// Building placement and superweapon targeting cannot both be active at
/// once. Arming one clears the other; right-click and Esc clear both.
/// The variant payload is the type_id (interned section name) the
/// targeting refers to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum TargetingMode {
    /// Ready building waiting to be placed on the tactical map.
    /// Payload: building INI section name (e.g., "GAPOWR").
    BuildingPlacement(String),
    /// Charged superweapon waiting for a target cell.
    /// Payload: SW INI section name (e.g., "LightningStormSpecial").
    SuperWeapon(String),
}

impl TargetingMode {
    pub fn as_building_placement(&self) -> Option<&str> {
        match self {
            Self::BuildingPlacement(s) => Some(s.as_str()),
            _ => None,
        }
    }

    pub fn as_super_weapon(&self) -> Option<&str> {
        match self {
            Self::SuperWeapon(s) => Some(s.as_str()),
            _ => None,
        }
    }
}
