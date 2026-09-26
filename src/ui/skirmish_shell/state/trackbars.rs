//! Trackbar and skirmish shell option input helpers.

use crate::map::scenario_menu::MapMenuEntry;
use crate::rules::ini_parser::IniFile;

use super::super::layout::{
    RectPx, SkirmishCheckboxId, SkirmishShellLayout, SkirmishTrackbarId, checkbox_icon_rect,
};
use super::{
    SkirmishShellAction, SkirmishShellState, SkirmishShellUiSound, handle_combo_mouse_down,
    set_open_combo_top_index, top_index_from_thumb_y,
};
use crate::ui::shell::trackbar::{TrackbarPress, TrackbarRange};

pub const GAME_SPEED_MIN: i32 = 0;
pub const GAME_SPEED_MAX: i32 = 6;
pub const GAME_SPEED_STEP: i32 = 1;
/// The RulesClass constructor's `[MultiplayerDialogSettings]` defaults
/// (`0x00667245..0x00667279`): MinMoney 2500, MaxMoney 10000,
/// MoneyIncrement 100, MinUnitCount 1, MaxUnitCount 20. Retail Rules sets
/// 5000, 10000, 100, 0 and 10.
pub const CREDITS_MIN: i32 = 2500;
pub const CREDITS_MAX: i32 = 10000;
pub const CREDITS_STEP: i32 = 100;
pub const UNIT_COUNT_MIN: i32 = 1;
pub const UNIT_COUNT_MAX: i32 = 20;
pub const UNIT_COUNT_STEP: i32 = 1;

/// Credits and Unit Count slider ranges for dialog 0x102.
///
/// The dialog's setup (`SkirmishDialog__OnSetup497`) sets them from the live
/// Rules instance: Credits `TBM_SETRANGE` MinMoney..MaxMoney and step
/// MoneyIncrement (`0x4AB`) at `0x006AED16..0x006AED59`, Unit Count
/// MinUnitCount..MaxUnitCount at `0x006AED72..0x006AED8E`, each packed as
/// 16-bit words. `RulesClass__ReadMultiplayerDialogSettings` (`0x00671EA0`)
/// reads the keys with `ReadInt` over the constructor's values, so a missing
/// key keeps the constructor default. GameSpeed's range is a literal 0..6, so
/// it is not stored here.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SkirmishTrackbarBounds {
    pub credits_min: i32,
    pub credits_max: i32,
    pub credits_step: i32,
    pub unit_count_min: i32,
    pub unit_count_max: i32,
}

impl Default for SkirmishTrackbarBounds {
    fn default() -> Self {
        Self {
            credits_min: CREDITS_MIN,
            credits_max: CREDITS_MAX,
            credits_step: CREDITS_STEP,
            unit_count_min: UNIT_COUNT_MIN,
            unit_count_max: UNIT_COUNT_MAX,
        }
    }
}

impl SkirmishTrackbarBounds {
    /// A trackbar's range and step. GameSpeed is a hardcoded range (matching
    /// gamemd's literal 0..6); Credits and Unit Count come from the seeded
    /// bounds. Unit Count has no increment message in gamemd, so its step
    /// stays `UNIT_COUNT_STEP`.
    pub fn range(&self, id: SkirmishTrackbarId) -> TrackbarRange {
        match id {
            SkirmishTrackbarId::GameSpeed0x529 => {
                TrackbarRange::new(GAME_SPEED_MIN, GAME_SPEED_MAX, GAME_SPEED_STEP)
            }
            SkirmishTrackbarId::Credits0x511 => {
                TrackbarRange::new(self.credits_min, self.credits_max, self.credits_step)
            }
            SkirmishTrackbarId::UnitCount0x50c => {
                TrackbarRange::new(self.unit_count_min, self.unit_count_max, UNIT_COUNT_STEP)
            }
        }
    }

    /// Seed Credits/Unit Count bounds from a merged rules INI's
    /// `[MultiplayerDialogSettings]` section, mirroring gamemd's runtime read.
    /// Each missing key keeps its stock-default value.
    pub fn from_multiplayer_dialog_settings(ini: &IniFile) -> Self {
        let mut bounds = Self::default();
        let Some(section) = ini.section("MultiplayerDialogSettings") else {
            return bounds;
        };
        if let Some(value) = section.get_i32("MinMoney") {
            bounds.credits_min = value;
        }
        if let Some(value) = section.get_i32("MaxMoney") {
            bounds.credits_max = value;
        }
        if let Some(value) = section.get_i32("MoneyIncrement") {
            bounds.credits_step = value;
        }
        if let Some(value) = section.get_i32("MinUnitCount") {
            bounds.unit_count_min = value;
        }
        if let Some(value) = section.get_i32("MaxUnitCount") {
            bounds.unit_count_max = value;
        }
        bounds
    }
}

