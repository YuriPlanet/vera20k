//! Retained TechnoType recoil configuration, original 711412 and 715269..7153DA.
//! Every reached reader copies the updated turret controls into the barrel
//! controls before reading barrel overrides. Merged INI text cannot preserve
//! that reset when a later layer has the section but omits BarrelTravel.
//!
//! Original executable comparisons: tools/voxel_oracle/recoil.json.
use crate::rules::ini_parser::IniSection;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct RecoilControl {
    travel: i32,
    compress_frames: i32,
    recover_frames: i32,
    hold_frames: i32,
}

impl Default for RecoilControl {
    fn default() -> Self {
        Self {
            travel: 2,
            compress_frames: 1,
            recover_frames: 1,
            hold_frames: 1,
        }
    }
}

impl RecoilControl {
    fn read(&mut self, section: &IniSection, keys: [&str; 4]) {
        self.travel = section.read_int(keys[0], self.travel);
        // 717A50 / 717AB0 / 717A80 clamp the three frame counts to at least1.
        self.compress_frames = section.read_int(keys[1], self.compress_frames).max(1);
        self.hold_frames = section.read_int(keys[2], self.hold_frames).max(1);
        self.recover_frames = section.read_int(keys[3], self.recover_frames).max(1);
    }

    /// Native field order: distance, compression, recovery, hold (frames).
    pub(crate) fn parameters(self) -> [i32; 4] {
        [
            self.travel,
            self.compress_frames,
            self.recover_frames,
            self.hold_frames,
        ]
    }
}

#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize,
)]
pub struct RecoilConfig {
    enabled: bool,
    turret: RecoilControl,
    barrel: RecoilControl,
}

impl RecoilConfig {
    pub(crate) fn from_ini_section(section: &IniSection) -> Self {
        let mut result = Self::default();
        result.apply_pass(section);
        result
    }

    pub(crate) fn apply_pass(&mut self, section: &IniSection) {
        self.enabled = section.read_bool("TurretRecoil", self.enabled);
        self.turret.read(
            section,
            [
                "TurretTravel",
                "TurretCompressFrames",
                "TurretHoldFrames",
                "TurretRecoverFrames",
            ],
        );
        self.barrel = self.turret;
        self.barrel.read(
            section,
            [
                "BarrelTravel",
                "BarrelCompressFrames",
                "BarrelHoldFrames",
                "BarrelRecoverFrames",
            ],
        );
    }

    pub(crate) fn enabled(self) -> bool {
        self.enabled
    }
    pub(crate) fn controls(self) -> [RecoilControl; 2] {
        [self.turret, self.barrel]
    }
}

#[cfg(test)]
#[path = "recoil_tests.rs"]
mod tests;
