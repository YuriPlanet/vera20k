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
//!
//! The edits' text, caret and focus, the lists' top and selection and the
//! buttons' enables live in the dialogs' widgets ([`page_spec`],
//! [`received_spec`]); the models keep the rows and the statics.

use vera20k_gui::{InputEvent, Key, NodeKind, NodeSpec, Prop, UiEvent, Value};

use crate::ui::lan_shell::{LanChatLine, apply_chat_length_limit};
use crate::ui::shell::controller::DialogController;
use crate::ui::shell::descriptor::DialogResourceId;
use crate::ui::shell::geom::ShellAlign;
use crate::ui::shell::page;
use crate::ui::wol_chat::trim_line_end;
use vera20k_gui::shell::new_edit::NewEdit;

pub(crate) const PAGE_DIALOG: DialogResourceId = DialogResourceId(0x00CF);
pub(crate) const RECEIVED_DIALOG: DialogResourceId = DialogResourceId(0x011E);
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
/// `0xCF`'s locate status and `0x11E`'s reply-to name.
pub(crate) const STATUS: u16 = 0x5C8;
pub(crate) const REPLY_TO: u16 = 0x718;
/// The captions' statics, which gamemd creates without a control id: VERA's
/// own handles for them in the trees.
pub(crate) const NAME_CAPTION: u16 = 0xF001;
pub(crate) const TEXT_CAPTION: u16 = 0xF002;
pub(crate) const STATUS_CAPTION: u16 = 0xF003;
pub(crate) const TITLE_CAPTION: u16 = 0xF004;
pub(crate) const REPLY_TO_CAPTION: u16 = 0xF005;

/// `EM_LIMITTEXT 9` on `0xCF`'s name (`0x0078D855`), which its locates
/// read into 10 bytes and Send cuts to (`0x0078D39A`). VERA deviation: the
/// lobby's names run to 24 characters, so all three take 24.
const NAME_LIMIT: usize = crate::ui::wol_account::NAME_LIMIT;
/// `EM_LIMITTEXT 0x78` on `0x11E`'s reply.
const REPLY_LIMIT: i32 = 120;

/// A transcript: an owner-draw list window whose rows the page paints and
/// whose every row press selects.
fn transcript(id: u16) -> NodeSpec {
    NodeSpec::new(NodeKind::List)
        .id(id.into())
        .with(Prop::RowCount, Value::Int(0))
}

fn node(kind: NodeKind, id: u16) -> NodeSpec {
    NodeSpec::new(kind).id(id.into())
}

/// A static at `align`: a caption with its string key, or a value its
/// dialog's model shows.
pub(crate) fn static_text(id: u16, key: Option<&str>, align: ShellAlign) -> NodeSpec {
    let node = node(NodeKind::Text, id).with(Prop::Align, Value::Int(align.0.into()));
    match key {
        Some(key) => node.label(key),
        None => node,
    }
}

/// An owner-draw button and its caption key.
pub(crate) fn button(id: u16, key: &str) -> NodeSpec {
    node(NodeKind::Button, id).label(key)
}

/// `0xCF`'s widgets (`0x497`, `0x0078D855..0x0078D91B`): the name limited
/// to [`NAME_LIMIT`] byte characters (`0x4D1(1)`), the text (no
/// `EM_LIMITTEXT`: its handler caps it), the transcript and the buttons,
/// Page `0x5C2` created disabled.
pub(crate) fn page_spec() -> NodeSpec {
    NodeSpec::new(NodeKind::Dialog)
        // SS_RIGHT, then two left-aligned statics and the status.
        .child(static_text(
            NAME_CAPTION,
            Some("GUI:PageBuddy"),
            ShellAlign::H_RIGHT,
        ))
        .child(static_text(
            TEXT_CAPTION,
            Some("GUI:TextToSend"),
            ShellAlign::NONE,
        ))
        .child(static_text(
            STATUS_CAPTION,
            Some("GUI:FindStatus"),
            ShellAlign::NONE,
        ))
        .child(static_text(STATUS, None, ShellAlign::NONE))
        .child(
            button(SEND, "GUI:Page")
                .with(Prop::Enabled, Value::Bool(false))
                .with(Prop::Default, Value::Bool(true)),
        )
        .child(button(CANCEL, "GUI:Cancel"))
        .child(
            node(NodeKind::Edit, NAME)
                .with(Prop::MaxLen, Value::Int(NAME_LIMIT as i32))
                .with(Prop::ByteOnly, Value::Bool(true)),
        )
        .child(node(NodeKind::Edit, TEXT))
        .child(transcript(PAGE_TRANSCRIPT).with(Prop::NoSelectionFill, Value::Bool(true)))
}

