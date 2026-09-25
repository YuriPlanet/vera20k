//! Shared Keyboard A3 control state and physical geometry. Native5FBEF0/5FB320,
//! RT_DIALOG A3 atBEF560; bindings remain in the app command owner.
use super::button::ShellButtonInteraction;
use super::geom::{self, RectPx};
use super::in_game_shell::InGameShellLayout;
use super::list::ListScrollInteraction;

/// A3 as a front-end family page: no top buttons, Back on the bottom row,
/// heading `0x694` and status line `0x695` (template at `0x00BEF560`).
pub const KEYBOARD_PAGE: super::menu_page::MenuPageSpec = super::menu_page::MenuPageSpec {
    dialog: super::descriptor::DialogId(0x00A3),
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
impl KeyboardControl {
    pub fn help_key(self) -> &'static str {
        match self {
            Self::Button(KeyboardButton::Back) => "STT:KeyboardButtonBack",
            Self::Button(KeyboardButton::Assign) => "STT:KeyboardButtonAssign",
            Self::Button(KeyboardButton::ResetAll) => "STT:KeyboardButtonResetAll",
            Self::Category => "STT:KeyboardComboCategory",
            Self::Commands => "STT:KeyboardListCommands",
            Self::Description => "STT:KeyboardGroupDescription",
            Self::Capture => "STT:KeyboardEditEntry",
            Self::CurrentShortcut => "STT:KeyboardLabelShortcut",
            Self::CurrentOwner => "STT:KeyboardLabelAssigned",
            Self::Error => "STT:KeyboardLabelError",
        }
    }
}
#[derive(Debug, Clone)]
pub struct KeyboardCommandRow {
    pub command_index: usize,
    pub name: String,
    pub category: String,
    pub description: String,
}
#[derive(Debug, Clone)]
pub struct KeyboardState {
    pub(crate) title: super::static_reveal::Kind1StaticReveal,
    pub(crate) title_receipt: Option<super::static_reveal::Kind1RevealReceipt>,
    pub parent: KeyboardParent,
    pub commands: Vec<KeyboardCommandRow>,
    pub categories: Vec<String>,
    pub category: usize,
    pub rows: Vec<usize>,
    pub selected: Option<usize>,
    pub top: usize,
    pub captured: u16,
    /// 464 clears63B independently of the retained capture field.
    pub current_owner_visible: bool,
    pub error: Option<&'static str>,
    pub capture_focused: bool,
    pub category_open: bool,
    pub category_hovered: Option<usize>,
    pub buttons: ShellButtonInteraction<KeyboardButton>,
    pub hovered: Option<KeyboardControl>,
    pub scroll: ListScrollInteraction,
}
impl KeyboardState {
    pub fn new(parent: KeyboardParent, commands: Vec<KeyboardCommandRow>) -> Self {
        // CBS_SORT/LBS_SORT: sort localized strings, preserving command identity.
        let mut categories: Vec<_> = commands.iter().map(|r| r.category.clone()).collect();
        categories.sort_by_key(|s| s.to_lowercase());
        categories.dedup();
        let mut result = Self {
            title: Default::default(),
            title_receipt: None,
            parent,
            commands,
            categories,
            category: 0,
            rows: Vec::new(),
            selected: None,
            top: 0,
            captured: 0,
            current_owner_visible: false,
            error: None,
            capture_focused: false,
            category_open: false,
            category_hovered: None,
            buttons: Default::default(),
            hovered: None,
            scroll: Default::default(),
        };
        result.select_category(0);
        result
    }
    pub fn select_category(&mut self, category: usize) {
        self.category_open = false;
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
        self.scroll.cancel();
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
        self.selected = None;
        self.top = 0;
        self.current_owner_visible = false;
        self.category_open = false;
    }
    pub fn selected_command(&self) -> Option<&KeyboardCommandRow> {
        self.selected
            .and_then(|i| self.rows.get(i))
            .map(|i| &self.commands[*i])
    }
    pub fn select_command(&mut self, row: usize) {
        if row >= self.rows.len() {
            return;
        }
        self.selected = Some(row);
        self.current_owner_visible = false;
        self.captured = 0;
        self.capture_focused = true;
    }
    pub fn reset_interaction(&mut self) {
        self.buttons = Default::default();
        self.scroll.cancel();
        self.hovered = None;
        self.category_open = false;
    }
}

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
}
impl KeyboardLayout {
    pub fn new(width: i32, height: i32, shell: Option<(InGameShellLayout, [i32; 2])>) -> Self {
        // Common60B7A0 ordinary controls use signed centering then clamp origins.
        let dlu = |x, y, w, h| {
            let r = geom::dlu_rect(x, y, w, h);
            RectPx::new(
                (r.x + (width - 800) / 2).max(0),
                (r.y + (height - 600) / 2).max(0),
                r.w,
                r.h,
            )
        };
        let (title, back, footer) = if let Some((shell, size)) = shell {
            (
                RectPx::new(width - 165, 2, 162, 16),
                RectPx::new(width - 147, shell.side3.y - size[1], size[0], size[1]),
                RectPx::new(10, height - 21, 455, 20),
            )
        } else {
            // The front-end page is a family page: its heading, Back and
            // status line sit where every family page puts them.
            let page =
                super::menu_page::compute_layout(&KEYBOARD_PAGE, width as u32, height as u32);
            (page.title, page.buttons[0].rect, page.status_help)
        };
        let mut category = dlu(63, 83, 138, 146);
        category.h = crate::ui::skirmish_shell::COMBO_FACE_H;
        Self {
            title,
            back,
            footer,
            category,
            commands: dlu(224, 82, 136, 110),
            group: dlu(63, 98, 138, 95),
            description: dlu(70, 109, 127, 42),
            capture: dlu(64, 245, 124, 14),
            assign: dlu(224, 245, 83, 15),
            reset: dlu(224, 278, 83, 15),
            current_owner: dlu(63, 278, 138, 10),
            current_shortcut: dlu(63, 215, 138, 10),
            error: dlu(224, 203, 146, 22),
            labels: [
                ("GUI:Category", dlu(63, 68, 146, 9), false),
                ("GUI:Commands", dlu(224, 68, 146, 8), false),
                ("GUI:PressShortcut", dlu(64, 233, 124, 9), false),
                ("GUI:CustomizeKeyboard", dlu(69, 45, 292, 11), true),
                ("GUI:CurAssignedTo", dlu(63, 266, 134, 11), false),
                ("GUI:CurrentShortcut", dlu(63, 203, 138, 11), false),
            ],
        }
    }
    pub fn button(self, button: KeyboardButton) -> RectPx {
        match button {
            KeyboardButton::Back => self.back,
            KeyboardButton::Assign => self.assign,
            KeyboardButton::ResetAll => self.reset,
        }
    }
    pub fn control_at(self, x: i32, y: i32) -> Option<KeyboardControl> {
        for button in [
            KeyboardButton::Back,
            KeyboardButton::Assign,
            KeyboardButton::ResetAll,
        ] {
            if self.button(button).contains(x, y) {
                return Some(KeyboardControl::Button(button));
            }
        }
        for (rect, id) in [
            (self.category, KeyboardControl::Category),
            (self.commands, KeyboardControl::Commands),
            (self.capture, KeyboardControl::Capture),
            (self.current_shortcut, KeyboardControl::CurrentShortcut),
            (self.current_owner, KeyboardControl::CurrentOwner),
            (self.error, KeyboardControl::Error),
            (self.group, KeyboardControl::Description),
        ] {
            if rect.contains(x, y) {
                return Some(id);
            }
        }
        None
    }
    /// Combo owner618AEA sets font+6=23px dropdown rows, independently of lists.
    pub fn category_popup(self, count: usize) -> RectPx {
        RectPx::new(
            self.category.x,
            self.category.y + self.category.h,
            self.category.w,
            count as i32 * 23 + 4,
        )
    }
    pub fn category_row_at(self, count: usize, x: i32, y: i32) -> Option<usize> {
        let popup = self.category_popup(count);
        let inner = RectPx::new(popup.x + 2, popup.y + 2, popup.w - 4, popup.h - 4);
        inner
            .contains(x, y)
            .then(|| ((y - inner.y) / 23) as usize)
            .filter(|i| *i < count)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn ordinary_a3_rectangles_match_original_60b7a0_execution() {
        let fixture: serde_json::Value = serde_json::from_str(include_str!(
            "../../../tools/storage_oracle/keyboard_shell_layout.json"
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
        let mut state = KeyboardState::new(KeyboardParent::Launcher, rows);
        assert_eq!(state.categories, ["Control", "Selection"]);
        assert!(state.selected_command().is_none());
        state.select_command(0);
        assert_eq!(state.selected_command().unwrap().command_index, 2);
        assert!(state.capture_focused);
        state.captured = 81;
        state.error = Some("Error:CannotMap");
        state.current_owner_visible = true;
        state.category_open = true;
        state.select_category(0);
        assert_eq!(state.captured, 81);
        assert_eq!(state.selected, Some(0));
        assert!(!state.category_open);
        state.reset_categories();
        assert!(state.selected.is_none());
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
