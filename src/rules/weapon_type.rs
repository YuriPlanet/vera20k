//! Weapon type definitions parsed from rules.ini.
//!
//! Each weapon in RA2 has its own `[WeaponName]` section in rules.ini,
//! defining damage, range, rate of fire, and references to a projectile
//! type and warhead type. Units reference weapons via their `Primary=`
//! and `Secondary=` keys.
//!
//! ## rules.ini format
//! ```ini
//! [105mm]
//! Damage=65
//! ROF=50
//! Range=5.75
//! Speed=40
//! Projectile=InvisibleLow
//! Warhead=AP
//! ```
//!
//! ## Dependency rules
//! - Part of rules/ — no dependencies on sim/, render/, ui/, etc.

use crate::rules::ini_parser::IniSection;
use crate::rules::phobos_shields::AffectedHouse;

/// A weapon definition parsed from a rules.ini section.
///
/// Weapons are the bridge between units and damage. A unit fires a weapon,
/// which spawns a projectile carrying a warhead. The warhead determines
/// actual damage based on the target's armor type.
///
/// Fields read by the original WeaponTypeClass::ReadINI at 0x772080.
#[derive(Debug, Clone)]
pub struct WeaponType {
    // ── Core fields ──────────────────────────────────────────────────
    /// Section name in rules.ini (e.g., "105mm", "Vulcan", "RedEye2").
    pub id: String,
    /// Base damage per hit (`+0xA4`).
    pub damage: i32,
    /// Maximum range in LEPTONS — the value gamemd actually stores.
    ///
    /// `WeaponTypeClass::ReadINI` 0x00772080 fills `+0xB4` through
    /// `CCINIClass::ReadRange` 0x00474620 at 0x00772336, and that reader
    /// multiplies the INI cell count by `256.0` (`FMUL double ptr
    /// [0x007E1710]` at 0x0047464C) before `Math__ftol` chops it, so a
    /// fractional `Range=` keeps its fraction. Every native consumer —
    /// `TechnoClass::InRange` 0x006F7220 included — reads leptons. The ordered
    /// Rules processor reads it on each reached pass; `Range=-1` keeps it.
    ///
    /// The `-2` "infinite" sentinel survives the conversion as `-512`, which
    /// is the constant 0x006F724E tests.
    pub range_leptons: i32,
    /// Rate of fire: frames between consecutive shots (lower = faster).
    pub rof: i32,
    /// Native stored travel speed in leptons/frame, after ReadSpeed and the
    /// per-pass ballistic postpass. Authored Speed=40 reads as 102. Zero alone
    /// does not designate hitscan; projectile type and launch determine motion.
    pub speed: i32,
    /// Projectile type ID (`+0xA0`, references a [ProjectileName] section).
    pub projectile: Option<String>,
    /// Warhead type ID (`+0xAC`, references a [WarheadName] section).
    pub warhead: Option<String>,
    /// `Report=` firing sounds read by `WeaponTypeClass::ReadINI @ 0x00772080`
    /// through `ReadSoundList @ 0x00525430`, resolved against the fixed registry.
    pub report: Vec<String>,
    /// `DownReport=` read through `ReadSoundList @ 0x00525430` by
    /// `WeaponTypeClass::ReadINI @ 0x00772080`; the constructor starts with an
    /// empty list (`WeaponTypeClass::Constructor @ 0x00771C70`).
    pub down_report: Vec<String>,
    /// Number of rapid shots per attack cycle (default 1).
    /// After the full burst, the ROF cooldown begins.
    pub burst: i32,
    /// Ares WeaponTypeExt constructor100549B4 and reader10055898 (+D0).
    /// Valueable<int> retains failed/missing reads; default one round.
    #[cfg(feature = "ares")]
    ares_ammo_cost: i32,
    /// `RevealOnFire=` (`+0x137`, default yes): a launched shot reveals the
    /// firer to a human victim (FireAt `0x006FF66C`). Retail turns it off on
    /// the sniper, Virus, disguise and Mirage weapons.
    pub reveal_on_fire: bool,
    /// `TerrainFire=` (`+0x138`, default no): the weapon may fire at a
    /// Terrain object unforced (`TechnoClass` vt+0x398, `0x0070EF00`).
    /// Retail authors it only on the unused `TankMakeupKit`.
    pub terrain_fire: bool,
    /// `DisguiseFireOnly=` (`+0x13B`, constructor default false at
    /// `WeaponTypeClass::Constructor @ 0x00771C70`); `ReadBool` is called by
    /// `WeaponTypeClass::ReadINI @ 0x00772080` at `0x007721F9`.
    pub disguise_fire_only: bool,
    /// Infantry WhatAction51EA80: Attack becomes action64 (C4 cursor).
    pub sabotage_cursor: bool,
    /// Infantry WhatAction51EAD2: eligible buildings show action71 (air strike).
    pub mig_attack_cursor: bool,

    // ── Int fields ───────────────────────────────────────────────────
    /// Ambient damage dealt to units standing near the weapon's owner (+0x98).
    pub ambient_damage: i32,
    /// `DisguiseFakeBlinkTime=` (`+0x13C`): the frames a `DisguiseWhenStill=`
    /// Unit's disguise blinks after it fires this weapon
    /// (`UnitClass::Fire_At @ 0x0074145E`).
    pub disguise_fake_blink_time: i32,
    /// Minimum firing range in LEPTONS — `+0xB8` through the same
    /// `CCINIClass::ReadRange` 0x00474620 call at 0x00772350.
    pub minimum_range_leptons: i32,
    /// Signed byte duration of the laser effect, sign-extended for its timer
    /// consumer (+0x14E; constructor10, ReadInt7727DE, MOVSX6FD21D).
    pub laser_duration: i32,
    /// Radiation level emitted on impact (+0x158).
    pub rad_level: i32,
    /// Phobos `RadType=`: the `[RadiationTypes]` entry this weapon's
    /// radiation field uses (`WeaponTypeExt::RadType`, default the
    /// `Radiation` type). Only read in Phobos builds; `None` binds the
    /// default type.
    pub rad_type: Option<String>,

    // ── String/reference fields ──────────────────────────────────────
    // The Type references are the stored IDs the ordered Rules processor's
    // factories answered (`native_processing`); `RuleSet` publishes them.
    /// Animation for the native `AssaultAnim=` pointer at +0x114.
    pub assault_anim: Option<String>,
    /// Animation played by occupants when firing from a building.
    pub occupant_anim: Option<String>,
    /// Animation played by units firing from an open-topped transport.
    pub open_topped_anim: Option<String>,
    /// Particle system spawned when this weapon fires.
    pub attached_particle_system: Option<String>,
    /// Comma-separated list of impact animations, indexed by damage magnitude.
    pub anim: Vec<String>,

