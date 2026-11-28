//! The `JumpjetTilt` gates and the draw-only angle kernel. Values are the
//! Phobos expression of `Ext/TechnoType/Hooks.MatrixOp.cpp:457-472` after the
//! `DirStruct` 128-facing quantization of `YRpp/Dir.h:44-96`.
use super::{PhobosJumpjetTiltGlobal, PhobosJumpjetTiltType, ResolvedJumpjetTilt};
use crate::rules::ini_parser::IniFile;

fn globals(text: &str) -> PhobosJumpjetTiltGlobal {
    PhobosJumpjetTiltGlobal::from_ini(&IniFile::from_str(text))
}

fn resolved() -> ResolvedJumpjetTilt {
    ResolvedJumpjetTilt {
        forward_accel_factor: 1.0,
        forward_speed_factor: 1.0,
        sideways_rotation_factor: 1.0,
        sideways_speed_factor: 1.0,
    }
}

#[test]
fn globals_and_type_overrides_keep_the_valueable_and_nullable_fallbacks() {
    // Constructor defaults: off, each factor 1.0 (`Ext/Rules/Body.h:646-650`).
    let defaults = globals("");
    assert_eq!(defaults, PhobosJumpjetTiltGlobal::default());
    assert!(!defaults.enabled);

    let authored = globals(
        "[AudioVisual]\nJumpjetTilt=yes\nJumpjetTilt.ForwardAccelFactor=2.5\n\
         JumpjetTilt.ForwardSpeedFactor=0\nJumpjetTilt.SidewaysRotationFactor=.5\n\
         JumpjetTilt.SidewaysSpeedFactor=3\n",
    );
    assert!(authored.enabled);
    assert_eq!(authored.forward_accel_factor, 2.5);
    assert_eq!(authored.forward_speed_factor, 0.0);
    assert_eq!(authored.sideways_rotation_factor, 0.5);
    assert_eq!(authored.sideways_speed_factor, 3.0);

    // Type keys are Nullable: absent and `<default>` inherit, a present value
    // overrides only its own field.
    let ini = IniFile::from_str(
        "[TYPE]\nJumpjetTilt.ForwardSpeedFactor=4\n\
         [CLEARED]\nJumpjetTilt=no\nJumpjetTilt.ForwardSpeedFactor=<default>\n\
         [EMPTY]\nKeep=1\n",
    );
    let typed = PhobosJumpjetTiltType::from_section(ini.section("TYPE").unwrap());
    assert_eq!(typed.forward_speed_factor, Some(4.0));
    assert_eq!(typed.enabled, None);
    let resolved = typed.resolve(&authored).unwrap();
    assert_eq!(resolved.forward_speed_factor, 4.0);
    assert_eq!(resolved.forward_accel_factor, 2.5);
    assert_eq!(resolved.sideways_speed_factor, 3.0);

    let cleared = PhobosJumpjetTiltType::from_section(ini.section("CLEARED").unwrap());
    assert_eq!(cleared.enabled, Some(false));
    assert_eq!(cleared.forward_speed_factor, None);
    assert_eq!(
        cleared.resolve(&authored),
        None,
        "a type's JumpjetTilt=no wins over the global yes"
    );
    assert_eq!(
        PhobosJumpjetTiltType::from_section(ini.section("EMPTY").unwrap()),
        PhobosJumpjetTiltType::default()
    );
    assert!(
        PhobosJumpjetTiltType::default()
            .resolve(&defaults)
            .is_none(),
        "no type or global gate keeps the stock draw"
    );
}

#[test]
fn tilt_angles_quantize_to_128_facings_and_skip_zero_axes() {
    let tilt = resolved();
    // Forwards = speed * (pi/2/4)/32 before the DirStruct quantization.
    // Speeds 1 and 2 stay below (or on) half a 128-step and truncate to zero.
    assert_eq!(tilt.tilt_angles(1.0, 0.0, false, 0), None);
    assert_eq!(tilt.tilt_angles(2.0, 0.0, false, 0), None);
    assert_eq!(
        tilt.tilt_angles(3.0, 0.0, false, 0),
        Some([0.0, f32::from_bits(0x3d49_0fdb)]),
        "speed 3 crosses into one 128-step (pi/64)"
    );
    assert_eq!(
        tilt.tilt_angles(32.0, 0.0, false, 0),
        Some([0.0, f32::from_bits(0x3ec9_0fdb)]),
        "speed 32 is pi/8"
    );
    assert_eq!(
        tilt.tilt_angles(10_000.0, 0.0, false, 0),
        Some([0.0, f32::from_bits(0x3fc9_0fdb)]),
        "the clamp holds MaxTilt = float(pi/2)"
    );

    // Sideways needs an active turn; the difference is the signed 16-bit
    // destination-minus-start word. At speed 32 and accel 2 the forwards
    // expression is exactly 8.5 steps before the truncating DirStruct
    // conversion, so it rounds back to 8 steps (pi/8). A quarter turn of
    // difference at the same speed is pi/16.
    assert_eq!(
        tilt.tilt_angles(32.0, 2.0, true, 16_384),
        Some([f32::from_bits(0x3e49_0fdb), f32::from_bits(0x3ec9_0fdb)])
    );
    assert_eq!(
        tilt.tilt_angles(32.0, 2.0, true, -16_384),
        Some([f32::from_bits(0xbe49_0fdb), f32::from_bits(0x3ec9_0fdb)])
    );
    assert_eq!(
        tilt.tilt_angles(32.0, 2.0, false, 16_384),
        Some([0.0, f32::from_bits(0x3ec9_0fdb)]),
        "a difference without IsRotating contributes no sideways angle"
    );

    let zeroed = ResolvedJumpjetTilt {
        forward_accel_factor: 0.0,
        forward_speed_factor: 0.0,
        ..tilt
    };
    assert_eq!(zeroed.tilt_angles(32.0, 2.0, false, 0), None);
}
