//! `[VoxelAnims]` type data — the flying debris a vehicle or building throws
//! when it dies.
//!
//! gamemd-derived: `VoxelAnimTypeClass::Constructor @ 0x0074AD80` and
//! `VoxelAnimTypeClass::ReadINI @ 0x0074B050`. This is a different class from
//! `AnimClass`: `AnimClass` draws SHP sprites, `VoxelAnimClass` draws a VXL
//! model carried by a `BounceClass` physics body. `sim::components::
//! VoxelAnimation` is a third, unrelated thing — a per-entity HVA frame cursor.
//!
//! ## Dependency rules
//! - Part of rules/ — depends only on the INI layer and util/.
//! - Never depends on sim/, render/, ui/, sidebar/, audio/, net/.

use serde::{Deserialize, Serialize};

use crate::rules::ini_parser::IniSection;
use crate::util::native_x87::NativeF64Bits;

/// Index into `RuleSet`'s voxel-anim type table.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct VoxelAnimTypeId(pub u32);

/// `pi / 180`, the degrees-to-radians factor at `0x007F65E8` that
/// `VoxelAnimTypeClass::ReadINI` applies to both angular-velocity keys. Carried
/// as bits: the stored constant is not the correctly-rounded `pi/180`, and the
/// spin it drives is compared against authored thresholds.
pub const DEGREES_TO_RADIANS: NativeF64Bits = NativeF64Bits::from_bits(0x3f91_df46_a245_2b7c);

/// Constructor defaults, all read from `VoxelAnimTypeClass::Constructor @
/// 0x0074AD80`. Named rather than inlined because several are load-bearing for
/// types that omit the key — `[TIRE]` omits `Damage`, every stock type omits
/// `VoxelIndex`.
const DEFAULT_DURATION: i32 = 30;
const DEFAULT_ELASTICITY: f64 = 0.8;
const DEFAULT_MIN_ANGULAR_VELOCITY_DEGREES: f64 = 0.0;
const DEFAULT_MAX_ANGULAR_VELOCITY_DEGREES: f64 = 10.0;
const DEFAULT_MIN_Z_VEL: f64 = 3.5;
const DEFAULT_MAX_Z_VEL: f64 = 5.0;
const DEFAULT_MAX_XY_VEL: f64 = 15.0;

/// One `[VoxelAnims]` entry.
///
/// Doubles are carried as `NativeF64Bits` rather than `SimFixed` because
/// `BounceClass` integrates them at double precision and the bounce-stop
/// threshold is an exact compare; quantising them here would change how many
/// times a piece of debris bounces.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VoxelAnimType {
    pub name: String,
    /// `+0x294`. Normalise voxel colours on draw.
    pub normalized: bool,
    /// `+0x295`. Draw with alpha blending.
    pub translucent: bool,
    /// `+0x300`. Meteor-style: lays ore or a crater on impact.
    pub is_tiberium: bool,
    /// `+0x2D0`. Spawns high and descends instead of being thrown from a wreck.
    pub is_meteor: bool,
    /// `+0x298`. Section index inside the VXL model.
    pub voxel_index: i32,
    /// `+0x29C`. Ticks the debris lives before expiring.
    pub duration: i32,
    /// `+0x2A0`. Bounce coefficient: 0 stops dead, 1 is perfectly elastic.
    pub elasticity: NativeF64Bits,
    /// `+0x2A8`, radians per tick — the INI authors degrees.
    pub min_angular_velocity: NativeF64Bits,
    /// `+0x2B0`, radians per tick.
    pub max_angular_velocity: NativeF64Bits,
    /// `+0x2B8`. Lower bound of the upward launch speed.
    pub min_z_vel: NativeF64Bits,
    /// `+0x2C0`. Upper bound of the upward launch speed.
    pub max_z_vel: NativeF64Bits,
    /// `+0x2C8`. Bound of the horizontal scatter speed.
    pub max_xy_vel: NativeF64Bits,
    /// `+0x2D4`. Child type spawned on impact — `[METEOR01] Spawns=PEBBLE`.
    pub spawns: Option<String>,
    /// `+0x2D8`. How many children.
    pub spawn_count: i32,
    /// `+0x2DC` / `+0x2E0`. Sound names; native stores `VocClass` indices with
    /// `-1` for absent.
    pub start_sound: Option<String>,
    pub stop_sound: Option<String>,
    /// `+0x2E4`. `AnimType` played on every ground bounce.
    pub bounce_anim: Option<String>,
    /// `+0x2E8`. `AnimType` played when `Duration` runs out.
    pub expire_anim: Option<String>,
    /// `+0x2EC`. `AnimType` played every other tick while alive.
    pub trailer_anim: Option<String>,
    /// `+0x2F0` / `+0x2F4` / `+0x2F8`. Area damage applied on expiry.
    pub damage: i32,
    pub damage_radius: i32,
    pub warhead: Option<String>,
    /// `+0x2FC`. Particle system carried while alive.
    pub attached_system: Option<String>,
    /// The `ShareBodyData`/`ShareTurretData`/`ShareBarrelData` + `ShareSource`
    /// group. All three destinations are the single body VXL slot
    /// (`+0xB0`/`+0xB4`) — the type has only one model — so only the SOURCE
    /// offset differs between them.
    pub share_source: Option<String>,
    pub share_body_data: bool,
    pub share_turret_data: bool,
    pub share_barrel_data: bool,
}

