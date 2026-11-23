//! The WOL persona questions, all family pages (`0x0060C540` accepts
//! them):
//!
//! - the auto-login query `0x112` (proc `0x0079AE00`), which `0x00787010`
//!   shows after a dialog login while `[Ask] AutoLogin` is set
//!   (`0x0078766F..0x007876C1`);
//! - the locale page `0xFE` (proc `0x00796B90`), which the connect
//!   `0x007864C0` shows when neither the persona nor the service knows the
//!   player's locale (`0x00786BE6..0x00786C07`);
//! - the add-buddy query `0x122` (proc `0x007A29E0`), which Quick Match's
//!   postmatch `0x007A2970` shows while `[Ask] AddMatchBuddies` is set. It
//!   has the auto-login query's shape (Yes, No, Don't Ask and a question)
//!   but no heading.
//!
//! The models own the controls and what each close answers; `wolinfoMD.ini`
//! and the service belong to the app's WOL owner.

use vera20k_gui::{NodeKind, NodeSpec, Prop, Value};

use crate::ui::shell::descriptor::DialogResourceId;
use crate::ui::shell::geom::ShellAlign;
use crate::ui::shell::geom::{RectPx, ShellScale};
use crate::ui::shell::layout::RectTable;
use crate::ui::shell::layout_data;
use crate::ui::shell::menu_page::{MenuPageButtonSpec, MenuPageLayout, MenuPageSpec};
use crate::ui::shell::page;

pub(crate) const AUTO_LOGIN_DIALOG: DialogResourceId = DialogResourceId(0x0112);
pub(crate) const LOCALE_DIALOG: DialogResourceId = DialogResourceId(0x00FE);
pub(crate) const ADD_BUDDY_DIALOG: DialogResourceId = DialogResourceId(0x0122);
/// IDOK/IDCANCEL: Yes/No on `0x112` and `0x122`, OK/Cancel on `0xFE`.
pub(crate) const ACCEPT: u16 = 0x0001;
pub(crate) const DECLINE: u16 = 0x0002;
pub(crate) const DO_NOT_ASK: u16 = 0x06F9;
pub(crate) const LOCALE_LIST: u16 = 0x03F0;

/// RT_DIALOG `0x112`: Yes on top, No on the bottom row. Help keys from the
/// executed `0x006040B0`.
pub(crate) const AUTO_LOGIN_PAGE: MenuPageSpec = MenuPageSpec {
    dialog: AUTO_LOGIN_DIALOG,
    title_key: "GUI:AutoLoginQueryTitle",
    stacked: &[MenuPageButtonSpec::new(ACCEPT, 116, "GUI:Yes", "STT:WOLAutoLoginYes").returning(1)],
    back: MenuPageButtonSpec::new(DECLINE, 257, "GUI:No", "STT:WOLAutoLoginNo").returning(2),
};

/// RT_DIALOG `0xFE`: OK (created disabled, `0x5800000B`) on top, Cancel on
/// the bottom row.
pub(crate) const LOCALE_PAGE: MenuPageSpec = MenuPageSpec {
    dialog: LOCALE_DIALOG,
    title_key: "GUI:Location",
    stacked: &[MenuPageButtonSpec::new(ACCEPT, 116, "GUI:OK", "STT:WOLLocaleOK").returning(1)],
    back: MenuPageButtonSpec::new(DECLINE, 257, "GUI:Cancel", "STT:WOLLocaleCancel").returning(2),
};

/// RT_DIALOG `0x122`: Yes on top, No on the bottom row, and no heading
/// `0x694` in the template.
pub(crate) const ADD_BUDDY_PAGE: MenuPageSpec = MenuPageSpec {
    dialog: ADD_BUDDY_DIALOG,
    title_key: "",
    stacked: &[
        MenuPageButtonSpec::new(ACCEPT, 116, "GUI:Yes", "STT:WOLAddBuddyQueryYes").returning(1),
    ],
    back: MenuPageButtonSpec::new(DECLINE, 257, "GUI:No", "STT:WOLAddBuddyQueryNo").returning(2),
};

