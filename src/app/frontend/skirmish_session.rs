//! Offline Skirmish shell process state and close-transaction helpers.
//!
//! The UI owns raw controls, including Random sentinels. This app-layer owner
//! retains the durable `[Skirmish]` snapshot, the process-continuity Scenario
//! RNG, and Cooperative progress records across shell closes and matches.

mod cooperative;
mod persistence;

use std::io;
use std::path::{Path, PathBuf};

use crate::app::loading::init::MapMenuEntry;
use crate::assets::asset_manager::AssetManager;
use crate::rules::ini_parser::IniFile;
use crate::sim::rng::SimRng;
use crate::skirmish_launch::{
    LaunchCountry, RandomCountryRole, ShellRandomAssignmentState, SkirmishLaunchSession,
};
use crate::skirmish_modes::{SkirmishGameMode, mode_by_id};
use crate::ui::skirmish_shell::{
    PlayerNameEditState, SkirmishAiRowType, SkirmishShellState, SkirmishTrackbarId,
    game_speed_from_visual_position, game_speed_visual_position,
};
use cooperative::{
    CooperativeCountryRole, CooperativeCountryRosterEntry, CooperativeError,
    CooperativeProgressRecord, CooperativeProgressState, CooperativeRegistry,
    draw_country_for_progress,
};
pub(crate) use persistence::SetupProfile;
use persistence::{
    MultiplayerSetupSnapshot, SKIRMISH_PERSISTED_SLOT_COUNT, SkirmishGlobalDefaults,
    SkirmishPersistedSlot, read_setup_snapshot,
};

const RA2MD_INI: &str = "RA2MD.INI";
const CONCRETE_ITEM_DATA: i32 = -1;
const RANDOM_ITEM_DATA: i32 = -2;
const MULTIPLAYER_HANDLE_UNITS: usize = 0x14;
const MULTIPLAYER_HANDLE_LIMIT_BYTES: usize = 19;
const DEFAULT_MULTIPLAYER_HANDLE: &[u8] = b"[New Player]";
const DEFAULT_MULTIPLAYER_GAME_MODE: i32 = 1;

#[derive(Debug, Clone, PartialEq, Eq)]
struct LocalMultiplayerPreferences {
    handle_bytes: Vec<u8>,
    side: i32,
    side_ex: i32,
    color_index: i32,
    color_ex: i32,
    game_mode: i32,
    /// Session+0x1FC1 / +0x1FC0, `[MultiPlayer] WOLScrollText` and
    /// `LANScrollText`: Add_Message's typing reveal in GameMode 4 / 3.
    wol_scroll_text: bool,
    lan_scroll_text: bool,
}

impl Default for LocalMultiplayerPreferences {
    fn default() -> Self {
        Self {
            handle_bytes: DEFAULT_MULTIPLAYER_HANDLE.to_vec(),
            side: 0,
            side_ex: 0,
            color_index: 0,
            color_ex: 0,
            game_mode: DEFAULT_MULTIPLAYER_GAME_MODE,
            // The Session constructor sets both (0x006974B1, 0x006974BA).
            wol_scroll_text: true,
            lan_scroll_text: true,
        }
    }
}

impl LocalMultiplayerPreferences {
    /// Apply the six fields written by `SessionClass__WriteMultiPlayerSettings`
    /// at 0x006990A0. The reader at 0x006980C0 first establishes a complete
    /// Session cache from constructor/string-table defaults, so pump/dialog
    /// exits still write all six fields when keys were absent from RA2MD.INI.
    fn update_ini_bytes(&self, content: &[u8], side_name: &str) -> Vec<u8> {
        let handle = encode_multiplayer_handle(&self.handle_bytes);
        let color = self.color_index.to_string();
        let color_ex = self.color_ex.to_string();
        let side_ex = self.side_ex.to_string();
        let game_mode = self.game_mode.to_string();
        let updates = [
            ("Handle", handle.as_str()),
            ("Color", color.as_str()),
            ("ColorEx", color_ex.as_str()),
            ("Side", side_name),
            ("SideEx", side_ex.as_str()),
            ("GameMode", game_mode.as_str()),
        ];
        crate::util::ini_writer::set_ini_values(content, "MultiPlayer", &updates)
    }
}

/// Borrowed presentation of the active canonical Cooperative progress record.
pub(crate) struct CooperativeSetupView<'a> {
    pub player_countries: &'a [String],
    pub campaign_index: usize,
    pub stage_index: usize,
    pub difficulty: i32,
}

/// App-owned state whose lifetime matches the front-end process, not one map.
pub(crate) struct MultiplayerShellRuntime {
    snapshot: MultiplayerSetupSnapshot,
    lan_snapshot: MultiplayerSetupSnapshot,
    wol_snapshot: MultiplayerSetupSnapshot,
    // These preferences mirror the process INI8870C0. Native6980C0 does not
    // reopen RA2MD.INI on a child-dialog visit; keep accepted in-memory values
    // rather than replacing them with an unsolicited disk refresh.
    local_preferences: LocalMultiplayerPreferences,
    /// Physical process INI8870C0 input. Accepted writers update this buffer
    /// before attempting disk persistence; repeated6980C0 reads never reopen disk.
    settings_ini: Vec<u8>,
    /// Observer preference AC10C8, read as an integer by 5EE190.
    lan_observer_mode: i32,
    scenario_start_header: crate::map::preview::ScenarioStartHeader,
    scenario_rng: SimRng,
    /// Process-persistent native MapSeed options. The setup dialog edits a
    /// working copy, but closing it does not reconstruct this global record.
    random_map_options: crate::map::rmg::RmgOptions,
    ini_path: Option<PathBuf>,
    cooperative_registry: CooperativeRegistry,
    cooperative_progress: CooperativeProgressState,
    cooperative_country_roster: Vec<CooperativeCountryRosterEntry>,
    /// Process A8EB68, initialized0; independent of COOPMD AIDifficulty.
    cooperative_difficulty: i32,
    gameplay_scenario_return_pending: bool,
}

impl MultiplayerShellRuntime {
    pub(crate) fn read_multiplayer_settings(
        &mut self,
        process_assets: &mut crate::app::process_assets::ProcessAssets,
    ) {
        let ini = IniFile::from_bytes(&self.settings_ini).unwrap_or_else(|_| IniFile::from_str(""));
        let side = process_assets.read_multiplayer_country(&ini, self.local_preferences.side);
        read_local_preference_fields(&ini, &mut self.local_preferences);
        self.local_preferences.side = side;
        self.refresh_multiplayer_country_catalog(process_assets);
    }

    /// Derived HouseType ID/Name view. Refresh after either native registry
    /// allocation or the PrepareSession body pass; choices remain native indices.
    pub(crate) fn refresh_multiplayer_country_catalog(
        &mut self,
        process_assets: &crate::app::process_assets::ProcessAssets,
    ) {
        self.cooperative_country_roster = process_assets
            .multiplayer_countries()
            .map(|(id, name)| CooperativeCountryRosterEntry::new(id, name))
            .collect();
    }

    /// CreateHouses687FBD directly dereferences HouseTypeArray[index]. Reject
    /// an unallocated identity before that undefined native domain; allocated
    /// custom entries remain valid even when stock menu/loading art is absent.
    pub(crate) fn validate_network_countries(
        &self,
        launch: &SkirmishLaunchSession,
    ) -> Result<(), CooperativeError> {
        for country in launch
            .humans
            .iter()
            .filter(|human| !human.is_observer())
            .map(|human| human.slot.country)
            .chain(launch.opponents.iter().map(|ai| ai.country))
        {
            if usize::from(country.raw_index()) >= self.cooperative_country_roster.len() {
                return Err(CooperativeError::InvalidCountryIndex {
                    index: country.raw_index(),
                    count: self.cooperative_country_roster.len(),
                });
            }
        }
        Ok(())
    }

    pub(crate) fn clear_network_cooperative_partners(&mut self) {
        self.cooperative_progress.clear_network_partners();
    }

    fn update_local_preference_bytes(&self, existing: &[u8]) -> Vec<u8> {
        let name = usize::try_from(self.local_preferences.side)
            .ok()
            .and_then(|index| self.cooperative_country_roster.get(index))
            .map_or("<random>", |country| country.id.as_str());
        self.local_preferences.update_ini_bytes(existing, name)
    }

    /// Add_Message's typing-reveal gate for a session mode
    /// (`0x005D3D8A..0x005D3DBD`): LANScrollText in LAN, WOLScrollText in
    /// WOL, on otherwise.
    pub(crate) fn scroll_text(&self, mode: crate::sim::scenario_session::ScenarioGameMode) -> bool {
        use crate::sim::scenario_session::ScenarioGameMode;
        match mode {
            ScenarioGameMode::Lan => self.local_preferences.lan_scroll_text,
            ScenarioGameMode::Wol => self.local_preferences.wol_scroll_text,
            _ => true,
        }
    }

    pub(crate) fn local_participant(&self) -> crate::skirmish_launch::SkirmishHumanSlot {
        crate::skirmish_launch::SkirmishHumanSlot {
            raw_slot_index: Some(0),
            name: crate::util::native_string::acp_decode(&self.local_preferences.handle_bytes),
            slot: crate::skirmish_launch::SkirmishLocalSlot {
                country: LaunchCountry::from_country_index(
                    self.local_preferences.side.max(0) as u32
                ),
                country_random: self.local_preferences.side_ex == RANDOM_ITEM_DATA,
                color_index: u8::try_from(self.local_preferences.color_index).unwrap_or(0),
                color_random: self.local_preferences.color_ex == RANDOM_ITEM_DATA,
                start_position: crate::skirmish_launch::LaunchStartPosition::Auto,
                team: crate::skirmish_launch::LaunchTeam::None,
            },
            role: crate::skirmish_launch::LaunchHumanRole::from_observer(self.lan_observer()),
        }
    }

    fn network_snapshot(&self, profile: SetupProfile) -> &MultiplayerSetupSnapshot {
        match profile {
            SetupProfile::Lan => &self.lan_snapshot,
            SetupProfile::Wol => &self.wol_snapshot,
            SetupProfile::Skirmish => unreachable!("offline Skirmish is not a network profile"),
        }
    }

    /// Restore Session+208's LAN or Session+284's WOL profile (5ED5A0 with
    /// the profile in EDX). Slot01..07 describe visible rows1..7; row0 is the
    /// new local host. 697F10 uses defaults2/2 for both.
    pub(crate) fn new_network_setup(
        &self,
        profile: SetupProfile,
        catalog: &crate::app::scenario_catalog::ScenarioCatalog,
        modes: &[SkirmishGameMode],
        mut options: crate::skirmish_launch::SkirmishLaunchOptions,
        bounds: crate::ui::skirmish_shell::SkirmishTrackbarBounds,
        observe: bool,
    ) -> anyhow::Result<(
        SkirmishLaunchSession,
        [crate::app::match_runtime::session::NetworkVacancy; 8],
    )> {
        use crate::app::match_runtime::session::NetworkVacancy;
        use crate::skirmish_launch::{
            AiDifficulty, LaunchStartPosition, LaunchTeam, PreFillHouseRoster, SkirmishAiSlot,
            SkirmishLaunchMode,
        };
        let snapshot = self.network_snapshot(profile);
        let mode = mode_by_id(modes, snapshot.game_mode)
            .or_else(|| modes.first())
            .ok_or_else(|| anyhow::anyhow!("network setup has no game mode"))?;
        let records = catalog.records();
        let preferred = usize::try_from(snapshot.scenario_index).unwrap_or(0);
        let selected = records
            .get(preferred)
            .filter(|record| crate::map::skirmish_scenarios::record_matches_mode(record, mode))
            .or_else(|| {
                records.iter().find(|record| {
                    crate::map::skirmish_scenarios::record_matches_mode(record, mode)
                })
            })
            .ok_or_else(|| anyhow::anyhow!("network game mode has no eligible map"))?;
        options.game_speed = game_speed_from_visual_position(
            bounds
                .range(SkirmishTrackbarId::GameSpeed0x529)
                .set_up(game_speed_visual_position(snapshot.game_speed)),
        );
        let credits = bounds.range(SkirmishTrackbarId::Credits0x511);
        options.starting_credits = credits.stepped(credits.set_up(snapshot.credits));
        options.unit_count = bounds
            .range(SkirmishTrackbarId::UnitCount0x50c)
            .set_up(snapshot.unit_count);
        options.short_game = snapshot.short_game;
        options.super_weapons = snapshot.super_weapons_allowed;
        options.build_off_ally = snapshot.build_off_ally;
        options.mcv_redeploy = snapshot.mcv_repacks;
        options.crates = snapshot.crates_appear;
        let mut local = self.local_participant();
        local.set_observer(observe);
        local.slot.team = if mode.must_ally {
            LaunchTeam::Team(0)
        } else {
            LaunchTeam::None
        };
        let mut vacancies = [NetworkVacancy::Open; 8];
        let mut opponents = Vec::new();
        for (index, persisted) in snapshot.slots.iter().enumerate() {
            let row = (index + 1) as u8;
            // LAN's pointer categories are4Easy/5Normal/6Hard (5ED400 /
            // 5ED109..135), unlike offline's4+nativeDifficulty encoding.
            let difficulty = match persisted.row_type {
                4 => Some(AiDifficulty::Easy),
                5 => Some(AiDifficulty::Normal),
                6 => Some(AiDifficulty::Hard),
                _ => None,
            };
            if let Some(difficulty) = difficulty.filter(|_| !observe && !self.lan_observer()) {
                opponents.push(SkirmishAiSlot {
                    raw_slot_index: row,
                    country: menu_country_from_item_data(persisted.country)
                        .unwrap_or(LaunchCountry::America),
                    country_random: persisted.country == RANDOM_ITEM_DATA,
                    color_index: if (0..8).contains(&persisted.colour) {
                        persisted.colour as u8
                    } else {
                        0
                    },
                    color_random: !(0..8).contains(&persisted.colour),
                    start_position: LaunchStartPosition::Auto,
                    team: if mode.allies_allowed {
                        LaunchTeam::Team(3)
                    } else {
                        LaunchTeam::None
                    },
                    difficulty,
                });
            } else if persisted.row_type == 3 {
                vacancies[usize::from(row)] = NetworkVacancy::Closed;
            }
            // 5ED5A0 normalizes savedOpenObserver7 toOpen2; AI becomesOpen
            // for observer sessions. Live observer admission is a later event.
        }
        let mut launch = SkirmishLaunchSession {
            mode: SkirmishLaunchMode::from_game_mode(mode),
            selected_map_file: Some(selected.file_name.clone()),
            humans: vec![local],
            local_player_index: 0,
            ai_player_count: opponents.len() as i32,
            opponents,
            pre_fill_house_roster: PreFillHouseRoster::from_compact_skirmish(0),
            options,
        };
        launch.rebuild_house_roster();
        Ok((launch, vacancies))
    }

