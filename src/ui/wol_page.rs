//! The WOL page dialogs: Page `0xCF` (proc `0x0078D120`), run modally from
//! `0x113`, C2 and C9 (`0x0078BA3E`, `0x00790C8D`, `0x0079235F`), and Page
//! Received `0x11E` (proc `0x0079DB50`), opened modeless by the page sink
//! when no dialog in the shell stack has a transcript `0x53F`.
//!
//! The procs dispatch `WM_COMMAND` on the control id alone, so every
//! notification of an edit runs its handler: focus gained or lost (the
//! original Edit proc sends `EN_SETFOCUS`/`EN_KILLFOCUS`) and each admitted
//! character (NewEdit `0x00614B30` sends `EN_CHANGE` and `EN_UPDATE`). The
//! text setters `0x4B2`/`0x4B4` notify nothing.

use crate::ui::lan_shell::{LanChatLine, LanEditKey, LanListState, apply_chat_length_limit};
use crate::ui::shell::button::ShellButtonInteraction;
use crate::ui::skirmish_shell::SavedSeedDescriptionEdit;

/// `0xCF`'s text to send and `0x11E`'s reply, both `0x540`.
pub(crate) const TEXT: u16 = 0x540;
/// `0xCF`'s name edit.
pub(crate) const NAME: u16 = 0x5C1;
/// `0xCF`'s transcript (`LBS_NOSEL`).
pub(crate) const PAGE_TRANSCRIPT: u16 = 0x5B5;
/// `0x11E`'s transcript (selectable, `LBS_NOTIFY`).
pub(crate) const RECEIVED_TRANSCRIPT: u16 = 0x53F;
/// Page (`0xCF`) and Send (`0x11E`).
pub(crate) const SEND: u16 = 0x5C2;
pub(crate) const CANCEL: u16 = 2;
/// `0x11E`'s Back.
pub(crate) const BACK: u16 = 0x686;

/// `EM_LIMITTEXT 9` on `0xCF`'s name (`0x0078D855`).
const NAME_LIMIT: usize = 9;
/// `EM_LIMITTEXT 0x78` on `0x11E`'s reply.
const REPLY_LIMIT: usize = 120;

/// What a `0xCF` handler asks of the WOL service and its owner.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum WolPageAction {
    /// API+0x60 for the name (a 0x6C-byte record, name at +0x24); the
    /// status `0x5C8` was cleared.
    Locate(String),
    /// Page with a name and text: the owner applies the observer gate
    /// (`0x0078D52A..0x0078D566`), then API+0xF8 and the echo.
    Send { name: String, text: Vec<u16> },
    /// A white (-1) line into the transcript through `0x005E2790`.
    Notice(&'static str),
    /// Cancel: `0x007757E0(dlg, 2)`.
    Close,
}

/// Dialog `0xCF` while it exists.
#[derive(Debug, Clone, Default)]
pub(crate) struct WolPageDialog {
    pub(crate) name: SavedSeedDescriptionEdit,
    pub(crate) text: SavedSeedDescriptionEdit,
    /// The control holding the keyboard focus; none (the dialog) at open.
    pub(crate) focus: Option<u16>,
    pub(crate) lines: Vec<LanChatLine>,
    pub(crate) list: LanListState,
    /// `0x5C8`: the locate result.
    pub(crate) status: String,
    /// `0x5C2`, created disabled.
    pub(crate) send_enabled: bool,
    pub(crate) buttons: ShellButtonInteraction<u16>,
    /// The focused edit's caret blink start (NewEdit's 1000 ms timer).
    pub(crate) focus_at: Option<std::time::Instant>,
}

/// The page reply window `{start [0xB76478], duration [0xB76480]}` in
/// `timeGetTime() >> 4` ticks (`0x006C8C40`). Its writers: the static
/// initializer `0x0077D160` and the game-start sink `0x007A7B82` (now, 0),
/// the ANSI page sink in a match `0x007A93B9` (now, 300 = 4800 ms), and the
/// `0xCF` prefill that uses it up (now, 0).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct PageReplyWindow {
    start: i32,
    duration: i32,
}

