use super::{InsigniaTypeCatalog, PhobosInsignia, normalize_insignia_image};
use crate::rules::ini_parser::{IniFile, IniSection};

fn catalog(text: &str) -> InsigniaTypeCatalog {
    let mut catalog = InsigniaTypeCatalog::default();
    catalog.apply_pass(&IniFile::from_str(text));
    catalog
}

fn section(text: &str) -> IniSection {
    IniFile::from_str(text).section("TEST").unwrap().clone()
}

fn apply(
    insignia: &mut PhobosInsignia,
    section: &IniSection,
    catalog: Option<&InsigniaTypeCatalog>,
    weapon_count: i32,
    gunner: bool,
) {
    insignia.apply_pass(section, catalog, weapon_count, gunner);
}

#[test]
fn insignia_type_defaults_and_promotable_overrides() {
    let catalog = catalog(
        "[InsigniaTypes]\n0=PLAIN\n1=FULL\n\
         [PLAIN]\n\
         [FULL]\nInsignia=shared\nInsignia.Veteran=vet\n\
         InsigniaFrame=4\nInsigniaFrame.Elite=7\n",
    );
    let plain = catalog.entry("plain").expect("case-insensitive entry");
    assert!(plain.images.iter().all(Vec::is_empty));
    assert_eq!(plain.frames, [-1, -1, -1]);

    let full = catalog.entry("FULL").expect("entry");
    assert_eq!(full.images[0], ["shared.shp"], "the common key applies");
    assert_eq!(
        full.images[1],
        ["shared.shp", "vet.shp"],
        "the common key applies, then Veteran appends its own read"
    );
    assert_eq!(full.images[2], ["shared.shp"]);
    assert_eq!(full.frames, [4, 4, 7]);
}

#[test]
fn insignia_images_apply_the_shp_suffix_case_insensitively() {
    assert_eq!(normalize_insignia_image("RANK"), "RANK.shp");
    assert_eq!(normalize_insignia_image("RANK.SHP"), "RANK.SHP");
    assert_eq!(normalize_insignia_image("Rank.shp"), "Rank.shp");
}

#[test]
fn later_passes_append_entries_and_keep_previous_values() {
    let mut catalog = catalog("[InsigniaTypes]\n0=FIRST\n[FIRST]\nInsignia=a\nInsigniaFrame=1\n");
    catalog.apply_pass(&IniFile::from_str(
        "[InsigniaTypes]\n0=SECOND\n1=FIRST\n[SECOND]\nInsignia=b\n[FIRST]\n",
    ));
    assert_eq!(catalog.entries().len(), 2);
    let first = catalog.entry("FIRST").unwrap();
    assert_eq!(
        first.images[0],
        ["a.shp"],
        "an absent key keeps the earlier pass's pointer"
    );
    assert_eq!(first.frames, [1, 1, 1], "absent keys keep the earlier pass");
    assert_eq!(catalog.entry("SECOND").unwrap().images[0], ["b.shp"]);
}

#[test]
fn catalog_keeps_every_applied_image_so_a_failed_load_falls_back() {
    let mut catalog = catalog("[InsigniaTypes]\n0=TYPE\n[TYPE]\nInsignia=loaded\n");
    catalog.apply_pass(&IniFile::from_str("[TYPE]\nInsignia=missing\n"));
    assert_eq!(
        catalog.entry("TYPE").unwrap().images[0],
        ["loaded.shp", "missing.shp"],
        "the SHP reader assigns only on a successful load"
    );
    catalog.apply_pass(&IniFile::from_str("[TYPE]\nInsignia.Elite=lost\n"));
    let entry = catalog.entry("TYPE").unwrap();
    assert_eq!(entry.images[2], ["loaded.shp", "missing.shp", "lost.shp"]);
    assert_eq!(
        entry.images[1],
        ["loaded.shp", "missing.shp"],
        "rank keys only append their own history"
    );
}