    /// 5ED400 snapshots current controls into the retained LAN or WOL
    /// profile at Back/Start. It neither resolves random choices nor persists
    /// MultiPlayer.
    pub(crate) fn remember_network_setup(
        &mut self,
        profile: SetupProfile,
        launch: &SkirmishLaunchSession,
        vacancies: [Option<crate::app::match_runtime::session::NetworkVacancy>; 8],
        row_choices: [Option<crate::skirmish_launch::SkirmishLocalSlot>; 8],
        catalog: &crate::app::scenario_catalog::ScenarioCatalog,
    ) {
        use crate::app::match_runtime::session::NetworkVacancy;
        use crate::skirmish_launch::AiDifficulty;
        let snapshot = match profile {
            SetupProfile::Lan => &mut self.lan_snapshot,
            SetupProfile::Wol => &mut self.wol_snapshot,
            SetupProfile::Skirmish => unreachable!("offline Skirmish is not a network profile"),
        };
        snapshot.game_mode = launch.mode.id;
        snapshot.scenario_index = catalog
            .records()
            .iter()
            .position(|record| {
                launch
                    .selected_map_file
                    .as_deref()
                    .is_some_and(|file| file.eq_ignore_ascii_case(&record.file_name))
            })
            .unwrap_or(0) as i32;
        snapshot.game_speed = launch.options.game_speed;
        snapshot.credits = launch.options.starting_credits;
        snapshot.unit_count = launch.options.unit_count;
        snapshot.short_game = launch.options.short_game;
        snapshot.super_weapons_allowed = launch.options.super_weapons;
        snapshot.build_off_ally = launch.options.build_off_ally;
        snapshot.mcv_repacks = launch.options.mcv_redeploy;
        snapshot.crates_appear = launch.options.crates;
        for (index, slot) in snapshot.slots.iter_mut().enumerate() {
            let row = (index + 1) as u8;
            *slot = if let Some(ai) = launch.opponents.iter().find(|ai| ai.raw_slot_index == row) {
                SkirmishPersistedSlot {
                    row_type: match ai.difficulty {
                        AiDifficulty::Easy => 4,
                        AiDifficulty::Normal => 5,
                        AiDifficulty::Hard => 6,
                    },
                    country: if ai.country_random {
                        RANDOM_ITEM_DATA
                    } else {
                        menu_country_item_data(ai.country)
                    },
                    colour: if ai.color_random {
                        RANDOM_ITEM_DATA
                    } else {
                        i32::from(ai.color_index)
                    },
                }
            } else if let Some(human) = launch
                .humans
                .iter()
                .find(|human| human.raw_slot_index == Some(row))
            {
                // A real participant pointer packs kind2, while the combo
                // getters still preserve that row's country/color (5ED400).
                SkirmishPersistedSlot {
                    row_type: 2,
                    country: if human.is_observer() {
                        -3
                    } else if human.slot.country_random {
                        RANDOM_ITEM_DATA
                    } else {
                        menu_country_item_data(human.slot.country)
                    },
                    colour: if human.slot.color_random {
                        RANDOM_ITEM_DATA
                    } else {
                        i32::from(human.slot.color_index)
                    },
                }
            } else {
                SkirmishPersistedSlot {
                    row_type: match vacancies[usize::from(row)] {
                        Some(NetworkVacancy::Closed) => 3,
                        Some(NetworkVacancy::OpenObserver) => 7,
                        _ => 2,
                    },
                    country: if vacancies[usize::from(row)] == Some(NetworkVacancy::OpenObserver) {
                        -3
                    } else {
                        row_choices[usize::from(row)]
                            .as_ref()
                            .map_or(RANDOM_ITEM_DATA, |choices| {
                                if choices.country_random {
                                    RANDOM_ITEM_DATA
                                } else {
                                    menu_country_item_data(choices.country)
                                }
                            })
                    },
                    colour: row_choices[usize::from(row)].as_ref().map_or(
                        RANDOM_ITEM_DATA,
                        |choices| {
                            if choices.color_random {
                                RANDOM_ITEM_DATA
                            } else {
                                i32::from(choices.color_index)
                            }
                        },
                    ),
                }
            };
        }
    }

    /// SessionClass__WriteMultiPlayerSettings 6990A0 writes every profile;
    /// only the network profile the setup edited can differ from disk.
    pub(crate) fn persist_network_setup(&mut self, profile: SetupProfile) {
        self.persist_preferences(|runtime, existing| {
            let preferences = runtime.update_local_preference_bytes(existing);
            runtime
                .network_snapshot(profile)
                .update_ini_bytes(&preferences)
        });
    }

    /// Session handle `[0xA8B380]`: BB's name edit, and the WOL nickname
    /// after a successful login (`0x007864C0`).
    pub(crate) fn set_handle(&mut self, name: &str) {
        self.local_preferences.handle_bytes = crate::util::native_string::acp_encode(name);
        self.local_preferences
            .handle_bytes
            .truncate(MULTIPLAYER_HANDLE_LIMIT_BYTES);
    }

    pub(crate) fn lan_observer(&self) -> bool {
        self.lan_observer_mode != 0
    }

    /// BB Create/double-click compare the raw persisted integer with exactly1.
    pub(crate) fn lan_create_observer(&self) -> bool {
        self.lan_observer_mode == 1
    }

    pub(crate) fn set_lan_observer(&mut self, observer: bool) {
        self.lan_observer_mode = i32::from(observer);
    }

    /// `0x5EE190`: `[MultiPlayer] ObserverMode` from the process RA2MD.INI
    /// object (ReadInt, default 0), as `WOL_Main`'s settings reader ends
    /// (`0x0077DED0`). An unsaved lobby toggle reverts here.
    pub(crate) fn reload_observer_mode(&mut self) {
        let ini = IniFile::from_bytes(&self.settings_ini).unwrap_or_else(|_| IniFile::from_str(""));
        self.lan_observer_mode = ini
            .section_or_empty("MultiPlayer")
            .read_int("ObserverMode", 0);
    }

    /// `0x5EE1B0` alone: write `[MultiPlayer] ObserverMode` and save, as WOL
    /// Options' writer ends (`0x0077DFD0`).
    pub(crate) fn persist_observer_mode(&mut self) {
        self.persist_preferences(|runtime, existing| {
            crate::util::ini_writer::set_ini_values(
                existing,
                "MultiPlayer",
                &[(
                    "ObserverMode",
                    runtime.lan_observer_mode.to_string().as_str(),
                )],
            )
        });
    }

    /// BB Main Menu saves ObserverMode through 5EE1B0; this is separate from
    /// the six-field SessionClass writer and does not change Skirmish settings.
    pub(crate) fn persist_lan_preferences(&mut self) {
        self.persist_preferences(|runtime, existing| {
            let updated = runtime.update_local_preference_bytes(existing);
            crate::util::ini_writer::set_ini_values(
                &updated,
                "MultiPlayer",
                &[(
                    "ObserverMode",
                    runtime.lan_observer_mode.to_string().as_str(),
                )],
            )
        });
    }

    pub(crate) fn persist_local_preferences(&mut self) {
        self.persist_preferences(|runtime, existing| {
            runtime.update_local_preference_bytes(existing)
        });
    }

    /// BD full-options receive5E5AAB refreshes persistent Color+17C only;
    /// ColorEx and both Side fields retain their prior user selection.
    pub(crate) fn accept_network_local_color(&mut self, color_index: u8) {
        self.local_preferences.color_index = i32::from(color_index);
    }

    pub(crate) fn set_preferred_game_mode(&mut self, mode: i32) {
        self.local_preferences.game_mode = mode;
    }

    /// SessionClass6990A0's cache is shared by LAN, WOL and Skirmish.
    pub(crate) fn update_local_preferences(
        &mut self,
        player: &crate::skirmish_launch::SkirmishHumanSlot,
        mode: i32,
    ) {
        let mut handle_bytes = crate::util::native_string::acp_encode(&player.name);
        handle_bytes.truncate(MULTIPLAYER_HANDLE_LIMIT_BYTES);
        self.local_preferences = LocalMultiplayerPreferences {
            // Session+0x1FC0/+0x1FC1 are not these writers' fields.
            wol_scroll_text: self.local_preferences.wol_scroll_text,
            lan_scroll_text: self.local_preferences.lan_scroll_text,
            handle_bytes,
            side: if player.is_observer() {
                -3
            } else if player.slot.country_random {
                0
            } else {
                menu_country_item_data(player.slot.country)
            },
            side_ex: if !player.is_observer() && player.slot.country_random {
                RANDOM_ITEM_DATA
            } else {
                CONCRETE_ITEM_DATA
            },
            color_index: i32::from(player.slot.color_index),
            color_ex: if player.slot.color_random {
                RANDOM_ITEM_DATA
            } else {
                CONCRETE_ITEM_DATA
            },
            game_mode: mode,
        };
    }

    /// Host ProcessRandomAssignments69B8C0 consumes this process Scenario
    /// cursor. The accepted host seed reseeds gameplay later at52E619.
    pub(crate) fn resolve_network_assignments(
        &mut self,
        session: &SkirmishLaunchSession,
    ) -> Result<SkirmishLaunchSession, CooperativeError> {
        let progress = is_cooperative_mode(session).then(|| {
            self.cooperative_progress
                .active()
                .cloned()
                .unwrap_or_default()
        });
        let registry = &self.cooperative_registry;
        let roster = &self.cooperative_country_roster;
        let mut draw_country = |role, rng: &mut SimRng| {
            draw_shell_country(registry, progress.as_ref(), roster, role, rng)
        };
        let mut assignments = ShellRandomAssignmentState::new(session);
        assignments.resolve_humans(&mut self.scenario_rng, &mut draw_country)?;
        assignments.resolve_ai(&mut self.scenario_rng, &mut draw_country)?;
        let resolved = assignments.finish();
        self.validate_network_countries(&resolved)?;
        Ok(resolved)
    }

    /// Establish the front-end Scenario cursor, then construct Cooperative
    /// progress before reading the durable shell snapshot. Stock Cooperative
    /// data advances the freshly seeded cursor by ten logical `(0,2)` calls.
    pub(crate) fn initialize(
        seed: u32,
        ra2_dir: Option<&Path>,
        assets: Option<&AssetManager>,
        startup_rules: Option<&crate::rules::ruleset::RuleSet>,
        defaults: SkirmishGlobalDefaults,
    ) -> Self {
        let mut scenario_rng = SimRng::new(u64::from(seed));
        let cooperative_registry = assets
            .map(CooperativeRegistry::from_assets)
            .transpose()
            .unwrap_or_else(|err| {
                log::warn!("Could not load Cooperative campaign registry: {err}");
                None
            })
            .unwrap_or_default();
        let cooperative_progress =
            CooperativeProgressState::construct(&cooperative_registry, &mut scenario_rng)
                .unwrap_or_else(|err| {
                    log::warn!("Could not construct Cooperative progress state: {err}");
                    CooperativeProgressState::default()
                });
        let cooperative_country_roster = startup_rules
            .map(cooperative_country_roster_from_rules)
            .unwrap_or_default();
        let ini_path = ra2_dir.map(|root| root.join(RA2MD_INI));
        let (
            snapshot,
            [lan_snapshot, wol_snapshot],
            local_preferences,
            lan_observer_mode,
            settings_ini,
        ) = load_persistence(ini_path.as_deref(), defaults);

        Self {
            snapshot,
            lan_snapshot,
            wol_snapshot,
            local_preferences,
            settings_ini,
            lan_observer_mode,
            scenario_rng,
            random_map_options: crate::map::rmg::RmgOptions::default(),
            ini_path,
            cooperative_registry,
            cooperative_progress,
            cooperative_country_roster,
            cooperative_difficulty: 0,
            scenario_start_header: crate::map::preview::ScenarioStartHeader::default(),
            gameplay_scenario_return_pending: false,
        }
    }