/// The question text: a centred static (`0x50020201`).
pub(crate) const AUTO_LOGIN_TEXT_KEY: &str = "GUI:AutoLoginQuery";
/// `0x122`'s question `0x71B`, which init formats with the opponent.
pub(crate) const ADD_BUDDY_TEXT_KEY: &str = "WOL:AddBuddyQuery";
pub(crate) const DO_NOT_ASK_KEY: &str = "GUI:DoNotAsk";
/// The list's caption (`0x50020001`, centred).
pub(crate) const LOCALE_LABEL_KEY: &str = "GUI:YourLocation";

/// Status help of the two pages' children (executed `0x006040B0`).
pub(crate) fn tooltip_key(dialog: DialogResourceId, control: u16) -> Option<&'static str> {
    match (dialog, control) {
        (AUTO_LOGIN_DIALOG, DO_NOT_ASK) => Some("STT:WOLAutoLoginDoNotAsk"),
        (AUTO_LOGIN_DIALOG, _) => AUTO_LOGIN_PAGE.button(control).map(|spec| spec.tooltip_key),
        (ADD_BUDDY_DIALOG, DO_NOT_ASK) => Some("STT:WOLAddBuddyQueryDoNotAsk"),
        (ADD_BUDDY_DIALOG, _) => ADD_BUDDY_PAGE.button(control).map(|spec| spec.tooltip_key),
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

/// Dialog `0x112` or `0x122` while it exists; its Don't Ask box (unchecked
/// at init) lives in the dialog's widgets. There is no Escape or Enter case.
#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct WolAutoLoginQuery;

impl WolAutoLoginQuery {
    pub(crate) fn activate(control: u16, do_not_ask: bool) -> Option<AutoLoginAnswer> {
        match control {
            ACCEPT => Some(AutoLoginAnswer::Yes),
            DECLINE => Some(AutoLoginAnswer::No {
                ask_again: !do_not_ask,
            }),
            _ => None,
        }
    }
}

/// `WOL:Locale00..37`: every locale the retail CSF names. WOLAPI asked the
/// server for the count (API+0xD0); VERA's services answer with this one.
pub(crate) const LOCALE_COUNT: i32 = 38;

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

/// Dialog `0xFE` while it exists: its rows; the list's selection (none
/// after the fill's LB_SETCURSEL(-1), so OK starts disabled) lives in the
/// dialog's widgets.
#[derive(Debug, Clone, Default)]
pub(crate) struct WolLocalePage {
    pub(crate) rows: Vec<LocaleRow>,
}

impl WolLocalePage {
    /// Init `0x497` (`0x00796C8E..0x00796D2F`): the rows of
    /// [`locale_rows`].
    pub(crate) fn fill(&mut self, texts: impl IntoIterator<Item = (i32, String)>) {
        self.rows = locale_rows(texts);
    }

    /// OK closes with the selected row's item data, Cancel with 0. OK is
    /// enabled exactly while a row is selected (LBN_SELCHANGE `0x00796C3C`).
    pub(crate) fn activate(&self, control: u16, selected: Option<usize>) -> Option<i32> {
        match control {
            ACCEPT => Some(self.rows.get(selected?)?.locale),
            DECLINE => Some(0),
            _ => None,
        }
    }
}

/// The question or the list's caption (a static without an id).
pub(crate) const TEXT: u16 = 0xFFFF;

/// A persona page's widgets: the column's buttons, the centred question and
/// Don't Ask (`OwnerDraw_CheckBox 0x006163A0`, toggled on its icon's press),
/// or the caption and the locale list (`0x00618D40`, every press selects).
pub(crate) fn spec(dialog: DialogResourceId) -> NodeSpec {
    let text = NodeSpec::new(NodeKind::Text).id(TEXT.into());
    let (text, control) = if dialog == LOCALE_DIALOG {
        let list = NodeSpec::new(NodeKind::List);
        (
            text.label(LOCALE_LABEL_KEY)
                .with(Prop::Align, align(ShellAlign::H_CENTER)),
            list.id(LOCALE_LIST.into()),
        )
    } else {
        let checkbox = NodeSpec::new(NodeKind::Checkbox).label(DO_NOT_ASK_KEY);
        let text = text.with(
            Prop::Align,
            align(ShellAlign::H_CENTER | ShellAlign::V_CENTER),
        );
        (text, checkbox.id(DO_NOT_ASK.into()))
    };
    let control = match tooltip_key(dialog, control_id(dialog)) {
        Some(key) => control.tooltip(key),
        None => control,
    };
    let text = if dialog == AUTO_LOGIN_DIALOG {
        text.label(AUTO_LOGIN_TEXT_KEY)
    } else {
        text
    };
    NodeSpec::new(NodeKind::Dialog)
        .children(page::menu_buttons(WolPersonaLayout::spec(dialog)))
        .child(text)
        .child(control)
}

fn align(align: ShellAlign) -> Value {
    Value::Int(i32::from(align.0))
}

fn control_id(dialog: DialogResourceId) -> u16 {
    if dialog == LOCALE_DIALOG {
        LOCALE_LIST
    } else {
        DO_NOT_ASK
    }
}

/// A persona page at the current screen: the family right panel, the
/// status line and the left children at their template places
/// (`0x0060C0C0`).
#[derive(Debug, Clone)]
pub(crate) struct WolPersonaLayout {
    pub(crate) dialog: DialogResourceId,
    pub(crate) page: MenuPageLayout,
    /// The question or the list's caption [`TEXT`], and Don't Ask or the
    /// list `0x3F0`.
    pub(crate) rects: RectTable,
}

impl WolPersonaLayout {
    pub(crate) fn spec(dialog: DialogResourceId) -> &'static MenuPageSpec {
        match dialog {
            LOCALE_DIALOG => &LOCALE_PAGE,
            ADD_BUDDY_DIALOG => &ADD_BUDDY_PAGE,
            _ => &AUTO_LOGIN_PAGE,
        }
    }

    pub(crate) fn new(
        dialog: DialogResourceId,
        width: i32,
        height: i32,
        scale: ShellScale,
    ) -> Self {
        let section = format!("{:#X}", dialog.0);
        let (page, rects) =
            layout_data::menu_page(Self::spec(dialog), &section, scale, width, height);
        Self {
            dialog,
            page,
            rects,
        }
    }

    /// Where control `id` sits.
    pub(crate) fn rect_of(&self, id: u16) -> Option<RectPx> {
        self.rects.get(id).or_else(|| self.page.button_rect(id))
    }
}

