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

use crate::rules::ini_parser::{IniFile, IniSection};

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
    /// Enrolls in the process-wide scalable-Bullet list used to adjust the
    /// type's Trailer cadence (InitScalable46B280, +0x2EC), not sprite size.
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
    /// Signed global-frame cadence for Trailer, read from fixed ART's Image
    /// section (Bullet AI46687C, +0x2E4).
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
    /// Native BulletType +2D4 runtime ColorScheme index. The retained rules
    /// processor owns ReadColor474A90; a name alone loses layered defaults.
    color_scheme: i32,

    // --- String/reference fields ---
    /// Weapon fired on airburst detonation (weapon type name).
    pub airburst_weapon: Option<String>,
    /// Weapon fired for each shrapnel fragment (weapon type name).
    pub shrapnel_weapon: Option<String>,
    /// Current native Image25 text; SHP admission is separately retained below.
    pub image: Option<String>,
    /// Last admitted native5F9070 load, independently of the later Image text.
    /// None means no reader load; an empty name is an actual .SHP/.GHP attempt.
    pub image_load: Option<ProjectileImageLoad>,
    /// Last native5F8110 VXL/HVA load attempt. Empty is an actual empty-name
    /// attempt; None means no reached voxel load since construction.
    voxel_image_load: Option<String>,
    /// ObjectType+236, loaded from ART before BulletRead clears a missing Image.
    /// FireAt 6FED2F uses this flag for its native launch-pitch branch.
    pub voxel: bool,
    /// ObjectType ART +22C/+237, retained before Bullet's separate Image read.
    pub theater: bool,
    pub new_theater: bool,
    /// Animation played as a trail behind the projectile (anim type name, art Image section).
    pub trailer: Option<String>,
    /// ObjectType ART trail inputs, retained at each native reader pass.
    pub use_line_trail: bool,
    pub line_trail_color: [u8; 3],
    pub line_trail_color_decrement: i32,
}

/// Inputs captured at the latest ObjectType::LoadImage5F9070 call. Asset binding
/// resolves this attempted name; it must not infer a load from constructor text.
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct ProjectileImageLoad {
    pub image: String,
    pub theater: bool,
    pub new_theater: bool,
}

/// Process-resident Bullet image, Color and ART values, owned by
/// native_processing::ProcessedType.
/// Native constructors46BBC0/5F7090 and full ReadINI46BEE0; executable inputs:
/// tools/projectile_oracle/bridge_render_art_state.{py,json}.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub(crate) struct ProjectileArtState {
    image: String,
    image_load: Option<ProjectileImageLoad>,
    #[serde(default)]
    voxel_image_load: Option<String>,
    /// Bullet ctor46BCA4 and ReadColor474A90 at46BFA5.
    #[serde(default)]
    color_scheme: i32,
    inviso: bool,
    voxel: bool,
    theater: bool,
    new_theater: bool,
    inverse_rotates: bool,
    flat: bool,
    anim_palette: bool,
    anim_low: u8,
    anim_high: u8,
    anim_rate: u8,
    spawn_delay: i32,
    trailer: Option<String>,
    use_line_trail: bool,
    line_trail_color: [u8; 3],
    line_trail_color_decrement: i32,
}

impl ProjectileArtState {
    pub(crate) fn new(native_id: &str) -> Self {
        Self {
            image: native_id.chars().take(24).collect(),
            image_load: None,
            voxel_image_load: None,
            color_scheme: 0,
            inviso: false,
            voxel: false,
            theater: false,
            new_theater: false,
            inverse_rotates: true,
            flat: false,
            anim_palette: false,
            anim_low: 0,
            anim_high: 0,
            anim_rate: 0,
            spawn_delay: 3,
            trailer: None,
            use_line_trail: false,
            line_trail_color: [128; 3],
            line_trail_color_decrement: 16,
        }
    }