impl SkirmishShellState {
    /// The credits the dialog reads from its slider (`TBM_GETPOS` at
    /// `0x006AD742..0x006AD75A`): the thumb's value rounded down to
    /// MoneyIncrement. The value text shows it (`0x0061E27A`), the game
    /// starts with it and the settings keep it (`0x006AD799`).
    pub fn credits(&self) -> i32 {
        self.trackbar_bounds
            .range(SkirmishTrackbarId::Credits0x511)
            .stepped(self.starting_credits)
    }
}

pub const fn game_speed_visual_position(stored_speed: i32) -> i32 {
    GAME_SPEED_MAX - stored_speed
}

pub const fn game_speed_from_visual_position(visual_position: i32) -> i32 {
    GAME_SPEED_MAX - visual_position
}

fn checkbox_value_mut(state: &mut SkirmishShellState, id: SkirmishCheckboxId) -> &mut bool {
    match id {
        SkirmishCheckboxId::ShortGame0x54e => &mut state.short_game,
        SkirmishCheckboxId::McvRepacks0x693 => &mut state.mcv_redeploy,
        SkirmishCheckboxId::CratesAppear0x696 => &mut state.crates,
        SkirmishCheckboxId::SuperWeapons0x69a => &mut state.super_weapons,
        SkirmishCheckboxId::BuildOffAlly0x69d => &mut state.build_off_ally,
    }
}

pub(super) fn trackbar_rect(layout: &SkirmishShellLayout, id: SkirmishTrackbarId) -> RectPx {
    match id {
        SkirmishTrackbarId::GameSpeed0x529 => layout.trackbars.game_speed,
        SkirmishTrackbarId::Credits0x511 => layout.trackbars.credits,
        SkirmishTrackbarId::UnitCount0x50c => layout.trackbars.unit_count,
    }
}

pub fn trackbar_visual_value(state: &SkirmishShellState, id: SkirmishTrackbarId) -> i32 {
    match id {
        SkirmishTrackbarId::GameSpeed0x529 => game_speed_visual_position(state.game_speed),
        SkirmishTrackbarId::Credits0x511 => state.starting_credits,
        SkirmishTrackbarId::UnitCount0x50c => state.unit_count,
    }
}

fn set_trackbar_visual_value(
    state: &mut SkirmishShellState,
    id: SkirmishTrackbarId,
    visual_value: i32,
) {
    match id {
        SkirmishTrackbarId::GameSpeed0x529 => {
            state.game_speed = game_speed_from_visual_position(visual_value);
        }
        SkirmishTrackbarId::Credits0x511 => {
            state.starting_credits = visual_value;
        }
        SkirmishTrackbarId::UnitCount0x50c => {
            state.unit_count = visual_value;
        }
    }
}

fn set_trackbar_visual_value_if_changed(
    state: &mut SkirmishShellState,
    id: SkirmishTrackbarId,
    visual_value: i32,
) -> bool {
    if trackbar_visual_value(state, id) == visual_value {
        return false;
    }
    set_trackbar_visual_value(state, id, visual_value);
    true
}

pub(super) fn trackbar_ids() -> [SkirmishTrackbarId; 3] {
    [
        SkirmishTrackbarId::GameSpeed0x529,
        SkirmishTrackbarId::Credits0x511,
        SkirmishTrackbarId::UnitCount0x50c,
    ]
}

pub fn handle_option_mouse_down(
    state: &mut SkirmishShellState,
    layout: &SkirmishShellLayout,
    maps: &[MapMenuEntry],
    x: i32,
    y: i32,
) -> SkirmishShellAction {
    state.trackbar_hold = None;

    if handle_combo_mouse_down(state, layout, maps, x, y) {
        return SkirmishShellAction::None;
    }

    for checkbox in layout.checkboxes {
        if checkbox_icon_rect(checkbox.rect).contains(x, y) {
            let value = checkbox_value_mut(state, checkbox.id);
            *value = !*value;
            state.push_ui_sound(SkirmishShellUiSound::GuiCheckboxSound);
            return SkirmishShellAction::None;
        }
    }

    for id in trackbar_ids() {
        let rect = trackbar_rect(layout, id);
        let range = state.trackbar_bounds.range(id);
        let visual_value = trackbar_visual_value(state, id);
        let press = range.press(visual_value, x - rect.x, y - rect.y, rect.w, rect.h);
        let Some(hold) = press.hold(id) else {
            continue;
        };
        state.trackbar_hold = Some(hold);
        // A pointer-driven change clicks (`0x0061E6DD`); the dialog has no
        // `WM_HSCROLL` case for the notification sent before it.
        if let TrackbarPress::Jump(value) = press
            && set_trackbar_visual_value_if_changed(state, id, value)
        {
            state.push_ui_sound(SkirmishShellUiSound::GenericClick);
        }
        return SkirmishShellAction::None;
    }

    SkirmishShellAction::None
}

