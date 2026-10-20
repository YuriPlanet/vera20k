//! The two persona questions of the WOL login flow, both family pages
//! (`0x0060C540` accepts them):
//!
//! - the auto-login query `0x112` (proc `0x0079AE00`), which `0x00787010`
//!   shows after a dialog login while `[Ask] AutoLogin` is set
//!   (`0x0078766F..0x007876C1`);
//! - the locale page `0xFE` (proc `0x00796B90`), which the connect
//!   `0x007864C0` shows when neither the persona nor the service knows the
//!   player's locale (`0x00786BE6..0x00786C07`).
//!
//! The models own the controls and what each close answers; `wolinfoMD.ini`
//! and the service belong to the app's WOL owner.

use crate::ui::shell::button::ShellButtonInteraction;
use crate::ui::shell::descriptor::DialogId;
use crate::ui::shell::geom::{
    RectPx, SHELL_BASE_H, SHELL_BASE_W, ShellScale, child_window, dlu_rect,
};
use crate::ui::shell::layout::status_line_rect;
use crate::ui::shell::list::ListScrollInteraction;
use crate::ui::shell::menu_page::{self, MenuPageButtonSpec, MenuPageLayout, MenuPageSpec};

pub(crate) const AUTO_LOGIN_DIALOG: DialogId = DialogId(0x0112);
pub(crate) const LOCALE_DIALOG: DialogId = DialogId(0x00FE);
/// IDOK/IDCANCEL: Yes/No on `0x112`, OK/Cancel on `0xFE`.
pub(crate) const ACCEPT: u16 = 0x0001;
pub(crate) const DECLINE: u16 = 0x0002;
pub(crate) const DO_NOT_ASK: u16 = 0x06F9;
pub(crate) const LOCALE_LIST: u16 = 0x03F0;

/// RT_DIALOG `0x112`: Yes on top, No on the bottom row. Help keys from the
/// executed `0x006040B0`.
pub(crate) const AUTO_LOGIN_PAGE: MenuPageSpec = MenuPageSpec {
    dialog: AUTO_LOGIN_DIALOG,
    title_key: "GUI:AutoLoginQueryTitle",
    stacked: &[MenuPageButtonSpec {
        id: ACCEPT,
        dlu_top: 116,
        csf_key: "GUI:Yes",
        tooltip_key: "STT:WOLAutoLoginYes",
        result: Some(1),
    }],
    back: MenuPageButtonSpec {
        id: DECLINE,
        dlu_top: 257,
        csf_key: "GUI:No",
        tooltip_key: "STT:WOLAutoLoginNo",
        result: Some(2),
    },
};

/// RT_DIALOG `0xFE`: OK (created disabled, `0x5800000B`) on top, Cancel on
/// the bottom row.
pub(crate) const LOCALE_PAGE: MenuPageSpec = MenuPageSpec {
    dialog: LOCALE_DIALOG,
    title_key: "GUI:Location",
    stacked: &[MenuPageButtonSpec {
        id: ACCEPT,
        dlu_top: 116,
        csf_key: "GUI:OK",
        tooltip_key: "STT:WOLLocaleOK",
        result: Some(1),
    }],
    back: MenuPageButtonSpec {
        id: DECLINE,
        dlu_top: 257,
        csf_key: "GUI:Cancel",
        tooltip_key: "STT:WOLLocaleCancel",
        result: Some(2),
    },
};

/// The question text: a centred static (`0x50020201`).
pub(crate) const AUTO_LOGIN_TEXT_KEY: &str = "GUI:AutoLoginQuery";
pub(crate) const DO_NOT_ASK_KEY: &str = "GUI:DoNotAsk";
/// The list's caption (`0x50020001`, centred).
pub(crate) const LOCALE_LABEL_KEY: &str = "GUI:YourLocation";

