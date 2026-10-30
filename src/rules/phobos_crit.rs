//! Phobos warhead critical hits, retained by the Rules registry.
//!
//! Native identity: Phobos `WarheadTypeExt`'s `Crit_*` block
//! (`src/Ext/WarheadType/Body.h:64..79`, `LoadFromINI` order in `Body.cpp:194..232`),
//! `GetCritChance`/`ApplyCrit` (`Detonate.cpp:525..608`, `735..778`) and the
//! `[CombatDamage]`/`[General]` defaults (`Ext/Rules/Body.{h,cpp}`). The
//! runtime owner is [`crate::sim::combat::crit`].
//!
//! Read but not consumed by this chain: the `RELOAD`, `EMPTY_RELOAD` and
//! `RANGE` Phobos rank abilities. `ReadAdditionalAbilities` parses the whole
//! `VeteranAbilities=`/`EliteAbilities=` list per present key, so the three
//! are retained alongside the two crit gates even though their own consumers
//! (weapon rearm, range fold) are separate chains.

use serde::{Deserialize, Serialize};

/// Phobos `AffectedTarget` flags (`src/Utilities/Enum.h:96..113`), the target
/// of `Valueable<AffectedTarget>::Read` (`Utilities/TemplateDef.h:778..818`):
/// the value starts at `None` and ORs one flag per comma-separated token; an
/// unknown token fails the whole read, leaving the caller's value.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct AffectedTarget(u8);

impl AffectedTarget {
    pub const NONE: Self = Self(0x0);
    pub const LAND: Self = Self(0x1);
    pub const WATER: Self = Self(0x2);
    pub const NO_CONTENT: Self = Self(0x4);
    pub const INFANTRY: Self = Self(0x8);
    pub const UNIT: Self = Self(0x10);
    pub const BUILDING: Self = Self(0x20);
    pub const AIRCRAFT: Self = Self(0x40);
    pub const ALL: Self = Self(0xFF);
    pub const ALL_CELLS: Self = Self(Self::LAND.0 | Self::WATER.0);
    pub const ALL_TECHNOS: Self =
        Self(Self::INFANTRY.0 | Self::UNIT.0 | Self::BUILDING.0 | Self::AIRCRAFT.0);
    pub const ALL_CONTENTS: Self = Self(Self::NO_CONTENT.0 | Self::ALL_TECHNOS.0);

    pub fn bits(self) -> u8 {
        self.0
    }

    pub fn contains(self, other: Self) -> bool {
        self.0 & other.0 != 0
    }

    /// `read<AffectedTarget>`: one token that matches no entry abandons the
    /// read; `None` keeps the caller's current value.
    #[cfg(feature = "phobos")]
    pub(crate) fn parse(raw: &str) -> Option<Self> {
        let mut parsed = Self::NONE;
        for part in raw.split(',') {
            let token = part.trim_matches(|c| matches!(c, ' ' | '\t' | '\r'));
            let flag = match token.to_ascii_lowercase().as_str() {
                "land" => Self::LAND,
                "water" => Self::WATER,
                "empty" => Self::NO_CONTENT,
                "infantry" => Self::INFANTRY,
                "units" => Self::UNIT,
                "buildings" => Self::BUILDING,
                "aircraft" => Self::AIRCRAFT,
                "all" => Self::ALL,
                "none" => Self::NONE,
                _ => return None,
            };
            parsed = Self(parsed.0 | flag.0);
        }
        Some(parsed)
    }
}

/// `WarheadTypeExt`'s constructor (`AffectedTarget::All`).
impl Default for AffectedTarget {
    fn default() -> Self {
        Self::ALL
    }
}

/// The five Phobos rank abilities (`TechnoType/Body.cpp:17..23`), parallel to
/// the stock `AbilityFlags` read of the same two keys.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct PhobosAbilityFlags {
    veteran: [bool; 5],
    elite: [bool; 5],
}

