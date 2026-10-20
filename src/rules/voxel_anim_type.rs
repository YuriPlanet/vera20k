//! `[VoxelAnims]` type data — the flying debris a vehicle or building throws
//! when it dies.
//!
//! gamemd-derived: `VoxelAnimTypeClass::Constructor @ 0x0074AD80` and
//! `VoxelAnimTypeClass::ReadINI @ 0x0074B050`. This is a different class from
//! `AnimClass`: `AnimClass` draws SHP sprites, `VoxelAnimClass` draws a VXL
//! model carried by a `BounceClass` physics body.
//!
//! ## Dependency rules
//! - Part of rules/ — depends only on the INI layer and util/.
//! - Never depends on sim/, render/, ui/, audio/, net/.

use serde::{Deserialize, Serialize};

use crate::rules::ini_parser::IniSection;
use crate::util::native_x87::{NativeF64Bits, X87Chop53};

/// Index into `RuleSet`'s voxel-anim type table.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct VoxelAnimTypeId(pub u32);

/// The degrees-to-radians factor read by VoxelAnimType74B128/74B159 from
/// file-backed 7F65E8: bytes `05 A1 1E 17 24 DF 91 3F`. It differs from
/// mathematical pi/180. Native reader/constructor comparisons consume its
/// resulting TIRE angular bits in tools/projectile_oracle/typed_warhead_debris.
pub const DEGREES_TO_RADIANS: NativeF64Bits = NativeF64Bits::from_bits(0x3f91_df24_171e_a105);

/// Constructor defaults, all read from `VoxelAnimTypeClass::Constructor @
/// 0x0074AD80`. Named rather than inlined because several are load-bearing for
/// types that omit the key — `[TIRE]` omits `Damage`, every stock type omits
/// `VoxelIndex`.
const DEFAULT_DURATION: i32 = 30;
/// `(double)0.8f`, stored as `0x3FE99999_A0000000` (`0x0074ADB4`, `0x0074ADF1`).
const DEFAULT_ELASTICITY: NativeF64Bits = NativeF64Bits::from_bits(0x3fe9_9999_a000_0000);
/// Zero (`0x0074ADBE`, `0x0074ADFB`).
const DEFAULT_MIN_ANGULAR_VELOCITY: NativeF64Bits = NativeF64Bits::POSITIVE_ZERO;
/// Constructor literal `0x3FC656ED_1CE64946`, written at74ADC4/74AE01.
/// The reader's separate degree conversion must use its own file constant.
const DEFAULT_MAX_ANGULAR_VELOCITY: NativeF64Bits = NativeF64Bits::from_bits(0x3fc6_56ed_1ce6_4946);
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
    /// `ImageFile` (`+0x1F8`): the type's ID until `ObjectTypeClass::ReadINI`
    /// reads `Image=` over it (`0x005F92F8..0x005F933B`, 25 bytes, the current
    /// value as default). `ReadINI` loads the model from it, `%s.VXL` with no
    /// HVA (`0x0074B56C..0x0074B5DB`); the `Share*Data=` arms copy another
    /// type's model instead.
    pub image: String,
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
            image: name.to_string(),
            normalized: false,
            translucent: false,
            is_tiberium: false,
            is_meteor: false,
            voxel_index: 0,
            duration: DEFAULT_DURATION,
            elasticity: DEFAULT_ELASTICITY,
            min_angular_velocity: DEFAULT_MIN_ANGULAR_VELOCITY,
            max_angular_velocity: DEFAULT_MAX_ANGULAR_VELOCITY,
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

    /// `VoxelAnimTypeClass::ReadINI @ 0x0074B050`, after the base
    /// `ObjectTypeClass::ReadINI` (`0x0074B06E`).
    pub fn from_ini_section(name: &str, section: &IniSection) -> Self {
        let mut out = Self::with_defaults(name);
        out.image = section.read_string("Image", &out.image, 25);
        out.normalized = section.read_bool("Normalized", out.normalized);
        out.translucent = section.read_bool("Translucent", out.translucent);
        out.is_tiberium = section.read_bool("IsTiberium", out.is_tiberium);
        out.is_meteor = section.read_bool("IsMeteor", out.is_meteor);
        out.voxel_index = section.read_int("VoxelIndex", out.voxel_index);
        out.duration = section.read_int("Duration", out.duration);

        out.elasticity = section.read_double_bits("Elasticity", out.elasticity);
        out.min_angular_velocity =
            read_angular_velocity(section, "MinAngularVelocity", out.min_angular_velocity);
        out.max_angular_velocity =
            read_angular_velocity(section, "MaxAngularVelocity", out.max_angular_velocity);
        out.min_z_vel = section.read_double_bits("MinZVel", out.min_z_vel);
        out.max_z_vel = section.read_double_bits("MaxZVel", out.max_z_vel);
        out.max_xy_vel = section.read_double_bits("MaxXYVel", out.max_xy_vel);

        // ReadString 0x80 ahead of each type or sound lookup
        // (`0x0074B207`-`0x0074B4C1`). The Type references (`Spawns=`, `BounceAnim=`,
        // `ExpireAnim=`, `TrailerAnim=`, `Warhead=`, `AttachedSystem=`) keep
        // the constructor's nulls: only the ordered Process owner reads them
        // (`VoxelAnimTypeReferences`), and `RuleSet` publishes them.
        out.spawn_count = section.read_int("SpawnCount", out.spawn_count);
        out.start_sound = section
            .read_type_name("StartSound", 0x80)
            .map(str::to_string);
        out.stop_sound = section
            .read_type_name("StopSound", 0x80)
            .map(str::to_string);
        out.damage = section.read_int("Damage", out.damage);
        out.damage_radius = section.read_int("DamageRadius", out.damage_radius);

        out.share_source = section
            .read_type_name("ShareSource", 0x80)
            .map(str::to_string);
        out.share_body_data = section.read_bool("ShareBodyData", false);
        out.share_turret_data = section.read_bool("ShareTurretData", false);
        out.share_barrel_data = section.read_bool("ShareBarrelData", false);
        out
    }

    /// `+0x296`, which ReadINI computes (`0x0074B48A..0x0074B4AD`) rather
    /// than reads. When set, ReadINI takes the `ShareSource=` type's model, or
    /// none when that type is missing (`0x0074B555`), and never loads
    /// `%s.VXL`.
    pub fn shares_data(&self) -> bool {
        self.share_body_data || self.share_turret_data || self.share_barrel_data
    }
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