/// Status help of the two pages' children (executed `0x006040B0`).
pub(crate) fn tooltip_key(dialog: DialogId, control: u16) -> Option<&'static str> {
    match (dialog, control) {
        (AUTO_LOGIN_DIALOG, DO_NOT_ASK) => Some("STT:WOLAutoLoginDoNotAsk"),
        (AUTO_LOGIN_DIALOG, _) => AUTO_LOGIN_PAGE.button(control).map(|spec| spec.tooltip_key),
        (LOCALE_DIALOG, LOCALE_LIST) => Some("STT:WOLMyInformationLocale"),
        (LOCALE_DIALOG, _) => LOCALE_PAGE.button(control).map(|spec| spec.tooltip_key),
        _ => None,
    }
}

/// How `0x112` closed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AutoLoginAnswer {
    /// `WM_COMMAND 1`: the nickname becomes `[DefaultPersona] AutoLogin`
    /// (`0x0079AEE7`); `[Ask]` is untouched.
    Yes,
    /// `WM_COMMAND 2`: `[Ask] AutoLogin` = Don't Ask is unchecked
    /// (`0x0079AEB8..0x0079AECF`).
    No { ask_again: bool },
}

/// Dialog `0x112` while it exists. Init only stores the controller, so Don't
/// Ask starts unchecked; there is no Escape or Enter case.
#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct WolAutoLoginQuery {
    pub(crate) do_not_ask: bool,
    pub(crate) buttons: ShellButtonInteraction<u16>,
    pub(crate) hovered: Option<u16>,
}

impl WolAutoLoginQuery {
    /// A click on Don't Ask's icon: BS_AUTOCHECKBOX flips it.
    pub(crate) fn toggle(&mut self) {
        self.do_not_ask = !self.do_not_ask;
    }

    pub(crate) fn activate(&self, control: u16) -> Option<AutoLoginAnswer> {
        match control {
            ACCEPT => Some(AutoLoginAnswer::Yes),
            DECLINE => Some(AutoLoginAnswer::No {
                ask_again: !self.do_not_ask,
            }),
            _ => None,
        }
    }
}

/// One row of `0xFE`'s list: the `WOL:Locale%02d` text and its item data.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LocaleRow {
    pub(crate) text: String,
    pub(crate) locale: i32,
}

/// The locale rows of `0xFE`'s list and `0x114`'s locale combo
/// (`0x00798F50`): for each locale from the last down to 0, its
/// `WOL:Locale%02d` text (`0x00780A00`) goes in with `0x4CD`/`0x4C2` for
/// locales 2 and up, which the sorted list or combo (`0x0061B481`) adds by
/// name, and with `0x4CC`/`0x4C1` (insert at row 0) for 0 and 1. So Unknown
/// and Rest of World lead and the others follow by name.
pub(crate) fn locale_rows(texts: impl IntoIterator<Item = (i32, String)>) -> Vec<LocaleRow> {
    let mut sorted: Vec<LocaleRow> = Vec::new();
    let mut leading: Vec<LocaleRow> = Vec::new();
    let mut texts: Vec<(i32, String)> = texts.into_iter().collect();
    texts.sort_by_key(|(locale, _)| std::cmp::Reverse(*locale));
    for (locale, text) in texts {
        let row = LocaleRow { text, locale };
        if locale < 2 {
            leading.insert(0, row);
        } else {
            // LBS_SORT/CBS_SORT over the narrowed text: case-insensitive
            // order, a later equal name after the earlier ones.
            let key = row.text.to_ascii_lowercase();
            let at = sorted
                .iter()
                .position(|other| other.text.to_ascii_lowercase() > key)
                .unwrap_or(sorted.len());
            sorted.insert(at, row);
        }
    }
    leading.extend(sorted);
    leading
}

/// Dialog `0xFE` while it exists.
#[derive(Debug, Clone, Default)]
pub(crate) struct WolLocalePage {
    pub(crate) rows: Vec<LocaleRow>,
    /// LB_SETCURSEL(-1) after the fill: nothing is selected, so OK starts
    /// disabled.
    pub(crate) selected: Option<usize>,
    pub(crate) top: usize,
    pub(crate) scroll: ListScrollInteraction,
    pub(crate) buttons: ShellButtonInteraction<u16>,
    pub(crate) hovered: Option<u16>,
}

