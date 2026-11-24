//! Player-name edit state and input helpers for the skirmish shell.

use crate::map::scenario_menu::MapMenuEntry;
use crate::sim::game_options::GameOptions;
use crate::skirmish_launch::{
    LaunchCountry, LaunchStartPosition, SKIRMISH_MAX_SLOT_COUNT, SKIRMISH_PLAYER_SLOT_COUNT,
    SkirmishLaunchOptions,
};
use crate::skirmish_modes::{SkirmishGameMode, mode_by_id};

use super::super::SkirmishStatics;
use super::super::layout::shown_player_rows;
use super::trackbars::SkirmishTrackbarBounds;
use super::{
    ChooseMapModalState, RandomMapSetupModalState, SkirmishModeSelection, SkirmishShellOpponent,
    default_opponents,
};

pub const PLAYER_NAME_DEFAULT: &str = "Player";
/// The name box `0x6A0`'s `EM_LIMITTEXT`: bytes in the ANSI code page.
pub const PLAYER_NAME_EDIT_LIMIT_BYTES: usize = 19;

/// The name the ANSI edit shows for the profile's: `WM_SETTEXT` is
/// programmatic, so the later `EM_LIMITTEXT` does not cut it.
pub fn player_name_from_profile(name: &str) -> String {
    crate::util::native_string::acp_round_trip(name)
        .chars()
        .filter(|ch| !matches!(*ch, '\r' | '\n'))
        .collect()
}

/// One visible country row's Ares presentation.
#[derive(Debug, Clone)]
pub(crate) struct CountryMenuEntry {
    country: Option<LaunchCountry>,
    /// `Game_GetFlagSurface @ 0x1001DCE0` answer for this row; `None` keeps the
    /// stock static country table.
    flag: Option<String>,
    status_key: Option<String>,
}

/// Derived country-menu projection, refreshed from the canonical Rules
/// registry at shell entry. It never owns a participant's selected Country.
#[derive(Debug, Clone)]
pub(crate) struct CountryMenuCatalog {
    items: Vec<super::SkirmishComboItem>,
    entries: Vec<CountryMenuEntry>,
}
impl Default for CountryMenuCatalog {
    fn default() -> Self {
        Self {
            items: vec![super::SkirmishComboItem::Country(
                super::SkirmishCountryChoice::Random,
            )],
            entries: vec![CountryMenuEntry {
                country: None,
                flag: None,
                status_key: None,
            }],
        }
    }
}
impl CountryMenuCatalog {
    pub(super) fn from_rules(rules: &crate::rules::ruleset::RuleSet) -> Self {
        use crate::rules::ruleset::CountryIdx;
        let items = super::combos::country_choices(rules);
        let entries = items
            .iter()
            .map(|item| match item {
                super::SkirmishComboItem::Country(super::SkirmishCountryChoice::Random) => {
                    CountryMenuEntry {
                        country: None,
                        flag: Some("rani.pcx".to_owned()),
                        status_key: Some("STT:PlayerSideRandom".to_owned()),
                    }
                }
                super::SkirmishComboItem::Country(super::SkirmishCountryChoice::Country(
                    country,
                )) => {
                    let id = rules.country_name(CountryIdx(country.raw_index()));
                    CountryMenuEntry {
                        country: Some(*country),
                        flag: id.and_then(|id| rules.country_flag_surface(id)),
                        status_key: id
                            .and_then(|id| rules.country_status_key(id))
                            .map(str::to_owned),
                    }
                }
                _ => CountryMenuEntry {
                    country: None,
                    flag: None,
                    status_key: None,
                },
            })
            .collect();
        Self { items, entries }
    }

    pub(crate) fn items(&self) -> &[super::SkirmishComboItem] {
        &self.items
    }

    /// Flag file this row draws. `None` keeps the stock static fallback.
    pub(crate) fn flag_for_choice(&self, random: bool, country: LaunchCountry) -> Option<&str> {
        if random {
            return Some("rani.pcx");
        }
        self.entries
            .iter()
            .find(|entry| entry.country == Some(country))?
            .flag
            .as_deref()
    }