impl PhobosAbilityFlags {
    pub const RELOAD: usize = 0;
    pub const EMPTY_RELOAD: usize = 1;
    pub const RANGE: usize = 2;
    pub const CRIT_IMMUNE: usize = 3;
    pub const CRIT_CHANCE: usize = 4;

    /// `ReadAdditionalAbilities`: a present key resets its tier and sets one
    /// bit per recognised token; an absent key keeps the tier.
    #[cfg(feature = "phobos")]
    pub(crate) fn read_pass(&mut self, section: &crate::rules::ini_parser::IniSection) {
        Self::read_tier(section, "VeteranAbilities", &mut self.veteran);
        Self::read_tier(section, "EliteAbilities", &mut self.elite);
    }

    #[cfg(feature = "phobos")]
    fn read_tier(section: &crate::rules::ini_parser::IniSection, key: &str, tier: &mut [bool; 5]) {
        let Some(tokens) = section.read_list(key, 2048) else {
            return;
        };
        *tier = [false; 5];
        for token in tokens {
            let slot = match token.trim().to_ascii_uppercase().as_str() {
                "RELOAD" => Self::RELOAD,
                "EMPTY_RELOAD" => Self::EMPTY_RELOAD,
                "RANGE" => Self::RANGE,
                "CRITIMMUNE" => Self::CRIT_IMMUNE,
                "CRITCHANCE" => Self::CRIT_CHANCE,
                _ => continue,
            };
            tier[slot] = true;
        }
    }

    /// `TechnoExt::HasAdditionalAbility`: a veteran reads the veteran tier, an
    /// elite either tier. The caller answers the rookie case.
    pub fn has(&self, slot: usize, elite: bool) -> bool {
        if elite {
            self.veteran[slot] || self.elite[slot]
        } else {
            self.veteran[slot]
        }
    }
}

/// `[CombatDamage]`/`[General]` defaults for the crit block.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct CritGlobals {
    /// `[CombatDamage] Crit.ApplyChancePerTarget`, default false.
    pub(crate) apply_chance_per_target: bool,
    /// `[CombatDamage] Crit.ExtraDamage.ApplyFirepowerMult`, default false.
    pub(crate) extra_damage_apply_firepower_mult: bool,
    /// `[CombatDamage] Crit.AnimOnAffectedTargets`, default false.
    pub(crate) anim_on_affected_targets: bool,
    /// `[CombatDamage] Crit.SuppressWhenIntercepted`, default false.
    pub(crate) suppress_when_intercepted: bool,
    /// `[CombatDamage] ApplyPerTargetEffectsOnDetonate`, default true. When
    /// false, native moves the per-target pass to ReceiveDamage and rolls a
    /// fresh chance for every target.
    pub(crate) apply_per_target_effects_on_detonate: bool,
    /// `[CombatDamage] ReturnWarhead.ApplyChancePerTarget`, default false.
    /// Only its effect on `Detonate`'s per-detonation buffer draw is ported;
    /// `ReturnWarhead` itself has no VERA owner.
    pub(crate) return_warhead_apply_chance_per_target: bool,
    /// `[General] VeteranCritChance`, default 1.0.
    pub(crate) veteran_crit_chance: f64,
}

impl Default for CritGlobals {
    fn default() -> Self {
        Self {
            apply_chance_per_target: false,
            extra_damage_apply_firepower_mult: false,
            anim_on_affected_targets: false,
            suppress_when_intercepted: false,
            apply_per_target_effects_on_detonate: true,
            return_warhead_apply_chance_per_target: false,
            veteran_crit_chance: 1.0,
        }
    }
}

