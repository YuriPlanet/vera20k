//! VERA's opt-in chat translation; gamemd has none. The VERA Online lobby's
//! Options page ([`crate::ui::wol_options`]) picks the language other players'
//! chat is translated into and whether a match's chat is translated too
//! ([`ChatTranslation`]). Lines that need it go to the lobby's
//! `POST /v1/translate` (`docs/lobby-protocol.md`) and the reply, without its
//! note naming the source language, is shown under the original, which always
//! stays visible.

/// A language received chat can be translated into. The lobby's translator
/// must offer each tag: an unsupported target gets no reply.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ChatLanguage {
    English,
    SimplifiedChinese,
    TraditionalChinese,
    Japanese,
    Korean,
    German,
    French,
    Spanish,
    Portuguese,
    Russian,
    Arabic,
    Thai,
    Turkish,
    Swedish,
}

/// Each language's lobby tag and its own name.
const LANGUAGES: [(ChatLanguage, &str, &str); 14] = [
    (ChatLanguage::English, "en", "English"),
    (ChatLanguage::SimplifiedChinese, "zh-Hans", "简体中文"),
    (ChatLanguage::TraditionalChinese, "zh-Hant", "繁體中文"),
    (ChatLanguage::Japanese, "ja", "日本語"),
    (ChatLanguage::Korean, "ko", "한국어"),
    (ChatLanguage::German, "de", "Deutsch"),
    (ChatLanguage::French, "fr", "Français"),
    (ChatLanguage::Spanish, "es", "Español"),
    (ChatLanguage::Portuguese, "pt", "Português"),
    (ChatLanguage::Russian, "ru", "Русский"),
    (ChatLanguage::Arabic, "ar", "العربية"),
    (ChatLanguage::Thai, "th", "ไทย"),
    (ChatLanguage::Turkish, "tr", "Türkçe"),
    (ChatLanguage::Swedish, "sv", "Svenska"),
];

/// The online lobby Options page's chat translation settings.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ChatTranslation {
    /// `[VERA] TranslateChat=`; `None` is off.
    pub(crate) language: Option<ChatLanguage>,
    /// `[VERA] TranslateMatches=`: a match's chat is translated too. Off
    /// leaves battle chat alone; the lobby, rooms and private messages still
    /// translate.
    pub(crate) in_matches: bool,
}

impl Default for ChatTranslation {
    fn default() -> Self {
        Self {
            language: None,
            in_matches: true,
        }
    }
}

impl ChatTranslation {
    /// Whether a received line goes to the translator; `in_match` says it
    /// was said in a match.
    pub(crate) fn sends(self, text: &[u16], in_match: bool) -> bool {
        (self.in_matches || !in_match) && self.language.is_some_and(|language| language.needs(text))
    }
}

/// Set between a line's prefix and its translation.
pub(crate) const MARKER: &str = "» ";

/// A note naming the source language is a few words; anything longer is not
/// one. The translator holds its notes to the same limit.
const MAX_NOTE_UNITS: usize = 40;

/// What the translator returned for one line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ChatReply {
    pub(crate) text: String,
    /// The language the model says the line was written in; a service that
    /// doesn't say, or a language not on the list, leaves it out.
    pub(crate) source: Option<ChatLanguage>,
}

/// A translation without the bracketed note the translator starts it with.
pub(crate) fn without_note(text: &str) -> &str {
    text.strip_prefix('[')
        .and_then(|rest| rest.split_once("] "))
        .filter(|(note, _)| note.encode_utf16().count() <= MAX_NOTE_UNITS)
        .map_or(text, |(_, translation)| translation)
}

