//! Retail-root discovery and `AssetManager` construction for the `asset` tool.
//!
//! Collapses the three incompatible conventions in the tree — hardcoded install
//! paths in tool binaries, `GameConfig::load()`, and the `RA2_DIR` env var used
//! by tests — into one resolution order.
//!
//! ## Dependency rules
//! - Depends on `assets/` and `util/config`. No render, sim, or app types.

use std::path::{Path, PathBuf};

use crate::assets::asset_manager::{AssetManager, MediaArchiveMode};
use crate::util::archive_names::ArchiveNames;
use crate::util::ares_launch::AresSetting;
use crate::util::config::GameConfig;
use crate::util::ini_roots::IniRootConfig;

/// Where the retail root came from, for diagnostics.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RootSource {
    Flag,
    Env,
    Config,
}

/// Resolve the retail install directory in strict priority order:
/// `--ra2-dir` → `$RA2_DIR` → `config.toml` via [`GameConfig::load`].
///
/// The directory is verified to exist, because every downstream failure from a
/// wrong root looks like "asset not found" instead of "wrong directory".
pub fn resolve_ra2_dir(explicit: Option<&Path>) -> Result<(PathBuf, RootSource), String> {
    if let Some(dir) = explicit {
        return check(dir.to_path_buf(), RootSource::Flag);
    }
    if let Ok(env_dir) = std::env::var("RA2_DIR")
        && !env_dir.trim().is_empty()
    {
        return check(PathBuf::from(env_dir), RootSource::Env);
    }
    match GameConfig::load() {
        Ok(config) => check(config.paths.ra2_dir, RootSource::Config),
        Err(err) => Err(format!(
            "no retail directory: --ra2-dir not given, RA2_DIR unset, and config.toml \
             could not be read ({err})"
        )),
    }
}

fn check(dir: PathBuf, source: RootSource) -> Result<(PathBuf, RootSource), String> {
    if !dir.is_dir() {
        return Err(format!(
            "retail directory does not exist: {} (from {source:?})",
            dir.display()
        ));
    }
    Ok((dir, source))
}

/// Build the archive stack.
///
/// Pins the stock numbered archive policy. Command arguments and paths cannot
/// change media selection.
///
/// `all_mixes` additionally mounts archives the startup path skips. It is a
/// tooling-only widening of the search set, so results found only that way are
/// not what the game would resolve. `ares` is the player's Ares setting the
/// game opens with: On reads the installation as an Ares game, as the game
/// does (`AssetManager::ships_ares`). `archives` supplies the archive names.
pub fn open_manager(
    ra2_dir: &Path,
    all_mixes: bool,
    ares: AresSetting,
    archives: &ArchiveNames,
) -> Result<AssetManager, String> {
    let mut manager = AssetManager::with_mod_and_ares(
        ra2_dir,
        None,
        MediaArchiveMode::STOCK_DIGITAL,
        ares,
        archives,
    )
    .map_err(|err| {
        format!(
            "could not mount archives under {}: {err}",
            ra2_dir.display()
        )
    })?;

    if all_mixes {
        manager
            .load_all_disk_mixes()
            .map_err(|err| format!("--all-mixes failed: {err}"))?;
    }

    Ok(manager)
}

/// INI commands and mod inspection share the host's base INI policy.
/// An explicit CLI request replaces the entire config, never individual fields.
fn resolve_ini_config(
    explicit: Option<&IniRootConfig>,
    load: impl FnOnce() -> anyhow::Result<GameConfig>,
) -> Result<IniRootConfig, String> {
    let config = match explicit {
        Some(config) => config.clone(),
        None => {
            load()
                .map_err(|error| format!("INI configuration: {error:#}"))?
                .ini
        }
    };
    config.validate()?;
    Ok(config)
}

/// Open an INI command's selected roots without a mod overlay.
pub fn open_ini_manager(
    root: &Path,
    all_mixes: bool,
    explicit: Option<&IniRootConfig>,
    ares: AresSetting,
) -> Result<AssetManager, String> {
    let config = resolve_ini_config(explicit, GameConfig::load)?;
    open_manager(root, all_mixes, ares, &ArchiveNames::default())?
        .with_ini_roots(&config)
        .map_err(|error| error.to_string())
}

