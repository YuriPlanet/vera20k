//! Movie list dialog `0x129` model: the retail movie table, the campaign
//! movie unlock progress, list rows and the retained selection.
//!
//! Table `0x00832C20`: `{file, CSF label, CD}` triples. The intro row is
//! always listed; the Soviet (`0x00832CA0`) and Allied (`0x00832C30`) rows are
//! listed up to OptionsClass `+0x4C` / `+0x50`. Those progress fields default
//! to -1 (OptionsClass__SetDefaults `0x005FA3C7`), persist in RA2MD.INI as the
//! obfuscated `[Network] NetID` (owned by the options profile), and are raised
//! by `0x005FBF80` whenever Play_Movie resolves a campaign movie name.

use vera20k_gui::{Gui, NodeId, NodeKind, Prop, Value};

use crate::ui::shell::controller::DialogController;
use crate::ui::shell::descriptor::DialogResourceId;
use crate::ui::shell::geom::{RectPx, dlu_rect};
use crate::ui::shell::menu_page::{self, MenuPageButtonSpec, MenuPageLayout, MenuPageSpec};
use vera20k_gui::shell::list::list_box_rect;

pub const MOVIE_LIST_DIALOG: DialogResourceId = DialogResourceId(0x0129);

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
    stacked: &[MenuPageButtonSpec::new(
        PLAY_MOVIE_CONTROL,
        122,
        "GUI:PlayMovie",
        "GUI:PlayMovie",
    )],
    back: MenuPageButtonSpec::new(0x0686, 346, "GUI:Back", "STT:MsnDltButtonBack").returning(-1),
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

vera20k_gui::scaled_rects!(MovieListLayout { page, list, prompt });

/// The list box (subclass `0x0061A948`) as a `List` node under `parent`:
/// window `rect` (its `Rect` the paint surface, [`list_box_rect`]), `rows`
/// the page paints itself, `selected` and `top`. The node holds the selection from then
/// on: the subclass's LB_SETCURSEL handler (`0x0061A534..0x0061A5F5`) always
/// returns 0, so the proc's fallback `LB_SETCURSEL(0)` never runs and a
/// fresh process opens with nothing selected.
pub fn build_family_list(
    gui: &mut Gui,
    parent: NodeId,
    rect: RectPx,
    rows: usize,
    selected: Option<usize>,
    top: usize,
) -> Option<NodeId> {
    let node = gui.create(NodeKind::List, Some(parent)).ok()?;
    gui.set(node, Prop::Rect, Value::Rect(list_box_rect(rect)))
        .ok()?;
    set_family_list(gui, node, rows, selected, top);
    Some(node)
}

/// The host's own change of the list's rows, selection and top.
pub fn set_family_list(
    gui: &mut Gui,
    node: NodeId,
    rows: usize,
    selected: Option<usize>,
    top: usize,
) {
    let int = |v: usize| Value::Int(i32::try_from(v).unwrap_or(i32::MAX));
    let _ = gui.set(node, Prop::RowCount, int(rows));
    let _ = gui.set(node, Prop::Selected, selected.map_or(Value::Int(-1), int));
    let _ = gui.set(node, Prop::Top, int(top));
}

/// The family page `dialog`'s `List` node, wherever its instance sits on
/// the stack.
pub fn find_family_list(controller: &DialogController, dialog: DialogResourceId) -> Option<NodeId> {
    let root = controller.root_in(dialog)?;
    let gui = controller.gui();
    gui.tree()
        .children(root)
        .ok()?
        .iter()
        .copied()
        .find(|node| gui.tree().kind(*node) == Ok(NodeKind::List))
}

/// The row the family page `dialog`'s list has selected.
pub fn family_list_selected(
    controller: &DialogController,
    dialog: DialogResourceId,
) -> Option<usize> {
    controller
        .gui()
        .list_view(find_family_list(controller, dialog)?)?
        .selected
}

/// The family page `dialog`'s `List` node, built with `rows`, `selected`
/// and `top` as a new instance's when the dialog instance has none yet; its
/// window follows `rect`.
pub fn family_list_node(
    controller: &mut DialogController,
    dialog: DialogResourceId,
    rect: RectPx,
    (rows, selected, top): (usize, Option<usize>, usize),
) -> Option<NodeId> {
    if let Some(node) = find_family_list(controller, dialog) {
        let _ = controller
            .gui_mut()
            .set(node, Prop::Rect, Value::Rect(list_box_rect(rect)));
        return Some(node);
    }
    let root = controller.root_in(dialog)?;
    build_family_list(controller.gui_mut(), root, rect, rows, selected, top)
}

/// One `0x129` instance's rows, from `0x005FC000`.
pub type MovieListState = Vec<MovieEntry>;

/// The row the retained `DAT_00825C80` selection names, if any.
pub fn retained_movie_row(rows: &[MovieEntry], retained_selection: i32) -> Option<usize> {
    usize::try_from(retained_selection)
        .ok()
        .filter(|index| *index < rows.len())
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

#[cfg(all(test, tests_ui))]
mod tests {
    use super::*;
    use vera20k_gui::{DialogKind, InputEvent, MouseButton, UiEvent};

    /// A retail `0x129` list as the page builds it, with nothing selected.
    fn list_page() -> (Gui, NodeId) {
        let mut gui = Gui::new(8);
        let root = gui.create(NodeKind::Dialog, None).unwrap();
        let list = active_movie_entries(MovieProgress::default());
        let node = build_family_list(
            &mut gui,
            root,
            compute_movie_list_layout(800, 600).list,
            list.len(),
            retained_movie_row(&list, -1),
            0,
        )
        .unwrap();
        gui.push_dialog(root, DialogKind::Page).unwrap();
        (gui, node)
    }

    /// The row a press at `now` selects, as the list reports it.
    fn press(gui: &mut Gui, x: i32, y: i32, now: u64) -> Option<u32> {
        let button = MouseButton::Left;
        gui.input(InputEvent::PointerDown { x, y, button }, now);
        gui.input(InputEvent::PointerUp { x, y, button }, now);
        gui.drain_events()
            .into_iter()
            .find_map(|event| match event {
                UiEvent::ListSelected { row, .. } => Some(row),
                _ => None,
            })
    }

    #[test]
    fn second_press_inside_the_double_click_window_is_not_a_selection_press() {
        let (mut gui, _) = list_page();
        assert_eq!(press(&mut gui, 150, 137, 0), Some(0));
        assert_eq!(press(&mut gui, 151, 138, 300), None);
        // The double-click ended the sequence: a third press starts anew.
        assert_eq!(press(&mut gui, 151, 138, 400), Some(0));
        // Too slow or too far is a fresh press.
        assert_eq!(press(&mut gui, 151, 138, 1000), Some(0));
        assert_eq!(press(&mut gui, 160, 138, 1100), Some(0));
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

    fn files(progress: MovieProgress) -> Vec<&'static str> {
        active_movie_entries(progress)
            .iter()
            .map(|movie| movie.file)
            .collect()
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