impl WolLocalePage {
    /// Init `0x497` (`0x00796C8E..0x00796D2F`): the rows of
    /// [`locale_rows`], none selected.
    pub(crate) fn fill(&mut self, texts: impl IntoIterator<Item = (i32, String)>) {
        self.rows = locale_rows(texts);
        self.selected = None;
        self.top = 0;
    }

    /// LBN_SELCHANGE (`0x00796C3C`): OK is enabled exactly while a row is
    /// selected.
    pub(crate) fn select(&mut self, row: usize) {
        if row < self.rows.len() {
            self.selected = Some(row);
        }
    }

    pub(crate) fn button_enabled(&self, control: u16) -> bool {
        control != ACCEPT || self.selected.is_some()
    }

    /// OK closes with the selected row's item data (0 with none), Cancel
    /// with 0.
    pub(crate) fn activate(&self, control: u16) -> Option<i32> {
        if !self.button_enabled(control) {
            return None;
        }
        match control {
            ACCEPT => Some(
                self.selected
                    .and_then(|row| self.rows.get(row))
                    .map_or(0, |row| row.locale),
            ),
            DECLINE => Some(0),
            _ => None,
        }
    }
}

/// Either page at the current screen: the family right panel, the status
/// line and the left children at their template places (`0x0060C0C0`).
#[derive(Debug, Clone)]
pub(crate) struct WolPersonaLayout {
    pub(crate) dialog: DialogId,
    pub(crate) page: MenuPageLayout,
    pub(crate) status_help: RectPx,
    /// `0x112`: the question; `0xFE`: the list's caption.
    pub(crate) text: RectPx,
    /// `0x112`: Don't Ask; `0xFE`: the list `0x3F0`.
    pub(crate) control: RectPx,
}

impl WolPersonaLayout {
    pub(crate) fn new(dialog: DialogId, width: i32, height: i32) -> Self {
        Self::build(dialog, width, height, ShellScale::RETAIL)
    }

    pub(crate) fn responsive(dialog: DialogId, width: i32, height: i32) -> Self {
        Self::build(
            dialog,
            width,
            height,
            ShellScale::from_screen(width, height),
        )
    }