    /// Status-help CSF key for a row; `None` keeps the stock static fallback.
    pub(crate) fn status_key_for_choice(
        &self,
        choice: super::SkirmishCountryChoice,
    ) -> Option<&str> {
        match choice {
            super::SkirmishCountryChoice::Random => Some("STT:PlayerSideRandom"),
            super::SkirmishCountryChoice::Country(country) => self
                .entries
                .iter()
                .find(|entry| entry.country == Some(country))?
                .status_key
                .as_deref(),
        }
    }
}

#[derive(Debug)]
pub struct SkirmishShellState {
    pub(crate) country_catalog: CountryMenuCatalog,
    /// The colours the loaded rules list for a lobby: every colour combo,
    /// claim and random draw of the shell reads this copy.
    pub(crate) lobby_colors: crate::rules::skirmish_colors::SkirmishColorMode,
    pub selected_map_idx: usize,
    pub selected_mode: SkirmishModeSelection,
    /// The name box `0x6A0`'s text; the page's edit types into it.
    pub player_name: String,
    pub player_country: LaunchCountry,
    pub player_country_random: bool,
    pub player_color_index: usize,
    pub player_color_claimed: bool,
    pub player_start_position: LaunchStartPosition,
    pub player_team: i32,
    pub starting_credits: i32,
    pub game_speed: i32,
    pub unit_count: i32,
    /// Credits/Unit Count slider ranges, seeded from `[MultiplayerDialogSettings]`
    /// at lobby construction (stock-default constants until then).
    pub trackbar_bounds: SkirmishTrackbarBounds,
    /// Per-match option base, seeded from `[MultiplayerDialogSettings]` at lobby
    /// construction (stock defaults until then). The launch path overrides the
    /// fields exposed as widgets from the live state above; this base carries
    /// the non-widget toggles (tech level, bases, shroud, …) into the match.
    pub launch_options_base: SkirmishLaunchOptions,
    pub short_game: bool,
    pub super_weapons: bool,
    pub build_off_ally: bool,
    pub crates: bool,
    pub mcv_redeploy: bool,
    /// The session options VERA's Game Options box shows, which override
    /// [`Self::launch_options_base`]'s at Start: the rules' Tech Level and
    /// Bases, and Fog of War, Multi-Engineer and Harvester Truce off, as
    /// SkirmishDialog's Start forces them ([`skirmish_session_options`]).
    pub session: crate::skirmish_launch::SessionOptions,
    /// Player rows 1.. : the retail seven, then VERA's rows up to thirty
    /// players for maps with more starts.
    pub opponents: Vec<SkirmishShellOpponent>,
    /// VERA: the index in [`Self::opponents`] of the first computer row the
    /// page shows. A map with more than eight starts scrolls its computer
    /// rows under the player's own row; 0 on a retail map.
    pub(crate) first_opponent: usize,
    pub selected_mode_allies_allowed: bool,
    pub selected_mode_must_ally: bool,
    pub choose_map_modal: Option<ChooseMapModalState>,
    /// The Create Random Map dialog, opened over the choose-map modal.
    pub random_map_setup_modal: Option<RandomMapSetupModalState>,
    /// The Load / Save / Delete browser the setup dialog opens over itself.
    /// Sits beside the setup modal rather than inside it because it replaces
    /// the setup dialog on screen while that dialog keeps its working options.
    pub saved_seed_browser: Option<super::SavedSeedBrowserState>,
    /// VERA's Setups page over the Skirmish page, its rows named by setup.
    pub setups_browser: Option<super::SavedSeedBrowserState<String>>,
    /// The item last picked from Quick Team Setup or Bulk Seat Actions,
    /// which the app carries out ([`super::MenuPick`]).
    pub menu_pick: Option<super::MenuPick>,
    pub validation_modal: Option<crate::ui::shell::modal::ShellBox>,
    /// VERA's Game Options box, open over the page: it takes every press
    /// and key until OK, Enter or Esc closes it.
    pub game_options: Option<crate::ui::skirmish_shell::game_options::GameOptionsBox>,
    /// The kind-1 statics; the status line holds the hover help.
    pub(crate) statics: SkirmishStatics,
}

/// A family dialog of the Skirmish stack. Choose Map `0x6B` hides `0x102`
/// and the random-map dialog `0x105` hides `0x6B`; the seed browser (`0xB7`,
/// `0x2B4`, `0x2B5`) opens over `0x105` without hiding it (`0x00558DD0`
/// creates and shows its own dialog, `0x00622650`, `0x00622800`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SkirmishShellDialog {
    Skirmish,
    ChooseMap,
    RandomMap,
    SeedBrowser,
    /// VERA's Setups page, which covers `0x102` while it is open.
    Setups,
}

