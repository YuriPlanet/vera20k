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
//! | 4 | `0x0089EC28` | `f64[19]` | `atof`, scaled by `0.01` when the token contains `%` |
//!
//! A missing token leaves that slot's previous value untouched — so the stock
//! rows that stop after three tokens (`HealBase`, `Reveal`, `Veteran`, `Unit`,
//! …) never write a magnitude, and a token-three value that is neither literal
//! leaves the flag alone. Because the default string carries only two tokens, a
//! `[Powerups]` section that omits a name still zeroes its weight and leaves it
//! with no animation — via the default's bare `NONE`, which is not
//! `Find_Index`'s `<none>` sentinel and reaches -1 only by failing the type
//! search, exactly as [`PowerupTable::anim_for`] does.
//!
//! ## Dependency rules
//! Part of `rules/` — INI parsing and rule data only.

use crate::rules::ini_parser::IniFile;
use crate::rules::ini_value::{strtrim_ascii, truncate_bytes};
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
/// Two tokens only: an absent row zeroes the weight and leaves the slot with no
/// resolvable animation, while the flag and magnitude keep their live values.
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
/// Native keeps these outside `RulesClass`; VERA keeps them beside the other
/// crate rule data because the ownership distinction has no player-visible
/// consequence, while the fixed slot order does.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PowerupTable {
    /// Token 1 — selection weight. `CellClass__PickupCrate` sums all
    /// nineteen and draws `RandomRanged(1, total)`.
    pub weights: [i32; POWERUP_COUNT],
    /// Token 2 — the pickup animation. `None` is native's `-1`: either the
    /// no-type sentinel or a name `AnimTypeClass::Find_Index` cannot resolve.
    pub anims: [Option<String>; POWERUP_COUNT],
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
            anims: [const { None }; POWERUP_COUNT],
            // `0x0089ECC0` and `0x0089EC28` live in zero-initialized storage.
            over_water: [false; POWERUP_COUNT],
            magnitudes: [NativeF64Bits::from_bits(0); POWERUP_COUNT],
        }
    }
}

impl PowerupTable {
    /// Resolve a slot's animation against the registered `[AnimTypes]` names.
    ///
    /// Native resolves at parse time through `AnimTypeClass::Find_Index`, so an
    /// unregistered name is stored as `-1` and can never spawn. VERA keeps the
    /// parsed name and applies the same filter here.
    ///
    /// These agree for the final rules state, which is the only state gameplay
    /// reads. They differ for one intermediate case: if a later INI layer
    /// registers an AnimType that an earlier layer's `[Powerups]` row already
    /// named, native has already stored `-1` and keeps it, while VERA resolves
    /// the name against the finished set. That needs a multi-layer mod to
    /// reach; recorded as a deferred DRIFT rather than claimed equivalent.
    #[cfg(test)]
    pub fn anim_for(&self, slot: usize, registered: &[String]) -> Option<&str> {
        let name = self.anims.get(slot)?.as_deref()?;
        registered
            .iter()
            .any(|candidate| candidate.eq_ignore_ascii_case(name))
            .then_some(name)
    }
}

/// Live table retained across successive `RulesClass::Process` passes.
#[derive(Debug, Default)]
pub(crate) struct PowerupsAccumulator(PowerupTable);

