//! Optional Ares Prism forwarding keys, BuildingTypeExt `cPrismForwarding`
//! (`+0xE0..+0x123`). `Initialize @ 100105A0` seeds them at the type's first
//! LoadFromINI; `LoadFromINIFile @ 10010680`, the first read of the
//! BuildingTypeExt loader `1000DD40`, reads them on every reached pass. The
//! support weapons' slot writes (`100103F0`) need the type registry and live
//! in `native_processing`. `sim::world::techno_ai::ares_prism_forwarding`
//! builds and fires the networks.

use crate::rules::ini_parser::IniSection;

/// `PrismForwarding=` (`+0xE0`): `yes` towers fire and support, `forward`
/// ones only support, `attack` ones only fire.
#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize,
)]
pub(crate) enum PrismForwardingMode {
    #[default]
    No,
    Yes,
    Forward,
    Attack,
}

/// The retained `cPrismForwarding` of one BuildingType. The constructor
/// leaves it disabled with an empty target list, `MaxChainLength` 1,
/// `Intensity` -2, `ChargeDelay` 1, both support weapon slots -1 and the
/// Nullable limits and modifier unset.
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub(crate) struct PrismForwardingRules {
    /// Initialize ran (it seeds once, at the type's first reached read).
    initialized: bool,
    mode: PrismForwardingMode,
    /// `PrismForwarding.Targets` (`+0xE4`, Find-only
    /// `ValueableVector<BuildingTypeClass*>` `10010850`): the types this
    /// tower may support, Initialize's own type until a pass reads the key.
    targets: Vec<String>,
    /// `PrismForwarding.MaxFeeds` (`+0xF0`, Nullable): how many towers may
    /// feed this one, else `[General] PrismSupportMax=`. Negative: no limit.
    max_feeds: Option<i32>,
    /// `PrismForwarding.MaxChainLength` (`+0xF8`): the longest chain behind a
    /// firing tower of this type. Negative: no limit.
    max_chain_length: i32,
    /// `PrismForwarding.MaxNetworkSize` (`+0xFC`, Nullable): how many towers
    /// a firing tower of this type may recruit, else `PrismSupportMax=`.
    max_network_size: Option<i32>,
    /// `PrismForwarding.SupportModifier` (`+0x104`, Nullable): the percent
    /// this tower's support adds, else `PrismSupportModifier=`.
    support_modifier: Option<i32>,
    /// `PrismForwarding.DamageAdd` (`+0x10C`): flat damage this tower's
    /// support (or shot) adds.
    damage_add: i32,
    /// `PrismForwarding.Intensity` (`+0x110`): the supported beam's extra
    /// thickness, absolute when negative, per chain link when positive.
    intensity: i32,
    /// `PrismForwarding.ChargeDelay` (`+0x114`, at least 1): the frames a
    /// tower waits per tower behind it before it starts charging.
    charge_delay: i32,
    /// `PrismForwarding.ToAllies` (`+0x118`).
    to_allies: bool,
    /// `PrismForwarding.BreakSupport` (`+0x119`): a charging supporter may
    /// still fire at its own target.
    break_support: bool,
    /// The weapon slots `PrismForwarding.SupportWeapon` and
    /// `.EliteSupportWeapon` were written to (`+0x11C`, `+0x120`), -1 for none.
    support_weapon_index: i32,
    elite_support_weapon_index: i32,
}

impl Default for PrismForwardingRules {
    fn default() -> Self {
        Self {
            initialized: false,
            mode: PrismForwardingMode::No,
            targets: Vec::new(),
            max_feeds: None,
            max_chain_length: 1,
            max_network_size: None,
            support_modifier: None,
            damage_add: 0,
            intensity: -2,
            charge_delay: 1,
            to_allies: false,
            break_support: false,
            support_weapon_index: -1,
            elite_support_weapon_index: -1,
        }
    }
}