impl VoxelAnimType {
    /// `VoxelAnimTypeClass::Constructor @ 0x0074AD80` — the state a type has
    /// before `ReadINI` touches it.
    pub fn with_defaults(name: &str) -> Self {
        Self {
            name: name.to_string(),
            normalized: false,
            translucent: false,
            is_tiberium: false,
            is_meteor: false,
            voxel_index: 0,
            duration: DEFAULT_DURATION,
            elasticity: NativeF64Bits::from_bits(DEFAULT_ELASTICITY.to_bits()),
            min_angular_velocity: radians_from_degrees(DEFAULT_MIN_ANGULAR_VELOCITY_DEGREES),
            max_angular_velocity: radians_from_degrees(DEFAULT_MAX_ANGULAR_VELOCITY_DEGREES),
            min_z_vel: NativeF64Bits::from_bits(DEFAULT_MIN_Z_VEL.to_bits()),
            max_z_vel: NativeF64Bits::from_bits(DEFAULT_MAX_Z_VEL.to_bits()),
            max_xy_vel: NativeF64Bits::from_bits(DEFAULT_MAX_XY_VEL.to_bits()),
            spawns: None,
            spawn_count: 0,
            start_sound: None,
            stop_sound: None,
            bounce_anim: None,
            expire_anim: None,
            trailer_anim: None,
            damage: 0,
            damage_radius: 0,
            warhead: None,
            attached_system: None,
            share_source: None,
            share_body_data: false,
            share_turret_data: false,
            share_barrel_data: false,
        }
    }

