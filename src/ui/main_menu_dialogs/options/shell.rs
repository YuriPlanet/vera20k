//! Physical-pixel presentation and input for launcher resource 0xD5.
//!
//! Retail RT_DIALOG/0xD5/1033 provides the DLU rectangles; common setup
//! 0x00608CD0/0x00609730 anchors the rail, and 0x0060B950 adjusts its title.
//! This module projects the retained OptionsDialogState instead of owning a
//! second set of option values or bypassing its ordered preview events.
//! The right-hand column picks a category; its page lays the controls out in
//! one column at the places retail's first band uses. The laid-out controls
//! are nodes of a `vera20k-gui` tree, which owns their hit tests, captures,
//! hover and the open list; the host paints its labels, sliders and boxes
//! with the shared widget painter.

use super::{
    CHECK_FOR_UPDATES_HELP, CHECK_FOR_UPDATES_LABEL, Category, ColorVisionMode, DISPLAY_AUTO_LABEL,
    EDIT_LAYOUT_HELP, EDIT_LAYOUT_LABEL, FULL_SCREEN_LABEL, HAS_CHECK_FOR_UPDATES, HAS_MODS,
    HAS_SET_WINDOW_SIZE, HealthBarMode, IN_GAME_OPTIONS_BACK_HELP, IN_GAME_OPTIONS_HELP,
    LANGUAGE_LABEL, LauncherButton, LauncherCheckboxId, LauncherComboId, LauncherOptionsEvent,
    LauncherParentResult, LauncherTrackbarId, NOT_IN_MATCH_HELP, OptionsDialogState,
    PERFORMANCE_STATS_LABEL, SET_WINDOW_SIZE_HELP, SET_WINDOW_SIZE_LABEL, Setting, SidebarColumns,
    SidebarSize, Zoom,
};
use crate::ui::shell::page::PageCue;
use std::borrow::Cow;

use vera20k_gui::{
    DialogKind, Gui, InputEvent, MouseButton, NodeId, NodeKind, Prop, UiEvent, Value,
};

use crate::i18n::tr;
use crate::render::shell_text::ShellAlign;
use crate::ui::shell::descriptor::DialogId;
use crate::ui::shell::geom::{
    RectPx, ShellScale, center_offset, child_window, dlu_rect, right_panel_rects,
};
use crate::ui::shell::in_game_shell;
use crate::ui::shell::menu_page::{self, MenuPageButtonSpec, MenuPageSpec};
use crate::ui::shell::modal::esc_menu_box_layout_on;
use crate::ui::skirmish_shell::COMBO_FACE_H;

/// What the status line says for a control, and the key of a caption the
/// dialog shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LauncherStatusHelp {
    /// A retail `STT:` string-table key; for a caption, one of
    /// [`super::LAUNCHER_LABEL_SPECS`].
    Csf(&'static str),
    /// A label of VERA's language files ([`crate::i18n`]), for the controls
    /// retail never had.
    Vera(&'static str),
}

use LauncherStatusHelp::{Csf, Vera};

/// A control of the dialog, as the node standing for it in the tree.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Control {
    Button(LauncherButton),
    Slider(LauncherTrackbarId),
    Check(LauncherCheckboxId),
    List(LauncherComboId),
    /// The layout's `labels[i]`.
    Label(usize),
}

impl Control {
    fn kind(self) -> NodeKind {
        match self {
            Self::Button(_) => NodeKind::Button,
            Self::Slider(_) => NodeKind::Trackbar,
            Self::Check(_) => NodeKind::Checkbox,
            Self::List(_) => NodeKind::Combo,
            Self::Label(_) => NodeKind::Text,
        }
    }

    /// The right-hand column's buttons stay through a page switch: the
    /// column doesn't move, so the button the pointer rests on keeps its
    /// hover.
    fn on_column(self) -> bool {
        matches!(self, Self::Button(id) if !id.on_page())
    }

    /// Labels, sliders and boxes paint through the shared widget painter;
    /// buttons and lists with the dialog's own chrome.
    fn generic(self) -> bool {
        matches!(self, Self::Label(_) | Self::Slider(_) | Self::Check(_))
    }
}

/// One control the layout asks for, with what its node shows.
struct Wanted {
    control: Control,
    rect: RectPx,
    props: Vec<(Prop, Value)>,
    help: Option<LauncherStatusHelp>,
}

struct Entry {
    control: Control,
    node: NodeId,
    help: Option<LauncherStatusHelp>,
}

/// The dialog's controls on a `vera20k-gui` tree, which owns their hit tests,
/// the press and capture of buttons and sliders, hover and the open list.
pub(super) struct ShellGui {
    gui: Gui,
    /// Labels, sliders and boxes.
    widgets: NodeId,
    /// Buttons and lists.
    chrome: NodeId,
    entries: Vec<Entry>,
    /// Built for the Esc menu, whose box rows take a press anywhere.
    esc_menu: bool,
    /// Status help last written by a dialog hit test (`0x00622CCB`).
    status_help: Option<LauncherStatusHelp>,
    /// The open list's row under the pointer, from the moves since it opened.
    popup_hovered: Option<usize>,
}

impl Default for ShellGui {
    fn default() -> Self {
        let mut gui = Gui::new(256);
        let root = gui.create(NodeKind::Dialog, None).expect("an empty tree");
        let widgets = gui.create(NodeKind::Panel, Some(root)).expect("budget");
        let chrome = gui.create(NodeKind::Panel, Some(root)).expect("budget");
        gui.push_dialog(root, DialogKind::Page, None, None)
            .expect("a Dialog root");
        Self {
            gui,
            widgets,
            chrome,
            entries: Vec::new(),
            esc_menu: false,
            status_help: None,
            popup_hovered: None,
        }
    }
}

impl std::fmt::Debug for ShellGui {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ShellGui")
            .field("controls", &self.entries.len())
            .field("status_help", &self.status_help)
            .finish()
    }
}

impl ShellGui {
    fn node(&self, control: Control) -> Option<NodeId> {
        self.entries
            .iter()
            .find(|entry| entry.control == control)
            .map(|entry| entry.node)
    }

    fn control(&self, node: NodeId) -> Option<Control> {
        self.entries
            .iter()
            .find(|entry| entry.node == node)
            .map(|entry| entry.control)
    }

    fn input(&mut self, event: InputEvent) {
        self.gui.input(event, 0);
    }

    /// The slider holding the mouse since a press on it.
    fn held_slider(&self) -> Option<LauncherTrackbarId> {
        match self.gui.pressed().and_then(|node| self.control(node)) {
            Some(Control::Slider(id)) => Some(id),
            _ => None,
        }
    }

    fn remove_where(&mut self, drop: impl Fn(&Entry) -> bool) {
        for entry in self.entries.iter().filter(|entry| drop(entry)) {
            let _ = self.gui.remove(entry.node);
        }
        self.entries.retain(|entry| !drop(entry));
    }

    /// A page switch: the page's controls go, and with them a held slider and
    /// an open list; the column and the status help stay. A column button
    /// still held (Esc in the in-game menu) loses its press with its node.
    fn page_switched(&mut self) {
        let pressed = self.gui.pressed();
        self.remove_where(|entry| !entry.control.on_column() || Some(entry.node) == pressed);
        self.popup_hovered = None;
    }

    /// Make the tree hold `wanted`, in order, with their rects and values.
    /// A page that lists other controls than the tree holds is rebuilt; nodes
    /// that stay keep their hover and capture.
    fn sync(&mut self, wanted: Vec<Wanted>, scale: ShellScale, esc_menu: bool) {
        if esc_menu != self.esc_menu {
            self.remove_where(|_| true);
            self.esc_menu = esc_menu;
        }
        let page = |control: &Control| !control.on_column();
        let held: Vec<Control> = self
            .entries
            .iter()
            .map(|entry| entry.control)
            .filter(page)
            .collect();
        let asked: Vec<Control> = wanted
            .iter()
            .map(|want| want.control)
            .filter(page)
            .collect();
        if held != asked {
            self.remove_where(|entry| !entry.control.on_column());
        }
        self.remove_where(|entry| !wanted.iter().any(|want| want.control == entry.control));
        for want in wanted {
            let node = match self.node(want.control) {
                Some(node) => node,
                None => {
                    let panel = if want.control.generic() {
                        self.widgets
                    } else {
                        self.chrome
                    };
                    let node = self
                        .gui
                        .create(want.control.kind(), Some(panel))
                        .expect("a page stays far below the node budget");
                    self.entries.push(Entry {
                        control: want.control,
                        node,
                        help: None,
                    });
                    node
                }
            };
            let _ = self.gui.set(node, Prop::Rect, Value::Rect(want.rect));
            if let Some(help) = want.help {
                let _ = self
                    .gui
                    .set(node, Prop::Tooltip, Value::Text(help.key().to_owned()));
            }
            for (prop, value) in want.props {
                let _ = self.gui.set(node, prop, value);
            }
            if let Some(entry) = self.entries.iter_mut().find(|entry| entry.node == node) {
                entry.help = want.help;
            }
        }
        if let Some(dialog) = self.gui.top_dialog() {
            self.gui.set_dialog_scale(dialog, scale);
        }
    }

    /// The help of the control under the pointer: the tree's hover, which is
    /// enable-unfiltered, so a greyed control still says what it is.
    fn hovered_help(&self) -> Option<LauncherStatusHelp> {
        let node = self.gui.hovered()?;
        self.entries
            .iter()
            .find(|entry| entry.node == node)
            .and_then(|entry| entry.help)
    }
}

impl LauncherStatusHelp {
    /// The key the status line looks up.
    pub(crate) const fn key(self) -> &'static str {
        match self {
            Self::Csf(key) | Self::Vera(key) => key,
        }
    }
}

pub(crate) const KEYBOARD_BUTTON: u16 = 0x05CE;
pub(crate) const MAIN_MENU_BUTTON: u16 = 0x0686;
/// VERA's category buttons under Keyboard, in [`Category::ALL`]'s order. Not
/// retail controls: the ids only name their rects in the page layout.
const CATEGORY_BUTTONS: [u16; 6] = [0x7FF0, 0x7FF1, 0x7FF2, 0x7FF3, 0x7FF4, 0x7FF5];
const _: () = assert!(CATEGORY_BUTTONS.len() == Category::ALL.len());
/// VERA's Mods button, on the tile between the last category and Main Menu.
const MODS_BUTTON: u16 = 0x7FF6;

/// A category's button: the stacked tops go on 27 DLUs at a time from
/// Keyboard's, as the family pages' do, so each lands on the next tile.
/// Its caption is the category's name and its help [`Category::help`].
const fn category_button(index: usize) -> MenuPageButtonSpec {
    MenuPageButtonSpec::new(CATEGORY_BUTTONS[index], 149 + 27 * index as i32, "", "")
}

/// The column above Main Menu, Mods last.
const COLUMN: &[MenuPageButtonSpec] = &[
    MenuPageButtonSpec::new(
        KEYBOARD_BUTTON,
        122,
        "GUI:Keyboard",
        "STT:MainOptButtonKeyboard",
    ),
    category_button(0),
    category_button(1),
    category_button(2),
    category_button(3),
    category_button(4),
    category_button(5),
    MenuPageButtonSpec::new(MODS_BUTTON, 149 + 27 * 6, "", ""),
];

/// `0xD5` as a family page (Session `+0x30D8` clear at the main menu): the
/// right-panel column is Keyboard and Network (`0x006092CB`), then Main Menu
/// (`0x0060982C`); the heading and status line sit at the family places.
/// VERA leaves Network (`0x5CD`) out: it opens the IPX settings page `0xD7`,
/// and VERA's LAN runs on IPv4 UDP. VERA's category buttons take the six
/// tiles under Keyboard, and VERA's Mods the one after them, where the build
/// has mods ([`HAS_MODS`]). Button captions come from the dialog's own labels.
pub(crate) const OPTIONS_PAGE: MenuPageSpec = MenuPageSpec {
    dialog: DialogId(0x00D5),
    title_key: "GUI:OptionsMenu",
    stacked: if HAS_MODS {
        COLUMN
    } else {
        COLUMN.split_at(COLUMN.len() - 1).0
    },
    back: MenuPageButtonSpec::new(
        MAIN_MENU_BUTTON,
        346,
        "GUI:MainMenu",
        "STT:MainOptButtonBack",
    )
    .returning(0x05CB),
};

/// Whether the retail panel at this window size has a tile for every column
/// button above Main Menu's. 640x480's six tiles hold five, too few for the
/// column, so `0xD5` takes the fitted 800x600 page there instead: not native,
/// and the only size whose Options art is scaled while the other pages keep
/// the 640-wide set.
pub(crate) fn retail_column_fits(width: i32, height: i32) -> bool {
    right_panel_rects(width, height).tile_count > OPTIONS_PAGE.stacked.len() as i32
}

// A page in DLUs: the category's name in the first retail band heading's
// place (`GUI:DisplayOptions` at 20,18), then one column of rows from the
// Visual Details caption's place (89,38). Boxes keep the UI Options boxes' x
// and 15-DLU pitch. A slider's track sits 15 DLUs under its caption, as
// Visual Details' does, and the next row starts 9 DLUs under the track: at
// 640x480's 0.8 fit, 8 puts a stacked slider's edge where it maps back a
// pixel off, and that slider loses its frame art. A list's face hangs 16 px
// under its caption's top, as VERA's lists did on main's page, and the next
// row starts 4 px under the face.
const HEADING: (i32, i32) = (20, 18);
const ROW_X: i32 = 89;
const BOX_X: i32 = 93;
const FIRST_ROW: i32 = 38;
const BOX_PITCH: i32 = 15;
const SLIDER_PITCH: i32 = 37;
const LIST_PITCH: i32 = 27;
/// A box's width. Retail's are 130 DLUs, where VERA's longer captions, such as
/// "Custom End Game Banner", wrap, and a caption that wraps loses its second
/// line to the 10-DLU height.
const BOX_W: i32 = 160;
/// Width of the retail resolution list's face in the chrome atlas.
const OPTIONS_COMBO_W: i32 = 180;
const ESC_MENU_ROW_PITCH: i32 = 40;
const ESC_MENU_COLUMN_GAP: i32 = 18;
/// Retail's status help for the in-game Game Options Keyboard button, which
/// the in-game menu shows for its own (`app::input::pause_menu`).
pub(crate) const IN_GAME_KEYBOARD_HELP: &str = "STT:IGGameOptButtonKeyboard";
/// The in-game page's first row. 8 DLUs below the main menu's to clear the
/// frame's top edge at 640x480, and a multiple of 8 so its sliders round to the
/// same authored heights ([`dlu_rect`] is `y * 13 / 8`).
const IN_GAME_FIRST_ROW: i32 = FIRST_ROW + 8;
/// The Game Options page's first row on FF, under the group that heads the page
/// there (`PauseMenuLayout::raise_internet_group`).
const IN_GAME_FIRST_ROW_UNDER_FF_GROUP: i32 = IN_GAME_FIRST_ROW + 80;

