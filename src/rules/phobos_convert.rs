//! Phobos `Convert.Undeploy` TechnoType key: an infantryman that finishes its
//! Undeploy sequence converts to another type once.
//!
//! Reader `Ext/TechnoType/Body.cpp:1014` (`Valueable<TechnoTypeClass*>`),
//! declaration `Ext/TechnoType/Body.h:249`, constructor null `:681`.
//! Consumers: `InfantryClass_DoingAI_DeployConvert_Undeploy @ 0x520B99`
//! (`Ext/Infantry/Hooks.cpp:36-50`) with the `HasUndeployConverted` latch and
//! its reset at `0x520E75` (`:52-66`); the deploy-side twin reads
//! `Convert.Deploy` and is a separate chain.
//!
//! ## Dependency rules
//! - Part of rules/ — no dependency on sim/, render/, ui/, etc.

use crate::rules::ini_parser::IniSection;

/// The `Convert.*` self-conversion triggers this chain reads.
#[derive(Debug, Clone, PartialEq, Default, serde::Serialize, serde::Deserialize)]
pub struct PhobosConvert {
    /// `Convert.Deploy=`: the type an infantryman converts to when its
    /// Deploy sequence completes (`Ext/TechnoType/Body.cpp:1230`); absent or
    /// `none` disables. The same key feeds the Ares Unit simple-deployer arm.
    pub deploy: Option<String>,
    /// `Convert.Undeploy=`: the type an infantryman converts to when its
    /// Undeploy sequence completes; absent or `none` disables.
    pub undeploy: Option<String>,
    /// `Convert.HumanToComputer=`: the type a Foot converts to when its
    /// owner changes from a human house to a computer house
    /// (`Ext/TechnoType/Body.cpp:1015`).
    pub human_to_computer: Option<String>,
    /// `Convert.ComputerToHuman=`: the same for a computer-to-human change
    /// (`Ext/TechnoType/Body.cpp:1016`).
    pub computer_to_human: Option<String>,
    /// `Convert.ResetMindControl=`: `Nullable<bool>`, unset falls back to the
    /// `[General]` key; rebuilds the capture-manager limits from the new
    /// type's weapons on conversion (`Ext/TechnoType/Body.cpp:1017`).
    pub reset_mind_control: Option<bool>,
    /// `Ammo.AutoConvertType=`: the type a Foot converts to when its ammo
    /// enters the band (`Ext/TechnoType/Body.cpp:1155`, `Nullable<TechnoType*>`).
    pub ammo_target: Option<String>,
    /// `Ammo.AutoConvertMinimumAmount=`, default -1 (`Body.h:794`).
    pub ammo_min: i32,
    /// `Ammo.AutoConvertMaximumAmount=`, default -1 (`Body.h:795`).
    pub ammo_max: i32,
}

impl PhobosConvert {
    pub fn from_section(section: &IniSection) -> Self {
        Self {
            deploy: section
                .read_type_name("Convert.Deploy", 0x800)
                .filter(|name| !name.eq_ignore_ascii_case("none"))
                .map(str::to_owned),
            undeploy: section
                .read_type_name("Convert.Undeploy", 0x800)
                .filter(|name| !name.eq_ignore_ascii_case("none"))
                .map(str::to_owned),
            human_to_computer: section
                .read_type_name("Convert.HumanToComputer", 0x800)
                .filter(|name| !name.eq_ignore_ascii_case("none"))
                .map(str::to_owned),
            computer_to_human: section
                .read_type_name("Convert.ComputerToHuman", 0x800)
                .filter(|name| !name.eq_ignore_ascii_case("none"))
                .map(str::to_owned),
            reset_mind_control: section.read_phobos_nullable_bool("Convert.ResetMindControl", None),
            ammo_target: section
                .read_type_name("Ammo.AutoConvertType", 0x800)
                .filter(|name| !name.eq_ignore_ascii_case("none"))
                .map(str::to_owned),
            ammo_min: section.read_ares_int("Ammo.AutoConvertMinimumAmount", -1),
            ammo_max: section.read_ares_int("Ammo.AutoConvertMaximumAmount", -1),
        }
    }
}
