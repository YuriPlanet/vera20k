//! The ten `Jumpjet*` fields every `TechnoType` carries.
//!
//! gamemd-derived: `TechnoTypeClass::ReadINI` reads all ten in one
//! straight-line run at `0x00715020`-`0x0071520F` with **zero branch
//! instructions** — `JumpJet=` itself is the last of them, an ordinary sibling
//! boolean at `+0xD94`, not a gate on the other nine. Every read uses the same
//! three-instruction idiom: load the field's *current* value, push it as the
//! reader's default, store the result back. So an absent key leaves the
//! constructor's value in place, which is why [`JumpjetParams::default`] is the
//! single source of truth for the defaults and why this struct is unconditional
//! rather than an `Option`.
//!
//! ## Key spelling is load-bearing
//!
//! gamemd's `INIClass` is case-SENSITIVE: it compares CRC-32s of the raw key
//! bytes on both the store side (`INIClass::LoadFromStraw @ 0x00525A60`) and
//! the lookup side (`CCINIClass::ReadInt @ 0x005276D0`), and
//! `CRCEngine::AddData @ 0x004A1DE0` folds no case. The literals this module
//! must use are the ones the binary pushes, read out of `.rdata` at
//! `0x00843640`-`0x008436D0`: `JumpjetTurnRate`, `JumpjetSpeed`,
//! `JumpjetClimb`, `JumpjetCrash`, `JumpjetHeight`, `JumpjetAccel`,
//! `JumpjetWobbles`, `JumpjetNoWobbles`, `JumpjetDeviation`, `JumpJet` — a
//! lowercase `jet` everywhere except the flag itself.
//!
//! Stock `rulesmd.ini` spells two of them with a capital mid-word `J`:
//! `JumpJetTurnRate=` and `JumpJetAccel=`, in all eight jumpjet sections. Those
//! two lines are therefore **inert in retail** — no stock unit has ever had a
//! turn rate or acceleration read from the INI, and all eight fly on the
//! constructor's 4 and 2.0. Reading the INI's spelling instead of gamemd's, as
//! this module used to, applied an acceleration gamemd ignores.
//!
//! ## Recorded DRIFTs
//!
//! One native behaviour this module does not reproduce, harmless on stock data
//! and written out where the field lives: the 208-lepton hover floor the
//! Jumpjet locomotor applies to [`JumpjetParams::height`].
//!
//! ## Dependency rules
//! - Part of rules/ — no dependencies on sim/, render/, ui/, etc.

use crate::rules::ini_parser::IniSection;
use crate::util::fixed_math::SimFixed;

/// `TechnoTypeClass::Constructor` seed for `+0xD70` (`0x007115AE`).
const CTOR_TURN_RATE: i32 = 4;
/// `TechnoTypeClass::Constructor` seed for `+0xD74` (`0x007115B8`, `0xE`).
const CTOR_SPEED: i32 = 14;
/// `TechnoTypeClass::Constructor` seed for `+0xD78` (`0x007115C7`,
/// `0x40A00000`). `+0xD7C` takes the same register on the next instruction.
const CTOR_CLIMB: f32 = 5.0;
/// `TechnoTypeClass::Constructor` seed for `+0xD7C` (`0x007115CD`).
const CTOR_CRASH: f32 = 5.0;
/// `TechnoTypeClass::Constructor` seed for `+0xD80` (`0x007115D3`, `0x1F4`).
const CTOR_HEIGHT: i32 = 500;
/// `TechnoTypeClass::Constructor` seed for `+0xD84` (`0x007115DD`,
/// `0x40000000`).
const CTOR_ACCEL: f32 = 2.0;
/// `TechnoTypeClass::Constructor` seed for `+0xD88` (`0x007115E7`,
/// `0x3E19999A`).
const CTOR_WOBBLES: f32 = 0.15;
/// `TechnoTypeClass::Constructor` seed for `+0xD90` (`0x007115F7`, `0x28`).
const CTOR_DEVIATION: i32 = 40;

