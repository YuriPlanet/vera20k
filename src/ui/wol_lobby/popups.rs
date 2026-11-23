//! The lobby's two popups: Create Game `0xC3` (proc `0x0078DB70`, run by
//! `0x00789B60`) and the channel password `0xE1` (proc `0x0078E030`, run by
//! `0x00789960` for a keyed game), modal dialogs in the lobby's widget tree.
//! Both edit a `ES_PASSWORD` NewEdit limited to 8 units whose text gates OK
//! ([`password_accepted`]); Enter (`0x4DA`) clicks OK, which a disabled OK
//! ignores.

use vera20k_gui::{DialogKind, NodeId, NodeKind, NodeSpec, Prop, Value};

use super::{WolLobbyState, password_accepted};
use crate::ui::room_limits::RoomLimits;
use crate::ui::shell::descriptor::DialogResourceId;
use crate::ui::shell::geom::RectPx;
use crate::ui::shell::modal::WolCreateGameLayout;
use crate::ui::shell::page;

pub(crate) const CREATE_DIALOG: DialogResourceId = DialogResourceId(0x00C3);
pub(crate) const PASSWORD_DIALOG: DialogResourceId = DialogResourceId(0x00E1);

pub(crate) const POPUP_OK: u16 = 1;
pub(crate) const POPUP_CANCEL: u16 = 2;
pub(crate) const POPUP_PASSWORD: u16 = 0x72C;
pub(crate) const CREATE_CLAN: u16 = 0x66C;
pub(crate) const CREATE_LIMIT: u16 = 0x71E;
pub(crate) const CREATE_OBSERVE: u16 = 0x71F;
/// VERA's online popup's Room name edit (no gamemd control).
pub(crate) const CREATE_NAME: u16 = 0x7FF0;
/// VERA's online popup's Ranked game box (no gamemd control).
pub(crate) const CREATE_RANKED: u16 = 0x7FF1;
/// VERA's online popup's Watching box (no gamemd control).
pub(crate) const CREATE_WATCHING: u16 = 0x7FF2;
/// VERA's online popup's Max players trackbar and Allow spectators box.
pub(crate) const CREATE_PLAYERS: u16 = 0x7FE0;
pub(crate) const CREATE_SPECTATORS: u16 = 0x7FE1;
/// VERA's online popup: an online setting's title, which shows its
/// control's help and whose press gives an edit the keys, is the control's
/// id with this bit.
pub(crate) const LABEL: u16 = 0x8000;

/// The popups' statics: the password's caption (`0xE1`'s centred title),
/// VERA's Max players and Room name captions, and `0xC3`'s status line.
pub(crate) const PASSWORD_CAPTION: u16 = 0x7FD0;
pub(crate) const PLAYERS_CAPTION: u16 = 0x7FD1;
pub(crate) const NAME_CAPTION: u16 = 0x7FD2;
pub(crate) const STATUS: u16 = 0x695;
/// `0xC3`'s boxes and their captions; VERA's online popup shows each
/// caption as a title left of its box ([`TITLE`] + the box's index) and
/// the box's value beside it.
pub(crate) const BOXES: [(u16, &str); 6] = [
    (CREATE_CLAN, "GUI:BattleClanTournament"),
    (CREATE_LIMIT, "WOL:LimitScreenResolution"),
    (CREATE_OBSERVE, "GUI:Observe"),
    (CREATE_SPECTATORS, "VERA:AllowSpectators"),
    (CREATE_RANKED, "VERA:RankedGame"),
    (CREATE_WATCHING, "VERA:Watching"),
];
pub(crate) const TITLE: u16 = 0x7FD8;

/// The Room name edit's limit: a nickname's 24, in units below `0x100`.
const ROOM_NAME_LIMIT: i32 = 24;

/// `EM_LIMITTEXT 8` at both procs' `0x497`.
const PASSWORD_LIMIT: usize = 8;

/// Executed `0x6040B0` help of `0xC3`; `0xE1` has none. VERA's Ranked game
/// box says when the lobby ranks a room's games, and its Watching box when
/// others may watch them.
pub(crate) fn create_tooltip_key(control: u16) -> Option<&'static str> {
    Some(match control & !LABEL {
        POPUP_OK => "STT:WOLNewGameOK",
        POPUP_CANCEL => "STT:WOLNewGameCancel",
        CREATE_CLAN => "STT:WOLNewGameClanTournament",
        CREATE_LIMIT => "STT:WOLLimitScreenResolution",
        POPUP_PASSWORD => "STT:WOLNewGamePassword",
        CREATE_RANKED => "VERA:RankedGameHelp",
        CREATE_WATCHING => "VERA:WatchingHelp",
        _ => return None,
    })
}