impl CritGlobals {
    #[cfg(feature = "phobos")]
    pub(crate) fn from_ini(ini: &crate::rules::ini_parser::IniFile) -> Self {
        let defaults = Self::default();
        let combat = ini.section_or_empty("CombatDamage");
        let general = ini.section_or_empty("General");
        Self {
            apply_chance_per_target: combat.read_bool(
                "Crit.ApplyChancePerTarget",
                defaults.apply_chance_per_target,
            ),
            extra_damage_apply_firepower_mult: combat.read_bool(
                "Crit.ExtraDamage.ApplyFirepowerMult",
                defaults.extra_damage_apply_firepower_mult,
            ),
            anim_on_affected_targets: combat.read_bool(
                "Crit.AnimOnAffectedTargets",
                defaults.anim_on_affected_targets,
            ),
            suppress_when_intercepted: combat.read_bool(
                "Crit.SuppressWhenIntercepted",
                defaults.suppress_when_intercepted,
            ),
            apply_per_target_effects_on_detonate: combat.read_bool(
                "ApplyPerTargetEffectsOnDetonate",
                defaults.apply_per_target_effects_on_detonate,
            ),
            return_warhead_apply_chance_per_target: combat.read_bool(
                "ReturnWarhead.ApplyChancePerTarget",
                defaults.return_warhead_apply_chance_per_target,
            ),
            veteran_crit_chance: general
                .read_phobos_double("VeteranCritChance", defaults.veteran_crit_chance),
        }
    }

    pub fn apply_chance_per_target(&self) -> bool {
        self.apply_chance_per_target
    }

    pub fn extra_damage_apply_firepower_mult(&self) -> bool {
        self.extra_damage_apply_firepower_mult
    }

    pub fn anim_on_affected_targets(&self) -> bool {
        self.anim_on_affected_targets
    }

    pub fn suppress_when_intercepted(&self) -> bool {
        self.suppress_when_intercepted
    }

    pub fn apply_per_target_effects_on_detonate(&self) -> bool {
        self.apply_per_target_effects_on_detonate
    }

    pub fn veteran_crit_chance(&self) -> f64 {
        self.veteran_crit_chance
    }
}

/// `WarheadTypeExt`'s `Crit.*` block, `LoadFromINI` order.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CritRules {
    pub(crate) chance: f64,
    /// `Crit.ApplyChancePerTarget`; `None` inherits [`CritGlobals`].
    pub(crate) apply_chance_per_target: Option<bool>,
    pub(crate) extra_damage: i32,
    /// `Crit.ExtraDamage.ApplyFirepowerMult`; `None` inherits.
    pub(crate) extra_damage_apply_firepower_mult: Option<bool>,
    /// `Crit.Warhead` (`Read<true>`): the extra warhead's name.
    pub(crate) warhead: Option<String>,
    pub(crate) warhead_full_detonation: bool,
    pub(crate) affects_target: AffectedTarget,
    pub(crate) affects_house: crate::rules::phobos_shields::AffectedHouse,
    pub(crate) anim_list: Vec<String>,
    /// `Crit.AnimList.PickRandom`; `None` falls back to the warhead's
    /// `AnimList.PickRandom`.
    pub(crate) anim_list_pick_random: Option<bool>,
    pub(crate) anim_list_create_all: Option<bool>,
    pub(crate) active_chance_anims: Vec<String>,
    /// `Crit.AnimOnAffectedTargets`; `None` inherits.
    pub(crate) anim_on_affected_targets: Option<bool>,
    pub(crate) affects_below_percent: f64,
    pub(crate) affects_above_percent: f64,
    /// `Crit.SuppressWhenIntercepted`; `None` inherits.
    pub(crate) suppress_when_intercepted: Option<bool>,
    /// `ReturnWarhead.ApplyChancePerTarget` (`Body.cpp:245`); `None` inherits
    /// [`CritGlobals`]. Read for `Detonate`'s buffer draw only.
    pub(crate) return_warhead_apply_chance_per_target: Option<bool>,
}