/// The Jumpjet block of `TechnoTypeClass`, `+0xD70` through `+0xD90`.
///
/// Present on every type, exactly as in gamemd: the constructor seeds all nine
/// and `ReadINI` overwrites whichever keys the section authors. `JumpJet=`
/// (`+0xD94`) is a separate sibling boolean and lives on `ObjectType`, not
/// here.
///
/// Only the Jumpjet locomotor ever reads these — the parameter copy at
/// `0x0054AD30` pulls `+0xD70`..`+0xD8C` off the type — so a Drive, Fly or
/// Hover unit carries them and never uses them.
#[derive(Debug, Clone)]
pub struct JumpjetParams {
    /// Turning speed while airborne (`JumpjetTurnRate=`, `+0xD70`).
    pub turn_rate: i32,
    /// Flight speed (`JumpjetSpeed=`, `+0xD74`). An absolute air speed, not a
    /// multiplier on `Speed=`: stock `[JUMPJET]` pairs `Speed=9` with
    /// `JumpjetSpeed=30`, and the constructor seeds 14 — the same value stock
    /// `[JumpjetControls] Speed=` carries. Native `+0xD74` is an `int`, read
    /// by `ReadInt`; VERA holds that integer as `SimFixed`.
    pub speed: SimFixed,
    /// Climb/ascent rate per tick (`JumpjetClimb=`, `+0xD78`).
    pub climb: f32,
    /// Extra descent speed added during crash (`JumpjetCrash=`, `+0xD7C`).
    /// Total crash speed = climb + crash.
    pub crash: f32,
    /// Target hover altitude in leptons (`JumpjetHeight=`, `+0xD80`).
    ///
    /// RESIDUAL — DRIFT, not implemented in VERA. gamemd never hovers at this
    /// value raw: the `JumpjetLocomotionClass` parameter copy at `0x0054AD30`
    /// clamps it **up** on the way into the locomotor (`0x0054AD9B`:
    /// `ECX = 2 * [0x00ABC5E8]`, `CMP EAX,ECX`, `JG` keeps the type's value,
    /// else `EAX = ECX`), giving an effective floor of **208 leptons**.
    /// `[0x00ABC5E8]` has exactly one writer, `0x0054AB2B`, and every input is
    /// a static `.rdata` constant: `[0x007ECD38] = pi/180`, times
    /// `[0x007E1730] = 90` gives `[0x00ABC570] = pi/2` (`0x0054AAE0`); times
    /// `[0x007E1728] = 60` gives `[0x00ABC598] = pi/3` (`0x0054AAC0`);
    /// `sqrt(2 * 256^2)` gives `[0x00ABC590] = 256*sqrt(2) = 362.0387`
    /// (`0x0054AA30`); then `0x0054AB00` computes
    /// `ftol(tan_table(pi/2 - pi/3) * 362.0387 * 0.5)`, the 0.5 being
    /// `[0x007E1738]`. The table entry the index lands on, `0x0085D5F8`, holds
    /// `0x3F13A08F = 0.5766687`, so `104.3878 -> 104` and the floor is
    /// `2 * 104`. Every constant in that chain was read out of the binary.
    ///
    /// Trigger: a section authoring `JumpjetHeight` below 208. Player effect:
    /// that unit hovers lower in VERA than in gamemd, permanently. Frequency:
    /// **zero on stock data** — the lowest authored height in retail
    /// `rulesmd.ini` is 500, and `[ZEP]`/`[DISK]` author 750, so every stock
    /// jumpjet clears the floor. Only a mod or map INI can reach it.
    /// Downstream risk: none — nothing else reads the clamped value.
    pub height: i32,
    /// Acceleration rate (`JumpjetAccel=`, `+0xD84`). Deceleration = accel *
    /// 1.5.
    pub accel: f32,
    /// Wobble amplitude while hovering (`JumpjetWobbles=`, `+0xD88`).
    /// KEPT as f32 — only used for render-side visual wobble, not sim state.
    pub wobbles: f32,
    /// Maximum random XY deviation in leptons (`JumpjetDeviation=`, `+0xD90`).
    pub deviation: i32,
    /// When true, disables wobble effect entirely (`JumpjetNoWobbles=`,
    /// `+0xD8C`).
    pub no_wobbles: bool,
}

impl Default for JumpjetParams {
    /// The `TechnoTypeClass::Constructor` seeds, `0x007115AE`-`0x007115F7`.
    ///
    /// These are what a type keeps for any key its section does not author,
    /// because `ReadINI` passes the current field value as the reader default.
    fn default() -> Self {
        Self {
            turn_rate: CTOR_TURN_RATE,
            speed: SimFixed::from_num(CTOR_SPEED),
            climb: CTOR_CLIMB,
            crash: CTOR_CRASH,
            height: CTOR_HEIGHT,
            accel: CTOR_ACCEL,
            wobbles: CTOR_WOBBLES,
            deviation: CTOR_DEVIATION,
            no_wobbles: false,
        }
    }
}

