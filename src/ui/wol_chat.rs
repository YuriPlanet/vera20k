//! Westwood Online chat-line commands, shared by every WOL chat edit: the
//! custom lobby `0x113` ([`parse_lobby`]) and the setup dialogs C2/C9.
//!
//! Source: the C2 setup Enter handler (`0078F7F0`, control 0x540 notify
//! 0x501) runs `0078A6B0` (page), `0078A8F0` (reply) and `0078A470`
//! (locate) in that order before the plain send `0077E1D0`; the emote button
//! 0x3ED sends through `0077E300`. Command words are CSF strings. Words are
//! separated by the one-character string `L" "` at `0082083C`.
//!
//! The in-game observer branch of page/reply (`TXT_OBSERVER_CANT_PAGE`,
//! gated by the byte VERA's Ghidra names `g_MuteSWLaunches`) only runs during
//! a match, not in the shell.

/// The localized command words the parser compares against.
#[derive(Debug, Clone, Default)]
pub(crate) struct WolChatCommands {
    /// `TXT_PAGE_COMMAND`, compared case-insensitively (`007DDE80`).
    pub(crate) page: Vec<u16>,
    /// `TXT_REPLY_COMMAND`, compared case-insensitively to accept the line.
    pub(crate) reply: Vec<u16>,
    /// `TXT_REPLY_COMMAND_SHORT`, compared case-sensitively (`007CB4CC`).
    pub(crate) reply_short: Vec<u16>,
    /// `TXT_ONLINE_COMMAND`, compared case-sensitively.
    pub(crate) online: Vec<u16>,
}

/// What one submitted chat line asks the service and the chat log for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum WolChatLine {
    /// `0077E1D0`: send to the channel (or the selected users).
    Say(Vec<u16>),
    /// `0078A6B0` with a valid name and text: page `name`.
    Page { name: Vec<u16>, text: Vec<u16> },
    /// `0078A8F0`: page the last user who paged us (`00B77E30`).
    Reply { text: Vec<u16> },
    /// `0078A470`: locate request API+0x60; the name is also kept at `00B76E28`.
    Locate { name: Vec<u16> },
    /// A command with an error: only this white (0xFFFFFFFF) CSF line is shown.
    Notice(&'static str),
}

const SPACE: u16 = b' ' as u16;

fn ascii_lower(unit: u16) -> u16 {
    if (0x41..0x5B).contains(&unit) {
        unit + 0x20
    } else {
        unit
    }
}

/// `007DDE80` in the C locale: ASCII-only case folding.
fn starts_with_ignore_case(line: &[u16], prefix: &[u16]) -> bool {
    line.len() >= prefix.len()
        && line
            .iter()
            .zip(prefix)
            .all(|(a, b)| ascii_lower(*a) == ascii_lower(*b))
}

fn skip_spaces(rest: &[u16]) -> &[u16] {
    let start = rest
        .iter()
        .position(|unit| *unit != SPACE)
        .unwrap_or(rest.len());
    &rest[start..]
}

/// Copies up to `limit` non-space units, as the native loops do; a longer
/// word stops the copy in the middle of the word.
fn take_word(rest: &[u16], limit: usize) -> (&[u16], &[u16]) {
    let length = rest
        .iter()
        .take(limit)
        .position(|unit| *unit == SPACE)
        .unwrap_or(rest.len().min(limit));
    rest.split_at(length)
}

/// The lobby `0x113`'s two filter words (`0x0078AC10`'s 0x540 Enter runs
/// them after page/reply and before locate). Retail `ra2md.csf` carries
/// neither label, so both hold the string table's missing-label text.
#[derive(Debug, Clone, Default)]
pub(crate) struct WolChatFilterCommands {
    /// `TXT_BUDDY_FILTER`, a case-insensitive prefix (`007DDE80`).
    pub(crate) buddy: Vec<u16>,
    /// `TXT_CLAN_FILTER`, a case-insensitive prefix.
    pub(crate) clan: Vec<u16>,
}

