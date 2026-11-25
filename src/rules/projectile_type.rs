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
use crate::rules::object_image::{ImageTheater, ObjectImageKind, ObjectImageState};
use crate::util::native_x87::NativeF64Bits;

/// Shared inputs captured by ObjectType::LoadSHP5F9070; asset binding resolves
/// the attempted name independently of constructor or later Image text.
use crate::rules::object_image::ObjectImageLoad;

/// VERA's bound on `ShrapnelCount=`'s magnitude, read by
/// `BulletTypeClass::ReadINI`'s ReadInt (`0x0046C325`, `+0x2B8`).
/// SpawnShrapnel (`0x0046A322..0x0046A3D2`) builds one projectile per
/// fragment, so a value near `i32::MAX` freezes the match; retail throws at
/// most 8.
pub const SHRAPNEL_COUNT_LIMIT: i32 = 256;

/// VERA's bound on `Cluster=`, read by `BulletTypeClass::ReadINI`'s ReadInt
/// (`0x0046C1A1`, `+0x2AC`). A non-airburst impact detonates once per
/// cluster and an airburst lists one target per cluster, so a value near
/// `i32::MAX` freezes the match or runs out of memory; retail clusters at
/// most 9.
pub const CLUSTER_LIMIT: i32 = 256;

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
    /// Inherited `ObjectTypeClass::ReadINI @ 0x005F92D0` AlphaImage buffer
    /// (`+0x213`, `char[25]`); BulletType reads its base at `0x0046BEE0`.
    pub(crate) alpha_image: String,

    // --- Targeting flags ---
    /// Can hit aircraft. Anti-air weapons need this flag.
    pub aa: bool,
    /// Can hit ground units and buildings. True by default for most projectiles.
    pub ag: bool,
    /// Kratos `AAOnly` (`BulletTypeExt::TypeData`, `src/Extension/BulletTypeExt.h`):
    /// the shot is refused against a target that is not in the air.
    /// `TechnoStatus::CanFire_CheckBullet` runs it from the hook at `0x6FC339`.
    pub aa_only: bool,
    /// Tiberian Sun's `AV=` (docs/ts-limpets.md): the projectile hunts only
    /// vehicles. Firestorm 2.0 reads it into BulletType+0x136 with ReadBool
    /// over the current value (`0x00448027..0x00448047`; the constructor
    /// zeroes it at `0x00447BB9`); gamemd has no such key. Its one reader is
    /// `WeaponTypeClass::Allowable_Threat @ 0x00681150`
    /// (`combat_weapon::weapon_class_bits`).
    pub av: bool,
    /// Phobos `AU` (`Ext/BulletType/Body.cpp:83`): this projectile may attack
    /// a techno that is underground. Auto-target ORs `0x20000` into the
    /// weapon's allowed threats (`Ext/Techno/Hooks.cpp:1537-1545`) and the
    /// mouse-over fire gate accepts it (`:1497-1510`).
    pub au: bool,

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
    /// Native `Proximity=` (`BulletTypeClass::ReadINI @ 0x0046BEE0`, +0x29F).
    /// The reader passes the current byte as `CCINIClass::ReadBool @ 0x005295F0`
    /// default (`0x0046C0AE`); the constructor at `0x0046BBC0` starts false.
    pub proximity: bool,
    /// Has a maximum range (non-homing). (+0x2A0)
    pub ranged: bool,
    /// Inverted binary storage for the art `Rotates=` key. (+0x2A1)
    /// `Rotates=yes` stores false, `Rotates=no` stores true.
    pub rotates: bool,
    /// Scatters like flak — random spread pattern for AA. (+0x2A3)
    pub flak_scatter: bool,
    /// Bullet AI467C86 decrements raw damage on continuing flights above five.
    /// ReadBooleanByte46BEE0 retains the previous value across Rules passes.
    pub degenerates: bool,
    /// Native `Bouncy=` (`BulletTypeClass::ReadINI @ 0x0046BEE0`, +0x2A7).
    /// The reader passes the current byte as `CCINIClass::ReadBool @ 0x005295F0`
    /// default (`0x0046C16A`); the constructor at `0x0046BBC0` starts false.
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
    /// Phobos `VerticalInitialFacing=` (`Nullable<bool>`,
    /// `Ext/BulletType/Body.cpp:37`, `Body.h:37`, constructor empty
    /// `Body.h:104`): `None` falls back to `Voxel || Vertical` at the
    /// consumer. The resolved flag decides whether a fired bullet takes the
    /// target-Z initial pitch arm in `TechnoClass::FireAt`
    /// (`Misc/Hooks.BugFixes.cpp:2429`, hook `0x6FED2F`
    /// `TechnoClass_FireAt_VerticalInitialFacing`).
    pub phobos_vertical_initial_facing: Option<bool>,
    /// Phobos `Vertical.AircraftFix=` (`Ext/BulletType/Body.cpp:36`,
    /// `Body.h:36`, `Nullable`): `None` takes `[General]
    /// Vertical.AircraftFix` (constructing **yes**, `Ext/Rules/Body.h:990`).
    /// With `Vertical=` it keeps a fired bullet's FireAt velocity instead of
    /// the aircraft's ROT course (`0x415F25`,
    /// `Misc/Hooks.BugFixes.cpp:2423`).
    pub vertical_aircraft_fix: Option<bool>,
    /// Phobos `Arcing.AllowElevationInaccuracy=` (`Nullable<bool>`,
    /// `Ext/BulletType/Body.cpp:53`, `Body.h:55`): when this refuses
    /// (the `[CombatDamage]` global defaults **`true`**), the arcing
    /// launch velocity is recomputed by `BulletExt::ApplyArcingFix`
    /// (`Ext/Bullet/Body.cpp:454-478`, hooks `0x6FF008`/`0x44D46E`).
    pub phobos_arcing_allow_elevation_inaccuracy: Option<bool>,
    /// Phobos `SubjectToGround=` (`Ext/BulletType/Body.cpp:56`, declared
    /// `Ext/BulletType/Body.h:59`, constructed `no` `:123`): the firing
    /// techno's pre-shot obstacle scan also refuses a line that passes below
    /// the cell floor (`Ext/Bullet/Hooks.Obstacles.cpp:49`, hook
    /// `0x6F7647`). Distinct from the per-trajectory
    /// `Trajectory.Straight/Bombard.SubjectToGround`, which detonates a
    /// flying projectile.
    pub phobos_subject_to_ground: bool,
    /// Phobos parabombs (`Ext/BulletType/Body.cpp:79-82`, declared
    /// `Ext/BulletType/Body.h:83-86`): `Parachuted=`, `Parachuted.FallRate=`,
    /// `Parachuted.MaxFallRate=` and `BombParachute=`.
    pub phobos_parabomb: crate::rules::phobos_parabomb::PhobosParabomb,
    /// Phobos `SubjectToLand=` / `SubjectToLand.Detonate=` /
    /// `SubjectToWater=` / `SubjectToWater.Detonate=`
    /// (`Ext/BulletType/Body.cpp:48-51`): the land/water half of the pre-shot
    /// obstacle walk (`Ext/Bullet/Hooks.Obstacles.cpp:74-87`).
    pub phobos_subject_to_terrain: crate::rules::phobos_bullet_terrain::PhobosSubjectToTerrain,
    /// Phobos `ReturnWeapon=` / `ReturnWeapon.ApplyFirepowerMult=`
    /// (`Ext/BulletType/Body.cpp:54-55`, declared `Ext/BulletType/Body.h:56-57`).
    pub phobos_return_weapon: crate::rules::phobos_return_weapon::PhobosReturnWeapon,
    /// ART Flat (+0x2F7), retained by the per-pass native rules processor.
    /// BulletClass::GetLayer 0x00468B90 selects Surface when true, Air otherwise.
    pub flat: bool,

    // --- Integer fields ---
    /// Number of sub-projectiles on detonation. (+0x2AC) At most
    /// [`CLUSTER_LIMIT`].
    pub cluster: i32,
    /// Phobos `ClusterScatter.Min=` (`Ext/BulletType/Body.cpp:46`,
    /// `Valueable<Leptons>`): lower `RandomRanged` bound, in leptons, of each
    /// cluster successor's distance from the impact. Constructor default 256
    /// (`Ext/BulletType/Body.h:111`).
    pub cluster_scatter_min: i32,
    /// Phobos `ClusterScatter.Max=` (`:47`), upper bound; default 512
    /// (`Ext/BulletType/Body.h:112`).
    pub cluster_scatter_max: i32,
    /// Number of shrapnel fragments on impact. (+0x2B8) Its magnitude is at
    /// most [`SHRAPNEL_COUNT_LIMIT`].
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
    /// `Strength=` (ObjectTypeClass::Strength on a BulletType): the bullet's
    /// interception hit points, default 0. Phobos `BulletExt::InitializeState`
    /// copies it to `CurrentStrength`.
    pub strength: i32,
    /// Phobos `BulletTypeExt::Armor` (`Nullable<ArmorType>`): unset leaves
    /// the bullet without interception armor.
    pub armor: Option<crate::rules::armor::ArmorId>,
    /// Phobos `BulletTypeExt::Interceptable` (`Nullable<bool>`), falling back
    /// to `[CombatDamage] ProjectileInterceptable`.
    pub interceptable: Option<bool>,
    /// Phobos `Interceptable.DeleteOnIntercept=`.
    pub interceptable_delete_on_intercept: bool,
    /// Phobos `Interceptable.WeaponOverride=` (`Valueable<WeaponTypeClass*>`
    /// read with allocate semantics).
    pub interceptable_weapon_override: Option<String>,

    // --- Float fields ---
    /// Bounce elasticity coefficient (0.0 = no bounce, 1.0 = perfect bounce). (+0x2C8)
    pub elasticity: f64,
    /// Phobos `BulletTypeExt::Gravity` (`Nullable<double>`): the ballistic
    /// gravity this type uses, defaulting to `[General] Gravity=` and halved
    /// for `Floater=`. Kept as exact binary64 bits, like `Elasticity=`.
    gravity: Option<NativeF64Bits>,

    // --- Color ---
    /// Native BulletType +2D4 runtime ColorScheme index. The retained rules
    /// processor owns ReadColor474A90; a name alone loses layered defaults.
    color_scheme: i32,

    // --- String/reference fields ---
    /// Phobos `Shrapnel.AffectsGround=` (`Nullable<bool>`,
    /// `Ext/BulletType/Body.cpp:41`): `None` falls back to the
    /// `[CombatDamage]` global. Only the shrapnel admission gate reads it.
    pub shrapnel_affects_ground: Option<bool>,
    /// Phobos `Shrapnel.AffectsBuildings=` (`:42`), same fallback.
    pub shrapnel_affects_buildings: Option<bool>,
    /// Phobos `Shrapnel.IgnoreHitBuildings=` (`Ext/BulletType/Body.cpp:44`),
    /// same fallback.
    pub shrapnel_ignore_hit_buildings: Option<bool>,
    /// Phobos `Shrapnel.UseWeaponTargeting=` (`:43`), same fallback; consumed
    /// by `sim::combat::shrapnel`'s ring walk.
    pub shrapnel_use_weapon_targeting: Option<bool>,
    /// Phobos `Shrapnel.ObeyWarheadTriggerConditions=` (`Ext/BulletType/Body.cpp:45`,
    /// `Body.h:45`, `Nullable`): `None` takes the `[CombatDamage]` global
    /// (default **yes**). An effective `yes` applies the warhead's health,
    /// veterancy, neutral and invoker conditions to a Techno target before
    /// its whole detonation body; failing them keeps only the impact extras
    /// (`Ext/Bullet/Hooks.DetonateLogics.cpp:22-29`, hook `0x4690D4`,
    /// `GoToExtras` `0x469AA4`).
    pub shrapnel_obey_warhead_trigger_conditions: Option<bool>,
    /// `ShrapnelWeapon=` (+2B4), the WeaponType fired for each shrapnel
    /// fragment, as the ordered Rules processor resolved it.
    pub shrapnel_weapon: Option<String>,
    /// Airburst child weapon (+2B0), retained by the ordered Rules reader.
    pub airburst_weapon: Option<String>,
    /// Phobos `BulletTypeExt` airburst/splits settings. Absent keys keep the
    /// Phobos constructor defaults; the per-game Phobos switch gates consumers.
    pub phobos_airburst: crate::rules::phobos_airburst::PhobosAirburstType,
    /// Phobos `Trajectory` / `Trajectory.Speed` / `Trajectory.Straight.*`
    /// (`Ext/Bullet/Trajectories/`): a selected flight trajectory instead of
    /// the stock `BulletClass::AI` arms.
    pub phobos_trajectory: crate::rules::phobos_trajectory::PhobosTrajectoryRules,
    /// Phobos `BulletTypeExt::LaserTrail_Types`
    /// (`Ext/BulletType/Body.cpp:89..94`): the ART `LaserTrail.Types` index
    /// list, by `[LaserTrailTypes]` name. Read from the `ImageFile` section
    /// (`Image=` when authored, otherwise the type ID).
    pub laser_trail_types: Vec<String>,
    /// Ares BulletTypeExt keys (`rules::ares_bullet_type`, read by
    /// `100193C0`). A stock-way read keeps the constructor defaults.
    pub(crate) bullet_type_ext: crate::rules::ares_bullet_type::BulletTypeExtRules,
    /// Current native Image25 text; SHP admission is separately retained below.
    pub image: Option<String>,
    /// Admitted native5F9070 requests, independently of later Image text.
    /// An empty list means no load; an empty name is an actual .SHP/.GHP attempt.
    pub image_loads: Vec<ObjectImageLoad>,
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

