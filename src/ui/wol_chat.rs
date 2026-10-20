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