/// A caption the page paints, by what it names.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PageLabel {
    /// The category's name, in a retail band heading's place.
    Heading,
    /// A list's caption, over its face.
    List(LauncherComboId),
    /// A setting named by one of VERA's translation keys.
    Localized(&'static str),
    /// A slider's caption, over its track.
    Slider(LauncherTrackbarId),
    /// The step a plaque-less slider stands at, right-aligned over the
    /// track's end as retail prints Visual Details'.
    Step(LauncherTrackbarId),
    /// Static `GUI:Blank` at the status line's template place.
    Blank,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum OptionsSurface {
    Launcher,
    InGame,
    /// The settings area above the Esc menu's footer, in device pixels.
    EscMenu(RectPx),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LauncherOptionsLayout {
    /// The category whose page this is.
    pub(crate) category: Category,
    pub(crate) title: RectPx,
    pub(crate) warning: RectPx,
    /// Status line `0x695`.
    pub(crate) status_help: RectPx,
    pub(crate) labels: Vec<(RectPx, PageLabel)>,
    pub(crate) combos: Vec<(LauncherComboId, RectPx)>,
    pub(crate) trackbars: Vec<(LauncherTrackbarId, RectPx)>,
    pub(crate) checkboxes: Vec<(LauncherCheckboxId, RectPx)>,
    /// The column top to bottom, then the page's own buttons. On the in-game
    /// menu, the buttons it gives the column's rows
    /// ([`Self::in_game`]).
    pub(crate) buttons: Vec<(LauncherButton, RectPx)>,
    /// The mapping the rects are in: [`ShellScale::RETAIL`] centres the
    /// authored dialog in the window, otherwise the authored dialog is
    /// scaled to fit it, centred. Control sub-rects and art fits key on this.
    pub(crate) scale: ShellScale,
    /// The window, which a match shares: the zoom list depends on it.
    screen: (u32, u32),
    /// A battle's frame in this window at the chosen zoom and shape: the
    /// sidebar list depends on it.
    battle_frame: (u32, u32),
    surface: OptionsSurface,
}

/// One page's controls, top to bottom.
struct PageRows {
    labels: Vec<(RectPx, PageLabel)>,
    combos: Vec<(LauncherComboId, RectPx)>,
    trackbars: Vec<(LauncherTrackbarId, RectPx)>,
    checkboxes: Vec<(LauncherCheckboxId, RectPx)>,
    /// Where a further row would start, in DLUs.
    end: i32,
}

/// The rows of `settings` from `first_row` DLUs down, each authored rect put
/// on screen by `px`.
fn page_rows(settings: &[Setting], first_row: i32, px: impl Fn(RectPx) -> RectPx) -> PageRows {
    let dlu = |x, y, w, h| px(dlu_rect(x, y, w, h));
    // The sliders run one pixel wider and taller than the template
    // (retail stills: 181x22 and 129x22).
    let slider = |x, y, w, h| px(child_window(x, y, w, h));
    let mut rows = PageRows {
        labels: Vec::new(),
        combos: Vec::new(),
        trackbars: Vec::new(),
        checkboxes: Vec::new(),
        end: first_row,
    };
    for &setting in settings {
        let top = rows.end;
        match setting {
            Setting::List(id) => {
                rows.labels
                    .push((dlu(ROW_X, top, 120, 10), PageLabel::List(id)));
                // The chrome atlas keys combo faces on their pixel size:
                // 180 is the retail resolution list's face.
                let caption = dlu_rect(ROW_X, top, 0, 0);
                rows.combos.push((
                    id,
                    px(RectPx::new(
                        caption.x,
                        caption.y + 16,
                        OPTIONS_COMBO_W,
                        COMBO_FACE_H,
                    )),
                ));
                rows.end += LIST_PITCH;
            }
            // Trackbar art exists for retail's windows only: 150 DLUs with
            // a value plaque (Generate Map's), 120 without (Visual Details').
            Setting::Slider(id) if id.plaque_reserve() > 0 => {
                rows.labels
                    .push((dlu(ROW_X, top, 150, 10), PageLabel::Slider(id)));
                rows.trackbars.push((id, slider(ROW_X, top + 15, 150, 13)));
                rows.end += SLIDER_PITCH;
            }
            Setting::Slider(id) => {
                rows.labels
                    .push((dlu(ROW_X, top, 60, 10), PageLabel::Slider(id)));
                rows.labels
                    .push((dlu(ROW_X + 60, top, 60, 10), PageLabel::Step(id)));
                rows.trackbars.push((id, slider(ROW_X, top + 15, 120, 13)));
                rows.end += SLIDER_PITCH;
            }
            Setting::Checkbox(id) => {
                rows.checkboxes.push((id, dlu(BOX_X, top, BOX_W, 10)));
                rows.end += BOX_PITCH;
            }
        }
    }
    rows
}

/// The button a page holds under its rows: General's Check for Updates where
/// the build has an updater, and Interface's Edit Layout. Not retail: the
/// column has no tile left for them, so each sits under its page's boxes, the
/// size of Sound Options' Play and Stop (`0xD6`, MNBTTN type 3).
fn page_button(
    category: Category,
    rows: &PageRows,
    px: impl Fn(RectPx) -> RectPx,
) -> Option<(LauncherButton, RectPx)> {
    let id = match category {
        Category::General if HAS_CHECK_FOR_UPDATES => LauncherButton::CheckForUpdates,
        Category::Interface => LauncherButton::EditLayout,
        _ => return None,
    };
    Some((id, px(dlu_rect(ROW_X, rows.end + 6, 83, 15))))
}

impl LauncherOptionsLayout {
    /// Gamemd's centred `0xD5`: the authored dialog placed in the window at
    /// [`ShellScale::RETAIL`], or fitted where the panel is too short for the
    /// column ([`retail_column_fits`]).
    pub(crate) fn new(width: i32, height: i32, category: Category) -> Self {
        let scale = if retail_column_fits(width, height) {
            ShellScale::RETAIL
        } else {
            ShellScale::from_screen(width, height)
        };
        Self::build(width, height, scale, category)
    }

    /// The same dialog scaled to fit the window.
    pub(crate) fn responsive(width: i32, height: i32, category: Category) -> Self {
        Self::build(
            width,
            height,
            ShellScale::from_screen(width, height),
            category,
        )
    }

    fn build(width: i32, height: i32, scale: ShellScale, category: Category) -> Self {
        // The scale centres a scaled page itself, so it takes no centring
        // offset of its own and every authored rect goes through the one scale.
        let stretch = !scale.is_retail();
        let offset_x = if stretch {
            0
        } else {
            center_offset(width, 800)
        };
        let offset_y = if stretch {
            0
        } else {
            center_offset(height, 600)
        };
        let px = |rect: RectPx| scale.scale_rect(rect.translate(offset_x, offset_y));
        let dlu = |x, y, w, h| px(dlu_rect(x, y, w, h));
        // The right-panel children follow the panel, not the dialog centring.
        let panel = if stretch {
            menu_page::compute_responsive_layout(&OPTIONS_PAGE, width as u32, height as u32)
        } else {
            menu_page::compute_layout(&OPTIONS_PAGE, width as u32, height as u32)
        };
        let column = |id| panel.button_rect(id).expect("0xD5 column button");
        let mut buttons = vec![(
            LauncherButton::Exit(LauncherParentResult::Keyboard),
            column(KEYBOARD_BUTTON),
        )];
        buttons.extend(
            Category::ALL
                .into_iter()
                .zip(CATEGORY_BUTTONS)
                .map(|(category, id)| (LauncherButton::Category(category), column(id))),
        );
        if let Some(mods) = panel.button_rect(MODS_BUTTON) {
            buttons.push((LauncherButton::Exit(LauncherParentResult::Mods), mods));
        }
        buttons.push((
            LauncherButton::Exit(LauncherParentResult::Back),
            column(MAIN_MENU_BUTTON),
        ));

        let mut labels = vec![
            (dlu(HEADING.0, HEADING.1, 298, 10), PageLabel::Heading),
            (dlu(2, 355, 303, 12), PageLabel::Blank),
        ];
        let rows = page_rows(category.settings(), FIRST_ROW, px);
        buttons.extend(page_button(category, &rows, px));
        labels.extend(rows.labels);
        Self {
            category,
            title: panel.title,
            warning: panel.warning_monitor,
            status_help: panel.status_help,
            labels,
            combos: rows.combos,
            trackbars: rows.trackbars,
            checkboxes: rows.checkboxes,
            buttons,
            scale,
            screen: (width.max(0) as u32, height.max(0) as u32),
            battle_frame: (width.max(0) as u32, height.max(0) as u32),
            surface: OptionsSurface::Launcher,
        }
    }

    /// VERA's in-game menu (`app::input::pause_menu`): a page left of the
    /// sidebar, on the active-game shell of `size` pixels, placed as gamemd's
    /// ordinary children are there. `column` is the sidebar rows the menu has
    /// for the page's own buttons (`PauseMenuLayout::page_column`): Options
    /// under Game Controls on that page, and on the others the main menu's
    /// column, Keyboard and a button per category, then Back. `game_speed`
    /// is whether Game Controls shows its Game Speed row, which gamemd's
    /// `0xBBB` init decides, and `keyboard` whether it holds the Keyboard
    /// button, for a sidebar with no row for one. `window` is the
    /// one the match shares, which the zoom list depends on. `internet` leaves
    /// room for FF's own group above the Game Options rows, and leaves out
    /// Commander Clips, which only a skirmish plays.
    pub(crate) fn in_game(
        (width, height): (i32, i32),
        column: &[RectPx],
        category: Category,
        internet: bool,
        game_speed: bool,
        keyboard: bool,
        window: (u32, u32),
    ) -> Self {
        let nothing = RectPx::new(0, 0, 0, 0);
        let px = |rect: RectPx| in_game_shell::ordinary_child_rect(rect, width, height);
        let mut buttons = Vec::new();
        if category == Category::GameControls {
            buttons.extend(
                column
                    .first()
                    .map(|&rect| (LauncherButton::InGameOptions, rect)),
            );
        } else if let Some((&back, rows)) = column.split_last() {
            let ids = std::iter::once(LauncherButton::Exit(LauncherParentResult::Keyboard))
                .chain(Category::ALL.map(LauncherButton::Category));
            buttons.extend(ids.zip(rows.iter().copied()));
            buttons.push((LauncherButton::InGameBack, back));
        }
        let first_row = if internet && category == Category::Gameplay {
            IN_GAME_FIRST_ROW_UNDER_FF_GROUP
        } else {
            IN_GAME_FIRST_ROW
        };
        let settings: Vec<Setting> = category
            .settings()
            .iter()
            .copied()
            .filter(|setting| {
                game_speed || *setting != Setting::Slider(LauncherTrackbarId::GameSpeed)
            })
            // The clips play in a skirmish only (`app::commander_feedback`),
            // and under FF's group a 640x480 page has no row left for the box.
            .filter(|setting| {
                !internet || *setting != Setting::Checkbox(LauncherCheckboxId::CommanderFeedback)
            })
            .collect();
        let mut rows = page_rows(&settings, first_row, px);
        buttons.extend(page_button(category, &rows, px));
        if category == Category::GameControls {
            // One row of buttons the size of Check for Updates ends the page:
            // the Esc menu's Set Window Size under its caption, then the
            // dialogs gamemd's Game Controls opens. Keyboard is here only
            // while the sidebar has no row for it.
            let top = rows.end + 3;
            let mut x = ROW_X;
            let mut row = Vec::new();
            if HAS_SET_WINDOW_SIZE {
                rows.labels.push((
                    px(dlu_rect(ROW_X, top, 83, 10)),
                    PageLabel::Localized(SET_WINDOW_SIZE_LABEL),
                ));
                row.push(LauncherButton::SetWindowSize);
            }
            if keyboard {
                row.push(LauncherButton::InGameKeyboard);
            }
            row.push(LauncherButton::InGameSound);
            for button in row {
                buttons.push((button, px(dlu_rect(x, top + 10, 83, 15))));
                x += 90;
            }
        }
        Self {
            category,
            title: nothing,
            warning: nothing,
            status_help: in_game_shell::status_line_rect(height),
            labels: rows.labels,
            combos: rows.combos,
            trackbars: rows.trackbars,
            checkboxes: rows.checkboxes,
            buttons,
            scale: ShellScale::RETAIL,
            screen: window,
            battle_frame: window,
            surface: OptionsSurface::InGame,
        }
    }

    /// Fit the settings between the top of the Allied box and its action row.
    /// The footer supplies the side margins and the native button dimensions.
    pub(crate) fn esc_menu(
        width: i32,
        height: i32,
        scale: ShellScale,
        text_width: impl Fn(&str) -> u32,
    ) -> Self {
        let modal = esc_menu_box_layout_on(width, height, scale);
        let dialog = scale.base_rect(modal.dialog);
        let action = scale.base_rect(modal.ok);
        let inset = action.x - dialog.x;
        let available = dialog.w - 2 * inset;
        let captions = [
            FULL_SCREEN_LABEL,
            PERFORMANCE_STATS_LABEL,
            LANGUAGE_LABEL,
            SET_WINDOW_SIZE_LABEL,
        ];
        let captions = &captions[..3 + usize::from(HAS_SET_WINDOW_SIZE)];
        // Place the controls beside the longest caption in the active language.
        // A long translation can borrow space from the list down to the button's width.
        let max_caption = (available - ESC_MENU_COLUMN_GAP - action.w).max(0) as u32;
        let caption_w = captions
            .iter()
            .map(|key| text_width(&tr(key)))
            .max()
            .unwrap_or(0)
            .min(max_caption) as i32;
        let offset = caption_w + ESC_MENU_COLUMN_GAP;
        let value_x = action.x + offset;
        let control_w = OPTIONS_COMBO_W.min(available - offset);
        let body = RectPx::new(
            action.x,
            dialog.y + 8,
            offset + control_w,
            action.y - dialog.y - 16,
        );
        let row_h = COMBO_FACE_H.max(action.h);
        let rows = captions.len() as i32;
        let top = body.y + (body.h - (rows - 1) * ESC_MENU_ROW_PITCH - row_h) / 2;
        let cell = |row: i32, x, w, h| {
            scale.scale_rect(RectPx::new(
                x,
                top + row * ESC_MENU_ROW_PITCH + (row_h - h) / 2,
                w,
                h,
            ))
        };
        let control = |row, w, h| cell(row, value_x, w, h);
        let nothing = RectPx::new(0, 0, 0, 0);
        use LauncherCheckboxId::{FullScreen, PerformanceStats};
        let labels = captions
            .iter()
            .enumerate()
            .map(|(row, &key)| {
                (
                    cell(row as i32, body.x, offset, 17),
                    PageLabel::Localized(key),
                )
            })
            .collect();
        let mut buttons = Vec::new();
        if HAS_SET_WINDOW_SIZE {
            buttons.push((
                LauncherButton::SetWindowSize,
                control(3, action.w, action.h),
            ));
        }
        Self {
            category: Category::ALL[0],
            title: nothing,
            warning: nothing,
            status_help: nothing,
            labels,
            combos: vec![(
                LauncherComboId::Language,
                control(2, control_w, COMBO_FACE_H),
            )],
            trackbars: Vec::new(),
            checkboxes: vec![
                (FullScreen, control(0, control_w, 18)),
                (PerformanceStats, control(1, control_w, 18)),
            ],
            buttons,
            scale,
            screen: (width.max(0) as u32, height.max(0) as u32),
            battle_frame: (width.max(0) as u32, height.max(0) as u32),
            surface: OptionsSurface::EscMenu(scale.scale_rect(body)),
        }
    }

    /// The same page for a battle frame other than the window: a zoomed or
    /// 4:3 match (`App::match_zoom_geometry`).
    pub(crate) fn with_battle_frame(mut self, battle_frame: (u32, u32)) -> Self {
        self.battle_frame = battle_frame;
        self
    }

    /// The Sidebar list's rows, top to bottom.
    pub(crate) fn sidebar_choices(&self) -> Vec<SidebarSize> {
        SidebarSize::choices(self.battle_frame.0, self.battle_frame.1)
    }

    /// The window size the zoom choices are for.
    pub(crate) fn screen(&self) -> (u32, u32) {
        self.screen
    }

    /// The Zoom list's rows, top to bottom.
    pub(crate) fn zoom_choices(&self) -> Vec<Zoom> {
        Zoom::choices(self.screen.0, self.screen.1)
    }

    /// The slider's rect when this page has it.
    #[cfg(test)]
    pub(crate) fn trackbar_rect(&self, id: LauncherTrackbarId) -> Option<RectPx> {
        self.trackbars
            .iter()
            .find(|(candidate, _)| *candidate == id)
            .map(|(_, rect)| *rect)
    }

    /// The list's rect when this page has it.
    #[cfg(test)]
    pub(crate) fn combo_rect(&self, id: LauncherComboId) -> Option<RectPx> {
        self.combos
            .iter()
            .find(|(candidate, _)| *candidate == id)
            .map(|(_, rect)| *rect)
    }
}

/// A caption the dialog shows: the heading, then the page's labels.
#[cfg(test)]
#[derive(Debug, Clone)]
pub(crate) struct LauncherShellLabel<'a> {
    pub(crate) rect: RectPx,
    pub(crate) text: Cow<'a, str>,
}

/// An open list: one row per [`LauncherOptionsLayout::zoom_choices`] or
/// [`LauncherOptionsLayout::sidebar_choices`], in order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct LauncherComboPopup {
    pub(crate) combo: LauncherComboId,
    pub(crate) rect: RectPx,
    pub(crate) row_height: i32,
    pub(crate) rows: usize,
    pub(crate) selected: usize,
    pub(crate) hovered: Option<usize>,
}

