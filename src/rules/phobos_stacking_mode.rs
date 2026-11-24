//! Phobos `StackingMode`: how a new value combines with an existing one.
//!
//! `TechnoClass_ReceiveDamage_Psychedelic` (`0x701D6B`,
//! `Ext/WarheadType/Hooks.cpp:678-696`) replaces the stock
//! `BerzerkDurationLeft = damage` write (`mov [esi+0x29C], eax` at
//! `0x701D6B`) with `EnumFunctions::CalcValueWithStackingMode`
//! (`Utilities/EnumFunctions.cpp:119-162`) over the warhead's
//! `Psychedelic.StackingMode`, else `[CombatDamage] Psychedelic.StackingMode`.
//! The stock block's flag write, team removal, target clear and Hunt
//! (`0x701D71..0x701DB4`) run only for a positive result; a non-positive one
//! jumps to `StopBerzerk` (`0x701DBA`).
//!
//! ## Dependency rules
//! - Part of rules/ — no dependencies on app/, render/, ui/, sim/ or audio/.

use crate::rules::ini_parser::{IniSection, is_native_none_type_name};

/// `Utilities/Enum.h:265-275`.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Default, Hash, serde::Serialize, serde::Deserialize,
)]
pub enum StackingMode {
    /// Replace the current value.
    #[default]
    Override,
    /// Write the new value only while the current one is zero.
    SetIfZero,
    Min,
    Max,
    Add,
    Subtract,
    Multiply,
    Divide,
}

impl StackingMode {
    /// `detail::read<StackingMode>` (`Utilities/TemplateDef.h:1180-1224`):
    /// case-insensitive names; anything else is rejected, keeping the field.
    fn read(raw: &str) -> Option<Self> {
        if raw.eq_ignore_ascii_case("override") {
            Some(Self::Override)
        } else if raw.eq_ignore_ascii_case("setifzero") {
            Some(Self::SetIfZero)
        } else if raw.eq_ignore_ascii_case("min") {
            Some(Self::Min)
        } else if raw.eq_ignore_ascii_case("max") {
            Some(Self::Max)
        } else if raw.eq_ignore_ascii_case("add") {
            Some(Self::Add)
        } else if raw.eq_ignore_ascii_case("subtract") {
            Some(Self::Subtract)
        } else if raw.eq_ignore_ascii_case("multiply") {
            Some(Self::Multiply)
        } else if raw.eq_ignore_ascii_case("divide") {
            Some(Self::Divide)
        } else {
            None
        }
    }

    /// `EnumFunctions::CalcValueWithStackingMode` (`EnumFunctions.cpp:119-162`)
    /// over the signed `BerzerkDurationLeft` and the incoming damage.
    /// `Add`/`Subtract` wrap like the native 32-bit field, `Multiply` clamps
    /// through `GeneralUtils::SafeMultiply` (`GeneralUtils.cpp:309-319`), and
    /// `Divide` keeps the old value when the new one is zero.
    pub fn combine(self, old: i32, new: i32) -> i32 {
        match self {
            Self::Override => new,
            Self::SetIfZero => {
                if old == 0 {
                    new
                } else {
                    old
                }
            }
            Self::Min => old.min(new),
            Self::Max => old.max(new),
            Self::Add => old.wrapping_add(new),
            Self::Subtract => old.wrapping_sub(new),
            Self::Multiply => i64::from(old)
                .wrapping_mul(i64::from(new))
                .clamp(i64::from(i32::MIN), i64::from(i32::MAX))
                as i32,
            Self::Divide => {
                if new != 0 {
                    old.wrapping_div(new)
                } else {
                    old
                }
            }
        }
    }

    /// `[CombatDamage] Psychedelic.StackingMode`, a
    /// `Valueable<StackingMode>` (`Ext/Rules/Body.h:462,967`, constructor
    /// `Override`; read `Ext/Rules/Body.cpp:551`). A present key always
    /// assigns: the rejected-token arm keeps the field, and the parser's
    /// standalone `override` branch still writes `Override`.
    pub(crate) fn read_valueable(section: &IniSection, key: &str) -> Self {
        section.read_string_with(key, 2048, Self::Override, |current, raw| {
            Self::read(raw).unwrap_or(current)
        })
    }

    /// The warhead key is a `Nullable<StackingMode>`
    /// (`Ext/WarheadType/Body.h:267,569`, unset; read
    /// `Ext/WarheadType/Body.cpp:446`). `<default>` and `INIClass::IsBlank`'s
    /// `none`/`<none>` clear it. `override` is the one name the templated
    /// reader assigns but rejects (`TemplateDef.h:1185-1222` has it in a
    /// standalone `if`), so `Nullable::Read` sets its presence only when a
    /// value was already set.
    pub(crate) fn read_nullable(section: &IniSection, key: &str) -> Option<Self> {
        section.read_string_with(key, 2048, None, |current, raw| {
            if raw.eq_ignore_ascii_case("<default>") || is_native_none_type_name(raw) {
                None
            } else if raw.eq_ignore_ascii_case("override") {
                current.map(|_| Self::Override)
            } else {
                Self::read(raw).or(current)
            }
        })
    }
}

#[cfg(all(test, tests_rules_o_z))]
mod tests {
    use super::StackingMode;
    use crate::rules::combat_damage::CombatDamageDefaults;
    use crate::rules::ini_parser::IniFile;
    use crate::rules::warhead_type::WarheadType;

