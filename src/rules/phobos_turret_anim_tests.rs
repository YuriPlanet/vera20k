//! `TurretAnim.*` reads (`Ext/BuildingType/Body.cpp:260-265`) and
//! `BuildingExt::GetTurretFrame` (`Ext/Building/Body.cpp:488-569`).
use super::{PhobosTurretAnim, TurretAnimState};
use crate::rules::ini_parser::IniFile;

#[test]
fn turret_anim_defaults_and_all_six_keys() {
    let defaults =
        PhobosTurretAnim::from_section(IniFile::from_str("[GTGCAN]\n").section_or_empty("GTGCAN"));
    assert_eq!(defaults, PhobosTurretAnim::default());
    assert_eq!(
        (
            defaults.idle_frames,
            defaults.low_power_idle_frames,
            defaults.firing_frames
        ),
        (1, 0, 0)
    );
    assert_eq!(
        (
            defaults.low_power_firing_frames,
            defaults.idle_rate,
            defaults.firing_rate
        ),
        (0, 1, 1)
    );
    assert_eq!(
        TurretAnimState::default(),
        TurretAnimState {
            idle_frame: 0,
            firing_frame: -1,
            rate_tick: 0
        }
    );

    let set = PhobosTurretAnim::from_section(
        IniFile::from_str(
            "[GTGCAN]\nTurretAnim.IdleFrames=3\nTurretAnim.LowPowerIdleFrames=2\n\
             TurretAnim.FiringFrames=4\nTurretAnim.LowPowerFiringFrames=5\n\
             TurretAnim.IdleRate=6\nTurretAnim.FiringRate=2Ah\n",
        )
        .section_or_empty("GTGCAN"),
    );
    assert_eq!(set.idle_frames, 3);
    assert_eq!(set.low_power_idle_frames, 2);
    assert_eq!(set.firing_frames, 4);
    assert_eq!(set.low_power_firing_frames, 5);
    assert_eq!(set.idle_rate, 6);
    assert_eq!(set.firing_rate, 42, "Ares Valueable<int> hex suffix");
}

#[test]
fn get_turret_frame_idle_rate_and_32_frame_blocks() {
    let rules = PhobosTurretAnim {
        idle_frames: 2,
        low_power_idle_frames: 3,
        firing_frames: 4,
        low_power_firing_frames: 5,
        idle_rate: 2,
        firing_rate: 2,
    };
    // Unset keys: one frame per facing, no rate advance, no block offset.
    let plain = PhobosTurretAnim::default();
    let mut plain_state = TurretAnimState::default();
    assert_eq!(plain.get_turret_frame(&mut plain_state, 31, false), 31);
    assert_eq!(plain_state, TurretAnimState::default());

    // IdleRate=2: each idle frame holds for two AnimationAI frames.
    // shapeFacing=1, Idle block at offset 0, framesPerFacing=2.
    let mut state = TurretAnimState::default();
    assert_eq!(rules.get_turret_frame(&mut state, 1, false), 2);
    assert_eq!(rules.get_turret_frame(&mut state, 1, false), 2);
    assert_eq!(rules.get_turret_frame(&mut state, 1, false), 3);
    assert_eq!(rules.get_turret_frame(&mut state, 1, false), 3);
    assert_eq!(rules.get_turret_frame(&mut state, 1, false), 2);

    // Low power + LowPowerIdleFrames>0: baseOffset = 32*IdleFrames = 64,
    // framesPerFacing=3. The 32-frame blocks scale shapeFacing=31.
    let mut low = TurretAnimState::default();
    assert_eq!(rules.get_turret_frame(&mut low, 31, true), 64 + 31 * 3);
    // LowPowerIdleFrames=0 falls back to the Idle block (baseOffset 0).
    let no_low = PhobosTurretAnim {
        low_power_idle_frames: 0,
        ..rules
    };
    let mut no_low_state = TurretAnimState::default();
    assert_eq!(no_low.get_turret_frame(&mut no_low_state, 0, true), 0);

    // Rate <= 0: the `>=` test is true every frame, so idle advances once
    // per call and loops at framesPerFacing.
    let fast = PhobosTurretAnim {
        idle_frames: 2,
        idle_rate: 0,
        ..PhobosTurretAnim::default()
    };
    let mut fast_state = TurretAnimState::default();
    assert_eq!(fast.get_turret_frame(&mut fast_state, 0, false), 0);
    assert_eq!(fast.get_turret_frame(&mut fast_state, 0, false), 1);
    assert_eq!(fast.get_turret_frame(&mut fast_state, 0, false), 0);
}

#[test]
fn get_turret_frame_firing_arm_fall_back_and_low_power_firing() {
    let rules = PhobosTurretAnim {
        idle_frames: 2,
        low_power_idle_frames: 3,
        firing_frames: 4,
        low_power_firing_frames: 5,
        idle_rate: 2,
        firing_rate: 2,
    };
    // Mission_Attack hook 0x44B6C7 arms once; a second arm mid-animation is
    // a no-op (shared RateTick is not reset).
    let mut state = TurretAnimState::default();
    rules.arm_firing(&mut state, false);
    assert_eq!((state.firing_frame, state.rate_tick), (0, 0));
    state.rate_tick = 1;
    rules.arm_firing(&mut state, false);
    assert_eq!((state.firing_frame, state.rate_tick), (0, 1));

    // Firing block: baseOffset = 32*Idle(64) + 32*LowPowerIdle(96) = 160,
    // framesPerFacing=4, FiringRate=2. After 8 frames the firing block ends
    // and the next frame returns to Idle at offset 0.
    let mut firing = TurretAnimState::default();
    rules.arm_firing(&mut firing, false);
    assert_eq!(rules.get_turret_frame(&mut firing, 0, false), 160);
    assert_eq!(rules.get_turret_frame(&mut firing, 0, false), 160);
    assert_eq!(rules.get_turret_frame(&mut firing, 0, false), 161);
    assert_eq!(rules.get_turret_frame(&mut firing, 0, false), 161);
    assert_eq!(rules.get_turret_frame(&mut firing, 0, false), 162);
    assert_eq!(rules.get_turret_frame(&mut firing, 0, false), 162);
    assert_eq!(rules.get_turret_frame(&mut firing, 0, false), 163);
    assert_eq!(rules.get_turret_frame(&mut firing, 0, false), 163);
    assert_eq!(firing.firing_frame, -1);
    assert_eq!((firing.idle_frame, firing.rate_tick), (0, 0));
    assert_eq!(rules.get_turret_frame(&mut firing, 0, false), 0);

    // Low power + firing + LowPowerFiringFrames>0: baseOffset =
    // 64 + 96 + 32*Firing(128) = 288, framesPerFacing=5.
    let mut low = TurretAnimState::default();
    rules.arm_firing(&mut low, true);
    assert_eq!(low.firing_frame, 0);
    assert_eq!(rules.get_turret_frame(&mut low, 31, true), 288 + 31 * 5);

    // LowPowerFiringFrames=0 never arms and, while low power, falls back to
    // the LowPowerIdle block (baseOffset 64) even with a firing frame set.
    let no_lpf = PhobosTurretAnim {
        low_power_firing_frames: 0,
        ..rules
    };
    let mut unarmed = TurretAnimState::default();
    no_lpf.arm_firing(&mut unarmed, true);
    assert_eq!(unarmed.firing_frame, -1);
    let mut low_idle = TurretAnimState::default();
    no_lpf.arm_firing(&mut low_idle, false);
    assert_eq!(no_lpf.get_turret_frame(&mut low_idle, 0, true), 64);
}
