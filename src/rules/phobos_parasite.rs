//! Phobos `Parasite.*` warhead keys (`Ext/WarheadType/Body.cpp:322-325`,
//! consumers `Ext/Techno/Hooks.cpp:912-968`): the bite's culling eligibility,
//! the spark particle system, and the Squid grapple animation.
//!
//! `Parasite.ParticleSystem=` is a `Nullable<ParticleSystemType*>`;
//! `.DisableParticleSystem=` a `Nullable<bool>` (global `[CombatDamage]` is a
//! `Valueable<bool>` default false); `.CullingTarget=` a
//! `Valueable<AffectedTarget>` default `Infantry`; `.GrappleAnim=` a
//! `NullableIdx<AnimTypeClass>`, read by the case-insensitive declared-name
//! `AnimTypeClass::FindIndex` (`Utilities/TemplateDef.h:1875-1892`), so an
//! unknown name leaves the value unset and the fallback applies.
//!
//! ## Dependency rules
//! - Part of rules/ — no dependencies on sim/, render/, ui/, etc.

use crate::rules::ini_parser::IniSection;
use crate::rules::phobos_crit::{AffectedTarget, read_affected_target};

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct PhobosParasiteWarhead {
    pub particle_system: Option<String>,
    pub disable_particle_system: Option<bool>,
    pub culling_target: AffectedTarget,
    pub grapple_anim: Option<String>,
}

impl Default for PhobosParasiteWarhead {
    fn default() -> Self {
        Self {
            particle_system: None,
            disable_particle_system: None,
            culling_target: AffectedTarget::INFANTRY,
            grapple_anim: None,
        }
    }
}

impl PhobosParasiteWarhead {
    pub fn from_section(section: &IniSection) -> Self {
        let defaults = Self::default();
        Self {
            particle_system: section
                .read_type_name("Parasite.ParticleSystem", 0x100)
                .map(str::to_owned),
            disable_particle_system: section
                .read_phobos_nullable_bool("Parasite.DisableParticleSystem", None),
            culling_target: read_affected_target(
                section,
                "Parasite.CullingTarget",
                defaults.culling_target,
            ),
            grapple_anim: section
                .read_type_name("Parasite.GrappleAnim", 0x100)
                .map(str::to_owned),
        }
    }
}