    /// `VoxelAnimTypeClass::ReadINI @ 0x0074B050`.
    pub fn from_ini_section(name: &str, section: &IniSection) -> Self {
        let mut out = Self::with_defaults(name);
        out.normalized = section.get_bool("Normalized").unwrap_or(out.normalized);
        out.translucent = section.get_bool("Translucent").unwrap_or(out.translucent);
        out.is_tiberium = section.get_bool("IsTiberium").unwrap_or(out.is_tiberium);
        out.is_meteor = section.get_bool("IsMeteor").unwrap_or(out.is_meteor);
        out.voxel_index = section.get_i32("VoxelIndex").unwrap_or(out.voxel_index);
        out.duration = section.get_i32("Duration").unwrap_or(out.duration);

        out.elasticity = read_double_bits(section, "Elasticity", out.elasticity);
        out.min_angular_velocity =
            read_angular_velocity(section, "MinAngularVelocity", out.min_angular_velocity);
        out.max_angular_velocity =
            read_angular_velocity(section, "MaxAngularVelocity", out.max_angular_velocity);
        out.min_z_vel = read_double_bits(section, "MinZVel", out.min_z_vel);
        out.max_z_vel = read_double_bits(section, "MaxZVel", out.max_z_vel);
        out.max_xy_vel = read_double_bits(section, "MaxXYVel", out.max_xy_vel);

        out.spawns = read_name(section, "Spawns");
        out.spawn_count = section.get_i32("SpawnCount").unwrap_or(out.spawn_count);
        out.start_sound = read_name(section, "StartSound");
        out.stop_sound = read_name(section, "StopSound");
        out.bounce_anim = read_name(section, "BounceAnim");
        out.expire_anim = read_name(section, "ExpireAnim");
        out.trailer_anim = read_name(section, "TrailerAnim");
        out.damage = section.get_i32("Damage").unwrap_or(out.damage);
        out.damage_radius = section.get_i32("DamageRadius").unwrap_or(out.damage_radius);
        out.warhead = read_name(section, "Warhead");
        out.attached_system = read_name(section, "AttachedSystem");

        out.share_source = read_name(section, "ShareSource");
        out.share_body_data = section.get_bool("ShareBodyData").unwrap_or(false);
        out.share_turret_data = section.get_bool("ShareTurretData").unwrap_or(false);
        out.share_barrel_data = section.get_bool("ShareBarrelData").unwrap_or(false);
        out
    }

    /// `+0x296`, which the constructor computes rather than reads.
    #[cfg(test)]
    pub fn shares_data(&self) -> bool {
        self.share_body_data || self.share_turret_data || self.share_barrel_data
    }
}

fn read_name(section: &IniSection, key: &str) -> Option<String> {
    section
        .get(key)
        .map(str::trim)
        .filter(|value| !value.is_empty() && !value.eq_ignore_ascii_case("none"))
        .map(str::to_string)
}

fn read_double_bits(section: &IniSection, key: &str, default: NativeF64Bits) -> NativeF64Bits {
    NativeF64Bits::from_bits(
        section
            .read_double(key, f64::from_bits(default.bits()))
            .to_bits(),
    )
}

/// The two angular-velocity keys, including native's sentinel bug.
///
/// `ReadINI` calls `ReadDouble` with a `-1.0` default and then compares the
/// result against `0.0` (`0x007E2800`), not against `-1.0`. So an ABSENT key
/// yields `-1.0`, which is not zero, and native stores `-1.0 * pi/180`
/// — a small negative spin — instead of keeping the constructor default. Only
/// an explicitly authored `0` retains it. The comparison was plainly meant to
/// test the sentinel, and reproducing the bug rather than the intent is the
/// point: every stock `[VoxelAnims]` type authors both keys, so the arm is
/// dormant in stock, but a mod that omits one gets native's answer here.
fn read_angular_velocity(section: &IniSection, key: &str, default: NativeF64Bits) -> NativeF64Bits {
    const ABSENT_SENTINEL: f64 = -1.0;
    let raw = section.read_double(key, ABSENT_SENTINEL);
    if raw == 0.0 {
        return default;
    }
    radians_from_degrees(raw)
}

