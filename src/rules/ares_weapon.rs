//! Ares 3.0 WeaponTypeExt weapon visuals: coloured rad beams, electric bolts
//! and waves, wave reversal and detached railguns.
//!
//! Defaults are the ExtData constructor's (Ares.dll 100548B0). The keys are
//! read by `WeaponTypeExt::ExtData::LoadFromINIFile` (10055520), which runs at
//! the end of every WeaponTypeClass::ReadINI pass (hooks 0x7729B0, 0x7729C7,
//! 0x7729D6); the projected section retains those passes. `Initialize`
//! (first load) seeds `Wave.ReverseAgainstVehicles` with IsMagBeam.
//! `Bolt.ParticleSystem` is a type lookup and is resolved by the ordered
//! Rules processor ([`crate::rules::native_processing`]).

use crate::rules::ini_parser::IniSection;
use crate::util::native_x87::NativeF64Bits;

/// The `Wave.ReverseAgainst*` slots (ExtData +0x5C..+0x60), chosen by the
/// target's What_Am_I in `IsWaveReversedAgainst`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WaveReverseTarget {
    Vehicle,
    Aircraft,
    Building,
    Infantry,
    Other,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AresWeaponVisuals {
    /// `IsDetachedRailgun` (+0x08): FireAt always runs the railgun arm and
    /// never stores the system in Techno+0x314.
    pub(crate) is_detached_railgun: bool,
    /// `Beam.Color` (+0x09, nullable).
    pub(crate) beam_color: Option<[u8; 3]>,
    /// `Beam.Duration` (+0x10, 15) and `Beam.Amplitude` (+0x18, 40.0).
    pub(crate) beam_duration: i32,
    pub(crate) beam_amplitude: NativeF64Bits,
    /// `Beam.IsHouseColor` (+0x20): keep the firing House's colour.
    pub(crate) beam_is_house_color: bool,
    /// `Bolt.Color1..3` (+0x21, +0x25, +0x29, nullable).
    pub(crate) bolt_colors: [Option<[u8; 3]>; 3],
    /// `Bolt.ParticleSystem` (+0x30, nullable): `Some(None)` is an explicit
    /// `none`; `None` falls back to `[CombatDamage] DefaultSparkSystem`.
    pub(crate) bolt_particle_system: Option<Option<String>>,
    /// `Wave.IsHouseColor` (+0x38).
    pub(crate) wave_is_house_color: bool,
    /// `Wave.Intensity` (+0x3C) and `Wave.Color` (+0x4C), three ints each.
    pub(crate) wave_intensity: Option<[i32; 3]>,
    pub(crate) wave_color: Option<[i32; 3]>,
    /// `Wave.ReverseAgainstVehicles/Aircraft/Buildings/Infantry/Others`.
    wave_reverse: [bool; 5],
}

impl AresWeaponVisuals {
    /// The constructor's values; `is_mag_beam` is Initialize's seed of the
    /// vehicle reversal.
    fn constructed(is_mag_beam: bool) -> Self {
        Self {
            is_detached_railgun: false,
            beam_color: None,
            beam_duration: 15,
            beam_amplitude: NativeF64Bits::from_bits(40.0_f64.to_bits()),
            beam_is_house_color: false,
            bolt_colors: [None; 3],
            bolt_particle_system: None,
            wave_is_house_color: false,
            wave_intensity: None,
            wave_color: None,
            wave_reverse: [is_mag_beam, false, false, false, false],
        }
    }

    /// LoadFromINIFile 10055520's reads of these keys, in its order. The
    /// vehicle reversal is seeded from this projection's IsMagBeam; native
    /// seeds it from the value after the first pass that read the weapon.
    pub(crate) fn read(section: &IniSection, is_mag_beam: bool) -> Self {
        let mut visuals = Self::constructed(is_mag_beam);
        visuals.is_detached_railgun =
            section.read_ares_bool("IsDetachedRailgun", visuals.is_detached_railgun);
        visuals.beam_duration = section.read_ares_int("Beam.Duration", visuals.beam_duration);
        visuals.beam_amplitude =
            section.read_ares_double_bits("Beam.Amplitude", visuals.beam_amplitude);
        visuals.beam_is_house_color =
            section.read_ares_bool("Beam.IsHouseColor", visuals.beam_is_house_color);
        visuals.beam_color = section.read_ares_nullable_color("Beam.Color", visuals.beam_color);
        visuals.wave_is_house_color =
            section.read_ares_bool("Wave.IsHouseColor", visuals.wave_is_house_color);
        visuals.wave_intensity =
            section.read_ares_nullable_coord3("Wave.Intensity", visuals.wave_intensity);
        visuals.wave_color = section.read_ares_nullable_coord3("Wave.Color", visuals.wave_color);
        for (slot, key) in [
            "Wave.ReverseAgainstVehicles",
            "Wave.ReverseAgainstAircraft",
            "Wave.ReverseAgainstBuildings",
            "Wave.ReverseAgainstInfantry",
            "Wave.ReverseAgainstOthers",
        ]
        .into_iter()
        .enumerate()
        {
            visuals.wave_reverse[slot] = section.read_ares_bool(key, visuals.wave_reverse[slot]);
        }
        for (slot, key) in ["Bolt.Color1", "Bolt.Color2", "Bolt.Color3"]
            .into_iter()
            .enumerate()
        {
            visuals.bolt_colors[slot] =
                section.read_ares_nullable_color(key, visuals.bolt_colors[slot]);
        }
        visuals
    }

