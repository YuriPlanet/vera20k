//! Test access to the retail INI corpus in the gitignored `ini/` directory.
//!
//! The retail INIs are EA data, so they are never committed. `cargo run --bin
//! extract-ini` fills `ini/` from a local Red Alert 2 / Yuri's Revenge install.
//! Without them a retail-data test prints a `SKIPPED` line and passes, so a
//! fresh clone's `cargo test -p vera20k --lib` is green. Set
//! `VERA20K_REQUIRE_RETAIL_INI=1` to make a missing file fail instead; do that
//! in any checkout whose results you rely on for parity. Archive-backed fixture
//! checks instead use explicit `RA2_DIR` and `VERA20K_REQUIRE_RETAIL_ASSETS=1`.

use std::ffi::OsStr;
use std::path::{Path, PathBuf};

use crate::assets::asset_manager::{AssetManager, MediaArchiveMode};
use crate::rules::ini_parser::IniFile;

/// Environment variable that turns a missing retail INI into a test failure.
pub(crate) const REQUIRE_RETAIL_INI_ENV: &str = "VERA20K_REQUIRE_RETAIL_INI";

/// Archive-backed tests have a separate requirement from extracted INI tests.
pub(crate) const REQUIRE_RETAIL_ASSETS_ENV: &str = "VERA20K_REQUIRE_RETAIL_ASSETS";

fn required(value: Option<&OsStr>) -> bool {
    value.is_some_and(|value| !value.is_empty() && value != "0")
}

fn retail_ini_path(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("ini").join(name)
}

fn require_retail_ini() -> bool {
    required(std::env::var_os(REQUIRE_RETAIL_INI_ENV).as_deref())
}

/// The bytes of `ini/<name>`, or `None` after printing a `SKIPPED` line when
/// the file is absent and [`REQUIRE_RETAIL_INI_ENV`] is not set.
pub(crate) fn retail_ini_bytes(name: &str) -> Option<Vec<u8>> {
    let path = retail_ini_path(name);
    match std::fs::read(&path) {
        Ok(bytes) => Some(bytes),
        Err(error) if require_retail_ini() => panic!(
            "cannot read {}: {error} ({REQUIRE_RETAIL_INI_ENV} is set)",
            path.display()
        ),
        Err(error) => {
            eprintln!(
                "SKIPPED: {} is unavailable ({error}); run `cargo run --bin extract-ini` \
                 to extract the retail INIs",
                path.display()
            );
            None
        }
    }
}

/// `ini/<name>` as UTF-8 text, or `None` when absent (see [`retail_ini_bytes`]).
pub(crate) fn retail_ini_text(name: &str) -> Option<String> {
    let bytes = retail_ini_bytes(name)?;
    Some(String::from_utf8(bytes).unwrap_or_else(|error| panic!("retail {name} is UTF-8: {error}")))
}

/// `ini/<name>` parsed, or `None` when absent (see [`retail_ini_bytes`]).
pub(crate) fn retail_ini(name: &str) -> Option<IniFile> {
    let bytes = retail_ini_bytes(name)?;
    Some(
        IniFile::from_bytes(&bytes).unwrap_or_else(|error| panic!("retail {name} parses: {error}")),
    )
}

/// Retail `rulesmd.ini` and `artmd.ini` parsed, or `None` when either is absent.
pub(crate) fn retail_rules_and_art() -> Option<(IniFile, IniFile)> {
    Some((retail_ini("rulesmd.ini")?, retail_ini("artmd.ini")?))
}

fn open_retail_assets(
    root: Option<&OsStr>,
    required: bool,
) -> Result<Option<(PathBuf, AssetManager)>, String> {
    let Some(root) = root else {
        return if required {
            Err(format!(
                "RA2_DIR is unset ({REQUIRE_RETAIL_ASSETS_ENV} is set)"
            ))
        } else {
            Ok(None)
        };
    };
    let root = PathBuf::from(root);
    if root.as_os_str().is_empty() || !root.is_dir() {
        return Err(format!(
            "RA2_DIR is not a retail directory: {}",
            root.display()
        ));
    }
    let assets = AssetManager::new(&root, MediaArchiveMode::STOCK_DIGITAL)
        .map_err(|error| format!("load retail archive stack at {}: {error}", root.display()))?;
    Ok(Some((root, assets)))
}

/// Active-YR archive fixtures use explicit RA2_DIR only, never config fallback.
/// An absent variable skips unless REQUIRE_RETAIL_ASSETS is enabled; an explicit
/// invalid install always fails. REQUIRE_RETAIL_INI still means extracted files.
pub(crate) fn retail_assets() -> Option<(PathBuf, AssetManager)> {
    let result = open_retail_assets(
        std::env::var_os("RA2_DIR").as_deref(),
        required(std::env::var_os(REQUIRE_RETAIL_ASSETS_ENV).as_deref()),
    )
    .unwrap_or_else(|error| panic!("{error}"));
    if result.is_none() {
        eprintln!(
            "SKIPPED: archive fixture needs RA2_DIR; set {REQUIRE_RETAIL_ASSETS_ENV}=1 to require it"
        );
    }
    result
}

