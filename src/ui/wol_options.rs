//! Westwood Online Options `0x2BC` (proc `0x0078EBC0`), opened only by lobby
//! `0x113`'s Options button `0x61F` (`0x0078B829..0x0078B88A`).
//!
//! The model owns the dialog's nine checkboxes and its results. The
//! `[WOnline]` preferences it shows and commits ([`WolPreferences`]) belong to
//! the app: the profile holds what VERA.INI says, and the WOL owner holds the
//! live process copy each WOL entry re-reads (`0x0077DED0`). VERA's online
//! lobby adds its chat translation settings
//! ([`crate::ui::chat_translation::ChatTranslation`]) under Play Music: the
//! language list and Translate in Matches. gamemd has no such controls.

use crate::ui::chat_translation::{ChatLanguage, ChatTranslation};
use crate::ui::shell::button::ShellButtonInteraction;
use crate::ui::shell::combobox::{ComboPopup, ComboPopupInteraction};
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
/// VERA's chat translation list; no retail control has this id.
pub(crate) const TRANSLATE: u16 = 0x7E10;
pub(crate) const TRANSLATE_CAPTION_KEY: &str = "VERA:TranslateChatInto";
/// The list's first row.
pub(crate) const TRANSLATE_OFF_KEY: &str = "VERA:ChatTranslationNone";
/// VERA's Translate in Matches checkbox under the list.
pub(crate) const TRANSLATE_MATCHES: u16 = 0x7E11;
pub(crate) const TRANSLATE_MATCHES_KEY: &str = "VERA:TranslateMatches";

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

/// Status help of `0x2BC`'s controls (executed `0x006040B0`), and of VERA's
/// translation controls.
pub(crate) fn tooltip_key(control: u16) -> Option<&'static str> {
    match control {
        TRANSLATE => return Some("VERA:ChatTranslationHelp"),
        TRANSLATE_MATCHES => return Some("VERA:TranslateMatchesHelp"),
        _ => {}
    }
    CHECKBOXES
        .iter()
        .find(|checkbox| checkbox.id == control)
        .map(|checkbox| checkbox.help_key)
        .or_else(|| PAGE.button(control).map(|spec| spec.tooltip_key))
}

/// What a press on the page asks its host to do.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum WolOptionsAction {
    /// IDOK: the staged values (`0x0078EC9A..0x0078EDE1`), and the chat
    /// translation settings.
    Ok(WolPreferences, ChatTranslation),
    /// IDCANCEL: nothing staged changes.
    Cancel,
    /// `0x63A`: the nested Sound Options page `0xD6`.
    SoundOptions,
    /// `0x5F8` notification 0/1: the live music preview, with the box's state.
    MusicPreview(bool),
}

/// The translation list's rows: Off, then every language.
fn translate_rows() -> impl Iterator<Item = Option<ChatLanguage>> {
    std::iter::once(None).chain(ChatLanguage::all().map(Some))
}

/// The open dialog: its checkbox states and button press.
#[derive(Debug, Clone)]
pub(crate) struct WolOptionsState {
    /// Template order, as [`CHECKBOXES`].
    pub(crate) checked: [bool; 9],
    /// The chat translation settings OK hands back.
    pub(crate) translate: ChatTranslation,
    pub(crate) translate_list: ComboPopupInteraction<u16>,
    pub(crate) buttons: ShellButtonInteraction<u16>,
    /// The control under the pointer, the status line's help source.
    pub(crate) hovered: Option<u16>,
}

