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

/// Fixed ARTMD.INI [GI]/[GISequence] inputs for asset-free authored scenarios.
/// Callers explicitly supply E1 Image=GI and use the existing ART reader and
/// sequence binder. These are input records, not native replay goldens. The
/// original reader/Ready receipt is in anytown_damage/foot_missions.json.
pub(crate) const GI_ART_EXCERPT: &str = "\
[GI]
Sequence=GISequence
Crawls=yes
FireUp=2
PrimaryFireFLH=80,0,105
SecondaryFireFLH=80,0,90
[GISequence]
Ready=0,1,1
Guard=0,1,1
Prone=86,1,6
Walk=8,6,6
FireUp=164,6,6
Down=260,2,2
Crawl=86,6,6
Up=276,2,2
FireProne=212,6,6
Idle1=56,15,0,S
Idle2=71,15,0,E
Die1=134,15,0
Die2=149,15,0
Die3=0,1,1
Die4=0,1,1
Die5=0,1,1
Deploy=300,15,0
Deployed=292,1,1
DeployedFire=315,6,6
DeployedIdle=0,0,0
Undeploy=276,2,2
Paradrop=363,1,0
Cheer=364,8,0,E
Panic=8,6,6
";

/// Physical ARTMD [TRST]/[TerroristSequence] inputs for fatal receiver
/// fixtures. These are reader inputs; native death goldens remain separate.
pub(crate) const TRST_ART_EXCERPT: &str = "\
[TRST]
Sequence=TerroristSequence
Crawls=no
FireUp=1
[TerroristSequence]
Ready=0,1,1
Guard=0,1,1
Prone=0,1,1
Walk=8,6,6
Down=8,2,6
Crawl=8,6,6
Up=8,2,6
Idle1=56,15,0,S
Idle2=71,15,0,E
Die1=86,15,0
Die2=101,15,0
Die3=0,1,1
Die4=0,1,1
Die5=0,1,1
Paradrop=179,1,0
Cheer=180,8,0,E
FireUp=164,6,6
FireProne=164,6,6
Deploy=164,15,0
Panic=8,6,6
";

/// Environment variable that turns a missing retail INI into a test failure.
pub(crate) const REQUIRE_RETAIL_INI_ENV: &str = "VERA20K_REQUIRE_RETAIL_INI";

/// Archive-backed tests have a separate requirement from extracted INI tests.
pub(crate) const REQUIRE_RETAIL_ASSETS_ENV: &str = "VERA20K_REQUIRE_RETAIL_ASSETS";

/// One mechanical export of the original registered UnitReady consumers.
/// The factory oracle owns selection and verifies this export against its
/// complete native receipts; component tests only read the selected state.
pub(crate) fn factory_unit_ready_native() -> serde_json::Value {
    let fixture: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/_factory_infantry_output/fixtures/unit-ready-consumer-rust.json",
    ))
    .expect("mechanically selected native UnitReady fixture");
    assert_eq!(fixture["schema"], 1);
    for control in fixture["controls"].as_object().unwrap().values() {
        assert_eq!(
            control["native_sha256"],
            "1cdd1180e49024fbda8ad568caac2e86e856063ff67ab38f62b7d2c7bb84298c"
        );
    }
    fixture
}

fn required(value: Option<&OsStr>) -> bool {
    value.is_some_and(|value| !value.is_empty() && value != "0")
}

fn retail_ini_path(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("ini").join(name)
}

pub(crate) fn require_retail_ini() -> bool {
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

    let audio_definitions = crate::rules::audio_sources::AudioDefinitions::select(assets);
    let sources = RetailRulesSources::select(assets).expect("select active-YR startup sources");
    for (name, selected) in [
        (
            "langrule.ini",
            sources.langrule.as_ref().map(|source| &source.source),
        ),
        ("soundmd.ini", audio_definitions.sound_source()),
        ("evamd.ini", audio_definitions.eva_source()),
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
    sources
        .into_startup(std::sync::Arc::clone(audio_definitions.sounds()))
        .expect("cold startup")
        .into_parts()
        .2
}

/// Bounded stock Hills/Battle fixture through the app's source-selection order.
/// No claims about campaign, TMCJ4F, LANGRULE Digest or other maps/modes.
// Keep the fixture's upward map calls explicitly test-gated: the source-level
// dependency guard scans leaf files without inheriting rules/mod.rs's gate.
#[cfg(test)]
pub(crate) fn retail_battle_rules() -> Option<RetailBattleRules> {
    retail_battle_rules_for_map("Hills.mmx")
}

/// The same physical retail startup/mode/map source owner for a requested
/// stock Battle map. Rules and the fixed ART snapshot keep their production
/// selection and reader order; this does not load the app's asset bindings.
#[cfg(test)]
pub(crate) fn retail_battle_rules_for_map(map_name: &str) -> Option<RetailBattleRules> {
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
    let map = crate::map::source::load_map_by_name_or_path_with_assets(&root, map_name, &assets)
        .expect("load requested retail Battle fixture");
    crate::map::theater::load_theater(&mut assets, &map.map.header.theater)
        .expect("load fixture theater before mode selection");
    let mode = select_ini(&assets, &mode.override_file).expect("select Battle override");
    eprintln!("retail mode {:?}; map {:?}", mode.source, map.source);
    let (mut rules, processed_rules, fixed_art, _) = owner
        .load_noncampaign_scenario(Some(&mode.ini), &map.map.ini)
        .expect("process noncampaign retail Battle Rules")
        .into_parts();
    let art = crate::rules::art_data::ArtRegistry::from_ini(&fixed_art);
    rules.install_art_data(art);
    // Match the app's data-only ART sequence binding before a frame fixture
    // fires or destroys Infantry. Without it death actions cannot finish.
    rules.bind_animation_sequences(
        &crate::rules::infantry_sequence::parse_infantry_sequence_registry(&fixed_art),
    );
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
