//! Phobos warp animation lists (`Ext/TechnoType/Body.cpp:857-860`, fields
//! `Ext/TechnoType/Body.h:121-124`, constructed empty at `Body.h:524-525`).
//!
//! A teleporting techno plays a random animation from these per-type lists at
//! its warp endpoints, with the Chronoshift pair preferred on a Chronosphere
//! warp and the `[General] WarpIn`/`WarpOut` single animation as the last
//! fallback. Consumers:
//! `TeleportLocomotionClass_ILocomotion_Process_WarpoutAnim @ 0x7193F6`,
//! `_WarpInAnim @ 0x719742`, `_ChronoshiftWarpOutAnim @ 0x719827` and
//! `_ChronoshiftWarpInAnim @ 0x719B1E`
//! (`Ext/TechnoType/Hooks.Teleport.cpp:13-140`), ported at
//! `sim::movement::teleport_movement::Simulation::teleport_warp_anim`.
//!
//! ## Dependency rules
//! - Part of rules/ — no dependency on sim/, render/, ui/, etc.

use crate::rules::ini_parser::IniSection;

/// The four `ValueableVector<AnimTypeClass*>` warp lists.
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct WarpAnimRules {
    /// `WarpOut=`, played at the ordinary departure point.
    pub warp_out: Vec<String>,
    /// `WarpIn=`, played at the ordinary arrival point.
    pub warp_in: Vec<String>,
    /// `Chronoshift.WarpOut=`, preferred over `WarpOut` on a Chronosphere
    /// departure.
    pub chronoshift_warp_out: Vec<String>,
    /// `Chronoshift.WarpIn=`, preferred over `WarpIn` on a Chronosphere
    /// arrival.
    pub chronoshift_warp_in: Vec<String>,
}

impl WarpAnimRules {
    /// Reads in `TechnoTypeExt::LoadFromINIFile` order. The native
    /// `ValueableVector` reader drops an unresolvable name at read time; the
    /// tokens stay here until a RuleSet resolves them, and the consumer drops
    /// the ones it cannot resolve before drawing.
    pub fn from_section(section: &IniSection) -> Self {
        use crate::rules::native_processing::read_valueable_type_vector;
        let keep = |name: &str| Some(name.to_owned());
        Self {
            warp_out: read_valueable_type_vector(section, "WarpOut", &[], keep),
            warp_in: read_valueable_type_vector(section, "WarpIn", &[], keep),
            chronoshift_warp_out: read_valueable_type_vector(
                section,
                "Chronoshift.WarpOut",
                &[],
                keep,
            ),
            chronoshift_warp_in: read_valueable_type_vector(
                section,
                "Chronoshift.WarpIn",
                &[],
                keep,
            ),
        }
    }
}
