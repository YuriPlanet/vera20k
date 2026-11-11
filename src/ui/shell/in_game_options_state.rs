//! The in-game projection of the `[Options]` values gamemd's Game Controls
//! (BBB) edits, which their live consumers read. App/ui-level only, never sim/.
//! VERA shows those settings as the in-game menu's first page
//! (`app::input::pause_menu`), which writes here as it commits.
//!
//! Stored native values remain unchanged. GameSpeed adds VERA's value 8 for
//! 90 ticks/s; ScrollRate stays 0..6. DetailLevel is 0..2 direct.

use super::game_speed::SpeedContext;

/// GameSpeed/ScrollRate internal range (0 = fastest .. 6 = slowest).
pub const OPTIONS_SPEED_MAX: u32 = 6;
/// DetailLevel range (0 = low .. 2 = high), direct (not inverted).
pub const OPTIONS_DETAIL_MAX: u32 = 2;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InGameOptionsState {
    /// Internal GameSpeed, projected to a slider by `speed_context`.
    pub game_speed: u32,
    /// Display ordering only; simulation speed remains owned by GameOptions.
    pub speed_context: SpeedContext,
    /// Internal ScrollRate 0..6 (0 = fastest).
    pub scroll_rate: u32,
    /// DetailLevel 0..2 (direct).
    pub detail_level: u32,
    pub unit_action_lines: bool,
    pub show_hidden: bool,
    pub tooltips: bool,
    /// BBB's init hides the GameSpeed row for this open
    /// (`app::input::in_game_options::on_open`).
    pub game_speed_hidden: bool,
}

impl Default for InGameOptionsState {
    fn default() -> Self {
        // gamemd OptionsClass::SetDefaults: GameSpeed 3, ScrollRate 3,
        // DetailLevel 2, UnitActionLines 1, ShowHidden 0, ToolTips 1.
        Self {
            game_speed: 3,
            speed_context: SpeedContext::Offline,
            scroll_rate: 3,
            detail_level: 2,
            unit_action_lines: true,
            show_hidden: false,
            tooltips: true,
            game_speed_hidden: false,
        }
    }
}

impl InGameOptionsState {
    /// Where the Game Speed slider stands for the speed in effect.
    pub fn game_speed_position(&self) -> u32 {
        self.speed_context
            .visual_position(self.game_speed as i32)
            .max(0) as u32
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_match_gamemd_setdefaults() {
        let s = InGameOptionsState::default();
        assert_eq!((s.game_speed, s.scroll_rate, s.detail_level), (3, 3, 2));
        assert!(s.unit_action_lines && !s.show_hidden && s.tooltips);
        assert_eq!(s.game_speed_position(), 3);
    }
}
