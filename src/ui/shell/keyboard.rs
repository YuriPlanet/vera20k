//! Shared Keyboard A3 control state and physical geometry. Native5FBEF0/5FB320,
//! RT_DIALOG A3 atBEF560; bindings remain in the app command owner.
use vera20k_gui::{NodeId, NodeKind, NodeSpec, Prop, Value};

use super::controller::DialogController;
use super::geom::{self, RectPx, ShellScale};
use super::in_game_shell::{self, InGameShellLayout};

/// A3 as a front-end family page: no top buttons, Back on the bottom row,
/// heading `0x694` and status line `0x695` (template at `0x00BEF560`).
pub const KEYBOARD_PAGE: super::menu_page::MenuPageSpec = super::menu_page::MenuPageSpec {
    dialog: super::descriptor::DialogResourceId(0x00A3),
    title_key: "GUI:KeyboardOptions",
    stacked: &[],
    back: super::menu_page::MenuPageButtonSpec {
        id: 0x0686,
        dlu_top: 346,
        csf_key: "GUI:Back",
        tooltip_key: "STT:KeyboardButtonBack",
        result: None,
    },
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyboardParent {
    Launcher,
    GameControls,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyboardButton {
    Back,
    Assign,
    ResetAll,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyboardControl {
    Button(KeyboardButton),
    Category,
    Commands,
    Description,
    Capture,
    CurrentShortcut,
    CurrentOwner,
    Error,
}
/// The children in the order the pointer finds them (the buttons, then the
/// others, the description group last), with the template's control ids
/// (the group box 0x6E9 holds the description) and status help.
const CONTROLS: [(KeyboardControl, u16, &str); 10] = [
    (
        KeyboardControl::Button(KeyboardButton::Back),
        0x686,
        "STT:KeyboardButtonBack",
    ),
    (
        KeyboardControl::Button(KeyboardButton::Assign),
        0x4CF,
        "STT:KeyboardButtonAssign",
    ),
    (
        KeyboardControl::Button(KeyboardButton::ResetAll),
        0x4D0,
        "STT:KeyboardButtonResetAll",
    ),
    (
        KeyboardControl::Category,
        0x4C7,
        "STT:KeyboardComboCategory",
    ),
    (KeyboardControl::Commands, 0x4C8, "STT:KeyboardListCommands"),
    (KeyboardControl::Capture, 0x4CD, "STT:KeyboardEditEntry"),
    (
        KeyboardControl::CurrentShortcut,
        0x63C,
        "STT:KeyboardLabelShortcut",
    ),
    (
        KeyboardControl::CurrentOwner,
        0x63B,
        "STT:KeyboardLabelAssigned",
    ),
    (KeyboardControl::Error, 0x63D, "STT:KeyboardLabelError"),
    (
        KeyboardControl::Description,
        0x6E9,
        "STT:KeyboardGroupDescription",
    ),
];

impl KeyboardControl {
    fn row(self) -> (KeyboardControl, u16, &'static str) {
        CONTROLS
            .into_iter()
            .find(|row| row.0 == self)
            .expect("every control has a row")
    }
    pub fn id(self) -> u16 {
        self.row().1
    }
    pub fn from_id(id: u16) -> Option<Self> {
        CONTROLS
            .into_iter()
            .find(|row| row.1 == id)
            .map(|row| row.0)
    }
    pub fn help_key(self) -> &'static str {
        self.row().2
    }
}

/// IDCANCEL: Escape closes A3 without saving (`0x005FB6D4`); no button
/// shows it.
pub const CANCEL: u16 = 2;

/// A3's children with their status help. The capture field is a static that
/// takes the focus on a press and reads the keys itself (A3 interprets
/// them); a press on the category face opens its list anywhere and takes the
/// focus off the field. Every press on a command row selects it, and the
/// wheel moves the list three rows a notch.
fn spec() -> NodeSpec {
    let focus = |node: NodeSpec| node.with(Prop::FocusOnPress, Value::Bool(true));
    let controls = CONTROLS.map(|(control, id, help)| {
        let node = match control {
            KeyboardControl::Button(_) => NodeSpec::new(NodeKind::Button),
            KeyboardControl::Commands => NodeSpec::new(NodeKind::List)
                .with(Prop::RowCount, Value::Int(0))
                .with(Prop::WheelStep, Value::Int(1)),
            KeyboardControl::Category => focus(NodeSpec::new(NodeKind::Combo))
                .with(Prop::RowCount, Value::Int(0))
                .with(Prop::OpenAnywhere, Value::Bool(true))
                // The open list's wheel steps the selection and closes it,
                // silently (the combo's own step: CBN_SELCHANGE, no cue).
                .with(Prop::WheelPicks, Value::Bool(true)),
            KeyboardControl::Capture => focus(NodeSpec::new(NodeKind::Text)),
            _ => NodeSpec::new(NodeKind::Text),
        };
        node.id(id.into()).tooltip(help)
    });
    let cancel = NodeSpec::new(NodeKind::Button)
        .id(CANCEL.into())
        .with(Prop::Cancel, Value::Bool(true));
    NodeSpec::new(NodeKind::Dialog)
        .children(controls)
        .child(cancel)
}

#[derive(Debug, Clone)]
pub struct KeyboardCommandRow {
    pub command_index: usize,
    pub name: String,
    pub category: String,
    pub description: String,
}
#[derive(Debug)]
pub struct KeyboardState {
    pub(crate) title: super::static_reveal::PresentedKind1Static,
    pub parent: KeyboardParent,
    pub commands: Vec<KeyboardCommandRow>,
    pub categories: Vec<String>,
    pub category: usize,
    pub rows: Vec<usize>,
    pub captured: u16,
    /// 464 clears63B independently of the retained capture field.
    pub current_owner_visible: bool,
    pub error: Option<&'static str>,
    /// The controls; the command list holds its selection and top, the
    /// category combo its open list and hot row, the gui which control has
    /// the focus.
    pub ui: DialogController,
}
impl KeyboardState {
    /// `trailing_categories` are listed after every category the commands
    /// carry, whatever the sort would do with them: VERA's own category must
    /// stay at the end and must exist before any command is registered in it.
    pub fn new(
        parent: KeyboardParent,
        commands: Vec<KeyboardCommandRow>,
        trailing_categories: Vec<String>,
    ) -> Self {
        // CBS_SORT/LBS_SORT: sort localized strings, preserving command identity.
        let mut categories: Vec<_> = commands.iter().map(|r| r.category.clone()).collect();
        categories.sort_by_key(|s| s.to_lowercase());
        categories.dedup();
        categories.retain(|category| !trailing_categories.contains(category));
        for category in trailing_categories {
            if !categories.contains(&category) {
                categories.push(category);
            }
        }
        let mut result = Self {
            title: super::static_reveal::PresentedKind1Static::new(
                super::static_reveal::HEADING_KIND1,
            ),
            parent,
            commands,
            categories,
            category: 0,
            rows: Vec::new(),
            captured: 0,
            current_owner_visible: false,
            error: None,
            ui: DialogController::default(),
        };
        result.ui.reset_to(KEYBOARD_PAGE.dialog);
        if let Some(root) = result.root() {
            let gui = result.ui.gui_mut();
            super::page::ensure_built(gui, root, spec);
            let count = Value::Int(result.categories.len() as i32);
            super::page::set(
                gui,
                root,
                KeyboardControl::Category.id(),
                Prop::RowCount,
                count,
            );
        }
        result.select_category(0);
        result
    }
    pub fn select_category(&mut self, category: usize) {
        self.ui.gui_mut().close_popup();
        // 5FB500..5FB514 compares the retained category index before reset.
        if category == self.category && !self.rows.is_empty() {
            return;
        }
        self.rebuild_category(category);
    }
    pub fn reset_categories(&mut self) {
        self.rebuild_category(0);
    }
    fn rebuild_category(&mut self, category: usize) {
        if category >= self.categories.len() {
            return;
        }
        self.category = category;
        self.rows = self
            .commands
            .iter()
            .enumerate()
            .filter(|(_, r)| r.category == self.categories[category])
            .map(|(i, _)| i)
            .collect();
        self.rows
            .sort_by_key(|i| self.commands[*i].name.to_lowercase());
        // 5FB60E targets static63B, not list4C8: initial selection stays absent.
        let count = self.rows.len() as i32;
        if let Some(node) = self.list_node() {
            let gui = self.ui.gui_mut();
            let _ = gui.set(node, Prop::RowCount, Value::Int(count));
            let _ = gui.set(node, Prop::Selected, Value::Int(-1));
            let _ = gui.set(node, Prop::Top, Value::Int(0));
        }
        if let Some(node) = self.category_node() {
            let gui = self.ui.gui_mut();
            let _ = gui.set(node, Prop::Selected, Value::Int(category as i32));
            gui.close_popup();
        }
        self.current_owner_visible = false;
    }
    fn category_node(&self) -> Option<NodeId> {
        self.ui
            .gui()
            .find_id(self.root()?, KeyboardControl::Category.id().into())
    }
    /// The category list is open.
    pub fn category_open(&self) -> bool {
        self.category_node()
            .is_some_and(|node| self.ui.gui().combo_drop_down(node).is_some())
    }
    /// The open category list's lit row: the one under the pointer, or the
    /// selection until the pointer reaches a row.
    pub fn category_hot_row(&self) -> Option<usize> {
        let node = self.category_node()?;
        self.ui.gui().combo_drop_down(node)?.hot_row
    }
    fn root(&self) -> Option<NodeId> {
        self.ui.root_in(KEYBOARD_PAGE.dialog)
    }
    fn list_node(&self) -> Option<NodeId> {
        self.ui
            .gui()
            .find_id(self.root()?, KeyboardControl::Commands.id().into())
    }
    /// The command list's view: top, selection and held scroll part.
    pub fn list(&self) -> vera20k_gui::ListView {
        let view = self
            .list_node()
            .and_then(|node| self.ui.gui().list_view(node));
        view.unwrap_or(vera20k_gui::ListView {
            rows: 0,
            top: 0,
            selected: None,
            pressed: None,
        })
    }
    pub fn selected(&self) -> Option<usize> {
        self.list().selected
    }
    fn capture_node(&self) -> Option<NodeId> {
        self.ui
            .gui()
            .find_id(self.root()?, KeyboardControl::Capture.id().into())
    }
    /// The capture field has the focus: the keys are a hotkey to capture.
    /// It is a static, not a gui `Edit`: it shows the hotkey, not typed text.
    pub fn capture_focused(&self) -> bool {
        self.capture_node().is_some() && self.ui.gui().focused_control() == self.capture_node()
    }
    pub fn selected_command(&self) -> Option<&KeyboardCommandRow> {
        self.selected()
            .and_then(|i| self.rows.get(i))
            .map(|i| &self.commands[*i])
    }
    pub fn select_command(&mut self, row: usize) {
        if row >= self.rows.len() {
            return;
        }
        if let Some(node) = self.list_node() {
            let _ = self
                .ui
                .gui_mut()
                .set(node, Prop::Selected, Value::Int(row as i32));
        }
        self.current_owner_visible = false;
        self.captured = 0;
        if let Some(node) = self.capture_node() {
            let _ = self.ui.gui_mut().focus_control(node);
        }
    }
    pub fn reset_interaction(&mut self) {
        let gui = self.ui.gui_mut();
        gui.cancel_pointer();
        gui.close_popup();
    }
}

/// The captions (statics `0xFFFF`): key and whether centred.
const LABELS: [(u16, &str, bool); 6] = [
    (0xFF00, "GUI:Category", false),
    (0xFF01, "GUI:Commands", false),
    (0xFF02, "GUI:PressShortcut", false),
    (0xFF03, "GUI:CustomizeKeyboard", true),
    (0xFF04, "GUI:CurAssignedTo", false),
    (0xFF05, "GUI:CurrentShortcut", false),
];
/// The description text inside group box `0x6E9`.
const DESCRIPTION_TEXT: u16 = 0xFF06;

#[derive(Debug, Clone, Copy)]
pub struct KeyboardLayout {
    pub title: RectPx,
    pub back: RectPx,
    pub footer: RectPx,
    pub category: RectPx,
    pub commands: RectPx,
    pub group: RectPx,
    pub description: RectPx,
    pub capture: RectPx,
    pub assign: RectPx,
    pub reset: RectPx,
    pub current_owner: RectPx,
    pub current_shortcut: RectPx,
    pub error: RectPx,
    pub labels: [(&'static str, RectPx, bool); 6],
    /// The mapping the control rects were placed in: retail centers the authored
    /// 800x600 dialog, the responsive page scales it to fit the window.
    pub scale: ShellScale,
}
impl KeyboardLayout {
    /// Gamemd's A3: the authored dialog centered in the window at its native
    /// size. Also the frame the in-game page sits in, which never scales.
    pub fn new(width: i32, height: i32, shell: Option<(InGameShellLayout, [i32; 2])>) -> Self {
        Self::build(width, height, shell, crate::ui::shell::geom::RETAIL_SCALE)
    }

    /// The responsive variant of [`Self::new`] for the front-end family page: the
    /// verified 800x600 controls scaled to fit the window, with the family
    /// chrome where every other scaled family page puts it. Identity at the
    /// base size. The in-game frame keeps its own layout, so it takes `new`.
    pub fn responsive(width: i32, height: i32) -> Self {
        Self::build(
            width,
            height,
            None,
            crate::ui::shell::geom::shell_scale(width, height),
        )
    }

    fn build(
        width: i32,
        height: i32,
        shell: Option<(InGameShellLayout, [i32; 2])>,
        scale: ShellScale,
    ) -> Self {
        // Common60B7A0 ordinary controls use signed centering then clamp origins.
        let dlu = |template: RectPx| {
            let r = geom::dlu_rect(template.x, template.y, template.w, template.h);
            if scale.is_unscaled() {
                in_game_shell::ordinary_child_rect(r, width, height)
            } else {
                scale.scale_rect(r)
            }
        };
        let (title, back, footer) = if let Some((shell, size)) = shell {
            (
                in_game_shell::right_panel_title_rect(width),
                shell.lower_button_rect(width, size),
                in_game_shell::status_line_rect(height),
            )
        } else if scale.is_unscaled() {
            // The front-end page is a family page: its heading, Back and
            // status line sit where every family page puts them.
            let page =
                super::menu_page::compute_layout(&KEYBOARD_PAGE, width as u32, height as u32);
            (page.title, page.buttons[0].rect, page.status_help)
        } else {
            let page = super::menu_page::compute_responsive_layout(
                &KEYBOARD_PAGE,
                width as u32,
                height as u32,
            );
            (page.title, page.buttons[0].rect, page.status_help)
        };
        let rects = super::layout_data::map("0xA3", dlu);
        let rect = |id| rects.get(id).unwrap_or_default();
        let control = |control: KeyboardControl| rect(control.id());
        let mut category = control(KeyboardControl::Category);
        category.h =
            (crate::ui::skirmish_shell::COMBO_FACE_H as f32 * scale.factor()).round() as i32;
        Self {
            title,
            back,
            footer,
            category,
            commands: control(KeyboardControl::Commands),
            group: control(KeyboardControl::Description),
            description: rect(DESCRIPTION_TEXT),
            capture: control(KeyboardControl::Capture),
            assign: control(KeyboardControl::Button(KeyboardButton::Assign)),
            reset: control(KeyboardControl::Button(KeyboardButton::ResetAll)),
            current_owner: control(KeyboardControl::CurrentOwner),
            current_shortcut: control(KeyboardControl::CurrentShortcut),
            error: control(KeyboardControl::Error),
            labels: LABELS.map(|(id, key, centred)| (key, rect(id), centred)),
            scale,
        }
    }
    pub fn button(self, button: KeyboardButton) -> RectPx {
        match button {
            KeyboardButton::Back => self.back,
            KeyboardButton::Assign => self.assign,
            KeyboardButton::ResetAll => self.reset,
        }
    }
    /// Control `id`'s rect.
    pub fn rect(self, id: u16) -> Option<RectPx> {
        Some(match KeyboardControl::from_id(id)? {
            KeyboardControl::Button(button) => self.button(button),
            KeyboardControl::Category => self.category,
            KeyboardControl::Commands => self.commands,
            KeyboardControl::Capture => self.capture,
            KeyboardControl::CurrentShortcut => self.current_shortcut,
            KeyboardControl::CurrentOwner => self.current_owner,
            KeyboardControl::Error => self.error,
            KeyboardControl::Description => self.group,
        })
    }
    /// Combo owner618AEA sets font+6=23px dropdown rows, independently of lists.
    /// The popup is authored on the face and stretched with it, so a stretched
    /// page's list keeps the same row count and pitch as its face.
    pub fn category_popup(self, count: usize) -> RectPx {
        self.scale.derive_rect(self.category, |face| {
            RectPx::new(
                face.x,
                face.y + face.h,
                face.w,
                count as i32 * crate::ui::skirmish_shell::COMBO_DROPDOWN_ROW_H + 4,
            )
        })
    }
    /// The rows inside the popup's two-pixel frame: the category combo's
    /// `PopupRect`, so its rows hit where they paint.
    pub fn category_rows(self, count: usize) -> RectPx {
        let popup = self.category_popup(count);
        RectPx::new(popup.x + 2, popup.y + 2, popup.w - 4, popup.h - 4)
    }
}

#[cfg(all(test, tests_ui))]
mod tests {
    use super::*;
    #[test]
    fn ordinary_a3_rectangles_match_original_60b7a0_execution() {
        let fixture: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
            "tools/storage_oracle/keyboard_shell_layout.json",
        ))
        .unwrap();
        let cases = fixture["cases"].as_array().unwrap();
        assert_eq!(cases.len(), 48);
        for case in cases {
            let layout = KeyboardLayout::new(
                case["width"].as_i64().unwrap() as i32,
                case["height"].as_i64().unwrap() as i32,
                None,
            );
            let index = case["resource_index"].as_u64().unwrap();
            let mut rect = match index {
                2 => layout.labels[0].1,
                3 => layout.category,
                4 => layout.labels[1].1,
                5 => layout.commands,
                6 => layout.labels[2].1,
                7 => layout.group,
                8 => layout.description,
                9 => layout.capture,
                10 => layout.assign,
                11 => layout.labels[3].1,
                12 => layout.labels[4].1,
                13 => layout.reset,
                14 => layout.current_owner,
                15 => layout.current_shortcut,
                16 => layout.labels[5].1,
                17 => layout.error,
                _ => panic!("unexpected original child {index}"),
            };
            // 60B7A0 sees the full combo template rectangle; owner617250 later
            // replaces its collapsed-face height with literal24.
            if index == 3 {
                rect.h = case["resource_rect"][3].as_i64().unwrap() as i32;
            }
            let expected: Vec<i32> = case["rect"]
                .as_array()
                .unwrap()
                .iter()
                .map(|v| v.as_i64().unwrap() as i32)
                .collect();
            assert_eq!(
                [rect.x, rect.y, rect.w, rect.h].as_slice(),
                expected,
                "resource child {index}, width{}",
                case["width"]
            );
        }
    }
    #[test]
    fn a_stretched_keyboard_page_moves_its_popup_rows_with_the_face() {
        use crate::ui::skirmish_shell::{COMBO_DROPDOWN_ROW_H, COMBO_FACE_H, DropdownMetrics};

        // At the authored size the responsive page is the retail page.
        let retail = KeyboardLayout::new(800, 600, None);
        let base = KeyboardLayout::responsive(800, 600);
        assert_eq!(base.category, retail.category);
        assert_eq!(base.commands, retail.commands);
        assert_eq!(base.title, retail.title);
        assert_eq!(base.back, retail.back);

        let stretched = KeyboardLayout::responsive(1600, 1200);
        assert_eq!(stretched.category.h, COMBO_FACE_H * 2);
        assert_eq!(stretched.category.w, retail.category.w * 2);

        // The paint path draws row `i` at `popup.y + 2 + i * scaled row_h`; a
        // click at that row's centre has to select it.
        for (layout, row_h) in [
            (retail, COMBO_DROPDOWN_ROW_H),
            (stretched, DropdownMetrics::scaled(stretched.scale).row_h),
        ] {
            assert_eq!(
                DropdownMetrics::scaled(layout.scale).row_h,
                row_h,
                "layout rows and paint pitch agree"
            );
            let rows = (0..4)
                .map(|i| KeyboardCommandRow {
                    command_index: i,
                    name: format!("Command {i}"),
                    category: format!("Category {i}"),
                    description: String::new(),
                })
                .collect();
            let mut state = KeyboardState::new(KeyboardParent::Launcher, rows, Vec::new());
            let count = state.categories.len();
            let popup = layout.category_popup(count);
            assert_eq!(popup.x, layout.category.x);
            assert_eq!(popup.w, layout.category.w);
            state.ui.set_scale(layout.scale);
            let root = state.root().unwrap();
            let gui = state.ui.gui_mut();
            super::super::page::sync_rects(gui, root, |id| layout.rect(id));
            let rows = Value::Rect(layout.category_rows(count));
            super::super::page::set(
                gui,
                root,
                KeyboardControl::Category.id(),
                Prop::PopupRect,
                rows,
            );
            for i in 0..count {
                let face = layout.category;
                state.ui.on_pointer_move(face.x + 2, face.y + 2);
                state.ui.on_pointer_down(face.x + 2, face.y + 2);
                assert!(state.category_open());
                let y = popup.y + 2 + i as i32 * row_h + row_h / 2;
                state.ui.on_pointer_move(popup.x + row_h, y);
                assert_eq!(state.category_hot_row(), Some(i), "row {i} of {face:?}");
                state.ui.on_pointer_down(popup.x + row_h, y);
                assert!(!state.category_open());
            }
        }
    }
    #[test]
    fn the_gui_moves_the_focus_between_the_category_and_the_capture_field() {
        use crate::ui::shell::page::{PageEvent, page_events};
        let rows = vec![KeyboardCommandRow {
            command_index: 0,
            name: "Stop".into(),
            category: "Control".into(),
            description: "stop".into(),
        }];
        let mut state = KeyboardState::new(KeyboardParent::Launcher, rows, Vec::new());
        let layout = KeyboardLayout::new(800, 600, None);
        let root = state.root().unwrap();
        super::super::page::sync_rects(state.ui.gui_mut(), root, |id| layout.rect(id));
        let press = |state: &mut KeyboardState, r: RectPx| {
            state.ui.on_pointer_move(r.x + 2, r.y + 2);
            state.ui.on_pointer_down(r.x + 2, r.y + 2);
            state.ui.on_pointer_up(r.x + 2, r.y + 2);
            state.ui.gui_mut().close_popup();
        };
        assert!(!state.capture_focused());
        press(&mut state, layout.capture);
        assert!(state.capture_focused());
        press(&mut state, layout.assign);
        assert!(state.capture_focused(), "a button leaves the focus");
        press(&mut state, layout.category);
        assert!(!state.capture_focused());
        state.select_command(0);
        assert!(state.capture_focused());
        state.ui.drain_events();
        state
            .ui
            .on_input(vera20k_gui::InputEvent::Key(vera20k_gui::Key::Escape));
        let events = state.ui.drain_events();
        assert!(page_events(state.ui.gui(), events).contains(&PageEvent::Activated(CANCEL)));
    }

    /// VERA's own category is listed after every retail one even though it
    /// sorts before them, so an enhancement's keys never land among the
    /// original ones and the category exists before its first command does.
    #[test]
    fn trailing_categories_follow_every_command_category() {
        let rows = vec![
            KeyboardCommandRow {
                command_index: 0,
                name: "Stop".into(),
                category: "Control".into(),
                description: "stop".into(),
            },
            KeyboardCommandRow {
                command_index: 1,
                name: "Health".into(),
                category: "Selection".into(),
                description: "health".into(),
            },
        ];
        // "AaaVera" sorts first, so finding it last proves it is appended;
        // A repeated trailing label must not appear twice.
        let state = KeyboardState::new(
            KeyboardParent::Launcher,
            rows,
            vec!["AaaVera".into(), "AaaVera".into()],
        );
        assert_eq!(state.categories, ["Control", "Selection", "AaaVera"]);
        assert_eq!(state.category, 0, "the retail category opens selected");
    }

    #[test]
    fn populated_trailing_category_stays_last_and_keeps_command_identity() {
        let rows = vec![
            KeyboardCommandRow {
                command_index: 7,
                name: "Extension".into(),
                category: "AaaVera".into(),
                description: "extension".into(),
            },
            KeyboardCommandRow {
                command_index: 8,
                name: "Stop".into(),
                category: "Control".into(),
                description: "stop".into(),
            },
        ];
        let mut state = KeyboardState::new(
            KeyboardParent::Launcher,
            rows,
            vec!["AaaVera".into(), "AaaVera".into()],
        );
        assert_eq!(state.categories, ["Control", "AaaVera"]);
        state.select_category(1);
        state.select_command(0);
        assert_eq!(state.selected_command().unwrap().command_index, 7);
    }

    #[test]
    fn sorted_categories_and_commands_keep_identity_and_no_initial_selection() {
        let rows = vec![
            KeyboardCommandRow {
                command_index: 9,
                name: "Stop".into(),
                category: "Control".into(),
                description: "stop".into(),
            },
            KeyboardCommandRow {
                command_index: 5,
                name: "Health".into(),
                category: "Selection".into(),
                description: "health".into(),
            },
            KeyboardCommandRow {
                command_index: 2,
                name: "Alliance".into(),
                category: "Control".into(),
                description: "alliance".into(),
            },
        ];
        let mut state = KeyboardState::new(KeyboardParent::Launcher, rows, Vec::new());
        assert_eq!(state.categories, ["Control", "Selection"]);
        assert!(state.selected_command().is_none());
        state.select_command(0);
        assert_eq!(state.selected_command().unwrap().command_index, 2);
        assert!(state.capture_focused());
        state.captured = 81;
        state.error = Some("Error:CannotMap");
        state.current_owner_visible = true;
        state.select_category(0);
        assert_eq!(state.captured, 81);
        assert_eq!(state.selected(), Some(0));
        assert!(!state.category_open());
        state.reset_categories();
        assert!(state.selected().is_none());
        assert_eq!(state.captured, 81);
        assert!(!state.current_owner_visible);
        state.select_command(1);
        assert_eq!(state.error, Some("Error:CannotMap"));
        assert_eq!(state.captured, 0);
        assert_eq!(state.selected_command().unwrap().command_index, 9);
        state.captured = 81;
        state.current_owner_visible = true;
        state.select_category(1);
        assert_eq!(state.captured, 81);
        assert_eq!(state.error, Some("Error:CannotMap"));
        assert!(!state.current_owner_visible);
        assert!(state.selected_command().is_none());
        assert_eq!(state.rows, [1]);
    }
}
