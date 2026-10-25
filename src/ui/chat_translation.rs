//! VERA's opt-in chat translation; gamemd has none. A player picks the
//! language received chat is translated into with `/translate` in a WOL chat
//! edit, saved as `[VERA] TranslateChat=`. Lines that need it go to the lobby's
//! `POST /v1/translate` (`services/lobby/protocol.md`) and the reply is shown
//! under the original, which always stays visible.

/// A language received chat can be translated into. GAME.FNT draws Latin-1,
/// CJK, kana and Hangul, so only these are offered.
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
}

/// Each language's lobby tag and its own name.
const LANGUAGES: [(ChatLanguage, &str, &str); 9] = [
    (ChatLanguage::English, "en", "English"),
    (ChatLanguage::SimplifiedChinese, "zh-Hans", "简体中文"),
    (ChatLanguage::TraditionalChinese, "zh-Hant", "繁體中文"),
    (ChatLanguage::Japanese, "ja", "日本語"),
    (ChatLanguage::Korean, "ko", "한국어"),
    (ChatLanguage::German, "de", "Deutsch"),
    (ChatLanguage::French, "fr", "Français"),
    (ChatLanguage::Spanish, "es", "Español"),
    (ChatLanguage::Portuguese, "pt", "Português"),
];

pub(crate) const USAGE: &str = "Use /translate en, zh, zh-tw, ja, ko, de, fr, es, pt or off.";

impl ChatLanguage {
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

    /// Whether a received line needs translating into this language. The
    /// script boundary is the WOL language gate's (`0x007B7D90`: every unit
    /// below 0x100 is Latin): a Latin-script reader gets lines with a letter
    /// past Latin-1, a CJK reader all-Latin-1 lines with a letter.
    pub(crate) fn needs(self, text: &[u16]) -> bool {
        let latin_reader = matches!(
            self,
            Self::English | Self::German | Self::French | Self::Spanish | Self::Portuguese
        );
        let mut letters = char::decode_utf16(text.iter().copied())
            .filter_map(Result::ok)
            .filter(|ch| ch.is_alphabetic());
        if latin_reader {
            letters.any(|ch| u32::from(ch) >= 0x100)
        } else {
            text.iter().all(|unit| *unit < 0x100) && letters.next().is_some()
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
        Some(language) => format!(
            "Translating received chat into {}. Lines are sent to the VERA20k translation service; /translate off stops it.",
            language.name()
        ),
        None => {
            "Chat translation is off. Type /translate zh or /translate en to turn it on.".to_owned()
        }
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
