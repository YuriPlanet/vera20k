//! Regression scenarios adapted from mono's IniDirectiveTests, plus VERA's
//! native parser, source-selection and snapshot boundaries.
use super::*;

fn text(ini: &IniFile, section: &str, key: &str) -> Option<String> {
    ini.section(section)?
        .read_name(key, usize::MAX)
        .map(str::to_owned)
}

fn expand(root: &str, files: &[(&str, &str)]) -> ExpandedIni {
    process_directives(
        IniFile::from_str(root),
        "root.ini",
        |name| {
            Ok(files
                .iter()
                .find(|(key, _)| *key == name)
                .filter(|(_, body)| !body.is_empty())
                .map(|(_, body)| IniFile::from_str(body)))
        },
        DirectiveOptions::default(),
    )
    .unwrap()
}

#[test]
fn ini_directives_nested_current_and_later_include_precedence() {
    let root = "[#include]\n10=late.ini\n2=early.ini\n[Unit]\nOwner=root\n";
    let result = expand(
        root,
        &[
            (
                "early.ini",
                "[#include]\n0=base.ini\n[Unit]\nOwner=early\nValue=early\n",
            ),
            ("base.ini", "[Unit]\nOwner=base\nInherited=yes\n"),
            ("late.ini", "[Unit]\nOwner=late\nValue=late\n"),
        ],
    );
    assert!(result.diagnostics.is_empty());
    assert_eq!(text(&result.ini, "Unit", "Owner").as_deref(), Some("root"));
    assert_eq!(text(&result.ini, "Unit", "Value").as_deref(), Some("late"));
    assert_eq!(
        text(&result.ini, "Unit", "Inherited").as_deref(),
        Some("yes")
    );
}

#[test]
fn ini_directives_phobos_section_wins_instead_of_combining_lists() {
    let result = expand(
        "[#include]\n0=missing.ini\n[$Include]\n0=phobos.ini\n",
        &[("phobos.ini", "[Unit]\nValue=phobos\n")],
    );
    assert!(result.diagnostics.is_empty());
    assert_eq!(
        text(&result.ini, "Unit", "Value").as_deref(),
        Some("phobos")
    );
}

#[test]
fn ini_directives_numeric_and_ordinal_key_order_including_ties() {
    let root = "[#include]\nz=z.ini\na=a.ini\n10=ten.ini\n2=two.ini\n01=one.ini\n1=tie.ini\n2147483648=overflow.ini\n";
    let mut reads = Vec::new();
    process_directives(
        IniFile::from_str(root),
        "root.ini",
        |name| {
            reads.push(name.to_owned());
            Ok(Some(IniFile::from_str("[Unit]\nValue=yes\n")))
        },
        DirectiveOptions::default(),
    )
    .unwrap();
    assert_eq!(
        reads,
        [
            "one.ini",
            "tie.ini",
            "two.ini",
            "ten.ini",
            "overflow.ini",
            "a.ini",
            "z.ini"
        ]
    );
}

#[test]
fn ini_directives_shared_dag_child_is_revisited_in_order() {
    let root = "[#include]\n0=left.ini\n1=right.ini\n";
    let files = [
        ("left.ini", "[#include]\n0=shared.ini\n[Unit]\nValue=left\n"),
        (
            "right.ini",
            "[#include]\n0=shared.ini\n[Unit]\nOther=right\n",
        ),
        ("shared.ini", "[Unit]\nValue=shared\n"),
    ];
    let mut reads = Vec::new();
    let result = process_directives(
        IniFile::from_str(root),
        "root.ini",
        |name| {
            reads.push(name.to_owned());
            Ok(Some(IniFile::from_str(
                files.iter().find(|(key, _)| *key == name).unwrap().1,
            )))
        },
        DirectiveOptions::default(),
    )
    .unwrap();
    assert_eq!(reads, ["left.ini", "shared.ini", "right.ini", "shared.ini"]);
    assert_eq!(
        text(&result.ini, "Unit", "Value").as_deref(),
        Some("shared")
    );
    assert!(result.diagnostics.is_empty());
}

#[test]
fn ini_directives_self_reference_normalizes_before_loading() {
    for alias in ["./ROOT.INI", "dir/../root.ini", "dir\\..\\ROOT.INI"] {
        let result = process_directives(
            IniFile::from_str(&format!("[#include]\n0={alias}\n")),
            "root.ini",
            |_| panic!("self reference must not read"),
            DirectiveOptions::default(),
        )
        .unwrap();
        assert_eq!(result.diagnostics.len(), 1);
        assert_eq!(result.diagnostics[0].kind, DiagnosticKind::IncludeCycle);
        assert_eq!(result.diagnostics[0].reference_chain, ["root.ini", alias]);
    }
}