/// The language a line's script alone gives away: the first letter only one
/// listed language writes. Latin and Han letters are shared, so they say
/// nothing.
fn distinct_language(text: &[u16]) -> Option<ChatLanguage> {
    char::decode_utf16(text.iter().copied())
        .filter_map(Result::ok)
        .filter(|ch| ch.is_alphabetic())
        .find_map(|ch| match u32::from(ch) {
            0x3040..=0x30FF | 0x31F0..=0x31FF | 0xFF66..=0xFF9D => Some(ChatLanguage::Japanese),
            0x1100..=0x11FF | 0x3130..=0x318F | 0xA960..=0xA97F | 0xAC00..=0xD7FF => {
                Some(ChatLanguage::Korean)
            }
            0x0400..=0x052F => Some(ChatLanguage::Russian),
            0x0E00..=0x0E7F => Some(ChatLanguage::Thai),
            0x0600..=0x06FF | 0x0750..=0x077F | 0xFB50..=0xFDFF | 0xFE70..=0xFEFF => {
                Some(ChatLanguage::Arabic)
            }
            _ => None,
        })
}

/// Whether a reply only repeats the line it translates: the model leaves `gg`
/// and `2v2` as they are, and `» gg` under `gg` tells the reader nothing. Case,
/// spaces and punctuation don't count.
pub(crate) fn only_repeats(original: &str, translated: &str) -> bool {
    fn letters(text: &str) -> impl Iterator<Item = char> {
        text.chars()
            .filter(|ch| ch.is_alphanumeric())
            .flat_map(char::to_lowercase)
    }
    letters(original).eq(letters(translated))
}

impl ChatLanguage {
    /// Every language, in the order the Options list shows them.
    pub(crate) fn all() -> impl Iterator<Item = Self> {
        LANGUAGES.into_iter().map(|(language, ..)| language)
    }

    fn entry(self) -> (ChatLanguage, &'static str, &'static str) {
        LANGUAGES
            .into_iter()
            .find(|(language, ..)| *language == self)
            .expect("every language is listed")
    }

    /// The tag `POST /v1/translate` and `[VERA] TranslateChat=` use.
    pub(crate) fn tag(self) -> &'static str {
        self.entry().1
    }

    pub(crate) fn name(self) -> &'static str {
        self.entry().2
    }

    /// A stored tag, ASCII case-insensitively; `zh`/`zh-cn` and
    /// `zh-tw`/`zh-hk` name the two Chinese scripts.
    pub(crate) fn from_tag(text: &str) -> Option<Self> {
        let text = text.to_ascii_lowercase();
        match text.as_str() {
            "zh" | "zh-cn" => return Some(Self::SimplifiedChinese),
            "zh-tw" | "zh-hk" => return Some(Self::TraditionalChinese),
            _ => {}
        }
        LANGUAGES
            .into_iter()
            .find(|(_, tag, _)| tag.eq_ignore_ascii_case(&text))
            .map(|(language, ..)| language)
    }

    /// Whether a received line needs translating into this language. Latin-1
    /// is the WOL language gate's boundary (`0x007B7D90`: every unit below
    /// 0x100 is Latin). A Latin-script reader gets a line with any letter they
    /// can't read. Any other reader gets a line with letters, none of them
    /// their own script's: a Russian line naming a `GI` is still Russian.
    /// A reader whose script another listed language shares also gets the
    /// lines in it that may be that other language; the reply says if one
    /// wasn't ([`Self::shows`]). English readers don't: nearly every Latin line
    /// would cost a model call only to be hidden.
    pub(crate) fn needs(self, text: &[u16]) -> bool {
        let (own, other) = char::decode_utf16(text.iter().copied())
            .filter_map(Result::ok)
            .filter(|ch| ch.is_alphabetic())
            .fold((false, false), |(own, other), ch| {
                let ours = self.writes(ch);
                (own || ours, other || !ours)
            });
        if other && (self.writes('a') || !own) {
            return true;
        }
        own && match self {
            Self::English | Self::Russian | Self::Arabic | Self::Thai => false,
            // Han letters alone read as the reader's own Chinese.
            Self::SimplifiedChinese | Self::TraditionalChinese => {
                matches!(distinct_language(text), Some(Self::Japanese | Self::Korean))
            }
            Self::Japanese | Self::Korean => distinct_language(text) != Some(self),
            _ => true,
        }
    }

    /// Whether a reply reaches the chat: not when the translator says the
    /// line was already in this language. A service that doesn't say still
    /// translates.
    pub(crate) fn shows(self, reply: &ChatReply) -> bool {
        reply.source != Some(self)
    }

    /// Whether `ch` is a letter of this language's script. Turkish alone of
    /// the Latin-script languages here has letters past Latin-1.
    fn writes(self, ch: char) -> bool {
        let code = u32::from(ch);
        match self {
            Self::English
            | Self::German
            | Self::French
            | Self::Spanish
            | Self::Portuguese
            | Self::Swedish => code < 0x100,
            Self::Turkish => code < 0x100 || matches!(ch, 'Ğ' | 'ğ' | 'İ' | 'ı' | 'Ş' | 'ş'),
            Self::SimplifiedChinese | Self::TraditionalChinese | Self::Japanese | Self::Korean => {
                crate::i18n::is_cjk_sized(code)
            }
            Self::Russian => matches!(code, 0x0400..=0x052F),
            Self::Arabic => crate::i18n::is_right_to_left_letter(ch),
            Self::Thai => matches!(code, 0x0E00..=0x0E7F),
        }
    }
}

