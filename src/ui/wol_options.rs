//! Westwood Online Options `0x2BC` (proc `0x0078EBC0`), opened only by lobby
//! `0x113`'s Options button `0x61F` (`0x0078B829..0x0078B88A`).
//!
//! The model owns the dialog's nine checkboxes and its results. The
//! `[WOnline]` preferences it shows and commits ([`WolPreferences`]) belong to
//! the app: the profile holds what VERA.INI says, and the WOL owner holds the
//! live process copy each WOL entry re-reads (`0x0077DED0`).

use crate::ui::shell::button::ShellButtonInteraction;
use crate::ui::shell::descriptor::DialogId;
use crate::ui::shell::geom::{
    RectPx, SHELL_BASE_H, SHELL_BASE_W, ShellScale, child_window, dlu_rect,
};
use crate::ui::shell::layout::status_line_rect;
use crate::ui::shell::menu_page::{self, MenuPageButtonSpec, MenuPageLayout, MenuPageSpec};

pub(crate) const DIALOG: DialogId = DialogId(0x02BC);
pub(crate) const OK: u16 = 0x0001;
pub(crate) const CANCEL: u16 = 0x0002;
pub(crate) const SOUND_OPTIONS: u16 = 0x063A;
pub(crate) const PAGES: u16 = 0x05F5;
pub(crate) const LANGUAGE: u16 = 0x05F6;
pub(crate) const FIND: u16 = 0x05F7;
pub(crate) const MUSIC: u16 = 0x05F8;
pub(crate) const SHOW_ALL: u16 = 0x05F9;
pub(crate) const ASIAN: u16 = 0x05FA;
pub(crate) const LATIN: u16 = 0x05FB;
pub(crate) const BUDDIES: u16 = 0x0774;
pub(crate) const CLAN: u16 = 0x0775;

/// The nine `[WOnline]` values (`0x0077DED0`, `0x0077DFD0`). The seven ints
/// stay raw: the dialog only tests them for nonzero and OK replaces them with
/// 0 or 1. Defaults are the reader's constants, not the retained values.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct WolPreferences {
    /// `AllowPage` `[0xB76234]`.
    pub(crate) allow_page: i32,
    /// `AllowFind` `[0xB779F0]`.
    pub(crate) allow_find: i32,
    /// `LangFilter` `[0xB779BC]`.
    pub(crate) lang_filter: i32,
    /// `LobMusic` `[0xB7690C]`.
    pub(crate) lobby_music: i32,
    /// `ShowAll` `[0xB77784]`: stored, shown and saved; nothing else reads it.
    pub(crate) show_all: i32,
    /// `DisplayAsian` `[0xB779DC]`.
    pub(crate) display_asian: i32,
    /// `DisplayLatin` `[0xB779C0]`.
    pub(crate) display_latin: i32,
    /// `DisplayBuddies` byte `[0xB76AF0]`, also the chat `/buddy` toggle.
    pub(crate) display_buddies: bool,
    /// `DisplayClan` byte `[0xB73AD0]`, also the chat `/clan` toggle.
    pub(crate) display_clan: bool,
}

impl Default for WolPreferences {
    fn default() -> Self {
        Self {
            allow_page: 1,
            allow_find: 1,
            lang_filter: 1,
            lobby_music: 1,
            show_all: 1,
            display_asian: 1,
            display_latin: 1,
            display_buddies: false,
            display_clan: false,
        }
    }
}

/// One of the nine auto-checkboxes (style `0x50008003`, placed by
/// `0x0060B950`), in template order. Help keys from the executed `0x006040B0`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct OptionCheckbox {
    pub(crate) id: u16,
    pub(crate) csf_key: &'static str,
    pub(crate) help_key: &'static str,
    /// Template DLU `(x, y, w, h)`.
    dlu: (i32, i32, i32, i32),
}

