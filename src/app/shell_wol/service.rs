//! The WOL service boundary and the login flow's reducer
//! (`WOL__RunLoginFlow` `0x00787010`, server list `0x00797640`, connect
//! `0x007864C0`, box `0x00786EF0`).
//!
//! Requests are semantic intents, events their decoded results; neither is a
//! packet or a COM call. A controlled service drains [`WolVisit::take_service_requests`]
//! and answers through [`service_event`]; production answers synchronously
//! from [`WolProvider::Unavailable`]. No request ever manufactures success.

use crate::util::time::{Duration, Instant};

use super::{WolBoxThen, WolPage, WolVisit};
use crate::app::persistence::wol_profile::{WolInfoFile, WolPersona};
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
    /// VERA's authenticated HTTPS lobby and UDP relay.
    Online,
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

impl WolServer {
    /// The text after the first ':' (`strchr` at `0x00787220`/`0x0078756E`);
    /// none without a ':' or with nothing after it.
    fn display_name(&self) -> Option<&str> {
        self.name
            .split_once(':')
            .map(|(_, rest)| rest)
            .filter(|rest| !rest.is_empty())
    }

    /// `0x0078686E..0x007868B5`: the name with everything through the
    /// first ':' removed, as the persona stores it.
    pub(super) fn persona_server(&self) -> &str {
        self.name
            .split_once(':')
            .map_or(&self.name, |(_, rest)| rest)
    }
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

#[derive(Clone, PartialEq, Eq)]
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
    /// Remember after a login: `0x00789500(nickname, password, changed,
    /// locale)`, which stores the account (API+0x94) and the slot's locale
    /// (API+0xC8, `0x00789610..0x0078961E`).
    StoreCredential {
        nickname: String,
        password: String,
        changed: bool,
        locale: i32,
    },
    /// API+0xBC: the locales of these users (the connect asks for its own,
    /// `0x00786A01..0x00786A48`).
    RequestLocales { names: Vec<String> },
    /// API+0xB8: this user's locale.
    SetLocale(i32),
    /// API+0xD0: how many locales the service knows (`0xFE`'s init,
    /// `0x00796C9C`).
    LocaleCount,
    /// `0x0077F120`: API+0x18 for channel type `kind` (0 chat lobbies,
    /// `0x29` games) with `flags`.
    ListChannels { kind: u32, flags: u32 },
    /// `0x00788800`: API+0x20 with the record and its key at `+0xD7`.
    JoinChannel { channel: WolChannel, key: String },
    /// API+0x24: leave the current channel.
    LeaveChannel,
    /// VERA HTTPS cleanup intent when the visit returns while its live service
    /// is borrowed by a callback. The same service owns the match/finish ID.
    MatchReturned,
    /// API+0x34 from the logout `0x00785EA0`.
    Logout,
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
    /// API+0xB0: a squad's record; 0 asks for the user's own (the lobby
    /// runner's entry `0x00787B33..0x00787B53`, a joiner's at
    /// `0x007A62B7..0x007A62EE`). The answer is [`WolServiceEvent::Squad`].
    SquadInfo { id: i32 },
    /// API+0xDC: the buddy list (`0x116`'s init, the lobby runner's entry
    /// `0x00787BC8`); the answer is [`WolServiceEvent::Buddies`].
    ListBuddies,
    /// API+0xE0 with the name (at most 9 bytes at `+0x24`): Add Buddy
    /// `0x0079DFE0`. Its result sink `+0x98` ignores the answer.
    AddBuddy(String),
    /// API+0xE4: Remove `0x0079CBA0`; its sink `+0x9C` ignores the answer.
    RemoveBuddy(String),
    /// API+0x60 for one buddy of the locate queue `0xB76980`; the find sink
    /// answers with [`WolServiceEvent::Located`].
    Locate(String),
    /// The ladder API `[0xAC1170]` vt+0x14 for the names (SKU
    /// `[0xB77A90]`); the answer is [`WolServiceEvent::Ladder`].
    LadderRecords(Vec<String>),
    /// The ladder API vt+0x38 for the names; the answer is
    /// [`WolServiceEvent::HighScores`].
    HighScores(Vec<String>),
    /// `0x007A0690`: API+0x30, the private ANSI text
    /// [`super::quick_match::MatchRequest::text`] to the Quick Match lobby's
    /// matchbot. The service fills the transport fields
    /// ([`super::quick_match::MatchTransport`]).
    RequestMatch {
        matchbot: String,
        request: super::quick_match::MatchRequest,
    },
    /// VERA's chat translation of a received line into `[VERA]
    /// TranslateChat=`'s language; [`super::translation`] shows the reply.
    Translate {
        text: Vec<u16>,
        line: super::translation::TranslatedLine,
    },
}

impl std::fmt::Debug for WolServiceRequest {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WolServiceRequest")
            .field("kind", &std::mem::discriminant(self))
            .finish_non_exhaustive()
    }
}

/// One ladder record (`0x6C` bytes) of the ladder sink
/// `CNetUtilEventSink+0x10` (`0x007A3100`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct WolLadderRecord {
    pub(crate) name: String,
    /// `+0x08`.
    pub(crate) wins: i32,
    /// `+0x0C`.
    pub(crate) losses: i32,
    /// `+0x10`.
    pub(crate) points: i32,
    /// `+0x1C`.
    pub(crate) rank: i32,
    /// `+0x20`.
    pub(crate) disconnects: i32,
}

