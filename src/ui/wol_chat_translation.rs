//! VERA's Chat Translation page, opened by the online lobby's sidebar button;
//! gamemd has none. A family page like WOL Options `0x2BC`, whose art it
//! borrows. The model owns a copy of the player's choices: OK hands it back
//! to the options profile, Cancel drops it.

use crate::ui::chat_translation::{
    ChatLanguage, ChatPlace, ChatReply, ChatTranslationSettings, MARKER,
};
use crate::ui::shell::button::ShellButtonInteraction;
use crate::ui::shell::combobox::{ComboPopup, ComboPopupInteraction};
use crate::ui::shell::descriptor::DialogId;
use crate::ui::shell::geom::{RectPx, SHELL_BASE_H, SHELL_BASE_W, ShellScale, dlu_rect};
use crate::ui::shell::layout::status_line_rect;
use crate::ui::shell::menu_page::{self, MenuPageButtonSpec, MenuPageLayout, MenuPageSpec};
use crate::ui::wol_chat::{WolSender, received_line};

/// Not a retail dialog: their ids stop near `0xBC7`.
pub(crate) const DIALOG: DialogId = DialogId(0x7E01);
pub(crate) const OK: u16 = 0x0001;
pub(crate) const CANCEL: u16 = 0x0002;
pub(crate) const LANGUAGE: u16 = 0x7E10;
const PLACE_FIRST: u16 = 0x7E11;
pub(crate) const SHOW_SOURCE: u16 = 0x7E15;
pub(crate) const SAME_ALPHABET: u16 = 0x7E16;
const PREVIEW: u16 = 0x7E17;
const READS_FIRST: u16 = 0x7E20;

/// The sidebar button's caption and the page's heading.
pub(crate) const TITLE_KEY: &str = "VERA:ChatTranslation";
pub(crate) const BUTTON_HELP_KEY: &str = "VERA:ChatTranslationButtonHelp";
pub(crate) const OFF_KEY: &str = "VERA:ChatTranslationNone";
pub(crate) const INTO_KEY: &str = "VERA:TranslateChatInto";
pub(crate) const PLACES_KEY: &str = "VERA:TranslateIn";
pub(crate) const LINES_KEY: &str = "VERA:TranslateLines";
pub(crate) const READS_KEY: &str = "VERA:TranslateKnown";
pub(crate) const PREVIEW_KEY: &str = "VERA:TranslatePreview";
pub(crate) const PREVIEW_OFF_KEY: &str = "VERA:TranslatePreviewOff";
pub(crate) const PRIVACY_KEY: &str = "VERA:TranslatePrivacy";
pub(crate) const SHOW_SOURCE_KEY: &str = "VERA:TranslateShowSource";
pub(crate) const SAME_ALPHABET_KEY: &str = "VERA:TranslateSameAlphabet";

/// Each place's checkbox label and status help, [`ChatPlace::ALL`] order.
pub(crate) const PLACE_KEYS: [(&str, &str); 4] = [
    ("VERA:TranslateLobby", "VERA:TranslateLobbyHelp"),
    ("VERA:TranslateRooms", "VERA:TranslateRoomsHelp"),
    ("VERA:TranslateMatches", "VERA:TranslateMatchesHelp"),
    ("VERA:TranslatePrivate", "VERA:TranslatePrivateHelp"),
];

const fn page_button(
    id: u16,
    dlu_top: i32,
    csf_key: &'static str,
    tooltip_key: &'static str,
    result: i32,
) -> MenuPageButtonSpec {
    MenuPageButtonSpec {
        id,
        dlu_top,
        csf_key,
        tooltip_key,
        result: Some(result),
    }
}

/// OK on the column's first rung, Cancel on the bottom row.
pub(crate) const PAGE: MenuPageSpec = MenuPageSpec {
    dialog: DIALOG,
    title_key: TITLE_KEY,
    stacked: &[page_button(OK, 111, "GUI:OK", "STT:WOLOptionsOk", 1)],
    back: page_button(CANCEL, 251, "GUI:Cancel", "STT:WOLOptionsCancel", 2),
};

