//! Physical-pixel layout and input for launcher resource 0xD5.
//!
//! Retail RT_DIALOG/0xD5/1033 provides the DLU rectangles; common setup
//! 0x00608CD0/0x00609730 anchors the rail, and 0x0060B950 adjusts its title.
//! The right-hand column picks a category; its page lays the settings table's
//! rows out in one column at the places retail's first band uses. Each laid-out
//! control is a node of a `vera20k-gui` tree, which owns its hit tests,
//! captures, hover and the open list; the host paints the tree with the
//! shared widget painter. The values stay the retained `OptionsDialogState`'s.

use super::{
    Category, HAS_CHECK_FOR_UPDATES, HAS_MODS, HAS_SET_WINDOW_SIZE, LauncherButton,
    LauncherCheckboxId, LauncherComboId, LauncherOptionsEvent, LauncherParentResult,
    LauncherTrackbarId, OptionsDialogState, Setting, SidebarSize, Zoom,
};
use std::borrow::Cow;

use vera20k_gui::{
    DialogKind, Gui, InputEvent, MouseButton, NodeId, NodeKind, NodeSpec, Prop, Value,
};

use crate::i18n::tr;
use crate::ui::shell::descriptor::DialogResourceId;
use crate::ui::shell::geom::ShellAlign;
use crate::ui::shell::geom::{
    RectPx, ShellScale, center_offset, child_window, dlu_rect, right_panel_rects,
};
use crate::ui::shell::in_game_shell;
use crate::ui::shell::menu_page::{self, MenuPageButtonSpec, MenuPageSpec};
use crate::ui::shell::modal::{FOOTER_TEMPLATE, template_on};
use crate::ui::shell::page::{self, PageCue, PageEvent};
use crate::ui::skirmish_shell::COMBO_FACE_H;

mod launcher_pages;

/// Something on a page, as the node standing for it in the tree.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Control {
    Label(PageLabel),
    Setting(Setting),
    Button(LauncherButton),
}

/// A caption the page paints, by what it names.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PageLabel {
    /// The category's name, in a retail band heading's place.
    Heading,
    Section(Category),
    /// A list's or slider's caption, over its face or track.
    Caption(Setting),
    /// One of VERA's translation keys.
    Localized(&'static str),
    /// The step a plaque-less slider stands at, right-aligned over the
    /// track's end as retail prints Visual Details'.
    Step(LauncherTrackbarId),
    /// Static `GUI:Blank` at the status line's template place.
    Blank,
}

/// The page's controls on a `vera20k-gui` tree, which owns their values on
/// screen, hit tests, the press and capture of buttons and sliders, hover
/// and the open list. A node's `Prop::Id` is its index in `built`; a layout
/// that lists other controls (a page switch) builds the tree again.
pub(super) struct ShellGui {
    gui: Gui,
    /// The nodes' parent, which the host paints from.
    panel: NodeId,
    built: Vec<(Control, NodeId)>,
    /// Status help last written by a dialog hit test (`0x00622CCB`).
    status_help: Option<String>,
    /// The tree's UI clock starts here.
    epoch: crate::util::time::Instant,
}

impl Default for ShellGui {
    fn default() -> Self {
        let mut gui = Gui::new(256);
        let root = gui.create(NodeKind::Dialog, None).expect("an empty tree");
        let panel = gui.create(NodeKind::Panel, Some(root)).expect("budget");
        gui.push_dialog(root, DialogKind::Page)
            .expect("a Dialog root");
        Self {
            gui,
            panel,
            built: Vec::new(),
            status_help: None,
            epoch: crate::util::time::Instant::now(),
        }
    }
}

impl std::fmt::Debug for ShellGui {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ShellGui")
            .field("controls", &self.built.len())
            .field("status_help", &self.status_help)
            .finish()
    }
}

impl ShellGui {
    /// One input for the tree, on its clock.
    fn input(&mut self, event: InputEvent) {
        let now = crate::util::time::Instant::now().saturating_duration_since(self.epoch);
        self.gui.input(event, now.as_millis() as u64);
    }

    fn node(&self, control: Control) -> Option<NodeId> {
        self.built
            .iter()
            .find(|(built, _)| *built == control)
            .map(|(_, node)| *node)
    }

    /// The control whose node has id `id`.
    fn control(&self, id: u16) -> Option<Control> {
        self.built.get(usize::from(id)).map(|(control, _)| *control)
    }

    fn control_of(&self, node: NodeId) -> Option<Control> {
        self.control(u16::try_from(self.gui.id_of(node)?).ok()?)
    }

    /// The slider holding the mouse since a press on it.
    fn held_slider(&self) -> Option<LauncherTrackbarId> {
        match self.gui.pressed().and_then(|node| self.control_of(node)) {
            Some(Control::Setting(Setting::Slider(id))) => Some(id),
            _ => None,
        }
    }

    /// Drops every control, and with them a held slider or button and an
    /// open list; the status help stays.
    pub(super) fn clear(&mut self) {
        for (_, node) in self.built.drain(..) {
            let _ = self.gui.remove(node);
        }
    }