    #[cfg(test)]
    fn snapshot(&self) -> &MultiplayerSetupSnapshot {
        &self.snapshot
    }

    /// Apply the one-time process snapshot to raw shell controls. The caller
    /// reconstructs Team defaults and map-capacity visibility afterward.
    pub(crate) fn hydrate_shell(
        &self,
        state: &mut SkirmishShellState,
        maps: &[MapMenuEntry],
        modes: &[SkirmishGameMode],
    ) {
        state.selected_mode_id = mode_by_id(modes, self.local_preferences.game_mode)
            .or_else(|| modes.first())
            .map(|mode| mode.id)
            .unwrap_or(state.selected_mode_id);
        state.selected_map_idx = usize::try_from(self.snapshot.scenario_index)
            .ok()
            .filter(|index| *index < maps.len())
            .unwrap_or(0);

        // `SkirmishDialog__OnSetup497` hands each saved value to its fresh
        // slider through `TBM_SETRANGE` and `TBM_SETPOS`.
        let bounds = state.trackbar_bounds;
        let speed = bounds.range(SkirmishTrackbarId::GameSpeed0x529);
        let credits = bounds.range(SkirmishTrackbarId::Credits0x511);
        let units = bounds.range(SkirmishTrackbarId::UnitCount0x50c);
        state.game_speed = game_speed_from_visual_position(
            speed.set_up(game_speed_visual_position(self.snapshot.game_speed)),
        );
        state.starting_credits = credits.set_up(self.snapshot.credits);
        state.unit_count = units.set_up(self.snapshot.unit_count);
        state.short_game = self.snapshot.short_game;
        state.super_weapons = self.snapshot.super_weapons_allowed;
        state.build_off_ally = self.snapshot.build_off_ally;
        state.mcv_redeploy = self.snapshot.mcv_repacks;
        state.crates = self.snapshot.crates_appear;

        state.player_name_edit = PlayerNameEditState::with_name(
            &crate::util::native_string::acp_decode(&self.local_preferences.handle_bytes),
        );
        state.player_country =
            LaunchCountry::from_country_index(self.local_preferences.side.max(0) as u32);
        state.player_country_random = self.local_preferences.side_ex == RANDOM_ITEM_DATA;
        state.player_color_index = usize::try_from(self.local_preferences.color_index).unwrap_or(0);
        state.player_color_claimed = self.local_preferences.color_ex != RANDOM_ITEM_DATA;

        for (opponent, persisted) in state.opponents.iter_mut().zip(self.snapshot.slots) {
            opponent.row_type = ai_row_type_from_persisted(persisted.row_type);
            opponent.enabled = opponent.row_type.is_active();
            if let Some(difficulty) = opponent.row_type.difficulty() {
                opponent.difficulty = difficulty;
            }

            opponent.country_random = persisted.country == RANDOM_ITEM_DATA;
            if let Some(country) = menu_country_from_item_data(persisted.country) {
                opponent.country = country;
                opponent.country_random = false;
            }

            opponent.color_claimed =
                (0..crate::skirmish_launch::HOUSE_COLOR_COUNT as i32).contains(&persisted.colour);
            if opponent.color_claimed {
                opponent.color_index = persisted.colour as usize;
            }
        }
    }

    /// Pack every durable raw control value without resolving a launch copy.
    /// Production Start/Back uses [`Self::close_shell_transaction`] so the
    /// same fields are committed at their verified native phase boundaries.
    #[cfg(test)]
    pub(crate) fn pack_shell_snapshot(
        &mut self,
        state: &SkirmishShellState,
        maps: &[MapMenuEntry],
        modes: &[SkirmishGameMode],
    ) {
        pack_snapshot_selection(&mut self.snapshot, state, maps, modes);
        pack_snapshot_slots(&mut self.snapshot, state);
        pack_snapshot_options(&mut self.snapshot, state);
    }

    /// Run the common Start/Back state transaction in the verified order:
    /// selection fields, local country, raw Slot triples, local colour, all AI
    /// assignments, then slider/checkbox mirrors. The raw UI state is borrowed
    /// throughout and never replaced by the resolved launch copy.
    pub(crate) fn close_shell_transaction(
        &mut self,
        state: &SkirmishShellState,
        maps: &[MapMenuEntry],
        modes: &[SkirmishGameMode],
        session: &SkirmishLaunchSession,
    ) -> Result<SkirmishLaunchSession, CooperativeError> {
        pack_snapshot_selection(&mut self.snapshot, state, maps, modes);

        // Validation may already have built a borrowed-input session, but it is
        // a pure local read. Material launch state deliberately starts without
        // AI arrays so their first write occurs after the local-country draw.
        let mut staged_session = SkirmishLaunchSession {
            mode: session.mode.clone(),
            selected_map_file: session.selected_map_file.clone(),
            humans: vec![crate::skirmish_launch::SkirmishHumanSlot {
                raw_slot_index: Some(0),
                name: session.local_player().name.clone(),
                slot: session.local_slot().clone(),
                role: crate::skirmish_launch::LaunchHumanRole::from_observer(false),
            }],
            local_player_index: 0,
            ai_player_count: 0,
            opponents: Vec::new(),
            pre_fill_house_roster: session.pre_fill_house_roster.clone(),
            options: session.options.clone(),
        };
        let cooperative = is_cooperative_mode(session);
        if cooperative
            && let Some(map) = session.selected_map_file.as_deref()
            && let Some(chosen_map) = self.ensure_cooperative_selection(map, maps)?
        {
            staged_session.selected_map_file = Some(chosen_map);
        }

        let progress = cooperative.then(|| {
            self.cooperative_progress
                .active()
                .cloned()
                .unwrap_or_else(CooperativeProgressRecord::default)
        });
        let registry = &self.cooperative_registry;
        let roster = &self.cooperative_country_roster;
        let mut assignments = ShellRandomAssignmentState::new(&staged_session);
        let mut draw_country = |role, rng: &mut SimRng| {
            draw_shell_country(registry, progress.as_ref(), roster, role, rng)
        };

        assignments.resolve_local_country(&mut self.scenario_rng, &mut draw_country)?;
        assignments.pack_ai_assignments(&session.opponents);
        pack_snapshot_slots(&mut self.snapshot, state);
        assignments.resolve_local_color(&mut self.scenario_rng);
        assignments.resolve_ai(&mut self.scenario_rng, &mut draw_country)?;
        pack_snapshot_options(&mut self.snapshot, state);
        let resolved = assignments.finish();
        self.validate_network_countries(&resolved)?;

        // Native Start/Back refreshes these cached SessionClass fields before
        // the unconditional writer at 0x006990A0. Keep the resolved concrete
        // values behind the raw -1/-2 markers so reopening the shell can show
        // Random without discarding the value selected for this launch.
        let mut handle_bytes = crate::util::native_string::acp_encode(&state.player_name_edit.text);
        handle_bytes.truncate(MULTIPLAYER_HANDLE_LIMIT_BYTES);
        self.local_preferences = LocalMultiplayerPreferences {
            // Session+0x1FC0/+0x1FC1 are not these writers' fields.
            wol_scroll_text: self.local_preferences.wol_scroll_text,
            lan_scroll_text: self.local_preferences.lan_scroll_text,
            handle_bytes,
            side: menu_country_item_data(resolved.local_slot().country),
            side_ex: if state.player_country_random {
                RANDOM_ITEM_DATA
            } else {
                CONCRETE_ITEM_DATA
            },
            color_index: i32::from(resolved.local_slot().color_index),
            color_ex: if state.player_color_claimed {
                CONCRETE_ITEM_DATA
            } else {
                RANDOM_ITEM_DATA
            },
            game_mode: self.snapshot.game_mode,
        };
        Ok(resolved)
    }

    /// Resolve a borrowed raw session on the process-continuity Scenario RNG.
    /// The shell and durable snapshot remain unchanged.
    #[cfg(test)]
    pub(crate) fn resolve_launch_session(
        &mut self,
        session: &SkirmishLaunchSession,
        maps: &[MapMenuEntry],
    ) -> Result<SkirmishLaunchSession, CooperativeError> {
        if !is_cooperative_mode(session) {
            return Ok(session.resolve_shell_random_assignments(&mut self.scenario_rng));
        }

        let mut cooperative_session = session.clone();
        if let Some(map) = session.selected_map_file.as_deref()
            && let Some(chosen_map) = self.ensure_cooperative_selection(map, maps)?
        {
            cooperative_session.selected_map_file = Some(chosen_map);
        }
        let progress = self
            .cooperative_progress
            .active()
            .cloned()
            .unwrap_or_else(CooperativeProgressRecord::default);
        let registry = &self.cooperative_registry;
        let roster = &self.cooperative_country_roster;
        let rng = &mut self.scenario_rng;
        cooperative_session.try_resolve_shell_random_assignments_with(rng, |role, rng| {
            let cooperative_role = match role {
                RandomCountryRole::Human => CooperativeCountryRole::Player,
                RandomCountryRole::Ai { .. } => CooperativeCountryRole::Enemy,
            };
            draw_country_for_progress(registry, &progress, cooperative_role, roster, rng)
                .map(|index| LaunchCountry::from_country_index(index as u32))
        })
    }

