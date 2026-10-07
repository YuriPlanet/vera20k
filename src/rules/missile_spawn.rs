//! `RulesClass` rocket blocks — the three hardcoded rocket families the spawn
//! manager and `RocketLocomotionClass` treat as missiles.
//!
//! `RulesClass` holds three `RocketStruct` blocks of 0x34 bytes: V3Rocket at
//! `+0x4B0`, DMisl at `+0x4E4` and CMisl at `+0x518`, each ending in the
//! resolved `[General] V3RocketType=` / `DMislType=` / `CMislType=` pointer
//! (`+0x4E0`, `+0x514`, `+0x548`). Two different tests read them:
//!
//! - `SpawnManagerClass` sets a slot's `IsSpawnedMissile` when the pool's
//!   `Spawns=` type is any of the three (`0x006B7920..0x006B7947`), and its
//!   post-launch timer adds the V3 PauseFrames + TiltFrames for the V3 type
//!   and the **DMisl** pair for every other type (`0x006B7A90..0x006B7B04`).
//! - `RocketLocomotionClass` (`Move_To 0x006632E0`, `Process 0x006622C0`,
//!   `Detonate 0x00663030`) selects the V3 block for the V3 type, the CMisl
//!   block for the CMisl type and the **DMisl** block for anything else.
//!
//! The child's own `MissileSpawn=` key is a separate test the manager also
//! makes, on a different decision (see `sim::spawn_manager`).
//!
//! Sources: `RulesClass__ReadGeneral @ 0x0066D530` (`0x00671212..0x00671723`),
//! `RulesClass__ReadCombatDamage` warhead reads (`0x0066C3A4..0x0066C4D5`) and
//! the constructor seeds `0x006678D3..0x006679F1`.
//!
//! ## Dependency rules
//! - Part of rules/ — no dependencies on sim/, render/, ui/, etc.

use crate::rules::ini_parser::{IniFile, IniSection};
use crate::util::native_x87::{NativeF32Bits, NativeF64Bits, X87Chop53};

/// `RulesClass::Constructor @ 0x00665650` seeds (`0x006678D3..0x006679F1`),
/// with `EBX` = 0 (`0x00665663`), `EDI` = 10 (`0x006656D9`), `EAX` = 1
/// (`0x00667202`) and `EDX` = binary32 1.0 (`0x006678BC`). The three type
/// slots and six warhead slots start null, held here as an empty name that
/// matches no type.
mod ctor {
    use crate::util::native_x87::NativeF32Bits;

    pub const ZERO: NativeF32Bits = NativeF32Bits::POSITIVE_ZERO;
    pub const ONE: NativeF32Bits = NativeF32Bits::ONE;
    /// `0x3D4CCCCD`, binary32 0.05.
    pub const TURN_RATE: NativeF32Bits = NativeF32Bits::from_bits(0x3D4C_CCCD);
    /// `0x3ECCCCCD`, binary32 0.4.
    pub const ACCELERATION: NativeF32Bits = NativeF32Bits::from_bits(0x3ECC_CCCD);
    /// `0x3F19999A`, binary32 0.6: CMisl's own acceleration seed.
    pub const CMISL_ACCELERATION: NativeF32Bits = NativeF32Bits::from_bits(0x3F19_999A);
    pub const ALTITUDE: i32 = 0x300;
    pub const RAISE_RATE: i32 = 1;
}

/// Which of the three hardcoded rocket families a type belongs to.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, serde::Serialize, serde::Deserialize,
)]
pub enum MissileFamily {
    /// `[General] V3RocketType=` — V3 Launcher.
    V3Rocket,
    /// `[General] DMislType=` — Dreadnought.
    DMisl,
    /// `[General] CMislType=` — Boomer.
    CMisl,
}

