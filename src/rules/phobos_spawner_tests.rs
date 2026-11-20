//! `Spawner.*` reads (`Ext/TechnoType/Body.cpp:790-794`, `:1120-1123`) and the
//! four `[General]` fallbacks (`Ext/Rules/Body.cpp:208-211`).
use super::PhobosSpawner;
use crate::rules::ini_parser::IniFile;
use crate::rules::phobos_globals::PhobosGlobals;

fn read(text: &str) -> PhobosSpawner {
    PhobosSpawner::from_section(IniFile::from_str(text).section_or_empty("Q"))
}

#[test]
fn spawner_defaults_and_all_nine_keys() {
    let defaults = PhobosSpawner::default();
    assert_eq!(read("[Q]\n"), defaults);
    assert!(!defaults.limit_range);
    assert_eq!(defaults.extra_limit_range, 0);
    assert_eq!(defaults.delay_frames, None);
    assert_eq!(defaults.attack_immediately, None);
    assert_eq!(defaults.use_turret_facing, None);
    assert_eq!(defaults.recycle_range, None);
    assert!(defaults.recycle_anim.is_empty());
    assert_eq!(defaults.recycle_coord, [0; 3]);
    assert_eq!(defaults.recycle_on_turret, None);

    let set = read(
        "[Q]\n\
         Spawner.LimitRange=yes\n\
         Spawner.ExtraLimitRange=2\n\
         Spawner.DelayFrames=0\n\
         Spawner.AttackImmediately=yes\n\
         Spawner.UseTurretFacing=yes\n\
         Spawner.RecycleRange=0.5\n\
         Spawner.RecycleAnim=GAANIM,GBANIM\n\
         Spawner.RecycleCoord=1,-2,3\n\
         Spawner.RecycleOnTurret=yes\n",
    );
    assert!(set.limit_range);
    assert_eq!(set.extra_limit_range, 2);
    assert_eq!(set.delay_frames, Some(0), "a zero delay is a set value");
    assert_eq!(set.attack_immediately, Some(true));
    assert_eq!(set.use_turret_facing, Some(true));
    // `Nullable<Leptons>` reads cells and stores `F2I(cells * 256)`.
    assert_eq!(set.recycle_range, Some(128));
    assert_eq!(set.recycle_anim, ["GAANIM", "GBANIM"]);
    assert_eq!(set.recycle_coord, [1, -2, 3]);
    assert_eq!(set.recycle_on_turret, Some(true));

    // `none` clears a Nullable; `DelayFrames` is an int Nullable.
    let cleared = read(
        "[Q]\nSpawner.DelayFrames=4\nSpawner.AttackImmediately=none\n\
         Spawner.UseTurretFacing=<none>\nSpawner.RecycleRange=<default>\n\
         Spawner.RecycleOnTurret=none\n",
    );
    assert_eq!(cleared.delay_frames, Some(4));
    assert_eq!(cleared.attack_immediately, None);
    assert_eq!(cleared.use_turret_facing, None);
    assert_eq!(cleared.recycle_range, None);
    assert_eq!(cleared.recycle_on_turret, None);
}

#[test]
fn spawner_general_fallbacks() {
    let globals = PhobosGlobals::default();
    assert!(!globals.spawner_attack_immediately);
    assert!(!globals.spawner_use_turret_facing);
    assert_eq!(
        globals.spawner_recycle_range, -1,
        "Leptons(-1) is the sentinel"
    );
    assert!(!globals.spawner_recycle_on_turret);

    let globals = PhobosGlobals::from_ini(&IniFile::from_str(
        "[General]\nSpawner.AttackImmediately=yes\nSpawner.UseTurretFacing=yes\n\
         Spawner.RecycleRange=0.5\nSpawner.RecycleOnTurret=yes\n",
    ));
    assert!(globals.spawner_attack_immediately);
    assert!(globals.spawner_use_turret_facing);
    assert_eq!(globals.spawner_recycle_range, 128);
    assert!(globals.spawner_recycle_on_turret);

    let defaults = PhobosSpawner::default();
    assert_eq!(defaults.attack_immediately(true), true);
    assert_eq!(defaults.use_turret_facing(true), true);
    assert_eq!(defaults.recycle_range(128), 128);
    assert_eq!(defaults.recycle_on_turret(true), true);
    let set = read("[Q]\nSpawner.AttackImmediately=no\n");
    assert!(!set.attack_immediately(true), "a set type key wins");
}
