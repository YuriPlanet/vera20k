//! Launch compatibility probe for external launchers (docs/spawn.md, "Probe").
//!
//! `vera20k --probe` (desktop) and the browser's `probe()` report what a
//! spawn.ini launch on this installation will be checked against: the build
//! version and multiplayer rules digest the multiplayer agreement compares,
//! the client map's physical SHA-256 and effective digest, and the country
//! and colour indices spawn.ini fields address. Every value comes from the
//! owner the match itself uses: [`agreement::version`], the process
//! [`ProcessAssets`] rules digest for the Battle mode, and
//! [`crate::map::source::load_network_map_ini`] for the map, so two
//! installations whose probes agree pass each other's agreement.
//!
//! It runs headless: no window, audio output, network or profile writes.

use std::path::Path;

use anyhow::{Context, Result};
use serde::Serialize;

use crate::app::frontend::cncnet_spawn::{
    BATTLE_MODE_OVERRIDE_FILE, max_spawn_color_index, max_spawn_country_index,
};
use crate::app::process_assets::ProcessAssets;
use crate::app::shell_network_setup::agreement;
use crate::assets::asset_manager::{AssetManager, MediaArchiveMode};

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct ProbeReport {
    /// The multiplayer agreement's version: package version, network build
    /// hash and the Ares/Phobos/NPatch feature bits. `ExpectedVersion`.
    pub version: String,
    pub build_identity: BuildIdentity,
    /// The multiplayer rules agreement for spawn Battle launches
    /// (`MPBattleMD.ini`). `ExpectedRulesDigest`.
    pub rules_digest: String,
    pub map: Option<ProbeMap>,
    /// `Side=` values: Country array indices this build admits.
    pub countries: Vec<IndexedName>,
    /// `Color=` values: colour priorities and the `[Colors]` entry each draws.
    pub colors: Vec<IndexedName>,
    /// Start waypoints the map authors (`[SpawnLocations]` 0..N-1); null
    /// without `map`.
    pub max_start_waypoints: Option<i32>,
}

/// The parts of [`ProbeReport::version`], for display; clients may play
/// together when `version` and `rules_digest` (and the map) are equal.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct BuildIdentity {
    pub package_version: String,
    /// Hash of the multiplayer sources and enabled Cargo features.
    pub network_build: String,
    pub ares: bool,
    pub phobos: bool,
    pub npatch: bool,
    /// Informational; not part of `version`.
    pub verax: bool,
    /// Informational: the build's platform.
    pub target: &'static str,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct ProbeMap {
    /// Logical name every participant shares (the file name).
    pub file: String,
    pub byte_length: usize,
    /// SHA-256 of the physical map bytes. `ExpectedMapDigest`.
    pub source_sha256: String,
    /// Digest of the expanded map content the network agreement compares.
    pub effective_digest: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct IndexedName {
    pub index: u32,
    pub name: String,
}

/// The physical identity and start capacity of a launcher's map, read
/// exactly as an external network launch loads it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SpawnMapIdentity {
    pub map: ProbeMap,
    pub start_waypoints: i32,
}

pub(crate) fn spawn_map_identity(
    root: &Path,
    path: &str,
    assets: &AssetManager,
) -> Result<SpawnMapIdentity> {
    let loaded = crate::map::source::load_network_map_ini(root, path, assets)
        .with_context(|| format!("cannot load map {path}"))?;
    let (byte_length, source_sha256) = loaded
        .source
        .physical_identity()
        .context("map has no physical source")?;
    let file = Path::new(path)
        .file_name()
        .and_then(|name| name.to_str())
        .context("map has no file name")?
        .to_owned();
    Ok(SpawnMapIdentity {
        map: ProbeMap {
            file,
            byte_length,
            source_sha256: source_sha256.to_owned(),
            effective_digest: crate::map::source::effective_network_map_digest(&loaded.ini, false),
        },
        start_waypoints: crate::map::waypoints::skirmish_player_capacity(&loaded.ini),
    })
}

pub(crate) fn build_identity() -> BuildIdentity {
    BuildIdentity {
        package_version: env!("CARGO_PKG_VERSION").to_owned(),
        network_build: env!("VERA20K_NETWORK_BUILD").to_owned(),
        ares: crate::rules::ARES_ENABLED,
        phobos: crate::rules::PHOBOS_ENABLED,
        npatch: crate::rules::NPATCH_ENABLED,
        verax: crate::rules::VERAX_ENABLED,
        target: if cfg!(all(target_arch = "wasm32", target_os = "unknown")) {
            "wasm32"
        } else {
            std::env::consts::OS
        },
    }
}