    /// `EnumFunctions.cpp:119-162` per mode, including the overflow traps:
    /// wrapping add/subtract, SafeMultiply's clamp, and the divide-by-zero
    /// no-change arm.
    #[test]
    fn every_mode_combines_like_calc_value_with_stacking_mode() {
        use StackingMode::*;
        let cases = [
            (Override, 30, 7, 7),
            (SetIfZero, 30, 7, 30),
            (SetIfZero, 0, 7, 7),
            (Min, 30, 7, 7),
            (Min, 0, 7, 0),
            (Max, 30, 7, 30),
            (Add, 30, 7, 37),
            (Add, i32::MAX, 1, i32::MIN),
            (Subtract, 30, 7, 23),
            (Subtract, 0, 7, -7),
            (Multiply, 6, 7, 42),
            (Multiply, i32::MAX, 2, i32::MAX),
            (Multiply, 0, 7, 0),
            (Divide, 30, 7, 4),
            (Divide, -30, 7, -4),
            (Divide, 30, 0, 30),
            (Divide, i32::MIN, -1, i32::MIN),
        ];
        for (mode, old, new, expected) in cases {
            assert_eq!(mode.combine(old, new), expected, "{mode:?}({old}, {new})");
        }
    }

    /// The global `Valueable<StackingMode>` takes the first accepted token
    /// and the last accepted layer value; rejected text keeps it.
    #[test]
    fn the_global_key_reads_case_insensitively_and_keeps_on_rejection() {
        let read = |text: &str| {
            let ini = IniFile::from_str(&format!(
                "[CombatDamage]\nPsychedelic.StackingMode={text}\n"
            ));
            CombatDamageDefaults::from_ini_section(ini.section("CombatDamage").unwrap())
                .phobos_psychedelic_stacking_mode
        };
        assert_eq!(read("override"), StackingMode::Override);
        assert_eq!(read("SETIFZERO"), StackingMode::SetIfZero);
        assert_eq!(read("Min"), StackingMode::Min);
        assert_eq!(read("Max"), StackingMode::Max);
        assert_eq!(read("add"), StackingMode::Add);
        assert_eq!(read("subtract"), StackingMode::Subtract);
        assert_eq!(read("multiply"), StackingMode::Multiply);
        assert_eq!(read("divide"), StackingMode::Divide);
        assert_eq!(
            read("bogus"),
            StackingMode::Override,
            "rejected keeps the default"
        );
    }

    /// The warhead `Nullable<StackingMode>`: unset by default, `<default>`,
    /// `none` and `<none>` clear it, and a lone `override` stays unset
    /// because the templated reader answers false for it. `override` after a
    /// set value still replaces it (the template writes the field).
    #[test]
    fn the_warhead_key_is_nullable_and_override_needs_a_prior_value() {
        let warhead = |text: &str| {
            let ini = IniFile::from_str(&format!(
                "[WH]\nPsychedelic=yes\nPsychedelic.StackingMode={text}\n"
            ));
            WarheadType::from_ini_section("WH", ini.section("WH").unwrap())
                .phobos_psychedelic_stacking_mode
        };
        assert_eq!(warhead(""), None);
        assert_eq!(warhead("add"), Some(StackingMode::Add));
        assert_eq!(warhead("<default>"), None);
        assert_eq!(warhead("none"), None);
        assert_eq!(warhead("<none>"), None);
        assert_eq!(warhead("override"), None, "the template rejects override");
        assert_eq!(warhead("bogus"), None);
    }

    /// Ordered rules layers fold per key like the native per-pass reads: a
    /// rejected later token keeps the earlier accepted value, `override`
    /// after an accepted value replaces it, and `<default>` clears only the
    /// `Nullable` warhead key (the `Valueable` global has no reset arm).
    #[test]
    fn ordered_layers_fold_like_the_native_readers() {
        let mut ini = IniFile::from_str("");
        ini.merge_rules_projection(&IniFile::from_str(
            "[CombatDamage]\nPsychedelic.StackingMode=add\n\
             [WH]\nPsychedelic=yes\nPsychedelic.StackingMode=add\n",
        ));
        let read = |ini: &IniFile| {
            (
                CombatDamageDefaults::from_ini_section(ini.section("CombatDamage").unwrap())
                    .phobos_psychedelic_stacking_mode,
                WarheadType::from_ini_section("WH", ini.section("WH").unwrap())
                    .phobos_psychedelic_stacking_mode,
            )
        };
        assert_eq!(read(&ini), (StackingMode::Add, Some(StackingMode::Add)));
        ini.merge_rules_projection(&IniFile::from_str(
            "[CombatDamage]\nPsychedelic.StackingMode=bogus\n[WH]\nPsychedelic.StackingMode=bogus\n",
        ));
        assert_eq!(
            read(&ini),
            (StackingMode::Add, Some(StackingMode::Add)),
            "a rejected later token keeps the earlier accepted value"
        );
        ini.merge_rules_projection(&IniFile::from_str(
            "[CombatDamage]\nPsychedelic.StackingMode=override\n\
             [WH]\nPsychedelic.StackingMode=override\n",
        ));
        assert_eq!(
            read(&ini),
            (StackingMode::Override, Some(StackingMode::Override)),
            "override after an accepted value replaces it despite the parser's false answer"
        );
        ini.merge_rules_projection(&IniFile::from_str(
            "[CombatDamage]\nPsychedelic.StackingMode=<default>\n\
             [WH]\nPsychedelic.StackingMode=<default>\n",
        ));
        assert_eq!(
            read(&ini),
            (StackingMode::Override, None),
            "Valueable keeps on <default>; Nullable resets"
        );
    }
}
