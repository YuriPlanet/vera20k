//! ParticleSystemType — container that owns particles, manages spawning, and dispatches AI.
//!
//! Each `[ParticleSystemName]` section in rulesmd.ini defines one ParticleSystemType.
//! A `ParticleSystem` (runtime instance) is created via `Simulation::spawn_particle_system`
//! by combat, damage events, refinery dumps, area damage, gap generators, and triggers.
//!
//! ## Dependency rules
//! - Part of rules/ — no dependencies on sim/, render/, ui/, etc.
//! - References `ParticleTypeId` from `crate::rules::particle_type`.

use glam::IVec3;
use serde::{Deserialize, Serialize};

use crate::rules::ini_parser::IniSection;
use crate::rules::particle_type::ParticleTypeId;
use crate::util::native_x87::{NativeF32Bits, NativeF64Bits};

/// Railgun doubles written by `ParticleSystemTypeClass::Constructor @
/// 0x006440A0` (`0x0064410E..0x0064414F`): `ParticlesPerCoord` 0.1,
/// `SpiralDeltaPerCoord` 0.025 and `SpiralRadius` 25.0. The three
/// perturbation coefficients start at 0.0.
const DEFAULT_PARTICLES_PER_COORD: NativeF64Bits = NativeF64Bits::from_bits(0x3fb9_9999_9999_999a);
const DEFAULT_SPIRAL_DELTA_PER_COORD: NativeF64Bits =
    NativeF64Bits::from_bits(0x3f99_9999_9999_999a);
const DEFAULT_SPIRAL_RADIUS: NativeF64Bits = NativeF64Bits::from_bits(0x4039_0000_0000_0000);

/// Interned identifier for a `ParticleSystemType`. Resolved at INI parse time;
/// consumers (TechnoType, WeaponType, RulesClass) store the ID, not the name.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Ord, PartialOrd, Serialize, Deserialize)]
pub struct ParticleSystemTypeId(pub u32);

/// System-level behavior dispatch enum.
///
/// Variant ordering matches the binary's string table:
/// `Smoke=0, Gas=1, Fire=2, Spark=3, Railgun=4`. This is **different** from
/// `ParticleBehavesLike` — Smoke and Gas are swapped at the particle level.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[repr(i32)]
pub enum ParticleSystemBehavesLike {
    Smoke = 0,
    Gas = 1,
    Fire = 2,
    Spark = 3,
    Railgun = 4,
    /// Constructor and unrecognized/empty ReadINI result.
    #[default]
    Unknown = -1,
}

impl ParticleSystemBehavesLike {
    /// Parse a `BehavesLike=` value from INI. Returns `None` for unknown strings.
    pub fn parse(s: &str) -> Option<Self> {
        [
            Self::Smoke,
            Self::Gas,
            Self::Fire,
            Self::Spark,
            Self::Railgun,
        ]
        .into_iter()
        .zip(["Smoke", "Gas", "Fire", "Spark", "Railgun"])
        .find_map(|(behavior, name)| s.eq_ignore_ascii_case(name).then_some(behavior))
    }
}

/// A particle system definition parsed from a `[ParticleSystemName]` section.
///
/// The binary reads every behavior's keys for every system, so the Railgun
/// and Spark fields are parsed whatever `BehavesLike=` says.
#[derive(Debug, Clone)]
pub struct ParticleSystemType {
    // ── Identity ─────────────────────────────────────────────────────
    /// Section name in rulesmd.ini (e.g., "BigGreySmokeSys").
    pub name: String,
    /// ObjectType+0x1F8 Image buffer. `ObjectTypeClass::Constructor @
    /// 0x005F7090` starts with the AbstractType ID; `ParticleSystemTypeClass::
    /// ReadINI @ 0x006442D0` reaches `ObjectTypeClass::ReadINI @ 0x005F92D0`,
    /// which retains the current 25-byte string when Image is missing or empty.
    pub image: String,
    /// Inherited `ObjectTypeClass::ReadINI @ 0x005F92D0` AlphaImage buffer
    /// (`+0x213`, `char[25]`), separate from the particle system's Image.
    pub(crate) alpha_image: String,
    /// System-level behavior dispatch — selects the per-tick spawn / AI branch.
    pub behaves_like: ParticleSystemBehavesLike,

    // ── Core spawn / lifetime ────────────────────────────────────────
    /// Resolved `HoldsWhat` reference (the particle this system spawns),
    /// ParticleSystemType+294. The ordered Rules processor binds it on every
    /// reached pass and `RuleSet` resolves the binding.
    pub holds_what: Option<ParticleTypeId>,
    /// Whether this system spawns new particles over time (vs. a one-shot).
    pub spawns: bool,
    /// Frame interval between spawns (default 1).
    pub spawn_frames: i32,
    /// Rate at which smoke particles decelerate (default 0.0).
    pub slowdown: NativeF32Bits,
    /// Maximum particles this system can hold simultaneously (default 50).
    pub particle_cap: u32,
    /// Random radius for spawn position offset.
    pub spawn_radius: i32,
    /// Distance threshold to stop spawning new particles.
    pub spawn_cutoff: NativeF32Bits,
    /// Distance threshold to start fading new particles.
    pub spawn_translucency_cutoff: NativeF32Bits,
    /// System lifetime in frames. -1 = infinite (system stays alive until all
    /// particles die). Default -1 per constructor.
    pub lifetime: i32,
    /// Direction vector for spawning (CoordStruct).
    pub spawn_direction: IVec3,

