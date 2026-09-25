//! Movie list dialog `0x129` model: the retail movie table, the campaign
//! movie unlock progress, list rows and the retained selection.
//!
//! Table `0x00832C20`: `{file, CSF label, CD}` triples. The intro row is
//! always listed; the Soviet (`0x00832CA0`) and Allied (`0x00832C30`) rows are
//! listed up to OptionsClass `+0x4C` / `+0x50`. Those progress fields default
//! to -1 (OptionsClass__SetDefaults `0x005FA3C7`), persist in RA2MD.INI as the
//! obfuscated `[Network] NetID` (owned by the options profile), and are raised
//! by `0x005FBF80` whenever Play_Movie resolves a campaign movie name.

use crate::ui::shell::descriptor::DialogId;
use crate::ui::shell::geom::{RectPx, dlu_rect};
use crate::ui::shell::list::{ListScrollInteraction, ROW_HEIGHT, ShellListGeometry};
use crate::ui::shell::menu_page::{self, MenuPageButtonSpec, MenuPageLayout, MenuPageSpec};
use std::time::{Duration, Instant};

pub const MOVIE_LIST_DIALOG: DialogId = DialogId(0x0129);

/// Owner-draw ListBox `0x744`.
pub const MOVIE_LIST_CONTROL: u16 = 0x0744;
/// Play Movie button `0x745`.
pub const PLAY_MOVIE_CONTROL: u16 = 0x0745;
/// Status help the shell writes to `0x695` while the list is hovered.
pub const MOVIE_LIST_TOOLTIP_KEY: &str = "GUI:SelectMovie";
/// Static `0x40C` caption (kind 0: drawn at once, centered, top-aligned).
pub const MOVIE_LIST_PROMPT_KEY: &str = "GUI:SelectMovie";

/// RT_DIALOG `0x129` right-panel buttons. Dialog proc `0x0052D870` writes
/// the selected row's item data (its movie table entry) for Play Movie and
/// -1 for Back.
pub const MOVIE_LIST_PAGE: MenuPageSpec = MenuPageSpec {
    dialog: MOVIE_LIST_DIALOG,
    title_key: "GUI:Blank",
    stacked: &[MenuPageButtonSpec {
        id: PLAY_MOVIE_CONTROL,
        dlu_top: 122,
        csf_key: "GUI:PlayMovie",
        tooltip_key: "GUI:PlayMovie",
        result: None,
    }],
    back: MenuPageButtonSpec {
        id: 0x0686,
        dlu_top: 346,
        csf_key: "GUI:Back",
        tooltip_key: "STT:MsnDltButtonBack",
        result: Some(-1),
    },
};

/// Dialog `0x129` geometry. The list `0x744` and prompt `0x40C` are in none
/// of the reposition sets, so they keep their raw DLU rectangles at every
/// resolution (`0x0060C42E`); the buttons, title and status follow the menu
/// page rules.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MovieListLayout {
    pub page: MenuPageLayout,
    pub list: RectPx,
    pub prompt: RectPx,
}

pub fn compute_movie_list_layout(screen_w: u32, screen_h: u32) -> MovieListLayout {
    MovieListLayout {
        page: menu_page::compute_layout(&MOVIE_LIST_PAGE, screen_w, screen_h),
        list: dlu_rect(80, 79, 266, 187),
        prompt: dlu_rect(80, 45, 266, 24),
    }
}

/// One `0x129` instance: rows from `0x005FC000` and the subclass selection.
#[derive(Debug, Clone)]
pub struct MovieListState {
    pub rows: Vec<MovieEntry>,
    /// Subclass current selection (`-1` = none). Its LB_SETCURSEL handler
    /// (`0x0061A534..0x0061A5F5`) always returns 0, so the proc's fallback
    /// `LB_SETCURSEL(0)` never runs: a fresh process opens with nothing
    /// selected.
    pub selected: Option<usize>,
    pub top: usize,
    /// Previous press for the ListBox class's `CS_DBLCLKS` detection.
    pub last_press: Option<(Instant, i32, i32)>,
    /// Scrollbar capture and arrow repeat (`0x0061C690`).
    pub scroll: ListScrollInteraction,
}

impl MovieListState {
    /// Populate and apply the retained `DAT_00825C80` selection.
    pub fn open(progress: MovieProgress, retained_selection: i32) -> Self {
        let rows = active_movie_entries(progress);
        let selected = usize::try_from(retained_selection)
            .ok()
            .filter(|index| *index < rows.len());
        Self {
            rows,
            selected,
            top: 0,
            last_press: None,
            scroll: ListScrollInteraction::default(),
        }
    }