/// Inspect a mod through the same definition and archive policy as a game
/// launch. This deliberately does not consult the desktop's selected mod.
pub fn open_mod_manager(
    root: &Path,
    mod_dir: &Path,
    all_mixes: bool,
    explicit: Option<&IniRootConfig>,
    ares: AresSetting,
) -> Result<AssetManager, String> {
    open_mod_manager_with_config_loader(root, mod_dir, all_mixes, explicit, ares, GameConfig::load)
}

fn open_mod_manager_with_config_loader(
    root: &Path,
    mod_dir: &Path,
    all_mixes: bool,
    explicit: Option<&IniRootConfig>,
    ares: AresSetting,
    load: impl FnOnce() -> anyhow::Result<GameConfig>,
) -> Result<AssetManager, String> {
    let base = resolve_ini_config(explicit, load)?;
    let definition = crate::util::mod_definition::ModDefinition::read(mod_dir)?;
    let resources = definition.resources(&base);
    let mut manager = AssetManager::with_mod_and_ares(
        root,
        Some(mod_dir),
        MediaArchiveMode::STOCK_DIGITAL,
        resources.effective_ares(ares)?,
        &resources.archives,
    )
    .and_then(|assets| assets.with_ini_roots(&resources.ini))
    .map_err(|error| format!("could not load mod {}: {error}", mod_dir.display()))?;
    if all_mixes {
        manager
            .load_all_disk_mixes()
            .map_err(|error| error.to_string())?;
    }
    Ok(manager)
}

/// INI expansion may be audited against a MOD/source directory that lacks
/// required retail media. This explicit tooling scope never changes new() or
/// the archive stack used by the game and normal asset commands.
pub fn open_ini_probe_manager(ra2_dir: &Path, all_mixes: bool) -> Result<AssetManager, String> {
    let mut manager = AssetManager::from_ini_probe_root(ra2_dir).map_err(|err| err.to_string())?;
    if all_mixes {
        manager
            .load_all_disk_mixes()
            .map_err(|err| err.to_string())?;
    }
    Ok(manager)
}

#[cfg(all(test, tests_asset_tools))]
mod tests {
    use super::*;
    use crate::asset_tools::verb_ini::{IniOptions, IniQuery};
    use crate::assets::asset_manager::test_support::audio_test_install;
    use crate::map::source::test_support::TestDirectory;

    #[test]
    fn metadata_only_mod_keeps_the_host_ini_configuration() {
        let install = audio_test_install("asset-mod-host-roots");
        let files: [(&str, &[u8]); 4] = [
            ("rules-alt.ini", b"[General]\nBuildSpeed=.7\n"),
            ("art-alt.ini", b"[TEST]\nVoxel=yes\n"),
            ("ai-alt.ini", b"[Marker]\nValue=custom\n"),
            ("artmd.ini", b"[TEST]\nVoxel=no\n"),
        ];
        for (name, body) in files {
            std::fs::write(install.path().join(name), body).unwrap();
        }
        let mod_dir = TestDirectory::new("asset-metadata-only-mod");
        mod_dir.write("mod.ini", b"[Mod]\nName=Metadata only\n");
        let host = TestDirectory::new("asset-host-config");
        host.write(
            "config.toml",
            format!(
                "[paths]\nra2_dir='{}'\nsettings_dir='{}'\n[ini]\n\
                 profile='custom'\nrules='rules-alt.ini'\nart='art-alt.ini'\nai='ai-alt.ini'\n",
                install.path().display(),
                host.path().join("settings").display()
            )
            .as_bytes(),
        );
        let mut manager = open_mod_manager_with_config_loader(
            install.path(),
            mod_dir.path(),
            false,
            None,
            AresSetting::Auto,
            || GameConfig::load_from(host.path(), &host.path().join("asset")),
        )
        .unwrap();
        let report = crate::asset_tools::verb_ini::run(
            &mut manager,
            install.path(),
            IniQuery::Accessor {
                section: "TEST",
                key: "Voxel",
            },
            &IniOptions {
                domain: Some("art".into()),
                reader: Some("bool".into()),
                default: Some("false".into()),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(report["ini_roots"]["art"], "art-alt.ini");
        assert_eq!(report["accessor_result"], true);
    }
}
