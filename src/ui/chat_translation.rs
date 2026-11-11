//! VERA's opt-in chat translation; gamemd has none. A player chooses the
//! language received chat is translated into, where, and what they can already
//! read on the VERA Online lobby's Chat Translation page
//! ([`crate::ui::wol_chat_translation`]), or just the language with
//! `/translate` in a WOL chat edit, all saved as `[VERA] Translate*=`. Lines
//! that need it go to the lobby's `POST /v1/translate`
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

/// Set between a line's prefix and its translation.
pub(crate) const MARKER: &str = "» ";

/// A note naming the source language is a few words; anything longer is not
/// one. The translator holds its notes to the same limit.
const MAX_NOTE_UNITS: usize = 40;

/// Where a received line was said.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ChatPlace {
    Lobby,
    Room,
    Match,
    Private,
}

impl ChatPlace {
    pub(crate) const ALL: [Self; 4] = [Self::Lobby, Self::Room, Self::Match, Self::Private];

    /// Its `[VERA]` switch.
    pub(crate) fn ini_key(self) -> &'static str {
        match self {
            Self::Lobby => "TranslateLobby",
            Self::Room => "TranslateRooms",
            Self::Match => "TranslateMatches",
            Self::Private => "TranslatePrivate",
        }
    }
}

/// What the translator returned for one line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ChatReply {
    pub(crate) text: String,
    /// The language the model says the line was written in; a service that
    /// doesn't say, or a language not on the list, leaves it out.
    pub(crate) source: Option<ChatLanguage>,
}

/// The player's chat translation choices, held by the options profile.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ChatTranslationSettings {
    /// What received chat is translated into; `None` is off.
    pub(crate) language: Option<ChatLanguage>,
    /// [`ChatPlace::ALL`] order.
    places: [bool; 4],
    /// Keep the `[Translated from Thai]` note a translation starts with.
    pub(crate) show_source: bool,
    /// Also translate other languages written in the reader's own alphabet.
    /// Every such line costs a model call; [`Self::set_language`] sets where
    /// it starts.
    pub(crate) same_alphabet: bool,
    /// One bit per [`LANGUAGES`] row.
    readable: u16,
}

impl Default for ChatTranslationSettings {
    fn default() -> Self {
        Self {
            language: None,
            places: [true; 4],
            show_source: true,
            same_alphabet: false,
            readable: 0,
        }
    }
}

impl ChatTranslationSettings {
    /// Choose the language received chat is translated into. A newly chosen
    /// language starts with [`Self::same_alphabet`] on, as its readers mostly
    /// want English chat translated. English starts with it off: nearly every
    /// Latin line would cost a model call only to be hidden.
    pub(crate) fn set_language(&mut self, language: Option<ChatLanguage>) {
        if self.language != language {
            self.language = language;
            self.same_alphabet = language.is_some_and(|language| language != ChatLanguage::English);
        }
    }

    pub(crate) fn translates(&self, place: ChatPlace) -> bool {
        self.places[place as usize]
    }

    pub(crate) fn set_translates(&mut self, place: ChatPlace, on: bool) {
        self.places[place as usize] = on;
    }

    /// Whether the player said they can read `language`.
    pub(crate) fn reads(&self, language: ChatLanguage) -> bool {
        self.readable & language.bit() != 0
    }

    pub(crate) fn set_reads(&mut self, language: ChatLanguage, reads: bool) {
        if reads {
            self.readable |= language.bit();
        } else {
            self.readable &= !language.bit();
        }
    }

    /// `[VERA] TranslateKnown=`: the readable languages' tags, comma-separated.
    pub(crate) fn readable_tags(&self) -> String {
        ChatLanguage::all()
            .filter(|language| self.reads(*language))
            .map(ChatLanguage::tag)
            .collect::<Vec<_>>()
            .join(",")
    }

    pub(crate) fn set_readable_tags(&mut self, tags: &str) {
        self.readable = tags
            .split(',')
            .filter_map(|tag| ChatLanguage::from_tag(tag.trim()))
            .fold(0, |bits, language| bits | language.bit());
    }

