//! `Promote.*` reads (`Ext/TechnoType/Body.cpp:800,1128-1129`) and the
//! `[General]`/`[AudioVisual]` fallbacks (`Ext/Rules/Body.cpp:212,316-317`).
use super::PhobosPromote;
use crate::rules::ini_parser::IniFile;
use crate::rules::phobos_globals::PhobosGlobals;

fn read(text: &str) -> PhobosPromote {
    PhobosPromote::from_section(IniFile::from_str(text).section_or_empty("Q"))
}

#[test]
fn promote_keys_read_with_their_global_fallbacks() {
    let defaults = PhobosPromote::default();
    assert_eq!(read("[Q]\n"), defaults);
    assert_eq!(defaults.include_spawns, None);
    assert!(defaults.veteran_animation.is_empty());
    assert!(defaults.elite_animation.is_empty());

    let set = read(
        "[Q]\n\
         Promote.IncludeSpawns=yes\n\
         Promote.VeteranAnimation=GAANIM,GBANIM\n\
         Promote.EliteAnimation=GCANIM\n",
    );
    assert_eq!(set.include_spawns, Some(true));
    assert_eq!(set.veteran_animation, ["GAANIM", "GBANIM"]);
    assert_eq!(set.elite_animation, ["GCANIM"]);

    // `Nullable<bool>` clears; `ValueableVector` keeps its current list on an
    // absent or empty key and replaces it wholesale otherwise.
    let cleared = read("[Q]\nPromote.IncludeSpawns=<none>\nPromote.VeteranAnimation=\n");
    assert_eq!(cleared.include_spawns, None);
    assert!(cleared.veteran_animation.is_empty());
    let replaced = read("[Q]\nPromote.VeteranAnimation=GXANIM\n");
    assert_eq!(replaced.veteran_animation, ["GXANIM"]);
    assert_ne!(
        replaced.veteran_animation, set.veteran_animation,
        "a type list replaces, never appends"
    );

    let globals = PhobosGlobals::default();
    assert!(!globals.promote_include_spawns);
    assert!(globals.promote_veteran_animation.is_empty());
    assert!(globals.promote_elite_animation.is_empty());
    let globals = PhobosGlobals::from_ini(&IniFile::from_str(
        "[General]\nPromote.IncludeSpawns=yes\n\
         [AudioVisual]\nPromote.VeteranAnimation=GVET\nPromote.EliteAnimation=GELI\n",
    ));
    assert!(globals.promote_include_spawns);
    assert_eq!(globals.promote_veteran_animation, ["GVET"]);
    assert_eq!(globals.promote_elite_animation, ["GELI"]);

    let own = read("[Q]\nPromote.VeteranAnimation=TVET\n");
    assert_eq!(
        own.veteran_animation(&globals.promote_veteran_animation),
        ["TVET"]
    );
    assert_eq!(
        own.elite_animation(&globals.promote_elite_animation),
        ["GELI"],
        "an unset tier keeps the global list"
    );
    // The three reads are independent: the type overrides only what it names.
    let unset = PhobosPromote::default();
    assert_eq!(
        unset.veteran_animation(&globals.promote_veteran_animation),
        ["GVET"]
    );
    assert_eq!(
        unset.elite_animation(&globals.promote_elite_animation),
        ["GELI"]
    );
    assert!(unset.include_spawns(true));
    assert!(!unset.include_spawns(false));
    let cleared = read("[Q]\nPromote.IncludeSpawns=no\n");
    assert!(
        !cleared.include_spawns(true),
        "a set `no` overrides the global"
    );
}
