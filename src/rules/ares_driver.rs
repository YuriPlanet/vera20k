//! Optional Ares TechnoType driver and vehicle-thief configuration.
//! Installed TypeExt constructor1003C88A..1003C9AA and LoadFromINI
//! 1003E39B..1003E3F2 /1003E5E3..1003E60A /1003E76F..1003E7AC.
//! The native processing registry owns reached reads; ObjectType projects them.

use crate::rules::{ini_parser::IniSection, superweapon_type::TechnoTypeReference};

#[derive(Debug, Clone, Default, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub(crate) enum OperatorRequirement {
    #[default]
    None,
    Any,
    Types(Vec<TechnoTypeReference>),
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub(crate) struct DriverTypeRules {
    operator: OperatorRequirement,
    can_drive: bool,
    can_be_driven: bool,
    kill_pilots: i32,
    break_mind_control: bool,
    theft_allowed: bool,
    one_time: bool,
    #[serde(default)]
    protected: bool,
    #[serde(default)]
    protected_min_health: Option<crate::util::native_x87::NativeF64Bits>,
}

impl Default for DriverTypeRules {
    fn default() -> Self {
        Self {
            operator: OperatorRequirement::None,
            can_drive: false,
            can_be_driven: true,
            kill_pilots: 0,
            break_mind_control: true,
            theft_allowed: true,
            one_time: false,
            protected: false,
            protected_min_health: None,
        }
    }
}

impl DriverTypeRules {
    /// Operator1003E39B reads 2048 bytes, compares the whole value to
    /// `_ANY_` without case, then resolves comma tokens through the existing
    /// TechnoType array (1002A1F0 ->10007EA0), never allocating types.
    pub(crate) fn read_pass(
        &mut self,
        section: &IniSection,
        operator: Option<OperatorRequirement>,
    ) {
        if let Some(operator) = operator {
            self.operator = operator;
        }
        self.can_drive = section.read_ares_bool("CanDrive", self.can_drive);
        self.can_be_driven = section.read_ares_bool("CanBeDriven", self.can_be_driven);
        // TypeExt+1D4 ctor1003C99B=0; signed reader1003E759..1003E76A.
        self.kill_pilots = section.read_ares_int("VehicleThief.KillPilots", self.kill_pilots);
        self.break_mind_control =
            section.read_ares_bool("VehicleThief.BreakMindControl", self.break_mind_control);
        self.theft_allowed = section.read_ares_bool("VehicleThief.Allowed", self.theft_allowed);
        self.one_time = section.read_ares_bool("VehicleThief.OneTime", self.one_time);
        // TypeExt+163/+168/+170;1003E5B7..1003E5DE. Constructor1003C936
        // leaves protection false and nullable presence clear1003C940.
        self.protected = section.read_ares_bool("ProtectedDriver", self.protected);
        self.protected_min_health = section
            .read_ares_nullable_double_bits("ProtectedDriver.MinHealth", self.protected_min_health);
    }

