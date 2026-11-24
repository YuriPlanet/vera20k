//! Retained BuildingType secret-production settings. Original gamemd.exe:
//! constructor45DE94..45DEA6/45E0EA and readers4605BB..460683/4609B6.
//! Native execution and layered controls: tools/can_build_secret_lab.json.

#[derive(Debug, Clone, Default, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct SecretLabRules {
    enabled: bool,
    /// Infantry, Unit, Building pointers, in GetSecretProduction459840 order.
    overrides: [Option<String>; 3],
}

impl SecretLabRules {
    pub fn enabled(&self) -> bool {
        self.enabled
    }

    pub fn production_override(&self) -> Option<(crate::rules::object_type::ObjectCategory, &str)> {
        use crate::rules::object_type::ObjectCategory;
        [
            ObjectCategory::Infantry,
            ObjectCategory::Vehicle,
            ObjectCategory::Building,
        ]
        .into_iter()
        .zip(&self.overrides)
        .find_map(|(category, name)| name.as_deref().map(|name| (category, name)))
    }

    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn overrides_for_test(&self) -> &[Option<String>; 3] {
        &self.overrides
    }

    pub(super) fn read_flag(&mut self, section: &crate::rules::ini_parser::IniSection) {
        // ReadBool5295F0 takes the existing byte as default, without a clamp.
        self.enabled = section.read_bool("SecretLab", self.enabled);
    }

    pub(super) fn set_override(&mut self, family: usize, value: Option<String>) {
        self.overrides[family] = value;
    }
}