/// One `RocketStruct` and the two `[CombatDamage]` warheads its family
/// detonates with.
#[derive(Debug, Clone)]
pub struct MissileSpawnParams {
    /// Resolved child TechnoType section name (`V3ROCKET` / `DMISL` / `CMISL`),
    /// `+0x30`; empty while the key is unset (a null pointer).
    pub type_name: String,
    /// `+0x00` frames in MissionState 1 before the tilt.
    pub pause_frames: i32,
    /// `+0x04` frames of the tilt (state 2) or the CMisl raise (state 6).
    pub tilt_frames: i32,
    /// `+0x08` launch pitch, in quarter turns (Move_To multiplies by pi/2).
    pub pitch_initial: NativeF32Bits,
    /// `+0x0C` pitch after the tilt, in quarter turns.
    pub pitch_final: NativeF32Bits,
    /// `+0x10` per-frame pitch step in radians.
    pub turn_rate: NativeF32Bits,
    /// `+0x14` leptons the CMisl raise adds to Z per frame.
    pub raise_rate: i32,
    /// `+0x18` per-frame speed gain (leptons per frame per frame).
    pub acceleration: NativeF32Bits,
    /// `+0x1C` height above ground (leptons) that ends the climb.
    pub altitude: i32,
    /// `+0x20` impact damage.
    pub damage: i32,
    /// `+0x24` impact damage when the launcher was elite.
    pub elite_damage: i32,
    /// `+0x28` leptons from the body's coordinate to its nose.
    pub body_length: i32,
    /// `+0x2C` blend the cruise pitch toward the target angle.
    pub lazy_curve: bool,
    /// `[CombatDamage] *Warhead`.
    pub warhead: String,
    /// `[CombatDamage] *EliteWarhead`.
    pub elite_warhead: String,
}

impl MissileSpawnParams {
    /// `Detonate` (`0x0066320C..0x00663215`) picks EliteDamage by the
    /// rocket's SpawnerIsElite latch, Damage otherwise.
    pub fn damage_for(&self, elite: bool) -> i32 {
        if elite {
            self.elite_damage
        } else {
            self.damage
        }
    }