impl LauncherComboPopup {
    pub(crate) fn row_rect(self, row: usize) -> RectPx {
        RectPx::new(
            self.rect.x,
            self.rect.y + row as i32 * self.row_height,
            self.rect.w,
            self.row_height,
        )
    }
}

/// A list's rows on a layout, the row and text its face shows, and what a pick
/// of each row publishes.
pub(crate) struct LauncherList {
    pub(crate) rows: Vec<String>,
    pub(crate) shown: Option<usize>,
    pub(crate) face: String,
    picks: Vec<LauncherOptionsEvent>,
}

impl LauncherList {
    /// `choices` with `shown` on the face.
    fn new<T: PartialEq>(
        choices: Vec<T>,
        shown: &T,
        label: impl Fn(&T) -> String,
        pick: fn(T) -> LauncherOptionsEvent,
    ) -> Self {
        Self {
            rows: choices.iter().map(&label).collect(),
            shown: choices.iter().position(|choice| choice == shown),
            face: label(shown),
            picks: choices.into_iter().map(pick).collect(),
        }
    }
}

impl OptionsDialogState {
    /// Native61E3AF..61E44E drops held/drag state once MK_LBUTTON is absent.
    /// Focus loss is the host's authoritative lost-gesture boundary; an open
    /// list stays.
    pub(crate) fn shell_cancel_pointer_gesture(&mut self) {
        self.shell.gui.cancel_pointer();
    }

    /// Whether an open list or a held slider has the pointer, so nothing
    /// outside the dialog should answer it.
    pub(crate) fn shell_pointer_taken(&self) -> bool {
        self.shell_list_open() || self.shell.held_slider().is_some()
    }

    /// Whether a list is open.
    pub(crate) fn shell_list_open(&self) -> bool {
        self.shell.gui.popup().is_some()
    }

    /// Close the open list with its close sound. False when none was open.
    pub(crate) fn shell_close_list(&mut self) -> bool {
        if !self.shell.gui.close_popup() {
            return false;
        }
        self.pending_events
            .push(LauncherOptionsEvent::Cue(PageCue::ComboClose));
        self.shell.popup_hovered = None;
        true
    }

    /// A page switch drops the page's controls from the tree.
    pub(super) fn shell_page_switched(&mut self) {
        self.shell.page_switched();
    }

    /// The tree the labels, sliders and boxes paint from, and their parent.
    pub(crate) fn shell_widgets(&self) -> (&vera20k_gui::Gui, NodeId) {
        (&self.shell.gui, self.shell.widgets)
    }

    /// A page label's text and alignment: left, but a step name sits
    /// right-aligned over its track's end.
    fn page_label(
        &self,
        layout: &LauncherOptionsLayout,
        label: PageLabel,
    ) -> (Cow<'_, str>, ShellAlign) {
        let (left, right) = (ShellAlign::NONE, ShellAlign::H_RIGHT);
        match label {
            PageLabel::Heading => (self.shell_category_label(layout.category), left),
            PageLabel::List(id) => (Cow::Owned(tr(id.text().0)), left),
            PageLabel::Localized(key) => (Cow::Owned(tr(key)), left),
            PageLabel::Slider(LauncherTrackbarId::GameSpeed) => {
                (self.game_speed_caption.as_str().into(), left)
            }
            PageLabel::Slider(id) => (self.text(id.text().0), left),
            PageLabel::Step(id) => (Cow::Borrowed(self.slider_step(id)), right),
            PageLabel::Blank => (Cow::Borrowed(self.labels.get("GUI:Blank")), left),
        }
    }

