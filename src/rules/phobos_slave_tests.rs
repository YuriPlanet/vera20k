//! Reader controls for the Phobos slave keys.

use super::*;
use crate::rules::ini_parser::IniFile;

fn read(section_ini: &str) -> PhobosSlaved {
    let ini = IniFile::from_str(&format!("[SLAV]\n{section_ini}"));
    PhobosSlaved::from_section(ini.section_or_empty("SLAV"))
}

#[test]
fn the_slave_keys_default_and_parse() {
    let defaults = read("");
    assert_eq!(
        defaults.owner_when_master_killed,
        SlaveChangeOwnerType::Killer
    );
    assert_eq!(defaults.slaves_free_sound, None);
    assert!(defaults.is_default());

    let suicide = read("Slaved.OwnerWhenMasterKilled=SUICIDE\nSlavesFreeSound=SLAVSND\n");
    assert_eq!(
        suicide.owner_when_master_killed,
        SlaveChangeOwnerType::Suicide
    );
    assert_eq!(suicide.slaves_free_sound, Some(Some("SLAVSND".to_string())));

    // `NullableIdx`: an authored `none` is explicit silence, not a fallback.
    for clearing in ["none", "<none>"] {
        let value = read(&format!("SlavesFreeSound={clearing}\n"));
        assert_eq!(value.slaves_free_sound, Some(None), "{clearing}");
    }

    for (raw, expected) in [
        ("master", SlaveChangeOwnerType::Master),
        ("neutral", SlaveChangeOwnerType::Neutral),
        ("killer", SlaveChangeOwnerType::Killer),
    ] {
        let value = read(&format!("Slaved.OwnerWhenMasterKilled={raw}\n"));
        assert_eq!(value.owner_when_master_killed, expected, "{raw}");
    }
    // An unknown spelling keeps the constructor value.
    let unknown = read("Slaved.OwnerWhenMasterKilled=nonsense\n");
    assert_eq!(
        unknown.owner_when_master_killed,
        SlaveChangeOwnerType::Killer
    );
}