impl SkirmishShellState {
    pub fn refresh_country_catalog(&mut self, rules: &crate::rules::ruleset::RuleSet) {
        self.country_catalog = CountryMenuCatalog::from_rules(rules);
        self.lobby_colors = rules.skirmish_colors();
    }

    /// VERA's shared profile can remember a country from another mod. Keep
    /// each restored row on a choice the current rules actually offer.
    pub(crate) fn repair_country_selections(&mut self) {
        let available = |country| {
            self.country_catalog
                .items()
                .contains(&super::SkirmishComboItem::Country(
                    super::SkirmishCountryChoice::Country(country),
                ))
        };
        self.player_country_random |= !available(self.player_country);
        for opponent in &mut self.opponents {
            opponent.country_random |= !available(opponent.country);
        }
    }

    /// The colours this shell's lobbies list.
    pub fn lobby_colors(&self) -> crate::rules::skirmish_colors::SkirmishColorMode {
        self.lobby_colors
    }

    /// The dialog that shows on top of the Skirmish stack: the one decision
    /// for the slide target, the renderer and input. `None` while the
    /// chooser stays hidden after the random-map run, behind its Use Map's
    /// eject box.
    pub fn top_dialog(&self) -> Option<SkirmishShellDialog> {
        let Some(chooser) = self.choose_map_modal.as_ref() else {
            return Some(if self.setups_browser.is_some() {
                SkirmishShellDialog::Setups
            } else {
                SkirmishShellDialog::Skirmish
            });
        };
        if self.saved_seed_browser.is_some() {
            Some(SkirmishShellDialog::SeedBrowser)
        } else if self.random_map_setup_modal.is_some() {
            Some(SkirmishShellDialog::RandomMap)
        } else if chooser.is_hidden() {
            None
        } else {
            Some(SkirmishShellDialog::ChooseMap)
        }
    }
}

/// The session options a Skirmish game gets with nothing turned on: the
/// rules' Tech Level and Bases, and the three SkirmishDialog's Start/Back
/// packing (`0x006AD88F..0x006AD8A4`) forces off whatever the rules default:
/// Fog of War (`0x00A8B31F`), Harvester Truce (`0x00A8B31D`) and
/// MultiEngineer (`0x00A8B26C`).
pub fn skirmish_session_options(
    base: &SkirmishLaunchOptions,
) -> crate::skirmish_launch::SessionOptions {
    crate::skirmish_launch::SessionOptions {
        fog_of_war: false,
        multi_engineer: false,
        harvester_truce: false,
        ..base.session_options()
    }
}

impl Default for SkirmishShellState {
    fn default() -> Self {
        let options = SkirmishLaunchOptions::default();
        Self {
            country_catalog: CountryMenuCatalog::default(),
            lobby_colors: Default::default(),
            selected_map_idx: 0,
            selected_mode: 1.into(),
            player_name: PLAYER_NAME_DEFAULT.to_owned(),
            player_country: LaunchCountry::default(),
            player_country_random: false,
            player_color_index: 0,
            player_color_claimed: true,
            // Native population selects Random when the local row owns no
            // numbered start reservation.
            player_start_position: LaunchStartPosition::Auto,
            player_team: -2,
            starting_credits: options.starting_credits,
            game_speed: options.game_speed,
            unit_count: options.unit_count,
            trackbar_bounds: SkirmishTrackbarBounds::default(),
            launch_options_base: SkirmishLaunchOptions::default(),
            short_game: options.short_game,
            super_weapons: options.super_weapons,
            build_off_ally: options.build_off_ally,
            crates: options.crates,
            mcv_redeploy: options.mcv_redeploy,
            session: skirmish_session_options(&options),
            opponents: default_opponents(LaunchCountry::Russia),
            first_opponent: 0,
            selected_mode_allies_allowed: true,
            selected_mode_must_ally: false,
            choose_map_modal: None,
            random_map_setup_modal: None,
            saved_seed_browser: None,
            setups_browser: None,
            menu_pick: None,
            validation_modal: None,
            game_options: None,
            statics: SkirmishStatics::default(),
        }
    }
}