impl PrismForwardingRules {
    /// One reached pass. `type_id` is the type's stored ID, `prism_type` the
    /// current `[General] PrismType=` (Initialize compares the pointers) and
    /// `targets` the Find-only read of `PrismForwarding.Targets`. Answers
    /// whether the tower is enabled, which gates the support weapon reads
    /// that follow (`10010726`).
    pub(crate) fn read_pass(
        &mut self,
        section: &IniSection,
        type_id: &str,
        prism_type: Option<&str>,
        targets: Option<Vec<String>>,
    ) -> bool {
        let _ares = crate::rules::ares_switch::ares_reads();
        if !self.initialized {
            self.initialized = true;
            if prism_type == Some(type_id) {
                self.mode = PrismForwardingMode::Yes;
            }
            self.targets.push(type_id.to_owned());
        }
        // `forward` and `attack` (strcmpi) first, then the bool parser; an
        // unparsed value keeps the mode.
        self.mode = section.read_string_with("PrismForwarding", 2048, self.mode, |mode, value| {
            if value.eq_ignore_ascii_case("forward") {
                PrismForwardingMode::Forward
            } else if value.eq_ignore_ascii_case("attack") {
                PrismForwardingMode::Attack
            } else {
                match crate::rules::ini_value::read_bool_decision(value) {
                    Some(true) => PrismForwardingMode::Yes,
                    Some(false) => PrismForwardingMode::No,
                    None => mode,
                }
            }
        });
        if self.mode == PrismForwardingMode::No {
            return false;
        }
        // 3.0 no longer reads PrismForwarding.MyHeight.
        if let Some(targets) = targets {
            self.targets = targets;
        }
        self.max_feeds = section.read_ares_nullable_int("PrismForwarding.MaxFeeds", self.max_feeds);
        self.max_chain_length =
            section.read_ares_int("PrismForwarding.MaxChainLength", self.max_chain_length);
        self.max_network_size =
            section.read_ares_nullable_int("PrismForwarding.MaxNetworkSize", self.max_network_size);
        self.support_modifier = section
            .read_ares_nullable_int("PrismForwarding.SupportModifier", self.support_modifier);
        self.damage_add = section.read_ares_int("PrismForwarding.DamageAdd", self.damage_add);
        self.to_allies = section.read_ares_bool("PrismForwarding.ToAllies", self.to_allies);
        self.break_support =
            section.read_ares_bool("PrismForwarding.BreakSupport", self.break_support);
        self.intensity = section.read_ares_int("PrismForwarding.Intensity", self.intensity);
        self.charge_delay = section.read_ares_int("PrismForwarding.ChargeDelay", self.charge_delay);
        if self.charge_delay < 1 {
            log::warn!(
                "{type_id} has an invalid PrismForwarding.ChargeDelay ({}), overriding to 1",
                self.charge_delay
            );
            self.charge_delay = 1;
        }
        true
    }

    /// The slot `PrismForwarding.[Elite]SupportWeapon` was written to.
    pub(crate) fn support_weapon_index(&self, elite: bool) -> i32 {
        if elite {
            self.elite_support_weapon_index
        } else {
            self.support_weapon_index
        }
    }

    pub(crate) fn set_support_weapon_index(&mut self, elite: bool, index: i32) {
        if elite {
            self.elite_support_weapon_index = index;
        } else {
            self.support_weapon_index = index;
        }
    }

    /// `CanAttack`: a `yes` or `attack` tower.
    pub(crate) fn can_attack(&self) -> bool {
        matches!(
            self.mode,
            PrismForwardingMode::Yes | PrismForwardingMode::Attack
        )
    }

    /// `CanForward`: a `yes` or `forward` tower.
    pub(crate) fn can_forward(&self) -> bool {
        matches!(
            self.mode,
            PrismForwardingMode::Yes | PrismForwardingMode::Forward
        )
    }

    /// Whether this tower may support a tower of `target_type`.
    pub(crate) fn targets(&self, target_type: &str) -> bool {
        self.targets
            .iter()
            .any(|target| target.eq_ignore_ascii_case(target_type))
    }

    /// `GetMaxFeeds`, over `PrismSupportMax=`.
    pub(crate) fn max_feeds(&self, prism_support_max: i32) -> i32 {
        self.max_feeds.unwrap_or(prism_support_max)
    }

    /// `GetMaxNetworkSize`, over `PrismSupportMax=`.
    pub(crate) fn max_network_size(&self, prism_support_max: i32) -> i32 {
        self.max_network_size.unwrap_or(prism_support_max)
    }

    pub(crate) fn max_chain_length(&self) -> i32 {
        self.max_chain_length
    }

    /// `GetSupportModifier`, over `PrismSupportModifier=`.
    pub(crate) fn support_modifier(&self, prism_support_modifier: i32) -> i32 {
        self.support_modifier.unwrap_or(prism_support_modifier)
    }