    // ── Railgun-only: the exact `ReadDouble` doubles that
    // `ParticleSystemClass::AI_Railgun @ 0x0062F230` reads ─────────────
    /// Particles per lepton of beam length (`+0x2C8`).
    pub particles_per_coord: NativeF64Bits,
    /// Spiral angle per lepton of beam length (`+0x2D0`).
    pub spiral_delta_per_coord: NativeF64Bits,
    /// Spiral radius in leptons (`+0x2D8`).
    pub spiral_radius: NativeF64Bits,
    /// Random position offset scale (`+0x2E0`).
    pub position_perturbation_coefficient: NativeF64Bits,
    /// Random movement offset scale (`+0x2E8`).
    pub movement_perturbation_coefficient: NativeF64Bits,
    /// Random speed offset scale (`+0x2F0`).
    pub velocity_perturbation_coefficient: NativeF64Bits,

    // ── Spark-only ───────────────────────────────────────────────────
    /// Probability of taking a spark burst this tick.
    ///
    /// Stored as the exact widened `ReadDouble` result, not `SimFixed`:
    /// `ParticleSystemClass::AI_Spark @ 0x0062E840` compares the scaled RNG
    /// draw against this as a *double* (`FCOMP double ptr [ECX + 0x2f8]` at
    /// `0x0062E88D`). `[LGSparkSys] SpawnSparkPercentage=.2` is not
    /// representable in `SimFixed`, and the ~3.05e-6 quantisation gap flips the
    /// gate often enough to diverge the shared RNG stream.
    pub spawn_spark_percentage: NativeF64Bits,
    /// Spark spawn frame counter.
    pub spark_spawn_frames: u32,
    /// Light radius for spark systems.
    pub light_size: i32,
    /// Spark light only lasts one frame.
    pub one_frame_light: bool,
    /// Whether to draw a railgun laser line.
    pub laser: bool,
    /// Railgun laser beam color.
    pub laser_color: [u8; 3],
}

impl ParticleSystemType {
    /// Parse a ParticleSystemType from an INI section.
    ///
    /// `holds_what` keeps the constructor's unbound value (-1, `0x006440B3`):
    /// `RuleSet` resolves the ordered Rules processor's binding.
    #[cfg(test)]
    pub fn from_ini_section(name: &str, section: &IniSection) -> Self {
        Self::from_read_state(name, section, Self::read_behavior(section), None)
    }

    pub(super) fn read_behavior(section: &IniSection) -> ParticleSystemBehavesLike {
        // ParticleSystemTypeClass::ReadINI @ 0x006442D0,64441E..644461:
        // ReadString64 then strcmpi; absent and unknown store -1.
        section
            .read_name("BehavesLike", 0x40)
            .and_then(ParticleSystemBehavesLike::parse)
            .unwrap_or(ParticleSystemBehavesLike::Unknown)
    }

