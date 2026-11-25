//! Westwood Online Options `0x2BC` (proc `0x0078EBC0`), opened only by lobby
//! `0x113`'s Options button `0x61F` (`0x0078B829..0x0078B88A`).
//!
//! The classic dialog's widgets own its nine checkboxes' states; OK reads them. The
//! `[WOnline]` preferences it shows and commits ([`WolPreferences`]) belong to
//! the app: the profile holds what VERA.INI says, and the WOL owner holds the
//! live process copy each WOL entry re-reads (`0x0077DED0`). VERA's online
//! lobby applies settings as they change. It adds a translation checkbox
//! and language list under Play Music and embeds the three volume sliders.

use vera20k_gui::{Gui, NodeId, NodeKind, NodeSpec, Prop, Value};

use crate::ui::chat_translation::{ChatLanguage, ChatTranslation};
use crate::ui::main_menu_dialogs::options::{Category, LauncherTrackbarId, Setting};
use crate::ui::shell::descriptor::DialogResourceId;
use crate::ui::shell::geom::{RectPx, ShellScale, shell_scale};
use crate::ui::shell::layout::RectTable;
use crate::ui::shell::layout_data;
use crate::ui::shell::menu_page::{MenuPageButtonSpec, MenuPageLayout, MenuPageSpec};
use crate::ui::shell::page::{self, PageEvent};
use crate::ui::shell::sound::{self, SHELL_SOUND_LABELS, SHELL_SOUND_SLIDERS, SoundSlider};
use vera20k_gui::shell::combobox::COMBO_FACE_H;

pub(crate) const DIALOG: DialogResourceId = DialogResourceId(0x02BC);
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
/// VERA's Translate in Matches checkbox above the list caption.
pub(crate) const TRANSLATE_MATCHES: u16 = 0x7E11;
pub(crate) const TRANSLATE_MATCHES_KEY: &str = "VERA:TranslateMatches";
/// The translation caption static (no control id).
const TRANSLATE_CAPTION: u16 = 0xFFFF;
const AUDIO_HEADING: u16 = 0x7E12;
const GAME_HEADING: u16 = 0x7E13;
const COMMUNICATION_HEADING: u16 = 0x7E14;
/// Leave room for the volume group when the language list is open.
const TRANSLATE_VISIBLE_ROWS: i32 = 6;

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
/// `0x0060B950`; rects in `shell_layout.ini`), in template order. Help keys
/// from the executed `0x006040B0`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct OptionCheckbox {
    pub(crate) id: u16,
    pub(crate) csf_key: &'static str,
    pub(crate) help_key: &'static str,
}

pub(crate) const CHECKBOXES: [OptionCheckbox; 9] = [
    checkbox(PAGES, "GUI:AllowPages", "STT:WOLOptionsPage"),
    checkbox(LANGUAGE, "GUI:FilterBadLan", "STT:WOLOptionsLanguageFilter"),
    checkbox(FIND, "GUI:AllowFinding", "STT:WOLOptionsFind"),
    checkbox(MUSIC, "GUI:PlayMusic", "STT:WOLOptionsMusic"),
    checkbox(
        SHOW_ALL,
        "GUI:ShowAllInLobby",
        "STT:WOLOptionsShowAllInLobby",
    ),
    checkbox(ASIAN, "GUI:FilterAsianMessages", "STT:WOLOptionFilterAsian"),
    checkbox(
        LATIN,
        "GUI:FilterNonAsianMessages",
        "STT:WOLOptionFilterNonAsian",
    ),
    checkbox(BUDDIES, "GUI:FilterBuddies", "STT:WOLOptionFilterBuddies"),
    checkbox(CLAN, "GUI:FilterClan", "STT:WOLOptionFilterClan"),
];

const fn checkbox(id: u16, csf_key: &'static str, help_key: &'static str) -> OptionCheckbox {
    OptionCheckbox {
        id,
        csf_key,
        help_key,
    }
}

