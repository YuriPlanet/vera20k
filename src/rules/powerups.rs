//! `[Powerups]` — the fixed nineteen-entry crate outcome table.
//!
//! `RulesClass__ReadPowerups @ 0x00673E80` does not read the INI section's own
//! keys. It walks a hardcoded nineteen-pointer name table at `0x007E523C` and
//! asks the INI for each name in turn, so the slot order is a property of the
//! binary and never of the file. Every value lands in one of four parallel
//! globals rather than in `RulesClass` itself.
//!
//! The read is `CCINIClass::ReadString` with default `"0,NONE"` (verified at
//! `0x0083D4AC`), then up to four comma-separated tokens:
//!
//! | Token | Global | Width | Extractor |
//! |---|---|---|---|
//! | 1 | `0x0081DA8C` | `i32[19]` | `strtrim` then `atoi` |
//! | 2 | `0x0081DAD8` | `i32[19]` | `strtrim` then `AnimTypeClass::Find_Index @ 0x00422B20` |
//! | 3 | `0x0089ECC0` | `u8[19]` | `strtrim`, then exact `"yes"`/`"no"` compare |
//! | 4 | `0x0089EC28` | `f64[19]` | `atof`; with a `%`, untrimmed and times the double 0.01 (`0x00673FAF`, chopped) |
//!
//! A missing token leaves that slot's previous value untouched — so the stock
//! rows that stop after three tokens (`HealBase`, `Reveal`, `Veteran`, `Unit`,
//! …) never write a magnitude, and a token-three value that is neither literal
//! leaves the flag alone. Because the default string carries only two tokens, a
//! `[Powerups]` section that omits a name still zeroes its weight and looks up
//! the default's bare `NONE`. That name is not `Find_Index`'s `<none>` sentinel:
//! it can resolve to a registered type, and otherwise stores -1 immediately.
//!
//! ## Dependency rules
//! Part of `rules/` — INI parsing and rule data only.

use crate::rules::ini_parser::IniFile;
use crate::rules::ini_value::{crt_atoi, parse_leading_f64, scale_percent, strtok, strtrim_ascii};
use crate::util::native_x87::NativeF64Bits;

/// Slots in the hardcoded name table at `0x007E523C`. The loop bound is
/// `pdVar5 < 0x0089ECC0` over an `f64` cursor starting at `0x0089EC28`:
/// `(0x0089ECC0 - 0x0089EC28) / 8 = 19`.
pub const POWERUP_COUNT: usize = 19;

/// Canonical slot order, read directly from the pointer table at `0x007E523C`
/// and its target literals. This ordering is load-bearing: `CellClass__PickupCrate`
/// indexes all four globals, the `[CrateRules]` solo mappings, and its own jump
/// table by these positions, and the INI's own line order is irrelevant.
pub const POWERUP_NAMES: [&str; POWERUP_COUNT] = [
    "Money",
    "Unit",
    "HealBase",
    "Cloak",
    "Explosion",
    "Napalm",
    "Squad",
    "Darkness",
    "Reveal",
    "Armor",
    "Speed",
    "Firepower",
    "ICBM",
    "Invulnerability",
    "Veteran",
    "IonStorm",
    "Gas",
    "Tiberium",
    "Pod",
];

/// Slot indices the rest of the crate system names directly.
pub const POWERUP_MONEY: usize = 0;
pub const POWERUP_UNIT: usize = 1;
pub const POWERUP_HEAL_BASE: usize = 2;
pub const POWERUP_SQUAD: usize = 6;
pub const POWERUP_REVEAL: usize = 8;
pub const POWERUP_ARMOR: usize = 9;
pub const POWERUP_SPEED: usize = 10;
pub const POWERUP_FIREPOWER: usize = 11;
pub const POWERUP_VETERAN: usize = 14;

