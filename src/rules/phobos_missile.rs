//! Phobos `Missile.*` (`Ext/AircraftType/Body.cpp:42-44`).
//!
//! Three AircraftType keys shape the linked rocket's flight:
//! - `Missile.Cruise=` forces the cruise-missile raise path
//!   (`RocketLocomotionClass_Process_CruiseMissileCheck` hook `0x662354` and
//!   `_CruiseMissileRaise` `0x662720`).
//! - `Missile.TakeOffSeparation=` replaces the takeoff trailer timer
//!   (`_CustomSmokeInterval` `0x6623FC`, and the takeoff hook `0x6624FB`).
//! - `Missile.TakeOffAnim=` names the takeoff animation (`0x6624FB`), default
//!   `V3TAKOFF` (`AircraftTypeExt::Initialize`, `Body.cpp:12`).
//!
//! ## Dependency rules
//! - Part of rules/ — no dependencies on sim/, render/, ui/, etc.

use crate::rules::ini_parser::IniSection;

/// The three `AircraftTypeExt` missile fields (`Body.h:33-35` constructors
/// `:57-59`); `Initialize` seeds the anim with `Find("V3TAKOFF")` and the
/// separation with 24.
#[derive(Debug, Clone, PartialEq)]
pub struct PhobosMissileRules {
    pub cruise: bool,
    pub takeoff_separation: i32,
    /// The takeoff animation; `None` means none (`Missile.TakeOffAnim=<none>`),
    /// while an absent key keeps the `V3TAKOFF` default.
    pub takeoff_anim: Option<String>,
}

impl Default for PhobosMissileRules {
    fn default() -> Self {
        Self {
            cruise: false,
            takeoff_separation: 24,
            takeoff_anim: Some("V3TAKOFF".to_owned()),
        }
    }
}

impl PhobosMissileRules {
    pub fn from_section(section: &IniSection) -> Self {
        let defaults = Self::default();
        Self {
            cruise: section.read_bool("Missile.Cruise", defaults.cruise),
            takeoff_separation: section
                .read_ares_int("Missile.TakeOffSeparation", defaults.takeoff_separation),
            takeoff_anim: takeoff_anim(section, defaults.takeoff_anim),
        }
    }
}

/// `Valueable<AnimTypeClass*>`: absent and blank keep the current (default
/// V3TAKOFF); `none`/`<none>` clear it to null. An unknown name keeps the
/// name here; native `AnimTypeClass::Find` would keep the previous value
/// instead (recorded residual: no retail key names an unregistered anim).
fn takeoff_anim(section: &IniSection, current: Option<String>) -> Option<String> {
    if !section.is_present("Missile.TakeOffAnim") {
        return current;
    }
    match section.read_name("Missile.TakeOffAnim", 2048) {
        None => current,
        Some(raw) if raw.trim().is_empty() => current,
        Some(raw)
            if raw.trim().eq_ignore_ascii_case("none")
                || raw.trim().eq_ignore_ascii_case("<none>") =>
        {
            None
        }
        Some(raw) => Some(raw.trim().to_owned()),
    }
}

/// The takeoff animation and trailer-timer separation a Phobos-enabled rocket
/// uses: when the linked type carries settings, its keys replace the rules
/// block's anim (a cleared anim means none) and the stock 24-frame timer.
pub fn takeoff_settings<'a>(
    phobos: Option<&'a PhobosMissileRules>,
    rules_block_anim: Option<&'a str>,
) -> (Option<&'a str>, i32) {
    match phobos {
        Some(settings) => (
            settings.takeoff_anim.as_deref(),
            settings.takeoff_separation,
        ),
        None => (rules_block_anim, 24),
    }
}

#[cfg(all(test, tests_rules_o_z))]
#[path = "phobos_missile_tests.rs"]
mod tests;
