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
fn fresh_load_keeps_inherited_defaults_and_authored_values_in_one_section() {
    let _ares = crate::rules::ares_switch::read_ares_way(true);
    let ini = IniFile::from_str(
        "[Base]\nStrength=100\nSpeed=5\n[Child]:[Base]\nStrength=200\n\
         [Early]:[Late]\nOwn=1\n[Late]\nOwn=2\n",
    );
    assert_eq!(ini.section_names(), ["Base", "Child", "Early", "Late"]);
    assert_eq!(physical(&ini, 1), [("Speed", "5"), ("Strength", "200")]);
    // A base not parsed yet copies nothing.
    assert_eq!(physical(&ini, 2), [("Own", "1")]);
}

#[test]
fn base_copies_stop_at_the_copy_budget() {
    let _ares = crate::rules::ares_switch::read_ares_way(true);
    // A 40 KB base and 200 one-line children: about 8 MB of copies from
    // 43 KB of text, past the 64-fold budget of 2.75 MB.
    let mut text = String::from("[Big]\n");
    for key in 0..100 {
        text.push_str(&format!("K{key}={}\n", "v".repeat(400)));
    }
    for child in 0..200 {
        text.push_str(&format!("[C{child}]:[Big]\n"));
    }
    let ini = IniFile::from_str(&text);
    assert_eq!(
        ini.section("C0").map(|c| c.physical_entries().count()),
        Some(100)
    );
    // Children past the budget copy nothing, so they hold no section.
    assert!(ini.section("C199").is_none());
}

#[test]
fn merge_load_inherits_from_the_including_file_and_continues_its_counter() {
    let _ares = crate::rules::ares_switch::read_ares_way(true);
    let mut ini = IniFile::from_str("[Base]\nStrength=100\nSpeed=5\n[BuildingTypes]\n+=GAPOWR\n");
    // VERA accepts inheritance on the first included header. The Ares
    // merge still puts an entry on the unterminated last line.
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
    assert_eq!(
        physical(&ini, 2),
        [("Strength", "100"), ("Speed", "5"), ("Own", "1")]
    );
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
fn stock_children_inherit_the_value_the_parent_reads_for_repeated_keys() {
    let _stock = crate::rules::ares_switch::read_ares_way(false);
    let ini = IniFile::from_str(
        "[Base]\nStrength=100\nStrength=200\n[Child]:[Base]\nSpeed=5\n\
         [Other]\nStrength=300\nStrength=400\nStrength=500\n[OtherChild]:[Other]\n",
    );
    for (parent, child) in [("Base", "Child"), ("Other", "OtherChild")] {
        let parent_strength = ini.section(parent).unwrap().read_int("Strength", 0);
        assert_eq!(
            ini.section(child).unwrap().read_int("Strength", 0),
            parent_strength,
            "{child} must inherit the value selected in {parent}",
        );
    }
}

#[test]
fn builtin_ini_syntax_reads_the_first_included_child_with_ares_off() {
    use super::super::directives::{DirectiveOptions, process_directives};
    let _stock = crate::rules::ares_switch::read_ares_way(false);
    let expanded = process_directives(
        IniFile::from_str(
            "[#include]\n0=units.ini\n[Base]\nSpeed=5\nCost=50\n[Types]\n+=Base\n\
             [VehicleTypes]\n0=Old\n1=Keep\n",
        ),
        "root.ini",
        |_| {
            Ok(Some(
                b"[Child]:[Base]\nCost=75\nCost=100\n[Types]\n+=Child\n[VehicleTypes]\n0=New\n"
                    .to_vec(),
            ))
        },
        DirectiveOptions::default(),
    )
    .unwrap();
    let child = expanded.ini.section("Child").unwrap();
    assert_eq!(child.get_for_test("Speed"), Some("5"));
    let types = expanded.ini.section("Types").unwrap();
    assert_eq!(
        types.physical_entries().collect::<Vec<_>>(),
        [("var_0", "Base"), ("var_1", "Child")]
    );
    assert_eq!(
        expanded.ini.section("VehicleTypes").unwrap().registry_ids(),
        ["New", "Keep"]
    );
    // The stock CRC lookup still selects the same physical duplicate as a
    // plain stock section with these keys and their existing ordering.
    assert_eq!(child.get_for_test("Cost"), Some("100"));
}

#[test]
fn a_merge_load_follows_include_entries_inherited_from_another_section() {
    use super::super::directives::{DirectiveOptions, process_directives};
    let _ares = crate::rules::ares_switch::read_ares_way(true);
    let expanded = process_directives(
        IniFile::from_str("[#include]\n0=first.ini\n1=carrier.ini\n[Parent]\n0=nested.ini\n"),
        "root.ini",
        |name| {
            let bytes: &[u8] = match name {
                "first.ini" => b"[Seen]\nFirst=yes\n",
                "carrier.ini" => b"[Seen]\nCarrier=yes\n[#include]:[Parent]\n",
                "nested.ini" => b"[Seen]\nNested=yes\n",
                _ => return Ok(None),
            };
            Ok(Some(bytes.to_vec()))
        },
        DirectiveOptions::default(),
    )
    .unwrap();
    assert!(expanded.diagnostics.is_empty());
    assert_eq!(
        expanded.ini.section("Seen").unwrap().get_for_test("Nested"),
        Some("yes")
    );
}

#[test]
fn startup_ini_numbering_continues_from_the_rules_include_into_art() {
    use crate::map::source::test_support::TestDirectory;
    use crate::rules::retail_sources::RetailRulesSources;
    let dir = TestDirectory::new("ares-ini-numbered-sources");
    dir.write("rulesmd.ini", b"[#include]\n0=units.ini\n[Types]\n+=Root\n");
    dir.write("units.ini", b"[Types]\n+=Included\n");
    dir.write("artmd.ini", b"[Types]\nvar_0=Authored\n+=Art\n");
    let assets =
        crate::assets::asset_manager::AssetManager::from_loose_root_with_ares_for_test(dir.path());
    let selected = RetailRulesSources::select(&assets).unwrap();
    let types = selected.artmd.ini.section("Types").unwrap();
    assert_eq!(
        types.physical_entries().collect::<Vec<_>>(),
        [("var_0", "Authored"), ("var_2", "Art")]
    );
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
    let assets =
        crate::assets::asset_manager::AssetManager::from_loose_root_with_ares_for_test(dir.path());
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