/// What a press on the page asks its host to do.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ChatTranslationAction {
    Ok(ChatTranslationSettings),
    Cancel,
}

/// The open page: its copy of the settings and its controls' pointer state.
#[derive(Debug, Clone)]
pub(crate) struct WolChatTranslationPage {
    pub(crate) settings: ChatTranslationSettings,
    pub(crate) buttons: ShellButtonInteraction<u16>,
    pub(crate) popup: ComboPopupInteraction<u16>,
    /// The control under the pointer, the status line's help source.
    pub(crate) hovered: Option<u16>,
}

/// The language list's rows: off, then every language.
fn language_rows() -> impl Iterator<Item = Option<ChatLanguage>> {
    std::iter::once(None).chain(ChatLanguage::all().map(Some))
}

/// A checkbox on the page, in draw order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Checkbox {
    Place(ChatPlace),
    ShowSource,
    SameAlphabet,
    Reads(ChatLanguage),
}

fn checkbox(control: u16) -> Option<Checkbox> {
    match control {
        SHOW_SOURCE => Some(Checkbox::ShowSource),
        SAME_ALPHABET => Some(Checkbox::SameAlphabet),
        _ => {
            let place = usize::from(control.checked_sub(PLACE_FIRST)?);
            if control >= READS_FIRST {
                return ChatLanguage::all()
                    .nth(usize::from(control - READS_FIRST))
                    .map(Checkbox::Reads);
            }
            ChatPlace::ALL.get(place).copied().map(Checkbox::Place)
        }
    }
}

pub(crate) fn place_control(index: usize) -> u16 {
    PLACE_FIRST + index as u16
}

pub(crate) fn reads_control(index: usize) -> u16 {
    READS_FIRST + index as u16
}

impl WolChatTranslationPage {
    pub(crate) fn open(settings: ChatTranslationSettings) -> Self {
        Self {
            settings,
            buttons: ShellButtonInteraction::default(),
            popup: ComboPopupInteraction::default(),
            hovered: None,
        }
    }

    /// The language list's row for the current choice.
    pub(crate) fn selected_row(&self) -> usize {
        language_rows()
            .position(|language| language == self.settings.language)
            .unwrap_or(0)
    }

    pub(crate) fn rows() -> usize {
        language_rows().count()
    }

    /// The name a list row shows; the off row's comes from `off`.
    pub(crate) fn row_text(row: usize, off: &str) -> String {
        language_rows()
            .nth(row)
            .flatten()
            .map_or_else(|| off.to_owned(), |language| language.name().to_owned())
    }

    pub(crate) fn select_row(&mut self, row: usize) {
        if let Some(language) = language_rows().nth(row) {
            self.settings.language = language;
        }
    }

    /// With translation off only the list and the buttons work. The language
    /// being translated into is always read, and only languages that share
    /// an alphabet have anything to add.
    pub(crate) fn enabled(&self, control: u16) -> bool {
        match checkbox(control) {
            None => true,
            Some(_) if self.settings.language.is_none() => false,
            Some(Checkbox::SameAlphabet) => self
                .settings
                .language
                .is_some_and(ChatLanguage::shares_alphabet),
            Some(Checkbox::Reads(language)) => self.settings.language != Some(language),
            Some(_) => true,
        }
    }

    pub(crate) fn is_checked(&self, control: u16) -> bool {
        match checkbox(control) {
            Some(Checkbox::Place(place)) => self.settings.translates(place),
            Some(Checkbox::ShowSource) => self.settings.show_source,
            Some(Checkbox::SameAlphabet) => self.settings.same_alphabet && self.enabled(control),
            Some(Checkbox::Reads(language)) => {
                self.settings.language == Some(language) || self.settings.reads(language)
            }
            None => false,
        }
    }

