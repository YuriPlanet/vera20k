//! VERA's opt-in chat translation; gamemd has none. A player picks the
//! language received chat is translated into on the Options Interface page or
//! with `/translate` in a WOL chat edit, saved as `[VERA] TranslateChat=`.
//! Lines that need it go to the lobby's `POST /v1/translate`
//! (`docs/lobby-protocol.md`) and the reply is shown under the original, which
//! always stays visible.

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

/// The chat notice for a `/translate` with an unknown language.
pub(crate) fn usage() -> String {
    crate::i18n::tr("VERA:TranslateUsage")
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

    /// A stored or typed tag, ASCII case-insensitively; `zh`/`zh-cn` and
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
    pub(crate) fn needs(self, text: &[u16]) -> bool {
        let (own, other) = char::decode_utf16(text.iter().copied())
            .filter_map(Result::ok)
            .filter(|ch| ch.is_alphabetic())
            .fold((false, false), |(own, other), ch| {
                let ours = self.writes(ch);
                (own || ours, other || !ours)
            });
        other && (self.writes('a') || !own)
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
                crate::render::text_engine::fonts::is_cjk_sized(code)
            }
            Self::Russian => matches!(code, 0x0400..=0x052F),
            Self::Arabic => crate::i18n::is_right_to_left_letter(ch),
            Self::Thai => matches!(code, 0x0E00..=0x0E7F),
        }
    }
}

/// What a `/translate` chat line asks for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TranslateCommand {
    Show,
    Set(Option<ChatLanguage>),
    Usage,
}

/// `/translate [language|off]`, ASCII case-insensitively; any other line is
/// ordinary chat.
pub(crate) fn parse_command(line: &[u16]) -> Option<TranslateCommand> {
    let line = String::from_utf16_lossy(line);
    let rest = line
        .get(..10)
        .filter(|word| word.eq_ignore_ascii_case("/translate"))
        .map(|_| &line[10..])?;
    if !rest.is_empty() && !rest.starts_with(' ') {
        return None;
    }
    Some(match rest.trim_matches(' ') {
        "" => TranslateCommand::Show,
        word if word.eq_ignore_ascii_case("off") => TranslateCommand::Set(None),
        word => ChatLanguage::from_tag(word).map_or(TranslateCommand::Usage, |language| {
            TranslateCommand::Set(Some(language))
        }),
    })
}

/// The chat notice for the current choice.
pub(crate) fn notice(language: Option<ChatLanguage>) -> String {
    match language {
        Some(language) => {
            crate::i18n::tr_fill("VERA:TranslatingChat", &[("language", &language.name())])
        }
        None => crate::i18n::tr("VERA:ChatTranslationOff"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn w(text: &str) -> Vec<u16> {
        text.encode_utf16().collect()
    }

    #[test]
    fn readers_translate_only_lines_in_the_other_script() {
        use ChatLanguage::*;
        assert!(SimplifiedChinese.needs(&w("rush his ore miners")));
        assert!(!SimplifiedChinese.needs(&w("冲他矿车 gg")));
        assert!(!SimplifiedChinese.needs(&w("100% :)")));
        assert!(English.needs(&w("冲他矿车")));
        assert!(!English.needs(&w("Café olé")));
        assert!(!English.needs(&w("→ 123")));
    }

    #[test]
    fn non_latin_readers_get_lines_without_a_letter_of_their_script() {
        use ChatLanguage::*;
        assert!(Russian.needs(&w("rush his ore miners")));
        assert!(Russian.needs(&w("冲他矿车")));
        assert!(!Russian.needs(&w("раш на руду, GI рядом")));
        assert!(Arabic.needs(&w("rush")));
        assert!(!Arabic.needs(&w("هجوم على القاعدة gg")));
        assert!(Thai.needs(&w("rush")));
        assert!(!Thai.needs(&w("บุกฐานเลย gg")));
        assert!(!Thai.needs(&w("100% :)")));
        assert!(SimplifiedChinese.needs(&w("раш на руду")));
    }

    #[test]
    fn turkish_letters_past_latin_1_are_foreign_to_everyone_but_turkish_readers() {
        use ChatLanguage::*;
        let turkish = "şimdi üssüne saldır";
        assert!(!Turkish.needs(&w(turkish)));
        assert!(English.needs(&w(turkish)));
        assert!(Turkish.needs(&w("冲他矿车")));
        assert!(!Swedish.needs(&w("anfall basen på östra sidan")));
    }

    #[test]
    fn translate_command_picks_saves_and_stops_a_language() {
        let parse = |line: &str| parse_command(&w(line));
        assert_eq!(parse("/translate"), Some(TranslateCommand::Show));
        assert_eq!(
            parse("/Translate ZH"),
            Some(TranslateCommand::Set(Some(ChatLanguage::SimplifiedChinese)))
        );
        assert_eq!(
            parse("/translate zh-tw"),
            Some(TranslateCommand::Set(Some(
                ChatLanguage::TraditionalChinese
            )))
        );
        assert_eq!(
            parse("/translate ru"),
            Some(TranslateCommand::Set(Some(ChatLanguage::Russian)))
        );
        assert_eq!(parse("/translate off"), Some(TranslateCommand::Set(None)));
        assert_eq!(parse("/translate klingon"), Some(TranslateCommand::Usage));
        assert_eq!(parse("/translated it"), None);
        assert_eq!(parse("翻译 /translate"), None);
        assert_eq!(
            ChatLanguage::from_tag(ChatLanguage::TraditionalChinese.tag()),
            Some(ChatLanguage::TraditionalChinese)
        );
    }
}