impl Default for CritRules {
    fn default() -> Self {
        Self {
            chance: 0.0,
            apply_chance_per_target: None,
            extra_damage: 0,
            extra_damage_apply_firepower_mult: None,
            warhead: None,
            warhead_full_detonation: true,
            affects_target: AffectedTarget::ALL,
            affects_house: crate::rules::phobos_shields::AffectedHouse::ALL,
            anim_list: Vec::new(),
            anim_list_pick_random: None,
            anim_list_create_all: None,
            active_chance_anims: Vec::new(),
            anim_on_affected_targets: None,
            affects_below_percent: 1.0,
            affects_above_percent: 0.0,
            suppress_when_intercepted: None,
            return_warhead_apply_chance_per_target: None,
        }
    }
}

impl CritRules {
    #[cfg(feature = "phobos")]
    pub(crate) fn from_section(section: &crate::rules::ini_parser::IniSection) -> Self {
        use crate::rules::phobos_shields::AffectedHouse;
        let mut crit = Self::default();
        crit.chance = section.read_phobos_double("Crit.Chance", crit.chance);
        crit.apply_chance_per_target = section
            .read_phobos_nullable_bool("Crit.ApplyChancePerTarget", crit.apply_chance_per_target);
        crit.extra_damage = section.read_int("Crit.ExtraDamage", crit.extra_damage);
        crit.extra_damage_apply_firepower_mult = section.read_phobos_nullable_bool(
            "Crit.ExtraDamage.ApplyFirepowerMult",
            crit.extra_damage_apply_firepower_mult,
        );
        if let Some(name) = section.read_name("Crit.Warhead", 0x80) {
            crit.warhead = Some(name.to_owned());
        }
        crit.warhead_full_detonation =
            section.read_bool("Crit.Warhead.FullDetonation", crit.warhead_full_detonation);
        // Deprecated spellings run first; the current key overwrites them.
        for key in ["Crit.Affects", "Crit.AffectsTarget"] {
            if let Some(raw) = section.read_name(key, 0x80)
                && let Some(parsed) = AffectedTarget::parse(raw)
            {
                crit.affects_target = parsed;
            }
        }
        for key in ["Crit.AffectsHouses", "Crit.AffectsHouse"] {
            if let Some(raw) = section.read_name(key, 0x80)
                && let Some(parsed) = AffectedHouse::parse(raw)
            {
                crit.affects_house = parsed;
            }
        }
        if let Some(tokens) = section.read_list("Crit.AnimList", 0x80) {
            crit.anim_list = tokens.into_iter().map(str::to_owned).collect();
        }
        crit.anim_list_pick_random = section
            .read_phobos_nullable_bool("Crit.AnimList.PickRandom", crit.anim_list_pick_random);
        crit.anim_list_create_all =
            section.read_phobos_nullable_bool("Crit.AnimList.CreateAll", crit.anim_list_create_all);
        if let Some(tokens) = section.read_list("Crit.ActiveChanceAnims", 0x80) {
            crit.active_chance_anims = tokens.into_iter().map(str::to_owned).collect();
        }
        crit.anim_on_affected_targets = section
            .read_phobos_nullable_bool("Crit.AnimOnAffectedTargets", crit.anim_on_affected_targets);
        for key in ["Crit.AffectBelowPercent", "Crit.AffectsBelowPercent"] {
            crit.affects_below_percent =
                section.read_phobos_double(key, crit.affects_below_percent);
        }
        for key in ["Crit.AffectAbovePercent", "Crit.AffectsAbovePercent"] {
            crit.affects_above_percent =
                section.read_phobos_double(key, crit.affects_above_percent);
        }
        crit.suppress_when_intercepted = section.read_phobos_nullable_bool(
            "Crit.SuppressWhenIntercepted",
            crit.suppress_when_intercepted,
        );
        crit.return_warhead_apply_chance_per_target = section.read_phobos_nullable_bool(
            "ReturnWarhead.ApplyChancePerTarget",
            crit.return_warhead_apply_chance_per_target,
        );
        crit
    }

    pub fn chance(&self) -> f64 {
        self.chance
    }