/// `0x11E`'s widgets (`0x497`): the reply limited to 120 and empty, Send
/// disabled.
pub(crate) fn received_spec() -> NodeSpec {
    NodeSpec::new(NodeKind::Dialog)
        .child(static_text(
            TITLE_CAPTION,
            Some("GUI:PageReceived"),
            ShellAlign::H_CENTER,
        ))
        .child(static_text(
            REPLY_TO_CAPTION,
            Some("GUI:ReplyTo"),
            ShellAlign::H_RIGHT,
        ))
        .child(static_text(REPLY_TO, None, ShellAlign::NONE))
        .child(
            button(SEND, "GUI:ReplyPage")
                .with(Prop::Enabled, Value::Bool(false))
                .with(Prop::Default, Value::Bool(true)),
        )
        .child(button(BACK, "GUI:Back"))
        // The multiline NewEdit: Enter appends CRLF ([`Prop::Multiline`]).
        .child(
            node(NodeKind::Edit, TEXT)
                .with(Prop::MaxLen, Value::Int(REPLY_LIMIT))
                .with(Prop::Multiline, Value::Bool(true)),
        )
        .child(transcript(RECEIVED_TRANSCRIPT))
}

/// Enter into `dialog`'s widgets; whether it clicked its default button,
/// Send (a disabled one takes no click).
fn enter_clicks_send(c: &mut DialogController, dialog: DialogResourceId) -> bool {
    c.on_input(InputEvent::Key(Key::Enter));
    let send = c.control(dialog, SEND);
    c.drain_events()
        .iter()
        .any(|event| matches!(event, UiEvent::Activated { node } if Some(*node) == send))
}

/// Edit `id` of `dialog` as the NewEdit its getters read.
fn edit(c: &DialogController, dialog: DialogResourceId, id: u16) -> NewEdit {
    let node = c.control(dialog, id);
    node.and_then(|node| c.gui().edit_state(node)).unwrap_or_default()
}

fn set_enabled(c: &mut DialogController, dialog: DialogResourceId, id: u16, enabled: bool) {
    c.set_prop(dialog, id, Prop::Enabled, Value::Bool(enabled));
}

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

/// Dialog `0xCF` while it exists: its rows and status; the rest is its
/// widgets'.
#[derive(Debug, Clone, Default)]
pub(crate) struct WolPageDialog {
    pub(crate) lines: Vec<LanChatLine>,
    /// `0x5C8`: the locate result.
    pub(crate) status: String,
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

/// The `0xCF` handlers on its widgets in `c` ([`PAGE_DIALOG`], the top
/// dialog). `last` is `[0xB77E8C]`, process global and never reset at open.
impl WolPageDialog {
    /// `0x497`'s tail: the name emptied with `0x4B4`, which notifies
    /// nothing. Focus stays on the dialog (`0x00622B50`'s
    /// `SetFocus(hDialog)`) unless the reply window has time left: then the
    /// name is the last pager `[0xB77E30]` (again `0x4B4`), the text edit
    /// takes the focus, whose notification runs the text handler, and the
    /// window restarts with no time.
    pub(crate) fn open(
        c: &mut DialogController,
        window: &mut PageReplyWindow,
        now: i32,
        last_pager: Option<&str>,
        last: &mut u16,
    ) -> (Self, Vec<WolPageAction>) {
        let mut page = Self::default();
        let mut actions = Vec::new();
        if window.remaining(now) > 0 {
            actions = page.address(c, last_pager.unwrap_or_default(), last);
            window.arm(now, 0);
        }
        (page, actions)
    }

    /// The name set with `0x4B4` and the focus moved to the text edit, as
    /// `0x497` addresses the last pager; VERA's player card addresses its
    /// player the same way.
    pub(crate) fn address(
        &mut self,
        c: &mut DialogController,
        name: &str,
        last: &mut u16,
    ) -> Vec<WolPageAction> {
        c.set_text(PAGE_DIALOG, NAME, name);
        self.focus_to(c, Some(TEXT), last)
    }

