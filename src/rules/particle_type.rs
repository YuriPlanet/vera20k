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
#[repr(i32)]
pub enum ParticleBehavesLike {
    Gas = 0,
    Smoke = 1,
    Fire = 2,
    Spark = 3,
    Railgun = 4,
    /// Constructor and unrecognized/empty ReadINI result.
    Unknown = -1,
}

impl ParticleBehavesLike {
    /// Parse a `BehavesLike=` value from INI. Returns `None` for unknown strings.
    pub fn parse(s: &str) -> Option<Self> {
        [
            Self::Gas,
            Self::Smoke,
            Self::Fire,
            Self::Spark,
            Self::Railgun,
        ]
        .into_iter()
        .zip(["Gas", "Smoke", "Fire", "Spark", "Railgun"])
        .find_map(|(behavior, name)| s.eq_ignore_ascii_case(name).then_some(behavior))
    }
}

/// A particle definition parsed from a `[ParticleName]` section in rulesmd.ini.
///
/// The binary reads every key for every `BehavesLike`; the Spark and Railgun
/// fields (`XVelocity`, `ColorList`, etc.) feed those behaviors' particle AI.
#[derive(Debug, Clone)]
pub struct ParticleType {
    // ── Identity ─────────────────────────────────────────────────────
    /// Section name in rulesmd.ini (e.g., "GasCloud1", "Smoke").
    pub name: String,
    /// Per-particle behavior dispatch — selects the AI branch each tick.
    pub behaves_like: ParticleBehavesLike,

    // ── Object-base (from ObjectTypeClass) ───────────────────────────
    /// ObjectType+0x1F8 Image buffer. `ObjectTypeClass::Constructor @
    /// 0x005F7090` starts with the AbstractType ID; `ParticleTypeClass::ReadINI
    /// @ 0x00644F50` reaches `ObjectTypeClass::ReadINI @ 0x005F92D0`, which
    /// retains the current 25-byte string when Image is missing or empty.
    pub image: String,
    /// Inherited `ObjectTypeClass::ReadINI @ 0x005F92D0` AlphaImage buffer
    /// (`+0x213`, `char[25]`), separate from the particle's main Image.
    pub(crate) alpha_image: String,
    /// Native `StartFrame=` (`ParticleTypeClass+0x2EC`): the constructor at
    /// `ParticleTypeClass::Constructor @ 0x00644BE0` starts it at zero;
    /// `ParticleTypeClass::ReadINI @ 0x00644F50` passes its current value
    /// (`0x00645067`) to `CCINIClass::ReadInt @ 0x005276D0` (`0x0064507C`);
    /// `ParticleTypeClass::ComputeCRC @ 0x006454E0` reads it at `0x00645518`.
    pub start_frame: i32,

    // ── [Particles] direct keys (§2.2) ───────────────────────────────
    /// Damage countdown reset value: frames between damage ticks (Gas/Fire).
    pub max_dc: i32,
    /// Lifetime in frames (countdown decremented each tick).
    pub max_ec: i32,
    /// Damage amount per damage tick.
    pub damage: i32,
    /// `Warhead=`, the stored ID the ordered Rules processor's WarheadType
    /// factory answered; resolved later against the warhead registry.
    pub warhead: Option<String>,
    /// Native `NumLoopFrames=` (`ParticleTypeClass+0x2F0`): the constructor at
    /// `ParticleTypeClass::Constructor @ 0x00644BE0` starts it at one;
    /// `ParticleTypeClass::ReadINI @ 0x00644F50` passes its current value
    /// (`0x00645087`) to `CCINIClass::ReadInt @ 0x005276D0` (`0x00645096`).
    /// Signed values are retained without a clamp; `ParticleTypeClass::
    /// ComputeCRC @ 0x006454E0` reads it at `0x00645526`.
    pub num_loop_frames: i32,
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
    /// Retained ParticleType+310 reference from the ordered native factory.
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

/// Fields whose reader resets on each admitted ParticleType ReadINI body,
/// unlike ordinary current-value scalar reads. One state belongs to each
/// allocated type, including types allocated without a section.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct ParticleReadState {
    behaves_like: ParticleBehavesLike,
    end_state_ai: u8,
    final_damage_state: u8,
    color_list: Vec<[u8; 3]>,
}

impl Default for ParticleReadState {
    fn default() -> Self {
        // ParticleTypeClass::Constructor @ 0x00644BE0: +309/+30C zero,
        // ColorList empty, and +314=-1 (0x00644CEE).
        Self {
            behaves_like: ParticleBehavesLike::Unknown,
            end_state_ai: 0,
            final_damage_state: 0,
            color_list: Vec::new(),
        }
    }
}

