//! Retained optional Ares stealth, power-source, radar-jammer and sensor-array
//! configuration, projected by the rules registry.
//! Source: installed Ares TypeExt reader1003DFB0 (RadarJamRadius, then
//! PoweredBy, later the cloak and sensor block), its constructor and the
//! RulesExt constructor1002A360 and readers1002AB60/1002A800.
//! CloakSound/DecloakSound are fixed-catalog sound references and bind with
//! the other TechnoType sounds; no resource loading belongs here.

use crate::rules::ini_parser::{IniFile, IniSection};
use crate::util::native_x87::NativeF64Bits;

/// TypeExt+278 RadarJamRadius, +280 PoweredBy, +1F8 Cloakable.Powered,
/// +1F9 Cloakable.Deployed, +1FC/+200 nullable Cloakable.Stages and +204
/// SensorArray.Warn.
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub(crate) struct CloakPowerTypeRules {
    radar_jam_radius: i32,
    /// Ordered BuildingType IDs; duplicates survive the native vector read.
    powered_by: Vec<String>,
    cloak_powered: bool,
    cloak_deployed: bool,
    cloak_stages: Option<i32>,
    sensor_array_warn: bool,
}

impl Default for CloakPowerTypeRules {
    fn default() -> Self {
        // TypeExt constructor: RadarJamRadius0, empty PoweredBy, both cloak
        // conditions false, Stages unset, SensorArray.Warn true.
        Self {
            radar_jam_radius: 0,
            powered_by: Vec::new(),
            cloak_powered: false,
            cloak_deployed: false,
            cloak_stages: None,
            sensor_array_warn: true,
        }
    }
}

impl CloakPowerTypeRules {
    /// The registry supplies the Find-only BuildingType vector read10010850
    /// of `PoweredBy`: a reached list replaces, a failed read retains.
    pub(crate) fn read_pass(&mut self, section: &IniSection, powered_by: Option<Vec<String>>) {
        let _ares = crate::rules::ares_switch::ares_reads();
        // Valueable<int>1000A570.
        self.radar_jam_radius = section.read_ares_int("RadarJamRadius", self.radar_jam_radius);
        if let Some(powered_by) = powered_by {
            self.powered_by = powered_by;
        }
        // Valueable<bool> then Nullable<int>10008CC0.
        self.cloak_powered = section.read_ares_bool("Cloakable.Powered", self.cloak_powered);
        self.cloak_deployed = section.read_ares_bool("Cloakable.Deployed", self.cloak_deployed);
        self.cloak_stages = section.read_ares_nullable_int("Cloakable.Stages", self.cloak_stages);
        self.sensor_array_warn = section.read_ares_bool("SensorArray.Warn", self.sensor_array_warn);
    }

    /// RadarJamRadius in cells; nonzero types run a JammerClass.
    pub(crate) fn radar_jam_radius(&self) -> i32 {
        self.radar_jam_radius
    }

    /// PoweredBy BuildingType IDs; nonempty types run a PoweredUnitClass.
    pub(crate) fn powered_by(&self) -> &[String] {
        &self.powered_by
    }

    /// A Building cloaks itself only while online (CloakDisallowed10043A40).
    pub(crate) fn cloak_powered(&self) -> bool {
        self.cloak_powered
    }

    /// Infantry cloaks itself only while deployed (CloakDisallowed10043A40).
    pub(crate) fn cloak_deployed(&self) -> bool {
        self.cloak_deployed
    }

    /// Cloakable.Stages, else `[General]CloakingStages`.
    pub(crate) fn cloaking_stages(&self, general: i32) -> i32 {
        self.cloak_stages.unwrap_or(general)
    }

    pub(crate) fn sensor_array_warn(&self) -> bool {
        self.sensor_array_warn
    }
}

/// RulesExt+40/+44 nullable CloakHeight, +50/+58/+60 the three dim factors
/// and +120/+128 the detection speak delays.
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct CloakPowerGlobalRules {
    cloak_height: Option<i32>,
    deactivate_dim: [NativeF64Bits; 3],
    speak_delay: [NativeF64Bits; 2],
}

/// Index of [`CloakPowerGlobalRules::deactivate_dim`]'s factors.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeactivateDim {
    Powered = 0,
    Emp = 1,
    Operator = 2,
}

/// Index of [`CloakPowerGlobalRules::speak_delay`]'s delays.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DetectedSpeech {
    Stealth = 0,
    Subterranean = 1,
}

impl Default for CloakPowerGlobalRules {
    fn default() -> Self {
        // Constructor1002A360: CloakHeight unset; the dim factors 0.5, 0.8
        // and 0.65; both speak delays 0.0.
        Self {
            cloak_height: None,
            deactivate_dim: [
                NativeF64Bits::from_bits(0.5f64.to_bits()),
                NativeF64Bits::from_bits(0.8f64.to_bits()),
                NativeF64Bits::from_bits(0.65f64.to_bits()),
            ],
            speak_delay: [NativeF64Bits::from_bits(0); 2],
        }
    }
}

impl CloakPowerGlobalRules {
    /// LoadBeforeTypeData1002AB60: Valueable<double>1000EB00 in AudioVisual.
    pub(crate) fn read_before_type_data(&mut self, pass: &IniFile) {
        let _ares = crate::rules::ares_switch::ares_reads();
        let Some(section) = pass.section("AudioVisual") else {
            return;
        };
        for (slot, key) in [
            "DeactivateDimPowered",
            "DeactivateDimEMP",
            "DeactivateDimOperator",
        ]
        .into_iter()
        .enumerate()
        {
            self.deactivate_dim[slot] =
                section.read_ares_double_bits(key, self.deactivate_dim[slot]);
        }
        for (slot, key) in ["StealthSpeakDelay", "SubterraneanSpeakDelay"]
            .into_iter()
            .enumerate()
        {
            self.speak_delay[slot] = section.read_ares_double_bits(key, self.speak_delay[slot]);
        }
    }

    /// LoadAfterTypeData1002A800: General's Nullable<int>10008CC0, after
    /// the AudioVisual DecloakSound that the sound binder owns.
    pub(crate) fn read_after_type_data(&mut self, pass: &IniFile) {
        let _ares = crate::rules::ares_switch::ares_reads();
        if let Some(section) = pass.section("General") {
            self.cloak_height = section.read_ares_nullable_int("CloakHeight", self.cloak_height);
        }
    }

    /// CloakHeight, else `[General]HoverHeight` (CloakDisallowed10043A40).
    pub(crate) fn cloak_height(&self, hover_height: i32) -> i32 {
        self.cloak_height.unwrap_or(hover_height)
    }

    pub fn deactivate_dim(&self, kind: DeactivateDim) -> f64 {
        f64::from_bits(self.deactivate_dim[kind as usize].bits())
    }

    pub(crate) fn speak_delay(&self, kind: DetectedSpeech) -> f64 {
        f64::from_bits(self.speak_delay[kind as usize].bits())
    }
}
