//! Offline Skirmish shell process state and close-transaction helpers.
//!
//! The UI owns raw controls, including Random sentinels. This app-layer owner
//! retains the durable `[Skirmish]` snapshot, the process-continuity Scenario
//! RNG, and Cooperative progress records across shell closes and matches.

use std::io;
use std::path::{Path, PathBuf};

use crate::app::loading::init::MapMenuEntry;
use crate::assets::asset_manager::AssetManager;
use crate::rules::ini_parser::IniFile;
use crate::sim::rng::SimRng;
use crate::skirmish_cooperative::{
    CooperativeCountryRole, CooperativeCountryRosterEntry, CooperativeError,
    CooperativeProgressRecord, CooperativeProgressState, CooperativeRegistry,
    draw_country_for_progress,
};
use crate::skirmish_launch::{
    LaunchCountry, RandomCountryRole, ShellRandomAssignmentState, SkirmishLaunchSession,
};
use crate::skirmish_modes::{SkirmishGameMode, mode_by_id};
use crate::skirmish_persistence::{
    SKIRMISH_PERSISTED_SLOT_COUNT, SkirmishGlobalDefaults, SkirmishPersistedSlot,
    SkirmishPersistedSnapshot, read_skirmish_snapshot,
};
use crate::ui::main_menu::SkirmishCountry;
use crate::ui::skirmish_shell::{
    PlayerNameEditState, SkirmishAiRowType, SkirmishShellState, SkirmishTrackbarId,
    game_speed_from_visual_position, game_speed_visual_position,
};

const RA2MD_INI: &str = "RA2MD.INI";
const CONCRETE_ITEM_DATA: i32 = -1;
const RANDOM_ITEM_DATA: i32 = -2;
const MULTIPLAYER_HANDLE_LIMIT_BYTES: usize = 19;
const DEFAULT_MULTIPLAYER_HANDLE: &[u8] = b"[New Player]";
const DEFAULT_MULTIPLAYER_GAME_MODE: i32 = 1;

#[derive(Debug, Clone, PartialEq, Eq)]
struct LocalMultiplayerPreferences {
    handle_bytes: Vec<u8>,
    country: SkirmishCountry,
    side_ex: i32,
    color_index: usize,
    color_ex: i32,
    game_mode: i32,
}

impl Default for LocalMultiplayerPreferences {
    fn default() -> Self {
        Self {
            handle_bytes: DEFAULT_MULTIPLAYER_HANDLE.to_vec(),
            country: SkirmishCountry::America,
            side_ex: 0,
            color_index: 0,
            color_ex: 0,
            game_mode: DEFAULT_MULTIPLAYER_GAME_MODE,
        }
    }
}

impl LocalMultiplayerPreferences {
    /// Apply the six fields written by `SessionClass__WriteMultiPlayerSettings`
    /// at 0x006990A0. The reader at 0x006980C0 first establishes a complete
    /// Session cache from constructor/string-table defaults, so pump/dialog
    /// exits still write all six fields when keys were absent from RA2MD.INI.
    fn update_ini_bytes(&self, content: &[u8]) -> Vec<u8> {
        let handle = encode_multiplayer_handle(&self.handle_bytes);
        let color = self.color_index.to_string();
        let color_ex = self.color_ex.to_string();
        let side_ex = self.side_ex.to_string();
        let game_mode = self.game_mode.to_string();
        let updates = [
            ("Handle", handle.as_str()),
            ("Color", color.as_str()),
            ("ColorEx", color_ex.as_str()),
            ("Side", self.country.country_name()),
            ("SideEx", side_ex.as_str()),
            ("GameMode", game_mode.as_str()),
        ];
        crate::util::ini_writer::set_ini_values(content, "MultiPlayer", &updates)
    }
}

/// App-owned state whose lifetime matches the front-end process, not one map.
pub(crate) struct OfflineSkirmishRuntime {
    snapshot: SkirmishPersistedSnapshot,
    local_preferences: LocalMultiplayerPreferences,
    scenario_rng: SimRng,
    /// Process-persistent native MapSeed options. The setup dialog edits a
    /// working copy, but closing it does not reconstruct this global record.
    random_map_options: crate::map::rmg::RmgOptions,
    ini_path: Option<PathBuf>,
    cooperative_registry: CooperativeRegistry,
    cooperative_progress: CooperativeProgressState,
    cooperative_country_roster: Vec<CooperativeCountryRosterEntry>,
    gameplay_rng_return_pending: bool,
}

