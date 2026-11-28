//! Choose Map `0x6B` state (proc `0x005E6920`): the game-type list `0x6EB`,
//! the map list `0x553` and the three buttons.
//!
//! Both lists are the shell list subclass `0x00618D40` as `vera20k-gui`
//! lists ([`super::super::choose_map_page`]): a press on a row selects it and
//! plays GenericClick (`0x0061AA5A`), a press below the last row is ignored,
//! the scrollbar captures and repeats, and no key or wheel case exists
//! (`0x00618D40..0x0061C600`). This state keeps what the lists show.

use crate::map::rmg::saved_seeds::SavedSeed;
use crate::map::skirmish_scenarios::{
    SkirmishScenarioRecord, SkirmishScenarioSource, filter_records_for_mode, is_valid_map,
    upsert_random_map_sentinel,
};
use crate::skirmish_modes::{SkirmishGameMode, mode_by_id};
use crate::ui::shell::modal::ShellBox;
use crate::ui::shell::static_reveal::DialogStatics;
use vera20k_gui::shell::list::{ListMetrics, ShellListGeometry};

use super::super::layout::{ChooseMapModalButton, ChooseMapModalLayout, RectPx};
use super::super::online_map_folders::OnlineMapFolders;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SkirmishModeSelection {
    pub id: i32,
    /// Only needed for a non-first entry with this numeric ID. The catalog
    /// stays fixed for the lifetime of the shell.
    catalog_index: Option<usize>,
}

impl From<i32> for SkirmishModeSelection {
    fn from(id: i32) -> Self {
        Self {
            id,
            catalog_index: None,
        }
    }
}

impl SkirmishModeSelection {
    pub(crate) fn for_skirmish_row(index: usize, modes: &[SkirmishGameMode]) -> Option<Self> {
        let mode = modes.get(index)?;
        // The catalog is sorted by ID. Only a duplicate needs a row index.
        let use_index = index > 0 && modes[index - 1].id == mode.id;
        Some(Self {
            id: mode.id,
            catalog_index: use_index.then_some(index),
        })
    }

    pub fn resolve(self, modes: &[SkirmishGameMode]) -> Option<&SkirmishGameMode> {
        match self.catalog_index {
            Some(index) => modes.get(index).filter(|mode| mode.id == self.id),
            None => mode_by_id(modes, self.id),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ChooseMapSelection {
    pub mode: SkirmishModeSelection,
    pub record_index: Option<usize>,
}

/// The buttons of Use Map's two-button box: OK (or Download) and Cancel.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChooseMapPromptButton {
    Ok,
    Cancel,
}

/// What Use Map's box asks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChooseMapPromptKind {
    /// Occupied AI rows would not fit the chosen map
    /// (`0x005E7285..0x005E7336`): `GUI:EjectAIPlayers` with OK and Cancel.
    /// OK goes on with the selection; Cancel keeps the chooser.
    EjectAi,
    /// VERA: the chosen map is only in the online map library. Download
    /// saves it in VERA's maps folder.
    Download,
    /// Its download is running; only Cancel works.
    Downloading,
    /// Its download failed; Download tries again.
    DownloadFailed,
    /// It arrived but could not be saved in VERA's maps folder; Download
    /// tries again.
    SaveFailed,
    /// It is saved in VERA's maps folder. The box waits: OK goes on with the
    /// selection, and Cancel keeps the chooser with the map selected.
    Downloaded,
    /// VERA: the random-map dialog's seed could not be saved, so no random
    /// map is selected. Either button keeps the chooser.
    RandomMapFailed,
}

/// What answering Use Map's box does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChooseMapPromptAnswer {
    /// Go on with the selection.
    UseMap,
    /// Download the selected online map.
    Download,
    /// Close the box, stopping any download, and keep the chooser.
    Close,
    /// Nothing: Download waits while one runs.
    Wait,
}

/// Use Map's two-button box over the chooser. Its buttons are the shared
/// box's widgets ([`Self::shell_box`]), which keep the press.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ChooseMapPrompt {
    pub selection: ChooseMapSelection,
    pub kind: ChooseMapPromptKind,
}

impl ChooseMapPrompt {
    /// The box as it paints and takes the pointer: the two-button message
    /// box `0x120` with `body` over OK (`ok`'s caption) and Cancel, OK
    /// greyed while a download runs.
    pub fn shell_box(&self, body: String, ok: String, cancel: String) -> ShellBox {
        use crate::ui::shell::modal::control::{CANCEL, OK};
        ShellBox::new(crate::ui::shell::descriptor::DialogResourceId(0x120), body)
            .button(OK, ok)
            .button(CANCEL, cancel)
            .disabled(OK, !self.button_enabled(ChooseMapPromptButton::Ok))
    }

    /// The button Enter answers with: Cancel on the eject question
    /// (`0x005D370D`); VERA's boxes take it as OK, as the room's
    /// missing-map offer does (a running download's greyed OK takes none).
    pub fn default_button(&self) -> u16 {
        use crate::ui::shell::modal::control::{CANCEL, OK};
        match self.kind {
            ChooseMapPromptKind::EjectAi => CANCEL,
            _ => OK,
        }
    }

    /// Whether a button takes presses: Download waits while one runs.
    pub fn button_enabled(&self, button: ChooseMapPromptButton) -> bool {
        button == ChooseMapPromptButton::Cancel || self.kind != ChooseMapPromptKind::Downloading
    }

    /// What OK (`ok`) or Cancel does, from a click or a key.
    pub fn answer(&self, ok: bool) -> ChooseMapPromptAnswer {
        use ChooseMapPromptKind as Kind;
        match (self.kind, ok) {
            (_, false) | (Kind::RandomMapFailed, true) => ChooseMapPromptAnswer::Close,
            (Kind::EjectAi | Kind::Downloaded, true) => ChooseMapPromptAnswer::UseMap,
            (Kind::Downloading, true) => ChooseMapPromptAnswer::Wait,
            (Kind::Download | Kind::DownloadFailed | Kind::SaveFailed, true) => {
                ChooseMapPromptAnswer::Download
            }
        }
    }
}

/// Whether any AI row at or past the map's player limit is occupied
/// (`0x006ACCA0`: rows from slot `limit` on whose type is Easy, Normal or
/// Hard; a limit of 8 or more never ejects). VERA's rows past the seventh
/// count the same way, so only a limit past every row never ejects.
pub fn ai_rows_beyond_limit_occupied(
    opponents: &[super::SkirmishShellOpponent],
    limit: i32,
) -> bool {
    opponents
        .iter()
        .skip(limit.max(1) as usize - 1)
        .any(|opponent| opponent.row_type != super::SkirmishAiRowType::None)
}

/// A list's paint surface: one pixel wider and taller than its window, like
/// every family list.
/// VERA: the game-type list's Online Maps row, after the game types. It
/// lists the online map library's maps, which no game type lists.
pub const ONLINE_MAPS_ROW: i32 = i32::MIN;

/// VERA: the game-type list's Favorites row, after the game types and
/// before Online Maps. It lists the maps the player starred.
pub const FAVORITES_ROW: i32 = i32::MIN + 1;

/// The folder of seeds saved from Create Random Map.
pub const RANDOM_MAPS_ROW: i32 = i32::MIN + 2;

// The website's gallery category IDs. These chooser rows are local navigation;
// Use Map still returns the actual game type admitted by the selected map.
const FIRST_ONLINE_CATEGORY: i32 = RANDOM_MAPS_ROW + 1;
const ONLINE_CATEGORIES: [(&str, &str); 8] = [
    ("missions", "VERA:OnlineMapMissions"),
    ("survival", "VERA:OnlineMapSurvival"),
    ("duels", "VERA:OnlineMapDuels"),
    ("small", "VERA:OnlineMapSmall"),
    ("large", "VERA:OnlineMapLarge"),
    ("oil", "VERA:OnlineMapOil"),
    ("megawealth", "VERA:OnlineMapMegawealth"),
    ("navalwar", "VERA:OnlineMapNavalWar"),
];

impl SkirmishModeSelection {
    fn online_category(self) -> Option<(&'static str, &'static str)> {
        let index = self.id.checked_sub(FIRST_ONLINE_CATEGORY)?;
        ONLINE_CATEGORIES.get(usize::try_from(index).ok()?).copied()
    }

    pub(crate) fn online_category_label(self) -> Option<&'static str> {
        self.online_category().map(|(_, label)| label)
    }
}

/// Whether `mode_id` is a row VERA fills itself, not a game type.
fn is_own_row(mode_id: i32) -> bool {
    matches!(mode_id, ONLINE_MAPS_ROW | FAVORITES_ROW | RANDOM_MAPS_ROW)
        || SkirmishModeSelection::from(mode_id)
            .online_category()
            .is_some()
}