#[cfg(all(test, tests_ui))]
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
        for dialog in [AUTO_LOGIN_DIALOG, LOCALE_DIALOG, ADD_BUDDY_DIALOG] {
            let key = format!("0x{:X}", dialog.0);
            for (size, case) in corpus["layouts"][&key]["sizes"].as_object().unwrap() {
                let (width, height) = size.split_once('x').unwrap();
                let layout = WolPersonaLayout::new(
                    dialog,
                    width.parse().unwrap(),
                    height.parse().unwrap(),
                    crate::ui::shell::geom::RETAIL_SCALE,
                );
                for (control, child) in case["controls"].as_object().unwrap() {
                    let id = u16::from_str_radix(&control[2..], 16).unwrap();
                    let actual = match id {
                        0x71C => layout.page.warning_monitor,
                        0x694 => layout.page.title,
                        0x695 => layout.page.status_help,
                        0x71B => layout.rect_of(TEXT).unwrap(),
                        _ => layout
                            .rect_of(id)
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
        assert_eq!(checked, 42 + 3 * 6);
    }

    #[test]
    fn auto_login_answers_follow_the_proc() {
        let activate = WolAutoLoginQuery::activate;
        assert_eq!(activate(ACCEPT, false), Some(AutoLoginAnswer::Yes));
        assert_eq!(
            activate(DECLINE, false),
            Some(AutoLoginAnswer::No { ask_again: true })
        );
        assert_eq!(
            activate(DECLINE, true),
            Some(AutoLoginAnswer::No { ask_again: false })
        );
        assert_eq!(activate(DO_NOT_ASK, true), None);
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
        // Nothing selected: OK answers nothing (it is disabled).
        assert_eq!(page.activate(ACCEPT, None), None);
        assert_eq!(page.activate(DECLINE, None), Some(0));
        assert_eq!(page.activate(ACCEPT, Some(3)), Some(5));
        assert_eq!(page.activate(ACCEPT, Some(99)), None);
    }
}