impl WolOptionsState {
    /// Init `0x497`: every nonzero live value checks its box; unchecked is the
    /// template default.
    pub(crate) fn open(preferences: &WolPreferences, translate: ChatTranslation) -> Self {
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
            translate,
            translate_list: ComboPopupInteraction::default(),
            buttons: ShellButtonInteraction::default(),
            hovered: None,
        }
    }

    pub(crate) fn is_checked(&self, id: u16) -> bool {
        if id == TRANSLATE_MATCHES {
            return self.translate.in_matches;
        }
        CHECKBOXES
            .iter()
            .position(|checkbox| checkbox.id == id)
            .is_some_and(|index| self.checked[index])
    }

    /// A click on a checkbox's icon: BS_AUTOCHECKBOX flips it. Only Play
    /// Music has a proc case.
    pub(crate) fn toggle(&mut self, id: u16) -> Option<WolOptionsAction> {
        if id == TRANSLATE_MATCHES {
            self.translate.in_matches = !self.translate.in_matches;
            return None;
        }
        let index = CHECKBOXES.iter().position(|checkbox| checkbox.id == id)?;
        self.checked[index] = !self.checked[index];
        (id == MUSIC).then_some(WolOptionsAction::MusicPreview(self.checked[index]))
    }

    /// A released owner-draw button.
    pub(crate) fn activate(&self, id: u16) -> Option<WolOptionsAction> {
        match id {
            OK => Some(WolOptionsAction::Ok(self.staged(), self.translate)),
            CANCEL => Some(WolOptionsAction::Cancel),
            SOUND_OPTIONS => Some(WolOptionsAction::SoundOptions),
            _ => None,
        }
    }

    /// The translation list's row for the current language.
    pub(crate) fn translate_row(&self) -> usize {
        translate_rows()
            .position(|language| language == self.translate.language)
            .unwrap_or(0)
    }

    pub(crate) fn pick_translate_row(&mut self, row: usize) {
        if let Some(language) = translate_rows().nth(row) {
            self.translate.language = language;
        }
    }

    /// The name translation list row `row` shows; Off's comes from `off`.
    pub(crate) fn translate_row_text(row: usize, off: &str) -> String {
        translate_rows()
            .nth(row)
            .flatten()
            .map_or_else(|| off.to_owned(), |language| language.name().to_owned())
    }

    /// The open translation list.
    pub(crate) fn translate_popup(&self, layout: &WolOptionsLayout) -> Option<ComboPopup> {
        self.translate_list.control?;
        let list = layout.translate?.list;
        Some(ComboPopup::new(
            list,
            layout.page.scale,
            translate_rows().count(),
            0,
            self.translate_list.top_index,
        ))
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

/// VERA's translation controls under Play Music, on the modern page only.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct TranslateLayout {
    pub(crate) caption: RectPx,
    pub(crate) list: RectPx,
    /// The Translate in Matches checkbox row.
    pub(crate) in_matches: RectPx,
}