/// RT_DIALOG `0x2BC` as a family page (`0x0060C540` accepts it): OK and
/// Sound Options stacked by template tops, Cancel on the bottom row. The
/// hidden, disabled Observe box `0x5FC` is never shown or touched.
pub(crate) const PAGE: MenuPageSpec = MenuPageSpec {
    dialog: DIALOG,
    title_key: "GUI:WOLOptions",
    stacked: &[
        MenuPageButtonSpec::new(OK, 111, "GUI:OK", "STT:WOLOptionsOk").returning(1),
        MenuPageButtonSpec::new(
            SOUND_OPTIONS,
            140,
            "GUI:SoundOptions",
            "STT:WOLOptionsSoundOptions",
        ),
    ],
    back: MenuPageButtonSpec::new(CANCEL, 251, "GUI:Cancel", "STT:WOLOptionsCancel").returning(2),
};

pub(crate) const MODERN_PAGE: MenuPageSpec = MenuPageSpec {
    stacked: &[],
    back: MenuPageButtonSpec::new(CANCEL, 251, "VERA:Previous", "STT:SoundOptButtonBack"),
    ..PAGE
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
    Back,
    LobbyMusic(bool),
    Translation(ChatTranslation),
}

/// The translation list's rows: Off, then every language.
fn translate_rows() -> impl Iterator<Item = Option<ChatLanguage>> {
    std::iter::once(None).chain(ChatLanguage::all().map(Some))
}

impl WolPreferences {
    /// Init `0x497`, in [`CHECKBOXES`] order: every nonzero live value checks
    /// its box; unchecked is the template default.
    pub(crate) fn checks(&self) -> [bool; 9] {
        [
            self.allow_page != 0,
            self.lang_filter != 0,
            self.allow_find != 0,
            self.lobby_music != 0,
            self.show_all != 0,
            self.display_asian != 0,
            self.display_latin != 0,
            self.display_buddies,
            self.display_clan,
        ]
    }

    /// IDOK's `BM_GETCHECK` reads, already in the committed form: the seven
    /// dwords as 0 or 1 and the two bytes as `staging != 0`.
    fn from_checks(c: [bool; 9]) -> Self {
        let flag = i32::from;
        Self {
            allow_page: flag(c[0]),
            lang_filter: flag(c[1]),
            allow_find: flag(c[2]),
            lobby_music: flag(c[3]),
            show_all: flag(c[4]),
            display_asian: flag(c[5]),
            display_latin: flag(c[6]),
            display_buddies: c[7],
            display_clan: c[8],
        }
    }
}