impl OfflineSkirmishRuntime {
    /// Establish the front-end Scenario cursor, then construct Cooperative
    /// progress before reading the durable shell snapshot. Stock Cooperative
    /// data advances the freshly seeded cursor by ten logical `(0,2)` calls.
    pub(crate) fn initialize(
        seed: u32,
        ra2_dir: Option<&Path>,
        assets: Option<&AssetManager>,
        startup_rules_projection: Option<&crate::rules::ini_parser::IniFile>,
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
        let cooperative_country_roster = startup_rules_projection
            .map(cooperative_country_roster_from_rules)
            .unwrap_or_else(stock_cooperative_country_roster);
        let ini_path = ra2_dir.map(|root| root.join(RA2MD_INI));
        let (snapshot, local_preferences) = load_persistence(ini_path.as_deref(), defaults);

        Self {
            snapshot,
            local_preferences,
            scenario_rng,
            random_map_options: crate::map::rmg::RmgOptions::default(),
            ini_path,
            cooperative_registry,
            cooperative_progress,
            cooperative_country_roster,
            gameplay_rng_return_pending: false,
        }
    }

    /// Re-read only the six process-cached `[MultiPlayer]` preferences before
    /// the launcher Options Network child route.
    ///
    /// gamemd-derived: `OptionsClass__ShowLauncherDialog @ 0x0055FC80` calls
    /// `SessionClass__ReadMultiPlayerSettings @ 0x006980C0` after destroying
    /// the `0xD5` primary and before constructing Network dialog `0xD7`.
    /// The existing parser establishes constructor defaults first, so a
    /// missing, unreadable, or malformed snapshot atomically replaces only
    /// this cache with defaults and cannot perturb shell/RNG/RMG/co-op state.
    pub(crate) fn refresh_local_multiplayer_preferences(&mut self) {
        let refreshed = match self.ini_path.as_deref() {
            Some(path) => match std::fs::read(path) {
                Ok(bytes) => read_local_multiplayer_preferences(&bytes),
                Err(error) if error.kind() == io::ErrorKind::NotFound => {
                    LocalMultiplayerPreferences::default()
                }
                Err(error) => {
                    log::warn!(
                        "Could not refresh Multiplayer settings from {}: {error}",
                        path.display()
                    );
                    LocalMultiplayerPreferences::default()
                }
            },
            None => LocalMultiplayerPreferences::default(),
        };
        self.local_preferences = refreshed;
    }

