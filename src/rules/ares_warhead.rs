//! Retained WarheadExt configuration shared by the Ares EMP and AttachEffect
//! receivers. Installed 10051890 owns defaults; 10051B70 owns House admission.
//! The retained processing registry reads it; RuleSet projects an immutable view.
#[cfg(feature = "ares")]
use crate::rules::ini_parser::IniSection;
use crate::util::native_x87::NativeF64Bits;

#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub(crate) struct AresWarheadRules {
    duration: i32,
    cap: i32,
    sparkles: Option<String>,
    affects_enemies: bool,
    affects_owner: Option<bool>,
    attach_effect: AttachEffectRules,
}
impl Default for AresWarheadRules {
    fn default() -> Self {
        Self {
            duration: 0,
            cap: -1,
            sparkles: None,
            affects_enemies: true,
            affects_owner: None,
            attach_effect: Default::default(),
        }
    }
}
impl AresWarheadRules {
    pub(crate) fn duration(&self) -> i32 {
        self.duration
    }
    pub(crate) fn cap(&self) -> i32 {
        self.cap
    }
    pub(crate) fn can_affect_house(
        &self,
        source_exists: bool,
        same_owner: bool,
        allied: bool,
        affects_allies: bool,
    ) -> bool {
        // Installed WarheadExt10051B70: the source tests the target's bit.
        !source_exists
            || if same_owner && self.affects_owner.is_some() {
                self.affects_owner.expect("present owner override")
            } else if allied {
                affects_allies
            } else {
                self.affects_enemies
            }
    }
    pub(crate) fn sparkles(&self) -> Option<&str> {
        self.sparkles.as_deref()
    }
    pub(crate) fn attach_effect(&self) -> &AttachEffectRules {
        &self.attach_effect
    }
    pub(crate) fn affects_enemies(&self) -> bool {
        self.affects_enemies
    }
    pub(crate) fn affects_owner(&self) -> Option<bool> {
        self.affects_owner
    }
    #[cfg(feature = "ares")]
    pub(crate) fn read_pass(
        &mut self,
        section: &IniSection,
        sparkle: Option<Option<String>>,
        animation: Option<Option<String>>,
    ) {
        self.duration = section.read_ares_int("EMP.Duration", self.duration);
        self.cap = section.read_ares_int("EMP.Cap", self.cap);
        if let Some(sparkle) = sparkle {
            self.sparkles = sparkle;
        }
        self.affects_enemies = section.read_ares_bool("AffectsEnemies", self.affects_enemies);
        self.affects_owner = section.read_ares_nullable_bool("AffectsOwner", self.affects_owner);
        self.attach_effect.read_pass(section, animation);
    }
}

