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
//!
//! The same reader holds the custom Ivan bomb keys ([`AresIvanBomb`]), whose
//! consumers are the bomb owner (`sim::bomb`); `IvanBomb.Warhead` resolves
//! in the ordered processor and the two sounds bind with the other Rules
//! sounds.

use crate::rules::ini_parser::IniSection;
use crate::util::native_x87::NativeF64Bits;

/// Ares WeaponTypeExt custom Ivan bombs (ExtData +0x68..+0xA8, constructor
/// 100548B0). `PlantBomb` (Ares.dll 10055AE0) remembers the planting weapon
/// on its bomb (`WeaponTypeExt::BombExt`), and every bomb hook reads these
/// settings through it. The two `IvanBomb.CanDetonate*` keys gate the
/// player's Detonate action, which VERA does not represent; they are read
/// and consumed by nothing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AresIvanBomb {
    /// `IvanBomb.DeathBomb` (+0x68): a bomb planted on a victim the planter's
    /// house does not count an ally is a death bomb (BombClass+0x30 = 1): it
    /// has no fuse and goes off only with its carrier.
    pub(crate) death_bomb: bool,
    /// `IvanBomb.DeathBombOnAllies` (+0x69): the same for an allied victim.
    pub(crate) death_bomb_on_allies: bool,
    /// `IvanBomb.DestroysBridges` (+0x6A, yes): a `BridgeRepairHut=`
    /// carrier's blast drops its bridge (BombClass_Detonate3, hook 0x438879).
    pub(crate) destroys_bridges: bool,
    /// `IvanBomb.Detachable` (+0x6B, yes): an Engineer may defuse it (hooks
    /// 0x6FCBAD, 0x51E488).
    pub(crate) detachable: bool,
    /// `IvanBomb.Damage` (+0x6C), else `[CombatDamage] IvanDamage`.
    pub(crate) damage: Option<i32>,
    /// `IvanBomb.Delay` (+0x74), else `[CombatDamage] IvanTimedDelay`.
    pub(crate) delay: Option<i32>,
    /// `IvanBomb.FlickerRate` (+0x9C), else `IvanIconFlickerRate`.
    pub(crate) flicker_rate: Option<i32>,
    /// `IvanBomb.TickingSound` (+0x7C) and `IvanBomb.AttachSound` (+0x84),
    /// NullableIdx<VocClass>, else `[AudioVisual] BombTickingSound` /
    /// `BombAttachSound`; `Some(None)` is an explicit silence. Bound against
    /// the fixed sound catalog by `RuleSet::bind_type_sound_references`.
    pub(crate) ticking_sound: Option<Option<String>>,
    pub(crate) attach_sound: Option<Option<String>>,
    /// `IvanBomb.Warhead` (+0x8C), a Nullable<WarheadTypeClass*> whose Find
    /// never allocates, else `IvanWarhead`; `Some(None)` is an explicit none.
    pub(crate) warhead: Option<Option<String>>,
    /// `IvanBomb.Image` (+0x94), Nullable<SHPStruct*> (`read<SHPStruct*>`
    /// 10040640): each pass's `<value>.shp` (at most 255 bytes), in pass
    /// order. Native keeps the last one that loads; the app, which owns the
    /// art, picks it, else BOMBCURS.SHP.
    pub(crate) image: Vec<String>,
    /// `IvanBomb.CanDetonateTimeBomb` (+0xA4) and `.CanDetonateDeathBomb`
    /// (+0xA6), Nullable<bool> over Rules `CanDetonateTimeBomb`/
    /// `CanDetonateDeathBomb` (`TechnoExt::CanDetonateAttachedBomb`).
    can_detonate: [Option<bool>; 2],
    /// `IvanBomb.DetonateOnSell` (+0xA8, yes): a bomb on a sold object goes
    /// off (`WeaponTypeExt::OnSell` 10055A80).
    pub(crate) detonate_on_sell: bool,
}