/// VERA: what a star is kept under: an online map's SHA-256, any other
/// map's lower-case file name. A random map has none.
pub fn favorite_key(record: &SkirmishScenarioRecord) -> Option<String> {
    match &record.source {
        SkirmishScenarioSource::Online(sha) => Some(sha.clone()),
        SkirmishScenarioSource::Synthetic => None,
        _ => (!record.file_name.is_empty()).then(|| record.file_name.to_ascii_lowercase()),
    }
}

/// Longest search text the chooser takes.
pub const SEARCH_MAX_CHARS: usize = 32;

fn list_surface(window: RectPx) -> RectPx {
    RectPx::new(window.x, window.y, window.w + 1, window.h + 1)
}

#[derive(Debug, Clone)]
pub struct ChooseMapModalState {
    pub saved_selection: ChooseMapSelection,
    /// FillModeList binds the mode object to the row (`0x005D6298`),
    /// and Use Map keeps that object as the current mode (`0x005E7367`).
    mode_rows: Vec<SkirmishModeSelection>,
    online_folders: OnlineMapFolders,
    /// The game type whose maps are listed; the list cursor is on its row
    /// when it has one.
    pub selected_mode: SkirmishModeSelection,
    /// `0x583`'s enable, set when the map list is built (`0x005E6CB3`).
    random_maps_allowed: bool,
    /// [`ChooseMapContext::official_only`].
    official_only: bool,
    pub filtered_record_indices: Vec<usize>,
    /// A saved seed becomes a playable scenario only after generation.
    saved_seeds: Vec<SavedSeed>,
    filtered_seed_indices: Vec<usize>,
    /// The last listed game type that allowed random maps.
    random_maps_mode: Option<SkirmishModeSelection>,
    /// Favorites and Online Maps only: the game type each row's map is
    /// played with, the first listed one that admits it.
    row_modes: Vec<SkirmishModeSelection>,
    /// VERA: typed text; every list shows only maps whose name holds it.
    pub search: String,
    /// VERA: the maps the player starred, by [`favorite_key`]: a copy of
    /// the map library's, which owns and saves them.
    pub favorites: std::collections::BTreeSet<String>,
    /// The rows' text when the mode fills its own list (Cooperative's
    /// `+0x4C`: `"%d. %s"` per campaign); otherwise each record's name.
    pub row_labels: Option<Vec<String>>,
    /// VERA: per listed row, whether its map can be starred
    /// ([`favorite_key`]); the random map cannot.
    starrable_rows: Vec<bool>,
    /// The highlighted map's name when the game type was last clicked
    /// (`[0xAC0EC8]`), which a rebuilt list reselects.
    reselect_name: String,
    pub highlighted_filtered_index: Option<usize>,
    /// The lists' first rows; their scrollbars move them.
    pub mode_top_index: usize,
    pub map_top_index: usize,
    /// Heading `0x694` and status line `0x695`, alive while `0x105` hides
    /// the chooser. A child writes the status help from its own mouse moves;
    /// `0x6B` has no dialog hit test that clears it, so it keeps the last
    /// text over the background (`0x00611CBA..0x00611E8B`).
    pub(crate) statics: DialogStatics,
    /// Use Map's two-button box while it shows.
    pub message_box: Option<ChooseMapPrompt>,
    /// The map Use Map accepted, kept here while `0x6B` slides out so a
    /// catalog change in the meantime (a kept online map replacing its row,
    /// `map_library`) can remap its record index.
    pub use_selection: Option<ChooseMapSelection>,
    /// Create Random Map hides the chooser (`0x005E6A0B`) until the
    /// random-map run returns. It then shows again (`0x005E6B51`) unless its
    /// Use Map closes it; it stays hidden while that Use Map's eject box asks.
    hidden: bool,
}

/// Native chooser admission depends on its caller: offline requires mode
/// vslot40, network lists all modes and observer callers require vslotBC.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChooseMapContext {
    Skirmish,
    /// `tournament_room`/`clan_room` are the WOL room tests `0x0077D940` and
    /// `0x0077D970`; both false on LAN.
    Network {
        observer: bool,
        tournament_room: bool,
        clan_room: bool,
    },
}
impl ChooseMapContext {
    /// `FillModeList` (`0x005D6130`), with its room tests at
    /// `0x005D6203..0x005D6237`.
    fn permits(self, mode: &SkirmishGameMode) -> bool {
        match self {
            Self::Skirmish => mode.class.policy().lists_in_skirmish,
            Self::Network {
                observer,
                tournament_room,
                clan_room,
            } => {
                (!observer || mode.class.policy().observers_allowed)
                    && (mode.wonline_tournament_allowed || !tournament_room)
                    && (mode.wonline_clan_tournament_allowed || !clan_room)
            }
        }
    }

    /// `IsValidMap`'s room test (`0x005D6409`): a tournament or clan room
    /// lists only official maps and the random map.
    fn official_only(self) -> bool {
        matches!(
            self,
            Self::Network {
                tournament_room: true,
                ..
            }
        )
    }
}

impl ChooseMapModalState {
    /// `0x497` (`0x005E6EA6`): the current game type (or the default) is
    /// selected, its maps listed with the current map selected and scrolled
    /// to the top of the list (`0x005E7000..0x005E701B`; the list clamps the
    /// top). The game-type list keeps its top at 0.
    ///
    /// Skirmish lists only the modes whose class answers vtable `+0x40`
    /// (`0x005D6263`). The fill's observer check (`0x005D6239`, vtable
    /// `+0xBC`) rejects only Cooperative, which that test already drops.
    pub fn open(
        context: ChooseMapContext,
        current_mode_id: i32,
        current_record_index: Option<usize>,
        modes: &[SkirmishGameMode],
        records: &[SkirmishScenarioRecord],
        layout: &ChooseMapModalLayout,
    ) -> Self {
        let selected_mode_id = mode_by_id(modes, current_mode_id)
            .or_else(|| modes.first())
            .map_or(current_mode_id, |mode| mode.id);
        let mut state = Self {
            saved_selection: ChooseMapSelection {
                mode: current_mode_id.into(),
                record_index: current_record_index,
            },
            mode_rows: modes
                .iter()
                .enumerate()
                .filter(|(_, mode)| context.permits(mode))
                .filter_map(|(index, mode)| {
                    if context == ChooseMapContext::Skirmish {
                        SkirmishModeSelection::for_skirmish_row(index, modes)
                    } else {
                        // Peers exchange only the ID, which resolves to the first match.
                        Some(mode.id.into())
                    }
                })
                .collect(),
            selected_mode: selected_mode_id.into(),
            online_folders: OnlineMapFolders::default(),
            random_maps_allowed: false,
            official_only: context.official_only(),
            filtered_record_indices: Vec::new(),
            saved_seeds: Vec::new(),
            filtered_seed_indices: Vec::new(),
            random_maps_mode: None,
            row_modes: Vec::new(),
            search: String::new(),
            favorites: Default::default(),
            row_labels: None,
            starrable_rows: Vec::new(),
            reselect_name: String::new(),
            highlighted_filtered_index: None,
            mode_top_index: 0,
            map_top_index: 0,
            statics: DialogStatics::default(),
            message_box: None,
            use_selection: None,
            hidden: false,
        };
        state.build_map_list(modes, records);
        // The fill selects only the current scenario; when the list lacks it
        // no row is selected (`0x005D6504`, `0x005E6F99`).
        state.highlighted_filtered_index = current_record_index.and_then(|record| {
            state
                .filtered_record_indices
                .iter()
                .position(|idx| *idx == record)
        });
        state.scroll_map_selection_to_top(layout);
        state
    }

    /// The listed game types, top row first.
    pub fn mode_rows(&self) -> &[SkirmishModeSelection] {
        &self.mode_rows
    }

    /// VERA: list the Favorites row after the game types, with the
    /// player's starred maps.
    pub fn add_favorites_row(&mut self, favorites: std::collections::BTreeSet<String>) {
        if !self.mode_rows.contains(&FAVORITES_ROW.into()) {
            self.mode_rows.push(FAVORITES_ROW.into());
        }
        self.favorites = favorites;
    }

    pub fn add_random_maps_row(&mut self, modes: &[SkirmishGameMode]) {
        let Some(mode) = self.random_maps_game_mode(modes) else {
            return;
        };
        self.random_maps_mode = Some(mode);
        if !self.mode_rows.contains(&RANDOM_MAPS_ROW.into()) {
            self.mode_rows.push(RANDOM_MAPS_ROW.into());
        }
    }

