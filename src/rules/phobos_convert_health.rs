//! Phobos `Convert.Health*` TechnoType keys: a Foot converts to another type
//! when its health ratio enters a threshold band.
//!
//! Readers: `Ext/TechnoType/Body.cpp:1193-1197`
//! (`Convert.Health.AbovePercent`, `Convert.Health.BelowPercent`,
//! `Convert.Health`, with the `Above > Below` swap at `:1197`). Declarations
//! `Ext/TechnoType/Body.h:433-435`; defaults `-1.0`/`-1.0`/null at
//! `Body.h:853-855`. Consumer: `FootExt::HealthAutoConvertActions`
//! (`Ext/Foot/Body.cpp:909-927`) from the `FootClass_AI` tail
//! (`Ext/Techno/Hooks.cpp:42-47`), with `TechnoExt::IsHealthInThreshold`
//! (`Ext/Techno/Body.cpp:752-759`).
//!
//! ## Dependency rules
//! - Part of rules/ — no dependency on sim/, render/, ui/, etc.

use crate::rules::ini_parser::IniSection;

/// The three `Convert.Health*` values.
#[derive(Debug, Clone, PartialEq, Default, serde::Serialize, serde::Deserialize)]
pub struct PhobosConvertHealth {
    /// `Convert.Health=`: the type to convert to; absent or `none` disables.
    pub health: Option<String>,
    /// `Convert.Health.AbovePercent=`: the exclusive lower bound of the
    /// health ratio; `-1.0` disables when the other bound is also negative.
    pub above_percent: f64,
    /// `Convert.Health.BelowPercent=`: the inclusive upper bound.
    pub below_percent: f64,
}

impl PhobosConvertHealth {
    pub fn from_section(section: &IniSection) -> Self {
        let mut settings = Self {
            // `Nullable<TechnoTypeClass*>`: a plain Find read, `none` clears.
            health: section
                .read_type_name("Convert.Health", 0x800)
                .filter(|name| !name.eq_ignore_ascii_case("none"))
                .map(str::to_owned),
            above_percent: section.read_double("Convert.Health.AbovePercent", -1.0),
            below_percent: section.read_double("Convert.Health.BelowPercent", -1.0),
        };
        // `Body.cpp:1197`: a reversed pair is swapped so the band stays valid.
        if settings.above_percent > settings.below_percent {
            std::mem::swap(&mut settings.above_percent, &mut settings.below_percent);
        }
        settings
    }

    /// Whether the band is configured at all: both bounds negative disables
    /// the arm (`Ext/Foot/Body.cpp:918-921`).
    pub fn enabled(&self) -> bool {
        self.health.is_some() && !(self.above_percent < 0.0 && self.below_percent < 0.0)
    }
}
