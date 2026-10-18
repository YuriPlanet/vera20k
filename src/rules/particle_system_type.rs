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
use crate::util::fixed_math::{SimFixed, sim_from_f32};
use crate::util::native_x87::{NativeF32Bits, NativeF64Bits};

/// Default `ParticlesPerCoord` from the constructor (Railgun field, parsed
/// for every system).
const DEFAULT_PARTICLES_PER_COORD: SimFixed = SimFixed::lit("0.1");
/// Default `SpiralDeltaPerCoord` from the constructor.
const DEFAULT_SPIRAL_DELTA_PER_COORD: SimFixed = SimFixed::lit("0.025");
/// Default `SpiralRadius` from the constructor.
const DEFAULT_SPIRAL_RADIUS: SimFixed = SimFixed::lit("25");

/// Interned identifier for a `ParticleSystemType`. Resolved at INI parse time;
/// consumers (TechnoType, WeaponType, RulesClass) store the ID, not the name.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Ord, PartialOrd, Serialize, Deserialize)]
pub struct ParticleSystemTypeId(pub u32);

/// System-level behavior dispatch enum.
///
/// Variant ordering matches the binary's string table:
/// `Smoke=0, Gas=1, Fire=2, Spark=3, Railgun=4`. This is **different** from
/// `ParticleBehavesLike` — Smoke and Gas are swapped at the particle level.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[repr(u8)]
pub enum ParticleSystemBehavesLike {
    Smoke = 0,
    Gas = 1,
    Fire = 2,
    Spark = 3,
    Railgun = 4,
}

impl ParticleSystemBehavesLike {
    /// Parse a `BehavesLike=` value from INI. Returns `None` for unknown strings.
    pub fn parse(s: &str) -> Option<Self> {
        match s.trim() {
            "Smoke" => Some(Self::Smoke),
            "Gas" => Some(Self::Gas),
            "Fire" => Some(Self::Fire),
            "Spark" => Some(Self::Spark),
            "Railgun" => Some(Self::Railgun),
            _ => None,
        }
    }
}

/// A particle system definition parsed from a `[ParticleSystemName]` section.
///
/// Tier 3 fields (Railgun spiral params, Spark percentages, laser color) are
/// parsed but unused at Tier 2 — the binary parses them unconditionally and
/// we mirror that so Tier 3 lights up cleanly later.
#[derive(Debug, Clone)]
pub struct ParticleSystemType {
    // ── Identity ─────────────────────────────────────────────────────
    /// Section name in rulesmd.ini (e.g., "BigGreySmokeSys").
    pub name: String,
    /// System-level behavior dispatch — selects the per-tick spawn / AI branch.
    pub behaves_like: ParticleSystemBehavesLike,

    // ── Core spawn / lifetime ────────────────────────────────────────
    /// Resolved `HoldsWhat` reference (the particle this system spawns).
    /// Always `None` after A3; the 2-pass resolver in Task A4 fills it in.
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

    // ── Railgun-only (Tier 3 — parsed but unused) ────────────────────
    /// Particles spawned per coordinate unit along a railgun beam.
    pub particles_per_coord: SimFixed,
    /// Railgun spiral angle increment per coord.
    pub spiral_delta_per_coord: SimFixed,
    /// Radius of the railgun spiral pattern.
    pub spiral_radius: SimFixed,
    /// Random position offset scale.
    pub position_perturbation_coefficient: SimFixed,
    /// Random movement offset scale.
    pub movement_perturbation_coefficient: SimFixed,
    /// Random velocity perturbation scale.
    pub velocity_perturbation_coefficient: SimFixed,

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

/// Parse-state wrapper around a `ParticleSystemType` whose `holds_what`
/// reference has not yet been resolved.
///
/// `RuleSet::from_ini` collects these in pass 1, then resolves
/// `holds_what_name` against the name → ID map in pass 2.
pub struct PendingParticleSystemType {
    /// The parsed type with `holds_what = None`.
    pub partial: ParticleSystemType,
    /// Captured `HoldsWhat=` value to resolve in pass 2.
    pub holds_what_name: Option<String>,
}

impl ParticleSystemType {
    /// Parse a ParticleSystemType from an INI section.
    ///
    /// `holds_what` is always `None` — the resolver in `RuleSet::from_ini`
    /// fills it in after all particle types have been collected.
    /// Use `from_ini_section_pending` if you need the unresolved reference name.
    #[cfg(test)]
    pub fn from_ini_section(name: &str, section: &IniSection) -> Self {
        Self::from_ini_section_pending(name, section).partial
    }