    pub(crate) fn cooperative_setup_view(&self) -> Option<CooperativeSetupView<'_>> {
        let progress = self.cooperative_progress.active()?;
        let campaign_index = usize::try_from(progress.campaign_type).ok()?;
        let stage_index = usize::try_from(progress.current_map).ok()?;
        let campaign = self.cooperative_registry.campaign(campaign_index)?;
        let stage = campaign.stages.get(stage_index)?;
        Some(CooperativeSetupView {
            player_countries: &stage.player_countries,
            campaign_index,
            stage_index,
            difficulty: self.cooperative_difficulty,
        })
    }

    /// Cooperative mode+14 writes partner names before the later Start gates
    /// (5C1D80→49B760), retaining the active campaign progress record.
    pub(crate) fn bind_network_cooperative_partners(
        &mut self,
        launch: &SkirmishLaunchSession,
    ) -> bool {
        let names: Vec<_> = launch
            .humans
            .iter()
            .map(|human| human.name.as_str())
            .collect();
        self.cooperative_progress.bind_network_partners(&names)
    }

    pub(crate) fn cooperative_player_countries(&self) -> Option<Vec<LaunchCountry>> {
        let view = self.cooperative_setup_view()?;
        Some(
            cooperative::country_indices_in_authored_order(
                view.player_countries,
                &self.cooperative_country_roster,
            )
            .into_iter()
            .map(|index| LaunchCountry::from_country_index(index as u32))
            .collect(),
        )
    }

    pub(crate) fn observe_map_preview(&mut self, map: &MapMenuEntry) {
        self.scenario_start_header.apply(map.preview.start_header);
    }
    /// Cooperative mode+68 before shared random assignment; ordinary modes
    /// produce no effect. Retained preview Header and progress stay here.
    pub(crate) fn prepare_network_mode_start(
        &self,
        session: &SkirmishLaunchSession,
        map: &MapMenuEntry,
    ) -> Result<cooperative::CooperativeAiPreparation, CooperativeError> {
        if !session.mode.class.uses_cooperative_setup() {
            return Ok(Default::default());
        }
        let campaign = self
            .cooperative_progress
            .active()
            .and_then(|record| usize::try_from(record.campaign_type).ok())
            .unwrap_or(0);
        cooperative::prepare_ai(
            &self.cooperative_registry,
            self.cooperative_progress.active(),
            campaign,
            self.scenario_start_header,
            map.player_capacity,
            self.cooperative_difficulty,
            &session.opponents,
        )
    }
    /// Cooperative mode+64 after current-round assignment retains all later
    /// round country/color pairs, using the same process Scenario stream.
    pub(crate) fn complete_network_mode_assignments(
        &mut self,
        session: &SkirmishLaunchSession,
    ) -> Result<(), CooperativeError> {
        if !session.mode.class.uses_cooperative_setup() {
            return Ok(());
        }
        let campaign = self
            .cooperative_progress
            .active()
            .and_then(|record| usize::try_from(record.campaign_type).ok())
            .unwrap_or(0);
        let colors: Vec<_> = session
            .humans
            .iter()
            .map(|human| i32::from(human.slot.color_index))
            .collect();
        self.cooperative_progress.prepare_round_assignments(
            &self.cooperative_registry,
            campaign,
            &self.cooperative_country_roster,
            &colors,
            &mut self.scenario_rng,
        )
    }
    pub(crate) fn cooperative_round_assignments(&self) -> &[[[i32; 2]; 8]] {
        self.cooperative_progress.round_assignments().rounds()
    }
    pub(crate) fn accept_cooperative_round_assignments(&mut self, rounds: Vec<[[i32; 2]; 8]>) {
        self.cooperative_progress.accept_round_assignments(rounds);
    }

    pub(crate) fn cooperative_difficulty(&self) -> i32 {
        self.cooperative_difficulty
    }

    /// BC5DEE63/C278FA0A and received options5E4F44 retain this process integer.
    /// Cooperative5C31A5 later maps0→Easy,1→Normal,other→Hard.
    pub(crate) fn set_cooperative_difficulty(&mut self, difficulty: i32) {
        self.cooperative_difficulty = difficulty;
    }

    /// Bind/open the active Cooperative progress record for a highlighted map.
    /// Reopening an already active campaign consumes no map-variant draw.
    pub(crate) fn ensure_cooperative_selection(
        &mut self,
        scenario: &str,
        available_maps: &[MapMenuEntry],
    ) -> Result<Option<String>, CooperativeError> {
        let Some(campaign_index) = self.cooperative_registry.campaign_for_map(scenario) else {
            return Ok(None);
        };
        let rng_checkpoint = self.scenario_rng.clone();
        let progress_checkpoint = self.cooperative_progress.clone();
        let result = (|| {
            self.cooperative_progress.ensure_active(
                &self.cooperative_registry,
                campaign_index,
                &mut self.scenario_rng,
            )?;
            let chosen = self
                .cooperative_progress
                .active()
                .and_then(CooperativeProgressRecord::current_chosen_map)
                .map(str::to_string);
            validate_cooperative_map_binding(chosen, available_maps)
        })();
        if result.is_err() {
            self.scenario_rng = rng_checkpoint;
            self.cooperative_progress = progress_checkpoint;
        }
        result
    }

    /// Apply an accepted Cooperative campaign selection. Moving to another
    /// campaign swaps in its preconstructed record and initializes exactly one
    /// replacement reserve record on the same front-end RNG.
    pub(crate) fn accept_cooperative_selection(
        &mut self,
        scenario: &str,
        available_maps: &[MapMenuEntry],
    ) -> Result<Option<String>, CooperativeError> {
        let Some(campaign_index) = self.cooperative_registry.campaign_for_map(scenario) else {
            return Ok(None);
        };
        let rng_checkpoint = self.scenario_rng.clone();
        let progress_checkpoint = self.cooperative_progress.clone();
        let result = (|| {
            if self.cooperative_progress.active().is_none() {
                self.cooperative_progress.ensure_active(
                    &self.cooperative_registry,
                    campaign_index,
                    &mut self.scenario_rng,
                )?;
            } else if self
                .cooperative_progress
                .active()
                .is_some_and(|active| active.campaign_type != campaign_index as i32)
            {
                self.cooperative_progress.accept_campaign_swap(
                    &self.cooperative_registry,
                    campaign_index,
                    &mut self.scenario_rng,
                )?;
            }
            let chosen = self
                .cooperative_progress
                .active()
                .and_then(CooperativeProgressRecord::current_chosen_map)
                .map(str::to_string);
            validate_cooperative_map_binding(chosen, available_maps)
        })();
        if result.is_err() {
            self.scenario_rng = rng_checkpoint;
            self.cooperative_progress = progress_checkpoint;
        }
        result
    }

    /// Apply all snapshot keys to one in-memory buffer and perform one write.
    /// Failure is intentionally nonfatal, matching the native caller.
    pub(crate) fn persist_snapshot(&mut self) {
        self.persist_preferences(|runtime, existing| runtime.update_persistence_bytes(existing));
    }

    fn persist_preferences(&mut self, update: impl Fn(&Self, &[u8]) -> Vec<u8>) {
        // WriteINI mutates process8870C0 before the fallible filesystem write.
        let process_updated = update(self, &self.settings_ini);
        self.settings_ini = process_updated;
        let Some(path) = self.ini_path.as_deref() else {
            return;
        };
        let existing = match std::fs::read(path) {
            Ok(bytes) => bytes,
            Err(err) if err.kind() == io::ErrorKind::NotFound => Vec::new(),
            Err(err) => {
                log::warn!(
                    "Could not read {} before multiplayer persistence: {err}",
                    path.display()
                );
                return;
            }
        };
        // Other existing profile writers own their sections. Merge those disk
        // bytes while retaining this owner's process inputs independently.
        let updated = update(self, &existing);
        if let Err(err) = std::fs::write(path, updated) {
            log::warn!(
                "Could not persist multiplayer settings to {}: {err}",
                path.display()
            );
        }
    }

    /// Compose the active offline writer's two relevant sections in native
    /// order, then let the owner perform one final filesystem write.
    fn update_persistence_bytes(&self, existing: &[u8]) -> Vec<u8> {
        let updated = self.update_local_preference_bytes(existing);
        self.snapshot.update_ini_bytes(&updated)
    }

    pub(crate) fn mark_gameplay_scenario_return_pending(&mut self) {
        self.gameplay_scenario_return_pending = true;
    }

    /// Clear_Scene685237 resets the retained Scenario header before fresh
    /// map parsing. Even a failed load has crossed that reset boundary.
    pub(crate) fn begin_fresh_scenario_load(&mut self) {
        self.scenario_start_header = crate::map::preview::ScenarioStartHeader::default();
    }

    pub(crate) fn observe_loaded_scenario_header(
        &mut self,
        ini: &crate::rules::ini_parser::IniFile,
    ) {
        self.scenario_start_header.read_ini(ini);
    }

    /// Return the live Scenario cursor and saved header as one shell handoff.
    pub(crate) fn capture_returned_gameplay_scenario(
        &mut self,
        gameplay_rng: SimRng,
        start_header: crate::map::preview::ScenarioStartHeader,
    ) -> bool {
        if !self.gameplay_scenario_return_pending {
            return false;
        }
        self.scenario_rng = gameplay_rng;
        self.scenario_start_header = start_header;
        self.gameplay_scenario_return_pending = false;
        true
    }

    /// Return the process MapSeed options for a setup instance. Only the first
    /// entry with the constructor sentinel spends a Scenario word; later
    /// entries reuse the stored seed and all other edited fields.
    ///
    /// gamemd provenance: setup `WM_INITDIALOG` at
    /// 0x00596BB1..0x00596BC8 calls Scenario `RandomRanged(0,0xFFFF)` only
    /// when MapSeed+0x74 is -1. The global MapSeed survives dialog teardown.
    pub(crate) fn random_map_options_for_setup(&mut self) -> crate::map::rmg::RmgOptions {
        if self.random_map_options.seed == -1 {
            self.random_map_options.seed =
                self.scenario_rng.next_range_u32_inclusive(0, 0xFFFF) as i32;
        }
        self.random_map_options.normalize();
        self.random_map_options.clone()
    }

    /// Persist the dialog's working record back into the process MapSeed owner.
    pub(crate) fn remember_random_map_options(&mut self, options: &crate::map::rmg::RmgOptions) {
        self.random_map_options = options.clone();
        self.random_map_options.normalize();
    }

    /// Replay the generated Building constructors on the shell Scenario owner.
    /// Geometry stays MapGen-only; every trace row, including discarded
    /// objects, represents exactly one raw Techno constructor word.
    ///
    /// gamemd provenance: TechnoClass constructor 0x006F3254 calls raw
    /// Scenario `Random__Next`; CABHUT reaches it through
    /// 0x005904B0 -> 0x0043B740 and the other RMG Building owners share it.
    pub(crate) fn replay_random_map_preview_construction(
        &mut self,
        trace: &crate::map::rmg::RmgConstructionTrace,
    ) {
        for (expected_ordinal, event) in trace.events.iter().enumerate() {
            debug_assert_eq!(event.ordinal, expected_ordinal);
            let _ = self.scenario_rng.next_u32();
        }
    }

    #[cfg(test)]
    fn scenario_rng_state(&self) -> u64 {
        self.scenario_rng.state()
    }

    #[cfg(test)]
    pub(crate) fn scenario_rng_logical_state_for_test(
        &self,
    ) -> crate::sim::rng::SimRngLogicalState {
        self.scenario_rng.logical_state()
    }
}

fn pack_snapshot_selection(
    snapshot: &mut MultiplayerSetupSnapshot,
    state: &SkirmishShellState,
    maps: &[MapMenuEntry],
    modes: &[SkirmishGameMode],
) {
    snapshot.game_mode = mode_by_id(modes, state.selected_mode_id)
        .or_else(|| modes.first())
        .map(|mode| mode.id)
        .unwrap_or(0);
    snapshot.scenario_index = if state.selected_map_idx < maps.len() {
        i32::try_from(state.selected_map_idx).unwrap_or(0)
    } else {
        0
    };
}

fn pack_snapshot_slots(snapshot: &mut MultiplayerSetupSnapshot, state: &SkirmishShellState) {
    for (index, destination) in snapshot.slots.iter_mut().enumerate() {
        let Some(opponent) = state.opponents.get(index) else {
            *destination = SkirmishPersistedSlot {
                row_type: 1,
                country: RANDOM_ITEM_DATA,
                colour: RANDOM_ITEM_DATA,
            };
            continue;
        };
        *destination = SkirmishPersistedSlot {
            row_type: persisted_row_type(opponent.row_type),
            country: if opponent.country_random {
                RANDOM_ITEM_DATA
            } else {
                menu_country_item_data(opponent.country)
            },
            colour: if opponent.color_claimed {
                i32::try_from(opponent.color_index).unwrap_or(RANDOM_ITEM_DATA)
            } else {
                RANDOM_ITEM_DATA
            },
        };
    }
}

fn pack_snapshot_options(snapshot: &mut MultiplayerSetupSnapshot, state: &SkirmishShellState) {
    snapshot.game_speed = state.game_speed;
    snapshot.credits = state.credits();
    snapshot.unit_count = state.unit_count;
    snapshot.short_game = state.short_game;
    snapshot.super_weapons_allowed = state.super_weapons;
    snapshot.build_off_ally = state.build_off_ally;
    snapshot.mcv_repacks = state.mcv_redeploy;
    snapshot.crates_appear = state.crates;
}

fn validate_cooperative_map_binding(
    chosen: Option<String>,
    available_maps: &[MapMenuEntry],
) -> Result<Option<String>, CooperativeError> {
    if let Some(chosen_map) = chosen.as_deref()
        && !available_maps
            .iter()
            .any(|map| map.file_name.eq_ignore_ascii_case(chosen_map))
    {
        return Err(CooperativeError::MissingScenarioMap {
            scenario: chosen_map.to_string(),
        });
    }
    Ok(chosen)
}

pub(crate) fn skirmish_global_defaults(state: &SkirmishShellState) -> SkirmishGlobalDefaults {
    SkirmishGlobalDefaults {
        game_mode: state.selected_mode_id,
        scenario_index: i32::try_from(state.selected_map_idx).unwrap_or(0),
        game_speed: state.game_speed,
        credits: state.credits(),
        unit_count: state.unit_count,
        short_game: state.short_game,
        super_weapons_allowed: state.super_weapons,
        build_off_ally: state.build_off_ally,
        mcv_repacks: state.mcv_redeploy,
        crates_appear: state.crates,
    }
}

fn draw_shell_country(
    registry: &CooperativeRegistry,
    progress: Option<&CooperativeProgressRecord>,
    roster: &[CooperativeCountryRosterEntry],
    role: RandomCountryRole,
    rng: &mut SimRng,
) -> Result<LaunchCountry, CooperativeError> {
    if let Some(progress) = progress {
        let cooperative_role = match role {
            RandomCountryRole::Human => CooperativeCountryRole::Player,
            RandomCountryRole::Ai { .. } => CooperativeCountryRole::Enemy,
        };
        draw_country_for_progress(registry, progress, cooperative_role, roster, rng)
            .map(|index| LaunchCountry::from_country_index(index as u32))
    } else {
        Ok(LaunchCountry::from_country_index(
            rng.next_range_u32_inclusive(0, 9),
        ))
    }
}