    /// `Detonate`'s warhead by the same latch (`0x00663064..0x006630D9`).
    pub fn warhead_for(&self, elite: bool) -> &str {
        if elite {
            &self.elite_warhead
        } else {
            &self.warhead
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn seeded(
        pause_frames: i32,
        tilt_frames: i32,
        pitch_initial: NativeF32Bits,
        turn_rate: NativeF32Bits,
        acceleration: NativeF32Bits,
        damage: i32,
        body_length: i32,
        lazy_curve: bool,
    ) -> Self {
        Self {
            type_name: String::new(),
            pause_frames,
            tilt_frames,
            pitch_initial,
            pitch_final: ctor::ONE,
            turn_rate,
            raise_rate: ctor::RAISE_RATE,
            acceleration,
            altitude: ctor::ALTITUDE,
            damage,
            elite_damage: damage,
            body_length,
            lazy_curve,
            warhead: String::new(),
            elite_warhead: String::new(),
        }
    }

    /// One family's `[General]` reads, in `ReadGeneral`'s order. Each reader
    /// gets the live field as its default. `acceleration_default` is the
    /// value the Acceleration read falls back to: the block's own field, except
    /// for CMisl, whose read passes DMisl's (`0x0067164D`).
    fn read_general(
        &mut self,
        general: &IniSection,
        prefix: &str,
        acceleration_default: NativeF32Bits,
    ) {
        let key = |suffix: &str| format!("{prefix}{suffix}");
        self.pause_frames = general.read_int(&key("PauseFrames"), self.pause_frames);
        self.tilt_frames = general.read_int(&key("TiltFrames"), self.tilt_frames);
        self.pitch_initial = general.read_double_to_float(&key("PitchInitial"), self.pitch_initial);
        self.pitch_final = general.read_double_to_float(&key("PitchFinal"), self.pitch_final);
        self.turn_rate = general.read_double_to_float(&key("TurnRate"), self.turn_rate);
        // An int field read through ReadDouble: FILD the field as the
        // default, then Math::ftol the result (`0x006712AB..0x006712CE`).
        self.raise_rate =
            general.read_double_with(&key("RaiseRate"), self.raise_rate, |_, value| {
                // A non-finite scan stores FISTP's indefinite integer, whose low
                // dword is zero.
                X87Chop53::load_f64(NativeF64Bits::from_bits(value.to_bits()))
                    .map(X87Chop53::ftol_i32_low_masked)
                    .unwrap_or(0)
            });
        self.acceleration =
            general.read_double_to_float(&key("Acceleration"), acceleration_default);
        self.altitude = general.read_int(&key("Altitude"), self.altitude);
        self.damage = general.read_int(&key("Damage"), self.damage);
        self.elite_damage = general.read_int(&key("EliteDamage"), self.elite_damage);
        self.body_length = general.read_int(&key("BodyLength"), self.body_length);
        self.lazy_curve = general.read_bool(&key("LazyCurve"), self.lazy_curve);
        // ReadString into 0x80 bytes ahead of the type lookup (`0x0067BD30`),
        // which an absent key skips.
        if let Some(name) = general.read_name(&key("Type"), 0x80) {
            self.type_name = name.to_string();
        }
    }
}

/// The three `RocketStruct` blocks.
#[derive(Debug, Clone)]
pub struct MissileSpawnRules {
    pub v3: MissileSpawnParams,
    pub dmisl: MissileSpawnParams,
    pub cmisl: MissileSpawnParams,
}

impl Default for MissileSpawnRules {
    fn default() -> Self {
        use ctor::*;
        Self {
            v3: MissileSpawnParams::seeded(0, 60, ZERO, TURN_RATE, ACCELERATION, 1000, 0x100, true),
            dmisl: MissileSpawnParams::seeded(
                10,
                60,
                ZERO,
                TURN_RATE,
                ACCELERATION,
                1000,
                0x80,
                false,
            ),
            cmisl: MissileSpawnParams::seeded(
                10,
                0x1E,
                ONE,
                ONE,
                CMISL_ACCELERATION,
                500,
                0x80,
                false,
            ),
        }
    }
}

impl MissileSpawnRules {
    /// One `RulesClass::Process` pass: `ReadGeneral` returns at once when the
    /// pass has no `[General]` (`0x0066D54C..0x0066D558`), and
    /// `ReadCombatDamage` likewise for `[CombatDamage]`. Within a present
    /// `[General]` every key keeps the live value when absent, except
    /// `CMislAcceleration`, whose default is DMisl's live acceleration: a pass
    /// with `[General]` but no `CMislAcceleration=` copies DMisl's.
    pub fn apply_pass(&mut self, ini: &IniFile) {
        if let Some(general) = ini.section("General") {
            self.read_general(general);
        }
        if let Some(combat_damage) = ini.section("CombatDamage") {
            self.read_combat_damage(combat_damage);
        }
    }

    /// The single-file read: one [`Self::apply_pass`] over a projected or raw
    /// INI. Production replaces it with the per-pass result.
    pub fn from_ini(ini: &IniFile) -> Self {
        let mut rules = Self::default();
        rules.apply_pass(ini);
        rules
    }

    fn read_general(&mut self, general: &IniSection) {
        let v3_acceleration = self.v3.acceleration;
        self.v3.read_general(general, "V3Rocket", v3_acceleration);
        let dmisl_acceleration = self.dmisl.acceleration;
        self.dmisl
            .read_general(general, "DMisl", dmisl_acceleration);
        let cmisl_acceleration_default = self.dmisl.acceleration;
        self.cmisl
            .read_general(general, "CMisl", cmisl_acceleration_default);
    }

    /// `ReadCombatDamage`'s six warhead reads (`0x0066C390..0x0066C4F4`), in
    /// order. Each is ReadString into 0x80 bytes, then the warhead lookup
    /// (`0x0075E3B0`) when the copy is nonempty; otherwise the default. A
    /// normal warhead's default is its own live value, but an elite warhead's
    /// is its family's normal warhead as this pass left it: a pass whose
    /// `[CombatDamage]` names `V3Warhead=` without `V3EliteWarhead=` gives the
    /// elite launcher the normal warhead.
    fn read_combat_damage(&mut self, combat_damage: &IniSection) {
        let read = |key: &str, default: &str| {
            combat_damage
                .read_name(key, 0x80)
                .map_or_else(|| default.to_string(), str::to_string)
        };
        self.v3.warhead = read("V3Warhead", &self.v3.warhead);
        self.dmisl.warhead = read("DMislWarhead", &self.dmisl.warhead);
        self.v3.elite_warhead = read("V3EliteWarhead", &self.v3.warhead);
        self.dmisl.elite_warhead = read("DMislEliteWarhead", &self.dmisl.warhead);
        self.cmisl.warhead = read("CMislWarhead", &self.cmisl.warhead);
        self.cmisl.elite_warhead = read("CMislEliteWarhead", &self.cmisl.warhead);
    }

    /// The spawn manager's missile-slot test: the type is one of the three
    /// (`0x006B7920..0x006B7947`).
    pub fn family_of(&self, type_name: &str) -> Option<MissileFamily> {
        if type_name.eq_ignore_ascii_case(&self.v3.type_name) {
            Some(MissileFamily::V3Rocket)
        } else if type_name.eq_ignore_ascii_case(&self.dmisl.type_name) {
            Some(MissileFamily::DMisl)
        } else if type_name.eq_ignore_ascii_case(&self.cmisl.type_name) {
            Some(MissileFamily::CMisl)
        } else {
            None
        }
    }

    /// The block `RocketLocomotionClass` reads for an owner of this type:
    /// V3 for the V3 type, CMisl for the CMisl type, DMisl for any other
    /// (`0x006622D7..0x0066230E`, `0x0066304F..0x006630B6`).
    pub fn rocket_family(&self, type_name: &str) -> MissileFamily {
        if type_name.eq_ignore_ascii_case(&self.v3.type_name) {
            MissileFamily::V3Rocket
        } else if type_name.eq_ignore_ascii_case(&self.cmisl.type_name) {
            MissileFamily::CMisl
        } else {
            MissileFamily::DMisl
        }
    }

    /// The block `RocketLocomotionClass` reads for an owner of this type
    /// ([`Self::rocket_family`]), and whether that block's type is the CMisl
    /// type. Process's Boomer arms compare those two type pointers
    /// (`0x0066235A`, `0x00662493`) rather than the selection, so a type that
    /// also names the V3 or DMisl block takes them too; two unset types are
    /// both null and compare equal.
    pub fn rocket_block(&self, type_name: &str) -> (&MissileSpawnParams, bool) {
        let block = self.params(self.rocket_family(type_name));
        (
            block,
            block.type_name.eq_ignore_ascii_case(&self.cmisl.type_name),
        )
    }

    /// A family's block.
    pub fn params(&self, family: MissileFamily) -> &MissileSpawnParams {
        match family {
            MissileFamily::V3Rocket => &self.v3,
            MissileFamily::DMisl => &self.dmisl,
            MissileFamily::CMisl => &self.cmisl,
        }
    }

    /// Frames a launched missile slot waits before it expires its child:
    /// `SpawnManagerClass::AI` adds the V3 TiltFrames + PauseFrames when the
    /// pool's type is the V3 type and the DMisl pair for every other type.
    /// A negative sum expires the native timer at once, as zero does.
    pub fn kamikaze_wait_frames(&self, family: MissileFamily) -> u32 {
        let block = if family == MissileFamily::V3Rocket {
            &self.v3
        } else {
            &self.dmisl
        };
        block.tilt_frames.wrapping_add(block.pause_frames).max(0) as u32
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(text: &str) -> MissileSpawnRules {
        MissileSpawnRules::from_ini(&IniFile::from_str(text))
    }

    #[test]
    fn constructor_seeds_every_block() {
        let rules = MissileSpawnRules::default();
        assert_eq!(rules.family_of("V3ROCKET"), None);
        let v3 = &rules.v3;
        assert_eq!((v3.pause_frames, v3.tilt_frames), (0, 60));
        assert_eq!(v3.pitch_initial.bits(), 0);
        assert_eq!(v3.pitch_final.bits(), 0x3F80_0000);
        assert_eq!(v3.turn_rate.bits(), 0x3D4C_CCCD);
        assert_eq!(v3.raise_rate, 1);
        assert_eq!(v3.acceleration.bits(), 0x3ECC_CCCD);
        assert_eq!((v3.altitude, v3.damage, v3.elite_damage), (768, 1000, 1000));
        assert_eq!((v3.body_length, v3.lazy_curve), (256, true));
        let dmisl = &rules.dmisl;
        assert_eq!((dmisl.pause_frames, dmisl.tilt_frames), (10, 60));
        assert_eq!((dmisl.body_length, dmisl.lazy_curve), (128, false));
        let cmisl = &rules.cmisl;
        assert_eq!((cmisl.pause_frames, cmisl.tilt_frames), (10, 30));
        assert_eq!(cmisl.pitch_initial.bits(), 0x3F80_0000);
        assert_eq!(cmisl.turn_rate.bits(), 0x3F80_0000);
        assert_eq!(cmisl.acceleration.bits(), 0x3F19_999A);
        assert_eq!(
            (cmisl.damage, cmisl.elite_damage, cmisl.body_length),
            (500, 500, 128)
        );
    }

    #[test]
    fn parses_general_and_combat_damage() {
        let rules = parse(
            "[General]\n\
             V3RocketType=V3ROCKET\n\
             V3RocketPauseFrames=0\n\
             V3RocketTiltFrames=60\n\
             V3RocketPitchInitial=0.21\n\
             V3RocketRaiseRate=2.9\n\
             V3RocketDamage=200\n\
             V3RocketEliteDamage=400\n\
             V3RocketLazyCurve=no\n\
             DMislType=DMISL\n\
             DMislPauseFrames=20\n\
             DMislDamage=300\n\
             CMislType=CMISL\n\
             \n\
             [CombatDamage]\n\
             V3Warhead=V3WH\n\
             V3EliteWarhead=V3EWH\n\
             DMislWarhead=DMISLWH\n",
        );
        assert_eq!(rules.v3.damage, 200);
        assert_eq!(rules.v3.damage_for(true), 400);
        assert_eq!(rules.v3.warhead_for(false), "V3WH");
        assert_eq!(rules.v3.warhead_for(true), "V3EWH");
        // binary32 0.21 under the chop control word.
        assert_eq!(rules.v3.pitch_initial.bits(), 0x3E57_0A3D);
        // ReadDouble then ftol truncates.
        assert_eq!(rules.v3.raise_rate, 2);
        assert!(!rules.v3.lazy_curve);
        assert_eq!(rules.dmisl.pause_frames, 20);
        assert_eq!(rules.dmisl.warhead, "DMISLWH");
        assert_eq!(rules.family_of("dmisl"), Some(MissileFamily::DMisl));
        assert_eq!(rules.rocket_family("CMISL"), MissileFamily::CMisl);
        assert_eq!(rules.rocket_family("SOMETHING"), MissileFamily::DMisl);
    }

    #[test]
    fn kamikaze_wait_reads_v3_or_dmisl_frames() {
        let rules = parse(
            "[General]\n\
             V3RocketPauseFrames=0\n\
             V3RocketTiltFrames=60\n\
             DMislPauseFrames=20\n\
             DMislTiltFrames=60\n\
             CMislPauseFrames=20\n\
             CMislTiltFrames=100\n",
        );
        assert_eq!(rules.kamikaze_wait_frames(MissileFamily::V3Rocket), 60);
        assert_eq!(rules.kamikaze_wait_frames(MissileFamily::DMisl), 80);
        assert_eq!(rules.kamikaze_wait_frames(MissileFamily::CMisl), 80);
        // The locomotor still reads CMisl's own frames.
        assert_eq!(rules.cmisl.tilt_frames, 100);
    }

    #[test]
    fn cmisl_acceleration_falls_back_to_dmisl_in_every_general_pass() {
        let mut rules = parse(
            "[General]\n\
             DMislAcceleration=0.8\n\
             CMislAcceleration=1.0\n",
        );
        assert_eq!(rules.cmisl.acceleration.bits(), 0x3F80_0000);
        // A later pass with [General] but without CMislAcceleration= copies
        // DMisl's live value.
        rules.apply_pass(&IniFile::from_str("[General]\nDMislAcceleration=2.0\n"));
        assert_eq!(rules.dmisl.acceleration.bits(), 0x4000_0000);
        assert_eq!(rules.cmisl.acceleration.bits(), 0x4000_0000);
        // A pass without [General] reads nothing.
        rules.apply_pass(&IniFile::from_str("[Basic]\nName=x\n"));
        assert_eq!(rules.cmisl.acceleration.bits(), 0x4000_0000);
    }

    #[test]
    fn elite_warheads_default_to_the_pass_normal_warhead() {
        let mut rules = parse(
            "[CombatDamage]\n\
             V3Warhead=V3WH\n\
             V3EliteWarhead=V3EWH\n\
             CMislWarhead=CMISLWH\n",
        );
        assert_eq!(rules.v3.elite_warhead, "V3EWH");
        assert_eq!(rules.cmisl.elite_warhead, "CMISLWH");
        rules.apply_pass(&IniFile::from_str("[CombatDamage]\nC4Delay=0.03\n"));
        assert_eq!(rules.v3.warhead, "V3WH");
        assert_eq!(rules.v3.elite_warhead, "V3WH");
    }

    #[test]
    fn custom_type_names_reroute_the_family_test() {
        let rules = parse("[General]\nV3RocketType=MYROCKET\n");
        assert_eq!(rules.family_of("MYROCKET"), Some(MissileFamily::V3Rocket));
        assert_eq!(rules.family_of("V3ROCKET"), None);
    }
}