    #[cfg(test)]
    fn snapshot(&self) -> &SkirmishPersistedSnapshot {
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
        state.player_country = self.local_preferences.country;
        if self.local_preferences.side_ex == RANDOM_ITEM_DATA {
            state.player_country_random = true;
        } else {
            state.player_country_random = false;
        }
        state.player_color_index = self.local_preferences.color_index;
        if self.local_preferences.color_ex == RANDOM_ITEM_DATA {
            state.player_color_claimed = false;
        } else {
            state.player_color_claimed = true;
        }

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
            player_name: session.player_name.clone(),
            local: session.local.clone(),
            opponents: Vec::new(),
            pre_fill_house_roster: session.pre_fill_house_roster.clone(),
            options: session.options.clone(),
        };
        let cooperative = is_cooperative_mode(session);
        if cooperative && let Some(map) = session.selected_map_file.as_deref() {
            if let Some(chosen_map) = self.ensure_cooperative_selection(map, maps)? {
                staged_session.selected_map_file = Some(chosen_map);
            }
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
        let mut draw_country =
            |role, rng: &mut SimRng| -> Result<LaunchCountry, CooperativeError> {
                if let Some(progress) = progress.as_ref() {
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
            };

        assignments.resolve_local_country(&mut self.scenario_rng, &mut draw_country)?;
        assignments.pack_ai_assignments(&session.opponents);
        pack_snapshot_slots(&mut self.snapshot, state);
        assignments.resolve_local_color(&mut self.scenario_rng);
        assignments.resolve_ai(&mut self.scenario_rng, &mut draw_country)?;
        pack_snapshot_options(&mut self.snapshot, state);
        let resolved = assignments.finish();

        // Native Start/Back refreshes these cached SessionClass fields before
        // the unconditional writer at 0x006990A0. Keep the resolved concrete
        // values behind the raw -1/-2 markers so reopening the shell can show
        // Random without discarding the value selected for this launch.
        let mut handle_bytes = crate::util::native_string::acp_encode(&state.player_name_edit.text);
        handle_bytes.truncate(MULTIPLAYER_HANDLE_LIMIT_BYTES);
        self.local_preferences = LocalMultiplayerPreferences {
            handle_bytes,
            country: menu_country_from_launch(resolved.local.country),
            side_ex: if state.player_country_random {
                RANDOM_ITEM_DATA
            } else {
                CONCRETE_ITEM_DATA
            },
            color_index: usize::from(resolved.local.color_index),
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
        if let Some(map) = session.selected_map_file.as_deref() {
            if let Some(chosen_map) = self.ensure_cooperative_selection(map, maps)? {
                cooperative_session.selected_map_file = Some(chosen_map);
            }
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
    pub(crate) fn persist_snapshot(&self) {
        let Some(path) = self.ini_path.as_deref() else {
            return;
        };
        let existing = match std::fs::read(path) {
            Ok(bytes) => bytes,
            Err(err) if err.kind() == io::ErrorKind::NotFound => Vec::new(),
            Err(err) => {
                log::warn!(
                    "Could not read {} before Skirmish persistence: {err}",
                    path.display()
                );
                return;
            }
        };
        let updated = self.update_persistence_bytes(&existing);
        if let Err(err) = std::fs::write(path, updated) {
            log::warn!(
                "Could not persist Skirmish settings to {}: {err}",
                path.display()
            );
        }
    }

    /// Compose the active offline writer's two relevant sections in native
    /// order, then let the owner perform one final filesystem write.
    fn update_persistence_bytes(&self, existing: &[u8]) -> Vec<u8> {
        let updated = self.local_preferences.update_ini_bytes(existing);
        self.snapshot.update_ini_bytes(&updated)
    }

    pub(crate) fn mark_gameplay_rng_return_pending(&mut self) {
        self.gameplay_rng_return_pending = true;
    }

    /// Restore the process Scenario cursor from a normally returning match.
    pub(crate) fn capture_returned_gameplay_rng(&mut self, gameplay_rng: SimRng) -> bool {
        if !self.gameplay_rng_return_pending {
            return false;
        }
        self.scenario_rng = gameplay_rng;
        self.gameplay_rng_return_pending = false;
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
    snapshot: &mut SkirmishPersistedSnapshot,
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

fn pack_snapshot_slots(snapshot: &mut SkirmishPersistedSnapshot, state: &SkirmishShellState) {
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

fn pack_snapshot_options(snapshot: &mut SkirmishPersistedSnapshot, state: &SkirmishShellState) {
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

fn load_persistence(
    path: Option<&Path>,
    defaults: SkirmishGlobalDefaults,
) -> (SkirmishPersistedSnapshot, LocalMultiplayerPreferences) {
    let fallback = || {
        (
            SkirmishPersistedSnapshot::from_global_defaults(defaults),
            LocalMultiplayerPreferences::default(),
        )
    };
    let Some(path) = path else {
        return fallback();
    };
    match std::fs::read(path) {
        Ok(bytes) => {
            let snapshot = read_skirmish_snapshot(&bytes, defaults).unwrap_or_else(|err| {
                log::warn!(
                    "Could not parse Skirmish settings from {}: {err}",
                    path.display()
                );
                SkirmishPersistedSnapshot::from_global_defaults(defaults)
            });
            (snapshot, read_local_multiplayer_preferences(&bytes))
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

fn read_local_multiplayer_preferences(bytes: &[u8]) -> LocalMultiplayerPreferences {
    let mut preferences = LocalMultiplayerPreferences::default();
    let Ok(ini) = IniFile::from_bytes(bytes) else {
        return preferences;
    };
    let Some(section) = ini.section("MultiPlayer") else {
        return preferences;
    };

    if let Some(mut handle_bytes) = section
        .get("Handle")
        .and_then(decode_multiplayer_handle_bytes)
    {
        handle_bytes.truncate(MULTIPLAYER_HANDLE_LIMIT_BYTES);
        preferences.handle_bytes = handle_bytes;
    }
    if let Some(country) = section.get("Side").and_then(|side| {
        SkirmishCountry::ALL
            .into_iter()
            .find(|country| country.country_name().eq_ignore_ascii_case(side.trim()))
    }) {
        preferences.country = country;
    }
    if let Some(color_index) = section
        .get_i32("Color")
        .and_then(|value| usize::try_from(value).ok())
        .filter(|value| *value < crate::skirmish_launch::HOUSE_COLOR_COUNT)
    {
        preferences.color_index = color_index;
    }
    if let Some(side_ex) = section.get_i32("SideEx") {
        preferences.side_ex = side_ex;
    }
    if let Some(color_ex) = section.get_i32("ColorEx") {
        preferences.color_ex = color_ex;
    }
    if let Some(game_mode) = section.get_i32("GameMode") {
        preferences.game_mode = game_mode;
    }

    preferences
}

fn encode_multiplayer_handle(bytes: &[u8]) -> String {
    let mut encoded = String::with_capacity(bytes.len().saturating_mul(3));
    for byte in bytes {
        encoded.push_str(&format!("{byte:x},"));
    }
    encoded
}

fn menu_country_from_launch(country: LaunchCountry) -> SkirmishCountry {
    match country {
        LaunchCountry::America => SkirmishCountry::America,
        LaunchCountry::Korea => SkirmishCountry::Korea,
        LaunchCountry::France => SkirmishCountry::France,
        LaunchCountry::Germany => SkirmishCountry::Germany,
        LaunchCountry::GreatBritain => SkirmishCountry::GreatBritain,
        LaunchCountry::Libya => SkirmishCountry::Libya,
        LaunchCountry::Iraq => SkirmishCountry::Iraq,
        LaunchCountry::Cuba => SkirmishCountry::Cuba,
        LaunchCountry::Russia => SkirmishCountry::Russia,
        LaunchCountry::Yuri => SkirmishCountry::Yuri,
    }
}

fn decode_multiplayer_handle_bytes(value: &str) -> Option<Vec<u8>> {
    let mut bytes = Vec::new();
    for part in value.split(',') {
        let part = part.trim();
        if part.is_empty() {
            continue;
        }
        let unit = u32::from_str_radix(part, 16).ok()? as u16;
        let byte = unit as u8;
        if byte == 0 {
            break;
        }
        bytes.push(byte);
    }

    if bytes.is_empty() {
        return None;
    }
    Some(bytes)
}

#[cfg(test)]
fn decode_multiplayer_handle(value: &str) -> Option<String> {
    decode_multiplayer_handle_bytes(value)
        .map(|bytes| crate::util::native_string::acp_decode(&bytes))
}

fn is_cooperative_mode(session: &SkirmishLaunchSession) -> bool {
    session
        .mode
        .override_file
        .eq_ignore_ascii_case("MPCoopMD.ini")
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

fn menu_country_from_item_data(value: i32) -> Option<SkirmishCountry> {
    usize::try_from(value)
        .ok()
        .and_then(|index| SkirmishCountry::ALL.get(index).copied())
}

fn menu_country_item_data(country: SkirmishCountry) -> i32 {
    SkirmishCountry::ALL
        .iter()
        .position(|candidate| *candidate == country)
        .and_then(|index| i32::try_from(index).ok())
        .unwrap_or(0)
}

fn cooperative_country_roster_from_rules(
    rules: &crate::rules::ini_parser::IniFile,
) -> Vec<CooperativeCountryRosterEntry> {
    SkirmishCountry::ALL
        .into_iter()
        .map(|country| {
            let id = country.country_name();
            let name = rules
                .section(id)
                .and_then(|section| section.get("Name"));
            CooperativeCountryRosterEntry::new(id, name)
        })
        .collect()
}

fn stock_cooperative_country_roster() -> Vec<CooperativeCountryRosterEntry> {
    SkirmishCountry::ALL
        .into_iter()
        .map(|country| {
            CooperativeCountryRosterEntry::new(country.country_name(), Some(country.label()))
        })
        .collect()
}

const _: [(); SKIRMISH_PERSISTED_SLOT_COUNT] = [(); crate::skirmish_launch::SKIRMISH_AI_SLOT_COUNT];

#[cfg(test)]
mod tests {
    use super::*;
    use crate::map::briefing::BriefingSection;
    use crate::map::preview::PreviewSection;
    use crate::skirmish_launch::{
        AiDifficulty, LaunchStartPosition, LaunchTeam, SkirmishAiSlot, SkirmishLaunchMode,
        SkirmishLaunchOptions, SkirmishLocalSlot,
    };
    use crate::ui::main_menu::StartPosition;

    fn map_named(file_name: &str) -> MapMenuEntry {
        MapMenuEntry {
            file_name: file_name.to_string(),
            display_name: "Test".to_string(),
            author: None,
            briefing: BriefingSection::default(),
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

    fn cooperative_runtime(seed: u32) -> OfflineSkirmishRuntime {
        let cooperative_registry = cooperative_registry();
        let mut scenario_rng = SimRng::new(u64::from(seed));
        let cooperative_progress =
            CooperativeProgressState::construct(&cooperative_registry, &mut scenario_rng)
                .expect("preconstructed Cooperative records");
        OfflineSkirmishRuntime {
            snapshot: SkirmishPersistedSnapshot::from_global_defaults(defaults()),
            local_preferences: LocalMultiplayerPreferences::default(),
            scenario_rng,
            random_map_options: crate::map::rmg::RmgOptions::default(),
            ini_path: None,
            cooperative_registry,
            cooperative_progress,
            cooperative_country_roster: stock_cooperative_country_roster(),
            gameplay_rng_return_pending: false,
        }
    }

    fn runtime(snapshot: SkirmishPersistedSnapshot, seed: u32) -> OfflineSkirmishRuntime {
        OfflineSkirmishRuntime {
            snapshot,
            local_preferences: LocalMultiplayerPreferences::default(),
            scenario_rng: SimRng::new(u64::from(seed)),
            random_map_options: crate::map::rmg::RmgOptions::default(),
            ini_path: None,
            cooperative_registry: CooperativeRegistry::default(),
            cooperative_progress: CooperativeProgressState::default(),
            cooperative_country_roster: stock_cooperative_country_roster(),
            gameplay_rng_return_pending: false,
        }
    }

    #[test]
    fn launcher_network_refresh_replaces_only_six_cached_multiplayer_fields() {
        let unique = format!(
            "vera20k-launcher-network-refresh-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        );
        let directory = std::env::temp_dir().join(unique);
        std::fs::create_dir_all(&directory).expect("create refresh fixture directory");
        let path = directory.join(RA2MD_INI);
        let bytes = b"; preserve me\r\n\
[MultiPlayer]\r\n\
Handle=52,65,66,72,65,73,68,65,64,\r\n\
Color=4\r\n\
ColorEx=-2\r\n\
Side=Arabs\r\n\
SideEx=7\r\n\
GameMode=9\r\n\
[Skirmish]\r\n\
Credits=12345\r\n";
        std::fs::write(&path, bytes).expect("write refresh fixture");

        let mut runtime = runtime(
            SkirmishPersistedSnapshot {
                credits: 22222,
                ..SkirmishPersistedSnapshot::from_global_defaults(defaults())
            },
            0x1234_5678,
        );
        runtime.ini_path = Some(path.clone());
        runtime.random_map_options.seed = 4242;
        runtime.gameplay_rng_return_pending = true;
        runtime.local_preferences = LocalMultiplayerPreferences {
            handle_bytes: b"Old".to_vec(),
            country: SkirmishCountry::America,
            side_ex: 1,
            color_index: 0,
            color_ex: 0,
            game_mode: 1,
        };

        let snapshot_before = runtime.snapshot.clone();
        let rng_before = runtime.scenario_rng_state();
        let rmg_before = runtime.random_map_options.clone();
        let registry_before = runtime.cooperative_registry.clone();
        let progress_before = runtime.cooperative_progress.clone();
        let roster_before = runtime.cooperative_country_roster.clone();
        let pending_before = runtime.gameplay_rng_return_pending;

        runtime.refresh_local_multiplayer_preferences();

        assert_eq!(runtime.local_preferences.handle_bytes, b"Refreshed");
        assert_eq!(runtime.local_preferences.country, SkirmishCountry::Iraq);
        assert_eq!(runtime.local_preferences.side_ex, 7);
        assert_eq!(runtime.local_preferences.color_index, 4);
        assert_eq!(runtime.local_preferences.color_ex, -2);
        assert_eq!(runtime.local_preferences.game_mode, 9);
        assert_eq!(runtime.snapshot, snapshot_before);
        assert_eq!(runtime.scenario_rng_state(), rng_before);
        assert_eq!(runtime.random_map_options, rmg_before);
        assert_eq!(runtime.cooperative_registry, registry_before);
        assert_eq!(runtime.cooperative_progress, progress_before);
        assert_eq!(runtime.cooperative_country_roster, roster_before);
        assert_eq!(runtime.gameplay_rng_return_pending, pending_before);
        assert_eq!(
            std::fs::read(&path).expect("reread refresh fixture"),
            bytes,
            "Network preparation is read-only"
        );

        std::fs::remove_dir_all(directory).expect("remove refresh fixture directory");
    }

    fn defaults() -> SkirmishGlobalDefaults {
        skirmish_global_defaults(&SkirmishShellState::default())
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
        let mut runtime = OfflineSkirmishRuntime::initialize(
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
                ini.section("Skirmish").unwrap().get("Credits"),
                Some(credits.to_string().as_str())
            );
            assert_eq!(
                ini.section("Options").unwrap().get("ScrollRate"),
                Some(scroll_rate.to_string().as_str())
            );
            assert_eq!(ini.section("Unrelated").unwrap().get("Key"), Some("kept"));
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
            player_name: "Player".to_string(),
            local: SkirmishLocalSlot {
                country: LaunchCountry::America,
                country_random: true,
                color_index: 0,
                color_random: true,
                start_position: LaunchStartPosition::Auto,
                team: LaunchTeam::None,
            },
            opponents: vec![SkirmishAiSlot {
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
            let mut snapshot = SkirmishPersistedSnapshot::from_global_defaults(defaults());
            snapshot.credits = credits;
            snapshot.unit_count = units;
            snapshot.game_speed = speed;
            let mut runtime = runtime(snapshot, 7);
            let mut shell = SkirmishShellState::default();
            shell.trackbar_bounds = bounds;
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
        let mut snapshot = SkirmishPersistedSnapshot::from_global_defaults(defaults());
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
                handle_bytes: b"[New Player]".to_vec(),
                country: SkirmishCountry::America,
                side_ex: CONCRETE_ITEM_DATA,
                color_index: 2,
                color_ex: CONCRETE_ITEM_DATA,
                game_mode: DEFAULT_MULTIPLAYER_GAME_MODE,
            }
        );

        let mut runtime = runtime(
            SkirmishPersistedSnapshot::from_global_defaults(defaults()),
            7,
        );
        runtime.local_preferences = preferences;
        let mut shell = SkirmishShellState::default();
        runtime.hydrate_shell(&mut shell, &[map()], &[mode()]);

        assert_eq!(shell.player_name_edit.text, "[New Player]");
        assert_eq!(shell.player_country, SkirmishCountry::America);
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
            SkirmishPersistedSnapshot::from_global_defaults(defaults()),
            7,
        );
        runtime.local_preferences = preferences;
        let mut shell = SkirmishShellState::default();
        runtime.hydrate_shell(&mut shell, &[map()], &[mode()]);

        assert_eq!(shell.player_country, SkirmishCountry::Yuri);
        assert!(shell.player_country_random);
        assert_eq!(shell.player_color_index, 7);
        assert!(!shell.player_color_claimed);

        let concrete = read_local_multiplayer_preferences(
            b"[MultiPlayer]\nColor=7\nColorEx=-1\nSide=YuriCountry\nSideEx=-3\n",
        );
        runtime.local_preferences = concrete;
        runtime.hydrate_shell(&mut shell, &[map()], &[mode()]);
        assert_eq!(shell.player_country, SkirmishCountry::Yuri);
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
            SkirmishPersistedSnapshot::from_global_defaults(defaults()),
            0x1234,
        );
        let mut shell = SkirmishShellState::default();
        shell.player_name_edit = PlayerNameEditState::with_name("12345678901234567890");
        shell.player_country_random = true;
        shell.player_color_claimed = false;
        let session = random_session();

        let resolved = active_runtime
            .close_shell_transaction(&shell, &[map()], &[mode()], &session)
            .expect("random close");
        assert_eq!(
            active_runtime.local_preferences.handle_bytes.as_slice(),
            b"1234567890123456789"
        );
        assert_eq!(
            active_runtime.local_preferences.country,
            menu_country_from_launch(resolved.local.country)
        );
        assert_eq!(active_runtime.local_preferences.side_ex, RANDOM_ITEM_DATA);
        assert_eq!(
            active_runtime.local_preferences.color_index,
            usize::from(resolved.local.color_index)
        );
        assert_eq!(active_runtime.local_preferences.color_ex, RANDOM_ITEM_DATA);

        let bytes = active_runtime.update_persistence_bytes(b"[Other]\nKeep=1\n");
        let reloaded = read_local_multiplayer_preferences(&bytes);
        let mut reopened = runtime(
            SkirmishPersistedSnapshot::from_global_defaults(defaults()),
            9,
        );
        reopened.local_preferences = reloaded;
        let mut reopened_shell = SkirmishShellState::default();
        reopened.hydrate_shell(&mut reopened_shell, &[map()], &[mode()]);
        assert!(reopened_shell.player_country_random);
        assert!(!reopened_shell.player_color_claimed);
        assert_eq!(
            reopened_shell.player_country,
            menu_country_from_launch(resolved.local.country)
        );
        assert_eq!(
            reopened_shell.player_color_index,
            usize::from(resolved.local.color_index)
        );
    }

    #[test]
    fn gsi_03_10_multiplayer_preferences_concrete_close_round_trips_minus_one() {
        let mut active_runtime = runtime(
            SkirmishPersistedSnapshot::from_global_defaults(defaults()),
            0x5678,
        );
        let mut shell = SkirmishShellState::default();
        shell.player_country = SkirmishCountry::Yuri;
        shell.player_country_random = false;
        shell.player_color_index = 7;
        shell.player_color_claimed = true;
        let mut session = random_session();
        session.local.country = LaunchCountry::Yuri;
        session.local.country_random = false;
        session.local.color_index = 7;
        session.local.color_random = false;

        let resolved = active_runtime
            .close_shell_transaction(&shell, &[map()], &[mode()], &session)
            .expect("concrete close");
        assert_eq!(resolved.local.country, LaunchCountry::Yuri);
        assert_eq!(resolved.local.color_index, 7);
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
            SkirmishPersistedSnapshot::from_global_defaults(defaults()),
            9,
        );
        reopened.local_preferences = read_local_multiplayer_preferences(&bytes);
        let mut reopened_shell = SkirmishShellState::default();
        reopened.hydrate_shell(&mut reopened_shell, &[map()], &[mode()]);
        assert_eq!(reopened_shell.player_country, SkirmishCountry::Yuri);
        assert!(!reopened_shell.player_country_random);
        assert_eq!(reopened_shell.player_color_index, 7);
        assert!(reopened_shell.player_color_claimed);
    }

    #[test]
    fn gsi_03_10_multiplayer_preferences_codec_is_ordered_and_preserving() {
        let mut runtime = runtime(
            SkirmishPersistedSnapshot::from_global_defaults(defaults()),
            7,
        );
        runtime.local_preferences = LocalMultiplayerPreferences {
            handle_bytes: vec![0x4a, 0x6f, 0x73, 0xe9],
            country: SkirmishCountry::Yuri,
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
        assert_eq!(preferences.country, SkirmishCountry::America);
        assert_eq!(preferences.side_ex, 0);
        assert_eq!(preferences.color_index, 7);
        assert_eq!(preferences.color_ex, 0);
        assert_eq!(preferences.game_mode, DEFAULT_MULTIPLAYER_GAME_MODE);

        let mut snapshot = SkirmishPersistedSnapshot::from_global_defaults(defaults());
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
            SkirmishPersistedSnapshot::from_global_defaults(defaults()),
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
            SkirmishPersistedSnapshot::from_global_defaults(defaults()),
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
            handle_bytes: b"Cached".to_vec(),
            country: SkirmishCountry::Russia,
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
        let preferences = read_local_multiplayer_preferences(
            b"[MultiPlayer]\nHandle=not-hex,\nColor=99\nSide=UnknownCountry\n",
        );
        assert_eq!(preferences, LocalMultiplayerPreferences::default());
        assert_eq!(decode_multiplayer_handle(""), None);
        assert_eq!(
            decode_multiplayer_handle("41,00,42,"),
            Some("A".to_string())
        );
        assert_eq!(decode_multiplayer_handle("e9,"), Some("\u{e9}".to_string()));
        #[cfg(windows)]
        assert_eq!(
            decode_multiplayer_handle("80,"),
            Some("\u{20ac}".to_string())
        );
        assert_eq!(
            decode_multiplayer_handle("20ac,"),
            Some("\u{ac}".to_string())
        );
        assert_eq!(decode_multiplayer_handle("100,41,"), None);
    }

    #[test]
    fn resolving_launch_copy_does_not_mutate_raw_random_fields() {
        let snapshot = SkirmishPersistedSnapshot::from_global_defaults(defaults());
        let mut runtime = runtime(snapshot, 11);
        let session = random_session();

        let resolved = runtime
            .resolve_launch_session(&session, &[])
            .expect("ordinary resolver");

        assert!(session.local.country_random);
        assert!(session.local.color_random);
        assert!(session.opponents[0].country_random);
        assert!(session.opponents[0].color_random);
        assert!(!resolved.local.country_random);
        assert!(!resolved.local.color_random);
        assert!(!resolved.opponents[0].country_random);
        assert!(!resolved.opponents[0].color_random);
    }

    #[test]
    fn start_and_back_use_identical_shell_resolution_transcripts() {
        let snapshot = SkirmishPersistedSnapshot::from_global_defaults(defaults());
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
        let snapshot = SkirmishPersistedSnapshot::from_global_defaults(defaults());
        let runtime = runtime(snapshot.clone(), 0x5678);
        let rng_before = runtime.scenario_rng_state();
        let mut shell = SkirmishShellState::default();
        shell.selected_mode_id = mode().id;
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
        session.local.country_random = false;
        session.local.color_random = false;
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
    fn captured_match_rng_replaces_frontend_cursor_only_when_marked() {
        let snapshot = SkirmishPersistedSnapshot::from_global_defaults(defaults());
        let mut runtime = runtime(snapshot, 1);
        let replacement = SimRng::new(99);
        let expected = replacement.state();

        assert!(!runtime.capture_returned_gameplay_rng(replacement.clone()));
        assert_ne!(runtime.scenario_rng_state(), expected);
        runtime.mark_gameplay_rng_return_pending();
        assert!(runtime.capture_returned_gameplay_rng(replacement));
        assert_eq!(runtime.scenario_rng_state(), expected);
    }

    #[test]
    fn gsi_04_12_first_random_map_entry_spends_one_scenario_draw_and_reentry_spends_none() {
        let snapshot = SkirmishPersistedSnapshot::from_global_defaults(defaults());
        let seed = 0x4321_u32;
        let mut runtime = runtime(snapshot, seed);
        let mut reference = SimRng::new(u64::from(seed));
        let expected_seed = reference.next_range_u32_inclusive(0, 0xFFFF) as i32;

        let mut first = runtime.random_map_options_for_setup();
        assert_eq!(first.seed, expected_seed);
        assert_eq!(runtime.scenario_rng.logical_state(), reference.logical_state());

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

        let snapshot = SkirmishPersistedSnapshot::from_global_defaults(defaults());
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

        assert_eq!(runtime.scenario_rng.logical_state(), reference.logical_state());
    }

    #[test]
    fn invalid_persisted_indices_clamp_to_first_mode_and_map() {
        let mut snapshot = SkirmishPersistedSnapshot::from_global_defaults(defaults());
        snapshot.game_mode = 999;
        snapshot.scenario_index = 999;
        let runtime = runtime(snapshot, 3);
        let mut shell = SkirmishShellState::default();
        shell.player_start_position = StartPosition::Auto;

        runtime.hydrate_shell(&mut shell, &[map()], &[mode()]);

        assert_eq!(shell.selected_mode_id, 1);
        assert_eq!(shell.selected_map_idx, 0);
    }
}