fn load_persistence(
    path: Option<&Path>,
    defaults: SkirmishGlobalDefaults,
) -> (
    MultiplayerSetupSnapshot,
    [MultiplayerSetupSnapshot; 2],
    LocalMultiplayerPreferences,
    i32,
    Vec<u8>,
) {
    let fallback = || {
        (
            MultiplayerSetupSnapshot::from_global_defaults(defaults, SetupProfile::Skirmish),
            [SetupProfile::Lan, SetupProfile::Wol]
                .map(|profile| MultiplayerSetupSnapshot::from_global_defaults(defaults, profile)),
            LocalMultiplayerPreferences::default(),
            0,
            Vec::new(),
        )
    };
    let Some(path) = path else {
        return fallback();
    };
    match std::fs::read(path) {
        Ok(bytes) => {
            let snapshot = read_setup_snapshot(&bytes, defaults, SetupProfile::Skirmish)
                .unwrap_or_else(|err| {
                    log::warn!(
                        "Could not parse Skirmish settings from {}: {err}",
                        path.display()
                    );
                    MultiplayerSetupSnapshot::from_global_defaults(defaults, SetupProfile::Skirmish)
                });
            // 5EE190: process INI / MultiPlayer / ObserverMode, ReadInt default0.
            let observer = IniFile::from_bytes(&bytes).ok().map_or(0, |ini| {
                ini.section_or_empty("MultiPlayer")
                    .read_int("ObserverMode", 0)
            });
            let network = [SetupProfile::Lan, SetupProfile::Wol].map(|profile| {
                read_setup_snapshot(&bytes, defaults, profile).unwrap_or_else(|_| {
                    MultiplayerSetupSnapshot::from_global_defaults(defaults, profile)
                })
            });
            (
                snapshot,
                network,
                read_initial_local_preference_fields(&bytes),
                observer,
                bytes,
            )
        }
        Err(err) if err.kind() == io::ErrorKind::NotFound => fallback(),
        Err(err) => {
            log::warn!(
                "Could not read Skirmish settings from {}: {err}",
                path.display()
            );
            fallback()
        }
    }
}

fn read_initial_local_preference_fields(bytes: &[u8]) -> LocalMultiplayerPreferences {
    let mut preferences = LocalMultiplayerPreferences::default();
    let Ok(ini) = IniFile::from_bytes(bytes) else {
        return preferences;
    };
    read_local_preference_fields(&ini, &mut preferences);
    preferences
}

fn read_local_preference_fields(ini: &IniFile, preferences: &mut LocalMultiplayerPreferences) {
    let Some(section) = ini.section("MultiPlayer") else {
        return;
    };

    // `SessionClass` reader at 0x006980C0: `ReadCommaHexUTF16(0x14)` over the
    // `TXT_NONAME` default. The visible units become handle bytes by their low
    // byte, and a handle that ends up empty keeps the current one.
    let default_handle: Vec<u16> = preferences
        .handle_bytes
        .iter()
        .map(|byte| u16::from(*byte))
        .collect();
    let handle_bytes: Vec<u8> = section
        .read_comma_hex_utf16("Handle", &default_handle, MULTIPLAYER_HANDLE_UNITS)
        .into_iter()
        .map(|unit| unit as u8)
        .take_while(|byte| *byte != 0)
        .take(MULTIPLAYER_HANDLE_LIMIT_BYTES)
        .collect();
    if !handle_bytes.is_empty() {
        preferences.handle_bytes = handle_bytes;
    }
    //6982E2 supplies the retained raw field as ReadInt default;6982FD stores
    //it without a display-range clamp. Projection never rewrites this cache.
    preferences.color_index = section.read_int("Color", preferences.color_index);
    preferences.side_ex = section.read_int("SideEx", preferences.side_ex);
    preferences.color_ex = section.read_int("ColorEx", preferences.color_ex);
    preferences.game_mode = section.read_int("GameMode", DEFAULT_MULTIPLAYER_GAME_MODE);
    // 0x006983A2..0x006983E4: ReadBool 0x005295F0 over the held values,
    // WOL first. WriteMultiPlayerSettings does not write them back.
    preferences.wol_scroll_text = section.read_bool("WOLScrollText", preferences.wol_scroll_text);
    preferences.lan_scroll_text = section.read_bool("LANScrollText", preferences.lan_scroll_text);
}

fn encode_multiplayer_handle(bytes: &[u8]) -> String {
    let mut encoded = String::with_capacity(bytes.len().saturating_mul(3));
    for byte in bytes {
        encoded.push_str(&format!("{byte:x},"));
    }
    encoded
}

fn is_cooperative_mode(session: &SkirmishLaunchSession) -> bool {
    session.mode.class == crate::skirmish_modes::MpModeClass::Cooperative
}

fn ai_row_type_from_persisted(value: i32) -> SkirmishAiRowType {
    match value {
        4 => SkirmishAiRowType::Hard,
        5 => SkirmishAiRowType::Normal,
        6 => SkirmishAiRowType::Easy,
        _ => SkirmishAiRowType::None,
    }
}

fn persisted_row_type(value: SkirmishAiRowType) -> i32 {
    match value {
        SkirmishAiRowType::None => 1,
        SkirmishAiRowType::Hard => 4,
        SkirmishAiRowType::Normal => 5,
        SkirmishAiRowType::Easy => 6,
    }
}

fn menu_country_from_item_data(value: i32) -> Option<LaunchCountry> {
    u16::try_from(value)
        .ok()
        .map(|index| LaunchCountry::from_country_index(u32::from(index)))
}

fn menu_country_item_data(country: LaunchCountry) -> i32 {
    i32::from(country.raw_index())
}

fn cooperative_country_roster_from_rules(
    rules: &crate::rules::ruleset::RuleSet,
) -> Vec<CooperativeCountryRosterEntry> {
    rules
        .cooperative_country_candidates()
        .map(|(id, name)| CooperativeCountryRosterEntry::new(id, name))
        .collect()
}

/// Controlled ten-country roster used by existing native corpus cases;
/// production consumes the complete processed Rules registry above.
#[cfg(test)]
fn controlled_ten_country_roster() -> Vec<CooperativeCountryRosterEntry> {
    [
        "Americans",
        "Alliance",
        "French",
        "Germans",
        "British",
        "Africans",
        "Arabs",
        "Confederation",
        "Russians",
        "YuriCountry",
    ]
    .into_iter()
    .map(|id| CooperativeCountryRosterEntry::new(id, None))
    .collect()
}

const _: [(); SKIRMISH_PERSISTED_SLOT_COUNT] = [(); crate::skirmish_launch::SKIRMISH_AI_SLOT_COUNT];

#[cfg(test)]
mod tests {
    use super::*;
    use crate::map::preview::PreviewSection;
    use crate::skirmish_launch::{
        AiDifficulty, LaunchStartPosition, LaunchTeam, SkirmishAiSlot, SkirmishLaunchMode,
        SkirmishLaunchOptions, SkirmishLocalSlot,
    };

    fn map_named(file_name: &str) -> MapMenuEntry {
        MapMenuEntry {
            file_name: file_name.to_string(),
            display_name: "Test".to_string(),
            author: None,
            preview: PreviewSection::default(),
            multiplayer_start_waypoints: Vec::new(),
            player_capacity: 8,
            preview_source_bounds: None,
        }
    }

    fn map() -> MapMenuEntry {
        map_named("test.mmx")
    }

    fn cooperative_maps() -> Vec<MapMenuEntry> {
        ["A1", "A1B", "A1C", "A2", "A2B", "A2C", "W1", "W1B", "W1C"]
            .into_iter()
            .map(map_named)
            .collect()
    }

    fn mode() -> SkirmishGameMode {
        SkirmishGameMode {
            class: crate::skirmish_modes::MpModeClass::Battle,
            id: 1,
            ui_name_key: "GUI:Battle".to_string(),
            tooltip_key: "STT:ModeBattle".to_string(),
            override_file: "MPBattleMD.ini".to_string(),
            map_filter: "standard".to_string(),
            random_maps_allowed: true,
            allies_allowed: true,
            must_ally: false,
        }
    }

    fn cooperative_mode() -> SkirmishGameMode {
        SkirmishGameMode {
            class: crate::skirmish_modes::MpModeClass::Cooperative,
            override_file: "MPCoopMD.ini".to_string(),
            ..mode()
        }
    }

    fn cooperative_registry() -> CooperativeRegistry {
        CooperativeRegistry::from_ini(&IniFile::from_str(
            "[Campaigns]\n1=Allied\n2=World\n\
             [Allied]\nNumberOfCampaignMaps=2\n\
             CampaignPlayer1=Americans\nCampaignEnemy1=Russians\n\
             CampaignPlayer2=Americans\nCampaignEnemy2=Russians\n\
             Map1=A1,A1B,A1C\nMap2=A2,A2B,A2C\n\
             [World]\nNumberOfCampaignMaps=1\n\
             CampaignPlayer1=Americans\nCampaignEnemy1=Russians\n\
             Map1=W1,W1B,W1C\n",
        ))
        .expect("Cooperative fixture")
    }

    fn read_local_multiplayer_preferences(bytes: &[u8]) -> LocalMultiplayerPreferences {
        let countries = controlled_ten_country_roster()
            .iter()
            .enumerate()
            .map(|(index, country)| format!("{index}={}\n", country.id))
            .collect::<String>();
        let mut owner =
            crate::rules::process_owner::NativeRulesProcessOwner::from_cold_start_sources(
                IniFile::from_str(&format!("[Countries]\n{countries}")),
                None,
                IniFile::empty(),
                std::sync::Arc::new(crate::rules::sound_ini::SoundRegistry::from_ini(
                    &IniFile::empty(),
                )),
                Default::default(),
            )
            .unwrap();
        let ini = IniFile::from_bytes(bytes).unwrap();
        let mut preferences = read_initial_local_preference_fields(bytes);
        preferences.side = owner.read_multiplayer_country(&ini, preferences.side);
        preferences
    }

    fn cooperative_runtime(seed: u32) -> MultiplayerShellRuntime {
        let cooperative_registry = cooperative_registry();
        let mut scenario_rng = SimRng::new(u64::from(seed));
        let cooperative_progress =
            CooperativeProgressState::construct(&cooperative_registry, &mut scenario_rng)
                .expect("preconstructed Cooperative records");
        MultiplayerShellRuntime {
            snapshot: MultiplayerSetupSnapshot::from_global_defaults(
                defaults(),
                SetupProfile::Skirmish,
            ),
            lan_snapshot: MultiplayerSetupSnapshot::from_global_defaults(
                defaults(),
                SetupProfile::Lan,
            ),
            wol_snapshot: MultiplayerSetupSnapshot::from_global_defaults(
                defaults(),
                SetupProfile::Wol,
            ),
            local_preferences: LocalMultiplayerPreferences::default(),
            settings_ini: Vec::new(),
            lan_observer_mode: 0,
            scenario_rng,
            random_map_options: crate::map::rmg::RmgOptions::default(),
            ini_path: None,
            cooperative_registry,
            cooperative_progress,
            cooperative_country_roster: controlled_ten_country_roster(),
            cooperative_difficulty: 0,
            scenario_start_header: crate::map::preview::ScenarioStartHeader::default(),
            gameplay_scenario_return_pending: false,
        }
    }

    fn runtime(snapshot: MultiplayerSetupSnapshot, seed: u32) -> MultiplayerShellRuntime {
        MultiplayerShellRuntime {
            snapshot,
            lan_snapshot: MultiplayerSetupSnapshot::from_global_defaults(
                defaults(),
                SetupProfile::Lan,
            ),
            wol_snapshot: MultiplayerSetupSnapshot::from_global_defaults(
                defaults(),
                SetupProfile::Wol,
            ),
            local_preferences: LocalMultiplayerPreferences::default(),
            settings_ini: Vec::new(),
            lan_observer_mode: 0,
            scenario_rng: SimRng::new(u64::from(seed)),
            random_map_options: crate::map::rmg::RmgOptions::default(),
            ini_path: None,
            cooperative_registry: CooperativeRegistry::default(),
            cooperative_progress: CooperativeProgressState::default(),
            cooperative_country_roster: controlled_ten_country_roster(),
            cooperative_difficulty: 0,
            scenario_start_header: crate::map::preview::ScenarioStartHeader::default(),
            gameplay_scenario_return_pending: false,
        }
    }

    fn defaults() -> SkirmishGlobalDefaults {
        skirmish_global_defaults(&SkirmishShellState::default())
    }

