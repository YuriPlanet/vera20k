//! ParticleType — runtime parameters for a single particle (gas, smoke, fire, spark, railgun).
//!
//! Each `[ParticleName]` section in rulesmd.ini defines one ParticleType. Particles are
//! the leaf entity inside a `ParticleSystem`: they carry position, lifetime, animation
//! state, and (for Gas/Fire) deal damage to objects in their cell.
//!
//! ## Dependency rules
//! - Part of rules/ — no dependencies on sim/, render/, ui/, etc.

use glam::IVec3;
use serde::{Deserialize, Serialize};

use crate::rules::ini_parser::IniSection;

use crate::util::native_x87::{NativeF32Bits, NativeF64Bits};

/// Interned identifier for a `ParticleType`. Resolved at INI parse time;
/// cross-references between types (e.g., `NextParticle=`) store the ID, not the name.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Ord, PartialOrd, Serialize, Deserialize)]
pub struct ParticleTypeId(pub u32);

/// Per-particle behavior dispatch enum.
///
/// Variant ordering matches the binary's string table:
/// `Gas=0, Smoke=1, Fire=2, Spark=3, Railgun=4`. This is **different** from
/// `ParticleSystemBehavesLike` — Gas and Smoke are swapped at the system level.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[repr(u8)]
pub enum ParticleBehavesLike {
    Gas = 0,
    Smoke = 1,
    Fire = 2,
    Spark = 3,
    Railgun = 4,
}

impl ParticleBehavesLike {
    /// Parse a `BehavesLike=` value from INI. Returns `None` for unknown strings.
    pub fn parse(s: &str) -> Option<Self> {
        match s.trim() {
            "Gas" => Some(Self::Gas),
            "Smoke" => Some(Self::Smoke),
            "Fire" => Some(Self::Fire),
            "Spark" => Some(Self::Spark),
            "Railgun" => Some(Self::Railgun),
            _ => None,
        }
    }
}

/// A particle definition parsed from a `[ParticleName]` section in rulesmd.ini.
///
/// Tier 3 fields (Spark/Railgun: `XVelocity`, `ColorList`, etc.) are parsed but
/// unused at Tier 2 — the binary parses them unconditionally and we mirror that
/// so Tier 3 lights up cleanly later without re-parsing.
#[derive(Debug, Clone)]
pub struct ParticleType {
    // ── Identity ─────────────────────────────────────────────────────
    /// Section name in rulesmd.ini (e.g., "GasCloud1", "Smoke").
    pub name: String,
    /// Per-particle behavior dispatch — selects the AI branch each tick.
    pub behaves_like: ParticleBehavesLike,

    // ── Object-base (from ObjectTypeClass) ───────────────────────────
    /// SHP image for animated particles. Resolved later via the SHP cache.
    pub image: Option<String>,

    // ── [Particles] direct keys (§2.2) ───────────────────────────────
    /// Damage countdown reset value: frames between damage ticks (Gas/Fire).
    pub max_dc: i32,
    /// Lifetime in frames (countdown decremented each tick).
    pub max_ec: i32,
    /// Damage amount per damage tick.
    pub damage: i32,
    /// Warhead reference name, resolved later against the warhead registry.
    pub warhead: Option<String>,
    /// Frames per animation loop.
    #[cfg(test)]
    pub num_loop_frames: u16,
    /// Base translucency level: 0, 25, or 50.
    pub translucency: u8,
    /// Native full signed WindEffect, multiplied by the Smoke wind table.
    pub wind_effect: i32,
    /// Stored native binary32 speed. Smoke's repeated stores affect rise and
    /// its spawn cadence/RNG; retain those bits through the numeric owner.
    pub velocity: NativeF32Bits,
    /// Per-frame deceleration applied to velocity.
    pub deacc: NativeF32Bits,
    /// Interaction radius (cells).
    pub radius: i32,
    /// Delete the particle when its animation state hits the limit.
    pub delete_on_state_limit: bool,
    /// Final animation state — particle is marked for deletion at this state.
    pub end_state_ai: u8,
    /// Initial animation state (start of state machine).
    pub start_state_ai: u8,
    /// State advance rate divisor (default 4).
    pub state_ai_advance: u8,
    /// State at which damage stops being applied. Defaults to `end_state_ai`
    /// when the INI key is absent (binary reads with `+0x309` as default).
    pub final_damage_state: u8,
    /// State at which 25% translucency begins. 0xFF = never.
    pub translucent_25_state: u8,
    /// State at which 50% translucency begins. 0xFF = never.
    pub translucent_50_state: u8,
    /// Normalize the direction vector based on distance to target.
    pub normalized: bool,
    /// Resolved next-particle reference. Always `None` after A2; the
    /// 2-pass resolver introduced in Task A4 fills this in from the
    /// captured name string.
    pub next_particle: Option<ParticleTypeId>,
    /// Position offset applied when transitioning to next particle in a chain.
    pub next_particle_offset: IVec3,

