//! `[AudioVisual] LaserZAdjust=` / `EBoltZAdjust=` reads and the nullable
//! fallback each weapon uses.

use super::{BeamZAdjustGlobals, resolved, resolved_clamp};
use crate::rules::ini_parser::IniFile;

fn z_adjusts(ini: &str) -> BeamZAdjustGlobals {
    BeamZAdjustGlobals::from_ini(&IniFile::from_str(ini))
}

/// `Ext/Rules/Body.cpp:277-278`: both live in `[AudioVisual]` and default to 0
/// when the section or the key is absent.
#[test]
fn the_global_z_adjusts_default_to_zero_and_read_audio_visual() {
    assert_eq!(z_adjusts("[General]\nLaserZAdjust=5\n").laser, 0);
    assert_eq!(
        z_adjusts("[AudioVisual]\nLaserZAdjust=5\nEBoltZAdjust=-3\n"),
        BeamZAdjustGlobals {
            laser: 5,
            ebolt: -3,
            ebolt_clamp_initial_depth_for_building: true,
        }
    );
    assert!(
        !z_adjusts("[AudioVisual]\nEBoltZAdjust.ClampInitialDepthForBuilding=no\n")
            .ebolt_clamp_initial_depth_for_building,
        "the clamp key defaults to the constructor's yes"
    );
}

/// `Nullable<int>::Get(global)`: an unset weapon key inherits, a set one wins.
#[test]
fn a_weapon_override_replaces_the_global() {
    assert_eq!(resolved(None, 4), 4);
    assert_eq!(resolved(Some(-7), 4), -7);
    assert_eq!(resolved(Some(0), 4), 0);
}

/// `Ext/Rules/Body.cpp:279` + `Body.h:720`: the clamp is a `Valueable<bool>`
/// whose constructor default is **true**, so an absent key keeps the clamp.
#[test]
fn the_building_clamp_defaults_to_on_and_reads_audio_visual() {
    assert!(
        z_adjusts("[AudioVisual]\nLaserZAdjust=1\n").ebolt_clamp_initial_depth_for_building,
        "an absent EBoltZAdjust.ClampInitialDepthForBuilding keeps the constructor default"
    );
    assert!(
        !z_adjusts("[AudioVisual]\nEBoltZAdjust.ClampInitialDepthForBuilding=no\n")
            .ebolt_clamp_initial_depth_for_building
    );
    assert!(
        z_adjusts("[General]\nEBoltZAdjust.ClampInitialDepthForBuilding=no\n")
            .ebolt_clamp_initial_depth_for_building,
        "the key is read from [AudioVisual], so a [General] copy leaves the default"
    );
}

/// `Nullable<bool>::Get(global)` at `Ext/EBolt/Hooks.cpp:196-200`.
#[test]
fn a_weapon_override_decides_the_building_clamp() {
    assert!(resolved_clamp(None, true));
    assert!(!resolved_clamp(None, false));
    assert!(!resolved_clamp(Some(false), true));
    assert!(resolved_clamp(Some(true), false));
}