    /// A category's name: retail's band heading for the four bands `0xD5`
    /// has, VERA's own for Controls and General, and the in-game menu's own
    /// button for Game Controls.
    pub(crate) fn shell_category_label(&self, category: Category) -> Cow<'_, str> {
        self.text(match category {
            Category::Display => Csf("GUI:DisplayOptions"),
            Category::Gameplay => Csf("GUI:GameOptions"),
            Category::Interface => Csf("GUI:UIOptions"),
            Category::Audio => Csf("GUI:AudioOptions"),
            Category::Controls => Vera("VERA:Controls"),
            Category::General => Vera("VERA:General"),
            Category::GameControls => Csf("GUI:GameControls"),
        })
    }

    /// The step name a plaque-less slider prints; a plaque slider shows its
    /// value on the plaque instead.
    pub(super) fn slider_step(&self, id: LauncherTrackbarId) -> &str {
        let position = usize::from(self.trackbar_position(id));
        match id {
            LauncherTrackbarId::GameSpeed => self
                .game_speed_names
                .get(position)
                .map_or("", String::as_str),
            _ => id
                .text()
                .2
                .get(position)
                .map_or("", |key| self.labels.get(key)),
        }
    }

    /// In the two-column form the setting is already named on the left.
    pub(crate) fn shell_checkbox_face_text(
        &self,
        layout: &LauncherOptionsLayout,
        id: LauncherCheckboxId,
    ) -> Cow<'_, str> {
        if matches!(layout.surface, OptionsSurface::EscMenu(_)) {
            let checked = self.shell_checkbox_checked(id);
            return Cow::Borrowed(self.labels.get(if checked { "GUI:Yes" } else { "GUI:No" }));
        }
        self.shell_checkbox_label(id)
    }

    pub(crate) fn shell_checkbox_label(&self, id: LauncherCheckboxId) -> Cow<'_, str> {
        self.text(id.spec().0)
    }

    pub(crate) fn shell_button_label(&self, id: LauncherButton) -> Cow<'_, str> {
        self.text(match id {
            LauncherButton::Exit(LauncherParentResult::Keyboard)
            | LauncherButton::InGameKeyboard => Csf("GUI:Keyboard"),
            LauncherButton::Exit(LauncherParentResult::Back) => Csf("GUI:MainMenu"),
            LauncherButton::Exit(LauncherParentResult::Mods) => {
                Vera(crate::ui::mods_shell::TITLE_KEY)
            }
            LauncherButton::Exit(LauncherParentResult::Terminal) => return Cow::Borrowed(""),
            LauncherButton::Category(category) => return self.shell_category_label(category),
            LauncherButton::InGameOptions => Csf("GUI:OptionsMenu"),
            LauncherButton::InGameBack => Csf("GUI:Back"),
            LauncherButton::CheckForUpdates => Vera(CHECK_FOR_UPDATES_LABEL),
            LauncherButton::EditLayout => Vera(EDIT_LAYOUT_LABEL),
            LauncherButton::SetWindowSize => {
                let (width, height) = self.launch_window_size;
                return Cow::Owned(format!("{width}x{height}"));
            }
            LauncherButton::InGameSound => return Cow::Borrowed(&self.in_game_sound_label),
        })
    }

    /// Whether `id` is the column button of the page that shows.
    pub(crate) fn shell_button_selected(&self, id: LauncherButton) -> bool {
        id == LauncherButton::Category(self.category)
    }

    /// The dialog's heading, the same on every page.
    pub(crate) fn shell_title_text(&self) -> &str {
        self.labels.get("GUI:OptionsMenu")
    }

    /// The zoom the face shows: a fixed choice this window can't reach shows
    /// as the step it gives way to.
    pub(crate) fn shell_selected_zoom(&self, layout: &LauncherOptionsLayout) -> Zoom {
        match self.zoom {
            Zoom::Auto => Zoom::Auto,
            fixed => Zoom::Percent(fixed.percent(layout.screen.0, layout.screen.1)),
        }
    }

    /// The pushed look: the button holds the press and the pointer is still
    /// on it (612B70).
    pub(crate) fn shell_button_pressed(&self, id: LauncherButton) -> bool {
        self.shell
            .node(Control::Button(id))
            .is_some_and(|node| self.shell.gui.shows_pressed(node))
    }

    /// The sidebar size the face shows: a size this battle frame has no room
    /// for shows as the size it gives way to.
    pub(crate) fn shell_selected_sidebar(&self, layout: &LauncherOptionsLayout) -> SidebarSize {
        let (width, height) = layout.battle_frame;
        self.values.sidebar.in_effect(width, height)
    }

    /// `combo`'s list on this page. The Display list shows a screen the window
    /// layer no longer lists by its id, numbering the others from 1; the
    /// Language list names each language in its own words.
    pub(crate) fn shell_list(
        &self,
        combo: LauncherComboId,
        layout: &LauncherOptionsLayout,
    ) -> LauncherList {
        use LauncherOptionsEvent as E;
        let v = &self.values;
        match combo {
            LauncherComboId::Zoom => {
                let (screen, original_shape) = (layout.screen, v.original_shape);
                let label = |zoom: &Zoom| super::zoom_label(*zoom, screen, original_shape);
                LauncherList::new(
                    layout.zoom_choices(),
                    &self.shell_selected_zoom(layout),
                    label,
                    E::ZoomSelected,
                )
            }
            LauncherComboId::Sidebar => {
                let shown = self.shell_selected_sidebar(layout);
                LauncherList::new(
                    layout.sidebar_choices(),
                    &shown,
                    |size| size.label(),
                    E::SidebarSelected,
                )
            }
            LauncherComboId::SidebarColumns => {
                let choices = SidebarColumns::choices();
                LauncherList::new(
                    choices,
                    &v.sidebar_columns,
                    |c| c.label(),
                    E::SidebarColumnsSelected,
                )
            }
            LauncherComboId::HealthBars => LauncherList::new(
                HealthBarMode::choices(),
                &v.health_bars,
                |m| m.label(),
                E::HealthBarModeSelected,
            ),
            LauncherComboId::ColorVision => {
                let choices = ColorVisionMode::choices();
                LauncherList::new(
                    choices,
                    &v.color_vision,
                    |m| m.label(),
                    E::ColorVisionModeSelected,
                )
            }
            LauncherComboId::Ares => LauncherList::new(
                super::ares_choices(),
                &v.ares,
                |s| super::ares_label(*s),
                E::AresSelected,
            ),
            LauncherComboId::Display => {
                let label = |target: &super::DisplayTarget| match target {
                    super::DisplayTarget::Auto => tr(DISPLAY_AUTO_LABEL),
                    super::DisplayTarget::Fixed(id) => self
                        .displays
                        .iter()
                        .position(|choice| &choice.id == id)
                        .map_or_else(|| id.clone(), |index| self.displays[index].label(index + 1)),
                };
                LauncherList::new(
                    self.display_choices(),
                    &self.display,
                    label,
                    E::DisplaySelected,
                )
            }
            LauncherComboId::Language => {
                let tags = self
                    .languages
                    .iter()
                    .map(|choice| choice.tag.clone())
                    .collect();
                let label = |tag: &String| {
                    self.languages
                        .iter()
                        .find(|choice| &choice.tag == tag)
                        .map_or_else(|| tag.clone(), |choice| choice.name.clone())
                };
                LauncherList::new(tags, &self.language, label, E::LanguageSelected)
            }
        }
    }

    /// The open list, on the tree's geometry: it hangs under the painted face
    /// and its rows use this window's dropdown device metrics, the same the
    /// combo pages use. Unlike the template's 74-DLU drop allocation
    /// (ComboDropWin `0x006180CF..0x00618205`, five whole rows), it grows with
    /// the choices, so it never scrolls: a 4K window's nine zoom rows still
    /// end well above the page's bottom edge.
    pub(crate) fn shell_popup(&self, layout: &LauncherOptionsLayout) -> Option<LauncherComboPopup> {
        let node = self.shell.gui.popup()?;
        let Some(Control::List(combo)) = self.shell.control(node) else {
            return None;
        };
        let popup = self.shell.gui.combo_drop_down(node)?.popup;
        Some(LauncherComboPopup {
            combo,
            rect: popup.rect,
            row_height: popup.row_height,
            rows: popup.total,
            selected: self.shell_list(combo, layout).shown.unwrap_or(0),
            hovered: self.shell.popup_hovered,
        })
    }

    /// The status help the line shows: what the last dialog hit test wrote.
    /// While a slider or button holds the mouse capture, or a list is open,
    /// the dialog gets no hit test and the text stays.
    pub(crate) fn shell_status_help(&self) -> Option<LauncherStatusHelp> {
        self.shell.status_help
    }

    /// Status help (`0x00604729..0x00604838`) for `control`.
    fn control_help(
        &self,
        control: Control,
        layout: &LauncherOptionsLayout,
    ) -> Option<LauncherStatusHelp> {
        let id = match control {
            Control::Button(id) => id,
            // A slider greyed for want of sound keeps its own help.
            Control::Slider(id) if self.in_match && id.fixed_in_match() => {
                return Some(Vera(NOT_IN_MATCH_HELP));
            }
            Control::Slider(id) => return Some(id.text().1),
            Control::Check(id) => return Some(id.spec().1),
            Control::List(combo) => return Some(Vera(combo.text().1)),
            Control::Label(_) => return None,
        };
        let in_game = layout.surface == OptionsSurface::InGame;
        Some(match id {
            LauncherButton::Category(category) => return category.help().map(Vera),
            LauncherButton::InGameOptions => Vera(IN_GAME_OPTIONS_HELP),
            LauncherButton::InGameBack => Vera(IN_GAME_OPTIONS_BACK_HELP),
            LauncherButton::CheckForUpdates if self.button_enabled(id) => {
                Vera(CHECK_FOR_UPDATES_HELP)
            }
            LauncherButton::CheckForUpdates => Vera(NOT_IN_MATCH_HELP),
            // Its help starts with "in a match", so the greyed button
            // says why as well.
            LauncherButton::EditLayout => Vera(EDIT_LAYOUT_HELP),
            LauncherButton::Exit(LauncherParentResult::Mods) => {
                Vera(crate::ui::mods_shell::BUTTON_HELP_KEY)
            }
            // The main menu's Esc box has no status line; the in-game
            // menu has.
            LauncherButton::SetWindowSize => return in_game.then_some(Vera(SET_WINDOW_SIZE_HELP)),
            // Retail's strings for the in-game Game Options buttons.
            LauncherButton::InGameKeyboard => Csf(IN_GAME_KEYBOARD_HELP),
            LauncherButton::Exit(LauncherParentResult::Keyboard) if in_game => {
                Csf(IN_GAME_KEYBOARD_HELP)
            }
            LauncherButton::InGameSound => Csf("STT:IGGameOptButtonSound"),
            LauncherButton::Exit(LauncherParentResult::Keyboard) => {
                return OPTIONS_PAGE
                    .button(KEYBOARD_BUTTON)
                    .map(|button| Csf(button.tooltip_key));
            }
            LauncherButton::Exit(LauncherParentResult::Back | LauncherParentResult::Terminal) => {
                return OPTIONS_PAGE
                    .button(MAIN_MENU_BUTTON)
                    .map(|button| Csf(button.tooltip_key));
            }
        })
    }

    /// What the layout's controls are, as nodes: their rects, values,
    /// enabled states and help.
    fn shell_wanted(&self, layout: &LauncherOptionsLayout) -> Vec<Wanted> {
        let mut wanted = Vec::new();
        let mut want = |control, rect, props| {
            wanted.push(Wanted {
                control,
                rect,
                props,
                help: self.control_help(control, layout),
            })
        };
        for (index, &(rect, label)) in layout.labels.iter().enumerate() {
            let (text, align) = self.page_label(layout, label);
            want(
                Control::Label(index),
                rect,
                vec![
                    (Prop::Text, Value::Text(text.into_owned())),
                    (Prop::Align, Value::Int(i32::from(align.0))),
                ],
            );
        }
        for &(id, rect) in &layout.trackbars {
            let base = id.value_base();
            want(
                Control::Slider(id),
                rect,
                vec![
                    (Prop::Min, Value::Int(base)),
                    (Prop::Max, Value::Int(base + i32::from(id.maximum()))),
                    (Prop::Step, Value::Int(1)),
                    (
                        Prop::Pos,
                        Value::Int(base + i32::from(self.trackbar_position(id))),
                    ),
                    (Prop::ShowValue, Value::Bool(id.plaque_reserve() > 0)),
                    (Prop::Enabled, Value::Bool(self.trackbar_enabled(id))),
                ],
            );
        }
        for &(id, rect) in &layout.checkboxes {
            // The boxes toggle on the press (`0x006163A0`).
            let mut props = vec![
                (Prop::Checked, Value::Bool(self.shell_checkbox_checked(id))),
                (
                    Prop::Text,
                    Value::Text(self.shell_checkbox_face_text(layout, id).into_owned()),
                ),
                (Prop::ActivateOnPress, Value::Bool(true)),
            ];
            if let OptionsSurface::EscMenu(body) = layout.surface {
                // The label and the Yes/No value are both part of this row.
                props.push((
                    Prop::HitRect,
                    Value::Rect(RectPx {
                        x: body.x,
                        w: body.w,
                        ..rect
                    }),
                ));
            }
            want(Control::Check(id), rect, props);
        }
        for &(combo, rect) in &layout.combos {
            // Selected stays unset: a pick publishes even the row the face
            // shows, and the painted highlight is the dialog's own.
            let mut props = vec![
                (
                    Prop::RowCount,
                    Value::Int(self.shell_list(combo, layout).rows.len() as i32),
                ),
                (Prop::Selected, Value::Int(-1)),
                // VERA's lists open from anywhere on their face.
                (Prop::OpenAnywhere, Value::Bool(true)),
            ];
            if let OptionsSurface::EscMenu(body) = layout.surface {
                // Keep the list in the control column and lift it just
                // enough to clear the footer. Extra installed languages can
                // use the window above the panel as well.
                props.push((Prop::PopupBottom, Value::Int(body.y + body.h)));
            }
            want(Control::List(combo), rect, props);
        }
        for &(id, rect) in &layout.buttons {
            // A disabled button takes the press and does nothing.
            want(
                Control::Button(id),
                rect,
                vec![(Prop::Enabled, Value::Bool(self.button_enabled(id)))],
            );
        }
        wanted
    }

    /// Bring the tree in line with `layout` and the values. Every pointer
    /// message does first; the hosts do before they paint.
    pub(crate) fn shell_sync(&mut self, layout: &LauncherOptionsLayout) {
        let wanted = self.shell_wanted(layout);
        let esc_menu = matches!(layout.surface, OptionsSurface::EscMenu(_));
        self.shell.sync(wanted, layout.scale, esc_menu);
    }

    /// A move: the tree updates hover, a held slider and the open list's hot
    /// row; the status line follows the hit test unless a press holds the
    /// mouse or a list is open.
    fn shell_pointer_moved(&mut self, x: i32, y: i32) {
        let held = self.shell.gui.pressed().is_some() || self.shell_list_open();
        self.shell.input(InputEvent::PointerMove { x, y });
        if !held {
            self.shell.status_help = self.shell.hovered_help();
        }
        if let Some(open) = self
            .shell
            .gui
            .popup()
            .and_then(|node| self.shell.gui.combo_drop_down(node))
        {
            self.shell.popup_hovered = open.popup.row_at(x, y);
        }
    }

    /// The caller routes all pointer messages here while this parent is topmost.
    pub(crate) fn shell_mouse_down(&mut self, x: i32, y: i32, layout: &LauncherOptionsLayout) {
        self.shell_sync(layout);
        let down = InputEvent::PointerDown {
            x,
            y,
            button: MouseButton::Left,
        };
        if self.shell_list_open() {
            // ComboDropWin 0x0060E4E9..0x0060E500 plays Rules+0x1A8
            // (GUIComboCloseSound) before selection or outside dismissal.
            self.shell.input(down);
            self.shell_events(layout);
            self.shell.status_help = None;
            self.shell.popup_hovered = None;
            return;
        }
        if self.shell.held_slider().is_some() {
            return;
        }
        self.shell_pointer_moved(x, y);
        self.shell.input(down);
        self.shell_events(layout);
    }

    pub(crate) fn shell_mouse_move(&mut self, x: i32, y: i32, layout: &LauncherOptionsLayout) {
        self.shell_sync(layout);
        self.shell_pointer_moved(x, y);
        self.shell_events(layout);
    }

    pub(crate) fn shell_mouse_up(&mut self, x: i32, y: i32, layout: &LauncherOptionsLayout) {
        self.shell_sync(layout);
        let up = InputEvent::PointerUp {
            x,
            y,
            button: MouseButton::Left,
        };
        // A slider lets go before the hit test runs again; a button's release
        // still sees its own capture.
        if self.shell.held_slider().is_some() {
            self.shell.input(up);
            self.shell_pointer_moved(x, y);
        } else {
            self.shell_pointer_moved(x, y);
            self.shell.input(up);
        }
        self.shell_events(layout);
    }

    /// What the tree's widgets did, in order, as the dialog's events.
    fn shell_events(&mut self, layout: &LauncherOptionsLayout) {
        for event in self.shell.gui.drain_events() {
            let cue = |cue| LauncherOptionsEvent::Cue(cue);
            match event {
                // gamemd-derived: ordinary `0xD5` owner-draw buttons emit
                // `[AudioVisual] GUIMainButtonSound` on admitted primary
                // mouse-down; `OptionsClass__ShowLauncherDialog @ 0x0055FC80`
                // observes the distinct Back/Keyboard result only on the later
                // activation release. Like gamemd's combo owner `0x00617250`,
                // a press on a list emits GUIComboOpen first.
                UiEvent::Pressed { node } => match self.shell.control(node) {
                    Some(Control::Button(_)) => self.pending_events.push(cue(PageCue::Button)),
                    Some(Control::List(_)) => self.pending_events.push(cue(PageCue::ComboOpen)),
                    _ => {}
                },
                UiEvent::ComboOpened { .. } | UiEvent::ComboClosed { .. } => {
                    let open = matches!(event, UiEvent::ComboOpened { .. });
                    self.pending_events.push(cue(if open {
                        PageCue::ComboOpen
                    } else {
                        PageCue::ComboClose
                    }));
                    self.shell.popup_hovered = None;
                }
                UiEvent::Changed { node, value, .. } => match (self.shell.control(node), value) {
                    (Some(Control::Check(id)), _) => self.toggle_checkbox(id),
                    (Some(Control::Slider(id)), Value::Int(value)) => {
                        if let Ok(position) = u8::try_from(value - id.value_base()) {
                            self.set_trackbar_position(id, position);
                        }
                    }
                    (Some(Control::List(combo)), Value::Int(row)) => {
                        let picks = self.shell_list(combo, layout).picks;
                        if let Some(pick) = usize::try_from(row)
                            .ok()
                            .and_then(|row| picks.into_iter().nth(row))
                        {
                            self.publish(pick);
                        }
                    }
                    _ => {}
                },
                UiEvent::Activated { node, .. } => {
                    if let Some(Control::Button(id)) = self.shell.control(node) {
                        self.activate_button(id);
                    }
                }
                _ => {}
            }
        }
    }

    /// A button's release over itself.
    fn activate_button(&mut self, id: LauncherButton) {
        let event = match id {
            LauncherButton::Exit(result) => return self.request_result(result),
            LauncherButton::Category(category) => return self.select_category(category),
            LauncherButton::InGameOptions => return self.select_category(Category::ALL[0]),
            LauncherButton::InGameBack => return self.select_category(Category::GameControls),
            LauncherButton::CheckForUpdates => return self.request_update_check(),
            LauncherButton::EditLayout => LauncherOptionsEvent::EditLayoutRequested,
            LauncherButton::SetWindowSize => LauncherOptionsEvent::SetWindowSizeRequested,
            LauncherButton::InGameKeyboard => LauncherOptionsEvent::InGameKeyboardRequested,
            LauncherButton::InGameSound => LauncherOptionsEvent::InGameSoundRequested,
        };
        self.pending_events.push(event);
    }
}

#[cfg(test)]
impl OptionsDialogState {
    pub(crate) fn shell_labels(
        &self,
        layout: &LauncherOptionsLayout,
    ) -> Vec<LauncherShellLabel<'_>> {
        let title = LauncherShellLabel {
            rect: layout.title,
            text: Cow::Borrowed(self.shell_title_text()),
        };
        std::iter::once(title)
            .chain(
                layout
                    .labels
                    .iter()
                    .map(|&(rect, label)| LauncherShellLabel {
                        rect,
                        text: self.page_label(layout, label).0,
                    }),
            )
            .collect()
    }

    pub(crate) fn shell_trackbar_thumb_left(
        &self,
        id: LauncherTrackbarId,
        window: RectPx,
        scale: ShellScale,
    ) -> i32 {
        crate::ui::shell::trackbar::thumb_left_at(
            i32::from(self.trackbar_position(id)),
            window,
            id.plaque_reserve(),
            i32::from(id.maximum()),
            scale,
        )
    }

    /// A list that needs no window: its rows don't depend on the layout.
    fn shell_list_anywhere(&self, combo: LauncherComboId) -> LauncherList {
        self.shell_list(
            combo,
            &LauncherOptionsLayout::new(800, 600, Category::Display),
        )
    }

    pub(crate) fn shell_selected_health_bars(&self) -> HealthBarMode {
        self.values.health_bars
    }

    pub(crate) fn shell_display_label(&self) -> String {
        self.shell_list_anywhere(LauncherComboId::Display).face
    }

    pub(crate) fn shell_display_rows(&self) -> Vec<String> {
        self.shell_list_anywhere(LauncherComboId::Display).rows
    }

    pub(crate) fn shell_language_label(&self) -> String {
        self.shell_list_anywhere(LauncherComboId::Language).face
    }

    pub(crate) fn shell_language_rows(&self) -> Vec<String> {
        self.shell_list_anywhere(LauncherComboId::Language).rows
    }

    /// The slider holding the mouse, and whether it follows the pointer.
    pub(crate) fn shell_trackbar_hold(
        &self,
    ) -> Option<crate::ui::shell::trackbar::TrackbarHold<LauncherTrackbarId>> {
        self.shell
            .held_slider()
            .map(|id| crate::ui::shell::trackbar::TrackbarHold {
                id,
                dragging: self.shell.gui.dragging(),
            })
    }

    /// The list that is open.
    pub(crate) fn shell_open_list(&self) -> Option<LauncherComboId> {
        match self
            .shell
            .gui
            .popup()
            .and_then(|node| self.shell.control(node))
        {
            Some(Control::List(combo)) => Some(combo),
            _ => None,
        }
    }

    /// The value the slider's node holds, which its plaque prints.
    pub(crate) fn shell_slider_value(&self, id: LauncherTrackbarId) -> Option<i32> {
        let node = self.shell.node(Control::Slider(id))?;
        match self.shell.gui.get(node, Prop::Pos) {
            Ok(Some(Value::Int(value))) => Some(*value),
            _ => None,
        }
    }
}

#[cfg(all(test, tests_ui))]
mod tests {
    use super::*;
    use crate::rules::house_colors::{STRENGTH_MAX, STRENGTH_RETAIL};
    use crate::ui::main_menu_dialogs::options::{
        COMMANDER_FEEDBACK_LABEL, CUSTOM_END_GAME_BANNER_LABEL, CUSTOM_END_GAME_MOVIE_LABEL,
        CUSTOM_END_GAME_SCORE_LABEL, DisplayChoice, DisplayTarget, FULL_SCREEN_HELP,
        LauncherOptionsEvent, LauncherOptionsLabels, LauncherOptionsValues, ORIGINAL_SHAPE_HELP,
        ORIGINAL_SHAPE_LABEL, PERFORMANCE_STATS_HELP, YR_ORIGINAL_STYLE_LABEL, ZOOM_HELP,
    };
    use crate::ui::shell::pause_menu::{
        PauseMenuButton, PauseMenuLayout, PauseMenuTemplate, pause_menu_layout,
    };
    use crate::ui::shell::trackbar::TrackbarHold;