    // ── Color fields (3 bytes each, "R,G,B" format) ──────────────────
    /// Inner color of laser beam (+0x120).
    pub laser_inner_color: [u8; 3],
    /// Outer color of laser beam (+0x123).
    pub laser_outer_color: [u8; 3],
    /// Outer spread/glow color of laser beam (+0x126).
    pub laser_outer_spread: [u8; 3],

    // ── Bool fields ──────────────────────────────────────────────────
    /// Weapon uses fire particle effects (+0x129).
    pub use_fire_particles: bool,
    /// Weapon uses spark particle effects (+0x12a).
    pub use_spark_particles: bool,
    /// Fires in all directions regardless of facing (+0x12b).
    pub omni_fire: bool,
    /// `DistributedWeaponFire=` (`+0x12C`, constructor default false at
    /// `WeaponTypeClass::Constructor @ 0x00771C70`); `ReadBool` is called by
    /// `WeaponTypeClass::ReadINI @ 0x00772080` at `0x0077271D`.
    pub distributed_weapon_fire: bool,
    /// Weapon is a railgun (special projectile visual) (+0x12d).
    pub is_railgun: bool,
    /// Projectile follows a lobbed arc trajectory (+0x12e).
    pub lobber: bool,
    /// Weapon flash is extra bright (+0x12f).
    pub bright: bool,
    /// Weapon deals sonic/disruptor damage (+0x130).
    pub is_sonic: bool,
    /// Weapon spawns aircraft (like aircraft carriers) (+0x131).
    pub spawner: bool,
    /// `LimboLaunch=` (+0x132, ReadINI `0x00772114`): TechnoClass::Fire
    /// `0x006FF751` puts the FIRER in limbo after launching the bullet, which
    /// carries it (attack dog, Terror Drone and Giant Squid jumps).
    pub limbo_launch: bool,
    /// Unit must decloak before firing this weapon (+0x133).
    pub decloak_to_fire: bool,
    /// Uses cell-center rangefinding instead of edge-to-edge (+0x134).
    pub cell_rangefinding: bool,
    /// Weapon fires only once then is consumed (+0x135).
    pub fire_once: bool,
    /// Weapon is never automatically selected for use (+0x136).
    pub never_use: bool,
    /// Mind control effect has no unit limit (+0x140).
    pub infinite_mind_control: bool,
    /// Unit can fire this weapon while moving (+0x141). The constructor sets
    /// it (`0x00771DE6`) and `ReadINI` keeps that as its default
    /// (`0x0077221E..0x00772232`); stock only DiskDrain says no.
    pub fire_while_moving: bool,
    /// Weapon drains target's health to heal the firer (+0x142).
    pub drain_weapon: bool,
    /// `FireInTransport=` (+0x143): an open-topped passenger may fire it.
    /// The constructor sets it (`0x00771DF3`) and `ReadINI` keeps that as its
    /// default (ReadBool at `0x00772252`); stock opts only melee and special
    /// weapons out. GetFireError refuses it from an open-topped transport
    /// (`0x006FC57D`).
    pub fire_in_transport: bool,
    /// Firing this weapon kills the attacker (+0x144).
    pub suicide: bool,
    /// Grants speed boost on hit (+0x145).
    pub turbo_boost: bool,
    /// Suppresses target (Westwood's misspelling preserved) (+0x146).
    pub supress: bool,
    /// Weapon spawns a camera/reveal at impact point (+0x147).
    pub camera: bool,
    /// Weapon has limited charges (+0x148).
    pub charges: bool,
    /// Weapon fires a laser beam visual (+0x149).
    pub is_laser: bool,
    /// Weapon fires a disk-shaped laser (Vortex) (+0x14a).
    pub disk_laser: bool,
    /// Weapon draws a visible line to target (+0x14b).
    pub is_line: bool,
    /// Weapon fires a thicker laser beam (+0x14c).
    pub is_big_laser: bool,
    /// Laser color matches house/player color (+0x14d).
    pub is_house_color: bool,
    /// Weapon is affected by ion storms (+0x14f).
    pub ion_sensitive: bool,
    /// Weapon fires at area/ground instead of specific target (+0x150).
    pub area_fire: bool,
    /// Weapon fires an electric bolt visual (Tesla) (+0x151).
    pub is_electric_bolt: bool,
    /// Draw electric bolt using laser rendering (+0x152).
    pub draw_bolt_as_laser: bool,
    /// Laser uses alternate (darker) color scheme (+0x153).
    pub is_alternate_color: bool,
    /// Weapon fires a radiation beam visual (+0x154).
    pub is_rad_beam: bool,
    /// Weapon triggers a radiation eruption effect (+0x155).
    pub is_rad_eruption: bool,
    /// Weapon fires a magnetron beam (+0x15c).
    pub is_mag_beam: bool,

    // ── Phobos AttachEffect gating ───────────────────────────────────
    // `WeaponTypeExt::HasRequiredAttachedEffects` gates a shot on the AE the
    // target (or, with `CheckOnFirer`, the firer) carries. The required lists
    // must all match; the disallowed lists refuse the shot on any match.
    // Counts pair with the type/group lists by position, the last value
    // covering the rest.
    pub attach_effect_required_types: Vec<String>,
    pub attach_effect_disallowed_types: Vec<String>,
    pub attach_effect_required_groups: Vec<String>,
    pub attach_effect_disallowed_groups: Vec<String>,
    pub attach_effect_required_min_counts: Vec<i32>,
    pub attach_effect_required_max_counts: Vec<i32>,
    pub attach_effect_disallowed_min_counts: Vec<i32>,
    pub attach_effect_disallowed_max_counts: Vec<i32>,
    pub attach_effect_check_on_firer: bool,
    pub attach_effect_ignore_from_same_source: bool,
    /// `AttachEffect.Required.Any` / `Disallowed.Any`: the native
    /// `requireAll = !Any` switch. Each list defaults to its constructor value
    /// (required `false`, disallowed `true`).
    pub attach_effect_required_any: bool,
    pub attach_effect_disallowed_any: bool,
    /// `AttachEffect.Required.Houses` / `Disallowed.Houses`: the house the
    /// matched instance's invoker must belong to.
    pub attach_effect_required_houses: AffectedHouse,
    pub attach_effect_disallowed_houses: AffectedHouse,
    /// `AttachEffect.SameSourceOnly`: only instances from the shot's exact
    /// invoker and source match.
    pub attach_effect_same_source_only: bool,
    /// Phobos `ROF.RandomDelay` (`WeaponType/Body.cpp:134`,
    /// `Nullable<PartialVector2D<int>>`): this weapon's full-rearm jitter
    /// range; `None` inherits `[CombatDamage]`.
    #[cfg(feature = "phobos")]
    pub rof_random_delay: Option<crate::rules::ini_value::PartialIntPair>,
    /// Phobos `WeaponTypeExt::CanTargetHouses` (`AffectedHouse::All` default),
    /// read for `Splits.UseWeaponTargeting`.
    #[cfg(feature = "phobos")]
    pub(crate) phobos_can_target_houses: crate::rules::phobos_shields::AffectedHouse,
    /// Phobos `CanTarget`, `CanTarget.MinHealth`/`MaxHealth` and
    /// `CanTargetVeterancy`, read for `Splits.UseWeaponTargeting`.
    #[cfg(feature = "phobos")]
    pub(crate) phobos_weapon_targeting: crate::rules::phobos_airburst::PhobosWeaponTargeting,
    /// Phobos `WeaponTypeExt::Laser_IsSingleColor` (`IsSingleColor=`): a
    /// simulated child laser draws one color instead of the inner/outer pair.
    #[cfg(feature = "phobos")]
    pub phobos_laser_is_single_color: bool,
    /// Phobos `WeaponTypeExt::LaserThickness` (`LaserThickness=`,
    /// constructor default 3): a simulated child laser's line count.
    #[cfg(feature = "phobos")]
    pub phobos_laser_thickness: i32,
    /// Phobos `WeaponTypeExt::Beam_IsHouseColor` (`Beam.IsHouseColor=`): a
    /// simulated child radiation beam takes the firing House's laser color.
    #[cfg(feature = "phobos")]
    pub phobos_beam_is_house_color: bool,
    /// Phobos `LaserPositionUpdate` (`Valueable<PositionFollow>`, default
    /// none): which ends of this weapon's lasers follow their live objects.
    #[cfg(feature = "phobos")]
    pub laser_position_update: crate::rules::phobos_laser_tracking::PositionFollow,
    /// Phobos `LaserPositionUpdate.StopOnFirerConvert` (`Nullable<bool>`):
    /// stop following the firer once its TechnoType changes; `None` inherits
    /// `[AudioVisual]`.
    #[cfg(feature = "phobos")]
    pub laser_position_update_stop_on_firer_convert: Option<bool>,
}

