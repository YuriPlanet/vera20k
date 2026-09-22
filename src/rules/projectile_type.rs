//! Projectile type definitions parsed from rules.ini.
//!
//! Each projectile in RA2 has its own `[ProjectileName]` section in rules.ini,
//! defining targeting capabilities (AA/AG), flight behavior (arcing,
//! homing turn rate), and accuracy. Weapons reference projectiles via their
//! `Projectile=` key.
//!
//! ## Targeting flags
//! - `AA=yes` — projectile can hit aircraft (anti-air)
//! - `AG=yes` — projectile can hit ground units and buildings (default true)
//!
//! These flags determine which weapon (Primary vs Secondary) a unit uses
//! against a given target type. A typical tank has AG=yes Primary and
//! AA=yes Secondary, enabling automatic weapon switching.
//!
//! ## Dependency rules
//! - Part of rules/ — no dependencies on sim/, render/, ui/, etc.

use crate::rules::ini_parser::IniSection;
use fixed::types::I8F8;

/// A projectile definition parsed from a rules.ini section.
///
/// Projectiles define how a weapon's shot travels and what it can target.
/// The targeting flags (AA/AG) are critical for weapon selection: if a
/// projectile can't hit the target type, the combat system falls back to
/// the unit's Secondary weapon.
///
/// Fields are mapped from the verified BulletTypeClass::ReadINI behavior.
#[derive(Debug, Clone)]
pub struct ProjectileType {
    /// Section name in rules.ini (e.g., "InvisibleLow", "MissileAA").
    pub id: String,

    // --- Targeting flags ---
    /// Can hit aircraft. Anti-air weapons need this flag.
    pub aa: bool,
    /// Can hit ground units and buildings. True by default for most projectiles.
    pub ag: bool,

    // --- Flight behavior ---
    /// Ballistic arc trajectory (e.g., artillery shells). Cannot be intercepted.
    pub arcing: bool,
    /// Homing missile turn rate (higher = tighter tracking). 0 = no homing.
    pub rot: i32,
    /// Random spread on impact (e.g., rapid-fire infantry weapons).
    pub inaccurate: bool,

    // --- Bool flags (offsets from BulletTypeClass base) ---
    /// Projectile explodes in the air, releasing sub-munitions. (+0x294)
    pub airburst: bool,
    /// Projectile floats above water instead of splashing. (+0x295)
    pub floater: bool,
    /// Trajectory affected by cliff elevation changes. (+0x296)
    pub subject_to_cliffs: bool,
    /// Trajectory affected by terrain elevation. (+0x297)
    pub subject_to_elevation: bool,
    /// Trajectory blocked by walls. (+0x298)
    pub subject_to_walls: bool,
    /// Flies at very high altitude (e.g., V3 rocket). (+0x299)
    pub very_high: bool,
    /// Casts a shadow on the ground while in flight. (+0x29A)
    pub shadow: bool,
    /// Projectile drops from above (e.g., paratroopers, bombs). (+0x29C)
    pub dropping: bool,
    /// Uses level flight trajectory. (+0x29D)
    pub level: bool,
    /// Invisible projectile (no sprite drawn). (+0x29E)
    pub inviso: bool,
    /// Detonates near the target rather than on contact. (+0x29F)
    pub proximity: bool,
    /// Has a maximum range (non-homing). (+0x2A0)
    pub ranged: bool,
    /// Inverted binary storage for the art `Rotates=` key. (+0x2A1)
    /// `Rotates=yes` stores false, `Rotates=no` stores true.
    pub rotates: bool,
    /// Scatters like flak — random spread pattern for AA. (+0x2A3)
    pub flak_scatter: bool,
    /// Projectile loses health over time. (+0x2A6)
    pub degenerates: bool,
    /// Bounces off terrain on impact. (+0x2A7)
    pub bouncy: bool,
    /// Uses a custom animation palette instead of the unit palette. (+0x2A8)
    pub anim_palette: bool,
    /// Uses the firing unit's palette for rendering. (+0x2A9)
    pub firers_palette: bool,
    /// Projectile can be scaled (e.g., for perspective). (+0x2EC)
    pub scalable: bool,
    /// Launches vertically before turning toward target. (+0x2C0)
    pub vertical: bool,
    /// ART Flat (+0x2F7), retained by the per-pass native rules processor.
    /// BulletClass::GetLayer 0x00468B90 selects Surface when true, Air otherwise.
    pub flat: bool,