    /// Refresh the open folder by file identity so two saves with the same
    /// description remain distinct.
    pub fn set_saved_seeds(&mut self, mut seeds: Vec<SavedSeed>, layout: &ChooseMapModalLayout) {
        let selected = self
            .selected_saved_seed()
            .map(|seed| seed.file_name.clone());
        seeds.retain(SavedSeed::is_valid);
        seeds.sort_by_key(|seed| std::cmp::Reverse(seed.last_write_time));
        self.saved_seeds = seeds;
        self.filter_saved_seeds();
        self.highlighted_filtered_index = selected
            .and_then(|file| {
                self.filtered_seed_indices
                    .iter()
                    .position(|&idx| self.saved_seeds[idx].file_name == file)
            })
            .or_else(|| (!self.filtered_seed_indices.is_empty()).then_some(0));
        self.scroll_map_selection_to_top(layout);
    }

    pub fn random_maps_selected(&self) -> bool {
        self.selected_mode.id == RANDOM_MAPS_ROW
    }

    /// Play a saved seed with the selected game type, the last eligible one,
    /// or the first listed type that allows the generator.
    pub fn random_maps_game_mode(
        &self,
        modes: &[SkirmishGameMode],
    ) -> Option<SkirmishModeSelection> {
        std::iter::once(self.selected_mode)
            .chain(self.random_maps_mode)
            .chain(self.mode_rows.iter().copied())
            .find(|row| {
                self.mode_rows.contains(row)
                    && !is_own_row(row.id)
                    && row
                        .resolve(modes)
                        .is_some_and(|mode| mode.random_maps_allowed)
            })
    }

    pub fn selected_saved_seed(&self) -> Option<&SavedSeed> {
        self.saved_seed_at(self.highlighted_filtered_index?)
    }

    pub fn saved_seed_at(&self, row: usize) -> Option<&SavedSeed> {
        self.saved_seeds.get(*self.filtered_seed_indices.get(row)?)
    }

    fn filter_saved_seeds(&mut self) {
        let search = self.search.to_lowercase();
        self.filtered_seed_indices = self
            .saved_seeds
            .iter()
            .enumerate()
            .filter(|(_, seed)| {
                search.is_empty()
                    || seed
                        .description
                        .display_text()
                        .to_lowercase()
                        .contains(&search)
            })
            .map(|(index, _)| index)
            .collect();
    }

    fn map_row_indices(&self) -> &[usize] {
        if self.random_maps_selected() {
            &self.filtered_seed_indices
        } else {
            &self.filtered_record_indices
        }
    }

    pub fn map_row_count(&self) -> usize {
        self.map_row_indices().len()
    }

    /// VERA: list the Online Maps row last. A room that takes only official
    /// maps has none.
    pub fn add_online_maps_row(&mut self) {
        if self.official_only {
            return;
        }
        if !self.mode_rows.contains(&ONLINE_MAPS_ROW.into()) {
            self.mode_rows.push(ONLINE_MAPS_ROW.into());
        }
    }

    pub(crate) fn set_online_categories(&mut self, folders: OnlineMapFolders) {
        self.online_folders = folders;
    }

    fn sync_online_category_rows(&mut self, layout: &ChooseMapModalLayout) {
        let expanded = self
            .mode_rows
            .last()
            .is_some_and(|row| row.online_category().is_some());
        let show = self.online_maps_selected() && self.online_folders.has_categories();
        if expanded == show {
            return;
        }
        if show {
            // Bring Online Maps up the list so its newly opened folders are visible.
            let parent = self.mode_rows.len().saturating_sub(1);
            self.mode_rows.extend(
                (0..ONLINE_CATEGORIES.len())
                    .map(|i| SkirmishModeSelection::from(FIRST_ONLINE_CATEGORY + i as i32)),
            );
            self.mode_top_index = parent.min(self.mode_geometry(layout).max_top);
        } else {
            self.mode_rows.retain(|row| row.online_category().is_none());
            if self.selected_mode.online_category().is_some() {
                self.selected_mode = ONLINE_MAPS_ROW.into();
            }
            self.mode_top_index = self.mode_top_index.min(self.mode_geometry(layout).max_top);
        }
    }

    /// Whether the Favorites row is the selected one.
    pub fn favorites_selected(&self) -> bool {
        self.selected_mode.id == FAVORITES_ROW
    }

    /// VERA: the catalog's rows changed (the library arrived, or a map was
    /// kept): the list shows them, keeping the highlighted map.
    pub fn refresh(
        &mut self,
        modes: &[SkirmishGameMode],
        records: &[SkirmishScenarioRecord],
        layout: &ChooseMapModalLayout,
    ) {
        self.sync_online_category_rows(layout);
        if self.row_labels.is_none() {
            self.rebuild_keeping_highlight(modes, records, layout);
        }
    }

    /// Whether Online Maps or one of its categories is selected.
    pub fn online_maps_selected(&self) -> bool {
        self.selected_mode.id == ONLINE_MAPS_ROW || self.selected_mode.online_category().is_some()
    }

    fn matches_search(&self, record: &SkirmishScenarioRecord) -> bool {
        self.search.is_empty()
            || record
                .display_name
                .to_lowercase()
                .contains(&self.search.to_lowercase())
    }

    /// Whether `record` is a map the player starred.
    pub fn is_favorite(&self, record: &SkirmishScenarioRecord) -> bool {
        favorite_key(record).is_some_and(|key| self.favorites.contains(&key))
    }

    pub fn mode_geometry(&self, layout: &ChooseMapModalLayout) -> ShellListGeometry {
        ShellListGeometry::new(
            list_surface(layout.mode_list),
            self.mode_rows.len(),
            self.mode_top_index,
            ListMetrics::scaled(layout.scale),
        )
    }

    pub fn map_geometry(&self, layout: &ChooseMapModalLayout) -> ShellListGeometry {
        ShellListGeometry::new(
            list_surface(layout.map_list),
            self.map_row_count(),
            self.map_top_index,
            ListMetrics::scaled(layout.scale),
        )
    }

    pub fn selected_record_index(&self) -> Option<usize> {
        self.highlighted_filtered_index
            .and_then(|idx| self.filtered_record_indices.get(idx).copied())
    }

    /// The row of the selected game type in the game-type list.
    pub fn selected_mode_row(&self) -> Option<usize> {
        self.mode_rows
            .iter()
            .position(|mode| *mode == self.selected_mode)
    }

    fn build_map_list(&mut self, modes: &[SkirmishGameMode], records: &[SkirmishScenarioRecord]) {
        self.build_map_rows(modes, records);
        self.starrable_rows = self
            .filtered_record_indices
            .iter()
            .map(|index| records.get(*index).and_then(favorite_key).is_some())
            .collect();
    }

    fn build_map_rows(&mut self, modes: &[SkirmishGameMode], records: &[SkirmishScenarioRecord]) {
        self.row_labels = None;
        self.row_modes.clear();
        self.filtered_seed_indices.clear();
        if self.random_maps_selected() {
            self.filtered_record_indices.clear();
            self.filter_saved_seeds();
            self.random_maps_allowed = self.random_maps_game_mode(modes).is_some();
            return;
        }
        if is_own_row(self.selected_mode.id) {
            // Each map is played with the first listed game type that takes
            // it, Cooperative aside: it plays campaigns, not maps. One no
            // listed type takes is not offered. Favorites lists
            // the starred maps, online ones only where Online Maps shows;
            // Online Maps lists its starred maps first. Catalog order
            // otherwise.
            let favorites = self.favorites_selected();
            let category = self.selected_mode.online_category().map(|(slug, _)| slug);
            let online_listed = self.mode_rows.contains(&ONLINE_MAPS_ROW.into());
            let listed: Vec<_> = self
                .mode_rows
                .iter()
                .filter_map(|row| row.resolve(modes).map(|mode| (*row, mode)))
                .filter(|(_, mode)| mode.class != crate::skirmish_modes::MpModeClass::Cooperative)
                .collect();
            let mut rows: Vec<_> = (0..records.len())
                .filter(|index| {
                    let record = &records[*index];
                    let listed = if favorites {
                        self.is_favorite(record) && (online_listed || !record.is_online())
                    } else {
                        self.online_folders.includes(record, category)
                    };
                    listed && self.matches_search(record)
                })
                .filter_map(|index| {
                    let (row, _) = listed.iter().find(|(_, mode)| {
                        is_valid_map(&records[index], mode, self.official_only)
                    })?;
                    Some((index, *row))
                })
                .collect();
            if !favorites {
                rows.sort_by_key(|(index, _)| !self.is_favorite(&records[*index]));
            }
            (self.filtered_record_indices, self.row_modes) = rows.into_iter().unzip();
            self.random_maps_allowed = false;
            return;
        }
        let mode = self.selected_mode.resolve(modes);
        // Online maps list only under Online Maps and Favorites.
        self.filtered_record_indices = mode
            .map(|mode| filter_records_for_mode(records, mode, self.official_only))
            .unwrap_or_default()
            .into_iter()
            .filter(|index| !records[*index].is_online() && self.matches_search(&records[*index]))
            .collect();
        self.random_maps_allowed = mode.is_some_and(|mode| mode.random_maps_allowed);
    }

