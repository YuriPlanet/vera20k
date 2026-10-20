//! The WOL service boundary and the login flow's reducer
//! (`WOL__RunLoginFlow` `0x00787010`, server list `0x00797640`, connect
//! `0x007864C0`, box `0x00786EF0`).
//!
//! Requests are semantic intents, events their decoded results; neither is a
//! packet or a COM call. A controlled service drains [`WolVisit::take_service_requests`]
//! and answers through [`service_event`]; production answers synchronously
//! from [`WolProvider::Unavailable`]. No request ever manufactures success.

use std::time::Instant;

use super::{WolBoxThen, WolPage, WolVisit};
use crate::app::{App, AppState};
use crate::ui::wol_chat::WolChatLine;
use crate::ui::wol_lobby::{WolChannel, WolMember};
use crate::ui::wol_login::WolCredential;
use crate::ui::wol_shell::{
    BOX_NO, BOX_OK, BOX_YES, WAIT_CHECK_STATUS, WAIT_DISCONNECT, WolBoxButtons, WolMessageBox,
    WolMode, WolWaitBox,
};

/// Which WOLAPI stands behind the visit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum WolProvider {
    /// No Westwood online components (every retail install today): the
    /// WOLAPI object cannot be created (`0x00786390` -> `0x00785711`).
    Unavailable,
    /// A diagnostics/test service drains requests and injects events.
    Controlled,
    /// VERA's offline stand-in service ([`super::test_service`]), chosen by
    /// the Options page's developer box.
    TestService,
}

/// One server record (`0xF8` bytes; IRC type at +0x5F). `name` is the text
/// at +0x18; its display name is the part after the first ':'.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct WolServer {
    pub(crate) name: String,
}