/// The page's widgets in hit-test order, showing `shown`: the buttons, the
/// checkboxes, then VERA's translation controls, whose list's Off row reads
/// `off`. The checkboxes take their press on the icon
/// (`OwnerDraw_CheckBox 0x006163A0` toggles on it); every control carries
/// its status help.
pub(crate) fn spec(
    (preferences, translate): (WolPreferences, ChatTranslation),
    off: &str,
    audio: Option<([u8; 3], bool)>,
) -> NodeSpec {
    let menu = if audio.is_some() { &MODERN_PAGE } else { &PAGE };
    let control = |kind, id: u16| {
        let node = NodeSpec::new(kind).id(id.into());
        match tooltip_key(id) {
            Some(key) => node.tooltip(key),
            None => node,
        }
    };
    let checkbox = |id, key, on| {
        control(NodeKind::Checkbox, id)
            .label(key)
            .with(Prop::Checked, Value::Bool(on))
    };
    let rows = translate_rows()
        .map(|language| language.map_or_else(|| off.to_owned(), |l| l.name().to_owned()))
        .collect();
    let row = translate_rows()
        .position(|language| language == translate.language)
        .unwrap_or(0);
    let spec = menu
        .nodes()
        .children(
            CHECKBOXES
                .iter()
                .zip(preferences.checks())
                .map(|(spec, on)| checkbox(spec.id, spec.csf_key, on)),
        )
        .child(control(NodeKind::Text, TRANSLATE_CAPTION).label(TRANSLATE_CAPTION_KEY))
        .child(
            control(NodeKind::Combo, TRANSLATE)
                .with(Prop::Items, Value::Items(rows))
                .with(Prop::Selected, Value::Int(row as i32))
                .with(Prop::VisibleRows, Value::Int(TRANSLATE_VISIBLE_ROWS)),
        )
        .child(checkbox(
            TRANSLATE_MATCHES,
            TRANSLATE_MATCHES_KEY,
            translate.in_matches,
        ));
    let Some((positions, enabled)) = audio else {
        return spec;
    };
    let sliders = [
        LauncherTrackbarId::Score,
        LauncherTrackbarId::Sound,
        LauncherTrackbarId::Voice,
    ];
    spec.children([
        control(NodeKind::Text, GAME_HEADING).label("VERA:WolGameOptions"),
        control(NodeKind::Text, COMMUNICATION_HEADING).label("VERA:WolCommunication"),
        control(NodeKind::Text, AUDIO_HEADING).label(Category::Audio.heading()),
    ])
    .children(
        SoundSlider::ALL
            .into_iter()
            .zip(sliders)
            .flat_map(|(channel, slider)| {
                let (caption, help) = Setting::Slider(slider).text();
                [
                    control(NodeKind::Text, SHELL_SOUND_LABELS[channel as usize]).label(caption),
                    sound::trackbar(channel, positions, enabled).tooltip(help),
                ]
            }),
    )
}

/// What the page's boxes and list show now: IDOK's staged values and the
/// translation settings.
pub(crate) fn shown(gui: &Gui, root: NodeId) -> (WolPreferences, ChatTranslation) {
    let preferences = WolPreferences::from_checks(
        CHECKBOXES.map(|checkbox| page::checked(gui, root, checkbox.id)),
    );
    let row = page::int(gui, root, TRANSLATE, Prop::Selected).unwrap_or(0);
    let translate = ChatTranslation {
        language: translate_rows().nth(row.max(0) as usize).flatten(),
        in_matches: page::checked(gui, root, TRANSLATE_MATCHES),
    };
    (preferences, translate)
}

/// What one of the page's control events asks its host to do.
pub(crate) fn action(
    gui: &Gui,
    root: NodeId,
    event: &PageEvent,
    modern: bool,
) -> Option<WolOptionsAction> {
    if modern {
        return match *event {
            PageEvent::Activated(CANCEL) => Some(WolOptionsAction::Back),
            PageEvent::Toggled(MUSIC, on) => Some(WolOptionsAction::LobbyMusic(on)),
            PageEvent::Picked(TRANSLATE, _) | PageEvent::Toggled(TRANSLATE_MATCHES, _) => {
                Some(WolOptionsAction::Translation(shown(gui, root).1))
            }
            _ => None,
        };
    }
    match *event {
        PageEvent::Activated(OK) => {
            let (preferences, translate) = shown(gui, root);
            Some(WolOptionsAction::Ok(preferences, translate))
        }
        PageEvent::Activated(CANCEL) => Some(WolOptionsAction::Cancel),
        PageEvent::Activated(SOUND_OPTIONS) => Some(WolOptionsAction::SoundOptions),
        PageEvent::Toggled(MUSIC, on) => Some(WolOptionsAction::MusicPreview(on)),
        _ => None,
    }
}

/// `0x2BC` at the current screen: the family right panel and the checkbox
/// column at the executed relayout places (`0x0060C0C0` keeps them at every
/// size).
#[derive(Debug, Clone)]
pub(crate) struct WolOptionsLayout {
    pub(crate) page: MenuPageLayout,
    /// The checkboxes, without those the page hides, and VERA's translation
    /// controls.
    pub(crate) rects: RectTable,
}