    fn state() -> OptionsDialogState {
        OptionsDialogState::new(
            LauncherOptionsLabels::resolve(&|_| None),
            LauncherOptionsValues::default(),
            Zoom::Auto,
            true,
            vec![
                DisplayChoice {
                    id: "\\\\.\\DISPLAY1".to_owned(),
                    width: 1920,
                    height: 1080,
                    refresh_millihertz: Some(60_000),
                },
                DisplayChoice {
                    id: "\\\\.\\DISPLAY2".to_owned(),
                    width: 2560,
                    height: 1440,
                    refresh_millihertz: Some(144_000),
                },
            ],
            DisplayTarget::Auto,
        )
    }

    /// A category's page in the retail 800x600 window.
    fn page(category: Category) -> LauncherOptionsLayout {
        LauncherOptionsLayout::new(800, 600, category)
    }

    fn checkbox(layout: &LauncherOptionsLayout, id: LauncherCheckboxId) -> RectPx {
        layout
            .checkboxes
            .iter()
            .find(|(candidate, _)| *candidate == id)
            .map(|(_, rect)| *rect)
            .unwrap_or_else(|| panic!("{id:?} on the {:?} page", layout.category))
    }

    fn button(layout: &LauncherOptionsLayout, id: LauncherButton) -> RectPx {
        layout
            .buttons
            .iter()
            .find(|(candidate, _)| *candidate == id)
            .map(|(_, rect)| *rect)
            .unwrap_or_else(|| panic!("{id:?} on the {:?} page", layout.category))
    }

    /// The status help after the pointer moves to `(x, y)`.
    fn help_at(
        state: &mut OptionsDialogState,
        x: i32,
        y: i32,
        layout: &LauncherOptionsLayout,
    ) -> Option<LauncherStatusHelp> {
        state.shell_mouse_move(x, y, layout);
        state.shell_status_help()
    }

    fn disjoint(a: RectPx, b: RectPx) -> bool {
        a.x + a.w <= b.x || b.x + b.w <= a.x || a.y + a.h <= b.y || b.y + b.h <= a.y
    }

    /// Every slider a page lays out must land on one of the chrome atlas's
    /// authored trackbar windows. The frame lookup is an exact size match, so
    /// a row sized like a plain track but painted with a value plaque
    /// resolves to no frame at all: the page would draw the thumb and the
    /// plaque over empty background.
    #[test]
    fn every_page_slider_lands_on_an_authored_trackbar_frame() {
        use crate::render::skirmish_shell_chrome::trackbar_frame_slot;
        for category in Category::ALL {
            let layout = page(category);
            for &(id, rect) in &layout.trackbars {
                let base = layout.scale.base_rect(rect);
                assert!(
                    trackbar_frame_slot(id.plaque_reserve() > 0, base.w, base.h).is_some(),
                    "{id:?} on the {category:?} page is {}x{}, which no authored trackbar frame covers",
                    base.w,
                    base.h
                );
            }
        }
    }

    /// The chrome atlas keys combo faces on exact authored widths, so a combo
    /// laid out at any other width paints its arrow over no face at all.
    #[test]
    fn every_combo_face_uses_an_authored_width() {
        for category in Category::ALL {
            let layout = page(category);
            for &(id, rect) in &layout.combos {
                assert_eq!(
                    layout.scale.base_rect(rect).w,
                    180,
                    "{id:?} on the {category:?} page"
                );
            }
        }
    }

    /// The right-hand column: retail's Keyboard, a button per category on the
    /// tiles under it, VERA's Mods on the next, and Main Menu on the last of
    /// the 800x600 panel's nine tiles, each a 156x42 SDBTNANM cell flush with
    /// the panel's right edge.
    #[test]
    fn the_column_holds_keyboard_every_category_mods_and_main_menu() {
        let layout = page(Category::Display);
        let mut expected = vec![LauncherButton::Exit(LauncherParentResult::Keyboard)];
        expected.extend(Category::ALL.map(LauncherButton::Category));
        if HAS_MODS {
            expected.push(LauncherButton::Exit(LauncherParentResult::Mods));
        }
        expected.push(LauncherButton::Exit(LauncherParentResult::Back));
        let ids: Vec<_> = layout.buttons.iter().map(|(id, _)| *id).collect();
        assert_eq!(ids, expected);
        let tile = |row: i32| RectPx::new(644, 199 + 42 * row, 156, 42);
        let (main_menu, stacked) = layout.buttons.split_last().unwrap();
        for (row, (id, rect)) in stacked.iter().enumerate() {
            assert_eq!(*rect, tile(row as i32), "{id:?}");
        }
        assert_eq!(main_menu.1, tile(8));
    }

    /// 640x480's retail panel has six tiles, five above Main Menu: too few for
    /// the column, so the page takes the fitted 800x600 layout there, and
    /// every button keeps a tile of its own inside the window.
    #[test]
    fn a_window_too_short_for_the_column_takes_the_fitted_page() {
        assert!(retail_column_fits(800, 600));
        assert!(!retail_column_fits(640, 480));
        let layout = LauncherOptionsLayout::new(640, 480, Category::Display);
        assert_eq!(
            layout,
            LauncherOptionsLayout::responsive(640, 480, Category::Display)
        );
        assert_eq!(layout.buttons.len(), 8 + usize::from(HAS_MODS));
        for (index, (id, a)) in layout.buttons.iter().enumerate() {
            assert!(a.x + a.w <= 640 && a.y + a.h <= 480, "{id:?} at {a:?}");
            for (other, b) in &layout.buttons[index + 1..] {
                assert!(disjoint(*a, *b), "{id:?} overlaps {other:?}");
            }
        }
    }

    /// Every page's controls stay left of the column, above the status line
    /// and clear of one another, at the retail size and fitted ones.
    #[test]
    fn every_page_keeps_its_controls_clear_of_one_another_and_the_column() {
        for (width, height) in [(800, 600), (640, 480), (1920, 1080)] {
            for category in Category::ALL {
                let layout = if (width, height) == (1920, 1080) {
                    LauncherOptionsLayout::responsive(width, height, category)
                } else {
                    LauncherOptionsLayout::new(width, height, category)
                };
                let column = button(&layout, LauncherButton::Exit(LauncherParentResult::Back));
                let rects: Vec<RectPx> = layout
                    .checkboxes
                    .iter()
                    .map(|(_, rect)| *rect)
                    .chain(layout.combos.iter().map(|(_, rect)| *rect))
                    .chain(layout.trackbars.iter().map(|(_, rect)| *rect))
                    .chain(
                        layout
                            .buttons
                            .iter()
                            .filter(|(id, _)| *id == LauncherButton::CheckForUpdates)
                            .map(|(_, rect)| *rect),
                    )
                    .collect();
                for (index, a) in rects.iter().enumerate() {
                    assert!(
                        a.x >= 0 && a.x + a.w <= column.x && a.y + a.h <= layout.status_help.y,
                        "{category:?} at {width}x{height}: {a:?}"
                    );
                    for b in &rects[index + 1..] {
                        assert!(
                            disjoint(*a, *b),
                            "{category:?} at {width}x{height}: {a:?} overlaps {b:?}"
                        );
                    }
                }
            }
        }
    }

    /// The menu the in-game page goes with, as the app lays it out: B5, or
    /// FF for `internet`, with the column `category`'s page gives it.
    fn in_game_menu(
        (width, height): (i32, i32),
        category: Category,
        internet: bool,
    ) -> PauseMenuLayout {
        let mut menu = pause_menu_layout(
            width,
            height,
            in_game_shell::allied_fixture_layout(width, height),
            [125, 25],
            if internet {
                PauseMenuTemplate::Internet
            } else {
                PauseMenuTemplate::Offline
            },
        );
        if category == Category::GameControls {
            menu.add_options_button();
        } else {
            menu.show_options_column();
        }
        menu.raise_internet_group();
        menu
    }

    /// A page on the in-game menu in a shell of `size` pixels.
    fn in_game(size: (i32, i32), category: Category, internet: bool) -> LauncherOptionsLayout {
        LauncherOptionsLayout::in_game(
            size,
            in_game_menu(size, category, internet).page_column(),
            category,
            internet,
            true,
            true,
            (1920, 1080),
        )
    }

    /// A skirmish's Game Options page keeps the Commander Clips box; an
    /// Internet match's leaves it out, as no clip plays there.
    #[test]
    fn commander_clips_stay_off_an_internet_match_page() {
        let has_box = |internet| {
            in_game((800, 600), Category::Gameplay, internet)
                .checkboxes
                .iter()
                .any(|(id, _)| *id == LauncherCheckboxId::CommanderFeedback)
        };
        assert!(has_box(false));
        assert!(!has_box(true));
    }

    /// Every in-game page fits left of the sidebar and above the status line
    /// at every shell size, down to the 640x480 minimum, clear of FF's group
    /// and with authored art for each slider and list.
    #[test]
    fn every_in_game_page_fits_beside_the_sidebar() {
        use crate::render::skirmish_shell_chrome::trackbar_frame_slot;
        const SIDEBAR_W: i32 = 168;
        for (size, internet) in [
            ((640, 480), false),
            ((640, 480), true),
            ((1920, 1080), true),
        ] {
            for category in [Category::GameControls].into_iter().chain(Category::ALL) {
                let menu = in_game_menu(size, category, internet);
                let layout = in_game(size, category, internet);
                for &(id, rect) in &layout.trackbars {
                    assert!(
                        trackbar_frame_slot(id.plaque_reserve() > 0, rect.w, rect.h).is_some(),
                        "{id:?} on {category:?} at {size:?} is {}x{}",
                        rect.w,
                        rect.h
                    );
                }
                for &(id, rect) in &layout.combos {
                    assert_eq!(rect.w, 180, "{id:?} on {category:?}");
                }
                let rects: Vec<RectPx> = layout
                    .checkboxes
                    .iter()
                    .map(|(_, rect)| *rect)
                    .chain(layout.combos.iter().map(|(_, rect)| *rect))
                    .chain(layout.trackbars.iter().map(|(_, rect)| *rect))
                    .chain(
                        layout
                            .buttons
                            .iter()
                            .filter(|(id, _)| id.on_page())
                            .map(|(_, rect)| *rect),
                    )
                    .chain(
                        menu.internet
                            .filter(|_| category == Category::Gameplay)
                            .map(|internet| internet.group),
                    )
                    .collect();
                for (index, a) in rects.iter().enumerate() {
                    assert!(
                        a.x >= 0
                            && a.y >= 0
                            && a.x + a.w <= size.0 - SIDEBAR_W
                            && a.y + a.h <= layout.status_help.y,
                        "{category:?} at {size:?}: {a:?}"
                    );
                    for b in &rects[index + 1..] {
                        assert!(
                            disjoint(*a, *b),
                            "{category:?} at {size:?}: {a:?} overlaps {b:?}"
                        );
                    }
                }
            }
        }
    }

    /// Game Controls has Options in the sidebar row under it. Options gives
    /// the column to the main menu's: Keyboard in Game Controls' row, a
    /// button per category under it and Back in Resume's place, with the
    /// menu's own buttons gone. Back shows Game Controls again.
    #[test]
    fn in_game_options_takes_the_column_and_back_returns_it() {
        let size = (800, 600);
        let landing = in_game(size, Category::GameControls, false);
        let native = in_game_menu(size, Category::GameControls, false);
        let controls = native.button(PauseMenuButton::GameControls).unwrap();
        let resume = native.button(PauseMenuButton::Resume).unwrap();
        let below = |rows: i32| controls.translate(0, rows * controls.h);
        assert_eq!(
            landing.buttons[0],
            (LauncherButton::InGameOptions, below(1))
        );
        assert_eq!(native.button(PauseMenuButton::Keyboard), Some(below(2)));
        assert_eq!(native.button(PauseMenuButton::Load), Some(below(3)));

        let mut dialog = state();
        dialog.select_category(Category::GameControls);
        let options = below(1);
        assert_eq!(
            help_at(&mut dialog, options.x + 2, options.y + 2, &landing),
            Some(LauncherStatusHelp::Vera(IN_GAME_OPTIONS_HELP))
        );
        dialog.shell_mouse_down(options.x + 5, options.y + 5, &landing);
        dialog.shell_mouse_up(options.x + 5, options.y + 5, &landing);
        assert_eq!(dialog.category(), Category::ALL[0]);

        let column = in_game_menu(size, Category::Display, false);
        assert_eq!(column.buttons().count(), 0, "the menu's own buttons hide");
        let page = in_game(size, Category::Display, false);
        let mut expected = vec![(
            LauncherButton::Exit(LauncherParentResult::Keyboard),
            controls,
        )];
        expected.extend(
            Category::ALL
                .into_iter()
                .enumerate()
                .map(|(row, category)| (LauncherButton::Category(category), below(row as i32 + 1))),
        );
        expected.push((LauncherButton::InGameBack, resume));
        assert_eq!(page.buttons, expected);

        let general = LauncherButton::Category(Category::General);
        let rect = button(&page, general);
        assert_eq!(
            help_at(&mut dialog, rect.x + 2, rect.y + 2, &page),
            Some(LauncherStatusHelp::Vera("VERA:GeneralPageHelp"))
        );
        dialog.shell_mouse_down(rect.x + 5, rect.y + 5, &page);
        dialog.shell_mouse_up(rect.x + 5, rect.y + 5, &page);
        assert!(dialog.shell_button_selected(general));
        let page = in_game(size, Category::General, false);
        dialog.shell_mouse_down(resume.x + 5, resume.y + 5, &page);
        dialog.shell_mouse_up(resume.x + 5, resume.y + 5, &page);
        assert_eq!(dialog.category(), Category::GameControls);
        assert_eq!(dialog.drain_output().result, None);
    }

