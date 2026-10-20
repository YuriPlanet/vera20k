//! Read-only probe for the same file-directive path used by rule selection.
use crate::asset_tools::report::ErrorReport;
use crate::assets::asset_manager::AssetManager;
use crate::rules::ini_parser::{IniFile, directives::DirectiveOptions};
use crate::rules::retail_sources::select_ini_with_options;
use serde_json::{Value, json};
use std::collections::BTreeSet;

pub fn run(
    assets: &AssetManager,
    name: &str,
    strict: bool,
    max_depth: usize,
    all_mixes: bool,
    source_only: bool,
) -> Result<Value, ErrorReport> {
    let fail = |error: String| {
        ErrorReport { error, hint: Some("This checks INI expansion, not MO gameplay. --all-mixes widens only this tool's archive search.".into()) }
    };
    let raw = assets
        .resolve_ref(name)
        .ok_or_else(|| fail(format!("INI {name} is unavailable")))?;
    let raw = IniFile::from_bytes(raw.bytes).map_err(|e| fail(e.to_string()))?;
    let selected = select_ini_with_options(assets, name, DirectiveOptions { strict, max_depth })
        .map_err(fail)?;
    let raw_names: BTreeSet<_> = raw.section_names().into_iter().collect();
    let names: BTreeSet<_> = selected.ini.section_names().into_iter().collect();
    let added: Vec<_> = names.difference(&raw_names).copied().collect();
    Ok(json!({
        "file": name,
        "scope": if source_only { "tooling_ini_root" } else if all_mixes { "tooling_all_mixes" } else { "game_archive_stack" },
        "all_mixes": all_mixes,
        "strict": strict, "max_depth": max_depth,
        "source": selected.source, "included_sources": selected.included_sources,
        "diagnostics": selected.directive_diagnostics,
        "raw_section_occurrences": raw.section_names().len(),
        "raw_unique_sections": raw_names.len(),
        "expanded_section_occurrences": selected.ini.section_names().len(),
        "expanded_unique_sections": names.len(), "added_sections": added,
        "effective_ini_hash": format!("{:016x}", selected.ini.content_hash()),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn ini_expand_probe_reports_real_dependencies_and_structured_failures() {
        let dir = crate::map::source::test_support::TestDirectory::new("ini-expand-probe");
        dir.write("root.ini", b"[#include]\n0=child.ini\n1=missing.ini\n");
        dir.write("child.ini", b"[Child]\nValue=yes\n");
        let assets = AssetManager::from_loose_root_for_test(dir.path());
        let report = run(&assets, "root.ini", false, 64, false, false).unwrap();
        assert_eq!(report["included_sources"].as_array().unwrap().len(), 1);
        assert_eq!(report["diagnostics"][0]["kind"], "include_missing");
        assert_eq!(report["added_sections"], json!(["Child"]));
        assert_eq!(report["scope"], "game_archive_stack");
        assert!(run(&assets, "root.ini", true, 64, false, false).is_err());
        assert!(run(&assets, "root.ini", false, 0, false, false).is_err());
    }

    #[test]
    fn ini_expand_source_only_root_needs_no_retail_bootstrap_and_keeps_mix_scope_explicit() {
        let dir = crate::map::source::test_support::TestDirectory::new("ini-source-only");
        dir.write("root.ini", b"[#include]\n0=child.ini\n");
        let mix = crate::map::source::test_support::make_new_format_mix_bytes(&[(
            "child.ini",
            b"[Child]\nValue=yes\n",
        )]);
        dir.write("custom.mix", &mix);
        assert!(crate::asset_tools::root::open_manager(dir.path(), false).is_err());
        let normal = crate::asset_tools::root::open_ini_probe_manager(dir.path(), false).unwrap();
        assert!(run(&normal, "root.ini", true, 64, false, true).is_err());
        let wide = crate::asset_tools::root::open_ini_probe_manager(dir.path(), true).unwrap();
        let report = run(&wide, "root.ini", true, 64, true, true).unwrap();
        assert_eq!(report["scope"], "tooling_ini_root");
        assert_eq!(
            report["included_sources"][0]["source_archive"],
            "custom.mix"
        );
    }
}