    #[test]
    fn lan_setup_uses_separate_profile_native_ai_codes_and_resets_start_team() {
        use crate::app::match_runtime::session::NetworkVacancy;
        use crate::map::skirmish_scenarios::{SkirmishScenarioRecord, SkirmishScenarioSource};
        let mut owner = runtime(
            MultiplayerSetupSnapshot::from_global_defaults(defaults(), SetupProfile::Skirmish),
            1,
        );
        owner.lan_snapshot = read_setup_snapshot(
            b"[LAN]\nSlot01=4,-2,1\nSlot02=3,8,7\nSlot03=7,-2,-2\n",
            defaults(),
            SetupProfile::Lan,
        )
        .unwrap();
        let catalog = crate::app::scenario_catalog::ScenarioCatalog::from_records(vec![
            SkirmishScenarioRecord::concrete_from_ini(
                0,
                SkirmishScenarioSource::Synthetic,
                "test.mmx",
                &IniFile::from_str("[Basic]\nGameMode=standard\n"),
            ),
        ]);
        let modes = [mode()];
        let (launch, vacancies) = owner
            .new_network_setup(
                SetupProfile::Lan,
                &catalog,
                &modes,
                crate::skirmish_launch::SkirmishLaunchOptions::default(),
                crate::ui::skirmish_shell::SkirmishTrackbarBounds::default(),
                false,
            )
            .unwrap();
        assert_eq!(launch.opponents.len(), 1);
        assert_eq!(launch.opponents[0].raw_slot_index, 1);
        assert_eq!(
            launch.opponents[0].difficulty,
            crate::skirmish_launch::AiDifficulty::Easy
        );
        assert!(launch.opponents[0].country_random);
        assert_eq!(
            launch.opponents[0].start_position,
            crate::skirmish_launch::LaunchStartPosition::Auto
        );
        assert_eq!(vacancies[2], NetworkVacancy::Closed);
        assert_eq!(vacancies[3], NetworkVacancy::Open);
        let offline_before = owner.snapshot.clone();
        let wol_before = owner.wol_snapshot.clone();
        owner.remember_network_setup(
            SetupProfile::Lan,
            &launch,
            std::array::from_fn(|row| Some(vacancies[row])),
            std::array::from_fn(|_| None),
            &catalog,
        );
        assert_eq!(owner.lan_snapshot.slots[0].row_type, 4);
        assert_eq!(owner.snapshot, offline_before);
        // C2/C9 keep their own Session+284 WonlinePref profile.
        assert_eq!(owner.wol_snapshot, wol_before);
        let (wol, wol_vacancies) = owner
            .new_network_setup(
                SetupProfile::Wol,
                &catalog,
                &modes,
                crate::skirmish_launch::SkirmishLaunchOptions::default(),
                crate::ui::skirmish_shell::SkirmishTrackbarBounds::default(),
                false,
            )
            .unwrap();
        assert!(wol.opponents.is_empty());
        assert_eq!(wol_vacancies, [NetworkVacancy::Open; 8]);
        owner.set_lan_observer(true);
        let (observer, _) = owner
            .new_network_setup(
                SetupProfile::Lan,
                &catalog,
                &modes,
                crate::skirmish_launch::SkirmishLaunchOptions::default(),
                crate::ui::skirmish_shell::SkirmishTrackbarBounds::default(),
                true,
            )
            .unwrap();
        assert!(observer.local_player().is_observer());
        assert!(observer.opponents.is_empty());
    }