    pub(crate) fn read_pass(
        &mut self,
        rules: &IniSection,
        fixed_art: &IniFile,
        color_names: &[String],
        resolve_trailer: impl FnMut(&str) -> Option<String>,
    ) {
        // ObjectType5F92F8..5F933B uses Image25 with the previous Image default.
        // Its ART reads memoize the address of the Image buffer (INI526B10).
        let object_image = rules.read_string("Image", &self.image, 25);
        let section = fixed_art.section(&object_image);
        self.read_object_fields(section);
        // ObjectRead5F963A..5F964D loads SHP for a non-Voxel Bullet before the
        // subclass updates Inviso or clears Image. Constructor-only types have
        // never taken this path. Missing files still replace the old image.
        if !self.voxel {
            self.record_image_load(&object_image);
        }
        // Bullet46BF96..46BFAA: constructor0, current-index default and the
        // Colors catalog already registered by this RulesClass pass. Missing
        // whole sections never enter this body; a missing key still reads.
        self.color_scheme = rules.read_color_scheme(
            "Color",
            self.color_scheme,
            color_names.iter().map(String::as_str),
        );
        // Bullet46C1CC clears absent Image. Later unguarded animation reads use
        // that same address, hence the ART section cached by ObjectType above.
        // Active retail's preceding Anim sweep679A5D..679A82 resets this cache:
        // even missing D ReadINI427D13 calls526B00. A stack without any prior
        // Anim/other ART read needs the shared INI-cache lifetime mechanism.
        self.image = rules.read_string("Image", "", 25);
        self.inviso = rules.read_bool("Inviso", self.inviso);
        self.read_bullet_fields(section, resolve_trailer);
        self.record_bullet_image_loads();
    }

    fn record_bullet_image_loads(&mut self) {
        // 46C3F8..46C406 skips only this second load for Inviso. Its current
        // Image may be empty: native then attempts .SHP and .GHP, clearing +A4
        // when neither exists. Inviso can instead retain the base-prefix load.
        if !self.inviso {
            self.record_image_load(&self.image.clone());
        }
        // 46C40B..46C417: independent of Inviso, after the clearing Image
        // read. A false Voxel flag preserves the prior pair; missing/empty
        // files on a reached load clear it in the asset owner's load result.
        if self.voxel {
            self.voxel_image_load = Some(self.image.clone());
        }
    }

    fn record_image_load(&mut self, image: &str) {
        self.image_load = Some(ProjectileImageLoad {
            image: image.to_owned(),
            theater: self.theater,
            new_theater: self.new_theater,
        });
    }

    fn read_object_fields(&mut self, section: Option<&IniSection>) {
        if let Some(section) = section {
            self.theater = section.read_bool("Theater", self.theater);
            self.new_theater = section.read_bool("NewTheater", self.new_theater);
            self.voxel = section.read_bool("Voxel", self.voxel);
            // ObjectType5F9574..5F95E2, after the base ART/Image selection.
            // Constructor5F7090 defaults; original executable controls:
            // tools/projectile_oracle/line_trail.json reader_controls.
            self.use_line_trail = section.read_bool("UseLineTrail", self.use_line_trail);
            self.line_trail_color = section.read_color_rgb("LineTrailColor", self.line_trail_color);
            self.line_trail_color_decrement =
                section.read_int("LineTrailColorDecrement", self.line_trail_color_decrement);
        }
    }

    fn read_bullet_fields(
        &mut self,
        section: Option<&IniSection>,
        mut resolve_trailer: impl FnMut(&str) -> Option<String>,
    ) {
        let Some(section) = section else { return };
        // 46C1ED..46C292: only these fields require a nonempty current Image.
        if !self.image.is_empty() {
            let incoming = section.read_string("Trailer", "", 128);
            if !incoming.is_empty() {
                self.trailer = resolve_trailer(&incoming);
            }
            self.spawn_delay = section.read_int("SpawnDelay", self.spawn_delay);
            self.inverse_rotates = !section.read_bool("Rotates", !self.inverse_rotates);
            self.flat = section.read_bool("Flat", self.flat);
        }
        // 46C37E..46C3F2: defaults are zero-extended retained bytes, and each
        // ReadInt result stores AL without a clamp. These calls are unguarded.
        self.anim_low = section.read_int("AnimLow", i32::from(self.anim_low)) as u8;
        self.anim_high = section.read_int("AnimHigh", i32::from(self.anim_high)) as u8;
        self.anim_rate = section.read_int("AnimRate", i32::from(self.anim_rate)) as u8;
        self.anim_palette = section.read_bool("AnimPalette", self.anim_palette);
    }