/// Process-resident Bullet Color, ART and Degenerates values, owned by
/// native_processing::ProcessedType alongside its shared ObjectImageState.
/// Native constructors46BBC0/5F7090 and full ReadINI46BEE0; executable inputs:
/// tools/projectile_oracle/bridge_render_art_state.{py,json}.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub(crate) struct ProjectileArtState {
    /// Bullet ctor46BCA4 and ReadColor474A90 at46BFA5.
    #[serde(default)]
    color_scheme: i32,
    /// Rules Degenerates (+2A6) shares this retained Bullet reader owner.
    #[serde(default)]
    degenerates: bool,
    /// BulletType+294 uses its current byte as the ReadBoolean default.
    #[serde(default)]
    airburst: bool,
    inviso: bool,
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
    /// `BulletTypeExt::LaserTrail_Types`, read from the ART `ImageFile`
    /// section after the stock Bullet reads (`Ext/BulletType/Body.cpp:89..94`).
    #[serde(default)]
    laser_trail_types: Vec<String>,
}

impl ProjectileArtState {
    pub(crate) fn new() -> Self {
        Self {
            color_scheme: 0,
            degenerates: false,
            airburst: false,
            inviso: false,
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
            laser_trail_types: Vec::new(),
        }
    }

    pub(crate) fn read_pass(
        &mut self,
        image: &mut ObjectImageState,
        theater: ImageTheater,
        rules: &IniSection,
        fixed_art: &IniFile,
        color_names: &[String],
        resolve_trailer: impl FnMut(&str) -> Option<String>,
    ) {
        // ObjectType5F92F8..5F933B uses Image25 with the previous Image default.
        // Its ART reads memoize the address of the Image buffer (INI526B10).
        let section = image.read_ini(rules, fixed_art, ObjectImageKind::Ordinary, theater);
        // ObjectRead5F963A..5F964D loads SHP for a non-Voxel Bullet before the
        // subclass updates Inviso or clears Image. Constructor-only types have
        // never taken this path. The shared owner records that load, including
        // its alternate-arctic mutation, while ART keeps the cached section.
        self.read_bullet_fields(image, theater, rules, section, color_names, resolve_trailer);
    }