    /// `CanCrit`, set at the end of the crit reads (`Body.cpp:235..239`):
    /// extra damage, an extra warhead or either animation list. `Crit.Chance`
    /// is not part of it, so an AttachEffect `Crit.ExtraChance` can still
    /// crit a warhead that leaves the chance at zero.
    pub fn can_crit(&self) -> bool {
        self.extra_damage > 0
            || self.warhead.is_some()
            || !self.active_chance_anims.is_empty()
            || !self.anim_list.is_empty()
    }

    pub fn apply_chance_per_target(&self, globals: &CritGlobals) -> bool {
        self.apply_chance_per_target
            .unwrap_or(globals.apply_chance_per_target)
    }

    pub fn extra_damage(&self) -> i32 {
        self.extra_damage
    }

    pub fn extra_damage_apply_firepower_mult(&self, globals: &CritGlobals) -> bool {
        self.extra_damage_apply_firepower_mult
            .unwrap_or(globals.extra_damage_apply_firepower_mult)
    }

    pub fn warhead(&self) -> Option<&str> {
        self.warhead.as_deref()
    }

    pub fn warhead_full_detonation(&self) -> bool {
        self.warhead_full_detonation
    }

    pub fn affects_target(&self) -> AffectedTarget {
        self.affects_target
    }

    pub fn affects_house(&self) -> crate::rules::phobos_shields::AffectedHouse {
        self.affects_house
    }

    pub fn anim_list(&self) -> &[String] {
        &self.anim_list
    }

    pub fn anim_list_pick_random(&self) -> bool {
        self.anim_list_pick_random.unwrap_or(false)
    }

    pub fn anim_list_create_all(&self) -> bool {
        self.anim_list_create_all.unwrap_or(false)
    }

    pub fn active_chance_anims(&self) -> &[String] {
        &self.active_chance_anims
    }

    pub fn anim_on_affected_targets(&self, globals: &CritGlobals) -> bool {
        self.anim_on_affected_targets
            .unwrap_or(globals.anim_on_affected_targets)
    }

    pub fn affects_below_percent(&self) -> f64 {
        self.affects_below_percent
    }

    pub fn affects_above_percent(&self) -> f64 {
        self.affects_above_percent
    }

    pub fn suppress_when_intercepted(&self, globals: &CritGlobals) -> bool {
        self.suppress_when_intercepted
            .unwrap_or(globals.suppress_when_intercepted)
    }