impl PowerupsAccumulator {
    pub(crate) fn apply_pass(&mut self, ini: &IniFile) {
        // `INIClass__FindSectionByName` gates the whole read: with no
        // `[Powerups]` section the function returns before touching a slot.
        let Some(section) = ini.section("Powerups") else {
            return;
        };
        for (slot, name) in POWERUP_NAMES.iter().enumerate() {
            // Native always parses, falling back to the literal `"0,NONE"`, so
            // an absent row still zeroes the weight and clears the animation
            // while leaving the flag and magnitude untouched.
            // `CCINIClass__ReadString` is `strncpy` plus a forced terminator, so
            // an over-long value is cut at 127 BYTES, not 127 characters.
            let raw = truncate_bytes(
                section.get(name).unwrap_or(DEFAULT_ROW),
                READ_STRING_CAPACITY - 1,
            );
            // `CRT__strtok` collapses runs of the delimiter and skips leading
            // ones, so an empty field is not a token at all: `20,,yes,2000`
            // yields three tokens, not four.
            let mut tokens = raw.split(',').filter(|token| !token.is_empty());

            if let Some(token) = tokens.next() {
                self.0.weights[slot] = native_atoi(strtrim_ascii(token));
            }
            if let Some(token) = tokens.next() {
                let token = strtrim_ascii(token);
                // `AnimTypeClass::Find_Index @ 0x00422B20` special-cases only
                // the literal `<none>`; every other name falls through to the
                // array search and yields -1 solely by not matching, which
                // `anim_for` reproduces against the registered set.
                self.0.anims[slot] =
                    (!token.eq_ignore_ascii_case("<none>")).then(|| token.to_ascii_uppercase());
            }
            if let Some(token) = tokens.next() {
                // Exactly two literals are recognised (`0x00825BF8` "yes" and
                // `0x00825BF4` "no"); anything else leaves the slot as it was.
                let token = strtrim_ascii(token);
                if token.eq_ignore_ascii_case("yes") {
                    self.0.over_water[slot] = true;
                } else if token.eq_ignore_ascii_case("no") {
                    self.0.over_water[slot] = false;
                }
            }
            if let Some(token) = tokens.next() {
                // The percent branch skips `strtrim` and scales by 0.01; the
                // plain branch trims first. Both then run the same `atof`.
                let value = if token.contains('%') {
                    native_atof(token) * 0.01
                } else {
                    native_atof(strtrim_ascii(token))
                };
                self.0.magnitudes[slot] = NativeF64Bits::from_bits(value.to_bits());
            }
        }
    }

    pub(crate) fn finish(self) -> PowerupTable {
        self.0
    }
}

/// CRT `atoi`: leading sign and digits only, everything from the first
/// non-digit onward ignored, no failure mode.
fn native_atoi(token: &str) -> i32 {
    let bytes = token.as_bytes();
    let mut index = 0;
    let negative = match bytes.first() {
        Some(b'-') => {
            index = 1;
            true
        }
        Some(b'+') => {
            index = 1;
            false
        }
        _ => false,
    };
    let mut value: i32 = 0;
    while let Some(digit) = bytes
        .get(index)
        .and_then(|byte| (*byte as char).to_digit(10))
    {
        value = value.wrapping_mul(10).wrapping_add(digit as i32);
        index += 1;
    }
    if negative {
        value.wrapping_neg()
    } else {
        value
    }
}

