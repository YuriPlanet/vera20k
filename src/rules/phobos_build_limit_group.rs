//! Phobos `BuildLimitGroup.*` type keys (`Ext/TechnoType/Body.cpp:1033-1041`;
//! fields and defaults `Ext/TechnoType/Body.h:265-273,697-705`) and the two
//! `[General]` fallbacks (`Ext/Rules/Body.cpp:223-224`, `Body.h:130-131,667-668`).
//!
//! The consumer is the House build-limit gate in `sim::production::can_build`
//! (`HouseExt::BuildLimitGroupUpgradeCheck @ Ext/House/Body.cpp:802-1013`,
//! `HouseExt::ReachedBuildLimit @ Ext/House/Body.cpp:1036-1188`). Type names
//! are kept as `Vec<String>` because types are still being constructed while
//! this section is read; the consumer drops unresolved names *before* pairing
//! with `Nums`, which reproduces the native read-time filter of
//! `Parser<TechnoTypeClass*>` (an invalid token is skipped, it does not hold a
//! `Nums` slot).
//!
//! ## Dependency rules
//! - Part of rules/ — no dependency on sim/, render/, ui/, etc.

use crate::rules::ini_parser::IniSection;

/// The nine `BuildLimitGroup.*` keys on one TechnoType. Registered on
/// `ObjectType` under `#[cfg(feature = "phobos")]`; this module is gated the
/// same way, so stock builds do not compile any of these fields.
#[derive(Debug, Clone, PartialEq)]
pub struct BuildLimitGroupSettings {
    /// `BuildLimitGroup.Types=`, `ValueableVector<TechnoTypeClass*>`, default
    /// empty. Unresolved names stay in the list for the consumer, which drops
    /// them before pairing (the native reader's skip).
    pub types: Vec<String>,
    /// `BuildLimitGroup.Nums=`, `ValueableVector<int>`, default empty.
    /// Unparseable tokens are dropped by the native parser, so the indices of
    /// the surviving values shift left (`Nums=2,3,x` is `[2,3]`).
    pub nums: Vec<i32>,
    /// `BuildLimitGroup.Factor=`, **`Valueable<int>`** (not double), default 1.
    pub factor: i32,
    /// `BuildLimitGroup.ContentIfAnyMatch=`, **`Nullable<bool>`**: unset falls
    /// back to `[General] BuildLimitGroup.ContentIfAnyMatch`.
    pub content_if_any_match: Option<bool>,
    /// `BuildLimitGroup.NotBuildableIfQueueMatch=`, **`Nullable<bool>`**: unset
    /// falls back to `[General] BuildLimitGroup.NotBuildableIfQueueMatch`.
    pub not_buildable_if_queue_match: Option<bool>,
    /// `BuildLimitGroup.ExtraLimit.Types=`, `ValueableVector<TechnoTypeClass*>`.
    pub extra_limit_types: Vec<String>,
    /// `BuildLimitGroup.ExtraLimit.Nums=`, `ValueableVector<int>` (aligned).
    pub extra_limit_nums: Vec<i32>,
    /// `BuildLimitGroup.ExtraLimit.MaxCount=`, `ValueableVector<int>` (aligned).
    pub extra_limit_max_count: Vec<i32>,
    /// `BuildLimitGroup.ExtraLimit.MaxNum=`, `Valueable<int>`, default 0
    /// (`<= 0` means no cap).
    pub extra_limit_max_num: i32,
}

impl Default for BuildLimitGroupSettings {
    fn default() -> Self {
        Self {
            types: Vec::new(),
            nums: Vec::new(),
            factor: 1,
            content_if_any_match: None,
            not_buildable_if_queue_match: None,
            extra_limit_types: Vec::new(),
            extra_limit_nums: Vec::new(),
            extra_limit_max_count: Vec::new(),
            extra_limit_max_num: 0,
        }
    }
}

impl BuildLimitGroupSettings {
    pub fn from_section(section: &IniSection) -> Self {
        use crate::rules::native_processing::read_valueable_type_vector;
        // `ValueableVector<TechnoTypeClass*>` filters at read time natively;
        // the names are kept here until a RuleSet can resolve them, and the
        // consumer applies the same filter before pairing.
        let keep = |name: &str| Some(name.to_owned());
        let read_ints = |key: &str| section.read_ares_int_list(key).unwrap_or_default();
        Self {
            types: read_valueable_type_vector(section, "BuildLimitGroup.Types", &[], keep),
            nums: read_ints("BuildLimitGroup.Nums"),
            factor: section.read_ares_int("BuildLimitGroup.Factor", 1),
            content_if_any_match: section
                .read_phobos_nullable_bool("BuildLimitGroup.ContentIfAnyMatch", None),
            not_buildable_if_queue_match: section
                .read_phobos_nullable_bool("BuildLimitGroup.NotBuildableIfQueueMatch", None),
            extra_limit_types: read_valueable_type_vector(
                section,
                "BuildLimitGroup.ExtraLimit.Types",
                &[],
                keep,
            ),
            extra_limit_nums: read_ints("BuildLimitGroup.ExtraLimit.Nums"),
            extra_limit_max_count: read_ints("BuildLimitGroup.ExtraLimit.MaxCount"),
            extra_limit_max_num: section.read_ares_int("BuildLimitGroup.ExtraLimit.MaxNum", 0),
        }
    }
}

#[cfg(all(test, tests_rules_o_z))]
#[path = "phobos_build_limit_group_tests.rs"]
mod tests;