#[cfg(all(test, tests_ui))]
mod tests {
    use super::*;

    fn w(text: &str) -> Vec<u16> {
        text.encode_utf16().collect()
    }

    #[test]
    fn readers_get_lines_in_another_script_or_that_may_be_another_language() {
        use ChatLanguage::*;
        assert!(SimplifiedChinese.needs(&w("rush his ore miners")));
        assert!(!SimplifiedChinese.needs(&w("冲他矿车 gg")));
        assert!(SimplifiedChinese.needs(&w("방어 없어요")));
        assert!(!SimplifiedChinese.needs(&w("100% :)")));
        assert!(Japanese.needs(&w("他没防守")));
        assert!(!Japanese.needs(&w("防御がない")));
        assert!(English.needs(&w("冲他矿车")));
        assert!(English.needs(&w("şimdi üssüne saldır")));
        // English readers send no Latin-1 line: it would mostly come back hidden.
        assert!(!English.needs(&w("ataca sus mineros")));
        assert!(German.needs(&w("ataca sus mineros")));
        assert!(Russian.needs(&w("rush his ore miners")));
        assert!(!Russian.needs(&w("раш на руду, GI рядом")));
        assert!(!Thai.needs(&w("บุกฐานเลย gg")));
    }

    #[test]
    fn match_chat_is_sent_only_while_translate_in_matches_is_on() {
        let line = w("rush his ore miners");
        let mut chat = ChatTranslation {
            language: Some(ChatLanguage::German),
            ..ChatTranslation::default()
        };
        assert!(chat.sends(&line, true));
        chat.in_matches = false;
        assert!(!chat.sends(&line, true));
        assert!(chat.sends(&line, false));
        assert!(!ChatTranslation::default().sends(&line, false));
    }

    #[test]
    fn the_note_naming_the_source_language_is_dropped() {
        assert_eq!(without_note("[从泰语翻译] 冲他矿车"), "冲他矿车");
        assert_eq!(without_note("冲他矿车"), "冲他矿车");
        assert_eq!(without_note("[gg"), "[gg");
        let long = format!("[{}] 冲", "x".repeat(41));
        assert_eq!(without_note(&long), long);
    }

    #[test]
    fn a_reply_in_the_reader_s_own_language_is_hidden() {
        let reply = |source| ChatReply {
            text: "[Übersetzt aus dem Englischen] Angriff".into(),
            source,
        };
        let german = ChatLanguage::German;
        assert!(!german.shows(&reply(Some(german))));
        assert!(german.shows(&reply(Some(ChatLanguage::English))));
        assert!(german.shows(&reply(None)));
    }
}
