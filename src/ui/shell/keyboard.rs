//! Shared Keyboard A3 control state and physical geometry. Native5FBEF0/5FB320,
//! RT_DIALOG A3 atBEF560; bindings remain in the app command owner.
use super::button::ShellButtonInteraction;
use super::geom::{self, RectPx, ShellScale};
use super::in_game_shell::{self, InGameShellLayout};
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
    /// The mapping the control rects were placed in: retail centers the authored
    /// 800x600 dialog, the responsive page scales it to fit the window.
    pub scale: ShellScale,
}
impl KeyboardLayout {
    /// Gamemd's A3: the authored dialog centered in the window at its native
    /// size. Also the frame the in-game page sits in, which never scales.
    pub fn new(width: i32, height: i32, shell: Option<(InGameShellLayout, [i32; 2])>) -> Self {
        Self::build(width, height, shell, ShellScale::RETAIL)
    }

    /// The responsive variant of [`Self::new`] for the front-end family page: the
    /// verified 800x600 controls scaled to fit the window, with the family
    /// chrome where every other scaled family page puts it. Identity at the
    /// base size. The in-game frame keeps its own layout, so it takes `new`.
    pub fn responsive(width: i32, height: i32) -> Self {
        Self::build(width, height, None, ShellScale::from_screen(width, height))
    }

    fn build(
        width: i32,
        height: i32,
        shell: Option<(InGameShellLayout, [i32; 2])>,
        scale: ShellScale,
    ) -> Self {
        // Common60B7A0 ordinary controls use signed centering then clamp origins.
        let dlu = |x, y, w, h| {
            let r = geom::dlu_rect(x, y, w, h);
            if scale.is_retail() {
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
        } else if scale.is_retail() {
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
        let mut category = dlu(63, 83, 138, 146);
        category.h =
            (crate::ui::skirmish_shell::COMBO_FACE_H as f32 * scale.factor()).round() as i32;
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
    pub fn category_row_at(self, count: usize, x: i32, y: i32) -> Option<usize> {
        let popup = self.category_popup(count);
        let inner = RectPx::new(popup.x + 2, popup.y + 2, popup.w - 4, popup.h - 4);
        inner
            .contains(x, y)
            .then(|| {
                ((y - inner.y)
                    / crate::ui::skirmish_shell::DropdownMetrics::scaled(self.scale).row_h)
                    as usize
            })
            .filter(|i| *i < count)
    }
}

#[cfg(test)]
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
            let count = 4;
            let popup = layout.category_popup(count);
            assert_eq!(popup.x, layout.category.x);
            assert_eq!(popup.w, layout.category.w);
            for i in 0..count {
                let y = popup.y + 2 + i as i32 * row_h + row_h / 2;
                assert_eq!(
                    layout.category_row_at(count, popup.x + row_h, y),
                    Some(i),
                    "row {i} of a {}x{} page",
                    layout.category.w,
                    layout.category.h
                );
            }
        }
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