/// CRT `atof`: the longest leading floating-point prefix, or zero.
fn native_atof(token: &str) -> f64 {
    let token = token.trim_start();
    let bytes = token.as_bytes();
    let mut end = 0;
    if matches!(bytes.first(), Some(b'+' | b'-')) {
        end = 1;
    }
    while matches!(bytes.get(end), Some(byte) if byte.is_ascii_digit()) {
        end += 1;
    }
    if matches!(bytes.get(end), Some(b'.')) {
        end += 1;
        while matches!(bytes.get(end), Some(byte) if byte.is_ascii_digit()) {
            end += 1;
        }
    }
    // CRT `atof` also accepts an exponent, but only when at least one digit
    // follows it; otherwise the prefix ends before the `e`.
    if matches!(bytes.get(end), Some(b'e' | b'E')) {
        let mut exponent = end + 1;
        if matches!(bytes.get(exponent), Some(b'+' | b'-')) {
            exponent += 1;
        }
        if matches!(bytes.get(exponent), Some(byte) if byte.is_ascii_digit()) {
            while matches!(bytes.get(exponent), Some(byte) if byte.is_ascii_digit()) {
                exponent += 1;
            }
            end = exponent;
        }
    }
    token[..end].parse::<f64>().unwrap_or(0.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn table(section: &str) -> PowerupTable {
        let mut accumulator = PowerupsAccumulator::default();
        accumulator.apply_pass(&IniFile::from_str(section));
        accumulator.finish()
    }

    /// The nineteen slots and their order come from the binary's pointer table,
    /// not from the INI. Pin the exact sequence and the indices the crate system
    /// names directly.
    #[test]
    fn powerup_slot_order_is_the_fixed_native_name_table() {
        assert_eq!(POWERUP_NAMES.len(), 19);
        assert_eq!(POWERUP_NAMES[POWERUP_MONEY], "Money");
        assert_eq!(POWERUP_NAMES[POWERUP_UNIT], "Unit");
        assert_eq!(POWERUP_NAMES[POWERUP_HEAL_BASE], "HealBase");
        assert_eq!(POWERUP_NAMES[POWERUP_SQUAD], "Squad");
        assert_eq!(POWERUP_NAMES[POWERUP_REVEAL], "Reveal");
        assert_eq!(POWERUP_NAMES[POWERUP_ARMOR], "Armor");
        assert_eq!(POWERUP_NAMES[POWERUP_SPEED], "Speed");
        assert_eq!(POWERUP_NAMES[POWERUP_FIREPOWER], "Firepower");
        assert_eq!(POWERUP_NAMES[POWERUP_VETERAN], "Veteran");
        assert_eq!(POWERUP_NAMES[15], "IonStorm");
        assert_eq!(POWERUP_NAMES[17], "Tiberium");
        assert_eq!(POWERUP_NAMES[18], "Pod");
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
        assert!(parsed.anims.iter().all(Option::is_none));
        assert!(parsed.over_water.iter().all(|flag| !flag));
    }

    /// A present section but an absent row still applies the `"0,NONE"`
    /// default's first two tokens, and leaves the last two alone.
    #[test]
    fn absent_row_zeroes_weight_and_anim_but_keeps_flag_and_magnitude() {
        let mut accumulator = PowerupsAccumulator::default();
        accumulator.apply_pass(&IniFile::from_str("[Powerups]\nMoney=20,MONEY,yes,2000\n"));
        // Second pass: the section exists but Money is gone.
        accumulator.apply_pass(&IniFile::from_str("[Powerups]\nArmor=10,ARMOR,yes,1.5\n"));
        let parsed = accumulator.finish();
        assert_eq!(
            parsed.weights[POWERUP_MONEY], 0,
            "the default's first token"
        );
        // The default row's second token is a bare `NONE`, which is NOT
        // Find_Index's `<none>` sentinel: it falls through the array search and
        // yields -1 only by failing to match. `anim_for` is the resolution
        // point, so that is where the absence becomes observable.
        assert_eq!(parsed.anims[POWERUP_MONEY].as_deref(), Some("NONE"));
        assert_eq!(
            parsed.anim_for(POWERUP_MONEY, &["MONEY".to_owned(), "ARMOR".to_owned()]),
            None,
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

        // `<none>` is the no-type sentinel; a real name is retained uppercased.
        assert_eq!(parsed.anims[POWERUP_UNIT], None);
        assert_eq!(parsed.anims[POWERUP_MONEY].as_deref(), Some("MONEY"));
        assert_eq!(parsed.anims[POWERUP_FIREPOWER].as_deref(), Some("FIREPOWR"));

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
        assert_eq!(parsed.anims[POWERUP_ARMOR].as_deref(), Some("ARMOR"));
        assert!(parsed.over_water[POWERUP_ARMOR]);
    }

    /// Token three recognises exactly two literals; anything else is inert.
    #[test]
    fn unrecognised_third_token_leaves_the_live_flag_untouched() {
        let mut accumulator = PowerupsAccumulator::default();
        accumulator.apply_pass(&IniFile::from_str("[Powerups]\nArmor=1,ARMOR,yes,1\n"));
        accumulator.apply_pass(&IniFile::from_str("[Powerups]\nArmor=1,ARMOR,true,1\n"));
        assert!(
            accumulator.finish().over_water[POWERUP_ARMOR],
            "`true` is neither literal, so the `yes` from the earlier pass stands"
        );

        let mut accumulator = PowerupsAccumulator::default();
        accumulator.apply_pass(&IniFile::from_str("[Powerups]\nArmor=1,ARMOR,yes,1\n"));
        accumulator.apply_pass(&IniFile::from_str("[Powerups]\nArmor=1,ARMOR,no,1\n"));
        assert!(!accumulator.finish().over_water[POWERUP_ARMOR]);
    }

    /// An animation name that no `[AnimTypes]` entry registers behaves exactly
    /// like native's `Find_Index` returning -1.
    #[test]
    fn unregistered_anim_resolves_to_none() {
        let parsed = table("[Powerups]\nMoney=20,BOGUSANIM,yes,2000\n");
        let registered = vec!["MONEY".to_owned(), "ARMOR".to_owned()];
        assert_eq!(parsed.anim_for(POWERUP_MONEY, &registered), None);

        let parsed = table("[Powerups]\nMoney=20,money,yes,2000\n");
        assert_eq!(
            parsed.anim_for(POWERUP_MONEY, &registered),
            Some("MONEY"),
            "resolution is case-insensitive, like Find_Index"
        );
    }

    /// `strtok` skips empty fields, so a doubled or leading comma shifts which
    /// token feeds which slot.
    #[test]
    fn empty_fields_are_not_tokens() {
        let parsed = table("[Powerups]\nMoney=20,,yes,2000\n");
        assert_eq!(parsed.weights[POWERUP_MONEY], 20);
        assert_eq!(
            parsed.anims[POWERUP_MONEY].as_deref(),
            Some("YES"),
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
        assert_eq!(parsed.anims[POWERUP_MONEY].as_deref(), Some("MONEY"));
        assert_eq!(f64::from_bits(parsed.magnitudes[POWERUP_MONEY].bits()), 3.0);
    }

    /// Retail lines all carry a trailing `;` comment; the loader strips it
    /// before the table ever sees the value.
    #[test]
    fn trailing_comments_are_stripped_before_tokenizing() {
        let parsed = table(
            "[Powerups]\nMoney=20,MONEY,yes,2000             ; a chunk o' cash (maximum cash)\n",
        );
        assert_eq!(parsed.weights[POWERUP_MONEY], 20);
        assert_eq!(parsed.anims[POWERUP_MONEY].as_deref(), Some("MONEY"));
        assert!(parsed.over_water[POWERUP_MONEY]);
        assert_eq!(
            f64::from_bits(parsed.magnitudes[POWERUP_MONEY].bits()),
            2000.0
        );
    }

    /// Only the literal `<none>` is Find_Index's sentinel; a bare `NONE` is an
    /// ordinary name that simply fails to resolve.
    #[test]
    fn only_angle_bracket_none_is_the_anim_sentinel() {
        let parsed = table("[Powerups]\nMoney=1,<NONE>,yes\nArmor=1,none,yes\n");
        assert_eq!(parsed.anims[POWERUP_MONEY], None);
        assert_eq!(parsed.anims[POWERUP_ARMOR].as_deref(), Some("NONE"));
        assert_eq!(
            parsed.anim_for(POWERUP_ARMOR, &["MONEY".to_owned()]),
            None,
            "an unregistered name still resolves to no animation"
        );
    }

    /// `atoi` stops at the first non-digit rather than failing.
    #[test]
    fn native_atoi_matches_crt_prefix_semantics() {
        assert_eq!(native_atoi("20"), 20);
        assert_eq!(native_atoi("-7"), -7);
        assert_eq!(native_atoi("+3"), 3);
        assert_eq!(native_atoi("12abc"), 12);
        assert_eq!(native_atoi("abc"), 0);
        assert_eq!(native_atoi(""), 0);
    }
}