    /// List and scrollbar geometry. The shared geometry takes the paint
    /// surface, one pixel wider and taller than the `0x744` window, which
    /// reproduces the retail frames: a scrollbar only above 15 rows, at
    /// `x + w - 20` and 20 wide, a 202 px thumb for 17 rows.
    pub fn geometry(&self, list: RectPx) -> ShellListGeometry {
        ShellListGeometry::new(
            RectPx::new(list.x, list.y, list.w + 1, list.h + 1),
            self.rows.len(),
            self.top,
        )
    }

    /// A press on the scrollbar child: capture it, step an arrow or jump the
    /// track. Returns `false` when the press is not on the scrollbar.
    pub fn scroll_press(&mut self, list: RectPx, x: i32, y: i32, now: Instant) -> bool {
        let geometry = self.geometry(list);
        let Some(part) = geometry.scroll_part_at(x, y) else {
            return false;
        };
        self.scroll.press(part, geometry, &mut self.top, y, now);
        true
    }

    /// Pointer motion while the scrollbar may hold capture (thumb drag).
    pub fn scroll_pointer_moved(&mut self, list: RectPx, x: i32, y: i32) {
        let geometry = self.geometry(list);
        self.scroll.pointer_moved(geometry, &mut self.top, x, y);
    }

    /// Arrow auto-repeat; returns whether the list changed.
    pub fn scroll_poll(&mut self, list: RectPx, x: i32, y: i32, now: Instant) -> bool {
        if self.scroll.repeat_at().is_none() {
            return false;
        }
        let geometry = self.geometry(list);
        self.scroll.poll(geometry, &mut self.top, x, y, now)
    }

    /// Whether a press is the second click of a double-click
    /// ([`crate::ui::shell::list::is_double_click`]).
    pub fn is_double_click(
        &mut self,
        now: Instant,
        x: i32,
        y: i32,
        limits: (Duration, i32, i32),
    ) -> bool {
        crate::ui::shell::list::is_double_click(&mut self.last_press, now, x, y, limits)
    }

    /// Row under a pointer press (`0x0061A948`): `client_y / 19 + top`,
    /// without a border correction, ignoring presses below the last row. The
    /// scrollbar is its own child window, so presses on it never reach the
    /// list.
    pub fn row_at(&self, list: RectPx, x: i32, y: i32) -> Option<usize> {
        if !list.contains(x, y)
            || self
                .geometry(list)
                .scrollbar
                .is_some_and(|bar| bar.contains(x, y))
        {
            return None;
        }
        let row = ((y - list.y) / ROW_HEIGHT) as usize + self.top;
        (row < self.rows.len()).then_some(row)
    }
}

/// One row of the retail movie table.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MovieEntry {
    /// Movie stem; Play_Movie appends `.BIK`.
    pub file: &'static str,
    /// CSF key for the list caption.
    pub label_key: &'static str,
    /// CD index checked before playback (`-1` = any).
    pub cd: i32,
}

const fn entry(file: &'static str, label_key: &'static str, cd: i32) -> MovieEntry {
    MovieEntry {
        file,
        label_key,
        cd,
    }
}

const INTRO_MOVIE: MovieEntry = entry("A00_F00E", "Name:IntroMovie", -1);

const SOVIET_MOVIES: [MovieEntry; 8] = [
    entry("S01_F00e", "Name:Sov01MD", 2),
    entry("S02_F00e", "Name:Sov02MD", 2),
    entry("S03_F00e", "Name:Sov03MD", 2),
    entry("S04_F00e", "Name:Sov04MD", 2),
    entry("S05_F00e", "Name:Sov05MD", 2),
    entry("S06_F00e", "Name:Sov06MD", 2),
    entry("S07_F00e", "Name:Sov07MD", 2),
    entry("S08_F00e", "Name:SovFinalMovie", 2),
];

const ALLIED_MOVIES: [MovieEntry; 8] = [
    entry("A01_F00e", "Name:All01MD", 2),
    entry("A02_F00e", "Name:All02MD", 2),
    entry("A03_F00e", "Name:All03MD", 2),
    entry("A04_F00e", "Name:All04MD", 2),
    entry("A05_F00e", "Name:All05MD", 2),
    entry("A06_F00e", "Name:All06MD", 2),
    entry("A07_F00e", "Name:All07MD", 2),
    entry("A08_F00e", "Name:AllFinalMovie", 2),
];

/// Campaign movie unlock progress (OptionsClass `+0x4C` Soviet, `+0x50`
/// Allied). `-1` means none unlocked.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MovieProgress {
    pub soviet: i32,
    pub allied: i32,
}

impl Default for MovieProgress {
    fn default() -> Self {
        Self {
            soviet: -1,
            allied: -1,
        }
    }
}

fn table_index(table: &[MovieEntry], stem: &str) -> i32 {
    table
        .iter()
        .position(|movie| movie.file.eq_ignore_ascii_case(stem))
        .map_or(-1, |index| index as i32)
}

