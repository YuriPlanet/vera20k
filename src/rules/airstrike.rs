//! TechnoType's retained Airstrike configuration, copied into each manager.
//! Original gamemd constructor711056..711075 clears all six fields;
//! reader71458A..714654 reads them in this order on every reached Rules pass.

use crate::rules::ini_parser::IniSection;

#[derive(Debug, Clone, Default, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct AirstrikeSettings {
    pub normal_count: i32,
    pub elite_count: i32,
    pub normal_type: Option<String>,
    pub elite_type: Option<String>,
    /// Already in simulation frames; ReadInt5276D0 applies no clamp or scaling.
    pub normal_recharge: i32,
    pub elite_recharge: i32,
}

impl AirstrikeSettings {
    pub(crate) fn from_ini_section(section: &IniSection) -> Self {
        let mut result = Self::default();
        result.apply_pass(section, |name| {
            (!crate::rules::ini_parser::is_native_none_type_name(name))
                .then(|| crate::rules::native_processing::abstract_type_stored_id(name))
        });
        result
    }

    /// Aircraft41CEF0 owns name resolution/allocation. The caller supplies that
    /// live registry operation so allocations remain at their native read point.
    /// An omitted elite key falls back to the newly read normal value, not the
    /// previous elite value. Empty ReadString results also take this fallback.
    pub(crate) fn apply_pass(
        &mut self,
        section: &IniSection,
        mut aircraft: impl FnMut(&str) -> Option<String>,
    ) {
        self.normal_count = section.read_int("AirstrikeTeam", self.normal_count);
        self.elite_count = section.read_int("EliteAirstrikeTeam", self.normal_count);
        let normal = section.read_string("AirstrikeTeamType", "", 0x80);
        if !normal.is_empty() {
            self.normal_type = aircraft(&normal);
        }
        let elite = section.read_string("EliteAirstrikeTeamType", "", 0x80);
        self.elite_type = if elite.is_empty() {
            self.normal_type.clone()
        } else {
            aircraft(&elite)
        };
        self.normal_recharge = section.read_int("AirstrikeRechargeTime", self.normal_recharge);
        self.elite_recharge = section.read_int("EliteAirstrikeRechargeTime", self.normal_recharge);
    }
}

#[cfg(test)]
#[path = "airstrike_tests.rs"]
mod tests;