/// Load the process sources as startup does (config INI roots, personal
/// rules, neutral archives) and report them; `map` is a path to the
/// launcher's map file.
pub(crate) fn probe(
    config: &crate::util::config::GameConfig,
    personal_rules: crate::rules::personal_rules::PersonalRules,
    media_archive_mode: MediaArchiveMode,
    map: Option<&str>,
) -> Result<ProbeReport> {
    let root = &config.paths.ra2_dir;
    let mut assets = AssetManager::new(root, media_archive_mode)
        .and_then(|assets| assets.with_ini_roots(&config.ini))
        .context("could not load the game archives")?;
    let mut process =
        ProcessAssets::new_with_ini_config(media_archive_mode, true, config.ini.clone())
            .with_personal_rules(personal_rules);
    let (startup_rules, projection) = process
        .initialize_sources_if_needed(&assets)
        .map_err(anyhow::Error::msg)
        .context("could not load the retail rules")?;
    // Startup registers these before any match computes its agreement.
    let _ = assets.register_neutral_archives();
    process.return_from_loading(assets);
    let rules_digest = process
        .multiplayer_rules_digest(BATTLE_MODE_OVERRIDE_FILE)
        .map_err(anyhow::Error::msg)
        .context("could not digest the multiplayer rules")?;
    // The Session Country array and PrepareSession's Country bodies, as the
    // shell reads them before a multiplayer start; the digested sources do
    // not change.
    process.read_multiplayer_country(&crate::rules::ini_parser::IniFile::empty(), -1);
    process.refresh_multiplayer_country_bodies();
    let max_country = max_spawn_country_index();
    let countries = process
        .multiplayer_countries()
        .enumerate()
        .take_while(|(index, _)| *index <= max_country as usize)
        .map(|(index, (id, _))| IndexedName {
            index: index as u32,
            name: id.to_owned(),
        })
        .collect();
    let schemes = projection
        .as_ref()
        .map(crate::rules::color_scheme::parse_color_schemes)
        .unwrap_or_default();
    let color_mode = startup_rules
        .as_ref()
        .map(crate::rules::ruleset::RuleSet::skirmish_colors)
        .unwrap_or_default();
    let colors = color_names(&schemes, color_mode);
    let map = map
        .map(|path| {
            spawn_map_identity(
                root,
                path,
                process.manager().context("retail sources missing")?,
            )
        })
        .transpose()?;
    Ok(ProbeReport {
        version: agreement::version(),
        build_identity: build_identity(),
        rules_digest,
        max_start_waypoints: map.as_ref().map(|map| map.start_waypoints),
        map: map.map(|map| map.map),
        countries,
        colors,
    })
}

/// Each `Color=` priority this build admits and the `[Colors]` entry it
/// selects (`Game_GetLinkedColor`). Vanilla offers the eight lobby
/// priorities; Phobos `SkirmishUnlimitedColors` addresses every entry.
fn color_names(
    schemes: &[crate::rules::color_scheme::ColorSchemeEntry],
    mode: crate::rules::skirmish_colors::SkirmishColorMode,
) -> Vec<IndexedName> {
    let last = if mode.unlimited() {
        (schemes.len() as i32 - 1).min(max_spawn_color_index())
    } else {
        7
    };
    (0..=last)
        .filter_map(|priority| {
            let entry = mode.entry_for_priority(priority);
            schemes.get(entry).map(|scheme| IndexedName {
                index: priority as u32,
                name: scheme.name.clone(),
            })
        })
        .collect()
}

/// Desktop `--probe [--ra2-dir DIR] [--map PATH]`: the game folder comes from
/// config.toml as for play unless `--ra2-dir` names one.
#[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
pub(crate) fn run_desktop(ra2_dir: Option<&Path>, map: Option<&Path>) -> Result<ProbeReport> {
    let config = crate::util::config::GameConfig::load()?;
    let config = match ra2_dir {
        Some(directory) => config.with_retail_directory(
            directory
                .canonicalize()
                .with_context(|| format!("retail directory {}", directory.display()))?,
        )?,
        None => config,
    };
    let personal_rules = existing_personal_rules(config.paths.settings_dir())?;
    let map = map
        .map(|path| {
            ensure_file(path)?;
            std::path::absolute(path)?
                .to_str()
                .map(str::to_owned)
                .context("map path is not UTF-8")
        })
        .transpose()?;
    probe(
        &config,
        personal_rules,
        crate::app::frontend::startup_options::RetailStartupOptions::default().media_archive_mode,
        map.as_deref(),
    )
}

