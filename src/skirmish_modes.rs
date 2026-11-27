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

/// Orthogonal capability and room-behavior policy owned by a multiplayer mode
/// class. Each variant supplies its exact policy row so callers query a
/// capability directly instead of branching on mode names.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ModePolicy {
    /// Native vtable `+0x40`, required by Skirmish's game-type dropdown and AI
    /// slot admission (`0x005D6130` at `0x005D6263`). Battle (`0x005C0E20`) and
    /// FreeForAll (`0x005C5E30`) answer true; ManBattle, Siege, Unholy and
    /// Cooperative keep base `0x005D6360` (false).
    pub lists_in_skirmish: bool,
    /// Native vtable `+0xBC`: admits observer seats (`0x005C0FD0` returns true
    /// for ordinary modes; `0x005C4F00` returns false for Cooperative).
    pub observers_allowed: bool,
    /// Native vtable `+0x98`: mode-level player cap composed with map capacity by
    /// `0x005EC3A0`/`0x005E9E70` (`0x005C42C0` returns 2; ordinary `0x005C0F30`
    /// returns `-1`, no extra cap).
    pub setup_player_limit: Option<usize>,
    /// Native vtable `+0x4`: whether an all-allied room is allowed to start
    /// (`0x005C4EE0` returns true only for Cooperative; `0x005C0E30` false).
    pub permits_all_allied: bool,
    /// Native vtable `+0x4` and `+0x8`, which the sidebar Diplomacy button asks
    /// before choosing mission briefing over Diplomacy in LAN/WOL
    /// (`0x00653918..0x00653938`). Cooperative (`0x007EE27C`) overrides base
    /// `0x005C0E30`/`0x005C0E40` (false) with `0x005C4EE0`/`0x005C4EF0` (true).
    pub restates_mission_for_diplomacy: bool,
    /// At launch, pair human Houses with humans and AI Houses with AI.
    pub allies_starting_houses_by_role: bool,
    /// Whether VERA's network room seats players into numbered teams when
    /// alliances are allowed outside clan rooms.
    pub room_teams: bool,
    /// Whether multiple players may pick the same start position to share a
    /// House in network setup.
    pub shared_start_positions: bool,
    /// Native `GameExit` mode `vt+0x8`: in a LAN or WOL game, skips the score
    /// dialog and its Scenario RNG draws (`0x00685843..0x0068586A`).
    pub skips_network_score_dialog: bool,
    /// Native vtable `+0x4` and `+0x8`, both of which `MPlayer_Defeated` asks
    /// in a LAN or WOL game (`0x004FC5CF..0x004FC5F1`): when all surviving
    /// contenders are allied and a human survives, defeated human allies win
    /// too.
    pub shared_allied_victory: bool,
    /// Whether VERA's mod additions apply to authored maps in this mode.
    pub allows_mod_map_additions: bool,
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

    /// The class's `mpmodes` section name (`Battle`, `FreeForAll`, ...).
    pub fn section(self) -> &'static str {
        Self::SECTIONS
            .iter()
            .find(|(class, _)| *class == self)
            .map_or("", |(_, section)| section)
    }

    /// Exhaustive mode policy row for this multiplayer mode class.
    pub const fn policy(self) -> ModePolicy {
        match self {
            Self::Battle | Self::FreeForAll => ModePolicy {
                lists_in_skirmish: true,
                observers_allowed: true,
                setup_player_limit: None,
                permits_all_allied: false,
                restates_mission_for_diplomacy: false,
                allies_starting_houses_by_role: false,
                room_teams: true,
                shared_start_positions: true,
                skips_network_score_dialog: false,
                shared_allied_victory: false,
                allows_mod_map_additions: true,
            },
            Self::ManBattle | Self::Siege | Self::Unholy => ModePolicy {
                lists_in_skirmish: false,
                observers_allowed: true,
                setup_player_limit: None,
                permits_all_allied: false,
                restates_mission_for_diplomacy: false,
                allies_starting_houses_by_role: false,
                room_teams: true,
                shared_start_positions: true,
                skips_network_score_dialog: false,
                shared_allied_victory: false,
                allows_mod_map_additions: true,
            },
            Self::Cooperative => ModePolicy {
                lists_in_skirmish: false,
                observers_allowed: false,
                setup_player_limit: Some(2),
                permits_all_allied: true,
                restates_mission_for_diplomacy: true,
                allies_starting_houses_by_role: true,
                room_teams: false,
                shared_start_positions: false,
                skips_network_score_dialog: true,
                shared_allied_victory: true,
                allows_mod_map_additions: false,
            },
        }
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
    #[error("{name} is missing from the configured archive stack")]
    MissingRoster { name: String },
    #[error("{name} from {archive} could not be parsed: {message}")]
    InvalidRoster {
        name: String,
        archive: String,
        message: String,
    },
    #[error("{name} from {archive} did not contain any usable mode rows")]
    EmptyRoster { name: String, archive: String },
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
    /// `+0x3D` `WonlineTournamentAllowed`: listed in WOL tournament and clan
    /// rooms (`0x005D6203`).
    pub wonline_tournament_allowed: bool,
    /// `+0x3E` `WonlineClanTournamentAllowed`: listed in WOL clan rooms
    /// (`0x005D6230`).
    pub wonline_clan_tournament_allowed: bool,
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
            // The constructor's +0x3D/+0x3E (0x005D5BEA..0x005D5BF6).
            wonline_tournament_allowed: true,
            wonline_clan_tournament_allowed: true,
        })
    }
}