/// The ANSI page sink's window in a match (`0x007A93C9`).
pub(crate) const REPLY_WINDOW_TICKS: i32 = 300;

impl PageReplyWindow {
    pub(crate) fn arm(&mut self, now: i32, duration: i32) {
        *self = Self {
            start: now,
            duration,
        };
    }

    /// `0x0078D8A1..0x0078D8CD`: the ticks left. `start` is never -1 after
    /// the static initializer, so the elapsed branch always runs.
    fn remaining(self, now: i32) -> i32 {
        let elapsed = now.wrapping_sub(self.start);
        if elapsed >= self.duration {
            0
        } else {
            self.duration - elapsed
        }
    }
}

/// The observer gate `0x0078D52A..0x0078D566`: with `[0xA8B538]` set, a
/// name `_stricmp`-equal (`0x007C8D20`) to any connection node's.
pub(crate) fn observer_refuses(out: bool, nodes: &[String], name: &str) -> bool {
    out && nodes.iter().any(|node| node.eq_ignore_ascii_case(name))
}

/// `timeGetTime() >> 4` (`0x006C8C40`).
pub(crate) fn reply_window_now(time_get_time_ms: u64) -> i32 {
    ((time_get_time_ms as u32) >> 4) as i32
}

impl WolPageDialog {
    /// `0x497` without a pending reply window.
    pub(crate) fn new() -> Self {
        Self::default()
    }

    /// `0x497` (`0x0078D855..0x0078D91B`): the name edit limited to 9 byte
    /// characters (`0x4D1(1)`) and emptied with `0x4B4`, which notifies
    /// nothing. Focus stays on the dialog (`0x00622B50`'s
    /// `SetFocus(hDialog)`) unless the reply window has time left: then the
    /// name is the last pager `[0xB77E30]` (again `0x4B4`), the text edit
    /// takes the focus, whose notification runs the text handler, and the
    /// window restarts with no time.
    pub(crate) fn open(
        window: &mut PageReplyWindow,
        now: i32,
        last_pager: Option<&str>,
        last: &mut u16,
    ) -> (Self, Vec<WolPageAction>) {
        let mut page = Self::new();
        let mut actions = Vec::new();
        if window.remaining(now) > 0 {
            page.name
                .set_units_limited(last_pager.unwrap_or_default().encode_utf16(), 0);
            page.focus_to(Some(TEXT), last, &mut actions);
            window.arm(now, 0);
        }
        (page, actions)
    }

    fn name_text(&self) -> String {
        self.name.narrow_text(usize::MAX)
    }

    fn enable_send(&mut self) {
        self.send_enabled = !self.name.units.is_empty() && !self.text.units.is_empty();
    }

    /// One `WM_COMMAND` from an edit. `last` is `[0xB77E8C]`, process
    /// global and never reset at open.
    fn notify(&mut self, control: u16, last: &mut u16, actions: &mut Vec<WolPageAction>) {
        match control {
            // 0x0078D1C0..0x0078D272.
            NAME => {
                self.enable_send();
                *last = NAME;
                self.status.clear();
            }
            // 0x0078D275..0x0078D37C: 0x005EDD00 caps the text first.
            TEXT => {
                apply_chat_length_limit(&mut self.text);
                self.enable_send();
                if *last == NAME {
                    actions.push(WolPageAction::Locate(self.name_text()));
                    self.status.clear();
                }
                *last = TEXT;
            }
            _ => {}
        }
    }

    /// `SetFocus`: the old control's kill notification, then the new one's.
    fn focus_to(&mut self, target: Option<u16>, last: &mut u16, actions: &mut Vec<WolPageAction>) {
        if self.focus == target {
            return;
        }
        let old = std::mem::replace(&mut self.focus, target);
        self.focus_at = Some(std::time::Instant::now());
        self.name.focused = target == Some(NAME);
        self.text.focused = target == Some(TEXT);
        if let Some(old) = old {
            self.notify(old, last, actions);
        }
        if let Some(target) = target {
            self.notify(target, last, actions);
        }
    }