    #[test]
    fn profile_and_seed_writers_work_with_process_asset_manager_alive() {
        use crate::app::persistence::options_profile::RetailOptionsProfile;
        use crate::map::rmg::{RmgOptions, saved_seeds};

        let directory = std::env::temp_dir().join(format!(
            "vera20k-live-profile-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        std::fs::create_dir_all(&directory).expect("fixture directory");
        let path = directory.join(RA2MD_INI);
        let initial =
            b"; keep this comment\r\n[Unrelated]\r\nKey=kept\r\n[Skirmish]\r\nCredits=10000\r\n";
        std::fs::write(&path, initial).expect("existing nonempty profile");
        let seed_name = crate::util::native_file_name::NativeFileName::from("SAVE0001.SED");
        let mut seed = RmgOptions::default();
        saved_seeds::write_browser_seed(&directory, &seed_name, &seed).expect("existing seed");
        let assets = AssetManager::from_loose_root_for_test(&directory);
        let borrowed_profile = assets.get_ref(RA2MD_INI).expect("profile snapshot");
        let borrowed_seed = assets.get_ref("SAVE0001.SED").expect("seed snapshot");
        let mut runtime = MultiplayerShellRuntime::initialize(
            42,
            Some(&directory),
            Some(&assets),
            None,
            defaults(),
        );
        let mut profile = RetailOptionsProfile::default();

        for (credits, scroll_rate) in [(22000, 3), (33000, 5)] {
            runtime.snapshot.credits = credits;
            runtime.persist_snapshot();
            profile.scroll_rate = scroll_rate;
            profile
                .commit_ra2md(&directory)
                .expect("Options write with manager alive");
            runtime.persist_snapshot();
            let bytes = std::fs::read(&path).expect("persisted profile");
            let ini = IniFile::from_bytes(&bytes).expect("profile INI");
            assert_eq!(
                ini.section("Skirmish").unwrap().get_for_test("Credits"),
                Some(credits.to_string().as_str())
            );
            assert_eq!(
                ini.section("Options").unwrap().get_for_test("ScrollRate"),
                Some(scroll_rate.to_string().as_str())
            );
            assert_eq!(
                ini.section("Unrelated").unwrap().get_for_test("Key"),
                Some("kept")
            );
            assert!(bytes.starts_with(b"; keep this comment"));
        }

        seed.seed = 42424;
        seed.description = "Overwritten".into();
        saved_seeds::write_browser_seed(&directory, &seed_name, &seed)
            .expect("overwrite existing seed");
        let reread = saved_seeds::read_browser_seed(
            &directory,
            &seed_name,
            &RmgOptions::default(),
            "Random Map",
        )
        .expect("read current saved seed");
        assert_eq!(reread.seed, seed.seed);
        assert_eq!(reread.description, seed.description);
        assert_eq!(borrowed_profile, initial);
        assert_ne!(borrowed_seed, seed.to_sed_bytes());
        assert_eq!(assets.get_ref(RA2MD_INI), Some(borrowed_profile));
        drop(assets);
        std::fs::remove_dir_all(directory).expect("remove owned fixture");
    }

    fn random_session() -> SkirmishLaunchSession {
        SkirmishLaunchSession {
            mode: SkirmishLaunchMode::from_game_mode(&mode()),
            selected_map_file: Some("test.mmx".to_string()),
            humans: vec![crate::skirmish_launch::SkirmishHumanSlot {
                raw_slot_index: Some(0),
                name: "Player".to_string(),
                slot: SkirmishLocalSlot {
                    country: LaunchCountry::America,
                    country_random: true,
                    color_index: 0,
                    color_random: true,
                    start_position: LaunchStartPosition::Auto,
                    team: LaunchTeam::None,
                },
                role: crate::skirmish_launch::LaunchHumanRole::from_observer(false),
            }],
            local_player_index: 0,
            ai_player_count: 1,
            opponents: vec![SkirmishAiSlot {
                raw_slot_index: 1,
                country: LaunchCountry::Russia,
                country_random: true,
                color_index: 1,
                color_random: true,
                start_position: LaunchStartPosition::Auto,
                team: LaunchTeam::None,
                difficulty: AiDifficulty::Easy,
            }],
            pre_fill_house_roster:
                crate::skirmish_launch::PreFillHouseRoster::from_compact_skirmish(1),
            options: SkirmishLaunchOptions::default(),
        }
    }

    #[test]
    fn saved_slider_values_reach_the_sliders_through_the_native_setup() {
        // `SkirmishDialog__OnSetup497` sets each fresh slider's range, then
        // its saved value; `TBM_SETPOS` ignores a value outside the range
        // (executed in `tools/storage_oracle/launcher_trackbar.py`).
        let Some(rules) = crate::rules::retail_ini_fixture::retail_ini("rulesmd.ini") else {
            return;
        };
        let bounds =
            crate::ui::skirmish_shell::SkirmishTrackbarBounds::from_multiplayer_dialog_settings(
                &rules,
            );
        for ((credits, units, speed), expected) in [
            ((7549, 4, 2), (7549, 7500, 4, 2)),
            ((20000, 15, 9), (10000, 10000, 0, 6)),
            ((4999, -1, -1), (10000, 10000, 0, 6)),
        ] {
            let mut snapshot =
                MultiplayerSetupSnapshot::from_global_defaults(defaults(), SetupProfile::Skirmish);
            snapshot.credits = credits;
            snapshot.unit_count = units;
            snapshot.game_speed = speed;
            let mut runtime = runtime(snapshot, 7);
            let mut shell = SkirmishShellState {
                trackbar_bounds: bounds,
                ..Default::default()
            };
            runtime.hydrate_shell(&mut shell, &[map()], &[mode()]);
            assert_eq!(
                (
                    shell.starting_credits,
                    shell.credits(),
                    shell.unit_count,
                    shell.game_speed
                ),
                expected,
                "saved {credits} {units} {speed}"
            );
            // The game starts with, and the settings keep, what the slider
            // reads back (`0x006AD742..0x006AD79E`).
            runtime.pack_shell_snapshot(&shell, &[map()], &[mode()]);
            assert_eq!(runtime.snapshot().credits, expected.1);
        }
    }

    #[test]
    fn hydrate_and_pack_preserve_raw_random_slot_sentinels() {
        let mut snapshot =
            MultiplayerSetupSnapshot::from_global_defaults(defaults(), SetupProfile::Skirmish);
        snapshot.slots[0] = SkirmishPersistedSlot {
            row_type: 4,
            country: RANDOM_ITEM_DATA,
            colour: RANDOM_ITEM_DATA,
        };
        let expected_slot = snapshot.slots[0];
        let mut runtime = runtime(snapshot, 7);
        let mut shell = SkirmishShellState::default();

        runtime.hydrate_shell(&mut shell, &[map()], &[mode()]);
        assert_eq!(shell.opponents[0].row_type, SkirmishAiRowType::Hard);
        assert!(shell.opponents[0].country_random);
        assert!(!shell.opponents[0].color_claimed);

        runtime.pack_shell_snapshot(&shell, &[map()], &[mode()]);
        assert_eq!(runtime.snapshot().slots[0], expected_slot);
    }

    #[test]
    fn scroll_text_keys_default_on_and_keep_the_held_value() {
        use crate::sim::scenario_session::ScenarioGameMode;
        let mut preferences = LocalMultiplayerPreferences::default();
        read_local_preference_fields(&IniFile::from_str("[MultiPlayer]\n"), &mut preferences);
        assert!(preferences.lan_scroll_text && preferences.wol_scroll_text);
        read_local_preference_fields(
            &IniFile::from_str("[MultiPlayer]\nLANScrollText=no\nWOLScrollText=junk\n"),
            &mut preferences,
        );
        assert!(!preferences.lan_scroll_text);
        assert!(
            preferences.wol_scroll_text,
            "an unparsed value keeps the held one"
        );
        let runtime = MultiplayerShellRuntime {
            local_preferences: preferences,
            ..MultiplayerShellRuntime::initialize(7, None, None, None, defaults())
        };
        assert!(!runtime.scroll_text(ScenarioGameMode::Lan));
        assert!(runtime.scroll_text(ScenarioGameMode::Wol));
        assert!(runtime.scroll_text(ScenarioGameMode::Skirmish));
    }

    #[test]
    fn repeated_settings_reads_use_retained_ini_and_canonical_country_allocation() {
        use crate::map::source::test_support::TestDirectory;
        let directory = TestDirectory::new("multiplayer-retained-country");
        directory.write(
            "rulesmd.ini",
            b"[Countries]\n0=Americans\n[Americans]\nSide=Allied\n[Sides]\nAllied=Americans\n",
        );
        directory.write("artmd.ini", b"[Fixture]\n");
        directory.write(
            RA2MD_INI,
            b"[MultiPlayer]\nSide=ABCDEFGHIJKLMNOPQRSTUVWXY\nColor=99\n",
        );
        let assets = AssetManager::from_loose_root_for_test(directory.path());
        let mut process = crate::app::process_assets::ProcessAssets::new(
            crate::assets::asset_manager::MediaArchiveMode::STOCK_DIGITAL,
            false,
        );
        let (rules, _) = process.initialize_sources_if_needed(&assets).unwrap();
        let mut runtime = MultiplayerShellRuntime::initialize(
            7,
            Some(directory.path()),
            Some(&assets),
            rules.as_ref(),
            defaults(),
        );
        runtime.read_multiplayer_settings(&mut process);
        assert_eq!(runtime.local_preferences.side, 1);
        assert_eq!(runtime.local_participant().slot.country.raw_index(), 1);
        runtime.read_multiplayer_settings(&mut process);
        assert_eq!(
            runtime.local_preferences.side, 2,
            "475540 rereads the original long token; constructor IDs truncate at24bytes"
        );
        directory.write(RA2MD_INI, b"[MultiPlayer]\nSide=Americans\nColor=0\n");
        runtime.read_multiplayer_settings(&mut process);
        assert_eq!(runtime.local_preferences.side, 3);
        assert_eq!(runtime.local_preferences.color_index, 99);
        // Simulate a persistence failure after WriteINI changed process8870C0.
        runtime.ini_path = Some(directory.path().to_owned());
        runtime.persist_local_preferences();
        let retained = IniFile::from_bytes(&runtime.settings_ini).unwrap();
        assert_eq!(
            retained
                .section_or_empty("MultiPlayer")
                .read_name("Side", 128),
            Some("ABCDEFGHIJKLMNOPQRSTUVWX")
        );
        assert_eq!(
            retained
                .section_or_empty("MultiPlayer")
                .read_int("Color", 0),
            99
        );
        runtime.read_multiplayer_settings(&mut process);
        assert_eq!(
            runtime.local_preferences.side, 1,
            "serialized canonical ID resolves the first allocated match"
        );
    }

    #[test]
    fn persistent_raw_side_and_color_are_not_rewritten_by_typed_projection() {
        let preferences = read_local_multiplayer_preferences(
            b"[MultiPlayer]\nSide=<random>\nSideEx=-1\nColor=-19\nColorEx=42\n",
        );
        assert_eq!(preferences.side, -2);
        assert_eq!(preferences.color_index, -19);
        let bytes = preferences.update_ini_bytes(b"", "<random>");
        let ini = IniFile::from_bytes(&bytes).unwrap();
        let section = ini.section_or_empty("MultiPlayer");
        assert_eq!(section.read_int("Color", 0), -19);
        assert_eq!(section.read_int("ColorEx", 0), 42);
        let mut runtime = runtime(
            MultiplayerSetupSnapshot::from_global_defaults(defaults(), SetupProfile::Skirmish),
            7,
        );
        runtime.local_preferences = preferences;
        let mut observer = runtime.local_participant();
        assert_eq!(runtime.local_preferences.color_index, -19);
        observer.set_observer(true);
        runtime.update_local_preferences(&observer, 1);
        assert_eq!(runtime.local_preferences.side, -3);
        assert_eq!(runtime.local_preferences.side_ex, -1);
        let bytes = runtime.local_preferences.update_ini_bytes(b"", "<random>");
        assert_eq!(
            IniFile::from_bytes(&bytes)
                .unwrap()
                .section_or_empty("MultiPlayer")
                .read_name("Side", 128),
            Some("<random>")
        );
    }

    #[test]
    fn retail_multiplayer_preferences_decode_into_local_shell_state() {
        let preferences = read_local_multiplayer_preferences(
            b"[MultiPlayer]\n\
              Handle=5b,4e,65,77,20,50,6c,61,79,65,72,5d,\n\
              Color=2\n\
              ColorEx=-1\n\
              Side=Americans\n\
              SideEx=-1\n",
        );
        assert_eq!(
            preferences,
            LocalMultiplayerPreferences {
                wol_scroll_text: true,
                lan_scroll_text: true,
                handle_bytes: b"[New Player]".to_vec(),
                side: menu_country_item_data(LaunchCountry::America),
                side_ex: CONCRETE_ITEM_DATA,
                color_index: 2,
                color_ex: CONCRETE_ITEM_DATA,
                game_mode: DEFAULT_MULTIPLAYER_GAME_MODE,
            }
        );

        let mut runtime = runtime(
            MultiplayerSetupSnapshot::from_global_defaults(defaults(), SetupProfile::Skirmish),
            7,
        );
        runtime.local_preferences = preferences;
        let mut shell = SkirmishShellState::default();
        runtime.hydrate_shell(&mut shell, &[map()], &[mode()]);

        assert_eq!(shell.player_name_edit.text, "[New Player]");
        assert_eq!(shell.player_country, LaunchCountry::America);
        assert!(!shell.player_country_random);
        assert_eq!(shell.player_color_index, 2);
        assert!(shell.player_color_claimed);
    }

    #[test]
    fn gsi_03_10_multiplayer_preferences_hydrate_respects_exact_ex_markers() {
        let preferences = read_local_multiplayer_preferences(
            b"[MultiPlayer]\n\
              Handle=52,61,6e,64,6f,6d,\n\
              Color=7\n\
              ColorEx=-2\n\
              Side=YuriCountry\n\
              SideEx=-2\n\
              GameMode=9\n",
        );
        let mut runtime = runtime(
            MultiplayerSetupSnapshot::from_global_defaults(defaults(), SetupProfile::Skirmish),
            7,
        );
        runtime.local_preferences = preferences;
        let mut shell = SkirmishShellState::default();
        runtime.hydrate_shell(&mut shell, &[map()], &[mode()]);

        assert_eq!(shell.player_country, LaunchCountry::Yuri);
        assert!(shell.player_country_random);
        assert_eq!(shell.player_color_index, 7);
        assert!(!shell.player_color_claimed);

        let concrete = read_local_multiplayer_preferences(
            b"[MultiPlayer]\nColor=7\nColorEx=-1\nSide=YuriCountry\nSideEx=-3\n",
        );
        runtime.local_preferences = concrete;
        runtime.hydrate_shell(&mut shell, &[map()], &[mode()]);
        assert_eq!(shell.player_country, LaunchCountry::Yuri);
        assert!(
            !shell.player_country_random,
            "only exact -2 restores Random"
        );
        assert_eq!(shell.player_color_index, 7);
        assert!(shell.player_color_claimed);
    }

    #[test]
    fn gsi_03_10_multiplayer_preferences_random_close_keeps_resolved_cache_and_markers() {
        let mut active_runtime = runtime(
            MultiplayerSetupSnapshot::from_global_defaults(defaults(), SetupProfile::Skirmish),
            0x1234,
        );
        let shell = SkirmishShellState {
            player_name_edit: PlayerNameEditState::with_name("12345678901234567890"),
            player_country_random: true,
            player_color_claimed: false,
            ..Default::default()
        };
        let session = random_session();

        let resolved = active_runtime
            .close_shell_transaction(&shell, &[map()], &[mode()], &session)
            .expect("random close");
        assert_eq!(
            active_runtime.local_preferences.handle_bytes.as_slice(),
            b"1234567890123456789"
        );
        assert_eq!(
            LaunchCountry::from_country_index(active_runtime.local_preferences.side as u32),
            resolved.local_slot().country
        );
        assert_eq!(active_runtime.local_preferences.side_ex, RANDOM_ITEM_DATA);
        assert_eq!(
            active_runtime.local_preferences.color_index,
            i32::from(resolved.local_slot().color_index)
        );
        assert_eq!(active_runtime.local_preferences.color_ex, RANDOM_ITEM_DATA);

        let bytes = active_runtime.update_persistence_bytes(b"[Other]\nKeep=1\n");
        let reloaded = read_local_multiplayer_preferences(&bytes);
        let mut reopened = runtime(
            MultiplayerSetupSnapshot::from_global_defaults(defaults(), SetupProfile::Skirmish),
            9,
        );
        reopened.local_preferences = reloaded;
        let mut reopened_shell = SkirmishShellState::default();
        reopened.hydrate_shell(&mut reopened_shell, &[map()], &[mode()]);
        assert!(reopened_shell.player_country_random);
        assert!(!reopened_shell.player_color_claimed);
        assert_eq!(reopened_shell.player_country, resolved.local_slot().country);
        assert_eq!(
            reopened_shell.player_color_index,
            usize::from(resolved.local_slot().color_index)
        );
    }

    #[test]
    fn gsi_03_10_multiplayer_preferences_concrete_close_round_trips_minus_one() {
        let mut active_runtime = runtime(
            MultiplayerSetupSnapshot::from_global_defaults(defaults(), SetupProfile::Skirmish),
            0x5678,
        );
        let shell = SkirmishShellState {
            player_country: LaunchCountry::Yuri,
            player_country_random: false,
            player_color_index: 7,
            player_color_claimed: true,
            ..Default::default()
        };
        let mut session = random_session();
        session.local_slot_mut().country = LaunchCountry::Yuri;
        session.local_slot_mut().country_random = false;
        session.local_slot_mut().color_index = 7;
        session.local_slot_mut().color_random = false;

        let resolved = active_runtime
            .close_shell_transaction(&shell, &[map()], &[mode()], &session)
            .expect("concrete close");
        assert_eq!(resolved.local_slot().country, LaunchCountry::Yuri);
        assert_eq!(resolved.local_slot().color_index, 7);
        assert_eq!(active_runtime.local_preferences.side_ex, CONCRETE_ITEM_DATA);
        assert_eq!(
            active_runtime.local_preferences.color_ex,
            CONCRETE_ITEM_DATA
        );

        let bytes = active_runtime.update_persistence_bytes(&[]);
        let text = String::from_utf8_lossy(&bytes);
        assert!(text.contains("Side=YuriCountry"));
        assert!(text.contains("SideEx=-1"));
        assert!(text.contains("Color=7"));
        assert!(text.contains("ColorEx=-1"));

        let mut reopened = runtime(
            MultiplayerSetupSnapshot::from_global_defaults(defaults(), SetupProfile::Skirmish),
            9,
        );
        reopened.local_preferences = read_local_multiplayer_preferences(&bytes);
        let mut reopened_shell = SkirmishShellState::default();
        reopened.hydrate_shell(&mut reopened_shell, &[map()], &[mode()]);
        assert_eq!(reopened_shell.player_country, LaunchCountry::Yuri);
        assert!(!reopened_shell.player_country_random);
        assert_eq!(reopened_shell.player_color_index, 7);
        assert!(reopened_shell.player_color_claimed);
    }

    #[test]
    fn gsi_03_10_multiplayer_preferences_codec_is_ordered_and_preserving() {
        let mut runtime = runtime(
            MultiplayerSetupSnapshot::from_global_defaults(defaults(), SetupProfile::Skirmish),
            7,
        );
        runtime.local_preferences = LocalMultiplayerPreferences {
            wol_scroll_text: true,
            lan_scroll_text: true,
            handle_bytes: vec![0x4a, 0x6f, 0x73, 0xe9],
            side: menu_country_item_data(LaunchCountry::Yuri),
            side_ex: CONCRETE_ITEM_DATA,
            color_index: 7,
            color_ex: CONCRETE_ITEM_DATA,
            game_mode: 9,
        };
        let bytes = runtime.update_persistence_bytes(
            b"; keep this comment\n[Other]\nKeep=1\n[MultiPlayer]\nLegacy=yes\n",
        );
        let text = String::from_utf8_lossy(&bytes);

        assert!(text.contains("; keep this comment"));
        assert!(text.contains("[Other]"));
        assert!(text.contains("Keep=1"));
        assert!(text.contains("Legacy=yes"));
        assert!(text.contains("Handle=4a,6f,73,e9,"));
        assert!(text.contains("[Skirmish]"));

        let positions = [
            "Handle=",
            "Color=",
            "ColorEx=",
            "Side=",
            "SideEx=",
            "GameMode=",
        ]
        .map(|needle| text.find(needle).expect("persisted key"));
        assert!(positions.windows(2).all(|pair| pair[0] < pair[1]));
    }

    #[test]
    fn gsi_03_10_multiplayer_preferences_missing_keys_write_complete_session_defaults() {
        let preferences =
            read_local_multiplayer_preferences(b"[MultiPlayer]\nColor=7\n[Skirmish]\nGameMode=9\n");
        assert_eq!(preferences.handle_bytes, b"[New Player]");
        assert_eq!(preferences.side, 0);
        assert_eq!(preferences.side_ex, 0);
        assert_eq!(preferences.color_index, 7);
        assert_eq!(preferences.color_ex, 0);
        assert_eq!(preferences.game_mode, DEFAULT_MULTIPLAYER_GAME_MODE);

        let mut snapshot =
            MultiplayerSetupSnapshot::from_global_defaults(defaults(), SetupProfile::Skirmish);
        snapshot.game_mode = 9;
        let mut runtime = runtime(snapshot, 0x6789);
        runtime.local_preferences = preferences;
        let mode_9 = SkirmishGameMode { id: 9, ..mode() };
        let mut shell = SkirmishShellState::default();
        runtime.hydrate_shell(&mut shell, &[map()], &[mode_9.clone(), mode()]);
        assert_eq!(
            shell.selected_mode_id, 1,
            "MultiPlayer default, not conflicting Skirmish mode, owns restore"
        );

        let bytes =
            runtime.update_persistence_bytes(b"[MultiPlayer]\nColor=7\n[Skirmish]\nGameMode=9\n");
        let text = String::from_utf8_lossy(&bytes);

        assert!(text.contains("Handle=5b,4e,65,77,20,50,6c,61,79,65,72,5d,"));
        assert!(text.contains("Color=7"));
        assert!(text.contains("ColorEx=0"));
        assert!(text.contains("Side=Americans"));
        assert!(text.contains("SideEx=0"));
        assert!(text.contains("[MultiPlayer]\n"));
        assert!(text.contains("GameMode=1"));
        assert!(text.contains("[Skirmish]\nGameMode=9"));

        runtime.local_preferences.game_mode = 999;
        runtime.hydrate_shell(&mut shell, &[map()], &[mode_9, mode()]);
        assert_eq!(shell.selected_mode_id, 9, "unknown mode falls to first row");
    }

    #[cfg(windows)]
    #[test]
    fn gsi_03_10_multiplayer_preferences_non_identity_acp_byte_round_trips() {
        let mut active_runtime = runtime(
            MultiplayerSetupSnapshot::from_global_defaults(defaults(), SetupProfile::Skirmish),
            0x89ab,
        );
        let mut shell = SkirmishShellState::default();
        shell.player_name_edit = PlayerNameEditState::with_name("\u{20ac}");

        active_runtime
            .close_shell_transaction(&shell, &[map()], &[mode()], &random_session())
            .expect("close with ACP-only handle");
        assert_eq!(active_runtime.local_preferences.handle_bytes, vec![0x80]);

        let bytes = active_runtime.update_persistence_bytes(&[]);
        let text = String::from_utf8_lossy(&bytes);
        assert!(text.contains("Handle=80,"));

        let mut reopened = runtime(
            MultiplayerSetupSnapshot::from_global_defaults(defaults(), SetupProfile::Skirmish),
            9,
        );
        reopened.local_preferences = read_local_multiplayer_preferences(&bytes);
        assert_eq!(reopened.local_preferences.handle_bytes, vec![0x80]);

        let mut reopened_shell = SkirmishShellState::default();
        reopened.hydrate_shell(&mut reopened_shell, &[map()], &[mode()]);
        assert_eq!(reopened_shell.player_name_edit.text, "\u{20ac}");
    }

    #[test]
    fn gsi_03_10_multiplayer_preferences_failed_close_keeps_cached_fields() {
        let mut runtime = cooperative_runtime(0x2345);
        runtime.local_preferences = LocalMultiplayerPreferences {
            wol_scroll_text: true,
            lan_scroll_text: true,
            handle_bytes: b"Cached".to_vec(),
            side: menu_country_item_data(LaunchCountry::Russia),
            side_ex: CONCRETE_ITEM_DATA,
            color_index: 3,
            color_ex: CONCRETE_ITEM_DATA,
            game_mode: 1,
        };
        let cached = runtime.local_preferences.clone();
        let mut session = random_session();
        session.mode = SkirmishLaunchMode::from_game_mode(&cooperative_mode());
        session.selected_map_file = Some("A2B".to_string());

        assert!(
            runtime
                .close_shell_transaction(
                    &SkirmishShellState::default(),
                    &[map_named("A2B")],
                    &[cooperative_mode()],
                    &session,
                )
                .is_err()
        );
        assert_eq!(runtime.local_preferences, cached);
    }

    #[test]
    fn malformed_multiplayer_preferences_fall_back_independently() {
        let preferences =
            read_local_multiplayer_preferences(b"[MultiPlayer]\nHandle=100,41,\nColor=99\n");
        assert_eq!(
            preferences,
            LocalMultiplayerPreferences {
                color_index: 99,
                ..LocalMultiplayerPreferences::default()
            }
        );
        let handle = |value: &str| {
            read_local_multiplayer_preferences(
                format!("[MultiPlayer]\nHandle={value}\n").as_bytes(),
            )
            .handle_bytes
        };
        assert_eq!(handle(""), DEFAULT_MULTIPLAYER_HANDLE);
        assert_eq!(handle("41,00,42,"), b"A");
        assert_eq!(handle("e9,"), [0xe9]);
        assert_eq!(handle("20ac,"), [0xac]);
        assert_eq!(
            handle(&"41,".repeat(25)),
            [b'A'; MULTIPLAYER_HANDLE_LIMIT_BYTES]
        );
    }

    #[test]
    fn resolving_launch_copy_does_not_mutate_raw_random_fields() {
        let snapshot =
            MultiplayerSetupSnapshot::from_global_defaults(defaults(), SetupProfile::Skirmish);
        let mut runtime = runtime(snapshot, 11);
        let session = random_session();

        let resolved = runtime
            .resolve_launch_session(&session, &[])
            .expect("ordinary resolver");

        assert!(session.local_slot().country_random);
        assert!(session.local_slot().color_random);
        assert!(session.opponents[0].country_random);
        assert!(session.opponents[0].color_random);
        assert!(!resolved.local_slot().country_random);
        assert!(!resolved.local_slot().color_random);
        assert!(!resolved.opponents[0].country_random);
        assert!(!resolved.opponents[0].color_random);
    }

    #[test]
    fn start_and_back_use_identical_shell_resolution_transcripts() {
        let snapshot =
            MultiplayerSetupSnapshot::from_global_defaults(defaults(), SetupProfile::Skirmish);
        let mut start_runtime = runtime(snapshot.clone(), 0x1234);
        let mut back_runtime = runtime(snapshot, 0x1234);
        let session = random_session();
        let shell = SkirmishShellState::default();

        let start_copy = start_runtime
            .close_shell_transaction(&shell, &[map()], &[mode()], &session)
            .expect("Start shell resolution");
        let back_copy = back_runtime
            .close_shell_transaction(&shell, &[map()], &[mode()], &session)
            .expect("Back shell resolution");

        assert_eq!(start_copy, back_copy);
        assert_eq!(start_runtime.snapshot(), back_runtime.snapshot());
        assert_eq!(
            start_runtime.scenario_rng_state(),
            back_runtime.scenario_rng_state()
        );
    }

    #[test]
    fn failed_start_validation_leaves_runtime_snapshot_and_rng_untouched() {
        let snapshot =
            MultiplayerSetupSnapshot::from_global_defaults(defaults(), SetupProfile::Skirmish);
        let runtime = runtime(snapshot.clone(), 0x5678);
        let rng_before = runtime.scenario_rng_state();
        let mut shell = SkirmishShellState {
            selected_mode_id: mode().id,
            ..Default::default()
        };
        for opponent in &mut shell.opponents {
            opponent.row_type = SkirmishAiRowType::None;
            opponent.enabled = false;
        }

        let result = crate::ui::skirmish_shell::launch_session(&shell, &[map()], &[mode()]);

        assert_eq!(
            result,
            Err(crate::skirmish_launch::LaunchValidationError::NoEnabledOpponent)
        );
        assert_eq!(runtime.snapshot(), &snapshot);
        assert_eq!(runtime.scenario_rng_state(), rng_before);
    }

    #[test]
    fn cooperative_selection_uses_progress_chosen_map_not_clicked_variant() {
        let mut runtime = cooperative_runtime(0x2345);

        let first_campaign_map = runtime
            .ensure_cooperative_selection("A2B", &cooperative_maps())
            .expect("bind active campaign")
            .expect("chosen active map");
        assert!(["A1", "A1B", "A1C"].contains(&first_campaign_map.as_str()));
        assert_eq!(
            runtime
                .cooperative_progress
                .active()
                .map(|progress| (progress.campaign_type, progress.current_map)),
            Some((0, 0))
        );

        let switched_map = runtime
            .accept_cooperative_selection("W1B", &cooperative_maps())
            .expect("accept campaign swap")
            .expect("chosen switched map");
        assert!(["W1", "W1B", "W1C"].contains(&switched_map.as_str()));
        assert_eq!(
            runtime
                .cooperative_progress
                .active()
                .map(|progress| (progress.campaign_type, progress.current_map)),
            Some((1, 0))
        );
    }

    #[test]
    fn cooperative_resolution_replaces_clicked_variant_with_progress_choice() {
        let mut runtime = cooperative_runtime(0x3456);
        let mut session = random_session();
        session.mode = SkirmishLaunchMode::from_game_mode(&cooperative_mode());
        session.selected_map_file = Some("A2C".to_string());
        session.local_slot_mut().country_random = false;
        session.local_slot_mut().color_random = false;
        session.opponents[0].country_random = false;
        session.opponents[0].color_random = false;
        let shell = SkirmishShellState::default();

        let resolved = runtime
            .close_shell_transaction(&shell, &cooperative_maps(), &[cooperative_mode()], &session)
            .expect("Cooperative launch resolution");

        assert!(
            ["A1", "A1B", "A1C"]
                .contains(&resolved.selected_map_file.as_deref().unwrap_or_default())
        );
    }

    #[test]
    fn missing_cooperative_map_rolls_back_progress_and_rng() {
        let mut runtime = cooperative_runtime(0x4567);
        let rng_before = runtime.scenario_rng_state();
        let progress_before = runtime.cooperative_progress.clone();

        let error = runtime
            .ensure_cooperative_selection("A2B", &[map_named("A2B")])
            .expect_err("the stage-zero chosen variant is absent");

        assert!(matches!(error, CooperativeError::MissingScenarioMap { .. }));
        assert_eq!(runtime.scenario_rng_state(), rng_before);
        assert_eq!(runtime.cooperative_progress, progress_before);
    }

    #[test]
    fn failed_cooperative_swap_rolls_back_progress_and_rng() {
        let mut runtime = cooperative_runtime(0x5678);
        runtime
            .ensure_cooperative_selection("A2B", &cooperative_maps())
            .expect("bind first campaign");
        let rng_before = runtime.scenario_rng_state();
        let progress_before = runtime.cooperative_progress.clone();
        let allied_maps: Vec<_> = cooperative_maps()
            .into_iter()
            .filter(|map| map.file_name.starts_with('A'))
            .collect();

        let error = runtime
            .accept_cooperative_selection("W1B", &allied_maps)
            .expect_err("the selected World variant is absent");

        assert!(matches!(error, CooperativeError::MissingScenarioMap { .. }));
        assert_eq!(runtime.scenario_rng_state(), rng_before);
        assert_eq!(runtime.cooperative_progress, progress_before);
    }

    #[test]
    fn scenario_return_and_failed_load_preserve_native_header_lifecycle() {
        let snapshot =
            MultiplayerSetupSnapshot::from_global_defaults(defaults(), SetupProfile::Skirmish);
        let mut runtime = runtime(snapshot, 1);
        let mut returned = crate::map::preview::ScenarioStartHeader::default();
        returned.read_ini(&IniFile::from_str("[Header]\nNumCoopHumanStartSpots=2\n"));
        let replacement = SimRng::new(99);
        let expected = replacement.state();
        assert!(!runtime.capture_returned_gameplay_scenario(replacement.clone(), returned));
        assert_ne!(runtime.scenario_rng_state(), expected);
        assert_eq!(runtime.scenario_start_header.coop_human_start_spots(), 4);
        runtime.mark_gameplay_scenario_return_pending();
        assert!(runtime.capture_returned_gameplay_scenario(replacement, returned));
        assert_eq!(runtime.scenario_rng_state(), expected);
        assert_eq!(runtime.scenario_start_header.coop_human_start_spots(), 2);
        runtime.observe_loaded_scenario_header(&IniFile::from_str("[Header]\n"));
        assert_eq!(runtime.scenario_start_header.coop_human_start_spots(), 2);
        runtime.begin_fresh_scenario_load();
        // A failure before ReadStartPoints leaves Set_Defaults' value.
        assert_eq!(runtime.scenario_start_header.coop_human_start_spots(), 4);
        runtime.observe_loaded_scenario_header(&IniFile::from_str(
            "[Header]\nNumCoopHumanStartSpots=3\n",
        ));
        assert_eq!(runtime.scenario_start_header.coop_human_start_spots(), 3);
    }

    #[test]
    fn gsi_04_12_first_random_map_entry_spends_one_scenario_draw_and_reentry_spends_none() {
        let snapshot =
            MultiplayerSetupSnapshot::from_global_defaults(defaults(), SetupProfile::Skirmish);
        let seed = 0x4321_u32;
        let mut runtime = runtime(snapshot, seed);
        let mut reference = SimRng::new(u64::from(seed));
        let expected_seed = reference.next_range_u32_inclusive(0, 0xFFFF) as i32;

        let mut first = runtime.random_map_options_for_setup();
        assert_eq!(first.seed, expected_seed);
        assert_eq!(
            runtime.scenario_rng.logical_state(),
            reference.logical_state()
        );

        first.resources = 3;
        first.num_players = 7;
        runtime.remember_random_map_options(&first);
        let before_reentry = runtime.scenario_rng.logical_state();
        let reopened = runtime.random_map_options_for_setup();

        assert_eq!(reopened.seed, expected_seed);
        assert_eq!(reopened.resources, 3);
        assert_eq!(reopened.num_players, 7);
        assert_eq!(runtime.scenario_rng.logical_state(), before_reentry);
    }

    #[test]
    fn gsi_04_12_preview_trace_spends_one_raw_scenario_word_per_constructor() {
        use crate::map::rmg::{RmgConstructionPhase, RmgConstructionTrace};

        let snapshot =
            MultiplayerSetupSnapshot::from_global_defaults(defaults(), SetupProfile::Skirmish);
        let seed = 0x1357_u32;
        let mut runtime = runtime(snapshot, seed);
        let mut reference = SimRng::new(u64::from(seed));
        let mut trace = RmgConstructionTrace::default();
        trace.push_emitted(
            RmgConstructionPhase::BridgeRepairHut,
            "CABHUT".to_string(),
            0,
            (12, 13),
        );
        trace.push_discarded(RmgConstructionPhase::NeutralTech, "CAOILD".to_string());
        trace.push_emitted(
            RmgConstructionPhase::NeutralTech,
            "CATHOSP".to_string(),
            1,
            (20, 21),
        );

        for _ in &trace.events {
            let _ = reference.next_u32();
        }
        runtime.replay_random_map_preview_construction(&trace);

        assert_eq!(
            runtime.scenario_rng.logical_state(),
            reference.logical_state()
        );
    }

    #[test]
    fn invalid_persisted_indices_clamp_to_first_mode_and_map() {
        let mut snapshot =
            MultiplayerSetupSnapshot::from_global_defaults(defaults(), SetupProfile::Skirmish);
        snapshot.game_mode = 999;
        snapshot.scenario_index = 999;
        let runtime = runtime(snapshot, 3);
        let mut shell = SkirmishShellState {
            player_start_position: LaunchStartPosition::Auto,
            ..Default::default()
        };

        runtime.hydrate_shell(&mut shell, &[map()], &[mode()]);

        assert_eq!(shell.selected_mode_id, 1);
        assert_eq!(shell.selected_map_idx, 0);
    }
}