impl WeaponType {
    /// `WeaponTypeExt::HasRequiredAttachedEffects` reads no list: every shot
    /// passes without asking the target.
    pub fn has_attach_effect_gates(&self) -> bool {
        !self.attach_effect_required_types.is_empty()
            || !self.attach_effect_disallowed_types.is_empty()
            || !self.attach_effect_required_groups.is_empty()
            || !self.attach_effect_disallowed_groups.is_empty()
    }

    /// Phobos `CanTargetHouses=`, default `AffectedHouse::All`.
    #[cfg(feature = "phobos")]
    pub(crate) const fn can_target_houses(&self) -> crate::rules::phobos_shields::AffectedHouse {
        self.phobos_can_target_houses
    }

    /// Phobos `CanTarget`, health range and `CanTargetVeterancy`, defaults
    /// `All`/`0.0`/`1.0`/`All`.
    #[cfg(feature = "phobos")]
    pub(crate) const fn weapon_targeting(
        &self,
    ) -> crate::rules::phobos_airburst::PhobosWeaponTargeting {
        self.phobos_weapon_targeting
    }

    pub(crate) fn ammo_cost(&self) -> i32 {
        #[cfg(feature = "ares")]
        return self.ares_ammo_cost;
        #[cfg(not(feature = "ares"))]
        1
    }

    /// `Report.Count` (`WeaponTypeClass+0xCC`).
    pub fn report_count(&self) -> i32 {
        self.report.len() as i32
    }

    /// `Report.Items[index]`.
    pub fn report_item(&self, index: usize) -> Option<&str> {
        self.report.get(index).map(String::as_str)
    }