    pub(crate) fn spec(dialog: DialogId) -> &'static MenuPageSpec {
        if dialog == LOCALE_DIALOG {
            &LOCALE_PAGE
        } else {
            &AUTO_LOGIN_PAGE
        }
    }

    fn build(dialog: DialogId, width: i32, height: i32, scale: ShellScale) -> Self {
        let spec = Self::spec(dialog);
        let locale = dialog == LOCALE_DIALOG;
        let status_template = RectPx::new(2, 355, 316, 12);
        let (mut page, status_help) = if scale.is_retail() {
            (
                menu_page::compute_layout(spec, width as u32, height as u32),
                status_line_rect(status_template, width, height),
            )
        } else {
            (
                menu_page::compute_responsive_layout(spec, width as u32, height as u32),
                scale.scale_rect(status_line_rect(
                    status_template,
                    SHELL_BASE_W,
                    SHELL_BASE_H,
                )),
            )
        };
        // 0x112's heading 0x694 is 108x11 DLU and placed by 0x0060B1D0, one
        // row taller than the family's. 0xFE's is not placed at all: it
        // stays at its template rect at every size.
        let title = if locale {
            child_window(424, 4, 108, 11)
        } else {
            let mut title =
                menu_page::compute_layout(spec, SHELL_BASE_W as u32, SHELL_BASE_H as u32).title;
            title.h = dlu_rect(316, 4, 108, 11).h + 1;
            title
        };
        if scale.is_retail() && !locale {
            page.title.h = title.h;
        } else {
            page.title = scale.scale_rect(title);
        }
        let (text, control) = if locale {
            (
                child_window(119, 91, 187, 15),
                child_window(119, 112, 188, 145),
            )
        } else {
            (
                child_window(15, 131, 392, 109),
                child_window(124, 236, 157, 14),
            )
        };
        Self {
            dialog,
            page,
            status_help,
            text: scale.scale_rect(text),
            control: scale.scale_rect(control),
        }
    }

    /// The control under `(x, y)`: buttons first, then the list or the
    /// checkbox.
    pub(crate) fn control_at(&self, x: i32, y: i32) -> Option<u16> {
        if let Some(button) = self.page.buttons.iter().find(|b| b.rect.contains(x, y)) {
            return Some(button.id);
        }
        self.control
            .contains(x, y)
            .then_some(if self.dialog == LOCALE_DIALOG {
                LOCALE_LIST
            } else {
                DO_NOT_ASK
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rect(value: &serde_json::Value) -> RectPx {
        let a: Vec<i32> = serde_json::from_value(value.clone()).unwrap();
        RectPx::new(a[0], a[1], a[2], a[3])
    }

    #[test]
    fn layout_and_help_follow_the_executed_tables() {
        let corpus: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
            "tools/storage_oracle/wol_shell.json",
        ))
        .unwrap();
        let mut checked = 0;
        for dialog in [AUTO_LOGIN_DIALOG, LOCALE_DIALOG] {
            let key = format!("0x{:X}", dialog.0);
            for (size, case) in corpus["layouts"][&key]["sizes"].as_object().unwrap() {
                let (width, height) = size.split_once('x').unwrap();
                let layout =
                    WolPersonaLayout::new(dialog, width.parse().unwrap(), height.parse().unwrap());
                for (control, child) in case["controls"].as_object().unwrap() {
                    let id = u16::from_str_radix(&control[2..], 16).unwrap();
                    let actual = match id {
                        0x71C => layout.page.warning_monitor,
                        0x694 => layout.page.title,
                        0x695 => layout.status_help,
                        0xFFFF => layout.text,
                        DO_NOT_ASK | LOCALE_LIST => layout.control,
                        _ => layout
                            .page
                            .button_rect(id)
                            .unwrap_or_else(|| panic!("{key} {control}")),
                    };
                    assert_eq!(actual, rect(&child["rect"]), "{key} {size} {control}");
                    checked += 1;
                }
            }
            for (control, help) in corpus["help"][&key].as_object().unwrap() {
                let id = u16::from_str_radix(&control[2..], 16).unwrap();
                assert_eq!(tooltip_key(dialog, id), help.as_str(), "{key} {control}");
            }
            let record = &corpus["slides"][&key];
            let slide = crate::ui::shell::slide::slide_spec_for(dialog).unwrap();
            assert_eq!(
                slide.top_buttons as usize,
                record["top"].as_array().unwrap().len()
            );
            assert_eq!(
                slide.bottom_button,
                !record["bottom"].as_array().unwrap().is_empty()
            );
            assert_eq!(
                (slide.top_panel, slide.map_button),
                (record["d5"] != 0, record["d6"] != 0)
            );
        }
        assert_eq!(checked, 42);
    }

    #[test]
    fn auto_login_answers_follow_the_proc() {
        let mut query = WolAutoLoginQuery::default();
        assert_eq!(query.activate(ACCEPT), Some(AutoLoginAnswer::Yes));
        assert_eq!(
            query.activate(DECLINE),
            Some(AutoLoginAnswer::No { ask_again: true })
        );
        query.toggle();
        assert_eq!(
            query.activate(DECLINE),
            Some(AutoLoginAnswer::No { ask_again: false })
        );
        assert_eq!(query.activate(DO_NOT_ASK), None);
    }

    #[test]
    fn locales_0_and_1_lead_and_the_rest_sort_by_name() {
        let mut page = WolLocalePage::default();
        page.fill([
            (0, "Unknown".to_owned()),
            (1, "Rest of World".to_owned()),
            (2, "United States".to_owned()),
            (3, "Canada".to_owned()),
            (4, "united Kingdom".to_owned()),
            (5, "Germany".to_owned()),
        ]);
        let order: Vec<i32> = page.rows.iter().map(|row| row.locale).collect();
        assert_eq!(order, [0, 1, 3, 5, 4, 2]);
        assert!(!page.button_enabled(ACCEPT));
        assert_eq!(page.activate(ACCEPT), None);
        assert_eq!(page.activate(DECLINE), Some(0));
        page.select(3);
        assert!(page.button_enabled(ACCEPT));
        assert_eq!(page.activate(ACCEPT), Some(5));
        page.select(99);
        assert_eq!(page.selected, Some(3));
    }
}