    /// A press on an edit takes the focus.
    pub(crate) fn press(&mut self, control: u16, last: &mut u16) -> Vec<WolPageAction> {
        let mut actions = Vec::new();
        if matches!(control, NAME | TEXT) {
            self.focus_to(Some(control), last, &mut actions);
        }
        actions
    }

    /// A press on a transcript row: the list subclass `0x00618D40` calls
    /// `SetFocus(list)`, whose `WM_SETFOCUS` the owner-draw default handler
    /// (`0x006118BF..0x006118DE`) hands straight back to the previous
    /// control. The focused edit keeps the focus and sees its kill and set
    /// notifications.
    pub(crate) fn press_transcript_row(&mut self, last: &mut u16) -> Vec<WolPageAction> {
        let mut actions = Vec::new();
        if let Some(edit) = self.focus {
            self.notify(edit, last, &mut actions);
            self.notify(edit, last, &mut actions);
        }
        actions
    }

    pub(crate) fn insert_text(&mut self, text: &str, last: &mut u16) -> Vec<WolPageAction> {
        let mut actions = Vec::new();
        for unit in text.encode_utf16() {
            let admitted = match self.focus {
                Some(NAME) => self.name.insert_unit_filtered(unit, NAME_LIMIT, true, &[]),
                // 0x540 has no EM_LIMITTEXT; its handler caps it.
                Some(TEXT) => self.text.insert_unit_filtered(unit, 0, false, &[]),
                _ => false,
            };
            if admitted && let Some(control) = self.focus {
                self.notify(control, last, &mut actions);
            }
        }
        actions
    }

    pub(crate) fn key(&mut self, key: LanEditKey, last: &mut u16) -> Vec<WolPageAction> {
        let mut actions = Vec::new();
        let Some(control @ (NAME | TEXT)) = self.focus else {
            return actions;
        };
        match key {
            // 0x4DA: BM_CLICK on 0x5C2, which a disabled button ignores.
            LanEditKey::Enter => return self.activate(SEND, last),
            // 0x4DB (0x0078D93A..0x0078D9FB): 0x00600C70 moves the focus to
            // the other tab stop; from the name it locates again.
            LanEditKey::Tab => {
                let other = if control == NAME { TEXT } else { NAME };
                self.focus_to(Some(other), last, &mut actions);
                if control == NAME {
                    actions.push(WolPageAction::Locate(self.name.narrow_text(10)));
                    self.status.clear();
                    *last = TEXT;
                }
                return actions;
            }
            _ => {}
        }
        let edit = if control == NAME {
            &mut self.name
        } else {
            &mut self.text
        };
        let before = (edit.units.clone(), edit.caret);
        match key {
            LanEditKey::Backspace => edit.backspace(),
            LanEditKey::Delete => edit.delete(),
            LanEditKey::Left => edit.left(),
            LanEditKey::Right => edit.right(),
            LanEditKey::Home => edit.home(),
            LanEditKey::End => edit.end(),
            LanEditKey::Tab | LanEditKey::Enter => {}
        }
        if edit.units != before.0 {
            self.notify(control, last, &mut actions);
        }
        actions
    }

    /// A button released over itself, or Enter's `BM_CLICK`.
    pub(crate) fn activate(&mut self, control: u16, last: &mut u16) -> Vec<WolPageAction> {
        let mut actions = Vec::new();
        match control {
            CANCEL => actions.push(WolPageAction::Close),
            // 0x0078D39A..0x0078D626.
            SEND if self.send_enabled => {
                if *last == NAME {
                    actions.push(WolPageAction::Locate(self.name.narrow_text(10)));
                    self.status.clear();
                }
                *last = SEND;
                apply_chat_length_limit(&mut self.text);
                let mut text = self.text.wide_text(usize::MAX);
                while matches!(text.last(), Some(10 | 13)) {
                    text.pop();
                }
                self.text.set_units_limited([], 0);
                self.send_enabled = false;
                self.focus_to(Some(TEXT), last, &mut actions);
                let mut name = self.name_text();
                name.truncate(NAME_LIMIT);
                actions.push(match (name.is_empty(), text.is_empty()) {
                    (false, false) => WolPageAction::Send { name, text },
                    (true, _) => WolPageAction::Notice("TXT_NAME_ERROR"),
                    (false, true) => WolPageAction::Notice("TXT_ENTER_MESSAGE"),
                });
            }
            _ => {}
        }
        actions
    }
}