pub(crate) const CHECKBOXES: [OptionCheckbox; 9] = [
    checkbox(PAGES, "GUI:AllowPages", "STT:WOLOptionsPage", 68),
    checkbox(
        LANGUAGE,
        "GUI:FilterBadLan",
        "STT:WOLOptionsLanguageFilter",
        86,
    ),
    checkbox(FIND, "GUI:AllowFinding", "STT:WOLOptionsFind", 104),
    checkbox(MUSIC, "GUI:PlayMusic", "STT:WOLOptionsMusic", 121),
    checkbox(
        SHOW_ALL,
        "GUI:ShowAllInLobby",
        "STT:WOLOptionsShowAllInLobby",
        140,
    ),
    checkbox(
        ASIAN,
        "GUI:FilterAsianMessages",
        "STT:WOLOptionFilterAsian",
        158,
    ),
    checkbox(
        LATIN,
        "GUI:FilterNonAsianMessages",
        "STT:WOLOptionFilterNonAsian",
        176,
    ),
    checkbox(
        BUDDIES,
        "GUI:FilterBuddies",
        "STT:WOLOptionFilterBuddies",
        194,
    ),
    checkbox(CLAN, "GUI:FilterClan", "STT:WOLOptionFilterClan", 212),
];

const fn checkbox(
    id: u16,
    csf_key: &'static str,
    help_key: &'static str,
    dlu_top: i32,
) -> OptionCheckbox {
    OptionCheckbox {
        id,
        csf_key,
        help_key,
        dlu: (97, dlu_top, 221, 15),
    }
}

/// RT_DIALOG `0x2BC` as a family page (`0x0060C540` accepts it): OK and
/// Sound Options stacked by template tops, Cancel on the bottom row. The
/// hidden, disabled Observe box `0x5FC` is never shown or touched.
pub(crate) const PAGE: MenuPageSpec = MenuPageSpec {
    dialog: DIALOG,
    title_key: "GUI:WOLOptions",
    stacked: &[
        MenuPageButtonSpec {
            id: OK,
            dlu_top: 111,
            csf_key: "GUI:OK",
            tooltip_key: "STT:WOLOptionsOk",
            result: Some(1),
        },
        MenuPageButtonSpec {
            id: SOUND_OPTIONS,
            dlu_top: 140,
            csf_key: "GUI:SoundOptions",
            tooltip_key: "STT:WOLOptionsSoundOptions",
            result: None,
        },
    ],
    back: MenuPageButtonSpec {
        id: CANCEL,
        dlu_top: 251,
        csf_key: "GUI:Cancel",
        tooltip_key: "STT:WOLOptionsCancel",
        result: Some(2),
    },
};

/// Status help of `0x2BC`'s controls (executed `0x006040B0`).
pub(crate) fn tooltip_key(control: u16) -> Option<&'static str> {
    CHECKBOXES
        .iter()
        .find(|checkbox| checkbox.id == control)
        .map(|checkbox| checkbox.help_key)
        .or_else(|| PAGE.button(control).map(|spec| spec.tooltip_key))
}

/// What a press on the page asks its host to do.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum WolOptionsAction {
    /// IDOK: the staged values (`0x0078EC9A..0x0078EDE1`).
    Ok(WolPreferences),
    /// IDCANCEL: nothing staged changes.
    Cancel,
    /// `0x63A`: the nested Sound Options page `0xD6`.
    SoundOptions,
    /// `0x5F8` notification 0/1: the live music preview, with the box's state.
    MusicPreview(bool),
}

/// The open dialog: its checkbox states and button press.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct WolOptionsState {
    /// Template order, as [`CHECKBOXES`].
    pub(crate) checked: [bool; 9],
    pub(crate) buttons: ShellButtonInteraction<u16>,
    /// The control under the pointer, the status line's help source.
    pub(crate) hovered: Option<u16>,
}