/// Static image defaults for the weight global at `0x0081DA8C`, read from the
/// binary's initialized `.data`. These are the pre-INI values; stock
/// `rulesmd.ini` overwrites every one of them.
const DEFAULT_WEIGHTS: [i32; POWERUP_COUNT] = [
    50, 20, 1, 3, 5, 5, 20, 1, 1, 10, 10, 10, 1, 3, 1, 1, 1, 1, 1,
];

/// `CCINIClass::ReadString` capacity supplied by `ReadPowerups`.
const READ_STRING_CAPACITY: usize = 0x80;

/// The default handed to every `ReadString` call, verified at `0x0083D4AC`.
/// Two tokens only: an absent row zeroes the weight and looks up `NONE`, while
/// the flag and magnitude keep their live values.
const DEFAULT_ROW: &str = "0,NONE";

/// `Powerup_From_Name @ 0x0048DE70`: case-insensitive walk of the same fixed
/// name table, returning the slot index. A null or unmatched name yields slot
/// zero — `Money` — rather than a failure, which is what the `[CrateRules]`
/// solo-play mappings rely on.
pub fn powerup_from_name(name: &str) -> usize {
    POWERUP_NAMES
        .iter()
        .position(|candidate| candidate.eq_ignore_ascii_case(name))
        .unwrap_or(POWERUP_MONEY)
}

/// The four parallel `[Powerups]` globals.
///
/// Native keeps these outside `RulesClass`; the process owner retains this
/// table across its Rules passes and type-registry resets.
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct PowerupTable {
    /// Token 1 — selection weight. `CellClass__PickupCrate` sums all
    /// nineteen and draws `RandomRanged(1, total)`.
    pub weights: [i32; POWERUP_COUNT],
    /// Token 2 — the immediate `AnimTypeClass::Find_Index422B20` result.
    /// Native `-1` means the sentinel or a name absent at this read boundary.
    anim_indices: [i32; POWERUP_COUNT],
    /// Token 3 — over-water eligibility, NOT an "enabled" switch. It is read
    /// only when the crate's cell land type is water (`0x00481D52`), and a
    /// cleared flag falls the outcome back to `Money` instead of suppressing
    /// the crate.
    pub over_water: [bool; POWERUP_COUNT],
    /// Token 4 — the per-outcome magnitude, in native binary64.
    pub magnitudes: [NativeF64Bits; POWERUP_COUNT],
}

impl Default for PowerupTable {
    fn default() -> Self {
        Self {
            weights: DEFAULT_WEIGHTS,
            // `0x0081DAD8` is initialized to -1 across all nineteen slots.
            anim_indices: [-1; POWERUP_COUNT],
            // `0x0089ECC0` and `0x0089EC28` live in zero-initialized storage.
            over_water: [false; POWERUP_COUNT],
            magnitudes: [NativeF64Bits::from_bits(0); POWERUP_COUNT],
        }
    }
}

impl PowerupTable {
    /// The index selected at the last reached read, as PickupCrate4832F9
    /// consumes it. An invalid slot has no animation.
    pub fn animation_index(&self, slot: usize) -> i32 {
        self.anim_indices.get(slot).copied().unwrap_or(-1)
    }

    pub(crate) fn animation_indices(&self) -> &[i32; POWERUP_COUNT] {
        &self.anim_indices
    }