/// One lobby filter toggle: `[0xB76AF0]` (buddy) or `[0xB73AD0]` (clan).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum WolChatFilter {
    Buddy,
    Clan,
}

impl WolChatFilter {
    /// The `L"%s"` line the toggle echoes in [`EMOTE_COLOR`]; the buddy
    /// "off" label is misspelt in the executable.
    pub(crate) fn notice(self, on: bool) -> &'static str {
        match (self, on) {
            (Self::Buddy, true) => "TXT_BUDDY_FILTER_ON",
            (Self::Buddy, false) => "TXT_BUDDY_FITER_OFF",
            (Self::Clan, true) => "TXT_CLAN_FILTER_ON",
            (Self::Clan, false) => "TXT_CLAN_FILTER_OFF",
        }
    }
}

/// What a lobby `0x113` chat line asks for: a shared command or a filter.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum WolLobbyChatLine {
    Line(WolChatLine),
    Filter(WolChatFilter),
}

/// The lobby's order: page (`0078A6B0`), reply (`0078A8F0`), the two filter
/// words, then locate (`0078A470`) and the plain send.
pub(crate) fn parse_lobby(
    line: &[u16],
    commands: &WolChatCommands,
    filters: &WolChatFilterCommands,
    last_pager_known: bool,
) -> WolLobbyChatLine {
    if let Some(parsed) = parse_page_or_reply(line, commands, last_pager_known) {
        return WolLobbyChatLine::Line(parsed);
    }
    for (word, filter) in [
        (&filters.buddy, WolChatFilter::Buddy),
        (&filters.clan, WolChatFilter::Clan),
    ] {
        if !word.is_empty() && starts_with_ignore_case(line, word) {
            return WolLobbyChatLine::Filter(filter);
        }
    }
    WolLobbyChatLine::Line(parse_locate_or_say(line, commands))
}

/// Classify one submitted line. `last_pager_known` is whether `00B77E30`
/// holds a name; an empty line never reaches this parser.
pub(crate) fn parse(
    line: &[u16],
    commands: &WolChatCommands,
    last_pager_known: bool,
) -> WolChatLine {
    parse_page_or_reply(line, commands, last_pager_known)
        .unwrap_or_else(|| parse_locate_or_say(line, commands))
}

fn parse_page_or_reply(
    line: &[u16],
    commands: &WolChatCommands,
    last_pager_known: bool,
) -> Option<WolChatLine> {
    if !commands.page.is_empty() && starts_with_ignore_case(line, &commands.page) {
        let rest = skip_spaces(&line[commands.page.len()..]);
        // 0078A78C..A7B6: at most 11 name units are copied; 0078A7E0 then
        // accepts at most 10 (`cmp ebp, 0xa; jle`).
        let (name, rest) = take_word(rest, 11);
        let text = skip_spaces(rest);
        return Some(if name.is_empty() {
            WolChatLine::Notice("TXT_NO_NAME_SPECIFIED")
        } else if name.len() > 10 {
            WolChatLine::Notice("TXT_NAME_TOO_LONG")
        } else if text.is_empty() {
            WolChatLine::Notice("TXT_CANT_SEND_BLANK_PAGE")
        } else {
            WolChatLine::Page {
                name: name.to_vec(),
                text: text.to_vec(),
            }
        });
    }
    let reply_long = !commands.reply.is_empty() && starts_with_ignore_case(line, &commands.reply);
    let reply_short = !commands.reply_short.is_empty() && line.starts_with(&commands.reply_short);
    if reply_long || reply_short {
        // 0078A9B0: the long word's length is used only for an exact-case
        // match; otherwise the short word's length is skipped.
        let prefix = if line.starts_with(&commands.reply) {
            commands.reply.len()
        } else {
            commands.reply_short.len()
        };
        let text = skip_spaces(line.get(prefix..).unwrap_or_default());
        return Some(if !last_pager_known {
            WolChatLine::Notice("TXT_NO_ONE_WHOM_TO_REPLY")
        } else if text.is_empty() {
            WolChatLine::Notice("TXT_CANT_SEND_BLANK_PAGE")
        } else {
            WolChatLine::Reply {
                text: text.to_vec(),
            }
        });
    }
    None
}

