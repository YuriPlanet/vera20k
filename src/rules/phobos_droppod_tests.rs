//! `DroppodTypeClass::LoadFromINI` (`New/Type/Affiliated/DroppodTypeClass.cpp:7-21`).

use super::PhobosDropPodRules;
use crate::rules::ini_parser::IniFile;
use crate::util::native_x87::NativeF64Bits;

fn read(ini: &str, section: &str) -> PhobosDropPodRules {
    let file = IniFile::from_str(ini);
    PhobosDropPodRules::from_section(file.section_or_empty(section))
}

#[test]
fn defaults_are_the_fallbacks_and_the_constructor_values() {
    let set = PhobosDropPodRules::default();
    assert_eq!(set.angle, None);
    assert_eq!(set.atmosphere_entry, None);
    assert_eq!(set.ground_anims, None);
    assert_eq!(set.air_image, None);
    assert_eq!(set.height, None);
    assert_eq!(set.puff, None);
    assert_eq!(set.speed, None);
    assert_eq!(set.trailer, None);
    assert_eq!(set.trailer_spawn_delay, 6);
    assert!(!set.trailer_attached);
    assert_eq!(set.weapon, None);
    assert!(!set.weapon_hit_land_only);
}

#[test]
fn reads_the_twelve_keys() {
    let set = read(
        "[POD]\nDropPod.Angle=0.5\nDropPod.AtmosphereEntry=ENTRY\n\
         DropPod.GroundAnim=LAND,ALT\nDropPod.AirImage=POD2.SHP\nDropPod.Height=700\n\
         DropPod.Puff=PUFF\nDropPod.Speed=25\nDropPod.Trailer=TRAIL\n\
         DropPod.Trailer.SpawnDelay=9\nDropPod.Trailer.Attached=yes\n\
         DropPod.Weapon=PODGUN\nDropPod.Weapon.HitLandOnly=yes\n",
        "POD",
    );
    assert_eq!(set.angle, Some(NativeF64Bits::from_bits(0.5f64.to_bits())));
    assert_eq!(set.atmosphere_entry.as_deref(), Some("ENTRY"));
    assert_eq!(
        set.ground_anims,
        Some(["LAND".to_owned(), "ALT".to_owned()])
    );
    assert_eq!(set.air_image.as_deref(), Some("POD2.SHP"));
    assert_eq!(set.height, Some(700));
    assert_eq!(set.puff.as_deref(), Some("PUFF"));
    assert_eq!(set.speed, Some(25));
    assert_eq!(set.trailer.as_deref(), Some("TRAIL"));
    assert_eq!(set.trailer_spawn_delay, 9);
    assert!(set.trailer_attached);
    assert_eq!(set.weapon.as_deref(), Some("PODGUN"));
    assert!(set.weapon_hit_land_only);
}

#[test]
fn clearing_tokens_reset_a_nullable_to_the_global_fallback() {
    for value in ["<none>", "none", "<default>"] {
        let ini = format!(
            "[P]\nDropPod.Angle={value}\nDropPod.Height={value}\nDropPod.Trailer={value}\n\
             DropPod.Weapon={value}\nDropPod.AtmosphereEntry={value}\nDropPod.Puff={value}\n"
        );
        let set = read(&ini, "P");
        assert_eq!(set.angle, None, "angle [{value}]");
        assert_eq!(set.height, None, "height [{value}]");
        assert_eq!(set.trailer, None, "trailer [{value}]");
        assert_eq!(set.weapon, None, "weapon [{value}]");
        assert_eq!(set.atmosphere_entry, None, "entry [{value}]");
        assert_eq!(set.puff, None, "puff [{value}]");
    }
}

#[test]
fn a_single_ground_anim_serves_both_slots_and_an_empty_list_keeps_the_global() {
    let one = read("[P]\nDropPod.GroundAnim=ONLY\n", "P");
    assert_eq!(
        one.ground_anims,
        Some(["ONLY".to_owned(), "ONLY".to_owned()])
    );
    let extra = read("[P]\nDropPod.GroundAnim=A,B,C\n", "P");
    assert_eq!(
        extra.ground_anims,
        Some(["A".to_owned(), "B".to_owned()]),
        "only the first two entries are read"
    );
    assert_eq!(read("[P]\nDropPod.GroundAnim=\n", "P").ground_anims, None);
    assert_eq!(
        read("[P]\nDropPod.GroundAnim=,,\n", "P").ground_anims,
        None,
        "a list with no names adds nothing"
    );
}

#[test]
fn numeric_reads_keep_the_fallback_on_malformed_text() {
    let set = read(
        "[P]\nDropPod.Height=abc\nDropPod.Speed=xyz\nDropPod.Trailer.SpawnDelay=q\n\
         DropPod.Trailer.Attached=maybe\n",
        "P",
    );
    assert_eq!(set.height, None);
    assert_eq!(set.speed, None);
    assert_eq!(set.trailer_spawn_delay, 6);
    assert!(!set.trailer_attached);
}
