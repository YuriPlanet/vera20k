//! Phobos `TransactMoney.*` warhead keys
//! (`Ext/WarheadType/Body.cpp:159-163`, fields `Ext/WarheadType/Body.h:30-34`,
//! constructor defaults `Body.h:309-313`).
//!
//! A detonating warhead moves `TransactMoney` credits to its firing house and
//! optionally emits a flying credit label. Consumer:
//! `WarheadTypeExt::Detonate` (`Ext/WarheadType/Detonate.cpp:102-111`), ported
//! at the detonation entry (`sim::combat::crit::detonate`).
//!
//! ## Dependency rules
//! - Part of rules/ — no dependency on sim/, render/, ui/, etc.

use crate::rules::ini_parser::IniSection;
use crate::rules::phobos_shields::{AffectedHouse, read_affected_house};

/// The five `TransactMoney.*` values.
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct TransactMoneyRules {
    /// `TransactMoney=`, `Valueable<int>`, default 0. Positive credits the
    /// firing house, negative debits it.
    pub amount: i32,
    /// `TransactMoney.Display=`, `Valueable<bool>`, default `no`.
    pub display: bool,
    /// `TransactMoney.Display.Houses=`, `Valueable<AffectedHouse>`, default
    /// `All`: which houses see the label.
    pub display_houses: AffectedHouse,
    /// `TransactMoney.Display.AtFirer=`, `Valueable<bool>`, default `no`: the
    /// label sits on the firer's `Location` instead of the impact point.
    pub display_at_firer: bool,
    /// `TransactMoney.Display.Offset=`, `Valueable<Point2D>`, default `0,0`.
    pub display_offset: [i32; 2],
}

impl Default for TransactMoneyRules {
    fn default() -> Self {
        Self {
            amount: 0,
            display: false,
            display_houses: AffectedHouse::ALL,
            display_at_firer: false,
            display_offset: [0, 0],
        }
    }
}

impl TransactMoneyRules {
    pub fn from_section(section: &IniSection) -> Self {
        Self {
            amount: section.read_ares_int("TransactMoney", 0),
            display: section.read_bool("TransactMoney.Display", false),
            display_houses: read_affected_house(
                section,
                "TransactMoney.Display.Houses",
                AffectedHouse::ALL,
            ),
            display_at_firer: section.read_bool("TransactMoney.Display.AtFirer", false),
            display_offset: section.read_phobos_int_pair("TransactMoney.Display.Offset", [0, 0]),
        }
    }
}