impl MovieProgress {
    /// `0x005FBF80`: raise each side's progress to the `_stricmp` table index
    /// of `stem` (the movie name truncated at its first `.`).
    pub fn note_played(&mut self, stem: &str) {
        self.soviet = self.soviet.max(table_index(&SOVIET_MOVIES, stem));
        self.allied = self.allied.max(table_index(&ALLIED_MOVIES, stem));
    }
}

/// Rows added by `0x005FC000`, in insertion order: the intro, then Soviet
/// rows `0..=soviet`, then Allied rows `0..=allied`.
pub fn active_movie_entries(progress: MovieProgress) -> Vec<MovieEntry> {
    let take = |progress: i32| usize::try_from(progress.saturating_add(1)).unwrap_or(0);
    std::iter::once(INTRO_MOVIE)
        .chain(SOVIET_MOVIES.iter().copied().take(take(progress.soviet)))
        .chain(ALLIED_MOVIES.iter().copied().take(take(progress.allied)))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn second_press_inside_the_double_click_window_is_not_a_selection_press() {
        let limits = (Duration::from_millis(500), 4, 4);
        let mut list = MovieListState::open(MovieProgress::default(), -1);
        let t0 = Instant::now();
        assert!(!list.is_double_click(t0, 150, 137, limits));
        assert!(list.is_double_click(t0 + Duration::from_millis(300), 151, 138, limits));
        // The double-click ended the sequence: a third press starts anew.
        assert!(!list.is_double_click(t0 + Duration::from_millis(400), 151, 138, limits));
        // Too slow or too far is a fresh press.
        assert!(!list.is_double_click(t0 + Duration::from_millis(1000), 151, 138, limits));
        assert!(!list.is_double_click(t0 + Duration::from_millis(1100), 160, 138, limits));
    }

    #[test]
    fn list_and_prompt_keep_raw_dlu_rects_at_every_resolution() {
        for (w, h) in [(640, 480), (800, 600), (1024, 768)] {
            let layout = compute_movie_list_layout(w, h);
            assert_eq!(layout.list, RectPx::new(120, 128, 399, 304));
            assert_eq!(layout.prompt, RectPx::new(120, 73, 399, 39));
        }
        let layout = compute_movie_list_layout(800, 600);
        assert_eq!(
            layout.page.button_rect(PLAY_MOVIE_CONTROL),
            Some(RectPx::new(644, 199, 156, 42))
        );
        assert_eq!(
            layout.page.button_rect(0x0686),
            Some(RectPx::new(644, 535, 156, 42))
        );
    }

    #[test]
    fn fresh_open_selects_nothing_and_retained_selection_restores() {
        let fresh = MovieListState::open(MovieProgress::default(), -1);
        assert_eq!(fresh.rows.len(), 1);
        assert_eq!(fresh.selected, None);
        let reopened = MovieListState::open(MovieProgress::default(), 0);
        assert_eq!(reopened.selected, Some(0));
    }

    #[test]
    fn press_hit_uses_client_y_without_border_correction() {
        let list = RectPx::new(120, 128, 399, 304);
        let state = MovieListState::open(MovieProgress::default(), -1);
        assert_eq!(state.row_at(list, 150, 128), Some(0));
        assert_eq!(state.row_at(list, 150, 146), Some(0));
        assert_eq!(state.row_at(list, 150, 147), None, "row 1 does not exist");
        assert_eq!(state.row_at(list, 119, 130), None);
        assert_eq!(state.row_at(list, 519, 130), None);
    }

    fn files(progress: MovieProgress) -> Vec<&'static str> {
        active_movie_entries(progress)
            .iter()
            .map(|movie| movie.file)
            .collect()
    }

    #[test]
    fn fresh_process_lists_only_the_intro() {
        assert_eq!(files(MovieProgress::default()), ["A00_F00E"]);
    }

    #[test]
    fn playing_a_campaign_movie_unlocks_its_prefix_case_insensitively() {
        let mut progress = MovieProgress::default();
        progress.note_played("s03_f00e");
        assert_eq!(
            progress,
            MovieProgress {
                soviet: 2,
                allied: -1
            }
        );
        progress.note_played("S01_F00E");
        assert_eq!(progress.soviet, 2, "progress never decreases");
        progress.note_played("A08_F00e");
        assert_eq!(
            files(progress),
            [
                "A00_F00E", "S01_F00e", "S02_F00e", "S03_F00e", "A01_F00e", "A02_F00e", "A03_F00e",
                "A04_F00e", "A05_F00e", "A06_F00e", "A07_F00e", "A08_F00e",
            ]
        );
    }

    #[test]
    fn intro_and_sneak_peek_do_not_unlock() {
        let mut progress = MovieProgress::default();
        progress.note_played("A00_F00E");
        progress.note_played("RENEGADE");
        assert_eq!(progress, MovieProgress::default());
    }
}