/// One high-score record (`0x44` bytes) of `CNetUtilEventSink+0x24`
/// (`0x007A3B30`); the player list compares its fields unsigned.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct WolHighScoreRecord {
    /// `+0x1C`.
    pub(crate) name: String,
    /// `+0x0C`: the badge's points.
    pub(crate) points: u32,
    /// `+0x10`: the rank the digits show.
    pub(crate) rank: u32,
    /// `+0x14`: one bit per finished co-op campaign.
    pub(crate) coop: u32,
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
    /// The chat sink's OnJoin `0x007A5890` for this user: the joined
    /// channel with its users, or the result code.
    Joined(Result<super::lobby::WolJoined, u32>),
    /// OnJoin for another user of the current channel.
    UserJoined(WolMember),
    /// OnPart of another user (by name).
    UserLeft(String),
    /// OnPart for this user: `[0xB77DD0]` cleared.
    ChannelLeft,
    /// The private game-options sink `+0x54` (`0x007A8260`): a guest's
    /// `R` choices, decoded (the observer flag is country -3).
    PlayerChoices {
        from: String,
        slot: crate::skirmish_launch::SkirmishLocalSlot,
        observer: bool,
    },
    /// The public game-options sink `+0x58` (`0x007A7EA0`): a user's
    /// `A<n>` readiness.
    PlayerReady {
        from: String,
        value: i32,
    },
    /// The same sink's `K<n>` map status (`0x005E4C39`): `K1` 1, `K2` 2,
    /// anything else 0.
    MapStatus {
        from: String,
        status: u8,
    },
    /// The squad sink `+0x80` (`0x007A4EC0`): the id it reports and the
    /// record, or the failure.
    Squad {
        id: i32,
        squad: Result<crate::ui::wol_lobby::WolSquad, u32>,
    },
    /// The private sink's `P<scenario>` from a guest (`0x007A8BF3`).
    #[expect(
        dead_code,
        reason = "a WOL backend delivers it; the test service's guest always has the map"
    )]
    PreviewRequested {
        from: String,
    },
    /// The create result and the channel's users.
    Created(Result<Vec<WolMember>, u32>),
    /// The page sinks: `ansi` for `0x007A9200`, otherwise the UTF-16
    /// `0x007A9550`, which gamemd's own pages (API+0xF8 with UTF-16 text)
    /// most likely reach.
    Page {
        from: WolMember,
        text: Vec<u16>,
        ansi: bool,
    },
    /// The page-send result sink `CChatEventSink+0x6C` (`0x007A9800`).
    PageResult(Result<(), u32>),
    /// A chat or action line from another user: the chat sinks
    /// `0x007A8DA0` (public), `0x007A8FC0` (private), `0x007AB170` (public
    /// action), `0x007AAFB0` (private action) and, without `ansi`, their
    /// UTF-16 forms (`0x007A8ED0`, `0x007A9110`, `0x007AB260`,
    /// `0x007AB0A0`). A matchbot's private `Start <options>:<players>`
    /// carries its options decoded (`0x005E3D10`'s default arm, as for a
    /// host's options).
    Message {
        from: WolMember,
        text: Vec<u16>,
        private: bool,
        action: bool,
        ansi: bool,
        options: Option<Box<crate::app::shell_network_setup::HostOptions>>,
    },
    /// The find sink `0x007A9970`: the channel the located user is in, or
    /// the result code.
    Located(Result<WolChannel, u32>),
    /// The ladder sink `CNetUtilEventSink+0x10` (`0x007A3100`): records
    /// for the table `[0xB77E0C]`.
    Ladder(Vec<WolLadderRecord>),
    /// The high-score sink `CNetUtilEventSink+0x24` (`0x007A3B30`): records
    /// for the table `0xB76640`, or a failed result (only logged).
    HighScores(Result<Vec<WolHighScoreRecord>, u32>),
    /// The buddy-list sink `CChatEventSink+0x94` (`0x007AC220`): the
    /// users of the list, or a negative result.
    Buddies(Result<Vec<String>, u32>),
    /// `CChatEventSink+0x84` (`0x007AB8E0`): the users' locales for the
    /// table `0xB76530`, or a failed result (only logged).
    UserLocales(Result<Vec<(String, i32)>, u32>),
    /// API+0xD0's answer: the locale count `0xFE` lists.
    LocaleCount(i32),
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
    /// `0x007864C0`'s wait: 100 ms polls with a 30 000 ms budget
    /// (`0x007865A4..0x007865CC`); running out fails silently.
    Connect {
        deadline: Instant,
    },
    /// The locale wait after the connect (`0x00786A4E..0x00786AB8`): event
    /// 14 or the same 30 s end it, and neither fails the login.
    Locale {
        deadline: Instant,
    },
}

/// The connect and locale waits' silent limit (`MOV EDI,0x7530`).
const WAIT_LIMIT: Duration = Duration::from_millis(30_000);

/// The login flow `0x00787010` between the connect it starts and its
/// return.
#[derive(Debug, Clone)]
pub(super) struct LoginFlow {
    /// The silent connect with `[DefaultPersona] AutoLogin`
    /// (`0x0078710B..0x00787336`): a success returns at once, without
    /// Remember or `0x112`; a failure goes on to `0x10F`.
    pub(super) auto: bool,
    /// The record the connect uses; modes 4 and 5 save its name as the
    /// persona's Server (`0x0078686E..0x007868F9`).
    pub(super) server: WolServer,
    /// The locale of the persona the dialog path loaded before the connect
    /// (`0x007874E2`): Remember passes it (`0x00787637..0x00787660`).
    pub(super) remember_locale: i32,
    /// The persona the connect's continuation loaded (`0x00786821`), held
    /// through the locale step.
    pub(super) persona: Option<WolPersona>,
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
    /// Both server-list events arrived: the flow goes on past `0x00797640`.
    ServersReady,
    /// A box closed and `0x10F` opens.
    OpenLogin,
    /// Event 2: the connect's continuation runs.
    Accepted,
    /// The silent auto-login connect failed (`0x00787350`).
    AutoLoginFailed,
    /// The locale wait ended.
    LocaleDecided,
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
    /// `0x00787010`'s entry: the last login result `[0xB77DF8]` is zeroed
    /// (`0x0078701E`) before the WOLAPI object is created.
    fn begin_login(&mut self) {
        self.last_login_result = None;
        self.ensure_api();
    }