impl WolOptionsState {
    /// Init `0x497`: every nonzero live value checks its box; unchecked is the
    /// template default.
    pub(crate) fn open(preferences: &WolPreferences) -> Self {
        let p = preferences;
        Self {
            checked: [
                p.allow_page != 0,
                p.lang_filter != 0,
                p.allow_find != 0,
                p.lobby_music != 0,
                p.show_all != 0,
                p.display_asian != 0,
                p.display_latin != 0,
                p.display_buddies,
                p.display_clan,
            ],
            buttons: ShellButtonInteraction::default(),
            hovered: None,
        }
    }

    pub(crate) fn is_checked(&self, id: u16) -> bool {
        CHECKBOXES
            .iter()
            .position(|checkbox| checkbox.id == id)
            .is_some_and(|index| self.checked[index])
    }

    /// A click on a checkbox's icon: BS_AUTOCHECKBOX flips it. Only Play
    /// Music has a proc case.
    pub(crate) fn toggle(&mut self, id: u16) -> Option<WolOptionsAction> {
        let index = CHECKBOXES.iter().position(|checkbox| checkbox.id == id)?;
        self.checked[index] = !self.checked[index];
        (id == MUSIC).then_some(WolOptionsAction::MusicPreview(self.checked[index]))
    }

    /// A released owner-draw button.
    pub(crate) fn activate(&self, id: u16) -> Option<WolOptionsAction> {
        match id {
            OK => Some(WolOptionsAction::Ok(self.staged())),
            CANCEL => Some(WolOptionsAction::Cancel),
            SOUND_OPTIONS => Some(WolOptionsAction::SoundOptions),
            _ => None,
        }
    }

    /// IDOK's `BM_GETCHECK` reads, already in the committed form: the seven
    /// dwords as 0 or 1 and the two bytes as `staging != 0`.
    fn staged(&self) -> WolPreferences {
        let flag = |id| i32::from(self.is_checked(id));
        WolPreferences {
            allow_page: flag(PAGES),
            allow_find: flag(FIND),
            lang_filter: flag(LANGUAGE),
            lobby_music: flag(MUSIC),
            show_all: flag(SHOW_ALL),
            display_asian: flag(ASIAN),
            display_latin: flag(LATIN),
            display_buddies: self.is_checked(BUDDIES),
            display_clan: self.is_checked(CLAN),
        }
    }
}

/// `0x2BC` at the current screen: the family right panel and the checkbox
/// column at the executed relayout places (`0x0060C0C0` keeps them at every
/// size).
#[derive(Debug, Clone)]
pub(crate) struct WolOptionsLayout {
    pub(crate) page: MenuPageLayout,
    /// Status line `0x695`: the template is 316 DLU wide here.
    pub(crate) status_help: RectPx,
    /// Template order, as [`CHECKBOXES`].
    pub(crate) checkboxes: [RectPx; 9],
}

impl WolOptionsLayout {
    pub(crate) fn new(width: i32, height: i32) -> Self {
        Self::build(width, height, ShellScale::RETAIL)
    }

    pub(crate) fn responsive(width: i32, height: i32) -> Self {
        Self::build(width, height, ShellScale::from_screen(width, height))
    }

    fn build(width: i32, height: i32, scale: ShellScale) -> Self {
        let status_template = RectPx::new(2, 355, 316, 12);
        let (mut page, status_help) = if scale.is_retail() {
            (
                menu_page::compute_layout(&PAGE, width as u32, height as u32),
                status_line_rect(status_template, width, height),
            )
        } else {
            (
                menu_page::compute_responsive_layout(&PAGE, width as u32, height as u32),
                scale.scale_rect(status_line_rect(
                    status_template,
                    SHELL_BASE_W,
                    SHELL_BASE_H,
                )),
            )
        };
        // Heading 0x694 is 108x11 DLU, one row taller than the family's.
        let mut title =
            menu_page::compute_layout(&PAGE, SHELL_BASE_W as u32, SHELL_BASE_H as u32).title;
        title.h = dlu_rect(424, 4, 108, 11).h + 1;
        if scale.is_retail() {
            page.title.h = title.h;
        } else {
            page.title = scale.scale_rect(title);
        }
        Self {
            page,
            status_help,
            checkboxes: CHECKBOXES.map(|checkbox| {
                let (x, y, w, h) = checkbox.dlu;
                scale.scale_rect(child_window(x, y, w, h))
            }),
        }
    }