#[test]
fn insignia_keys_accumulate_pass_by_pass() {
    let mut insignia = PhobosInsignia::default();
    apply(
        &mut insignia,
        &section(
            "[TEST]\nInsignia=rank\nInsignia.Veteran=vet\nInsigniaFrame=2\n\
             InsigniaFrames=1,2,3\nInsignia.ShowEnemy=yes\n\
             WeaponCount=2\nPassengers=1\n\
             Insignia.Weapon1=gun1\nInsigniaFrame.Weapon1=8\nInsigniaFrames.Weapon1=4,5,6\n\
             Insignia.Weapon2.Rookie=gun2r\n\
             Insignia.Passengers0=p0\nInsigniaFrame.Passengers1=9\n",
        ),
        None,
        2,
        true,
    );
    assert_eq!(insignia.base.images[0], ["rank.shp"]);
    assert_eq!(insignia.base.images[1], ["rank.shp", "vet.shp"]);
    assert_eq!(insignia.base.images[2], ["rank.shp"]);
    assert_eq!(insignia.base.frames, [2, 2, 2]);
    assert_eq!(insignia.base.frame_lists, [1, 2, 3]);
    assert_eq!(insignia.show_enemy, Some(true));

    assert_eq!(insignia.weapons.len(), 2, "Weapon1..WeaponN");
    assert_eq!(insignia.weapons[0].images[0], ["gun1.shp"]);
    assert_eq!(insignia.weapons[0].frames[0], 8);
    assert_eq!(insignia.weapons[0].frame_lists, [4, 5, 6]);
    assert_eq!(insignia.weapons[1].images[0], ["gun2r.shp"]);
    assert!(insignia.weapons[1].images[1].is_empty());

    assert_eq!(insignia.passengers.len(), 2, "Passengers0..PassengersN");
    assert_eq!(insignia.passengers[0].images[0], ["p0.shp"]);
    assert_eq!(insignia.passengers[1].frames, [9, 9, 9]);

    // A later layer without the keys changes nothing.
    apply(&mut insignia, &section("[TEST]\nSpeed=4\n"), None, 2, true);
    assert_eq!(insignia.base.images[0], ["rank.shp"]);
    assert_eq!(insignia.base.frames, [2, 2, 2]);
    assert_eq!(insignia.weapons[0].images[0], ["gun1.shp"]);

    // A later layer naming a file appends to the same slot; the earlier name
    // stays as the fallback for a failed load.
    apply(
        &mut insignia,
        &section("[TEST]\nInsignia=missing\n"),
        None,
        2,
        true,
    );
    assert_eq!(insignia.base.images[0], ["rank.shp", "missing.shp"]);
}

#[test]
fn insignia_type_resolves_per_pass_and_falls_back_to_direct_keys() {
    let catalog = catalog("[InsigniaTypes]\n0=SOME\n[SOME]\nInsignia=typed\nInsigniaFrame=6\n");
    let mut insignia = PhobosInsignia::default();
    apply(
        &mut insignia,
        &section(
            "[TEST]\nInsigniaType=SOME\nInsignia=direct\nInsigniaFrames=1,2,3\n\
             WeaponCount=1\nInsignia.Weapon1=directgun\nInsigniaType.Weapon1=SOMEGUN\n",
        ),
        Some(&catalog),
        1,
        true,
    );
    assert_eq!(
        insignia.base.images[0],
        ["typed.shp"],
        "a resolved InsigniaType overrides the direct keys"
    );
    assert_eq!(insignia.base.frames, [6, 6, 6]);
    assert_eq!(
        insignia.base.frame_lists,
        [-1, -1, -1],
        "InsigniaType holds no frame vector and clears the direct one"
    );
    assert_eq!(
        insignia.weapons[0].images[0],
        ["directgun.shp"],
        "an unknown InsigniaType.Weapon1 falls back to the direct keys"
    );

    // The next layer has no InsigniaType, so it reads the direct keys again on
    // the retained Promotable: the type's names stay as fallbacks and the
    // type's frame survives without an InsigniaFrame key.
    apply(
        &mut insignia,
        &section("[TEST]\nInsignia=later\n"),
        Some(&catalog),
        1,
        true,
    );
    assert_eq!(insignia.base.images[0], ["typed.shp", "later.shp"]);
    assert_eq!(insignia.base.frames, [6, 6, 6]);

    // An unknown type name is not a resolution: the direct keys read normally.
    apply(
        &mut insignia,
        &section("[TEST]\nInsigniaType=UNKNOWN\nInsignia=direct2\nInsigniaFrame=9\n"),
        Some(&catalog),
        1,
        true,
    );
    assert_eq!(
        insignia.base.images[0],
        ["typed.shp", "later.shp", "direct2.shp"]
    );
    assert_eq!(insignia.base.frames, [9, 9, 9]);
}