    /// A click on an enabled checkbox's icon. Returns whether it flipped.
    pub(crate) fn toggle(&mut self, control: u16) -> bool {
        if !self.enabled(control) {
            return false;
        }
        let settings = &mut self.settings;
        match checkbox(control) {
            Some(Checkbox::Place(place)) => {
                settings.set_translates(place, !settings.translates(place));
            }
            Some(Checkbox::ShowSource) => settings.show_source = !settings.show_source,
            Some(Checkbox::SameAlphabet) => settings.same_alphabet = !settings.same_alphabet,
            Some(Checkbox::Reads(language)) => {
                settings.set_reads(language, !settings.reads(language));
            }
            None => return false,
        }
        true
    }

    /// A released button.
    pub(crate) fn activate(&self, control: u16) -> Option<ChatTranslationAction> {
        match control {
            OK => Some(ChatTranslationAction::Ok(self.settings)),
            CANCEL => Some(ChatTranslationAction::Cancel),
            _ => None,
        }
    }

    /// The status help of `control`, which can depend on the language.
    pub(crate) fn tooltip_key(&self, control: u16) -> Option<&'static str> {
        if let Some(spec) = PAGE.button(control) {
            return Some(spec.tooltip_key);
        }
        Some(match (control, checkbox(control)) {
            (LANGUAGE, _) => "VERA:ChatTranslationHelp",
            (PREVIEW, _) => "VERA:TranslatePreviewHelp",
            (_, Some(Checkbox::Place(place))) => PLACE_KEYS[place as usize].1,
            (_, Some(Checkbox::ShowSource)) => "VERA:TranslateShowSourceHelp",
            (_, Some(Checkbox::SameAlphabet)) if self.settings.language.is_some() => {
                if self.enabled(control) {
                    "VERA:TranslateSameAlphabetHelp"
                } else {
                    "VERA:TranslateNoSameAlphabetHelp"
                }
            }
            (_, Some(Checkbox::SameAlphabet)) => "VERA:TranslateSameAlphabetHelp",
            (_, Some(Checkbox::Reads(language))) if self.settings.language == Some(language) => {
                "VERA:TranslateOwnHelp"
            }
            (_, Some(Checkbox::Reads(_))) => "VERA:TranslateKnownHelp",
            _ => return None,
        })
    }

    pub(crate) fn popup_geometry(&self, layout: &WolChatTranslationLayout) -> Option<ComboPopup> {
        self.popup.control?;
        Some(ComboPopup::new(
            layout.language,
            layout.page.scale,
            Self::rows(),
            0,
            self.popup.top_index,
        ))
    }
}

/// One row of the Preview panel.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PreviewRow {
    /// The place's label, on a sample's first row.
    pub(crate) place_key: Option<&'static str>,
    pub(crate) text: String,
    /// Authored pixels below the panel's top.
    pub(crate) top: i32,
}

/// Lines from the live translator: target, place, source language, sender,
/// the line, and its translation with the translator's note.
const SAMPLES: &str = include_str!("chat_translation_samples.tsv");

const PREVIEW_FIRST_TOP: i32 = 7;
const PREVIEW_ROW_H: i32 = 21;
const PREVIEW_SAMPLE_GAP: i32 = 2;

fn place_named(name: &str) -> Option<ChatPlace> {
    Some(match name {
        "lobby" => ChatPlace::Lobby,
        "room" => ChatPlace::Room,
        "match" => ChatPlace::Match,
        "private" => ChatPlace::Private,
        _ => return None,
    })
}

/// How the place's real chat line starts: WOL's transcript format, or a match
/// message's `name:`.
fn chat_prefix(place: ChatPlace, sender: &str) -> String {
    if place == ChatPlace::Match {
        return format!("{sender}:");
    }
    let (prefix, _) = received_line(
        &WolSender {
            operator: false,
            name: sender,
            clan: "",
        },
        &[],
        place == ChatPlace::Private,
        false,
    );
    String::from_utf16_lossy(&prefix)
}