/// What a `0x11E` handler asks of its owner.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum WolPageReceivedAction {
    /// API+0xF8 to the reply-to name, and the echo into its own list.
    Send { name: String, text: Vec<u16> },
    /// Back: `0x007757E0(dlg, 0x5C2)`.
    Close,
}

/// Dialog `0x11E` while it exists.
#[derive(Debug, Clone, Default)]
pub(crate) struct WolPageReceived {
    pub(crate) reply: SavedSeedDescriptionEdit,
    pub(crate) lines: Vec<LanChatLine>,
    pub(crate) list: LanListState,
    /// `0x718`.
    pub(crate) reply_to: String,
    pub(crate) send_enabled: bool,
    pub(crate) buttons: ShellButtonInteraction<u16>,
    pub(crate) focus_at: Option<std::time::Instant>,
    /// `[0xB76E24]`, the selected buddy, as it was at open: only `0x116`
    /// writes it, and nothing under `0x11E` takes input while it exists.
    pub(crate) fallback: String,
    /// Opened by `0x116`'s Page (`0x0079EA23`): modal over the Buddy List
    /// instead of modeless over the empty backdrop.
    pub(crate) modal: bool,
}

impl WolPageReceived {
    /// `0x497`: the reply limited to 120 and empty, Send disabled.
    pub(crate) fn new(fallback: &str, modal: bool) -> Self {
        Self {
            fallback: fallback.to_owned(),
            modal,
            ..Self::default()
        }
    }

    /// `0x0079DA90`: from the selected row (row 0 without one) to the end,
    /// the first row starting `(` with a `)`: the text before that `)`
    /// without its `(`s; else `[0xB76E24]`.
    pub(crate) fn pager_from_list(&self) -> String {
        let start = self.list.selected.unwrap_or(0);
        self.lines
            .iter()
            .skip(start)
            .find_map(|line| {
                let text = String::from_utf16_lossy(&line.text);
                let close = text.find(')')?;
                text.starts_with('(')
                    .then(|| text[..close].replace('(', ""))
            })
            .unwrap_or_else(|| self.fallback.clone())
    }

    /// `LBN_SELCHANGE`: the reply-to follows the selection.
    pub(crate) fn select(&mut self, row: usize) {
        if row < self.lines.len() {
            self.list.selected = Some(row);
            self.reply_to = self.pager_from_list();
        }
    }

    /// A press on the reply edit takes the focus; a transcript press hands
    /// it back to the previous control (see
    /// [`WolPageDialog::press_transcript_row`]).
    pub(crate) fn press(&mut self, control: u16) {
        if control == TEXT && !self.reply.focused {
            self.focus_at = Some(std::time::Instant::now());
            self.reply.focused = true;
        }
    }

    /// `EN_CHANGE` (`0x300`): Send follows the text.
    fn changed(&mut self) {
        self.send_enabled = !self.reply.units.is_empty();
    }

    pub(crate) fn insert_text(&mut self, text: &str) {
        if !self.reply.focused {
            return;
        }
        for unit in text.encode_utf16() {
            if self
                .reply
                .insert_unit_filtered(unit, REPLY_LIMIT, false, &[])
            {
                self.changed();
            }
        }
    }