/// Authored root text and final scenario fields are deliberately separate.
pub(crate) struct RetailBattleRules {
    pub(crate) authored_rules: IniFile,
    pub(crate) fixed_art: IniFile,
    pub(crate) processed_rules: IniFile,
    pub(crate) rules: crate::rules::ruleset::RuleSet,
}

/// Select once and retain the production process owner. A present but malformed
/// optional input cannot serve as successful retail-fixture evidence.
pub(crate) fn retail_rules_owner(
    assets: &AssetManager,
) -> crate::rules::process_owner::NativeRulesProcessOwner {
    use crate::rules::retail_sources::RetailRulesSources;

    let sources = RetailRulesSources::select(assets).expect("select active-YR startup sources");
    for (name, selected) in [
        ("langrule.ini", sources.langrule.as_ref()),
        ("soundmd.ini", sources.soundmd.as_ref()),
    ] {
        assert!(
            selected.is_some() || assets.resolve_ref(name).is_none(),
            "present optional retail source {name} must parse"
        );
    }
    eprintln!(
        "retail root {:?}; fixed ART {:?}",
        sources.rulesmd.source, sources.artmd.source
    );
    sources.into_startup().expect("cold startup").into_parts().2
}

/// Bounded stock Hills/Battle fixture through the app's source-selection order.
/// No claims about campaign, TMCJ4F, LANGRULE Digest or other maps/modes.
// Keep the fixture's upward map calls explicitly test-gated: the source-level
// dependency guard scans leaf files without inheriting rules/mod.rs's gate.
#[cfg(test)]
pub(crate) fn retail_battle_rules() -> Option<RetailBattleRules> {
    use crate::rules::retail_sources::select_ini;

    let (root, mut assets) = retail_assets()?;
    let mut owner = retail_rules_owner(&assets);
    let authored_rules = owner.selected_rules_root().clone();
    assets
        .register_neutral_archives()
        .expect("register retail shell archives");
    crate::map::scenario_sources::list_skirmish_scenario_records_with_assets(
        &root,
        &mut assets,
        None,
    )
    .expect("register retail scenario archives");
    let modes = crate::skirmish_modes::skirmish_modes_from_assets(&assets)
        .expect("load retail mode roster");
    let mode = crate::skirmish_modes::mode_by_id(&modes, 1).expect("stock Battle mode");
    let map = crate::map::source::load_map_by_name_or_path_with_assets(&root, "Hills.mmx", &assets)
        .expect("load stock Hills fixture");
    crate::map::theater::load_theater(&mut assets, &map.map.header.theater)
        .expect("load Hills theater before mode selection");
    let mode = select_ini(&assets, &mode.override_file).expect("select Battle override");
    eprintln!("retail mode {:?}; map {:?}", mode.source, map.source);
    let (mut rules, processed_rules, fixed_art, _) = owner
        .load_noncampaign_scenario(Some(&mode.ini), &map.map.ini)
        .expect("process noncampaign Hills/Battle Rules")
        .into_parts();
    let mut art = crate::rules::art_data::ArtRegistry::from_ini(&fixed_art);
    art.apply_anim_type_read_states(&rules.anim_type_art_read_states);
    rules.merge_art_data(&art);
    rules.general.resolve_art_rates(&fixed_art);
    Some(RetailBattleRules {
        authored_rules,
        fixed_art,
        processed_rules,
        rules,
    })
}

// The same explicit boundary covers the map-owned temporary-directory helper.
#[cfg(test)]
#[test]
fn archive_gate_requires_only_explicit_inputs_without_environment_mutation() {
    assert!(open_retail_assets(None, false).unwrap().is_none());
    assert!(open_retail_assets(None, true).is_err());
    for mandatory in [false, true] {
        assert!(open_retail_assets(Some(OsStr::new("")), mandatory).is_err());
        let directory = crate::map::source::test_support::TestDirectory::new("retail-gate");
        assert!(open_retail_assets(Some(directory.path().as_os_str()), mandatory).is_err());
        assert!(
            open_retail_assets(Some(directory.path().join("absent").as_os_str()), mandatory)
                .is_err()
        );
        directory.write("ra2md.mix", b"malformed required archive");
        assert!(open_retail_assets(Some(directory.path().as_os_str()), mandatory).is_err());
    }
}

#[test]
fn retail_requirement_values_preserve_extracted_ini_policy() {
    for value in [None, Some(OsStr::new("")), Some(OsStr::new("0"))] {
        assert!(!required(value));
    }
    for value in ["1", "true", "required"] {
        assert!(required(Some(OsStr::new(value))));
    }
}