impl WolOptionsLayout {
    pub(crate) fn new(width: i32, height: i32, scale: ShellScale) -> Self {
        let (page, rects) = layout_data::menu_page(&PAGE, "0x2BC", scale, width, height);
        Self { page, rects }
    }

    /// The modern page uses the fitted shell at every size so its three
    /// volume controls stay clear of the buttons even in the 640px window.
    pub(crate) fn modern(width: i32, height: i32) -> Self {
        let scale = shell_scale(width, height);
        let (page, mut rects) = layout_data::menu_page(&MODERN_PAGE, "0x2BC", scale, width, height);
        for checkbox in CHECKBOXES {
            rects.remove(checkbox.id);
        }

        let heading_x = 72;
        let control_x = 104;
        let heading = |y| RectPx::new(heading_x, y, 240, 17);
        let checkbox = |y| RectPx::new(control_x, y, 333, 25);
        for (id, rect) in [
            (GAME_HEADING, heading(58)),
            (MUSIC, checkbox(84)),
            (COMMUNICATION_HEADING, heading(132)),
            (TRANSLATE_MATCHES, checkbox(158)),
            (TRANSLATE_CAPTION, RectPx::new(control_x, 193, 240, 17)),
            (TRANSLATE, RectPx::new(control_x, 211, 180, COMBO_FACE_H)),
            (AUDIO_HEADING, heading(390)),
        ] {
            rects.insert(id, scale.scale_rect(rect));
        }
        let (first_caption, _) = sound::launcher_volume_rects(SoundSlider::Music);
        let dx = control_x - first_caption.x;
        let dy = 420 - first_caption.y;
        for channel in SoundSlider::ALL {
            let index = channel as usize;
            let (caption, track) = sound::launcher_volume_rects(channel);
            rects.insert(
                SHELL_SOUND_LABELS[index],
                scale.scale_rect(caption.translate(dx, dy)),
            );
            rects.insert(
                SHELL_SOUND_SLIDERS[index],
                scale.scale_rect(track.translate(dx, dy)),
            );
        }
        Self { page, rects }
    }

    /// Control `id`'s rect, `None` for one the page hides.
    pub(crate) fn rect(&self, id: u16) -> Option<RectPx> {
        self.rects.get(id).or_else(|| self.page.button_rect(id))
    }