    /// RulesClass::ReadPowerups673E80 reads against the AnimType registry at
    /// this physical pass. Unknown names store -1; missing tokens retain.
    pub(crate) fn apply_pass(
        &mut self,
        ini: &IniFile,
        mut find_animation: impl FnMut(&str) -> Option<usize>,
    ) {
        // `INIClass__FindSectionByName` gates the whole read: with no
        // `[Powerups]` section the function returns before touching a slot.
        let Some(section) = ini.section("Powerups") else {
            return;
        };
        for (slot, name) in POWERUP_NAMES.iter().enumerate() {
            // Native always parses, falling back to the literal `"0,NONE"`, so
            // an absent row still zeroes the weight and looks up NONE while
            // leaving the flag and magnitude untouched.
            let raw = section.read_string(name, DEFAULT_ROW, READ_STRING_CAPACITY);
            // `CRT__strtok` collapses runs of the delimiter and skips leading
            // ones, so an empty field is not a token at all: `20,,yes,2000`
            // yields three tokens, not four.
            let mut tokens = strtok(&raw, &[',']);

            if let Some(token) = tokens.next() {
                self.weights[slot] = crt_atoi(strtrim_ascii(token));
            }
            if let Some(token) = tokens.next() {
                let token = strtrim_ascii(token);
                // `AnimTypeClass::Find_Index @ 0x00422B20` special-cases only
                // the literal `<none>`; every other name falls through to the
                // current array search. The returned index is stored even on
                // failure (673F2A..673F2F); later allocation cannot bind it.
                self.anim_indices[slot] = if token.eq_ignore_ascii_case("<none>") {
                    -1
                } else {
                    find_animation(token)
                        .and_then(|index| i32::try_from(index).ok())
                        .unwrap_or(-1)
                };
            }
            if let Some(token) = tokens.next() {
                // Exactly two literals are recognised (`0x00825BF8` "yes" and
                // `0x00825BF4` "no"); anything else leaves the slot as it was.
                let token = strtrim_ascii(token);
                if token.eq_ignore_ascii_case("yes") {
                    self.over_water[slot] = true;
                } else if token.eq_ignore_ascii_case("no") {
                    self.over_water[slot] = false;
                }
            }
            if let Some(token) = tokens.next() {
                // The percent branch skips `strtrim` and scales by 0.01 as
                // ReadDouble does; the plain branch trims first. Both then run
                // the same `atof`.
                let value = if token.contains('%') {
                    scale_percent(parse_leading_f64(token))
                } else {
                    parse_leading_f64(strtrim_ascii(token))
                };
                self.magnitudes[slot] = NativeF64Bits::from_bits(value.to_bits());
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ANIMATIONS: &[&str] = &[
        "MONEY", "ARMOR", "FIREPOWR", "HEALALL", "REVEAL", "SPEED", "VETERAN", "CLOAK", "SHROUDX",
        "CHEMISLE", "YES",
    ];

    fn find_animation(name: &str) -> Option<usize> {
        ANIMATIONS
            .iter()
            .position(|candidate| candidate.eq_ignore_ascii_case(name))
    }

    fn table(section: &str) -> PowerupTable {
        let mut table = PowerupTable::default();
        table.apply_pass(&IniFile::from_str(section), find_animation);
        table
    }

    /// `Powerup_From_Name` never fails: an unmatched or empty name is Money.
    #[test]
    fn powerup_from_name_is_case_insensitive_and_falls_back_to_money() {
        assert_eq!(powerup_from_name("HealBase"), POWERUP_HEAL_BASE);
        assert_eq!(powerup_from_name("healbase"), POWERUP_HEAL_BASE);
        assert_eq!(powerup_from_name("MONEY"), POWERUP_MONEY);
        assert_eq!(powerup_from_name("Veteran"), POWERUP_VETERAN);
        assert_eq!(powerup_from_name("NotAPowerup"), POWERUP_MONEY);
        assert_eq!(powerup_from_name(""), POWERUP_MONEY);
    }

    /// No section at all leaves every static image default in place.
    #[test]
    fn missing_section_retains_the_static_image_defaults() {
        let parsed = table("[General]\n");
        assert_eq!(parsed, PowerupTable::default());
        assert_eq!(parsed.weights[POWERUP_MONEY], 50);
        assert_eq!(parsed.weights[POWERUP_SQUAD], 20);
        assert_eq!(parsed.weights[18], 1);
        assert_eq!(parsed.animation_indices(), &[-1; POWERUP_COUNT]);
        assert_eq!(parsed.animation_index(POWERUP_COUNT), -1);
        assert_eq!(parsed.animation_index(usize::MAX), -1);
        assert!(parsed.over_water.iter().all(|flag| !flag));
    }

    /// A present section but an absent row still applies the `"0,NONE"`
    /// default's first two tokens, and leaves the last two alone.
    #[test]
    fn absent_row_zeroes_weight_and_anim_but_keeps_flag_and_magnitude() {
        let mut parsed = table("[Powerups]\nMoney=20,MONEY,yes,2000\n");
        // Second pass: the section exists but Money is gone.
        parsed.apply_pass(
            &IniFile::from_str("[Powerups]\nArmor=10,ARMOR,yes,1.5\n"),
            find_animation,
        );
        assert_eq!(
            parsed.weights[POWERUP_MONEY], 0,
            "the default's first token"
        );
        // The default row's second token is a bare `NONE`, which is NOT
        // Find_Index's `<none>` sentinel: it falls through the array search and
        // yields -1 by failing to match the registry at this read boundary.
        assert_eq!(
            parsed.animation_index(POWERUP_MONEY),
            -1,
            "no registered AnimType is called NONE, so the slot has no animation"
        );
        assert!(
            parsed.over_water[POWERUP_MONEY],
            "no third token, so the live flag survives"
        );
        assert_eq!(
            f64::from_bits(parsed.magnitudes[POWERUP_MONEY].bits()),
            2000.0,
            "no fourth token, so the live magnitude survives"
        );
    }

    /// The stock section, parsed exactly. Weights are the selection authority,
    /// so pin the whole vector and its total.
    #[test]
    fn stock_section_parses_the_verified_weight_vector() {
        let parsed = table(
            "[Powerups]\n\
             Armor=10,ARMOR,yes,1.5\n\
             Firepower=10,FIREPOWR,yes,2.0\n\
             HealBase=10,HEALALL,yes\n\
             Money=20,MONEY,yes,2000\n\
             Reveal=10,REVEAL,yes\n\
             Speed=10,SPEED,yes,1.2\n\
             Veteran=20,VETERAN,yes,1\n\
             Unit=20,<none>,no\n\
             Invulnerability=0,ARMOR,yes,1.0\n\
             IonStorm=0,<none>,yes\n\
             Gas=0,<none>,yes,100\n\
             Tiberium=0,<none>,no\n\
             Pod=0,<none>,no\n\
             Cloak=0,CLOAK,yes\n\
             Darkness=0,SHROUDX,yes\n\
             Explosion=0,<none>,yes,500\n\
             ICBM=0,CHEMISLE,yes\n\
             Napalm=0,<none>,no,600\n\
             Squad=0,<none>,no\n",
        );

        assert_eq!(
            parsed.weights,
            [
                20, 20, 10, 0, 0, 0, 0, 0, 10, 10, 10, 10, 0, 0, 20, 0, 0, 0, 0
            ]
        );
        assert_eq!(parsed.weights.iter().sum::<i32>(), 110);

        // `<none>` is the no-type sentinel; real names resolve immediately.
        assert_eq!(parsed.animation_index(POWERUP_UNIT), -1);
        assert_eq!(parsed.animation_index(POWERUP_MONEY), 0);
        assert_eq!(parsed.animation_index(POWERUP_FIREPOWER), 2);

        // Over-water eligibility, not an enable switch: Unit is weighted 20 and
        // still drops on land, but is redirected to Money over water.
        assert!(!parsed.over_water[POWERUP_UNIT]);
        assert!(parsed.over_water[POWERUP_MONEY]);
        assert!(!parsed.over_water[17], "Tiberium");
        assert!(!parsed.over_water[POWERUP_SQUAD]);

        assert_eq!(
            f64::from_bits(parsed.magnitudes[POWERUP_MONEY].bits()),
            2000.0
        );
        assert_eq!(f64::from_bits(parsed.magnitudes[POWERUP_ARMOR].bits()), 1.5);
        assert_eq!(f64::from_bits(parsed.magnitudes[POWERUP_SPEED].bits()), 1.2);
        assert_eq!(
            f64::from_bits(parsed.magnitudes[POWERUP_VETERAN].bits()),
            1.0
        );
        // Three-token rows never write a magnitude.
        assert_eq!(
            f64::from_bits(parsed.magnitudes[POWERUP_HEAL_BASE].bits()),
            0.0
        );
        assert_eq!(
            f64::from_bits(parsed.magnitudes[POWERUP_REVEAL].bits()),
            0.0
        );
    }

    /// The percent branch scales by 0.01 and deliberately skips the trim the
    /// plain branch performs.
    #[test]
    fn percent_magnitude_is_scaled_and_plain_magnitude_is_trimmed() {
        let parsed = table("[Powerups]\nArmor=1, ARMOR , yes , 50%\nSpeed=1,SPEED,yes,  1.25  \n");
        assert_eq!(f64::from_bits(parsed.magnitudes[POWERUP_ARMOR].bits()), 0.5);
        assert_eq!(
            f64::from_bits(parsed.magnitudes[POWERUP_SPEED].bits()),
            1.25
        );
        assert_eq!(parsed.animation_index(POWERUP_ARMOR), 1);
        assert!(parsed.over_water[POWERUP_ARMOR]);
    }

    /// Every integer percent reads the double native ReadDouble's executed
    /// sweep stored (`tools/rules_oracle/read_double_percent.json`): `atof` of
    /// an integer is the widened float, and `0x00673FAF` is the same chopped
    /// `fmul qword [0x007E3808]` as ReadDouble's `0x0052857E`.
    #[test]
    fn percent_magnitude_is_the_chopped_native_product() {
        let native: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
            "tools/rules_oracle/read_double_percent.json",
        ))
        .unwrap();
        let sweep = native["reader"]["percent_sweep"].as_array().unwrap();
        assert_eq!(sweep.len(), 1001);
        for (percent, bits) in sweep.iter().enumerate() {
            let parsed = table(&format!("[Powerups]\nArmor=1,ARMOR,yes, {percent}%\n"));
            assert_eq!(
                parsed.magnitudes[POWERUP_ARMOR].bits(),
                u64::from_str_radix(bits.as_str().unwrap(), 16).unwrap(),
                "{percent}%"
            );
        }
    }

    /// Token three recognises exactly two literals; anything else is inert.
    #[test]
    fn unrecognised_third_token_leaves_the_live_flag_untouched() {
        let mut parsed = table("[Powerups]\nArmor=1,ARMOR,yes,1\n");
        parsed.apply_pass(
            &IniFile::from_str("[Powerups]\nArmor=1,ARMOR,true,1\n"),
            find_animation,
        );
        assert!(
            parsed.over_water[POWERUP_ARMOR],
            "`true` is neither literal, so the `yes` from the earlier pass stands"
        );

        parsed.apply_pass(
            &IniFile::from_str("[Powerups]\nArmor=1,ARMOR,no,1\n"),
            find_animation,
        );
        assert!(!parsed.over_water[POWERUP_ARMOR]);
    }

    /// A missing token retains the prior index; an unknown name replaces it
    /// with -1, even when a later pass can resolve that name.
    #[test]
    fn animation_lookup_retains_only_resolved_indices() {
        let mut parsed = table("[Powerups]\nMoney=20,money,yes,2000\n");
        assert_eq!(
            parsed.animation_index(POWERUP_MONEY),
            0,
            "resolution is case-insensitive, like Find_Index"
        );
        parsed.apply_pass(&IniFile::from_str("[Powerups]\nMoney=30\n"), find_animation);
        assert_eq!(parsed.animation_index(POWERUP_MONEY), 0);
        parsed.apply_pass(
            &IniFile::from_str("[Powerups]\nMoney=20,BOGUSANIM,yes,2000\n"),
            find_animation,
        );
        assert_eq!(parsed.animation_index(POWERUP_MONEY), -1);
        let mut lookups = 0;
        let mut later_catalog = |name: &str| {
            lookups += 1;
            name.eq_ignore_ascii_case("BOGUSANIM").then_some(11)
        };
        parsed.apply_pass(&IniFile::from_str("[General]\n"), &mut later_catalog);
        assert_eq!(parsed.animation_index(POWERUP_MONEY), -1);
        parsed.apply_pass(
            &IniFile::from_str("[Powerups]\nMoney=20,BOGUSANIM\n"),
            &mut later_catalog,
        );
        assert_eq!(parsed.animation_index(POWERUP_MONEY), 11);
        assert_eq!(
            lookups, POWERUP_COUNT,
            "only the last pass performs lookups"
        );
    }

    /// `strtok` skips empty fields, so a doubled or leading comma shifts which
    /// token feeds which slot.
    #[test]
    fn empty_fields_are_not_tokens() {
        let parsed = table("[Powerups]\nMoney=20,,yes,2000\n");
        assert_eq!(parsed.weights[POWERUP_MONEY], 20);
        assert_eq!(
            parsed.animation_index(POWERUP_MONEY),
            10,
            "the third field became the second token"
        );
        assert!(
            !parsed.over_water[POWERUP_MONEY],
            "`2000` is neither literal, so the flag keeps its default"
        );
        assert_eq!(
            f64::from_bits(parsed.magnitudes[POWERUP_MONEY].bits()),
            0.0,
            "there is no fourth token"
        );

        let parsed = table("[Powerups]\nMoney=,20,MONEY\n");
        assert_eq!(
            parsed.weights[POWERUP_MONEY], 20,
            "a leading delimiter is skipped, not treated as an empty first token"
        );
    }

    /// `atof` accepts an exponent, but only with at least one digit after it.
    #[test]
    fn magnitude_accepts_exponent_notation() {
        let parsed = table("[Powerups]\nGas=1,<none>,yes,1e3\nArmor=1,ARMOR,yes,2.5E-2\n");
        assert_eq!(f64::from_bits(parsed.magnitudes[16].bits()), 1000.0);
        assert_eq!(
            f64::from_bits(parsed.magnitudes[POWERUP_ARMOR].bits()),
            0.025
        );

        // A trailing `e` with no digits ends the prefix before it.
        let parsed = table("[Powerups]\nGas=1,<none>,yes,7e\n");
        assert_eq!(f64::from_bits(parsed.magnitudes[16].bits()), 7.0);
    }

    /// `strtrim` strips bytes <= 0x20, which is not `str::trim`'s Unicode set.
    #[test]
    fn tokens_are_trimmed_with_the_native_ascii_rule() {
        let parsed = table("[Powerups]\nMoney=\u{1}20\u{1},\u{b}MONEY\u{b},yes,3\n");
        assert_eq!(parsed.weights[POWERUP_MONEY], 20);
        assert_eq!(parsed.animation_index(POWERUP_MONEY), 0);
        assert_eq!(f64::from_bits(parsed.magnitudes[POWERUP_MONEY].bits()), 3.0);
    }

    /// Only the literal `<none>` is Find_Index's sentinel; a bare `NONE` is an
    /// ordinary name that can resolve, including the absent row's default.
    #[test]
    fn only_angle_bracket_none_is_the_anim_sentinel() {
        let parsed = table("[Powerups]\nMoney=1,<NONE>,yes\nArmor=1,none,yes\n");
        assert_eq!(parsed.animation_index(POWERUP_MONEY), -1);
        assert_eq!(parsed.animation_index(POWERUP_ARMOR), -1);
        let mut parsed = PowerupTable::default();
        parsed.apply_pass(
            &IniFile::from_str("[Powerups]\nMoney=1,<NONE>,yes\nArmor=1,none,yes\n"),
            |name| {
                assert!(!name.eq_ignore_ascii_case("<none>"));
                name.eq_ignore_ascii_case("none").then_some(5)
            },
        );
        assert_eq!(parsed.animation_index(POWERUP_MONEY), -1);
        assert_eq!(parsed.animation_index(POWERUP_ARMOR), 5);
        assert_eq!(
            parsed.animation_index(POWERUP_UNIT),
            5,
            "an absent row looks up its literal NONE default"
        );
    }
}
