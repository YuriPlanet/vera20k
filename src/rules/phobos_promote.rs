//! Phobos `Promote.*` type keys (`Ext/TechnoType/Body.cpp:800`,
//! `:1128-1129`; fields `Body.h:68,341-342`; constructor defaults
//! `Body.h:506,778-779`) and their three `[General]`/`[AudioVisual]`
//! fallbacks (`Ext/Rules/Body.cpp:212,316-317`; fields `Body.h:119,239-240`;
//! constructor defaults `:656,748-749`).
//!
//! Consumers: the rank-crossing animation
//! (`TechnoClass_AI_PromoteAnim @ 0x6FA07A`,
//! `Ext/Techno/Hooks.cpp:158-170` → `TechnoExt::ShowPromoteAnim`,
//! `Ext/Techno/Body.Visuals.cpp:863-874`) and the spawner rank lift
//! (`SpawnManagerClass_AI_PromoteSpawns @ 0x6B7282`,
//! `Ext/Techno/Hooks.Misc.cpp:216-238`).
//!
//! Literal semantics: each animation list is a `ValueableVector`, so a read
//! replaces the whole list (an absent or empty key keeps it); a non-empty
//! type list replaces — not appends to — the global list of that tier.
//!
//! ## Dependency rules
//! - Part of rules/ — no dependency on sim/, render/, ui/, etc.

use crate::rules::ini_parser::IniSection;
use crate::rules::native_processing::read_valueable_type_vector;

/// The three `Promote.*` keys on one TechnoType.
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct PhobosPromote {
    /// `Promote.IncludeSpawns=`, `Nullable<bool>`: unset falls back to the
    /// `[General]` global.
    pub include_spawns: Option<bool>,
    /// `Promote.VeteranAnimation=`, `ValueableVector<AnimTypeClass*>`.
    pub veteran_animation: Vec<String>,
    /// `Promote.EliteAnimation=`, `ValueableVector<AnimTypeClass*>`.
    pub elite_animation: Vec<String>,
}

impl Default for PhobosPromote {
    fn default() -> Self {
        Self {
            include_spawns: None,
            veteran_animation: Vec::new(),
            elite_animation: Vec::new(),
        }
    }
}

impl PhobosPromote {
    pub fn from_section(section: &IniSection) -> Self {
        // `ValueableVector<AnimTypeClass*>` drops unresolvable names at read
        // time natively; the names stay here until a RuleSet resolves them,
        // and the consumer drops the ones it cannot resolve.
        let keep = |name: &str| Some(name.to_owned());
        Self {
            include_spawns: section.read_phobos_nullable_bool("Promote.IncludeSpawns", None),
            veteran_animation: read_valueable_type_vector(
                section,
                "Promote.VeteranAnimation",
                &[],
                keep,
            ),
            elite_animation: read_valueable_type_vector(
                section,
                "Promote.EliteAnimation",
                &[],
                keep,
            ),
        }
    }

    pub fn include_spawns(&self, global: bool) -> bool {
        self.include_spawns.unwrap_or(global)
    }

    /// `!Promote_VeteranAnimation.empty() ? type : [AudioVisual]`.
    pub fn veteran_animation<'a>(&'a self, global: &'a [String]) -> &'a [String] {
        if self.veteran_animation.is_empty() {
            global
        } else {
            &self.veteran_animation
        }
    }

    /// `!Promote_EliteAnimation.empty() ? type : [AudioVisual]`.
    pub fn elite_animation<'a>(&'a self, global: &'a [String]) -> &'a [String] {
        if self.elite_animation.is_empty() {
            global
        } else {
            &self.elite_animation
        }
    }
}

#[cfg(all(test, tests_rules_o_z))]
#[path = "phobos_promote_tests.rs"]
mod tests;