impl AresIvanBomb {
    /// The constructor's values (100548B0: +0x68 = 0x01010000, +0xA8 = 1),
    /// also what a bomb without a planting weapon reads.
    pub(crate) const CONSTRUCTED: Self = Self {
        death_bomb: false,
        death_bomb_on_allies: false,
        destroys_bridges: true,
        detachable: true,
        damage: None,
        delay: None,
        flicker_rate: None,
        ticking_sound: None,
        attach_sound: None,
        warhead: None,
        image: Vec::new(),
        can_detonate: [None, None],
        detonate_on_sell: true,
    };

    /// The `IvanBomb.Image` file names in pass order, for the art owner.
    pub fn image_names(&self) -> &[String] {
        &self.image
    }

    /// `TechnoExt::CanDetonateAttachedBomb @ 100433A0`'s keys: a death bomb's
    /// `IvanBomb.CanDetonateDeathBomb` (+0xA6), else
    /// `IvanBomb.CanDetonateTimeBomb` (+0xA4), each over its Rules key.
    pub(crate) fn can_detonate(
        &self,
        death_bomb: bool,
        rules_time: bool,
        rules_death: bool,
    ) -> bool {
        if death_bomb {
            self.can_detonate[1].unwrap_or(rules_death)
        } else {
            self.can_detonate[0].unwrap_or(rules_time)
        }
    }

    /// LoadFromINIFile 10055520's reads of these keys, in its order. The
    /// warhead and the two sounds are bound by their own owners.
    pub(crate) fn read(section: &IniSection) -> Self {
        let _ares = crate::rules::ares_switch::ares_reads();
        let mut bomb = Self::CONSTRUCTED;
        bomb.death_bomb = section.read_ares_bool("IvanBomb.DeathBomb", bomb.death_bomb);
        bomb.death_bomb_on_allies =
            section.read_ares_bool("IvanBomb.DeathBombOnAllies", bomb.death_bomb_on_allies);
        bomb.destroys_bridges =
            section.read_ares_bool("IvanBomb.DestroysBridges", bomb.destroys_bridges);
        bomb.detachable = section.read_ares_bool("IvanBomb.Detachable", bomb.detachable);
        bomb.damage = section.read_ares_nullable_int("IvanBomb.Damage", bomb.damage);
        bomb.delay = section.read_ares_nullable_int("IvanBomb.Delay", bomb.delay);
        bomb.flicker_rate =
            section.read_ares_nullable_int("IvanBomb.FlickerRate", bomb.flicker_rate);
        bomb.image =
            section.read_string_with("IvanBomb.Image", 2048, Vec::new(), |mut images, name| {
                // `_snprintf_s(flag, _TRUNCATE, "%s.shp")` into char[256].
                let mut file = format!("{name}.shp");
                let mut end = file.len().min(255);
                while !file.is_char_boundary(end) {
                    end -= 1;
                }
                file.truncate(end);
                images.push(file);
                images
            });
        for (slot, key) in [
            "IvanBomb.CanDetonateTimeBomb",
            "IvanBomb.CanDetonateDeathBomb",
        ]
        .into_iter()
        .enumerate()
        {
            bomb.can_detonate[slot] = section.read_ares_nullable_bool(key, bomb.can_detonate[slot]);
        }
        bomb.detonate_on_sell =
            section.read_ares_bool("IvanBomb.DetonateOnSell", bomb.detonate_on_sell);
        bomb
    }
}

/// Ares WeaponTypeExt abduction (#680 Chrono Prison; ExtData +0xB0..+0xC4,
/// constructor 100548B0), consumed by `conductAbduction` (Ares.dll 10054A80,
/// `sim::abduction`). `Abductor.Anim` resolves in the ordered processor.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AresAbductor {
    /// `Abductor` (+0xB0): the weapon puts an eligible target into its
    /// firer's passenger hold instead of hitting it.
    pub(crate) abductor: bool,
    /// `Abductor.ChangeOwner` (+0xB1): the abductee joins the firer's house.
    pub(crate) change_owner: bool,
    /// `Abductor.Temporal` (+0xB2): abduct when the warp erases the target
    /// (hook 0x71AAAC) instead of on impact (hook 0x46920B).
    pub(crate) temporal: bool,
    /// `Abductor.AbductBelowPercent` (+0xB8, Valueable<double>, 1.0): only a
    /// target at or below this health ratio.
    pub(crate) abduct_below_percent: NativeF64Bits,
    /// `Abductor.MaxHealth` (+0xC0, 0 = any): only a target with at most this
    /// much health.
    pub(crate) max_health: i32,
    /// `Abductor.Anim` (+0xC4, Valueable<AnimTypeClass*>, Find): played at
    /// the abductee.
    pub(crate) anim: Option<String>,
}