    /// Game Controls, the page the menu opens on, holds gamemd's Game
    /// Controls settings with the three volumes, then the window-size,
    /// Keyboard and Sound buttons. A volume drag is heard at once, a moved
    /// Game Speed waits for the menu to close, and BBB's init can hide it.
    #[test]
    fn the_in_game_landing_page_is_game_controls_with_volume_and_its_buttons() {
        use LauncherTrackbarId as Slider;
        let layout = in_game((800, 600), Category::GameControls, false);
        let sliders = |layout: &LauncherOptionsLayout| {
            layout
                .trackbars
                .iter()
                .map(|(id, _)| *id)
                .collect::<Vec<_>>()
        };
        assert_eq!(
            sliders(&layout),
            [
                Slider::GameSpeed,
                Slider::Scroll,
                Slider::Score,
                Slider::Sound,
                Slider::Voice
            ]
        );
        let hidden = LauncherOptionsLayout::in_game(
            (800, 600),
            in_game_menu((800, 600), Category::GameControls, false).page_column(),
            Category::GameControls,
            false,
            false,
            false,
            (1920, 1080),
        );
        assert_eq!(sliders(&hidden)[0], Slider::Scroll);
        assert!(
            !hidden
                .buttons
                .iter()
                .any(|(id, _)| *id == LauncherButton::InGameKeyboard),
            "the sidebar has Keyboard"
        );

        let mut dialog = state();
        dialog.set_in_game_controls(
            "Game Speed".to_owned(),
            (0..8).map(|speed| format!("speed {speed}")).collect(),
            3,
            "Sound".to_owned(),
        );
        dialog.select_category(Category::GameControls);
        assert!(!dialog.shell_button_selected(LauncherButton::Category(Category::Gameplay)));
        let packed = dialog.pack();
        let drag_to_end = |dialog: &mut OptionsDialogState, id| {
            let rect = layout.trackbar_rect(id).unwrap();
            let thumb = dialog.shell_trackbar_thumb_left(id, rect, layout.scale);
            dialog.shell_mouse_down(rect.x + thumb + 5, rect.y + rect.h - 3, &layout);
            dialog.shell_mouse_move(rect.x + rect.w + 200, rect.y + rect.h - 3, &layout);
            dialog.shell_mouse_up(rect.x + rect.w + 200, rect.y + rect.h - 3, &layout);
            dialog.drain_output().events
        };
        assert_eq!(
            drag_to_end(&mut dialog, Slider::Sound),
            [LauncherOptionsEvent::SoundPreview(1.0)]
        );
        let speed_events = drag_to_end(&mut dialog, Slider::GameSpeed);
        assert_eq!(dialog.trackbar_position(Slider::GameSpeed), 7);
        assert!(
            speed_events
                .iter()
                .all(|event| *event == LauncherOptionsEvent::Cue(PageCue::ListClick))
        );
        assert_eq!(dialog.slider_step(Slider::GameSpeed), "speed 7");
        assert_eq!(
            crate::ui::main_menu_dialogs::options::LauncherOptionsPacked {
                sound_volume: packed.sound_volume,
                ..dialog.pack()
            },
            packed,
            "the profile pack has no game speed"
        );

        for (button, event) in [
            (
                LauncherButton::InGameKeyboard,
                LauncherOptionsEvent::InGameKeyboardRequested,
            ),
            (
                LauncherButton::InGameSound,
                LauncherOptionsEvent::InGameSoundRequested,
            ),
        ] {
            let rect = self::button(&layout, button);
            dialog.shell_mouse_down(rect.x + 5, rect.y + 5, &layout);
            dialog.shell_mouse_up(rect.x + 5, rect.y + 5, &layout);
            assert!(dialog.drain_output().events.contains(&event), "{button:?}");
        }
        assert_eq!(
            dialog.shell_button_label(LauncherButton::InGameSound),
            "Sound"
        );
        assert_eq!(
            layout
                .buttons
                .iter()
                .any(|(id, _)| *id == LauncherButton::SetWindowSize),
            HAS_SET_WINDOW_SIZE
        );
    }

    #[test]
    fn a_stretched_slider_maps_the_pointer_back_to_its_authored_track() {
        let mut dialog = state();
        let base = page(Category::Audio);
        let scale = ShellScale::from_screen(1600, 1200);
        let wide = LauncherOptionsLayout::responsive(1600, 1200, Category::Audio);
        let authored = base.trackbar_rect(LauncherTrackbarId::Score).unwrap();
        let stretched = wide.trackbar_rect(LauncherTrackbarId::Score).unwrap();
        assert_eq!(stretched, scale.scale_rect(authored));
        // The thumb travels by the same authored span, then stretches.
        let thumb = dialog.shell_trackbar_thumb_left(
            LauncherTrackbarId::Score,
            authored,
            ShellScale::RETAIL,
        );
        let wide_thumb =
            dialog.shell_trackbar_thumb_left(LauncherTrackbarId::Score, stretched, scale);
        assert_eq!(wide_thumb, (thumb as f32 * scale.factor()).round() as i32);
        // The same authored point addressed on either page takes the same
        // thumb and the same position.
        let press = (authored.x + thumb + 5, authored.y + authored.h - 3);
        dialog.shell_mouse_down(press.0, press.1, &base);
        let held = Some(TrackbarHold {
            id: LauncherTrackbarId::Score,
            dragging: true,
        });
        assert_eq!(dialog.shell_trackbar_hold(), held);
        let pressed_position = dialog.trackbar_position(LauncherTrackbarId::Score);
        dialog.shell_mouse_up(press.0, press.1, &base);
        let device = scale.scale_rect(RectPx::new(press.0, press.1, 1, 1));
        let mut stretched_dialog = state();
        stretched_dialog.shell_mouse_down(device.x, device.y, &wide);
        assert_eq!(
            stretched_dialog.shell_trackbar_hold(),
            held,
            "{device:?} over {stretched:?}"
        );
        assert_eq!(
            stretched_dialog.trackbar_position(LauncherTrackbarId::Score),
            pressed_position
        );
        // A jump press beside the thumb quantizes on the authored width.
        let jump = (authored.x + authored.w - 2, authored.y + authored.h - 3);
        dialog.shell_mouse_down(jump.0, jump.1, &base);
        let jumped = dialog.trackbar_position(LauncherTrackbarId::Score);
        dialog.shell_mouse_up(jump.0, jump.1, &base);
        let device = scale.scale_rect(RectPx::new(jump.0, jump.1, 1, 1));
        let mut stretched_dialog = state();
        stretched_dialog.shell_mouse_down(device.x, device.y, &wide);
        assert_eq!(
            stretched_dialog.trackbar_position(LauncherTrackbarId::Score),
            jumped,
            "{device:?} over {stretched:?}"
        );
    }

    /// A category button shows its page in place, applying nothing; the
    /// page's heading is retail's band name. Main Menu closes from any page.
    #[test]
    fn pressing_a_category_shows_its_page_in_place_and_main_menu_closes() {
        let before = state().pack();
        let mut state = state();
        assert_eq!(state.category(), Category::Display);
        assert!(state.shell_button_selected(LauncherButton::Category(Category::Display)));
        let display = page(Category::Display);
        let audio = button(&display, LauncherButton::Category(Category::Audio));
        state.shell_mouse_down(audio.x + 5, audio.y + 5, &display);
        assert!(state.shell_button_pressed(LauncherButton::Category(Category::Audio)));
        state.shell_mouse_up(audio.x + 5, audio.y + 5, &display);
        assert_eq!(state.category(), Category::Audio);
        assert!(state.shell_button_selected(LauncherButton::Category(Category::Audio)));
        assert!(!state.shell_button_selected(LauncherButton::Category(Category::Display)));
        let output = state.drain_output();
        assert_eq!(output.events, [LauncherOptionsEvent::Cue(PageCue::Button)]);
        assert_eq!((output.result, output.check_for_updates), (None, false));
        // The pointer still rests on the button: its help stays up.
        assert_eq!(
            state.shell_status_help(),
            Some(LauncherStatusHelp::Vera("VERA:AudioPageHelp"))
        );
        let audio_page = page(Category::Audio);
        let labels = state.shell_labels(&audio_page);
        assert_eq!(
            (&*labels[0].text, &*labels[1].text),
            ("Options", "Audio Options")
        );
        let back = button(
            &audio_page,
            LauncherButton::Exit(LauncherParentResult::Back),
        );
        state.shell_mouse_down(back.x + 5, back.y + 5, &audio_page);
        state.shell_mouse_up(back.x + 5, back.y + 5, &audio_page);
        assert_eq!(
            state.drain_output().result,
            Some(LauncherParentResult::Back)
        );
        assert_eq!(state.pack(), before);
    }

    /// Switching pages closes an open list: the next page starts clean.
    #[test]
    fn switching_category_drops_an_open_list() {
        let mut state = state();
        let layout = page(Category::Display);
        let zoom = layout.combo_rect(LauncherComboId::Zoom).unwrap();
        state.shell_mouse_down(zoom.x + 10, zoom.y + 5, &layout);
        assert_eq!(state.shell_open_list(), Some(LauncherComboId::Zoom));
        state.select_category(Category::Audio);
        assert_eq!(state.shell_open_list(), None);
        assert!(state.shell_popup(&page(Category::Audio)).is_none());
    }

    /// The Language list on the General page offers each language by its
    /// own name and publishes a new pick at once.
    #[test]
    fn the_language_list_publishes_a_new_pick_by_tag() {
        use crate::i18n::LanguageChoice;
        let mut state = state();
        let choice = |tag: &str, name: &str| LanguageChoice {
            tag: tag.to_owned(),
            name: name.to_owned(),
        };
        state.set_languages(
            vec![choice("en", "English"), choice("zh-CN", "简体中文")],
            "en".to_owned(),
        );
        state.select_category(Category::General);
        let layout = page(Category::General);
        let face = layout.combo_rect(LauncherComboId::Language).unwrap();
        state.shell_mouse_down(face.x + 10, face.y + 5, &layout);
        let popup = state.shell_popup(&layout).unwrap();
        assert_eq!(
            (popup.combo, popup.rows, popup.selected),
            (LauncherComboId::Language, 2, 0)
        );
        assert_eq!(state.shell_language_rows(), ["English", "简体中文"]);
        // The language in use changes nothing; another publishes its tag.
        let first = popup.row_rect(0);
        state.shell_mouse_down(first.x + 10, first.y + 5, &layout);
        let (open, close) = (
            LauncherOptionsEvent::Cue(PageCue::ComboOpen),
            LauncherOptionsEvent::Cue(PageCue::ComboClose),
        );
        assert_eq!(state.drain_output().events, [open.clone(), close.clone()]);
        state.shell_mouse_down(face.x + 10, face.y + 5, &layout);
        let second = popup.row_rect(1);
        state.shell_mouse_down(second.x + 10, second.y + 5, &layout);
        assert_eq!(state.shell_language_label(), "简体中文");
        assert_eq!(
            state.drain_output().events,
            [
                open,
                close,
                LauncherOptionsEvent::LanguageSelected("zh-CN".to_owned()),
            ]
        );
    }

    #[test]
    fn esc_menu_controls_follow_the_widest_visible_caption() {
        let stats = tr(PERFORMANCE_STATS_LABEL);
        for width in [94, 166] {
            let layout = LauncherOptionsLayout::esc_menu(800, 600, ShellScale::RETAIL, |caption| {
                if caption == stats { width } else { 40 }
            });
            let face = layout.combo_rect(LauncherComboId::Language).unwrap();
            let gap = face.x - (layout.labels[0].0.x + width as i32);
            assert!(
                (12..=22).contains(&gap),
                "gap after the longest caption is {gap}px"
            );
            let lamp = checkbox(&layout, LauncherCheckboxId::FullScreen);
            assert!(help_at(&mut state(), face.x + face.w + 1, lamp.y + 2, &layout).is_none());
        }
    }

    /// The Esc menu's settings share two columns. Opening even the full
    /// language list leaves its three action buttons visible.
    #[test]
    fn esc_menu_settings_stay_clear_of_the_box_buttons() {
        use crate::i18n::LanguageChoice;
        use crate::ui::shell::modal::esc_menu_box_layout_on;
        let mut state = state();
        let languages = (0..11)
            .map(|i| LanguageChoice {
                tag: i.to_string(),
                name: i.to_string(),
            })
            .collect();
        state.set_languages(languages, "0".to_owned());
        for (w, h) in [(640, 480), (1920, 1080)] {
            let scale = ShellScale::for_window(w, h);
            let layout = LauncherOptionsLayout::esc_menu(w, h, scale, |_| 100);
            let message_box = esc_menu_box_layout_on(w, h, scale);
            let face = layout.combo_rect(LauncherComboId::Language).unwrap();
            let caption = |kind| layout.labels.iter().find(|(_, id)| *id == kind).unwrap().0;
            let language = caption(PageLabel::Localized(LANGUAGE_LABEL));
            assert!(language.x + language.w <= face.x);
            assert!(language.y < face.y + face.h && face.y < language.y + language.h);
            for (id, rect) in &layout.checkboxes {
                let title = state.shell_checkbox_label(*id);
                assert!(state.shell_labels(&layout).iter().any(|label| {
                    label.text == title
                        && label.rect.x == language.x
                        && label.rect.y < rect.y + rect.h
                        && rect.y < label.rect.y + label.rect.h
                }));
                assert_eq!(rect.x, face.x);
            }
            if HAS_SET_WINDOW_SIZE {
                let size = button(&layout, LauncherButton::SetWindowSize);
                let title = caption(PageLabel::Localized(SET_WINDOW_SIZE_LABEL));
                assert_eq!((title.x, size.x), (language.x, face.x));
                assert!(title.y < size.y + size.h && size.y < title.y + title.h);
                assert!(size.y + size.h < message_box.ok.y);
            }
            state.shell_mouse_down(face.x + 10, face.y + 5, &layout);
            let list = state.shell_popup(&layout).unwrap().rect;
            assert_eq!((list.x, list.w), (face.x, face.w));
            assert!(list.y >= message_box.dialog.y && list.y + list.h < message_box.ok.y);
            assert!(state.shell_close_list());
            assert!(!state.shell_close_list());
            assert_eq!(
                state.drain_output().events,
                [
                    LauncherOptionsEvent::Cue(PageCue::ComboOpen),
                    LauncherOptionsEvent::Cue(PageCue::ComboClose),
                ]
            );
        }
    }

    /// Like the page's other buttons, Set Window Size acts on the release over
    /// it, and a press dragged off it does nothing.
    #[test]
    fn esc_menu_set_window_size_asks_on_release_over_the_button() {
        if !HAS_SET_WINDOW_SIZE {
            return;
        }
        let mut state = state();
        let layout = LauncherOptionsLayout::esc_menu(800, 600, ShellScale::RETAIL, |_| 100);
        let rect = button(&layout, LauncherButton::SetWindowSize);
        let (x, y) = (rect.x + 2, rect.y + 2);
        state.shell_mouse_down(x, y, &layout);
        assert_eq!(
            state.drain_output().events,
            [LauncherOptionsEvent::Cue(PageCue::Button)]
        );
        state.shell_mouse_up(x, y - 100, &layout);
        assert!(state.drain_output().events.is_empty());
        state.shell_mouse_down(x, y, &layout);
        state.shell_mouse_up(x, y, &layout);
        assert_eq!(
            state.drain_output().events,
            [
                LauncherOptionsEvent::Cue(PageCue::Button),
                LauncherOptionsEvent::SetWindowSizeRequested,
            ]
        );
    }

    /// The button names the size it sets, the one this launch opened at.
    #[test]
    fn esc_menu_set_window_size_names_the_launch_size() {
        let mut state = state();
        state.set_launch_window_size((640, 480));
        assert_eq!(
            state.shell_button_label(LauncherButton::SetWindowSize),
            "640x480"
        );
    }