    /// Parse a WeaponType from a rules.ini section.
    ///
    /// Damage, Speed, Range, Projectile and Warhead keep the constructor's
    /// zero and null (`0x00771C70`): the ordered Rules processor owns those
    /// reads (Damage `0x007722E3`, Speed `0x007722FD`, Range `0x00772336`,
    /// Warhead `0x0077295F`, Projectile `0x00772998`) and the speed postpass
    /// (`0x007729F0`), and `RuleSet` builds each weapon with its retained
    /// values.
    pub fn from_ini_section(id: &str, section: &IniSection) -> Self {
        // Phobos `WeaponTypeExt::RadType` read; absent (and every build
        // without the capability) binds the default radiation type.
        #[cfg(feature = "phobos")]
        let rad_type = section.read_name("RadType", 0x80).map(str::to_owned);
        #[cfg(not(feature = "phobos"))]
        let rad_type = None;
        // `WeaponTypeExt::LoadFromINIFile`'s AttachEffect gates. A missing key
        // keeps the constructor's empty list / false.
        #[cfg(feature = "phobos")]
        let (
            attach_effect_required_types,
            attach_effect_disallowed_types,
            attach_effect_required_groups,
            attach_effect_disallowed_groups,
        ) = (
            name_list(section, "AttachEffect.RequiredTypes"),
            name_list(section, "AttachEffect.DisallowedTypes"),
            name_list(section, "AttachEffect.RequiredGroups"),
            name_list(section, "AttachEffect.DisallowedGroups"),
        );
        #[cfg(feature = "phobos")]
        let (
            attach_effect_required_min_counts,
            attach_effect_required_max_counts,
            attach_effect_disallowed_min_counts,
            attach_effect_disallowed_max_counts,
        ) = (
            section
                .read_int_list("AttachEffect.RequiredMinCounts")
                .unwrap_or_default(),
            section
                .read_int_list("AttachEffect.RequiredMaxCounts")
                .unwrap_or_default(),
            section
                .read_int_list("AttachEffect.DisallowedMinCounts")
                .unwrap_or_default(),
            section
                .read_int_list("AttachEffect.DisallowedMaxCounts")
                .unwrap_or_default(),
        );
        #[cfg(feature = "phobos")]
        let attach_effect_check_on_firer = section.read_bool("AttachEffect.CheckOnFirer", false);
        #[cfg(feature = "phobos")]
        let attach_effect_ignore_from_same_source =
            section.read_bool("AttachEffect.IgnoreFromSameSource", false);
        #[cfg(feature = "phobos")]
        let (
            attach_effect_required_any,
            attach_effect_disallowed_any,
            attach_effect_required_houses,
            attach_effect_disallowed_houses,
            attach_effect_same_source_only,
        ) = (
            section.read_bool("AttachEffect.Required.Any", false),
            section.read_bool("AttachEffect.Disallowed.Any", true),
            section.read_string_with(
                "AttachEffect.Required.Houses",
                0x100,
                AffectedHouse::ALL,
                |current, raw| AffectedHouse::parse(raw).unwrap_or(current),
            ),
            section.read_string_with(
                "AttachEffect.Disallowed.Houses",
                0x100,
                AffectedHouse::ALL,
                |current, raw| AffectedHouse::parse(raw).unwrap_or(current),
            ),
            section.read_bool("AttachEffect.SameSourceOnly", false),
        );
        #[cfg(not(feature = "phobos"))]
        let (
            attach_effect_required_any,
            attach_effect_disallowed_any,
            attach_effect_required_houses,
            attach_effect_disallowed_houses,
            attach_effect_same_source_only,
        ) = (false, true, AffectedHouse::ALL, AffectedHouse::ALL, false);
        #[cfg(not(feature = "phobos"))]
        let (
            attach_effect_required_types,
            attach_effect_disallowed_types,
            attach_effect_required_groups,
            attach_effect_disallowed_groups,
            attach_effect_required_min_counts,
            attach_effect_required_max_counts,
            attach_effect_disallowed_min_counts,
            attach_effect_disallowed_max_counts,
            attach_effect_check_on_firer,
            attach_effect_ignore_from_same_source,
        ) = (
            Vec::new(),
            Vec::new(),
            Vec::new(),
            Vec::new(),
            Vec::new(),
            Vec::new(),
            Vec::new(),
            Vec::new(),
            false,
            false,
        );
        Self {
            id: id.to_string(),
            damage: 0,
            // `WeaponTypeClass::Constructor` 0x00771C70 zeroes +0xB4 at
            // 0x00771CB6.
            range_leptons: 0,
            rof: section.read_int("ROF", 0),
            speed: 0,
            projectile: None,
            warhead: None,
            report: sound_list(section, "Report"),
            down_report: Vec::new(),
            burst: section.read_int("Burst", 1),
            #[cfg(feature = "ares")]
            ares_ammo_cost: section.read_ares_int("Ammo", 1),
            // Constructor 1 (`0x00771DBB`); ReadINI passes it as the default
            // (`0x00772182..0x00772196`).
            reveal_on_fire: section.read_bool("RevealOnFire", true),
            // Constructor771DC2/771DC8/771DCE zero +138/+139/+13A;
            // ReadINI7721A3..7721E4 passes the retained bytes to the native
            // boolean reader5295F0.
            terrain_fire: section.read_bool("TerrainFire", false),
            disguise_fire_only: section.read_bool("DisguiseFireOnly", false),
            sabotage_cursor: section.read_bool("SabotageCursor", false),
            mig_attack_cursor: section.read_bool("MigAttackCursor", false),

            // Int fields
            ambient_damage: section.read_int("AmbientDamage", 0),
            // Constructor 0 (`0x00771DDA`); ReadINI passes it as the default
            // (`0x0077226C..0x00772280`).
            disguise_fake_blink_time: section.read_int("DisguiseFakeBlinkTime", 0),
            // +0xB8 is zeroed alongside +0xB4 at 0x00771CBF.
            minimum_range_leptons: section.read_range("MinimumRange", 0),
            // ReadINI stores only AL; SpawnLaser6FD21D sign-extends the
            // retained byte. ReadInt uses its default only for absent keys,
            // so narrowing after the projected pass history is equivalent.
            laser_duration: i32::from(section.read_int("LaserDuration", 10) as i8),
            rad_level: section.read_int("RadLevel", 0),
            rad_type,
            #[cfg(feature = "phobos")]
            phobos_can_target_houses: crate::rules::phobos_shields::read_affected_house(
                section,
                "CanTargetHouses",
                crate::rules::phobos_shields::AffectedHouse::ALL,
            ),
            #[cfg(feature = "phobos")]
            phobos_weapon_targeting: crate::rules::phobos_airburst::read_weapon_targeting(section),
            // Phobos `Valueable<T>` reads through the shared Ares parser: a
            // failed conversion keeps the constructor's value.
            #[cfg(feature = "phobos")]
            phobos_laser_is_single_color: section.read_ares_bool("IsSingleColor", false),
            #[cfg(feature = "phobos")]
            phobos_laser_thickness: section.read_ares_int("LaserThickness", 3),
            #[cfg(feature = "phobos")]
            phobos_beam_is_house_color: section.read_ares_bool("Beam.IsHouseColor", false),

            // The constructor's nulls and empty list (`0x00771C70`); the
            // ordered Process owner reads the Animation references.
            assault_anim: None,
            occupant_anim: None,
            open_topped_anim: None,
            attached_particle_system: None,
            anim: Vec::new(),

            // Color fields: ReadColorRGB over the constructor's zero
            // (`0x00771D30`-`0x00771D60`), at `0x00772770`, `0x00772796` and
            // `0x007727BC`.
            laser_inner_color: section.read_color_rgb("LaserInnerColor", [0; 3], None),
            laser_outer_color: section.read_color_rgb("LaserOuterColor", [0; 3], None),
            laser_outer_spread: section.read_color_rgb("LaserOuterSpread", [0; 3], None),

            // Bool fields
            use_fire_particles: section.read_bool("UseFireParticles", false),
            use_spark_particles: section.read_bool("UseSparkParticles", false),
            omni_fire: section.read_bool("OmniFire", false),
            distributed_weapon_fire: section.read_bool("DistributedWeaponFire", false),
            is_railgun: section.read_bool("IsRailgun", false),
            lobber: section.read_bool("Lobber", false),
            bright: section.read_bool("Bright", false),
            is_sonic: section.read_bool("IsSonic", false),
            spawner: section.read_bool("Spawner", false),
            limbo_launch: section.read_bool("LimboLaunch", false),
            // WeaponTypeClass constructor / ReadINI consumed by
            // TechnoClass::GetFireError @ 0x006FC0B0: omitted
            // DecloakToFire= is YES. Stock CruiseLauncher relies on the
            // constructor default; BoomerTorpedo explicitly opts out.
            decloak_to_fire: section.read_bool("DecloakToFire", true),
            cell_rangefinding: section.read_bool("CellRangefinding", false),
            fire_once: section.read_bool("FireOnce", false),
            never_use: section.read_bool("NeverUse", false),
            infinite_mind_control: section.read_bool("InfiniteMindControl", false),
            fire_while_moving: section.read_bool("FireWhileMoving", true),
            drain_weapon: section.read_bool("DrainWeapon", false),
            fire_in_transport: section.read_bool("FireInTransport", true),
            suicide: section.read_bool("Suicide", false),
            turbo_boost: section.read_bool("TurboBoost", false),
            supress: section.read_bool("Supress", false),
            camera: section.read_bool("Camera", false),
            charges: section.read_bool("Charges", false),
            is_laser: section.read_bool("IsLaser", false),
            disk_laser: section.read_bool("DiskLaser", false),
            is_line: section.read_bool("IsLine", false),
            is_big_laser: section.read_bool("IsBigLaser", false),
            is_house_color: section.read_bool("IsHouseColor", false),
            ion_sensitive: section.read_bool("IonSensitive", false),
            area_fire: section.read_bool("AreaFire", false),
            is_electric_bolt: section.read_bool("IsElectricBolt", false),
            draw_bolt_as_laser: section.read_bool("DrawBoltAsLaser", false),
            is_alternate_color: section.read_bool("IsAlternateColor", false),
            is_rad_beam: section.read_bool("IsRadBeam", false),
            is_rad_eruption: section.read_bool("IsRadEruption", false),
            is_mag_beam: section.read_bool("IsMagBeam", false),

            attach_effect_required_types,
            attach_effect_disallowed_types,
            attach_effect_required_groups,
            attach_effect_disallowed_groups,
            attach_effect_required_min_counts,
            attach_effect_required_max_counts,
            attach_effect_disallowed_min_counts,
            attach_effect_disallowed_max_counts,
            attach_effect_check_on_firer,
            attach_effect_ignore_from_same_source,
            attach_effect_required_any,
            attach_effect_disallowed_any,
            attach_effect_required_houses,
            attach_effect_disallowed_houses,
            attach_effect_same_source_only,
            #[cfg(feature = "phobos")]
            rof_random_delay: Some(section.read_ares_partial_int_pair(
                "ROF.RandomDelay",
                crate::rules::ini_value::PartialIntPair {
                    low: 0,
                    high: 0,
                    values: 0,
                },
            ))
            .filter(|range| range.values > 0),
            #[cfg(feature = "phobos")]
            laser_position_update: section.read_string_with(
                "LaserPositionUpdate",
                2048,
                crate::rules::phobos_laser_tracking::PositionFollow::None,
                |current, raw| {
                    crate::rules::phobos_laser_tracking::PositionFollow::parse(raw)
                        .unwrap_or(current)
                },
            ),
            #[cfg(feature = "phobos")]
            laser_position_update_stop_on_firer_convert: section
                .read_phobos_nullable_bool("LaserPositionUpdate.StopOnFirerConvert", None),
        }
    }
}

