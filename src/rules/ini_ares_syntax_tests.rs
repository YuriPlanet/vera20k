//! Ares's INI syntax, which a thread that reads the Ares way loads with
//! (`ares_switch::ares_way`): each test opens that scope first.
use super::super::IniFile;

fn physical(ini: &IniFile, ordinal: usize) -> Vec<(&str, &str)> {
    ini.sections[ordinal].physical_entries().collect()
}

#[test]
fn fresh_load_renames_plus_keys_and_lists_a_repeated_key_last() {
    let _ares = crate::rules::ares_switch::read_ares_way(true);
    let ini = IniFile::from_str("[BuildingTypes]\n+=GAPOWR\n+=GACNST\n[General]\nA=1\nB=2\nA=3\n");
    assert_eq!(
        physical(&ini, 0),
        [("var_0", "GAPOWR"), ("var_1", "GACNST")]
    );
    // INIClass_Parse_Override deletes the first A; the later entry is last.
    assert_eq!(physical(&ini, 1), [("B", "2"), ("A", "3")]);
    assert_eq!(ini.section("General").unwrap().get_for_test("A"), Some("3"));
}

#[test]
fn fresh_load_copies_a_base_into_a_separate_section_of_the_childs_name() {
    let _ares = crate::rules::ares_switch::read_ares_way(true);
    let ini = IniFile::from_str(
        "[Base]\nStrength=100\nSpeed=5\n[Child]:[Base]\nStrength=200\n\
         [Early]:[Late]\nOwn=1\n[Late]\nOwn=2\n",
    );
    // `_CopySection2` writes before the child is linked: PutString appends a
    // section holding the copy, and the child's own entries follow apart.
    assert_eq!(
        ini.section_names(),
        ["Base", "Child", "Child", "Early", "Late"]
    );
    assert_eq!(physical(&ini, 1), [("Strength", "100"), ("Speed", "5")]);
    assert_eq!(physical(&ini, 2), [("Strength", "200")]);
    // A base not parsed yet copies nothing.
    assert_eq!(physical(&ini, 3), [("Own", "1")]);
}

#[test]
fn merge_load_inherits_from_the_including_file_and_continues_its_counter() {
    let _ares = crate::rules::ares_switch::read_ares_way(true);
    let mut ini = IniFile::from_str("[Base]\nStrength=100\nSpeed=5\n[BuildingTypes]\n+=GAPOWR\n");
    // The first header is read before the hooked loop, so it never inherits.
    // The unterminated last line is still put.
    let listed = ini.merge_load(
        b"[First]:[Base]\nOwn=1\n[Child]:[Base]\nStrength=200\n[BuildingTypes]\n+=NAPOWR",
        "BuildingTypes",
    );
    // The file's own entries of the named section, for its include list.
    assert_eq!(
        listed.physical_entries().collect::<Vec<_>>(),
        [("var_1", "NAPOWR")]
    );
    assert_eq!(
        ini.section_names(),
        ["Base", "BuildingTypes", "First", "Child"]
    );
    assert_eq!(physical(&ini, 2), [("Own", "1")]);
    assert_eq!(physical(&ini, 3), [("Speed", "5"), ("Strength", "200")]);
    assert_eq!(
        physical(&ini, 1),
        [("var_0", "GAPOWR"), ("var_1", "NAPOWR")]
    );
    let child = ini.section("Child").unwrap();
    assert_eq!(child.get_for_test("Strength"), Some("200"));
    assert_eq!(child.get_for_test("Speed"), Some("5"));
}

#[test]
fn an_included_file_lists_its_unterminated_last_line_and_inherits_from_the_root() {
    let _ares = crate::rules::ares_switch::read_ares_way(true);
    use super::super::directives::{DirectiveOptions, process_directives};
    let files = [
        (
            "a.ini",
            "[Head]\nX=1\n[Unit]:[Base]\nOwn=a\n[#include]\n+=b.ini",
        ),
        ("b.ini", "[Unit]\nLate=b\n"),
    ];
    let expanded = process_directives(
        IniFile::from_str("[#include]\n+=a.ini\n[Base]\nShared=root\n"),
        "root.ini",
        |name| {
            Ok(files
                .iter()
                .find(|(file, _)| *file == name)
                .map(|(_, body)| body.as_bytes().to_vec()))
        },
        DirectiveOptions::default(),
    )
    .unwrap();
    assert!(expanded.diagnostics.is_empty());
    let unit = expanded.ini.section("Unit").unwrap();
    assert_eq!(
        unit.physical_entries().collect::<Vec<_>>(),
        [("Shared", "root"), ("Own", "a"), ("Late", "b")]
    );
}

#[test]
fn mod_rules_allocate_plus_registries_inherit_across_includes_and_take_later_keys() {
    let _ares = crate::rules::ares_switch::read_ares_way(true);
    use crate::map::source::test_support::TestDirectory;
    use crate::rules::retail_sources::{load_startup_rules, select_mode_ini};
    let dir = TestDirectory::new("ares-ini-syntax-rules");
    dir.write(
        "rulesmd.ini",
        b"[#include]\n+=units.ini\n[VehicleTypes]\n+=MTNK\n[MTNK]\nCost=700\nSight=6\nCost=800\n",
    );
    dir.write(
        "units.ini",
        b"[VehicleTypes]\n+=TANK2\n[TANK2]:[MTNK]\nSight=9\n",
    );
    dir.write("artmd.ini", b"[MTNK]\nVoxel=yes\n");
    let assets = crate::assets::asset_manager::AssetManager::from_loose_root_for_test(dir.path());
    let (_, _, mut owner) = load_startup_rules(&assets, Default::default(), Default::default())
        .unwrap()
        .into_parts();
    let mode = select_mode_ini(&assets, None).unwrap();
    let (rules, ..) = owner
        .load_scenario(
            crate::rules::process_owner::NativeScenarioRulesPrefix::NonCampaign(mode.as_ref()),
            &IniFile::empty(),
        )
        .unwrap()
        .into_parts();
    assert_eq!(rules.vehicle_ids, ["MTNK", "TANK2"]);
    assert_eq!(rules.object("MTNK").unwrap().cost, 800);
    let inherited = rules.object("TANK2").unwrap();
    assert_eq!((inherited.cost, inherited.sight), (800, 9));
}
