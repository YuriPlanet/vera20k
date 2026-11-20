//! Phobos `WeaponTypeExt` ExtraWarheads (`Ext/WeaponType/Body.cpp:139-166`,
//! `Ext/Bullet/Hooks.DetonateLogics.cpp:367-470`): warheads detonated after
//! the weapon's own warhead at the same impact.
//!
//! Reader order: `ExtraWarheads` (a `ValueableVector<WarheadTypeClass*>`, so
//! names are kept here and resolved by the consumer like every other Phobos
//! type list), `.DamageOverrides` (`ValueableVector<int>`), `.DetonationChances`
//! (`ValueableVector<double>`, binary32 with `%` at 0.01), `.RollChances` (the
//! same), the numbered `ExtraWarheads.RandomWeights{i}` list loop (stopping at
//! the first absent or empty entry) then the unnumbered alias replacing entry 0
//! or appending it, and `.FullDetonation` (`ValueableVector<bool>`).
//!
//! ## Dependency rules
//! - Part of rules/ — no dependencies on sim/, render/, ui/, etc.

use crate::rules::ini_parser::IniSection;
use crate::rules::ini_value::read_bool_decision;

#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct PhobosExtraWarheads {
    pub warheads: Vec<String>,
    pub damage_overrides: Vec<i32>,
    /// `DetonationChances=`: an empty list detonates every extra warhead.
    pub detonation_chances: Vec<f64>,
    /// `RollChances=`: an empty list is one sure roll.
    pub roll_chances: Vec<f64>,
    /// `RandomWeights{i}=`: per-roll warhead weights; empty selects every
    /// warhead in order.
    pub weights: Vec<Vec<i32>>,
    /// `FullDetonation=`: an empty list treats every row as full.
    pub full_detonation: Vec<bool>,
}

impl PhobosExtraWarheads {
    pub fn from_section(section: &IniSection) -> Self {
        let mut weights = Vec::new();
        for index in 0.. {
            let Some(entry) =
                section.read_ares_int_list(&format!("ExtraWarheads.RandomWeights{index}"))
            else {
                break;
            };
            // `if (!weights3.size()) break;`
            if entry.is_empty() {
                break;
            }
            weights.push(entry);
        }
        if let Some(entry) = section.read_ares_int_list("ExtraWarheads.RandomWeights") {
            if !entry.is_empty() {
                if weights.is_empty() {
                    weights.push(entry);
                } else {
                    weights[0] = entry;
                }
            }
        }
        Self {
            warheads: section
                .read_list("ExtraWarheads", 0x800)
                .map(|tokens| tokens.into_iter().map(str::to_owned).collect())
                .unwrap_or_default(),
            damage_overrides: section
                .read_ares_int_list("ExtraWarheads.DamageOverrides")
                .unwrap_or_default(),
            detonation_chances: section
                .read_phobos_double_list("ExtraWarheads.DetonationChances")
                .unwrap_or_default(),
            roll_chances: section
                .read_phobos_double_list("ExtraWarheads.RollChances")
                .unwrap_or_default(),
            weights,
            full_detonation: section
                .read_list("ExtraWarheads.FullDetonation", 0x800)
                .map(|tokens| tokens.into_iter().filter_map(read_bool_decision).collect())
                .unwrap_or_default(),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.warheads.is_empty()
    }
}