/// The Preview panel's rows: each sample as it was written, and under it what
/// the real chat path would add with these settings.
pub(crate) fn preview_rows(settings: &ChatTranslationSettings) -> Vec<PreviewRow> {
    let Some(target) = settings.language else {
        return Vec::new();
    };
    let mut rows = Vec::new();
    let mut top = PREVIEW_FIRST_TOP;
    for line in SAMPLES.lines() {
        let columns: Vec<&str> = line.split('\t').collect();
        let [tag, place, source, sender, text, translated] = columns[..] else {
            continue;
        };
        let Some(place) = place_named(place).filter(|_| tag == target.tag()) else {
            continue;
        };
        let prefix = chat_prefix(place, sender);
        rows.push(PreviewRow {
            place_key: Some(match place {
                ChatPlace::Lobby => "VERA:TranslatePlaceLobby",
                ChatPlace::Room => "VERA:TranslatePlaceRoom",
                ChatPlace::Match => "VERA:TranslatePlaceMatch",
                ChatPlace::Private => "VERA:TranslatePlacePrivate",
            }),
            text: format!("{prefix}{text}"),
            top,
        });
        top += PREVIEW_ROW_H;
        let units: Vec<u16> = text.encode_utf16().collect();
        let reply = ChatReply {
            text: translated.to_owned(),
            source: ChatLanguage::from_tag(source),
        };
        if settings.target_for(place, &units).is_some()
            && let Some(shown) = settings.shown(&reply)
        {
            rows.push(PreviewRow {
                place_key: None,
                text: format!("{prefix}{MARKER}{shown}"),
                top,
            });
            top += PREVIEW_ROW_H;
        }
        top += PREVIEW_SAMPLE_GAP;
    }
    rows
}

/// The page at the current screen, authored at 800x600.
#[derive(Debug, Clone)]
pub(crate) struct WolChatTranslationLayout {
    pub(crate) page: MenuPageLayout,
    pub(crate) status_help: RectPx,
    pub(crate) into_caption: RectPx,
    pub(crate) language: RectPx,
    pub(crate) places_caption: RectPx,
    /// [`ChatPlace::ALL`] order.
    pub(crate) places: [RectPx; 4],
    pub(crate) lines_caption: RectPx,
    pub(crate) show_source: RectPx,
    pub(crate) same_alphabet: RectPx,
    pub(crate) reads_caption: RectPx,
    /// [`ChatLanguage::all`] order.
    pub(crate) reads: [RectPx; 14],
    pub(crate) preview_caption: RectPx,
    pub(crate) preview: RectPx,
    pub(crate) privacy: RectPx,
}

const CAPTION_H: i32 = 17;
const CHECK_H: i32 = 19;
const LEFT_COLUMN: i32 = 134;
const CHECK_COLUMN: i32 = 141;
const CHECK_W: i32 = 204;
const READS_COLUMNS: [i32; 2] = [350, 478];
const READS_W: i32 = 122;
const READS_PER_COLUMN: usize = 7;
const PREVIEW_RECT: RectPx = RectPx::new(60, 320, 552, 188);