fn apply_common_override(mode: &mut SkirmishGameMode, ini: &IniFile) {
    // ReadBool over each current value (`0x005D5C7D..0x005D5D11`).
    let section = ini.section_or_empty("MultiplayerDialogSettings");
    mode.wonline_tournament_allowed =
        section.read_bool("WonlineTournamentAllowed", mode.wonline_tournament_allowed);
    mode.wonline_clan_tournament_allowed = section.read_bool(
        "WonlineClanTournamentAllowed",
        mode.wonline_clan_tournament_allowed,
    );
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

#[cfg(any(test, feature = "test-support"))]
pub fn parse_mpmodes_ini(ini: &IniFile) -> Vec<SkirmishGameMode> {
    parse_mpmodes_ini_with_overrides(ini, |_| None)
}

pub fn skirmish_modes_from_assets(
    assets: &AssetManager,
) -> Result<Vec<SkirmishGameMode>, SkirmishModeLoadError> {
    let name = assets.ini_roots().modes();
    let (data, source) =
        assets
            .get_with_source(name)
            .ok_or_else(|| SkirmishModeLoadError::MissingRoster {
                name: name.to_owned(),
            })?;
    log::info!("Loading {} ({} bytes) from {}", name, data.len(), source);
    let roster_ini =
        IniFile::from_bytes(&data).map_err(|err| SkirmishModeLoadError::InvalidRoster {
            name: name.to_owned(),
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
    let mut modes = parse_mpmodes_ini_with_overrides(roster_ini, |name| {
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
    if let Some(client) = assets.client_install() {
        modes.retain_mut(|mode| {
            // The native mode supplies gameplay policy. The client list
            // tells us which of those modes can use its supplied maps.
            mode.random_maps_allowed = false;
            mode.class.policy().lists_in_skirmish
                && client.maps.iter().any(|map| {
                    map.game_modes
                        .iter()
                        .any(|name| name.eq_ignore_ascii_case(&mode.map_filter))
                })
        });
    }

    if modes.is_empty() {
        Err(SkirmishModeLoadError::EmptyRoster {
            name: assets.ini_roots().modes().to_owned(),
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
            "mpfreeforallmd.ini" => "AlliesAllowed=no\nWonlineClanTournamentAllowed=no\n",
            "mpcoopmd.ini" => "AlliesAllowed=no\nWonlineTournamentAllowed=no\n",
            _ => "AlliesAllowed=yes\n",
        };
        Some(IniFile::from_str(&format!(
            "[MultiplayerDialogSettings]\n{settings}"
        )))
    })
}

/// Default for generic process consumers that have no selected mode: prefer
/// the loaded row at the usual id, then the first row the Skirmish menu admits.
/// This never manufactures a stock mode for a custom catalog.
pub fn default_skirmish_mode(modes: &[SkirmishGameMode]) -> Option<&SkirmishGameMode> {
    mode_by_id(modes, crate::skirmish_launch::BATTLE_MODE_ID)
        .filter(|mode| mode.class.policy().lists_in_skirmish)
        .or_else(|| {
            modes
                .iter()
                .find(|mode| mode.class.policy().lists_in_skirmish)
        })
}

pub fn mode_by_id(modes: &[SkirmishGameMode], id: i32) -> Option<&SkirmishGameMode> {
    modes.iter().find(|mode| mode.id == id)
}

#[cfg(all(test, tests_scenario))]
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
            .filter(|mode| mode.class.policy().lists_in_skirmish)
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
    fn stock_launch_rows_are_the_retail_roster_rows() {
        use crate::rules::retail_ini_fixture::retail_ini;
        use crate::skirmish_launch::SkirmishLaunchMode;
        let Some(roster) = retail_ini("mpmodesmd.ini") else {
            return;
        };
        let modes = parse_mpmodes_ini_with_overrides(&roster, retail_ini);
        assert_eq!(modes.len(), 9);
        for mode in &modes {
            assert_eq!(
                SkirmishLaunchMode::stock(mode.id),
                Some(SkirmishLaunchMode::from_game_mode(mode)),
                "{}",
                mode.ui_name_key
            );
        }
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
        // The WOL room filter's retail inputs (FillModeList 5D6203..37).
        let by_class = |class| modes.iter().find(|mode| mode.class == class).unwrap();
        assert!(!by_class(MpModeClass::FreeForAll).wonline_clan_tournament_allowed);
        assert!(by_class(MpModeClass::FreeForAll).wonline_tournament_allowed);
        assert!(!by_class(MpModeClass::Cooperative).wonline_tournament_allowed);
    }
}
