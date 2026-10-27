//! Phobos `FlyNoWobbles` for technos on `Locomotor=Fly`.
//!
//! Retail `0x4CF8B1` tests `TechnoType+C95` (`IsDropship`) to decide whether
//! `FlyLocomotionClass::Draw_Point` skips its wobble, so a mod can only turn
//! the wobble off by also taking every other `IsDropship` behaviour. Phobos's
//! `FlyLocomotionClass_Draw_Point_NoWobbles` hook at that same address
//! (`src/Ext/Techno/Hooks.cpp`) replaces the read with `FlyNoWobbles`, stored
//! per TechnoType in `AircraftTypeExt` (`src/Ext/AircraftType/Body.cpp`) and
//! globally in `RulesExt` from `[AudioVisual]` (`src/Ext/Rules/Body.cpp`).
//!
//! The resolution is the hook's
//! `pTypeExt->FlyNoWobbles.Get(Global()->FlyNoWobbles.Get(pType->IsDropship))`:
//! the type's own value, then the global, then the retail `IsDropship` rule.
//!
//! ## Dependency rules
//! - Part of rules/ — no dependencies on app/, render/, ui/, sim/ or audio/.

/// The value original `0x4CF8B1` branches on: `true` skips the wobble.
pub fn resolve(techno: Option<bool>, global: Option<bool>, is_dropship: bool) -> bool {
    techno.or(global).unwrap_or(is_dropship)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn techno_then_global_then_the_retail_dropship_rule() {
        // The type's own value always wins, either way.
        assert!(resolve(Some(true), Some(false), false));
        assert!(!resolve(Some(false), Some(true), true));
        // Unset types take the global switch.
        assert!(resolve(None, Some(true), false));
        assert!(!resolve(None, Some(false), true));
        // Both unset keeps the retail rule, which reads IsDropship alone.
        assert!(!resolve(None, None, false));
        assert!(resolve(None, None, true));
    }

    /// The whole chain through the production readers: `[AudioVisual]`, the
    /// TechnoType section, and the resolved value the draw branch consumes.
    #[test]
    fn ruleset_resolves_the_branch_value_from_each_layer() {
        use crate::rules::ini_parser::IniFile;
        use crate::rules::ruleset::RuleSet;

        for (global, techno, dropship, expected) in [
            (None, None, false, false),
            (None, None, true, true),
            (Some("no"), None, false, false),
            (Some("yes"), None, false, true),
            // Turning the wobble off without taking IsDropship's behaviours.
            (None, Some("yes"), false, true),
            // And turning it back on for a dropship type.
            (Some("yes"), Some("no"), true, false),
        ] {
            let mut text = String::new();
            if let Some(global) = global {
                text.push_str(&format!("[AudioVisual]\nFlyNoWobbles={global}\n"));
            }
            text.push_str("[AircraftTypes]\n0=TEST\n[TEST]\n");
            if let Some(techno) = techno {
                text.push_str(&format!("FlyNoWobbles={techno}\n"));
            }
            text.push_str(if dropship {
                "IsDropship=yes\n"
            } else {
                "IsDropship=no\n"
            });
            let rules = RuleSet::from_ini(&IniFile::from_str(&text)).expect("rules read");
            let ty = rules.object("TEST").expect("TEST type");
            assert_eq!(rules.fly_no_wobbles_for(ty), expected, "{text}");
        }
    }

    /// Phobos keeps the key in `AircraftTypeExt`, so a ground type that
    /// repeats it carries no value and keeps the global/retail answer.
    #[test]
    fn only_aircraft_sections_carry_the_key() {
        use crate::rules::ini_parser::IniFile;
        use crate::rules::ruleset::RuleSet;

        let rules = RuleSet::from_ini(&IniFile::from_str(
            "[VehicleTypes]\n0=TANK\n[TANK]\nFlyNoWobbles=yes\n",
        ))
        .expect("rules read");
        let ty = rules.object("TANK").expect("TANK type");
        assert!(!rules.fly_no_wobbles_for(ty));
    }
}