/// WarheadExt+88, constructor 10051890 and reader 10058A50. This is the
/// descriptor, not the independently timed TechnoExt+20 instance vector.
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub(crate) struct AttachEffectRules {
    duration: i32,
    cumulative: bool,
    animation: Option<String>,
    anim_reset_on_reapply: bool,
    temporal_hides_anim: bool,
    penetrates_iron_curtain: bool,
    force_decloak: bool,
    discard_on_entry: bool,
    firepower: NativeF64Bits,
    armor: NativeF64Bits,
    rof: NativeF64Bits,
    speed: NativeF64Bits,
    cloakable: bool,
    delay: i32,
    initial_delay: i32,
}
impl Default for AttachEffectRules {
    fn default() -> Self {
        Self {
            duration: 0,
            cumulative: false,
            animation: None,
            anim_reset_on_reapply: false,
            temporal_hides_anim: false,
            penetrates_iron_curtain: false,
            force_decloak: false,
            discard_on_entry: false,
            firepower: NativeF64Bits::ONE,
            armor: NativeF64Bits::ONE,
            rof: NativeF64Bits::ONE,
            speed: NativeF64Bits::ONE,
            cloakable: false,
            delay: 0,
            initial_delay: 0,
        }
    }
}
impl AttachEffectRules {
    pub(crate) fn duration(&self) -> i32 {
        self.duration
    }
    pub(crate) fn animation(&self) -> Option<&str> {
        self.animation.as_deref()
    }
    pub(crate) fn cumulative(&self) -> bool {
        self.cumulative
    }
    pub(crate) fn anim_reset_on_reapply(&self) -> bool {
        self.anim_reset_on_reapply
    }
    pub(crate) fn discard_on_entry(&self) -> bool {
        self.discard_on_entry
    }
    pub(crate) fn penetrates_iron_curtain(&self) -> bool {
        self.penetrates_iron_curtain
    }
    pub(crate) fn speed_multiplier(&self) -> NativeF64Bits {
        self.speed
    }
    /// Ground WH effects support their ordered Foot speed aggregate. Other
    /// stat/decloak/Temporal/self-effect consumers remain separate mechanisms.
    pub(crate) fn visual_speed_path(&self) -> bool {
        self.firepower == NativeF64Bits::ONE
            && self.armor == NativeF64Bits::ONE
            && self.rof == NativeF64Bits::ONE
            && !self.cloakable
            && !self.force_decloak
            && !self.temporal_hides_anim
    }
    #[cfg(feature = "ares")]
    fn read_pass(&mut self, section: &IniSection, animation: Option<Option<String>>) {
        // Exact 10058A59..10058B3A read order. Missing keys retain the live
        // constructor/prior-layer value; existing scalar readers own parsing.
        self.duration = section.read_ares_int("AttachEffect.Duration", self.duration);
        self.cumulative = section.read_ares_bool("AttachEffect.Cumulative", self.cumulative);
        if let Some(animation) = animation {
            self.animation = animation;
        }
        self.anim_reset_on_reapply = section.read_ares_bool(
            "AttachEffect.AnimResetOnReapply",
            self.anim_reset_on_reapply,
        );
        self.temporal_hides_anim =
            section.read_ares_bool("AttachEffect.TemporalHidesAnim", self.temporal_hides_anim);
        self.penetrates_iron_curtain = section.read_ares_bool(
            "AttachEffect.PenetratesIronCurtain",
            self.penetrates_iron_curtain,
        );
        self.force_decloak =
            section.read_ares_bool("AttachEffect.ForceDecloak", self.force_decloak);
        self.discard_on_entry =
            section.read_ares_bool("AttachEffect.DiscardOnEntry", self.discard_on_entry);
        self.firepower =
            section.read_ares_double_bits("AttachEffect.FirepowerMultiplier", self.firepower);
        self.armor = section.read_ares_double_bits("AttachEffect.ArmorMultiplier", self.armor);
        self.rof = section.read_ares_double_bits("AttachEffect.ROFMultiplier", self.rof);
        self.speed = section.read_ares_double_bits("AttachEffect.SpeedMultiplier", self.speed);
        self.cloakable = section.read_ares_bool("AttachEffect.Cloakable", self.cloakable);
        self.delay = section.read_ares_int("AttachEffect.Delay", self.delay);
        self.initial_delay = section.read_ares_int("AttachEffect.InitialDelay", self.initial_delay);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rules::{
        ini_parser::IniFile,
        native_processing::{RulesLayerKind, RulesLayerStack},
        ruleset::RuleSet,
    };

    #[test]
    fn attach_effect_production_layers_retain_registry_references_and_feature_gate() {
        // All fifteen descriptor keys go through the retained production
        // processor, including invalid/missing later reads and Anim Find.
        let root = IniFile::from_str(
            "[Warheads]\n0=ATTACH_WH\n1=DEFAULT_WH\n\
             [Animations]\n0=AE_VISUAL\n\
             [ATTACH_WH]\nEMP.Duration=7\nAffectsOwner=yes\n\
             AttachEffect.Duration=5\nAttachEffect.Cumulative=yes\n\
             AttachEffect.Animation=ae_visual\nAttachEffect.AnimResetOnReapply=yes\n\
             AttachEffect.TemporalHidesAnim=yes\nAttachEffect.PenetratesIronCurtain=yes\n\
             AttachEffect.ForceDecloak=yes\nAttachEffect.DiscardOnEntry=yes\n\
             AttachEffect.FirepowerMultiplier=2\nAttachEffect.ArmorMultiplier=0.5\n\
             AttachEffect.ROFMultiplier=3\nAttachEffect.SpeedMultiplier=4\n\
             AttachEffect.Cloakable=yes\nAttachEffect.Delay=9\nAttachEffect.InitialDelay=11\n\
             [DEFAULT_WH]\nCellSpread=0\n",
        );
        let scenario = "[ATTACH_WH]\nAttachEffect.Duration=junk\n\
                        AttachEffect.Animation=UNREGISTERED\nAttachEffect.SpeedMultiplier=\n";
        let read = |source: IniFile, scenario: &str| {
            let mut layers = RulesLayerStack::new(source);
            layers.push(RulesLayerKind::Scenario, IniFile::from_str(scenario));
            RuleSet::from_processed_rules(&layers.process().unwrap()).unwrap()
        };
        let rules = read(root.clone(), scenario);
        let changed = read(root.clone(), "[ATTACH_WH]\nAttachEffect.Duration=6\n");
        let cleared = read(root, "[ATTACH_WH]\nAttachEffect.Animation=<none>\n");
        if crate::rules::ARES_ENABLED {
            let wh = rules.warhead("ATTACH_WH").unwrap().ares_rules().unwrap();
            assert_eq!(wh.duration(), 7, "existing EMP state remains independent");
            assert!(wh.can_affect_house(true, true, true, false));
            let ae = wh.attach_effect();
            assert_eq!(ae.duration, 5);
            assert!(ae.cumulative);
            assert_eq!(ae.animation(), Some("AE_VISUAL"));
            assert!(ae.anim_reset_on_reapply);
            assert!(ae.temporal_hides_anim);
            assert!(ae.penetrates_iron_curtain);
            assert!(ae.force_decloak);
            assert!(ae.discard_on_entry);
            assert_eq!(ae.firepower, NativeF64Bits::from_bits(2.0_f64.to_bits()));
            assert_eq!(ae.armor, NativeF64Bits::from_bits(0.5_f64.to_bits()));
            assert_eq!(ae.rof, NativeF64Bits::from_bits(3.0_f64.to_bits()));
            assert_eq!(ae.speed, NativeF64Bits::from_bits(4.0_f64.to_bits()));
            assert!(ae.cloakable);
            assert_eq!((ae.delay, ae.initial_delay), (9, 11));
            assert!(!ae.visual_speed_path());
            assert_eq!(
                rules
                    .warhead("DEFAULT_WH")
                    .unwrap()
                    .ares_rules()
                    .unwrap()
                    .attach_effect(),
                &AttachEffectRules::default(),
                "constructor defaults do not come from retail tuning"
            );
            assert_eq!(
                cleared
                    .warhead("ATTACH_WH")
                    .unwrap()
                    .ares_rules()
                    .unwrap()
                    .attach_effect()
                    .animation(),
                None,
                "successful native none clears the retained reference"
            );
            assert_ne!(
                rules.simulation_config_hash(),
                changed.simulation_config_hash()
            );
            assert_ne!(
                rules.simulation_config_hash(),
                cleared.simulation_config_hash()
            );
        } else {
            for candidate in [&rules, &changed, &cleared] {
                assert!(
                    candidate
                        .warhead("ATTACH_WH")
                        .unwrap()
                        .ares_rules()
                        .is_none()
                );
            }
            // The whole configuration hash deliberately includes raw source
            // identity, even for unconsumed keys (RuleSet::source_ini_hash).
            // Ignoring extension state does not erase different INI inputs.
            assert_ne!(rules.source_ini_hash(), changed.source_ini_hash());
            assert_ne!(rules.source_ini_hash(), cleared.source_ini_hash());
            assert_ne!(
                rules.simulation_config_hash(),
                changed.simulation_config_hash()
            );
            assert_ne!(
                rules.simulation_config_hash(),
                cleared.simulation_config_hash()
            );
        }
    }
}
