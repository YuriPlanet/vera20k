//! Phobos `SWTypeExt`'s `LimboDelivery.*`/`LimboKill.*` keys.
//!
//! Reader `SWTypeExt::LoadFromINIFile` (`src/Ext/SWType/Body.cpp:159-169`):
//! `LimboDelivery.Types` (`ValueableVector<BuildingTypeClass*>`),
//! `.IDs`/`LimboKill.IDs`/`.Counts` (`ValueableVector<int>`),
//! `.RollChances` (`ValueableVector<float>`), the numbered
//! `LimboDelivery.RandomWeightsN` lists with their unnumbered slot-0 alias,
//! and `LimboKill.AffectsHouse` (`Valueable<AffectedHouse>`, constructor
//! `Owner`) read after the deprecated `LimboKill.Affected` spelling
//! (`Body.cpp:162-166`). Declarations and defaults: `Body.h:65-70`, `:98`,
//! `:164-168`.
//!
//! Consumers, in `SWTypeExt::FireSuperWeaponExt`
//! (`Ext/SWType/FireSuperWeapon.cpp:13-40`): `ApplyLimboDelivery` (`:172-201`)
//! and `ApplyLimboKill` (`:203-262`), run before `SW.Next`.
//!
//! `ValueableVector<BuildingTypeClass*>` resolves each token through
//! `BuildingTypeClass::Find`; an unknown name is dropped at read time and a
//! reached list replaces the field. The int and float vectors keep their
//! tokens in order; an unconvertible token is skipped. The native fields live
//! on the retained `SWTypeExt`, so a later rules layer that omits a key keeps
//! the earlier layer's value and the numbered weight lists grow through the
//! whole load instead of being rebuilt per layer (`Body.cpp:159-169`,
//! `:184-209`).
//!
//! ## Dependency rules
//! - Part of rules/ — no dependency on sim/, render/, ui/, etc.

use crate::rules::ini_parser::IniSection;
use crate::rules::phobos_shields::{AffectedHouse, read_affected_house};
use crate::util::native_x87::NativeF32Bits;

/// One superweapon type's limbo-delivery and limbo-kill settings.
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub(crate) struct PhobosSuperWeaponLimbo {
    /// `LimboDelivery.Types=`: the buildings delivered on launch.
    types: Vec<String>,
    /// `LimboDelivery.IDs=`: the `LimboID` each delivered building takes,
    /// carried over to later entries when the list is shorter.
    ids: Vec<i32>,
    /// `LimboDelivery.RollChances=`: binary32 chance per weighted roll.
    roll_chances: Vec<NativeF32Bits>,
    /// `LimboDelivery.RandomWeightsN=` / `.RandomWeights`.
    random_weights: Vec<Vec<i32>>,
    /// `LimboKill.IDs=`: the delivered `LimboID`s to remove.
    kill_ids: Vec<i32>,
    /// `LimboKill.Counts=`: per-id removal caps; a missing entry removes all.
    kill_counts: Vec<i32>,
    /// `LimboKill.AffectsHouse=` (legacy `LimboKill.Affected`), constructor
    /// `Owner`: which houses' delivered lists are walked.
    kill_affects_house: AffectedHouse,
}

impl Default for PhobosSuperWeaponLimbo {
    fn default() -> Self {
        Self {
            types: Vec::new(),
            ids: Vec::new(),
            roll_chances: Vec::new(),
            random_weights: Vec::new(),
            kill_ids: Vec::new(),
            kill_counts: Vec::new(),
            kill_affects_house: AffectedHouse::OWNER,
        }
    }
}

impl PhobosSuperWeaponLimbo {
    /// `SWTypeExt::LoadFromINIFile`'s limbo reads into the retained
    /// extension. Native calls this once per reached rules layer on the same
    /// `SWTypeExt`, so a key this layer does not reach keeps the earlier
    /// layer's value, and the numbered weight lists keep the native
    /// replacement rules (`Body.cpp:184-209`). `resolve_building` answers the
    /// canonical registered building name (`BuildingTypeClass::Find`).
    pub(crate) fn from_section(
        previous: Self,
        section: &IniSection,
        resolve_building: impl Fn(&str) -> Option<String>,
    ) -> Self {
        let mut out = previous;
        if let Some(types) = section.read_list("LimboDelivery.Types", 2048) {
            out.types = types
                .into_iter()
                .filter_map(|token| resolve_building(token.trim()))
                .collect();
        }
        // `LimboKill.Affected` is the deprecated spelling and is read first;
        // the canonical key then overrides it when both are present. Both
        // keep the retained value when absent.
        out.kill_affects_house =
            read_affected_house(section, "LimboKill.Affected", out.kill_affects_house);
        out.kill_affects_house =
            read_affected_house(section, "LimboKill.AffectsHouse", out.kill_affects_house);
        if let Some(ids) = section.read_int_list("LimboDelivery.IDs") {
            out.ids = ids;
        }
        if let Some(chances) = read_roll_chances(section, "LimboDelivery.RollChances") {
            out.roll_chances = chances;
        }
        crate::rules::phobos_sw_chaining::read_numbered_weights(
            section,
            "LimboDelivery",
            "LimboDelivery.RandomWeights",
            &mut out.random_weights,
        );
        if let Some(ids) = section.read_int_list("LimboKill.IDs") {
            out.kill_ids = ids;
        }
        if let Some(counts) = section.read_int_list("LimboKill.Counts") {
            out.kill_counts = counts;
        }
        out
    }

    pub(crate) fn types(&self) -> &[String] {
        &self.types
    }

    pub(crate) fn ids(&self) -> &[i32] {
        &self.ids
    }

    pub(crate) fn roll_chances(&self) -> Vec<f64> {
        self.roll_chances
            .iter()
            .map(|bits| f64::from(f32::from_bits(bits.bits())))
            .collect()
    }

    pub(crate) fn random_weights(&self) -> &[Vec<i32>] {
        &self.random_weights
    }

    pub(crate) fn kill_ids(&self) -> &[i32] {
        &self.kill_ids
    }

    pub(crate) fn kill_counts(&self) -> &[i32] {
        &self.kill_counts
    }

    pub(crate) fn kill_affects_house(&self) -> AffectedHouse {
        self.kill_affects_house
    }
}

/// `ValueableVector<float>`: a present list replaces the field; a token that
/// does not convert is skipped.
fn read_roll_chances(section: &IniSection, key: &str) -> Option<Vec<NativeF32Bits>> {
    section.read_phobos_double_list(key).map(|chances| {
        chances
            .into_iter()
            .map(|chance| NativeF32Bits::from_bits((chance as f32).to_bits()))
            .collect()
    })
}

#[cfg(test)]
#[path = "phobos_limbo_tests.rs"]
mod tests;