    #[test]
    fn esc_menu_checkboxes_publish_what_the_options_page_does() {
        let mut state = state();
        let layout = LauncherOptionsLayout::esc_menu(800, 600, ShellScale::RETAIL, |_| 100);
        let event = |id, checked| match id {
            LauncherCheckboxId::FullScreen => LauncherOptionsEvent::FullScreenChanged(checked),
            _ => LauncherOptionsEvent::PerformanceStatsChanged(checked),
        };
        for id in [
            LauncherCheckboxId::FullScreen,
            LauncherCheckboxId::PerformanceStats,
        ] {
            let rect = checkbox(&layout, id);
            let title = state
                .shell_labels(&layout)
                .into_iter()
                .find(|label| label.text == state.shell_checkbox_label(id))
                .unwrap()
                .rect;
            for (x, y) in [
                (rect.x + 2, rect.y + 2),
                (title.x + 2, title.y + 2),
                (rect.x + 30, rect.y + 2),
            ] {
                let checked = !state.shell_checkbox_checked(id);
                state.shell_mouse_down(x, y, &layout);
                assert!(
                    state.drain_output().events.contains(&event(id, checked)),
                    "{id:?}"
                );
                assert_eq!(
                    state.shell_checkbox_face_text(&layout, id),
                    if checked { "Yes" } else { "No" }
                );
                state.shell_mouse_up(x, y, &layout);
            }
        }
    }

    #[test]
    fn status_help_names_the_control_under_the_pointer() {
        let csf = |key| Some(LauncherStatusHelp::Csf(key));
        let text = |text| Some(LauncherStatusHelp::Vera(text));
        let mut dialog = state();
        let layout = page(Category::Display);
        let mut at = |rect: RectPx| help_at(&mut dialog, rect.x + 2, rect.y + 2, &layout);
        for (result, key) in [
            (LauncherParentResult::Keyboard, "STT:MainOptButtonKeyboard"),
            (LauncherParentResult::Back, "STT:MainOptButtonBack"),
        ] {
            assert_eq!(
                at(button(&layout, LauncherButton::Exit(result))),
                csf(key),
                "{result:?}"
            );
        }
        for category in Category::ALL {
            assert_eq!(
                at(button(&layout, LauncherButton::Category(category))),
                text(category.help().expect("every page in the column has help"))
            );
        }
        assert_eq!(at(RectPx::new(3, 3, 1, 1)), None);
        assert_eq!(
            at(layout.combo_rect(LauncherComboId::Zoom).unwrap()),
            text(ZOOM_HELP)
        );
        use LauncherCheckboxId::*;
        for (id, help) in [
            (FullScreen, FULL_SCREEN_HELP),
            (OriginalShape, ORIGINAL_SHAPE_HELP),
            (PerformanceStats, PERFORMANCE_STATS_HELP),
        ] {
            assert_eq!(at(checkbox(&layout, id)), text(help), "{id:?}");
        }
    }

    /// VERA's Mods button is a column button on every page: a press asks for
    /// the Mods page as Keyboard's asks for the key page, and its caption and
    /// status help are VERA's own texts.
    #[test]
    fn the_mods_button_asks_for_the_mods_page() {
        if !HAS_MODS {
            return;
        }
        let mods = LauncherButton::Exit(LauncherParentResult::Mods);
        let mut state = state();
        assert!(!mods.on_page());
        for category in Category::ALL {
            assert_eq!(
                button(&page(category), mods),
                RectPx::new(644, 199 + 42 * 7, 156, 42),
                "{category:?}"
            );
            // A match's menu has no way to the Mods page, whose choice
            // restarts the game.
            assert!(
                !in_game((800, 600), category, false)
                    .buttons
                    .iter()
                    .any(|(id, _)| *id == mods),
                "{category:?}"
            );
        }
        let layout = page(Category::Display);
        let rect = button(&layout, mods);
        assert_eq!(state.shell_button_label(mods), "Mods");
        assert_eq!(
            help_at(&mut state, rect.x + 2, rect.y + 2, &layout),
            Some(LauncherStatusHelp::Vera("VERA:ModsHelp"))
        );
        state.shell_mouse_down(rect.x + 5, rect.y + 5, &layout);
        state.shell_mouse_up(rect.x + 5, rect.y + 5, &layout);
        let output = state.drain_output();
        assert_eq!(
            (output.result, output.check_for_updates),
            (Some(LauncherParentResult::Mods), false)
        );
    }

    #[test]
    fn check_for_updates_sits_on_the_general_page_and_only_asks() {
        for category in Category::ALL {
            assert_eq!(
                page(category)
                    .buttons
                    .iter()
                    .any(|(id, _)| *id == LauncherButton::CheckForUpdates),
                HAS_CHECK_FOR_UPDATES && category == Category::General,
                "{category:?}"
            );
        }
        if !HAS_CHECK_FOR_UPDATES {
            return;
        }
        let mut state = state();
        state.select_category(Category::General);
        let layout = page(Category::General);
        let check = button(&layout, LauncherButton::CheckForUpdates);
        let last_box = layout.checkboxes.last().unwrap().1;
        assert!(
            check.y >= last_box.y + last_box.h,
            "{check:?} under {last_box:?}"
        );
        assert_eq!(
            state.shell_button_label(LauncherButton::CheckForUpdates),
            "Check for Updates"
        );
        let before = state.pack();
        state.shell_mouse_down(check.x + 5, check.y + 5, &layout);
        state.shell_mouse_up(check.x + 5, check.y + 5, &layout);
        let output = state.drain_output();
        assert_eq!((output.result, output.check_for_updates), (None, true));
        assert_eq!(state.pack(), before);

        // The in-game menu shows it greyed, saying why, and a press asks
        // nothing.
        state.set_in_game_controls(String::new(), Vec::new(), 0, String::new());
        let layout = in_game((800, 600), Category::General, false);
        let check = button(&layout, LauncherButton::CheckForUpdates);
        assert_eq!(
            help_at(&mut state, check.x + 5, check.y + 5, &layout),
            Some(LauncherStatusHelp::Vera(NOT_IN_MATCH_HELP))
        );
        state.shell_mouse_down(check.x + 5, check.y + 5, &layout);
        state.shell_mouse_up(check.x + 5, check.y + 5, &layout);
        assert!(!state.drain_output().check_for_updates);
    }

    /// Edit Layout sits under the Interface page's rows in both menus. The
    /// main menu has no match screen to arrange, so a press there asks
    /// nothing; in a match it asks for the layout edit mode, which the
    /// in-game menu starts as it closes.
    #[test]
    fn edit_layout_sits_on_the_interface_page_and_asks_in_a_match() {
        let edit = LauncherButton::EditLayout;
        for category in Category::ALL {
            assert_eq!(
                page(category).buttons.iter().any(|(id, _)| *id == edit),
                category == Category::Interface,
                "{category:?}"
            );
        }
        let press = |state: &mut OptionsDialogState, layout: &LauncherOptionsLayout| {
            let rect = button(layout, edit);
            state.shell_mouse_down(rect.x + 5, rect.y + 5, layout);
            state.shell_mouse_up(rect.x + 5, rect.y + 5, layout);
            state
                .drain_output()
                .events
                .contains(&LauncherOptionsEvent::EditLayoutRequested)
        };
        let mut state = state();
        let layout = page(Category::Interface);
        let rect = button(&layout, edit);
        let last_box = layout.checkboxes.last().unwrap().1;
        assert!(
            rect.y >= last_box.y + last_box.h,
            "{rect:?} under {last_box:?}"
        );
        assert_eq!(state.shell_button_label(edit), "Edit Layout");
        assert!(!press(&mut state, &layout), "the main menu has no match");

        state.set_in_game_controls(String::new(), Vec::new(), 0, String::new());
        let layout = in_game((800, 600), Category::Interface, false);
        let rect = button(&layout, edit);
        assert_eq!(
            help_at(&mut state, rect.x + 5, rect.y + 5, &layout),
            Some(LauncherStatusHelp::Vera(EDIT_LAYOUT_HELP))
        );
        assert!(press(&mut state, &layout));
    }

    #[test]
    fn physical_slider_capture_crosses_other_controls_without_activating_them() {
        let mut state = state();
        let layout = page(Category::Audio);
        let rect = layout.trackbar_rect(LauncherTrackbarId::Score).unwrap();
        let thumb = state.shell_trackbar_thumb_left(LauncherTrackbarId::Score, rect, layout.scale);
        state.shell_mouse_down(rect.x + thumb + 5, rect.y + 10, &layout);
        assert_eq!(
            state.shell_trackbar_hold(),
            Some(TrackbarHold {
                id: LauncherTrackbarId::Score,
                dragging: true,
            })
        );
        state.shell_mouse_move(799, 550, &layout);
        state.shell_mouse_up(799, 550, &layout);
        assert_eq!(state.trackbar_position(LauncherTrackbarId::Score), 10);
        assert_eq!(state.shell_trackbar_hold(), None);
        let output = state.drain_output();
        assert_eq!(output.events, [LauncherOptionsEvent::ScorePreview(1.0)]);
        assert_eq!(output.result, None);
    }

    #[test]
    fn lost_focus_cancels_drag_before_pointer_reentry() {
        let mut state = state();
        let layout = page(Category::Audio);
        // Press inside the Score slider's track to begin a drag.
        let rect = layout.trackbar_rect(LauncherTrackbarId::Score).unwrap();
        let thumb = state.shell_trackbar_thumb_left(LauncherTrackbarId::Score, rect, layout.scale);
        state.shell_mouse_down(rect.x + thumb + 5, rect.y + rect.h / 2, &layout);
        assert_eq!(
            state.shell_trackbar_hold(),
            Some(TrackbarHold {
                id: LauncherTrackbarId::Score,
                dragging: true,
            })
        );
        state.shell_cancel_pointer_gesture();
        let before = state.pack();
        state.shell_mouse_move(799, 550, &layout);
        state.shell_mouse_up(799, 550, &layout);
        assert_eq!(state.pack(), before);
        assert!(state.drain_output().events.is_empty());
    }

    #[test]
    fn every_slider_press_holds_the_mouse_until_release() {
        let mut state = state();
        let gameplay = page(Category::Gameplay);
        let srect = gameplay
            .trackbar_rect(LauncherTrackbarId::Difficulty)
            .unwrap();
        let held = Some(TrackbarHold {
            id: LauncherTrackbarId::Difficulty,
            dragging: false,
        });
        // Above the admitted strip (the window's bottom 18 rows) the press
        // only takes the capture.
        state.shell_mouse_down(srect.x + 2, srect.y + 2, &gameplay);
        assert_eq!(
            state.trackbar_position(LauncherTrackbarId::Difficulty),
            1,
            "native lower strip rejects y=2"
        );
        assert_eq!(state.shell_trackbar_hold(), held);
        state.shell_mouse_up(srect.x + 2, srect.y + 2, &gameplay);
        // Beside the thumb it jumps once and holds without dragging.
        state.shell_mouse_down(srect.x + 2, srect.y + 10, &gameplay);
        assert_eq!(state.trackbar_position(LauncherTrackbarId::Difficulty), 0);
        assert_eq!(state.shell_trackbar_hold(), held);
        assert_eq!(
            state.drain_output().events,
            [LauncherOptionsEvent::Cue(PageCue::ListClick)]
        );
        state.shell_mouse_move(srect.x + 2, srect.y + 10, &gameplay);
        assert_eq!(
            state.shell_status_help(),
            Some(LauncherStatusHelp::Csf("STT:MainOptSliderDifficulty"))
        );
        state.shell_mouse_up(srect.x + srect.w - 2, srect.y + srect.h - 3, &gameplay);
        assert_eq!(state.shell_trackbar_hold(), None);
        // Main Menu's status help reads after hovering it.
        let back = button(&gameplay, LauncherButton::Exit(LauncherParentResult::Back));
        state.shell_mouse_move(back.x + 5, back.y + 5, &gameplay);
        assert_eq!(
            state.shell_status_help(),
            Some(LauncherStatusHelp::Csf("STT:MainOptButtonBack"))
        );
        assert_eq!(state.drain_output().result, None);
        // The Tooltips box: its caption takes no press; its 18x18 icon
        // toggles, and the box starts ticked.
        let interface = page(Category::Interface);
        let crect = checkbox(&interface, LauncherCheckboxId::Tooltips);
        state.shell_mouse_down(crect.x + 30, crect.y + 5, &interface);
        assert!(state.shell_checkbox_checked(LauncherCheckboxId::Tooltips));
        state.shell_mouse_down(crect.x + 5, crect.y + 5, &interface);
        assert!(!state.shell_checkbox_checked(LauncherCheckboxId::Tooltips));
        assert_eq!(
            state.drain_output().events,
            [LauncherOptionsEvent::Cue(PageCue::Checkbox)]
        );
    }

    #[test]
    fn button_release_requires_the_pressed_button_and_popup_dismissal_consumes_click() {
        let layout = page(Category::Display);
        let mut state = state();
        // A press lights the button; a release off it does nothing.
        let audio = button(&layout, LauncherButton::Category(Category::Audio));
        state.shell_mouse_down(audio.x + 5, audio.y + 5, &layout);
        assert!(state.shell_button_pressed(LauncherButton::Category(Category::Audio)));
        state.shell_mouse_up(audio.x - 200, audio.y + 5, &layout);
        assert_eq!(state.category(), Category::Display);
        assert_eq!(state.drain_output().result, None);
        // With a list open, a click elsewhere only closes it.
        let zoom = layout.combo_rect(LauncherComboId::Zoom).unwrap();
        state.shell_mouse_down(zoom.x + 10, zoom.y + 5, &layout);
        assert_eq!(state.shell_open_list(), Some(LauncherComboId::Zoom));
        let back = button(&layout, LauncherButton::Exit(LauncherParentResult::Back));
        state.shell_mouse_down(back.x + 5, back.y + 5, &layout);
        state.shell_mouse_up(back.x + 5, back.y + 5, &layout);
        assert_eq!(state.shell_open_list(), None);
        let dismissed = state.drain_output();
        assert_eq!(dismissed.result, None);
        assert_eq!(
            dismissed.events,
            [
                LauncherOptionsEvent::Cue(PageCue::ComboOpen),
                LauncherOptionsEvent::Cue(PageCue::ComboClose)
            ]
        );
        state.shell_mouse_down(back.x + 5, back.y + 5, &layout);
        state.shell_mouse_up(back.x + 5, back.y + 5, &layout);
        assert_eq!(
            state.drain_output().result,
            Some(LauncherParentResult::Back)
        );
    }