impl ParticleReadState {
    pub(super) fn apply_pass(&mut self, section: &IniSection) {
        // ParticleTypeClass::ReadINI @ 0x00644F50: fresh ColorList temporary
        // 644F7A..644FE1; signed EndStateAI default at6452DC; strcmpi table
        // 6453BA..6453FF stores -1 for absent/unrecognized BehavesLike.
        self.color_list = section.read_color_list("ColorList").unwrap_or_default();
        self.end_state_ai =
            section.read_int("EndStateAI", i32::from(self.end_state_ai as i8)) as u8;
        self.final_damage_state =
            section.read_int("FinalDamageState", i32::from(self.end_state_ai as i8)) as u8;
        self.behaves_like = section
            .read_name("BehavesLike", 0x20)
            .and_then(ParticleBehavesLike::parse)
            .unwrap_or(ParticleBehavesLike::Unknown);
    }
}

impl ParticleType {
    #[cfg(test)]
    pub fn from_ini_section(name: &str, section: &IniSection) -> Self {
        let mut read = ParticleReadState::default();
        read.apply_pass(section);
        Self::from_read_state(name, section, &read, None, None)
    }

    pub(super) fn from_read_state(
        name: &str,
        section: &IniSection,
        read: &ParticleReadState,
        next_particle: Option<ParticleTypeId>,
        warhead: Option<String>,
    ) -> Self {
        Self {
            name: name.to_string(),
            behaves_like: read.behaves_like,
            image: section.read_name("Image", 0x19).unwrap_or(name).to_owned(),
            alpha_image: section.read_object_alpha_image(""),
            start_frame: section.read_int("StartFrame", 0),

            // ParticleType644BE0/644F50 retain full signed words; the
            // instance constructor62B890 alone narrows RemainingEC to16bits.
            max_dc: section.read_int("MaxDC", 0),
            max_ec: section.read_int("MaxEC", 1),
            damage: section.read_int("Damage", 0),
            warhead,
            num_loop_frames: section.read_int("NumLoopFrames", 1),
            translucency: section.read_int("Translucency", 0) as u8,
            wind_effect: section.read_int("WindEffect", 0),
            // Float fields (`FSTP dword` at `0x006450E9`, `0x00645108`).
            velocity: section.read_double_to_float("Velocity", NativeF32Bits::POSITIVE_ZERO),
            deacc: section.read_double_to_float("Deacc", NativeF32Bits::POSITIVE_ZERO),
            radius: section.read_int("Radius", 0),
            delete_on_state_limit: section.read_bool("DeleteOnStateLimit", false),
            end_state_ai: read.end_state_ai,
            start_state_ai: section.read_int("StartStateAI", 0) as u8,
            state_ai_advance: section.read_int("StateAIAdvance", 4) as u8,
            final_damage_state: read.final_damage_state,
            translucent_25_state: section.read_int("Translucent25State", 0xFF) as u8,
            translucent_50_state: section.read_int("Translucent50State", 0xFF) as u8,
            normalized: section.read_bool("Normalized", false),
            next_particle,
            next_particle_offset: IVec3::from_array(
                section.read_coord_tokens("NextParticleOffset", [0; 3]),
            ),

            color_list: read.color_list.clone(),
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
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rules::ini_parser::IniFile;

    #[test]
    fn particle_behaviors_reset_per_reached_layer() {
        use crate::rules::native_processing::{RulesLayerKind, RulesLayerStack};
        use crate::rules::particle_system_type::ParticleSystemBehavesLike;
        use crate::rules::ruleset::RuleSet;

        let mut layers = RulesLayerStack::new(IniFile::from_str(
            "[Particles]\n0=P\n[ParticleSystems]\n0=S\n\
             [P]\nBehavesLike=sMoKe\nMaxEC=8\nEndStateAI=7\nFinalDamageState=9\n\
             ColorList=(255,0,0),(0,255,0)\nXVelocity=3\n\
             [S]\nBehavesLike=sPaRk\nHoldsWhat=P\nSpawnSparkPercentage=.2\n",
        ));
        let first = RuleSet::from_rules_layers(&layers).unwrap();
        let p = first.particle_type(first.p_type_id_by_name("P").unwrap());
        let s = first.particle_system_type(first.ps_type_id_by_name("S").unwrap());
        assert_eq!(p.behaves_like, ParticleBehavesLike::Smoke);
        assert_eq!(s.behaves_like, ParticleSystemBehavesLike::Spark);
        assert_eq!(p.color_list.len(), 2);
        assert_eq!(p.final_damage_state, 9);

        layers.push(
            RulesLayerKind::GameMode,
            IniFile::from_str("[P]\nMaxEC=9\nbehaveslike=Fire\n[S]\nLifetime=5\n"),
        );
        let second = RuleSet::from_rules_layers(&layers).unwrap();
        let p = second.particle_type(second.p_type_id_by_name("P").unwrap());
        let s2 = second.particle_system_type(second.ps_type_id_by_name("S").unwrap());
        assert_eq!(p.behaves_like as i32, -1);
        assert_eq!(s2.behaves_like as i32, -1);
        assert!(p.color_list.is_empty());
        assert_eq!(p.final_damage_state, 7);
        assert_eq!(p.x_velocity, 3);
        assert_eq!(s2.spawn_spark_percentage, s.spawn_spark_percentage);
    }

    #[test]
    fn particle_factory_bindings_preserve_allocation_order_and_saved_reads() {
        use crate::rules::native_processing::{RulesLayerKind, RulesLayerStack};
        use crate::rules::ruleset::RuleSet;
        let mut layers = RulesLayerStack::new(IniFile::from_str(
            "[Particles]\n0=P\n[ParticleSystems]\n0=S\n\
             [P]\nBehavesLike=Smoke\nNextParticle=none\n\
             [none]\nBehavesLike=Fire\nMaxEC=6\n\
             [S]\nBehavesLike=Smoke\nHoldsWhat=Late\n\
             [Late]\nBehavesLike=Spark\nMaxEC=200\n",
        ));
        let first = RuleSet::from_rules_layers(&layers).unwrap();
        let p_id = first.p_type_id_by_name("P").unwrap();
        let next = first.particle_type(p_id).next_particle.unwrap();
        assert_eq!(first.particle_type(next).name, "none");
        assert_eq!(
            first.particle_type(next).behaves_like,
            ParticleBehavesLike::Fire
        );
        let late = first
            .particle_system_type(first.ps_type_id_by_name("S").unwrap())
            .holds_what
            .unwrap();
        assert_eq!(
            first.particle_type(late).behaves_like,
            ParticleBehavesLike::Unknown
        );
        assert_eq!(first.particle_type(late).max_ec, 1);

        layers.push(
            RulesLayerKind::Scenario,
            IniFile::from_str(
                "[P]\nNextParticle=\n[S]\nBehavesLike=Smoke\n[Late]\nBehavesLike=Railgun\n",
            ),
        );
        let processed = layers.process().unwrap();
        let second = RuleSet::from_processed_rules(&processed).unwrap();
        assert_eq!(second.particle_type(p_id).next_particle, Some(next));
        assert_eq!(
            second.particle_type(late).behaves_like,
            ParticleBehavesLike::Railgun
        );
        let empty = second
            .particle_system_type(second.ps_type_id_by_name("S").unwrap())
            .holds_what
            .unwrap();
        assert_eq!(second.particle_type(empty).name, "");
        assert_eq!(second.particle_type(empty).max_ec, 1);
        assert_eq!(
            second.particle_type(empty).behaves_like,
            ParticleBehavesLike::Unknown
        );
        let restored = serde_json::from_slice(&serde_json::to_vec(&processed).unwrap()).unwrap();
        assert_eq!(
            second.simulation_config_hash(),
            RuleSet::from_processed_rules(&restored)
                .unwrap()
                .simulation_config_hash()
        );

        layers.push(
            RulesLayerKind::Scenario,
            IniFile::from_str("[P]\nNextParticle=<none>\n"),
        );
        let cleared = RuleSet::from_rules_layers(&layers).unwrap();
        assert_eq!(cleared.particle_type(p_id).next_particle, None);
        assert_ne!(
            cleared.simulation_config_hash(),
            second.simulation_config_hash()
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
    fn from_ini_uses_documented_defaults() {
        let ini = IniFile::from_str("[Foo]\nFixtureOnly=1\n");
        let section = ini.section("Foo").unwrap();
        let pt = ParticleType::from_ini_section("Foo", section);
        assert_eq!(pt.state_ai_advance, 4);
        assert_eq!(pt.translucent_25_state, 0xFF);
        assert_eq!(pt.translucent_50_state, 0xFF);
        assert_eq!(pt.color_list, Vec::<[u8; 3]>::new());
        // The native constructor and unknown reader result are -1.
        assert_eq!(pt.behaves_like, ParticleBehavesLike::Unknown);
        // FinalDamageState mirrors EndStateAI when both absent — both 0 here.
        assert_eq!(pt.final_damage_state, pt.end_state_ai);
        assert_eq!(pt.next_particle, None);
        assert_eq!(pt.next_particle_offset, IVec3::ZERO);
        assert_eq!(pt.velocity, NativeF32Bits::POSITIVE_ZERO);
        assert_eq!(pt.max_ec, 1);
        assert_eq!(pt.start_frame, 0);
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
        assert_eq!(pt.image, "GASLRGMK");
        assert_eq!(pt.max_dc, 60);
        assert_eq!(pt.max_ec, 448);
        assert_eq!(pt.damage, 0);
        assert_eq!(pt.end_state_ai, 11);
        assert_eq!(pt.state_ai_advance, 3);
        assert_eq!(pt.translucency, 50);
        assert!(pt.delete_on_state_limit);
        assert_eq!(pt.next_particle_offset, IVec3::new(0, 0, 150));
        // FinalDamageState absent → mirrors EndStateAI.
        assert_eq!(pt.final_damage_state, 11);
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
    fn from_ini_unknown_behaves_like_is_minus_one() {
        let ini = IniFile::from_str("[Foo]\nBehavesLike=Bogus\n");
        let section = ini.section("Foo").unwrap();
        let pt = ParticleType::from_ini_section("Foo", section);
        assert_eq!(pt.behaves_like, ParticleBehavesLike::Unknown);
    }
}