    /// Make the tree hold `wanted`, in order, each node with its spec's
    /// props. Nodes that stay keep their hover and capture.
    fn sync(&mut self, wanted: Vec<(Control, NodeSpec)>, scale: ShellScale) {
        let same = wanted
            .iter()
            .map(|(control, _)| control)
            .eq(self.built.iter().map(|(control, _)| control));
        if !same {
            self.clear();
            for (id, (control, spec)) in wanted.iter().enumerate() {
                let node = self
                    .gui
                    .create(spec.kind, Some(self.panel))
                    .expect("a page stays far below the node budget");
                let _ = self.gui.set(node, Prop::Id, Value::Int(id as i32));
                self.built.push((*control, node));
            }
        }
        for ((_, node), (_, spec)) in self.built.iter().zip(wanted) {
            for (prop, value) in spec.props {
                let _ = self.gui.set(*node, prop, value);
            }
        }
        if let Some(dialog) = self.gui.top_dialog() {
            self.gui.set_dialog_scale(dialog, scale);
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
    dialog: DialogResourceId(0x00D5),
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

/// The launcher page's slide column: its top buttons, and whether a button
/// stands on the tile above Back. None for a page the launcher reaches only
/// through the older column.
pub(crate) fn launcher_slide_column(category: Category) -> Option<(u32, bool)> {
    match category {
        Category::LauncherMain => Some((3, HAS_MODS)),
        Category::Enhancements | Category::Extensions => Some((0, false)),
        _ => None,
    }
}

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
/// What a greyed control says on the in-game menu: a setting the next game
/// starts with ([`LauncherTrackbarId::fixed_in_match`]), or Check for Updates.
const NOT_IN_MATCH_HELP: &str = "VERA:NotInMatchHelp";
const SET_WINDOW_SIZE_LABEL: &str = "VERA:SetWindowSize";
/// The in-game page's first row. 8 DLUs below the main menu's to clear the
/// frame's top edge at 640x480, and a multiple of 8 so its sliders round to the
/// same authored heights ([`dlu_rect`] is `y * 13 / 8`).
const IN_GAME_FIRST_ROW: i32 = FIRST_ROW + 8;
/// The Game Options page's first row on FF, under the group that heads the page
/// there (`PauseMenuLayout::raise_internet_group`).
const IN_GAME_FIRST_ROW_UNDER_FF_GROUP: i32 = IN_GAME_FIRST_ROW + 80;

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
    /// Every control in hit and paint order: the column's buttons top to
    /// bottom (on the in-game menu, the ones it gives the column's rows),
    /// then the page's captions, settings and own buttons.
    pub(crate) controls: Vec<(Control, RectPx)>,
    /// The mapping the rects are in: `RETAIL_SCALE` centres the
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

/// The rows of `settings` from `first_row` DLUs down, each authored rect put
/// on screen by `px`; returns where a further row would start, in DLUs.
fn page_rows(
    out: &mut Vec<(Control, RectPx)>,
    settings: &[Setting],
    first_row: i32,
    px: impl Fn(RectPx) -> RectPx,
) -> i32 {
    let dlu = |x, y, w, h| px(dlu_rect(x, y, w, h));
    let mut top = first_row;
    for &setting in settings {
        let caption = Control::Label(PageLabel::Caption(setting));
        let control = Control::Setting(setting);
        match setting {
            Setting::List(_) => {
                out.push((caption, dlu(ROW_X, top, 120, 10)));
                // The chrome atlas keys combo faces on their pixel size:
                // 180 is the retail resolution list's face.
                let at = dlu_rect(ROW_X, top, 0, 0);
                let face = RectPx::new(at.x, at.y + 16, OPTIONS_COMBO_W, COMBO_FACE_H);
                out.push((control, px(face)));
                top += LIST_PITCH;
            }
            // Trackbar art exists for retail's windows only: 150 DLUs with
            // a value plaque (Generate Map's), 120 without (Visual Details').
            // The sliders run one pixel wider and taller than the template
            // (retail stills: 181x22 and 129x22).
            Setting::Slider(id) => {
                let width = if id.plaque_reserve() > 0 { 150 } else { 120 };
                if width == 150 {
                    out.push((caption, dlu(ROW_X, top, 150, 10)));
                } else {
                    // A level number needs a narrow box; a step name, such
                    // as Visual Details' "Medium", half the track.
                    let step_w = match id {
                        LauncherTrackbarId::HouseColorStrength => 20,
                        _ => 60,
                    };
                    out.push((caption, dlu(ROW_X, top, 120 - step_w, 10)));
                    let step = Control::Label(PageLabel::Step(id));
                    out.push((step, dlu(ROW_X + 120 - step_w, top, step_w, 10)));
                }
                out.push((control, px(child_window(ROW_X, top + 15, width, 13))));
                top += SLIDER_PITCH;
            }
            Setting::Checkbox(_) => {
                out.push((control, dlu(BOX_X, top, BOX_W, 10)));
                top += BOX_PITCH;
            }
        }
    }
    top
}

/// The button a page holds under its rows: General's Check for Updates where
/// the build has an updater, and Interface's Edit Layout. Not retail: the
/// column has no tile left for them, so each sits under its page's boxes, the
/// size of Sound Options' Play and Stop (`0xD6`, MNBTTN type 3).
fn page_button(
    category: Category,
    end: i32,
    px: impl Fn(RectPx) -> RectPx,
) -> Option<(Control, RectPx)> {
    let id = match category {
        Category::General if HAS_CHECK_FOR_UPDATES => LauncherButton::CheckForUpdates,
        Category::Interface => LauncherButton::EditLayout,
        _ => return None,
    };
    Some((Control::Button(id), px(dlu_rect(ROW_X, end + 6, 83, 15))))
}

impl LauncherOptionsLayout {
    /// Gamemd's centred `0xD5`: the authored dialog placed in the window at
    /// `RETAIL_SCALE`, or fitted where the panel is too short for the
    /// column ([`retail_column_fits`]).
    pub(crate) fn new(width: i32, height: i32, category: Category) -> Self {
        let scale = if retail_column_fits(width, height) {
            crate::ui::shell::geom::RETAIL_SCALE
        } else {
            crate::ui::shell::geom::shell_scale(width, height)
        };
        Self::build(width, height, scale, category)
    }

    /// The same dialog scaled to fit the window.
    pub(crate) fn responsive(width: i32, height: i32, category: Category) -> Self {
        let scale = crate::ui::shell::geom::shell_scale(width, height);
        Self::build(width, height, scale, category)
    }

    fn build(width: i32, height: i32, scale: ShellScale, category: Category) -> Self {
        // The scale centres a scaled page itself, so it takes no centring
        // offset of its own and every authored rect goes through the one scale.
        let stretch = !scale.is_unscaled();
        let (offset_x, offset_y) = if stretch {
            (0, 0)
        } else {
            (center_offset(width, 800), center_offset(height, 600))
        };
        let px = |rect: RectPx| scale.scale_rect(rect.translate(offset_x, offset_y));
        // The right-panel children follow the panel, not the dialog centring.
        let panel = if stretch {
            menu_page::compute_responsive_layout(&OPTIONS_PAGE, width as u32, height as u32)
        } else {
            menu_page::compute_layout(&OPTIONS_PAGE, width as u32, height as u32)
        };
        if let Some(controls) = launcher_pages::controls(category, &panel, px) {
            let size = (width.max(0) as u32, height.max(0) as u32);
            return Self {
                category,
                title: panel.title,
                warning: panel.warning_monitor,
                status_help: panel.status_help,
                controls,
                scale,
                screen: size,
                battle_frame: size,
                surface: OptionsSurface::Launcher,
            };
        }
        let keyboard = (
            KEYBOARD_BUTTON,
            LauncherButton::Exit(LauncherParentResult::Keyboard),
        );
        let column = std::iter::once(keyboard)
            .chain(
                CATEGORY_BUTTONS
                    .into_iter()
                    .zip(Category::ALL.map(LauncherButton::Category)),
            )
            .chain([
                (
                    MODS_BUTTON,
                    LauncherButton::Exit(LauncherParentResult::Mods),
                ),
                (
                    MAIN_MENU_BUTTON,
                    LauncherButton::Exit(LauncherParentResult::Back),
                ),
            ]);
        let mut controls: Vec<_> = column
            .filter_map(|(id, button)| Some((Control::Button(button), panel.button_rect(id)?)))
            .collect();
        controls.push((
            Control::Label(PageLabel::Heading),
            px(dlu_rect(HEADING.0, HEADING.1, 298, 10)),
        ));
        controls.push((
            Control::Label(PageLabel::Blank),
            px(dlu_rect(2, 355, 303, 12)),
        ));
        let end = page_rows(&mut controls, &category.settings(), FIRST_ROW, px);
        controls.extend(page_button(category, end, px));
        let size = (width.max(0) as u32, height.max(0) as u32);
        Self {
            category,
            title: panel.title,
            warning: panel.warning_monitor,
            status_help: panel.status_help,
            controls,
            scale,
            screen: size,
            battle_frame: size,
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
        let mut controls = Vec::new();
        if category == Category::GameControls {
            controls.extend(
                column
                    .first()
                    .map(|&rect| (Control::Button(LauncherButton::InGameOptions), rect)),
            );
        } else if let Some((&back, rows)) = column.split_last() {
            let ids = std::iter::once(LauncherButton::Exit(LauncherParentResult::Keyboard))
                .chain(Category::ALL.map(LauncherButton::Category));
            controls.extend(ids.map(Control::Button).zip(rows.iter().copied()));
            controls.push((Control::Button(LauncherButton::InGameBack), back));
        }
        let first_row = if internet && category == Category::Gameplay {
            IN_GAME_FIRST_ROW_UNDER_FF_GROUP
        } else {
            IN_GAME_FIRST_ROW
        };
        let settings: Vec<Setting> = category
            .settings()
            .into_iter()
            .filter(|setting| match setting {
                Setting::Slider(LauncherTrackbarId::GameSpeed) => game_speed,
                // The clips play in a skirmish only (`app::commander_feedback`),
                // and under FF's group a 640x480 page has no row left for the box.
                Setting::Checkbox(LauncherCheckboxId::CommanderFeedback) => !internet,
                _ => true,
            })
            .collect();
        let end = page_rows(&mut controls, &settings, first_row, px);
        controls.extend(page_button(category, end, px));
        if category == Category::GameControls {
            // One row of buttons the size of Check for Updates ends the page:
            // the Esc menu's Set Window Size under its caption, then the
            // dialogs gamemd's Game Controls opens. Keyboard is here only
            // while the sidebar has no row for it.
            let top = end + 3;
            if HAS_SET_WINDOW_SIZE {
                let caption = Control::Label(PageLabel::Localized(SET_WINDOW_SIZE_LABEL));
                controls.push((caption, px(dlu_rect(ROW_X, top, 83, 10))));
            }
            let row = [
                (HAS_SET_WINDOW_SIZE, LauncherButton::SetWindowSize),
                (keyboard, LauncherButton::InGameKeyboard),
                (true, LauncherButton::InGameSound),
            ];
            let shown = row
                .into_iter()
                .filter_map(|(shown, id)| shown.then_some(id));
            for (x, id) in (0..).map(|i| ROW_X + 90 * i).zip(shown) {
                controls.push((Control::Button(id), px(dlu_rect(x, top + 10, 83, 15))));
            }
        }
        Self {
            category,
            title: nothing,
            warning: nothing,
            status_help: in_game_shell::status_line_rect(height),
            controls,
            scale: crate::ui::shell::geom::RETAIL_SCALE,
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
        use LauncherCheckboxId::{FullScreen, PerformanceStats};
        let modal = template_on(FOOTER_TEMPLATE, width, height, scale);
        let dialog = scale.base_rect(modal.dialog);
        let action = scale.base_rect(modal.button(0));
        let inset = action.x - dialog.x;
        let available = dialog.w - 2 * inset;
        let rows = [
            Setting::Checkbox(FullScreen),
            Setting::Checkbox(PerformanceStats),
            Setting::List(LauncherComboId::Language),
        ];
        let captions = rows
            .map(|setting| setting.text().0)
            .into_iter()
            .chain(HAS_SET_WINDOW_SIZE.then_some(SET_WINDOW_SIZE_LABEL))
            .collect::<Vec<_>>();
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
        let count = captions.len() as i32;
        let top = body.y + (body.h - (count - 1) * ESC_MENU_ROW_PITCH - row_h) / 2;
        let cell = |row: usize, x, w, h| {
            let y = top + row as i32 * ESC_MENU_ROW_PITCH + (row_h - h) / 2;
            scale.scale_rect(RectPx::new(x, y, w, h))
        };
        let mut controls: Vec<_> = captions
            .iter()
            .enumerate()
            .map(|(row, &key)| {
                let label = Control::Label(PageLabel::Localized(key));
                (label, cell(row, body.x, offset, 17))
            })
            .collect();
        for (row, setting) in rows.into_iter().enumerate() {
            let h = if row == 2 { COMBO_FACE_H } else { 18 };
            controls.push((Control::Setting(setting), cell(row, value_x, control_w, h)));
        }
        if HAS_SET_WINDOW_SIZE {
            let button = Control::Button(LauncherButton::SetWindowSize);
            controls.push((button, cell(3, value_x, action.w, action.h)));
        }
        let nothing = RectPx::new(0, 0, 0, 0);
        let size = (width.max(0) as u32, height.max(0) as u32);
        Self {
            category: Category::ALL[0],
            title: nothing,
            warning: nothing,
            status_help: nothing,
            controls,
            scale,
            screen: size,
            battle_frame: size,
            surface: OptionsSurface::EscMenu(scale.scale_rect(body)),
        }
    }

    /// The same page for a battle frame other than the window: a zoomed or
    /// 4:3 match (`App::match_zoom_geometry`).
    pub(crate) fn with_battle_frame(mut self, battle_frame: (u32, u32)) -> Self {
        self.battle_frame = battle_frame;
        self
    }

    /// `control`'s rect when this page has it.
    pub(crate) fn rect(&self, control: Control) -> Option<RectPx> {
        self.controls
            .iter()
            .find(|(candidate, _)| *candidate == control)
            .map(|(_, rect)| *rect)
    }

    /// The page's buttons, the column's first.
    pub(crate) fn buttons(&self) -> impl Iterator<Item = (LauncherButton, RectPx)> + '_ {
        self.controls
            .iter()
            .filter_map(|&(control, rect)| match control {
                Control::Button(id) => Some((id, rect)),
                _ => None,
            })
    }

    /// The page's settings, top to bottom.
    #[cfg(test)]
    pub(crate) fn settings(&self) -> impl Iterator<Item = (Setting, RectPx)> + '_ {
        self.controls
            .iter()
            .filter_map(|&(control, rect)| match control {
                Control::Setting(setting) => Some((setting, rect)),
                _ => None,
            })
    }

    pub(crate) fn button_rect(&self, id: LauncherButton) -> Option<RectPx> {
        self.rect(Control::Button(id))
    }

    #[cfg(test)]
    pub(crate) fn trackbar_rect(&self, id: LauncherTrackbarId) -> Option<RectPx> {
        self.rect(Control::Setting(Setting::Slider(id)))
    }

    #[cfg(test)]
    pub(crate) fn checkbox_rect(&self, id: LauncherCheckboxId) -> Option<RectPx> {
        self.rect(Control::Setting(Setting::Checkbox(id)))
    }

    #[cfg(test)]
    pub(crate) fn combo_rect(&self, id: LauncherComboId) -> Option<RectPx> {
        self.rect(Control::Setting(Setting::List(id)))
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
        let closed = self.shell.gui.close_popup();
        if closed {
            self.pending_cues.push(PageCue::ComboClose);
        }
        closed
    }

    /// The tree the page paints from, and the nodes' parent.
    pub(crate) fn shell_gui(&self) -> (&Gui, NodeId) {
        (&self.shell.gui, self.shell.panel)
    }

    /// The control whose node has `Prop::Id` `id`.
    pub(crate) fn shell_control(&self, id: i32) -> Option<Control> {
        self.shell.control(u16::try_from(id).ok()?)
    }

    /// Where the open list hangs, on the tree's geometry: under the painted
    /// face, its rows on this window's dropdown device metrics. Unlike the
    /// template's 74-DLU drop allocation (ComboDropWin `0x006180CF..0x00618205`,
    /// five whole rows), it grows with the choices, so it never scrolls.
    pub(crate) fn shell_popup_rect(&self) -> Option<RectPx> {
        let node = self.shell.gui.popup()?;
        Some(self.shell.gui.combo_drop_down(node)?.popup.rect)
    }

    /// A page label's text and alignment: left, but a step name sits
    /// right-aligned over its track's end.
    fn page_label(
        &self,
        layout: &LauncherOptionsLayout,
        label: PageLabel,
    ) -> (Cow<'_, str>, ShellAlign) {
        let text = match label {
            PageLabel::Heading => self.text(layout.category.heading()),
            PageLabel::Section(category) => self.text(category.heading()),
            PageLabel::Caption(Setting::Slider(LauncherTrackbarId::GameSpeed)) => {
                self.game_speed_caption.as_str().into()
            }
            PageLabel::Caption(setting) => self.text(setting.text().0),
            PageLabel::Localized(key) => tr(key).into(),
            PageLabel::Step(id) => return (self.slider_step(id), ShellAlign::H_RIGHT),
            PageLabel::Blank => self.labels.get("GUI:Blank").into(),
        };
        (text, ShellAlign::NONE)
    }

    /// The step name a plaque-less slider prints; a plaque slider shows its
    /// value on the plaque instead. VERA's Team Color Strength prints the
    /// level its position stands for, so the player never sees a 0 for
    /// gamemd's own colours.
    pub(super) fn slider_step(&self, id: LauncherTrackbarId) -> Cow<'_, str> {
        let position = usize::from(self.trackbar_position(id));
        Cow::Borrowed(match id {
            LauncherTrackbarId::GameSpeed => self
                .game_speed_names
                .get(position)
                .map_or("", String::as_str),
            LauncherTrackbarId::HouseColorStrength => {
                return Cow::Owned(self.house_color_strength().to_string());
            }
            _ => id
                .steps()
                .get(position)
                .map_or("", |key| self.labels.get(key)),
        })
    }

    /// A box's text: its caption, or in the Esc menu's two columns, where the
    /// setting is already named on the left, Yes or No.
    pub(super) fn shell_checkbox_face_text(
        &self,
        layout: &LauncherOptionsLayout,
        id: LauncherCheckboxId,
    ) -> Cow<'_, str> {
        match layout.surface {
            OptionsSurface::EscMenu(_) => {
                let checked = self.shell_checkbox_checked(id);
                self.labels
                    .get(if checked { "GUI:Yes" } else { "GUI:No" })
                    .into()
            }
            _ => self.text(Setting::Checkbox(id).text().0),
        }
    }

    pub(crate) fn shell_button_label(&self, id: LauncherButton) -> Cow<'_, str> {
        self.text(match id {
            LauncherButton::Exit(LauncherParentResult::Keyboard)
            | LauncherButton::InGameKeyboard => "GUI:Keyboard",
            LauncherButton::Exit(LauncherParentResult::Back) => "GUI:MainMenu",
            LauncherButton::Exit(LauncherParentResult::Mods) => crate::ui::mods_shell::TITLE_KEY,
            LauncherButton::Exit(LauncherParentResult::Terminal) => return Cow::Borrowed(""),
            LauncherButton::Category(category) => category.heading(),
            LauncherButton::InGameOptions => "GUI:OptionsMenu",
            LauncherButton::InGameBack => "GUI:Back",
            LauncherButton::CheckForUpdates => "VERA:CheckForUpdates",
            LauncherButton::EditLayout => "VERA:LayoutEdit",
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

    /// The pushed look: the button holds the press and the pointer is still
    /// on it (612B70).
    pub(crate) fn shell_button_pressed(&self, id: LauncherButton) -> bool {
        self.shell
            .node(Control::Button(id))
            .is_some_and(|node| self.shell.gui.shows_pressed(node))
    }

    /// The zoom the face shows: a fixed choice this window can't reach shows
    /// as the step it gives way to.
    pub(super) fn shell_selected_zoom(&self, layout: &LauncherOptionsLayout) -> Zoom {
        match self.zoom {
            Zoom::Auto => Zoom::Auto,
            fixed => Zoom::Percent(fixed.percent(layout.screen.0, layout.screen.1)),
        }
    }

    /// The sidebar size the face shows: a size this battle frame has no room
    /// for shows as the size it gives way to.
    pub(super) fn shell_selected_sidebar(&self, layout: &LauncherOptionsLayout) -> SidebarSize {
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
        use super::{ColorVisionMode, DisplayTarget, HealthBarMode, SidebarColumns};
        use LauncherOptionsEvent as E;
        let v = &self.values;
        match combo {
            LauncherComboId::Zoom => {
                let (screen, original_shape) =
                    (layout.screen, v[LauncherCheckboxId::OriginalShape]);
                let label = |zoom: &Zoom| super::zoom_label(*zoom, screen, original_shape);
                let choices = Zoom::choices(screen.0, screen.1);
                LauncherList::new(
                    choices,
                    &self.shell_selected_zoom(layout),
                    label,
                    E::ZoomSelected,
                )
            }
            LauncherComboId::Sidebar => {
                let (width, height) = layout.battle_frame;
                let choices = SidebarSize::choices(width, height);
                let shown = self.shell_selected_sidebar(layout);
                LauncherList::new(choices, &shown, |size| size.label(), E::SidebarSelected)
            }
            LauncherComboId::SidebarColumns => LauncherList::new(
                SidebarColumns::choices(),
                &v.sidebar_columns,
                |c| c.label(),
                E::SidebarColumnsSelected,
            ),
            LauncherComboId::HealthBars => LauncherList::new(
                HealthBarMode::choices(),
                &v.health_bars,
                |m| m.label(),
                E::HealthBarModeSelected,
            ),
            LauncherComboId::ColorVision => LauncherList::new(
                ColorVisionMode::choices(),
                &v.color_vision,
                |m| m.label(),
                E::ColorVisionModeSelected,
            ),
            LauncherComboId::Ares => LauncherList::new(
                super::ares_choices(),
                &v.ares,
                |s| super::ares_label(*s),
                E::AresSelected,
            ),
            LauncherComboId::GraphicsApi => LauncherList::new(
                super::GraphicsApi::choices(),
                &v.graphics_api,
                |api| api.label(),
                E::GraphicsApiSelected,
            ),
            LauncherComboId::Display => {
                let label = |target: &DisplayTarget| match target {
                    DisplayTarget::Auto => tr("VERA:DisplayAuto"),
                    DisplayTarget::Fixed(id) => self
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

    /// The status help the line shows, a string-table or `VERA:` key: what
    /// the last dialog hit test wrote. While a slider or button holds the
    /// mouse capture, or a list is open, the dialog gets no hit test and the
    /// text stays.
    pub(crate) fn shell_status_help(&self) -> Option<&str> {
        self.shell.status_help.as_deref()
    }

    /// Status help (`0x00604729..0x00604838`) for `control`.
    fn control_help(
        &self,
        control: Control,
        layout: &LauncherOptionsLayout,
    ) -> Option<&'static str> {
        let id = match control {
            Control::Label(_) => return None,
            // A slider greyed for want of sound keeps its own help.
            Control::Setting(Setting::Slider(id)) if self.in_match && id.fixed_in_match() => {
                return Some(NOT_IN_MATCH_HELP);
            }
            Control::Setting(setting) => return Some(setting.text().1),
            Control::Button(id) => id,
        };
        let in_game = layout.surface == OptionsSurface::InGame;
        Some(match id {
            LauncherButton::Category(category) => return category.help(),
            LauncherButton::InGameOptions => "VERA:InGameOptionsHelp",
            LauncherButton::InGameBack => "VERA:InGameOptionsBackHelp",
            LauncherButton::CheckForUpdates if self.button_enabled(id) => {
                "VERA:CheckForUpdatesHelp"
            }
            LauncherButton::CheckForUpdates => NOT_IN_MATCH_HELP,
            // Its help starts with "in a match", so the greyed button
            // says why as well.
            LauncherButton::EditLayout => "VERA:EditLayoutHelp",
            LauncherButton::Exit(LauncherParentResult::Mods) => {
                crate::ui::mods_shell::BUTTON_HELP_KEY
            }
            // The main menu's Esc box has no status line; the in-game
            // menu has.
            LauncherButton::SetWindowSize => return in_game.then_some("VERA:SetWindowSizeHelp"),
            // Retail's strings for the in-game Game Options buttons.
            LauncherButton::InGameKeyboard => IN_GAME_KEYBOARD_HELP,
            LauncherButton::Exit(LauncherParentResult::Keyboard) if in_game => {
                IN_GAME_KEYBOARD_HELP
            }
            LauncherButton::InGameSound => "STT:IGGameOptButtonSound",
            LauncherButton::Exit(LauncherParentResult::Keyboard) => COLUMN[0].tooltip_key,
            LauncherButton::Exit(LauncherParentResult::Back | LauncherParentResult::Terminal) => {
                OPTIONS_PAGE.back.tooltip_key
            }
        })
    }

    /// The layout's controls as nodes: their rects, values, texts, enabled
    /// states and help.
    fn shell_wanted(&self, layout: &LauncherOptionsLayout) -> Vec<(Control, NodeSpec)> {
        let esc_menu = match layout.surface {
            OptionsSurface::EscMenu(body) => Some(body),
            _ => None,
        };
        let control_spec = |control: Control, rect: RectPx| {
            let kind = match control {
                Control::Label(_) => NodeKind::Text,
                Control::Setting(Setting::Slider(_)) => NodeKind::Trackbar,
                Control::Setting(Setting::Checkbox(_)) => NodeKind::Checkbox,
                Control::Setting(Setting::List(_)) => NodeKind::Combo,
                Control::Button(_) => NodeKind::Button,
            };
            let mut node = NodeSpec::new(kind).with(Prop::Rect, Value::Rect(rect));
            if let Some(help) = self.control_help(control, layout) {
                node = node.tooltip(help);
            }
            match control {
                Control::Label(label) => {
                    let (caption, align) = self.page_label(layout, label);
                    node.with(Prop::Text, Value::Text(caption.into_owned()))
                        .with(Prop::Align, Value::Int(i32::from(align.0)))
                }
                Control::Setting(Setting::Slider(id)) => {
                    let base = id.value_base();
                    let position = base + i32::from(self.trackbar_position(id));
                    node.with(Prop::Min, Value::Int(base))
                        .with(Prop::Max, Value::Int(base + i32::from(id.maximum())))
                        .with(Prop::Step, Value::Int(1))
                        .with(Prop::Pos, Value::Int(position))
                        .with(
                            Prop::ShowValue,
                            Value::Bool(
                                if layout.category == Category::Enhancements
                                    && layout.surface == OptionsSurface::Launcher
                                {
                                    launcher_pages::shows_value(id)
                                } else {
                                    id.plaque_reserve() > 0
                                },
                            ),
                        )
                        .with(Prop::Enabled, Value::Bool(self.trackbar_enabled(id)))
                }
                // The boxes toggle on the press (`0x006163A0`).
                Control::Setting(Setting::Checkbox(id)) => {
                    node = node
                        .with(Prop::Checked, Value::Bool(self.shell_checkbox_checked(id)))
                        .with(
                            Prop::Text,
                            Value::Text(self.shell_checkbox_face_text(layout, id).into_owned()),
                        );
                    match esc_menu {
                        // The label and the Yes/No value are both part of this row.
                        Some(body) => node.with(
                            Prop::HitRect,
                            Value::Rect(RectPx {
                                x: body.x,
                                w: body.w,
                                ..rect
                            }),
                        ),
                        None => node,
                    }
                }
                // VERA's lists open from anywhere on their face; a pick
                // publishes even the row the face shows.
                Control::Setting(Setting::List(combo)) => {
                    let list = self.shell_list(combo, layout);
                    node = node
                        .with(Prop::Selected, Value::Int(list.shown.unwrap_or(0) as i32))
                        .with(Prop::Text, Value::Text(list.face))
                        .with(Prop::Items, Value::Items(list.rows))
                        // The face's text 3 px in, clear of the arrow.
                        .with(Prop::TextInset, Value::Rect(RectPx::new(3, 0, 20, 0)))
                        .with(Prop::OpenAnywhere, Value::Bool(true));
                    match esc_menu {
                        // Keep the list in the control column and lift it just
                        // enough to clear the footer. Extra installed languages
                        // can use the window above the panel as well.
                        Some(body) => node.with(Prop::PopupBottom, Value::Int(body.y + body.h)),
                        None => node,
                    }
                }
                // A disabled button takes the press and does nothing. The page
                // that shows keeps its column button lit (612B70 record+C5).
                Control::Button(id) => node
                    .with(Prop::Enabled, Value::Bool(self.button_enabled(id)))
                    .with(
                        Prop::Text,
                        Value::Text(self.shell_button_label(id).into_owned()),
                    )
                    .with(Prop::Toggled, Value::Bool(self.shell_button_selected(id))),
            }
        };
        layout
            .controls
            .iter()
            .map(|&(control, rect)| (control, control_spec(control, rect)))
            .collect()
    }

    /// Bring the tree in line with `layout` and the values. Every pointer
    /// message does first; the hosts do before they paint.
    pub(crate) fn shell_sync(&mut self, layout: &LauncherOptionsLayout) {
        let wanted = self.shell_wanted(layout);
        self.shell.sync(wanted, layout.scale);
    }

    /// A move: the tree updates hover, a held slider and the open list's hot
    /// row; the status line follows the hit test unless a press holds the
    /// mouse or a list is open.
    fn shell_pointer_moved(&mut self, x: i32, y: i32) {
        let held = self.shell.gui.pressed().is_some() || self.shell_list_open();
        self.shell.input(InputEvent::PointerMove { x, y });
        if !held {
            self.shell.status_help = page::hovered_help(&self.shell.gui, self.shell.gui.hovered());
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

    /// What the tree's widgets did, in order ([`page::page_events`]): their
    /// cues wait for the host, and the dialog acts on the rest. gamemd-derived:
    /// ordinary `0xD5` owner-draw buttons emit `GUIMainButtonSound` on
    /// admitted primary mouse-down; `OptionsClass__ShowLauncherDialog @
    /// 0x0055FC80` observes the distinct Back/Keyboard result only on the
    /// later activation release. Like gamemd's combo owner `0x00617250`, a
    /// press on a list emits GUIComboOpen first.
    fn shell_events(&mut self, layout: &LauncherOptionsLayout) {
        let raw = self.shell.gui.drain_events();
        for event in page::page_events(&self.shell.gui, raw) {
            let id = match event {
                PageEvent::Cue(cue) => {
                    self.pending_cues.push(cue);
                    continue;
                }
                PageEvent::Toggled(id, _)
                | PageEvent::Picked(id, _)
                | PageEvent::Slid { id, .. }
                | PageEvent::Activated(id) => id,
                _ => continue,
            };
            // A page switch earlier in the batch leaves no control here.
            let Some(control) = self.shell.control(id) else {
                continue;
            };
            match (event, control) {
                (PageEvent::Toggled(_, on), Control::Setting(Setting::Checkbox(id))) => {
                    self.set_checkbox(id, on)
                }
                (PageEvent::Slid { value, .. }, Control::Setting(Setting::Slider(id))) => {
                    if let Ok(position) = u8::try_from(value - id.value_base()) {
                        self.set_trackbar_position(id, position);
                    }
                }
                (PageEvent::Picked(_, row), Control::Setting(Setting::List(combo))) => {
                    let picks = self.shell_list(combo, layout).picks;
                    let row = usize::try_from(row).ok();
                    if let Some(pick) = row.and_then(|row| picks.into_iter().nth(row)) {
                        self.publish(pick);
                    }
                }
                (PageEvent::Activated(_), Control::Button(id)) => self.activate_button(id),
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

/// An open list, for tests: one row per choice, in order.
#[cfg(test)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct LauncherComboPopup {
    pub(crate) combo: LauncherComboId,
    pub(crate) rect: RectPx,
    pub(crate) row_height: i32,
    pub(crate) rows: usize,
    pub(crate) selected: usize,
    pub(crate) hovered: Option<usize>,
}

#[cfg(test)]
impl LauncherComboPopup {
    pub(crate) fn row_rect(self, row: usize) -> RectPx {
        let y = self.rect.y + row as i32 * self.row_height;
        RectPx::new(self.rect.x, y, self.rect.w, self.row_height)
    }
}

#[cfg(test)]
impl OptionsDialogState {
    pub(crate) fn shell_trackbar_thumb_left(
        &self,
        id: LauncherTrackbarId,
        window: RectPx,
        scale: ShellScale,
    ) -> i32 {
        vera20k_gui::shell::trackbar::thumb_left_at(
            i32::from(self.trackbar_position(id)),
            window,
            id.plaque_reserve(),
            i32::from(id.maximum()),
            scale,
        )
    }

    /// The slider holding the mouse, and whether it follows the pointer.
    #[cfg(test)]
    pub(crate) fn shell_trackbar_hold(&self) -> Option<(LauncherTrackbarId, bool)> {
        self.shell
            .held_slider()
            .map(|id| (id, self.shell.gui.dragging()))
    }

    /// The list that is open.
    pub(crate) fn shell_open_list(&self) -> Option<LauncherComboId> {
        Some(
            self.shell_popup(&LauncherOptionsLayout::new(800, 600, self.category))?
                .combo,
        )
    }

    /// The open list as the tree hangs it, and the row its face showed.
    pub(crate) fn shell_popup(&self, _: &LauncherOptionsLayout) -> Option<LauncherComboPopup> {
        let node = self.shell.gui.popup()?;
        let Some(Control::Setting(Setting::List(combo))) = self.shell.control_of(node) else {
            return None;
        };
        let open = self.shell.gui.combo_drop_down(node)?;
        let selected = match self.shell.gui.get(node, Prop::Selected) {
            Ok(Some(Value::Int(row))) => *row as usize,
            _ => 0,
        };
        Some(LauncherComboPopup {
            combo,
            rect: open.popup.rect,
            row_height: open.popup.row_height,
            rows: open.popup.total,
            selected,
            hovered: open.hot_row,
        })
    }
}

#[cfg(all(test, tests_ui))]
mod tests {
    use super::*;
    use crate::rules::house_colors::{STRENGTH_MAX, STRENGTH_RETAIL};
    use crate::ui::main_menu_dialogs::options::{
        DisplayChoice, DisplayTarget, HealthBarMode, LauncherOptionsLabels, LauncherOptionsValues,
    };
    use crate::ui::shell::pause_menu::{
        PauseMenuButton, PauseMenuLayout, PauseMenuTemplate, pause_menu_layout,
    };

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
            .checkbox_rect(id)
            .unwrap_or_else(|| panic!("{id:?} on the {:?} page", layout.category))
    }

    fn button(layout: &LauncherOptionsLayout, id: LauncherButton) -> RectPx {
        layout
            .button_rect(id)
            .unwrap_or_else(|| panic!("{id:?} on the {:?} page", layout.category))
    }

    fn buttons(layout: &LauncherOptionsLayout) -> Vec<(LauncherButton, RectPx)> {
        layout.buttons().collect()
    }

    fn trackbars(layout: &LauncherOptionsLayout) -> Vec<(LauncherTrackbarId, RectPx)> {
        let slider = |(setting, rect)| match setting {
            Setting::Slider(id) => Some((id, rect)),
            _ => None,
        };
        layout.settings().filter_map(slider).collect()
    }

    fn checkboxes(layout: &LauncherOptionsLayout) -> Vec<(LauncherCheckboxId, RectPx)> {
        let check = |(setting, rect)| match setting {
            Setting::Checkbox(id) => Some((id, rect)),
            _ => None,
        };
        layout.settings().filter_map(check).collect()
    }

    fn combos(layout: &LauncherOptionsLayout) -> Vec<(LauncherComboId, RectPx)> {
        let list = |(setting, rect)| match setting {
            Setting::List(id) => Some((id, rect)),
            _ => None,
        };
        layout.settings().filter_map(list).collect()
    }

    fn labels(layout: &LauncherOptionsLayout) -> Vec<(RectPx, PageLabel)> {
        let label = |&(control, rect): &(Control, RectPx)| match control {
            Control::Label(label) => Some((rect, label)),
            _ => None,
        };
        layout.controls.iter().filter_map(label).collect()
    }

    /// A box's caption.
    fn box_caption(state: &OptionsDialogState, id: LauncherCheckboxId) -> String {
        state.text(Setting::Checkbox(id).text().0).into_owned()
    }

    /// The status help after the pointer moves to `(x, y)`.
    fn help_at(
        state: &mut OptionsDialogState,
        x: i32,
        y: i32,
        layout: &LauncherOptionsLayout,
    ) -> Option<String> {
        state.shell_mouse_move(x, y, layout);
        state.shell_status_help().map(str::to_owned)
    }

    /// The frame's cues and events.
    fn drained(state: &mut OptionsDialogState) -> (Vec<PageCue>, Vec<LauncherOptionsEvent>) {
        let output = state.drain_output();
        (output.cues, output.events)
    }

    /// Where the page's label that reads `text` stands.
    fn label_rect(
        state: &OptionsDialogState,
        layout: &LauncherOptionsLayout,
        text: &str,
    ) -> Option<RectPx> {
        labels(layout)
            .into_iter()
            .find(|(_, label)| state.page_label(layout, *label).0 == text)
            .map(|(rect, _)| rect)
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
            for (id, rect) in trackbars(&layout) {
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
            for (id, rect) in combos(&layout) {
                assert_eq!(
                    layout.scale.base_rect(rect).w,
                    180,
                    "{id:?} on the {category:?} page"
                );
            }
        }
    }

    /// The launcher page's Mods stands on the tile right above Main Menu,
    /// and a press there opens Mods.
    #[test]
    fn launcher_mods_slot_and_gui_hit_rect_are_next_to_main_menu() {
        for (w, h) in [(640, 480), (800, 600), (1536, 1152)] {
            let layout = LauncherOptionsLayout::responsive(w, h, Category::LauncherMain);
            let mods = LauncherButton::Exit(LauncherParentResult::Mods);
            assert_eq!(layout.button_rect(mods).is_some(), HAS_MODS);
            if let Some(rect) = layout.button_rect(mods) {
                let back = button(&layout, LauncherButton::Exit(LauncherParentResult::Back));
                assert_eq!((rect.x, rect.y + rect.h, rect.w), (back.x, back.y, back.w));
                let mut dialog = state();
                dialog.shell_mouse_down(rect.x + 4, rect.y + 4, &layout);
                dialog.shell_mouse_up(rect.x + 4, rect.y + 4, &layout);
                assert_eq!(
                    dialog.drain_output().result,
                    Some(LauncherParentResult::Mods)
                );
            }
        }
    }

    #[test]
    fn enhancements_give_both_slider_tracks_the_same_value_reserve() {
        use LauncherTrackbarId::{HouseColorStrength, SidebarColumns};
        let layout = page(Category::Enhancements);
        let rect = |id| {
            layout
                .controls
                .iter()
                .find(|(control, _)| *control == Control::Setting(Setting::Slider(id)))
                .unwrap()
                .1
        };
        assert_eq!(
            (rect(SidebarColumns).x, rect(SidebarColumns).w),
            (rect(HouseColorStrength).x, rect(HouseColorStrength).w)
        );
        let dialog = state();
        for id in [SidebarColumns, HouseColorStrength] {
            let controls = dialog.shell_wanted(&layout);
            let (_, node) = controls
                .iter()
                .find(|(control, _)| *control == Control::Setting(Setting::Slider(id)))
                .unwrap();
            assert!(node.props.contains(&(Prop::ShowValue, Value::Bool(true))));
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
        let ids: Vec<_> = layout.buttons().map(|(id, _)| id).collect();
        assert_eq!(ids, expected);
        let tile = |row: i32| RectPx::new(644, 199 + 42 * row, 156, 42);
        let all = buttons(&layout);
        let (main_menu, stacked) = all.split_last().unwrap();
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
        let all = buttons(&layout);
        assert_eq!(all.len(), 8 + usize::from(HAS_MODS));
        for (index, (id, a)) in all.iter().enumerate() {
            assert!(a.x + a.w <= 640 && a.y + a.h <= 480, "{id:?} at {a:?}");
            for (other, b) in &all[index + 1..] {
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
                    .settings()
                    .map(|(_, rect)| rect)
                    .chain(layout.button_rect(LauncherButton::CheckForUpdates))
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
                .checkbox_rect(LauncherCheckboxId::CommanderFeedback)
                .is_some()
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
                for (id, rect) in trackbars(&layout) {
                    assert!(
                        trackbar_frame_slot(id.plaque_reserve() > 0, rect.w, rect.h).is_some(),
                        "{id:?} on {category:?} at {size:?} is {}x{}",
                        rect.w,
                        rect.h
                    );
                }
                for (id, rect) in combos(&layout) {
                    assert_eq!(rect.w, 180, "{id:?} on {category:?}");
                }
                let rects: Vec<RectPx> = layout
                    .settings()
                    .map(|(_, rect)| rect)
                    .chain(
                        layout
                            .buttons()
                            .filter(|(id, _)| id.on_page())
                            .map(|(_, rect)| rect),
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
            buttons(&landing)[0],
            (LauncherButton::InGameOptions, below(1))
        );
        assert_eq!(native.button(PauseMenuButton::Keyboard), Some(below(2)));
        assert_eq!(native.button(PauseMenuButton::Load), Some(below(3)));

        let mut dialog = state();
        dialog.select_category(Category::GameControls);
        let options = below(1);
        assert_eq!(
            help_at(&mut dialog, options.x + 2, options.y + 2, &landing),
            Some("VERA:InGameOptionsHelp".to_owned())
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
        assert_eq!(buttons(&page), expected);

        let general = LauncherButton::Category(Category::General);
        let rect = button(&page, general);
        assert_eq!(
            help_at(&mut dialog, rect.x + 2, rect.y + 2, &page),
            Some("VERA:GeneralPageHelp".to_owned())
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
            trackbars(layout)
                .into_iter()
                .map(|(id, _)| id)
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
            hidden.button_rect(LauncherButton::InGameKeyboard).is_none(),
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
        assert!(speed_events.is_empty());
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
            layout.button_rect(LauncherButton::SetWindowSize).is_some(),
            HAS_SET_WINDOW_SIZE
        );
    }

    #[test]
    fn a_stretched_slider_maps_the_pointer_back_to_its_authored_track() {
        let mut dialog = state();
        let base = page(Category::Audio);
        let scale = crate::ui::shell::geom::shell_scale(1600, 1200);
        let wide = LauncherOptionsLayout::responsive(1600, 1200, Category::Audio);
        let authored = base.trackbar_rect(LauncherTrackbarId::Score).unwrap();
        let stretched = wide.trackbar_rect(LauncherTrackbarId::Score).unwrap();
        assert_eq!(stretched, scale.scale_rect(authored));
        // The thumb travels by the same authored span, then stretches.
        let thumb = dialog.shell_trackbar_thumb_left(
            LauncherTrackbarId::Score,
            authored,
            crate::ui::shell::geom::RETAIL_SCALE,
        );
        let wide_thumb =
            dialog.shell_trackbar_thumb_left(LauncherTrackbarId::Score, stretched, scale);
        assert_eq!(wide_thumb, (thumb as f32 * scale.factor()).round() as i32);
        // The same authored point addressed on either page takes the same
        // thumb and the same position.
        let press = (authored.x + thumb + 5, authored.y + authored.h - 3);
        dialog.shell_mouse_down(press.0, press.1, &base);
        let held = Some((LauncherTrackbarId::Score, true));
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
        assert_eq!(
            (output.cues, output.events),
            (vec![PageCue::Button], vec![])
        );
        assert_eq!((output.result, output.check_for_updates), (None, false));
        // The pointer still rests on the button: its help stays up.
        assert_eq!(state.shell_status_help(), Some("VERA:AudioPageHelp"));
        let audio_page = page(Category::Audio);
        assert_eq!(state.shell_title_text(), "Options");
        assert_eq!(
            state.page_label(&audio_page, PageLabel::Heading).0,
            "Audio Options"
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
        let list =
            |state: &OptionsDialogState| state.shell_list(LauncherComboId::Language, &layout);
        assert_eq!(list(&state).rows, ["English", "简体中文"]);
        // The language in use changes nothing; another publishes its tag.
        let first = popup.row_rect(0);
        state.shell_mouse_down(first.x + 10, first.y + 5, &layout);
        let cues = vec![PageCue::ComboOpen, PageCue::ComboClose];
        assert_eq!(drained(&mut state), (cues.clone(), vec![]));
        state.shell_mouse_down(face.x + 10, face.y + 5, &layout);
        let second = popup.row_rect(1);
        state.shell_mouse_down(second.x + 10, second.y + 5, &layout);
        assert_eq!(list(&state).face, "简体中文");
        assert_eq!(
            drained(&mut state),
            (
                cues,
                vec![LauncherOptionsEvent::LanguageSelected("zh-CN".to_owned())]
            )
        );
    }

    #[test]
    fn esc_menu_controls_follow_the_widest_visible_caption() {
        let stats = tr("VERA:PerformanceStats");
        for width in [94, 166] {
            let layout = LauncherOptionsLayout::esc_menu(
                800,
                600,
                crate::ui::shell::geom::RETAIL_SCALE,
                |caption| {
                    if caption == stats { width } else { 40 }
                },
            );
            let face = layout.combo_rect(LauncherComboId::Language).unwrap();
            let gap = face.x - (labels(&layout)[0].0.x + width as i32);
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
        let mut state = state();
        let languages = (0..11)
            .map(|i| LanguageChoice {
                tag: i.to_string(),
                name: i.to_string(),
            })
            .collect();
        state.set_languages(languages, "0".to_owned());
        for (w, h) in [(640, 480), (1920, 1080)] {
            let scale = crate::ui::shell::geom::shell_scale_for_window(w, h);
            let layout = LauncherOptionsLayout::esc_menu(w, h, scale, |_| 100);
            let message_box = template_on(FOOTER_TEMPLATE, w, h, scale);
            let face = layout.combo_rect(LauncherComboId::Language).unwrap();
            let caption = |kind| layout.rect(Control::Label(kind)).unwrap();
            let language = caption(PageLabel::Localized("VERA:Language"));
            assert!(language.x + language.w <= face.x);
            assert!(language.y < face.y + face.h && face.y < language.y + language.h);
            for (id, rect) in &checkboxes(&layout) {
                let title = label_rect(&state, &layout, &box_caption(&state, *id)).unwrap();
                assert!(title.x == language.x && title.y < rect.y + rect.h);
                assert!(rect.y < title.y + title.h && rect.x == face.x);
            }
            if HAS_SET_WINDOW_SIZE {
                let size = button(&layout, LauncherButton::SetWindowSize);
                let title = caption(PageLabel::Localized(SET_WINDOW_SIZE_LABEL));
                assert_eq!((title.x, size.x), (language.x, face.x));
                assert!(title.y < size.y + size.h && size.y < title.y + title.h);
                assert!(size.y + size.h < message_box.button(0).y);
            }
            state.shell_mouse_down(face.x + 10, face.y + 5, &layout);
            let list = state.shell_popup(&layout).unwrap().rect;
            assert_eq!((list.x, list.w), (face.x, face.w));
            assert!(list.y >= message_box.dialog.y && list.y + list.h < message_box.button(0).y);
            assert!(state.shell_close_list());
            assert!(!state.shell_close_list());
            assert_eq!(
                drained(&mut state),
                (vec![PageCue::ComboOpen, PageCue::ComboClose], vec![])
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
        let layout =
            LauncherOptionsLayout::esc_menu(800, 600, crate::ui::shell::geom::RETAIL_SCALE, |_| {
                100
            });
        let rect = button(&layout, LauncherButton::SetWindowSize);
        let (x, y) = (rect.x + 2, rect.y + 2);
        state.shell_mouse_down(x, y, &layout);
        assert_eq!(drained(&mut state), (vec![PageCue::Button], vec![]));
        state.shell_mouse_up(x, y - 100, &layout);
        assert_eq!(drained(&mut state), (vec![], vec![]));
        state.shell_mouse_down(x, y, &layout);
        state.shell_mouse_up(x, y, &layout);
        assert_eq!(
            drained(&mut state),
            (
                vec![PageCue::Button],
                vec![LauncherOptionsEvent::SetWindowSizeRequested]
            )
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
        let layout =
            LauncherOptionsLayout::esc_menu(800, 600, crate::ui::shell::geom::RETAIL_SCALE, |_| {
                100
            });
        for id in [
            LauncherCheckboxId::FullScreen,
            LauncherCheckboxId::PerformanceStats,
        ] {
            let rect = checkbox(&layout, id);
            let title = label_rect(&state, &layout, &box_caption(&state, id)).unwrap();
            for (x, y) in [
                (rect.x + 2, rect.y + 2),
                (title.x + 2, title.y + 2),
                (rect.x + 30, rect.y + 2),
            ] {
                let checked = !state.shell_checkbox_checked(id);
                state.shell_mouse_down(x, y, &layout);
                assert!(
                    state
                        .drain_output()
                        .events
                        .contains(&LauncherOptionsEvent::Toggled(id, checked)),
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

    #[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
    #[test]
    fn high_refresh_checkbox_sends_both_choices_with_vsync_off() {
        let layout = LauncherOptionsLayout::new(640, 480, Category::Display);
        let mut dialog = state();
        dialog.values[LauncherCheckboxId::VSync] = false;
        let id = LauncherCheckboxId::HighRefreshRateInBattle;
        let rect = checkbox(&layout, id);
        assert!(!dialog.shell_checkbox_checked(id));
        for enabled in [true, false] {
            dialog.shell_mouse_down(rect.x + 2, rect.y + 2, &layout);
            dialog.shell_mouse_up(rect.x + 2, rect.y + 2, &layout);
            assert_eq!(dialog.shell_checkbox_checked(id), enabled);
            assert!(
                dialog
                    .drain_output()
                    .events
                    .contains(&LauncherOptionsEvent::Toggled(id, enabled))
            );
            assert!(!dialog.shell_checkbox_checked(LauncherCheckboxId::VSync));
        }
    }

    #[test]
    fn status_help_names_the_control_under_the_pointer() {
        let csf = |key: &str| Some(key.to_owned());
        let text = csf;
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
            text("VERA:ZoomHelp")
        );
        use LauncherCheckboxId::*;
        for (id, help) in [
            (FullScreen, "VERA:FullScreenHelp"),
            (OriginalShape, "VERA:OriginalShapeHelp"),
            (PerformanceStats, "VERA:PerformanceStatsHelp"),
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
                in_game((800, 600), category, false)
                    .button_rect(mods)
                    .is_none(),
                "{category:?}"
            );
        }
        let layout = page(Category::Display);
        let rect = button(&layout, mods);
        assert_eq!(state.shell_button_label(mods), "Mods");
        assert_eq!(
            help_at(&mut state, rect.x + 2, rect.y + 2, &layout),
            Some("VERA:ModsHelp".to_owned())
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
                    .button_rect(LauncherButton::CheckForUpdates)
                    .is_some(),
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
        let last_box = checkboxes(&layout).last().unwrap().1;
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
            Some(NOT_IN_MATCH_HELP.to_owned())
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
                page(category).button_rect(edit).is_some(),
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
        let last_box = checkboxes(&layout).last().unwrap().1;
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
            Some("VERA:EditLayoutHelp".to_owned())
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
            Some((LauncherTrackbarId::Score, true))
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
            Some((LauncherTrackbarId::Score, true))
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
        let held = Some((LauncherTrackbarId::Difficulty, false));
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
        assert_eq!(drained(&mut state), (vec![PageCue::ListClick], vec![]));
        state.shell_mouse_move(srect.x + 2, srect.y + 10, &gameplay);
        assert_eq!(
            state.shell_status_help(),
            Some("STT:MainOptSliderDifficulty")
        );
        state.shell_mouse_up(srect.x + srect.w - 2, srect.y + srect.h - 3, &gameplay);
        assert_eq!(state.shell_trackbar_hold(), None);
        // Main Menu's status help reads after hovering it.
        let back = button(&gameplay, LauncherButton::Exit(LauncherParentResult::Back));
        state.shell_mouse_move(back.x + 5, back.y + 5, &gameplay);
        assert_eq!(state.shell_status_help(), Some("STT:MainOptButtonBack"));
        assert_eq!(state.drain_output().result, None);
        // The Tooltips box: its caption takes no press; its 18x18 icon
        // toggles, and the box starts ticked.
        let interface = page(Category::Interface);
        let crect = checkbox(&interface, LauncherCheckboxId::Tooltips);
        state.shell_mouse_down(crect.x + 30, crect.y + 5, &interface);
        assert!(state.shell_checkbox_checked(LauncherCheckboxId::Tooltips));
        state.shell_mouse_down(crect.x + 5, crect.y + 5, &interface);
        assert!(!state.shell_checkbox_checked(LauncherCheckboxId::Tooltips));
        assert_eq!(drained(&mut state), (vec![PageCue::Checkbox], vec![]));
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
            (dismissed.cues, dismissed.events),
            (vec![PageCue::ComboOpen, PageCue::ComboClose], vec![])
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
            drained(&mut state),
            (
                vec![PageCue::ComboClose],
                vec![LauncherOptionsEvent::ZoomSelected(Zoom::Percent(100))]
            )
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
            drained(&mut state),
            (
                vec![PageCue::ComboClose],
                vec![LauncherOptionsEvent::SidebarSelected(
                    SidebarSize::ExtraLarge
                )]
            )
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
        assert_eq!(
            state
                .shell_list(LauncherComboId::Display, &page(Category::Display))
                .face,
            "Auto"
        );
        state.shell_mouse_down(face.x + 10, face.y + 5, &layout);
        assert_eq!(state.shell_open_list(), Some(LauncherComboId::Display));
        let popup = state.shell_popup(&layout).unwrap();
        assert_eq!(
            (popup.combo, popup.rows, popup.selected),
            (LauncherComboId::Display, 3, 0),
            "Auto, then the two screens"
        );
        assert_eq!(
            state.shell_list(LauncherComboId::Display, &layout).rows,
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
        assert_eq!(
            state
                .shell_list(LauncherComboId::Display, &page(Category::Display))
                .face,
            "Screen 2: 2560x1440 144Hz"
        );
        let display2 = DisplayTarget::Fixed("\\\\.\\DISPLAY2".to_owned());
        assert_eq!(
            drained(&mut state),
            (
                vec![PageCue::ComboClose],
                vec![LauncherOptionsEvent::DisplaySelected(display2)]
            )
        );
        // Back to Auto: the window returns to the system's own choice.
        state.shell_mouse_down(face.x + 10, face.y + 5, &layout);
        let popup = state.shell_popup(&layout).unwrap();
        assert_eq!(popup.selected, 2, "the picked screen shows selected");
        state.drain_output();
        let auto = popup.row_rect(0);
        state.shell_mouse_down(auto.x + 10, auto.y + 5, &layout);
        assert_eq!(
            state
                .shell_list(LauncherComboId::Display, &page(Category::Display))
                .face,
            "Auto"
        );
        assert_eq!(
            drained(&mut state),
            (
                vec![PageCue::ComboClose],
                vec![LauncherOptionsEvent::DisplaySelected(DisplayTarget::Auto)]
            )
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
        assert_eq!(
            state
                .shell_list(LauncherComboId::Display, &page(Category::Display))
                .face,
            "\\\\.\\DISPLAY9"
        );
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

    /// Every box toggles on its page with its cue and leaves the others
    /// alone: VERA's report the change at once, retail's three wait for the
    /// accepted pack.
    #[test]
    fn every_box_toggles_with_its_cue_and_vera_boxes_report_it() {
        for category in Category::ALL {
            let layout = page(category);
            for (id, rect) in checkboxes(&layout) {
                let mut state = state();
                let mut expected = state.values;
                let checked = !state.shell_checkbox_checked(id);
                expected[id] = checked;
                state.shell_mouse_down(rect.x + 5, rect.y + 5, &layout);
                assert_eq!(state.values, expected, "{id:?}");
                let events = if id.publishes() {
                    vec![LauncherOptionsEvent::Toggled(id, checked)]
                } else {
                    vec![]
                };
                assert_eq!(drained(&mut state), (vec![PageCue::Checkbox], events));
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
        assert_eq!(state.values.health_bars, HealthBarMode::Vanilla);
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
        assert_eq!(state.values.health_bars, HealthBarMode::AlwaysExceptFull);
        assert_eq!(
            drained(&mut state),
            (
                vec![PageCue::ComboClose],
                vec![LauncherOptionsEvent::HealthBarModeSelected(
                    HealthBarMode::AlwaysExceptFull
                )]
            )
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
            state.slider_step(LauncherTrackbarId::HouseColorStrength),
            "1"
        );

        // The strip admits from `bottom - 18`; the far right is the top level.
        let retail_pack = state.pack();
        state.shell_mouse_down(slider.x + slider.w - 2, slider.y + slider.h - 3, &layout);
        state.shell_mouse_up(slider.x + slider.w - 2, slider.y + slider.h - 3, &layout);
        assert_eq!(state.house_color_strength(), STRENGTH_MAX);
        assert_eq!(
            state.slider_step(LauncherTrackbarId::HouseColorStrength),
            "10"
        );
        assert_eq!(
            drained(&mut state),
            (
                vec![PageCue::ListClick],
                vec![LauncherOptionsEvent::HouseColorStrengthChanged(
                    STRENGTH_MAX
                )]
            ),
            "the notch click leads the level it stands for"
        );
        assert_eq!(state.pack(), retail_pack, "the retail pack is untouched");

        // Back to the left end: the original bands, and a level of 1.
        state.shell_mouse_down(slider.x + 1, slider.y + slider.h - 3, &layout);
        state.shell_mouse_up(slider.x + 1, slider.y + slider.h - 3, &layout);
        assert_eq!(state.house_color_strength(), STRENGTH_RETAIL);
        assert_eq!(
            drained(&mut state),
            (
                vec![PageCue::ListClick],
                vec![LauncherOptionsEvent::HouseColorStrengthChanged(
                    STRENGTH_RETAIL
                )]
            )
        );
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