    pub(crate) fn key(&mut self, key: LanEditKey) -> Option<WolPageReceivedAction> {
        if !self.reply.focused {
            return None;
        }
        let before = self.reply.units.clone();
        match key {
            // The multiline NewEdit appends CRLF and sends 0x501; a text
            // that is not empty once trimmed clicks Send.
            LanEditKey::Enter => {
                self.reply.append_enter_line_end();
                let trimmed = trim_line_ends(&self.reply.units);
                return (!trimmed.is_empty()).then(|| self.activate(SEND)).flatten();
            }
            LanEditKey::Backspace => self.reply.backspace(),
            LanEditKey::Delete => self.reply.delete(),
            LanEditKey::Left => self.reply.left(),
            LanEditKey::Right => self.reply.right(),
            LanEditKey::Home => self.reply.home(),
            LanEditKey::End => self.reply.end(),
            LanEditKey::Tab => {}
        }
        if self.reply.units != before {
            self.changed();
        }
        None
    }

    /// Send (`0x0079DB50`'s `0x5C2`) or Back.
    pub(crate) fn activate(&mut self, control: u16) -> Option<WolPageReceivedAction> {
        match control {
            BACK => Some(WolPageReceivedAction::Close),
            SEND if self.send_enabled => {
                let text = self.reply.wide_text(usize::MAX);
                self.reply.set_units_limited([], 0);
                let name = self.pager_from_list();
                self.send_enabled = false;
                (!name.is_empty() && !text.is_empty()).then(|| WolPageReceivedAction::Send {
                    name,
                    text: trim_line_ends(&text).to_vec(),
                })
            }
            _ => None,
        }
    }
}

/// NewEdit's caret: shown in the first of each two seconds of focus.
pub(crate) fn caret_visible(focus_at: Option<std::time::Instant>, now: std::time::Instant) -> bool {
    focus_at.is_some_and(|at| now.saturating_duration_since(at).as_secs() % 2 == 0)
}