    // --- Integer fields ---
    /// Number of sub-projectiles on detonation. (+0x2AC)
    pub cluster: i32,
    /// Number of shrapnel fragments on impact. (+0x2B8)
    pub shrapnel_count: i32,
    /// Altitude at which the projectile detonates (for airburst). (+0x2BC)
    pub detonation_altitude: i32,
    /// Acceleration rate for missiles. (+0x2D0)
    pub acceleration: i32,
    /// Number of frames the projectile flies straight before homing. (+0x2E0)
    pub course_lock_duration: i32,
    /// Delay in frames between spawning sub-projectiles (read from Image section). (+0x2E4)
    pub spawn_delay: i32,
    /// Signed proximity-fuse delay (+0x2F0); collisions bypass it and Aircraft targets use zero.
    pub arm: i32,
    /// Lowest animation frame index for in-flight animation. (+0x2F4)
    pub anim_low: i32,
    /// Highest animation frame index for in-flight animation. (+0x2F5)
    pub anim_high: i32,
    /// Animation rate (frames between animation steps). (+0x2F6)
    pub anim_rate: i32,

    // --- Float fields ---
    /// Bounce elasticity coefficient (0.0 = no bounce, 1.0 = perfect bounce). (+0x2C8)
    pub elasticity: f64,

    // --- Color ---
    /// Projectile trail color as RGB. (+0x2D4)
    pub color: [u8; 3],

    /// Per-bullet rocker force scale (RockerScale= in [Projectile] section).
    /// Multiplies the DirectRocker impulse force. Default 1.0. Stored as Q8.8
    /// (matches the gamemd representation).
    pub rocker_scale: I8F8,

    // --- String/reference fields ---
    /// Weapon fired on airburst detonation (weapon type name).
    pub airburst_weapon: Option<String>,
    /// Weapon fired for each shrapnel fragment (weapon type name).
    pub shrapnel_weapon: Option<String>,
    /// SHP/VXL image used for the in-flight projectile.
    pub image: Option<String>,
    /// ObjectType+236, loaded from ART before BulletRead clears a missing Image.
    /// FireAt 6FED2F uses this flag for its native launch-pitch branch.
    pub voxel: bool,
    /// Animation played as a trail behind the projectile (anim type name, art Image section).
    pub trailer: Option<String>,
}