    fn name_text(c: &DialogController, capacity: usize) -> String {
        edit(c, PAGE_DIALOG, NAME).narrow_text(capacity)
    }

    fn enable_send(c: &mut DialogController) {
        let filled = |id| !edit(c, PAGE_DIALOG, id).units.is_empty();
        let both = filled(NAME) && filled(TEXT);
        set_enabled(c, PAGE_DIALOG, SEND, both);
    }

    /// The name's locate: the status clears.
    fn locate(&mut self, c: &DialogController, capacity: usize, actions: &mut Vec<WolPageAction>) {
        actions.push(WolPageAction::Locate(Self::name_text(c, capacity)));
        self.status.clear();
    }

    /// One `WM_COMMAND` from an edit.
    fn notify(
        &mut self,
        c: &mut DialogController,
        control: u16,
        last: &mut u16,
        actions: &mut Vec<WolPageAction>,
    ) {
        match control {
            // 0x0078D1C0..0x0078D272.
            NAME => {
                Self::enable_send(c);
                *last = NAME;
                self.status.clear();
            }
            // 0x0078D275..0x0078D37C: 0x005EDD00 caps the text first.
            TEXT => {
                let mut text = edit(c, PAGE_DIALOG, TEXT);
                let before = text.units.len();
                apply_chat_length_limit(&mut text);
                if text.units.len() != before {
                    c.set_text(PAGE_DIALOG, TEXT, &text.display_text());
                }
                Self::enable_send(c);
                if *last == NAME {
                    self.locate(c, usize::MAX, actions);
                }
                *last = TEXT;
            }
            _ => {}
        }
    }

    /// The focus moved from `old` to `new` (a press, or `SetFocus`): the old
    /// control's kill notification, then the new one's.
    pub(crate) fn focus_moved(
        &mut self,
        c: &mut DialogController,
        old: Option<u16>,
        new: Option<u16>,
        last: &mut u16,
    ) -> Vec<WolPageAction> {
        let mut actions = Vec::new();
        if old != new {
            for control in [old, new].into_iter().flatten() {
                self.notify(c, control, last, &mut actions);
            }
        }
        actions
    }

    /// `SetFocus` on `target`.
    fn focus_to(
        &mut self,
        c: &mut DialogController,
        target: Option<u16>,
        last: &mut u16,
    ) -> Vec<WolPageAction> {
        let old = c.focused_control(PAGE_DIALOG);
        c.focus_control(PAGE_DIALOG, target);
        self.focus_moved(c, old, target, last)
    }

    /// A press on a transcript row: the list subclass `0x00618D40` calls
    /// `SetFocus(list)`, whose `WM_SETFOCUS` the owner-draw default handler
    /// (`0x006118BF..0x006118DE`) hands straight back to the previous
    /// control. The focused edit keeps the focus and sees its kill and set
    /// notifications.
    pub(crate) fn press_transcript_row(
        &mut self,
        c: &mut DialogController,
        last: &mut u16,
    ) -> Vec<WolPageAction> {
        let mut actions = Vec::new();
        if let Some(edit) = c.focused_control(PAGE_DIALOG) {
            self.notify(c, edit, last, &mut actions);
            self.notify(c, edit, last, &mut actions);
        }
        actions
    }

    /// A key or typed text while an edit has the focus; each admitted
    /// character and each edit notifies. Neither loop runs
    /// `IsDialogMessage`: Escape does nothing.
    pub(crate) fn input(
        &mut self,
        c: &mut DialogController,
        event: InputEvent,
        last: &mut u16,
    ) -> Vec<WolPageAction> {
        let mut actions = Vec::new();
        let Some(control @ (NAME | TEXT)) = c.focused_control(PAGE_DIALOG) else {
            return actions;
        };
        match event {
            // 0x4DA: BM_CLICK on 0x5C2 (the dialog's default button), which
            // a disabled button ignores.
            InputEvent::Key(Key::Enter) => {
                if enter_clicks_send(c, PAGE_DIALOG) {
                    return self.activate(c, SEND, last);
                }
            }
            // 0x4DB (0x0078D93A..0x0078D9FB): 0x00600C70 moves the focus to
            // the other tab stop (the widgets' Tab); from the name it locates
            // again.
            InputEvent::Key(key @ (Key::Tab | Key::BackTab)) => {
                c.on_input(InputEvent::Key(key));
                c.drain_events();
                let to = c.focused_control(PAGE_DIALOG);
                actions = self.focus_moved(c, Some(control), to, last);
                if control == NAME {
                    self.locate(c, NAME_LIMIT + 1, &mut actions);
                    *last = TEXT;
                }
            }
            InputEvent::Key(Key::Escape) => {}
            event => page::edit_input(c, event, |c| self.notify(c, control, last, &mut actions)),
        }
        actions
    }