    #[test]
    fn zoom_selection_uses_the_painted_row_and_emits_at_once() {
        let mut state = state();
        let layout = page(Category::Display);
        let zoom = layout.combo_rect(LauncherComboId::Zoom).unwrap();
        state.shell_mouse_down(zoom.x + 10, zoom.y + 5, &layout);
        state.drain_output();
        let popup = state.shell_popup(&layout).unwrap();
        // 800x600 has room for Auto and 100% only.
        assert_eq!((popup.rows, popup.selected), (2, 0));
        assert_eq!(popup.rect.h, 2 * popup.row_height);
        let row = popup.row_rect(1);
        state.shell_mouse_move(row.x + 10, row.y + 5, &layout);
        assert_eq!(state.shell_popup(&layout).unwrap().hovered, Some(1));
        state.shell_mouse_down(row.x + 10, row.y + 5, &layout);
        assert_eq!(state.shell_selected_zoom(&layout), Zoom::Percent(100));
        assert!(state.shell_popup(&layout).is_none());
        assert_eq!(
            state.drain_output().events,
            [
                LauncherOptionsEvent::Cue(PageCue::ComboClose),
                LauncherOptionsEvent::ZoomSelected(Zoom::Percent(100))
            ]
        );

        // A bigger window lists more steps, and a choice it can't reach
        // shows and selects as the step it gives way to.
        let mut state = OptionsDialogState::new(
            LauncherOptionsLabels::resolve(&|_| None),
            LauncherOptionsValues::default(),
            Zoom::Percent(300),
            true,
            Vec::new(),
            DisplayTarget::Auto,
        );
        // 1920x1080 lists four steps; 300% is beyond them.
        let layout = LauncherOptionsLayout::responsive(1920, 1080, Category::Display);
        let face = layout.combo_rect(LauncherComboId::Zoom).unwrap();
        state.shell_mouse_down(face.x + 10, face.y + 5, &layout);
        assert_eq!(state.shell_selected_zoom(&layout), Zoom::Percent(200));
        let popup = state.shell_popup(&layout).unwrap();
        assert_eq!((popup.rows, popup.selected), (4, 3));
        let below = popup.row_rect(4);
        state.shell_mouse_down(below.x + 10, below.y + 5, &layout);
        assert_eq!(
            state.zoom,
            Zoom::Percent(300),
            "below the last row picks nothing"
        );
    }

    #[test]
    fn sidebar_list_offers_the_sizes_the_battle_frame_has_room_for() {
        let mut state = state();
        // A 1920x1080 window at 100%: every size fits.
        let layout = page(Category::Interface).with_battle_frame((1920, 1080));
        let face = layout.combo_rect(LauncherComboId::Sidebar).unwrap();
        state.shell_mouse_down(face.x + 10, face.y + 5, &layout);
        assert_eq!(state.shell_open_list(), Some(LauncherComboId::Sidebar));
        let popup = state.shell_popup(&layout).unwrap();
        assert_eq!(
            (popup.combo, popup.rows, popup.selected),
            (LauncherComboId::Sidebar, 3, 0)
        );
        let xl = popup.row_rect(2);
        state.drain_output();
        state.shell_mouse_down(xl.x + 10, xl.y + 5, &layout);
        assert_eq!(state.shell_open_list(), None);
        assert_eq!(
            state.shell_selected_sidebar(&layout),
            SidebarSize::ExtraLarge
        );
        assert_eq!(
            state.drain_output().events,
            [
                LauncherOptionsEvent::Cue(PageCue::ComboClose),
                LauncherOptionsEvent::SidebarSelected(SidebarSize::ExtraLarge)
            ]
        );

        // Zoomed in on a 3440x1440 screen, XL has no room: it shows as Large,
        // and the list stops there.
        let zoomed = page(Category::Interface).with_battle_frame((1720, 720));
        assert_eq!(state.shell_selected_sidebar(&zoomed), SidebarSize::Large);
        state.shell_mouse_down(face.x + 10, face.y + 5, &zoomed);
        let popup = state.shell_popup(&zoomed).unwrap();
        assert_eq!((popup.rows, popup.selected), (2, 1));
    }

    #[test]
    fn display_list_picks_the_screen_a_row_names() {
        let mut state = state();
        let layout = page(Category::Display);
        let face = layout.combo_rect(LauncherComboId::Display).unwrap();
        // Auto is the face's text and the list's first row until a screen is
        // picked, so a fresh profile reads the way it always behaved.
        assert_eq!(state.shell_display_label(), "Auto");
        state.shell_mouse_down(face.x + 10, face.y + 5, &layout);
        assert_eq!(state.shell_open_list(), Some(LauncherComboId::Display));
        let popup = state.shell_popup(&layout).unwrap();
        assert_eq!(
            (popup.combo, popup.rows, popup.selected),
            (LauncherComboId::Display, 3, 0),
            "Auto, then the two screens"
        );
        assert_eq!(
            state.shell_display_rows(),
            [
                "Auto".to_owned(),
                "Screen 1: 1920x1080 60Hz".to_owned(),
                "Screen 2: 2560x1440 144Hz".to_owned(),
            ]
        );
        // The second screen's row: the pick publishes at once, like Sidebar.
        state.drain_output();
        let second = popup.row_rect(2);
        state.shell_mouse_down(second.x + 10, second.y + 5, &layout);
        assert_eq!(state.shell_open_list(), None);
        assert_eq!(state.shell_display_label(), "Screen 2: 2560x1440 144Hz");
        assert_eq!(
            state.drain_output().events,
            [
                LauncherOptionsEvent::Cue(PageCue::ComboClose),
                LauncherOptionsEvent::DisplaySelected(DisplayTarget::Fixed(
                    "\\\\.\\DISPLAY2".to_owned()
                )),
            ]
        );
        // Back to Auto: the window returns to the system's own choice.
        state.shell_mouse_down(face.x + 10, face.y + 5, &layout);
        let popup = state.shell_popup(&layout).unwrap();
        assert_eq!(popup.selected, 2, "the picked screen shows selected");
        state.drain_output();
        let auto = popup.row_rect(0);
        state.shell_mouse_down(auto.x + 10, auto.y + 5, &layout);
        assert_eq!(state.shell_display_label(), "Auto");
        assert_eq!(
            state.drain_output().events,
            [
                LauncherOptionsEvent::Cue(PageCue::ComboClose),
                LauncherOptionsEvent::DisplaySelected(DisplayTarget::Auto),
            ]
        );
    }

    #[test]
    fn display_face_names_a_saved_screen_the_desktop_no_longer_has() {
        // The profile keeps a name this desktop does not: the face still names
        // it, so the player can see what is saved, and its row is not selected.
        let mut state = OptionsDialogState::new(
            LauncherOptionsLabels::resolve(&|_| None),
            LauncherOptionsValues::default(),
            Zoom::Auto,
            true,
            state().displays().to_vec(),
            DisplayTarget::Fixed("\\\\.\\DISPLAY9".to_owned()),
        );
        assert_eq!(state.shell_display_label(), "\\\\.\\DISPLAY9");
        let layout = page(Category::Display);
        let face = layout.combo_rect(LauncherComboId::Display).unwrap();
        state.shell_mouse_down(face.x + 10, face.y + 5, &layout);
        let popup = state.shell_popup(&layout).unwrap();
        assert_eq!(
            (popup.rows, popup.selected),
            (3, 0),
            "an absent screen still falls back to Auto's row"
        );
    }

    /// Each box toggles on its page with its cue and reports the change; a
    /// fresh dialog per box keeps them independent.
    #[test]
    fn settings_checkboxes_toggle_with_their_cue() {
        use LauncherCheckboxId::*;
        for (category, id, label, event) in [
            (
                Category::Display,
                FullScreen,
                FULL_SCREEN_LABEL,
                LauncherOptionsEvent::FullScreenChanged as fn(bool) -> LauncherOptionsEvent,
            ),
            (
                Category::Display,
                OriginalShape,
                ORIGINAL_SHAPE_LABEL,
                LauncherOptionsEvent::OriginalShapeChanged,
            ),
            (
                Category::General,
                YrOriginalStyle,
                YR_ORIGINAL_STYLE_LABEL,
                LauncherOptionsEvent::YrOriginalStyleChanged,
            ),
            (
                Category::Gameplay,
                CommanderFeedback,
                COMMANDER_FEEDBACK_LABEL,
                LauncherOptionsEvent::CommanderFeedbackChanged,
            ),
        ] {
            let mut state = state();
            let layout = page(category);
            let rect = checkbox(&layout, id);
            assert_eq!(state.shell_checkbox_label(id), tr(label));
            let before = state.shell_checkbox_checked(id);
            state.shell_mouse_down(rect.x + 5, rect.y + 5, &layout);
            assert_eq!(state.shell_checkbox_checked(id), !before, "{id:?}");
            assert_eq!(
                state.drain_output().events,
                [LauncherOptionsEvent::Cue(PageCue::Checkbox), event(!before)],
                "{id:?}"
            );
        }
    }

    /// The three end-game boxes start unticked like every VERA box. Each
    /// publishes its own event and leaves the other two alone.
    #[test]
    fn the_end_game_boxes_start_unticked_and_publish_their_own_event() {
        use LauncherCheckboxId::{CustomEndGameBanner, CustomEndGameMovie, CustomEndGameScore};
        let layout = page(Category::Gameplay);
        for (id, label, on) in [
            (
                CustomEndGameBanner,
                CUSTOM_END_GAME_BANNER_LABEL,
                LauncherOptionsEvent::CustomEndGameBannerChanged(true),
            ),
            (
                CustomEndGameScore,
                CUSTOM_END_GAME_SCORE_LABEL,
                LauncherOptionsEvent::CustomEndGameScoreChanged(true),
            ),
            (
                CustomEndGameMovie,
                CUSTOM_END_GAME_MOVIE_LABEL,
                LauncherOptionsEvent::CustomEndGameMovieChanged(true),
            ),
        ] {
            // A fresh dialog per box: each click is the player's first, and
            // the boxes must not remember each other's state.
            let mut state = state();
            assert_eq!(state.shell_checkbox_label(id), tr(label));
            assert!(!state.shell_checkbox_checked(id), "{id:?} starts unticked");
            let rect = checkbox(&layout, id);
            state.shell_mouse_down(rect.x + 5, rect.y + 5, &layout);
            assert!(state.shell_checkbox_checked(id), "{id:?} ticked");
            assert_eq!(
                state.drain_output().events,
                [LauncherOptionsEvent::Cue(PageCue::Checkbox), on],
                "{id:?}"
            );
            for other in [CustomEndGameBanner, CustomEndGameScore, CustomEndGameMovie] {
                if other != id {
                    assert!(
                        !state.shell_checkbox_checked(other),
                        "{id:?}'s click left {other:?} alone"
                    );
                }
            }
        }
    }

    /// The Health Bars list on the Interface page: Original, All Units, and
    /// All Units, Not Full. Its pick publishes at once like the Sidebar list.
    #[test]
    fn health_bars_list_publishes_its_pick_at_once() {
        let mut state = state();
        let layout = page(Category::Interface);
        let face = layout.combo_rect(LauncherComboId::HealthBars).unwrap();
        assert_eq!(state.shell_selected_health_bars(), HealthBarMode::Vanilla);
        state.shell_mouse_down(face.x + 10, face.y + 5, &layout);
        assert_eq!(state.shell_open_list(), Some(LauncherComboId::HealthBars));
        let popup = state.shell_popup(&layout).unwrap();
        assert_eq!(
            (popup.combo, popup.rows, popup.selected),
            (LauncherComboId::HealthBars, 3, 0)
        );
        state.drain_output();
        let last = popup.row_rect(2);
        state.shell_mouse_down(last.x + 10, last.y + 5, &layout);
        assert_eq!(state.shell_open_list(), None);
        assert_eq!(
            state.shell_selected_health_bars(),
            HealthBarMode::AlwaysExceptFull
        );
        assert_eq!(
            state.drain_output().events,
            [
                LauncherOptionsEvent::Cue(PageCue::ComboClose),
                LauncherOptionsEvent::HealthBarModeSelected(HealthBarMode::AlwaysExceptFull)
            ]
        );
    }

    /// The Team Color Strength slider publishes the level it stops on, not
    /// the position, so the profile never sees gamemd's bands as 0.
    #[test]
    fn house_color_strength_slider_publishes_the_level_it_stops_on() {
        let mut state = state();
        let layout = page(Category::Display);
        let slider = layout
            .trackbar_rect(LauncherTrackbarId::HouseColorStrength)
            .unwrap();
        assert_eq!(state.house_color_strength(), STRENGTH_RETAIL);
        state.shell_sync(&layout);
        assert_eq!(
            state.shell_slider_value(LauncherTrackbarId::HouseColorStrength),
            Some(1),
            "the plaque reads the level"
        );

        // The strip admits from `bottom - 18`; the far right is the top level.
        let retail_pack = state.pack();
        state.shell_mouse_down(slider.x + slider.w - 2, slider.y + slider.h - 3, &layout);
        state.shell_mouse_up(slider.x + slider.w - 2, slider.y + slider.h - 3, &layout);
        assert_eq!(state.house_color_strength(), STRENGTH_MAX);
        assert_eq!(
            state.shell_slider_value(LauncherTrackbarId::HouseColorStrength),
            Some(10)
        );
        assert_eq!(
            state.drain_output().events,
            [
                LauncherOptionsEvent::Cue(PageCue::ListClick),
                LauncherOptionsEvent::HouseColorStrengthChanged(STRENGTH_MAX),
            ],
            "the notch click leads the level it stands for"
        );
        assert_eq!(state.pack(), retail_pack, "the retail pack is untouched");

        // Back to the left end: the original bands, and a level of 1.
        state.shell_mouse_down(slider.x + 1, slider.y + slider.h - 3, &layout);
        state.shell_mouse_up(slider.x + 1, slider.y + slider.h - 3, &layout);
        assert_eq!(state.house_color_strength(), STRENGTH_RETAIL);
        assert_eq!(
            state.drain_output().events,
            [
                LauncherOptionsEvent::Cue(PageCue::ListClick),
                LauncherOptionsEvent::HouseColorStrengthChanged(STRENGTH_RETAIL),
            ]
        );
    }

    #[test]
    fn performance_string_keys_and_skip_intro_boxes_send_their_events() {
        let mut state = state();
        for (category, id, event) in [
            (
                Category::Display,
                LauncherCheckboxId::PerformanceStats,
                LauncherOptionsEvent::PerformanceStatsChanged(true),
            ),
            (
                Category::General,
                LauncherCheckboxId::ShowStringKeys,
                LauncherOptionsEvent::ShowStringKeysChanged(true),
            ),
            (
                Category::General,
                LauncherCheckboxId::SkipIntro,
                LauncherOptionsEvent::SkipIntroChanged(true),
            ),
        ] {
            let layout = page(category);
            let rect = checkbox(&layout, id);
            state.shell_mouse_down(rect.x + 5, rect.y + 5, &layout);
            assert_eq!(
                state.drain_output().events,
                [LauncherOptionsEvent::Cue(PageCue::Checkbox), event]
            );
        }
    }

    #[test]
    fn performance_stats_box_follows_live_state() {
        let mut state = state();
        let id = LauncherCheckboxId::PerformanceStats;
        let layout = page(Category::Display);
        let rect = checkbox(&layout, id);
        state.shell_mouse_down(rect.x + 5, rect.y + 5, &layout);
        assert!(state.shell_checkbox_checked(id));
        // Ctrl+Alt+F while the page is open moves the box with the readout.
        state.sync_performance_stats(false);
        assert!(!state.shell_checkbox_checked(id));
    }
}