    /// `LB_SETTOPINDEX(selection)`, clamped by the list.
    fn scroll_map_selection_to_top(&mut self, layout: &ChooseMapModalLayout) {
        let max_top = self.map_geometry(layout).max_top;
        self.map_top_index = self.highlighted_filtered_index.unwrap_or(0).min(max_top);
    }

    /// Rebuild the map list for the selected game type and reselect the map
    /// whose name matches the highlighted one (`[0xAC0EC8]`; the last match
    /// wins, row 0 when none), scrolled to the top (`0x005E6C81..0x005E6D43`).
    pub fn select_mode(
        &mut self,
        mode: impl Into<SkirmishModeSelection>,
        modes: &[SkirmishGameMode],
        records: &[SkirmishScenarioRecord],
        layout: &ChooseMapModalLayout,
    ) -> bool {
        let mode = mode.into();
        if (is_own_row(mode.id) && !self.mode_rows.contains(&mode))
            || (!is_own_row(mode.id) && mode.resolve(modes).is_none())
        {
            return false;
        }
        self.reselect_name = self
            .selected_record_index()
            .and_then(|record| records.get(record))
            .map(|record| record.display_name.clone())
            .unwrap_or_default();
        self.selected_mode = mode;
        self.sync_online_category_rows(layout);
        self.random_maps_mode = self.random_maps_game_mode(modes);
        self.build_map_list(modes, records);
        self.reselect(records, layout);
        true
    }

    fn reselect(&mut self, records: &[SkirmishScenarioRecord], layout: &ChooseMapModalLayout) {
        let name = &self.reselect_name;
        let matched = self.filtered_record_indices.iter().rposition(|record| {
            records
                .get(*record)
                .is_some_and(|record| &record.display_name == name)
        });
        self.highlighted_filtered_index =
            matched.or_else(|| (self.map_row_count() > 0).then_some(0));
        self.scroll_map_selection_to_top(layout);
    }

    /// VERA: new search text rebuilds the list, keeping the highlighted map
    /// when it still shows. A list the mode fills itself (Cooperative) stays.
    pub fn set_search(
        &mut self,
        text: String,
        modes: &[SkirmishGameMode],
        records: &[SkirmishScenarioRecord],
        layout: &ChooseMapModalLayout,
    ) {
        self.search = text.chars().take(SEARCH_MAX_CHARS).collect();
        if self.row_labels.is_none() {
            self.rebuild_keeping_highlight(modes, records, layout);
        }
    }

    /// VERA: star or unstar the highlighted map. Returns its
    /// [`favorite_key`] and whether it is now starred.
    pub fn toggle_favorite(
        &mut self,
        modes: &[SkirmishGameMode],
        records: &[SkirmishScenarioRecord],
        layout: &ChooseMapModalLayout,
    ) -> Option<(String, bool)> {
        let key = self
            .favorite_button(records)
            .and_then(|_| favorite_key(records.get(self.selected_record_index()?)?))?;
        let starred = !self.favorites.remove(&key);
        if starred {
            self.favorites.insert(key.clone());
        }
        self.rebuild_keeping_highlight(modes, records, layout);
        Some((key, starred))
    }

    /// The favorite button's state: `Some(starred)` while a map that can
    /// be starred is highlighted. A list the mode fills itself
    /// (Cooperative) and a random map have none.
    pub fn favorite_button(&self, records: &[SkirmishScenarioRecord]) -> Option<bool> {
        if self.row_labels.is_some() {
            return None;
        }
        let record = records.get(self.selected_record_index()?)?;
        favorite_key(record).map(|key| self.favorites.contains(&key))
    }

    fn rebuild_keeping_highlight(
        &mut self,
        modes: &[SkirmishGameMode],
        records: &[SkirmishScenarioRecord],
        layout: &ChooseMapModalLayout,
    ) {
        let highlighted = self
            .highlighted_filtered_index
            .and_then(|i| self.map_row_indices().get(i).copied());
        self.build_map_list(modes, records);
        self.highlighted_filtered_index = highlighted
            .and_then(|index| self.map_row_indices().iter().position(|row| *row == index))
            .or_else(|| (self.map_row_count() > 0).then_some(0));
        // The highlighted map stays in view.
        let geometry = self.map_geometry(layout);
        let mut top = self.map_top_index;
        if let Some(row) = self.highlighted_filtered_index {
            top = top
                .min(row)
                .max((row + 1).saturating_sub(geometry.visible_rows));
        }
        self.map_top_index = top.min(geometry.max_top);
    }

    /// A mode that fills its own list (Cooperative's `+0x4C`) replaces the
    /// rows built from the mode's valid records: `(record, text)` each. When
    /// the chooser opens the current record is selected (`0x005E6F99`);
    /// after a game-type click the name highlighted before it is
    /// (`0x005E6CA2..0x005E6D43`).
    pub fn replace_rows(
        &mut self,
        rows: Vec<(usize, String)>,
        records: &[SkirmishScenarioRecord],
        layout: &ChooseMapModalLayout,
        clicked: bool,
    ) {
        let current = self.saved_selection.record_index;
        let (indices, labels) = rows.into_iter().unzip();
        self.filtered_record_indices = indices;
        self.row_labels = Some(labels);
        if clicked {
            self.reselect(records, layout);
        } else {
            self.highlighted_filtered_index = current.and_then(|record| {
                self.filtered_record_indices
                    .iter()
                    .position(|idx| *idx == record)
            });
            self.scroll_map_selection_to_top(layout);
        }
    }

    /// `LB_ADDSTRING` then `LB_SETITEMDATA` on the map list: a row at the
    /// end, the selection unchanged.
    pub fn push_row(&mut self, record: usize, label: String) {
        self.filtered_record_indices.push(record);
        if let Some(labels) = self.row_labels.as_mut() {
            labels.push(label);
        }
    }

    /// The last row's `LB_DELETESTRING`; a selection on it first moves to
    /// the row before (`LB_SETCURSEL(count - 2)`, none for a single row).
    pub fn pop_row(&mut self) {
        let Some(last) = self.filtered_record_indices.len().checked_sub(1) else {
            return;
        };
        if self.highlighted_filtered_index == Some(last) {
            self.highlighted_filtered_index = last.checked_sub(1);
        }
        self.filtered_record_indices.pop();
        if let Some(labels) = self.row_labels.as_mut() {
            labels.pop();
        }
    }

    /// A press on game-type row `row` (`0x0061A948`). A row whose index
    /// equals the last one clicked (`[0x8316FC]`, which lives across chooser
    /// visits and starts at -1) moves the cursor without rebuilding the map
    /// list (`0x005E6BB7`). VERA rebuilds if the mode selection changes to or
    /// from an explicit catalog entry, since the remembered row can outlive
    /// the map list built for that entry. Online Maps and Favorites always
    /// rebuild their lists. Returns whether it rebuilt.
    pub fn click_mode_row(
        &mut self,
        row: usize,
        layout: &ChooseMapModalLayout,
        modes: &[SkirmishGameMode],
        records: &[SkirmishScenarioRecord],
        last_mode_row: &mut Option<usize>,
    ) -> bool {
        let Some(&mode) = self.mode_rows.get(row) else {
            return false;
        };
        let changes_catalog_entry = self.selected_mode != mode
            && (self.selected_mode.catalog_index.is_some() || mode.catalog_index.is_some());
        if *last_mode_row == Some(row)
            && !is_own_row(self.selected_mode.id)
            && !is_own_row(mode.id)
            && !changes_catalog_entry
        {
            self.selected_mode = mode;
            return false;
        }
        *last_mode_row = Some(row);
        self.select_mode(mode, modes, records, layout);
        true
    }

    pub fn accept_selection(&self) -> Option<ChooseMapSelection> {
        let mode = if is_own_row(self.selected_mode.id) {
            *self.row_modes.get(self.highlighted_filtered_index?)?
        } else {
            self.selected_mode
        };
        Some(ChooseMapSelection {
            mode,
            record_index: Some(self.selected_record_index()?),
        })
    }

    /// Whether Create Random Map is enabled: the built list's mode admits
    /// random maps (`MultiplayerGameMode::RandomMapsAllowed` `0x005D6350`,
    /// read at `0x005E6CAF` and `0x005E6F6F`).
    pub fn random_map_button_enabled(&self) -> bool {
        self.random_maps_allowed
    }

