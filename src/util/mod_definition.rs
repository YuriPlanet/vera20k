//! VERA's `mod.ini`: display metadata and the files a selected mod loads.
//!
//! This is a VERA UTF-8 manifest, not a gamemd INI. The same bounded reader
//! serves the Mods page, game startup and launch probes on every platform.

// Outside data reaches this module: it refuses bad input, never panics.
#![cfg_attr(
    not(test),
    deny(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::unreachable,
        clippy::indexing_slicing
    )
)]

use std::{path::Path, sync::Arc};

use crate::assets::client_install::ClientInstall;

use super::archive_names::ArchiveNames;
use super::ares_launch::AresSetting;
use super::file_system;
use super::ini_roots::{IniProfile, IniRootConfig, IniRoots};
use super::mod_package::ModFolder;

const MANIFEST: &str = "mod.ini";
const FILE_LIMIT: u64 = 64 * 1024;
const TEXT_LIMIT: usize = 300;

/// The launch's resource choices. Retained by the process asset owner after
/// selection, so recovery does not read a changed manifest.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ResourceConfig {
    pub ini: IniRootConfig,
    pub archives: ArchiveNames,
    pub requires_ares: bool,
    pub requires_kratos: bool,
    /// The existing launcher catalog of an imported client folder, shared
    /// with the provider and retained if its archive stack is recovered.
    pub client_install: Option<Arc<ClientInstall>>,
    /// The selected files, kept alive if this launch assembled package parts.
    pub mod_folder: Option<ModFolder>,
}

impl ResourceConfig {
    pub fn from_ini(ini: IniRootConfig) -> Self {
        Self {
            ini,
            ..Self::default()
        }
    }

    /// A mod's requirement belongs to this launch, never to the saved player
    /// setting. Explicit Off refuses the mod rather than misreading its rules.
    /// A mod that declares `[Extensions] Kratos=yes` plays with Kratos on for
    /// this launch. An explicit Off refuses it, as it refuses an Ares mod.
    pub fn effective_kratos(
        &self,
        setting: crate::util::kratos_launch::KratosSetting,
    ) -> Result<crate::util::kratos_launch::KratosSetting, String> {
        use crate::util::kratos_launch::KratosSetting;
        match (self.requires_kratos, setting) {
            (true, KratosSetting::Off) => Err(
                "This mod requires Kratos. Set Kratos to Auto or On in Options, restart, and choose the mod again."
                    .into(),
            ),
            (true, KratosSetting::Auto) => Ok(KratosSetting::On),
            _ => Ok(setting),
        }
    }