    /// The control under `(x, y)`, buttons first, then the checkboxes.
    pub(crate) fn control_at(&self, x: i32, y: i32) -> Option<u16> {
        if let Some(button) = self.page.buttons.iter().find(|b| b.rect.contains(x, y)) {
            return Some(button.id);
        }
        CHECKBOXES
            .iter()
            .zip(self.checkboxes)
            .find_map(|(checkbox, rect)| rect.contains(x, y).then_some(checkbox.id))
    }

    pub(crate) fn checkbox_rect(&self, id: u16) -> Option<RectPx> {
        CHECKBOXES
            .iter()
            .zip(self.checkboxes)
            .find_map(|(checkbox, rect)| (checkbox.id == id).then_some(rect))
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
    fn layout_and_help_follow_the_executed_0x2bc_table() {
        let corpus: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
            "tools/storage_oracle/wol_shell.json",
        ))
        .unwrap();
        let mut checked = 0;
        for (size, case) in corpus["layouts"]["0x2BC"]["sizes"].as_object().unwrap() {
            let (width, height) = size.split_once('x').unwrap();
            let layout = WolOptionsLayout::new(width.parse().unwrap(), height.parse().unwrap());
            for (key, child) in case["controls"].as_object().unwrap() {
                let id = u16::from_str_radix(&key[2..], 16).unwrap();
                let actual = match id {
                    // Hidden and disabled; the page neither draws nor hits it.
                    0x5FC => continue,
                    // The monitor is the shared family placement.
                    0x71C => layout.page.warning_monitor,
                    0x694 => layout.page.title,
                    0x695 => layout.status_help,
                    _ => layout
                        .checkbox_rect(id)
                        .or_else(|| layout.page.button_rect(id))
                        .unwrap_or_else(|| panic!("{key}")),
                };
                assert_eq!(actual, rect(&child["rect"]), "{size} {key}");
                checked += 1;
            }
        }
        assert_eq!(checked, 45);
        for (key, help) in corpus["help"]["0x2BC"].as_object().unwrap() {
            let id = u16::from_str_radix(&key[2..], 16).unwrap();
            assert_eq!(tooltip_key(id), help.as_str(), "{key}");
        }
    }

    #[test]
    fn ok_stages_checkbox_states_and_cancel_stages_nothing() {
        // Raw INI ints only matter as nonzero; OK writes 0 or 1.
        let live = WolPreferences {
            allow_page: 7,
            display_asian: 0,
            ..WolPreferences::default()
        };
        let mut dialog = WolOptionsState::open(&live);
        assert!(dialog.is_checked(PAGES) && !dialog.is_checked(ASIAN));
        assert!(!dialog.is_checked(BUDDIES));
        assert_eq!(dialog.toggle(PAGES), None);
        assert_eq!(dialog.toggle(BUDDIES), None);
        assert_eq!(
            dialog.toggle(MUSIC),
            Some(WolOptionsAction::MusicPreview(false))
        );
        let Some(WolOptionsAction::Ok(staged)) = dialog.activate(OK) else {
            panic!("OK commits");
        };
        assert_eq!(
            staged,
            WolPreferences {
                allow_page: 0,
                lobby_music: 0,
                display_asian: 0,
                display_buddies: true,
                ..WolPreferences::default()
            }
        );
        assert_eq!(dialog.activate(CANCEL), Some(WolOptionsAction::Cancel));
        assert_eq!(
            dialog.activate(SOUND_OPTIONS),
            Some(WolOptionsAction::SoundOptions)
        );
    }
}