    /// The language a line received at `place` should be translated into. A
    /// script only one listed language uses needs no model call to tell it's
    /// readable; other readable lines come back named by [`Self::shown`].
    pub(crate) fn target_for(&self, place: ChatPlace, text: &[u16]) -> Option<ChatLanguage> {
        let target = self.language?;
        (self.translates(place)
            && !distinct_language(text).is_some_and(|language| self.reads(language))
            && target.needs(text, self.same_alphabet))
        .then_some(target)
    }

    /// What of a reply reaches the chat: nothing when the line turned out to
    /// be in a language the player reads, otherwise its text, with or without
    /// the leading note.
    pub(crate) fn shown<'a>(&self, reply: &'a ChatReply) -> Option<&'a str> {
        if reply
            .source
            .is_some_and(|source| self.language == Some(source) || self.reads(source))
        {
            return None;
        }
        Some(if self.show_source {
            &reply.text
        } else {
            without_note(&reply.text)
        })
    }
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
    /// Every language, in the order the Chat Translation page shows them.
    pub(crate) fn all() -> impl Iterator<Item = Self> {
        LANGUAGES.into_iter().map(|(language, ..)| language)
    }

    fn entry(self) -> (ChatLanguage, &'static str, &'static str) {
        LANGUAGES
            .into_iter()
            .find(|(language, ..)| *language == self)
            .expect("every language is listed")
    }

    fn bit(self) -> u16 {
        let row = LANGUAGES
            .iter()
            .position(|(language, ..)| *language == self)
            .expect("every language is listed");
        1 << row
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
    /// `same_alphabet` adds the lines written in the reader's own script that
    /// may be another language; the reply says if one wasn't.
    pub(crate) fn needs(self, text: &[u16], same_alphabet: bool) -> bool {
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
        same_alphabet
            && own
            && self.shares_alphabet()
            && match self {
                // Han letters alone read as the reader's own Chinese.
                Self::SimplifiedChinese | Self::TraditionalChinese => {
                    matches!(distinct_language(text), Some(Self::Japanese | Self::Korean))
                }
                Self::Japanese | Self::Korean => distinct_language(text) != Some(self),
                _ => true,
            }
    }

    /// Whether another language on the list is written in this one's script.
    pub(crate) fn shares_alphabet(self) -> bool {
        !matches!(self, Self::Russian | Self::Arabic | Self::Thai)
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

    fn settings(language: ChatLanguage) -> ChatTranslationSettings {
        ChatTranslationSettings {
            language: Some(language),
            ..ChatTranslationSettings::default()
        }
    }

    #[test]
    fn each_place_can_be_switched_off_and_off_translates_nothing() {
        let mut chinese = settings(ChatLanguage::SimplifiedChinese);
        let line = w("rush his ore miners");
        for place in ChatPlace::ALL {
            assert_eq!(
                chinese.target_for(place, &line),
                Some(ChatLanguage::SimplifiedChinese)
            );
        }
        chinese.set_translates(ChatPlace::Match, false);
        assert_eq!(chinese.target_for(ChatPlace::Match, &line), None);
        assert!(chinese.target_for(ChatPlace::Lobby, &line).is_some());
        assert_eq!(
            ChatTranslationSettings::default().target_for(ChatPlace::Lobby, &line),
            None
        );
    }

    #[test]
    fn same_alphabet_adds_only_lines_that_may_be_another_language() {
        use ChatLanguage::*;
        let mut english = settings(English);
        assert_eq!(
            english.target_for(ChatPlace::Lobby, &w("ataca sus mineros")),
            None
        );
        english.same_alphabet = true;
        assert!(
            english
                .target_for(ChatPlace::Lobby, &w("ataca sus mineros"))
                .is_some()
        );
        assert!(
            !English.needs(&w("100 %"), true),
            "no letters, nothing to translate"
        );

        assert!(SimplifiedChinese.needs(&w("방어 없어요"), true));
        assert!(SimplifiedChinese.needs(&w("防御がない"), true));
        assert!(!SimplifiedChinese.needs(&w("他没防守"), true));
        assert!(Japanese.needs(&w("他没防守"), true));
        assert!(!Japanese.needs(&w("防御がない"), true));
        assert!(Korean.needs(&w("他没防守"), true));
        assert!(!Korean.needs(&w("방어 없어요"), true));
        // No other language on the list writes these scripts.
        assert!(!Russian.needs(&w("кто хочет сыграть"), true));
        assert!(!Thai.needs(&w("บุกฐานเลย"), true));
    }

    #[test]
    fn readable_languages_are_skipped_by_script_and_hidden_by_the_reply() {
        use ChatLanguage::*;
        let mut chinese = settings(SimplifiedChinese);
        chinese.set_reads(Russian, true);
        chinese.set_reads(Spanish, true);
        assert_eq!(chinese.readable_tags(), "es,ru");
        // Cyrillic needs no model call. Spanish looks like any Latin line.
        assert_eq!(chinese.target_for(ChatPlace::Lobby, &w("кто хочет")), None);
        assert!(chinese.target_for(ChatPlace::Lobby, &w("ataca")).is_some());

        let reply = |source| ChatReply {
            text: "[从西班牙语翻译] 攻击".into(),
            source,
        };
        assert_eq!(chinese.shown(&reply(Some(Spanish))), None);
        assert_eq!(chinese.shown(&reply(Some(SimplifiedChinese))), None);
        assert_eq!(
            chinese.shown(&reply(Some(English))),
            Some("[从西班牙语翻译] 攻击")
        );
        assert_eq!(
            chinese.shown(&reply(None)),
            Some("[从西班牙语翻译] 攻击"),
            "a service that doesn't name the language still translates"
        );

        let mut reread = ChatTranslationSettings::default();
        reread.set_readable_tags(&chinese.readable_tags());
        assert!(reread.reads(Spanish) && reread.reads(Russian) && !reread.reads(Thai));
        reread.set_readable_tags("");
        assert!(!reread.reads(Spanish));
    }

    #[test]
    fn show_source_off_drops_only_the_leading_note() {
        let mut chinese = settings(ChatLanguage::SimplifiedChinese);
        let reply = |text: &str| ChatReply {
            text: text.into(),
            source: None,
        };
        let noted = reply("[从泰语翻译] 冲他矿车");
        assert_eq!(chinese.shown(&noted), Some("[从泰语翻译] 冲他矿车"));
        chinese.show_source = false;
        assert_eq!(chinese.shown(&noted), Some("冲他矿车"));
        assert_eq!(chinese.shown(&reply("冲他矿车")), Some("冲他矿车"));
        assert_eq!(chinese.shown(&reply("[gg")), Some("[gg"));
        let long = format!("[{}] 冲", "x".repeat(41));
        assert_eq!(chinese.shown(&reply(&long)), Some(long.as_str()));
    }

    #[test]
    fn readers_translate_only_lines_in_the_other_script() {
        use ChatLanguage::*;
        assert!(SimplifiedChinese.needs(&w("rush his ore miners"), false));
        assert!(!SimplifiedChinese.needs(&w("冲他矿车 gg"), false));
        assert!(!SimplifiedChinese.needs(&w("100% :)"), false));
        assert!(English.needs(&w("冲他矿车"), false));
        assert!(!English.needs(&w("Café olé"), false));
        assert!(!English.needs(&w("→ 123"), false));
    }

    #[test]
    fn non_latin_readers_get_lines_without_a_letter_of_their_script() {
        use ChatLanguage::*;
        assert!(Russian.needs(&w("rush his ore miners"), false));
        assert!(Russian.needs(&w("冲他矿车"), false));
        assert!(!Russian.needs(&w("раш на руду, GI рядом"), false));
        assert!(Arabic.needs(&w("rush"), false));
        assert!(!Arabic.needs(&w("هجوم على القاعدة gg"), false));
        assert!(Thai.needs(&w("rush"), false));
        assert!(!Thai.needs(&w("บุกฐานเลย gg"), false));
        assert!(!Thai.needs(&w("100% :)"), false));
        assert!(SimplifiedChinese.needs(&w("раш на руду"), false));
    }

    #[test]
    fn turkish_letters_past_latin_1_are_foreign_to_everyone_but_turkish_readers() {
        use ChatLanguage::*;
        let turkish = "şimdi üssüne saldır";
        assert!(!Turkish.needs(&w(turkish), false));
        assert!(English.needs(&w(turkish), false));
        assert!(Turkish.needs(&w("冲他矿车"), false));
        assert!(!Swedish.needs(&w("anfall basen på östra sidan"), false));
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