    // ── ColorList runtime (§11.1) ────────────────────────────────────
    /// Packed RGB triplets parsed from `ColorList=`. Stride is 3 bytes per entry,
    /// no padding; empty/missing key yields an empty Vec.
    pub color_list: Vec<[u8; 3]>,
    /// Rate of color interpolation across the ColorList.
    /// Stored as the exact widened `ReadDouble` result because Spark consumes
    /// the native double bits directly.
    pub color_speed: NativeF64Bits,
    /// Spark: starting color 1 (RGB).
    pub start_color_1: [u8; 3],
    /// Spark: starting color 2 (RGB).
    pub start_color_2: [u8; 3],

    // ── Spark-only (parsed but unused at Tier 2) ─────────────────────
    /// Spark: max random X velocity.
    pub x_velocity: i32,
    /// Spark: max random Y velocity.
    pub y_velocity: i32,
    /// Spark: minimum upward Z velocity.
    pub min_z_velocity: i32,
    /// Spark: random range added to MinZVelocity.
    pub z_velocity_range: i32,
}

/// Parse-state wrapper around a `ParticleType` whose `next_particle` reference
/// has not yet been resolved.
///
/// `RuleSet::from_ini` collects these in pass 1, then resolves
/// `next_particle_name` against the name → ID map in pass 2. The resulting
/// `ParticleType` is stored in the registry.
pub struct PendingParticleType {
    /// The parsed type with `next_particle = None`.
    pub partial: ParticleType,
    /// Captured `NextParticle=` value to resolve in pass 2.
    pub next_particle_name: Option<String>,
}

impl ParticleType {
    /// Parse a ParticleType from an INI section.
    ///
    /// `next_particle` is always `None` — the resolver in `RuleSet::from_ini`
    /// fills it in after all particle types have been collected.
    /// Use `from_ini_section_pending` if you need the unresolved reference name.
    #[cfg(test)]
    pub fn from_ini_section(name: &str, section: &IniSection) -> Self {
        Self::from_ini_section_pending(name, section).partial
    }