    pub(crate) fn apply_to(&self, projectile: &mut ProjectileType) {
        projectile.image = (!self.image.is_empty()).then(|| self.image.clone());
        projectile.image_load = self.image_load.clone();
        projectile.voxel_image_load = self.voxel_image_load.clone();
        projectile.color_scheme = self.color_scheme;
        projectile.inviso = self.inviso;
        projectile.voxel = self.voxel;
        projectile.theater = self.theater;
        projectile.new_theater = self.new_theater;
        projectile.rotates = self.inverse_rotates;
        projectile.flat = self.flat;
        projectile.anim_palette = self.anim_palette;
        projectile.anim_low = i32::from(self.anim_low);
        projectile.anim_high = i32::from(self.anim_high);
        projectile.anim_rate = i32::from(self.anim_rate);
        projectile.spawn_delay = self.spawn_delay;
        projectile.trailer = self.trailer.clone();
        projectile.use_line_trail = self.use_line_trail;
        projectile.line_trail_color = self.line_trail_color;
        projectile.line_trail_color_decrement = self.line_trail_color_decrement;
    }
}

impl ProjectileType {
    /// Runtime ColorScheme index (two native shade variants per Colors entry).
    pub fn color_scheme(&self) -> i32 {
        self.color_scheme
    }

    /// Current retained VXL/HVA load attempt, including an empty filename.
    /// Asset loading owns whether that attempted pair resolved successfully.
    pub fn voxel_image_load(&self) -> Option<&str> {
        self.voxel_image_load.as_deref()
    }

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
        let mut result = Self {
            id: id.to_string(),
            // Targeting
            aa: section.read_bool("AA", false),
            ag: section.read_bool("AG", true),
            // Flight behavior
            arcing: section.read_bool("Arcing", false),
            rot: section.read_int("ROT", 0),
            inaccurate: section.read_bool("Inaccurate", false),
            // Bool flags
            airburst: section.read_bool("Airburst", false),
            floater: section.read_bool("Floater", false),
            subject_to_cliffs: section.read_bool("SubjectToCliffs", false),
            subject_to_elevation: section.read_bool("SubjectToElevation", false),
            subject_to_walls: section.read_bool("SubjectToWalls", false),
            very_high: section.read_bool("VeryHigh", false),
            shadow: section.read_bool("Shadow", true),
            dropping: section.read_bool("Dropping", false),
            level: section.read_bool("Level", false),
            inviso: section.read_bool("Inviso", false),
            proximity: section.read_bool("Proximity", false),
            ranged: section.read_bool("Ranged", false),
            rotates: true,
            flak_scatter: section.read_bool("FlakScatter", false),
            degenerates: section.read_bool("Degenerates", false),
            bouncy: section.read_bool("Bouncy", false),
            anim_palette: false,
            firers_palette: section.read_bool("FirersPalette", false),
            scalable: section.read_bool("Scalable", false),
            vertical: section.read_bool("Vertical", false),
            flat: false,
            // Integer fields
            cluster: section.read_int("Cluster", 1),
            shrapnel_count: section.read_int("ShrapnelCount", 0),
            detonation_altitude: section.read_int("DetonationAltitude", 0),
            acceleration: section.read_int("Acceleration", 3),
            course_lock_duration: section.read_int("CourseLockDuration", 0),
            spawn_delay: 3,
            arm: section.read_int("Arm", 0),
            anim_low: 0,
            anim_high: 0,
            anim_rate: 0,
            // ReadDouble -> `FSTP qword [ESI+0x2C8]` (`0x0046BF71`) over the
            // constructor's 0.75 (`0x0046BC65`/`0x0046BC9A`).
            elasticity: section.read_double("Elasticity", 0.75),
            // Constructor46BCA4. Resolution requires the pass-local Colors
            // registry and is supplied by the retained rules processor.
            color_scheme: 0,
            // ReadString 0x80 ahead of each weapon lookup (`0x0046C2B9`,
            // `0x0046C2F8`).
            airburst_weapon: section
                .read_name("AirburstWeapon", 0x80)
                .map(str::to_string),
            shrapnel_weapon: section
                .read_name("ShrapnelWeapon", 0x80)
                .map(str::to_string),
            image: None,
            image_load: None,
            voxel_image_load: None,
            voxel: false,
            theater: false,
            new_theater: false,
            trailer: None,
            use_line_trail: false,
            line_trail_color: [128; 3],
            line_trail_color_decrement: 16,
        };
        // Standalone reader callers use the same field kernel. Production
        // RuleSet receives the retained per-pass state from native_processing.
        let mut art = ProjectileArtState::new(id);
        art.read_object_fields(image_section);
        if !art.voxel {
            art.record_image_load(&section.read_string("Image", id, 25));
        }
        art.image = section.read_string("Image", "", 25);
        art.inviso = section.read_bool("Inviso", false);
        art.read_bullet_fields(image_section, |incoming| {
            (!incoming.eq_ignore_ascii_case("none") && !incoming.eq_ignore_ascii_case("<none>"))
                .then(|| incoming.chars().take(24).collect())
        });
        art.record_bullet_image_loads();
        art.apply_to(&mut result);
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rules::ini_parser::IniFile;

