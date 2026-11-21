//! The Rules keys of the Firestorm ice chain (docs/ts-ice.md). gamemd still
//! constructs and reads all five, though nothing in it uses them.

use crate::rules::ini_parser::IniSection;
use crate::util::fixed_math::SimFixed;

#[derive(Debug, Clone, PartialEq)]
pub struct IceRules {
    /// `[AudioVisual] IceGrowthRate=`, minutes between growth passes, Rules+0x1650
    /// double: constructor 0x667420 1.0, ReadAudioVisual 0x66b523 (Firestorm 2.0
    /// Rules+0xDF0). Zero turns growth off.
    pub growth_rate: f64,
    /// `[AudioVisual] IceSolidifyFrameTime=`, frames before cracked ice
    /// freezes over, Rules+0x1660 int: constructor 0x667438 500, ReadAudioVisual
    /// 0x66b543 (Firestorm 2.0 Rules+0xE00).
    pub solidify_frame_time: i32,
    /// `[General] IceCrackingWeight=`, Rules+0x638 double: constructor
    /// 0x665ef8 2.0, ReadGeneral 0x66f192 (Firestorm 2.0 Rules+0x368). Kept
    /// as `Weight=` is, so the two compare alike.
    pub cracking_weight: SimFixed,
    /// `[General] IceBreakingWeight=`, Rules+0x640 double: constructor
    /// 0x665f09 4.0, ReadGeneral 0x66f1b8 (Firestorm 2.0 Rules+0x370).
    pub breaking_weight: SimFixed,
    /// `[AudioVisual] IceCrackSounds=`, the Voc list at Rules+0x648
    /// (ReadAudioVisual 0x66b550..0x66b596; Firestorm 2.0 Rules+0x378), bound
    /// with the other Rules sounds once the sound registry is loaded.
    pub crack_sounds: Vec<String>,
}

/// Constructor 0x665ef8/0x665f09 weights.
const CRACKING_WEIGHT: f64 = 2.0;
const BREAKING_WEIGHT: f64 = 4.0;

impl Default for IceRules {
    fn default() -> Self {
        Self {
            growth_rate: 1.0,
            solidify_frame_time: 500,
            cracking_weight: weight(CRACKING_WEIGHT),
            breaking_weight: weight(BREAKING_WEIGHT),
            crack_sounds: Vec::new(),
        }
    }
}

/// A double weight as `Weight=` keeps it (`object_type::reader`).
fn weight(value: f64) -> SimFixed {
    SimFixed::saturating_from_num(value as f32)
}

impl IceRules {
    /// ReadAudioVisual's two scalars, read whether or not `[General]` exists.
    pub(crate) fn read_audio_visual(&mut self, audio_visual: &IniSection) {
        self.growth_rate = audio_visual.read_double("IceGrowthRate", self.growth_rate);
        self.solidify_frame_time =
            audio_visual.read_int("IceSolidifyFrameTime", self.solidify_frame_time);
    }

    /// ReadGeneral's two weights, over the constructor's.
    pub(crate) fn read_general(&mut self, general: &IniSection) {
        self.cracking_weight = weight(general.read_double("IceCrackingWeight", CRACKING_WEIGHT));
        self.breaking_weight = weight(general.read_double("IceBreakingWeight", BREAKING_WEIGHT));
    }
}