    /// Create Random Map follows the game type; VERA's favorite button is
    /// enabled while a map that can be starred is highlighted in a list of
    /// maps.
    pub fn button_enabled(&self, button: ChooseMapModalButton) -> bool {
        match button {
            ChooseMapModalButton::CreateRandomMap0x583 => self.random_map_button_enabled(),
            ChooseMapModalButton::UseMap0x6c5 if self.random_maps_selected() => {
                self.selected_saved_seed().is_some()
            }
            ChooseMapModalButton::Favorite => {
                self.row_labels.is_none()
                    && self
                        .highlighted_filtered_index
                        .and_then(|row| self.starrable_rows.get(row))
                        .is_some_and(|starrable| *starrable)
            }
            _ => true,
        }
    }

    pub const fn cancel_selection(&self) -> ChooseMapSelection {
        self.saved_selection
    }

    /// Create Random Map: the chooser has slid out and hides (`0x005E6A0B`).
    pub fn hide(&mut self) {
        self.hidden = true;
    }

    /// `ShowWindow` (`0x005E6B51`): a hidden chooser shows again, and its
    /// first paint slides it in.
    pub fn show(&mut self) {
        self.hidden = false;
    }

    pub const fn is_hidden(&self) -> bool {
        self.hidden
    }

    /// Cancel on Use Map's box: Use Map returns false and the chooser
    /// stays (`0x005E733B..0x005E7348`). A chooser the random-map run hid
    /// shows again (`0x005E6B47`). False when no box was up.
    pub fn close_prompt(&mut self) -> bool {
        if self.message_box.take().is_none() {
            return false;
        }
        self.show();
        true
    }

    /// VERA: the random-map dialog could not save its seed. The box says so
    /// over the chooser the run hid, which has no random map selected.
    pub fn show_random_map_failure(&mut self) {
        self.message_box = Some(ChooseMapPrompt {
            selection: ChooseMapSelection {
                mode: self.selected_mode,
                record_index: None,
            },
            kind: ChooseMapPromptKind::RandomMapFailed,
        });
    }

    pub fn create_random_map(
        &mut self,
        records: &mut Vec<SkirmishScenarioRecord>,
        modes: &[SkirmishGameMode],
        display_name: impl Into<String>,
        player_capacity: i32,
        layout: &ChooseMapModalLayout,
    ) -> Option<usize> {
        // Keep the late defense even though normal input is disabled earlier:
        // direct/programmatic callers must not write a disallowed sentinel.
        if !self.random_map_button_enabled() {
            return None;
        }

        if self.random_maps_selected() {
            self.selected_mode = self.random_maps_game_mode(modes)?;
        }
        let record_index = upsert_random_map_sentinel(records, display_name, player_capacity);
        self.build_map_list(modes, records);
        self.highlighted_filtered_index = self
            .filtered_record_indices
            .iter()
            .position(|idx| *idx == record_index)
            .or_else(|| (!self.filtered_record_indices.is_empty()).then_some(0));
        self.scroll_map_selection_to_top(layout);
        Some(record_index)
    }
}

#[cfg(all(test, tests_ui))]
mod tests {
    use super::*;
    use crate::skirmish_modes::stock_skirmish_modes;
    use crate::ui::shell::page::PageCue;
    use crate::ui::skirmish_shell::choose_map_page::{self, ChooserInput};
    use crate::ui::skirmish_shell::compute_choose_map_modal_layout;
    use crate::ui::skirmish_shell::page::Pointer;
    use crate::util::time::{Duration, Instant};

    // The chooser's widgets driven as the app drives them, one shell
    // controller per test thread.
    thread_local! {
        static UI: std::cell::RefCell<crate::ui::shell::controller::DialogController> =
            Default::default();
    }

    #[allow(clippy::too_many_arguments)]
    fn drive(
        modal: &mut ChooseMapModalState,
        layout: &ChooseMapModalLayout,
        lists: (&[SkirmishGameMode], &[SkirmishScenarioRecord]),
        last: &mut Option<usize>,
        at: Pointer,
        (x, y): (i32, i32),
    ) -> ChooserInput {
        UI.with_borrow_mut(|ui| choose_map_page::pointer(ui, modal, layout, lists, last, at, x, y))
    }