#[test]
fn ini_directives_indirect_cycle_has_chain_and_keeps_valid_content() {
    let result = expand(
        "[#include]\n0=a.ini\n",
        &[
            ("a.ini", "[#include]\n0=b.ini\n[A]\nValue=a\n"),
            ("b.ini", "[#include]\n0=a.ini\n[B]\nValue=b\n"),
        ],
    );
    assert_eq!(result.diagnostics.len(), 1);
    assert_eq!(
        result.diagnostics[0].reference_chain,
        ["root.ini", "a.ini", "b.ini", "a.ini"]
    );
    assert_eq!(text(&result.ini, "A", "Value").as_deref(), Some("a"));
    assert_eq!(text(&result.ini, "B", "Value").as_deref(), Some("b"));
}

#[test]
fn ini_directives_strict_errors_do_not_poison_a_fresh_retry() {
    let root = IniFile::from_str("[#include]\n0=child.ini\n");
    let options = DirectiveOptions {
        strict: true,
        ..Default::default()
    };
    let error = process_directives(
        root.clone(),
        "root.ini",
        |_| Ok(Some(IniFile::from_str("[#include]\n0=child.ini\n"))),
        options,
    )
    .err()
    .unwrap();
    assert_eq!(error.kind, DiagnosticKind::IncludeCycle);
    assert_eq!(error.source_name, "root.ini");
    let result = process_directives(
        root,
        "root.ini",
        |_| Ok(Some(IniFile::from_str("[Unit]\nValue=repaired\n"))),
        options,
    )
    .unwrap();
    assert!(result.diagnostics.is_empty());
    assert_eq!(
        text(&result.ini, "Unit", "Value").as_deref(),
        Some("repaired")
    );
}