impl AresAbductor {
    /// The constructor's values (100548B0: +0xB0..+0xB2 zero, +0xB8 1.0).
    pub(crate) const CONSTRUCTED: Self = Self {
        abductor: false,
        change_owner: false,
        temporal: false,
        abduct_below_percent: NativeF64Bits::ONE,
        max_health: 0,
        anim: None,
    };

    /// LoadFromINIFile 10055520's reads of these keys, in its order.
    pub(crate) fn read(section: &IniSection) -> Self {
        let _ares = crate::rules::ares_switch::ares_reads();
        let mut abductor = Self::CONSTRUCTED;
        abductor.abductor = section.read_ares_bool("Abductor", abductor.abductor);
        abductor.change_owner =
            section.read_ares_bool("Abductor.ChangeOwner", abductor.change_owner);
        abductor.temporal = section.read_ares_bool("Abductor.Temporal", abductor.temporal);
        abductor.abduct_below_percent = section
            .read_ares_double_bits("Abductor.AbductBelowPercent", abductor.abduct_below_percent);
        abductor.max_health = section.read_ares_int("Abductor.MaxHealth", abductor.max_health);
        abductor
    }
}

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
    /// `Wave.IsLaser` (+0x39) and `Wave.IsBigLaser` (+0x3A): the weapon
    /// fires a TS laser wave ([`Self::laser_wave_type`]).
    wave_is_laser: [bool; 2],
    /// `Wave.IsHouseColor` (+0x38).
    pub(crate) wave_is_house_color: bool,
    /// `Wave.Intensity` (+0x3C) and `Wave.Color` (+0x4C), three ints each.
    pub(crate) wave_intensity: Option<[i32; 3]>,
    pub(crate) wave_color: Option<[i32; 3]>,
    /// `Wave.ReverseAgainstVehicles/Aircraft/Buildings/Infantry/Others`.
    wave_reverse: [bool; 5],
    /// `LaserThickness` (+0x64, Valueable<int>, -1): the laser's line width
    /// for SpawnLaser (100563A0), FireAt (100563D0) and the Prism support
    /// beam (10016490); each keeps its own default for -1.
    pub(crate) laser_thickness: i32,
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
            wave_is_laser: [false; 2],
            wave_is_house_color: false,
            wave_intensity: None,
            wave_color: None,
            wave_reverse: [is_mag_beam, false, false, false, false],
            laser_thickness: -1,
        }
    }

    /// LoadFromINIFile 10055520's reads of these keys, in its order. The
    /// vehicle reversal is seeded from this projection's IsMagBeam; native
    /// seeds it from the value after the first pass that read the weapon.
    pub(crate) fn read(section: &IniSection, is_mag_beam: bool) -> Self {
        let _ares = crate::rules::ares_switch::ares_reads();
        let mut visuals = Self::constructed(is_mag_beam);
        visuals.is_detached_railgun =
            section.read_ares_bool("IsDetachedRailgun", visuals.is_detached_railgun);
        visuals.beam_duration = section.read_ares_int("Beam.Duration", visuals.beam_duration);
        visuals.beam_amplitude =
            section.read_ares_double_bits("Beam.Amplitude", visuals.beam_amplitude);
        visuals.beam_is_house_color =
            section.read_ares_bool("Beam.IsHouseColor", visuals.beam_is_house_color);
        visuals.beam_color = section.read_ares_nullable_color("Beam.Color", visuals.beam_color);
        for (slot, key) in ["Wave.IsLaser", "Wave.IsBigLaser"].into_iter().enumerate() {
            visuals.wave_is_laser[slot] = section.read_ares_bool(key, visuals.wave_is_laser[slot]);
        }
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
        visuals.laser_thickness = section.read_ares_int("LaserThickness", visuals.laser_thickness);
        visuals
    }

    /// TechnoClass_Fire_OtherWaves (Ares.dll 10056E60, hook 0x6FF5F5)'s
    /// laser pick: `Wave.IsBigLaser` makes WaveClass type 2 (BigLaser), else
    /// `Wave.IsLaser` type 1 (Laser).
    pub(crate) fn laser_wave_type(&self) -> Option<u8> {
        match self.wave_is_laser {
            [_, true] => Some(2),
            [true, false] => Some(1),
            [false, false] => None,
        }
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

    /// The Ivan bomb keys over two passes: constructor defaults, Nullable
    /// presence, the `.shp` image names in pass order, and the warhead's
    /// non-allocating Find (`none` stores null, an unknown name retains).
    #[test]
    fn ivan_bomb_keys_read_over_passes() {
        use crate::rules::native_processing::{RulesLayerKind, RulesLayerStack};
        use crate::rules::ruleset::RuleSet;

        let mut layers = RulesLayerStack::new(IniFile::from_str(
            "[InfantryTypes]\n0=PROBE\n[PROBE]\nPrimary=BOOM\nSecondary=DUD\n\
             [Warheads]\n0=BLAST\n[BLAST]\n\
             [BOOM]\nIvanBomb.Warhead=blast\nIvanBomb.Damage=x\nIvanBomb.Delay=90\n\
             IvanBomb.Image=CLOCK\nIvanBomb.Detachable=no\n\
             [DUD]\nIvanBomb.Warhead=none\n",
        ));
        layers.push(
            RulesLayerKind::Scenario,
            IniFile::from_str("[BOOM]\nIvanBomb.Warhead=Missing\nIvanBomb.Image=MAPCLOCK\n"),
        );
        let rules = RuleSet::from_rules_layers(&layers).unwrap();
        let bomb = |weapon| rules.weapon(weapon).unwrap().ares_ivan_bomb.clone();
        let boom = bomb("BOOM");
        assert_eq!(boom.warhead, Some(Some("BLAST".to_string())));
        assert_eq!((boom.damage, boom.delay), (None, Some(90)));
        assert_eq!(boom.image, ["CLOCK.shp", "MAPCLOCK.shp"]);
        assert!(!boom.detachable && boom.destroys_bridges && boom.detonate_on_sell);
        assert_eq!(bomb("DUD").warhead, Some(None));
        let read = AresIvanBomb::read(IniFile::from_str("[W]\nDamage=1\n").section("W").unwrap());
        assert_eq!(read, AresIvanBomb::CONSTRUCTED);
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

    /// 10055520's reads of `Wave.IsLaser`/`Wave.IsBigLaser` (Valueable<bool>,
    /// false; BigLaser wins the wave type) and
    /// `IvanBomb.CanDetonateTimeBomb`/`DeathBomb` (Nullable<bool>, unset),
    /// which nothing consumes.
    #[test]
    fn laser_wave_and_detonation_keys_read_over_constructor_defaults() {
        let visuals = read("[W]\nWave.IsLaser=yes\nWave.IsBigLaser=x\n", false);
        assert_eq!(visuals.wave_is_laser, [true, false]);
        assert_eq!(visuals.laser_wave_type(), Some(1));
        let big = read("[W]\nWave.IsLaser=yes\nWave.IsBigLaser=yes\n", false);
        assert_eq!(big.laser_wave_type(), Some(2));
        assert_eq!(visuals.laser_thickness, -1);
        assert_eq!(read("[W]\nLaserThickness=6\n", false).laser_thickness, 6);
        assert_eq!(read("[W]\nDamage=1\n", false).laser_wave_type(), None);
        let file = IniFile::from_str(
            "[W]\nIvanBomb.CanDetonateTimeBomb=no\nIvanBomb.CanDetonateDeathBomb=bad\n",
        );
        let bomb = AresIvanBomb::read(file.section("W").unwrap());
        assert_eq!(bomb.can_detonate, [Some(false), None]);
        assert_eq!(AresIvanBomb::CONSTRUCTED.can_detonate, [None, None]);
    }
}