impl WolChatTranslationLayout {
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
        // The same one-row-taller heading as WOL Options' template.
        let mut title =
            menu_page::compute_layout(&PAGE, SHELL_BASE_W as u32, SHELL_BASE_H as u32).title;
        title.h = dlu_rect(424, 4, 108, 11).h + 1;
        if scale.is_retail() {
            page.title.h = title.h;
        } else {
            page.title = scale.scale_rect(title);
        }
        let rect = |x, y, w, h| scale.scale_rect(RectPx::new(x, y, w, h));
        let caption = |x, y, w| rect(x, y, w, CAPTION_H);
        let check = |y| rect(CHECK_COLUMN, y, CHECK_W, CHECK_H);
        Self {
            page,
            status_help,
            into_caption: caption(LEFT_COLUMN, 61, 200),
            language: rect(LEFT_COLUMN, 78, 180, 24),
            places_caption: caption(LEFT_COLUMN, 109, 200),
            places: [128, 152, 176, 200].map(check),
            lines_caption: caption(LEFT_COLUMN, 227, 200),
            show_source: check(246),
            same_alphabet: check(270),
            reads_caption: caption(READS_COLUMNS[0], 61, 262),
            reads: std::array::from_fn(|index| {
                rect(
                    READS_COLUMNS[index / READS_PER_COLUMN],
                    82 + (index % READS_PER_COLUMN) as i32 * 25,
                    READS_W,
                    CHECK_H,
                )
            }),
            preview_caption: caption(PREVIEW_RECT.x, 301, 300),
            preview: scale.scale_rect(PREVIEW_RECT),
            privacy: caption(PREVIEW_RECT.x, 515, PREVIEW_RECT.w),
        }
    }

    /// Every checkbox and its control, in draw order.
    pub(crate) fn checkboxes(&self) -> impl Iterator<Item = (u16, RectPx)> + '_ {
        let places = self
            .places
            .iter()
            .enumerate()
            .map(|(index, rect)| (place_control(index), *rect));
        let reads = self
            .reads
            .iter()
            .enumerate()
            .map(|(index, rect)| (reads_control(index), *rect));
        places
            .chain([
                (SHOW_SOURCE, self.show_source),
                (SAME_ALPHABET, self.same_alphabet),
            ])
            .chain(reads)
    }

    pub(crate) fn checkbox_rect(&self, control: u16) -> Option<RectPx> {
        self.checkboxes()
            .find(|(id, _)| *id == control)
            .map(|(_, rect)| rect)
    }

    /// The control under `(x, y)`.
    pub(crate) fn control_at(&self, x: i32, y: i32) -> Option<u16> {
        if let Some(button) = self.page.buttons.iter().find(|b| b.rect.contains(x, y)) {
            return Some(button.id);
        }
        if self.language.contains(x, y) {
            return Some(LANGUAGE);
        }
        if let Some((control, _)) = self.checkboxes().find(|(_, rect)| rect.contains(x, y)) {
            return Some(control);
        }
        self.preview.contains(x, y).then_some(PREVIEW)
    }

    /// A preview row's place tag and line, `top` authored pixels into the
    /// panel.
    pub(crate) fn preview_row(&self, top: i32) -> (RectPx, RectPx) {
        const TAG_LEFT: i32 = 12;
        const LINE_LEFT: i32 = 64;
        let scale = self.page.scale;
        let y = PREVIEW_RECT.y + top;
        (
            scale.scale_rect(RectPx::new(
                PREVIEW_RECT.x + TAG_LEFT,
                y,
                LINE_LEFT - TAG_LEFT - 4,
                CAPTION_H,
            )),
            scale.scale_rect(RectPx::new(
                PREVIEW_RECT.x + LINE_LEFT,
                y,
                PREVIEW_RECT.w - LINE_LEFT - TAG_LEFT,
                CAPTION_H,
            )),
        )
    }

    /// The panel's one line while translation is off.
    pub(crate) fn preview_off(&self) -> RectPx {
        const LEFT: i32 = 12;
        self.page.scale.scale_rect(RectPx::new(
            PREVIEW_RECT.x + LEFT,
            PREVIEW_RECT.y + PREVIEW_FIRST_TOP,
            PREVIEW_RECT.w - LEFT * 2,
            CAPTION_H,
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn settings_for(language: ChatLanguage) -> ChatTranslationSettings {
        let mut settings = ChatTranslationSettings::default();
        settings.language = Some(language);
        settings
    }

    fn with(language: ChatLanguage) -> WolChatTranslationPage {
        WolChatTranslationPage::open(settings_for(language))
    }

    #[test]
    fn off_leaves_only_the_list_and_the_buttons_working() {
        let mut page = WolChatTranslationPage::open(ChatTranslationSettings::default());
        assert!(page.enabled(LANGUAGE) && page.enabled(OK) && page.enabled(CANCEL));
        assert!(!page.enabled(place_control(0)) && !page.enabled(SHOW_SOURCE));
        assert!(!page.toggle(place_control(0)));
        assert!(page.settings.translates(ChatPlace::Lobby));
        page.select_row(1);
        assert_eq!(page.settings.language, Some(ChatLanguage::English));
        assert!(page.toggle(place_control(0)));
        assert!(!page.settings.translates(ChatPlace::Lobby));
        page.select_row(0);
        assert_eq!(page.settings.language, None);
    }

    #[test]
    fn the_reader_s_own_language_is_checked_and_locked() {
        let mut page = with(ChatLanguage::Japanese);
        let japanese = reads_control(3);
        assert!(page.is_checked(japanese) && !page.enabled(japanese));
        assert!(!page.toggle(japanese));
        assert!(!page.settings.reads(ChatLanguage::Japanese));
        assert_eq!(page.tooltip_key(japanese), Some("VERA:TranslateOwnHelp"));
        let korean = reads_control(4);
        assert!(!page.is_checked(korean));
        assert!(page.toggle(korean) && page.is_checked(korean));
    }

    #[test]
    fn same_alphabet_is_locked_for_scripts_no_other_language_shares() {
        let mut page = with(ChatLanguage::Russian);
        page.settings.same_alphabet = true;
        assert!(!page.enabled(SAME_ALPHABET) && !page.is_checked(SAME_ALPHABET));
        assert_eq!(
            page.tooltip_key(SAME_ALPHABET),
            Some("VERA:TranslateNoSameAlphabetHelp")
        );
        assert!(page.settings.same_alphabet, "a later language still has it");
        page.select_row(1);
        assert!(page.enabled(SAME_ALPHABET) && page.is_checked(SAME_ALPHABET));
    }

    #[test]
    fn ok_hands_back_the_changes_and_cancel_nothing() {
        let mut page = with(ChatLanguage::English);
        page.toggle(SHOW_SOURCE);
        let Some(ChatTranslationAction::Ok(staged)) = page.activate(OK) else {
            panic!("OK commits");
        };
        assert!(!staged.show_source);
        assert_eq!(page.activate(CANCEL), Some(ChatTranslationAction::Cancel));
    }

    #[test]
    fn every_language_has_four_samples_and_the_preview_follows_the_settings() {
        for language in ChatLanguage::all() {
            let originals = preview_rows(&settings_for(language))
                .into_iter()
                .filter(|row| row.place_key.is_some())
                .count();
            assert_eq!(originals, 4, "{}", language.tag());
        }
        let mut page = with(ChatLanguage::SimplifiedChinese);
        let translated = |page: &WolChatTranslationPage| -> Vec<String> {
            preview_rows(&page.settings)
                .into_iter()
                .filter(|row| row.place_key.is_none())
                .map(|row| row.text)
                .collect()
        };
        // The Korean room line waits for Same-Alphabet Languages.
        assert_eq!(translated(&page).len(), 3);
        page.toggle(SAME_ALPHABET);
        assert_eq!(translated(&page).len(), 4);
        assert!(translated(&page)[0].starts_with("[Niran] : » [从泰语翻译] "));
        page.toggle(SHOW_SOURCE);
        assert!(translated(&page)[0].starts_with("[Niran] : » 直接"));
        page.toggle(reads_control(11));
        assert_eq!(translated(&page).len(), 3, "Thai is read");
        page.toggle(place_control(3));
        assert_eq!(translated(&page).len(), 2, "private lines are left alone");
        assert_eq!(preview_rows(&ChatTranslationSettings::default()), []);
    }

    #[test]
    fn layout_scales_with_the_window_and_finds_each_control() {
        let layout = WolChatTranslationLayout::responsive(1600, 1200);
        let center = |rect: RectPx| (rect.x + rect.w / 2, rect.y + rect.h / 2);
        for (control, rect) in layout.checkboxes() {
            let (x, y) = center(rect);
            assert_eq!(layout.control_at(x, y), Some(control));
        }
        let (x, y) = center(layout.language);
        assert_eq!(layout.control_at(x, y), Some(LANGUAGE));
        let retail = WolChatTranslationLayout::new(800, 600);
        assert_eq!(layout.language.w, retail.language.w * 2);
    }
}