#[test]
fn an_entry_named_none_resolves_like_any_other_name() {
    let catalog = catalog("[InsigniaTypes]\n0=none\n[none]\nInsignia=x\n");
    let mut insignia = PhobosInsignia::default();
    apply(
        &mut insignia,
        &section("[TEST]\nInsigniaType=none\nInsignia=direct\n"),
        Some(&catalog),
        0,
        false,
    );
    assert_eq!(
        insignia.base.images[0],
        ["x.shp"],
        "Enumerable::Find has no none literal"
    );
}

#[test]
fn a_resolved_type_copies_the_entrys_load_history() {
    let mut catalog = catalog("[InsigniaTypes]\n0=TYPE\n[TYPE]\nInsignia=loaded\n");
    let mut insignia = PhobosInsignia::default();
    apply(
        &mut insignia,
        &section("[TEST]\nInsigniaType=TYPE\n"),
        Some(&catalog),
        0,
        false,
    );
    assert_eq!(insignia.base.images[0], ["loaded.shp"]);

    catalog.apply_pass(&IniFile::from_str("[TYPE]\nInsignia=missing\n"));
    apply(
        &mut insignia,
        &section("[TEST]\nInsigniaType=TYPE\n"),
        Some(&catalog),
        0,
        false,
    );
    assert_eq!(
        insignia.base.images[0],
        ["loaded.shp", "missing.shp"],
        "the type assignment copies the entry's retained pointers"
    );
}

#[test]
fn weapon_variants_exist_only_for_gunner_types() {
    let mut insignia = PhobosInsignia::default();
    apply(
        &mut insignia,
        &section("[TEST]\nWeaponCount=2\nInsignia.Weapon1=gun\n"),
        None,
        2,
        false,
    );
    assert!(insignia.weapons.is_empty(), "Gunner=no skips the block");
    apply(
        &mut insignia,
        &section("[TEST]\nWeaponCount=2\nInsignia.Weapon1=gun\n"),
        None,
        2,
        true,
    );
    assert_eq!(insignia.weapons.len(), 2);
    assert_eq!(insignia.weapons[0].images[0], ["gun.shp"]);
}

#[test]
fn selection_merges_passengers_then_weapons() {
    let mut insignia = PhobosInsignia::default();
    apply(
        &mut insignia,
        &section(
            "[TEST]\nInsignia=base\nInsigniaFrame=1\nInsigniaFrames=1,2,3\n\
             WeaponCount=1\nPassengers=1\n\
             Insignia.Weapon1=gun\nInsigniaFrame.Weapon1=7\n\
             Insignia.Passengers1=carry\nInsigniaFrames.Passengers1=4,4,4\n\
             InsigniaFrame.Passengers1=2\n",
        ),
        None,
        1,
        true,
    );
    let base = insignia.select(None, None);
    assert_eq!(base.layers.len(), 1);
    assert_eq!(base.layers[0][0], ["base.shp"]);
    assert_eq!(base.frames, [1, 1, 1]);
    assert_eq!(base.frame_lists, [1, 2, 3]);

    let carried = insignia.select(None, Some(1));
    assert_eq!(carried.layers.len(), 2);
    assert_eq!(carried.layers[1][0], ["carry.shp"]);
    assert_eq!(carried.frames, [2, 2, 2]);
    assert_eq!(carried.frame_lists, [4, 4, 4]);

    let gunned = insignia.select(Some(0), Some(1));
    assert_eq!(gunned.layers[0][0], ["base.shp"]);
    assert_eq!(gunned.layers[1][0], ["carry.shp"]);
    assert_eq!(
        gunned.layers[2][0],
        ["gun.shp"],
        "layers apply base, passenger, then weapon"
    );
    assert_eq!(gunned.frames, [7, 7, 7]);
    assert_eq!(
        gunned.frame_lists,
        [4, 4, 4],
        "a variant without InsigniaFrames keeps the merged vector"
    );
}