    fn read_bullet_fields(
        &mut self,
        image: &mut ObjectImageState,
        theater: ImageTheater,
        rules: &IniSection,
        section: Option<&IniSection>,
        color_names: &[String],
        mut resolve_trailer: impl FnMut(&str) -> Option<String>,
    ) {
        if let Some(section) = section {
            // ObjectType5F9574..5F95E2, after the base ART/Image selection.
            // Constructor5F7090 defaults; original executable controls:
            // tools/projectile_oracle/line_trail.json reader_controls.
            self.use_line_trail = section.read_bool("UseLineTrail", self.use_line_trail);
            self.line_trail_color =
                section.read_color_rgb("LineTrailColor", self.line_trail_color, None);
            self.line_trail_color_decrement =
                section.read_int("LineTrailColorDecrement", self.line_trail_color_decrement);
        }
        // Bullet46BF96..46BFAA: constructor0, current-index default and the
        // Colors catalog already registered by this RulesClass pass. Missing
        // whole sections never enter this body; a missing key still reads.
        self.color_scheme = rules.read_color_scheme(
            "Color",
            self.color_scheme,
            color_names.iter().map(String::as_str),
        );
        // Bullet46BEE0 reads with the constructor/current byte as default;
        // a flattened invalid later token cannot replace a prior true value.
        self.degenerates = rules.read_bool("Degenerates", self.degenerates);
        self.airburst = rules.read_bool("Airburst", self.airburst);
        // Bullet46C1CC clears absent Image. Later unguarded animation reads use
        // that same address, hence the ART section cached by ObjectType above.
        // Active retail's preceding Anim sweep679A5D..679A82 resets this cache:
        // even missing D ReadINI427D13 calls526B00. A stack without any prior
        // Anim/other ART read needs the shared INI-cache lifetime mechanism.
        image.replace_image(rules, "");
        self.inviso = rules.read_bool("Inviso", self.inviso);
        if let Some(section) = section {
            // 46C1ED..46C292: only these fields require a nonempty current Image.
            if !image.image().is_empty() {
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
        // 46C3F8..46C406 skips only this second load for Inviso. Its current
        // Image may be empty: native then attempts .SHP and .GHP, clearing +A4
        // when neither exists. Inviso can instead retain the base-prefix load.
        if !self.inviso {
            image.load_unowned_shp(ObjectImageKind::Ordinary, theater, None);
        }
        // 46C40B..46C417: independent of Inviso, after the clearing Image
        // read. A false Voxel flag preserves the prior pair; missing/empty
        // files on a reached load clear it in the asset owner's load result.
        if image.voxel() {
            image.load_voxel();
        }
        // BulletTypeExt::LoadFromINIFile reads `LaserTrail.Types` from the
        // ART layer after the stock Bullet body, with the `ImageFile` section
        // (`Ext/BulletType/Body.cpp:89..94`).
        if let Some(section) = section {
            self.laser_trail_types = crate::rules::phobos_laser_trail::read_laser_trail_types(
                section,
                std::mem::take(&mut self.laser_trail_types),
            );
        }
    }

    pub(crate) fn apply_to(&self, image: &ObjectImageState, projectile: &mut ProjectileType) {
        projectile.image = (!image.image().is_empty()).then(|| image.image().to_string());
        projectile.image_loads = image.shp_loads().to_vec();
        projectile.voxel_image_load = image.voxel_load().map(str::to_string);
        projectile.color_scheme = self.color_scheme;
        projectile.degenerates = self.degenerates;
        projectile.airburst = self.airburst;
        projectile.inviso = self.inviso;
        projectile.voxel = image.voxel();
        projectile.theater = image.theater();
        projectile.new_theater = image.new_theater();
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
        {
            projectile.laser_trail_types = self.laser_trail_types.clone();
        }
    }
}

/// Phobos `BulletTypeExt::Gravity` (`Nullable<double>`) as exact binary64
/// bits, shared by the type reader and the weapon speed pass. RESIDUAL: a
/// value the float parse makes infinite (such as `1e39`) counts as unset;
/// Phobos would carry the infinity through its x87 gravity arithmetic.
pub(crate) fn read_phobos_gravity(section: &IniSection) -> Option<NativeF64Bits> {
    section
        .read_phobos_nullable_double("Gravity", None)
        .filter(|value| value.is_finite())
        .map(|value| NativeF64Bits::from_bits(value.to_bits()))
}

impl ProjectileType {
    /// Phobos `Splits=`: the Phobos emitter replaces the native tail and the
    /// parent's outer cluster loop, exactly as `BulletClass::Explodes`'s
    /// airburst checks do with `Airburst || Splits`. Without Phobos, Ares'
    /// `Splits=` does the same (`HasSplitBehavior`, BulletClass_Fire_SplitsA/B
    /// `1001A7E0`/`1001A810`, hooks `0x468EB9`/`0x468FFA`). False in stock:
    /// a stock-way read leaves the Ares key unread.
    pub const fn splits(&self) -> bool {
        self.phobos_airburst.splits
    }

    /// Phobos `BulletTypeExt::Gravity`, unset when the key is absent.
    pub fn gravity(&self) -> Option<NativeF64Bits> {
        self.gravity
    }

    /// Phobos `SubjectToGround=` (`Ext/BulletType/Body.cpp:56`).
    pub fn phobos_subject_to_ground(&self) -> bool {
        self.phobos_subject_to_ground
    }

    /// `Airburst=` or a game's `Splits=`: the BulletClass impact and detonation
    /// arms the `BulletClass::Explodes` hooks select.
    pub const fn airburst_arm(&self) -> bool {
        self.airburst || self.splits()
    }

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
            alpha_image: section.read_object_alpha_image(""),
            // Targeting
            aa: section.read_bool("AA", false),
            ag: section.read_bool("AG", true),
            aa_only: crate::rules::kratos_switch::kratos_read(|| {
                section.read_bool("AAOnly", false)
            }),
            av: section.read_bool("AV", false),
            au: section.read_bool("AU", false),
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
            inviso: false,
            proximity: section.read_bool("Proximity", false),
            ranged: section.read_bool("Ranged", false),
            rotates: true,
            flak_scatter: section.read_bool("FlakScatter", false),
            degenerates: false,
            bouncy: section.read_bool("Bouncy", false),
            anim_palette: false,
            firers_palette: section.read_bool("FirersPalette", false),
            scalable: section.read_bool("Scalable", false),
            vertical: section.read_bool("Vertical", false),
            phobos_vertical_initial_facing: section
                .read_phobos_nullable_bool("VerticalInitialFacing", None),
            vertical_aircraft_fix: section.read_phobos_nullable_bool("Vertical.AircraftFix", None),
            phobos_arcing_allow_elevation_inaccuracy: section
                .read_phobos_nullable_bool("Arcing.AllowElevationInaccuracy", None),
            flat: false,
            // Integer fields
            strength: section.read_int("Strength", 0),
            armor: section.read_phobos_nullable_with("Armor", None, |raw| {
                crate::rules::armor::try_armor_index(raw)
            }),
            interceptable: section.read_phobos_nullable_bool("Interceptable", None),
            interceptable_delete_on_intercept: section
                .read_bool("Interceptable.DeleteOnIntercept", false),
            interceptable_weapon_override: section.read_phobos_nullable_with(
                "Interceptable.WeaponOverride",
                None,
                |raw| Some(raw.to_owned()),
            ),
            cluster_scatter_min: section.read_phobos_leptons("ClusterScatter.Min", 256),
            cluster_scatter_max: section.read_phobos_leptons("ClusterScatter.Max", 512),
            shrapnel_affects_ground: section
                .read_phobos_nullable_bool("Shrapnel.AffectsGround", None),
            shrapnel_affects_buildings: section
                .read_phobos_nullable_bool("Shrapnel.AffectsBuildings", None),
            shrapnel_ignore_hit_buildings: section
                .read_phobos_nullable_bool("Shrapnel.IgnoreHitBuildings", None),
            shrapnel_use_weapon_targeting: section
                .read_phobos_nullable_bool("Shrapnel.UseWeaponTargeting", None),
            shrapnel_obey_warhead_trigger_conditions: section
                .read_phobos_nullable_bool("Shrapnel.ObeyWarheadTriggerConditions", None),
            cluster: section.read_int("Cluster", 1).min(CLUSTER_LIMIT),
            shrapnel_count: section
                .read_int("ShrapnelCount", 0)
                .clamp(-SHRAPNEL_COUNT_LIMIT, SHRAPNEL_COUNT_LIMIT),
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
            gravity: read_phobos_gravity(section),
            // Constructor46BCA4. Resolution requires the pass-local Colors
            // registry and is supplied by the retained rules processor.
            color_scheme: 0,
            // Constructor null; only the ordered Process owner reads it.
            shrapnel_weapon: None,
            airburst_weapon: None,
            phobos_airburst: crate::rules::phobos_airburst::PhobosAirburstType::from_section(
                section,
            ),
            phobos_trajectory: crate::rules::phobos_trajectory::PhobosTrajectoryRules::from_section(
                section,
            ),
            laser_trail_types: Vec::new(),
            phobos_subject_to_ground: section.read_ares_bool("SubjectToGround", false),
            phobos_parabomb: crate::rules::phobos_parabomb::PhobosParabomb::from_section(section),
            phobos_subject_to_terrain:
                crate::rules::phobos_bullet_terrain::PhobosSubjectToTerrain::from_section(section),
            phobos_return_weapon:
                crate::rules::phobos_return_weapon::PhobosReturnWeapon::from_section(section),
            bullet_type_ext: if crate::rules::ares_switch::ares_way() {
                crate::rules::ares_bullet_type::BulletTypeExtRules::from_section(section)
            } else {
                Default::default()
            },
            image: None,
            image_loads: Vec::new(),
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
        let mut image = ObjectImageState::new(id);
        image.read_input(section);
        image.read_art(image_section);
        image.load_for_read(ObjectImageKind::Ordinary, ImageTheater::Uninitialized, None);
        let mut art = ProjectileArtState::new();
        art.read_bullet_fields(
            &mut image,
            ImageTheater::Uninitialized,
            section,
            image_section,
            &[],
            |incoming| {
                (!incoming.eq_ignore_ascii_case("none") && !incoming.eq_ignore_ascii_case("<none>"))
                    .then(|| incoming.chars().take(24).collect())
            },
        );
        art.apply_to(&image, &mut result);
        result
    }
}

#[cfg(all(test, tests_rules_o_z))]
mod tests {
    use super::*;
    use crate::rules::ini_parser::IniFile;