fn parse_locate_or_say(line: &[u16], commands: &WolChatCommands) -> WolChatLine {
    if !commands.online.is_empty() && line.starts_with(&commands.online) {
        let rest = skip_spaces(&line[commands.online.len()..]);
        // 0078A517: `cmp ebx, 0xa; jge` stops after ten units, silently.
        let (name, _) = take_word(rest, 10);
        return if name.is_empty() {
            WolChatLine::Notice("TXT_NO_NAME")
        } else {
            WolChatLine::Locate {
                name: name.to_vec(),
            }
        };
    }
    WolChatLine::Say(line.to_vec())
}

/// `005EDD00`'s `TrimRight(L"\n\r")` (`00831D9C`), whose result WOL sends.
pub(crate) fn trim_line_end(text: &[u16]) -> &[u16] {
    let end = text
        .iter()
        .rposition(|unit| !matches!(*unit, 0x0A | 0x0D))
        .map_or(0, |index| index + 1);
    &text[..end]
}

/// Chat-log colors of the C2 echo lines (BGR-ordered native values).
pub(crate) const SAY_COLOR: u32 = 0x40FFFF;
pub(crate) const PRIVATE_COLOR: u32 = 0xFF8080;
pub(crate) const EMOTE_COLOR: u32 = 0x3050FF;
pub(crate) const PRIVATE_EMOTE_COLOR: u32 = 0xFF00FF;

/// `"[%hs] %s"` at `0084B94C`.
pub(crate) fn say_echo(nickname: &str, text: &[u16]) -> Vec<u16> {
    let mut line: Vec<u16> = format!("[{nickname}] ").encode_utf16().collect();
    line.extend_from_slice(text);
    line
}

/// `"[%hs > %s] %s"` at `0084B9B0`, for pages and replies.
pub(crate) fn page_echo(nickname: &str, target: &[u16], text: &[u16]) -> Vec<u16> {
    let mut line: Vec<u16> = format!("[{nickname} > ").encode_utf16().collect();
    line.extend_from_slice(target);
    line.extend("] ".encode_utf16());
    line.extend_from_slice(text);
    line
}

/// `"%hs %s"` at `0084B224`, the emote echo.
pub(crate) fn emote_echo(nickname: &str, text: &[u16]) -> Vec<u16> {
    let mut line: Vec<u16> = format!("{nickname} ").encode_utf16().collect();
    line.extend_from_slice(text);
    line
}

/// The chat sink's (`CChatEventSink`, vtable `0x007F76B4`) colors of
/// received lines (table `0x007EEF28`, BGR-ordered like the echoes).
pub(crate) const PUBLIC_COLOR: u32 = 0xF0F0;
pub(crate) const OPERATOR_COLOR: u32 = 0xFFFF00;
/// The page sink's white (`[0x007EEF40]`).
pub(crate) const PAGE_COLOR: u32 = 0xFF_FFFF;

/// The sender record a sink receives (`User*`): flags `+0x00`, name
/// `+0x24`, clan `+0x2E`.
pub(crate) struct WolSender<'a> {
    pub(crate) operator: bool,
    pub(crate) name: &'a str,
    pub(crate) clan: &'a str,
}

/// The `[WOnline]` display filters the sinks read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct WolDisplayFilters {
    /// `DisplayLatin` `[0xB779C0]` and `DisplayAsian` `[0xB779DC]`.
    pub(crate) latin: bool,
    pub(crate) asian: bool,
    /// `DisplayBuddies` `[0xB76AF0]` and `DisplayClan` `[0xB73AD0]`.
    pub(crate) buddies: bool,
    pub(crate) clan: bool,
}

