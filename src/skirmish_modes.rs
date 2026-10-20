//! Data model for Yuri's Revenge Skirmish MPModes rows.
//!
//! This module is app/UI-facing data. It deliberately stays out of `sim/`
//! because retail mode rows are shell/session setup state, not deterministic
//! tick logic by themselves.

use crate::assets::asset_manager::AssetManager;
use crate::rules::ini_parser::{IniFile, IniSection};

/// The mode class an `MPModesMD.ini` section builds. The loader `0x005D7CE0`
/// reads the sections in this order, each through its own factory
/// (`0x005D8170..0x005D82B0`), and inserts every mode by id (`0x005D7590`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
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

    /// Vtable `+0x4` and `+0x8`, which the sidebar Diplomacy button asks
    /// before choosing the mission briefing over Diplomacy in LAN/WOL
    /// (`0x00653918..0x00653938`). Only Cooperative (vtable `0x007EE27C`)
    /// overrides the base `0x005C0E30`/`0x005C0E40` (false) with
    /// `0x005C4EE0`/`0x005C4EF0` (true).
    pub const fn restates_mission_for_diplomacy(self) -> bool {
        matches!(self, Self::Cooperative)
    }
}

/// Effects of the mode's successful Start validation. The shell/session owner
/// applies these before random assignments, preserving vtable+14 ordering.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModeStartEffect {
    None,
    /// FreeForAll5C5D40 assigns each non-observer node+6B its source index.
    FreeForAllSourceRoles,
    /// Cooperative5C1D80 binds the two names to an existing campaign record.
    CooperativePartners,
}

impl MpModeClass {
    /// Active original vtable+BC:5C0FD0 for ordinary modes,5C4F00 for Coop.
    pub const fn allows_observers(self) -> bool {
        !matches!(self, Self::Cooperative)
    }

    /// Active original vtable+4:5C4EE0 is true only for Cooperative.
    pub const fn permits_all_allied(self) -> bool {
        matches!(self, Self::Cooperative)
    }

    /// Mode-specific cap used alongside the map's capacity by5EC3A0/5E9E70.
    /// vtable+98:5C42C0 returns2; ordinary5C0F30 returns-1(no extra cap).
    pub const fn setup_player_limit(self) -> Option<usize> {
        if matches!(self, Self::Cooperative) {
            Some(2)
        } else {
            None
        }
    }

    ///5C21D0/5C2500 show Cooperative's description/difficulty controls and
    /// hide rows beyond the two human positions. Ordinary options keep their
    /// existing resource visibility; this method does not invent forced values.
    pub const fn uses_cooperative_setup(self) -> bool {
        matches!(self, Self::Cooperative)
    }

    ///5D6310/5CB400/5CA6D0/5C5D40/5C1D80 at the host's vtable+14 gate.
    /// `roles` are node+6B in human source order, not team selectors(+63/+67).
    /// A failure with no message is intentional: Unholy only returns false.
    pub fn validate_setup_start(
        self,
        bases: bool,
        roles: &[i32],
    ) -> Result<ModeStartEffect, Option<&'static str>> {
        match self {
            Self::Battle | Self::ManBattle => Ok(ModeStartEffect::None),
            Self::Unholy => {
                if bases {
                    Ok(ModeStartEffect::None)
                } else {
                    Err(None)
                }
            }
            Self::FreeForAll => Ok(ModeStartEffect::FreeForAllSourceRoles),
            Self::Cooperative => Ok(ModeStartEffect::CooperativePartners),
            Self::Siege => {
                let mut defender = false;
                let mut attackers = 0;
                for role in roles {
                    match role {
                        0 => {}
                        1 => {
                            if defender {
                                return Err(Some("MP:OnlyOneBeseiged"));
                            }
                            defender = true;
                        }
                        2 => attackers += 1,
                        _ => return Err(Some("MP:IllegalTeam")),
                    }
                }
                if !defender {
                    Err(Some("MP:NoDefender"))
                } else if attackers == 0 {
                    Err(Some("MP:NoAttackers"))
                } else {
                    Ok(ModeStartEffect::None)
                }
            }
        }
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
    /// MultiplayerGameMode5D5DC0 reads MustAlly at+3F.
    pub const fn default_human_team(&self) -> i32 {
        if self.must_ally { 0 } else { -2 }
    }

    /// MultiplayerGameMode5D5DD0 reads AlliesAllowed at+3C.
    pub const fn default_ai_team(&self) -> i32 {
        if self.allies_allowed { 3 } else { -2 }
    }