#[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
fn ensure_file(path: &Path) -> Result<()> {
    anyhow::ensure!(
        crate::util::file_system::is_file(path),
        "map not found: {}",
        path.display()
    );
    Ok(())
}

/// The player's VERARULES.INI as startup reads it, without creating the
/// starter file (an absent file is the inactive starter).
pub(crate) fn existing_personal_rules(
    settings_dir: &Path,
) -> Result<crate::rules::personal_rules::PersonalRules> {
    match crate::app::persistence::rules_profile::load_existing(settings_dir)? {
        Some(bytes) => crate::rules::personal_rules::PersonalRules::from_bytes(&bytes)
            .context("Could not read VERARULES.INI"),
        None => Ok(Default::default()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rules::color_scheme::ColorSchemeEntry;
    use crate::rules::skirmish_colors::SkirmishColorMode;

    fn schemes(count: usize) -> Vec<ColorSchemeEntry> {
        (0..count)
            .map(|index| ColorSchemeEntry {
                name: format!("C{index}"),
                hsv: [0; 3],
            })
            .collect()
    }

    #[test]
    fn vanilla_colors_are_the_eight_priorities_through_the_linked_color_table() {
        let colors = color_names(&schemes(20), SkirmishColorMode::VANILLA);
        let names: Vec<_> = colors.iter().map(|c| (c.index, c.name.as_str())).collect();
        // PRIORITY_TO_SCHEME_INDEX [3,11,21,29,13,25,17,15] halved.
        assert_eq!(
            names,
            [
                (0, "C1"),
                (1, "C5"),
                (2, "C10"),
                (3, "C14"),
                (4, "C6"),
                (5, "C12"),
                (6, "C8"),
                (7, "C7")
            ]
        );
    }

    #[test]
    fn report_serializes_the_documented_shape() {
        let report = ProbeReport {
            version: "v".into(),
            build_identity: build_identity(),
            rules_digest: "r".into(),
            map: Some(ProbeMap {
                file: "spawnmap.ini".into(),
                byte_length: 3,
                source_sha256: "s".into(),
                effective_digest: "e".into(),
            }),
            countries: vec![IndexedName {
                index: 0,
                name: "Americans".into(),
            }],
            colors: Vec::new(),
            max_start_waypoints: Some(4),
        };
        let value = serde_json::to_value(&report).unwrap();
        let mut keys: Vec<_> = value.as_object().unwrap().keys().cloned().collect();
        keys.sort();
        assert_eq!(
            keys,
            [
                "build_identity",
                "colors",
                "countries",
                "map",
                "max_start_waypoints",
                "rules_digest",
                "version"
            ]
        );
        let map: Vec<_> = value["map"].as_object().unwrap().keys().cloned().collect();
        assert_eq!(
            map,
            ["byte_length", "effective_digest", "file", "source_sha256"]
        );
        assert_eq!(
            value["countries"][0],
            serde_json::json!({"index":0,"name":"Americans"})
        );
        assert_eq!(
            value["build_identity"]["package_version"],
            env!("CARGO_PKG_VERSION")
        );
        assert!(value["version"].is_string());
        let none = ProbeReport {
            map: None,
            max_start_waypoints: None,
            ..report
        };
        let value = serde_json::to_value(&none).unwrap();
        assert!(value["map"].is_null() && value["max_start_waypoints"].is_null());
    }

    #[test]
    fn map_identity_is_the_physical_bytes_and_start_waypoints() {
        use crate::map::source::test_support::TestDirectory;
        let dir = TestDirectory::new("probe-map");
        let bytes = b"[Map]\nTheater=TEMPERATE\nSize=0,0,2,1\nLocalSize=0,0,2,1\n[Waypoints]\n0=1001\n1=1002\n2=1003\n[IsoMapPack5]\n1=DwALABwBAAIA/////wAAABEAAA==\n";
        dir.write("spawnmap.ini", bytes);
        let assets = AssetManager::from_loose_root_for_test(dir.path());
        let path = dir.path().join("spawnmap.ini");
        let identity = spawn_map_identity(dir.path(), path.to_str().unwrap(), &assets).unwrap();
        assert_eq!(identity.map.file, "spawnmap.ini");
        assert_eq!(identity.map.byte_length, bytes.len());
        assert_eq!(
            identity.map.source_sha256,
            crate::util::sha256::sha256_hex(bytes)
        );
        assert_eq!(identity.map.effective_digest.len(), 64);
        assert_eq!(identity.start_waypoints, 3);
    }
}