impl SkirmishShellState {
    /// Seed the lobby from per-match options parsed from
    /// `[MultiplayerDialogSettings]`. The values the setup dialog exposes as
    /// widgets are copied into the live fields so each control opens on the
    /// configured value; the full set is also retained as the launch base so
    /// the non-widget toggles reach the match unchanged. `GameSpeed` is stored
    /// as parsed — the trackbar inverts it only for display.
    pub fn apply_multiplayer_dialog_values(&mut self, options: &GameOptions) {
        self.starting_credits = options.starting_credits;
        self.unit_count = options.unit_count;
        self.game_speed = options.game_speed;
        self.short_game = options.short_game;
        self.super_weapons = options.super_weapons;
        self.build_off_ally = options.build_off_ally;
        self.crates = options.crates;
        self.mcv_redeploy = options.mcv_redeploy;
        self.launch_options_base = SkirmishLaunchOptions::from_game_options(options);
        self.session = skirmish_session_options(&self.launch_options_base);
    }

    /// The row slot (`0..shown`, [`SkirmishShellLayout::shown_rows`])
    /// showing player row `row`: the player's own row always takes slot 0, a
    /// computer row only while scrolled into view.
    pub fn row_slot(&self, row: usize, shown: usize) -> Option<usize> {
        if row == 0 {
            return Some(0);
        }
        row.checked_sub(self.first_opponent)
            .filter(|slot| (1..shown).contains(slot))
    }

    /// The player row that retail row slot `slot` shows.
    pub fn slot_row(&self, slot: usize) -> usize {
        if slot == 0 {
            0
        } else {
            self.first_opponent + slot
        }
    }

    /// The computer rows in view of `shown` row slots, as (row slot, player
    /// row, its opponent); the map's capacity still hides rows
    /// ([`player_row_visible`]).
    pub fn paged_opponents(
        &self,
        shown: usize,
    ) -> impl Iterator<Item = (usize, usize, &SkirmishShellOpponent)> {
        (1..shown).filter_map(move |slot| {
            let row = self.slot_row(slot);
            self.opponents
                .get(row - 1)
                .map(|opponent| (slot, row, opponent))
        })
    }
}

pub(super) fn inactive_ai_team_default(state: &SkirmishShellState) -> i32 {
    if state.selected_mode_allies_allowed {
        3
    } else {
        -2
    }
}

/// Whether a complete player-row control group is visible for the selected map.
/// Slot zero is the local row and never hides; opponent rows are bounded by the
/// selected map's native player-capacity query, and VERA's past the eighth
/// show while scrolled into view.
pub fn player_row_visible(state: &SkirmishShellState, maps: &[MapMenuEntry], row: usize) -> bool {
    if row == 0 {
        return true;
    }
    let rows = selected_player_rows(state, maps);
    row < rows && state.row_slot(row, shown_player_rows(rows)).is_some()
}

/// The player rows the selected map admits: the retail eight with no map.
pub fn selected_player_rows(state: &SkirmishShellState, maps: &[MapMenuEntry]) -> usize {
    maps.get(state.selected_map_idx)
        .map_or(SKIRMISH_PLAYER_SLOT_COUNT, |map| {
            admitted_player_rows(map.player_capacity)
        })
}

/// A map's player capacity as rows: offline Skirmish seats up to
/// [`SKIRMISH_MAX_SLOT_COUNT`] players; a retail map declares eight or fewer.
fn admitted_player_rows(capacity: i32) -> usize {
    capacity.clamp(0, SKIRMISH_MAX_SLOT_COUNT as i32) as usize
}

/// The last [`SkirmishShellState::first_opponent`] for `rows` admitted player
/// rows, which shows the map's last computer rows; 0 while every row shows
/// ([`shown_player_rows`]).
pub(super) fn last_first_opponent(rows: usize) -> usize {
    rows.saturating_sub(shown_player_rows(rows))
}

fn reset_rows_hidden_by_capacity(state: &mut SkirmishShellState, capacity: i32) {
    let capacity = admitted_player_rows(capacity);
    state.first_opponent = state.first_opponent.min(last_first_opponent(capacity));
    let visible_opponents = capacity.saturating_sub(1);
    let team_default = inactive_ai_team_default(state);
    for opponent in state.opponents.iter_mut().skip(visible_opponents) {
        opponent.row_type = super::SkirmishAiRowType::None;
        opponent.apply_inactive_combo_defaults(team_default);
    }
}