impl ProjectileType {
    /// Parse a ProjectileType from a rules.ini section.
    ///
    /// AG defaults to true because most projectiles can hit ground targets.
    /// Rotates is read from the optional art Image section and exposed in the
    /// same inverted form stored by the binary.
    ///
    /// `image_section` is the optional art.ini section resolved via Image= key.
    /// Art-side fields (Rotates, Trailer, Flat, SpawnDelay, AnimLow/High/Rate,
    /// AnimPalette) are read from this section when present.
    pub fn from_ini_section(
        id: &str,
        section: &IniSection,
        image_section: Option<&IniSection>,
    ) -> Self {
        // Parse "R,G,B" color string into [u8; 3], defaulting to [0,0,0].
        let color = section
            .get("Color")
            .and_then(|s| {
                let parts: Vec<&str> = s.split(',').collect();
                if parts.len() == 3 {
                    let r = parts[0].trim().parse::<u8>().ok()?;
                    let g = parts[1].trim().parse::<u8>().ok()?;
                    let b = parts[2].trim().parse::<u8>().ok()?;
                    Some([r, g, b])
                } else {
                    None
                }
            })
            .unwrap_or([0, 0, 0]);

        Self {
            id: id.to_string(),
            // Targeting
            aa: section.get_bool("AA").unwrap_or(false),
            ag: section.get_bool("AG").unwrap_or(true),
            // Flight behavior
            arcing: section.get_bool("Arcing").unwrap_or(false),
            rot: section.get_i32("ROT").unwrap_or(0),
            inaccurate: section.get_bool("Inaccurate").unwrap_or(false),
            // Bool flags
            airburst: section.get_bool("Airburst").unwrap_or(false),
            floater: section.get_bool("Floater").unwrap_or(false),
            subject_to_cliffs: section.get_bool("SubjectToCliffs").unwrap_or(false),
            subject_to_elevation: section.get_bool("SubjectToElevation").unwrap_or(false),
            subject_to_walls: section.get_bool("SubjectToWalls").unwrap_or(false),
            very_high: section.get_bool("VeryHigh").unwrap_or(false),
            shadow: section.get_bool("Shadow").unwrap_or(true),
            dropping: section.get_bool("Dropping").unwrap_or(false),
            level: section.get_bool("Level").unwrap_or(false),
            inviso: section.get_bool("Inviso").unwrap_or(false),
            proximity: section.get_bool("Proximity").unwrap_or(false),
            ranged: section.get_bool("Ranged").unwrap_or(false),
            // Binary stores the inverse of the art Rotates key.
            rotates: image_section
                .and_then(|s| s.get_bool("Rotates"))
                .map(|v| !v)
                .unwrap_or(true),
            flak_scatter: section.get_bool("FlakScatter").unwrap_or(false),
            degenerates: section.get_bool("Degenerates").unwrap_or(false),
            bouncy: section.get_bool("Bouncy").unwrap_or(false),
            anim_palette: image_section
                .and_then(|s| s.get_bool("AnimPalette"))
                .unwrap_or(false),
            firers_palette: section.get_bool("FirersPalette").unwrap_or(false),
            scalable: section.get_bool("Scalable").unwrap_or(false),
            vertical: section.get_bool("Vertical").unwrap_or(false),
            // Flat is read from the Image section in art.ini
            flat: image_section
                .and_then(|s| s.get_bool("Flat"))
                .unwrap_or(false),
            // Integer fields
            cluster: section.get_i32("Cluster").unwrap_or(1),
            shrapnel_count: section.get_i32("ShrapnelCount").unwrap_or(0),
            detonation_altitude: section.get_i32("DetonationAltitude").unwrap_or(0),
            acceleration: section.get_i32("Acceleration").unwrap_or(3),
            course_lock_duration: section.get_i32("CourseLockDuration").unwrap_or(0),
            // SpawnDelay is read from the Image section in art.ini
            spawn_delay: image_section
                .and_then(|s| s.get_i32("SpawnDelay"))
                .unwrap_or(3),
            arm: section.get_i32("Arm").unwrap_or(0),
            anim_low: image_section
                .and_then(|s| s.get_i32("AnimLow"))
                .unwrap_or(0),
            anim_high: image_section
                .and_then(|s| s.get_i32("AnimHigh"))
                .unwrap_or(0),
            anim_rate: image_section
                .and_then(|s| s.get_i32("AnimRate"))
                .unwrap_or(0),
            // Float
            elasticity: section
                .get("Elasticity")
                .and_then(|s| s.trim().parse::<f64>().ok())
                .unwrap_or(0.75),
            // Color
            color,
            // Rocker
            rocker_scale: section
                .get_f32("RockerScale")
                .map(I8F8::from_num)
                .unwrap_or(I8F8::ONE),
            // String/reference fields
            airburst_weapon: section.get("AirburstWeapon").map(|s| s.trim().to_string()),
            shrapnel_weapon: section.get("ShrapnelWeapon").map(|s| s.trim().to_string()),
            image: section.get("Image").map(|s| s.trim().to_string()),
            voxel: image_section
                .and_then(|s| s.get_bool("Voxel"))
                .unwrap_or(false),
            trailer: image_section
                .and_then(|s| s.get("Trailer"))
                .map(|s| s.trim().to_string()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rules::ini_parser::IniFile;

    #[test]
    fn projectile_voxel_art_binding_preserves_explicit_image_and_absent_keys() {
        use crate::rules::{art_data::ArtRegistry, ruleset::RuleSet};
        let ini = IniFile::from_str(
            "[VehicleTypes]\n0=UNIT\n[UNIT]\nPrimary=GUN\n[GUN]\nProjectile=SHOT\n[SHOT]\nImage=OTHER\n",
        );
        let mut rules = RuleSet::from_ini(&ini).unwrap();
        rules.merge_art_data(&ArtRegistry::from_ini(&IniFile::from_str(
            "[SHOT]\nVoxel=yes\n[OTHER]\nImage=SHOT\n",
        )));
        assert!(
            !rules.projectile("SHOT").unwrap().voxel,
            "explicit Image never falls back or follows ART redirects"
        );
        rules.merge_art_data(&ArtRegistry::from_ini(&IniFile::from_str(
            "[OTHER]\nVoxel=yes\n",
        )));
        assert!(rules.projectile("SHOT").unwrap().voxel);
        rules.merge_art_data(&ArtRegistry::from_ini(&IniFile::from_str(
            "[OTHER]\nHeight=3\n",
        )));
        assert!(
            rules.projectile("SHOT").unwrap().voxel,
            "absent key retains the loaded ObjectType value"
        );
        rules.merge_art_data(&ArtRegistry::from_ini(&IniFile::from_str(
            "[OTHER]\nVoxel=no\n",
        )));
        assert!(!rules.projectile("SHOT").unwrap().voxel);
        let ini = IniFile::from_str(
            "[VehicleTypes]\n0=UNIT\n[UNIT]\nPrimary=GUN\n[GUN]\nProjectile=SHOT\n[SHOT]\n",
        );
        let mut rules = RuleSet::from_ini(&ini).unwrap();
        rules.merge_art_data(&ArtRegistry::from_ini(&IniFile::from_str(
            "[SHOT]\nVoxel=yes\n",
        )));
        let projectile = rules.projectile("SHOT").unwrap();
        assert!(
            projectile.voxel,
            "fresh ObjectType image defaults to its ID before BulletRead"
        );
        assert_eq!(
            projectile.image, None,
            "the later Bullet rendering name remains empty"
        );
    }

    #[test]
    fn test_parse_aa_projectile() {
        let ini: IniFile = IniFile::from_str("[MissileAA]\nAA=yes\nAG=no\nROT=20\nImage=DRAGON\n");
        let section: &IniSection = ini.section("MissileAA").unwrap();
        let proj: ProjectileType = ProjectileType::from_ini_section("MissileAA", section, None);

        assert_eq!(proj.id, "MissileAA");
        assert_eq!(proj.image.as_deref(), Some("DRAGON"));
        assert!(proj.aa);
        assert!(!proj.ag);
        assert_eq!(proj.rot, 20);
        assert!(!proj.arcing);
        assert!(!proj.inaccurate);
        // Rotates defaults to true when not specified
        assert!(proj.rotates);
    }

    #[test]
    fn test_defaults() {
        let ini: IniFile = IniFile::from_str("[Empty]\nFixtureOnly=1\n");
        let section: &IniSection = ini.section("Empty").unwrap();
        let proj: ProjectileType = ProjectileType::from_ini_section("Empty", section, None);

        assert!(!proj.aa);
        assert!(proj.ag, "AG should default to true");
        assert!(!proj.arcing);
        assert_eq!(proj.rot, 0);
        assert!(!proj.inaccurate);
        // Binary constructor defaults.
        assert!(!proj.airburst);
        assert!(!proj.floater);
        assert!(!proj.subject_to_cliffs);
        assert!(!proj.subject_to_elevation);
        assert!(!proj.subject_to_walls);
        assert!(!proj.very_high);
        assert!(proj.shadow);
        assert!(!proj.dropping);
        assert!(!proj.level);
        assert!(!proj.inviso);
        assert!(!proj.proximity);
        assert!(!proj.ranged);
        assert!(proj.rotates, "Rotates should default to true");
        assert!(!proj.flak_scatter);
        assert!(!proj.degenerates);
        assert!(!proj.bouncy);
        assert!(!proj.anim_palette);
        assert!(!proj.firers_palette);
        assert!(!proj.scalable);
        assert!(!proj.vertical);
        assert!(!proj.flat);
        // Int fields with verified non-zero constructor defaults.
        assert_eq!(proj.cluster, 1);
        assert_eq!(proj.shrapnel_count, 0);
        assert_eq!(proj.detonation_altitude, 0);
        assert_eq!(proj.acceleration, 3);
        assert_eq!(proj.course_lock_duration, 0);
        assert_eq!(proj.spawn_delay, 3);
        assert_eq!(proj.arm, 0);
        assert_eq!(proj.anim_low, 0);
        assert_eq!(proj.anim_high, 0);
        assert_eq!(proj.anim_rate, 0);
        // Float defaults
        assert_eq!(proj.elasticity, 0.75);
        // Color defaults
        assert_eq!(proj.color, [0, 0, 0]);
        // String fields default to None
        assert!(proj.airburst_weapon.is_none());
        assert!(proj.shrapnel_weapon.is_none());
        assert!(proj.trailer.is_none());
    }

    #[test]
    fn test_arcing_projectile() {
        let ini: IniFile =
            IniFile::from_str("[ArtilleryShell]\nAG=yes\nArcing=yes\nInaccurate=yes\n");
        let section: &IniSection = ini.section("ArtilleryShell").unwrap();
        let proj: ProjectileType =
            ProjectileType::from_ini_section("ArtilleryShell", section, None);

        assert!(proj.ag);
        assert!(proj.arcing);
        assert!(proj.inaccurate);
    }

    #[test]
    fn test_rotates_inversion() {
        // Rotates is an art Image key. The binary stores its inverse, and this
        // field intentionally exposes that stored form.
        let ini: IniFile = IniFile::from_str(
            "[Rules]\nRotates=yes\n[RotYesImage]\nRotates=yes\n[RotNoImage]\nRotates=no\n[NoKey]\nFixtureOnly=1\n",
        );
        let rules_section = ini.section("Rules").unwrap();

        let proj_yes =
            ProjectileType::from_ini_section("RotYes", rules_section, ini.section("RotYesImage"));
        assert!(!proj_yes.rotates, "art Rotates=yes should invert to false");

        let proj_no =
            ProjectileType::from_ini_section("RotNo", rules_section, ini.section("RotNoImage"));
        assert!(proj_no.rotates, "art Rotates=no should invert to true");

        let proj_rules_key_ignored = ProjectileType::from_ini_section("Rules", rules_section, None);
        assert!(
            proj_rules_key_ignored.rotates,
            "rules Rotates key is ignored; missing art key keeps default true"
        );

        let sec_none = ini.section("NoKey").unwrap();
        let proj_none = ProjectileType::from_ini_section("NoKey", sec_none, None);
        assert!(
            proj_none.rotates,
            "missing Rotates key should default to true"
        );
    }

    #[test]
    fn test_color_parsing() {
        let ini: IniFile = IniFile::from_str("[Tracer]\nColor=255,128,0\n");
        let section = ini.section("Tracer").unwrap();
        let proj = ProjectileType::from_ini_section("Tracer", section, None);

        assert_eq!(proj.color, [255, 128, 0]);
    }

    #[test]
    fn test_string_fields() {
        let ini: IniFile = IniFile::from_str(
            "[V3Rocket]\nAirburstWeapon=V3Warhead\nShrapnelWeapon=ShrapWep\n[V3RocketImage]\nTrailer=V3TRAIL\n",
        );
        let section = ini.section("V3Rocket").unwrap();
        let image_section = ini.section("V3RocketImage").unwrap();
        let proj = ProjectileType::from_ini_section("V3Rocket", section, Some(image_section));

        assert_eq!(proj.airburst_weapon.as_deref(), Some("V3Warhead"));
        assert_eq!(proj.shrapnel_weapon.as_deref(), Some("ShrapWep"));
        assert_eq!(proj.trailer.as_deref(), Some("V3TRAIL"));
    }

    #[test]
    fn parse_rocker_scale_default_one() {
        let ini: IniFile = IniFile::from_str("[TestBullet]\nFixtureOnly=1\n");
        let section = ini.section("TestBullet").unwrap();
        let p = ProjectileType::from_ini_section("TestBullet", section, None);
        assert_eq!(p.rocker_scale, I8F8::ONE);
    }

    #[test]
    fn parse_rocker_scale_custom() {
        let ini: IniFile = IniFile::from_str("[TestBullet]\nRockerScale=2.5\n");
        let section = ini.section("TestBullet").unwrap();
        let p = ProjectileType::from_ini_section("TestBullet", section, None);
        assert_eq!(p.rocker_scale, I8F8::from_num(2.5));
    }

    #[test]
    fn test_image_section_fields() {
        let ini: IniFile = IniFile::from_str(
            "[Proj]\nFlat=no\nSpawnDelay=99\nAnimLow=1\nAnimHigh=2\nAnimRate=3\nAnimPalette=no\nTrailer=RULES_TRAIL\n[ProjImage]\nFlat=yes\nSpawnDelay=10\nAnimLow=4\nAnimHigh=5\nAnimRate=6\nAnimPalette=yes\nTrailer=ART_TRAIL\n",
        );
        let proj_section = ini.section("Proj").unwrap();
        let image_section = ini.section("ProjImage").unwrap();
        let proj = ProjectileType::from_ini_section("Proj", proj_section, Some(image_section));

        assert!(proj.flat);
        assert_eq!(proj.spawn_delay, 10);
        assert_eq!(proj.anim_low, 4);
        assert_eq!(proj.anim_high, 5);
        assert_eq!(proj.anim_rate, 6);
        assert!(proj.anim_palette);
        assert_eq!(proj.trailer.as_deref(), Some("ART_TRAIL"));
    }

    #[test]
    fn test_arm_is_projectile_arming_delay_not_speed() {
        let ini: IniFile = IniFile::from_str("[Proj]\nArm=2\nSpeed=99\n");
        let section = ini.section("Proj").unwrap();
        let proj = ProjectileType::from_ini_section("Proj", section, None);

        assert_eq!(proj.arm, 2);
        assert_eq!(proj.rot, 0);
        assert_eq!(proj.spawn_delay, 3);
    }
}