/// The language gate inlined in every chat, action and page sink (its
/// out-of-line copy is `0x0077D2E0`): an operator, or a running setup
/// C2/C9, always passes; otherwise Latin-1 text (`0x007B7D90`: every unit
/// below 0x100) needs DisplayLatin and any other text DisplayAsian.
pub(crate) fn language_gate(
    sender: &WolSender,
    setup_open: bool,
    text: &[u16],
    filters: WolDisplayFilters,
) -> bool {
    if sender.operator || setup_open {
        return true;
    }
    if text.iter().all(|unit| *unit < 0x100) {
        filters.latin
    } else {
        filters.asian
    }
}

/// `0x0077D4C0`: with DisplayBuddies or DisplayClan on and the lobby
/// `0x113` in the shell stack, a sender passes when its clan (`+0x2E`)
/// equals the local user's (`strcmpi`; two empty clans are equal) or, with
/// DisplayBuddies, when its name equals a buddy's byte for byte. Without a
/// local record in the channel's users (`0xB77908`) everything passes.
pub(crate) fn buddy_clan_gate(
    sender: &WolSender,
    local_clan: Option<&str>,
    buddies: &[String],
    filters: WolDisplayFilters,
    lobby_open: bool,
) -> bool {
    if !(filters.buddies || filters.clan) || !lobby_open {
        return true;
    }
    let Some(local_clan) = local_clan else {
        return true;
    };
    let clan_match = sender.clan.eq_ignore_ascii_case(local_clan);
    let buddy_match = buddies.iter().any(|buddy| buddy == sender.name);
    match (filters.buddies, filters.clan) {
        (true, true) => clan_match || buddy_match,
        (true, false) => buddy_match,
        _ => clan_match,
    }
}

