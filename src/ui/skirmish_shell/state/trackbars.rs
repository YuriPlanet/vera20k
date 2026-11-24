//! The skirmish sliders' ranges.

use crate::rules::ini_parser::IniFile;

use super::super::layout::SkirmishTrackbarId;
use super::SkirmishShellState;
use vera20k_gui::shell::trackbar::TrackbarRange;

pub const GAME_SPEED_MIN: i32 = 0;
pub const GAME_SPEED_MAX: i32 = crate::ui::shell::game_speed::MAX_POSITION;
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
const EXTRA_CREDITS: [i32; 2] = [30_000, 100_000];

/// Credits and Unit Count slider ranges for dialog 0x102.
///
/// The dialog's setup (`SkirmishDialog__OnSetup497`) sets them from the live
/// Rules instance: Credits `TBM_SETRANGE` MinMoney..MaxMoney and step
/// MoneyIncrement (`0x4AB`) at `0x006AED16..0x006AED59`, Unit Count
/// MinUnitCount..MaxUnitCount at `0x006AED72..0x006AED8E`, each packed as
/// 16-bit words. `RulesClass__ReadMultiplayerDialogSettings` (`0x00671EA0`)
/// reads the keys with `ReadInt` over the constructor's values, so a missing
/// key keeps the constructor default. VERA extends the controls with a 90-tick
/// speed and high-credit choices without changing the native bounds reader.
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
    fn preset_span(&self) -> i32 {
        let step = self
            .range(SkirmishTrackbarId::Credits0x511)
            .step
            .saturating_abs()
            .max(1);
        // Give each extra choice about a tenth of the ordinary rail. A single
        // MoneyIncrement would leave it narrower than a pixel in many mods.
        let span = self.credits_max.saturating_sub(self.credits_min) / 10;
        (span / step).max(1) * step
    }

    /// VERA appends two exact credit choices after the rules-owned range.
    /// Compressing them into two notches keeps ordinary MoneyIncrement values
    /// usable on the short rail and makes both high amounts reachable by mouse.
    pub fn visual_range(&self, id: SkirmishTrackbarId) -> TrackbarRange {
        let mut range = self.range(id);
        if id == SkirmishTrackbarId::Credits0x511 {
            let extra = EXTRA_CREDITS
                .iter()
                .filter(|&&v| v > self.credits_max)
                .count();
            range.max = range
                .max
                .saturating_add((extra as i32).saturating_mul(self.preset_span()));
        }
        range
    }

    pub fn credits_visual_position(&self, credits: i32) -> i32 {
        let step = self.preset_span();
        EXTRA_CREDITS
            .iter()
            .filter(|&&v| v > self.credits_max)
            .position(|&v| v == credits)
            .map_or(credits, |index| {
                self.credits_max + (index as i32 + 1) * step
            })
    }

    pub fn credits_from_visual_position(&self, position: i32) -> i32 {
        if position <= self.credits_max {
            return position;
        }
        let step = self.preset_span();
        let index = (position - self.credits_max - 1) / step;
        EXTRA_CREDITS
            .iter()
            .copied()
            .filter(|&v| v > self.credits_max)
            .nth(index as usize)
            .unwrap_or(self.credits_max)
    }

    pub fn accepts_credits(&self, credits: i32) -> bool {
        (self.credits_min..=self.credits_max).contains(&credits)
            || (credits > self.credits_max && EXTRA_CREDITS.contains(&credits))
    }

    /// Added presets survive reopening; ordinary values retain TBM_SETPOS's
    /// native handling of values outside the rules-owned range.
    pub fn set_up_credits(&self, credits: i32) -> i32 {
        if self.accepts_credits(credits) {
            credits
        } else {
            self.range(SkirmishTrackbarId::Credits0x511).set_up(credits)
        }
    }

    pub fn credits_value(&self, credits: i32) -> i32 {
        if credits > self.credits_max && EXTRA_CREDITS.contains(&credits) {
            credits
        } else {
            self.range(SkirmishTrackbarId::Credits0x511)
                .stepped(credits)
        }
    }

    /// Native money/unit bounds, and VERA's extended speed range. Rendering
    /// and pointer input use `visual_range` to include high-credit choices.
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
        // ReadInt over each current bound (`0x00671ED7`-`0x00671F95`).
        let section = ini.section_or_empty("MultiplayerDialogSettings");
        bounds.credits_min = section.read_int("MinMoney", bounds.credits_min);
        bounds.credits_max = section.read_int("MaxMoney", bounds.credits_max);
        bounds.credits_step = section.read_int("MoneyIncrement", bounds.credits_step);
        bounds.unit_count_min = section.read_int("MinUnitCount", bounds.unit_count_min);
        bounds.unit_count_max = section.read_int("MaxUnitCount", bounds.unit_count_max);
        bounds
    }
}

impl SkirmishShellState {
    /// The credits the dialog reads from its slider (`TBM_GETPOS` at
    /// `0x006AD742..0x006AD75A`): the thumb's value rounded down to
    /// MoneyIncrement. The value text shows it (`0x0061E27A`), the game
    /// starts with it and the settings keep it (`0x006AD799`).
    pub fn credits(&self) -> i32 {
        self.trackbar_bounds.credits_value(self.starting_credits)
    }
}

#[cfg(all(test, tests_ui))]
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
