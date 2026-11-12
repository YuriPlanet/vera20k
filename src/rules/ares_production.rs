//! Retained optional Ares production configuration, projected by the rules
//! registry: per-type build time (`BuildTime.*`), the FactoryPlant discount
//! scale (`FactoryPlant.Multiplier`) and cloning (`Cloneable`, `ClonedAt`,
//! `ClonedAs`, BuildingType `CloningFacility`).
//! Source: installed Ares TypeExt reader1003DFB0 and constructor1003C710,
//! BuildingTypeExt reader1000DD40, and the post-read validation
//! ValidateThings100677B0 (hook687C16). `ClonedAt` is the Find-only
//! BuildingType vector10010850 and `ClonedAs` the Find-only TechnoType
//! pointer1003FA40, both resolved by the registry.

use crate::rules::ini_parser::IniSection;
use crate::rules::superweapon_type::TechnoTypeReference;
use crate::util::native_x87::NativeF64Bits;

/// TypeExt+2E4..+2F4 (cloning), +548 (FactoryPlant.Multiplier) and
/// +5A8..+5F8 (BuildTime.*).
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub(crate) struct ProductionTypeRules {
    cloneable: bool,
    /// Ordered BuildingType IDs; ValidateThings drops any with `Factory=`.
    cloned_at: Vec<String>,
    cloned_as: Option<TechnoTypeReference>,
    factory_plant_multiplier: NativeF64Bits,
    build_time_speed: Option<NativeF64Bits>,
    build_time_cost: Option<i32>,
    build_time_low_power_penalty: Option<NativeF64Bits>,
    build_time_min_low_power: Option<NativeF64Bits>,
    build_time_max_low_power: Option<NativeF64Bits>,
    build_time_multiple_factory: Option<NativeF64Bits>,
    /// BuildingTypeExt+125 `CloningFacility` (constructor false); cleared by
    /// ValidateThings on a type with `Factory=`.
    cloning_facility: bool,
}

impl Default for ProductionTypeRules {
    fn default() -> Self {
        // Constructor1003C710: Cloneable true, no cloning sources or
        // replacement type, FactoryPlant.Multiplier 1.0, every BuildTime.*
        // unset.
        Self {
            cloneable: true,
            cloned_at: Vec::new(),
            cloned_as: None,
            factory_plant_multiplier: NativeF64Bits::ONE,
            build_time_speed: None,
            build_time_cost: None,
            build_time_low_power_penalty: None,
            build_time_min_low_power: None,
            build_time_max_low_power: None,
            build_time_multiple_factory: None,
            cloning_facility: false,
        }
    }
}

impl ProductionTypeRules {
    /// One reached TypeExt LoadFromINI pass. `cloned_at` is a reached
    /// vector read (replaces), `cloned_as` a pointer read (`none` clears,
    /// unknown retains). The doubles are Valueable<double>1000EB00 and
    /// Nullable<double>10008CE0, BuildTime.Cost Nullable<int>10008CC0.
    pub(crate) fn read_pass(
        &mut self,
        section: &IniSection,
        cloned_at: Option<Vec<String>>,
        cloned_as: Option<Option<TechnoTypeReference>>,
    ) {
        let _ares = crate::rules::ares_switch::ares_reads();
        self.cloneable = section.read_ares_bool("Cloneable", self.cloneable);
        if let Some(cloned_at) = cloned_at {
            self.cloned_at = cloned_at;
        }
        if let Some(cloned_as) = cloned_as {
            self.cloned_as = cloned_as;
        }
        self.factory_plant_multiplier =
            section.read_ares_double_bits("FactoryPlant.Multiplier", self.factory_plant_multiplier);
        self.build_time_speed =
            section.read_ares_nullable_double_bits("BuildTime.Speed", self.build_time_speed);
        self.build_time_cost =
            section.read_ares_nullable_int("BuildTime.Cost", self.build_time_cost);
        self.build_time_low_power_penalty = section.read_ares_nullable_double_bits(
            "BuildTime.LowPowerPenalty",
            self.build_time_low_power_penalty,
        );
        self.build_time_min_low_power = section
            .read_ares_nullable_double_bits("BuildTime.MinLowPower", self.build_time_min_low_power);
        self.build_time_max_low_power = section
            .read_ares_nullable_double_bits("BuildTime.MaxLowPower", self.build_time_max_low_power);
        self.build_time_multiple_factory = section.read_ares_nullable_double_bits(
            "BuildTime.MultipleFactory",
            self.build_time_multiple_factory,
        );
    }

    /// BuildingTypeExt reader1000DD40: Valueable<bool>1000EBF0.
    pub(crate) fn read_building_pass(&mut self, section: &IniSection) {
        let _ares = crate::rules::ares_switch::ares_reads();
        self.cloning_facility = section.read_ares_bool("CloningFacility", self.cloning_facility);
    }

    /// ValidateThings100677B0, once the INIs are read: a `ClonedAt` source
    /// with `Factory=` is dropped, and a `Factory=` building loses
    /// `CloningFacility`.
    pub(crate) fn validate(&mut self, factory: bool, source_has_factory: impl Fn(&str) -> bool) {
        self.cloned_at.retain(|source| !source_has_factory(source));
        if factory {
            self.cloning_facility = false;
        }
    }

    pub(crate) fn cloning_facility(&self) -> bool {
        self.cloning_facility
    }

    pub(crate) fn cloneable(&self) -> bool {
        self.cloneable
    }

    pub(crate) fn cloned_at(&self) -> &[String] {
        &self.cloned_at
    }

    pub(crate) fn cloned_as(&self) -> Option<&TechnoTypeReference> {
        self.cloned_as.as_ref()
    }

    pub(crate) fn factory_plant_multiplier(&self) -> NativeF64Bits {
        self.factory_plant_multiplier
    }

    pub(crate) fn build_time_speed(&self) -> Option<NativeF64Bits> {
        self.build_time_speed
    }

    pub(crate) fn build_time_cost(&self) -> Option<i32> {
        self.build_time_cost
    }

    pub(crate) fn build_time_low_power_penalty(&self) -> Option<NativeF64Bits> {
        self.build_time_low_power_penalty
    }

    pub(crate) fn build_time_min_low_power(&self) -> Option<NativeF64Bits> {
        self.build_time_min_low_power
    }

    pub(crate) fn build_time_max_low_power(&self) -> Option<NativeF64Bits> {
        self.build_time_max_low_power
    }

    pub(crate) fn build_time_multiple_factory(&self) -> Option<NativeF64Bits> {
        self.build_time_multiple_factory
    }
}