    /// `BulletTypeExt::LoadFromINIFile` reads `LaserTrail.Types` from the ART
    /// `ImageFile` section: the authored `Image=` when present, otherwise the
    /// type ID (`Ext/BulletType/Body.cpp:89..94`).
    #[test]
    fn laser_trail_types_come_from_the_art_image_section() {
        let rules = IniFile::from_str("[RULES]\nImage=ARTNAME\n[PLAIN]\nImage=PLAIN\n");
        let art =
            IniFile::from_str("[ARTNAME]\nLaserTrail.Types=A, B\n[PLAIN]\nLaserTrail.Types=C\n");
        let projectile = ProjectileType::from_ini_section(
            "RULES",
            rules.section("RULES").unwrap(),
            art.section("ARTNAME"),
        );
        assert_eq!(projectile.laser_trail_types, ["A", "B"]);
        let plain = ProjectileType::from_ini_section(
            "PLAIN",
            rules.section("PLAIN").unwrap(),
            art.section("PLAIN"),
        );
        assert_eq!(plain.laser_trail_types, ["C"]);
    }

    #[test]
    fn degenerates_retains_reader_defaults_through_layers_and_save() {
        use crate::rules::native_processing::{NativeRulesRegistryState, RulesLayerStack};
        use crate::rules::ruleset::RuleSet;
        use crate::util::save_codec;

        let first = RulesLayerStack::new(IniFile::from_str(
            "[VehicleTypes]\n0=UNIT\n[UNIT]\nPrimary=GUN\n[GUN]\nProjectile=SHOT\n[SHOT]\nDegenerates=yes\n",
        )).process().unwrap();
        let (_, trace) = first.into_ini_and_native_type_construction_trace();
        let mut registry = trace.into_registry_state_discarding_events();
        for (body, expected) in [
            ("[SHOT]\nDegenerates=invalid\n", true),
            ("[SHOT]\nName=no key\n", true),
            ("[Other]\nName=no section\n", true),
            ("[SHOT]\nDegenerates=no\n", false),
            ("[SHOT]\nDegenerates=invalid\n", false),
        ] {
            // SavedScenarioResources retains this same process registry;
            // loading must not reparse an invalid token against false.
            let bytes = save_codec::serialize(&registry).unwrap();
            let loaded: NativeRulesRegistryState = save_codec::deserialize(&bytes).unwrap();
            let next = RulesLayerStack::new(IniFile::from_str(body))
                .process_with_fixed_art_and_registry_state(&IniFile::empty(), loaded)
                .unwrap();
            assert_eq!(
                RuleSet::from_processed_rules(&next)
                    .unwrap()
                    .projectile("SHOT")
                    .unwrap()
                    .degenerates,
                expected,
                "{body}"
            );
            let (_, trace) = next.into_ini_and_native_type_construction_trace();
            registry = trace.into_registry_state_discarding_events();
        }
    }

