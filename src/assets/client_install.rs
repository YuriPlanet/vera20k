//! Read portable CnCNet client catalogs without running their launcher.
//!
//! Source: https://github.com/CnCNet/xna-cncnet-client/tree/develop/DXMainClient/Domain/Multiplayer
//! MapLoader.cs (LoadMultiMapsAsync/LoadGameModes), GameMode.cs (Initialize/
//! GetMapRulesIniFiles), Map.cs, and GameModeMapBase.cs. These UTF-8 client metadata
//! files are not gamemd INIs; their plural GameModes and relative paths are
//! interpreted here, before the existing game readers consume selected files.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use crate::util::{case_insensitive_path, file_system};

const MAX_INI_BYTES: usize = 4 * 1024 * 1024;
const MAX_LINE_BYTES: usize = 4096;
const MAX_ENTRIES: usize = 65_536;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ClientInstall {
    pub maps: Vec<ClientMap>,
    pub modes: Vec<ClientMode>,
    pub theme: ClientTheme,
}

/// Optional launcher artwork for equivalent VERA screens.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ClientTheme {
    pub startup_splash: Option<PathBuf>,
    pub skirmish_background: Option<PathBuf>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ClientMap {
    /// Actual spelling relative to the selected folder, with `/` separators.
    pub file_name: String,
    pub display_name: Option<String>,
    pub game_modes: Vec<String>,
    pub min_players: Option<u8>,
    pub max_players: Option<u8>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ClientMode {
    pub name: String,
    pub map_code: Option<PathBuf>,
}

impl ClientInstall {
    /// None means this is not a portable client folder. An invalid catalog or
    /// escaping path is an error; unsupported option-bearing rows are omitted.
    pub fn read(root: &Path) -> Result<Option<Self>, String> {
        let root = canonical(root)?;
        let Some(definitions) = resolve_file(&root, "Resources/ClientDefinitions.ini")? else {
            return Ok(None);
        };
        let definitions = ClientIni::read(&definitions)?;
        let catalog_name = definitions
            .value("Settings", "MPMapsPath")
            .filter(|name| !name.is_empty());
        let Some(catalog) = resolve_file(&root, catalog_name.unwrap_or("INI/MPMaps.ini"))? else {
            return match catalog_name {
                Some(name) => Err(format!("Client map catalog is unavailable: {name}")),
                None => Ok(None),
            };
        };
        let catalog = ClientIni::read(&catalog)?;
        let extension = definitions
            .value("Settings", "MapFileExtension")
            .unwrap_or("map");
        if extension.is_empty() || !extension.bytes().all(|c| c.is_ascii_alphanumeric()) {
            return Err("[Settings] MapFileExtension must be a plain file extension".into());
        }

        let mut modes = Vec::new();
        let mut mode_names = BTreeSet::new();
        for (_, name) in catalog.entries("GameModes") {
            if name.is_empty()
                || unsupported(&catalog, name, true)
                || !mode_names.insert(name.to_ascii_lowercase())
            {
                continue;
            }
            let Some(mode) = read_mode(&root, &catalog, name)? else {
                continue;
            };
            modes.push(mode);
        }
        let mode_names: BTreeMap<_, _> = modes
            .iter()
            .map(|mode| (mode.name.to_ascii_lowercase(), mode.name.as_str()))
            .collect();
        let mut maps = Vec::new();
        for (_, name) in catalog.entries("MultiMaps") {
            if name.is_empty() || unsupported(&catalog, name, false) {
                continue;
            }
            let map_path = resolve_file(&root, &format!("{name}.{extension}"))?;
            let Some(map_path) = map_path else { continue };
            let memberships = catalog.value(name, "GameModes").unwrap_or("Default");
            let game_modes: Vec<String> = memberships
                .split(',')
                .map(str::trim)
                .flat_map(|name| {
                    catalog
                        .value("GameModeAliases", name)
                        .unwrap_or(name)
                        .split(',')
                })
                .filter_map(|name| mode_names.get(&name.trim().to_ascii_lowercase()))
                .map(|name| (*name).to_owned())
                .collect();
            if game_modes.is_empty() {
                continue;
            }
            let (Ok(min_players), Ok(max_players)) = (
                player_count(
                    &catalog,
                    name,
                    &["ClientMinPlayer", "MinPlayers", "MinPlayer"],
                ),
                player_count(
                    &catalog,
                    name,
                    &["ClientMaxPlayer", "MaxPlayers", "MaxPlayer"],
                ),
            ) else {
                continue;
            };
            if min_players
                .zip(max_players)
                .is_some_and(|(min, max)| min > max)
                || catalog.enabled(name, "EnforceMinPlayers")
                    && min_players.is_none_or(|min| min > 2)
            {
                continue;
            }
            let relative = map_path
                .strip_prefix(&root)
                .map_err(|_| "Map is outside the selected folder")?;
            let file_name = relative
                .components()
                .map(|part| {
                    part.as_os_str()
                        .to_str()
                        .ok_or("Client map path is not UTF-8")
                })
                .collect::<Result<Vec<_>, _>>()?
                .join("/");
            maps.push(ClientMap {
                file_name,
                display_name: catalog
                    .value(name, "Description")
                    .filter(|text| !text.is_empty())
                    .map(str::to_owned),
                game_modes,
                min_players,
                max_players,
            });
        }
        Ok(Some(Self {
            maps,
            modes,
            theme: read_theme(&root, &definitions),
        }))
    }
}

fn theme_entry(value: &str) -> Option<(&str, &str)> {
    let (name, path) = value.split_once(',')?;
    let name = name.trim();
    (!name.is_empty()).then_some((name, path.trim()))
}

fn read_theme(root: &Path, definitions: &ClientIni) -> ClientTheme {
    // CnCNet ClientConfiguration.cs and UserINISettings.cs: [Themes] lists
    // name,directory pairs. The user's Theme defaults to entry 0.
    // https://github.com/CnCNet/xna-cncnet-client/tree/0ebe0d60/ClientCore
    let settings_file = definitions
        .value("Settings", "SettingsFile")
        .unwrap_or("Settings.ini");
    let settings = resolve_file(root, settings_file)
        .ok()
        .flatten()
        .and_then(|path| ClientIni::read(&path).ok());
    let selected = settings
        .as_ref()
        .and_then(|ini| ini.value("MultiPlayer", "Theme"))
        .and_then(|selection| {
            definitions
                .entries("Themes")
                .iter()
                .filter_map(|(_, value)| theme_entry(value))
                .find(|(name, _)| *name == selection)
                // Older packages store the numbered entry in Theme.
                .or_else(|| {
                    let index = selection.parse::<u32>().ok()?;
                    theme_entry(definitions.value("Themes", &index.to_string())?)
                })
        });
    let directory = selected
        .or_else(|| definitions.value("Themes", "0").and_then(theme_entry))
        .map(|(_, directory)| directory)
        .filter(|directory| !directory.is_empty());

    // Startup.cs and GameClass.cs search the chosen resource directory
    // before the base Resources. Theme directories can end in a separator.
    let resource = |name: &str| {
        path_parts(name).ok()?;
        if let Some(directory) = directory {
            let directory = directory.trim_end_matches(['/', '\\']);
            let path = resolve_file(root, &format!("Resources/{directory}/{name}")).ok()?;
            if path.is_some() {
                return path;
            }
        }
        resolve_file(root, &format!("Resources/{name}"))
            .ok()
            .flatten()
    };

    // GameLobbyBase.Initialize loads gamelobbybg.png before loading the
    // window's INI. INItializableWindow reads the named theme INI first.
    let skirmish = resource("SkirmishLobby.ini").and_then(|path| ClientIni::read(&path).ok());
    let background = skirmish
        .as_ref()
        .and_then(|ini| ini.value("SkirmishLobby", "BackgroundTexture"))
        .unwrap_or("gamelobbybg.png");
    ClientTheme {
        startup_splash: resource("splashScreen.png"),
        skirmish_background: resource(background),
    }
}

// Missing explicitly named code omits the mode. A missing implicit default
// remains None, as the client's IniFile constructor reads it as an empty file.
fn read_mode(root: &Path, ini: &ClientIni, name: &str) -> Result<Option<ClientMode>, String> {
    let map_code = if let Some(legacy) = ini
        .value(name, "CustomIniPath")
        .filter(|value| !value.is_empty())
    {
        match resolve_file(root, legacy)? {
            Some(path) => Some(path),
            None => {
                let parts = path_parts(legacy)?;
                if parts.len() != 2 || !parts[0].eq_ignore_ascii_case("INI") {
                    return Ok(None);
                }
                let Some(path) = resolve_file(root, &format!("INI/Map Code/{}", parts[1]))? else {
                    return Ok(None);
                };
                Some(path)
            }
        }
    } else {
        let explicit = ini
            .value(name, "MapCodeIniName")
            .filter(|value| !value.is_empty());
        let default = format!("{name}.ini");
        let code_name = explicit.unwrap_or(&default);
        // Validate before prefixing, so an absolute path cannot become relative.
        path_parts(code_name)?;
        let code = resolve_file(root, &format!("INI/Map Code/{code_name}"))?;
        if explicit.is_some() && code.is_none() {
            return Ok(None);
        }
        code
    };
    Ok(Some(ClientMode {
        name: name.to_owned(),
        map_code,
    }))
}

fn unsupported(ini: &ClientIni, section: &str, mode: bool) -> bool {
    let has = |key| {
        ini.value(section, key)
            .is_some_and(|value| !value.is_empty())
    };
    if [
        "ForcedOptions",
        "ForcedSpawnIniOptions",
        "BaseSection",
        "AllowedStartingLocations",
        "DisallowedPlayerSides",
        "DisallowedHumanPlayerSides",
        "DisallowedComputerPlayerSides",
        "RandomizedMapCodeIniNames",
        "ExtraIniName",
        "MinPlayersOverride",
        "MaxPlayersOverride",
        "CoopDifficultyLevel",
        "Bases",
    ]
    .iter()
    .any(|key| has(key))
        || [
            "MultiplayerOnly",
            "ClientMultiplayerOnly",
            "HumanPlayersOnly",
            "IsCoopMission",
            "ForceRandomStartLocations",
            "ForceNoTeams",
            "UseDifficultyDropDown",
        ]
        .iter()
        .any(|key| ini.enabled(section, key))
        || ["Credits", "UnitCount", "NeutralColor", "SpecialColor"]
            .iter()
            .any(|key| {
                ini.value(section, key)
                    .is_some_and(|value| !value.is_empty() && value != "-1")
            })
    {
        return true;
    }
    if !mode && has("CustomIniPath") {
        return true;
    }
    mode && (ini.enabled(section, "EnforceMinPlayers")
        || !ini
            .entries(&format!("{section}ForcedSpawnIniOptions"))
            .is_empty())
}

fn player_count(ini: &ClientIni, section: &str, keys: &[&str]) -> Result<Option<u8>, ()> {
    keys.iter()
        .find_map(|key| ini.value(section, key))
        .map(|value| value.parse::<u8>().map_err(|_| ()))
        .transpose()
}

fn path_parts(name: &str) -> Result<Vec<&str>, String> {
    let parts: Vec<_> = name.split(['/', '\\']).collect();
    if name.len() > MAX_LINE_BYTES
        || parts.iter().any(|part| {
            part.is_empty()
                || matches!(*part, "." | "..")
                || part.contains(':')
                || part.chars().any(char::is_control)
        })
    {
        return Err(format!(
            "Client path must stay inside the selected folder: {name:?}"
        ));
    }
    Ok(parts)
}

/// Resolve every component case-insensitively, checking symlinks before
/// descending. The returned spelling is the actual file, not the request.
fn resolve_file(root: &Path, name: &str) -> Result<Option<PathBuf>, String> {
    let mut path = root.to_owned();
    for part in path_parts(name)? {
        let Some(found) = case_insensitive_path::find(&path, part) else {
            return Ok(None);
        };
        path = canonical(&found)?;
        if !path.starts_with(root) {
            return Err(format!(
                "Client path resolves outside the selected folder: {name}"
            ));
        }
    }
    Ok(file_system::is_file(&path).then_some(path))
}

fn canonical(path: &Path) -> Result<PathBuf, String> {
    #[cfg(not(all(feature = "wasm", target_arch = "wasm32", target_os = "unknown")))]
    {
        file_system::canonicalize(path)
            .map_err(|error| format!("Cannot resolve {}: {error}", path.display()))
    }
    // The browser's in-memory filesystem has no symlinks. All child segments
    // have already been validated; this also builds without wasm-net.
    #[cfg(all(feature = "wasm", target_arch = "wasm32", target_os = "unknown"))]
    {
        Ok(path.to_path_buf())
    }
}

/// A bounded reader for the client's metadata syntax. Row order is retained;
/// repeated keys replace their value without changing that order.
#[derive(Default)]
struct ClientIni(BTreeMap<String, Vec<(String, String)>>);

impl ClientIni {
    fn read(path: &Path) -> Result<Self, String> {
        #[cfg(not(all(feature = "wasm", target_arch = "wasm32", target_os = "unknown")))]
        let bytes = {
            use std::io::Read;
            let mut bytes = Vec::new();
            file_system::File::open(path)
                .and_then(|file| {
                    file.take((MAX_INI_BYTES + 1) as u64)
                        .read_to_end(&mut bytes)
                })
                .map_err(|error| format!("Cannot read {}: {error}", path.display()))?;
            bytes
        };
        #[cfg(all(feature = "wasm", target_arch = "wasm32", target_os = "unknown"))]
        let bytes = file_system::read(path)
            .map_err(|error| format!("Cannot read {}: {error}", path.display()))?;
        if bytes.len() > MAX_INI_BYTES {
            return Err(format!("Client catalog exceeds 4 MiB: {}", path.display()));
        }
        let text = String::from_utf8_lossy(&bytes);
        let mut ini = Self::default();
        let mut section = String::new();
        let mut count = 0;
        // Parsing-only index avoids quadratic scans of large MultiMaps lists.
        // It is discarded once the ordered rows become immutable.
        let mut positions = BTreeMap::<(String, String), usize>::new();
        for line in text.trim_start_matches('\u{feff}').lines() {
            if line.len() > MAX_LINE_BYTES {
                return Err(format!(
                    "Client catalog line exceeds 4096 bytes: {}",
                    path.display()
                ));
            }
            let line = line.split(';').next().unwrap_or_default().trim();
            if let Some(header) = line
                .strip_prefix('[')
                .and_then(|line| line.strip_suffix(']'))
            {
                section = header.trim().to_ascii_lowercase();
            } else if let Some((key, value)) = line.split_once('=') {
                count += 1;
                if count > MAX_ENTRIES {
                    return Err(format!(
                        "Client catalog has too many entries: {}",
                        path.display()
                    ));
                }
                let entries = ini.0.entry(section.clone()).or_default();
                let key = key.trim();
                let value = value.trim().to_owned();
                let lookup = (section.clone(), key.to_ascii_lowercase());
                if let Some(index) = positions.get(&lookup).copied() {
                    entries[index].1 = value;
                } else {
                    positions.insert(lookup, entries.len());
                    entries.push((key.to_owned(), value));
                }
            }
        }
        Ok(ini)
    }

    fn entries(&self, section: &str) -> &[(String, String)] {
        self.0
            .get(&section.to_ascii_lowercase())
            .map(Vec::as_slice)
            .unwrap_or_default()
    }

    fn value(&self, section: &str, key: &str) -> Option<&str> {
        self.entries(section)
            .iter()
            .find(|(name, _)| name.eq_ignore_ascii_case(key))
            .map(|(_, value)| value.as_str())
    }

    fn enabled(&self, section: &str, key: &str) -> bool {
        self.value(section, key).is_some_and(|value| {
            !matches!(
                value.to_ascii_lowercase().as_str(),
                "" | "no" | "false" | "0"
            )
        })
    }
}

#[cfg(all(test, tests_assets))]
mod tests {
    use super::*;
    use crate::assets::asset_manager::test_support::TestDirectory;

    #[test]
    fn client_theme_uses_selected_lobby_art_and_base_splash() {
        let dir = TestDirectory::new("client-theme");
        let resources = dir.path().join("Resources");
        std::fs::create_dir_all(resources.join("Alternate/Pictures")).unwrap();
        std::fs::create_dir_all(dir.path().join("INI")).unwrap();
        std::fs::write(
            resources.join("ClientDefinitions.ini"),
            "[Themes]\n0=Default,\n1=Other,alternate/\n[Settings]\nSettingsFile=client.ini\n",
        )
        .unwrap();
        std::fs::write(dir.path().join("INI/MPMaps.ini"), "[GameModes]\n").unwrap();
        std::fs::write(resources.join("SplashScreen.PNG"), []).unwrap();
        std::fs::write(resources.join("gamelobbybg.png"), []).unwrap();
        std::fs::write(resources.join("Alternate/Pictures/Lobby.PNG"), []).unwrap();
        std::fs::write(
            resources.join("Alternate/SkirmishLobby.INI"),
            "[SkirmishLobby]\nBackgroundTexture=pictures\\lobby.png\n",
        )
        .unwrap();
        for selection in ["Other", "1"] {
            std::fs::write(
                dir.path().join("client.ini"),
                format!("[MultiPlayer]\nTheme={selection}\n"),
            )
            .unwrap();
            let theme = ClientInstall::read(dir.path()).unwrap().unwrap().theme;
            assert_eq!(
                theme,
                ClientTheme {
                    startup_splash: Some(canonical(&resources.join("SplashScreen.PNG")).unwrap()),
                    skirmish_background: Some(
                        canonical(&resources.join("Alternate/Pictures/Lobby.PNG")).unwrap()
                    ),
                }
            );
        }
    }

    #[test]
    fn invalid_theme_background_does_not_reject_a_valid_map() {
        let dir = TestDirectory::new("client-theme-invalid");
        let resources = dir.path().join("Resources");
        std::fs::create_dir_all(&resources).unwrap();
        std::fs::create_dir_all(dir.path().join("INI")).unwrap();
        std::fs::write(
            resources.join("ClientDefinitions.ini"),
            "[Themes]\n0=Default,\n",
        )
        .unwrap();
        std::fs::write(
            dir.path().join("INI/MPMaps.ini"),
            "[GameModes]\n0=Default\n[MultiMaps]\n0=Arena\n",
        )
        .unwrap();
        std::fs::write(dir.path().join("Arena.map"), []).unwrap();
        std::fs::write(dir.path().join("outside.png"), []).unwrap();
        std::fs::write(
            resources.join("SkirmishLobby.ini"),
            "[SkirmishLobby]\nBackgroundTexture=../outside.png\n",
        )
        .unwrap();

        let install = ClientInstall::read(dir.path()).unwrap().unwrap();
        assert_eq!(install.theme.skirmish_background, None);
        assert_eq!(install.maps[0].file_name, "Arena.map");
    }

    #[test]
    fn client_metadata_resolves_real_spelling_and_plain_modes() {
        let dir = TestDirectory::new("client-install");
        for name in ["rEsOuRcEs", "INI/Map Code", "Maps/Sub"] {
            std::fs::create_dir_all(dir.path().join(name)).unwrap();
        }
        std::fs::write(
            dir.path().join("rEsOuRcEs/ClientDefinitions.INI"),
            "[Settings]\nMPMapsPath=ini/maps.ini\n",
        )
        .unwrap();
        std::fs::write(dir.path().join("INI/Maps.ini"), "[GameModes]\n0=Plain\n1=Modern\n2=Locked\n3=Online\n4=Literal\n[Plain]\nCustomIniPath=INI\\Plain.ini\n[Modern]\nMapCodeIniName=modern.ini\n[Literal]\nCustomIniPath=INI\\Literal.ini\n[Locked]\nForcedOptions=Locks\n[Online]\nMultiplayerOnly=yes\n[MultiMaps]\n0=maps\\sub\\alpha\n1=maps\\sub\\locked\n2=maps\\sub\\missing\n[maps\\sub\\alpha]\nDescription=Alpha\nGameModes=Plain,Modern,Locked,Online\nMinPlayers=1\nMaxPlayers=4\n[maps\\sub\\locked]\nGameModes=Plain\nForcedOptions=Locks\n").unwrap();
        for name in [
            "INI/Map Code/Plain.ini",
            "INI/Map Code/Modern.ini",
            "INI/Literal.ini",
            "INI/Map Code/Literal.ini",
            "Maps/Sub/Alpha.MAP",
            "Maps/Sub/Locked.map",
        ] {
            std::fs::write(dir.path().join(name), "[General]\n").unwrap();
        }
        let install = ClientInstall::read(dir.path()).unwrap().unwrap();
        assert_eq!(
            install
                .modes
                .iter()
                .map(|mode| mode.name.as_str())
                .collect::<Vec<_>>(),
            ["Plain", "Modern", "Literal"]
        );
        assert_eq!(
            install.modes[0].map_code,
            Some(canonical(&dir.path().join("INI/Map Code/Plain.ini")).unwrap())
        );
        assert_eq!(
            install.modes[1].map_code,
            Some(canonical(&dir.path().join("INI/Map Code/Modern.ini")).unwrap())
        );
        assert_eq!(
            install.modes[2].map_code,
            Some(canonical(&dir.path().join("INI/Literal.ini")).unwrap())
        );
        assert_eq!(
            install.maps,
            [ClientMap {
                file_name: "Maps/Sub/Alpha.MAP".into(),
                display_name: Some("Alpha".into()),
                game_modes: vec!["Plain".into(), "Modern".into()],
                min_players: Some(1),
                max_players: Some(4)
            }]
        );
    }

    #[test]
    fn unavailable_and_escaping_client_files_are_not_imported() {
        let dir = TestDirectory::new("client-invalid");
        assert_eq!(ClientInstall::read(dir.path()).unwrap(), None);
        std::fs::create_dir_all(dir.path().join("Resources")).unwrap();
        let definitions = dir.path().join("Resources/ClientDefinitions.ini");
        std::fs::write(&definitions, "[Settings]\nMPMapsPath=missing.ini\n").unwrap();
        assert!(
            ClientInstall::read(dir.path())
                .unwrap_err()
                .contains("unavailable")
        );
        std::fs::write(&definitions, "[Settings]\nMPMapsPath=../outside.ini\n").unwrap();
        assert!(
            ClientInstall::read(dir.path())
                .unwrap_err()
                .contains("inside")
        );
        #[cfg(unix)]
        {
            let outside = TestDirectory::new("client-outside");
            std::fs::write(outside.path().join("maps.ini"), "[GameModes]\n").unwrap();
            std::os::unix::fs::symlink(
                outside.path().join("maps.ini"),
                dir.path().join("escape.ini"),
            )
            .unwrap();
            std::fs::write(&definitions, "[Settings]\nMPMapsPath=escape.ini\n").unwrap();
            assert!(
                ClientInstall::read(dir.path())
                    .unwrap_err()
                    .contains("outside")
            );
        }
    }
}