/// TrimRight with `"\n\r"` (`0x00831D9C`).
fn trim_line_ends(units: &[u16]) -> &[u16] {
    let end = units
        .iter()
        .rposition(|unit| !matches!(unit, 10 | 13))
        .map_or(0, |index| index + 1);
    &units[..end]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn page_follows_both_fields_and_leaving_the_name_locates_it() {
        let mut last = 0u16;
        let mut page = WolPageDialog::new();
        assert!(page.insert_text("x", &mut last).is_empty());
        assert!(page.press(NAME, &mut last).is_empty());
        page.insert_text("Tanya123456", &mut last);
        assert_eq!(page.name_text(), "Tanya1234");
        assert!(!page.send_enabled && last == NAME);
        // Clicking into the text field locates the name once.
        assert_eq!(
            page.press(TEXT, &mut last),
            vec![WolPageAction::Locate("Tanya1234".into())]
        );
        assert!(page.insert_text("hi", &mut last).is_empty());
        assert!(page.send_enabled);
        let sent = page.key(LanEditKey::Enter, &mut last);
        assert_eq!(
            sent,
            vec![WolPageAction::Send {
                name: "Tanya1234".into(),
                text: "hi".encode_utf16().collect()
            }]
        );
        // The text edit already has the focus, so SetFocus notifies nothing.
        assert!(page.text.units.is_empty() && !page.send_enabled && last == SEND);
        // A second Enter with Page disabled does nothing.
        assert!(page.key(LanEditKey::Enter, &mut last).is_empty());
    }

    #[test]
    fn tab_from_the_name_locates_twice() {
        let mut last = TEXT;
        let mut page = WolPageDialog::new();
        page.press(NAME, &mut last);
        page.insert_text("Echo", &mut last);
        let actions = page.key(LanEditKey::Tab, &mut last);
        assert_eq!(
            actions,
            vec![
                WolPageAction::Locate("Echo".into()),
                WolPageAction::Locate("Echo".into())
            ]
        );
        assert_eq!((page.focus, last), (Some(TEXT), TEXT));
        // Tab back from the text field only moves the focus.
        assert!(page.key(LanEditKey::Tab, &mut last).is_empty());
        assert_eq!(page.focus, Some(NAME));
    }

    #[test]
    fn an_open_inside_the_reply_window_prefills_the_pager_once() {
        let mut window = PageReplyWindow::default();
        let mut last = NAME;
        // The static initializer's window has no time.
        let (page, actions) = WolPageDialog::open(&mut window, 50, Some("Tanya"), &mut last);
        assert!(actions.is_empty() && page.focus.is_none() && page.name.units.is_empty());
        window.arm(1_000, REPLY_WINDOW_TICKS);
        let (page, actions) = WolPageDialog::open(&mut window, 1_299, Some("Tanya"), &mut last);
        assert_eq!(page.name_text(), "Tanya");
        assert_eq!(page.focus, Some(TEXT));
        // The text edit's focus notification sees `last == 0x5C1`.
        assert_eq!(actions, vec![WolPageAction::Locate("Tanya".into())]);
        assert_eq!(last, TEXT);
        assert!(!page.send_enabled);
        // The prefill used the window up.
        let (page, _) = WolPageDialog::open(&mut window, 1_300, Some("Tanya"), &mut last);
        assert!(page.focus.is_none());
        window.arm(1_000, REPLY_WINDOW_TICKS);
        let (page, _) = WolPageDialog::open(&mut window, 1_300, Some("Tanya"), &mut last);
        assert!(page.focus.is_none(), "300 ticks elapsed: no time left");
    }

    #[test]
    fn an_observer_cannot_page_a_player_in_the_game() {
        let nodes = ["Tester".to_owned(), "Hotel".to_owned()];
        assert!(observer_refuses(true, &nodes, "hOTEL"));
        assert!(!observer_refuses(true, &nodes, "Tanya"), "outsiders pass");
        assert!(!observer_refuses(false, &nodes, "Hotel"), "players pass");
    }

    #[test]
    fn a_transcript_row_click_hands_the_focus_back_to_the_edit() {
        let mut last = 0u16;
        let mut page = WolPageDialog::new();
        page.press(NAME, &mut last);
        page.insert_text("Tanya", &mut last);
        page.status = "Lobby 1".into();
        // The name's kill and set notifications: `last` and the status only.
        assert!(page.press_transcript_row(&mut last).is_empty());
        assert_eq!((page.focus, last), (Some(NAME), NAME));
        assert!(page.status.is_empty());
        page.insert_text("x", &mut last);
        assert_eq!(page.name_text(), "Tanyax");
        // From the text field the first notification locates once.
        page.press(TEXT, &mut last);
        last = NAME;
        assert_eq!(
            page.press_transcript_row(&mut last),
            vec![WolPageAction::Locate("Tanyax".into())]
        );
        assert_eq!((page.focus, last), (Some(TEXT), TEXT));
        // With the dialog itself focused nothing is notified.
        let mut fresh = WolPageDialog::new();
        assert!(fresh.press_transcript_row(&mut last).is_empty());
        assert_eq!(fresh.focus, None);
    }

    #[test]
    fn page_received_replies_to_the_first_pager_from_the_selection() {
        let line = |text: &str| LanChatLine {
            text: text.encode_utf16().collect(),
            color: 0xFF_FFFF,
        };
        let mut received = WolPageReceived::new("Natasha", false);
        assert_eq!(received.pager_from_list(), "Natasha");
        received.lines = vec![
            line("(Tanya) first"),
            line("[Me] reply"),
            line("(Boris) second"),
        ];
        assert_eq!(received.pager_from_list(), "Tanya");
        received.select(1);
        assert_eq!(received.reply_to, "Boris");
        received.press(TEXT);
        received.insert_text("o");
        // Choosing another pager keeps the reply's focus.
        received.select(0);
        received.press(RECEIVED_TRANSCRIPT);
        received.select(1);
        received.insert_text("k");
        assert!(received.send_enabled);
        assert_eq!(
            received.key(LanEditKey::Enter),
            Some(WolPageReceivedAction::Send {
                name: "Boris".into(),
                text: "ok".encode_utf16().collect()
            })
        );
        assert!(received.reply.units.is_empty() && !received.send_enabled);
    }
}
