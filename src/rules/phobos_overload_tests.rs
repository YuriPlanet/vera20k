//! `Overload.*` reads (`Ext/TechnoType/Body.cpp:1133-1138`).
use super::PhobosOverloadSettings;
use crate::rules::ini_parser::IniFile;

fn read(text: &str) -> PhobosOverloadSettings {
    PhobosOverloadSettings::from_section(IniFile::from_str(text).section_or_empty("MIND"))
}

#[test]
fn overload_defaults_leave_every_fallback_in_place() {
    let set = read("");
    assert_eq!(set.count, None);
    assert_eq!(set.damage, None);
    assert_eq!(set.frames, None);
    assert_eq!(set.death_sound, None);
    assert_eq!(set.particle_sys, None);
    assert_eq!(set.particle_sys_count, None);
}

#[test]
fn overload_reads_its_lists_particle_and_count() {
    let set = read(
        "[MIND]\nOverload.Count=2,5,9\nOverload.Damage=0,25,75\n\
         Overload.Frames=30,15,5\nOverload.ParticleSys=SPARKX\n\
         Overload.ParticleSysCount=2\n",
    );
    assert_eq!(set.count, Some(vec![2, 5, 9]));
    assert_eq!(set.damage, Some(vec![0, 25, 75]));
    assert_eq!(set.frames, Some(vec![30, 15, 5]));
    assert_eq!(set.particle_sys.as_deref(), Some("SPARKX"));
    assert_eq!(set.particle_sys_count, Some(2));
}

/// `NullableVector<int>`: a present key replaces the Rules list even when no
/// token converts, which the overload hook then reads as "no checks".
/// `<default>` and an empty value return to the Rules fallback.
#[test]
fn a_present_empty_list_is_set_not_absent() {
    let set = read("[MIND]\nOverload.Count=,,,\nOverload.Damage=abc\n");
    assert_eq!(set.count, Some(Vec::new()));
    assert_eq!(set.damage, Some(Vec::new()));
    assert_eq!(set.frames, None);
    let reset =
        read("[MIND]\nOverload.Count=<default>\nOverload.Damage=\nOverload.Frames=<DEFAULT>\n");
    assert_eq!(reset.count, None);
    assert_eq!(reset.damage, None);
    assert_eq!(reset.frames, None);
}