impl JumpjetParams {
    /// Parse the nine jumpjet parameters from a rules.ini section.
    ///
    /// gamemd-derived: `TechnoTypeClass::ReadINI` @ `0x00715020`-`0x007151DF`,
    /// one unconditional read per key over the constructor seed. Called for
    /// every section, jumpjet or not, exactly as the native run is.
    pub fn from_ini_section(section: &IniSection) -> Self {
        let ctor = Self::default();
        Self {
            // `0x007150AF PUSH 0x8436D0` -> `0x007150C3 MOV [EBP+0xD70],EAX`.
            // Stock spells this `JumpJetTurnRate=`, which gamemd cannot see, so
            // all eight stock jumpjet sections keep the constructor's 4.
            turn_rate: section.read_int("JumpjetTurnRate", ctor.turn_rate),
            // `0x007150D0 PUSH 0x8436C0` -> `0x007150E4 MOV [EBP+0xD74],EAX`:
            // ReadInt, so a fractional `JumpjetSpeed=16.5` reads as 16.
            speed: SimFixed::from_num(section.read_int("JumpjetSpeed", CTOR_SPEED)),
            // `0x007150F8 PUSH 0x8436B0` -> `0x0071510A FSTP [EBP+0xD78]`.
            climb: section.read_float("JumpjetClimb", ctor.climb),
            // `0x0071511E PUSH 0x8436A0` -> `0x00715130 FSTP [EBP+0xD7C]`.
            crash: section.read_float("JumpjetCrash", ctor.crash),
            // `0x0071513F PUSH 0x843690` -> `0x00715151 MOV [EBP+0xD80],EAX`.
            height: section.read_int("JumpjetHeight", ctor.height),
            // `0x00715165 PUSH 0x843680` -> `0x00715177 FSTP [EBP+0xD84]`.
            // Stock spells this `JumpJetAccel=`; same story as the turn rate.
            accel: section.read_float("JumpjetAccel", ctor.accel),
            // `0x0071518B PUSH 0x843670` -> `0x0071519D FSTP [EBP+0xD88]`.
            wobbles: section.read_float("JumpjetWobbles", ctor.wobbles),
            // `0x007151CB PUSH 0x843648` -> `0x007151DF MOV [EBP+0xD90],EAX`.
            deviation: section.read_int("JumpjetDeviation", ctor.deviation),
            // `0x007151AC PUSH 0x84365C` -> `0x007151BE MOV [EBP+0xD8C],AL`.
            no_wobbles: section.read_bool("JumpjetNoWobbles", ctor.no_wobbles),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rules::ini_parser::IniFile;

    #[test]
    fn test_parse_jumpjet_defaults() {
        let ini = IniFile::from_str("[RTNK]\nJumpJet=yes\n");
        let section = ini.section("RTNK").unwrap();
        let params = JumpjetParams::from_ini_section(section);

        assert_eq!(params.turn_rate, 4);
        assert_eq!(params.speed, SimFixed::from_num(14));
        assert_eq!(params.climb, 5.0);
        assert_eq!(params.crash, 5.0);
        assert_eq!(params.height, 500);
        assert_eq!(params.accel, 2.0);
        assert!((params.wobbles - 0.15).abs() < 0.01);
        assert_eq!(params.deviation, 40);
        assert!(!params.no_wobbles);
    }

    /// gamemd looks up `JumpjetTurnRate` and `JumpjetAccel` (`0x008436D0` and
    /// `0x00843680`), and its `INIClass` compares CRC-32s of the raw key bytes
    /// with no case folding (`CRCEngine::AddData @ 0x004A1DE0`). Stock spells
    /// both with a capital mid-word `J`, so retail never reads either one.
    #[test]
    fn miscased_turn_rate_and_accel_are_not_read() {
        let ini =
            IniFile::from_str("[ZEP]\nJumpJetTurnRate=2\nJumpJetAccel=10\nJumpjetHeight=750\n");
        let section = ini.section("ZEP").unwrap();
        let params = JumpjetParams::from_ini_section(section);

        assert_eq!(
            params.turn_rate, 4,
            "`JumpJetTurnRate=` is invisible to gamemd; the constructor's 4 stands"
        );
        assert_eq!(
            params.accel, 2.0,
            "`JumpJetAccel=` is invisible to gamemd; the constructor's 2.0 stands"
        );
        // The correctly-cased sibling on the same fixture still lands, which is
        // what makes this a case test rather than a "nothing parses" test.
        assert_eq!(params.height, 750);
    }
}