    #[test]
    fn projectile_art_binding_does_not_reread_after_retained_processing() {
        use crate::rules::{art_data::ArtRegistry, ruleset::RuleSet};
        let ini = IniFile::from_str(
            "[VehicleTypes]\n0=UNIT\n[UNIT]\nPrimary=GUN\n[GUN]\nProjectile=SHOT\n[SHOT]\nImage=OTHER\n",
        );
        let art = IniFile::from_str("[SHOT]\nVoxel=no\n[OTHER]\nVoxel=yes\nRotates=yes\n");
        let mut rules = RuleSet::from_ini_with_fixed_art_for_test(&ini, &art).unwrap();
        let hash = rules.simulation_config_hash();
        rules.install_art_data(ArtRegistry::from_ini(&IniFile::from_str(
            "[OTHER]\nVoxel=no\nRotates=no\nAnimPalette=yes\n",
        )));
        let projectile = rules.projectile("SHOT").unwrap();
        assert!(projectile.voxel);
        assert!(!projectile.rotates);
        assert!(!projectile.anim_palette);
        assert_eq!(
            hash,
            rules.simulation_config_hash(),
            "late binding cannot replace Bullet state"
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
        assert_eq!(proj.color_scheme(), 0);
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
            "[Rules]\nImage=ART\nRotates=yes\n[RotYesImage]\nRotates=yes\n[RotNoImage]\nRotates=no\n[NoKey]\nFixtureOnly=1\n",
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
    fn isolated_section_keeps_constructor_color_without_a_registry() {
        let ini: IniFile = IniFile::from_str("[ChemMissile]\nColor=DarkGreen\n");
        let section = ini.section("ChemMissile").unwrap();
        let proj = ProjectileType::from_ini_section("ChemMissile", section, None);

        assert_eq!(proj.color_scheme(), 0);
    }

    #[test]
    fn test_string_fields() {
        let ini: IniFile = IniFile::from_str(
            "[V3Rocket]\nImage=V3RocketImage\nAirburstWeapon=V3Warhead\nShrapnelWeapon=ShrapWep\n[V3RocketImage]\nTrailer=V3TRAIL\n",
        );
        let section = ini.section("V3Rocket").unwrap();
        let image_section = ini.section("V3RocketImage").unwrap();
        let proj = ProjectileType::from_ini_section("V3Rocket", section, Some(image_section));

        assert_eq!(proj.airburst_weapon.as_deref(), Some("V3Warhead"));
        assert_eq!(proj.shrapnel_weapon.as_deref(), Some("ShrapWep"));
        assert_eq!(proj.trailer.as_deref(), Some("V3TRAIL"));
    }

    #[test]
    fn test_image_section_fields() {
        let ini: IniFile = IniFile::from_str(
            "[Proj]\nImage=ProjImage\nFlat=no\nSpawnDelay=99\nAnimLow=1\nAnimHigh=2\nAnimRate=3\nAnimPalette=no\nTrailer=RULES_TRAIL\n[ProjImage]\nFlat=yes\nSpawnDelay=10\nAnimLow=4\nAnimHigh=5\nAnimRate=6\nAnimPalette=yes\nTrailer=ART_TRAIL\n",
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

#[cfg(test)]
#[path = "projectile_line_trail_tests.rs"]
mod line_trail_tests;