/// VERA's online popup's Watching: whether players in the online lobby may
/// watch the room's games, and how far behind its players. The lobby still
/// lets everyone watch Quick Match and ranked games 2 minutes behind, and
/// nobody a room with a password (docs/lobby-protocol.md "Watching").
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RoomWatching {
    /// Everyone, 2 minutes behind the players.
    Delayed,
    Off,
    /// Everyone, with no delay. A host starts with this choice.
    #[default]
    Live,
}

impl RoomWatching {
    /// The value a press on the box shows next.
    fn next(self) -> Self {
        match self {
            Self::Delayed => Self::Off,
            Self::Off => Self::Live,
            Self::Live => Self::Delayed,
        }
    }

    pub(crate) fn label_key(self) -> &'static str {
        match self {
            Self::Delayed => "VERA:WatchingDelayed",
            Self::Off => "VERA:WatchingOff",
            Self::Live => "VERA:WatchingLive",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum WolPopupAction {
    /// `0xC3` OK: `0xB77DC4 = 7` and the dialog object's bytes.
    CreateConfirmed {
        password: String,
        clan: bool,
        limit: bool,
        observe: bool,
        /// VERA's online popup's Max players and Allow spectators.
        room: Option<RoomLimits>,
        /// VERA's online popup's Room name, trimmed and not empty.
        name: Option<String>,
        /// VERA's online popup's Ranked game.
        ranked: Option<bool>,
        /// VERA's online popup's Watching.
        watching: Option<RoomWatching>,
    },
    CreateCancelled,
    PasswordConfirmed(String),
    PasswordCancelled,
}

/// The open popup: its modal dialog in the lobby's tree, which holds every
/// value it shows, and for `0xC3` VERA's three-way Watching.
/// `CreateGameDialogControl` (vtable `0x7F788C`): +0x11 clan, +0x12 limit,
/// +0x13 observe; +0x10 belongs to control `0x629`, absent from the
/// template, and stays 0.
#[derive(Debug)]
pub(crate) struct WolLobbyPopup {
    pub(crate) dialog: vera20k_gui::DialogId,
    pub(crate) root: NodeId,
    /// `0xC3` (else `0xE1`); VERA's online popup keeps its Watching here.
    pub(crate) create: Option<Option<RoomWatching>>,
}

impl WolLobbyPopup {
    pub(crate) fn dialog(&self) -> DialogResourceId {
        if self.create.is_some() {
            CREATE_DIALOG
        } else {
            PASSWORD_DIALOG
        }
    }

    pub(crate) fn watching(&self) -> Option<RoomWatching> {
        self.create.flatten()
    }
}

/// The popup's widgets: OK and Cancel, the password NewEdit and for `0xC3`
/// its boxes (toggled on the press); VERA's online popup adds its room's
/// limits, name, Ranked game and Watching, and its settings' titles. The
/// statics come last: they take no pointer.
fn popup_spec(create: bool, room: Option<RoomLimits>) -> NodeSpec {
    let online = room.is_some();
    use crate::ui::shell::geom::ShellAlign;
    // A greyed caption takes 0x612F5F's colour, as the buttons' do.
    let grey = Value::Rgba(vera20k_gui::Rgba(0x48, 0, 0, 0xFF));
    let caption = |id: u16| {
        BOXES
            .iter()
            .find(|(known, _)| *known == id)
            .map_or("", |(_, key)| key)
    };
    let checkbox = |id: u16| {
        NodeSpec::new(NodeKind::Checkbox)
            .id(id.into())
            .label(caption(id))
            .with(Prop::DisabledColor, grey.clone())
    };
    let button = |id: u16, key: &str| NodeSpec::new(NodeKind::Button).id(id.into()).label(key);
    let field = if online {
        ShellAlign::NONE
    } else {
        ShellAlign::H_RIGHT
    };
    let static_text = crate::ui::wol_page::static_text;
    let edit = |id: u16, limit: i32| {
        // Tab moves the focus; the caret keeps the chat edit's blink phase.
        NodeSpec::new(NodeKind::Edit)
            .id(id.into())
            .with(Prop::MaxLen, Value::Int(limit))
            .with(Prop::KeepsBlink, Value::Bool(true))
    };
    let mut spec = NodeSpec::new(NodeKind::Dialog)
        .child(button(POPUP_OK, "GUI:OK").with(Prop::Default, Value::Bool(true)))
        .child(button(POPUP_CANCEL, "GUI:Cancel"))
        .child(edit(POPUP_PASSWORD, PASSWORD_LIMIT as i32).with(Prop::Password, Value::Bool(true)));
    if create {
        spec = spec.children([CREATE_CLAN, CREATE_LIMIT, CREATE_OBSERVE].map(checkbox));
    }
    if let Some(limits) = room.filter(|_| create) {
        // VERA's lobby refuses a room name with C1 controls too.
        let c1: String = (0x7F..=0x9F).filter_map(char::from_u32).collect();
        spec = spec
            .child(
                edit(CREATE_NAME, ROOM_NAME_LIMIT)
                    .with(Prop::ByteOnly, Value::Bool(true))
                    .with(Prop::Rejected, Value::Text(c1)),
            )
            .children([CREATE_RANKED, CREATE_WATCHING].map(checkbox))
            .children(crate::ui::room_limits::widgets(
                limits,
                CREATE_PLAYERS,
                CREATE_SPECTATORS,
            ))
            .children(
                [
                    CREATE_CLAN,
                    CREATE_LIMIT,
                    CREATE_OBSERVE,
                    POPUP_PASSWORD,
                    CREATE_NAME,
                    CREATE_RANKED,
                    CREATE_WATCHING,
                ]
                .map(|id| {
                    let label = NodeSpec::new(NodeKind::Text)
                        .id((id | LABEL).into())
                        .tooltip(" ");
                    // A press on an edit's title gives the edit the keys.
                    match id {
                        POPUP_PASSWORD | CREATE_NAME => {
                            label.with(Prop::FocusTarget, Value::Int(id.into()))
                        }
                        _ => label,
                    }
                }),
            )
            .child(static_text(PLAYERS_CAPTION, Some("VERA:MaxPlayers"), field))
            .child(static_text(NAME_CAPTION, Some("VERA:RoomName"), field))
            .children(BOXES.iter().zip(TITLE..).map(|((_, key), id)| {
                static_text(id, Some(key), field).with(Prop::DisabledColor, grey.clone())
            }));
    }
    match create {
        true => spec
            .child(static_text(PASSWORD_CAPTION, Some("GUI:Password"), field))
            .child(static_text(STATUS, None, ShellAlign::NONE)),
        // Style SS_CENTER.
        false => spec.child(static_text(
            PASSWORD_CAPTION,
            Some("GUI:ChannelPassword"),
            ShellAlign::H_CENTER,
        )),
    }
}

/// Where popup control `id` sits on `0xC3`. An online setting's title spans
/// from its caption to the end of its control (the controls come first in
/// the tree, so they keep their own presses).
pub(crate) fn create_rect(layout: &WolCreateGameLayout, id: u16) -> Option<RectPx> {
    if let Some((control, _)) = id
        .checked_sub(TITLE)
        .and_then(|index| BOXES.get(usize::from(index)))
    {
        return create_rect(layout, *control).map(|control| layout.setting_label(control));
    }
    let control = match id & !LABEL {
        PASSWORD_CAPTION => return Some(layout.password_label),
        PLAYERS_CAPTION => return layout.room.map(|room| room.players_label),
        NAME_CAPTION => return layout.name.map(|(caption, _)| caption),
        STATUS => return Some(layout.status),
        POPUP_OK => Some(layout.ok),
        POPUP_CANCEL => Some(layout.cancel),
        POPUP_PASSWORD => Some(layout.password),
        CREATE_CLAN => layout.clan,
        CREATE_LIMIT => Some(layout.limit),
        CREATE_OBSERVE => layout.observe,
        CREATE_NAME => layout.name.map(|(_, rect)| rect),
        CREATE_RANKED => layout.ranked,
        CREATE_WATCHING => layout.watching,
        CREATE_PLAYERS => layout.room.map(|room| room.players),
        CREATE_SPECTATORS => layout.room.map(|room| room.spectators),
        _ => None,
    }?;
    if id & LABEL == 0 {
        return Some(control);
    }
    let label = layout.setting_label(control);
    Some(RectPx::new(
        label.x,
        control.y,
        control.x + control.w - label.x,
        control.h,
    ))
}

impl WolLobbyState {
    fn push_popup(&mut self, create: Option<Option<RoomWatching>>, room: Option<RoomLimits>) {
        self.close_popup();
        let Ok(root) = self.gui.create(NodeKind::Dialog, None) else {
            return;
        };
        page::ensure_built(&mut self.gui, root, || popup_spec(create.is_some(), room));
        let Ok(dialog) = self.gui.push_dialog(root, DialogKind::Modal) else {
            return;
        };
        self.gui.set_dialog_scale(dialog, self.scale);
        self.popup = Some(WolLobbyPopup {
            dialog,
            root,
            create,
        });
    }

    /// `0x0078DB70`'s `0x497`. `screen_width` is Options' ScreenWidth
    /// (`[0xA8EB84]`); `limit` the process byte `[0xA8B540]`, already cleared
    /// by `0x00789B60` at 1024 or wider; `clan_id` `[0xB77DE4]`; `room` the
    /// host's last limits, room name, Ranked game and Watching in VERA's
    /// online lobby. Room name takes the keys first.
    pub(crate) fn open_create(
        &mut self,
        screen_width: i32,
        limit: bool,
        clan_id: i32,
        observe: bool,
        room: Option<(RoomLimits, &str, bool, RoomWatching)>,
    ) {
        let narrow = screen_width < 1024;
        let watching = room.map(|(.., watching)| watching);
        self.push_popup(Some(watching), room.map(|(limits, ..)| limits));
        for (id, checked, enabled) in [
            (CREATE_CLAN, false, clan_id >= 1),
            (CREATE_LIMIT, limit && narrow, narrow),
            (CREATE_OBSERVE, observe, true),
        ] {
            self.set(id, Prop::Checked, Value::Bool(checked));
            self.set(id, Prop::Enabled, Value::Bool(enabled));
        }
        if let Some((_, name, ranked, watching)) = room {
            self.set(CREATE_RANKED, Prop::Checked, Value::Bool(ranked));
            self.set(
                CREATE_WATCHING,
                Prop::Checked,
                Value::Bool(watching != RoomWatching::Off),
            );
            let mut edit = vera20k_gui::shell::new_edit::NewEdit::default();
            edit.set_units_limited(name.encode_utf16(), ROOM_NAME_LIMIT as usize);
            self.store_edit(CREATE_NAME, &edit);
        }
        self.focus_popup_edit(if room.is_some() {
            CREATE_NAME
        } else {
            POPUP_PASSWORD
        });
        self.gate_popup_ok();
    }

    pub(crate) fn open_password(&mut self) {
        self.push_popup(None, None);
        self.focus_popup_edit(POPUP_PASSWORD);
        self.gate_popup_ok();
    }

    pub(crate) fn close_popup(&mut self) {
        if let Some(popup) = self.popup.take() {
            let _ = self.gui.pop_dialog(popup.dialog);
        }
    }

    /// Puts the popup's controls on its layout.
    pub(crate) fn sync_popup(&mut self, rect_of: impl Fn(u16) -> Option<RectPx>) {
        if let Some(popup) = &self.popup {
            let root = popup.root;
            page::sync_rects(&mut self.gui, root, rect_of);
        }
    }

    pub(crate) fn focus_popup_edit(&mut self, id: u16) {
        let Some(node) = self
            .popup
            .as_ref()
            .and_then(|popup| self.gui.find_id(popup.root, id.into()))
        else {
            return;
        };
        // The keys move; the caret keeps the chat edit's blink phase.
        let _ = self.gui.set_focus(Some(node));
    }

    /// Box `id` is checked in the popup.
    pub(crate) fn popup_checked(&self, id: u16) -> bool {
        self.popup.is_some() && self.flag(id, Prop::Checked) == Some(true)
    }

    /// VERA's Max players and Allow spectators as the popup shows them.
    pub(crate) fn room_limits(&self) -> Option<RoomLimits> {
        let players = self
            .popup
            .as_ref()
            .and(self.int(CREATE_PLAYERS, Prop::Pos))?;
        Some(RoomLimits {
            players: players as u8,
            spectators: self.popup_checked(CREATE_SPECTATORS),
        })
    }

    /// The Room name as OK sends it: trimmed, `None` when empty.
    fn room_name(&self) -> Option<String> {
        self.node(CREATE_NAME)?;
        let text = self.edit(CREATE_NAME).display_text();
        Some(text.trim().to_owned()).filter(|text| !text.is_empty())
    }

    /// The EN_CHANGE gate of OK; VERA's Room name must not be blank.
    pub(crate) fn ok_enabled(&self) -> bool {
        (self.node(CREATE_NAME).is_none() || self.room_name().is_some())
            && password_accepted(&self.edit(POPUP_PASSWORD).units)
    }

    pub(crate) fn popup_enabled(&self, control: u16) -> bool {
        self.popup.is_some()
            && self.node(control).is_some()
            && self.flag(control, Prop::Enabled) != Some(false)
    }

    pub(super) fn gate_popup_ok(&mut self) {
        if self.popup.is_some() {
            let ok = self.ok_enabled();
            self.set(POPUP_OK, Prop::Enabled, Value::Bool(ok));
        }
    }

    /// VERA's Watching box steps to its next value on each press.
    pub(super) fn step_watching(&mut self) {
        let Some(Some(Some(watching))) = self.popup.as_mut().map(|popup| popup.create.as_mut())
        else {
            return;
        };
        *watching = watching.next();
        let on = *watching != RoomWatching::Off;
        self.set(CREATE_WATCHING, Prop::Checked, Value::Bool(on));
    }

    /// OK or Cancel released (or Enter's click on OK).
    pub(crate) fn popup_activate(&mut self, control: u16) -> Option<WolPopupAction> {
        let popup = self.popup.as_ref()?;
        if !self.popup_enabled(control) || (control == POPUP_OK && !self.ok_enabled()) {
            return None;
        }
        let password = self.edit(POPUP_PASSWORD).narrow_text(PASSWORD_LIMIT + 1);
        let watching = popup.watching();
        Some(match (popup.create.is_some(), control) {
            (true, POPUP_OK) => WolPopupAction::CreateConfirmed {
                password,
                clan: self.popup_checked(CREATE_CLAN),
                limit: self.popup_checked(CREATE_LIMIT),
                observe: self.popup_checked(CREATE_OBSERVE),
                room: self.room_limits(),
                name: self.room_name(),
                ranked: self
                    .node(CREATE_RANKED)
                    .map(|_| self.popup_checked(CREATE_RANKED)),
                watching,
            },
            (true, POPUP_CANCEL) => WolPopupAction::CreateCancelled,
            (false, POPUP_OK) => WolPopupAction::PasswordConfirmed(password),
            (false, POPUP_CANCEL) => WolPopupAction::PasswordCancelled,
            _ => return None,
        })
    }
}

#[cfg(all(test, tests_ui))]
mod tests {
    use super::*;
    use crate::ui::wol_lobby::WolLobbyEffect;
    use vera20k_gui::Key;

    /// A lobby with `0xC3` open on the 800x600 box.
    fn create(
        screen_width: i32,
        limit: bool,
        clan_id: i32,
        observe: bool,
        room: Option<(RoomLimits, &str, bool, RoomWatching)>,
    ) -> WolLobbyState {
        let mut lobby = WolLobbyState::new(false, room.is_some());
        lobby.open_create(screen_width, limit, clan_id, observe, room);
        let layout = crate::ui::shell::modal::wol_create_game_layout(800, 600, room.is_some());
        lobby.sync_popup(|id| create_rect(&layout, id));
        lobby
    }

    /// A press on box `id`'s icon; whether it changed what the box shows.
    fn toggle(lobby: &mut WolLobbyState, id: u16) -> bool {
        let shown = |lobby: &WolLobbyState| {
            (
                lobby.popup_checked(id),
                lobby.popup.as_ref().and_then(WolLobbyPopup::watching),
            )
        };
        let before = shown(lobby);
        if let Some(rect) = lobby
            .node(id)
            .and_then(|node| lobby.gui.tree().screen_rect(node).ok())
        {
            lobby.pointer(rect.x + 2, rect.y + 2, Some(true));
            lobby.pointer(rect.x + 2, rect.y + 2, Some(false));
        }
        shown(lobby) != before
    }

    fn enter(lobby: &mut WolLobbyState) -> Option<WolPopupAction> {
        lobby
            .key(Some(Key::Enter), None)
            .into_iter()
            .find_map(|effect| match effect {
                WolLobbyEffect::Action(super::super::WolLobbyAction::Popup(action)) => Some(action),
                _ => None,
            })
    }

    #[test]
    fn create_popup_initializes_from_width_clan_and_preferences() {
        let popup = create(800, true, 0, true, None);
        assert!(
            popup.popup_checked(CREATE_LIMIT)
                && popup.popup_enabled(CREATE_LIMIT)
                && popup.popup_checked(CREATE_OBSERVE)
        );
        assert!(!popup.popup_checked(CREATE_CLAN) && !popup.popup_enabled(CREATE_CLAN));
        let mut wide = create(1024, true, 3, false, None);
        assert!(!wide.popup_enabled(CREATE_LIMIT) && wide.popup_enabled(CREATE_CLAN));
        assert!(!wide.popup_checked(CREATE_LIMIT));
        // VERA's online popup carries its room's limits to OK.
        let limits = RoomLimits {
            players: 5,
            spectators: false,
        };
        let mut modern = create(
            1024,
            false,
            0,
            false,
            Some((limits, "Boris's game", true, RoomWatching::Delayed)),
        );
        assert!(matches!(
            modern.popup_activate(POPUP_OK),
            Some(WolPopupAction::CreateConfirmed {
                room: Some(room),
                ranked: Some(true),
                watching: Some(RoomWatching::Delayed),
                ..
            }) if room == limits
        ));
        // Its Ranked game box toggles; the native popup has none.
        assert!(toggle(&mut modern, CREATE_RANKED));
        assert!(matches!(
            modern.popup_activate(POPUP_OK),
            Some(WolPopupAction::CreateConfirmed {
                ranked: Some(false),
                ..
            })
        ));
        assert!(!toggle(&mut wide, CREATE_RANKED));
    }

    #[test]
    fn the_password_gates_ok_and_enter_clicks_it() {
        let mut popup = create(800, false, 0, false, None);
        assert!(toggle(&mut popup, CREATE_LIMIT) && !toggle(&mut popup, CREATE_CLAN));
        popup.key(None, Some("ab c"));
        assert!(!popup.ok_enabled());
        assert_eq!(enter(&mut popup), None);
        popup.key(Some(Key::Left), None);
        popup.key(Some(Key::Backspace), None);
        popup.key(None, Some("123456789"));
        assert_eq!(popup.edit(POPUP_PASSWORD).units.len(), 8);
        assert_eq!(
            enter(&mut popup),
            Some(WolPopupAction::CreateConfirmed {
                password: "ab12345c".into(),
                clan: false,
                limit: true,
                observe: false,
                room: None,
                name: None,
                ranked: None,
                watching: None,
            })
        );
        let mut password = WolLobbyState::new(false, false);
        password.open_password();
        password.key(None, Some("s;"));
        assert_eq!(password.popup_activate(POPUP_OK), None);
        assert_eq!(
            password.popup_activate(POPUP_CANCEL),
            Some(WolPopupAction::PasswordCancelled)
        );
    }

    /// VERA's Room name takes the keys first and Tab hands them to the
    /// password; OK sends it trimmed and stays off while it is blank.
    #[test]
    fn the_room_name_takes_keys_first_and_gates_ok() {
        let mut popup = create(
            1024,
            false,
            0,
            false,
            Some((
                RoomLimits::default(),
                "Boris's game",
                true,
                RoomWatching::Delayed,
            )),
        );
        for _ in 0.."Boris's game".len() {
            popup.key(Some(Key::Backspace), None);
        }
        assert!(!popup.ok_enabled());
        popup.key(None, Some("  No rush \u{4e2d}"));
        popup.key(Some(Key::Tab), None);
        popup.key(None, Some("pw"));
        let Some(WolPopupAction::CreateConfirmed { password, name, .. }) =
            popup.popup_activate(POPUP_OK)
        else {
            panic!("OK");
        };
        assert_eq!(
            (password.as_str(), name.as_deref()),
            ("pw", Some("No rush"))
        );
    }

    /// A fresh host can confirm Live immediately or cycle through every
    /// Watching choice. The native popup has none.
    #[test]
    fn watching_steps_through_its_three_values() {
        let room = (
            RoomLimits::default(),
            "Boris's game",
            true,
            RoomWatching::default(),
        );
        let mut popup = create(1024, false, 0, false, Some(room));
        let sent = |popup: &mut WolLobbyState| match popup.popup_activate(POPUP_OK) {
            Some(WolPopupAction::CreateConfirmed { watching, .. }) => watching,
            other => panic!("{other:?}"),
        };
        use RoomWatching::*;
        assert_eq!(sent(&mut popup), Some(Live));
        for choice in [Delayed, Off, Live] {
            assert!(toggle(&mut popup, CREATE_WATCHING));
            assert_eq!(sent(&mut popup), Some(choice));
        }
        let mut native = create(1024, false, 0, false, None);
        assert!(!toggle(&mut native, CREATE_WATCHING));
        assert_eq!(sent(&mut native), None);
    }
}