/// A Phobos name list: `ReadString(2048)`, `strtok(",")`, tokens kept as the
/// factory would resolve them. Absent keys leave the constructor's empty list.
#[cfg(feature = "phobos")]
fn name_list(section: &IniSection, key: &str) -> Vec<String> {
    section
        .read_list(key, 0x800)
        .map(|tokens| tokens.into_iter().map(str::to_string).collect())
        .unwrap_or_default()
}

/// `Report=` through ReadSoundList (`0x00772394`); production binding resolves
/// both report lists through the fixed SOUNDMD registry.
fn sound_list(section: &IniSection, key: &str) -> Vec<String> {
    section
        .read_sound_list(key)
        .unwrap_or_default()
        .into_iter()
        .map(str::to_string)
        .collect()
}

#[cfg(test)]
mod tests {
    #[test]
    fn weapon_ammo_cost_uses_ares_retained_integer_reader_only_when_enabled() {
        use crate::rules::native_processing::{RulesLayerKind, RulesLayerStack};
        use crate::rules::ruleset::RuleSet;

        let mut layers = RulesLayerStack::new(IniFile::from_str(
            "[InfantryTypes]\n0=PROBE\n[PROBE]\nPrimary=PAIR\nSecondary=FREE\n[PAIR]\nAmmo=2\n[FREE]\nAmmo=0\n",
        ));
        layers.push(
            RulesLayerKind::Scenario,
            IniFile::from_str("[PAIR]\nAmmo=invalid\n"),
        );
        let rules = RuleSet::from_rules_layers(&layers).unwrap();
        assert_eq!(
            rules.weapon("PAIR").unwrap().ammo_cost(),
            if cfg!(feature = "ares") { 2 } else { 1 }
        );
        assert_eq!(
            rules.weapon("FREE").unwrap().ammo_cost(),
            if cfg!(feature = "ares") { 0 } else { 1 }
        );
    }

    /// Retail `RevealOnFire=` through the production reader: the constructor's
    /// yes stands unless a section says no (the sniper and disguise weapons).
    #[test]
    fn retail_reveal_on_fire_defaults_to_yes() {
        let Some(rules) = crate::rules::retail_ini_fixture::retail_rulesmd_rules() else {
            return;
        };
        assert!(rules.weapon("M60").unwrap().reveal_on_fire);
        assert!(rules.weapon("105mm").unwrap().reveal_on_fire);
        assert!(!rules.weapon("AWP").unwrap().reveal_on_fire);
    }

    use super::*;
    use crate::rules::ini_parser::IniFile;