    /// `0x005D7590`: the row is ReadString into a CString (`0x00528C00`,
    /// 0x3FE bytes), then `CString::Tokenize(",")` fields trimmed of spaces
    /// ([`IniSection::read_trimmed_list`]).
    fn from_roster_row_native_defaults(
        class: MpModeClass,
        id: i32,
        section: &IniSection,
        key: &str,
    ) -> Option<Self> {
        let fields = section.read_trimmed_list(key, 0x3FF)?;
        if fields.len() < 5 {
            return None;
        }

        // Any fifth field but `false` allows random maps (`_stricmp` at
        // `0x005D796C`).
        let random_maps_allowed = !fields[4].eq_ignore_ascii_case("false");
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

fn apply_common_override(mode: &mut SkirmishGameMode, ini: &IniFile) {
    // ReadBool over each current value (`0x005D5CE5`, `0x005D5D00`).
    let section = ini.section_or_empty("MultiplayerDialogSettings");
    mode.allies_allowed = section.read_bool("AlliesAllowed", mode.allies_allowed);
    mode.must_ally = section.read_bool("MustAlly", mode.must_ally);
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
            let id = crate::rules::ini_value::crt_atoi(key);
            let Some(mut mode) =
                SkirmishGameMode::from_roster_row_native_defaults(class, id, section, key)
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

#[cfg(test)]
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

    skirmish_modes_from_selected_ini(assets, &roster_ini, &source)
}

/// Consume the same selected roster in the app and provenance-bearing tools.
/// Mode body interpretation stays here; callers supply source identity only.
pub fn skirmish_modes_from_selected_ini(
    assets: &AssetManager,
    roster_ini: &IniFile,
    source: &str,
) -> Result<Vec<SkirmishGameMode>, SkirmishModeLoadError> {
    let modes = parse_mpmodes_ini_with_overrides(roster_ini, |name| {
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
        Err(SkirmishModeLoadError::EmptyRoster {
            archive: source.to_owned(),
        })
    } else {
        Ok(modes)
    }
}

#[cfg(any(test, feature = "test-support"))]
pub fn stock_skirmish_modes() -> Vec<SkirmishGameMode> {
    let roster = IniFile::from_str(crate::test_fixture::text(
        "tests/fixtures/ini/mpmodesmd_stock_contract.ini",
    ));
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

    #[test]
    fn network_start_validation_keeps_mode_effects_and_failure_order() {
        use super::{ModeStartEffect, MpModeClass};
        assert_eq!(
            MpModeClass::Unholy.validate_setup_start(false, &[0, 0]),
            Err(None)
        );
        assert_eq!(
            MpModeClass::Battle.validate_setup_start(false, &[0, 0]),
            Ok(ModeStartEffect::None)
        );
        assert_eq!(
            MpModeClass::FreeForAll.validate_setup_start(true, &[0, -1, 0]),
            Ok(ModeStartEffect::FreeForAllSourceRoles)
        );
        assert_eq!(
            MpModeClass::Cooperative.validate_setup_start(true, &[0, 0]),
            Ok(ModeStartEffect::CooperativePartners)
        );
        assert_eq!(
            MpModeClass::Siege.validate_setup_start(true, &[1, 2]),
            Ok(ModeStartEffect::None)
        );
        assert_eq!(
            MpModeClass::Siege.validate_setup_start(true, &[1, 1, 2]),
            Err(Some("MP:OnlyOneBeseiged"))
        );
        assert_eq!(
            MpModeClass::Siege.validate_setup_start(true, &[2, 0]),
            Err(Some("MP:NoDefender"))
        );
        assert_eq!(
            MpModeClass::Siege.validate_setup_start(true, &[1, 0]),
            Err(Some("MP:NoAttackers"))
        );
        assert_eq!(
            MpModeClass::Siege.validate_setup_start(true, &[-1, 1, 2]),
            Err(Some("MP:IllegalTeam"))
        );
    }
    use super::*;

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
    #[test]
    fn stock_contract_modes_match_selected_retail_roster_and_overrides() {
        let Some((_, assets)) = crate::rules::retail_ini_fixture::retail_assets() else {
            return;
        };
        let modes = skirmish_modes_from_assets(&assets).expect("selected retail MPModes roster");
        // Fixture coverage must not silently accept the production reader's
        // tolerant missing/malformed override fallback as retail evidence.
        for mode in &modes {
            if !mode.override_file.is_empty() {
                crate::rules::retail_sources::select_ini(&assets, &mode.override_file)
                    .expect("every stock roster override is present and parseable");
            }
        }
        assert_eq!(
            modes,
            stock_skirmish_modes(),
            "every consumed roster and override field"
        );
    }
}