    /// `0x00786390`, which answers with [`WolServiceEvent::Api`].
    fn ensure_api(&mut self) {
        self.awaiting = Awaiting::Api;
        self.requests.push(WolServiceRequest::EnsureApi);
    }

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

    /// The record whose display name is the persona's Server, from record 1
    /// (`0x00787202..0x00787246`, `0x00787557..0x0078759B`; `0x007B4710` is
    /// an exact `strcmp`).
    fn preferred_server(&self, persona_server: &str) -> Option<usize> {
        if persona_server.is_empty() {
            return None;
        }
        (1..self.servers.len())
            .find(|&index| self.servers[index].display_name() == Some(persona_server))
    }

    /// The dialog path's record (`0x0078752D..0x007875F8`): the persona's
    /// server when listed, else `[0xB76468]`; an index past the list
    /// becomes -1, and -1 falls back to record 1 when there are two or more.
    /// Index 0, and a one-record list, connect to nothing.
    pub(super) fn dialog_server(&mut self, persona_server: &str) -> Option<WolServer> {
        let count = self.servers.len() as i32;
        if let Some(index) = self.preferred_server(persona_server) {
            self.selected_server = index as i32;
        }
        if self.selected_server >= count {
            self.selected_server = -1;
        }
        if self.selected_server == -1 {
            if count <= 1 {
                return None;
            }
            self.selected_server = 1;
        }
        if self.selected_server < 1 || self.selected_server >= count {
            return None;
        }
        self.servers.get(self.selected_server as usize).cloned()
    }

    /// The silent path's index: the record matching the persona's server.
    /// Without one, modes 1-3 keep `[0xB76468]` (`0x007871D1..0x0078725D`);
    /// the other modes keep it only when the persona has no server, and an
    /// unlisted one gives -1, no connect and no box (`0x00787262..0x007872F4`).
    fn auto_login_server(&self, persona_server: &str) -> i32 {
        match self.preferred_server(persona_server) {
            Some(index) => index as i32,
            None if persona_server.is_empty()
                || matches!(self.mode.map(WolMode::native), Some(1..=3)) =>
            {
                self.selected_server
            }
            None => -1,
        }
    }

    /// A connect failed (an error result, the Disconnect button or the
    /// silent limit): the dialog path shows `[0xB77AD0]` and returns 0;
    /// the silent path goes on to `0x10F`.
    fn connect_failed(&mut self) -> WolStep {
        match self.flow.take() {
            Some(flow) if flow.auto => WolStep::AutoLoginFailed,
            _ => self.failed_login_box(),
        }
    }