    /// A button released over itself, or Enter's `BM_CLICK`.
    pub(crate) fn activate(
        &mut self,
        c: &mut DialogController,
        control: u16,
        last: &mut u16,
    ) -> Vec<WolPageAction> {
        let mut actions = Vec::new();
        match control {
            CANCEL => actions.push(WolPageAction::Close),
            // 0x0078D39A..0x0078D626.
            SEND if c.flag(PAGE_DIALOG, SEND, Prop::Enabled, true) => {
                if *last == NAME {
                    self.locate(c, NAME_LIMIT + 1, &mut actions);
                }
                *last = SEND;
                let mut edit = edit(c, PAGE_DIALOG, TEXT);
                apply_chat_length_limit(&mut edit);
                let mut text = edit.wide_text(usize::MAX);
                while matches!(text.last(), Some(10 | 13)) {
                    text.pop();
                }
                c.set_text(PAGE_DIALOG, TEXT, "");
                set_enabled(c, PAGE_DIALOG, SEND, false);
                actions.extend(self.focus_to(c, Some(TEXT), last));
                // `Wstring__truncate` to the limit, by byte character: an
                // addressed name (`0x4B4`) may be longer than typing allows.
                let name = Self::name_text(c, NAME_LIMIT + 1);
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

/// Dialog `0x11E` while it exists: its rows and statics; the reply and the
/// selection are its widgets' ([`RECEIVED_DIALOG`]).
#[derive(Debug, Clone, Default)]
pub(crate) struct WolPageReceived {
    pub(crate) lines: Vec<LanChatLine>,
    /// `0x718`.
    pub(crate) reply_to: String,
    /// `[0xB76E24]`, the selected buddy, as it was at open: only `0x116`
    /// writes it, and nothing under `0x11E` takes input while it exists.
    pub(crate) fallback: String,
    /// Opened by `0x116`'s Page (`0x0079EA23`): modal over the Buddy List
    /// instead of modeless over the empty backdrop.
    pub(crate) modal: bool,
}

impl WolPageReceived {
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
    pub(crate) fn pager_from_list(&self, c: &DialogController) -> String {
        let start = match c.prop(RECEIVED_DIALOG, RECEIVED_TRANSCRIPT, Prop::Selected) {
            Some(Value::Int(row)) => usize::try_from(*row).unwrap_or(0),
            _ => 0,
        };
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

    /// `LBN_SELCHANGE` on `row`: the reply-to follows the selection.
    pub(crate) fn select(&mut self, c: &mut DialogController, row: usize) {
        if row < self.lines.len() {
            c.set_prop(
                RECEIVED_DIALOG,
                RECEIVED_TRANSCRIPT,
                Prop::Selected,
                Value::Int(row as i32),
            );
            self.reply_to = self.pager_from_list(c);
        }
    }

    /// A key or typed text while the reply has the focus.
    pub(crate) fn input(
        &mut self,
        c: &mut DialogController,
        event: InputEvent,
    ) -> Option<WolPageReceivedAction> {
        c.focused_control(RECEIVED_DIALOG)?;
        match event {
            // The multiline NewEdit (the gui's `Multiline`) appends CRLF,
            // keeping the caret, and sends 0x501; a text that is not empty
            // once trimmed clicks Send, the default button, when enabled.
            // The append runs no EN_CHANGE.
            InputEvent::Key(Key::Enter) => enter_clicks_send(c, RECEIVED_DIALOG)
                .then(|| self.activate(c, SEND))
                .flatten(),
            InputEvent::Key(Key::Escape | Key::Tab | Key::BackTab) => None,
            event => {
                // EN_CHANGE (0x300): Send follows the text.
                page::edit_input(c, event, |c| {
                    let filled = !edit(c, RECEIVED_DIALOG, TEXT).units.is_empty();
                    set_enabled(c, RECEIVED_DIALOG, SEND, filled);
                });
                None
            }
        }
    }

    /// Send (`0x0079DB50`'s `0x5C2`) or Back.
    pub(crate) fn activate(
        &mut self,
        c: &mut DialogController,
        control: u16,
    ) -> Option<WolPageReceivedAction> {
        match control {
            BACK => Some(WolPageReceivedAction::Close),
            SEND if c.flag(RECEIVED_DIALOG, SEND, Prop::Enabled, true) => {
                let text = edit(c, RECEIVED_DIALOG, TEXT).wide_text(usize::MAX);
                c.set_text(RECEIVED_DIALOG, TEXT, "");
                let name = self.pager_from_list(c);
                set_enabled(c, RECEIVED_DIALOG, SEND, false);
                (!name.is_empty() && !text.is_empty()).then(|| WolPageReceivedAction::Send {
                    name,
                    text: trim_line_end(&text).to_vec(),
                })
            }
            _ => None,
        }
    }
}

#[cfg(all(test, tests_ui))]
mod tests {
    use super::*;

    const ENTER: InputEvent = InputEvent::Key(Key::Enter);
    const TAB: InputEvent = InputEvent::Key(Key::Tab);

    fn typed(text: &str) -> InputEvent {
        InputEvent::Text(text.into())
    }

    /// A dialog's widgets built in a fresh shell stack.
    fn controller(dialog: DialogResourceId, spec: fn() -> NodeSpec) -> DialogController {
        let mut c = DialogController::default();
        c.push(dialog);
        let root = c.root_in(dialog).unwrap();
        page::ensure_built(c.gui_mut(), root, spec);
        c
    }

    fn page_controller() -> DialogController {
        controller(PAGE_DIALOG, page_spec)
    }

    /// A press on an edit takes the focus.
    fn press(
        page: &mut WolPageDialog,
        c: &mut DialogController,
        id: u16,
        last: &mut u16,
    ) -> Vec<WolPageAction> {
        let old = c.focused_control(PAGE_DIALOG);
        c.focus_control(PAGE_DIALOG, Some(id));
        page.focus_moved(c, old, Some(id), last)
    }

    fn enabled(c: &DialogController, dialog: DialogResourceId, id: u16) -> bool {
        c.flag(dialog, id, Prop::Enabled, true)
    }

    fn text(c: &DialogController, dialog: DialogResourceId, id: u16) -> String {
        c.text(dialog, id)
    }

    #[test]
    fn page_follows_both_fields_and_leaving_the_name_locates_it() {
        let mut c = page_controller();
        let mut last = 0u16;
        let mut page = WolPageDialog::default();
        assert!(page.input(&mut c, typed("x"), &mut last).is_empty());
        assert!(press(&mut page, &mut c, NAME, &mut last).is_empty());
        page.input(&mut c, typed("Tanya123"), &mut last);
        assert!(!enabled(&c, PAGE_DIALOG, SEND) && last == NAME);
        // Clicking into the text field locates the name once.
        assert_eq!(
            press(&mut page, &mut c, TEXT, &mut last),
            vec![WolPageAction::Locate("Tanya123".into())]
        );
        assert!(page.input(&mut c, typed("hi"), &mut last).is_empty());
        assert!(enabled(&c, PAGE_DIALOG, SEND));
        let sent = page.input(&mut c, ENTER, &mut last);
        assert_eq!(
            sent,
            vec![WolPageAction::Send {
                name: "Tanya123".into(),
                text: "hi".encode_utf16().collect()
            }]
        );
        // The text edit already has the focus, so SetFocus notifies nothing.
        assert!(
            text(&c, PAGE_DIALOG, TEXT).is_empty()
                && !enabled(&c, PAGE_DIALOG, SEND)
                && last == SEND
        );
        // A second Enter with Page disabled does nothing.
        assert!(page.input(&mut c, ENTER, &mut last).is_empty());
    }

    /// VERA's player card addresses a page to a name longer than WOL's 9
    /// characters, and Send keeps all of it.
    #[test]
    fn an_addressed_page_sends_a_long_vera_name_whole() {
        let mut c = page_controller();
        let mut last = TEXT;
        let mut page = WolPageDialog::default();
        assert!(
            page.address(&mut c, "Commander_Tanya", &mut last)
                .is_empty()
        );
        assert_eq!(c.focused_control(PAGE_DIALOG), Some(TEXT));
        page.input(&mut c, typed("gg"), &mut last);
        assert_eq!(
            page.input(&mut c, ENTER, &mut last),
            vec![WolPageAction::Send {
                name: "Commander_Tanya".into(),
                text: "gg".encode_utf16().collect()
            }]
        );
    }

    /// A typed name keeps VERA's 24 characters through the edit, both
    /// locates and Send, cut by character: `ø` is two bytes in UTF-8.
    #[test]
    fn a_typed_page_name_keeps_24_characters() {
        let mut c = page_controller();
        let mut last = TEXT;
        let mut page = WolPageDialog::default();
        press(&mut page, &mut c, NAME, &mut last);
        let name = format!("Bjørn{}", "x".repeat(19));
        page.input(&mut c, typed(&format!("{name}y")), &mut last);
        assert_eq!(text(&c, PAGE_DIALOG, NAME), name);
        assert_eq!(
            page.input(&mut c, TAB, &mut last),
            vec![
                WolPageAction::Locate(name.clone()),
                WolPageAction::Locate(name.clone())
            ]
        );
        page.input(&mut c, typed("hi"), &mut last);
        assert_eq!(
            page.input(&mut c, ENTER, &mut last),
            vec![WolPageAction::Send {
                name,
                text: "hi".encode_utf16().collect()
            }]
        );
    }

    #[test]
    fn tab_from_the_name_locates_twice() {
        let mut c = page_controller();
        let mut last = TEXT;
        let mut page = WolPageDialog::default();
        press(&mut page, &mut c, NAME, &mut last);
        page.input(&mut c, typed("Echo"), &mut last);
        let actions = page.input(&mut c, TAB, &mut last);
        assert_eq!(
            actions,
            vec![
                WolPageAction::Locate("Echo".into()),
                WolPageAction::Locate("Echo".into())
            ]
        );
        assert_eq!((c.focused_control(PAGE_DIALOG), last), (Some(TEXT), TEXT));
        // Tab back from the text field only moves the focus.
        assert!(page.input(&mut c, TAB, &mut last).is_empty());
        assert_eq!(c.focused_control(PAGE_DIALOG), Some(NAME));
    }

    #[test]
    fn an_open_inside_the_reply_window_prefills_the_pager_once() {
        let mut window = PageReplyWindow::default();
        let mut last = NAME;
        let open = |window: &mut PageReplyWindow, now, last: &mut u16| {
            let mut c = page_controller();
            let (_, actions) = WolPageDialog::open(&mut c, window, now, Some("Tanya"), last);
            (c, actions)
        };
        // The static initializer's window has no time.
        let (c, actions) = open(&mut window, 50, &mut last);
        assert!(
            actions.is_empty()
                && c.focused_control(PAGE_DIALOG).is_none()
                && text(&c, PAGE_DIALOG, NAME).is_empty()
        );
        window.arm(1_000, REPLY_WINDOW_TICKS);
        let (c, actions) = open(&mut window, 1_299, &mut last);
        assert_eq!(text(&c, PAGE_DIALOG, NAME), "Tanya");
        assert_eq!(c.focused_control(PAGE_DIALOG), Some(TEXT));
        // The text edit's focus notification sees `last == 0x5C1`.
        assert_eq!(actions, vec![WolPageAction::Locate("Tanya".into())]);
        assert_eq!(last, TEXT);
        assert!(!enabled(&c, PAGE_DIALOG, SEND));
        // The prefill used the window up.
        let (c, _) = open(&mut window, 1_300, &mut last);
        assert!(c.focused_control(PAGE_DIALOG).is_none());
        window.arm(1_000, REPLY_WINDOW_TICKS);
        let (c, _) = open(&mut window, 1_300, &mut last);
        assert!(
            c.focused_control(PAGE_DIALOG).is_none(),
            "300 ticks elapsed: no time left"
        );
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
        let mut c = page_controller();
        let mut last = 0u16;
        let mut page = WolPageDialog::default();
        press(&mut page, &mut c, NAME, &mut last);
        page.input(&mut c, typed("Tanya"), &mut last);
        page.status = "Lobby 1".into();
        // The name's kill and set notifications: `last` and the status only.
        assert!(page.press_transcript_row(&mut c, &mut last).is_empty());
        assert_eq!((c.focused_control(PAGE_DIALOG), last), (Some(NAME), NAME));
        assert!(page.status.is_empty());
        page.input(&mut c, typed("x"), &mut last);
        assert_eq!(text(&c, PAGE_DIALOG, NAME), "Tanyax");
        // From the text field the first notification locates once.
        press(&mut page, &mut c, TEXT, &mut last);
        last = NAME;
        assert_eq!(
            page.press_transcript_row(&mut c, &mut last),
            vec![WolPageAction::Locate("Tanyax".into())]
        );
        assert_eq!((c.focused_control(PAGE_DIALOG), last), (Some(TEXT), TEXT));
        // With the dialog itself focused nothing is notified.
        let (mut fresh, mut c) = (WolPageDialog::default(), page_controller());
        assert!(fresh.press_transcript_row(&mut c, &mut last).is_empty());
        assert_eq!(c.focused_control(PAGE_DIALOG), None);
    }

    #[test]
    fn page_received_replies_to_the_first_pager_from_the_selection() {
        let line = |text: &str| LanChatLine {
            text: text.encode_utf16().collect(),
            color: 0xFF_FFFF,
        };
        let mut c = controller(RECEIVED_DIALOG, received_spec);
        let layout = crate::ui::shell::modal::wol_page_received_layout(800, 600);
        let root = c.root_in(RECEIVED_DIALOG).unwrap();
        page::sync_rects(c.gui_mut(), root, |id| layout.rect_of(id));
        let mut received = WolPageReceived::new("Natasha", false);
        assert_eq!(received.pager_from_list(&c), "Natasha");
        received.lines = vec![
            line("(Tanya) first"),
            line("[Me] reply"),
            line("(Boris) second"),
        ];
        page::set(
            c.gui_mut(),
            root,
            RECEIVED_TRANSCRIPT,
            Prop::RowCount,
            Value::Int(3),
        );
        assert_eq!(received.pager_from_list(&c), "Tanya");
        received.select(&mut c, 1);
        assert_eq!(received.reply_to, "Boris");
        c.focus_control(RECEIVED_DIALOG, Some(TEXT));
        received.input(&mut c, typed("o"));
        // Choosing another pager keeps the reply's focus: a press on the
        // transcript's first row.
        let row = layout.rect_of(0x53F).unwrap();
        c.on_pointer_down(row.x + 10, row.y + 6);
        assert_eq!(
            c.prop(RECEIVED_DIALOG, RECEIVED_TRANSCRIPT, Prop::Selected),
            Some(&Value::Int(0))
        );
        received.select(&mut c, 1);
        received.input(&mut c, typed("k"));
        assert!(enabled(&c, RECEIVED_DIALOG, SEND));
        assert_eq!(
            received.input(&mut c, ENTER),
            Some(WolPageReceivedAction::Send {
                name: "Boris".into(),
                text: "ok".encode_utf16().collect()
            })
        );
        assert!(text(&c, RECEIVED_DIALOG, TEXT).is_empty() && !enabled(&c, RECEIVED_DIALOG, SEND));
    }

    /// The reply's Enter appends CRLF wherever the caret is and clicks Send
    /// only for a text that is not empty once trimmed.
    #[test]
    fn a_reply_enter_appends_crlf_and_sends_only_a_trimmed_text() {
        let mut c = controller(RECEIVED_DIALOG, received_spec);
        let mut received = WolPageReceived::new("Boris", false);
        c.focus_control(RECEIVED_DIALOG, Some(TEXT));
        assert_eq!(received.input(&mut c, ENTER), None);
        assert_eq!(text(&c, RECEIVED_DIALOG, TEXT), "\r\n");
        assert!(!enabled(&c, RECEIVED_DIALOG, SEND), "the append is no EN_CHANGE");
        received.input(&mut c, typed("a"));
        assert_eq!(text(&c, RECEIVED_DIALOG, TEXT), "a\r\n", "typed at the kept caret");
        assert_eq!(
            received.input(&mut c, ENTER),
            Some(WolPageReceivedAction::Send {
                name: "Boris".into(),
                text: "a".encode_utf16().collect()
            })
        );
    }
}