pub fn handle_option_mouse_move(
    state: &mut SkirmishShellState,
    layout: &SkirmishShellLayout,
    maps: &[MapMenuEntry],
    x: i32,
    y: i32,
) -> SkirmishShellAction {
    if let Some(drag) = state.dropdown_scroll_drag {
        if let Some(top_index) =
            top_index_from_thumb_y(state, layout, maps, drag.id, y, drag.grab_offset_y)
        {
            set_open_combo_top_index(state, maps, drag.id, top_index);
        }
        return SkirmishShellAction::None;
    }

    // Only a hold that grabbed the thumb follows the pointer.
    let Some(hold) = state.trackbar_hold.filter(|hold| hold.dragging) else {
        return SkirmishShellAction::None;
    };
    let rect = trackbar_rect(layout, hold.id);
    let value = state
        .trackbar_bounds
        .range(hold.id)
        .value_at(x - rect.x, rect.w);
    if set_trackbar_visual_value_if_changed(state, hold.id, value) {
        state.push_ui_sound(SkirmishShellUiSound::GenericClick);
    }
    SkirmishShellAction::None
}

pub fn handle_option_mouse_up(state: &mut SkirmishShellState) -> SkirmishShellAction {
    state.trackbar_hold = None;
    state.dropdown_scroll_drag = None;
    state.dropdown_scroll_press = None;
    SkirmishShellAction::None
}

/// Mouse-wheel handling for the skirmish shell.
///
/// There is no verified evidence that retail scrolls an open dropdown via the
/// mouse wheel: no wheel handler exists in the owner-draw combo, popup, or
/// scrollbar callbacks. Until a runtime capture proves otherwise, the wheel is
/// inert in the shell and does not consume the event. Do NOT reintroduce a
/// wheel-driven dropdown scroll without that evidence.
pub fn handle_option_mouse_wheel(
    _state: &mut SkirmishShellState,
    _maps: &[MapMenuEntry],
    _lines: f32,
) -> bool {
    false
}

#[cfg(test)]
mod bounds_tests {
    use super::*;

    #[test]
    fn retail_rules_set_the_credits_and_unit_count_ranges() {
        let Some(rules) = crate::rules::retail_ini_fixture::retail_ini("rulesmd.ini") else {
            return;
        };
        let bounds = SkirmishTrackbarBounds::from_multiplayer_dialog_settings(&rules);
        assert_eq!(
            bounds.range(SkirmishTrackbarId::Credits0x511),
            TrackbarRange::new(5000, 10000, 100)
        );
        assert_eq!(
            bounds.range(SkirmishTrackbarId::UnitCount0x50c),
            TrackbarRange::new(0, 10, 1)
        );
    }

    #[test]
    fn default_bounds_match_stock_constants() {
        let bounds = SkirmishTrackbarBounds::default();
        assert_eq!(
            bounds.range(SkirmishTrackbarId::Credits0x511),
            TrackbarRange::new(CREDITS_MIN, CREDITS_MAX, CREDITS_STEP)
        );
        assert_eq!(
            bounds.range(SkirmishTrackbarId::UnitCount0x50c),
            TrackbarRange::new(UNIT_COUNT_MIN, UNIT_COUNT_MAX, UNIT_COUNT_STEP)
        );
        assert_eq!(
            bounds.range(SkirmishTrackbarId::GameSpeed0x529),
            TrackbarRange::new(GAME_SPEED_MIN, GAME_SPEED_MAX, GAME_SPEED_STEP)
        );
    }

    #[test]
    fn modded_multiplayer_dialog_settings_override_credit_and_unit_bounds() {
        let ini = IniFile::from_str(
            "[MultiplayerDialogSettings]\n\
             MinMoney=2000\nMaxMoney=50000\nMoneyIncrement=250\n\
             MinUnitCount=1\nMaxUnitCount=20\n",
        );
        let bounds = SkirmishTrackbarBounds::from_multiplayer_dialog_settings(&ini);
        assert_eq!(
            bounds.range(SkirmishTrackbarId::Credits0x511),
            TrackbarRange::new(2000, 50000, 250)
        );
        assert_eq!(
            bounds.range(SkirmishTrackbarId::UnitCount0x50c),
            TrackbarRange::new(1, 20, UNIT_COUNT_STEP)
        );
        // GameSpeed has a hardcoded range in gamemd; modded keys never touch it.
        assert_eq!(
            bounds.range(SkirmishTrackbarId::GameSpeed0x529),
            TrackbarRange::new(GAME_SPEED_MIN, GAME_SPEED_MAX, GAME_SPEED_STEP)
        );
    }

    #[test]
    fn absent_keys_keep_stock_defaults() {
        let ini = IniFile::from_str("[MultiplayerDialogSettings]\nMinMoney=3000\n");
        let bounds = SkirmishTrackbarBounds::from_multiplayer_dialog_settings(&ini);
        assert_eq!(
            bounds.range(SkirmishTrackbarId::Credits0x511),
            TrackbarRange::new(3000, CREDITS_MAX, CREDITS_STEP)
        );
        assert_eq!(
            bounds.range(SkirmishTrackbarId::UnitCount0x50c),
            TrackbarRange::new(UNIT_COUNT_MIN, UNIT_COUNT_MAX, UNIT_COUNT_STEP)
        );
    }
}
