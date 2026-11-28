//! Phobos `ExtraThreat.*` / `ExtraThreatCoefficient.*` threat-scoring keys
//! (`Ext/TechnoType/Body.cpp:1181-1191`, declarations `Ext/TechnoType/Body.h:
//! 423-429`, constructed `:843-849`).
//!
//! The five `Nullable<double>` values fall back per key to the same-named
//! `[General]` global (`Ext/Rules/Body.cpp:535-539`, defaults
//! `Ext/Rules/Body.h:952-956`, all `0.0`). There is no enable key: Phobos
//! derives `ExtraThreat_Enabled` as "any resolved value is not `0.0`"
//! (`Body.cpp:1187-1191`); [`PhobosExtraThreat::enabled`] resolves that at the
//! consumer from the rebuilt merged layers, which is the same final set of
//! values.
//!
//! Consumer: `TechnoClass_ThreatCoefficient_CanAttackMeThreatBonus`
//! (`0x70CF87`, `Ext/Techno/Hooks.TargetEvaluation.cpp:429-511`), ported at
//! [`crate::sim::combat::greatest_threat`]'s score body.
//!
//! ## Dependency rules
//! - Part of rules/ — no dependencies on sim/, render/, ui/, etc.

use crate::rules::ini_parser::IniSection;
use crate::rules::ruleset::RuleSet;

#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct PhobosExtraThreat {
    /// `ExtraThreat.IsThreat=` (`Body.cpp:1181`): added when the candidate can
    /// fire at the scanner (`IsAThreatToMe`, `Hooks.TargetEvaluation.cpp:
    /// 381-404`).
    pub is_threat: Option<f64>,
    /// `AlwaysConsideredThreat=` (`Body.cpp:1182`, `Valueable<bool>`, default
    /// `no`): the candidate counts as a threat with no weapon (`:387-388`).
    pub always_considered_threat: bool,
    /// `ExtraThreat.InRange=` (`Body.cpp:1183`): the flat part of the
    /// close-enough bonus (`:455-469`).
    pub in_range: Option<f64>,
    /// `ExtraThreatCoefficient.InRangeDistance=` (`Body.cpp:1184`): the
    /// per-cell part of the close-enough bonus.
    pub in_range_distance: Option<f64>,
    /// `ExtraThreatCoefficient.Facing=` (`Body.cpp:1185`): the angular bonus.
    pub facing: Option<f64>,
    /// `ExtraThreatCoefficient.DistanceToLastTarget=` (`Body.cpp:1186`): the
    /// bonus from the candidate's distance to the scanner's last target.
    pub distance_to_last_target: Option<f64>,
}

impl PhobosExtraThreat {
    /// Reads in `TechnoTypeExt::LoadFromINIFile` order (`Body.cpp:1181-1186`);
    /// an absent key keeps the current value like every `Nullable::Read`.
    pub fn from_section(section: &IniSection) -> Self {
        Self {
            is_threat: section.read_phobos_nullable_double("ExtraThreat.IsThreat", None),
            always_considered_threat: section.read_bool("AlwaysConsideredThreat", false),
            in_range: section.read_phobos_nullable_double("ExtraThreat.InRange", None),
            in_range_distance: section
                .read_phobos_nullable_double("ExtraThreatCoefficient.InRangeDistance", None),
            facing: section.read_phobos_nullable_double("ExtraThreatCoefficient.Facing", None),
            distance_to_last_target: section
                .read_phobos_nullable_double("ExtraThreatCoefficient.DistanceToLastTarget", None),
        }
    }

    /// `ExtraThreat_IsThreat.Get(RulesExt::Global()->ExtraThreat_IsThreat)`.
    pub fn is_threat(&self, rules: &RuleSet) -> f64 {
        self.is_threat
            .unwrap_or(rules.phobos_globals.extra_threat_is_threat())
    }

    /// `ExtraThreat_InRange.Get(RulesExt::Global()->ExtraThreat_InRange)`.
    pub fn in_range(&self, rules: &RuleSet) -> f64 {
        self.in_range
            .unwrap_or(rules.phobos_globals.extra_threat_in_range())
    }

    /// `ExtraThreatCoefficient_InRangeDistance.Get(...)`.
    pub fn in_range_distance(&self, rules: &RuleSet) -> f64 {
        self.in_range_distance
            .unwrap_or(rules.phobos_globals.extra_threat_in_range_distance())
    }

    /// `ExtraThreatCoefficient_Facing.Get(...)`.
    pub fn facing(&self, rules: &RuleSet) -> f64 {
        self.facing
            .unwrap_or(rules.phobos_globals.extra_threat_coefficient_facing())
    }

    /// `ExtraThreatCoefficient_DistanceToLastTarget.Get(...)`.
    pub fn distance_to_last_target(&self, rules: &RuleSet) -> f64 {
        self.distance_to_last_target.unwrap_or(
            rules
                .phobos_globals
                .extra_threat_coefficient_distance_to_last_target(),
        )
    }

    /// `ExtraThreat_Enabled` (`Body.cpp:1187-1191`): any resolved value is not
    /// `0.0`. Because every bonus body also checks its own resolved value, a
    /// disabled type adds nothing either way; the flag keeps the native early
    /// return.
    pub fn enabled(&self, rules: &RuleSet) -> bool {
        self.is_threat(rules) != 0.0
            || self.in_range(rules) != 0.0
            || self.in_range_distance(rules) != 0.0
            || self.facing(rules) != 0.0
            || self.distance_to_last_target(rules) != 0.0
    }
}