    #[test]
    fn degenerates_retained_value_changes_effective_rules_hash() {
        use crate::rules::native_processing::{RulesLayerKind, RulesLayerStack};
        use crate::rules::ruleset::RuleSet;

        let rules = |initial| {
            let first = IniFile::from_str(&format!(
                "[VehicleTypes]\n0=UNIT\n[UNIT]\nPrimary=GUN\n[GUN]\nProjectile=SHOT\n[SHOT]\nDegenerates={initial}\n",
            ));
            let mut layers = RulesLayerStack::new(first);
            layers.push(
                RulesLayerKind::Scenario,
                IniFile::from_str("[SHOT]\nDegenerates=invalid\n"),
            );
            RuleSet::from_processed_rules(&layers.process().unwrap()).unwrap()
        };
        let enabled = rules("yes");
        let disabled = rules("no");
        assert!(enabled.projectile("SHOT").unwrap().degenerates);
        assert!(!disabled.projectile("SHOT").unwrap().degenerates);
        assert_ne!(
            enabled.simulation_config_hash(),
            disabled.simulation_config_hash(),
            "equal final raw tokens do not erase retained gameplay inputs"
        );
    }

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

    /// Phobos `Nullable<double> Gravity`: absent keeps the Rules fallback
    /// (None), `<default>` clears it, a value keeps its exact binary64, and an
    /// infinite one is dropped.
    #[test]
    fn phobos_gravity_reads_nullable_double_and_clears() {
        let ini =
            IniFile::from_str("[P]\nGravity=6.5\n[Q]\nGravity=<default>\n[R]\nGravity=1e39\n");
        let gravity = |name: &str| {
            ProjectileType::from_ini_section(name, ini.section(name).unwrap(), None).gravity()
        };
        assert_eq!(
            gravity("P"),
            Some(NativeF64Bits::from_bits(6.5f64.to_bits()))
        );
        assert_eq!(gravity("Q"), None);
        assert_eq!(gravity("R"), None);
    }