    /// A checkbox row, Translate in Matches's included.
    pub(crate) fn checkbox_rect(&self, id: u16) -> Option<RectPx> {
        self.rects.get(id)
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
            let layout = WolOptionsLayout::new(
                width.parse().unwrap(),
                height.parse().unwrap(),
                crate::ui::shell::geom::RETAIL_SCALE,
            );
            for (key, child) in case["controls"].as_object().unwrap() {
                let id = u16::from_str_radix(&key[2..], 16).unwrap();
                let actual = match id {
                    // Hidden and disabled; the page neither draws nor hits it.
                    0x5FC => continue,
                    // The monitor is the shared family placement.
                    0x71C => layout.page.warning_monitor,
                    0x694 => layout.page.title,
                    0x695 => layout.page.status_help,
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

    /// The page's widgets on their own GUI, driven by pointer input.
    struct Page {
        gui: Gui,
        root: NodeId,
        modern: bool,
    }

    impl Page {
        fn new(layout: &WolOptionsLayout, preferences: &WolPreferences) -> Self {
            let mut gui = Gui::new(64);
            let root = gui.create(NodeKind::Dialog, None).unwrap();
            let id = gui
                .push_dialog(root, vera20k_gui::DialogKind::Page)
                .unwrap();
            gui.set_dialog_scale(id, layout.page.scale);
            let modern = layout.rect(AUDIO_HEADING).is_some();
            page::ensure_built(&mut gui, root, || {
                let audio = modern.then_some(([4, 7, 7], true));
                spec((*preferences, ChatTranslation::default()), "Off", audio)
            });
            page::sync_rects(&mut gui, root, |id| layout.rect(id));
            Self { gui, root, modern }
        }

        fn hover(&mut self, x: i32, y: i32) -> Option<u16> {
            self.gui
                .input(vera20k_gui::InputEvent::PointerMove { x, y }, 0);
            self.gui.drain_events();
            let node = self.gui.hovered()?;
            self.gui.id_of(node).map(|id| id as u16)
        }

        fn click(&mut self, rect: RectPx) -> Vec<PageEvent> {
            use vera20k_gui::{InputEvent, MouseButton};
            let (x, y) = (rect.x + rect.w / 2, rect.y + rect.h / 2);
            let button = MouseButton::Left;
            self.gui.input(InputEvent::PointerMove { x, y }, 0);
            self.gui.input(InputEvent::PointerDown { x, y, button }, 0);
            self.gui.input(InputEvent::PointerUp { x, y, button }, 0);
            let events = self.gui.drain_events();
            page::page_events(&self.gui, events)
        }

        fn action(&self, event: &PageEvent) -> Option<WolOptionsAction> {
            action(&self.gui, self.root, event, self.modern)
        }

        fn is_checked(&self, id: u16) -> bool {
            page::checked(&self.gui, self.root, id)
        }

        fn icon(layout: &WolOptionsLayout, id: u16) -> RectPx {
            crate::ui::skirmish_shell::checkbox_icon_rect(
                layout.checkbox_rect(id).unwrap(),
                layout.page.scale,
            )
        }
    }

    #[test]
    fn modern_page_keeps_only_play_music() {
        let retail =
            WolOptionsLayout::new(1600, 1200, crate::ui::shell::geom::shell_scale(1600, 1200));
        let modern = WolOptionsLayout::modern(1600, 1200);
        assert!(modern.checkbox_rect(MUSIC).is_some());
        for checkbox in CHECKBOXES.iter().filter(|checkbox| checkbox.id != MUSIC) {
            assert_eq!(modern.checkbox_rect(checkbox.id), None);
        }
        let center = |layout: &WolOptionsLayout, id| {
            let rect = layout.checkbox_rect(id).unwrap();
            (rect.x + rect.w / 2, rect.y + rect.h / 2)
        };
        let mut modern_page = Page::new(&modern, &WolPreferences::default());
        let mut retail_page = Page::new(&retail, &WolPreferences::default());
        let (x, y) = center(&modern, MUSIC);
        assert_eq!(modern_page.hover(x, y), Some(MUSIC));
        let (x, y) = center(&retail, LANGUAGE);
        assert_ne!(modern_page.hover(x, y), Some(LANGUAGE));
        assert_eq!(retail_page.hover(x, y), Some(LANGUAGE));
    }

    #[test]
    fn modern_page_applies_a_picked_language_before_back() {
        use crate::ui::shell::page::PageCue;
        let retail =
            WolOptionsLayout::new(1600, 1200, crate::ui::shell::geom::shell_scale(1600, 1200));
        let modern = WolOptionsLayout::modern(1600, 1200);
        assert_eq!(retail.rect(TRANSLATE), None);
        let list = modern
            .rect(TRANSLATE)
            .expect("the modern page has the list");
        let music = modern.checkbox_rect(MUSIC).unwrap();
        assert!(list.y > music.y + music.h, "the list sits under Play Music");
        let mut page = Page::new(&modern, &WolPreferences::default());
        assert_eq!(
            page.hover(list.x + list.w / 2, list.y + list.h / 2),
            Some(TRANSLATE)
        );

        assert_eq!(
            page::int(&page.gui, page.root, TRANSLATE, Prop::Selected),
            Some(0)
        );
        let arrow = RectPx::new(list.x + list.w - 12, list.y, 4, list.h);
        assert_eq!(page.click(arrow), [PageEvent::Cue(PageCue::ComboOpen)]);
        let node = page.gui.find_id(page.root, TRANSLATE.into()).unwrap();
        let popup = page.gui.combo_drop_down(node).unwrap().popup;
        assert_eq!(popup.total, 15);
        assert_eq!(
            page.click(popup.row_rect(2).unwrap()),
            [
                PageEvent::Cue(PageCue::ComboClose),
                PageEvent::Picked(TRANSLATE, 2)
            ]
        );
        assert_eq!(
            shown(&page.gui, page.root).1.language,
            Some(ChatLanguage::SimplifiedChinese)
        );
        let Ok(Some(Value::Items(rows))) = page.gui.get(node, Prop::Items) else {
            panic!("the list has rows");
        };
        assert_eq!(rows[2], "简体中文");
        assert_eq!(rows[0], "Off");
        assert_eq!(
            page.action(&PageEvent::Picked(TRANSLATE, 2)),
            Some(WolOptionsAction::Translation(ChatTranslation {
                language: Some(ChatLanguage::SimplifiedChinese),
                in_matches: true,
            }))
        );
        let back = modern.page.button_rect(CANCEL).unwrap();
        assert_eq!(Some(back), retail.page.button_rect(CANCEL));
        let events = page.click(back);
        assert_eq!(
            events,
            [
                PageEvent::Cue(PageCue::Button),
                PageEvent::Activated(CANCEL)
            ]
        );
        assert_eq!(page.action(&events[1]), Some(WolOptionsAction::Back));
    }

    #[test]
    fn translate_in_matches_sits_above_the_caption_and_applies_on_toggle() {
        use crate::ui::shell::page::PageCue;
        let modern = WolOptionsLayout::modern(1600, 1200);
        let list = modern.rect(TRANSLATE).unwrap();
        let row = modern.checkbox_rect(TRANSLATE_MATCHES).unwrap();
        let caption = modern.rect(TRANSLATE_CAPTION).unwrap();
        assert!(row.y + row.h <= caption.y && caption.y + caption.h <= list.y);
        let mut page = Page::new(&modern, &WolPreferences::default());
        assert_eq!(
            page.hover(row.x + 2, row.y + row.h / 2),
            Some(TRANSLATE_MATCHES)
        );
        assert_eq!(
            tooltip_key(TRANSLATE_MATCHES),
            Some("VERA:TranslateMatchesHelp")
        );

        assert!(page.is_checked(TRANSLATE_MATCHES));
        let events = page.click(Page::icon(&modern, TRANSLATE_MATCHES));
        assert_eq!(
            events,
            [
                PageEvent::Cue(PageCue::Checkbox),
                PageEvent::Toggled(TRANSLATE_MATCHES, false)
            ]
        );
        assert_eq!(
            page.action(&events[1]),
            Some(WolOptionsAction::Translation(ChatTranslation {
                in_matches: false,
                ..ChatTranslation::default()
            }))
        );
    }

    #[test]
    fn modern_page_moves_the_launcher_volume_row_and_keeps_its_bindings() {
        let layout = WolOptionsLayout::modern(800, 600);
        let mut page = Page::new(&layout, &WolPreferences::default());
        for id in [OK, SOUND_OPTIONS] {
            assert!(layout.rect(id).is_none());
            assert!(page.gui.find_id(page.root, id.into()).is_none());
        }
        for (index, channel) in SoundSlider::ALL.into_iter().enumerate() {
            let track = layout.rect(SHELL_SOUND_SLIDERS[index]).unwrap();
            assert_eq!(track, RectPx::new(104 + index as i32 * 154, 441, 129, 22));
            let events = page.click(RectPx::new(track.x + 1, track.y, 2, track.h));
            assert_eq!(
                events
                    .iter()
                    .filter_map(sound::shell_sound_volume)
                    .collect::<Vec<_>>(),
                [(channel, 0.0)]
            );
        }
        assert_eq!(
            sound::shell_sound_positions(&page.gui, page.root),
            [0, 0, 0]
        );
    }

    #[test]
    fn the_scrollable_language_list_stays_clear_of_the_volume_group() {
        for (width, height) in [(640, 480), (800, 600), (1280, 720)] {
            let layout = WolOptionsLayout::modern(width, height);
            let game = layout.rect(GAME_HEADING).unwrap();
            let communication = layout.rect(COMMUNICATION_HEADING).unwrap();
            let audio = layout.rect(AUDIO_HEADING).unwrap();
            assert_eq!([game.x, communication.x], [audio.x; 2]);
            let list = layout.rect(TRANSLATE).unwrap();
            assert_eq!(layout.rect(MUSIC).unwrap().x, list.x);
            assert_eq!(layout.rect(SHELL_SOUND_SLIDERS[0]).unwrap().x, list.x);
            let track = layout.rect(SHELL_SOUND_SLIDERS[2]).unwrap();
            assert!(track.x + track.w < layout.page.button_rect(CANCEL).unwrap().x);
            assert!(track.y + track.h < layout.page.status_help.y);

            let mut page = Page::new(&layout, &WolPreferences::default());
            let arrow = vera20k_gui::shell::combobox::combo_arrow_hit_rect(list, layout.page.scale);
            page.click(arrow);
            let node = page.gui.find_id(page.root, TRANSLATE.into()).unwrap();
            let popup = page.gui.combo_drop_down(node).unwrap().popup;
            assert!(popup.rect.y + popup.rect.h < audio.y, "{width}x{height}");
            let bar = popup.scrollbar().unwrap();
            let down = RectPx::new(bar.x, bar.y + bar.h - 2, bar.w, 2);
            for _ in 0..(popup.total - popup.visible_rows) {
                page.click(down);
            }
            let popup = page.gui.combo_drop_down(node).unwrap().popup;
            let last = popup.total - 1;
            let events = page.click(popup.row_rect(last).unwrap());
            assert!(events.contains(&PageEvent::Picked(TRANSLATE, last as i32)));
        }
    }

    #[test]
    fn ok_stages_checkbox_states_and_cancel_stages_nothing() {
        use crate::ui::shell::page::PageCue;
        // Raw INI ints only matter as nonzero; OK writes 0 or 1.
        let live = WolPreferences {
            allow_page: 7,
            display_asian: 0,
            ..WolPreferences::default()
        };
        let layout = WolOptionsLayout::new(800, 600, crate::ui::shell::geom::RETAIL_SCALE);
        let mut page = Page::new(&layout, &live);
        assert!(page.is_checked(PAGES) && !page.is_checked(ASIAN));
        assert!(!page.is_checked(BUDDIES));
        for (id, on) in [(PAGES, false), (BUDDIES, true)] {
            let events = page.click(Page::icon(&layout, id));
            assert_eq!(
                events,
                [
                    PageEvent::Cue(PageCue::Checkbox),
                    PageEvent::Toggled(id, on)
                ]
            );
            assert_eq!(page.action(&events[1]), None);
        }
        let events = page.click(Page::icon(&layout, MUSIC));
        assert_eq!(
            page.action(&events[1]),
            Some(WolOptionsAction::MusicPreview(false))
        );
        let click = |page: &mut Page, id| {
            let events = page.click(layout.page.button_rect(id).unwrap());
            assert_eq!(events[0], PageEvent::Cue(PageCue::Button));
            page.action(&events[1])
        };
        let Some(WolOptionsAction::Ok(staged, _)) = click(&mut page, OK) else {
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
        assert_eq!(click(&mut page, CANCEL), Some(WolOptionsAction::Cancel));
        assert_eq!(
            click(&mut page, SOUND_OPTIONS),
            Some(WolOptionsAction::SoundOptions)
        );
    }
}