/// Apply the accepted map's capacity to the opponent rows. SkirmishDialog
/// (`0x006ADDF0`) refreshes the rows even when both maps are `RandMap.Sed`.
pub fn accept_selected_map(
    state: &mut SkirmishShellState,
    maps: &[MapMenuEntry],
    map_idx: usize,
) -> bool {
    let Some(new_map) = maps.get(map_idx) else {
        return false;
    };
    // Generating another random map replaces the selected entry in place, so
    // reading its old index here would already give the new capacity.
    reset_rows_hidden_by_capacity(state, new_map.player_capacity);
    state.selected_map_idx = map_idx;
    true
}

/// Apply the native first-open row hide pass for the initially selected map
/// and scroll back to the first rows. Native runs the pass only below eight
/// players; at eight or more it would reset none of the retail rows, so
/// running it always changes only VERA's rows past the map's capacity.
pub fn initialize_rows_for_selected_map(state: &mut SkirmishShellState, maps: &[MapMenuEntry]) {
    state.first_opponent = 0;
    let capacity = maps
        .get(state.selected_map_idx)
        .map(|map| map.player_capacity)
        .unwrap_or(SKIRMISH_PLAYER_SLOT_COUNT as i32);
    reset_rows_hidden_by_capacity(state, capacity);
}

#[cfg(all(test, tests_ui))]
mod multiplayer_dialog_value_tests {
    use super::*;

    #[test]
    fn default_shell_launch_base_matches_hardcoded_defaults() {
        // The safety-net default path must stay on the hardcoded fallback so a
        // default()-constructed shell is byte-identical without an INI.
        let shell = SkirmishShellState::default();
        assert_eq!(shell.launch_options_base, SkirmishLaunchOptions::default());
    }

    #[test]
    fn apply_multiplayer_dialog_values_seeds_widgets_and_launch_base() {
        let mut shell = SkirmishShellState::default();
        let options = GameOptions {
            starting_credits: 7400,
            unit_count: 4,
            game_speed: 4,
            short_game: false,
            super_weapons: false,
            build_off_ally: false,
            crates: false,
            mcv_redeploy: false,
            tech_level: 3,
            bases: false,
            shroud: false,
            ..GameOptions::default()
        };

        shell.apply_multiplayer_dialog_values(&options);

        // Widget-backed fields are seeded into the live state.
        assert_eq!(shell.starting_credits, 7400);
        assert_eq!(shell.unit_count, 4);
        // GameSpeed is stored as parsed; the trackbar handles display inversion.
        assert_eq!(shell.game_speed, 4);
        assert!(!shell.short_game);
        assert!(!shell.super_weapons);
        assert!(!shell.build_off_ally);
        assert!(!shell.crates);
        assert!(!shell.mcv_redeploy);
        // Non-widget toggles ride on the launch base into the match.
        assert_eq!(shell.launch_options_base.tech_level, 3);
        assert!(!shell.launch_options_base.bases);
        assert!(!shell.launch_options_base.shroud);
    }
}

pub fn repair_teams_for_selected_mode(state: &mut SkirmishShellState, modes: &[SkirmishGameMode]) {
    let mode = state
        .selected_mode
        .resolve(modes)
        .or_else(|| mode_by_id(modes, 1))
        .or_else(|| modes.first());
    if let Some(mode) = mode {
        state.selected_mode_allies_allowed = mode.allies_allowed;
        state.selected_mode_must_ally = mode.must_ally;
    }

    let inactive_default = inactive_ai_team_default(state);
    // Rebuilding the local Team combo selects its first row: Team A when
    // MustAlly suppresses None, otherwise None. Accepted map/type refreshes are
    // constructor-like and do not preserve a prior explicit local selection.
    state.player_team = if state.selected_mode_must_ally { 0 } else { -2 };
    for opponent in &mut state.opponents {
        // Native selected-mode refresh writes the mode default to every AI Team
        // control. This is a refresh event, not a per-frame repair.
        opponent.team = inactive_default;
    }
}
