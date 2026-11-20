//! `SpyEffect.*` reads (`Ext/BuildingType/Body.cpp:337-345`).
use super::PhobosSpyEffect;
use crate::rules::ini_parser::IniFile;

fn read(text: &str) -> PhobosSpyEffect {
    PhobosSpyEffect::from_section(IniFile::from_str(text).section_or_empty("Q"))
}

#[test]
fn spy_effect_keys_defaults_reads_and_clear_tokens() {
    let defaults = PhobosSpyEffect::default();
    assert_eq!(read("[Q]\n"), defaults);
    assert!(!defaults.custom);
    assert_eq!(defaults.victim_super_weapon, None);
    assert_eq!(defaults.infiltrator_super_weapon, None);

    let set = read(
        "[Q]\n\
         SpyEffect.Custom=yes\n\
         SpyEffect.VictimSuperWeapon=VSHIELD\n\
         SpyEffect.InfiltratorSuperWeapon=ISTORM\n",
    );
    assert!(set.custom);
    assert_eq!(set.victim_super_weapon.as_deref(), Some("VSHIELD"));
    assert_eq!(set.infiltrator_super_weapon.as_deref(), Some("ISTORM"));

    // `ValueableIdx`: `none`/`<none>` stores -1; the name is otherwise kept
    // for the use-time lookup (an unknown name launches nothing).
    let cleared = read(
        "[Q]\nSpyEffect.VictimSuperWeapon=<none>\n\
         SpyEffect.InfiltratorSuperWeapon=none\n",
    );
    assert_eq!(cleared.victim_super_weapon, None);
    assert_eq!(cleared.infiltrator_super_weapon, None);
    let unknown = read("[Q]\nSpyEffect.VictimSuperWeapon=NOTASW\n");
    assert_eq!(unknown.victim_super_weapon.as_deref(), Some("NOTASW"));
}