    /// `IsWaveReversedAgainst` (Ares.dll 100553F0, called by
    /// WaveClass_Update_Beam 10057290): the slot of the target's class.
    pub(crate) fn wave_reversed_against(&self, target: WaveReverseTarget) -> bool {
        self.wave_reverse[target as usize]
    }

    /// `GetBeamColor` (Ares.dll 10055210): `Beam.Color`, else
    /// ChronoBeamColor for a temporal RadBeam/RadEruption, else RadColor.
    pub(crate) fn beam_color(
        &self,
        temporal_beam: bool,
        chrono_beam_color: [u8; 3],
        rad_color: [u8; 3],
    ) -> [u8; 3] {
        self.beam_color.unwrap_or(if temporal_beam {
            chrono_beam_color
        } else {
            rad_color
        })
    }

    /// `GetWaveColorData` 10055270 for a wave of a weapon with these flags:
    /// the kind's defaults (laser, Sonic, MagBeam at 100A1F98/100A1FB0/
    /// 100A1FC8) under `Wave.Intensity`/`Wave.Color`, then the owner's House
    /// colour for `Wave.IsHouseColor` when the wave has an owner. `None` when
    /// all six values are zero (the data is disabled).
    pub fn wave_color_data(
        &self,
        is_mag_beam: bool,
        is_sonic: bool,
        owner_house_rgb: Option<[u8; 3]>,
    ) -> Option<([i32; 3], [i32; 3])> {
        const INTENSITY: [[i32; 3]; 3] = [[0, 0, 0], [0, 256, 256], [128, 0, 1024]];
        const COLOR: [[i32; 3]; 3] = [[64, 0, 96], [0, 0, 0], [0, 0, 0]];
        let kind = if is_mag_beam {
            2
        } else {
            usize::from(is_sonic)
        };
        let intensity = self.wave_intensity.unwrap_or(INTENSITY[kind]);
        let mut color = self.wave_color.unwrap_or(COLOR[kind]);
        if self.wave_is_house_color
            && let Some(rgb) = owner_house_rgb
        {
            color = rgb.map(i32::from);
        }
        (intensity.iter().chain(&color).any(|value| *value != 0)).then_some((intensity, color))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rules::ini_parser::IniFile;

    fn read(ini: &str, is_mag_beam: bool) -> AresWeaponVisuals {
        let file = IniFile::from_str(ini);
        AresWeaponVisuals::read(file.section("W").unwrap(), is_mag_beam)
    }

    #[test]
    fn keys_keep_constructor_defaults_and_nullable_presence() {
        let visuals = read("[W]\nBeam.Color=1,2\nWave.Color=5,6\n", true);
        assert_eq!(visuals, AresWeaponVisuals::constructed(true));
        assert!(visuals.wave_reversed_against(WaveReverseTarget::Vehicle));
        let visuals = read(
            "[W]\nBeam.Color=$10, 20 ,0ah\nBeam.Duration=30\nBeam.Amplitude=50%\n\
             Bolt.Color2=255,0,256\nBolt.Color3=1,2,3\nWave.Intensity=1,-2,3\n\
             Wave.ReverseAgainstVehicles=no\nWave.ReverseAgainstOthers=yes\n",
            true,
        );
        assert_eq!(visuals.beam_color, Some([10, 20, 10]));
        assert_eq!(
            (visuals.beam_duration, visuals.beam_amplitude.bits()),
            (30, 0.5_f64.to_bits())
        );
        assert_eq!(visuals.bolt_colors, [None, None, Some([1, 2, 3])]);
        assert_eq!(visuals.wave_intensity, Some([1, -2, 3]));
        assert!(!visuals.wave_reversed_against(WaveReverseTarget::Vehicle));
        assert!(visuals.wave_reversed_against(WaveReverseTarget::Other));
    }

    /// The ordered processor's Find: a listed system resolves, `none` stores
    /// null, and a name no pass registered keeps the earlier pass's value.
    #[test]
    fn bolt_particle_system_finds_existing_types_across_passes() {
        use crate::rules::native_processing::{RulesLayerKind, RulesLayerStack};
        use crate::rules::ruleset::RuleSet;

        let mut layers = RulesLayerStack::new(IniFile::from_str(
            "[InfantryTypes]\n0=PROBE\n[PROBE]\nPrimary=ZAP\nSecondary=OFF\n\
             [ParticleSystems]\n0=SPARKY\n[SPARKY]\n\
             [ZAP]\nBolt.ParticleSystem=sparky\n[OFF]\nBolt.ParticleSystem=none\n",
        ));
        layers.push(
            RulesLayerKind::Scenario,
            IniFile::from_str("[ZAP]\nBolt.ParticleSystem=Missing\n"),
        );
        let rules = RuleSet::from_rules_layers(&layers).unwrap();
        let system = |weapon| {
            rules
                .weapon(weapon)
                .unwrap()
                .ares_visuals
                .bolt_particle_system
                .clone()
        };
        assert_eq!(system("ZAP"), Some(Some("SPARKY".to_string())));
        assert_eq!(system("OFF"), Some(None));
    }

    #[test]
    fn wave_color_data_takes_kind_defaults_house_color_and_disables_on_zero() {
        let mut visuals = AresWeaponVisuals::constructed(false);
        assert_eq!(
            visuals.wave_color_data(true, false, Some([9, 9, 9])),
            Some(([128, 0, 1024], [0, 0, 0]))
        );
        visuals.wave_is_house_color = true;
        assert_eq!(
            visuals.wave_color_data(false, true, Some([1, 2, 3])),
            Some(([0, 256, 256], [1, 2, 3]))
        );
        visuals.wave_intensity = Some([0; 3]);
        assert_eq!(visuals.wave_color_data(false, true, None), None);
    }
}
