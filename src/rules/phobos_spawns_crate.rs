//! Phobos `SpawnsCrate*` warhead keys: a detonation places a fixed powerup
//! crate at the impact, chosen from weighted numbered lists.
//!
//! Reader `WarheadTypeExt::LoadFromINIFile` (`Ext/WarheadType/Body.cpp:520-560`):
//! for `i` from 0, `SpawnsCrate{i}.Type` is a `Nullable<Powerup>`
//! (`Parser<Powerup>` names the fixed table at `0x007E523C`); entry 0 also
//! tries the unnumbered `SpawnsCrate.Type`. The first unset/invalid type ends
//! the list. `SpawnsCrate{i}.Weight` (`Nullable<int>`, entry 0 falls back to
//! `SpawnsCrate.Weight`) defaults to 1 when unset. The two vectors stay
//! parallel.
//!
//! Consumer `WarheadTypeExt::Detonate` (`Ext/WarheadType/Detonate.cpp:113-118`):
//! one Scenario `RandomDouble`, `GeneralUtils::ChooseOneWeighted` over the
//! weights, then `MapClass::PlacePowerupCrate(Cell(Coord2Cell(coords)),
//! types[index])` when the index is inside the type list.
//!
//! ## Dependency rules
//! - Part of rules/ — no dependencies on sim/, render/, ui/, etc.

use crate::rules::ini_parser::IniSection;
use crate::rules::powerups::powerup_index_from_name;

#[derive(Debug, Clone, Default, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct PhobosSpawnsCrate {
    pub types: Vec<usize>,
    pub weights: Vec<i32>,
}
impl PhobosSpawnsCrate {
    pub(crate) fn from_section(section: &IniSection) -> Self {
        let mut value = Self::default();
        for index in 0.. {
            let type_key = format!("SpawnsCrate{index}.Type");
            let mut powerup = read_powerup(section, &type_key);
            if index == 0 && powerup.is_none() {
                powerup = read_powerup(section, "SpawnsCrate.Type");
            }
            let Some(powerup) = powerup else {
                break;
            };
            let weight_key = format!("SpawnsCrate{index}.Weight");
            let mut weight = read_weight(section, &weight_key);
            if index == 0 && weight.is_none() {
                weight = read_weight(section, "SpawnsCrate.Weight");
            }
            value.types.push(powerup);
            value.weights.push(weight.unwrap_or(1));
        }
        value
    }
}

fn read_powerup(section: &IniSection, key: &str) -> Option<usize> {
    let raw = section.read_name(key, 0x100)?;
    if raw.eq_ignore_ascii_case("<default>") {
        return None;
    }
    powerup_index_from_name(raw)
}

fn read_weight(section: &IniSection, key: &str) -> Option<i32> {
    section
        .is_present(key)
        .then(|| section.read_phobos_nullable_int(key, None))
        .flatten()
}

#[cfg(all(test, tests_rules_o_z))]
#[path = "phobos_spawns_crate_tests.rs"]
mod tests;