#[test]
fn frame_precedence_matches_draw_insignia() {
    let mut insignia = PhobosInsignia::default();
    apply(
        &mut insignia,
        &section("[TEST]\nInsigniaFrames=3,4,5\nInsigniaFrame.Veteran=8\n"),
        None,
        0,
        false,
    );
    let selection = insignia.select(None, None);
    assert_eq!(
        selection.frame(0, true),
        Some(3),
        "custom rookie uses InsigniaFrames.X"
    );
    assert_eq!(
        selection.frame(1, true),
        Some(8),
        "the single Veteran frame wins over the vector"
    );
    assert_eq!(selection.frame(2, true), Some(5), "Elite falls back to Z");
    assert_eq!(
        selection.frame(2, false),
        Some(5),
        "the vector wins over the stock default"
    );

    let bare = PhobosInsignia::default().select(None, None);
    assert_eq!(bare.frame(0, true), Some(0), "custom default frame 0");
    assert_eq!(bare.frame(0, false), None, "stock rookie draws nothing");
    assert_eq!(bare.frame(1, false), Some(14), "stock Veteran frame 14");
    assert_eq!(bare.frame(2, false), Some(15), "stock Elite frame 15");
}

/// The production rules owner, not the section reader alone: Phobos reads the
/// insignia keys on every reached TechnoType pass, so `RuleSet` carries the
/// accumulated slots.
mod layered_rules {
    use crate::rules::ini_parser::IniFile;
    use crate::rules::native_processing::{RulesLayerKind, RulesLayerStack};
    use crate::rules::ruleset::RuleSet;

    fn layered(root: &str, layer: &str) -> RuleSet {
        let mut stack = RulesLayerStack::new(IniFile::from_str(root));
        stack.push(RulesLayerKind::Scenario, IniFile::from_str(layer));
        RuleSet::from_processed_rules(&stack.process().unwrap()).unwrap()
    }

    #[test]
    fn direct_keys_accumulate_across_layers() {
        let rules = layered(
            "[InfantryTypes]\n0=GUY\n[GUY]\nStrength=100\nInsignia=one\nInsigniaFrame=3\n",
            "[GUY]\nInsignia=two\n",
        );
        let insignia = &rules.object("GUY").unwrap().phobos_insignia;
        assert_eq!(insignia.base.images[0], ["one.shp", "two.shp"]);
        assert_eq!(insignia.base.frames, [3, 3, 3]);
    }

    #[test]
    fn a_scenario_insignia_type_resolves_before_its_type_read() {
        let rules = layered(
            "[InfantryTypes]\n0=GUY\n[GUY]\nStrength=100\nInsignia=one\n",
            "[InsigniaTypes]\n0=MAPTYPE\n[MAPTYPE]\nInsignia=typed\nInsigniaFrame=6\n\
             [GUY]\nInsigniaType=MAPTYPE\n",
        );
        let insignia = &rules.object("GUY").unwrap().phobos_insignia;
        assert_eq!(
            insignia.base.images[0],
            ["typed.shp"],
            "the type replaces the earlier direct name"
        );
        assert_eq!(insignia.base.frames, [6, 6, 6]);
        assert!(rules.insignia_types().entry("MAPTYPE").is_some());
    }

    #[test]
    fn a_later_layer_without_the_type_reads_direct_keys_on_the_retained_slots() {
        let rules = layered(
            "[InfantryTypes]\n0=GUY\n[GUY]\nStrength=100\nInsigniaType=TYPE\n\
             [InsigniaTypes]\n0=TYPE\n[TYPE]\nInsignia=typed\nInsigniaFrame=6\n",
            "[GUY]\nInsignia=later\n",
        );
        let insignia = &rules.object("GUY").unwrap().phobos_insignia;
        assert_eq!(
            insignia.base.images[0],
            ["typed.shp", "later.shp"],
            "the type's names stay as fallbacks under the later direct read"
        );
        assert_eq!(insignia.base.frames, [6, 6, 6]);
    }
}