    fn laser_corpus() -> &'static serde_json::Value {
        crate::test_fixture::json("tools/rules_oracle/weapon_laser.json")
    }

    fn laser_state(weapon: &WeaponType) -> serde_json::Value {
        serde_json::json!({
            "is_laser": weapon.is_laser,
            "is_house_color": weapon.is_house_color,
            "is_big_laser": weapon.is_big_laser,
            "duration": weapon.laser_duration,
            "inner": weapon.laser_inner_color,
            "outer": weapon.laser_outer_color,
            "spread": weapon.laser_outer_spread,
        })
    }

    fn cached_laser_sections(sections: &serde_json::Value) -> IniFile {
        let mut ini = IniFile::empty();
        for (name, keys) in sections.as_object().unwrap() {
            let mut section = IniSection::new(name.clone());
            for (key, value) in keys.as_object().unwrap() {
                section.set(key, value.as_str().unwrap());
            }
            ini.replace_first_section(section);
        }
        ini
    }

    #[test]
    fn laser_constructor_and_signed_duration_match_original_reader() {
        let native = laser_corpus();
        let controls = &native["controls"];
        let defaults = WeaponType::from_ini_section("LaserProbe", IniSection::empty());
        assert_eq!(laser_state(&defaults), controls["constructor"]);
        let rows = controls["duration_controls"].as_array().unwrap();
        assert_eq!(rows.len(), 24);
        for row in rows {
            let ini = cached_laser_sections(&row["sections"]);
            let weapon =
                WeaponType::from_ini_section("LaserProbe", ini.section("LaserProbe").unwrap());
            assert_eq!(laser_state(&weapon), row["state"], "{}", row["raw"]);
        }
    }

    #[test]
    fn laser_fields_retain_native_defaults_through_production_rules_passes() {
        use crate::rules::native_processing::{RulesLayerKind, RulesLayerStack};
        use crate::rules::ruleset::RuleSet;

        let native = laser_corpus();
        let rows = native["controls"]["retained_history"].as_array().unwrap();
        assert_eq!(rows.len(), 8);
        let mut layers = RulesLayerStack::new(IniFile::from_str(
            "[BuildingTypes]\n0=Tower\n[Tower]\nPrimary=LaserProbe\n",
        ));
        for (index, row) in rows.iter().enumerate() {
            // Preserve the native cache's explicit empty values; the physical
            // INI loader omits them, so this is a reader-component control.
            layers.push(
                RulesLayerKind::Scenario,
                cached_laser_sections(&row["sections"]),
            );
            let rules = RuleSet::from_rules_layers(&layers).unwrap();
            assert_eq!(
                laser_state(rules.weapon("LaserProbe").unwrap()),
                row["state"],
                "pass {index}"
            );
        }
    }

    #[test]
    fn retail_prism_laser_fields_match_original_layered_readers() {
        use crate::rules::native_processing::{RulesLayerKind, RulesLayerStack};
        use crate::rules::ruleset::RuleSet;

        let Some(ini) = crate::rules::retail_ini_fixture::retail_ini("rulesmd.ini") else {
            return;
        };
        let native = laser_corpus();
        let rows = native["physical_prism"].as_array().unwrap();
        let mut layers = RulesLayerStack::new(ini);
        for row in rows {
            if row["absent"] == true {
                continue;
            }
            if row["file"] != "RULESMD.INI" {
                layers.push(
                    RulesLayerKind::Scenario,
                    cached_laser_sections(&row["sections"]),
                );
            }
            let rules = RuleSet::from_rules_layers(&layers).unwrap();
            assert_eq!(
                laser_state(rules.weapon("PrismShot").unwrap()),
                row["state"],
                "{}",
                row["file"]
            );
        }
        assert_eq!(
            RuleSet::from_rules_layers(&layers)
                .unwrap()
                .general
                .prism_support
                .duration,
            15
        );
    }

    #[test]
    fn retail_prism_laser_and_detail_use_complete_production_source_chain() {
        let Some(retail) =
            crate::rules::retail_ini_fixture::retail_battle_rules_for_map("XMP03T4.MAP")
        else {
            return;
        };
        let native = laser_corpus();
        assert_eq!(
            laser_state(retail.rules.weapon("PrismShot").unwrap()),
            native["physical_prism"].as_array().unwrap().last().unwrap()["state"]
        );
        let detail = retail.rules.general.detail;
        assert_eq!(
            serde_json::json!({
                "min_frame_rate_normal": detail.min_frame_rate_normal,
                "min_frame_rate_movie": detail.min_frame_rate_movie,
                "buffer_zone_width": detail.buffer_zone_width,
            }),
            native["physical_detail"]
                .as_array()
                .unwrap()
                .last()
                .unwrap()["state"]
        );
        assert_eq!(retail.rules.general.prism_support.duration, 15);
    }

    #[test]
    fn retained_laser_fields_participate_in_configuration_identity() {
        use crate::rules::native_processing::RulesLayerStack;
        use crate::rules::ruleset::RuleSet;

        let common = RulesLayerStack::new(IniFile::from_str(
            "[LaserProbe]\nIsLaser=junk\nIsHouseColor=junk\nIsBigLaser=junk\n",
        ));
        let mut identities = Vec::new();
        for initial in ["yes", "no"] {
            let first = RulesLayerStack::new(IniFile::from_str(&format!(
                "[BuildingTypes]\n0=Tower\n[Tower]\nPrimary=LaserProbe\n\
                 [LaserProbe]\nIsLaser={initial}\nIsHouseColor={initial}\nIsBigLaser={initial}\n",
            )))
            .process()
            .unwrap();
            let (_, trace) = first.into_ini_and_native_type_construction_trace();
            let processed = common
                .process_with_fixed_art_and_registry_state(
                    &IniFile::empty(),
                    trace.into_registry_state_discarding_events(),
                )
                .unwrap();
            let rules = RuleSet::from_processed_rules(&processed).unwrap();
            assert_eq!(
                rules.weapon("LaserProbe").unwrap().is_laser,
                initial == "yes"
            );
            identities.push((rules.source_ini_hash(), rules.simulation_config_hash()));
        }
        assert_eq!(
            identities[0].0, identities[1].0,
            "same current rules source"
        );
        assert_ne!(
            identities[0].1, identities[1].1,
            "different retained laser admission"
        );
    }

    /// Phobos `WeaponTypeExt` AttachEffect gates: the constructor's empty
    /// lists and false switches, then every key through the production reader.
    #[cfg(feature = "phobos")]
    #[test]
    fn attach_effect_gate_keys_read_with_their_defaults() {
        use crate::rules::native_processing::{RulesLayerKind, RulesLayerStack};
        use crate::rules::ruleset::RuleSet;

        let mut layers = RulesLayerStack::new(IniFile::from_str(
            "[VehicleTypes]\n0=UNIT\n[UNIT]\nPrimary=Plain\nSecondary=Gated\n\
             [Plain]\nDamage=1\n\
             [Gated]\nDamage=1\nAttachEffect.RequiredTypes=AE1,AE2\n\
             AttachEffect.DisallowedTypes=AE3\n\
             AttachEffect.RequiredGroups=tag\nAttachEffect.DisallowedGroups=bad\n\
             AttachEffect.RequiredMinCounts=2,3\nAttachEffect.RequiredMaxCounts=4\n\
             AttachEffect.DisallowedMinCounts=1\nAttachEffect.DisallowedMaxCounts=5,6\n\
             AttachEffect.CheckOnFirer=yes\nAttachEffect.IgnoreFromSameSource=yes\n\
             AttachEffect.Required.Any=yes\nAttachEffect.Required.Houses=enemies\n\
             AttachEffect.Disallowed.Any=no\nAttachEffect.Disallowed.Houses=allies\n\
             AttachEffect.SameSourceOnly=yes\n\
             [AttachEffectTypes]\n0=AE1\n1=AE2\n2=AE3\n\
             [AE1]\n[AE2]\n[AE3]\n",
        ));
        layers.push(
            RulesLayerKind::GameMode,
            IniFile::from_str("[Gated]\nAttachEffect.RequiredGroups=other\n"),
        );
        let rules = RuleSet::from_rules_layers(&layers).unwrap();
        let plain = rules.weapon("Plain").unwrap();
        assert!(!plain.has_attach_effect_gates());
        assert!(plain.attach_effect_required_types.is_empty());
        assert!(plain.attach_effect_disallowed_types.is_empty());
        assert!(plain.attach_effect_required_groups.is_empty());
        assert!(plain.attach_effect_disallowed_groups.is_empty());
        assert!(plain.attach_effect_required_min_counts.is_empty());
        assert!(plain.attach_effect_required_max_counts.is_empty());
        assert!(plain.attach_effect_disallowed_min_counts.is_empty());
        assert!(plain.attach_effect_disallowed_max_counts.is_empty());
        assert!(!plain.attach_effect_check_on_firer);
        assert!(!plain.attach_effect_ignore_from_same_source);
        // `AEAttachInfoTypeClass`'s `requireAll = !Any` defaults.
        assert!(!plain.attach_effect_required_any);
        assert!(plain.attach_effect_disallowed_any);
        assert_eq!(
            plain.attach_effect_required_houses,
            crate::rules::phobos_shields::AffectedHouse::ALL
        );
        assert_eq!(
            plain.attach_effect_disallowed_houses,
            crate::rules::phobos_shields::AffectedHouse::ALL
        );
        assert!(!plain.attach_effect_same_source_only);

        let gated = rules.weapon("Gated").unwrap();
        assert!(gated.has_attach_effect_gates());
        assert_eq!(gated.attach_effect_required_types, vec!["AE1", "AE2"]);
        assert_eq!(gated.attach_effect_disallowed_types, vec!["AE3"]);
        assert_eq!(
            gated.attach_effect_required_groups,
            vec!["other"],
            "the later pass replaces the list"
        );
        assert_eq!(gated.attach_effect_disallowed_groups, vec!["bad"]);
        assert_eq!(gated.attach_effect_required_min_counts, vec![2, 3]);
        assert_eq!(gated.attach_effect_required_max_counts, vec![4]);
        assert_eq!(gated.attach_effect_disallowed_min_counts, vec![1]);
        assert_eq!(gated.attach_effect_disallowed_max_counts, vec![5, 6]);
        assert!(gated.attach_effect_check_on_firer);
        assert!(gated.attach_effect_ignore_from_same_source);
        assert!(gated.attach_effect_required_any);
        assert!(!gated.attach_effect_disallowed_any);
        assert!(
            gated
                .attach_effect_required_houses
                .contains(crate::rules::phobos_shields::AffectedHouse::ENEMIES)
        );
        assert!(
            gated
                .attach_effect_disallowed_houses
                .contains(crate::rules::phobos_shields::AffectedHouse::ALLIES)
        );
        assert!(gated.attach_effect_same_source_only);
    }

    #[test]
    fn test_parse_weapon() {
        // Damage, Speed, Range, Projectile and Warhead come from the ordered
        // Rules owner. A nonzero ROT keeps the ballistic postpass off
        // ReadSpeed's value.
        let rules = crate::rules::ruleset::RuleSet::from_ini(&IniFile::from_str(
            "[VehicleTypes]\n0=MTNK\n[MTNK]\nPrimary=105mm\n\
             [105mm]\nDamage=65\nROF=50\nRange=5.75\nSpeed=40\n\
             Projectile=InvisibleLow\nWarhead=AP\n\
             [InvisibleLow]\nROT=1\n",
        ))
        .unwrap();
        let weapon = rules.weapon("105mm").unwrap();

        assert_eq!(weapon.id, "105mm");
        assert_eq!(weapon.damage, 65);
        assert_eq!(weapon.range_leptons, 1472, "5.75 cells * 256");
        assert_eq!(weapon.rof, 50);
        assert_eq!(weapon.speed, 102);
        assert_eq!(weapon.projectile, Some("InvisibleLow".to_string()));
        assert_eq!(weapon.warhead, Some("AP".to_string()));
    }

    #[test]
    fn test_weapon_defaults() {
        let ini: IniFile = IniFile::from_str("[Empty]\nFixtureOnly=1\n");
        let section: &IniSection = ini.section("Empty").unwrap();
        let weapon: WeaponType = WeaponType::from_ini_section("Empty", section);

        assert_eq!(weapon.damage, 0);
        assert_eq!(weapon.range_leptons, 0);
        assert_eq!(weapon.rof, 0);
        assert_eq!(weapon.projectile, None);
        assert_eq!(weapon.warhead, None);
        // New fields should all default correctly
        assert_eq!(weapon.ambient_damage, 0);
        assert_eq!(weapon.laser_duration, 10);
        assert_eq!(weapon.rad_level, 0);
        assert!(!weapon.is_sonic);
        assert!(!weapon.is_laser);
        assert!(!weapon.is_electric_bolt);
        assert!(!weapon.distributed_weapon_fire);
        assert!(!weapon.disguise_fire_only);
        assert!(weapon.anim.is_empty());
        assert_eq!(weapon.laser_inner_color, [0, 0, 0]);
        assert!(weapon.decloak_to_fire, "native omitted-key default is yes");
    }

    #[test]
    fn retail_battle_retains_native_weapon_reference_fields() {
        let Some(retail) = crate::rules::retail_ini_fixture::retail_battle_rules() else {
            return;
        };
        let akm = retail.rules.weapon("AKM").unwrap();
        assert_eq!(akm.assault_anim.as_deref(), Some("UCBLOOD"));
        assert!(akm.down_report.is_empty(), "constructor list is empty");

        let electric_bolt = retail.rules.weapon("CRElectricBolt").unwrap();
        assert_eq!(electric_bolt.assault_anim.as_deref(), Some("UCELEC"));
        assert!(
            electric_bolt.down_report.is_empty(),
            "stock has no DownReport"
        );
    }

    #[test]
    fn assault_anim_keeps_current_on_empty_and_uses_the_native_factory() {
        use crate::rules::native_processing::{RulesLayerKind, RulesLayerStack};
        use crate::rules::ruleset::RuleSet;

        let mut layers = RulesLayerStack::new(IniFile::from_str(
            "[VehicleTypes]\n0=KEEP_MISSING\n1=KEEP_EMPTY\n2=CLEAR\n3=CREATE\n\
             [KEEP_MISSING]\nPrimary=KeepMissing\n[KEEP_EMPTY]\nPrimary=KeepEmpty\n\
             [CLEAR]\nPrimary=Clear\n[CREATE]\nPrimary=Create\n\
             [KeepMissing]\nDamage=1\nAssaultAnim=UCFLASH\n\
             [KeepEmpty]\nDamage=1\nAssaultAnim=UCFLASH\n\
             [Clear]\nDamage=1\nAssaultAnim=UCFLASH\n\
             [Create]\nDamage=1\nAssaultAnim=UCFLASH\n",
        ));
        layers.push(
            RulesLayerKind::Scenario,
            IniFile::from_str(
                "[KEEP_MISSING]\nSecondary=KeepMissing\n\
                 [KeepMissing]\nDamage=1\n\
                 [KeepEmpty]\nAssaultAnim=\n\
                 [Clear]\nAssaultAnim=none\n\
                 [Create]\nAssaultAnim=LateAnim\n",
            ),
        );
        let processed = layers.process().unwrap();
        let rules = RuleSet::from_processed_rules(&processed).unwrap();

        assert_eq!(
            rules.weapon("KeepMissing").unwrap().assault_anim.as_deref(),
            Some("UCFLASH"),
            "a later missing key retains the current animation"
        );
        assert_eq!(
            rules.weapon("KeepEmpty").unwrap().assault_anim.as_deref(),
            Some("UCFLASH"),
            "an empty ReadString result retains the current animation"
        );
        assert_eq!(rules.weapon("Clear").unwrap().assault_anim, None);
        assert_eq!(
            rules.weapon("Create").unwrap().assault_anim.as_deref(),
            Some("LateAnim"),
            "the native factory allocates an unknown nonempty name"
        );
    }

    #[test]
    fn weapon_report_lists_use_the_sound_registry_filter_and_native_reset_rules() {
        use crate::rules::native_processing::{RulesLayerKind, RulesLayerStack};
        use crate::rules::ruleset::RuleSet;
        use crate::rules::sound_ini::SoundRegistry;

        let mut layers = RulesLayerStack::new(IniFile::from_str(
            "[VehicleTypes]\n0=UNIT\n[UNIT]\nPrimary=Filtered\nSecondary=Cleared\n\
             [Filtered]\nDamage=1\nReport=A,Unknown,<none>,B,A\n\
             DownReport=A,Unknown,<none>,B,A\n\
             [Cleared]\nDamage=1\nReport=A,B\nDownReport=A,B\n",
        ));
        layers.push(
            RulesLayerKind::GameMode,
            IniFile::from_str("[Filtered]\nReport=\nDownReport=\n"),
        );
        layers.push(
            RulesLayerKind::Scenario,
            IniFile::from_str("[Cleared]\nReport=Unknown,<none>\nDownReport=,,,\n"),
        );

        let processed = layers.process().unwrap();
        let mut rules = RuleSet::from_processed_rules(&processed).unwrap();
        let sounds = SoundRegistry::from_ini(&IniFile::from_str(
            "[SoundList]\n0=A\n1=B\n[A]\nSounds=a.wav\n[B]\nSounds=b.wav\n",
        ));
        rules.bind_type_sound_references(processed.ini(), &sounds);

        let filtered = rules.weapon("Filtered").unwrap();
        assert_eq!(
            filtered
                .report
                .iter()
                .map(String::as_str)
                .collect::<Vec<_>>(),
            vec!["A", "B", "A"],
            "registered sounds retain native order and duplicate entries"
        );
        assert_eq!(
            filtered
                .down_report
                .iter()
                .map(String::as_str)
                .collect::<Vec<_>>(),
            vec!["A", "B", "A"]
        );
        assert_eq!(filtered.report_count(), 3);

        let cleared = rules.weapon("Cleared").unwrap();
        assert_eq!(
            cleared.report_count(),
            0,
            "unknown-only replaces with empty"
        );
        assert!(
            cleared.down_report.is_empty(),
            "comma-only replaces with empty"
        );
    }

    #[test]
    fn weapon_fire_flags_keep_read_bool_defaults_across_rules_passes() {
        use crate::rules::native_processing::{RulesLayerKind, RulesLayerStack};
        use crate::rules::ruleset::RuleSet;

        let mut layers = RulesLayerStack::new(IniFile::from_str(
            "[VehicleTypes]\n0=UNIT\n[UNIT]\nPrimary=Probe\nSecondary=Defaults\n\
             [Probe]\nDistributedWeaponFire=yes\nDisguiseFireOnly=yes\n\
             [Defaults]\nDamage=1\n",
        ));
        layers.push(
            RulesLayerKind::GameMode,
            IniFile::from_str("[Probe]\nDistributedWeaponFire=on\nDisguiseFireOnly=no\n"),
        );
        layers.push(
            RulesLayerKind::Scenario,
            IniFile::from_str("[Probe]\nDisguiseFireOnly=on\n"),
        );

        let rules = RuleSet::from_rules_layers(&layers).unwrap();
        let probe = rules.weapon("Probe").unwrap();
        assert!(
            probe.distributed_weapon_fire,
            "invalid 'on' keeps the current true value"
        );
        assert!(
            !probe.disguise_fire_only,
            "valid no followed by invalid 'on' keeps the current false value"
        );
        let defaults = rules.weapon("Defaults").unwrap();
        assert!(!defaults.distributed_weapon_fire);
        assert!(!defaults.disguise_fire_only);
    }

    #[test]
    fn weapon_fire_flags_retail_battle_defaults_and_map_overrides() {
        let Some(stock) = crate::rules::retail_ini_fixture::retail_battle_rules() else {
            return;
        };
        for id in ["105mm", "MirageGun", "MirageGunE"] {
            let weapon = stock.rules.weapon(id).expect("stock WeaponType");
            assert!(!weapon.distributed_weapon_fire, "{id}");
            assert!(!weapon.disguise_fire_only, "{id}");
        }

        let patch = IniFile::from_str("[105mm]\nDistributedWeaponFire=yes\nDisguiseFireOnly=yes\n");
        let Some(patched) =
            crate::rules::retail_ini_fixture::retail_battle_rules_with_map_patch(&patch)
        else {
            return;
        };
        let weapon = patched.rules.weapon("105mm").unwrap();
        assert!(weapon.distributed_weapon_fire);
        assert!(weapon.disguise_fire_only);
    }

    /// `CCINIClass::ReadRange` 0x00474620 multiplies by `256.0` and THEN chops,
    /// so a fractional `Range=` keeps its fraction. 60 of the 256 stock
    /// `Range=` weapons carry one and 21 sit at exactly `1.5`, the stock
    /// `[BlimpBomb]` among them. Its `-1` sentinel (`0x0047463E`) returns the
    /// default, the field's current value: the 0x00771CB6 constructor zero, or
    /// what an earlier reached pass read.
    #[test]
    fn fractional_range_keeps_its_fraction_in_leptons() {
        use crate::rules::native_processing::{RulesLayerKind, RulesLayerStack};
        use crate::rules::ruleset::RuleSet;

        let mut layers = RulesLayerStack::new(IniFile::from_str(
            "[VehicleTypes]\n0=A\n1=B\n2=C\n3=D\n4=E\n5=F\n\
             [A]\nPrimary=BlimpBomb\n[B]\nPrimary=Nuke\n[C]\nPrimary=Quarter\n\
             [D]\nPrimary=NoKey\n[E]\nPrimary=Unset\n[F]\nPrimary=Kept\n\
             [BlimpBomb]\nDamage=250\nROF=50\nRange=1.5\nCellRangefinding=yes\n\
             [Nuke]\nRange=-2\n\
             [Quarter]\nRange=0.25\n\
             [NoKey]\nDamage=1\n\
             [Unset]\nRange=-1\n\
             [Kept]\nRange=3\n",
        ));
        layers.push(
            RulesLayerKind::Scenario,
            IniFile::from_str("[Kept]\nRange=-1\n"),
        );
        let rules = RuleSet::from_rules_layers(&layers).unwrap();
        let range = |id: &str| rules.weapon(id).unwrap().range_leptons;

        assert_eq!(range("BlimpBomb"), 384, "1.5 cells is 384 leptons, not 256");
        assert!(rules.weapon("BlimpBomb").unwrap().cell_rangefinding);
        assert_eq!(
            range("Nuke"),
            -512,
            "the -2 'infinite' sentinel scales to the -512 that 0x006F724E tests"
        );
        assert_eq!(range("Quarter"), 64);
        assert_eq!(range("NoKey"), 0, "absent key keeps the constructor zero");
        assert_eq!(rules.weapon("NoKey").unwrap().minimum_range_leptons, 0);
        assert_eq!(range("Unset"), 0, "-1 keeps the constructor zero");
        assert_eq!(range("Kept"), 3 * 256, "-1 keeps the earlier pass's range");
    }
}