    /// What a press on the chooser's lists did.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    enum ChooseMapListPress {
        Missed,
        Consumed,
        RowClicked { rebuilt: bool },
    }

    /// A press that starts a new click sequence (a press off the dialog
    /// first ends any double-click pairing).
    fn mouse_down(
        modal: &mut ChooseMapModalState,
        layout: &ChooseMapModalLayout,
        modes: &[SkirmishGameMode],
        records: &[SkirmishScenarioRecord],
        last: &mut Option<usize>,
        point: (i32, i32),
        _now: Instant,
    ) -> ChooseMapListPress {
        drive(
            modal,
            layout,
            (modes, records),
            last,
            Pointer::Down,
            (-1, -1),
        );
        drive(modal, layout, (modes, records), last, Pointer::Up, (-1, -1));
        let input = drive(modal, layout, (modes, records), last, Pointer::Down, point);
        let on_list = [layout.mode_list, layout.map_list]
            .iter()
            .any(|list| list_surface(*list).contains(point.0, point.1));
        if input.cues.contains(&PageCue::ListClick) {
            ChooseMapListPress::RowClicked {
                rebuilt: input.rebuilt,
            }
        } else if on_list {
            ChooseMapListPress::Consumed
        } else {
            ChooseMapListPress::Missed
        }
    }

    fn record(name: &str) -> SkirmishScenarioRecord {
        let mut record = SkirmishScenarioRecord::concrete_from_ini(
            0,
            crate::map::skirmish_scenarios::SkirmishScenarioSource::Synthetic,
            &format!("{name}.map"),
            &crate::rules::ini_parser::IniFile::from_str(""),
        );
        record.display_name = name.to_string();
        record.game_modes.clear();
        record
    }

    #[test]
    fn a_stale_random_map_press_cannot_dispatch() {
        let modes = stock_skirmish_modes();
        let layout = compute_choose_map_modal_layout(800, 600);
        let mut modal = ChooseMapModalState::open(
            crate::ui::skirmish_shell::ChooseMapContext::Skirmish,
            9,
            None,
            &modes,
            &[],
            &layout,
        );
        let random = layout.rect(0x583);
        let random = (random.x + 2, random.y + 2);
        let at = |modal: &mut ChooseMapModalState, pointer| {
            drive(modal, &layout, (&modes, &[]), &mut None, pointer, random)
        };
        assert!(
            !at(&mut modal, Pointer::Down)
                .cues
                .contains(&PageCue::Button)
        );
        assert_eq!(at(&mut modal, Pointer::Up).activated, None);
        assert!(modal.select_mode(1, &modes, &[], &layout));
        assert!(
            at(&mut modal, Pointer::Down)
                .cues
                .contains(&PageCue::Button)
        );
        assert!(modal.select_mode(9, &modes, &[], &layout));
        assert_eq!(at(&mut modal, Pointer::Up).activated, None);
        let mut records = Vec::new();
        assert_eq!(
            modal.create_random_map(&mut records, &modes, "Disallowed", 4, &layout),
            None
        );
        assert!(
            records.is_empty(),
            "late defense must not upsert a sentinel"
        );
    }

    #[test]
    fn opening_scrolls_the_current_map_to_the_top_clamped() {
        let modes = stock_skirmish_modes();
        let layout = compute_choose_map_modal_layout(800, 600);
        let standard = modes
            .iter()
            .find(|mode| mode.map_filter == "standard")
            .expect("a standard mode");
        let all: Vec<_> = (0..40).map(|i| record(&format!("Map {i:02}"))).collect();
        let modal = ChooseMapModalState::open(
            crate::ui::skirmish_shell::ChooseMapContext::Skirmish,
            standard.id,
            Some(25),
            &modes,
            &all,
            &layout,
        );
        let geometry = modal.map_geometry(&layout);
        // VERA: 17 rows, one fewer than `0x6B`, under the search box.
        assert_eq!(geometry.visible_rows, 17, "surface 325 px tall: 17 rows");
        assert_eq!(modal.highlighted_filtered_index, Some(25));
        assert_eq!(modal.map_top_index, 23, "40 rows - 17 visible");
        let modal = ChooseMapModalState::open(
            crate::ui::skirmish_shell::ChooseMapContext::Skirmish,
            standard.id,
            Some(5),
            &modes,
            &all,
            &layout,
        );
        assert_eq!(modal.map_top_index, 5);
    }

    fn record_for(name: &str, game_modes: &[&str]) -> SkirmishScenarioRecord {
        let mut record = record(name);
        record.game_modes = game_modes.iter().map(|mode| mode.to_string()).collect();
        record
    }

    #[test]
    fn clicking_a_mode_row_uses_its_own_filter_when_ids_collide() {
        let mod_dir = std::env::temp_dir().join(format!("vera-mode-alias-{}", std::process::id()));
        std::fs::create_dir_all(&mod_dir).unwrap();
        let roster_path = mod_dir.join("MPModesMD.ini");
        let mode_rules_path = mod_dir.join("MPMWMD.ini");
        std::fs::write(
            &roster_path,
            "[Battle]\n\
             1=GUI:Battle,STT:ModeBattle,MPBattleMD.ini,standard,true\n\
             01=GUI:Megawealth,STT:ModeMegawealth,MPMWMD.ini,megawealth,false\n",
        )
        .unwrap();
        std::fs::write(
            &mode_rules_path,
            "[MultiplayerDialogSettings]\nAlliesAllowed=no\n",
        )
        .unwrap();
        let modes = {
            let assets =
                crate::assets::asset_manager::AssetManager::from_loose_root_for_test(&mod_dir);
            crate::skirmish_modes::skirmish_modes_from_assets(&assets).unwrap()
        };
        std::fs::remove_file(roster_path).unwrap();
        std::fs::remove_file(mode_rules_path).unwrap();
        std::fs::remove_dir(mod_dir).unwrap();
        assert_eq!(modes.iter().map(|mode| mode.id).collect::<Vec<_>>(), [1, 1]);
        let maps = [
            record_for("Battle Map", &["standard"]),
            record_for("Megawealth Map", &["megawealth"]),
        ];
        let layout = compute_choose_map_modal_layout(800, 600);
        let mut modal = ChooseMapModalState::open(
            ChooseMapContext::Skirmish,
            1,
            Some(0),
            &modes,
            &maps,
            &layout,
        );
        assert_eq!(modal.filtered_record_indices, [0]);
        let second_mode = modal.mode_geometry(&layout).row(1);
        let second_mode_point = (second_mode.x + 2, second_mode.y + 2);
        let mut last_mode_row = None;
        assert_eq!(
            mouse_down(
                &mut modal,
                &layout,
                &modes,
                &maps,
                &mut last_mode_row,
                second_mode_point,
                Instant::now(),
            ),
            ChooseMapListPress::RowClicked { rebuilt: true },
        );
        assert_eq!(modal.filtered_record_indices, [1], "Megawealth Map");

        let previous = modal.cancel_selection();
        let mut modal = ChooseMapModalState::open(
            ChooseMapContext::Skirmish,
            previous.mode.id,
            previous.record_index,
            &modes,
            &maps,
            &layout,
        );
        assert_eq!(
            modal.filtered_record_indices,
            [0],
            "cancel keeps Battle Map"
        );
        mouse_down(
            &mut modal,
            &layout,
            &modes,
            &maps,
            &mut last_mode_row,
            second_mode_point,
            Instant::now(),
        );
        assert_eq!(
            modal.filtered_record_indices,
            [1],
            "reopening and clicking the same row must show Megawealth Map"
        );
        assert_eq!(modal.selected_mode_row(), Some(1));
        assert!(!modal.button_enabled(ChooseMapModalButton::CreateRandomMap0x583));
        let choice = modal.accept_selection().unwrap();
        let shell = crate::ui::skirmish_shell::SkirmishShellState {
            selected_mode: choice.mode,
            selected_map_idx: choice.record_index.unwrap(),
            ..Default::default()
        };
        let launch = crate::ui::skirmish_shell::pack_launch_session_without_start_validation(
            &shell,
            &maps
                .each_ref()
                .map(SkirmishScenarioRecord::to_map_menu_entry),
            &modes,
        )
        .unwrap();
        assert_eq!(
            launch.selected_map_file.as_deref(),
            Some(maps[1].file_name.as_str())
        );
        assert_eq!(launch.mode.id, 1);
        assert_eq!(launch.mode.ui_name_key, "GUI:Megawealth");
        assert_eq!(launch.mode.override_file, "MPMWMD.ini");
        assert!(!launch.mode.allies_allowed);
    }

    #[test]
    fn the_last_same_named_map_wins_and_no_match_takes_row_0() {
        // `0x005E6CDF..0x005E6D43`: `wcscmp` over the new list, the last
        // match kept; no match selects row 0.
        let modes = stock_skirmish_modes();
        let layout = compute_choose_map_modal_layout(800, 600);
        let all = vec![
            record_for("Alpha", &["standard"]),
            record_for("Bravo", &["standard", "teamgame"]),
            record_for("Charlie", &["teamgame"]),
            record_for("Bravo", &["teamgame"]),
            record_for("alpha", &["teamgame"]),
        ];
        let mut modal = ChooseMapModalState::open(
            crate::ui::skirmish_shell::ChooseMapContext::Skirmish,
            1,
            Some(1),
            &modes,
            &all,
            &layout,
        );
        assert!(modal.select_mode(9, &modes, &all, &layout));
        assert_eq!(modal.filtered_record_indices, [1, 2, 3, 4]);
        assert_eq!(modal.selected_record_index(), Some(3), "the last Bravo");
        let mut modal = ChooseMapModalState::open(
            crate::ui::skirmish_shell::ChooseMapContext::Skirmish,
            1,
            Some(0),
            &modes,
            &all,
            &layout,
        );
        assert!(modal.select_mode(9, &modes, &all, &layout));
        assert_eq!(
            modal.highlighted_filtered_index,
            Some(0),
            "case-sensitive: \"alpha\" is not \"Alpha\""
        );
    }

    /// VERA: Favorites lists the starred maps of every game type, each
    /// played with its own; taking a star away drops the row.
    #[test]
    fn favorites_list_starred_maps_of_any_game_type_and_unstarring_drops_the_row() {
        let modes = stock_skirmish_modes();
        let layout = compute_choose_map_modal_layout(800, 600);
        let loose = |name: &str, game_modes: &[&str]| {
            let mut record = record_for(name, game_modes);
            record.source = SkirmishScenarioSource::LooseMap(format!("{name}.map"));
            record
        };
        let mut arctic = record_for("Arctic Pass", &["standard"]);
        arctic.source = SkirmishScenarioSource::Online("b".repeat(64));
        let all = vec![
            loose("Alpha", &["standard"]),
            loose("Bravo", &["teamgame"]),
            arctic,
            record_for("Random Map", &["standard"]),
        ];
        let mut modal = ChooseMapModalState::open(
            crate::ui::skirmish_shell::ChooseMapContext::Skirmish,
            1,
            Some(0),
            &modes,
            &all,
            &layout,
        );
        modal.add_favorites_row(["bravo.map".to_string(), "b".repeat(64)].into());
        assert_eq!(modal.favorite_button(&all), Some(false), "Alpha, no star");
        assert!(modal.select_mode(FAVORITES_ROW, &modes, &all, &layout));
        assert_eq!(
            modal.filtered_record_indices,
            [1],
            "an online map lists only where Online Maps does"
        );
        assert_eq!(modal.accept_selection().unwrap().mode.id, 9, "Bravo's own");
        modal.add_online_maps_row();
        assert!(modal.select_mode(FAVORITES_ROW, &modes, &all, &layout));
        assert_eq!(modal.filtered_record_indices, [1, 2]);
        assert_eq!(modal.favorite_button(&all), Some(true));
        assert_eq!(
            modal.toggle_favorite(&modes, &all, &layout),
            Some(("bravo.map".to_string(), false))
        );
        assert_eq!(modal.filtered_record_indices, [2]);
        // A random map has no file to star.
        assert!(modal.select_mode(1, &modes, &all, &layout));
        modal.highlighted_filtered_index = Some(1);
        assert_eq!(modal.selected_record_index(), Some(3));
        assert_eq!(modal.favorite_button(&all), None);
        assert_eq!(modal.toggle_favorite(&modes, &all, &layout), None);
        assert!(!modal.button_enabled(ChooseMapModalButton::Favorite));
        modal.highlighted_filtered_index = Some(0);
        assert!(modal.button_enabled(ChooseMapModalButton::Favorite));
    }

    #[test]
    fn saved_random_maps_filter_by_name_and_reopen_in_a_playable_mode() {
        use crate::map::rmg::saved_seeds::SavedSeedFile;

        let modes = stock_skirmish_modes();
        let layout = compute_choose_map_modal_layout(800, 600);
        let mut records = vec![record("Ordinary map")];
        let mut modal = ChooseMapModalState::open(
            ChooseMapContext::Skirmish,
            2,
            Some(0),
            &modes,
            &records,
            &layout,
        );
        let seed = |name: &str, description: &str, time| SavedSeed {
            file_name: SavedSeedFile::in_random_maps(name.into()),
            description: description.into(),
            last_write_time: time,
        };
        let older = seed("SAVE0001.SED", "Snow pass", 3);
        let newer = seed("SAVE0002.SED", "Twin peaks", 5);
        modal.add_random_maps_row(&modes);
        assert!(modal.select_mode(RANDOM_MAPS_ROW, &modes, &records, &layout));
        modal.set_saved_seeds(vec![older.clone(), newer.clone()], &layout);
        assert_eq!(modal.selected_saved_seed(), Some(&newer));
        assert_eq!(modal.selected_record_index(), None);
        assert_eq!(modal.favorite_button(&records), None);
        let use_map = ChooseMapModalButton::UseMap0x6c5;
        modal.set_search("missing".into(), &modes, &records, &layout);
        assert!(!modal.button_enabled(use_map));
        modal.set_search("SNOW".into(), &modes, &records, &layout);
        assert_eq!(modal.selected_saved_seed(), Some(&older));
        assert!(modal.button_enabled(use_map));

        let saved = modal.create_random_map(&mut records, &modes, "Snow pass", 4, &layout);
        assert_eq!(modal.selected_mode.id, 2, "keep Free For All");
        assert_eq!(modal.accept_selection().unwrap().record_index, saved);
    }

    #[test]
    fn online_maps_list_only_under_their_row_starred_first_and_search_filters_every_list() {
        let modes = stock_skirmish_modes();
        let layout = compute_choose_map_modal_layout(800, 600);
        let online = |name: &str, sha: char, game_modes: &[&str]| {
            let mut record = record_for(name, game_modes);
            record.source = SkirmishScenarioSource::Online(sha.to_string().repeat(64));
            record
        };
        let all = vec![
            record_for("Alpha", &["standard"]),
            online("Frozen Lakes", 'a', &["teamgame"]),
            online("Arctic Pass", 'b', &["standard"]),
            online("Nowhere", 'c', &["no such game type"]),
        ];
        let mut modal = ChooseMapModalState::open(
            crate::ui::skirmish_shell::ChooseMapContext::Skirmish,
            1,
            Some(0),
            &modes,
            &all,
            &layout,
        );
        modal.add_favorites_row(["b".repeat(64)].into());
        modal.add_online_maps_row();
        assert_eq!(
            modal.filtered_record_indices,
            [0],
            "no game type lists them"
        );
        assert!(modal.select_mode(ONLINE_MAPS_ROW, &modes, &all, &layout));
        assert_eq!(
            modal.filtered_record_indices,
            [2, 1],
            "starred first; none for a map no listed game type takes"
        );
        assert_eq!(modal.accept_selection().unwrap().mode.id, 1);
        modal.highlighted_filtered_index = Some(1);
        assert_eq!(
            modal.accept_selection().unwrap().mode.id,
            9,
            "its own game type"
        );
        modal.set_search("FROZ".into(), &modes, &all, &layout);
        assert_eq!(modal.filtered_record_indices, [1]);
        assert!(modal.select_mode(1, &modes, &all, &layout));
        assert!(modal.filtered_record_indices.is_empty(), "the search stays");

        // A later visit opens on a game type. Online Maps, the last row
        // clicked, still builds its own list.
        let mut modal = ChooseMapModalState::open(
            crate::ui::skirmish_shell::ChooseMapContext::Skirmish,
            1,
            Some(0),
            &modes,
            &all,
            &layout,
        );
        modal.add_online_maps_row();
        let row = modal.mode_rows().len() - 1;
        let at = modal.mode_geometry(&layout).row(row);
        let mut last = Some(row);
        assert_eq!(
            mouse_down(
                &mut modal,
                &layout,
                &modes,
                &all,
                &mut last,
                (at.x + 2, at.y + 2),
                Instant::now()
            ),
            ChooseMapListPress::RowClicked { rebuilt: true }
        );
        assert_eq!(modal.filtered_record_indices, [1, 2]);
        assert!(modal.accept_selection().is_some());

        // A tournament room takes only official maps: no Online Maps row.
        let mut modal = ChooseMapModalState::open(
            crate::ui::skirmish_shell::ChooseMapContext::Network {
                observer: false,
                tournament_room: true,
                clan_room: false,
            },
            1,
            Some(0),
            &modes,
            &all,
            &layout,
        );
        modal.add_online_maps_row();
        assert!(!modal.mode_rows().contains(&ONLINE_MAPS_ROW.into()));
    }

    #[test]
    fn online_folders_filter_search_and_use_the_maps_real_game_type() {
        let layout = compute_choose_map_modal_layout(800, 600);
        let modes = stock_skirmish_modes();
        let sha = "a".repeat(64);
        let mut sea = record_for("Open Sea", &["teamgame"]);
        sea.source = SkirmishScenarioSource::Online(sha.clone());
        let mut land = record_for("Mainland", &["standard"]);
        land.source = SkirmishScenarioSource::Online("b".repeat(64));
        let records = [sea, land];
        let mut modal = ChooseMapModalState::open(
            ChooseMapContext::Network {
                observer: false,
                clan_room: false,
                tournament_room: false,
            },
            1,
            None,
            &modes,
            &records,
            &layout,
        );
        modal.add_online_maps_row();
        assert!(modal.select_mode(ONLINE_MAPS_ROW, &modes, &records, &layout));
        assert_eq!(modal.filtered_record_indices, [0, 1]);
        let root_rows = modal.mode_rows().len();

        // Memberships can arrive while the host already has Online Maps open.
        modal.set_online_categories(OnlineMapFolders::from_memberships(
            [(sha, vec!["megawealth".into(), "navalwar".into()])].into(),
        ));
        modal.refresh(&modes, &records, &layout);
        assert_eq!(modal.mode_rows().len(), root_rows + ONLINE_CATEGORIES.len());
        for category in ["megawealth", "navalwar"] {
            let row = modal
                .mode_rows()
                .iter()
                .position(|row| {
                    row.online_category()
                        .is_some_and(|(slug, _)| slug == category)
                })
                .unwrap();
            assert!(modal.click_mode_row(row, &layout, &modes, &records, &mut None));
            assert_eq!(modal.filtered_record_indices, [0]);
            assert_eq!(modal.accept_selection().unwrap().mode.id, 9);
        }
        modal.set_search("Mainland".into(), &modes, &records, &layout);
        assert!(modal.accept_selection().is_none());
        assert!(modal.select_mode(ONLINE_MAPS_ROW, &modes, &records, &layout));
        assert_eq!(modal.filtered_record_indices, [1]);
        assert!(modal.select_mode(1, &modes, &records, &layout));
        assert_eq!(modal.mode_rows().len(), root_rows);
    }

    #[test]
    fn the_search_edit_takes_typed_text_and_backspace_and_filters_map_names() {
        let modes = stock_skirmish_modes();
        let layout = compute_choose_map_modal_layout(800, 600);
        let all = vec![record("雪地遭遇战"), record("沙漠遭遇战")];
        let mut modal = ChooseMapModalState::open(
            crate::ui::skirmish_shell::ChooseMapContext::Skirmish,
            1,
            Some(0),
            &modes,
            &all,
            &layout,
        );
        let input = |modal: &mut ChooseMapModalState, event| {
            UI.with_borrow_mut(|ui| {
                choose_map_page::search_input(ui, modal, &layout, (&modes, &all), event)
            })
        };
        use vera20k_gui::{InputEvent, Key};
        input(&mut modal, InputEvent::Text("雪".into()));
        input(&mut modal, InputEvent::Text("地".into()));
        assert_eq!(modal.search, "雪地");
        assert_eq!(modal.filtered_record_indices, [0]);
        input(&mut modal, InputEvent::Key(Key::Backspace));
        assert_eq!(modal.search, "雪");
        // The search keeps its first 32 characters; Backspace then takes the
        // 32nd.
        input(&mut modal, InputEvent::Text("x".repeat(40)));
        assert_eq!(modal.search.chars().count(), SEARCH_MAX_CHARS);
        input(&mut modal, InputEvent::Key(Key::Backspace));
        assert_eq!(modal.search.chars().count(), SEARCH_MAX_CHARS - 1);
    }

    #[test]
    fn a_current_map_outside_the_list_opens_with_no_row() {
        let modes = stock_skirmish_modes();
        let layout = compute_choose_map_modal_layout(800, 600);
        let all = vec![
            record_for("Alpha", &["standard"]),
            record_for("Charlie", &["teamgame"]),
        ];
        let modal = ChooseMapModalState::open(
            crate::ui::skirmish_shell::ChooseMapContext::Skirmish,
            1,
            Some(1),
            &modes,
            &all,
            &layout,
        );
        assert_eq!(modal.highlighted_filtered_index, None);
        assert_eq!(modal.accept_selection(), None, "Use Map does nothing");
    }

    #[test]
    fn a_double_click_on_a_row_is_not_a_second_press() {
        let modes = stock_skirmish_modes();
        let layout = compute_choose_map_modal_layout(800, 600);
        let mut modal = ChooseMapModalState::open(
            crate::ui::skirmish_shell::ChooseMapContext::Skirmish,
            1,
            None,
            &modes,
            &[],
            &layout,
        );
        let mode_row = modal.mode_geometry(&layout).row(0);
        let map_row = modal.map_geometry(&layout).row(0);
        let mode_point = (mode_row.x + 2, mode_row.y + 2);
        let map_point = (map_row.x + 2, map_row.y + 2);
        // Presses follow each other well within the double-click time.
        let mut is_double_click = |point| {
            drive(
                &mut modal,
                &layout,
                (&modes, &[]),
                &mut None,
                Pointer::Down,
                point,
            )
            .double_click
        };
        assert!(!is_double_click(mode_point));
        assert!(is_double_click(mode_point));
        // The other list starts a new sequence.
        assert!(!is_double_click(mode_point));
        assert!(!is_double_click(map_point));
        assert!(!is_double_click(mode_point));
        // A press off the lists breaks the sequence.
        assert!(!is_double_click((
            layout.rect(0x6C5).x + 2,
            layout.rect(0x6C5).y + 2
        )));
        assert!(!is_double_click(mode_point));
    }

    #[test]
    fn row_clicks_select_and_scrollbar_presses_capture() {
        let modes = stock_skirmish_modes();
        let layout = compute_choose_map_modal_layout(800, 600);
        let mut modal = ChooseMapModalState::open(
            crate::ui::skirmish_shell::ChooseMapContext::Skirmish,
            modes[0].id,
            None,
            &modes,
            &[],
            &layout,
        );
        modal.filtered_record_indices = (0..30).collect();
        modal.highlighted_filtered_index = Some(0);
        let mut last = None;
        let now = Instant::now();
        let geometry = modal.map_geometry(&layout);
        let row = geometry.row(2);
        assert_eq!(
            mouse_down(
                &mut modal,
                &layout,
                &modes,
                &[],
                &mut last,
                (row.x + 2, row.y + 2),
                now
            ),
            ChooseMapListPress::RowClicked { rebuilt: false }
        );
        assert_eq!(modal.highlighted_filtered_index, Some(2));
        let bar = geometry.scrollbar.expect("30 rows overflow 18");
        assert_eq!(
            mouse_down(
                &mut modal,
                &layout,
                &modes,
                &[],
                &mut last,
                (bar.x + 3, bar.y + bar.h - 3),
                now
            ),
            ChooseMapListPress::Consumed
        );
        assert_eq!(modal.map_top_index, 1);
        // The arrow repeats after 500 ms while held.
        std::thread::sleep(Duration::from_millis(510));
        let poll = |modal: &mut ChooseMapModalState| {
            UI.with_borrow_mut(|ui| choose_map_page::poll(ui, modal, &layout))
        };
        assert!(poll(&mut modal).0);
        assert_eq!(modal.map_top_index, 2);
        drive(
            &mut modal,
            &layout,
            (&modes, &[]),
            &mut last,
            Pointer::Up,
            (bar.x + 3, bar.y + bar.h - 3),
        );
        assert_eq!(poll(&mut modal).1, None);
    }

    #[test]
    fn only_occupied_rows_past_the_limit_eject() {
        let mut shell = crate::ui::skirmish_shell::SkirmishShellState::default();
        for opponent in shell.opponents.iter_mut() {
            opponent.row_type = crate::ui::skirmish_shell::SkirmishAiRowType::None;
        }
        // Slots 0 (player) and 1 fit a two-player map.
        shell.opponents[0].row_type = crate::ui::skirmish_shell::SkirmishAiRowType::Hard;
        assert!(!ai_rows_beyond_limit_occupied(&shell.opponents, 2));
        shell.opponents[1].row_type = crate::ui::skirmish_shell::SkirmishAiRowType::Easy;
        assert!(ai_rows_beyond_limit_occupied(&shell.opponents, 2));
        assert!(!ai_rows_beyond_limit_occupied(&shell.opponents, 3));
        assert!(!ai_rows_beyond_limit_occupied(&shell.opponents, 8));
    }

    #[test]
    fn the_last_clicked_game_type_row_does_not_rebuild_the_list() {
        let modes = stock_skirmish_modes();
        let layout = compute_choose_map_modal_layout(800, 600);
        let mut modal = ChooseMapModalState::open(
            crate::ui::skirmish_shell::ChooseMapContext::Skirmish,
            modes[0].id,
            None,
            &modes,
            &[],
            &layout,
        );
        let geometry = modal.mode_geometry(&layout);
        let row1 = geometry.row(1);
        let now = Instant::now();
        let mut last = None;
        assert_eq!(
            mouse_down(
                &mut modal,
                &layout,
                &modes,
                &[],
                &mut last,
                (row1.x + 2, row1.y + 2),
                now
            ),
            ChooseMapListPress::RowClicked { rebuilt: true }
        );
        assert_eq!(last, Some(1));
        assert_eq!(
            mouse_down(
                &mut modal,
                &layout,
                &modes,
                &[],
                &mut last,
                (row1.x + 2, row1.y + 2),
                now
            ),
            ChooseMapListPress::RowClicked { rebuilt: false }
        );
        // A later visit keeps the global: the same row still does not rebuild.
        let mut modal = ChooseMapModalState::open(
            crate::ui::skirmish_shell::ChooseMapContext::Skirmish,
            modes[0].id,
            None,
            &modes,
            &[],
            &layout,
        );
        assert_eq!(
            mouse_down(
                &mut modal,
                &layout,
                &modes,
                &[],
                &mut last,
                (row1.x + 2, row1.y + 2),
                now
            ),
            ChooseMapListPress::RowClicked { rebuilt: false }
        );
        assert_eq!(
            modal.selected_mode,
            modal.mode_rows()[1],
            "the cursor still moves"
        );
    }

    #[test]
    fn the_lists_match_the_retail_capture() {
        // Retail `0x6B` at 800x600 (cm-6b-steady.png): selection fills at
        // x 117..=311 (game types) and 339..=513 (maps, beside the
        // scrollbar), rows from y 128; scrollbar edge at x 514; the rings
        // at x 115/116 and 312/313, y 126/127 and 471/472.
        let modes = stock_skirmish_modes();
        let layout = compute_choose_map_modal_layout(800, 600);
        let mut modal = ChooseMapModalState::open(
            crate::ui::skirmish_shell::ChooseMapContext::Skirmish,
            1,
            None,
            &modes,
            &[],
            &layout,
        );
        // VERA: both lists are 20 px wider, the game types to the left and
        // the maps to the right.
        assert_eq!(
            modal.mode_geometry(&layout).row(0),
            RectPx::new(97, 128, 215, 19)
        );
        modal.filtered_record_indices = (0..60).collect();
        let maps = modal.map_geometry(&layout);
        // VERA: the map rows start 20 px lower, under the search box.
        assert_eq!(maps.row(0), RectPx::new(339, 148, 195, 19));
        assert_eq!(maps.scrollbar.map(|bar| bar.x), Some(534));
        let lines = vera20k_gui::shell::list::list_frame_lines(layout.mode_list);
        let left = lines.iter().map(|(r, _)| r.x).min();
        let right = lines.iter().map(|(r, _)| r.x + r.w - 1).max();
        let top = lines.iter().map(|(r, _)| r.y).min();
        let bottom = lines.iter().map(|(r, _)| r.y + r.h - 1).max();
        assert_eq!(
            (left, right, top, bottom),
            (Some(95), Some(313), Some(126), Some(472))
        );
    }

    #[test]
    fn skirmish_lists_battle_free_for_all_and_team_game() {
        let modes = stock_skirmish_modes();
        let layout = compute_choose_map_modal_layout(800, 600);
        let modal = ChooseMapModalState::open(
            crate::ui::skirmish_shell::ChooseMapContext::Skirmish,
            9,
            None,
            &modes,
            &[],
            &layout,
        );
        assert_eq!(
            modal
                .mode_rows()
                .iter()
                .map(|row| row.id)
                .collect::<Vec<_>>(),
            [1, 2, 9]
        );
        assert_eq!(modal.selected_mode_row(), Some(2));
        // A current mode Skirmish does not list keeps its maps, on no row.
        let modal = ChooseMapModalState::open(
            crate::ui::skirmish_shell::ChooseMapContext::Skirmish,
            3,
            None,
            &modes,
            &[],
            &layout,
        );
        assert_eq!(modal.selected_mode.id, 3);
        assert_eq!(modal.selected_mode_row(), None);
    }
}