    /// Parse a ParticleType plus its unresolved `NextParticle=` name string.
    pub fn from_ini_section_pending(name: &str, section: &IniSection) -> PendingParticleType {
        let behaves_like = section
            .read_name("BehavesLike", 0x20)
            .and_then(ParticleBehavesLike::parse)
            // Binary's string-table loop falls through to index 0 (Gas) when
            // the INI string doesn't match any entry.
            .unwrap_or(ParticleBehavesLike::Gas);

        let end_state_ai = section.read_int("EndStateAI", 0) as u8;
        let final_damage_state =
            section.read_int("FinalDamageState", i32::from(end_state_ai)) as u8;

        let next_particle_name = section
            .read_name("NextParticle", 0x20)
            .filter(|s| !s.eq_ignore_ascii_case("none"))
            .map(str::to_owned);

        let partial = Self {
            name: name.to_string(),
            behaves_like,
            image: section.read_name("Image", 0x19).map(str::to_owned),

            // ParticleType644BE0/644F50 retain full signed words; the
            // instance constructor62B890 alone narrows RemainingEC to16bits.
            max_dc: section.read_int("MaxDC", 0),
            max_ec: section.read_int("MaxEC", 1),
            damage: section.read_int("Damage", 0),
            warhead: section.read_name("Warhead", 0x80).map(str::to_owned),
            #[cfg(test)]
            num_loop_frames: section
                .read_int("NumLoopFrames", 1)
                .clamp(0, u16::MAX as i32) as u16,
            translucency: section.read_int("Translucency", 0) as u8,
            wind_effect: section.read_int("WindEffect", 0),
            // Float fields (`FSTP dword` at `0x006450E9`, `0x00645108`).
            velocity: section.read_double_to_float("Velocity", NativeF32Bits::POSITIVE_ZERO),
            deacc: section.read_double_to_float("Deacc", NativeF32Bits::POSITIVE_ZERO),
            radius: section.read_int("Radius", 0),
            delete_on_state_limit: section.read_bool("DeleteOnStateLimit", false),
            end_state_ai,
            start_state_ai: section.read_int("StartStateAI", 0) as u8,
            state_ai_advance: section.read_int("StateAIAdvance", 4) as u8,
            final_damage_state,
            translucent_25_state: section.read_int("Translucent25State", 0xFF) as u8,
            translucent_50_state: section.read_int("Translucent50State", 0xFF) as u8,
            normalized: section.read_bool("Normalized", false),
            next_particle: None,
            next_particle_offset: IVec3::from_array(
                section.read_coord_tokens("NextParticleOffset", [0; 3]),
            ),

            color_list: section.read_color_list("ColorList").unwrap_or_default(),
            color_speed: NativeF64Bits::from_bits(section.read_double("ColorSpeed", 0.0).to_bits()),
            start_color_1: section.read_color_rgb("StartColor1", [0, 0, 0], None),
            start_color_2: section.read_color_rgb("StartColor2", [0, 0, 0], None),

            // ParticleTypeCtor644BE0 initializes these divisors to one;
            // ReadINI64521A/645234/645268 passes each current value as default.
            // Original constructor/read controls: procedural_drawing_oracle/welding.
            x_velocity: section.read_int("XVelocity", 1),
            y_velocity: section.read_int("YVelocity", 1),
            min_z_velocity: section.read_int("MinZVelocity", 0),
            z_velocity_range: section.read_int("ZVelocityRange", 1),
        };

        PendingParticleType {
            partial,
            next_particle_name,
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
            ParticleBehavesLike::parse("Gas"),
            Some(ParticleBehavesLike::Gas)
        );
        assert_eq!(
            ParticleBehavesLike::parse("Smoke"),
            Some(ParticleBehavesLike::Smoke)
        );
        assert_eq!(
            ParticleBehavesLike::parse("Fire"),
            Some(ParticleBehavesLike::Fire)
        );
        assert_eq!(
            ParticleBehavesLike::parse("Spark"),
            Some(ParticleBehavesLike::Spark)
        );
        assert_eq!(
            ParticleBehavesLike::parse("Railgun"),
            Some(ParticleBehavesLike::Railgun)
        );
        assert_eq!(ParticleBehavesLike::parse("nope"), None);
    }

    #[test]
    fn behaves_like_parse_trims_whitespace() {
        assert_eq!(
            ParticleBehavesLike::parse("  Gas  "),
            Some(ParticleBehavesLike::Gas)
        );
    }

    #[test]
    fn behaves_like_discriminants_match_binary() {
        // Binary indexes string-table by enum value; preserve the exact ordering.
        assert_eq!(ParticleBehavesLike::Gas as u8, 0);
        assert_eq!(ParticleBehavesLike::Smoke as u8, 1);
        assert_eq!(ParticleBehavesLike::Fire as u8, 2);
        assert_eq!(ParticleBehavesLike::Spark as u8, 3);
        assert_eq!(ParticleBehavesLike::Railgun as u8, 4);
    }

    #[test]
    fn particle_type_id_is_copy_eq_hash() {
        let a = ParticleTypeId(7);
        let b = a;
        assert_eq!(a, b);
        let mut set = std::collections::HashSet::new();
        set.insert(a);
        assert!(set.contains(&b));
    }