#[test]
fn ini_directives_default_and_custom_depth_stop_before_next_read() {
    for limit in [2, 64] {
        let mut reads = 0;
        let result = process_directives(
            IniFile::from_str("[#include]\n0=1.ini\n"),
            "0.ini",
            |name| {
                reads += 1;
                assert!(reads <= limit);
                let next = name.trim_end_matches(".ini").parse::<usize>().unwrap() + 1;
                Ok(Some(IniFile::from_str(&format!(
                    "[#include]\n0={next}.ini\n"
                ))))
            },
            DirectiveOptions {
                max_depth: limit,
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(reads, limit);
        assert_eq!(result.diagnostics.len(), 1);
        assert_eq!(
            result.diagnostics[0].kind,
            DiagnosticKind::IncludeDepthLimit
        );
        assert_eq!(result.diagnostics[0].reference_chain.len(), limit + 2);
    }
}

#[test]
fn ini_directives_missing_empty_and_io_failures_are_distinct_and_continue() {
    for missing in [true, false] {
        let result = process_directives(
            IniFile::from_str("[#include]\n0=bad.ini\n1=good.ini\n"),
            "root.ini",
            |name| {
                if name == "bad.ini" {
                    if missing {
                        Ok(None)
                    } else {
                        Err("original IO reason".into())
                    }
                } else {
                    Ok(Some(IniFile::from_str("[Unit]\nValue=good\n")))
                }
            },
            DirectiveOptions::default(),
        )
        .unwrap();
        assert_eq!(result.diagnostics.len(), 1);
        assert_eq!(
            result.diagnostics[0].kind,
            if missing {
                DiagnosticKind::IncludeMissing
            } else {
                DiagnosticKind::IncludeLoadFailed
            }
        );
        assert_eq!(
            result.diagnostics[0].reference_chain,
            ["root.ini", "bad.ini"]
        );
        if !missing {
            assert!(result.diagnostics[0].message.contains("original IO reason"));
        }
        assert_eq!(text(&result.ini, "Unit", "Value").as_deref(), Some("good"));
    }
}

#[test]
fn ini_directives_strict_missing_and_io_failures_stop_before_following_file() {
    for missing in [true, false] {
        let mut reads = Vec::new();
        let result = process_directives(
            IniFile::from_str("[#include]\n0=bad.ini\n1=good.ini\n"),
            "root.ini",
            |name| {
                reads.push(name.to_owned());
                if missing {
                    Ok(None)
                } else {
                    Err("original IO reason".into())
                }
            },
            DirectiveOptions {
                strict: true,
                ..Default::default()
            },
        );
        assert_eq!(reads, ["bad.ini"]);
        assert!(result.is_err());
    }
}

#[test]
fn ini_directives_child_first_parent_and_grandparent_inheritance() {
    let result = expand(
        "[Child]\n$Inherits=First,Second\nOwn=child\n[First]\n$Inherits=Base\nShared=first\nOwn=parent\n[Second]\nShared=second\nExtra=yes\n[Base]\nDeep=yes\n",
        &[],
    );
    for (key, value) in [
        ("Own", "child"),
        ("Shared", "first"),
        ("Extra", "yes"),
        ("Deep", "yes"),
    ] {
        assert_eq!(text(&result.ini, "Child", key).as_deref(), Some(value));
    }
    assert_eq!(
        text(&result.ini, "Child", INHERITS).as_deref(),
        Some("First,Second")
    );
    assert!(result.diagnostics.is_empty());
}

#[test]
fn ini_directives_inheritance_cycles_do_not_copy_failed_branches() {
    for root in [
        "[A]\n$Inherits=A\nOwnA=yes\n",
        "[A]\n$Inherits=B\nOwnA=yes\n[B]\n$Inherits=A\nOwnB=yes\n",
    ] {
        let result = expand(root, &[]);
        assert!(!result.diagnostics.is_empty());
        assert!(
            result
                .diagnostics
                .iter()
                .all(|d| d.kind == DiagnosticKind::InheritanceCycle)
        );
        assert_eq!(text(&result.ini, "A", "OwnA").as_deref(), Some("yes"));
        assert!(text(&result.ini, "A", "OwnB").is_none());
        assert!(text(&result.ini, "B", "OwnA").is_none());
    }
}

#[test]
fn ini_directives_strict_inheritance_cycle_has_section_chain() {
    let error = process_directives(
        IniFile::from_str("[A]\n$Inherits=B\n[B]\n$Inherits=A\n"),
        "root.ini",
        |_| panic!("no include"),
        DirectiveOptions {
            strict: true,
            ..Default::default()
        },
    )
    .err()
    .unwrap();
    assert_eq!(error.kind, DiagnosticKind::InheritanceCycle);
    assert_eq!(error.reference_chain, ["A", "B", "A"]);
}

#[test]
fn ini_directives_inheritance_depth_cuts_only_unresolved_branch() {
    let root = "[Child]\n$Inherits=Parent\nOwn=yes\n[Parent]\n$Inherits=Base\nParentValue=yes\n[Base]\nDeep=yes\n";
    let result = process_directives(
        IniFile::from_str(root),
        "root.ini",
        |_| panic!("no include"),
        DirectiveOptions {
            max_depth: 1,
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(result.diagnostics.len(), 1);
    assert_eq!(
        result.diagnostics[0].reference_chain,
        ["Child", "Parent", "Base"]
    );
    assert_eq!(
        result.diagnostics[0].kind,
        DiagnosticKind::InheritanceDepthLimit
    );
    assert!(text(&result.ini, "Child", "ParentValue").is_none());
    assert_eq!(text(&result.ini, "Child", "Own").as_deref(), Some("yes"));
}

#[test]
fn ini_directives_missing_parents_remain_case_sensitive_and_keep_valid_parents() {
    let result = expand(
        "[Child]\n$Inherits=parent,Good\nOwn=yes\n[Parent]\nValue=no\n[Good]\nExtra=yes\n",
        &[],
    );
    assert_eq!(result.diagnostics.len(), 1);
    assert_eq!(
        result.diagnostics[0].kind,
        DiagnosticKind::InheritanceParentMissing
    );
    assert_eq!(result.diagnostics[0].reference_chain, ["Child", "parent"]);
    assert!(text(&result.ini, "Child", "Value").is_none());
    assert_eq!(text(&result.ini, "Child", "Extra").as_deref(), Some("yes"));
}

#[test]
fn ini_directives_expand_includes_before_inheritance() {
    let result = expand(
        "[#include]\n0=base.ini\n[Child]\n$Inherits=Base\nOwn=child\n",
        &[("base.ini", "[Base]\nOwn=base\nOther=yes\n")],
    );
    assert!(result.diagnostics.is_empty());
    assert_eq!(text(&result.ini, "Child", "Own").as_deref(), Some("child"));
    assert_eq!(text(&result.ini, "Child", "Other").as_deref(), Some("yes"));
}

#[test]
fn ini_directives_completed_parents_reuse_without_depth_descent() {
    let result = process_directives(
        IniFile::from_str(
            "[Base]\nDeep=yes\n[Parent]\n$Inherits=Base\n[Child]\n$Inherits=Parent\n",
        ),
        "root.ini",
        |_| panic!("no include"),
        DirectiveOptions {
            max_depth: 1,
            ..Default::default()
        },
    )
    .unwrap();
    assert!(result.diagnostics.is_empty());
    assert_eq!(text(&result.ini, "Child", "Deep").as_deref(), Some("yes"));
}

#[test]
fn ini_directives_no_directive_preserves_native_duplicates_hash_and_saved_bytes() {
    let root = IniFile::from_str("[Unit]\nValue=first\nValue=ignored\n[Unit]\nOther=secondbody\n");
    let bytes = crate::util::save_codec::serialize(&root).unwrap();
    let hash = root.content_hash();
    let result = process_directives(
        root,
        "root.ini",
        |_| panic!("no file IO"),
        Default::default(),
    )
    .unwrap();
    assert!(result.diagnostics.is_empty());
    assert_eq!(result.ini.content_hash(), hash);
    assert_eq!(
        crate::util::save_codec::serialize(&result.ini).unwrap(),
        bytes
    );
}

#[test]
fn ini_directives_reject_zero_depth_even_without_directives() {
    let result = process_directives(
        IniFile::empty(),
        "root.ini",
        |_| Ok(None),
        DirectiveOptions {
            max_depth: 0,
            ..Default::default()
        },
    );
    assert_eq!(result.err().unwrap().kind, DiagnosticKind::InvalidDepth);
}

#[test]
fn ini_directives_raw_parse_and_restore_never_open_files_or_expand_again() {
    let raw = IniFile::from_str("[#include]\n0=child.ini\n[Child]\n$Inherits=Base\n");
    assert!(raw.section("Base").is_none());
    let bytes = crate::util::save_codec::serialize(&raw).unwrap();
    let restored: IniFile = crate::util::save_codec::deserialize(&bytes).unwrap();
    assert_eq!(restored.content_hash(), raw.content_hash());
    assert!(restored.section("Base").is_none());
}

#[test]
fn ini_directives_production_vfs_sources_and_immutable_consumed_snapshots() {
    use crate::map::source::test_support::TestDirectory;
    use crate::rules::retail_sources::select_ini;
    let dir = TestDirectory::new("ini-directives-source");
    dir.write(
        "root.ini",
        b"[#include]\n0=./CHILD.INI\n[Child]\n$Inherits=Base\nOwn=root\n",
    );
    dir.write("child.ini", b"[Base]\nOwn=base\nOther=child\n");
    let assets = crate::assets::asset_manager::AssetManager::from_loose_root_for_test(dir.path());
    let selected = select_ini(&assets, "root.ini").unwrap();
    assert!(selected.directive_diagnostics.is_empty());
    assert_eq!(selected.included_sources.len(), 1);
    assert!(
        selected.included_sources[0]
            .source_archive
            .starts_with("loose:")
    );
    assert_eq!(text(&selected.ini, "Child", "Own").as_deref(), Some("root"));
    assert_eq!(
        text(&selected.ini, "Child", "Other").as_deref(),
        Some("child")
    );
    dir.write("child.ini", b"[Base]\nOther=changed\n");
    assert_eq!(
        text(&selected.ini, "Child", "Other").as_deref(),
        Some("child")
    );
}

#[test]
fn ini_directives_production_missing_empty_and_strict_policy() {
    use crate::map::source::test_support::TestDirectory;
    use crate::rules::retail_sources::{select_ini, select_ini_with_options};
    let dir = TestDirectory::new("ini-directives-missing");
    dir.write(
        "root.ini",
        b"[#include]\n0=missing.ini\n1=empty.ini\n2=good.ini\n",
    );
    dir.write("empty.ini", b"");
    dir.write("good.ini", b"[Unit]\nValue=good\n");
    let assets = crate::assets::asset_manager::AssetManager::from_loose_root_for_test(dir.path());
    let selected = select_ini(&assets, "root.ini").unwrap();
    assert_eq!(selected.directive_diagnostics.len(), 2);
    assert_eq!(selected.included_sources.len(), 1);
    assert_eq!(
        text(&selected.ini, "Unit", "Value").as_deref(),
        Some("good")
    );
    assert!(
        select_ini_with_options(
            &assets,
            "root.ini",
            DirectiveOptions {
                strict: true,
                ..Default::default()
            }
        )
        .is_err()
    );
}

#[test]
fn ini_directives_production_loose_child_wins_over_mounted_mix() {
    use crate::assets::asset_manager::{
        AssetManager, MediaArchiveMode, test_support::audio_test_install,
    };
    use crate::rules::retail_sources::select_ini;
    let dir = audio_test_install("ini-directives-mix");
    dir.write_mix("expandmd01.mix", "root.ini", b"[#include]\n0=child.ini\n");
    dir.write_mix("expandmd00.mix", "child.ini", b"[Unit]\nValue=mix\n");
    let assets = AssetManager::new(dir.path(), MediaArchiveMode::STOCK_DIGITAL).unwrap();
    let selected = select_ini(&assets, "root.ini").unwrap();
    assert_eq!(
        selected.included_sources[0].source_archive,
        "expandmd00.mix"
    );
    assert_eq!(text(&selected.ini, "Unit", "Value").as_deref(), Some("mix"));
    std::fs::write(dir.path().join("child.ini"), b"[Unit]\nValue=loose\n").unwrap();
    let assets = AssetManager::new(dir.path(), MediaArchiveMode::STOCK_DIGITAL).unwrap();
    let selected = select_ini(&assets, "root.ini").unwrap();
    assert!(
        selected.included_sources[0]
            .source_archive
            .starts_with("loose:")
    );
    assert_eq!(
        text(&selected.ini, "Unit", "Value").as_deref(),
        Some("loose")
    );
}

#[test]
fn ini_directives_production_child_changes_effective_hash_and_survives_restore() {
    let a = expand(
        "[#include]\n0=child.ini\n",
        &[("child.ini", "[Unit]\nValue=a\n")],
    );
    let b = expand(
        "[#include]\n0=child.ini\n",
        &[("child.ini", "[Unit]\nValue=b\n")],
    );
    assert_ne!(a.ini.content_hash(), b.ini.content_hash());
    let saved = crate::util::save_codec::serialize(&a.ini).unwrap();
    let restored: IniFile = crate::util::save_codec::deserialize(&saved).unwrap();
    assert_eq!(text(&restored, "Unit", "Value").as_deref(), Some("a"));
    assert_eq!(a.ini.content_hash(), restored.content_hash());
}

#[test]
fn ini_directives_strict_missing_parent_and_independent_valid_cycle_branch() {
    let root = IniFile::from_str("[Child]\n$Inherits=Missing\nOwn=yes\n");
    let error = process_directives(
        root,
        "root.ini",
        |_| panic!("no include"),
        DirectiveOptions {
            strict: true,
            ..Default::default()
        },
    )
    .err()
    .unwrap();
    assert_eq!(error.kind, DiagnosticKind::InheritanceParentMissing);
    assert_eq!(error.reference_chain, ["Child", "Missing"]);
    let result = expand(
        "[A]\n$Inherits=B,Good\nOwnA=yes\n[B]\n$Inherits=A\nOwnB=yes\n[Good]\nExtra=yes\n",
        &[],
    );
    assert_eq!(text(&result.ini, "A", "Extra").as_deref(), Some("yes"));
    assert!(text(&result.ini, "A", "OwnB").is_none());
    assert!(text(&result.ini, "B", "OwnA").is_none());
}

#[test]
fn ini_directives_reach_the_existing_ordered_type_processor_and_fixed_art() {
    use crate::map::source::test_support::TestDirectory;
    use crate::rules::retail_sources::{load_startup_rules, select_mode_ini};
    let dir = TestDirectory::new("ini-directives-typed-rules");
    dir.write(
        "rulesmd.ini",
        b"[#include]\n0=types.ini\n[General]\nBuildSpeed=.7\n[TEST]\nStrength=250\n",
    );
    dir.write(
        "types.ini",
        b"[VehicleTypes]\n0=TEST\n[TEST]\nStrength=100\n",
    );
    dir.write("artmd.ini", b"[#include]\n0=art-extra.ini\n");
    dir.write("art-extra.ini", b"[TEST]\nVoxel=yes\n");
    dir.write("mode.ini", b"[#include]\n0=mode-extra.ini\n");
    dir.write("mode-extra.ini", b"[General]\nBuildSpeed=.9\n");
    let assets = crate::assets::asset_manager::AssetManager::from_loose_root_for_test(dir.path());
    let (_, _, mut owner) = load_startup_rules(&assets, Default::default(), Default::default())
        .unwrap()
        .into_parts();
    let mode = select_mode_ini(&assets, Some("mode.ini")).unwrap();
    let loaded = owner
        .load_scenario(
            crate::rules::process_owner::NativeScenarioRulesPrefix::NonCampaign(mode.as_ref()),
            &IniFile::empty(),
        )
        .unwrap();
    let (rules, _, art, _) = loaded.into_parts();
    assert_eq!(rules.object("TEST").unwrap().strength, 250);
    assert_eq!(
        rules.production.build_speed.bits(),
        f64::from(0.9_f32).to_bits()
    );
    assert!(art.section_or_empty("TEST").read_bool("Voxel", false));
}