    pub fn effective_ares(&self, setting: AresSetting) -> Result<AresSetting, String> {
        match (self.requires_ares, setting) {
            (true, AresSetting::Off) => Err(
                "This mod requires Ares. Set Ares to Auto or On in Options, restart, and choose the mod again."
                    .into(),
            ),
            (true, AresSetting::Auto) => Ok(AresSetting::On),
            _ => Ok(setting),
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ModDefinition {
    pub name: Option<String>,
    pub version: Option<String>,
    pub author: Option<String>,
    pub about: Option<String>,
    pub terms: Option<String>,
    pub needs: Option<String>,
    files: Option<IniRootConfig>,
    archives: ArchiveNames,
    requires_ares: bool,
    requires_kratos: bool,
    explicit_loading: bool,
}

impl ModDefinition {
    /// Missing manifests preserve ordinary folder mods. An existing manifest
    /// with invalid loading declarations is refused with its file name.
    pub fn read(dir: &Path) -> Result<Self, String> {
        let Some(path) = super::case_insensitive_path::find(dir, MANIFEST) else {
            return Ok(Self::default());
        };
        let metadata = file_system::metadata(&path)
            .map_err(|error| format!("Cannot read {}: {error}", path.display()))?;
        if !metadata.is_file() || metadata.len() > FILE_LIMIT {
            return Err(format!(
                "{} must be a file smaller than 64 KiB",
                path.display()
            ));
        }
        let bytes = file_system::read(&path)
            .map_err(|error| format!("Cannot read {}: {error}", path.display()))?;
        Self::parse(&String::from_utf8_lossy(&bytes))
            .map_err(|error| format!("{}: {error}", path.display()))
    }

    pub fn parse(text: &str) -> Result<Self, String> {
        let mut definition = Self::default();
        let mut section = String::new();
        for line in text.trim_start_matches('\u{feff}').lines() {
            let line = line.split(';').next().unwrap_or_default().trim();
            if let Some(header) = line.strip_prefix('[') {
                section = header
                    .strip_suffix(']')
                    .ok_or("A section header must end with ]")?
                    .trim()
                    .to_ascii_lowercase();
                if section == "files" {
                    definition.files.get_or_insert_with(stock_files);
                }
                definition.explicit_loading |=
                    matches!(section.as_str(), "files" | "archives" | "extensions");
                continue;
            }
            let Some((key, value)) = line.split_once('=') else {
                if !line.is_empty()
                    && matches!(section.as_str(), "files" | "archives" | "extensions")
                {
                    return Err(format!("[{section}] expected Key=value, found {line:?}"));
                }
                continue;
            };
            let key = key.trim().to_ascii_lowercase();
            let value = value.trim();
            match section.as_str() {
                "mod" => definition.read_metadata(&key, value),
                "files" => {
                    let files = definition.files.get_or_insert_with(stock_files);
                    let slot = match key.as_str() {
                        "rules" => &mut files.rules,
                        "art" => &mut files.art,
                        "ai" => &mut files.ai,
                        "sound" => &mut files.sound,
                        "eva" => &mut files.eva,
                        "theatersuffix" => &mut files.theater_suffix,
                        "theme" => &mut files.theme,
                        "battle" => &mut files.battle,
                        "mission" => &mut files.mission,
                        "mapselect" => &mut files.mapsel,
                        "modes" => &mut files.modes,
                        "mapcatalog" => &mut files.map_catalog,
                        _ => continue,
                    };
                    if value.is_empty() {
                        return Err(format!("[Files] {key} needs a file name"));
                    }
                    *slot = Some(value.to_owned());
                }
                "archives" => {
                    let slot = match key.as_str() {
                        "expansionprefix" => &mut definition.archives.expansion_prefix,
                        "mapprefix" => &mut definition.archives.map_prefix,
                        "multiplayer" => &mut definition.archives.multiplayer,
                        "theme" => &mut definition.archives.theme,
                        "movieprefix" => &mut definition.archives.movie_prefix,
                        _ => continue,
                    };
                    *slot = value.to_owned();
                }
                "extensions" => {
                    let required = match value.to_ascii_lowercase().as_str() {
                        "yes" | "true" | "1" => true,
                        "no" | "false" | "0" => false,
                        _ => return Err(format!("[Extensions] {key} must be yes or no")),
                    };
                    if key == "ares" {
                        definition.requires_ares = required;
                    } else if key == "kratos" {
                        definition.requires_kratos = required;
                    } else if required {
                        return Err(format!(
                            "This build does not support required extension '{key}'. Use a compatible build or mod."
                        ));
                    }
                }
                _ => {}
            }
        }
        if let Some(files) = &definition.files {
            files
                .validate()
                .map_err(|error| format!("[Files]: {error}"))?;
        }
        definition
            .archives
            .validate()
            .map_err(|error| format!("[Archives]: {error}"))?;
        Ok(definition)
    }

    fn read_metadata(&mut self, key: &str, value: &str) {
        let slot = match key {
            "name" => &mut self.name,
            "version" => &mut self.version,
            "author" => &mut self.author,
            "about" => &mut self.about,
            "terms" => &mut self.terms,
            "needs" => &mut self.needs,
            _ => return,
        };
        let value: String = value.chars().take(TEXT_LIMIT).collect();
        *slot = Some(value)
            .filter(|value| !value.is_empty() && (key == "needs" || !value.contains('\u{fffd}')));
    }

    pub fn title(&self, folder: &str) -> String {
        match (self.name.as_deref().unwrap_or(folder), &self.version) {
            (name, Some(version)) => format!("{name} {version}"),
            (name, None) => name.to_owned(),
        }
    }

    /// A metadata-only manifest inherits the host's existing INI choice.
    /// Declaring [Files] selects its own complete stock-based set instead.
    pub fn resources(&self, base: &IniRootConfig) -> ResourceConfig {
        ResourceConfig {
            ini: self.files.clone().unwrap_or_else(|| base.clone()),
            archives: self.archives.clone(),
            requires_ares: self.requires_ares,
            requires_kratos: self.requires_kratos,
            client_install: None,
            mod_folder: None,
        }
    }

    /// Even an empty loading section is an explicit choice of its defaults.
    pub fn declares_loading(&self) -> bool {
        self.explicit_loading
    }
}

fn stock_files() -> IniRootConfig {
    let stock = IniRoots::default();
    IniRootConfig {
        profile: IniProfile::Custom,
        rules: Some(stock.rules().into()),
        art: Some(stock.art().into()),
        ai: Some(stock.ai().into()),
        ..IniRootConfig::default()
    }
}

#[cfg(all(test, tests_util))]
mod tests {
    use super::*;

    #[test]
    fn unsupported_required_extensions_are_refused() {
        assert!(ModDefinition::parse("[Extensions]\nOtherEngine=yes\n").is_err());
        assert!(ModDefinition::parse("[Extensions]\nOtherEngine=no\n").is_ok());
        let kratos = ModDefinition::parse("[Extensions]\nKratos=yes\n").unwrap();
        assert!(kratos.resources(&IniRootConfig::default()).requires_kratos);
        let definition = ModDefinition::parse("[Extensions]\nAres=yes\n").unwrap();
        assert!(
            definition
                .resources(&IniRootConfig::default())
                .requires_ares
        );
    }
}