    /// Reduce one service event. Events nothing waits for are stale and do
    /// nothing, except the credential store, which is always current.
    pub(super) fn reduce(&mut self, event: WolServiceEvent) -> WolStep {
        let awaiting = std::mem::take(&mut self.awaiting);
        match (awaiting, event) {
            (awaiting, WolServiceEvent::Credentials(credentials)) => {
                if self.provider != WolProvider::Online {
                    self.credentials = credentials;
                }
                self.awaiting = awaiting;
                WolStep::Stay
            }
            (Awaiting::Api, WolServiceEvent::Api(WolApiStatus::Missing)) => WolStep::Message {
                key: "TXT_APIMISSING",
                yes_no: false,
                then: WolBoxThen::Leave(-2),
            },
            (Awaiting::Api, WolServiceEvent::Api(WolApiStatus::Available)) => {
                // 0x00786390 creates the object once per visit ([0xB77E75]);
                // only then does it reset the WOL globals (0x00780110),
                // including [0xB77AD0], the selected record and the locale
                // table, and set [0xB77DC0] (0x00786485) until a
                // connect clears it.
                if !self.api {
                    self.api = true;
                    self.last_error = None;
                    self.selected_server = 0;
                    self.locales.clear();
                    // 0x0078061B..0x0078065F.
                    self.squad_id = 0;
                    self.squad_own_pending = false;
                    self.connected = false;
                }
                if self.provider != WolProvider::Online {
                    self.requests.push(WolServiceRequest::ListCredentials);
                }
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
            (Awaiting::Connect { .. }, WolServiceEvent::Login(Ok(accepted))) => {
                self.motd = Some(accepted.motd);
                self.authenticated = true;
                // The connect clears [0xB77DC0] (0x007867E6) and the login
                // sink [0xB77DC2] (0x007A559D).
                self.connected = true;
                self.disconnected = false;
                WolStep::Accepted
            }
            (Awaiting::Connect { .. }, WolServiceEvent::Login(Err(code))) => {
                // The login sink sets [0xB77DC0] (0x007A52A0).
                self.connected = false;
                self.last_login_result = Some(code);
                self.last_error = Some(login_error_key(code));
                self.connect_failed()
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
                self.selected_server = list.selected as i32;
                WolStep::ServersReady
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
            Awaiting::Connect { .. } => {
                self.requests.push(WolServiceRequest::CancelWait);
                self.connect_failed()
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

/// The login flow `0x00787010` starts by creating the WOLAPI object.
pub(super) fn begin_flow(state: &mut AppState) {
    let Some(visit) = state.frontend.wol.as_mut() else {
        return;
    };
    visit.begin_login();
    answer_unavailable(state);
}

/// My Information's driver `0x00799D30` starts by creating the WOLAPI
/// object; unlike the login flow it leaves `[0xB77DF8]` alone.
pub(super) fn begin_my_information(state: &mut AppState) {
    let Some(visit) = state.frontend.wol.as_mut() else {
        return;
    };
    visit.ensure_api();
    answer_unavailable(state);
}

/// Queue a request from the login page (credential store changes).
pub(crate) fn request(state: &mut AppState, request: WolServiceRequest) {
    if let Some(visit) = state.frontend.wol.as_mut() {
        visit.requests.push(request);
    }
    answer_unavailable(state);
}

/// `0x007864C0` with `server`: the nickname and password go into the
/// record (+0xE4/+0xEE) and the connect waits behind `0xC4`.
fn connect(state: &mut AppState, server: WolServer, auto: bool, remember_locale: i32) -> WolStep {
    let online = state
        .frontend
        .wol
        .as_ref()
        .is_some_and(|visit| visit.provider == WolProvider::Online);
    let (nickname, password, changed) = if online {
        // The shared native request retains its credential fields for the
        // test service; modern HTTPS admission uses only this username.
        (
            state.frontend.wol_account.nickname.clone(),
            String::new(),
            false,
        )
    } else {
        let globals = &state.frontend.wol_account;
        (
            globals.nickname.clone(),
            globals.password.clone(),
            globals.changed,
        )
    };
    let visit = state.frontend.wol.as_mut().expect("WOL visit");
    visit.requests.push(WolServiceRequest::Connect {
        server: server.clone(),
        nickname,
        password,
        changed,
    });
    visit.awaiting = Awaiting::Connect {
        deadline: Instant::now() + WAIT_LIMIT,
    };
    visit.flow = Some(LoginFlow {
        auto,
        server,
        remember_locale,
        persona: None,
    });
    WolStep::Wait("TXT_CONNECTING_SERVER")
}

/// `0x00787010` past the server list (`0x007870F5..0x00787359`): with
/// `[DefaultPersona] AutoLogin` naming a stored account, connect silently.
fn after_server_list(state: &mut AppState) {
    if state
        .frontend
        .wol
        .as_ref()
        .is_some_and(|visit| visit.provider == WolProvider::Online)
    {
        super::login::open(state);
        return;
    }
    let auto_name = super::persona::info_file(state)
        .map(|file| file.auto_login())
        .unwrap_or_default();
    if !auto_name.is_empty() {
        // 0x00787125..0x00787191: the password clears, then slots 1..32 are
        // searched with _stricmp.
        let stored = state
            .frontend
            .wol
            .as_ref()
            .and_then(|visit| crate::ui::wol_login::lookup(&visit.credentials, &auto_name))
            .cloned();
        let globals = &mut state.frontend.wol_account;
        globals.password.clear();
        match stored {
            None => globals.nickname.clear(),
            Some(stored) => {
                globals.nickname = stored.nickname;
                globals.password = stored.password;
                globals.changed = false;
                let persona = super::persona::load(state, &auto_name);
                let visit = state.frontend.wol.as_mut().expect("WOL visit");
                let index = visit.auto_login_server(&persona.server);
                if index != -1 {
                    visit.selected_server = index;
                    let server = usize::try_from(index)
                        .ok()
                        .filter(|index| *index >= 1)
                        .and_then(|index| visit.servers.get(index).cloned());
                    let step = match server {
                        Some(server) => connect(state, server, true, 0),
                        // 0x00787350: out of range, the box and its flag.
                        None => WolStep::AutoLoginFailed,
                    };
                    apply(state, step);
                    return;
                }
            }
        }
    }
    prepare_login_dialog(state);
    super::login::open(state);
}

/// `0x00787362..0x007873AC`: an empty nickname takes LastValid, then
/// AutoLogin is cleared whatever happened, which creates the file.
fn prepare_login_dialog(state: &mut AppState) {
    let Some(file) = super::persona::info_file(state) else {
        return;
    };
    if state.frontend.wol_account.nickname.is_empty() {
        state.frontend.wol_account.nickname = file.last_valid();
    }
    file.set_auto_login("");
}

/// The silent connect failed: `0x00786EF0` shows `[0xB77AD0]` and its flag
/// is set only for the Yes/No results, which return 0; otherwise `0x10F`
/// opens after the LastValid and AutoLogin steps.
fn auto_login_failed(state: &mut AppState) {
    prepare_login_dialog(state);
    let visit = state.frontend.wol.as_mut().expect("WOL visit");
    let step = match visit.last_error {
        Some(key) => {
            let yes_no = visit.last_login_result.is_some_and(login_error_asks);
            WolStep::Message {
                key,
                yes_no,
                then: if yes_no {
                    WolBoxThen::Welcome
                } else {
                    WolBoxThen::Login
                },
            }
        }
        None => WolStep::OpenLogin,
    };
    apply(state, step);
}

/// Start the connect after `0x10F` closed with Login (`0x007874E2..`):
/// the persona's preferred server picks the record.
pub(super) fn begin_connect(state: &mut AppState) {
    let nickname = state.frontend.wol_account.nickname.clone();
    let online = state
        .frontend
        .wol
        .as_ref()
        .is_some_and(|visit| visit.provider == WolProvider::Online);
    let persona = if online {
        WolPersona::default()
    } else {
        super::persona::load(state, &nickname)
    };
    let Some(visit) = state.frontend.wol.as_mut() else {
        return;
    };
    let step = match visit.dialog_server(&persona.server) {
        Some(server) => connect(state, server, false, persona.locale),
        None => {
            visit.flow = None;
            visit.failed_login_box()
        }
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
        WolServiceEvent::Page { from, text, ansi } => {
            super::lobby::page_received(state, from, text, ansi);
            return;
        }
        WolServiceEvent::PageResult(result) => {
            super::lobby::page_result(state, result);
            return;
        }
        WolServiceEvent::Message {
            from,
            text,
            private,
            action,
            ansi,
            options,
        } => {
            super::lobby::message_received(
                state,
                super::lobby::ReceivedMessage {
                    from,
                    text,
                    private,
                    action,
                    ansi,
                    options,
                },
            );
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
        WolServiceEvent::Ladder(records) => {
            super::buddies::ladder_received(state, records);
            return;
        }
        WolServiceEvent::HighScores(result) => {
            super::lobby::high_scores_received(state, result);
            return;
        }
        WolServiceEvent::Buddies(result) => {
            super::buddies::buddy_list_received(state, result);
            return;
        }
        WolServiceEvent::UserLocales(result) => {
            // Event 14 ends the locale wait (0x00786AB2).
            if super::persona::user_locales(state, result) {
                let visit = state.frontend.wol.as_mut().expect("checked");
                if matches!(visit.awaiting, Awaiting::Locale { .. }) {
                    visit.awaiting = Awaiting::Nothing;
                    apply(state, WolStep::LocaleDecided);
                    answer_unavailable(state);
                }
            }
            state.platform.window.request_redraw();
            return;
        }
        WolServiceEvent::LocaleCount(count) => {
            if super::my_info::page_open(state) {
                super::my_info::locale_count(state, count);
            } else {
                super::persona::locale_count(state, count);
            }
            return;
        }
        WolServiceEvent::Squad { id, squad } => {
            super::lobby::squad(state, id, squad);
            answer_unavailable(state);
            return;
        }
        event @ (WolServiceEvent::Channels { .. }
        | WolServiceEvent::Joined(_)
        | WolServiceEvent::UserJoined(_)
        | WolServiceEvent::UserLeft(_)
        | WolServiceEvent::ChannelLeft
        | WolServiceEvent::PlayerChoices { .. }
        | WolServiceEvent::PlayerReady { .. }
        | WolServiceEvent::MapStatus { .. }
        | WolServiceEvent::PreviewRequested { .. }
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

/// A live-service refusal whose reason no CSF label carries: the box its
/// failure opened shows `body` instead. False when no box is open.
pub(super) fn replace_open_box_body(state: &mut AppState, body: &str) -> bool {
    match state.frontend.wol.as_mut().map(|visit| &mut visit.page) {
        Some(WolPage::Message { message, .. }) => {
            message.body = body.to_owned();
            state.platform.window.request_redraw();
            true
        }
        _ => false,
    }
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
        WolStep::ServersReady => {
            let visit = state.frontend.wol.as_mut().expect("WOL visit");
            if visit.mode == Some(WolMode::MyInformation) {
                // 0x797640 closed 0xC4 itself.
                visit.page = WolPage::Backdrop { wait: None };
                super::my_info::open(state);
            } else {
                after_server_list(state);
            }
        }
        WolStep::OpenLogin => super::login::open(state),
        WolStep::Accepted => connect_succeeded(state),
        WolStep::AutoLoginFailed => auto_login_failed(state),
        WolStep::LocaleDecided => locale_decided(state),
    }
}

/// Login's persona load/save (`0x00786821..0x007868F9`) for native diagnostics.
/// `persona::info_file` excludes modern visits; their nickname still needs
/// validation here, including providers bypassing the HTTP worker.
fn connected_persona(
    file: Option<&WolInfoFile>,
    nickname: &str,
    online: bool,
    server: Option<&str>,
    csf: &dyn Fn(&str) -> String,
) -> anyhow::Result<WolPersona> {
    if online {
        crate::app::frontend::skirmish_session::validate_online_handle(nickname)?;
    }
    let mut persona =
        file.map_or_else(WolPersona::default, |file| file.load_persona(nickname, csf));
    // Modes 4 and 5 retain the connected server and save the named persona.
    if let Some(server) = server {
        persona.set_server(server);
        if let Some(file) = file {
            file.save_persona(&mut persona, nickname, csf);
        }
    }
    Ok(persona)
}

/// `0x007864C0`'s success continuation (`0x00786795..0x00786A48`), up to
/// the locale wait.
fn connect_succeeded(state: &mut AppState) {
    // 0xC4 closes. API+0xD4(0x10010) has no service request.
    if let Some(visit) = state.frontend.wol.as_mut() {
        visit.page = WolPage::Backdrop { wait: None };
    }
    // ReadMultiPlayerSettings 0x006980C0.
    App::prepare_multiplayer_settings(
        state,
        crate::app::shell_main_menu::MultiplayerSettingsContext::Shell,
    );
    let nickname = state.frontend.wol_account.nickname.clone();
    let visit = state.frontend.wol.as_ref().expect("WOL visit");
    let online = visit.provider == WolProvider::Online;
    let server = visit
        .flow
        .as_ref()
        .filter(|_| matches!(visit.mode.map(WolMode::native), Some(4 | 5)))
        .map(|flow| flow.server.clone());
    let file = super::persona::info_file(state);
    let persona = connected_persona(
        file.as_ref(),
        &nickname,
        online,
        server.as_ref().map(WolServer::persona_server),
        &|key| resolve(state, key),
    );
    let Ok(persona) = persona else {
        if let Some(visit) = state.frontend.wol.as_mut() {
            visit.authenticated = false;
            visit.connected = false;
            visit.requests.push(WolServiceRequest::Logout);
        }
        apply(
            state,
            WolStep::Message {
                key: "TXT_CANT_CONNECT",
                yes_no: false,
                then: WolBoxThen::Welcome,
            },
        );
        return;
    };
    // The nickname becomes the session handle [0xA8B380], the persona's
    // Country and Color the MultiPlayer picks, and WriteMultiPlayerSettings
    // 0x006990A0 saves them.
    let runtime = &mut state.frontend.multiplayer_shell_runtime;
    if online {
        runtime
            .accept_online_handle(&nickname)
            .expect("online nickname admitted before persona access");
    } else {
        runtime.set_handle(&nickname);
    }
    if !online {
        runtime.set_side_and_color(persona.country, persona.color);
    }
    runtime.persist_local_preferences();
    if let Some(file) = file {
        file.set_last_valid(&nickname);
    }
    if online {
        state.frontend.wol.as_mut().expect("WOL visit").flow = None;
        finish_login(state);
        return;
    }
    // 0x00786964..0x007869E3: the ladder requests need the ladder service
    // ([0xAC1170]); none is modelled, so none is sent.
    let visit = state.frontend.wol.as_mut().expect("WOL visit");
    visit.requests.push(WolServiceRequest::RequestLocales {
        names: vec![nickname],
    });
    visit.awaiting = Awaiting::Locale {
        deadline: Instant::now() + WAIT_LIMIT,
    };
    if let Some(flow) = visit.flow.as_mut() {
        flow.persona = Some(persona);
    }
    state.platform.window.request_redraw();
}

/// `0x00786B87..0x00786D7D`: the table's locale for this user (zero counts
/// as unknown) against the persona's decides what is sent; with neither,
/// `0xFE` asks.
fn locale_decided(state: &mut AppState) {
    let nickname = state.frontend.wol_account.nickname.clone();
    let Some(visit) = state.frontend.wol.as_mut() else {
        return;
    };
    let known = visit
        .locales
        .get(&nickname.to_ascii_lowercase())
        .copied()
        .filter(|locale| *locale != 0);
    let Some(flow) = visit.flow.as_mut() else {
        return;
    };
    let mut persona = flow.persona.take().unwrap_or_default();
    match known {
        Some(locale) if persona.locale == locale => {}
        Some(locale) => {
            if persona.locale == 0 {
                persona.set_locale(locale, 2);
                super::persona::save(state, &mut persona, &nickname);
            }
            send_locale(state, persona.locale);
        }
        None if persona.locale != 0 => send_locale(state, persona.locale),
        None => {
            flow.persona = Some(persona);
            super::persona::open_locale_page(state);
            return;
        }
    }
    connect_returned(state);
}

/// `0xFE` closed with `locale` (0 for Cancel; a negative result counts as
/// 0, `0x00786C10`): the persona takes it with priority 3 and is saved, and
/// a nonzero one is sent (`0x00786CEF..0x00786D5E`).
pub(super) fn locale_chosen(state: &mut AppState, locale: i32) {
    let nickname = state.frontend.wol_account.nickname.clone();
    let mut persona = state
        .frontend
        .wol
        .as_mut()
        .and_then(|visit| visit.flow.as_mut())
        .and_then(|flow| flow.persona.take())
        .unwrap_or_default();
    persona.set_locale(locale, 3);
    super::persona::save(state, &mut persona, &nickname);
    if locale != 0 {
        send_locale(state, locale);
    }
    connect_returned(state);
}

/// API+0xB8, the locale request again, and the table entry: `0x007AE1F0`
/// removes it and only a removed one is inserted again (`0x007ACBE0`).
fn send_locale(state: &mut AppState, locale: i32) {
    let nickname = state.frontend.wol_account.nickname.clone();
    let Some(visit) = state.frontend.wol.as_mut() else {
        return;
    };
    let key = nickname.to_ascii_lowercase();
    visit.requests.push(WolServiceRequest::SetLocale(locale));
    visit.requests.push(WolServiceRequest::RequestLocales {
        names: vec![nickname],
    });
    if visit.locales.remove(&key).is_some() {
        visit.locales.insert(key, locale);
    }
}

/// `0x007864C0` returned 1: the silent path returns at once (`0x0078733A`);
/// the dialog path stores a remembered account and asks `0x112`
/// (`0x00787619..0x007876E4`).
fn connect_returned(state: &mut AppState) {
    let Some(flow) = state
        .frontend
        .wol
        .as_mut()
        .and_then(|visit| visit.flow.take())
    else {
        return;
    };
    if !flow.auto {
        // Remember with registry bit 1 clear: 0x00789500 stores the account
        // and the settings are written again (0x00787621..0x0078766A).
        let globals = state.frontend.wol_account.clone();
        if globals.remember {
            request(
                state,
                WolServiceRequest::StoreCredential {
                    nickname: globals.nickname,
                    password: globals.password,
                    changed: globals.changed,
                    locale: flow.remember_locale,
                },
            );
            state
                .frontend
                .multiplayer_shell_runtime
                .persist_local_preferences();
        }
        if super::persona::info_file(state).is_some_and(|file| file.ask_auto_login()) {
            super::persona::open_auto_login_query(state);
            return;
        }
    }
    finish_login(state);
}

/// The login flow returned 1. A re-login on runner entry (`0x007879B6`)
/// continues that runner; otherwise `WOL_Main`'s mode-loop pass applies
/// before the runner.
pub(super) fn finish_login(state: &mut AppState) {
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
    if matches!(visit.page, WolPage::Lobby | WolPage::QuickMatch(_)) {
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
                WolBoxThen::Login => WolStep::OpenLogin,
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

/// The connect and locale waits' silent limit; returns its deadline while
/// one runs.
fn poll_deadline(state: &mut AppState) -> Option<Instant> {
    let visit = state.frontend.wol.as_mut()?;
    let deadline = match visit.awaiting {
        Awaiting::Connect { deadline } | Awaiting::Locale { deadline } => deadline,
        _ => return None,
    };
    if Instant::now() < deadline {
        return Some(deadline);
    }
    let step = match std::mem::take(&mut visit.awaiting) {
        // 0x007866E4: no box from the wait itself.
        Awaiting::Connect { .. } => visit.connect_failed(),
        _ => WolStep::LocaleDecided,
    };
    apply(state, step);
    answer_unavailable(state);
    state.platform.window.request_redraw();
    None
}

/// The wait box's reveal timer and the waits' limits; returns the earliest
/// deadline while one runs.
pub(super) fn poll_wait(state: &mut AppState) -> Option<Instant> {
    let limit = poll_deadline(state);
    let reveal = (|| {
        let visit = state.frontend.wol.as_mut()?;
        let WolPage::Backdrop { wait: Some(wait) } = &mut visit.page else {
            return None;
        };
        if wait.poll(Instant::now()) {
            state.platform.window.request_redraw();
            return None;
        }
        (!wait.shown).then(|| wait.reveal_at())
    })();
    [limit, reveal].into_iter().flatten().min()
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
    fn online_admission_never_requests_or_retains_native_credentials() {
        let mut visit = visit();
        visit.provider = WolProvider::Online;
        assert_eq!(
            visit.reduce(WolServiceEvent::Api(WolApiStatus::Available)),
            WolStep::Wait("TXT_FETCHING_SERVLIST")
        );
        assert_eq!(
            visit.take_service_requests(),
            [WolServiceRequest::FetchServers { flags: 0x2900 }]
        );
        assert_eq!(
            visit.reduce(WolServiceEvent::Credentials(vec![WolCredential {
                slot: 1,
                nickname: "Alpha".into(),
                password: "12345678".into(),
            }])),
            WolStep::Stay
        );
        assert!(visit.credentials.is_empty());
        assert!(matches!(visit.awaiting, Awaiting::Servers { .. }));
    }

    #[test]
    fn login_slide_policy_has_only_the_modern_or_native_forms_buttons() {
        use crate::app::frontend::shell_transition::ShellSlideKind;
        use crate::ui::wol_login::{LoginPolicy, WolAccountGlobals, WolLoginState};
        let mut visit = visit();
        let mut globals = WolAccountGlobals::default();
        visit.page = WolPage::Login(Box::new(WolLoginState::open_online(&mut globals)));
        let modern = visit.login().unwrap().policy.page_spec();
        assert_eq!(modern, LoginPolicy::Online.page_spec());
        let column = ShellSlideKind::WolLogin.column(9, Some(modern));
        assert_eq!((column.top_buttons, column.bottom_button), (1, true));
        visit.page = WolPage::Login(Box::new(WolLoginState::open(&mut globals, &[])));
        let native = visit.login().unwrap().policy.page_spec();
        assert_eq!(native, LoginPolicy::Legacy.page_spec());
        let column = ShellSlideKind::WolLogin.column(9, Some(native));
        assert_eq!((column.top_buttons, column.bottom_button), (3, true));
    }

    #[test]
    fn online_login_validates_without_persona_io_and_native_login_still_saves() {
        use crate::app::persistence::wol_profile::WOLINFO_INI;
        let directory = crate::map::source::test_support::TestDirectory::new("login-persona");
        let file = WolInfoFile::in_directory(directory.path());
        let path = directory.path().join(WOLINFO_INI);
        let nickname = "a]\r\n[Ask]\r\nAutoLogin=no\r\n[b";
        let csf = |key: &str| key.to_owned();
        assert!(connected_persona(Some(&file), nickname, true, Some("Online"), &csf).is_err());
        assert!(
            !path.exists(),
            "rejected identity must not create a profile"
        );
        let previous = b"; retained profile\r\n[Ask]\r\nAutoLogin=yes\r\n";
        directory.write(WOLINFO_INI, previous);
        assert!(connected_persona(Some(&file), nickname, true, Some("Online"), &csf).is_err());
        assert_eq!(std::fs::read(&path).unwrap(), previous);

        // The production persona owner supplies no file for modern visits.
        let persona = connected_persona(None, "Player_Alpha", true, Some("Online"), &csf).unwrap();
        assert_eq!(persona.server, "Online");
        assert_eq!(std::fs::read(&path).unwrap(), previous);
        connected_persona(Some(&file), "Player_Alpha", false, Some("Legacy"), &csf).unwrap();
        assert!(file.ask_auto_login());
        assert_eq!(file.load_persona("Player_Alpha", &csf).server, "Legacy");
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
            WolStep::ServersReady
        );
        assert_eq!(visit.dialog_server("").unwrap().name, "irc:Server2");

        let mut visit = visit_waiting_servers();
        assert_eq!(
            visit.reduce(WolServiceEvent::NetStatus(SERVER_STATUS_READY)),
            WolStep::Stay
        );
        assert_eq!(
            visit.reduce(WolServiceEvent::ServerList(Ok(servers(2, 0)))),
            WolStep::ServersReady
        );
        // Record 0 is never the connect target (0x007875ED): the dialog path
        // fails rather than falling back.
        assert_eq!(visit.dialog_server(""), None);
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
        assert_eq!(visit.dialog_server(""), None);
        // [0xB77AD0] is empty after 0x00780110: no box, the welcome again.
        assert_eq!(visit.failed_login_box(), WolStep::Welcome);
    }

    #[test]
    fn login_results_lead_to_the_lobby_or_a_box_then_the_welcome() {
        let mut visit = visit_waiting_servers();
        visit.awaiting = Awaiting::Connect {
            deadline: Instant::now() + WAIT_LIMIT,
        };
        assert_eq!(
            visit.reduce(WolServiceEvent::Login(Err(0x8004_0065))),
            WolStep::Message {
                key: "TXT_BADPASS",
                yes_no: false,
                then: WolBoxThen::Welcome
            }
        );
        assert!(!visit.authenticated);
        visit.awaiting = Awaiting::Connect {
            deadline: Instant::now() + WAIT_LIMIT,
        };
        assert_eq!(
            visit.reduce(WolServiceEvent::Login(Err(0x8004_0076))),
            WolStep::Message {
                key: "TXT_SERIALDUP",
                yes_no: true,
                then: WolBoxThen::Welcome
            }
        );
        visit.awaiting = Awaiting::Connect {
            deadline: Instant::now() + WAIT_LIMIT,
        };
        assert_eq!(visit.disconnect(), visit.failed_login_box());
        visit.awaiting = Awaiting::Connect {
            deadline: Instant::now() + WAIT_LIMIT,
        };
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
    fn the_dialog_path_prefers_the_persona_server_then_falls_back() {
        let mut visit = visit_waiting_servers();
        visit.reduce(WolServiceEvent::ServerList(Ok(servers(4, 2))));
        visit.reduce(WolServiceEvent::NetStatus(SERVER_STATUS_READY));
        // The persona's server matches the text after the first ':'.
        assert_eq!(visit.dialog_server("Server3").unwrap().name, "irc:Server3");
        assert_eq!(visit.selected_server, 3);
        // Record 0 is never matched; an unlisted name keeps [0xB76468].
        assert_eq!(visit.dialog_server("Server0").unwrap().name, "irc:Server3");
        // An index past the list becomes -1, which falls back to record 1.
        visit.selected_server = 9;
        assert_eq!(visit.dialog_server("").unwrap().name, "irc:Server1");
        assert_eq!(visit.selected_server, 1);
    }

    #[test]
    fn the_silent_path_skips_an_unlisted_persona_server() {
        let mut visit = visit_waiting_servers();
        visit.reduce(WolServiceEvent::ServerList(Ok(servers(3, 1))));
        visit.reduce(WolServiceEvent::NetStatus(SERVER_STATUS_READY));
        assert_eq!(visit.auto_login_server(""), 1);
        assert_eq!(visit.auto_login_server("Server2"), 2);
        assert_eq!(visit.auto_login_server("Nowhere"), -1);
        // Quick Match and Quick Co-op keep [0xB76468] instead.
        visit.mode = Some(WolMode::QuickMatch);
        assert_eq!(visit.auto_login_server("Nowhere"), 1);
        assert_eq!(visit.auto_login_server("Server2"), 2);
        let display = WolServer {
            name: "irc:USA East:6667".into(),
        };
        assert_eq!(display.display_name(), Some("USA East:6667"));
        assert_eq!(display.persona_server(), "USA East:6667");
        let bare = WolServer {
            name: "Bare".into(),
        };
        assert_eq!((bare.display_name(), bare.persona_server()), (None, "Bare"));
    }

    #[test]
    fn a_failed_silent_connect_goes_on_to_the_login_dialog() {
        let mut visit = visit_waiting_servers();
        visit.flow = Some(LoginFlow {
            auto: true,
            server: WolServer {
                name: "irc:Server1".into(),
            },
            remember_locale: 0,
            persona: None,
        });
        visit.awaiting = Awaiting::Connect {
            deadline: Instant::now() + WAIT_LIMIT,
        };
        assert_eq!(
            visit.reduce(WolServiceEvent::Login(Err(0x8004_0065))),
            WolStep::AutoLoginFailed
        );
        assert!(visit.flow.is_none());
        // The dialog path shows the box and returns to the welcome.
        visit.awaiting = Awaiting::Connect {
            deadline: Instant::now() + WAIT_LIMIT,
        };
        assert_eq!(
            visit.reduce(WolServiceEvent::Login(Err(0x8004_0065))),
            WolStep::Message {
                key: "TXT_BADPASS",
                yes_no: false,
                then: WolBoxThen::Welcome
            }
        );
    }

    #[test]
    fn a_new_login_flow_forgets_the_last_result() {
        let mut visit = visit_waiting_servers();
        visit.last_error = Some("TXT_BADPASS");
        visit.last_login_result = Some(0x8004_0072);
        visit.begin_login();
        // A timeout leaves no new result: the box is OK-only.
        assert_eq!(
            visit.failed_login_box(),
            WolStep::Message {
                key: "TXT_BADPASS",
                yes_no: false,
                then: WolBoxThen::Welcome
            }
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