/// A received chat or action line and its color: public `"[%hs] : %s"`
/// (`0x0084AFB0`, operator yellow), private `"%hs : %s"` (`0x0084AFC8`,
/// operator yellow), public and private actions `"%hs %s"` (`0x0084B224`)
/// in the echo colors (`0x007A8DA0`, `0x007A8FC0`, `0x007AB170`,
/// `0x007AAFB0` and their UTF-16 forms).
pub(crate) fn received_line(
    sender: &WolSender,
    text: &[u16],
    private: bool,
    action: bool,
) -> (Vec<u16>, u32) {
    let (prefix, color) = match (action, private) {
        (true, false) => (format!("{} ", sender.name), EMOTE_COLOR),
        (true, true) => (format!("{} ", sender.name), PRIVATE_EMOTE_COLOR),
        (false, false) => (
            format!("[{}] : ", sender.name),
            if sender.operator {
                OPERATOR_COLOR
            } else {
                PUBLIC_COLOR
            },
        ),
        (false, true) => (
            format!("{} : ", sender.name),
            if sender.operator {
                OPERATOR_COLOR
            } else {
                PRIVATE_COLOR
            },
        ),
    };
    let mut line: Vec<u16> = prefix.encode_utf16().collect();
    line.extend_from_slice(text);
    (line, color)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn w(text: &str) -> Vec<u16> {
        text.encode_utf16().collect()
    }

    fn commands() -> WolChatCommands {
        WolChatCommands {
            page: w("/page"),
            reply: w("/reply"),
            reply_short: w("/r"),
            online: w("/locate"),
        }
    }

    const ALL: WolDisplayFilters = WolDisplayFilters {
        latin: true,
        asian: true,
        buddies: false,
        clan: false,
    };

    fn sender<'a>(name: &'a str, clan: &'a str, operator: bool) -> WolSender<'a> {
        WolSender {
            operator,
            name,
            clan,
        }
    }

    #[test]
    fn the_language_gate_reads_the_text_unless_an_operator_or_setup() {
        let latin = WolDisplayFilters {
            asian: false,
            ..ALL
        };
        let asian = WolDisplayFilters {
            latin: false,
            ..ALL
        };
        let user = sender("Boris", "", false);
        // U+00FF is still Latin-1; U+0100 is not.
        assert!(language_gate(&user, false, &w("caf\u{e9} \u{ff}"), latin));
        assert!(!language_gate(&user, false, &w("\u{100}"), latin));
        assert!(language_gate(&user, false, &w("\u{4f60}"), asian));
        assert!(!language_gate(&user, false, &w("hi"), asian));
        assert!(language_gate(
            &sender("Tanya", "", true),
            false,
            &w("hi"),
            asian
        ));
        assert!(language_gate(&user, true, &w("hi"), asian));
    }

    #[test]
    fn the_buddy_and_clan_gate_follows_0x77d4c0() {
        let clan = WolDisplayFilters { clan: true, ..ALL };
        let buddies = WolDisplayFilters {
            buddies: true,
            ..ALL
        };
        let both = WolDisplayFilters {
            buddies: true,
            clan: true,
            ..ALL
        };
        let friend = ["Boris".to_owned()];
        let boris = sender("Boris", "RED", false);
        let tanya = sender("Tanya", "", false);
        // Off, outside the lobby, or without a local record: everything passes.
        assert!(buddy_clan_gate(&boris, Some("BLUE"), &[], ALL, true));
        assert!(buddy_clan_gate(&boris, Some("BLUE"), &[], clan, false));
        assert!(buddy_clan_gate(&boris, None, &[], both, true));
        // Clan names compare without case; two empty clans are equal.
        assert!(buddy_clan_gate(&boris, Some("red"), &[], clan, true));
        assert!(!buddy_clan_gate(&boris, Some("BLUE"), &[], clan, true));
        assert!(buddy_clan_gate(&tanya, Some(""), &[], clan, true));
        // Buddy names compare byte for byte.
        assert!(buddy_clan_gate(&boris, Some(""), &friend, buddies, true));
        assert!(!buddy_clan_gate(
            &sender("boris", "", false),
            Some(""),
            &friend,
            buddies,
            true
        ));
        // With no buddies, DisplayBuddies alone hides every sender.
        assert!(!buddy_clan_gate(&tanya, Some(""), &[], buddies, true));
        // Both: the clan match or the buddy match.
        assert!(buddy_clan_gate(&tanya, Some(""), &[], both, true));
        assert!(buddy_clan_gate(&boris, Some("BLUE"), &friend, both, true));
        assert!(!buddy_clan_gate(&boris, Some("BLUE"), &[], both, true));
    }

    #[test]
    fn received_lines_take_their_sink_format_and_color() {
        let text = w("hi");
        let user = sender("Boris", "", false);
        let operator = sender("Tanya", "", true);
        assert_eq!(
            received_line(&user, &text, false, false),
            (w("[Boris] : hi"), PUBLIC_COLOR)
        );
        assert_eq!(
            received_line(&operator, &text, false, false),
            (w("[Tanya] : hi"), OPERATOR_COLOR)
        );
        assert_eq!(
            received_line(&user, &text, true, false),
            (w("Boris : hi"), PRIVATE_COLOR)
        );
        assert_eq!(
            received_line(&operator, &text, true, false),
            (w("Tanya : hi"), OPERATOR_COLOR)
        );
        assert_eq!(
            received_line(&operator, &text, false, true),
            (w("Tanya hi"), EMOTE_COLOR)
        );
        assert_eq!(
            received_line(&user, &text, true, true),
            (w("Boris hi"), PRIVATE_EMOTE_COLOR)
        );
    }

    #[test]
    fn page_takes_a_name_of_at_most_ten_units_and_needs_text() {
        let c = commands();
        assert_eq!(
            parse(&w("/PAGE  bob   hi there"), &c, false),
            WolChatLine::Page {
                name: w("bob"),
                text: w("hi there")
            }
        );
        assert_eq!(
            parse(&w("/page abcdefghij x"), &c, false),
            WolChatLine::Page {
                name: w("abcdefghij"),
                text: w("x")
            }
        );
        assert_eq!(
            parse(&w("/page abcdefghijk x"), &c, false),
            WolChatLine::Notice("TXT_NAME_TOO_LONG")
        );
        assert_eq!(
            parse(&w("/page   "), &c, false),
            WolChatLine::Notice("TXT_NO_NAME_SPECIFIED")
        );
        assert_eq!(
            parse(&w("/page bob  "), &c, false),
            WolChatLine::Notice("TXT_CANT_SEND_BLANK_PAGE")
        );
    }

    #[test]
    fn reply_needs_a_previous_pager_and_skips_the_short_word_on_case_mismatch() {
        let c = commands();
        assert_eq!(
            parse(&w("/reply ok"), &c, false),
            WolChatLine::Notice("TXT_NO_ONE_WHOM_TO_REPLY")
        );
        assert_eq!(
            parse(&w("/reply ok"), &c, true),
            WolChatLine::Reply { text: w("ok") }
        );
        assert_eq!(
            parse(&w("/r ok"), &c, true),
            WolChatLine::Reply { text: w("ok") }
        );
        // Case-insensitive long match, short-word length skipped (0078A9B0).
        assert_eq!(
            parse(&w("/REPLY ok"), &c, true),
            WolChatLine::Reply { text: w("EPLY ok") }
        );
        assert_eq!(
            parse(&w("/r   "), &c, true),
            WolChatLine::Notice("TXT_CANT_SEND_BLANK_PAGE")
        );
    }

    #[test]
    fn locate_is_case_sensitive_and_truncates_names_to_ten_units() {
        let c = commands();
        assert_eq!(
            parse(&w("/locate abcdefghijkl"), &c, false),
            WolChatLine::Locate {
                name: w("abcdefghij")
            }
        );
        assert_eq!(
            parse(&w("/locate "), &c, false),
            WolChatLine::Notice("TXT_NO_NAME")
        );
        assert_eq!(
            parse(&w("/LOCATE bob"), &c, false),
            WolChatLine::Say(w("/LOCATE bob"))
        );
    }

    #[test]
    fn lobby_lines_check_the_filters_between_reply_and_locate() {
        let c = commands();
        let filters = WolChatFilterCommands {
            buddy: w("/buddy"),
            clan: w("/clan"),
        };
        assert_eq!(
            parse_lobby(&w("/BUDDY now"), &c, &filters, false),
            WolLobbyChatLine::Filter(WolChatFilter::Buddy)
        );
        assert_eq!(
            parse_lobby(&w("/clan"), &c, &filters, false),
            WolLobbyChatLine::Filter(WolChatFilter::Clan)
        );
        // Page and reply come first, locate and the plain line after.
        assert_eq!(
            parse_lobby(&w("/reply x"), &c, &filters, false),
            WolLobbyChatLine::Line(WolChatLine::Notice("TXT_NO_ONE_WHOM_TO_REPLY"))
        );
        assert_eq!(
            parse_lobby(&w("/locate bob"), &c, &filters, false),
            WolLobbyChatLine::Line(WolChatLine::Locate { name: w("bob") })
        );
        assert_eq!(
            parse_lobby(&w("hello"), &c, &filters, false),
            WolLobbyChatLine::Line(WolChatLine::Say(w("hello")))
        );
        assert_eq!(WolChatFilter::Buddy.notice(false), "TXT_BUDDY_FITER_OFF");
    }

    #[test]
    fn submitted_lines_lose_only_trailing_line_breaks() {
        assert_eq!(trim_line_end(&w(" hi \r\n\n")), w(" hi "));
        assert!(trim_line_end(&w("\r\n")).is_empty());
    }

    #[test]
    fn echo_lines_use_the_native_formats() {
        assert_eq!(say_echo("me", &w("hi")), w("[me] hi"));
        assert_eq!(page_echo("me", &w("bob"), &w("hi")), w("[me > bob] hi"));
        assert_eq!(emote_echo("me", &w("waves")), w("me waves"));
    }
}