    /// Parse a ParticleSystemType plus its unresolved `HoldsWhat=` name string.
    pub fn from_ini_section_pending(name: &str, section: &IniSection) -> PendingParticleSystemType {
        let behaves_like = section
            .read_name("BehavesLike", 0x40)
            .and_then(ParticleSystemBehavesLike::parse)
            // Binary's string-table loop falls through to index 0 (Smoke) when
            // the INI string doesn't match any entry.
            .unwrap_or(ParticleSystemBehavesLike::Smoke);

        let holds_what_name = section
            .read_name("HoldsWhat", 0x40)
            .filter(|s| !s.eq_ignore_ascii_case("none"))
            .map(str::to_owned);

        let partial = Self {
            name: name.to_string(),
            behaves_like,
            holds_what: None,
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
            particles_per_coord: sim_from_f32(
                section.read_double("ParticlesPerCoord", DEFAULT_PARTICLES_PER_COORD.to_num())
                    as f32,
            ),
            spiral_delta_per_coord: sim_from_f32(section.read_double(
                "SpiralDeltaPerCoord",
                DEFAULT_SPIRAL_DELTA_PER_COORD.to_num(),
            ) as f32),
            spiral_radius: sim_from_f32(
                section.read_double("SpiralRadius", DEFAULT_SPIRAL_RADIUS.to_num()) as f32,
            ),
            position_perturbation_coefficient: sim_from_f32(
                section.read_double("PositionPerturbationCoefficient", 0.0) as f32,
            ),
            movement_perturbation_coefficient: sim_from_f32(
                section.read_double("MovementPerturbationCoefficient", 0.0) as f32,
            ),
            velocity_perturbation_coefficient: sim_from_f32(
                section.read_double("VelocityPerturbationCoefficient", 0.0) as f32,
            ),

            spawn_spark_percentage: NativeF64Bits::from_bits(
                section.read_double("SpawnSparkPercentage", 0.0).to_bits(),
            ),
            spark_spawn_frames: section.read_int("SparkSpawnFrames", 0).max(0) as u32,
            light_size: section.read_int("LightSize", 0),
            one_frame_light: section.read_bool("OneFrameLight", false),
            laser: section.read_bool("Laser", false),
            laser_color: section.read_color_rgb("LaserColor", [0, 0, 0], None),
        };

        PendingParticleSystemType {
            partial,
            holds_what_name,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rules::ini_parser::IniFile;

    #[test]
    fn behaves_like_string_to_enum() {
        assert_eq!(
            ParticleSystemBehavesLike::parse("Smoke"),
            Some(ParticleSystemBehavesLike::Smoke)
        );
        assert_eq!(
            ParticleSystemBehavesLike::parse("Gas"),
            Some(ParticleSystemBehavesLike::Gas)
        );
        assert_eq!(
            ParticleSystemBehavesLike::parse("Fire"),
            Some(ParticleSystemBehavesLike::Fire)
        );
        assert_eq!(
            ParticleSystemBehavesLike::parse("Spark"),
            Some(ParticleSystemBehavesLike::Spark)
        );
        assert_eq!(
            ParticleSystemBehavesLike::parse("Railgun"),
            Some(ParticleSystemBehavesLike::Railgun)
        );
        assert_eq!(ParticleSystemBehavesLike::parse("nope"), None);
    }

    #[test]
    fn behaves_like_parse_trims_whitespace() {
        assert_eq!(
            ParticleSystemBehavesLike::parse("  Smoke  "),
            Some(ParticleSystemBehavesLike::Smoke)
        );
    }

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
    fn system_and_particle_enum_have_swapped_smoke_gas() {
        // Critical asymmetry: at the SYSTEM level Smoke=0/Gas=1, but at the PARTICLE
        // level Gas=0/Smoke=1. Mismatching them is the kind of bug that produces
        // "smoke deals damage / gas drifts silently" symptoms.
        use crate::rules::particle_type::ParticleBehavesLike;
        assert_ne!(
            ParticleSystemBehavesLike::Smoke as u8,
            ParticleBehavesLike::Smoke as u8
        );
        assert_ne!(
            ParticleSystemBehavesLike::Gas as u8,
            ParticleBehavesLike::Gas as u8
        );
    }

    #[test]
    fn particle_system_type_id_is_copy_eq_hash() {
        let a = ParticleSystemTypeId(13);
        let b = a;
        assert_eq!(a, b);
        let mut set = std::collections::HashSet::new();
        set.insert(a);
        assert!(set.contains(&b));
    }

    #[test]
    fn defaults_match_binary_constructor() {
        let ini = IniFile::from_str("[Foo]\nFixtureOnly=1\n");
        let section = ini.section("Foo").unwrap();
        let pst = ParticleSystemType::from_ini_section("Foo", section);

        assert_eq!(pst.lifetime, -1);
        assert_eq!(pst.particle_cap, 50);
        assert_eq!(pst.spawn_frames, 1);
        assert_eq!(pst.particles_per_coord, SimFixed::lit("0.1"));
        assert_eq!(pst.spiral_delta_per_coord, SimFixed::lit("0.025"));
        assert_eq!(pst.spiral_radius, SimFixed::lit("25"));
        assert_eq!(pst.slowdown, NativeF32Bits::POSITIVE_ZERO);
        assert_eq!(pst.spawn_radius, 0);
        assert_eq!(pst.spawn_direction, IVec3::ZERO);
        assert!(!pst.spawns);
        assert!(!pst.laser);
        assert!(!pst.one_frame_light);
        assert_eq!(pst.laser_color, [0, 0, 0]);
        // Unknown / absent BehavesLike falls through to the index-0 entry (Smoke).
        assert_eq!(pst.behaves_like, ParticleSystemBehavesLike::Smoke);
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
        // HoldsWhat is unresolved at A3 — the string is captured and resolved in A4.
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
    fn from_ini_parses_railgun_system() {
        let ini = IniFile::from_str(
            "[RGSys]\n\
             BehavesLike=Railgun\n\
             ParticlesPerCoord=0.5\n\
             SpiralDeltaPerCoord=0.05\n\
             SpiralRadius=12.0\n\
             Laser=yes\n\
             LaserColor=255,128,64\n",
        );
        let section = ini.section("RGSys").unwrap();
        let pst = ParticleSystemType::from_ini_section("RGSys", section);

        assert_eq!(pst.behaves_like, ParticleSystemBehavesLike::Railgun);
        assert_eq!(pst.particles_per_coord, sim_from_f32(0.5));
        assert_eq!(pst.spiral_delta_per_coord, sim_from_f32(0.05));
        assert_eq!(pst.spiral_radius, sim_from_f32(12.0));
        assert!(pst.laser);
        assert_eq!(pst.laser_color, [255, 128, 64]);
    }

    #[test]
    fn from_ini_parses_spawn_direction() {
        let ini = IniFile::from_str("[Foo]\nSpawnDirection=10,-5,42\n");
        let section = ini.section("Foo").unwrap();
        let pst = ParticleSystemType::from_ini_section("Foo", section);
        assert_eq!(pst.spawn_direction, IVec3::new(10, -5, 42));
    }

    #[test]
    fn from_ini_unknown_behaves_like_falls_through_to_smoke() {
        let ini = IniFile::from_str("[Foo]\nBehavesLike=Bogus\n");
        let section = ini.section("Foo").unwrap();
        let pst = ParticleSystemType::from_ini_section("Foo", section);
        assert_eq!(pst.behaves_like, ParticleSystemBehavesLike::Smoke);
    }
}