/// The server sink's IRC records in their native order and the record its
/// preferred-server selection left in `[0xB76468]`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct WolServerList {
    pub(crate) servers: Vec<WolServer>,
    pub(crate) selected: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum WolApiStatus {
    Available,
    /// `TXT_APIMISSING`: missing or invalid.
    Missing,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct WolLoginAccepted {
    /// The message of the day the login sink stores in `[0xB73AE0]`.
    pub(crate) motd: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum WolServiceRequest {
    /// `WOL__EnsureWolapiObject` `0x00786390`.
    EnsureApi,
    /// Read the credential store (API+0x90, slots 1..32).
    ListCredentials,
    /// `0x00797640`: API+0x10 with these SKU flags (`0x2900`, or `0x2300`
    /// for mode 3).
    FetchServers { flags: u32 },
    /// `0xC4`'s Disconnect during the connect: `SetEvent(event 0)` ends the
    /// running wait. A Disconnect during the server list leaves WOL, which
    /// ends the visit and with it the service session; nothing is queued.
    CancelWait,
    /// `0x007864C0`: API+0x14 with the selected record, the nickname and
    /// password copied into it (+0xE4/+0xEE) and the changed byte.
    Connect {
        server: WolServer,
        nickname: String,
        password: String,
        changed: bool,
    },
    /// Forget Account: API+0x94(slot, "", "", 1).
    ForgetCredential { slot: u32 },
    /// Remember after a login: `0x00789500(nickname, password, changed)`.
    StoreCredential {
        nickname: String,
        password: String,
        changed: bool,
    },
    /// `0x0077F120`: API+0x18 for channel type `kind` (0 chat lobbies,
    /// `0x29` games) with `flags`.
    ListChannels { kind: u32, flags: u32 },
    /// `0x00788800`: API+0x20 with the record and its key at `+0xD7`.
    JoinChannel { channel: WolChannel, key: String },
    /// API+0x24: leave the current channel.
    LeaveChannel,
    /// `0x00789B60`: API+0x1C with the new game channel's record and its
    /// key at `+0xD7`.
    CreateChannel { channel: WolChannel, key: String },
    /// `0x0077E1D0`, the pager `0x0078A6B0`/`0x0078A8F0` or the locate
    /// `0x0078A470` from the lobby; `to` are the selected users of a
    /// private line.
    Chat { line: WolChatLine, to: Vec<String> },
    /// `0x0077E300`: an action line, private to `to` when any.
    Emote { text: Vec<u16>, to: Vec<String> },
    /// `0x0077E110`: API+0x7C with `[WOnline] LangFilter`.
    SetLangFilter(i32),
    /// `0x0077E110`: API+0x68 with AllowFind and AllowPage.
    SetFindPage { find: i32, page: i32 },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum WolServiceEvent {
    Api(WolApiStatus),
    /// The credential store's slots.
    Credentials(Vec<WolCredential>),
    /// The NetStatus sink `0x007AA6D0`. During the server-list wait
    /// (`0x00797669` holds `[0xB77DC0]` at 1) it is event 9's status
    /// `[0xB77DE8]`; while connected, [`NET_STATUS_LOST`] is a lost
    /// connection.
    NetStatus(u32),
    /// Event 1 (server sink `0x007A41C0`), or its negative result (event 0
    /// with `TXT_NO_SERV_LIST`).
    ServerList(Result<WolServerList, ()>),
    /// Event 8: the patch download finished (`0x00797640` returns 2). Only a
    /// patching service sends it; the capture provider has no patch.
    #[cfg_attr(not(test), allow(dead_code))]
    Interrupted,
    /// The login sink `0x007A5290`: 0 with the MOTD, or the result code.
    Login(Result<WolLoginAccepted, u32>),
    /// The channel-list sink: every record of one requested type.
    Channels {
        kind: u32,
        channels: Vec<WolChannel>,
    },
    /// The chat sink's OnJoin `0x007A5E40` for this user: the joined
    /// channel with its users, or the result code.
    Joined(Result<super::lobby::WolJoined, u32>),
    /// OnJoin for another user of the current channel.
    UserJoined(WolMember),
    /// OnPart of another user (by name).
    UserLeft(String),
    /// OnPart for this user: `[0xB77DD0]` cleared.
    ChannelLeft,
    /// The create result and the channel's users.
    Created(Result<Vec<WolMember>, u32>),
    /// The page sinks `0x007A9200`/`0x007A9550`: sender and text.
    Page {
        from: WolMember,
        text: Vec<u16>,
    },
    /// A chat or action line from another user: the chat sinks
    /// `0x007A8DA0` (public), `0x007A8FC0` (private), `0x007AB170` (public
    /// action), `0x007AAFB0` (private action) and their UTF-16 forms.
    Message {
        from: WolMember,
        text: Vec<u16>,
        private: bool,
        action: bool,
    },
    /// The find sink `0x007A9970`: the channel the located user is in, or
    /// the result code.
    Located(Result<WolChannel, u32>),
    /// A user's ladder rank (`CNetUtilEventSink+0x10`, the table
    /// `[0xB77E0C]`).
    LadderRank {
        name: String,
        rank: i32,
    },
    /// A user's locale (`CChatEventSink+0x84`/`+0x8C`, the table
    /// `0xB76530`).
    UserLocale {
        name: String,
        locale: i32,
    },
    /// The kick sink `0x007AB670`: `user` (flagged
    /// [`crate::ui::wol_lobby::MEMBER_LOCAL`] for this user) was kicked out
    /// of `channel` by `by`.
    Kicked {
        channel: WolChannel,
        user: WolMember,
        by: WolMember,
    },
    /// The ban sink `0x007AB3C0`: `name` banned from, or unbanned in, the
    /// current channel.
    ChannelBan {
        name: String,
        banned: bool,
    },
}

/// The server status that completes the list wait with event 1, and that
/// means a lost connection outside it.
const SERVER_STATUS_READY: u32 = 0x0004_012F;
pub(crate) const NET_STATUS_LOST: u32 = SERVER_STATUS_READY;

/// What the flow waits for.
#[derive(Debug, Clone, Default)]
pub(super) enum Awaiting {
    #[default]
    Nothing,
    Api,
    /// Both event 1 and event 9 = `0x4012F` must arrive, in either order.
    Servers {
        ready: bool,
        list: Option<WolServerList>,
    },
    Connect,
}

/// A step the app takes after the reducer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum WolStep {
    Stay,
    /// Open the wait box with this text.
    Wait(&'static str),
    /// Close the wait box and show this box.
    Message {
        key: &'static str,
        yes_no: bool,
        then: WolBoxThen,
    },
    /// `WOL_Main` returns this result.
    Leave(i32),
    /// The login flow returned 0.
    Welcome,
    OpenLogin,
    Accepted,
}

/// `0x007A5290`: the CSF text a login result leaves in `[0xB77AD0]`.
pub(super) fn login_error_key(code: u32) -> &'static str {
    match code {
        0x8004_0064 => "TXT_LOGIN_USED",
        0x8004_0065 => "TXT_BADPASS",
        0x8004_0072 | 0x8004_0075 => "TXT_BANNED",
        0x8004_0074 => "TXT_DISABLED",
        0x8004_0076 => "TXT_SERIALDUP",
        0x8004_0077 => "TXT_SERIALUNKNOWN",
        0x8004_0078 => "TXT_SKUSERIALMISMATCH",
        _ => "TXT_CANT_CONNECT",
    }
}

/// `0x00786EF0`: these results ask Yes/No in `0x126`; Yes opens an
/// `apiregister.westwood.com` page.
pub(super) fn login_error_asks(code: u32) -> bool {
    matches!(
        code,
        0x8004_0072 | 0x8004_0074 | 0x8004_0075 | 0x8004_0076 | 0x8004_0077 | 0x8004_0078
    )
}

/// Event 9 statuses that show `TXT_CANT_CONNECT` and end the list wait.
fn server_status_fails(status: u32) -> bool {
    matches!(status, 0x8004_0067..=0x8004_006A | 0x8004_006C..=0x8004_006D)
}

impl WolVisit {
    /// `0x00786EF0` over `[0xB77AD0]`: an empty text shows nothing
    /// (`0x005E2CD0` returns on length 0); either way the flow returns 0.
    fn failed_login_box(&self) -> WolStep {
        match self.last_error {
            Some(key) => WolStep::Message {
                key,
                yes_no: self.last_login_result.is_some_and(login_error_asks),
                then: WolBoxThen::Welcome,
            },
            None => WolStep::Welcome,
        }
    }

    /// The record `0x00787010` connects to (`0x0078753D..0x007875F8`):
    /// with no preferred-server profile, `[0xB76468]` when it is a valid
    /// record 1..count-1, else record 1 when there are two or more.
    pub(super) fn connect_server(&self) -> Option<WolServer> {
        let count = self.servers.len();
        let index = if (1..count).contains(&self.selected_server) {
            self.selected_server
        } else if count > 1 {
            1
        } else {
            return None;
        };
        self.servers.get(index).cloned()
    }

    /// Reduce one service event. Events nothing waits for are stale and do
    /// nothing, except the credential store, which is always current.
    pub(super) fn reduce(&mut self, event: WolServiceEvent) -> WolStep {
        let awaiting = std::mem::take(&mut self.awaiting);
        match (awaiting, event) {
            (awaiting, WolServiceEvent::Credentials(credentials)) => {
                self.credentials = credentials;
                self.awaiting = awaiting;
                WolStep::Stay
            }
            (Awaiting::Api, WolServiceEvent::Api(WolApiStatus::Missing)) => WolStep::Message {
                key: "TXT_APIMISSING",
                yes_no: false,
                then: WolBoxThen::Leave(-2),
            },
            (Awaiting::Api, WolServiceEvent::Api(WolApiStatus::Available)) => {
                if self.mode != Some(WolMode::CustomMatch) {
                    log::warn!(
                        "WOL mode {:?} after WOLAPI creation is not implemented",
                        self.mode
                    );
                    return WolStep::Welcome;
                }
                // 0x00786390 creates the object once per visit ([0xB77E75]);
                // only then does it reset the WOL globals (0x00780110),
                // including [0xB77AD0], the selected record and the ladder
                // and locale tables, and set [0xB77DC0] (0x00786485) until a
                // connect clears it.
                if !self.api {
                    self.api = true;
                    self.last_error = None;
                    self.selected_server = 0;
                    self.ladder_ranks.clear();
                    self.locales.clear();
                    self.connected = false;
                }
                self.requests.push(WolServiceRequest::ListCredentials);
                self.requests
                    .push(WolServiceRequest::FetchServers { flags: 0x2900 });
                self.awaiting = Awaiting::Servers {
                    ready: false,
                    list: None,
                };
                WolStep::Wait("TXT_FETCHING_SERVLIST")
            }
            (Awaiting::Servers { ready, list }, WolServiceEvent::NetStatus(status)) => {
                if server_status_fails(status) {
                    // The box shows from inside 0x00797640, which returns 1.
                    return WolStep::Message {
                        key: "TXT_CANT_CONNECT",
                        yes_no: false,
                        then: WolBoxThen::Leave(-2),
                    };
                }
                self.servers_arrived(ready || status == SERVER_STATUS_READY, list)
            }
            (Awaiting::Servers { ready, .. }, WolServiceEvent::ServerList(Ok(list))) => {
                self.servers_arrived(ready, Some(list))
            }
            (Awaiting::Servers { .. }, WolServiceEvent::ServerList(Err(()))) => {
                // Event 0 with TXT_NO_SERV_LIST in [0xB77AD0], not shown: the
                // flow returns -2 without a box.
                self.last_error = Some("TXT_NO_SERV_LIST");
                WolStep::Leave(-2)
            }
            (Awaiting::Servers { .. }, WolServiceEvent::Interrupted) => WolStep::Leave(-1),
            (Awaiting::Connect, WolServiceEvent::Login(Ok(accepted))) => {
                self.motd = Some(accepted.motd);
                self.authenticated = true;
                // The connect clears [0xB77DC0] (0x007867E6) and the login
                // sink [0xB77DC2] (0x007A559D).
                self.connected = true;
                self.disconnected = false;
                WolStep::Accepted
            }
            (Awaiting::Connect, WolServiceEvent::Login(Err(code))) => {
                // The login sink sets [0xB77DC0] (0x007A52A0).
                self.connected = false;
                self.last_login_result = Some(code);
                self.last_error = Some(login_error_key(code));
                self.failed_login_box()
            }
            (awaiting, event) => {
                log::debug!("stale WOL service event {event:?}");
                self.awaiting = awaiting;
                WolStep::Stay
            }
        }
    }

    fn servers_arrived(&mut self, ready: bool, list: Option<WolServerList>) -> WolStep {
        match (ready, list) {
            (true, Some(list)) => {
                self.servers = list.servers;
                self.selected_server = list.selected;
                WolStep::OpenLogin
            }
            (ready, list) => {
                self.awaiting = Awaiting::Servers { ready, list };
                WolStep::Stay
            }
        }
    }

    /// `0xC4`'s Disconnect while it shows.
    pub(super) fn disconnect(&mut self) -> WolStep {
        match std::mem::take(&mut self.awaiting) {
            // 0x00797640 returns 1: the login flow returns -2.
            Awaiting::Servers { .. } => WolStep::Leave(-2),
            // 0x007864C0 fails silently; 0x00786EF0 shows [0xB77AD0].
            Awaiting::Connect => {
                self.requests.push(WolServiceRequest::CancelWait);
                self.failed_login_box()
            }
            other => {
                self.awaiting = other;
                WolStep::Stay
            }
        }
    }
}

pub(super) fn resolve(state: &AppState, key: &str) -> String {
    App::csf_label(state, key, key)
}

/// Production's synchronous answers: no request reaches a service, and the
/// only answer is that WOLAPI is missing.
fn answer_unavailable(state: &mut AppState) {
    let Some(visit) = state.frontend.wol.as_mut() else {
        return;
    };
    if visit.provider != WolProvider::Unavailable {
        return;
    }
    for request in visit.take_service_requests() {
        if request == WolServiceRequest::EnsureApi {
            service_event(state, WolServiceEvent::Api(WolApiStatus::Missing));
        }
    }
}

/// `0x00787010` (and `0x00799D30` for My Information) start by creating the
/// WOLAPI object.
pub(super) fn begin_flow(state: &mut AppState) {
    let Some(visit) = state.frontend.wol.as_mut() else {
        return;
    };
    visit.awaiting = Awaiting::Api;
    visit.requests.push(WolServiceRequest::EnsureApi);
    answer_unavailable(state);
}

/// Queue a request from the login page (credential store changes).
pub(super) fn request(state: &mut AppState, request: WolServiceRequest) {
    if let Some(visit) = state.frontend.wol.as_mut() {
        visit.requests.push(request);
    }
    answer_unavailable(state);
}

/// Start the connect after `0x10F` closed with Login.
pub(super) fn begin_connect(state: &mut AppState) {
    let globals = state.frontend.wol_account.clone();
    let Some(visit) = state.frontend.wol.as_mut() else {
        return;
    };
    let step = match visit.connect_server() {
        Some(server) => {
            visit.requests.push(WolServiceRequest::Connect {
                server,
                nickname: globals.nickname,
                password: globals.password,
                changed: globals.changed,
            });
            visit.awaiting = Awaiting::Connect;
            WolStep::Wait("TXT_CONNECTING_SERVER")
        }
        None => visit.failed_login_box(),
    };
    apply(state, step);
    answer_unavailable(state);
}

/// Deliver one service event to the visit. Only declared events move the
/// flow; a stale one changes nothing.
pub(crate) fn service_event(state: &mut AppState, event: WolServiceEvent) {
    if state.frontend.wol.is_none() {
        return;
    }
    let event = match event {
        WolServiceEvent::Page { from, text } => {
            super::lobby::page_received(state, from, text);
            return;
        }
        WolServiceEvent::Message {
            from,
            text,
            private,
            action,
        } => {
            super::lobby::message_received(state, from, text, private, action);
            return;
        }
        WolServiceEvent::Located(result) => {
            super::lobby::located(state, result);
            return;
        }
        WolServiceEvent::NetStatus(status)
            if !matches!(
                state.frontend.wol.as_ref().expect("checked").awaiting,
                Awaiting::Servers { .. }
            ) =>
        {
            super::lobby::net_status(state, status);
            return;
        }
        WolServiceEvent::Kicked { channel, user, by } => {
            super::lobby::kicked(state, channel, user, by);
            return;
        }
        WolServiceEvent::ChannelBan { name, banned } => {
            super::lobby::channel_ban(state, &name, banned);
            return;
        }
        WolServiceEvent::LadderRank { name, rank } => {
            let visit = state.frontend.wol.as_mut().expect("checked");
            visit.ladder_ranks.insert(name.to_ascii_lowercase(), rank);
            return;
        }
        WolServiceEvent::UserLocale { name, locale } => {
            let visit = state.frontend.wol.as_mut().expect("checked");
            visit.locales.insert(name.to_ascii_lowercase(), locale);
            return;
        }
        event @ (WolServiceEvent::Channels { .. }
        | WolServiceEvent::Joined(_)
        | WolServiceEvent::UserJoined(_)
        | WolServiceEvent::UserLeft(_)
        | WolServiceEvent::ChannelLeft
        | WolServiceEvent::Created(_)) => {
            super::lobby::service_event(state, event);
            answer_unavailable(state);
            return;
        }
        event => event,
    };
    let visit = state.frontend.wol.as_mut().expect("checked");
    let step = visit.reduce(event);
    apply(state, step);
    answer_unavailable(state);
    state.platform.window.request_redraw();
}

pub(super) fn wait_box(state: &AppState, key: &str) -> WolWaitBox {
    WolWaitBox {
        text: resolve(state, key),
        disconnect: resolve(state, "GUI:Disconnect"),
        check_status: resolve(state, "GUI:WOLCheckStatus"),
        opened_at: Instant::now(),
        shown: false,
    }
}

/// `0x005E2CD0`'s box over the shell: `0xD0` (OK) or `0x126` (Yes/No).
pub(super) fn message_box(state: &AppState, key: &str, yes_no: bool) -> WolMessageBox {
    message_box_text(state, resolve(state, key), yes_no)
}

/// The same box with an already formatted body.
pub(super) fn message_box_text(state: &AppState, body: String, yes_no: bool) -> WolMessageBox {
    let buttons = if yes_no {
        WolBoxButtons::YesNo {
            yes: resolve(state, "GUI:Yes"),
            no: resolve(state, "GUI:No"),
        }
    } else {
        WolBoxButtons::Ok(resolve(state, "GUI:OK"))
    };
    WolMessageBox { body, buttons }
}

fn apply(state: &mut AppState, step: WolStep) {
    match step {
        WolStep::Stay => {}
        WolStep::Wait(key) => {
            // 0x00792BE0 with an open box only replaces its text.
            let wait = wait_box(state, key);
            let visit = state.frontend.wol.as_mut().expect("WOL visit");
            match &mut visit.page {
                WolPage::Backdrop { wait: Some(open) } => open.text = wait.text,
                page => *page = WolPage::Backdrop { wait: Some(wait) },
            }
            state
                .frontend
                .shell_controller
                .reset_to(crate::ui::wol_shell::WAIT_DIALOG, false);
        }
        WolStep::Message { key, yes_no, then } => {
            let message = message_box(state, key, yes_no);
            let dialog = message.dialog();
            let visit = state.frontend.wol.as_mut().expect("WOL visit");
            visit.status_help = None;
            visit.page = WolPage::Message { message, then };
            state.frontend.shell_controller.reset_to(dialog, false);
        }
        WolStep::Leave(result) => {
            // -1 (the patch download) takes PrepareSession's 0x0052E817
            // branch, which is not ported; both return to a new 0xE2.
            if result == -1 {
                log::warn!("WOL_Main returned -1; its PrepareSession branch is not ported");
            }
            App::return_from_wol(state);
        }
        WolStep::Welcome => App::reopen_wol_welcome(state),
        WolStep::OpenLogin => super::login::open(state),
        WolStep::Accepted => accept_login(state),
    }
}

/// `0x007864C0`'s success continuation and the rest of `0x00787010`.
fn accept_login(state: &mut AppState) {
    // ReadMultiPlayerSettings, then the nickname becomes the session handle
    // [0xA8B380] and WriteMultiPlayerSettings 0x006990A0 saves it.
    App::prepare_multiplayer_settings(
        state,
        crate::app::shell_main_menu::MultiplayerSettingsContext::Shell,
    );
    let globals = state.frontend.wol_account.clone();
    let runtime = &mut state.frontend.multiplayer_shell_runtime;
    runtime.set_handle(&globals.nickname);
    runtime.persist_local_preferences();
    // Remember with registry bit 1 clear: 0x00789500 stores the account and
    // the settings are written again (0x00787614..0x0078763E).
    if globals.remember {
        request(
            state,
            WolServiceRequest::StoreCredential {
                nickname: globals.nickname,
                password: globals.password,
                changed: globals.changed,
            },
        );
        state
            .frontend
            .multiplayer_shell_runtime
            .persist_local_preferences();
    }
    // wolinfoMD.ini is absent from retail: no auto-login query 0x112.
    // A re-login on runner entry (0x007879B6) continues that runner;
    // otherwise the login flow returned 1 to WOL_Main, whose mode-loop pass
    // applies before the runner.
    match state
        .frontend
        .wol
        .as_mut()
        .and_then(|visit| visit.relogin.take())
    {
        Some(postmatch) => super::open_lobby(state, postmatch),
        None => {
            super::apply_wol_preferences(state);
            super::open_lobby(state, false);
        }
    }
}

/// A box or wait-box button released.
pub(super) fn box_command(state: &mut AppState, control: u16) {
    let Some(visit) = state.frontend.wol.as_mut() else {
        return;
    };
    if matches!(visit.page, WolPage::Lobby) {
        super::lobby::box_command(state, control);
        answer_unavailable(state);
        state.platform.window.request_redraw();
        return;
    }
    let then = match &visit.page {
        WolPage::Message { then, .. } => Some(*then),
        _ => None,
    };
    let backdrop = matches!(visit.page, WolPage::Backdrop { .. });
    let step = match (then, control) {
        (Some(then), BOX_OK | BOX_YES | BOX_NO) => {
            if control == BOX_YES {
                // Yes opens an apiregister.westwood.com page through
                // WOL__LaunchBrowser; VERA has no browser.
                log::warn!("WOL account page needs a browser; not opened");
            }
            match then {
                WolBoxThen::Leave(result) => WolStep::Leave(result),
                WolBoxThen::Welcome => WolStep::Welcome,
            }
        }
        (None, WAIT_DISCONNECT) if backdrop => visit.disconnect(),
        (None, WAIT_CHECK_STATUS) if backdrop => {
            // 0x00773E90 opens the embedded status browser.
            log::warn!("WOL Check Status needs the embedded browser; not opened");
            WolStep::Stay
        }
        _ => WolStep::Stay,
    };
    apply(state, step);
    answer_unavailable(state);
    state.platform.window.request_redraw();
}

/// The wait box's reveal timer; returns its deadline while it runs.
pub(super) fn poll_wait(state: &mut AppState) -> Option<Instant> {
    let visit = state.frontend.wol.as_mut()?;
    let WolPage::Backdrop { wait: Some(wait) } = &mut visit.page else {
        return None;
    };
    if wait.poll(Instant::now()) {
        state.platform.window.request_redraw();
        return None;
    }
    (!wait.shown).then(|| wait.reveal_at())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn visit() -> WolVisit {
        let mut visit = WolVisit::open(false);
        visit.mode = Some(WolMode::CustomMatch);
        visit.provider = WolProvider::Controlled;
        visit.awaiting = Awaiting::Api;
        visit
    }

    fn servers(count: usize, selected: usize) -> WolServerList {
        WolServerList {
            servers: (0..count)
                .map(|index| WolServer {
                    name: format!("irc:Server{index}"),
                })
                .collect(),
            selected,
        }
    }

    #[test]
    fn login_results_map_to_the_sink_texts() {
        assert_eq!(login_error_key(0x8004_0064), "TXT_LOGIN_USED");
        assert_eq!(login_error_key(0x8004_0065), "TXT_BADPASS");
        assert_eq!(login_error_key(0x8004_0072), "TXT_BANNED");
        assert_eq!(login_error_key(0x8004_0075), "TXT_BANNED");
        assert_eq!(login_error_key(0x8004_0074), "TXT_DISABLED");
        assert_eq!(login_error_key(0x8004_0076), "TXT_SERIALDUP");
        assert_eq!(login_error_key(0x8004_0077), "TXT_SERIALUNKNOWN");
        assert_eq!(login_error_key(0x8004_0078), "TXT_SKUSERIALMISMATCH");
        assert_eq!(login_error_key(0x8004_0073), "TXT_CANT_CONNECT");
        assert_eq!(login_error_key(0x8000_4005), "TXT_CANT_CONNECT");
        assert_eq!(login_error_key(1), "TXT_CANT_CONNECT");
        assert!(!login_error_asks(0x8004_0064) && !login_error_asks(0x8004_0065));
        assert!(!login_error_asks(0x8004_0073));
        for code in [
            0x8004_0072,
            0x8004_0074,
            0x8004_0075,
            0x8004_0076,
            0x8004_0077,
            0x8004_0078,
        ] {
            assert!(login_error_asks(code), "{code:#x}");
        }
    }

    #[test]
    fn missing_api_shows_its_box_and_then_leaves() {
        let mut visit = visit();
        assert_eq!(
            visit.reduce(WolServiceEvent::Api(WolApiStatus::Missing)),
            WolStep::Message {
                key: "TXT_APIMISSING",
                yes_no: false,
                then: WolBoxThen::Leave(-2)
            }
        );
        assert!(visit.requests.is_empty());
    }

    #[test]
    fn server_list_needs_both_events_in_either_order() {
        let mut visit = visit();
        assert!(!visit.api);
        assert_eq!(
            visit.reduce(WolServiceEvent::Api(WolApiStatus::Available)),
            WolStep::Wait("TXT_FETCHING_SERVLIST")
        );
        // [0xAC116C] stays set for the rest of the visit (0x0077E110's gate).
        assert!(visit.api);
        assert_eq!(
            visit.take_service_requests(),
            [
                WolServiceRequest::ListCredentials,
                WolServiceRequest::FetchServers { flags: 0x2900 }
            ]
        );
        assert_eq!(
            visit.reduce(WolServiceEvent::ServerList(Ok(servers(3, 2)))),
            WolStep::Stay
        );
        // Other statuses are ignored.
        assert_eq!(
            visit.reduce(WolServiceEvent::NetStatus(0x8004_0066)),
            WolStep::Stay
        );
        assert_eq!(
            visit.reduce(WolServiceEvent::NetStatus(SERVER_STATUS_READY)),
            WolStep::OpenLogin
        );
        assert_eq!(visit.connect_server().unwrap().name, "irc:Server2");

        let mut visit = visit_waiting_servers();
        assert_eq!(
            visit.reduce(WolServiceEvent::NetStatus(SERVER_STATUS_READY)),
            WolStep::Stay
        );
        assert_eq!(
            visit.reduce(WolServiceEvent::ServerList(Ok(servers(2, 0)))),
            WolStep::OpenLogin
        );
        // Record 0 is never the connect target; record 1 is the fallback.
        assert_eq!(visit.connect_server().unwrap().name, "irc:Server1");
    }

    fn visit_waiting_servers() -> WolVisit {
        let mut visit = visit();
        visit.reduce(WolServiceEvent::Api(WolApiStatus::Available));
        visit.take_service_requests();
        visit
    }

    #[test]
    fn server_list_failures_leave_wol() {
        let mut visit = visit_waiting_servers();
        assert_eq!(
            visit.reduce(WolServiceEvent::ServerList(Err(()))),
            WolStep::Leave(-2)
        );
        let mut visit = visit_waiting_servers();
        assert_eq!(
            visit.reduce(WolServiceEvent::Interrupted),
            WolStep::Leave(-1)
        );
        let mut visit = visit_waiting_servers();
        assert_eq!(
            visit.reduce(WolServiceEvent::NetStatus(0x8004_0068)),
            WolStep::Message {
                key: "TXT_CANT_CONNECT",
                yes_no: false,
                then: WolBoxThen::Leave(-2)
            }
        );
        let mut visit = visit_waiting_servers();
        assert_eq!(visit.disconnect(), WolStep::Leave(-2));
        assert!(visit.take_service_requests().is_empty());
        // A late list after the cancel is stale.
        assert_eq!(
            visit.reduce(WolServiceEvent::ServerList(Ok(servers(2, 1)))),
            WolStep::Stay
        );
    }

    #[test]
    fn a_single_record_list_cannot_connect() {
        let mut visit = visit_waiting_servers();
        visit.reduce(WolServiceEvent::ServerList(Ok(servers(1, 0))));
        visit.reduce(WolServiceEvent::NetStatus(SERVER_STATUS_READY));
        assert_eq!(visit.connect_server(), None);
        // [0xB77AD0] is empty after 0x00780110: no box, the welcome again.
        assert_eq!(visit.failed_login_box(), WolStep::Welcome);
    }

    #[test]
    fn login_results_lead_to_the_lobby_or_a_box_then_the_welcome() {
        let mut visit = visit_waiting_servers();
        visit.awaiting = Awaiting::Connect;
        assert_eq!(
            visit.reduce(WolServiceEvent::Login(Err(0x8004_0065))),
            WolStep::Message {
                key: "TXT_BADPASS",
                yes_no: false,
                then: WolBoxThen::Welcome
            }
        );
        assert!(!visit.authenticated);
        visit.awaiting = Awaiting::Connect;
        assert_eq!(
            visit.reduce(WolServiceEvent::Login(Err(0x8004_0076))),
            WolStep::Message {
                key: "TXT_SERIALDUP",
                yes_no: true,
                then: WolBoxThen::Welcome
            }
        );
        visit.awaiting = Awaiting::Connect;
        assert_eq!(visit.disconnect(), visit.failed_login_box());
        visit.awaiting = Awaiting::Connect;
        assert_eq!(
            visit.reduce(WolServiceEvent::Login(Ok(WolLoginAccepted {
                motd: "Welcome".into()
            }))),
            WolStep::Accepted
        );
        assert!(visit.authenticated);
        assert_eq!(visit.motd(), Some("Welcome"));
        // Nothing waits any more.
        assert_eq!(
            visit.reduce(WolServiceEvent::Login(Err(0x8004_0065))),
            WolStep::Stay
        );
    }

    #[test]
    fn credentials_are_kept_whatever_the_flow_waits_for() {
        let mut visit = visit_waiting_servers();
        let stored = vec![WolCredential {
            slot: 1,
            nickname: "Alpha".into(),
            password: "12345678".into(),
        }];
        assert_eq!(
            visit.reduce(WolServiceEvent::Credentials(stored.clone())),
            WolStep::Stay
        );
        assert_eq!(visit.credentials, stored);
        assert!(matches!(visit.awaiting, Awaiting::Servers { .. }));
    }
}