    pub(crate) fn operator(&self) -> &OperatorRequirement {
        &self.operator
    }
    pub(crate) fn can_drive(&self) -> bool {
        self.can_drive
    }
    pub(crate) fn can_be_driven(&self) -> bool {
        self.can_be_driven
    }
    pub(crate) fn kill_pilots(&self) -> i32 {
        self.kill_pilots
    }
    pub(crate) fn break_mind_control(&self) -> bool {
        self.break_mind_control
    }
    pub(crate) fn theft_allowed(&self) -> bool {
        self.theft_allowed
    }
    pub(crate) fn one_time(&self) -> bool {
        self.one_time
    }
    pub(crate) fn protected(&self) -> bool {
        self.protected
    }
    pub(crate) fn protected_min_health(&self) -> Option<crate::util::native_x87::NativeF64Bits> {
        self.protected_min_health
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rules::{
        ini_parser::IniFile,
        native_processing::{RulesLayerKind, RulesLayerStack},
        ruleset::RuleSet,
    };

    #[test]
    fn driver_protection_and_warhead_inputs_keep_native_defaults_and_invalid_layers() {
        use crate::rules::ares_warhead::{HouseSelector, KillDriverRules};
        use crate::util::native_x87::NativeF64Bits;
        let root = "[VehicleTypes]\n0=HOST\n[Warheads]\n0=DRIVER_WH\n1=DEFAULT_WH\n\
                    [HOST]\nProtectedDriver=yes\nProtectedDriver.MinHealth=50%\n\
                    [DRIVER_WH]\nKillDriver=yes\nKillDriver.KillBelowPercent=50%\n\
                    KillDriver.Chance=25%\nKillDriver.Owner=victim\nKillDriver.RemoveVeterancy=yes\n\
                    EffectsRequireDamage=yes\nEffectsRequireVerses=no\n";
        let map = IniFile::from_str(
            "[HOST]\nProtectedDriver=invalid\nProtectedDriver.MinHealth=none\n\
             [DRIVER_WH]\nKillDriver=invalid\nKillBelowPercent=0\n\
             KillDriver.KillBelowPercent=invalid\nKillDriver.Chance=invalid\n\
             KillDriver.Owner=unknown\nKillDriver.RemoveVeterancy=invalid\n\
             EffectsRequireDamage=invalid\nEffectsRequireVerses=invalid\n",
        );
        let load = |root: &str| {
            let mut layers = RulesLayerStack::new(IniFile::from_str(root));
            layers.push(RulesLayerKind::Scenario, map.clone());
            let (_, trace) = layers
                .process()
                .unwrap()
                .into_ini_and_native_type_construction_trace();
            let retained = RulesLayerStack::new(IniFile::empty())
                .process_with_fixed_art_and_registry_state(
                    &IniFile::empty(),
                    trace.into_registry_state_discarding_events(),
                )
                .unwrap();
            RuleSet::from_processed_rules(&retained).unwrap()
        };
        let rules = load(root);
        let protection = rules.object("HOST").unwrap().driver_rules();
        assert!(protection.protected());
        assert_eq!(
            protection.protected_min_health(),
            Some(NativeF64Bits::from_bits(0.5f64.to_bits()))
        );
        let driver = rules
            .warhead("DRIVER_WH")
            .unwrap()
            .ares_rules()
            .unwrap()
            .kill_driver();
        assert!(driver.enabled() && driver.remove_veterancy());
        assert_eq!(
            driver.kill_below_percent(),
            NativeF64Bits::from_bits(0.5f64.to_bits())
        );
        assert_eq!(driver.chance(), NativeF64Bits::from_bits(0.25f64.to_bits()));
        assert_eq!(driver.owner(), HouseSelector::Victim);
        let effects = rules.warhead("DRIVER_WH").unwrap().ares_rules().unwrap();
        assert!(effects.effects_require_damage());
        assert!(!effects.effects_require_verses());
        let defaults = rules.warhead("DEFAULT_WH").unwrap().ares_rules().unwrap();
        assert!(!defaults.effects_require_damage() && defaults.effects_require_verses());
        assert_eq!(
            rules
                .warhead("DEFAULT_WH")
                .unwrap()
                .ares_rules()
                .unwrap()
                .kill_driver(),
            &KillDriverRules::default()
        );
        let changed = load(&root.replace("KillDriver.Chance=25%", "KillDriver.Chance=75%"));
        assert_eq!(changed.source_ini_hash(), rules.source_ini_hash());
        assert_ne!(
            rules.simulation_config_hash(),
            changed.simulation_config_hash()
        );
    }

    #[test]
    fn ares_vehicle_driver_rules_keep_native_defaults_and_reached_layers() {
        let mut layers = RulesLayerStack::new(IniFile::from_str(
            "[Sides]\nGDI=Neutral\nNod=Child\n[GDI]\nCrew=OP\nTechnician=none\n\
             [Countries]\n0=Neutral\n1=Child\n\
             [Neutral]\nMultiplayPassive=yes\nCanBeDriven=no\n\
             [Child]\nParentCountry=Neutral\nMultiplayPassive=yes\n\
             [InfantryTypes]\n0=DRIVER\n1=OP\n[VehicleTypes]\n0=HOST\n\
             [DRIVER]\nCanDrive=yes\nVehicleThief.BreakMindControl=no\n\
             VehicleThief.OneTime=yes\nOperator=_aNy_\n\
             Survivor.Pilots=2\nSurvivor.PilotChance=30\nSurvivor.VeteranPilotChance=75\n\
             Survivor.ElitePassengerChance=90\nSurvivor.Side1=OP\nCrew.TechnicianChance=0\n\
             [HOST]\nCanBeDriven=no\nVehicleThief.Allowed=no\nCrashable=no\n[OP]\nStrength=50\n",
        ));
        layers.push(
            RulesLayerKind::Scenario,
            IniFile::from_str(
                "[DRIVER]\nCanDrive=invalid\nVehicleThief.BreakMindControl=invalid\n\
             VehicleThief.OneTime=invalid\nOperator=\n\
             Survivor.PilotChance=invalid\nSurvivor.RookiePilotChance=0\n\
             Survivor.VeteranPilotChance=invalid\nSurvivor.Side1=MISSING_PILOT\n\
             Crew.TechnicianChance=invalid\n[GDI]\nCrew=MISSING_PILOT\n\
             [HOST]\nCanBeDriven=invalid\nVehicleThief.Allowed=invalid\nCrashable=invalid\n\
             [Neutral]\nCanBeDriven=invalid\n",
            ),
        );
        let rules = RuleSet::from_rules_layers(&layers).unwrap();
        let driver = rules.object("DRIVER").unwrap().driver_rules();
        assert!(driver.can_drive() && driver.one_time());
        assert!(!driver.break_mind_control());
        assert_eq!(driver.operator(), &OperatorRequirement::Any);
        let crew = rules.object("DRIVER").unwrap().crew_rules();
        assert_eq!(crew.pilot_count(1), 2);
        assert_eq!(crew.pilot_chances(), [0, 75, 30]);
        assert_eq!(crew.passenger_chances(), [-1, -1, 90]);
        assert_eq!(crew.pilot_for_side(1), Some("OP"));
        assert_eq!(crew.technician_chance(), Some(0));
        assert_eq!(rules.side_crew(0), Some(Some("OP")));
        assert_eq!(rules.side_technician(0), Some(None));
        assert!(rules.object("MISSING_PILOT").is_none());
        assert_eq!(
            rules.object("HOST").unwrap().aircraft_crashable_override(),
            Some(false)
        );
        assert_eq!(
            rules.object("OP").unwrap().aircraft_crashable_override(),
            None
        );
        let host = rules.object("HOST").unwrap().driver_rules();
        assert!(!host.can_be_driven() && !host.theft_allowed());
        assert!(!rules.country_can_be_driven("Neutral"));
        assert!(
            rules.country_can_be_driven("Child"),
            "Country Parent copy excludes CanBeDriven"
        );
        assert_eq!(
            rules.object("OP").unwrap().driver_rules(),
            &DriverTypeRules::default()
        );
        let saved: DriverTypeRules =
            serde_json::from_slice(&serde_json::to_vec(driver).unwrap()).unwrap();
        assert_eq!(&saved, driver);

        layers.push(
            RulesLayerKind::Scenario,
            IniFile::from_str("[DRIVER]\nOperator=OP,NOT_ALLOCATED\n"),
        );
        let rules = RuleSet::from_rules_layers(&layers).unwrap();
        assert_eq!(
            rules.object("DRIVER").unwrap().driver_rules().operator(),
            &OperatorRequirement::Types(vec![TechnoTypeReference {
                category: crate::rules::object_type::ObjectCategory::Infantry,
                id: "OP".into(),
            }])
        );
        assert!(rules.object("NOT_ALLOCATED").is_none());
        layers.push(
            RulesLayerKind::Scenario,
            IniFile::from_str("[DRIVER]\nOperator=none\n"),
        );
        assert_eq!(
            RuleSet::from_rules_layers(&layers)
                .unwrap()
                .object("DRIVER")
                .unwrap()
                .driver_rules()
                .operator(),
            &OperatorRequirement::None
        );
    }
}