    /// Phobos `AU` (`Ext/BulletType/Body.cpp:83`): the two-letter key is read
    /// as a plain bool, absent false.
    #[test]
    fn phobos_au_reads_as_a_bool() {
        let ini = IniFile::from_str("[P]\nAU=yes\n[Q]\nAU=no\n[R]\nAA=no\n");
        let read = |name: &str| {
            ProjectileType::from_ini_section(name, ini.section(name).unwrap(), None).au
        };
        assert!(read("P"));
        assert!(!read("Q"));
        assert!(!read("R"));
    }

    /// Phobos `ClusterScatter.Min=`/`Max=` (`Valueable<Leptons>`): absent
    /// keeps the constructor's 256/512 leptons, a value is read in cells and
    /// truncated to leptons, and `<default>` keeps the current value.
    #[test]
    fn phobos_cluster_scatter_reads_cell_leptons_with_constructor_defaults() {
        let ini = IniFile::from_str(
            "[P]\nClusterScatter.Min=0.5\nClusterScatter.Max=2.25\n\
             [Q]\nClusterScatter.Min=<default>\nClusterScatter.Max=<default>\n",
        );
        let scatter = |name: &str| {
            let projectile =
                ProjectileType::from_ini_section(name, ini.section(name).unwrap(), None);
            (
                projectile.cluster_scatter_min,
                projectile.cluster_scatter_max,
            )
        };
        assert_eq!(scatter("P"), (128, 576));
        assert_eq!(scatter("Q"), (256, 512));
    }

