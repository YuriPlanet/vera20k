//! Phobos `PassengerDeletion.*` (`Ext/TechnoType/Body.cpp:1389-1399`,
//! `New/Type/Affiliated/PassengerDeletionTypeClass.cpp`).
//!
//! The affiliated type exists only when the current pass sets a positive
//! `PassengerDeletion.Rate=` or `PassengerDeletion.UseCostAsRate=yes`
//! (`PassengerDeletionTypeClass::CanParse`): every other key keeps the
//! constructor default below.
//!
//! ## Dependency rules
//! - Part of rules/ — no dependency on sim/, render/, ui/, etc.

use crate::rules::ini_parser::IniSection;
use crate::rules::phobos_shields::AffectedHouse;

/// One `PassengerDeletionTypeClass` (`Body.h` field list).
#[derive(Debug, Clone, PartialEq)]
pub struct PassengerDeletionSettings {
    /// `PassengerDeletion.Rate=` (`:9` in the affiliated reader), default 0.
    pub rate: i32,
    /// `PassengerDeletion.Rate.SizeMultiply=` (`:11`), default `yes`.
    pub rate_size_multiply: bool,
    /// `PassengerDeletion.UseCostAsRate=` (`:12`), default `no`.
    pub use_cost_as_rate: bool,
    /// `PassengerDeletion.CostMultiplier=` (`:13`), default 1.0.
    pub cost_multiplier: f64,
    /// `PassengerDeletion.CostRateCap=` (`:14`), `Nullable<int>`.
    pub cost_rate_cap: Option<i32>,
    /// `PassengerDeletion.AllowedHouses=` (`:15`), default All.
    pub allowed_houses: AffectedHouse,
    /// `PassengerDeletion.DontScore=` (`:16`), default `no`.
    pub dont_score: bool,
    /// `PassengerDeletion.Soylent=` (`:17`), default `no`.
    pub soylent: bool,
    /// `PassengerDeletion.SoylentMultiplier=` (`:18`), default 1.0.
    pub soylent_multiplier: f64,
    /// `PassengerDeletion.SoylentAllowedHouses=` (`:19`), default Enemies.
    pub soylent_allowed_houses: AffectedHouse,
    /// `PassengerDeletion.DisplaySoylent=` (`:20`), default `no`.
    pub display_soylent: bool,
    /// `PassengerDeletion.DisplaySoylentToHouses=` (`:21`), default All.
    pub display_soylent_to_houses: AffectedHouse,
    /// `PassengerDeletion.DisplaySoylentOffset=` (`:22`), default 0,0.
    pub display_soylent_offset: [i32; 2],
    /// `PassengerDeletion.ReportSound=` (`:23`), `ValueableIdx<VocClass>`.
    pub report_sound: Option<String>,
    /// `PassengerDeletion.Anim=` (`:24`), `ValueableVector<AnimTypeClass*>`.
    pub anim: Vec<String>,
    /// `PassengerDeletion.UnderEMP=` (`:25`), default `no`.
    pub under_emp: bool,
}

impl Default for PassengerDeletionSettings {
    fn default() -> Self {
        Self {
            rate: 0,
            rate_size_multiply: true,
            use_cost_as_rate: false,
            cost_multiplier: 1.0,
            cost_rate_cap: None,
            allowed_houses: AffectedHouse::ALL,
            dont_score: false,
            soylent: false,
            soylent_multiplier: 1.0,
            soylent_allowed_houses: AffectedHouse::ENEMIES,
            display_soylent: false,
            display_soylent_to_houses: AffectedHouse::ALL,
            display_soylent_offset: [0, 0],
            report_sound: None,
            anim: Vec::new(),
            under_emp: false,
        }
    }
}

impl PassengerDeletionSettings {
    /// `PassengerDeletionTypeClass::CanParse` (`New/Type/Affiliated/
    /// PassengerDeletionTypeClass.cpp:3-13`) plus `LoadFromINI`'s reads.
    pub fn from_section(section: &IniSection) -> Option<Self> {
        let defaults = Self::default();
        let rate = section.read_int("PassengerDeletion.Rate", defaults.rate);
        let use_cost_as_rate =
            section.read_bool("PassengerDeletion.UseCostAsRate", defaults.use_cost_as_rate);
        if !(rate > 0 || use_cost_as_rate) {
            return None;
        }
        Some(Self {
            rate,
            rate_size_multiply: section.read_bool(
                "PassengerDeletion.Rate.SizeMultiply",
                defaults.rate_size_multiply,
            ),
            use_cost_as_rate,
            cost_multiplier: section
                .read_phobos_double("PassengerDeletion.CostMultiplier", defaults.cost_multiplier),
            cost_rate_cap: section
                .read_phobos_nullable_int("PassengerDeletion.CostRateCap", defaults.cost_rate_cap),
            allowed_houses: crate::rules::phobos_shields::read_affected_house(
                section,
                "PassengerDeletion.AllowedHouses",
                defaults.allowed_houses,
            ),
            dont_score: section.read_bool("PassengerDeletion.DontScore", defaults.dont_score),
            soylent: section.read_bool("PassengerDeletion.Soylent", defaults.soylent),
            soylent_multiplier: section.read_phobos_double(
                "PassengerDeletion.SoylentMultiplier",
                defaults.soylent_multiplier,
            ),
            soylent_allowed_houses: crate::rules::phobos_shields::read_affected_house(
                section,
                "PassengerDeletion.SoylentAllowedHouses",
                defaults.soylent_allowed_houses,
            ),
            display_soylent: section
                .read_bool("PassengerDeletion.DisplaySoylent", defaults.display_soylent),
            display_soylent_to_houses: crate::rules::phobos_shields::read_affected_house(
                section,
                "PassengerDeletion.DisplaySoylentToHouses",
                defaults.display_soylent_to_houses,
            ),
            // Phobos `Valueable<Point2D>`: `INI_EX::Read2Integers` writes each
            // converted token of `Parser<int, 2>` straight into the pair, so a
            // one-token read changes only X and a failed first token keeps both.
            display_soylent_offset: section.read_phobos_int_pair(
                "PassengerDeletion.DisplaySoylentOffset",
                defaults.display_soylent_offset,
            ),
            report_sound: section
                .read_name("PassengerDeletion.ReportSound", 0x80)
                .map(str::to_owned),
            anim: section
                .read_list("PassengerDeletion.Anim", 2048)
                .map(|tokens| tokens.into_iter().map(str::to_owned).collect())
                .unwrap_or_default(),
            under_emp: section.read_bool("PassengerDeletion.UnderEMP", defaults.under_emp),
        })
    }
}

#[cfg(all(test, tests_rules_o_z))]
#[path = "phobos_passenger_deletion_tests.rs"]
mod tests;