    pub(super) fn from_read_state(
        name: &str,
        section: &IniSection,
        behaves_like: ParticleSystemBehavesLike,
        holds_what: Option<ParticleTypeId>,
    ) -> Self {
        Self {
            name: name.to_string(),
            image: section.read_name("Image", 0x19).unwrap_or(name).to_owned(),
            alpha_image: section.read_object_alpha_image(""),
            behaves_like,
            holds_what,
            spawns: section.read_bool("Spawns", false),
            spawn_frames: section.read_int("SpawnFrames", 1),
            // Float fields (`FSTP dword` at `0x0064439F..0x006443DD`).
            slowdown: section.read_double_to_float("Slowdown", NativeF32Bits::POSITIVE_ZERO),
            particle_cap: section.read_int("ParticleCap", 50).max(0) as u32,
            spawn_radius: section.read_int("SpawnRadius", 0),
            spawn_cutoff: section.read_double_to_float("SpawnCutoff", NativeF32Bits::POSITIVE_ZERO),
            spawn_translucency_cutoff: section
                .read_double_to_float("SpawnTranslucencyCutoff", NativeF32Bits::POSITIVE_ZERO),
            lifetime: section.read_int("Lifetime", -1),
            // A float vector natively; VERA keeps whole components for its
            // integer velocity math. Retail sets no `SpawnDirection=`.
            spawn_direction: IVec3::from_array(
                section
                    .read_float_tokens("SpawnDirection", [0.0; 3])
                    .map(|component| component as i32),
            ),

            // Double fields (`FSTP qword` at `0x006444B0..0x00644555`).
            particles_per_coord: section
                .read_double_bits("ParticlesPerCoord", DEFAULT_PARTICLES_PER_COORD),
            spiral_delta_per_coord: section
                .read_double_bits("SpiralDeltaPerCoord", DEFAULT_SPIRAL_DELTA_PER_COORD),
            spiral_radius: section.read_double_bits("SpiralRadius", DEFAULT_SPIRAL_RADIUS),
            position_perturbation_coefficient: section.read_double_bits(
                "PositionPerturbationCoefficient",
                NativeF64Bits::POSITIVE_ZERO,
            ),
            movement_perturbation_coefficient: section.read_double_bits(
                "MovementPerturbationCoefficient",
                NativeF64Bits::POSITIVE_ZERO,
            ),
            velocity_perturbation_coefficient: section.read_double_bits(
                "VelocityPerturbationCoefficient",
                NativeF64Bits::POSITIVE_ZERO,
            ),

            spawn_spark_percentage: NativeF64Bits::from_bits(
                section.read_double("SpawnSparkPercentage", 0.0).to_bits(),
            ),
            spark_spawn_frames: section.read_int("SparkSpawnFrames", 0).max(0) as u32,
            light_size: section.read_int("LightSize", 0),
            one_frame_light: section.read_bool("OneFrameLight", false),
            laser: section.read_bool("Laser", false),
            laser_color: section.read_color_rgb("LaserColor", [0, 0, 0], None),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rules::ini_parser::IniFile;

    #[test]
    fn behaves_like_discriminants_match_binary() {
        // System-level enum: Smoke=0, Gas=1 (NOT Gas=0 like the particle-level enum).
        assert_eq!(ParticleSystemBehavesLike::Smoke as u8, 0);
        assert_eq!(ParticleSystemBehavesLike::Gas as u8, 1);
        assert_eq!(ParticleSystemBehavesLike::Fire as u8, 2);
        assert_eq!(ParticleSystemBehavesLike::Spark as u8, 3);
        assert_eq!(ParticleSystemBehavesLike::Railgun as u8, 4);
    }

    #[test]
    fn defaults_match_binary_constructor() {
        let ini = IniFile::from_str("[Foo]\nFixtureOnly=1\n");
        let section = ini.section("Foo").unwrap();
        let pst = ParticleSystemType::from_ini_section("Foo", section);

        assert_eq!(pst.lifetime, -1);
        assert_eq!(pst.particle_cap, 50);
        assert_eq!(pst.spawn_frames, 1);
        assert_eq!(pst.particles_per_coord.bits(), 0x3fb9_9999_9999_999a);
        assert_eq!(pst.spiral_delta_per_coord.bits(), 0x3f99_9999_9999_999a);
        assert_eq!(pst.spiral_radius.bits(), 0x4039_0000_0000_0000);
        assert_eq!(pst.slowdown, NativeF32Bits::POSITIVE_ZERO);
        assert_eq!(pst.spawn_radius, 0);
        assert_eq!(pst.spawn_direction, IVec3::ZERO);
        assert!(!pst.spawns);
        assert!(!pst.laser);
        assert!(!pst.one_frame_light);
        assert_eq!(pst.laser_color, [0, 0, 0]);
        // The native constructor and unknown reader result are -1.
        assert_eq!(pst.behaves_like, ParticleSystemBehavesLike::Unknown);
    }

    #[test]
    fn from_ini_parses_smoke_system_like_section() {
        let ini = IniFile::from_str(
            "[BigGreySSys]\n\
             BehavesLike=Smoke\n\
             HoldsWhat=GreySmoke\n\
             Spawns=yes\n\
             SpawnFrames=18\n\
             ParticleCap=15\n\
             Slowdown=0.4\n\
             Lifetime=200\n",
        );
        let section = ini.section("BigGreySSys").unwrap();
        let pst = ParticleSystemType::from_ini_section("BigGreySSys", section);

        assert_eq!(pst.name, "BigGreySSys");
        assert_eq!(pst.behaves_like, ParticleSystemBehavesLike::Smoke);
        // The ordered Rules processor binds HoldsWhat; RuleSet resolves it.
        assert_eq!(pst.holds_what, None);
        assert!(pst.spawns);
        assert_eq!(pst.spawn_frames, 18);
        assert_eq!(pst.particle_cap, 15);
        // ReadDouble5283D0 scans %f before widening. The subsequent chop
        // store preserves that already-rounded binary32 value.
        assert_eq!(pst.slowdown.bits(), 0x3ecc_cccd);
        assert_eq!(pst.lifetime, 200);
    }

    #[test]
    fn from_ini_unknown_behaves_like_is_minus_one() {
        let ini = IniFile::from_str("[Foo]\nBehavesLike=Bogus\n");
        let section = ini.section("Foo").unwrap();
        let pst = ParticleSystemType::from_ini_section("Foo", section);
        assert_eq!(pst.behaves_like, ParticleSystemBehavesLike::Unknown);
    }
}
