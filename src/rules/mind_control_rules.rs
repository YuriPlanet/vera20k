//! Mind-control globals `RulesClass` reads: the capture ring anim, the
//! Mastermind overload table, the three mind-control sounds and the AI
//! capture-decision tables.
//!
//! The per-type keys (`ImmuneToPsionics=`, `MindControlRingOffset=`, a type's
//! own `MindClearedSound=`) live on `ObjectType`; the per-weapon ones
//! (`InfiniteMindControl=`) on `WeaponType`; `MindControl=` on `WarheadType`.
//!
//! ## Dependency rules
//! - Part of rules/ — no dependencies on sim/, render/, ui/, etc.

use crate::rules::ini_parser::{IniFile, IniSection};

/// The four `[General] AICapture*=` choice tables `DecideUnitFate
/// @ 0x004723B0` rolls against, indexed by its reason.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CaptureReason {
    /// `AICaptureLowMoney=` (Rules `+0xEA0`).
    LowMoney,
    /// `AICaptureLowPower=` (Rules `+0xE84`).
    LowPower,
    /// `AICaptureWounded=` (Rules `+0xE68`).
    Wounded,
    /// `AICaptureNormal=` (Rules `+0xE4C`).
    Normal,
}

#[derive(Debug, Clone, Default)]
pub struct MindControlRules {
    /// `[CombatDamage] ControlledAnimationType=` (Rules `+0x320`, read at
    /// `0x0066CA11`): the ring a captured object wears (stock `MINDANIM`).
    pub controlled_anim: Option<String>,
    /// `[CombatDamage] OverloadCount=` (Rules `+0xEE8`, `0x0066C7A4`): the
    /// captive counts that select each overload row.
    pub overload_count: Vec<i32>,
    /// `[CombatDamage] OverloadDamage=` (Rules `+0xF04`, `0x0066C823`).
    pub overload_damage: Vec<i32>,
    /// `[CombatDamage] OverloadFrames=` (Rules `+0xF20`, `0x0066C89D`).
    pub overload_frames: Vec<i32>,
    /// `[AudioVisual] YuriMindControlSound=` (Rules `+0x214`, `0x00669AEF`).
    pub mind_control_sound: Option<String>,
    /// `[AudioVisual] MindClearedSound=` (Rules `+0x264`, `0x0066A0D6`).
    pub mind_cleared_sound: Option<String>,
    /// `[AudioVisual] MasterMindOverloadDeathSound=` (Rules `+0x258`,
    /// `0x0066A011`).
    pub overload_sound: Option<String>,
    /// `[General] AICaptureLowMoney=`, `AICaptureLowPower=`,
    /// `AICaptureWounded=`, `AICaptureNormal=` (int lists, `0x006703FB..`),
    /// in [`CaptureReason`] order.
    pub ai_capture: [Vec<i32>; 4],
    /// `[General] AICaptureLowMoneyMark=` (Rules `+0xEBC`, `0x006704D8`).
    pub ai_capture_low_money_mark: i32,
    /// `[General] AICaptureWoundedMark=` (Rules `+0xEC0`, a float,
    /// `0x006704FE`). The constructor leaves both marks unset; stock authors
    /// every key of this struct.
    pub ai_capture_wounded_mark: f32,
}

impl MindControlRules {
    pub fn from_ini(ini: &IniFile) -> Self {
        let combat = ini.section("CombatDamage");
        let audio = ini.section("AudioVisual");
        let general = ini.section("General");
        let list = |section: Option<&IniSection>, key: &str| -> Vec<i32> {
            section
                .and_then(|section| section.get_list(key))
                .map(|items| {
                    items
                        .iter()
                        .filter_map(|item| item.trim().parse::<i32>().ok())
                        .collect()
                })
                .unwrap_or_default()
        };
        Self {
            controlled_anim: combat.and_then(|s| name(s, "ControlledAnimationType")),
            overload_count: list(combat, "OverloadCount"),
            overload_damage: list(combat, "OverloadDamage"),
            overload_frames: list(combat, "OverloadFrames"),
            mind_control_sound: audio.and_then(|s| name(s, "YuriMindControlSound")),
            mind_cleared_sound: audio.and_then(|s| name(s, "MindClearedSound")),
            overload_sound: audio.and_then(|s| name(s, "MasterMindOverloadDeathSound")),
            ai_capture: [
                list(general, "AICaptureLowMoney"),
                list(general, "AICaptureLowPower"),
                list(general, "AICaptureWounded"),
                list(general, "AICaptureNormal"),
            ],
            ai_capture_low_money_mark: general
                .and_then(|s| s.get_i32("AICaptureLowMoneyMark"))
                .unwrap_or(0),
            ai_capture_wounded_mark: general
                .and_then(|s| s.get_f32("AICaptureWoundedMark"))
                .unwrap_or(0.0),
        }
    }

    /// The choice table for `reason`.
    pub fn ai_capture_table(&self, reason: CaptureReason) -> &[i32] {
        let index = match reason {
            CaptureReason::LowMoney => 0,
            CaptureReason::LowPower => 1,
            CaptureReason::Wounded => 2,
            CaptureReason::Normal => 3,
        };
        &self.ai_capture[index]
    }
}

fn name(section: &IniSection, key: &str) -> Option<String> {
    section
        .get(key)
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty() && !value.eq_ignore_ascii_case("none"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stock_keys_parse() {
        let ini = IniFile::from_str(
            "[General]\nAICaptureNormal=75,5,5,15\nAICaptureWounded=15,40,40,5\n\
             AICaptureLowPower=15,5,75,5\nAICaptureLowMoney=15,75,5,5\n\
             AICaptureLowMoneyMark=2000\nAICaptureWoundedMark=.25\n\
             [CombatDamage]\nOverloadCount=3,6,10,50\nOverloadDamage=0,50,100,500\n\
             OverloadFrames=30,60,60,60\nControlledAnimationType=MINDANIM\n\
             [AudioVisual]\nYuriMindControlSound=YuriMindControl\n\
             MasterMindOverloadDeathSound=MasterMindOverloadVoice\nMindClearedSound=MindCleared\n",
        );
        let rules = MindControlRules::from_ini(&ini);
        assert_eq!(rules.controlled_anim.as_deref(), Some("MINDANIM"));
        assert_eq!(rules.overload_count, [3, 6, 10, 50]);
        assert_eq!(rules.overload_damage, [0, 50, 100, 500]);
        assert_eq!(rules.overload_frames, [30, 60, 60, 60]);
        assert_eq!(
            rules.ai_capture_table(CaptureReason::Normal),
            [75, 5, 5, 15]
        );
        assert_eq!(
            rules.ai_capture_table(CaptureReason::LowMoney),
            [15, 75, 5, 5]
        );
        assert_eq!(rules.ai_capture_low_money_mark, 2000);
        assert_eq!(rules.ai_capture_wounded_mark, 0.25);
        assert_eq!(rules.mind_cleared_sound.as_deref(), Some("MindCleared"));
    }
}