fn radians_from_degrees(degrees: f64) -> NativeF64Bits {
    NativeF64Bits::from_bits((degrees * f64::from_bits(DEGREES_TO_RADIANS.bits())).to_bits())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rules::ini_parser::IniFile;

    fn section(text: &str) -> IniFile {
        IniFile::from_str(text)
    }

    #[test]
    fn defaults_match_the_constructor() {
        let vat = VoxelAnimType::with_defaults("EMPTY");
        assert_eq!(vat.duration, 30);
        assert_eq!(f64::from_bits(vat.elasticity.bits()), 0.8);
        assert_eq!(f64::from_bits(vat.min_z_vel.bits()), 3.5);
        assert_eq!(f64::from_bits(vat.max_z_vel.bits()), 5.0);
        assert_eq!(f64::from_bits(vat.max_xy_vel.bits()), 15.0);
        assert!(!vat.is_meteor && !vat.is_tiberium);
        assert_eq!(vat.spawn_count, 0);
        // 10 degrees, converted with the stored pi/180.
        let expected = 10.0 * f64::from_bits(DEGREES_TO_RADIANS.bits());
        assert_eq!(f64::from_bits(vat.max_angular_velocity.bits()), expected);
    }

    #[test]
    fn stock_piece_section_parses() {
        // Verbatim from `rulesmd.ini` `[PIECE]`, the scrap thrown by a dying
        // vehicle — the only stock VoxelAnim with an expiry warhead.
        let ini = section(
            "[PIECE]\nName=Scrap Metal Debris\nElasticity=0\n\
             MinAngularVelocity=5.0\nMaxAngularVelocity=9.0\n\
             MinZVel=24.0\nMaxZVel=28.0\nMaxXYVel=15.0\n\
             Duration=75\nDamage=5\nExpireAnim=TWLT036\n\
             DamageRadius=100\nWarhead=TankOGas\n",
        );
        let vat = VoxelAnimType::from_ini_section("PIECE", ini.section("PIECE").unwrap());
        assert_eq!(f64::from_bits(vat.elasticity.bits()), 0.0);
        assert_eq!(vat.duration, 75);
        assert_eq!(vat.damage, 5);
        assert_eq!(vat.damage_radius, 100);
        assert_eq!(vat.warhead.as_deref(), Some("TankOGas"));
        assert_eq!(vat.expire_anim.as_deref(), Some("TWLT036"));
        assert_eq!(f64::from_bits(vat.min_z_vel.bits()), 24.0);
        assert_eq!(
            f64::from_bits(vat.min_angular_velocity.bits()),
            5.0 * f64::from_bits(DEGREES_TO_RADIANS.bits())
        );
        assert!(vat.bounce_anim.is_none() && vat.spawns.is_none());
    }

    #[test]
    fn absent_angular_velocity_takes_the_engines_sentinel_bug_not_the_default() {
        // `ReadDouble(key, -1.0)` then `if (result == 0.0) keep default`. An
        // absent key returns -1.0, which is not 0.0, so native stores
        // `-1.0 * pi/180` rather than the constructor's 0 / 10 degrees.
        let ini = section("[NOSPIN]\nDuration=10\n");
        let vat = VoxelAnimType::from_ini_section("NOSPIN", ini.section("NOSPIN").unwrap());
        let expected = -f64::from_bits(DEGREES_TO_RADIANS.bits());
        assert_eq!(f64::from_bits(vat.min_angular_velocity.bits()), expected);
        assert_eq!(f64::from_bits(vat.max_angular_velocity.bits()), expected);

        // An explicitly authored zero is the one value that keeps the default.
        let ini = section("[ZEROSPIN]\nMinAngularVelocity=0\nMaxAngularVelocity=0\n");
        let vat = VoxelAnimType::from_ini_section("ZEROSPIN", ini.section("ZEROSPIN").unwrap());
        assert_eq!(f64::from_bits(vat.min_angular_velocity.bits()), 0.0);
        assert_eq!(
            f64::from_bits(vat.max_angular_velocity.bits()),
            10.0 * f64::from_bits(DEGREES_TO_RADIANS.bits())
        );
    }

    #[test]
    fn share_group_is_read_and_folded() {
        // `[SONICTURRET]` borrows the Sonic Tank's turret model rather than
        // loading a standalone VXL.
        let ini = section("[SONICTURRET]\nShareTurretData=yes\nShareSource=SONIC\n");
        let vat =
            VoxelAnimType::from_ini_section("SONICTURRET", ini.section("SONICTURRET").unwrap());
        assert!(vat.share_turret_data);
        assert!(!vat.share_body_data && !vat.share_barrel_data);
        assert_eq!(vat.share_source.as_deref(), Some("SONIC"));
        assert!(vat.shares_data());
    }
}