/// `0x2BC` at the current screen: the family right panel and the checkbox
/// column at the executed relayout places (`0x0060C0C0` keeps them at every
/// size).
#[derive(Debug, Clone)]
pub(crate) struct WolOptionsLayout {
    pub(crate) page: MenuPageLayout,
    /// Status line `0x695`: the template is 316 DLU wide here.
    pub(crate) status_help: RectPx,
    /// Template order, as [`CHECKBOXES`]; `None` for a box the page hides.
    pub(crate) checkboxes: [Option<RectPx>; 9],
    pub(crate) translate: Option<TranslateLayout>,
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
                Some(scale.scale_rect(child_window(x, y, w, h)))
            }),
            translate: None,
        }
    }

    /// The modern page keeps Play Music alone, in the first row; the other
    /// boxes neither draw nor hit. Paging, finding and the bad-language
    /// filter have no online service, nothing reads Show All, the language
    /// filters count any text beyond Latin-1 as Asian (`0x007B7D90`), and
    /// with no buddy list or clans Filter Buddies hides every sender and
    /// Filter Clan none. VERA's translation caption and list sit under Play
    /// Music, and Translate in Matches under them. Native diagnostics keep
    /// the retail page through the same layout and state owners.
    pub(crate) fn modern(mut self) -> Self {
        let first_row = self.checkboxes[0];
        for (checkbox, rect) in CHECKBOXES.iter().zip(&mut self.checkboxes) {
            *rect = if checkbox.id == MUSIC {
                first_row
            } else {
                None
            };
        }
        let (x, y, w, h) = CHECKBOXES[0].dlu;
        let row = child_window(x, y, w, h);
        let caption = RectPx::new(row.x, row.y + row.h + 10, row.w, 17);
        let list = RectPx::new(row.x, caption.y + caption.h, 180, 24);
        let in_matches = RectPx::new(row.x, list.y + list.h + 10, row.w, row.h);
        let scale = self.page.scale;
        self.translate = Some(TranslateLayout {
            caption: scale.scale_rect(caption),
            list: scale.scale_rect(list),
            in_matches: scale.scale_rect(in_matches),
        });
        self
    }

    /// The control under `(x, y)`, buttons first, then the checkboxes and
    /// the translation controls.
    pub(crate) fn control_at(&self, x: i32, y: i32) -> Option<u16> {
        if let Some(button) = self.page.buttons.iter().find(|b| b.rect.contains(x, y)) {
            return Some(button.id);
        }
        CHECKBOXES
            .iter()
            .zip(self.checkboxes)
            .find_map(|(checkbox, rect)| rect?.contains(x, y).then_some(checkbox.id))
            .or_else(|| {
                let translate = self.translate?;
                [
                    (TRANSLATE, translate.list),
                    (TRANSLATE_MATCHES, translate.in_matches),
                ]
                .into_iter()
                .find_map(|(id, rect)| rect.contains(x, y).then_some(id))
            })
    }

    /// A checkbox row, Translate in Matches's included.
    pub(crate) fn checkbox_rect(&self, id: u16) -> Option<RectPx> {
        if id == TRANSLATE_MATCHES {
            return self.translate.map(|translate| translate.in_matches);
        }
        CHECKBOXES
            .iter()
            .zip(self.checkboxes)
            .find(|(checkbox, _)| checkbox.id == id)
            .and_then(|(_, rect)| rect)
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
    fn modern_page_keeps_only_play_music() {
        let retail = WolOptionsLayout::responsive(1600, 1200);
        let modern = WolOptionsLayout::responsive(1600, 1200).modern();
        assert_eq!(modern.checkbox_rect(MUSIC), retail.checkbox_rect(PAGES));
        for checkbox in CHECKBOXES.iter().filter(|checkbox| checkbox.id != MUSIC) {
            assert_eq!(modern.checkbox_rect(checkbox.id), None);
        }
        let center = |id| {
            let rect = retail.checkbox_rect(id).unwrap();
            (rect.x + rect.w / 2, rect.y + rect.h / 2)
        };
        let (x, y) = center(PAGES);
        assert_eq!(modern.control_at(x, y), Some(MUSIC));
        let (x, y) = center(LANGUAGE);
        assert_eq!(modern.control_at(x, y), None);
        assert_eq!(retail.control_at(x, y), Some(LANGUAGE));
    }

    #[test]
    fn modern_page_picks_the_chat_translation_language_and_ok_hands_it_back() {
        let retail = WolOptionsLayout::responsive(1600, 1200);
        let modern = WolOptionsLayout::responsive(1600, 1200).modern();
        assert_eq!(retail.translate, None);
        let list = modern.translate.expect("the modern page has the list").list;
        let music = modern.checkbox_rect(MUSIC).unwrap();
        assert!(list.y > music.y + music.h, "the list sits under Play Music");
        assert_eq!(
            modern.control_at(list.x + list.w / 2, list.y + list.h / 2),
            Some(TRANSLATE)
        );

        let mut dialog =
            WolOptionsState::open(&WolPreferences::default(), ChatTranslation::default());
        assert_eq!(dialog.translate_row(), 0);
        dialog.translate_list.open(TRANSLATE, Some(0));
        let popup = dialog.translate_popup(&modern).unwrap();
        assert_eq!(popup.total, 15);
        dialog.pick_translate_row(2);
        assert_eq!(
            dialog.translate.language,
            Some(ChatLanguage::SimplifiedChinese)
        );
        assert_eq!(WolOptionsState::translate_row_text(2, "Off"), "简体中文");
        assert_eq!(WolOptionsState::translate_row_text(0, "Off"), "Off");
        assert_eq!(
            dialog.activate(OK),
            Some(WolOptionsAction::Ok(
                WolPreferences::default(),
                ChatTranslation {
                    language: Some(ChatLanguage::SimplifiedChinese),
                    in_matches: true,
                }
            ))
        );
    }

    #[test]
    fn translate_in_matches_sits_under_the_list_and_ok_hands_back_its_state() {
        let modern = WolOptionsLayout::responsive(1600, 1200).modern();
        let translate = modern.translate.unwrap();
        let row = modern.checkbox_rect(TRANSLATE_MATCHES).unwrap();
        assert!(row.y > translate.list.y + translate.list.h);
        assert_eq!(
            modern.control_at(row.x + 2, row.y + row.h / 2),
            Some(TRANSLATE_MATCHES)
        );
        assert_eq!(
            tooltip_key(TRANSLATE_MATCHES),
            Some("VERA:TranslateMatchesHelp")
        );

        let mut dialog =
            WolOptionsState::open(&WolPreferences::default(), ChatTranslation::default());
        assert!(dialog.is_checked(TRANSLATE_MATCHES));
        assert_eq!(dialog.toggle(TRANSLATE_MATCHES), None);
        let Some(WolOptionsAction::Ok(_, chat)) = dialog.activate(OK) else {
            panic!("OK commits");
        };
        assert!(!chat.in_matches);
    }

    #[test]
    fn ok_stages_checkbox_states_and_cancel_stages_nothing() {
        // Raw INI ints only matter as nonzero; OK writes 0 or 1.
        let live = WolPreferences {
            allow_page: 7,
            display_asian: 0,
            ..WolPreferences::default()
        };
        let mut dialog = WolOptionsState::open(&live, ChatTranslation::default());
        assert!(dialog.is_checked(PAGES) && !dialog.is_checked(ASIAN));
        assert!(!dialog.is_checked(BUDDIES));
        assert_eq!(dialog.toggle(PAGES), None);
        assert_eq!(dialog.toggle(BUDDIES), None);
        assert_eq!(
            dialog.toggle(MUSIC),
            Some(WolOptionsAction::MusicPreview(false))
        );
        let Some(WolOptionsAction::Ok(staged, _)) = dialog.activate(OK) else {
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