    pub(crate) fn damage_add(&self) -> i32 {
        self.damage_add
    }

    pub(crate) fn intensity(&self) -> i32 {
        self.intensity
    }

    pub(crate) fn charge_delay(&self) -> i32 {
        self.charge_delay
    }

    pub(crate) fn to_allies(&self) -> bool {
        self.to_allies
    }

    pub(crate) fn break_support(&self) -> bool {
        self.break_support
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rules::ini_parser::IniFile;

    /// Initialize enables only the PrismType and targets the type itself;
    /// a disabled tower reads none of the other keys, and a ChargeDelay
    /// below 1 becomes 1 (`10010680`).
    #[test]
    fn prism_type_seeds_the_tower_and_disabled_towers_skip_the_keys() {
        let ini = IniFile::from_str(
            "[TWR]\nPrismForwarding.MaxFeeds=3\n\
             [OTHER]\nPrismForwarding.MaxFeeds=3\n\
             [FWD]\nPrismForwarding=Forward\nPrismForwarding.ChargeDelay=0\n\
             PrismForwarding.MaxNetworkSize=-1\nPrismForwarding.Intensity=x\n",
        );
        let mut tower = PrismForwardingRules::default();
        assert!(tower.read_pass(ini.section("TWR").unwrap(), "TWR", Some("TWR"), None));
        assert!(tower.can_attack() && tower.can_forward() && tower.targets("twr"));
        assert_eq!(tower.max_feeds(8), 3);
        let mut other = PrismForwardingRules::default();
        assert!(!other.read_pass(ini.section("OTHER").unwrap(), "OTHER", Some("TWR"), None));
        assert_eq!(other.max_feeds(8), 8);
        let mut forward = PrismForwardingRules::default();
        assert!(forward.read_pass(ini.section("FWD").unwrap(), "FWD", Some("TWR"), None));
        assert!(forward.can_forward() && !forward.can_attack());
        assert_eq!(forward.charge_delay(), 1);
        assert_eq!(forward.max_network_size(8), -1);
        assert_eq!(forward.intensity(), -2);
    }

    /// `PrismForwarding.SupportWeapon` (`100103F0`) allocates its weapon,
    /// takes the first empty slot of 2..17 and keeps it on later passes. The
    /// weapon gets `NeverUse=yes`, and `[CombatDamage] C4Warhead` once that
    /// is known while the weapon still has no Warhead: not on the first
    /// rules pass, where CombatDamage follows the types.
    #[test]
    fn support_weapon_takes_a_free_slot_and_the_c4_warhead() {
        use crate::rules::native_processing::{RulesLayerKind, RulesLayerStack};
        use crate::rules::ruleset::RuleSet;
        let mut layers = RulesLayerStack::new(IniFile::from_str(
            "[General]\nPrismType=TWR\n[BuildingTypes]\n0=TWR\n[Warheads]\n0=Super\n\
             [CombatDamage]\nC4Warhead=Super\n\
             [TWR]\nPrimary=SHOT\nPrismForwarding.SupportWeapon=SUP\n[SHOT]\nDamage=1\n",
        ));
        let rules = RuleSet::from_processed_rules(&layers.process().unwrap()).unwrap();
        let tower = rules.object("TWR").unwrap();
        let prism = tower.building_type_ext().prism_forwarding();
        assert_eq!(prism.support_weapon_index(false), 2);
        assert_eq!(prism.support_weapon_index(true), -1);
        assert_eq!(tower.weapon_at(2), Some("SUP"));
        let support = rules.weapon("SUP").unwrap();
        assert!(support.never_use);
        assert_eq!(support.warhead, None);
        layers.push(
            RulesLayerKind::Scenario,
            IniFile::from_str("[TWR]\nPrismForwarding.SupportWeapon=SUP\n"),
        );
        let rules = RuleSet::from_processed_rules(&layers.process().unwrap()).unwrap();
        let tower = rules.object("TWR").unwrap();
        assert_eq!(
            tower
                .building_type_ext()
                .prism_forwarding()
                .support_weapon_index(false),
            2
        );
        assert_eq!(tower.weapon_at(3), None);
        assert_eq!(
            rules.weapon("SUP").unwrap().warhead.as_deref(),
            Some("Super")
        );
    }
}