/// `FMUL qword [0x007F65E8]` / `FSTP qword` under the process's chop mode.
fn radians_from_degrees(degrees: f64) -> NativeF64Bits {
    let degrees = X87Chop53::load_f64(NativeF64Bits::from_bits(degrees.to_bits()))
        .expect("ReadDouble returns a finite double");
    X87Chop53::store_f64(X87Chop53::mul(
        degrees,
        X87Chop53::load_f64(DEGREES_TO_RADIANS).expect("pi/180 is finite"),
    ))
    .expect("an INI angle in degrees stays finite in radians")
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
        assert_eq!(f64::from_bits(vat.elasticity.bits()), f64::from(0.8f32));
        assert_eq!(f64::from_bits(vat.min_z_vel.bits()), 3.5);
        assert_eq!(f64::from_bits(vat.max_z_vel.bits()), 5.0);
        assert_eq!(f64::from_bits(vat.max_xy_vel.bits()), 15.0);
        assert!(!vat.is_meteor && !vat.is_tiberium);
        assert_eq!(vat.spawn_count, 0);
        // Literal stores at74ADC4/74AE01 establish the constructor default.
        assert_eq!(vat.max_angular_velocity.bits(), 0x3fc6_56ed_1ce6_4946);
    }

    #[test]
    fn stock_piece_section_parses() {
        // Verbatim from `rulesmd.ini` `[PIECE]`, the scrap thrown by a dying
        // vehicle — the only stock VoxelAnim with an expiry warhead.
        // The Type references come from the ordered Rules processor, which
        // `RuleSet` publishes.
        let rules = crate::rules::ruleset::RuleSet::from_ini(&section(
            "[VoxelAnims]\n0=PIECE\n\
             [PIECE]\nName=Scrap Metal Debris\nElasticity=0\n\
             MinAngularVelocity=5.0\nMaxAngularVelocity=9.0\n\
             MinZVel=24.0\nMaxZVel=28.0\nMaxXYVel=15.0\n\
             Duration=75\nDamage=5\nExpireAnim=TWLT036\n\
             DamageRadius=100\nWarhead=TankOGas\n",
        ))
        .unwrap();
        let vat = rules.voxel_anim_type(rules.voxel_anim_type_id_by_name("PIECE").unwrap());
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
        // No `Image=`: the model is named by the ID.
        assert_eq!(vat.image, "PIECE");
    }

    #[test]
    fn retail_images_name_the_model_files() {
        let Some(ini) = crate::rules::retail_ini_fixture::retail_ini("rulesmd.ini") else {
            return;
        };
        let image =
            |name: &str| VoxelAnimType::from_ini_section(name, ini.section(name).unwrap()).image;
        // The thrown tyre keeps its ID; the meteors and crystals borrow models.
        assert_eq!(image("TIRE"), "TIRE");
        assert_eq!(image("METEOR01"), "MTRS");
        assert_eq!(image("PEBBLE"), "MTRB");
        assert_eq!(image("CRYSTAL02"), "GASTANK");
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
        assert_eq!(vat.max_angular_velocity, DEFAULT_MAX_ANGULAR_VELOCITY);
    }
}