    #[test]
    fn huge_fragment_counts_stop_at_their_bounds() {
        let ini = IniFile::from_str(
            "[A]\nShrapnelCount=2147483647\nCluster=2147483647\n[B]\nShrapnelCount=-2147483648\n",
        );
        let read = |name| ProjectileType::from_ini_section(name, ini.section(name).unwrap(), None);
        assert_eq!(read("A").shrapnel_count, SHRAPNEL_COUNT_LIMIT);
        assert_eq!(read("A").cluster, CLUSTER_LIMIT);
        assert_eq!(read("B").shrapnel_count, -SHRAPNEL_COUNT_LIMIT);
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
        assert!(proj.shrapnel_weapon.is_none());
        assert!(proj.trailer.is_none());
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

    /// Phobos `Vertical.AircraftFix` (`Ext/BulletType/Body.cpp:36`,
    /// `Nullable`) and the `[General]` fallback (`Ext/Rules/Body.cpp:581`,
    /// constructing true).
    #[test]
    fn vertical_aircraft_fix_reads_the_bullet_key_and_global() {
        let read = |text: &str| {
            let ini = IniFile::from_str(text);
            ProjectileType::from_ini_section("SHELL", ini.section_or_empty("SHELL"), None)
        };
        let set = read("[SHELL]\nVertical=yes\nVertical.AircraftFix=no\n");
        assert!(set.vertical);
        assert_eq!(set.vertical_aircraft_fix, Some(false));
        let unset = read("[SHELL]\nVertical=yes\n");
        assert_eq!(unset.vertical_aircraft_fix, None);

        let globals = crate::rules::phobos_globals::PhobosGlobals::from_ini(&IniFile::from_str(
            "[General]\nVertical.AircraftFix=no\n",
        ));
        assert!(!globals.vertical_aircraft_fix);
        assert!(
            crate::rules::phobos_globals::PhobosGlobals::default().vertical_aircraft_fix,
            "the constructor value is yes"
        );
    }

    /// Package 45 G1.2 `Arcing.AllowElevationInaccuracy` (`Ext/BulletType/
    /// Body.cpp:53`): a `Nullable<bool>` whose unset state defers to the
    /// `[CombatDamage]` global (default yes). Refusing it enables
    /// `BulletExt::ApplyArcingFix`.
    #[test]
    fn the_arcing_elevation_inaccuracy_nullable_reads_yes_no_and_unset() {
        let ini = IniFile::from_str(
            "[YES]\nArcing.AllowElevationInaccuracy=yes\n\
             [NO]\nArcing.AllowElevationInaccuracy=no\n\
             [UNSET]\nFixtureOnly=1\n",
        );
        let read = |id: &str| {
            ProjectileType::from_ini_section(id, ini.section(id).unwrap(), None)
                .phobos_arcing_allow_elevation_inaccuracy
        };
        assert_eq!(read("YES"), Some(true));
        assert_eq!(read("NO"), Some(false));
        assert_eq!(read("UNSET"), None);
    }
}

#[cfg(all(test, tests_rules_o_z))]
#[path = "projectile_line_trail_tests.rs"]
mod line_trail_tests;