    #[test]
    fn from_ini_uses_documented_defaults() {
        let ini = IniFile::from_str("[Foo]\nFixtureOnly=1\n");
        let section = ini.section("Foo").unwrap();
        let pt = ParticleType::from_ini_section("Foo", section);
        assert_eq!(pt.state_ai_advance, 4);
        assert_eq!(pt.translucent_25_state, 0xFF);
        assert_eq!(pt.translucent_50_state, 0xFF);
        assert_eq!(pt.color_list, Vec::<[u8; 3]>::new());
        // BehavesLike defaults to Gas (binary's string-table fallthrough).
        assert_eq!(pt.behaves_like, ParticleBehavesLike::Gas);
        // FinalDamageState mirrors EndStateAI when both absent — both 0 here.
        assert_eq!(pt.final_damage_state, pt.end_state_ai);
        assert_eq!(pt.next_particle, None);
        assert_eq!(pt.next_particle_offset, IVec3::ZERO);
        assert_eq!(pt.velocity, NativeF32Bits::POSITIVE_ZERO);
        assert_eq!(pt.max_ec, 1);
        assert_eq!(pt.num_loop_frames, 1);
        assert_eq!(pt.color_speed, NativeF64Bits::POSITIVE_ZERO);
    }

    #[test]
    fn from_ini_parses_gas_cloud_like_section() {
        // Mirrors GasCloudM1 from rulesmd.ini, with NextParticle deferred to A4.
        let ini = IniFile::from_str(
            "[GasCloudM1]\n\
             BehavesLike=Gas\n\
             Image=GASLRGMK\n\
             MaxDC=60\n\
             MaxEC=448\n\
             Damage=0\n\
             Warhead=Gas\n\
             EndStateAI=11\n\
             StateAIAdvance=3\n\
             Translucency=50\n\
             DeleteOnStateLimit=yes\n\
             NextParticleOffset=0,0,150\n",
        );
        let section = ini.section("GasCloudM1").unwrap();
        let pt = ParticleType::from_ini_section("GasCloudM1", section);

        assert_eq!(pt.name, "GasCloudM1");
        assert_eq!(pt.behaves_like, ParticleBehavesLike::Gas);
        assert_eq!(pt.image.as_deref(), Some("GASLRGMK"));
        assert_eq!(pt.max_dc, 60);
        assert_eq!(pt.max_ec, 448);
        assert_eq!(pt.damage, 0);
        assert_eq!(pt.warhead.as_deref(), Some("Gas"));
        assert_eq!(pt.end_state_ai, 11);
        assert_eq!(pt.state_ai_advance, 3);
        assert_eq!(pt.translucency, 50);
        assert!(pt.delete_on_state_limit);
        assert_eq!(pt.next_particle_offset, IVec3::new(0, 0, 150));
        // FinalDamageState absent → mirrors EndStateAI.
        assert_eq!(pt.final_damage_state, 11);
    }

    #[test]
    fn from_ini_final_damage_state_overrides_default_when_present() {
        let ini = IniFile::from_str(
            "[Foo]\n\
             EndStateAI=10\n\
             FinalDamageState=5\n",
        );
        let section = ini.section("Foo").unwrap();
        let pt = ParticleType::from_ini_section("Foo", section);
        assert_eq!(pt.end_state_ai, 10);
        assert_eq!(pt.final_damage_state, 5);
    }

    #[test]
    fn from_ini_parses_color_list_and_start_colors() {
        let ini = IniFile::from_str(
            "[Spark]\n\
             BehavesLike=Spark\n\
             ColorList=(255,255,255),(200,200,80)\n\
             ColorSpeed=.13\n\
             StartColor1=255,128,0\n\
             StartColor2=128,64,32\n",
        );
        let section = ini.section("Spark").unwrap();
        let pt = ParticleType::from_ini_section("Spark", section);
        assert_eq!(pt.color_list, vec![[255, 255, 255], [200, 200, 80]]);
        assert_eq!(pt.color_speed.bits(), 0x3fc0_a3d7_0000_0000);
        assert_eq!(pt.start_color_1, [255, 128, 0]);
        assert_eq!(pt.start_color_2, [128, 64, 32]);
    }

    #[test]
    fn from_ini_unknown_behaves_like_falls_through_to_gas() {
        let ini = IniFile::from_str("[Foo]\nBehavesLike=Bogus\n");
        let section = ini.section("Foo").unwrap();
        let pt = ParticleType::from_ini_section("Foo", section);
        assert_eq!(pt.behaves_like, ParticleBehavesLike::Gas);
    }
}