    pub fn return_warhead_apply_chance_per_target(&self, globals: &CritGlobals) -> bool {
        self.return_warhead_apply_chance_per_target
            .unwrap_or(globals.return_warhead_apply_chance_per_target)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rules::ini_parser::IniFile;

    /// Constructor defaults: no chance, no extra warhead, full detonation, all
    /// targets and houses, `(0.0, 1.0]`.
    #[test]
    fn crit_defaults_match_the_constructor() {
        let rules = CritRules::default();
        assert_eq!(rules.chance(), 0.0);
        assert!(!rules.can_crit());
        assert_eq!(rules.extra_damage(), 0);
        assert!(rules.warhead().is_none());
        assert!(rules.warhead_full_detonation());
        assert_eq!(rules.affects_target(), AffectedTarget::ALL);
        assert_eq!(
            rules.affects_house(),
            crate::rules::phobos_shields::AffectedHouse::ALL
        );
        assert!(rules.anim_list().is_empty());
        assert!(!rules.anim_list_pick_random());
        assert!(!rules.anim_list_create_all());
        assert!(rules.active_chance_anims().is_empty());
        assert_eq!(rules.affects_above_percent(), 0.0);
        assert_eq!(rules.affects_below_percent(), 1.0);
        let globals = CritGlobals::default();
        assert!(!globals.apply_chance_per_target());
        assert!(!globals.extra_damage_apply_firepower_mult());
        assert!(!globals.anim_on_affected_targets());
        assert!(!globals.suppress_when_intercepted());
        assert_eq!(globals.veteran_crit_chance(), 1.0);
    }

    /// `LoadFromINI` order: the deprecated spellings run before their current
    /// keys, so the current key wins; nullable switches inherit the globals.
    #[test]
    fn crit_keys_read_in_native_order() {
        let ini = IniFile::from_str(
            "[WH]\n\
             Crit.Chance=25%\n\
             Crit.ApplyChancePerTarget=yes\n\
             Crit.ExtraDamage=40\n\
             Crit.ExtraDamage.ApplyFirepowerMult=yes\n\
             Crit.Warhead=MINI\n\
             Crit.Warhead.FullDetonation=no\n\
             Crit.Affects=water\nCrit.AffectsTarget=infantry,units\n\
             Crit.AffectsHouses=enemies\nCrit.AffectsHouse=owner,allies\n\
             Crit.AnimList=A,B\nCrit.AnimList.PickRandom=yes\nCrit.AnimList.CreateAll=yes\n\
             Crit.ActiveChanceAnims=C\n\
             Crit.AnimOnAffectedTargets=yes\n\
             Crit.AffectBelowPercent=0.75\nCrit.AffectsAbovePercent=0.25\n\
             Crit.SuppressWhenIntercepted=yes\n",
        );
        let section = ini.section("WH").expect("section");
        let crit = CritRules::from_section(section);
        assert_eq!(crit.chance(), 0.25);
        assert_eq!(crit.apply_chance_per_target(&CritGlobals::default()), true);
        assert_eq!(crit.extra_damage(), 40);
        assert!(crit.extra_damage_apply_firepower_mult(&CritGlobals::default()));
        assert_eq!(crit.warhead(), Some("MINI"));
        assert!(!crit.warhead_full_detonation());
        assert_eq!(
            crit.affects_target(),
            AffectedTarget::parse("infantry,units").unwrap()
        );
        assert_eq!(
            crit.affects_house(),
            crate::rules::phobos_shields::AffectedHouse::parse("owner,allies").unwrap()
        );
        assert_eq!(crit.anim_list(), ["A", "B"]);
        assert!(crit.anim_list_pick_random());
        assert!(crit.anim_list_create_all());
        assert_eq!(crit.active_chance_anims(), ["C"]);
        assert!(crit.anim_on_affected_targets(&CritGlobals::default()));
        assert_eq!(crit.affects_below_percent(), 0.75);
        assert_eq!(crit.affects_above_percent(), 0.25);
        assert!(crit.suppress_when_intercepted(&CritGlobals::default()));

        let globals = CritGlobals::from_ini(&IniFile::from_str(
            "[CombatDamage]\nCrit.ApplyChancePerTarget=yes\n\
             Crit.ExtraDamage.ApplyFirepowerMult=yes\n\
             Crit.AnimOnAffectedTargets=yes\nCrit.SuppressWhenIntercepted=yes\n\
             [General]\nVeteranCritChance=0.5\n",
        ));
        assert!(globals.apply_chance_per_target());
        assert!(globals.extra_damage_apply_firepower_mult());
        assert!(globals.anim_on_affected_targets());
        assert!(globals.suppress_when_intercepted());
        assert_eq!(globals.veteran_crit_chance(), 0.5);
    }

    /// `read<AffectedTarget>` fails the whole read on one unknown token, so a
    /// later pass's typo keeps the earlier value instead of clearing it.
    #[test]
    fn affected_target_tokens_are_strict_and_case_insensitive() {
        assert_eq!(
            AffectedTarget::parse("Land, WATER ,empty"),
            Some(AffectedTarget::parse("land,water,empty").unwrap())
        );
        assert_eq!(AffectedTarget::parse("all"), Some(AffectedTarget::ALL));
        assert_eq!(AffectedTarget::parse("none"), Some(AffectedTarget::NONE));
        assert_eq!(AffectedTarget::parse("units,typo"), None);
    }
}
