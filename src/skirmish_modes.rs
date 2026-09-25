//! Data model for Yuri's Revenge Skirmish MPModes rows.
//!
//! This module is app/UI-facing data. It deliberately stays out of `sim/`
//! because retail mode rows are shell/session setup state, not deterministic
//! tick logic by themselves.

use crate::assets::asset_manager::AssetManager;
use crate::rules::ini_parser::IniFile;

/// The mode class an `MPModesMD.ini` section builds. The loader `0x005D7CE0`
/// reads the sections in this order, each through its own factory
/// (`0x005D8170..0x005D82B0`), and inserts every mode by id (`0x005D7590`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MpModeClass {
    Battle,
    ManBattle,
    Siege,
    Unholy,
    FreeForAll,
    Cooperative,
}

impl MpModeClass {
    const SECTIONS: [(Self, &'static str); 6] = [
        (Self::Battle, "Battle"),
        (Self::ManBattle, "ManBattle"),
        (Self::Siege, "Siege"),
        (Self::Unholy, "Unholy"),
        (Self::FreeForAll, "FreeForAll"),
        (Self::Cooperative, "Cooperative"),
    ];

    /// Vtable `+0x40`, which Skirmish's game-type list requires
    /// (`0x005D6130` at `0x005D6263`). Battle (`0x005C0E20`) and FreeForAll
    /// (`0x005C5E30`) answer true; ManBattle, Siege, Unholy and Cooperative
    /// keep the base `0x005D6360` (false).
    pub const fn lists_in_skirmish(self) -> bool {
        matches!(self, Self::Battle | Self::FreeForAll)
    }
}

#[derive(Debug, thiserror::Error)]
pub enum SkirmishModeLoadError {
    #[error("MPModesMD.ini is missing from the configured retail archive stack")]
    MissingRoster,
    #[error("MPModesMD.ini from {archive} could not be parsed: {message}")]
    InvalidRoster { archive: String, message: String },
    #[error("MPModesMD.ini from {archive} did not contain any usable mode rows")]
    EmptyRoster { archive: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkirmishGameMode {
    pub class: MpModeClass,
    pub id: i32,
    pub ui_name_key: String,
    pub tooltip_key: String,
    pub override_file: String,
    pub map_filter: String,
    pub random_maps_allowed: bool,
    pub allies_allowed: bool,
    pub must_ally: bool,
}

impl SkirmishGameMode {
    fn from_roster_row_native_defaults(class: MpModeClass, id: i32, value: &str) -> Option<Self> {
        let fields: Vec<&str> = value.split(',').map(str::trim).collect();
        if fields.len() < 5 {
            return None;
        }

        let random_maps_allowed = parse_bool(fields[4]).unwrap_or(false);
        Some(Self {
            class,
            id,
            ui_name_key: fields[0].to_string(),
            tooltip_key: fields[1].to_string(),
            override_file: fields[2].to_string(),
            map_filter: fields[3].to_string(),
            random_maps_allowed,
            allies_allowed: true,
            must_ally: false,
        })
    }
}

fn parse_bool(value: &str) -> Option<bool> {
    match value.trim().to_ascii_lowercase().as_str() {
        "yes" | "true" | "1" => Some(true),
        "no" | "false" | "0" => Some(false),
        _ => None,
    }
}

fn apply_common_override(mode: &mut SkirmishGameMode, ini: &IniFile) {
    let Some(section) = ini.section("MultiplayerDialogSettings") else {
        return;
    };
    if let Some(allies_allowed) = section.get_bool("AlliesAllowed") {
        mode.allies_allowed = allies_allowed;
    }
    if let Some(must_ally) = section.get_bool("MustAlly") {
        mode.must_ally = must_ally;
    }
    if !mode.allies_allowed {
        mode.must_ally = false;
    }
}

fn parse_mpmodes_ini_with_overrides<F>(ini: &IniFile, mut override_ini: F) -> Vec<SkirmishGameMode>
where
    F: FnMut(&str) -> Option<IniFile>,
{
    let mut modes = Vec::new();
    for (class, category) in MpModeClass::SECTIONS {
        let Some(section) = ini.section(category) else {
            continue;
        };
        for key in section.keys() {
            // The id is the key's atoi (`CString` to int `0x007B6280`).
            let id = crate::rules::ini_value::atoi_lenient(key);
            let Some(value) = section.get(key) else {
                continue;
            };
            let Some(mut mode) =
                SkirmishGameMode::from_roster_row_native_defaults(class, id, value)
            else {
                continue;
            };
            if let Some(override_ini) = override_ini(&mode.override_file) {
                apply_common_override(&mut mode, &override_ini);
            }
            modes.push(mode);
        }
    }
    modes.sort_by_key(|mode| mode.id);
    modes
}

pub fn parse_mpmodes_ini(ini: &IniFile) -> Vec<SkirmishGameMode> {
    parse_mpmodes_ini_with_overrides(ini, |_| None)
}

pub fn skirmish_modes_from_assets(
    assets: &AssetManager,
) -> Result<Vec<SkirmishGameMode>, SkirmishModeLoadError> {
    let (data, source) = assets
        .get_with_source("MPModesMD.ini")
        .ok_or(SkirmishModeLoadError::MissingRoster)?;
    log::info!(
        "Loading MPModesMD.ini ({} bytes) from {}",
        data.len(),
        source
    );
    let roster_ini =
        IniFile::from_bytes(&data).map_err(|err| SkirmishModeLoadError::InvalidRoster {
            archive: source.clone(),
            message: err.to_string(),
        })?;

    let modes = parse_mpmodes_ini_with_overrides(&roster_ini, |name| {
        let Some((data, override_source)) = assets.get_with_source(name) else {
            log::warn!(
                "Skirmish MPMode override {name} is missing; using native constructor defaults"
            );
            return None;
        };
        log::debug!(
            "Loading Skirmish MPMode override {} ({} bytes) from {}",
            name,
            data.len(),
            override_source
        );
        IniFile::from_bytes(&data)
            .map_err(|err| {
                log::warn!(
                    "Failed to parse Skirmish MPMode override {name} from \
                     {override_source}; using native constructor defaults: {err}"
                )
            })
            .ok()
    });

    if modes.is_empty() {
        Err(SkirmishModeLoadError::EmptyRoster { archive: source })
    } else {
        Ok(modes)
    }
}

#[cfg(test)]
pub(crate) fn stock_skirmish_modes() -> Vec<SkirmishGameMode> {
    const TEST_ROSTER: &str = include_str!("../tests/fixtures/ini/mpmodesmd_stock_contract.ini");
    let roster = IniFile::from_str(TEST_ROSTER);
    parse_mpmodes_ini_with_overrides(&roster, |name| {
        let settings = match name.to_ascii_lowercase().as_str() {
            "mpteammd.ini" => "AlliesAllowed=yes\nMustAlly=yes\n",
            "mpfreeforallmd.ini" | "mpcoopmd.ini" => "AlliesAllowed=no\n",
            _ => "AlliesAllowed=yes\n",
        };
        Some(IniFile::from_str(&format!(
            "[MultiplayerDialogSettings]\n{settings}"
        )))
    })
}

pub fn mode_by_id(modes: &[SkirmishGameMode], id: i32) -> Option<&SkirmishGameMode> {
    modes.iter().find(|mode| mode.id == id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_stock_mpmodesmd_roster() {
        let modes = stock_skirmish_modes();
        let ids: Vec<i32> = modes.iter().map(|mode| mode.id).collect();
        assert_eq!(ids, vec![1, 2, 3, 4, 5, 6, 7, 8, 9]);
        assert_eq!(modes[0].ui_name_key, "GUI:Battle");
        assert_eq!(modes[0].map_filter, "standard");
        assert!(modes[0].random_maps_allowed);
    }

    #[test]
    fn retail_skirmish_lists_battle_free_for_all_and_team_alliance() {
        let Some(ini) = crate::rules::retail_ini_fixture::retail_ini("mpmodesmd.ini") else {
            return;
        };
        let listed: Vec<(i32, String)> = parse_mpmodes_ini(&ini)
            .into_iter()
            .filter(|mode| mode.class.lists_in_skirmish())
            .map(|mode| (mode.id, mode.ui_name_key))
            .collect();
        assert_eq!(
            listed,
            [
                (1, "GUI:Battle".to_string()),
                (2, "GUI:FreeForAll".to_string()),
                (9, "GUI:TeamGame".to_string()),
            ]
        );
    }

    #[test]
    fn stock_mpmodes_do_not_include_siege_without_roster_row() {
        let modes = stock_skirmish_modes();
        assert!(!modes.iter().any(|mode| mode.ui_name_key.contains("Siege")));
    }

    #[test]
    fn team_game_has_must_ally() {
        let modes = stock_skirmish_modes();
        let team = mode_by_id(&modes, 9).expect("team game mode");
        assert_eq!(team.ui_name_key, "GUI:TeamGame");
        assert!(team.allies_allowed);
        assert!(team.must_ally);
        assert!(!team.random_maps_allowed);
        assert_eq!(team.map_filter, "teamgame");
    }

    #[test]
    fn free_for_all_disables_allies() {
        let modes = stock_skirmish_modes();
        let ffa = mode_by_id(&modes, 2).expect("free for all mode");
        assert_eq!(ffa.ui_name_key, "GUI:FreeForAll");
        assert!(!ffa.allies_allowed);
        assert!(!ffa.must_ally);
        assert!(ffa.random_maps_allowed);
    }

    #[test]
    fn battle_and_free_for_all_allow_random_maps() {
        let modes = stock_skirmish_modes();
        let random_ids: Vec<i32> = modes
            .iter()
            .filter(|mode| mode.random_maps_allowed)
            .map(|mode| mode.id)
            .collect();
        assert_eq!(random_ids, vec![1, 2]);
    }

    #[test]
    fn malformed_rows_are_skipped() {
        let ini = IniFile::from_str(
            "[Battle]\n1=GUI:Battle, STT:ModeBattle, MPBattleMD.ini, standard, true\n2=bad\n",
        );
        let modes = parse_mpmodes_ini(&ini);
        assert_eq!(modes.len(), 1);
        assert_eq!(modes[0].id, 1);
    }

    #[test]
    fn mpmode_missing_override_uses_native_constructor_defaults() {
        let ini = IniFile::from_str(
            "[FreeForAll]\n2=GUI:FreeForAll, STT:ModeFreeForAll, MPFreeForAllMD.ini, standard, true\n\
             [ManBattle]\n9=GUI:TeamGame, STT:ModeTeamGame, MPTeamMD.ini, teamgame, false\n",
        );

        let modes = parse_mpmodes_ini_with_overrides(&ini, |_| None);

        let ffa = mode_by_id(&modes, 2).expect("free for all");
        assert!(ffa.allies_allowed);
        assert!(!ffa.must_ally);

        let team = mode_by_id(&modes, 9).expect("team game");
        assert!(team.allies_allowed);
        assert!(!team.must_ally);
    }

    #[test]
    fn mpmode_override_clears_must_ally_when_allies_disabled() {
        let ini = IniFile::from_str(
            "[Battle]\n1=GUI:Battle, STT:ModeBattle, Custom.ini, standard, true\n",
        );
        let override_ini =
            IniFile::from_str("[MultiplayerDialogSettings]\nAlliesAllowed=no\nMustAlly=yes\n");

        let modes = parse_mpmodes_ini_with_overrides(&ini, |name| {
            (name == "Custom.ini").then(|| override_ini.clone())
        });

        assert!(!modes[0].allies_allowed);
        assert!(!modes[0].must_ally);
    }

    #[test]
    fn mpmode_override_ignores_ally_change_allowed_for_common_mode() {
        let ini = IniFile::from_str(
            "[Battle]\n1=GUI:Battle, STT:ModeBattle, Custom.ini, standard, true\n",
        );
        let override_ini =
            IniFile::from_str("[MultiplayerDialogSettings]\nAllyChangeAllowed=no\nMustAlly=yes\n");

        let modes = parse_mpmodes_ini_with_overrides(&ini, |name| {
            (name == "Custom.ini").then(|| override_ini.clone())
        });

        assert!(modes[0].allies_allowed);
        assert!(modes[0].must_ally);
    }
}
